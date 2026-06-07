use gold_band::acp::client;
use gold_band::acp::events::{AcpUiEvent, append_ui_event, current_timestamp, latest_timeline_source_seq, load_timeline_items, permission_decision_event, write_timeline_items};
use gold_band::acp::permission::{
    cancel_pending_permission_requests, request_cancel, write_permission_response,
};
use gold_band::app::{
    CreateTaskInput, ProfileCommandError, ProfileEntry, ProfileInput, ProfileList, WorkflowTemplateStore,
};
use gold_band::domain::{NodeOutcome, PauseReason, SessionMode};
use gold_band::dsl::{NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::provider::supported_modes_from_capabilities;
use gold_band::runtime::{NodeState, WorkerRefState};
use gold_band::storage::read_json;
use gold_band::storage::sqlite::{self, AttemptIndexContext};
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader},
    str::FromStr,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use camino::Utf8PathBuf;
use gold_band::config::{
    AcpAdapterConfig, DesktopFontPreference, DesktopLanguage, DesktopThemePreference,
    ManagedAgentConfig, ManagedAgentType,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::i18n::Translator;
use crate::state::{DesktopState, UpdateBadgeSeenTarget};
use crate::updater::{
    UpdateStatusVm, UpdaterSettingsVm, check_update,
    download_and_install_update as run_download_and_install_update, normalize_updater_url_override,
    updater_settings,
};
use crate::view_models::{
    AcpRawFramePageVm, AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AgentRegistryVm,
    AppBootstrapVm, ContentVm, LocalClaudeStatusVm, LogPageVm, LogQueryInput, PreferencesVm, RoundDetailVm,
    RoundSelectionInput, RunDetailVm, RunSummaryVm, TaskDetailVm, TaskListVm, UpdateBadgeStateVm,
    WorkflowVm, acp_raw_frame_page_vm, acp_session_vm, agent_registry_vm,
    bootstrap_vm, dynamic_acp_session_vm, log_page_vm, preferences_vm, round_detail_vm, run_detail_vm,
    run_summary_vm, task_detail_vm, task_list_vm, workflow_vm,
};

const ACP_SESSION_EVENT: &str = "gold-band://acp-session-updated";
const ACP_CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(5);
const ACP_CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub type CommandResult<T> = Result<T, CommandErrorVm>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcpSessionUpdatedEventVm {
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
    event: Option<AcpUiEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorVm {
    pub code: String,
    pub params: serde_json::Value,
}

impl CommandErrorVm {
    fn new(code: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            code: code.into(),
            params,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentInput {
    pub display_name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInputVm {
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub requirement_file_name: Option<String>,
    pub requirement_content: String,
    pub workflow: WorkflowDsl,
    pub workflow_template_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowInputVm {
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowTemplateInputVm {
    pub name: String,
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowTemplateInputVm {
    pub workflow: WorkflowDsl,
}

#[tauri::command]
pub fn get_system_fonts() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let mut families = BTreeSet::new();
    for face in database.faces() {
        for (family, _) in &face.families {
            families.insert(family.clone());
        }
    }
    families.into_iter().collect()
}

#[tauri::command]
pub fn check_local_claude() -> LocalClaudeStatusVm {
    match gold_band::process::find_executable_in_path("claude") {
        Some(path) => LocalClaudeStatusVm {
            found: true,
            path: Some(path.to_string_lossy().into_owned()),
        },
        None => LocalClaudeStatusVm {
            found: false,
            path: None,
        },
    }
}

#[tauri::command]
pub fn get_app_bootstrap(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
) -> CommandResult<AppBootstrapVm> {
    let context = state.context().map_err(command_error)?;
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app_handle.package_info().version.to_string(),
        context.needs_workspace,
    ))
}

#[tauri::command]
pub fn get_agent_registry(state: State<'_, DesktopState>) -> CommandResult<AgentRegistryVm> {
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub fn create_agent(
    state: State<'_, DesktopState>,
    agent_type: String,
    input: ManagedAgentInput,
) -> CommandResult<AgentRegistryVm> {
    let app = state.app().map_err(command_error)?;
    let agent_type = ManagedAgentType::from_str(&agent_type).map_err(command_error)?;
    if app.managed_agents().contains_key(&agent_type) {
        return Err(CommandErrorVm::new(
            "agent.already-exists",
            serde_json::json!({ "agentType": agent_type.as_str() }),
        ));
    }
    let settings = app
        .save_managed_agent(
            agent_type,
            ManagedAgentConfig::new(AcpAdapterConfig {
                command: input.command,
                args: input.args,
                display_name: input.display_name,
                env: input.env,
            }),
        )
        .map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub fn update_agent(
    state: State<'_, DesktopState>,
    agent_type: String,
    input: ManagedAgentInput,
) -> CommandResult<AgentRegistryVm> {
    let app = state.app().map_err(command_error)?;
    let agent_type = ManagedAgentType::from_str(&agent_type).map_err(command_error)?;
    if !app.managed_agents().contains_key(&agent_type) {
        return Err(CommandErrorVm::new(
            "agent.not-configured",
            serde_json::json!({ "agentType": agent_type.as_str() }),
        ));
    }
    let settings = app
        .save_managed_agent(
            agent_type,
            ManagedAgentConfig::new(AcpAdapterConfig {
                command: input.command,
                args: input.args,
                display_name: input.display_name,
                env: input.env,
            }),
        )
        .map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    state
        .clear_agent_diagnostic(agent_type)
        .map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub fn delete_agent(
    state: State<'_, DesktopState>,
    agent_type: String,
) -> CommandResult<AgentRegistryVm> {
    let app = state.app().map_err(command_error)?;
    let agent_type = ManagedAgentType::from_str(&agent_type).map_err(command_error)?;
    let settings = app
        .remove_managed_agent(agent_type)
        .map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub async fn doctor_agent(
    state: State<'_, DesktopState>,
    agent_type: String,
) -> CommandResult<AgentRegistryVm> {
    let agent_type = ManagedAgentType::from_str(&agent_type).map_err(command_error)?;
    state
        .refresh_agent_diagnostic(agent_type)
        .map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub fn get_task_list(state: State<'_, DesktopState>) -> CommandResult<TaskListVm> {
    let app = state.app().map_err(command_error)?;
    task_list_vm(&app).map_err(command_error)
}

#[tauri::command]
pub fn get_profiles(state: State<'_, DesktopState>) -> CommandResult<ProfileList> {
    let app = state.app().map_err(command_error)?;
    app.profiles().map_err(command_error)
}

#[tauri::command]
pub fn get_profile(state: State<'_, DesktopState>, id: String) -> CommandResult<ProfileEntry> {
    let app = state.app().map_err(command_error)?;
    app.profile_show(&id).map_err(command_error)
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, DesktopState>,
    input: ProfileInput,
) -> CommandResult<ProfileEntry> {
    let app = state.app().map_err(command_error)?;
    app.create_profile(input).map_err(command_error)
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, DesktopState>,
    id: String,
    input: ProfileInput,
) -> CommandResult<ProfileEntry> {
    let app = state.app().map_err(command_error)?;
    app.update_profile(&id, input).map_err(command_error)
}

#[tauri::command]
pub fn delete_profile(
    state: State<'_, DesktopState>,
    id: String,
    force: Option<bool>,
) -> CommandResult<ProfileList> {
    let app = state.app().map_err(|error| {
        CommandErrorVm::new(
            "app.unexpected",
            serde_json::json!({
                "message": format!("delete_profile `{}` failed before execution: {:#}", id, error),
            }),
        )
    })?;
    match app.delete_profile(&id, force.unwrap_or(false)) {
        Ok(list) => Ok(list),
        Err(error) => {
            if error.downcast_ref::<ProfileCommandError>().is_some() {
                Err(command_error(error))
            } else {
                Err(CommandErrorVm::new(
                    "app.unexpected",
                    serde_json::json!({
                        "message": format!("delete_profile `{}` failed: {:#}", id, error),
                    }),
                ))
            }
        }
    }
}

#[tauri::command]
pub fn choose_workspace(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> CommandResult<Option<AppBootstrapVm>> {
    let current = state.context().map_err(command_error)?.repo_root;
    let Some(path) = app
        .dialog()
        .file()
        .set_directory(current.as_std_path())
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|_| {
        CommandErrorVm::new("workspace.path-resolve-failed", serde_json::json!({}))
    })?;
    let repo_root = Utf8PathBuf::from_path_buf(path).map_err(|_| {
        CommandErrorVm::new("workspace.path-invalid-utf8", serde_json::json!({}))
    })?;
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    let update_status = state.update_status().map_err(command_error)?;
    Ok(Some(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app.package_info().version.to_string(),
        false,
    )))
}

#[tauri::command]
pub fn select_recent_workspace(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    workspace: String,
) -> CommandResult<AppBootstrapVm> {
    let repo_root = Utf8PathBuf::from(workspace);
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app_handle.package_info().version.to_string(),
        false,
    ))
}

#[tauri::command]
pub fn get_task_detail(
    state: State<'_, DesktopState>,
    task_id: String,
) -> CommandResult<TaskDetailVm> {
    let app = state.app().map_err(command_error)?;
    task_detail_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub fn create_task(
    state: State<'_, DesktopState>,
    input: CreateTaskInputVm,
) -> CommandResult<WorkflowVm> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    let summary = app
        .create_task_from_requirement(CreateTaskInput {
            title: input.title,
            description: input.description,
            requirement_file_name: input.requirement_file_name,
            requirement_content: input.requirement_content,
            workflow: input.workflow,
            workflow_template_id: input.workflow_template_id,
        })
        .map_err(command_error)?;
    let task_id = summary.task.id.clone();
    let task_dir = app.paths.task_dir(&task_id);
    tauri::async_runtime::spawn_blocking(move || {
        sqlite::index_task_with_retry(&task_dir, &task_id);
    });
    workflow_vm(&app, &summary.task.id).map_err(command_error)
}

#[tauri::command]
pub fn save_task_workflow(
    state: State<'_, DesktopState>,
    task_id: String,
    input: SaveWorkflowInputVm,
) -> CommandResult<WorkflowVm> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    app.save_task_workflow(&task_id, input.workflow)
        .map_err(command_error)?;
    workflow_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub fn get_workflow(state: State<'_, DesktopState>, task_id: String) -> CommandResult<WorkflowVm> {
    let app = state.app().map_err(command_error)?;
    workflow_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub fn get_workflow_templates(
    state: State<'_, DesktopState>,
) -> CommandResult<WorkflowTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.workflow_templates().map_err(command_error)
}

#[tauri::command]
pub fn save_workflow_template(
    state: State<'_, DesktopState>,
    input: SaveWorkflowTemplateInputVm,
) -> CommandResult<WorkflowTemplateStore> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    app.save_workflow_template(input.name, input.workflow)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_workflow_template(
    state: State<'_, DesktopState>,
    template_id: String,
    input: UpdateWorkflowTemplateInputVm,
) -> CommandResult<WorkflowTemplateStore> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    app.update_workflow_template(&template_id, input.workflow)
        .map_err(command_error)
}

#[tauri::command]
pub fn delete_workflow_template(
    state: State<'_, DesktopState>,
    template_id: String,
) -> CommandResult<WorkflowTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.delete_workflow_template(&template_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn get_run_detail(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunDetailVm> {
    let app = state.app().map_err(command_error)?;
    run_detail_vm(&app, &task_id, &run_id).map_err(command_error)
}

#[tauri::command]
pub fn get_round_detail(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    selection: Option<RoundSelectionInput>,
) -> CommandResult<RoundDetailVm> {
    let app = state.app().map_err(command_error)?;
    round_detail_vm(&app, &task_id, &run_id, &round_id, selection).map_err(command_error)
}

#[tauri::command]
pub fn start_run(app_handle: AppHandle, state: State<'_, DesktopState>, task_id: String) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app_with_acp_live_update(acp_live_update_emitter(app_handle));
    app.run_start_background(&task_id, None)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn continue_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    prompt_id: Option<String>,
) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app_with_acp_live_update(acp_live_update_emitter(app_handle));
    app.run_continue_background(&task_id, &run_id, prompt_id)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn submit_manual_check(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outcome: String,
) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app_with_acp_live_update(acp_live_update_emitter(app_handle));
    let outcome = match outcome.as_str() {
        "success" => NodeOutcome::Success,
        "failure" => NodeOutcome::Failure,
        _ => {
            return Err(CommandErrorVm::new(
                "manual-check.invalid-outcome",
                serde_json::json!({ "outcome": outcome }),
            ));
        }
    };
    app.submit_manual_check_background(&task_id, &run_id, &round_id, &node_id, &attempt_id, outcome)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn retry_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app_with_acp_live_update(acp_live_update_emitter(app_handle));
    app.run_retry(&task_id, &run_id)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn kill_run(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
    app.run_kill(&task_id, &run_id)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn show_artifact(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) = (&outer_node_id, &outer_attempt_id) {
        let artifact_name = name.strip_suffix(".json").unwrap_or(&name);
        let path = app.paths.dynamic_node_artifact_file(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            artifact_name,
        );
        app.artifact_show_path(&path)
    } else {
        app.artifact_show(&task_id, &run_id, &round_id, &node_id, &attempt_id, &name)
    };
    content
        .map(|content| ContentVm {
            title: labels.format("detail.artifact", &name),
            kind: "artifact".to_string(),
            content,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id, "outerNodeId": outer_node_id, "outerAttemptId": outer_attempt_id }),
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn get_log_page(
    state: State<'_, DesktopState>,
    query: LogQueryInput,
) -> CommandResult<LogPageVm> {
    let app = state.app().map_err(command_error)?;
    log_page_vm(&app, query).map_err(command_error)
}

fn acp_live_update_emitter(app_handle: AppHandle) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext, AcpUiEvent) -> anyhow::Result<()> + Send + Sync> {
    Arc::new(move |context, event| {
        emit_acp_event_update(
            &app_handle,
            &context.task_id,
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
            context.outer_node_id,
            context.outer_attempt_id,
            event,
        );
        Ok(())
    })
}

fn emit_acp_session_update(
    app_handle: &AppHandle,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
) {
    emit_acp_update(app_handle, task_id, run_id, round_id, node_id, attempt_id, outer_node_id, outer_attempt_id, session, None);
}

fn emit_acp_event_update(
    app_handle: &AppHandle,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    event: AcpUiEvent,
) {
    emit_acp_update(app_handle, task_id, run_id, round_id, node_id, attempt_id, outer_node_id, outer_attempt_id, None, Some(event));
}

#[allow(clippy::too_many_arguments)]
fn emit_acp_update(
    app_handle: &AppHandle,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
    event: Option<AcpUiEvent>,
) {
    let _ = app_handle.emit(
        ACP_SESSION_EVENT,
        AcpSessionUpdatedEventVm {
            task_id: task_id.to_string(),
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            outer_node_id,
            outer_attempt_id,
            session,
            event,
        },
    );
}

#[tauri::command]
pub fn get_acp_session(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpSessionQueryInput>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
        return dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            query,
        )
        .map_err(command_error);
    }
    acp_session_vm(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        query,
    )
    .map_err(command_error)
}

#[tauri::command]
pub async fn send_acp_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    let task_id_for_emit = task_id.clone();
    let run_id_for_emit = run_id.clone();
    let round_id_for_emit = round_id.clone();
    let node_id_for_emit = node_id.clone();
    let attempt_id_for_emit = attempt_id.clone();
    let outer_node_id_for_emit = outer_node_id.clone();
    let outer_attempt_id_for_emit = outer_attempt_id.clone();
    let app_handle_for_task = app_handle.clone();
    let session = tauri::async_runtime::spawn_blocking(move || {
        if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (outer_node_id.as_deref(), outer_attempt_id.as_deref())
        {
            let attempt_dir = app.paths.dynamic_node_attempt_dir(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            );
            let worker_ref_path = app.paths.dynamic_node_worker_ref_file(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            );
            let node_path = app.paths.dynamic_node_file(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
            );
            let node = read_json::<gold_band::dynamic::DynamicNodeState>(&node_path)
                .map_err(command_error)?;
            let provider = node.provider.as_deref().ok_or_else(|| {
                CommandErrorVm::new("acp.missing-provider", serde_json::json!({}))
            })?;
            let (_, agent_config) = app.managed_agent(provider).map_err(command_error)?;
            let permission_mode = node.permission_mode.clone();
            let (session_mode, continue_ref) = if worker_ref_path.exists() {
                let worker_ref =
                    read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?;
                (worker_ref.mode, worker_ref.continue_ref)
            } else {
                (SessionMode::New, None)
            };
            let prompt_bundle = app
                .dynamic_acp_prompt_bundle_for_attempt(
                    &task_id,
                    &run_id,
                    &round_id,
                    outer_node_id,
                    outer_attempt_id,
                    &node_id,
                    &attempt_id,
                    prompt,
                    prompt_id.clone(),
                    continue_ref.clone(),
                )
                .map_err(command_error)?;
            let app_handle_for_live = app_handle_for_task.clone();
            let task_id_for_live = task_id.clone();
            let run_id_for_live = run_id.clone();
            let round_id_for_live = round_id.clone();
            let node_id_for_live = node_id.clone();
            let attempt_id_for_live = attempt_id.clone();
            let outer_node_id_for_live = Some(outer_node_id.to_string());
            let outer_attempt_id_for_live = Some(outer_attempt_id.to_string());
            client::run_prompt(
                provider,
                &agent_config.adapter,
                app.paths.repo_root.clone(),
                attempt_dir,
                &prompt_bundle,
                session_mode,
                permission_mode,
                continue_ref,
                app.config.use_local_claude,
                app.config.acp_session_title_refresh_enabled,
                Some(&|event| {
                    emit_acp_event_update(
                        &app_handle_for_live,
                        &task_id_for_live,
                        &run_id_for_live,
                        &round_id_for_live,
                        &node_id_for_live,
                        &attempt_id_for_live,
                        outer_node_id_for_live.clone(),
                        outer_attempt_id_for_live.clone(),
                        event.clone(),
                    );
                    Ok(())
                }),
            )
            .map_err(command_error)?;
            return dynamic_acp_session_vm(
                &app,
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
                None,
            )
            .map_err(command_error);
        }
        let attempt_dir =
            app.paths
                .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let worker_ref_path =
            app.paths
                .worker_ref_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let node_path = app
            .paths
            .node_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let node = read_json::<NodeState>(&node_path).map_err(command_error)?;
        let provider = node
            .resolved_config
            .get("provider")
            .and_then(|value| value.as_str())
            .ok_or_else(|| CommandErrorVm::new("acp.missing-provider", serde_json::json!({})))?;
        let (_, agent_config) = app.managed_agent(provider).map_err(command_error)?;
        let permission_mode = node
            .resolved_config
            .get("permissionMode")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let (session_mode, continue_ref) = if worker_ref_path.exists() {
            let worker_ref =
                read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?;
            (worker_ref.mode, worker_ref.continue_ref)
        } else {
            (SessionMode::New, None)
        };
        let prompt_bundle = app
            .acp_prompt_bundle_for_attempt(
                &task_id,
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
                prompt,
                prompt_id,
                continue_ref.clone(),
            )
            .map_err(command_error)?;
        let app_handle_for_live = app_handle_for_task.clone();
        let task_id_for_live = task_id.clone();
        let run_id_for_live = run_id.clone();
        let round_id_for_live = round_id.clone();
        let node_id_for_live = node_id.clone();
        let attempt_id_for_live = attempt_id.clone();
        client::run_prompt(
            provider,
            &agent_config.adapter,
            app.paths.repo_root.clone(),
            attempt_dir,
            &prompt_bundle,
            session_mode,
            permission_mode,
            continue_ref,
            app.config.use_local_claude,
            app.config.acp_session_title_refresh_enabled,
            Some(&|event| {
                emit_acp_event_update(
                    &app_handle_for_live,
                    &task_id_for_live,
                    &run_id_for_live,
                    &round_id_for_live,
                    &node_id_for_live,
                    &attempt_id_for_live,
                    None,
                    None,
                    event.clone(),
                );
                Ok(())
            }),
        )
        .map_err(command_error)?;
        acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
        )
        .map_err(command_error)
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))??;
    emit_acp_session_update(
        &app_handle,
        &task_id_for_emit,
        &run_id_for_emit,
        &round_id_for_emit,
        &node_id_for_emit,
        &attempt_id_for_emit,
        outer_node_id_for_emit.clone(),
        outer_attempt_id_for_emit.clone(),
        session.clone(),
    );

    // Fire-and-forget: index this attempt for cross-session search
    spawn_index_attempt(
        state.inner(),
        &task_id_for_emit,
        &run_id_for_emit,
        &round_id_for_emit,
        &node_id_for_emit,
        &attempt_id_for_emit,
        outer_node_id_for_emit.as_deref(),
        outer_attempt_id_for_emit.as_deref(),
    );

    Ok(session)
}

#[tauri::command]
pub fn respond_acp_permission(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    request_id: String,
    option_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    let session = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (outer_node_id.as_deref(), outer_attempt_id.as_deref())
    {
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        );
        write_permission_response(
            &attempt_dir,
            &request_id,
            option_id.clone(),
            false,
            current_timestamp(),
        )
        .map_err(command_error)?;
        let events_path = attempt_dir.join("acp.events.jsonl");
        append_permission_decision_artifacts(&attempt_dir, &events_path, request_id, option_id)?;
        dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            None,
        )
        .map_err(command_error)?
    } else {
        let attempt_dir = app
            .paths
            .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        write_permission_response(
            &attempt_dir,
            &request_id,
            option_id.clone(),
            false,
            current_timestamp(),
        )
        .map_err(command_error)?;
        let events_path =
            app.paths
                .acp_events_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        append_permission_decision_artifacts(&attempt_dir, &events_path, request_id, option_id)?;
        acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
        )
        .map_err(command_error)?
    };
    emit_acp_session_update(
        &app_handle,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.clone(),
        outer_attempt_id.clone(),
        session.clone(),
    );
    spawn_index_attempt(
        state.inner(),
        &task_id, &run_id, &round_id, &node_id, &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    Ok(session)
}

fn spawn_index_attempt(
    state: &DesktopState,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) {
    let Ok(app) = state.app() else { return };
    let attempt_dir = if let (Some(on), Some(oa)) = (outer_node_id, outer_attempt_id) {
        app.paths.dynamic_node_attempt_dir(
            task_id, run_id, round_id, on, oa, node_id, attempt_id,
        )
    } else {
        app.paths.attempt_dir(task_id, run_id, round_id, node_id, attempt_id)
    };
    let ctx = AttemptIndexContext {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: outer_node_id.map(String::from),
        outer_attempt_id: outer_attempt_id.map(String::from),
    };
    tauri::async_runtime::spawn_blocking(move || {
        sqlite::index_attempt_with_retry(&attempt_dir, &ctx);
    });
}

fn spawn_acp_cancel_shutdown(
    app: gold_band::app::App,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) {
    thread::spawn(move || {
        let attempt_dir = if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (outer_node_id.as_deref(), outer_attempt_id.as_deref())
        {
            app.paths.dynamic_node_attempt_dir(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            )
        } else {
            app.paths
                .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id)
        };
        graceful_stop_provider(&attempt_dir.join("provider.pid"));
    });
}

fn graceful_stop_provider(pid_path: &camino::Utf8Path) {
    let started_at = Instant::now();
    while pid_path.exists() && started_at.elapsed() < ACP_CANCEL_GRACE_PERIOD {
        thread::sleep(ACP_CANCEL_POLL_INTERVAL);
    }
    if !pid_path.exists() {
        return;
    }
    if let Ok(pid_text) = fs::read_to_string(pid_path.as_std_path()) {
        if let Ok(pid) = pid_text.trim().parse::<u32>() {
            let _ = gold_band::process::kill_process_tree(pid);
        }
    }
    let _ = fs::remove_file(pid_path.as_std_path());
}

fn persist_cancelled_session_snapshot(
    app: &gold_band::app::App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> anyhow::Result<()> {
    let snapshot_path = app
        .paths
        .acp_snapshot_file(task_id, run_id, round_id, node_id, attempt_id);
    if !snapshot_path.exists() {
        return Ok(());
    }
    let mut session = read_json::<serde_json::Value>(&snapshot_path)?;
    let now = current_timestamp();
    session["status"] = serde_json::json!("cancelled");
    session["stopReason"] = serde_json::json!("cancelled");
    session["updatedAt"] = serde_json::json!(now.clone());
    if session.get("updated_at").is_some() {
        session["updated_at"] = serde_json::json!(now);
    }
    write_json(&snapshot_path, &session)?;
    Ok(())
}

fn persist_cancelled_dynamic_session_snapshot(
    app: &gold_band::app::App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> anyhow::Result<()> {
    let snapshot_path = app.paths.dynamic_node_attempt_dir(
        task_id,
        run_id,
        round_id,
        outer_node_id,
        outer_attempt_id,
        node_id,
        attempt_id,
    ).join("acp.snapshot.json");
    if !snapshot_path.exists() {
        return Ok(());
    }
    let mut session = read_json::<serde_json::Value>(&snapshot_path)?;
    let now = current_timestamp();
    session["status"] = serde_json::json!("cancelled");
    session["stopReason"] = serde_json::json!("cancelled");
    session["updatedAt"] = serde_json::json!(now.clone());
    if session.get("updated_at").is_some() {
        session["updated_at"] = serde_json::json!(now);
    }
    write_json(&snapshot_path, &session)?;
    Ok(())
}

fn next_acp_event_seq(path: &camino::Utf8Path) -> u64 {
    if !path.exists() {
        return 1;
    }
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return 1;
    };
    BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .count() as u64
        + 1
}

fn should_append_legacy_permission_event(events_path: &camino::Utf8Path, timeline_path: &camino::Utf8Path) -> bool {
    events_path.exists() && !timeline_path.exists()
}

fn append_permission_decision_artifacts(
    attempt_dir: &camino::Utf8Path,
    events_path: &camino::Utf8Path,
    request_id: String,
    option_id: Option<String>,
) -> CommandResult<()> {
    let timeline_path = attempt_dir.join("acp.timeline.jsonl");
    let source_seq = if timeline_path.exists() || !events_path.exists() {
        latest_timeline_source_seq(&timeline_path) + 1
    } else {
        next_acp_event_seq(events_path)
    };
    let mut event = permission_decision_event(source_seq, request_id.clone(), option_id);
    event.id = format!("permission-{request_id}");
    event.started_seq = Some(source_seq);
    event.ended_seq = Some(source_seq);
    event.started_at = Some(event.timestamp.clone());
    event.ended_at = Some(event.timestamp.clone());

    if should_append_legacy_permission_event(events_path, &timeline_path) {
        append_ui_event(events_path, &event).map_err(command_error)?;
    }

    let mut items = load_timeline_items(&timeline_path).map_err(command_error)?;
    if let Some(existing) = items.iter_mut().find(|item| item.id == event.id) {
        event.started_seq = existing.started_seq.or(event.started_seq);
        event.started_at = existing.started_at.clone().or(event.started_at.clone());
        *existing = event;
    } else {
        items.push(event);
    }
    items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
    write_timeline_items(&timeline_path, &items).map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn cancel_acp_session(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    let requested_at = current_timestamp();
    let background_app = app.clone_for_background();
    let task_id_for_shutdown = task_id.clone();
    let run_id_for_shutdown = run_id.clone();
    let round_id_for_shutdown = round_id.clone();
    let node_id_for_shutdown = node_id.clone();
    let attempt_id_for_shutdown = attempt_id.clone();
    let outer_node_id_for_shutdown = outer_node_id.clone();
    let outer_attempt_id_for_shutdown = outer_attempt_id.clone();
    let session = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (outer_node_id.as_deref(), outer_attempt_id.as_deref())
    {
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        );
        request_cancel(&attempt_dir, requested_at.clone()).map_err(command_error)?;
        cancel_pending_permission_requests(&attempt_dir, requested_at.clone()).map_err(command_error)?;
        background_app
            .pause_dynamic_attempt_runtime_state(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                PauseReason::ProcessInterrupted,
            )
            .map_err(command_error)?;
        persist_cancelled_dynamic_session_snapshot(
            &app,
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        )
        .map_err(command_error)?;
        spawn_acp_cancel_shutdown(
            background_app,
            task_id_for_shutdown,
            run_id_for_shutdown,
            round_id_for_shutdown,
            node_id_for_shutdown,
            attempt_id_for_shutdown,
            outer_node_id_for_shutdown,
            outer_attempt_id_for_shutdown,
        );
        dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            None,
        )
        .map_err(command_error)?
    } else {
        let attempt_dir = app
            .paths
            .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        request_cancel(&attempt_dir, requested_at.clone()).map_err(command_error)?;
        cancel_pending_permission_requests(&attempt_dir, requested_at).map_err(command_error)?;
        background_app
            .pause_attempt_runtime_state(
                &task_id,
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
                PauseReason::ProcessInterrupted,
            )
            .map_err(command_error)?;
        persist_cancelled_session_snapshot(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
        )
        .map_err(command_error)?;
        spawn_acp_cancel_shutdown(
            background_app,
            task_id_for_shutdown,
            run_id_for_shutdown,
            round_id_for_shutdown,
            node_id_for_shutdown,
            attempt_id_for_shutdown,
            outer_node_id_for_shutdown,
            outer_attempt_id_for_shutdown,
        );
        acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
        )
        .map_err(command_error)?
    };
    emit_acp_session_update(
        &app_handle,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.clone(),
        outer_attempt_id.clone(),
        session.clone(),
    );
    spawn_index_attempt(
        state.inner(),
        &task_id, &run_id, &round_id, &node_id, &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    Ok(session)
}

#[tauri::command]
pub async fn get_acp_raw_frames(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpRawFrameQueryInput>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<AcpRawFramePageVm> {
    let app = state.app().map_err(command_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
            let path = app.paths.dynamic_node_attempt_dir(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            ).join("acp.raw.jsonl");
            return super::view_models::acp_raw_frame_page_vm_for_path(
                &path,
                query.unwrap_or(AcpRawFrameQueryInput {
                    page: None,
                    page_size: None,
                    search: None,
                    kind: None,
                    direction: None,
                }),
            )
            .map_err(command_error);
        }
        acp_raw_frame_page_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            query.unwrap_or(AcpRawFrameQueryInput {
                page: None,
                page_size: None,
                search: None,
                kind: None,
                direction: None,
            }),
        )
        .map_err(command_error)
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub fn show_attachment(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) = (&outer_node_id, &outer_attempt_id) {
        let path = app.paths.dynamic_node_attachments_dir(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        ).join(&name);
        app.artifact_show_path(&path)
    } else {
        app.attachment_show(&task_id, &run_id, &round_id, &node_id, &attempt_id, &name)
    };
    content
        .map(|content| ContentVm {
            title: labels.format("detail.attachment", &name),
            kind: "attachment".to_string(),
            content,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id, "outerNodeId": outer_node_id, "outerAttemptId": outer_attempt_id }),
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn show_worker_ref(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) = (&outer_node_id, &outer_attempt_id) {
        let path = app.paths.dynamic_node_worker_ref_file(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        );
        if path.exists() {
            Some(
                std::fs::read_to_string(path.as_std_path())
                    .map_err(|error| command_error(error.into()))?,
            )
        } else {
            None
        }
    } else {
        app.worker_ref_show(&task_id, &run_id, &round_id, &node_id, &attempt_id)
            .map_err(command_error)?
    };
    Ok(ContentVm {
        title: labels.format("detail.workerRef", &node_id),
        kind: "worker-ref".to_string(),
        content: content.unwrap_or_else(|| labels.tr("fallback.missingWorkerRef")),
        metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id, "outerNodeId": outer_node_id, "outerAttemptId": outer_attempt_id }),
    })
}

