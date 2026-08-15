use anyhow::{Result, anyhow};

use crate::dsl::{WorkflowDsl, normalize_legacy_workflow_snapshot};
use crate::runtime::{NodeState, RoundState, RunState};
use crate::storage::{read_json, write_json};

use super::App;
use super::attempt_runtime_state_lock;

pub(crate) fn current_attempt_state(
    app: &App,
    task_id: &str,
    run: &RunState,
) -> Result<(RoundState, NodeState)> {
    let round_id = run
        .current_round
        .as_ref()
        .ok_or_else(|| anyhow!("run has no current round"))?;
    let node_id = run
        .current_node
        .as_ref()
        .ok_or_else(|| anyhow!("run has no current node"))?;
    let attempt_id = run
        .current_attempt
        .as_ref()
        .ok_or_else(|| anyhow!("run has no current attempt"))?;
    let round: RoundState = read_json(&app.paths.round_file(task_id, &run.id, round_id))?;
    let node: NodeState = read_json(
        &app.paths
            .node_file(task_id, &run.id, round_id, node_id, attempt_id),
    )?;
    Ok((round, node))
}

pub(crate) fn load_run_workflow(app: &App, task_id: &str, run_id: &str) -> Result<WorkflowDsl> {
    let snapshot_path = app.paths.workflow_snapshot_file(task_id, run_id);
    Ok(normalize_legacy_workflow_snapshot(read_json(
        &snapshot_path,
    )?))
}

pub(crate) fn persist_runtime_state(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) -> Result<()> {
    write_json(&app.paths.run_file(task_id, &run.id), run)?;
    write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;
    write_json(
        &app.paths
            .node_file(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id),
        node,
    )?;
    Ok(())
}

/// Persists a Runtime-controlled attempt only while its durable execution
/// identity is still current. The same short lock is used by stop and failure
/// convergence, making the identity comparison and state write one operation.
pub(crate) fn persist_runtime_state_if_execution_current(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) -> Result<bool> {
    let Some(execution_id) = node.runtime_execution_id.as_deref() else {
        persist_runtime_state(app, task_id, run, round, node)?;
        return Ok(true);
    };
    let state_lock = attempt_runtime_state_lock(
        app,
        task_id,
        &run.id,
        &round.id,
        &node.node_id,
        &node.attempt_id,
    );
    let _guard = state_lock
        .lock()
        .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
    let node_path =
        app.paths
            .node_file(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
    let current: NodeState = read_json(&node_path)?;
    if current.runtime_execution_id.as_deref() != Some(execution_id) {
        return Ok(false);
    }
    persist_runtime_state(app, task_id, run, round, node)?;
    Ok(true)
}
