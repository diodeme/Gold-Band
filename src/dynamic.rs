use crate::domain::{NodeOutcome, PauseReason, RunOutcome, SessionMode, VERSION};
use crate::dsl::{DynamicControlDsl, WorkflowDsl};
use anyhow::{Result, ensure};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

pub const DYNAMIC_COMPLETION_ARTIFACT: &str = "dynamic-node-completion";

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
pub enum WorkspaceMode {
    Readonly,
    Worktree,
    Main,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePolicy {
    pub mode: WorkspaceMode,
}

impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            mode: WorkspaceMode::Readonly,
        }
    }
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
    pub id: String,
    pub dynamic_run_id: String,
    pub kind: DynamicNodeKind,
    pub title: String,
    pub task: String,
    pub status: DynamicNodeStatus,
    pub outcome: Option<NodeOutcome>,
    pub group_id: Option<String>,
    pub chain_id: String,
    pub depth: u32,
    pub depends_on: Vec<String>,
    pub workspace: WorkspacePolicy,
    pub workspace_path: Option<Utf8PathBuf>,
    pub provider: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub session_mode: SessionMode,
    pub continue_from_node_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_snapshot_id: Option<String>,
    pub child_run_id: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
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
    pub proposals: Vec<DynamicProposalState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub workspace: WorkspacePolicy,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeSpecKind {
    Worker,
    WorkflowInvocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicAgentTaskSpec {
    pub title: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub profile: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicNodeCompletionKind {
    DynamicNodeCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicCompletionStatus {
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub fn dynamic_completion_schema() -> serde_json::Value {
    serde_json::json!({
        "version": "String",
        "kind": "String",
        "status": "String",
        "summary": "String",
        "next": {
            "type": "String"
        }
    })
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
    Ok(())
}
