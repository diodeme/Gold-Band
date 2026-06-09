use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;

use anyhow::{Result, anyhow, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};

use crate::acp::permission::cancel_pending_permission_requests;
use crate::artifacts::parse_json_artifact;
use crate::config::DesktopLanguage;
use crate::control::{ControlDecision, decide_next_step};
use crate::domain::{
    InvocationKind, NodeOutcome, PauseReason, RoundTrigger, RunOutcome, RunStatus, SessionMode,
    VERSION,
};
use crate::dsl::{
    AiDynamicAgentStrategy, AiDynamicNode, NodeDsl, ValidatedWorkflow, WorkflowDsl,
    validate_workflow, workflow_contains_ai_dynamic,
};
use crate::dynamic::{
    AllowedWorkflowSnapshot, DYNAMIC_COMPLETION_ARTIFACT, DynamicAgentTaskSpec,
    DynamicCompletionStatus, DynamicGraphState, DynamicGroupState, DynamicGroupStatus, DynamicNext,
    DynamicNodeCompletion, DynamicNodeCompletionKind, DynamicNodeKind, DynamicNodeSpec,
    DynamicNodeSpecKind, DynamicNodeState, DynamicNodeStatus, DynamicProposalState,
    DynamicProposalValidationError, DynamicProposalValidationStatus, DynamicRunState,
    DynamicRunStatus, WorkspaceMode,
    WorkspacePolicy, dynamic_completion_schema, validate_dynamic_group_state,
    validate_dynamic_node_state, validate_dynamic_run_state,
};
use crate::observability::{
    ExecutionContext, ProgressStage, append_run_event_best_effort, progress, run_event_data,
    write_progress_hint, write_run_progress_best_effort,
};
use crate::prompts::{AI_DYNAMIC_ACCEPTANCE_EN, AI_DYNAMIC_ACCEPTANCE_ZH_CN, AI_DYNAMIC_FANOUT_EN, AI_DYNAMIC_FANOUT_ZH_CN, AI_DYNAMIC_MERGE_EN, AI_DYNAMIC_MERGE_ZH_CN, AI_DYNAMIC_NODE_TASK_EN, AI_DYNAMIC_NODE_TASK_ZH_CN, AI_DYNAMIC_OUTPUT_PROTOCOL_EN, AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN, AI_DYNAMIC_PROPOSAL_REPAIR_EN, AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN, AI_DYNAMIC_SYSTEM_EN, AI_DYNAMIC_SYSTEM_ZH_CN, AI_DYNAMIC_WORKFLOW_INVOCATION_EN, AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN, RUNTIME_INVALID_OUTPUT_REPAIR_EN, RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN, prompt_by_language, render as render_template};
use crate::provider::{
    PromptBundle, PromptOutputContract, PromptRuntimeContext, PromptVisibility,
    ProviderRunResult, ProviderRunStatus, StreamMode, WorkerInvocation, render_prompt_bundle,
};
use crate::runtime::{
    NodeState, RoundState, RoundTraceStep, RunState, TaskState, WorkerRefState,
    validate_round_state, validate_run_state, validate_worker_ref_state,
};
use crate::storage::{append_jsonl, read_json, write_json};

use super::ids::{generate_uuid, next_attempt_id, next_run_id, now_rfc3339_like};
use super::node_executor::{execute_ai_node, re_evaluate_attempt};
use super::profile_resolver::{resolve_profile_for_node, resolve_workflow_profiles};
use super::state_access::{current_attempt_state, load_run_workflow, persist_runtime_state};
use super::state_factory::create_node_state;
use super::transition_context::find_latest_worker_ref_for_transition;
use super::{AcpLiveEventContext, App, is_run_continuable};

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

const MAX_INVALID_OUTPUT_REPAIR_PROMPTS: u32 = 3;
const MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS: u32 = 3;

fn dynamic_validation_error(
    code: &str,
    message: impl Into<String>,
    params: serde_json::Value,
) -> DynamicProposalValidationError {
    DynamicProposalValidationError::new(code, message, params)
}

fn dynamic_validation_error_lines(errors: &[DynamicProposalValidationError]) -> String {
    errors
        .iter()
        .map(|error| format!("- [{}] {}", error.code, error.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn localized_continue_prompt(language: DesktopLanguage) -> String {
    match language {
        DesktopLanguage::ZhCn => "继续".to_string(),
        DesktopLanguage::En => "Continue".to_string(),
    }
}

fn output_schema_for_node<'a>(
    workflow: &'a ValidatedWorkflow,
    node_id: &str,
) -> Option<&'a serde_json::Value> {
    match workflow.get_node(node_id)? {
        crate::dsl::NodeDsl::Worker(worker) => worker.output.as_ref()?.schema.as_ref(),
        crate::dsl::NodeDsl::AiDynamic(_) => None,
    }
}

fn invalid_output_repair_prompt(schema: &serde_json::Value) -> String {
    let schema = serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string());
    render_template(
        prompt_by_language(
            DesktopLanguage::ZhCn,
            RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN,
            RUNTIME_INVALID_OUTPUT_REPAIR_EN,
        ),
        serde_json::json!({
            "schema": schema,
        }),
    )
    .expect("prompt template renders")
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
    let background_app = app.clone_for_background();
    let task_id = task_id.to_string();

    thread::spawn(move || {
        let app = background_app;
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
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    write_json(
        &app.paths.task_workflow_resolved_file(task_id),
        &validated.raw,
    )?;
    write_json(&app.paths.task_provenance_file(task_id), &resolved_profiles)?;

    let run_id = next_run_id(&app.paths.runs_dir(task_id))?;
    let round_id = "round-001".to_string();
    let attempt_id = "attempt-001".to_string();
    let now = now_rfc3339_like();

    let task_uuid = read_json::<TaskState>(&app.paths.task_file(task_id))
        .ok()
        .and_then(|t| t.uuid);
    let run = RunState {
        version: VERSION.to_string(),
        id: run_id.clone(),
        task_id: task_id.to_string(),
        task_uuid,
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
        uuid: Some(generate_uuid()),
        last_executed_node: None,
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
        uuid: Some(generate_uuid()),
    };
    validate_round_state(&round)?;
    write_json(&app.paths.round_file(task_id, &run_id, &round_id), &round)?;

    let entry_node = validated
        .get_node(&validated.raw.entry)
        .expect("validated entry exists");
    let entry_profile = entry_node
        .profile()
        .and_then(|name| resolve_profile_for_node(&resolved_profiles, name));
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
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
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
            match validated.get_node(&node.node_id) {
                Some(NodeDsl::AiDynamic(_)) => (
                    SessionMode::Continue,
                    None,
                    None,
                    None,
                ),
                _ => {
                    let provider_pid_path = app.paths.provider_pid_file(
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &node.attempt_id,
                    );
                    if provider_pid_path.exists() {
                        bail!("current attempt is still stopping; wait for provider shutdown before continuing");
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
            }
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
    let background_app = app.clone_for_background();
    let task_id = task_id.to_string();
    let run_id = run_id.to_string();
    let prompt_id = prompt_id.clone();

    thread::spawn(move || {
        let app = background_app;
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
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
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
    let background_app = app.clone_for_background();
    let task_id = task_id.to_string();
    let run_id = run_id.to_string();
    let round_id = round_id.to_string();
    let node_id = node_id.to_string();
    let attempt_id = attempt_id.to_string();

    thread::spawn(move || {
        let app = background_app;
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
    let resolved_profiles = resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    let mut run = app.run_status(task_id, run_id)?;
    let (mut round, node) = current_attempt_state(app, task_id, &run)?;
    let node_id = node.node_id.clone();
    let attempt_id = next_attempt_id(&app.paths.node_dir(task_id, run_id, &round.id, &node_id))?;
    let fresh_node = validated.get_node(&node_id).expect("validated node exists");
    let fresh_profile = fresh_node
        .profile()
        .and_then(|name| resolve_profile_for_node(&resolved_profiles, name));
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
    control_failure: serde_json::Value,
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
    let mut event_data = run_event_data(
        &ExecutionContext::for_run(task_id, &run.id)
            .with_round(round.id.clone())
            .with_node(node.node_id.clone())
            .with_attempt(node.attempt_id.clone()),
        Some(ProgressStage::Completed),
        Some(run.status),
        Some(summary),
        None,
    );
    event_data.control_failure = Some(control_failure);
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "workflow_control_limit_exceeded",
        now,
        event_data,
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

fn is_repair_outcome(outcome: &str) -> bool {
    outcome == "failure"
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
            let edge_outcome = node.outcome.map(edge_outcome_label);
            if let (Some(max_attempts), Some(outcome)) =
                (workflow.raw.control.max_attempts, edge_outcome.as_deref())
            {
                if is_repair_outcome(outcome) {
                    let proposed_attempts = round
                        .trace
                        .iter()
                        .filter(|step| {
                            step.from_node_id.as_deref() == Some(previous_node_id.as_str())
                                && step.node_id == node_id
                                && step.edge_outcome.as_deref().is_some_and(is_repair_outcome)
                        })
                        .count() as u32
                        + 1;
                    if proposed_attempts > max_attempts {
                        let summary = format!(
                            "max repair attempts exceeded for {} -> {}: {} > {}",
                            previous_node_id, node_id, proposed_attempts, max_attempts
                        );
                        return fail_workflow_control_limit(
                            app,
                            task_id,
                            run,
                            round,
                            node,
                            summary.clone(),
                            serde_json::json!({
                                "reasonKind": "max_repair_attempts_exceeded",
                                "fromNodeId": previous_node_id,
                                "toNodeId": node_id,
                                "target": node_id,
                                "edgeOutcome": outcome,
                                "proposedCount": proposed_attempts,
                                "limit": max_attempts,
                                "message": summary,
                            }),
                        );
                    }
                }
            }
            let next_attempt_id =
                next_attempt_id(&app.paths.node_dir(task_id, &run.id, &round.id, &node_id))?;
            let continue_ref = find_latest_worker_ref_for_transition(
                app, task_id, &run.id, &round.id, node, &node_id, session,
            )?
            .map(|path| read_json::<WorkerRefState>(&path))
            .transpose()?
            .and_then(|worker_ref| worker_ref.continue_ref);
            let next_profile = next_node_dsl
                .profile()
                .and_then(|name| resolve_profile_for_node(resolved_profiles, name));
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
                    let summary = format!(
                        "max rounds exceeded for $new-round: {} > {}",
                        proposed_rounds, max_rounds
                    );
                    return fail_workflow_control_limit(
                        app,
                        task_id,
                        run,
                        round,
                        node,
                        summary.clone(),
                        serde_json::json!({
                            "reasonKind": "max_rounds_exceeded",
                            "target": "$new-round",
                            "proposedCount": proposed_rounds,
                            "limit": max_rounds,
                            "message": summary,
                        }),
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
                uuid: Some(generate_uuid()),
            };
            validate_round_state(round)?;
            write_json(&app.paths.round_file(task_id, &run.id, &round.id), round)?;

            let next_node_dsl = workflow
                .get_node(&workflow.raw.entry)
                .expect("validated entry exists");
            let next_attempt_id = "attempt-001".to_string();
            let next_profile = next_node_dsl
                .profile()
                .and_then(|name| resolve_profile_for_node(resolved_profiles, name));
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

struct DynamicExecutionContext<'a> {
    app: &'a App,
    task_id: &'a str,
    run_id: &'a str,
    round_id: &'a str,
    outer_node_id: &'a str,
    outer_attempt_id: &'a str,
    dynamic: &'a AiDynamicNode,
}

#[derive(Debug)]
struct DynamicExecutionResult {
    node: DynamicNodeState,
    proposals: Vec<DynamicProposalState>,
}

#[derive(Debug)]
struct DynamicExecutionMessage {
    node_id: String,
    result: Result<DynamicExecutionResult>,
}

fn freeze_allowed_workflow_snapshots(
    app: &App,
    dynamic: &AiDynamicNode,
) -> Result<Vec<AllowedWorkflowSnapshot>> {
    if dynamic.allowed_workflows.is_empty() {
        return Ok(Vec::new());
    }
    let store = app.workflow_templates()?;
    let mut snapshots = Vec::new();
    for allowed in &dynamic.allowed_workflows {
        let workflow_id = allowed.workflow_id.trim();
        let template = store
            .templates
            .iter()
            .find(|template| template.workflow.id.trim() == workflow_id)
            .ok_or_else(|| anyhow!("allowed workflow `{workflow_id}` not found"))?;
        let validated = validate_workflow(template.workflow.clone())?;
        app.validate_workflow_agents(&validated)?;
        let contains_ai_dynamic = workflow_contains_ai_dynamic(&validated.raw);
        ensure!(
            dynamic.control.allow_nested_dynamic || !contains_ai_dynamic,
            "allowed workflow `{workflow_id}` contains AI-DYNAMIC but nested dynamic is disabled"
        );
        snapshots.push(AllowedWorkflowSnapshot {
            workflow_id: workflow_id.to_string(),
            snapshot_id: format!("wf-snapshot-{:03}", snapshots.len() + 1),
            name: template.name.clone(),
            contains_ai_dynamic,
            workflow: validated.raw,
        });
    }
    Ok(snapshots)
}

fn dynamic_acp_live_event_context(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
) -> AcpLiveEventContext {
    AcpLiveEventContext {
        task_id: ctx.task_id.to_string(),
        run_id: ctx.run_id.to_string(),
        round_id: ctx.round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: Some(ctx.outer_node_id.to_string()),
        outer_attempt_id: Some(ctx.outer_attempt_id.to_string()),
    }
}

fn dynamic_runtime_context(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
) -> PromptRuntimeContext {
    let run_dir = ctx.app.paths.run_dir(ctx.task_id, ctx.run_id);
    let round_dir = ctx
        .app
        .paths
        .round_dir(ctx.task_id, ctx.run_id, ctx.round_id);
    let node_dir = ctx.app.paths.dynamic_node_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        node_id,
    );
    let attempt_dir = ctx.app.paths.dynamic_node_attempt_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        node_id,
        attempt_id,
    );
    let attachments_dir = ctx.app.paths.dynamic_node_attachments_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        node_id,
        attempt_id,
    );
    PromptRuntimeContext {
        project_id: ctx.app.paths.project_id.clone(),
        task_id: ctx.task_id.to_string(),
        run_id: ctx.run_id.to_string(),
        round_id: ctx.round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        language: ctx.app.config.desktop_language,
        run_dir,
        round_dir,
        node_dir,
        attempt_dir,
        attachments_dir,
    }
}

fn dynamic_agent_strategy_mode(dynamic: &AiDynamicNode) -> &'static str {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => "fixed",
        AiDynamicAgentStrategy::Dynamic { .. } => "dynamic",
    }
}

fn dynamic_agent_routing_prompt(dynamic: &AiDynamicNode) -> Option<&str> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => None,
        AiDynamicAgentStrategy::Dynamic { routing_prompt, .. } => Some(routing_prompt.trim()),
    }
}

fn dynamic_output_contract(language: DesktopLanguage, dynamic: &AiDynamicNode) -> PromptOutputContract {
    let schema_text = render_template(
        prompt_by_language(
            language,
            AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN,
            AI_DYNAMIC_OUTPUT_PROTOCOL_EN,
        ),
        serde_json::json!({
            "agent_strategy_mode": dynamic_agent_strategy_mode(dynamic),
        }),
    )
    .expect("prompt template renders");
    PromptOutputContract {
        artifact: DYNAMIC_COMPLETION_ARTIFACT.to_string(),
        kind: "json".to_string(),
        schema: Some(dynamic_completion_schema()),
        schema_text: Some(schema_text.trim().to_string()),
        success_condition: None,
    }
}

fn dynamic_attempt_id(_node: &DynamicNodeState) -> String {
    "attempt-001".to_string()
}

fn dynamic_proposal_file_path(
    ctx: &DynamicExecutionContext<'_>,
    proposal_id: &str,
) -> Utf8PathBuf {
    ctx.app
        .paths
        .dynamic_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        )
        .join("proposals")
        .join(format!("{proposal_id}.json"))
}

fn execute_ai_dynamic_node(
    app: &App,
    task_id: &str,
    run_id: &str,
    round: &RoundState,
    attempt_id: &str,
    dynamic: &AiDynamicNode,
    mut outer_node: NodeState,
) -> Result<NodeState> {
    let ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id,
        round_id: &round.id,
        outer_node_id: &outer_node.node_id,
        outer_attempt_id: attempt_id,
        dynamic,
    };
    let mut graph = load_or_create_dynamic_graph(&ctx)?;
    if graph.run.status == DynamicRunStatus::Paused {
        graph.run.status = DynamicRunStatus::Running;
        graph.run.outcome = None;
        graph.run.pause_reason = None;
        graph.run.updated_at = now_rfc3339_like();
        for node in &mut graph.nodes {
            if node.status == DynamicNodeStatus::Paused {
                node.status = DynamicNodeStatus::Ready;
                node.finished_at = None;
            }
        }
    }
    persist_dynamic_graph(&ctx, &graph)?;
    drive_dynamic_graph(&ctx, &mut graph)?;

    match (graph.run.status, graph.run.outcome) {
        (DynamicRunStatus::Completed, Some(RunOutcome::Success)) => {
            outer_node.status = RunStatus::Completed;
            outer_node.outcome = Some(NodeOutcome::Success);
            outer_node.finished_at = Some(now_rfc3339_like());
        }
        (DynamicRunStatus::Completed, Some(RunOutcome::Failure)) => {
            outer_node.status = RunStatus::Completed;
            outer_node.outcome = Some(NodeOutcome::Failure);
            outer_node.finished_at = Some(now_rfc3339_like());
        }
        (DynamicRunStatus::Completed, Some(RunOutcome::Killed)) => {
            outer_node.status = RunStatus::Completed;
            outer_node.outcome = Some(NodeOutcome::Killed);
            outer_node.finished_at = Some(now_rfc3339_like());
        }
        (DynamicRunStatus::Paused, _) => {
            outer_node.status = RunStatus::Paused;
            outer_node.outcome = None;
            outer_node.finished_at = Some(now_rfc3339_like());
        }
        _ => bail!(
            "AI-DYNAMIC node `{}` did not reach a terminal state",
            outer_node.node_id
        ),
    }
    crate::runtime::validate_node_state(&outer_node)?;
    Ok(outer_node)
}

