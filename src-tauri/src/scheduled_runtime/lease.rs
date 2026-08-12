use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Utc};
use gold_band::scheduler::db::ScheduledTaskDatabase;
use gold_band::scheduler::occurrence::{LeaseConfig, OccurrenceStatus};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::warn;

pub(crate) struct OccurrenceExecutionGuard {
    cancellation_requested: Arc<AtomicBool>,
    cancel: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

trait OccurrenceLeaseStore: Send + Sync + 'static {
    fn renew_lease(
        &self,
        occurrence_id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> anyhow::Result<bool>;

    fn release_owned_occurrence_for_retry(
        &self,
        occurrence_id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
    ) -> anyhow::Result<bool>;

    fn occurrence_lease_state(
        &self,
        occurrence_id: &str,
        owner_id: &str,
    ) -> anyhow::Result<OccurrenceLeaseState>;
}

impl OccurrenceLeaseStore for ScheduledTaskDatabase {
    fn renew_lease(
        &self,
        occurrence_id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> anyhow::Result<bool> {
        Ok(ScheduledTaskDatabase::renew_lease(
            self,
            occurrence_id,
            owner_id,
            now,
            lease_until,
        )?)
    }

    fn release_owned_occurrence_for_retry(
        &self,
        occurrence_id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
    ) -> anyhow::Result<bool> {
        Ok(ScheduledTaskDatabase::release_owned_occurrence_for_retry(
            self,
            occurrence_id,
            owner_id,
            now,
        )?)
    }

    fn occurrence_lease_state(
        &self,
        occurrence_id: &str,
        owner_id: &str,
    ) -> anyhow::Result<OccurrenceLeaseState> {
        let state = match ScheduledTaskDatabase::get_occurrence(self, occurrence_id)? {
            Some(occurrence) if occurrence.status.is_terminal() => OccurrenceLeaseState::Terminal,
            Some(occurrence)
                if occurrence.status == OccurrenceStatus::Running
                    && occurrence.owner_id.as_deref() == Some(owner_id) =>
            {
                OccurrenceLeaseState::OwnedRunning
            }
            Some(_) => OccurrenceLeaseState::NoLongerOwned,
            None => OccurrenceLeaseState::Missing,
        };
        Ok(state)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OccurrenceLeaseState {
    Terminal,
    OwnedRunning,
    NoLongerOwned,
    Missing,
}

enum HeartbeatAction {
    ContinueRenewing,
    RetryRelease,
    Stop,
    NotifyLeaseLost,
}

impl OccurrenceExecutionGuard {
    pub(crate) fn start<F>(
        database: ScheduledTaskDatabase,
        occurrence_id: String,
        owner_id: String,
        config: LeaseConfig,
        on_lease_lost: F,
    ) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self::start_with_store(database, occurrence_id, owner_id, config, on_lease_lost)
    }

    fn start_with_store<S, F>(
        store: S,
        occurrence_id: String,
        owner_id: String,
        config: LeaseConfig,
        on_lease_lost: F,
    ) -> Self
    where
        S: OccurrenceLeaseStore,
        F: Fn() + Send + Sync + 'static,
    {
        let heartbeat_interval = config
            .heartbeat_interval()
            .to_std()
            .expect("LeaseConfig heartbeat interval must be positive");
        let first_heartbeat_at = tokio::time::Instant::now() + heartbeat_interval;
        Self::start_with_store_at(
            store,
            occurrence_id,
            owner_id,
            config,
            first_heartbeat_at,
            on_lease_lost,
        )
    }

    fn start_with_store_at<S, F>(
        store: S,
        occurrence_id: String,
        owner_id: String,
        config: LeaseConfig,
        first_heartbeat_at: tokio::time::Instant,
        on_lease_lost: F,
    ) -> Self
    where
        S: OccurrenceLeaseStore,
        F: Fn() + Send + Sync + 'static,
    {
        let heartbeat_interval = config
            .heartbeat_interval()
            .to_std()
            .expect("LeaseConfig heartbeat interval must be positive");
        let (cancel, mut canceled) = oneshot::channel();
        let store: Arc<dyn OccurrenceLeaseStore> = Arc::new(store);
        let cancellation_requested = Arc::new(AtomicBool::new(false));
        let cancellation_requested_for_task = cancellation_requested.clone();
        let task = tokio::spawn(async move {
            let mut heartbeat = tokio::time::interval_at(first_heartbeat_at, heartbeat_interval);
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut release_pending = false;

            loop {
                tokio::select! {
                    biased;
                    _ = &mut canceled => break,
                    _ = heartbeat.tick() => {
                        if cancellation_requested_for_task.load(Ordering::Acquire) {
                            break;
                        }
                        let action = heartbeat_step(
                            store.clone(),
                            occurrence_id.clone(),
                            owner_id.clone(),
                            config,
                            release_pending,
                            cancellation_requested_for_task.clone(),
                        )
                        .await;
                        if cancellation_requested_for_task.load(Ordering::Acquire) {
                            break;
                        }
                        match action {
                            HeartbeatAction::ContinueRenewing => {}
                            HeartbeatAction::RetryRelease => release_pending = true,
                            HeartbeatAction::Stop => break,
                            HeartbeatAction::NotifyLeaseLost => {
                                // This CAS is the lease-loss linearization point. Lifecycle
                                // paths publish cancellation before awaiting the guard, so a
                                // later terminal transition cannot race a new callback here.
                                if cancellation_requested_for_task
                                    .compare_exchange(
                                        false,
                                        true,
                                        Ordering::AcqRel,
                                        Ordering::Acquire,
                                    )
                                    .is_ok()
                                {
                                    on_lease_lost();
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

        Self {
            cancellation_requested,
            cancel: Some(cancel),
            task: Some(task),
        }
    }

    pub(crate) fn stop(mut self) -> impl Future<Output = ()> + Send {
        self.request_cancel();
        async move {
            let Some(task) = self.task.take() else {
                return;
            };
            if let Err(error) = task.await {
                warn!(%error, "scheduled occurrence lease guard failed while stopping");
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn is_running(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    fn request_cancel(&mut self) {
        self.cancellation_requested.store(true, Ordering::Release);
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

impl Drop for OccurrenceExecutionGuard {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

async fn heartbeat_step(
    store: Arc<dyn OccurrenceLeaseStore>,
    occurrence_id: String,
    owner_id: String,
    config: LeaseConfig,
    release_pending: bool,
    cancellation_requested: Arc<AtomicBool>,
) -> HeartbeatAction {
    let warning_occurrence_id = occurrence_id.clone();
    match tokio::task::spawn_blocking(move || {
        if release_pending {
            resolve_failed_renewal(
                store.as_ref(),
                &occurrence_id,
                &owner_id,
                cancellation_requested.as_ref(),
            )
        } else {
            heartbeat_action(
                store.as_ref(),
                &occurrence_id,
                &owner_id,
                config,
                cancellation_requested.as_ref(),
            )
        }
    })
    .await
    {
        Ok(action) => action,
        Err(error) => {
            warn!(%error, occurrence_id = %warning_occurrence_id, "scheduled occurrence lease heartbeat worker failed");
            HeartbeatAction::RetryRelease
        }
    }
}

fn heartbeat_action(
    store: &dyn OccurrenceLeaseStore,
    occurrence_id: &str,
    owner_id: &str,
    config: LeaseConfig,
    cancellation_requested: &AtomicBool,
) -> HeartbeatAction {
    if cancellation_requested.load(Ordering::Acquire) {
        return HeartbeatAction::Stop;
    }
    let now = Utc::now();
    let renewed = store.renew_lease(occurrence_id, owner_id, now, config.lease_until(now));
    if cancellation_requested.load(Ordering::Acquire) {
        return HeartbeatAction::Stop;
    }
    match renewed {
        Ok(true) => HeartbeatAction::ContinueRenewing,
        Ok(false) => {
            warn!(%occurrence_id, "scheduled occurrence lease was not renewed");
            resolve_failed_renewal(store, occurrence_id, owner_id, cancellation_requested)
        }
        Err(error) => {
            warn!(%error, %occurrence_id, "scheduled occurrence lease renewal failed");
            resolve_failed_renewal(store, occurrence_id, owner_id, cancellation_requested)
        }
    }
}

fn resolve_failed_renewal(
    store: &dyn OccurrenceLeaseStore,
    occurrence_id: &str,
    owner_id: &str,
    cancellation_requested: &AtomicBool,
) -> HeartbeatAction {
    if cancellation_requested.load(Ordering::Acquire) {
        return HeartbeatAction::Stop;
    }
    let released = store.release_owned_occurrence_for_retry(occurrence_id, owner_id, Utc::now());
    if cancellation_requested.load(Ordering::Acquire) {
        return HeartbeatAction::Stop;
    }
    match released {
        Ok(true) => {
            warn!(%occurrence_id, "scheduled occurrence lease was lost");
            HeartbeatAction::NotifyLeaseLost
        }
        Ok(false) => {
            if cancellation_requested.load(Ordering::Acquire) {
                return HeartbeatAction::Stop;
            }
            match store.occurrence_lease_state(occurrence_id, owner_id) {
                Ok(OccurrenceLeaseState::Terminal) => HeartbeatAction::Stop,
                Ok(OccurrenceLeaseState::OwnedRunning) => HeartbeatAction::RetryRelease,
                Ok(OccurrenceLeaseState::NoLongerOwned) => HeartbeatAction::NotifyLeaseLost,
                Ok(OccurrenceLeaseState::Missing) => HeartbeatAction::Stop,
                Err(error) => {
                    warn!(%error, %occurrence_id, "failed to inspect scheduled occurrence after lease renewal failure");
                    HeartbeatAction::RetryRelease
                }
            }
        }
        Err(error) => {
            warn!(%error, %occurrence_id, "failed to persist scheduled occurrence lease loss");
            HeartbeatAction::RetryRelease
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier, Mutex};
    use std::time::Duration as StdDuration;

    use chrono::{DateTime, Duration, Utc};
    use gold_band::scheduler::db::ScheduledTaskDatabase;
    use gold_band::scheduler::occurrence::{
        ClaimResult, LeaseConfig, OccurrenceStatus, OccurrenceTriggerKind,
    };
    use gold_band::scheduler::{OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition};
    use tempfile::tempdir;
    use tokio::sync::Notify;

    use super::{OccurrenceExecutionGuard, OccurrenceLeaseState, OccurrenceLeaseStore};

    const EXECUTOR_SCHEDULING_TIMEOUT: StdDuration = StdDuration::from_millis(250);

    struct ExecutorProgressWait {
        notify: Arc<Notify>,
        signal: mpsc::Receiver<()>,
        observed: tokio::sync::oneshot::Sender<bool>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum LeaseStoreCall {
        Renew,
        Release,
        Inspect,
    }

    #[derive(Clone)]
    struct ScriptedLeaseStore {
        state: Arc<Mutex<ScriptedLeaseState>>,
    }

    struct ScriptedLeaseState {
        renew_results: VecDeque<anyhow::Result<bool>>,
        release_results: VecDeque<anyhow::Result<bool>>,
        lease_state_results: VecDeque<anyhow::Result<OccurrenceLeaseState>>,
        renew_calls: usize,
        release_calls: usize,
        lease_state_calls: usize,
        renew_started: Option<tokio::sync::oneshot::Sender<()>>,
        renew_barrier: Option<Arc<Barrier>>,
        executor_progress_wait: Option<ExecutorProgressWait>,
        call_observer: Option<tokio::sync::mpsc::UnboundedSender<LeaseStoreCall>>,
    }

    impl ScriptedLeaseStore {
        fn new(
            renew_results: Vec<anyhow::Result<bool>>,
            release_results: Vec<anyhow::Result<bool>>,
            lease_state_results: Vec<anyhow::Result<OccurrenceLeaseState>>,
        ) -> Self {
            Self {
                state: Arc::new(Mutex::new(ScriptedLeaseState {
                    renew_results: renew_results.into(),
                    release_results: release_results.into(),
                    lease_state_results: lease_state_results.into(),
                    renew_calls: 0,
                    release_calls: 0,
                    lease_state_calls: 0,
                    renew_started: None,
                    renew_barrier: None,
                    executor_progress_wait: None,
                    call_observer: None,
                })),
            }
        }

        fn block_next_renewal(
            &self,
            started: tokio::sync::oneshot::Sender<()>,
            barrier: Arc<Barrier>,
        ) {
            let mut state = self.state.lock().unwrap();
            state.renew_started = Some(started);
            state.renew_barrier = Some(barrier);
        }

        fn wait_for_async_executor(
            &self,
            notify: Arc<Notify>,
            signal: mpsc::Receiver<()>,
            observed: tokio::sync::oneshot::Sender<bool>,
        ) {
            self.state.lock().unwrap().executor_progress_wait = Some(ExecutorProgressWait {
                notify,
                signal,
                observed,
            });
        }

        fn observe_calls(&self) -> tokio::sync::mpsc::UnboundedReceiver<LeaseStoreCall> {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            self.state.lock().unwrap().call_observer = Some(sender);
            receiver
        }

        fn call_counts(&self) -> (usize, usize, usize) {
            let state = self.state.lock().unwrap();
            (
                state.renew_calls,
                state.release_calls,
                state.lease_state_calls,
            )
        }
    }

    impl OccurrenceLeaseStore for ScriptedLeaseStore {
        fn renew_lease(
            &self,
            _occurrence_id: &str,
            _owner_id: &str,
            _now: DateTime<Utc>,
            _lease_until: DateTime<Utc>,
        ) -> anyhow::Result<bool> {
            let (result, started, barrier, executor_progress_wait, call_observer) = {
                let mut state = self.state.lock().unwrap();
                state.renew_calls += 1;
                (
                    state.renew_results.pop_front().unwrap_or(Ok(true)),
                    state.renew_started.take(),
                    state.renew_barrier.take(),
                    state.executor_progress_wait.take(),
                    state.call_observer.clone(),
                )
            };
            if let Some(call_observer) = call_observer {
                let _ = call_observer.send(LeaseStoreCall::Renew);
            }
            if let Some(started) = started {
                let _ = started.send(());
            }
            if let Some(executor_progress_wait) = executor_progress_wait {
                executor_progress_wait.notify.notify_one();
                let observed = executor_progress_wait
                    .signal
                    .recv_timeout(EXECUTOR_SCHEDULING_TIMEOUT)
                    .is_ok();
                let _ = executor_progress_wait.observed.send(observed);
            }
            if let Some(barrier) = barrier {
                barrier.wait();
            }
            result
        }

        fn release_owned_occurrence_for_retry(
            &self,
            _occurrence_id: &str,
            _owner_id: &str,
            _now: DateTime<Utc>,
        ) -> anyhow::Result<bool> {
            let (result, call_observer) = {
                let mut state = self.state.lock().unwrap();
                state.release_calls += 1;
                (
                    state.release_results.pop_front().unwrap_or(Ok(true)),
                    state.call_observer.clone(),
                )
            };
            if let Some(call_observer) = call_observer {
                let _ = call_observer.send(LeaseStoreCall::Release);
            }
            result
        }

        fn occurrence_lease_state(
            &self,
            _occurrence_id: &str,
            _owner_id: &str,
        ) -> anyhow::Result<OccurrenceLeaseState> {
            let (result, call_observer) = {
                let mut state = self.state.lock().unwrap();
                state.lease_state_calls += 1;
                (
                    state
                        .lease_state_results
                        .pop_front()
                        .unwrap_or(Ok(OccurrenceLeaseState::NoLongerOwned)),
                    state.call_observer.clone(),
                )
            };
            if let Some(call_observer) = call_observer {
                let _ = call_observer.send(LeaseStoreCall::Inspect);
            }
            result
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_guard_stops_future_heartbeats() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "drop-guard-job",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let owner_id = "lease-guard-owner";
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, owner_id, now, now + Duration::hours(1),)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let before = database.get_occurrence(&occurrence.id).unwrap().unwrap();

        let guard = OccurrenceExecutionGuard::start(
            database.clone(),
            occurrence.id.clone(),
            owner_id.to_string(),
            gold_band::scheduler::occurrence::LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 1,
            },
            || {},
        );
        drop(guard);

        tokio::time::advance(StdDuration::from_secs(1)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        let after = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(after.heartbeat_at, before.heartbeat_at);
        assert_eq!(after.lease_until, before.lease_until);
    }

    #[tokio::test(start_paused = true)]
    async fn attention_required_occurrence_does_not_notify_lease_loss() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "terminal-guard-job",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let owner_id = "lease-guard-owner";
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, owner_id, now, now + Duration::hours(1),)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        assert!(
            database
                .finish_occurrence(
                    &occurrence.id,
                    owner_id,
                    OccurrenceStatus::AttentionRequired,
                    None,
                    None,
                )
                .unwrap()
        );
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_for_guard = lost.clone();
        let _guard = OccurrenceExecutionGuard::start(
            database.clone(),
            occurrence.id.clone(),
            owner_id.to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 1,
            },
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
            },
        );

        tokio::time::advance(StdDuration::from_secs(2)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        let occurrence = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(lost.load(Ordering::SeqCst), 0);
        assert_eq!(occurrence.status, OccurrenceStatus::AttentionRequired);
    }

    #[tokio::test(start_paused = true)]
    async fn missing_occurrence_stops_without_notifying_lease_loss() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_for_guard = lost.clone();
        let _guard = OccurrenceExecutionGuard::start(
            database,
            "missing-occurrence".to_string(),
            "lease-guard-owner".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 1,
            },
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
            },
        );

        tokio::time::advance(StdDuration::from_secs(1)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        assert_eq!(lost.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn release_error_retries_before_notifying_lease_loss() {
        let store = ScriptedLeaseStore::new(
            vec![Err(anyhow::anyhow!("renew unavailable")), Ok(true)],
            vec![Err(anyhow::anyhow!("release unavailable")), Ok(true)],
            Vec::new(),
        );
        let mut calls = store.observe_calls();
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_notify = Arc::new(Notify::new());
        let lost_for_guard = lost.clone();
        let lost_notify_for_guard = lost_notify.clone();
        let _guard = OccurrenceExecutionGuard::start_with_store(
            store.clone(),
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 1,
            },
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
                lost_notify_for_guard.notify_one();
            },
        );

        tokio::time::advance(StdDuration::from_secs(1)).await;
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Renew));
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Release));
        assert_eq!(lost.load(Ordering::SeqCst), 0);
        assert_eq!(store.call_counts(), (1, 1, 0));

        tokio::time::advance(StdDuration::from_secs(1)).await;
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Release));
        lost_notify.notified().await;
        assert_eq!(lost.load(Ordering::SeqCst), 1);
        assert_eq!(store.call_counts(), (1, 2, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn unreleased_owned_occurrence_retries_without_notifying_lease_loss() {
        let store = ScriptedLeaseStore::new(
            vec![Err(anyhow::anyhow!("renew unavailable")), Ok(true)],
            vec![Ok(false), Ok(true)],
            vec![Ok(OccurrenceLeaseState::OwnedRunning)],
        );
        let mut calls = store.observe_calls();
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_notify = Arc::new(Notify::new());
        let lost_for_guard = lost.clone();
        let lost_notify_for_guard = lost_notify.clone();
        let _guard = OccurrenceExecutionGuard::start_with_store(
            store.clone(),
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 1,
            },
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
                lost_notify_for_guard.notify_one();
            },
        );

        tokio::time::advance(StdDuration::from_secs(1)).await;
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Renew));
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Release));
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Inspect));
        assert_eq!(lost.load(Ordering::SeqCst), 0);
        assert_eq!(store.call_counts(), (1, 1, 1));

        tokio::time::advance(StdDuration::from_secs(1)).await;
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Release));
        lost_notify.notified().await;
        assert_eq!(lost.load(Ordering::SeqCst), 1);
        assert_eq!(store.call_counts(), (1, 2, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn blocking_heartbeat_keeps_async_executor_schedulable() {
        let store = ScriptedLeaseStore::new(vec![Ok(true)], Vec::new(), Vec::new());
        let executor_notify = Arc::new(Notify::new());
        let executor_notify_for_task = executor_notify.clone();
        let (executor_signal, executor_progress) = mpsc::channel();
        let (responder_armed, responder_is_armed) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let notified = executor_notify_for_task.notified();
            tokio::pin!(notified);
            let _ = responder_armed.send(());
            notified.await;
            let _ = executor_signal.send(());
        });
        responder_is_armed.await.unwrap();

        let (observed, executor_was_schedulable) = tokio::sync::oneshot::channel();
        store.wait_for_async_executor(executor_notify, executor_progress, observed);
        let guard = OccurrenceExecutionGuard::start_with_store_at(
            store,
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 60,
            },
            tokio::time::Instant::now(),
            || {},
        );

        assert!(executor_was_schedulable.await.unwrap());
        guard.stop().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn stop_requests_cancellation_before_returned_future_is_polled() {
        let store = ScriptedLeaseStore::new(vec![Ok(false)], vec![Ok(true)], Vec::new());
        let (renew_started, renewal_started) = tokio::sync::oneshot::channel();
        let renew_barrier = Arc::new(Barrier::new(2));
        store.block_next_renewal(renew_started, renew_barrier.clone());
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_for_guard = lost.clone();
        let guard = OccurrenceExecutionGuard::start_with_store_at(
            store.clone(),
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 60,
            },
            tokio::time::Instant::now(),
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
            },
        );
        renewal_started.await.unwrap();

        let cancellation_requested = guard.cancellation_requested.clone();
        let stop = guard.stop();
        let cancellation_was_requested = cancellation_requested.load(Ordering::Acquire);

        renew_barrier.wait();
        stop.await;

        assert!(
            cancellation_was_requested,
            "stop() must request cancellation before its returned future is polled"
        );
        assert_eq!(lost.load(Ordering::SeqCst), 0);
        assert_eq!(store.call_counts(), (1, 0, 0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn stop_waits_for_in_flight_renewal_without_notifying_lease_loss() {
        let store = ScriptedLeaseStore::new(vec![Ok(false)], vec![Ok(true)], Vec::new());
        let (renew_started, renewal_started) = tokio::sync::oneshot::channel();
        let renew_barrier = Arc::new(Barrier::new(2));
        store.block_next_renewal(renew_started, renew_barrier.clone());
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_for_guard = lost.clone();
        let guard = OccurrenceExecutionGuard::start_with_store_at(
            store.clone(),
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 60,
            },
            tokio::time::Instant::now(),
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
            },
        );
        renewal_started.await.unwrap();

        let stop_task = tokio::spawn(guard.stop());
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!stop_task.is_finished());

        renew_barrier.wait();
        stop_task.await.unwrap();
        assert_eq!(lost.load(Ordering::SeqCst), 0);
        assert_eq!(store.call_counts(), (1, 0, 0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn dropping_guard_during_in_flight_heartbeat_does_not_notify_lease_loss() {
        let store = ScriptedLeaseStore::new(vec![Ok(false)], vec![Ok(true)], Vec::new());
        let (renew_started, renewal_started) = tokio::sync::oneshot::channel();
        let renew_barrier = Arc::new(Barrier::new(2));
        store.block_next_renewal(renew_started, renew_barrier.clone());
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_for_guard = lost.clone();
        let guard = OccurrenceExecutionGuard::start_with_store_at(
            store.clone(),
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 60,
            },
            tokio::time::Instant::now(),
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
            },
        );
        renewal_started.await.unwrap();

        drop(guard);
        renew_barrier.wait();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        assert_eq!(lost.load(Ordering::SeqCst), 0);
        assert_eq!(store.call_counts(), (1, 0, 0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn cancellation_is_checked_before_release_after_in_flight_renewal() {
        let store = ScriptedLeaseStore::new(vec![Ok(false)], vec![Ok(true)], Vec::new());
        let (renew_started, renewal_started) = tokio::sync::oneshot::channel();
        let renew_barrier = Arc::new(Barrier::new(2));
        store.block_next_renewal(renew_started, renew_barrier.clone());
        let guard = OccurrenceExecutionGuard::start_with_store_at(
            store.clone(),
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 60,
            },
            tokio::time::Instant::now(),
            || {},
        );
        renewal_started.await.unwrap();

        let stop = guard.stop();
        renew_barrier.wait();
        stop.await;

        assert_eq!(store.call_counts(), (1, 0, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn normalized_non_default_heartbeat_drives_guard_before_lease_expiry() {
        let store = ScriptedLeaseStore::new(vec![Ok(false)], vec![Ok(true)], Vec::new());
        let mut calls = store.observe_calls();
        let guard = OccurrenceExecutionGuard::start_with_store(
            store,
            "occurrence-a".to_string(),
            "owner-a".to_string(),
            LeaseConfig {
                lease_seconds: 3,
                heartbeat_seconds: 3,
            },
            || {},
        );

        tokio::time::advance(StdDuration::from_secs(1)).await;
        assert!(calls.is_empty());

        tokio::time::advance(StdDuration::from_secs(1)).await;
        assert_eq!(calls.recv().await, Some(LeaseStoreCall::Renew));
        guard.stop().await;
    }
}
