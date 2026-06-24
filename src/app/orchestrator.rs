use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use jsonschema::JSONSchema;
use jsonschema::error::{ValidationError, ValidationErrorKind};

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
    DynamicCompletionSchemaPolicy, DynamicCompletionStatus, DynamicGraphState, DynamicGroupState,
    DynamicGroupStatus, DynamicNext, DynamicNodeCompletion, DynamicNodeCompletionKind,
    DynamicNodeKind, DynamicNodeSpec, DynamicNodeSpecKind, DynamicNodeState, DynamicNodeStatus,
    DynamicProposalState, DynamicProposalValidationError, DynamicProposalValidationStatus,
    DynamicRunState, DynamicRunStatus, WorkspaceMode, WorkspacePolicy,
    dynamic_completion_effective_schema, dynamic_graph_has_active_leaf, dynamic_leaf_is_active,
    refresh_dynamic_current_leaf_ids, validate_dynamic_group_state, validate_dynamic_node_state,
    validate_dynamic_run_state,
};
use crate::observability::{
    ExecutionContext, ProgressStage, append_run_event_best_effort, progress, run_event_data,
    write_progress_hint, write_run_progress_best_effort,
};
use crate::prompts::{
    AI_DYNAMIC_ACCEPTANCE_EN, AI_DYNAMIC_ACCEPTANCE_ZH_CN, AI_DYNAMIC_FANOUT_EN,
    AI_DYNAMIC_FANOUT_ZH_CN, AI_DYNAMIC_MERGE_EN, AI_DYNAMIC_MERGE_ZH_CN, AI_DYNAMIC_NODE_TASK_EN,
    AI_DYNAMIC_NODE_TASK_ZH_CN, AI_DYNAMIC_OUTPUT_PROTOCOL_EN, AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN,
    AI_DYNAMIC_PROPOSAL_REPAIR_EN, AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN, AI_DYNAMIC_SYSTEM_EN,
    AI_DYNAMIC_SYSTEM_ZH_CN, AI_DYNAMIC_WORKFLOW_INVOCATION_EN,
    AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN, RUNTIME_INVALID_OUTPUT_REPAIR_EN,
    RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN, prompt_by_language, render as render_template,
};
use crate::provider::{
    PromptBundle, PromptOutputContract, PromptRuntimeContext, PromptVisibility, ProviderRunResult,
    ProviderRunStatus, StreamMode, WorkerInvocation, render_prompt_bundle,
    supported_models_from_capabilities,
};
use crate::runtime::{
    NodeState, RoundState, RoundTraceStep, RunState, TaskState, WorkerRefState,
    validate_round_state, validate_run_state, validate_worker_ref_state,
};
use crate::storage::{append_jsonl, read_json, write_json};

use super::ids::{generate_uuid, next_attempt_id, now_rfc3339_like, reserve_next_run_dir};
use super::node_executor::{execute_ai_node, re_evaluate_attempt};
use super::profile_resolver::{resolve_profile_for_node, resolve_workflow_profiles};
use super::state_access::{current_attempt_state, load_run_workflow, persist_runtime_state};
use super::state_factory::create_node_state;
use super::transition_context::find_latest_worker_ref_for_transition;
use super::{
    AcpLiveEventContext, App, RuntimeInterventionKind, RuntimeLifecycleEvent, is_run_continuable,
};

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
static DYNAMIC_COMPLETION_SCHEMA_CACHE: OnceLock<Mutex<HashMap<String, Arc<JSONSchema>>>> =
    OnceLock::new();
static DYNAMIC_WORKTREE_GIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static DYNAMIC_EVENT_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static DYNAMIC_STATE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static DYNAMIC_RESUME_REGISTRY: OnceLock<
    Mutex<HashMap<String, mpsc::Sender<DynamicResumeOverride>>>,
> = OnceLock::new();

#[derive(Debug)]
struct GitCommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DynamicWorkspaceCapability {
    supports_worktree: bool,
    reason_code: &'static str,
    reason: String,
}

fn dynamic_validation_error(
    code: &str,
    message: impl Into<String>,
    params: serde_json::Value,
) -> DynamicProposalValidationError {
    let mut error = DynamicProposalValidationError::new(code, message, params);
    enrich_dynamic_validation_error_defaults(&mut error);
    error
}

fn dynamic_validation_error_lines(errors: &[DynamicProposalValidationError]) -> String {
    errors
        .iter()
        .map(|error| {
            let path = error
                .path
                .as_deref()
                .map(|path| format!(" path={path}"))
                .unwrap_or_default();
            format!("- [{}]{} {}", error.code, path, error.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn enrich_dynamic_validation_error_defaults(error: &mut DynamicProposalValidationError) {
    if error.path.is_none() {
        error.path = infer_dynamic_error_path(&error.params);
    }
    if error.actual.is_none() {
        error.actual = infer_dynamic_error_actual(&error.params);
    }
    if error.expected.is_none() {
        error.expected = infer_dynamic_error_expected(error.code.as_str(), &error.params);
    }
}

fn json_param_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn infer_dynamic_error_path(params: &serde_json::Value) -> Option<String> {
    if let Some(path) = params.get("path").and_then(|value| value.as_str()) {
        return Some(path.to_string());
    }
    let field = params.get("field").and_then(|value| value.as_str());
    let stage = params.get("stage").and_then(|value| value.as_str());
    let node_id = params.get("nodeId").and_then(|value| value.as_str());
    match (stage, node_id, field) {
        (Some(stage @ ("merge" | "acceptance")), _, Some(field)) => {
            Some(format!("next.{stage}.{field}"))
        }
        (_, Some(node_id), Some(field)) => Some(format!("next.nodes[id={node_id}].{field}")),
        (_, Some(node_id), None) => Some(format!("next.nodes[id={node_id}]")),
        (_, _, Some(field)) => Some(field.to_string()),
        _ => None,
    }
}

fn infer_dynamic_error_actual(params: &serde_json::Value) -> Option<String> {
    [
        "actual",
        "profile",
        "provider",
        "model",
        "permissionMode",
        "workflowId",
        "nodeId",
        "groupId",
    ]
    .into_iter()
    .find_map(|key| params.get(key).and_then(json_param_string))
}

fn infer_dynamic_error_expected(code: &str, params: &serde_json::Value) -> Option<String> {
    if let Some(expected) = params.get("expected").and_then(json_param_string) {
        return Some(expected);
    }
    if code.ends_with(".blank") {
        return Some("non-empty value".to_string());
    }
    if code.ends_with(".unknown") {
        return Some("known configured value".to_string());
    }
    if code.ends_with(".unallowed") {
        return Some("allowed configured value".to_string());
    }
    None
}

fn localized_continue_prompt(language: DesktopLanguage) -> String {
    match language {
        DesktopLanguage::ZhCn => "继续".to_string(),
        DesktopLanguage::En => "Continue".to_string(),
    }
}

fn continue_prompt_or_default(language: DesktopLanguage, prompt: Option<String>) -> String {
    prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| localized_continue_prompt(language))
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
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    write_json(
        &app.paths.task_workflow_resolved_file(task_id),
        &validated.raw,
    )?;
    write_json(&app.paths.task_provenance_file(task_id), &resolved_profiles)?;

    let (run_id, _) = reserve_next_run_dir(&app.paths.runs_dir(task_id))?;
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
    write_json(
        &app.paths
            .node_file(task_id, &run_id, &round_id, &node.node_id, &node.attempt_id),
        &node,
    )?;
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

#[derive(Debug, Clone)]
struct DynamicResumeOverride {
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    attachment_paths: Vec<String>,
}

pub(crate) fn run_continue(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
    prompt: Option<String>,
) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
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
                Some(NodeDsl::AiDynamic(_)) => (SessionMode::Continue, None, None, None),
                _ => {
                    let continue_ref = read_json::<WorkerRefState>(&app.paths.worker_ref_file(
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &node.attempt_id,
                    ))?
                    .continue_ref
                    .ok_or_else(|| {
                        anyhow::anyhow!("current attempt has no ACP continue reference")
                    })?;
                    (
                        SessionMode::Continue,
                        Some(continue_ref),
                        Some(continue_prompt_or_default(
                            app.config.desktop_language,
                            prompt,
                        )),
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
        None,
    )?;
    Ok(run)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_continue_dynamic_inner(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    dynamic_attempt_id: &str,
    prompt_id: Option<String>,
    prompt: String,
    attachment_paths: Vec<String>,
) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow(workflow)?;
    app.validate_workflow_agents(&validated)?;
    ensure!(
        matches!(
            validated.get_node(outer_node_id),
            Some(NodeDsl::AiDynamic(_))
        ),
        "node `{outer_node_id}` is not an AI-DYNAMIC node"
    );
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    let mut run = app.run_status(task_id, run_id)?;
    let run_can_continue = is_run_continuable(&run) || run.status == RunStatus::Running;
    if !run_can_continue {
        bail!("current run is not resumable by continue");
    }
    ensure!(
        run.current_round.as_deref() == Some(round_id)
            && run.current_node.as_deref() == Some(outer_node_id)
            && run.current_attempt.as_deref() == Some(outer_attempt_id),
        "dynamic inner attempt is not in the current AI-DYNAMIC node"
    );
    let mut round: RoundState = read_json(&app.paths.round_file(task_id, run_id, round_id))?;
    let mut outer_node: NodeState = read_json(&app.paths.node_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    ))?;
    let graph_path =
        app.paths
            .dynamic_graph_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let mut graph = read_json::<DynamicGraphState>(&graph_path)?;
    let target_index = graph
        .nodes
        .iter()
        .position(|node| node.id == dynamic_node_id)
        .ok_or_else(|| anyhow!("dynamic node `{dynamic_node_id}` not found"))?;
    ensure!(
        self::dynamic_attempt_id(&graph.nodes[target_index]) == dynamic_attempt_id,
        "dynamic attempt `{dynamic_attempt_id}` does not match target node"
    );
    if normalize_stale_cancelled_dynamic_leaves_for_resume(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &mut graph,
    ) {
        persist_dynamic_graph_for_resume(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            &graph,
        )?;
    }
    ensure!(
        graph.nodes[target_index].status == DynamicNodeStatus::Paused,
        "dynamic node `{dynamic_node_id}` is not paused"
    );
    let resume_prompt = continue_prompt_or_default(app.config.desktop_language, Some(prompt));
    let resume_override = DynamicResumeOverride {
        node_id: dynamic_node_id.to_string(),
        attempt_id: dynamic_attempt_id.to_string(),
        prompt: resume_prompt,
        prompt_id,
        attachment_paths,
    };
    if graph.run.status == DynamicRunStatus::Running
        && send_dynamic_resume_override(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            resume_override.clone(),
        )?
    {
        return Ok(run);
    }
    outer_node.status = RunStatus::Paused;
    outer_node.outcome = None;
    outer_node.finished_at = None;
    drive_from_node_with_initial_session(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        outer_node,
        SessionMode::Continue,
        None,
        None,
        None,
        Some(resume_override),
    )?;
    Ok(run)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_continue_dynamic_inner_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    dynamic_attempt_id: &str,
    prompt_id: Option<String>,
    prompt: String,
    attachment_paths: Vec<String>,
) -> Result<RunState> {
    let initial_run = app.run_status(task_id, run_id)?;
    if !(is_run_continuable(&initial_run) || initial_run.status == RunStatus::Running) {
        bail!("current run is not resumable by continue");
    }
    ensure!(
        initial_run.current_round.as_deref() == Some(round_id)
            && initial_run.current_node.as_deref() == Some(outer_node_id)
            && initial_run.current_attempt.as_deref() == Some(outer_attempt_id),
        "dynamic inner attempt is not in the current AI-DYNAMIC node"
    );
    let background_app = app.clone_for_background();
    let task_id = task_id.to_string();
    let run_id = run_id.to_string();
    let round_id = round_id.to_string();
    let outer_node_id = outer_node_id.to_string();
    let outer_attempt_id = outer_attempt_id.to_string();
    let dynamic_node_id = dynamic_node_id.to_string();
    let dynamic_attempt_id = dynamic_attempt_id.to_string();
    thread::spawn(move || {
        let app = background_app;
        if let Err(err) = run_continue_dynamic_inner(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &outer_node_id,
            &outer_attempt_id,
            &dynamic_node_id,
            &dynamic_attempt_id,
            prompt_id,
            prompt,
            attachment_paths,
        ) {
            let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
            let _ = std::fs::write(
                app.paths
                    .runs_dir(&task_id)
                    .join("desktop-dynamic-continue-error.txt")
                    .as_std_path(),
                err.to_string(),
            );
        }
    });
    Ok(initial_run)
}

pub(crate) fn run_continue_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
    prompt: Option<String>,
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
    let prompt = prompt.clone();

    thread::spawn(move || {
        let app = background_app;
        if let Err(err) = run_continue(&app, &task_id, &run_id, prompt_id, prompt) {
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
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
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
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
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

fn attempt_is_still_current_running(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<bool> {
    let run: RunState = read_json(&app.paths.run_file(task_id, run_id))?;
    Ok(run.status == RunStatus::Running
        && run.current_round.as_deref() == Some(round_id)
        && run.current_node.as_deref() == Some(node_id)
        && run.current_attempt.as_deref() == Some(attempt_id))
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

fn node_label(node: &NodeState) -> String {
    node.resolved_config
        .get("profileName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| node.node_id.clone())
}

fn task_title(app: &App, task_id: &str) -> Option<String> {
    app.task_show(task_id).ok().and_then(|t| t.title)
}

fn emit_run_paused_lifecycle_event(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) {
    let reason = run.pause_reason.unwrap_or(PauseReason::ProcessInterrupted);
    app.emit_lifecycle_event(RuntimeLifecycleEvent::RunPaused {
        event_id: super::notification::make_dedup_key(
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
            reason,
        ),
        occurred_at: now_rfc3339_like(),
        task_id: task_id.to_string(),
        run_id: run.id.clone(),
        round_id: round.id.clone(),
        node_id: node.node_id.clone(),
        attempt_id: node.attempt_id.clone(),
        node_label: node_label(node),
        pause_reason: reason,
        task_title: task_title(app, task_id),
    });
}

fn emit_intervention_requested(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
    kind: RuntimeInterventionKind,
) {
    let pause_reason = super::notification::pause_reason_for_intervention(kind);
    app.emit_lifecycle_event(RuntimeLifecycleEvent::InterventionRequested {
        event_id: super::notification::make_dedup_key(
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
            pause_reason,
        ),
        occurred_at: now_rfc3339_like(),
        task_id: task_id.to_string(),
        run_id: run.id.clone(),
        round_id: round.id.clone(),
        node_id: node.node_id.clone(),
        attempt_id: node.attempt_id.clone(),
        node_label: node_label(node),
        kind,
        task_title: task_title(app, task_id),
    });
}

fn emit_run_completed_lifecycle_event(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
    outcome: RunOutcome,
) {
    app.emit_lifecycle_event(RuntimeLifecycleEvent::RunCompleted {
        event_id: super::notification::make_completion_dedup_key(
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        ),
        occurred_at: now_rfc3339_like(),
        task_id: task_id.to_string(),
        run_id: run.id.clone(),
        round_id: round.id.clone(),
        node_id: node.node_id.clone(),
        attempt_id: node.attempt_id.clone(),
        node_label: node_label(node),
        outcome,
        task_title: task_title(app, task_id),
    });
}

fn intervention_kind_for_pause(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) -> Option<RuntimeInterventionKind> {
    match run.pause_reason.unwrap_or(PauseReason::ProcessInterrupted) {
        reason @ (PauseReason::WaitingForUserInput
        | PauseReason::PermissionRequested
        | PauseReason::ErrorBlocked) => Some(RuntimeInterventionKind::from(reason)),
        PauseReason::ProcessInterrupted => {
            (!attempt_was_user_cancelled(app, task_id, &run.id, &round.id, node))
                .then_some(RuntimeInterventionKind::ProcessInterrupted)
        }
    }
}

fn emit_pause_side_effects(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) {
    emit_run_paused_lifecycle_event(app, task_id, run, round, node);
    if let Some(kind) = intervention_kind_for_pause(app, task_id, run, round, node) {
        emit_intervention_requested(app, task_id, run, round, node, kind);
    }
}

fn attempt_was_user_cancelled(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node: &NodeState,
) -> bool {
    let attempt_dir =
        app.paths
            .attempt_dir(task_id, run_id, round_id, &node.node_id, &node.attempt_id);
    if attempt_dir_was_cancelled(&attempt_dir) {
        return true;
    }
    if node.node_type != crate::domain::NodeType::AiDynamic {
        return false;
    }
    let graph_path =
        app.paths
            .dynamic_graph_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id);
    let Ok(graph) = read_json::<DynamicGraphState>(&graph_path) else {
        return false;
    };
    graph.nodes.iter().any(|dynamic_node| {
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            &node.node_id,
            &node.attempt_id,
            &dynamic_node.id,
            &dynamic_attempt_id(dynamic_node),
        );
        attempt_dir_was_cancelled(&attempt_dir)
    })
}

fn dynamic_node_attempt_was_cancelled(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node: &DynamicNodeState,
) -> bool {
    let attempt_dir = app.paths.dynamic_node_attempt_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &dynamic_node.id,
        &dynamic_attempt_id(dynamic_node),
    );
    attempt_dir_was_cancelled(&attempt_dir)
}

fn dynamic_node_is_stale_resumable_active(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph_status: DynamicRunStatus,
    dynamic_node: &DynamicNodeState,
) -> bool {
    dynamic_leaf_is_active(dynamic_node.status)
        && dynamic_node.outcome.is_none()
        && (graph_status == DynamicRunStatus::Paused
            || dynamic_node_attempt_was_cancelled(
                app,
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                dynamic_node,
            ))
}

fn normalize_stale_cancelled_dynamic_leaves_for_resume(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &mut DynamicGraphState,
) -> bool {
    let mut changed = false;
    for node in &mut graph.nodes {
        if dynamic_node_is_stale_resumable_active(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            graph.run.status,
            node,
        ) {
            node.status = DynamicNodeStatus::Paused;
            node.outcome = None;
            node.finished_at = Some(now_rfc3339_like());
            changed = true;
        }
    }
    if changed {
        refresh_dynamic_current_leaf_ids(graph);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.outcome = None;
        graph.run.pause_reason = Some(PauseReason::ProcessInterrupted);
        graph.run.updated_at = now_rfc3339_like();
    }
    changed
}

fn attempt_dir_was_cancelled(attempt_dir: &Utf8Path) -> bool {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    [snapshot_path, session_path].iter().any(|path| {
        read_json::<serde_json::Value>(path)
            .ok()
            .and_then(|value| {
                value
                    .get("stopReason")
                    .or_else(|| value.get("stop_reason"))
                    .and_then(|reason| reason.as_str().map(str::to_string))
                    .or_else(|| {
                        value
                            .get("status")
                            .and_then(|status| status.as_str().map(str::to_string))
                    })
            })
            .is_some_and(|value| value.eq_ignore_ascii_case("cancelled"))
    })
}

fn completed_node_snapshot(
    round: &RoundState,
    node: &NodeState,
    attempt_dir: Option<String>,
) -> crate::runtime::LastExecutedNode {
    let status = match node.outcome {
        Some(crate::domain::NodeOutcome::Success) => "SUCCESS",
        Some(crate::domain::NodeOutcome::Failure)
        | Some(crate::domain::NodeOutcome::Killed)
        | Some(crate::domain::NodeOutcome::Invalid)
        | None => "FAILED",
    };
    let node_name = node
        .resolved_config
        .get("profileName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
        .or_else(|| {
            node.resolved_config
                .get("provider")
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();
    let seq = round
        .trace
        .iter()
        .filter(|t| t.node_id == node.node_id)
        .map(|t| t.sequence)
        .last();
    let agent_type = node
        .resolved_config
        .get("provider")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    crate::runtime::LastExecutedNode {
        node_id: node.node_id.clone(),
        uuid: node.uuid.clone().unwrap_or_default(),
        round_uuid: round.uuid.clone().unwrap_or_default(),
        node_name,
        seq,
        agent_type,
        status: status.to_string(),
        started_at: node.started_at.clone(),
        finished_at: node.finished_at.clone(),
        attempt_dir,
    }
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
            persist_runtime_state(app, task_id, run, round, &next_node)?;
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
            persist_runtime_state(app, task_id, run, round, &next_node)?;
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
            round.outcome = None;
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
            emit_pause_side_effects(app, task_id, run, round, node);
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
            let completed_node_id = node.node_id.clone();
            let completed_attempt_id = node.attempt_id.clone();
            persist_runtime_state(app, task_id, run, round, node)?;
            emit_run_completed_lifecycle_event(app, task_id, run, round, node, outcome);
            emit_completed_acp_session_update_best_effort(
                app,
                task_id,
                &run.id,
                &round.id,
                &completed_node_id,
                &completed_attempt_id,
            );
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
    // UUIDs from the outer run/round/node — used for metrics reporting
    task_uuid: Option<&'a str>,
    run_uuid: Option<&'a str>,
    round_uuid: Option<&'a str>,
    outer_node_uuid: Option<&'a str>,
    resume_override: Option<DynamicResumeOverride>,
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

#[derive(Debug, Clone, Copy)]
struct DynamicNodeStatusCounts {
    pending: usize,
    ready: usize,
    running: usize,
    paused: usize,
    completed: usize,
}

fn dynamic_node_status_counts(graph: &DynamicGraphState) -> DynamicNodeStatusCounts {
    let mut counts = DynamicNodeStatusCounts {
        pending: 0,
        ready: 0,
        running: 0,
        paused: 0,
        completed: 0,
    };
    for node in &graph.nodes {
        match node.status {
            DynamicNodeStatus::Pending => counts.pending += 1,
            DynamicNodeStatus::Ready => counts.ready += 1,
            DynamicNodeStatus::Running => counts.running += 1,
            DynamicNodeStatus::Paused => counts.paused += 1,
            DynamicNodeStatus::Completed => counts.completed += 1,
        }
    }
    counts
}

fn dynamic_timing_data(graph: &DynamicGraphState) -> serde_json::Value {
    let counts = dynamic_node_status_counts(graph);
    serde_json::json!({
        "dynamicRunId": graph.run.id,
        "pendingCount": counts.pending,
        "readyCount": counts.ready,
        "runningCount": counts.running,
        "pausedCount": counts.paused,
        "completedCount": counts.completed,
        "maxParallel": graph.run.control.max_parallel,
        "currentNodeIds": graph.run.current_node_ids.clone(),
    })
}

fn dynamic_event_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    event_type: &str,
    data: serde_json::Value,
) {
    let _ = append_dynamic_event(ctx, event_type, data);
}

fn append_dynamic_event_for_ids_best_effort(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    event_type: &str,
    data: serde_json::Value,
) {
    let _ = append_dynamic_event_for_ids(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        event_type,
        data,
    );
}

fn dynamic_event_lock_key(
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> String {
    format!("{task_id}/{run_id}/{round_id}/{outer_node_id}/{outer_attempt_id}")
}

fn dynamic_event_lock_for(
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> Result<Arc<Mutex<()>>> {
    let key = dynamic_event_lock_key(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let mut locks = DYNAMIC_EVENT_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("dynamic event lock registry poisoned"))?;
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn append_dynamic_event_for_ids(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    event_type: &str,
    data: serde_json::Value,
) -> Result<()> {
    let lock = dynamic_event_lock_for(task_id, run_id, round_id, outer_node_id, outer_attempt_id)?;
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("dynamic event lock poisoned"))?;
    append_jsonl(
        &app.paths
            .dynamic_events_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
        &serde_json::json!({
            "timestamp": now_rfc3339_like(),
            "type": event_type,
            "data": data,
        }),
    )
}

fn elapsed_ms(started_at: Instant) -> u128 {
    started_at.elapsed().as_millis()
}

fn dynamic_invocation_build_step_begin(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    attempt_id: &str,
    step: &str,
) -> Instant {
    let started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_invocation_build_step_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "attemptId": attempt_id,
            "step": step,
        }),
    );
    started_at
}

fn dynamic_invocation_build_step_end(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    attempt_id: &str,
    step: &str,
    started_at: Instant,
    data: serde_json::Value,
) {
    let mut payload = serde_json::json!({
        "nodeId": node.id,
        "kind": node.kind,
        "attemptId": attempt_id,
        "step": step,
        "elapsedMs": elapsed_ms(started_at),
    });
    if let (Some(target), serde_json::Value::Object(extra)) = (payload.as_object_mut(), data) {
        for (key, value) in extra {
            target.insert(key, value);
        }
    }
    dynamic_event_best_effort(ctx, "dynamic_worker_invocation_build_step_end", payload);
}

fn truncate_diagnostic_text(value: &str) -> String {
    const MAX_LEN: usize = 500;
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX_LEN {
        return trimmed.to_string();
    }
    let mut output = trimmed.chars().take(MAX_LEN).collect::<String>();
    output.push_str("...");
    output
}

struct DynamicResumeRegistration {
    key: String,
}

impl Drop for DynamicResumeRegistration {
    fn drop(&mut self) {
        if let Some(registry) = DYNAMIC_RESUME_REGISTRY.get() {
            if let Ok(mut registry) = registry.lock() {
                registry.remove(&self.key);
            }
        }
    }
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

fn emit_completed_acp_session_update_best_effort(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) {
    let _ = app.emit_acp_session_update(AcpLiveEventContext {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: None,
        outer_attempt_id: None,
    });
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

fn emit_dynamic_session_update_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
) {
    let _ = ctx
        .app
        .emit_acp_session_update(dynamic_acp_live_event_context(ctx, node_id, attempt_id));
}

fn emit_dynamic_session_updates_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node_ids: &[String],
) {
    let mut seen = std::collections::HashSet::new();
    for node_id in node_ids {
        if !seen.insert(node_id) {
            continue;
        }
        let Some(node) = graph.nodes.iter().find(|node| node.id == *node_id) else {
            continue;
        };
        emit_dynamic_session_update_best_effort(ctx, &node.id, &dynamic_attempt_id(node));
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
        runtime_node_id: Some(ctx.outer_node_id.to_string()),
        runtime_attempt_id: Some(ctx.outer_attempt_id.to_string()),
        attempt_state_file: Some(ctx.app.paths.dynamic_node_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            node_id,
        )),
        language: ctx.app.config.desktop_language,
        run_dir,
        round_dir,
        node_dir,
        attempt_dir,
        attachments_dir,
        task_inputs_dir: super::existing_task_inputs_dir(ctx.app, ctx.task_id),
    }
}

fn dynamic_agent_strategy_mode(dynamic: &AiDynamicNode) -> &'static str {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => "fixed",
        AiDynamicAgentStrategy::Dynamic { .. } => "dynamic",
    }
}

fn dynamic_model_for_provider(dynamic: &AiDynamicNode, provider: &str) -> Option<String> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { model, .. } => model.clone(),
        AiDynamicAgentStrategy::Dynamic {
            available_agents, ..
        } => available_agents
            .iter()
            .find(|agent_ref| agent_ref.provider == provider)
            .and_then(|agent_ref| agent_ref.model.clone()),
    }
}

fn dynamic_acceptance_model(dynamic: &AiDynamicNode) -> Option<&str> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => None,
        AiDynamicAgentStrategy::Dynamic {
            acceptance_model, ..
        } => acceptance_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty()),
    }
}

