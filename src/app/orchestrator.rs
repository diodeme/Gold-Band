use std::thread;

use anyhow::{Result, bail, ensure};
use camino::Utf8Path;

use crate::acp::permission::cancel_pending_permission_requests;
use crate::config::DesktopLanguage;
use crate::control::{ControlDecision, decide_next_step};
use crate::domain::{
    NodeOutcome, PauseReason, RoundTrigger, RunOutcome, RunStatus, SessionMode, VERSION,
};
use crate::dsl::{ValidatedWorkflow, WorkflowDsl, validate_workflow};
use crate::observability::{
    ExecutionContext, ProgressStage, append_run_event_best_effort, progress, run_event_data,
    write_progress_hint, write_run_progress_best_effort,
};
use crate::runtime::{
    NodeState, RoundState, RoundTraceStep, RunState, WorkerRefState, validate_round_state,
    validate_run_state,
};
use crate::storage::{read_json, write_json};

use super::ids::{next_attempt_id, next_run_id, now_rfc3339_like};
use super::node_executor::{execute_ai_node, re_evaluate_attempt};
use super::profile_resolver::{resolve_profile_for_node, resolve_workflow_profiles};
use super::state_access::{current_attempt_state, load_run_workflow, persist_runtime_state};
use super::state_factory::create_node_state;
use super::transition_context::find_latest_worker_ref_for_transition;
use super::{App, is_run_continuable};

struct PreparedRun {
    validated: ValidatedWorkflow,
    resolved_profiles: super::profile_resolver::ResolvedWorkflowMetadata,
    run: RunState,
    round: RoundState,
    node: NodeState,
}

struct NextExecution {
    node: NodeState,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
}

fn localized_continue_prompt(language: DesktopLanguage) -> String {
    match language {
        DesktopLanguage::ZhCn => "继续".to_string(),
        DesktopLanguage::En => "Continue".to_string(),
    }
}

pub(crate) fn run_start(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
) -> Result<RunState> {
    let PreparedRun {
        validated,
        resolved_profiles,
        mut run,
        mut round,
        node,
    } = prepare_run(app, task_id, workflow_override)?;
    drive_from_node(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        node,
    )?;
    Ok(run)
}

pub(crate) fn run_start_background(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
) -> Result<RunState> {
    let prepared = prepare_run(app, task_id, workflow_override)?;
    let initial_run = prepared.run.clone();
    let repo_root = app.paths.repo_root.clone();
    let config = app.config.clone();
    let task_id = task_id.to_string();

    thread::spawn(move || {
        let app = App::with_config(repo_root, config);
        let PreparedRun {
            validated,
            resolved_profiles,
            mut run,
            mut round,
            node,
        } = prepared;
        if let Err(err) = drive_from_node(
            &app,
            &task_id,
            &validated,
            &resolved_profiles,
            &mut run,
            &mut round,
            node,
        ) {
            let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
            let _ = std::fs::write(
                app.paths
                    .runs_dir(&task_id)
                    .join("desktop-start-error.txt")
                    .as_std_path(),
                err.to_string(),
            );
        }
    });

    Ok(initial_run)
}