#[tauri::command]
pub fn save_desktop_preferences(
    state: State<'_, DesktopState>,
    theme: DesktopThemePreference,
    language: DesktopLanguage,
    font: DesktopFontPreference,
    use_local_claude: bool,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    app.set_user_desktop_preferences(theme, language, font.clone())
        .map_err(command_error)?;
    let settings = app
        .set_user_use_local_claude(use_local_claude)
        .map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    Ok(preferences_vm(theme, language, font, use_local_claude))
}

#[tauri::command]
pub fn save_updater_settings(
    state: State<'_, DesktopState>,
    override_url: Option<String>,
) -> CommandResult<UpdaterSettingsVm> {
    let override_url = normalize_updater_url_override(override_url).map_err(command_error)?;
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let settings = app
        .set_user_desktop_updater_url_override(override_url)
        .map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    let config = state.context().map_err(command_error)?.config;
    let settings = updater_settings(&config);
    Ok(settings)
}

#[tauri::command]
pub fn get_update_status(state: State<'_, DesktopState>) -> CommandResult<UpdateStatusVm> {
    state.update_status().map_err(command_error)
}

#[tauri::command]
pub fn mark_settings_update_seen(
    state: State<'_, DesktopState>,
    version: String,
) -> CommandResult<UpdateBadgeStateVm> {
    let config = state
        .mark_update_badge_seen(UpdateBadgeSeenTarget::SettingsEntry, version)
        .map_err(command_error)?;
    Ok(UpdateBadgeStateVm {
        settings_entry_seen_version: config.desktop_update_badges.settings_entry_seen_version,
        settings_advanced_seen_version: config.desktop_update_badges.settings_advanced_seen_version,
        announcement_closed_version: config.desktop_update_badges.announcement_closed_version,
    })
}

