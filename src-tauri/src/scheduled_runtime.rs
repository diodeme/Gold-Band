use camino::Utf8PathBuf;
use chrono::{DateTime, Duration, Utc};
use gold_band::app::App;
use gold_band::config::ConversationRunMode;
use gold_band::scheduler::{OverlapPolicy, ScheduleKind, ScheduledTaskDefinition, SessionPolicy};
use std::thread;
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

use crate::commands::acp_live_update_emitter_for_app;
use crate::commands::acp_session_update_emitter;
use crate::conversation_workspace::app_for_workspace;
use crate::state::DesktopState;
use crate::view_models_conversation::ConversationCreateInputVm;

const POLL_INTERVAL: StdDuration = StdDuration::from_secs(1);
pub const SCHEDULED_TASK_UPDATED_EVENT: &str = "gold-band://scheduled-task-updated";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskUpdatedEventVm {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub task_id: Option<String>,
    pub status: String,
}

pub fn emit_scheduled_task_updated(app_handle: &AppHandle, definition: &ScheduledTaskDefinition) {
    let _ = app_handle.emit(
        SCHEDULED_TASK_UPDATED_EVENT,
        ScheduledTaskUpdatedEventVm {
            project_id: definition.project_id.clone(),
            scheduled_task_id: definition.id.clone(),
            task_id: definition.task_id.clone(),
            status: definition.last_trigger_status.clone().unwrap_or_else(|| {
                if definition.enabled {
                    "enabled"
                } else {
                    "paused"
                }
                .to_string()
            }),
        },
    );
}

fn save_and_emit(
    app_handle: &AppHandle,
    store: &gold_band::scheduler::store::ScheduledTaskStore,
    definition: &ScheduledTaskDefinition,
) -> anyhow::Result<()> {
    store.save(definition)?;
    emit_scheduled_task_updated(app_handle, definition);
    Ok(())
}

pub fn start(app_handle: AppHandle) {
    info!("scheduled task scheduler started");
    thread::spawn(move || {
        loop {
            if let Err(error) = tick(&app_handle) {
                error!(%error, "scheduled task scheduler tick failed");
            }
            thread::sleep(POLL_INTERVAL);
        }
    });
}

fn tick(app_handle: &AppHandle) -> anyhow::Result<()> {
    let state = app_handle.state::<DesktopState>();
    let context = state.context()?;
    let global_app = state.app()?;
    let persisted = global_app.load_state()?;
    let mut workspaces = std::collections::BTreeSet::new();
    workspaces.insert(context.repo_root.clone());
    workspaces.extend(
        persisted
            .conversation_workspaces
            .iter()
            .map(|workspace| Utf8PathBuf::from(workspace.workspace_path.clone())),
    );

    for workspace in workspaces {
        let app = app_for_workspace(&context, workspace.as_str())?;
        let store = gold_band::scheduler::store::ScheduledTaskStore::new(app.paths.clone());
        for definition in store.list()? {
            if definition.enabled {
                process_definition(app_handle, &app, &store, definition)?;
            }
        }
    }
    Ok(())
}

