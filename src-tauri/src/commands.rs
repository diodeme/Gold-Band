use std::{collections::BTreeSet, io::{BufRead, BufReader}};

use gold_band::acp::client;
use gold_band::acp::events::{append_ui_event, current_timestamp, permission_decision_event};
use gold_band::acp::permission::write_permission_response;
use gold_band::provider::PromptBundle;
use gold_band::runtime::WorkerRefState;
use gold_band::storage::read_json;

use camino::Utf8PathBuf;
use gold_band::config::{
    DesktopFontPreference, DesktopLanguage, DesktopThemePreference, RuntimeConfig,
};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::i18n::Translator;
use crate::state::DesktopState;
use crate::view_models::{
    AcpRawFramePageVm, AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AppBootstrapVm, ContentVm,
    LogPageVm, LogQueryInput, PreferencesVm, RoundDetailVm, RoundSelectionInput, RunDetailVm,
    RunSummaryVm, TaskDetailVm, TaskListVm, WorkflowVm, acp_raw_frame_page_vm,
    acp_session_vm, bootstrap_vm, log_page_vm, preferences_vm, round_detail_vm, run_detail_vm,
    run_summary_vm, task_detail_vm, task_list_vm, workflow_vm,
};

pub type CommandResult<T> = Result<T, String>;

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
pub fn get_app_bootstrap(state: State<'_, DesktopState>) -> CommandResult<AppBootstrapVm> {
    let context = state.context().map_err(command_error)?;
    Ok(bootstrap_vm(&context.app(), context.recent_workspaces))
}

#[tauri::command]
pub fn get_task_list(state: State<'_, DesktopState>) -> CommandResult<TaskListVm> {
    let app = state.app().map_err(command_error)?;
    task_list_vm(&app).map_err(command_error)
}

#[tauri::command]
pub fn choose_workspace(
    app: tauri::AppHandle,
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
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve selected workspace path: {error}"))?;
    let repo_root = Utf8PathBuf::from_path_buf(path)
        .map_err(|_| "selected workspace path is not valid UTF-8".to_string())?;
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    Ok(Some(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
    )))
}

#[tauri::command]
pub fn select_recent_workspace(
    state: State<'_, DesktopState>,
    workspace: String,
) -> CommandResult<AppBootstrapVm> {
    let repo_root = Utf8PathBuf::from(workspace);
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    Ok(bootstrap_vm(&context.app(), context.recent_workspaces))
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
pub fn get_workflow(state: State<'_, DesktopState>, task_id: String) -> CommandResult<WorkflowVm> {
    let app = state.app().map_err(command_error)?;
    workflow_vm(&app, &task_id).map_err(command_error)
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
) -> CommandResult<RunSummaryVm> {
    let app = state.app().map_err(command_error)?;
    app.run_continue(&task_id, &run_id)
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
    acp_session_vm(&app, &task_id, &run_id, &round_id, &node_id, &attempt_id, query).map_err(command_error)
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
) -> CommandResult<Option<AcpSessionVm>> {
    let app = state.app().map_err(command_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        let attempt_dir =
            app.paths
                .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let worker_ref_path =
            app.paths
                .worker_ref_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let continue_ref = if worker_ref_path.exists() {
            read_json::<WorkerRefState>(&worker_ref_path)
                .map_err(command_error)?
                .continue_ref
        } else {
            None
        };
        client::run_prompt(
            &app.config.acp_adapter,
            app.paths.repo_root.clone(),
            attempt_dir,
            &PromptBundle {
                system_prompt: String::new(),
                user_prompt: prompt,
            },
            continue_ref,
        )
        .map_err(command_error)?;
        acp_session_vm(&app, &task_id, &run_id, &round_id, &node_id, &attempt_id, None)
            .map_err(command_error)
    })
    .await
    .map_err(|error| error.to_string())?
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
    acp_session_vm(&app, &task_id, &run_id, &round_id, &node_id, &attempt_id, None).map_err(command_error)
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
    let marker = app
        .paths
        .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id)
        .join("acp.cancel-requested");
    gold_band::storage::ensure_parent_dir(&marker).map_err(command_error)?;
    std::fs::write(marker.as_std_path(), current_timestamp())
        .map_err(|error| command_error(error.into()))?;
    acp_session_vm(&app, &task_id, &run_id, &round_id, &node_id, &attempt_id, None).map_err(command_error)
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
    .map_err(|error| error.to_string())?
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

fn command_error(error: anyhow::Error) -> String {
    error.to_string()
}
