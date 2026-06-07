use camino::Utf8PathBuf;
use gold_band::config::{
    ConversationAutoConfig, ConversationPin, ConversationRunModeEntry, ConversationWorkspaceEntry,
    DesktopUiMode,
};
use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{CommandErrorVm, CommandResult, command_error};
use crate::state::DesktopState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunModeSettingsVm {
    pub mode: String,
    pub workflow_template_id: Option<String>,
    pub auto_config: Option<crate::view_models_conversation::ConversationAutoConfigVm>,
}

#[tauri::command]
pub fn save_desktop_ui_mode(
    state: State<'_, DesktopState>,
    mode: String,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut conv_state = app.load_conversation_state().map_err(command_error)?;
    conv_state.desktop_ui_mode = Some(match mode.as_str() {
        "workbench" => DesktopUiMode::Workbench,
        _ => DesktopUiMode::Conversation,
    });
    app.save_conversation_state(&conv_state)
        .map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn get_conversation_sidebar(
    state: State<'_, DesktopState>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let conv_state = app.load_conversation_state().map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(&app, &conv_state))
}

#[tauri::command]
pub fn get_conversation_run(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    run_id: String,
    selected_session_key: Option<String>,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    eprintln!("[cmd] get_conversation_run project={project_id} task={task_id} run={run_id} session_key={selected_session_key:?}");
    let app = state.app().map_err(command_error)?;
    let result = crate::view_models_conversation::conversation_run_vm(
        &app,
        &project_id,
        &task_id,
        &run_id,
        selected_session_key.as_deref(),
    )
    .map_err(command_error);
    match &result {
        Ok(vm) => eprintln!("[cmd] get_conversation_run OK: title={} rounds={} has_session={}", vm.title, vm.session_tree.rounds.len(), vm.selected_session.is_some()),
        Err(e) => eprintln!("[cmd] get_conversation_run ERROR: {e:?}"),
    }
    result
}

#[tauri::command]
pub fn validate_conversation_create(
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::ConversationCreateInputVm,
) -> CommandResult<crate::view_models_conversation::ConversationValidationResultVm> {
    let app = state.app().map_err(command_error)?;
    crate::view_models_conversation::validate_conversation_create_vm(&app, &input)
        .map_err(command_error)
}

#[tauri::command]
pub fn create_conversation_run(
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::ConversationCreateInputVm,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let app = state.app().map_err(command_error)?;
    crate::view_models_conversation::create_conversation_run_vm(&app, &input)
        .map_err(command_error)
}

#[tauri::command]
pub fn rerun_conversation_task(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let app = state.app().map_err(command_error)?;
    crate::view_models_conversation::rerun_conversation_task_vm(&app, &project_id, &task_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_task_metadata(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    title: String,
    description: Option<String>,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    crate::view_models_conversation::update_task_metadata_vm(
        &app, &project_id, &task_id, &title, description.as_deref(),
    )
    .map_err(command_error)
}

#[tauri::command]
pub fn pin_conversation(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut conv_state = app.load_conversation_state().map_err(command_error)?;
    let max_order = conv_state
        .conversation_pins
        .iter()
        .map(|p| p.order)
        .max()
        .unwrap_or(0);
    conv_state.conversation_pins.push(ConversationPin {
        project_id,
        task_id,
        order: max_order + 1,
    });
    app.save_conversation_state(&conv_state)
        .map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(&app, &conv_state))
}

#[tauri::command]
pub fn unpin_conversation(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut conv_state = app.load_conversation_state().map_err(command_error)?;
    conv_state
        .conversation_pins
        .retain(|p| p.project_id != project_id || p.task_id != task_id);
    app.save_conversation_state(&conv_state)
        .map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(&app, &conv_state))
}

#[tauri::command]
pub fn reorder_pinned_conversations(
    state: State<'_, DesktopState>,
    ordered: Vec<gold_band::config::ConversationPin>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut conv_state = app.load_conversation_state().map_err(command_error)?;
    conv_state.conversation_pins = ordered
        .into_iter()
        .enumerate()
        .map(|(i, mut pin)| {
            pin.order = i;
            pin
        })
        .collect();
    app.save_conversation_state(&conv_state)
        .map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(&app, &conv_state))
}