fn dynamic_requires_model_in_proposal(dynamic: &AiDynamicNode) -> bool {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => false,
        AiDynamicAgentStrategy::Dynamic { routing_prompt, .. } => !routing_prompt.trim().is_empty(),
    }
}

fn dynamic_requires_provider_in_proposal(dynamic: &AiDynamicNode) -> bool {
    matches!(
        &dynamic.agent_strategy,
        AiDynamicAgentStrategy::Dynamic { .. }
    )
}

fn provider_model_options_summary(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> Vec<String> {
    provider_model_option_values(ctx, provider)
        .into_iter()
        .map(|(name, description)| match description {
            Some(description) => format!("{name} — {description}"),
            None => name,
        })
        .collect()
}

fn provider_model_option_values(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> Vec<(String, Option<String>)> {
    let Ok(doctor) = ctx.app.provider_doctor(provider) else {
        return Vec::new();
    };
    supported_models_from_capabilities(doctor.capabilities.as_ref())
        .into_iter()
        .map(|model| {
            let name = model.name.as_deref().unwrap_or(model.id.as_str());
            (name.to_string(), model.description)
        })
        .collect()
}

fn dynamic_worker_model_required_from_proposal(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> bool {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Dynamic { .. } => dynamic_requires_model_in_proposal(ctx.dynamic),
        AiDynamicAgentStrategy::Fixed { .. } => {
            dynamic_model_for_provider(ctx.dynamic, provider).is_none()
                && !provider_model_options_summary(ctx, provider).is_empty()
        }
    }
}

fn dynamic_agent_task_model_required_from_proposal(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> bool {
    if dynamic_acceptance_model(ctx.dynamic).is_some() {
        return false;
    }
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Dynamic { .. } => dynamic_requires_model_in_proposal(ctx.dynamic),
        AiDynamicAgentStrategy::Fixed { .. } => {
            dynamic_model_for_provider(ctx.dynamic, provider).is_none()
                && !provider_model_options_summary(ctx, provider).is_empty()
        }
    }
}

fn dynamic_any_worker_model_required_from_proposal(ctx: &DynamicExecutionContext<'_>) -> bool {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, .. } => {
            dynamic_worker_model_required_from_proposal(ctx, provider)
        }
        AiDynamicAgentStrategy::Dynamic { .. } => dynamic_requires_model_in_proposal(ctx.dynamic),
    }
}

fn dynamic_model_policy_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, model } => {
            if let Some(model) = model.as_deref().filter(|model| !model.trim().is_empty()) {
                return format!(
                    "The fixed provider has configured model `{model}`; do not output `model`."
                );
            }
            if dynamic_worker_model_required_from_proposal(ctx, provider) {
                "The fixed provider has no configured model and exposes selectable models; output `model` for every worker / merge / acceptance node, using one model name from the provider list.".to_string()
            } else {
                "The fixed provider has no configured model catalog; do not output `model`, and runtime will use the provider default.".to_string()
            }
        }
        AiDynamicAgentStrategy::Dynamic { routing_prompt, .. } => {
            if let Some(model) = dynamic_acceptance_model(ctx.dynamic) {
                if routing_prompt.trim().is_empty() {
                    format!(
                        "Routing guidance is empty, so worker models stay runtime-configured; do not output `model` for workers. `merge` / `acceptance` use the configured acceptance model `{model}`; do not output `model` for them."
                    )
                } else {
                    format!(
                        "Routing guidance is configured, so every worker node must output `model`; if a provider already has a configured model, runtime still prefers the configured model. `merge` / `acceptance` use the configured acceptance model `{model}`; do not output `model` for them."
                    )
                }
            } else if routing_prompt.trim().is_empty() {
                "Routing guidance is empty, so provider models are configured by runtime; do not output `model` for worker / merge / acceptance nodes.".to_string()
            } else {
                "Routing guidance is configured, so every worker / merge / acceptance node must output `model`; if a provider already has a configured model, runtime still prefers the configured model.".to_string()
            }
        }
    }
}

fn dynamic_model_policy_summary_zh_cn(ctx: &DynamicExecutionContext<'_>) -> String {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, model } => {
            if let Some(model) = model.as_deref().filter(|model| !model.trim().is_empty()) {
                return format!("固定 provider 已配置模型 `{model}`；不要输出 `model`。");
            }
            if dynamic_worker_model_required_from_proposal(ctx, provider) {
                "固定 provider 未配置模型且提供了可选模型列表；每个 worker / merge / acceptance 节点都必须输出 `model`，并使用 provider 列表中的模型名称。".to_string()
            } else {
                "固定 provider 没有可用模型列表；不要输出 `model`，runtime 会使用 provider 默认模型。".to_string()
            }
        }
        AiDynamicAgentStrategy::Dynamic { routing_prompt, .. } => {
            if let Some(model) = dynamic_acceptance_model(ctx.dynamic) {
                if routing_prompt.trim().is_empty() {
                    format!(
                        "当前没有节点 agent 选择说明，worker 的 provider 模型由 runtime 配置决定；不要在 worker 节点中输出 `model`。`merge` / `acceptance` 统一使用已配置的验收模型 `{model}`；不要为它们输出 `model`。"
                    )
                } else {
                    format!(
                        "当前提供了节点 agent 选择说明，因此每个 worker 节点都必须输出 `model`；如果某个 provider 已经锁定模型，runtime 仍会优先使用配置模型。`merge` / `acceptance` 统一使用已配置的验收模型 `{model}`；不要为它们输出 `model`。"
                    )
                }
            } else if routing_prompt.trim().is_empty() {
                "当前没有节点 agent 选择说明，provider 模型由 runtime 配置决定；不要在 worker / merge / acceptance 节点中输出 `model`。".to_string()
            } else {
                "当前提供了节点 agent 选择说明，因此每个 worker / merge / acceptance 节点都必须输出 `model`；如果某个 provider 已经锁定模型，runtime 仍会优先使用配置模型。".to_string()
            }
        }
    }
}

fn dynamic_agent_routing_prompt(dynamic: &AiDynamicNode) -> Option<&str> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => None,
        AiDynamicAgentStrategy::Dynamic { routing_prompt, .. } => Some(routing_prompt.trim()),
    }
}

fn dynamic_completion_schema_policy(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) -> DynamicCompletionSchemaPolicy {
    let provider_ids = dynamic_available_provider_ids(ctx);
    let mut model_names = Vec::new();
    let node_model_required = dynamic_any_worker_model_required_from_proposal(ctx);
    let agent_task_model_required = match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, .. } => {
            dynamic_agent_task_model_required_from_proposal(ctx, provider)
        }
        AiDynamicAgentStrategy::Dynamic { .. } => {
            dynamic_requires_model_in_proposal(ctx.dynamic)
                && dynamic_acceptance_model(ctx.dynamic).is_none()
        }
    };
    let any_model_visible = node_model_required || agent_task_model_required;
    if any_model_visible {
        for provider in &provider_ids {
            for (model, _) in provider_model_option_values(ctx, provider) {
                if !model_names.iter().any(|existing| existing == &model) {
                    model_names.push(model);
                }
            }
        }
    }
    DynamicCompletionSchemaPolicy {
        provider_required: dynamic_requires_provider_in_proposal(ctx.dynamic),
        node_model_required,
        agent_task_model_required,
        agent_task_model_visible: dynamic_acceptance_model(ctx.dynamic).is_none(),
        provider_ids,
        model_names,
        profile_ids: available_profile_refs(ctx)
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
        workflow_ids: graph
            .run
            .allowed_workflow_snapshots
            .iter()
            .map(|snapshot| snapshot.workflow_id.clone())
            .collect(),
        max_fanout: graph.run.control.max_fanout,
    }
}

fn dynamic_effective_completion_schema(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) -> serde_json::Value {
    let policy = dynamic_completion_schema_policy(ctx, graph);
    dynamic_completion_effective_schema(&policy)
}

fn dynamic_output_contract(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) -> PromptOutputContract {
    let language = ctx.app.config.desktop_language;
    let schema = dynamic_effective_completion_schema(ctx, graph);
    let json_schema = serde_json::to_string_pretty(&schema).expect("serialize dynamic schema");
    let schema_text = render_template(
        prompt_by_language(
            language,
            AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN,
            AI_DYNAMIC_OUTPUT_PROTOCOL_EN,
        ),
        serde_json::json!({
            "agent_strategy_mode": dynamic_agent_strategy_mode(ctx.dynamic),
            "provider_required_in_proposal": dynamic_requires_provider_in_proposal(ctx.dynamic),
            "model_required_in_proposal": dynamic_any_worker_model_required_from_proposal(ctx),
            "model_policy": match language {
                DesktopLanguage::ZhCn => dynamic_model_policy_summary_zh_cn(ctx),
                DesktopLanguage::En => dynamic_model_policy_summary(ctx),
            },
            "json_schema": json_schema,
        }),
    )
    .expect("prompt template renders");
    PromptOutputContract {
        artifact: DYNAMIC_COMPLETION_ARTIFACT.to_string(),
        kind: "json".to_string(),
        schema: Some(schema),
        schema_text: Some(schema_text.trim().to_string()),
        success_condition: None,
    }
}

fn dynamic_attempt_id(_node: &DynamicNodeState) -> String {
    "attempt-001".to_string()
}

