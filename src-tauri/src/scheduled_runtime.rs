use camino::Utf8PathBuf;
use chrono::{DateTime, Duration, Utc};
use gold_band::app::{AcpTurnOutcome, App, RuntimeInterventionKind, RuntimeLifecycleEvent};
use gold_band::config::ConversationRunMode;
use gold_band::domain::{RunOutcome, RunStatus};
use gold_band::scheduler::db::ScheduledTaskDatabase;
use gold_band::scheduler::occurrence::{
    ClaimResult, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind, ScheduledError,
    ScheduledErrorCode, ScheduledOccurrence,
};
use gold_band::scheduler::store::ScheduledTaskStore;
use gold_band::scheduler::{
    OverlapPolicy, ScheduleKind, ScheduledMode, ScheduledTaskDefinition, SessionPolicy,
};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::commands::acp_live_update_emitter_for_app;
use crate::commands::acp_session_update_emitter;
use crate::state::DesktopState;
use crate::view_models_conversation::ConversationCreateInputVm;

const SCHEDULER_SUBSCRIBER_NAME: &str = "desktop.scheduled-runtime";
const SCHEDULER_EVENT: &str = "gold-band://scheduled-occurrence-updated";
const POLL_CAP: StdDuration = StdDuration::from_secs(1);
const LEASE_SECONDS: i64 = 60;
const MAX_MISSED_POINTS_PER_STARTUP: usize = 10_000;
pub const SCHEDULED_TASK_UPDATED_EVENT: &str = "gold-band://scheduled-task-updated";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskUpdatedEventVm {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub task_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledOccurrenceUpdatedEventVm {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
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

#[derive(Debug, Clone)]
struct ActiveOccurrence {
    database: ScheduledTaskDatabase,
    owner_id: String,
    project_id: String,
    scheduled_task_id: String,
}

#[derive(Clone)]
struct ScheduledRuntime {
    app_handle: AppHandle,
    owner_id: String,
    active: Arc<Mutex<HashMap<String, ActiveOccurrence>>>,
}

pub fn start(app_handle: AppHandle) {
    let state = app_handle.state::<DesktopState>();
    let Ok(runtime_app) = state.app() else {
        error!("failed to create scheduler runtime app");
        return;
    };
    let runtime = ScheduledRuntime {
        app_handle: app_handle.clone(),
        owner_id: format!("desktop-{}", Uuid::new_v4().simple()),
        active: Arc::new(Mutex::new(HashMap::new())),
    };
    let runtime_for_events = runtime.clone();
    runtime_app.lifecycle_bus.subscribe_named_with_mode(
        SCHEDULER_SUBSCRIBER_NAME,
        gold_band::app::observability::SubscriberMode::Inline,
        Arc::new(move |event| runtime_for_events.handle_lifecycle_event(event)),
    );
    thread::Builder::new()
        .name("scheduled-runtime".to_string())
        .spawn(move || runtime.run())
        .expect("scheduled runtime thread must start");
    info!("scheduled task scheduler started");
}

impl ScheduledRuntime {
    fn run(self) {
        let mut startup = true;
        loop {
            let now = Utc::now();
            let result = if startup {
                startup = false;
                self.tick(now, true)
            } else {
                self.tick(now, false)
            };
            let sleep_for = match result {
                Ok(next_wake) => next_wake
                    .map(|value| (value - Utc::now()).to_std().unwrap_or_default())
                    .unwrap_or(POLL_CAP)
                    .min(POLL_CAP),
                Err(error) => {
                    error!(%error, "scheduled task scheduler tick failed");
                    POLL_CAP
                }
            };
            thread::sleep(sleep_for.max(StdDuration::from_millis(50)));
        }
    }

    fn tick(&self, now: DateTime<Utc>, startup: bool) -> anyhow::Result<Option<DateTime<Utc>>> {
        self.renew_active_leases(now)?;
        let state = self.app_handle.state::<DesktopState>();
        let context = state.context()?;
        let global_app = state.app()?;
        let persisted = global_app.load_state()?;
        let mut workspaces = BTreeSet::new();
        workspaces.insert(context.repo_root.to_string());
        workspaces.extend(
            persisted
                .conversation_workspaces
                .iter()
                .map(|workspace| workspace.workspace_path.clone()),
        );

        let mut next_wake = None;
        for workspace in workspaces {
            let app = runtime_app_for_workspace(&state, &context, &workspace)?;
            let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
            if startup {
                if database.list_job_definitions()?.is_empty() {
                    let legacy_store = ScheduledTaskStore::new(app.paths.clone());
                    let _ = database.import_legacy_store(&legacy_store)?;
                }
                database.recover_expired(now)?;
            }
            for mut definition in database.list_job_definitions()? {
                if startup {
                    let missed = mark_past_points_missed(&database, &mut definition, now)?;
                    if missed {
                        database.save_job_definition(&definition)?;
                        emit_scheduled_task_updated(&self.app_handle, &definition);
                    }
                }
                if !definition.enabled {
                    continue;
                }
                if let Some(next) = definition.schedule.next_occurrence_after(now) {
                    next_wake = min_datetime(next_wake, Some(next));
                }
                self.process_definition(&database, &app, &mut definition, now)?;
            }
        }
        Ok(next_wake)
    }

    fn process_definition(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        definition: &mut ScheduledTaskDefinition,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        if definition.retry_at.is_some_and(|retry_at| retry_at > now) {
            return Ok(());
        }
        let Some(scheduled_at) = definition.next_due(now) else {
            return Ok(());
        };
        let occurrence = database.create_or_get_occurrence(
            definition.id(),
            scheduled_at,
            OccurrenceTriggerKind::Scheduled,
        )?;
        let lease_until = now + Duration::seconds(LEASE_SECONDS);
        let claim = database.claim_occurrence(&occurrence.id, &self.owner_id, now, lease_until)?;
        let claimed = match claim {
            ClaimResult::Claimed(value) => value,
            ClaimResult::AlreadyOwned => return Ok(()),
            ClaimResult::Busy => {
                if database
                    .get_occurrence(&occurrence.id)?
                    .is_some_and(|value| value.status.is_terminal())
                {
                    advance_definition_after_point(definition, scheduled_at, "completed", now);
                    database.save_job_definition(definition)?;
                }
                return Ok(());
            }
            ClaimResult::NotFound => return Ok(()),
        };
        self.register_active(&claimed, definition, database);

        if task_has_active_execution(app, definition.task_id.as_deref())? {
            let (status, error) = match definition.overlap_policy {
                OverlapPolicy::SkipWhenRunning => (OccurrenceStatus::Skipped, None),
                OverlapPolicy::RetryWhenBusy if definition.retry_count < 3 => {
                    definition.retry_count += 1;
                    definition.retry_at = Some(now + Duration::seconds(30));
                    (
                        OccurrenceStatus::Retrying,
                        Some(ScheduledError::new(ScheduledErrorCode::QueueBusy)),
                    )
                }
                OverlapPolicy::RetryWhenBusy => (
                    OccurrenceStatus::Skipped,
                    Some(ScheduledError::new(ScheduledErrorCode::QueueBusy)),
                ),
            };
            let finished =
                database.finish_occurrence(&claimed.id, &self.owner_id, status, None, error)?;
            self.remove_active(&claimed.id);
            if finished {
                if status == OccurrenceStatus::Retrying {
                    definition.last_trigger_status = Some("retrying".to_string());
                    definition.last_error = Some(ScheduledErrorCode::QueueBusy.to_string());
                    definition.updated_at = now;
                } else {
                    advance_definition_after_point(definition, scheduled_at, "skipped", now);
                }
                database.save_job_definition(definition)?;
                emit_scheduled_task_updated(&self.app_handle, definition);
            }
            return Ok(());
        }

        advance_definition_after_point(definition, scheduled_at, "running", now);
        if matches!(definition.schedule.kind, ScheduleKind::At { .. }) {
            definition.enabled = false;
        }
        database.save_job_definition(definition)?;
        match execute_definition(&self.app_handle, app, definition, &claimed.id) {
            Ok(execution) => {
                let _ = execution.immediate_links;
                database.save_job_definition(definition)?;
            }
            Err(error) => {
                self.finish_immediate_failure(
                    database,
                    definition,
                    &claimed,
                    ScheduledError::with_params(
                        ScheduledErrorCode::ExecutionFailed,
                        serde_json::json!({ "message": error.to_string() }),
                    ),
                )?;
            }
        }
        Ok(())
    }

    fn finish_immediate_failure(
        &self,
        database: &ScheduledTaskDatabase,
        definition: &mut ScheduledTaskDefinition,
        occurrence: &ScheduledOccurrence,
        error: ScheduledError,
    ) -> anyhow::Result<()> {
        let finished = database.finish_occurrence(
            &occurrence.id,
            &self.owner_id,
            OccurrenceStatus::Failed,
            None,
            Some(error),
        )?;
        self.remove_active(&occurrence.id);
        if finished {
            definition.last_trigger_status = Some("failed".to_string());
            definition.last_error = Some(ScheduledErrorCode::ExecutionFailed.to_string());
            definition.updated_at = Utc::now();
            database.save_job_definition(definition)?;
            emit_scheduled_task_updated(&self.app_handle, definition);
        }
        Ok(())
    }

    fn register_active(
        &self,
        occurrence: &ScheduledOccurrence,
        definition: &ScheduledTaskDefinition,
        database: &ScheduledTaskDatabase,
    ) {
        if let Ok(mut active) = self.active.lock() {
            active.insert(
                occurrence.id.clone(),
                ActiveOccurrence {
                    database: database.clone(),
                    owner_id: self.owner_id.clone(),
                    project_id: definition.project_id.clone(),
                    scheduled_task_id: definition.id.clone(),
                },
            );
        }
    }

    fn remove_active(&self, occurrence_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(occurrence_id);
        }
    }

    fn renew_active_leases(&self, now: DateTime<Utc>) -> anyhow::Result<()> {
        let active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduler active state lock poisoned"))?
            .clone();
        for (occurrence_id, lease) in active {
            if !lease.database.renew_lease(
                &occurrence_id,
                &lease.owner_id,
                now,
                now + Duration::seconds(LEASE_SECONDS),
            )? {
                self.remove_active(&occurrence_id);
            }
        }
        Ok(())
    }

    fn handle_lifecycle_event(&self, event: RuntimeLifecycleEvent) {
        let Some(occurrence_id) = scheduled_occurrence_id(&event) else {
            return;
        };
        let active = self
            .active
            .lock()
            .ok()
            .and_then(|mut active| active.remove(&occurrence_id));
        let Some(active) = active else {
            return;
        };
        match finish_occurrence_for_event(
            &active.database,
            &occurrence_id,
            &active.owner_id,
            &event,
        ) {
            Ok(Some(occurrence)) => {
                if let Ok(mut definitions) = active.database.list_job_definitions() {
                    if let Some(mut definition) = definitions
                        .drain(..)
                        .find(|definition| definition.id == active.scheduled_task_id)
                    {
                        definition.last_trigger_at = Some(occurrence.scheduled_at);
                        definition.last_trigger_status = Some(occurrence.status.to_string());
                        definition.last_error =
                            occurrence.error_code.map(|value| value.to_string());
                        if occurrence.task_id.is_some() {
                            definition.task_id = occurrence.task_id.clone();
                        }
                        definition.updated_at = Utc::now();
                        let _ = active.database.save_job_definition(&definition);
                        emit_scheduled_task_updated(&self.app_handle, &definition);
                    }
                }
                let _ = self.app_handle.emit(
                    SCHEDULER_EVENT,
                    ScheduledOccurrenceUpdatedEventVm {
                        project_id: active.project_id,
                        scheduled_task_id: active.scheduled_task_id,
                        occurrence_id: occurrence.id,
                        status: occurrence.status.to_string(),
                        error_code: occurrence.error_code.map(|value| value.to_string()),
                        task_id: occurrence.task_id,
                        run_id: occurrence.run_id,
                    },
                );
            }
            Ok(None) => {}
            Err(error) => warn!(%error, %occurrence_id, "failed to finish scheduled occurrence"),
        }
    }
}

fn runtime_app_for_workspace(
    state: &DesktopState,
    context: &crate::state::DesktopContext,
    workspace: &str,
) -> anyhow::Result<App> {
    let base = state.app()?;
    Ok(base.with_repo_root(Utf8PathBuf::from(workspace), context.config.clone()))
}

fn min_datetime(
    current: Option<DateTime<Utc>>,
    candidate: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (None, candidate) => candidate,
        (current, None) => current,
    }
}

