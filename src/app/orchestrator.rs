use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use jsonschema::JSONSchema;
use jsonschema::error::{ValidationError, ValidationErrorKind};
use parking_lot::ReentrantMutex;

use crate::acp::elicitation::cancel_pending_elicitation_requests;
use crate::acp::events::annotate_latest_runtime_control_output;
use crate::acp::permission::cancel_pending_permission_requests;
use crate::artifacts::parse_json_artifact;
use crate::config::DesktopLanguage;
use crate::control::{ControlDecision, decide_next_step};
use crate::domain::{
    InvocationKind, NodeOutcome, PauseReason, RoundTrigger, RunOutcome, RunStatus, SessionMode,
    TurnControlMode, VERSION,
};
use crate::dsl::{
    AiDynamicAgentStrategy, AiDynamicNode, NodeDsl, ValidatedWorkflow, WorkflowDsl,
    validate_authoring_workflow, validate_workflow, validate_workflow_snapshot,
    workflow_contains_ai_dynamic,
};
use crate::dynamic::{
    AllowedWorkflowSnapshot, DYNAMIC_COMPLETION_ARTIFACT, DynamicAgentTaskSpec,
    DynamicCompletionSchemaPolicy, DynamicCompletionStatus, DynamicGraphState, DynamicGroupState,
    DynamicGroupStatus, DynamicNext, DynamicNodeCompletion, DynamicNodeCompletionKind,
    DynamicNodeKind, DynamicNodeSpec, DynamicNodeSpecKind, DynamicNodeState, DynamicNodeStatus,
    DynamicProposalState, DynamicProposalValidationError, DynamicProposalValidationStatus,
    DynamicRunPhase, DynamicRunState, DynamicRunStatus, WorkspaceKind, WorkspaceOwnership,
    WorkspaceState, WorkspaceStatus, dynamic_completion_effective_schema,
    dynamic_graph_has_active_leaf, dynamic_leaf_is_active, dynamic_runtime_owns_leaf_projection,
    refresh_dynamic_current_leaf_ids, validate_dynamic_group_state, validate_dynamic_node_state,
    validate_dynamic_run_state, validate_workspace_state, validate_workspace_topology,
    write_dynamic_node_state,
};
use crate::dynamic_store::{
    CURRENT_DYNAMIC_GRAPH_VERSION, load_dynamic_graph, validate_dynamic_graph,
};
#[cfg(test)]
use crate::git::{GitCommandOutput, GitCommandRunner};
use crate::git::{GitRepositoryService, GitWorkspaceManager};
use crate::observability::{
    ExecutionContext, ProgressStage, append_run_event_best_effort, progress, run_event_data,
    write_progress_hint, write_run_progress_best_effort,
};
use crate::prompts::{
    AI_DYNAMIC_ACCEPTANCE_EN, AI_DYNAMIC_ACCEPTANCE_ZH_CN, AI_DYNAMIC_FANOUT_EN,
    AI_DYNAMIC_FANOUT_ZH_CN, AI_DYNAMIC_HIDDEN_CONTEXT_EN, AI_DYNAMIC_HIDDEN_CONTEXT_ZH_CN,
    AI_DYNAMIC_MERGE_EN, AI_DYNAMIC_MERGE_ZH_CN, AI_DYNAMIC_NODE_TASK_EN,
    AI_DYNAMIC_NODE_TASK_ZH_CN, AI_DYNAMIC_OUTPUT_PROTOCOL_EN, AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN,
    AI_DYNAMIC_PROPOSAL_REPAIR_EN, AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN, AI_DYNAMIC_SYSTEM_EN,
    AI_DYNAMIC_SYSTEM_ZH_CN, AI_DYNAMIC_WORKFLOW_INVOCATION_EN,
    AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN, PromptExecutionSurface, RUNTIME_CONTROL_RESUME_EN,
    RUNTIME_CONTROL_RESUME_WITH_MESSAGE_EN, RUNTIME_CONTROL_RESUME_WITH_MESSAGE_ZH_CN,
    RUNTIME_CONTROL_RESUME_ZH_CN, RUNTIME_INVALID_OUTPUT_REPAIR_EN,
    RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN, RUNTIME_WORKFLOW_RESUME_EN, RUNTIME_WORKFLOW_RESUME_ZH_CN,
    prompt_by_language, render as render_template,
};
use crate::provider::{
    ConversationPromptInput, OutputEmissionMode, PromptHiddenSection, PromptOutputContract,
    PromptRuntimeContext, PromptVisibility, ProviderRunResult, ProviderRunStatus,
    RuntimeControlIntent, StreamMode, UserPromptRenderMode, WorkerInvocation,
    conversation_prompt_text, render_prompt_bundle, supported_models_from_capabilities,
    supported_modes_from_capabilities,
};
use crate::runtime::{
    NodeState, RoundState, RoundTraceStep, RunState, RunWorktreeState, RuntimeAttemptLocator,
    RuntimeExecutionPhase, RuntimeExecutionState, TaskState, WorkerRefState, validate_round_state,
    validate_run_state, validate_worker_ref_state, write_node_state,
};
use crate::runtime_error::{
    RecoveryMode, RuntimeErrorDomain, RuntimeErrorInfo, blocked_runtime_error_info,
    manual_runtime_error_info, normalize_runtime_error, runtime_error,
};
use crate::storage::{append_jsonl, read_json, write_json};
use crate::workflow_model_binding::{TaskAuthoringWorkflow, validate_and_inject};

use super::ids::{
    generate_uuid, next_attempt_id, next_dynamic_resume_request_id, next_runtime_execution_id,
    now_rfc3339_like, reserve_next_run_dir,
};
use super::node_executor::{execute_ai_node, re_evaluate_attempt};
use super::profile_resolver::{resolve_profile_for_node, resolve_workflow_profiles};
use super::state_access::{
    current_attempt_state, load_run_workflow, persist_runtime_state,
    persist_runtime_state_if_execution_current,
    persist_runtime_state_if_expected_execution_current, refresh_runtime_execution_if_current,
};
use super::state_factory::create_node_state;
use super::transition_context::find_latest_worker_ref_for_transition;
use super::{
    AcpLiveEventContext, App, AttemptRuntimePauseResult, PreparedAcpPrompt,
    RuntimeInterventionKind, RuntimeLifecycleEvent, is_run_continuable,
};

struct PreparedRunData {
    validated: ValidatedWorkflow,
    resolved_profiles: super::profile_resolver::ResolvedWorkflowMetadata,
    run: RunState,
    round: RoundState,
    node: NodeState,
}

pub struct PreparedRun {
    data: Option<PreparedRunData>,
    run_dir: Utf8PathBuf,
    runtime_candidate: Option<super::RuntimeCandidateRegistration>,
}

impl PreparedRun {
    pub fn run(&self) -> &RunState {
        &self.data.as_ref().expect("prepared run data exists").run
    }

    pub fn accept(mut self) -> AcceptedRun {
        if let Some(runtime_candidate) = self.runtime_candidate.take() {
            runtime_candidate.commit();
        }
        AcceptedRun {
            data: self.data.take().expect("prepared run data exists"),
        }
    }
}

impl Drop for PreparedRun {
    fn drop(&mut self) {
        if let Some(runtime_candidate) = self.runtime_candidate.take() {
            let _ = runtime_candidate.abort();
        }
        if self.data.is_some() {
            let _ = std::fs::remove_dir_all(self.run_dir.as_std_path());
        }
    }
}

pub struct AcceptedRun {
    data: PreparedRunData,
}

impl AcceptedRun {
    pub fn run(&self) -> &RunState {
        &self.data.run
    }
}

struct NextExecution {
    node: NodeState,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct AcpInvocationPromptState {
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
    resume_prompt: Option<String>,
    resume_prompt_id: Option<String>,
    prompt_display: Option<ConversationPromptInput>,
    resume_prompt_visibility: PromptVisibility,
    user_prompt_render_mode: UserPromptRenderMode,
    runtime_control_intent: RuntimeControlIntent,
    input_attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
}

const MAX_INVALID_OUTPUT_REPAIR_PROMPTS: u32 = 3;
const MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS: u32 = 3;
const AUTO_RETRY_STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DYNAMIC_RUNTIME_CONTINUE_LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const DYNAMIC_BOOTSTRAP_NODE_ID: &str = "bootstrap";
static DYNAMIC_COMPLETION_SCHEMA_CACHE: OnceLock<Mutex<HashMap<String, Arc<JSONSchema>>>> =
    OnceLock::new();
static DYNAMIC_WORKTREE_GIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static DYNAMIC_STATE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<ReentrantMutex<()>>>>> =
    OnceLock::new();
#[derive(Default)]
struct DynamicResumeCoordinator {
    drivers: HashMap<String, DynamicResumeDriver>,
    pending: HashMap<String, Vec<DynamicResumeOverride>>,
    inflight: HashMap<String, HashMap<String, DynamicResumeLease>>,
    starting: HashSet<String>,
}

struct DynamicResumeDriver {
    id: String,
    sender: mpsc::Sender<DynamicResumeOverride>,
}

static DYNAMIC_RESUME_COORDINATOR: OnceLock<Mutex<DynamicResumeCoordinator>> = OnceLock::new();
static FIXED_RUN_CONTINUE_STARTING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

struct FixedRunContinueLease {
    key: String,
}

pub struct ManualCheckSubmissionLease {
    _continue_lease: FixedRunContinueLease,
}

#[derive(Clone)]
struct BackgroundDriveFallback {
    run: RunState,
    round: RoundState,
    node: NodeState,
}

#[derive(Debug)]
enum RuntimeContinueLaunch {
    Started(RunState),
    Failed(RuntimeErrorInfo),
}

fn runtime_continue_launch_error(error: &anyhow::Error) -> RuntimeErrorInfo {
    let cause = normalize_runtime_error(error);
    manual_runtime_error_info(
        RuntimeErrorDomain::Workflow,
        "runtime.continue-launch-failed",
        format!("runtime continue failed to launch: {error:#}"),
        serde_json::json!({
            "causeCode": cause.code_str(),
            "causeDomain": cause.domain,
        }),
    )
}

fn notify_runtime_continue_started(
    launch: &mut Option<mpsc::Sender<RuntimeContinueLaunch>>,
    run: &RunState,
) {
    if let Some(sender) = launch.take() {
        let _ = sender.send(RuntimeContinueLaunch::Started(run.clone()));
    }
}

fn notify_dynamic_resume_started(
    ctx: &DynamicExecutionContext<'_>,
    resume: &mut DynamicResumeOverride,
) -> bool {
    let key = dynamic_state_lock_key(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    let launch = match ctx.app.run_status(ctx.task_id, ctx.run_id) {
        Ok(run) => RuntimeContinueLaunch::Started(run),
        Err(error) => RuntimeContinueLaunch::Failed(runtime_continue_launch_error(&error)),
    };
    let started = matches!(launch, RuntimeContinueLaunch::Started(_));
    resume.launch_signal.take();
    let completed = complete_dynamic_resume_request(&key, &resume.request_id, Some(launch));
    started && completed
}

fn notify_dynamic_resume_failed(
    ctx: &DynamicExecutionContext<'_>,
    resume: &mut DynamicResumeOverride,
    info: RuntimeErrorInfo,
) {
    let key = dynamic_state_lock_key(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    resume.launch_signal.take();
    let _ = complete_dynamic_resume_request(
        &key,
        &resume.request_id,
        Some(RuntimeContinueLaunch::Failed(info)),
    );
}

fn wait_for_runtime_continue_launch(
    receiver: mpsc::Receiver<RuntimeContinueLaunch>,
) -> Result<RunState> {
    match receiver.recv() {
        Ok(RuntimeContinueLaunch::Started(run)) => Ok(run),
        Ok(RuntimeContinueLaunch::Failed(info)) => Err(runtime_error(info)),
        Err(error) => Err(runtime_error(manual_runtime_error_info(
            RuntimeErrorDomain::Internal,
            "runtime.continue-launch-channel-closed",
            format!("runtime continue launch channel closed before acknowledgement: {error}"),
            serde_json::json!({}),
        ))),
    }
}

fn dynamic_runtime_continue_stopped_info(node_id: &str, attempt_id: &str) -> RuntimeErrorInfo {
    manual_runtime_error_info(
        RuntimeErrorDomain::Workflow,
        "runtime.continue-stopped-before-start",
        "dynamic runtime continue was stopped before launch acknowledgement",
        serde_json::json!({
            "nodeId": node_id,
            "attemptId": attempt_id,
        }),
    )
}

fn dynamic_runtime_continue_timeout_info(request_id: &str, timeout: Duration) -> RuntimeErrorInfo {
    manual_runtime_error_info(
        RuntimeErrorDomain::Workflow,
        "runtime.continue-launch-timeout",
        "dynamic runtime continue did not acknowledge launch before the deadline",
        serde_json::json!({
            "requestId": request_id,
            "timeoutMs": timeout.as_millis(),
        }),
    )
}

fn dynamic_runtime_continue_superseded_info(request_id: &str, node_id: &str) -> RuntimeErrorInfo {
    manual_runtime_error_info(
        RuntimeErrorDomain::Workflow,
        "runtime.continue-superseded",
        "dynamic runtime continue was superseded by a newer lifecycle transition",
        serde_json::json!({
            "requestId": request_id,
            "nodeId": node_id,
        }),
    )
}

fn dynamic_runtime_continue_was_cancelled(error: &anyhow::Error) -> bool {
    matches!(
        normalize_runtime_error(error).code_str(),
        "runtime.continue-superseded"
            | "runtime.continue-stopped-before-start"
            | "runtime.continue-launch-timeout"
    )
}

impl FixedRunContinueLease {
    fn acquire(app: &App, task_id: &str, run_id: &str) -> Result<Self> {
        let key = format!("{}/{task_id}/{run_id}", app.paths.repo_root);
        let mut starting = FIXED_RUN_CONTINUE_STARTING
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .map_err(|_| anyhow!("fixed runtime continue registry poisoned"))?;
        if !starting.insert(key.clone()) {
            return Err(runtime_error(manual_runtime_error_info(
                RuntimeErrorDomain::Workflow,
                "runtime.continue-already-active",
                "runtime continue is already starting",
                serde_json::json!({ "taskId": task_id, "runId": run_id }),
            )));
        }
        Ok(Self { key })
    }
}

impl Drop for FixedRunContinueLease {
    fn drop(&mut self) {
        if let Some(starting) = FIXED_RUN_CONTINUE_STARTING.get()
            && let Ok(mut starting) = starting.lock()
        {
            starting.remove(&self.key);
        }
    }
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

fn localized_runtime_control_resume_prompt(language: DesktopLanguage) -> String {
    prompt_by_language(
        language,
        RUNTIME_CONTROL_RESUME_ZH_CN,
        RUNTIME_CONTROL_RESUME_EN,
    )
    .trim()
    .to_string()
}

fn localized_runtime_control_resume_with_message_prompt(
    language: DesktopLanguage,
    input: &ConversationPromptInput,
    artifact_emission_mode: Option<OutputEmissionMode>,
) -> String {
    render_template(
        prompt_by_language(
            language,
            RUNTIME_CONTROL_RESUME_WITH_MESSAGE_ZH_CN,
            RUNTIME_CONTROL_RESUME_WITH_MESSAGE_EN,
        ),
        serde_json::json!({
            "user_message": conversation_prompt_text(&input.display_text, &input.quotes),
            "artifact_emission_mode": artifact_emission_mode,
        }),
    )
    .expect("bundled runtime resume-with-message prompt renders")
    .trim()
    .to_string()
}

fn localized_workflow_resume_prompt(language: DesktopLanguage) -> String {
    prompt_by_language(
        language,
        RUNTIME_WORKFLOW_RESUME_ZH_CN,
        RUNTIME_WORKFLOW_RESUME_EN,
    )
    .trim()
    .to_string()
}

impl AcpInvocationPromptState {
    fn workflow_transition(
        language: DesktopLanguage,
        session_mode: SessionMode,
        continue_ref: Option<serde_json::Value>,
    ) -> Self {
        let (resume_prompt, user_prompt_render_mode) = match session_mode {
            SessionMode::New => (None, UserPromptRenderMode::RequirementTask),
            SessionMode::Continue => (
                Some(localized_workflow_resume_prompt(language)),
                UserPromptRenderMode::WorkflowResume,
            ),
        };
        Self {
            session_mode,
            continue_ref,
            resume_prompt,
            resume_prompt_id: None,
            prompt_display: None,
            resume_prompt_visibility: PromptVisibility::Visible,
            user_prompt_render_mode,
            runtime_control_intent: RuntimeControlIntent::Unchanged,
            input_attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
        }
    }

    fn runtime_control_resume(
        language: DesktopLanguage,
        continue_ref: Option<serde_json::Value>,
    ) -> Self {
        Self {
            session_mode: SessionMode::Continue,
            continue_ref,
            resume_prompt: Some(localized_runtime_control_resume_prompt(language)),
            resume_prompt_id: None,
            prompt_display: None,
            resume_prompt_visibility: PromptVisibility::Hidden,
            user_prompt_render_mode: UserPromptRenderMode::RuntimeResume,
            runtime_control_intent: RuntimeControlIntent::Resume,
            input_attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_continue_input_to_prompt_state(
    state: &mut AcpInvocationPromptState,
    language: DesktopLanguage,
    artifact_emission_mode: Option<OutputEmissionMode>,
    input: Option<ConversationPromptInput>,
    prompt_id: Option<String>,
    input_attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
) {
    state.resume_prompt_id = prompt_id;
    state.input_attachment_paths = input_attachment_paths;
    state.model_override = model_override;
    state.permission_mode_override = permission_mode_override;

    if let Some(mut input) = input.filter(|value| {
        !value.display_text.trim().is_empty() || !state.input_attachment_paths.is_empty()
    }) {
        input.display_text = input.display_text.trim().to_string();
        state.resume_prompt = Some(localized_runtime_control_resume_with_message_prompt(
            language,
            &input,
            artifact_emission_mode,
        ));
        state.prompt_display = Some(input);
        state.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
        state.resume_prompt_visibility = PromptVisibility::Visible;
    }
}

#[allow(clippy::too_many_arguments)]
fn runtime_control_resume_prompt_state(
    language: DesktopLanguage,
    continue_ref: serde_json::Value,
    artifact_emission_mode: Option<OutputEmissionMode>,
    input: Option<ConversationPromptInput>,
    prompt_id: Option<String>,
    input_attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
) -> AcpInvocationPromptState {
    let mut state = AcpInvocationPromptState::runtime_control_resume(language, Some(continue_ref));
    apply_continue_input_to_prompt_state(
        &mut state,
        language,
        artifact_emission_mode,
        input,
        prompt_id,
        input_attachment_paths,
        model_override,
        permission_mode_override,
    );
    state
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

fn clear_invalid_output_artifact_for_repair(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node: &NodeState,
) -> Result<()> {
    let Some(artifact_name) = node
        .resolved_config
        .get("outputArtifact")
        .and_then(|value| value.as_str())
    else {
        return Ok(());
    };
    let artifact_path = app.paths.artifact_file(
        task_id,
        run_id,
        round_id,
        &node.node_id,
        &node.attempt_id,
        artifact_name,
    );
    match std::fs::remove_file(artifact_path.as_std_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to clear invalid output artifact before repair: {}",
                artifact_path
            )
        }),
    }
}

pub(crate) fn run_start(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
) -> Result<RunState> {
    let PreparedRunData {
        validated,
        resolved_profiles,
        mut run,
        mut round,
        node,
    } = prepare_run(app, task_id, workflow_override)?.accept().data;
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
    let prepared = prepare_run(app, task_id, workflow_override)?.accept();
    launch_prepared_run_background(app, task_id, prepared)
}

fn terminalize_background_drive_error(
    app: &App,
    task_id: &str,
    fallback_run: &RunState,
    fallback_round: &RoundState,
    fallback_node: &NodeState,
    error: &anyhow::Error,
) {
    terminalize_background_drive_error_with_policy(
        app,
        task_id,
        fallback_run,
        fallback_round,
        fallback_node,
        BackgroundDriveFailurePolicy::ActiveExecution,
        error,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundDriveFailurePolicy {
    ActiveExecution,
    PausedManualCheck,
}

fn terminalize_manual_check_precommit_error(
    app: &App,
    task_id: &str,
    fallback_run: &RunState,
    fallback_round: &RoundState,
    fallback_node: &NodeState,
    error: &anyhow::Error,
) {
    terminalize_background_drive_error_with_policy(
        app,
        task_id,
        fallback_run,
        fallback_round,
        fallback_node,
        BackgroundDriveFailurePolicy::PausedManualCheck,
        error,
    );
}

fn terminalize_background_drive_error_with_policy(
    app: &App,
    task_id: &str,
    fallback_run: &RunState,
    fallback_round: &RoundState,
    fallback_node: &NodeState,
    policy: BackgroundDriveFailurePolicy,
    error: &anyhow::Error,
) {
    let convergence = match policy {
        BackgroundDriveFailurePolicy::ActiveExecution => {
            if let Some(execution_id) = fallback_node.runtime_execution_id.as_deref() {
                app.pause_attempt_runtime_state_if_active_execution(
                    task_id,
                    &fallback_run.id,
                    &fallback_round.id,
                    &fallback_node.node_id,
                    &fallback_node.attempt_id,
                    execution_id,
                    PauseReason::RuntimeAbnormal,
                )
            } else {
                app.pause_attempt_runtime_state_if_active_without_execution(
                    task_id,
                    &fallback_run.id,
                    &fallback_round.id,
                    &fallback_node.node_id,
                    &fallback_node.attempt_id,
                    PauseReason::RuntimeAbnormal,
                )
            }
        }
        BackgroundDriveFailurePolicy::PausedManualCheck => app
            .pause_attempt_runtime_state_if_paused_manual_check(
                task_id,
                &fallback_run.id,
                &fallback_round.id,
                &fallback_node.node_id,
                &fallback_node.attempt_id,
                PauseReason::RuntimeAbnormal,
            ),
    };

    match convergence {
        Ok(AttemptRuntimePauseResult::Converged) => {
            app.finish_runtime_candidate_best_effort(
                task_id,
                &fallback_run.id,
                fallback_run.execution.recovery_candidate_token.as_deref(),
            );
            emit_background_drive_error_from_fallback(
                app,
                task_id,
                fallback_run,
                fallback_round,
                fallback_node,
                error,
            );
        }
        Ok(AttemptRuntimePauseResult::Superseded) => {
            if policy == BackgroundDriveFailurePolicy::PausedManualCheck
                || background_drive_attempt_is_partially_converged(
                    app,
                    task_id,
                    fallback_run,
                    fallback_round,
                    fallback_node,
                )
            {
                if let Ok(run) = app.run_status(task_id, &fallback_run.id)
                    && run.status != RunStatus::Running
                {
                    app.finish_runtime_candidate_best_effort(
                        task_id,
                        &fallback_run.id,
                        run.execution.recovery_candidate_token.as_deref(),
                    );
                }
                emit_background_drive_error_from_fallback(
                    app,
                    task_id,
                    fallback_run,
                    fallback_round,
                    fallback_node,
                    error,
                );
            }
        }
        Err(convergence_error) => {
            if policy == BackgroundDriveFailurePolicy::ActiveExecution
                && background_drive_execution_was_superseded(
                    app,
                    task_id,
                    fallback_run,
                    fallback_round,
                    fallback_node,
                )
            {
                return;
            }
            let combined_error = anyhow!(
                "{error:#}; runtime failure convergence also failed: {convergence_error:#}"
            );
            emit_background_drive_error_from_fallback(
                app,
                task_id,
                fallback_run,
                fallback_round,
                fallback_node,
                &combined_error,
            );
        }
    }
}

fn background_drive_execution_was_superseded(
    app: &App,
    task_id: &str,
    fallback_run: &RunState,
    fallback_round: &RoundState,
    fallback_node: &NodeState,
) -> bool {
    let Ok(run) = app.run_status(task_id, &fallback_run.id) else {
        return false;
    };
    if background_drive_attempt_is_partially_converged(
        app,
        task_id,
        fallback_run,
        fallback_round,
        fallback_node,
    ) {
        return false;
    }
    let locator_matches = run.current_round.as_deref() == Some(fallback_round.id.as_str())
        && run.current_node.as_deref() == Some(fallback_node.node_id.as_str())
        && run.current_attempt.as_deref() == Some(fallback_node.attempt_id.as_str());
    if !locator_matches || run.status != RunStatus::Running {
        return true;
    }
    let node_path = app.paths.node_file(
        task_id,
        &fallback_run.id,
        &fallback_round.id,
        &fallback_node.node_id,
        &fallback_node.attempt_id,
    );
    let Ok(node) = read_json::<NodeState>(&node_path) else {
        return false;
    };
    node.runtime_execution_id != fallback_node.runtime_execution_id
}

fn background_drive_attempt_is_partially_converged(
    app: &App,
    task_id: &str,
    fallback_run: &RunState,
    fallback_round: &RoundState,
    fallback_node: &NodeState,
) -> bool {
    app.run_status(task_id, &fallback_run.id).is_ok_and(|run| {
        run.current_round.as_deref() == Some(fallback_round.id.as_str())
            && run.current_node.as_deref() == Some(fallback_node.node_id.as_str())
            && run.current_attempt.as_deref() == Some(fallback_node.attempt_id.as_str())
            && run.status == RunStatus::Paused
            && run.pause_reason == Some(PauseReason::RuntimeAbnormal)
    })
}

fn emit_background_drive_error_from_fallback(
    app: &App,
    task_id: &str,
    fallback_run: &RunState,
    fallback_round: &RoundState,
    fallback_node: &NodeState,
    error: &anyhow::Error,
) {
    let mut run = fallback_run.clone();
    let mut round = fallback_round.clone();
    let mut node = fallback_node.clone();
    let now = now_rfc3339_like();
    run.status = RunStatus::Paused;
    run.outcome = None;
    run.pause_reason = Some(PauseReason::RuntimeAbnormal);
    run.updated_at = now.clone();
    if run
        .transition_current_execution(RuntimeExecutionPhase::Paused, now.clone())
        .is_err()
    {
        return;
    }
    round.status = RunStatus::Paused;
    round.outcome = None;
    node.status = RunStatus::Paused;
    node.outcome = None;
    node.runtime_execution_id = None;
    node.finished_at = Some(now);
    emit_background_drive_error(app, task_id, &run, &round, &node, error);
}

fn emit_background_drive_error(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
    error: &anyhow::Error,
) {
    let runtime_error = normalize_runtime_error(error);
    let mut event_data = run_event_data(
        &ExecutionContext::for_run(task_id, &run.id)
            .with_round(round.id.clone())
            .with_node(node.node_id.clone())
            .with_attempt(node.attempt_id.clone()),
        Some(ProgressStage::Paused),
        Some(run.status),
        Some(format!("background execution failed: {error:#}")),
        run.pause_reason,
    );
    event_data.control_failure = Some(serde_json::json!({
        "runtimeError": runtime_error,
    }));
    append_run_event_best_effort(
        &app.paths,
        task_id,
        &run.id,
        "run_paused",
        run.updated_at.clone(),
        event_data,
    );
    emit_pause_side_effects(app, task_id, run, round, node);
}

pub(crate) fn launch_prepared_run_background(
    app: &App,
    task_id: &str,
    prepared: AcceptedRun,
) -> Result<RunState> {
    let initial_run = prepared.run().clone();
    let background_app = app.clone_for_background();
    let task_id = task_id.to_string();

    thread::Builder::new()
        .name("gold-band-run".to_string())
        .spawn(move || {
            let app = background_app;
            let PreparedRunData {
                validated,
                resolved_profiles,
                mut run,
                mut round,
                node,
            } = prepared.data;
            let fallback_node = node.clone();
            // 用 catch_unwind 包裹 drive_from_node：panic 也走 terminalize_background_drive_error，
            // 否则后台线程 panic 会跳过终止事件，导致订阅 scheduled_occurrence 的 occurrence 永久卡 running。
            let drive_result = catch_unwind(AssertUnwindSafe(|| {
                prepare_run_worktree_background(&app, &task_id, &mut run, &round, &node).and_then(
                    |ready| {
                        if !ready {
                            return Ok(());
                        }
                        drive_from_node(
                            &app,
                            &task_id,
                            &validated,
                            &resolved_profiles,
                            &mut run,
                            &mut round,
                            node,
                        )
                    },
                )
            }));
            let err = match drive_result {
                Ok(Ok(())) => None,
                Ok(Err(err)) => Some(err),
                Err(panic_payload) => Some(anyhow::anyhow!(
                    "background drive panicked: {}",
                    panic_payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic_payload.downcast_ref::<&'static str>().copied())
                        .unwrap_or("<non-string panic payload>")
                )),
            };
            if let Some(err) = err {
                terminalize_background_drive_error(
                    &app,
                    &task_id,
                    &run,
                    &round,
                    &fallback_node,
                    &err,
                );
                let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
                let _ = std::fs::write(
                    app.paths
                        .runs_dir(&task_id)
                        .join("desktop-start-error.txt")
                        .as_std_path(),
                    err.to_string(),
                );
            }
        })?;

    Ok(initial_run)
}

pub(crate) fn prepare_run(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
) -> Result<PreparedRun> {
    let workflow: WorkflowDsl = match workflow_override {
        Some(path) => read_json(path)?,
        None => app.executable_task_workflow(task_id)?,
    };
    prepare_run_from_workflow(app, task_id, workflow, None)
}

pub(crate) fn prepare_run_in_worktree(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
) -> Result<PreparedRun> {
    let workflow: WorkflowDsl = match workflow_override {
        Some(path) => read_json(path)?,
        None => app.executable_task_workflow(task_id)?,
    };
    let fork_commit = GitRepositoryService::default()
        .require_worktree(&app.paths.repo_root)?
        .head
        .ok_or_else(|| anyhow!("Git preflight returned no HEAD"))?;
    prepare_run_from_workflow(app, task_id, workflow, Some(fork_commit))
}

pub(crate) fn prepare_run_in_worktree_at(
    app: &App,
    task_id: &str,
    workflow_override: Option<&Utf8Path>,
    fork_commit: String,
) -> Result<PreparedRun> {
    let workflow: WorkflowDsl = match workflow_override {
        Some(path) => read_json(path)?,
        None => app.executable_task_workflow(task_id)?,
    };
    prepare_run_from_workflow(app, task_id, workflow, Some(fork_commit))
}

pub(crate) fn prepare_run_with_authoring(
    app: &App,
    task_id: &str,
    authoring: &TaskAuthoringWorkflow,
) -> Result<PreparedRun> {
    let validated_authoring = validate_authoring_workflow(authoring.workflow.clone())?;
    let workflow = validate_and_inject(
        &validated_authoring.raw,
        &authoring.model_bindings,
        &app.config.agents,
        &app.provider_diagnostics(),
    )?;
    prepare_run_from_workflow(app, task_id, workflow, None)
}

fn prepare_run_from_workflow(
    app: &App,
    task_id: &str,
    workflow: WorkflowDsl,
    worktree_fork_commit: Option<String>,
) -> Result<PreparedRun> {
    let validated = validate_workflow_snapshot(workflow)?;
    if worktree_fork_commit.is_none() && workflow_contains_ai_dynamic(&validated.raw) {
        GitRepositoryService::default().require_worktree(&app.paths.repo_root)?;
    }
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    write_json(
        &app.paths.task_workflow_resolved_file(task_id),
        &validated.raw,
    )?;
    write_json(&app.paths.task_provenance_file(task_id), &resolved_profiles)?;

    let (run_id, run_dir) = reserve_next_run_dir(&app.paths.runs_dir(task_id))?;
    let task_uuid = read_json::<TaskState>(&app.paths.task_file(task_id))?.uuid;
    let run_uuid = generate_uuid();
    let mut prepared = PreparedRun {
        data: None,
        run_dir,
        runtime_candidate: None,
    };
    let round_id = "round-001".to_string();
    let attempt_id = "attempt-001".to_string();
    let now = now_rfc3339_like();
    let worktree = if let Some(fork_commit) = worktree_fork_commit {
        Some(conversation_run_worktree_state(
            app,
            task_uuid.as_deref(),
            &run_uuid,
            fork_commit,
        ))
    } else {
        None
    };
    let creating_worktree = worktree.is_some();

    let execution_locator = RuntimeAttemptLocator {
        round_id: round_id.clone(),
        node_id: validated.raw.entry.clone(),
        attempt_id: attempt_id.clone(),
        outer_node_id: None,
        outer_attempt_id: None,
    };
    let mut run = RunState {
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
        uuid: Some(run_uuid),
        last_executed_node: None,
        worktree,
        execution: RuntimeExecutionState::new(
            if creating_worktree {
                RuntimeExecutionPhase::PreparingWorkspace
            } else {
                RuntimeExecutionPhase::StartingNode
            },
            Some(execution_locator),
            now.clone(),
        ),
    };
    let runtime_candidate = app.begin_runtime_candidate(task_id, &run_id)?;
    if let Some(runtime_candidate) = runtime_candidate.as_ref() {
        run.execution.recovery_candidate_token = Some(runtime_candidate.token().to_string());
    }
    validate_run_state(&run)?;
    write_json(&app.paths.run_file(task_id, &run_id), &run)?;
    prepared.runtime_candidate = runtime_candidate;
    write_json(
        &app.paths.workflow_snapshot_file(task_id, &run_id),
        &validated.raw,
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
    write_node_state(
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

    emit_run_metrics_fact(
        app,
        &run,
        super::observability::LifecycleEventType::ExecutionStarted,
        uuid::Uuid::new_v4().to_string(),
        run.started_at.clone(),
        None,
        None,
        None,
    );

    prepared.data = Some(PreparedRunData {
        validated,
        resolved_profiles,
        run,
        round,
        node,
    });
    Ok(prepared)
}

fn conversation_run_worktree_state(
    app: &App,
    task_uuid: Option<&str>,
    run_uuid: &str,
    fork_commit: String,
) -> RunWorktreeState {
    let mut hasher = blake3::Hasher::new();
    for identity_part in [
        app.paths.project_id.as_bytes(),
        task_uuid.unwrap_or_default().as_bytes(),
        run_uuid.as_bytes(),
    ] {
        hasher.update(&(identity_part.len() as u64).to_le_bytes());
        hasher.update(identity_part);
    }
    let digest = hasher.finalize().to_hex();
    let short_id = digest.as_str()[..16].to_string();
    RunWorktreeState {
        path: app.paths.conversation_worktrees_dir().join(&short_id),
        branch: format!("gold-band/conversation/{short_id}"),
        fork_commit,
    }
}

fn prepare_run_worktree_background(
    app: &App,
    task_id: &str,
    run: &mut RunState,
    round: &RoundState,
    node: &NodeState,
) -> Result<bool> {
    let Some(worktree) = run.worktree.clone() else {
        return Ok(true);
    };
    let preparation_started_at = Instant::now();
    tracing::info!(
        target: "gold_band::perf",
        task_id,
        run_id = run.id.as_str(),
        repository_root = app.paths.repo_root.as_str(),
        workspace_root = worktree.path.as_str(),
        branch = worktree.branch.as_str(),
        "conversation worktree preparation started"
    );
    let preparation_result = GitWorkspaceManager::default().ensure_worktree(
        &app.paths.repo_root,
        &worktree.path,
        &worktree.branch,
        &worktree.fork_commit,
    );
    tracing::info!(
        target: "gold_band::perf",
        task_id,
        run_id = run.id.as_str(),
        repository_root = app.paths.repo_root.as_str(),
        workspace_root = worktree.path.as_str(),
        branch = worktree.branch.as_str(),
        elapsed_ms = preparation_started_at.elapsed().as_millis(),
        status = if preparation_result.is_ok() { "ok" } else { "error" },
        "conversation worktree preparation completed"
    );
    preparation_result?;

    let state_lock = super::attempt_runtime_state_lock(
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
    let mut durable_run: RunState = read_json(&app.paths.run_file(task_id, &run.id))?;
    let durable_node: NodeState = read_json(&app.paths.node_file(
        task_id,
        &run.id,
        &round.id,
        &node.node_id,
        &node.attempt_id,
    ))?;
    if durable_run.status != RunStatus::Running
        || durable_run.current_round.as_deref() != Some(round.id.as_str())
        || durable_run.current_node.as_deref() != Some(node.node_id.as_str())
        || durable_run.current_attempt.as_deref() != Some(node.attempt_id.as_str())
        || durable_node.runtime_execution_id != node.runtime_execution_id
    {
        return Ok(false);
    }
    durable_run.updated_at = now_rfc3339_like();
    durable_run.transition_current_execution(
        RuntimeExecutionPhase::StartingNode,
        durable_run.updated_at.clone(),
    )?;
    validate_run_state(&durable_run)?;
    write_json(&app.paths.run_file(task_id, &run.id), &durable_run)?;
    *run = durable_run;
    Ok(true)
}

pub(crate) fn run_workspace_dir(app: &App, task_id: &str, run_id: &str) -> Result<Utf8PathBuf> {
    let run = app.run_status(task_id, run_id)?;
    let Some(worktree) = run.worktree else {
        return Ok(app.paths.repo_root.clone());
    };
    let expected_root = app.paths.conversation_worktrees_dir();
    let canonical_root = Utf8PathBuf::from_path_buf(std::fs::canonicalize(
        expected_root.as_std_path(),
    )?)
    .map_err(|path| {
        anyhow!(
            "managed conversation worktree root is not UTF-8: {}",
            path.display()
        )
    })?;
    let canonical_worktree =
        Utf8PathBuf::from_path_buf(std::fs::canonicalize(worktree.path.as_std_path())?)
            .map_err(|path| anyhow!("run worktree path is not UTF-8: {}", path.display()))?;
    ensure!(
        canonical_worktree.starts_with(&canonical_root),
        "run worktree is outside the managed conversation worktree root"
    );
    let worktree_manager = GitWorkspaceManager::default();
    worktree_manager.ensure_worktree_registration(&app.paths.repo_root, &worktree.path)?;
    worktree_manager.validate_worktree(&worktree.path, &worktree.branch)?;
    Ok(worktree.path)
}

#[derive(Debug, Clone)]
struct DynamicResumeOverride {
    request_id: String,
    execution_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    prompt_display: Option<ConversationPromptInput>,
    attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
    user_prompt_render_mode: UserPromptRenderMode,
    prompt_visibility: PromptVisibility,
    runtime_control_intent: RuntimeControlIntent,
    launch_signal: Option<mpsc::Sender<RuntimeContinueLaunch>>,
}

#[derive(Clone)]
struct DynamicResumeLease {
    request_id: String,
    execution_id: String,
    node_id: String,
    attempt_id: String,
    launch_signal: Option<mpsc::Sender<RuntimeContinueLaunch>>,
}

impl From<&DynamicResumeOverride> for DynamicResumeLease {
    fn from(resume: &DynamicResumeOverride) -> Self {
        Self {
            request_id: resume.request_id.clone(),
            execution_id: resume.execution_id.clone(),
            node_id: resume.node_id.clone(),
            attempt_id: resume.attempt_id.clone(),
            launch_signal: resume.launch_signal.clone(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn pause_dynamic_leaf_runtime_state(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    dynamic_attempt_id: &str,
    reason: PauseReason,
) -> Result<()> {
    pause_dynamic_leaf_runtime_state_with_policy(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        dynamic_node_id,
        Some(dynamic_attempt_id),
        reason,
        false,
        None,
    )
    .map(|_| ())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn pause_dynamic_leaf_runtime_state_if_active_execution(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    execution_id: &str,
    reason: PauseReason,
) -> Result<bool> {
    pause_dynamic_leaf_runtime_state_with_policy(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        dynamic_node_id,
        None,
        reason,
        true,
        Some(execution_id),
    )
}

#[allow(clippy::too_many_arguments)]
fn pause_dynamic_leaf_runtime_state_with_policy(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    expected_dynamic_attempt_id: Option<&str>,
    reason: PauseReason,
    require_active_leaf: bool,
    expected_execution_id: Option<&str>,
) -> Result<bool> {
    let graph_path =
        app.paths
            .dynamic_graph_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let state_lock = dynamic_state_lock_for(
        &app.paths.repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    )?;
    let _guard = state_lock.lock();
    if !require_active_leaf
        && load_dynamic_graph(&graph_path, &app.paths.repo_root).is_ok_and(|graph| {
            graph.run.status == DynamicRunStatus::Running
                && graph.run.phase == DynamicRunPhase::PreparingWorkspace
                && graph.nodes.iter().any(|node| {
                    node.id == dynamic_node_id && node.status == DynamicNodeStatus::Completed
                })
        })
    {
        app.run_pause(task_id, run_id, reason)?;
        if let Some(dynamic_attempt_id) = expected_dynamic_attempt_id {
            stop_dynamic_resume_target(
                &app.paths.repo_root,
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                dynamic_node_id,
                dynamic_attempt_id,
            )?;
        }
        return Ok(false);
    }
    let now = now_rfc3339_like();
    if graph_path.exists() {
        let mut graph = load_dynamic_graph(&graph_path, &app.paths.repo_root)?;
        let target_index = graph
            .nodes
            .iter()
            .position(|node| node.id == dynamic_node_id)
            .ok_or_else(|| anyhow!("dynamic node `{dynamic_node_id}` not found"))?;
        if let Some(expected_dynamic_attempt_id) = expected_dynamic_attempt_id
            && self::dynamic_attempt_id(&graph.nodes[target_index]) != expected_dynamic_attempt_id
        {
            return Ok(false);
        }
        if let Some(expected_execution_id) = expected_execution_id
            && graph.nodes[target_index].runtime_execution_id.as_deref()
                != Some(expected_execution_id)
        {
            return Ok(false);
        }
        if graph.nodes[target_index].status == DynamicNodeStatus::Completed {
            if let Some(dynamic_attempt_id) = expected_dynamic_attempt_id {
                stop_dynamic_resume_target(
                    &app.paths.repo_root,
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                    dynamic_node_id,
                    dynamic_attempt_id,
                )?;
            }
            return Ok(false);
        }
        if require_active_leaf
            && !matches!(
                graph.nodes[target_index].status,
                DynamicNodeStatus::Ready | DynamicNodeStatus::Running
            )
        {
            return Ok(false);
        }
        mark_dynamic_node_paused(&mut graph.nodes[target_index], reason, None);
        refresh_dynamic_current_leaf_ids(&mut graph);
        let has_active_leaf = dynamic_graph_has_active_leaf(&graph);
        if has_active_leaf {
            graph.run.status = DynamicRunStatus::Running;
            graph.run.phase = DynamicRunPhase::Executing;
            graph.run.outcome = None;
            graph.run.pause_reason = None;
        } else if graph.run.status == DynamicRunStatus::Running {
            graph.run.status = DynamicRunStatus::Paused;
            graph.run.phase = DynamicRunPhase::Executing;
            graph.run.outcome = None;
            graph.run.pause_reason = Some(reason);
        }
        graph.run.updated_at = now.clone();
        validate_dynamic_run_state(&graph.run)?;
        for node in &graph.nodes {
            validate_dynamic_node_state(node)?;
        }
        persist_dynamic_graph_for_resume_unlocked(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            &mut graph,
        )?;
        if has_active_leaf {
            if let Some(dynamic_attempt_id) = expected_dynamic_attempt_id {
                stop_dynamic_resume_target(
                    &app.paths.repo_root,
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                    dynamic_node_id,
                    dynamic_attempt_id,
                )?;
            }
            return Ok(true);
        }
    }

    let run_path = app.paths.run_file(task_id, run_id);
    if !run_path.exists() {
        return Ok(false);
    }
    let mut run: RunState = read_json(&run_path)?;
    let mut run_became_inactive = run.status != RunStatus::Running;
    if run.status == RunStatus::Running
        && run.current_round.as_deref() == Some(round_id)
        && run.current_node.as_deref() == Some(outer_node_id)
        && run.current_attempt.as_deref() == Some(outer_attempt_id)
    {
        run.status = RunStatus::Paused;
        run.outcome = None;
        run.pause_reason = Some(reason);
        run.updated_at = now.clone();
        run.transition_current_execution(RuntimeExecutionPhase::Paused, now.clone())?;
        validate_run_state(&run)?;
        write_json(&run_path, &run)?;
        run_became_inactive = true;
    }

    let round_path = app.paths.round_file(task_id, run_id, round_id);
    if round_path.exists() {
        let mut round: RoundState = read_json(&round_path)?;
        if round.status == RunStatus::Running {
            round.status = RunStatus::Paused;
            round.outcome = None;
            validate_round_state(&round)?;
            write_json(&round_path, &round)?;
        }
    }

    let outer_node_path =
        app.paths
            .node_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    if outer_node_path.exists() {
        let mut outer_node: NodeState = read_json(&outer_node_path)?;
        if outer_node.status != RunStatus::Completed {
            outer_node.status = RunStatus::Paused;
            outer_node.outcome = None;
            outer_node.finished_at = Some(now);
            crate::runtime::validate_node_state(&outer_node)?;
            write_node_state(&outer_node_path, &outer_node)?;
            write_run_progress_best_effort(
                &app.paths,
                task_id,
                &run,
                Some(outer_node.node_type),
                if reason == PauseReason::ErrorBlocked {
                    ProgressStage::Blocked
                } else {
                    ProgressStage::Paused
                },
                format!(
                    "run {} paused at {}/{}/{}",
                    run.id, round_id, outer_node_id, outer_attempt_id
                ),
            );
        }
    }

    let recovery_candidate_token = run.execution.recovery_candidate_token.clone();
    if let Some(dynamic_attempt_id) = expected_dynamic_attempt_id {
        stop_dynamic_resume_target(
            &app.paths.repo_root,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            dynamic_node_id,
            dynamic_attempt_id,
        )?;
    }
    drop(_guard);
    if run_became_inactive {
        app.finish_runtime_candidate_best_effort(
            task_id,
            run_id,
            recovery_candidate_token.as_deref(),
        );
    }
    Ok(true)
}

pub(crate) fn run_continue(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
    prompt: Option<String>,
    attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
) -> Result<RunState> {
    let _continue_lease = FixedRunContinueLease::acquire(app, task_id, run_id)?;
    run_continue_with_launch_signal(
        app,
        task_id,
        run_id,
        prompt_id,
        prompt.map(ConversationPromptInput::from),
        attachment_paths,
        model_override,
        permission_mode_override,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_continue_with_launch_signal(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
    input: Option<ConversationPromptInput>,
    attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
    launch: Option<mpsc::Sender<RuntimeContinueLaunch>>,
    expected_execution_id: Option<String>,
) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow_snapshot(workflow)?;
    if workflow_contains_ai_dynamic(&validated.raw) {
        GitRepositoryService::default().require_worktree(&app.paths.repo_root)?;
    }
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    let mut run = app.run_status(task_id, run_id)?;
    let current = current_attempt_state(app, task_id, &run)?;
    let (mut round, mut node) = current;
    if let Some(expected_execution_id) = expected_execution_id.as_deref() {
        ensure!(
            node.runtime_execution_id.as_deref() == Some(expected_execution_id),
            "runtime continue execution was superseded before launch"
        );
    }
    let claimed_runtime_resume = expected_execution_id.is_some()
        && run.status == RunStatus::Running
        && matches!(
            run.execution.phase,
            RuntimeExecutionPhase::StartingNode | RuntimeExecutionPhase::FinalizingArtifact
        );
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
        initial_prompt_display,
        initial_user_prompt_render_mode,
        initial_runtime_control_intent,
        initial_resume_input_attachment_paths,
        initial_parent_continue_input,
        initial_parent_continue_prompt_id,
        initial_model_override,
        initial_permission_mode_override,
    ) = match node.status {
        RunStatus::Paused => {
            if !is_run_continuable(&run) && !claimed_runtime_resume {
                bail!("current attempt is paused but not resumable by continue");
            }
            if node.manual_check_pending {
                bail!("current attempt is waiting for manual check");
            }
            match validated.get_node(&node.node_id) {
                Some(NodeDsl::AiDynamic(_)) => {
                    let parent_continue_input = input.clone();
                    let mut prompt_state = AcpInvocationPromptState::runtime_control_resume(
                        app.config.desktop_language,
                        None,
                    );
                    apply_continue_input_to_prompt_state(
                        &mut prompt_state,
                        app.config.desktop_language,
                        None,
                        input,
                        prompt_id,
                        attachment_paths,
                        model_override,
                        permission_mode_override,
                    );
                    let parent_continue_prompt_id = prompt_state.resume_prompt_id.clone();
                    (
                        prompt_state.session_mode,
                        prompt_state.continue_ref,
                        prompt_state.resume_prompt,
                        prompt_state.resume_prompt_id,
                        prompt_state.prompt_display,
                        prompt_state.user_prompt_render_mode,
                        prompt_state.runtime_control_intent,
                        prompt_state.input_attachment_paths,
                        parent_continue_input,
                        parent_continue_prompt_id,
                        prompt_state.model_override,
                        prompt_state.permission_mode_override,
                    )
                }
                Some(NodeDsl::Worker(worker)) => {
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
                    let prompt_state = runtime_control_resume_prompt_state(
                        app.config.desktop_language,
                        continue_ref,
                        worker
                            .output
                            .as_ref()
                            .map(|_| OutputEmissionMode::PostTurnProjection),
                        input,
                        prompt_id,
                        attachment_paths,
                        model_override,
                        permission_mode_override,
                    );
                    (
                        prompt_state.session_mode,
                        prompt_state.continue_ref,
                        prompt_state.resume_prompt,
                        prompt_state.resume_prompt_id,
                        prompt_state.prompt_display,
                        prompt_state.user_prompt_render_mode,
                        prompt_state.runtime_control_intent,
                        prompt_state.input_attachment_paths,
                        None,
                        None,
                        prompt_state.model_override,
                        prompt_state.permission_mode_override,
                    )
                }
                None => unreachable!("validated workflow contains the current node"),
            }
        }
        RunStatus::Completed if node.outcome == Some(NodeOutcome::Invalid) => {
            node = re_evaluate_attempt(app, task_id, &run.id, &round.id, node)?;
            (
                SessionMode::New,
                None,
                None,
                None,
                None,
                UserPromptRenderMode::RequirementTask,
                RuntimeControlIntent::Unchanged,
                Vec::new(),
                None,
                None,
                None,
                None,
            )
        }
        _ => bail!("current attempt is not continuable"),
    };

    let runtime_candidate = if !claimed_runtime_resume && run.status != RunStatus::Running {
        let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;
        if let Some(runtime_candidate) = runtime_candidate.as_ref() {
            run.execution.recovery_candidate_token = Some(runtime_candidate.token().to_string());
        }
        runtime_candidate
    } else {
        None
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
        initial_prompt_display,
        initial_user_prompt_render_mode,
        initial_resume_input_attachment_paths,
        initial_runtime_control_intent,
        initial_parent_continue_input,
        initial_parent_continue_prompt_id,
        None,
        initial_model_override,
        initial_permission_mode_override,
        launch,
        runtime_candidate,
    )?;
    Ok(run)
}

#[allow(clippy::too_many_arguments)]
fn run_continue_dynamic_inner(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node_id: &str,
    dynamic_attempt_id: &str,
    prompt_id: Option<String>,
    input: Option<ConversationPromptInput>,
    attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
    mut launch: Option<mpsc::Sender<RuntimeContinueLaunch>>,
    request_id: String,
    execution_id: String,
    outer_execution_id: String,
) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow_snapshot(workflow)?;
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
    let (dispatch, prompt_state, resume_override) = {
        let lock = dynamic_state_lock_for(
            &app.paths.repo_root,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
        )?;
        let _guard = lock.lock();
        let graph_path = app.paths.dynamic_graph_file(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
        );
        let mut graph = load_dynamic_graph(&graph_path, &app.paths.repo_root)?;
        if run.status == RunStatus::Paused
            && recover_legacy_cancelled_dynamic_leaves_for_paused_graph(
                app,
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                &mut graph,
            )
        {
            persist_dynamic_graph_for_resume_unlocked(
                app,
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                &mut graph,
            )?;
        }
        let target_index = graph
            .nodes
            .iter()
            .position(|node| node.id == dynamic_node_id)
            .ok_or_else(|| anyhow!("dynamic node `{dynamic_node_id}` not found"))?;
        ensure!(
            self::dynamic_attempt_id(&graph.nodes[target_index]) == dynamic_attempt_id,
            "dynamic attempt `{dynamic_attempt_id}` does not match target node"
        );
        ensure!(
            graph.nodes[target_index].status == DynamicNodeStatus::Paused
                && graph.nodes[target_index]
                    .pause_reason
                    .is_some_and(PauseReason::allows_explicit_runtime_continue),
            "dynamic node `{dynamic_node_id}` is not paused"
        );
        let target = &graph.nodes[target_index];
        let artifact_emission_mode = dynamic_output_emission_mode_for_node(target);
        let mut prompt_state =
            AcpInvocationPromptState::runtime_control_resume(app.config.desktop_language, None);
        if input.is_some() {
            apply_continue_input_to_prompt_state(
                &mut prompt_state,
                app.config.desktop_language,
                artifact_emission_mode,
                input,
                prompt_id.clone(),
                Vec::new(),
                None,
                None,
            );
        }
        let resume_override = DynamicResumeOverride {
            request_id: request_id.clone(),
            execution_id,
            node_id: dynamic_node_id.to_string(),
            attempt_id: dynamic_attempt_id.to_string(),
            prompt: prompt_state
                .resume_prompt
                .clone()
                .expect("continue prompt state always has a resume prompt"),
            prompt_id,
            prompt_display: prompt_state.prompt_display.clone(),
            attachment_paths,
            model_override,
            permission_mode_override,
            user_prompt_render_mode: prompt_state.user_prompt_render_mode,
            prompt_visibility: prompt_state.resume_prompt_visibility,
            runtime_control_intent: prompt_state.runtime_control_intent,
            launch_signal: launch.take(),
        };
        let dispatch = dispatch_registered_dynamic_resume_override(
            &app.paths.repo_root,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            resume_override.clone(),
        )?;
        (dispatch, prompt_state, resume_override)
    };
    if matches!(
        dispatch,
        DynamicResumeDispatch::Sent | DynamicResumeDispatch::QueuedStarting
    ) {
        return Ok(run);
    }
    let mut runtime_candidate = if run.status == RunStatus::Paused {
        let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;
        runtime_candidate
    } else {
        None
    };
    rearm_dynamic_outer_runtime_for_resume(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        &request_id,
        &outer_execution_id,
        &mut run,
        &mut round,
        &mut outer_node,
        &mut runtime_candidate,
    )?;
    outer_node.status = RunStatus::Paused;
    outer_node.outcome = None;
    outer_node.finished_at = None;
    let drive_result = drive_from_node_with_initial_session(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        outer_node,
        prompt_state.session_mode,
        prompt_state.continue_ref,
        prompt_state.resume_prompt,
        prompt_state.resume_prompt_id,
        prompt_state.prompt_display,
        prompt_state.user_prompt_render_mode,
        prompt_state.input_attachment_paths,
        prompt_state.runtime_control_intent,
        None,
        None,
        Some(resume_override),
        prompt_state.model_override,
        prompt_state.permission_mode_override,
        None,
        runtime_candidate,
    );
    drive_result?;
    let resume_key = dynamic_state_lock_key(
        &app.paths.repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    ensure!(
        !dynamic_resume_request_is_active(&resume_key, &request_id)?,
        runtime_error(manual_runtime_error_info(
            RuntimeErrorDomain::Workflow,
            "runtime.continue-driver-ended",
            "dynamic runtime driver ended before the requested leaf started",
            serde_json::json!({
                "requestId": request_id,
                "nodeId": dynamic_node_id,
            }),
        ))
    );
    Ok(run)
}

#[allow(clippy::too_many_arguments)]
fn rearm_dynamic_outer_runtime_for_resume(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    request_id: &str,
    outer_execution_id: &str,
    run: &mut RunState,
    round: &mut RoundState,
    outer_node: &mut NodeState,
    runtime_candidate: &mut Option<super::RuntimeCandidateRegistration>,
) -> Result<()> {
    if run.status == RunStatus::Running {
        return Ok(());
    }

    let state_lock = super::attempt_runtime_state_lock(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let _guard = state_lock
        .lock()
        .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
    let resume_key = dynamic_state_lock_key(
        &app.paths.repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    ensure!(
        dynamic_resume_request_is_active(&resume_key, request_id)?,
        runtime_error(dynamic_runtime_continue_superseded_info(
            request_id,
            outer_node_id,
        ))
    );

    let mut durable_run = app.run_status(task_id, run_id)?;
    ensure!(
        is_run_continuable(&durable_run)
            && durable_run.current_round.as_deref() == Some(round_id)
            && durable_run.current_node.as_deref() == Some(outer_node_id)
            && durable_run.current_attempt.as_deref() == Some(outer_attempt_id),
        "current run is not resumable by dynamic continue"
    );
    let mut durable_round: RoundState =
        read_json(&app.paths.round_file(task_id, run_id, round_id))?;
    let outer_node_path =
        app.paths
            .node_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id);
    let mut durable_outer_node: NodeState = read_json(&outer_node_path)?;
    ensure!(
        durable_outer_node.status == RunStatus::Paused
            && durable_outer_node.outcome.is_none()
            && !durable_outer_node.manual_check_pending,
        "current AI-DYNAMIC outer attempt is not resumable"
    );

    let previous_execution_id = durable_outer_node.runtime_execution_id.clone();
    durable_outer_node.runtime_execution_id = Some(outer_execution_id.to_string());
    let resumed_at = now_rfc3339_like();
    durable_run.status = RunStatus::Running;
    durable_run.outcome = None;
    durable_run.pause_reason = None;
    durable_run.updated_at = resumed_at.clone();
    if let Some(runtime_candidate) = runtime_candidate.as_ref() {
        durable_run.execution.recovery_candidate_token =
            Some(runtime_candidate.token().to_string());
    }
    durable_run.transition_current_execution(RuntimeExecutionPhase::StartingNode, resumed_at)?;
    validate_run_state(&durable_run)?;
    crate::runtime::validate_node_state(&durable_outer_node)?;
    write_node_state(&outer_node_path, &durable_outer_node)?;
    if let Err(error) = write_json(&app.paths.run_file(task_id, run_id), &durable_run) {
        durable_outer_node.runtime_execution_id = previous_execution_id;
        let _ = write_node_state(&outer_node_path, &durable_outer_node);
        return Err(error);
    }

    if let Some(runtime_candidate) = runtime_candidate.take() {
        runtime_candidate.commit();
    }
    durable_round.status = RunStatus::Running;
    *run = durable_run;
    *round = durable_round;
    *outer_node = durable_outer_node;
    Ok(())
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
    input: Option<ConversationPromptInput>,
    attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
) -> Result<RunState> {
    let request_id = next_dynamic_resume_request_id();
    let execution_id = next_runtime_execution_id();
    let outer_execution_id = next_runtime_execution_id();
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
    let background_task_id = task_id.to_string();
    let background_task_uuid = initial_run.task_uuid.clone();
    let background_run_id = run_id.to_string();
    let background_round_id = round_id.to_string();
    let background_outer_node_id = outer_node_id.to_string();
    let background_outer_attempt_id = outer_attempt_id.to_string();
    let background_dynamic_node_id = dynamic_node_id.to_string();
    let background_dynamic_attempt_id = dynamic_attempt_id.to_string();
    let background_request_id = request_id.clone();
    let background_execution_id = execution_id.clone();
    let background_outer_execution_id = outer_execution_id.clone();
    let (launch_sender, launch_receiver) = mpsc::channel();
    let launch_failure_sender = launch_sender.clone();
    let resume_key = reserve_dynamic_resume_request(
        &app.paths.repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        DynamicResumeLease {
            request_id: request_id.clone(),
            execution_id: execution_id.clone(),
            node_id: dynamic_node_id.to_string(),
            attempt_id: dynamic_attempt_id.to_string(),
            launch_signal: Some(launch_sender.clone()),
        },
    )?;
    let spawn_result = thread::Builder::new()
        .name("gold-band-dynamic-runtime-continue".to_string())
        .spawn(move || {
            let app = background_app;
            if let Err(err) = run_continue_dynamic_inner(
                &app,
                &background_task_id,
                &background_run_id,
                &background_round_id,
                &background_outer_node_id,
                &background_outer_attempt_id,
                &background_dynamic_node_id,
                &background_dynamic_attempt_id,
                prompt_id,
                input,
                attachment_paths,
                model_override,
                permission_mode_override,
                Some(launch_sender),
                background_request_id.clone(),
                background_execution_id.clone(),
                background_outer_execution_id.clone(),
            ) {
                if dynamic_runtime_continue_was_cancelled(&err) {
                    tracing::info!(
                        project_id = %app.paths.project_id,
                        task_id = %background_task_id,
                        run_id = %background_run_id,
                        round_id = %background_round_id,
                        node_id = %background_dynamic_node_id,
                        attempt_id = %background_dynamic_attempt_id,
                        outer_node_id = %background_outer_node_id,
                        outer_attempt_id = %background_outer_attempt_id,
                        request_id = %background_request_id,
                        execution_id = %background_execution_id,
                        "revoked dynamic conversation runtime continue exited before leaf launch"
                    );
                    return;
                }
                let info = runtime_continue_launch_error(&err);
                tracing::warn!(
                    project_id = %app.paths.project_id,
                    task_id = %background_task_id,
                    run_id = %background_run_id,
                    round_id = %background_round_id,
                    node_id = %background_dynamic_node_id,
                    attempt_id = %background_dynamic_attempt_id,
                    outer_node_id = %background_outer_node_id,
                    outer_attempt_id = %background_outer_attempt_id,
                    request_id = %background_request_id,
                    execution_id = %background_execution_id,
                    error_code = %info.code.code,
                    error_domain = ?info.domain,
                    error = %err,
                    "accepted dynamic conversation runtime continue failed in background"
                );
                let resume_key = dynamic_state_lock_key(
                    &app.paths.repo_root,
                    &background_task_id,
                    &background_run_id,
                    &background_round_id,
                    &background_outer_node_id,
                    &background_outer_attempt_id,
                );
                let failed_resumes =
                    fail_dynamic_resume_requests(&resume_key, |_| true, info.clone())
                        .unwrap_or_default();
                for mut resume in failed_resumes {
                    let _ = app.pause_dynamic_attempt_runtime_state_if_active_execution(
                        &background_task_id,
                        &background_run_id,
                        &background_round_id,
                        &background_outer_node_id,
                        &background_outer_attempt_id,
                        &resume.node_id,
                        &resume.execution_id,
                        PauseReason::RuntimeAbnormal,
                    );
                    if let Some(sender) = resume.launch_signal.take() {
                        let _ = sender.send(RuntimeContinueLaunch::Failed(info.clone()));
                    }
                }
                let converged = app
                    .pause_dynamic_attempt_runtime_state_if_active_execution(
                        &background_task_id,
                        &background_run_id,
                        &background_round_id,
                        &background_outer_node_id,
                        &background_outer_attempt_id,
                        &background_dynamic_node_id,
                        &background_execution_id,
                        PauseReason::RuntimeAbnormal,
                    )
                    .unwrap_or(false);
                let outer_converged = if converged {
                    false
                } else {
                    matches!(
                        app.pause_attempt_runtime_state_if_active_execution(
                            &background_task_id,
                            &background_run_id,
                            &background_round_id,
                            &background_outer_node_id,
                            &background_outer_attempt_id,
                            &background_outer_execution_id,
                            PauseReason::RuntimeAbnormal,
                        ),
                        Ok(AttemptRuntimePauseResult::Converged)
                    )
                };
                if converged || outer_converged {
                    let _ = app.emit_acp_session_update(AcpLiveEventContext {
                        task_id: background_task_id.clone(),
                        task_uuid: background_task_uuid.clone(),
                        run_id: background_run_id.clone(),
                        round_id: background_round_id.clone(),
                        node_id: background_dynamic_node_id.clone(),
                        attempt_id: background_dynamic_attempt_id.clone(),
                        outer_node_id: Some(background_outer_node_id.clone()),
                        outer_attempt_id: Some(background_outer_attempt_id.clone()),
                    });
                }
                let _ = launch_failure_sender.send(RuntimeContinueLaunch::Failed(info.clone()));
                let _ =
                    std::fs::create_dir_all(app.paths.runs_dir(&background_task_id).as_std_path());
                let _ = std::fs::write(
                    app.paths
                        .runs_dir(&background_task_id)
                        .join("desktop-dynamic-continue-error.txt")
                        .as_std_path(),
                    err.to_string(),
                );
            }
        });
    if let Err(error) = spawn_result {
        let error: anyhow::Error = error.into();
        let info = runtime_continue_launch_error(&error);
        tracing::warn!(
            project_id = %app.paths.project_id,
            %task_id,
            %run_id,
            %round_id,
            node_id = %dynamic_node_id,
            attempt_id = %dynamic_attempt_id,
            %outer_node_id,
            %outer_attempt_id,
            %request_id,
            %execution_id,
            error_code = %info.code.code,
            error_domain = ?info.domain,
            %error,
            "failed to start dynamic conversation runtime continue worker"
        );
        let _ = fail_dynamic_resume_requests(
            &resume_key,
            |request| request.request_id == request_id,
            info.clone(),
        );
        let _ = app.pause_dynamic_attempt_runtime_state_if_active_execution(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            dynamic_node_id,
            &execution_id,
            PauseReason::RuntimeAbnormal,
        );
        return Err(runtime_error(info));
    }
    let launch_result = wait_for_dynamic_runtime_continue_launch(
        launch_receiver,
        &resume_key,
        &request_id,
        DYNAMIC_RUNTIME_CONTINUE_LAUNCH_TIMEOUT,
    );
    if launch_result.as_ref().is_err_and(|error| {
        normalize_runtime_error(error).code_str() == "runtime.continue-launch-timeout"
    }) {
        let convergence = app.pause_attempt_runtime_state_if_active_execution(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            &outer_execution_id,
            PauseReason::ProcessInterrupted,
        );
        if matches!(convergence, Ok(AttemptRuntimePauseResult::Converged)) {
            let _ = app.emit_acp_session_update(AcpLiveEventContext {
                task_id: task_id.to_string(),
                task_uuid: initial_run.task_uuid.clone(),
                run_id: run_id.to_string(),
                round_id: round_id.to_string(),
                node_id: dynamic_node_id.to_string(),
                attempt_id: dynamic_attempt_id.to_string(),
                outer_node_id: Some(outer_node_id.to_string()),
                outer_attempt_id: Some(outer_attempt_id.to_string()),
            });
        }
    }
    launch_result
}

pub(crate) fn run_continue_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
    input: Option<ConversationPromptInput>,
    attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
) -> Result<RunState> {
    let continue_lease = FixedRunContinueLease::acquire(app, task_id, run_id)?;
    let initial_run = app.run_status(task_id, run_id)?;
    if !is_run_continuable(&initial_run) {
        bail!("current run is not resumable by continue");
    }
    let (round, mut node) = current_attempt_state(app, task_id, &initial_run)?;
    if node.manual_check_pending {
        bail!("current attempt is waiting for manual check");
    }
    let execution_id = next_runtime_execution_id();
    let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;
    {
        let state_lock = super::attempt_runtime_state_lock(
            app,
            task_id,
            run_id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        );
        let _guard = state_lock
            .lock()
            .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
        let mut current_run = app.run_status(task_id, run_id)?;
        ensure!(
            is_run_continuable(&current_run)
                && current_run.current_round.as_deref() == Some(round.id.as_str())
                && current_run.current_node.as_deref() == Some(node.node_id.as_str())
                && current_run.current_attempt.as_deref() == Some(node.attempt_id.as_str()),
            "current run is not resumable by continue"
        );
        node = read_json(&app.paths.node_file(
            task_id,
            run_id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        ))?;
        ensure!(
            node.status == RunStatus::Paused
                && node.outcome.is_none()
                && !node.manual_check_pending,
            "current attempt is not resumable by continue"
        );
        node.runtime_execution_id = Some(execution_id.clone());
        let resumed_at = now_rfc3339_like();
        let attempt_dir =
            app.paths
                .attempt_dir(task_id, run_id, &round.id, &node.node_id, &node.attempt_id);
        let resume_phase =
            if crate::provider::post_turn_projection_checkpoint_is_finalizing(&attempt_dir)? {
                RuntimeExecutionPhase::FinalizingArtifact
            } else {
                RuntimeExecutionPhase::StartingNode
            };
        current_run.status = RunStatus::Running;
        current_run.pause_reason = None;
        current_run.updated_at = resumed_at.clone();
        if let Some(runtime_candidate) = runtime_candidate.as_ref() {
            current_run.execution.recovery_candidate_token =
                Some(runtime_candidate.token().to_string());
        }
        current_run.transition_current_execution(resume_phase, resumed_at)?;
        crate::runtime::validate_node_state(&node)?;
        validate_run_state(&current_run)?;
        let node_path =
            app.paths
                .node_file(task_id, run_id, &round.id, &node.node_id, &node.attempt_id);
        write_node_state(&node_path, &node)?;
        if let Err(error) = write_json(&app.paths.run_file(task_id, run_id), &current_run) {
            node.runtime_execution_id = None;
            let _ = write_node_state(&node_path, &node);
            return Err(error);
        }
    }
    if let Some(runtime_candidate) = runtime_candidate {
        runtime_candidate.commit();
    }
    let background_app = app.clone_for_background();
    let task_uuid = initial_run.task_uuid.clone();
    let task_id = task_id.to_string();
    let run_id = run_id.to_string();
    let prompt_id = prompt_id.clone();
    let input = input.clone();
    let attachment_paths = attachment_paths.clone();
    let model_override = model_override.clone();
    let permission_mode_override = permission_mode_override.clone();
    let round_id = round.id;
    let node_id = node.node_id;
    let attempt_id = node.attempt_id;
    let background_execution_id = execution_id.clone();
    let log_project_id = app.paths.project_id.clone();
    let log_task_id = task_id.clone();
    let log_run_id = run_id.clone();
    let log_round_id = round_id.clone();
    let log_node_id = node_id.clone();
    let log_attempt_id = attempt_id.clone();
    let log_execution_id = execution_id.clone();
    let (launch_sender, launch_receiver) = mpsc::channel();
    let launch_failure_sender = launch_sender.clone();

    let spawn_result = thread::Builder::new()
        .name("gold-band-runtime-continue".to_string())
        .spawn(move || {
            let app = background_app;
            if let Err(err) = run_continue_with_launch_signal(
                &app,
                &task_id,
                &run_id,
                prompt_id,
                input,
                attachment_paths,
                model_override,
                permission_mode_override,
                Some(launch_sender),
                Some(background_execution_id.clone()),
            ) {
                let info = runtime_continue_launch_error(&err);
                tracing::warn!(
                    project_id = %app.paths.project_id,
                    %task_id,
                    %run_id,
                    %round_id,
                    %node_id,
                    %attempt_id,
                    execution_id = %background_execution_id,
                    error_code = %info.code.code,
                    error_domain = ?info.domain,
                    error = %err,
                    "accepted conversation runtime continue failed in background"
                );
                let convergence = app.pause_attempt_runtime_state_if_active_execution(
                    &task_id,
                    &run_id,
                    &round_id,
                    &node_id,
                    &attempt_id,
                    &background_execution_id,
                    PauseReason::RuntimeAbnormal,
                );
                if !matches!(&convergence, Ok(AttemptRuntimePauseResult::Superseded)) {
                    let _ = app.emit_acp_session_update(AcpLiveEventContext {
                        task_id: task_id.clone(),
                        task_uuid: task_uuid.clone(),
                        run_id: run_id.clone(),
                        round_id: round_id.clone(),
                        node_id: node_id.clone(),
                        attempt_id: attempt_id.clone(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    });
                }
                let _ = launch_failure_sender.send(RuntimeContinueLaunch::Failed(info.clone()));
                let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
                let convergence_details = convergence
                    .err()
                    .map(|error| format!("; runtime failure convergence also failed: {error:#}"))
                    .unwrap_or_default();
                let _ = std::fs::write(
                    app.paths
                        .runs_dir(&task_id)
                        .join("desktop-continue-error.txt")
                        .as_std_path(),
                    format!("{err:#}{convergence_details}"),
                );
            }
        });
    if let Err(error) = spawn_result {
        let error: anyhow::Error = error.into();
        let info = runtime_continue_launch_error(&error);
        tracing::warn!(
            project_id = %log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            execution_id = %log_execution_id,
            error_code = %info.code.code,
            error_domain = ?info.domain,
            %error,
            "failed to start conversation runtime continue worker"
        );
        return Err(runtime_error(info));
    }

    drop(initial_run);
    let launched_run = wait_for_runtime_continue_launch(launch_receiver);
    drop(continue_lease);
    launched_run
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_recover_completed_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    expected_revision: u64,
) -> Result<RunState> {
    let _continue_lease = FixedRunContinueLease::acquire(app, task_id, run_id)?;
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow_snapshot(workflow)?;
    if workflow_contains_ai_dynamic(&validated.raw) {
        GitRepositoryService::default().require_worktree(&app.paths.repo_root)?;
    }
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;

    let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;
    let state_lock =
        super::attempt_runtime_state_lock(app, task_id, run_id, round_id, node_id, attempt_id);
    let mut run;
    let mut round;
    let mut node;
    {
        let _guard = state_lock
            .lock()
            .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
        run = app.run_status(task_id, run_id)?;
        ensure!(
            run.current_round.as_deref() == Some(round_id)
                && run.current_node.as_deref() == Some(node_id)
                && run.current_attempt.as_deref() == Some(attempt_id),
            "runtime recovery locator is no longer current"
        );
        ensure!(
            run.execution.revision == expected_revision,
            "runtime recovery revision is stale"
        );
        (round, node) = current_attempt_state(app, task_id, &run)?;
        ensure!(
            super::is_completed_attempt_recoverable(&run, &node),
            "current completed attempt is not recoverable"
        );

        node.runtime_execution_id = Some(next_runtime_execution_id());
        run.status = RunStatus::Running;
        run.outcome = None;
        run.pause_reason = None;
        run.updated_at = now_rfc3339_like();
        if let Some(runtime_candidate) = runtime_candidate.as_ref() {
            run.execution.recovery_candidate_token = Some(runtime_candidate.token().to_string());
        }
        run.transition_current_execution(
            RuntimeExecutionPhase::Transitioning,
            run.updated_at.clone(),
        )?;
        round.status = RunStatus::Running;
        round.outcome = None;
        validate_run_state(&run)?;
        validate_round_state(&round)?;
        crate::runtime::validate_node_state(&node)?;
        persist_runtime_state(app, task_id, &run, &round, &node)?;
    }
    if let Some(runtime_candidate) = runtime_candidate {
        runtime_candidate.commit();
    }

    let attempt_dir = app
        .paths
        .attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
        .to_string();
    let completed_snapshot = completed_node_snapshot(&round, &node, Some(attempt_dir));
    let decision = decide_next_step(&validated, &run, &round, &node);
    let expected_execution_id = node.runtime_execution_id.clone();
    let Some(next) = apply_control_decision(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        &node,
        decision,
        expected_execution_id.as_deref(),
    )?
    else {
        if run.status != RunStatus::Running {
            app.finish_runtime_candidate_best_effort(
                task_id,
                run_id,
                run.execution.recovery_candidate_token.as_deref(),
            );
        }
        return Ok(run);
    };

    run.last_executed_node = Some(completed_snapshot);
    persist_runtime_state(app, task_id, &run, &round, &next.node)?;
    let initial_run = run.clone();
    let fallback_node = next.node.clone();
    let prompt_state = AcpInvocationPromptState::workflow_transition(
        app.config.desktop_language,
        next.session_mode,
        next.continue_ref,
    );
    let background_app = app.clone_for_background();
    let background_task_id = task_id.to_string();
    thread::Builder::new()
        .name("gold-band-runtime-recovery".to_string())
        .spawn(move || {
            let app = background_app;
            if let Err(error) = drive_from_node_with_initial_session(
                &app,
                &background_task_id,
                &validated,
                &resolved_profiles,
                &mut run,
                &mut round,
                next.node,
                prompt_state.session_mode,
                prompt_state.continue_ref,
                prompt_state.resume_prompt,
                prompt_state.resume_prompt_id,
                prompt_state.prompt_display,
                prompt_state.user_prompt_render_mode,
                prompt_state.input_attachment_paths,
                prompt_state.runtime_control_intent,
                None,
                None,
                None,
                prompt_state.model_override,
                prompt_state.permission_mode_override,
                None,
                None,
            ) {
                terminalize_background_drive_error(
                    &app,
                    &background_task_id,
                    &run,
                    &round,
                    &fallback_node,
                    &error,
                );
            }
        })?;
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
    let lease =
        reserve_manual_check_submission(app, task_id, run_id, round_id, node_id, attempt_id)?;
    let result = submit_manual_check_with_launch_signal(
        app, task_id, run_id, round_id, node_id, attempt_id, outcome, None, None,
    );
    drop(lease);
    result
}

fn submit_manual_check_with_launch_signal(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outcome: NodeOutcome,
    mut launch: Option<mpsc::Sender<RuntimeContinueLaunch>>,
    launch_fallback: Option<Arc<Mutex<Option<BackgroundDriveFallback>>>>,
) -> Result<RunState> {
    ensure!(
        matches!(outcome, NodeOutcome::Success | NodeOutcome::Failure),
        "manual check outcome must be success or failure"
    );
    validate_manual_check_submission(app, task_id, run_id, round_id, node_id, attempt_id)?;
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow_snapshot(workflow)?;
    app.validate_workflow_agents(&validated)?;
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    let mut run = app.run_status(task_id, run_id)?;
    let (mut round, mut node) = current_attempt_state(app, task_id, &run)?;
    let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;

    node.status = RunStatus::Completed;
    node.outcome = Some(outcome);
    node.manual_check_pending = false;
    node.finished_at = Some(now_rfc3339_like());
    run.status = RunStatus::Running;
    run.pause_reason = None;
    run.updated_at = now_rfc3339_like();
    if let Some(runtime_candidate) = runtime_candidate.as_ref() {
        run.execution.recovery_candidate_token = Some(runtime_candidate.token().to_string());
    }
    run.transition_current_execution(RuntimeExecutionPhase::Transitioning, run.updated_at.clone())?;
    round.status = RunStatus::Running;
    round.outcome = None;

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
    if let Some(runtime_candidate) = runtime_candidate {
        runtime_candidate.commit();
    }
    let decision = decide_next_step(&validated, &run, &round, &node);
    let next = apply_control_decision(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        &node,
        decision,
        None,
    )?;
    if next.is_none() && run.status != RunStatus::Running {
        app.finish_runtime_candidate_best_effort(
            task_id,
            run_id,
            run.execution.recovery_candidate_token.as_deref(),
        );
    }
    if let Some(next) = next.as_ref()
        && let Some(launch_fallback) = launch_fallback.as_ref()
    {
        *launch_fallback
            .lock()
            .map_err(|_| anyhow!("manual check launch fallback lock poisoned"))? =
            Some(BackgroundDriveFallback {
                run: run.clone(),
                round: round.clone(),
                node: next.node.clone(),
            });
    }
    notify_runtime_continue_started(&mut launch, &run);
    if let Some(next) = next {
        let prompt_state = AcpInvocationPromptState::workflow_transition(
            app.config.desktop_language,
            next.session_mode,
            next.continue_ref,
        );
        drive_from_node_with_initial_session(
            app,
            task_id,
            &validated,
            &resolved_profiles,
            &mut run,
            &mut round,
            next.node,
            prompt_state.session_mode,
            prompt_state.continue_ref,
            prompt_state.resume_prompt,
            prompt_state.resume_prompt_id,
            prompt_state.prompt_display,
            prompt_state.user_prompt_render_mode,
            prompt_state.input_attachment_paths,
            prompt_state.runtime_control_intent,
            None,
            None,
            None,
            prompt_state.model_override,
            prompt_state.permission_mode_override,
            None,
            None,
        )?;
    }
    Ok(run)
}

pub(crate) fn validate_manual_check_submission(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<()> {
    let run = app.run_status(task_id, run_id)?;
    ensure!(run.status == RunStatus::Paused, "run is not paused");
    ensure!(
        run.current_round.as_deref() == Some(round_id)
            && run.current_node.as_deref() == Some(node_id)
            && run.current_attempt.as_deref() == Some(attempt_id),
        "manual check can only be submitted for the current paused attempt"
    );
    let (round, node) = current_attempt_state(app, task_id, &run)?;
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
    Ok(())
}

pub(crate) fn reserve_manual_check_submission(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<ManualCheckSubmissionLease> {
    let continue_lease = FixedRunContinueLease::acquire(app, task_id, run_id)?;
    validate_manual_check_submission(app, task_id, run_id, round_id, node_id, attempt_id)?;
    Ok(ManualCheckSubmissionLease {
        _continue_lease: continue_lease,
    })
}

pub(crate) fn submit_manual_check_background(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outcome: NodeOutcome,
    _lease: ManualCheckSubmissionLease,
) -> Result<RunState> {
    let initial_run = app.run_status(task_id, run_id)?;
    let (initial_round, initial_node) = current_attempt_state(app, task_id, &initial_run)?;
    let background_app = app.clone_for_background();
    let task_id = task_id.to_string();
    let spawn_failure_task_id = task_id.clone();
    let run_id = run_id.to_string();
    let round_id = round_id.to_string();
    let node_id = node_id.to_string();
    let attempt_id = attempt_id.to_string();
    let fallback_run = initial_run.clone();
    let fallback_round = initial_round.clone();
    let fallback_node = initial_node.clone();
    let (launch_sender, launch_receiver) = mpsc::channel();
    let launch_failure_sender = launch_sender.clone();
    let launch_fallback = Arc::new(Mutex::new(None));
    let background_launch_fallback = launch_fallback.clone();

    if let Err(spawn_error) = thread::Builder::new()
        .name("gold-band-manual-check".to_string())
        .spawn(move || {
            let app = background_app;
            let submission = catch_unwind(AssertUnwindSafe(|| {
                submit_manual_check_with_launch_signal(
                    &app,
                    &task_id,
                    &run_id,
                    &round_id,
                    &node_id,
                    &attempt_id,
                    outcome,
                    Some(launch_sender),
                    Some(background_launch_fallback),
                )
            }));
            let error = match submission {
                Ok(Ok(_)) => None,
                Ok(Err(error)) => Some(error),
                Err(panic_payload) => Some(anyhow::anyhow!(
                    "manual check submission panicked: {}",
                    panic_payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic_payload.downcast_ref::<&'static str>().copied())
                        .unwrap_or("<non-string panic payload>")
                )),
            };
            if let Some(error) = error {
                let committed_fallback = launch_fallback
                    .lock()
                    .ok()
                    .and_then(|fallback| fallback.clone());
                if let Some(failure_fallback) = committed_fallback {
                    terminalize_background_drive_error(
                        &app,
                        &task_id,
                        &failure_fallback.run,
                        &failure_fallback.round,
                        &failure_fallback.node,
                        &error,
                    );
                } else {
                    terminalize_manual_check_precommit_error(
                        &app,
                        &task_id,
                        &fallback_run,
                        &fallback_round,
                        &fallback_node,
                        &error,
                    );
                }
                let info = runtime_continue_launch_error(&error);
                let _ = launch_failure_sender.send(RuntimeContinueLaunch::Failed(info));
                let _ = std::fs::create_dir_all(app.paths.runs_dir(&task_id).as_std_path());
                let _ = std::fs::write(
                    app.paths
                        .runs_dir(&task_id)
                        .join("desktop-manual-check-error.txt")
                        .as_std_path(),
                    error.to_string(),
                );
            }
        })
    {
        let error = anyhow::Error::from(spawn_error);
        terminalize_manual_check_precommit_error(
            app,
            &spawn_failure_task_id,
            &initial_run,
            &initial_round,
            &initial_node,
            &error,
        );
        return Err(runtime_error(runtime_continue_launch_error(&error)));
    }

    wait_for_runtime_continue_launch(launch_receiver)
}

pub(crate) fn run_retry(app: &App, task_id: &str, run_id: &str) -> Result<RunState> {
    let workflow = load_run_workflow(app, task_id, run_id)?;
    let validated = validate_workflow_snapshot(workflow)?;
    if workflow_contains_ai_dynamic(&validated.raw) {
        GitRepositoryService::default().require_worktree(&app.paths.repo_root)?;
    }
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
    let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;
    if let Some(runtime_candidate) = runtime_candidate.as_ref() {
        run.execution.recovery_candidate_token = Some(runtime_candidate.token().to_string());
    }
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
    drive_from_node_with_runtime_candidate(
        app,
        task_id,
        &validated,
        &resolved_profiles,
        &mut run,
        &mut round,
        fresh,
        runtime_candidate,
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
    run.transition_current_execution(RuntimeExecutionPhase::Terminal, now.clone())?;
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
    emit_run_completed_lifecycle_event(app, task_id, run, round, node, RunOutcome::Failure);
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

#[allow(clippy::too_many_arguments)]
fn attempt_execution_is_still_current_running(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    expected_execution_id: Option<&str>,
) -> Result<bool> {
    if !attempt_is_still_current_running(app, task_id, run_id, round_id, node_id, attempt_id)? {
        return Ok(false);
    }
    let Some(expected_execution_id) = expected_execution_id else {
        return Ok(true);
    };
    let node: NodeState = read_json(
        &app.paths
            .node_file(task_id, run_id, round_id, node_id, attempt_id),
    )?;
    Ok(node.runtime_execution_id.as_deref() == Some(expected_execution_id))
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
    let decided_at = now_rfc3339_like();
    let _ = cancel_pending_permission_requests(&attempt_dir, decided_at.clone());
    let _ = cancel_pending_elicitation_requests(&attempt_dir, decided_at);
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

fn metrics_user_id() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into())
}

fn metrics_pause_reason(reason: PauseReason) -> super::observability::MetricsPauseReason {
    use super::observability::MetricsPauseReason as M;
    match reason {
        PauseReason::WaitingForUserInput => M::WaitingForUserInput,
        PauseReason::PermissionRequested => M::PermissionRequested,
        PauseReason::RuntimeAbnormal => M::RuntimeAbnormal,
        PauseReason::ErrorBlocked => M::ErrorBlocked,
        PauseReason::ProcessInterrupted => M::ProcessInterrupted,
    }
}

fn failed_dynamic_metrics_unit(
    graph: &DynamicGraphState,
) -> Option<&crate::dynamic::DynamicNodeState> {
    graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.outcome,
                Some(NodeOutcome::Failure | NodeOutcome::Invalid | NodeOutcome::Killed)
            )
        })
        .max_by_key(|node| node.finished_at.as_deref())
}

fn dynamic_metrics_terminal_reason(
    node: &crate::dynamic::DynamicNodeState,
) -> super::observability::TerminalReason {
    match node.outcome {
        Some(NodeOutcome::Killed) => super::observability::TerminalReason::ProcessKilled,
        Some(NodeOutcome::Invalid) => super::observability::TerminalReason::ValidationError,
        Some(NodeOutcome::Failure) if node.kind == crate::dynamic::DynamicNodeKind::Acceptance => {
            crate::app::observability::TerminalReason::AcceptanceRejected
        }
        Some(NodeOutcome::Failure) if node.runtime_error.is_some() => {
            super::observability::TerminalReason::RuntimeError
        }
        _ => super::observability::TerminalReason::ExecutionFailed,
    }
}

fn metrics_intervention_kind(
    kind: RuntimeInterventionKind,
) -> super::observability::MetricsInterventionKind {
    use super::observability::MetricsInterventionKind as M;
    match kind {
        RuntimeInterventionKind::ManualDecisionRequired => M::ManualDecision,
        RuntimeInterventionKind::ElicitationRequested => M::Elicitation,
        RuntimeInterventionKind::PermissionRequested => M::Permission,
        RuntimeInterventionKind::RuntimeAbnormal => M::RuntimeAbnormal,
        RuntimeInterventionKind::ErrorBlocked => M::ErrorBlocked,
        RuntimeInterventionKind::ProcessInterrupted => M::ProcessInterrupted,
    }
}

fn emit_run_metrics_fact(
    app: &App,
    run: &RunState,
    event_type: super::observability::LifecycleEventType,
    _event_id: String,
    occurred_at: String,
    pause_reason: Option<PauseReason>,
    intervention_kind: Option<RuntimeInterventionKind>,
    outcome: Option<RunOutcome>,
) {
    if !app.metrics_collection_enabled() {
        return;
    }
    let (Some(task_uuid), Some(run_uuid)) = (run.task_uuid.clone(), run.uuid.clone()) else {
        return;
    };
    // Direct conversations own their per-turn lifecycle in the ACP command
    // path. Their internal worker run must not create a second delivery.
    if super::notification::direct_conversation_agent_label(app, &run.task_id).is_some() {
        return;
    }
    let path = app
        .paths
        .run_dir(&run.task_id, &run.id)
        .join("observability")
        .join(&task_uuid)
        .join(super::observability::OBSERVABILITY_SNAPSHOT_FILE);
    let state = app.update_observability_state(&task_uuid, path.clone(), |state| {
        state.next_revision();

        if event_type == super::observability::LifecycleEventType::ExecutionPaused {
            state.record_pause(true);
        }
        if event_type == super::observability::LifecycleEventType::ExecutionResumed {
            let cause = state
                .take_pending_resume_cause()
                .unwrap_or(super::observability::ResumeCause::AutomaticRecovery);
            state.record_resume(true, cause);
        }
    });
    let revision = state.event_revision;
    let current_node_state = run
        .current_round
        .as_deref()
        .zip(run.current_node.as_deref())
        .zip(run.current_attempt.as_deref())
        .and_then(|((round_id, node_id), attempt_id)| {
            read_json::<NodeState>(&app.paths.node_file(
                &run.task_id,
                &run.id,
                round_id,
                node_id,
                attempt_id,
            ))
            .ok()
        });
    let current_round_index = run
        .current_round
        .as_deref()
        .and_then(|round_id| {
            read_json::<RoundState>(&app.paths.round_file(&run.task_id, &run.id, round_id)).ok()
        })
        .map(|round| round.index);
    let is_auto = current_node_state
        .as_ref()
        .is_some_and(|node| node.node_type == crate::domain::NodeType::AiDynamic);
    let is_intermediate = matches!(
        event_type,
        super::observability::LifecycleEventType::ExecutionPaused
            | super::observability::LifecycleEventType::ExecutionResumed
            | super::observability::LifecycleEventType::InterventionRequested
    );
    let mut fact = super::observability::MetricsLifecycleFact::new(
        event_type,
        revision,
        occurred_at,
        metrics_user_id(),
        app.paths.repo_root.to_string(),
        if is_auto {
            super::observability::MetricsSessionMode::Auto
        } else {
            super::observability::MetricsSessionMode::Workflow
        },
        task_uuid.clone(),
        if is_auto {
            super::observability::ExecutionKind::OuterRun
        } else {
            super::observability::ExecutionKind::Run
        },
        task_uuid.clone(),
    );
    fact.task_title = task_title(app, &run.task_id);
    fact.pause_reason = pause_reason.map(metrics_pause_reason);
    fact.previous_pause_reason =
        if event_type == super::observability::LifecycleEventType::ExecutionResumed {
            pause_reason.map(metrics_pause_reason)
        } else {
            None
        };
    fact.intervention_kind = intervention_kind.map(metrics_intervention_kind);
    fact.collection_state_recovered = state.collection_state_recovered;
    if is_intermediate {
        if let Some(node) = &current_node_state {
            fact.round_index = current_round_index;
            fact.role_name = Some(node_label(node));
            fact.attempt_index =
                super::observability::attempt_index_from_local_id(&node.attempt_id);
            if is_auto {
                fact.node_id = node.uuid.clone();
                fact.attempt_id = node.uuid.as_ref().and_then(|uuid| {
                    super::observability::derive_attempt_id(uuid, &node.attempt_id)
                });
            } else {
                let logical = current_round_index
                    .and_then(|round_index| {
                        super::observability::derive_execution_id(
                            &run_uuid,
                            &format!("round:{round_index}:node:{}", node.node_id),
                        )
                    })
                    .or_else(|| node.uuid.clone());
                fact.node_id = logical;
                fact.attempt_id = node.uuid.clone();
            }
        }
    }
    if let Some(outcome) = outcome {
        fact.outcome = Some(match outcome {
            RunOutcome::Success => super::observability::ExecutionOutcome::Success,
            RunOutcome::Failure => super::observability::ExecutionOutcome::Failure,
            RunOutcome::Killed => super::observability::ExecutionOutcome::Killed,
        });
        fact.terminal_reason = Some(match outcome {
            RunOutcome::Success => super::observability::TerminalReason::Completed,
            RunOutcome::Failure => super::observability::TerminalReason::ExecutionFailed,
            RunOutcome::Killed => super::observability::TerminalReason::ProcessKilled,
        });
        if !is_auto {
            fact.round_count = Some(run.new_rounds_opened.saturating_add(1));
        }
        if outcome != RunOutcome::Success {
            let outer_attempt = run
                .current_round
                .as_deref()
                .zip(run.current_node.as_deref())
                .zip(run.current_attempt.as_deref());
            let failed_unit = is_auto
                .then(|| outer_attempt)
                .flatten()
                .and_then(|((round_id, node_id), attempt_id)| {
                    read_json::<DynamicGraphState>(&app.paths.dynamic_graph_file(
                        &run.task_id,
                        &run.id,
                        round_id,
                        node_id,
                        attempt_id,
                    ))
                    .ok()
                })
                .and_then(|graph| failed_dynamic_metrics_unit(&graph).cloned());
            if let Some(node) = failed_unit {
                fact.failed_attempt_id = node.uuid.as_deref().and_then(|execution_id| {
                    super::observability::derive_attempt_id(
                        execution_id,
                        &dynamic_attempt_id(&node),
                    )
                });
                fact.terminal_reason = Some(dynamic_metrics_terminal_reason(&node));
                fact.terminal_reason_code = node
                    .runtime_error
                    .as_ref()
                    .map(|error| error.code_str().to_string());
            } else {
                fact.failed_attempt_id =
                    outer_attempt.and_then(|((round_id, node_id), attempt_id)| {
                        read_json::<NodeState>(&app.paths.node_file(
                            &run.task_id,
                            &run.id,
                            round_id,
                            node_id,
                            attempt_id,
                        ))
                        .ok()
                        .and_then(|node| node.uuid)
                    });
            }
        }
        fact.counters = Some(state.counters.clone());
    }
    app.emit_lifecycle_event(RuntimeLifecycleEvent::MetricsFact(fact));
    if outcome.is_some() {
        app.release_observability_state(&task_uuid);
    }
}

fn emit_run_paused_lifecycle_event(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) {
    let reason = run.pause_reason.unwrap_or(PauseReason::ProcessInterrupted);
    let event_id = super::notification::make_dedup_key(
        &app.paths.project_id,
        &run.id,
        &round.id,
        &node.node_id,
        &node.attempt_id,
        reason,
    );
    let occurred_at = now_rfc3339_like();
    app.emit_lifecycle_event(RuntimeLifecycleEvent::RunPaused {
        event_id: event_id.clone(),
        occurred_at: occurred_at.clone(),
        scheduled_occurrence_id: None,
        project_id: app.paths.project_id.clone(),
        task_id: task_id.to_string(),
        task_uuid: run.task_uuid.clone(),
        run_id: run.id.clone(),
        round_id: round.id.clone(),
        node_id: node.node_id.clone(),
        attempt_id: node.attempt_id.clone(),
        node_label: node_label(node),
        pause_reason: reason,
        task_title: task_title(app, task_id),
    });
    emit_run_metrics_fact(
        app,
        run,
        super::observability::LifecycleEventType::ExecutionPaused,
        event_id,
        occurred_at,
        Some(reason),
        None,
        None,
    );
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
    let event_id = super::notification::make_dedup_key(
        &app.paths.project_id,
        &run.id,
        &round.id,
        &node.node_id,
        &node.attempt_id,
        pause_reason,
    );
    let occurred_at = now_rfc3339_like();
    app.emit_lifecycle_event(RuntimeLifecycleEvent::InterventionRequested {
        event_id: event_id.clone(),
        occurred_at: occurred_at.clone(),
        scheduled_occurrence_id: None,
        project_id: app.paths.project_id.clone(),
        task_id: task_id.to_string(),
        run_id: run.id.clone(),
        round_id: round.id.clone(),
        node_id: node.node_id.clone(),
        attempt_id: node.attempt_id.clone(),
        node_label: node_label(node),
        kind,
        task_title: task_title(app, task_id),
    });
    emit_run_metrics_fact(
        app,
        run,
        super::observability::LifecycleEventType::InterventionRequested,
        event_id,
        occurred_at,
        None,
        Some(kind),
        None,
    );
}

fn emit_run_completed_lifecycle_event(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
    outcome: RunOutcome,
) {
    let event_id = super::notification::make_completion_dedup_key(
        &app.paths.project_id,
        &run.id,
        &round.id,
        &node.node_id,
        &node.attempt_id,
    );
    let occurred_at = now_rfc3339_like();
    app.emit_lifecycle_event(RuntimeLifecycleEvent::RunCompleted {
        event_id: event_id.clone(),
        occurred_at: occurred_at.clone(),
        scheduled_occurrence_id: None,
        project_id: app.paths.project_id.clone(),
        task_id: task_id.to_string(),
        task_uuid: run.task_uuid.clone(),
        run_id: run.id.clone(),
        round_id: round.id.clone(),
        node_id: node.node_id.clone(),
        attempt_id: node.attempt_id.clone(),
        node_label: node_label(node),
        outcome,
        task_title: task_title(app, task_id),
        completion_agent_label: super::notification::direct_conversation_agent_label(app, task_id),
    });
    emit_run_metrics_fact(
        app,
        run,
        super::observability::LifecycleEventType::ExecutionCompleted,
        event_id,
        occurred_at,
        None,
        None,
        Some(outcome),
    );
    let _ = app.notify_prompt_turn_finished(
        crate::app::AcpLiveEventContext {
            task_id: task_id.to_string(),
            task_uuid: run.task_uuid.clone(),
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            node_id: node.node_id.clone(),
            attempt_id: node.attempt_id.clone(),
            outer_node_id: None,
            outer_attempt_id: None,
        },
        super::direct_conversation_agent_label(app, task_id)
            .map(|_| super::INITIAL_DIRECT_TURN_ID.to_string()),
        outcome == RunOutcome::Success,
    );
}

fn intervention_kind_for_pause(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) -> Option<RuntimeInterventionKind> {
    match run.pause_reason.unwrap_or(PauseReason::ProcessInterrupted) {
        PauseReason::WaitingForUserInput => Some(waiting_for_user_input_intervention_kind(
            app, task_id, run, round, node,
        )),
        reason @ (PauseReason::PermissionRequested
        | PauseReason::RuntimeAbnormal
        | PauseReason::ErrorBlocked) => Some(RuntimeInterventionKind::from(reason)),
        PauseReason::ProcessInterrupted => {
            (!attempt_was_user_cancelled(app, task_id, &run.id, &round.id, node))
                .then_some(RuntimeInterventionKind::ProcessInterrupted)
        }
    }
}

fn waiting_for_user_input_intervention_kind(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    node: &NodeState,
) -> RuntimeInterventionKind {
    if node.manual_check_pending {
        return RuntimeInterventionKind::ManualDecisionRequired;
    }
    let attempt_dir =
        app.paths
            .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
    if attempt_has_pending_elicitation(&attempt_dir) {
        return RuntimeInterventionKind::ElicitationRequested;
    }
    RuntimeInterventionKind::ManualDecisionRequired
}

fn attempt_has_pending_elicitation(attempt_dir: &camino::Utf8Path) -> bool {
    let Ok(entries) = std::fs::read_dir(attempt_dir.as_std_path()) else {
        return false;
    };
    entries.filter_map(|entry| entry.ok()).any(|entry| {
        entry
            .file_name()
            .to_str()
            .map(|name| name.starts_with("acp.elicitation-request.") && name.ends_with(".json"))
            .unwrap_or(false)
    })
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
    let Ok(graph) = load_dynamic_graph(&graph_path, &app.paths.repo_root) else {
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

fn dynamic_node_is_legacy_cancelled_active(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    dynamic_node: &DynamicNodeState,
) -> bool {
    dynamic_leaf_is_active(dynamic_node.status)
        && dynamic_node.outcome.is_none()
        && dynamic_node_attempt_was_cancelled(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            dynamic_node,
        )
}

fn recover_legacy_cancelled_dynamic_leaves_for_paused_graph(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &mut DynamicGraphState,
) -> bool {
    if graph.run.status != DynamicRunStatus::Paused {
        return false;
    }
    let mut changed = false;
    for node in &mut graph.nodes {
        if dynamic_node_is_legacy_cancelled_active(
            app,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node,
        ) {
            mark_dynamic_node_paused(node, PauseReason::ProcessInterrupted, None);
            changed = true;
        }
    }
    if changed {
        refresh_dynamic_current_leaf_ids(graph);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.phase = DynamicRunPhase::Executing;
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
        crate::acp::events::load_session_metadata_value(path, None)
            .ok()
            .is_some_and(|metadata| {
                metadata
                    .get("latestTurnStatus")
                    .and_then(serde_json::Value::as_str)
                    == Some("cancelled")
                    || metadata
                        .get("stopReason")
                        .or_else(|| metadata.get("stop_reason"))
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|reason| reason.eq_ignore_ascii_case("cancelled"))
            })
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
    expected_execution_id: Option<&str>,
) -> Result<Option<NextExecution>> {
    let state_lock = expected_execution_id.map(|_| {
        super::attempt_runtime_state_lock(
            app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        )
    });
    let _state_guard = state_lock
        .as_ref()
        .map(|lock| {
            lock.lock()
                .map_err(|_| anyhow!("attempt runtime state lock poisoned"))
        })
        .transpose()?;
    if let Some(expected_execution_id) = expected_execution_id {
        let durable_run = app.run_status(task_id, &run.id)?;
        let durable_node: NodeState = read_json(&app.paths.node_file(
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        ))?;
        if durable_run.status != RunStatus::Running
            || durable_run.current_round.as_deref() != Some(round.id.as_str())
            || durable_run.current_node.as_deref() != Some(node.node_id.as_str())
            || durable_run.current_attempt.as_deref() != Some(node.attempt_id.as_str())
            || durable_node.runtime_execution_id.as_deref() != Some(expected_execution_id)
        {
            return Ok(None);
        }
    }
    match decision {
        ControlDecision::TransitionToNode { node_id, session } => {
            run.transition_current_execution(
                RuntimeExecutionPhase::Transitioning,
                now_rfc3339_like(),
            )?;
            validate_run_state(run)?;
            persist_runtime_state(app, task_id, run, round, node)?;
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
            run.transition_current_execution(
                RuntimeExecutionPhase::LaunchingNextNode,
                run.updated_at.clone(),
            )?;
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
        ControlDecision::OpenNewRound { entry_node_id } => {
            run.transition_current_execution(
                RuntimeExecutionPhase::Transitioning,
                now_rfc3339_like(),
            )?;
            validate_run_state(run)?;
            persist_runtime_state(app, task_id, run, round, node)?;
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
                .get_node(&entry_node_id)
                .expect("validated new round entry exists");
            let next_attempt_id = "attempt-001".to_string();
            let next_profile = next_node_dsl
                .profile()
                .and_then(|name| resolve_profile_for_node(resolved_profiles, name));
            let next_node = create_node_state(
                &run.id,
                &round.id,
                &entry_node_id,
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
            run.transition_current_execution(
                RuntimeExecutionPhase::LaunchingNextNode,
                run.updated_at.clone(),
            )?;
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
            run.transition_current_execution(
                RuntimeExecutionPhase::Paused,
                run.updated_at.clone(),
            )?;
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
            run.transition_current_execution(
                RuntimeExecutionPhase::Terminal,
                run.updated_at.clone(),
            )?;
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
                run.task_uuid.clone(),
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
    drive_from_node_with_runtime_candidate(
        app,
        task_id,
        workflow,
        resolved_profiles,
        run,
        round,
        node,
        None,
    )
}

fn drive_from_node_with_runtime_candidate(
    app: &App,
    task_id: &str,
    workflow: &ValidatedWorkflow,
    resolved_profiles: &super::profile_resolver::ResolvedWorkflowMetadata,
    run: &mut RunState,
    round: &mut RoundState,
    node: NodeState,
    runtime_candidate: Option<super::RuntimeCandidateRegistration>,
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
        UserPromptRenderMode::RequirementTask,
        Vec::new(),
        RuntimeControlIntent::Unchanged,
        None,
        None,
        None,
        None,
        None,
        None,
        runtime_candidate,
    )
}

struct DynamicExecutionContext<'a> {
    app: &'a App,
    task_id: &'a str,
    run_id: &'a str,
    round_id: &'a str,
    outer_node_id: &'a str,
    outer_attempt_id: &'a str,
    /// Runtime generation that owns scheduler-side graph transitions. A stop
    /// clears it, and an explicit continue always allocates a different one.
    outer_runtime_execution_id: Option<String>,
    dynamic: &'a AiDynamicNode,
    // UUIDs from the outer run/round/node — used for metrics reporting
    task_uuid: Option<&'a str>,
    run_uuid: Option<&'a str>,
    round_uuid: Option<&'a str>,
    outer_node_uuid: Option<&'a str>,
    parent_continue_input: Option<ConversationPromptInput>,
    parent_continue_prompt_id: Option<String>,
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
    execution_id: Option<String>,
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

struct DynamicResumeRegistration {
    key: String,
    driver_id: String,
    app: App,
    task_id: String,
    run_id: String,
    round_id: String,
    outer_node_id: String,
    outer_attempt_id: String,
}

impl Drop for DynamicResumeRegistration {
    fn drop(&mut self) {
        let failed_resumes = unregister_dynamic_resume_driver(&self.key, &self.driver_id);
        for resume in failed_resumes {
            let _ = self
                .app
                .pause_dynamic_attempt_runtime_state_if_active_execution(
                    &self.task_id,
                    &self.run_id,
                    &self.round_id,
                    &self.outer_node_id,
                    &self.outer_attempt_id,
                    &resume.node_id,
                    &resume.execution_id,
                    PauseReason::RuntimeAbnormal,
                );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DynamicResumeDispatch {
    Sent,
    QueuedStarting,
    StartDriver,
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
        let workflow = crate::workflow_model_binding::validate_and_inject(
            &template.workflow,
            &template.model_bindings,
            &app.config.agents,
            &app.provider_diagnostics(),
        )?;
        let validated = validate_workflow(workflow)?;
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
    task_uuid: Option<String>,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) {
    let _ = app.emit_acp_session_update(AcpLiveEventContext {
        task_id: task_id.to_string(),
        task_uuid,
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
        task_uuid: ctx.task_uuid.map(str::to_string),
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

fn emit_dynamic_run_terminal_session_update_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) {
    let Some(node) = graph
        .nodes
        .last()
        .filter(|node| node.status == DynamicNodeStatus::Completed && node.outcome.is_some())
    else {
        return;
    };
    emit_dynamic_session_update_best_effort(ctx, &node.id, &dynamic_attempt_id(node));
}

fn advance_dynamic_leaf_runtime_lifecycle_revisions(
    graph: &mut DynamicGraphState,
    node_ids: &[String],
    updated_at: &str,
) -> Vec<String> {
    let mut advanced_node_ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for node_id in node_ids {
        if !seen.insert(node_id.as_str()) {
            continue;
        }
        let Some(node) = graph.nodes.iter_mut().find(|node| node.id == *node_id) else {
            continue;
        };
        node.advance_runtime_lifecycle_revision(updated_at.to_string());
        advanced_node_ids.push(node.id.clone());
    }
    advanced_node_ids
}

fn begin_dynamic_workspace_transition_ownership(
    graph: &mut DynamicGraphState,
    owner_node_ids: &[String],
    updated_at: &str,
) -> Result<Vec<(String, Option<RuntimeExecutionPhase>)>> {
    ensure!(
        !owner_node_ids.is_empty(),
        "dynamic workspace transition requires at least one causal leaf"
    );
    let mut owners = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for node_id in owner_node_ids {
        if !seen.insert(node_id.as_str()) {
            continue;
        }
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.id == *node_id)
            .ok_or_else(|| anyhow!("dynamic workspace transition leaf `{node_id}` is missing"))?;
        owners.push((node.id.clone(), node.runtime_execution_phase));
        node.runtime_execution_phase = Some(RuntimeExecutionPhase::PreparingWorkspace);
        node.advance_runtime_lifecycle_revision(updated_at.to_string());
    }
    Ok(owners)
}

fn finish_dynamic_workspace_transition_ownership(
    graph: &mut DynamicGraphState,
    owners: &[(String, Option<RuntimeExecutionPhase>)],
    updated_at: &str,
) -> Result<Vec<String>> {
    let mut owner_node_ids = Vec::with_capacity(owners.len());
    for (node_id, previous_phase) in owners {
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.id == *node_id)
            .ok_or_else(|| anyhow!("dynamic workspace transition leaf `{node_id}` is missing"))?;
        node.runtime_execution_phase = *previous_phase;
        node.advance_runtime_lifecycle_revision(updated_at.to_string());
        owner_node_ids.push(node.id.clone());
    }
    Ok(owner_node_ids)
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

fn dynamic_permission_mode_for_provider(dynamic: &AiDynamicNode, provider: &str) -> Option<String> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed {
            permission_mode, ..
        } => permission_mode.clone(),
        AiDynamicAgentStrategy::Dynamic {
            available_agents, ..
        } => available_agents
            .iter()
            .find(|agent_ref| agent_ref.provider == provider)
            .and_then(|agent_ref| agent_ref.permission_mode.clone()),
    }
}

fn dynamic_control_provider(dynamic: &AiDynamicNode) -> &str {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, .. } => provider,
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_provider, ..
        } => bootstrap_provider,
    }
}

fn dynamic_control_permission_mode(dynamic: &AiDynamicNode) -> Option<String> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed {
            permission_mode, ..
        } => permission_mode.clone(),
        AiDynamicAgentStrategy::Dynamic {
            permission_mode, ..
        } => permission_mode.clone(),
    }
}

fn dynamic_config_options_for_invocation(
    dynamic: &AiDynamicNode,
    node: &DynamicNodeState,
) -> BTreeMap<String, String> {
    match &dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { .. } => dynamic.config_options.clone(),
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_config_options,
            acceptance_config_options,
            available_agents,
            ..
        } => {
            if node.id == DYNAMIC_BOOTSTRAP_NODE_ID {
                return bootstrap_config_options.clone();
            }
            match node.kind {
                DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => {
                    acceptance_config_options.clone()
                }
                DynamicNodeKind::Worker => node
                    .provider
                    .as_deref()
                    .and_then(|provider| {
                        available_agents
                            .iter()
                            .find(|agent| agent.provider == provider)
                    })
                    .map(|agent| agent.config_options.clone())
                    .unwrap_or_default(),
                DynamicNodeKind::WorkflowInvocation => BTreeMap::new(),
            }
        }
    }
}

fn resolve_dynamic_invocation_model(
    dynamic: &AiDynamicNode,
    node: &DynamicNodeState,
    model_override: Option<String>,
) -> Option<String> {
    model_override
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if node.id == DYNAMIC_BOOTSTRAP_NODE_ID {
                return node.model.clone().or_else(|| {
                    dynamic
                        .bootstrap_model()
                        .map(str::trim)
                        .filter(|model| !model.is_empty())
                        .map(str::to_string)
                });
            }
            match node.kind {
                DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => {
                    dynamic_acceptance_model(dynamic)
                        .map(ToOwned::to_owned)
                        .or_else(|| node.model.clone())
                }
                _ => node
                    .provider
                    .as_deref()
                    .and_then(|provider| dynamic_model_for_provider(dynamic, provider))
                    .or_else(|| node.model.clone()),
            }
        })
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
        AiDynamicAgentStrategy::Dynamic { .. } => false,
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
    supported_provider_model_options(ctx, provider)
        .into_iter()
        .map(|model| match (model.name, model.description) {
            (Some(name), Some(description)) => format!("{} ({name}) — {description}", model.id),
            (Some(name), None) => format!("{} ({name})", model.id),
            (None, Some(description)) => format!("{} — {description}", model.id),
            (None, None) => model.id,
        })
        .collect()
}

fn provider_diagnostic_capabilities(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> Option<serde_json::Value> {
    ctx.app
        .provider_diagnostics()
        .get(provider)
        .filter(|diagnostic| diagnostic.available)
        .and_then(|diagnostic| diagnostic.capabilities.clone())
}

fn supported_provider_model_options(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> Vec<crate::provider::AcpModeOption> {
    let capabilities = provider_diagnostic_capabilities(ctx, provider);
    supported_models_from_capabilities(capabilities.as_ref())
}

fn provider_model_option_values(ctx: &DynamicExecutionContext<'_>, provider: &str) -> Vec<String> {
    supported_provider_model_options(ctx, provider)
        .into_iter()
        .map(|model| model.id)
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
                && !provider_model_option_values(ctx, provider).is_empty()
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
                && !provider_model_option_values(ctx, provider).is_empty()
        }
    }
}

fn dynamic_proposed_model_validation_error(
    code: &str,
    message: String,
    provider: &str,
    field_owner: serde_json::Value,
    actual: Option<&str>,
    expected: &str,
    allowed_values: Vec<String>,
) -> DynamicProposalValidationError {
    let mut params = match field_owner {
        serde_json::Value::Object(map) => serde_json::Value::Object(map),
        _ => serde_json::json!({}),
    };
    if let Some(object) = params.as_object_mut() {
        object.insert("provider".to_string(), serde_json::json!(provider));
        object.insert("field".to_string(), serde_json::json!("model"));
        object.insert("expected".to_string(), serde_json::json!(expected));
        if let Some(actual) = actual {
            object.insert("actual".to_string(), serde_json::json!(actual));
        }
    }
    let mut error = dynamic_validation_error(code, message, params);
    error.allowed_values = allowed_values;
    error
}

fn dynamic_provider_requires_proposal_model_catalog(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
) -> bool {
    dynamic_worker_model_required_from_proposal(ctx, provider)
        || dynamic_agent_task_model_required_from_proposal(ctx, provider)
}

fn dynamic_catalog_missing_error(
    code_prefix: &str,
    label: &str,
    provider: &str,
    field_owner: serde_json::Value,
    actual: Option<&str>,
) -> DynamicProposalValidationError {
    dynamic_proposed_model_validation_error(
        &format!("{code_prefix}.model.catalog-missing"),
        format!(
            "{label} requires a model for provider `{provider}`, but the latest provider diagnostics has no model catalog"
        ),
        provider,
        field_owner,
        actual,
        "provider model catalog from agent diagnostics",
        Vec::new(),
    )
}

fn validate_dynamic_proposed_model(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
    proposed_model: Option<&str>,
    required: bool,
    code_prefix: &str,
    label: &str,
    field_owner: serde_json::Value,
) -> Option<DynamicProposalValidationError> {
    let allowed_values = provider_model_option_values(ctx, provider);
    if required && allowed_values.is_empty() {
        return Some(dynamic_catalog_missing_error(
            code_prefix,
            label,
            provider,
            field_owner,
            proposed_model,
        ));
    }
    if required && proposed_model.is_none() {
        return Some(dynamic_proposed_model_validation_error(
            &format!("{code_prefix}.model.required"),
            format!(
                "{label} must output model for provider `{provider}` because the AI-DYNAMIC config did not lock one"
            ),
            provider,
            field_owner,
            None,
            "one model value from the provider catalog",
            allowed_values,
        ));
    }
    if let Some(model) = proposed_model
        && !allowed_values.is_empty()
        && !allowed_values.iter().any(|allowed| allowed == model)
    {
        return Some(dynamic_proposed_model_validation_error(
            &format!("{code_prefix}.model.unsupported"),
            format!("{label} model `{model}` is not supported by provider `{provider}`"),
            provider,
            field_owner,
            Some(model),
            "one model value from the provider catalog",
            allowed_values,
        ));
    }
    None
}

fn dynamic_any_worker_model_required_from_proposal(ctx: &DynamicExecutionContext<'_>) -> bool {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { provider, .. } => {
            dynamic_worker_model_required_from_proposal(ctx, provider)
        }
        AiDynamicAgentStrategy::Dynamic { .. } => dynamic_available_provider_ids(ctx)
            .iter()
            .any(|provider| dynamic_worker_model_required_from_proposal(ctx, provider)),
    }
}

fn dynamic_model_policy_summary(ctx: &DynamicExecutionContext<'_>) -> String {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed {
            provider, model, ..
        } => {
            if let Some(model) = model.as_deref().filter(|model| !model.trim().is_empty()) {
                return format!(
                    "The fixed provider has configured model `{model}`; do not output `model`."
                );
            }
            if dynamic_worker_model_required_from_proposal(ctx, provider) {
                "The fixed provider has no configured model and exposes selectable models; output `model` for every worker / merge / acceptance node, using one model value from the provider list.".to_string()
            } else {
                "The fixed provider has no configured model catalog; do not output `model`, and runtime will use the provider default.".to_string()
            }
        }
        AiDynamicAgentStrategy::Dynamic { .. } => {
            if let Some(model) = dynamic_acceptance_model(ctx.dynamic) {
                format!(
                    "Select a provider only for workers. Runtime uses each worker provider's configured model; `merge` / `acceptance` always use the bootstrap Agent and configured acceptance model `{model}`. Do not output provider for merge / acceptance, and do not output `model`."
                )
            } else {
                "Select a provider only for workers. Runtime uses each worker provider's configured model; `merge` / `acceptance` always use the bootstrap Agent default model. Do not output provider for merge / acceptance, and do not output `model`.".to_string()
            }
        }
    }
}

fn dynamic_model_policy_summary_zh_cn(ctx: &DynamicExecutionContext<'_>) -> String {
    match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed {
            provider, model, ..
        } => {
            if let Some(model) = model.as_deref().filter(|model| !model.trim().is_empty()) {
                return format!("固定 provider 已配置模型 `{model}`；不要输出 `model`。");
            }
            if dynamic_worker_model_required_from_proposal(ctx, provider) {
                "固定 provider 未配置模型且提供了可选模型列表；每个 worker / merge / acceptance 节点都必须输出 `model`，并使用 provider 列表中的模型值。".to_string()
            } else {
                "固定 provider 没有可用模型列表；不要输出 `model`，runtime 会使用 provider 默认模型。".to_string()
            }
        }
        AiDynamicAgentStrategy::Dynamic { .. } => {
            if let Some(model) = dynamic_acceptance_model(ctx.dynamic) {
                format!(
                    "只为 worker 选择 provider。worker 使用该 provider 预先配置的模型；merge / acceptance 固定使用初始分发 Agent 和验收模型 `{model}`。不要为 merge / acceptance 输出 provider，也不要输出 `model`。"
                )
            } else {
                "只为 worker 选择 provider。worker 使用该 provider 预先配置的模型；merge / acceptance 固定使用初始分发 Agent 的默认模型。不要为 merge / acceptance 输出 provider，也不要输出 `model`。".to_string()
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
        AiDynamicAgentStrategy::Dynamic { .. } => false,
    };
    let any_model_visible = node_model_required || agent_task_model_required;
    if any_model_visible {
        for provider in &provider_ids {
            for model in provider_model_option_values(ctx, provider) {
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
        agent_task_model_visible: matches!(
            ctx.dynamic.agent_strategy,
            AiDynamicAgentStrategy::Fixed { .. }
        ),
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
    emission_mode: OutputEmissionMode,
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
        finalize_context: None,
        emission_mode,
    }
}

fn dynamic_node_is_bootstrap_dispatch(node: &DynamicNodeState) -> bool {
    node.id == DYNAMIC_BOOTSTRAP_NODE_ID
        && node.kind == DynamicNodeKind::Worker
        && node.depth == 0
        && node.group_id.is_none()
        && node.depends_on.is_empty()
        && node.chain_id == DYNAMIC_BOOTSTRAP_NODE_ID
}

fn dynamic_output_emission_mode(node: &DynamicNodeState) -> OutputEmissionMode {
    if dynamic_node_is_bootstrap_dispatch(node) {
        OutputEmissionMode::InlineControl
    } else {
        OutputEmissionMode::PostTurnProjection
    }
}

fn dynamic_output_emission_mode_for_node(node: &DynamicNodeState) -> Option<OutputEmissionMode> {
    dynamic_node_uses_completion_contract(node.kind).then(|| dynamic_output_emission_mode(node))
}

fn dynamic_output_contract_for_node(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> Option<PromptOutputContract> {
    dynamic_output_emission_mode_for_node(node)
        .map(|emission_mode| dynamic_output_contract(ctx, graph, emission_mode))
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
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> String {
    format!("{repo_root}/{task_id}/{run_id}/{round_id}/{outer_node_id}/{outer_attempt_id}")
}

fn dynamic_state_lock(ctx: &DynamicExecutionContext<'_>) -> Result<Arc<ReentrantMutex<()>>> {
    dynamic_state_lock_for(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    )
}

pub(crate) fn dynamic_state_lock_for(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> Result<Arc<ReentrantMutex<()>>> {
    let key = dynamic_state_lock_key(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let mut locks = DYNAMIC_STATE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("dynamic state lock registry poisoned"))?;
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(ReentrantMutex::new(())))
        .clone())
}

fn persist_dynamic_graph_for_resume_unlocked(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    graph: &DynamicGraphState,
) -> Result<()> {
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
        write_dynamic_node_state(
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
    let lock = dynamic_state_lock_for(
        &app.paths.repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    )?;
    let _guard = lock.lock();
    persist_dynamic_graph_for_resume_unlocked(
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        graph,
    )
}

fn register_dynamic_resume_channel(
    ctx: &DynamicExecutionContext<'_>,
    tx: mpsc::Sender<DynamicResumeOverride>,
) -> Result<(DynamicResumeRegistration, Vec<DynamicResumeOverride>)> {
    let key = dynamic_state_lock_key(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    let driver_id = generate_uuid();
    let mut coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    coordinator.drivers.insert(
        key.clone(),
        DynamicResumeDriver {
            id: driver_id.clone(),
            sender: tx,
        },
    );
    coordinator.starting.remove(&key);
    let pending = coordinator.pending.remove(&key).unwrap_or_default();
    drop(coordinator);
    Ok((
        DynamicResumeRegistration {
            key,
            driver_id,
            app: ctx.app.clone_for_background(),
            task_id: ctx.task_id.to_string(),
            run_id: ctx.run_id.to_string(),
            round_id: ctx.round_id.to_string(),
            outer_node_id: ctx.outer_node_id.to_string(),
            outer_attempt_id: ctx.outer_attempt_id.to_string(),
        },
        pending,
    ))
}

#[cfg(test)]
fn dispatch_dynamic_resume_override(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    resume: DynamicResumeOverride,
) -> Result<DynamicResumeDispatch> {
    dispatch_dynamic_resume_override_with_lease(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        resume,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_registered_dynamic_resume_override(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    resume: DynamicResumeOverride,
) -> Result<DynamicResumeDispatch> {
    dispatch_dynamic_resume_override_with_lease(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        resume,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn reserve_dynamic_resume_request(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    lease: DynamicResumeLease,
) -> Result<String> {
    let key = dynamic_state_lock_key(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let mut coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    let duplicate_target = coordinator.inflight.get(&key).is_some_and(|requests| {
        requests.values().any(|request| {
            request.request_id != lease.request_id
                && request.node_id == lease.node_id
                && request.attempt_id == lease.attempt_id
        })
    });
    if duplicate_target {
        return Err(runtime_error(manual_runtime_error_info(
            RuntimeErrorDomain::Workflow,
            "runtime.continue-already-active",
            "dynamic runtime continue is already active for this attempt",
            serde_json::json!({
                "nodeId": lease.node_id,
                "attemptId": lease.attempt_id,
            }),
        )));
    }
    coordinator
        .inflight
        .entry(key.clone())
        .or_default()
        .insert(lease.request_id.clone(), lease);
    Ok(key)
}

#[allow(clippy::too_many_arguments)]
fn dispatch_dynamic_resume_override_with_lease(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    resume: DynamicResumeOverride,
    require_reserved_lease: bool,
) -> Result<DynamicResumeDispatch> {
    let key = dynamic_state_lock_key(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    if !require_reserved_lease {
        reserve_dynamic_resume_request(
            repo_root,
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            DynamicResumeLease::from(&resume),
        )?;
    }
    let mut coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    if !coordinator
        .inflight
        .get(&key)
        .is_some_and(|requests| requests.contains_key(&resume.request_id))
    {
        return Err(runtime_error(dynamic_runtime_continue_superseded_info(
            &resume.request_id,
            &resume.node_id,
        )));
    }
    if let Some(driver) = coordinator.drivers.get(&key) {
        if driver.sender.send(resume.clone()).is_ok() {
            return Ok(DynamicResumeDispatch::Sent);
        }
        coordinator.drivers.remove(&key);
    }
    if !coordinator.starting.insert(key.clone()) {
        coordinator.pending.entry(key).or_default().push(resume);
        Ok(DynamicResumeDispatch::QueuedStarting)
    } else {
        Ok(DynamicResumeDispatch::StartDriver)
    }
}

fn complete_dynamic_resume_request(
    key: &str,
    request_id: &str,
    launch: Option<RuntimeContinueLaunch>,
) -> bool {
    let Some(coordinator) = DYNAMIC_RESUME_COORDINATOR.get() else {
        return false;
    };
    let Ok(mut coordinator) = coordinator.lock() else {
        return false;
    };
    let mut completed = None;
    let remove_graph = coordinator.inflight.get_mut(key).is_some_and(|requests| {
        completed = requests.remove(request_id);
        requests.is_empty()
    });
    if remove_graph {
        coordinator.inflight.remove(key);
    }
    let Some(mut completed) = completed else {
        return false;
    };
    if let (Some(sender), Some(launch)) = (completed.launch_signal.take(), launch) {
        return sender.send(launch).is_ok();
    }
    true
}

fn dynamic_resume_request_is_active(key: &str, request_id: &str) -> Result<bool> {
    let coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    Ok(coordinator
        .inflight
        .get(key)
        .is_some_and(|requests| requests.contains_key(request_id)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dynamic_resume_target_is_active(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<bool> {
    let key = dynamic_state_lock_key(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    Ok(coordinator.inflight.get(&key).is_some_and(|requests| {
        requests
            .values()
            .any(|request| request.node_id == node_id && request.attempt_id == attempt_id)
    }))
}

fn fail_dynamic_resume_requests(
    key: &str,
    mut matches: impl FnMut(&DynamicResumeLease) -> bool,
    info: RuntimeErrorInfo,
) -> Result<Vec<DynamicResumeLease>> {
    let mut coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    let mut failed = Vec::new();
    let remove_graph = coordinator.inflight.get_mut(key).is_some_and(|requests| {
        let request_ids = requests
            .iter()
            .filter(|(_, request)| matches(request))
            .map(|(request_id, _)| request_id.clone())
            .collect::<Vec<_>>();
        for request_id in request_ids {
            if let Some(request) = requests.remove(&request_id) {
                failed.push(request);
            }
        }
        requests.is_empty()
    });
    if let Some(pending) = coordinator.pending.get_mut(key) {
        pending.retain(|request| {
            !failed
                .iter()
                .any(|item| item.request_id == request.request_id)
        });
        if pending.is_empty() {
            coordinator.pending.remove(key);
        }
    }
    if remove_graph {
        coordinator.inflight.remove(key);
        coordinator.starting.remove(key);
    }
    for request in &mut failed {
        if let Some(sender) = request.launch_signal.take() {
            let _ = sender.send(RuntimeContinueLaunch::Failed(info.clone()));
        }
    }
    drop(coordinator);
    Ok(failed)
}

#[allow(clippy::too_many_arguments)]
fn stop_dynamic_resume_target(
    repo_root: &Utf8Path,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<usize> {
    let key = dynamic_state_lock_key(
        repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    );
    fail_dynamic_resume_requests(
        &key,
        |request| request.node_id == node_id && request.attempt_id == attempt_id,
        dynamic_runtime_continue_stopped_info(node_id, attempt_id),
    )
    .map(|failed| failed.len())
}

fn wait_for_dynamic_runtime_continue_launch(
    receiver: mpsc::Receiver<RuntimeContinueLaunch>,
    key: &str,
    request_id: &str,
    timeout: Duration,
) -> Result<RunState> {
    match receiver.recv_timeout(timeout) {
        Ok(RuntimeContinueLaunch::Started(run)) => Ok(run),
        Ok(RuntimeContinueLaunch::Failed(info)) => Err(runtime_error(info)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(runtime_error(manual_runtime_error_info(
            RuntimeErrorDomain::Internal,
            "runtime.continue-launch-channel-closed",
            "dynamic runtime continue launch channel closed before acknowledgement",
            serde_json::json!({ "requestId": request_id }),
        ))),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Launch acknowledgement and timeout revocation compete on the
            // in-memory request lease. The winner removes the lease and owns
            // the terminal signal; the leaf spawn path must claim that same
            // lease after persisting Running and immediately before spawn.
            let info = dynamic_runtime_continue_timeout_info(request_id, timeout);
            let failed = fail_dynamic_resume_requests(
                key,
                |request| request.request_id == request_id,
                info.clone(),
            )?;
            if failed.is_empty() {
                return match receiver.try_recv() {
                    Ok(RuntimeContinueLaunch::Started(run)) => Ok(run),
                    Ok(RuntimeContinueLaunch::Failed(info)) => Err(runtime_error(info)),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err(runtime_error(manual_runtime_error_info(
                            RuntimeErrorDomain::Internal,
                            "runtime.continue-launch-channel-closed",
                            "dynamic runtime continue launch channel closed during deadline resolution",
                            serde_json::json!({ "requestId": request_id }),
                        )))
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        Err(runtime_error(manual_runtime_error_info(
                            RuntimeErrorDomain::Internal,
                            "runtime.continue-launch-resolution-missing",
                            "dynamic runtime continue launch winner did not publish its acknowledgement atomically",
                            serde_json::json!({ "requestId": request_id }),
                        )))
                    }
                };
            }
            Err(runtime_error(info))
        }
    }
}

fn unregister_dynamic_resume_driver(key: &str, driver_id: &str) -> Vec<DynamicResumeLease> {
    let Some(coordinator) = DYNAMIC_RESUME_COORDINATOR.get() else {
        return Vec::new();
    };
    let Ok(mut coordinator) = coordinator.lock() else {
        return Vec::new();
    };
    if coordinator
        .drivers
        .get(key)
        .map(|driver| driver.id.as_str())
        != Some(driver_id)
    {
        return Vec::new();
    }
    coordinator.drivers.remove(key);
    coordinator.starting.remove(key);
    coordinator.pending.remove(key);
    let mut failed: Vec<DynamicResumeLease> = coordinator
        .inflight
        .remove(key)
        .map(|requests| requests.into_values().collect())
        .unwrap_or_default();
    for resume in &mut failed {
        if let Some(sender) = resume.launch_signal.take() {
            let _ = sender.send(RuntimeContinueLaunch::Failed(manual_runtime_error_info(
                RuntimeErrorDomain::Workflow,
                "runtime.continue-driver-ended",
                "dynamic runtime driver ended before the requested leaf started",
                serde_json::json!({
                    "requestId": resume.request_id,
                    "nodeId": resume.node_id,
                }),
            )));
        }
    }
    failed
}

fn clear_dynamic_resume_starting_window(key: &str) -> Result<Vec<DynamicResumeOverride>> {
    let mut coordinator = DYNAMIC_RESUME_COORDINATOR
        .get_or_init(|| Mutex::new(DynamicResumeCoordinator::default()))
        .lock()
        .map_err(|_| anyhow!("dynamic resume coordinator poisoned"))?;
    coordinator.starting.remove(key);
    Ok(coordinator.pending.remove(key).unwrap_or_default())
}

fn execute_ai_dynamic_node(
    app: &App,
    task_id: &str,
    run: &RunState,
    round: &RoundState,
    attempt_id: &str,
    dynamic: &AiDynamicNode,
    mut outer_node: NodeState,
    parent_continue_input: Option<ConversationPromptInput>,
    parent_continue_prompt_id: Option<String>,
    resume_override: Option<DynamicResumeOverride>,
) -> Result<NodeState> {
    let mut ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id: &run.id,
        round_id: &round.id,
        outer_node_id: &outer_node.node_id,
        outer_attempt_id: attempt_id,
        outer_runtime_execution_id: outer_node.runtime_execution_id.clone(),
        dynamic,
        task_uuid: run.task_uuid.as_deref(),
        run_uuid: run.uuid.as_deref(),
        round_uuid: round.uuid.as_deref(),
        outer_node_uuid: outer_node.uuid.as_deref(),
        parent_continue_input,
        parent_continue_prompt_id,
        resume_override,
    };
    let mut graph = {
        let state_lock = dynamic_state_lock(&ctx)?;
        let _guard = state_lock.lock();
        let mut graph = load_or_create_dynamic_graph(&ctx)?;
        if let Some(resume) = ctx.resume_override.as_ref() {
            let key = dynamic_state_lock_key(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
            );
            ensure!(
                dynamic_resume_request_is_active(&key, &resume.request_id)?,
                runtime_error(dynamic_runtime_continue_superseded_info(
                    &resume.request_id,
                    &resume.node_id,
                ))
            );
        }
        if let Some(resume) = ctx.resume_override.clone()
            && try_reconcile_dynamic_resume_completion(&ctx, &mut graph, &resume)?
        {
            ctx.resume_override = None;
        }
        resume_paused_dynamic_graph(&mut graph, ctx.resume_override.as_ref())?;
        persist_dynamic_graph(&ctx, &mut graph)?;
        graph
    };
    ctx.app.transition_runtime_execution_phase(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        RuntimeExecutionPhase::RunningNode,
    )?;
    ensure_dynamic_required_model_catalogs(&ctx, &mut graph)?;
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
            outer_node.outcome = Some(NodeOutcome::Failure);
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
    match target.status {
        DynamicNodeStatus::Paused => {
            ensure!(
                target
                    .pause_reason
                    .is_some_and(PauseReason::allows_explicit_runtime_continue),
                "dynamic node `{}` is not resumable by explicit continue",
                resume.node_id
            );
            target.begin_runtime_execution(resume.execution_id.clone(), now_rfc3339_like());
        }
        DynamicNodeStatus::Ready => {
            ensure!(
                target.runtime_execution_id.as_deref() == Some(resume.execution_id.as_str()),
                "dynamic resume request `{}` was superseded",
                resume.request_id
            );
        }
        _ => bail!("dynamic node `{}` is not paused", resume.node_id),
    }
    rearm_dynamic_node(target, DynamicNodeStatus::Ready);
    Ok(())
}

fn mark_dynamic_node_paused(
    node: &mut DynamicNodeState,
    pause_reason: PauseReason,
    runtime_error: Option<RuntimeErrorInfo>,
) {
    let now = now_rfc3339_like();
    node.status = DynamicNodeStatus::Paused;
    node.outcome = None;
    node.pause_reason = Some(pause_reason);
    node.runtime_error = runtime_error;
    node.pause_runtime_execution(now.clone());
    node.finished_at = Some(now);
}

fn rearm_dynamic_node(node: &mut DynamicNodeState, status: DynamicNodeStatus) {
    node.status = status;
    node.outcome = None;
    node.pause_reason = None;
    node.runtime_error = None;
    node.finished_at = None;
}

fn transition_dynamic_leaf_runtime_phase(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
    expected_execution_id: &str,
    phase: RuntimeExecutionPhase,
) -> Result<()> {
    {
        let state_lock = dynamic_state_lock(ctx)?;
        let _guard = state_lock.lock();
        let graph_path = ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        );
        let mut graph = load_dynamic_graph(&graph_path, &ctx.app.paths.repo_root)?;
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.id == node_id && dynamic_attempt_id(node) == attempt_id)
            .ok_or_else(|| anyhow!("dynamic node `{node_id}` attempt `{attempt_id}` not found"))?;
        ensure!(
            node.status == DynamicNodeStatus::Running,
            "dynamic leaf runtime phase requires a running node"
        );
        ensure!(
            node.transition_runtime_execution(expected_execution_id, phase, now_rfc3339_like(),)?,
            "dynamic leaf runtime phase was superseded by a newer execution"
        );
        validate_dynamic_node_state(node)?;
        let node_snapshot = node.clone();
        write_json(&graph_path, &graph)?;
        write_dynamic_node_state(
            &ctx.app.paths.dynamic_node_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                node_id,
            ),
            &node_snapshot,
        )?;
    }
    emit_dynamic_session_update_best_effort(ctx, node_id, attempt_id);
    Ok(())
}

fn refresh_dynamic_leaf_runtime_execution(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
) -> Result<()> {
    let state_lock = dynamic_state_lock(ctx)?;
    let _guard = state_lock.lock();
    let graph = load_dynamic_graph(
        &ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        &ctx.app.paths.repo_root,
    )?;
    let current = graph
        .nodes
        .iter()
        .find(|candidate| candidate.id == node.id)
        .ok_or_else(|| anyhow!("dynamic node `{}` not found", node.id))?;
    if current.runtime_execution_id == node.runtime_execution_id {
        node.runtime_execution_phase = current.runtime_execution_phase;
        node.runtime_lifecycle_revision = current.runtime_lifecycle_revision;
        node.runtime_lifecycle_updated_at = current.runtime_lifecycle_updated_at.clone();
    }
    Ok(())
}

fn dynamic_pause_reason_priority(reason: PauseReason) -> u8 {
    match reason {
        PauseReason::ErrorBlocked => 5,
        PauseReason::RuntimeAbnormal => 4,
        PauseReason::PermissionRequested => 3,
        PauseReason::WaitingForUserInput => 2,
        PauseReason::ProcessInterrupted => 1,
    }
}

fn aggregate_dynamic_pause_reason(graph: &DynamicGraphState) -> PauseReason {
    graph
        .nodes
        .iter()
        .filter(|node| node.status == DynamicNodeStatus::Paused && node.outcome.is_none())
        .filter_map(|node| node.pause_reason)
        .max_by_key(|reason| dynamic_pause_reason_priority(*reason))
        .or(graph.run.pause_reason)
        .unwrap_or(PauseReason::ProcessInterrupted)
}

fn apply_dynamic_resume_overrides(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    resumes: &mut Vec<DynamicResumeOverride>,
) -> Result<Vec<DynamicResumeOverride>> {
    let key = dynamic_state_lock_key(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    let mut applied_indexes = Vec::new();
    let mut launch_resumes = Vec::new();
    let mut reconciled_completion = false;
    for (index, resume) in resumes.iter().enumerate() {
        if !dynamic_resume_request_is_active(&key, &resume.request_id)? {
            // Stop/timeout already resolved the launch signal and revoked the
            // lease. Drop the driver's delayed local copy without re-arming
            // durable state or publishing a second terminal result.
            applied_indexes.push(index);
            continue;
        }
        let request_is_current = graph.nodes.iter().any(|node| {
            node.id == resume.node_id
                && dynamic_attempt_id(node) == resume.attempt_id
                && node.outcome.is_none()
                && (node.status == DynamicNodeStatus::Paused
                    && node
                        .pause_reason
                        .is_some_and(PauseReason::allows_explicit_runtime_continue)
                    || node.status == DynamicNodeStatus::Ready
                        && node.runtime_execution_id.as_deref()
                            == Some(resume.execution_id.as_str()))
        });
        if !request_is_current {
            let mut stale = resume.clone();
            notify_dynamic_resume_failed(
                ctx,
                &mut stale,
                dynamic_runtime_continue_superseded_info(&resume.request_id, &resume.node_id),
            );
            applied_indexes.push(index);
            continue;
        }
        if try_reconcile_dynamic_resume_completion(ctx, graph, resume)? {
            applied_indexes.push(index);
            reconciled_completion = true;
            continue;
        }
        rearm_dynamic_resume_target(graph, resume)?;
        applied_indexes.push(index);
        launch_resumes.push(resume.clone());
    }
    for index in applied_indexes.iter().rev() {
        resumes.remove(*index);
    }
    if graph.run.status == DynamicRunStatus::Paused || !applied_indexes.is_empty() {
        graph.run.status = DynamicRunStatus::Running;
        graph.run.phase = DynamicRunPhase::Executing;
        graph.run.outcome = None;
        graph.run.pause_reason = None;
    }
    graph.run.updated_at = now_rfc3339_like();
    refresh_dynamic_current_leaf_ids(graph);
    if reconciled_completion {
        persist_dynamic_graph(ctx, graph)?;
    }
    Ok(launch_resumes)
}

fn try_reconcile_dynamic_resume_completion(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    resume: &DynamicResumeOverride,
) -> Result<bool> {
    let Some(index) = graph
        .nodes
        .iter()
        .position(|node| node.id == resume.node_id)
    else {
        return Ok(false);
    };
    if dynamic_attempt_id(&graph.nodes[index]) != resume.attempt_id
        || !matches!(
            graph.nodes[index].status,
            DynamicNodeStatus::Paused | DynamicNodeStatus::Ready
        )
        || graph.nodes[index].outcome.is_some()
    {
        return Ok(false);
    }
    let mut node = graph.nodes[index].clone();
    let Some(proposal) =
        try_build_accepted_interrupted_dynamic_completion(ctx, &node, &resume.attempt_id, None)?
    else {
        return Ok(false);
    };
    // A reconciled completion does not spawn a new Agent. Claim the same
    // launch lease before applying any durable completion/workspace side
    // effects, so timeout/Stop and reconciliation have one terminal winner.
    // The outer Runtime is already durably Running at this point; subsequent
    // reconciliation failures are ordinary post-start Runtime failures.
    let mut started_resume = resume.clone();
    ensure!(
        notify_dynamic_resume_started(ctx, &mut started_resume),
        runtime_error(dynamic_runtime_continue_superseded_info(
            &resume.request_id,
            &resume.node_id,
        ))
    );
    let proposal =
        accept_interrupted_dynamic_completion(ctx, &mut node, &resume.attempt_id, None, proposal)?;
    graph.nodes[index] = node;
    graph.run.status = DynamicRunStatus::Running;
    graph.run.phase = DynamicRunPhase::Executing;
    graph.run.outcome = None;
    graph.run.pause_reason = None;
    let visible_node_ids = accept_dynamic_completion_proposal(ctx, graph, proposal)?;
    graph.run.updated_at = now_rfc3339_like();
    refresh_dynamic_current_leaf_ids(graph);
    append_dynamic_event(
        ctx,
        "dynamic_resume_completion_reconciled",
        serde_json::json!({
            "nodeId": resume.node_id,
            "attemptId": resume.attempt_id,
        }),
    )?;
    emit_dynamic_session_update_best_effort(ctx, &resume.node_id, &resume.attempt_id);
    emit_dynamic_session_updates_best_effort(ctx, graph, &visible_node_ids);
    Ok(true)
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
            rearm_dynamic_node(node, DynamicNodeStatus::Ready);
            changed = true;
        }
    }
    if changed {
        graph.run.status = DynamicRunStatus::Running;
        graph.run.phase = DynamicRunPhase::Executing;
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
    // A persisted preparation phase has no live owner after process recovery.
    // Explicit continue establishes a new execution generation and retries from
    // the durable graph/workspace catalog boundary.
    graph.run.phase = DynamicRunPhase::Executing;
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
        let _guard = lock.lock();
        let mut graph = load_dynamic_graph(&graph_path, &ctx.app.paths.repo_root)?;
        if reconcile_closed_dynamic_group_workspaces(ctx, &mut graph) {
            persist_dynamic_graph(ctx, &mut graph)?;
        }
        validate_dynamic_workspace_catalog(&graph)?;
        return Ok(graph);
    }

    let snapshots = freeze_allowed_workflow_snapshots(ctx.app, ctx.dynamic)?;
    let now = now_rfc3339_like();
    let dynamic_run_id = "dynamic-run-001".to_string();
    let run_workspace = run_workspace_dir(ctx.app, ctx.task_id, ctx.run_id)?;
    let capability = GitRepositoryService::default().require_worktree(&run_workspace)?;
    let main_workspace = WorkspaceState {
        version: VERSION.to_string(),
        id: "workspace-main".to_string(),
        dynamic_run_id: dynamic_run_id.clone(),
        kind: WorkspaceKind::Main,
        ownership: WorkspaceOwnership::User,
        repo_root: ctx.app.paths.repo_root.clone(),
        path: run_workspace,
        branch: None,
        parent_workspace_id: None,
        created_by_group_id: None,
        fork_commit: capability
            .head
            .ok_or_else(|| anyhow!("Git preflight returned no HEAD"))?,
        checkpoint_commit: None,
        status: WorkspaceStatus::Active,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let bootstrap = DynamicNodeState {
        version: VERSION.to_string(),
        acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
        id: DYNAMIC_BOOTSTRAP_NODE_ID.to_string(),
        dynamic_run_id: dynamic_run_id.clone(),
        kind: DynamicNodeKind::Worker,
        title: "AI-DYNAMIC bootstrap".to_string(),
        task: "Design the first internal dynamic step for this AI-DYNAMIC node.".to_string(),
        status: DynamicNodeStatus::Ready,
        outcome: None,
        pause_reason: None,
        runtime_error: None,
        runtime_execution_id: None,
        runtime_execution_phase: None,
        runtime_lifecycle_revision: 0,
        runtime_lifecycle_updated_at: None,
        group_id: None,
        chain_id: DYNAMIC_BOOTSTRAP_NODE_ID.to_string(),
        depth: 0,
        depends_on: Vec::new(),
        workspace_id: main_workspace.id.clone(),
        provider: ctx.dynamic.bootstrap_provider().map(ToOwned::to_owned),
        profile: None,
        permission_mode: dynamic_control_permission_mode(ctx.dynamic),
        model: ctx.dynamic.bootstrap_model().map(ToOwned::to_owned),
        session_mode: SessionMode::New,
        continue_from_node_id: None,
        workflow_id: None,
        workflow_snapshot_id: None,
        child_run_id: None,
        started_at: None,
        finished_at: None,
        uuid: Some(generate_uuid()),
    };
    let run = DynamicRunState {
        version: VERSION.to_string(),
        id: dynamic_run_id,
        parent_run_id: ctx.run_id.to_string(),
        parent_round_id: ctx.round_id.to_string(),
        parent_node_id: ctx.outer_node_id.to_string(),
        parent_attempt_id: ctx.outer_attempt_id.to_string(),
        status: DynamicRunStatus::Running,
        phase: DynamicRunPhase::Executing,
        outcome: None,
        pause_reason: None,
        started_at: now.clone(),
        updated_at: now,
        control: ctx.dynamic.control.clone(),
        allowed_workflow_snapshots: snapshots,
        current_node_ids: vec![bootstrap.id.clone()],
    };
    let graph = DynamicGraphState {
        version: CURRENT_DYNAMIC_GRAPH_VERSION.to_string(),
        run,
        nodes: vec![bootstrap],
        groups: Vec::new(),
        workspaces: vec![main_workspace],
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
    let (_resume_registration, pending_startup_resumes) =
        register_dynamic_resume_channel(ctx, resume_tx)?;
    let mut pending_resume_overrides = pending_startup_resumes;
    let mut launch_resume_overrides = ctx.resume_override.clone().into_iter().collect::<Vec<_>>();
    let mut scheduler_loop_count = 0_u64;
    let mut last_waiting_workers_event_at: Option<Instant> = None;
    loop {
        scheduler_loop_count = scheduler_loop_count.saturating_add(1);
        let state_lock = dynamic_state_lock(ctx)?;
        let state_guard = state_lock.lock();
        *graph = load_dynamic_graph(
            &ctx.app.paths.dynamic_graph_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
            ),
            &ctx.app.paths.repo_root,
        )?;
        if !outer_attempt_is_still_current_running(ctx)? {
            mark_dynamic_graph_paused_in_memory(graph, PauseReason::ProcessInterrupted);
            return Ok(());
        }
        while let Ok(resume) = resume_rx.try_recv() {
            pending_resume_overrides.push(resume);
        }
        launch_resume_overrides.extend(apply_dynamic_resume_overrides(
            ctx,
            graph,
            &mut pending_resume_overrides,
        )?);
        let ready_refresh_started_at = Instant::now();
        let ready_node_ids = refresh_dynamic_ready_nodes(graph);
        if !ready_node_ids.is_empty() {
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
            persist_dynamic_graph(ctx, graph)?;
            emit_dynamic_session_updates_best_effort(ctx, graph, &ready_node_ids);
        }
        let launch_started_at = Instant::now();
        let ready_launch_node_ids = graph
            .nodes
            .iter()
            .filter(|node| node.status == DynamicNodeStatus::Ready)
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        if !ready_launch_node_ids.is_empty() {
            dynamic_event_best_effort(
                ctx,
                "dynamic_launch_ready_begin",
                serde_json::json!({
                    "loop": scheduler_loop_count,
                    "readyNodeIds": ready_launch_node_ids,
                    "state": dynamic_timing_data(graph),
                }),
            );
        }
        let launched_node_ids =
            launch_ready_dynamic_nodes(ctx, graph, &tx, &mut launch_resume_overrides)?;
        if !launched_node_ids.is_empty() {
            dynamic_event_best_effort(
                ctx,
                "dynamic_launch_ready_end",
                serde_json::json!({
                    "loop": scheduler_loop_count,
                    "elapsedMs": elapsed_ms(launch_started_at),
                    "launchedNodeIds": launched_node_ids,
                    "state": dynamic_timing_data(graph),
                }),
            );
        }
        if advance_dynamic_groups(ctx, graph)?.changed {
            continue;
        }
        if dynamic_graph_completed(graph) {
            let workspace_ids = graph
                .workspaces
                .iter()
                .filter(|workspace| workspace.ownership == WorkspaceOwnership::Runtime)
                .map(|workspace| workspace.id.clone())
                .collect::<Vec<_>>();
            if !workspace_ids.is_empty() {
                let transition_owner_node_ids = graph
                    .nodes
                    .last()
                    .map(|node| vec![node.id.clone()])
                    .unwrap_or_default();
                with_dynamic_workspace_transition(
                    ctx,
                    graph,
                    &transition_owner_node_ids,
                    |graph| {
                        for workspace_id in workspace_ids {
                            release_dynamic_workspace_best_effort(ctx, graph, &workspace_id);
                        }
                        Ok(())
                    },
                )?;
            }
            complete_dynamic_graph_success(ctx, graph)?;
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
            drop(state_guard);
            let message = match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(recoverable_runtime_error(
                        "dynamic execution channel closed unexpectedly",
                    ));
                }
            };
            let _message_guard = state_lock.lock();
            *graph = load_dynamic_graph(
                &ctx.app.paths.dynamic_graph_file(
                    ctx.task_id,
                    ctx.run_id,
                    ctx.round_id,
                    ctx.outer_node_id,
                    ctx.outer_attempt_id,
                ),
                &ctx.app.paths.repo_root,
            )?;
            apply_dynamic_execution_message(ctx, graph, message)?;
            if graph.run.status == DynamicRunStatus::Paused {
                return Ok(());
            }
            continue;
        }

        if dynamic_graph_has_paused_leaf(graph) {
            if dynamic_graph_has_active_leaf(graph) {
                drop(state_guard);
                let message = match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(message) => message,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(recoverable_runtime_error(
                            "dynamic execution channel closed unexpectedly",
                        ));
                    }
                };
                let _message_guard = state_lock.lock();
                *graph = load_dynamic_graph(
                    &ctx.app.paths.dynamic_graph_file(
                        ctx.task_id,
                        ctx.run_id,
                        ctx.round_id,
                        ctx.outer_node_id,
                        ctx.outer_attempt_id,
                    ),
                    &ctx.app.paths.repo_root,
                )?;
                apply_dynamic_execution_message(ctx, graph, message)?;
                if graph.run.status == DynamicRunStatus::Paused {
                    return Ok(());
                }
                continue;
            }
            let pause_reason = aggregate_dynamic_pause_reason(graph);
            pause_dynamic_graph(
                ctx,
                graph,
                pause_reason,
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

fn complete_dynamic_graph_success(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> Result<()> {
    let terminal_leaf_ids = graph
        .nodes
        .last()
        .filter(|node| node.status == DynamicNodeStatus::Completed && node.outcome.is_some())
        .map(|node| vec![node.id.clone()])
        .unwrap_or_default();
    let updated_at = now_rfc3339_like();
    graph.run.status = DynamicRunStatus::Completed;
    graph.run.phase = DynamicRunPhase::Executing;
    graph.run.outcome = Some(RunOutcome::Success);
    graph.run.updated_at = updated_at.clone();
    advance_dynamic_leaf_runtime_lifecycle_revisions(graph, &terminal_leaf_ids, &updated_at);
    persist_dynamic_graph(ctx, graph)?;
    let event_result = append_dynamic_event(
        ctx,
        "dynamic_run_completed",
        serde_json::json!({
            "dynamicRunId": graph.run.id,
            "outcome": "success",
        }),
    );
    emit_dynamic_run_terminal_session_update_best_effort(ctx, graph);
    event_result
}

fn discard_revoked_dynamic_resume_before_launch(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    node_id: &str,
    launch_resume_overrides: &mut Vec<DynamicResumeOverride>,
) -> Result<bool> {
    let Some(resume_index) = launch_resume_overrides
        .iter()
        .rposition(|resume| resume.node_id == node_id)
    else {
        return Ok(false);
    };
    let resume = &launch_resume_overrides[resume_index];
    let key = dynamic_state_lock_key(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    if dynamic_resume_request_is_active(&key, &resume.request_id)? {
        return Ok(false);
    }

    let resume = launch_resume_overrides.remove(resume_index);
    let Some(node_index) = graph.nodes.iter().position(|node| {
        node.id == node_id
            && dynamic_attempt_id(node) == resume.attempt_id
            && node.runtime_execution_id.as_deref() == Some(resume.execution_id.as_str())
    }) else {
        return Ok(true);
    };
    if graph.nodes[node_index].status == DynamicNodeStatus::Ready {
        mark_dynamic_node_paused(
            &mut graph.nodes[node_index],
            PauseReason::ProcessInterrupted,
            None,
        );
        refresh_dynamic_current_leaf_ids(graph);
        if dynamic_graph_has_active_leaf(graph) {
            graph.run.status = DynamicRunStatus::Running;
            graph.run.phase = DynamicRunPhase::Executing;
            graph.run.outcome = None;
            graph.run.pause_reason = None;
            graph.run.updated_at = now_rfc3339_like();
            persist_dynamic_graph(ctx, graph)?;
        } else {
            pause_dynamic_graph(
                ctx,
                graph,
                PauseReason::ProcessInterrupted,
                "dynamic resume request was revoked before leaf launch",
            )?;
        }
        emit_dynamic_session_update_best_effort(
            ctx,
            node_id,
            &dynamic_attempt_id(&graph.nodes[node_index]),
        );
    }
    Ok(true)
}

fn launch_ready_dynamic_nodes(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    tx: &mpsc::Sender<DynamicExecutionMessage>,
    launch_resume_overrides: &mut Vec<DynamicResumeOverride>,
) -> Result<Vec<String>> {
    let ready_ids = graph
        .nodes
        .iter()
        .filter(|node| node.status == DynamicNodeStatus::Ready)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let mut launched_node_ids = Vec::new();
    for node_id in ready_ids {
        let Some(index) = graph.nodes.iter().position(|node| node.id == node_id) else {
            continue;
        };
        if discard_revoked_dynamic_resume_before_launch(
            ctx,
            graph,
            &node_id,
            launch_resume_overrides,
        )? {
            continue;
        }
        dynamic_event_best_effort(
            ctx,
            "dynamic_launch_candidate",
            serde_json::json!({
                "nodeId": node_id,
                "state": dynamic_timing_data(graph),
            }),
        );
        let Some((node_clone, resume_override)) =
            persist_dynamic_node_running_and_ack(ctx, graph, index, launch_resume_overrides)?
        else {
            continue;
        };
        let node_id_for_job = node_clone.id.clone();

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
        let outer_runtime_execution_id = ctx.outer_runtime_execution_id.clone();
        let parent_continue_input = ctx.parent_continue_input.clone();
        let parent_continue_prompt_id = ctx.parent_continue_prompt_id.clone();
        let spawned_node_id = node_id_for_job.clone();
        let spawned_execution_id = node_clone.runtime_execution_id.clone();
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
                    outer_runtime_execution_id,
                    &dynamic,
                    node_clone,
                    task_uuid.as_deref(),
                    run_uuid.as_deref(),
                    round_uuid.as_deref(),
                    outer_node_uuid.as_deref(),
                    parent_continue_input,
                    parent_continue_prompt_id,
                    resume_override,
                )
            }))
            .unwrap_or_else(|payload| {
                let panic_message = payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_string());
                Err(recoverable_runtime_error(format!(
                    "dynamic node job panicked: {panic_message}"
                )))
            });
            let message = DynamicExecutionMessage {
                node_id,
                execution_id: spawned_execution_id,
                result,
            };
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
        launched_node_ids.push(spawned_node_id);
    }
    Ok(launched_node_ids)
}

fn persist_dynamic_node_running_and_ack(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    index: usize,
    launch_resume_overrides: &mut Vec<DynamicResumeOverride>,
) -> Result<Option<(DynamicNodeState, Option<DynamicResumeOverride>)>> {
    let node = graph
        .nodes
        .get_mut(index)
        .ok_or_else(|| anyhow!("dynamic node index out of range"))?;
    rearm_dynamic_node(node, DynamicNodeStatus::Running);
    if node.runtime_execution_id.is_none() {
        node.begin_runtime_execution(next_runtime_execution_id(), now_rfc3339_like());
    }
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
            "workspaceId": node_clone.workspace_id,
            "providerId": node_clone.provider.clone(),
            "model": node_clone.model.clone(),
            "state": dynamic_timing_data(graph),
        }),
    );
    let mut resume_override = launch_resume_overrides
        .iter()
        .rposition(|resume| resume.node_id == node_id_for_job)
        .map(|index| launch_resume_overrides.remove(index));
    persist_dynamic_graph(ctx, graph)?;
    if let Some(resume) = resume_override.as_mut()
        && !notify_dynamic_resume_started(ctx, resume)
    {
        mark_dynamic_node_paused(
            &mut graph.nodes[index],
            PauseReason::ProcessInterrupted,
            None,
        );
        refresh_dynamic_current_leaf_ids(graph);
        if dynamic_graph_has_active_leaf(graph) {
            graph.run.status = DynamicRunStatus::Running;
            graph.run.phase = DynamicRunPhase::Executing;
            graph.run.outcome = None;
            graph.run.pause_reason = None;
            graph.run.updated_at = now_rfc3339_like();
            persist_dynamic_graph(ctx, graph)?;
        } else {
            pause_dynamic_graph(
                ctx,
                graph,
                PauseReason::ProcessInterrupted,
                "dynamic resume lease was revoked while committing leaf launch",
            )?;
        }
        emit_dynamic_session_update_best_effort(
            ctx,
            &node_id_for_job,
            &dynamic_attempt_id(&graph.nodes[index]),
        );
        return Ok(None);
    }
    emit_dynamic_session_update_best_effort(
        ctx,
        &node_id_for_job,
        &dynamic_attempt_id(&node_clone),
    );
    Ok(Some((node_clone, resume_override)))
}

fn persist_paused_dynamic_leaf_or_graph(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    index: usize,
    pause_reason: PauseReason,
    reason: &str,
) -> Result<bool> {
    graph.nodes[index].pause_reason = Some(pause_reason);
    refresh_dynamic_current_leaf_ids(graph);
    let has_active_leaf = dynamic_graph_has_active_leaf(graph);
    if has_active_leaf {
        graph.run.status = DynamicRunStatus::Running;
        graph.run.phase = DynamicRunPhase::Executing;
        graph.run.outcome = None;
        graph.run.pause_reason = None;
        graph.run.updated_at = now_rfc3339_like();
        persist_dynamic_graph(ctx, graph)?;
    } else {
        pause_dynamic_graph(ctx, graph, pause_reason, reason)?;
    }
    emit_dynamic_session_update_best_effort(
        ctx,
        &graph.nodes[index].id,
        &dynamic_attempt_id(&graph.nodes[index]),
    );
    Ok(has_active_leaf)
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
    if graph.nodes[index].runtime_execution_id != message.execution_id {
        dynamic_event_best_effort(
            ctx,
            "dynamic_stale_execution_result_ignored",
            serde_json::json!({
                "nodeId": message.node_id,
                "executionId": message.execution_id,
                "currentExecutionId": graph.nodes[index].runtime_execution_id,
            }),
        );
        return Ok(());
    }
    let result = match message.result {
        Ok(result) => result,
        Err(error) => {
            let reason = format!("{error:#}");
            let info = normalize_runtime_error(&error);
            let pause_reason = info.pause_reason_after_retry_boundary();
            mark_dynamic_node_paused(&mut graph.nodes[index], pause_reason, Some(info.clone()));
            append_dynamic_event(
                ctx,
                "dynamic_runtime_error",
                serde_json::json!({
                    "nodeId": message.node_id,
                    "pauseReason": pause_reason,
                    "runtimeError": info,
                }),
            )?;
            let has_active_leaf =
                persist_paused_dynamic_leaf_or_graph(ctx, graph, index, pause_reason, &reason)?;
            return match info.recovery {
                RecoveryMode::Auto | RecoveryMode::Manual => Ok(()),
                RecoveryMode::Blocked if has_active_leaf => Ok(()),
                RecoveryMode::Blocked => Err(error),
            };
        }
    };
    if !outer_attempt_is_still_current_running(ctx)? {
        let pause_reason = ctx
            .app
            .run_status(ctx.task_id, ctx.run_id)?
            .pause_reason
            .unwrap_or(PauseReason::ProcessInterrupted);
        mark_dynamic_node_paused(&mut graph.nodes[index], pause_reason, None);
        mark_dynamic_graph_paused_in_memory(graph, pause_reason);
        append_dynamic_event(
            ctx,
            "dynamic_result_ignored_after_outer_attempt_stopped",
            serde_json::json!({
                "nodeId": graph.nodes[index].id,
                "attemptId": dynamic_attempt_id(&graph.nodes[index]),
            }),
        )?;
        return Ok(());
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
        graph.nodes[index].pause_reason = Some(pause_reason);
        graph.nodes[index].runtime_error = None;
        persist_paused_dynamic_leaf_or_graph(
            ctx,
            graph,
            index,
            pause_reason,
            "dynamic node paused and no active dynamic leaf remains",
        )?;
        return Ok(());
    }
    let mut accepted_any = false;
    let mut rejected_source_node_id = None;
    let mut visible_node_ids = Vec::new();
    for proposal in result.proposals {
        if proposal.validation_status == DynamicProposalValidationStatus::Rejected {
            rejected_source_node_id = Some(proposal.source_node_id.clone());
            graph.proposals.push(proposal);
            continue;
        }
        accepted_any = true;
        visible_node_ids.extend(accept_dynamic_completion_proposal(ctx, graph, proposal)?);
    }
    if !accepted_any {
        if let Some(source_node_id) = rejected_source_node_id {
            let blocked_error = blocked_runtime_error_info(
                RuntimeErrorDomain::Dynamic,
                "dynamic.proposal-rejected",
                format!("dynamic proposal from `{source_node_id}` was rejected"),
                serde_json::json!({ "sourceNodeId": source_node_id }),
            );
            mark_dynamic_node_paused(
                &mut graph.nodes[index],
                PauseReason::ErrorBlocked,
                Some(blocked_error),
            );
            let has_active_leaf = persist_paused_dynamic_leaf_or_graph(
                ctx,
                graph,
                index,
                PauseReason::ErrorBlocked,
                "invalid dynamic-node-completion proposal",
            )?;
            if has_active_leaf {
                return Ok(());
            }
            return Err(blocked_runtime_error(format!(
                "dynamic proposal from `{source_node_id}` was rejected"
            )));
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

fn accept_dynamic_completion_proposal(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    proposal: DynamicProposalState,
) -> Result<Vec<String>> {
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
    let completion: DynamicNodeCompletion = serde_json::from_value(proposal.parsed.clone())?;
    let proposal_id = proposal.id.clone();
    let source_node_id = proposal.source_node_id.clone();
    graph.proposals.push(proposal);
    let visible_node_ids = materialize_dynamic_next(ctx, graph, source_index, completion.next)?;
    append_dynamic_event(
        ctx,
        "dynamic_proposal_accepted",
        serde_json::json!({
            "proposalId": proposal_id,
            "sourceNodeId": source_node_id,
        }),
    )?;
    Ok(visible_node_ids)
}

fn outer_attempt_is_still_current_running(ctx: &DynamicExecutionContext<'_>) -> Result<bool> {
    if !attempt_is_still_current_running(
        ctx.app,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    )? {
        return Ok(false);
    }
    let Some(expected_execution_id) = ctx.outer_runtime_execution_id.as_deref() else {
        return Ok(true);
    };
    let node: NodeState = read_json(&ctx.app.paths.node_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    ))?;
    Ok(node.runtime_execution_id.as_deref() == Some(expected_execution_id))
}

fn dynamic_leaf_attempt_is_still_running(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
    execution_id: Option<&str>,
) -> Result<bool> {
    if !outer_attempt_is_still_current_running(ctx)? {
        return Ok(false);
    }
    let state_lock = dynamic_state_lock_for(
        &ctx.app.paths.repo_root,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    )?;
    let _guard = state_lock.lock();
    let graph = load_dynamic_graph(
        &ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        &ctx.app.paths.repo_root,
    )?;
    Ok(graph.nodes.iter().any(|node| {
        node.id == node_id
            && dynamic_attempt_id(node) == attempt_id
            && node.status == DynamicNodeStatus::Running
            && node.outcome.is_none()
            && execution_id.map_or(true, |execution_id| {
                node.runtime_execution_id.as_deref() == Some(execution_id)
            })
    }))
}

fn dynamic_node_uses_completion_contract(kind: DynamicNodeKind) -> bool {
    matches!(
        kind,
        DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation | DynamicNodeKind::Acceptance
    )
}

fn outer_attempt_is_current_recoverable_pause(ctx: &DynamicExecutionContext<'_>) -> Result<bool> {
    let run: RunState = read_json(&ctx.app.paths.run_file(ctx.task_id, ctx.run_id))?;
    Ok(run.current_round.as_deref() == Some(ctx.round_id)
        && run.current_node.as_deref() == Some(ctx.outer_node_id)
        && run.current_attempt.as_deref() == Some(ctx.outer_attempt_id)
        && run.status == RunStatus::Paused
        && matches!(
            run.pause_reason,
            Some(PauseReason::ProcessInterrupted | PauseReason::RuntimeAbnormal)
        ))
}

fn restore_outer_attempt_running_for_dynamic_resume(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
) -> Result<bool> {
    let mut run: RunState = read_json(&app.paths.run_file(task_id, run_id))?;
    if run.current_round.as_deref() != Some(round_id)
        || run.current_node.as_deref() != Some(outer_node_id)
        || run.current_attempt.as_deref() != Some(outer_attempt_id)
        || run.status != RunStatus::Paused
        || !matches!(
            run.pause_reason,
            Some(PauseReason::ProcessInterrupted | PauseReason::RuntimeAbnormal)
        )
    {
        return Ok(false);
    }
    let mut round: RoundState = read_json(&app.paths.round_file(task_id, run_id, round_id))?;
    let mut node: NodeState = read_json(&app.paths.node_file(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    ))?;
    if round.status != RunStatus::Paused || node.status != RunStatus::Paused {
        return Ok(false);
    }
    let runtime_candidate = app.begin_runtime_candidate(task_id, run_id)?;
    let previous_pause_reason = run.pause_reason.unwrap_or(PauseReason::ProcessInterrupted);
    let now = now_rfc3339_like();
    run.status = RunStatus::Running;
    run.pause_reason = None;
    run.updated_at = now;
    if let Some(runtime_candidate) = runtime_candidate.as_ref() {
        run.execution.recovery_candidate_token = Some(runtime_candidate.token().to_string());
    }
    round.status = RunStatus::Running;
    round.outcome = None;
    node.status = RunStatus::Running;
    node.outcome = None;
    node.finished_at = None;
    persist_runtime_state(app, task_id, &run, &round, &node)?;
    if let Some(runtime_candidate) = runtime_candidate {
        runtime_candidate.commit();
    }
    app.record_metrics_resume_cause(
        task_id,
        run_id,
        super::observability::ResumeCause::AutomaticRecovery,
    );
    emit_run_metrics_fact(
        app,
        &run,
        super::observability::LifecycleEventType::ExecutionResumed,
        uuid::Uuid::new_v4().to_string(),
        now_rfc3339_like(),
        Some(previous_pause_reason),
        None,
        None,
    );
    Ok(true)
}

fn try_restore_outer_attempt_running_for_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
) -> Result<bool> {
    restore_outer_attempt_running_for_dynamic_resume(
        ctx.app,
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    )
}

fn mark_dynamic_graph_paused_in_memory(graph: &mut DynamicGraphState, pause_reason: PauseReason) {
    refresh_dynamic_current_leaf_ids(graph);
    graph.run.status = DynamicRunStatus::Paused;
    graph.run.phase = DynamicRunPhase::Executing;
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
        Some(NodeOutcome::Invalid) => "INVALID",
        Some(NodeOutcome::Killed) => "KILLED",
        _ => "FAILED",
    };

    app.emit_lifecycle_event(RuntimeLifecycleEvent::NodeCompleted {
        task_id: ctx.task_id.to_string(),
        task_uuid: ctx.task_uuid.map(|s| s.to_string()),
        run_id: ctx.run_id.to_string(),
        run_uuid: ctx.run_uuid.map(|s| s.to_string()),
        round_id: ctx.round_id.to_string(),
        round_uuid: ctx.round_uuid.map(|s| s.to_string()),
        round_index: read_json::<RoundState>(&app.paths.round_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
        ))
        .ok()
        .map(|round| round.index),
        node_id: node.id.clone(),
        node_uuid: node.uuid.clone(),
        attempt_id: dynamic_attempt_id(node),
        repo_root: app.paths.repo_root.to_string(),
        seq: None,
        node_name: node.title.clone(),
        agent_type: node.provider.clone(),
        resolved_model: node.model.clone(),
        started_at: node.started_at.clone().unwrap_or_default(),
        finished_at: node.finished_at.clone(),
        outcome: outcome.to_string(),
        attempt_dir,
        suppress_sentinel: true,
        metrics_unit_kind: Some(node.kind),
        child_run_id: node.child_run_id.clone(),
    });
}

fn execute_dynamic_node_job(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    outer_runtime_execution_id: Option<String>,
    dynamic: &AiDynamicNode,
    node: DynamicNodeState,
    task_uuid: Option<&str>,
    run_uuid: Option<&str>,
    round_uuid: Option<&str>,
    outer_node_uuid: Option<&str>,
    parent_continue_input: Option<ConversationPromptInput>,
    parent_continue_prompt_id: Option<String>,
    resume_override: Option<DynamicResumeOverride>,
) -> Result<DynamicExecutionResult> {
    app.emit_lifecycle_event(RuntimeLifecycleEvent::NodeStarted {
        project_id: app.paths.project_id.clone(),
        task_id: task_id.to_string(),
        task_uuid: task_uuid.map(str::to_string),
        run_id: run_id.to_string(),
        run_uuid: run_uuid.map(str::to_string),
        round_id: round_id.to_string(),
        round_uuid: round_uuid.map(str::to_string),
        round_index: read_json::<RoundState>(&app.paths.round_file(task_id, run_id, round_id))
            .ok()
            .map(|round| round.index),
        node_id: node.id.clone(),
        node_uuid: node.uuid.clone(),
        attempt_id: dynamic_attempt_id(&node),
        repo_root: app.paths.repo_root.to_string(),
        seq: None,
        node_name: Some(node.title.clone()),
        agent_type: node.provider.clone(),
        resolved_model: node.model.clone(),
        started_at: node.started_at.clone().unwrap_or_else(now_rfc3339_like),
        attempt_dir: None,
        predecessor: None,
        metrics_unit_kind: Some(node.kind),
        child_run_id: node.child_run_id.clone(),
    });
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
    let state_lock = dynamic_state_lock_for(
        &app.paths.repo_root,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
    )?;
    let state_load_started_at = Instant::now();
    let (run, mut graph): (DynamicRunState, DynamicGraphState) = {
        let _guard = state_lock.lock();
        (
            read_json(&dynamic_run_path)?,
            load_dynamic_graph(&graph_path, &app.paths.repo_root)?,
        )
    };
    let state_load_elapsed_ms = elapsed_ms(state_load_started_at);
    let ctx = DynamicExecutionContext {
        app,
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        outer_runtime_execution_id,
        dynamic,
        task_uuid,
        run_uuid,
        round_uuid,
        outer_node_uuid,
        parent_continue_input,
        parent_continue_prompt_id,
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
            "workspaceId": node.workspace_id,
            "providerId": node.provider.clone(),
            "model": node.model.clone(),
        }),
    );

    let result = match node.kind {
        DynamicNodeKind::Worker | DynamicNodeKind::Acceptance => {
            execute_dynamic_worker(&ctx, &graph, node)
        }
        DynamicNodeKind::WorkflowInvocation => {
            execute_dynamic_workflow_invocation(&ctx, &graph, node)
        }
        DynamicNodeKind::Merge => execute_dynamic_agent_stage(&ctx, &graph, node),
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
    let workspace_path = ensure_dynamic_workspace(graph, &node)?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_workspace_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "workspaceId": node.workspace_id,
            "workspacePath": workspace_path,
        }),
    );
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_workspace_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(workspace_started_at),
            "workspaceId": node.workspace_id,
            "workspacePath": workspace_path,
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
        .ok_or_else(|| {
            runtime_error(manual_runtime_error_info(
                RuntimeErrorDomain::Config,
                "config.provider-missing",
                format!("dynamic worker `{}` is missing provider", node.id),
                serde_json::json!({ "nodeId": node.id }),
            ))
        })?
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
    let continue_ref = match node.session_mode {
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
    let session_mode = if continue_ref.is_some() {
        SessionMode::Continue
    } else {
        SessionMode::New
    };
    let mut prompt_state = AcpInvocationPromptState::workflow_transition(
        ctx.app.config.desktop_language,
        session_mode,
        continue_ref,
    );
    if let Some(resume) = ctx
        .resume_override
        .as_ref()
        .filter(|resume| resume.node_id == node.id && resume.attempt_id == attempt_id)
    {
        let Some(saved_continue_ref) = dynamic_node_continue_ref(ctx, &node, &attempt_id) else {
            return Err(blocked_runtime_error(format!(
                "dynamic node `{}` has no ACP continue reference",
                node.id
            )));
        };
        prompt_state = AcpInvocationPromptState::runtime_control_resume(
            ctx.app.config.desktop_language,
            Some(saved_continue_ref),
        );
        prompt_state.resume_prompt = Some(resume.prompt.clone());
        prompt_state.resume_prompt_id = resume.prompt_id.clone();
        prompt_state.prompt_display = resume.prompt_display.clone();
        prompt_state.resume_prompt_visibility = resume.prompt_visibility;
        prompt_state.user_prompt_render_mode = resume.user_prompt_render_mode;
        prompt_state.input_attachment_paths = resume.attachment_paths.clone();
        prompt_state.model_override = resume.model_override.clone();
        prompt_state.permission_mode_override = resume.permission_mode_override.clone();
        prompt_state.runtime_control_intent = resume.runtime_control_intent;
    }
    let mut session_mode = prompt_state.session_mode;
    let mut continue_ref = prompt_state.continue_ref;
    let mut resume_prompt = prompt_state.resume_prompt;
    let resume_prompt_id = prompt_state.resume_prompt_id;
    let mut prompt_display = prompt_state.prompt_display;
    let logical_prompt_id = logical_prompt_id(resume_prompt_id.clone());
    let mut current_prompt_id = logical_prompt_id.clone();
    let mut resume_prompt_visibility = prompt_state.resume_prompt_visibility;
    let mut user_prompt_render_mode = prompt_state.user_prompt_render_mode;
    let mut runtime_control_intent = prompt_state.runtime_control_intent;
    let resume_input_attachment_paths = prompt_state.input_attachment_paths;
    let resume_model_override = prompt_state.model_override;
    let resume_permission_mode_override = prompt_state.permission_mode_override;
    let mut proposals = Vec::new();
    let mut auto_retry_attempts = 0;
    let expected_execution_id = node
        .runtime_execution_id
        .clone()
        .ok_or_else(|| anyhow!("dynamic node `{}` has no active execution", node.id))?;

    loop {
        if !dynamic_leaf_attempt_is_still_running(
            ctx,
            &node.id,
            &attempt_id,
            Some(expected_execution_id.as_str()),
        )? {
            mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
            return Ok(DynamicExecutionResult { node, proposals });
        }
        let live_update_context = dynamic_acp_live_event_context(ctx, &node.id, &attempt_id);
        let live_update = ctx.app.acp_live_update_for(live_update_context.clone());
        let session_update = ctx.app.acp_session_update_for(live_update_context.clone());
        let prompt_accepted = ctx.app.acp_prompt_accepted_for(live_update_context);
        let runtime_prompt_accepted = |prompt_id: &str| {
            transition_dynamic_leaf_runtime_phase(
                ctx,
                &node.id,
                &attempt_id,
                &expected_execution_id,
                RuntimeExecutionPhase::RunningNode,
            )?;
            if let Some(callback) = prompt_accepted.as_ref() {
                callback(prompt_id)?;
            }
            Ok(())
        };
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
        let output_contract = dynamic_output_contract_for_node(ctx, graph, &node)
            .expect("dynamic worker stage requires a completion contract");
        let runtime_phase_update = |phase| {
            let phase = match phase {
                crate::provider::ProviderRuntimePhase::FinalizingArtifact => {
                    RuntimeExecutionPhase::FinalizingArtifact
                }
            };
            transition_dynamic_leaf_runtime_phase(
                ctx,
                &node.id,
                &attempt_id,
                &expected_execution_id,
                phase,
            )?;
            Ok(())
        };
        let accept_interrupted_completion =
            interrupted_dynamic_completion_is_trusted(output_contract.emission_mode);
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
        let mut invocation = build_dynamic_worker_invocation(
            ctx,
            graph,
            &node,
            &attempt_id,
            Some(output_contract),
            session_mode,
            continue_ref.clone(),
            resume_prompt.clone(),
            current_prompt_id.clone(),
            prompt_display.clone(),
            resume_prompt_visibility,
            user_prompt_render_mode,
            resume_input_attachment_paths.clone(),
            resume_model_override.clone(),
            resume_permission_mode_override.clone(),
        )
        .with_context(|| {
            format!(
                "failed to build dynamic worker invocation for `{}`",
                node.id
            )
        })?;
        invocation.runtime_control_intent = runtime_control_intent;
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
        .with_context(|| format!("failed to append dynamic start event for `{}`", node.id))
        .map_err(|error| recoverable_runtime_error(error.to_string()))?;
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
        let provider = ctx.app.provider_for_id(&provider_id).with_context(|| {
            format!(
                "failed to resolve provider `{}` for `{}`",
                provider_id, node.id
            )
        })?;
        let provider_resolve_elapsed_ms = elapsed_ms(provider_started_at);
        let result = match provider
            .run_worker_with_runtime_callbacks(
                invocation,
                live_update.as_ref().map(|callback| callback as _),
                session_update.as_ref().map(|callback| callback as _),
                Some(&runtime_prompt_accepted),
                Some(&runtime_phase_update),
            )
            .with_context(|| format!("provider `{}` failed to run `{}`", provider_id, node.id))
        {
            Ok(result) => result,
            Err(error) => {
                if !dynamic_leaf_attempt_is_still_running(
                    ctx,
                    &node.id,
                    &attempt_id,
                    Some(expected_execution_id.as_str()),
                )? {
                    mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
                    return Ok(DynamicExecutionResult { node, proposals });
                }
                let info = normalize_runtime_error(&error);
                if let Some(delay_ms) = auto_retry_delay_ms(&info, auto_retry_attempts) {
                    if !dynamic_leaf_attempt_is_still_running(
                        ctx,
                        &node.id,
                        &attempt_id,
                        Some(expected_execution_id.as_str()),
                    )? {
                        mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
                        return Ok(DynamicExecutionResult { node, proposals });
                    }
                    auto_retry_attempts += 1;
                    append_dynamic_event(
                        ctx,
                        "dynamic_runtime_auto_retry",
                        serde_json::json!({
                            "nodeId": node.id,
                            "attemptId": attempt_id,
                            "promptId": current_prompt_id,
                            "retryAttempt": auto_retry_attempts,
                            "maxAttempts": info.retry_policy.as_ref().map(|policy| policy.max_attempts),
                            "delayMs": delay_ms,
                            "runtimeError": info,
                        }),
                    )?;
                    if !wait_for_retry_while_active(delay_ms, || {
                        dynamic_leaf_attempt_is_still_running(
                            ctx,
                            &node.id,
                            &attempt_id,
                            Some(expected_execution_id.as_str()),
                        )
                    })? {
                        mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
                        return Ok(DynamicExecutionResult { node, proposals });
                    }
                    continue;
                }
                return Err(error);
            }
        };
        refresh_dynamic_leaf_runtime_execution(ctx, &mut node)?;
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
        if let Some(info) = result.runtime_error.as_ref() {
            if !dynamic_leaf_attempt_is_still_running(
                ctx,
                &node.id,
                &attempt_id,
                Some(expected_execution_id.as_str()),
            )? {
                mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
                return Ok(DynamicExecutionResult { node, proposals });
            }
            if let Some(delay_ms) = auto_retry_delay_ms(info, auto_retry_attempts) {
                if !dynamic_leaf_attempt_is_still_running(
                    ctx,
                    &node.id,
                    &attempt_id,
                    Some(expected_execution_id.as_str()),
                )? {
                    mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
                    return Ok(DynamicExecutionResult { node, proposals });
                }
                auto_retry_attempts += 1;
                append_dynamic_event(
                    ctx,
                    "dynamic_runtime_auto_retry",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "promptId": current_prompt_id,
                        "retryAttempt": auto_retry_attempts,
                        "maxAttempts": info.retry_policy.as_ref().map(|policy| policy.max_attempts),
                        "delayMs": delay_ms,
                        "runtimeError": info,
                    }),
                )?;
                if !wait_for_retry_while_active(delay_ms, || {
                    dynamic_leaf_attempt_is_still_running(
                        ctx,
                        &node.id,
                        &attempt_id,
                        Some(expected_execution_id.as_str()),
                    )
                })? {
                    mark_dynamic_node_paused(&mut node, PauseReason::ProcessInterrupted, None);
                    return Ok(DynamicExecutionResult { node, proposals });
                }
                continue;
            }
        }
        let provider_status = result.status;
        let interrupted_output_artifact = accept_interrupted_completion
            .then(|| interrupted_dynamic_output_artifact_candidate(&result))
            .flatten();
        finalize_dynamic_worker_result(ctx, &mut node, &attempt_id, result)?;
        if provider_status == ProviderRunStatus::Interrupted
            && accept_interrupted_completion
            && let Some(proposal) = try_accept_interrupted_dynamic_completion(
                ctx,
                &mut node,
                &attempt_id,
                interrupted_output_artifact.as_ref(),
            )?
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
                let now = now_rfc3339_like();
                node.status = DynamicNodeStatus::Paused;
                node.outcome = None;
                node.pause_runtime_execution(now.clone());
                node.finished_at = Some(now);
                return Ok(DynamicExecutionResult {
                    node,
                    proposals: Vec::new(),
                });
            }
            node.complete_runtime_execution(now_rfc3339_like());
            bail!("dynamic worker `{}` failed", node.id);
        }
        if !outer_attempt_is_still_current_running(ctx)?
            && let Some(proposal) =
                try_accept_interrupted_dynamic_completion(ctx, &mut node, &attempt_id, None)?
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
            let now = now_rfc3339_like();
            node.status = DynamicNodeStatus::Paused;
            node.outcome = None;
            node.pause_runtime_execution(now.clone());
            node.finished_at = Some(now);
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
                node.complete_runtime_execution(now_rfc3339_like());
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
                current_prompt_id =
                    dynamic_proposal_repair_prompt_id(&logical_prompt_id, proposal_repair_prompts);
                append_dynamic_event(
                    ctx,
                    "dynamic_proposal_repair_requested",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "repairAttempt": proposal_repair_prompts,
                        "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                        "promptId": current_prompt_id,
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
                prompt_display = None;
                resume_prompt_visibility = PromptVisibility::Hidden;
                user_prompt_render_mode = UserPromptRenderMode::RuntimeRepair;
                runtime_control_intent = RuntimeControlIntent::Unchanged;
                node.status = DynamicNodeStatus::Running;
                node.outcome = None;
                node.finished_at = None;
                transition_dynamic_leaf_runtime_phase(
                    ctx,
                    &node.id,
                    &attempt_id,
                    &expected_execution_id,
                    RuntimeExecutionPhase::RepairingArtifact,
                )?;
                refresh_dynamic_leaf_runtime_execution(ctx, &mut node)?;
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
                current_prompt_id =
                    dynamic_proposal_repair_prompt_id(&logical_prompt_id, proposal_repair_prompts);
                append_dynamic_event(
                    ctx,
                    "dynamic_proposal_repair_requested",
                    serde_json::json!({
                        "nodeId": node.id,
                        "attemptId": attempt_id,
                        "repairAttempt": proposal_repair_prompts,
                        "maxRepairAttempts": MAX_DYNAMIC_PROPOSAL_REPAIR_PROMPTS,
                        "promptId": current_prompt_id,
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
                prompt_display = None;
                resume_prompt_visibility = PromptVisibility::Hidden;
                user_prompt_render_mode = UserPromptRenderMode::RuntimeRepair;
                runtime_control_intent = RuntimeControlIntent::Unchanged;
                node.status = DynamicNodeStatus::Running;
                node.outcome = None;
                node.finished_at = None;
                transition_dynamic_leaf_runtime_phase(
                    ctx,
                    &node.id,
                    &attempt_id,
                    &expected_execution_id,
                    RuntimeExecutionPhase::RepairingArtifact,
                )?;
                refresh_dynamic_leaf_runtime_execution(ctx, &mut node)?;
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
    let workspace_path = ensure_dynamic_workspace(graph, &node)?;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_workspace_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(workspace_started_at),
            "workspaceId": node.workspace_id,
            "workspacePath": workspace_path,
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
    let mut prompt_state = AcpInvocationPromptState::workflow_transition(
        ctx.app.config.desktop_language,
        session_mode,
        continue_ref,
    );
    if let Some(resume) = ctx
        .resume_override
        .as_ref()
        .filter(|resume| resume.node_id == node.id && resume.attempt_id == attempt_id)
    {
        let Some(saved_continue_ref) = dynamic_node_continue_ref(ctx, &node, &attempt_id) else {
            return Err(blocked_runtime_error(format!(
                "dynamic node `{}` has no ACP continue reference",
                node.id
            )));
        };
        prompt_state = runtime_control_resume_prompt_state(
            ctx.app.config.desktop_language,
            saved_continue_ref,
            None,
            resume.prompt_display.clone(),
            resume.prompt_id.clone(),
            resume.attachment_paths.clone(),
            resume.model_override.clone(),
            resume.permission_mode_override.clone(),
        );
    }
    let logical_prompt_id = logical_prompt_id(prompt_state.resume_prompt_id.clone());
    let live_update_context = dynamic_acp_live_event_context(ctx, &node.id, &attempt_id);
    let live_update = ctx.app.acp_live_update_for(live_update_context.clone());
    let session_update = ctx.app.acp_session_update_for(live_update_context.clone());
    let prompt_accepted = ctx.app.acp_prompt_accepted_for(live_update_context);
    let expected_execution_id = node
        .runtime_execution_id
        .clone()
        .ok_or_else(|| anyhow!("dynamic node `{}` has no active execution", node.id))?;
    let runtime_prompt_accepted = |prompt_id: &str| {
        transition_dynamic_leaf_runtime_phase(
            ctx,
            &node.id,
            &attempt_id,
            &expected_execution_id,
            RuntimeExecutionPhase::RunningNode,
        )?;
        if let Some(callback) = prompt_accepted.as_ref() {
            callback(prompt_id)?;
        }
        Ok(())
    };
    let invocation_build_started_at = Instant::now();
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_invocation_build_begin",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "sessionMode": prompt_state.session_mode,
            "providerId": node.provider.clone(),
            "model": node.model.clone(),
        }),
    );
    let mut invocation = build_dynamic_worker_invocation(
        ctx,
        graph,
        &node,
        &attempt_id,
        None,
        prompt_state.session_mode,
        prompt_state.continue_ref.clone(),
        prompt_state.resume_prompt.clone(),
        logical_prompt_id,
        prompt_state.prompt_display.clone(),
        prompt_state.resume_prompt_visibility,
        prompt_state.user_prompt_render_mode,
        prompt_state.input_attachment_paths.clone(),
        prompt_state.model_override.clone(),
        prompt_state.permission_mode_override.clone(),
    )?;
    invocation.runtime_control_intent = prompt_state.runtime_control_intent;
    dynamic_event_best_effort(
        ctx,
        "dynamic_worker_invocation_build_end",
        serde_json::json!({
            "nodeId": node.id,
            "kind": node.kind,
            "elapsedMs": elapsed_ms(invocation_build_started_at),
            "sessionMode": prompt_state.session_mode,
            "providerId": node.provider.clone(),
            "model": node.model.clone(),
        }),
    );
    let provider_id = node
        .provider
        .as_deref()
        .ok_or_else(|| {
            runtime_error(manual_runtime_error_info(
                RuntimeErrorDomain::Config,
                "config.provider-missing",
                format!("dynamic stage `{}` is missing provider", node.id),
                serde_json::json!({ "nodeId": node.id }),
            ))
        })?
        .to_string();
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
            "sessionMode": prompt_state.session_mode,
            "providerId": provider_id,
            "model": node.model.clone(),
        }),
    );
    let provider = ctx.app.provider_for_id(&provider_id).with_context(|| {
        format!(
            "failed to resolve provider `{provider_id}` for `{}`",
            node.id
        )
    })?;
    let provider_resolve_elapsed_ms = elapsed_ms(provider_started_at);
    let result = provider
        .run_worker_with_callbacks(
            invocation,
            live_update.as_ref().map(|callback| callback as _),
            session_update.as_ref().map(|callback| callback as _),
            Some(&runtime_prompt_accepted),
        )
        .with_context(|| format!("provider `{provider_id}` failed to run `{}`", node.id))?;
    refresh_dynamic_leaf_runtime_execution(ctx, &mut node)?;
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
        let now = now_rfc3339_like();
        node.pause_runtime_execution(now.clone());
        node.finished_at = Some(now);
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
    if node.outcome != Some(NodeOutcome::Success) {
        node.complete_runtime_execution(now_rfc3339_like());
        bail!("dynamic stage `{}` failed", node.id);
    }
    node.complete_runtime_execution(now_rfc3339_like());
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
    let workspace_path = ensure_dynamic_workspace(graph, &node)?;
    let workflow_id = node.workflow_id.clone().ok_or_else(|| {
        blocked_runtime_error(format!(
            "workflow invocation `{}` is missing workflowId",
            node.id
        ))
    })?;
    let snapshot = graph
        .run
        .allowed_workflow_snapshots
        .iter()
        .find(|snapshot| snapshot.workflow_id == workflow_id)
        .ok_or_else(|| {
            blocked_runtime_error(format!(
                "workflow invocation `{}` references a workflow that is not allowed",
                node.id
            ))
        })?;
    ensure!(
        ctx.dynamic.control.allow_nested_dynamic || !snapshot.contains_ai_dynamic,
        "workflow invocation `{}` references a nested AI-DYNAMIC snapshot",
        node.id
    );

    let attempt_id = dynamic_attempt_id(&node);
    let expected_execution_id = node
        .runtime_execution_id
        .clone()
        .ok_or_else(|| anyhow!("dynamic node `{}` has no active execution", node.id))?;
    transition_dynamic_leaf_runtime_phase(
        ctx,
        &node.id,
        &attempt_id,
        &expected_execution_id,
        RuntimeExecutionPhase::RunningNode,
    )?;
    refresh_dynamic_leaf_runtime_execution(ctx, &mut node)?;
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
    let mut child_app = ctx.app.clone_for_background();
    child_app.paths.repo_root = workspace_path;
    let child_run = match node.child_run_id.as_deref() {
        Some(child_run_id) => {
            if let Some(resume) = ctx
                .resume_override
                .as_ref()
                .filter(|resume| resume.node_id == node.id && resume.attempt_id == attempt_id)
            {
                child_app.run_continue_with_prompt_input(
                    ctx.task_id,
                    child_run_id,
                    resume.prompt_id.clone(),
                    resume.prompt_display.clone(),
                )?
            } else if ctx.parent_continue_input.is_some() {
                child_app.run_continue_with_prompt_input(
                    ctx.task_id,
                    child_run_id,
                    ctx.parent_continue_prompt_id.clone(),
                    ctx.parent_continue_input.clone(),
                )?
            } else {
                child_app.run_continue(ctx.task_id, child_run_id, None, None)?
            }
        }
        None => child_app.run_start(ctx.task_id, Some(child_workflow_path.as_path()))?,
    };
    node.child_run_id = Some(child_run.id.clone());
    match child_run.status {
        RunStatus::Paused => {
            let pause_reason = child_run
                .pause_reason
                .unwrap_or(PauseReason::ProcessInterrupted);
            mark_dynamic_node_paused(&mut node, pause_reason, None);
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
            let now = now_rfc3339_like();
            node.finished_at = Some(now_rfc3339_like());
            node.status = DynamicNodeStatus::Completed;
            node.outcome = Some(match child_run.outcome {
                Some(RunOutcome::Success) => NodeOutcome::Success,
                _ => NodeOutcome::Failure,
            });
            node.pause_reason = None;
            node.runtime_error = None;
            node.complete_runtime_execution(now);
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
        bail!("child workflow invocation `{}` failed", node.id);
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
    if let Some(info) = result.runtime_error {
        return Err(runtime_error(info));
    }
    match status {
        ProviderRunStatus::Success => {
            if let Some(payload) = result.result_payload
                && let Some(output_artifact) = payload.output_artifact
            {
                write_dynamic_output_artifact(ctx, &node_id, attempt_id, &output_artifact)?;
            }
            node.status = DynamicNodeStatus::Completed;
            node.outcome = Some(NodeOutcome::Success);
            node.pause_reason = None;
            node.runtime_error = None;
        }
        ProviderRunStatus::Failure => {
            node.status = DynamicNodeStatus::Completed;
            node.outcome = Some(NodeOutcome::Failure);
            node.pause_reason = None;
            node.runtime_error = None;
            node.complete_runtime_execution(now_rfc3339_like());
        }
        ProviderRunStatus::Interrupted
        | ProviderRunStatus::WaitingForUserInput
        | ProviderRunStatus::PermissionRequested => {
            node.status = DynamicNodeStatus::Paused;
            node.outcome = None;
            node.pause_runtime_execution(now_rfc3339_like());
        }
    }
    validate_dynamic_node_state(node)
}

fn interrupted_dynamic_output_artifact_candidate(
    result: &ProviderRunResult,
) -> Option<crate::provider::OutputArtifactPayload> {
    (result.status == ProviderRunStatus::Interrupted)
        .then(|| {
            result
                .result_payload
                .as_ref()
                .and_then(|payload| payload.output_artifact.clone())
        })
        .flatten()
}

fn interrupted_dynamic_completion_is_trusted(emission_mode: OutputEmissionMode) -> bool {
    emission_mode == OutputEmissionMode::InlineControl
}

fn dynamic_output_artifact_path(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
    artifact_name: &str,
) -> Utf8PathBuf {
    ctx.app.paths.dynamic_node_artifact_file(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        node_id,
        attempt_id,
        artifact_name,
    )
}

fn write_dynamic_output_artifact(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
    output_artifact: &crate::provider::OutputArtifactPayload,
) -> Result<Option<Utf8PathBuf>> {
    if output_artifact.content.trim().is_empty() {
        return Ok(None);
    }
    annotate_dynamic_runtime_control_output_best_effort(
        ctx,
        node_id,
        attempt_id,
        &output_artifact.name,
        "dynamic-node-completion",
    );
    let artifact_path =
        dynamic_output_artifact_path(ctx, node_id, attempt_id, &output_artifact.name);
    std::fs::create_dir_all(
        ctx.app
            .paths
            .dynamic_node_artifacts_dir(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                node_id,
                attempt_id,
            )
            .as_std_path(),
    )?;
    std::fs::write(artifact_path.as_std_path(), &output_artifact.content)?;
    Ok(Some(artifact_path))
}

fn annotate_dynamic_runtime_control_output_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    node_id: &str,
    attempt_id: &str,
    artifact_name: &str,
    kind: &str,
) {
    let path = ctx
        .app
        .paths
        .dynamic_node_attempt_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            node_id,
            attempt_id,
        )
        .join("acp.timeline.jsonl");
    if let Err(error) = annotate_latest_runtime_control_output(&path, artifact_name, kind) {
        tracing::warn!(
            task_id = ctx.task_id,
            run_id = ctx.run_id,
            round_id = ctx.round_id,
            outer_node_id = ctx.outer_node_id,
            outer_attempt_id = ctx.outer_attempt_id,
            node_id,
            attempt_id,
            artifact_name,
            error = %error,
            "failed to annotate dynamic runtime control output display"
        );
    }
}

fn try_accept_interrupted_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
    attempt_id: &str,
    candidate_artifact: Option<&crate::provider::OutputArtifactPayload>,
) -> Result<Option<DynamicProposalState>> {
    let Some(proposal) = try_build_accepted_interrupted_dynamic_completion(
        ctx,
        node,
        attempt_id,
        candidate_artifact,
    )?
    else {
        return Ok(None);
    };
    accept_interrupted_dynamic_completion(ctx, node, attempt_id, candidate_artifact, proposal)
        .map(Some)
}

fn try_build_accepted_interrupted_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
    attempt_id: &str,
    candidate_artifact: Option<&crate::provider::OutputArtifactPayload>,
) -> Result<Option<DynamicProposalState>> {
    match build_interrupted_dynamic_completion(ctx, attempt_id, node, candidate_artifact) {
        Ok(proposal) if proposal.validation_status == DynamicProposalValidationStatus::Accepted => {
            Ok(Some(proposal))
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
            Ok(None)
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
            Ok(None)
        }
    }
}

fn accept_interrupted_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
    node: &mut DynamicNodeState,
    attempt_id: &str,
    candidate_artifact: Option<&crate::provider::OutputArtifactPayload>,
    proposal: DynamicProposalState,
) -> Result<DynamicProposalState> {
    if let Some(candidate_artifact) = candidate_artifact {
        write_dynamic_output_artifact(ctx, &node.id, attempt_id, candidate_artifact)?;
    }
    node.status = DynamicNodeStatus::Completed;
    node.outcome = Some(NodeOutcome::Success);
    node.pause_reason = None;
    node.runtime_error = None;
    let now = now_rfc3339_like();
    node.complete_runtime_execution(now.clone());
    node.finished_at = Some(now);
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
    Ok(proposal)
}

fn build_interrupted_dynamic_completion(
    ctx: &DynamicExecutionContext<'_>,
    attempt_id: &str,
    node: &DynamicNodeState,
    candidate_artifact: Option<&crate::provider::OutputArtifactPayload>,
) -> Result<DynamicProposalState> {
    if let Some(candidate_artifact) = candidate_artifact {
        ensure!(
            candidate_artifact.name == DYNAMIC_COMPLETION_ARTIFACT,
            "interrupted dynamic worker returned unexpected artifact `{}`",
            candidate_artifact.name
        );
        return build_dynamic_completion_from_content(
            ctx,
            attempt_id,
            node,
            &candidate_artifact.content,
        );
    }
    build_dynamic_completion_from_artifact(ctx, attempt_id, node)
}

fn build_dynamic_completion_from_artifact(
    ctx: &DynamicExecutionContext<'_>,
    attempt_id: &str,
    node: &DynamicNodeState,
) -> Result<DynamicProposalState> {
    let artifact_path =
        dynamic_output_artifact_path(ctx, &node.id, attempt_id, DYNAMIC_COMPLETION_ARTIFACT);
    ensure!(
        artifact_path.exists(),
        "dynamic node `{}` did not produce dynamic-node-completion",
        node.id
    );
    let raw = std::fs::read_to_string(artifact_path.as_std_path())?;
    build_dynamic_completion_from_raw(ctx, attempt_id, node, &raw, artifact_path)
}

fn build_dynamic_completion_from_content(
    ctx: &DynamicExecutionContext<'_>,
    attempt_id: &str,
    node: &DynamicNodeState,
    raw: &str,
) -> Result<DynamicProposalState> {
    let artifact_path =
        dynamic_output_artifact_path(ctx, &node.id, attempt_id, DYNAMIC_COMPLETION_ARTIFACT);
    build_dynamic_completion_from_raw(ctx, attempt_id, node, raw, artifact_path)
}

fn build_dynamic_completion_from_raw(
    ctx: &DynamicExecutionContext<'_>,
    attempt_id: &str,
    node: &DynamicNodeState,
    raw: &str,
    artifact_path: Utf8PathBuf,
) -> Result<DynamicProposalState> {
    let graph: DynamicGraphState = {
        let lock = dynamic_state_lock(ctx)?;
        let _guard = lock.lock();
        load_dynamic_graph(
            &ctx.app.paths.dynamic_graph_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
            ),
            &ctx.app.paths.repo_root,
        )?
    };
    let (completion, parsed, schema_errors) = parse_dynamic_completion_artifact(ctx, &graph, raw)?;
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
        let _guard = lock.lock();
        load_dynamic_graph(
            &ctx.app.paths.dynamic_graph_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
            ),
            &ctx.app.paths.repo_root,
        )?
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
            errors.extend(validate_dynamic_node_spec(
                ctx, graph, source, node, 1, false,
            ));
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
                    "dynamic fanout must create at least two nodes",
                    serde_json::json!({
                        "field": "next.nodes",
                        "actual": 0,
                        "expected": "at least 2",
                    }),
                ));
            }
            if nodes.len() == 1 {
                let mut error = dynamic_validation_error(
                    "dynamic.fanout.nodes.too-few",
                    "dynamic fanout must create at least two nodes; use next.type=single for one successor",
                    serde_json::json!({
                        "field": "next.nodes",
                        "actual": nodes.len(),
                        "expected": "at least 2",
                    }),
                );
                error.suggestion = Some(
                    "replace next.type=\"fanout\" with next.type=\"single\" when there is only one successor node".to_string(),
                );
                errors.push(error);
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
                    true,
                ));
            }
        }
    }
    errors
}

fn validate_dynamic_permission_mode(
    ctx: &DynamicExecutionContext<'_>,
    provider: &str,
    permission_mode: &str,
    make_error: impl FnOnce() -> DynamicProposalValidationError,
) -> Option<DynamicProposalValidationError> {
    let capabilities = provider_diagnostic_capabilities(ctx, provider);
    let supported = supported_modes_from_capabilities(capabilities.as_ref());
    let supported_ids: Vec<_> = supported.into_iter().map(|m| m.id).collect();
    if !supported_ids.is_empty() && !supported_ids.iter().any(|id| id == permission_mode) {
        Some(make_error())
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
    _allow_worktree: bool,
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
    let dependency_workspace_ids = spec
        .depends_on
        .iter()
        .filter_map(|dependency| graph.nodes.iter().find(|node| node.id == *dependency))
        .map(|node| node.workspace_id.as_str())
        .collect::<HashSet<_>>();
    if dependency_workspace_ids.len() > 1 {
        let mut error = dynamic_validation_error(
            "dynamic.node.dependencies.workspace-diverged",
            format!(
                "dynamic node `{}` depends on nodes from multiple unmerged workspaces",
                spec.id
            ),
            serde_json::json!({ "nodeId": spec.id, "workspaceIds": dependency_workspace_ids }),
        );
        error.suggestion = Some(
            "converge the branches through their fanout group merge before creating this node"
                .to_string(),
        );
        errors.push(error);
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
                    } else if let Some(permission_mode) =
                        dynamic_permission_mode_for_provider(ctx.dynamic, provider)
                    {
                        if let Some(error) = validate_dynamic_permission_mode(
                            ctx,
                            provider,
                            &permission_mode,
                            || {
                                dynamic_validation_error(
                                    "dynamic.node.permission-mode.unsupported",
                                    format!(
                                        "dynamic worker `{}` permissionMode `{}` is not supported by provider `{provider}`",
                                        spec.id, permission_mode
                                    ),
                                    serde_json::json!({
                                        "nodeId": spec.id,
                                        "provider": provider,
                                        "permissionMode": permission_mode,
                                    }),
                                )
                            },
                        ) {
                            errors.push(error);
                        }
                    }
                    let proposed_model = spec
                        .model
                        .as_deref()
                        .map(str::trim)
                        .filter(|model| !model.is_empty());
                    if let Some(error) = validate_dynamic_proposed_model(
                        ctx,
                        provider,
                        proposed_model,
                        dynamic_worker_model_required_from_proposal(ctx, provider),
                        "dynamic.node",
                        &format!("dynamic worker `{}`", spec.id),
                        serde_json::json!({ "nodeId": spec.id }),
                    ) {
                        errors.push(error);
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
    if proposed_provider.is_some() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.provider.unsupported"),
            format!(
                "dynamic {name} must not output provider; runtime uses the control-plane agent"
            ),
            serde_json::json!({
                "field": "provider",
                "stage": name,
                "provider": proposed_provider.unwrap(),
                "expected": "omit this field",
            }),
        ));
    }
    let resolved_provider = dynamic_control_provider(ctx.dynamic);
    if ctx.app.provider_for_id(resolved_provider).is_err() {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.provider.unknown"),
            format!("dynamic {name} references unknown provider `{resolved_provider}`"),
            serde_json::json!({
                "provider": resolved_provider,
                "stage": name,
            }),
        ));
    } else if let Some(permission_mode) = dynamic_control_permission_mode(ctx.dynamic)
        && let Some(error) = validate_dynamic_permission_mode(
            ctx,
            resolved_provider,
            &permission_mode,
            || {
                dynamic_validation_error(
                    &format!("dynamic.{name}.permission-mode.unsupported"),
                    format!(
                        "dynamic {name} permissionMode `{}` is not supported by provider `{resolved_provider}`",
                        permission_mode
                    ),
                    serde_json::json!({
                        "provider": resolved_provider,
                        "stage": name,
                        "permissionMode": permission_mode,
                    }),
                )
            },
        )
    {
        errors.push(error);
    }
    let proposed_model = spec
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(model) = proposed_model
        && matches!(
            ctx.dynamic.agent_strategy,
            AiDynamicAgentStrategy::Dynamic { .. }
        )
    {
        errors.push(dynamic_validation_error(
            &format!("dynamic.{name}.model.unsupported"),
            format!(
                "dynamic {name} must not output model; runtime uses the configured acceptance model"
            ),
            serde_json::json!({
                "provider": resolved_provider,
                "stage": name,
                "field": "model",
                "actual": model,
                "expected": "omit this field",
            }),
        ));
    } else if matches!(
        ctx.dynamic.agent_strategy,
        AiDynamicAgentStrategy::Fixed { .. }
    ) && let Some(error) = validate_dynamic_proposed_model(
        ctx,
        resolved_provider,
        proposed_model,
        dynamic_agent_task_model_required_from_proposal(ctx, resolved_provider),
        &format!("dynamic.{name}"),
        &format!("dynamic {name}"),
        serde_json::json!({ "stage": name }),
    ) {
        errors.push(error);
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
    spec.provider = dynamic_control_provider(ctx.dynamic).to_string();
    spec.model = match &ctx.dynamic.agent_strategy {
        AiDynamicAgentStrategy::Fixed { model, .. } => model.clone().or_else(|| {
            spec.model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string)
        }),
        AiDynamicAgentStrategy::Dynamic { .. } => {
            dynamic_acceptance_model(ctx.dynamic).map(ToOwned::to_owned)
        }
    };
    Ok(spec)
}

fn materialize_dynamic_next(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    source_index: usize,
    next: DynamicNext,
) -> Result<Vec<String>> {
    let mut visible_node_ids = Vec::new();
    let source_node_id = graph
        .nodes
        .get(source_index)
        .map(|source| source.id.clone())
        .ok_or_else(|| anyhow!("dynamic proposal source node is missing"))?;
    let source_is_acceptance = graph
        .nodes
        .get(source_index)
        .map(|source| source.kind == DynamicNodeKind::Acceptance)
        .unwrap_or(false);
    let source_group_id = graph
        .nodes
        .get(source_index)
        .and_then(|source| source.group_id.clone());
    match next {
        DynamicNext::End => {
            with_dynamic_workspace_transition(
                ctx,
                graph,
                std::slice::from_ref(&source_node_id),
                |graph| {
                    let source = graph.nodes[source_index].clone();
                    checkpoint_dynamic_workspace(
                        graph,
                        &source.workspace_id,
                        source.group_id.as_deref(),
                    )?;
                    if let Some(group_id) = source.group_id.as_deref() {
                        if let Some(group) =
                            graph.groups.iter_mut().find(|group| group.id == group_id)
                        {
                            if !group.terminal_node_ids.iter().any(|id| id == &source.id) {
                                group.terminal_node_ids.push(source.id.clone());
                            }
                            group.updated_at = now_rfc3339_like();
                        }
                    }
                    Ok(())
                },
            )?;
        }
        DynamicNext::Single { node } => {
            reopen_acceptance_group_for_repair(
                graph,
                source_is_acceptance,
                source_group_id.as_deref(),
            );
            let source = graph.nodes[source_index].clone();
            let new_node = dynamic_node_state_from_spec(
                ctx,
                graph,
                &source,
                node,
                source.group_id.clone(),
                source.chain_id.clone(),
                source.workspace_id.clone(),
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
            with_dynamic_workspace_transition(
                ctx,
                graph,
                std::slice::from_ref(&source_node_id),
                |graph| {
                    reopen_acceptance_group_for_repair(
                        graph,
                        source_is_acceptance,
                        source_group_id.as_deref(),
                    );
                    let source = graph.nodes[source_index].clone();
                    let merge = dynamic_agent_task_spec_with_resolved_provider(ctx, merge)?;
                    let acceptance =
                        dynamic_agent_task_spec_with_resolved_provider(ctx, acceptance)?;
                    let group_depth = source
                        .group_id
                        .as_deref()
                        .and_then(|group_id| graph.groups.iter().find(|group| group.id == group_id))
                        .map(|group| group.depth + 1)
                        .unwrap_or(1);
                    let root_node_ids =
                        nodes.iter().map(|node| node.id.clone()).collect::<Vec<_>>();
                    let mut child_workspace_ids = Vec::with_capacity(nodes.len());
                    for node in &nodes {
                        child_workspace_ids.push(fork_dynamic_workspace(
                            ctx,
                            graph,
                            &source.workspace_id,
                            &group_id,
                            &node.id,
                        )?);
                    }
                    {
                        let parent = dynamic_workspace_mut(graph, &source.workspace_id)?;
                        parent.status = WorkspaceStatus::Frozen;
                        parent.updated_at = now_rfc3339_like();
                    }
                    let group = DynamicGroupState {
                        version: VERSION.to_string(),
                        id: group_id.clone(),
                        dynamic_run_id: graph.run.id.clone(),
                        status: DynamicGroupStatus::Open,
                        depth: group_depth,
                        parent_group_id: source.group_id.clone(),
                        root_node_ids: root_node_ids.clone(),
                        terminal_node_ids: Vec::new(),
                        target_workspace_id: source.workspace_id.clone(),
                        child_workspace_ids: child_workspace_ids.clone(),
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
                    for (node, workspace_id) in nodes.into_iter().zip(child_workspace_ids) {
                        let chain_id = node.id.clone();
                        let new_node = dynamic_node_state_from_spec(
                            ctx,
                            graph,
                            &source,
                            node,
                            Some(group_id.clone()),
                            chain_id,
                            workspace_id,
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
                    Ok(())
                },
            )?;
        }
    }
    let promoted_node_ids = refresh_dynamic_ready_nodes(graph);
    visible_node_ids.retain(|node_id| promoted_node_ids.iter().any(|promoted| promoted == node_id));
    graph.run.updated_at = now_rfc3339_like();
    Ok(visible_node_ids)
}

fn reopen_acceptance_group_for_repair(
    graph: &mut DynamicGraphState,
    source_is_acceptance: bool,
    group_id: Option<&str>,
) {
    if !source_is_acceptance {
        return;
    }
    let Some(group_id) = group_id else {
        return;
    };
    if let Some(group) = graph.groups.iter_mut().find(|group| group.id == group_id) {
        group.status = DynamicGroupStatus::Open;
        group.merge_node_id = None;
        group.acceptance_node_id = None;
        group.updated_at = now_rfc3339_like();
    }
}

fn dynamic_node_state_from_spec(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    source: &DynamicNodeState,
    spec: DynamicNodeSpec,
    group_id: Option<String>,
    chain_id: String,
    workspace_id: String,
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
    let permission_mode = provider
        .as_deref()
        .and_then(|provider| dynamic_permission_mode_for_provider(ctx.dynamic, provider));
    let node = DynamicNodeState {
        version: VERSION.to_string(),
        acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
        id: spec.id,
        dynamic_run_id: graph.run.id.clone(),
        kind,
        title: spec.title,
        task: spec.task,
        status: DynamicNodeStatus::Pending,
        outcome: None,
        pause_reason: None,
        runtime_error: None,
        runtime_execution_id: None,
        runtime_execution_phase: None,
        runtime_lifecycle_revision: 0,
        runtime_lifecycle_updated_at: None,
        group_id,
        chain_id,
        depth: source.depth + 1,
        depends_on: spec.depends_on,
        workspace_id,
        provider,
        profile: spec.profile,
        model,
        permission_mode,
        session_mode: spec.session_mode,
        continue_from_node_id: spec.continue_from_node_id,
        workflow_id: spec.workflow_id,
        workflow_snapshot_id,
        child_run_id: None,
        started_at: None,
        finished_at: None,
        uuid: Some(generate_uuid()),
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
                let transition_owner_node_ids = graph.groups[group_index].terminal_node_ids.clone();
                let merge_node_id = with_dynamic_workspace_transition(
                    ctx,
                    graph,
                    &transition_owner_node_ids,
                    |graph| {
                        let group_id = graph.groups[group_index].id.clone();
                        let child_workspace_ids =
                            graph.groups[group_index].child_workspace_ids.clone();
                        for workspace_id in child_workspace_ids {
                            checkpoint_dynamic_workspace(graph, &workspace_id, Some(&group_id))?;
                        }
                        let target_workspace_id =
                            graph.groups[group_index].target_workspace_id.clone();
                        {
                            let target = dynamic_workspace_mut(graph, &target_workspace_id)?;
                            target.status = WorkspaceStatus::Merging;
                            target.updated_at = now_rfc3339_like();
                        }
                        let merge_node = create_dynamic_merge_node(ctx, graph, group_index)?;
                        graph.groups[group_index].status = DynamicGroupStatus::Merging;
                        graph.groups[group_index].merge_node_id = Some(merge_node.id.clone());
                        graph.groups[group_index].updated_at = now_rfc3339_like();
                        let merge_node_id = merge_node.id.clone();
                        graph.nodes.push(merge_node);
                        append_dynamic_event(
                            ctx,
                            "dynamic_group_merge_started",
                            serde_json::json!({
                                "groupId": group_id,
                            }),
                        )?;
                        Ok(merge_node_id)
                    },
                )?;
                visible_node_ids.push(merge_node_id);
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
                if acceptance_completed_with_end(
                    graph,
                    graph.groups[group_index].acceptance_node_id.as_deref(),
                ) =>
            {
                let transition_owner_node_ids = graph.groups[group_index]
                    .acceptance_node_id
                    .clone()
                    .into_iter()
                    .collect::<Vec<_>>();
                with_dynamic_workspace_transition(
                    ctx,
                    graph,
                    &transition_owner_node_ids,
                    |graph| {
                        let group_id = graph.groups[group_index].id.clone();
                        let child_workspace_ids =
                            graph.groups[group_index].child_workspace_ids.clone();
                        let target_workspace_id =
                            graph.groups[group_index].target_workspace_id.clone();
                        graph.groups[group_index].status = DynamicGroupStatus::Closed;
                        graph.groups[group_index].updated_at = now_rfc3339_like();
                        for workspace_id in child_workspace_ids {
                            release_dynamic_workspace_best_effort(ctx, graph, &workspace_id);
                        }
                        if let Ok(target) = dynamic_workspace_mut(graph, &target_workspace_id) {
                            target.status = WorkspaceStatus::Active;
                            target.updated_at = now_rfc3339_like();
                        }
                        attach_closed_child_group_to_parent(graph, group_index);
                        append_dynamic_event(
                            ctx,
                            "dynamic_group_closed",
                            serde_json::json!({
                                "groupId": group_id,
                            }),
                        )?;
                        Ok(())
                    },
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

fn acceptance_completed_with_end(graph: &DynamicGraphState, node_id: Option<&str>) -> bool {
    let Some(node_id) = node_id else {
        return false;
    };
    if !group_node_completed(graph, Some(node_id)) {
        return false;
    }
    graph.proposals.iter().any(|proposal| {
        if proposal.source_node_id != node_id
            || proposal.validation_status != DynamicProposalValidationStatus::Accepted
        {
            return false;
        }
        serde_json::from_value::<DynamicNodeCompletion>(proposal.parsed.clone())
            .map(|completion| matches!(completion.next, DynamicNext::End))
            .unwrap_or(false)
    })
}

fn unique_dynamic_node_id(graph: &DynamicGraphState, base: &str) -> String {
    if !graph.nodes.iter().any(|node| node.id == base) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if !graph.nodes.iter().any(|node| node.id == candidate) {
            return candidate;
        }
    }
    unreachable!()
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
    let id = unique_dynamic_node_id(graph, &format!("{}-merge", group.id));
    let task = group.merge.task.clone();
    let node = DynamicNodeState {
        version: VERSION.to_string(),
        acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
        id: id.clone(),
        dynamic_run_id: graph.run.id.clone(),
        kind: DynamicNodeKind::Merge,
        title: group.merge.title.clone(),
        task,
        status: DynamicNodeStatus::Ready,
        outcome: None,
        pause_reason: None,
        runtime_error: None,
        runtime_execution_id: None,
        runtime_execution_phase: None,
        runtime_lifecycle_revision: 0,
        runtime_lifecycle_updated_at: None,
        group_id: Some(group.id.clone()),
        chain_id: id.clone(),
        depth: group.depth,
        depends_on: group.terminal_node_ids.clone(),
        workspace_id: group.target_workspace_id.clone(),
        provider: Some(group.merge.provider.clone()),
        profile: None,
        model: group.merge.model.clone(),
        permission_mode: dynamic_control_permission_mode(ctx.dynamic),
        session_mode: SessionMode::New,
        continue_from_node_id: None,
        workflow_id: None,
        workflow_snapshot_id: None,
        child_run_id: None,
        started_at: None,
        finished_at: None,
        uuid: Some(generate_uuid()),
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
    let id = unique_dynamic_node_id(graph, &format!("{}-accept", group.id));
    let task = group.acceptance.task.clone();
    let node = DynamicNodeState {
        version: VERSION.to_string(),
        acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
        id: id.clone(),
        dynamic_run_id: graph.run.id.clone(),
        kind: DynamicNodeKind::Acceptance,
        title: group.acceptance.title.clone(),
        task,
        status: DynamicNodeStatus::Ready,
        outcome: None,
        pause_reason: None,
        runtime_error: None,
        runtime_execution_id: None,
        runtime_execution_phase: None,
        runtime_lifecycle_revision: 0,
        runtime_lifecycle_updated_at: None,
        group_id: Some(group.id.clone()),
        chain_id: id.clone(),
        depth: group.depth,
        depends_on: vec![merge_node_id.clone()],
        workspace_id: group.target_workspace_id.clone(),
        provider: Some(group.acceptance.provider.clone()),
        profile: None,
        model: group.acceptance.model.clone(),
        permission_mode: dynamic_control_permission_mode(ctx.dynamic),
        session_mode: SessionMode::New,
        continue_from_node_id: None,
        workflow_id: None,
        workflow_snapshot_id: None,
        child_run_id: None,
        started_at: None,
        finished_at: None,
        uuid: Some(generate_uuid()),
    };
    validate_dynamic_node_state(&node)?;
    Ok(node)
}

fn dynamic_group_workspace_summary(
    _ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    group: &DynamicGroupState,
) -> String {
    let repository = GitRepositoryService::default();
    let lines = group.child_workspace_ids.iter().filter_map(|workspace_id| {
        let workspace = dynamic_workspace(graph, workspace_id).ok()?;
        let head = repository.head(&workspace.path).unwrap_or_else(|_| "unknown".to_string());
        let status = repository.status_porcelain(&workspace.path)
            .map(|value| if value.is_empty() { "clean".to_string() } else { value.replace('\n', "; ") })
            .unwrap_or_else(|_| "unavailable".to_string());
        Some(format!(
            "- workspaceId={} path={} branch={} parentWorkspaceId={} forkCommit={} checkpointCommit={} head={} status={}",
            workspace.id, workspace.path, workspace.branch.as_deref().unwrap_or("none"),
            workspace.parent_workspace_id.as_deref().unwrap_or("none"), workspace.fork_commit,
            workspace.checkpoint_commit.as_deref().unwrap_or("none"), head, status,
        ))
    }).collect::<Vec<_>>();
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

pub(crate) fn prepare_dynamic_acp_prompt(
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
) -> Result<PreparedAcpPrompt> {
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
        validated = Some(validate_workflow_snapshot(workflow)?);
        dynamic = match validated.as_ref().unwrap().get_node(outer_node_id) {
            Some(NodeDsl::AiDynamic(d)) => d,
            _ => return Err(anyhow!("node `{outer_node_id}` is not an ai-dynamic node")),
        };
    }
    let run: RunState = read_json(&app.paths.run_file(task_id, run_id))?;
    validate_run_state(&run)?;
    let round: RoundState = read_json(&app.paths.round_file(task_id, run_id, round_id))?;
    validate_round_state(&round)?;
    let graph = load_dynamic_graph(
        &app.paths
            .dynamic_graph_file(task_id, run_id, round_id, outer_node_id, outer_attempt_id),
        &app.paths.repo_root,
    )?;
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
        outer_runtime_execution_id: None,
        dynamic,
        task_uuid: None,
        run_uuid: None,
        round_uuid: None,
        outer_node_uuid: None,
        parent_continue_input: None,
        parent_continue_prompt_id: None,
        resume_override: None,
    };
    let output_contract = dynamic_output_contract_for_node(&ctx, &graph, &node);
    let logical_prompt_id = logical_prompt_id(prompt_id);
    let mut invocation = build_dynamic_worker_invocation(
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
        logical_prompt_id,
        None,
        PromptVisibility::Visible,
        UserPromptRenderMode::UserMessage,
        Vec::new(),
        None,
        None,
    )?;
    invocation.turn_control_mode = TurnControlMode::NonRuntimeControlled;
    invocation.runtime_control_intent = RuntimeControlIntent::ManualFollowUp;
    invocation.extra_hidden_sections.clear();
    Ok(PreparedAcpPrompt {
        prompt: render_prompt_bundle(&invocation)?,
        adapter_workspace_dir: invocation.adapter_workspace_dir,
        session_workspace_dir: invocation.workspace_dir,
    })
}

pub(crate) fn run_continue_with_prompt_input(
    app: &App,
    task_id: &str,
    run_id: &str,
    prompt_id: Option<String>,
    input: Option<ConversationPromptInput>,
) -> Result<RunState> {
    run_continue_with_launch_signal(
        app,
        task_id,
        run_id,
        prompt_id,
        input,
        Vec::new(),
        None,
        None,
        None,
        None,
    )
}

fn build_dynamic_worker_invocation(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
    attempt_id: &str,
    mut output_contract: Option<PromptOutputContract>,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
    resume_prompt: Option<String>,
    logical_prompt_id: String,
    prompt_display: Option<ConversationPromptInput>,
    resume_prompt_visibility: PromptVisibility,
    user_prompt_render_mode: UserPromptRenderMode,
    resume_input_attachment_paths: Vec<String>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
) -> Result<WorkerInvocation> {
    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "runtime_context");
    let runtime_context = dynamic_runtime_context(ctx, &node.id, attempt_id);
    let mut config_options = dynamic_config_options_for_invocation(ctx.dynamic, node);
    config_options.extend(dynamic_acp_config_option_overrides(
        &runtime_context.attempt_dir,
    ));
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
    let profile_entry = match builtin_profile {
        Some(_) => None,
        None => node
            .profile
            .as_deref()
            .and_then(|profile| ctx.app.profile_show(profile).ok()),
    };
    let profile_content = match builtin_profile {
        Some((_, content)) => Some(content.trim().to_string()),
        None => profile_entry.as_ref().map(|entry| entry.content.clone()),
    };
    let profile_dynamic_template = builtin_profile.is_some()
        || profile_entry
            .as_ref()
            .is_some_and(|entry| entry.dynamic_template);
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
    let workspace_dir = dynamic_workspace(graph, &node.workspace_id)?.path.clone();
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "workspace_dir",
        step_started_at,
        serde_json::json!({
            "workspaceId": node.workspace_id,
            "workspacePath": workspace_dir,
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "system_sections");
    let control_emission_mode = output_contract
        .as_ref()
        .map(|contract| contract.emission_mode);
    let has_output_contract =
        control_emission_mode == Some(OutputEmissionMode::InlineControl);
    let extra_system_sections = dynamic_system_sections(ctx, control_emission_mode)?;
    let extra_hidden_sections = dynamic_hidden_sections(
        ctx,
        graph,
        node,
        attempt_id,
        session_mode,
        has_output_contract,
    )?;
    if let Some(contract) = output_contract
        .as_mut()
        .filter(|contract| contract.emission_mode == OutputEmissionMode::PostTurnProjection)
    {
        contract.finalize_context = Some(
            dynamic_hidden_sections(ctx, graph, node, attempt_id, session_mode, true)?
                .into_iter()
                .map(|section| section.content)
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    }
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
    let model = resolve_dynamic_invocation_model(ctx.dynamic, node, model_override);
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
    let task_instruction = dynamic_task_instruction(ctx, graph, node, has_output_contract);
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
    let permission_mode = permission_mode_override
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| node.permission_mode.clone());
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
    let task_input_attachment_paths = if matches!(session_mode, SessionMode::New) {
        super::task_input_attachment_paths(ctx.app, ctx.task_id)
    } else {
        Vec::new()
    };
    let user_input_attachment_paths = resume_input_attachment_paths;
    dynamic_invocation_build_step_end(
        ctx,
        node,
        attempt_id,
        "input_attachment_paths",
        step_started_at,
        serde_json::json!({
            "taskInputCount": task_input_attachment_paths.len(),
            "userInputCount": user_input_attachment_paths.len(),
        }),
    );

    let step_started_at =
        dynamic_invocation_build_step_begin(ctx, node, attempt_id, "assemble_invocation");
    let invocation = WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        turn_control_mode: TurnControlMode::RuntimeControlled,
        runtime_control_intent: RuntimeControlIntent::Unchanged,
        prompt_envelope: crate::dsl::PromptEnvelopeMode::RuntimeManaged,
        execution_surface: PromptExecutionSurface::AiDynamic,
        profile,
        profile_content,
        profile_dynamic_template,
        requirement_path: None,
        requirement_text: Some(requirement_text),
        adapter_workspace_dir: ctx.app.paths.repo_root.clone(),
        workspace_dir,
        attempt_dir: runtime_context.attempt_dir.clone(),
        output_contract,
        runtime_context,
        predecessors,
        new_round_trigger: None,
        extra_system_sections,
        extra_hidden_sections,
        task_instruction: Some(task_instruction.clone()),
        user_tips_instruction: dynamic_user_tips_instruction(ctx),
        resume_task_instruction: dynamic_resume_task_instruction(session_mode, &task_instruction),
        session_mode,
        user_prompt_render_mode,
        permission_mode,
        model,
        config_options,
        continue_ref,
        resume_prompt,
        resume_prompt_id: Some(logical_prompt_id),
        prompt_display,
        resume_prompt_visibility,
        stream_mode: StreamMode::StreamJson,
        log_prompts: ctx.app.config.log_prompts,
        log_provider_command: ctx.app.config.log_provider_command,
        attachments_dir: Some(attachments_dir),
        cold_artifacts: Vec::new(),
        cold_attachments: Vec::new(),
        task_input_attachment_paths,
        user_input_attachment_paths,
        attachment_projection_policy: crate::provider::AttachmentProjectionPolicy::from(
            &ctx.app.config,
        ),
        mcp_servers: Vec::new(),
        scheduled_context: None,
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
            "inputAttachmentCount": invocation.task_input_attachment_paths.len()
                + invocation.user_input_attachment_paths.len(),
        }),
    );
    Ok(invocation)
}

fn dynamic_acp_config_option_overrides(attempt_dir: &Utf8Path) -> BTreeMap<String, String> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return BTreeMap::new();
    };
    crate::acp::events::load_session_metadata(&path, None)
        .ok()
        .map(|metadata| metadata.config_option_overrides)
        .unwrap_or_default()
}

fn dynamic_builtin_profile(
    language: DesktopLanguage,
    node: &DynamicNodeState,
) -> Option<(&'static str, &'static str)> {
    match node.kind {
        DynamicNodeKind::Worker if dynamic_node_is_bootstrap_dispatch(node) => Some((
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
            available_agents, ..
        } => available_agents
            .iter()
            .map(|agent| agent.provider.clone())
            .collect(),
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
            return provider_model_option_values(ctx, provider);
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
    has_output_contract: bool,
) -> String {
    let metadata = render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_NODE_TASK_ZH_CN,
            AI_DYNAMIC_NODE_TASK_EN,
        ),
        serde_json::json!({
            "title": node.title,
            "has_output_contract": has_output_contract,
        }),
    )
    .expect("prompt template renders");
    let task = node.task.trim().to_string();
    let metadata = metadata.trim();
    if metadata.is_empty() {
        task
    } else if task.is_empty() {
        metadata.to_string()
    } else {
        format!("{}\n\n{}", task, metadata)
    }
}

fn dynamic_user_tips_instruction(ctx: &DynamicExecutionContext<'_>) -> Option<String> {
    ctx.dynamic
        .global_goal()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
            output_artifact: None,
            branch_reason: dependency.finished_at.clone(),
            attachments: Vec::new(),
        })
        .collect()
}

struct DynamicContextProjection {
    direct_predecessors: String,
    has_direct_predecessors: bool,
    active_group: String,
    has_active_group: bool,
    inherited_groups: String,
    has_inherited_groups: bool,
    siblings: String,
    has_siblings: bool,
    available_attachments: String,
    has_available_attachments: bool,
}

#[derive(Clone)]
struct DynamicAttachmentEntry {
    node_id: String,
    attempt_id: String,
    name: String,
    path: Utf8PathBuf,
}

fn dynamic_context_projection(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> DynamicContextProjection {
    let direct_nodes = dynamic_direct_predecessor_nodes(graph, node);
    let direct_predecessors = dynamic_node_list_summary(&direct_nodes);
    let active_group = dynamic_active_group_summary(ctx, graph, node);
    let inherited_groups = dynamic_inherited_group_summary(graph, node);
    let siblings = dynamic_sibling_summary(graph, node);
    let available_attachments =
        dynamic_attachment_manifest_summary(ctx, graph, node, &direct_nodes);
    DynamicContextProjection {
        has_direct_predecessors: !direct_predecessors.is_empty(),
        direct_predecessors,
        has_active_group: !active_group.is_empty(),
        active_group,
        has_inherited_groups: !inherited_groups.is_empty(),
        inherited_groups,
        has_siblings: !siblings.is_empty(),
        siblings,
        has_available_attachments: !available_attachments.is_empty(),
        available_attachments,
    }
}

fn dynamic_direct_predecessor_nodes<'a>(
    graph: &'a DynamicGraphState,
    node: &DynamicNodeState,
) -> Vec<&'a DynamicNodeState> {
    let mut nodes = Vec::new();
    let mut seen = HashSet::<String>::new();
    for dependency_id in &node.depends_on {
        if let Some(dependency) = graph.nodes.iter().find(|item| item.id == *dependency_id) {
            if seen.insert(dependency.id.clone()) {
                nodes.push(dependency);
            }
        }
    }
    if nodes.is_empty() {
        if let Some(previous) = graph
            .nodes
            .iter()
            .filter(|candidate| candidate.id != node.id)
            .filter(|candidate| candidate.chain_id == node.chain_id)
            .filter(|candidate| candidate.group_id == node.group_id)
            .filter(|candidate| candidate.depth < node.depth)
            .max_by_key(|candidate| candidate.depth)
        {
            if seen.insert(previous.id.clone()) {
                nodes.push(previous);
            }
        }
    }
    if nodes.is_empty() {
        if let Some(group_id) = node.group_id.as_deref()
            && let Some(group) = graph.groups.iter().find(|group| group.id == group_id)
            && group
                .root_node_ids
                .iter()
                .any(|node_id| node_id == &node.id)
            && let Some(created_by) = graph
                .nodes
                .iter()
                .find(|candidate| candidate.id == group.created_by_node_id)
            && seen.insert(created_by.id.clone())
        {
            nodes.push(created_by);
        }
    }
    nodes
}

fn dynamic_node_list_summary(nodes: &[&DynamicNodeState]) -> String {
    nodes
        .iter()
        .map(|node| format!("- {}", dynamic_node_ref_summary(node)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn dynamic_node_ref_summary(node: &DynamicNodeState) -> String {
    format!(
        "{} title={} kind={:?} status={:?} outcome={} workspaceId={}",
        node.id,
        node.title,
        node.kind,
        node.status,
        node.outcome
            .map(|outcome| format!("{outcome:?}"))
            .unwrap_or_else(|| "none".to_string()),
        node.workspace_id,
    )
}

fn dynamic_active_group_summary(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> String {
    let Some(group_id) = node.group_id.as_deref() else {
        return String::new();
    };
    let Some(group) = graph.groups.iter().find(|group| group.id == group_id) else {
        return String::new();
    };
    let mut lines = vec![
        format!("- groupId: {}", group.id),
        format!("- status: {:?}", group.status),
        format!("- depth: {}", group.depth),
        format!("- createdByNodeId: {}", group.created_by_node_id),
        format!(
            "- parentGroupId: {}",
            group.parent_group_id.as_deref().unwrap_or("none")
        ),
        format!("- root nodes: {}", dynamic_join_ids(&group.root_node_ids)),
        format!(
            "- terminal nodes: {}",
            dynamic_join_ids(&group.terminal_node_ids)
        ),
        format!(
            "- merge node: {}",
            group.merge_node_id.as_deref().unwrap_or("none")
        ),
        format!(
            "- acceptance node: {}",
            group.acceptance_node_id.as_deref().unwrap_or("none")
        ),
    ];
    if matches!(
        node.kind,
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance
    ) {
        lines.push("- branch workspaces:".to_string());
        lines.push(dynamic_group_workspace_summary(ctx, graph, group));
    }
    lines.join("\n")
}

fn dynamic_inherited_group_summary(graph: &DynamicGraphState, node: &DynamicNodeState) -> String {
    let mut lines = Vec::new();
    let mut next_group_id = node
        .group_id
        .as_deref()
        .and_then(|group_id| graph.groups.iter().find(|group| group.id == group_id))
        .and_then(|group| group.parent_group_id.as_deref());
    while let Some(group_id) = next_group_id {
        let Some(group) = graph.groups.iter().find(|group| group.id == group_id) else {
            break;
        };
        lines.push(format!(
            "- {} status={:?} depth={} roots={} merge={} acceptance={}",
            group.id,
            group.status,
            group.depth,
            dynamic_join_ids(&group.root_node_ids),
            group.merge_node_id.as_deref().unwrap_or("none"),
            group.acceptance_node_id.as_deref().unwrap_or("none"),
        ));
        next_group_id = group.parent_group_id.as_deref();
    }
    lines.join("\n")
}

fn dynamic_sibling_summary(graph: &DynamicGraphState, node: &DynamicNodeState) -> String {
    if matches!(
        node.kind,
        DynamicNodeKind::Merge | DynamicNodeKind::Acceptance
    ) {
        return String::new();
    }
    let Some(group_id) = node.group_id.as_deref() else {
        return String::new();
    };
    let Some(group) = graph.groups.iter().find(|group| group.id == group_id) else {
        return String::new();
    };
    group
        .root_node_ids
        .iter()
        .filter(|node_id| *node_id != &node.id)
        .filter_map(|node_id| graph.nodes.iter().find(|candidate| candidate.id == *node_id))
        .map(|sibling| {
            format!(
                "- {} (parallel sibling; not an input dependency) status={:?} outcome={} workspaceId={}",
                sibling.id,
                sibling.status,
                sibling
                    .outcome
                    .map(|outcome| format!("{outcome:?}"))
                    .unwrap_or_else(|| "none".to_string()),
                sibling.workspace_id,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dynamic_attachment_manifest_summary(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
    direct_nodes: &[&DynamicNodeState],
) -> String {
    let mut attachment_node_ids = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for direct in direct_nodes {
        if seen.insert(direct.id.clone()) {
            attachment_node_ids.push(direct.id.clone());
        }
    }
    if let Some(group_id) = node.group_id.as_deref()
        && let Some(group) = graph.groups.iter().find(|group| group.id == group_id)
    {
        match node.kind {
            DynamicNodeKind::Merge => {
                for node_id in group
                    .terminal_node_ids
                    .iter()
                    .chain(group.root_node_ids.iter())
                {
                    if seen.insert(node_id.clone()) {
                        attachment_node_ids.push(node_id.clone());
                    }
                }
            }
            DynamicNodeKind::Acceptance => {
                if let Some(merge_node_id) = group.merge_node_id.as_ref()
                    && seen.insert(merge_node_id.clone())
                {
                    attachment_node_ids.push(merge_node_id.clone());
                }
                for node_id in group
                    .terminal_node_ids
                    .iter()
                    .chain(group.root_node_ids.iter())
                {
                    if seen.insert(node_id.clone()) {
                        attachment_node_ids.push(node_id.clone());
                    }
                }
            }
            DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation => {
                if let Some(parent_group_id) = group.parent_group_id.as_deref() {
                    collect_group_exit_attachment_node_ids(
                        graph,
                        parent_group_id,
                        &mut seen,
                        &mut attachment_node_ids,
                    );
                }
            }
        }
    }
    let mut entries = Vec::new();
    for node_id in attachment_node_ids {
        if let Some(source_node) = graph.nodes.iter().find(|candidate| candidate.id == node_id) {
            entries.extend(dynamic_attachment_entries_for_node(ctx, source_node));
        }
    }
    if entries.is_empty() {
        return String::new();
    }
    entries
        .into_iter()
        .map(|entry| {
            format!(
                "- {}/{}/attachments/{}: {}",
                entry.node_id, entry.attempt_id, entry.name, entry.path
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_group_exit_attachment_node_ids(
    graph: &DynamicGraphState,
    group_id: &str,
    seen: &mut HashSet<String>,
    ids: &mut Vec<String>,
) {
    let Some(group) = graph.groups.iter().find(|group| group.id == group_id) else {
        return;
    };
    for node_id in [
        group.acceptance_node_id.as_ref(),
        group.merge_node_id.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if seen.insert(node_id.clone()) {
            ids.push(node_id.clone());
        }
    }
}

fn dynamic_attachment_entries_for_node(
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
) -> Vec<DynamicAttachmentEntry> {
    let attempt_id = dynamic_attempt_id(node);
    let root = ctx.app.paths.dynamic_node_attachments_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        &node.id,
        &attempt_id,
    );
    if !root.exists() {
        return Vec::new();
    }
    let mut files = Vec::<(String, Utf8PathBuf)>::new();
    collect_dynamic_attachment_files(&root, &root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
        .into_iter()
        .map(|(name, path)| DynamicAttachmentEntry {
            node_id: node.id.clone(),
            attempt_id: attempt_id.clone(),
            name,
            path,
        })
        .collect()
}

fn collect_dynamic_attachment_files(
    root: &Utf8Path,
    dir: &Utf8Path,
    files: &mut Vec<(String, Utf8PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_dynamic_attachment_files(root, &path, files);
        } else if metadata.is_file() {
            let name = path
                .strip_prefix(root)
                .map(|relative| relative.to_string())
                .unwrap_or_else(|_| {
                    path.file_name()
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| path.to_string())
                });
            files.push((name, path));
        }
    }
}

fn dynamic_join_ids(ids: &[String]) -> String {
    if ids.is_empty() {
        "none".to_string()
    } else {
        ids.join(", ")
    }
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
        AiDynamicAgentStrategy::Fixed {
            provider, model, ..
        } => {
            if let Some(model) = model.as_deref() {
                return format!("- {provider} (configured model: {model}; do not output model)");
            }
            let options = provider_model_options_summary(ctx, provider);
            if options.is_empty() {
                format!("- {provider} (model not configured; provider default will be used)")
            } else {
                format!(
                    "- {provider} (model required in proposal; choose one model value)\n  models:\n  - {}",
                    options.join("\n  - ")
                )
            }
        }
        AiDynamicAgentStrategy::Dynamic {
            available_agents, ..
        } => {
            if available_agents.is_empty() {
                return "none".to_string();
            }
            available_agents
                .iter()
                .map(|agent_ref| {
                    let model = agent_ref.model.as_deref().unwrap_or("provider default");
                    let permission = agent_ref
                        .permission_mode
                        .as_deref()
                        .unwrap_or("provider default");
                    format!(
                        "- {provider} (runtime model: {model}; runtime permission mode: {permission}; output provider only)",
                        provider = agent_ref.provider,
                    )
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

fn dynamic_system_sections(
    ctx: &DynamicExecutionContext<'_>,
    control_emission_mode: Option<OutputEmissionMode>,
) -> Result<Vec<String>> {
    Ok(vec![render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_SYSTEM_ZH_CN,
            AI_DYNAMIC_SYSTEM_EN,
        ),
        serde_json::json!({
            "control_emission_mode": control_emission_mode,
        }),
    )?])
}

fn dynamic_hidden_sections(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
    attempt_id: &str,
    session_mode: SessionMode,
    has_output_contract: bool,
) -> Result<Vec<PromptHiddenSection>> {
    let workspace_path = dynamic_workspace(graph, &node.workspace_id)?
        .path
        .to_string();
    let dynamic_root = ctx.app.paths.dynamic_dir(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
    );
    let runtime_context = dynamic_runtime_context(ctx, &node.id, attempt_id);
    let projection = dynamic_context_projection(ctx, graph, node);
    let content = render_template(
        prompt_by_language(
            ctx.app.config.desktop_language,
            AI_DYNAMIC_HIDDEN_CONTEXT_ZH_CN,
            AI_DYNAMIC_HIDDEN_CONTEXT_EN,
        ),
        serde_json::json!({
            "outer_node_id": ctx.outer_node_id,
            "outer_attempt_id": ctx.outer_attempt_id,
            "dynamic_run_id": graph.run.id,
            "node_id": node.id,
            "title": node.title,
            "kind": format!("{:?}", node.kind),
            "group_id": node.group_id.as_deref().unwrap_or("none"),
            "chain_id": node.chain_id,
            "depth": node.depth,
            "session_mode": match session_mode {
                SessionMode::New => "new",
                SessionMode::Continue => "continue",
            },
            "continue_from_node_id": node.continue_from_node_id.as_deref().unwrap_or("none"),
            "dynamic_root": dynamic_root,
            "node_dir": runtime_context.node_dir,
            "attempt_dir": runtime_context.attempt_dir,
            "attachments_dir": runtime_context.attachments_dir,
            "workspace_id": node.workspace_id,
            "workspace_path": workspace_path,
            "workspace_capability": dynamic_workspace_capability_summary(ctx),
            "direct_predecessors": projection.direct_predecessors,
            "has_direct_predecessors": projection.has_direct_predecessors,
            "active_group": projection.active_group,
            "has_active_group": projection.has_active_group,
            "inherited_groups": projection.inherited_groups,
            "has_inherited_groups": projection.has_inherited_groups,
            "siblings": projection.siblings,
            "has_siblings": projection.has_siblings,
            "available_attachments": projection.available_attachments,
            "has_available_attachments": projection.has_available_attachments,
            "has_output_contract": has_output_contract,
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
            "resumable_sessions": dynamic_resumable_session_summary(ctx, graph, node),
            "depends_on": if node.depends_on.is_empty() {
                "none".to_string()
            } else {
                node.depends_on.join(", ")
            },
        }),
    )?;
    Ok(vec![PromptHiddenSection {
        title: "Gold Band AI-DYNAMIC runtime context".to_string(),
        content,
    }])
}

fn dynamic_resume_task_instruction(
    session_mode: SessionMode,
    task_instruction: &str,
) -> Option<String> {
    if session_mode != SessionMode::Continue {
        return None;
    }
    let task = task_instruction.trim();
    (!task.is_empty()).then(|| task.to_string())
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

fn dynamic_worktree_short_id(ctx: &DynamicExecutionContext<'_>, workspace_id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    ctx.round_id.hash(&mut hasher);
    ctx.outer_node_id.hash(&mut hasher);
    ctx.outer_attempt_id.hash(&mut hasher);
    workspace_id.hash(&mut hasher);
    format!("dyn-{:016x}", hasher.finish())
}

fn dynamic_worktree_branch_name(ctx: &DynamicExecutionContext<'_>, workspace_id: &str) -> String {
    format!(
        "gb-dyn-{}-{}-{}",
        safe_dynamic_ref(ctx.task_id),
        safe_dynamic_ref(ctx.run_id),
        dynamic_worktree_short_id(ctx, workspace_id)
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

fn dynamic_worktree_dir(ctx: &DynamicExecutionContext<'_>, workspace_id: &str) -> Utf8PathBuf {
    dynamic_worktree_base_dir(ctx).join(dynamic_worktree_short_id(ctx, workspace_id))
}

#[cfg(test)]
fn git_output(cwd: &Utf8Path, args: &[&str]) -> Result<GitCommandOutput> {
    GitCommandRunner::default().run(cwd, args)
}

fn dynamic_workspace<'a>(
    graph: &'a DynamicGraphState,
    workspace_id: &str,
) -> Result<&'a WorkspaceState> {
    graph
        .workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
        .ok_or_else(|| anyhow!("dynamic workspace `{workspace_id}` is missing"))
}

fn dynamic_workspace_mut<'a>(
    graph: &'a mut DynamicGraphState,
    workspace_id: &str,
) -> Result<&'a mut WorkspaceState> {
    graph
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .ok_or_else(|| anyhow!("dynamic workspace `{workspace_id}` is missing"))
}

fn ensure_dynamic_workspace(
    graph: &DynamicGraphState,
    node: &DynamicNodeState,
) -> Result<Utf8PathBuf> {
    let workspace = dynamic_workspace(graph, &node.workspace_id)?;
    ensure!(
        !matches!(
            workspace.status,
            WorkspaceStatus::Released | WorkspaceStatus::Merged
        ),
        "dynamic workspace `{}` is no longer available",
        workspace.id
    );
    if workspace.kind == WorkspaceKind::Worktree {
        GitWorkspaceManager::default().validate_worktree(
            &workspace.path,
            workspace
                .branch
                .as_deref()
                .ok_or_else(|| anyhow!("runtime worktree is missing branch"))?,
        )?;
    } else {
        if workspace.ownership == WorkspaceOwnership::User && workspace.path != workspace.repo_root
        {
            GitWorkspaceManager::default()
                .ensure_worktree_registration(&workspace.repo_root, &workspace.path)?;
        }
        GitRepositoryService::default().require_worktree(&workspace.path)?;
    }
    Ok(workspace.path.clone())
}

fn dynamic_workspace_capability_summary(_ctx: &DynamicExecutionContext<'_>) -> String {
    "- workspaceAssignedByRuntime: true\n- fanoutCreatesIsolatedWorktrees: true".to_string()
}

fn checkpoint_dynamic_workspace(
    graph: &mut DynamicGraphState,
    workspace_id: &str,
    group_id: Option<&str>,
) -> Result<String> {
    let workspace = dynamic_workspace(graph, workspace_id)?.clone();
    if workspace.ownership == WorkspaceOwnership::User {
        return GitRepositoryService::default().head(&workspace.path);
    }
    let _guard = DYNAMIC_WORKTREE_GIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow!("dynamic worktree git lock poisoned"))?;
    let checkpoint =
        GitWorkspaceManager::default().checkpoint(&workspace.path, &workspace.id, group_id)?;
    let head = GitRepositoryService::default().head(&workspace.path)?;
    let state = dynamic_workspace_mut(graph, workspace_id)?;
    if checkpoint.is_some() {
        state.checkpoint_commit = checkpoint;
    }
    state.updated_at = now_rfc3339_like();
    Ok(head)
}

fn fork_dynamic_workspace(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    parent_workspace_id: &str,
    group_id: &str,
    node_id: &str,
) -> Result<String> {
    let fork_commit = checkpoint_dynamic_workspace(graph, parent_workspace_id, Some(group_id))?;
    let workspace_id = format!(
        "workspace-{}",
        dynamic_worktree_short_id(ctx, &format!("{group_id}:{node_id}"))
    );
    let path = dynamic_worktree_dir(ctx, &workspace_id);
    let branch = dynamic_worktree_branch_name(ctx, &workspace_id);
    let parent = dynamic_workspace(graph, parent_workspace_id)?.clone();
    let _guard = DYNAMIC_WORKTREE_GIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow!("dynamic worktree git lock poisoned"))?;
    if let Err(error) = GitWorkspaceManager::default().ensure_worktree(
        &parent.repo_root,
        &path,
        &branch,
        &fork_commit,
    ) {
        return Err(error)
            .with_context(|| format!("failed to fork dynamic workspace `{workspace_id}`"));
    }
    let now = now_rfc3339_like();
    let workspace = WorkspaceState {
        version: VERSION.to_string(),
        id: workspace_id.clone(),
        dynamic_run_id: graph.run.id.clone(),
        kind: WorkspaceKind::Worktree,
        ownership: WorkspaceOwnership::Runtime,
        repo_root: parent.repo_root,
        path,
        branch: Some(branch),
        parent_workspace_id: Some(parent_workspace_id.to_string()),
        created_by_group_id: Some(group_id.to_string()),
        fork_commit,
        checkpoint_commit: None,
        status: WorkspaceStatus::Active,
        created_at: now.clone(),
        updated_at: now,
    };
    validate_workspace_state(&workspace)?;
    graph.workspaces.push(workspace);
    Ok(workspace_id)
}

fn release_dynamic_workspace_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    workspace_id: &str,
) -> bool {
    let Ok(workspace) = dynamic_workspace(graph, workspace_id).cloned() else {
        return false;
    };
    if workspace.ownership != WorkspaceOwnership::Runtime
        || workspace.status == WorkspaceStatus::Released
    {
        return false;
    }
    let Some(branch) = workspace.branch.as_deref() else {
        return false;
    };
    let Ok(_guard) = DYNAMIC_WORKTREE_GIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
    else {
        return false;
    };
    if GitWorkspaceManager::default()
        .remove_worktree(&workspace.repo_root, &workspace.path, branch)
        .is_ok()
        && let Ok(state) = dynamic_workspace_mut(graph, workspace_id)
    {
        state.status = WorkspaceStatus::Released;
        state.updated_at = now_rfc3339_like();
        dynamic_event_best_effort(
            ctx,
            "dynamic_workspace_released",
            serde_json::json!({ "workspaceId": workspace_id }),
        );
        return true;
    }
    false
}

fn reconcile_closed_dynamic_group_workspaces(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> bool {
    let mut workspace_ids = graph
        .groups
        .iter()
        .filter(|group| group.status == DynamicGroupStatus::Closed)
        .flat_map(|group| group.child_workspace_ids.iter().cloned())
        .collect::<Vec<_>>();
    workspace_ids.sort_unstable();
    workspace_ids.dedup();

    workspace_ids
        .into_iter()
        .fold(false, |changed, workspace_id| {
            release_dynamic_workspace_best_effort(ctx, graph, &workspace_id) || changed
        })
}

fn validate_dynamic_workspace_catalog(graph: &DynamicGraphState) -> Result<()> {
    validate_workspace_topology(graph)?;
    for workspace in &graph.workspaces {
        validate_workspace_state(workspace)?;
        if workspace.ownership == WorkspaceOwnership::Runtime
            && workspace.status != WorkspaceStatus::Released
        {
            GitWorkspaceManager::default().validate_worktree(
                &workspace.path,
                workspace
                    .branch
                    .as_deref()
                    .ok_or_else(|| anyhow!("runtime workspace branch is missing"))?,
            )?;
        }
    }
    Ok(())
}

fn persist_dynamic_graph(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> Result<()> {
    validate_dynamic_graph(graph)?;
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
    for workspace in &graph.workspaces {
        write_json(
            &ctx.app.paths.dynamic_workspace_file(
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                &workspace.id,
            ),
            workspace,
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

fn emit_dynamic_workspace_transition_updates_best_effort(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
    node_ids: &[String],
) {
    emit_dynamic_session_updates_best_effort(ctx, graph, node_ids);
}

fn persist_dynamic_workspace_phase(
    ctx: &DynamicExecutionContext<'_>,
    graph: &DynamicGraphState,
) -> Result<()> {
    validate_dynamic_run_state(&graph.run)?;
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
        &ctx.app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ),
        graph,
    )
}

/// Workspace topology and its Git representation form one consistency boundary.
/// Keep the dynamic state lock held by the scheduler while this closure runs;
/// stop requests may update the outer run first, then wait for this boundary to
/// finish before pausing dynamic descendants.
fn with_dynamic_workspace_transition<T>(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    owner_node_ids: &[String],
    operation: impl FnOnce(&mut DynamicGraphState) -> Result<T>,
) -> Result<T> {
    ensure!(
        graph.run.status == DynamicRunStatus::Running,
        "workspace transition requires a running dynamic run"
    );
    ensure!(
        graph.run.phase == DynamicRunPhase::Executing,
        "dynamic workspace transition is already active"
    );

    let transition_started_at = now_rfc3339_like();
    graph.run.phase = DynamicRunPhase::PreparingWorkspace;
    graph.run.updated_at = transition_started_at.clone();
    let transition_owners = begin_dynamic_workspace_transition_ownership(
        graph,
        owner_node_ids,
        &transition_started_at,
    )?;
    let transition_entry_node_ids = transition_owners
        .iter()
        .map(|(node_id, _)| node_id.clone())
        .collect::<Vec<_>>();
    if let Err(error) = persist_dynamic_workspace_phase(ctx, graph) {
        let transition_failed_at = now_rfc3339_like();
        graph.run.phase = DynamicRunPhase::Executing;
        graph.run.updated_at = transition_failed_at.clone();
        let _ = finish_dynamic_workspace_transition_ownership(
            graph,
            &transition_owners,
            &transition_failed_at,
        );
        let _ = persist_dynamic_workspace_phase(ctx, graph);
        return Err(error.context("failed to persist dynamic workspace transition start"));
    }
    if let Err(error) = ctx.app.transition_runtime_execution_phase(
        ctx.task_id,
        ctx.run_id,
        ctx.round_id,
        ctx.outer_node_id,
        ctx.outer_attempt_id,
        RuntimeExecutionPhase::PreparingWorkspace,
    ) {
        let transition_failed_at = now_rfc3339_like();
        graph.run.phase = DynamicRunPhase::Executing;
        graph.run.updated_at = transition_failed_at.clone();
        finish_dynamic_workspace_transition_ownership(
            graph,
            &transition_owners,
            &transition_failed_at,
        )?;
        persist_dynamic_graph(ctx, graph)?;
        return Err(error.context("failed to project dynamic workspace transition start"));
    }
    dynamic_event_best_effort(
        ctx,
        "dynamic_workspace_transition_started",
        serde_json::json!({ "dynamicRunId": graph.run.id }),
    );
    emit_dynamic_workspace_transition_updates_best_effort(ctx, graph, &transition_entry_node_ids);

    let operation_result = operation(graph);
    let transition_finished_at = now_rfc3339_like();
    graph.run.phase = DynamicRunPhase::Executing;
    graph.run.updated_at = transition_finished_at.clone();
    let transition_exit_node_ids = finish_dynamic_workspace_transition_ownership(
        graph,
        &transition_owners,
        &transition_finished_at,
    )?;
    let persist_result = persist_dynamic_graph(ctx, graph);
    let phase_restore_result = if persist_result.is_err() {
        persist_dynamic_workspace_phase(ctx, graph)
    } else {
        Ok(())
    };
    if phase_restore_result.is_ok() {
        let _ = ctx.app.transition_runtime_execution_phase(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            RuntimeExecutionPhase::RunningNode,
        );
    }
    if phase_restore_result.is_ok() {
        dynamic_event_best_effort(
            ctx,
            "dynamic_workspace_transition_finished",
            serde_json::json!({
                "dynamicRunId": graph.run.id,
                "succeeded": operation_result.is_ok(),
            }),
        );
        emit_dynamic_workspace_transition_updates_best_effort(
            ctx,
            graph,
            &transition_exit_node_ids,
        );
    }

    match (operation_result, persist_result, phase_restore_result) {
        (Ok(value), Ok(()), Ok(())) => Ok(value),
        (Err(error), Ok(()), Ok(())) => Err(error),
        (Ok(_), Err(error), Ok(())) => {
            Err(error.context("failed to persist dynamic workspace transition completion"))
        }
        (Err(operation_error), Err(persist_error), Ok(())) => Err(anyhow!(
            "dynamic workspace transition failed: {operation_error:#}; full graph persistence also failed: {persist_error:#}"
        )),
        (Ok(_), Err(persist_error), Err(phase_error)) => Err(anyhow!(
            "failed to persist dynamic workspace transition completion: {persist_error:#}; phase restoration also failed: {phase_error:#}"
        )),
        (Err(operation_error), Err(persist_error), Err(phase_error)) => Err(anyhow!(
            "dynamic workspace transition failed: {operation_error:#}; full graph persistence failed: {persist_error:#}; phase restoration also failed: {phase_error:#}"
        )),
        (_, Ok(()), Err(_)) => unreachable!("phase restoration is skipped after full persistence"),
    }
}

fn dynamic_graph_has_paused_leaf(graph: &DynamicGraphState) -> bool {
    graph
        .nodes
        .iter()
        .any(|node| node.status == DynamicNodeStatus::Paused && node.outcome.is_none())
}

fn ensure_dynamic_required_model_catalogs(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
) -> Result<()> {
    for provider in dynamic_available_provider_ids(ctx) {
        if dynamic_provider_requires_proposal_model_catalog(ctx, &provider)
            && provider_model_option_values(ctx, &provider).is_empty()
        {
            let error = dynamic_catalog_missing_error(
                "dynamic.provider",
                "AI-DYNAMIC provider",
                &provider,
                serde_json::json!({ "provider": provider.clone() }),
                None,
            );
            let reason = error.message.clone();
            pause_dynamic_graph(ctx, graph, PauseReason::ErrorBlocked, &reason)?;
            return Err(runtime_error(manual_runtime_error_info(
                RuntimeErrorDomain::Config,
                "config.catalog-missing",
                reason,
                serde_json::json!({ "provider": provider }),
            )));
        }
    }
    Ok(())
}

fn blocked_runtime_error(message: impl Into<String>) -> anyhow::Error {
    let message = message.into();
    runtime_error(blocked_runtime_error_info(
        RuntimeErrorDomain::Dynamic,
        "dynamic.blocked",
        message,
        serde_json::json!({}),
    ))
}

fn recoverable_runtime_error(message: impl Into<String>) -> anyhow::Error {
    let message = message.into();
    runtime_error(manual_runtime_error_info(
        RuntimeErrorDomain::RuntimeTransport,
        "runtime.recoverable",
        message,
        serde_json::json!({}),
    ))
}

fn auto_retry_delay_ms(info: &RuntimeErrorInfo, completed_retries: u32) -> Option<u64> {
    if info.recovery != RecoveryMode::Auto {
        return None;
    }
    let policy = info.retry_policy.as_ref()?;
    if completed_retries >= policy.max_attempts {
        return None;
    }
    policy
        .backoff_ms
        .get(completed_retries as usize)
        .copied()
        .or_else(|| policy.backoff_ms.last().copied())
}

fn wait_for_retry_while_active(
    delay_ms: u64,
    mut is_active: impl FnMut() -> Result<bool>,
) -> Result<bool> {
    if !is_active()? {
        return Ok(false);
    }
    let deadline = Instant::now() + Duration::from_millis(delay_ms);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return is_active();
        }
        thread::sleep((deadline - now).min(AUTO_RETRY_STOP_POLL_INTERVAL));
        if !is_active()? {
            return Ok(false);
        }
    }
}

/// One user-visible turn may recreate its provider runtime many times. Keep
/// its identity in the scheduler, where that retry lifecycle actually lives.
fn logical_prompt_id(existing: Option<String>) -> String {
    existing
        .filter(|prompt_id| !prompt_id.trim().is_empty())
        .unwrap_or_else(|| format!("runtime-turn-{}", uuid::Uuid::new_v4().simple()))
}

fn dynamic_proposal_repair_prompt_id(logical_prompt_id: &str, repair_attempt: u32) -> String {
    format!("{logical_prompt_id}-dynamic-repair-{repair_attempt}")
}

fn pause_active_dynamic_leaves(
    graph: &mut DynamicGraphState,
    pause_reason: PauseReason,
) -> Vec<String> {
    let mut paused_node_ids = Vec::new();
    for node in &mut graph.nodes {
        if dynamic_leaf_is_active(node.status) && node.outcome.is_none() {
            mark_dynamic_node_paused(node, pause_reason, None);
            paused_node_ids.push(node.id.clone());
        }
    }
    refresh_dynamic_current_leaf_ids(graph);
    paused_node_ids
}

fn pause_dynamic_graph(
    ctx: &DynamicExecutionContext<'_>,
    graph: &mut DynamicGraphState,
    pause_reason: PauseReason,
    reason: &str,
) -> Result<()> {
    let graph_owned_leaf_ids = graph
        .nodes
        .iter()
        .filter(|node| dynamic_runtime_owns_leaf_projection(graph, node))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let mut lifecycle_changed_node_ids = if matches!(
        pause_reason,
        PauseReason::ErrorBlocked | PauseReason::RuntimeAbnormal
    ) {
        pause_active_dynamic_leaves(graph, pause_reason)
    } else {
        refresh_dynamic_current_leaf_ids(graph);
        Vec::new()
    };
    let updated_at = now_rfc3339_like();
    let already_advanced = lifecycle_changed_node_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let released_graph_owner_ids = graph_owned_leaf_ids
        .into_iter()
        .filter(|node_id| !already_advanced.contains(node_id))
        .collect::<Vec<_>>();
    lifecycle_changed_node_ids.extend(advance_dynamic_leaf_runtime_lifecycle_revisions(
        graph,
        &released_graph_owner_ids,
        &updated_at,
    ));
    graph.run.status = DynamicRunStatus::Paused;
    graph.run.phase = DynamicRunPhase::Executing;
    graph.run.outcome = None;
    graph.run.pause_reason = Some(pause_reason);
    graph.run.updated_at = updated_at;
    persist_dynamic_graph(ctx, graph)?;
    let event_result = append_dynamic_event(
        ctx,
        "dynamic_run_paused",
        serde_json::json!({
            "dynamicRunId": graph.run.id,
            "pauseReason": pause_reason,
            "reason": reason,
        }),
    );
    emit_dynamic_session_updates_best_effort(ctx, graph, &lifecycle_changed_node_ids);
    event_result
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

fn commit_runtime_candidate_after_running_persist(
    runtime_candidate: &mut Option<super::RuntimeCandidateRegistration>,
    persisted: Result<bool>,
) -> Result<bool> {
    match persisted {
        Ok(true) => {
            if let Some(runtime_candidate) = runtime_candidate.take() {
                runtime_candidate.commit();
            }
            Ok(true)
        }
        Ok(false) => {
            if let Some(runtime_candidate) = runtime_candidate.take() {
                runtime_candidate.abort()?;
            }
            Ok(false)
        }
        Err(error) => Err(error),
    }
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
    initial_prompt_display: Option<ConversationPromptInput>,
    initial_user_prompt_render_mode: UserPromptRenderMode,
    initial_resume_input_attachment_paths: Vec<String>,
    initial_runtime_control_intent: RuntimeControlIntent,
    parent_continue_input: Option<ConversationPromptInput>,
    parent_continue_prompt_id: Option<String>,
    dynamic_resume_override: Option<DynamicResumeOverride>,
    initial_model_override: Option<String>,
    initial_permission_mode_override: Option<String>,
    mut launch: Option<mpsc::Sender<RuntimeContinueLaunch>>,
    mut runtime_candidate: Option<super::RuntimeCandidateRegistration>,
) -> Result<()> {
    let mut session_mode = initial_session_mode;
    let mut continue_ref = initial_continue_ref;
    let mut resume_prompt = initial_resume_prompt;
    let mut resume_prompt_id = initial_resume_prompt_id;
    let mut prompt_display = initial_prompt_display;
    let mut resume_prompt_visibility = if matches!(
        initial_user_prompt_render_mode,
        UserPromptRenderMode::RuntimeResume
            | UserPromptRenderMode::RuntimeFinalize
            | UserPromptRenderMode::RuntimeRepair
    ) {
        PromptVisibility::Hidden
    } else {
        PromptVisibility::Visible
    };
    let mut user_prompt_render_mode = initial_user_prompt_render_mode;
    let mut resume_input_attachment_paths = initial_resume_input_attachment_paths;
    let mut runtime_control_intent = initial_runtime_control_intent;
    let mut model_override = initial_model_override;
    let mut permission_mode_override = initial_permission_mode_override;
    let mut invalid_output_repair_prompts = 0;

    loop {
        let current_attempt_id = node.attempt_id.clone();
        let current_node_id = node.node_id.clone();
        let current_execution_id = node.runtime_execution_id.clone();
        let ctx = ExecutionContext::for_run(task_id, &run.id)
            .with_round(round.id.clone())
            .with_node(current_node_id.clone())
            .with_attempt(current_attempt_id.clone());
        let previous_pause_reason = (run.status == RunStatus::Paused)
            .then_some(run.pause_reason)
            .flatten();
        run.status = RunStatus::Running;
        run.pause_reason = None;
        run.updated_at = now_rfc3339_like();
        let starting_phase = match run.execution.phase {
            RuntimeExecutionPhase::FinalizingArtifact => RuntimeExecutionPhase::FinalizingArtifact,
            RuntimeExecutionPhase::RepairingArtifact => RuntimeExecutionPhase::RepairingArtifact,
            _ => RuntimeExecutionPhase::StartingNode,
        };
        run.transition_current_execution(starting_phase, run.updated_at.clone())?;
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
        if !commit_runtime_candidate_after_running_persist(
            &mut runtime_candidate,
            persist_runtime_state_if_execution_current(app, task_id, run, round, &node),
        )? {
            return Ok(());
        }
        notify_runtime_continue_started(&mut launch, run);
        if let Some(previous_pause_reason) = previous_pause_reason {
            emit_run_metrics_fact(
                app,
                run,
                super::observability::LifecycleEventType::ExecutionResumed,
                uuid::Uuid::new_v4().to_string(),
                now_rfc3339_like(),
                Some(previous_pause_reason),
                None,
                None,
            );
        }

        // ── Observability: notify node started (skip on recovery) ──
        if previous_pause_reason.is_none() {
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
            app.emit_lifecycle_event(super::RuntimeLifecycleEvent::NodeStarted {
                project_id: app.paths.project_id.clone(),
                task_id: task_id.to_string(),
                task_uuid: run.task_uuid.clone(),
                run_id: run.id.clone(),
                run_uuid: run.uuid.clone(),
                round_id: round.id.clone(),
                round_uuid: round.uuid.clone(),
                round_index: Some(round.index),
                node_id: node.node_id.clone(),
                node_uuid: node.uuid.clone(),
                attempt_id: node.attempt_id.clone(),
                repo_root: app.paths.repo_root.to_string(),
                seq,
                node_name,
                agent_type,
                resolved_model: node
                    .resolved_config
                    .get("model")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                started_at: node.started_at.clone(),
                attempt_dir: None,
                predecessor: run.last_executed_node.clone(),
                metrics_unit_kind: None,
                child_run_id: None,
            });
        }

        let current_node_dsl = workflow
            .get_node(&current_node_id)
            .expect("validated node exists");
        if matches!(current_node_dsl, NodeDsl::Worker(_)) {
            setup_node_environment(app, task_id, &run.id, &round.id, &node, &ctx)?;
        }
        // A provider invocation may be rebuilt for an automatic retry.  The
        // user turn does not change when that happens, so allocate its
        // identity at the orchestration boundary rather than inside an ACP
        // runtime (which is deliberately short lived).
        let logical_prompt_id = logical_prompt_id(resume_prompt_id.clone());
        let mut auto_retry_attempts = 0;
        let execution_result = loop {
            if !attempt_execution_is_still_current_running(
                app,
                task_id,
                &run.id,
                &round.id,
                &current_node_id,
                &current_attempt_id,
                current_execution_id.as_deref(),
            )? {
                return Ok(());
            }
            let result = match current_node_dsl {
                NodeDsl::Worker(_) => app
                    .validate_workflow_node_agent_options(current_node_dsl)
                    .and_then(|_| {
                        execute_ai_node(
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
                            resume_prompt.clone(),
                            Some(logical_prompt_id.clone()),
                            prompt_display.clone(),
                            resume_prompt_visibility,
                            user_prompt_render_mode,
                            resume_input_attachment_paths.clone(),
                            runtime_control_intent,
                            model_override.clone(),
                            permission_mode_override.clone(),
                        )
                    }),
                NodeDsl::AiDynamic(dynamic) => execute_ai_dynamic_node(
                    app,
                    task_id,
                    run,
                    round,
                    &current_attempt_id,
                    dynamic,
                    node.clone(),
                    parent_continue_input.clone(),
                    parent_continue_prompt_id.clone(),
                    dynamic_resume_override.clone(),
                ),
            };
            match result {
                Err(err) => {
                    if !attempt_execution_is_still_current_running(
                        app,
                        task_id,
                        &run.id,
                        &round.id,
                        &current_node_id,
                        &current_attempt_id,
                        current_execution_id.as_deref(),
                    )? {
                        return Ok(());
                    }
                    let info = normalize_runtime_error(&err);
                    if let Some(delay_ms) = auto_retry_delay_ms(&info, auto_retry_attempts) {
                        if !attempt_execution_is_still_current_running(
                            app,
                            task_id,
                            &run.id,
                            &round.id,
                            &current_node_id,
                            &current_attempt_id,
                            current_execution_id.as_deref(),
                        )? {
                            return Ok(());
                        }
                        auto_retry_attempts += 1;
                        let summary = format!(
                            "auto retry {}/{} after {} at {}/{}/{}: {}",
                            auto_retry_attempts,
                            info.retry_policy
                                .as_ref()
                                .map(|policy| policy.max_attempts)
                                .unwrap_or(auto_retry_attempts),
                            info.code_str(),
                            round.id,
                            current_node_id,
                            current_attempt_id,
                            err
                        );
                        progress(&summary);
                        write_run_progress_best_effort(
                            &app.paths,
                            task_id,
                            run,
                            Some(node.node_type),
                            ProgressStage::CallingProvider,
                            summary.clone(),
                        );
                        let mut event_data = run_event_data(
                            &ctx,
                            Some(ProgressStage::CallingProvider),
                            Some(run.status),
                            Some(summary),
                            None,
                        );
                        event_data.control_failure = Some(serde_json::json!({
                            "runtimeError": info,
                            "retryAttempt": auto_retry_attempts,
                            "delayMs": delay_ms,
                        }));
                        append_run_event_best_effort(
                            &app.paths,
                            task_id,
                            &run.id,
                            "runtime_auto_retry",
                            now_rfc3339_like(),
                            event_data,
                        );
                        if !wait_for_retry_while_active(delay_ms, || {
                            attempt_execution_is_still_current_running(
                                app,
                                task_id,
                                &run.id,
                                &round.id,
                                &current_node_id,
                                &current_attempt_id,
                                current_execution_id.as_deref(),
                            )
                        })? {
                            return Ok(());
                        }
                        continue;
                    }
                    break Err(err);
                }
                ok => break ok,
            }
        };
        runtime_control_intent = RuntimeControlIntent::Unchanged;
        if !attempt_execution_is_still_current_running(
            app,
            task_id,
            &run.id,
            &round.id,
            &current_node_id,
            &current_attempt_id,
            current_execution_id.as_deref(),
        )? {
            return Ok(());
        }
        if !refresh_runtime_execution_if_current(
            app,
            task_id,
            run,
            &round.id,
            &current_node_id,
            &current_attempt_id,
            current_execution_id.as_deref(),
        )? {
            return Ok(());
        }

        node = match execution_result {
            Ok(node) => node,
            Err(err) => {
                let info = normalize_runtime_error(&err);
                let pause_reason = info.pause_reason_after_retry_boundary();
                let progress_stage = info.progress_stage_after_retry_boundary();
                let error_summary = match info.recovery {
                    RecoveryMode::Auto | RecoveryMode::Manual => format!(
                        "run {} paused with runtime abnormal at {}/{}/{}: {}",
                        run.id, round.id, current_node_id, current_attempt_id, err
                    ),
                    RecoveryMode::Blocked => format!(
                        "run {} blocked at {}/{}/{}: {}",
                        run.id, round.id, current_node_id, current_attempt_id, err
                    ),
                };
                progress(&error_summary);
                run.status = RunStatus::Paused;
                run.pause_reason = Some(pause_reason);
                run.updated_at = now_rfc3339_like();
                let active_locator = run.execution.locator.clone();
                run.transition_execution(
                    RuntimeExecutionPhase::Paused,
                    active_locator,
                    run.updated_at.clone(),
                )?;
                round.status = RunStatus::Paused;
                let mut failed_node = node;
                failed_node.status = RunStatus::Paused;
                failed_node.outcome = None;
                failed_node.finished_at = Some(run.updated_at.clone());
                let expected_execution_id = failed_node.runtime_execution_id.take();
                write_run_progress_best_effort(
                    &app.paths,
                    task_id,
                    run,
                    Some(failed_node.node_type),
                    progress_stage,
                    error_summary.clone(),
                );
                let mut event_data = run_event_data(
                    &ctx,
                    Some(progress_stage),
                    Some(run.status),
                    Some(error_summary.clone()),
                    run.pause_reason,
                );
                event_data.control_failure = Some(serde_json::json!({
                    "runtimeError": info,
                }));
                append_run_event_best_effort(
                    &app.paths,
                    task_id,
                    &run.id,
                    "run_paused",
                    run.updated_at.clone(),
                    event_data,
                );
                teardown_node_environment_best_effort(
                    app,
                    task_id,
                    &run.id,
                    &round.id,
                    &failed_node,
                    &ctx,
                );
                if !persist_runtime_state_if_expected_execution_current(
                    app,
                    task_id,
                    run,
                    round,
                    &failed_node,
                    expected_execution_id.as_deref(),
                )? {
                    return Ok(());
                }
                emit_pause_side_effects(app, task_id, run, round, &failed_node);
                app.finish_runtime_candidate_best_effort(
                    task_id,
                    &run.id,
                    run.execution.recovery_candidate_token.as_deref(),
                );
                return Ok(());
            }
        };

        if node.status == RunStatus::Completed {
            teardown_node_environment_best_effort(app, task_id, &run.id, &round.id, &node, &ctx);

            // Emit NodeCompleted immediately when the node reaches terminal execution state.
            // This must happen BEFORE any control flow decisions (manual check, invalid
            // repair, transition, run completion) so metrics are reported for every node.
            let node_attempt_dir = app
                .paths
                .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)
                .to_string();
            let node_completed_snapshot =
                completed_node_snapshot(round, &node, Some(node_attempt_dir.clone()));
            app.emit_lifecycle_event(super::RuntimeLifecycleEvent::NodeCompleted {
                task_id: task_id.to_string(),
                task_uuid: run.task_uuid.clone(),
                run_id: run.id.clone(),
                run_uuid: run.uuid.clone(),
                round_id: round.id.clone(),
                round_uuid: round.uuid.clone(),
                round_index: Some(round.index),
                node_id: node.node_id.clone(),
                node_uuid: node.uuid.clone(),
                attempt_id: node.attempt_id.clone(),
                repo_root: app.paths.repo_root.to_string(),
                seq: node_completed_snapshot.seq,
                node_name: node_completed_snapshot.node_name.clone(),
                agent_type: node_completed_snapshot.agent_type.clone(),
                resolved_model: node
                    .resolved_config
                    .get("model")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                started_at: node.started_at.clone(),
                finished_at: node.finished_at.clone(),
                outcome: node_completed_snapshot.status.clone(),
                attempt_dir: node_attempt_dir,
                suppress_sentinel: false,
                metrics_unit_kind: None,
                child_run_id: None,
            });
        }

        if node.status == RunStatus::Paused {
            let pause_reason = if node.node_type == crate::domain::NodeType::AiDynamic {
                let graph = load_dynamic_graph(
                    &app.paths.dynamic_graph_file(
                        task_id,
                        &run.id,
                        &round.id,
                        &node.node_id,
                        &node.attempt_id,
                    ),
                    &app.paths.repo_root,
                )?;
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
            // Provider and AI-DYNAMIC callbacks may have advanced the
            // authoritative execution locator to an inner leaf. Pausing the
            // outer aggregate changes its phase, not the identity of the
            // execution whose result is being committed.
            let active_locator = run.execution.locator.clone();
            run.transition_execution(
                RuntimeExecutionPhase::Paused,
                active_locator,
                run.updated_at.clone(),
            )?;
            round.status = RunStatus::Paused;
            round.outcome = None;
            let expected_execution_id = node.runtime_execution_id.take();
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
            if !persist_runtime_state_if_expected_execution_current(
                app,
                task_id,
                run,
                round,
                &node,
                expected_execution_id.as_deref(),
            )? {
                return Ok(());
            }
            emit_pause_side_effects(app, task_id, run, round, &node);
            app.finish_runtime_candidate_best_effort(
                task_id,
                &run.id,
                run.execution.recovery_candidate_token.as_deref(),
            );
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
                            Some(ProgressStage::Paused),
                            Some(RunStatus::Paused),
                            Some(format!(
                                "invalid output repair exhausted at {}/{}/{}",
                                round.id, node.node_id, node.attempt_id
                            )),
                            Some(PauseReason::RuntimeAbnormal),
                        ),
                    );
                    node.status = RunStatus::Paused;
                    node.outcome = None;
                    node.finished_at = Some(now_rfc3339_like());
                    let expected_execution_id = node.runtime_execution_id.take();
                    apply_control_decision(
                        app,
                        task_id,
                        workflow,
                        resolved_profiles,
                        run,
                        round,
                        &node,
                        ControlDecision::PauseRun(PauseReason::RuntimeAbnormal),
                        expected_execution_id.as_deref(),
                    )?;
                    app.finish_runtime_candidate_best_effort(
                        task_id,
                        &run.id,
                        run.execution.recovery_candidate_token.as_deref(),
                    );
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
                        node.runtime_execution_id.as_deref(),
                    )?;
                    app.finish_runtime_candidate_best_effort(
                        task_id,
                        &run.id,
                        run.execution.recovery_candidate_token.as_deref(),
                    );
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
                clear_invalid_output_artifact_for_repair(app, task_id, &run.id, &round.id, &node)?;
                node.status = RunStatus::Running;
                node.outcome = None;
                node.finished_at = None;
                run.status = RunStatus::Running;
                run.pause_reason = None;
                run.updated_at = now_rfc3339_like();
                run.transition_current_execution(
                    RuntimeExecutionPhase::RepairingArtifact,
                    run.updated_at.clone(),
                )?;
                round.status = RunStatus::Running;
                if !persist_runtime_state_if_execution_current(app, task_id, run, round, &node)? {
                    return Ok(());
                }
                session_mode = SessionMode::Continue;
                continue_ref = Some(repair_continue_ref);
                resume_prompt = Some(invalid_output_repair_prompt(schema));
                resume_prompt_id = None;
                prompt_display = None;
                resume_prompt_visibility = PromptVisibility::Hidden;
                user_prompt_render_mode = UserPromptRenderMode::RuntimeRepair;
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
            run.transition_current_execution(
                RuntimeExecutionPhase::AwaitingManualCheck,
                run.updated_at.clone(),
            )?;
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
            if !persist_runtime_state_if_execution_current(app, task_id, run, round, &node)? {
                return Ok(());
            }
            emit_pause_side_effects(app, task_id, run, round, &node);
            app.finish_runtime_candidate_best_effort(
                task_id,
                &run.id,
                run.execution.recovery_candidate_token.as_deref(),
            );
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
        if !persist_runtime_state_if_execution_current(app, task_id, run, round, &node)? {
            return Ok(());
        }

        // NodeCompleted already emitted above inside the if-completed block.
        let completed_snapshot = completed_node_snapshot(
            round,
            &node,
            Some(
                app.paths
                    .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)
                    .to_string(),
            ),
        );
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
            node.runtime_execution_id.as_deref(),
        )? {
            run.last_executed_node = Some(completed_snapshot);
            node = next.node;
            let prompt_state = AcpInvocationPromptState::workflow_transition(
                app.config.desktop_language,
                next.session_mode,
                next.continue_ref,
            );
            session_mode = prompt_state.session_mode;
            continue_ref = prompt_state.continue_ref;
            resume_prompt = prompt_state.resume_prompt;
            resume_prompt_id = prompt_state.resume_prompt_id;
            prompt_display = prompt_state.prompt_display;
            resume_prompt_visibility = prompt_state.resume_prompt_visibility;
            resume_input_attachment_paths = prompt_state.input_attachment_paths;
            user_prompt_render_mode = prompt_state.user_prompt_render_mode;
            runtime_control_intent = prompt_state.runtime_control_intent;
            model_override = prompt_state.model_override;
            permission_mode_override = prompt_state.permission_mode_override;
            invalid_output_repair_prompts = 0;
            continue;
        }
        // Workflow ended - NodeCompleted already emitted above before control decision.
        if run.status != RunStatus::Running {
            app.finish_runtime_candidate_best_effort(
                task_id,
                &run.id,
                run.execution.recovery_candidate_token.as_deref(),
            );
        }
        run.last_executed_node = Some(completed_snapshot);
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::elicitation::{pending_elicitation_state, write_pending_elicitation};
    use crate::config::{ProviderDiagnosticSnapshot, RuntimeConfig};
    use crate::dsl::{AiDynamicAgentStrategy, DynamicControlDsl};
    use crate::provider::{
        DoctorResult, OutputArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
        ProviderResultPayload, SessionRef,
    };
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    struct DynamicRepairIdentityProvider {
        invocations: Arc<Mutex<Vec<(String, SessionMode, UserPromptRenderMode)>>>,
    }

    impl ProviderAdapter for DynamicRepairIdentityProvider {
        fn describe_provider(&self) -> ProviderInfo {
            ProviderInfo {
                provider_id: "claude-acp".to_string(),
                display_name: "Dynamic repair identity test".to_string(),
                capabilities: ProviderCapabilities {
                    supports_open_session: true,
                    supports_continue_session: true,
                    supports_system_prompt: false,
                    supports_raw_stream: false,
                },
                is_default: true,
            }
        }

        fn doctor(&self) -> DoctorResult {
            DoctorResult {
                available: true,
                reason: None,
                capabilities: None,
            }
        }

        fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult> {
            let prompt_id = req
                .resume_prompt_id
                .clone()
                .expect("dynamic invocation must carry a prompt identity");
            let mut invocations = self.invocations.lock().unwrap();
            let invocation_index = invocations.len();
            invocations.push((prompt_id, req.session_mode, req.user_prompt_render_mode));
            drop(invocations);

            let output_artifact = match invocation_index {
                0 => None,
                1 => Some(OutputArtifactPayload {
                    name: DYNAMIC_COMPLETION_ARTIFACT.to_string(),
                    content: test_end_completion("repaired completion"),
                }),
                _ => panic!("dynamic repair should finish after the second provider turn"),
            };
            Ok(ProviderRunResult {
                status: ProviderRunStatus::Success,
                exit_code: None,
                result_payload: output_artifact.map(|output_artifact| ProviderResultPayload {
                    output_artifact: Some(output_artifact),
                }),
                worker_ref_seed: Some(SessionRef {
                    provider: "claude-acp".to_string(),
                    mode: req.session_mode,
                    supports_open_session: true,
                    supports_continue_session: true,
                    continue_ref: Some(serde_json::json!({
                        "acpSessionId": "dynamic-repair-session"
                    })),
                    open_command: None,
                }),
                stream_path: None,
                runtime_error: None,
            })
        }

        fn open_session(&self, _worker_ref: &SessionRef) -> Result<()> {
            Ok(())
        }

        fn build_continue_command(&self, _worker_ref: &SessionRef) -> Result<Option<String>> {
            Ok(None)
        }
    }

    #[test]
    fn runtime_candidate_commits_only_after_canonical_running_persist() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let paths = crate::storage::GoldBandPaths::new(repo_root);
        let core_db = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        let coordinator = crate::app::RuntimeRecoveryCoordinator::new(core_db);
        coordinator
            .complete_startup_recovery(std::collections::HashSet::new())
            .unwrap();
        let mut registration = Some(coordinator.begin(&paths, "task-001", "run-001").unwrap());

        assert!(
            !commit_runtime_candidate_after_running_persist(&mut registration, Ok(false)).unwrap()
        );
        assert!(registration.is_none());
        assert!(coordinator.list_persisted_candidates().unwrap().is_empty());
        assert!(coordinator.begin_shutdown().unwrap().is_empty());
    }

    #[test]
    fn metrics_resume_cause_is_consumed_without_pause_reason_inference() {
        let mut state = crate::app::observability::ExecutionObservabilityState::default();
        state.set_pending_resume_cause(crate::app::observability::ResumeCause::PermissionResolved);
        let cause = state.take_pending_resume_cause().unwrap();
        state.record_resume(true, cause);
        assert_eq!(state.counters.resume_count, 1);
        assert_eq!(state.counters.manual_continue_count, 0);
        state.set_pending_resume_cause(crate::app::observability::ResumeCause::ManualContinue);
        let cause = state.take_pending_resume_cause().unwrap();
        state.record_resume(true, cause);
        assert_eq!(state.counters.resume_count, 2);
        assert_eq!(state.counters.manual_continue_count, 1);
    }

    #[test]
    fn run_intermediate_metrics_carry_current_workflow_node_context() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let bus = crate::app::observability::RuntimeLifecycleBus::new();
        let app = App::new(repo_root)
            .with_metrics_collection_enabled(true)
            .with_lifecycle_bus(bus.clone());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_handler = seen.clone();
        bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::MetricsFact(fact) = event {
                seen_for_handler.lock().unwrap().push(fact);
            }
        }));
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let started_at = "2026-08-11T00:00:00Z".to_string();
        let task_uuid = generate_uuid();
        let run_uuid = generate_uuid();
        let node_uuid = generate_uuid();
        let run = RunState {
            version: crate::domain::VERSION.into(),
            id: run_id.into(),
            task_id: task_id.into(),
            task_uuid: Some(task_uuid),
            status: RunStatus::Paused,
            outcome: None,
            started_at: started_at.clone(),
            updated_at: started_at.clone(),
            workflow_snapshot: "workflow.snapshot.json".into(),
            current_round: Some(round_id.into()),
            current_node: Some("plan".into()),
            current_attempt: Some("attempt-001".into()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: Some(run_uuid.clone()),
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let round = RoundState {
            version: crate::domain::VERSION.into(),
            id: round_id.into(),
            run_id: run_id.into(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: started_at.clone(),
            trace: Vec::new(),
            uuid: None,
        };
        let mut node = NodeState {
            version: crate::domain::VERSION.into(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "plan".into(),
            node_type: crate::domain::NodeType::Worker,
            run_id: run_id.into(),
            round_id: round_id.into(),
            attempt_id: "attempt-001".into(),
            status: RunStatus::Running,
            outcome: None,
            started_at: started_at.clone(),
            finished_at: None,
            manual_check_pending: false,
            runtime_execution_id: None,
            resolved_config: Default::default(),
            uuid: Some(node_uuid.clone()),
        };
        node.resolved_config
            .insert("profileName".into(), serde_json::json!("Planner"));
        write_json(&app.paths.run_file(task_id, run_id), &run).unwrap();
        write_json(&app.paths.round_file(task_id, run_id, round_id), &round).unwrap();
        write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, "plan", "attempt-001"),
            &node,
        )
        .unwrap();

        emit_run_metrics_fact(
            &app,
            &run,
            crate::app::observability::LifecycleEventType::ExecutionPaused,
            "paused-1".into(),
            now_rfc3339_like(),
            Some(PauseReason::WaitingForUserInput),
            None,
            None,
        );
        emit_run_metrics_fact(
            &app,
            &run,
            crate::app::observability::LifecycleEventType::ExecutionResumed,
            "resumed-1".into(),
            now_rfc3339_like(),
            Some(PauseReason::WaitingForUserInput),
            None,
            None,
        );

        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), 2);
        for fact in facts.iter() {
            assert_eq!(fact.round_index, Some(1));
            assert_eq!(fact.attempt_index, Some(1));
            assert_eq!(fact.attempt_id.as_deref(), Some(node_uuid.as_str()));
            assert_eq!(fact.role_name.as_deref(), Some("Planner"));
            assert_eq!(
                fact.node_id.as_deref(),
                crate::app::observability::derive_execution_id(&run_uuid, "round:1:node:plan")
                    .as_deref()
            );
        }
        assert_eq!(
            facts[1].previous_pause_reason,
            Some(crate::app::observability::MetricsPauseReason::WaitingForUserInput)
        );
    }

    #[test]
    fn auto_metrics_selects_latest_failed_unit_and_classifies_acceptance() {
        fn node(
            id: &str,
            kind: crate::dynamic::DynamicNodeKind,
            outcome: NodeOutcome,
            finished_at: &str,
        ) -> crate::dynamic::DynamicNodeState {
            crate::dynamic::DynamicNodeState {
                version: crate::domain::VERSION.into(),
                acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                id: id.into(),
                dynamic_run_id: "dynamic-001".into(),
                kind,
                title: id.into(),
                task: id.into(),
                status: DynamicNodeStatus::Completed,
                outcome: Some(outcome),
                pause_reason: None,
                runtime_error: None,
                group_id: None,
                chain_id: "chain-001".into(),
                depth: 0,
                depends_on: Vec::new(),
                runtime_execution_id: None,
                runtime_execution_phase: None,
                runtime_lifecycle_revision: 0,
                runtime_lifecycle_updated_at: None,
                workspace_id: "workspace-main".into(),
                provider: None,
                profile: None,
                permission_mode: None,
                model: None,
                session_mode: SessionMode::New,
                continue_from_node_id: None,
                workflow_id: None,
                workflow_snapshot_id: None,
                child_run_id: None,
                started_at: Some("2026-08-01T00:00:00Z".into()),
                finished_at: Some(finished_at.into()),
                uuid: Some(format!("uuid-{id}")),
            }
        }
        let graph = DynamicGraphState {
            version: crate::domain::VERSION.into(),
            run: DynamicRunState {
                version: crate::domain::VERSION.into(),
                id: "dynamic-001".into(),
                parent_run_id: "run-001".into(),
                parent_round_id: "round-001".into(),
                parent_node_id: "auto".into(),
                parent_attempt_id: "attempt-001".into(),
                status: DynamicRunStatus::Completed,
                phase: crate::dynamic::DynamicRunPhase::Executing,
                outcome: Some(RunOutcome::Failure),
                pause_reason: None,
                started_at: "2026-08-01T00:00:00Z".into(),
                updated_at: "2026-08-01T00:00:02Z".into(),
                control: crate::dsl::DynamicControlDsl::default(),
                allowed_workflow_snapshots: Vec::new(),
                current_node_ids: Vec::new(),
            },
            nodes: vec![
                node(
                    "worker",
                    crate::dynamic::DynamicNodeKind::Worker,
                    NodeOutcome::Failure,
                    "2026-08-01T00:00:01Z",
                ),
                node(
                    "acceptance",
                    crate::dynamic::DynamicNodeKind::Acceptance,
                    NodeOutcome::Failure,
                    "2026-08-01T00:00:02Z",
                ),
            ],
            groups: Vec::new(),
            workspaces: Vec::new(),
            proposals: Vec::new(),
        };
        let failed = failed_dynamic_metrics_unit(&graph).unwrap();
        assert_eq!(failed.uuid.as_deref(), Some("uuid-acceptance"));
        assert_eq!(
            dynamic_metrics_terminal_reason(failed),
            crate::app::observability::TerminalReason::AcceptanceRejected
        );
    }

    #[test]
    fn logical_prompt_id_preserves_the_same_turn_across_runtime_retries() {
        let original = logical_prompt_id(None);
        assert!(original.starts_with("runtime-turn-"));
        assert_eq!(logical_prompt_id(Some(original.clone())), original);
        assert_eq!(
            logical_prompt_id(Some("user-turn-001".to_string())),
            "user-turn-001"
        );
    }

    #[test]
    fn dynamic_repair_prompt_identity_is_distinct_and_stable_per_attempt() {
        let business = "runtime-turn-business";

        assert_eq!(
            dynamic_proposal_repair_prompt_id(business, 1),
            "runtime-turn-business-dynamic-repair-1"
        );
        assert_eq!(
            dynamic_proposal_repair_prompt_id(business, 1),
            dynamic_proposal_repair_prompt_id(business, 1)
        );
        assert_ne!(
            dynamic_proposal_repair_prompt_id(business, 1),
            dynamic_proposal_repair_prompt_id(business, 2)
        );
    }

    #[test]
    fn workflow_session_continue_does_not_claim_runtime_control_resume() {
        let state = AcpInvocationPromptState::workflow_transition(
            DesktopLanguage::ZhCn,
            SessionMode::Continue,
            Some(serde_json::json!({ "acpSessionId": "session-1" })),
        );

        assert_eq!(state.session_mode, SessionMode::Continue);
        assert_eq!(
            state.user_prompt_render_mode,
            UserPromptRenderMode::WorkflowResume
        );
        assert_eq!(
            state.runtime_control_intent,
            RuntimeControlIntent::Unchanged
        );
        assert_eq!(state.resume_prompt_visibility, PromptVisibility::Visible);
        assert!(
            !state
                .resume_prompt
                .as_deref()
                .unwrap_or_default()
                .contains("用户已选择将当前节点重新交由 Runtime 控制")
        );
    }

    #[test]
    fn explicit_runtime_continue_owns_hidden_resume_prompt_and_control_intent() {
        let state = AcpInvocationPromptState::runtime_control_resume(
            DesktopLanguage::ZhCn,
            Some(serde_json::json!({ "acpSessionId": "session-1" })),
        );

        assert_eq!(state.session_mode, SessionMode::Continue);
        assert_eq!(
            state.user_prompt_render_mode,
            UserPromptRenderMode::RuntimeResume
        );
        assert_eq!(state.runtime_control_intent, RuntimeControlIntent::Resume);
        assert_eq!(state.resume_prompt_visibility, PromptVisibility::Hidden);
        assert_eq!(
            state.resume_prompt.as_deref(),
            Some("用户已选择将当前节点重新交由 Runtime 控制。当前输出契约（如有）重新生效。")
        );
    }

    #[test]
    fn runtime_continue_with_message_separates_visible_input_from_hidden_control_prompt() {
        let input = ConversationPromptInput {
            display_text: "  请先补充回归测试  ".to_string(),
            quotes: Vec::new(),
        };
        let state = runtime_control_resume_prompt_state(
            DesktopLanguage::ZhCn,
            serde_json::json!({ "acpSessionId": "session-1" }),
            Some(OutputEmissionMode::PostTurnProjection),
            Some(input),
            Some("prompt-1".to_string()),
            Vec::new(),
            None,
            None,
        );

        assert_eq!(
            state
                .prompt_display
                .as_ref()
                .map(|value| value.display_text.as_str()),
            Some("请先补充回归测试")
        );
        assert_eq!(
            state.user_prompt_render_mode,
            UserPromptRenderMode::UserMessage
        );
        assert_eq!(state.runtime_control_intent, RuntimeControlIntent::Resume);
        assert_eq!(state.resume_prompt_visibility, PromptVisibility::Visible);

        let prompt = state.resume_prompt.as_deref().unwrap_or_default();
        assert_eq!(prompt.lines().next(), Some("请先补充回归测试"));
        assert!(prompt.contains("show=\"false\""));
        assert!(prompt.contains("请先完整执行本消息中的用户指令，然后继续完成你之前的任务"));
        assert!(prompt.contains("本 turn 不适用此前的 artifact 输出约束"));
        assert!(prompt.contains("完成后再由 Runtime 在后续独立 turn 中完成结果归一化"));
        assert!(!prompt.contains("按当前输出契约输出 artifact"));
        assert!(!prompt.contains("当前输出契约（如有）重新生效"));

        let english_state = runtime_control_resume_prompt_state(
            DesktopLanguage::En,
            serde_json::json!({ "acpSessionId": "session-1" }),
            Some(OutputEmissionMode::PostTurnProjection),
            Some(ConversationPromptInput {
                display_text: "Write another essay".to_string(),
                quotes: Vec::new(),
            }),
            Some("prompt-2".to_string()),
            Vec::new(),
            None,
            None,
        );
        let english_prompt = english_state.resume_prompt.as_deref().unwrap_or_default();
        assert!(english_prompt.contains("fully carry out the user instruction in this message"));
        assert!(
            english_prompt
                .contains("then continue and complete the task you were previously working on")
        );
        assert!(english_prompt.contains("artifact-output constraints do not apply"));
        assert!(english_prompt.contains("once the task is complete"));
        assert!(english_prompt.contains("in a separate subsequent turn"));

        let inline_state = runtime_control_resume_prompt_state(
            DesktopLanguage::ZhCn,
            serde_json::json!({ "acpSessionId": "session-1" }),
            Some(OutputEmissionMode::InlineControl),
            Some(ConversationPromptInput::from("继续调整方案".to_string())),
            Some("prompt-3".to_string()),
            Vec::new(),
            None,
            None,
        );
        let inline_prompt = inline_state.resume_prompt.as_deref().unwrap_or_default();
        assert!(
            inline_prompt.contains("然后继续完成你之前的任务，完成后再按当前输出契约输出 artifact")
        );
        assert!(!inline_prompt.contains("后续独立 turn"));

        let inline_english_state = runtime_control_resume_prompt_state(
            DesktopLanguage::En,
            serde_json::json!({ "acpSessionId": "session-1" }),
            Some(OutputEmissionMode::InlineControl),
            Some(ConversationPromptInput::from(
                "Continue adjusting the plan".to_string(),
            )),
            Some("prompt-4".to_string()),
            Vec::new(),
            None,
            None,
        );
        let inline_english_prompt = inline_english_state
            .resume_prompt
            .as_deref()
            .unwrap_or_default();
        assert!(
            inline_english_prompt
                .contains("then continue and complete the task you were previously working on")
        );
        assert!(inline_english_prompt.contains("only after that"));
        assert!(inline_english_prompt.contains("according to the current output contract"));
        assert!(!inline_english_prompt.contains("separate subsequent turn"));

        let no_contract_state = runtime_control_resume_prompt_state(
            DesktopLanguage::ZhCn,
            serde_json::json!({ "acpSessionId": "session-1" }),
            None,
            Some(ConversationPromptInput::from("继续普通任务".to_string())),
            Some("prompt-5".to_string()),
            Vec::new(),
            None,
            None,
        );
        let no_contract_prompt = no_contract_state
            .resume_prompt
            .as_deref()
            .unwrap_or_default();
        assert!(
            no_contract_prompt.contains("请先完整执行本消息中的用户指令，然后继续完成你之前的任务")
        );
        assert!(!no_contract_prompt.contains("artifact 输出约束"));
        assert!(!no_contract_prompt.contains("输出 artifact"));
    }

    #[test]
    fn runtime_continue_with_attachment_only_keeps_a_visible_empty_user_message() {
        let state = runtime_control_resume_prompt_state(
            DesktopLanguage::ZhCn,
            serde_json::json!({ "acpSessionId": "session-1" }),
            None,
            Some(ConversationPromptInput {
                display_text: String::new(),
                quotes: Vec::new(),
            }),
            Some("prompt-attachment-only".to_string()),
            vec!["C:/temp/context.txt".to_string()],
            None,
            None,
        );

        assert_eq!(
            state
                .prompt_display
                .as_ref()
                .map(|value| value.display_text.as_str()),
            Some("")
        );
        assert_eq!(state.input_attachment_paths, vec!["C:/temp/context.txt"]);
        assert_eq!(
            state.user_prompt_render_mode,
            UserPromptRenderMode::UserMessage
        );
        assert_eq!(state.resume_prompt_visibility, PromptVisibility::Visible);
        assert!(
            state
                .resume_prompt
                .as_deref()
                .unwrap_or_default()
                .starts_with("<hidden")
        );
    }

    #[test]
    fn fixed_runtime_continue_starting_lease_is_idempotent() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);

        let first = FixedRunContinueLease::acquire(&app, "task-001", "run-001").unwrap();
        assert!(
            FixedRunContinueLease::acquire(&app, "task-001", "run-001").is_err(),
            "a second continue cannot claim the same fixed run"
        );
        assert!(
            FixedRunContinueLease::acquire(&app, "task-001", "run-002").is_ok(),
            "different runs remain independent"
        );
        drop(first);
        assert!(FixedRunContinueLease::acquire(&app, "task-001", "run-001").is_ok());
    }

    #[test]
    fn completed_attempt_recovery_finishes_without_reinvoking_provider_and_is_idempotent() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let app = App::new(repo_root);
        let task_id = "task-recovery";
        let run_id = "run-001";
        let round_id = "round-001";
        let node_id = "dev";
        let attempt_id = "attempt-001";
        let workflow: WorkflowDsl = serde_json::from_value(serde_json::json!({
            "version": VERSION,
            "id": "recovery-workflow",
            "entry": node_id,
            "nodes": [{
                "id": node_id,
                "type": "worker",
                "provider": "claude-acp",
                "prompt_envelope": "raw-agent"
            }],
            "edges": [{ "from": node_id, "to": "$end", "on": "success" }]
        }))
        .unwrap();
        write_json(
            &app.paths.workflow_snapshot_file(task_id, run_id),
            &workflow,
        )
        .unwrap();
        let run = RunState {
            version: VERSION.to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-14T00:00:00Z".to_string(),
            updated_at: "2026-08-14T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some(round_id.to_string()),
            current_node: Some(node_id.to_string()),
            current_attempt: Some(attempt_id.to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::ProcessInterrupted),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: crate::runtime::RuntimeExecutionState::new(
                RuntimeExecutionPhase::Paused,
                Some(crate::runtime::RuntimeAttemptLocator {
                    round_id: round_id.to_string(),
                    node_id: node_id.to_string(),
                    attempt_id: attempt_id.to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "2026-08-14T00:00:01Z",
            ),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: round_id.to_string(),
            run_id: run_id.to_string(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "2026-08-14T00:00:00Z".to_string(),
            trace: vec![round_trace_step(
                1,
                node_id,
                attempt_id,
                None,
                None,
                "2026-08-14T00:00:00Z".to_string(),
            )],
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: node_id.to_string(),
            node_type: crate::domain::NodeType::Worker,
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            attempt_id: attempt_id.to_string(),
            status: RunStatus::Completed,
            outcome: Some(NodeOutcome::Success),
            started_at: "2026-08-14T00:00:00Z".to_string(),
            finished_at: Some("2026-08-14T00:00:01Z".to_string()),
            manual_check_pending: false,
            runtime_execution_id: None,
            resolved_config: Default::default(),
            uuid: None,
        };
        persist_runtime_state(&app, task_id, &run, &round, &node).unwrap();

        assert!(
            run_recover_completed_background(
                &app, task_id, run_id, round_id, node_id, attempt_id, 0,
            )
            .is_err(),
            "a stale lifecycle projection cannot claim recovery"
        );
        let recovered = run_recover_completed_background(
            &app, task_id, run_id, round_id, node_id, attempt_id, 1,
        )
        .unwrap();
        assert_eq!(recovered.status, RunStatus::Completed);
        assert_eq!(recovered.outcome, Some(RunOutcome::Success));
        assert!(
            run_recover_completed_background(
                &app, task_id, run_id, round_id, node_id, attempt_id, 1,
            )
            .is_err()
        );
    }

    #[test]
    fn automatic_retry_wait_aborts_when_attempt_is_no_longer_active() {
        let mut checks = 0;
        let active = wait_for_retry_while_active(0, || {
            checks += 1;
            Ok(checks == 1)
        })
        .unwrap();

        assert!(!active);
        assert_eq!(checks, 2);
    }

    #[test]
    fn dynamic_graph_persistence_declares_storage_schema_only_on_leaf_node() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut graph = test_dynamic_graph(vec![test_worktree_node("leaf-a")]);

        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let graph_json = read_json::<serde_json::Value>(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert!(
            graph_json["nodes"][0]
                .get("acpStorageSchemaVersion")
                .is_none()
        );
        let leaf_json = read_json::<serde_json::Value>(&app.paths.dynamic_node_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            "leaf-a",
        ))
        .unwrap();
        assert_eq!(
            leaf_json
                .get("acpStorageSchemaVersion")
                .and_then(serde_json::Value::as_u64),
            Some(u64::from(
                crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION
            ))
        );
    }

    #[test]
    fn dynamic_retry_gate_observes_a_stopped_leaf_while_parent_stays_running() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut leaf = test_worktree_node("leaf-a");
        leaf.status = DynamicNodeStatus::Running;
        leaf.started_at = Some("2026-08-06T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![leaf]);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        assert!(
            dynamic_leaf_attempt_is_still_running(&ctx, "leaf-a", "attempt-001", None).unwrap()
        );

        mark_dynamic_node_paused(&mut graph.nodes[0], PauseReason::ProcessInterrupted, None);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        assert!(
            !dynamic_leaf_attempt_is_still_running(&ctx, "leaf-a", "attempt-001", None).unwrap()
        );
    }

    fn git(cwd: &Utf8Path, args: &[&str]) {
        let output = git_output(cwd, args).expect("git command should run");
        assert!(
            output.success,
            "git {:?} failed: stdout={} stderr={}",
            args, output.stdout, output.stderr
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

    #[test]
    fn dynamic_outer_conversation_worktree_repairs_registration_before_use() {
        let (temp, repo_root) = init_repo();
        let manager = GitWorkspaceManager::default();
        let old_parent = Utf8PathBuf::from_path_buf(temp.path().join("runtime-old")).unwrap();
        let old_worktree = old_parent.join("conversation");
        manager
            .create_worktree(
                &repo_root,
                &old_worktree,
                "gold-band/test-dynamic-registration",
                "HEAD",
            )
            .unwrap();
        let new_parent = Utf8PathBuf::from_path_buf(temp.path().join("runtime-new")).unwrap();
        std::fs::rename(old_parent.as_std_path(), new_parent.as_std_path()).unwrap();
        let new_worktree = new_parent.join("conversation");
        let node = test_worktree_node("bootstrap");
        let mut graph = test_dynamic_graph_at(repo_root.clone(), vec![node.clone()]);
        graph.workspaces[0].path = new_worktree.clone();

        assert_eq!(
            ensure_dynamic_workspace(&graph, &node).unwrap(),
            new_worktree
        );
        assert_eq!(
            manager
                .ensure_worktree_registration(&repo_root, &graph.workspaces[0].path)
                .unwrap(),
            crate::git::WorktreeRegistrationStatus::AlreadyRegistered
        );
    }

    fn create_direct_test_task(app: &App) -> String {
        let workflow: WorkflowDsl = serde_json::from_value(serde_json::json!({
            "version": VERSION,
            "id": "conversation-worktree-test",
            "entry": "dev",
            "nodes": [{
                "id": "dev",
                "type": "worker",
                "provider": "claude-acp",
                "prompt_envelope": "raw-agent"
            }],
            "edges": [{ "from": "dev", "to": "$end", "on": "success" }]
        }))
        .unwrap();
        app.create_task_from_requirement(super::super::CreateTaskInput {
            title: Some("Conversation worktree".to_string()),
            description: None,
            requirement_file_name: None,
            requirement_content: "test worktree".to_string(),
            workflow,
            workflow_template_id: None,
        })
        .unwrap()
        .task
        .id
    }

    #[test]
    fn conversation_worktree_run_records_head_and_stays_stopped_after_preparation() {
        let (_temp, app) = test_app_with_provider_capabilities(serde_json::json!({}));
        let repo_root = app.paths.repo_root.clone();
        let task_id = create_direct_test_task(&app);
        let expected_head = GitRepositoryService::default().head(&repo_root).unwrap();
        let prepared = prepare_run_in_worktree(&app, &task_id, None).unwrap();
        let prepared_run = prepared.run();
        let worktree = prepared_run.worktree.as_ref().unwrap();
        assert_eq!(
            prepared_run.execution.phase,
            RuntimeExecutionPhase::PreparingWorkspace
        );
        assert_eq!(worktree.fork_commit, expected_head);
        assert!(
            worktree
                .path
                .starts_with(app.paths.conversation_worktrees_dir())
        );

        let accepted = prepared.accept();
        let PreparedRunData {
            validated,
            mut run,
            round,
            node,
            ..
        } = accepted.data;
        assert_eq!(
            node.acp_storage_schema_version,
            crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION
        );
        let attempt_dir = app.paths.attempt_dir(
            &task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        );
        let durable_node: NodeState = read_json(&attempt_dir.join("node.json")).unwrap();
        assert_eq!(
            durable_node.acp_storage_schema_version,
            crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION
        );
        assert!(
            !attempt_dir
                .join(".acp-branch-timeline-migration-v1")
                .exists()
        );
        assert!(!attempt_dir.join(".acp-agent-result-migration-v2").exists());
        app.run_pause(&task_id, &run.id, PauseReason::ProcessInterrupted)
            .unwrap();

        assert!(!prepare_run_worktree_background(&app, &task_id, &mut run, &round, &node).unwrap());
        let durable = app.run_status(&task_id, &run.id).unwrap();
        assert_eq!(durable.status, RunStatus::Paused);
        assert_eq!(durable.execution.phase, RuntimeExecutionPhase::Paused);
        let durable_worktree = durable.worktree.as_ref().unwrap();
        assert!(durable_worktree.path.is_dir());

        let invocation = crate::app::node_executor::build_worker_invocation(
            &app,
            &task_id,
            &run.id,
            &round,
            &node.attempt_id,
            &validated,
            &node.node_id,
            SessionMode::New,
            None,
            None,
            None,
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(invocation.adapter_workspace_dir, repo_root);
        assert_eq!(invocation.workspace_dir, durable_worktree.path);

        let prepared_follow_up = app
            .prepare_acp_prompt_for_attempt(
                &task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
                "follow up".to_string(),
                Some("follow-up-001".to_string()),
                Some(serde_json::json!({
                    "acpSessionId": "session-001",
                    "cwd": repo_root,
                })),
            )
            .unwrap();
        assert_eq!(prepared_follow_up.adapter_workspace_dir, repo_root);
        assert_eq!(
            prepared_follow_up.session_workspace_dir,
            durable_worktree.path
        );

        let mut tampered = durable.clone();
        tampered.worktree.as_mut().unwrap().path = repo_root;
        write_json(&app.paths.run_file(&task_id, &run.id), &tampered).unwrap();
        assert!(run_workspace_dir(&app, &task_id, &run.id).is_err());
    }

    #[test]
    fn conversation_worktree_identity_uses_durable_task_and_run_uuids() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let app = App::new(repo_root);
        let original = conversation_run_worktree_state(
            &app,
            Some("task-uuid-a"),
            "run-uuid-a",
            "head-a".to_string(),
        );
        let same = conversation_run_worktree_state(
            &app,
            Some("task-uuid-a"),
            "run-uuid-a",
            "head-b".to_string(),
        );
        let different_task = conversation_run_worktree_state(
            &app,
            Some("task-uuid-b"),
            "run-uuid-a",
            "head-a".to_string(),
        );
        let different_run = conversation_run_worktree_state(
            &app,
            Some("task-uuid-a"),
            "run-uuid-b",
            "head-a".to_string(),
        );

        assert_eq!(original.path, same.path);
        assert_eq!(original.branch, same.branch);
        assert_ne!(original.path, different_task.path);
        assert_ne!(original.branch, different_task.branch);
        assert_ne!(original.path, different_run.path);
        assert_ne!(original.branch, different_run.branch);
    }

    #[test]
    fn invalid_output_repair_cleanup_removes_output_artifact() {
        let temp = tempdir().unwrap();
        let app = App::with_config(
            Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap(),
            RuntimeConfig::default(),
        );
        let mut resolved_config = crate::domain::ResolvedConfig::new();
        resolved_config.insert(
            "outputArtifact".to_string(),
            serde_json::Value::String("accept-result".to_string()),
        );
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "accept".to_string(),
            node_type: crate::domain::NodeType::Worker,
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            attempt_id: "attempt-001".to_string(),
            status: RunStatus::Completed,
            outcome: Some(NodeOutcome::Invalid),
            started_at: "1Z".to_string(),
            finished_at: Some("2Z".to_string()),
            manual_check_pending: false,
            runtime_execution_id: None,
            resolved_config,
            uuid: None,
        };
        let artifact_dir =
            app.paths
                .artifacts_dir("task-001", "run-001", "round-001", "accept", "attempt-001");
        std::fs::create_dir_all(artifact_dir.as_std_path()).unwrap();
        let artifact_path = app.paths.artifact_file(
            "task-001",
            "run-001",
            "round-001",
            "accept",
            "attempt-001",
            "accept-result",
        );
        std::fs::write(artifact_path.as_std_path(), "not json").unwrap();

        clear_invalid_output_artifact_for_repair(&app, "task-001", "run-001", "round-001", &node)
            .unwrap();

        assert!(!artifact_path.exists());
        clear_invalid_output_artifact_for_repair(&app, "task-001", "run-001", "round-001", &node)
            .unwrap();
    }

    fn test_dynamic() -> AiDynamicNode {
        AiDynamicNode {
            id: "ai-dynamic".to_string(),
            agent_strategy: AiDynamicAgentStrategy::Fixed {
                provider: "claude-acp".to_string(),
                model: None,
                permission_mode: None,
            },
            config_options: Default::default(),
            allowed_profiles: Vec::new(),
            global_goal: None,
            control: DynamicControlDsl::default(),
            allowed_workflows: Vec::new(),
        }
    }

    #[test]
    fn dynamic_model_resolution_keeps_bootstrap_model_separate_from_available_agent_model() {
        let dynamic = AiDynamicNode {
            id: "ai-dynamic".to_string(),
            agent_strategy: AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider: "codex-acp".to_string(),
                bootstrap_model: Some("gpt-5.6-sol".to_string()),
                permission_mode: Some("agent-full-access".to_string()),
                bootstrap_config_options: BTreeMap::from([(
                    "reasoning_effort".to_string(),
                    "high".to_string(),
                )]),
                acceptance_model: Some("gpt-5.6-sol".to_string()),
                acceptance_config_options: BTreeMap::from([(
                    "reasoning_effort".to_string(),
                    "medium".to_string(),
                )]),
                routing_prompt: String::new(),
                available_agents: vec![crate::dsl::DynamicAgentRef {
                    provider: "codex-acp".to_string(),
                    model: Some("gpt-5.4".to_string()),
                    permission_mode: Some("auto".to_string()),
                    config_options: BTreeMap::from([(
                        "reasoning_effort".to_string(),
                        "low".to_string(),
                    )]),
                }],
            },
            config_options: Default::default(),
            allowed_profiles: Vec::new(),
            global_goal: None,
            control: DynamicControlDsl::default(),
            allowed_workflows: Vec::new(),
        };
        let mut bootstrap = test_worktree_node(DYNAMIC_BOOTSTRAP_NODE_ID);
        bootstrap.provider = Some("codex-acp".to_string());
        bootstrap.model = Some("gpt-5.6-sol".to_string());

        assert_eq!(
            resolve_dynamic_invocation_model(&dynamic, &bootstrap, None).as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(
            dynamic_config_options_for_invocation(&dynamic, &bootstrap)
                .get("reasoning_effort")
                .map(String::as_str),
            Some("high")
        );
        assert_eq!(dynamic_control_provider(&dynamic), "codex-acp");
        assert_eq!(
            dynamic_control_permission_mode(&dynamic).as_deref(),
            Some("agent-full-access")
        );

        let mut worker = test_worktree_node("implementation");
        worker.provider = Some("codex-acp".to_string());
        worker.model = Some("gpt-5.5".to_string());
        assert_eq!(
            resolve_dynamic_invocation_model(&dynamic, &worker, None).as_deref(),
            Some("gpt-5.4")
        );
        assert_eq!(
            dynamic_config_options_for_invocation(&dynamic, &worker)
                .get("reasoning_effort")
                .map(String::as_str),
            Some("low")
        );

        worker.kind = DynamicNodeKind::Acceptance;
        assert_eq!(
            resolve_dynamic_invocation_model(&dynamic, &worker, None).as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(
            dynamic_config_options_for_invocation(&dynamic, &worker)
                .get("reasoning_effort")
                .map(String::as_str),
            Some("medium")
        );
    }

    fn test_app_with_provider_capabilities(
        capabilities: serde_json::Value,
    ) -> (tempfile::TempDir, App) {
        let (temp, repo_root) = init_repo();
        let config = RuntimeConfig::default().with_provider_diagnostics(BTreeMap::from([(
            "claude-acp".to_string(),
            ProviderDiagnosticSnapshot {
                available: true,
                reason: None,
                checked_at: "2026-06-16T00:00:00Z".to_string(),
                capabilities: Some(capabilities),
            },
        )]));
        (temp, App::with_config(repo_root, config))
    }

    #[test]
    fn workflow_control_limit_failure_emits_run_completed_lifecycle_event() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let app = App::new(repo_root).with_inline_lifecycle_subscriber(Arc::new(move |event| {
            captured_events.lock().unwrap().push(event);
        }));
        let task_id = "task-001";
        let mut run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-07-09T00:00:00Z".to_string(),
            updated_at: "2026-07-09T00:00:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("accept".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 1,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let mut round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "2026-07-09T00:00:00Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let mut resolved_config = crate::domain::ResolvedConfig::new();
        resolved_config.insert(
            "profileName".to_string(),
            serde_json::Value::String("验收".to_string()),
        );
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "accept".to_string(),
            node_type: crate::domain::NodeType::Worker,
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            status: RunStatus::Completed,
            outcome: Some(NodeOutcome::Failure),
            started_at: "2026-07-09T00:00:00Z".to_string(),
            finished_at: Some("2026-07-09T00:00:01Z".to_string()),
            manual_check_pending: false,
            runtime_execution_id: None,
            resolved_config,
            uuid: None,
        };

        fail_workflow_control_limit(
            &app,
            task_id,
            &mut run,
            &mut round,
            &node,
            "max rounds exceeded for $new-round: 2 > 1".to_string(),
            serde_json::json!({
                "reasonKind": "max_rounds_exceeded",
                "target": "$new-round",
                "proposedCount": 2,
                "limit": 1,
                "message": "max rounds exceeded for $new-round: 2 > 1",
            }),
        )
        .unwrap();

        let persisted_run: RunState = read_json(&app.paths.run_file(task_id, &run.id)).unwrap();
        assert_eq!(persisted_run.status, RunStatus::Completed);
        assert_eq!(persisted_run.outcome, Some(RunOutcome::Failure));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            RuntimeLifecycleEvent::RunCompleted {
                task_id: emitted_task_id,
                run_id,
                round_id,
                node_id,
                attempt_id,
                node_label,
                outcome,
                ..
            } => {
                assert_eq!(emitted_task_id, task_id);
                assert_eq!(run_id, "run-001");
                assert_eq!(round_id, "round-001");
                assert_eq!(node_id, "accept");
                assert_eq!(attempt_id, "attempt-001");
                assert_eq!(node_label, "验收");
                assert_eq!(*outcome, RunOutcome::Failure);
            }
            event => panic!("expected RunCompleted event, got {event:?}"),
        }
    }

    #[test]
    fn background_drive_error_terminalizes_runtime_and_emits_intervention() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let app = App::new(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                captured_events.lock().unwrap().push(event);
            }));
        let task_id = "task-001";
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            updated_at: "2026-08-06T00:00:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("worker".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "worker".to_string(),
            node_type: crate::domain::NodeType::Worker,
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            finished_at: None,
            manual_check_pending: false,
            resolved_config: crate::domain::ResolvedConfig::new(),
            uuid: None,
            runtime_execution_id: None,
        };
        persist_runtime_state(&app, task_id, &run, &round, &node).unwrap();

        let startup_error = runtime_error(manual_runtime_error_info(
            RuntimeErrorDomain::Workspace,
            "workspace.worktree-create-failed",
            "git worktree add failed",
            serde_json::json!({ "branch": "gold-band/conversation/conflict" }),
        ));
        terminalize_background_drive_error(&app, task_id, &run, &round, &node, &startup_error);

        let persisted_run = app.run_status(task_id, &run.id).unwrap();
        let (persisted_round, persisted_node) =
            current_attempt_state(&app, task_id, &persisted_run).unwrap();
        assert_eq!(persisted_run.status, RunStatus::Paused);
        assert_eq!(
            persisted_run.pause_reason,
            Some(PauseReason::RuntimeAbnormal)
        );
        assert_eq!(persisted_run.outcome, None);
        assert_eq!(persisted_round.status, RunStatus::Paused);
        assert_eq!(persisted_round.outcome, None);
        assert_eq!(persisted_node.status, RunStatus::Paused);
        assert_eq!(persisted_node.outcome, None);
        assert!(persisted_node.finished_at.is_some());

        let run_events =
            std::fs::read_to_string(app.paths.run_events_file(task_id, &run.id)).unwrap();
        let paused_event: serde_json::Value = serde_json::from_str(
            run_events
                .lines()
                .last()
                .expect("background failure must append a canonical pause event"),
        )
        .unwrap();
        assert_eq!(paused_event["type"], "run_paused");
        assert_eq!(paused_event["timestamp"], persisted_run.updated_at);
        assert_eq!(
            paused_event["data"]["controlFailure"]["runtimeError"]["code"]["code"],
            "workspace.worktree-create-failed"
        );
        assert_eq!(
            paused_event["data"]["controlFailure"]["runtimeError"]["params"]["branch"],
            "gold-band/conversation/conflict"
        );

        let captured = events.lock().unwrap();
        assert_eq!(
            captured
                .iter()
                .filter(|event| matches!(
                    event,
                    RuntimeLifecycleEvent::InterventionRequested {
                        scheduled_occurrence_id: Some(occurrence_id),
                        kind: RuntimeInterventionKind::RuntimeAbnormal,
                        ..
                    } if occurrence_id == "occurrence-001"
                ))
                .count(),
            1
        );
        drop(captured);

        events.lock().unwrap().clear();
        terminalize_background_drive_error(
            &app,
            task_id,
            &persisted_run,
            &persisted_round,
            &persisted_node,
            &anyhow!("round persistence failed after run pause"),
        );
        let captured = events.lock().unwrap();
        assert_eq!(
            captured
                .iter()
                .filter(|event| matches!(
                    event,
                    RuntimeLifecycleEvent::InterventionRequested {
                        scheduled_occurrence_id: Some(occurrence_id),
                        kind: RuntimeInterventionKind::RuntimeAbnormal,
                        ..
                    } if occurrence_id == "occurrence-001"
                ))
                .count(),
            1,
            "a partially persisted pause still needs a terminal lifecycle event"
        );
    }

    #[test]
    fn stale_background_drive_error_does_not_override_a_newer_execution() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = events.clone();
        let app = App::new(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                captured_events.lock().unwrap().push(event);
            }));
        let task_id = "task-001";
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            updated_at: "2026-08-06T00:00:02Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("worker".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::RunningNode,
                Some(RuntimeAttemptLocator {
                    round_id: "round-001".to_string(),
                    node_id: "worker".to_string(),
                    attempt_id: "attempt-001".to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "2026-08-06T00:00:02Z",
            ),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let current_node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "worker".to_string(),
            node_type: crate::domain::NodeType::Worker,
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            finished_at: None,
            manual_check_pending: false,
            resolved_config: crate::domain::ResolvedConfig::new(),
            uuid: None,
            runtime_execution_id: Some("execution-b".to_string()),
        };
        persist_runtime_state(&app, task_id, &run, &round, &current_node).unwrap();
        let mut stale_node = current_node.clone();
        stale_node.runtime_execution_id = Some("execution-a".to_string());

        terminalize_background_drive_error(
            &app,
            task_id,
            &run,
            &round,
            &stale_node,
            &anyhow!("late execution-a failure"),
        );

        let persisted_run = app.run_status(task_id, &run.id).unwrap();
        let (_, persisted_node) = current_attempt_state(&app, task_id, &persisted_run).unwrap();
        assert_eq!(persisted_run.status, RunStatus::Running);
        assert_eq!(persisted_run.pause_reason, None);
        assert_eq!(persisted_node.status, RunStatus::Running);
        assert_eq!(
            persisted_node.runtime_execution_id.as_deref(),
            Some("execution-b")
        );
        assert!(events.lock().unwrap().is_empty());
    }

    fn test_context<'a>(app: &'a App, dynamic: &'a AiDynamicNode) -> DynamicExecutionContext<'a> {
        DynamicExecutionContext {
            app,
            task_id: "task-006",
            run_id: "run-001",
            round_id: "round-001",
            outer_node_id: "ai-dynamic",
            outer_attempt_id: "attempt-001",
            outer_runtime_execution_id: None,
            dynamic,
            task_uuid: None,
            run_uuid: None,
            round_uuid: None,
            outer_node_uuid: None,
            parent_continue_input: None,
            parent_continue_prompt_id: None,
            resume_override: None,
        }
    }

    fn test_dynamic_resume_override(
        request_id: &str,
        execution_id: &str,
        node_id: &str,
        launch_signal: Option<mpsc::Sender<RuntimeContinueLaunch>>,
    ) -> DynamicResumeOverride {
        DynamicResumeOverride {
            request_id: request_id.to_string(),
            execution_id: execution_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: format!("continue {node_id}"),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal,
        }
    }

    fn clear_test_dynamic_resume_coordinator(key: &str) {
        if let Some(coordinator) = DYNAMIC_RESUME_COORDINATOR.get() {
            let mut coordinator = coordinator.lock().unwrap();
            coordinator.drivers.remove(key);
            coordinator.pending.remove(key);
            coordinator.inflight.remove(key);
            coordinator.starting.remove(key);
        }
    }

    #[test]
    fn waiting_for_user_input_maps_to_elicitation_when_pending_request_exists() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);
        let task_id = "task-001";
        let run = RunState {
            version: crate::domain::VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("plan".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let round = RoundState {
            version: crate::domain::VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: crate::domain::RoundTrigger::Initial,
            started_at: "1Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: crate::domain::VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "plan".to_string(),
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            node_type: crate::domain::NodeType::Worker,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            finished_at: None,
            resolved_config: std::collections::BTreeMap::new(),
            manual_check_pending: false,
            runtime_execution_id: None,
            uuid: None,
        };
        let attempt_dir =
            app.paths
                .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        write_pending_elicitation(
            &attempt_dir,
            &pending_elicitation_state(
                "elicit-001",
                "turn-1",
                "prompt-turn-1",
                serde_json::json!(1),
                serde_json::from_value(serde_json::json!({
                    "mode": "form",
                    "sessionId": "session-test",
                    "message": "请选择",
                    "requestedSchema": { "type": "object", "properties": {} }
                }))
                .unwrap(),
                "1Z".to_string(),
            ),
        )
        .unwrap();

        let kind = waiting_for_user_input_intervention_kind(&app, task_id, &run, &round, &node);

        assert_eq!(kind, RuntimeInterventionKind::ElicitationRequested);
    }

    #[test]
    fn waiting_for_user_input_keeps_manual_check_when_flagged() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);
        let run = RunState {
            version: crate::domain::VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("plan".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let round = RoundState {
            version: crate::domain::VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: crate::domain::RoundTrigger::Initial,
            started_at: "1Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: crate::domain::VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "plan".to_string(),
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            node_type: crate::domain::NodeType::Worker,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            finished_at: None,
            resolved_config: std::collections::BTreeMap::new(),
            manual_check_pending: true,
            runtime_execution_id: None,
            uuid: None,
        };

        let kind = waiting_for_user_input_intervention_kind(&app, "task-001", &run, &round, &node);

        assert_eq!(kind, RuntimeInterventionKind::ManualDecisionRequired);
    }

    #[test]
    fn manual_check_submission_rejects_a_stale_attempt_before_resume() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let app = App::new(repo_root);
        let task_id = "task-001";
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("review".to_string()),
            current_attempt: Some("attempt-002".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::AwaitingManualCheck,
                Some(RuntimeAttemptLocator {
                    round_id: "round-001".to_string(),
                    node_id: "review".to_string(),
                    attempt_id: "attempt-002".to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "1Z",
            ),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "1Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "review".to_string(),
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-002".to_string(),
            node_type: crate::domain::NodeType::Worker,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            finished_at: None,
            resolved_config: BTreeMap::new(),
            manual_check_pending: true,
            runtime_execution_id: None,
            uuid: None,
        };
        persist_runtime_state(&app, task_id, &run, &round, &node).unwrap();

        validate_manual_check_submission(
            &app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        )
        .unwrap();
        assert!(
            validate_manual_check_submission(
                &app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                "attempt-001",
            )
            .is_err()
        );

        let reservation = reserve_manual_check_submission(
            &app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        )
        .unwrap();
        assert!(
            reserve_manual_check_submission(
                &app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            )
            .is_err(),
            "a second manual decision cannot enter the same Run while submission is in flight"
        );
        drop(reservation);
        assert!(
            reserve_manual_check_submission(
                &app,
                task_id,
                &run.id,
                &round.id,
                &node.node_id,
                &node.attempt_id,
            )
            .is_ok()
        );
    }

    #[test]
    fn manual_check_background_converges_a_precommit_failure() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let lifecycle_events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = lifecycle_events.clone();
        let app = App::new(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                captured_events.lock().unwrap().push(event);
            }));
        let task_id = "task-001";
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "missing-workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("review".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::AwaitingManualCheck,
                Some(RuntimeAttemptLocator {
                    round_id: "round-001".to_string(),
                    node_id: "review".to_string(),
                    attempt_id: "attempt-001".to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "1Z",
            ),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "1Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "review".to_string(),
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            node_type: crate::domain::NodeType::Worker,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            finished_at: None,
            resolved_config: BTreeMap::new(),
            manual_check_pending: true,
            runtime_execution_id: Some("manual-execution-001".to_string()),
            uuid: None,
        };
        persist_runtime_state(&app, task_id, &run, &round, &node).unwrap();

        let submission_lease = reserve_manual_check_submission(
            &app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
        )
        .unwrap();
        let error = submit_manual_check_background(
            &app,
            task_id,
            &run.id,
            &round.id,
            &node.node_id,
            &node.attempt_id,
            NodeOutcome::Success,
            submission_lease,
        )
        .expect_err("the command must not acknowledge before the decision is committed");

        assert_eq!(
            normalize_runtime_error(&error).code_str(),
            "runtime.continue-launch-failed"
        );
        let converged = app.run_status(task_id, &run.id).unwrap();
        assert_eq!(converged.status, RunStatus::Paused);
        assert_eq!(converged.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert!(lifecycle_events.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                RuntimeLifecycleEvent::InterventionRequested {
                    scheduled_occurrence_id: Some(occurrence_id),
                    kind: RuntimeInterventionKind::RuntimeAbnormal,
                    ..
                } if occurrence_id == "occurrence-001"
            )
        }));
    }

    #[test]
    fn manual_check_precommit_failure_does_not_pause_a_replacement_attempt() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let lifecycle_events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = lifecycle_events.clone();
        let app = App::new(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                captured_events.lock().unwrap().push(event);
            }));
        let task_id = "task-001";
        let original_run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "missing-workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("review".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::AwaitingManualCheck,
                Some(RuntimeAttemptLocator {
                    round_id: "round-001".to_string(),
                    node_id: "review".to_string(),
                    attempt_id: "attempt-001".to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "1Z",
            ),
        };
        let original_round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: original_run.id.clone(),
            index: 1,
            status: RunStatus::Paused,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "1Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let original_node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "review".to_string(),
            run_id: original_run.id.clone(),
            round_id: original_round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            node_type: crate::domain::NodeType::Worker,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "1Z".to_string(),
            finished_at: None,
            resolved_config: BTreeMap::new(),
            manual_check_pending: true,
            runtime_execution_id: Some("manual-execution-001".to_string()),
            uuid: None,
        };
        persist_runtime_state(
            &app,
            task_id,
            &original_run,
            &original_round,
            &original_node,
        )
        .unwrap();
        let _reservation = reserve_manual_check_submission(
            &app,
            task_id,
            &original_run.id,
            &original_round.id,
            &original_node.node_id,
            &original_node.attempt_id,
        )
        .unwrap();

        let mut replacement_run = original_run.clone();
        replacement_run.status = RunStatus::Running;
        replacement_run.pause_reason = None;
        replacement_run.current_attempt = Some("attempt-002".to_string());
        let mut replacement_round = original_round.clone();
        replacement_round.status = RunStatus::Running;
        let mut replacement_node = original_node.clone();
        replacement_node.attempt_id = "attempt-002".to_string();
        replacement_node.status = RunStatus::Running;
        replacement_node.manual_check_pending = false;
        replacement_node.runtime_execution_id = Some("execution-b".to_string());
        persist_runtime_state(
            &app,
            task_id,
            &replacement_run,
            &replacement_round,
            &replacement_node,
        )
        .unwrap();

        terminalize_manual_check_precommit_error(
            &app,
            task_id,
            &original_run,
            &original_round,
            &original_node,
            &anyhow!("simulated precommit failure"),
        );

        let durable_run = app.run_status(task_id, &original_run.id).unwrap();
        let durable_node: NodeState = read_json(&app.paths.node_file(
            task_id,
            &original_run.id,
            &original_round.id,
            &replacement_node.node_id,
            &replacement_node.attempt_id,
        ))
        .unwrap();
        assert_eq!(durable_run.status, RunStatus::Running);
        assert_eq!(durable_run.current_attempt.as_deref(), Some("attempt-002"));
        assert_eq!(durable_node.status, RunStatus::Running);
        assert_eq!(
            durable_node.runtime_execution_id.as_deref(),
            Some("execution-b")
        );
        assert!(lifecycle_events.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                RuntimeLifecycleEvent::InterventionRequested {
                    scheduled_occurrence_id: Some(occurrence_id),
                    attempt_id,
                    kind: RuntimeInterventionKind::RuntimeAbnormal,
                    ..
                } if occurrence_id == "occurrence-001" && attempt_id == "attempt-001"
            )
        }));
    }

    #[test]
    fn background_drive_persistence_failure_still_finishes_the_occurrence() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let lifecycle_events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = lifecycle_events.clone();
        let app = App::new(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                captured_events.lock().unwrap().push(event);
            }));
        let task_id = "task-001";
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "1Z".to_string(),
            updated_at: "1Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("worker".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::RunningNode,
                Some(RuntimeAttemptLocator {
                    round_id: "round-001".to_string(),
                    node_id: "worker".to_string(),
                    attempt_id: "attempt-001".to_string(),
                    outer_node_id: None,
                    outer_attempt_id: None,
                }),
                "1Z",
            ),
        };
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: run.id.clone(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "1Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "worker".to_string(),
            run_id: run.id.clone(),
            round_id: round.id.clone(),
            attempt_id: "attempt-001".to_string(),
            node_type: crate::domain::NodeType::Worker,
            status: RunStatus::Running,
            outcome: None,
            started_at: "1Z".to_string(),
            finished_at: None,
            resolved_config: BTreeMap::new(),
            manual_check_pending: false,
            runtime_execution_id: Some("execution-a".to_string()),
            uuid: None,
        };
        persist_runtime_state(&app, task_id, &run, &round, &node).unwrap();
        let round_path = app.paths.round_file(task_id, &run.id, &round.id);
        std::fs::remove_file(round_path.as_std_path()).unwrap();
        std::fs::create_dir_all(round_path.as_std_path()).unwrap();

        terminalize_background_drive_error(
            &app,
            task_id,
            &run,
            &round,
            &node,
            &anyhow!("simulated background failure"),
        );

        let durable_run = app.run_status(task_id, &run.id).unwrap();
        assert_eq!(durable_run.status, RunStatus::Paused);
        assert_eq!(durable_run.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert!(lifecycle_events.lock().unwrap().iter().any(|event| {
            matches!(
                event,
                RuntimeLifecycleEvent::InterventionRequested {
                    scheduled_occurrence_id: Some(occurrence_id),
                    attempt_id,
                    kind: RuntimeInterventionKind::RuntimeAbnormal,
                    ..
                } if occurrence_id == "occurrence-001" && attempt_id == "attempt-001"
            )
        }));
    }

    fn test_worktree_node(id: &str) -> DynamicNodeState {
        DynamicNodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            id: id.to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            kind: DynamicNodeKind::Worker,
            title: id.to_string(),
            task: id.to_string(),
            status: DynamicNodeStatus::Ready,
            outcome: None,
            pause_reason: None,
            runtime_error: None,
            runtime_execution_id: None,
            runtime_execution_phase: None,
            runtime_lifecycle_revision: 0,
            runtime_lifecycle_updated_at: None,
            group_id: None,
            chain_id: id.to_string(),
            depth: 1,
            depends_on: Vec::new(),
            workspace_id: "workspace-main".to_string(),
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
            uuid: None,
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
            worktree: None,
            execution: Default::default(),
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
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
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
            runtime_execution_id: None,
            resolved_config: Default::default(),
            uuid: None,
        };
        persist_runtime_state(app, "task-006", &run, &round, &node).unwrap();
    }

    fn test_workspace(repo_root: Utf8PathBuf) -> WorkspaceState {
        WorkspaceState {
            version: VERSION.to_string(),
            id: "workspace-main".to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            kind: WorkspaceKind::Main,
            ownership: WorkspaceOwnership::User,
            repo_root: repo_root.clone(),
            path: repo_root,
            branch: None,
            parent_workspace_id: None,
            created_by_group_id: None,
            fork_commit: "test-head".to_string(),
            checkpoint_commit: None,
            status: WorkspaceStatus::Active,
            created_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:00Z".to_string(),
        }
    }

    fn test_dynamic_graph_at(
        repo_root: Utf8PathBuf,
        nodes: Vec<DynamicNodeState>,
    ) -> DynamicGraphState {
        DynamicGraphState {
            version: CURRENT_DYNAMIC_GRAPH_VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-001".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "ai-dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: DynamicRunStatus::Running,
                phase: DynamicRunPhase::Executing,
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
            workspaces: vec![test_workspace(repo_root)],
            proposals: Vec::new(),
        }
    }

    fn test_dynamic_graph(nodes: Vec<DynamicNodeState>) -> DynamicGraphState {
        test_dynamic_graph_at(Utf8PathBuf::from("."), nodes)
    }

    fn test_group_state(
        id: &str,
        created_by_node_id: &str,
        root_node_ids: Vec<&str>,
        terminal_node_ids: Vec<&str>,
    ) -> DynamicGroupState {
        DynamicGroupState {
            version: VERSION.to_string(),
            id: id.to_string(),
            dynamic_run_id: "dynamic-run-001".to_string(),
            status: DynamicGroupStatus::Open,
            depth: 1,
            parent_group_id: None,
            root_node_ids: root_node_ids.into_iter().map(ToOwned::to_owned).collect(),
            terminal_node_ids: terminal_node_ids
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            target_workspace_id: "workspace-main".to_string(),
            child_workspace_ids: Vec::new(),
            merge_node_id: None,
            acceptance_node_id: None,
            created_by_node_id: created_by_node_id.to_string(),
            merge: test_agent_task("merge"),
            acceptance: test_agent_task("accept"),
            created_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:00Z".to_string(),
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
    fn dynamic_inline_control_repair_uses_a_distinct_prompt_identity() {
        let (_temp, repo_root) = init_repo();
        let invocations = Arc::new(Mutex::new(Vec::new()));
        let app = App::with_provider(
            repo_root.clone(),
            Box::new(DynamicRepairIdentityProvider {
                invocations: invocations.clone(),
            }),
        );
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("bootstrap");
        node.status = DynamicNodeStatus::Running;
        node.begin_runtime_execution("execution-bootstrap", "2026-06-16T00:00:01Z");
        let mut graph = test_dynamic_graph_at(repo_root, vec![node.clone()]);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let result = execute_dynamic_worker(&ctx, &graph, node).unwrap();

        assert_eq!(result.node.status, DynamicNodeStatus::Completed);
        assert_eq!(result.node.outcome, Some(NodeOutcome::Success));
        assert_eq!(result.proposals.len(), 1);
        assert_eq!(
            result.proposals[0].validation_status,
            DynamicProposalValidationStatus::Accepted
        );
        let invocations = invocations.lock().unwrap();
        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].1, SessionMode::New);
        assert_eq!(invocations[0].2, UserPromptRenderMode::RequirementTask);
        assert_eq!(invocations[1].1, SessionMode::Continue);
        assert_eq!(invocations[1].2, UserPromptRenderMode::RuntimeRepair);
        assert_eq!(
            invocations[1].0,
            dynamic_proposal_repair_prompt_id(&invocations[0].0, 1)
        );
        assert_ne!(invocations[0].0, invocations[1].0);
    }

    fn write_dynamic_completion_artifact(app: &App, node_id: &str, content: String) {
        let artifacts_dir = app.paths.dynamic_node_artifacts_dir(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            node_id,
            "attempt-001",
        );
        std::fs::create_dir_all(artifacts_dir.as_std_path()).unwrap();
        std::fs::write(
            app.paths
                .dynamic_node_artifact_file(
                    "task-006",
                    "run-001",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    node_id,
                    "attempt-001",
                    DYNAMIC_COMPLETION_ARTIFACT,
                )
                .as_std_path(),
            content,
        )
        .unwrap();
    }

    fn write_dynamic_attachment_for_test(
        app: &App,
        ctx: &DynamicExecutionContext<'_>,
        node: &DynamicNodeState,
        name: &str,
        content: &str,
    ) -> Utf8PathBuf {
        let attachments_dir = app.paths.dynamic_node_attachments_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &node.id,
            &dynamic_attempt_id(node),
        );
        std::fs::create_dir_all(attachments_dir.as_std_path()).unwrap();
        let path = attachments_dir.join(name);
        std::fs::write(path.as_std_path(), content).unwrap();
        path
    }

    #[test]
    fn provider_model_options_read_current_provider_cache() {
        let capabilities = serde_json::json!({
            "configOptions": [
                {
                    "category": "model",
                    "options": [
                        { "value": "sonnet", "name": "Sonnet", "description": "fast" },
                        { "value": "opus" }
                    ]
                }
            ]
        });
        let (_temp, app) = test_app_with_provider_capabilities(capabilities);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);

        let values = provider_model_option_values(&ctx, "claude-acp");

        assert_eq!(values, vec!["sonnet".to_string(), "opus".to_string()]);
        assert_eq!(
            provider_model_options_summary(&ctx, "claude-acp"),
            vec!["sonnet (Sonnet) — fast".to_string(), "opus".to_string()]
        );
    }

    #[test]
    fn dynamic_worker_model_validation_uses_provider_model_values() {
        let capabilities = serde_json::json!({
            "configOptions": [
                {
                    "category": "model",
                    "options": [
                        { "value": "sonnet", "name": "gpt-5.4(xhigh)" },
                        { "value": "opus", "name": "gpt-5.5(xhigh)[1m]" }
                    ]
                }
            ]
        });
        let (_temp, app) = test_app_with_provider_capabilities(capabilities);
        let dynamic = AiDynamicNode {
            agent_strategy: AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider: "claude-acp".to_string(),
                bootstrap_model: None,
                permission_mode: None,
                bootstrap_config_options: Default::default(),
                acceptance_model: None,
                acceptance_config_options: Default::default(),
                routing_prompt: "choose provider and model".to_string(),
                available_agents: vec![crate::dsl::DynamicAgentRef {
                    provider: "claude-acp".to_string(),
                    model: None,
                    permission_mode: None,
                    config_options: Default::default(),
                }],
            },
            ..test_dynamic()
        };
        let ctx = test_context(&app, &dynamic);
        let source = test_worktree_node("bootstrap");
        let graph = test_dynamic_graph(vec![source.clone()]);
        let valid = DynamicNodeSpec {
            id: "valid".to_string(),
            kind: DynamicNodeSpecKind::Worker,
            title: "Valid".to_string(),
            task: "work".to_string(),
            provider: Some("claude-acp".to_string()),
            profile: None,
            model: Some("sonnet".to_string()),
            permission_mode: None,
            session_mode: SessionMode::New,
            continue_from_node_id: None,
            depends_on: Vec::new(),
            workflow_id: None,
        };
        let invalid = DynamicNodeSpec {
            model: Some("gpt-5.4(xhigh)".to_string()),
            ..valid.clone()
        };

        assert!(validate_dynamic_node_spec(&ctx, &graph, &source, &valid, 1, true).is_empty());
        let errors = validate_dynamic_node_spec(&ctx, &graph, &source, &invalid, 1, true);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "dynamic.node.model.unsupported");
        assert_eq!(
            errors[0].allowed_values,
            vec!["sonnet".to_string(), "opus".to_string()]
        );
    }

    #[test]
    fn dynamic_fanout_requires_multiple_children_without_workspace_policy() {
        let (_temp, app) = test_app_with_provider_capabilities(serde_json::json!({
            "configOptions": [
                {
                    "category": "model",
                    "options": [
                        { "value": "sonnet" }
                    ]
                }
            ]
        }));
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let source = test_worktree_node("bootstrap");
        let graph = test_dynamic_graph(vec![source]);
        let child = DynamicNodeSpec {
            id: "only-child".to_string(),
            kind: DynamicNodeSpecKind::Worker,
            title: "Only child".to_string(),
            task: "Do one follow-up task.".to_string(),
            provider: None,
            profile: None,
            model: None,
            permission_mode: None,
            session_mode: SessionMode::New,
            continue_from_node_id: None,
            depends_on: Vec::new(),
            workflow_id: None,
        };
        let completion = DynamicNodeCompletion {
            version: VERSION.to_string(),
            kind: DynamicNodeCompletionKind::DynamicNodeCompletion,
            status: DynamicCompletionStatus::Success,
            summary: "one branch".to_string(),
            next: DynamicNext::Fanout {
                group_id: "group-one".to_string(),
                nodes: vec![child],
                merge: test_agent_task("merge"),
                acceptance: test_agent_task("accept"),
            },
            source: None,
        };

        let errors = validate_dynamic_completion(&ctx, &graph, 0, &completion);

        assert!(errors.iter().any(|error| {
            error.code == "dynamic.fanout.nodes.too-few"
                && error.path.as_deref() == Some("next.nodes")
                && error.actual.as_deref() == Some("1")
        }));
        assert!(!errors.iter().any(|error| {
            error
                .path
                .as_deref()
                .is_some_and(|path| path.contains("workspace"))
        }));
    }

    #[test]
    fn dynamic_provider_selection_does_not_require_model_catalog() {
        let (_temp, app) = test_app_with_provider_capabilities(serde_json::json!({
            "configOptions": []
        }));
        let dynamic = AiDynamicNode {
            agent_strategy: AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider: "claude-acp".to_string(),
                bootstrap_model: None,
                permission_mode: None,
                bootstrap_config_options: Default::default(),
                acceptance_model: None,
                acceptance_config_options: Default::default(),
                routing_prompt: "choose provider and model".to_string(),
                available_agents: vec![crate::dsl::DynamicAgentRef {
                    provider: "claude-acp".to_string(),
                    model: None,
                    permission_mode: None,
                    config_options: Default::default(),
                }],
            },
            ..test_dynamic()
        };
        let ctx = test_context(&app, &dynamic);
        let mut graph = test_dynamic_graph(vec![test_worktree_node("bootstrap")]);

        ensure_dynamic_required_model_catalogs(&ctx, &mut graph).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_ne!(graph.nodes[0].status, DynamicNodeStatus::Paused);
    }

    #[test]
    fn dynamic_permission_mode_validation_reads_current_provider_cache() {
        let capabilities = serde_json::json!({
            "configOptions": [
                {
                    "id": "mode",
                    "options": [
                        { "value": "plan", "name": "Plan" },
                        { "value": "acceptEdits", "name": "Accept Edits" }
                    ]
                }
            ]
        });
        let (_temp, app) = test_app_with_provider_capabilities(capabilities);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);

        let allowed = validate_dynamic_permission_mode(&ctx, "claude-acp", "acceptEdits", || {
            DynamicProposalValidationError::new("invalid", "acceptEdits", serde_json::Value::Null)
        });
        let rejected = validate_dynamic_permission_mode(&ctx, "claude-acp", "full_access", || {
            DynamicProposalValidationError::new("invalid", "full_access", serde_json::Value::Null)
        });

        assert!(allowed.is_none());
        assert_eq!(rejected.unwrap().message, "full_access");
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
    fn stop_waits_for_workspace_transition_and_keeps_materialized_workspace() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let mut anchor = test_worktree_node("bootstrap");
        anchor.status = DynamicNodeStatus::Completed;
        anchor.outcome = Some(NodeOutcome::Success);
        anchor.finished_at = Some("2026-06-16T00:00:01Z".to_string());
        let mut graph = test_dynamic_graph_at(repo_root.clone(), vec![anchor]);
        graph.run.current_node_ids.clear();
        let graph_path = app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let (transition_started_tx, transition_started_rx) = mpsc::channel();
        let (release_transition_tx, release_transition_rx) = mpsc::channel();
        let (transition_done_tx, transition_done_rx) = mpsc::channel();
        let (pause_done_tx, pause_done_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let transition_app = &app;
            let pause_app = &app;
            let transition_repo_root = repo_root.clone();
            scope.spawn(move || {
                let ctx = test_context(transition_app, &dynamic);
                let state_lock = dynamic_state_lock(&ctx).unwrap();
                let _state_guard = state_lock.lock();
                let mut graph = graph;
                let transition_owner_node_ids = vec![graph.nodes[0].id.clone()];
                let result = with_dynamic_workspace_transition(
                    &ctx,
                    &mut graph,
                    &transition_owner_node_ids,
                    |graph| {
                        transition_started_tx.send(()).unwrap();
                        release_transition_rx.recv().unwrap();
                        graph.workspaces.push(WorkspaceState {
                            version: VERSION.to_string(),
                            id: "workspace-created".to_string(),
                            dynamic_run_id: graph.run.id.clone(),
                            kind: WorkspaceKind::Worktree,
                            ownership: WorkspaceOwnership::Runtime,
                            repo_root: transition_repo_root.clone(),
                            path: transition_repo_root.join("created-worktree"),
                            branch: Some("gold-band/test-created-worktree".to_string()),
                            parent_workspace_id: Some("workspace-main".to_string()),
                            created_by_group_id: Some("group-test".to_string()),
                            fork_commit: "test-head".to_string(),
                            checkpoint_commit: None,
                            status: WorkspaceStatus::Active,
                            created_at: "2026-06-16T00:00:00Z".to_string(),
                            updated_at: "2026-06-16T00:00:00Z".to_string(),
                        });
                        Ok(())
                    },
                );
                transition_done_tx.send(result).unwrap();
            });

            transition_started_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            let preparing: DynamicGraphState = read_json(&graph_path).unwrap();
            assert_eq!(preparing.run.phase, DynamicRunPhase::PreparingWorkspace);

            scope.spawn(move || {
                let result = pause_app.pause_dynamic_attempt_runtime_state(
                    "task-006",
                    "run-001",
                    "round-001",
                    "ai-dynamic",
                    "attempt-001",
                    "bootstrap",
                    "attempt-001",
                    PauseReason::ProcessInterrupted,
                );
                pause_done_tx.send(result).unwrap();
            });

            let deadline = Instant::now() + Duration::from_secs(2);
            while app.run_status("task-006", "run-001").unwrap().status == RunStatus::Running {
                assert!(
                    Instant::now() < deadline,
                    "outer stop request was not accepted"
                );
                thread::sleep(Duration::from_millis(10));
            }
            assert!(matches!(
                pause_done_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            let still_preparing: DynamicGraphState = read_json(&graph_path).unwrap();
            assert_eq!(
                still_preparing.run.phase,
                DynamicRunPhase::PreparingWorkspace
            );

            release_transition_tx.send(()).unwrap();
            transition_done_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            pause_done_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
        });

        let paused: DynamicGraphState = read_json(&graph_path).unwrap();
        assert_eq!(paused.run.status, DynamicRunStatus::Paused);
        assert_eq!(paused.run.phase, DynamicRunPhase::Executing);
        assert!(
            paused
                .workspaces
                .iter()
                .any(|workspace| workspace.id == "workspace-created")
        );
        assert_eq!(paused.nodes[0].status, DynamicNodeStatus::Completed);
        assert!(paused.nodes[0].runtime_execution_id.is_none());
    }

    #[test]
    fn failed_workspace_transition_restores_executing_phase() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut graph = test_dynamic_graph_at(repo_root, vec![test_worktree_node("bootstrap")]);
        let transition_owner_node_ids = vec!["bootstrap".to_string()];

        let error = with_dynamic_workspace_transition::<()>(
            &ctx,
            &mut graph,
            &transition_owner_node_ids,
            |_graph| bail!("simulated workspace failure"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("simulated workspace failure"));
        assert_eq!(graph.run.phase, DynamicRunPhase::Executing);
        let persisted: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(persisted.run.phase, DynamicRunPhase::Executing);
    }

    #[test]
    fn workspace_transition_advances_one_leaf_lifecycle_watermark_at_each_handoff() {
        let (_temp, repo_root) = init_repo();
        let base_app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let graph_path = base_app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let callback_graph_path = graph_path.clone();
        let seen_revisions = Arc::new(Mutex::new(Vec::new()));
        let callback_revisions = seen_revisions.clone();
        let app = base_app.with_acp_session_update(Arc::new(move |_context| {
            let persisted: DynamicGraphState = read_json(&callback_graph_path)?;
            callback_revisions
                .lock()
                .unwrap()
                .push(persisted.nodes[0].runtime_lifecycle_revision);
            Ok(())
        }));
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut graph = test_dynamic_graph_at(repo_root, vec![test_worktree_node("bootstrap")]);
        let transition_owner_node_ids = vec!["bootstrap".to_string()];

        with_dynamic_workspace_transition(&ctx, &mut graph, &transition_owner_node_ids, |_graph| {
            Ok(())
        })
        .unwrap();

        assert_eq!(graph.nodes[0].runtime_lifecycle_revision, 2);
        let persisted: DynamicGraphState = read_json(&graph_path).unwrap();
        assert_eq!(persisted.nodes[0].runtime_lifecycle_revision, 2);
        assert_eq!(*seen_revisions.lock().unwrap(), vec![1, 2]);
    }

    #[test]
    fn workspace_fanout_advances_only_the_causal_leaf_lifecycle_watermark() {
        let (_temp, repo_root) = init_repo();
        let base_app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let graph_path = base_app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let callback_graph_path = graph_path.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let callback_seen = seen.clone();
        let app = base_app.with_acp_session_update(Arc::new(move |context| {
            let persisted: DynamicGraphState = read_json(&callback_graph_path)?;
            let revision = persisted
                .nodes
                .iter()
                .find(|node| node.id == context.node_id)
                .map(|node| node.runtime_lifecycle_revision)
                .unwrap_or_default();
            callback_seen
                .lock()
                .unwrap()
                .push((context.node_id, revision));
            Ok(())
        }));
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut bootstrap = test_worktree_node("bootstrap");
        bootstrap.status = DynamicNodeStatus::Completed;
        bootstrap.outcome = Some(NodeOutcome::Success);
        bootstrap.complete_runtime_execution("2026-06-16T00:00:01Z");
        let mut graph = test_dynamic_graph_at(repo_root, vec![bootstrap]);
        graph.run.current_node_ids.clear();
        let transition_owner_node_ids = vec!["bootstrap".to_string()];

        with_dynamic_workspace_transition(&ctx, &mut graph, &transition_owner_node_ids, |graph| {
            graph.nodes.push(test_worktree_node("hello-worker"));
            graph.nodes.push(test_worktree_node("goodbye-worker"));
            graph.run.current_node_ids =
                vec!["hello-worker".to_string(), "goodbye-worker".to_string()];
            Ok(())
        })
        .unwrap();

        assert_eq!(graph.nodes[0].runtime_lifecycle_revision, 3);
        assert_eq!(graph.nodes[1].runtime_lifecycle_revision, 0);
        assert_eq!(graph.nodes[2].runtime_lifecycle_revision, 0);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![("bootstrap".to_string(), 2), ("bootstrap".to_string(), 3),]
        );
    }

    #[test]
    fn launch_ready_dynamic_nodes_returns_empty_without_scheduler_events_when_no_ready_nodes() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("bootstrap");
        node.status = DynamicNodeStatus::Pending;
        let mut graph = test_dynamic_graph(vec![node]);
        let (tx, _rx) = mpsc::channel();
        let mut overrides = Vec::new();

        let launched = launch_ready_dynamic_nodes(&ctx, &mut graph, &tx, &mut overrides).unwrap();

        assert!(launched.is_empty());
        assert!(
            !app.paths
                .dynamic_events_file(
                    ctx.task_id,
                    ctx.run_id,
                    ctx.round_id,
                    ctx.outer_node_id,
                    ctx.outer_attempt_id,
                )
                .exists()
        );
    }

    #[test]
    fn dynamic_group_successor_creation_emits_session_update() {
        let (_temp, repo_root) = init_repo();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default())
            .with_acp_session_update(Arc::new(move |context| {
                seen_for_callback.lock().unwrap().push(context);
                Ok(())
            }));
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut source = test_worktree_node("bootstrap");
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        source.group_id = Some("python-classes".to_string());
        source.chain_id = "bootstrap".to_string();
        let mut graph = test_dynamic_graph_at(repo_root, vec![source]);
        let child_workspace_id = fork_dynamic_workspace(
            &ctx,
            &mut graph,
            "workspace-main",
            "python-classes",
            "bootstrap",
        )
        .unwrap();
        graph.nodes[0].workspace_id = child_workspace_id.clone();
        graph.groups.push(DynamicGroupState {
            version: VERSION.to_string(),
            id: "python-classes".to_string(),
            dynamic_run_id: graph.run.id.clone(),
            status: DynamicGroupStatus::Open,
            depth: 1,
            parent_group_id: None,
            root_node_ids: vec!["bootstrap".to_string()],
            terminal_node_ids: vec!["bootstrap".to_string()],
            target_workspace_id: "workspace-main".to_string(),
            child_workspace_ids: vec![child_workspace_id],
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
        let merge_calls = calls
            .iter()
            .filter(|call| call.node_id == "python-classes-merge")
            .collect::<Vec<_>>();
        assert_eq!(merge_calls.len(), 1);
        assert_eq!(merge_calls[0].attempt_id, "attempt-001");
        assert_eq!(merge_calls[0].outer_node_id.as_deref(), Some("ai-dynamic"));
    }

    #[test]
    fn dynamic_worktree_invocation_uses_repo_adapter_workspace_and_worktree_session_workspace() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("good-night");
        let worktree_id = "workspace-invocation";
        let worktree_dir = dynamic_worktree_dir(&ctx, worktree_id);
        let branch = dynamic_worktree_branch_name(&ctx, worktree_id);
        GitWorkspaceManager::default()
            .create_worktree(&repo_root, &worktree_dir, &branch, "HEAD")
            .unwrap();
        node.workspace_id = worktree_id.to_string();
        let mut graph = test_dynamic_graph_at(repo_root.clone(), vec![node.clone()]);
        let mut workspace = test_workspace(repo_root.clone());
        workspace.id = worktree_id.to_string();
        workspace.kind = WorkspaceKind::Worktree;
        workspace.ownership = WorkspaceOwnership::Runtime;
        workspace.path = worktree_dir.clone();
        workspace.branch = Some(branch);
        workspace.parent_workspace_id = Some("workspace-main".to_string());
        workspace.created_by_group_id = Some("group-invocation".to_string());
        workspace.fork_commit = GitRepositoryService::default().head(&repo_root).unwrap();
        graph.workspaces.push(workspace);

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &node,
            &dynamic_attempt_id(&node),
            None,
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(invocation.adapter_workspace_dir, repo_root);
        assert_eq!(invocation.workspace_dir, worktree_dir);
    }

    #[test]
    fn dynamic_prompt_moves_runtime_facts_to_hidden_context() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("bootstrap");
        node.depth = 0;
        let graph = test_dynamic_graph_at(app.paths.repo_root.clone(), vec![node.clone()]);

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &node,
            &dynamic_attempt_id(&node),
            dynamic_output_contract_for_node(&ctx, &graph, &node),
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.system_prompt.contains("AI-DYNAMIC 稳定规则"));
        assert!(prompt.system_prompt.contains("用户主动打断当前工作"));
        assert!(prompt.system_prompt.contains("dynamic-node-completion"));
        assert!(!prompt.system_prompt.contains("本次业务 turn 使用后置控制流程"));
        assert!(!prompt.system_prompt.contains("内部 attempt 目录"));
        assert!(!prompt.system_prompt.contains("remaining dynamic nodes"));
        assert!(!prompt.system_prompt.contains("当前链路可复用会话节点"));
        assert!(
            prompt
                .user_prompt
                .matches("<hidden data-gold-band-hidden=\"true\"")
                .count()
                == 1
        );
        assert!(prompt.user_prompt.contains("# 本次 AI-DYNAMIC 运行上下文"));
        assert!(!prompt.user_prompt.contains("## 最新前序执行链"));
        assert!(!prompt.user_prompt.contains("当前节点的前序运行节点：无"));
        assert!(!prompt.user_prompt.contains("## Current task"));
        assert!(prompt.user_prompt.contains("remaining dynamic nodes"));
        assert!(prompt.user_prompt.contains("bootstrap"));
    }

    #[test]
    fn dynamic_task_instruction_keeps_global_goal_as_user_tips() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let mut dynamic = test_dynamic();
        dynamic.global_goal = Some("初始fanout时必须用至少两个节点完成开发任务".to_string());
        let ctx = test_context(&app, &dynamic);
        let mut node = test_worktree_node("implement-greeting-classes");
        node.task = "实现 .claude 下的问候 Python 类。".to_string();
        let graph = test_dynamic_graph(vec![node.clone()]);

        let task = dynamic_task_instruction(&ctx, &graph, &node, true);

        assert!(!task.contains("初始fanout时必须用至少两个节点完成开发任务"));
        assert!(!task.contains("---"));
        assert!(task.contains("实现 .claude 下的问候 Python 类。"));
        assert_eq!(
            dynamic_user_tips_instruction(&ctx).as_deref(),
            Some("初始fanout时必须用至少两个节点完成开发任务")
        );
        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &node,
            &dynamic_attempt_id(&node),
            dynamic_output_contract_for_node(&ctx, &graph, &node),
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let prompt = render_prompt_bundle(&invocation).unwrap();
        assert!(
            prompt
                .user_prompt
                .contains("# 用户提示\n初始fanout时必须用至少两个节点完成开发任务")
        );
        let task_section = prompt
            .user_prompt
            .rsplit_once("# 任务")
            .map(|(_, task)| task)
            .unwrap_or(&prompt.user_prompt);
        assert!(task_section.contains("实现 .claude 下的问候 Python 类。"));
        assert!(!task_section.contains("初始fanout时必须用至少两个节点完成开发任务"));
    }

    #[test]
    fn dynamic_prompt_projection_shows_attachments_without_control_artifacts() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut source = test_worktree_node("dev-good-morning");
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        source.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut node = test_worktree_node("verify-good-morning");
        node.depends_on = vec![source.id.clone()];
        let graph = test_dynamic_graph(vec![source.clone(), node.clone()]);
        let attachments_dir = app.paths.dynamic_node_attachments_dir(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &source.id,
            &dynamic_attempt_id(&source),
        );
        std::fs::create_dir_all(attachments_dir.as_std_path()).unwrap();
        std::fs::write(
            attachments_dir.join("dev-report.md").as_std_path(),
            "GoodMorning verified.",
        )
        .unwrap();

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &node,
            &dynamic_attempt_id(&node),
            dynamic_output_contract_for_node(&ctx, &graph, &node),
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.user_prompt.contains("## 直接前序节点"));
        assert!(prompt.user_prompt.contains("dev-good-morning"));
        assert!(prompt.user_prompt.contains("## 可用附件"));
        assert!(prompt.user_prompt.contains("dev-report.md"));
        assert!(
            !prompt
                .user_prompt
                .contains("artifacts/dynamic-node-completion")
        );
        assert!(!prompt.user_prompt.contains("completion="));
        assert!(!prompt.system_prompt.contains("dynamic-node-completion"));
        assert!(
            prompt
                .system_prompt
                .contains("本次业务 turn 使用后置控制流程")
        );
        assert!(
            prompt
                .system_prompt
                .contains("不要在当前 turn 拆分任务、选择 Agent、规划或执行后继节点")
        );
        assert!(!prompt.system_prompt.contains("本次 invocation 是执行型节点"));
        assert!(prompt.system_prompt.contains("隐藏 finalize turn"));
    }

    #[test]
    fn dynamic_prompt_projection_hides_parallel_sibling_attachments_from_worker() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut bootstrap = test_worktree_node("bootstrap");
        bootstrap.status = DynamicNodeStatus::Completed;
        bootstrap.outcome = Some(NodeOutcome::Success);
        bootstrap.depth = 0;
        let mut branch_a = test_worktree_node("branch-a");
        branch_a.group_id = Some("group-core".to_string());
        branch_a.chain_id = "branch-a".to_string();
        let mut branch_b = test_worktree_node("branch-b");
        branch_b.group_id = Some("group-core".to_string());
        branch_b.chain_id = "branch-b".to_string();
        branch_b.status = DynamicNodeStatus::Completed;
        branch_b.outcome = Some(NodeOutcome::Success);
        let mut graph = test_dynamic_graph(vec![bootstrap, branch_a.clone(), branch_b.clone()]);
        graph.groups.push(test_group_state(
            "group-core",
            "bootstrap",
            vec!["branch-a", "branch-b"],
            Vec::new(),
        ));
        write_dynamic_attachment_for_test(
            &app,
            &ctx,
            &branch_b,
            "branch-b-report.md",
            "branch-b evidence",
        );

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &branch_a,
            &dynamic_attempt_id(&branch_a),
            dynamic_output_contract_for_node(&ctx, &graph, &branch_a),
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.user_prompt.contains("## 并行兄弟节点"));
        assert!(prompt.user_prompt.contains("branch-b"));
        assert!(!prompt.user_prompt.contains("branch-b-report.md"));
    }

    #[test]
    fn dynamic_prompt_projection_merge_receives_group_branch_attachments() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut branch_a = test_worktree_node("branch-a");
        branch_a.group_id = Some("group-core".to_string());
        branch_a.status = DynamicNodeStatus::Completed;
        branch_a.outcome = Some(NodeOutcome::Success);
        let mut branch_b = test_worktree_node("branch-b");
        branch_b.group_id = Some("group-core".to_string());
        branch_b.status = DynamicNodeStatus::Completed;
        branch_b.outcome = Some(NodeOutcome::Success);
        let mut merge = test_worktree_node("group-core-merge");
        merge.kind = DynamicNodeKind::Merge;
        merge.group_id = Some("group-core".to_string());
        merge.depends_on = vec!["branch-a".to_string(), "branch-b".to_string()];
        let mut graph = test_dynamic_graph(vec![branch_a.clone(), branch_b.clone(), merge.clone()]);
        let mut group = test_group_state(
            "group-core",
            "bootstrap",
            vec!["branch-a", "branch-b"],
            vec!["branch-a", "branch-b"],
        );
        group.status = DynamicGroupStatus::Merging;
        group.merge_node_id = Some("group-core-merge".to_string());
        graph.groups.push(group);
        write_dynamic_attachment_for_test(&app, &ctx, &branch_a, "a-report.md", "a evidence");
        write_dynamic_attachment_for_test(&app, &ctx, &branch_b, "b-report.md", "b evidence");

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &merge,
            &dynamic_attempt_id(&merge),
            None,
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.user_prompt.contains("## 当前 group"));
        assert!(prompt.user_prompt.contains("## 可用附件"));
        assert!(prompt.user_prompt.contains("a-report.md"));
        assert!(prompt.user_prompt.contains("b-report.md"));
        assert!(!prompt.user_prompt.contains("## 并行兄弟节点"));
        assert!(!prompt.user_prompt.contains("## 会话复用"));
        assert!(!prompt.user_prompt.contains("## 运行预算"));
        assert!(!prompt.user_prompt.contains("## Agent 与 profile 选项"));
        assert!(!prompt.system_prompt.contains("dynamic-node-completion"));
        assert!(!prompt.system_prompt.contains("next.type"));
        assert!(
            !prompt
                .user_prompt
                .contains("output contract 要求的控制 JSON")
        );
    }

    #[test]
    fn dynamic_prompt_projection_acceptance_keeps_control_context_without_sibling_noise() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut branch_a = test_worktree_node("branch-a");
        branch_a.group_id = Some("group-core".to_string());
        branch_a.status = DynamicNodeStatus::Completed;
        branch_a.outcome = Some(NodeOutcome::Success);
        let mut branch_b = test_worktree_node("branch-b");
        branch_b.group_id = Some("group-core".to_string());
        branch_b.status = DynamicNodeStatus::Completed;
        branch_b.outcome = Some(NodeOutcome::Success);
        let mut merge = test_worktree_node("group-core-merge");
        merge.kind = DynamicNodeKind::Merge;
        merge.group_id = Some("group-core".to_string());
        merge.status = DynamicNodeStatus::Completed;
        merge.outcome = Some(NodeOutcome::Success);
        let mut acceptance = test_worktree_node("group-core-acceptance");
        acceptance.kind = DynamicNodeKind::Acceptance;
        acceptance.group_id = Some("group-core".to_string());
        acceptance.depends_on = vec![merge.id.clone()];
        let mut graph = test_dynamic_graph(vec![
            branch_a.clone(),
            branch_b.clone(),
            merge.clone(),
            acceptance.clone(),
        ]);
        let mut group = test_group_state(
            "group-core",
            "bootstrap",
            vec!["branch-a", "branch-b"],
            vec!["branch-a", "branch-b"],
        );
        group.status = DynamicGroupStatus::Accepting;
        group.merge_node_id = Some("group-core-merge".to_string());
        group.acceptance_node_id = Some("group-core-acceptance".to_string());
        graph.groups.push(group);

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &acceptance,
            &dynamic_attempt_id(&acceptance),
            dynamic_output_contract_for_node(&ctx, &graph, &acceptance),
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let finalize_context = invocation
            .output_contract
            .as_ref()
            .and_then(|contract| contract.finalize_context.as_deref())
            .expect("acceptance finalizer needs dynamic routing context");
        assert!(finalize_context.contains("## 会话复用"));
        assert!(finalize_context.contains("## 运行预算"));
        assert!(finalize_context.contains("## Agent 与 profile 选项"));
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.user_prompt.contains("## 当前 group"));
        assert!(!prompt.user_prompt.contains("## 并行兄弟节点"));
        assert!(!prompt.user_prompt.contains("## 会话复用"));
        assert!(!prompt.user_prompt.contains("## 运行预算"));
        assert!(!prompt.user_prompt.contains("## Agent 与 profile 选项"));
        assert!(!prompt.system_prompt.contains("dynamic-node-completion"));
        assert!(!prompt.system_prompt.contains("next.type"));
        assert!(
            prompt
                .system_prompt
                .contains("本次业务 turn 使用后置控制流程")
        );
        assert!(prompt.system_prompt.contains("隐藏 finalize turn"));

        let mut finalize_invocation = invocation;
        finalize_invocation.session_mode = SessionMode::Continue;
        finalize_invocation.user_prompt_render_mode = UserPromptRenderMode::RuntimeFinalize;
        finalize_invocation.resume_prompt_visibility = PromptVisibility::Hidden;
        finalize_invocation.resume_prompt = Some("finalize".to_string());
        let finalize_prompt = render_prompt_bundle(&finalize_invocation).unwrap();
        assert!(
            !finalize_prompt
                .system_prompt
                .contains("dynamic-node-completion")
        );
        assert!(!finalize_prompt.system_prompt.contains("next.type"));
        assert!(finalize_prompt.system_prompt.contains("隐藏 finalize turn"));
    }

    #[test]
    fn dynamic_prompt_projection_nested_fanout_inherits_parent_group_exit_attachments() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut parent_accept = test_worktree_node("g1-accept");
        parent_accept.kind = DynamicNodeKind::Acceptance;
        parent_accept.group_id = Some("g1".to_string());
        parent_accept.status = DynamicNodeStatus::Completed;
        parent_accept.outcome = Some(NodeOutcome::Success);
        let mut source = test_worktree_node("repair-source");
        source.group_id = Some("g1".to_string());
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        let mut child_a = test_worktree_node("child-a");
        child_a.group_id = Some("g2".to_string());
        child_a.chain_id = "child-a".to_string();
        let mut child_b = test_worktree_node("child-b");
        child_b.group_id = Some("g2".to_string());
        child_b.chain_id = "child-b".to_string();
        let mut graph = test_dynamic_graph(vec![
            parent_accept.clone(),
            source.clone(),
            child_a.clone(),
            child_b.clone(),
        ]);
        let mut parent_group = test_group_state(
            "g1",
            "bootstrap",
            vec!["root-a", "root-b"],
            vec!["g1-accept"],
        );
        parent_group.status = DynamicGroupStatus::Open;
        parent_group.acceptance_node_id = Some("g1-accept".to_string());
        let mut child_group = test_group_state(
            "g2",
            "repair-source",
            vec!["child-a", "child-b"],
            Vec::new(),
        );
        child_group.depth = 2;
        child_group.parent_group_id = Some("g1".to_string());
        graph.groups.push(parent_group);
        graph.groups.push(child_group);
        write_dynamic_attachment_for_test(
            &app,
            &ctx,
            &parent_accept,
            "verification.json",
            r#"{ "accepted": false }"#,
        );
        write_dynamic_attachment_for_test(
            &app,
            &ctx,
            &child_b,
            "child-b-report.md",
            "sibling evidence",
        );

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &child_a,
            &dynamic_attempt_id(&child_a),
            dynamic_output_contract_for_node(&ctx, &graph, &child_a),
            SessionMode::New,
            None,
            None,
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::RequirementTask,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.user_prompt.contains("## 继承的 group 上下文"));
        assert!(prompt.user_prompt.contains("g1"));
        assert!(prompt.user_prompt.contains("verification.json"));
        assert!(prompt.user_prompt.contains("child-b"));
        assert!(!prompt.user_prompt.contains("child-b-report.md"));
    }

    #[test]
    fn dynamic_continue_prompt_keeps_current_node_task_visible() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut source = test_worktree_node("hello-step");
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        let mut node = test_worktree_node("goodbye-step");
        node.task = "Create the good bye class in the current workspace.".to_string();
        node.session_mode = SessionMode::Continue;
        node.continue_from_node_id = Some("hello-step".to_string());
        node.chain_id = source.chain_id.clone();
        let graph = test_dynamic_graph(vec![source, node.clone()]);

        let invocation = build_dynamic_worker_invocation(
            &ctx,
            &graph,
            &node,
            &dynamic_attempt_id(&node),
            dynamic_output_contract_for_node(&ctx, &graph, &node),
            SessionMode::Continue,
            Some(serde_json::json!({ "acpSessionId": "session-1" })),
            Some(localized_workflow_resume_prompt(
                ctx.app.config.desktop_language,
            )),
            "test-turn".to_string(),
            None,
            PromptVisibility::Visible,
            UserPromptRenderMode::WorkflowResume,
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        let finalize_context = invocation
            .output_contract
            .as_ref()
            .and_then(|contract| contract.finalize_context.as_deref())
            .expect("continued dynamic worker needs finalize context");
        assert!(finalize_context.contains("continueFromNodeId"));
        assert!(finalize_context.contains("hello-step"));
        let prompt = render_prompt_bundle(&invocation).unwrap();

        assert!(prompt.user_prompt.contains("# 目标"));
        assert!(prompt.user_prompt.contains("# 任务"));
        assert!(prompt.user_prompt.contains("goodbye-step"));
        assert!(!prompt.user_prompt.contains("continueFromNodeId"));
        assert!(prompt.user_prompt.contains("hello-step"));
        assert!(
            prompt
                .user_prompt
                .contains("Create the good bye class in the current workspace.")
        );
        let task_section = prompt
            .user_prompt
            .rsplit_once("# 任务")
            .map(|(_, task)| task)
            .unwrap_or(&prompt.user_prompt);
        assert!(task_section.contains("Create the good bye class in the current workspace."));
        assert!(!task_section.contains("当前 AI-DYNAMIC 内部节点"));
        assert!(!task_section.contains("Current AI-DYNAMIC internal node"));
        assert!(!task_section.contains("continueFromNodeId"));
        assert!(!task_section.contains("本次 continue 只复用"));
        assert!(!task_section.contains("This continue only reuses"));
        assert!(!task_section.contains("完成当前节点任务"));
        assert!(!task_section.contains("output contract 要求的控制 JSON"));
    }

    #[test]
    fn acceptance_uses_completion_contract_but_merge_does_not() {
        assert!(dynamic_node_uses_completion_contract(
            DynamicNodeKind::Acceptance
        ));
        assert!(dynamic_node_uses_completion_contract(
            DynamicNodeKind::Worker
        ));
        assert!(!dynamic_node_uses_completion_contract(
            DynamicNodeKind::Merge
        ));
    }

    #[test]
    fn dynamic_bootstrap_is_inline_but_work_nodes_are_post_turn() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut bootstrap = test_worktree_node(DYNAMIC_BOOTSTRAP_NODE_ID);
        bootstrap.depth = 0;
        let worker = test_worktree_node("implementation");
        let mut acceptance = test_worktree_node("acceptance");
        acceptance.kind = DynamicNodeKind::Acceptance;
        let mut merge = test_worktree_node("merge");
        merge.kind = DynamicNodeKind::Merge;
        let graph = test_dynamic_graph(vec![
            bootstrap.clone(),
            worker.clone(),
            acceptance.clone(),
            merge.clone(),
        ]);

        assert!(dynamic_node_is_bootstrap_dispatch(&bootstrap));
        assert_eq!(
            dynamic_output_emission_mode_for_node(&bootstrap),
            Some(OutputEmissionMode::InlineControl)
        );
        assert_eq!(
            dynamic_output_emission_mode_for_node(&worker),
            Some(OutputEmissionMode::PostTurnProjection)
        );
        assert_eq!(dynamic_output_emission_mode_for_node(&merge), None);
        assert_eq!(
            dynamic_output_contract_for_node(&ctx, &graph, &bootstrap)
                .unwrap()
                .emission_mode,
            OutputEmissionMode::InlineControl
        );
        assert_eq!(
            dynamic_output_contract_for_node(&ctx, &graph, &worker)
                .unwrap()
                .emission_mode,
            OutputEmissionMode::PostTurnProjection
        );
        assert_eq!(
            dynamic_output_contract_for_node(&ctx, &graph, &acceptance)
                .unwrap()
                .emission_mode,
            OutputEmissionMode::PostTurnProjection
        );
    }

    #[test]
    fn dynamic_system_prompt_distinguishes_all_control_emission_modes() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);

        let inline = dynamic_system_sections(&ctx, Some(OutputEmissionMode::InlineControl))
            .unwrap()
            .join("\n");
        assert!(inline.contains("最后一步必须产出 `dynamic-node-completion` artifact"));
        assert!(!inline.contains("本次业务 turn 使用后置控制流程"));
        assert!(!inline.contains("本次 invocation 是执行型节点"));

        let deferred =
            dynamic_system_sections(&ctx, Some(OutputEmissionMode::PostTurnProjection))
                .unwrap()
                .join("\n");
        assert!(deferred.contains("本次业务 turn 使用后置控制流程"));
        assert!(deferred.contains("立即停止执行并自然结束本 turn"));
        assert!(deferred.contains("只有收到 runtime 的 hidden finalize 提示后"));
        assert!(!deferred.contains("dynamic-node-completion"));
        assert!(!deferred.contains("next.type"));
        assert!(!deferred.contains("本次 invocation 是执行型节点"));

        let execution_only = dynamic_system_sections(&ctx, None).unwrap().join("\n");
        assert!(execution_only.contains("本次 invocation 是执行型节点"));
        assert!(!execution_only.contains("本次业务 turn 使用后置控制流程"));
        assert!(!execution_only.contains("dynamic-node-completion"));

        let mut english_config = RuntimeConfig::default();
        english_config.desktop_language = DesktopLanguage::En;
        let english_app = App::with_config(repo_root, english_config);
        let english_ctx = test_context(&english_app, &dynamic);
        let english_deferred = dynamic_system_sections(
            &english_ctx,
            Some(OutputEmissionMode::PostTurnProjection),
        )
        .unwrap()
        .join("\n");
        assert!(english_deferred.contains("This business turn uses deferred control"));
        assert!(english_deferred.contains("stop execution immediately and end this turn naturally"));
        assert!(english_deferred.contains("Only after receiving runtime's hidden finalize prompt"));
        assert!(!english_deferred.contains("dynamic-node-completion"));
        assert!(!english_deferred.contains("next.type"));
    }

    #[test]
    fn interrupted_post_turn_projection_is_not_trusted_as_complete() {
        assert!(interrupted_dynamic_completion_is_trusted(
            OutputEmissionMode::InlineControl
        ));
        assert!(!interrupted_dynamic_completion_is_trusted(
            OutputEmissionMode::PostTurnProjection
        ));
    }

    fn accepted_proposal(source_node_id: &str, parsed: serde_json::Value) -> DynamicProposalState {
        DynamicProposalState {
            version: VERSION.to_string(),
            id: format!("proposal-{source_node_id}-001"),
            dynamic_run_id: "dynamic-run-001".to_string(),
            source_node_id: source_node_id.to_string(),
            artifact_path: Utf8PathBuf::from("artifact.json"),
            raw_output_path: Utf8PathBuf::from("raw.jsonl"),
            parsed,
            validation_status: DynamicProposalValidationStatus::Accepted,
            validation_errors: Vec::new(),
            materialized_event_ids: Vec::new(),
            created_at: "2026-06-16T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn acceptance_group_closes_only_after_end_completion() {
        let mut acceptance = test_worktree_node("python-classes-accept");
        acceptance.kind = DynamicNodeKind::Acceptance;
        acceptance.status = DynamicNodeStatus::Completed;
        acceptance.outcome = Some(NodeOutcome::Success);
        acceptance.group_id = Some("python-classes".to_string());
        let mut graph = test_dynamic_graph(vec![acceptance]);
        graph.proposals.push(accepted_proposal(
            "python-classes-accept",
            serde_json::json!({
                "version": "0.1",
                "kind": "dynamic-node-completion",
                "status": "success",
                "summary": "accepted",
                "next": { "type": "end" }
            }),
        ));

        assert!(acceptance_completed_with_end(
            &graph,
            Some("python-classes-accept")
        ));

        graph.proposals[0].parsed = serde_json::json!({
            "version": "0.1",
            "kind": "dynamic-node-completion",
            "status": "success",
            "summary": "needs repair",
            "next": {
                "type": "single",
                "node": {
                    "id": "repair",
                    "kind": "worker",
                    "title": "Repair",
                    "task": "Repair the failed acceptance item."
                }
            }
        });

        assert!(!acceptance_completed_with_end(
            &graph,
            Some("python-classes-accept")
        ));
    }

    #[test]
    fn acceptance_repair_materialization_reopens_group() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut acceptance = test_worktree_node("python-classes-accept");
        acceptance.kind = DynamicNodeKind::Acceptance;
        acceptance.status = DynamicNodeStatus::Completed;
        acceptance.outcome = Some(NodeOutcome::Success);
        acceptance.group_id = Some("python-classes".to_string());
        acceptance.chain_id = "python-classes-accept".to_string();
        let mut graph = test_dynamic_graph(vec![acceptance]);
        graph.groups.push(DynamicGroupState {
            version: VERSION.to_string(),
            id: "python-classes".to_string(),
            dynamic_run_id: graph.run.id.clone(),
            status: DynamicGroupStatus::Accepting,
            depth: 1,
            parent_group_id: None,
            root_node_ids: vec!["root".to_string()],
            terminal_node_ids: vec!["root".to_string()],
            target_workspace_id: "workspace-main".to_string(),
            child_workspace_ids: Vec::new(),
            merge_node_id: Some("python-classes-merge".to_string()),
            acceptance_node_id: Some("python-classes-accept".to_string()),
            created_by_node_id: "root".to_string(),
            merge: test_agent_task("merge"),
            acceptance: test_agent_task("accept"),
            created_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:00Z".to_string(),
        });

        let visible = materialize_dynamic_next(
            &ctx,
            &mut graph,
            0,
            DynamicNext::Single {
                node: DynamicNodeSpec {
                    id: "repair".to_string(),
                    kind: DynamicNodeSpecKind::Worker,
                    title: "Repair".to_string(),
                    task: "Repair the failed acceptance item.".to_string(),
                    provider: None,
                    profile: None,
                    model: None,
                    permission_mode: None,
                    session_mode: SessionMode::New,
                    continue_from_node_id: None,
                    depends_on: Vec::new(),
                    workflow_id: None,
                },
            },
        )
        .unwrap();

        assert_eq!(graph.groups[0].status, DynamicGroupStatus::Open);
        assert_eq!(graph.groups[0].merge_node_id, None);
        assert_eq!(graph.groups[0].acceptance_node_id, None);
        assert!(graph.nodes.iter().any(|node| node.id == "repair"));
        assert_eq!(visible, vec!["repair".to_string()]);
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
    fn dynamic_single_inherits_source_workspace() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut source = test_worktree_node("source");
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        let mut graph = test_dynamic_graph_at(repo_root, vec![source]);

        let visible = materialize_dynamic_next(
            &ctx,
            &mut graph,
            0,
            DynamicNext::Single {
                node: DynamicNodeSpec {
                    id: "successor".to_string(),
                    kind: DynamicNodeSpecKind::Worker,
                    title: "Successor".to_string(),
                    task: "Continue in the same workspace.".to_string(),
                    provider: None,
                    profile: None,
                    model: None,
                    permission_mode: None,
                    session_mode: SessionMode::New,
                    continue_from_node_id: None,
                    depends_on: Vec::new(),
                    workflow_id: None,
                },
            },
        )
        .unwrap();

        let successor = graph
            .nodes
            .iter()
            .find(|node| node.id == "successor")
            .unwrap();
        assert_eq!(successor.workspace_id, "workspace-main");
        assert_eq!(visible, vec!["successor".to_string()]);
    }

    #[test]
    fn nested_fanout_forks_from_and_targets_parent_runtime_workspace() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut source = test_worktree_node("nested-source");
        source.status = DynamicNodeStatus::Completed;
        source.outcome = Some(NodeOutcome::Success);
        let mut graph = test_dynamic_graph_at(repo_root, vec![source]);
        let parent_workspace_id = fork_dynamic_workspace(
            &ctx,
            &mut graph,
            "workspace-main",
            "outer-group",
            "nested-source",
        )
        .unwrap();
        graph.nodes[0].workspace_id = parent_workspace_id.clone();

        let branch = |id: &str| DynamicNodeSpec {
            id: id.to_string(),
            kind: DynamicNodeSpecKind::Worker,
            title: id.to_string(),
            task: format!("Implement {id}."),
            provider: None,
            profile: None,
            model: None,
            permission_mode: None,
            session_mode: SessionMode::New,
            continue_from_node_id: None,
            depends_on: Vec::new(),
            workflow_id: None,
        };
        materialize_dynamic_next(
            &ctx,
            &mut graph,
            0,
            DynamicNext::Fanout {
                group_id: "nested-group".to_string(),
                nodes: vec![branch("child-a"), branch("child-b")],
                merge: test_agent_task("merge"),
                acceptance: test_agent_task("accept"),
            },
        )
        .unwrap();

        let group = graph
            .groups
            .iter()
            .find(|group| group.id == "nested-group")
            .unwrap();
        assert_eq!(group.target_workspace_id, parent_workspace_id);
        assert_eq!(group.child_workspace_ids.len(), 2);
        assert_ne!(group.child_workspace_ids[0], group.child_workspace_ids[1]);
        for child_id in &group.child_workspace_ids {
            let child = dynamic_workspace(&graph, child_id).unwrap();
            assert_eq!(
                child.parent_workspace_id.as_deref(),
                Some(parent_workspace_id.as_str())
            );
            assert_eq!(child.created_by_group_id.as_deref(), Some("nested-group"));
            assert_eq!(child.status, WorkspaceStatus::Active);
        }
        assert_eq!(
            dynamic_workspace(&graph, &parent_workspace_id)
                .unwrap()
                .status,
            WorkspaceStatus::Frozen
        );

        let child_ids = group.child_workspace_ids.clone();
        for child_id in child_ids {
            release_dynamic_workspace_best_effort(&ctx, &mut graph, &child_id);
        }
        release_dynamic_workspace_best_effort(&ctx, &mut graph, &parent_workspace_id);
    }

    #[test]
    fn loading_dynamic_graph_releases_stale_workspace_from_closed_group() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let child_node_id = "closed-child";
        let group_id = "closed-group";
        let mut graph =
            test_dynamic_graph_at(repo_root.clone(), vec![test_worktree_node(child_node_id)]);
        let child_workspace_id =
            fork_dynamic_workspace(&ctx, &mut graph, "workspace-main", group_id, child_node_id)
                .unwrap();
        graph.nodes[0].workspace_id = child_workspace_id.clone();
        let mut group = test_group_state(
            group_id,
            child_node_id,
            vec![child_node_id],
            vec![child_node_id],
        );
        group.status = DynamicGroupStatus::Closed;
        group.child_workspace_ids = vec![child_workspace_id.clone()];
        graph.groups.push(group);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let workspace = dynamic_workspace(&graph, &child_workspace_id)
            .unwrap()
            .clone();
        git(
            &repo_root,
            &["worktree", "remove", "--force", workspace.path.as_str()],
        );
        std::fs::create_dir_all(workspace.path.as_std_path()).unwrap();

        let loaded = load_or_create_dynamic_graph(&ctx).unwrap();

        assert_eq!(
            dynamic_workspace(&loaded, &child_workspace_id)
                .unwrap()
                .status,
            WorkspaceStatus::Released
        );
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(
            dynamic_workspace(&durable, &child_workspace_id)
                .unwrap()
                .status,
            WorkspaceStatus::Released
        );
    }

    #[test]
    fn loading_dynamic_graph_rejects_missing_workspace_from_open_group() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let child_node_id = "open-child";
        let group_id = "open-group";
        let mut graph =
            test_dynamic_graph_at(repo_root.clone(), vec![test_worktree_node(child_node_id)]);
        let child_workspace_id =
            fork_dynamic_workspace(&ctx, &mut graph, "workspace-main", group_id, child_node_id)
                .unwrap();
        graph.nodes[0].workspace_id = child_workspace_id.clone();
        let mut group = test_group_state(
            group_id,
            child_node_id,
            vec![child_node_id],
            vec![child_node_id],
        );
        group.child_workspace_ids = vec![child_workspace_id.clone()];
        graph.groups.push(group);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let workspace = dynamic_workspace(&graph, &child_workspace_id)
            .unwrap()
            .clone();
        git(
            &repo_root,
            &["worktree", "remove", "--force", workspace.path.as_str()],
        );
        std::fs::create_dir_all(workspace.path.as_std_path()).unwrap();

        let error = load_or_create_dynamic_graph(&ctx).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("workspace path is not the expected Git worktree")
        );
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(
            dynamic_workspace(&durable, &child_workspace_id)
                .unwrap()
                .status,
            WorkspaceStatus::Active
        );
    }

    #[test]
    fn concurrent_dynamic_resume_queues_during_scheduler_startup() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let key = dynamic_state_lock_key(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        );
        clear_dynamic_resume_starting_window(&key).unwrap();
        if let Some(coordinator) = DYNAMIC_RESUME_COORDINATOR.get() {
            let mut coordinator = coordinator.lock().unwrap();
            coordinator.drivers.remove(&key);
            coordinator.pending.remove(&key);
            coordinator.inflight.remove(&key);
            coordinator.starting.remove(&key);
        }
        let first = DynamicResumeOverride {
            request_id: "resume-morning".to_string(),
            execution_id: "execution-morning".to_string(),
            node_id: "good-morning".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue morning".to_string(),
            prompt_id: Some("prompt-morning".to_string()),
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };
        let second = DynamicResumeOverride {
            request_id: "resume-night".to_string(),
            execution_id: "execution-night".to_string(),
            node_id: "good-night".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue night".to_string(),
            prompt_id: Some("prompt-night".to_string()),
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };

        let first_dispatch = dispatch_dynamic_resume_override(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            first,
        )
        .unwrap();
        let second_dispatch = dispatch_dynamic_resume_override(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            second,
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel();
        let (_registration, pending) = register_dynamic_resume_channel(&ctx, tx).unwrap();

        assert_eq!(first_dispatch, DynamicResumeDispatch::StartDriver);
        assert_eq!(second_dispatch, DynamicResumeDispatch::QueuedStarting);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].node_id, "good-night");
        clear_dynamic_resume_starting_window(&key).unwrap();
    }

    #[test]
    fn stopping_starting_dynamic_resume_releases_lease_and_allows_retry() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let key = dynamic_state_lock_key(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        );
        clear_test_dynamic_resume_coordinator(&key);
        let (launch_signal, launch_result) = mpsc::channel();
        let resume = test_dynamic_resume_override(
            "resume-stop-before-start",
            "execution-stop-before-start",
            "good-morning",
            Some(launch_signal),
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                resume,
            )
            .unwrap(),
            DynamicResumeDispatch::StartDriver
        );
        assert!(
            dynamic_resume_target_is_active(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                "good-morning",
                "attempt-001",
            )
            .unwrap()
        );

        assert_eq!(
            stop_dynamic_resume_target(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                "good-morning",
                "attempt-001",
            )
            .unwrap(),
            1
        );
        match launch_result.recv_timeout(Duration::from_secs(1)).unwrap() {
            RuntimeContinueLaunch::Failed(info) => {
                assert_eq!(info.code_str(), "runtime.continue-stopped-before-start")
            }
            RuntimeContinueLaunch::Started(_) => panic!("stopped request must not start"),
        }
        {
            let coordinator = DYNAMIC_RESUME_COORDINATOR.get().unwrap().lock().unwrap();
            assert!(!coordinator.pending.contains_key(&key));
            assert!(!coordinator.inflight.contains_key(&key));
            assert!(!coordinator.starting.contains(&key));
        }

        let retry = test_dynamic_resume_override(
            "resume-after-stop",
            "execution-after-stop",
            "good-morning",
            None,
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                retry,
            )
            .unwrap(),
            DynamicResumeDispatch::StartDriver
        );
        clear_test_dynamic_resume_coordinator(&key);
    }

    #[test]
    fn dynamic_resume_launch_timeout_releases_lease_and_allows_retry() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let key = dynamic_state_lock_key(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        );
        clear_test_dynamic_resume_coordinator(&key);
        let (launch_signal, launch_result) = mpsc::channel();
        let resume = test_dynamic_resume_override(
            "resume-timeout",
            "execution-timeout",
            "good-morning",
            Some(launch_signal),
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                resume,
            )
            .unwrap(),
            DynamicResumeDispatch::StartDriver
        );

        let error = wait_for_dynamic_runtime_continue_launch(
            launch_result,
            &key,
            "resume-timeout",
            Duration::from_millis(10),
        )
        .unwrap_err();
        let info = normalize_runtime_error(&error);
        assert_eq!(info.code_str(), "runtime.continue-launch-timeout");
        assert_eq!(info.params["timeoutMs"], 10);
        {
            let coordinator = DYNAMIC_RESUME_COORDINATOR.get().unwrap().lock().unwrap();
            assert!(!coordinator.pending.contains_key(&key));
            assert!(!coordinator.inflight.contains_key(&key));
            assert!(!coordinator.starting.contains(&key));
        }

        let retry = test_dynamic_resume_override(
            "resume-after-timeout",
            "execution-after-timeout",
            "good-morning",
            None,
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                retry,
            )
            .unwrap(),
            DynamicResumeDispatch::StartDriver
        );
        clear_test_dynamic_resume_coordinator(&key);
    }

    #[test]
    fn paused_dynamic_resume_rearms_outer_runtime_before_driver_commit() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_attempt(
            &app,
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
        );
        let outer_node_path = app.paths.node_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let mut durable_outer_node: NodeState = read_json(&outer_node_path).unwrap();
        durable_outer_node.runtime_execution_id = Some("outer-execution-stopped".to_string());
        write_node_state(&outer_node_path, &durable_outer_node).unwrap();

        let resume = test_dynamic_resume_override(
            "resume-rearm-outer",
            "leaf-execution-new",
            "good-morning",
            None,
        );
        let key = reserve_dynamic_resume_request(
            &app.paths.repo_root,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            DynamicResumeLease::from(&resume),
        )
        .unwrap();
        let mut run = app.run_status("task-006", "run-001").unwrap();
        let mut round: RoundState =
            read_json(&app.paths.round_file("task-006", "run-001", "round-001")).unwrap();
        let mut outer_node: NodeState = read_json(&outer_node_path).unwrap();
        let mut runtime_candidate = None;

        rearm_dynamic_outer_runtime_for_resume(
            &app,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "resume-rearm-outer",
            "outer-execution-new",
            &mut run,
            &mut round,
            &mut outer_node,
            &mut runtime_candidate,
        )
        .unwrap();

        let durable_run = app.run_status("task-006", "run-001").unwrap();
        let durable_outer_node: NodeState = read_json(&outer_node_path).unwrap();
        assert_eq!(durable_run.status, RunStatus::Running);
        assert_eq!(
            durable_run.execution.phase,
            RuntimeExecutionPhase::StartingNode
        );
        assert_eq!(
            durable_outer_node.runtime_execution_id.as_deref(),
            Some("outer-execution-new")
        );

        outer_node.status = RunStatus::Running;
        outer_node.finished_at = None;
        assert!(
            persist_runtime_state_if_execution_current(
                &app,
                "task-006",
                &mut run,
                &round,
                &outer_node,
            )
            .unwrap(),
            "the driver must cross the execution fence after outer re-arm"
        );
        clear_test_dynamic_resume_coordinator(&key);
    }

    #[test]
    fn revoked_dynamic_resume_cannot_rearm_paused_outer_runtime() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_attempt(
            &app,
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
        );
        let outer_node_path = app.paths.node_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let mut durable_outer_node: NodeState = read_json(&outer_node_path).unwrap();
        durable_outer_node.runtime_execution_id = Some("outer-execution-stopped".to_string());
        write_node_state(&outer_node_path, &durable_outer_node).unwrap();
        let resume = test_dynamic_resume_override(
            "resume-revoked-before-outer-rearm",
            "leaf-execution-new",
            "good-morning",
            None,
        );
        let key = reserve_dynamic_resume_request(
            &app.paths.repo_root,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            DynamicResumeLease::from(&resume),
        )
        .unwrap();
        fail_dynamic_resume_requests(
            &key,
            |request| request.request_id == "resume-revoked-before-outer-rearm",
            dynamic_runtime_continue_timeout_info(
                "resume-revoked-before-outer-rearm",
                Duration::from_secs(60),
            ),
        )
        .unwrap();
        let mut run = app.run_status("task-006", "run-001").unwrap();
        let mut round: RoundState =
            read_json(&app.paths.round_file("task-006", "run-001", "round-001")).unwrap();
        let mut outer_node: NodeState = read_json(&outer_node_path).unwrap();
        let mut runtime_candidate = None;

        let error = rearm_dynamic_outer_runtime_for_resume(
            &app,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            "resume-revoked-before-outer-rearm",
            "outer-execution-new",
            &mut run,
            &mut round,
            &mut outer_node,
            &mut runtime_candidate,
        )
        .unwrap_err();
        assert_eq!(
            normalize_runtime_error(&error).code_str(),
            "runtime.continue-superseded"
        );
        assert_eq!(
            app.run_status("task-006", "run-001").unwrap().status,
            RunStatus::Paused
        );
        assert_eq!(
            read_json::<NodeState>(&outer_node_path)
                .unwrap()
                .runtime_execution_id
                .as_deref(),
            Some("outer-execution-stopped")
        );
        clear_test_dynamic_resume_coordinator(&key);
    }

    #[test]
    fn dynamic_resume_driver_exit_fails_every_request_and_clears_coordinator_state() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let key = dynamic_state_lock_key(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        );
        if let Some(coordinator) = DYNAMIC_RESUME_COORDINATOR.get() {
            let mut coordinator = coordinator.lock().unwrap();
            coordinator.drivers.remove(&key);
            coordinator.pending.remove(&key);
            coordinator.inflight.remove(&key);
            coordinator.starting.remove(&key);
        }
        let (morning_signal, morning_launch) = mpsc::channel();
        let morning = test_dynamic_resume_override(
            "resume-morning",
            "execution-morning",
            "good-morning",
            Some(morning_signal),
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                morning,
            )
            .unwrap(),
            DynamicResumeDispatch::StartDriver
        );

        let duplicate = test_dynamic_resume_override(
            "resume-morning-duplicate",
            "execution-morning-duplicate",
            "good-morning",
            None,
        );
        let duplicate_error = dispatch_dynamic_resume_override(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            duplicate,
        )
        .unwrap_err();
        assert_eq!(
            duplicate_error
                .downcast_ref::<crate::runtime_error::RuntimeError>()
                .unwrap()
                .info
                .code_str(),
            "runtime.continue-already-active"
        );

        let (night_signal, night_launch) = mpsc::channel();
        let night = test_dynamic_resume_override(
            "resume-night",
            "execution-night",
            "good-night",
            Some(night_signal),
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &ctx.app.paths.repo_root,
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                night,
            )
            .unwrap(),
            DynamicResumeDispatch::QueuedStarting
        );
        let (driver_tx, _driver_rx) = mpsc::channel();
        let (registration, pending) = register_dynamic_resume_channel(&ctx, driver_tx).unwrap();
        assert_eq!(pending.len(), 1);
        drop(registration);

        for receiver in [morning_launch, night_launch] {
            match receiver.recv_timeout(Duration::from_secs(1)).unwrap() {
                RuntimeContinueLaunch::Failed(info) => {
                    assert_eq!(info.code_str(), "runtime.continue-driver-ended")
                }
                RuntimeContinueLaunch::Started(_) => panic!("request must not report Started"),
            }
        }
        let coordinator = DYNAMIC_RESUME_COORDINATOR.get().unwrap().lock().unwrap();
        assert!(!coordinator.drivers.contains_key(&key));
        assert!(!coordinator.pending.contains_key(&key));
        assert!(!coordinator.inflight.contains_key(&key));
        assert!(!coordinator.starting.contains(&key));
    }

    #[test]
    fn dynamic_resume_routing_is_scoped_by_project_root() {
        let (_first_temp, first_root) = init_repo();
        let (_second_temp, second_root) = init_repo();
        let first_app = App::with_config(first_root, RuntimeConfig::default());
        let second_app = App::with_config(second_root, RuntimeConfig::default());
        let dynamic = test_dynamic();
        let first_ctx = test_context(&first_app, &dynamic);
        let second_ctx = test_context(&second_app, &dynamic);
        let (first_tx, first_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        let (first_registration, _) =
            register_dynamic_resume_channel(&first_ctx, first_tx).unwrap();
        let (second_registration, _) =
            register_dynamic_resume_channel(&second_ctx, second_tx).unwrap();

        let resume = test_dynamic_resume_override(
            "resume-first-project",
            "execution-first-project",
            "good-morning",
            None,
        );
        assert_eq!(
            dispatch_dynamic_resume_override(
                &first_app.paths.repo_root,
                first_ctx.task_id,
                first_ctx.run_id,
                first_ctx.round_id,
                first_ctx.outer_node_id,
                first_ctx.outer_attempt_id,
                resume,
            )
            .unwrap(),
            DynamicResumeDispatch::Sent
        );
        assert_eq!(
            first_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .request_id,
            "resume-first-project"
        );
        assert!(matches!(
            second_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        let first_key = dynamic_state_lock_key(
            &first_app.paths.repo_root,
            first_ctx.task_id,
            first_ctx.run_id,
            first_ctx.round_id,
            first_ctx.outer_node_id,
            first_ctx.outer_attempt_id,
        );
        let _ = complete_dynamic_resume_request(&first_key, "resume-first-project", None);
        drop(first_registration);
        drop(second_registration);
    }

    #[test]
    fn dynamic_inner_resume_only_rearms_target_node() {
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.pause_reason = Some(PauseReason::ProcessInterrupted);
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut other = test_worktree_node("other");
        other.status = DynamicNodeStatus::Paused;
        other.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = DynamicGraphState {
            version: CURRENT_DYNAMIC_GRAPH_VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-001".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "ai-dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: DynamicRunStatus::Paused,
                phase: DynamicRunPhase::Executing,
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
            workspaces: vec![test_workspace(Utf8PathBuf::from("."))],
            proposals: Vec::new(),
        };
        let resume = DynamicResumeOverride {
            request_id: "resume-target".to_string(),
            execution_id: "execution-target".to_string(),
            node_id: "target".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
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
        target.pause_reason = Some(PauseReason::ProcessInterrupted);
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
        write_test_outer_attempt(
            &app,
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
        );
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
        assert!(recover_legacy_cancelled_dynamic_leaves_for_paused_graph(
            &app,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            &mut graph,
        ));
        let resume = DynamicResumeOverride {
            request_id: "resume-bootstrap".to_string(),
            execution_id: "execution-bootstrap".to_string(),
            node_id: "bootstrap".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };
        resume_paused_dynamic_graph(&mut graph, Some(&resume)).unwrap();
        persist_dynamic_graph_for_resume(
            &app,
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            &graph,
        )
        .unwrap();

        let persisted = read_json::<DynamicGraphState>(&app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        ))
        .unwrap();
        assert_eq!(persisted.run.status, DynamicRunStatus::Running);
        assert_eq!(persisted.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(persisted.nodes[0].finished_at, None);
        assert_eq!(
            persisted.run.current_node_ids,
            vec!["bootstrap".to_string()]
        );
    }

    #[test]
    fn dynamic_inner_resume_does_not_pause_cancelled_running_sibling() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.pause_reason = Some(PauseReason::ProcessInterrupted);
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
            request_id: "resume-target".to_string(),
            execution_id: "execution-target".to_string(),
            node_id: "target".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };

        assert!(!recover_legacy_cancelled_dynamic_leaves_for_paused_graph(
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
        assert_eq!(
            graph.run.current_node_ids,
            vec!["target".to_string(), "sibling".to_string()]
        );
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Running);
        assert_eq!(graph.nodes[1].outcome, None);
    }

    #[test]
    fn dynamic_inner_resume_running_graph_rearms_only_target_node() {
        let mut target = test_worktree_node("target");
        target.status = DynamicNodeStatus::Paused;
        target.pause_reason = Some(PauseReason::ProcessInterrupted);
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut other = test_worktree_node("other");
        other.status = DynamicNodeStatus::Running;
        let mut graph = test_dynamic_graph(vec![target, other]);
        graph.run.phase = DynamicRunPhase::PreparingWorkspace;
        let resume = DynamicResumeOverride {
            request_id: "resume-target".to_string(),
            execution_id: "execution-target".to_string(),
            node_id: "target".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };

        resume_paused_dynamic_graph(&mut graph, Some(&resume)).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.phase, DynamicRunPhase::Executing);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(
            graph.run.current_node_ids,
            vec!["target".to_string(), "other".to_string()]
        );
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Running);
    }

    #[test]
    fn dynamic_leaf_interleaved_continue_keeps_running_sibling_active() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut morning = test_worktree_node("good-morning");
        morning.status = DynamicNodeStatus::Paused;
        morning.pause_reason = Some(PauseReason::ProcessInterrupted);
        morning.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut night = test_worktree_node("good-night");
        night.status = DynamicNodeStatus::Paused;
        night.pause_reason = Some(PauseReason::ProcessInterrupted);
        night.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph(vec![morning, night]);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.pause_reason = Some(PauseReason::ProcessInterrupted);
        graph.run.current_node_ids.clear();
        persist_dynamic_graph_for_resume(
            &app,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &graph,
        )
        .unwrap();

        let morning_resume = DynamicResumeOverride {
            request_id: "resume-morning".to_string(),
            execution_id: "execution-morning".to_string(),
            node_id: "good-morning".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue morning".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };
        resume_paused_dynamic_graph(&mut graph, Some(&morning_resume)).unwrap();
        graph.nodes[0].status = DynamicNodeStatus::Running;
        refresh_dynamic_current_leaf_ids(&mut graph);
        persist_dynamic_graph_for_resume(
            &app,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &graph,
        )
        .unwrap();
        write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    ctx.task_id,
                    ctx.run_id,
                    ctx.round_id,
                    ctx.outer_node_id,
                    ctx.outer_attempt_id,
                    "good-morning",
                    "attempt-001",
                )
                .join("acp.session.json"),
            &serde_json::json!({
                "status": "cancelled",
                "stopReason": "cancelled",
                "sessionId": "session-morning"
            }),
        )
        .unwrap();

        let night_resume = DynamicResumeOverride {
            request_id: "resume-night".to_string(),
            execution_id: "execution-night".to_string(),
            node_id: "good-night".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue night".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };
        resume_paused_dynamic_graph(&mut graph, Some(&night_resume)).unwrap();
        persist_dynamic_graph_for_resume(
            &app,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            &graph,
        )
        .unwrap();
        let persisted: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();

        assert_eq!(persisted.run.status, DynamicRunStatus::Running);
        assert_eq!(persisted.run.pause_reason, None);
        assert_eq!(persisted.nodes[0].status, DynamicNodeStatus::Running);
        assert_eq!(persisted.nodes[1].status, DynamicNodeStatus::Ready);
        assert_eq!(
            persisted.run.current_node_ids,
            vec!["good-morning".to_string(), "good-night".to_string()]
        );
    }

    #[test]
    fn parallel_leaf_finalizing_does_not_block_sibling_start() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        app.transition_runtime_execution_phase(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
            RuntimeExecutionPhase::RunningNode,
        )
        .unwrap();
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut finalizing = test_worktree_node("finalizing-worker");
        finalizing.status = DynamicNodeStatus::Running;
        finalizing
            .begin_runtime_execution("execution-finalizing", "2026-06-16T00:00:01Z".to_string());
        let sibling = test_worktree_node("starting-worker");
        let mut graph = test_dynamic_graph(vec![finalizing, sibling]);
        refresh_dynamic_current_leaf_ids(&mut graph);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        transition_dynamic_leaf_runtime_phase(
            &ctx,
            "finalizing-worker",
            "attempt-001",
            "execution-finalizing",
            RuntimeExecutionPhase::RunningNode,
        )
        .unwrap();
        transition_dynamic_leaf_runtime_phase(
            &ctx,
            "finalizing-worker",
            "attempt-001",
            "execution-finalizing",
            RuntimeExecutionPhase::FinalizingArtifact,
        )
        .unwrap();
        graph = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        let mut launch_resumes = Vec::new();
        let (started, _) =
            persist_dynamic_node_running_and_ack(&ctx, &mut graph, 1, &mut launch_resumes)
                .unwrap()
                .unwrap();

        let outer = app.run_status(ctx.task_id, ctx.run_id).unwrap();
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(outer.execution.phase, RuntimeExecutionPhase::RunningNode);
        assert_eq!(
            outer.execution.locator.as_ref().map(|locator| (
                locator.node_id.as_str(),
                locator.attempt_id.as_str(),
                locator.outer_node_id.as_deref(),
            )),
            Some(("ai-dynamic", "attempt-001", None))
        );
        assert_eq!(
            durable.nodes[0].runtime_execution_phase,
            Some(RuntimeExecutionPhase::FinalizingArtifact)
        );
        assert_eq!(started.id, "starting-worker");
        assert_eq!(
            durable.nodes[1].runtime_execution_phase,
            Some(RuntimeExecutionPhase::StartingNode)
        );
    }

    #[test]
    fn stale_leaf_phase_callback_cannot_override_new_execution() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut current = test_worktree_node("good-morning");
        current.status = DynamicNodeStatus::Running;
        current.begin_runtime_execution("execution-b", "2026-06-16T00:00:01Z".to_string());
        current
            .transition_runtime_execution(
                "execution-b",
                RuntimeExecutionPhase::RunningNode,
                "2026-06-16T00:00:02Z".to_string(),
            )
            .unwrap();
        let expected_revision = current.runtime_lifecycle_revision;
        let mut graph = test_dynamic_graph(vec![current]);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let error = transition_dynamic_leaf_runtime_phase(
            &ctx,
            "good-morning",
            "attempt-001",
            "execution-a",
            RuntimeExecutionPhase::FinalizingArtifact,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("superseded by a newer execution")
        );
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(
            durable.nodes[0].runtime_execution_id.as_deref(),
            Some("execution-b")
        );
        assert_eq!(
            durable.nodes[0].runtime_execution_phase,
            Some(RuntimeExecutionPhase::RunningNode)
        );
        assert_eq!(
            durable.nodes[0].runtime_lifecycle_revision,
            expected_revision
        );
    }

    #[test]
    fn dynamic_resume_reports_started_only_after_running_state_is_durable() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut target = test_worktree_node("good-morning");
        target.status = DynamicNodeStatus::Paused;
        target.pause_reason = Some(PauseReason::ProcessInterrupted);
        target.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut sibling = test_worktree_node("good-night");
        sibling.status = DynamicNodeStatus::Running;
        sibling.begin_runtime_execution("execution-night", "2026-06-16T00:00:01Z".to_string());
        let mut graph = test_dynamic_graph(vec![target, sibling]);
        refresh_dynamic_current_leaf_ids(&mut graph);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let (launch_signal, launch_result) = mpsc::channel();
        let resume = test_dynamic_resume_override(
            "resume-morning",
            "execution-morning",
            "good-morning",
            Some(launch_signal),
        );
        reserve_dynamic_resume_request(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            DynamicResumeLease::from(&resume),
        )
        .unwrap();
        let mut pending = vec![resume];
        let mut launch_resumes =
            apply_dynamic_resume_overrides(&ctx, &mut graph, &mut pending).unwrap();
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert!(matches!(
            launch_result.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        let (running_node, _) =
            persist_dynamic_node_running_and_ack(&ctx, &mut graph, 0, &mut launch_resumes)
                .unwrap()
                .unwrap();
        assert_eq!(running_node.status, DynamicNodeStatus::Running);
        match launch_result.recv_timeout(Duration::from_secs(1)).unwrap() {
            RuntimeContinueLaunch::Started(_) => {}
            RuntimeContinueLaunch::Failed(info) => {
                panic!("running launch unexpectedly failed: {}", info.code_str())
            }
        }
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(durable.nodes[0].status, DynamicNodeStatus::Running);
        assert_eq!(
            durable.nodes[0].runtime_execution_id.as_deref(),
            Some("execution-morning")
        );
    }

    #[test]
    fn revoked_dynamic_resume_cannot_cross_final_leaf_spawn_gate() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut target = test_worktree_node("good-morning");
        target.status = DynamicNodeStatus::Ready;
        target.begin_runtime_execution("execution-morning", "2026-06-16T00:00:01Z".to_string());
        let mut sibling = test_worktree_node("good-night");
        sibling.status = DynamicNodeStatus::Running;
        sibling.begin_runtime_execution("execution-night", "2026-06-16T00:00:01Z".to_string());
        let mut graph = test_dynamic_graph(vec![target, sibling]);
        refresh_dynamic_current_leaf_ids(&mut graph);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        let (launch_signal, launch_result) = mpsc::channel();
        let resume = test_dynamic_resume_override(
            "resume-revoked-at-commit",
            "execution-morning",
            "good-morning",
            Some(launch_signal),
        );
        let key = reserve_dynamic_resume_request(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            DynamicResumeLease::from(&resume),
        )
        .unwrap();
        let timeout_info = dynamic_runtime_continue_timeout_info(
            "resume-revoked-at-commit",
            Duration::from_secs(60),
        );
        assert_eq!(
            fail_dynamic_resume_requests(
                &key,
                |request| request.request_id == "resume-revoked-at-commit",
                timeout_info,
            )
            .unwrap()
            .len(),
            1
        );
        assert!(matches!(
            launch_result.recv_timeout(Duration::from_secs(1)).unwrap(),
            RuntimeContinueLaunch::Failed(_)
        ));

        let mut launch_resumes = vec![resume];
        assert!(
            persist_dynamic_node_running_and_ack(&ctx, &mut graph, 0, &mut launch_resumes,)
                .unwrap()
                .is_none(),
            "revoked launch must not return a node for thread::spawn"
        );
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(durable.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(
            durable.nodes[0].pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert_eq!(durable.nodes[1].status, DynamicNodeStatus::Running);
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
                execution_id: None,
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
        assert_eq!(
            graph.nodes[0].pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert_eq!(graph.nodes[1].status, DynamicNodeStatus::Running);
    }

    #[test]
    fn stale_dynamic_execution_message_cannot_override_new_execution() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut current = test_worktree_node("good-morning");
        current.status = DynamicNodeStatus::Running;
        current.begin_runtime_execution("execution-b", "2026-06-16T00:00:01Z".to_string());
        let mut graph = test_dynamic_graph(vec![current]);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                execution_id: Some("execution-a".to_string()),
                result: Err(recoverable_runtime_error("late execution-a failure")),
            },
        )
        .unwrap();

        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Running);
        assert_eq!(
            graph.nodes[0].runtime_execution_id.as_deref(),
            Some("execution-b")
        );
        assert_eq!(
            graph.nodes[0].runtime_execution_phase,
            Some(RuntimeExecutionPhase::StartingNode)
        );
        let durable: DynamicGraphState = read_json(&app.paths.dynamic_graph_file(
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
        ))
        .unwrap();
        assert_eq!(durable.nodes[0].status, DynamicNodeStatus::Running);
        assert_eq!(
            durable.nodes[0].runtime_execution_id.as_deref(),
            Some("execution-b")
        );
    }

    #[test]
    fn dynamic_runtime_error_is_owned_by_leaf_and_converges_to_graph() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut failed = test_worktree_node("good-morning");
        failed.status = DynamicNodeStatus::Running;
        let mut sibling = test_worktree_node("good-night");
        sibling.status = DynamicNodeStatus::Running;
        let mut graph = test_dynamic_graph(vec![failed, sibling]);

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                execution_id: None,
                result: Err(recoverable_runtime_error(
                    "session/set_config_option: failed to persist config.toml",
                )
                .context("provider `codex-acp` failed to run `good-morning`")),
            },
        )
        .unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(graph.run.current_node_ids, vec!["good-night".to_string()]);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(
            graph.nodes[0].pause_reason,
            Some(PauseReason::RuntimeAbnormal)
        );
        let runtime_error = graph.nodes[0].runtime_error.as_ref().unwrap();
        assert_eq!(runtime_error.code_str(), "runtime.recoverable");
        assert!(runtime_error.diagnostic.contains("provider `codex-acp`"));
        assert!(
            runtime_error
                .diagnostic
                .contains("session/set_config_option")
        );

        graph.nodes[1].status = DynamicNodeStatus::Completed;
        graph.nodes[1].outcome = Some(NodeOutcome::Success);
        graph.nodes[1].finished_at = Some(now_rfc3339_like());
        refresh_dynamic_current_leaf_ids(&mut graph);
        persist_dynamic_graph(&ctx, &mut graph).unwrap();
        drive_dynamic_graph(&ctx, &mut graph).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(graph.run.pause_reason, Some(PauseReason::RuntimeAbnormal));
    }

    #[test]
    fn rearming_dynamic_leaf_clears_only_its_pause_details() {
        let mut first = test_worktree_node("good-morning");
        mark_dynamic_node_paused(
            &mut first,
            PauseReason::RuntimeAbnormal,
            Some(manual_runtime_error_info(
                RuntimeErrorDomain::Provider,
                "provider.acp-error",
                "first failed",
                serde_json::json!({}),
            )),
        );
        let mut second = test_worktree_node("good-night");
        mark_dynamic_node_paused(&mut second, PauseReason::PermissionRequested, None);
        let mut graph = test_dynamic_graph(vec![first, second]);
        let resume = DynamicResumeOverride {
            request_id: "resume-morning".to_string(),
            execution_id: "execution-morning".to_string(),
            node_id: "good-morning".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: None,
        };

        rearm_dynamic_resume_target(&mut graph, &resume).unwrap();

        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Ready);
        assert_eq!(graph.nodes[0].pause_reason, None);
        assert_eq!(graph.nodes[0].runtime_error, None);
        assert_eq!(
            graph.nodes[1].pause_reason,
            Some(PauseReason::PermissionRequested)
        );
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
            runtime_error: None,
        };

        let candidate = interrupted_dynamic_output_artifact_candidate(&result);
        finalize_dynamic_worker_result(&ctx, &mut node, &attempt_id, result).unwrap();
        let proposal = try_accept_interrupted_dynamic_completion(
            &ctx,
            &mut node,
            &attempt_id,
            candidate.as_ref(),
        )
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
    fn interrupted_dynamic_worker_with_invalid_completion_stays_paused_without_artifact() {
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
                    content: "我会继续当前会话，在 `.claude` 下补上独立的 good bye Python 类并写开发报告。"
                        .to_string(),
                }),
            }),
            worker_ref_seed: None,
            stream_path: None,
            runtime_error: None,
        };

        let candidate = interrupted_dynamic_output_artifact_candidate(&result);
        finalize_dynamic_worker_result(&ctx, &mut node, &attempt_id, result).unwrap();
        let proposal = try_accept_interrupted_dynamic_completion(
            &ctx,
            &mut node,
            &attempt_id,
            candidate.as_ref(),
        )
        .unwrap();

        assert!(proposal.is_none());
        assert_eq!(node.status, DynamicNodeStatus::Paused);
        assert_eq!(node.outcome, None);
        assert!(
            !app.paths
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
    }

    #[test]
    fn dynamic_completed_result_cannot_cross_process_interrupted_stop_boundary() {
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
        let mut graph = test_dynamic_graph_at(
            app.paths.repo_root.clone(),
            vec![test_worktree_node("good-morning")],
        );
        let dynamic_run_id = graph.run.id.clone();

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                execution_id: None,
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
        assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[0].outcome, None);
        assert_eq!(graph.nodes[0].pause_reason, run.pause_reason);
        assert!(graph.proposals.is_empty());
    }

    #[test]
    fn dynamic_completed_result_cannot_cross_runtime_abnormal_boundary() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_attempt(&app, RunStatus::Paused, Some(PauseReason::RuntimeAbnormal));
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut completed = test_worktree_node("good-morning");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph_at(
            app.paths.repo_root.clone(),
            vec![test_worktree_node("good-morning")],
        );
        let dynamic_run_id = graph.run.id.clone();

        apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-morning".to_string(),
                execution_id: None,
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
        assert_eq!(run.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[0].outcome, None);
        assert_eq!(graph.nodes[0].pause_reason, run.pause_reason);
        assert!(graph.proposals.is_empty());
    }

    #[test]
    fn dynamic_resume_reconciles_existing_valid_completion_before_launch() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        // The continue command claims the outer Runtime before reconciliation;
        // the dynamic leaf itself remains paused until its durable completion
        // is accepted below.
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut paused = test_worktree_node("bootstrap");
        paused.status = DynamicNodeStatus::Paused;
        paused.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph_at(app.paths.repo_root.clone(), vec![paused]);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        graph.run.current_node_ids = vec!["bootstrap".to_string()];
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
        write_dynamic_completion_artifact(&app, "bootstrap", test_end_completion("already done"));
        let (launch_signal, launch_result) = mpsc::channel();
        let resume = DynamicResumeOverride {
            request_id: "resume-bootstrap".to_string(),
            execution_id: "execution-bootstrap".to_string(),
            node_id: "bootstrap".to_string(),
            attempt_id: "attempt-001".to_string(),
            prompt: "continue".to_string(),
            prompt_id: None,
            prompt_display: None,
            user_prompt_render_mode: UserPromptRenderMode::UserMessage,
            prompt_visibility: PromptVisibility::Visible,
            runtime_control_intent: RuntimeControlIntent::Resume,
            attachment_paths: Vec::new(),
            model_override: None,
            permission_mode_override: None,
            launch_signal: Some(launch_signal),
        };
        reserve_dynamic_resume_request(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            DynamicResumeLease::from(&resume),
        )
        .unwrap();

        assert!(try_reconcile_dynamic_resume_completion(&ctx, &mut graph, &resume).unwrap());
        assert!(matches!(
            launch_result.recv_timeout(Duration::from_secs(1)).unwrap(),
            RuntimeContinueLaunch::Started(_)
        ));

        assert_eq!(graph.run.status, DynamicRunStatus::Running);
        assert_eq!(graph.run.pause_reason, None);
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Completed);
        assert_eq!(graph.nodes[0].outcome, Some(NodeOutcome::Success));
        assert_eq!(graph.proposals.len(), 1);
    }

    #[test]
    fn revoked_dynamic_resume_cannot_reconcile_existing_completion() {
        let (_temp, repo_root) = init_repo();
        let app = App::with_config(repo_root, RuntimeConfig::default());
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut paused = test_worktree_node("bootstrap");
        paused.status = DynamicNodeStatus::Paused;
        paused.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph_at(app.paths.repo_root.clone(), vec![paused]);
        graph.run.status = DynamicRunStatus::Paused;
        graph.run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        graph.run.current_node_ids = vec!["bootstrap".to_string()];
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
        write_dynamic_completion_artifact(&app, "bootstrap", test_end_completion("already done"));
        let (launch_signal, launch_result) = mpsc::channel();
        let resume = test_dynamic_resume_override(
            "resume-bootstrap-revoked",
            "execution-bootstrap",
            "bootstrap",
            Some(launch_signal),
        );
        let key = reserve_dynamic_resume_request(
            &ctx.app.paths.repo_root,
            ctx.task_id,
            ctx.run_id,
            ctx.round_id,
            ctx.outer_node_id,
            ctx.outer_attempt_id,
            DynamicResumeLease::from(&resume),
        )
        .unwrap();
        let timeout = dynamic_runtime_continue_timeout_info(
            &resume.request_id,
            DYNAMIC_RUNTIME_CONTINUE_LAUNCH_TIMEOUT,
        );
        assert_eq!(
            fail_dynamic_resume_requests(
                &key,
                |request| request.request_id == resume.request_id,
                timeout,
            )
            .unwrap()
            .len(),
            1
        );
        assert!(matches!(
            launch_result.recv_timeout(Duration::from_secs(1)).unwrap(),
            RuntimeContinueLaunch::Failed(info)
                if info.code_str() == "runtime.continue-launch-timeout"
        ));

        let error = try_reconcile_dynamic_resume_completion(&ctx, &mut graph, &resume).unwrap_err();
        assert_eq!(
            normalize_runtime_error(&error).code_str(),
            "runtime.continue-superseded"
        );
        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(graph.run.pause_reason, Some(PauseReason::RuntimeAbnormal));
        assert_eq!(graph.nodes[0].status, DynamicNodeStatus::Paused);
        assert_eq!(graph.nodes[0].outcome, None);
        assert!(graph.proposals.is_empty());
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
                execution_id: None,
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
    fn dynamic_graph_completion_emits_leaf_refresh_after_completed_graph_is_durable() {
        let (_temp, repo_root) = init_repo();
        let base_app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let graph_path = base_app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let graph_path_for_callback = graph_path.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_callback = seen.clone();
        let app = base_app.with_acp_session_update(Arc::new(move |context| {
            let persisted: DynamicGraphState = read_json(&graph_path_for_callback)?;
            let revision = persisted
                .nodes
                .last()
                .map(|node| node.runtime_lifecycle_revision)
                .unwrap_or_default();
            seen_for_callback
                .lock()
                .unwrap()
                .push((context, persisted.run.status, revision));
            Ok(())
        }));
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut completed = test_worktree_node("goodbye-worker");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let mut graph = test_dynamic_graph_at(repo_root, vec![completed]);
        graph.run.current_node_ids.clear();

        complete_dynamic_graph_success(&ctx, &mut graph).unwrap();

        let persisted: DynamicGraphState = read_json(&graph_path).unwrap();
        assert_eq!(persisted.run.status, DynamicRunStatus::Completed);
        assert_eq!(persisted.run.outcome, Some(RunOutcome::Success));
        assert_eq!(persisted.nodes[0].runtime_lifecycle_revision, 1);
        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.node_id, "goodbye-worker");
        assert_eq!(calls[0].0.attempt_id, "attempt-001");
        assert_eq!(calls[0].0.outer_node_id.as_deref(), Some("ai-dynamic"));
        assert_eq!(calls[0].0.outer_attempt_id.as_deref(), Some("attempt-001"));
        assert_eq!(calls[0].1, DynamicRunStatus::Completed);
        assert_eq!(calls[0].2, 1);
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
                execution_id: None,
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
                execution_id: None,
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
        let base_app = App::with_config(repo_root.clone(), RuntimeConfig::default());
        let graph_path = base_app.paths.dynamic_graph_file(
            "task-006",
            "run-001",
            "round-001",
            "ai-dynamic",
            "attempt-001",
        );
        let callback_graph_path = graph_path.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let callback_seen = seen.clone();
        let app = base_app.with_acp_session_update(Arc::new(move |context| {
            let persisted: DynamicGraphState = read_json(&callback_graph_path)?;
            let revision = persisted
                .nodes
                .iter()
                .find(|node| node.id == context.node_id)
                .map(|node| node.runtime_lifecycle_revision)
                .unwrap_or_default();
            callback_seen
                .lock()
                .unwrap()
                .push((context, persisted.run.status, revision));
            Ok(())
        }));
        write_test_outer_run(&app);
        let dynamic = test_dynamic();
        let ctx = test_context(&app, &dynamic);
        let mut paused = test_worktree_node("good-morning");
        paused.status = DynamicNodeStatus::Paused;
        paused.finished_at = Some("2026-06-16T00:00:00Z".to_string());
        let paused_revision = paused.runtime_lifecycle_revision;
        let mut completed = test_worktree_node("goodbye-worker");
        completed.status = DynamicNodeStatus::Completed;
        completed.outcome = Some(NodeOutcome::Success);
        completed.finished_at = Some("2026-06-16T00:00:01Z".to_string());
        completed.complete_runtime_execution("2026-06-16T00:00:01Z");
        let completed_revision = completed.runtime_lifecycle_revision;
        let mut graph = test_dynamic_graph_at(repo_root, vec![paused, completed]);
        graph.run.current_node_ids.clear();
        persist_dynamic_graph(&ctx, &mut graph).unwrap();

        drive_dynamic_graph(&ctx, &mut graph).unwrap();

        assert_eq!(graph.run.status, DynamicRunStatus::Paused);
        assert_eq!(
            graph.run.pause_reason,
            Some(PauseReason::ProcessInterrupted)
        );
        assert!(graph.run.current_node_ids.is_empty());
        let persisted: DynamicGraphState = read_json(&graph_path).unwrap();
        assert_eq!(persisted.run.status, DynamicRunStatus::Paused);
        assert_eq!(
            persisted.nodes[0].runtime_lifecycle_revision,
            paused_revision
        );
        assert_eq!(
            persisted.nodes[1].runtime_lifecycle_revision,
            completed_revision + 1
        );
        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.node_id, "goodbye-worker");
        assert_eq!(calls[0].0.attempt_id, "attempt-001");
        assert_eq!(calls[0].0.outer_node_id.as_deref(), Some("ai-dynamic"));
        assert_eq!(calls[0].0.outer_attempt_id.as_deref(), Some("attempt-001"));
        assert_eq!(calls[0].1, DynamicRunStatus::Paused);
        assert_eq!(calls[0].2, completed_revision + 1);
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
            version: CURRENT_DYNAMIC_GRAPH_VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-001".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "ai-dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: DynamicRunStatus::Running,
                phase: DynamicRunPhase::Executing,
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
            workspaces: vec![test_workspace(app.paths.repo_root.clone())],
            proposals: Vec::new(),
        };

        let error = apply_dynamic_execution_message(
            &ctx,
            &mut graph,
            DynamicExecutionMessage {
                node_id: "good-night".to_string(),
                execution_id: None,
                result: Err(blocked_runtime_error(
                    "failed to create dynamic worktree for `good-night`",
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
        assert_eq!(graph.nodes[0].pause_reason, Some(PauseReason::ErrorBlocked));
        assert_eq!(
            graph.nodes[0]
                .runtime_error
                .as_ref()
                .map(|error| error.code_str()),
            Some("dynamic.blocked")
        );
        assert!(graph.nodes[0].finished_at.is_some());
    }
}