#[tauri::command]
pub fn mark_settings_advanced_update_seen(
    state: State<'_, DesktopState>,
    version: String,
) -> CommandResult<UpdateBadgeStateVm> {
    let config = state
        .mark_update_badge_seen(UpdateBadgeSeenTarget::SettingsAdvanced, version)
        .map_err(command_error)?;
    Ok(UpdateBadgeStateVm {
        settings_entry_seen_version: config.desktop_update_badges.settings_entry_seen_version,
        settings_advanced_seen_version: config.desktop_update_badges.settings_advanced_seen_version,
        announcement_closed_version: config.desktop_update_badges.announcement_closed_version,
    })
}

#[tauri::command]
pub fn dismiss_update_announcement(
    state: State<'_, DesktopState>,
    version: String,
) -> CommandResult<UpdateBadgeStateVm> {
    let config = state
        .mark_update_badge_seen(UpdateBadgeSeenTarget::Announcement, version)
        .map_err(command_error)?;
    Ok(UpdateBadgeStateVm {
        settings_entry_seen_version: config.desktop_update_badges.settings_entry_seen_version,
        settings_advanced_seen_version: config.desktop_update_badges.settings_advanced_seen_version,
        announcement_closed_version: config.desktop_update_badges.announcement_closed_version,
    })
}