fn load_or_create_dynamic_graph(ctx: &DynamicExecutionContext<'_>) -> Result<DynamicGraphState> {
    let graph_path = ctx.app.paths.dynamic_graph_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    if graph_path.exists() {
        return read_json(&graph_path);
    }

    let snapshots = freeze_allowed_workflow_snapshots(ctx.app, ctx.dynamic)?;
    let now = now_rfc3339_like();
    let dynamic_run_id = "dynamic-run-001".to_string();
    let bootstrap = DynamicNodeState {
        version: VERSION.to_string(),
        id: "bootstrap".to_string(),
        dynamic_run_id: dynamic_run_id.clone(),
        kind: DynamicNodeKind::Worker,
        title: "AI-DYNAMIC bootstrap".to_string(),
        task: "Design the first internal dynamic step for this AI-DYNAMIC node.".to_string(),
        status: DynamicNodeStatus::Ready,
        outcome: None,
        group_id: None,
        chain_id: "bootstrap".to_string(),
        depth: 0,
        depends_on: Vec::new(),
        workspace: WorkspacePolicy {
            mode: WorkspaceMode::Readonly,
        },
        workspace_path: Some(ctx.app.paths.repo_root.clone()),
        provider: ctx.dynamic.bootstrap_provider().map(ToOwned::to_owned),
        profile: None,
        permission_mode: ctx.dynamic.permission_mode().map(ToOwned::to_owned),
        session_mode: SessionMode::New,
        continue_from_node_id: None,
        workflow_id: None,
        workflow_snapshot_id: None,
        child_run_id: None,
        started_at: None,
        finished_at: None,
    };
    let run = DynamicRunState {
        version: VERSION.to_string(),
        id: dynamic_run_id,
        parent_run_id: ctx.run_id.to_string(),
        parent_round_id: ctx.round_id.to_string(),
        parent_node_id: ctx.outer_node_id.to_string(),
        parent_attempt_id: ctx.outer_attempt_id.to_string(),
        status: DynamicRunStatus::Running,
        outcome: None,
        pause_reason: None,
        started_at: now.clone(),
        updated_at: now,
        control: ctx.dynamic.control.clone(),
        allowed_workflow_snapshots: snapshots,
        current_node_ids: vec![bootstrap.id.clone()],
    };
    let graph = DynamicGraphState {
        version: VERSION.to_string(),
        run,
        nodes: vec![bootstrap],
        groups: Vec::new(),
        proposals: Vec::new(),
    };
    append_dynamic_event(
        ctx,
        "dynamic_run_started",
        serde_json::json!({
            "dynamicRunId": graph.run.id,
            "parentNodeId": ctx.outer_node_id,
            "parentAttemptId": ctx.outer_attempt_id,
        }),
    )?;
    Ok(graph)
}

fn drive_dynamic_graph(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<DynamicExecutionMessage>();
    loop {
        refresh_dynamic_ready_nodes(graph);
        launch_ready_dynamic_nodes(ctx, graph, &tx)?;
        persist_dynamic_graph(ctx, graph)?;

        if advance_dynamic_groups(ctx, graph)? {
            continue;
        }
        if dynamic_graph_completed(graph) {
            graph.run.status = DynamicRunStatus::Completed;
            graph.run.outcome = Some(RunOutcome::Success);
            graph.run.updated_at = now_rfc3339_like();
            persist_dynamic_graph(ctx, graph)?;
            append_dynamic_event(
                ctx,
                "dynamic_run_completed",
                serde_json::json!({
                    "dynamicRunId": graph.run.id,
                    "outcome": "success",
                }),
            )?;
            return Ok(());
        }

        if graph
            .nodes
            .iter()
            .any(|node| node.status == DynamicNodeStatus::Running)
        {
                let message = rx
                .recv()
                .map_err(|_| anyhow!("dynamic execution channel closed unexpectedly"))?;
            apply_dynamic_execution_message(ctx, graph, message)?;
            if graph.run.status == DynamicRunStatus::Paused {
                return Ok(());
            }
            continue;
        }

        pause_dynamic_graph(
            ctx,
            graph,
            graph.run.pause_reason.unwrap_or(PauseReason::ErrorBlocked),
            "dynamic graph has no ready node and is not complete",
        )?;
        bail!("AI-DYNAMIC graph `{}` is blocked", graph.run.id);
    }
}

fn launch_ready_dynamic_nodes(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    tx: &mpsc::Sender<DynamicExecutionMessage>,
) -> Result<()> {
    let ready_ids = graph
        .nodes
        .iter()
        .filter(|node| node.status == DynamicNodeStatus::Ready)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    for node_id in ready_ids {
        let Some(index) = graph.nodes.iter().position(|node| node.id == node_id) else {
            continue;
        };
        let node = graph
            .nodes
            .get_mut(index)
            .ok_or_else(|| anyhow!("dynamic node index out of range"))?;
        node.status = DynamicNodeStatus::Running;
        node.started_at.get_or_insert_with(now_rfc3339_like);
        let node_clone = node.clone();
        graph.run.updated_at = now_rfc3339_like();
        persist_dynamic_graph(ctx, graph)?;

        let background_app = ctx.app.clone_for_background();
        let task_id = ctx.task_id.to_string();
        let run_id = ctx.run_id.to_string();
        let round_id = ctx.round_id.to_string();
        let outer_node_id = ctx.outer_node_id.to_string();
        let outer_attempt_id = ctx.outer_attempt_id.to_string();
        let dynamic = ctx.dynamic.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            let app = background_app;
            let message = DynamicExecutionMessage {
                node_id: node_clone.id.clone(),
                result: execute_dynamic_node_job(
                    &app,
                    &task_id,
                    &run_id,
                    &round_id,
                    &outer_node_id,
                    &outer_attempt_id,
                    &dynamic,
                    node_clone,
                ),
            };
            let _ = tx.send(message);
        });
    }
    Ok(())
}

fn apply_dynamic_execution_message(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    message: DynamicExecutionMessage,
) -> Result<()> {
    let index = graph
        .nodes
        .iter()
        .position(|node| node.id == message.node_id)
        .ok_or_else(|| anyhow!("dynamic node `{}` missing from graph", message.node_id))?;
    let result = message.result?;
    graph.nodes[index] = result.node;
    if graph.nodes[index].status == DynamicNodeStatus::Paused {
        let pause_reason = match graph.nodes[index].kind {
            DynamicNodeKind::WorkflowInvocation => {
                let child_run_id = graph.nodes[index]
                    .child_run_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("paused workflow invocation missing child run id"))?;
                ctx.app
                    .run_status(ctx.task_id, child_run_id)?
                    .pause_reason
                    .unwrap_or(PauseReason::ProcessInterrupted)
            }
            _ => PauseReason::ProcessInterrupted,
        };
        pause_dynamic_graph(
            ctx,
            graph,
            pause_reason,
            "dynamic node paused and is waiting to continue",
        )?;
        return Ok(());
    }
    let mut accepted_any = false;
    let mut rejected_source_node_id = None;
    for proposal in result.proposals {
        let source_index = graph
            .nodes
            .iter()
            .position(|node| node.id == proposal.source_node_id)
            .ok_or_else(|| anyhow!("dynamic proposal source node `{}` missing", proposal.source_node_id))?;
        if proposal.validation_status == DynamicProposalValidationStatus::Rejected {
            rejected_source_node_id = Some(graph.nodes[source_index].id.clone());
            graph.proposals.push(proposal);
            continue;
        }
        let completion: DynamicNodeCompletion = serde_json::from_value(proposal.parsed.clone())?;
        accepted_any = true;
        graph.proposals.push(proposal.clone());
        materialize_dynamic_next(ctx, graph, source_index, completion.next)?;
        append_dynamic_event(
            ctx,
            "dynamic_proposal_accepted",
            serde_json::json!({
                "proposalId": proposal.id,
                "sourceNodeId": proposal.source_node_id,
            }),
        )?;
    }
    if !accepted_any {
        if let Some(source_node_id) = rejected_source_node_id {
            pause_dynamic_graph(
                ctx,
                graph,
                PauseReason::ErrorBlocked,
                "invalid dynamic-node-completion proposal",
            )?;
            return Err(anyhow!("dynamic proposal from `{source_node_id}` was rejected"));
        }
    }
    graph.run.updated_at = now_rfc3339_like();
    persist_dynamic_graph(ctx, graph)
}

fn execute_dynamic_node_job(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic: &AiDynamicNode,
    node: DynamicNodeState,
) -> Result<DynamicExecutionResult> {
    let dynamic_run_path = app
        .paths
        .dynamic_run_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let graph_path = app
        .paths
        .dynamic_graph_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let run: DynamicRunState = read_json(&dynamic_run_path)?;
    let mut graph: DynamicGraphState = read_json(&graph_path)?;
    let ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        dynamic,
    };
    let index = graph
        .nodes
        .iter()
        .position(|candidate| candidate.id == node.id)
        .ok_or_else(|| anyhow!("dynamic node `{}` missing from graph", node.id))?;
    graph.run = run;
    graph.nodes[index] = node.clone();
    match node.kind {
        DynamicNodeKind::Worker => execute_dynamic_worker(&ctx, &graph, node),
        DynamicNodeKind::WorkflowInvocation => execute_dynamic_workflow_invocation(&ctx, &graph, node),
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => execute_dynamic_agent_stage(&ctx, &graph, node),
    }
}

fn dynamic_node_continue_ref(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    attempt_id: &str,
) -> Option<serde_json::Value> {
    read_json::<WorkerRefState>(&ctx.app.paths.dynamic_node_worker_ref_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        &node.id,
        attempt_id,
    ))
    .ok()
    .and_then(|worker_ref| worker_ref.continue_ref)
}

fn dynamic_continue_ref_for_source_node(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    source_node_id: &str,
) -> Option<serde_json::Value> {
    let target = graph.nodes.iter().find(|node| node.id == source_node_id)?;
    dynamic_node_continue_ref(ctx, target, &dynamic_attempt_id(target))
}

