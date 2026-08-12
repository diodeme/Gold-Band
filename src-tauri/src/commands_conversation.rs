use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::config::{
    ConversationAllowedWorkflowRef, ConversationAutoConfig, ConversationDirectConfig,
    ConversationDynamicAgentRef, ConversationDynamicControl, ConversationPin, ConversationRunMode,
    ConversationRunModeEntry, ConversationWorkspaceEntry, DesktopUiMode,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::time::Instant;
use tauri::{AppHandle, State};
use tracing::info;
use uuid::Uuid;

use crate::commands::{
    CommandErrorVm, CommandResult, command_error, configure_conversation_runtime_callbacks,
    spawn_blocking_command,
};
use crate::conversation_workspace::{
    app_for_workspace, project_id_for_workspace, project_ids_match, remove_workspace_from_state,
    workspace_entry_for_project,
};
use crate::state::DesktopContext;
use crate::state::DesktopState;
use crate::view_models::ContentVm;

fn scheduled_service_error(
    error: crate::scheduled_service::ScheduledServiceError,
) -> CommandErrorVm {
    let mut params = error.params;
    if let Some(trace_id) = error.trace_id {
        if let Some(object) = params.as_object_mut() {
            object.insert("traceId".to_string(), serde_json::json!(trace_id));
        }
    }
    CommandErrorVm::new(error.code.to_string(), params)
}

fn validate_scheduled_runtime_settings_input(
    input: &crate::view_models_conversation::ScheduledRuntimeSettingsInputVm,
) -> crate::scheduled_service::ScheduledServiceResult<()> {
    use gold_band::scheduler::queue::{
        MAX_OCCURRENCE_RETENTION_DAYS, MIN_OCCURRENCE_RETENTION_DAYS,
    };

    if !(MIN_OCCURRENCE_RETENTION_DAYS..=MAX_OCCURRENCE_RETENTION_DAYS)
        .contains(&input.occurrence_retention_days)
    {
        return Err(crate::scheduled_service::ScheduledServiceError::new(
            gold_band::scheduler::occurrence::ScheduledErrorCode::ValidationFailed,
            serde_json::json!({
                "field": "occurrenceRetentionDays",
                "minimum": MIN_OCCURRENCE_RETENTION_DAYS,
                "maximum": MAX_OCCURRENCE_RETENTION_DAYS,
                "actual": input.occurrence_retention_days,
            }),
        ));
    }
    Ok(())
}

fn scheduled_runtime_settings_vm(
    config: &gold_band::config::RuntimeConfig,
    power: crate::scheduled_runtime::power::ScheduledPowerStatus,
) -> crate::view_models_conversation::ScheduledRuntimeSettingsVm {
    crate::view_models_conversation::ScheduledRuntimeSettingsVm {
        keep_awake_enabled: config.scheduled_keep_awake_enabled,
        keep_awake_effective: power.effective,
        completion_notifications_enabled: config.scheduled_completion_notifications_enabled,
        enabled_job_count: power.enabled_job_count,
        occurrence_retention_days: config.scheduled_occurrence_retention_days,
        power_error_code: power.error.map(|error| error.code.to_string()),
    }
}

#[tauri::command]
pub fn get_scheduled_runtime_settings(
    state: State<'_, DesktopState>,
) -> CommandResult<crate::view_models_conversation::ScheduledRuntimeSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let power = state.scheduled_power_status().map_err(command_error)?;
    Ok(scheduled_runtime_settings_vm(&context.config, power))
}

