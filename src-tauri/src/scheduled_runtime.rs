use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Duration, Utc};
use gold_band::acp::client::prompt_activity;
use gold_band::app::{AcpTurnOutcome, App, RuntimeInterventionKind, RuntimeLifecycleEvent};
use gold_band::config::ConversationRunMode;
use gold_band::domain::{RunOutcome, RunStatus};
use gold_band::runtime::RunState;
use gold_band::scheduler::coordinator::{DeadlineRegistry, ReconcileReason, ScheduledJobKey};
use gold_band::scheduler::db::{
    DueMaterialization, RecoverableScheduledJob, ScheduledJobRecord, ScheduledTaskDatabase,
    UpdateJobResult,
};
use gold_band::scheduler::occurrence::{
    ClaimResult, LeaseConfig, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind,
    ScheduledError, ScheduledErrorCode, ScheduledOccurrence,
};
use gold_band::scheduler::queue::{
    ActiveExecution, DEFAULT_OCCURRENCE_RETENTION_DAYS, LATE_FIRE_GRACE,
    MISSED_RECONCILE_BATCH_SIZE, QueueDecision, RETENTION_DELETE_BATCH_SIZE, decide_queue,
};
use gold_band::scheduler::store::ScheduledTaskStore;
use gold_band::scheduler::{ScheduleKind, ScheduledMode, ScheduledTaskDefinition, SessionPolicy};
use gold_band::storage::GoldBandPaths;
use gold_band::workflow_model_binding::{
    TaskAuthoringWorkflow, TaskAuthoringWorkflowCompat, migrate_authoring_workflow,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::commands::configure_conversation_runtime_callbacks;
use crate::scheduled_service::{ManualRunResult, ScheduledServiceError, ScheduledServiceResult};
use crate::state::DesktopState;
use crate::view_models_conversation::ConversationCreateInputVm;

mod execution;
mod lease;
mod notification;
pub(crate) mod power;

use execution::{ScheduledExecutionContext, adapter_for};
pub(crate) use lease::OccurrenceExecutionGuard;
use notification::{
    SCHEDULED_NOTIFICATION_EVENT, missed_notification_event, notification_event_for_occurrence,
};

const SCHEDULER_SUBSCRIBER_NAME: &str = "desktop.scheduled-runtime";
const SCHEDULER_EVENT: &str = "gold-band://scheduled-occurrence-updated";
const WORKSPACE_REGISTRATION_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
const DEADLINE_FAILURE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
const CLOCK_DRIFT_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const CLOCK_DRIFT_TOLERANCE: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(test)]
const MAX_MISSED_POINTS_PER_STARTUP: usize = 10_000;
pub const SCHEDULED_TASK_UPDATED_EVENT: &str = "gold-band://scheduled-task-updated";

#[derive(Debug)]
pub enum SchedulerCommand {
    RegisterWorkspace {
        workspace_path: Utf8PathBuf,
    },
    RetryRegisterWorkspace {
        workspace_path: Utf8PathBuf,
    },
    UnregisterWorkspace {
        workspace_path: Utf8PathBuf,
    },
    JobCreated {
        key: ScheduledJobKey,
    },
    JobUpdated {
        key: ScheduledJobKey,
    },
    JobEnabled {
        key: ScheduledJobKey,
    },
    JobDisabled {
        key: ScheduledJobKey,
    },
    JobDeleted {
        key: ScheduledJobKey,
    },
    RunNow {
        key: ScheduledJobKey,
        reply: oneshot::Sender<ScheduledServiceResult<ManualRunResult>>,
    },
    ResumeAttention {
        workspace_path: Utf8PathBuf,
        task_id: String,
        run_id: String,
        round_id: String,
        attempt_id: String,
        reply: oneshot::Sender<ScheduledServiceResult<Option<String>>>,
    },
    SettingsChanged,
    CleanupWorkspace {
        workspace_path: Utf8PathBuf,
    },
    Reconcile {
        reason: ReconcileReason,
    },
    Shutdown {
        ack: oneshot::Sender<ScheduledServiceResult<()>>,
    },
}

#[derive(Clone)]
pub struct SchedulerCoordinatorHandle {
    sender: mpsc::UnboundedSender<SchedulerCommand>,
    task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
}

impl SchedulerCoordinatorHandle {
    fn new(sender: mpsc::UnboundedSender<SchedulerCommand>) -> Self {
        Self {
            sender,
            task: Arc::new(Mutex::new(None)),
        }
    }

