use serde::{Deserialize, Serialize};

use crate::config::ConversationAutoConfig;
use crate::dsl::WorkflowDsl;
use crate::workflow_model_binding::WorkflowModelBindings;

use super::error::ExecutionPlanError;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionPlanRunMode {
    Workflow,
    Auto,
}

impl ExecutionPlanRunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionPlanTarget {
    Current,
    Next,
    CurrentAndNext,
}

impl ExecutionPlanTarget {
    pub fn includes_current(self) -> bool {
        matches!(self, Self::Current | Self::CurrentAndNext)
    }

    pub fn includes_next(self) -> bool {
        matches!(self, Self::Next | Self::CurrentAndNext)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanManifest {
    pub schema_version: u32,
    pub plan_revision: u64,
    pub run_mode: ExecutionPlanRunMode,
    pub source_authoring_revision: u64,
    pub current_plan_file: String,
    pub published_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExecutionPlanPayload {
    Workflow {
        workflow: WorkflowDsl,
        #[serde(default)]
        model_bindings: WorkflowModelBindings,
    },
    Auto {
        config: ConversationAutoConfig,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanSnapshot {
    pub schema_version: u32,
    pub plan_revision: u64,
    pub run_mode: ExecutionPlanRunMode,
    pub source_authoring_revision: u64,
    pub published_at: String,
    pub payload: ExecutionPlanPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringRevisionFile {
    pub schema_version: u32,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAuthoringDraft {
    pub workflow: WorkflowDsl,
    #[serde(default)]
    pub model_bindings: WorkflowModelBindings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanLocator {
    pub project_id: String,
    pub task_id: String,
    pub task_uuid: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanSaveCommand {
    pub project_id: String,
    pub task_id: String,
    pub task_uuid: String,
    pub run_id: String,
    pub operation_id: Option<String>,
    pub target: ExecutionPlanTarget,
    pub expected_plan_revision: u64,
    pub expected_authoring_revision: u64,
    pub expected_run_status: String,
    pub expected_current_round: Option<String>,
    pub expected_current_node: Option<String>,
    pub expected_current_attempt: Option<String>,
    pub workflow: Option<WorkflowAuthoringDraft>,
    pub auto_config: Option<ConversationAutoConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanPreflight {
    pub plan_revision: u64,
    pub authoring_revision: u64,
    pub execution_revision: u64,
    pub run_status: String,
    pub run_outcome: Option<String>,
    pub current_round: Option<String>,
    pub current_node: Option<String>,
    pub current_attempt: Option<String>,
    pub current_editable: bool,
    pub diverged: bool,
    pub blocking: Vec<ExecutionPlanError>,
    pub affected_node_ids: Vec<String>,
    pub resume_identity_risks: Vec<ExecutionPlanError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanView {
    pub project_id: String,
    pub task_id: String,
    pub task_uuid: String,
    pub run_id: String,
    pub run_mode: String,
    pub run_status: String,
    pub run_outcome: Option<String>,
    pub plan_revision: u64,
    pub authoring_revision: u64,
    pub execution_revision: u64,
    pub current_editable: bool,
    pub diverged: bool,
    pub current_round: Option<String>,
    pub current_node: Option<String>,
    pub current_attempt: Option<String>,
    pub current_workflow: Option<WorkflowDsl>,
    pub current_model_bindings: WorkflowModelBindings,
    pub current_auto_config: Option<ConversationAutoConfig>,
    pub next_workflow: Option<WorkflowDsl>,
    pub next_model_bindings: WorkflowModelBindings,
    pub next_auto_config: Option<ConversationAutoConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanTargetResult {
    pub target: ExecutionPlanTarget,
    pub committed: bool,
    pub plan_revision: Option<u64>,
    pub authoring_revision: Option<u64>,
    pub error: Option<ExecutionPlanError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanSaveResult {
    pub operation_id: Option<String>,
    pub complete: bool,
    pub plan_revision: u64,
    pub authoring_revision: u64,
    pub execution_revision: u64,
    /// Whether Current and Next still differ after this commit, using the same comparison as the plan view.
    pub diverged: bool,
    pub targets: Vec<ExecutionPlanTargetResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationCommit {
    pub schema_version: u32,
    pub operation_id: String,
    pub phase: String,
    pub plan_revision: u64,
    pub authoring_revision: u64,
    pub current_committed: bool,
    pub next_committed: bool,
}