fn execute_dynamic_worker(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    mut node: DynamicNodeState,
) -> Result<DynamicExecutionResult> {
    ensure_dynamic_workspace(ctx, &mut node)?;
    let attempt_id = dynamic_attempt_id(&node);
    prepare_dynamic_attempt_dirs(ctx, &node, &attempt_id)?;
    let provider_id = node
        .provider
        .as_deref()
        .ok_or_else(|| anyhow!("dynamic worker `{}` is missing provider", node.id))?
        .to_string();
    let worker_ref_path = ctx.app.paths.dynamic_node_worker_ref_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        &node.id,
        &attempt_id,
    );
    let mut proposal_repair_prompts = 0;
    let mut continue_ref = match node.session_mode {
        SessionMode::Continue => node
            .continue_from_node_id
            .as_deref()
            .and_then(|source_node_id| dynamic_continue_ref_for_source_node(ctx, graph, source_node_id)),
        SessionMode::New => dynamic_node_continue_ref(ctx, &node, &attempt_id),
    };
    let mut session_mode = if continue_ref.is_some() {
        SessionMode::Continue
    } else {
        SessionMode::New
    };
    let mut resume_prompt = if continue_ref.is_some() {
        Some(localized_continue_prompt(ctx.app.config.desktop_language))
    } else {
        None
    };
    let mut resume_prompt_visibility = PromptVisibility::Visible;
    let mut proposals = Vec::new();

    loop {
        let live_update_context = dynamic_acp_live_event_context(ctx, &node.id, &attempt_id);
        let live_update = ctx.app.acp_live_update_for(live_update_context);
        let invocation = build_dynamic_worker_invocation(
            ctx,
            graph,
            &node,
            &attempt_id,
            Some(dynamic_output_contract(ctx.app.config.desktop_language, ctx.dynamic)),
            session_mode,
            continue_ref.clone(),
            resume_prompt.take(),
            None,
            resume_prompt_visibility,
        )
        .map_err(|error| anyhow!("failed to build dynamic worker invocation for `{}`: {error}", node.id))?;
        append_dynamic_event(
            ctx,
            "dynamic_node_started",
            serde_json::json!({
                "nodeId": node.id,
                "kind": node.kind,
                "sessionMode": session_mode,
            }),
        )
        .map_err(|error| anyhow!("failed to append dynamic start event for `{}`: {error}", node.id))?;
        let result = ctx
            .app
            .provider_for_id(&provider_id)
            .map_err(|error| anyhow!("failed to resolve provider `{}` for `{}`: {error}", provider_id, node.id))?
            .run_worker_with_live_update(invocation, live_update.as_ref().map(|callback| callback as _))
            .map_err(|error| anyhow!("provider `{}` failed to run `{}`: {error}", provider_id, node.id))?;
        finalize_dynamic_worker_result(ctx, &mut node, &attempt_id, result)?;
        if node.status == DynamicNodeStatus::Paused {
            return Ok(DynamicExecutionResult {
                node,
                proposals: Vec::new(),
            });
        }
        if node.outcome != Some(NodeOutcome::Success) {
            bail!("dynamic worker `{}` failed", node.id);
        }
        match build_dynamic_completion_from_artifact(ctx, &attempt_id, &node) {
            Ok(proposal) if proposal.validation_status == DynamicProposalValidationStatus::Accepted => {
                proposals.push(proposal);
                append_dynamic_event(
                    ctx,
                    "dynamic_node_completed",
                    serde_json::json!({
                        "nodeId": node.id,
                        "kind": node.kind,
                        "outcome": node.outcome,
                    }),
                )?;
                return Ok(DynamicExecutionResult { node, proposals });
            }
            Ok(proposal) if proposal_repair_prompts < MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS => {
                let repair_continue_ref = read_json::<WorkerRefState>(&worker_ref_path)
                    .ok()
                    .and_then(|worker_ref| worker_ref.continue_ref);
                let validation_error = dynamic_validation_error_lines(&proposal.validation_errors);
                let validation_errors = proposal.validation_errors.clone();
                proposals.push(proposal);
                let Some(repair_continue_ref) = repair_continue_ref else {
                    append_dynamic_event(
                        ctx,
                        "dynamic_proposal_repair_exhausted",
                        serde_json::json!({
                            "nodeId": node.id,
                            "attemptId": attempt_id,
                            "repairAttempts": proposal_repair_prompts,
                            "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                            "error": validation_error,
                            "validationErrors": validation_errors,
                        }),
                    )?;
                    return Ok(DynamicExecutionResult { node, proposals });
                };
                proposal_repair_prompts += 1;
                append_dynamic_event(
                    ctx,
                    "dynamic_proposal_repair_requested",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "repairAttempt": proposal_repair_prompts,
                        "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                        "error": validation_error,
                        "validationErrors": validation_errors,
                    }),
                )?;
                session_mode = SessionMode::Continue;
                continue_ref = Some(repair_continue_ref);
                resume_prompt = Some(dynamic_proposal_repair_prompt(ctx, graph, &node, &validation_errors));
                resume_prompt_visibility = PromptVisibility::Hidden;
                node.status = DynamicNodeStatus::Running;
                node.outcome = None;
                node.finished_at = None;
                continue;
            }
            Ok(proposal) => {
                let validation_error = dynamic_validation_error_lines(&proposal.validation_errors);
                let validation_errors = proposal.validation_errors.clone();
                proposals.push(proposal);
                append_dynamic_event(
                    ctx,
                    "dynamic_proposal_repair_exhausted",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "repairAttempts": proposal_repair_prompts,
                        "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                        "error": validation_error,
                        "validationErrors": validation_errors,
                    }),
                )?;
                return Ok(DynamicExecutionResult { node, proposals });
            }
            Err(err) if proposal_repair_prompts < MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS => {
                let repair_continue_ref = read_json::<WorkerRefState>(&worker_ref_path)
                    .ok()
                    .and_then(|worker_ref| worker_ref.continue_ref);
                let Some(repair_continue_ref) = repair_continue_ref else {
                    return Err(err);
                };
                proposal_repair_prompts += 1;
                append_dynamic_event(
                    ctx,
                    "dynamic_proposal_repair_requested",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "repairAttempt": proposal_repair_prompts,
                        "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                        "error": err.to_string(),
                    }),
                )?;
                session_mode = SessionMode::Continue;
                continue_ref = Some(repair_continue_ref);
                resume_prompt = Some(dynamic_text_repair_prompt(ctx, graph, &node, err.to_string()));
                resume_prompt_visibility = PromptVisibility::Hidden;
                node.status = DynamicNodeStatus::Running;
                node.outcome = None;
                node.finished_at = None;
                continue;
            }
            Err(err) => {
                append_dynamic_event(
                    ctx,
                    "dynamic_proposal_repair_exhausted",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "repairAttempts": proposal_repair_prompts,
                        "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                        "error": err.to_string(),
                    }),
                )?;
                return Err(err);
            }
        }
    }
}

fn execute_dynamic_agent_stage(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    mut node: DynamicNodeState,
) -> Result<DynamicExecutionResult> {
    node.workspace_path = Some(ctx.app.paths.repo_root.clone());
    let attempt_id = dynamic_attempt_id(&node);
    prepare_dynamic_attempt_dirs(ctx, &node, &attempt_id)?;
    let continue_ref = dynamic_node_continue_ref(ctx, &node, &attempt_id);
    let session_mode = if continue_ref.is_some() {
        SessionMode::Continue
    } else {
        SessionMode::New
    };
    let resume_prompt = if continue_ref.is_some() {
        Some(localized_continue_prompt(ctx.app.config.desktop_language))
    } else {
        None
    };
    let live_update_context = dynamic_acp_live_event_context(ctx, &node.id, &attempt_id);
    let live_update = ctx.app.acp_live_update_for(live_update_context);
    let invocation = build_dynamic_worker_invocation(
        ctx,
        graph,
        &node,
        &attempt_id,
        None,
        session_mode,
        continue_ref,
        resume_prompt,
        None,
        PromptVisibility::Visible,
    )?;
    let provider_id = node
        .provider
        .as_deref()
        .ok_or_else(|| anyhow!("dynamic stage `{}` is missing provider", node.id))?;
    append_dynamic_event(
        ctx,
        "dynamic_node_started",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
        }),
    )?;
    let result = ctx
        .app
        .provider_for_id(provider_id)?
        .run_worker_with_live_update(invocation, live_update.as_ref().map(|callback| callback as _))?;
    finalize_dynamic_worker_result(ctx, &mut node, &attempt_id, result)?;
    if node.status == DynamicNodeStatus::Paused {
        return Ok(DynamicExecutionResult {
            node,
            proposals: Vec::new(),
        });
    }
    if node.outcome != Some(NodeOutcome::Success) {
        bail!("dynamic stage `{}` failed", node.id);
    }
    append_dynamic_event(
        ctx,
        "dynamic_node_completed",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "outcome": node.outcome,
        }),
    )?;
    Ok(DynamicExecutionResult {
        node,
        proposals: Vec::new(),
    })
}

fn execute_dynamic_workflow_invocation(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    mut node: DynamicNodeState,
) -> Result<DynamicExecutionResult> {
    ensure_dynamic_workspace(ctx, &mut node)?;
    let workflow_id = node
        .workflow_id
        .as_deref()
        .ok_or_else(|| anyhow!("workflow invocation `{}` is missing workflowId", node.id))?;
    let snapshot = graph
        .run
        .allowed_workflow_snapshots
        .iter()
        .find(|snapshot| snapshot.workflow_id == workflow_id)
        .ok_or_else(|| {
            anyhow!(
                "workflow invocation `{}` references a workflow that is not allowed",
                node.id
            )
        })?;
    ensure!(
        ctx.dynamic.control.allow_nested_dynamic || !snapshot.contains_ai_dynamic,
        "workflow invocation `{}` references a nested AI-DYNAMIC snapshot",
        node.id
    );

    let attempt_id = dynamic_attempt_id(&node);
    prepare_dynamic_attempt_dirs(ctx, &node, &attempt_id)?;
    let child_workflow = workflow_with_dynamic_invocation_task(
        ctx.app.config.desktop_language,
        snapshot.workflow.clone(),
        &node.task,
    );
    let child_workflow_path = ctx
        .app
        .paths
        .dynamic_node_attempt_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &node.id,
            &attempt_id,
        )
        .join("child-workflow.snapshot.json");
    write_json(&child_workflow_path, &child_workflow)?;
    append_dynamic_event(
        ctx,
        "dynamic_child_workflow_started",
        serde_json::json!({
            "nodeId": node.id,
            "workflowId": workflow_id,
            "snapshotId": snapshot.snapshot_id,
        }),
    )?;
    let child_run = match node.child_run_id.as_deref() {
        Some(child_run_id) => ctx.app.run_continue(ctx.task_id, child_run_id, None)?,
        None => ctx
            .app
            .run_start(ctx.task_id, Some(child_workflow_path.as_path()))?,
    };
    node.child_run_id = Some(child_run.id.clone());
    match child_run.status {
        RunStatus::Paused => {
            let pause_reason = child_run
                .pause_reason
                .unwrap_or(PauseReason::ProcessInterrupted);
            node.status = DynamicNodeStatus::Paused;
            node.outcome = None;
            node.finished_at = Some(now_rfc3339_like());
            append_dynamic_event(
                ctx,
                "dynamic_child_workflow_paused",
                serde_json::json!({
                    "nodeId": node.id,
                    "workflowId": workflow_id,
                    "childRunId": child_run.id,
                    "pauseReason": pause_reason,
                }),
            )?;
            return Ok(DynamicExecutionResult {
                node,
                proposals: Vec::new(),
            });
        }
        RunStatus::Completed => {
            node.finished_at = Some(now_rfc3339_like());
            node.status = DynamicNodeStatus::Completed;
            node.outcome = Some(match child_run.outcome {
                Some(RunOutcome::Success) => NodeOutcome::Success,
                Some(RunOutcome::Killed) => NodeOutcome::Killed,
                _ => NodeOutcome::Failure,
            });
        }
        RunStatus::Running => {
            bail!("child workflow invocation `{}` is still running", node.id);
        }
    }
    append_dynamic_event(
        ctx,
        "dynamic_child_workflow_completed",
        serde_json::json!({
            "nodeId": node.id,
            "workflowId": workflow_id,
            "childRunId": child_run.id,
            "outcome": child_run.outcome,
            "status": child_run.status,
        }),
    )?;
    if node.outcome != Some(NodeOutcome::Success) {
        match node.outcome {
            Some(NodeOutcome::Killed) => {
                node.status = DynamicNodeStatus::Paused;
                node.outcome = None;
                return Ok(DynamicExecutionResult {
                    node,
                    proposals: Vec::new(),
                });
            }
            _ => bail!("child workflow invocation `{}` failed", node.id),
        }
    }
    let proposal_id = format!("proposal-{}-001", safe_dynamic_ref(&node.id));
    let completion = DynamicNodeCompletion {
        version: VERSION.to_string(),
        kind: DynamicNodeCompletionKind::DynamicNodeCompletion,
        status: DynamicCompletionStatus::Success,
        summary: format!("workflow {workflow_id} completed successfully"),
        next: DynamicNext::End,
        source: Some(serde_json::json!({
            "kind": "workflow-run",
            "childRunId": child_run.id,
        })),
    };
    let proposal = build_dynamic_completion_proposal(
        ctx,
        &node,
        completion,
        Some(dynamic_proposal_file_path(ctx, &proposal_id)),
        Some(
            ctx.app
                .paths
                .dynamic_node_attempt_dir(
                    ctx.task_id,
                    ctx.run_id,
                    ctx.round_id,
                    ctx.outer_node_id,
                    ctx.outer_attempt_id,
                    &node.id,
                    &attempt_id,
                )
                .join("raw.stream.jsonl"),
        ),
    )?;
    Ok(DynamicExecutionResult {
        node,
        proposals: vec![proposal],
    })
}

fn workflow_with_dynamic_invocation_task(
    language: DesktopLanguage,
    mut workflow: WorkflowDsl,
    task: &str,
) -> WorkflowDsl {
    for node in &mut workflow.nodes {
        if let NodeDsl::Worker(worker) = node {
            worker.goal = Some(match worker.goal.as_deref() {
                Some(goal) if !goal.trim().is_empty() => render_template(
                    prompt_by_language(
                        language,
                        AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN,
                        AI_DYNAMIC_WORKFLOW_INVOCATION_EN,
                    ),
                    serde_json::json!({
                        "invocation_task": task.trim(),
                        "node_goal": goal.trim(),
                    }),
                )
                .expect("prompt template renders"),
                _ => task.trim().to_string(),
            });
        }
    }
    workflow
}