fn dynamic_proposal_file_path(ctx: &DynamicExecutionContext<'_>, proposal_id: &str) -> Utf8PathBuf {
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

fn dynamic_state_lock_key(
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> String {
    format!("{task_id}/{run_id}/{round_id}/{outer_node_id}/{outer_attempt_id}")
}

fn dynamic_state_lock(ctx: &DynamicExecutionContext<'_>) -> Result<Arc<Mutex<()>>> {
    dynamic_state_lock_for(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    )
}

fn dynamic_state_lock_for(
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> Result<Arc<Mutex<()>>> {
    let key = dynamic_state_lock_key(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let mut locks = DYNAMIC_STATE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("dynamic state lock registry poisoned"))?;
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn persist_dynamic_graph_for_resume(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &DynamicGraphState,
) -> Result<()> {
    validate_dynamic_run_state(&graph.run)?;
    for node in &graph.nodes {
        validate_dynamic_node_state(node)?;
    }
    let lock = dynamic_state_lock_for(task_id, run_id, round_id, outer_node_id, outer_attempt_id)?;
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("dynamic state lock poisoned"))?;
    write_json(
        &app.paths
            .dynamic_run_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
        &graph.run,
    )?;
    write_json(
        &app.paths
            .dynamic_graph_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
        graph,
    )?;
    for node in &graph.nodes {
        write_json(
            &app.paths.dynamic_node_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                &node.id,
            ),
            node,
        )?;
    }
    Ok(())
}

fn register_dynamic_resume_channel(
    ctx: &DynamicExecutionContext<'_>,
    tx: mpsc::Sender<DynamicResumeOverride>,
) -> Result<DynamicResumeRegistration> {
    let key = dynamic_state_lock_key(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    DYNAMIC_RESUME_REGISTRY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume registry poisoned"))?
        .insert(key.clone(), tx);
    Ok(DynamicResumeRegistration { key })
}

fn send_dynamic_resume_override(
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    resume: DynamicResumeOverride,
) -> Result<bool> {
    let key = dynamic_state_lock_key(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let sender = DYNAMIC_RESUME_REGISTRY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume registry poisoned"))?
        .get(&key)
        .cloned();
    let Some(sender) = sender else {
        return Ok(false);
    };
    Ok(sender.send(resume).is_ok())
}

fn execute_ai_dynamic_node(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    attempt_id: &str,
    dynamic: &AiDynamicNode,
    mut outer_node: NodeState,
    resume_override: Option<DynamicResumeOverride>,
) -> Result<NodeState> {
    let ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id: &run.id,
        round_id: &round.id,
        outer_node_id: &outer_node.node_id,
        outer_attempt_id: attempt_id,
        dynamic,
        task_uuid: run.task_uuid.as_deref(),
        run_uuid: run.uuid.as_deref(),
        round_uuid: round.uuid.as_deref(),
        outer_node_uuid: outer_node.uuid.as_deref(),
        resume_override,
    };
    let mut graph = load_or_create_dynamic_graph(&ctx)?;
    resume_paused_dynamic_graph(&mut graph, ctx.resume_override.as_ref())?;
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

fn rearm_dynamic_resume_target(
    graph: &mut DynamicGraphState,
    resume: &DynamicResumeOverride,
) -> Result<()> {
    let target = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == resume.node_id)
        .ok_or_else(|| anyhow!("dynamic node `{}` not found", resume.node_id))?;
    ensure!(
        self::dynamic_attempt_id(target) == resume.attempt_id,
        "dynamic attempt `{}` does not match target node",
        resume.attempt_id
    );
    ensure!(
        target.status == DynamicNodeStatus::Paused,
        "dynamic node `{}` is not paused",
        resume.node_id
    );
    target.status = DynamicNodeStatus::Ready;
    target.outcome = None;
    target.finished_at = None;
    Ok(())
}

fn apply_dynamic_resume_overrides(
    graph: &mut DynamicGraphState,
    resumes: &mut Vec<DynamicResumeOverride>,
) -> Result<Vec<DynamicResumeOverride>> {
    let mut applied_indexes = Vec::new();
    for (index, resume) in resumes.iter().enumerate() {
        rearm_dynamic_resume_target(graph, resume)?;
        applied_indexes.push(index);
    }
    let mut applied = Vec::new();
    for index in applied_indexes.into_iter().rev() {
        applied.push(resumes.remove(index));
    }
    applied.reverse();
    if graph.run.status == DynamicRunStatus::Paused || !applied.is_empty() {
        graph.run.status = DynamicRunStatus::Running;
        graph.run.outcome = None;
        graph.run.pause_reason = None;
    }
    graph.run.updated_at = now_rfc3339_like();
    refresh_dynamic_current_leaf_ids(graph);
    Ok(applied)
}

fn rearm_paused_workflow_invocations_for_parent_continue(graph: &mut DynamicGraphState) -> bool {
    if graph.run.status != DynamicRunStatus::Paused {
        return false;
    }
    let mut changed = false;
    for node in &mut graph.nodes {
        if node.kind == DynamicNodeKind::WorkflowInvocation
            && node.status == DynamicNodeStatus::Paused
            && node.outcome.is_none()
        {
            node.status = DynamicNodeStatus::Ready;
            node.finished_at = None;
            changed = true;
        }
    }
    if changed {
        graph.run.status = DynamicRunStatus::Running;
        graph.run.outcome = None;
        graph.run.pause_reason = None;
        graph.run.updated_at = now_rfc3339_like();
        refresh_dynamic_current_leaf_ids(graph);
    }
    changed
}

fn resume_paused_dynamic_graph(
    graph: &mut DynamicGraphState,
    resume_override: Option<&DynamicResumeOverride>,
) -> Result<()> {
    if let Some(resume) = resume_override {
        if graph.run.status == DynamicRunStatus::Paused {
            graph.run.status = DynamicRunStatus::Running;
            graph.run.outcome = None;
            graph.run.pause_reason = None;
        }
        rearm_dynamic_resume_target(graph, resume)?;
        graph.run.updated_at = now_rfc3339_like();
        refresh_dynamic_current_leaf_ids(graph);
        return Ok(());
    }
    rearm_paused_workflow_invocations_for_parent_continue(graph);
    Ok(())
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
        let lock = dynamic_state_lock(ctx)?;
        let _guard = lock
            .lock()
            .map_err(|_| anyhow!("dynamic state lock poisoned"))?;
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
        permission_mode: ctx
            .dynamic
            .bootstrap_provider()
            .and_then(|provider| {
                ctx.dynamic
                    .permission_mode()
                    .map(|mode| ctx.app.config.resolve_permission_mode(provider, mode))
            })
            .or_else(|| ctx.dynamic.permission_mode().map(ToOwned::to_owned)),
        model: ctx.dynamic.bootstrap_model().map(ToOwned::to_owned),
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
    let (resume_tx, resume_rx) = mpsc::channel::<DynamicResumeOverride>();
    let _resume_registration = register_dynamic_resume_channel(ctx, resume_tx)?;
    let mut pending_resume_overrides = Vec::new();
    let mut launch_resume_overrides = ctx.resume_override.clone().into_iter().collect::<Vec<_>>();
    let mut scheduler_loop_count = 0_u64;
    let mut last_waiting_workers_event_at: Option<Instant> = None;
    loop {
        scheduler_loop_count = scheduler_loop_count.saturating_add(1);
        if !outer_attempt_is_still_current_running(ctx)? {
            pause_dynamic_graph(
                ctx,
                graph,
                PauseReason::ProcessInterrupted,
                "outer runtime attempt stopped before dynamic graph settled",
            )?;
            return Ok(());
        }
        while let Ok(resume) = resume_rx.try_recv() {
            pending_resume_overrides.push(resume);
        }
        launch_resume_overrides.extend(apply_dynamic_resume_overrides(
            graph,
            &mut pending_resume_overrides,
        )?);
        let ready_refresh_started_at = Instant::now();
        let ready_node_ids = refresh_dynamic_ready_nodes(graph);
        dynamic_event_best_effort(
            ctx,
            "dynamic_ready_refreshed",
            serde_json::json!({
                "loop": scheduler_loop_count,
                "elapsedMs": elapsed_ms(ready_refresh_started_at),
                "readyNodeIds": ready_node_ids,
                "state": dynamic_timing_data(graph),
            }),
        );
        if !ready_node_ids.is_empty() {
            persist_dynamic_graph(ctx, graph)?;
            emit_dynamic_session_updates_best_effort(ctx, graph, &ready_node_ids);
        }
        let launch_started_at = Instant::now();
        dynamic_event_best_effort(
            ctx,
            "dynamic_launch_ready_begin",
            serde_json::json!({
                "loop": scheduler_loop_count,
                "state": dynamic_timing_data(graph),
            }),
        );
        launch_ready_dynamic_nodes(ctx, graph, &tx, &mut launch_resume_overrides)?;
        dynamic_event_best_effort(
            ctx,
            "dynamic_launch_ready_end",
            serde_json::json!({
                "loop": scheduler_loop_count,
                "elapsedMs": elapsed_ms(launch_started_at),
                "state": dynamic_timing_data(graph),
            }),
        );
        persist_dynamic_graph(ctx, graph)?;

        if advance_dynamic_groups(ctx, graph)?.changed {
            continue;
        }
        if dynamic_graph_completed(graph) {
            for node in &graph.nodes {
                teardown_dynamic_workspace_best_effort(ctx, node);
            }
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
            if last_waiting_workers_event_at
                .map(|last| last.elapsed() >= Duration::from_secs(5))
                .unwrap_or(true)
            {
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_waiting_workers",
                    serde_json::json!({
                        "loop": scheduler_loop_count,
                        "state": dynamic_timing_data(graph),
                    }),
                );
                last_waiting_workers_event_at = Some(Instant::now());
            }
            let message = match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("dynamic execution channel closed unexpectedly"));
                }
            };
            apply_dynamic_execution_message(ctx, graph, message)?;
            if graph.run.status == DynamicRunStatus::Paused {
                return Ok(());
            }
            continue;
        }

        if dynamic_graph_has_paused_leaf(graph) {
            pause_dynamic_graph(
                ctx,
                graph,
                PauseReason::ProcessInterrupted,
                "dynamic graph is waiting for paused dynamic leaf continue",
            )?;
            return Ok(());
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
    launch_resume_overrides: &mut Vec<DynamicResumeOverride>,
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
        dynamic_event_best_effort(
            ctx,
            "dynamic_launch_candidate",
            serde_json::json!({
                "nodeId": node_id,
                "state": dynamic_timing_data(graph),
            }),
        );
        let node = graph
            .nodes
            .get_mut(index)
            .ok_or_else(|| anyhow!("dynamic node index out of range"))?;
        node.status = DynamicNodeStatus::Running;
        node.started_at.get_or_insert_with(now_rfc3339_like);
        let node_clone = node.clone();
        let node_id_for_job = node_clone.id.clone();
        graph.run.updated_at = now_rfc3339_like();
        dynamic_event_best_effort(
            ctx,
            "dynamic_node_marked_running",
            serde_json::json!({
                "nodeId": node_id_for_job,
                "kind": node_clone.kind,
                "sessionMode": node_clone.session_mode,
                "workspaceMode": node_clone.workspace.mode,
                "providerId": node_clone.provider.clone(),
                "model": node_clone.model.clone(),
                "state": dynamic_timing_data(graph),
            }),
        );
        persist_dynamic_graph(ctx, graph)?;
        emit_dynamic_session_update_best_effort(
            ctx,
            &node_id_for_job,
            &dynamic_attempt_id(&node_clone),
        );

        let background_app = ctx.app.clone_for_background();
        let task_id = ctx.task_id.to_string();
        let run_id = ctx.run_id.to_string();
        let round_id = ctx.round_id.to_string();
        let outer_node_id = ctx.outer_node_id.to_string();
        let outer_attempt_id = ctx.outer_attempt_id.to_string();
        let dynamic = ctx.dynamic.clone();
        let tx = tx.clone();
        let task_uuid = ctx.task_uuid.map(|s| s.to_string());
        let run_uuid = ctx.run_uuid.map(|s| s.to_string());
        let round_uuid = ctx.round_uuid.map(|s| s.to_string());
        let outer_node_uuid = ctx.outer_node_uuid.map(|s| s.to_string());
        let resume_override = launch_resume_overrides
            .iter()
            .rposition(|resume| resume.node_id == node_id_for_job)
            .map(|index| launch_resume_overrides.remove(index));
        let spawned_node_id = node_id_for_job.clone();
        thread::spawn(move || {
            let app = background_app;
            let node_id = node_id_for_job;
            let result = catch_unwind(AssertUnwindSafe(|| {
                execute_dynamic_node_job(
                    &app,
                    &task_id,
                    &run_id,
                    &round_id,
                    &outer_node_id,
                    &outer_attempt_id,
                    &dynamic,
                    node_clone,
                    task_uuid.as_deref(),
                    run_uuid.as_deref(),
                    round_uuid.as_deref(),
                    outer_node_uuid.as_deref(),
                    resume_override,
                )
            }))
            .unwrap_or_else(|payload| {
                let panic_message = payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_string());
                Err(anyhow!("dynamic node job panicked: {panic_message}"))
            });
            let message = DynamicExecutionMessage { node_id, result };
            let _ = tx.send(message);
        });
        dynamic_event_best_effort(
            ctx,
            "dynamic_thread_spawned",
            serde_json::json!({
                "nodeId": spawned_node_id,
                "state": dynamic_timing_data(graph),
            }),
        );
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
    let result = match message.result {
        Ok(result) => result,
        Err(error) => {
            let reason = error.to_string();
            graph.nodes[index].status = DynamicNodeStatus::Paused;
            graph.nodes[index].outcome = None;
            graph.nodes[index].finished_at = Some(now_rfc3339_like());
            pause_dynamic_graph(ctx, graph, PauseReason::ErrorBlocked, &reason)?;
            return Err(error);
        }
    };
    if !outer_attempt_is_still_current_running(ctx)? {
        if !(dynamic_result_is_successful_completion(&result)
            && try_restore_outer_attempt_running_for_dynamic_completion(ctx)?)
        {
            graph.nodes[index].status = DynamicNodeStatus::Paused;
            graph.nodes[index].outcome = None;
            graph.nodes[index].finished_at = Some(now_rfc3339_like());
            if outer_attempt_is_current_process_interrupted(ctx)? {
                pause_dynamic_graph(
                    ctx,
                    graph,
                    PauseReason::ProcessInterrupted,
                    "outer runtime attempt stopped before dynamic node result was accepted",
                )?;
            } else {
                mark_dynamic_graph_paused_in_memory(graph, PauseReason::ProcessInterrupted);
                append_dynamic_event(
                    ctx,
                    "dynamic_result_ignored_after_outer_attempt_stopped",
                    serde_json::json!({
                        "nodeId": graph.nodes[index].id,
                        "attemptId": dynamic_attempt_id(&graph.nodes[index]),
                    }),
                )?;
            }
            return Ok(());
        }
    }
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
        refresh_dynamic_current_leaf_ids(graph);
        if dynamic_graph_has_active_leaf(graph) {
            graph.run.updated_at = now_rfc3339_like();
            persist_dynamic_graph(ctx, graph)?;
            emit_dynamic_session_update_best_effort(
                ctx,
                &graph.nodes[index].id,
                &dynamic_attempt_id(&graph.nodes[index]),
            );
            return Ok(());
        }
        pause_dynamic_graph(
            ctx,
            graph,
            pause_reason,
            "dynamic node paused and no active dynamic leaf remains",
        )?;
        emit_dynamic_session_update_best_effort(
            ctx,
            &graph.nodes[index].id,
            &dynamic_attempt_id(&graph.nodes[index]),
        );
        return Ok(());
    }
    let mut accepted_any = false;
    let mut rejected_source_node_id = None;
    let mut visible_node_ids = Vec::new();
    for proposal in result.proposals {
        let source_index = graph
            .nodes
            .iter()
            .position(|node| node.id == proposal.source_node_id)
            .ok_or_else(|| {
                anyhow!(
                    "dynamic proposal source node `{}` missing",
                    proposal.source_node_id
                )
            })?;
        if proposal.validation_status == DynamicProposalValidationStatus::Rejected {
            rejected_source_node_id = Some(graph.nodes[source_index].id.clone());
            graph.proposals.push(proposal);
            continue;
        }
        let completion: DynamicNodeCompletion = serde_json::from_value(proposal.parsed.clone())?;
        accepted_any = true;
        graph.proposals.push(proposal.clone());
        visible_node_ids.extend(materialize_dynamic_next(
            ctx,
            graph,
            source_index,
            completion.next,
        )?);
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
            return Err(anyhow!(
                "dynamic proposal from `{source_node_id}` was rejected"
            ));
        }
    }
    graph.run.updated_at = now_rfc3339_like();
    persist_dynamic_graph(ctx, graph)?;
    emit_dynamic_session_update_best_effort(
        ctx,
        &graph.nodes[index].id,
        &dynamic_attempt_id(&graph.nodes[index]),
    );
    emit_dynamic_session_updates_best_effort(ctx, graph, &visible_node_ids);
    Ok(())
}

fn outer_attempt_is_still_current_running(ctx: &DynamicExecutionContext<'_>) -> Result<bool> {
    attempt_is_still_current_running(
        ctx.app,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    )
}

fn dynamic_result_is_successful_completion(result: &DynamicExecutionResult) -> bool {
    result.node.status == DynamicNodeStatus::Completed
        && result.node.outcome == Some(NodeOutcome::Success)
        && result
            .proposals
            .iter()
            .any(|proposal| proposal.validation_status == DynamicProposalValidationStatus::Accepted)
}

fn outer_attempt_is_current_process_interrupted(ctx: &DynamicExecutionContext<'_>) -> Result<bool> {
    let run: RunState = read_json(&ctx.app.paths.run_file(ctx.task_id, ctx.run_id))?;
    Ok(run.current_round.as_deref() == Some(ctx.round_id)
        && run.current_node.as_deref() == Some(ctx.outer_node_id)
        && run.current_attempt.as_deref() == Some(ctx.outer_attempt_id)
        && run.status == RunStatus::Paused
        && run.pause_reason == Some(PauseReason::ProcessInterrupted))
}

fn try_restore_outer_attempt_running_for_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
) -> Result<bool> {
    let mut run: RunState = read_json(&ctx.app.paths.run_file(ctx.task_id, ctx.run_id))?;
    if run.current_round.as_deref() != Some(ctx.round_id)
        || run.current_node.as_deref() != Some(ctx.outer_node_id)
        || run.current_attempt.as_deref() != Some(ctx.outer_attempt_id)
        || run.status != RunStatus::Paused
        || run.pause_reason != Some(PauseReason::ProcessInterrupted)
    {
        return Ok(false);
    }
    let mut round: RoundState = read_json(&ctx.app.paths.round_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
    ))?;
    let mut node: NodeState = read_json(&ctx.app.paths.node_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    ))?;
    if round.status != RunStatus::Paused || node.status != RunStatus::Paused {
        return Ok(false);
    }
    let now = now_rfc3339_like();
    run.status = RunStatus::Running;
    run.pause_reason = None;
    run.updated_at = now;
    round.status = RunStatus::Running;
    round.outcome = None;
    node.status = RunStatus::Running;
    node.outcome = None;
    node.finished_at = None;
    persist_runtime_state(ctx.app, ctx.task_id, &run, &round, &node)?;
    Ok(true)
}

fn mark_dynamic_graph_paused_in_memory(graph: &mut DynamicGraphState, pause_reason: PauseReason) {
    refresh_dynamic_current_leaf_ids(graph);
    graph.run.status = DynamicRunStatus::Paused;
    graph.run.outcome = None;
    graph.run.pause_reason = Some(pause_reason);
    graph.run.updated_at = now_rfc3339_like();
}

fn emit_dynamic_worker_completed(
    app: &App,
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
) {
    let attempt_dir = app
        .paths
        .dynamic_node_attempt_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &node.id,
            &dynamic_attempt_id(node),
        )
        .to_string();

    let outcome = match node.outcome {
        Some(NodeOutcome::Success) => "SUCCESS",
        _ => "FAILED",
    };

    app.lifecycle_bus
        .emit(RuntimeLifecycleEvent::NodeCompleted {
            task_id: ctx.task_id.to_string(),
            task_uuid: ctx.task_uuid.map(|s| s.to_string()),
            run_id: ctx.run_id.to_string(),
            run_uuid: ctx.run_uuid.map(|s| s.to_string()),
            round_id: ctx.round_id.to_string(),
            round_uuid: ctx.round_uuid.map(|s| s.to_string()),
            node_id: node.id.clone(),
            node_uuid: None,
            attempt_id: dynamic_attempt_id(node),
            repo_root: app.paths.repo_root.to_string(),
            seq: None,
            node_name: node.title.clone(),
            agent_type: node.provider.clone(),
            started_at: node.started_at.clone().unwrap_or_default(),
            finished_at: node.finished_at.clone(),
            outcome: outcome.to_string(),
            attempt_dir,
            suppress_sentinel: true,
        });
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
    task_uuid: Option<&str>,
    run_uuid: Option<&str>,
    round_uuid: Option<&str>,
    outer_node_uuid: Option<&str>,
    resume_override: Option<DynamicResumeOverride>,
) -> Result<DynamicExecutionResult> {
    append_dynamic_event_for_ids_best_effort(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        "dynamic_job_thread_started",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
        }),
    );
    let dynamic_run_path =
        app.paths
            .dynamic_run_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let graph_path =
        app.paths
            .dynamic_graph_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let state_lock =
        dynamic_state_lock_for(task_id, run_id, round_id, outer_node_id, outer_attempt_id)?;
    let state_load_started_at = Instant::now();
    let (run, mut graph): (DynamicRunState, DynamicGraphState) = {
        let _guard = state_lock
            .lock()
            .map_err(|_| anyhow!("dynamic state lock poisoned"))?;
        (read_json(&dynamic_run_path)?, read_json(&graph_path)?)
    };
    let state_load_elapsed_ms = elapsed_ms(state_load_started_at);
    let ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        dynamic,
        task_uuid,
        run_uuid,
        round_uuid,
        outer_node_uuid,
        resume_override,
    };
    dynamic_event_best_effort(
        &ctx,
        "dynamic_job_state_loaded",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": state_load_elapsed_ms,
            "state": dynamic_timing_data(&graph),
        }),
    );
    let index = graph
        .nodes
        .iter()
        .position(|candidate| candidate.id == node.id)
        .ok_or_else(|| anyhow!("dynamic node `{}` missing from graph", node.id))?;
    graph.run = run;
    graph.nodes[index] = node.clone();
    dynamic_event_best_effort(
        &ctx,
        "dynamic_job_kind_dispatch",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "workspaceMode": node.workspace.mode,
            "providerId": node.provider.clone(),
            "model": node.model.clone(),
        }),
    );

    let result = match node.kind {
        DynamicNodeKind::Worker => execute_dynamic_worker(&ctx, &graph, node),
        DynamicNodeKind::WorkflowInvocation => {
            execute_dynamic_workflow_invocation(&ctx, &graph, node)
        }
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => {
            execute_dynamic_agent_stage(&ctx, &graph, node)
        }
    }?;

    // Only emit NodeCompleted if the worker reached a terminal state
    // (not Paused — paused workers will be retried and emit fresh events).
    if result.node.status != DynamicNodeStatus::Paused {
        emit_dynamic_worker_completed(app, &ctx, &result.node);
    }

    Ok(result)
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
    dynamic_node_continue_ref(ctx, target, &self::dynamic_attempt_id(target))
}

