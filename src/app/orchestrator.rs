use anyhow::{bail, Result};
use camino::Utf8Path;
use camino::Utf8PathBuf;

use crate::control::{decide_next_step, ControlDecision};
use crate::domain::{NodeOutcome, PauseReason, RoundTrigger, RunOutcome, RunStatus, SessionMode, VERSION};
use crate::dsl::{validate_workflow, ValidatedWorkflow, WorkflowDsl};
use crate::runtime::{validate_round_state, validate_run_state, NodeState, RoundState, RunState, WorkerRefState};
use crate::storage::{read_json, write_json};

use super::ids::{next_attempt_id, next_run_id, now_rfc3339_like};
use super::node_executor::{execute_ai_node, execute_exec_node, re_evaluate_attempt};
use super::state_access::{current_attempt_state, load_run_workflow, persist_runtime_state};
use super::state_factory::create_node_state;
use super::transition_context::{feedback_summary_from_previous_node, find_latest_artifact_path, find_latest_worker_ref_for_transition};
use super::App;

pub(crate) fn run_start(app: &App, task_id: &str, workflow_override: Option<&Utf8Path>) -> Result<RunState> {
    let workflow_path = workflow_override
        .map(|path| path.to_owned())
        .unwrap_or_else(|| app.paths.workflow_file(task_id));
    let workflow: WorkflowDsl = read_json(&workflow_path)?;
    let validated = validate_workflow(workflow.clone())?;

    let run_id = next_run_id(&app.paths.runs_dir(task_id))?;
    let round_id = "round-001".to_string();
    let attempt_id = "attempt-001".to_string();
    let now = now_rfc3339_like();

    let mut run = RunState {
        version: VERSION.to_string(),
        id: run_id.clone(),
        task_id: task_id.to_string(),
        status: RunStatus::Running,
        outcome: None,
        started_at: now.clone(),
        updated_at: now.clone(),
        workflow_snapshot: "workflow.snapshot.json".to_string(),
        current_round: Some(round_id.clone()),
        current_node: Some(validated.raw.entry.clone()),
        current_attempt: Some(attempt_id.clone()),
        acceptance_loops_used: 0,
        pause_reason: None,
    };
    validate_run_state(&run)?;
    write_json(&app.paths.run_file(task_id, &run_id), &run)?;
    write_json(&app.paths.workflow_snapshot_file(task_id, &run_id), &workflow)?;

    let mut round = RoundState {
        version: VERSION.to_string(),
        id: round_id.clone(),
        run_id: run_id.clone(),
        index: 1,
        status: RunStatus::Running,
        outcome: None,
        trigger: RoundTrigger::Initial,
        repair_loops_used: 0,
        started_at: now.clone(),
    };
    validate_round_state(&round)?;
    write_json(&app.paths.round_file(task_id, &run_id, &round_id), &round)?;

    let node = create_node_state(&run_id, &round_id, &validated.raw.entry, &attempt_id, validated.get_node(&validated.raw.entry).expect("validated entry exists"));
    drive_from_node(app, task_id, &validated, &mut run, &mut round, node)?;
    Ok(run)
}

pub(crate) fn run_continue(app: &App, task_id: &str, run_id: &str) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    let mut run = app.run_status(task_id, run_id)?;
    let current = current_attempt_state(app, task_id, &run)?;
    let (mut round, mut node) = current;

    match node.status {
        RunStatus::Paused => {
            if run.pause_reason == Some(PauseReason::ProcessInterrupted) {
                let continue_ref = read_json::<WorkerRefState>(&app.paths.worker_ref_file(task_id, run_id, &round.id, &node.node_id, &node.attempt_id))?.continue_ref;
                node = execute_ai_node(
                    app,
                    task_id,
                    &run.id,
                    &round.id,
                    &node.attempt_id,
                    &validated,
                    &node.node_id,
                    node.clone(),
                    SessionMode::Continue,
                    continue_ref,
                    None,
                    None,
                )?;
            } else {
                bail!("current attempt is paused but not resumable by continue");
            }
        }
        RunStatus::Completed if node.outcome == Some(NodeOutcome::Invalid) => {
            node = re_evaluate_attempt(app, task_id, &run.id, &round.id, node)?;
        }
        _ => bail!("current attempt is not continuable"),
    }

    drive_from_node(app, task_id, &validated, &mut run, &mut round, node)?;
    Ok(run)
}

pub(crate) fn run_retry(app: &App, task_id: &str, run_id: &str) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    let mut run = app.run_status(task_id, run_id)?;
    let (mut round, node) = current_attempt_state(app, task_id, &run)?;
    let node_id = node.node_id.clone();
    let attempt_id = next_attempt_id(&app.paths.node_dir(task_id, run_id, &round.id, &node_id))?;
    let fresh = create_node_state(run_id, &round.id, &node_id, &attempt_id, validated.get_node(&node_id).expect("validated node exists"));
    drive_from_node(app, task_id, &validated, &mut run, &mut round, fresh)?;
    Ok(run)
}