fn finalize_dynamic_worker_result(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
    attempt_id: &str,
    result: ProviderRunResult,
) -> Result<()> {
    let node_id = node.id.clone();
    node.finished_at = Some(now_rfc3339_like());
    match result.status {
        ProviderRunStatus::Success => {
            if let Some(payload) = result.result_payload {
                if let Some(output_artifact) = payload.output_artifact {
                    let artifact_path = ctx.app.paths.dynamic_node_artifact_file(
                        ctx.task_id,
                        ctx.run_id,
                        ctx.round_id,
                        ctx.outer_node_id,
                        ctx.outer_attempt_id,
                        &node_id,
                        attempt_id,
                        &output_artifact.name,
                    );
                    std::fs::create_dir_all(
                        ctx.app
                            .paths
                            .dynamic_node_artifacts_dir(
                                ctx.task_id,
                                ctx.run_id,
                                ctx.round_id,
                                ctx.outer_node_id,
                                ctx.outer_attempt_id,
                                &node_id,
                                attempt_id,
                            )
                            .as_std_path(),
                    )?;
                    std::fs::write(artifact_path.as_std_path(), output_artifact.content)?;
                }
            }
            if let Some(seed) = result.worker_ref_seed {
                let worker_ref = WorkerRefState {
                    version: VERSION.to_string(),
                    provider: seed.provider,
                    mode: seed.mode,
                    supports_open_session: seed.supports_open_session,
                    supports_continue_session: seed.supports_continue_session,
                    continue_ref: seed.continue_ref,
                    open_command: seed.open_command,
                };
                validate_worker_ref_state(&worker_ref)?;
                write_json(
                    &ctx.app.paths.dynamic_node_worker_ref_file(
                        ctx.task_id,
                        ctx.run_id,
                        ctx.round_id,
                        ctx.outer_node_id,
                        ctx.outer_attempt_id,
                        &node_id,
                        attempt_id,
                    ),
                    &worker_ref,
                )?;
            }
            node.status = DynamicNodeStatus::Completed;
            node.outcome = Some(NodeOutcome::Success);
        }
        ProviderRunStatus::Failure => {
            node.status = DynamicNodeStatus::Completed;
            node.outcome = Some(NodeOutcome::Failure);
        }
        ProviderRunStatus::Interrupted
        | ProviderRunStatus::WaitingForUserInput
        | ProviderRunStatus::PermissionRequested => {
            node.status = DynamicNodeStatus::Paused;
            node.outcome = None;
        }
    }
    validate_dynamic_node_state(node)
}

fn build_dynamic_completion_from_artifact(
    ctx: &DynamicExecutionContext<'_>,
    attempt_id: &str,
    node: &DynamicNodeState,
) -> Result<DynamicProposalState> {
    let artifact_path = ctx.app.paths.dynamic_node_artifact_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        &node.id,
        attempt_id,
        DYNAMIC_COMPLETION_ARTIFACT,
    );
    ensure!(
        artifact_path.exists(),
        "dynamic node `{}` did not produce dynamic-node-completion",
        node.id
    );
    let raw = std::fs::read_to_string(artifact_path.as_std_path())?;
    let completion: DynamicNodeCompletion = parse_json_artifact(&raw)?;
    let raw_output_path = ctx
        .app
        .paths
        .dynamic_node_attempt_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &node.id,
            attempt_id,
        )
        .join("raw.stream.jsonl");
    build_dynamic_completion_proposal(ctx, node, completion, Some(artifact_path), Some(raw_output_path))
}

fn build_dynamic_completion_proposal(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    completion: DynamicNodeCompletion,
    artifact_path: Option<Utf8PathBuf>,
    raw_output_path: Option<Utf8PathBuf>,
) -> Result<DynamicProposalState> {
    let graph: DynamicGraphState = read_json(&ctx.app.paths.dynamic_graph_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    ))?;
    let index = graph
        .nodes
        .iter()
        .position(|candidate| candidate.id == node.id)
        .ok_or_else(|| anyhow!("dynamic source node `{}` missing", node.id))?;
    let source_node_id = node.id.clone();
    let proposal_id = format!("proposal-{}-001", safe_dynamic_ref(&source_node_id));
    let proposal_artifact_path =
        artifact_path.unwrap_or_else(|| dynamic_proposal_file_path(ctx, &proposal_id));
    let proposal_raw_output_path = raw_output_path.unwrap_or_else(|| {
        ctx.app
            .paths
            .dynamic_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
            )
            .join("events.jsonl")
    });
    let parsed = serde_json::to_value(&completion)?;
    let validation_errors = validate_dynamic_completion(ctx, &graph, index, &completion);
    if validation_errors.is_empty() {
        Ok(DynamicProposalState {
            version: VERSION.to_string(),
            id: proposal_id,
            dynamic_run_id: graph.run.id,
            source_node_id,
            artifact_path: proposal_artifact_path,
            raw_output_path: proposal_raw_output_path,
            parsed,
            validation_status: DynamicProposalValidationStatus::Accepted,
            validation_errors: Vec::new(),
            materialized_event_ids: Vec::new(),
            created_at: now_rfc3339_like(),
        })
    } else {
        let error_message = dynamic_validation_error_lines(&validation_errors);
        append_dynamic_event(
            ctx,
            "dynamic_proposal_rejected",
            serde_json::json!({
                "proposalId": proposal_id,
                "sourceNodeId": source_node_id,
                "error": error_message,
                "validationErrors": validation_errors,
            }),
        )?;
        Ok(DynamicProposalState {
            version: VERSION.to_string(),
            id: proposal_id,
            dynamic_run_id: graph.run.id,
            source_node_id,
            artifact_path: proposal_artifact_path,
            raw_output_path: proposal_raw_output_path,
            parsed,
            validation_status: DynamicProposalValidationStatus::Rejected,
            validation_errors,
            materialized_event_ids: Vec::new(),
            created_at: now_rfc3339_like(),
        })
    }
}

fn validate_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    source_index: usize,
    completion: &DynamicNodeCompletion,
) -> Vec<DynamicProposalValidationError> {
    let mut errors = Vec::new();
    if completion.version != VERSION {
        errors.push(dynamic_validation_error(
            "dynamic.version.unsupported",
            "unsupported dynamic completion version",
            serde_json::json!({
                "field": "version",
                "value": completion.version,
                "expected": VERSION,
            }),
        ));
    }
    if completion.kind != DynamicNodeCompletionKind::DynamicNodeCompletion {
        errors.push(dynamic_validation_error(
            "dynamic.kind.invalid",
            "dynamic completion kind must be dynamic-node-completion",
            serde_json::json!({
                "field": "kind",
                "value": completion.kind,
            }),
        ));
    }
    if completion.status != DynamicCompletionStatus::Success {
        errors.push(dynamic_validation_error(
            "dynamic.status.invalid",
            "dynamic completion status must be success",
            serde_json::json!({
                "field": "status",
                "value": completion.status,
            }),
        ));
    }
    if completion.summary.trim().is_empty() {
        errors.push(dynamic_validation_error(
            "dynamic.summary.blank",
            "dynamic completion summary cannot be blank",
            serde_json::json!({
                "field": "summary",
            }),
        ));
    }
    let source_node_id = graph
        .nodes
        .get(source_index)
        .map(|node| node.id.clone())
        .unwrap_or_default();
    if graph.proposals.iter().any(|proposal| {
        proposal.source_node_id == source_node_id
            && proposal.validation_status == DynamicProposalValidationStatus::Accepted
    }) {
        let node_id = graph
            .nodes
            .get(source_index)
            .map(|node| node.id.clone())
            .unwrap_or_else(|| "unknown".to_string());
        errors.push(dynamic_validation_error(
            "dynamic.proposal.duplicate-accepted",
            format!("dynamic node `{node_id}` already has an accepted completion proposal"),
            serde_json::json!({
                "nodeId": node_id,
            }),
        ));
    }
    let Some(source) = graph.nodes.get(source_index) else {
        errors.push(dynamic_validation_error(
            "dynamic.source.missing",
            "dynamic source node missing",
            serde_json::json!({}),
        ));
        return errors;
    };
    match &completion.next {
        DynamicNext::End => {}
        DynamicNext::Single { node } => {
            errors.extend(validate_dynamic_node_spec(ctx, graph, source, node, 1));
        }
        DynamicNext::Fanout {
            group_id,
            nodes,
            merge,
            acceptance,
        } => {
            if group_id.trim().is_empty() {
                errors.push(dynamic_validation_error(
                    "dynamic.fanout.group-id.blank",
                    "dynamic fanout groupId cannot be blank",
                    serde_json::json!({
                        "field": "next.groupId",
                    }),
                ));
            }
            if graph.groups.iter().any(|group| group.id == *group_id) {
                errors.push(dynamic_validation_error(
                    "dynamic.fanout.group-id.duplicate",
                    format!("dynamic fanout group `{group_id}` already exists"),
                    serde_json::json!({
                        "field": "next.groupId",
                        "groupId": group_id,
                    }),
                ));
            }
            if nodes.is_empty() {
                errors.push(dynamic_validation_error(
                    "dynamic.fanout.nodes.empty",
                    "dynamic fanout must create at least one node",
                    serde_json::json!({
                        "field": "next.nodes",
                    }),
                ));
            }
            if nodes.len() as u32 > graph.run.control.max_fanout {
                errors.push(dynamic_validation_error(
                    "dynamic.fanout.max-fanout-exceeded",
                    "dynamic fanout exceeds maxFanout",
                    serde_json::json!({
                        "field": "next.nodes",
                        "limit": graph.run.control.max_fanout,
                        "actual": nodes.len(),
                    }),
                ));
            }
            errors.extend(validate_dynamic_agent_task_spec(ctx, merge, "merge"));
            errors.extend(validate_dynamic_agent_task_spec(ctx, acceptance, "acceptance"));
            let group_depth = source
                .group_id
                .as_deref()
                .and_then(|group_id| graph.groups.iter().find(|group| group.id == group_id))
                .map(|group| group.depth + 1)
                .unwrap_or(1);
            if group_depth > graph.run.control.max_group_depth {
                errors.push(dynamic_validation_error(
                    "dynamic.fanout.max-group-depth-exceeded",
                    "dynamic fanout exceeds maxGroupDepth",
                    serde_json::json!({
                        "limit": graph.run.control.max_group_depth,
                        "actual": group_depth,
                    }),
                ));
            }
            if graph.nodes.len() + nodes.len() + 2 > graph.run.control.max_dynamic_nodes as usize {
                errors.push(dynamic_validation_error(
                    "dynamic.graph.max-nodes-exceeded",
                    "dynamic graph exceeds maxDynamicNodes",
                    serde_json::json!({
                        "limit": graph.run.control.max_dynamic_nodes,
                        "actual": graph.nodes.len() + nodes.len() + 2,
                    }),
                ));
            }
            let mut ids = HashSet::new();
            for node in nodes {
                if !ids.insert(node.id.trim().to_string()) {
                    errors.push(dynamic_validation_error(
                        "dynamic.fanout.node-id.duplicate",
                        "dynamic fanout node id is duplicated",
                        serde_json::json!({
                            "nodeId": node.id,
                        }),
                    ));
                }
                errors.extend(validate_dynamic_node_spec(ctx, graph, source, node, nodes.len()));
            }
        }
    }
    errors
}