#[tauri::command]
pub fn search_conversation_tasks(
    state: State<'_, DesktopState>,
    query: String,
    limit: Option<usize>,
) -> CommandResult<Vec<crate::view_models_conversation::ConversationSearchResultVm>> {
    let limit = limit.unwrap_or(50).min(200);
    let app = state.app().map_err(command_error)?;
    let conv_state = app.load_conversation_state().unwrap_or_default();
    if let Some(index) = gold_band::storage::sqlite::search_index() {
        index
            .search_tasks(&query, limit)
            .map(|results| {
                results
                    .into_iter()
                    .map(|r| {
                        let (project_id, workspace_name) =
                            extract_project_from_task_path(&r.task_path, &conv_state);
                        crate::view_models_conversation::ConversationSearchResultVm {
                            project_id,
                            workspace_path: String::new(),
                            workspace_name,
                            task_id: r.task_id,
                            title: r.title,
                            description: Some(r.description),
                            requirement_preview: r.requirement_preview,
                            latest_run: None,
                        }
                    })
                    .collect()
            })
            .map_err(|e| {
                CommandErrorVm::new(
                    "search.query-failed",
                    serde_json::json!({ "message": e.to_string() }),
                )
            })
    } else {
        Ok(Vec::new())
    }
}

fn extract_project_from_task_path(
    task_path: &str,
    conv_state: &gold_band::config::ConversationState,
) -> (String, String) {
    // Path structure: .../projects/{project_id}/tasks/{task_id}
    let path = task_path.replace('\\', "/");
    let segments: Vec<&str> = path.split('/').collect();
    let mut project_id = String::new();
    for i in 0..segments.len().saturating_sub(1) {
        if segments[i] == "projects" {
            project_id = segments.get(i + 1).map(|s| s.to_string()).unwrap_or_default();
            break;
        }
    }
    let workspace_name = conv_state
        .conversation_workspaces
        .iter()
        .find(|w| w.project_id == project_id)
        .map(|w| w.name.clone())
        .unwrap_or(project_id.clone());
    (project_id, workspace_name)
}