fn advance_definition_after_point(
    definition: &mut ScheduledTaskDefinition,
    scheduled_at: DateTime<Utc>,
    status: &str,
    now: DateTime<Utc>,
) {
    definition.last_trigger_at = Some(scheduled_at);
    definition.last_trigger_status = Some(status.to_string());
    definition.last_error = None;
    definition.retry_count = 0;
    definition.retry_at = None;
    definition.updated_at = now;
}

pub(crate) fn mark_past_points_missed(
    database: &ScheduledTaskDatabase,
    definition: &mut ScheduledTaskDefinition,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let mut cursor = definition.last_trigger_at.unwrap_or_else(|| {
        definition
            .created_at
            .checked_sub_signed(Duration::seconds(1))
            .unwrap_or(definition.created_at)
    });
    let mut latest = None;
    for _ in 0..MAX_MISSED_POINTS_PER_STARTUP {
        let Some(next) = definition.schedule.next_occurrence_after(cursor) else {
            break;
        };
        if next >= now {
            break;
        }
        database.mark_missed(definition.id(), next)?;
        latest = Some(next);
        cursor = next;
    }
    let Some(latest) = latest else {
        return Ok(false);
    };
    advance_definition_after_point(definition, latest, "missed", now);
    Ok(true)
}

#[allow(dead_code)]
pub(crate) fn create_manual_occurrence(
    database: &ScheduledTaskDatabase,
    job_id: &str,
) -> anyhow::Result<ScheduledOccurrence> {
    Ok(database.create_or_get_occurrence(job_id, Utc::now(), OccurrenceTriggerKind::Manual)?)
}

