use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: &str = "0.1";
pub const DEFAULT_PROVIDER: &str = "claude-acp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunOutcome {
    Success,
    Failure,
    Killed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeType {
    Worker,
    AiDynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeOutcome {
    Success,
    Failure,
    Invalid,
    Killed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    New,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TurnControlMode {
    #[default]
    RuntimeControlled,
    NonRuntimeControlled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnControlTransitionCause {
    RuntimeInterrupted,
    ManualFollowUp,
    WorkflowContinued,
    RuntimeTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PauseReason {
    ProcessInterrupted,
    RuntimeAbnormal,
    ErrorBlocked,
    WaitingForUserInput,
    PermissionRequested,
}

impl PauseReason {
    pub fn allows_explicit_runtime_continue(self) -> bool {
        matches!(self, Self::ProcessInterrupted | Self::RuntimeAbnormal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoundTrigger {
    Initial,
    NewRound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvocationKind {
    WorkerGeneric,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRef {
    pub provider: String,
    pub mode: SessionMode,
    pub supports_open_session: bool,
    pub supports_continue_session: bool,
    pub continue_ref: Option<serde_json::Value>,
    pub open_command: Option<String>,
}

pub type ResolvedConfig = BTreeMap<String, serde_json::Value>;

#[cfg(test)]
mod tests {
    use super::PauseReason;

    #[test]
    fn explicit_runtime_continue_is_limited_to_recoverable_pause_reasons() {
        assert!(PauseReason::ProcessInterrupted.allows_explicit_runtime_continue());
        assert!(PauseReason::RuntimeAbnormal.allows_explicit_runtime_continue());
        assert!(!PauseReason::WaitingForUserInput.allows_explicit_runtime_continue());
        assert!(!PauseReason::PermissionRequested.allows_explicit_runtime_continue());
        assert!(!PauseReason::ErrorBlocked.allows_explicit_runtime_continue());
    }
}
