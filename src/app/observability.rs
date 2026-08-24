use std::collections::{BTreeMap, HashSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};

use crate::app::RuntimeLifecycleEvent;
use crate::storage::write_json;
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMetricsKey {
    pub project_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptMetricsKey {
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsRuntimeLocator {
    pub run_id: String,
    pub round_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MetricsSubject {
    DirectTurn {
        attempt_id: String,
        attempt_index: u32,
    },
    WorkflowRun,
    WorkflowNodeAttempt {
        node_id: String,
        attempt_id: String,
        attempt_index: u32,
        round_index: u32,
        role_name: String,
    },
    AutoOuterRun,
    AutoUnitAttempt {
        node_id: String,
        attempt_id: String,
        attempt_index: u32,
        round_index: u32,
        role_name: String,
        unit_kind: UnitKind,
    },
}

impl MetricsSubject {
    pub fn execution_kind(&self) -> ExecutionKind {
        match self {
            Self::DirectTurn { .. } => ExecutionKind::Turn,
            Self::WorkflowRun => ExecutionKind::Run,
            Self::WorkflowNodeAttempt { .. } => ExecutionKind::NodeAttempt,
            Self::AutoOuterRun => ExecutionKind::OuterRun,
            Self::AutoUnitAttempt { .. } => ExecutionKind::UnitAttempt,
        }
    }

    pub fn attempt_key(&self, locator: &MetricsRuntimeLocator) -> Option<AttemptMetricsKey> {
        match self {
            Self::DirectTurn { attempt_id, .. } => Some(AttemptMetricsKey {
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                node_id: "direct-turn".to_string(),
                attempt_id: attempt_id.clone(),
            }),
            Self::WorkflowNodeAttempt {
                node_id,
                attempt_id,
                ..
            }
            | Self::AutoUnitAttempt {
                node_id,
                attempt_id,
                ..
            } => Some(AttemptMetricsKey {
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                node_id: node_id.clone(),
                attempt_id: attempt_id.clone(),
            }),
            Self::WorkflowRun | Self::AutoOuterRun => None,
        }
    }

    pub fn is_delivery(&self) -> bool {
        matches!(
            self,
            Self::DirectTurn { .. } | Self::WorkflowRun | Self::AutoOuterRun
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MetricsTaskOrigin {
    User,
    ScheduledTask { scheduled_task_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScheduledTriggerKind {
    Scheduled,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScheduledSessionPolicy {
    New,
    Continuous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MetricsExecutionTrigger {
    User,
    ScheduledOccurrence {
        scheduled_task_id: String,
        scheduled_occurrence_id: String,
        trigger_kind: ScheduledTriggerKind,
        scheduled_at: String,
        session_policy: ScheduledSessionPolicy,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserExecutionAction {
    ManualContinue,
    PermissionResponse,
    ElicitationResponse,
    FollowUp,
    AutomaticRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MetricsTransition {
    None,
    Paused {
        transition_id: String,
    },
    Resumed {
        transition_id: String,
        action: UserExecutionAction,
    },
    PermissionRequested {
        request_id: String,
    },
    ElicitationRequested {
        request_id: String,
    },
    FollowUp {
        action_id: String,
    },
    Acceptance {
        action_id: String,
        passed: bool,
    },
}

impl Default for MetricsTransition {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeChangeCompleteness {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeChangeFileDelta {
    pub logical_path: String,
    pub added_lines: u64,
    pub deleted_lines: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCodeChangeDelta {
    pub completeness: CodeChangeCompleteness,
    pub files: Vec<CodeChangeFileDelta>,
    pub limitation_codes: Vec<String>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsPayload {
    pub task_title: Option<String>,
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
    pub child_run_id: Option<String>,
    pub code_change_delta: Option<TaskCodeChangeDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMetricsFact {
    pub key: TaskMetricsKey,
    pub event_type: LifecycleEventType,
    pub occurred_at: String,
    pub user_id: String,
    pub workspace: String,
    pub session_mode: MetricsSessionMode,
    pub subject: MetricsSubject,
    pub runtime_locator: MetricsRuntimeLocator,
    pub task_origin: MetricsTaskOrigin,
    pub execution_trigger: MetricsExecutionTrigger,
    #[serde(default)]
    pub transition: MetricsTransition,
    #[serde(default)]
    pub payload: MetricsPayload,
}

impl PendingMetricsFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: TaskMetricsKey,
        event_type: LifecycleEventType,
        occurred_at: String,
        user_id: String,
        workspace: String,
        session_mode: MetricsSessionMode,
        subject: MetricsSubject,
        runtime_locator: MetricsRuntimeLocator,
        task_origin: MetricsTaskOrigin,
        execution_trigger: MetricsExecutionTrigger,
    ) -> Self {
        Self {
            key,
            event_type,
            occurred_at,
            user_id,
            workspace,
            session_mode,
            subject,
            runtime_locator,
            task_origin,
            execution_trigger,
            transition: MetricsTransition::None,
            payload: MetricsPayload::default(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.key.project_id.trim().is_empty()
            || self.key.execution_id.trim().is_empty()
            || self.runtime_locator.run_id.trim().is_empty()
            || self.runtime_locator.round_id.trim().is_empty()
        {
            return Err("project, task, run and round ids are required");
        }
        let terminal = self.event_type == LifecycleEventType::ExecutionCompleted;
        if terminal != self.payload.outcome.is_some()
            || terminal != self.payload.terminal_reason.is_some()
        {
            return Err("outcome and terminalReason are terminal-only and required together");
        }
        match (&self.session_mode, &self.subject) {
            (MetricsSessionMode::Direct, MetricsSubject::DirectTurn { attempt_index, .. })
                if *attempt_index > 0 => {}
            (MetricsSessionMode::Workflow, MetricsSubject::WorkflowRun) => {}
            (
                MetricsSessionMode::Workflow,
                MetricsSubject::WorkflowNodeAttempt {
                    attempt_index,
                    round_index,
                    ..
                },
            ) if *attempt_index > 0 && *round_index > 0 => {}
            (MetricsSessionMode::Auto, MetricsSubject::AutoOuterRun) => {}
            (
                MetricsSessionMode::Auto,
                MetricsSubject::AutoUnitAttempt {
                    attempt_index,
                    round_index,
                    ..
                },
            ) if *attempt_index > 0 && *round_index > 0 => {}
            _ => return Err("session mode and metrics subject are inconsistent"),
        }
        let intermediate = matches!(
            self.event_type,
            LifecycleEventType::ExecutionPaused
                | LifecycleEventType::ExecutionResumed
                | LifecycleEventType::InterventionRequested
        );
        if intermediate
            && !matches!(
                self.subject,
                MetricsSubject::DirectTurn { .. }
                    | MetricsSubject::WorkflowNodeAttempt { .. }
                    | MetricsSubject::AutoUnitAttempt { .. }
            )
        {
            return Err("intermediate lifecycle events require an attempt subject");
        }
        if let (
            MetricsTaskOrigin::ScheduledTask {
                scheduled_task_id: origin_id,
            },
            MetricsExecutionTrigger::ScheduledOccurrence {
                scheduled_task_id: trigger_id,
                ..
            },
        ) = (&self.task_origin, &self.execution_trigger)
            && origin_id != trigger_id
        {
            return Err("scheduled task origin and trigger ids must match");
        }
        if self.session_mode != MetricsSessionMode::Direct
            && matches!(
                self.execution_trigger,
                MetricsExecutionTrigger::ScheduledOccurrence {
                    session_policy: ScheduledSessionPolicy::Continuous,
                    ..
                }
            )
        {
            return Err("continuous scheduled sessions are direct-only");
        }
        if self.payload.code_change_delta.is_some() && !terminal {
            return Err("code change deltas are terminal-only");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionObservabilityState {
    #[serde(default)]
    model_usages: BTreeMap<String, ModelUsage>,
    #[serde(default)]
    model_order: Vec<String>,
    #[serde(default)]
    provider_cumulative: BTreeMap<String, TokenUsage>,
    #[serde(default)]
    provider_elapsed_cumulative: BTreeMap<String, u64>,
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
    pub fn record_started_at(&mut self, started_at: String) {
        if self.started_at.is_none() {
            self.started_at = Some(started_at);
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
    if let Err(error) = snapshot_writer().try_send(SnapshotWrite {
        path: path.clone(),
        state,
    }) {
        match error {
            std::sync::mpsc::TrySendError::Full(_) => tracing::warn!(
                queue = "observability-snapshot-writer",
                capacity = SNAPSHOT_QUEUE_CAPACITY,
                path = %path,
                "observability snapshot queue is full; snapshot dropped"
            ),
            std::sync::mpsc::TrySendError::Disconnected(_) => tracing::warn!(
                queue = "observability-snapshot-writer",
                path = %path,
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
                    if let Err(error) = write_json(&write.path, &write.state) {
                        tracing::warn!(
                            path = %write.path,
                            error = %error,
                            "observability snapshot write failed"
                        );
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
        // Observability must not unwind a canonical task transition. Recover
        // the last subscriber snapshot even if an earlier registration panic
        // poisoned the lock.
        let subscribers = match self.subscribers.read() {
            Ok(subscribers) => subscribers.entries.clone(),
            Err(poisoned) => poisoned.into_inner().entries.clone(),
        };
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
    fn direct_session_identity_is_task_scoped_typed_subject() {
        let task_uuid = uuid::Uuid::new_v4().to_string();
        let mut fact = PendingMetricsFact::new(
            TaskMetricsKey {
                project_id: "project-1".to_string(),
                execution_id: task_uuid.clone(),
            },
            LifecycleEventType::ExecutionStarted,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            MetricsSessionMode::Direct,
            MetricsSubject::DirectTurn {
                attempt_id: task_uuid.clone(),
                attempt_index: 1,
            },
            MetricsRuntimeLocator {
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
            },
            MetricsTaskOrigin::User,
            MetricsExecutionTrigger::User,
        );
        assert!(fact.validate().is_ok());

        fact.subject = MetricsSubject::WorkflowRun;
        assert!(fact.validate().is_err());
    }

    fn sample_event() -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunPaused {
            event_id: "event-1".to_string(),
            occurred_at: "2026-01-01T00:00:00".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-1".to_string(),
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

    #[test]
    fn poisoned_subscriber_lock_does_not_unwind_publisher() {
        let bus = RuntimeLifecycleBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = count.clone();
        bus.subscribe_inline(Arc::new(move |_| {
            count_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        let subscribers = bus.subscribers.clone();
        let _ = panic::catch_unwind(AssertUnwindSafe(move || {
            let _guard = subscribers.write().unwrap();
            panic!("poison subscriber registry");
        }));

        bus.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    fn pending_workflow_fact(subject: MetricsSubject) -> PendingMetricsFact {
        PendingMetricsFact::new(
            TaskMetricsKey {
                project_id: "project-1".to_string(),
                execution_id: uuid::Uuid::new_v4().to_string(),
            },
            LifecycleEventType::ExecutionStarted,
            "2026-08-20T00:00:00Z".to_string(),
            "user".to_string(),
            "D:/repo".to_string(),
            MetricsSessionMode::Workflow,
            subject,
            MetricsRuntimeLocator {
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
            },
            MetricsTaskOrigin::User,
            MetricsExecutionTrigger::User,
        )
    }

    #[test]
    fn pending_fact_derives_execution_kind_from_typed_subject() {
        let fact = pending_workflow_fact(MetricsSubject::WorkflowNodeAttempt {
            node_id: "node-uuid".to_string(),
            attempt_id: "attempt-uuid".to_string(),
            attempt_index: 1,
            round_index: 1,
            role_name: "reviewer".to_string(),
        });

        assert_eq!(fact.subject.execution_kind(), ExecutionKind::NodeAttempt);
        assert!(fact.validate().is_ok());
    }

    #[test]
    fn pending_fact_rejects_delivery_subject_for_intermediate_event() {
        let mut fact = pending_workflow_fact(MetricsSubject::WorkflowRun);
        fact.event_type = LifecycleEventType::ExecutionPaused;

        assert_eq!(
            fact.validate(),
            Err("intermediate lifecycle events require an attempt subject")
        );
    }

    #[test]
    fn pending_fact_rejects_cross_job_scheduled_provenance() {
        let mut fact = pending_workflow_fact(MetricsSubject::WorkflowRun);
        fact.task_origin = MetricsTaskOrigin::ScheduledTask {
            scheduled_task_id: "job-a".to_string(),
        };
        fact.execution_trigger = MetricsExecutionTrigger::ScheduledOccurrence {
            scheduled_task_id: "job-b".to_string(),
            scheduled_occurrence_id: "occurrence-1".to_string(),
            trigger_kind: ScheduledTriggerKind::Scheduled,
            scheduled_at: "2026-08-20T00:00:00Z".to_string(),
            session_policy: ScheduledSessionPolicy::New,
        };

        assert_eq!(
            fact.validate(),
            Err("scheduled task origin and trigger ids must match")
        );
    }
}