fn scheduled_occurrence_id(event: &RuntimeLifecycleEvent) -> Option<String> {
    match event {
        RuntimeLifecycleEvent::RunPaused {
            scheduled_occurrence_id,
            ..
        }
        | RuntimeLifecycleEvent::InterventionRequested {
            scheduled_occurrence_id,
            ..
        }
        | RuntimeLifecycleEvent::RunCompleted {
            scheduled_occurrence_id,
            ..
        }
        | RuntimeLifecycleEvent::AcpTurnFinished {
            scheduled_occurrence_id,
            ..
        } => scheduled_occurrence_id.clone(),
        RuntimeLifecycleEvent::NodeStarted { .. } | RuntimeLifecycleEvent::NodeCompleted { .. } => {
            None
        }
    }
}

pub(crate) fn finish_occurrence_for_event(
    database: &ScheduledTaskDatabase,
    occurrence_id: &str,
    owner_id: &str,
    event: &RuntimeLifecycleEvent,
) -> anyhow::Result<Option<ScheduledOccurrence>> {
    let (status, links, error) = match event {
        RuntimeLifecycleEvent::RunCompleted {
            task_id,
            run_id,
            round_id,
            attempt_id,
            outcome,
            ..
        } => (
            occurrence_status_for_run_outcome(*outcome),
            Some(OccurrenceLinks {
                task_id: Some(task_id.clone()),
                run_id: Some(run_id.clone()),
                round_id: Some(round_id.clone()),
                attempt_id: Some(attempt_id.clone()),
            }),
            None,
        ),
        RuntimeLifecycleEvent::AcpTurnFinished {
            task_id,
            run_id,
            round_id,
            node_id: _,
            attempt_id,
            outcome,
            ..
        } => (
            match outcome {
                AcpTurnOutcome::Completed => OccurrenceStatus::Succeeded,
                AcpTurnOutcome::Failed | AcpTurnOutcome::Cancelled => OccurrenceStatus::Failed,
            },
            Some(OccurrenceLinks {
                task_id: Some(task_id.clone()),
                run_id: Some(run_id.clone()),
                round_id: Some(round_id.clone()),
                attempt_id: Some(attempt_id.clone()),
            }),
            matches!(outcome, AcpTurnOutcome::Failed | AcpTurnOutcome::Cancelled)
                .then(|| ScheduledError::new(ScheduledErrorCode::ExecutionFailed)),
        ),
        RuntimeLifecycleEvent::InterventionRequested {
            task_id,
            run_id,
            round_id,
            attempt_id,
            kind,
            ..
        } => {
            let (status, code) = match kind {
                RuntimeInterventionKind::PermissionRequested => (
                    OccurrenceStatus::Failed,
                    ScheduledErrorCode::PermissionRequired,
                ),
                RuntimeInterventionKind::ElicitationRequested
                | RuntimeInterventionKind::ManualDecisionRequired => (
                    OccurrenceStatus::AttentionRequired,
                    ScheduledErrorCode::UserInputRequired,
                ),
                RuntimeInterventionKind::RuntimeAbnormal
                | RuntimeInterventionKind::ErrorBlocked
                | RuntimeInterventionKind::ProcessInterrupted => (
                    OccurrenceStatus::Failed,
                    ScheduledErrorCode::ExecutionFailed,
                ),
            };
            (
                status,
                Some(OccurrenceLinks {
                    task_id: Some(task_id.clone()),
                    run_id: Some(run_id.clone()),
                    round_id: Some(round_id.clone()),
                    attempt_id: Some(attempt_id.clone()),
                }),
                Some(ScheduledError::new(code)),
            )
        }
        RuntimeLifecycleEvent::RunPaused { .. }
        | RuntimeLifecycleEvent::NodeStarted { .. }
        | RuntimeLifecycleEvent::NodeCompleted { .. } => return Ok(None),
    };
    if !database.finish_occurrence(occurrence_id, owner_id, status, links, error)? {
        return Ok(None);
    }
    Ok(database.get_occurrence(occurrence_id)?)
}

