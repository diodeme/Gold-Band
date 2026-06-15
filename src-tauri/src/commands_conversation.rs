use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::config::{
    ConversationAllowedWorkflowRef, ConversationAutoConfig, ConversationDynamicAgentRef,
    ConversationDynamicControl, ConversationPin, ConversationRunModeEntry,
    ConversationWorkspaceEntry, DesktopUiMode,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use crate::commands::{
    CommandErrorVm, CommandResult, acp_live_update_emitter, acp_session_update_emitter,
    command_error,
};
use crate::state::DesktopContext;
use crate::state::DesktopState;
use crate::view_models::ContentVm;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunModeSettingsVm {
    pub mode: String,
    pub workflow_template_id: Option<String>,
    pub auto_config: Option<crate::view_models_conversation::ConversationAutoConfigVm>,
}

#[tauri::command]
pub fn save_desktop_ui_mode(state: State<'_, DesktopState>, mode: String) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut state = app.load_state().map_err(command_error)?;
    state.desktop_ui_mode = Some(match mode.as_str() {
        "workbench" => DesktopUiMode::Workbench,
        _ => DesktopUiMode::Conversation,
    });
    app.save_state(&state).map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub fn get_conversation_sidebar(
    state: State<'_, DesktopState>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let state = app.load_state().map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &state,
    ))
}

#[tauri::command]
pub fn get_conversation_run(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    run_id: String,
    selected_session_key: Option<String>,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let app = state.app().map_err(command_error)?;
    let result = crate::view_models_conversation::conversation_run_vm(
        &app,
        &project_id,
        &task_id,
        &run_id,
        selected_session_key.as_deref(),
    )
    .map_err(command_error);
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
pub async fn create_conversation_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::ConversationCreateInputVm,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app_with_metrics(
        acp_live_update_emitter(app_handle.clone()),
        acp_session_update_emitter(app_handle.clone(), context.app()),
        crate::metrics::create_metrics_callback(app_handle),
    );
    tauri::async_runtime::spawn_blocking(move || {
        crate::view_models_conversation::create_conversation_run_vm(&app, &input)
            .map_err(command_error)
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub fn rerun_conversation_task(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app_with_metrics(
        acp_live_update_emitter(app_handle.clone()),
        acp_session_update_emitter(app_handle.clone(), context.app()),
        crate::metrics::create_metrics_callback(app_handle),
    );
    crate::view_models_conversation::rerun_conversation_task_vm(&app, &project_id, &task_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn switch_conversation_session(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<crate::view_models_conversation::ConversationSessionSwitchVm> {
    let app = state.app().map_err(command_error)?;
    crate::view_models_conversation::switch_conversation_session_vm(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    )
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
        &app,
        &project_id,
        &task_id,
        &title,
        description.as_deref(),
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
    let mut state = app.load_state().map_err(command_error)?;
    let max_order = state
        .conversation_pins
        .iter()
        .map(|p| p.order)
        .max()
        .unwrap_or(0);
    state.conversation_pins.push(ConversationPin {
        project_id,
        task_id,
        order: max_order + 1,
    });
    app.save_state(&state).map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &state,
    ))
}

#[tauri::command]
pub fn unpin_conversation(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut state = app.load_state().map_err(command_error)?;
    state
        .conversation_pins
        .retain(|p| p.project_id != project_id || p.task_id != task_id);
    app.save_state(&state).map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &state,
    ))
}

#[tauri::command]
pub fn reorder_pinned_conversations(
    state: State<'_, DesktopState>,
    ordered: Vec<gold_band::config::ConversationPin>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut state = app.load_state().map_err(command_error)?;
    state.conversation_pins = ordered
        .into_iter()
        .enumerate()
        .map(|(i, mut pin)| {
            pin.order = i;
            pin
        })
        .collect();
    app.save_state(&state).map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &state,
    ))
}