#[tauri::command]
pub fn save_scheduled_runtime_settings(
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::ScheduledRuntimeSettingsInputVm,
) -> CommandResult<crate::view_models_conversation::ScheduledRuntimeSettingsVm> {
    validate_scheduled_runtime_settings_input(&input).map_err(scheduled_service_error)?;

    let app = state.app().map_err(command_error)?;
    let mut settings = app.load_settings().map_err(command_error)?;
    settings.scheduled_keep_awake_enabled = Some(input.keep_awake_enabled);
    settings.scheduled_completion_notifications_enabled =
        Some(input.completion_notifications_enabled);
    settings.scheduled_occurrence_retention_days = Some(input.occurrence_retention_days);
    app.save_settings(&settings).map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    let power = state
        .reconcile_scheduled_power_setting()
        .map_err(command_error)?;
    let context = state.context().map_err(command_error)?;
    Ok(scheduled_runtime_settings_vm(&context.config, power))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunModeSettingsVm {
    pub mode: ConversationRunMode,
    pub workflow_template_id: Option<String>,
    pub include_interview: Option<bool>,
    pub direct_config: Option<crate::view_models_conversation::ConversationDirectConfigVm>,
    #[serde(default)]
    pub direct_preferences: std::collections::HashMap<
        String,
        crate::view_models_conversation::ConversationDirectConfigVm,
    >,
    pub auto_config: Option<crate::view_models_conversation::ConversationAutoConfigVm>,
}

fn validate_direct_capabilities(
    state: &DesktopState,
    input: &crate::view_models_conversation::ConversationCreateInputVm,
    result: &mut crate::view_models_conversation::ConversationValidationResultVm,
) -> CommandResult<()> {
    if input.run_mode != ConversationRunMode::Direct.as_str() {
        return Ok(());
    }
    let Some(config) = input.direct_config.as_ref() else {
        return Ok(());
    };
    let Ok(agent_id) = gold_band::config::ManagedAgentId::from_str(&config.agent_type) else {
        return Ok(());
    };
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    let Some(diagnostic) = diagnostics.get(&agent_id) else {
        return Ok(());
    };
    if !diagnostic.available {
        result
            .missing_items
            .push(crate::view_models_conversation::ConversationMissingItemVm {
                code: "direct.agent.unavailable".to_string(),
                label: "Selected Direct Agent is unavailable".to_string(),
                recovery_path: "/chat/agents".to_string(),
            });
    }
    let models =
        gold_band::provider::supported_models_from_capabilities(diagnostic.capabilities.as_ref());
    if let Some(model_id) = config
        .model_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        && !models.is_empty()
        && !models.iter().any(|model| model.id == model_id)
    {
        result
            .missing_items
            .push(crate::view_models_conversation::ConversationMissingItemVm {
                code: "direct.model.not-found".to_string(),
                label: "Selected model is not supported by this Agent".to_string(),
                recovery_path: "/chat".to_string(),
            });
    }
    let modes =
        gold_band::provider::supported_modes_from_capabilities(diagnostic.capabilities.as_ref());
    if let Some(permission_mode) = config
        .permission_mode
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        && !modes.is_empty()
        && !modes.iter().any(|mode| mode.id == permission_mode)
    {
        result
            .missing_items
            .push(crate::view_models_conversation::ConversationMissingItemVm {
                code: "direct.permission.not-found".to_string(),
                label: "Selected permission mode is not supported by this Agent".to_string(),
                recovery_path: "/chat".to_string(),
            });
    }
    result.valid = result.missing_items.is_empty();
    Ok(())
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
pub async fn get_conversation_sidebar(
    state: State<'_, DesktopState>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let started = Instant::now();
    let context = state.context().map_err(command_error)?;
    let result = spawn_blocking_command(move || {
        let app = context.app();
        let state = app.load_state().map_err(command_error)?;
        conversation_sidebar_for_state(&context, &app, &state)
    })
    .await;
    info!(
        target: "gold_band::perf",
        command = "get_conversation_sidebar",
        elapsed_ms = started.elapsed().as_millis(),
        status = if result.is_ok() { "ok" } else { "error" },
        "conversation sidebar loaded"
    );
    result
}

#[tauri::command]
pub fn list_scheduled_tasks(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
) -> CommandResult<Vec<crate::view_models_conversation::ScheduledTaskVm>> {
    let service = state.scheduled_service().map_err(command_error)?;
    service
        .list(project_id.as_deref())
        .map_err(scheduled_service_error)?
        .into_iter()
        .map(|definition| {
            let workspace_name = service
                .workspace_name(&definition.project_id)
                .map_err(scheduled_service_error)?;
            Ok(
                crate::view_models_conversation::ScheduledTaskVm::from_definition_in_workspace(
                    &definition,
                    &workspace_name,
                ),
            )
        })
        .collect()
}

#[tauri::command]
pub fn list_scheduled_task_occurrences(
    state: State<'_, DesktopState>,
    project_id: String,
    scheduled_task_id: String,
    limit: Option<u32>,
) -> CommandResult<Vec<crate::view_models_conversation::ScheduledOccurrenceVm>> {
    state
        .scheduled_service()
        .map_err(command_error)?
        .list_occurrences(
            &project_id,
            &scheduled_task_id,
            limit.unwrap_or(50).clamp(1, 200) as usize,
        )
        .map(|occurrences| scheduled_occurrence_vms_from_occurrences(&occurrences))
        .map_err(scheduled_service_error)
}

fn scheduled_occurrence_vms_from_occurrences(
    occurrences: &[gold_band::scheduler::occurrence::ScheduledOccurrence],
) -> Vec<crate::view_models_conversation::ScheduledOccurrenceVm> {
    occurrences
        .iter()
        .map(crate::view_models_conversation::ScheduledOccurrenceVm::from_occurrence)
        .collect()
}

#[tauri::command]
pub fn get_scheduled_task_diagnostics(
    state: State<'_, DesktopState>,
    project_id: String,
    scheduled_task_id: String,
) -> CommandResult<crate::view_models_conversation::ScheduledTaskDiagnosticsVm> {
    let service = state.scheduled_service().map_err(command_error)?;
    let record = service
        .get(&project_id, &scheduled_task_id)
        .map_err(scheduled_service_error)?;
    let occurrences = service
        .list_occurrences(&project_id, &scheduled_task_id, 200)
        .map_err(scheduled_service_error)?;
    Ok(scheduled_task_diagnostics_vm(
        project_id,
        scheduled_task_id,
        record,
        occurrences,
    ))
}

fn scheduled_task_diagnostics_vm(
    project_id: String,
    scheduled_task_id: String,
    record: gold_band::scheduler::db::ScheduledJobRecord,
    occurrences: Vec<gold_band::scheduler::occurrence::ScheduledOccurrence>,
) -> crate::view_models_conversation::ScheduledTaskDiagnosticsVm {
    let run_count = occurrences
        .iter()
        .filter(|occurrence| occurrence.run_id.is_some())
        .count() as u64;
    crate::view_models_conversation::ScheduledTaskDiagnosticsVm {
        scheduled_task_id,
        project_id,
        next_at: record.next_run_at.map(|value| value.to_rfc3339()),
        last_status: record.definition.last_trigger_status,
        last_error: record.definition.last_error,
        run_count,
        retry_count: record.definition.retry_count,
        occurrences: occurrences
            .iter()
            .map(crate::view_models_conversation::ScheduledOccurrenceVm::from_occurrence)
            .collect(),
    }
}

#[tauri::command]
pub async fn run_scheduled_task_now(
    state: State<'_, DesktopState>,
    project_id: String,
    scheduled_task_id: String,
) -> CommandResult<crate::view_models_conversation::RunScheduledTaskResultVm> {
    let service = state.scheduled_service().map_err(command_error)?;
    let result = service
        .run_now(&project_id, &scheduled_task_id)
        .await
        .map_err(scheduled_service_error)?;
    let links = result.immediate_links;
    Ok(crate::view_models_conversation::RunScheduledTaskResultVm {
        occurrence: crate::view_models_conversation::ScheduledOccurrenceVm::from_occurrence(
            &result.occurrence,
        ),
        task_id: links
            .as_ref()
            .and_then(|links| links.task_id.clone())
            .or(result.occurrence.task_id),
        run_id: links
            .as_ref()
            .and_then(|links| links.run_id.clone())
            .or(result.occurrence.run_id),
        round_id: links
            .as_ref()
            .and_then(|links| links.round_id.clone())
            .or(result.occurrence.round_id),
        attempt_id: links
            .as_ref()
            .and_then(|links| links.attempt_id.clone())
            .or(result.occurrence.attempt_id),
    })
}

#[tauri::command]
pub fn set_scheduled_task_enabled(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    scheduled_task_id: String,
    enabled: bool,
) -> CommandResult<crate::view_models_conversation::ScheduledTaskVm> {
    let service = state.scheduled_service().map_err(command_error)?;
    let project_id = match project_id {
        Some(project_id) => project_id,
        None => state.app().map_err(command_error)?.paths.project_id,
    };
    let record = service
        .set_enabled(&project_id, &scheduled_task_id, enabled)
        .map_err(scheduled_service_error)?;
    let workspace_name = service
        .workspace_name(&record.definition.project_id)
        .map_err(scheduled_service_error)?;
    Ok(
        crate::view_models_conversation::ScheduledTaskVm::from_definition_in_workspace(
            &record.definition,
            &workspace_name,
        ),
    )
}

#[tauri::command]
pub fn create_scheduled_task(
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::CreateScheduledTaskInputVm,
) -> CommandResult<crate::view_models_conversation::ScheduledTaskVm> {
    let service = state.scheduled_service().map_err(command_error)?;
    let record = service.create(input).map_err(scheduled_service_error)?;
    let workspace_name = service
        .workspace_name(&record.definition.project_id)
        .map_err(scheduled_service_error)?;
    Ok(
        crate::view_models_conversation::ScheduledTaskVm::from_definition_in_workspace(
            &record.definition,
            &workspace_name,
        ),
    )
}

#[tauri::command]
pub fn get_scheduled_task(
    state: State<'_, DesktopState>,
    project_id: String,
    scheduled_task_id: String,
) -> CommandResult<crate::view_models_conversation::ScheduledTaskEditVm> {
    let record = state
        .scheduled_service()
        .map_err(command_error)?
        .get(&project_id, &scheduled_task_id)
        .map_err(scheduled_service_error)?;
    Ok(crate::view_models_conversation::ScheduledTaskEditVm::from_definition(&record.definition))
}

#[tauri::command]
pub fn update_scheduled_task(
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::UpdateScheduledTaskInputVm,
) -> CommandResult<crate::view_models_conversation::ScheduledTaskEditVm> {
    let record = state
        .scheduled_service()
        .map_err(command_error)?
        .update(input)
        .map_err(scheduled_service_error)?;
    Ok(crate::view_models_conversation::ScheduledTaskEditVm::from_definition(&record.definition))
}

#[tauri::command]
pub fn delete_scheduled_task(
    state: State<'_, DesktopState>,
    project_id: String,
    scheduled_task_id: String,
) -> CommandResult<()> {
    state
        .scheduled_service()
        .map_err(command_error)?
        .delete(&project_id, &scheduled_task_id)
        .map_err(scheduled_service_error)
}

#[tauri::command]
pub async fn get_conversation_workspaces(
    state: State<'_, DesktopState>,
) -> CommandResult<Vec<crate::view_models_conversation::ConversationWorkspaceVm>> {
    let started = Instant::now();
    let context = state.context().map_err(command_error)?;
    let result = spawn_blocking_command(move || {
        let app = context.app();
        let state = app.load_state().map_err(command_error)?;
        Ok(crate::view_models_conversation::conversation_workspace_vms(
            &state,
        ))
    })
    .await;
    info!(
        target: "gold_band::perf",
        command = "get_conversation_workspaces",
        elapsed_ms = started.elapsed().as_millis(),
        status = if result.is_ok() { "ok" } else { "error" },
        "conversation workspaces loaded"
    );
    result
}

#[tauri::command]
pub async fn get_conversation_run(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    run_id: String,
    selected_session_key: Option<String>,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let started = Instant::now();
    let context = state.context().map_err(command_error)?;
    let log_project_id = project_id.clone();
    let log_task_id = task_id.clone();
    let log_run_id = run_id.clone();
    let log_selected_session_key = selected_session_key.clone();
    let result = spawn_blocking_command(move || {
        let global_app = context.app();
        let app_state = global_app.load_state().map_err(command_error)?;
        let Some((workspace_path, resolved_project_id)) =
            workspace_entry_for_project(&app_state, &project_id)
        else {
            return Err(CommandErrorVm::new(
                "workspace.not-found",
                serde_json::json!({ "projectId": project_id }),
            ));
        };
        let workspace_app =
            global_app.with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone());
        crate::view_models_conversation::conversation_run_vm(
            &workspace_app,
            &resolved_project_id,
            &task_id,
            &run_id,
            selected_session_key.as_deref(),
        )
        .map_err(command_error)
    })
    .await;
    info!(
        target: "gold_band::perf",
        command = "get_conversation_run",
        project_id = %log_project_id,
        task_id = %log_task_id,
        run_id = %log_run_id,
        selected_session_key = ?log_selected_session_key,
        elapsed_ms = started.elapsed().as_millis(),
        status = if result.is_ok() { "ok" } else { "error" },
        "conversation run view model loaded"
    );
    result
}

#[tauri::command]
pub fn validate_conversation_create(
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::ConversationCreateInputVm,
) -> CommandResult<crate::view_models_conversation::ConversationValidationResultVm> {
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, resolved_project_id)) =
        workspace_entry_for_project(&app_state, &input.project_id)
    else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": input.project_id }),
        ));
    };
    let workspace_app = app_for_workspace(&context, &workspace_path).map_err(command_error)?;
    let mut input = input;
    input.project_id = resolved_project_id;
    let mut result =
        crate::view_models_conversation::validate_conversation_create_vm(&workspace_app, &input)
            .map_err(command_error)?;
    validate_direct_capabilities(state.inner(), &input, &mut result)?;
    Ok(result)
}