pub(crate) fn drive_from_node(
    app: &App,
    task_id: &str,
    workflow: &ValidatedWorkflow,
    run: &mut RunState,
    round: &mut RoundState,
    mut node: NodeState,
) -> Result<()> {
    let mut session_mode = SessionMode::New;
    let mut continue_ref: Option<serde_json::Value> = None;
    let mut feedback_summary: Option<String> = None;
    let mut verify_result_path: Option<Utf8PathBuf> = None;

    loop {
        let current_attempt_id = node.attempt_id.clone();
        let current_node_id = node.node_id.clone();
        node = match workflow.get_node(&current_node_id).expect("validated node exists") {
            crate::dsl::NodeDsl::Worker(_) | crate::dsl::NodeDsl::Verify(_) => execute_ai_node(
                app,
                task_id,
                &run.id,
                &round.id,
                &current_attempt_id,
                workflow,
                &current_node_id,
                node,
                session_mode,
                continue_ref.as_ref().cloned(),
                feedback_summary.clone(),
                verify_result_path.as_deref(),
            )?,
            crate::dsl::NodeDsl::Exec(_) => execute_exec_node(app, task_id, &run.id, &round.id, workflow, node)?,
        };

        persist_runtime_state(app, task_id, run, round, &node)?;
        let decision = decide_next_step(workflow, run, round, &node);

        match decision {
            ControlDecision::TransitionToNode { node_id, session } => {
                if matches!(node.node_type, crate::domain::NodeType::Exec)
                    && matches!(node.outcome, Some(NodeOutcome::Failure | NodeOutcome::Invalid))
                {
                    round.repair_loops_used += 1;
                }

                let next_node_dsl = workflow.get_node(&node_id).expect("validated transition target exists");
                let next_attempt_id = next_attempt_id(&app.paths.node_dir(task_id, &run.id, &round.id, &node_id))?;
                session_mode = session;
                continue_ref = find_latest_worker_ref_for_transition(app, task_id, &run.id, &round.id, &node, &node_id, session)?
                    .map(|path| read_json::<WorkerRefState>(&path))
                    .transpose()?
                    .and_then(|worker_ref| worker_ref.continue_ref);
                feedback_summary = feedback_summary_from_previous_node(app, task_id, &run.id, &round.id, &node)?;
                verify_result_path = None;
                node = create_node_state(&run.id, &round.id, &node_id, &next_attempt_id, next_node_dsl);
                run.current_node = Some(node_id);
                run.current_attempt = Some(next_attempt_id);
                run.status = RunStatus::Running;
                run.pause_reason = None;
                run.updated_at = now_rfc3339_like();
                validate_round_state(round)?;
                validate_run_state(run)?;
                continue;
            }
            ControlDecision::OpenNewRound => {
                round.status = RunStatus::Completed;
                round.outcome = Some(RunOutcome::Failure);
                validate_round_state(round)?;
                write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;

                run.acceptance_loops_used += 1;
                let next_round_index = round.index + 1;
                let next_round_id = format!("round-{next_round_index:03}");
                *round = RoundState {
                    version: VERSION.to_string(),
                    id: next_round_id.clone(),
                    run_id: run.id.clone(),
                    index: next_round_index,
                    status: RunStatus::Running,
                    outcome: None,
                    trigger: RoundTrigger::AcceptanceLoop,
                    repair_loops_used: 0,
                    started_at: now_rfc3339_like(),
                };
                validate_round_state(round)?;
                write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;

                let next_node_dsl = workflow.get_node(&workflow.raw.entry).expect("validated entry exists");
                let next_attempt_id = "attempt-001".to_string();
                verify_result_path = find_latest_artifact_path(app, task_id, &run.id, &format!("round-{:03}", next_round_index - 1), &node.node_id, "verify-result")?;
                feedback_summary = feedback_summary_from_previous_node(app, task_id, &run.id, &format!("round-{:03}", next_round_index - 1), &node)?;
                continue_ref = None;
                session_mode = SessionMode::New;
                node = create_node_state(&run.id, &round.id, &workflow.raw.entry, &next_attempt_id, next_node_dsl);
                run.current_round = Some(round.id.clone());
                run.current_node = Some(node.node_id.clone());
                run.current_attempt = Some(next_attempt_id);
                run.status = RunStatus::Running;
                run.pause_reason = None;
                run.updated_at = now_rfc3339_like();
                validate_run_state(run)?;
                continue;
            }
            ControlDecision::PauseRun(reason) => {
                run.status = RunStatus::Paused;
                run.pause_reason = Some(reason);
                run.updated_at = now_rfc3339_like();
                round.status = RunStatus::Paused;
                validate_round_state(round)?;
                validate_run_state(run)?;
                persist_runtime_state(app, task_id, run, round, &node)?;
                return Ok(());
            }
            ControlDecision::CompleteRun(outcome) => {
                run.status = RunStatus::Completed;
                run.outcome = Some(outcome);
                run.pause_reason = None;
                run.updated_at = now_rfc3339_like();
                round.status = RunStatus::Completed;
                round.outcome = Some(outcome);
                validate_round_state(round)?;
                validate_run_state(run)?;
                persist_runtime_state(app, task_id, run, round, &node)?;
                return Ok(());
            }
        }
    }
}