fn process_definition(
    app_handle: &AppHandle,
    app: &App,
    store: &gold_band::scheduler::store::ScheduledTaskStore,
    mut definition: ScheduledTaskDefinition,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let Some(due_at) = definition.next_due(now) else {
        return Ok(());
    };

    if let Some(retry_at) = definition.retry_at {
        if retry_at > now {
            return Ok(());
        }
    }

    if task_has_active_execution(app, definition.task_id.as_deref())? {
        match definition.overlap_policy {
            OverlapPolicy::SkipWhenRunning => {
                mark_trigger(&mut definition, due_at, "skipped", None);
                save_and_emit(app_handle, store, &definition)?;
                append_trigger_record(
                    store,
                    &definition,
                    due_at,
                    "skipped",
                    definition.task_id.clone(),
                    latest_run_id(app, definition.task_id.as_deref())?,
                    1,
                )?;
                info!(scheduled_task_id = %definition.id, reason = "active-execution", "scheduled task skipped");
                return Ok(());
            }
            OverlapPolicy::RetryWhenBusy if definition.retry_count < 3 => {
                definition.retry_count += 1;
                definition.retry_at = Some(now + Duration::seconds(30));
                definition.last_trigger_status = Some("retrying".to_string());
                definition.updated_at = now;
                save_and_emit(app_handle, store, &definition)?;
                info!(scheduled_task_id = %definition.id, retry_count = definition.retry_count, "scheduled task deferred by queue protection");
                return Ok(());
            }
            OverlapPolicy::RetryWhenBusy => {
                let attempts = u32::from(definition.retry_count).saturating_add(1);
                mark_trigger(&mut definition, due_at, "skipped", Some("queue-busy"));
                save_and_emit(app_handle, store, &definition)?;
                append_trigger_record(
                    store,
                    &definition,
                    due_at,
                    "skipped",
                    definition.task_id.clone(),
                    latest_run_id(app, definition.task_id.as_deref())?,
                    attempts,
                )?;
                info!(scheduled_task_id = %definition.id, reason = "queue-busy", "scheduled task skipped after retries");
                return Ok(());
            }
        }
    }

    let attempts = u32::from(definition.retry_count).saturating_add(1);
    match execute_definition(app_handle, app, &mut definition) {
        Ok(task_id) => {
            definition.task_id = Some(task_id.clone());
            mark_trigger(&mut definition, due_at, "completed", None);
            if matches!(definition.schedule.kind, ScheduleKind::At { .. }) {
                definition.enabled = false;
            }
            save_and_emit(app_handle, store, &definition)?;
            append_trigger_record(
                store,
                &definition,
                due_at,
                "completed",
                Some(task_id),
                latest_run_id(app, definition.task_id.as_deref())?,
                attempts,
            )?;
            info!(scheduled_task_id = %definition.id, "scheduled task triggered");
        }
        Err(error) => {
            mark_trigger(&mut definition, due_at, "failed", Some(&error.to_string()));
            save_and_emit(app_handle, store, &definition)?;
            append_trigger_record(
                store,
                &definition,
                due_at,
                "failed",
                definition.task_id.clone(),
                latest_run_id(app, definition.task_id.as_deref())?,
                attempts,
            )?;
            warn!(scheduled_task_id = %definition.id, %error, "scheduled task execution failed");
        }
    }
    Ok(())
}

fn mark_trigger(
    definition: &mut ScheduledTaskDefinition,
    due_at: DateTime<Utc>,
    status: &str,
    error: Option<&str>,
) {
    definition.last_trigger_at = Some(due_at);
    definition.last_trigger_status = Some(status.to_string());
    definition.last_error = error.map(ToOwned::to_owned);
    definition.retry_count = 0;
    definition.retry_at = None;
    definition.updated_at = Utc::now();
}

