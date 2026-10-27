use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::{
    IM_LIFECYCLE_QUEUE_CAPACITY, ImChannelKind, ImConnector, ImDelivery, ImDeliveryPayload,
    ImDeliveryReceipt, ImErrorCode, ImIntegrationError, ImMessageUpdateMode, ImRepository,
    ImRepositoryError, MAX_IM_DUE_BATCH,
};

pub const DEFAULT_IM_SEND_CONCURRENCY: usize = 4;
pub const DEFAULT_IM_LEASE_DURATION: Duration = Duration::from_secs(60);
pub const DEFAULT_IM_RETRY_DELAY: Duration = Duration::from_secs(5);

enum DeliveryAttempt {
    Sent(ImDeliveryReceipt),
    AwaitingSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImWorkerSignal {
    DeliveryAvailable,
}

pub fn im_worker_channel() -> (mpsc::Sender<ImWorkerSignal>, mpsc::Receiver<ImWorkerSignal>) {
    mpsc::channel(IM_LIFECYCLE_QUEUE_CAPACITY)
}

pub struct ImDeliveryWorker {
    channel: ImChannelKind,
    repository: Arc<ImRepository>,
    connector: Arc<dyn ImConnector>,
    concurrency: usize,
    lease_duration: Duration,
}

impl ImDeliveryWorker {
    pub fn new(
        channel: ImChannelKind,
        repository: Arc<ImRepository>,
        connector: Arc<dyn ImConnector>,
    ) -> Self {
        Self {
            channel,
            repository,
            connector,
            concurrency: DEFAULT_IM_SEND_CONCURRENCY,
            lease_duration: DEFAULT_IM_LEASE_DURATION,
        }
    }

    #[cfg(test)]
    fn with_limits(mut self, concurrency: usize, lease_duration: Duration) -> Self {
        self.concurrency = concurrency.max(1);
        self.lease_duration = lease_duration;
        self
    }

    pub async fn run(
        self,
        mut signals: mpsc::Receiver<ImWorkerSignal>,
        cancellation: CancellationToken,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                signal = signals.recv() => match signal {
                    None => break,
                    Some(ImWorkerSignal::DeliveryAvailable) => {
                        if !self.dispatch_or_cancel(now_millis(), &cancellation).await {
                            break;
                        }
                    }
                },
                _ = interval.tick() => {
                    if !self.dispatch_or_cancel(now_millis(), &cancellation).await {
                        break;
                    }
                }
            }
        }
    }

    async fn dispatch_or_cancel(&self, now_ms: i64, cancellation: &CancellationToken) -> bool {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => false,
            _ = self.dispatch_once(now_ms) => true,
        }
    }

    pub async fn dispatch_once(&self, now_ms: i64) -> Result<usize, ImRepositoryError> {
        let repository = Arc::clone(&self.repository);
        let channel = self.channel;
        let lease_duration = self.lease_duration;
        let deliveries = tokio::task::spawn_blocking(move || {
            repository.claim_due(channel, now_ms, lease_duration, MAX_IM_DUE_BATCH)
        })
        .await
        .map_err(|_| ImRepositoryError::LockUnavailable)??;
        let count = deliveries.len();
        let mut pending = deliveries.into_iter();
        let mut sends = JoinSet::new();
        for _ in 0..self.concurrency {
            let Some(claimed) = pending.next() else {
                break;
            };
            spawn_send(
                &mut sends,
                Arc::clone(&self.connector),
                Arc::clone(&self.repository),
                claimed,
            );
        }
        while let Some(result) = sends.join_next().await {
            if let Ok((claimed, send_result)) = result {
                self.settle_send(claimed, send_result, now_ms).await?;
            }
            if let Some(claimed) = pending.next() {
                spawn_send(
                    &mut sends,
                    Arc::clone(&self.connector),
                    Arc::clone(&self.repository),
                    claimed,
                );
            }
        }
        Ok(count)
    }

    async fn settle_send(
        &self,
        claimed: super::ClaimedImDelivery,
        result: Result<DeliveryAttempt, super::ImIntegrationError>,
        now_ms: i64,
    ) -> Result<(), ImRepositoryError> {
        let repository = Arc::clone(&self.repository);
        let delivery = claimed.delivery;
        let expected_attempt_count = claimed.attempt_count;
        match result {
            Ok(DeliveryAttempt::AwaitingSource) => {
                tokio::task::spawn_blocking(move || {
                    repository.defer_delivery(
                        &delivery.delivery_id,
                        expected_attempt_count,
                        now_ms.saturating_add(duration_millis(DEFAULT_IM_RETRY_DELAY)),
                        now_ms,
                    )
                })
                .await
                .map_err(|_| ImRepositoryError::LockUnavailable)??;
            }
            Ok(DeliveryAttempt::Sent(receipt)) => {
                let delivery_id = delivery.delivery_id;
                let channel = delivery.channel;
                let binding = receipt.into_binding(channel, delivery_id.clone());
                tokio::task::spawn_blocking(move || {
                    repository.mark_sent(
                        &delivery_id,
                        channel,
                        expected_attempt_count,
                        binding.as_ref(),
                        now_ms,
                    )
                })
                .await
                .map_err(|_| ImRepositoryError::LockUnavailable)??;
            }
            Err(error) => {
                let retry_at_ms = error.retryable.then(|| {
                    now_ms.saturating_add(duration_millis(
                        error.retry_after.unwrap_or(DEFAULT_IM_RETRY_DELAY),
                    ))
                });
                let delivery_id = delivery.delivery_id;
                let error_code = error.code.as_str();
                tracing::warn!(
                    channel = delivery.channel.as_str(),
                    delivery_id,
                    error_code,
                    platform_code = ?error.platform_code,
                    "IM delivery failed"
                );
                tokio::task::spawn_blocking(move || {
                    repository.mark_failed(
                        &delivery_id,
                        expected_attempt_count,
                        retry_at_ms,
                        error_code,
                        now_ms,
                    )
                })
                .await
                .map_err(|_| ImRepositoryError::LockUnavailable)??;
            }
        }
        Ok(())
    }
}

