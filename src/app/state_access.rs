use anyhow::{Result, anyhow};

use crate::dsl::{WorkflowDsl, normalize_legacy_workflow_snapshot};
use crate::runtime::{NodeState, RoundState, RunState, RuntimeAttemptLocator};
use crate::storage::{read_json, write_json};

use super::App;
use super::attempt_runtime_state_lock;

pub(crate) fn refresh_runtime_execution_if_current(
    app: &App,
    task_id: &str,
    run: &mut RunState,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    expected_execution_id: Option<&str>,
) -> Result<bool> {
    let state_lock =
        attempt_runtime_state_lock(app, task_id, &run.id, round_id, node_id, attempt_id);
    let _guard = state_lock
        .lock()
        .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
    let durable_run: RunState = read_json(&app.paths.run_file(task_id, &run.id))?;
    let durable_node: NodeState = read_json(
        &app.paths
            .node_file(task_id, &run.id, round_id, node_id, attempt_id),
    )?;
    if durable_run.status != crate::domain::RunStatus::Running
        || durable_run.current_round.as_deref() != Some(round_id)
        || durable_run.current_node.as_deref() != Some(node_id)
        || durable_run.current_attempt.as_deref() != Some(attempt_id)
        || durable_node.runtime_execution_id.as_deref() != expected_execution_id
    {
        return Ok(false);
    }
    if durable_run.execution.revision > run.execution.revision {
        run.execution = durable_run.execution;
        run.updated_at = durable_run.updated_at;
    }
    Ok(true)
}

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
    // Commit the node outcome before publishing any Runtime transition that
    // may point at a successor. This is the aggregate's crash-consistency
    // boundary across the three atomic JSON files.
    write_json(
        &app.paths
            .node_file(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id),
        node,
    )?;
    write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;
    write_json(&app.paths.run_file(task_id, &run.id), run)?;
    Ok(())
}

fn execution_locator_belongs_to_outer_attempt(
    locator: Option<&RuntimeAttemptLocator>,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> bool {
    let Some(locator) = locator else {
        return false;
    };
    if locator.round_id != round_id {
        return false;
    }
    match (&locator.outer_node_id, &locator.outer_attempt_id) {
        (Some(outer_node_id), Some(outer_attempt_id)) => {
            outer_node_id == node_id && outer_attempt_id == attempt_id
        }
        (None, None) => locator.node_id == node_id && locator.attempt_id == attempt_id,
        _ => false,
    }
}

/// Persists a Runtime-controlled attempt only while its durable execution
/// identity is still current. The same short lock is used by stop and failure
/// convergence, making the identity comparison and state write one operation.
pub(crate) fn persist_runtime_state_if_execution_current(
    app: &App,
    task_id: &str,
    run: &mut RunState,
    round: &RoundState,
    node: &NodeState,
) -> Result<bool> {
    let expected_execution_id = node.runtime_execution_id.clone();
    persist_runtime_state_if_expected_execution_current(
        app,
        task_id,
        run,
        round,
        node,
        expected_execution_id.as_deref(),
    )
}

pub(crate) fn persist_runtime_state_if_expected_execution_current(
    app: &App,
    task_id: &str,
    run: &mut RunState,
    round: &RoundState,
    node: &NodeState,
    expected_execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = expected_execution_id else {
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
    let durable_run: RunState = read_json(&app.paths.run_file(task_id, &run.id))?;
    if durable_run.status != crate::domain::RunStatus::Running
        || durable_run.current_round != run.current_round
        || durable_run.current_node != run.current_node
        || durable_run.current_attempt != run.current_attempt
        || !execution_locator_belongs_to_outer_attempt(
            durable_run.execution.locator.as_ref(),
            &round.id,
            &node.node_id,
            &node.attempt_id,
        )
        || !execution_locator_belongs_to_outer_attempt(
            run.execution.locator.as_ref(),
            &round.id,
            &node.node_id,
            &node.attempt_id,
        )
    {
        return Ok(false);
    }
    // Provider callbacks advance the authoritative durable execution phase.
    // Preserve that monotonic phase/revision when the orchestrator commits the
    // node result instead of overwriting it with its older in-memory snapshot.
    if durable_run.execution.revision > run.execution.revision {
        run.execution = durable_run.execution;
        run.updated_at = durable_run.updated_at;
    }
    persist_runtime_state(app, task_id, run, round, node)?;
    Ok(true)
}
