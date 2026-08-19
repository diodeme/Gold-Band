use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use camino::{Utf8Path, Utf8PathBuf};
use gold_band::acp::client::{self, AcpOutputPolicy, AcpRuntimePolicy};
use gold_band::artifacts::json_artifact_text_from_outputs;
use gold_band::config::ManagedAgentId;
use gold_band::domain::{SessionMode, TurnControlMode};
use gold_band::personal_analytics::{
    canonicalize_personal_analytics_report, index::PersonalAnalyticsDateRange,
    index::PersonalAnalyticsIndex, personal_analytics_narrative_schema, PersonalAnalyticsError,
    PersonalAnalyticsNarrative, PersonalAnalyticsOperation, PersonalAnalyticsOperationStatus,
    PersonalAnalyticsProgress, PersonalAnalyticsProjection, PersonalAnalyticsSemanticBatch,
    PersonalAnalyticsSnapshot, PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION,
};
use gold_band::prompts::{
    prompt_by_language, render, PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN,
    PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN, PERSONAL_ANALYTICS_REPAIR_USER_EN,
    PERSONAL_ANALYTICS_REPAIR_USER_ZH_CN, PERSONAL_ANALYTICS_SYSTEM_EN,
    PERSONAL_ANALYTICS_SYSTEM_ZH_CN, PERSONAL_ANALYTICS_USER_EN, PERSONAL_ANALYTICS_USER_ZH_CN,
};
use gold_band::provider::{
    resolve_attachments, PromptBundle, PromptVisibility, RuntimeControlIntent,
};
use gold_band::storage::{read_json, write_json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    operation: Option<PersonalAnalyticsOperation>,
    cancellation: Option<CancellationState>,
}

