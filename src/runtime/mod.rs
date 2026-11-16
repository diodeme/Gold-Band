use crate::domain::{
    NodeOutcome, NodeType, PauseReason, ResolvedConfig, RoundTrigger, RunOutcome, RunStatus,
    SessionMode, VERSION,
};
use anyhow::{Result, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CURRENT_ACP_STORAGE_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{msg}")]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLifecycleTransitionError {
    pub code: String,
    pub msg: String,
    pub from: RuntimeExecutionPhase,
    pub to: RuntimeExecutionPhase,
}

pub struct RuntimeLifecycleStore;

impl RuntimeLifecycleStore {
    pub fn transition(
        run: &mut RunState,
        phase: RuntimeExecutionPhase,
        locator: Option<RuntimeAttemptLocator>,
        updated_at: impl Into<String>,
    ) -> std::result::Result<(), RuntimeLifecycleTransitionError> {
        let from = run.execution.phase;
        if !runtime_execution_transition_allowed(from, phase) {
            return Err(RuntimeLifecycleTransitionError {
                code: "runtime.execution-transition-invalid".to_string(),
                msg: format!("runtime execution cannot transition from {from:?} to {phase:?}"),
                from,
                to: phase,
            });
        }
        run.execution.revision = run.execution.revision.saturating_add(1);
        run.execution.phase = phase;
        run.execution.locator = locator;
        run.execution.updated_at = updated_at.into();
        Ok(())
    }
}

