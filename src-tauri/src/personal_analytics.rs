use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use camino::{Utf8Path, Utf8PathBuf};
use gold_band::acp::client::{self, AcpRuntimePolicy};
use gold_band::acp::events::{
    AcpLifecycleOwner, AcpPromptSubmission, AcpTurnExecutionClaim, admit_session_turn_for_execution,
};
use gold_band::artifacts::json_artifact_text;
use gold_band::config::ManagedAgentId;
use gold_band::domain::{SessionMode, TurnControlMode};
use gold_band::personal_analytics::{
    AgentInsightOperation, AgentInsightOperationStatus, AgentInsightProgress,
    PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION, PersonalAnalyticsError, PersonalAnalyticsNarrative,
    PersonalAnalyticsOperation, PersonalAnalyticsOperationStatus, PersonalAnalyticsProgress,
    PersonalAnalyticsProjection, PersonalAnalyticsReport, PersonalAnalyticsSemanticBatch,
    PersonalAnalyticsSemanticItem, PersonalAnalyticsSnapshot,
    canonicalize_personal_analytics_report, index::InsightIdentity,
    index::PersonalAnalyticsDateRange, index::PersonalAnalyticsIndex,
    personal_analytics_narrative_schema,
};
use gold_band::prompts::{
    PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN, PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN,
    PERSONAL_ANALYTICS_REPAIR_USER_EN, PERSONAL_ANALYTICS_REPAIR_USER_ZH_CN,
    PERSONAL_ANALYTICS_SYSTEM_EN, PERSONAL_ANALYTICS_SYSTEM_ZH_CN, PERSONAL_ANALYTICS_USER_EN,
    PERSONAL_ANALYTICS_USER_ZH_CN, prompt_by_language, render,
};
use gold_band::provider::{
    AttachmentProjectionPolicy, ConversationPromptInput, PromptBundle, PromptVisibility,
    RuntimeControlIntent, resolve_attachments,
};
use gold_band::storage::{read_json, write_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::state::DesktopState;

pub const PERSONAL_ANALYTICS_UPDATED_EVENT: &str = "gold-band://personal-analytics-updated";

type CommandResult<T> = Result<T, PersonalAnalyticsError>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelPersonalAnalyticsInput {
    pub operation_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryPersonalAnalyticsReportInput {
    pub range: PersonalAnalyticsDateRange,
    pub agent_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartPersonalAnalyticsInsightsInput {
    pub agent_type: String,
    pub range: PersonalAnalyticsDateRange,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContentManifest {
    schema_version: String,
    authorized_locators: Vec<String>,
    excluded_sources: Vec<String>,
}

#[derive(Default)]
struct RuntimeInner {
    snapshot: Option<PersonalAnalyticsSnapshot>,
    cancellation: Option<CancellationState>,
}

struct CancellationState {
    operation_id: String,
    requested: Arc<AtomicBool>,
    attempt_dir: Utf8PathBuf,
}

#[derive(Clone, Default)]
pub struct PersonalAnalyticsRuntime {
    inner: Arc<Mutex<RuntimeInner>>,
}

#[derive(Default)]
struct InsightRuntimeInner {
    operation: Option<AgentInsightOperation>,
    cancellation: Option<CancellationState>,
}

#[derive(Clone, Default)]
pub struct PersonalAnalyticsInsightRuntime {
    inner: Arc<Mutex<InsightRuntimeInner>>,
}

fn operation_transition_allowed(
    current: PersonalAnalyticsOperationStatus,
    next: PersonalAnalyticsOperationStatus,
) -> bool {
    current != PersonalAnalyticsOperationStatus::Cancelling
        || matches!(
            next,
            PersonalAnalyticsOperationStatus::Cancelling
                | PersonalAnalyticsOperationStatus::Cancelled
        )
}

fn insight_transition_allowed(
    current: AgentInsightOperationStatus,
    next: AgentInsightOperationStatus,
) -> bool {
    use AgentInsightOperationStatus as Status;
    current == next
        || matches!(
            (current, next),
            (
                Status::Queued,
                Status::Analyzing | Status::Cancelling | Status::Failed
            ) | (
                Status::Analyzing,
                Status::ValidatingReport | Status::Cancelling | Status::Failed
            ) | (
                Status::ValidatingReport,
                Status::Analyzing | Status::Completed | Status::Cancelling | Status::Failed
            ) | (Status::Cancelling, Status::Cancelled)
        )
}

impl PersonalAnalyticsRuntime {
    fn snapshot(&self, analytics_root: &Utf8Path) -> CommandResult<PersonalAnalyticsSnapshot> {
        let mut guard = self.lock()?;
        if guard.snapshot.is_none() {
            guard.snapshot = Some(load_snapshot(analytics_root));
        }
        Ok(guard.snapshot.clone().unwrap_or_default())
    }

    fn begin(
        &self,
        analytics_root: &Utf8Path,
        operation: PersonalAnalyticsOperation,
        attempt_dir: Utf8PathBuf,
    ) -> CommandResult<(PersonalAnalyticsSnapshot, Arc<AtomicBool>)> {
        let mut guard = self.lock()?;
        if guard.snapshot.is_none() {
            guard.snapshot = Some(load_snapshot(analytics_root));
        }
        if let Some(existing) = guard
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.operation.as_ref())
            .filter(|operation| operation.status.is_active())
        {
            return Err(analytics_error(
                "analytics.operation-conflict",
                json!({ "operationId": existing.operation_id }),
            ));
        }
        let requested = Arc::new(AtomicBool::new(false));
        let mut persisted = guard.snapshot.clone().unwrap_or_default();
        persisted.operation = Some(operation.clone());
        persist_snapshot(analytics_root, &persisted)?;
        guard.snapshot = Some(persisted.clone());
        guard.cancellation = Some(CancellationState {
            operation_id: operation.operation_id,
            requested: requested.clone(),
            attempt_dir,
        });
        Ok((persisted, requested))
    }

    fn transition(
        &self,
        analytics_root: &Utf8Path,
        operation_id: &str,
        update: impl FnOnce(&mut PersonalAnalyticsOperation, &mut PersonalAnalyticsSnapshot),
    ) -> CommandResult<Option<PersonalAnalyticsSnapshot>> {
        let mut guard = self.lock()?;
        let Some(mut snapshot) = guard.snapshot.clone() else {
            return Ok(None);
        };
        let Some(mut operation) = snapshot.operation.clone() else {
            return Ok(None);
        };
        if operation.operation_id != operation_id || !operation.status.is_active() {
            return Ok(None);
        }
        let current_status = operation.status;
        update(&mut operation, &mut snapshot);
        if !operation_transition_allowed(current_status, operation.status) {
            return Ok(None);
        }
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        let terminal = !operation.status.is_active();
        snapshot.operation = Some(operation);
        let persisted = snapshot.clone();
        persist_snapshot(analytics_root, &persisted)?;
        guard.snapshot = Some(persisted.clone());
        if terminal {
            guard.cancellation = None;
        }
        Ok(Some(persisted))
    }

    fn request_cancel(
        &self,
        analytics_root: &Utf8Path,
        operation_id: &str,
    ) -> CommandResult<(PersonalAnalyticsSnapshot, Option<Utf8PathBuf>)> {
        let (attempt_dir, requested, already_terminal) = {
            let mut guard = self.lock()?;
            if guard.snapshot.is_none() {
                guard.snapshot = Some(load_snapshot(analytics_root));
            }
            let Some(operation) = guard
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.operation.as_ref())
            else {
                return Err(analytics_error(
                    "analytics.operation-not-found",
                    json!({ "operationId": operation_id }),
                ));
            };
            if operation.operation_id != operation_id {
                return Err(analytics_error(
                    "analytics.operation-not-found",
                    json!({ "operationId": operation_id }),
                ));
            }
            let already_terminal = !operation.status.is_active();
            let attempt_dir = guard
                .cancellation
                .as_ref()
                .filter(|state| state.operation_id == operation_id)
                .map(|state| state.attempt_dir.clone());
            let requested = guard
                .cancellation
                .as_ref()
                .filter(|state| state.operation_id == operation_id)
                .map(|state| state.requested.clone());
            (attempt_dir, requested, already_terminal)
        };
        if already_terminal {
            return Ok((self.snapshot(analytics_root)?, None));
        }
        let snapshot = self
            .transition(analytics_root, operation_id, |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Cancelling;
                operation.progress.stage = PersonalAnalyticsOperationStatus::Cancelling;
            })?
            .unwrap_or(self.snapshot(analytics_root)?);
        let cancellation_accepted = snapshot.operation.as_ref().is_some_and(|operation| {
            operation.status == PersonalAnalyticsOperationStatus::Cancelling
        });
        if cancellation_accepted {
            if let Some(requested) = requested {
                requested.store(true, Ordering::Release);
            }
        }
        Ok((
            snapshot,
            cancellation_accepted.then_some(attempt_dir).flatten(),
        ))
    }

    fn lock(&self) -> CommandResult<std::sync::MutexGuard<'_, RuntimeInner>> {
        self.inner.lock().map_err(|_| {
            analytics_error(
                "analytics.state-unavailable",
                json!({ "reason": "poisoned" }),
            )
        })
    }
}

