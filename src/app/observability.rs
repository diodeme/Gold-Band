use std::collections::{BTreeMap, HashSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};

use crate::app::RuntimeLifecycleEvent;
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsSessionMode {
    Direct,
    Workflow,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionKind {
    Turn,
    Run,
    NodeAttempt,
    OuterRun,
    UnitAttempt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnitKind {
    Worker,
    WorkflowInvocation,
    Merge,
    Acceptance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionOutcome {
    Completed,
    Failed,
    Cancelled,
    Success,
    Failure,
    Killed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalReason {
    Completed,
    UserCancelled,
    ProcessKilled,
    ProviderError,
    RuntimeError,
    ValidationError,
    ExecutionFailed,
    RetryExhausted,
    AcceptanceRejected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleEventType {
    ExecutionStarted,
    ExecutionCompleted,
    ExecutionPaused,
    ExecutionResumed,
    InterventionRequested,
    AcceptanceCompleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsInterventionKind {
    ManualDecision,
    Elicitation,
    Permission,
    RuntimeAbnormal,
    ErrorBlocked,
    ProcessInterrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsPauseReason {
    WaitingForUserInput,
    PermissionRequested,
    RuntimeAbnormal,
    ErrorBlocked,
    ProcessInterrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResumeCause {
    ManualContinue,
    PermissionResolved,
    ElicitationResolved,
    AutomaticRecovery,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    #[serde(flatten)]
    pub usage: TokenUsage,
    pub acp_session_elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsCounters {
    pub pause_count: u32,
    pub resume_count: u32,
    pub permission_request_count: u32,
    pub elicitation_count: u32,
    pub manual_continue_count: u32,
    #[serde(default)]
    pub follow_up_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleTiming {
    pub started_at: String,
    pub ended_at: Option<String>,
    pub acp_session_elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsLifecycleFact {
    pub event_id: String,
    pub event_revision: u64,
    pub event_type: LifecycleEventType,
    pub occurred_at: String,
    pub user_id: String,
    pub workspace: String,
    pub session_mode: MetricsSessionMode,
    pub task_id: String,
    pub task_title: Option<String>,
    pub execution_kind: ExecutionKind,
    pub execution_id: String,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
    pub attempt_index: Option<u32>,
    pub round_index: Option<u32>,
    pub role_name: Option<String>,
    pub unit_kind: Option<UnitKind>,
    pub child_run_id: Option<String>,
    pub outcome: Option<ExecutionOutcome>,
    pub terminal_reason: Option<TerminalReason>,
    pub terminal_reason_code: Option<String>,
    pub failed_attempt_id: Option<String>,
    pub round_count: Option<u32>,
    pub passed: Option<bool>,
    pub acceptance_attempt: Option<u32>,
    pub first_pass: Option<bool>,
    pub intervention_kind: Option<MetricsInterventionKind>,
    pub pause_reason: Option<MetricsPauseReason>,
    pub previous_pause_reason: Option<MetricsPauseReason>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: Option<TokenUsage>,
    pub model_usages: Option<Vec<ModelUsage>>,
    pub timing: Option<LifecycleTiming>,
    pub counters: Option<MetricsCounters>,
    pub collection_state_recovered: Option<bool>,
}

impl MetricsLifecycleFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_type: LifecycleEventType,
        event_revision: u64,
        occurred_at: String,
        user_id: String,
        workspace: String,
        session_mode: MetricsSessionMode,
        task_id: String,
        execution_kind: ExecutionKind,
        execution_id: String,
    ) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_revision,
            event_type,
            occurred_at,
            user_id,
            workspace,
            session_mode,
            task_id,
            task_title: None,
            execution_kind,
            execution_id,
            node_id: None,
            attempt_id: None,
            attempt_index: None,
            round_index: None,
            role_name: None,
            unit_kind: None,
            child_run_id: None,
            outcome: None,
            terminal_reason: None,
            terminal_reason_code: None,
            failed_attempt_id: None,
            round_count: None,
            passed: None,
            acceptance_attempt: None,
            first_pass: None,
            intervention_kind: None,
            pause_reason: None,
            previous_pause_reason: None,
            provider: None,
            model: None,
            usage: None,
            model_usages: None,
            timing: None,
            counters: None,
            collection_state_recovered: None,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.event_id.trim().is_empty()
            || self.execution_id.trim().is_empty()
            || self.task_id.trim().is_empty()
        {
            return Err("stable ids are required");
        }
        let terminal = self.event_type == LifecycleEventType::ExecutionCompleted;
        if terminal != self.outcome.is_some() || terminal != self.terminal_reason.is_some() {
            return Err("outcome and terminalReason are terminal-only and required together");
        }
        let delivery = matches!(
            self.execution_kind,
            ExecutionKind::Turn | ExecutionKind::Run | ExecutionKind::OuterRun
        );
        if self.counters.is_some() != (terminal && delivery) {
            return Err("counters are required only on delivery terminal events");
        }
        let attempt = matches!(
            self.execution_kind,
            ExecutionKind::Turn | ExecutionKind::NodeAttempt | ExecutionKind::UnitAttempt
        );
        if attempt && self.attempt_id.as_deref().is_none_or(str::is_empty) {
            return Err("attempt execution requires attemptId");
        }
        if attempt && self.attempt_index.is_none_or(|index| index == 0) {
            return Err("attempt execution requires positive attemptIndex");
        }
        if !attempt && self.attempt_index.is_some() {
            return Err("delivery execution must not carry attemptIndex");
        }
        if self.execution_kind == ExecutionKind::Turn
            && (self.attempt_id.as_deref() != Some(self.execution_id.as_str())
                || self.attempt_index != Some(1))
        {
            return Err("Direct turn requires attemptId equal to executionId and attemptIndex 1");
        }
        if matches!(
            self.execution_kind,
            ExecutionKind::NodeAttempt | ExecutionKind::UnitAttempt
        ) && self.attempt_id.as_deref() == Some(self.execution_id.as_str())
        {
            return Err("Workflow/AUTO attemptId must differ from logical executionId");
        }
        if self.execution_kind == ExecutionKind::NodeAttempt
            && (self.node_id.is_none() || self.round_index.is_none_or(|index| index == 0))
        {
            return Err("Workflow node attempt requires nodeId and positive roundIndex");
        }
        if self.execution_kind == ExecutionKind::UnitAttempt
            && (self.node_id.is_none() || self.unit_kind.is_none())
        {
            return Err("AUTO unit attempt requires nodeId and unitKind");
        }
        if matches!(
            self.execution_kind,
            ExecutionKind::Run | ExecutionKind::OuterRun
        ) && (self.usage.is_some()
            || self.model_usages.is_some()
            || self.provider.is_some()
            || self.model.is_some()
            || self.timing.is_some())
        {
            return Err("run delivery must not carry attempt usage or model fields");
        }
        if self.event_type == LifecycleEventType::AcceptanceCompleted
            && (self.passed.is_none()
                || self.acceptance_attempt.is_none()
                || self.first_pass.is_none())
        {
            return Err("acceptance result fields are required");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionObservabilityState {
    pub event_revision: u64,
    pub counters: MetricsCounters,
    #[serde(default)]
    permission_request_ids: HashSet<String>,
    #[serde(default)]
    elicitation_request_ids: HashSet<String>,
    #[serde(default)]
    model_usages: BTreeMap<String, ModelUsage>,
    #[serde(default)]
    model_order: Vec<String>,
    #[serde(default)]
    provider_cumulative: BTreeMap<String, TokenUsage>,
    #[serde(default)]
    provider_elapsed_cumulative: BTreeMap<String, u64>,
    #[serde(default)]
    acceptance_attempts: u32,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_resume_cause: Option<ResumeCause>,
    pub collection_state_recovered: Option<bool>,
}

impl ExecutionObservabilityState {
    pub fn recovered() -> Self {
        Self {
            collection_state_recovered: Some(true),
            ..Self::default()
        }
    }
    pub fn next_revision(&mut self) -> u64 {
        self.event_revision += 1;
        self.event_revision
    }
    pub fn record_started_at(&mut self, started_at: String) {
        if self.started_at.is_none() {
            self.started_at = Some(started_at);
        }
    }
    pub fn record_pause(&mut self, transitioned: bool) {
        if transitioned {
            self.counters.pause_count += 1;
        }
    }
    pub fn set_pending_resume_cause(&mut self, cause: ResumeCause) {
        self.pending_resume_cause = Some(cause);
    }
    pub fn take_pending_resume_cause(&mut self) -> Option<ResumeCause> {
        self.pending_resume_cause.take()
    }
    pub fn clear_pending_resume_cause(&mut self, expected: ResumeCause) {
        if self.pending_resume_cause == Some(expected) {
            self.pending_resume_cause = None;
        }
    }
    pub fn record_resume(&mut self, transitioned: bool, cause: ResumeCause) {
        if transitioned {
            self.counters.resume_count += 1;
            if cause == ResumeCause::ManualContinue {
                self.counters.manual_continue_count += 1;
            }
        }
    }
    pub fn record_follow_up(&mut self) {
        self.counters.follow_up_count = self.counters.follow_up_count.saturating_add(1);
    }
    pub fn record_permission(&mut self, id: &str) {
        if self.permission_request_ids.insert(id.into()) {
            self.counters.permission_request_count += 1;
        }
    }
    pub fn record_elicitation(&mut self, id: &str) {
        if self.elicitation_request_ids.insert(id.into()) {
            self.counters.elicitation_count += 1;
        }
    }
    pub fn record_model_usage(&mut self, usage: ModelUsage) {
        let key = format!("{}\u{0}{}", usage.provider, usage.model);
        if !self.model_usages.contains_key(&key) {
            self.model_order.push(key.clone());
        }
        let entry = self.model_usages.entry(key).or_insert_with(|| ModelUsage {
            provider: usage.provider.clone(),
            model: usage.model.clone(),
            usage: TokenUsage::default(),
            acp_session_elapsed_ms: None,
        });
        fn add(target: &mut Option<u64>, value: Option<u64>) {
            if let Some(value) = value {
                *target = Some(target.unwrap_or(0).saturating_add(value));
            }
        }
        add(&mut entry.usage.input_tokens, usage.usage.input_tokens);
        add(&mut entry.usage.output_tokens, usage.usage.output_tokens);
        add(
            &mut entry.usage.cache_read_tokens,
            usage.usage.cache_read_tokens,
        );
        add(&mut entry.usage.total_tokens, usage.usage.total_tokens);
        add(
            &mut entry.acp_session_elapsed_ms,
            usage.acp_session_elapsed_ms,
        );
    }
    pub fn record_cumulative_model_usage(
        &mut self,
        provider: String,
        model: String,
        cumulative: TokenUsage,
        elapsed_ms: Option<u64>,
    ) {
        let previous = self
            .provider_cumulative
            .insert(provider.clone(), cumulative.clone())
            .unwrap_or_default();
        fn delta(current: Option<u64>, previous: Option<u64>) -> Option<u64> {
            match (current, previous) {
                (Some(current), Some(previous)) if current >= previous => Some(current - previous),
                (Some(current), None) => Some(current),
                _ => None,
            }
        }
        let elapsed_delta = elapsed_ms.and_then(|current| {
            self.provider_elapsed_cumulative
                .insert(provider.clone(), current)
                .map_or(Some(current), |previous| current.checked_sub(previous))
        });
        self.record_model_usage(ModelUsage {
            provider,
            model,
            usage: TokenUsage {
                input_tokens: delta(cumulative.input_tokens, previous.input_tokens),
                output_tokens: delta(cumulative.output_tokens, previous.output_tokens),
                cache_read_tokens: delta(cumulative.cache_read_tokens, previous.cache_read_tokens),
                total_tokens: delta(cumulative.total_tokens, previous.total_tokens),
            },
            acp_session_elapsed_ms: elapsed_delta,
        });
    }
    pub fn set_cumulative_usage_baseline(
        &mut self,
        provider: String,
        cumulative: TokenUsage,
        elapsed_ms: Option<u64>,
    ) {
        self.provider_cumulative
            .insert(provider.clone(), cumulative);
        if let Some(elapsed_ms) = elapsed_ms {
            self.provider_elapsed_cumulative
                .insert(provider, elapsed_ms);
        }
    }
    pub fn next_acceptance_attempt(&mut self) -> u32 {
        self.acceptance_attempts = self.acceptance_attempts.saturating_add(1);
        self.acceptance_attempts
    }
    pub fn next_acceptance_attempt_value(&self) -> u32 {
        self.acceptance_attempts
    }
    pub fn model_usages(&self) -> Vec<ModelUsage> {
        self.model_order
            .iter()
            .filter_map(|key| self.model_usages.get(key).cloned())
            .collect()
    }
}

pub const OBSERVABILITY_SNAPSHOT_FILE: &str = "observability.snapshot.json";

pub fn derive_execution_id(parent_execution_id: &str, logical_key: &str) -> Option<String> {
    let namespace = uuid::Uuid::parse_str(parent_execution_id).ok()?;
    Some(uuid::Uuid::new_v5(&namespace, logical_key.as_bytes()).to_string())
}

pub fn derive_attempt_id(execution_id: &str, local_attempt_id: &str) -> Option<String> {
    let namespace = uuid::Uuid::parse_str(execution_id).ok()?;
    Some(uuid::Uuid::new_v5(&namespace, local_attempt_id.as_bytes()).to_string())
}

pub fn attempt_index_from_local_id(local_attempt_id: &str) -> Option<u32> {
    local_attempt_id
        .strip_prefix("attempt-")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
}

pub fn load_observability_snapshot(path: &camino::Utf8Path) -> ExecutionObservabilityState {
    match std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<ExecutionObservabilityState>(&json).ok())
    {
        Some(mut state) => {
            state.collection_state_recovered = Some(true);
            state
        }
        None if path.exists() => ExecutionObservabilityState {
            collection_state_recovered: Some(false),
            ..ExecutionObservabilityState::default()
        },
        None => ExecutionObservabilityState::default(),
    }
}

/// Snapshot persistence is deliberately detached from the runtime transition.
/// A failed write is diagnostic-only and can never roll back session state.
pub fn persist_observability_snapshot_best_effort(
    path: Utf8PathBuf,
    state: ExecutionObservabilityState,
) {
    let revision = state.event_revision;
    if let Err(error) = snapshot_writer().try_send(SnapshotWrite {
        path: path.clone(),
        state,
    }) {
        match error {
            std::sync::mpsc::TrySendError::Full(_) => tracing::warn!(
                queue = "observability-snapshot-writer",
                capacity = SNAPSHOT_QUEUE_CAPACITY,
                path = %path,
                revision,
                "observability snapshot queue is full; snapshot dropped"
            ),
            std::sync::mpsc::TrySendError::Disconnected(_) => tracing::warn!(
                queue = "observability-snapshot-writer",
                path = %path,
                revision,
                "observability snapshot writer is disconnected; snapshot dropped"
            ),
        }
    }
}

const SNAPSHOT_QUEUE_CAPACITY: usize = 2048;

struct SnapshotWrite {
    path: Utf8PathBuf,
    state: ExecutionObservabilityState,
}

fn snapshot_writer() -> &'static std::sync::mpsc::SyncSender<SnapshotWrite> {
    static WRITER: OnceLock<std::sync::mpsc::SyncSender<SnapshotWrite>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<SnapshotWrite>(SNAPSHOT_QUEUE_CAPACITY);
        let _ = std::thread::Builder::new()
            .name("observability-snapshot-writer".into())
            .spawn(move || {
                while let Ok(write) = receiver.recv() {
                    let Some(parent) = write.path.parent() else {
                        continue;
                    };
                    if std::fs::create_dir_all(parent).is_err() {
                        continue;
                    }
                    let temporary = write.path.with_extension("json.tmp");
                    let Ok(json) = serde_json::to_vec_pretty(&write.state) else {
                        continue;
                    };
                    if std::fs::write(&temporary, json).is_ok() {
                        let _ = std::fs::rename(temporary, write.path);
                    }
                }
            });
        sender
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriberMode {
    Async,
    Inline,
}

#[derive(Clone)]
struct RuntimeLifecycleSubscriber {
    mode: SubscriberMode,
    handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
}

#[derive(Default)]
struct RuntimeLifecycleSubscribers {
    entries: Vec<RuntimeLifecycleSubscriber>,
    names: HashSet<&'static str>,
}

#[derive(Clone, Default)]
pub struct RuntimeLifecycleBus {
    subscribers: Arc<RwLock<RuntimeLifecycleSubscribers>>,
}

impl RuntimeLifecycleBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>) {
        self.subscribe_with_mode(SubscriberMode::Async, handler);
    }

    pub fn subscribe_inline(&self, handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>) {
        self.subscribe_with_mode(SubscriberMode::Inline, handler);
    }

    pub fn subscribe_named(
        &self,
        name: &'static str,
        handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) -> bool {
        self.subscribe_named_with_mode(name, SubscriberMode::Async, handler)
    }

    pub fn subscribe_named_with_mode(
        &self,
        name: &'static str,
        mode: SubscriberMode,
        handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) -> bool {
        let mut subscribers = self
            .subscribers
            .write()
            .expect("RuntimeLifecycleBus subscriber state poisoned; this is a bug");
        if !subscribers.names.insert(name) {
            return false;
        }
        subscribers
            .entries
            .push(RuntimeLifecycleSubscriber { mode, handler });
        true
    }

    pub fn subscribe_with_mode(
        &self,
        mode: SubscriberMode,
        handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) {
        self.subscribers
            .write()
            .expect("RuntimeLifecycleBus subscriber state poisoned; this is a bug")
            .entries
            .push(RuntimeLifecycleSubscriber { mode, handler });
    }

    pub fn emit(&self, event: RuntimeLifecycleEvent) {
        let subscribers = self
            .subscribers
            .read()
            .expect("RuntimeLifecycleBus subscriber state poisoned; this is a bug")
            .entries
            .clone();
        for subscriber in subscribers {
            let event = event.clone();
            match subscriber.mode {
                SubscriberMode::Async => {
                    let handler = subscriber.handler;
                    let run = move || {
                        let _ = panic::catch_unwind(AssertUnwindSafe(move || {
                            handler(event);
                        }));
                    };
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move { run() });
                    } else {
                        let _ = std::thread::Builder::new()
                            .name("runtime-lifecycle-subscriber".to_string())
                            .spawn(run);
                    }
                }
                SubscriberMode::Inline => {
                    let handler = subscriber.handler;
                    let _ = panic::catch_unwind(AssertUnwindSafe(move || {
                        handler(event);
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PauseReason;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn execution_state_keeps_revision_counters_and_model_order_stable() {
        let mut state = ExecutionObservabilityState::recovered();
        assert_eq!(state.next_revision(), 1);
        assert_eq!(state.next_revision(), 2);
        state.record_pause(true);
        state.record_pause(false);
        state.record_resume(true, ResumeCause::ManualContinue);
        state.record_follow_up();
        state.record_permission("permission-1");
        state.record_permission("permission-1");
        state.record_elicitation("elicitation-1");
        for (provider, model, tokens) in [("a", "m1", 2), ("b", "m2", 3), ("a", "m1", 5)] {
            state.record_model_usage(ModelUsage {
                provider: provider.into(),
                model: model.into(),
                usage: TokenUsage {
                    input_tokens: Some(tokens),
                    output_tokens: None,
                    cache_read_tokens: None,
                    total_tokens: Some(tokens),
                },
                acp_session_elapsed_ms: Some(tokens),
            });
        }
        assert_eq!(state.counters.pause_count, 1);
        assert_eq!(state.counters.resume_count, 1);
        assert_eq!(state.counters.manual_continue_count, 1);
        assert_eq!(state.counters.follow_up_count, 1);
        assert_eq!(state.counters.permission_request_count, 1);
        assert_eq!(state.counters.elicitation_count, 1);
        let usages = state.model_usages();
        assert_eq!(
            usages
                .iter()
                .map(|usage| usage.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(usages[0].usage.total_tokens, Some(7));
    }

    #[test]
    fn missing_snapshot_means_no_recovery_was_attempted() {
        let dir = tempfile::tempdir().unwrap();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("missing.json")).unwrap();
        let state = load_observability_snapshot(&path);
        assert_eq!(state.collection_state_recovered, None);
        assert_eq!(state.event_revision, 0);
    }

    #[test]
    fn cumulative_usage_uses_delta_and_does_not_guess_after_reset() {
        let mut state = ExecutionObservabilityState::recovered();
        let usage = |total| TokenUsage {
            input_tokens: Some(total),
            output_tokens: None,
            cache_read_tokens: None,
            total_tokens: Some(total),
        };
        state.record_cumulative_model_usage("p".into(), "a".into(), usage(10), Some(100));
        state.record_cumulative_model_usage("p".into(), "b".into(), usage(15), Some(50));
        state.record_cumulative_model_usage("p".into(), "a".into(), usage(3), Some(25));
        let usages = state.model_usages();
        assert_eq!(usages[0].usage.total_tokens, Some(10));
        assert_eq!(usages[1].usage.total_tokens, Some(5));
        assert_eq!(usages[0].usage.input_tokens, Some(10));
    }

    #[test]
    fn cumulative_usage_baseline_is_excluded_from_turn_totals() {
        let mut state = ExecutionObservabilityState::default();
        state.set_cumulative_usage_baseline(
            "provider".into(),
            TokenUsage {
                input_tokens: Some(100),
                output_tokens: Some(20),
                cache_read_tokens: None,
                total_tokens: Some(120),
            },
            Some(5_000),
        );
        state.record_cumulative_model_usage(
            "provider".into(),
            "model".into(),
            TokenUsage {
                input_tokens: Some(180),
                output_tokens: Some(35),
                cache_read_tokens: None,
                total_tokens: Some(215),
            },
            Some(8_000),
        );
        let usages = state.model_usages();
        assert_eq!(usages[0].usage.input_tokens, Some(80));
        assert_eq!(usages[0].usage.output_tokens, Some(15));
        assert_eq!(usages[0].usage.cache_read_tokens, None);
        assert_eq!(usages[0].usage.total_tokens, Some(95));
        assert_eq!(usages[0].acp_session_elapsed_ms, Some(3_000));
    }

    #[test]
    fn resume_cause_controls_manual_continue_without_pause_reason_inference() {
        let mut state = ExecutionObservabilityState::default();
        for cause in [
            ResumeCause::PermissionResolved,
            ResumeCause::ElicitationResolved,
            ResumeCause::AutomaticRecovery,
            ResumeCause::ManualContinue,
        ] {
            state.set_pending_resume_cause(cause);
            let persisted_cause = state.take_pending_resume_cause().unwrap();
            state.record_resume(true, persisted_cause);
        }
        assert_eq!(state.counters.resume_count, 4);
        assert_eq!(state.counters.manual_continue_count, 1);
        assert_eq!(state.take_pending_resume_cause(), None);

        state.set_pending_resume_cause(ResumeCause::ElicitationResolved);
        let json = serde_json::to_string(&state).unwrap();
        let mut restored: ExecutionObservabilityState = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.take_pending_resume_cause(),
            Some(ResumeCause::ElicitationResolved)
        );
        restored.set_pending_resume_cause(ResumeCause::PermissionResolved);
        restored.clear_pending_resume_cause(ResumeCause::ElicitationResolved);
        assert_eq!(
            restored.take_pending_resume_cause(),
            Some(ResumeCause::PermissionResolved),
            "rollback must not clear a newer resume cause"
        );
    }

    #[test]
    fn lifecycle_contract_rejects_terminal_fields_and_counters_in_wrong_scope() {
        let mut fact = MetricsLifecycleFact::new(
            LifecycleEventType::ExecutionStarted,
            1,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            MetricsSessionMode::Workflow,
            "task-uuid".into(),
            ExecutionKind::NodeAttempt,
            "node-execution".into(),
        );
        fact.attempt_id = Some("node-attempt".into());
        fact.attempt_index = Some(1);
        fact.node_id = Some("node-execution".into());
        fact.round_index = Some(1);
        assert!(fact.validate().is_ok());
        fact.outcome = Some(ExecutionOutcome::Success);
        assert!(fact.validate().is_err());
        fact.event_type = LifecycleEventType::ExecutionCompleted;
        fact.terminal_reason = Some(TerminalReason::Completed);
        assert!(fact.validate().is_ok());
        fact.counters = Some(MetricsCounters::default());
        assert!(fact.validate().is_err());
    }

    #[test]
    fn lifecycle_contract_uses_attempt_granularity_for_usage() {
        let mut turn = MetricsLifecycleFact::new(
            LifecycleEventType::ExecutionStarted,
            1,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            MetricsSessionMode::Direct,
            "task-uuid".into(),
            ExecutionKind::Turn,
            "task-uuid".into(),
        );
        turn.attempt_id = Some("task-uuid".into());
        turn.attempt_index = Some(1);
        assert!(turn.validate().is_ok());
        turn.attempt_id = Some("task-attempt".into());
        assert!(turn.validate().is_err());

        let mut run = MetricsLifecycleFact::new(
            LifecycleEventType::ExecutionCompleted,
            2,
            "2026-08-01T00:01:00Z".into(),
            "user".into(),
            "workspace".into(),
            MetricsSessionMode::Workflow,
            "task-uuid".into(),
            ExecutionKind::Run,
            "run-uuid".into(),
        );
        run.outcome = Some(ExecutionOutcome::Success);
        run.terminal_reason = Some(TerminalReason::Completed);
        run.counters = Some(MetricsCounters::default());
        assert!(run.validate().is_ok());
        run.usage = Some(TokenUsage::default());
        assert!(run.validate().is_err());
    }

    #[test]
    fn workflow_and_auto_ids_keep_execution_stable_and_attempts_distinct() {
        let run_id = uuid::Uuid::new_v4().to_string();
        let node_execution = derive_execution_id(&run_id, "round:1:node:review").unwrap();
        assert_eq!(
            node_execution,
            derive_execution_id(&run_id, "round:1:node:review").unwrap()
        );
        let first = derive_attempt_id(&node_execution, "attempt-001").unwrap();
        let second = derive_attempt_id(&node_execution, "attempt-002").unwrap();
        assert_ne!(first, second);
        assert_ne!(first, node_execution);
        assert_eq!(attempt_index_from_local_id("attempt-001"), Some(1));
        assert_eq!(attempt_index_from_local_id("attempt-002"), Some(2));
        assert_eq!(attempt_index_from_local_id("invalid"), None);
    }

    #[test]
    fn direct_session_identity_is_task_scoped_single_attempt() {
        let task_uuid = uuid::Uuid::new_v4().to_string();
        let mut fact = MetricsLifecycleFact::new(
            LifecycleEventType::ExecutionStarted,
            1,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            MetricsSessionMode::Direct,
            task_uuid.clone(),
            ExecutionKind::Turn,
            task_uuid.clone(),
        );
        fact.attempt_id = Some(task_uuid.clone());
        fact.attempt_index = Some(1);
        assert!(fact.validate().is_ok());

        fact.attempt_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(fact.validate().is_err());
        fact.attempt_id = Some(task_uuid);
        fact.attempt_index = Some(2);
        assert!(fact.validate().is_err());
    }

    #[test]
    fn direct_attempt_snapshot_accumulates_usage_and_counters_across_turns() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("direct-attempt.json")).unwrap();
        let mut first = ExecutionObservabilityState::default();
        first.record_resume(true, ResumeCause::ManualContinue);
        first.record_follow_up();
        first.record_model_usage(ModelUsage {
            provider: "codex-acp".into(),
            model: "glm-5.1".into(),
            usage: TokenUsage {
                total_tokens: Some(10),
                ..Default::default()
            },
            acp_session_elapsed_ms: Some(10),
        });
        persist_observability_snapshot_best_effort(path.clone(), first);

        let mut second = load_observability_snapshot(&path);
        for _ in 0..100 {
            if second.counters.follow_up_count == 1
                && second
                    .model_usages()
                    .first()
                    .is_some_and(|u| u.usage.total_tokens == Some(10))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            second = load_observability_snapshot(&path);
        }
        second.record_resume(true, ResumeCause::ManualContinue);
        second.record_follow_up();
        second.record_model_usage(ModelUsage {
            provider: "codex-acp".into(),
            model: "glm-5.1".into(),
            usage: TokenUsage {
                total_tokens: Some(20),
                ..Default::default()
            },
            acp_session_elapsed_ms: Some(20),
        });
        persist_observability_snapshot_best_effort(path.clone(), second);

        let mut state = load_observability_snapshot(&path);
        for _ in 0..100 {
            if state.counters.follow_up_count == 2
                && state
                    .model_usages()
                    .first()
                    .is_some_and(|u| u.usage.total_tokens == Some(30))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            state = load_observability_snapshot(&path);
        }
        assert_eq!(state.counters.manual_continue_count, 2);
        assert_eq!(state.counters.resume_count, 2);
        assert_eq!(state.counters.follow_up_count, 2);
        let usages = state.model_usages();
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].usage.total_tokens, Some(30));
        assert_eq!(usages[0].acp_session_elapsed_ms, Some(30));
    }

    #[test]
    fn legacy_counters_json_defaults_follow_up_count() {
        let raw = r#"{"pauseCount":1,"resumeCount":1,"permissionRequestCount":0,"elicitationCount":0,"manualContinueCount":1}"#;
        let counters: MetricsCounters = serde_json::from_str(raw).unwrap();
        assert_eq!(counters.pause_count, 1);
        assert_eq!(counters.manual_continue_count, 1);
        assert_eq!(counters.follow_up_count, 0);
    }

    #[test]
    fn snapshot_single_writer_preserves_latest_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("observability.snapshot.json"))
                .unwrap();
        let mut first = ExecutionObservabilityState::recovered();
        first.event_revision = 1;
        let mut second = first.clone();
        second.event_revision = 2;
        persist_observability_snapshot_best_effort(path.clone(), first);
        persist_observability_snapshot_best_effort(path.clone(), second);
        for _ in 0..100 {
            if load_observability_snapshot(&path).event_revision == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(load_observability_snapshot(&path).event_revision, 2);
    }

    fn sample_event() -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunPaused {
            event_id: "event-1".to_string(),
            occurred_at: "2026-01-01T00:00:00".to_string(),
            task_id: "task-1".to_string(),
            run_id: "run-1".to_string(),
            round_id: "round-1".to_string(),
            node_id: "node-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            node_label: "node".to_string(),
            pause_reason: PauseReason::ProcessInterrupted,
            task_title: None,
        }
    }

    #[test]
    fn inline_subscriber_panic_does_not_stop_other_subscribers() {
        let bus = RuntimeLifecycleBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = count.clone();
        bus.subscribe_inline(Arc::new(|_| panic!("subscriber panic")));
        bus.subscribe_inline(Arc::new(move |_| {
            count_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        bus.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cloned_bus_shares_subscribers() {
        let bus = RuntimeLifecycleBus::new();
        let cloned = bus.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = count.clone();
        bus.subscribe_inline(Arc::new(move |_| {
            count_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        cloned.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn named_subscriber_is_registered_once_across_clones() {
        let bus = RuntimeLifecycleBus::new();
        let cloned = bus.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let first_count = count.clone();
        let duplicate_count = count.clone();

        assert!(bus.subscribe_named_with_mode(
            "desktop.metrics",
            SubscriberMode::Inline,
            Arc::new(move |_| {
                first_count.fetch_add(1, Ordering::SeqCst);
            }),
        ));
        assert!(!cloned.subscribe_named_with_mode(
            "desktop.metrics",
            SubscriberMode::Inline,
            Arc::new(move |_| {
                duplicate_count.fetch_add(100, Ordering::SeqCst);
            }),
        ));

        cloned.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn different_named_subscribers_are_all_registered() {
        let bus = RuntimeLifecycleBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        for name in ["desktop.metrics", "desktop.notifications"] {
            let count = count.clone();
            assert!(bus.subscribe_named_with_mode(
                name,
                SubscriberMode::Inline,
                Arc::new(move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                }),
            ));
        }

        bus.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