fn prepare_run(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
) -> Result<PreparedRun> {
    let workflow_path = workflow_override
        .map(|path| path.to_owned())
        .unwrap_or_else(|| app.paths.workflow_file(task_id));
    let workflow: WorkflowDsl = read_json(&workflow_path)?;
    let validated = validate_workflow(workflow.clone())?;
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw)?;
    write_json(
        &app.paths.task_workflow_resolved_file(task_id),
        &validated.raw,
    )?;
    write_json(&app.paths.task_provenance_file(task_id), &resolved_profiles)?;

    let run_id = next_run_id(&app.paths.runs_dir(task_id))?;
    let round_id = "round-001".to_string();
    let attempt_id = "attempt-001".to_string();
    let now = now_rfc3339_like();

    let run = RunState {
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
        new_rounds_opened: 0,
        pause_reason: None,
    };
    validate_run_state(&run)?;
    write_json(&app.paths.run_file(task_id, &run_id), &run)?;
    write_json(
        &app.paths.workflow_snapshot_file(task_id, &run_id),
        &workflow,
    )?;

    let round = RoundState {
        version: VERSION.to_string(),
        id: round_id.clone(),
        run_id: run_id.clone(),
        index: 1,
        status: RunStatus::Running,
        outcome: None,
        trigger: RoundTrigger::Initial,
        started_at: now.clone(),
        trace: vec![round_trace_step(
            1,
            &validated.raw.entry,
            &attempt_id,
            None,
            None,
            now.clone(),
        )],
    };
    validate_round_state(&round)?;
    write_json(&app.paths.round_file(task_id, &run_id, &round_id), &round)?;

    let entry_node = validated
        .get_node(&validated.raw.entry)
        .expect("validated entry exists");
    let entry_profile = match entry_node {
        crate::dsl::NodeDsl::Worker(worker) => worker
            .profile
            .as_deref()
            .and_then(|name| resolve_profile_for_node(&resolved_profiles, name)),
    };
    let node = create_node_state(
        &run_id,
        &round_id,
        &validated.raw.entry,
        &attempt_id,
        entry_node,
        entry_profile,
    );
    let ctx = ExecutionContext::for_run(task_id, &run.id)
        .with_round(round.id.clone())
        .with_node(node.node_id.clone())
        .with_attempt(node.attempt_id.clone());
    let summary = format!(
        "starting run {} at {}/{}/{}",
        run.id, round.id, node.node_id, node.attempt_id
    );
    progress(&summary);
    write_run_progress_best_effort(
        &app.paths,
        task_id,
        &run,
        Some(node.node_type),
        ProgressStage::Starting,
        summary.clone(),
    );
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "run_started",
        now,
        run_event_data(
            &ctx,
            Some(ProgressStage::Starting),
            Some(run.status),
            Some(summary),
            None,
        ),
    );
    write_progress_hint(
        &app.paths,
        task_id,
        &run.id,
        Some(
            app.paths
                .raw_stream_file(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)
                .as_path(),
        ),
    );

    Ok(PreparedRun {
        validated,
        resolved_profiles,
        run,
        round,
        node,
    })
}

pub(crate) fn run_continue(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw)?;
    let mut run = app.run_status(task_id, run_id)?;
    let current = current_attempt_state(app, task_id, &run)?;
    let (mut round, mut node) = current;
    let ctx = ExecutionContext::for_run(task_id, &run.id)
        .with_round(round.id.clone())
        .with_node(node.node_id.clone())
        .with_attempt(node.attempt_id.clone());
    let summary = format!(
        "continuing run {} at {}/{}/{}",
        run.id, round.id, node.node_id, node.attempt_id
    );
    progress(&summary);
    write_run_progress_best_effort(
        &app.paths,
        task_id,
        &run,
        Some(node.node_type),
        ProgressStage::Starting,
        summary.clone(),
    );
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "run_continue_requested",
        run.updated_at.clone(),
        run_event_data(
            &ctx,
            Some(ProgressStage::Starting),
            Some(run.status),
            Some(summary),
            run.pause_reason,
        ),
    );

    let (
        initial_session_mode,
        initial_continue_ref,
        initial_resume_prompt,
        initial_resume_prompt_id,
    ) = match node.status {
        RunStatus::Paused => {
            if !is_run_continuable(&run) {
                bail!("current attempt is paused but not resumable by continue");
            }
            if node.manual_check_pending {
                bail!("current attempt is waiting for manual check");
            }
            let continue_ref = read_json::<WorkerRefState>(&app.paths.worker_ref_file(
                task_id,
                run_id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            ))?
            .continue_ref
            .ok_or_else(|| anyhow::anyhow!("current attempt has no ACP continue reference"))?;
            (
                SessionMode::Continue,
                Some(continue_ref),
                Some(localized_continue_prompt(app.config.desktop_language)),
                prompt_id,
            )
        }
        RunStatus::Completed if node.outcome == Some(NodeOutcome::Invalid) => {
            node = re_evaluate_attempt(app, task_id, &run.id, &round.id, node)?;
            (SessionMode::New, None, None, None)
        }
        _ => bail!("current attempt is not continuable"),
    };

    drive_from_node_with_initial_session(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        node,
        initial_session_mode,
        initial_continue_ref,
        initial_resume_prompt,
        initial_resume_prompt_id,
    )?;
    Ok(run)
}

