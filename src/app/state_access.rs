use anyhow::{anyhow, Result};

use crate::dsl::WorkflowDsl;
use crate::runtime::{NodeState, RoundState, RunState};
use crate::storage::{read_json, write_json};

use super::App;

pub(crate) fn current_attempt_state(app: &App, task_id: &str, run: &RunState) -> Result<(RoundState, NodeState)> {
    let round_id = run.current_round.as_ref().ok_or_else(|| anyhow!("run has no current round"))?;
    let node_id = run.current_node.as_ref().ok_or_else(|| anyhow!("run has no current node"))?;
    let attempt_id = run.current_attempt.as_ref().ok_or_else(|| anyhow!("run has no current attempt"))?;
    let round: RoundState = read_json(&app.paths.round_file(task_id, &run.id, round_id))?;
    let node: NodeState = read_json(&app.paths.node_file(task_id, &run.id, round_id, node_id, attempt_id))?;
    Ok((round, node))
}

pub(crate) fn load_run_workflow(app: &App, task_id: &str, run_id: &str) -> Result<WorkflowDsl> {
    read_json(&app.paths.workflow_snapshot_file(task_id, run_id))
}

pub(crate) fn persist_runtime_state(app: &App, task_id: &str, run: &RunState, round: &RoundState, node: &NodeState) -> Result<()> {
    write_json(&app.paths.run_file(task_id, &run.id), run)?;
    write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;
    write_json(&app.paths.node_file(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id), node)?;
    Ok(())
}