impl PersonalAnalyticsInsightRuntime {
    fn operation(
        &self,
        root: &Utf8Path,
        database_path: &Utf8Path,
    ) -> CommandResult<Option<AgentInsightOperation>> {
        let mut guard = self.lock()?;
        if guard.operation.is_none() {
            guard.operation = load_insight_operation(root, database_path);
        }
        Ok(guard.operation.clone())
    }

    fn begin(
        &self,
        root: &Utf8Path,
        mut operation: AgentInsightOperation,
        attempt_dir: Utf8PathBuf,
        database_path: &Utf8Path,
    ) -> CommandResult<(AgentInsightOperation, Arc<AtomicBool>)> {
        let mut guard = self.lock()?;
        if guard.operation.is_none() {
            guard.operation = load_insight_operation(root, database_path);
        }
        operation.generation = guard
            .operation
            .as_ref()
            .map_or(1, |current| current.generation.saturating_add(1));
        if guard
            .operation
            .as_ref()
            .is_some_and(|operation| operation.status.is_active())
        {
            return Err(analytics_error(
                "analytics.insight-operation-conflict",
                json!({ "operationId": guard.operation.as_ref().unwrap().operation_id }),
            ));
        }
        let requested = Arc::new(AtomicBool::new(false));
        write_json(&root.join("insight-state.json"), &operation).map_err(storage_error)?;
        guard.operation = Some(operation.clone());
        guard.cancellation = Some(CancellationState {
            operation_id: operation.operation_id.clone(),
            requested: requested.clone(),
            attempt_dir,
        });
        Ok((operation, requested))
    }

    fn transition(
        &self,
        root: &Utf8Path,
        operation_id: &str,
        update: impl FnOnce(&mut AgentInsightOperation),
    ) -> CommandResult<Option<AgentInsightOperation>> {
        let mut guard = self.lock()?;
        let Some(mut operation) = guard.operation.clone() else {
            return Ok(None);
        };
        if operation.operation_id != operation_id || !operation.status.is_active() {
            return Ok(None);
        }
        let current_status = operation.status;
        update(&mut operation);
        if !insight_transition_allowed(current_status, operation.status) {
            return Ok(None);
        }
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        write_json(&root.join("insight-state.json"), &operation).map_err(storage_error)?;
        if !operation.status.is_active() {
            guard.cancellation = None;
        }
        guard.operation = Some(operation.clone());
        Ok(Some(operation))
    }

    fn complete_with_cache(
        &self,
        root: &Utf8Path,
        operation_id: &str,
        index: &mut PersonalAnalyticsIndex,
        identity: &InsightIdentity,
        narrative: &PersonalAnalyticsNarrative,
    ) -> CommandResult<Option<AgentInsightOperation>> {
        let mut guard = self.lock()?;
        let Some(mut operation) = guard.operation.clone() else {
            return Ok(None);
        };
        if operation.operation_id != operation_id
            || operation.status != AgentInsightOperationStatus::ValidatingReport
        {
            return Ok(None);
        }
        let completed_at = timestamp();
        index
            .store_completed_insight(identity, narrative, &completed_at)
            .map_err(|error| {
                analytics_error(
                    "analytics.storage-failed",
                    json!({ "reason": error.to_string() }),
                )
            })?;
        operation.status = AgentInsightOperationStatus::Completed;
        operation.progress.stage = AgentInsightOperationStatus::Completed;
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = completed_at.clone();
        operation.completed_at = Some(completed_at);
        operation.error = None;

        // The completed cache is the durable commit marker. If the JSON projection cannot be
        // replaced, startup recovery reconstructs this terminal state from that cache entry.
        let _ = write_json(&root.join("insight-state.json"), &operation);
        guard.operation = Some(operation.clone());
        guard.cancellation = None;
        Ok(Some(operation))
    }

    fn request_cancel(
        &self,
        root: &Utf8Path,
        operation_id: &str,
        database_path: &Utf8Path,
    ) -> CommandResult<(AgentInsightOperation, Option<Utf8PathBuf>)> {
        let mut guard = self.lock()?;
        if guard.operation.is_none() {
            guard.operation = load_insight_operation(root, database_path);
        }
        let Some(mut operation) = guard.operation.clone() else {
            return Err(analytics_error(
                "analytics.insight-operation-not-found",
                json!({ "operationId": operation_id }),
            ));
        };
        if operation.operation_id != operation_id {
            return Err(analytics_error(
                "analytics.insight-operation-not-found",
                json!({ "operationId": operation_id }),
            ));
        }
        if !operation.status.is_active() {
            return Ok((operation.clone(), None));
        }
        let cancellation = guard
            .cancellation
            .as_ref()
            .filter(|state| state.operation_id == operation_id)
            .map(|state| (state.requested.clone(), state.attempt_dir.clone()));
        operation.status = AgentInsightOperationStatus::Cancelling;
        operation.progress.stage = AgentInsightOperationStatus::Cancelling;
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        write_json(&root.join("insight-state.json"), &operation).map_err(storage_error)?;
        guard.operation = Some(operation.clone());
        if let Some((requested, _)) = &cancellation {
            requested.store(true, Ordering::Release);
        }
        let attempt_dir = cancellation.map(|(_, attempt_dir)| attempt_dir);
        Ok((operation, attempt_dir))
    }

    fn set_attempt_dir(&self, operation_id: &str, attempt_dir: Utf8PathBuf) {
        let mut guard = self.inner.lock().ok();
        if let Some(state) = guard
            .as_mut()
            .and_then(|inner| inner.cancellation.as_mut())
            .filter(|state| state.operation_id == operation_id)
        {
            state.attempt_dir = attempt_dir;
        }
    }

    fn lock(&self) -> CommandResult<std::sync::MutexGuard<'_, InsightRuntimeInner>> {
        self.inner.lock().map_err(|_| {
            analytics_error(
                "analytics.state-unavailable",
                json!({ "reason": "poisoned" }),
            )
        })
    }
}

