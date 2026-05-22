use crate::domain::{
    NodeOutcome, NodeType, PauseReason, ResolvedConfig, RoundTrigger, RunOutcome, RunStatus,
    SessionMode, VERSION,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub version: String,
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub version: String,
    pub id: String,
    pub task_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub version: String,
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
    pub resolved_config: ResolvedConfig,
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
        }
    }
}

pub fn validate_task_state(state: &TaskState) -> Result<()> {
    ensure!(state.version == VERSION, "unsupported task state version");
    ensure!(!state.id.trim().is_empty(), "task id cannot be empty");
    Ok(())
}

pub fn validate_run_state(state: &RunState) -> Result<()> {
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
