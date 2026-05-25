use gold_band::acp::client;
use gold_band::acp::events::{append_ui_event, current_timestamp, permission_decision_event};
use gold_band::acp::permission::{
    cancel_pending_permission_requests, request_cancel, write_permission_response,
};
use gold_band::app::{
    CreateTaskInput, ProfileEntry, ProfileInput, ProfileList, WorkflowTemplateStore,
};
use gold_band::domain::{NodeOutcome, SessionMode};
use gold_band::dsl::{NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::provider::supported_modes_from_capabilities;
use gold_band::runtime::{NodeState, WorkerRefState};
use gold_band::storage::read_json;
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader},
    str::FromStr,
};

use camino::Utf8PathBuf;
use gold_band::config::{
    AcpAdapterConfig, DesktopFontPreference, DesktopLanguage, DesktopThemePreference,
    ManagedAgentConfig, ManagedAgentType, RuntimeConfig,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::i18n::Translator;
use crate::state::DesktopState;
use crate::updater::{
    UpdateStatusVm, UpdaterSettingsVm, check_update,
    download_and_install_update as run_download_and_install_update, normalize_updater_url_override,
    updater_settings,
};
use crate::view_models::{
    AcpRawFramePageVm, AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AgentRegistryVm,
    AppBootstrapVm, ContentVm, LogPageVm, LogQueryInput, PreferencesVm, RoundDetailVm,
    RoundSelectionInput, RunDetailVm, RunSummaryVm, TaskDetailVm, TaskListVm, WorkflowVm,
    acp_raw_frame_page_vm, acp_session_vm, agent_registry_vm, bootstrap_vm, log_page_vm,
    preferences_vm, round_detail_vm, run_detail_vm, run_summary_vm, task_detail_vm, task_list_vm,
    workflow_vm,
};

pub type CommandResult<T> = Result<T, CommandErrorVm>;

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
    pub requirement_file_name: String,
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
    let user_config = app
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
    let config = RuntimeConfig::default().apply_user_config(&user_config);
    state.update_config(config).map_err(command_error)?;
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
    let user_config = app
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
    let config = RuntimeConfig::default().apply_user_config(&user_config);
    state.update_config(config).map_err(command_error)?;
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
    let user_config = app
        .remove_managed_agent(agent_type)
        .map_err(command_error)?;
    let config = RuntimeConfig::default().apply_user_config(&user_config);
    state.update_config(config).map_err(command_error)?;
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
pub fn start_run(state: State<'_, DesktopState>, task_id: String) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
    app.run_start_background(&task_id, None)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn continue_run(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    prompt_id: Option<String>,
) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
    app.run_continue_background(&task_id, &run_id, prompt_id)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn submit_manual_check(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outcome: String,
) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
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
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
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
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    app.artifact_show(&task_id, &run_id, &round_id, &node_id, &attempt_id, &name)
        .map(|content| ContentVm {
            title: labels.format("detail.artifact", &name),
            kind: "artifact".to_string(),
            content,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id }),
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
pub fn get_acp_session(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpSessionQueryInput>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
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
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    tauri::async_runtime::spawn_blocking(move || {
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
        client::run_prompt(
            provider,
            &agent_config.adapter,
            app.paths.repo_root.clone(),
            attempt_dir,
            &prompt_bundle,
            session_mode,
            permission_mode,
            continue_ref,
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
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub fn respond_acp_permission(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    request_id: String,
    option_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
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
    let seq = next_acp_event_seq(&events_path);
    append_ui_event(
        &events_path,
        &permission_decision_event(seq, request_id, option_id),
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

#[tauri::command]
pub fn cancel_acp_session(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    let attempt_dir = app
        .paths
        .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
    let requested_at = current_timestamp();
    request_cancel(&attempt_dir, requested_at.clone()).map_err(command_error)?;
    cancel_pending_permission_requests(&attempt_dir, requested_at).map_err(command_error)?;
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
) -> CommandResult<AcpRawFramePageVm> {
    let app = state.app().map_err(command_error)?;
    tauri::async_runtime::spawn_blocking(move || {
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
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    app.attachment_show(&task_id, &run_id, &round_id, &node_id, &attempt_id, &name)
        .map(|content| ContentVm {
            title: labels.format("detail.attachment", &name),
            kind: "attachment".to_string(),
            content,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id }),
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
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    app.worker_ref_show(&task_id, &run_id, &round_id, &node_id, &attempt_id)
        .map(|content| ContentVm {
            title: labels.format("detail.workerRef", &node_id),
            kind: "worker-ref".to_string(),
            content: content.unwrap_or_else(|| labels.tr("fallback.missingWorkerRef")),
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id }),
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn save_desktop_preferences(
    state: State<'_, DesktopState>,
    theme: DesktopThemePreference,
    language: DesktopLanguage,
    font: DesktopFontPreference,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let user_config = app
        .set_user_desktop_preferences(theme, language, font.clone())
        .map_err(command_error)?;
    let config = RuntimeConfig::default().apply_user_config(&user_config);
    state.update_config(config).map_err(command_error)?;
    Ok(preferences_vm(theme, language, font))
}

#[tauri::command]
pub fn save_updater_settings(
    state: State<'_, DesktopState>,
    override_url: Option<String>,
) -> CommandResult<UpdaterSettingsVm> {
    let override_url = normalize_updater_url_override(override_url).map_err(command_error)?;
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let user_config = app
        .set_user_desktop_updater_url_override(override_url)
        .map_err(command_error)?;
    let config = RuntimeConfig::default().apply_user_config(&user_config);
    let settings = updater_settings(&config);
    state.update_config(config).map_err(command_error)?;
    Ok(settings)
}

#[tauri::command]
pub fn get_update_status(state: State<'_, DesktopState>) -> CommandResult<UpdateStatusVm> {
    state.update_status().map_err(command_error)
}

#[tauri::command]
pub async fn check_update_manual(app: AppHandle) -> CommandResult<UpdateStatusVm> {
    Ok(check_update(&app, false).await)
}

#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> CommandResult<()> {
    run_download_and_install_update(&app).await.map_err(command_error)
}

fn ensure_workflow_agents_doctor_ready(
    state: &DesktopState,
    workflow: &WorkflowDsl,
) -> CommandResult<()> {
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    let mut providers = BTreeSet::new();
    for node in &workflow.nodes {
        if let Some(provider) = node.provider() {
            providers.insert(provider.to_string());
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
        let NodeDsl::Worker(worker) = node;
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
    let message = error.to_string();
    if let Some(code) = updater_command_error_code(&message) {
        return CommandErrorVm::new(code, serde_json::json!({ "message": message }));
    }
    CommandErrorVm::new("app.unexpected", serde_json::json!({}))
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
        WorkflowValidationError::SuccessNewRoundTarget { from } => CommandErrorVm::new(
            "workflow.success-new-round-target",
            serde_json::json!({ "from": from }),
        ),
    }
}