fn validate_dynamic_node_spec(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    source: &DynamicNodeState,
    spec: &DynamicNodeSpec,
    additional_nodes: usize,
) -> Vec<DynamicProposalValidationError> {
    let mut errors = Vec::new();
    let resumable_nodes = dynamic_resumable_session_nodes(graph, source);
    if spec.id.trim().is_empty() {
        errors.push(dynamic_validation_error(
            "dynamic.node.id.blank",
            "dynamic node id cannot be blank",
            serde_json::json!({
                "field": "id",
            }),
        ));
    }
    if graph.nodes.iter().any(|node| node.id == spec.id) {
        errors.push(dynamic_validation_error(
            "dynamic.node.id.duplicate",
            format!("dynamic node `{}` already exists", spec.id),
            serde_json::json!({
                "nodeId": spec.id,
                "field": "id",
            }),
        ));
    }
    if spec.title.trim().is_empty() {
        errors.push(dynamic_validation_error(
            "dynamic.node.title.blank",
            format!("dynamic node `{}` title cannot be blank", spec.id),
            serde_json::json!({
                "nodeId": spec.id,
                "field": "title",
            }),
        ));
    }
    if spec.task.trim().is_empty() {
        errors.push(dynamic_validation_error(
            "dynamic.node.task.blank",
            format!("dynamic node `{}` task cannot be blank", spec.id),
            serde_json::json!({
                "nodeId": spec.id,
                "field": "task",
            }),
        ));
    }
    if source.depth + 1 > graph.run.control.max_depth {
        errors.push(dynamic_validation_error(
            "dynamic.node.max-depth-exceeded",
            format!("dynamic node `{}` exceeds maxDepth", spec.id),
            serde_json::json!({
                "nodeId": spec.id,
                "limit": graph.run.control.max_depth,
                "actual": source.depth + 1,
            }),
        ));
    }
    if graph.nodes.len() + additional_nodes > graph.run.control.max_dynamic_nodes as usize {
        errors.push(dynamic_validation_error(
            "dynamic.graph.max-nodes-exceeded",
            "dynamic graph exceeds maxDynamicNodes",
            serde_json::json!({
                "limit": graph.run.control.max_dynamic_nodes,
                "actual": graph.nodes.len() + additional_nodes,
            }),
        ));
    }
    for dependency in &spec.depends_on {
        if !graph.nodes.iter().any(|node| node.id == *dependency) {
            errors.push(dynamic_validation_error(
                "dynamic.node.depends-on.unknown",
                format!("dynamic node `{}` depends on unknown node `{dependency}`", spec.id),
                serde_json::json!({
                    "nodeId": spec.id,
                    "dependency": dependency,
                }),
            ));
        }
    }
    match spec.session_mode {
        SessionMode::New => {
            if let Some(continue_from_node_id) = spec.continue_from_node_id.as_deref() {
                errors.push(dynamic_validation_error(
                    "dynamic.node.session.continue-from-with-new",
                    format!("dynamic node `{}` cannot set continueFromNodeId when session is new", spec.id),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "field": "continueFromNodeId",
                        "continueFromNodeId": continue_from_node_id,
                    }),
                ));
            }
        }
        SessionMode::Continue => {
            let Some(continue_from_node_id) = spec.continue_from_node_id.as_deref() else {
                errors.push(dynamic_validation_error(
                    "dynamic.node.session.continue-from-missing",
                    format!("dynamic node `{}` must provide continueFromNodeId when session is continue", spec.id),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "field": "continueFromNodeId",
                    }),
                ));
                return errors;
            };
            if spec.kind == DynamicNodeSpecKind::WorkflowInvocation {
                errors.push(dynamic_validation_error(
                    "dynamic.node.session.workflow-invocation-disallowed",
                    format!("workflow invocation `{}` cannot use continue session", spec.id),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "continueFromNodeId": continue_from_node_id,
                    }),
                ));
            }
            match resumable_nodes
                .iter()
                .find(|node| node.id == continue_from_node_id)
            {
                Some(target) => {
                    if dynamic_node_continue_ref(ctx, target, &dynamic_attempt_id(target)).is_none() {
                        errors.push(dynamic_validation_error(
                            "dynamic.node.session.continue-target-missing-ref",
                            format!("dynamic node `{}` cannot continue from `{}` because it has no continue ref", spec.id, continue_from_node_id),
                            serde_json::json!({
                                "nodeId": spec.id,
                                "continueFromNodeId": continue_from_node_id,
                            }),
                        ));
                    }
                    if spec.kind == DynamicNodeSpecKind::Worker {
                        if let Some(provider) = spec.provider.as_deref() {
                            if target.provider.as_deref() != Some(provider) {
                                errors.push(dynamic_validation_error(
                                    "dynamic.node.session.provider-mismatch",
                                    format!("dynamic node `{}` must use the same provider as continue source `{}`", spec.id, continue_from_node_id),
                                    serde_json::json!({
                                        "nodeId": spec.id,
                                        "provider": provider,
                                        "continueFromNodeId": continue_from_node_id,
                                        "expectedProvider": target.provider,
                                    }),
                                ));
                            }
                        }
                    }
                }
                None => errors.push(dynamic_validation_error(
                    "dynamic.node.session.continue-target-unavailable",
                    format!("dynamic node `{}` cannot continue from `{}`", spec.id, continue_from_node_id),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "continueFromNodeId": continue_from_node_id,
                    }),
                )),
            }
        }
    }
    match spec.kind {
        DynamicNodeSpecKind::Worker => match spec.provider.as_deref() {
            Some(provider) if !provider.trim().is_empty() => {
                if ctx.app.provider_for_id(provider).is_err() {
                    errors.push(dynamic_validation_error(
                        "dynamic.node.provider.unknown",
                        format!("dynamic worker `{}` references unknown provider `{provider}`", spec.id),
                        serde_json::json!({
                            "nodeId": spec.id,
                            "provider": provider,
                        }),
                    ));
                } else if let Some(permission_mode) = ctx.dynamic.permission_mode() {
                    if let Ok(doctor) = ctx.app.provider_doctor(provider) {
                        let supported_mode_ids = doctor
                            .supported_modes()
                            .into_iter()
                            .map(|mode| mode.id)
                            .collect::<std::collections::HashSet<_>>();
                        if !supported_mode_ids.is_empty() && !supported_mode_ids.contains(permission_mode) {
                            errors.push(dynamic_validation_error(
                                "dynamic.node.permission-mode.unsupported",
                                format!("dynamic worker `{}` permissionMode `{}` is not supported by provider `{provider}`", spec.id, permission_mode),
                                serde_json::json!({
                                    "nodeId": spec.id,
                                    "provider": provider,
                                    "permissionMode": permission_mode,
                                }),
                            ));
                        }
                    }
                }
                if let Some(profile) = spec.profile.as_deref() {
                    let allowed = ctx
                        .dynamic
                        .allowed_profiles
                        .iter()
                        .map(|item| item.as_str())
                        .collect::<std::collections::HashSet<_>>();
                    if !allowed.is_empty() && !allowed.contains(profile) {
                        errors.push(dynamic_validation_error(
                            "dynamic.node.profile.unallowed",
                            format!("dynamic worker `{}` profile `{profile}` is not allowed by this AI-DYNAMIC node", spec.id),
                            serde_json::json!({
                                "nodeId": spec.id,
                                "profile": profile,
                            }),
                        ));
                    }
                }
            }
            _ => errors.push(dynamic_validation_error(
                "dynamic.node.provider.blank",
                format!("dynamic worker `{}` provider cannot be blank", spec.id),
                serde_json::json!({
                    "nodeId": spec.id,
                    "field": "provider",
                }),
            )),
        },
        DynamicNodeSpecKind::WorkflowInvocation => {
            let workflow_id = spec.workflow_id.as_deref();
            match workflow_id {
                Some(workflow_id) if !workflow_id.trim().is_empty() => {
                    match graph
                        .run
                        .allowed_workflow_snapshots
                        .iter()
                        .find(|snapshot| snapshot.workflow_id == workflow_id)
                    {
                        Some(snapshot) => {
                            if !graph.run.control.allow_nested_dynamic && snapshot.contains_ai_dynamic {
                                errors.push(dynamic_validation_error(
                                    "dynamic.workflow-invocation.nested-dynamic-disallowed",
                                    format!("workflow invocation `{}` references nested AI-DYNAMIC snapshot", spec.id),
                                    serde_json::json!({
                                        "nodeId": spec.id,
                                        "workflowId": workflow_id,
                                    }),
                                ));
                            }
                        }
                        None => errors.push(dynamic_validation_error(
                            "dynamic.workflow-invocation.workflow-unallowed",
                            format!("workflow invocation `{}` references unallowed workflow `{workflow_id}`", spec.id),
                            serde_json::json!({
                                "nodeId": spec.id,
                                "workflowId": workflow_id,
                            }),
                        )),
                    }
                }
                _ => errors.push(dynamic_validation_error(
                    "dynamic.workflow-invocation.workflow-id.blank",
                    format!("workflow invocation `{}` workflowId cannot be blank", spec.id),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "field": "workflowId",
                    }),
                )),
            }
            let invocation_count = graph
                .nodes
                .iter()
                .filter(|node| node.kind == DynamicNodeKind::WorkflowInvocation)
                .count()
                + 1;
            if invocation_count as u32 > graph.run.control.max_workflow_invocations {
                errors.push(dynamic_validation_error(
                    "dynamic.workflow-invocation.max-invocations-exceeded",
                    "workflow invocation count exceeds maxWorkflowInvocations",
                    serde_json::json!({
                        "limit": graph.run.control.max_workflow_invocations,
                        "actual": invocation_count,
                    }),
                ));
            }
        }
    }
    if let Some(profile) = spec.profile.as_deref() {
        errors.extend(validate_dynamic_profile_reference(
            ctx,
            profile,
            &format!("dynamic node `{}`", spec.id),
            serde_json::json!({
                "nodeId": spec.id,
                "field": "profile",
                "profile": profile,
            }),
        ));
    }
    errors
}

fn validate_dynamic_agent_task_spec(
    ctx: &DynamicExecutionContext<'_>,
    spec: &DynamicAgentTaskSpec,
    name: &str,
) -> Vec<DynamicProposalValidationError> {
    let mut errors = Vec::new();
    if spec.title.trim().is_empty() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.title.blank"),
            format!("dynamic {name} title cannot be blank"),
            serde_json::json!({
                "field": "title",
                "stage": name,
            }),
        ));
    }
    if spec.provider.trim().is_empty() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.provider.blank"),
            format!("dynamic {name} provider cannot be blank"),
            serde_json::json!({
                "field": "provider",
                "stage": name,
            }),
        ));
    } else if ctx.app.provider_for_id(&spec.provider).is_err() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.provider.unknown"),
            format!("dynamic {name} references unknown provider `{}`", spec.provider),
            serde_json::json!({
                "provider": spec.provider,
                "stage": name,
            }),
        ));
    } else if let Some(permission_mode) = ctx.dynamic.permission_mode() {
        if let Ok(doctor) = ctx.app.provider_doctor(&spec.provider) {
            let supported_mode_ids = doctor
                .supported_modes()
                .into_iter()
                .map(|mode| mode.id)
                .collect::<std::collections::HashSet<_>>();
            if !supported_mode_ids.is_empty() && !supported_mode_ids.contains(permission_mode) {
                errors.push(dynamic_validation_error(
                    &format!("dynamic.{name}.permission-mode.unsupported"),
                    format!("dynamic {name} permissionMode `{}` is not supported by provider `{}`", permission_mode, spec.provider),
                    serde_json::json!({
                        "provider": spec.provider,
                        "stage": name,
                        "permissionMode": permission_mode,
                    }),
                ));
            }
        }
    }
    if spec.task.trim().is_empty() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.task.blank"),
            format!("dynamic {name} task cannot be blank"),
            serde_json::json!({
                "field": "task",
                "stage": name,
            }),
        ));
    }
    errors.extend(validate_dynamic_profile_reference(
        ctx,
        &spec.profile,
        &format!("dynamic {name}"),
        serde_json::json!({
            "field": "profile",
            "stage": name,
            "profile": spec.profile,
        }),
    ));
    errors
}

fn validate_dynamic_profile_reference(
    ctx: &DynamicExecutionContext<'_>,
    profile: &str,
    owner: &str,
    params: serde_json::Value,
) -> Vec<DynamicProposalValidationError> {
    if profile.trim().is_empty() {
        return Vec::new();
    }
    if ctx.app.profile_show(profile).is_ok() {
        Vec::new()
    } else {
        vec![dynamic_validation_error(
            "dynamic.profile.unknown",
            format!("{owner} references unknown profile `{profile}`"),
            params,
        )]
    }
}

fn materialize_dynamic_next(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    source_index: usize,
    next: DynamicNext,
) -> Result<()> {
    match next {
        DynamicNext::End => {
            let source = graph.nodes[source_index].clone();
            if let Some(group_id) = source.group_id.as_deref() {
                if let Some(group) = graph.groups.iter_mut().find(|group| group.id == group_id) {
                    if !group.terminal_node_ids.iter().any(|id| id == &source.id) {
                        group.terminal_node_ids.push(source.id.clone());
                    }
                    group.updated_at = now_rfc3339_like();
                }
            }
        }
        DynamicNext::Single { node } => {
            let source = graph.nodes[source_index].clone();
            let new_node = dynamic_node_state_from_spec(
                graph,
                &source,
                node,
                source.group_id.clone(),
                source.chain_id.clone(),
                ctx.dynamic.permission_mode().map(ToOwned::to_owned),
            )?;
            append_dynamic_event(
                ctx,
                "dynamic_node_materialized",
                serde_json::json!({
                    "nodeId": new_node.id,
                    "sourceNodeId": source.id,
                    "kind": new_node.kind,
                }),
            )?;
            graph.nodes.push(new_node);
        }
        DynamicNext::Fanout {
            group_id,
            nodes,
            merge,
            acceptance,
        } => {
            let source = graph.nodes[source_index].clone();
            let group_depth = source
                .group_id
                .as_deref()
                .and_then(|group_id| graph.groups.iter().find(|group| group.id == group_id))
                .map(|group| group.depth + 1)
                .unwrap_or(1);
            let root_node_ids = nodes.iter().map(|node| node.id.clone()).collect::<Vec<_>>();
            let group = DynamicGroupState {
                version: VERSION.to_string(),
                id: group_id.clone(),
                dynamic_run_id: graph.run.id.clone(),
                status: DynamicGroupStatus::Open,
                depth: group_depth,
                parent_group_id: source.group_id.clone(),
                root_node_ids: root_node_ids.clone(),
                terminal_node_ids: Vec::new(),
                merge_node_id: None,
                acceptance_node_id: None,
                created_by_node_id: source.id.clone(),
                merge,
                acceptance,
                created_at: now_rfc3339_like(),
                updated_at: now_rfc3339_like(),
            };
            validate_dynamic_group_state(&group)?;
            graph.groups.push(group);
            for node in nodes {
                let chain_id = node.id.clone();
                let new_node = dynamic_node_state_from_spec(
                    graph,
                    &source,
                    node,
                    Some(group_id.clone()),
                    chain_id,
                    ctx.dynamic.permission_mode().map(ToOwned::to_owned),
                )?;
                append_dynamic_event(
                    ctx,
                    "dynamic_node_materialized",
                    serde_json::json!({
                        "nodeId": new_node.id,
                        "sourceNodeId": source.id,
                        "kind": new_node.kind,
                        "groupId": group_id,
                    }),
                )?;
                graph.nodes.push(new_node);
            }
            append_dynamic_event(
                ctx,
                "dynamic_group_created",
                serde_json::json!({
                    "groupId": group_id,
                    "rootNodeIds": root_node_ids,
                }),
            )?;
        }
    }
    refresh_dynamic_ready_nodes(graph);
    graph.run.updated_at = now_rfc3339_like();
    Ok(())
}

fn dynamic_node_state_from_spec(
    graph: &DynamicGraphState,
    source: &DynamicNodeState,
    spec: DynamicNodeSpec,
    group_id: Option<String>,
    chain_id: String,
    inherited_permission_mode: Option<String>,
) -> Result<DynamicNodeState> {
    let kind = match spec.kind {
        DynamicNodeSpecKind::Worker => DynamicNodeKind::Worker,
        DynamicNodeSpecKind::WorkflowInvocation => DynamicNodeKind::WorkflowInvocation,
    };
    let workflow_snapshot_id = spec.workflow_id.as_ref().and_then(|workflow_id| {
        graph
            .run
            .allowed_workflow_snapshots
            .iter()
            .find(|snapshot| snapshot.workflow_id == *workflow_id)
            .map(|snapshot| snapshot.snapshot_id.clone())
    });
    let node = DynamicNodeState {
        version: VERSION.to_string(),
        id: spec.id,
        dynamic_run_id: graph.run.id.clone(),
        kind,
        title: spec.title,
        task: spec.task,
        status: DynamicNodeStatus::Pending,
        outcome: None,
        group_id,
        chain_id,
        depth: source.depth + 1,
        depends_on: spec.depends_on,
        workspace: spec.workspace,
        workspace_path: None,
        provider: spec.provider,
        profile: spec.profile,
        permission_mode: inherited_permission_mode,
        session_mode: spec.session_mode,
        continue_from_node_id: spec.continue_from_node_id,
        workflow_id: spec.workflow_id,
        workflow_snapshot_id,
        child_run_id: None,
        started_at: None,
        finished_at: None,
    };
    validate_dynamic_node_state(&node)?;
    Ok(node)
}

fn refresh_dynamic_ready_nodes(graph: &mut DynamicGraphState) {
    let completed_success = graph
        .nodes
        .iter()
        .filter(|node| {
            node.status == DynamicNodeStatus::Completed
                && node.outcome == Some(NodeOutcome::Success)
        })
        .map(|node| node.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let occupied_slots = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.status,
                DynamicNodeStatus::Ready | DynamicNodeStatus::Running
            )
        })
        .count();
    let mut available_slots =
        (graph.run.control.max_parallel as usize).saturating_sub(occupied_slots);
    for index in 0..graph.nodes.len() {
        if available_slots == 0 {
            break;
        }
        if graph.nodes[index].status != DynamicNodeStatus::Pending {
            continue;
        }
        if graph.nodes[index]
            .depends_on
            .iter()
            .all(|dependency| completed_success.contains(dependency))
        {
            graph.nodes[index].status = DynamicNodeStatus::Ready;
            available_slots -= 1;
        }
    }
    graph.run.current_node_ids = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.status,
                DynamicNodeStatus::Ready | DynamicNodeStatus::Running
            )
        })
        .map(|node| node.id.clone())
        .collect();
}

