use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOT_FOUND: &str = "conversation.execution-plan.not-found";
pub const REVISION_CONFLICT: &str = "conversation.execution-plan.revision-conflict";
pub const AUTHORING_CONFLICT: &str = "conversation.execution-plan.authoring-conflict";
pub const CURRENT_LOCATOR_CONFLICT: &str = "conversation.execution-plan.current-locator-conflict";
pub const CURRENT_RUN_NOT_EDITABLE: &str = "conversation.execution-plan.current-run-not-editable";
pub const CURRENT_NODE_IDENTITY_CHANGED: &str =
    "conversation.execution-plan.current-node-identity-changed";
pub const CURRENT_NODE_REMOVED: &str = "conversation.execution-plan.current-node-removed";
pub const AGENT_IDENTITY_CHANGED: &str = "conversation.execution-plan.agent-identity-changed";
pub const CONTINUE_UNSUPPORTED: &str = "conversation.execution-plan.continue-unsupported";
pub const RESUME_FAILED: &str = "conversation.execution-plan.resume-failed";
pub const VALIDATION_FAILED: &str = "conversation.execution-plan.validation-failed";
pub const PARTIAL_COMMIT: &str = "conversation.execution-plan.partial-commit";
pub const RECOVERY_REQUIRED: &str = "conversation.execution-plan.recovery-required";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlanError {
    pub code: String,
    pub context: Value,
}

impl ExecutionPlanError {
    pub fn new(code: impl Into<String>, context: Value) -> Self {
        Self {
            code: code.into(),
            context,
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn context(&self) -> &Value {
        &self.context
    }
}

impl std::fmt::Display for ExecutionPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}

impl std::error::Error for ExecutionPlanError {}

pub type PlanResult<T> = Result<T, ExecutionPlanError>;