fn execute_dynamic_worker(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    mut node: DynamicNodeState,
) -> Result<DynamicExecutionResult> {
    let workspace_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_workspace_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "workspaceMode": node.workspace.mode,
            "workspacePath": node.workspace_path.clone(),
        }),
    );
    ensure_dynamic_workspace(ctx, &mut node)?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_workspace_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(workspace_started_at),
            "workspaceMode": node.workspace.mode,
            "workspacePath": node.workspace_path.clone(),
        }),
    );
    let attempt_id = dynamic_attempt_id(&node);
    let attempt_dirs_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_attempt_dirs_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "attemptId": attempt_id,
        }),
    );
    prepare_dynamic_attempt_dirs(ctx, &node, &attempt_id)?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_attempt_dirs_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "attemptId": attempt_id,
            "elapsedMs": elapsed_ms(attempt_dirs_started_at),
        }),
    );
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
    let continue_ref_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_continue_ref_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "configuredSessionMode": node.session_mode,
            "continueFromNodeId": node.continue_from_node_id,
        }),
    );
    let mut continue_ref = match node.session_mode {
        SessionMode::Continue => node
            .continue_from_node_id
            .as_deref()
            .and_then(|source_node_id| {
                dynamic_continue_ref_for_source_node(ctx, graph, source_node_id)
            }),
        SessionMode::New => dynamic_node_continue_ref(ctx, &node, &attempt_id),
    };
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_continue_ref_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(continue_ref_started_at),
            "hasContinueRef": continue_ref.is_some(),
        }),
    );
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
    let mut resume_prompt_id = None;
    let mut resume_input_attachment_paths = Vec::new();
    if let Some(resume) = ctx
        .resume_override
        .as_ref()
        .filter(|resume| resume.node_id == node.id && resume.attempt_id == attempt_id)
    {
        let Some(saved_continue_ref) = dynamic_node_continue_ref(ctx, &node, &attempt_id) else {
            bail!("dynamic node `{}` has no ACP continue reference", node.id);
        };
        continue_ref = Some(saved_continue_ref);
        session_mode = SessionMode::Continue;
        resume_prompt = Some(resume.prompt.clone());
        resume_prompt_id = resume.prompt_id.clone();
        resume_input_attachment_paths = resume.attachment_paths.clone();
    }
    let mut resume_prompt_visibility = PromptVisibility::Visible;
    let mut proposals = Vec::new();

    loop {
        let live_update_context = dynamic_acp_live_event_context(ctx, &node.id, &attempt_id);
        let live_update = ctx.app.acp_live_update_for(live_update_context.clone());
        let session_update = ctx.app.acp_session_update_for(live_update_context);
        let invocation_build_started_at = Instant::now();
        dynamic_event_best_effort(
            ctx,
            "dynamic_worker_invocation_build_begin",
            serde_json::json!({
                "nodeId": node.id,
                "kind": node.kind,
                "sessionMode": session_mode,
                "providerId": provider_id,
                "model": node.model.clone(),
                "repairPromptCount": proposal_repair_prompts,
            }),
        );
        let output_contract_started_at =
            dynamic_invocation_build_step_begin(ctx, &node, &attempt_id, "output_contract");
        let output_contract = dynamic_output_contract(ctx, graph);
        dynamic_invocation_build_step_end(
            ctx,
            &node,
            &attempt_id,
            "output_contract",
            output_contract_started_at,
            serde_json::json!({
                "artifact": output_contract.artifact,
                "kind": output_contract.kind,
                "hasSchema": output_contract.schema.is_some(),
                "schemaTextBytes": output_contract
                    .schema_text
                    .as_ref()
                    .map(|value| value.len())
                    .unwrap_or(0),
            }),
        );
        let invocation = build_dynamic_worker_invocation(
            ctx,
            graph,
            &node,
            &attempt_id,
            Some(output_contract),
            session_mode,
            continue_ref.clone(),
            resume_prompt.take(),
            resume_prompt_id.take(),
            resume_prompt_visibility,
            resume_input_attachment_paths.clone(),
        )
        .map_err(|error| {
            anyhow!(
                "failed to build dynamic worker invocation for `{}`: {error}",
                node.id
            )
        })?;
        dynamic_event_best_effort(
            ctx,
            "dynamic_worker_invocation_build_end",
            serde_json::json!({
                "nodeId": node.id,
                "kind": node.kind,
                "elapsedMs": elapsed_ms(invocation_build_started_at),
                "sessionMode": session_mode,
                "providerId": provider_id,
                "model": node.model.clone(),
                "repairPromptCount": proposal_repair_prompts,
            }),
        );
        append_dynamic_event(
            ctx,
            "dynamic_node_started",
            serde_json::json!({
                "nodeId": node.id,
                "kind": node.kind,
                "sessionMode": session_mode,
            }),
        )
        .map_err(|error| {
            anyhow!(
                "failed to append dynamic start event for `{}`: {error}",
                node.id
            )
        })?;
        let provider_started_at = Instant::now();
        dynamic_event_best_effort(
            ctx,
            "dynamic_worker_provider_begin",
            serde_json::json!({
                "nodeId": node.id,
                "kind": node.kind,
                "sessionMode": session_mode,
                "providerId": provider_id,
                "model": node.model.clone(),
                "repairPromptCount": proposal_repair_prompts,
            }),
        );
        let provider = ctx.app.provider_for_id(&provider_id).map_err(|error| {
            anyhow!(
                "failed to resolve provider `{}` for `{}`: {error}",
                provider_id,
                node.id
            )
        })?;
        let provider_resolve_elapsed_ms = elapsed_ms(provider_started_at);
        let result = provider
            .run_worker_with_callbacks(
                invocation,
                live_update.as_ref().map(|callback| callback as _),
                session_update.as_ref().map(|callback| callback as _),
            )
            .map_err(|error| {
                anyhow!(
                    "provider `{}` failed to run `{}`: {error}",
                    provider_id,
                    node.id
                )
            })?;
        dynamic_event_best_effort(
            ctx,
            "dynamic_worker_provider_end",
            serde_json::json!({
                "nodeId": node.id,
                "kind": node.kind,
                "elapsedMs": elapsed_ms(provider_started_at),
                "providerResolveElapsedMs": provider_resolve_elapsed_ms,
                "sessionMode": session_mode,
                "providerId": provider_id,
                "model": node.model.clone(),
                "repairPromptCount": proposal_repair_prompts,
            }),
        );
        let provider_status = result.status;
        finalize_dynamic_worker_result(ctx, &mut node, &attempt_id, result)?;
        if provider_status == ProviderRunStatus::Interrupted
            && let Some(proposal) =
                try_accept_interrupted_dynamic_completion(ctx, &mut node, &attempt_id)?
        {
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
        if node.status == DynamicNodeStatus::Paused {
            return Ok(DynamicExecutionResult {
                node,
                proposals: Vec::new(),
            });
        }
        if node.outcome != Some(NodeOutcome::Success) {
            if !outer_attempt_is_still_current_running(ctx)? {
                node.status = DynamicNodeStatus::Paused;
                node.outcome = None;
                node.finished_at = Some(now_rfc3339_like());
                return Ok(DynamicExecutionResult {
                    node,
                    proposals: Vec::new(),
                });
            }
            bail!("dynamic worker `{}` failed", node.id);
        }
        if !outer_attempt_is_still_current_running(ctx)?
            && let Some(proposal) =
                try_accept_interrupted_dynamic_completion(ctx, &mut node, &attempt_id)?
        {
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
        if !outer_attempt_is_still_current_running(ctx)? {
            node.status = DynamicNodeStatus::Paused;
            node.outcome = None;
            node.finished_at = Some(now_rfc3339_like());
            return Ok(DynamicExecutionResult {
                node,
                proposals: Vec::new(),
            });
        }
        match build_dynamic_completion_from_artifact(ctx, &attempt_id, &node) {
            Ok(proposal)
                if proposal.validation_status == DynamicProposalValidationStatus::Accepted =>
            {
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
                resume_prompt = Some(dynamic_proposal_repair_prompt(
                    ctx,
                    graph,
                    &node,
                    &validation_errors,
                ));
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
                let schema_validation_errors = err
                    .downcast_ref::<DynamicCompletionSchemaValidationError>()
                    .map(|error| error.errors.clone());
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
                        "validationErrors": schema_validation_errors.clone(),
                    }),
                )?;
                session_mode = SessionMode::Continue;
                continue_ref = Some(repair_continue_ref);
                resume_prompt = Some(match schema_validation_errors {
                    Some(errors) => dynamic_structured_repair_prompt(ctx, graph, &node, &errors),
                    None => dynamic_text_repair_prompt(ctx, graph, &node, err.to_string()),
                });
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
    let workspace_started_at = Instant::now();
    node.workspace_path = Some(ctx.app.paths.repo_root.clone());
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_workspace_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(workspace_started_at),
            "workspaceMode": node.workspace.mode,
            "workspacePath": node.workspace_path.clone(),
        }),
    );
    let attempt_id = dynamic_attempt_id(&node);
    let attempt_dirs_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_attempt_dirs_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "attemptId": attempt_id,
        }),
    );
    prepare_dynamic_attempt_dirs(ctx, &node, &attempt_id)?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_attempt_dirs_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "attemptId": attempt_id,
            "elapsedMs": elapsed_ms(attempt_dirs_started_at),
        }),
    );
    let continue_ref_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_continue_ref_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "configuredSessionMode": node.session_mode,
        }),
    );
    let continue_ref = dynamic_node_continue_ref(ctx, &node, &attempt_id);
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_continue_ref_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(continue_ref_started_at),
            "hasContinueRef": continue_ref.is_some(),
        }),
    );
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
    let live_update = ctx.app.acp_live_update_for(live_update_context.clone());
    let session_update = ctx.app.acp_session_update_for(live_update_context);
    let invocation_build_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_invocation_build_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "sessionMode": session_mode,
            "providerId": node.provider.clone(),
            "model": node.model.clone(),
        }),
    );
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
        Vec::new(),
    )?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_invocation_build_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(invocation_build_started_at),
            "sessionMode": session_mode,
            "providerId": node.provider.clone(),
            "model": node.model.clone(),
        }),
    );
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
    let provider_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_provider_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "sessionMode": session_mode,
            "providerId": provider_id,
            "model": node.model.clone(),
        }),
    );
    let provider = ctx.app.provider_for_id(provider_id)?;
    let provider_resolve_elapsed_ms = elapsed_ms(provider_started_at);
    let result = provider.run_worker_with_callbacks(
        invocation,
        live_update.as_ref().map(|callback| callback as _),
        session_update.as_ref().map(|callback| callback as _),
    )?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_provider_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(provider_started_at),
            "providerResolveElapsedMs": provider_resolve_elapsed_ms,
            "sessionMode": session_mode,
            "providerId": provider_id,
            "model": node.model.clone(),
        }),
    );
    if !outer_attempt_is_still_current_running(ctx)? {
        node.status = DynamicNodeStatus::Paused;
        node.outcome = None;
        node.finished_at = Some(now_rfc3339_like());
        return Ok(DynamicExecutionResult {
            node,
            proposals: Vec::new(),
        });
    }
    finalize_dynamic_worker_result(ctx, &mut node, &attempt_id, result)?;
    if node.status == DynamicNodeStatus::Paused {
        return Ok(DynamicExecutionResult {
            node,
            proposals: Vec::new(),
        });
    }
    teardown_dynamic_workspace_best_effort(ctx, &node);
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
        Some(child_run_id) => ctx
            .app
            .run_continue(ctx.task_id, child_run_id, None, None)?,
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
            _ => {
                teardown_dynamic_workspace_best_effort(ctx, &node);
                bail!("child workflow invocation `{}` failed", node.id);
            }
        }
    }
    teardown_dynamic_workspace_best_effort(ctx, &node);
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
        None,
        Vec::new(),
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
    let status = result.status;
    node.finished_at = Some(now_rfc3339_like());
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
    if let Some(payload) = result.result_payload
        && let Some(output_artifact) = payload.output_artifact
    {
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
    match status {
        ProviderRunStatus::Success => {
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

fn try_accept_interrupted_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
    attempt_id: &str,
) -> Result<Option<DynamicProposalState>> {
    let proposal = match build_dynamic_completion_from_artifact(ctx, attempt_id, node) {
        Ok(proposal) if proposal.validation_status == DynamicProposalValidationStatus::Accepted => {
            proposal
        }
        Ok(proposal) => {
            append_dynamic_event(
                ctx,
                "dynamic_interrupted_completion_ignored",
                serde_json::json!({
                    "nodeId": node.id,
                    "attemptId": attempt_id,
                    "validationStatus": proposal.validation_status,
                    "validationErrors": proposal.validation_errors,
                }),
            )?;
            return Ok(None);
        }
        Err(error) => {
            append_dynamic_event(
                ctx,
                "dynamic_interrupted_completion_ignored",
                serde_json::json!({
                    "nodeId": node.id,
                    "attemptId": attempt_id,
                    "error": error.to_string(),
                }),
            )?;
            return Ok(None);
        }
    };
    node.status = DynamicNodeStatus::Completed;
    node.outcome = Some(NodeOutcome::Success);
    node.finished_at = Some(now_rfc3339_like());
    validate_dynamic_node_state(node)?;
    append_dynamic_event(
        ctx,
        "dynamic_interrupted_completion_accepted",
        serde_json::json!({
            "nodeId": node.id,
            "attemptId": attempt_id,
            "proposalId": proposal.id,
        }),
    )?;
    Ok(Some(proposal))
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
    let graph: DynamicGraphState = {
        let lock = dynamic_state_lock(ctx)?;
        let _guard = lock
            .lock()
            .map_err(|_| anyhow!("dynamic state lock poisoned"))?;
        read_json(&ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))?
    };
    let raw = std::fs::read_to_string(artifact_path.as_std_path())?;
    let (completion, parsed, schema_errors) = parse_dynamic_completion_artifact(ctx, &graph, &raw)?;
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
    build_dynamic_completion_proposal(
        ctx,
        node,
        completion,
        Some(artifact_path),
        Some(raw_output_path),
        Some(parsed),
        schema_errors,
    )
}

fn parse_dynamic_completion_artifact(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    raw: &str,
) -> Result<(
    DynamicNodeCompletion,
    serde_json::Value,
    Vec<DynamicProposalValidationError>,
)> {
    let parsed: serde_json::Value = parse_json_artifact(raw)?;
    let schema_errors = validate_dynamic_completion_schema(ctx, graph, &parsed)?;
    let completion: DynamicNodeCompletion = serde_path_to_error::deserialize(parsed.clone())
        .map_err(|err| {
            if !schema_errors.is_empty() {
                return DynamicCompletionSchemaValidationError {
                    errors: schema_errors.clone(),
                }
                .into();
            }
            let path = err.path().to_string();
            let path = if path == "." { "$".to_string() } else { path };
            let path = refine_dynamic_parse_error_path(&parsed, &path, &err.inner().to_string());
            anyhow!(
                "failed to parse dynamic-node-completion at JSON path `{}`: {}",
                path,
                err.inner()
            )
        })?;
    Ok((completion, parsed, schema_errors))
}

fn refine_dynamic_parse_error_path(
    parsed: &serde_json::Value,
    path: &str,
    message: &str,
) -> String {
    let Some(field) = missing_field_from_serde_message(message) else {
        return path.to_string();
    };
    if path != "next" {
        return format!("{path}.{field}");
    }
    let Some(next) = parsed.get("next").and_then(|value| value.as_object()) else {
        return format!("{path}.{field}");
    };
    match next.get("type").and_then(|value| value.as_str()) {
        Some("single") => next
            .get("node")
            .and_then(|value| value.as_object())
            .filter(|object| !object.contains_key(field))
            .map(|_| format!("next.node.{field}"))
            .unwrap_or_else(|| format!("{path}.{field}")),
        Some("fanout") => {
            for stage in ["merge", "acceptance"] {
                if next
                    .get(stage)
                    .and_then(|value| value.as_object())
                    .filter(|object| !object.contains_key(field))
                    .is_some()
                {
                    return format!("next.{stage}.{field}");
                }
            }
            if let Some(index) = next
                .get("nodes")
                .and_then(|value| value.as_array())
                .and_then(|nodes| {
                    nodes.iter().position(|node| {
                        node.as_object()
                            .map(|object| !object.contains_key(field))
                            .unwrap_or(false)
                    })
                })
            {
                return format!("next.nodes[{index}].{field}");
            }
            format!("{path}.{field}")
        }
        _ => format!("{path}.{field}"),
    }
}

fn missing_field_from_serde_message(message: &str) -> Option<&str> {
    message
        .split("missing field `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .filter(|field| !field.trim().is_empty())
}

#[derive(Debug, thiserror::Error)]
#[error("dynamic-node-completion schema validation failed")]
struct DynamicCompletionSchemaValidationError {
    errors: Vec<DynamicProposalValidationError>,
}

fn validate_dynamic_completion_schema(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    parsed: &serde_json::Value,
) -> Result<Vec<DynamicProposalValidationError>> {
    let schema = dynamic_effective_completion_schema(ctx, graph);
    let compiled = compiled_dynamic_completion_schema(&schema)?;
    let errors = match compiled.validate(parsed) {
        Ok(()) => Vec::new(),
        Err(errors) => errors
            .map(|error| dynamic_schema_validation_error(parsed, error))
            .collect::<Vec<_>>(),
    };
    Ok(dedupe_dynamic_validation_errors(errors))
}

fn compiled_dynamic_completion_schema(schema: &serde_json::Value) -> Result<Arc<JSONSchema>> {
    let key = serde_json::to_string(schema)?;
    let cache = DYNAMIC_COMPLETION_SCHEMA_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(compiled) = cache.lock().unwrap().get(&key).cloned() {
        return Ok(compiled);
    }
    let compiled =
        Arc::new(JSONSchema::compile(schema).map_err(|error| {
            anyhow!("failed to compile dynamic-node-completion schema: {error}")
        })?);
    cache.lock().unwrap().insert(key, compiled.clone());
    Ok(compiled)
}

fn dedupe_dynamic_validation_errors(
    errors: Vec<DynamicProposalValidationError>,
) -> Vec<DynamicProposalValidationError> {
    let mut seen = HashSet::new();
    errors
        .into_iter()
        .filter(|error| {
            seen.insert(format!(
                "{}|{}|{}",
                error.code,
                error.path.as_deref().unwrap_or_default(),
                error.message
            ))
        })
        .collect()
}

fn dynamic_schema_validation_error(
    root: &serde_json::Value,
    error: ValidationError<'_>,
) -> DynamicProposalValidationError {
    let base_path = json_pointer_to_dynamic_path(&error.instance_path.to_string());
    let schema_path = error.schema_path.to_string();
    let mut code = "dynamic.schema.invalid".to_string();
    let mut path = base_path.clone();
    let mut expected = "valid value for dynamic-node-completion schema".to_string();
    let mut allowed_values = Vec::new();
    let mut actual = schema_actual_value(&error.instance);
    let mut message = match &error.kind {
        ValidationErrorKind::Required { property } => {
            code = "dynamic.schema.required".to_string();
            let property = property
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| property.to_string());
            path = append_dynamic_path(&base_path, &property);
            actual = Some("missing".to_string());
            expected = "required field".to_string();
            format!("required field `{property}` is missing")
        }
        ValidationErrorKind::AdditionalProperties { unexpected }
        | ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            code = "dynamic.schema.additional-property".to_string();
            let property = unexpected
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            path = append_dynamic_path(&base_path, &property);
            actual = value_at_dynamic_path(root, &path).and_then(json_param_string);
            expected = "omit this field".to_string();
            format!("field `{property}` is not allowed here")
        }
        ValidationErrorKind::FalseSchema => {
            code = "dynamic.schema.forbidden-field".to_string();
            expected = "omit this field".to_string();
            format!("field at `{path}` is forbidden by the dynamic-node-completion schema")
        }
        ValidationErrorKind::Enum { options } => {
            code = "dynamic.schema.enum".to_string();
            allowed_values = options
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            expected = if allowed_values.is_empty() {
                "one of the schema enum values".to_string()
            } else {
                format!("one of: {}", allowed_values.join(", "))
            };
            format!("value at `{path}` is not one of the allowed schema values")
        }
        ValidationErrorKind::MaxItems { limit } => {
            code = "dynamic.schema.max-items".to_string();
            expected = format!("at most {limit} items");
            format!("array at `{path}` has too many items")
        }
        ValidationErrorKind::MinItems { limit } => {
            code = "dynamic.schema.min-items".to_string();
            expected = format!("at least {limit} items");
            format!("array at `{path}` has too few items")
        }
        ValidationErrorKind::Type { kind } => {
            code = "dynamic.schema.type".to_string();
            expected = format!("{kind:?}");
            format!("value at `{path}` has the wrong type")
        }
        ValidationErrorKind::OneOfNotValid
        | ValidationErrorKind::OneOfMultipleValid
        | ValidationErrorKind::AnyOf => {
            code = "dynamic.schema.branch".to_string();
            expected = "one valid dynamic-node-completion branch".to_string();
            format!("value at `{path}` does not match the expected schema branch")
        }
        _ => format!("{error}"),
    };
    if code == "dynamic.schema.max-items" && path == "next.nodes" {
        code = "dynamic.fanout.max-fanout-exceeded".to_string();
        message = "dynamic fanout exceeds maxFanout".to_string();
    } else if matches!(
        code.as_str(),
        "dynamic.schema.forbidden-field" | "dynamic.schema.additional-property"
    ) && path == "next.merge.profile"
    {
        code = "dynamic.merge.profile.unsupported".to_string();
        message = "dynamic merge must not output profile; runtime uses the built-in AI-DYNAMIC merge prompt".to_string();
    } else if matches!(
        code.as_str(),
        "dynamic.schema.forbidden-field" | "dynamic.schema.additional-property"
    ) && path == "next.acceptance.profile"
    {
        code = "dynamic.acceptance.profile.unsupported".to_string();
        message = "dynamic acceptance must not output profile; runtime uses the built-in AI-DYNAMIC acceptance prompt".to_string();
    }
    let mut validation_error = dynamic_validation_error(
        &code,
        message,
        serde_json::json!({
            "path": path,
            "actual": actual,
            "expected": expected,
            "schemaPath": schema_path,
        }),
    );
    validation_error.path = Some(path);
    validation_error.actual = actual;
    validation_error.expected = Some(expected);
    validation_error.allowed_values = allowed_values;
    if validation_error.expected.as_deref() == Some("omit this field") {
        validation_error.suggestion = Some("remove this field from the JSON output".to_string());
    }
    validation_error
}

