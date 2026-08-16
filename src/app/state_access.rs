use anyhow::{Result, anyhow};

use crate::dsl::{WorkflowDsl, normalize_legacy_workflow_snapshot};
use crate::observability::{ExecutionContext, append_run_event_best_effort, run_event_data};
use crate::runtime::{NodeState, RoundState, RunState};
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
    let mut workflow = normalize_legacy_workflow_snapshot(read_json(&snapshot_path)?);
    let normalizations = app.normalize_workflow_models(&mut workflow);
    if !normalizations.is_empty() {
        write_json(&snapshot_path, &workflow)?;
        let ctx = ExecutionContext::for_run(task_id, run_id);
        for normalization in normalizations {
            let mut event_data = run_event_data(&ctx, None, None, None, None);
            event_data.details =
                Some(serde_json::to_value(normalization).unwrap_or_else(|_| serde_json::json!({})));
            append_run_event_best_effort(
                &app.paths,
                task_id,
                run_id,
                "model_config_normalized",
                super::ids::now_rfc3339_like(),
                event_data,
            );
        }
    }
    Ok(workflow)
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
    let durable_run: RunState = read_json(&app.paths.run_file(task_id, &run.id))?;
    if durable_run.status != crate::domain::RunStatus::Running
        || durable_run.current_round != run.current_round
        || durable_run.current_node != run.current_node
        || durable_run.current_attempt != run.current_attempt
        || durable_run.execution.locator != run.execution.locator
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