pub(crate) fn run_continue_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
) -> Result<RunState> {
    let initial_run = app.run_status(task_id, run_id)?;
    if !is_run_continuable(&initial_run) {
        bail!("current run is not resumable by continue");
    }
    let (_, node) = current_attempt_state(app, task_id, &initial_run)?;
    if node.manual_check_pending {
        bail!("current attempt is waiting for manual check");
    }
    let repo_root = app.paths.repo_root.clone();
    let config = app.config.clone();
    let task_id = task_id.to_string();
    let run_id = run_id.to_string();
    let prompt_id = prompt_id.clone();

    thread::spawn(move || {
        let app = App::with_config(repo_root, config);
        if let Err(err) = run_continue(&app, &task_id, &run_id, prompt_id) {
            let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
            let _ = std::fs::write(
                app.paths
                    .runs_dir(&task_id)
                    .join("desktop-continue-error.txt")
                    .as_std_path(),
                err.to_string(),
            );
        }
    });

    Ok(initial_run)
}

pub(crate) fn submit_manual_check(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outcome: NodeOutcome,
) -> Result<RunState> {
    ensure!(
        matches!(outcome, NodeOutcome::Success | NodeOutcome::Failure),
        "manual check outcome must be success or failure"
    );
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw)?;
    let mut run = app.run_status(task_id, run_id)?;
    ensure!(run.status == RunStatus::Paused, "run is not paused");
    ensure!(
        run.current_round.as_deref() == Some(round_id)
            && run.current_node.as_deref() == Some(node_id)
            && run.current_attempt.as_deref() == Some(attempt_id),
        "manual check can only be submitted for the current paused attempt"
    );
    let (mut round, mut node) = current_attempt_state(app, task_id, &run)?;
    ensure!(round.id == round_id, "round mismatch for manual check");
    ensure!(node.node_id == node_id, "node mismatch for manual check");
    ensure!(
        node.attempt_id == attempt_id,
        "attempt mismatch for manual check"
    );
    ensure!(node.status == RunStatus::Paused, "node is not paused");
    ensure!(
        node.manual_check_pending,
        "node is not waiting for manual check"
    );

    node.status = RunStatus::Completed;
    node.outcome = Some(outcome);
    node.manual_check_pending = false;
    node.finished_at = Some(now_rfc3339_like());

    let ctx = ExecutionContext::for_run(task_id, &run.id)
        .with_round(round.id.clone())
        .with_node(node.node_id.clone())
        .with_attempt(node.attempt_id.clone());
    let decision_summary = format!(
        "manual check decided {} for {}/{}/{}",
        edge_outcome_label(outcome),
        round.id,
        node.node_id,
        node.attempt_id
    );
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "manual_check_submitted",
        now_rfc3339_like(),
        run_event_data(
            &ctx,
            Some(ProgressStage::NormalizingArtifact),
            Some(node.status),
            Some(decision_summary),
            None,
        ),
    );
    let completion_summary = format!(
        "completed {}/{}/{} via manual check",
        round.id, node.node_id, node.attempt_id
    );
    write_run_progress_best_effort(
        &app.paths,
        task_id,
        &run,
        Some(node.node_type),
        ProgressStage::NormalizingArtifact,
        completion_summary.clone(),
    );
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "node_completed",
        now_rfc3339_like(),
        run_event_data(
            &ctx,
            Some(ProgressStage::NormalizingArtifact),
            Some(node.status),
            Some(completion_summary),
            None,
        ),
    );
    persist_runtime_state(app, task_id, &run, &round, &node)?;
    let decision = decide_next_step(&validated, &run, &round, &node);
    if let Some(next) = apply_control_decision(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        &node,
        decision,
    )? {
        drive_from_node_with_initial_session(
            app,
            task_id,
            &validated,
            &resolved_profiles,
            &mut run,
            &mut round,
            next.node,
            next.session_mode,
            next.continue_ref,
            None,
            None,
        )?;
    }
    Ok(run)
}

pub(crate) fn submit_manual_check_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outcome: NodeOutcome,
) -> Result<RunState> {
    let initial_run = app.run_status(task_id, run_id)?;
    let repo_root = app.paths.repo_root.clone();
    let config = app.config.clone();
    let task_id = task_id.to_string();
    let run_id = run_id.to_string();
    let round_id = round_id.to_string();
    let node_id = node_id.to_string();
    let attempt_id = attempt_id.to_string();

    thread::spawn(move || {
        let app = App::with_config(repo_root, config);
        if let Err(err) = submit_manual_check(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            outcome,
        ) {
            let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
            let _ = std::fs::write(
                app.paths
                    .runs_dir(&task_id)
                    .join("desktop-manual-check-error.txt")
                    .as_std_path(),
                err.to_string(),
            );
        }
    });

    Ok(initial_run)
}