fn schema_actual_value(value: &Cow<'_, serde_json::Value>) -> Option<String> {
    json_param_string(value.as_ref()).or_else(|| Some(value.as_ref().to_string()))
}

fn json_pointer_to_dynamic_path(pointer: &str) -> String {
    if pointer.is_empty() || pointer == "/" {
        return "$".to_string();
    }
    let mut path = String::new();
    for segment in pointer.trim_start_matches('/').split('/') {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        if segment.chars().all(|ch| ch.is_ascii_digit()) {
            path.push('[');
            path.push_str(&segment);
            path.push(']');
        } else {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(&segment);
        }
    }
    if path.is_empty() {
        "$".to_string()
    } else {
        path
    }
}

fn append_dynamic_path(base: &str, field: &str) -> String {
    if base == "$" || base.is_empty() {
        field.to_string()
    } else {
        format!("{base}.{field}")
    }
}

fn value_at_dynamic_path<'a>(
    root: &'a serde_json::Value,
    dynamic_path: &str,
) -> Option<&'a serde_json::Value> {
    if dynamic_path == "$" {
        return Some(root);
    }
    let mut value = root;
    for raw_segment in dynamic_path.split('.') {
        let mut segment = raw_segment;
        loop {
            if let Some(index_start) = segment.find('[') {
                let field = &segment[..index_start];
                if !field.is_empty() {
                    value = value.get(field)?;
                }
                let index_end = segment[index_start + 1..].find(']')? + index_start + 1;
                let index = segment[index_start + 1..index_end].parse::<usize>().ok()?;
                value = value.get(index)?;
                segment = &segment[index_end + 1..];
                if segment.is_empty() {
                    break;
                }
            } else {
                value = value.get(segment)?;
                break;
            }
        }
    }
    Some(value)
}

fn build_dynamic_completion_proposal(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    completion: DynamicNodeCompletion,
    artifact_path: Option<Utf8PathBuf>,
    raw_output_path: Option<Utf8PathBuf>,
    parsed_override: Option<serde_json::Value>,
    pre_validation_errors: Vec<DynamicProposalValidationError>,
) -> Result<DynamicProposalState> {
    let graph: DynamicGraphState = {
        let lock = dynamic_state_lock(ctx)?;
        let _guard = lock
            .lock()
            .map_err(|_| anyhow!("dynamic state lock poisoned"))?;
        read_json(&ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))?
    };
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
    let parsed = match parsed_override {
        Some(parsed) => parsed,
        None => serde_json::to_value(&completion)?,
    };
    let mut validation_errors = pre_validation_errors;
    validation_errors.extend(validate_dynamic_completion(ctx, &graph, index, &completion));
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
            errors.extend(validate_dynamic_agent_task_spec(
                ctx,
                acceptance,
                "acceptance",
            ));
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
                errors.extend(validate_dynamic_node_spec(
                    ctx,
                    graph,
                    source,
                    node,
                    nodes.len(),
                ));
            }
        }
    }
    errors
}

fn validate_dynamic_permission_mode(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
    normative_mode: &str,
    make_error: impl FnOnce(&str) -> DynamicProposalValidationError,
) -> Option<DynamicProposalValidationError> {
    let doctor = ctx.app.provider_doctor(provider).ok()?;
    let resolved = ctx
        .app
        .config
        .resolve_permission_mode(provider, normative_mode);
    let supported = doctor.supported_modes();
    let supported_ids: Vec<_> = supported.into_iter().map(|m| m.id).collect();
    if !supported_ids.is_empty() && !supported_ids.iter().any(|id| id == &resolved) {
        Some(make_error(&resolved))
    } else {
        None
    }
}

fn normalized_dynamic_provider(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn dynamic_fixed_provider(dynamic: &AiDynamicNode) -> Option<&str> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, .. } => Some(provider.as_str()),
        AiDynamicAgentStrategy::Dynamic { .. } => None,
    }
}

fn dynamic_resolved_proposal_provider<'a>(
    ctx: &'a DynamicExecutionContext<'_>,
    proposed: Option<&'a str>,
) -> Option<&'a str> {
    dynamic_fixed_provider(ctx.dynamic).or_else(|| normalized_dynamic_provider(proposed))
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
    if spec.workspace.mode == WorkspaceMode::Worktree {
        let capability = dynamic_workspace_capability(ctx);
        if !capability.supports_worktree {
            let mut error = dynamic_validation_error(
                "dynamic.node.workspace.worktree-git-required",
                format!(
                    "dynamic node `{}` cannot use workspace.mode=worktree because the current workspace cannot create git worktrees",
                    spec.id
                ),
                serde_json::json!({
                    "nodeId": spec.id,
                    "field": "workspace.mode",
                    "actual": "worktree",
                    "expected": "readonly or main",
                    "workspacePath": ctx.app.paths.repo_root,
                    "reasonCode": capability.reason_code,
                    "reason": capability.reason,
                }),
            );
            error.allowed_values = vec!["readonly".to_string(), "main".to_string()];
            error.suggestion = Some(
                "replace writable parallel fan-out with serial main workspace work, or ask the user to initialize Git before using worktree fan-out".to_string(),
            );
            errors.push(error);
        }
    }
    for dependency in &spec.depends_on {
        if !graph.nodes.iter().any(|node| node.id == *dependency) {
            errors.push(dynamic_validation_error(
                "dynamic.node.depends-on.unknown",
                format!(
                    "dynamic node `{}` depends on unknown node `{dependency}`",
                    spec.id
                ),
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
                    format!(
                        "dynamic node `{}` cannot set continueFromNodeId when session is new",
                        spec.id
                    ),
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
                    format!(
                        "workflow invocation `{}` cannot use continue session",
                        spec.id
                    ),
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
                    if dynamic_node_continue_ref(ctx, target, &self::dynamic_attempt_id(target))
                        .is_none()
                    {
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
                        if let Some(provider) =
                            dynamic_resolved_proposal_provider(ctx, spec.provider.as_deref())
                        {
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
                    format!(
                        "dynamic node `{}` cannot continue from `{}`",
                        spec.id, continue_from_node_id
                    ),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "continueFromNodeId": continue_from_node_id,
                    }),
                )),
            }
        }
    }
    match spec.kind {
        DynamicNodeSpecKind::Worker => {
            let proposed_provider = normalized_dynamic_provider(spec.provider.as_deref());
            if dynamic_fixed_provider(ctx.dynamic).is_some() && proposed_provider.is_some() {
                errors.push(dynamic_validation_error(
                    "dynamic.node.provider.unsupported",
                    format!(
                        "dynamic worker `{}` must not output provider under fixed agent strategy",
                        spec.id
                    ),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "field": "provider",
                        "provider": proposed_provider.unwrap(),
                        "expected": "omit this field",
                    }),
                ));
            }
            match dynamic_resolved_proposal_provider(ctx, spec.provider.as_deref()) {
                Some(provider) => {
                    if ctx.app.provider_for_id(provider).is_err() {
                        errors.push(dynamic_validation_error(
                            "dynamic.node.provider.unknown",
                            format!(
                                "dynamic worker `{}` references unknown provider `{provider}`",
                                spec.id
                            ),
                            serde_json::json!({
                                "nodeId": spec.id,
                                "provider": provider,
                            }),
                        ));
                    } else if let Some(normative_mode) = ctx.dynamic.permission_mode() {
                        if let Some(error) = validate_dynamic_permission_mode(
                            ctx,
                            provider,
                            normative_mode,
                            |resolved| {
                                dynamic_validation_error(
                                    "dynamic.node.permission-mode.unsupported",
                                    format!(
                                        "dynamic worker `{}` permissionMode `{}` (resolved to `{}`) is not supported by provider `{provider}`",
                                        spec.id, normative_mode, resolved
                                    ),
                                    serde_json::json!({
                                        "nodeId": spec.id,
                                        "provider": provider,
                                        "permissionMode": normative_mode,
                                    }),
                                )
                            },
                        ) {
                            errors.push(error);
                        }
                    }
                    if dynamic_worker_model_required_from_proposal(ctx, provider)
                        && spec
                            .model
                            .as_deref()
                            .map(str::trim)
                            .filter(|model| !model.is_empty())
                            .is_none()
                    {
                        errors.push(dynamic_validation_error(
                        "dynamic.node.model.required",
                        format!("dynamic worker `{}` must output model for provider `{provider}` because the AI-DYNAMIC config did not lock one", spec.id),
                        serde_json::json!({
                            "nodeId": spec.id,
                            "provider": provider,
                            "field": "model",
                        }),
                    ));
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
                None => errors.push(dynamic_validation_error(
                    "dynamic.node.provider.blank",
                    format!("dynamic worker `{}` provider cannot be blank", spec.id),
                    serde_json::json!({
                        "nodeId": spec.id,
                        "field": "provider",
                    }),
                )),
            }
        }
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
                    format!(
                        "workflow invocation `{}` workflowId cannot be blank",
                        spec.id
                    ),
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
    let proposed_provider = normalized_dynamic_provider(Some(spec.provider.as_str()));
    if dynamic_fixed_provider(ctx.dynamic).is_some() && proposed_provider.is_some() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.provider.unsupported"),
            format!("dynamic {name} must not output provider under fixed agent strategy"),
            serde_json::json!({
                "field": "provider",
                "stage": name,
                "provider": proposed_provider.unwrap(),
                "expected": "omit this field",
            }),
        ));
    }
    let resolved_provider = dynamic_resolved_proposal_provider(ctx, Some(spec.provider.as_str()));
    if let Some(provider) = resolved_provider {
        if ctx.app.provider_for_id(provider).is_err() {
            errors.push(dynamic_validation_error(
                &format!("dynamic.{name}.provider.unknown"),
                format!("dynamic {name} references unknown provider `{provider}`"),
                serde_json::json!({
                    "provider": provider,
                    "stage": name,
                }),
            ));
        } else if let Some(normative_mode) = ctx.dynamic.permission_mode() {
            if let Some(error) = validate_dynamic_permission_mode(
                ctx,
                provider,
                normative_mode,
                |resolved| {
                    dynamic_validation_error(
                        &format!("dynamic.{name}.permission-mode.unsupported"),
                        format!(
                            "dynamic {name} permissionMode `{}` (resolved to `{}`) is not supported by provider `{provider}`",
                            normative_mode, resolved
                        ),
                        serde_json::json!({
                            "provider": provider,
                            "stage": name,
                            "permissionMode": normative_mode,
                        }),
                    )
                },
            ) {
                errors.push(error);
            }
        }
    } else {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.provider.blank"),
            format!("dynamic {name} provider cannot be blank"),
            serde_json::json!({
                "field": "provider",
                "stage": name,
            }),
        ));
    }
    let proposed_model = spec
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(model) = proposed_model
        && dynamic_acceptance_model(ctx.dynamic).is_some()
    {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.model.unsupported"),
            format!(
                "dynamic {name} must not output model because AI-DYNAMIC configured acceptanceModel"
            ),
            serde_json::json!({
                "provider": resolved_provider,
                "stage": name,
                "field": "model",
                "actual": model,
                "expected": "omit this field",
            }),
        ));
    } else if let Some(provider) = resolved_provider
        && dynamic_agent_task_model_required_from_proposal(ctx, provider)
        && proposed_model.is_none()
    {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.model.required"),
            format!("dynamic {name} must output model for provider `{provider}` because the AI-DYNAMIC config did not lock one"),
            serde_json::json!({
                "provider": provider,
                "stage": name,
                "field": "model",
            }),
        ));
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

fn dynamic_agent_task_spec_with_resolved_provider(
    ctx: &DynamicExecutionContext<'_>,
    mut spec: DynamicAgentTaskSpec,
) -> Result<DynamicAgentTaskSpec> {
    spec.provider = dynamic_resolved_proposal_provider(ctx, Some(spec.provider.as_str()))
        .ok_or_else(|| anyhow!("dynamic agent task provider was not resolved"))?
        .to_string();
    spec.model = dynamic_acceptance_model(ctx.dynamic)
        .map(ToOwned::to_owned)
        .or_else(|| {
            spec.model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string)
        });
    Ok(spec)
}

fn materialize_dynamic_next(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    source_index: usize,
    next: DynamicNext,
) -> Result<Vec<String>> {
    let mut visible_node_ids = Vec::new();
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
                ctx,
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
            let new_node_id = new_node.id.clone();
            graph.nodes.push(new_node);
            visible_node_ids.push(new_node_id);
        }
        DynamicNext::Fanout {
            group_id,
            nodes,
            merge,
            acceptance,
        } => {
            let source = graph.nodes[source_index].clone();
            let merge = dynamic_agent_task_spec_with_resolved_provider(ctx, merge)?;
            let acceptance = dynamic_agent_task_spec_with_resolved_provider(ctx, acceptance)?;
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
                    ctx,
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
                let new_node_id = new_node.id.clone();
                graph.nodes.push(new_node);
                visible_node_ids.push(new_node_id);
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
    let promoted_node_ids = refresh_dynamic_ready_nodes(graph);
    visible_node_ids.retain(|node_id| promoted_node_ids.iter().any(|promoted| promoted == node_id));
    graph.run.updated_at = now_rfc3339_like();
    Ok(visible_node_ids)
}

fn dynamic_node_state_from_spec(
    ctx: &DynamicExecutionContext<'_>,
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
    let provider = match kind {
        DynamicNodeKind::Worker => {
            dynamic_resolved_proposal_provider(ctx, spec.provider.as_deref()).map(ToOwned::to_owned)
        }
        DynamicNodeKind::WorkflowInvocation => None,
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => unreachable!(),
    };
    let workflow_snapshot_id = spec.workflow_id.as_ref().and_then(|workflow_id| {
        graph
            .run
            .allowed_workflow_snapshots
            .iter()
            .find(|snapshot| snapshot.workflow_id == *workflow_id)
            .map(|snapshot| snapshot.snapshot_id.clone())
    });
    let model = provider
        .as_deref()
        .and_then(|provider| dynamic_model_for_provider(ctx.dynamic, provider))
        .or(spec.model.clone());
    let resolved_permission_mode = provider
        .as_deref()
        .and_then(|provider| {
            inherited_permission_mode
                .as_deref()
                .map(|mode| ctx.app.config.resolve_permission_mode(provider, mode))
        })
        .or(inherited_permission_mode);
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
        provider,
        profile: spec.profile,
        model,
        permission_mode: resolved_permission_mode,
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

fn refresh_dynamic_ready_nodes(graph: &mut DynamicGraphState) -> Vec<String> {
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
        .filter(|node| dynamic_leaf_is_active(node.status))
        .count();
    let mut available_slots =
        (graph.run.control.max_parallel as usize).saturating_sub(occupied_slots);
    let mut promoted_node_ids = Vec::new();
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
            promoted_node_ids.push(graph.nodes[index].id.clone());
            available_slots -= 1;
        }
    }
    refresh_dynamic_current_leaf_ids(graph);
    promoted_node_ids
}

struct DynamicGroupAdvanceResult {
    changed: bool,
}