#[tauri::command]
pub fn search_conversation_tasks(
    state: State<'_, DesktopState>,
    query: String,
    limit: Option<usize>,
) -> CommandResult<Vec<crate::view_models_conversation::ConversationSearchResultVm>> {
    let limit = limit.unwrap_or(50).min(200);
    let app = state.app().map_err(command_error)?;
    let state = app.load_state().unwrap_or_default();
    if let Some(index) = gold_band::storage::sqlite::search_index() {
        index
            .search_tasks(&query, limit)
            .map(|results| {
                results
                    .into_iter()
                    .map(|r| {
                        let (project_id, workspace_name) =
                            extract_project_from_task_path(&r.task_path, &state);
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
    state: &gold_band::config::StateConfig,
) -> (String, String) {
    // Path structure: .../projects/{project_id}/tasks/{task_id}
    let path = task_path.replace('\\', "/");
    let segments: Vec<&str> = path.split('/').collect();
    let mut project_id = String::new();
    for i in 0..segments.len().saturating_sub(1) {
        if segments[i] == "projects" {
            project_id = segments
                .get(i + 1)
                .map(|s| s.to_string())
                .unwrap_or_default();
            break;
        }
    }
    let workspace_name = state
        .conversation_workspaces
        .iter()
        .find(|w| w.project_id == project_id)
        .map(|w| w.name.clone())
        .unwrap_or(project_id.clone());
    (project_id, workspace_name)
}

fn workspace_entry_for_project(
    app: &App,
    state: &gold_band::config::StateConfig,
    project_id: &str,
) -> Option<(String, String)> {
    let default_repo = app.paths.repo_root.to_string();
    let default_project_id = default_repo
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");
    if project_id == default_project_id {
        return Some((default_repo, project_id.to_string()));
    }
    state
        .conversation_workspaces
        .iter()
        .find(|w| w.project_id == project_id)
        .map(|w| (w.workspace_path.clone(), w.project_id.clone()))
}

fn app_for_workspace(context: &DesktopContext, workspace_path: &str) -> anyhow::Result<App> {
    let repo_root = Utf8PathBuf::from(workspace_path);
    Ok(App::with_config(repo_root, context.config.clone()))
}

#[tauri::command]
pub fn get_conversation_run_mode(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<Option<crate::view_models_conversation::ConversationRunModeVm>> {
    let app = state.app().map_err(command_error)?;
    let state = app.load_state().map_err(command_error)?;
    Ok(state.conversation_run_modes.get(&project_id).map(|entry| {
        crate::view_models_conversation::ConversationRunModeVm {
            mode: entry.mode.clone(),
            workflow_template_id: entry.workflow_template_id.clone(),
            auto_config: entry.auto_config.as_ref().map(|cfg| {
                crate::view_models_conversation::ConversationAutoConfigVm {
                    agent_strategy: cfg.agent_strategy.clone(),
                    agent_type: cfg.agent_type.clone(),
                    bootstrap_agent_type: cfg.bootstrap_agent_type.clone(),
                    bootstrap_model_id: cfg.bootstrap_model_id.clone(),
                    acceptance_model_id: cfg.acceptance_model_id.clone(),
                    model_id: cfg.model_id.clone(),
                    permission_mode: cfg.permission_mode.clone(),
                    available_agents: cfg.available_agents.as_ref().map(|agents| {
                        agents
                            .iter()
                            .map(|agent| {
                                crate::view_models_conversation::ConversationDynamicAgentRefVm {
                                    provider: agent.provider.clone(),
                                    model: agent.model.clone(),
                                }
                            })
                            .collect()
                    }),
                    routing_prompt: cfg.routing_prompt.clone(),
                    allowed_workflows: cfg.allowed_workflows.as_ref().map(|workflows| {
                        workflows
                            .iter()
                            .map(|workflow| {
                                crate::view_models_conversation::ConversationAllowedWorkflowRefVm {
                                    workflow_id: workflow.workflow_id.clone(),
                                }
                            })
                            .collect()
                    }),
                    allowed_profiles: cfg.allowed_profiles.clone(),
                    global_goal: cfg.global_goal.clone(),
                    control: cfg.control.as_ref().map(|control| {
                        crate::view_models_conversation::ConversationDynamicControlVm {
                            max_dynamic_nodes: control.max_dynamic_nodes,
                            max_fanout: control.max_fanout,
                            max_depth: control.max_depth,
                            max_parallel: control.max_parallel,
                            max_group_depth: control.max_group_depth,
                            max_workflow_invocations: control.max_workflow_invocations,
                            allow_nested_dynamic: control.allow_nested_dynamic,
                        }
                    }),
                    active_template_id: cfg.active_template_id.clone(),
                    active_template_name: cfg.active_template_name.clone(),
                }
            }),
        }
    }))
}

#[tauri::command]
pub fn save_conversation_run_mode(
    state: State<'_, DesktopState>,
    project_id: String,
    settings: ConversationRunModeSettingsVm,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut state = app.load_state().map_err(command_error)?;
    state.conversation_run_modes.insert(
        project_id,
        ConversationRunModeEntry {
            mode: settings.mode,
            workflow_template_id: settings.workflow_template_id,
            auto_config: settings.auto_config.map(|cfg| ConversationAutoConfig {
                agent_strategy: cfg.agent_strategy,
                agent_type: cfg.agent_type,
                bootstrap_agent_type: cfg.bootstrap_agent_type,
                bootstrap_model_id: cfg.bootstrap_model_id,
                acceptance_model_id: cfg.acceptance_model_id,
                model_id: cfg.model_id,
                permission_mode: cfg.permission_mode,
                available_agents: cfg.available_agents.map(|agents| {
                    agents
                        .into_iter()
                        .map(|agent| ConversationDynamicAgentRef {
                            provider: agent.provider,
                            model: agent.model,
                        })
                        .collect()
                }),
                routing_prompt: cfg.routing_prompt,
                allowed_workflows: cfg.allowed_workflows.map(|workflows| {
                    workflows
                        .into_iter()
                        .map(|workflow| ConversationAllowedWorkflowRef {
                            workflow_id: workflow.workflow_id,
                        })
                        .collect()
                }),
                allowed_profiles: cfg.allowed_profiles,
                global_goal: cfg.global_goal,
                control: cfg.control.map(|control| ConversationDynamicControl {
                    max_dynamic_nodes: control.max_dynamic_nodes,
                    max_fanout: control.max_fanout,
                    max_depth: control.max_depth,
                    max_parallel: control.max_parallel,
                    max_group_depth: control.max_group_depth,
                    max_workflow_invocations: control.max_workflow_invocations,
                    allow_nested_dynamic: control.allow_nested_dynamic,
                }),
                active_template_id: cfg.active_template_id,
                active_template_name: cfg.active_template_name,
            }),
        },
    );
    app.save_state(&state).map_err(command_error)?;
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
    let Some(path) = app_handle.dialog().file().blocking_pick_folder() else {
        return Err(CommandErrorVm::new(
            "workspace.cancelled",
            serde_json::json!({}),
        ));
    };
    let path = path
        .into_path()
        .map_err(|_| CommandErrorVm::new("workspace.path-invalid", serde_json::json!({})))?;
    let workspace_path = Utf8PathBuf::from_path_buf(path)
        .map_err(|_| CommandErrorVm::new("workspace.path-invalid-utf8", serde_json::json!({})))?;
    let workspace_path_str = workspace_path.as_str().to_string();

    let name = std::path::Path::new(&workspace_path_str)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace_path_str.clone());
    let project_id = workspace_path_str
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");

    let mut state = gold_band_app.load_state().map_err(command_error)?;

    // Ensure default workspace is persisted in stored state
    let default_repo = gold_band_app.paths.repo_root.to_string();
    let default_id = default_repo
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");
    if default_id != project_id
        && !state
            .conversation_workspaces
            .iter()
            .any(|w| w.project_id == default_id)
    {
        let default_name = std::path::Path::new(&default_repo)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| default_repo.clone());
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: default_id.clone(),
                workspace_path: default_repo,
                name: default_name,
                added_at: chrono::Utc::now().to_rfc3339(),
            });
    }

    // Check not already added
    if state
        .conversation_workspaces
        .iter()
        .any(|w| w.project_id == project_id)
    {
        return Err(CommandErrorVm::new(
            "workspace.already-exists",
            serde_json::json!({ "name": name }),
        ));
    }

    state
        .conversation_workspaces
        .push(ConversationWorkspaceEntry {
            project_id: project_id.clone(),
            workspace_path: workspace_path_str,
            name: name.clone(),
            added_at: chrono::Utc::now().to_rfc3339(),
        });
    state.last_conversation_workspace = Some(project_id.clone());
    gold_band_app.save_state(&state).map_err(command_error)?;

    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &gold_band_app,
        &state,
    ))
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
pub fn save_last_conversation_workspace(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut app_state = app.load_state().map_err(command_error)?;
    app_state.last_conversation_workspace = Some(project_id);
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

    let mut state = app.load_state().map_err(command_error)?;

    if !state
        .conversation_workspaces
        .iter()
        .any(|w| w.project_id == project_id)
    {
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: project_id.clone(),
                workspace_path: workspace_path.clone(),
                name: name.clone(),
                added_at: chrono::Utc::now().to_rfc3339(),
            });
    }
    state.last_conversation_workspace = Some(project_id);
    app.save_state(&state).map_err(command_error)?;

    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &state,
    ))
}