#[tauri::command]
pub fn get_personal_analytics(
    state: State<'_, DesktopState>,
    runtime: State<'_, PersonalAnalyticsRuntime>,
    insight_runtime: State<'_, PersonalAnalyticsInsightRuntime>,
) -> CommandResult<PersonalAnalyticsSnapshot> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let mut snapshot = runtime.snapshot(&analytics_root(&app.paths.user_gold_band_dir()))?;
    snapshot.insight_operation = insight_runtime.operation(
        &analytics_root(&app.paths.user_gold_band_dir()),
        &app.paths.sqlite_db_path(),
    )?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn sync_personal_analytics(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, PersonalAnalyticsRuntime>,
) -> CommandResult<PersonalAnalyticsSnapshot> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let root = analytics_root(&app.paths.user_gold_band_dir());
    let operation_id = Uuid::new_v4().to_string();
    let operation_dir = root.join("operations").join(&operation_id);
    let attempt_dir = operation_dir.clone();
    let now = timestamp();
    let operation = PersonalAnalyticsOperation {
        operation_id: operation_id.clone(),
        agent_type: "index".to_string(),
        status: PersonalAnalyticsOperationStatus::Queued,
        revision: 1,
        progress: PersonalAnalyticsProgress {
            stage: PersonalAnalyticsOperationStatus::Queued,
            processed_units: 0,
            total_units: 0,
        },
        source_watermark: now.clone(),
        report_id: None,
        error: None,
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
    };
    let (snapshot, cancellation) = runtime.begin(&root, operation, attempt_dir)?;
    emit_snapshot(&app_handle, &snapshot);

    let runtime = runtime.inner().clone();
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_sync_operation(
            &handle,
            &runtime,
            app,
            root,
            operation_dir,
            operation_id,
            cancellation,
        );
    });
    Ok(snapshot)
}