fn task_has_active_execution(app: &App, task_id: Option<&str>) -> anyhow::Result<bool> {
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    Ok(app.run_list(task_id)?.into_iter().any(|run| {
        matches!(
            run.status,
            gold_band::domain::RunStatus::Running | gold_band::domain::RunStatus::Paused
        )
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScheduledExecutionAction {
    MaterializeTaskAndRun,
    StartNewRun { task_id: String },
    ContinueSession { task_id: String },
}

fn scheduled_execution_action(definition: &ScheduledTaskDefinition) -> ScheduledExecutionAction {
    match definition.mode {
        gold_band::scheduler::ScheduledMode::Direct
            if definition.session_policy == SessionPolicy::New =>
        {
            ScheduledExecutionAction::MaterializeTaskAndRun
        }
        gold_band::scheduler::ScheduledMode::Direct => definition
            .task_id
            .as_ref()
            .map(|task_id| ScheduledExecutionAction::ContinueSession {
                task_id: task_id.clone(),
            })
            .unwrap_or(ScheduledExecutionAction::MaterializeTaskAndRun),
        gold_band::scheduler::ScheduledMode::Workflow
        | gold_band::scheduler::ScheduledMode::Auto => definition
            .task_id
            .as_ref()
            .map(|task_id| ScheduledExecutionAction::StartNewRun {
                task_id: task_id.clone(),
            })
            .unwrap_or(ScheduledExecutionAction::MaterializeTaskAndRun),
    }
}

fn scheduled_execution_action_for_fingerprint(
    definition: &ScheduledTaskDefinition,
    task_fingerprint: Option<&str>,
) -> ScheduledExecutionAction {
    if definition.task_id.is_some()
        && !matches!(
            (definition.mode, definition.session_policy),
            (
                gold_band::scheduler::ScheduledMode::Direct,
                SessionPolicy::New
            )
        )
        && task_fingerprint != Some(definition.content_fingerprint.as_str())
    {
        return ScheduledExecutionAction::MaterializeTaskAndRun;
    }
    scheduled_execution_action(definition)
}

fn append_trigger_record(
    store: &gold_band::scheduler::store::ScheduledTaskStore,
    definition: &ScheduledTaskDefinition,
    scheduled_at: DateTime<Utc>,
    status: &str,
    task_id: Option<String>,
    run_id: Option<String>,
    attempts: u32,
) -> anyhow::Result<()> {
    store.append_trigger(gold_band::scheduler::store::ScheduledTriggerRecord::new(
        definition.id.clone(),
        scheduled_at,
        status,
        task_id,
        run_id,
        attempts,
    ))?;
    Ok(())
}

fn latest_run_id(app: &App, task_id: Option<&str>) -> anyhow::Result<Option<String>> {
    Ok(task_id
        .map(|task_id| app.run_list(task_id))
        .transpose()?
        .and_then(|runs| runs.into_iter().last().map(|run| run.id)))
}

fn execute_definition(
    app_handle: &AppHandle,
    app: &App,
    definition: &mut ScheduledTaskDefinition,
) -> anyhow::Result<String> {
    let task_fingerprint = definition.task_id.as_deref().and_then(|task_id| {
        crate::view_models_conversation::scheduled_content_fingerprint_for_task(app, task_id)
    });
    match scheduled_execution_action_for_fingerprint(definition, task_fingerprint.as_deref()) {
        ScheduledExecutionAction::ContinueSession { task_id } => {
            if let Some((run_id, round_id, node_id, attempt_id)) = latest_attempt(app, &task_id)? {
                let input = scheduled_create_input(app, definition)?;
                let state = app_handle.state::<DesktopState>();
                let result = tauri::async_runtime::block_on(crate::commands::send_acp_prompt(
                    app_handle.clone(),
                    state,
                    Some(definition.project_id.clone()),
                    task_id.clone(),
                    run_id,
                    round_id,
                    node_id,
                    attempt_id,
                    input.content,
                    None,
                    None,
                    None,
                    input.attachment_paths,
                ));
                if let Err(error) = result {
                    anyhow::bail!(error.code);
                }
                return Ok(task_id);
            }
        }
        ScheduledExecutionAction::StartNewRun { task_id } => {
            app.run_start_background(&task_id, None)?;
            return Ok(task_id);
        }
        ScheduledExecutionAction::MaterializeTaskAndRun => {}
    }

    let input = scheduled_create_input(app, definition)?;
    let app_base = app.clone_for_background();
    let live_update = acp_live_update_emitter_for_app(
        &app_base,
        app_handle.clone(),
        Some(definition.project_id.clone()),
    );
    let background_app = app_base.clone_for_background();
    let run_app = app_base
        .with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(
            app_handle.clone(),
            background_app,
            Some(definition.project_id.clone()),
        ));
    let run = crate::view_models_conversation::create_conversation_run_vm(&run_app, &input)?;
    Ok(run.task_id)
}

fn latest_attempt(
    app: &App,
    task_id: &str,
) -> anyhow::Result<Option<(String, String, String, String)>> {
    let Some(run) = app.run_list(task_id)?.into_iter().rev().find(|run| {
        run.current_round.is_some() && run.current_node.is_some() && run.current_attempt.is_some()
    }) else {
        return Ok(None);
    };
    let Some(round_id) = run.current_round else {
        return Ok(None);
    };
    let Some(node_id) = run.current_node else {
        return Ok(None);
    };
    let Some(attempt_id) = run.current_attempt else {
        return Ok(None);
    };
    Ok(Some((run.id, round_id, node_id, attempt_id)))
}

fn scheduled_create_input(
    app: &App,
    definition: &ScheduledTaskDefinition,
) -> anyhow::Result<ConversationCreateInputVm> {
    let config = &definition.execution_config;
    let direct_config = config
        .get("directConfig")
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value)
        .transpose()?;
    let auto_config = config
        .get("autoConfig")
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value)
        .transpose()?;
    let workflow_template_id = config
        .get("workflowTemplateId")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let include_interview = config
        .get("includeInterview")
        .and_then(|value| value.as_bool());
    let input_dir = app.paths.scheduled_task_dir(&definition.id).join("inputs");
    let attachment_paths = definition
        .attachment_names
        .iter()
        .map(|name| input_dir.join(name).to_string())
        .filter(|path| std::path::Path::new(path).is_file())
        .collect::<Vec<_>>();
    Ok(ConversationCreateInputVm {
        project_id: definition.project_id.clone(),
        content: definition.instruction.clone(),
        run_mode: config
            .get("runMode")
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| ConversationRunMode::Direct.as_str())
            .to_string(),
        workflow_template_id,
        include_interview,
        direct_config,
        auto_config,
        attachment_paths: (!attachment_paths.is_empty()).then_some(attachment_paths),
        scheduled_task_id: Some(definition.id.clone()),
        scheduled_content_fingerprint: Some(definition.content_fingerprint.clone()),
    })
}