#[tauri::command]
pub fn delete_conversation_task(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let mut app_state = app.load_state().map_err(command_error)?;
    let Some((workspace_path, normalized_project_id)) =
        workspace_entry_for_project(&app, &app_state, &project_id)
    else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": project_id }),
        ));
    };
    let workspace_app = app_for_workspace(&context, &workspace_path).map_err(command_error)?;
    let task_dir = workspace_app.paths.task_dir(&task_id);
    if !task_dir.exists() {
        return Err(CommandErrorVm::new(
            "conversation.task-not-found",
            serde_json::json!({ "taskId": task_id }),
        ));
    }
    if let Ok(runs) = workspace_app.run_list(&task_id) {
        if runs
            .iter()
            .any(|run| run.status == gold_band::domain::RunStatus::Running)
        {
            return Err(CommandErrorVm::new(
                "conversation.task-running",
                serde_json::json!({ "taskId": task_id }),
            ));
        }
    }
    trash::delete(task_dir.as_std_path()).map_err(|error| {
        CommandErrorVm::new(
            "conversation.task-delete-failed",
            serde_json::json!({ "taskId": task_id, "message": error.to_string() }),
        )
    })?;
    gold_band::storage::sqlite::delete_task(&task_id);
    app_state
        .conversation_pins
        .retain(|p| p.project_id != normalized_project_id || p.task_id != task_id);
    app.save_state(&app_state).map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &app_state,
    ))
}