#[tauri::command]
pub fn query_personal_analytics_report(
    state: State<'_, DesktopState>,
    input: QueryPersonalAnalyticsReportInput,
) -> CommandResult<gold_band::personal_analytics::PersonalAnalyticsReport> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let index = PersonalAnalyticsIndex::open(&app.paths.sqlite_db_path()).map_err(|error| {
        analytics_error(
            "analytics.storage-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let mut report = index
        .report(&input.range, Uuid::new_v4().to_string())
        .map_err(|error| {
            analytics_error(
                "analytics.report-query-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
    let report_index_revision = report.index_revision;
    if let Some(agent_type) = input.agent_type.as_deref() {
        let identity = gold_band::personal_analytics::index::InsightIdentity {
            operation_id: String::new(),
            range_start: input.range.start.clone(),
            range_end: input.range.end.clone(),
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            index_revision: report.index_revision,
            agent_type: agent_type.to_string(),
        };
        if let Some(narrative) = index.completed_insight(&identity).map_err(|error| {
            analytics_error(
                "analytics.storage-failed",
                json!({ "reason": error.to_string() }),
            )
        })? {
            report = canonicalize_personal_analytics_report(
                &projection_from_report(&report),
                narrative,
                report.report_id.clone(),
                report.generated_at.clone(),
            );
            report.index_revision = report_index_revision;
            report.range = input.range;
        }
    }
    Ok(report)
}

#[tauri::command]
pub async fn start_personal_analytics_insights(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, PersonalAnalyticsInsightRuntime>,
    input: StartPersonalAnalyticsInsightsInput,
) -> CommandResult<AgentInsightOperation> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let agent_id = input.agent_type.parse::<ManagedAgentId>().map_err(|_| {
        analytics_error(
            "analytics.agent-unavailable",
            json!({ "agentType": input.agent_type }),
        )
    })?;
    let diagnostics = state.agent_diagnostics().map_err(|error| {
        analytics_error(
            "analytics.agent-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    if !diagnostics
        .get(&agent_id)
        .is_some_and(|diagnostic| diagnostic.available)
    {
        return Err(analytics_error(
            "analytics.agent-unavailable",
            json!({ "agentType": input.agent_type }),
        ));
    }
    let (_, agent_config) = app.managed_agent(&input.agent_type).map_err(|_| {
        analytics_error(
            "analytics.agent-unavailable",
            json!({ "agentType": input.agent_type }),
        )
    })?;
    let agent_config = agent_config.clone();
    let root = analytics_root(&app.paths.user_gold_band_dir());
    let database_path = app.paths.sqlite_db_path();
    let report_range = input.range.clone();
    let report_database_path = database_path.clone();
    let (report, semantic_items) = tauri::async_runtime::spawn_blocking(move || {
        let index = PersonalAnalyticsIndex::open(&report_database_path).map_err(|error| {
            analytics_error(
                "analytics.storage-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
        index
            .report_with_semantic_batch(&report_range, Uuid::new_v4().to_string())
            .map_err(|error| {
                analytics_error(
                    "analytics.report-query-failed",
                    json!({ "reason": error.to_string() }),
                )
            })
    })
    .await
    .map_err(|_| analytics_error("analytics.task-join-failed", json!({})))??;
    let operation_id = Uuid::new_v4().to_string();
    let operation_dir = root.join("operations").join(&operation_id);
    let now = timestamp();
    let operation = AgentInsightOperation {
        operation_id: operation_id.clone(),
        generation: 0,
        agent_type: input.agent_type.clone(),
        range: input.range,
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        index_revision: report.index_revision,
        status: AgentInsightOperationStatus::Queued,
        revision: 1,
        progress: AgentInsightProgress {
            stage: AgentInsightOperationStatus::Queued,
            processed_units: 0,
            total_units: 0,
        },
        source_watermark: report.source_watermark.clone(),
        report_id: report.report_id.clone(),
        error: None,
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
    };
    let (operation, cancellation) = runtime.begin(
        &root,
        operation,
        operation_dir.join("analysis-attempt"),
        &database_path,
    )?;
    emit_snapshot(
        &app_handle,
        &PersonalAnalyticsSnapshot {
            insight_operation: Some(operation.clone()),
            ..PersonalAnalyticsSnapshot::default()
        },
    );
    let runtime = runtime.inner.clone();
    let handle = app_handle.clone();
    let worker_operation = operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_insight_operation(
            &handle,
            &PersonalAnalyticsInsightRuntime { inner: runtime },
            &app,
            &agent_config,
            &root,
            &operation_dir,
            &worker_operation,
            report,
            semantic_items,
            &cancellation,
        );
    });
    Ok(operation)
}

#[tauri::command]
pub fn cancel_personal_analytics_insights(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, PersonalAnalyticsInsightRuntime>,
    input: CancelPersonalAnalyticsInput,
) -> CommandResult<AgentInsightOperation> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let root = analytics_root(&app.paths.user_gold_band_dir());
    let (operation, attempt_dir) =
        runtime.request_cancel(&root, &input.operation_id, &app.paths.sqlite_db_path())?;
    if let Some(attempt_dir) = attempt_dir {
        client::request_prompt_cancel(&attempt_dir);
    }
    emit_snapshot(
        &app_handle,
        &PersonalAnalyticsSnapshot {
            insight_operation: Some(operation.clone()),
            ..PersonalAnalyticsSnapshot::default()
        },
    );
    Ok(operation)
}

#[tauri::command]
pub fn cancel_personal_analytics(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, PersonalAnalyticsRuntime>,
    input: CancelPersonalAnalyticsInput,
) -> CommandResult<PersonalAnalyticsSnapshot> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let root = analytics_root(&app.paths.user_gold_band_dir());
    let (snapshot, attempt_dir) = runtime.request_cancel(&root, &input.operation_id)?;
    if let Some(attempt_dir) = attempt_dir {
        client::request_prompt_cancel(&attempt_dir);
    }
    emit_snapshot(&app_handle, &snapshot);
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
fn run_sync_operation(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsRuntime,
    app: gold_band::app::App,
    analytics_root: Utf8PathBuf,
    _operation_dir: Utf8PathBuf,
    operation_id: String,
    cancellation: Arc<AtomicBool>,
) {
    let result = run_sync_operation_inner(
        app_handle,
        runtime,
        &app,
        &analytics_root,
        &operation_id,
        &cancellation,
    );
    if cancellation.load(Ordering::Acquire) {
        finish_cancelled(app_handle, runtime, &analytics_root, &operation_id);
    } else if let Err(error) = result {
        finish_failed(app_handle, runtime, &analytics_root, &operation_id, error);
    }
}

fn run_sync_operation_inner(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsRuntime,
    app: &gold_band::app::App,
    analytics_root: &Utf8Path,
    operation_id: &str,
    cancellation: &AtomicBool,
) -> CommandResult<()> {
    transition_and_emit(
        app_handle,
        runtime,
        analytics_root,
        operation_id,
        |operation, _| {
            operation.status = PersonalAnalyticsOperationStatus::Scanning;
            operation.progress.stage = PersonalAnalyticsOperationStatus::Scanning;
        },
    )?;
    ensure_not_cancelled(cancellation)?;
    let mut index = PersonalAnalyticsIndex::open(&app.paths.sqlite_db_path()).map_err(|error| {
        analytics_error(
            "analytics.storage-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let stats = index
        .sync(
            &app.paths.projects_dir(),
            |processed, total| {
                let _ = transition_and_emit(
                    app_handle,
                    runtime,
                    analytics_root,
                    operation_id,
                    |operation, _| {
                        operation.progress.processed_units = processed;
                        operation.progress.total_units = total;
                    },
                );
            },
            || cancellation.load(Ordering::Acquire),
        )
        .map_err(|error| {
            if cancellation.load(Ordering::Acquire) {
                analytics_error("analytics.cancelled", json!({}))
            } else {
                analytics_error(
                    "analytics.index-sync-failed",
                    json!({ "reason": error.to_string() }),
                )
            }
        })?;
    ensure_not_cancelled(cancellation)?;
    let report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            Uuid::new_v4().to_string(),
        )
        .map_err(|error| {
            analytics_error(
                "analytics.report-query-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
    ensure_not_cancelled(cancellation)?;
    write_json(&analytics_root.join("latest-report.json"), &report).map_err(storage_error)?;
    ensure_not_cancelled(cancellation)?;
    transition_and_emit(
        app_handle,
        runtime,
        analytics_root,
        operation_id,
        |operation, snapshot| {
            operation.status = PersonalAnalyticsOperationStatus::Completed;
            operation.progress.stage = PersonalAnalyticsOperationStatus::Completed;
            operation.source_watermark = stats.index_revision.to_string();
            operation.report_id = Some(report.report_id.clone());
            operation.completed_at = Some(timestamp());
            snapshot.latest_report = Some(report);
        },
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_insight_operation(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsInsightRuntime,
    app: &gold_band::app::App,
    agent_config: &gold_band::config::ManagedAgentConfig,
    analytics_root: &Utf8Path,
    operation_dir: &Utf8Path,
    operation: &AgentInsightOperation,
    report: PersonalAnalyticsReport,
    semantic_items: Vec<PersonalAnalyticsSemanticItem>,
    cancellation: &AtomicBool,
) {
    let result = run_insight_operation_inner(
        app_handle,
        runtime,
        app,
        agent_config,
        analytics_root,
        operation_dir,
        operation,
        report,
        semantic_items,
        cancellation,
    );
    if cancellation.load(Ordering::Acquire) {
        finish_insight_cancelled(app_handle, runtime, analytics_root, &operation.operation_id);
    } else if let Err(error) = result {
        let _ = transition_insight_and_emit(
            app_handle,
            runtime,
            analytics_root,
            &operation.operation_id,
            |operation| {
                operation.status = AgentInsightOperationStatus::Failed;
                operation.progress.stage = AgentInsightOperationStatus::Failed;
                operation.error = Some(error);
                operation.completed_at = Some(timestamp());
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn run_insight_operation_inner(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsInsightRuntime,
    app: &gold_band::app::App,
    agent_config: &gold_band::config::ManagedAgentConfig,
    analytics_root: &Utf8Path,
    operation_dir: &Utf8Path,
    operation: &AgentInsightOperation,
    report: PersonalAnalyticsReport,
    semantic_items: Vec<PersonalAnalyticsSemanticItem>,
    cancellation: &AtomicBool,
) -> CommandResult<()> {
    let operation_id = &operation.operation_id;
    transition_insight_and_emit(
        app_handle,
        runtime,
        analytics_root,
        operation_id,
        |operation| {
            operation.status = AgentInsightOperationStatus::Analyzing;
            operation.progress.stage = AgentInsightOperationStatus::Analyzing;
        },
    )?;
    ensure_not_cancelled(cancellation)?;
    let mut index = PersonalAnalyticsIndex::open(&app.paths.sqlite_db_path()).map_err(|error| {
        analytics_error(
            "analytics.storage-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let identity = InsightIdentity {
        operation_id: operation_id.to_string(),
        range_start: operation.range.start.clone(),
        range_end: operation.range.end.clone(),
        schema_version: operation.schema_version.clone(),
        index_revision: operation.index_revision,
        agent_type: operation.agent_type.clone(),
    };
    let cached_narrative = index.completed_insight(&identity).map_err(|error| {
        analytics_error(
            "analytics.storage-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    ensure_not_cancelled(cancellation)?;
    let cache_hit = cached_narrative.is_some();
    let narrative = match cached_narrative {
        Some(cached) => cached,
        None => {
            let projection = projection_from_report(&report);
            let semantic_batch = PersonalAnalyticsSemanticBatch {
                schema_version: operation.schema_version.clone(),
                items: semantic_items,
            };
            std::fs::create_dir_all(operation_dir).map_err(|error| {
                analytics_error(
                    "analytics.storage-failed",
                    json!({ "reason": error.to_string() }),
                )
            })?;
            let projection_path = operation_dir.join("projection.json");
            let semantic_path = operation_dir.join("semantic-batch.json");
            let content_manifest_path = operation_dir.join("content-manifest.json");
            write_json(&projection_path, &projection).map_err(storage_error)?;
            write_json(&semantic_path, &semantic_batch).map_err(storage_error)?;
            write_json(&content_manifest_path, &content_manifest(&semantic_batch))
                .map_err(storage_error)?;
            ensure_not_cancelled(cancellation)?;
            let candidate = invoke_agent(
                app,
                agent_config,
                &operation.agent_type,
                operation_id,
                operation_dir,
                &projection_path,
                &content_manifest_path,
                &semantic_path,
                &projection,
                operation.index_revision,
                &serde_json::to_string(&operation.range).unwrap_or_default(),
                false,
                None,
            )?;
            transition_insight_and_emit(
                app_handle,
                runtime,
                analytics_root,
                operation_id,
                |operation| {
                    operation.status = AgentInsightOperationStatus::ValidatingReport;
                    operation.progress.stage = AgentInsightOperationStatus::ValidatingReport;
                },
            )?;
            match parse_personal_analytics_narrative(&candidate) {
                Ok(narrative) => narrative,
                Err(first_error) => {
                    let invalid_path = operation_dir.join("invalid-report.json");
                    std::fs::write(&invalid_path, candidate).map_err(storage_error)?;
                    runtime.set_attempt_dir(operation_id, operation_dir.join("repair-attempt"));
                    ensure_not_cancelled(cancellation)?;
                    transition_insight_and_emit(
                        app_handle,
                        runtime,
                        analytics_root,
                        operation_id,
                        |operation| {
                            operation.status = AgentInsightOperationStatus::Analyzing;
                            operation.progress.stage = AgentInsightOperationStatus::Analyzing;
                        },
                    )?;
                    let repaired = invoke_agent(
                        app,
                        agent_config,
                        &operation.agent_type,
                        operation_id,
                        operation_dir,
                        &projection_path,
                        &content_manifest_path,
                        &semantic_path,
                        &projection,
                        operation.index_revision,
                        &serde_json::to_string(&operation.range).unwrap_or_default(),
                        true,
                        Some((&invalid_path, first_error)),
                    )?;
                    transition_insight_and_emit(
                        app_handle,
                        runtime,
                        analytics_root,
                        operation_id,
                        |operation| {
                            operation.status = AgentInsightOperationStatus::ValidatingReport;
                            operation.progress.stage =
                                AgentInsightOperationStatus::ValidatingReport;
                        },
                    )?;
                    parse_personal_analytics_narrative(&repaired).map_err(|error| {
                        analytics_error("analytics.report-invalid", json!({ "reason": error }))
                    })?
                }
            }
        }
    };
    if cache_hit {
        transition_insight_and_emit(
            app_handle,
            runtime,
            analytics_root,
            operation_id,
            |operation| {
                operation.status = AgentInsightOperationStatus::ValidatingReport;
                operation.progress.stage = AgentInsightOperationStatus::ValidatingReport;
            },
        )?;
    }
    ensure_not_cancelled(cancellation)?;
    let completed = runtime.complete_with_cache(
        analytics_root,
        operation_id,
        &mut index,
        &identity,
        &narrative,
    )?;
    let Some(completed) = completed else {
        ensure_not_cancelled(cancellation)?;
        return Err(analytics_error(
            "analytics.operation-stale",
            json!({ "operationId": operation_id }),
        ));
    };
    emit_snapshot(
        app_handle,
        &PersonalAnalyticsSnapshot {
            insight_operation: Some(completed),
            ..PersonalAnalyticsSnapshot::default()
        },
    );
    Ok(())
}

fn ensure_not_cancelled(cancellation: &AtomicBool) -> CommandResult<()> {
    if cancellation.load(Ordering::Acquire) {
        Err(analytics_error("analytics.cancelled", json!({})))
    } else {
        Ok(())
    }
}

fn transition_insight_and_emit(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsInsightRuntime,
    root: &Utf8Path,
    operation_id: &str,
    update: impl FnOnce(&mut AgentInsightOperation),
) -> CommandResult<()> {
    if let Some(operation) = runtime.transition(root, operation_id, update)? {
        emit_snapshot(
            app_handle,
            &PersonalAnalyticsSnapshot {
                insight_operation: Some(operation),
                ..PersonalAnalyticsSnapshot::default()
            },
        );
    }
    Ok(())
}

fn finish_insight_cancelled(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsInsightRuntime,
    root: &Utf8Path,
    operation_id: &str,
) {
    let _ = transition_insight_and_emit(app_handle, runtime, root, operation_id, |operation| {
        operation.status = AgentInsightOperationStatus::Cancelled;
        operation.progress.stage = AgentInsightOperationStatus::Cancelled;
        operation.error = Some(analytics_error("analytics.cancelled", json!({})));
        operation.completed_at = Some(timestamp());
    });
}

#[allow(clippy::too_many_arguments)]
fn invoke_agent(
    app: &gold_band::app::App,
    agent_config: &gold_band::config::ManagedAgentConfig,
    agent_type: &str,
    operation_id: &str,
    operation_dir: &Utf8Path,
    projection_path: &Utf8Path,
    content_manifest_path: &Utf8Path,
    semantic_path: &Utf8Path,
    projection: &PersonalAnalyticsProjection,
    index_revision: u64,
    date_range: &str,
    repair: bool,
    invalid: Option<(&Utf8Path, String)>,
) -> CommandResult<String> {
    let language = app.config.desktop_language;
    let schema = personal_analytics_narrative_schema();
    let (system_prompt, user_prompt, attachments, attempt_name) = if repair {
        let (invalid_path, validation_errors) = invalid.expect("repair requires invalid output");
        (
            render(
                prompt_by_language(
                    language,
                    PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN,
                    PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN,
                ),
                json!({}),
            ),
            render(
                prompt_by_language(
                    language,
                    PERSONAL_ANALYTICS_REPAIR_USER_ZH_CN,
                    PERSONAL_ANALYTICS_REPAIR_USER_EN,
                ),
                json!({
                    "operation_id": operation_id,
                    "invalid_report_path": invalid_path,
                    "validation_errors": validation_errors,
                    "report_schema": schema,
                }),
            ),
            vec![invalid_path.to_string()],
            "repair-attempt",
        )
    } else {
        (
            render(
                prompt_by_language(
                    language,
                    PERSONAL_ANALYTICS_SYSTEM_ZH_CN,
                    PERSONAL_ANALYTICS_SYSTEM_EN,
                ),
                json!({ "report_schema": schema }),
            ),
            render(
                prompt_by_language(
                    language,
                    PERSONAL_ANALYTICS_USER_ZH_CN,
                    PERSONAL_ANALYTICS_USER_EN,
                ),
                json!({
                    "operation_id": operation_id,
                    "report_schema_version": PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION,
                    "source_watermark": projection.source_watermark,
                    "index_revision": index_revision,
                    "date_range": date_range,
                    "projection_path": projection_path,
                    "content_manifest_path": content_manifest_path,
                    "semantic_batch_manifest_path": semantic_path,
                    "coverage_summary": serde_json::to_string(&projection.source_coverage).unwrap_or_default(),
                }),
            ),
            vec![
                projection_path.to_string(),
                content_manifest_path.to_string(),
                semantic_path.to_string(),
            ],
            "analysis-attempt",
        )
    };
    let resolved = resolve_attachments(
        &attachments,
        "analytics-inputs",
        AttachmentProjectionPolicy::from(&app.config),
    )
    .map_err(|error| {
        analytics_error(
            "analytics.execution-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let turn_id = Uuid::new_v4().to_string();
    let prompt = PromptBundle {
        system_prompt: system_prompt.map_err(prompt_error)?,
        user_prompt: user_prompt.map_err(prompt_error)?,
        display_text: None,
        quotes: Vec::new(),
        prompt_id: Some(turn_id.clone()),
        visibility: PromptVisibility::Hidden,
        hidden_reason: Some("personalAnalytics".to_string()),
        turn_control_mode: TurnControlMode::NonRuntimeControlled,
        runtime_control_intent: RuntimeControlIntent::Unchanged,
        runtime_control_transition_id: None,
        runtime_control_source_transition_id: None,
        runtime_control_transition_cause: None,
        attachment_metas: resolved.iter().map(|item| item.meta.clone()).collect(),
        content_blocks: resolved.iter().map(|item| item.block.clone()).collect(),
    };
    let attempt_dir = operation_dir.join(attempt_name);
    let lifecycle_owner = claim_agent_prompt_lifecycle(
        &attempt_dir,
        &turn_id,
        agent_type,
        &agent_config.adapter.display_name,
        &prompt.user_prompt,
        &attachments,
    )?;
    let run = client::run_prompt(
        agent_type,
        &agent_config.adapter,
        operation_dir.to_path_buf(),
        operation_dir.to_path_buf(),
        attempt_dir,
        &prompt,
        SessionMode::New,
        None,
        None,
        BTreeMap::new(),
        None,
        app.config.use_local_claude,
        app.config.require_local_claude_executable,
        app.config.acp_session_title_refresh_enabled,
        app.config.acp_raw_max_size_bytes,
        app.config.acp_raw_target_size_bytes,
        AcpRuntimePolicy::from(&app.config)
            .with_external_session_sync_enabled(agent_config.external_session_sync_enabled)
            .with_system_prompt_support(agent_config.supports_system_prompt()),
        lifecycle_owner,
        None,
        &[],
        None,
        None,
        None,
    )
    .map_err(|error| {
        analytics_error(
            "analytics.execution-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    if let Some(failure) = run.terminal_failure {
        return Err(analytics_error(
            "analytics.execution-failed",
            json!({ "agentCode": failure.code }),
        ));
    }
    personal_analytics_artifact_text(&run.output)
        .ok_or_else(|| analytics_error("analytics.report-invalid", json!({ "reason": "empty" })))
}

fn claim_agent_prompt_lifecycle(
    attempt_dir: &Utf8Path,
    turn_id: &str,
    agent_type: &str,
    agent_display_name: &str,
    display_text: &str,
    attachment_paths: &[String],
) -> CommandResult<AcpLifecycleOwner> {
    let submission = AcpPromptSubmission {
        turn_id: turn_id.to_string(),
        operation_id: format!("prompt:{}", Uuid::new_v4().simple()),
        adapter_id: agent_type.to_string(),
        adapter_display_name: agent_display_name.to_string(),
        cwd: attempt_dir.to_string(),
        input: ConversationPromptInput {
            display_text: display_text.to_string(),
            quotes: Vec::new(),
        },
        attachment_paths: attachment_paths.to_vec(),
        admitted_at: gold_band::acp::events::current_timestamp(),
    };
    match admit_session_turn_for_execution(&attempt_dir.join("acp.snapshot.json"), &submission)
        .map_err(|error| {
            analytics_error(
                "analytics.execution-failed",
                json!({ "reason": error.to_string() }),
            )
        })? {
        AcpTurnExecutionClaim::Claimed(owner) => Ok(owner),
        AcpTurnExecutionClaim::AlreadySettled(_) => Err(analytics_error(
            "analytics.execution-failed",
            json!({ "reason": "acp.prompt-execution-already-settled" }),
        )),
        AcpTurnExecutionClaim::Stale => Err(analytics_error(
            "analytics.execution-failed",
            json!({ "reason": "acp.prompt-execution-claim-lost" }),
        )),
    }
}

fn personal_analytics_artifact_text(output: &client::AcpPromptOutput) -> Option<String> {
    let terminal_message = output.recent_messages.last()?;
    if terminal_message.has_stable_id {
        output
            .recent_messages
            .iter()
            .rev()
            .take(3)
            .find_map(|message| json_artifact_text(&message.text))
    } else if output.observed_stable_message {
        None
    } else {
        json_artifact_text(&terminal_message.text)
    }
}

fn content_manifest(batch: &PersonalAnalyticsSemanticBatch) -> ContentManifest {
    ContentManifest {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        authorized_locators: batch
            .items
            .iter()
            .map(|item| item.locator.clone())
            .collect(),
        excluded_sources: vec![
            "acp.raw.jsonl".to_string(),
            "doctor/".to_string(),
            "diagnostics/".to_string(),
            "binary".to_string(),
        ],
    }
}

fn parse_personal_analytics_narrative(
    payload: &str,
) -> std::result::Result<PersonalAnalyticsNarrative, String> {
    let narrative: PersonalAnalyticsNarrative =
        serde_json::from_str(payload).map_err(|error| error.to_string())?;
    if narrative.schema_version != PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION {
        return Err(format!(
            "schemaVersion must be {}",
            PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION
        ));
    }
    Ok(narrative)
}

fn projection_from_report(
    report: &gold_band::personal_analytics::PersonalAnalyticsReport,
) -> PersonalAnalyticsProjection {
    PersonalAnalyticsProjection {
        schema_version: report.schema_version.clone(),
        source_watermark: report.index_revision.to_string(),
        source_coverage: report.source_coverage.clone(),
        overview: report.overview.clone(),
        recent_tasks: report.recent_tasks.clone(),
        reliability: report.reliability.clone(),
        quality: report.quality.clone(),
        efficiency: report.efficiency.clone(),
        token_usage: report.token_usage.clone(),
        context_and_tools: report.context_and_tools.clone(),
        warnings: report.warnings.clone(),
        evidence_locators: Vec::new(),
    }
}

fn transition_and_emit(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsRuntime,
    analytics_root: &Utf8Path,
    operation_id: &str,
    update: impl FnOnce(&mut PersonalAnalyticsOperation, &mut PersonalAnalyticsSnapshot),
) -> CommandResult<()> {
    if let Some(snapshot) = runtime.transition(analytics_root, operation_id, update)? {
        emit_snapshot(app_handle, &snapshot);
    }
    Ok(())
}

fn finish_cancelled(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsRuntime,
    root: &Utf8Path,
    operation_id: &str,
) {
    let _ = transition_and_emit(app_handle, runtime, root, operation_id, |operation, _| {
        operation.status = PersonalAnalyticsOperationStatus::Cancelled;
        operation.progress.stage = PersonalAnalyticsOperationStatus::Cancelled;
        operation.error = Some(analytics_error("analytics.cancelled", json!({})));
        operation.completed_at = Some(timestamp());
    });
}

fn finish_failed(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsRuntime,
    root: &Utf8Path,
    operation_id: &str,
    error: PersonalAnalyticsError,
) {
    let _ = transition_and_emit(
        app_handle,
        runtime,
        root,
        operation_id,
        move |operation, _| {
            operation.status = PersonalAnalyticsOperationStatus::Failed;
            operation.progress.stage = PersonalAnalyticsOperationStatus::Failed;
            operation.error = Some(error);
            operation.completed_at = Some(timestamp());
        },
    );
}

fn emit_snapshot(app_handle: &AppHandle, snapshot: &PersonalAnalyticsSnapshot) {
    let _ = app_handle.emit(PERSONAL_ANALYTICS_UPDATED_EVENT, snapshot);
}

fn analytics_root(user_root: &Utf8Path) -> Utf8PathBuf {
    user_root.join("analytics")
}

fn load_insight_operation(
    root: &Utf8Path,
    database_path: &Utf8Path,
) -> Option<AgentInsightOperation> {
    let state_path = root.join("insight-state.json");
    let operation = read_json::<AgentInsightOperation>(&state_path).ok()?;
    if !operation.status.is_active() {
        return Some(operation);
    }
    let mut recovered = operation.clone();
    let identity = InsightIdentity {
        operation_id: recovered.operation_id.clone(),
        range_start: recovered.range.start.clone(),
        range_end: recovered.range.end.clone(),
        schema_version: recovered.schema_version.clone(),
        index_revision: recovered.index_revision,
        agent_type: recovered.agent_type.clone(),
    };
    let cache_committed = PersonalAnalyticsIndex::open(database_path)
        .and_then(|index| index.completed_insight(&identity))
        .ok()
        .flatten()
        .is_some();
    recovered.status = if cache_committed {
        AgentInsightOperationStatus::Completed
    } else {
        AgentInsightOperationStatus::Failed
    };
    recovered.progress.stage = recovered.status;
    recovered.revision = recovered.revision.saturating_add(1);
    recovered.updated_at = timestamp();
    recovered.completed_at = Some(recovered.updated_at.clone());
    recovered.error = (!cache_committed).then(|| {
        analytics_error(
            "analytics.execution-interrupted",
            json!({ "operationId": recovered.operation_id }),
        )
    });
    Some(if write_json(&state_path, &recovered).is_ok() {
        recovered
    } else {
        operation
    })
}

fn load_snapshot(root: &Utf8Path) -> PersonalAnalyticsSnapshot {
    let state_path = root.join("state.json");
    let mut snapshot: PersonalAnalyticsSnapshot = read_json(&state_path).unwrap_or_default();
    let report_path = root.join("latest-report.json");
    if report_path.is_file() {
        snapshot.latest_report = read_json(&report_path).ok();
    }
    let mut recovered_snapshot = snapshot.clone();
    let mut recovered = false;
    if let Some(operation) = recovered_snapshot.operation.as_mut() {
        if operation.status.is_active() {
            recovered = true;
            operation.status = PersonalAnalyticsOperationStatus::Failed;
            operation.progress.stage = PersonalAnalyticsOperationStatus::Failed;
            operation.revision = operation.revision.saturating_add(1);
            operation.updated_at = timestamp();
            operation.completed_at = Some(operation.updated_at.clone());
            operation.error = Some(analytics_error(
                "analytics.execution-interrupted",
                json!({ "operationId": operation.operation_id }),
            ));
        }
    }
    if recovered && write_json(&state_path, &recovered_snapshot).is_ok() {
        recovered_snapshot
    } else {
        snapshot
    }
}

fn persist_snapshot(root: &Utf8Path, snapshot: &PersonalAnalyticsSnapshot) -> CommandResult<()> {
    write_json(&root.join("state.json"), snapshot).map_err(storage_error)
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn analytics_error(code: impl Into<String>, params: Value) -> PersonalAnalyticsError {
    PersonalAnalyticsError::new(code, params)
}

fn storage_error(error: impl std::fmt::Display) -> PersonalAnalyticsError {
    analytics_error(
        "analytics.storage-failed",
        json!({ "reason": error.to_string() }),
    )
}

fn prompt_error(error: impl std::fmt::Display) -> PersonalAnalyticsError {
    analytics_error(
        "analytics.prompt-invalid",
        json!({ "reason": error.to_string() }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message_output(text: &str, has_stable_id: bool) -> client::AcpPromptMessageOutput {
        client::AcpPromptMessageOutput {
            text: text.to_string(),
            has_stable_id,
        }
    }

    #[test]
    fn artifact_text_scans_last_three_stable_messages_backwards() {
        let output = client::AcpPromptOutput {
            recent_messages: vec![
                message_output("{\"result\":\"old\"}", true),
                message_output("prose", true),
                message_output("{\"result\":false}", true),
                message_output("final", true),
            ],
            observed_stable_message: true,
            ..Default::default()
        };

        assert_eq!(
            personal_analytics_artifact_text(&output),
            Some("{\"result\":false}".to_string())
        );
    }

    #[test]
    fn artifact_text_accepts_json_from_anonymous_only_agent_output() {
        let output = client::AcpPromptOutput {
            recent_messages: vec![message_output("summary\n{\"result\":true}", false)],
            observed_stable_message: false,
            ..Default::default()
        };

        assert_eq!(
            personal_analytics_artifact_text(&output),
            Some("{\"result\":true}".to_string())
        );
    }

    #[test]
    fn artifact_text_rejects_anonymous_terminal_after_stable_output() {
        let output = client::AcpPromptOutput {
            recent_messages: vec![
                message_output("{\"result\":true}", true),
                message_output("late anonymous text", false),
            ],
            observed_stable_message: true,
            ..Default::default()
        };

        assert_eq!(personal_analytics_artifact_text(&output), None);
    }

    #[test]
    fn narrative_parser_rejects_mismatched_schema_version() {
        let payload = r#"{"schemaVersion":"0.0.0","insights":[]}"#;
        let error = parse_personal_analytics_narrative(&payload)
            .expect_err("narrative schema version must match the report contract");
        assert!(error.contains(PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION));
    }

    #[test]
    fn insight_agent_attempt_claims_a_durable_acp_lifecycle_owner() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().join("analysis-attempt")).unwrap();
        let attachment_paths = vec!["projection.json".to_string()];

        let owner = claim_agent_prompt_lifecycle(
            &attempt_dir,
            "turn-1",
            "agent-a",
            "Agent A",
            "analyze the projection",
            &attachment_paths,
        )
        .unwrap();
        let snapshot = read_json::<Value>(&attempt_dir.join("acp.snapshot.json")).unwrap();

        assert_eq!(owner.turn_id, "turn-1");
        assert!(owner.operation_id.starts_with("prompt:"));
        assert_eq!(snapshot["turnId"], owner.turn_id);
        assert_eq!(snapshot["lifecycleOperationId"], owner.operation_id);
        assert_eq!(snapshot["acpRevision"], owner.revision);
        assert_eq!(snapshot["liveTurnActivity"], "accepted");
        assert_eq!(
            snapshot["promptSubmission"]["attachmentPaths"][0],
            "projection.json"
        );

        gold_band::acp::events::persist_session_turn_terminal_owned(
            &attempt_dir.join("acp.snapshot.json"),
            &owner.turn_id,
            Some(&owner.operation_id),
            owner.revision,
            gold_band::acp::events::AcpLatestTurnStatus::Completed,
            "completed",
            &gold_band::acp::events::current_timestamp(),
        )
        .unwrap();
    }

    fn operation(id: &str) -> PersonalAnalyticsOperation {
        PersonalAnalyticsOperation {
            operation_id: id.to_string(),
            agent_type: "agent-a".to_string(),
            status: PersonalAnalyticsOperationStatus::Queued,
            revision: 1,
            progress: PersonalAnalyticsProgress {
                stage: PersonalAnalyticsOperationStatus::Queued,
                processed_units: 0,
                total_units: 0,
            },
            source_watermark: timestamp(),
            report_id: None,
            error: None,
            created_at: timestamp(),
            updated_at: timestamp(),
            completed_at: None,
        }
    }

    fn insight_operation(
        status: AgentInsightOperationStatus,
        revision: u64,
    ) -> AgentInsightOperation {
        AgentInsightOperation {
            operation_id: format!("operation-{revision}"),
            generation: 1,
            agent_type: "agent-a".to_string(),
            range: PersonalAnalyticsDateRange::default(),
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            index_revision: 1,
            status,
            revision,
            progress: AgentInsightProgress {
                stage: status,
                processed_units: 0,
                total_units: 0,
            },
            source_watermark: "1".to_string(),
            report_id: "report-1".to_string(),
            error: None,
            created_at: "2026-08-18T00:00:00Z".to_string(),
            updated_at: "2026-08-18T00:00:00Z".to_string(),
            completed_at: None,
        }
    }

    #[test]
    fn active_operation_conflicts_and_terminal_operation_can_be_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let runtime = PersonalAnalyticsRuntime::default();
        runtime
            .begin(&root, operation("one"), root.join("one"))
            .unwrap();
        let error = runtime
            .begin(&root, operation("two"), root.join("two"))
            .unwrap_err();
        assert_eq!(error.code, "analytics.operation-conflict");
        runtime
            .transition(&root, "one", |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Completed;
            })
            .unwrap();
        runtime
            .begin(&root, operation("two"), root.join("two"))
            .unwrap();
    }

    #[test]
    fn terminal_state_rejects_late_updates() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let runtime = PersonalAnalyticsRuntime::default();
        runtime
            .begin(&root, operation("one"), root.join("one"))
            .unwrap();
        let completed = runtime
            .transition(&root, "one", |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Completed;
            })
            .unwrap()
            .unwrap();
        let revision = completed.operation.unwrap().revision;
        assert!(
            runtime
                .transition(&root, "one", |operation, _| {
                    operation.status = PersonalAnalyticsOperationStatus::Failed;
                })
                .unwrap()
                .is_none()
        );
        assert_eq!(
            runtime.snapshot(&root).unwrap().operation.unwrap().revision,
            revision
        );
    }

    #[test]
    fn cancelling_sync_operation_only_allows_cancelled_terminal_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let runtime = PersonalAnalyticsRuntime::default();
        runtime
            .begin(&root, operation("one"), root.join("one"))
            .unwrap();
        let cancelling = runtime
            .transition(&root, "one", |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Cancelling;
            })
            .unwrap()
            .unwrap()
            .operation
            .unwrap();

        assert!(
            runtime
                .transition(&root, "one", |operation, _| {
                    operation.status = PersonalAnalyticsOperationStatus::Completed;
                })
                .unwrap()
                .is_none()
        );
        let unchanged = runtime.snapshot(&root).unwrap().operation.unwrap();
        assert_eq!(
            unchanged.status,
            PersonalAnalyticsOperationStatus::Cancelling
        );
        assert_eq!(unchanged.revision, cancelling.revision);

        let cancelled = runtime
            .transition(&root, "one", |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Cancelled;
            })
            .unwrap()
            .unwrap()
            .operation
            .unwrap();
        assert_eq!(
            cancelled.status,
            PersonalAnalyticsOperationStatus::Cancelled
        );
        assert_eq!(cancelled.revision, cancelling.revision + 1);
    }

    #[test]
    fn cancelling_insight_operation_only_allows_cancelled_terminal_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let database_path = root.join("gold-band.db");
        let runtime = PersonalAnalyticsInsightRuntime::default();
        let initial = insight_operation(AgentInsightOperationStatus::Queued, 1);
        let operation_id = initial.operation_id.clone();
        runtime
            .begin(
                &root,
                initial,
                root.join("analysis-attempt"),
                &database_path,
            )
            .unwrap();
        let cancelling = runtime
            .transition(&root, &operation_id, |operation| {
                operation.status = AgentInsightOperationStatus::Cancelling;
            })
            .unwrap()
            .unwrap();

        assert!(
            runtime
                .transition(&root, &operation_id, |operation| {
                    operation.status = AgentInsightOperationStatus::Completed;
                })
                .unwrap()
                .is_none()
        );
        let unchanged = runtime.operation(&root, &database_path).unwrap().unwrap();
        assert_eq!(unchanged.status, AgentInsightOperationStatus::Cancelling);
        assert_eq!(unchanged.revision, cancelling.revision);

        let cancelled = runtime
            .transition(&root, &operation_id, |operation| {
                operation.status = AgentInsightOperationStatus::Cancelled;
            })
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.status, AgentInsightOperationStatus::Cancelled);
        assert_eq!(cancelled.revision, cancelling.revision + 1);
    }

    #[test]
    fn cancellation_wins_before_the_completed_cache_commit() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let database_path = root.join("gold-band.db");
        let runtime = PersonalAnalyticsInsightRuntime::default();
        let initial = insight_operation(AgentInsightOperationStatus::Queued, 1);
        let operation_id = initial.operation_id.clone();
        let (accepted, _) = runtime
            .begin(
                &root,
                initial,
                root.join("analysis-attempt"),
                &database_path,
            )
            .unwrap();
        runtime
            .transition(&root, &operation_id, |operation| {
                operation.status = AgentInsightOperationStatus::Analyzing;
            })
            .unwrap();
        runtime
            .transition(&root, &operation_id, |operation| {
                operation.status = AgentInsightOperationStatus::ValidatingReport;
            })
            .unwrap();
        runtime
            .request_cancel(&root, &operation_id, &database_path)
            .unwrap();
        let identity = InsightIdentity {
            operation_id,
            range_start: accepted.range.start,
            range_end: accepted.range.end,
            schema_version: accepted.schema_version,
            index_revision: accepted.index_revision,
            agent_type: accepted.agent_type,
        };
        let narrative = PersonalAnalyticsNarrative {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            insights: Vec::new(),
        };
        let mut index = PersonalAnalyticsIndex::open(&database_path).unwrap();

        assert!(
            runtime
                .complete_with_cache(
                    &root,
                    &identity.operation_id,
                    &mut index,
                    &identity,
                    &narrative
                )
                .unwrap()
                .is_none()
        );
        assert!(index.completed_insight(&identity).unwrap().is_none());
    }

    #[test]
    fn failed_snapshot_persistence_does_not_advance_memory_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let runtime = PersonalAnalyticsRuntime::default();
        runtime
            .begin(&root, operation("one"), root.join("one"))
            .unwrap();
        let state_path = root.join("state.json");
        std::fs::remove_file(&state_path).unwrap();
        std::fs::create_dir(&state_path).unwrap();

        let error = runtime
            .transition(&root, "one", |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Completed;
            })
            .unwrap_err();
        assert_eq!(error.code, "analytics.storage-failed");
        let current = runtime.snapshot(&root).unwrap().operation.unwrap();
        assert_eq!(current.status, PersonalAnalyticsOperationStatus::Queued);
        assert_eq!(current.revision, 1);
    }

    #[test]
    fn failed_insight_persistence_does_not_advance_memory_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let database_path = root.join("gold-band.db");
        let runtime = PersonalAnalyticsInsightRuntime::default();
        let initial = insight_operation(AgentInsightOperationStatus::Queued, 1);
        let operation_id = initial.operation_id.clone();
        runtime
            .begin(
                &root,
                initial,
                root.join("analysis-attempt"),
                &database_path,
            )
            .unwrap();
        let state_path = root.join("insight-state.json");
        std::fs::remove_file(&state_path).unwrap();
        std::fs::create_dir(&state_path).unwrap();

        let error = runtime
            .transition(&root, &operation_id, |operation| {
                operation.status = AgentInsightOperationStatus::Analyzing;
            })
            .unwrap_err();
        assert_eq!(error.code, "analytics.storage-failed");
        let current = runtime.operation(&root, &database_path).unwrap().unwrap();
        assert_eq!(current.status, AgentInsightOperationStatus::Queued);
        assert_eq!(current.revision, 1);
    }

    #[test]
    fn interrupted_insight_operation_recovers_as_failed_and_allows_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let database_path = root.join("gold-band.db");
        std::fs::create_dir_all(&root).unwrap();
        write_json(
            &root.join("insight-state.json"),
            &insight_operation(AgentInsightOperationStatus::Analyzing, 2),
        )
        .unwrap();

        let runtime = PersonalAnalyticsInsightRuntime::default();
        let recovered = runtime
            .operation(&root, &database_path)
            .unwrap()
            .expect("operation is persisted");
        assert_eq!(recovered.status, AgentInsightOperationStatus::Failed);
        assert_eq!(recovered.revision, 3);
        assert_eq!(
            recovered.error.expect("recovery error").code,
            "analytics.execution-interrupted"
        );

        let restarted = insight_operation(AgentInsightOperationStatus::Queued, 1);
        let operation_id = restarted.operation_id.clone();
        let (accepted, _) = runtime
            .begin(
                &root,
                restarted,
                root.join("operations").join(operation_id.clone()),
                &database_path,
            )
            .unwrap();
        assert_eq!(accepted.status, AgentInsightOperationStatus::Queued);
        assert_eq!(accepted.generation, 2);
        assert_eq!(
            runtime
                .operation(&root, &database_path)
                .unwrap()
                .unwrap()
                .operation_id,
            operation_id
        );
    }

    #[test]
    fn interrupted_insight_recovers_completed_after_cache_commit() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let database_path = root.join("gold-band.db");
        std::fs::create_dir_all(&root).unwrap();
        let operation = insight_operation(AgentInsightOperationStatus::ValidatingReport, 4);
        let identity = InsightIdentity {
            operation_id: operation.operation_id.clone(),
            range_start: operation.range.start.clone(),
            range_end: operation.range.end.clone(),
            schema_version: operation.schema_version.clone(),
            index_revision: operation.index_revision,
            agent_type: operation.agent_type.clone(),
        };
        let mut index = PersonalAnalyticsIndex::open(&database_path).unwrap();
        index
            .store_completed_insight(
                &identity,
                &PersonalAnalyticsNarrative {
                    schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
                    insights: Vec::new(),
                },
                "2026-08-18T00:01:00Z",
            )
            .unwrap();
        write_json(&root.join("insight-state.json"), &operation).unwrap();

        let recovered = PersonalAnalyticsInsightRuntime::default()
            .operation(&root, &database_path)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, AgentInsightOperationStatus::Completed);
        assert_eq!(recovered.revision, 5);
        assert!(recovered.error.is_none());
    }
}