#[tauri::command]
pub async fn create_conversation_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    input: crate::view_models_conversation::ConversationCreateInputVm,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let started = Instant::now();
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, resolved_project_id)) =
        workspace_entry_for_project(&app_state, &input.project_id)
    else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": input.project_id }),
        ));
    };
    let workspace_app = state
        .app()
        .map_err(command_error)?
        .with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone());
    let mut input = input;
    input.project_id = resolved_project_id.clone();
    let project_id_for_current = resolved_project_id.clone();
    let project_id_for_emit = resolved_project_id;
    let app = workspace_app;
    let mut validation =
        crate::view_models_conversation::validate_conversation_create_vm(&app, &input)
            .map_err(command_error)?;
    validate_direct_capabilities(state.inner(), &input, &mut validation)?;
    if !validation.valid {
        return Err(CommandErrorVm::new(
            "conversation.validation-failed",
            serde_json::json!({
                "codes": validation
                    .missing_items
                    .iter()
                    .map(|item| item.code.clone())
                    .collect::<Vec<_>>()
            }),
        ));
    }
    let app = configure_conversation_runtime_callbacks(
        app,
        app_handle.clone(),
        Some(project_id_for_emit),
    );
    let run = tauri::async_runtime::spawn_blocking(move || {
        crate::view_models_conversation::create_conversation_run_vm(&app, &input)
            .map_err(command_error)
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))??;
    persist_last_conversation_workspace(&global_app, &project_id_for_current)?;
    info!(
        target: "gold_band::perf",
        command = "create_conversation_run",
        project_id = %project_id_for_current,
        elapsed_ms = started.elapsed().as_millis(),
        "conversation run created"
    );
    Ok(run)
}

#[tauri::command]
pub fn rerun_conversation_task(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationRunVm> {
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, resolved_project_id)) =
        workspace_entry_for_project(&app_state, &project_id)
    else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": project_id }),
        ));
    };
    let workspace_app = state
        .app()
        .map_err(command_error)?
        .with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone());
    let app = configure_conversation_runtime_callbacks(
        workspace_app,
        app_handle.clone(),
        Some(resolved_project_id.clone()),
    );
    let run = crate::view_models_conversation::rerun_conversation_task_vm(
        &app,
        &resolved_project_id,
        &task_id,
    )
    .map_err(command_error)?;
    persist_last_conversation_workspace(&global_app, &resolved_project_id)?;
    Ok(run)
}

#[tauri::command]
pub fn switch_conversation_session(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<crate::view_models_conversation::ConversationSessionSwitchVm> {
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, _)) = workspace_entry_for_project(&app_state, &project_id) else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": project_id }),
        ));
    };
    let workspace_app = app_for_workspace(&context, &workspace_path).map_err(command_error)?;
    crate::view_models_conversation::switch_conversation_session_vm(
        &workspace_app,
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
pub async fn update_task_metadata(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    title: String,
    description: Option<String>,
) -> CommandResult<()> {
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, resolved_project_id)) =
        workspace_entry_for_project(&app_state, &project_id)
    else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": project_id }),
        ));
    };
    let workspace_app = app_for_workspace(&context, &workspace_path).map_err(command_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::view_models_conversation::update_task_metadata_vm(
            &workspace_app,
            &resolved_project_id,
            &task_id,
            &title,
            description.as_deref(),
        )
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
    .map_err(command_error)
}

#[tauri::command]
pub async fn pin_conversation(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let mut state = app.load_state().map_err(command_error)?;
        let (_, resolved_project_id) = workspace_entry_for_project(&state, &project_id)
            .ok_or_else(|| {
                CommandErrorVm::new(
                    "workspace.not-found",
                    serde_json::json!({ "projectId": project_id }),
                )
            })?;
        if state.conversation_pins.iter().any(|pin| {
            project_ids_match(&pin.project_id, &resolved_project_id) && pin.task_id == task_id
        }) {
            return conversation_sidebar_for_state(&context, &app, &state);
        }
        let max_order = state
            .conversation_pins
            .iter()
            .map(|p| p.order)
            .max()
            .unwrap_or(0);
        state.conversation_pins.push(ConversationPin {
            project_id: resolved_project_id,
            task_id,
            order: max_order + 1,
        });
        app.save_state(&state).map_err(command_error)?;
        conversation_sidebar_for_state(&context, &app, &state)
    })
    .await
}