pub(crate) fn run_retry(app: &App, task_id: &str, run_id: &str) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw)?;
    let mut run = app.run_status(task_id, run_id)?;
    let (mut round, node) = current_attempt_state(app, task_id, &run)?;
    let node_id = node.node_id.clone();
    let attempt_id = next_attempt_id(&app.paths.node_dir(task_id, run_id, &round.id, &node_id))?;
    let fresh_node = validated.get_node(&node_id).expect("validated node exists");
    let fresh_profile = match fresh_node {
        crate::dsl::NodeDsl::Worker(worker) => worker
            .profile
            .as_deref()
            .and_then(|name| resolve_profile_for_node(&resolved_profiles, name)),
    };
    let fresh = create_node_state(
        run_id,
        &round.id,
        &node_id,
        &attempt_id,
        fresh_node,
        fresh_profile,
    );
    round.trace.push(round_trace_step(
        next_trace_sequence(&round),
        &node_id,
        &attempt_id,
        Some(node_id.clone()),
        Some("retry".to_string()),
        now_rfc3339_like(),
    ));
    let ctx = ExecutionContext::for_run(task_id, &run.id)
        .with_round(round.id.clone())
        .with_node(node_id.clone())
        .with_attempt(attempt_id.clone());
    let summary = format!("retrying node {} with {}", node_id, attempt_id);
    progress(&summary);
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "run_retry_requested",
        run.updated_at.clone(),
        run_event_data(
            &ctx,
            Some(ProgressStage::Starting),
            Some(run.status),
            Some(summary),
            None,
        ),
    );
    drive_from_node(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        fresh,
    )?;
    Ok(run)
}

fn round_trace_step(
    sequence: u32,
    node_id: &str,
    attempt_id: &str,
    from_node_id: Option<String>,
    edge_outcome: Option<String>,
    entered_at: String,
) -> RoundTraceStep {
    RoundTraceStep {
        sequence,
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        from_node_id,
        edge_outcome,
        entered_at,
    }
}

fn next_trace_sequence(round: &RoundState) -> u32 {
    round
        .trace
        .last()
        .map(|step| step.sequence + 1)
        .unwrap_or(1)
}

fn fail_workflow_control_limit(
    app: &App,
    task_id: &str,
    run: &mut RunState,
    round: &mut RoundState,
    node: &NodeState,
    summary: String,
) -> Result<Option<NextExecution>> {
    let now = now_rfc3339_like();
    run.status = RunStatus::Completed;
    run.outcome = Some(RunOutcome::Failure);
    run.pause_reason = None;
    run.updated_at = now.clone();
    round.status = RunStatus::Completed;
    round.outcome = Some(RunOutcome::Failure);
    progress(&summary);
    write_run_progress_best_effort(
        &app.paths,
        task_id,
        run,
        Some(node.node_type),
        ProgressStage::Completed,
        summary.clone(),
    );
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "workflow_control_limit_exceeded",
        now,
        run_event_data(
            &ExecutionContext::for_run(task_id, &run.id)
                .with_round(round.id.clone())
                .with_node(node.node_id.clone())
                .with_attempt(node.attempt_id.clone()),
            Some(ProgressStage::Completed),
            Some(run.status),
            Some(summary),
            None,
        ),
    );
    validate_round_state(round)?;
    validate_run_state(run)?;
    persist_runtime_state(app, task_id, run, round, node)?;
    Ok(None)
}

fn edge_outcome_label(outcome: NodeOutcome) -> String {
    match outcome {
        NodeOutcome::Success => "success".to_string(),
        NodeOutcome::Failure => "failure".to_string(),
        NodeOutcome::Invalid => "invalid".to_string(),
        NodeOutcome::Killed => "killed".to_string(),
    }
}

fn run_is_killed(app: &App, task_id: &str, run_id: &str) -> Result<bool> {
    let run: RunState = read_json(&app.paths.run_file(task_id, run_id))?;
    Ok(run.status == RunStatus::Completed && run.outcome == Some(RunOutcome::Killed))
}

fn setup_node_environment(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node: &NodeState,
    ctx: &ExecutionContext,
) -> Result<()> {
    std::fs::create_dir_all(
        app.paths
            .attempt_dir(task_id, run_id, round_id, &node.node_id, &node.attempt_id)
            .as_std_path(),
    )?;
    std::fs::create_dir_all(
        app.paths
            .artifacts_dir(task_id, run_id, round_id, &node.node_id, &node.attempt_id)
            .as_std_path(),
    )?;
    std::fs::create_dir_all(
        app.paths
            .attachments_dir(task_id, run_id, round_id, &node.node_id, &node.attempt_id)
            .as_std_path(),
    )?;
    append_run_event_best_effort(
        &app.paths,
        task_id,
        run_id,
        "node_environment_setup",
        now_rfc3339_like(),
        run_event_data(
            ctx,
            Some(ProgressStage::Starting),
            Some(node.status),
            Some("node environment prepared".to_string()),
            None,
        ),
    );
    Ok(())
}