fn occurrence_status_for_run_outcome(outcome: RunOutcome) -> OccurrenceStatus {
    match outcome {
        RunOutcome::Success => OccurrenceStatus::Succeeded,
        RunOutcome::Failure | RunOutcome::Killed => OccurrenceStatus::Failed,
    }
}

#[derive(Debug, Clone)]
struct ExecutionResult {
    immediate_links: Option<OccurrenceLinks>,
}

fn task_has_active_execution(app: &App, task_id: Option<&str>) -> anyhow::Result<bool> {
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    Ok(app
        .run_list(task_id)?
        .into_iter()
        .any(|run| matches!(run.status, RunStatus::Running | RunStatus::Paused)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScheduledExecutionAction {
    MaterializeTaskAndRun,
    StartNewRun { task_id: String },
    ContinueSession { task_id: String },
}

fn scheduled_execution_action(definition: &ScheduledTaskDefinition) -> ScheduledExecutionAction {
    match definition.mode {
        ScheduledMode::Direct if definition.session_policy == SessionPolicy::New => {
            ScheduledExecutionAction::MaterializeTaskAndRun
        }
        ScheduledMode::Direct => definition
            .task_id
            .as_ref()
            .map(|task_id| ScheduledExecutionAction::ContinueSession {
                task_id: task_id.clone(),
            })
            .unwrap_or(ScheduledExecutionAction::MaterializeTaskAndRun),
        ScheduledMode::Workflow | ScheduledMode::Auto => definition
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
            (ScheduledMode::Direct, SessionPolicy::New)
        )
        && task_fingerprint != Some(definition.content_fingerprint.as_str())
    {
        return ScheduledExecutionAction::MaterializeTaskAndRun;
    }
    scheduled_execution_action(definition)
}

fn execute_definition(
    app_handle: &AppHandle,
    app: &App,
    definition: &mut ScheduledTaskDefinition,
    occurrence_id: &str,
) -> anyhow::Result<ExecutionResult> {
    let task_fingerprint = definition.task_id.as_deref().and_then(|task_id| {
        crate::view_models_conversation::scheduled_content_fingerprint_for_task(app, task_id)
    });
    match scheduled_execution_action_for_fingerprint(definition, task_fingerprint.as_deref()) {
        ScheduledExecutionAction::ContinueSession { task_id } => {
            if let Some((run_id, round_id, node_id, attempt_id)) = latest_attempt(app, &task_id)? {
                let input = scheduled_create_input(app, definition)?;
                let scheduled_app = app
                    .clone_for_background()
                    .with_scheduled_occurrence_id(Some(occurrence_id.to_string()));
                let live_update = acp_live_update_emitter_for_app(
                    &scheduled_app,
                    app_handle.clone(),
                    Some(definition.project_id.clone()),
                );
                let background_app = scheduled_app.clone_for_background();
                let scheduled_app = scheduled_app
                    .with_acp_live_update(live_update)
                    .with_acp_session_update(acp_session_update_emitter(
                        app_handle.clone(),
                        background_app,
                        Some(definition.project_id.clone()),
                    ));
                let handle = app_handle.clone();
                let project_id = Some(definition.project_id.clone());
                let task_id_for_thread = task_id.clone();
                let run_id_for_thread = run_id.clone();
                let round_id_for_thread = round_id.clone();
                let node_id_for_thread = node_id.clone();
                let attempt_id_for_thread = attempt_id.clone();
                thread::spawn(move || {
                    let result =
                        tauri::async_runtime::block_on(crate::commands::send_acp_prompt_with_app(
                            handle,
                            scheduled_app,
                            project_id,
                            task_id_for_thread,
                            run_id_for_thread,
                            round_id_for_thread,
                            node_id_for_thread,
                            attempt_id_for_thread,
                            input.content,
                            None,
                            None,
                            None,
                            input.attachment_paths,
                        ));
                    if let Err(error) = result {
                        warn!(%error.code, "scheduled continuous prompt failed");
                    }
                });
                return Ok(ExecutionResult {
                    immediate_links: Some(OccurrenceLinks {
                        task_id: Some(task_id),
                        run_id: Some(run_id),
                        round_id: Some(round_id),
                        attempt_id: Some(attempt_id),
                    }),
                });
            }
        }
        ScheduledExecutionAction::StartNewRun { task_id } => {
            let scheduled_app = app
                .clone_for_background()
                .with_scheduled_occurrence_id(Some(occurrence_id.to_string()));
            let live_update = acp_live_update_emitter_for_app(
                &scheduled_app,
                app_handle.clone(),
                Some(definition.project_id.clone()),
            );
            let background_app = scheduled_app.clone_for_background();
            let scheduled_app = scheduled_app
                .with_acp_live_update(live_update)
                .with_acp_session_update(acp_session_update_emitter(
                    app_handle.clone(),
                    background_app,
                    Some(definition.project_id.clone()),
                ));
            let run = scheduled_app.run_start_background(&task_id, None)?;
            return Ok(ExecutionResult {
                immediate_links: Some(OccurrenceLinks {
                    task_id: Some(task_id),
                    run_id: Some(run.id),
                    round_id: run.current_round,
                    attempt_id: run.current_attempt,
                }),
            });
        }
        ScheduledExecutionAction::MaterializeTaskAndRun => {}
    }

    let input = scheduled_create_input(app, definition)?;
    let scheduled_app = app
        .clone_for_background()
        .with_scheduled_occurrence_id(Some(occurrence_id.to_string()));
    let live_update = acp_live_update_emitter_for_app(
        &scheduled_app,
        app_handle.clone(),
        Some(definition.project_id.clone()),
    );
    let background_app = scheduled_app.clone_for_background();
    let run_app = scheduled_app
        .with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(
            app_handle.clone(),
            background_app,
            Some(definition.project_id.clone()),
        ));
    let run = crate::view_models_conversation::create_conversation_run_vm(&run_app, &input)?;
    definition.task_id = Some(run.task_id.clone());
    Ok(ExecutionResult {
        immediate_links: Some(OccurrenceLinks {
            task_id: Some(run.task_id),
            run_id: Some(run.run_id),
            round_id: None,
            attempt_id: None,
        }),
    })
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
    Ok(Some((
        run.id,
        run.current_round.expect("checked above"),
        run.current_node.expect("checked above"),
        run.current_attempt.expect("checked above"),
    )))
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
    use chrono::{Duration, TimeZone, Utc};
    use gold_band::app::{RuntimeInterventionKind, RuntimeLifecycleEvent};
    use gold_band::domain::RunOutcome;
    use gold_band::scheduler::db::ScheduledTaskDatabase;
    use gold_band::scheduler::occurrence::{
        OccurrenceStatus, OccurrenceTriggerKind, ScheduledErrorCode,
    };
    use gold_band::scheduler::{
        OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition, SessionPolicy,
    };
    use tempfile::tempdir;

    use super::{
        ScheduledExecutionAction, create_manual_occurrence, finish_occurrence_for_event,
        mark_past_points_missed, scheduled_execution_action,
        scheduled_execution_action_for_fingerprint,
    };

    fn claimed_occurrence() -> (ScheduledTaskDatabase, String, String) {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let scheduled_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let occurrence = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let owner_id = "scheduler-test".to_string();
        assert!(
            database
                .claim_occurrence(
                    &occurrence.id,
                    &owner_id,
                    scheduled_at,
                    scheduled_at + Duration::minutes(5),
                )
                .unwrap()
                .is_claimed()
        );
        std::mem::forget(directory);
        (database, occurrence.id, owner_id)
    }

    #[test]
    fn scheduled_run_completion_finishes_matching_occurrence() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        finish_occurrence_for_event(
            &database,
            &occurrence_id,
            &owner_id,
            &RuntimeLifecycleEvent::RunCompleted {
                event_id: "event-1".to_string(),
                occurred_at: "2026-08-03T12:01:00Z".to_string(),
                scheduled_occurrence_id: Some(occurrence_id.clone()),
                task_id: "task-1".to_string(),
                run_id: "run-1".to_string(),
                round_id: "round-1".to_string(),
                node_id: "node-1".to_string(),
                attempt_id: "attempt-1".to_string(),
                node_label: "node".to_string(),
                outcome: RunOutcome::Success,
                task_title: None,
                completion_agent_label: None,
            },
        )
        .unwrap();
        let occurrence = database.get_occurrence(&occurrence_id).unwrap().unwrap();
        assert_eq!(occurrence.status, OccurrenceStatus::Succeeded);
        assert_eq!(occurrence.task_id.as_deref(), Some("task-1"));
        assert_eq!(occurrence.run_id.as_deref(), Some("run-1"));
    }

    #[test]
    fn scheduled_turn_failure_finishes_occurrence_as_failed() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        finish_occurrence_for_event(
            &database,
            &occurrence_id,
            &owner_id,
            &RuntimeLifecycleEvent::AcpTurnFinished {
                event_id: "turn-1".to_string(),
                occurred_at: "2026-08-03T12:01:00Z".to_string(),
                scheduled_occurrence_id: Some(occurrence_id.clone()),
                task_id: "task-1".to_string(),
                run_id: "run-1".to_string(),
                round_id: "round-1".to_string(),
                node_id: "node-1".to_string(),
                attempt_id: "attempt-1".to_string(),
                turn_id: "turn-1".to_string(),
                agent_label: "agent".to_string(),
                outcome: gold_band::app::AcpTurnOutcome::Failed,
                task_title: None,
            },
        )
        .unwrap();
        assert_eq!(
            database
                .get_occurrence(&occurrence_id)
                .unwrap()
                .unwrap()
                .status,
            OccurrenceStatus::Failed
        );
    }

    #[test]
    fn scheduled_intervention_releases_lease_as_attention_required() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        finish_occurrence_for_event(
            &database,
            &occurrence_id,
            &owner_id,
            &RuntimeLifecycleEvent::InterventionRequested {
                event_id: "question-1".to_string(),
                occurred_at: "2026-08-03T12:01:00Z".to_string(),
                scheduled_occurrence_id: Some(occurrence_id.clone()),
                task_id: "task-1".to_string(),
                run_id: "run-1".to_string(),
                round_id: "round-1".to_string(),
                node_id: "node-1".to_string(),
                attempt_id: "attempt-1".to_string(),
                node_label: "node".to_string(),
                kind: RuntimeInterventionKind::ElicitationRequested,
                task_title: None,
            },
        )
        .unwrap();
        let occurrence = database.get_occurrence(&occurrence_id).unwrap().unwrap();
        assert_eq!(occurrence.status, OccurrenceStatus::AttentionRequired);
        assert_eq!(
            occurrence.error_code,
            Some(ScheduledErrorCode::UserInputRequired)
        );
        assert!(occurrence.owner_id.is_none());
        assert!(occurrence.lease_until.is_none());
    }

    #[test]
    fn startup_marks_past_points_missed_without_backfill() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let past = Utc.with_ymd_and_hms(2026, 8, 3, 10, 0, 0).unwrap();
        let mut definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(past),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.created_at = past - Duration::hours(1);
        let now = past + Duration::hours(1);
        mark_past_points_missed(&database, &mut definition, now).unwrap();
        let history = database.list_occurrences("job-1", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, OccurrenceStatus::Missed);
        assert_eq!(definition.last_trigger_at, Some(past));
        assert!(definition.next_due(now).is_none());
    }

    #[test]
    fn run_now_does_not_advance_next_scheduled_time() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let next = Utc.with_ymd_and_hms(2026, 8, 4, 10, 0, 0).unwrap();
        let definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(next),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let before = definition.next_due(next - Duration::hours(1));
        let occurrence = create_manual_occurrence(&database, "job-1").unwrap();
        assert_eq!(occurrence.trigger_kind, OccurrenceTriggerKind::Manual);
        assert_eq!(definition.next_due(next - Duration::hours(1)), before);
    }

    #[test]
    fn scheduler_uses_instruction_first_line_as_human_title() {
        assert_eq!(
            crate::view_models_conversation::scheduled_task_title("整理今日工作\n补充细节"),
            "整理今日工作"
        );
        assert!(
            crate::view_models_conversation::scheduled_task_title(&"a".repeat(60))
                .chars()
                .count()
                <= 49
        );
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