#[tauri::command]
pub fn get_conversation_run_mode(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<Option<crate::view_models_conversation::ConversationRunModeVm>> {
    let app = state.app().map_err(command_error)?;
    let conv_state = app.load_conversation_state().map_err(command_error)?;
    Ok(conv_state
        .conversation_run_modes
        .get(&project_id)
        .map(|entry| crate::view_models_conversation::ConversationRunModeVm {
            mode: entry.mode.clone(),
            workflow_template_id: entry.workflow_template_id.clone(),
            auto_config: entry.auto_config.as_ref().map(|cfg| {
                crate::view_models_conversation::ConversationAutoConfigVm {
                    agent_type: cfg.agent_type.clone(),
                    model_id: cfg.model_id.clone(),
                    permission_mode: cfg.permission_mode.clone(),
                    allowed_profiles: cfg.allowed_profiles.clone(),
                    global_goal: cfg.global_goal.clone(),
                }
            }),
        }))
}

#[tauri::command]
pub fn save_conversation_run_mode(
    state: State<'_, DesktopState>,
    project_id: String,
    settings: ConversationRunModeSettingsVm,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut conv_state = app.load_conversation_state().map_err(command_error)?;
    conv_state.conversation_run_modes.insert(
        project_id,
        ConversationRunModeEntry {
            mode: settings.mode,
            workflow_template_id: settings.workflow_template_id,
            auto_config: settings.auto_config.map(|cfg| ConversationAutoConfig {
                agent_type: cfg.agent_type,
                model_id: cfg.model_id,
                permission_mode: cfg.permission_mode,
                allowed_profiles: cfg.allowed_profiles,
                global_goal: cfg.global_goal,
            }),
        },
    );
    app.save_conversation_state(&conv_state)
        .map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn choose_conversation_workspace(
    state: State<'_, DesktopState>,
) -> CommandResult<crate::view_models_conversation::ConversationWorkspaceVm> {
    let context = state.context().map_err(command_error)?;
    let workspace_path = context.repo_root.to_string();
    let name = std::path::Path::new(&workspace_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace_path.clone());
    let project_id = workspace_path
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");
    Ok(crate::view_models_conversation::ConversationWorkspaceVm {
        project_id,
        workspace_path,
        name,
    })
}

#[tauri::command]
pub fn add_conversation_workspace(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let gold_band_app = state.app().map_err(command_error)?;

    // Open directory picker via Tauri dialog plugin
    let Some(path) = app_handle
        .dialog()
        .file()
        .blocking_pick_folder()
    else {
        return Err(CommandErrorVm::new("workspace.cancelled", serde_json::json!({})));
    };
    let path = path.into_path().map_err(|_| {
        CommandErrorVm::new("workspace.path-invalid", serde_json::json!({}))
    })?;
    let workspace_path = Utf8PathBuf::from_path_buf(path).map_err(|_| {
        CommandErrorVm::new("workspace.path-invalid-utf8", serde_json::json!({}))
    })?;
    let workspace_path_str = workspace_path.as_str().to_string();

    let name = std::path::Path::new(&workspace_path_str)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace_path_str.clone());
    let project_id = workspace_path_str
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");

    let mut conv_state = gold_band_app.load_conversation_state().map_err(command_error)?;

    // Ensure default workspace is persisted in stored state
    let default_repo = gold_band_app.paths.repo_root.to_string();
    let default_id = default_repo
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");
    if default_id != project_id && !conv_state.conversation_workspaces.iter().any(|w| w.project_id == default_id) {
        let default_name = std::path::Path::new(&default_repo)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| default_repo.clone());
        conv_state.conversation_workspaces.push(ConversationWorkspaceEntry {
            project_id: default_id.clone(),
            workspace_path: default_repo,
            name: default_name,
            added_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    // Check not already added
    if conv_state.conversation_workspaces.iter().any(|w| w.project_id == project_id) {
        return Err(CommandErrorVm::new(
            "workspace.already-exists",
            serde_json::json!({ "name": name }),
        ));
    }

    conv_state.conversation_workspaces.push(ConversationWorkspaceEntry {
        project_id: project_id.clone(),
        workspace_path: workspace_path_str,
        name: name.clone(),
        added_at: chrono::Utc::now().to_rfc3339(),
    });
    conv_state.last_conversation_workspace = Some(project_id.clone());
    gold_band_app.save_conversation_state(&conv_state).map_err(command_error)?;

    Ok(crate::view_models_conversation::conversation_sidebar_vm(&gold_band_app, &conv_state))
}

#[tauri::command]
pub fn save_conversation_preference(
    state: State<'_, DesktopState>,
    key: String,
    value: serde_json::Value,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut app_state = app.load_state().map_err(command_error)?;
    app_state.preferences.insert(key, value);
    app.save_state(&app_state).map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn sync_conversation_workspace(
    state: State<'_, DesktopState>,
    workspace_path: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let name = std::path::Path::new(&workspace_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace_path.clone());
    let project_id = workspace_path
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");

    let mut conv_state = app.load_conversation_state().map_err(command_error)?;

    if !conv_state.conversation_workspaces.iter().any(|w| w.project_id == project_id) {
        conv_state.conversation_workspaces.push(ConversationWorkspaceEntry {
            project_id: project_id.clone(),
            workspace_path: workspace_path.clone(),
            name: name.clone(),
            added_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    conv_state.last_conversation_workspace = Some(project_id);
    app.save_conversation_state(&conv_state).map_err(command_error)?;

    Ok(crate::view_models_conversation::conversation_sidebar_vm(&app, &conv_state))
}

#[tauri::command]
pub fn remove_conversation_workspace(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut conv_state = app.load_conversation_state().map_err(command_error)?;

    conv_state.conversation_workspaces.retain(|w| w.project_id != project_id);
    // Also clean up pins and run modes for this workspace
    conv_state.conversation_pins.retain(|p| p.project_id != project_id);
    conv_state.conversation_run_modes.remove(&project_id);
    if conv_state.last_conversation_workspace.as_deref() == Some(&project_id) {
        conv_state.last_conversation_workspace = conv_state
            .conversation_workspaces
            .first()
            .map(|w| w.project_id.clone());
    }
    app.save_conversation_state(&conv_state).map_err(command_error)?;

    Ok(crate::view_models_conversation::conversation_sidebar_vm(&app, &conv_state))
}