#[tauri::command]
pub async fn check_update_manual(app: AppHandle) -> CommandResult<UpdateStatusVm> {
    Ok(check_update(&app, false).await)
}

#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> CommandResult<()> {
    run_download_and_install_update(&app).await.map_err(command_error)
}

fn providers_for_node(node: &NodeDsl) -> Vec<String> {
    match node {
        NodeDsl::Worker(worker) => worker.provider.iter().cloned().collect(),
        NodeDsl::AiDynamic(dynamic) => dynamic.bootstrap_provider().map(|provider| vec![provider.to_string()]).unwrap_or_default(),
    }
}

fn ensure_workflow_agents_doctor_ready(
    state: &DesktopState,
    workflow: &WorkflowDsl,
) -> CommandResult<()> {
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    let mut providers = BTreeSet::new();
    for node in &workflow.nodes {
        for provider in providers_for_node(node) {
            providers.insert(provider);
        }
    }
    for provider in providers {
        let agent_type = ManagedAgentType::from_str(&provider).map_err(command_error)?;
        match diagnostics.get(&agent_type) {
            Some(diagnostic) if diagnostic.available => {}
            Some(diagnostic) => {
                return Err(CommandErrorVm::new(
                    "workflow.agent-doctor-failed",
                    serde_json::json!({ "agentType": provider, "reason": diagnostic.reason }),
                ));
            }
            None => {
                return Err(CommandErrorVm::new(
                    "workflow.agent-doctor-required",
                    serde_json::json!({ "agentType": provider }),
                ));
            }
        }
    }
    for node in &workflow.nodes {
        let NodeDsl::Worker(worker) = node else {
            continue;
        };
        let Some(provider) = worker.provider.as_deref() else {
            continue;
        };
        let Some(permission_mode) = worker
            .permission_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let agent_type = ManagedAgentType::from_str(provider).map_err(command_error)?;
        let diagnostic = diagnostics.get(&agent_type).ok_or_else(|| {
            CommandErrorVm::new(
                "workflow.agent-doctor-required",
                serde_json::json!({ "agentType": provider }),
            )
        })?;
        let supported_modes = supported_modes_from_capabilities(diagnostic.capabilities.as_ref());
        if !supported_modes.is_empty()
            && !supported_modes.iter().any(|mode| mode.id == permission_mode)
        {
            return Err(CommandErrorVm::new(
                "workflow.permission-mode-unsupported",
                serde_json::json!({ "agentType": provider, "permissionMode": permission_mode }),
            ));
        }
    }
    Ok(())
}