#[tauri::command]
pub async fn unpin_conversation(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let mut state = app.load_state().map_err(command_error)?;
        let (_, resolved_project_id) = workspace_entry_for_project(&state, &project_id)
            .ok_or_else(|| {
                CommandErrorVm::new(
                    "workspace.not-found",
                    serde_json::json!({ "projectId": project_id }),
                )
            })?;
        state.conversation_pins.retain(|p| {
            !project_ids_match(&p.project_id, &resolved_project_id) || p.task_id != task_id
        });
        app.save_state(&state).map_err(command_error)?;
        conversation_sidebar_for_state(&context, &app, &state)
    })
    .await
}

#[tauri::command]
pub async fn reorder_pinned_conversations(
    state: State<'_, DesktopState>,
    ordered: Vec<gold_band::config::ConversationPin>,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let mut state = app.load_state().map_err(command_error)?;
        let normalized_pins = ordered
            .into_iter()
            .enumerate()
            .map(|(i, mut pin)| {
                let (_, resolved_project_id) = workspace_entry_for_project(&state, &pin.project_id)
                    .ok_or_else(|| {
                        CommandErrorVm::new(
                            "workspace.not-found",
                            serde_json::json!({ "projectId": pin.project_id }),
                        )
                    })?;
                pin.project_id = resolved_project_id;
                pin.order = i;
                Ok(pin)
            })
            .collect::<CommandResult<Vec<_>>>()?;
        state.conversation_pins = normalized_pins;
        app.save_state(&state).map_err(command_error)?;
        conversation_sidebar_for_state(&context, &app, &state)
    })
    .await
}