#[tauri::command]
pub fn remove_conversation_workspace(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let app = state.app().map_err(command_error)?;
    let mut state = app.load_state().map_err(command_error)?;

    state
        .conversation_workspaces
        .retain(|w| w.project_id != project_id);
    // Also clean up pins and run modes for this workspace
    state
        .conversation_pins
        .retain(|p| p.project_id != project_id);
    state.conversation_run_modes.remove(&project_id);
    if state.last_conversation_workspace.as_deref() == Some(&project_id) {
        state.last_conversation_workspace = state
            .conversation_workspaces
            .first()
            .map(|w| w.project_id.clone());
    }
    app.save_state(&state).map_err(command_error)?;

    Ok(crate::view_models_conversation::conversation_sidebar_vm(
        &app, &state,
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFileVm {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeAttachmentFileInput {
    pub name: String,
    #[serde(default)]
    pub mime: Option<String>,
    pub size: u64,
    pub data_base64: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeConversationAttachmentsInput {
    pub files: Vec<MaterializeAttachmentFileInput>,
}

#[tauri::command]
pub fn pick_attachment_files(
    app_handle: AppHandle,
    _state: State<'_, DesktopState>,
) -> CommandResult<Vec<AttachmentFileVm>> {
    let Some(paths) = app_handle.dialog().file().blocking_pick_files() else {
        return Ok(Vec::new());
    };
    let files: Vec<AttachmentFileVm> = paths
        .into_iter()
        .filter_map(|p| {
            let path = p.into_path().ok()?;
            let name = path.file_name()?.to_str()?.to_string();
            let size = path.metadata().ok()?.len();
            Some(AttachmentFileVm {
                path: path.to_string_lossy().to_string(),
                name,
                size,
            })
        })
        .collect();
    Ok(files)
}

#[tauri::command]
pub fn materialize_conversation_attachments(
    state: State<'_, DesktopState>,
    input: MaterializeConversationAttachmentsInput,
) -> CommandResult<Vec<AttachmentFileVm>> {
    let app = state.app().map_err(command_error)?;
    let root = app
        .paths
        .user_gold_band_dir()
        .join("temp")
        .join("conversation-attachments")
        .join(Uuid::new_v4().to_string());
    materialize_attachment_files_to_dir(&root, &input.files)
}

#[tauri::command]
pub fn show_conversation_attachment(
    state: State<'_, DesktopState>,
    task_id: String,
    name: String,
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let path = app
        .paths
        .task_dir(&task_id)
        .join("authoring")
        .join("inputs")
        .join(&name);
    if !path.exists() {
        return Err(CommandErrorVm::new(
            "attachment.not-found",
            serde_json::json!({ "name": name }),
        ));
    }
    let ext = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_image = matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
    );
    let mime = attachment_mime_for_ext(&ext);
    let content = if is_image {
        let bytes = fs::read(path.as_std_path()).map_err(|e| {
            CommandErrorVm::new(
                "attachment.unreadable",
                serde_json::json!({ "message": e.to_string() }),
            )
        })?;
        format!("data:{};base64,{}", mime, base64_encode(&bytes))
    } else {
        fs::read_to_string(path.as_std_path()).map_err(|e| {
            CommandErrorVm::new(
                "attachment.unreadable",
                serde_json::json!({ "message": e.to_string() }),
            )
        })?
    };
    Ok(ContentVm {
        title: name.clone(),
        kind: "input-attachment".to_string(),
        content,
        metadata: serde_json::json!({
            "name": name,
            "mimeType": mime,
            "isImage": is_image,
            "encoding": if is_image { "data-url" } else { "text" },
        }),
    })
}

fn attachment_mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "json" | "jsonl" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "jsx" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "rs" => "text/rust",
        "py" => "text/python",
        "go" => "text/go",
        "java" => "text/java",
        "c" | "h" => "text/c",
        "cpp" | "hpp" => "text/cpp",
        "yaml" | "yml" => "text/yaml",
        "xml" => "text/xml",
        "toml" => "text/toml",
        "log" => "text/plain",
        "sql" => "text/sql",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        _ => "application/octet-stream",
    }
}

fn materialize_attachment_files_to_dir(
    dir: &camino::Utf8Path,
    files: &[MaterializeAttachmentFileInput],
) -> CommandResult<Vec<AttachmentFileVm>> {
    if files.len() > crate::view_models_conversation::MAX_ATTACHMENT_COUNT {
        return Err(CommandErrorVm::new(
            "conversation.attachment-count-exceeded",
            serde_json::json!({}),
        ));
    }

    fs::create_dir_all(dir.as_std_path()).map_err(|error| {
        CommandErrorVm::new(
            "conversation.attachment-materialize-failed",
            serde_json::json!({ "message": error.to_string() }),
        )
    })?;

    let mut total_size = 0_u64;
    let mut used_names = HashSet::new();
    let mut materialized = Vec::with_capacity(files.len());

    for file in files {
        let _declared_mime = file.mime.as_deref().unwrap_or_default();
        let name = sanitize_attachment_file_name(&file.name)?;
        let ext = Path::new(&name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !crate::view_models_conversation::allowed_attachment_ext(&ext) {
            return Err(CommandErrorVm::new(
                "conversation.attachment-unsupported-type",
                serde_json::json!({ "name": file.name }),
            ));
        }

        let bytes = base64_decode(&file.data_base64).map_err(|message| {
            CommandErrorVm::new(
                "conversation.attachment-unreadable",
                serde_json::json!({ "name": file.name, "message": message }),
            )
        })?;
        let size = bytes.len() as u64;
        if size == 0 || size != file.size {
            return Err(CommandErrorVm::new(
                "conversation.attachment-unreadable",
                serde_json::json!({ "name": file.name }),
            ));
        }
        if size > crate::view_models_conversation::MAX_ATTACHMENT_PER_FILE {
            return Err(CommandErrorVm::new(
                "conversation.attachment-too-large",
                serde_json::json!({ "name": file.name }),
            ));
        }
        total_size += size;
        if total_size > crate::view_models_conversation::MAX_ATTACHMENT_TOTAL {
            return Err(CommandErrorVm::new(
                "conversation.attachment-total-too-large",
                serde_json::json!({}),
            ));
        }

        let name = unique_attachment_file_name(&name, &mut used_names);
        let path = dir.join(&name);
        fs::write(path.as_std_path(), bytes).map_err(|error| {
            CommandErrorVm::new(
                "conversation.attachment-materialize-failed",
                serde_json::json!({ "name": name, "message": error.to_string() }),
            )
        })?;
        materialized.push(AttachmentFileVm {
            path: path.to_string(),
            name,
            size,
        });
    }

    Ok(materialized)
}

fn sanitize_attachment_file_name(name: &str) -> CommandResult<String> {
    let normalized = name.trim().replace('\\', "/");
    let file_name = normalized
        .rsplit('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CommandErrorVm::new(
                "conversation.attachment-unreadable",
                serde_json::json!({ "name": name }),
            )
        })?;
    if file_name == "." || file_name == ".." || file_name.chars().any(char::is_control) {
        return Err(CommandErrorVm::new(
            "conversation.attachment-unreadable",
            serde_json::json!({ "name": name }),
        ));
    }
    Ok(file_name.to_string())
}

fn unique_attachment_file_name(base_name: &str, used_names: &mut HashSet<String>) -> String {
    let path = Path::new(base_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(base_name);
    let ext = path.extension().and_then(|value| value.to_str());
    let mut index = 1_u32;
    loop {
        let candidate = if index == 1 {
            base_name.to_string()
        } else if let Some(ext) = ext {
            format!("{stem}-{index}.{ext}")
        } else {
            format!("{stem}-{index}")
        };
        if used_names.insert(candidate.to_lowercase()) {
            return candidate;
        }
        index += 1;
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            b'=' as char
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            b'=' as char
        });
    }
    out
}

