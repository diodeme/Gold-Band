use gold_band::acp::client;
use gold_band::acp::events::{
    AcpUiEvent, append_ui_event, current_timestamp, latest_timeline_source_seq,
    load_timeline_items, permission_decision_event, write_timeline_items,
};
use gold_band::acp::permission::{
    PendingPermissionState, cancel_pending_permission_requests, clear_cancel_request,
    request_cancel, write_permission_response,
};
use gold_band::app::{
    App, AutoTemplate, AutoTemplateStore, CreateTaskInput, ProfileCommandError, ProfileEntry,
    ProfileInput, ProfileList, RuntimeInterventionKind, RuntimeLifecycleEvent,
    WorkflowTemplateStore,
};
use gold_band::domain::{NodeOutcome, PauseReason, RunStatus, SessionMode};
use gold_band::dsl::{NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::provider::supported_modes_from_capabilities;
use gold_band::runtime::{NodeState, RunState, WorkerRefState};
use gold_band::storage::sqlite::{self, AttemptIndexContext};
use gold_band::storage::{read_json, write_json};
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
    AcpAdapterConfig, ConversationAutoConfig, DesktopFontPreference, DesktopLanguage,
    DesktopThemePreference, ManagedAgentConfig, ManagedAgentType,
};
use gold_band::observability::set_runtime_log_level;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::i18n::Translator;
use crate::metrics::{MetricsSettingsVm, metrics_settings, normalize_metrics_base_url};
use crate::state::{
    DesktopContext, DesktopState, NotificationAttentionInput, UpdateBadgeSeenTarget,
};
use crate::updater::{
    UpdateStatusVm, UpdaterSettingsVm, check_update,
    download_and_install_update as run_download_and_install_update, normalize_updater_url_override,
    updater_settings,
};
use crate::view_models::{
    AcpRawFramePageVm, AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AgentRegistryVm,
    AppBootstrapVm, ContentVm, LocalClaudeStatusVm, LogPageVm, LogQueryInput, McpServerVm,
    PreferencesVm, RoundDetailVm, RoundSelectionInput, RunDetailVm, RunSummaryVm, SkillContentVm,
    SkillListVm, SkillMetaVm, TaskDetailVm, TaskListVm, UpdateBadgeStateVm, WorkflowVm,
    acp_raw_frame_page_vm, acp_session_vm, agent_registry_vm, bootstrap_vm, dynamic_acp_session_vm,
    log_page_vm, mcp_server_list_vm, preferences_vm, round_detail_vm, run_detail_vm,
    run_summary_vm, skill_content_vm, skill_list_vm, skill_meta_vm, task_detail_vm, task_list_vm,
    workflow_vm,
};
use crate::view_models_conversation::{
    ConversationAttemptLifecycleVm, conversation_attempt_lifecycle_vm,
};

const ACP_SESSION_EVENT: &str = "gold-band://acp-session-updated";
const ACP_CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(5);
const ACP_CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub type CommandResult<T> = Result<T, CommandErrorVm>;

#[derive(Debug, Clone)]
struct AttemptLocator {
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
}

impl AttemptLocator {
    fn new(
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        outer_node_id: Option<String>,
        outer_attempt_id: Option<String>,
    ) -> Self {
        let has_outer = outer_node_id.is_some() && outer_attempt_id.is_some();
        Self {
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id: has_outer.then(|| outer_node_id.unwrap()),
            outer_attempt_id: has_outer.then(|| outer_attempt_id.unwrap()),
        }
    }

    fn outer_node_id(&self) -> Option<&str> {
        self.outer_node_id.as_deref()
    }

    fn outer_attempt_id(&self) -> Option<&str> {
        self.outer_attempt_id.as_deref()
    }

    fn runtime_node_id(&self) -> &str {
        self.outer_node_id().unwrap_or(&self.node_id)
    }

    fn runtime_attempt_id(&self) -> &str {
        self.outer_attempt_id().unwrap_or(&self.attempt_id)
    }

    fn matches_run_current(&self, run: &RunState) -> bool {
        run.current_round.as_deref() == Some(self.round_id.as_str())
            && run.current_node.as_deref() == Some(self.runtime_node_id())
            && run.current_attempt.as_deref() == Some(self.runtime_attempt_id())
    }

    fn attempt_dir(&self, app: &gold_band::app::App) -> Utf8PathBuf {
        if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (self.outer_node_id(), self.outer_attempt_id())
        {
            app.paths.dynamic_node_attempt_dir(
                &self.task_id,
                &self.run_id,
                &self.round_id,
                outer_node_id,
                outer_attempt_id,
                &self.node_id,
                &self.attempt_id,
            )
        } else {
            app.paths.attempt_dir(
                &self.task_id,
                &self.run_id,
                &self.round_id,
                &self.node_id,
                &self.attempt_id,
            )
        }
    }
}