    fn install_task(
        &self,
        task: tauri::async_runtime::JoinHandle<()>,
    ) -> ScheduledServiceResult<()> {
        let mut installed = self.task.lock().map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::CoordinatorUnavailable,
                serde_json::json!({ "operation": "install-scheduler-task" }),
            )
        })?;
        if installed.is_some() {
            return Err(ScheduledServiceError::new(
                ScheduledErrorCode::CoordinatorUnavailable,
                serde_json::json!({ "operation": "scheduler-task-already-installed" }),
            ));
        }
        *installed = Some(task);
        Ok(())
    }

    pub fn send(&self, command: SchedulerCommand) -> ScheduledServiceResult<()> {
        self.sender.send(command).map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::CoordinatorUnavailable,
                serde_json::json!({ "operation": "send-scheduler-command" }),
            )
        })
    }

    pub async fn run_now(&self, key: ScheduledJobKey) -> ScheduledServiceResult<ManualRunResult> {
        let (reply, receiver) = oneshot::channel();
        self.send(SchedulerCommand::RunNow { key, reply })?;
        receiver.await.map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::CoordinatorUnavailable,
                serde_json::json!({ "operation": "run-now-reply" }),
            )
        })?
    }

    pub async fn resume_attention(
        &self,
        workspace_path: Utf8PathBuf,
        task_id: String,
        run_id: String,
        round_id: String,
        attempt_id: String,
    ) -> ScheduledServiceResult<Option<String>> {
        let (reply, receiver) = oneshot::channel();
        self.send(SchedulerCommand::ResumeAttention {
            workspace_path,
            task_id,
            run_id,
            round_id,
            attempt_id,
            reply,
        })?;
        receiver.await.map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::CoordinatorUnavailable,
                serde_json::json!({ "operation": "resume-attention-reply" }),
            )
        })?
    }

    pub async fn shutdown(&self) -> ScheduledServiceResult<()> {
        let (ack, receiver) = oneshot::channel();
        let release_result = match self.send(SchedulerCommand::Shutdown { ack }) {
            Ok(()) => match receiver.await {
                Ok(result) => result,
                Err(_) => Err(ScheduledServiceError::new(
                    ScheduledErrorCode::CoordinatorUnavailable,
                    serde_json::json!({ "operation": "shutdown-ack" }),
                )),
            },
            Err(error) => Err(error),
        };
        let join_result = self.join_task().await;
        match release_result {
            Err(release_error) => Err(release_error),
            Ok(()) => join_result,
        }
    }

    async fn join_task(&self) -> ScheduledServiceResult<()> {
        let task = self
            .task
            .lock()
            .map_err(|_| {
                ScheduledServiceError::new(
                    ScheduledErrorCode::CoordinatorUnavailable,
                    serde_json::json!({ "operation": "take-scheduler-task" }),
                )
            })?
            .take();
        if let Some(task) = task {
            task.await.map_err(|error| {
                ScheduledServiceError::new(
                    ScheduledErrorCode::CoordinatorUnavailable,
                    serde_json::json!({
                        "operation": "join-scheduler-task",
                        "reason": error.to_string(),
                    }),
                )
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegisteredDeadline {
    revision: i64,
    scheduled_deadline: Option<DateTime<Utc>>,
    wake_at: DateTime<Utc>,
}

impl RegisteredDeadline {
    #[cfg(test)]
    fn from_record(record: &ScheduledJobRecord) -> Option<Self> {
        let deadline = record.next_run_at?;
        record.definition.enabled.then_some(Self {
            revision: record.revision,
            scheduled_deadline: Some(deadline),
            wake_at: deadline,
        })
    }

    fn matches(&self, record: &ScheduledJobRecord) -> bool {
        let scheduled_deadline = record
            .definition
            .enabled
            .then_some(record.next_run_at)
            .flatten();
        record.revision == self.revision && scheduled_deadline == self.scheduled_deadline
    }
}

#[derive(Clone)]
struct WorkspaceRegistration {
    app: Arc<App>,
    database: ScheduledTaskDatabase,
}

fn scheduled_task_context_info(
    definition: &ScheduledTaskDefinition,
    trigger_kind: &str,
    triggered_at: chrono::DateTime<chrono::Utc>,
) -> gold_band::provider::ScheduledTaskContextInfo {
    gold_band::provider::ScheduledTaskContextInfo {
        title: definition
            .instruction
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
        mode: match definition.mode {
            ScheduledMode::Direct => "direct",
            ScheduledMode::Workflow => "workflow",
            ScheduledMode::Auto => "auto",
        }
        .to_string(),
        session_policy: match definition.session_policy {
            gold_band::scheduler::SessionPolicy::New => "new".to_string(),
            gold_band::scheduler::SessionPolicy::Continuous => "continuous".to_string(),
        },
        trigger_kind: trigger_kind.to_string(),
        triggered_at: triggered_at.to_rfc3339(),
        instruction: Some(definition.instruction.clone()),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskUpdatedEventVm {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub task_id: Option<String>,
    pub status: String,
    /// Full VM snapshot so subscribers can merge locally without a full reload.
    pub task: Option<crate::view_models_conversation::ScheduledTaskVm>,
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

fn scheduled_occurrence_updated_event(
    project_id: &str,
    scheduled_task_id: &str,
    occurrence: &ScheduledOccurrence,
) -> ScheduledOccurrenceUpdatedEventVm {
    ScheduledOccurrenceUpdatedEventVm {
        project_id: project_id.to_string(),
        scheduled_task_id: scheduled_task_id.to_string(),
        occurrence_id: occurrence.id.clone(),
        status: occurrence.status.to_string(),
        error_code: occurrence.error_code.map(|value| value.to_string()),
        task_id: occurrence.task_id.clone(),
        run_id: occurrence.run_id.clone(),
    }
}

fn emit_scheduled_occurrence_updated(
    app_handle: &AppHandle,
    project_id: &str,
    scheduled_task_id: &str,
    occurrence: &ScheduledOccurrence,
) {
    let _ = app_handle.emit(
        SCHEDULER_EVENT,
        scheduled_occurrence_updated_event(project_id, scheduled_task_id, occurrence),
    );
}

pub fn emit_scheduled_task_updated(
    app_handle: &AppHandle,
    definition: &ScheduledTaskDefinition,
    next_run_at: Option<chrono::DateTime<chrono::Utc>>,
) {
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
            task: Some(
                crate::view_models_conversation::ScheduledTaskVm::from_definition(
                    definition,
                    next_run_at,
                ),
            ),
        },
    );
}

pub fn emit_scheduled_task_deleted(app_handle: &AppHandle, definition: &ScheduledTaskDefinition) {
    let _ = app_handle.emit(
        SCHEDULED_TASK_UPDATED_EVENT,
        ScheduledTaskUpdatedEventVm {
            project_id: definition.project_id.clone(),
            scheduled_task_id: definition.id.clone(),
            task_id: definition.task_id.clone(),
            status: "deleted".to_string(),
            task: None,
        },
    );
}

fn emit_scheduled_occurrence_notification(
    app_handle: &AppHandle,
    project_id: &str,
    occurrence: &ScheduledOccurrence,
) {
    let completion_notifications_enabled = app_handle
        .state::<DesktopState>()
        .context()
        .map(|context| context.config.scheduled_completion_notifications_enabled)
        .unwrap_or(true);
    let Some(event) =
        notification_event_for_occurrence(project_id, completion_notifications_enabled, occurrence)
    else {
        return;
    };
    if let Err(error) = app_handle.emit(SCHEDULED_NOTIFICATION_EVENT, event) {
        warn!(%error, "failed to emit scheduled notification event");
    }
}

fn emit_scheduled_missed_notification(
    app_handle: &AppHandle,
    project_id: &str,
    scheduled_task_id: &str,
    missed_count: u32,
) {
    if missed_count == 0 {
        return;
    }
    let event = missed_notification_event(project_id, scheduled_task_id, missed_count);
    if let Err(error) = app_handle.emit(SCHEDULED_NOTIFICATION_EVENT, event) {
        warn!(%error, "failed to emit scheduled missed notification event");
    }
}

#[derive(Debug, Clone)]
struct ActiveOccurrenceMetadata {
    database: ScheduledTaskDatabase,
    workspace_path: Utf8PathBuf,
    owner_id: String,
    project_id: String,
    scheduled_task_id: String,
    expected_revision: Option<i64>,
}

struct ActiveOccurrence {
    metadata: ActiveOccurrenceMetadata,
    guard: OccurrenceExecutionGuard,
}

type ActiveOccurrenceRegistry = Arc<Mutex<HashMap<String, ActiveOccurrence>>>;
type PendingGuardJoins = Arc<Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>>;

struct ClaimToHandoffGuard {
    active: ActiveOccurrenceRegistry,
    pending_stops: PendingGuardJoins,
    occurrence_id: String,
    lease: ActiveOccurrenceMetadata,
    armed: bool,
}

impl ClaimToHandoffGuard {
    fn new_with_pending(
        active: ActiveOccurrenceRegistry,
        pending_stops: PendingGuardJoins,
        occurrence_id: String,
        lease: ActiveOccurrenceMetadata,
    ) -> anyhow::Result<Self> {
        {
            let active_state = active
                .lock()
                .map_err(|_| anyhow::anyhow!("scheduler active state lock poisoned"))?;
            if active_state.contains_key(&occurrence_id) {
                anyhow::bail!("scheduled occurrence is already active: {occurrence_id}");
            }
        }
        let callback_active = active.clone();
        let callback_occurrence_id = occurrence_id.clone();
        let execution_guard = OccurrenceExecutionGuard::start(
            lease.database.clone(),
            occurrence_id.clone(),
            lease.owner_id.clone(),
            LeaseConfig::default(),
            move || {
                // The guard task owns the callback. Detach the registry entry here and let
                // the guard task finish naturally; joining from this callback would deadlock.
                detach_active_occurrence(&callback_active, &callback_occurrence_id);
            },
        );
        let guard = Self {
            active,
            pending_stops,
            occurrence_id,
            lease,
            armed: true,
        };
        let entry = ActiveOccurrence {
            metadata: guard.lease.clone(),
            guard: execution_guard,
        };
        let previous = guard
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduler active state lock poisoned"))?
            .insert(guard.occurrence_id.clone(), entry);
        if let Some(previous) = previous {
            let previous_lease = previous.metadata.clone();
            schedule_claim_cleanup(
                Some(previous),
                previous_lease,
                guard.occurrence_id.clone(),
                &guard.pending_stops,
            );
            anyhow::bail!(
                "scheduled occurrence is already active: {}",
                guard.occurrence_id
            );
        }
        Ok(guard)
    }

    async fn stop(&mut self) {
        if let Some(entry) = take_active_occurrence(&self.active, &self.occurrence_id) {
            entry.guard.stop().await;
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn handoff(mut self) {
        self.armed = false;
    }
}

impl Drop for ClaimToHandoffGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let entry = take_active_occurrence(&self.active, &self.occurrence_id);
        schedule_claim_cleanup(
            entry,
            self.lease.clone(),
            self.occurrence_id.clone(),
            &self.pending_stops,
        );
    }
}

fn detach_active_occurrence(active: &ActiveOccurrenceRegistry, occurrence_id: &str) {
    if let Ok(mut active) = active.lock() {
        active.remove(occurrence_id);
    }
}

fn take_active_occurrence(
    active: &ActiveOccurrenceRegistry,
    occurrence_id: &str,
) -> Option<ActiveOccurrence> {
    active.lock().ok()?.remove(occurrence_id)
}

fn schedule_claim_cleanup(
    entry: Option<ActiveOccurrence>,
    lease: ActiveOccurrenceMetadata,
    occurrence_id: String,
    pending_stops: &PendingGuardJoins,
) {
    let stop = entry.map(|entry| entry.guard.stop());
    let join = tauri::async_runtime::spawn(async move {
        if let Some(stop) = stop {
            stop.await;
        }
        let database = lease.database;
        let owner_id = lease.owner_id;
        let release = tokio::task::spawn_blocking(move || {
            database.release_owned_occurrence_for_retry(&occurrence_id, &owner_id, Utc::now())
        })
        .await;
        match release {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                warn!(%error, "failed to release occurrence before execution handoff")
            }
            Err(error) => {
                warn!(%error, "failed to join occurrence cleanup before execution handoff")
            }
        }
    });
    if let Ok(mut pending) = pending_stops.lock() {
        pending.push(join);
    }
}

async fn shutdown_active_occurrences(
    active: &ActiveOccurrenceRegistry,
    pending_guard_joins: &PendingGuardJoins,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let active = active
        .lock()
        .map_err(|_| anyhow::anyhow!("scheduler active state lock poisoned"))?
        .drain()
        .collect::<Vec<_>>();

    // Calling stop() synchronously publishes cancellation before any future is awaited.
    let mut release_inputs = Vec::with_capacity(active.len());
    let mut stops = Vec::with_capacity(active.len());
    for (occurrence_id, entry) in active {
        release_inputs.push((
            occurrence_id,
            entry.metadata.database.clone(),
            entry.metadata.owner_id.clone(),
        ));
        stops.push(entry.guard.stop());
    }
    for stop in stops {
        stop.await;
    }

    let pending_joins = {
        let mut pending = pending_guard_joins
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduler pending guard state lock poisoned"))?;
        std::mem::take(&mut *pending)
    };
    for join in pending_joins {
        let _ = join.await;
    }

    let failures = tokio::task::spawn_blocking(move || {
        let mut failures = Vec::new();
        for (occurrence_id, database, owner_id) in release_inputs {
            if let Err(error) =
                database.release_owned_occurrence_for_retry(&occurrence_id, &owner_id, now)
            {
                failures.push(format!("{occurrence_id}: {error}"));
            }
        }
        failures
    })
    .await
    .map_err(|error| anyhow::anyhow!("scheduled lease release task failed: {error}"))?;
    if !failures.is_empty() {
        anyhow::bail!(
            "failed to release scheduled occurrence leases: {}",
            failures.join(", ")
        );
    }
    Ok(())
}

#[derive(Clone)]
struct ScheduledRuntime {
    app_handle: AppHandle,
    owner_id: String,
    active: ActiveOccurrenceRegistry,
    pending_guard_joins: PendingGuardJoins,
}

struct SchedulerCoordinator<R = ScheduledRuntime> {
    runtime: Arc<R>,
    sender: mpsc::UnboundedSender<SchedulerCommand>,
    receiver: mpsc::UnboundedReceiver<SchedulerCommand>,
    deadlines: DeadlineRegistry,
    registered_deadlines: HashMap<ScheduledJobKey, RegisteredDeadline>,
    workspaces: HashMap<Utf8PathBuf, WorkspaceRegistration>,
    workspace_registration_retries: HashSet<Utf8PathBuf>,
    timer_drift_reconcile_pending: bool,
}

pub fn start(app_handle: AppHandle) -> anyhow::Result<()> {
    let state = app_handle.state::<DesktopState>();
    let runtime_app = state.app()?;
    let runtime = Arc::new(ScheduledRuntime {
        app_handle: app_handle.clone(),
        owner_id: format!("desktop-{}", Uuid::new_v4().simple()),
        active: Arc::new(Mutex::new(HashMap::new())),
        pending_guard_joins: Arc::new(Mutex::new(Vec::new())),
    });
    let runtime_for_events = runtime.clone();
    runtime_app.lifecycle_bus.subscribe_named_with_mode(
        SCHEDULER_SUBSCRIBER_NAME,
        gold_band::app::observability::SubscriberMode::Inline,
        Arc::new(move |event| runtime_for_events.handle_lifecycle_event(event)),
    );

    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = SchedulerCoordinatorHandle::new(sender.clone());
    state.install_scheduler_coordinator(handle.clone())?;
    let coordinator = SchedulerCoordinator::new_with_sender(runtime, sender, receiver);
    let task = tauri::async_runtime::spawn(async move {
        let _ = coordinator.run().await;
    });
    handle
        .install_task(task)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let context = state.context()?;
    let persisted = runtime_app.load_state()?;
    let mut workspaces = HashSet::new();
    workspaces.insert(context.repo_root);
    workspaces.extend(
        persisted
            .conversation_workspaces
            .into_iter()
            .map(|workspace| Utf8PathBuf::from(workspace.workspace_path)),
    );
    for workspace_path in workspaces {
        handle.send(SchedulerCommand::RegisterWorkspace { workspace_path })?;
    }
    info!("scheduled task scheduler started");
    Ok(())
}

impl ScheduledRuntime {
    async fn run_manual(
        &self,
        app: &App,
        job: &ScheduledJobRecord,
    ) -> anyhow::Result<ManualRunResult> {
        let mut expected_revision = Some(job.revision);
        let mut definition = job.definition.clone();
        ensure_definition_workspace(app, &definition)?;
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
        let occurrence =
            create_manual_occurrence(&database, &definition.project_id, definition.id())?;
        let now = Utc::now();
        let claim = database.claim_occurrence(
            &occurrence.id,
            &self.owner_id,
            now,
            LeaseConfig::default().lease_until(now),
        )?;
        let claimed = match claim {
            ClaimResult::Claimed(value) => value,
            ClaimResult::AlreadyOwned | ClaimResult::Busy => {
                anyhow::bail!("scheduled manual occurrence is already running")
            }
            ClaimResult::NotFound => anyhow::bail!("scheduled manual occurrence was not found"),
        };
        let mut handoff = self.claim_to_handoff_guard(
            &claimed,
            &definition,
            &database,
            &app.paths.repo_root,
            expected_revision,
        )?;
        let active_execution = active_execution_for_task(app, definition.task_id.as_deref())?;
        let queue_decision = decide_queue(
            definition.overlap_policy,
            active_execution,
            claimed.attempt.min(u8::MAX as u32) as u8,
            now,
        );
        let terminal_queue_result = match queue_decision {
            QueueDecision::StartNow => None,
            QueueDecision::RetryAt(retry_at) => {
                definition.retry_count = claimed.attempt.min(u8::MAX as u32) as u8;
                definition.retry_at = Some(retry_at);
                Some((
                    OccurrenceStatus::Retrying,
                    Some(ScheduledError::new(ScheduledErrorCode::QueueBusy)),
                ))
            }
            QueueDecision::Skipped => Some((
                OccurrenceStatus::Skipped,
                Some(ScheduledError::new(ScheduledErrorCode::QueueBusy)),
            )),
        };
        if let Some((status, error)) = terminal_queue_result {
            handoff.stop().await;
            let finished =
                database.finish_occurrence(&claimed.id, &self.owner_id, status, None, error)?;
            if !finished {
                anyhow::bail!("failed to finish busy manual occurrence");
            }
            handoff.disarm();
            if status == OccurrenceStatus::Retrying {
                definition.last_trigger_status = Some("retrying".to_string());
                definition.last_error = Some(ScheduledErrorCode::QueueBusy.to_string());
                definition.updated_at = now;
                self.persist_active_projection(
                    &claimed.id,
                    &database,
                    &mut definition,
                    &mut expected_revision,
                )?;
            }
            return Ok(ManualRunResult {
                occurrence: database
                    .get_occurrence(&claimed.id)?
                    .ok_or_else(|| anyhow::anyhow!("manual occurrence disappeared"))?,
                immediate_links: None,
            });
        }
        if let Some(error) = scheduled_agent_unattended_error(app, &definition) {
            self.finish_immediate_failure(
                &database,
                &mut definition,
                &claimed,
                &mut expected_revision,
                &mut handoff,
                error,
            )
            .await?;
            return Ok(ManualRunResult {
                occurrence: database
                    .get_occurrence(&claimed.id)?
                    .ok_or_else(|| anyhow::anyhow!("manual occurrence disappeared"))?,
                immediate_links: None,
            });
        }
        let execution = match execute_definition(
            &self.app_handle,
            app,
            &database,
            &self.owner_id,
            &mut definition,
            &claimed,
            &claimed.trigger_kind.to_string(),
        ) {
            Ok(execution) => execution,
            Err(error) => {
                self.finish_execution_failure(&database, &claimed, &mut handoff, &error)
                    .await?;
                return Err(error);
            }
        };
        handoff.handoff();
        if let Err(error) = self.persist_active_projection(
            &claimed.id,
            &database,
            &mut definition,
            &mut expected_revision,
        ) {
            warn!(%error, occurrence_id = %claimed.id, "accepted manual occurrence projection failed");
        }
        Ok(ManualRunResult {
            occurrence: database
                .get_occurrence(&claimed.id)?
                .ok_or_else(|| anyhow::anyhow!("manual occurrence disappeared"))?,
            immediate_links: execution.immediate_links,
        })
    }

    async fn process_occurrence(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        job: ScheduledJobRecord,
        occurrence: ScheduledOccurrence,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let mut expected_revision = Some(job.revision);
        let next_run_at = job.next_run_at;
        let mut definition = job.definition;
        ensure_definition_workspace(app, &definition)?;
        if definition.retry_at.is_some_and(|retry_at| retry_at > now) {
            return Ok(());
        }
        let scheduled_at = occurrence.scheduled_at;
        let lease_until = LeaseConfig::default().lease_until(now);
        let claim = database.claim_occurrence(&occurrence.id, &self.owner_id, now, lease_until)?;
        let claimed = match claim {
            ClaimResult::Claimed(value) => value,
            ClaimResult::AlreadyOwned => return Ok(()),
            ClaimResult::Busy => return Ok(()),
            ClaimResult::NotFound => return Ok(()),
        };
        // next_run_at 已在 materialize_due_occurrence 中推进到下一个触发点。
        // 触发瞬间就通知前端，让「下次执行时间」立即更新——它与本次 occurrence 是否执行完、
        // 执行多久完全解耦；否则 UI 在 execute → persist_active_projection 之间（可能长达整个 Run）
        // 一直显示旧的 next_run_at。
        emit_scheduled_task_updated(&self.app_handle, &definition, next_run_at);
        let mut handoff = self.claim_to_handoff_guard(
            &claimed,
            &definition,
            database,
            &app.paths.repo_root,
            expected_revision,
        )?;

        if let Some((status, error)) = recovery_outcome_for_accepted_occurrence(app, &claimed)? {
            handoff.stop().await;
            if !database.finish_occurrence(&claimed.id, &self.owner_id, status, None, error)? {
                anyhow::bail!("failed to finish recovered accepted scheduled occurrence");
            }
            handoff.disarm();
            let recovered = database
                .get_occurrence(&claimed.id)?
                .ok_or_else(|| anyhow::anyhow!("recovered scheduled occurrence disappeared"))?;
            definition.last_trigger_at = Some(recovered.scheduled_at);
            definition.last_trigger_status = Some(recovered.status.to_string());
            definition.last_error = recovered.error_code.map(|code| code.to_string());
            if recovered.task_id.is_some() {
                definition.task_id = recovered.task_id.clone();
            }
            definition.updated_at = now;
            if let Err(error) = self.persist_active_projection(
                &claimed.id,
                database,
                &mut definition,
                &mut expected_revision,
            ) {
                warn!(%error, occurrence_id = %claimed.id, "recovered accepted occurrence projection failed");
            }
            return Ok(());
        }

        let active_execution = active_execution_for_task(app, definition.task_id.as_deref())?;
        if active_execution.is_active() {
            let (status, error) = match decide_queue(
                definition.overlap_policy,
                active_execution,
                claimed.attempt.min(u8::MAX as u32) as u8,
                now,
            ) {
                QueueDecision::RetryAt(retry_at) => {
                    definition.retry_count = claimed.attempt.min(u8::MAX as u32) as u8;
                    definition.retry_at = Some(retry_at);
                    (
                        OccurrenceStatus::Retrying,
                        Some(ScheduledError::new(ScheduledErrorCode::QueueBusy)),
                    )
                }
                QueueDecision::Skipped => (
                    OccurrenceStatus::Skipped,
                    Some(ScheduledError::new(ScheduledErrorCode::QueueBusy)),
                ),
                QueueDecision::StartNow => {
                    anyhow::bail!(
                        "active scheduled execution must not receive a start-now decision"
                    )
                }
            };
            handoff.stop().await;
            let finished =
                database.finish_occurrence(&claimed.id, &self.owner_id, status, None, error)?;
            if !finished {
                anyhow::bail!("failed to finish busy scheduled occurrence");
            }
            if status == OccurrenceStatus::Retrying {
                definition.last_trigger_status = Some("retrying".to_string());
                definition.last_error = Some(ScheduledErrorCode::QueueBusy.to_string());
                definition.updated_at = now;
            } else {
                advance_definition_after_point(&mut definition, scheduled_at, "skipped", now);
            }
            self.persist_active_projection(
                &claimed.id,
                database,
                &mut definition,
                &mut expected_revision,
            )?;
            handoff.disarm();
            return Ok(());
        }

        advance_definition_after_point(&mut definition, scheduled_at, "running", now);
        if matches!(definition.schedule.kind, ScheduleKind::At { .. }) {
            definition.enabled = false;
        }
        self.persist_active_projection(
            &claimed.id,
            database,
            &mut definition,
            &mut expected_revision,
        )?;
        if let Some(error) = scheduled_agent_unattended_error(app, &definition) {
            self.finish_immediate_failure(
                database,
                &mut definition,
                &claimed,
                &mut expected_revision,
                &mut handoff,
                error,
            )
            .await?;
            return Ok(());
        }
        let execution = match execute_definition(
            &self.app_handle,
            app,
            database,
            &self.owner_id,
            &mut definition,
            &claimed,
            &claimed.trigger_kind.to_string(),
        ) {
            Ok(execution) => execution,
            Err(error) => {
                self.finish_execution_failure(database, &claimed, &mut handoff, &error)
                    .await?;
                return Err(error);
            }
        };
        handoff.handoff();
        let _ = execution.immediate_links;
        if let Err(error) = self.persist_active_projection(
            &claimed.id,
            database,
            &mut definition,
            &mut expected_revision,
        ) {
            warn!(%error, occurrence_id = %claimed.id, "accepted scheduled occurrence projection failed");
        }
        Ok(())
    }

    async fn reconcile_running_occurrences(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        job: &ScheduledJobRecord,
    ) -> anyhow::Result<()> {
        let occurrences = database.list_running_occurrences_for_job(&job.definition.id())?;
        for occurrence in occurrences {
            let Some((status, error)) = reconcile_running_occurrence_outcome(app, &occurrence)?
            else {
                continue; // Task/Run 仍 active 或尚未 execute，保留 running。
            };
            let Some(finished) = finish_reconciled_occurrence(
                database,
                &self.active,
                &occurrence.id,
                &self.owner_id,
                status,
                error.clone(),
            )
            .await?
            else {
                // lease 已被别人接手或已终态：跳过，交由其 owner 处理。
                continue;
            };
            apply_terminal_occurrence_side_effects(
                &self.app_handle,
                ActiveOccurrenceMetadata {
                    database: database.clone(),
                    workspace_path: app.paths.repo_root.clone(),
                    owner_id: self.owner_id.clone(),
                    project_id: job.definition.project_id.clone(),
                    scheduled_task_id: job.definition.id.clone(),
                    expected_revision: Some(job.revision),
                },
                &finished,
            );
            warn!(
                occurrence_id = %occurrence.id,
                job_id = %job.definition.id(),
                status = %status,
                error = ?error.map(|e| e.code.to_string()),
                "scheduled running occurrence reconciled to terminal (lifecycle event likely lost)"
            );
        }
        Ok(())
    }

    async fn finish_immediate_failure(
        &self,
        database: &ScheduledTaskDatabase,
        definition: &mut ScheduledTaskDefinition,
        occurrence: &ScheduledOccurrence,
        expected_revision: &mut Option<i64>,
        handoff: &mut ClaimToHandoffGuard,
        error: ScheduledError,
    ) -> anyhow::Result<()> {
        let error_code = error.code;
        handoff.stop().await;
        let finished = database.finish_occurrence(
            &occurrence.id,
            &self.owner_id,
            OccurrenceStatus::Failed,
            None,
            Some(error),
        )?;
        if !finished {
            anyhow::bail!("failed to finish scheduled occurrence before execution handoff");
        }
        handoff.disarm();
        definition.last_trigger_status = Some("failed".to_string());
        definition.last_error = Some(error_code.to_string());
        definition.updated_at = Utc::now();
        self.persist_active_projection(&occurrence.id, database, definition, expected_revision)?;
        Ok(())
    }

    async fn finish_execution_failure(
        &self,
        database: &ScheduledTaskDatabase,
        occurrence: &ScheduledOccurrence,
        handoff: &mut ClaimToHandoffGuard,
        execution_error: &anyhow::Error,
    ) -> anyhow::Result<()> {
        handoff.stop().await;
        let finished = database.finish_occurrence(
            &occurrence.id,
            &self.owner_id,
            OccurrenceStatus::Failed,
            None,
            Some(ScheduledError::with_params(
                ScheduledErrorCode::ExecutionFailed,
                serde_json::json!({ "reason": execution_error.to_string() }),
            )),
        )?;
        if !finished {
            anyhow::bail!("failed to finish scheduled occurrence after execution handoff failure");
        }
        handoff.disarm();
        Ok(())
    }

    fn claim_to_handoff_guard(
        &self,
        occurrence: &ScheduledOccurrence,
        definition: &ScheduledTaskDefinition,
        database: &ScheduledTaskDatabase,
        workspace_path: &Utf8Path,
        expected_revision: Option<i64>,
    ) -> anyhow::Result<ClaimToHandoffGuard> {
        ClaimToHandoffGuard::new_with_pending(
            self.active.clone(),
            self.pending_guard_joins.clone(),
            occurrence.id.clone(),
            ActiveOccurrenceMetadata {
                database: database.clone(),
                workspace_path: workspace_path.to_path_buf(),
                owner_id: self.owner_id.clone(),
                project_id: definition.project_id.clone(),
                scheduled_task_id: definition.id.clone(),
                expected_revision,
            },
        )
    }

    fn persist_active_projection(
        &self,
        occurrence_id: &str,
        database: &ScheduledTaskDatabase,
        definition: &mut ScheduledTaskDefinition,
        expected_revision: &mut Option<i64>,
    ) -> anyhow::Result<()> {
        let Some(revision) = *expected_revision else {
            return Ok(());
        };
        let updated = persist_runtime_projection(database, definition, revision, |record| {
            emit_scheduled_task_updated(&self.app_handle, &record.definition, record.next_run_at);
        })?;
        match updated {
            Some(updated) => {
                *definition = updated.definition;
                *expected_revision = Some(updated.revision);
            }
            None => *expected_revision = None,
        }
        if let Ok(mut active) = self.active.lock()
            && let Some(active) = active.get_mut(occurrence_id)
        {
            active.metadata.expected_revision = *expected_revision;
        }
        Ok(())
    }

    async fn shutdown_active_leases(&self, now: DateTime<Utc>) -> anyhow::Result<()> {
        shutdown_active_occurrences(&self.active, &self.pending_guard_joins, now).await
    }

    async fn resume_attention(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        attempt_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let Some(attention) =
            database.find_attention_occurrence_by_links(task_id, run_id, round_id, attempt_id)?
        else {
            return Ok(None);
        };
        let Some(mut job) = database.get_job_definition_by_id(&attention.job_id)? else {
            anyhow::bail!("scheduled attention definition was not found");
        };
        let claim = database.resume_attention_occurrence(
            &attention.id,
            &self.owner_id,
            Utc::now(),
            LeaseConfig::default().lease_until(Utc::now()),
        )?;
        let claimed = match claim {
            ClaimResult::Claimed(value) => value,
            ClaimResult::AlreadyOwned => return Ok(Some(attention.id)),
            ClaimResult::Busy => {
                anyhow::bail!("scheduled attention occurrence is owned by another runtime")
            }
            ClaimResult::NotFound => return Ok(None),
        };
        let guard = self.claim_to_handoff_guard(
            &claimed,
            &job.definition,
            database,
            &app.paths.repo_root,
            Some(job.revision),
        )?;
        project_resumed_attention(&mut job.definition, &claimed, Utc::now());
        let mut expected_revision = Some(job.revision);
        self.persist_active_projection(
            &claimed.id,
            database,
            &mut job.definition,
            &mut expected_revision,
        )?;
        guard.handoff();
        emit_scheduled_occurrence_updated(
            &self.app_handle,
            &job.definition.project_id,
            job.definition.id(),
            &claimed,
        );
        Ok(Some(claimed.id))
    }

    fn handle_lifecycle_event(&self, event: RuntimeLifecycleEvent) {
        let Some(occurrence_id) = scheduled_occurrence_id(&event) else {
            // 终止事件未携带 occurrence_id（orchestrator 硬编码 None，依赖 App 注入）。
            // 若这是某条 scheduled run 的完成事件，对应 occurrence 会因收不到事件卡 running，
            // 只能靠主动对账收尾。这里记录便于定位。
            if event_finishes_occurrence(&event) {
                warn!(
                    event = ?event,
                    "scheduled lifecycle terminal event has no occurrence id; occurrence may stick in running"
                );
            }
            return;
        };
        if !event_finishes_occurrence(&event) {
            return;
        }
        let Some(entry) = take_active_occurrence(&self.active, &occurrence_id) else {
            warn!(
                %occurrence_id,
                "scheduled lifecycle terminal event arrived but no active occurrence registered (lease lost or already finished)"
            );
            return;
        };
        let app_handle = self.app_handle.clone();
        let pending_guard_joins = self.pending_guard_joins.clone();
        let join = tauri::async_runtime::spawn(async move {
            entry.guard.stop().await;
            finish_lifecycle_occurrence(app_handle, occurrence_id, entry.metadata, event);
        });
        if let Ok(mut pending) = pending_guard_joins.lock() {
            pending.push(join);
        }
    }
}

fn finish_lifecycle_occurrence(
    app_handle: AppHandle,
    occurrence_id: String,
    active: ActiveOccurrenceMetadata,
    event: RuntimeLifecycleEvent,
) {
    match finish_occurrence_for_event(&active.database, &occurrence_id, &active.owner_id, &event) {
        Ok(Some(occurrence)) => {
            apply_terminal_occurrence_side_effects(&app_handle, active, &occurrence);
        }
        Ok(None) => {}
        Err(error) => warn!(%error, %occurrence_id, "failed to finish scheduled occurrence"),
    }
}

fn project_resumed_attention(
    definition: &mut ScheduledTaskDefinition,
    occurrence: &ScheduledOccurrence,
    now: DateTime<Utc>,
) {
    definition.last_trigger_at = Some(occurrence.scheduled_at);
    definition.last_trigger_status = Some(OccurrenceStatus::Running.to_string());
    definition.last_error = None;
    definition.retry_count = 0;
    definition.retry_at = None;
    definition.updated_at = now;
}

fn apply_terminal_occurrence_side_effects(
    app_handle: &AppHandle,
    active: ActiveOccurrenceMetadata,
    occurrence: &ScheduledOccurrence,
) {
    if let Some(expected_revision) = active.expected_revision {
        match active
            .database
            .get_job_definition(&active.project_id, &active.scheduled_task_id)
        {
            Ok(Some(current)) => {
                let mut definition = current.definition;
                definition.last_trigger_at = Some(occurrence.scheduled_at);
                definition.last_trigger_status = Some(occurrence.status.to_string());
                definition.last_error = occurrence.error_code.map(|value| value.to_string());
                if occurrence.task_id.is_some() {
                    definition.task_id = occurrence.task_id.clone();
                }
                definition.updated_at = Utc::now();
                if let Err(error) = persist_runtime_projection(
                    &active.database,
                    &definition,
                    expected_revision,
                    |record| {
                        emit_scheduled_task_updated(
                            app_handle,
                            &record.definition,
                            record.next_run_at,
                        )
                    },
                ) {
                    warn!(%error, occurrence_id = %occurrence.id, "failed to persist scheduled terminal projection");
                }
            }
            Ok(None) => {}
            Err(error) => {
                warn!(%error, occurrence_id = %occurrence.id, "failed to load scheduled terminal projection")
            }
        }
    }
    emit_scheduled_occurrence_notification(app_handle, &active.project_id, occurrence);
    emit_scheduled_occurrence_updated(
        app_handle,
        &active.project_id,
        &active.scheduled_task_id,
        occurrence,
    );
    if let Ok(coordinator) = app_handle.state::<DesktopState>().scheduler_coordinator() {
        let _ = coordinator.send(SchedulerCommand::CleanupWorkspace {
            workspace_path: active.workspace_path,
        });
    }
}

enum CoordinatorEvent {
    Command(Option<SchedulerCommand>),
    Deadline(Option<ScheduledJobKey>),
    ClockCheck,
}

struct ClockDriftDetector {
    wall_at_sample: DateTime<Utc>,
    monotonic_at_sample: tokio::time::Instant,
}

impl ClockDriftDetector {
    fn new(wall_now: DateTime<Utc>) -> Self {
        Self {
            wall_at_sample: wall_now,
            monotonic_at_sample: tokio::time::Instant::now(),
        }
    }

    fn observe(&mut self, wall_now: DateTime<Utc>) -> bool {
        let monotonic_now = tokio::time::Instant::now();
        let monotonic_elapsed = monotonic_now.duration_since(self.monotonic_at_sample);
        let wall_elapsed = wall_now.signed_duration_since(self.wall_at_sample);
        let drifted = match wall_elapsed.to_std() {
            Ok(wall_elapsed) => monotonic_elapsed.abs_diff(wall_elapsed) > CLOCK_DRIFT_TOLERANCE,
            Err(_) => true,
        };
        if drifted {
            self.wall_at_sample = wall_now;
            self.monotonic_at_sample = monotonic_now;
        }
        drifted
    }
}

trait CoordinatorRuntimeDriver: Send + Sync + 'static {
    fn app_for_workspace(&self, workspace_path: &Utf8Path) -> anyhow::Result<App>;

    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn scheduled_occurrence_retention_days(&self) -> u16 {
        DEFAULT_OCCURRENCE_RETENTION_DAYS
    }

    fn reconcile_power_state(&self, _enabled_job_count: usize, _app_is_running: bool) {}

    fn notify_occurrence(&self, _project_id: &str, _occurrence: &ScheduledOccurrence) {}

    fn notify_missed(&self, _project_id: &str, _scheduled_task_id: &str, _missed_count: u32) {}

    async fn shutdown_active_leases(&self, _now: DateTime<Utc>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn process_occurrence(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        job: ScheduledJobRecord,
        occurrence: ScheduledOccurrence,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()>;

    /// 主动状态对账：核对指定 job 的 running occurrence 与底层 Task/Run 真实状态，
    /// 把 lifecycle 终止事件已丢失的 occurrence 收尾为终态。默认不做任何事（测试 mock 可不实现）。
    async fn reconcile_running_occurrences(
        &self,
        _database: &ScheduledTaskDatabase,
        _app: &App,
        _job: &ScheduledJobRecord,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run_manual(
        &self,
        app: &App,
        job: &ScheduledJobRecord,
    ) -> anyhow::Result<ManualRunResult>;

    async fn resume_attention(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        attempt_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let _ = (database, app, task_id, run_id, round_id, attempt_id);
        Ok(None)
    }
}

impl CoordinatorRuntimeDriver for ScheduledRuntime {
    fn app_for_workspace(&self, workspace_path: &Utf8Path) -> anyhow::Result<App> {
        let state = self.app_handle.state::<DesktopState>();
        let context = state.context()?;
        runtime_app_for_workspace(&state, &context, workspace_path.as_str())
    }

    fn scheduled_occurrence_retention_days(&self) -> u16 {
        self.app_handle
            .state::<DesktopState>()
            .context()
            .map(|context| context.config.scheduled_occurrence_retention_days)
            .unwrap_or(DEFAULT_OCCURRENCE_RETENTION_DAYS)
    }

    async fn reconcile_running_occurrences(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        job: &ScheduledJobRecord,
    ) -> anyhow::Result<()> {
        ScheduledRuntime::reconcile_running_occurrences(self, database, app, job).await
    }

    fn reconcile_power_state(&self, enabled_job_count: usize, app_is_running: bool) {
        match self
            .app_handle
            .state::<DesktopState>()
            .reconcile_scheduled_power(enabled_job_count, app_is_running)
        {
            Ok(status) => {
                if let Some(error) = status.error {
                    warn!(
                        code = %error.code,
                        params = ?error.params,
                        enabled_job_count,
                        "scheduled power inhibitor acquisition failed"
                    );
                }
            }
            Err(error) => warn!(%error, "scheduled power state reconciliation failed"),
        }
    }

    fn notify_occurrence(&self, project_id: &str, occurrence: &ScheduledOccurrence) {
        emit_scheduled_occurrence_notification(&self.app_handle, project_id, occurrence);
    }

    fn notify_missed(&self, project_id: &str, scheduled_task_id: &str, missed_count: u32) {
        emit_scheduled_missed_notification(
            &self.app_handle,
            project_id,
            scheduled_task_id,
            missed_count,
        );
    }

    async fn shutdown_active_leases(&self, now: DateTime<Utc>) -> anyhow::Result<()> {
        ScheduledRuntime::shutdown_active_leases(self, now).await
    }

    async fn process_occurrence(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        job: ScheduledJobRecord,
        occurrence: ScheduledOccurrence,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        ScheduledRuntime::process_occurrence(self, database, app, job, occurrence, now).await
    }

    async fn run_manual(
        &self,
        app: &App,
        job: &ScheduledJobRecord,
    ) -> anyhow::Result<ManualRunResult> {
        ScheduledRuntime::run_manual(self, app, job).await
    }

    async fn resume_attention(
        &self,
        database: &ScheduledTaskDatabase,
        app: &App,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        attempt_id: &str,
    ) -> anyhow::Result<Option<String>> {
        ScheduledRuntime::resume_attention(
            self, database, app, task_id, run_id, round_id, attempt_id,
        )
        .await
    }
}

impl<R: CoordinatorRuntimeDriver> SchedulerCoordinator<R> {
    fn new_with_sender(
        runtime: Arc<R>,
        sender: mpsc::UnboundedSender<SchedulerCommand>,
        receiver: mpsc::UnboundedReceiver<SchedulerCommand>,
    ) -> Self {
        Self {
            runtime,
            sender,
            receiver,
            deadlines: DeadlineRegistry::new(),
            registered_deadlines: HashMap::new(),
            workspaces: HashMap::new(),
            workspace_registration_retries: HashSet::new(),
            timer_drift_reconcile_pending: false,
        }
    }

    async fn run(mut self) -> Self {
        let mut clock_check = tokio::time::interval(CLOCK_DRIFT_CHECK_INTERVAL);
        clock_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        clock_check.tick().await;
        let mut drift_detector = ClockDriftDetector::new(self.runtime.now());
        loop {
            let event = if self.deadlines.is_empty() {
                tokio::select! {
                    command = self.receiver.recv() => CoordinatorEvent::Command(command),
                    _ = clock_check.tick() => CoordinatorEvent::ClockCheck,
                }
            } else {
                tokio::select! {
                    command = self.receiver.recv() => CoordinatorEvent::Command(command),
                    deadline = self.deadlines.next_expired() => CoordinatorEvent::Deadline(deadline),
                    _ = clock_check.tick() => CoordinatorEvent::ClockCheck,
                }
            };
            match event {
                CoordinatorEvent::Command(Some(SchedulerCommand::Shutdown { ack })) => {
                    let result = self
                        .runtime
                        .shutdown_active_leases(self.runtime.now())
                        .await
                        .map_err(|error| {
                            ScheduledServiceError::new(
                                ScheduledErrorCode::CoordinatorUnavailable,
                                serde_json::json!({
                                    "operation": "release-active-leases",
                                    "reason": error.to_string(),
                                }),
                            )
                        });
                    if let Err(error) = &result {
                        error!(%error, "failed to release scheduled occurrence leases on shutdown");
                    }
                    self.reconcile_power_state(false);
                    let _ = ack.send(result);
                    break;
                }
                CoordinatorEvent::Command(None) => {
                    if let Err(error) = self
                        .runtime
                        .shutdown_active_leases(self.runtime.now())
                        .await
                    {
                        error!(%error, "failed to release scheduled occurrence leases on shutdown");
                    }
                    self.reconcile_power_state(false);
                    break;
                }
                CoordinatorEvent::Command(Some(command)) => self.handle_command(command).await,
                CoordinatorEvent::Deadline(Some(key)) => {
                    if let Err(error) = self.handle_deadline(key).await {
                        error!(%error, "scheduled deadline handling failed");
                    }
                }
                CoordinatorEvent::Deadline(None) => {}
                CoordinatorEvent::ClockCheck => {
                    let now = self.runtime.now();
                    if drift_detector.observe(now) {
                        self.timer_drift_reconcile_pending = true;
                    }
                    if self.timer_drift_reconcile_pending {
                        match self.reconcile_all(ReconcileReason::TimerDrift).await {
                            Ok(()) => self.timer_drift_reconcile_pending = false,
                            Err(error) => {
                                error!(%error, "scheduled timer drift reconcile failed");
                            }
                        }
                    }
                }
            }
        }
        info!("scheduled task scheduler stopped");
        self
    }

    async fn handle_command(&mut self, command: SchedulerCommand) {
        let refresh_power = matches!(
            &command,
            SchedulerCommand::RegisterWorkspace { .. }
                | SchedulerCommand::RetryRegisterWorkspace { .. }
                | SchedulerCommand::UnregisterWorkspace { .. }
                | SchedulerCommand::JobCreated { .. }
                | SchedulerCommand::JobUpdated { .. }
                | SchedulerCommand::JobEnabled { .. }
                | SchedulerCommand::JobDisabled { .. }
                | SchedulerCommand::JobDeleted { .. }
                | SchedulerCommand::SettingsChanged
                | SchedulerCommand::Reconcile { .. }
        );
        let result = match command {
            SchedulerCommand::RegisterWorkspace { workspace_path } => {
                self.register_workspace_with_retry(workspace_path, ReconcileReason::Startup)
                    .await
            }
            SchedulerCommand::RetryRegisterWorkspace { workspace_path } => {
                if !self.workspace_registration_retries.remove(&workspace_path) {
                    return;
                }
                self.register_workspace_with_retry(workspace_path, ReconcileReason::Startup)
                    .await
            }
            SchedulerCommand::UnregisterWorkspace { workspace_path } => {
                self.unregister_workspace(&workspace_path);
                Ok(())
            }
            SchedulerCommand::JobCreated { key }
            | SchedulerCommand::JobUpdated { key }
            | SchedulerCommand::JobEnabled { key } => self.refresh_changed_job(key).await,
            SchedulerCommand::JobDisabled { key } | SchedulerCommand::JobDeleted { key } => {
                self.refresh_changed_job(key).await
            }
            SchedulerCommand::RunNow { key, reply } => {
                let result = self.run_now(key).await;
                let _ = reply.send(result);
                Ok(())
            }
            SchedulerCommand::ResumeAttention {
                workspace_path,
                task_id,
                run_id,
                round_id,
                attempt_id,
                reply,
            } => {
                let result = self
                    .resume_attention(workspace_path, &task_id, &run_id, &round_id, &attempt_id)
                    .await;
                let _ = reply.send(result);
                Ok(())
            }
            SchedulerCommand::SettingsChanged => {
                self.reconcile_all(ReconcileReason::Explicit).await
            }
            SchedulerCommand::CleanupWorkspace { workspace_path } => {
                self.run_retention_for_workspace(&workspace_path).await
            }
            SchedulerCommand::Reconcile { reason } => self.reconcile_all(reason).await,
            SchedulerCommand::Shutdown { .. } => Ok(()),
        };
        if let Err(error) = result {
            error!(%error, "scheduled coordinator command failed");
        }
        if refresh_power {
            self.reconcile_power_state(true);
        }
    }

    async fn register_workspace_with_retry(
        &mut self,
        workspace_path: Utf8PathBuf,
        reason: ReconcileReason,
    ) -> anyhow::Result<()> {
        match self
            .register_workspace(workspace_path.clone(), reason)
            .await
        {
            Ok(()) => {
                self.workspace_registration_retries.remove(&workspace_path);
                Ok(())
            }
            Err(error) => {
                self.schedule_workspace_registration_retry(workspace_path);
                Err(error)
            }
        }
    }

    fn schedule_workspace_registration_retry(&mut self, workspace_path: Utf8PathBuf) {
        if !self
            .workspace_registration_retries
            .insert(workspace_path.clone())
        {
            return;
        }
        let sender = self.sender.clone();
        tokio::spawn(async move {
            tokio::time::sleep(WORKSPACE_REGISTRATION_RETRY_DELAY).await;
            let _ = sender.send(SchedulerCommand::RetryRegisterWorkspace { workspace_path });
        });
    }

    async fn register_workspace(
        &mut self,
        workspace_path: Utf8PathBuf,
        reason: ReconcileReason,
    ) -> anyhow::Result<()> {
        let app = self.runtime.app_for_workspace(&workspace_path)?;
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
        let now = self.runtime.now();
        migrate_legacy_scheduler_database(&app.paths, &database, now)?;
        let legacy_store = ScheduledTaskStore::new(app.paths.clone());
        database.import_legacy_store(&legacy_store)?;
        database.recover_expired(now)?;
        let candidate = WorkspaceRegistration {
            app: Arc::new(app),
            database,
        };
        let previous_deadlines = self.workspace_deadline_snapshot(&workspace_path);
        if let Err(error) = self
            .reconcile_workspace_registration(&workspace_path, &candidate, reason)
            .await
        {
            self.restore_workspace_deadlines(&workspace_path, previous_deadlines);
            return Err(error);
        }
        if let Err(error) = self.run_retention_for_registration(&candidate).await {
            warn!(
                code = %ScheduledErrorCode::StorageFailed,
                params = ?serde_json::json!({ "reason": error.to_string() }),
                workspace_path = %workspace_path,
                "scheduled occurrence retention cleanup failed"
            );
        }
        self.workspaces.insert(workspace_path, candidate);
        Ok(())
    }

    fn unregister_workspace(&mut self, workspace_path: &Utf8Path) {
        self.workspace_registration_retries.remove(workspace_path);
        self.workspaces.remove(workspace_path);
        let keys = self
            .registered_deadlines
            .keys()
            .filter(|key| key.workspace_path == workspace_path)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.cancel_job(&key);
        }
    }

    async fn refresh_changed_job(&mut self, key: ScheduledJobKey) -> anyhow::Result<()> {
        if !self.workspaces.contains_key(&key.workspace_path) {
            self.register_workspace_with_retry(
                key.workspace_path.clone(),
                ReconcileReason::Explicit,
            )
            .await?;
        }
        self.refresh_job(&key, self.runtime.now())
    }

    async fn reconcile_all(&mut self, reason: ReconcileReason) -> anyhow::Result<()> {
        let workspaces = self.workspaces.keys().cloned().collect::<Vec<_>>();
        for workspace_path in workspaces {
            self.reconcile_workspace(&workspace_path, reason).await?;
        }
        Ok(())
    }

    fn reconcile_power_state(&self, app_is_running: bool) {
        let enabled_job_count = self
            .workspaces
            .values()
            .try_fold(0usize, |count, registration| {
                registration
                    .database
                    .enabled_job_count()
                    .map(|workspace_count| count.saturating_add(workspace_count))
            });
        match enabled_job_count {
            Ok(enabled_job_count) => self
                .runtime
                .reconcile_power_state(enabled_job_count, app_is_running),
            Err(error) => warn!(%error, "failed to count enabled scheduled jobs for power state"),
        }
    }

    async fn run_retention_for_workspace(&self, workspace_path: &Utf8Path) -> anyhow::Result<()> {
        let registration = self
            .workspaces
            .get(workspace_path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("scheduled workspace is not registered"))?;
        self.run_retention_for_registration(&registration).await
    }

    async fn run_retention_for_registration(
        &self,
        registration: &WorkspaceRegistration,
    ) -> anyhow::Result<()> {
        let protected_run_ids = active_run_ids(&registration.app)?;
        let cutoff = self.runtime.now()
            - Duration::days(i64::from(
                self.runtime.scheduled_occurrence_retention_days(),
            ));
        loop {
            let result = registration.database.cleanup_terminal_occurrences(
                cutoff,
                RETENTION_DELETE_BATCH_SIZE,
                &protected_run_ids,
            )?;
            if !result.has_more {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    }

    async fn run_retention_best_effort(&self, registration: &WorkspaceRegistration) {
        if let Err(error) = self.run_retention_for_registration(registration).await {
            warn!(
                code = %ScheduledErrorCode::StorageFailed,
                params = ?serde_json::json!({ "reason": error.to_string() }),
                workspace_path = %registration.app.paths.repo_root,
                "scheduled occurrence retention cleanup failed"
            );
        }
    }

    fn notify_occurrence_best_effort(
        &self,
        registration: &WorkspaceRegistration,
        occurrence_id: &str,
    ) {
        match registration.database.get_occurrence(occurrence_id) {
            Ok(Some(occurrence)) => self
                .runtime
                .notify_occurrence(&registration.app.paths.project_id, &occurrence),
            Ok(None) => {}
            Err(error) => warn!(
                %error,
                occurrence_id,
                "failed to reload occurrence for scheduled notification"
            ),
        }
    }

    async fn reconcile_workspace(
        &mut self,
        workspace_path: &Utf8Path,
        reason: ReconcileReason,
    ) -> anyhow::Result<()> {
        let registration = self
            .workspaces
            .get(workspace_path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("scheduled workspace is not registered"))?;
        let previous_deadlines = self.workspace_deadline_snapshot(workspace_path);
        if let Err(error) = self
            .reconcile_workspace_registration(workspace_path, &registration, reason)
            .await
        {
            self.restore_workspace_deadlines(workspace_path, previous_deadlines);
            return Err(error);
        }
        Ok(())
    }

    async fn reconcile_workspace_registration(
        &mut self,
        workspace_path: &Utf8Path,
        registration: &WorkspaceRegistration,
        _reason: ReconcileReason,
    ) -> anyhow::Result<()> {
        let now = self.runtime.now();
        registration.database.recover_expired(now)?;
        let recoverable = registration
            .database
            .list_recoverable_jobs_for_project(&registration.app.paths.project_id)?;
        let persisted_keys = recoverable
            .iter()
            .map(|record| {
                ScheduledJobKey::new(
                    workspace_path.to_path_buf(),
                    record.job.definition.project_id.clone(),
                    record.job.definition.id().to_string(),
                )
            })
            .collect::<HashSet<_>>();
        let removed = self
            .registered_deadlines
            .keys()
            .filter(|key| key.workspace_path == workspace_path && !persisted_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in removed {
            self.cancel_job(&key);
        }

        for key in persisted_keys {
            let Some(recovery) = registration
                .database
                .get_recoverable_job_for_project(&key.project_id, &key.job_id)?
            else {
                self.cancel_job(&key);
                continue;
            };
            if recovery.has_runnable_occurrence
                && let Some(occurrence) =
                    next_runnable_occurrence(&registration.database, &key.job_id)?
            {
                if recovery
                    .job
                    .definition
                    .retry_at
                    .is_none_or(|retry_at| retry_at <= now)
                {
                    let occurrence_id = occurrence.id.clone();
                    self.runtime
                        .process_occurrence(
                            &registration.database,
                            &registration.app,
                            recovery.job,
                            occurrence,
                            now,
                        )
                        .await?;
                    self.notify_occurrence_best_effort(registration, &occurrence_id);
                    self.run_retention_best_effort(registration).await;
                    self.refresh_job_from_registration(&key, registration, self.runtime.now())?;
                } else {
                    self.register_record(key, recovery, now)?;
                }
                continue;
            }
            if recovery.job.definition.enabled {
                let missed = reconcile_missed_deadlines(&registration.database, &key, now)?;
                self.runtime
                    .notify_missed(&key.project_id, &key.job_id, missed.missed_count);
            }
            self.refresh_job_from_registration(&key, registration, self.runtime.now())?;
        }
        Ok(())
    }

    async fn handle_deadline(&mut self, key: ScheduledJobKey) -> anyhow::Result<()> {
        let Some(registered) = self.registered_deadlines.remove(&key) else {
            return self.refresh_job(&key, self.runtime.now());
        };
        match self.handle_registered_deadline(&key, registered).await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.rearm_failed_deadline(key, registered);
                Err(error)
            }
        }
    }

    async fn handle_registered_deadline(
        &mut self,
        key: &ScheduledJobKey,
        registered: RegisteredDeadline,
    ) -> anyhow::Result<()> {
        let registration = self
            .workspaces
            .get(&key.workspace_path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("scheduled workspace is not registered"))?;
        let now = self.runtime.now();
        registration.database.recover_expired(now)?;
        let Some(recovery) = registration
            .database
            .get_recoverable_job_for_project(&key.project_id, &key.job_id)?
        else {
            return Ok(());
        };
        // 主动状态对账：在每次触发点核对当前 job 的 running occurrence。
        // recover_expired 只回收 lease 过期的；lease 仍新鲜但 lifecycle 终止事件已丢失的
        // occurrence 会永久卡 running。这里以 Task/Run 真实状态为基准收尾，真在跑的保留。
        if let Err(error) = self
            .runtime
            .reconcile_running_occurrences(&registration.database, &registration.app, &recovery.job)
            .await
        {
            warn!(%error, job_id = %key.job_id, "scheduled running occurrence reconcile failed");
        }
        if !registered.matches(&recovery.job) {
            if recovery.has_runnable_occurrence {
                return self.register_record(key.clone(), recovery, now);
            }
            return self.register_record_not_before(
                key.clone(),
                recovery,
                now,
                Some(stale_deadline_retry_at(registered.wake_at, now)),
            );
        }
        if recovery.has_runnable_occurrence
            && let Some(occurrence) = next_runnable_occurrence(&registration.database, &key.job_id)?
        {
            if recovery
                .job
                .definition
                .retry_at
                .is_some_and(|retry_at| retry_at > now)
            {
                return self.register_record(key.clone(), recovery, now);
            }
            let occurrence_id = occurrence.id.clone();
            self.runtime
                .process_occurrence(
                    &registration.database,
                    &registration.app,
                    recovery.job,
                    occurrence,
                    now,
                )
                .await?;
            self.notify_occurrence_best_effort(&registration, &occurrence_id);
            self.run_retention_best_effort(&registration).await;
            return self.refresh_job(&key, self.runtime.now());
        }
        if registered
            .scheduled_deadline
            .is_some_and(|deadline| deadline < now - LATE_FIRE_GRACE)
        {
            let missed = reconcile_missed_deadlines(&registration.database, &key, now)?;
            self.runtime
                .notify_missed(&key.project_id, &key.job_id, missed.missed_count);
            return self.refresh_job(&key, self.runtime.now());
        }
        if let DueMaterialization::Ready { job, occurrence } = materialize_registered_deadline(
            &registration.database,
            &key.project_id,
            &key.job_id,
            registered,
            now,
        )? {
            let occurrence_id = occurrence.id.clone();
            self.runtime
                .process_occurrence(
                    &registration.database,
                    &registration.app,
                    job,
                    occurrence,
                    now,
                )
                .await?;
            self.notify_occurrence_best_effort(&registration, &occurrence_id);
            self.run_retention_best_effort(&registration).await;
        }
        self.refresh_job(&key, self.runtime.now())
    }

    fn rearm_failed_deadline(&mut self, key: ScheduledJobKey, mut registered: RegisteredDeadline) {
        let now = self.runtime.now();
        let retry_delay = Duration::from_std(DEADLINE_FAILURE_RETRY_DELAY)
            .expect("deadline retry delay must fit chrono::Duration");
        registered.wake_at = now + retry_delay;
        self.deadlines
            .register_at(key.clone(), registered.wake_at, now);
        self.registered_deadlines.insert(key, registered);
    }

    async fn run_now(&mut self, key: ScheduledJobKey) -> ScheduledServiceResult<ManualRunResult> {
        if !self.workspaces.contains_key(&key.workspace_path) {
            self.register_workspace_with_retry(
                key.workspace_path.clone(),
                ReconcileReason::Explicit,
            )
            .await
            .map_err(|_| coordinator_error("register-run-now-workspace"))?;
        }
        let registration = self
            .workspaces
            .get(&key.workspace_path)
            .cloned()
            .ok_or_else(|| coordinator_error("resolve-run-now-workspace"))?;
        let record = registration
            .database
            .get_job_definition(&key.project_id, &key.job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| {
                ScheduledServiceError::new(
                    ScheduledErrorCode::NotFound,
                    serde_json::json!({ "scheduledTaskId": key.job_id }),
                )
            })?;
        let result = self
            .runtime
            .run_manual(&registration.app, &record)
            .await
            .map_err(|_| coordinator_error("run-now"));
        if let Ok(manual) = &result {
            self.runtime
                .notify_occurrence(&key.project_id, &manual.occurrence);
        }
        self.run_retention_best_effort(&registration).await;
        let _ = self.refresh_job(&key, self.runtime.now());
        result
    }

    async fn resume_attention(
        &mut self,
        workspace_path: Utf8PathBuf,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        attempt_id: &str,
    ) -> ScheduledServiceResult<Option<String>> {
        let Some(registration) = self.workspaces.get(&workspace_path).cloned() else {
            return Err(coordinator_error("resume-attention-workspace"));
        };
        self.runtime
            .resume_attention(
                &registration.database,
                &registration.app,
                task_id,
                run_id,
                round_id,
                attempt_id,
            )
            .await
            .map_err(|_| coordinator_error("resume-attention"))
    }

    fn refresh_job(&mut self, key: &ScheduledJobKey, now: DateTime<Utc>) -> anyhow::Result<()> {
        let Some(registration) = self.workspaces.get(&key.workspace_path).cloned() else {
            self.cancel_job(key);
            return Ok(());
        };
        self.refresh_job_from_registration(key, &registration, now)
    }

    fn refresh_job_from_registration(
        &mut self,
        key: &ScheduledJobKey,
        registration: &WorkspaceRegistration,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let Some(record) = registration
            .database
            .get_recoverable_job_for_project(&key.project_id, &key.job_id)?
        else {
            self.cancel_job(key);
            return Ok(());
        };
        self.register_record(key.clone(), record, now)
    }

    fn workspace_deadline_snapshot(
        &self,
        workspace_path: &Utf8Path,
    ) -> Vec<(ScheduledJobKey, RegisteredDeadline)> {
        self.registered_deadlines
            .iter()
            .filter(|(key, _)| key.workspace_path == workspace_path)
            .map(|(key, deadline)| (key.clone(), *deadline))
            .collect()
    }

    fn restore_workspace_deadlines(
        &mut self,
        workspace_path: &Utf8Path,
        deadlines: Vec<(ScheduledJobKey, RegisteredDeadline)>,
    ) {
        let current_keys = self
            .registered_deadlines
            .keys()
            .filter(|key| key.workspace_path == workspace_path)
            .cloned()
            .collect::<Vec<_>>();
        for key in current_keys {
            self.cancel_job(&key);
        }
        let now = self.runtime.now();
        for (key, deadline) in deadlines {
            self.deadlines
                .register_at(key.clone(), deadline.wake_at, now);
            self.registered_deadlines.insert(key, deadline);
        }
    }

    fn register_record(
        &mut self,
        key: ScheduledJobKey,
        record: RecoverableScheduledJob,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        self.register_record_not_before(key, record, now, None)
    }

    fn register_record_not_before(
        &mut self,
        key: ScheduledJobKey,
        record: RecoverableScheduledJob,
        now: DateTime<Utc>,
        not_before: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        let scheduled_deadline = record
            .job
            .definition
            .enabled
            .then_some(record.job.next_run_at)
            .flatten();
        let runnable_at = record
            .has_runnable_occurrence
            .then_some(record.job.definition.retry_at.unwrap_or(now));
        let wake_at = [
            runnable_at,
            record.earliest_running_lease_until,
            scheduled_deadline,
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(mut wake_at) = wake_at else {
            self.cancel_job(&key);
            return Ok(());
        };
        if let Some(not_before) = not_before {
            wake_at = wake_at.max(not_before);
        }
        let registered = RegisteredDeadline {
            revision: record.job.revision,
            scheduled_deadline,
            wake_at,
        };
        self.deadlines.register_at(key.clone(), wake_at, now);
        self.registered_deadlines.insert(key, registered);
        Ok(())
    }

    fn cancel_job(&mut self, key: &ScheduledJobKey) {
        self.deadlines.cancel(key);
        self.registered_deadlines.remove(key);
    }
}

fn stale_deadline_retry_at(wake_at: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    if wake_at > now {
        return wake_at;
    }
    now + Duration::from_std(DEADLINE_FAILURE_RETRY_DELAY)
        .expect("deadline retry delay must fit chrono::Duration")
}

fn coordinator_error(operation: &'static str) -> ScheduledServiceError {
    ScheduledServiceError::new(
        ScheduledErrorCode::CoordinatorUnavailable,
        serde_json::json!({ "operation": operation }),
    )
}

fn next_runnable_occurrence(
    database: &ScheduledTaskDatabase,
    job_id: &str,
) -> anyhow::Result<Option<ScheduledOccurrence>> {
    Ok(database.oldest_runnable_occurrence(job_id)?)
}

fn runtime_app_for_workspace(
    state: &DesktopState,
    context: &crate::state::DesktopContext,
    workspace: &str,
) -> anyhow::Result<App> {
    let base = state.app()?;
    Ok(base.with_repo_root(Utf8PathBuf::from(workspace), context.config.clone()))
}

fn migrate_legacy_scheduler_database(
    paths: &GoldBandPaths,
    destination: &ScheduledTaskDatabase,
    applied_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let legacy_path = paths.legacy_scheduler_db_path();
    if legacy_path == paths.scheduler_db_path() {
        return Ok(());
    }
    destination.import_legacy_database_path_once(legacy_path, &paths.project_id, applied_at)?;
    Ok(())
}

fn ensure_definition_workspace(
    app: &App,
    definition: &ScheduledTaskDefinition,
) -> anyhow::Result<()> {
    if definition.project_id != app.paths.project_id {
        anyhow::bail!(
            "scheduled task {} belongs to project {}, but scheduler app is project {}",
            definition.id,
            definition.project_id,
            app.paths.project_id
        );
    }
    Ok(())
}

fn persist_runtime_projection<F>(
    database: &ScheduledTaskDatabase,
    definition: &ScheduledTaskDefinition,
    expected_revision: i64,
    notify: F,
) -> anyhow::Result<Option<ScheduledJobRecord>>
where
    F: FnOnce(&ScheduledJobRecord),
{
    match database.update_job_runtime_projection(definition, expected_revision)? {
        UpdateJobResult::Updated(updated) => {
            notify(&updated);
            Ok(Some(updated))
        }
        UpdateJobResult::Conflict(_) | UpdateJobResult::NotFound => Ok(None),
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

fn materialize_registered_deadline(
    database: &ScheduledTaskDatabase,
    project_id: &str,
    job_id: &str,
    registered: RegisteredDeadline,
    now: DateTime<Utc>,
) -> anyhow::Result<DueMaterialization> {
    let Some(current) = database.get_job_definition(project_id, job_id)? else {
        return Ok(DueMaterialization::Stale);
    };
    if !registered.matches(&current) || registered.scheduled_deadline.is_none() {
        return Ok(DueMaterialization::Stale);
    }
    Ok(database.materialize_due_occurrence(project_id, job_id, registered.revision, now)?)
}

struct MissedReconcileResult {
    record: Option<ScheduledJobRecord>,
    missed_count: u32,
}

fn reconcile_missed_deadlines(
    database: &ScheduledTaskDatabase,
    key: &ScheduledJobKey,
    now: DateTime<Utc>,
) -> anyhow::Result<MissedReconcileResult> {
    let mut missed_count = 0u32;
    for _ in 0..MISSED_RECONCILE_BATCH_SIZE {
        let Some(current) = database.get_job_definition(&key.project_id, &key.job_id)? else {
            return Ok(MissedReconcileResult {
                record: None,
                missed_count,
            });
        };
        if !current.definition.enabled {
            return Ok(MissedReconcileResult {
                record: Some(current),
                missed_count,
            });
        }
        let Some(deadline) = current.next_run_at else {
            return Ok(MissedReconcileResult {
                record: Some(current),
                missed_count,
            });
        };
        if deadline >= now - LATE_FIRE_GRACE {
            return Ok(MissedReconcileResult {
                record: Some(current),
                missed_count,
            });
        }
        let materialized = database.materialize_due_occurrence(
            &key.project_id,
            &key.job_id,
            current.revision,
            now,
        )?;
        let DueMaterialization::Ready {
            mut job,
            occurrence,
        } = materialized
        else {
            continue;
        };
        match database.mark_missed_for_existing_job(
            &key.project_id,
            &key.job_id,
            occurrence.scheduled_at,
        )? {
            Some(true) => {}
            Some(false) => {
                return Ok(MissedReconcileResult {
                    record: database.get_job_definition(&key.project_id, &key.job_id)?,
                    missed_count,
                });
            }
            None => {
                return Ok(MissedReconcileResult {
                    record: None,
                    missed_count,
                });
            }
        }
        missed_count = missed_count.saturating_add(1);
        advance_definition_after_point(&mut job.definition, occurrence.scheduled_at, "missed", now);
        if matches!(job.definition.schedule.kind, ScheduleKind::At { .. }) {
            job.definition.enabled = false;
        }
        match database.update_job_runtime_projection(&job.definition, job.revision)? {
            UpdateJobResult::Updated(_) | UpdateJobResult::Conflict(_) => {}
            UpdateJobResult::NotFound => {
                return Ok(MissedReconcileResult {
                    record: None,
                    missed_count,
                });
            }
        }
    }
    Ok(MissedReconcileResult {
        record: database.get_job_definition(&key.project_id, &key.job_id)?,
        missed_count,
    })
}

#[cfg(test)]
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
        if database
            .mark_missed_for_existing_job(&definition.project_id, definition.id(), next)?
            .is_none()
        {
            return Ok(false);
        }
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
    project_id: &str,
    job_id: &str,
) -> anyhow::Result<ScheduledOccurrence> {
    database
        .create_or_get_occurrence_for_existing_job(
            project_id,
            job_id,
            Utc::now(),
            OccurrenceTriggerKind::Manual,
        )?
        .ok_or_else(|| anyhow::anyhow!("scheduled job no longer exists"))
}

pub(crate) fn scheduled_agent_unattended_error(
    app: &App,
    definition: &ScheduledTaskDefinition,
) -> Option<ScheduledError> {
    if definition.mode != ScheduledMode::Direct {
        return None;
    }
    let provider = definition
        .execution_config
        .get("directConfig")
        .and_then(|value| value.get("agentType"))
        .and_then(serde_json::Value::as_str)
        .or(definition.content_snapshot.direct_agent_id.as_deref())?;
    let diagnostics = app.provider_diagnostics();
    let available_modes = diagnostics
        .get(provider)
        .map(|diagnostic| {
            gold_band::provider::supported_modes_from_capabilities(diagnostic.capabilities.as_ref())
        })
        .unwrap_or_default();
    // When a provider exposes no ACP mode config options at all (e.g. Codex ACP,
    // which manages permissions internally), we cannot evaluate its unattended
    // capability through the standard mode contract. Treat an empty mode list
    // as "provider-managed permissions" and skip the gate so that such agents
    // are not blocked from scheduled execution.
    if available_modes.is_empty() {
        return None;
    }
    let resolved_full_auto = app.config.resolve_permission_mode(provider, "full_access");
    let configured_mode = definition
        .execution_config
        .get("directConfig")
        .and_then(|value| value.get("permissionMode"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let configured_is_full_auto = configured_mode.map(is_full_auto_mode).unwrap_or(true);
    let resolved_is_available = available_modes.iter().any(|mode| {
        mode.id.eq_ignore_ascii_case(&resolved_full_auto) || is_full_auto_mode(&mode.id)
    });
    if configured_is_full_auto && resolved_is_available {
        return None;
    }
    Some(ScheduledError::with_params(
        ScheduledErrorCode::AgentUnattendedModeUnsupported,
        serde_json::json!({
            "provider": provider,
            "requestedMode": configured_mode,
            "resolvedFullAutoMode": resolved_full_auto,
            "availableModes": available_modes.iter().map(|mode| mode.id.clone()).collect::<Vec<_>>(),
        }),
    ))
}

fn is_full_auto_mode(value: &str) -> bool {
    matches!(
        value
            .trim()
            .to_ascii_lowercase()
            .replace(['_', '-'], "")
            .as_str(),
        "fullaccess" | "bypass" | "bypasspermissions" | "fullauto" | "agentfullaccess"
    )
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

fn event_finishes_occurrence(event: &RuntimeLifecycleEvent) -> bool {
    matches!(
        event,
        RuntimeLifecycleEvent::InterventionRequested { .. }
            | RuntimeLifecycleEvent::RunCompleted { .. }
            | RuntimeLifecycleEvent::AcpTurnFinished { .. }
    )
}

async fn finish_reconciled_occurrence(
    database: &ScheduledTaskDatabase,
    active: &ActiveOccurrenceRegistry,
    occurrence_id: &str,
    owner_id: &str,
    status: OccurrenceStatus,
    error: Option<ScheduledError>,
) -> anyhow::Result<Option<ScheduledOccurrence>> {
    if let Some(entry) = take_active_occurrence(active, occurrence_id) {
        entry.guard.stop().await;
    }
    if !database.finish_occurrence(occurrence_id, owner_id, status, None, error)? {
        return Ok(None);
    }
    Ok(database.get_occurrence(occurrence_id)?)
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

fn active_run_ids(app: &App) -> anyhow::Result<HashSet<String>> {
    let mut active = HashSet::new();
    for task in app.task_list()? {
        for run in app.run_list(&task.id)? {
            if run.status != RunStatus::Completed {
                active.insert(run.id);
            }
        }
    }
    Ok(active)
}

#[derive(Debug, Clone)]
pub(super) struct ExecutionResult {
    pub(super) immediate_links: Option<OccurrenceLinks>,
}

#[cfg(test)]
fn accept_occurrence_links_then<T, F>(
    database: &ScheduledTaskDatabase,
    occurrence_id: &str,
    owner_id: &str,
    now: DateTime<Utc>,
    links: &OccurrenceLinks,
    launch: F,
) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T>,
{
    if !database.accept_occurrence_links(occurrence_id, owner_id, now, links)? {
        anyhow::bail!("scheduled occurrence execution links were not accepted");
    }
    match launch() {
        Ok(result) => Ok(result),
        Err(launch_error) => {
            let finish_result = database.finish_occurrence(
                occurrence_id,
                owner_id,
                OccurrenceStatus::Failed,
                None,
                Some(ScheduledError::with_params(
                    ScheduledErrorCode::ExecutionFailed,
                    serde_json::json!({ "reason": launch_error.to_string() }),
                )),
            );
            match finish_result {
                Ok(true) => Err(launch_error),
                Ok(false) => Err(launch_error.context(
                    "accepted scheduled occurrence could not be finished after launch failure",
                )),
                Err(finish_error) => Err(launch_error.context(format!(
                    "failed to finish accepted scheduled occurrence after launch failure: {finish_error}"
                ))),
            }
        }
    }
}

fn accept_occurrence_links_then_deferred<T, F>(
    database: &ScheduledTaskDatabase,
    occurrence_id: &str,
    owner_id: &str,
    now: DateTime<Utc>,
    links: &OccurrenceLinks,
    launch: F,
) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T>,
{
    if !database.accept_occurrence_links(occurrence_id, owner_id, now, links)? {
        anyhow::bail!("scheduled occurrence execution links were not accepted");
    }
    launch()
}

#[cfg(test)]
fn recover_accepted_occurrence(
    database: &ScheduledTaskDatabase,
    app: &App,
    occurrence: &ScheduledOccurrence,
    owner_id: &str,
) -> anyhow::Result<Option<ScheduledOccurrence>> {
    let Some((status, error)) = recovery_outcome_for_accepted_occurrence(app, occurrence)? else {
        return Ok(None);
    };
    if !database.finish_occurrence(&occurrence.id, owner_id, status, None, error)? {
        anyhow::bail!("failed to finish recovered accepted scheduled occurrence");
    }
    Ok(database.get_occurrence(&occurrence.id)?)
}

fn recovery_outcome_for_accepted_occurrence(
    app: &App,
    occurrence: &ScheduledOccurrence,
) -> anyhow::Result<Option<(OccurrenceStatus, Option<ScheduledError>)>> {
    let (Some(task_id), Some(run_id)) =
        (occurrence.task_id.as_deref(), occurrence.run_id.as_deref())
    else {
        return Ok(None);
    };
    let (status, error) = match app.run_status(task_id, run_id) {
        Ok(run) if run.status == RunStatus::Completed => match run.outcome {
            Some(RunOutcome::Success) => (OccurrenceStatus::Succeeded, None),
            Some(RunOutcome::Failure | RunOutcome::Killed) | None => (
                OccurrenceStatus::Failed,
                Some(ScheduledError::new(ScheduledErrorCode::ExecutionFailed)),
            ),
        },
        Ok(run) => {
            if run.status == RunStatus::Running {
                let _ = app.run_pause(
                    task_id,
                    run_id,
                    gold_band::domain::PauseReason::ProcessInterrupted,
                );
            }
            (
                OccurrenceStatus::Failed,
                Some(ScheduledError::new(ScheduledErrorCode::LeaseLost)),
            )
        }
        Err(_) => (
            OccurrenceStatus::Failed,
            Some(ScheduledError::new(ScheduledErrorCode::LeaseLost)),
        ),
    };
    Ok(Some((status, error)))
}

fn task_has_active_execution(app: &App, task_id: Option<&str>) -> anyhow::Result<bool> {
    Ok(active_execution_for_task(app, task_id)?.is_active())
}

/// 主动状态对账：判断一条 running occurrence 是否需要收尾。
///
/// 与 `recovery_outcome_for_accepted_occurrence`（claim 时自检，run 还 Running 会 pause 并 Failed）
/// 不同，本函数用于 scheduler 周期性对账：**Task/Run 仍 active 就保留**（返回 None，不误杀长任务），
/// 只有 Task/Run 已 Completed/不存在（说明 lifecycle 事件丢失）才给出终态。
fn reconcile_running_occurrence_outcome(
    app: &App,
    occurrence: &ScheduledOccurrence,
) -> anyhow::Result<Option<(OccurrenceStatus, Option<ScheduledError>)>> {
    let (Some(task_id), Some(run_id)) =
        (occurrence.task_id.as_deref(), occurrence.run_id.as_deref())
    else {
        // claim 后尚未 execute（无 task_id/run_id）：交给 lease 机制处理，对账不动。
        return Ok(None);
    };
    let (status, error) = match app.run_status(task_id, run_id) {
        Ok(run) => {
            if active_execution_for_loaded_run(app, task_id, &run).is_active() {
                // 只判断 occurrence 自己关联的 Run；同 Task 的其他 Run 不属于该 occurrence。
                return Ok(None);
            }
            if run.status == RunStatus::Completed {
                match run.outcome {
                    Some(RunOutcome::Success) => (OccurrenceStatus::Succeeded, None),
                    Some(RunOutcome::Failure | RunOutcome::Killed) | None => (
                        OccurrenceStatus::Failed,
                        Some(ScheduledError::new(ScheduledErrorCode::ExecutionFailed)),
                    ),
                }
            } else {
                (
                    OccurrenceStatus::Failed,
                    Some(ScheduledError::new(ScheduledErrorCode::LeaseLost)),
                )
            }
        }
        Err(_) => (
            OccurrenceStatus::Failed,
            Some(ScheduledError::new(ScheduledErrorCode::LeaseLost)),
        ),
    };
    Ok(Some((status, error)))
}

fn active_execution_for_loaded_run(app: &App, task_id: &str, run: &RunState) -> ActiveExecution {
    let has_active_prompt = match (
        run.current_round.as_deref(),
        run.current_node.as_deref(),
        run.current_attempt.as_deref(),
    ) {
        (Some(round_id), Some(node_id), Some(attempt_id)) => {
            let attempt_dir = app
                .paths
                .attempt_dir(task_id, &run.id, round_id, node_id, attempt_id);
            attempt_tree_has_active_prompt(&attempt_dir, &|path| prompt_activity(path).is_some())
        }
        _ => false,
    };
    active_execution_for_run(run.status, run.pause_reason, has_active_prompt)
}

fn active_execution_for_task(app: &App, task_id: Option<&str>) -> anyhow::Result<ActiveExecution> {
    active_execution_for_task_with_prompt_probe(app, task_id, |attempt_dir| {
        prompt_activity(attempt_dir).is_some()
    })
}

fn task_has_active_execution_with_prompt_probe<F>(
    app: &App,
    task_id: Option<&str>,
    prompt_probe: F,
) -> anyhow::Result<bool>
where
    F: Fn(&Utf8Path) -> bool,
{
    Ok(active_execution_for_task_with_prompt_probe(app, task_id, prompt_probe)?.is_active())
}

fn active_execution_for_task_with_prompt_probe<F>(
    app: &App,
    task_id: Option<&str>,
    prompt_probe: F,
) -> anyhow::Result<ActiveExecution>
where
    F: Fn(&Utf8Path) -> bool,
{
    let Some(task_id) = task_id else {
        return Ok(ActiveExecution::Idle);
    };
    let mut active = ActiveExecution::Idle;
    for run in app.run_list(task_id)? {
        let has_active_prompt = match (
            run.current_round.as_deref(),
            run.current_node.as_deref(),
            run.current_attempt.as_deref(),
        ) {
            (Some(round_id), Some(node_id), Some(attempt_id)) => {
                let attempt_dir = app
                    .paths
                    .attempt_dir(task_id, &run.id, round_id, node_id, attempt_id);
                attempt_tree_has_active_prompt(&attempt_dir, &prompt_probe)
            }
            _ => false,
        };
        active = merge_active_execution(
            active,
            active_execution_for_run(run.status, run.pause_reason, has_active_prompt),
        );
        if active == ActiveExecution::Running {
            break;
        }
    }
    Ok(active)
}

fn active_execution_for_run(
    status: RunStatus,
    pause_reason: Option<gold_band::domain::PauseReason>,
    has_active_prompt: bool,
) -> ActiveExecution {
    if has_active_prompt || status == RunStatus::Running {
        return ActiveExecution::Running;
    }
    if status != RunStatus::Paused {
        return ActiveExecution::Idle;
    }
    match pause_reason {
        Some(gold_band::domain::PauseReason::PermissionRequested) => {
            ActiveExecution::PermissionWaiting
        }
        Some(gold_band::domain::PauseReason::WaitingForUserInput) => {
            ActiveExecution::WaitingForUserInput
        }
        Some(
            gold_band::domain::PauseReason::ProcessInterrupted
            | gold_band::domain::PauseReason::RuntimeAbnormal
            | gold_band::domain::PauseReason::ErrorBlocked,
        )
        | None => ActiveExecution::ResumablePaused,
    }
}

fn merge_active_execution(current: ActiveExecution, candidate: ActiveExecution) -> ActiveExecution {
    fn priority(state: ActiveExecution) -> u8 {
        match state {
            ActiveExecution::Idle => 0,
            ActiveExecution::ResumablePaused => 1,
            ActiveExecution::WaitingForUserInput => 2,
            ActiveExecution::PermissionWaiting => 3,
            ActiveExecution::Running => 4,
        }
    }
    if priority(candidate) > priority(current) {
        candidate
    } else {
        current
    }
}

fn attempt_tree_has_active_prompt<F>(attempt_dir: &Utf8Path, prompt_probe: &F) -> bool
where
    F: Fn(&Utf8Path) -> bool,
{
    if prompt_probe(attempt_dir) {
        return true;
    }
    let dynamic_nodes_dir = attempt_dir.join("dynamic").join("nodes");
    let Ok(nodes) = std::fs::read_dir(dynamic_nodes_dir.as_std_path()) else {
        return false;
    };
    nodes.filter_map(Result::ok).any(|node_entry| {
        let node_dir = node_entry.path();
        if !node_dir.is_dir() {
            return false;
        }
        let Ok(attempts) = std::fs::read_dir(node_dir) else {
            return false;
        };
        attempts.filter_map(Result::ok).any(|attempt_entry| {
            let Ok(attempt_dir) = Utf8PathBuf::from_path_buf(attempt_entry.path()) else {
                return false;
            };
            attempt_dir.is_dir() && prompt_probe(attempt_dir.as_path())
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScheduledExecutionAction {
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
    if definition.mode != ScheduledMode::Direct
        && definition.task_id.is_some()
        && task_fingerprint != Some(definition.content_fingerprint.as_str())
    {
        return ScheduledExecutionAction::MaterializeTaskAndRun;
    }
    scheduled_execution_action(definition)
}

fn execute_definition(
    app_handle: &AppHandle,
    app: &App,
    database: &ScheduledTaskDatabase,
    owner_id: &str,
    definition: &mut ScheduledTaskDefinition,
    occurrence: &ScheduledOccurrence,
    trigger_kind: &str,
) -> anyhow::Result<ExecutionResult> {
    ensure_definition_workspace(app, definition)?;
    let task_fingerprint = definition.task_id.as_deref().and_then(|task_id| {
        crate::view_models_conversation::scheduled_content_fingerprint_for_task(app, task_id)
    });
    let action =
        scheduled_execution_action_for_fingerprint(definition, task_fingerprint.as_deref());
    let adapter = adapter_for(definition, &action);
    let binding = adapter.start(ScheduledExecutionContext {
        app_handle,
        app,
        database,
        owner_id,
        definition,
        occurrence,
        trigger_kind,
    })?;
    Ok(ExecutionResult {
        immediate_links: Some(OccurrenceLinks {
            task_id: binding.task_id,
            run_id: binding.run_id,
            round_id: binding.round_id,
            attempt_id: binding.attempt_id,
        }),
    })
}

pub(super) fn execute_definition_with_action(
    app_handle: &AppHandle,
    app: &App,
    database: &ScheduledTaskDatabase,
    owner_id: &str,
    definition: &mut ScheduledTaskDefinition,
    occurrence: &ScheduledOccurrence,
    trigger_kind: &str,
    action: ScheduledExecutionAction,
) -> anyhow::Result<ExecutionResult> {
    ensure_definition_workspace(app, definition)?;
    let occurrence_id = occurrence.id.as_str();
    let scheduled_at = occurrence.scheduled_at;
    match action {
        ScheduledExecutionAction::ContinueSession { task_id } => {
            if let Some((run_id, round_id, node_id, attempt_id)) = latest_attempt(app, &task_id)? {
                let input = scheduled_create_input(app, definition)?;
                let scheduled_app = app
                    .clone_for_background()
                    .with_scheduled_occurrence_id(Some(occurrence_id.to_string()))
                    .with_scheduled_task_context(Some(scheduled_task_context_info(
                        definition,
                        trigger_kind,
                        scheduled_at,
                    )));
                let scheduled_app = configure_conversation_runtime_callbacks(
                    scheduled_app,
                    app_handle.clone(),
                    Some(definition.project_id.clone()),
                );
                let handle = app_handle.clone();
                let project_id = Some(definition.project_id.clone());
                let task_id_for_thread = task_id.clone();
                let run_id_for_thread = run_id.clone();
                let round_id_for_thread = round_id.clone();
                let node_id_for_thread = node_id.clone();
                let attempt_id_for_thread = attempt_id.clone();
                let links = OccurrenceLinks {
                    task_id: Some(task_id),
                    run_id: Some(run_id),
                    round_id: Some(round_id),
                    attempt_id: Some(attempt_id),
                };
                accept_occurrence_links_then_deferred(
                    database,
                    occurrence_id,
                    owner_id,
                    Utc::now(),
                    &links,
                    || {
                        thread::Builder::new()
                            .name("scheduled-continuous-prompt".to_string())
                            .spawn(move || {
                                let result = tauri::async_runtime::block_on(
                                    crate::commands::send_acp_prompt_with_configured_app(
                                        handle,
                                        scheduled_app,
                                        project_id,
                                        task_id_for_thread,
                                        run_id_for_thread,
                                        round_id_for_thread,
                                        node_id_for_thread,
                                        attempt_id_for_thread,
                                        input.content.into(),
                                        None,
                                        None,
                                        None,
                                        input.attachment_paths,
                                    ),
                                );
                                if let Err(error) = result {
                                    warn!(%error.code, "scheduled continuous prompt failed");
                                }
                            })?;
                        Ok(())
                    },
                )?;
                return Ok(ExecutionResult {
                    immediate_links: Some(links),
                });
            }
        }
        ScheduledExecutionAction::StartNewRun { task_id } => {
            let scheduled_app = app
                .clone_for_background()
                .with_scheduled_occurrence_id(Some(occurrence_id.to_string()))
                .with_scheduled_task_context(Some(scheduled_task_context_info(
                    definition,
                    trigger_kind,
                    scheduled_at,
                )));
            let scheduled_app = configure_conversation_runtime_callbacks(
                scheduled_app,
                app_handle.clone(),
                Some(definition.project_id.clone()),
            );
            let prepared_run = match definition.mode {
                ScheduledMode::Workflow => {
                    let authoring = scheduled_workflow_authoring(definition)?.ok_or_else(|| {
                        anyhow::anyhow!("scheduled workflow authoring snapshot is missing")
                    })?;
                    scheduled_app.prepare_run_with_authoring(&task_id, &authoring)?
                }
                ScheduledMode::Auto => scheduled_app.prepare_run(&task_id, None)?,
                ScheduledMode::Direct => {
                    anyhow::bail!("direct mode cannot use the start-new-run action")
                }
            };
            let run = prepared_run.run().clone();
            let links = OccurrenceLinks {
                task_id: Some(task_id.clone()),
                run_id: Some(run.id),
                round_id: run.current_round,
                attempt_id: run.current_attempt,
            };
            accept_occurrence_links_then_deferred(
                database,
                occurrence_id,
                owner_id,
                Utc::now(),
                &links,
                || {
                    scheduled_app
                        .launch_prepared_run_background(&task_id, prepared_run.accept())?;
                    Ok(())
                },
            )?;
            return Ok(ExecutionResult {
                immediate_links: Some(links),
            });
        }
        ScheduledExecutionAction::MaterializeTaskAndRun => {}
    }

    let input = scheduled_create_input(app, definition)?;
    let scheduled_app = app
        .clone_for_background()
        .with_scheduled_occurrence_id(Some(occurrence_id.to_string()))
        .with_scheduled_task_context(Some(scheduled_task_context_info(
            definition,
            trigger_kind,
            scheduled_at,
        )));
    let run_app = configure_conversation_runtime_callbacks(
        scheduled_app,
        app_handle.clone(),
        Some(definition.project_id.clone()),
    );
    let prepared_task =
        crate::view_models_conversation::prepare_conversation_task_vm(&run_app, &input)?;
    let task_id = prepared_task.task_id().to_string();
    let prepared_run = run_app.prepare_run(&task_id, None)?;
    let run = prepared_run.run().clone();
    let links = OccurrenceLinks {
        task_id: Some(task_id.clone()),
        run_id: Some(run.id),
        round_id: run.current_round,
        attempt_id: run.current_attempt,
    };
    accept_occurrence_links_then_deferred(
        database,
        occurrence_id,
        owner_id,
        Utc::now(),
        &links,
        || {
            let _ = prepared_task.accept();
            run_app.launch_prepared_run_background(&task_id, prepared_run.accept())?;
            Ok(())
        },
    )?;
    definition.task_id = Some(task_id);
    Ok(ExecutionResult {
        immediate_links: Some(links),
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
    let include_optional_entry = config
        .get("includeOptionalEntry")
        .and_then(|value| value.as_bool());
    let workflow_authoring = scheduled_workflow_authoring(definition)?;
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
        include_optional_entry,
        direct_config,
        auto_config,
        attachment_paths: (!attachment_paths.is_empty()).then_some(attachment_paths),
        work_location: Default::default(),
        scheduled_task_id: Some(definition.id.clone()),
        scheduled_content_fingerprint: Some(definition.content_fingerprint.clone()),
        workflow_authoring,
    })
}

fn scheduled_workflow_authoring(
    definition: &ScheduledTaskDefinition,
) -> anyhow::Result<Option<TaskAuthoringWorkflow>> {
    let Some(value) = definition.content_snapshot.workflow_authoring.clone() else {
        return Ok(None);
    };
    let compat = serde_json::from_value::<TaskAuthoringWorkflowCompat>(value)?;
    let (mut authoring, _) = compat.into_current();
    migrate_authoring_workflow(&mut authoring.workflow, &mut authoring.model_bindings, None)?;
    Ok(Some(authoring))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration as StdDuration;

    use chrono::{Duration, TimeZone, Utc};
    use gold_band::app::{RuntimeInterventionKind, RuntimeLifecycleEvent};
    use gold_band::config::ProviderDiagnosticSnapshot;
    use gold_band::domain::{PauseReason, RunOutcome, RunStatus, VERSION};
    use gold_band::runtime::{RunState, RuntimeExecutionPhase, RuntimeExecutionState};
    use gold_band::scheduler::coordinator::{DeadlineRegistry, ReconcileReason, ScheduledJobKey};
    use gold_band::scheduler::db::{DueMaterialization, ScheduledTaskDatabase, UpdateJobResult};
    use gold_band::scheduler::occurrence::{
        ClaimResult, LeaseConfig, OccurrenceStatus, OccurrenceTriggerKind, ScheduledErrorCode,
    };
    use gold_band::scheduler::queue::{
        ActiveExecution, MISSED_RECONCILE_BATCH_SIZE, QueueDecision, decide_queue,
    };
    use gold_band::scheduler::{
        OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition, SessionPolicy,
    };
    use gold_band::storage::write_json;
    use gold_band::workflow_model_binding::{
        TaskAuthoringWorkflow, WorkerModelBinding, WorkflowModelBindings,
    };
    use tempfile::tempdir;
    use tokio::sync::Notify;

    use super::{
        ActiveOccurrenceMetadata, CLOCK_DRIFT_CHECK_INTERVAL, ClaimToHandoffGuard,
        ClockDriftDetector, CoordinatorRuntimeDriver, LATE_FIRE_GRACE, OccurrenceExecutionGuard,
        PendingGuardJoins, RegisteredDeadline, ScheduledExecutionAction, SchedulerCommand,
        SchedulerCoordinator, SchedulerCoordinatorHandle, WORKSPACE_REGISTRATION_RETRY_DELAY,
        WorkspaceRegistration, accept_occurrence_links_then, active_execution_for_run,
        attempt_tree_has_active_prompt, create_manual_occurrence, ensure_definition_workspace,
        finish_occurrence_for_event, finish_reconciled_occurrence, mark_past_points_missed,
        materialize_registered_deadline, migrate_legacy_scheduler_database,
        persist_runtime_projection, project_resumed_attention, reconcile_missed_deadlines,
        recover_accepted_occurrence, scheduled_agent_unattended_error, scheduled_execution_action,
        scheduled_execution_action_for_fingerprint, scheduled_occurrence_updated_event,
        shutdown_active_occurrences, task_has_active_execution,
        task_has_active_execution_with_prompt_probe,
    };

    #[derive(Default)]
    struct TestCoordinatorRuntime;

    impl CoordinatorRuntimeDriver for TestCoordinatorRuntime {
        fn app_for_workspace(
            &self,
            workspace_path: &camino::Utf8Path,
        ) -> anyhow::Result<gold_band::app::App> {
            Ok(gold_band::app::App::new(workspace_path.to_path_buf()))
        }

        async fn shutdown_active_leases(&self, _now: chrono::DateTime<Utc>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn process_occurrence(
            &self,
            _database: &ScheduledTaskDatabase,
            _app: &gold_band::app::App,
            _job: gold_band::scheduler::db::ScheduledJobRecord,
            _occurrence: gold_band::scheduler::occurrence::ScheduledOccurrence,
            _now: chrono::DateTime<Utc>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn run_manual(
            &self,
            app: &gold_band::app::App,
            job: &gold_band::scheduler::db::ScheduledJobRecord,
        ) -> anyhow::Result<crate::scheduled_service::ManualRunResult> {
            let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
            let occurrence = create_manual_occurrence(
                &database,
                &job.definition.project_id,
                job.definition.id(),
            )?;
            let now = Utc::now();
            let owner_id = "test-coordinator";
            database.claim_occurrence(&occurrence.id, owner_id, now, now + Duration::minutes(1))?;
            database.finish_occurrence(
                &occurrence.id,
                owner_id,
                OccurrenceStatus::Succeeded,
                None,
                None,
            )?;
            Ok(crate::scheduled_service::ManualRunResult {
                occurrence: database
                    .get_occurrence(&occurrence.id)?
                    .ok_or_else(|| anyhow::anyhow!("manual occurrence disappeared"))?,
                immediate_links: None,
            })
        }

        async fn resume_attention(
            &self,
            database: &ScheduledTaskDatabase,
            _app: &gold_band::app::App,
            task_id: &str,
            run_id: &str,
            round_id: &str,
            attempt_id: &str,
        ) -> anyhow::Result<Option<String>> {
            Ok(database
                .find_attention_occurrence_by_links(task_id, run_id, round_id, attempt_id)?
                .map(|occurrence| occurrence.id))
        }
    }

    struct LoopCoordinatorRuntime {
        wall_now: Mutex<chrono::DateTime<Utc>>,
        remaining_workspace_failures: AtomicUsize,
        remaining_process_failures: AtomicUsize,
        remaining_release_failures: AtomicUsize,
        registration_attempts: AtomicUsize,
        processed_occurrences: AtomicUsize,
        shutdown_releases: AtomicUsize,
        power_reconciliations: Mutex<Vec<(usize, bool)>>,
    }

    impl LoopCoordinatorRuntime {
        fn new(now: chrono::DateTime<Utc>, workspace_failures: usize) -> Self {
            Self {
                wall_now: Mutex::new(now),
                remaining_workspace_failures: AtomicUsize::new(workspace_failures),
                remaining_process_failures: AtomicUsize::new(0),
                remaining_release_failures: AtomicUsize::new(0),
                registration_attempts: AtomicUsize::new(0),
                processed_occurrences: AtomicUsize::new(0),
                shutdown_releases: AtomicUsize::new(0),
                power_reconciliations: Mutex::new(Vec::new()),
            }
        }

        fn set_now(&self, now: chrono::DateTime<Utc>) {
            *self.wall_now.lock().unwrap() = now;
        }

        fn advance_wall(&self, duration: StdDuration) {
            let duration = Duration::from_std(duration).unwrap();
            let mut now = self.wall_now.lock().unwrap();
            *now += duration;
        }

        fn fail_next_processes(&self, count: usize) {
            self.remaining_process_failures
                .store(count, Ordering::SeqCst);
        }

        fn fail_next_releases(&self, count: usize) {
            self.remaining_release_failures
                .store(count, Ordering::SeqCst);
        }
    }

    impl CoordinatorRuntimeDriver for LoopCoordinatorRuntime {
        fn app_for_workspace(
            &self,
            workspace_path: &camino::Utf8Path,
        ) -> anyhow::Result<gold_band::app::App> {
            self.registration_attempts.fetch_add(1, Ordering::SeqCst);
            if self
                .remaining_workspace_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                anyhow::bail!("transient workspace registration failure");
            }
            Ok(gold_band::app::App::new(workspace_path.to_path_buf()))
        }

        fn now(&self) -> chrono::DateTime<Utc> {
            *self.wall_now.lock().unwrap()
        }

        async fn shutdown_active_leases(&self, _now: chrono::DateTime<Utc>) -> anyhow::Result<()> {
            self.shutdown_releases.fetch_add(1, Ordering::SeqCst);
            if self
                .remaining_release_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                anyhow::bail!("injected active lease release failure");
            }
            Ok(())
        }

        fn reconcile_power_state(&self, enabled_job_count: usize, app_is_running: bool) {
            self.power_reconciliations
                .lock()
                .unwrap()
                .push((enabled_job_count, app_is_running));
        }

        async fn process_occurrence(
            &self,
            database: &ScheduledTaskDatabase,
            _app: &gold_band::app::App,
            _job: gold_band::scheduler::db::ScheduledJobRecord,
            occurrence: gold_band::scheduler::occurrence::ScheduledOccurrence,
            now: chrono::DateTime<Utc>,
        ) -> anyhow::Result<()> {
            if self
                .remaining_process_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                anyhow::bail!("transient occurrence processing failure");
            }
            let owner_id = "loop-runtime";
            let claim = database.claim_occurrence(
                &occurrence.id,
                owner_id,
                now,
                now + Duration::days(365),
            )?;
            if let gold_band::scheduler::occurrence::ClaimResult::Claimed(_) = claim {
                self.processed_occurrences.fetch_add(1, Ordering::SeqCst);
                database.finish_occurrence(
                    &occurrence.id,
                    owner_id,
                    OccurrenceStatus::Succeeded,
                    None,
                    None,
                )?;
            }
            Ok(())
        }

        async fn run_manual(
            &self,
            app: &gold_band::app::App,
            job: &gold_band::scheduler::db::ScheduledJobRecord,
        ) -> anyhow::Result<crate::scheduled_service::ManualRunResult> {
            let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
            let occurrence = create_manual_occurrence(
                &database,
                &job.definition.project_id,
                job.definition.id(),
            )?;
            Ok(crate::scheduled_service::ManualRunResult {
                occurrence,
                immediate_links: None,
            })
        }
    }

    fn command_loop_coordinator(
        runtime: Arc<LoopCoordinatorRuntime>,
    ) -> (
        SchedulerCoordinatorHandle,
        SchedulerCoordinator<LoopCoordinatorRuntime>,
    ) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = SchedulerCoordinatorHandle::new(sender.clone());
        let coordinator = SchedulerCoordinator::new_with_sender(runtime, sender, receiver);
        (handle, coordinator)
    }

    async fn settle_command_loop() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn scheduled_create_input_uses_frozen_workflow_authoring_snapshot() {
        let directory = tempdir().unwrap();
        let app = gold_band::app::App::new(
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap(),
        );
        let mut definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "frozen-workflow",
            "workflow",
            ScheduleSpec::at(Utc::now() + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let workflow = gold_band::dsl::WorkflowDsl {
            version: "0.1".to_string(),
            id: "frozen".to_string(),
            entry: "worker".to_string(),
            control: Default::default(),
            nodes: vec![gold_band::dsl::NodeDsl::Worker(
                gold_band::dsl::WorkerNode {
                    id: "worker".to_string(),
                    execution_slot_id: Some("slot-frozen".to_string()),
                    provider: None,
                    model: None,
                    profile: Some("developer".to_string()),
                    goal: Some("frozen goal".to_string()),
                    output: None,
                    success_condition: None,
                    permission_mode: None,
                    config_options: Default::default(),
                    manual_check: None,
                    prompt_envelope: Default::default(),
                },
            )],
            edges: vec![],
        };
        let authoring = TaskAuthoringWorkflow {
            workflow,
            model_bindings: WorkflowModelBindings {
                definition_revision: "revision".to_string(),
                binding_revision: 4,
                bindings: vec![WorkerModelBinding {
                    execution_slot_id: "slot-frozen".to_string(),
                    agent_id: "agent-frozen".to_string(),
                    model_id: Some("model-frozen".to_string()),
                    permission_mode_id: Some("ask".to_string()),
                    config_options: Default::default(),
                }],
            },
        };
        definition.content_snapshot.workflow_authoring =
            Some(serde_json::to_value(&authoring).unwrap());
        definition.execution_config = serde_json::json!({
            "runMode": "workflow",
            "workflowTemplateId": "template-that-no-longer-exists"
        });

        let input = super::scheduled_create_input(&app, &definition).unwrap();

        assert_eq!(
            input.workflow_template_id.as_deref(),
            Some("template-that-no-longer-exists")
        );
        let frozen = input.workflow_authoring.unwrap();
        assert_eq!(
            serde_json::to_value(&frozen.workflow).unwrap(),
            serde_json::to_value(&authoring.workflow).unwrap()
        );
        assert_eq!(
            frozen.model_bindings.bindings,
            authoring.model_bindings.bindings
        );
        assert_eq!(
            frozen.model_bindings.definition_revision,
            gold_band::workflow_model_binding::definition_revision(&frozen.workflow)
        );
        assert_eq!(
            frozen.model_bindings.binding_revision,
            authoring.model_bindings.binding_revision
        );
    }

    #[tokio::test(start_paused = true)]
    async fn power_reconcile_counts_enabled_jobs_across_registered_workspaces() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let first_path = camino::Utf8PathBuf::from_path_buf(first.path().to_path_buf()).unwrap();
        let second_path = camino::Utf8PathBuf::from_path_buf(second.path().to_path_buf()).unwrap();
        let first_app = gold_band::app::App::new(first_path.clone());
        let second_app = gold_band::app::App::new(second_path.clone());
        let first_db = ScheduledTaskDatabase::open(first_app.paths.scheduler_db_path()).unwrap();
        let second_db = ScheduledTaskDatabase::open(second_app.paths.scheduler_db_path()).unwrap();
        let deadline = Utc::now() + Duration::hours(1);
        for (database, project_id, job_id) in [
            (&first_db, first_app.paths.project_id.as_str(), "job-a"),
            (&second_db, second_app.paths.project_id.as_str(), "job-b"),
        ] {
            let definition = ScheduledTaskDefinition::new(
                project_id,
                job_id,
                "direct",
                ScheduleSpec::at(deadline),
                OverlapPolicy::SkipWhenRunning,
            )
            .unwrap();
            database.create_job(&definition, Some(deadline)).unwrap();
        }
        let runtime = Arc::new(LoopCoordinatorRuntime::new(Utc::now(), 0));
        let (_handle, mut coordinator) = command_loop_coordinator(runtime.clone());
        coordinator.workspaces.insert(
            first_path,
            WorkspaceRegistration {
                app: Arc::new(first_app),
                database: first_db,
            },
        );
        coordinator.workspaces.insert(
            second_path,
            WorkspaceRegistration {
                app: Arc::new(second_app),
                database: second_db,
            },
        );

        coordinator.reconcile_power_state(true);

        assert_eq!(
            runtime.power_reconciliations.lock().unwrap().last(),
            Some(&(2, true))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_shutdown_releases_power_state() {
        let runtime = Arc::new(LoopCoordinatorRuntime::new(Utc::now(), 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());

        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();

        assert_eq!(
            runtime.power_reconciliations.lock().unwrap().last(),
            Some(&(0, false))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn startup_registration_runs_occurrence_retention() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let finished_at = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "retention-job",
            "direct",
            ScheduleSpec::at(finished_at + Duration::days(60)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(finished_at + Duration::days(60)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                finished_at,
                OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();
        database
            .claim_occurrence(
                &occurrence.id,
                "retention-owner",
                finished_at,
                finished_at + Duration::minutes(5),
            )
            .unwrap();
        database
            .finish_occurrence(
                &occurrence.id,
                "retention-owner",
                OccurrenceStatus::Succeeded,
                None,
                None,
            )
            .unwrap();

        let runtime = Arc::new(LoopCoordinatorRuntime::new(
            finished_at + Duration::days(40),
            0,
        ));
        let (_handle, mut coordinator) = command_loop_coordinator(runtime);
        coordinator
            .register_workspace(workspace_path, ReconcileReason::Startup)
            .await
            .unwrap();

        assert!(database.get_occurrence(&occurrence.id).unwrap().is_none());
    }

    async fn drain_pending_guard_joins(pending: &PendingGuardJoins) {
        let joins = {
            let mut pending = pending.lock().unwrap();
            std::mem::take(&mut *pending)
        };
        for join in joins {
            join.await.unwrap();
        }
    }

    async fn advance_command_loop(runtime: &LoopCoordinatorRuntime, duration: StdDuration) {
        runtime.advance_wall(duration);
        tokio::time::advance(duration).await;
        settle_command_loop().await;
    }

    fn coordinator_with_workspace(
        workspace_path: camino::Utf8PathBuf,
        database: ScheduledTaskDatabase,
    ) -> (
        SchedulerCoordinatorHandle,
        SchedulerCoordinator<TestCoordinatorRuntime>,
    ) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = SchedulerCoordinatorHandle::new(sender.clone());
        let mut coordinator = SchedulerCoordinator::new_with_sender(
            std::sync::Arc::new(TestCoordinatorRuntime),
            sender,
            receiver,
        );
        coordinator.workspaces.insert(
            workspace_path.clone(),
            WorkspaceRegistration {
                app: std::sync::Arc::new(gold_band::app::App::new(workspace_path)),
                database,
            },
        );
        (handle, coordinator)
    }

    #[tokio::test]
    async fn resume_attention_returns_the_reclaimed_occurrence_identity() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "job-resume",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();
        database
            .claim_occurrence(
                &occurrence.id,
                "test-owner",
                now,
                now + Duration::minutes(5),
            )
            .unwrap();
        database
            .finish_occurrence(
                &occurrence.id,
                "test-owner",
                OccurrenceStatus::AttentionRequired,
                Some(gold_band::scheduler::occurrence::OccurrenceLinks {
                    task_id: Some("task-1".to_string()),
                    run_id: Some("run-1".to_string()),
                    round_id: Some("round-1".to_string()),
                    attempt_id: Some("attempt-1".to_string()),
                }),
                Some(gold_band::scheduler::occurrence::ScheduledError::new(
                    ScheduledErrorCode::UserInputRequired,
                )),
            )
            .unwrap();
        let (handle, coordinator) =
            coordinator_with_workspace(workspace_path.clone(), database.clone());
        let loop_task = tokio::spawn(coordinator.run());

        let resumed_occurrence_id: Option<String> = handle
            .resume_attention(
                workspace_path,
                "task-1".to_string(),
                "run-1".to_string(),
                "round-1".to_string(),
                "attempt-1".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(
            resumed_occurrence_id.as_deref(),
            Some(occurrence.id.as_str())
        );
        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    fn claimed_occurrence() -> (ScheduledTaskDatabase, String, String) {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let scheduled_at = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(scheduled_at + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(scheduled_at + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                scheduled_at,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
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
    fn occurrence_accept_failure_never_calls_launch() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let launches = AtomicUsize::new(0);
        let links = gold_band::scheduler::occurrence::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            ..Default::default()
        };

        assert!(
            accept_occurrence_links_then(
                &database,
                &occurrence_id,
                &owner_id,
                Utc::now() + Duration::minutes(10),
                &links,
                || {
                    launches.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .is_err()
        );

        assert_eq!(launches.load(Ordering::SeqCst), 0);
        assert_eq!(
            database
                .get_occurrence(&occurrence_id)
                .unwrap()
                .unwrap()
                .links(),
            gold_band::scheduler::occurrence::OccurrenceLinks::default()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accepted_launch_failure_finishes_terminal_and_clears_active_guard() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let active = Arc::new(Mutex::new(HashMap::new()));
        let pending_stops = Arc::new(Mutex::new(Vec::new()));
        let lease = ActiveOccurrenceMetadata {
            database: database.clone(),
            workspace_path: camino::Utf8PathBuf::from("C:/workspace"),
            owner_id: owner_id.clone(),
            project_id: "project-1".to_string(),
            scheduled_task_id: "job-1".to_string(),
            expected_revision: None,
        };
        let mut guard = ClaimToHandoffGuard::new_with_pending(
            active.clone(),
            pending_stops.clone(),
            occurrence_id.clone(),
            lease,
        )
        .unwrap();
        let links = gold_band::scheduler::occurrence::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            ..Default::default()
        };

        guard.stop().await;
        assert!(
            accept_occurrence_links_then(
                &database,
                &occurrence_id,
                &owner_id,
                Utc::now(),
                &links,
                || -> anyhow::Result<()> { anyhow::bail!("injected synchronous launch failure") },
            )
            .is_err()
        );
        guard.disarm();

        let occurrence = database.get_occurrence(&occurrence_id).unwrap().unwrap();
        assert_eq!(occurrence.status, OccurrenceStatus::Failed);
        assert_eq!(
            occurrence.error_code,
            Some(ScheduledErrorCode::ExecutionFailed)
        );
        assert_eq!(occurrence.links(), links);
        assert!(!active.lock().unwrap().contains_key(&occurrence_id));
    }

    #[test]
    fn accepted_occurrence_recovery_finishes_persisted_run_without_duplicate_launch() {
        let directory = tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root);
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "job-1",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let owner_id = "recovery-owner";
        let claimed = match database
            .claim_occurrence(&occurrence.id, owner_id, now, now + Duration::minutes(5))
            .unwrap()
        {
            ClaimResult::Claimed(claimed) => claimed,
            result => panic!("expected claim, got {result:?}"),
        };
        let links = gold_band::scheduler::occurrence::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            round_id: Some("round-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
        };
        assert!(
            database
                .accept_occurrence_links(&claimed.id, owner_id, now, &links)
                .unwrap()
        );
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            task_uuid: None,
            status: RunStatus::Completed,
            outcome: Some(RunOutcome::Success),
            started_at: "2026-08-06T00:00:00Z".to_string(),
            updated_at: "2026-08-06T00:01:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-1".to_string()),
            current_node: Some("node-1".to_string()),
            current_attempt: Some("attempt-1".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        write_json(&app.paths.run_file("task-1", "run-1"), &run).unwrap();

        let accepted = database.get_occurrence(&claimed.id).unwrap().unwrap();
        let recovered = recover_accepted_occurrence(&database, &app, &accepted, owner_id)
            .unwrap()
            .unwrap();

        assert_eq!(recovered.status, OccurrenceStatus::Succeeded);
        assert_eq!(recovered.links(), links);
        assert_eq!(app.run_list("task-1").unwrap().len(), 1);
    }

    #[test]
    fn accepted_running_occurrence_recovery_interrupts_run_and_finishes_lease_lost() {
        let directory = tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root);
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "job-1",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let owner_id = "recovery-owner";
        let claimed = match database
            .claim_occurrence(&occurrence.id, owner_id, now, now + Duration::minutes(5))
            .unwrap()
        {
            ClaimResult::Claimed(claimed) => claimed,
            result => panic!("expected claim, got {result:?}"),
        };
        let links = gold_band::scheduler::occurrence::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            round_id: Some("round-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
        };
        assert!(
            database
                .accept_occurrence_links(&claimed.id, owner_id, now, &links)
                .unwrap()
        );
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-06T00:00:00Z".to_string(),
            updated_at: "2026-08-06T00:01:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-1".to_string()),
            current_node: Some("node-1".to_string()),
            current_attempt: Some("attempt-1".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        write_json(&app.paths.run_file("task-1", "run-1"), &run).unwrap();
        let accepted = database.get_occurrence(&claimed.id).unwrap().unwrap();

        let recovered = recover_accepted_occurrence(&database, &app, &accepted, owner_id)
            .unwrap()
            .unwrap();

        assert_eq!(recovered.status, OccurrenceStatus::Failed);
        assert_eq!(recovered.error_code, Some(ScheduledErrorCode::LeaseLost));
        assert_eq!(recovered.links(), links);
        let interrupted = app.run_status("task-1", "run-1").unwrap();
        assert_eq!(interrupted.status, RunStatus::Paused);
        assert_eq!(
            interrupted.pause_reason,
            Some(gold_band::domain::PauseReason::ProcessInterrupted)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claim_to_handoff_guard_releases_manual_and_scheduled_after_active_probe_error() {
        let directory = tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(root);
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let now = Utc::now();
        let mut definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "claim-guard-job",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.task_id = Some("corrupt-task".to_string());
        let job = database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let corrupt_run = app.paths.run_file("corrupt-task", "corrupt-run");
        std::fs::create_dir_all(corrupt_run.parent().unwrap().as_std_path()).unwrap();
        std::fs::write(corrupt_run.as_std_path(), b"{not-valid-json").unwrap();
        let active = Arc::new(Mutex::new(HashMap::new()));
        let pending_stops = Arc::new(Mutex::new(Vec::new()));

        for (offset, trigger_kind) in [
            OccurrenceTriggerKind::Manual,
            OccurrenceTriggerKind::Scheduled,
        ]
        .into_iter()
        .enumerate()
        {
            let scheduled_at = now + Duration::seconds(offset as i64);
            let occurrence = database
                .create_or_get_occurrence_for_existing_job(
                    &definition.project_id,
                    definition.id(),
                    scheduled_at,
                    trigger_kind,
                )
                .unwrap()
                .unwrap();
            let owner_id = format!("claim-owner-{offset}");
            let claimed = match database
                .claim_occurrence(&occurrence.id, &owner_id, now, now + Duration::minutes(1))
                .unwrap()
            {
                ClaimResult::Claimed(claimed) => claimed,
                result => panic!("expected claim, got {result:?}"),
            };
            let lease = ActiveOccurrenceMetadata {
                database: database.clone(),
                workspace_path: app.paths.repo_root.clone(),
                owner_id: owner_id.clone(),
                project_id: definition.project_id.clone(),
                scheduled_task_id: definition.id.clone(),
                expected_revision: Some(job.revision),
            };

            {
                let _guard = ClaimToHandoffGuard::new_with_pending(
                    active.clone(),
                    pending_stops.clone(),
                    claimed.id.clone(),
                    lease,
                )
                .unwrap();
                assert!(task_has_active_execution(&app, definition.task_id.as_deref()).is_err());
            }

            drain_pending_guard_joins(&pending_stops).await;

            let released = database.get_occurrence(&claimed.id).unwrap().unwrap();
            assert_eq!(released.status, OccurrenceStatus::Retrying);
            assert_eq!(released.owner_id, None);
            assert_eq!(released.lease_until, None);
            assert!(!active.lock().unwrap().contains_key(&claimed.id));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn claim_to_handoff_registry_entry_owns_guard_after_handoff() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let active = Arc::new(Mutex::new(HashMap::new()));
        let pending_stops = Arc::new(Mutex::new(Vec::new()));
        let lease = ActiveOccurrenceMetadata {
            database,
            workspace_path: camino::Utf8PathBuf::from("C:/workspace"),
            owner_id,
            project_id: "project-1".to_string(),
            scheduled_task_id: "job-1".to_string(),
            expected_revision: None,
        };

        let guard = ClaimToHandoffGuard::new_with_pending(
            active.clone(),
            pending_stops.clone(),
            occurrence_id.clone(),
            lease,
        )
        .unwrap();
        guard.handoff();

        {
            let active_state = active.lock().unwrap();
            assert!(active_state.contains_key(&occurrence_id));
            assert!(active_state.get(&occurrence_id).unwrap().guard.is_running());
        }
        shutdown_active_occurrences(&active, &pending_stops, Utc::now())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_drains_owned_occurrences_before_returning() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let active = Arc::new(Mutex::new(HashMap::new()));
        let pending_stops = Arc::new(Mutex::new(Vec::new()));
        let lease = ActiveOccurrenceMetadata {
            database: database.clone(),
            workspace_path: camino::Utf8PathBuf::from("C:/workspace"),
            owner_id,
            project_id: "project-1".to_string(),
            scheduled_task_id: "job-1".to_string(),
            expected_revision: None,
        };
        let guard = ClaimToHandoffGuard::new_with_pending(
            active.clone(),
            pending_stops.clone(),
            occurrence_id.clone(),
            lease,
        )
        .unwrap();
        guard.handoff();

        shutdown_active_occurrences(&active, &pending_stops, Utc::now())
            .await
            .unwrap();

        let occurrence = database.get_occurrence(&occurrence_id).unwrap().unwrap();
        assert!(active.lock().unwrap().is_empty());
        assert!(pending_stops.lock().unwrap().is_empty());
        assert_eq!(occurrence.status, OccurrenceStatus::Retrying);
        assert_eq!(occurrence.owner_id, None);
        assert_eq!(occurrence.lease_until, None);
    }

    #[tokio::test(start_paused = true)]
    async fn lost_lease_cancels_guard_and_records_lease_lost() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let now = Utc::now();
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "lease-loss-job",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let owner_id = "lease-guard-owner";
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, owner_id, now, now - Duration::seconds(1),)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let lost = Arc::new(AtomicUsize::new(0));
        let lost_notify = Arc::new(Notify::new());
        let lost_for_guard = lost.clone();
        let lost_notify_for_guard = lost_notify.clone();
        let lost_notification = lost_notify.notified();
        tokio::pin!(lost_notification);
        let _guard = OccurrenceExecutionGuard::start(
            database.clone(),
            occurrence.id.clone(),
            owner_id.to_string(),
            LeaseConfig {
                lease_seconds: 60,
                heartbeat_seconds: 1,
            },
            move || {
                lost_for_guard.fetch_add(1, Ordering::SeqCst);
                lost_notify_for_guard.notify_one();
            },
        );

        tokio::time::advance(StdDuration::from_secs(1)).await;
        lost_notification.await;

        let occurrence = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(lost.load(Ordering::SeqCst), 1);
        assert_eq!(occurrence.status, OccurrenceStatus::Retrying);
        assert_eq!(occurrence.error_code, Some(ScheduledErrorCode::LeaseLost));
    }

    #[tokio::test(start_paused = true)]
    async fn command_loop_materializes_and_processes_job_created_deadline() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 9, 0, 0).unwrap();
        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());

        handle
            .send(SchedulerCommand::RegisterWorkspace {
                workspace_path: workspace_path.clone(),
            })
            .unwrap();
        settle_command_loop().await;

        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let deadline = base + Duration::minutes(1);
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "command-loop-job",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database.create_job(&definition, Some(deadline)).unwrap();
        handle
            .send(SchedulerCommand::JobCreated {
                key: ScheduledJobKey::new(
                    workspace_path,
                    paths.project_id.clone(),
                    definition.id(),
                ),
            })
            .unwrap();
        settle_command_loop().await;

        advance_command_loop(&runtime, StdDuration::from_secs(59)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 0);
        advance_command_loop(&runtime, StdDuration::from_secs(1)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 1);

        let history = database.list_occurrences(definition.id(), 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, OccurrenceStatus::Succeeded);
        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn command_backlog_does_not_starve_due_deadline() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 9, 15, 0).unwrap();
        let deadline = base + Duration::seconds(1);
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "command-backlog-job",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database.create_job(&definition, Some(deadline)).unwrap();

        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());
        handle
            .send(SchedulerCommand::RegisterWorkspace { workspace_path })
            .unwrap();
        settle_command_loop().await;

        for _ in 0..100 {
            handle.send(SchedulerCommand::SettingsChanged).unwrap();
        }
        advance_command_loop(&runtime, StdDuration::from_secs(1)).await;
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }

        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 1);
        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_process_failure_rearms_and_retries_without_external_signal() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 9, 30, 0).unwrap();
        let deadline = base + Duration::minutes(1);
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "deadline-retry-job",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database.create_job(&definition, Some(deadline)).unwrap();

        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        runtime.fail_next_processes(1);
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());
        handle
            .send(SchedulerCommand::RegisterWorkspace { workspace_path })
            .unwrap();
        settle_command_loop().await;

        advance_command_loop(&runtime, StdDuration::from_secs(60)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 0);
        advance_command_loop(&runtime, StdDuration::from_millis(1_999)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 0);
        advance_command_loop(&runtime, StdDuration::from_millis(1)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 1);
        assert_eq!(
            database
                .list_occurrences(definition.id(), 10)
                .unwrap()
                .first()
                .unwrap()
                .status,
            OccurrenceStatus::Succeeded
        );

        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_job_running_lease_wakes_exactly_at_expiry_and_recovers() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 10, 0, 0).unwrap();
        let mut definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "disabled-running-job",
            "direct",
            ScheduleSpec::at(base + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.enabled = false;
        database.create_job(&definition, None).unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                base,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        database
            .claim_occurrence(
                &occurrence.id,
                "crashed-owner",
                base,
                base + Duration::minutes(1),
            )
            .unwrap();

        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());
        handle
            .send(SchedulerCommand::RegisterWorkspace { workspace_path })
            .unwrap();
        settle_command_loop().await;

        advance_command_loop(&runtime, StdDuration::from_secs(59)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 0);
        advance_command_loop(&runtime, StdDuration::from_secs(1)).await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 1);
        assert_eq!(
            database
                .get_occurrence(&occurrence.id)
                .unwrap()
                .unwrap()
                .status,
            OccurrenceStatus::Succeeded
        );

        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn failed_workspace_registration_retries_once_and_succeeds() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 11, 0, 0).unwrap();
        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 1));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());

        handle
            .send(SchedulerCommand::RegisterWorkspace {
                workspace_path: workspace_path.clone(),
            })
            .unwrap();
        settle_command_loop().await;
        assert_eq!(runtime.registration_attempts.load(Ordering::SeqCst), 1);

        let before_retry = WORKSPACE_REGISTRATION_RETRY_DELAY
            .checked_sub(StdDuration::from_millis(1))
            .unwrap();
        advance_command_loop(&runtime, before_retry).await;
        assert_eq!(runtime.registration_attempts.load(Ordering::SeqCst), 1);
        advance_command_loop(&runtime, StdDuration::from_millis(1)).await;
        assert_eq!(runtime.registration_attempts.load(Ordering::SeqCst), 2);

        handle.shutdown().await.unwrap();
        let coordinator = loop_task.await.unwrap();
        assert!(coordinator.workspaces.contains_key(&workspace_path));
    }

    #[test]
    fn stale_past_deadline_is_rearmed_with_failure_backoff() {
        let now = Utc.with_ymd_and_hms(2026, 8, 12, 9, 0, 0).unwrap();
        let stale = now - Duration::minutes(1);

        assert_eq!(
            super::stale_deadline_retry_at(stale, now),
            now + Duration::from_std(super::DEADLINE_FAILURE_RETRY_DELAY).unwrap()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failed_workspace_refresh_preserves_existing_registration_and_deadline() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 12, 0, 0).unwrap();
        let deadline = base + Duration::hours(1);
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "preserved-job",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database.create_job(&definition, Some(deadline)).unwrap();
        let key = ScheduledJobKey::new(
            workspace_path.clone(),
            paths.project_id.clone(),
            definition.id(),
        );
        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 1));
        let (handle, mut coordinator) = command_loop_coordinator(runtime);
        coordinator.workspaces.insert(
            workspace_path.clone(),
            WorkspaceRegistration {
                app: Arc::new(gold_band::app::App::new(workspace_path.clone())),
                database,
            },
        );
        let recovery = coordinator
            .workspaces
            .get(&workspace_path)
            .unwrap()
            .database
            .get_recoverable_job_for_project(&paths.project_id, definition.id())
            .unwrap()
            .unwrap();
        coordinator
            .register_record(key.clone(), recovery, base)
            .unwrap();
        let loop_task = tokio::spawn(coordinator.run());

        handle
            .send(SchedulerCommand::RegisterWorkspace {
                workspace_path: workspace_path.clone(),
            })
            .unwrap();
        settle_command_loop().await;
        handle.shutdown().await.unwrap();
        let coordinator = loop_task.await.unwrap();

        assert!(coordinator.workspaces.contains_key(&workspace_path));
        assert!(coordinator.registered_deadlines.contains_key(&key));
    }

    #[tokio::test(start_paused = true)]
    async fn reconcile_failure_preserves_old_registration_until_retry_replaces_it() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 12, 30, 0).unwrap();
        let old_deadline = base + Duration::hours(1);
        let old_definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "old-registration-job",
            "direct",
            ScheduleSpec::at(old_deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&old_definition, Some(old_deadline))
            .unwrap();
        let old_key = ScheduledJobKey::new(
            workspace_path.clone(),
            paths.project_id.clone(),
            old_definition.id(),
        );

        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        runtime.fail_next_processes(1);
        let (handle, mut coordinator) = command_loop_coordinator(runtime.clone());
        let old_app = Arc::new(gold_band::app::App::new(workspace_path.clone()));
        coordinator.workspaces.insert(
            workspace_path.clone(),
            WorkspaceRegistration {
                app: old_app.clone(),
                database: database.clone(),
            },
        );
        let recovery = database
            .get_recoverable_job_for_project(&paths.project_id, old_definition.id())
            .unwrap()
            .unwrap();
        coordinator
            .register_record(old_key.clone(), recovery, base)
            .unwrap();

        assert!(
            database
                .delete_job(&paths.project_id, old_definition.id())
                .unwrap()
        );
        let new_deadline = base + Duration::hours(2);
        let new_definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "replacement-job",
            "direct",
            ScheduleSpec::at(new_deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&new_definition, Some(new_deadline))
            .unwrap();
        database
            .create_or_get_occurrence_for_existing_job(
                &new_definition.project_id,
                new_definition.id(),
                base,
                OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();
        let new_key = ScheduledJobKey::new(
            workspace_path.clone(),
            paths.project_id.clone(),
            new_definition.id(),
        );

        assert!(
            coordinator
                .register_workspace_with_retry(workspace_path.clone(), ReconcileReason::Explicit,)
                .await
                .is_err()
        );
        assert!(Arc::ptr_eq(
            &coordinator.workspaces[&workspace_path].app,
            &old_app
        ));
        assert!(coordinator.registered_deadlines.contains_key(&old_key));
        assert!(!coordinator.registered_deadlines.contains_key(&new_key));

        settle_command_loop().await;
        let loop_task = tokio::spawn(coordinator.run());
        advance_command_loop(&runtime, WORKSPACE_REGISTRATION_RETRY_DELAY).await;
        handle.shutdown().await.unwrap();
        let coordinator = loop_task.await.unwrap();

        assert!(!Arc::ptr_eq(
            &coordinator.workspaces[&workspace_path].app,
            &old_app
        ));
        assert!(!coordinator.registered_deadlines.contains_key(&old_key));
        assert!(coordinator.registered_deadlines.contains_key(&new_key));
    }

    #[tokio::test(start_paused = true)]
    async fn wall_clock_jump_triggers_timer_drift_reconcile_without_job_polling() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 13, 0, 0).unwrap();
        let deadline = base + Duration::minutes(10);
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "drift-job",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database.create_job(&definition, Some(deadline)).unwrap();

        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());
        handle
            .send(SchedulerCommand::RegisterWorkspace { workspace_path })
            .unwrap();
        settle_command_loop().await;

        runtime.set_now(base + Duration::minutes(9) + Duration::seconds(50));
        tokio::time::advance(CLOCK_DRIFT_CHECK_INTERVAL).await;
        settle_command_loop().await;
        runtime.set_now(deadline);
        tokio::time::advance(StdDuration::from_secs(10)).await;
        settle_command_loop().await;

        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 1);
        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn failed_timer_drift_reconcile_is_retried_on_the_next_clock_check() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path.clone());
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 8, 7, 9, 0, 0).unwrap();
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "drift-retry-job",
            "direct",
            ScheduleSpec::at(base + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(base + Duration::hours(1)))
            .unwrap();

        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_task = tokio::spawn(coordinator.run());
        handle
            .send(SchedulerCommand::RegisterWorkspace { workspace_path })
            .unwrap();
        settle_command_loop().await;

        database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                base,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        runtime.fail_next_processes(1);

        runtime.set_now(base + Duration::seconds(40));
        tokio::time::advance(CLOCK_DRIFT_CHECK_INTERVAL).await;
        settle_command_loop().await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 0);

        runtime.advance_wall(CLOCK_DRIFT_CHECK_INTERVAL);
        tokio::time::advance(CLOCK_DRIFT_CHECK_INTERVAL).await;
        settle_command_loop().await;
        assert_eq!(runtime.processed_occurrences.load(Ordering::SeqCst), 1);

        handle.shutdown().await.unwrap();
        loop_task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn clock_drift_detector_accumulates_residual_until_tolerance_is_exceeded() {
        let mut wall_now = Utc.with_ymd_and_hms(2026, 8, 6, 13, 30, 0).unwrap();
        let mut detector = ClockDriftDetector::new(wall_now);

        for _ in 0..2 {
            tokio::time::advance(StdDuration::from_secs(30)).await;
            wall_now += Duration::seconds(32);
            assert!(!detector.observe(wall_now));
        }

        tokio::time::advance(StdDuration::from_secs(30)).await;
        wall_now += Duration::seconds(32);
        assert!(detector.observe(wall_now));

        tokio::time::advance(StdDuration::from_secs(30)).await;
        wall_now += Duration::seconds(30);
        assert!(!detector.observe(wall_now));
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_ack_returns_only_after_active_lease_release() {
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 14, 0, 0).unwrap();
        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_completed = Arc::new(AtomicUsize::new(0));
        let completed_for_task = loop_completed.clone();
        let loop_task = tauri::async_runtime::spawn(async move {
            coordinator.run().await;
            completed_for_task.store(1, Ordering::SeqCst);
        });
        handle.install_task(loop_task).unwrap();

        handle.shutdown().await.unwrap();
        assert_eq!(runtime.shutdown_releases.load(Ordering::SeqCst), 1);
        assert_eq!(loop_completed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_returns_release_error_after_joining_coordinator_task() {
        let base = Utc.with_ymd_and_hms(2026, 8, 6, 14, 30, 0).unwrap();
        let runtime = Arc::new(LoopCoordinatorRuntime::new(base, 0));
        runtime.fail_next_releases(1);
        let (handle, coordinator) = command_loop_coordinator(runtime.clone());
        let loop_completed = Arc::new(AtomicUsize::new(0));
        let completed_for_task = loop_completed.clone();
        let loop_task = tauri::async_runtime::spawn(async move {
            coordinator.run().await;
            completed_for_task.store(1, Ordering::SeqCst);
        });
        handle.install_task(loop_task).unwrap();

        let error = handle.shutdown().await.unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::CoordinatorUnavailable);
        assert_eq!(error.params["operation"], "release-active-leases");
        assert_eq!(runtime.shutdown_releases.load(Ordering::SeqCst), 1);
        assert_eq!(loop_completed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_prioritizes_ack_error_when_join_also_fails() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<SchedulerCommand>();
        let handle = SchedulerCoordinatorHandle::new(sender);
        let (ack_sent, ack_received) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            let SchedulerCommand::Shutdown { ack } = receiver
                .recv()
                .await
                .expect("shutdown command must be sent")
            else {
                panic!("expected shutdown command");
            };
            let _ = ack.send(Err(crate::scheduled_service::ScheduledServiceError::new(
                ScheduledErrorCode::CoordinatorUnavailable,
                serde_json::json!({ "operation": "release-active-leases" }),
            )));
            let _ = ack_sent.send(());
            std::future::pending::<()>().await;
        });
        let abort_handle = task.inner().abort_handle();
        handle.install_task(task).unwrap();

        let shutdown = tokio::spawn({
            let handle = handle.clone();
            async move { handle.shutdown().await }
        });
        ack_received.await.unwrap();
        abort_handle.abort();
        let error = shutdown.await.unwrap().unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::CoordinatorUnavailable);
        assert_eq!(error.params["operation"], "release-active-leases");
        assert!(handle.task.lock().unwrap().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn stale_timer_is_a_no_op_after_revision_change() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let first_deadline = Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap();
        let second_deadline = first_deadline + Duration::hours(1);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "job-a",
            "direct",
            ScheduleSpec::at(first_deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let created = database
            .create_job(&definition, Some(first_deadline))
            .unwrap();
        let registered = RegisteredDeadline::from_record(&created).unwrap();
        let mut updated = created.definition.clone();
        updated.schedule = ScheduleSpec::at(second_deadline);
        updated.updated_at += Duration::milliseconds(1);
        assert!(matches!(
            database
                .update_job(
                    &updated,
                    created.definition.updated_at,
                    Some(second_deadline)
                )
                .unwrap(),
            UpdateJobResult::Updated(_)
        ));

        let result = materialize_registered_deadline(
            &database,
            "project-a",
            "job-a",
            registered,
            first_deadline,
        )
        .unwrap();

        assert_eq!(result, DueMaterialization::Stale);
        assert!(database.list_occurrences("job-a", 10).unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn stale_disable_and_delete_commands_refresh_the_final_enabled_state() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let database = ScheduledTaskDatabase::open(
            gold_band::storage::GoldBandPaths::new(workspace_path.clone()).scheduler_db_path(),
        )
        .unwrap();
        let deadline = Utc::now() + Duration::hours(1);
        let mut disabled_definition = ScheduledTaskDefinition::new(
            "project-a",
            "stale-disabled",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let created_disabled = database
            .create_job(&disabled_definition, Some(deadline))
            .unwrap();
        disabled_definition.enabled = false;
        disabled_definition.updated_at += Duration::milliseconds(1);
        let disabled = match database
            .update_job(
                &disabled_definition,
                created_disabled.definition.updated_at,
                None,
            )
            .unwrap()
        {
            UpdateJobResult::Updated(record) => record,
            result => panic!("expected disabled update, got {result:?}"),
        };
        disabled_definition.enabled = true;
        disabled_definition.updated_at += Duration::milliseconds(1);
        assert!(matches!(
            database
                .update_job(
                    &disabled_definition,
                    disabled.definition.updated_at,
                    Some(deadline),
                )
                .unwrap(),
            UpdateJobResult::Updated(_)
        ));

        let mut deleted_definition = ScheduledTaskDefinition::new(
            "project-a",
            "stale-deleted",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&deleted_definition, Some(deadline))
            .unwrap();
        assert!(database.delete_job("project-a", "stale-deleted").unwrap());
        deleted_definition.updated_at += Duration::milliseconds(1);
        database
            .create_job(&deleted_definition, Some(deadline))
            .unwrap();
        let (handle, coordinator) =
            coordinator_with_workspace(workspace_path.clone(), database.clone());
        let loop_task = tokio::spawn(coordinator.run());

        handle
            .send(SchedulerCommand::JobDisabled {
                key: ScheduledJobKey::new(workspace_path.clone(), "project-a", "stale-disabled"),
            })
            .unwrap();
        handle
            .send(SchedulerCommand::JobDeleted {
                key: ScheduledJobKey::new(workspace_path.clone(), "project-a", "stale-deleted"),
            })
            .unwrap();
        handle.shutdown().await.unwrap();

        let coordinator = loop_task.await.unwrap();
        assert!(
            coordinator
                .registered_deadlines
                .contains_key(&ScheduledJobKey::new(
                    workspace_path.clone(),
                    "project-a",
                    "stale-disabled",
                ))
        );
        assert!(
            coordinator
                .registered_deadlines
                .contains_key(&ScheduledJobKey::new(
                    workspace_path,
                    "project-a",
                    "stale-deleted",
                ))
        );
    }

    #[test]
    fn stale_run_now_record_cannot_create_manual_placeholder_after_delete() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let deadline = Utc::now() + Duration::hours(1);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "deleted-run-now-job",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let stale = database.create_job(&definition, Some(deadline)).unwrap();
        assert!(
            database
                .delete_job(&definition.project_id, definition.id())
                .unwrap()
        );

        assert!(
            create_manual_occurrence(
                &database,
                &stale.definition.project_id,
                stale.definition.id(),
            )
            .is_err()
        );
        assert!(
            database
                .get_job_definition(&definition.project_id, definition.id())
                .unwrap()
                .is_none()
        );
        assert!(
            database
                .list_occurrences(definition.id(), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn completed_shared_database_marker_skips_corrupt_source_during_workspace_migration() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let paths = gold_band::storage::GoldBandPaths::new(workspace_path);
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let source = ScheduledTaskDatabase::open(paths.legacy_scheduler_db_path()).unwrap();

        database
            .import_legacy_database_once(&source, &paths.project_id, Utc::now())
            .unwrap();
        drop(source);
        std::fs::write(paths.legacy_scheduler_db_path(), b"not sqlite").unwrap();

        migrate_legacy_scheduler_database(&paths, &database, Utc::now()).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn run_now_command_materializes_manual_occurrence_without_advancing_deadline() {
        let directory = tempdir().unwrap();
        let workspace_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let database = ScheduledTaskDatabase::open(
            gold_band::storage::GoldBandPaths::new(workspace_path.clone()).scheduler_db_path(),
        )
        .unwrap();
        let deadline = Utc::now() + Duration::hours(1);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "job-a",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let before = database.create_job(&definition, Some(deadline)).unwrap();
        let (handle, coordinator) =
            coordinator_with_workspace(workspace_path.clone(), database.clone());
        let loop_task = tokio::spawn(coordinator.run());

        let result = handle
            .run_now(ScheduledJobKey::new(workspace_path, "project-a", "job-a"))
            .await
            .unwrap();
        handle.shutdown().await.unwrap();
        let coordinator = loop_task.await.unwrap();
        let after = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();

        assert_eq!(
            result.occurrence.trigger_kind,
            OccurrenceTriggerKind::Manual
        );
        assert_eq!(after.next_run_at, before.next_run_at);
        assert!(
            coordinator
                .registered_deadlines
                .contains_key(&ScheduledJobKey::new(
                    coordinator.workspaces.keys().next().unwrap().clone(),
                    "project-a",
                    "job-a",
                ))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn manual_run_does_not_replace_scheduled_deadline() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let deadline = Utc::now() + Duration::minutes(1);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "job-a",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let before = database.create_job(&definition, Some(deadline)).unwrap();
        let key = ScheduledJobKey::new(
            camino::Utf8PathBuf::from("C:/workspace"),
            "project-a",
            "job-a",
        );
        let mut registry = DeadlineRegistry::new();
        registry.register_after(key.clone(), StdDuration::from_secs(60));

        create_manual_occurrence(&database, "project-a", "job-a").unwrap();

        let after = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();
        assert_eq!(after.next_run_at, before.next_run_at);
        assert_eq!(after.revision, before.revision);
        assert_eq!(registry.len(), 1);
        tokio::time::advance(StdDuration::from_secs(60)).await;
        assert_eq!(registry.next_expired().await, Some(key));
    }

    #[tokio::test(start_paused = true)]
    async fn reconcile_marks_points_beyond_grace_missed_and_keeps_near_late_point() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap();
        let old_deadline = now - Duration::minutes(2);
        let near_late_deadline = now - Duration::minutes(1);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "job-a",
            "direct",
            ScheduleSpec::every(1, "minutes", old_deadline).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(old_deadline))
            .unwrap();

        let reconciled = reconcile_missed_deadlines(
            &database,
            &ScheduledJobKey::new(
                camino::Utf8PathBuf::from("C:/workspace"),
                "project-a",
                "job-a",
            ),
            now,
        )
        .unwrap();
        assert_eq!(reconciled.missed_count, 1);
        let reconciled = reconciled.record.unwrap();

        assert_eq!(reconciled.next_run_at, Some(near_late_deadline));
        let occurrences = database.list_occurrences("job-a", 10).unwrap();
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].scheduled_at, old_deadline);
        assert_eq!(occurrences[0].status, OccurrenceStatus::Missed);
        assert!(matches!(
            materialize_registered_deadline(
                &database,
                "project-a",
                "job-a",
                RegisteredDeadline::from_record(&reconciled).unwrap(),
                now,
            )
            .unwrap(),
            DueMaterialization::Ready { occurrence, .. }
                if occurrence.scheduled_at == near_late_deadline
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn reconcile_missed_deadlines_returns_before_exhausting_a_large_backlog() {
        const BACKLOG_POINTS: i64 = 600;

        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap();
        let first_deadline = now - LATE_FIRE_GRACE - Duration::minutes(BACKLOG_POINTS);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "bounded-missed-reconcile",
            "direct",
            ScheduleSpec::every(1, "minutes", first_deadline).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(first_deadline))
            .unwrap();

        let reconciled = reconcile_missed_deadlines(
            &database,
            &ScheduledJobKey::new(
                camino::Utf8PathBuf::from("C:/workspace"),
                "project-a",
                definition.id(),
            ),
            now,
        )
        .unwrap();
        assert_eq!(reconciled.missed_count, MISSED_RECONCILE_BATCH_SIZE as u32);
        let reconciled = reconciled.record.unwrap();
        let occurrences = database.list_occurrences(definition.id(), 1_000).unwrap();

        assert!(!occurrences.is_empty());
        assert!(occurrences.len() < BACKLOG_POINTS as usize);
        assert!(reconciled.next_run_at.unwrap() < now - LATE_FIRE_GRACE);
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
                project_id: "project-1".to_string(),
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
                project_id: "project-1".to_string(),
                task_id: "task-1".to_string(),
                run_id: "run-1".to_string(),
                round_id: "round-1".to_string(),
                node_id: "node-1".to_string(),
                attempt_id: "attempt-1".to_string(),
                turn_id: "turn-1".to_string(),
                agent_label: "agent".to_string(),
                outcome: gold_band::app::AcpTurnOutcome::Failed,
                batch_progress: gold_band::app::AcpTurnBatchProgress::terminal(1),
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
    fn scheduled_run_paused_keeps_occurrence_claimed_until_intervention() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        assert!(
            finish_occurrence_for_event(
                &database,
                &occurrence_id,
                &owner_id,
                &RuntimeLifecycleEvent::RunPaused {
                    event_id: "pause-1".to_string(),
                    occurred_at: "2026-08-03T12:01:00Z".to_string(),
                    scheduled_occurrence_id: Some(occurrence_id.clone()),
                    project_id: "project-1".to_string(),
                    task_id: "task-1".to_string(),
                    run_id: "run-1".to_string(),
                    round_id: "round-1".to_string(),
                    node_id: "node-1".to_string(),
                    attempt_id: "attempt-1".to_string(),
                    node_label: "node".to_string(),
                    pause_reason: PauseReason::ProcessInterrupted,
                    task_title: None,
                },
            )
            .unwrap()
            .is_none()
        );
        let paused_occurrence = database.get_occurrence(&occurrence_id).unwrap().unwrap();
        assert_eq!(paused_occurrence.status, OccurrenceStatus::Running);
        assert_eq!(
            paused_occurrence.owner_id.as_deref(),
            Some(owner_id.as_str())
        );

        finish_occurrence_for_event(
            &database,
            &occurrence_id,
            &owner_id,
            &RuntimeLifecycleEvent::InterventionRequested {
                event_id: "intervention-1".to_string(),
                occurred_at: "2026-08-03T12:01:01Z".to_string(),
                scheduled_occurrence_id: Some(occurrence_id.clone()),
                project_id: "project-1".to_string(),
                task_id: "task-1".to_string(),
                run_id: "run-1".to_string(),
                round_id: "round-1".to_string(),
                node_id: "node-1".to_string(),
                attempt_id: "attempt-1".to_string(),
                node_label: "node".to_string(),
                kind: RuntimeInterventionKind::ProcessInterrupted,
                task_title: None,
            },
        )
        .unwrap();
        let finished_occurrence = database.get_occurrence(&occurrence_id).unwrap().unwrap();
        assert_eq!(finished_occurrence.status, OccurrenceStatus::Failed);
        assert!(finished_occurrence.owner_id.is_none());
    }

    #[test]
    fn scheduler_treats_prompt_activity_as_busy_after_run_completion() {
        let directory = tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root);
        let task_id = "task-1";
        let run_id = "run-1";
        let run = RunState {
            version: VERSION.to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Completed,
            outcome: Some(RunOutcome::Success),
            started_at: "2026-08-03T12:00:00Z".to_string(),
            updated_at: "2026-08-03T12:01:00Z".to_string(),
            workflow_snapshot: "workflow.json".to_string(),
            current_round: Some("round-1".to_string()),
            current_node: Some("node-1".to_string()),
            current_attempt: Some("attempt-1".to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        let run_file = app.paths.run_file(task_id, run_id);
        std::fs::create_dir_all(run_file.parent().unwrap().as_std_path()).unwrap();
        write_json(&run_file, &run).unwrap();

        assert!(
            task_has_active_execution_with_prompt_probe(&app, Some(task_id), |_| true).unwrap()
        );
    }

    #[test]
    fn scheduler_scans_dynamic_attempt_directories_for_prompt_activity() {
        let directory = tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let dynamic_attempt = root.join("dynamic/nodes/node-1/attempt-001");
        std::fs::create_dir_all(dynamic_attempt.as_std_path()).unwrap();

        assert!(attempt_tree_has_active_prompt(&root, &|attempt_dir| {
            attempt_dir.file_name() == Some("attempt-001")
        }));
    }

    #[test]
    fn every_active_state_is_classified_by_decide_queue() {
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 15, 0, 0).unwrap();
        let cases = [
            (RunStatus::Running, None, false, ActiveExecution::Running),
            (
                RunStatus::Paused,
                Some(PauseReason::PermissionRequested),
                false,
                ActiveExecution::PermissionWaiting,
            ),
            (
                RunStatus::Paused,
                Some(PauseReason::WaitingForUserInput),
                false,
                ActiveExecution::WaitingForUserInput,
            ),
            (
                RunStatus::Paused,
                Some(PauseReason::ProcessInterrupted),
                false,
                ActiveExecution::ResumablePaused,
            ),
            (RunStatus::Completed, None, false, ActiveExecution::Idle),
        ];

        for (status, pause_reason, has_active_prompt, expected) in cases {
            let active = active_execution_for_run(status, pause_reason, has_active_prompt);
            assert_eq!(active, expected);
            assert_eq!(
                decide_queue(OverlapPolicy::SkipWhenRunning, active, 0, now),
                if active == ActiveExecution::Idle {
                    QueueDecision::StartNow
                } else {
                    QueueDecision::Skipped
                }
            );
        }
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
                project_id: "project-1".to_string(),
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
    fn resumed_attention_occurrence_projects_running_update_event() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let now = Utc::now();
        database
            .finish_occurrence(
                &occurrence_id,
                &owner_id,
                OccurrenceStatus::AttentionRequired,
                Some(gold_band::scheduler::occurrence::OccurrenceLinks {
                    task_id: Some("task-1".to_string()),
                    run_id: Some("run-1".to_string()),
                    round_id: Some("round-1".to_string()),
                    attempt_id: Some("attempt-1".to_string()),
                }),
                Some(gold_band::scheduler::occurrence::ScheduledError::new(
                    ScheduledErrorCode::UserInputRequired,
                )),
            )
            .unwrap();
        let resumed = match database
            .resume_attention_occurrence(
                &occurrence_id,
                "resume-owner",
                now,
                now + Duration::minutes(5),
            )
            .unwrap()
        {
            ClaimResult::Claimed(value) => value,
            other => panic!("expected resumed occurrence, got {other:?}"),
        };

        let event = scheduled_occurrence_updated_event("project-1", "job-1", &resumed);

        assert_eq!(event.occurrence_id, occurrence_id);
        assert_eq!(event.status, "running");
        assert_eq!(event.error_code, None);
        assert_eq!(event.task_id.as_deref(), Some("task-1"));
        assert_eq!(event.run_id.as_deref(), Some("run-1"));
    }

    #[test]
    fn resumed_attention_occurrence_clears_attention_definition_projection() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let now = Utc::now();
        database
            .finish_occurrence(
                &occurrence_id,
                &owner_id,
                OccurrenceStatus::AttentionRequired,
                Some(gold_band::scheduler::occurrence::OccurrenceLinks {
                    task_id: Some("task-1".to_string()),
                    run_id: Some("run-1".to_string()),
                    round_id: Some("round-1".to_string()),
                    attempt_id: Some("attempt-1".to_string()),
                }),
                Some(gold_band::scheduler::occurrence::ScheduledError::new(
                    ScheduledErrorCode::UserInputRequired,
                )),
            )
            .unwrap();
        let resumed = match database
            .resume_attention_occurrence(
                &occurrence_id,
                "resume-owner",
                now,
                now + Duration::minutes(5),
            )
            .unwrap()
        {
            ClaimResult::Claimed(value) => value,
            other => panic!("expected resumed occurrence, got {other:?}"),
        };
        let mut definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(now + Duration::hours(1)),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.last_trigger_status = Some("attention_required".to_string());
        definition.last_error = Some(ScheduledErrorCode::UserInputRequired.to_string());
        definition.retry_count = 2;
        definition.retry_at = Some(now + Duration::minutes(1));

        project_resumed_attention(&mut definition, &resumed, now);

        assert_eq!(definition.last_trigger_status.as_deref(), Some("running"));
        assert_eq!(definition.last_trigger_at, Some(resumed.scheduled_at));
        assert_eq!(definition.last_error, None);
        assert_eq!(definition.retry_count, 0);
        assert_eq!(definition.retry_at, None);
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
        database.create_job(&definition, Some(past)).unwrap();
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
        database.create_job(&definition, Some(next)).unwrap();
        let occurrence = create_manual_occurrence(&database, "project-1", "job-1").unwrap();
        assert_eq!(occurrence.trigger_kind, OccurrenceTriggerKind::Manual);
        assert_eq!(definition.next_due(next - Duration::hours(1)), before);
    }

    #[test]
    fn scheduled_direct_rejects_agent_without_full_auto_mode() {
        let directory = tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root).with_provider_diagnostics_source(
            std::sync::Arc::new(|| {
                Ok([(
                    "claude-acp".to_string(),
                    ProviderDiagnosticSnapshot {
                        available: true,
                        reason: None,
                        checked_at: "2026-08-03T00:00:00Z".to_string(),
                        capabilities: Some(serde_json::json!({
                            "configOptions": [{
                                "id": "mode",
                                "category": "mode",
                                "options": [{ "value": "ask", "name": "Ask" }]
                            }]
                        })),
                    },
                )]
                .into_iter()
                .collect())
            }),
        );
        let mut definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 4, 10, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.content_snapshot.direct_agent_id = Some("claude-acp".to_string());
        definition.execution_config = serde_json::json!({
            "directConfig": { "agentType": "claude-acp", "permissionMode": "ask" }
        });
        let error = scheduled_agent_unattended_error(&app, &definition).unwrap();
        assert_eq!(
            error.code,
            ScheduledErrorCode::AgentUnattendedModeUnsupported
        );
    }

    #[test]
    fn scheduled_direct_accepts_supported_full_auto_mode() {
        let directory = tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root).with_provider_diagnostics_source(
            std::sync::Arc::new(|| {
                Ok([(
                    "claude-acp".to_string(),
                    ProviderDiagnosticSnapshot {
                        available: true,
                        reason: None,
                        checked_at: "2026-08-03T00:00:00Z".to_string(),
                        capabilities: Some(serde_json::json!({
                            "configOptions": [{
                                "id": "mode",
                                "category": "mode",
                                "options": [{ "value": "bypassPermissions", "name": "Bypass" }]
                            }]
                        })),
                    },
                )]
                .into_iter()
                .collect())
            }),
        );
        let mut definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 4, 10, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.content_snapshot.direct_agent_id = Some("claude-acp".to_string());
        definition.execution_config = serde_json::json!({
            "directConfig": { "agentType": "claude-acp" }
        });
        assert!(scheduled_agent_unattended_error(&app, &definition).is_none());
    }

    #[test]
    fn scheduled_direct_accepts_codex_agent_full_access_mode() {
        let directory = tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root).with_provider_diagnostics_source(
            std::sync::Arc::new(|| {
                Ok([(
                    "codex-acp".to_string(),
                    ProviderDiagnosticSnapshot {
                        available: true,
                        reason: None,
                        checked_at: "2026-08-05T00:00:00Z".to_string(),
                        capabilities: Some(serde_json::json!({
                            "configOptions": [{
                                "id": "mode",
                                "category": "mode",
                                "options": [
                                    { "value": "read-only", "name": "Read-only" },
                                    { "value": "agent", "name": "Agent" },
                                    { "value": "agent-full-access", "name": "Agent (full access)" }
                                ]
                            }]
                        })),
                    },
                )]
                .into_iter()
                .collect())
            }),
        );
        let mut definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-codex",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 5, 10, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.content_snapshot.direct_agent_id = Some("codex-acp".to_string());
        definition.execution_config = serde_json::json!({
            "directConfig": { "agentType": "codex-acp" }
        });
        assert!(scheduled_agent_unattended_error(&app, &definition).is_none());
    }

    #[test]
    fn scheduler_rejects_a_definition_for_a_different_workspace() {
        let first_dir = tempdir().unwrap();
        let second_dir = tempdir().unwrap();
        let first_root =
            camino::Utf8PathBuf::from_path_buf(first_dir.path().to_path_buf()).unwrap();
        let second_root =
            camino::Utf8PathBuf::from_path_buf(second_dir.path().to_path_buf()).unwrap();
        let first_app = gold_band::app::App::new(first_root);
        let second_app = gold_band::app::App::new(second_root);
        let definition = ScheduledTaskDefinition::new(
            &first_app.paths.project_id,
            "job-1",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 4, 10, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();

        assert!(ensure_definition_workspace(&second_app, &definition).is_err());
    }

    #[test]
    fn successful_runtime_projection_notifies_listeners() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let mut definition = ScheduledTaskDefinition::new(
            "project-1",
            "job-1",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 4, 10, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let created = database.create_job(&definition, None).unwrap();
        definition.task_id = Some("task-1".to_string());
        let mut notified_task_id = None;

        let updated =
            persist_runtime_projection(&database, &definition, created.revision, |updated| {
                notified_task_id = updated.definition.task_id.clone();
            })
            .unwrap();

        assert!(updated.is_some());
        assert_eq!(notified_task_id.as_deref(), Some("task-1"));
        assert_eq!(
            database.list_job_definitions().unwrap()[0]
                .task_id
                .as_deref(),
            Some("task-1")
        );
    }

    #[test]
    fn stale_runtime_projection_does_not_overwrite_concurrent_authoring() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let deadline = Utc.with_ymd_and_hms(2026, 8, 6, 15, 0, 0).unwrap();
        let definition = ScheduledTaskDefinition::new(
            "project-1",
            "stale-projection",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let created = database.create_job(&definition, Some(deadline)).unwrap();
        let mut authoring = created.definition.clone();
        authoring.instruction = "new authoring instruction".to_string();
        authoring.updated_at += Duration::milliseconds(1);
        assert!(matches!(
            database
                .update_job(&authoring, created.definition.updated_at, Some(deadline),)
                .unwrap(),
            UpdateJobResult::Updated(_)
        ));

        let mut stale_projection = created.definition;
        stale_projection.task_id = Some("stale-task".to_string());
        let result =
            persist_runtime_projection(&database, &stale_projection, created.revision, |_| {
                panic!("stale projection must not notify")
            })
            .unwrap();

        assert!(result.is_none());
        let current = database
            .get_job_definition("project-1", "stale-projection")
            .unwrap()
            .unwrap();
        assert_eq!(current.definition.instruction, "new authoring instruction");
        assert_eq!(current.definition.task_id, None);
    }

    #[test]
    fn deleted_job_is_not_recreated_by_stale_runtime_projection() {
        let directory = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(directory.path().join("scheduler.db")).unwrap();
        let deadline = Utc.with_ymd_and_hms(2026, 8, 6, 16, 0, 0).unwrap();
        let definition = ScheduledTaskDefinition::new(
            "project-1",
            "deleted-projection",
            "direct",
            ScheduleSpec::at(deadline),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let created = database.create_job(&definition, Some(deadline)).unwrap();
        assert!(
            database
                .delete_job("project-1", "deleted-projection")
                .unwrap()
        );
        let mut stale_projection = created.definition;
        stale_projection.task_id = Some("stale-task".to_string());

        let result =
            persist_runtime_projection(&database, &stale_projection, created.revision, |_| {
                panic!("deleted projection must not notify")
            })
            .unwrap();

        assert!(result.is_none());
        assert!(
            database
                .get_job_definition("project-1", "deleted-projection")
                .unwrap()
                .is_none()
        );
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
    fn direct_content_fingerprint_change_keeps_the_existing_task_chain() {
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-direct-continuous-content-change",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap()
        .with_session_policy(SessionPolicy::Continuous)
        .unwrap();
        definition.task_id = Some("task-001".to_string());
        definition.content_fingerprint = "sha256:new".to_string();
        assert_eq!(
            scheduled_execution_action_for_fingerprint(&definition, Some("sha256:old")),
            ScheduledExecutionAction::ContinueSession {
                task_id: "task-001".to_string(),
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

    fn reconcile_outcome_setup(
        run: Option<RunState>,
    ) -> (
        gold_band::app::App,
        gold_band::scheduler::occurrence::ScheduledOccurrence,
        tempfile::TempDir,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = gold_band::app::App::new(repo_root);
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let now = Utc::now();
        let definition = gold_band::scheduler::ScheduledTaskDefinition::new(
            "project-a",
            "job-reconcile",
            "direct",
            gold_band::scheduler::ScheduleSpec::every(1, "hours", now).unwrap(),
            gold_band::scheduler::OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database.create_job(&definition, None).unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &definition.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let owner = "reconcile-owner";
        let claimed = match database
            .claim_occurrence(&occurrence.id, owner, now, now + Duration::minutes(5))
            .unwrap()
        {
            ClaimResult::Claimed(claimed) => claimed,
            result => panic!("expected claim, got {result:?}"),
        };
        let links = gold_band::scheduler::occurrence::OccurrenceLinks {
            task_id: Some("task-reconcile".to_string()),
            run_id: Some("run-reconcile".to_string()),
            round_id: Some("round-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
        };
        assert!(
            database
                .accept_occurrence_links(&claimed.id, owner, now, &links)
                .unwrap()
        );
        if let Some(run) = run {
            write_json(&app.paths.run_file("task-reconcile", "run-reconcile"), &run).unwrap();
        }
        let occurrence = database.get_occurrence(&claimed.id).unwrap().unwrap();
        (app, occurrence, directory)
    }

    #[test]
    fn reconcile_running_occurrence_finalizes_when_underlying_run_completed() {
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-reconcile".to_string(),
            task_id: "task-reconcile".to_string(),
            task_uuid: None,
            status: RunStatus::Completed,
            outcome: Some(RunOutcome::Success),
            started_at: "2026-08-12T00:00:00Z".to_string(),
            updated_at: "2026-08-12T00:01:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::Terminal,
                None,
                "2026-08-12T00:01:00Z",
            ),
        };
        let (app, occurrence, _dir) = reconcile_outcome_setup(Some(run));

        // Task/Run 已 Completed（不再 active）：对账应给出 Succeeded 终态。
        let outcome = super::reconcile_running_occurrence_outcome(&app, &occurrence).unwrap();
        assert_eq!(
            outcome.map(|(status, _)| status),
            Some(OccurrenceStatus::Succeeded)
        );
    }

    #[test]
    fn reconcile_running_occurrence_finalizes_when_underlying_run_missing() {
        // 不写 run.json：Task/Run 不存在，对账应给出 Failed 终态。
        let (app, occurrence, _dir) = reconcile_outcome_setup(None);

        let outcome = super::reconcile_running_occurrence_outcome(&app, &occurrence).unwrap();
        assert_eq!(
            outcome.map(|(status, error)| (status, error.map(|e| e.code))),
            Some((
                OccurrenceStatus::Failed,
                Some(ScheduledErrorCode::LeaseLost)
            ))
        );
    }

    #[test]
    fn reconcile_running_occurrence_preserves_when_underlying_run_still_active() {
        let run = RunState {
            version: VERSION.to_string(),
            id: "run-reconcile".to_string(),
            task_id: "task-reconcile".to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-12T00:00:00Z".to_string(),
            updated_at: "2026-08-12T00:01:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::RunningNode,
                None,
                "2026-08-12T00:01:00Z",
            ),
        };
        let (app, occurrence, _dir) = reconcile_outcome_setup(Some(run));

        // Task/Run 仍 Running（active）：对账应保留（返回 None），不误杀。
        let outcome = super::reconcile_running_occurrence_outcome(&app, &occurrence).unwrap();
        assert!(
            outcome.is_none(),
            "active run must be preserved, got {outcome:?}"
        );
    }

    #[test]
    fn reconcile_running_occurrence_ignores_a_newer_active_run_on_the_same_task() {
        let completed = RunState {
            version: VERSION.to_string(),
            id: "run-reconcile".to_string(),
            task_id: "task-reconcile".to_string(),
            task_uuid: None,
            status: RunStatus::Completed,
            outcome: Some(RunOutcome::Success),
            started_at: "2026-08-12T00:00:00Z".to_string(),
            updated_at: "2026-08-12T00:01:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::Terminal,
                None,
                "2026-08-12T00:01:00Z",
            ),
        };
        let (app, occurrence, _dir) = reconcile_outcome_setup(Some(completed));
        let newer_active = RunState {
            version: VERSION.to_string(),
            id: "run-newer".to_string(),
            task_id: "task-reconcile".to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-12T00:02:00Z".to_string(),
            updated_at: "2026-08-12T00:03:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState::new(
                RuntimeExecutionPhase::RunningNode,
                None,
                "2026-08-12T00:03:00Z",
            ),
        };
        write_json(
            &app.paths.run_file("task-reconcile", "run-newer"),
            &newer_active,
        )
        .unwrap();

        let outcome = super::reconcile_running_occurrence_outcome(&app, &occurrence).unwrap();

        assert_eq!(
            outcome.map(|(status, _)| status),
            Some(OccurrenceStatus::Succeeded),
            "another active run under the reused task must not keep this occurrence running"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconciled_terminal_occurrence_stops_and_removes_its_active_guard() {
        let (database, occurrence_id, owner_id) = claimed_occurrence();
        let active = Arc::new(Mutex::new(HashMap::new()));
        let pending_stops = Arc::new(Mutex::new(Vec::new()));
        let lease = ActiveOccurrenceMetadata {
            database: database.clone(),
            workspace_path: camino::Utf8PathBuf::from("C:/workspace"),
            owner_id: owner_id.clone(),
            project_id: "project-1".to_string(),
            scheduled_task_id: "job-1".to_string(),
            expected_revision: None,
        };
        let guard = ClaimToHandoffGuard::new_with_pending(
            active.clone(),
            pending_stops,
            occurrence_id.clone(),
            lease,
        )
        .unwrap();
        guard.handoff();
        assert!(active.lock().unwrap().contains_key(&occurrence_id));

        assert!(
            finish_reconciled_occurrence(
                &database,
                &active,
                &occurrence_id,
                &owner_id,
                OccurrenceStatus::Succeeded,
                None,
            )
            .await
            .unwrap()
            .is_some()
        );

        assert!(!active.lock().unwrap().contains_key(&occurrence_id));
        assert_eq!(
            database
                .get_occurrence(&occurrence_id)
                .unwrap()
                .unwrap()
                .status,
            OccurrenceStatus::Succeeded
        );
    }
}