#[cfg(test)]
mod tests {
    use crate::view_models_conversation::scheduled_task_title;
    use chrono::{TimeZone, Utc};
    use gold_band::scheduler::{
        OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition, SessionPolicy,
    };

    use super::{
        ScheduledExecutionAction, scheduled_execution_action,
        scheduled_execution_action_for_fingerprint,
    };

    #[test]
    fn scheduler_uses_instruction_first_line_as_human_title() {
        assert_eq!(
            scheduled_task_title("整理今日工作\n补充细节"),
            "整理今日工作"
        );
        assert!(scheduled_task_title(&"a".repeat(60)).chars().count() <= 49);
    }

    #[test]
    fn workflow_and_auto_repeated_triggers_start_new_run_on_existing_task() {
        for mode in ["workflow", "auto"] {
            let mut definition = ScheduledTaskDefinition::new(
                "project-a",
                &format!("scheduled-{mode}"),
                mode,
                ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
                OverlapPolicy::SkipWhenRunning,
            )
            .unwrap()
            .with_session_policy(SessionPolicy::New)
            .unwrap();
            definition.task_id = Some("task-001".to_string());

            assert_eq!(
                scheduled_execution_action(&definition),
                ScheduledExecutionAction::StartNewRun {
                    task_id: "task-001".to_string(),
                }
            );
        }
    }

    #[test]
    fn direct_new_always_materializes_a_new_task() {
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-direct",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.task_id = Some("task-001".to_string());

        assert_eq!(
            scheduled_execution_action(&definition),
            ScheduledExecutionAction::MaterializeTaskAndRun
        );
    }

    #[test]
    fn direct_continuous_reuses_only_the_associated_task_chain() {
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-direct-continuous",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap()
        .with_session_policy(SessionPolicy::Continuous)
        .unwrap();

        assert_eq!(
            scheduled_execution_action(&definition),
            ScheduledExecutionAction::MaterializeTaskAndRun
        );
        definition.task_id = Some("task-001".to_string());
        assert_eq!(
            scheduled_execution_action(&definition),
            ScheduledExecutionAction::ContinueSession {
                task_id: "task-001".to_string()
            }
        );
    }

    #[test]
    fn authoring_fingerprint_change_materializes_a_new_workflow_task() {
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-workflow",
            "workflow",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.task_id = Some("task-001".to_string());
        definition.content_fingerprint = "sha256:new".to_string();

        assert_eq!(
            scheduled_execution_action_for_fingerprint(&definition, Some("sha256:old")),
            ScheduledExecutionAction::MaterializeTaskAndRun
        );
        assert_eq!(
            scheduled_execution_action_for_fingerprint(&definition, Some("sha256:new")),
            ScheduledExecutionAction::StartNewRun {
                task_id: "task-001".to_string(),
            }
        );
    }
}
