use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: &str = "0.1";
pub const DEFAULT_PROVIDER: &str = "claude-code";

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeOutcome {
    Success,
    Failure,
    Invalid,
    Killed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    New,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PauseReason {
    ProcessInterrupted,
    ErrorBlocked,
    WaitingForUserInput,
    PermissionRequested,
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