fn base64_decode(value: &str) -> Result<Vec<u8>, String> {
    let normalized = value
        .split_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or(value)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if normalized.is_empty() || normalized.len() % 4 != 0 {
        return Err("invalid base64 length".to_string());
    }

    let mut out = Vec::with_capacity((normalized.len() / 4) * 3);
    let bytes = normalized.as_bytes();
    for chunk in bytes.chunks(4) {
        let mut values = [0_u8; 4];
        let mut padding = 0;
        for (index, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                padding += 1;
                values[index] = 0;
            } else if padding > 0 {
                return Err("invalid base64 padding".to_string());
            } else {
                values[index] =
                    base64_value(*byte).ok_or_else(|| "invalid base64 character".to_string())?;
            }
        }
        if padding > 2 {
            return Err("invalid base64 padding".to_string());
        }
        let n = ((values[0] as u32) << 18)
            | ((values[1] as u32) << 12)
            | ((values[2] as u32) << 6)
            | values[3] as u32;
        out.push(((n >> 16) & 0xFF) as u8);
        if padding < 2 {
            out.push(((n >> 8) & 0xFF) as u8);
        }
        if padding < 1 {
            out.push((n & 0xFF) as u8);
        }
    }
    Ok(out)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[tauri::command]
pub fn get_supported_attachment_extensions() -> CommandResult<Vec<String>> {
    Ok(gold_band::provider::supported_attachment_extensions()
        .into_iter()
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        MaterializeAttachmentFileInput, base64_encode, materialize_attachment_files_to_dir,
    };
    use camino::Utf8PathBuf;
    use uuid::Uuid;

    #[test]
    fn materializes_memory_attachments_with_unique_names() {
        let root = Utf8PathBuf::from_path_buf(
            std::env::temp_dir()
                .join("gold-band-materialize-test")
                .join(Uuid::new_v4().to_string()),
        )
        .unwrap();
        let files = vec![
            MaterializeAttachmentFileInput {
                name: "shot.png".to_string(),
                mime: Some("image/png".to_string()),
                size: 4,
                data_base64: base64_encode(&[1, 2, 3, 4]),
            },
            MaterializeAttachmentFileInput {
                name: "nested\\shot.png".to_string(),
                mime: Some("image/png".to_string()),
                size: 3,
                data_base64: base64_encode(&[5, 6, 7]),
            },
        ];

        let result = materialize_attachment_files_to_dir(&root, &files).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "shot.png");
        assert_eq!(result[1].name, "shot-2.png");
        assert_eq!(std::fs::read(&result[0].path).unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(std::fs::read(&result[1].path).unwrap(), vec![5, 6, 7]);

        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn rejects_unsupported_materialized_attachment_types() {
        let root = Utf8PathBuf::from_path_buf(
            std::env::temp_dir()
                .join("gold-band-materialize-test")
                .join(Uuid::new_v4().to_string()),
        )
        .unwrap();
        let files = vec![MaterializeAttachmentFileInput {
            name: "archive.exe".to_string(),
            mime: None,
            size: 2,
            data_base64: base64_encode(&[1, 2]),
        }];

        let error = materialize_attachment_files_to_dir(&root, &files).unwrap_err();

        assert_eq!(error.code, "conversation.attachment-unsupported-type");
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }
}