fn advance_dynamic_groups(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> Result<bool> {
    let mut changed = false;
    for group_index in 0..graph.groups.len() {
        let status = graph.groups[group_index].status;
        match status {
            DynamicGroupStatus::Open if dynamic_group_ready(graph, group_index) => {
                let merge_node = create_dynamic_merge_node(graph, group_index)?;
                let group_id = graph.groups[group_index].id.clone();
                graph.groups[group_index].status = DynamicGroupStatus::Merging;
                graph.groups[group_index].merge_node_id = Some(merge_node.id.clone());
                graph.groups[group_index].updated_at = now_rfc3339_like();
                graph.nodes.push(merge_node);
                append_dynamic_event(
                    ctx,
                    "dynamic_group_merge_started",
                    serde_json::json!({
                        "groupId": group_id,
                    }),
                )?;
                changed = true;
            }
            DynamicGroupStatus::Merging
                if group_node_completed(
                    graph,
                    graph.groups[group_index].merge_node_id.as_deref(),
                ) =>
            {
                let acceptance_node = create_dynamic_acceptance_node(graph, group_index)?;
                let group_id = graph.groups[group_index].id.clone();
                graph.groups[group_index].status = DynamicGroupStatus::Accepting;
                graph.groups[group_index].acceptance_node_id = Some(acceptance_node.id.clone());
                graph.groups[group_index].updated_at = now_rfc3339_like();
                graph.nodes.push(acceptance_node);
                append_dynamic_event(
                    ctx,
                    "dynamic_group_acceptance_started",
                    serde_json::json!({
                        "groupId": group_id,
                    }),
                )?;
                changed = true;
            }
            DynamicGroupStatus::Accepting
                if group_node_completed(
                    graph,
                    graph.groups[group_index].acceptance_node_id.as_deref(),
                ) =>
            {
                let group_id = graph.groups[group_index].id.clone();
                graph.groups[group_index].status = DynamicGroupStatus::Closed;
                graph.groups[group_index].updated_at = now_rfc3339_like();
                attach_closed_child_group_to_parent(graph, group_index);
                append_dynamic_event(
                    ctx,
                    "dynamic_group_closed",
                    serde_json::json!({
                        "groupId": group_id,
                    }),
                )?;
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        refresh_dynamic_ready_nodes(graph);
        graph.run.updated_at = now_rfc3339_like();
        persist_dynamic_graph(ctx, graph)?;
    }
    Ok(changed)
}

fn dynamic_group_ready(graph: &DynamicGraphState, group_index: usize) -> bool {
    let Some(group) = graph.groups.get(group_index) else {
        return false;
    };
    let group_nodes = graph
        .nodes
        .iter()
        .filter(|node| node.group_id.as_deref() == Some(group.id.as_str()))
        .filter(|node| {
            matches!(
                node.kind,
                DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation
            )
        })
        .collect::<Vec<_>>();
    let child_groups = graph
        .groups
        .iter()
        .filter(|child| child.parent_group_id.as_deref() == Some(group.id.as_str()))
        .collect::<Vec<_>>();
    !group_nodes.is_empty()
        && group_nodes.iter().all(|node| {
            node.status == DynamicNodeStatus::Completed
                && node.outcome == Some(NodeOutcome::Success)
        })
        && group_nodes
            .iter()
            .all(|node| accepted_completion_exists(graph, &node.id))
        && child_groups.iter().all(|child| {
            child.status == DynamicGroupStatus::Closed && child.acceptance_node_id.is_some()
        })
        && group
            .terminal_node_ids
            .iter()
            .all(|node_id| terminal_belongs_to_group_boundary(graph, group, node_id))
        && group
            .root_node_ids
            .iter()
            .all(|root_id| root_chain_has_terminal_boundary(graph, group, root_id))
}

fn accepted_completion_exists(graph: &DynamicGraphState, source_node_id: &str) -> bool {
    graph.proposals.iter().any(|proposal| {
        proposal.source_node_id == source_node_id
            && proposal.validation_status == DynamicProposalValidationStatus::Accepted
    })
}

fn attach_closed_child_group_to_parent(graph: &mut DynamicGraphState, group_index: usize) {
    let Some(child) = graph.groups.get(group_index) else {
        return;
    };
    let Some(parent_group_id) = child.parent_group_id.clone() else {
        return;
    };
    let Some(acceptance_node_id) = child.acceptance_node_id.clone() else {
        return;
    };
    let Some(parent) = graph
        .groups
        .iter_mut()
        .find(|group| group.id == parent_group_id)
    else {
        return;
    };
    if !parent
        .terminal_node_ids
        .iter()
        .any(|node_id| node_id == &acceptance_node_id)
    {
        parent.terminal_node_ids.push(acceptance_node_id);
        parent.updated_at = now_rfc3339_like();
    }
}

fn terminal_belongs_to_group_boundary(
    graph: &DynamicGraphState,
    group: &DynamicGroupState,
    node_id: &str,
) -> bool {
    if graph.nodes.iter().any(|node| {
        node.id == node_id
            && node.group_id.as_deref() == Some(group.id.as_str())
            && matches!(
                node.kind,
                DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation
            )
    }) {
        return true;
    }
    graph.groups.iter().any(|child| {
        child.parent_group_id.as_deref() == Some(group.id.as_str())
            && child.status == DynamicGroupStatus::Closed
            && child.acceptance_node_id.as_deref() == Some(node_id)
    })
}

fn root_chain_has_terminal_boundary(
    graph: &DynamicGraphState,
    group: &DynamicGroupState,
    root_id: &str,
) -> bool {
    let Some(root_chain_id) = graph
        .nodes
        .iter()
        .find(|node| node.id == root_id && node.group_id.as_deref() == Some(group.id.as_str()))
        .map(|node| node.chain_id.as_str())
    else {
        return false;
    };
    group.terminal_node_ids.iter().any(|terminal_id| {
        terminal_chain_id(graph, group, terminal_id).as_deref() == Some(root_chain_id)
    })
}

fn terminal_chain_id(
    graph: &DynamicGraphState,
    group: &DynamicGroupState,
    terminal_id: &str,
) -> Option<String> {
    if let Some(node) = graph
        .nodes
        .iter()
        .find(|node| node.id == terminal_id && node.group_id.as_deref() == Some(group.id.as_str()))
    {
        return Some(node.chain_id.clone());
    }
    let child = graph.groups.iter().find(|child| {
        child.parent_group_id.as_deref() == Some(group.id.as_str())
            && child.acceptance_node_id.as_deref() == Some(terminal_id)
    })?;
    graph
        .nodes
        .iter()
        .find(|node| node.id == child.created_by_node_id)
        .map(|node| node.chain_id.clone())
}

fn group_node_completed(graph: &DynamicGraphState, node_id: Option<&str>) -> bool {
    let Some(node_id) = node_id else {
        return false;
    };
    graph.nodes.iter().any(|node| {
        node.id == node_id
            && node.status == DynamicNodeStatus::Completed
            && node.outcome == Some(NodeOutcome::Success)
    })
}

fn create_dynamic_merge_node(
    graph: &DynamicGraphState,
    group_index: usize,
) -> Result<DynamicNodeState> {
    let group = graph
        .groups
        .get(group_index)
        .ok_or_else(|| anyhow!("dynamic group missing"))?;
    let id = format!("{}-merge", group.id);
    ensure!(
        !graph.nodes.iter().any(|node| node.id == id),
        "dynamic merge node `{id}` already exists"
    );
    let task = format!(
        "{}\n\nGroup: {}\nTerminal nodes: {}\nMain workspace: {}\nBranch workspaces:\n{}",
        group.merge.task,
        group.id,
        group.terminal_node_ids.join(", "),
        graph.run.parent_run_id,
        dynamic_group_workspace_summary(graph, group),
    );
    let node = DynamicNodeState {
        version: VERSION.to_string(),
        id,
        dynamic_run_id: graph.run.id.clone(),
        kind: DynamicNodeKind::Merge,
        title: group.merge.title.clone(),
        task,
        status: DynamicNodeStatus::Ready,
        outcome: None,
        group_id: Some(group.id.clone()),
        chain_id: format!("{}-merge", group.id),
        depth: group.depth,
        depends_on: group.terminal_node_ids.clone(),
        workspace: WorkspacePolicy {
            mode: WorkspaceMode::Main,
        },
        workspace_path: None,
        provider: Some(group.merge.provider.clone()),
        profile: None,
        permission_mode: None,
        session_mode: SessionMode::New,
        continue_from_node_id: None,
        workflow_id: None,
        workflow_snapshot_id: None,
        child_run_id: None,
        started_at: None,
        finished_at: None,
    };
    validate_dynamic_node_state(&node)?;
    Ok(node)
}

fn create_dynamic_acceptance_node(
    graph: &DynamicGraphState,
    group_index: usize,
) -> Result<DynamicNodeState> {
    let group = graph
        .groups
        .get(group_index)
        .ok_or_else(|| anyhow!("dynamic group missing"))?;
    let merge_node_id = group
        .merge_node_id
        .as_ref()
        .ok_or_else(|| anyhow!("dynamic group `{}` has no merge node", group.id))?;
    let id = format!("{}-accept", group.id);
    ensure!(
        !graph.nodes.iter().any(|node| node.id == id),
        "dynamic acceptance node `{id}` already exists"
    );
    let task = format!(
        "{}\n\nGroup: {}\nMerge node: {}\nRoot nodes: {}\nTerminal nodes: {}",
        group.acceptance.task,
        group.id,
        merge_node_id,
        group.root_node_ids.join(", "),
        group.terminal_node_ids.join(", "),
    );
    let node = DynamicNodeState {
        version: VERSION.to_string(),
        id,
        dynamic_run_id: graph.run.id.clone(),
        kind: DynamicNodeKind::Acceptance,
        title: group.acceptance.title.clone(),
        task,
        status: DynamicNodeStatus::Ready,
        outcome: None,
        group_id: Some(group.id.clone()),
        chain_id: format!("{}-accept", group.id),
        depth: group.depth,
        depends_on: vec![merge_node_id.clone()],
        workspace: WorkspacePolicy {
            mode: WorkspaceMode::Main,
        },
        workspace_path: None,
        provider: Some(group.acceptance.provider.clone()),
        profile: None,
        permission_mode: None,
        session_mode: SessionMode::New,
        continue_from_node_id: None,
        workflow_id: None,
        workflow_snapshot_id: None,
        child_run_id: None,
        started_at: None,
        finished_at: None,
    };
    validate_dynamic_node_state(&node)?;
    Ok(node)
}

fn dynamic_group_workspace_summary(graph: &DynamicGraphState, group: &DynamicGroupState) -> String {
    graph
        .nodes
        .iter()
        .filter(|node| node.group_id.as_deref() == Some(group.id.as_str()))
        .filter_map(|node| {
            node.workspace_path
                .as_ref()
                .map(|path| format!("- {}: {}", node.id, path))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dynamic_graph_completed(graph: &DynamicGraphState) -> bool {
    graph.run.status == DynamicRunStatus::Running
        && graph
            .groups
            .iter()
            .all(|group| group.status == DynamicGroupStatus::Closed)
        && graph.nodes.iter().all(|node| {
            node.status == DynamicNodeStatus::Completed
                && node.outcome == Some(NodeOutcome::Success)
        })
        && graph
            .nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.kind,
                    DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation
                )
            })
            .all(|node| accepted_completion_exists(graph, &node.id))
}

pub(crate) fn build_dynamic_prompt_bundle(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    dynamic_attempt_id: &str,
    prompt: String,
    prompt_id: Option<String>,
    continue_ref: Option<serde_json::Value>,
) -> Result<PromptBundle> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    let dynamic = match validated.get_node(outer_node_id) {
        Some(NodeDsl::AiDynamic(dynamic)) => dynamic,
        _ => return Err(anyhow!("node `{outer_node_id}` is not an ai-dynamic node")),
    };
    let round: RoundState = read_json(&app.paths.round_file(task_id, run_id, round_id))?;
    validate_round_state(&round)?;
    let graph: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    ))?;
    let node: DynamicNodeState = read_json(&app.paths.dynamic_node_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        dynamic_node_id,
    ))?;
    let ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        dynamic,
    };
    let output_contract = match node.kind {
        DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation => {
            Some(dynamic_output_contract(app.config.desktop_language, dynamic))
        }
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => None,
    };
    let invocation = build_dynamic_worker_invocation(
        &ctx,
        &graph,
        &node,
        dynamic_attempt_id,
        output_contract,
        if continue_ref.is_some() {
            SessionMode::Continue
        } else {
            SessionMode::New
        },
        continue_ref,
        Some(prompt),
        prompt_id,
        PromptVisibility::Visible,
    )?;
    render_prompt_bundle(&invocation)
}

fn build_dynamic_worker_invocation(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
    attempt_id: &str,
    output_contract: Option<PromptOutputContract>,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
    resume_prompt: Option<String>,
    resume_prompt_id: Option<String>,
    resume_prompt_visibility: PromptVisibility,
) -> Result<WorkerInvocation> {
    let runtime_context = dynamic_runtime_context(ctx, &node.id, attempt_id);
    let builtin_profile = dynamic_builtin_profile(ctx.app.config.desktop_language, node);
    let profile = builtin_profile
        .map(|(profile, _)| profile.to_string())
        .or_else(|| node.profile.clone());
    let profile_content = match builtin_profile {
        Some((_, content)) => Some(content.trim().to_string()),
        None => node
            .profile
            .as_deref()
            .and_then(|profile| ctx.app.profile_show(profile).ok().map(|entry| entry.content)),
    };
    let workspace_dir = node
        .workspace_path
        .clone()
        .unwrap_or_else(|| ctx.app.paths.repo_root.clone());
    let extra_system_sections = dynamic_system_sections(ctx, graph, node)?;
    Ok(WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        profile,
        profile_content,
        requirement_path: None,
        requirement_text: Some(dynamic_requirement_text(ctx)?),
        workspace_dir,
        attempt_dir: runtime_context.attempt_dir.clone(),
        output_contract,
        runtime_context,
        predecessors: dynamic_predecessor_contexts(ctx, graph, node),
        extra_system_sections,
        task_instruction: Some(dynamic_task_instruction(ctx, graph, node)),
        session_mode,
        permission_mode: node
            .permission_mode
            .clone()
            .or_else(|| ctx.dynamic.permission_mode().map(ToOwned::to_owned)),
        continue_ref,
        resume_prompt,
        resume_prompt_id,
        resume_prompt_visibility,
        stream_mode: StreamMode::StreamJson,
        log_prompts: ctx.app.config.log_prompts,
        log_provider_command: ctx.app.config.log_provider_command,
        attachments_dir: Some(ctx.app.paths.dynamic_node_attachments_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &node.id,
            attempt_id,
        )),
        cold_artifacts: Vec::new(),
        cold_attachments: Vec::new(),
    })
}

