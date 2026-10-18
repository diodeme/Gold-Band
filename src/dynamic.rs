use crate::domain::{NodeOutcome, PauseReason, RunOutcome, SessionMode, VERSION};
use crate::dsl::{DynamicControlDsl, WorkflowDsl};
use crate::runtime::{RuntimeExecutionPhase, runtime_execution_transition_allowed};
use crate::runtime_error::RuntimeErrorInfo;
use anyhow::{Result, ensure};
use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DYNAMIC_COMPLETION_ARTIFACT: &str = "dynamic-node-completion";
pub const AI_DYNAMIC_RESULT_ARTIFACT: &str = "ai-dynamic-result";
pub const AI_DYNAMIC_REPORT_MANIFEST_ARTIFACT: &str = "ai-dynamic-report-manifest";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicRunStatus {
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeKind {
    Worker,
    WorkflowInvocation,
    Merge,
    Acceptance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeStatus {
    Pending,
    Ready,
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicGroupStatus {
    Open,
    MergeReady,
    Merging,
    Merged,
    Accepting,
    Accepted,
    Closed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceKind {
    Main,
    Worktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceOwnership {
    User,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceStatus {
    Active,
    Frozen,
    Merging,
    Merged,
    Released,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicRunPhase {
    #[default]
    Executing,
    PreparingWorkspace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    pub version: String,
    pub id: String,
    pub dynamic_run_id: String,
    pub kind: WorkspaceKind,
    pub ownership: WorkspaceOwnership,
    pub repo_root: Utf8PathBuf,
    pub path: Utf8PathBuf,
    pub branch: Option<String>,
    pub parent_workspace_id: Option<String>,
    pub created_by_group_id: Option<String>,
    pub fork_commit: String,
    pub checkpoint_commit: Option<String>,
    pub status: WorkspaceStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedWorkflowSnapshot {
    pub workflow_id: String,
    pub snapshot_id: String,
    pub name: String,
    pub contains_ai_dynamic: bool,
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicRunState {
    pub version: String,
    pub id: String,
    pub parent_run_id: String,
    pub parent_round_id: String,
    pub parent_node_id: String,
    pub parent_attempt_id: String,
    pub status: DynamicRunStatus,
    /// Internal non-interruptible phase. Stop requests may be accepted while
    /// workspace preparation is active, but become effective only after the
    /// workspace/graph transition reaches a consistent boundary.
    #[serde(default)]
    pub phase: DynamicRunPhase,
    pub outcome: Option<RunOutcome>,
    #[serde(default)]
    pub pause_reason: Option<PauseReason>,
    pub started_at: String,
    pub updated_at: String,
    pub control: DynamicControlDsl,
    pub allowed_workflow_snapshots: Vec<AllowedWorkflowSnapshot>,
    pub current_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicNodeState {
    pub version: String,
    /// Canonical ACP storage layout version for this leaf's fixed attempt.
    #[serde(default, skip_serializing)]
    pub acp_storage_schema_version: u32,
    pub id: String,
    pub dynamic_run_id: String,
    pub kind: DynamicNodeKind,
    pub title: String,
    pub task: String,
    pub status: DynamicNodeStatus,
    pub outcome: Option<NodeOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_error: Option<RuntimeErrorInfo>,
    /// Identifies the currently authorized Runtime invocation for this leaf.
    /// It changes on every explicit continue and is cleared when the leaf pauses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_execution_id: Option<String>,
    /// Canonical Runtime phase projected on this leaf attempt. Parallel leaves
    /// update only their own phase; graph-owned workspace transitions temporarily
    /// take over only their causal leaves. The outer Run execution remains a
    /// graph aggregate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_execution_phase: Option<RuntimeExecutionPhase>,
    /// Monotonic watermark for every Runtime lifecycle projection visible on
    /// this leaf, including leaf-local execution and graph-owned transitions.
    /// ACP turn state has its own revision domain and is not folded into this
    /// counter.
    #[serde(default)]
    pub runtime_lifecycle_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_lifecycle_updated_at: Option<String>,
    pub group_id: Option<String>,
    pub chain_id: String,
    pub depth: u32,
    pub depends_on: Vec<String>,
    pub workspace_id: String,
    pub provider: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_accept: bool,
    #[serde(default)]
    pub model: Option<String>,
    pub session_mode: SessionMode,
    pub continue_from_node_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_snapshot_id: Option<String>,
    pub child_run_id: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Hidden UUID used for metrics reporting (see docs/gold-band/observability-bus-design.md).
    #[serde(default)]
    pub uuid: Option<String>,
}

pub fn write_dynamic_node_state(path: &camino::Utf8Path, state: &DynamicNodeState) -> Result<()> {
    crate::storage::with_file_lock(path, || {
        let mut durable = serde_json::to_value(state)?;
        let mut storage_version = state.acp_storage_schema_version;
        if path.exists() {
            let current = crate::storage::read_json::<serde_json::Value>(path)?;
            let current_version = current
                .get("acpStorageSchemaVersion")
                .map(|version| {
                    version
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("acp.attempt-state-invalid"))
                        .and_then(|version| {
                            u32::try_from(version).map_err(|_| {
                                anyhow::anyhow!("acp.storage-schema-version-unsupported")
                            })
                        })
                })
                .transpose()?
                .unwrap_or_default();
            storage_version = storage_version.max(current_version);
        }
        durable["acpStorageSchemaVersion"] = serde_json::Value::from(storage_version);
        crate::storage::write_json(path, &durable)
    })
}

impl DynamicNodeState {
    pub fn advance_runtime_lifecycle_revision(&mut self, updated_at: impl Into<String>) {
        self.runtime_lifecycle_revision = self.runtime_lifecycle_revision.saturating_add(1);
        self.runtime_lifecycle_updated_at = Some(updated_at.into());
    }

    pub fn begin_runtime_execution(
        &mut self,
        execution_id: impl Into<String>,
        updated_at: impl Into<String>,
    ) {
        self.runtime_execution_id = Some(execution_id.into());
        self.runtime_execution_phase = Some(RuntimeExecutionPhase::StartingNode);
        self.advance_runtime_lifecycle_revision(updated_at);
    }

    pub fn transition_runtime_execution(
        &mut self,
        expected_execution_id: &str,
        requested_phase: RuntimeExecutionPhase,
        updated_at: impl Into<String>,
    ) -> Result<bool> {
        if self.runtime_execution_id.as_deref() != Some(expected_execution_id) {
            return Ok(false);
        }
        let current_phase = self
            .runtime_execution_phase
            .unwrap_or(RuntimeExecutionPhase::StartingNode);
        let phase = if requested_phase == RuntimeExecutionPhase::RunningNode
            && matches!(
                current_phase,
                RuntimeExecutionPhase::FinalizingArtifact
                    | RuntimeExecutionPhase::RepairingArtifact
            ) {
            current_phase
        } else {
            requested_phase
        };
        ensure!(
            runtime_execution_transition_allowed(current_phase, phase),
            "dynamic leaf runtime execution cannot transition from {current_phase:?} to {phase:?}"
        );
        self.runtime_execution_phase = Some(phase);
        self.advance_runtime_lifecycle_revision(updated_at);
        Ok(true)
    }

    pub fn pause_runtime_execution(&mut self, updated_at: impl Into<String>) {
        self.runtime_execution_id = None;
        self.runtime_execution_phase = Some(RuntimeExecutionPhase::Paused);
        self.advance_runtime_lifecycle_revision(updated_at);
    }

    pub fn complete_runtime_execution(&mut self, updated_at: impl Into<String>) {
        self.runtime_execution_id = None;
        self.runtime_execution_phase = Some(RuntimeExecutionPhase::Terminal);
        self.advance_runtime_lifecycle_revision(updated_at);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicGroupState {
    pub version: String,
    pub id: String,
    pub dynamic_run_id: String,
    pub status: DynamicGroupStatus,
    pub depth: u32,
    pub parent_group_id: Option<String>,
    pub root_node_ids: Vec<String>,
    pub terminal_node_ids: Vec<String>,
    pub target_workspace_id: String,
    pub child_workspace_ids: Vec<String>,
    pub merge_node_id: Option<String>,
    pub acceptance_node_id: Option<String>,
    pub created_by_node_id: String,
    pub merge: DynamicAgentTaskSpec,
    pub acceptance: DynamicAgentTaskSpec,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct DynamicProposalValidationError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl DynamicProposalValidationError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path: None,
            actual: None,
            expected: None,
            allowed_values: Vec::new(),
            suggestion: None,
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicProposalState {
    pub version: String,
    pub id: String,
    pub dynamic_run_id: String,
    pub source_node_id: String,
    pub artifact_path: Utf8PathBuf,
    pub raw_output_path: Utf8PathBuf,
    pub parsed: serde_json::Value,
    pub validation_status: DynamicProposalValidationStatus,
    pub validation_errors: Vec<DynamicProposalValidationError>,
    pub materialized_event_ids: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicProposalValidationStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicGraphState {
    pub version: String,
    pub run: DynamicRunState,
    pub nodes: Vec<DynamicNodeState>,
    pub groups: Vec<DynamicGroupState>,
    pub workspaces: Vec<WorkspaceState>,
    pub proposals: Vec<DynamicProposalState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicResult {
    pub version: String,
    pub kind: AiDynamicResultKind,
    pub outcome: RunOutcome,
    pub summary: String,
    pub source_node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_group_id: Option<String>,
    pub report_manifest: AiDynamicReportManifestRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiDynamicResultKind {
    AiDynamicResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicReportManifestRef {
    pub path: Utf8PathBuf,
    pub format_version: String,
    pub unit_count: usize,
    pub attachment_count: usize,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicReportManifest {
    pub version: String,
    pub kind: AiDynamicReportManifestKind,
    pub dynamic_run_id: String,
    pub root_node_id: String,
    pub outcome: RunOutcome,
    pub generated_at: String,
    pub nodes: Vec<AiDynamicReportNode>,
    pub groups: Vec<AiDynamicReportGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiDynamicReportManifestKind {
    AiDynamicReportManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicReportNode {
    pub id: String,
    pub kind: DynamicNodeKind,
    pub title: String,
    pub task: String,
    pub status: DynamicNodeStatus,
    pub outcome: Option<NodeOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawned_by_node_id: Option<String>,
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub chain_id: String,
    pub workspace: AiDynamicWorkspaceRef,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<AiDynamicReportNext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_workflow: Option<AiDynamicChildWorkflowRef>,
    pub attachments: Vec<AiDynamicReportAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AiDynamicReportNext {
    End,
    Single {
        #[serde(rename = "nodeId")]
        node_id: String,
    },
    Fanout {
        #[serde(rename = "groupId")]
        group_id: String,
        #[serde(rename = "rootNodeIds")]
        root_node_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicWorkspaceRef {
    pub id: String,
    pub kind: WorkspaceKind,
    pub path: Utf8PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicChildWorkflowRef {
    pub workflow_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicReportAttachment {
    pub name: String,
    pub path: Utf8PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicReportGroup {
    pub id: String,
    pub status: DynamicGroupStatus,
    pub depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_group_id: Option<String>,
    pub created_by_node_id: String,
    pub root_node_ids: Vec<String>,
    pub terminal_node_ids: Vec<String>,
    pub target_workspace_id: String,
    pub child_workspace_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_node_id: Option<String>,
    pub merge: AiDynamicTaskRef,
    pub acceptance: AiDynamicTaskRef,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicTaskRef {
    pub title: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicCoordinationSnapshot {
    pub version: String,
    pub kind: AiDynamicCoordinationSnapshotKind,
    pub dynamic_run_id: String,
    pub generated_at: String,
    pub workstreams: Vec<AiDynamicCoordinationWorkstream>,
    pub groups: Vec<AiDynamicCoordinationGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiDynamicCoordinationSnapshotKind {
    AiDynamicCoordinationSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiDynamicWorkstreamStatus {
    Pending,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicCoordinationWorkstream {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_workstream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_group_id: Option<String>,
    pub title: String,
    pub goal: String,
    pub status: AiDynamicWorkstreamStatus,
    pub workspace: AiDynamicWorkspaceRef,
    pub child_group_ids: Vec<String>,
    pub steps: Vec<AiDynamicCoordinationStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicCoordinationStep {
    pub node_id: String,
    pub kind: DynamicNodeKind,
    pub title: String,
    pub task: String,
    pub status: DynamicNodeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<NodeOutcome>,
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicCoordinationGroup {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_workstream_id: Option<String>,
    pub branch_workstream_ids: Vec<String>,
    pub phase: DynamicGroupStatus,
    pub target_workspace: AiDynamicWorkspaceRef,
    pub merge: AiDynamicCoordinationGroupStage,
    pub acceptance: AiDynamicCoordinationGroupStage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDynamicCoordinationGroupStage {
    pub title: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DynamicNodeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<NodeOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicNodeSpec {
    pub id: String,
    pub kind: DynamicNodeSpecKind,
    pub title: String,
    pub task: String,
    pub provider: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub session_mode: SessionMode,
    #[serde(default)]
    pub continue_from_node_id: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeSpecKind {
    Worker,
    WorkflowInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicAgentTaskSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicNodeCompletion {
    pub version: String,
    pub kind: DynamicNodeCompletionKind,
    pub status: DynamicCompletionStatus,
    pub summary: String,
    pub next: DynamicNext,
    #[serde(default)]
    pub source: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeCompletionKind {
    DynamicNodeCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicCompletionStatus {
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DynamicNext {
    End,
    Single {
        node: DynamicNodeSpec,
    },
    Fanout {
        #[serde(rename = "groupId")]
        group_id: String,
        nodes: Vec<DynamicNodeSpec>,
        merge: DynamicAgentTaskSpec,
        acceptance: DynamicAgentTaskSpec,
    },
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::New
    }
}

/// Long completion text is submitted as a file in the node's attempt
/// attachments directory: the wire JSON carries `<field>Path`, and the runtime
/// freezes the file content into the canonical `<field>` before acceptance.
pub const DYNAMIC_HANDOFF_SUMMARY_FIELD: &str = "summary";
pub const DYNAMIC_HANDOFF_TASK_FIELD: &str = "task";
pub const DYNAMIC_HANDOFF_SUMMARY_PATH_FIELD: &str = "summaryPath";
pub const DYNAMIC_HANDOFF_TASK_PATH_FIELD: &str = "taskPath";
pub const DYNAMIC_HANDOFF_FILE_MAX_BYTES: u64 = 256 * 1024;
pub const DYNAMIC_HANDOFF_INVALID_PATH: &str = "dynamic.handoff-file.invalid-path";
pub const DYNAMIC_HANDOFF_OUTSIDE_ATTACHMENTS: &str = "dynamic.handoff-file.outside-attachments";
pub const DYNAMIC_HANDOFF_NOT_FOUND: &str = "dynamic.handoff-file.not-found";
pub const DYNAMIC_HANDOFF_EMPTY: &str = "dynamic.handoff-file.empty";
pub const DYNAMIC_HANDOFF_NOT_UTF8: &str = "dynamic.handoff-file.not-utf8";
pub const DYNAMIC_HANDOFF_TOO_LARGE: &str = "dynamic.handoff-file.too-large";

/// Replaces every `summaryPath` / `taskPath` in a schema-valid completion
/// submission with the referenced file content. Paths are relative to the
/// submitting attempt's attachments directory and may not leave it.
pub fn resolve_dynamic_completion_handoff_files(
    submission: &serde_json::Value,
    attachments_dir: &camino::Utf8Path,
) -> std::result::Result<serde_json::Value, Vec<DynamicProposalValidationError>> {
    let mut resolved = submission.clone();
    let mut errors = Vec::new();
    let mut slots = vec![(
        String::new(),
        DYNAMIC_HANDOFF_SUMMARY_PATH_FIELD,
        DYNAMIC_HANDOFF_SUMMARY_FIELD,
    )];
    let next = submission.get("next");
    for stage in ["node", "merge", "acceptance"] {
        if next.and_then(|next| next.get(stage)).is_some() {
            slots.push((
                format!("/next/{stage}"),
                DYNAMIC_HANDOFF_TASK_PATH_FIELD,
                DYNAMIC_HANDOFF_TASK_FIELD,
            ));
        }
    }
    let branch_count = next
        .and_then(|next| next.get("nodes"))
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    for index in 0..branch_count {
        slots.push((
            format!("/next/nodes/{index}"),
            DYNAMIC_HANDOFF_TASK_PATH_FIELD,
            DYNAMIC_HANDOFF_TASK_FIELD,
        ));
    }
    for (pointer, path_field, text_field) in slots {
        let Some(owner) = resolved
            .pointer_mut(&pointer)
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let Some(relative) = owner
            .get(path_field)
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        let json_path = dynamic_handoff_json_path(&pointer, path_field);
        match read_dynamic_handoff_file(attachments_dir, &relative) {
            Ok(text) => {
                owner.remove(path_field);
                owner.insert(text_field.to_string(), serde_json::Value::String(text));
            }
            Err((code, message)) => {
                let mut error = DynamicProposalValidationError::new(
                    code,
                    message,
                    serde_json::json!({
                        "field": path_field,
                        "file": relative,
                        "attachmentsDir": attachments_dir,
                        "maxBytes": DYNAMIC_HANDOFF_FILE_MAX_BYTES,
                    }),
                );
                error.path = Some(json_path);
                error.actual = Some(relative);
                error.expected = Some(
                    "relative path of a non-empty UTF-8 file written inside the attachments directory"
                        .to_string(),
                );
                error.suggestion = Some(format!(
                    "write the text to a file inside {attachments_dir} and set `{path_field}` to its path relative to that directory; keep the other fields unchanged"
                ));
                errors.push(error);
            }
        }
    }
    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

fn dynamic_handoff_json_path(pointer: &str, field: &str) -> String {
    let mut path = String::new();
    for segment in pointer.split('/').filter(|segment| !segment.is_empty()) {
        if segment.chars().all(|ch| ch.is_ascii_digit()) {
            path.push_str(&format!("[{segment}]"));
        } else {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(segment);
        }
    }
    if path.is_empty() {
        field.to_string()
    } else {
        format!("{path}.{field}")
    }
}

fn read_dynamic_handoff_file(
    attachments_dir: &camino::Utf8Path,
    relative: &str,
) -> std::result::Result<String, (&'static str, String)> {
    let candidate = camino::Utf8Path::new(relative);
    let lexically_contained = !relative.trim().is_empty()
        && candidate.components().all(|component| {
            matches!(
                component,
                camino::Utf8Component::Normal(_) | camino::Utf8Component::CurDir
            )
        });
    if !lexically_contained {
        return Err((
            DYNAMIC_HANDOFF_INVALID_PATH,
            format!("`{relative}` must be a relative path without `..`"),
        ));
    }
    let full = attachments_dir.join(candidate);
    let not_found = || {
        (
            DYNAMIC_HANDOFF_NOT_FOUND,
            format!("no file exists at `{relative}` in the attachments directory"),
        )
    };
    let real = std::fs::canonicalize(full.as_std_path()).map_err(|_| not_found())?;
    let root = std::fs::canonicalize(attachments_dir.as_std_path()).map_err(|_| not_found())?;
    if !real.starts_with(&root) {
        return Err((
            DYNAMIC_HANDOFF_OUTSIDE_ATTACHMENTS,
            format!("`{relative}` resolves outside the attachments directory"),
        ));
    }
    let metadata = std::fs::metadata(&real).map_err(|_| not_found())?;
    if !metadata.is_file() {
        return Err(not_found());
    }
    if metadata.len() > DYNAMIC_HANDOFF_FILE_MAX_BYTES {
        return Err((
            DYNAMIC_HANDOFF_TOO_LARGE,
            format!(
                "`{relative}` has {} bytes; the limit is {DYNAMIC_HANDOFF_FILE_MAX_BYTES}",
                metadata.len()
            ),
        ));
    }
    let bytes = std::fs::read(&real).map_err(|_| not_found())?;
    let text = String::from_utf8(bytes).map_err(|_| {
        (
            DYNAMIC_HANDOFF_NOT_UTF8,
            format!("`{relative}` is not valid UTF-8 text"),
        )
    })?;
    if text.trim().is_empty() {
        return Err((DYNAMIC_HANDOFF_EMPTY, format!("`{relative}` is empty")));
    }
    Ok(text)
}

pub fn dynamic_completion_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(DynamicNodeCompletion))
        .expect("dynamic completion JSON schema serializes")
}

#[derive(Debug, Clone, Default)]
pub struct DynamicCompletionSchemaPolicy {
    pub provider_required: bool,
    pub node_model_required: bool,
    pub agent_task_model_required: bool,
    pub agent_task_model_visible: bool,
    pub provider_ids: Vec<String>,
    pub model_names: Vec<String>,
    pub profile_ids: Vec<String>,
    pub workflow_ids: Vec<String>,
    pub max_fanout: u32,
}

pub fn dynamic_completion_effective_schema(
    policy: &DynamicCompletionSchemaPolicy,
) -> serde_json::Value {
    let mut schema = dynamic_completion_schema();
    patch_dynamic_completion_root_schema(&mut schema);
    reset_schema_definitions(&mut schema);
    set_schema_definition(
        &mut schema,
        "SessionMode",
        enum_string_schema(["new", "continue"]),
    );
    set_schema_definition(
        &mut schema,
        "DynamicNodeSpec",
        dynamic_node_spec_schema(policy),
    );
    set_schema_definition(
        &mut schema,
        "DynamicAgentTaskSpec",
        dynamic_agent_task_spec_schema(policy),
    );
    set_schema_definition(&mut schema, "DynamicNext", dynamic_next_schema(policy));
    schema
}

fn patch_dynamic_completion_root_schema(schema: &mut serde_json::Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    object.insert(
        "required".to_string(),
        serde_json::json!([
            "version",
            "kind",
            "status",
            DYNAMIC_HANDOFF_SUMMARY_PATH_FIELD,
            "next"
        ]),
    );
    object.insert("additionalProperties".to_string(), serde_json::json!(false));
    let Some(properties) = object
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    properties.remove("source");
    properties.remove(DYNAMIC_HANDOFF_SUMMARY_FIELD);
    properties.insert(
        DYNAMIC_HANDOFF_SUMMARY_PATH_FIELD.to_string(),
        string_schema(),
    );
    properties.insert(
        "version".to_string(),
        enum_string_schema([VERSION.to_string()]),
    );
    properties.insert(
        "kind".to_string(),
        enum_string_schema([DYNAMIC_COMPLETION_ARTIFACT.to_string()]),
    );
    properties.insert("status".to_string(), enum_string_schema(["success"]));
}

fn schema_definitions_mut(
    schema: &mut serde_json::Value,
) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
    if schema.get("definitions").is_some() {
        schema
            .get_mut("definitions")
            .and_then(serde_json::Value::as_object_mut)
    } else {
        schema
            .get_mut("$defs")
            .and_then(serde_json::Value::as_object_mut)
    }
}

fn reset_schema_definitions(schema: &mut serde_json::Value) {
    if let Some(definitions) = schema_definitions_mut(schema) {
        definitions.clear();
    }
}

fn set_schema_definition(schema: &mut serde_json::Value, name: &str, value: serde_json::Value) {
    if let Some(definitions) = schema_definitions_mut(schema) {
        definitions.insert(name.to_string(), value);
    }
}

fn schema_ref(name: &str) -> serde_json::Value {
    serde_json::json!({
        "$ref": format!("#/definitions/{name}")
    })
}

fn string_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "string",
        "minLength": 1
    })
}

fn enum_string_schema(values: impl IntoIterator<Item = impl Into<String>>) -> serde_json::Value {
    let values = values.into_iter().map(Into::into).collect::<Vec<String>>();
    serde_json::json!({
        "type": "string",
        "enum": values
    })
}

fn optional_enum_or_string_schema(values: &[String]) -> serde_json::Value {
    if values.is_empty() {
        string_schema()
    } else {
        enum_string_schema(values.iter().cloned())
    }
}

fn forbidden_properties_schema(fields: &[&str]) -> serde_json::Value {
    let properties = fields
        .iter()
        .map(|field| ((*field).to_string(), serde_json::json!(false)))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({ "properties": properties })
}

fn conditional_schema(
    discriminator_field: &str,
    discriminator_value: &str,
    required: &[&str],
    forbidden: &[&str],
) -> serde_json::Value {
    let mut discriminator_properties = serde_json::Map::new();
    discriminator_properties.insert(
        discriminator_field.to_string(),
        serde_json::json!({ "enum": [discriminator_value] }),
    );
    let if_schema = serde_json::json!({
        "required": [discriminator_field],
        "properties": discriminator_properties,
    });
    let mut then_schema = serde_json::Map::new();
    if !required.is_empty() {
        then_schema.insert("required".to_string(), serde_json::json!(required));
    }
    if !forbidden.is_empty() {
        then_schema.insert(
            "properties".to_string(),
            forbidden_properties_schema(forbidden)
                .get("properties")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        );
    }
    serde_json::json!({
        "if": if_schema,
        "then": serde_json::Value::Object(then_schema)
    })
}

fn dynamic_node_spec_schema(policy: &DynamicCompletionSchemaPolicy) -> serde_json::Value {
    let mut worker_required = vec!["id", "kind", "title", DYNAMIC_HANDOFF_TASK_PATH_FIELD];
    if policy.provider_required {
        worker_required.push("provider");
    }
    if policy.node_model_required {
        worker_required.push("model");
    }
    let mut worker_forbidden = vec!["workflowId", "permissionMode"];
    if !policy.provider_required {
        worker_forbidden.push("provider");
    }
    if !policy.node_model_required {
        worker_forbidden.push("model");
    }
    serde_json::json!({
        "type": "object",
        "required": ["id", "kind", "title", DYNAMIC_HANDOFF_TASK_PATH_FIELD],
        "additionalProperties": false,
        "properties": {
            "id": string_schema(),
            "kind": enum_string_schema(["worker", "workflow-invocation"]),
            "title": string_schema(),
            DYNAMIC_HANDOFF_TASK_PATH_FIELD: string_schema(),
            "provider": optional_enum_or_string_schema(&policy.provider_ids),
            "profile": optional_enum_or_string_schema(&policy.profile_ids),
            "model": optional_enum_or_string_schema(&policy.model_names),
            "sessionMode": schema_ref("SessionMode"),
            "continueFromNodeId": string_schema(),
            "dependsOn": {
                "type": "array",
                "items": string_schema()
            },
            "workflowId": optional_enum_or_string_schema(&policy.workflow_ids)
        },
        "allOf": [
            conditional_schema(
                "kind",
                "worker",
                &worker_required,
                &worker_forbidden,
            ),
            conditional_schema(
                "kind",
                "workflow-invocation",
                &["id", "kind", "title", DYNAMIC_HANDOFF_TASK_PATH_FIELD, "workflowId"],
                &["provider", "profile", "model", "permissionMode"],
            )
        ]
    })
}

fn dynamic_agent_task_spec_schema(policy: &DynamicCompletionSchemaPolicy) -> serde_json::Value {
    let mut required = vec!["title", DYNAMIC_HANDOFF_TASK_PATH_FIELD];
    if policy.agent_task_model_required {
        required.push("model");
    }
    let mut forbidden = vec!["provider"];
    if policy.agent_task_model_visible && !policy.agent_task_model_required {
        forbidden.push("model");
    }
    let mut properties = serde_json::Map::from_iter([
        ("title".to_string(), string_schema()),
        (
            "provider".to_string(),
            optional_enum_or_string_schema(&policy.provider_ids),
        ),
        (DYNAMIC_HANDOFF_TASK_PATH_FIELD.to_string(), string_schema()),
    ]);
    if policy.agent_task_model_visible {
        properties.insert(
            "model".to_string(),
            optional_enum_or_string_schema(&policy.model_names),
        );
    }
    let mut schema = serde_json::json!({
        "type": "object",
        "required": required,
        "additionalProperties": false,
        "properties": properties
    });
    if !forbidden.is_empty() {
        if let Some(object) = schema.as_object_mut() {
            object.insert(
                "allOf".to_string(),
                serde_json::json!([forbidden_properties_schema(&forbidden)]),
            );
        }
    }
    schema
}

fn dynamic_next_schema(policy: &DynamicCompletionSchemaPolicy) -> serde_json::Value {
    let max_items = u64::from(policy.max_fanout.max(1));
    serde_json::json!({
        "type": "object",
        "required": ["type"],
        "additionalProperties": false,
        "properties": {
            "type": enum_string_schema(["end", "single", "fanout"]),
            "node": schema_ref("DynamicNodeSpec"),
            "groupId": string_schema(),
            "nodes": {
                "type": "array",
                "minItems": 2,
                "maxItems": max_items,
                "items": schema_ref("DynamicNodeSpec")
            },
            "merge": schema_ref("DynamicAgentTaskSpec"),
            "acceptance": schema_ref("DynamicAgentTaskSpec")
        },
        "allOf": [
            conditional_schema(
                "type",
                "end",
                &["type"],
                &["node", "groupId", "nodes", "merge", "acceptance"],
            ),
            conditional_schema(
                "type",
                "single",
                &["type", "node"],
                &["groupId", "nodes", "merge", "acceptance"],
            ),
            conditional_schema(
                "type",
                "fanout",
                &["type", "groupId", "nodes", "merge", "acceptance"],
                &["node"],
            )
        ]
    })
}

pub fn dynamic_leaf_is_active(status: DynamicNodeStatus) -> bool {
    matches!(
        status,
        DynamicNodeStatus::Ready | DynamicNodeStatus::Running
    )
}

/// The outer AI-DYNAMIC Runtime may temporarily own the phase projected on a
/// leaf attempt while the leaf keeps its own lifecycle ordering watermark.
/// Keep this ownership predicate in the core model so transition producers and
/// desktop lifecycle consumers cannot drift apart.
pub fn dynamic_runtime_owns_leaf_projection(
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> bool {
    if graph.run.status != DynamicRunStatus::Running {
        return false;
    }
    if graph.run.phase == DynamicRunPhase::PreparingWorkspace
        && node.runtime_execution_phase == Some(RuntimeExecutionPhase::PreparingWorkspace)
    {
        return true;
    }
    graph.run.current_node_ids.is_empty()
        && graph.nodes.last().is_some_and(|last| {
            last.id == node.id
                && node.status == DynamicNodeStatus::Completed
                && node.outcome.is_some()
        })
}

pub fn refresh_dynamic_current_leaf_ids(graph: &mut DynamicGraphState) {
    graph.run.current_node_ids = graph
        .nodes
        .iter()
        .filter(|node| dynamic_leaf_is_active(node.status))
        .map(|node| node.id.clone())
        .collect();
}

pub fn dynamic_graph_has_active_leaf(graph: &DynamicGraphState) -> bool {
    graph
        .nodes
        .iter()
        .any(|node| dynamic_leaf_is_active(node.status))
}

pub fn validate_dynamic_run_state(state: &DynamicRunState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported dynamic run version");
    ensure!(
        !state.id.trim().is_empty(),
        "dynamic run id cannot be empty"
    );
    ensure!(
        !state.parent_run_id.trim().is_empty(),
        "dynamic run parentRunId cannot be empty"
    );
    ensure!(
        !(state.status != DynamicRunStatus::Completed && state.outcome.is_some()),
        "non-completed dynamic run cannot have outcome"
    );
    ensure!(
        !(state.status == DynamicRunStatus::Completed && state.outcome.is_none()),
        "completed dynamic run must have outcome"
    );
    ensure!(
        !(state.status != DynamicRunStatus::Paused && state.pause_reason.is_some()),
        "non-paused dynamic run cannot have pauseReason"
    );
    ensure!(
        state.status == DynamicRunStatus::Running || state.phase == DynamicRunPhase::Executing,
        "non-running dynamic run cannot prepare workspace"
    );
    Ok(())
}

pub fn validate_dynamic_node_state(state: &DynamicNodeState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported dynamic node version");
    ensure!(
        !state.id.trim().is_empty(),
        "dynamic node id cannot be empty"
    );
    ensure!(
        !state.dynamic_run_id.trim().is_empty(),
        "dynamic node dynamicRunId cannot be empty"
    );
    ensure!(
        !state.workspace_id.trim().is_empty(),
        "dynamic node workspaceId cannot be empty"
    );
    ensure!(
        !state.title.trim().is_empty(),
        "dynamic node title cannot be empty"
    );
    ensure!(
        !state.task.trim().is_empty(),
        "dynamic node task cannot be empty"
    );
    ensure!(
        !(state.status != DynamicNodeStatus::Completed && state.outcome.is_some()),
        "non-completed dynamic node cannot have outcome"
    );
    ensure!(
        !(state.status == DynamicNodeStatus::Completed && state.outcome.is_none()),
        "completed dynamic node must have outcome"
    );
    ensure!(
        state.status == DynamicNodeStatus::Paused
            || (state.pause_reason.is_none() && state.runtime_error.is_none()),
        "non-paused dynamic node cannot have pauseReason or runtimeError"
    );
    ensure!(
        state.runtime_error.is_none() || state.pause_reason.is_some(),
        "dynamic node runtimeError requires pauseReason"
    );
    ensure!(
        state.runtime_execution_id.is_none()
            || matches!(
                state.runtime_execution_phase,
                Some(
                    RuntimeExecutionPhase::StartingNode
                        | RuntimeExecutionPhase::RunningNode
                        | RuntimeExecutionPhase::FinalizingArtifact
                        | RuntimeExecutionPhase::RepairingArtifact
                )
            ),
        "active dynamic leaf execution requires an active phase"
    );
    ensure!(
        !matches!(
            state.runtime_execution_phase,
            Some(RuntimeExecutionPhase::Paused | RuntimeExecutionPhase::Terminal)
        ) || state.runtime_execution_id.is_none(),
        "inactive dynamic leaf execution phase cannot retain an execution id"
    );
    ensure!(
        (state.runtime_lifecycle_revision == 0) == state.runtime_lifecycle_updated_at.is_none(),
        "dynamic leaf Runtime lifecycle revision and updatedAt must be initialized together"
    );
    ensure!(
        state
            .runtime_lifecycle_updated_at
            .as_deref()
            .is_none_or(|updated_at| !updated_at.trim().is_empty()),
        "dynamic leaf Runtime lifecycle updatedAt cannot be empty"
    );
    Ok(())
}

pub fn validate_dynamic_group_state(state: &DynamicGroupState) -> Result<()> {
    ensure!(
        state.version == VERSION,
        "unsupported dynamic group version"
    );
    ensure!(
        !state.id.trim().is_empty(),
        "dynamic group id cannot be empty"
    );
    if let Some(parent_group_id) = state.parent_group_id.as_deref() {
        ensure!(
            !parent_group_id.trim().is_empty(),
            "dynamic group parentGroupId cannot be empty"
        );
        ensure!(
            parent_group_id != state.id,
            "dynamic group cannot reference itself as parent"
        );
    }
    ensure!(
        !state.root_node_ids.is_empty(),
        "dynamic group must have root nodes"
    );
    ensure!(
        !state.target_workspace_id.trim().is_empty(),
        "dynamic group targetWorkspaceId cannot be empty"
    );
    ensure!(
        state.child_workspace_ids.len() == state.root_node_ids.len(),
        "dynamic group must assign one child workspace per root node"
    );
    Ok(())
}

pub fn validate_workspace_state(state: &WorkspaceState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported workspace version");
    ensure!(!state.id.trim().is_empty(), "workspace id cannot be empty");
    ensure!(
        !state.dynamic_run_id.trim().is_empty(),
        "workspace dynamicRunId cannot be empty"
    );
    ensure!(
        !state.path.as_str().is_empty(),
        "workspace path cannot be empty"
    );
    ensure!(
        !state.fork_commit.trim().is_empty(),
        "workspace forkCommit cannot be empty"
    );
    match state.kind {
        WorkspaceKind::Main => {
            ensure!(
                state.ownership == WorkspaceOwnership::User,
                "main workspace must be user-owned"
            );
            ensure!(
                state.branch.is_none(),
                "main workspace branch is not runtime-owned"
            );
            ensure!(
                state.parent_workspace_id.is_none(),
                "main workspace cannot have a parent"
            );
        }
        WorkspaceKind::Worktree => {
            ensure!(
                state.ownership == WorkspaceOwnership::Runtime,
                "worktree must be runtime-owned"
            );
            ensure!(
                state
                    .branch
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "worktree branch is required"
            );
            ensure!(
                state
                    .parent_workspace_id
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "worktree parentWorkspaceId is required"
            );
        }
    }
    Ok(())
}

pub fn validate_workspace_topology(graph: &DynamicGraphState) -> Result<()> {
    let workspace_ids = graph
        .workspaces
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    ensure!(
        workspace_ids.len() == graph.workspaces.len(),
        "workspace ids must be unique"
    );
    for node in &graph.nodes {
        ensure!(
            workspace_ids.contains(node.workspace_id.as_str()),
            "node `{}` references unknown workspace `{}`",
            node.id,
            node.workspace_id
        );
    }
    for group in &graph.groups {
        ensure!(
            workspace_ids.contains(group.target_workspace_id.as_str()),
            "group `{}` references unknown target workspace",
            group.id
        );
        for child_workspace_id in &group.child_workspace_ids {
            let child = graph
                .workspaces
                .iter()
                .find(|workspace| workspace.id == *child_workspace_id)
                .ok_or_else(|| anyhow::anyhow!("group child workspace is missing"))?;
            ensure!(
                child.parent_workspace_id.as_deref() == Some(group.target_workspace_id.as_str()),
                "group child workspace must descend from target workspace"
            );
            ensure!(
                child.created_by_group_id.as_deref() == Some(group.id.as_str()),
                "group child workspace must reference its creating group"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dynamic_node_with_storage_version(storage_version: u32) -> DynamicNodeState {
        serde_json::from_value(serde_json::json!({
            "version": VERSION,
            "acpStorageSchemaVersion": storage_version,
            "id": "worker",
            "dynamicRunId": "dynamic-run-001",
            "kind": "worker",
            "title": "Worker",
            "task": "Run worker",
            "status": "running",
            "chainId": "worker",
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "sessionMode": "new"
        }))
        .unwrap()
    }

    #[test]
    fn dynamic_node_storage_schema_is_only_persisted_in_leaf_node_and_is_monotonic() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("node.json")).unwrap();
        let current =
            dynamic_node_with_storage_version(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        assert!(
            serde_json::to_value(&current)
                .unwrap()
                .get("acpStorageSchemaVersion")
                .is_none()
        );
        write_dynamic_node_state(&path, &current).unwrap();

        let mut stale = dynamic_node_with_storage_version(0);
        stale.title = "Updated by stale lifecycle state".to_string();
        write_dynamic_node_state(&path, &stale).unwrap();

        let durable = crate::storage::read_json::<serde_json::Value>(&path).unwrap();
        assert_eq!(
            durable
                .get("acpStorageSchemaVersion")
                .and_then(serde_json::Value::as_u64),
            Some(u64::from(
                crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION
            ))
        );
        assert_eq!(
            durable.get("title").and_then(serde_json::Value::as_str),
            Some("Updated by stale lifecycle state")
        );
    }

    #[test]
    fn dynamic_leaf_runtime_lifecycle_watermark_advances_for_local_and_graph_owned_changes() {
        let mut node =
            dynamic_node_with_storage_version(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);

        node.begin_runtime_execution("execution-1", "2026-08-24T00:00:01Z");
        assert_eq!(node.runtime_lifecycle_revision, 1);
        node.transition_runtime_execution(
            "execution-1",
            RuntimeExecutionPhase::RunningNode,
            "2026-08-24T00:00:02Z",
        )
        .unwrap();
        assert_eq!(node.runtime_lifecycle_revision, 2);
        node.advance_runtime_lifecycle_revision("2026-08-24T00:00:03Z");
        assert_eq!(node.runtime_lifecycle_revision, 3);
        assert_eq!(
            node.runtime_lifecycle_updated_at.as_deref(),
            Some("2026-08-24T00:00:03Z")
        );
        validate_dynamic_node_state(&node).unwrap();

        let durable = serde_json::to_value(&node).unwrap();
        assert_eq!(durable["runtimeLifecycleRevision"], 3);
        assert_eq!(durable["runtimeLifecycleUpdatedAt"], "2026-08-24T00:00:03Z");
        assert!(durable.get("runtimeExecutionRevision").is_none());
        assert!(durable.get("runtimeExecutionUpdatedAt").is_none());
    }

    #[test]
    fn provider_routing_contract_rejects_model_and_permission_mode() {
        let schema = dynamic_completion_effective_schema(&DynamicCompletionSchemaPolicy {
            provider_required: true,
            node_model_required: false,
            agent_task_model_required: false,
            agent_task_model_visible: false,
            provider_ids: vec!["claude-acp".to_string(), "codex-acp".to_string()],
            max_fanout: 5,
            ..Default::default()
        });
        let compiled = jsonschema::JSONSchema::compile(&schema).unwrap();
        let proposal = serde_json::json!({
            "version": VERSION,
            "kind": DYNAMIC_COMPLETION_ARTIFACT,
            "status": "success",
            "summaryPath": "handoff/summary.md",
            "next": {
                "type": "single",
                "node": {
                    "id": "worker-1",
                    "kind": "worker",
                    "title": "Worker",
                    "taskPath": "handoff/worker-1.md",
                    "provider": "codex-acp"
                }
            }
        });
        assert!(compiled.is_valid(&proposal));

        let mut with_model = proposal.clone();
        with_model["next"]["node"]["model"] = serde_json::json!("gpt-5.6-sol");
        assert!(!compiled.is_valid(&with_model));

        let mut with_permission = proposal;
        with_permission["next"]["node"]["permissionMode"] = serde_json::json!("agent-full-access");
        assert!(!compiled.is_valid(&with_permission));

        let mut with_workspace = serde_json::json!({
            "version": VERSION,
            "kind": DYNAMIC_COMPLETION_ARTIFACT,
            "status": "success",
            "summaryPath": "handoff/summary.md",
            "next": {
                "type": "single",
                "node": {
                    "id": "worker-1",
                    "kind": "worker",
                    "title": "Worker",
                    "taskPath": "handoff/worker-1.md",
                    "provider": "codex-acp"
                }
            }
        });
        with_workspace["next"]["node"]["workspace"] = serde_json::json!({ "mode": "worktree" });
        assert!(!compiled.is_valid(&with_workspace));
    }

    fn handoff_attachments_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp.path().join("attachments")).unwrap();
        std::fs::create_dir_all(dir.join("handoff")).unwrap();
        (temp, dir)
    }

    fn fanout_submission() -> serde_json::Value {
        serde_json::json!({
            "version": VERSION,
            "kind": DYNAMIC_COMPLETION_ARTIFACT,
            "status": "success",
            "summaryPath": "handoff/summary.md",
            "next": {
                "type": "fanout",
                "groupId": "g",
                "nodes": [
                    { "id": "a", "kind": "worker", "title": "A", "taskPath": "handoff/a.md" },
                    { "id": "b", "kind": "worker", "title": "B", "taskPath": "./handoff/b.md" }
                ],
                "merge": { "title": "Merge", "taskPath": "handoff/merge.md" },
                "acceptance": { "title": "Accept", "taskPath": "handoff/accept.md" }
            }
        })
    }

    #[test]
    fn handoff_files_resolve_into_canonical_completion_text() {
        let (_temp, dir) = handoff_attachments_dir();
        for (name, text) in [
            ("summary", "split work"),
            ("a", "task A"),
            ("b", "task B"),
            ("merge", "merge both"),
            ("accept", "accept merged"),
        ] {
            std::fs::write(dir.join(format!("handoff/{name}.md")), text).unwrap();
        }

        let resolved =
            resolve_dynamic_completion_handoff_files(&fanout_submission(), &dir).unwrap();
        // The resolved value is frozen: later file edits cannot reach it.
        std::fs::write(dir.join("handoff/a.md"), "edited later").unwrap();
        let completion: DynamicNodeCompletion = serde_json::from_value(resolved.clone()).unwrap();

        assert_eq!(completion.summary, "split work");
        let DynamicNext::Fanout {
            nodes,
            merge,
            acceptance,
            ..
        } = completion.next
        else {
            panic!("fanout expected");
        };
        assert_eq!(nodes[0].task, "task A");
        assert_eq!(nodes[1].task, "task B");
        assert_eq!(merge.task, "merge both");
        assert_eq!(acceptance.task, "accept merged");
        assert!(resolved.get("summaryPath").is_none());
        assert!(resolved["next"]["nodes"][0].get("taskPath").is_none());
    }

    #[test]
    fn handoff_file_errors_are_structured_per_field() {
        let (_temp, dir) = handoff_attachments_dir();
        std::fs::write(dir.join("handoff/summary.md"), " \n").unwrap();
        std::fs::write(dir.join("handoff/a.md"), [0xff, 0xfe]).unwrap();
        std::fs::write(
            dir.join("handoff/merge.md"),
            vec![b'x'; DYNAMIC_HANDOFF_FILE_MAX_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::write(dir.parent().unwrap().join("outside.md"), "outside").unwrap();
        let mut submission = fanout_submission();
        submission["next"]["nodes"][1]["taskPath"] = serde_json::json!("handoff/missing.md");
        submission["next"]["acceptance"]["taskPath"] = serde_json::json!("../outside.md");

        let errors = resolve_dynamic_completion_handoff_files(&submission, &dir).unwrap_err();
        let codes = errors
            .iter()
            .map(|error| (error.path.as_deref().unwrap(), error.code.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            codes,
            vec![
                ("summaryPath", DYNAMIC_HANDOFF_EMPTY),
                ("next.merge.taskPath", DYNAMIC_HANDOFF_TOO_LARGE),
                ("next.acceptance.taskPath", DYNAMIC_HANDOFF_INVALID_PATH),
                ("next.nodes[0].taskPath", DYNAMIC_HANDOFF_NOT_UTF8),
                ("next.nodes[1].taskPath", DYNAMIC_HANDOFF_NOT_FOUND),
            ]
        );
        assert_eq!(errors[0].params["file"], "handoff/summary.md");
    }

    #[test]
    fn handoff_file_paths_cannot_be_absolute() {
        let (_temp, dir) = handoff_attachments_dir();
        let mut submission = fanout_submission();
        submission["summaryPath"] = serde_json::json!(dir.join("handoff/summary.md").as_str());

        let errors = resolve_dynamic_completion_handoff_files(&submission, &dir).unwrap_err();

        assert_eq!(errors[0].code, DYNAMIC_HANDOFF_INVALID_PATH);
    }

    #[cfg(unix)]
    #[test]
    fn handoff_file_symlinks_cannot_escape_attachments() {
        let (_temp, dir) = handoff_attachments_dir();
        let outside = dir.parent().unwrap().join("outside.md");
        std::fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("handoff/summary.md")).unwrap();
        let mut submission = fanout_submission();
        submission["next"] = serde_json::json!({ "type": "end" });

        let errors = resolve_dynamic_completion_handoff_files(&submission, &dir).unwrap_err();

        assert_eq!(errors[0].code, DYNAMIC_HANDOFF_OUTSIDE_ATTACHMENTS);
    }

    #[test]
    fn effective_schema_accepts_handoff_paths_and_rejects_inline_text() {
        let schema = dynamic_completion_effective_schema(&DynamicCompletionSchemaPolicy {
            max_fanout: 5,
            ..Default::default()
        });
        let compiled = jsonschema::JSONSchema::compile(&schema).unwrap();
        assert!(compiled.is_valid(&fanout_submission()));

        let mut inline_summary = fanout_submission();
        inline_summary["summary"] = serde_json::json!("inline");
        assert!(!compiled.is_valid(&inline_summary));

        let mut inline_task = fanout_submission();
        inline_task["next"]["merge"]["task"] = serde_json::json!("inline");
        assert!(!compiled.is_valid(&inline_task));

        let mut missing_path = fanout_submission();
        missing_path["next"]["nodes"][0]
            .as_object_mut()
            .unwrap()
            .remove("taskPath");
        assert!(!compiled.is_valid(&missing_path));
    }

    #[test]
    fn dynamic_control_tasks_reject_provider_and_model() {
        let schema = dynamic_agent_task_spec_schema(&DynamicCompletionSchemaPolicy {
            provider_required: true,
            agent_task_model_visible: false,
            provider_ids: vec!["claude-acp".to_string(), "codex-acp".to_string()],
            ..Default::default()
        });
        let compiled = jsonschema::JSONSchema::compile(&schema).unwrap();
        let task = serde_json::json!({
            "title": "Merge",
            "taskPath": "handoff/merge.md"
        });
        assert!(compiled.is_valid(&task));

        let mut with_provider = task.clone();
        with_provider["provider"] = serde_json::json!("codex-acp");
        assert!(!compiled.is_valid(&with_provider));

        let mut with_model = task;
        with_model["model"] = serde_json::json!("gpt-5.6-sol");
        assert!(!compiled.is_valid(&with_model));
    }

    /// A DynamicNodeState without the `uuid` field (legacy on-disk data)
    /// must still deserialize thanks to `#[serde(default)]`.
    #[test]
    fn dynamic_node_state_uuid_is_optional_for_legacy_data() {
        let json = r#"{
            "version": "1",
            "id": "bootstrap",
            "dynamicRunId": "dr-1",
            "kind": "worker",
            "title": "t",
            "task": "t",
            "status": "ready",
            "chainId": "bootstrap",
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "sessionMode": "new"
        }"#;
        let node: DynamicNodeState =
            serde_json::from_str(json).expect("legacy node must deserialize");
        assert!(node.uuid.is_none());
        assert!(node.pause_reason.is_none());
        assert!(node.runtime_error.is_none());
    }

    /// When present, the uuid field round-trips through serde.
    #[test]
    fn dynamic_node_state_uuid_round_trips() {
        let json = r#"{
            "version": "1",
            "id": "bootstrap",
            "dynamicRunId": "dr-1",
            "kind": "worker",
            "title": "t",
            "task": "t",
            "status": "ready",
            "chainId": "bootstrap",
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "sessionMode": "new",
            "uuid": "abc123"
        }"#;
        let node: DynamicNodeState = serde_json::from_str(json).expect("node must deserialize");
        assert_eq!(node.uuid.as_deref(), Some("abc123"));
    }

    #[test]
    fn dynamic_node_pause_reason_and_runtime_error_round_trip() {
        let json = r#"{
            "version": "1",
            "id": "bootstrap",
            "dynamicRunId": "dr-1",
            "kind": "worker",
            "title": "t",
            "task": "t",
            "status": "paused",
            "pauseReason": "runtime-abnormal",
            "runtimeError": {
                "code": { "domain": "provider", "code": "provider.acp-error" },
                "domain": "provider",
                "recovery": "manual",
                "retryPolicy": null,
                "params": { "method": "session/set_config_option" },
                "diagnostic": "failed to persist config",
                "raw": null
            },
            "chainId": "bootstrap",
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "sessionMode": "new"
        }"#;
        let node: DynamicNodeState = serde_json::from_str(json).unwrap();
        assert_eq!(node.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert_eq!(
            node.runtime_error.as_ref().map(|error| error.code_str()),
            Some("provider.acp-error")
        );
        let restored: DynamicNodeState =
            serde_json::from_value(serde_json::to_value(&node).unwrap()).unwrap();
        assert_eq!(restored.pause_reason, node.pause_reason);
        assert_eq!(restored.runtime_error, node.runtime_error);
    }
}