fn teardown_node_environment_best_effort(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node: &NodeState,
    ctx: &ExecutionContext,
) {
    let attempt_dir =
        app.paths
            .attempt_dir(task_id, run_id, round_id, &node.node_id, &node.attempt_id);
    let _ = cancel_pending_permission_requests(&attempt_dir, now_rfc3339_like());
    let pid_path =
        app.paths
            .provider_pid_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id);
    if pid_path.exists() {
        let _ = std::fs::remove_file(pid_path.as_std_path());
    }
    append_run_event_best_effort(
        &app.paths,
        task_id,
        run_id,
        "node_environment_teardown",
        now_rfc3339_like(),
        run_event_data(
            ctx,
            Some(ProgressStage::NormalizingArtifact),
            Some(node.status),
            Some("node environment released".to_string()),
            None,
        ),
    );
}

fn should_pause_for_manual_check(workflow: &ValidatedWorkflow, node: &NodeState) -> bool {
    let Some(node_dsl) = workflow.get_node(&node.node_id) else {
        return false;
    };
    node_dsl.manual_check_enabled()
        && matches!(node.node_type, crate::domain::NodeType::Worker)
        && node.status == RunStatus::Completed
        && matches!(
            node.outcome,
            Some(NodeOutcome::Success | NodeOutcome::Failure | NodeOutcome::Invalid)
        )
}