fn dynamic_builtin_profile(
    language: DesktopLanguage,
    node: &DynamicNodeState,
) -> Option<(&'static str, &'static str)> {
    match node.kind {
        DynamicNodeKind::Worker if node.depth == 0 && node.chain_id == "bootstrap" => Some((
            "ai-dynamic-fanout",
            prompt_by_language(language, AI_DYNAMIC_FANOUT_ZH_CN, AI_DYNAMIC_FANOUT_EN),
        )),
        DynamicNodeKind::Merge => Some((
            "ai-dynamic-merge",
            prompt_by_language(language, AI_DYNAMIC_MERGE_ZH_CN, AI_DYNAMIC_MERGE_EN),
        )),
        DynamicNodeKind::Acceptance => Some((
            "ai-dynamic-acceptance",
            prompt_by_language(
                language,
                AI_DYNAMIC_ACCEPTANCE_ZH_CN,
                AI_DYNAMIC_ACCEPTANCE_EN,
            ),
        )),
        _ => None,
    }
}

fn dynamic_requirement_text(ctx: &DynamicExecutionContext<'_>) -> Result<String> {
    Ok(std::fs::read_to_string(
        ctx.app.paths.requirement_file(ctx.task_id).as_std_path(),
    )
    .unwrap_or_default())
}

fn dynamic_proposal_repair_prompt(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
    errors: &[DynamicProposalValidationError],
) -> String {
    render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN,
            AI_DYNAMIC_PROPOSAL_REPAIR_EN,
        ),
        serde_json::json!({
            "validation_errors": dynamic_validation_error_lines(errors),
            "remaining_budget": dynamic_remaining_budget_summary(graph, node),
        }),
    )
    .expect("prompt template renders")
}

fn dynamic_text_repair_prompt(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
    error: String,
) -> String {
    let validation_errors = error
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN,
            AI_DYNAMIC_PROPOSAL_REPAIR_EN,
        ),
        serde_json::json!({
            "validation_errors": validation_errors,
            "remaining_budget": dynamic_remaining_budget_summary(graph, node),
        }),
    )
    .expect("prompt template renders")
}

fn dynamic_task_instruction(ctx: &DynamicExecutionContext<'_>, _graph: &DynamicGraphState, node: &DynamicNodeState) -> String {
    let metadata = render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_NODE_TASK_ZH_CN,
            AI_DYNAMIC_NODE_TASK_EN,
        ),
        serde_json::json!({
            "title": node.title,
        }),
    )
    .expect("prompt template renders");
    let task = if let Some(global_goal) = ctx.dynamic.global_goal() {
        if global_goal.trim().is_empty() {
            node.task.trim().to_string()
        } else if node.task.trim().is_empty() {
            global_goal.trim().to_string()
        } else {
            format!("{}\n\n---\n\n{}", global_goal.trim(), node.task.trim())
        }
    } else {
        node.task.trim().to_string()
    };
    format!("{}\n\n{}", task, metadata.trim())
}