#[derive(Clone, Default)]
pub struct PersonalAnalyticsInsightRuntime {
    inner: Arc<Mutex<InsightRuntimeInner>>,
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
        let snapshot = guard.snapshot.get_or_insert_with(Default::default);
        snapshot.operation = Some(operation.clone());
        let persisted = snapshot.clone();
        guard.cancellation = Some(CancellationState {
            operation_id: operation.operation_id,
            requested: requested.clone(),
            attempt_dir,
        });
        drop(guard);
        persist_snapshot(analytics_root, &persisted)?;
        Ok((persisted, requested))
    }

    fn transition(
        &self,
        analytics_root: &Utf8Path,
        operation_id: &str,
        update: impl FnOnce(&mut PersonalAnalyticsOperation, &mut PersonalAnalyticsSnapshot),
    ) -> CommandResult<Option<PersonalAnalyticsSnapshot>> {
        let mut guard = self.lock()?;
        let Some(mut snapshot) = guard.snapshot.take() else {
            return Ok(None);
        };
        let Some(mut operation) = snapshot.operation.take() else {
            guard.snapshot = Some(snapshot);
            return Ok(None);
        };
        if operation.operation_id != operation_id || !operation.status.is_active() {
            snapshot.operation = Some(operation);
            guard.snapshot = Some(snapshot);
            return Ok(None);
        }
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        update(&mut operation, &mut snapshot);
        let terminal = !operation.status.is_active();
        snapshot.operation = Some(operation);
        let persisted = snapshot.clone();
        guard.snapshot = Some(snapshot);
        if terminal {
            guard.cancellation = None;
        }
        drop(guard);
        persist_snapshot(analytics_root, &persisted)?;
        Ok(Some(persisted))
    }

    fn request_cancel(
        &self,
        analytics_root: &Utf8Path,
        operation_id: &str,
    ) -> CommandResult<(PersonalAnalyticsSnapshot, Option<Utf8PathBuf>)> {
        let (attempt_dir, already_terminal) = {
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
                .map(|state| {
                    state.requested.store(true, Ordering::Release);
                    state.attempt_dir.clone()
                });
            (attempt_dir, already_terminal)
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
        Ok((snapshot, attempt_dir))
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
    fn operation(&self, root: &Utf8Path) -> CommandResult<Option<PersonalAnalyticsOperation>> {
        let mut guard = self.lock()?;
        if guard.operation.is_none() {
            guard.operation = load_insight_operation(root);
        }
        Ok(guard.operation.clone())
    }

    fn begin(
        &self,
        root: &Utf8Path,
        operation: PersonalAnalyticsOperation,
        attempt_dir: Utf8PathBuf,
    ) -> CommandResult<(PersonalAnalyticsOperation, Arc<AtomicBool>)> {
        let mut guard = self.lock()?;
        if guard.operation.is_none() {
            guard.operation = load_insight_operation(root);
        }
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
        guard.operation = Some(operation.clone());
        guard.cancellation = Some(CancellationState {
            operation_id: operation.operation_id.clone(),
            requested: requested.clone(),
            attempt_dir,
        });
        write_json(&root.join("insight-state.json"), &guard.operation).map_err(storage_error)?;
        Ok((operation, requested))
    }

    fn transition(
        &self,
        root: &Utf8Path,
        operation_id: &str,
        update: impl FnOnce(&mut PersonalAnalyticsOperation),
    ) -> CommandResult<Option<PersonalAnalyticsOperation>> {
        let mut guard = self.lock()?;
        let Some(mut operation) = guard.operation.clone() else {
            return Ok(None);
        };
        if operation.operation_id != operation_id || !operation.status.is_active() {
            return Ok(None);
        }
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        update(&mut operation);
        if !operation.status.is_active() {
            guard.cancellation = None;
        }
        guard.operation = Some(operation.clone());
        write_json(&root.join("insight-state.json"), &guard.operation).map_err(storage_error)?;
        Ok(Some(operation))
    }

    fn request_cancel(
        &self,
        root: &Utf8Path,
        operation_id: &str,
    ) -> CommandResult<(PersonalAnalyticsOperation, Option<Utf8PathBuf>)> {
        let mut guard = self.lock()?;
        if guard.operation.is_none() {
            guard.operation = load_insight_operation(root);
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
        let attempt_dir = guard
            .cancellation
            .as_ref()
            .filter(|state| state.operation_id == operation_id)
            .map(|state| {
                state.requested.store(true, Ordering::Release);
                state.attempt_dir.clone()
            });
        operation.status = PersonalAnalyticsOperationStatus::Cancelling;
        operation.progress.stage = PersonalAnalyticsOperationStatus::Cancelling;
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        guard.operation = Some(operation.clone());
        write_json(&root.join("insight-state.json"), &guard.operation).map_err(storage_error)?;
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
    snapshot.insight_operation =
        insight_runtime.operation(&analytics_root(&app.paths.user_gold_band_dir()))?;
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
            report.index_revision = index
                .state()
                .map_err(|error| {
                    analytics_error(
                        "analytics.storage-failed",
                        json!({ "reason": error.to_string() }),
                    )
                })?
                .index_revision;
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
) -> CommandResult<PersonalAnalyticsOperation> {
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
    let operation_id = Uuid::new_v4().to_string();
    let operation_dir = root.join("operations").join(&operation_id);
    let now = timestamp();
    let operation = PersonalAnalyticsOperation {
        operation_id: operation_id.clone(),
        agent_type: input.agent_type.clone(),
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
    let (operation, cancellation) =
        runtime.begin(&root, operation, operation_dir.join("analysis-attempt"))?;
    emit_snapshot(
        &app_handle,
        &PersonalAnalyticsSnapshot {
            insight_operation: Some(operation.clone()),
            ..PersonalAnalyticsSnapshot::default()
        },
    );
    let runtime = runtime.inner.clone();
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_insight_operation(
            &handle,
            &PersonalAnalyticsInsightRuntime { inner: runtime },
            &app,
            &agent_config,
            &root,
            &operation_dir,
            &operation_id,
            &input.agent_type,
            &input.range,
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
) -> CommandResult<PersonalAnalyticsOperation> {
    let app = state.app().map_err(|error| {
        analytics_error(
            "analytics.source-unavailable",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let root = analytics_root(&app.paths.user_gold_band_dir());
    let (operation, attempt_dir) = runtime.request_cancel(&root, &input.operation_id)?;
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
    write_json(&analytics_root.join("latest-report.json"), &report).map_err(storage_error)?;
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
    operation_id: &str,
    agent_type: &str,
    range: &PersonalAnalyticsDateRange,
    cancellation: &AtomicBool,
) {
    let result = run_insight_operation_inner(
        app_handle,
        runtime,
        app,
        agent_config,
        analytics_root,
        operation_dir,
        operation_id,
        agent_type,
        range,
        cancellation,
    );
    if cancellation.load(Ordering::Acquire) {
        finish_insight_database_operation(app, operation_id, "cancelled", None);
        finish_insight_cancelled(app_handle, runtime, analytics_root, operation_id);
    } else if let Err(error) = result {
        finish_insight_database_operation(app, operation_id, "failed", Some(error.code.clone()));
        let _ = transition_insight_and_emit(
            app_handle,
            runtime,
            analytics_root,
            operation_id,
            |operation| {
                operation.status = PersonalAnalyticsOperationStatus::Failed;
                operation.progress.stage = PersonalAnalyticsOperationStatus::Failed;
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
    operation_id: &str,
    agent_type: &str,
    range: &PersonalAnalyticsDateRange,
    cancellation: &AtomicBool,
) -> CommandResult<()> {
    transition_insight_and_emit(
        app_handle,
        runtime,
        analytics_root,
        operation_id,
        |operation| {
            operation.status = PersonalAnalyticsOperationStatus::Analyzing;
            operation.progress.stage = PersonalAnalyticsOperationStatus::Analyzing;
        },
    )?;
    let index = PersonalAnalyticsIndex::open(&app.paths.sqlite_db_path()).map_err(|error| {
        analytics_error(
            "analytics.storage-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let report = index
        .report(range, Uuid::new_v4().to_string())
        .map_err(|error| {
            analytics_error(
                "analytics.report-query-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
    let identity = gold_band::personal_analytics::index::InsightIdentity {
        operation_id: operation_id.to_string(),
        range_start: range.start.clone(),
        range_end: range.end.clone(),
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        index_revision: report.index_revision,
        agent_type: agent_type.to_string(),
    };
    index
        .begin_insight(&identity, &timestamp())
        .map_err(|error| {
            analytics_error(
                "analytics.storage-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
    let narrative = if let Some(cached) = index.completed_insight(&identity).map_err(|error| {
        analytics_error(
            "analytics.storage-failed",
            json!({ "reason": error.to_string() }),
        )
    })? {
        cached
    } else {
        let projection = projection_from_report(&report);
        let semantic_items = index.semantic_batch(range).map_err(|error| {
            analytics_error(
                "analytics.storage-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
        let semantic_batch = PersonalAnalyticsSemanticBatch {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
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
        let candidate = invoke_agent(
            app,
            agent_config,
            agent_type,
            operation_id,
            operation_dir,
            &projection_path,
            &content_manifest_path,
            &semantic_path,
            &projection,
            report.index_revision,
            &serde_json::to_string(range).unwrap_or_default(),
            false,
            None,
        )?;
        match serde_json::from_str::<PersonalAnalyticsNarrative>(&candidate) {
            Ok(narrative) => narrative,
            Err(first_error) => {
                let invalid_path = operation_dir.join("invalid-report.json");
                std::fs::write(&invalid_path, candidate).map_err(storage_error)?;
                runtime.set_attempt_dir(operation_id, operation_dir.join("repair-attempt"));
                let repaired = invoke_agent(
                    app,
                    agent_config,
                    agent_type,
                    operation_id,
                    operation_dir,
                    &projection_path,
                    &content_manifest_path,
                    &semantic_path,
                    &projection,
                    report.index_revision,
                    &serde_json::to_string(range).unwrap_or_default(),
                    true,
                    Some((&invalid_path, first_error.to_string())),
                )?;
                serde_json::from_str(&repaired).map_err(|error| {
                    analytics_error(
                        "analytics.report-invalid",
                        json!({ "reason": error.to_string() }),
                    )
                })?
            }
        }
    };
    if cancellation.load(Ordering::Acquire) {
        return Err(analytics_error("analytics.cancelled", json!({})));
    }
    index
        .finish_insight(operation_id, &narrative, "completed", None, &timestamp())
        .map_err(|error| {
            analytics_error(
                "analytics.storage-failed",
                json!({ "reason": error.to_string() }),
            )
        })?;
    transition_insight_and_emit(
        app_handle,
        runtime,
        analytics_root,
        operation_id,
        |operation| {
            operation.status = PersonalAnalyticsOperationStatus::Completed;
            operation.progress.stage = PersonalAnalyticsOperationStatus::Completed;
            operation.report_id = Some(report.report_id.clone());
            operation.completed_at = Some(timestamp());
        },
    )?;
    Ok(())
}

fn transition_insight_and_emit(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsInsightRuntime,
    root: &Utf8Path,
    operation_id: &str,
    update: impl FnOnce(&mut PersonalAnalyticsOperation),
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

fn finish_insight_database_operation(
    app: &gold_band::app::App,
    operation_id: &str,
    status: &'static str,
    error_code: Option<String>,
) {
    let Ok(index) = PersonalAnalyticsIndex::open(&app.paths.sqlite_db_path()) else {
        return;
    };
    let narrative = PersonalAnalyticsNarrative {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        insights: Vec::new(),
    };
    let _ = index.finish_insight(
        operation_id,
        &narrative,
        status,
        error_code.as_deref(),
        &timestamp(),
    );
}

fn finish_insight_cancelled(
    app_handle: &AppHandle,
    runtime: &PersonalAnalyticsInsightRuntime,
    root: &Utf8Path,
    operation_id: &str,
) {
    let _ = transition_insight_and_emit(app_handle, runtime, root, operation_id, |operation| {
        operation.status = PersonalAnalyticsOperationStatus::Cancelled;
        operation.progress.stage = PersonalAnalyticsOperationStatus::Cancelled;
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
    let resolved = resolve_attachments(&attachments, "analytics-inputs").map_err(|error| {
        analytics_error(
            "analytics.execution-failed",
            json!({ "reason": error.to_string() }),
        )
    })?;
    let prompt = PromptBundle {
        system_prompt: system_prompt.map_err(prompt_error)?,
        user_prompt: user_prompt.map_err(prompt_error)?,
        display_text: None,
        quotes: Vec::new(),
        prompt_id: Some(Uuid::new_v4().to_string()),
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
        AcpOutputPolicy::ArtifactContract,
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
    json_artifact_text_from_outputs(&run.output.identified_outputs, &run.output.identified_text)
        .ok_or_else(|| analytics_error("analytics.report-invalid", json!({ "reason": "empty" })))
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

fn load_insight_operation(root: &Utf8Path) -> Option<PersonalAnalyticsOperation> {
    let state_path = root.join("insight-state.json");
    let mut operation = read_json::<PersonalAnalyticsOperation>(&state_path).ok()?;
    if operation.status.is_active() {
        operation.status = PersonalAnalyticsOperationStatus::Failed;
        operation.progress.stage = PersonalAnalyticsOperationStatus::Failed;
        operation.revision = operation.revision.saturating_add(1);
        operation.updated_at = timestamp();
        operation.completed_at = Some(operation.updated_at.clone());
        operation.error = Some(analytics_error(
            "analytics.execution-interrupted",
            json!({ "operationId": operation.operation_id }),
        ));
        let _ = write_json(&state_path, &operation);
    }
    Some(operation)
}

fn load_snapshot(root: &Utf8Path) -> PersonalAnalyticsSnapshot {
    let state_path = root.join("state.json");
    let mut snapshot: PersonalAnalyticsSnapshot = read_json(&state_path).unwrap_or_default();
    let report_path = root.join("latest-report.json");
    if report_path.is_file() {
        snapshot.latest_report = read_json(&report_path).ok();
    }
    let mut recovered = false;
    if let Some(operation) = snapshot.operation.as_mut() {
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
    if recovered {
        let _ = write_json(&state_path, &snapshot);
    }
    snapshot
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
        status: PersonalAnalyticsOperationStatus,
        revision: u64,
    ) -> PersonalAnalyticsOperation {
        PersonalAnalyticsOperation {
            operation_id: format!("operation-{revision}"),
            agent_type: "agent-a".to_string(),
            status,
            revision,
            progress: PersonalAnalyticsProgress {
                stage: status,
                processed_units: 0,
                total_units: 0,
            },
            source_watermark: "1".to_string(),
            report_id: None,
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
        assert!(runtime
            .transition(&root, "one", |operation, _| {
                operation.status = PersonalAnalyticsOperationStatus::Failed;
            })
            .unwrap()
            .is_none());
        assert_eq!(
            runtime.snapshot(&root).unwrap().operation.unwrap().revision,
            revision
        );
    }

    #[test]
    fn interrupted_insight_operation_recovers_as_failed_and_allows_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        write_json(
            &root.join("insight-state.json"),
            &insight_operation(PersonalAnalyticsOperationStatus::Analyzing, 2),
        )
        .unwrap();

        let runtime = PersonalAnalyticsInsightRuntime::default();
        let recovered = runtime
            .operation(&root)
            .unwrap()
            .expect("operation is persisted");
        assert_eq!(recovered.status, PersonalAnalyticsOperationStatus::Failed);
        assert_eq!(recovered.revision, 3);
        assert_eq!(
            recovered.error.expect("recovery error").code,
            "analytics.execution-interrupted"
        );

        let restarted = insight_operation(PersonalAnalyticsOperationStatus::Queued, 1);
        let operation_id = restarted.operation_id.clone();
        let (accepted, _) = runtime
            .begin(
                &root,
                restarted,
                root.join("operations").join(operation_id.clone()),
            )
            .unwrap();
        assert_eq!(accepted.status, PersonalAnalyticsOperationStatus::Queued);
        assert_eq!(
            runtime.operation(&root).unwrap().unwrap().operation_id,
            operation_id
        );
    }
}