fn advance_dynamic_groups(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> Result<DynamicGroupAdvanceResult> {
    let mut changed = false;
    let mut visible_node_ids = Vec::new();
    for group_index in 0..graph.groups.len() {
        let status = graph.groups[group_index].status;
        match status {
            DynamicGroupStatus::Open if dynamic_group_ready(graph, group_index) => {
                let merge_node = create_dynamic_merge_node(ctx, graph, group_index)?;
                let group_id = graph.groups[group_index].id.clone();
                graph.groups[group_index].status = DynamicGroupStatus::Merging;
                graph.groups[group_index].merge_node_id = Some(merge_node.id.clone());
                graph.groups[group_index].updated_at = now_rfc3339_like();
                visible_node_ids.push(merge_node.id.clone());
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
                let acceptance_node = create_dynamic_acceptance_node(ctx, graph, group_index)?;
                let group_id = graph.groups[group_index].id.clone();
                graph.groups[group_index].status = DynamicGroupStatus::Accepting;
                graph.groups[group_index].acceptance_node_id = Some(acceptance_node.id.clone());
                graph.groups[group_index].updated_at = now_rfc3339_like();
                visible_node_ids.push(acceptance_node.id.clone());
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
                for node in &graph.nodes {
                    if node.group_id.as_deref() == Some(group_id.as_str()) {
                        teardown_dynamic_workspace_best_effort(ctx, node);
                    }
                }
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
        let promoted_node_ids = refresh_dynamic_ready_nodes(graph);
        visible_node_ids.extend(promoted_node_ids);
        graph.run.updated_at = now_rfc3339_like();
        persist_dynamic_graph(ctx, graph)?;
        emit_dynamic_session_updates_best_effort(ctx, graph, &visible_node_ids);
    }
    Ok(DynamicGroupAdvanceResult { changed })
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
    ctx: &DynamicExecutionContext<'_>,
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
        ctx.app.paths.repo_root,
        dynamic_group_workspace_summary(ctx, graph, group),
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
        model: group.merge.model.clone(),
        permission_mode: ctx.dynamic.permission_mode().map(|mode| {
            ctx.app
                .config
                .resolve_permission_mode(&group.merge.provider, mode)
        }),
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
    ctx: &DynamicExecutionContext<'_>,
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
        model: group.acceptance.model.clone(),
        permission_mode: ctx.dynamic.permission_mode().map(|mode| {
            ctx.app
                .config
                .resolve_permission_mode(&group.acceptance.provider, mode)
        }),
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

fn dynamic_group_workspace_summary(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    group: &DynamicGroupState,
) -> String {
    let lines = graph
        .nodes
        .iter()
        .filter(|node| node.group_id.as_deref() == Some(group.id.as_str()))
        .filter_map(|node| node.workspace_path.as_ref().map(|path| (node, path)))
        .map(|(node, path)| {
            let mode = format!("{:?}", node.workspace.mode);
            if node.workspace.mode != WorkspaceMode::Worktree {
                return format!("- {}: mode={} path={}", node.id, mode, path);
            }
            let branch = dynamic_worktree_branch_name(ctx, &node.id);
            let actual_branch = git_capture(path, &["rev-parse", "--abbrev-ref", "HEAD"])
                .unwrap_or_else(|| branch.clone());
            let head =
                git_capture(path, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
            let merge_base = git_capture(
                &ctx.app.paths.repo_root,
                &["merge-base", "HEAD", actual_branch.as_str()],
            )
            .unwrap_or_else(|| "unknown".to_string());
            let status = git_capture(path, &["status", "--short"])
                .map(|value| value.lines().collect::<Vec<_>>().join("; "))
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "clean".to_string());
            format!(
                "- {}: mode={} path={} branch={} head={} mergeBase={} status={}",
                node.id, mode, path, actual_branch, head, merge_base, status
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
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
    let is_follow_up = continue_ref.is_some();
    // For follow-up chats in an existing session, skip full workflow validation.
    let validated: Option<ValidatedWorkflow>;
    let dynamic: &AiDynamicNode;
    if is_follow_up {
        dynamic = match workflow.nodes.iter().find(|n| n.id() == outer_node_id) {
            Some(NodeDsl::AiDynamic(d)) => d,
            _ => return Err(anyhow!("node `{outer_node_id}` is not an ai-dynamic node")),
        };
    } else {
        validated = Some(validate_workflow(workflow)?);
        dynamic = match validated.as_ref().unwrap().get_node(outer_node_id) {
            Some(NodeDsl::AiDynamic(d)) => d,
            _ => return Err(anyhow!("node `{outer_node_id}` is not an ai-dynamic node")),
        };
    }
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
        task_uuid: None,
        run_uuid: None,
        round_uuid: None,
        outer_node_uuid: None,
        resume_override: None,
    };
    let output_contract = match node.kind {
        DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation => {
            Some(dynamic_output_contract(&ctx, &graph))
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
        Vec::new(),
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
    resume_input_attachment_paths: Vec<String>,
) -> Result<WorkerInvocation> {
    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "runtime_context");
    let runtime_context = dynamic_runtime_context(ctx, &node.id, attempt_id);
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "runtime_context",
        step_started_at,
        serde_json::json!({
            "attemptDir": runtime_context.attempt_dir,
        }),
    );

    let step_started_at = dynamic_invocation_build_step_begin(ctx, node, attempt_id, "profile");
    let builtin_profile = dynamic_builtin_profile(ctx.app.config.desktop_language, node);
    let profile = builtin_profile
        .map(|(profile, _)| profile.to_string())
        .or_else(|| node.profile.clone());
    let profile_content = match builtin_profile {
        Some((_, content)) => Some(content.trim().to_string()),
        None => node.profile.as_deref().and_then(|profile| {
            ctx.app
                .profile_show(profile)
                .ok()
                .map(|entry| entry.content)
        }),
    };
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "profile",
        step_started_at,
        serde_json::json!({
            "profile": profile,
            "hasBuiltinProfile": builtin_profile.is_some(),
            "profileContentBytes": profile_content.as_ref().map(|value| value.len()).unwrap_or(0),
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "workspace_dir");
    let workspace_dir = node
        .workspace_path
        .clone()
        .unwrap_or_else(|| ctx.app.paths.repo_root.clone());
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "workspace_dir",
        step_started_at,
        serde_json::json!({
            "workspaceMode": node.workspace.mode,
            "workspacePath": workspace_dir,
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "system_sections");
    let extra_system_sections = dynamic_system_sections(ctx, graph, node)?;
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "system_sections",
        step_started_at,
        serde_json::json!({
            "sectionCount": extra_system_sections.len(),
            "sectionBytes": extra_system_sections.iter().map(|value| value.len()).sum::<usize>(),
        }),
    );

    let step_started_at = dynamic_invocation_build_step_begin(ctx, node, attempt_id, "model");
    let model = match node.kind {
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => {
            dynamic_acceptance_model(ctx.dynamic)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    node.provider
                        .as_deref()
                        .and_then(|provider| dynamic_model_for_provider(ctx.dynamic, provider))
                })
                .or_else(|| node.model.clone())
        }
        _ => node
            .provider
            .as_deref()
            .and_then(|provider| dynamic_model_for_provider(ctx.dynamic, provider))
            .or_else(|| node.model.clone()),
    };
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "model",
        step_started_at,
        serde_json::json!({
            "providerId": node.provider,
            "model": model,
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "requirement_text");
    let requirement_text = dynamic_requirement_text(ctx)?;
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "requirement_text",
        step_started_at,
        serde_json::json!({
            "bytes": requirement_text.len(),
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "predecessors");
    let predecessors = dynamic_predecessor_contexts(ctx, graph, node);
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "predecessors",
        step_started_at,
        serde_json::json!({
            "count": predecessors.len(),
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "task_instruction");
    let task_instruction = dynamic_task_instruction(ctx, graph, node);
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "task_instruction",
        step_started_at,
        serde_json::json!({
            "bytes": task_instruction.len(),
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "permission_mode");
    let permission_mode = {
        let raw = node
            .permission_mode
            .clone()
            .or_else(|| ctx.dynamic.permission_mode().map(ToOwned::to_owned));
        match (raw, node.provider.as_deref()) {
            (Some(normative), Some(provider)) => {
                Some(ctx.app.config.resolve_permission_mode(provider, &normative))
            }
            (other, _) => other,
        }
    };
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "permission_mode",
        step_started_at,
        serde_json::json!({
            "permissionMode": permission_mode,
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "attachments_dir");
    let attachments_dir = ctx.app.paths.dynamic_node_attachments_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        &node.id,
        attempt_id,
    );
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "attachments_dir",
        step_started_at,
        serde_json::json!({
            "attachmentsDir": attachments_dir,
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "input_attachment_paths");
    let mut input_attachment_paths = super::task_input_attachment_paths(ctx.app, ctx.task_id);
    input_attachment_paths.extend(resume_input_attachment_paths);
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "input_attachment_paths",
        step_started_at,
        serde_json::json!({
            "count": input_attachment_paths.len(),
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "assemble_invocation");
    let invocation = WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        profile,
        profile_content,
        requirement_path: None,
        requirement_text: Some(requirement_text),
        adapter_workspace_dir: ctx.app.paths.repo_root.clone(),
        workspace_dir,
        attempt_dir: runtime_context.attempt_dir.clone(),
        output_contract,
        runtime_context,
        predecessors,
        extra_system_sections,
        task_instruction: Some(task_instruction),
        session_mode,
        permission_mode,
        model,
        continue_ref,
        resume_prompt,
        resume_prompt_id,
        resume_prompt_visibility,
        stream_mode: StreamMode::StreamJson,
        log_prompts: ctx.app.config.log_prompts,
        log_provider_command: ctx.app.config.log_provider_command,
        attachments_dir: Some(attachments_dir),
        cold_artifacts: Vec::new(),
        cold_attachments: Vec::new(),
        input_attachment_paths,
        mcp_servers: Vec::new(),
        skill_catalog: String::new(),
    };
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "assemble_invocation",
        step_started_at,
        serde_json::json!({
            "hasOutputContract": invocation.output_contract.is_some(),
            "predecessorCount": invocation.predecessors.len(),
            "systemSectionCount": invocation.extra_system_sections.len(),
            "inputAttachmentCount": invocation.input_attachment_paths.len(),
        }),
    );
    Ok(invocation)
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
    Ok(
        std::fs::read_to_string(ctx.app.paths.requirement_file(ctx.task_id).as_std_path())
            .unwrap_or_default(),
    )
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
            "validation_errors": dynamic_validation_repair_lines(ctx, graph, errors),
            "repair_reference": dynamic_repair_reference_summary(ctx, graph),
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
    let validation_error = dynamic_parse_repair_error(error);
    dynamic_structured_repair_prompt(ctx, graph, node, &[validation_error])
}

fn dynamic_structured_repair_prompt(
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
            "validation_errors": dynamic_validation_repair_lines(ctx, graph, errors),
            "repair_reference": dynamic_repair_reference_summary(ctx, graph),
            "remaining_budget": dynamic_remaining_budget_summary(graph, node),
        }),
    )
    .expect("prompt template renders")
}

fn dynamic_parse_repair_error(error: String) -> DynamicProposalValidationError {
    let path = error
        .split("JSON path `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .filter(|path| !path.trim().is_empty())
        .unwrap_or("$");
    dynamic_validation_error(
        "dynamic.parse.invalid",
        "dynamic-node-completion is not valid for the expected DSL shape",
        serde_json::json!({
            "path": path,
            "actual": error,
            "expected": "valid dynamic-node-completion JSON matching the output protocol",
        }),
    )
}

fn dynamic_validation_repair_lines(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    errors: &[DynamicProposalValidationError],
) -> String {
    if errors.is_empty() {
        return "- none".to_string();
    }
    errors
        .iter()
        .map(|error| {
            let allowed_values = dynamic_allowed_values_for_error(ctx, graph, error);
            let suggestion = error
                .suggestion
                .clone()
                .or_else(|| dynamic_suggestion_for_error(ctx, error, &allowed_values));
            let mut lines = vec![format!("- [{}] {}", error.code, error.message)];
            if let Some(path) = error.path.as_deref() {
                lines.push(format!("  path: {path}"));
            }
            if let Some(actual) = error.actual.as_deref() {
                lines.push(format!("  actual: {actual}"));
            }
            if let Some(expected) = error.expected.as_deref() {
                lines.push(format!("  expected: {expected}"));
            }
            if !allowed_values.is_empty() {
                lines.push(format!("  allowed values: {}", allowed_values.join(", ")));
            }
            if let Some(suggestion) = suggestion {
                lines.push(format!("  suggested repair: {suggestion}"));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dynamic_repair_reference_summary(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) -> String {
    format!(
        "Available providers and models:\n{}\n\nAvailable worker profile IDs:\n{}\n\nAllowed workflow IDs:\n{}\n\nWorkspace capability:\n{}",
        available_provider_summary(ctx),
        available_profile_summary(ctx),
        allowed_workflow_snapshot_summary(&graph.run.allowed_workflow_snapshots),
        dynamic_workspace_capability_summary(ctx),
    )
}

fn dynamic_available_provider_ids(ctx: &DynamicExecutionContext<'_>) -> Vec<String> {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, .. } => vec![provider.clone()],
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_provider,
            available_agents,
            ..
        } => {
            let mut providers = vec![bootstrap_provider.clone()];
            for agent in available_agents {
                if !providers.iter().any(|provider| provider == &agent.provider) {
                    providers.push(agent.provider.clone());
                }
            }
            providers
        }
    }
}

fn dynamic_allowed_values_for_error(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    error: &DynamicProposalValidationError,
) -> Vec<String> {
    if !error.allowed_values.is_empty() {
        return error.allowed_values.clone();
    }
    let field = error
        .params
        .get("field")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if error.code.contains(".profile.") || field == "profile" {
        if !ctx.dynamic.allowed_profiles.is_empty() {
            return ctx.dynamic.allowed_profiles.clone();
        }
        return available_profile_refs(ctx)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
    }
    if error.code.contains(".provider.") || field == "provider" {
        return dynamic_available_provider_ids(ctx);
    }
    if error.code.contains(".model.") || field == "model" {
        if let Some(provider) = error
            .params
            .get("provider")
            .and_then(|value| value.as_str())
        {
            return provider_model_option_values(ctx, provider)
                .into_iter()
                .map(|(name, _)| name)
                .collect();
        }
    }
    if error.code.contains(".workflow-invocation.") || field == "workflowId" {
        return graph
            .run
            .allowed_workflow_snapshots
            .iter()
            .map(|snapshot| snapshot.workflow_id.clone())
            .collect();
    }
    Vec::new()
}

fn dynamic_suggestion_for_error(
    ctx: &DynamicExecutionContext<'_>,
    error: &DynamicProposalValidationError,
    allowed_values: &[String],
) -> Option<String> {
    let actual = error.actual.as_deref()?.trim();
    if actual.is_empty() {
        return None;
    }
    if error.code.contains(".profile.")
        || error.params.get("field").and_then(|value| value.as_str()) == Some("profile")
    {
        for (id, name) in available_profile_refs(ctx) {
            if actual == name || actual.eq_ignore_ascii_case(&name) {
                return Some(format!("replace with profileId `{id}`"));
            }
            if actual == id || actual.eq_ignore_ascii_case(&id) {
                return Some(format!("use profileId `{id}`"));
            }
        }
    }
    if allowed_values.iter().any(|value| value == actual) {
        return Some(format!("use `{actual}`"));
    }
    None
}

fn dynamic_task_instruction(
    ctx: &DynamicExecutionContext<'_>,
    _graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> String {
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
                snapshot.workflow_id, snapshot.name, snapshot.contains_ai_dynamic,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn available_provider_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, model } => {
            if let Some(model) = model.as_deref() {
                return format!("- {provider} (configured model: {model}; do not output model)");
            }
            let options = provider_model_options_summary(ctx, provider);
            if options.is_empty() {
                format!("- {provider} (model not configured; provider default will be used)")
            } else {
                format!(
                    "- {provider} (model required in proposal; choose one model by name)\n  models:\n  - {}",
                    options.join("\n  - ")
                )
            }
        }
        AiDynamicAgentStrategy::Dynamic {
            routing_prompt,
            available_agents,
            ..
        } => {
            if available_agents.is_empty() {
                return "none".to_string();
            }
            let requires_model_output = !routing_prompt.trim().is_empty();
            available_agents
                .iter()
                .map(|agent_ref| {
                    if let Some(model) = agent_ref.model.as_deref() {
                        return if requires_model_output {
                            format!(
                                "- {provider} (configured model: {model}; output model is still required, but runtime will use the configured model)",
                                provider = agent_ref.provider,
                            )
                        } else {
                            format!(
                                "- {provider} (configured model: {model}; do not output model)",
                                provider = agent_ref.provider,
                            )
                        };
                    }
                    let options = provider_model_options_summary(ctx, &agent_ref.provider);
                    if options.is_empty() {
                        if requires_model_output {
                            format!(
                                "- {provider} (model required in proposal; no model catalog is available, use a model supported by this provider)",
                                provider = agent_ref.provider,
                            )
                        } else {
                            format!(
                                "- {provider} (model not configured; provider default will be used)",
                                provider = agent_ref.provider,
                            )
                        }
                    } else {
                        format!(
                            "- {provider} (model required in proposal; choose one model by name)\n  models:\n  - {models}",
                            provider = agent_ref.provider,
                            models = options.join("\n  - "),
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

fn available_profile_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    let profiles = available_profile_refs(ctx);
    if profiles.is_empty() {
        "none".to_string()
    } else {
        profiles
            .into_iter()
            .map(|(id, name)| format!("- profileId={id} displayName={name}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn available_profile_refs(ctx: &DynamicExecutionContext<'_>) -> Vec<(String, String)> {
    match ctx.app.profiles() {
        Ok(list) => {
            let allowed = ctx
                .dynamic
                .allowed_profiles
                .iter()
                .map(|profile| profile.as_str())
                .collect::<std::collections::HashSet<_>>();
            list.profiles
                .into_iter()
                .filter(|profile| allowed.is_empty() || allowed.contains(profile.id.as_str()))
                .map(|profile| (profile.id, profile.name))
                .collect()
        }
        Err(_) => Vec::new(),
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
            let continue_ref =
                dynamic_node_continue_ref(ctx, candidate, &dynamic_attempt_id(candidate))?;
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
    ctx: &DynamicExecutionContext<'_>,
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
                    candidate
                        .child_run_id
                        .as_ref()
                        .map(|child_run_id| format!("{}={}", candidate.id, child_run_id))
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
                dynamic_group_workspace_summary(ctx, graph, group),
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
            "workspace_capability": dynamic_workspace_capability_summary(ctx),
            "upstream_refs": dynamic_upstream_refs_summary(ctx, graph, node),
            "allowed_workflow_snapshots": allowed_workflow_snapshot_summary(&graph.run.allowed_workflow_snapshots),
            "agent_strategy_mode": dynamic_agent_strategy_mode(ctx.dynamic),
            "bootstrap_provider": ctx.dynamic.bootstrap_provider().unwrap_or("none"),
            "agent_routing_prompt": dynamic_agent_routing_prompt(ctx.dynamic).unwrap_or("none"),
            "acceptance_model_policy": match ctx.app.config.desktop_language {
                DesktopLanguage::ZhCn => match dynamic_acceptance_model(ctx.dynamic) {
                    Some(model) => format!(
                        "`merge` / `acceptance` 固定使用验收模型 `{model}`；这两个 spec 不要输出 `model`。"
                    ),
                    None => "未单独配置验收模型；`merge` / `acceptance` 与普通动态节点沿用同一套模型规则。".to_string(),
                },
                DesktopLanguage::En => match dynamic_acceptance_model(ctx.dynamic) {
                    Some(model) => format!(
                        "`merge` / `acceptance` use the configured acceptance model `{model}`; those specs must not output `model`."
                    ),
                    None => "No dedicated acceptance model is configured; `merge` / `acceptance` follow the same model rules as other dynamic nodes.".to_string(),
                },
            },
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

fn dynamic_worktree_short_id(ctx: &DynamicExecutionContext<'_>, node_id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    ctx.round_id.hash(&mut hasher);
    ctx.outer_node_id.hash(&mut hasher);
    ctx.outer_attempt_id.hash(&mut hasher);
    node_id.hash(&mut hasher);
    format!("dyn-{:016x}", hasher.finish())
}

fn dynamic_worktree_branch_name(ctx: &DynamicExecutionContext<'_>, node_id: &str) -> String {
    format!(
        "gb-dyn-{}-{}-{}",
        safe_dynamic_ref(ctx.task_id),
        safe_dynamic_ref(ctx.run_id),
        dynamic_worktree_short_id(ctx, node_id)
    )
}

fn dynamic_worktree_base_dir(ctx: &DynamicExecutionContext<'_>) -> Utf8PathBuf {
    ctx.app
        .paths
        .repo_gold_band_root
        .join("worktrees")
        .join(safe_dynamic_ref(ctx.task_id))
        .join(safe_dynamic_ref(ctx.run_id))
}

fn dynamic_worktree_dir(ctx: &DynamicExecutionContext<'_>, node_id: &str) -> Utf8PathBuf {
    dynamic_worktree_base_dir(ctx).join(dynamic_worktree_short_id(ctx, node_id))
}

fn git_output(cwd: &Utf8Path, args: &[&str]) -> Result<GitCommandOutput> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd.as_str())
        .args(args)
        .output()?;
    Ok(GitCommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn git_command_details(output: &GitCommandOutput) -> String {
    match (output.stdout.is_empty(), output.stderr.is_empty()) {
        (true, true) => "no git output".to_string(),
        (false, true) => format!("stdout: {}", output.stdout),
        (true, false) => format!("stderr: {}", output.stderr),
        (false, false) => format!("stdout: {}; stderr: {}", output.stdout, output.stderr),
    }
}

fn git_capture(cwd: &Utf8Path, args: &[&str]) -> Option<String> {
    let output = git_output(cwd, args).ok()?;
    if !output.success {
        return None;
    }
    Some(output.stdout).filter(|value| !value.is_empty())
}

fn dynamic_worktree_is_git_worktree(path: &Utf8Path) -> bool {
    git_capture(path, &["rev-parse", "--show-toplevel"])
        .map(|root| Utf8PathBuf::from(root) == path)
        .unwrap_or(false)
}

fn parse_git_branch_list_name(line: &str) -> Option<&str> {
    line.trim_start_matches(['*', '+', ' '])
        .split_whitespace()
        .next()
}

fn delete_dynamic_worktree_branch(
    ctx: &DynamicExecutionContext<'_>,
    branch: &str,
    missing_ok: bool,
) -> Result<()> {
    let list_output = git_output(&ctx.app.paths.repo_root, &["branch", "--list", branch])?;
    if !list_output.success {
        bail!(
            "failed to inspect branch `{}`: {}",
            branch,
            git_command_details(&list_output)
        );
    }
    let exists = list_output
        .stdout
        .lines()
        .any(|line| parse_git_branch_list_name(line) == Some(branch));
    if !exists {
        ensure!(missing_ok, "branch `{}` does not exist", branch);
        return Ok(());
    }
    let delete_output = git_output(&ctx.app.paths.repo_root, &["branch", "-D", branch])?;
    ensure!(
        delete_output.success,
        "git branch -D `{}` failed: {}",
        branch,
        git_command_details(&delete_output)
    );
    Ok(())
}

fn dynamic_workspace_capability(ctx: &DynamicExecutionContext<'_>) -> DynamicWorkspaceCapability {
    let repo_root = &ctx.app.paths.repo_root;
    let Some(is_inside_work_tree) = git_capture(repo_root, &["rev-parse", "--is-inside-work-tree"])
    else {
        return DynamicWorkspaceCapability {
            supports_worktree: false,
            reason_code: "git-unavailable-or-non-git",
            reason: format!(
                "`{}` is not a Git working tree or the `git` command is unavailable",
                repo_root
            ),
        };
    };
    if is_inside_work_tree != "true" {
        return DynamicWorkspaceCapability {
            supports_worktree: false,
            reason_code: "non-git-workspace",
            reason: format!("`{}` is not inside a Git working tree", repo_root),
        };
    }
    if git_capture(repo_root, &["rev-parse", "--verify", "HEAD"]).is_none() {
        return DynamicWorkspaceCapability {
            supports_worktree: false,
            reason_code: "missing-head",
            reason: format!("`{}` has no HEAD commit for git worktree add", repo_root),
        };
    }
    if git_capture(repo_root, &["worktree", "list", "--porcelain"]).is_none() {
        return DynamicWorkspaceCapability {
            supports_worktree: false,
            reason_code: "worktree-unsupported",
            reason: "git worktree is not available for the current workspace".to_string(),
        };
    }
    DynamicWorkspaceCapability {
        supports_worktree: true,
        reason_code: "ready",
        reason: "git worktree is available".to_string(),
    }
}

fn dynamic_workspace_capability_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    let capability = dynamic_workspace_capability(ctx);
    match (ctx.app.config.desktop_language, capability.supports_worktree) {
        (DesktopLanguage::ZhCn, true) => {
            "- supportsWorktree: true\n- 原因：git worktree 可用\n- 可写并行分支：需要隔离时可以使用 workspace.mode=\"worktree\"".to_string()
        }
        (DesktopLanguage::ZhCn, false) => format!(
            "- supportsWorktree: false\n- reasonCode: {}\n- 原因：{}\n- 必须遵守：不要输出 workspace.mode=\"worktree\"；分析使用 readonly，需要写入时使用串行 main，或结束并说明需要用户初始化 Git 后才能使用并行可写 fan-out",
            capability.reason_code,
            capability.reason
        ),
        (DesktopLanguage::En, true) => {
            "- supportsWorktree: true\n- reason: git worktree is available\n- writable parallel branches: use workspace.mode=\"worktree\" when isolation is needed".to_string()
        }
        (DesktopLanguage::En, false) => format!(
            "- supportsWorktree: false\n- reasonCode: {}\n- reason: {}\n- required behavior: do not output workspace.mode=\"worktree\"; use readonly for analysis and main only for serial writable work, or end with a summary that asks the user to initialize Git before parallel writable fan-out",
            capability.reason_code,
            capability.reason
        ),
    }
}

fn dynamic_worktree_unavailable_error(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    capability: &DynamicWorkspaceCapability,
) -> anyhow::Error {
    anyhow!(
        "workspace.worktree-git-required: dynamic node `{}` requested workspace.mode=worktree but workspace `{}` cannot create git worktrees (reasonCode={}, reason={})",
        node_id,
        ctx.app.paths.repo_root,
        capability.reason_code,
        capability.reason
    )
}

fn ensure_dynamic_workspace(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
) -> Result<()> {
    match node.workspace.mode {
        WorkspaceMode::Readonly | WorkspaceMode::Main => {
            node.workspace_path = Some(ctx.app.paths.repo_root.clone());
            dynamic_event_best_effort(
                ctx,
                "dynamic_workspace_ready",
                serde_json::json!({
                    "nodeId": node.id,
                    "workspaceMode": node.workspace.mode,
                    "workspacePath": node.workspace_path.clone(),
                }),
            );
            Ok(())
        }
        WorkspaceMode::Worktree => {
            if node.workspace_path.is_some() {
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_ready",
                    serde_json::json!({
                        "nodeId": node.id,
                        "workspaceMode": node.workspace.mode,
                        "workspacePath": node.workspace_path.clone(),
                        "reusedExistingPath": true,
                    }),
                );
                return Ok(());
            }
            let capability_started_at = Instant::now();
            dynamic_event_best_effort(
                ctx,
                "dynamic_worktree_capability_begin",
                serde_json::json!({
                    "nodeId": node.id,
                    "workspaceMode": node.workspace.mode,
                }),
            );
            let capability = dynamic_workspace_capability(ctx);
            dynamic_event_best_effort(
                ctx,
                "dynamic_worktree_capability_end",
                serde_json::json!({
                    "nodeId": node.id,
                    "elapsedMs": elapsed_ms(capability_started_at),
                    "supportsWorktree": capability.supports_worktree,
                    "reasonCode": capability.reason_code,
                }),
            );
            if !capability.supports_worktree {
                return Err(dynamic_worktree_unavailable_error(
                    ctx,
                    &node.id,
                    &capability,
                ));
            }
            let worktree_dir = dynamic_worktree_dir(ctx, &node.id);
            let branch = dynamic_worktree_branch_name(ctx, &node.id);
            let existing_check_started_at = Instant::now();
            dynamic_event_best_effort(
                ctx,
                "dynamic_worktree_existing_check_begin",
                serde_json::json!({
                    "nodeId": node.id,
                    "workspacePath": worktree_dir,
                    "branch": branch,
                    "exists": worktree_dir.exists(),
                }),
            );
            if worktree_dir.exists() && !dynamic_worktree_is_git_worktree(&worktree_dir) {
                std::fs::remove_dir_all(worktree_dir.as_std_path()).map_err(|error| {
                    anyhow!(
                        "failed to remove incomplete dynamic worktree directory `{}` for `{}`: {error}",
                        worktree_dir,
                        node.id
                    )
                })?;
            }
            if worktree_dir.exists() {
                let actual_branch =
                    git_capture(&worktree_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
                        .unwrap_or_else(|| "unknown".to_string());
                ensure!(
                    actual_branch == branch,
                    "dynamic worktree `{}` for `{}` is on branch `{}` but expected `{}`",
                    worktree_dir,
                    node.id,
                    actual_branch,
                    branch
                );
            }
            dynamic_event_best_effort(
                ctx,
                "dynamic_worktree_existing_check_end",
                serde_json::json!({
                    "nodeId": node.id,
                    "elapsedMs": elapsed_ms(existing_check_started_at),
                    "workspacePath": worktree_dir,
                    "branch": branch,
                    "exists": worktree_dir.exists(),
                }),
            );
            if !worktree_dir.exists() {
                std::fs::create_dir_all(
                    worktree_dir
                        .parent()
                        .ok_or_else(|| anyhow!("dynamic worktree path has no parent"))?
                        .as_std_path(),
                )?;
                let lock_wait_started_at = Instant::now();
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_git_lock_wait_begin",
                    serde_json::json!({
                        "nodeId": node.id,
                        "workspacePath": worktree_dir,
                        "branch": branch,
                    }),
                );
                let _git_guard = DYNAMIC_WORKTREE_GIT_LOCK
                    .get_or_init(|| Mutex::new(()))
                    .lock()
                    .map_err(|_| anyhow!("dynamic worktree git lock poisoned"))?;
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_git_lock_wait_end",
                    serde_json::json!({
                        "nodeId": node.id,
                        "elapsedMs": elapsed_ms(lock_wait_started_at),
                        "workspacePath": worktree_dir,
                        "branch": branch,
                    }),
                );
                let cleanup_started_at = Instant::now();
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_branch_cleanup_begin",
                    serde_json::json!({
                        "nodeId": node.id,
                        "branch": branch,
                    }),
                );
                delete_dynamic_worktree_branch(ctx, &branch, true).map_err(|error| {
                    anyhow!(
                        "failed to clear stale dynamic worktree branch `{}` for `{}`: {error}",
                        branch,
                        node.id
                    )
                })?;
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_branch_cleanup_end",
                    serde_json::json!({
                        "nodeId": node.id,
                        "elapsedMs": elapsed_ms(cleanup_started_at),
                        "branch": branch,
                    }),
                );
                let add_started_at = Instant::now();
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_add_begin",
                    serde_json::json!({
                        "nodeId": node.id,
                        "workspacePath": worktree_dir,
                        "branch": branch,
                    }),
                );
                let output = git_output(
                    &ctx.app.paths.repo_root,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        &branch,
                        worktree_dir.as_str(),
                        "HEAD",
                    ],
                )?;
                dynamic_event_best_effort(
                    ctx,
                    "dynamic_worktree_add_end",
                    serde_json::json!({
                        "nodeId": node.id,
                        "elapsedMs": elapsed_ms(add_started_at),
                        "workspacePath": worktree_dir,
                        "branch": branch,
                        "success": output.success,
                        "stdout": truncate_diagnostic_text(&output.stdout),
                        "stderr": truncate_diagnostic_text(&output.stderr),
                    }),
                );
                if !output.success {
                    let _ = delete_dynamic_worktree_branch(ctx, &branch, true);
                    bail!(
                        "failed to create dynamic worktree for `{}` at `{}` with branch `{}`: {}",
                        node.id,
                        worktree_dir,
                        branch,
                        git_command_details(&output)
                    );
                }
            }
            node.workspace_path = Some(worktree_dir.clone());
            dynamic_event_best_effort(
                ctx,
                "dynamic_worktree_ready",
                serde_json::json!({
                    "nodeId": node.id,
                    "workspaceMode": node.workspace.mode,
                    "workspacePath": worktree_dir,
                    "branch": branch,
                    "reusedExistingPath": false,
                }),
            );
            Ok(())
        }
    }
}

fn teardown_dynamic_workspace_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
) {
    if node.workspace.mode != WorkspaceMode::Worktree {
        return;
    }
    let Some(worktree_dir) = node.workspace_path.as_ref() else {
        return;
    };
    let branch = dynamic_worktree_branch_name(ctx, &node.id);
    let _ = git_output(
        &ctx.app.paths.repo_root,
        &["worktree", "remove", "--force", worktree_dir.as_str()],
    );
    let _ = delete_dynamic_worktree_branch(ctx, &branch, true);
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
    persist_dynamic_graph_for_resume(
        ctx.app,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        graph,
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

fn dynamic_graph_has_paused_leaf(graph: &DynamicGraphState) -> bool {
    graph
        .nodes
        .iter()
        .any(|node| node.status == DynamicNodeStatus::Paused && node.outcome.is_none())
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
    append_dynamic_event_for_ids(
        ctx.app,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        event_type,
        data,
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
    mut dynamic_resume_override: Option<DynamicResumeOverride>,
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

        // ── Observability: notify node started ──
        {
            let seq = round
                .trace
                .iter()
                .filter(|t| t.node_id == node.node_id)
                .map(|t| t.sequence)
                .last();
            let node_name = node
                .resolved_config
                .get("profileName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
                .or_else(|| {
                    node.resolved_config
                        .get("provider")
                        .and_then(|v| v.as_str())
                })
                .map(|s| s.to_string());
            let agent_type = node
                .resolved_config
                .get("provider")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            app.lifecycle_bus
                .emit(super::RuntimeLifecycleEvent::NodeStarted {
                    task_id: task_id.to_string(),
                    task_uuid: run.task_uuid.clone(),
                    run_id: run.id.clone(),
                    run_uuid: run.uuid.clone(),
                    round_id: round.id.clone(),
                    round_uuid: round.uuid.clone(),
                    node_id: node.node_id.clone(),
                    node_uuid: node.uuid.clone(),
                    attempt_id: node.attempt_id.clone(),
                    repo_root: app.paths.repo_root.to_string(),
                    seq,
                    node_name,
                    agent_type,
                    started_at: node.started_at.clone(),
                    attempt_dir: None,
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
                run,
                round,
                &current_attempt_id,
                dynamic,
                node.clone(),
                dynamic_resume_override.take(),
            ),
        };
        if !attempt_is_still_current_running(
            app,
            task_id,
            &run.id,
            &round.id,
            &current_node_id,
            &current_attempt_id,
        )? {
            return Ok(());
        }

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
                emit_pause_side_effects(app, task_id, run, round, &failed_node);
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
                graph
                    .run
                    .pause_reason
                    .unwrap_or(PauseReason::ProcessInterrupted)
            } else {
                PauseReason::ProcessInterrupted
            };
            run.status = RunStatus::Paused;
            run.pause_reason = Some(pause_reason);
            run.updated_at = now_rfc3339_like();
            round.status = RunStatus::Paused;
            round.outcome = None;
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
            emit_pause_side_effects(app, task_id, run, round, &node);
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
            round.outcome = None;
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
            emit_pause_side_effects(app, task_id, run, round, &node);
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

        // Build attempt_dir for both snapshot persistence and observability event.
        let attempt_dir = app
            .paths
            .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)
            .to_string();

        let completed_snapshot = completed_node_snapshot(round, &node, Some(attempt_dir.clone()));
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
            run.last_executed_node = Some(completed_snapshot);
            node = next.node;
            session_mode = next.session_mode;
            continue_ref = next.continue_ref;
            resume_prompt = None;
            resume_prompt_id = None;
            resume_prompt_visibility = PromptVisibility::Visible;
            invalid_output_repair_prompts = 0;
            continue;
        }
        // Workflow ended — emit completed event for observability subscribers
        run.last_executed_node = Some(completed_snapshot.clone());
        app.lifecycle_bus
            .emit(super::RuntimeLifecycleEvent::NodeCompleted {
                task_id: task_id.to_string(),
                task_uuid: run.task_uuid.clone(),
                run_id: run.id.clone(),
                run_uuid: run.uuid.clone(),
                round_id: round.id.clone(),
                round_uuid: round.uuid.clone(),
                node_id: node.node_id.clone(),
                node_uuid: node.uuid.clone(),
                attempt_id: node.attempt_id.clone(),
                repo_root: app.paths.repo_root.to_string(),
                seq: completed_snapshot.seq,
                node_name: completed_snapshot.node_name.clone(),
                agent_type: completed_snapshot.agent_type.clone(),
                started_at: node.started_at.clone(),
                finished_at: node.finished_at.clone(),
                outcome: completed_snapshot.status.clone(),
                attempt_dir: attempt_dir.clone(),
                suppress_sentinel: false,
            });
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;
    use crate::dsl::{AiDynamicAgentStrategy, DynamicControlDsl};
    use crate::provider::{OutputArtifactPayload, ProviderResultPayload, SessionRef};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn git(cwd: &Utf8Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd.as_str())
            .args(args)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo() -> (tempfile::TempDir, Utf8PathBuf) {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        git(&repo_root, &["init"]);
        git(&repo_root, &["config", "user.email", "test@example.com"]);
        git(&repo_root, &["config", "user.name", "Test User"]);
        std::fs::write(repo_root.join("README.md").as_std_path(), "hello\n").unwrap();
        git(&repo_root, &["add", "README.md"]);
        git(&repo_root, &["commit", "-m", "init"]);
        (temp, repo_root)
    }

    fn test_dynamic() -> AiDynamicNode {
        AiDynamicNode {
            id: "ai-dynamic".to_string(),
            agent_strategy: AiDynamicAgentStrategy::Fixed {
                provider: "claude-acp".to_string(),
                model: None,
            },
            permission_mode: None,
            allowed_profiles: Vec::new(),
            global_goal: None,
            control: DynamicControlDsl::default(),
            allowed_workflows: Vec::new(),
        }
    }

    fn test_context<'a>(app: &'a App, dynamic: &'a AiDynamicNode) -> DynamicExecutionContext<'a> {
        DynamicExecutionContext {
            app,
            task_id: "task-006",
            run_id: "run-001",
            round_id: "round-001",
            outer_node_id: "ai-dynamic",
            outer_attempt_id: "attempt-001",
            dynamic,
            task_uuid: None,
            run_uuid: None,
            round_uuid: None,
            outer_node_uuid: None,
            resume_override: None,
        }
    }

    fn test_worktree_node(id: &str) -> DynamicNodeState {
        DynamicNodeState {
            version: VERSION.to_string(),
            id: id.to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            kind: DynamicNodeKind::Worker,
            title: id.to_string(),
            task: id.to_string(),
            status: DynamicNodeStatus::Ready,
            outcome: None,
            group_id: None,
            chain_id: id.to_string(),
            depth: 1,
            depends_on: Vec::new(),
            workspace: WorkspacePolicy {
                mode: WorkspaceMode::Worktree,
            },
            workspace_path: None,
            provider: Some("claude-acp".to_string()),
            profile: None,
            permission_mode: None,
            model: None,
            session_mode: SessionMode::New,
            continue_from_node_id: None,
            workflow_id: None,
            workflow_snapshot_id: None,
            child_run_id: None,
            started_at: None,
            finished_at: None,
        }
    }

    fn write_test_outer_run(app: &App) {
        write_test_outer_attempt(app, RunStatus::Running, None);
    }

    fn write_test_outer_attempt(app: &App, status: RunStatus, pause_reason: Option<PauseReason>) {
        let now = "2026-06-16T00:00:00Z".to_string();
        let outcome = None;
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-006".to_string(),
            task_uuid: None,
            status,
            outcome,
            started_at: now.clone(),
            updated_at: now.clone(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("ai-dynamic".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason,
            uuid: None,
            last_executed_node: None,
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: "run-001".to_string(),
            index: 1,
            status,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: now.clone(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            node_id: "ai-dynamic".to_string(),
            node_type: crate::domain::NodeType::AiDynamic,
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            attempt_id: "attempt-001".to_string(),
            status,
            outcome: None,
            started_at: now,
            finished_at: if status == RunStatus::Paused {
                Some("2026-06-16T00:00:01Z".to_string())
            } else {
                None
            },
            manual_check_pending: false,
            resolved_config: Default::default(),
            uuid: None,
        };
        persist_runtime_state(app, "task-006", &run, &round, &node).unwrap();
    }

    fn test_dynamic_graph(nodes: Vec<DynamicNodeState>) -> DynamicGraphState {
        DynamicGraphState {
            version: VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-001".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "ai-dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: DynamicRunStatus::Running,
                outcome: None,
                pause_reason: None,
                started_at: "2026-06-16T00:00:00Z".to_string(),
                updated_at: "2026-06-16T00:00:00Z".to_string(),
                control: DynamicControlDsl::default(),
                allowed_workflow_snapshots: Vec::new(),
                current_node_ids: nodes.iter().map(|node| node.id.clone()).collect(),
            },
            nodes,
            groups: Vec::new(),
            proposals: Vec::new(),
        }
    }

    fn test_agent_task(title: &str) -> DynamicAgentTaskSpec {
        DynamicAgentTaskSpec {
            title: title.to_string(),
            provider: "claude-acp".to_string(),
            model: None,
            task: title.to_string(),
        }
    }

    fn test_end_completion(summary: &str) -> String {
        format!(
            r#"{{
                "version": "0.1",
                "kind": "dynamic-node-completion",
                "status": "success",
                "summary": "{summary}",
                "next": {{ "type": "end" }}
            }}"#
        )
    }

    #[test]
    fn refresh_dynamic_ready_nodes_returns_promoted_leaf_ids() {
        let mut completed = test_worktree_node("bootstrap");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        let mut pending = test_worktree_node("next");
        pending.status = DynamicNodeStatus::Pending;
        pending.depends_on = vec!["bootstrap".to_string()];
        let mut graph = test_dynamic_graph(vec![completed, pending]);
        graph.run.current_node_ids.clear();

        let promoted = refresh_dynamic_ready_nodes(&mut graph);

        assert_eq!(promoted, vec!["next".to_string()]);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.run.current_node_ids, vec!["next".to_string()]);
    }

    #[test]
    fn dynamic_group_successor_creation_emits_session_update() {
        let (_temp, repo_root) = init_repo();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = App::with_config(repo_root, RuntimeConfig::default()).with_acp_session_update(
            Arc::new(move |context| {
                seen_for_callback.lock().unwrap().push(context);
                Ok(())
            }),
        );
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut source = test_worktree_node("bootstrap");
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        source.group_id = Some("python-classes".to_string());
        source.chain_id = "bootstrap".to_string();
        let mut graph = test_dynamic_graph(vec![source]);
        graph.groups.push(DynamicGroupState {
            version: VERSION.to_string(),
            id: "python-classes".to_string(),
            dynamic_run_id: graph.run.id.clone(),
            status: DynamicGroupStatus::Open,
            depth: 1,
            parent_group_id: None,
            root_node_ids: vec!["bootstrap".to_string()],
            terminal_node_ids: vec!["bootstrap".to_string()],
            merge_node_id: None,
            acceptance_node_id: None,
            created_by_node_id: "bootstrap".to_string(),
            merge: test_agent_task("merge"),
            acceptance: test_agent_task("accept"),
            created_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:00Z".to_string(),
        });
        graph.proposals.push(DynamicProposalState {
            version: VERSION.to_string(),
            id: "proposal-bootstrap-001".to_string(),
            dynamic_run_id: graph.run.id.clone(),
            source_node_id: "bootstrap".to_string(),
            artifact_path: Utf8PathBuf::from("artifact.json"),
            raw_output_path: Utf8PathBuf::from("raw.txt"),
            parsed: serde_json::json!({}),
            validation_status: DynamicProposalValidationStatus::Accepted,
            validation_errors: Vec::new(),
            materialized_event_ids: Vec::new(),
            created_at: "2026-06-16T00:00:00Z".to_string(),
        });

        let advanced = advance_dynamic_groups(&ctx, &mut graph).unwrap();

        assert!(advanced.changed);
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "python-classes-merge")
        );
        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].node_id, "python-classes-merge");
        assert_eq!(calls[0].attempt_id, "attempt-001");
        assert_eq!(calls[0].outer_node_id.as_deref(), Some("ai-dynamic"));
    }

    #[test]
    fn dynamic_worktree_invocation_uses_repo_adapter_workspace_and_worktree_session_workspace() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("good-night");
        let worktree_dir = dynamic_worktree_dir(&ctx, &node.id);
        node.workspace_path = Some(worktree_dir.clone());
        let graph = test_dynamic_graph(vec![node.clone()]);

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &node,
            &dynamic_attempt_id(&node),
            None,
            SessionMode::New,
            None,
            None,
            None,
            PromptVisibility::Visible,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(invocation.adapter_workspace_dir, repo_root);
        assert_eq!(invocation.workspace_dir, worktree_dir);
    }

    #[test]
    fn dynamic_worktree_dir_uses_project_gold_band_task_run_short_id() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);

        let worktree_dir = dynamic_worktree_dir(&ctx, "good-night");
        let short_id = dynamic_worktree_short_id(&ctx, "good-night");

        assert!(worktree_dir.starts_with(&app.paths.repo_gold_band_root));
        assert!(!worktree_dir.starts_with(&app.paths.runtime_root));
        assert_eq!(short_id.len(), "dyn-0000000000000000".len());
        assert_eq!(worktree_dir.file_name(), Some(short_id.as_str()));
        assert_eq!(
            worktree_dir,
            app.paths
                .repo_gold_band_root
                .join("worktrees")
                .join("task-006")
                .join("run-001")
                .join(short_id)
        );
    }

    #[test]
    fn dynamic_worktree_short_id_includes_round_outer_attempt_and_node() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let base = test_context(&app, &dynamic);
        let same = dynamic_worktree_short_id(&base, "good-night");
        let different_node = dynamic_worktree_short_id(&base, "good-morning");
        let different_round = DynamicExecutionContext {
            round_id: "round-002",
            ..test_context(&app, &dynamic)
        };
        let different_outer_attempt = DynamicExecutionContext {
            outer_attempt_id: "attempt-002",
            ..test_context(&app, &dynamic)
        };

        assert_eq!(same, dynamic_worktree_short_id(&base, "good-night"));
        assert_ne!(same, different_node);
        assert_ne!(
            same,
            dynamic_worktree_short_id(&different_round, "good-night")
        );
        assert_ne!(
            same,
            dynamic_worktree_short_id(&different_outer_attempt, "good-night")
        );
    }

    #[test]
    fn ensure_dynamic_workspace_clears_stale_branch_before_add() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("good-night");
        let branch = dynamic_worktree_branch_name(&ctx, &node.id);
        git(&repo_root, &["branch", &branch, "HEAD"]);

        ensure_dynamic_workspace(&ctx, &mut node).unwrap();

        let worktree_path = node.workspace_path.as_ref().unwrap();
        assert!(worktree_path.exists());
        assert_eq!(
            git_capture(worktree_path, &["rev-parse", "--abbrev-ref", "HEAD"]).as_deref(),
            Some(branch.as_str())
        );
        let branch_list = git_capture(&repo_root, &["branch", "--list", &branch]).unwrap();
        assert!(
            branch_list
                .lines()
                .any(|line| parse_git_branch_list_name(line) == Some(branch.as_str()))
        );
        teardown_dynamic_workspace_best_effort(&ctx, &node);
        assert!(git_capture(&repo_root, &["branch", "--list", &branch]).is_none());
    }

    #[test]
    fn ensure_dynamic_workspace_removes_incomplete_directory() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("good-night");
        let worktree_dir = dynamic_worktree_dir(&ctx, &node.id);
        std::fs::create_dir_all(worktree_dir.as_std_path()).unwrap();
        std::fs::write(worktree_dir.join("stale.txt").as_std_path(), "stale").unwrap();

        ensure_dynamic_workspace(&ctx, &mut node).unwrap();

        assert!(node.workspace_path.as_ref().unwrap().exists());
        assert!(!worktree_dir.join("stale.txt").exists());
        assert_eq!(
            git_capture(
                node.workspace_path.as_ref().unwrap(),
                &["rev-parse", "--is-inside-work-tree"]
            )
            .as_deref(),
            Some("true")
        );
        teardown_dynamic_workspace_best_effort(&ctx, &node);
    }

    #[test]
    fn dynamic_inner_resume_only_rearms_target_node() {
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut other = test_worktree_node("other");
        other.status = DynamicNodeStatus::Paused;
        other.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = DynamicGraphState {
            version: VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-001".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "ai-dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: DynamicRunStatus::Paused,
                outcome: None,
                pause_reason: Some(PauseReason::ProcessInterrupted),
                started_at: "2026-06-16T00:00:00Z".to_string(),
                updated_at: "2026-06-16T00:00:00Z".to_string(),
                control: DynamicControlDsl::default(),
                allowed_workflow_snapshots: Vec::new(),
                current_node_ids: vec!["target".to_string(), "other".to_string()],
            },
            nodes: vec![target, other],
            groups: Vec::new(),
            proposals: Vec::new(),
        };
        let resume = DynamicResumeOverride {
            node_id: "target".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            attachment_paths: Vec::new(),
        };

        resume_paused_dynamic_graph(&mut graph, Some(&resume)).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(graph.run.current_node_ids, vec!["target".to_string()]);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[0].outcome, None);
        assert_eq!(graph.nodes[0].finished_at, None);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Paused);
        assert!(graph.nodes[1].finished_at.is_some());
    }

    #[test]
    fn dynamic_graph_continue_without_leaf_override_does_not_rearm_all_paused_leaves() {
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut other = test_worktree_node("other");
        other.status = DynamicNodeStatus::Paused;
        other.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![target, other]);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.pause_reason = Some(PauseReason::ProcessInterrupted);
        graph.run.current_node_ids.clear();

        resume_paused_dynamic_graph(&mut graph, None).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(
            graph.run.pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert!(graph.run.current_node_ids.is_empty());
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Paused);
    }

    #[test]
    fn dynamic_graph_parent_continue_rearms_paused_workflow_invocation_only() {
        let mut invocation = test_worktree_node("child-flow-node");
        invocation.kind = DynamicNodeKind::WorkflowInvocation;
        invocation.status = DynamicNodeStatus::Paused;
        invocation.provider = None;
        invocation.workflow_id = Some("child-flow".to_string());
        invocation.child_run_id = Some("run-002".to_string());
        invocation.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut worker = test_worktree_node("worker");
        worker.status = DynamicNodeStatus::Paused;
        worker.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![invocation, worker]);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.pause_reason = Some(PauseReason::ProcessInterrupted);
        graph.run.current_node_ids.clear();

        resume_paused_dynamic_graph(&mut graph, None).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(
            graph.run.current_node_ids,
            vec!["child-flow-node".to_string()]
        );
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[0].finished_at, None);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Paused);
        assert!(graph.nodes[1].finished_at.is_some());
    }

    #[test]
    fn stale_cancelled_running_dynamic_leaf_is_rearmed_on_resume() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let mut target = test_worktree_node("bootstrap");
        target.status = DynamicNodeStatus::Running;
        target.outcome = None;
        let mut graph = test_dynamic_graph(vec![target]);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.pause_reason = Some(PauseReason::ProcessInterrupted);
        write_json(
            &app.paths.dynamic_graph_file(
                "task-006",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
            ),
            &graph,
        )
        .unwrap();
        write_json(
            &app.paths.dynamic_node_file(
                "task-006",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
                "bootstrap",
            ),
            &graph.nodes[0],
        )
        .unwrap();
        write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    "task-006",
                    "run-001",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "bootstrap",
                    "attempt-001",
                )
                .join("acp.session.json"),
            &serde_json::json!({
                "status": "cancelled",
                "stopReason": "cancelled",
                "sessionId": "session-bootstrap"
            }),
        )
        .unwrap();
        let mut persisted = read_json::<DynamicGraphState>(&app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();
        let resume = DynamicResumeOverride {
            node_id: "bootstrap".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            attachment_paths: Vec::new(),
        };
        if dynamic_node_is_stale_resumable_active(
            &app,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            persisted.run.status,
            &persisted.nodes[0],
        ) {
            persisted.nodes[0].status = DynamicNodeStatus::Paused;
            persisted.nodes[0].finished_at = Some("2026-06-16T00:00:01Z".to_string());
        }

        resume_paused_dynamic_graph(&mut persisted, Some(&resume)).unwrap();

        assert_eq!(persisted.run.status, DynamicRunStatus::Running);
        assert_eq!(persisted.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(persisted.nodes[0].finished_at, None);
        assert_eq!(
            persisted.run.current_node_ids,
            vec!["bootstrap".to_string()]
        );
    }

    #[test]
    fn dynamic_inner_resume_pauses_cancelled_running_sibling() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut sibling = test_worktree_node("sibling");
        sibling.status = DynamicNodeStatus::Running;
        sibling.outcome = None;
        let mut graph = test_dynamic_graph(vec![target, sibling]);
        write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    "task-006",
                    "run-001",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "sibling",
                    "attempt-001",
                )
                .join("acp.session.json"),
            &serde_json::json!({
                "status": "cancelled",
                "stopReason": "cancelled",
                "sessionId": "session-sibling"
            }),
        )
        .unwrap();
        let resume = DynamicResumeOverride {
            node_id: "target".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            attachment_paths: Vec::new(),
        };

        assert!(normalize_stale_cancelled_dynamic_leaves_for_resume(
            &app,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            &mut graph,
        ));
        resume_paused_dynamic_graph(&mut graph, Some(&resume)).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(graph.run.current_node_ids, vec!["target".to_string()]);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[1].outcome, None);
    }

    #[test]
    fn dynamic_inner_resume_running_graph_rearms_only_target_node() {
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut other = test_worktree_node("other");
        other.status = DynamicNodeStatus::Running;
        let mut graph = test_dynamic_graph(vec![target, other]);
        let resume = DynamicResumeOverride {
            node_id: "target".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            attachment_paths: Vec::new(),
        };

        resume_paused_dynamic_graph(&mut graph, Some(&resume)).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(
            graph.run.current_node_ids,
            vec!["target".to_string(), "other".to_string()]
        );
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Running);
    }

    #[test]
    fn dynamic_node_paused_result_keeps_graph_running_when_sibling_is_active() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut paused = test_worktree_node("good-morning");
        paused.status = DynamicNodeStatus::Paused;
        paused.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut sibling = test_worktree_node("good-night");
        sibling.status = DynamicNodeStatus::Running;
        let mut graph = test_dynamic_graph(vec![test_worktree_node("good-morning"), sibling]);

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                result: Ok(DynamicExecutionResult {
                    node: paused,
                    proposals: Vec::new(),
                }),
            },
        )
        .unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(graph.run.current_node_ids, vec!["good-night".to_string()]);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Running);
    }

    #[test]
    fn interrupted_dynamic_worker_with_valid_completion_is_success() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("bootstrap");
        node.status = DynamicNodeStatus::Running;
        let attempt_id = dynamic_attempt_id(&node);
        let graph = test_dynamic_graph(vec![node.clone()]);
        write_json(
            &app.paths.dynamic_graph_file(
                "task-006",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
            ),
            &graph,
        )
        .unwrap();
        let result = ProviderRunResult {
            status: ProviderRunStatus::Interrupted,
            exit_code: None,
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: DYNAMIC_COMPLETION_ARTIFACT.to_string(),
                    content: test_end_completion("already done"),
                }),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: SessionMode::New,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({ "sessionId": "bootstrap-session" })),
                open_command: None,
            }),
            stream_path: None,
        };

        finalize_dynamic_worker_result(&ctx, &mut node, &attempt_id, result).unwrap();
        let proposal = try_accept_interrupted_dynamic_completion(&ctx, &mut node, &attempt_id)
            .unwrap()
            .expect("valid interrupted completion is accepted");

        assert_eq!(node.status, DynamicNodeStatus::Completed);
        assert_eq!(node.outcome, Some(NodeOutcome::Success));
        assert_eq!(
            proposal.validation_status,
            DynamicProposalValidationStatus::Accepted
        );
        assert!(
            app.paths
                .dynamic_node_artifact_file(
                    "task-006",
                    "run-001",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "bootstrap",
                    "attempt-001",
                    DYNAMIC_COMPLETION_ARTIFACT,
                )
                .exists()
        );
        assert!(
            app.paths
                .dynamic_node_worker_ref_file(
                    "task-006",
                    "run-001",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "bootstrap",
                    "attempt-001",
                )
                .exists()
        );
    }

    #[test]
    fn interrupted_dynamic_worker_with_invalid_completion_stays_paused() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("bootstrap");
        node.status = DynamicNodeStatus::Running;
        let attempt_id = dynamic_attempt_id(&node);
        let graph = test_dynamic_graph(vec![node.clone()]);
        write_json(
            &app.paths.dynamic_graph_file(
                "task-006",
                "run-001",
                "round-001",
                "ai-dynamic",
                "attempt-001",
            ),
            &graph,
        )
        .unwrap();
        let result = ProviderRunResult {
            status: ProviderRunStatus::Interrupted,
            exit_code: None,
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: DYNAMIC_COMPLETION_ARTIFACT.to_string(),
                    content: "{\"version\":\"0.1\"".to_string(),
                }),
            }),
            worker_ref_seed: None,
            stream_path: None,
        };

        finalize_dynamic_worker_result(&ctx, &mut node, &attempt_id, result).unwrap();
        let proposal =
            try_accept_interrupted_dynamic_completion(&ctx, &mut node, &attempt_id).unwrap();

        assert!(proposal.is_none());
        assert_eq!(node.status, DynamicNodeStatus::Paused);
        assert_eq!(node.outcome, None);
    }

    #[test]
    fn dynamic_completed_result_from_process_interrupted_attempt_is_accepted() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_attempt(
            &app,
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
        );
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut completed = test_worktree_node("good-morning");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![test_worktree_node("good-morning")]);
        let dynamic_run_id = graph.run.id.clone();

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                result: Ok(DynamicExecutionResult {
                    node: completed,
                    proposals: vec![DynamicProposalState {
                        version: VERSION.to_string(),
                        id: "proposal-good-morning-001".to_string(),
                        dynamic_run_id,
                        source_node_id: "good-morning".to_string(),
                        artifact_path: Utf8PathBuf::from("artifact.json"),
                        raw_output_path: Utf8PathBuf::from("raw.txt"),
                        parsed: serde_json::json!({
                            "version": "0.1",
                            "kind": "dynamic-node-completion",
                            "status": "success",
                            "summary": "done",
                            "next": { "type": "end" }
                        }),
                        validation_status: DynamicProposalValidationStatus::Accepted,
                        validation_errors: Vec::new(),
                        materialized_event_ids: Vec::new(),
                        created_at: "2026-06-16T00:00:00Z".to_string(),
                    }],
                }),
            },
        )
        .unwrap();

        let run: RunState = read_json(&app.paths.run_file("task-006", "run-001")).unwrap();
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.pause_reason, None);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Completed);
        assert_eq!(graph.nodes[0].outcome, Some(NodeOutcome::Success));
        assert_eq!(graph.proposals.len(), 1);
    }

    #[test]
    fn dynamic_completed_result_from_error_blocked_attempt_is_ignored() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_attempt(&app, RunStatus::Paused, Some(PauseReason::ErrorBlocked));
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut completed = test_worktree_node("good-morning");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![test_worktree_node("good-morning")]);
        let dynamic_run_id = graph.run.id.clone();

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                result: Ok(DynamicExecutionResult {
                    node: completed,
                    proposals: vec![DynamicProposalState {
                        version: VERSION.to_string(),
                        id: "proposal-good-morning-001".to_string(),
                        dynamic_run_id,
                        source_node_id: "good-morning".to_string(),
                        artifact_path: Utf8PathBuf::from("artifact.json"),
                        raw_output_path: Utf8PathBuf::from("raw.txt"),
                        parsed: serde_json::json!({
                            "version": "0.1",
                            "kind": "dynamic-node-completion",
                            "status": "success",
                            "summary": "done",
                            "next": { "type": "end" }
                        }),
                        validation_status: DynamicProposalValidationStatus::Accepted,
                        validation_errors: Vec::new(),
                        materialized_event_ids: Vec::new(),
                        created_at: "2026-06-16T00:00:00Z".to_string(),
                    }],
                }),
            },
        )
        .unwrap();

        let run: RunState = read_json(&app.paths.run_file("task-006", "run-001")).unwrap();
        assert_eq!(run.status, RunStatus::Paused);
        assert_eq!(run.pause_reason, Some(PauseReason::ErrorBlocked));
        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[0].outcome, None);
        assert!(graph.proposals.is_empty());
    }

    #[test]
    fn dynamic_node_completed_result_emits_session_update() {
        let (_temp, repo_root) = init_repo();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = App::with_config(repo_root, RuntimeConfig::default()).with_acp_session_update(
            Arc::new(move |context| {
                seen_for_callback.lock().unwrap().push(context);
                Ok(())
            }),
        );
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut completed = test_worktree_node("good-morning");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![test_worktree_node("good-morning")]);

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                result: Ok(DynamicExecutionResult {
                    node: completed,
                    proposals: Vec::new(),
                }),
            },
        )
        .unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].task_id, "task-006");
        assert_eq!(calls[0].run_id, "run-001");
        assert_eq!(calls[0].round_id, "round-001");
        assert_eq!(calls[0].node_id, "good-morning");
        assert_eq!(calls[0].attempt_id, "attempt-001");
        assert_eq!(calls[0].outer_node_id.as_deref(), Some("ai-dynamic"));
        assert_eq!(calls[0].outer_attempt_id.as_deref(), Some("attempt-001"));
    }

    #[test]
    fn dynamic_node_paused_result_pauses_graph_when_no_active_leaf_remains() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut paused = test_worktree_node("good-night");
        paused.status = DynamicNodeStatus::Paused;
        paused.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![test_worktree_node("good-night")]);

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-night".to_string(),
                result: Ok(DynamicExecutionResult {
                    node: paused,
                    proposals: Vec::new(),
                }),
            },
        )
        .unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(
            graph.run.pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert!(graph.run.current_node_ids.is_empty());
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
    }

    #[test]
    fn dynamic_graph_with_only_paused_leaf_remaining_is_interrupted_not_error_blocked() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut paused = test_worktree_node("good-morning");
        paused.status = DynamicNodeStatus::Paused;
        paused.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut completed = test_worktree_node("good-night");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:01Z".to_string());
        let mut graph = test_dynamic_graph(vec![paused, completed]);
        graph.run.current_node_ids.clear();

        drive_dynamic_graph(&ctx, &mut graph).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(
            graph.run.pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert!(graph.run.current_node_ids.is_empty());
    }

    #[test]
    fn dynamic_node_job_error_pauses_graph_and_node() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let node = test_worktree_node("good-night");
        let mut graph = DynamicGraphState {
            version: VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-001".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "ai-dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: DynamicRunStatus::Running,
                outcome: None,
                pause_reason: None,
                started_at: "2026-06-16T00:00:00Z".to_string(),
                updated_at: "2026-06-16T00:00:00Z".to_string(),
                control: DynamicControlDsl::default(),
                allowed_workflow_snapshots: Vec::new(),
                current_node_ids: vec!["good-night".to_string()],
            },
            nodes: vec![node],
            groups: Vec::new(),
            proposals: Vec::new(),
        };

        let error = apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-night".to_string(),
                result: Err(anyhow!(
                    "failed to create dynamic worktree for `good-night`"
                )),
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "failed to create dynamic worktree for `good-night`"
        );
        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(graph.run.pause_reason, Some(PauseReason::ErrorBlocked));
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert!(graph.nodes[0].finished_at.is_some());
    }
}