fn apply_control_decision(
    app: &App,
    task_id: &str,
    workflow: &ValidatedWorkflow,
    resolved_profiles: &super::profile_resolver::ResolvedWorkflowMetadata,
    run: &mut RunState,
    round: &mut RoundState,
    node: &NodeState,
    decision: ControlDecision,
) -> Result<Option<NextExecution>> {
    match decision {
        ControlDecision::TransitionToNode { node_id, session } => {
            let next_node_dsl = workflow
                .get_node(&node_id)
                .expect("validated transition target exists");
            let previous_node_id = node.node_id.clone();
            if let Some(max_attempts) = workflow.raw.control.max_attempts {
                let proposed_attempts = round
                    .trace
                    .iter()
                    .filter(|step| {
                        step.from_node_id.as_deref() == Some(previous_node_id.as_str())
                            && step.node_id == node_id
                    })
                    .count() as u32
                    + 1;
                if proposed_attempts > max_attempts {
                    return fail_workflow_control_limit(
                        app,
                        task_id,
                        run,
                        round,
                        node,
                        format!(
                            "max attempts exceeded for {} -> {}: {} > {}",
                            previous_node_id, node_id, proposed_attempts, max_attempts
                        ),
                    );
                }
            }
            let next_attempt_id =
                next_attempt_id(&app.paths.node_dir(task_id, &run.id, &round.id, &node_id))?;
            let edge_outcome = node.outcome.map(edge_outcome_label);
            let continue_ref = find_latest_worker_ref_for_transition(
                app, task_id, &run.id, &round.id, node, &node_id, session,
            )?
            .map(|path| read_json::<WorkerRefState>(&path))
            .transpose()?
            .and_then(|worker_ref| worker_ref.continue_ref);
            let next_profile = match next_node_dsl {
                crate::dsl::NodeDsl::Worker(worker) => worker
                    .profile
                    .as_deref()
                    .and_then(|name| resolve_profile_for_node(resolved_profiles, name)),
            };
            let next_node = create_node_state(
                &run.id,
                &round.id,
                &node_id,
                &next_attempt_id,
                next_node_dsl,
                next_profile,
            );
            run.current_node = Some(node_id.clone());
            run.current_attempt = Some(next_attempt_id.clone());
            round.trace.push(round_trace_step(
                next_trace_sequence(round),
                &node_id,
                &next_attempt_id,
                Some(previous_node_id),
                edge_outcome,
                now_rfc3339_like(),
            ));
            run.status = RunStatus::Running;
            run.pause_reason = None;
            run.updated_at = now_rfc3339_like();
            let transition_summary = format!(
                "transitioned to {}/{}/{}",
                round.id, node_id, next_attempt_id
            );
            progress(&transition_summary);
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                run,
                Some(next_node.node_type),
                ProgressStage::Starting,
                transition_summary.clone(),
            );
            append_run_event_best_effort(
                &app.paths,
                task_id,
                &run.id,
                "transitioned",
                run.updated_at.clone(),
                run_event_data(
                    &ExecutionContext::for_run(task_id, &run.id)
                        .with_round(round.id.clone())
                        .with_node(node_id)
                        .with_attempt(next_attempt_id),
                    Some(ProgressStage::Starting),
                    Some(run.status),
                    Some(transition_summary),
                    None,
                ),
            );
            validate_round_state(round)?;
            validate_run_state(run)?;
            Ok(Some(NextExecution {
                node: next_node,
                session_mode: session,
                continue_ref,
            }))
        }
        ControlDecision::OpenNewRound => {
            if let Some(max_rounds) = workflow.raw.control.max_rounds {
                let proposed_rounds = run.new_rounds_opened + 1;
                if proposed_rounds > max_rounds {
                    return fail_workflow_control_limit(
                        app,
                        task_id,
                        run,
                        round,
                        node,
                        format!(
                            "max rounds exceeded for $new-round: {} > {}",
                            proposed_rounds, max_rounds
                        ),
                    );
                }
            }
            round.status = RunStatus::Completed;
            round.outcome = Some(RunOutcome::Failure);
            validate_round_state(round)?;
            write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;

            run.new_rounds_opened += 1;
            let next_round_index = round.index + 1;
            let next_round_id = format!("round-{next_round_index:03}");
            *round = RoundState {
                version: VERSION.to_string(),
                id: next_round_id.clone(),
                run_id: run.id.clone(),
                index: next_round_index,
                status: RunStatus::Running,
                outcome: None,
                trigger: RoundTrigger::NewRound,
                started_at: now_rfc3339_like(),
                trace: Vec::new(),
            };
            validate_round_state(round)?;
            write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;

            let next_node_dsl = workflow
                .get_node(&workflow.raw.entry)
                .expect("validated entry exists");
            let next_attempt_id = "attempt-001".to_string();
            let next_profile = match next_node_dsl {
                crate::dsl::NodeDsl::Worker(worker) => worker
                    .profile
                    .as_deref()
                    .and_then(|name| resolve_profile_for_node(resolved_profiles, name)),
            };
            let next_node = create_node_state(
                &run.id,
                &round.id,
                &workflow.raw.entry,
                &next_attempt_id,
                next_node_dsl,
                next_profile,
            );
            round.trace.push(round_trace_step(
                1,
                &next_node.node_id,
                &next_attempt_id,
                None,
                None,
                now_rfc3339_like(),
            ));
            run.current_round = Some(round.id.clone());
            run.current_node = Some(next_node.node_id.clone());
            run.current_attempt = Some(next_attempt_id.clone());
            run.status = RunStatus::Running;
            run.pause_reason = None;
            run.updated_at = now_rfc3339_like();
            let round_summary = format!(
                "opened {} and restarted at {}/{}",
                round.id, next_node.node_id, next_attempt_id
            );
            progress(&round_summary);
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                run,
                Some(next_node.node_type),
                ProgressStage::Starting,
                round_summary.clone(),
            );
            append_run_event_best_effort(
                &app.paths,
                task_id,
                &run.id,
                "round_opened",
                run.updated_at.clone(),
                run_event_data(
                    &ExecutionContext::for_run(task_id, &run.id)
                        .with_round(round.id.clone())
                        .with_node(next_node.node_id.clone())
                        .with_attempt(next_attempt_id),
                    Some(ProgressStage::Starting),
                    Some(run.status),
                    Some(round_summary),
                    None,
                ),
            );
            validate_run_state(run)?;
            Ok(Some(NextExecution {
                node: next_node,
                session_mode: SessionMode::New,
                continue_ref: None,
            }))
        }
        ControlDecision::PauseRun(reason) => {
            run.status = RunStatus::Paused;
            run.pause_reason = Some(reason);
            run.updated_at = now_rfc3339_like();
            round.status = RunStatus::Paused;
            let pause_stage = if reason == PauseReason::ErrorBlocked {
                ProgressStage::Blocked
            } else {
                ProgressStage::Paused
            };
            let pause_summary = format!(
                "run {} paused at {}/{}/{}",
                run.id, round.id, node.node_id, node.attempt_id
            );
            progress(&pause_summary);
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                run,
                Some(node.node_type),
                pause_stage,
                pause_summary.clone(),
            );
            append_run_event_best_effort(
                &app.paths,
                task_id,
                &run.id,
                "run_paused",
                run.updated_at.clone(),
                run_event_data(
                    &ExecutionContext::for_run(task_id, &run.id)
                        .with_round(round.id.clone())
                        .with_node(node.node_id.clone())
                        .with_attempt(node.attempt_id.clone()),
                    Some(pause_stage),
                    Some(run.status),
                    Some(pause_summary),
                    Some(reason),
                ),
            );
            persist_runtime_state(app, task_id, run, round, node)?;
            Ok(None)
        }
        ControlDecision::CompleteRun(outcome) => {
            run.status = RunStatus::Completed;
            run.outcome = Some(outcome);
            run.pause_reason = None;
            run.updated_at = now_rfc3339_like();
            round.status = RunStatus::Completed;
            round.outcome = Some(outcome);
            let complete_summary = format!("run {} completed with {:?}", run.id, outcome);
            progress(&complete_summary);
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                run,
                Some(node.node_type),
                ProgressStage::Completed,
                complete_summary.clone(),
            );
            append_run_event_best_effort(
                &app.paths,
                task_id,
                &run.id,
                "run_completed",
                run.updated_at.clone(),
                run_event_data(
                    &ExecutionContext::for_run(task_id, &run.id)
                        .with_round(round.id.clone())
                        .with_node(node.node_id.clone())
                        .with_attempt(node.attempt_id.clone()),
                    Some(ProgressStage::Completed),
                    Some(run.status),
                    Some(complete_summary),
                    None,
                ),
            );
            validate_round_state(round)?;
            validate_run_state(run)?;
            persist_runtime_state(app, task_id, run, round, node)?;
            Ok(None)
        }
    }
}