#[tauri::command]
pub async fn search_conversation_tasks(
    state: State<'_, DesktopState>,
    query: String,
    limit: Option<usize>,
) -> CommandResult<Vec<crate::view_models_conversation::ConversationSearchResultVm>> {
    let limit = limit.unwrap_or(50).min(200);
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let app_state = app.load_state().unwrap_or_default();
    let task_roots = conversation_search_task_roots(&app, &app_state);
    let index = gold_band::storage::sqlite::search_index()
        .ok_or_else(|| CommandErrorVm::new("search.index-unavailable", serde_json::json!({})))?;
    let index = index.clone();
    tauri::async_runtime::spawn_blocking(move || {
        index
            .search_tasks_in_task_roots(&query, &task_roots, limit)
            .map(|results| {
                results
                    .into_iter()
                    .filter_map(|result| {
                        let (project_id, workspace_name) =
                            extract_project_from_task_path(&result.task_path, &app_state);
                        let (workspace_path, resolved_project_id) =
                            workspace_entry_for_project(&app_state, &project_id)?;
                        let workspace_app = app_for_workspace(&context, &workspace_path).ok()?;
                        conversation_search_result_for_workspace(
                            &workspace_app,
                            resolved_project_id,
                            workspace_path,
                            workspace_name,
                            result,
                        )
                    })
                    .collect()
            })
            .map_err(|error| {
                CommandErrorVm::new(
                    "search.query-failed",
                    serde_json::json!({ "message": error.to_string() }),
                )
            })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

fn conversation_search_task_roots(
    _app: &App,
    state: &gold_band::config::StateConfig,
) -> Vec<String> {
    state
        .conversation_workspaces
        .iter()
        .map(|workspace| {
            gold_band::storage::GoldBandPaths::new(Utf8PathBuf::from(&workspace.workspace_path))
                .tasks_dir()
                .to_string()
        })
        .collect()
}

fn conversation_search_result_for_workspace(
    workspace_app: &App,
    project_id: String,
    workspace_path: String,
    workspace_name: String,
    result: gold_band::storage::sqlite::TaskSearchResult,
) -> Option<crate::view_models_conversation::ConversationSearchResultVm> {
    let latest_run = workspace_app
        .task_summary(&result.task_id)
        .ok()?
        .latest_run
        .as_ref()
        .map(crate::view_models_conversation::conversation_run_summary_vm)?;
    let metadata =
        gold_band::storage::read_json::<crate::view_models_conversation::ConversationMetadata>(
            &Utf8PathBuf::from(&result.task_path)
                .join("authoring")
                .join("conversation.json"),
        )
        .ok();
    Some(
        crate::view_models_conversation::ConversationSearchResultVm {
            project_id,
            workspace_path,
            workspace_name,
            task_id: result.task_id,
            title: result.title,
            description: Some(result.description),
            requirement_preview: result.requirement_preview,
            match_preview: result.match_preview,
            latest_run: Some(latest_run),
            run_mode: metadata
                .as_ref()
                .map(|metadata| metadata.run_mode.clone())
                .unwrap_or_else(|| "workflow".to_string()),
            agent_identity: metadata
                .as_ref()
                .and_then(|metadata| metadata.agent_identity.clone()),
            last_activity_at: metadata.as_ref().and_then(|metadata| {
                metadata
                    .last_activity_at
                    .clone()
                    .or_else(|| Some(metadata.created_at.clone()))
            }),
        },
    )
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
        .find(|workspace| project_ids_match(&workspace.project_id, &project_id))
        .map(|w| w.name.clone())
        .unwrap_or(project_id.clone());
    (project_id, workspace_name)
}

fn persist_last_conversation_workspace(app: &App, project_id: &str) -> CommandResult<()> {
    let mut state = app.load_state().map_err(command_error)?;
    let (_, resolved_project_id) =
        workspace_entry_for_project(&state, project_id).ok_or_else(|| {
            CommandErrorVm::new(
                "workspace.not-found",
                serde_json::json!({ "projectId": project_id }),
            )
        })?;
    state.last_conversation_workspace = Some(resolved_project_id);
    app.save_state(&state).map_err(command_error)?;
    Ok(())
}

fn conversation_sidebar_sources(
    context: &DesktopContext,
    _app: &App,
    state: &gold_band::config::StateConfig,
) -> anyhow::Result<Vec<crate::view_models_conversation::ConversationWorkspaceSource>> {
    state
        .conversation_workspaces
        .iter()
        .map(|workspace| {
            Ok(
                crate::view_models_conversation::ConversationWorkspaceSource {
                    workspace: crate::view_models_conversation::ConversationWorkspaceVm {
                        project_id: workspace.project_id.clone(),
                        workspace_path: workspace.workspace_path.clone(),
                        name: workspace.name.clone(),
                    },
                    app: app_for_workspace(context, &workspace.workspace_path)?,
                },
            )
        })
        .collect()
}

#[cfg(test)]
fn workspace_name_for_project(state: &gold_band::config::StateConfig, project_id: &str) -> String {
    state
        .conversation_workspaces
        .iter()
        .find(|workspace| project_ids_match(&workspace.project_id, project_id))
        .map(|workspace| workspace.name.clone())
        .unwrap_or_else(|| project_id.to_string())
}

fn conversation_sidebar_for_state(
    context: &DesktopContext,
    app: &App,
    state: &gold_band::config::StateConfig,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let sources = conversation_sidebar_sources(context, app, state).map_err(command_error)?;
    Ok(crate::view_models_conversation::conversation_sidebar_vm_from_sources(state, &sources))
}

#[tauri::command]
pub fn get_conversation_run_mode(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<Option<crate::view_models_conversation::ConversationRunModeVm>> {
    let app = state.app().map_err(command_error)?;
    let state = app.load_state().map_err(command_error)?;
    let (_, resolved_project_id) =
        workspace_entry_for_project(&state, &project_id).ok_or_else(|| {
            CommandErrorVm::new(
                "workspace.not-found",
                serde_json::json!({ "projectId": project_id }),
            )
        })?;
    Ok(state
        .conversation_run_modes
        .get(&resolved_project_id)
        .map(
            |entry| crate::view_models_conversation::ConversationRunModeVm {
                mode: entry.mode.as_str().to_string(),
                workflow_template_id: entry.workflow_template_id.clone(),
                include_interview: entry.include_interview,
                direct_config: entry.direct_config.as_ref().map(|config| {
                    crate::view_models_conversation::ConversationDirectConfigVm {
                        agent_type: config.agent_type.clone(),
                        model_id: config.model_id.clone(),
                        permission_mode: config.permission_mode.clone(),
                        config_options: config.config_options.clone(),
                    }
                }),
                direct_preferences: entry
                    .direct_preferences
                    .iter()
                    .map(|(agent_type, config)| {
                        (
                            agent_type.clone(),
                            crate::view_models_conversation::ConversationDirectConfigVm {
                                agent_type: config.agent_type.clone(),
                                model_id: config.model_id.clone(),
                                permission_mode: config.permission_mode.clone(),
                                config_options: config.config_options.clone(),
                            },
                        )
                    })
                    .collect(),
                auto_config: entry.auto_config.as_ref().map(|cfg| {
                    crate::view_models_conversation::ConversationAutoConfigVm {
                        agent_strategy: cfg.agent_strategy.clone(),
                        agent_type: cfg.agent_type.clone(),
                        bootstrap_agent_type: cfg.bootstrap_agent_type.clone(),
                        bootstrap_model_id: cfg.bootstrap_model_id.clone(),
                        bootstrap_config_options: cfg.bootstrap_config_options.clone(),
                        acceptance_model_id: cfg.acceptance_model_id.clone(),
                        acceptance_config_options: cfg.acceptance_config_options.clone(),
                        model_id: cfg.model_id.clone(),
                        permission_mode: cfg.permission_mode.clone(),
                        config_options: cfg.config_options.clone(),
                        available_agents: cfg.available_agents.as_ref().map(|agents| {
                            agents
                                .iter()
                                .map(|agent| {
                                    crate::view_models_conversation::ConversationDynamicAgentRefVm {
                                        provider: agent.provider.clone(),
                                        model: agent.model.clone(),
                                        permission_mode: agent.permission_mode.clone(),
                                        config_options: agent.config_options.clone(),
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
            },
        ))
}

#[tauri::command]
pub fn save_conversation_run_mode(
    state: State<'_, DesktopState>,
    project_id: String,
    settings: ConversationRunModeSettingsVm,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let mut state = app.load_state().map_err(command_error)?;
    let (_, resolved_project_id) =
        workspace_entry_for_project(&state, &project_id).ok_or_else(|| {
            CommandErrorVm::new(
                "workspace.not-found",
                serde_json::json!({ "projectId": project_id }),
            )
        })?;
    state.conversation_run_modes.insert(
        resolved_project_id,
        ConversationRunModeEntry {
            mode: settings.mode,
            workflow_template_id: settings.workflow_template_id,
            include_interview: settings.include_interview,
            direct_config: settings
                .direct_config
                .map(|config| ConversationDirectConfig {
                    agent_type: config.agent_type,
                    model_id: config.model_id,
                    permission_mode: config.permission_mode,
                    config_options: config.config_options,
                }),
            direct_preferences: settings
                .direct_preferences
                .into_iter()
                .map(|(agent_type, config)| {
                    (
                        agent_type,
                        ConversationDirectConfig {
                            agent_type: config.agent_type,
                            model_id: config.model_id,
                            permission_mode: config.permission_mode,
                            config_options: config.config_options,
                        },
                    )
                })
                .collect(),
            auto_config: settings.auto_config.map(|cfg| ConversationAutoConfig {
                agent_strategy: cfg.agent_strategy,
                agent_type: cfg.agent_type,
                bootstrap_agent_type: cfg.bootstrap_agent_type,
                bootstrap_model_id: cfg.bootstrap_model_id,
                bootstrap_config_options: cfg.bootstrap_config_options,
                acceptance_model_id: cfg.acceptance_model_id,
                acceptance_config_options: cfg.acceptance_config_options,
                model_id: cfg.model_id,
                permission_mode: cfg.permission_mode,
                config_options: cfg.config_options,
                available_agents: cfg.available_agents.map(|agents| {
                    agents
                        .into_iter()
                        .map(|agent| ConversationDynamicAgentRef {
                            provider: agent.provider,
                            model: agent.model,
                            permission_mode: agent.permission_mode,
                            config_options: agent.config_options,
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
    let project_id = project_id_for_workspace(&workspace_path);
    Ok(crate::view_models_conversation::ConversationWorkspaceVm {
        project_id,
        workspace_path,
        name,
    })
}

#[tauri::command]
pub async fn add_conversation_workspace(
    state: State<'_, DesktopState>,
    path: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    let coordinator = state.scheduler_coordinator().map_err(command_error)?;
    spawn_blocking_command(move || {
        let gold_band_app = context.app();
        let workspace_path = Utf8PathBuf::from(path);
        let workspace_path_str = workspace_path.as_str().to_string();
        info!(workspace_path = %workspace_path_str, "conversation workspace picker returned selection");

        let name = std::path::Path::new(&workspace_path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| workspace_path_str.clone());
        let project_id = project_id_for_workspace(&workspace_path_str);
        let mut state = gold_band_app.load_state().map_err(command_error)?;

        if state
            .conversation_workspaces
            .iter()
            .any(|workspace| project_ids_match(&workspace.project_id, &project_id))
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
        coordinator
            .send(crate::scheduled_runtime::SchedulerCommand::RegisterWorkspace {
                workspace_path: workspace_path.clone(),
            })
            .map_err(scheduled_service_error)?;
        info!(
            project_id = %project_id,
            workspace_count = state.conversation_workspaces.len(),
            "conversation workspace added"
        );

        conversation_sidebar_for_state(&context, &gold_band_app, &state)
    })
    .await
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
    let (_, resolved_project_id) = workspace_entry_for_project(&app_state, &project_id)
        .ok_or_else(|| {
            CommandErrorVm::new(
                "workspace.not-found",
                serde_json::json!({ "projectId": project_id }),
            )
        })?;
    app_state.last_conversation_workspace = Some(resolved_project_id);
    app.save_state(&app_state).map_err(command_error)?;
    Ok(())
}

#[tauri::command]
pub async fn sync_conversation_workspace(
    state: State<'_, DesktopState>,
    workspace_path: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    let coordinator = state.scheduler_coordinator().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let name = std::path::Path::new(&workspace_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| workspace_path.clone());
        let project_id = project_id_for_workspace(&workspace_path);
        let mut state = app.load_state().map_err(command_error)?;

        let resolved_project_id = if let Some((_, resolved_project_id)) =
            workspace_entry_for_project(&state, &project_id)
        {
            resolved_project_id
        } else {
            state
                .conversation_workspaces
                .push(ConversationWorkspaceEntry {
                    project_id: project_id.clone(),
                    workspace_path: workspace_path.clone(),
                    name: name.clone(),
                    added_at: chrono::Utc::now().to_rfc3339(),
                });
            project_id
        };
        state.last_conversation_workspace = Some(resolved_project_id);
        app.save_state(&state).map_err(command_error)?;
        coordinator
            .send(
                crate::scheduled_runtime::SchedulerCommand::RegisterWorkspace {
                    workspace_path: Utf8PathBuf::from(workspace_path.clone()),
                },
            )
            .map_err(scheduled_service_error)?;

        conversation_sidebar_for_state(&context, &app, &state)
    })
    .await
}

#[tauri::command]
pub async fn delete_conversation_task(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let mut app_state = app.load_state().map_err(command_error)?;
        let Some((workspace_path, normalized_project_id)) =
            workspace_entry_for_project(&app_state, &project_id)
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
        if let Ok(runs) = workspace_app.run_list(&task_id)
            && runs
                .iter()
                .any(|run| run.status == gold_band::domain::RunStatus::Running)
        {
            return Err(CommandErrorVm::new(
                "conversation.task-running",
                serde_json::json!({ "taskId": task_id }),
            ));
        }
        trash::delete(task_dir.as_std_path()).map_err(|error| {
            CommandErrorVm::new(
                "conversation.task-delete-failed",
                serde_json::json!({ "taskId": task_id, "message": error.to_string() }),
            )
        })?;
        gold_band::storage::sqlite::delete_task(&task_dir);
        app_state
            .conversation_pins
            .retain(|p| p.project_id != normalized_project_id || p.task_id != task_id);
        app.save_state(&app_state).map_err(command_error)?;
        conversation_sidebar_for_state(&context, &app, &app_state)
    })
    .await
}

#[tauri::command]
pub async fn remove_conversation_workspace(
    state: State<'_, DesktopState>,
    project_id: String,
) -> CommandResult<crate::view_models_conversation::ConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    let coordinator = state.scheduler_coordinator().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let mut state = app.load_state().map_err(command_error)?;
        let workspace_path = workspace_entry_for_project(&state, &project_id)
            .map(|(workspace_path, _)| workspace_path)
            .ok_or_else(|| {
                CommandErrorVm::new(
                    "conversation.workspace-not-found",
                    serde_json::json!({ "projectId": project_id }),
                )
            })?;
        gold_band::acp::client::close_workspace_connections_bounded(&Utf8PathBuf::from(
            workspace_path.clone(),
        ))
        .map_err(command_error)?;

        remove_workspace_from_state(&mut state, &project_id).ok_or_else(|| {
            CommandErrorVm::new(
                "conversation.workspace-not-found",
                serde_json::json!({ "projectId": project_id }),
            )
        })?;
        app.save_state(&state).map_err(command_error)?;
        coordinator
            .send(
                crate::scheduled_runtime::SchedulerCommand::UnregisterWorkspace {
                    workspace_path: Utf8PathBuf::from(workspace_path),
                },
            )
            .map_err(scheduled_service_error)?;

        conversation_sidebar_for_state(&context, &app, &state)
    })
    .await
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
pub fn stat_attachment_files(paths: Vec<String>) -> CommandResult<Vec<AttachmentFileVm>> {
    let files: Vec<AttachmentFileVm> = paths
        .into_iter()
        .filter_map(|p| {
            let path = Path::new(&p);
            let name = path.file_name()?.to_str()?.to_string();
            let size = path.metadata().ok()?.len();
            Some(AttachmentFileVm {
                path: p,
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
    project_id: String,
    task_id: String,
    name: String,
) -> CommandResult<ContentVm> {
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, _)) = workspace_entry_for_project(&app_state, &project_id) else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": project_id }),
        ));
    };
    let app = app_for_workspace(&context, &workspace_path).map_err(command_error)?;
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

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn show_conversation_message_attachment(
    state: State<'_, DesktopState>,
    project_id: String,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    path: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let context = state.context().map_err(command_error)?;
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, _)) = workspace_entry_for_project(&app_state, &project_id) else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": project_id }),
        ));
    };
    let app = app_for_workspace(&context, &workspace_path).map_err(command_error)?;
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
    message_attachment_content_from_attempt_dir(&attempt_dir, &name, &path)
}

fn message_attachment_content_from_attempt_dir(
    attempt_dir: &camino::Utf8Path,
    name: &str,
    attachment_path: &str,
) -> CommandResult<ContentVm> {
    let relative_path = sanitize_message_attachment_relative_path(attachment_path)?;
    let path = attempt_dir.join(&relative_path);
    if !path.exists() {
        return Err(CommandErrorVm::new(
            "attachment.not-found",
            serde_json::json!({ "name": name, "path": attachment_path }),
        ));
    }
    let ext = Path::new(&relative_path)
        .extension()
        .and_then(|e| e.to_str())
        .or_else(|| Path::new(name).extension().and_then(|e| e.to_str()))
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
        title: name.to_string(),
        kind: "message-attachment".to_string(),
        content,
        metadata: serde_json::json!({
            "name": name,
            "path": relative_path,
            "mimeType": mime,
            "isImage": is_image,
            "encoding": if is_image { "data-url" } else { "text" },
        }),
    })
}

fn sanitize_message_attachment_relative_path(path: &str) -> CommandResult<String> {
    let normalized = path.trim().replace('\\', "/");
    let components: Vec<&str> = normalized.split('/').collect();
    if components.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with('~')
        || components.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.contains(':')
                || part.chars().any(char::is_control)
        })
    {
        return Err(CommandErrorVm::new(
            "attachment.invalid-path",
            serde_json::json!({ "path": path }),
        ));
    }
    Ok(components.join("/"))
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
        MaterializeAttachmentFileInput, base64_encode, conversation_search_result_for_workspace,
        conversation_search_task_roots, materialize_attachment_files_to_dir,
        message_attachment_content_from_attempt_dir, scheduled_occurrence_vms_from_occurrences,
        scheduled_runtime_settings_vm, scheduled_service_error,
        validate_scheduled_runtime_settings_input,
    };
    use camino::Utf8PathBuf;
    use gold_band::app::App;
    use gold_band::config::{ConversationWorkspaceEntry, StateConfig};
    use gold_band::domain::{RunStatus, VERSION};
    use gold_band::runtime::{RunState, TaskState};
    use gold_band::scheduler::occurrence::ScheduledErrorCode;
    use gold_band::storage::{sqlite::TaskSearchResult, write_json};
    use uuid::Uuid;

    use crate::view_models_conversation::ScheduledRuntimeSettingsInputVm;

    #[test]
    fn scheduled_occurrence_list_keeps_skipped_and_missed_history() {
        use chrono::{TimeZone, Utc};
        use gold_band::scheduler::occurrence::{
            OccurrenceStatus, OccurrenceTriggerKind, ScheduledOccurrence,
        };

        let now = Utc.with_ymd_and_hms(2026, 8, 7, 9, 0, 0).unwrap();
        let make_occurrence = |id: &str, status| ScheduledOccurrence {
            id: id.to_string(),
            job_id: "scheduled-1".to_string(),
            scheduled_at: now,
            trigger_kind: OccurrenceTriggerKind::Scheduled,
            status,
            attempt: 1,
            owner_id: None,
            lease_until: None,
            heartbeat_at: None,
            task_id: None,
            run_id: None,
            round_id: None,
            attempt_id: None,
            error_code: None,
            error_params: None,
            started_at: None,
            finished_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let occurrences = vec![
            make_occurrence("skipped", OccurrenceStatus::Skipped),
            make_occurrence("missed", OccurrenceStatus::Missed),
        ];

        let statuses = scheduled_occurrence_vms_from_occurrences(&occurrences)
            .into_iter()
            .map(|occurrence| occurrence.status)
            .collect::<Vec<_>>();

        assert_eq!(statuses, vec!["skipped", "missed"]);
    }

    #[test]
    fn scheduled_runtime_settings_reject_retention_below_minimum() {
        let error = validate_scheduled_runtime_settings_input(&ScheduledRuntimeSettingsInputVm {
            keep_awake_enabled: true,
            completion_notifications_enabled: true,
            occurrence_retention_days: 0,
        })
        .unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert_eq!(
            error.params,
            serde_json::json!({
                "field": "occurrenceRetentionDays",
                "minimum": 1,
                "maximum": 3650,
                "actual": 0,
            })
        );
    }

    #[test]
    fn scheduled_runtime_settings_reject_retention_above_maximum() {
        let error = validate_scheduled_runtime_settings_input(&ScheduledRuntimeSettingsInputVm {
            keep_awake_enabled: false,
            completion_notifications_enabled: false,
            occurrence_retention_days: 3651,
        })
        .unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert_eq!(error.params["actual"], 3651);
    }

    #[test]
    fn scheduled_runtime_settings_report_config_and_effective_power_separately() {
        let config = gold_band::config::RuntimeConfig {
            scheduled_keep_awake_enabled: true,
            scheduled_completion_notifications_enabled: false,
            scheduled_occurrence_retention_days: 90,
            ..gold_band::config::RuntimeConfig::default()
        };
        let vm = scheduled_runtime_settings_vm(
            &config,
            crate::scheduled_runtime::power::ScheduledPowerStatus {
                effective: false,
                enabled_job_count: 4,
                error: Some(gold_band::scheduler::occurrence::ScheduledError::new(
                    ScheduledErrorCode::PowerInhibitorFailed,
                )),
            },
        );

        assert!(vm.keep_awake_enabled);
        assert!(!vm.keep_awake_effective);
        assert!(!vm.completion_notifications_enabled);
        assert_eq!(vm.enabled_job_count, 4);
        assert_eq!(vm.occurrence_retention_days, 90);
        assert_eq!(
            vm.power_error_code.as_deref(),
            Some("SCHEDULED_POWER_INHIBITOR_FAILED")
        );
    }

    #[test]
    fn workspace_name_for_project_uses_registered_workspace_name() {
        let mut state = StateConfig::default();
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: "project-a".to_string(),
                workspace_path: "D:/workspace-a".to_string(),
                name: "Workspace A".to_string(),
                added_at: "2026-07-30T00:00:00Z".to_string(),
            });

        assert_eq!(
            super::workspace_name_for_project(&state, "project-a"),
            "Workspace A"
        );
        assert_eq!(
            super::workspace_name_for_project(&state, "project-b"),
            "project-b"
        );
    }

    #[test]
    fn scheduled_service_errors_keep_structured_command_contract() {
        let error = crate::scheduled_service::ScheduledServiceError {
            code: ScheduledErrorCode::Conflict,
            params: serde_json::json!({
                "scheduledTaskId": "scheduled-a",
                "revision": 3,
            }),
            trace_id: Some("trace-a".to_string()),
        };

        let mapped = scheduled_service_error(error);

        assert_eq!(mapped.code, "SCHEDULED_CONFLICT");
        assert_eq!(
            mapped.params,
            serde_json::json!({
                "scheduledTaskId": "scheduled-a",
                "revision": 3,
                "traceId": "trace-a",
            })
        );
    }

    #[test]
    fn scheduled_diagnostics_uses_the_persisted_deadline() {
        let definition = gold_band::scheduler::ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-a",
            "direct",
            gold_band::scheduler::ScheduleSpec::at(chrono::Utc::now() + chrono::Duration::hours(1)),
            gold_band::scheduler::OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let record = gold_band::scheduler::db::ScheduledJobRecord {
            definition,
            revision: 3,
            next_run_at: None,
        };

        let diagnostics = super::scheduled_task_diagnostics_vm(
            "project-a".to_string(),
            "scheduled-a".to_string(),
            record,
            Vec::new(),
        );

        assert_eq!(diagnostics.next_at, None);
    }

    #[test]
    fn conversation_search_result_contains_latest_run_for_navigation() {
        let root = Utf8PathBuf::from_path_buf(
            std::env::temp_dir()
                .join("gold-band-conversation-search-test")
                .join(Uuid::new_v4().to_string()),
        )
        .unwrap();
        std::fs::create_dir_all(root.as_std_path()).unwrap();
        let app = App::new(root.clone());
        let task_id = "task-001";
        let run_id = "run-001";
        write_json(
            &app.paths.task_file(task_id),
            &TaskState {
                version: VERSION.to_string(),
                id: task_id.to_string(),
                title: Some("Searchable conversation".to_string()),
                description: None,
                uuid: None,
            },
        )
        .unwrap();
        write_json(
            &app.paths.run_file(task_id, run_id),
            &RunState {
                version: VERSION.to_string(),
                id: run_id.to_string(),
                task_id: task_id.to_string(),
                task_uuid: None,
                status: RunStatus::Completed,
                outcome: None,
                started_at: "2026-07-24T00:00:00Z".to_string(),
                updated_at: "2026-07-24T00:01:00Z".to_string(),
                workflow_snapshot: "workflow.snapshot.json".to_string(),
                current_round: Some("round-001".to_string()),
                current_node: Some("direct-agent".to_string()),
                current_attempt: Some("attempt-001".to_string()),
                new_rounds_opened: 0,
                pause_reason: None,
                uuid: None,
                last_executed_node: None,
            },
        )
        .unwrap();

        let result = conversation_search_result_for_workspace(
            &app,
            "project-a".to_string(),
            root.to_string(),
            "Project A".to_string(),
            TaskSearchResult {
                task_id: task_id.to_string(),
                task_path: app.paths.task_dir(task_id).to_string(),
                title: "Searchable conversation".to_string(),
                description: String::new(),
                requirement_preview: "find a file".to_string(),
                match_preview: "find a file".to_string(),
            },
        )
        .unwrap();

        assert_eq!(result.project_id, "project-a");
        assert_eq!(result.workspace_path, root.as_str());
        assert_eq!(result.latest_run.unwrap().run_id, run_id);
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn conversation_search_scope_contains_only_sidebar_workspaces() {
        let app = make_test_app();
        let mut state = gold_band::config::StateConfig::default();
        state
            .conversation_workspaces
            .push(gold_band::config::ConversationWorkspaceEntry {
                project_id: "sidebar-workspace".to_string(),
                workspace_path: "/path/to/sidebar-workspace".to_string(),
                name: "Sidebar workspace".to_string(),
                added_at: "2026-07-24T00:00:00Z".to_string(),
            });

        let roots = conversation_search_task_roots(&app, &state);

        assert_eq!(roots.len(), 1);
        assert_eq!(
            roots[0],
            gold_band::storage::GoldBandPaths::new(Utf8PathBuf::from("/path/to/sidebar-workspace"))
                .tasks_dir()
                .to_string()
        );
    }

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

    #[test]
    fn shows_message_attachment_from_attempt_user_inputs() {
        let root = Utf8PathBuf::from_path_buf(
            std::env::temp_dir()
                .join("gold-band-message-attachment-test")
                .join(Uuid::new_v4().to_string()),
        )
        .unwrap();
        let user_inputs = root.join("user-inputs");
        std::fs::create_dir_all(user_inputs.as_std_path()).unwrap();
        std::fs::write(user_inputs.join("image.png").as_std_path(), [1_u8, 2, 3]).unwrap();

        let content = message_attachment_content_from_attempt_dir(
            &root,
            "image.png",
            "user-inputs/image.png",
        )
        .unwrap();

        assert_eq!(content.kind, "message-attachment");
        assert_eq!(content.title, "image.png");
        assert!(content.content.starts_with("data:image/png;base64,"));
        assert_eq!(content.content, "data:image/png;base64,AQID");
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn rejects_message_attachment_path_traversal() {
        let root = Utf8PathBuf::from_path_buf(
            std::env::temp_dir()
                .join("gold-band-message-attachment-test")
                .join(Uuid::new_v4().to_string()),
        )
        .unwrap();

        let error = message_attachment_content_from_attempt_dir(&root, "image.png", "../image.png")
            .unwrap_err();

        assert_eq!(error.code, "attachment.invalid-path");
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    // ── Workspace resolution tests ──

    fn temp_repo_root() -> Utf8PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!("gold-band-workspace-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Utf8PathBuf::from_path_buf(root).unwrap()
    }

    fn make_test_app() -> gold_band::app::App {
        gold_band::app::App::new(temp_repo_root())
    }

    #[test]
    fn workspace_entry_does_not_implicitly_resolve_desktop_workspace() {
        let state = gold_band::config::StateConfig::default();

        let result = super::workspace_entry_for_project(&state, "desktop-workspace");

        assert!(result.is_none());
    }

    #[test]
    fn workspace_entry_resolves_non_default_from_state() {
        let mut state = gold_band::config::StateConfig::default();
        state
            .conversation_workspaces
            .push(gold_band::config::ConversationWorkspaceEntry {
                project_id: "claude-code".to_string(),
                workspace_path: "/path/to/claude-code".to_string(),
                name: "claude-code".to_string(),
                added_at: "2025-01-01T00:00:00Z".to_string(),
            });

        let result = super::workspace_entry_for_project(&state, "claude-code");
        assert!(result.is_some());
        let (path, id) = result.unwrap();
        assert_eq!(path, "/path/to/claude-code");
        assert_eq!(id, "claude-code");
    }

    #[cfg(windows)]
    #[test]
    fn workspace_entry_matches_legacy_project_id_case_insensitively() {
        let mut state = gold_band::config::StateConfig::default();
        state
            .conversation_workspaces
            .push(gold_band::config::ConversationWorkspaceEntry {
                project_id: "d--projects-code-ai-claude-code".to_string(),
                workspace_path: "D:\\Projects\\code\\ai\\claude code".to_string(),
                name: "claude code".to_string(),
                added_at: "2025-01-01T00:00:00Z".to_string(),
            });

        let result =
            super::workspace_entry_for_project(&state, "D--Projects-code-ai-claude-code").unwrap();

        assert_eq!(result.0, "D:\\Projects\\code\\ai\\claude code");
        assert_eq!(result.1, "d--projects-code-ai-claude-code");
    }

    #[cfg(windows)]
    #[test]
    fn indexed_task_path_resolves_legacy_workspace_without_dropping_search_result() {
        let mut state = gold_band::config::StateConfig::default();
        state
            .conversation_workspaces
            .push(gold_band::config::ConversationWorkspaceEntry {
                project_id: "d--projects-code-ai-claude-code".to_string(),
                workspace_path: "D:\\Projects\\code\\ai\\claude code".to_string(),
                name: "claude code".to_string(),
                added_at: "2025-01-01T00:00:00Z".to_string(),
            });
        let task_path = "C:\\Users\\user\\.gold-band\\projects\\D--Projects-code-ai-claude-code\\tasks\\task-053";

        let (indexed_project_id, workspace_name) =
            super::extract_project_from_task_path(task_path, &state);
        let resolved = super::workspace_entry_for_project(&state, &indexed_project_id).unwrap();

        assert_eq!(indexed_project_id, "D--Projects-code-ai-claude-code");
        assert_eq!(workspace_name, "claude code");
        assert_eq!(resolved.1, "d--projects-code-ai-claude-code");
    }

    #[test]
    fn workspace_entry_returns_none_for_unknown_project() {
        let state = gold_band::config::StateConfig::default();

        let result = super::workspace_entry_for_project(&state, "no-such-workspace");
        assert!(result.is_none());
    }

    #[test]
    fn remove_conversation_workspace_cleans_up_pins_and_run_modes() {
        let mut state = gold_band::config::StateConfig::default();
        state
            .conversation_workspaces
            .push(gold_band::config::ConversationWorkspaceEntry {
                project_id: "ws-a".to_string(),
                workspace_path: "/ws-a".to_string(),
                name: "Workspace A".to_string(),
                added_at: "2025-01-01T00:00:00Z".to_string(),
            });
        state
            .conversation_workspaces
            .push(gold_band::config::ConversationWorkspaceEntry {
                project_id: "ws-b".to_string(),
                workspace_path: "/ws-b".to_string(),
                name: "Workspace B".to_string(),
                added_at: "2025-01-01T00:00:00Z".to_string(),
            });
        state.last_conversation_workspace = Some("ws-a".to_string());
        state
            .conversation_pins
            .push(gold_band::config::ConversationPin {
                project_id: "ws-a".to_string(),
                task_id: "task-1".to_string(),
                order: 0,
            });
        state
            .conversation_pins
            .push(gold_band::config::ConversationPin {
                project_id: "ws-b".to_string(),
                task_id: "task-2".to_string(),
                order: 1,
            });
        state.conversation_run_modes.insert(
            "ws-a".to_string(),
            gold_band::config::ConversationRunModeEntry {
                mode: gold_band::config::ConversationRunMode::Auto,
                workflow_template_id: None,
                include_interview: None,
                direct_config: None,
                direct_preferences: Default::default(),
                auto_config: None,
            },
        );

        let removed = super::remove_workspace_from_state(&mut state, "ws-a").unwrap();

        assert_eq!(removed.name, "Workspace A");
        assert_eq!(state.conversation_workspaces.len(), 1);
        assert_eq!(state.conversation_workspaces[0].project_id, "ws-b");
        assert_eq!(state.conversation_pins.len(), 1);
        assert_eq!(state.conversation_pins[0].project_id, "ws-b");
        assert!(state.conversation_run_modes.get("ws-a").is_none());
        assert_eq!(state.last_conversation_workspace.as_deref(), Some("ws-b"));
    }
}