fn command_error(error: anyhow::Error) -> CommandErrorVm {
    if let Some(error) = error.downcast_ref::<WorkflowValidationError>() {
        return workflow_validation_command_error(error);
    }
    if let Some(error) = error.downcast_ref::<ProfileCommandError>() {
        return CommandErrorVm::new(error.code(), error.params());
    }
    let message = error.to_string();
    if let Some(code) = updater_command_error_code(&message) {
        return CommandErrorVm::new(code, serde_json::json!({ "message": message }));
    }
    CommandErrorVm::new("app.unexpected", serde_json::json!({ "message": message }))
}

fn updater_command_error_code(message: &str) -> Option<&'static str> {
    if message.contains("updater.invalid-url") {
        Some("updater.invalid-url")
    } else if message.contains("updater.no-update") {
        Some("updater.no-update")
    } else if message.contains("updater.install-failed") {
        Some("updater.install-failed")
    } else if message.contains("updater.check-failed") {
        Some("updater.check-failed")
    } else {
        None
    }
}

fn workflow_validation_command_error(error: &WorkflowValidationError) -> CommandErrorVm {
    match error {
        WorkflowValidationError::MissingEndNode => {
            CommandErrorVm::new("workflow.missing-end-node", serde_json::json!({}))
        }
        WorkflowValidationError::UnreachableNode { node_id } => CommandErrorVm::new(
            "workflow.unreachable-node",
            serde_json::json!({ "nodeId": node_id }),
        ),
        WorkflowValidationError::SuccessNewRoundTarget { from } => CommandErrorVm::new(
            "workflow.success-new-round-target",
            serde_json::json!({ "from": from }),
        ),
        WorkflowValidationError::DuplicateWorkflowId {
            workflow_name,
            workflow_id,
            conflicts,
        } => CommandErrorVm::new(
            "workflow.duplicate-id",
            serde_json::json!({
                "workflowName": workflow_name,
                "workflowId": workflow_id,
                "conflicts": conflicts,
            }),
        ),
        WorkflowValidationError::AiDynamicInvalidWorkflow {
            node_id,
            workflow_name,
            reason,
        } => CommandErrorVm::new(
            "workflow.ai-dynamic-invalid-workflow",
            serde_json::json!({
                "nodeId": node_id,
                "workflowName": workflow_name,
                "reason": reason,
            }),
        ),
    }
}