pub(crate) fn drive_from_node(
    app: &App,
    task_id: &str,
    workflow: &ValidatedWorkflow,
    resolved_profiles: &super::profile_resolver::ResolvedWorkflowMetadata,
    run: &mut RunState,
    round: &mut RoundState,
    node: NodeState,
) -> Result<()> {
    drive_from_node_with_initial_session(
        app,
        task_id,
        workflow,
        resolved_profiles,
        run,
        round,
        node,
        SessionMode::New,
        None,
        None,
        None,
    )
}

fn drive_from_node_with_initial_session(
    app: &App,
    task_id: &str,
    workflow: &ValidatedWorkflow,
    resolved_profiles: &super::profile_resolver::ResolvedWorkflowMetadata,
    run: &mut RunState,
    round: &mut RoundState,
    mut node: NodeState,
    initial_session_mode: SessionMode,
    initial_continue_ref: Option<serde_json::Value>,
    initial_resume_prompt: Option<String>,
    initial_resume_prompt_id: Option<String>,
) -> Result<()> {
    let mut session_mode = initial_session_mode;
    let mut continue_ref = initial_continue_ref;
    let mut resume_prompt = initial_resume_prompt;
    let mut resume_prompt_id = initial_resume_prompt_id;

    loop {
        if run_is_killed(app, task_id, &run.id)? {
            return Ok(());
        }
        let current_attempt_id = node.attempt_id.clone();
        let current_node_id = node.node_id.clone();
        let ctx = ExecutionContext::for_run(task_id, &run.id)
            .with_round(round.id.clone())
            .with_node(current_node_id.clone())
            .with_attempt(current_attempt_id.clone());
        run.status = RunStatus::Running;
        run.pause_reason = None;
        run.updated_at = now_rfc3339_like();
        round.status = RunStatus::Running;
        if node.status == RunStatus::Paused {
            node.status = RunStatus::Running;
            node.finished_at = None;
        }
        let node_stage = ProgressStage::CallingProvider;
        let summary = format!(
            "running {}/{}/{}",
            round.id, current_node_id, current_attempt_id
        );
        progress(&summary);
        write_run_progress_best_effort(
            &app.paths,
            task_id,
            run,
            Some(node.node_type),
            node_stage,
            summary.clone(),
        );
        append_run_event_best_effort(
            &app.paths,
            task_id,
            &run.id,
            "node_started",
            now_rfc3339_like(),
            run_event_data(
                &ctx,
                Some(node_stage),
                Some(run.status),
                Some(summary),
                run.pause_reason,
            ),
        );
        persist_runtime_state(app, task_id, run, round, &node)?;
        setup_node_environment(app, task_id, &run.id, &round.id, &node, &ctx)?;
        node = match execute_ai_node(
            app,
            task_id,
            &run.id,
            round,
            &current_attempt_id,
            workflow,
            &current_node_id,
            node.clone(),
            session_mode,
            continue_ref.as_ref().cloned(),
            resume_prompt.take(),
            resume_prompt_id.take(),
        ) {
            Ok(node) => node,
            Err(err) => {
                if run_is_killed(app, task_id, &run.id)? {
                    return Ok(());
                }
                let error_summary = format!(
                    "run {} blocked at {}/{}/{}: {}",
                    run.id, round.id, current_node_id, current_attempt_id, err
                );
                progress(&error_summary);
                run.status = RunStatus::Paused;
                run.pause_reason = Some(PauseReason::ErrorBlocked);
                run.updated_at = now_rfc3339_like();
                round.status = RunStatus::Paused;
                let mut failed_node = node;
                failed_node.status = RunStatus::Paused;
                failed_node.outcome = None;
                failed_node.finished_at = Some(run.updated_at.clone());
                write_run_progress_best_effort(
                    &app.paths,
                    task_id,
                    run,
                    Some(failed_node.node_type),
                    ProgressStage::Blocked,
                    error_summary.clone(),
                );
                append_run_event_best_effort(
                    &app.paths,
                    task_id,
                    &run.id,
                    "run_paused",
                    run.updated_at.clone(),
                    run_event_data(
                        &ctx,
                        Some(ProgressStage::Blocked),
                        Some(run.status),
                        Some(error_summary),
                        run.pause_reason,
                    ),
                );
                teardown_node_environment_best_effort(
                    app,
                    task_id,
                    &run.id,
                    &round.id,
                    &failed_node,
                    &ctx,
                );
                persist_runtime_state(app, task_id, run, round, &failed_node)?;
                return Ok(());
            }
        };

        if node.status == RunStatus::Completed {
            teardown_node_environment_best_effort(app, task_id, &run.id, &round.id, &node, &ctx);
        }

        if should_pause_for_manual_check(workflow, &node) {
            node.status = RunStatus::Paused;
            node.outcome = None;
            node.manual_check_pending = true;
            node.finished_at = Some(now_rfc3339_like());
            run.status = RunStatus::Paused;
            run.pause_reason = Some(PauseReason::WaitingForUserInput);
            run.updated_at = now_rfc3339_like();
            round.status = RunStatus::Paused;
            let summary = format!(
                "manual check required at {}/{}/{}",
                round.id, node.node_id, node.attempt_id
            );
            progress(&summary);
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                run,
                Some(node.node_type),
                ProgressStage::Paused,
                summary.clone(),
            );
            append_run_event_best_effort(
                &app.paths,
                task_id,
                &run.id,
                "manual_check_pending",
                run.updated_at.clone(),
                run_event_data(
                    &ExecutionContext::for_run(task_id, &run.id)
                        .with_round(round.id.clone())
                        .with_node(node.node_id.clone())
                        .with_attempt(node.attempt_id.clone()),
                    Some(ProgressStage::Paused),
                    Some(run.status),
                    Some(summary),
                    run.pause_reason,
                ),
            );
            persist_runtime_state(app, task_id, run, round, &node)?;
            return Ok(());
        }

        let completion_summary = format!(
            "completed {}/{}/{}",
            round.id, node.node_id, node.attempt_id
        );
        write_run_progress_best_effort(
            &app.paths,
            task_id,
            run,
            Some(node.node_type),
            ProgressStage::NormalizingArtifact,
            completion_summary.clone(),
        );
        append_run_event_best_effort(
            &app.paths,
            task_id,
            &run.id,
            "node_completed",
            now_rfc3339_like(),
            run_event_data(
                &ExecutionContext::for_run(task_id, &run.id)
                    .with_round(round.id.clone())
                    .with_node(node.node_id.clone())
                    .with_attempt(node.attempt_id.clone()),
                Some(ProgressStage::NormalizingArtifact),
                Some(node.status),
                Some(completion_summary),
                None,
            ),
        );
        persist_runtime_state(app, task_id, run, round, &node)?;
        let decision = decide_next_step(workflow, run, round, &node);

        if let Some(next) = apply_control_decision(
            app,
            task_id,
            workflow,
            resolved_profiles,
            run,
            round,
            &node,
            decision,
        )? {
            node = next.node;
            session_mode = next.session_mode;
            continue_ref = next.continue_ref;
            resume_prompt = None;
            resume_prompt_id = None;
            continue;
        }
        return Ok(());
    }
}