fn dynamic_predecessor_contexts(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> Vec<crate::provider::PromptPredecessorContext> {
    node.depends_on
        .iter()
        .filter_map(|dependency| graph.nodes.iter().find(|item| item.id == *dependency))
        .map(|dependency| crate::provider::PromptPredecessorContext {
            round_id: ctx.round_id.to_string(),
            node_id: dependency.id.clone(),
            attempt_id: dynamic_attempt_id(dependency),
            node_type: format!("{:?}", dependency.kind).to_ascii_lowercase(),
            branch_kind: "AI-DYNAMIC dependency".to_string(),
            outcome: dependency
                .outcome
                .map(|outcome| format!("{:?}", outcome).to_ascii_lowercase()),
            branch_direction: Some("dependency".to_string()),
            output_artifact: Some(crate::provider::PromptArtifactRef {
                name: DYNAMIC_COMPLETION_ARTIFACT.to_string(),
                path: ctx.app.paths.dynamic_node_artifact_file(
                    ctx.task_id,
                    ctx.run_id,
                    ctx.round_id,
                    ctx.outer_node_id,
                    ctx.outer_attempt_id,
                    &dependency.id,
                    &dynamic_attempt_id(dependency),
                    DYNAMIC_COMPLETION_ARTIFACT,
                ),
                preview: None,
            }),
            branch_reason: dependency.finished_at.clone(),
        })
        .collect()
}

fn allowed_workflow_snapshot_summary(snapshots: &[AllowedWorkflowSnapshot]) -> String {
    if snapshots.is_empty() {
        return "none".to_string();
    }
    snapshots
        .iter()
        .map(|snapshot| {
            format!(
                "- workflowId={} name={} containsAiDynamic={}",
                snapshot.workflow_id,
                snapshot.name,
                snapshot.contains_ai_dynamic,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn available_provider_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    let mut providers = ctx
        .app
        .managed_agents()
        .keys()
        .map(|agent_type| agent_type.as_str().to_string())
        .collect::<Vec<_>>();
    if let Some(provider) = ctx.dynamic.bootstrap_provider() {
        providers.push(provider.to_string());
    }
    providers.sort();
    providers.dedup();
    if providers.is_empty() {
        return "none".to_string();
    }
    providers
        .into_iter()
        .map(|provider| format!("- {provider}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn available_profile_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    match ctx.app.profiles() {
        Ok(list) => {
            let allowed = ctx
                .dynamic
                .allowed_profiles
                .iter()
                .map(|profile| profile.as_str())
                .collect::<std::collections::HashSet<_>>();
            let profiles = list
                .profiles
                .into_iter()
                .filter(|profile| allowed.is_empty() || allowed.contains(profile.id.as_str()))
                .collect::<Vec<_>>();
            if profiles.is_empty() {
                return "none".to_string();
            }
            profiles
                .into_iter()
                .map(|profile| format!("- {} ({})", profile.name, profile.id))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(_) => "none".to_string(),
    }
}

fn dynamic_remaining_budget_summary(graph: &DynamicGraphState, node: &DynamicNodeState) -> String {
    let current_workflow_invocations = graph
        .nodes
        .iter()
        .filter(|candidate| candidate.kind == DynamicNodeKind::WorkflowInvocation)
        .count() as u32;
    let parent_group_depth = node
        .group_id
        .as_deref()
        .and_then(|group_id| graph.groups.iter().find(|group| group.id == group_id))
        .map(|group| group.depth)
        .unwrap_or(0);
    let next_group_depth = parent_group_depth + 1;
    let running_count = graph
        .nodes
        .iter()
        .filter(|candidate| candidate.status == DynamicNodeStatus::Running)
        .count() as u32;
    format!(
        "- remaining dynamic nodes: {}\n- max fanout nodes in one proposal: {}\n- remaining workflow invocations: {}\n- current group depth: {}\n- remaining nested group depth headroom: {}\n- available parallel slots right now: {}\n- nested AI-DYNAMIC allowed: {}",
        graph
            .run
            .control
            .max_dynamic_nodes
            .saturating_sub(graph.nodes.len() as u32),
        graph.run.control.max_fanout,
        graph
            .run
            .control
            .max_workflow_invocations
            .saturating_sub(current_workflow_invocations),
        parent_group_depth,
        graph
            .run
            .control
            .max_group_depth
            .saturating_sub(next_group_depth.saturating_sub(1)),
        graph.run.control.max_parallel.saturating_sub(running_count),
        graph.run.control.allow_nested_dynamic,
    )
}

fn dynamic_graph_summary(graph: &DynamicGraphState) -> String {
    let current = if graph.run.current_node_ids.is_empty() {
        "none".to_string()
    } else {
        graph.run.current_node_ids.join(", ")
    };
    let completed = graph
        .nodes
        .iter()
        .filter(|node| node.status == DynamicNodeStatus::Completed)
        .map(|node| format!("{}({:?})", node.id, node.outcome))
        .collect::<Vec<_>>();
    let completed = if completed.is_empty() {
        "none".to_string()
    } else {
        completed.join(", ")
    };
    format!(
        "- current internal nodes: {}\n- total nodes: {}\n- groups: {}\n- completed nodes: {}",
        current,
        graph.nodes.len(),
        graph.groups.len(),
        completed,
    )
}

fn dynamic_resumable_session_nodes<'a>(
    graph: &'a DynamicGraphState,
    source: &DynamicNodeState,
) -> Vec<&'a DynamicNodeState> {
    let boundary_group_id = source.group_id.clone();
    graph
        .nodes
        .iter()
        .filter(|candidate| candidate.kind == DynamicNodeKind::Worker)
        .filter(|candidate| candidate.chain_id == source.chain_id)
        .filter(|candidate| candidate.group_id == boundary_group_id)
        .filter(|candidate| {
            candidate.id == source.id
                || (candidate.status == DynamicNodeStatus::Completed
                    && candidate.outcome == Some(NodeOutcome::Success))
        })
        .collect()
}

fn dynamic_resumable_session_summary(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    source: &DynamicNodeState,
) -> String {
    let lines = dynamic_resumable_session_nodes(graph, source)
        .into_iter()
        .filter_map(|candidate| {
            let continue_ref = dynamic_node_continue_ref(ctx, candidate, &dynamic_attempt_id(candidate))?;
            let _ = continue_ref;
            Some(format!(
                "- nodeId={} title={} goal={}",
                candidate.id,
                candidate.title,
                candidate.task.replace('\n', " ").trim()
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        "- none".to_string()
    } else {
        lines.join("\n")
    }
}

fn dynamic_upstream_refs_summary(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> String {
    if node.depends_on.is_empty() {
        return "- none".to_string();
    }
    node.depends_on
        .iter()
        .filter_map(|dependency_id| graph.nodes.iter().find(|item| item.id == *dependency_id))
        .map(|dependency| {
            let attempt_id = dynamic_attempt_id(dependency);
            let artifact_path = ctx.app.paths.dynamic_node_artifact_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &dependency.id,
                &attempt_id,
                DYNAMIC_COMPLETION_ARTIFACT,
            );
            let attachments_dir = ctx.app.paths.dynamic_node_attachments_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &dependency.id,
                &attempt_id,
            );
            format!(
                "- {}: completion={} attachments={}",
                dependency.id, artifact_path, attachments_dir
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dynamic_kind_specific_summary(
    _ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> String {
    match node.kind {
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => {
            let Some(group_id) = node.group_id.as_deref() else {
                return "- group summary unavailable".to_string();
            };
            let Some(group) = graph.groups.iter().find(|group| group.id == group_id) else {
                return "- group summary unavailable".to_string();
            };
            let child_runs = graph
                .nodes
                .iter()
                .filter(|candidate| candidate.group_id.as_deref() == Some(group_id))
                .filter_map(|candidate| {
                    candidate.child_run_id.as_ref().map(|child_run_id| {
                        format!("{}={}", candidate.id, child_run_id)
                    })
                })
                .collect::<Vec<_>>();
            let child_runs = if child_runs.is_empty() {
                "none".to_string()
            } else {
                child_runs.join(", ")
            };
            format!(
                "- group: {}\n- root nodes: {}\n- terminal nodes: {}\n- branch workspaces:\n{}\n- child runs: {}",
                group.id,
                if group.root_node_ids.is_empty() {
                    "none".to_string()
                } else {
                    group.root_node_ids.join(", ")
                },
                if group.terminal_node_ids.is_empty() {
                    "none".to_string()
                } else {
                    group.terminal_node_ids.join(", ")
                },
                dynamic_group_workspace_summary(graph, group),
                child_runs,
            )
        }
        _ => "- none".to_string(),
    }
}

fn dynamic_system_sections(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> Result<Vec<String>> {
    let workspace_path = node
        .workspace_path
        .as_ref()
        .map(|path| path.to_string())
        .unwrap_or_else(|| ctx.app.paths.repo_root.to_string());
    let dynamic_root = ctx.app.paths.dynamic_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    let runtime_context = dynamic_runtime_context(ctx, &node.id, &dynamic_attempt_id(node));
    Ok(vec![render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_SYSTEM_ZH_CN,
            AI_DYNAMIC_SYSTEM_EN,
        ),
        serde_json::json!({
            "outer_node_id": ctx.outer_node_id,
            "outer_attempt_id": ctx.outer_attempt_id,
            "dynamic_run_id": graph.run.id,
            "node_id": node.id,
            "kind": format!("{:?}", node.kind),
            "group_id": node.group_id.as_deref().unwrap_or("none"),
            "chain_id": node.chain_id,
            "depth": node.depth,
            "dynamic_root": dynamic_root,
            "node_dir": runtime_context.node_dir,
            "attempt_dir": runtime_context.attempt_dir,
            "attachments_dir": runtime_context.attachments_dir,
            "workspace_mode": format!("{:?}", node.workspace.mode),
            "workspace_path": workspace_path,
            "upstream_refs": dynamic_upstream_refs_summary(ctx, graph, node),
            "allowed_workflow_snapshots": allowed_workflow_snapshot_summary(&graph.run.allowed_workflow_snapshots),
            "agent_strategy_mode": dynamic_agent_strategy_mode(ctx.dynamic),
            "bootstrap_provider": ctx.dynamic.bootstrap_provider().unwrap_or("none"),
            "agent_routing_prompt": dynamic_agent_routing_prompt(ctx.dynamic).unwrap_or("none"),
            "available_providers": available_provider_summary(ctx),
            "available_profiles": available_profile_summary(ctx),
            "remaining_budget": dynamic_remaining_budget_summary(graph, node),
            "graph_summary": dynamic_graph_summary(graph),
            "resumable_sessions": dynamic_resumable_session_summary(ctx, graph, node),
            "depends_on": if node.depends_on.is_empty() {
                "none".to_string()
            } else {
                node.depends_on.join(", ")
            },
            "kind_specific_context": dynamic_kind_specific_summary(ctx, graph, node),
        }),
    )?])
}

fn prepare_dynamic_attempt_dirs(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    attempt_id: &str,
) -> Result<()> {
    std::fs::create_dir_all(
        ctx.app
            .paths
            .dynamic_node_attempt_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &node.id,
                attempt_id,
            )
            .as_std_path(),
    )?;
    std::fs::create_dir_all(
        ctx.app
            .paths
            .dynamic_node_artifacts_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &node.id,
                attempt_id,
            )
            .as_std_path(),
    )?;
    std::fs::create_dir_all(
        ctx.app
            .paths
            .dynamic_node_attachments_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &node.id,
                attempt_id,
            )
            .as_std_path(),
    )?;
    Ok(())
}

fn ensure_dynamic_workspace(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
) -> Result<()> {
    match node.workspace.mode {
        WorkspaceMode::Readonly | WorkspaceMode::Main => {
            node.workspace_path = Some(ctx.app.paths.repo_root.clone());
            Ok(())
        }
        WorkspaceMode::Worktree => {
            if node.workspace_path.is_some() {
                return Ok(());
            }
            let worktree_dir = ctx
                .app
                .paths
                .dynamic_dir(
                    ctx.task_id,
                    ctx.run_id,
                    ctx.round_id,
                    ctx.outer_node_id,
                    ctx.outer_attempt_id,
                )
                .join("worktrees")
                .join(safe_dynamic_ref(&node.id));
            if !worktree_dir.exists() {
                std::fs::create_dir_all(
                    worktree_dir
                        .parent()
                        .ok_or_else(|| anyhow!("dynamic worktree path has no parent"))?
                        .as_std_path(),
                )?;
                let branch = format!(
                    "gb-dynamic-{}-{}-{}",
                    safe_dynamic_ref(ctx.run_id),
                    safe_dynamic_ref(ctx.outer_node_id),
                    safe_dynamic_ref(&node.id)
                );
                let status = std::process::Command::new("git")
                    .arg("-C")
                    .arg(ctx.app.paths.repo_root.as_str())
                    .arg("worktree")
                    .arg("add")
                    .arg("-b")
                    .arg(branch)
                    .arg(worktree_dir.as_str())
                    .arg("HEAD")
                    .status()?;
                ensure!(
                    status.success(),
                    "failed to create dynamic worktree for `{}`",
                    node.id
                );
            }
            node.workspace_path = Some(worktree_dir);
            Ok(())
        }
    }
}

fn persist_dynamic_graph(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) -> Result<()> {
    validate_dynamic_run_state(&graph.run)?;
    for node in &graph.nodes {
        validate_dynamic_node_state(node)?;
    }
    for group in &graph.groups {
        validate_dynamic_group_state(group)?;
    }
    write_json(
        &ctx.app.paths.dynamic_run_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        &graph.run,
    )?;
    write_json(
        &ctx.app.paths.dynamic_allowed_workflow_snapshots_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        &graph.run.allowed_workflow_snapshots,
    )?;
    write_json(
        &ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        graph,
    )?;
    for node in &graph.nodes {
        write_json(
            &ctx.app.paths.dynamic_node_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &node.id,
            ),
            node,
        )?;
    }
    for group in &graph.groups {
        write_json(
            &ctx.app.paths.dynamic_group_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &group.id,
            ),
            group,
        )?;
    }
    for proposal in &graph.proposals {
        let path = ctx
            .app
            .paths
            .dynamic_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
            )
            .join("proposals")
            .join(format!("{}.json", proposal.id));
        write_json(&path, proposal)?;
    }
    Ok(())
}

fn pause_dynamic_graph(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    pause_reason: PauseReason,
    reason: &str,
) -> Result<()> {
    graph.run.status = DynamicRunStatus::Paused;
    graph.run.outcome = None;
    graph.run.pause_reason = Some(pause_reason);
    graph.run.updated_at = now_rfc3339_like();
    append_dynamic_event(
        ctx,
        "dynamic_run_paused",
        serde_json::json!({
            "dynamicRunId": graph.run.id,
            "pauseReason": pause_reason,
            "reason": reason,
        }),
    )?;
    persist_dynamic_graph(ctx, graph)
}

fn append_dynamic_event(
    ctx: &DynamicExecutionContext<'_>,
    event_type: &str,
    data: serde_json::Value,
) -> Result<()> {
    append_jsonl(
        &ctx.app.paths.dynamic_events_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        &serde_json::json!({
            "timestamp": now_rfc3339_like(),
            "type": event_type,
            "data": data,
        }),
    )
}

fn safe_dynamic_ref(value: &str) -> String {
    let mut out = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            out.push(character);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
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
    let mut resume_prompt_visibility = PromptVisibility::Visible;
    let mut invalid_output_repair_prompts = 0;

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

        // ── Metrics: notify node started (predecessor + current) ──
        if let Some(metrics_cb) = &app.metrics_callback {
            let seq = round.trace.iter()
                .filter(|t| t.node_id == node.node_id)
                .map(|t| t.sequence)
                .last();
            let node_name = node.resolved_config.get("profileName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
                .map(|s| s.to_string());
            let agent_type = node.resolved_config.get("provider")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let metrics_ctx = super::MetricsEventContext {
                repo_root: app.paths.repo_root.to_string(),
                task_id: task_id.to_string(),
                run_id: run.id.clone(),
                round_id: round.id.clone(),
                node_id: node.node_id.clone(),
                attempt_id: node.attempt_id.clone(),
                task_uuid: run.task_uuid.clone(),
                run_uuid: run.uuid.clone(),
                round_uuid: round.uuid.clone(),
                node_uuid: node.uuid.clone(),
                seq,
                node_name,
                agent_type,
                started_at: node.started_at.clone(),
                finished_at: node.finished_at.clone(),
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                total_tokens: 0,
            };
            metrics_cb(metrics_ctx, super::MetricsEvent::NodeStarted {
                predecessor: run.last_executed_node.clone(),
            });
        }

        let current_node_dsl = workflow
            .get_node(&current_node_id)
            .expect("validated node exists");
        if matches!(current_node_dsl, NodeDsl::Worker(_)) {
            setup_node_environment(app, task_id, &run.id, &round.id, &node, &ctx)?;
        }
        let execution_result = match current_node_dsl {
            NodeDsl::Worker(_) => execute_ai_node(
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
                resume_prompt_visibility,
            ),
            NodeDsl::AiDynamic(dynamic) => execute_ai_dynamic_node(
                app,
                task_id,
                &run.id,
                round,
                &current_attempt_id,
                dynamic,
                node.clone(),
            ),
        };
        node = match execution_result {
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

        if node.status == RunStatus::Paused {
            let pause_reason = if node.node_type == crate::domain::NodeType::AiDynamic {
                let graph: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
                    task_id,
                    &run.id,
                    &round.id,
                    &node.node_id,
                    &node.attempt_id,
                ))?;
                graph.run.pause_reason.unwrap_or(PauseReason::ProcessInterrupted)
            } else {
                PauseReason::ProcessInterrupted
            };
            run.status = RunStatus::Paused;
            run.pause_reason = Some(pause_reason);
            run.updated_at = now_rfc3339_like();
            round.status = RunStatus::Paused;
            let summary = format!(
                "run {} paused at {}/{}/{}",
                run.id, round.id, node.node_id, node.attempt_id
            );
            progress(&summary);
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                run,
                Some(node.node_type),
                if pause_reason == PauseReason::ErrorBlocked {
                    ProgressStage::Blocked
                } else {
                    ProgressStage::Paused
                },
                summary.clone(),
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
                    Some(if pause_reason == PauseReason::ErrorBlocked {
                        ProgressStage::Blocked
                    } else {
                        ProgressStage::Paused
                    }),
                    Some(run.status),
                    Some(summary),
                    run.pause_reason,
                ),
            );
            persist_runtime_state(app, task_id, run, round, &node)?;
            return Ok(());
        }

        if node.status == RunStatus::Completed && node.outcome == Some(NodeOutcome::Invalid) {
            if let Some(schema) = output_schema_for_node(workflow, &node.node_id) {
                if invalid_output_repair_prompts >= MAX_INVALID_OUTPUT_REPAIR_PROMPTS {
                    append_run_event_best_effort(
                        &app.paths,
                        task_id,
                        &run.id,
                        "invalid_output_repair_exhausted",
                        now_rfc3339_like(),
                        run_event_data(
                            &ctx,
                            Some(ProgressStage::Completed),
                            Some(node.status),
                            Some(format!(
                                "invalid output repair exhausted at {}/{}/{}",
                                round.id, node.node_id, node.attempt_id
                            )),
                            None,
                        ),
                    );
                    apply_control_decision(
                        app,
                        task_id,
                        workflow,
                        resolved_profiles,
                        run,
                        round,
                        &node,
                        ControlDecision::CompleteRun(RunOutcome::Failure),
                    )?;
                    return Ok(());
                }

                let worker_ref_path = app.paths.worker_ref_file(
                    task_id,
                    &run.id,
                    &round.id,
                    &node.node_id,
                    &node.attempt_id,
                );
                let repair_continue_ref = read_json::<WorkerRefState>(&worker_ref_path)
                    .ok()
                    .and_then(|worker_ref| worker_ref.continue_ref);
                let Some(repair_continue_ref) = repair_continue_ref else {
                    apply_control_decision(
                        app,
                        task_id,
                        workflow,
                        resolved_profiles,
                        run,
                        round,
                        &node,
                        ControlDecision::PauseRun(PauseReason::ErrorBlocked),
                    )?;
                    return Ok(());
                };

                invalid_output_repair_prompts += 1;
                let summary = format!(
                    "invalid output repair requested at {}/{}/{} ({}/{})",
                    round.id,
                    node.node_id,
                    node.attempt_id,
                    invalid_output_repair_prompts,
                    MAX_INVALID_OUTPUT_REPAIR_PROMPTS
                );
                progress(&summary);
                append_run_event_best_effort(
                    &app.paths,
                    task_id,
                    &run.id,
                    "invalid_output_repair_requested",
                    now_rfc3339_like(),
                    run_event_data(
                        &ctx,
                        Some(ProgressStage::CallingProvider),
                        Some(RunStatus::Running),
                        Some(summary),
                        None,
                    ),
                );
                node.status = RunStatus::Running;
                node.outcome = None;
                node.finished_at = None;
                run.status = RunStatus::Running;
                run.pause_reason = None;
                run.updated_at = now_rfc3339_like();
                round.status = RunStatus::Running;
                persist_runtime_state(app, task_id, run, round, &node)?;
                session_mode = SessionMode::Continue;
                continue_ref = Some(repair_continue_ref);
                resume_prompt = Some(invalid_output_repair_prompt(schema));
                resume_prompt_id = None;
                resume_prompt_visibility = PromptVisibility::Hidden;
                continue;
            }
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

        // ── Metrics: update last_executed_node and notify node completed ──
        {
            let status_str = match node.outcome {
                Some(crate::domain::NodeOutcome::Success) => "SUCCESS",
                Some(crate::domain::NodeOutcome::Failure) => "FAILED",
                Some(crate::domain::NodeOutcome::Killed) => "FAILED",
                Some(crate::domain::NodeOutcome::Invalid) => "FAILED",
                None => "FAILED",
            };
            let node_name = node.resolved_config.get("profileName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            // Read real token data from ACP session metadata
            let attempt_dir = app.paths.attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
            let session_paths = crate::acp::events::AcpAttemptPaths::from_attempt_dir(attempt_dir);
            let (input_tokens, output_tokens, cache_read_tokens, total_tokens) =
                crate::acp::events::read_session_tokens(&session_paths.session);
            let seq = round.trace.iter()
                .filter(|t| t.node_id == node.node_id)
                .map(|t| t.sequence)
                .last();
            let agent_type = node.resolved_config.get("provider")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            run.last_executed_node = Some(crate::runtime::LastExecutedNode {
                node_id: node.node_id.clone(),
                uuid: node.uuid.clone().unwrap_or_default(),
                round_uuid: round.uuid.clone().unwrap_or_default(),
                node_name,
                seq,
                agent_type,
                status: status_str.to_string(),
                started_at: node.started_at.clone(),
                finished_at: node.finished_at.clone(),
                input_tokens,
                output_tokens,
                cache_read_tokens,
                total_tokens,
            });
        }

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
            resume_prompt_visibility = PromptVisibility::Visible;
            invalid_output_repair_prompts = 0;
            continue;
        }
        // Workflow ended — send final metrics for the last completed node
        if let Some(metrics_cb) = &app.metrics_callback {
            let status_str = match node.outcome {
                Some(crate::domain::NodeOutcome::Success) => "SUCCESS",
                _ => "FAILED",
            };
            let node_name = node.resolved_config.get("profileName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            let agent_type = node.resolved_config.get("provider")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let seq = round.trace.iter()
                .filter(|t| t.node_id == node.node_id)
                .map(|t| t.sequence)
                .last();
            // Update last_executed_node with final state (read real token data from ACP session)
            let ad = app.paths.attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
            let sp = crate::acp::events::AcpAttemptPaths::from_attempt_dir(ad);
            let (it, ot, cr, tt) = crate::acp::events::read_session_tokens(&sp.session);
            run.last_executed_node = Some(crate::runtime::LastExecutedNode {
                node_id: node.node_id.clone(),
                uuid: node.uuid.clone().unwrap_or_default(),
                round_uuid: round.uuid.clone().unwrap_or_default(),
                node_name: node_name.clone(),
                seq,
                agent_type: agent_type.clone(),
                status: status_str.to_string(),
                started_at: node.started_at.clone(),
                finished_at: node.finished_at.clone(),
                input_tokens: it,
                output_tokens: ot,
                cache_read_tokens: cr,
                total_tokens: tt,
            });
            let metrics_ctx = super::MetricsEventContext {
                repo_root: app.paths.repo_root.to_string(),
                task_id: task_id.to_string(),
                run_id: run.id.clone(),
                round_id: round.id.clone(),
                node_id: node.node_id.clone(),
                attempt_id: node.attempt_id.clone(),
                task_uuid: run.task_uuid.clone(),
                run_uuid: run.uuid.clone(),
                round_uuid: round.uuid.clone(),
                node_uuid: node.uuid.clone(),
                seq,
                node_name: Some(node_name),
                agent_type,
                started_at: node.started_at.clone(),
                finished_at: node.finished_at.clone(),
                input_tokens: it,
                output_tokens: ot,
                cache_read_tokens: cr,
                total_tokens: tt,
            };
            // Send with predecessor = this node's final state
            metrics_cb(metrics_ctx, super::MetricsEvent::NodeCompleted);
        }
        return Ok(());
    }
}