// ── SQLite search commands ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchAcpPromptsInput {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchAcpSessionsInput {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchTasksInput {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

#[tauri::command]
pub async fn search_tasks(
    state: State<'_, DesktopState>,
    input: SearchTasksInput,
) -> CommandResult<Vec<gold_band::storage::sqlite::TaskSearchResult>> {
    let _ = state.app().map_err(command_error)?;
    let limit = input.limit.min(200);
    let query = input.query;
    tauri::async_runtime::spawn_blocking(move || {
        let index = gold_band::storage::sqlite::search_index()
            .ok_or_else(|| CommandErrorVm::new("search.index-unavailable", serde_json::json!({})))?;
        index.search_tasks(&query, limit).map_err(|e| {
            CommandErrorVm::new(
                "search.query-failed",
                serde_json::json!({ "message": e.to_string() }),
            )
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub async fn search_acp_prompts(
    state: State<'_, DesktopState>,
    input: SearchAcpPromptsInput,
) -> CommandResult<Vec<gold_band::storage::sqlite::PromptSearchResult>> {
    let _ = state.app().map_err(command_error)?;
    let limit = input.limit.min(200);
    let query = input.query;
    tauri::async_runtime::spawn_blocking(move || {
        let index = gold_band::storage::sqlite::search_index()
            .ok_or_else(|| CommandErrorVm::new("search.index-unavailable", serde_json::json!({})))?;
        index.search_prompts(&query, limit).map_err(|e| {
            CommandErrorVm::new(
                "search.query-failed",
                serde_json::json!({ "message": e.to_string() }),
            )
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub async fn search_acp_sessions(
    state: State<'_, DesktopState>,
    input: SearchAcpSessionsInput,
) -> CommandResult<Vec<gold_band::storage::sqlite::SessionSearchResult>> {
    let _ = state.app().map_err(command_error)?;
    let limit = input.limit.min(200);
    let query = input.query;
    tauri::async_runtime::spawn_blocking(move || {
        let index = gold_band::storage::sqlite::search_index()
            .ok_or_else(|| CommandErrorVm::new("search.index-unavailable", serde_json::json!({})))?;
        index.search_sessions(&query, limit).map_err(|e| {
            CommandErrorVm::new(
                "search.query-failed",
                serde_json::json!({ "message": e.to_string() }),
            )
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}