fn spawn_send(
    sends: &mut JoinSet<(
        super::ClaimedImDelivery,
        Result<DeliveryAttempt, super::ImIntegrationError>,
    )>,
    connector: Arc<dyn ImConnector>,
    repository: Arc<ImRepository>,
    claimed: super::ClaimedImDelivery,
) {
    sends.spawn(async move {
        let result = deliver(connector.as_ref(), repository, claimed.delivery.clone()).await;
        (claimed, result)
    });
}

/// Resolution deliveries on `Anytime` channels update the source card in place;
/// every other delivery, and an update that can no longer succeed (message
/// deleted, update window closed), is sent as a new message.
async fn deliver(
    connector: &dyn ImConnector,
    repository: Arc<ImRepository>,
    delivery: ImDelivery,
) -> Result<DeliveryAttempt, ImIntegrationError> {
    let source = if connector.capabilities().message_update == ImMessageUpdateMode::Anytime {
        resolution_source(repository.clone(), &delivery).await?
    } else {
        None
    };
    let binding = if let Some(source) = source.as_ref() {
        let source_id = source.delivery_id.clone();
        let repository_for_state = Arc::clone(&repository);
        // Read lifecycle before binding: pending/sending may still acquire a
        // message ID, including after retries or lease recovery. Terminal
        // states never return to sending, and mark_sent stores the ID atomically.
        let state =
            tokio::task::spawn_blocking(move || repository_for_state.delivery_state(&source_id))
                .await
                .ok()
                .and_then(Result::ok)
                .ok_or_else(|| {
                    ImIntegrationError::retryable(ImErrorCode::StorageUnavailable, None)
                })?;
        if matches!(
            state,
            Some(super::ImDeliveryState::Pending | super::ImDeliveryState::Sending)
        ) {
            return Ok(DeliveryAttempt::AwaitingSource);
        }
        source_binding(repository, &delivery).await?
    } else {
        None
    };
    if let Some(binding) = binding {
        match connector
            .update_message(&binding, &delivery.payload, source.as_ref())
            .await
        {
            Ok(()) => {
                return Ok(DeliveryAttempt::Sent(ImDeliveryReceipt {
                    platform_message_id: None,
                    platform_chat_id: binding.platform_chat_id,
                    update_token_ref: None,
                }));
            }
            Err(error)
                if !error.retryable && error.code == ImErrorCode::MessageUpdateUnavailable =>
            {
                tracing::warn!(
                    channel = delivery.channel.as_str(),
                    delivery_id = delivery.delivery_id,
                    error_code = error.code.as_str(),
                    platform_code = ?error.platform_code,
                    "IM message update unavailable; sending the resolution as a new message"
                )
            }
            Err(error) => return Err(error),
        }
    }
    connector
        .send(delivery, source.as_ref())
        .await
        .map(DeliveryAttempt::Sent)
}