fn resolve_acp_attempt_dir(
    app: &gold_band::app::App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> Utf8PathBuf {
    AttemptLocator::new(
        task_id.to_string(),
        run_id.to_string(),
        round_id.to_string(),
        node_id.to_string(),
        attempt_id.to_string(),
        outer_node_id.map(str::to_string),
        outer_attempt_id.map(str::to_string),
    )
    .attempt_dir(app)
}

fn lifecycle_for_locator(
    app: &App,
    locator: &AttemptLocator,
) -> Option<ConversationAttemptLifecycleVm> {
    conversation_attempt_lifecycle_vm(
        app,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id(),
        locator.outer_attempt_id(),
    )
    .ok()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcpSessionUpdatedEventVm {
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
    event: Option<AcpUiEvent>,
    lifecycle: Option<ConversationAttemptLifecycleVm>,
}

fn normalize_workspace_project_id(workspace_path: &str) -> String {
    workspace_path
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
}

fn resolve_workspace_app(
    context: &DesktopContext,
    project_id: Option<&str>,
) -> Result<(App, String), CommandErrorVm> {
    match project_id {
        None | Some("") => {
            let pid = normalize_workspace_project_id(context.repo_root.as_str());
            Ok((context.app(), pid))
        }
        Some(pid) => {
            let default_pid = normalize_workspace_project_id(context.repo_root.as_str());
            if pid == default_pid {
                return Ok((context.app(), default_pid));
            }
            let global_app = context.app();
            let state = global_app.load_state().map_err(command_error)?;
            for w in &state.conversation_workspaces {
                if w.project_id == pid {
                    let app = App::with_config(
                        Utf8PathBuf::from(&w.workspace_path),
                        context.config.clone(),
                    );
                    return Ok((app, w.project_id.clone()));
                }
            }
            Err(CommandErrorVm::new(
                "workspace.not-found",
                serde_json::json!({ "projectId": pid }),
            ))
        }
    }
}

fn resolve_command_app(
    state: &DesktopState,
    project_id: Option<&str>,
) -> Result<App, CommandErrorVm> {
    let context = state.context().map_err(command_error)?;
    let (app, _) = resolve_workspace_app(&context, project_id)?;
    Ok(app)
}

pub(crate) fn register_lifecycle_subscribers(app: &App, app_handle: &AppHandle) {
    app.lifecycle_bus
        .subscribe(crate::metrics::create_metrics_subscriber(
            app_handle.clone(),
        ));
    app.lifecycle_bus.subscribe(
        crate::notifications::create_intervention_notification_subscriber(app_handle.clone()),
    );
}

pub(crate) fn acp_live_update_emitter_for_app(
    app: &App,
    app_handle: AppHandle,
    project_id: Option<String>,
) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext, AcpUiEvent) -> anyhow::Result<()> + Send + Sync>
{
    acp_live_update_emitter(app_handle, project_id, Some(app.lifecycle_bus.clone()))
}

fn resolve_command_app_with_emitters(
    app_handle: &AppHandle,
    context: &DesktopContext,
    project_id: Option<&str>,
) -> Result<App, CommandErrorVm> {
    let (base_app, _) = resolve_workspace_app(context, project_id)?;
    let pid = project_id.map(|s| s.to_string());
    let bg_app = base_app.clone_for_background();
    let app = base_app;
    let live_update = acp_live_update_emitter_for_app(&app, app_handle.clone(), pid.clone());
    let app = app
        .with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(app_handle.clone(), bg_app, pid));
    register_lifecycle_subscribers(&app, app_handle);
    Ok(app)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorVm {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptSubmitVm {
    pub kind: String,
    pub session: Option<AcpSessionVm>,
    pub run: Option<RunSummaryVm>,
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSessionStopVm {
    pub kind: String,
    pub run: Option<RunSummaryVm>,
    pub session: Option<AcpSessionVm>,
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
}

impl CommandErrorVm {
    pub fn new(code: impl Into<String>, params: serde_json::Value) -> Self {
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAutoTemplateInputVm {
    pub name: String,
    pub config: ConversationAutoConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutoTemplateInputVm {
    pub name: String,
    pub config: ConversationAutoConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceAutoTemplatesInputVm {
    pub templates: Vec<AutoTemplate>,
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
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
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
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
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
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
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
pub async fn choose_workspace(
    app: AppHandle,
    state: State<'_, DesktopState>,
    path: String,
) -> CommandResult<AppBootstrapVm> {
    let repo_root = Utf8PathBuf::from_path_buf(std::path::PathBuf::from(&path))
        .map_err(|_| CommandErrorVm::new("workspace.path-invalid-utf8", serde_json::json!({})))?;
    info!(selected_repo_root = %repo_root, "workspace picker returned selection");
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    info!(
        active_repo_root = %context.repo_root,
        recent_workspace_count = context.recent_workspaces.len(),
        "workspace selection applied"
    );
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app.package_info().version.to_string(),
        false,
    ))
}

#[tauri::command]
pub fn select_recent_workspace(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    workspace: String,
) -> CommandResult<AppBootstrapVm> {
    info!(selected_repo_root = %workspace, "switching to recent workspace");
    let repo_root = Utf8PathBuf::from(workspace);
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    info!(
        active_repo_root = %context.repo_root,
        recent_workspace_count = context.recent_workspaces.len(),
        "recent workspace selection applied"
    );
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
    project_id: Option<String>,
    task_id: String,
    input: SaveWorkflowInputVm,
) -> CommandResult<WorkflowVm> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
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
pub fn get_auto_templates(state: State<'_, DesktopState>) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.auto_templates().map_err(command_error)
}

#[tauri::command]
pub fn save_auto_template(
    state: State<'_, DesktopState>,
    input: SaveAutoTemplateInputVm,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.save_auto_template(input.name, input.config)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_auto_template(
    state: State<'_, DesktopState>,
    template_id: String,
    input: UpdateAutoTemplateInputVm,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.update_auto_template(&template_id, input.name, input.config)
        .map_err(command_error)
}

#[tauri::command]
pub fn delete_auto_template(
    state: State<'_, DesktopState>,
    template_id: String,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.delete_auto_template(&template_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn replace_auto_templates(
    state: State<'_, DesktopState>,
    input: ReplaceAutoTemplatesInputVm,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.replace_auto_templates(input.templates)
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
pub fn start_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let live_update = acp_live_update_emitter_for_app(&app, app_handle.clone(), None);
    let app = app
        .with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(
            app_handle.clone(),
            context.app(),
            None,
        ));
    register_lifecycle_subscribers(&app, &app_handle);
    app.run_start_background(&task_id, None)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn continue_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    prompt_id: Option<String>,
    prompt: Option<String>,
) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = resolve_command_app_with_emitters(&app_handle, &context, project_id.as_deref())?;
    app.run_continue_background(&task_id, &run_id, prompt_id, prompt)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn pause_run(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
    app.run_pause(&task_id, &run_id, PauseReason::ProcessInterrupted)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn stop_active_session(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ActiveSessionStopVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let was_running = app
        .run_status(&task_id, &run_id)
        .map(|run| run.status == RunStatus::Running)
        .map_err(command_error)?;

    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let session = cancel_acp_session(
        app_handle,
        state,
        project_id.clone(),
        locator.task_id.clone(),
        locator.run_id.clone(),
        locator.round_id.clone(),
        locator.node_id.clone(),
        locator.attempt_id.clone(),
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
    )?;
    let lifecycle = lifecycle_for_locator(&app, &locator);

    if was_running {
        let paused = app.run_status(&task_id, &run_id).map_err(command_error)?;
        return Ok(ActiveSessionStopVm {
            kind: "run-paused".to_string(),
            run: Some(run_summary_vm(paused)),
            session,
            lifecycle,
        });
    }

    Ok(ActiveSessionStopVm {
        kind: "session-cancelled".to_string(),
        run: None,
        session,
        lifecycle,
    })
}

#[tauri::command]
pub fn submit_manual_check(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outcome: String,
) -> CommandResult<RunSummaryVm> {
    let context = state.context().map_err(command_error)?;
    let app = resolve_command_app_with_emitters(&app_handle, &context, project_id.as_deref())?;
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
    let app = context.app();
    let live_update = acp_live_update_emitter_for_app(&app, app_handle.clone(), None);
    let app = app
        .with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(
            app_handle.clone(),
            context.app(),
            None,
        ));
    register_lifecycle_subscribers(&app, &app_handle);
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
    let summary = app.run_kill(&task_id, &run_id).map_err(command_error)?;
    // run 终态：清理该 run 的全部干预通知 dedup key，防常驻 EXE 内存泄漏（方案 §8.3）。
    // 幂等；clear_run 不影响其他 run。
    state.notification_dedup().clear_run(&summary.id);
    Ok(run_summary_vm(summary))
}

#[tauri::command]
pub fn show_artifact(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&outer_node_id, &outer_attempt_id)
    {
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

#[tauri::command]
pub fn get_metrics_settings(state: State<'_, DesktopState>) -> CommandResult<MetricsSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let vm = metrics_settings(&context.config);
    eprintln!(
        "[metrics] enabled={} toggle_locked={} base_url={:?} heartbeat={:?} node_metrics={:?} api_key_set={}",
        vm.enabled,
        vm.toggle_locked,
        vm.metrics_base_url,
        vm.heartbeat_endpoint,
        vm.node_metrics_endpoint,
        vm.api_key_set,
    );
    Ok(vm)
}

#[tauri::command]
pub fn update_notification_attention(
    state: State<'_, DesktopState>,
    input: NotificationAttentionInput,
) -> CommandResult<()> {
    state
        .update_notification_attention(input)
        .map_err(command_error)
}

#[tauri::command]
pub fn save_metrics_settings(
    state: State<'_, DesktopState>,
    enabled: bool,
    metrics_base_url: Option<String>,
    api_key: Option<String>,
) -> CommandResult<MetricsSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let mut existing = app.load_settings().map_err(command_error)?;
    existing.desktop_metrics_enabled = Some(enabled);
    existing.desktop_metrics_base_url = metrics_base_url
        .as_deref()
        .and_then(normalize_metrics_base_url);
    existing.desktop_metrics_api_key = api_key.filter(|s| !s.trim().is_empty());
    app.save_settings(&existing).map_err(command_error)?;
    state
        .update_settings_config(&existing)
        .map_err(command_error)?;
    let updated_context = state.context().map_err(command_error)?;
    Ok(metrics_settings(&updated_context.config))
}

pub(crate) fn acp_live_update_emitter(
    app_handle: AppHandle,
    project_id: Option<String>,
    lifecycle_bus: Option<gold_band::app::observability::RuntimeLifecycleBus>,
) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext, AcpUiEvent) -> anyhow::Result<()> + Send + Sync>
{
    Arc::new(move |context, event| {
        if let Some(lifecycle_bus) = lifecycle_bus.as_ref() {
            maybe_emit_permission_intervention(lifecycle_bus, &context, &event);
        }
        emit_acp_event_update(
            &app_handle,
            project_id.clone(),
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

/// 路径 B：旁路监听 `permissionRequest` 事件流，强制 `PermissionRequested` 发干预通知。
///
/// 仅当 `event.kind == "permissionRequest" && status == "pending"` 时触发。文案用一般性
/// 描述（node_label 用 node_id、task_title 留 None），不查 App、不改主干 context（方案
/// §6.2/§9.4）。dedup 与路径 A 共享同一 `DesktopState.notification_dedup` 实例。
fn maybe_emit_permission_intervention(
    lifecycle_bus: &gold_band::app::observability::RuntimeLifecycleBus,
    context: &gold_band::app::AcpLiveEventContext,
    event: &AcpUiEvent,
) {
    if event.kind != "permissionRequest" {
        return;
    }
    let is_pending = event
        .status
        .as_deref()
        .map(|s| s == "pending")
        .unwrap_or(false);
    if !is_pending {
        return;
    }
    lifecycle_bus.emit(RuntimeLifecycleEvent::InterventionRequested {
        event_id: gold_band::app::make_dedup_key(
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
            PauseReason::PermissionRequested,
        ),
        occurred_at: current_timestamp(),
        task_id: context.task_id.clone(),
        run_id: context.run_id.clone(),
        round_id: context.round_id.clone(),
        node_id: context.node_id.clone(),
        attempt_id: context.attempt_id.clone(),
        node_label: context.node_id.clone(),
        kind: RuntimeInterventionKind::PermissionRequested,
        task_title: None,
    });
}

pub(crate) fn acp_session_update_emitter(
    app_handle: AppHandle,
    app: gold_band::app::App,
    project_id: Option<String>,
) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext) -> anyhow::Result<()> + Send + Sync> {
    Arc::new(move |context| {
        let session = if let (Some(outer_node_id), Some(outer_attempt_id)) = (
            context.outer_node_id.as_deref(),
            context.outer_attempt_id.as_deref(),
        ) {
            dynamic_acp_session_vm(
                &app,
                &context.task_id,
                &context.run_id,
                &context.round_id,
                outer_node_id,
                outer_attempt_id,
                &context.node_id,
                &context.attempt_id,
                None,
                None,
            )?
        } else {
            acp_session_vm(
                &app,
                &context.task_id,
                &context.run_id,
                &context.round_id,
                &context.node_id,
                &context.attempt_id,
                None,
                None,
            )?
        };
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id.clone(),
            &context.task_id,
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
            context.outer_node_id.clone(),
            context.outer_attempt_id.clone(),
            session,
        );
        Ok(())
    })
}

fn emit_acp_session_update(
    app_handle: &AppHandle,
    app: &App,
    project_id: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
) {
    emit_acp_update(
        app_handle,
        Some(app),
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        session,
        None,
    );
}

fn emit_acp_event_update(
    app_handle: &AppHandle,
    project_id: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    event: AcpUiEvent,
) {
    emit_acp_update(
        app_handle,
        None,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        None,
        Some(event),
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_acp_update(
    app_handle: &AppHandle,
    app: Option<&App>,
    project_id: Option<String>,
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
    let lifecycle = app.and_then(|app| {
        conversation_attempt_lifecycle_vm(
            app,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id.as_deref(),
            outer_attempt_id.as_deref(),
        )
        .ok()
    });
    let _ = app_handle.emit(
        ACP_SESSION_EVENT,
        AcpSessionUpdatedEventVm {
            project_id,
            task_id: task_id.to_string(),
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            outer_node_id,
            outer_attempt_id,
            session,
            event,
            lifecycle,
        },
    );
}

#[tauri::command]
pub fn get_acp_session(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpSessionQueryInput>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (outer_node_id.as_deref(), outer_attempt_id.as_deref())
    {
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
            None,
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
        None,
    )
    .map_err(command_error)
}

#[tauri::command]
pub async fn submit_conversation_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let context = state.context().map_err(command_error)?;
    let app = resolve_command_app_with_emitters(&app_handle, &context, project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let run = app
        .run_status(&locator.task_id, &locator.run_id)
        .map_err(command_error)?;
    let submit_target = if run.status == RunStatus::Paused
        && gold_band::app::is_run_continuable(&run)
        && locator.matches_run_current(&run)
    {
        "runtime-continue"
    } else {
        "acp-prompt"
    };

    if submit_target == "runtime-continue" {
        let run = if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (locator.outer_node_id(), locator.outer_attempt_id())
        {
            app.run_continue_dynamic_inner_background(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                outer_node_id,
                outer_attempt_id,
                &locator.node_id,
                &locator.attempt_id,
                prompt_id,
                prompt,
                attachment_paths.unwrap_or_default(),
            )
        } else {
            app.run_continue_background(&locator.task_id, &locator.run_id, prompt_id, Some(prompt))
        }
        .map(run_summary_vm)
        .map_err(command_error)?;
        return Ok(ConversationPromptSubmitVm {
            kind: "runtime-continue-started".to_string(),
            session: None,
            run: Some(run),
            lifecycle: lifecycle_for_locator(&app, &locator),
        });
    }

    let session = send_acp_prompt(
        app_handle,
        state,
        project_id,
        locator.task_id.clone(),
        locator.run_id.clone(),
        locator.round_id.clone(),
        locator.node_id.clone(),
        locator.attempt_id.clone(),
        prompt,
        prompt_id,
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
        attachment_paths,
    )
    .await?;
    Ok(ConversationPromptSubmitVm {
        kind: "acp-session".to_string(),
        session,
        run: None,
        lifecycle: lifecycle_for_locator(&app, &locator),
    })
}

#[tauri::command]
pub async fn send_acp_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<Option<AcpSessionVm>> {
    let context = state.context().map_err(command_error)?;
    let app = resolve_command_app_with_emitters(&app_handle, &context, project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
    if let Ok(run) = app.run_status(&task_id, &run_id) {
        if run.status == RunStatus::Paused
            && gold_band::app::is_run_continuable(&run)
            && locator.matches_run_current(&run)
        {
            return Err(CommandErrorVm::new(
                "acp.runtime-submit-required",
                serde_json::json!({
                    "taskId": task_id,
                    "runId": run_id,
                    "roundId": round_id,
                    "nodeId": node_id,
                    "attemptId": attempt_id,
                    "outerNodeId": outer_node_id,
                    "outerAttemptId": outer_attempt_id,
                }),
            ));
        }
    }
    let project_id_for_emit = project_id.clone();
    let project_id_for_spawn = project_id_for_emit.clone();
    let task_id_for_emit = task_id.clone();
    let run_id_for_emit = run_id.clone();
    let round_id_for_emit = round_id.clone();
    let node_id_for_emit = node_id.clone();
    let attempt_id_for_emit = attempt_id.clone();
    let outer_node_id_for_emit = outer_node_id.clone();
    let outer_attempt_id_for_emit = outer_attempt_id.clone();
    let app_for_emit = app.clone_for_background();
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
            let permission_mode = current_acp_session_permission_mode(&attempt_dir)
                .or_else(|| node.permission_mode.clone());
            let model = current_acp_session_model(&attempt_dir).or_else(|| node.model.clone());
            let (session_mode, continue_ref) = if worker_ref_path.exists() {
                let worker_ref =
                    read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?;
                (worker_ref.mode, worker_ref.continue_ref)
            } else {
                (SessionMode::New, None)
            };
            let mut prompt_bundle = app
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
            // Resolve attachments
            if let Some(ref paths) = attachment_paths {
                if !paths.is_empty() {
                    let user_inputs_dir = format!("{}/user-inputs", attempt_dir);
                    let _ = std::fs::create_dir_all(&user_inputs_dir);
                    if let Ok(resolved) =
                        gold_band::provider::resolve_attachments(paths, "user-inputs")
                    {
                        // Copy files to user-inputs/
                        for (r, src) in resolved.iter().zip(paths.iter()) {
                            let src_path = std::path::Path::new(src);
                            if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                                let dest = std::path::Path::new(&user_inputs_dir).join(name);
                                let _ = std::fs::copy(src_path, &dest);
                            }
                            prompt_bundle.attachment_metas.push(r.meta.clone());
                            prompt_bundle.content_blocks.push(r.block.clone());
                        }
                    }
                }
            }
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
                model,
                continue_ref,
                app.config.use_local_claude,
                app.config.acp_session_title_refresh_enabled,
                app.config.acp_raw_max_size_bytes,
                app.config.acp_raw_target_size_bytes,
                Some(&|event| {
                    maybe_emit_permission_intervention(
                        &app.lifecycle_bus,
                        &gold_band::app::AcpLiveEventContext {
                            task_id: task_id_for_live.clone(),
                            run_id: run_id_for_live.clone(),
                            round_id: round_id_for_live.clone(),
                            node_id: node_id_for_live.clone(),
                            attempt_id: attempt_id_for_live.clone(),
                            outer_node_id: outer_node_id_for_live.clone(),
                            outer_attempt_id: outer_attempt_id_for_live.clone(),
                        },
                        event,
                    );
                    emit_acp_event_update(
                        &app_handle_for_live,
                        project_id_for_spawn.clone(),
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
                &app.acp_mcp_servers().unwrap_or_else(|e| {
                    eprintln!("WARN: failed to load MCP servers for ACP session: {e}");
                    Vec::new()
                }),
                None, // session_update
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
        let permission_mode = current_acp_session_permission_mode(&attempt_dir).or_else(|| {
            node.resolved_config
                .get("permissionMode")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
        let (session_mode, continue_ref) = if worker_ref_path.exists() {
            let worker_ref =
                read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?;
            (worker_ref.mode, worker_ref.continue_ref)
        } else {
            (SessionMode::New, None)
        };
        let mut prompt_bundle = app
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
        // Resolve attachments
        if let Some(ref paths) = attachment_paths {
            if !paths.is_empty() {
                let user_inputs_dir = format!("{}/user-inputs", attempt_dir);
                let _ = std::fs::create_dir_all(&user_inputs_dir);
                if let Ok(resolved) = gold_band::provider::resolve_attachments(paths, "user-inputs")
                {
                    for (r, src) in resolved.iter().zip(paths.iter()) {
                        let src_path = std::path::Path::new(src);
                        if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                            let dest = std::path::Path::new(&user_inputs_dir).join(name);
                            let _ = std::fs::copy(src_path, &dest);
                        }
                        prompt_bundle.attachment_metas.push(r.meta.clone());
                        prompt_bundle.content_blocks.push(r.block.clone());
                    }
                }
            }
        }
        let app_handle_for_live = app_handle_for_task.clone();
        let task_id_for_live = task_id.clone();
        let run_id_for_live = run_id.clone();
        let round_id_for_live = round_id.clone();
        let node_id_for_live = node_id.clone();
        let attempt_id_for_live = attempt_id.clone();
        let model = current_acp_session_model(&attempt_dir);
        client::run_prompt(
            provider,
            &agent_config.adapter,
            app.paths.repo_root.clone(),
            attempt_dir,
            &prompt_bundle,
            session_mode,
            permission_mode,
            model,
            continue_ref,
            app.config.use_local_claude,
            app.config.acp_session_title_refresh_enabled,
            app.config.acp_raw_max_size_bytes,
            app.config.acp_raw_target_size_bytes,
            Some(&|event| {
                maybe_emit_permission_intervention(
                    &app.lifecycle_bus,
                    &gold_band::app::AcpLiveEventContext {
                        task_id: task_id_for_live.clone(),
                        run_id: run_id_for_live.clone(),
                        round_id: round_id_for_live.clone(),
                        node_id: node_id_for_live.clone(),
                        attempt_id: attempt_id_for_live.clone(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    event,
                );
                emit_acp_event_update(
                    &app_handle_for_live,
                    project_id_for_spawn.clone(),
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
            &app.acp_mcp_servers().unwrap_or_else(|e| {
                eprintln!("WARN: failed to load MCP servers for ACP session: {e}");
                Vec::new()
            }),
            None, // session_update
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
            None,
        )
        .map_err(command_error)
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))??;
    emit_acp_session_update(
        &app_handle,
        &app_for_emit,
        project_id_for_emit,
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
    project_id: Option<String>,
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
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
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
        let canonical_request_id = canonical_permission_request_id(&attempt_dir, &request_id);
        write_permission_response(
            &attempt_dir,
            &canonical_request_id,
            option_id.clone(),
            false,
            current_timestamp(),
        )
        .map_err(command_error)?;
        let events_path = attempt_dir.join("acp.events.jsonl");
        append_permission_decision_artifacts(
            &attempt_dir,
            &events_path,
            canonical_request_id,
            option_id,
        )?;
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
            None,
        )
        .map_err(command_error)?
    } else {
        let attempt_dir =
            app.paths
                .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let canonical_request_id = canonical_permission_request_id(&attempt_dir, &request_id);
        write_permission_response(
            &attempt_dir,
            &canonical_request_id,
            option_id.clone(),
            false,
            current_timestamp(),
        )
        .map_err(command_error)?;
        let events_path =
            app.paths
                .acp_events_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        append_permission_decision_artifacts(
            &attempt_dir,
            &events_path,
            canonical_request_id,
            option_id,
        )?;
        acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            None,
        )
        .map_err(command_error)?
    };
    emit_acp_session_update(
        &app_handle,
        &app,
        project_id.clone(),
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
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
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
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
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
    app_handle: AppHandle,
    app: gold_band::app::App,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) {
    thread::spawn(move || {
        let attempt_dir = resolve_acp_attempt_dir(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            outer_node_id.as_deref(),
            outer_attempt_id.as_deref(),
        );
        let pid_path = attempt_dir.join("provider.pid");
        eprintln!(
            "[gb-acp-stop] shutdown-start task={} run={} round={} node={} attempt={} pid_exists={} cancel_requested={}",
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            pid_path.exists(),
            gold_band::acp::permission::is_cancel_requested(&attempt_dir)
        );
        graceful_stop_provider(&pid_path);
        let clear_result = clear_cancel_request(&attempt_dir);
        eprintln!(
            "[gb-acp-stop] shutdown-after-provider task={} run={} round={} node={} attempt={} pid_exists={} cancel_requested={} clear_cancel_ok={}",
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            pid_path.exists(),
            gold_band::acp::permission::is_cancel_requested(&attempt_dir),
            clear_result.is_ok()
        );

        // Provider has fully stopped — now write the cancelled snapshot and
        // emit a live update so the UI transitions from "cancelling" → "cancelled".
        let session = if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (outer_node_id.as_deref(), outer_attempt_id.as_deref())
        {
            let _ = persist_cancelled_dynamic_session_snapshot(
                &app,
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
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
                None,
            )
            .ok()
            .flatten()
        } else {
            let _ = persist_cancelled_session_snapshot(
                &app,
                &task_id,
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
            );
            acp_session_vm(
                &app,
                &task_id,
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        };
        eprintln!(
            "[gb-acp-stop] shutdown-emit task={} run={} round={} node={} attempt={} session_status={} session_id={} has_session={}",
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            session
                .as_ref()
                .map(|session| session.status.as_str())
                .unwrap_or("<none>"),
            session
                .as_ref()
                .and_then(|session| session.session_id.as_deref())
                .unwrap_or("<none>"),
            session.is_some()
        );
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            outer_node_id,
            outer_attempt_id,
            session,
        );
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
    let snapshot_path = app
        .paths
        .dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            attempt_id,
        )
        .join("acp.snapshot.json");
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

fn should_append_legacy_permission_event(
    events_path: &camino::Utf8Path,
    timeline_path: &camino::Utf8Path,
) -> bool {
    events_path.exists() && !timeline_path.exists()
}

fn canonical_permission_request_id(attempt_dir: &camino::Utf8Path, request_id: &str) -> String {
    let stripped_request_id = strip_permission_display_prefix(request_id);
    let candidates = [request_id.to_string(), stripped_request_id.clone()];
    for candidate in candidates {
        let path = gold_band::acp::permission::pending_permission_file(attempt_dir, &candidate);
        if let Ok(pending) = read_json::<PendingPermissionState>(&path) {
            return pending.request_id;
        }
    }
    stripped_request_id
}

fn strip_permission_display_prefix(request_id: &str) -> String {
    let mut current = request_id;
    while let Some(next) = current.strip_prefix("permission-") {
        current = next;
    }
    current.to_string()
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
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let requested_at = current_timestamp();
    let background_app = app.clone_for_background();
    let task_id_for_shutdown = task_id.clone();
    let run_id_for_shutdown = run_id.clone();
    let round_id_for_shutdown = round_id.clone();
    let node_id_for_shutdown = node_id.clone();
    let attempt_id_for_shutdown = attempt_id.clone();
    let outer_node_id_for_shutdown = outer_node_id.clone();
    let outer_attempt_id_for_shutdown = outer_attempt_id.clone();
    let pid_for_shutdown = project_id.clone();
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
        cancel_pending_permission_requests(&attempt_dir, requested_at.clone())
            .map_err(command_error)?;
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
        spawn_acp_cancel_shutdown(
            app_handle.clone(),
            background_app,
            pid_for_shutdown,
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
            None,
        )
        .map_err(command_error)?
    } else {
        let attempt_dir =
            app.paths
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
        spawn_acp_cancel_shutdown(
            app_handle.clone(),
            background_app,
            pid_for_shutdown,
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
            None,
        )
        .map_err(command_error)?
    };
    emit_acp_session_update(
        &app_handle,
        &app,
        project_id,
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
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    Ok(session)
}

#[tauri::command]
pub async fn get_acp_raw_frames(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpRawFrameQueryInput>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<AcpRawFramePageVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (outer_node_id.as_deref(), outer_attempt_id.as_deref())
        {
            let path = app
                .paths
                .dynamic_node_attempt_dir(
                    &task_id,
                    &run_id,
                    &round_id,
                    outer_node_id,
                    outer_attempt_id,
                    &node_id,
                    &attempt_id,
                )
                .join("acp.raw.jsonl");
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
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&outer_node_id, &outer_attempt_id)
    {
        let path = app
            .paths
            .dynamic_node_attachments_dir(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            )
            .join(&name);
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
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&outer_node_id, &outer_attempt_id)
    {
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
    verbose_logging: bool,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    app.set_user_desktop_preferences(theme, language, font.clone())
        .map_err(command_error)?;
    app.set_user_use_local_claude(use_local_claude)
        .map_err(command_error)?;
    let settings = app
        .set_user_verbose_logging(verbose_logging)
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    let log_level = settings.log_level.unwrap_or(context.config.log_level);
    set_runtime_log_level(log_level);
    Ok(preferences_vm(
        theme,
        language,
        font,
        use_local_claude,
        log_level,
    ))
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
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
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
    run_download_and_install_update(&app)
        .await
        .map_err(command_error)
}

fn providers_for_node(node: &NodeDsl) -> Vec<String> {
    match node {
        NodeDsl::Worker(worker) => worker.provider.iter().cloned().collect(),
        NodeDsl::AiDynamic(dynamic) => dynamic
            .bootstrap_provider()
            .map(|provider| vec![provider.to_string()])
            .unwrap_or_default(),
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
            && !supported_modes
                .iter()
                .any(|mode| mode.id == permission_mode)
        {
            return Err(CommandErrorVm::new(
                "workflow.permission-mode-unsupported",
                serde_json::json!({ "agentType": provider, "permissionMode": permission_mode }),
            ));
        }
    }
    Ok(())
}

pub fn command_error(error: anyhow::Error) -> CommandErrorVm {
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
        WorkflowValidationError::WorkerModelBlank { node_id, provider } => CommandErrorVm::new(
            "workflow.model-blank",
            serde_json::json!({ "nodeId": node_id, "provider": provider }),
        ),
        WorkflowValidationError::DynamicFixedModelBlank { node_id } => CommandErrorVm::new(
            "workflow.dynamic-fixed-model-blank",
            serde_json::json!({ "nodeId": node_id }),
        ),
        WorkflowValidationError::DynamicAgentsEmpty { node_id } => CommandErrorVm::new(
            "workflow.dynamic-agents-empty",
            serde_json::json!({ "nodeId": node_id }),
        ),
        WorkflowValidationError::DynamicAgentDuplicate { node_id, provider } => {
            CommandErrorVm::new(
                "workflow.dynamic-agent-duplicate",
                serde_json::json!({ "nodeId": node_id, "provider": provider }),
            )
        }
        WorkflowValidationError::DynamicAgentModelBlank { node_id, provider } => {
            CommandErrorVm::new(
                "workflow.dynamic-agent-model-blank",
                serde_json::json!({ "nodeId": node_id, "provider": provider }),
            )
        }
        WorkflowValidationError::AgentModelBlank { provider } => CommandErrorVm::new(
            "workflow.agent-model-blank",
            serde_json::json!({ "provider": provider }),
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

fn set_acp_config_option_current_value(
    value: &mut serde_json::Value,
    category_or_id: &str,
    next_value: &str,
) {
    let Some(options) = value
        .get_mut("configOptions")
        .and_then(|options| options.as_array_mut())
    else {
        return;
    };
    if let Some(option) = options.iter_mut().find(|option| {
        option.get("id").and_then(|item| item.as_str()) == Some(category_or_id)
            || option.get("category").and_then(|item| item.as_str()) == Some(category_or_id)
    }) {
        if let Some(object) = option.as_object_mut() {
            object.insert(
                "currentValue".to_string(),
                serde_json::Value::String(next_value.to_string()),
            );
        }
    }
}

fn current_acp_session_value(
    attempt_dir: &Utf8PathBuf,
    top_level_key: &str,
    current_key: &str,
    config_category: &str,
) -> Option<String> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return None;
    };
    let value = std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())?;
    value
        .get(top_level_key)
        .and_then(|section| section.get(current_key))
        .and_then(|item| item.as_str())
        .or_else(|| {
            value
                .get("configOptions")
                .and_then(|options| options.as_array())
                .and_then(|options| {
                    options.iter().find(|option| {
                        option.get("id").and_then(|item| item.as_str()) == Some(config_category)
                            || option.get("category").and_then(|item| item.as_str())
                                == Some(config_category)
                    })
                })
                .and_then(|option| option.get("currentValue"))
                .and_then(|item| item.as_str())
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn current_acp_session_model(attempt_dir: &Utf8PathBuf) -> Option<String> {
    current_acp_session_value(attempt_dir, "models", "currentModelId", "model")
}

fn current_acp_session_permission_mode(attempt_dir: &Utf8PathBuf) -> Option<String> {
    current_acp_session_value(attempt_dir, "modes", "currentModeId", "mode")
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
        let index = gold_band::storage::sqlite::search_index().ok_or_else(|| {
            CommandErrorVm::new("search.index-unavailable", serde_json::json!({}))
        })?;
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
pub async fn set_acp_session_model(
    _app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    model_id: String,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return Ok(None);
    };

    let session_json = std::fs::read_to_string(&path).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value: serde_json::Value = serde_json::from_str(&session_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    // Update models.currentModelId
    if let Some(models) = value.get_mut("models").and_then(|m| m.as_object_mut()) {
        models.insert(
            "currentModelId".to_string(),
            serde_json::Value::String(model_id.clone()),
        );
    }
    set_acp_config_option_current_value(&mut value, "model", &model_id);

    let updated_json = serde_json::to_string_pretty(&value).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-serialize-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    std::fs::write(&path, &updated_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-write-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    let vm = if let (Some(on), Some(oa)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
        crate::view_models::dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            on,
            oa,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    } else {
        crate::view_models::acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    };
    Ok(vm.map_err(command_error)?)
}

#[tauri::command]
pub async fn set_acp_session_permission_mode(
    _app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    permission_mode_id: String,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return Ok(None);
    };

    let session_json = std::fs::read_to_string(&path).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value: serde_json::Value = serde_json::from_str(&session_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    // Update modes.currentModeId
    if let Some(modes) = value.get_mut("modes").and_then(|m| m.as_object_mut()) {
        modes.insert(
            "currentModeId".to_string(),
            serde_json::Value::String(permission_mode_id.clone()),
        );
    }
    set_acp_config_option_current_value(&mut value, "mode", &permission_mode_id);

    let updated_json = serde_json::to_string_pretty(&value).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-serialize-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    std::fs::write(&path, &updated_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-write-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    let vm = if let (Some(on), Some(oa)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
        crate::view_models::dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            on,
            oa,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    } else {
        crate::view_models::acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    };
    Ok(vm.map_err(command_error)?)
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
        let index = gold_band::storage::sqlite::search_index().ok_or_else(|| {
            CommandErrorVm::new("search.index-unavailable", serde_json::json!({}))
        })?;
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
        let index = gold_band::storage::sqlite::search_index().ok_or_else(|| {
            CommandErrorVm::new("search.index-unavailable", serde_json::json!({}))
        })?;
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

#[tauri::command]
pub fn open_in_file_manager(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<()> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    // outer_node_id is the container node (e.g. "ai-dynamic"),
    // node_id is the actual dynamic internal node (e.g. "create-hello-world-python-class").
    let path = match (&outer_node_id, &outer_attempt_id, &node_id, &attempt_id) {
        (Some(onid), Some(oaid), nid, aid) => {
            let p = app.paths.dynamic_node_attempt_dir(
                &task_id,
                &run_id,
                &round_id,
                onid,
                oaid,
                nid,
                aid.as_deref().unwrap_or(""),
            );
            eprintln!("[open_in_file_manager] dynamic path: {}", p);
            p
        }
        _ => {
            let p = if let Some(aid) = &attempt_id {
                app.paths
                    .attempt_dir(&task_id, &run_id, &round_id, &node_id, aid)
            } else {
                app.paths.node_dir(&task_id, &run_id, &round_id, &node_id)
            };
            eprintln!("[open_in_file_manager] path: {}", p);
            p
        }
    };
    open_path(path.as_std_path()).map_err(|e| {
        CommandErrorVm::new(
            "file-manager.open-failed",
            serde_json::json!({ "message": e }),
        )
    })
}

fn open_path(path: &std::path::Path) -> Result<(), String> {
    open::that(path).map_err(|e| format!("Failed to open path: {e}"))
}

// ── MCP Server Commands ──

#[tauri::command]
pub fn list_mcp_servers(state: State<'_, DesktopState>) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    Ok(mcp_server_list_vm(
        &app.list_mcp_servers().map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn add_mcp_server(
    state: State<'_, DesktopState>,
    json_content: String,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    Ok(mcp_server_list_vm(
        &app.add_mcp_server(&json_content).map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn update_mcp_server(
    state: State<'_, DesktopState>,
    id: String,
    json_content: String,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    Ok(mcp_server_list_vm(
        &app.update_mcp_server(&id, &json_content)
            .map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn delete_mcp_server(
    state: State<'_, DesktopState>,
    id: String,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    Ok(mcp_server_list_vm(
        &app.delete_mcp_server(&id).map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn toggle_mcp_server(
    state: State<'_, DesktopState>,
    id: String,
    enabled: bool,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    Ok(mcp_server_list_vm(
        &app.toggle_mcp_server(&id, enabled).map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn check_mcp_server_health(
    state: State<'_, DesktopState>,
    id: String,
) -> CommandResult<gold_band::config::McpServerHealthResult> {
    let app = state.app().map_err(command_error)?;
    app.check_mcp_server_health(&id).map_err(command_error)
}

// ── SKILL Commands ──

#[tauri::command]
pub fn list_skills(state: State<'_, DesktopState>) -> CommandResult<SkillListVm> {
    let app = state.app().map_err(command_error)?;
    Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
}

#[tauri::command]
pub fn list_project_skills(
    state: State<'_, DesktopState>,
    workspace_path: String,
) -> CommandResult<Vec<SkillMetaVm>> {
    let app = state.app().map_err(command_error)?;
    let manager = app.skill_manager();
    let skills = manager
        .list_by_workspace(&workspace_path)
        .map_err(command_error)?;
    Ok(skills.iter().map(|s| skill_meta_vm(s)).collect())
}

#[tauri::command]
pub fn read_skill(
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    workspace_path: Option<String>,
) -> CommandResult<SkillContentVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;
    if let Some(ref ws_path) = workspace_path {
        if skill_source == gold_band::config::SkillSource::Project {
            let dir = gold_band::skill::SkillManager::workspace_skills_dir(ws_path);
            let skill_path = dir.join(&name).join(gold_band::config::SKILL_FILE_NAME);
            let raw = std::fs::read_to_string(&skill_path)
                .map_err(|e| command_error(anyhow::anyhow!(e)))?;
            let (meta, body) = gold_band::skill::parse_skill_md_public(
                &raw,
                &name,
                skill_source,
                skill_path.as_str(),
            );
            return Ok(skill_content_vm(&gold_band::skill::SkillContent {
                meta,
                body,
            }));
        }
    }
    Ok(skill_content_vm(
        &app.read_skill(&name, skill_source).map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn write_skill(
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    content: String,
    workspace_path: Option<String>,
    old_name: Option<String>,
) -> CommandResult<SkillListVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;

    // 写入新 SKILL
    if let Some(ref ws_path) = workspace_path {
        if skill_source == gold_band::config::SkillSource::Project {
            app.skill_manager()
                .write_to_workspace(&name, ws_path, &content)
                .map_err(command_error)?;
        } else {
            app.write_skill(&name, skill_source, &content)
                .map_err(command_error)?;
        }
    } else {
        app.write_skill(&name, skill_source, &content)
            .map_err(command_error)?;
    }

    // 同步 symlink 到 .claude/skills/
    app.sync_skill_symlinks(workspace_path.as_deref());

    // 如果改名了，删除旧 SKILL
    if let Some(old) = old_name {
        if old != name {
            if let Some(ref ws_path) = workspace_path {
                if skill_source == gold_band::config::SkillSource::Project {
                    let dir =
                        gold_band::skill::SkillManager::workspace_skills_dir(ws_path).join(&old);
                    if dir.exists() {
                        let _ = std::fs::remove_dir_all(dir.as_std_path());
                    }
                }
            } else {
                let _ = app.delete_skill(&old, skill_source);
            }
            // 改名后旧目录已删，再次 sync 清理旧名 symlink
            app.sync_skill_symlinks(workspace_path.as_deref());
        }
    }

    Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
}

#[tauri::command]
pub fn delete_skill(
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    workspace_path: Option<String>,
) -> CommandResult<SkillListVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;
    if let Some(ref ws_path) = workspace_path {
        if skill_source == gold_band::config::SkillSource::Project {
            let dir = gold_band::skill::SkillManager::workspace_skills_dir(ws_path);
            let skill_dir = dir.join(&name);
            if !skill_dir.exists() {
                return Err(command_error(anyhow::anyhow!("SKILL `{name}` not found")));
            }
            std::fs::remove_dir_all(skill_dir.as_std_path())
                .map_err(|e| command_error(anyhow::anyhow!(e)))?;
            app.sync_skill_symlinks(workspace_path.as_deref());
            return Ok(skill_list_vm(&app.list_skills().map_err(command_error)?));
        }
    }
    app.delete_skill(&name, skill_source)
        .map_err(command_error)?;
    app.sync_skill_symlinks(workspace_path.as_deref());
    Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
}

fn parse_skill_source(source: &str) -> Result<gold_band::config::SkillSource, CommandErrorVm> {
    match source {
        "global" => Ok(gold_band::config::SkillSource::Global),
        "project" => Ok(gold_band::config::SkillSource::Project),
        "built-in" => Ok(gold_band::config::SkillSource::BuiltIn),
        _ => Err(CommandErrorVm::new(
            "skill.invalid-source",
            serde_json::json!({ "source": source }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn canonical_permission_request_id_maps_display_id_to_pending_file_id() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-permission-id-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        gold_band::acp::permission::write_pending_permission(
            &attempt_dir,
            "0",
            serde_json::json!({}),
            "1778771541Z".to_string(),
        )
        .unwrap();

        assert_eq!(
            canonical_permission_request_id(&attempt_dir, "permission-permission-0"),
            "0"
        );

        fs::remove_dir_all(dir).unwrap();
    }
}