pub(crate) fn runtime_execution_transition_allowed(
    from: RuntimeExecutionPhase,
    to: RuntimeExecutionPhase,
) -> bool {
    use RuntimeExecutionPhase::*;
    from == to
        || matches!(
            (from, to),
            (
                StartingNode,
                RunningNode
                    | FinalizingArtifact
                    | RepairingArtifact
                    | PreparingWorkspace
                    | Paused
                    | Terminal
            ) | (
                RunningNode,
                StartingNode
                    | FinalizingArtifact
                    | RepairingArtifact
                    | AwaitingManualCheck
                    | Transitioning
                    | PreparingWorkspace
                    | Paused
                    | Terminal
            ) | (
                FinalizingArtifact,
                RepairingArtifact | AwaitingManualCheck | Transitioning | Paused | Terminal
            ) | (
                RepairingArtifact,
                FinalizingArtifact | AwaitingManualCheck | Transitioning | Paused | Terminal
            ) | (AwaitingManualCheck, Transitioning | Paused)
                | (
                    Transitioning,
                    LaunchingNextNode | StartingNode | PreparingWorkspace | Paused | Terminal
                )
                | (
                    LaunchingNextNode,
                    StartingNode | RunningNode | PreparingWorkspace | Paused | Terminal
                )
                | (
                    PreparingWorkspace,
                    StartingNode
                        | RunningNode
                        | Transitioning
                        | LaunchingNextNode
                        | Paused
                        | Terminal
                )
                | (
                    Paused,
                    StartingNode
                        | FinalizingArtifact
                        | RepairingArtifact
                        | AwaitingManualCheck
                        | Transitioning
                        | PreparingWorkspace
                )
                | (Terminal, Terminal)
        )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAttemptLocator {
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_attempt_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeExecutionPhase {
    StartingNode,
    RunningNode,
    FinalizingArtifact,
    RepairingArtifact,
    AwaitingManualCheck,
    Transitioning,
    LaunchingNextNode,
    PreparingWorkspace,
    Paused,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExecutionState {
    pub revision: u64,
    pub phase: RuntimeExecutionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<RuntimeAttemptLocator>,
    /// Fencing token for the current persisted runtime recovery candidate.
    /// It does not express lifecycle state; `run.status` remains canonical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_candidate_token: Option<String>,
    pub updated_at: String,
}

impl Default for RuntimeExecutionState {
    fn default() -> Self {
        Self {
            revision: 0,
            phase: RuntimeExecutionPhase::StartingNode,
            locator: None,
            recovery_candidate_token: None,
            updated_at: String::new(),
        }
    }
}

impl RuntimeExecutionState {
    pub fn new(
        phase: RuntimeExecutionPhase,
        locator: Option<RuntimeAttemptLocator>,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            revision: 1,
            phase,
            locator,
            recovery_candidate_token: None,
            updated_at: updated_at.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LastExecutedNode {
    pub node_id: String,
    pub uuid: String,
    #[serde(default)]
    pub round_uuid: String,
    pub node_name: String,
    #[serde(default)]
    pub seq: Option<u32>,
    #[serde(default)]
    pub agent_type: Option<String>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Path to this node's attempt directory. Subscribers read
    /// `acp.snapshot.json` from here for token counts.
    #[serde(default)]
    pub attempt_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub version: String,
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunWorktreeState {
    pub path: Utf8PathBuf,
    pub branch: String,
    pub fork_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunRerunOrigin {
    pub source_run_id: String,
    pub source_run_uuid: Option<String>,
    pub source_round_id: String,
    pub source_round_index: u32,
    pub source_node_id: String,
    pub source_attempt_id: String,
    pub operation_id: String,
    pub inherited_trace_sequence: u32,
    pub source_authoring_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunRerunSeal {
    pub operation_id: String,
    pub successor_run_id: String,
    pub sealed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunRerunMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<RunRerunOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_by: Option<RunRerunSeal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub version: String,
    pub id: String,
    pub task_id: String,
    #[serde(default)]
    pub task_uuid: Option<String>,
    pub status: RunStatus,
    pub outcome: Option<RunOutcome>,
    pub started_at: String,
    pub updated_at: String,
    pub workflow_snapshot: String,
    pub current_round: Option<String>,
    pub current_node: Option<String>,
    pub current_attempt: Option<String>,
    #[serde(default, alias = "acceptance_loops_used")]
    pub new_rounds_opened: u32,
    pub pause_reason: Option<PauseReason>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub last_executed_node: Option<LastExecutedNode>,
    /// Runtime-owned worktree selected when this run was created. `None`
    /// means the run executes in the project's main workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<RunWorktreeState>,
    /// Cross-run retry provenance and the source run's immutable handoff fact.
    /// This is optional for legacy runs and does not add a lifecycle state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerun: Option<RunRerunMetadata>,
    /// Authoritative Workflow Runtime phase. ACP session/turn state must never
    /// infer or mutate this aggregate.
    #[serde(default)]
    pub execution: RuntimeExecutionState,
}

impl RunState {
    pub fn is_sealed(&self) -> bool {
        self.rerun
            .as_ref()
            .is_some_and(|rerun| rerun.sealed_by.is_some())
    }

    pub fn is_inherited_step(&self, round: &RoundState, step: &RoundTraceStep) -> bool {
        let Some(origin) = self.rerun.as_ref().and_then(|rerun| rerun.origin.as_ref()) else {
            return false;
        };
        round.index < origin.source_round_index
            || (round.id == origin.source_round_id
                && step.sequence <= origin.inherited_trace_sequence)
    }

    pub fn current_execution_locator(&self) -> Option<RuntimeAttemptLocator> {
        Some(RuntimeAttemptLocator {
            round_id: self.current_round.clone()?,
            node_id: self.current_node.clone()?,
            attempt_id: self.current_attempt.clone()?,
            outer_node_id: None,
            outer_attempt_id: None,
        })
    }

    pub fn transition_execution(
        &mut self,
        phase: RuntimeExecutionPhase,
        locator: Option<RuntimeAttemptLocator>,
        updated_at: impl Into<String>,
    ) -> std::result::Result<(), RuntimeLifecycleTransitionError> {
        RuntimeLifecycleStore::transition(self, phase, locator, updated_at)
    }

    pub fn transition_current_execution(
        &mut self,
        phase: RuntimeExecutionPhase,
        updated_at: impl Into<String>,
    ) -> std::result::Result<(), RuntimeLifecycleTransitionError> {
        let locator = self.current_execution_locator();
        self.transition_execution(phase, locator, updated_at)
    }

    /// Deterministic one-time migration for run.json written before execution
    /// phases became authoritative.
    pub fn reconcile_legacy_execution(&mut self) -> bool {
        if self.execution.revision != 0 || !self.execution.updated_at.is_empty() {
            return false;
        }
        let phase = match self.status {
            RunStatus::Running => RuntimeExecutionPhase::StartingNode,
            RunStatus::Paused => RuntimeExecutionPhase::Paused,
            RunStatus::Completed => RuntimeExecutionPhase::Terminal,
        };
        self.execution = RuntimeExecutionState::new(
            phase,
            self.current_execution_locator(),
            self.updated_at.clone(),
        );
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundState {
    pub version: String,
    pub id: String,
    pub run_id: String,
    pub index: u32,
    pub status: RunStatus,
    pub outcome: Option<RunOutcome>,
    pub trigger: RoundTrigger,
    pub started_at: String,
    #[serde(default)]
    pub trace: Vec<RoundTraceStep>,
    #[serde(default)]
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTraceStep {
    pub sequence: u32,
    pub node_id: String,
    pub attempt_id: String,
    pub from_node_id: Option<String>,
    pub edge_outcome: Option<String>,
    pub entered_at: String,
}

/// Lifecycle-only admission result for retrying the canonical current node.
///
/// This deliberately does not include executable preflight (provider/profile,
/// workflow snapshot or Git/worktree checks). Those checks belong to the
/// command boundary after lifecycle admission has succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentNodeRetryReadiness {
    Ready,
    RunActive,
    RunTerminal,
    RunNotRetryable,
    AttemptNotRetryable,
    ErrorBlocked,
    ManualCheckPending,
    UnsupportedNode,
}

/// Decides whether a conversation Workflow's canonical current attempt may
/// create a same-node retry attempt. Callers must pass the resolved
/// conversation run mode because it is authoring metadata, not part of
/// `RunState`.
pub fn current_node_retry_readiness(
    run: &RunState,
    node: &NodeState,
    workflow_mode: bool,
) -> CurrentNodeRetryReadiness {
    if !workflow_mode || run.is_sealed() {
        return CurrentNodeRetryReadiness::RunNotRetryable;
    }
    if run.status == RunStatus::Running || node.runtime_execution_id.is_some() {
        return CurrentNodeRetryReadiness::RunActive;
    }
    if run.status == RunStatus::Completed {
        return CurrentNodeRetryReadiness::RunTerminal;
    }
    if run.pause_reason == Some(PauseReason::ErrorBlocked) {
        return CurrentNodeRetryReadiness::ErrorBlocked;
    }
    if node.manual_check_pending {
        return CurrentNodeRetryReadiness::ManualCheckPending;
    }
    if node.node_type != NodeType::Worker {
        return CurrentNodeRetryReadiness::UnsupportedNode;
    }
    if run.status != RunStatus::Paused
        || run.pause_reason != Some(PauseReason::RuntimeAbnormal)
        || node.status != RunStatus::Paused
        || node.outcome.is_some()
        || run.current_round.as_deref() != Some(node.round_id.as_str())
        || run.current_node.as_deref() != Some(node.node_id.as_str())
        || run.current_attempt.as_deref() != Some(node.attempt_id.as_str())
    {
        return CurrentNodeRetryReadiness::AttemptNotRetryable;
    }
    CurrentNodeRetryReadiness::Ready
}

/// Completed failures fork a successor run; they must never re-open this run.
/// Inherited prefix attempts are historical projections of an earlier Run and
/// are therefore read-only for new handoffs.
pub fn can_rerun_failed_attempt(
    run: &RunState,
    round: &RoundState,
    step: &RoundTraceStep,
    node: &NodeState,
    workflow_mode: bool,
) -> bool {
    workflow_mode
        && run.pause_reason != Some(PauseReason::ErrorBlocked)
        && !run.is_inherited_step(round, step)
        && node.run_id == run.id
        && node.round_id == round.id
        && node.node_id == step.node_id
        && node.attempt_id == step.attempt_id
        && node.node_type == NodeType::Worker
        && node.status == RunStatus::Completed
        && matches!(
            node.outcome,
            Some(crate::domain::NodeOutcome::Failure | crate::domain::NodeOutcome::Invalid)
        )
        && !node.manual_check_pending
}

/// Returns the canonical trace projection for one round.
///
/// The current round is truncated at the unique `RunState.current*` locator;
/// earlier completed rounds expose their complete validated trace; later
/// rounds are uncommitted and therefore ignored. This function is intentionally
/// pure so every reader (runtime, recovery and view-model projection) can use
/// the same crash-consistency rule.
pub fn canonical_trace_for_round(
    run: &RunState,
    rounds: &[RoundState],
    target_round: &RoundState,
) -> Result<Vec<RoundTraceStep>> {
    ensure!(target_round.run_id == run.id, "runtime.round-run-mismatch");
    let current_round_id = run
        .current_round
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime.current-round-missing"))?;
    let current_round = rounds
        .iter()
        .find(|round| round.id == current_round_id)
        .or_else(|| (target_round.id == current_round_id).then_some(target_round))
        .ok_or_else(|| anyhow::anyhow!("runtime.current-round-missing"))?;
    ensure!(
        !rounds
            .iter()
            .any(|round| round.index == current_round.index && round.id != current_round.id),
        "runtime.round-identity-conflict"
    );
    ensure!(
        target_round.index != current_round.index || target_round.id == current_round.id,
        "runtime.round-identity-conflict"
    );

    let mut trace = target_round.trace.clone();
    trace.sort_by_key(|step| step.sequence);
    let mut previous_sequence = 0_u32;
    for step in &trace {
        ensure!(
            step.sequence > previous_sequence,
            "runtime.round-trace-sequence-invalid"
        );
        previous_sequence = step.sequence;
    }

    if target_round.index > current_round.index {
        return Ok(Vec::new());
    }
    if target_round.index < current_round.index {
        ensure!(
            target_round.status == RunStatus::Completed && target_round.outcome.is_some(),
            "runtime.previous-round-not-completed"
        );
        return Ok(trace);
    }

    let (current_node_id, current_attempt_id) = (
        run.current_node
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("runtime.current-node-missing"))?,
        run.current_attempt
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("runtime.current-attempt-missing"))?,
    );
    let mut matches = trace
        .iter()
        .filter(|step| step.node_id == current_node_id && step.attempt_id == current_attempt_id);
    let Some(current_index) = matches.next().map(|step| {
        trace
            .iter()
            .position(|candidate| candidate.sequence == step.sequence)
            .expect("trace step came from trace")
    }) else {
        return Err(anyhow::anyhow!("runtime.current-trace-locator-missing"));
    };
    ensure!(
        matches.next().is_none(),
        "runtime.current-trace-locator-duplicate"
    );
    Ok(trace.into_iter().take(current_index + 1).collect())
}

/// Finds the direct same-node retry successor in a canonical trace.
pub fn direct_retry_successor(
    trace: &[RoundTraceStep],
    expected_node_id: &str,
    expected_attempt_id: &str,
    current_node_id: &str,
    current_attempt_id: &str,
) -> Option<String> {
    trace.windows(2).find_map(|pair| {
        let previous = &pair[0];
        let current = &pair[1];
        (previous.node_id == expected_node_id
            && previous.attempt_id == expected_attempt_id
            && current.node_id == current_node_id
            && current.attempt_id == current_attempt_id
            && current.from_node_id.as_deref() == Some(expected_node_id)
            && current.edge_outcome.as_deref() == Some("retry"))
        .then(|| current.attempt_id.clone())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub version: String,
    /// Canonical version of the ACP storage layout owned by this attempt.
    /// Legacy node.json files omit the field and deserialize as version 0.
    #[serde(default)]
    pub acp_storage_schema_version: u32,
    pub node_id: String,
    pub node_type: NodeType,
    pub run_id: String,
    pub round_id: String,
    pub attempt_id: String,
    pub status: RunStatus,
    pub outcome: Option<NodeOutcome>,
    pub started_at: String,
    pub finished_at: Option<String>,
    #[serde(default)]
    pub manual_check_pending: bool,
    /// Identifies the currently authorized Runtime invocation for this attempt.
    /// A stop clears it and every explicit continue allocates a new value so
    /// stale background work cannot mutate a newer execution of the same attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_execution_id: Option<String>,
    pub resolved_config: ResolvedConfig,
    #[serde(default)]
    pub uuid: Option<String>,
}

/// Persists lifecycle state without allowing a stale in-memory NodeState to
/// lower an ACP storage version already committed by the migration boundary.
pub fn write_node_state(path: &Utf8Path, state: &NodeState) -> Result<()> {
    crate::storage::with_file_lock(path, || {
        let mut durable = state.clone();
        if path.exists() {
            let current = crate::storage::read_json::<NodeState>(path)?;
            durable.acp_storage_schema_version = durable
                .acp_storage_schema_version
                .max(current.acp_storage_schema_version);
        }
        crate::storage::write_json(path, &durable)
    })
}

/// Advances only the ACP storage version while preserving the latest durable
/// lifecycle fields written by Runtime. The version is monotonic and each
/// migration step calls this only after its idempotent data rewrite succeeds.
pub fn advance_node_acp_storage_schema_version(path: &Utf8Path, target: u32) -> Result<bool> {
    crate::storage::with_file_lock(path, || {
        let mut current = crate::storage::read_json::<serde_json::Value>(path)?;
        let object = current
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("acp.attempt-state-invalid"))?;
        let current_version = object
            .get("acp_storage_schema_version")
            .or_else(|| object.get("acpStorageSchemaVersion"))
            .map(|version| {
                version
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("acp.attempt-state-invalid"))
            })
            .transpose()?
            .unwrap_or_default();
        if current_version >= u64::from(target) {
            return Ok(false);
        }
        let field = if object.contains_key("node_id") {
            "acp_storage_schema_version"
        } else {
            "acpStorageSchemaVersion"
        };
        object.insert(field.to_string(), serde_json::Value::from(target));
        crate::storage::write_json(path, &current)?;
        Ok(true)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRefState {
    pub version: String,
    pub provider: String,
    pub mode: SessionMode,
    pub supports_open_session: bool,
    pub supports_continue_session: bool,
    pub continue_ref: Option<serde_json::Value>,
    pub open_command: Option<String>,
}

impl TaskState {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            version: VERSION.to_string(),
            id: id.into(),
            title: None,
            description: None,
            uuid: Some(Uuid::new_v4().simple().to_string()),
        }
    }
}

pub fn validate_task_state(state: &TaskState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported task state version");
    ensure!(!state.id.trim().is_empty(), "task id cannot be empty");
    Ok(())
}

pub fn validate_run_state(state: &RunState) -> Result<()> {
    ensure!(
        !state.is_sealed() || state.status != RunStatus::Running,
        "sealed run cannot be running"
    );
    if let Some(origin) = state.rerun.as_ref().and_then(|rerun| rerun.origin.as_ref()) {
        ensure!(
            origin.source_run_id != state.id && !origin.source_run_id.is_empty(),
            "rerun source must be a different run"
        );
        ensure!(
            origin.source_round_index > 0 && origin.inherited_trace_sequence > 0,
            "rerun inheritance boundary must be positive"
        );
        ensure!(
            !origin.operation_id.is_empty()
                && !origin.source_round_id.is_empty()
                && !origin.source_node_id.is_empty()
                && !origin.source_attempt_id.is_empty(),
            "rerun origin must contain a complete locator"
        );
    }
    if let Some(seal) = state
        .rerun
        .as_ref()
        .and_then(|rerun| rerun.sealed_by.as_ref())
    {
        ensure!(
            seal.successor_run_id != state.id
                && !seal.successor_run_id.is_empty()
                && !seal.operation_id.is_empty()
                && !seal.sealed_at.is_empty(),
            "rerun seal must identify its successor"
        );
    }
    ensure!(state.version == VERSION, "unsupported run state version");
    ensure!(
        !(state.status != RunStatus::Completed && state.outcome.is_some()),
        "non-completed run cannot have outcome"
    );
    ensure!(
        !(state.status == RunStatus::Completed && state.outcome.is_none()),
        "completed run must have outcome"
    );
    ensure!(
        !(state.status != RunStatus::Paused && state.pause_reason.is_some()),
        "non-paused run cannot have pauseReason"
    );
    ensure!(
        !(state.current_attempt.is_some() && state.current_node.is_none()),
        "currentAttempt requires currentNode"
    );
    ensure!(
        state
            .execution
            .recovery_candidate_token
            .as_deref()
            .is_none_or(|token| !token.trim().is_empty()),
        "runtime recovery candidate token cannot be empty"
    );
    if let Some(worktree) = state.worktree.as_ref() {
        ensure!(
            !worktree.path.as_str().trim().is_empty(),
            "worktree path cannot be empty"
        );
        ensure!(
            !worktree.branch.trim().is_empty(),
            "worktree branch cannot be empty"
        );
        ensure!(
            !worktree.fork_commit.trim().is_empty(),
            "worktree fork commit cannot be empty"
        );
    }
    // A zero revision exists only in in-memory legacy/test fixtures. App
    // storage access reconciles it before exposing or writing the run.
    ensure!(
        state.status != RunStatus::Paused
            || state.execution.phase == RuntimeExecutionPhase::Paused
            || state.execution.phase == RuntimeExecutionPhase::AwaitingManualCheck,
        "paused run must have paused or manual-check runtime execution phase"
    );
    ensure!(
        state.status != RunStatus::Completed
            || state.execution.phase == RuntimeExecutionPhase::Terminal,
        "completed run must have terminal runtime execution phase"
    );
    ensure!(
        !(state.current_node.is_some() && state.current_round.is_none()),
        "currentNode requires currentRound"
    );
    Ok(())
}

pub fn validate_round_state(state: &RoundState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported round state version");
    ensure!(state.index > 0, "round index must be positive");
    ensure!(
        !(state.status != RunStatus::Completed && state.outcome.is_some()),
        "non-completed round cannot have outcome"
    );
    ensure!(
        !(state.status == RunStatus::Completed && state.outcome.is_none()),
        "completed round must have outcome"
    );
    for step in &state.trace {
        ensure!(step.sequence > 0, "round trace sequence must be positive");
        ensure!(
            !step.node_id.trim().is_empty(),
            "round trace node id cannot be empty"
        );
        ensure!(
            !step.attempt_id.trim().is_empty(),
            "round trace attempt id cannot be empty"
        );
    }
    Ok(())
}

pub fn validate_node_state(state: &NodeState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported node state version");
    ensure!(
        state.acp_storage_schema_version <= CURRENT_ACP_STORAGE_SCHEMA_VERSION,
        "acp.storage-schema-version-unsupported"
    );
    ensure!(
        !(state.status != RunStatus::Completed && state.outcome.is_some()),
        "non-completed node cannot have outcome"
    );
    ensure!(
        !(state.status == RunStatus::Completed && state.outcome.is_none()),
        "completed node must have outcome"
    );
    ensure!(
        !(state.status == RunStatus::Completed && state.finished_at.is_none()),
        "completed node must have finishedAt"
    );
    Ok(())
}

pub fn validate_worker_ref_state(state: &WorkerRefState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported worker-ref version");
    ensure!(
        !state.provider.trim().is_empty(),
        "worker-ref provider cannot be empty"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node(storage_version: u32) -> NodeState {
        NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: storage_version,
            node_id: "node-a".to_string(),
            node_type: NodeType::Worker,
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            attempt_id: "attempt-001".to_string(),
            status: RunStatus::Running,
            outcome: None,
            started_at: "t0".to_string(),
            finished_at: None,
            manual_check_pending: false,
            runtime_execution_id: Some("execution-001".to_string()),
            resolved_config: ResolvedConfig::new(),
            uuid: None,
        }
    }

    fn test_run(status: RunStatus, phase: RuntimeExecutionPhase) -> RunState {
        RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status,
            outcome: None,
            started_at: "t0".to_string(),
            updated_at: "t0".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("node-a".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            rerun: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                phase,
                Some(RuntimeAttemptLocator {
                    round_id: "round-001".to_string(),
                    node_id: "node-a".to_string(),
                    attempt_id: "attempt-001".to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "t0",
            ),
        }
    }

    #[test]
    fn task_state_new_generates_uuid() {
        let task = TaskState::new("task-001");
        let uuid = task.uuid.expect("TaskState::new must generate a uuid");
        assert_eq!(uuid.len(), 32);
        assert!(uuid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn stale_lifecycle_write_cannot_lower_acp_storage_schema_version() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("node.json")).unwrap();
        let mut stale = test_node(0);
        crate::storage::write_json(&path, &stale).unwrap();

        assert!(
            advance_node_acp_storage_schema_version(&path, CURRENT_ACP_STORAGE_SCHEMA_VERSION)
                .unwrap()
        );
        stale.manual_check_pending = true;
        write_node_state(&path, &stale).unwrap();

        let durable = crate::storage::read_json::<NodeState>(&path).unwrap();
        assert_eq!(
            durable.acp_storage_schema_version,
            CURRENT_ACP_STORAGE_SCHEMA_VERSION
        );
        assert!(durable.manual_check_pending);
    }

    #[test]
    fn task_state_uuid_is_unique_per_construction() {
        let a = TaskState::new("task-a");
        let b = TaskState::new("task-b");
        assert_ne!(a.uuid, b.uuid);
    }

    #[test]
    fn runtime_lifecycle_transition_is_validated_and_revision_is_monotonic() {
        let mut run = test_run(RunStatus::Running, RuntimeExecutionPhase::StartingNode);
        let locator = run.current_execution_locator();
        RuntimeLifecycleStore::transition(
            &mut run,
            RuntimeExecutionPhase::RunningNode,
            locator.clone(),
            "t1",
        )
        .unwrap();
        RuntimeLifecycleStore::transition(
            &mut run,
            RuntimeExecutionPhase::FinalizingArtifact,
            locator,
            "t2",
        )
        .unwrap();
        assert_eq!(run.execution.revision, 3);
        assert_eq!(
            run.execution.phase,
            RuntimeExecutionPhase::FinalizingArtifact
        );
    }

    #[test]
    fn runtime_lifecycle_allows_workspace_preparation_before_provider_start() {
        let mut run = test_run(RunStatus::Running, RuntimeExecutionPhase::StartingNode);
        let locator = run.current_execution_locator();

        RuntimeLifecycleStore::transition(
            &mut run,
            RuntimeExecutionPhase::PreparingWorkspace,
            locator,
            "t1",
        )
        .unwrap();

        assert_eq!(
            run.execution.phase,
            RuntimeExecutionPhase::PreparingWorkspace
        );
        assert_eq!(run.execution.revision, 2);
    }

    #[test]
    fn runtime_lifecycle_rejects_terminal_to_running_transition() {
        let mut run = test_run(RunStatus::Completed, RuntimeExecutionPhase::Terminal);
        run.outcome = Some(RunOutcome::Success);
        let error = RuntimeLifecycleStore::transition(
            &mut run,
            RuntimeExecutionPhase::RunningNode,
            None,
            "t1",
        )
        .unwrap_err();
        assert_eq!(error.code, "runtime.execution-transition-invalid");
        assert_eq!(run.execution.phase, RuntimeExecutionPhase::Terminal);
        assert_eq!(run.execution.revision, 1);
    }

    #[test]
    fn legacy_run_migration_uses_run_status_without_acp_facts() {
        let mut run = test_run(RunStatus::Paused, RuntimeExecutionPhase::StartingNode);
        run.pause_reason = Some(PauseReason::ProcessInterrupted);
        run.execution = RuntimeExecutionState::default();
        assert!(run.reconcile_legacy_execution());
        assert_eq!(run.execution.phase, RuntimeExecutionPhase::Paused);
        assert_eq!(run.execution.revision, 1);
        assert!(!run.reconcile_legacy_execution());
    }

    #[test]
    fn run_worktree_state_round_trips_and_legacy_runs_default_to_main_workspace() {
        let mut run = test_run(
            RunStatus::Running,
            RuntimeExecutionPhase::PreparingWorkspace,
        );
        run.worktree = Some(RunWorktreeState {
            path: Utf8PathBuf::from("D:/GoldBand/worktrees/abc123"),
            branch: "gold-band/conversation/abc123".to_string(),
            fork_commit: "0123456789abcdef".to_string(),
        });

        let serialized = serde_json::to_value(&run).unwrap();
        let restored: RunState = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored.worktree, run.worktree);

        let mut legacy = serde_json::to_value(&run).unwrap();
        legacy.as_object_mut().unwrap().remove("worktree");
        let restored_legacy: RunState = serde_json::from_value(legacy).unwrap();
        assert!(restored_legacy.worktree.is_none());
    }

    #[test]
    fn runtime_recovery_candidate_token_is_optional_but_never_empty() {
        let mut run = test_run(RunStatus::Running, RuntimeExecutionPhase::RunningNode);
        assert!(validate_run_state(&run).is_ok());

        run.execution.recovery_candidate_token = Some("candidate-001".to_string());
        assert!(validate_run_state(&run).is_ok());
        let serialized = serde_json::to_value(&run).unwrap();
        assert_eq!(
            serialized["execution"]["recoveryCandidateToken"],
            "candidate-001"
        );

        run.execution.recovery_candidate_token = Some("  ".to_string());
        assert!(validate_run_state(&run).is_err());
    }

    #[test]
    fn retry_readiness_accepts_only_paused_runtime_abnormal_worker() {
        let mut run = test_run(RunStatus::Paused, RuntimeExecutionPhase::Paused);
        run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        let mut node = test_node(CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        node.status = RunStatus::Paused;
        node.runtime_execution_id = None;
        node.finished_at = Some("t1".to_string());

        assert_eq!(
            current_node_retry_readiness(&run, &node, true),
            CurrentNodeRetryReadiness::Ready
        );

        run.status = RunStatus::Running;
        assert_eq!(
            current_node_retry_readiness(&run, &node, true),
            CurrentNodeRetryReadiness::RunActive
        );
        run.status = RunStatus::Completed;
        run.outcome = Some(RunOutcome::Failure);
        assert_eq!(
            current_node_retry_readiness(&run, &node, true),
            CurrentNodeRetryReadiness::RunTerminal
        );
    }

    #[test]
    fn retry_readiness_rejects_blocked_manual_check_and_dynamic_nodes() {
        let mut run = test_run(RunStatus::Paused, RuntimeExecutionPhase::Paused);
        run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        let mut node = test_node(CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        node.status = RunStatus::Paused;
        node.runtime_execution_id = None;
        node.finished_at = Some("t1".to_string());

        node.manual_check_pending = true;
        assert_eq!(
            current_node_retry_readiness(&run, &node, true),
            CurrentNodeRetryReadiness::ManualCheckPending
        );
        node.manual_check_pending = false;
        node.node_type = NodeType::AiDynamic;
        assert_eq!(
            current_node_retry_readiness(&run, &node, true),
            CurrentNodeRetryReadiness::UnsupportedNode
        );
        node.node_type = NodeType::Worker;
        run.pause_reason = Some(PauseReason::ErrorBlocked);
        assert_eq!(
            current_node_retry_readiness(&run, &node, true),
            CurrentNodeRetryReadiness::ErrorBlocked
        );
    }

    fn trace_step(
        sequence: u32,
        node_id: &str,
        attempt_id: &str,
        from_node_id: Option<&str>,
        edge_outcome: Option<&str>,
    ) -> RoundTraceStep {
        RoundTraceStep {
            sequence,
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            from_node_id: from_node_id.map(str::to_string),
            edge_outcome: edge_outcome.map(str::to_string),
            entered_at: format!("t{sequence}"),
        }
    }

    fn current_round(trace: Vec<RoundTraceStep>) -> RoundState {
        RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: "run-001".to_string(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "t0".to_string(),
            trace,
            uuid: None,
        }
    }

    #[test]
    fn canonical_trace_truncates_uncommitted_tail_and_finds_retry_successor() {
        let mut run = test_run(RunStatus::Paused, RuntimeExecutionPhase::Paused);
        run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        let round = current_round(vec![
            trace_step(1, "node-a", "attempt-001", None, None),
            trace_step(2, "node-a", "attempt-002", Some("node-a"), Some("retry")),
            trace_step(3, "orphan", "attempt-001", Some("node-a"), Some("success")),
        ]);
        run.current_attempt = Some("attempt-002".to_string());
        let canonical = canonical_trace_for_round(&run, std::slice::from_ref(&round), &round)
            .expect("canonical trace should truncate after current locator");
        assert_eq!(canonical.len(), 2);
        assert_eq!(
            direct_retry_successor(&canonical, "node-a", "attempt-001", "node-a", "attempt-002"),
            Some("attempt-002".to_string())
        );
    }

    #[test]
    fn failed_attempt_rerun_rejects_inherited_prefix() {
        let mut run = test_run(RunStatus::Completed, RuntimeExecutionPhase::Terminal);
        run.outcome = Some(RunOutcome::Failure);
        let round = current_round(vec![trace_step(1, "node-a", "attempt-001", None, None)]);
        let mut node = test_node(CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        node.status = RunStatus::Completed;
        node.outcome = Some(NodeOutcome::Failure);
        node.runtime_execution_id = None;
        assert!(can_rerun_failed_attempt(
            &run,
            &round,
            &round.trace[0],
            &node,
            true
        ));

        run.rerun = Some(RunRerunMetadata {
            origin: Some(RunRerunOrigin {
                source_run_id: "run-source".to_string(),
                source_run_uuid: None,
                source_round_id: round.id.clone(),
                source_round_index: round.index,
                source_node_id: "node-a".to_string(),
                source_attempt_id: "attempt-001".to_string(),
                operation_id: "rerun-001".to_string(),
                inherited_trace_sequence: 1,
                source_authoring_revision: 1,
            }),
            sealed_by: None,
        });
        assert!(!can_rerun_failed_attempt(
            &run,
            &round,
            &round.trace[0],
            &node,
            true
        ));
    }

    #[test]
    fn canonical_trace_rejects_duplicate_current_locator_and_stale_locator() {
        let mut run = test_run(RunStatus::Paused, RuntimeExecutionPhase::Paused);
        run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        let duplicate = current_round(vec![
            trace_step(1, "node-a", "attempt-001", None, None),
            trace_step(2, "node-a", "attempt-001", Some("node-a"), Some("retry")),
        ]);
        let error = canonical_trace_for_round(&run, std::slice::from_ref(&duplicate), &duplicate)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("current-trace-locator-duplicate")
        );

        run.current_attempt = Some("attempt-999".to_string());
        let error = canonical_trace_for_round(&run, &[duplicate.clone()], &duplicate).unwrap_err();
        assert!(error.to_string().contains("current-trace-locator-missing"));
    }
}