async fn source_binding(
    repository: Arc<ImRepository>,
    delivery: &ImDelivery,
) -> Result<Option<super::ImDeliveryBinding>, ImIntegrationError> {
    let ImDeliveryPayload::InterventionResolution { resolution, .. } = &delivery.payload else {
        return Ok(None);
    };
    let source_delivery_id = ImDelivery::deterministic_id(
        delivery.channel,
        &delivery.destination.destination_id,
        delivery.notification_kind,
        &resolution.source_event_id,
    );
    tokio::task::spawn_blocking(move || repository.binding(&source_delivery_id))
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or(ImIntegrationError::retryable(
            ImErrorCode::StorageUnavailable,
            None,
        ))
}

async fn resolution_source(
    repository: Arc<ImRepository>,
    delivery: &ImDelivery,
) -> Result<Option<ImDelivery>, ImIntegrationError> {
    let ImDeliveryPayload::InterventionResolution { resolution, .. } = &delivery.payload else {
        return Ok(None);
    };
    let source_delivery_id = ImDelivery::deterministic_id(
        delivery.channel,
        &delivery.destination.destination_id,
        delivery.notification_kind,
        &resolution.source_event_id,
    );
    match tokio::task::spawn_blocking(move || repository.delivery(&source_delivery_id)).await {
        Ok(Ok(delivery)) => Ok(delivery),
        _ => Err(ImIntegrationError::retryable(
            ImErrorCode::StorageUnavailable,
            None,
        )),
    }
}

fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    use super::*;
    use crate::im::{
        IM_PAYLOAD_VERSION, ImActionResponse, ImActionResponseContext, ImChannelCapabilities,
        ImDelivery, ImDeliveryBinding, ImDeliveryPayload, ImDeliveryReceipt, ImDestination,
        ImIntegrationError, ImMessageUpdateMode, ImNavigationLocator, ImNotificationKind,
        InformationalNotification, ResolvedImChannelConfig,
    };

    struct FakeConnector {
        message_update: ImMessageUpdateMode,
        update_error: Option<ImIntegrationError>,
        sends: AtomicUsize,
        updated_message_ids: std::sync::Mutex<Vec<String>>,
        sent_source_ids: std::sync::Mutex<Vec<String>>,
    }

    impl FakeConnector {
        fn new(message_update: ImMessageUpdateMode) -> Self {
            Self {
                message_update,
                update_error: None,
                sends: AtomicUsize::new(0),
                updated_message_ids: std::sync::Mutex::new(Vec::new()),
                sent_source_ids: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    struct BlockingConnector {
        started: tokio::sync::Notify,
    }

    #[async_trait]
    impl ImConnector for BlockingConnector {
        fn kind(&self) -> ImChannelKind {
            ImChannelKind::WeCom
        }

        fn capabilities(&self) -> ImChannelCapabilities {
            crate::im::connector::test_support::capabilities(ImMessageUpdateMode::CallbackOnly)
        }

        fn advance_generation(&self, _generation: u64) {}

        async fn connect(
            &self,
            _config: ResolvedImChannelConfig,
            _generation: u64,
            _events: mpsc::Sender<super::super::ImConnectorEvent>,
            _cancellation: CancellationToken,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }

        async fn send(
            &self,
            _delivery: ImDelivery,
            _source: Option<&ImDelivery>,
        ) -> Result<ImDeliveryReceipt, ImIntegrationError> {
            self.started.notify_one();
            pending().await
        }

        async fn respond_to_action(
            &self,
            _context: ImActionResponseContext,
            _response: ImActionResponse,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }

        async fn update_message(
            &self,
            _binding: &ImDeliveryBinding,
            _payload: &ImDeliveryPayload,
            _source: Option<&ImDelivery>,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ImConnector for FakeConnector {
        fn kind(&self) -> ImChannelKind {
            ImChannelKind::WeCom
        }

        fn capabilities(&self) -> ImChannelCapabilities {
            crate::im::connector::test_support::capabilities(self.message_update)
        }

        fn advance_generation(&self, _generation: u64) {}

        async fn connect(
            &self,
            _config: ResolvedImChannelConfig,
            _generation: u64,
            _events: mpsc::Sender<super::super::ImConnectorEvent>,
            _cancellation: CancellationToken,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }

        async fn send(
            &self,
            delivery: ImDelivery,
            source: Option<&ImDelivery>,
        ) -> Result<ImDeliveryReceipt, ImIntegrationError> {
            if let Some(source) = source {
                self.sent_source_ids
                    .lock()
                    .unwrap()
                    .push(source.delivery_id.clone());
            }
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(ImDeliveryReceipt {
                platform_message_id: Some(format!("message-{}", delivery.delivery_id)),
                platform_chat_id: delivery.destination.conversation_id,
                update_token_ref: None,
            })
        }

        async fn respond_to_action(
            &self,
            _context: ImActionResponseContext,
            _response: ImActionResponse,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }

        async fn update_message(
            &self,
            binding: &ImDeliveryBinding,
            _payload: &ImDeliveryPayload,
            _source: Option<&ImDelivery>,
        ) -> Result<(), ImIntegrationError> {
            if let Some(error) = &self.update_error {
                return Err(error.clone());
            }
            self.updated_message_ids
                .lock()
                .unwrap()
                .push(binding.platform_message_id.clone());
            Ok(())
        }
    }

    fn permission_source() -> ImDelivery {
        use crate::app::intervention::{
            InterventionAllowedAction, InterventionLocator, InterventionRequestIdentity,
            PermissionActionKind,
        };
        let kind = ImNotificationKind::Permission;
        ImDelivery {
            delivery_id: ImDelivery::deterministic_id(
                ImChannelKind::WeCom,
                "user-1",
                kind,
                "permission-event-1",
            ),
            channel: ImChannelKind::WeCom,
            destination: ImDestination {
                destination_id: "user-1".into(),
                conversation_id: "chat-1".into(),
                authorized_actor_id: "actor-1".into(),
            },
            notification_kind: kind,
            canonical_event_id: "permission-event-1".into(),
            payload: ImDeliveryPayload::Intervention {
                version: IM_PAYLOAD_VERSION,
                reference: crate::im::InterventionRef {
                    locator: InterventionLocator {
                        project_id: "project-1".into(),
                        task_id: "task-1".into(),
                        run_id: "run-1".into(),
                        round_id: "round-1".into(),
                        node_id: "node-1".into(),
                        attempt_id: "attempt-1".into(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    request: InterventionRequestIdentity::Permission {
                        request_id: "permission-1".into(),
                    },
                    expected_state: "state-1".into(),
                    allowed_actions: vec![InterventionAllowedAction::PermissionOption {
                        option_id: "allow_once".into(),
                        name: "Allow once".into(),
                        permission_kind: PermissionActionKind::AllowOnce,
                    }],
                    expires_at_ms: None,
                },
                presentation: crate::im::InterventionPresentation {
                    title_key: "im.notification.permission.title".into(),
                    summary_key: "im.notification.permission.summary".into(),
                    title: None,
                    summary: None,
                    body: None,
                    context: None,
                    fields: Default::default(),
                    questions: Vec::new(),
                },
            },
            expires_at_ms: i64::MAX,
            display_ref: None,
        }
    }

    fn resolution_for(source: &ImDelivery) -> ImDelivery {
        let canonical_event_id = crate::im::resolution_event_id(&source.canonical_event_id);
        ImDelivery {
            delivery_id: ImDelivery::deterministic_id(
                source.channel,
                &source.destination.destination_id,
                source.notification_kind,
                &canonical_event_id,
            ),
            channel: source.channel,
            destination: source.destination.clone(),
            notification_kind: source.notification_kind,
            canonical_event_id: canonical_event_id.clone(),
            payload: ImDeliveryPayload::InterventionResolution {
                version: IM_PAYLOAD_VERSION,
                resolution: crate::im::InterventionResolutionNotification {
                    canonical_event_id,
                    source_event_id: source.canonical_event_id.clone(),
                    notification_kind: source.notification_kind,
                    locator: ImNavigationLocator {
                        project_id: "project-1".into(),
                        task_id: None,
                        run_id: None,
                        round_id: None,
                        node_id: None,
                        attempt_id: None,
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    display_ref: None,
                    resolved_by: crate::im::ImResolvedBy::Desktop,
                    outcome: crate::im::ImResolutionOutcome::PermissionOption {
                        option_id: "allow_once".into(),
                    },
                },
            },
            expires_at_ms: i64::MAX,
            display_ref: None,
        }
    }

    /// Sends the source with a plain connector, then dispatches its resolution
    /// through `connector`.
    async fn dispatch_resolution(connector: Arc<FakeConnector>) -> (TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let source = permission_source();
        repository.enqueue(&source, 100).unwrap();
        let sender = ImDeliveryWorker::new(
            ImChannelKind::WeCom,
            Arc::clone(&repository),
            Arc::new(FakeConnector::new(ImMessageUpdateMode::CallbackOnly)),
        );
        assert_eq!(sender.dispatch_once(100).await.unwrap(), 1);
        repository
            .enqueue_resolution_and_expire_pending_source(
                &source.canonical_event_id,
                &resolution_for(&source),
                crate::im::ImResolutionDispatch::Send,
                101,
            )
            .unwrap();
        let worker =
            ImDeliveryWorker::new(ImChannelKind::WeCom, Arc::clone(&repository), connector);
        assert_eq!(worker.dispatch_once(102).await.unwrap(), 1);
        assert!(
            repository
                .claim_due(ImChannelKind::WeCom, 103, Duration::from_secs(30), 1)
                .unwrap()
                .is_empty()
        );
        (temp, format!("message-{}", source.delivery_id))
    }

    #[tokio::test]
    async fn resolution_of_a_failed_inflight_source_is_sent_on_its_own_without_resending_the_card()
    {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let source = permission_source();
        repository.enqueue(&source, 100).unwrap();
        let claims = repository
            .claim_due(
                source.channel,
                100,
                Duration::from_secs(60),
                MAX_IM_DUE_BATCH,
            )
            .unwrap();
        let source_claim = claims
            .iter()
            .find(|c| c.delivery.delivery_id == source.delivery_id)
            .unwrap();
        repository
            .enqueue_resolution_and_expire_pending_source(
                &source.canonical_event_id,
                &resolution_for(&source),
                crate::im::ImResolutionDispatch::Send,
                101,
            )
            .unwrap();
        let connector = Arc::new(FakeConnector::new(ImMessageUpdateMode::Anytime));
        let worker = ImDeliveryWorker::new(source.channel, repository.clone(), connector.clone());

        assert_eq!(worker.dispatch_once(102).await.unwrap(), 1);
        assert_eq!(
            connector.sends.load(Ordering::SeqCst),
            0,
            "resolution must not fall back while the source send is in flight"
        );
        repository
            .mark_failed(
                &source.delivery_id,
                source_claim.attempt_count,
                Some(20_000),
                "IM_CONNECTION_UNAVAILABLE",
                103,
            )
            .unwrap();
        // The failed source is already resolved, so it expires instead of
        // retrying, and the resolution no longer has a card to update.
        assert_eq!(worker.dispatch_once(5_102).await.unwrap(), 1);
        assert_eq!(connector.sends.load(Ordering::SeqCst), 1);
        assert!(connector.updated_message_ids.lock().unwrap().is_empty());
        assert!(
            repository
                .claim_due(
                    source.channel,
                    20_000,
                    Duration::from_secs(60),
                    MAX_IM_DUE_BATCH,
                )
                .unwrap()
                .is_empty(),
            "an actionable card is never resent for a resolved request"
        );
        assert_eq!(worker.dispatch_once(90_000).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn resolution_of_a_source_with_a_lost_lease_is_sent_on_its_own() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let source = permission_source();
        repository.enqueue(&source, 100).unwrap();
        repository
            .claim_due(source.channel, 100, Duration::from_millis(10), 1)
            .unwrap();
        repository
            .enqueue_resolution_and_expire_pending_source(
                &source.canonical_event_id,
                &resolution_for(&source),
                crate::im::ImResolutionDispatch::Send,
                101,
            )
            .unwrap();
        let connector = Arc::new(FakeConnector::new(ImMessageUpdateMode::Anytime));
        let worker = ImDeliveryWorker::new(source.channel, repository.clone(), connector.clone());
        assert_eq!(worker.dispatch_once(102).await.unwrap(), 1);
        assert_eq!(connector.sends.load(Ordering::SeqCst), 0);
        assert_eq!(
            repository
                .recover_expired_leases(110, MAX_IM_DUE_BATCH)
                .unwrap(),
            1
        );
        worker.dispatch_once(5_102).await.unwrap();
        worker.dispatch_once(10_102).await.unwrap();
        assert_eq!(
            connector.sends.load(Ordering::SeqCst),
            1,
            "only the resolution is sent; the resolved source expired"
        );
        assert!(connector.updated_message_ids.lock().unwrap().is_empty());
        assert_eq!(worker.dispatch_once(20_000).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn resolution_falls_back_when_source_cannot_acquire_a_binding() {
        for state in [
            None,
            Some(super::super::ImDeliveryState::Expired),
            Some(super::super::ImDeliveryState::DeadLetter),
            Some(super::super::ImDeliveryState::Sent),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let repository = Arc::new(ImRepository::new(
                Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
            ));
            let source = permission_source();
            if let Some(state) = state {
                repository.enqueue(&source, 100).unwrap();
                if state != super::super::ImDeliveryState::Expired {
                    let claim = repository
                        .claim_due(source.channel, 100, Duration::from_secs(60), 1)
                        .unwrap()
                        .remove(0);
                    if state == super::super::ImDeliveryState::Sent {
                        repository
                            .mark_sent(
                                &source.delivery_id,
                                source.channel,
                                claim.attempt_count,
                                None,
                                101,
                            )
                            .unwrap();
                    } else {
                        repository
                            .mark_failed(
                                &source.delivery_id,
                                claim.attempt_count,
                                None,
                                "IM_PROTOCOL_INVALID",
                                101,
                            )
                            .unwrap();
                    }
                }
            }
            repository
                .enqueue_resolution_and_expire_pending_source(
                    &source.canonical_event_id,
                    &resolution_for(&source),
                    crate::im::ImResolutionDispatch::Send,
                    102,
                )
                .unwrap();
            let connector = Arc::new(FakeConnector::new(ImMessageUpdateMode::Anytime));
            let worker = ImDeliveryWorker::new(source.channel, repository, connector.clone());
            assert_eq!(worker.dispatch_once(103).await.unwrap(), 1);
            assert_eq!(connector.sends.load(Ordering::SeqCst), 1);
            assert!(connector.updated_message_ids.lock().unwrap().is_empty());
            assert_eq!(worker.dispatch_once(6_000).await.unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn anytime_resolution_updates_the_source_message_in_place() {
        let connector = Arc::new(FakeConnector::new(ImMessageUpdateMode::Anytime));
        let (_temp, source_message_id) = dispatch_resolution(Arc::clone(&connector)).await;
        assert_eq!(connector.sends.load(Ordering::SeqCst), 0);
        assert_eq!(
            *connector.updated_message_ids.lock().unwrap(),
            vec![source_message_id]
        );
    }

    #[tokio::test]
    async fn callback_only_resolution_is_sent_as_a_new_message() {
        let connector = Arc::new(FakeConnector::new(ImMessageUpdateMode::CallbackOnly));
        let _ = dispatch_resolution(Arc::clone(&connector)).await;
        assert_eq!(connector.sends.load(Ordering::SeqCst), 1);
        assert!(connector.updated_message_ids.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unrecoverable_update_failure_falls_back_to_a_new_message() {
        let mut connector = FakeConnector::new(ImMessageUpdateMode::Anytime);
        connector.update_error = Some(ImIntegrationError::permanent(
            ImErrorCode::MessageUpdateUnavailable,
        ));
        let connector = Arc::new(connector);
        let _ = dispatch_resolution(Arc::clone(&connector)).await;
        assert_eq!(connector.sends.load(Ordering::SeqCst), 1);
        assert_eq!(
            *connector.sent_source_ids.lock().unwrap(),
            vec![permission_source().delivery_id],
            "fallback sends must preserve the source card and its published form"
        );
    }

    #[tokio::test]
    async fn permanent_protocol_or_authentication_errors_must_not_send_duplicate_cards() {
        for code in [
            ImErrorCode::ProtocolInvalid,
            ImErrorCode::AuthenticationRequired,
            ImErrorCode::ScopeMissing,
        ] {
            let mut connector = FakeConnector::new(ImMessageUpdateMode::Anytime);
            connector.update_error = Some(ImIntegrationError::permanent(code));
            let connector = Arc::new(connector);
            let _ = dispatch_resolution(Arc::clone(&connector)).await;
            assert_eq!(connector.sends.load(Ordering::SeqCst), 0, "{code:?}");
        }
    }

    #[tokio::test]
    async fn rate_limited_patch_retries_the_same_resolution_after_backoff() {
        let mut connector = FakeConnector::new(ImMessageUpdateMode::Anytime);
        connector.update_error = Some(ImIntegrationError::retryable(
            ImErrorCode::RateLimited,
            Some(Duration::from_secs(2)),
        ));
        let connector = Arc::new(connector);
        let (temp, source_message_id) = dispatch_resolution(connector.clone()).await;
        assert_eq!(connector.sends.load(Ordering::SeqCst), 0);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let recovered = Arc::new(FakeConnector::new(ImMessageUpdateMode::Anytime));
        let worker = ImDeliveryWorker::new(ImChannelKind::WeCom, repository, recovered.clone());
        assert_eq!(worker.dispatch_once(2101).await.unwrap(), 0);
        assert_eq!(worker.dispatch_once(2102).await.unwrap(), 1);
        assert_eq!(
            *recovered.updated_message_ids.lock().unwrap(),
            vec![source_message_id]
        );
        assert_eq!(recovered.sends.load(Ordering::SeqCst), 0);
        assert_eq!(worker.dispatch_once(5000).await.unwrap(), 0);
    }

    fn fixture() -> (TempDir, Arc<ImRepository>, ImDelivery) {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let channel = ImChannelKind::WeCom;
        let kind = ImNotificationKind::RunFailure;
        let destination = ImDestination {
            destination_id: "user-1".into(),
            conversation_id: "chat-1".into(),
            authorized_actor_id: "actor-1".into(),
        };
        let delivery = ImDelivery {
            delivery_id: ImDelivery::deterministic_id(channel, "user-1", kind, "event-1"),
            channel,
            destination,
            notification_kind: kind,
            canonical_event_id: "event-1".into(),
            payload: ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: "event-1".into(),
                    notification_kind: kind,
                    locator: ImNavigationLocator {
                        project_id: "project-1".into(),
                        task_id: Some("task-1".into()),
                        run_id: Some("run-1".into()),
                        round_id: None,
                        node_id: None,
                        attempt_id: None,
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    outcome: "failure".into(),
                    summary_key: "im.notification.runFailure.summary".into(),
                    parameters: Default::default(),
                    missed_count: None,
                },
            },
            expires_at_ms: i64::MAX,
            display_ref: None,
        };
        repository.enqueue(&delivery, 100).unwrap();
        (temp, repository, delivery)
    }

    #[tokio::test]
    async fn worker_releases_sqlite_before_network_and_settles_delivery() {
        let (_temp, repository, delivery) = fixture();
        let connector = Arc::new(FakeConnector::new(ImMessageUpdateMode::CallbackOnly));
        let worker = ImDeliveryWorker::new(
            ImChannelKind::WeCom,
            Arc::clone(&repository),
            connector.clone(),
        )
        .with_limits(1, Duration::from_secs(30));
        assert_eq!(worker.dispatch_once(100).await.unwrap(), 1);
        assert_eq!(connector.sends.load(Ordering::SeqCst), 1);
        assert!(
            repository
                .claim_due(ImChannelKind::WeCom, 101, Duration::from_secs(30), 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(delivery.canonical_event_id, "event-1");
    }

    #[tokio::test]
    async fn worker_cancellation_interrupts_a_blocked_connector_send() {
        let (_temp, repository, _delivery) = fixture();
        let connector = Arc::new(BlockingConnector {
            started: tokio::sync::Notify::new(),
        });
        let worker = ImDeliveryWorker::new(
            ImChannelKind::WeCom,
            Arc::clone(&repository),
            connector.clone(),
        );
        let (sender, receiver) = im_worker_channel();
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let started = connector.started.notified();
        let task = tokio::spawn(worker.run(receiver, task_cancellation));

        sender
            .send(ImWorkerSignal::DeliveryAvailable)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), started)
            .await
            .expect("connector send started");
        cancellation.cancel();

        tokio::time::timeout(Duration::from_millis(200), task)
            .await
            .expect("worker must converge after cancellation")
            .unwrap();
    }
}
