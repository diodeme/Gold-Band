use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use gold_band::app::history_deletion::request_run_history_deletion;
use gold_band::app::observability::RuntimeLifecycleBus;
use gold_band::app::{App, DEFAULT_WORKFLOW_TEMPLATE_ID, RuntimeLifecycleEvent};
use gold_band::scheduler::db::{
    ScheduledExecutionHistoryAnchor, ScheduledExecutionHistoryCursor,
    ScheduledExecutionHistoryPage, ScheduledHistoryDeletionOperation, ScheduledJobRecord,
    ScheduledTaskDatabase, UpdateJobResult, derived_next_run_at,
};
use gold_band::scheduler::fingerprint::canonical_content_json;
use gold_band::scheduler::occurrence::{OccurrenceLinks, ScheduledErrorCode, ScheduledOccurrence};
use gold_band::scheduler::{ScheduleError, ScheduledMode, ScheduledTaskDefinition, SessionPolicy};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::view_models_conversation::{
    ConversationCreateInputVm, CreateScheduledTaskInputVm, UpdateScheduledTaskInputVm,
    scheduled_content_snapshot, validate_conversation_create_vm,
};

#[derive(Debug, Clone)]
pub struct ScheduledServiceError {
    pub code: ScheduledErrorCode,
    pub params: Value,
    pub trace_id: Option<String>,
}

impl ScheduledServiceError {
    pub fn new(code: ScheduledErrorCode, params: Value) -> Self {
        Self {
            code,
            params,
            trace_id: None,
        }
    }

    pub(crate) fn from_database(_error: gold_band::scheduler::db::SchedulerDatabaseError) -> Self {
        Self::new(
            ScheduledErrorCode::StorageFailed,
            serde_json::json!({ "operation": "scheduler-database" }),
        )
    }

    fn invalid(operation: &'static str, params: Value) -> Self {
        Self::new(
            ScheduledErrorCode::ValidationFailed,
            serde_json::json!({ "operation": operation, "details": params }),
        )
    }

    fn not_found(project_id: &str, job_id: &str) -> Self {
        Self::new(
            ScheduledErrorCode::NotFound,
            serde_json::json!({
                "operation": "get-job",
                "projectId": project_id,
                "scheduledTaskId": job_id,
            }),
        )
    }

    fn internal(operation: &'static str) -> Self {
        Self::new(
            ScheduledErrorCode::StorageFailed,
            serde_json::json!({ "operation": operation }),
        )
    }
}

impl std::fmt::Display for ScheduledServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.code)
    }
}

impl std::error::Error for ScheduledServiceError {}

pub type ScheduledServiceResult<T> = Result<T, ScheduledServiceError>;
pub type CoordinatorRunFuture =
    Pin<Box<dyn Future<Output = ScheduledServiceResult<ManualRunResult>> + Send + 'static>>;

fn schedule_input_error(error: ScheduleError) -> ScheduledServiceError {
    let params = match error {
        ScheduleError::InvalidCron { expression } => serde_json::json!({
            "field": "schedule.cron",
            "reason": "invalid-cron",
            "expression": expression,
        }),
        ScheduleError::EmptyWeekdays => serde_json::json!({
            "field": "schedule.weekdays",
            "reason": "empty-weekdays",
        }),
        ScheduleError::InvalidEveryValue => serde_json::json!({
            "field": "schedule.every",
            "reason": "invalid-every-value",
        }),
        ScheduleError::UnsupportedEveryUnit { unit } => serde_json::json!({
            "field": "schedule.every",
            "reason": "unsupported-every-unit",
            "unit": unit,
        }),
        ScheduleError::InvalidTimezone { timezone } => serde_json::json!({
            "field": "schedule.timezone",
            "reason": "invalid-timezone",
            "timezone": timezone,
        }),
        ScheduleError::InvalidLocalDate { date } => serde_json::json!({
            "field": "schedule.at",
            "reason": "invalid-date",
            "date": date,
        }),
        ScheduleError::InvalidTime { time } => serde_json::json!({
            "field": "schedule.at",
            "reason": "invalid-time",
            "time": time,
        }),
        ScheduleError::NonexistentLocalTime {
            local_date,
            local_time,
            timezone,
        } => serde_json::json!({
            "field": "schedule.at",
            "reason": "nonexistent-local-time",
            "localDate": local_date,
            "localTime": local_time,
            "timezone": timezone,
        }),
        ScheduleError::EmptyScheduledTaskId => serde_json::json!({
            "field": "scheduledTaskId",
            "reason": "empty-scheduled-task-id",
        }),
        ScheduleError::EmptyProjectId => serde_json::json!({
            "field": "projectId",
            "reason": "empty-project-id",
        }),
        ScheduleError::UnsupportedMode { mode } => serde_json::json!({
            "field": "runMode",
            "reason": "unsupported-mode",
            "mode": mode,
        }),
    };
    ScheduledServiceError::new(ScheduledErrorCode::ValidationFailed, params)
}

#[derive(Debug, Clone)]
pub struct ManualRunResult {
    pub occurrence: ScheduledOccurrence,
    pub immediate_links: Option<OccurrenceLinks>,
}

#[derive(Debug, Clone)]
pub enum SchedulerCommand {
    JobCreated(ScheduledJobRecord),
    JobUpdated(ScheduledJobRecord),
    JobEnabled(ScheduledJobRecord),
    JobDisabled(ScheduledJobRecord),
    JobDeleted(ScheduledTaskDefinition),
}

pub trait ScheduledCoordinator: Send + Sync {
    fn notify(&self, command: SchedulerCommand) -> ScheduledServiceResult<()>;

    fn run_now(&self, app: App, definition: ScheduledTaskDefinition) -> CoordinatorRunFuture;
}

struct ResolvedWorkspace {
    app: App,
    workspace_name: String,
}

type WorkspaceResolver =
    Arc<dyn Fn(&str) -> ScheduledServiceResult<ResolvedWorkspace> + Send + Sync + 'static>;
type WorkspaceLister =
    Arc<dyn Fn() -> ScheduledServiceResult<Vec<ResolvedWorkspace>> + Send + Sync + 'static>;

#[derive(Clone)]
pub struct ScheduledTaskService {
    resolve_workspace: WorkspaceResolver,
    list_workspaces: WorkspaceLister,
    coordinator: Arc<dyn ScheduledCoordinator>,
    lifecycle_bus: RuntimeLifecycleBus,
}

impl ScheduledTaskService {
    pub fn desktop(app_handle: AppHandle) -> Self {
        let lifecycle_bus = app_handle
            .state::<crate::state::DesktopState>()
            .lifecycle_bus();
        let resolve_handle = app_handle.clone();
        let resolve_workspace = Arc::new(move |project_id: &str| {
            resolve_conversation_workspace(&resolve_handle, project_id)
        });
        let list_handle = app_handle.clone();
        let list_workspaces = Arc::new(move || list_conversation_workspaces(&list_handle));
        Self {
            resolve_workspace,
            list_workspaces,
            coordinator: Arc::new(DesktopScheduledCoordinator { app_handle }),
            lifecycle_bus,
        }
    }

    #[cfg(test)]
    fn for_test(
        app: &App,
        workspace_name: &str,
        coordinator: Arc<dyn ScheduledCoordinator>,
    ) -> Self {
        Self::for_test_workspaces(&[(app, workspace_name)], coordinator)
    }

    #[cfg(test)]
    fn for_test_workspaces(
        workspaces: &[(&App, &str)],
        coordinator: Arc<dyn ScheduledCoordinator>,
    ) -> Self {
        let lifecycle_bus = workspaces
            .first()
            .map(|(app, _)| app.lifecycle_bus.clone())
            .unwrap_or_default();
        let specifications = workspaces
            .iter()
            .map(|(app, name)| {
                (
                    app.paths.project_id.clone(),
                    app.paths.repo_root.clone(),
                    app.config.clone(),
                    (*name).to_string(),
                )
            })
            .collect::<Vec<_>>();
        let resolve_specifications = specifications.clone();
        let resolve_workspace = Arc::new(move |requested: &str| {
            let (_, repo_root, config, workspace_name) = resolve_specifications
                .iter()
                .find(|(project_id, _, _, _)| project_id == requested)
                .ok_or_else(|| ScheduledServiceError::not_found(requested, ""))?;
            Ok(ResolvedWorkspace {
                app: App::with_config(repo_root.clone(), config.clone()),
                workspace_name: workspace_name.clone(),
            })
        });
        let list_workspaces = Arc::new(move || {
            Ok(specifications
                .iter()
                .map(|(_, repo_root, config, workspace_name)| ResolvedWorkspace {
                    app: App::with_config(repo_root.clone(), config.clone()),
                    workspace_name: workspace_name.clone(),
                })
                .collect())
        });
        Self {
            resolve_workspace,
            list_workspaces,
            coordinator,
            lifecycle_bus,
        }
    }

    pub fn list(
        &self,
        project_id: Option<&str>,
    ) -> ScheduledServiceResult<Vec<ScheduledJobRecord>> {
        let workspaces = match project_id {
            Some(project_id) => match (self.resolve_workspace)(project_id) {
                Ok(workspace) => vec![workspace],
                Err(error) if error.code == ScheduledErrorCode::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error),
            },
            None => (self.list_workspaces)()?,
        };
        let mut records = Vec::new();
        for workspace in workspaces {
            let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
                .map_err(ScheduledServiceError::from_database)?;
            records.extend(
                database
                    .list_job_records_for_project(&workspace.app.paths.project_id)
                    .map_err(ScheduledServiceError::from_database)?,
            );
        }
        records.sort_by_key(|record| record.definition.created_at);
        Ok(records)
    }

    pub fn get(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> ScheduledServiceResult<ScheduledJobRecord> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))
    }

    pub fn workspace_name(&self, project_id: &str) -> ScheduledServiceResult<String> {
        Ok((self.resolve_workspace)(project_id)?.workspace_name)
    }

    pub fn list_occurrence_page(
        &self,
        project_id: &str,
        job_id: &str,
        status: Option<gold_band::scheduler::occurrence::OccurrenceStatus>,
        cursor: Option<&gold_band::scheduler::db::OccurrencePageCursor>,
    ) -> ScheduledServiceResult<gold_band::scheduler::db::OccurrencePage> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))?;
        database
            .list_occurrence_page(
                &resolved_project_id,
                job_id,
                status,
                cursor,
                gold_band::scheduler::db::OCCURRENCE_HISTORY_PAGE_SIZE,
            )
            .map_err(ScheduledServiceError::from_database)
    }

    pub fn list_execution_history_page(
        &self,
        project_id: &str,
        scheduled_task_id: &str,
        cursor: Option<&ScheduledExecutionHistoryCursor>,
    ) -> ScheduledServiceResult<ScheduledExecutionHistoryPage> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        database
            .list_execution_history_page(
                &resolved_project_id,
                scheduled_task_id,
                cursor,
                gold_band::scheduler::db::OCCURRENCE_HISTORY_PAGE_SIZE,
            )
            .map_err(ScheduledServiceError::from_database)
    }

    pub fn list_execution_history_page_anchored(
        &self,
        project_id: &str,
        scheduled_task_id: &str,
        cursor: Option<&ScheduledExecutionHistoryCursor>,
        anchor: Option<(&str, &str)>,
    ) -> ScheduledServiceResult<ScheduledExecutionHistoryPage> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let anchor_cursor = if cursor.is_none() {
            match anchor {
                Some((task_id, run_id)) => match database
                    .execution_history_cursor_before_run(
                        &resolved_project_id,
                        scheduled_task_id,
                        task_id,
                        run_id,
                    )
                    .map_err(ScheduledServiceError::from_database)?
                {
                    ScheduledExecutionHistoryAnchor::Missing => {
                        return Err(ScheduledServiceError::new(
                            ScheduledErrorCode::NotFound,
                            serde_json::json!({
                                "operation": "list-execution-history",
                                "projectId": project_id,
                                "scheduledTaskId": scheduled_task_id,
                                "taskId": task_id,
                                "runId": run_id,
                            }),
                        ));
                    }
                    ScheduledExecutionHistoryAnchor::Newest => None,
                    ScheduledExecutionHistoryAnchor::After(cursor) => Some(cursor),
                },
                None => None,
            }
        } else {
            None
        };
        database
            .list_execution_history_page(
                &resolved_project_id,
                scheduled_task_id,
                cursor.or(anchor_cursor.as_ref()),
                gold_band::scheduler::db::OCCURRENCE_HISTORY_PAGE_SIZE,
            )
            .map_err(ScheduledServiceError::from_database)
    }

    pub fn request_execution_history_deletion(
        &self,
        project_id: &str,
        scheduled_task_id: &str,
        task_id: &str,
        run_id: &str,
    ) -> ScheduledServiceResult<ScheduledHistoryDeletionOperation> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let operation = database
            .create_or_get_history_deletion(
                &resolved_project_id,
                scheduled_task_id,
                task_id,
                run_id,
            )
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| {
                ScheduledServiceError::new(
                    ScheduledErrorCode::NotFound,
                    serde_json::json!({
                        "operation": "delete-execution-history",
                        "projectId": project_id,
                        "scheduledTaskId": scheduled_task_id,
                        "taskId": task_id,
                        "runId": run_id,
                    }),
                )
            })?;
        let action_result = request_run_history_deletion(&workspace.app, &operation);
        let operation = database
            .get_history_deletion(&operation.operation_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::internal("load-history-deletion"))?;
        if action_result.is_err() && operation.last_error.is_none() {
            return Err(ScheduledServiceError::internal("delete-execution-history"));
        }
        Ok(operation)
    }

    pub(crate) fn reconcile_history_deletion_stop(
        &self,
        operation: &ScheduledHistoryDeletionOperation,
    ) -> ScheduledServiceResult<ScheduledHistoryDeletionOperation> {
        let workspace = (self.resolve_workspace)(&operation.project_id)?;
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let _ = crate::commands::reconcile_history_deletion_stop(&workspace.app, operation);
        database
            .get_history_deletion(&operation.operation_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::internal("load-history-deletion"))
    }

    pub fn occurrence_diagnostics(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> ScheduledServiceResult<(u64, Vec<ScheduledOccurrence>)> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))?;
        let run_count = database
            .count_run_occurrences(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?;
        let page = database
            .list_occurrence_page(
                &resolved_project_id,
                job_id,
                None,
                None,
                gold_band::scheduler::db::OCCURRENCE_HISTORY_PAGE_SIZE,
            )
            .map_err(ScheduledServiceError::from_database)?;
        Ok((run_count, page.items))
    }

    pub fn create(
        &self,
        input: CreateScheduledTaskInputVm,
    ) -> ScheduledServiceResult<ScheduledJobRecord> {
        let schedule = input
            .schedule
            .try_into_schedule_spec()
            .map_err(schedule_input_error)?;
        let workspace = (self.resolve_workspace)(&input.project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let validation_input = ConversationCreateInputVm {
            project_id: resolved_project_id.clone(),
            content: input.content.clone(),
            run_mode: input.run_mode.clone(),
            workflow_template_id: input.workflow_template_id.clone(),
            include_optional_entry: input.include_optional_entry,
            direct_config: input.direct_config.clone(),
            auto_config: input.auto_config.clone(),
            attachment_paths: input.attachment_paths.clone(),
            work_location: Default::default(),
            branch_checkpoint: None,
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
            workflow_authoring: None,
        };
        let validation = validate_conversation_create_vm(&workspace.app, &validation_input)
            .map_err(|_| {
                ScheduledServiceError::invalid(
                    "validate-create",
                    serde_json::json!({ "projectId": resolved_project_id }),
                )
            })?;
        if !validation.valid {
            return Err(ScheduledServiceError::invalid(
                "validate-create",
                serde_json::json!({
                    "codes": validation
                        .missing_items
                        .into_iter()
                        .map(|item| item.code)
                        .collect::<Vec<_>>()
                }),
            ));
        }

        let id = format!("scheduled-{}", Uuid::new_v4());
        let session_policy = input.session_policy.unwrap_or(SessionPolicy::New);
        let mut definition = ScheduledTaskDefinition::new(
            &resolved_project_id,
            &id,
            &input.run_mode,
            schedule,
            input.overlap_policy,
        )
        .and_then(|definition| definition.with_session_policy(session_policy))
        .map_err(|_| {
            ScheduledServiceError::invalid(
                "build-definition",
                serde_json::json!({ "projectId": resolved_project_id }),
            )
        })?;
        definition.instruction = input.content;
        definition.content_snapshot = scheduled_content_snapshot(&workspace.app, &validation_input)
            .map_err(|_| {
                ScheduledServiceError::invalid(
                    "build-content-snapshot",
                    serde_json::json!({ "projectId": resolved_project_id }),
                )
            })?;
        definition.recompute_content_fingerprint().map_err(|_| {
            ScheduledServiceError::invalid(
                "fingerprint-content",
                serde_json::json!({ "projectId": resolved_project_id }),
            )
        })?;
        let effective_optional_entry = effective_optional_entry_choice(
            &workspace.app,
            &input.run_mode,
            input.workflow_template_id.as_deref(),
            input.include_optional_entry,
        )?;
        definition.execution_config = serde_json::json!({
            "runMode": input.run_mode,
            "workflowTemplateId": input.workflow_template_id,
            "includeOptionalEntry": effective_optional_entry,
            "directConfig": input.direct_config,
            "autoConfig": input.auto_config,
        });
        let job_dir = workspace.app.paths.scheduled_task_dir(&id);
        let staging_dir = job_dir.join(format!("inputs.staging-{}", Uuid::new_v4().simple()));
        let input_dir = job_dir.join("inputs");
        std::fs::create_dir_all(staging_dir.as_std_path()).map_err(|_| {
            ScheduledServiceError::invalid(
                "stage-inputs",
                serde_json::json!({ "scheduledTaskId": id }),
            )
        })?;
        if let Err(error) = copy_attachments(
            input.attachment_paths.as_deref().unwrap_or_default(),
            &staging_dir,
            &mut definition.attachment_names,
        ) {
            let _ = std::fs::remove_dir_all(job_dir.as_std_path());
            return Err(error);
        }
        if std::fs::rename(staging_dir.as_std_path(), input_dir.as_std_path()).is_err() {
            let _ = std::fs::remove_dir_all(job_dir.as_std_path());
            return Err(ScheduledServiceError::invalid(
                "commit-inputs",
                serde_json::json!({ "scheduledTaskId": id }),
            ));
        }

        let record = persist_created_job_transactionally(&job_dir, || {
            let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
                .map_err(ScheduledServiceError::from_database)?;
            database
                .create_job(&definition, derived_next_run_at(&definition))
                .map_err(ScheduledServiceError::from_database)
        })?;
        self.lifecycle_bus
            .emit(RuntimeLifecycleEvent::ScheduledTaskCreated {
                project_id: record.definition.project_id.clone(),
                scheduled_task_id: record.definition.id.clone(),
            });
        self.coordinator
            .notify(SchedulerCommand::JobCreated(record.clone()))?;
        let _ = workspace.workspace_name;
        Ok(record)
    }

    pub fn update(
        &self,
        input: UpdateScheduledTaskInputVm,
    ) -> ScheduledServiceResult<ScheduledJobRecord> {
        let schedule = input
            .schedule
            .try_into_schedule_spec()
            .map_err(schedule_input_error)?;
        let workspace = (self.resolve_workspace)(&input.project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let current = database
            .get_job_definition(&resolved_project_id, &input.scheduled_task_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| {
                ScheduledServiceError::not_found(&input.project_id, &input.scheduled_task_id)
            })?;
        let expected_updated_at = chrono::DateTime::parse_from_rfc3339(&input.expected_updated_at)
            .map_err(|_| {
                ScheduledServiceError::invalid(
                    "parse-expected-updated-at",
                    serde_json::json!({ "expectedUpdatedAt": input.expected_updated_at }),
                )
            })?
            .with_timezone(&chrono::Utc);
        if current.definition.updated_at != expected_updated_at {
            return Err(conflict_error(&current));
        }

        let replacement_paths = input.attachment_paths.clone();
        let attachment_paths = replacement_paths.clone().unwrap_or_else(|| {
            current
                .definition
                .attachment_names
                .iter()
                .map(|name| {
                    workspace
                        .app
                        .paths
                        .scheduled_task_dir(&current.definition.id)
                        .join("inputs")
                        .join(name)
                        .to_string()
                })
                .collect()
        });
        let validation_input = ConversationCreateInputVm {
            project_id: input.project_id.clone(),
            content: input.content.clone(),
            run_mode: input.run_mode.clone(),
            workflow_template_id: input.workflow_template_id.clone(),
            include_optional_entry: input.include_optional_entry,
            direct_config: input.direct_config.clone(),
            auto_config: input.auto_config.clone(),
            attachment_paths: Some(attachment_paths),
            work_location: Default::default(),
            branch_checkpoint: None,
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
            workflow_authoring: None,
        };
        let validation = validate_conversation_create_vm(&workspace.app, &validation_input)
            .map_err(|_| {
                ScheduledServiceError::invalid(
                    "validate-update",
                    serde_json::json!({ "scheduledTaskId": input.scheduled_task_id }),
                )
            })?;
        if !validation.valid {
            return Err(ScheduledServiceError::invalid(
                "validate-update",
                serde_json::json!({
                    "codes": validation
                        .missing_items
                        .into_iter()
                        .map(|item| item.code)
                        .collect::<Vec<_>>()
                }),
            ));
        }
        if current.definition.mode == ScheduledMode::Direct {
            let old_agent = current
                .definition
                .content_snapshot
                .direct_agent_id
                .as_deref();
            let new_agent = input
                .direct_config
                .as_ref()
                .map(|config| config.agent_type.trim());
            if old_agent != new_agent {
                return Err(ScheduledServiceError::new(
                    ScheduledErrorCode::ValidationFailed,
                    serde_json::json!({
                        "field": "directAgentId",
                        "scheduledTaskId": input.scheduled_task_id,
                    }),
                ));
            }
        }

        let mut definition = current.definition.clone();
        let previous_schedule = definition.schedule.clone();
        let previous_mode = definition.mode;
        let new_snapshot =
            scheduled_content_snapshot(&workspace.app, &validation_input).map_err(|_| {
                ScheduledServiceError::invalid(
                    "build-content-snapshot",
                    serde_json::json!({ "scheduledTaskId": input.scheduled_task_id }),
                )
            })?;
        let content_changed = canonical_content_json(&new_snapshot)
            != canonical_content_json(&definition.content_snapshot);
        definition.content_snapshot = new_snapshot;
        if content_changed || definition.content_fingerprint.trim().is_empty() {
            definition.recompute_content_fingerprint().map_err(|_| {
                ScheduledServiceError::invalid(
                    "fingerprint-content",
                    serde_json::json!({ "scheduledTaskId": input.scheduled_task_id }),
                )
            })?;
        }
        definition.instruction = input.content;
        definition.schedule = schedule;
        definition.overlap_policy = input.overlap_policy;
        let next_mode = scheduled_mode_from_run_mode(&input.run_mode);
        let mut policy_definition = definition.clone();
        policy_definition.mode = next_mode;
        let session_policy = policy_definition
            .with_session_policy(input.session_policy)
            .map_err(|_| {
                ScheduledServiceError::invalid(
                    "validate-session-policy",
                    serde_json::json!({ "scheduledTaskId": input.scheduled_task_id }),
                )
            })?
            .session_policy;
        let session_policy_changed = definition.session_policy != session_policy;
        definition.session_policy = session_policy;
        definition.mode = next_mode;
        if should_reset_task_association(
            previous_mode,
            next_mode,
            content_changed,
            session_policy_changed,
        ) {
            definition.task_id = None;
        }
        let effective_optional_entry = effective_optional_entry_choice(
            &workspace.app,
            &input.run_mode,
            input.workflow_template_id.as_deref(),
            input.include_optional_entry,
        )?;
        definition.execution_config = serde_json::json!({
            "runMode": input.run_mode,
            "workflowTemplateId": input.workflow_template_id,
            "includeOptionalEntry": effective_optional_entry,
            "directConfig": input.direct_config,
            "autoConfig": input.auto_config,
        });
        let now = chrono::Utc::now();
        definition.updated_at = if now > expected_updated_at {
            now
        } else {
            expected_updated_at + chrono::Duration::milliseconds(1)
        };
        let definition_id = definition.id().to_string();
        let mut swap = if let Some(paths) = replacement_paths {
            Some(stage_replacement_inputs(
                &workspace.app,
                &definition_id,
                &paths,
                &mut definition.attachment_names,
            )?)
        } else {
            None
        };
        // next_run_at 单一维护者：只有 schedule 真正变化时才基于 now 重算下一次触发；
        // 改 instruction / 附件 / 模式等非调度字段时保留 scheduler 已推进的 next_run_at，
        // 避免编辑把调度时机重置或倒退。
        let next_run_at = if definition.schedule != previous_schedule {
            if definition.enabled {
                definition.schedule.next_occurrence_after(now)
            } else {
                None
            }
        } else {
            current.next_run_at
        };
        let update_result = database
            .update_job(&definition, expected_updated_at, next_run_at)
            .map_err(ScheduledServiceError::from_database);
        let record = match update_result {
            Ok(UpdateJobResult::Updated(record)) => {
                if let Some(swap) = swap.take() {
                    swap.commit()?;
                }
                record
            }
            Ok(UpdateJobResult::Conflict(record)) => {
                if let Some(swap) = swap.take() {
                    swap.rollback()?;
                }
                return Err(conflict_error(&record));
            }
            Ok(UpdateJobResult::NotFound) => {
                if let Some(swap) = swap.take() {
                    swap.rollback()?;
                }
                return Err(ScheduledServiceError::not_found(
                    &input.project_id,
                    &input.scheduled_task_id,
                ));
            }
            Err(error) => {
                if let Some(swap) = swap.take() {
                    swap.rollback()?;
                }
                return Err(error);
            }
        };
        self.coordinator
            .notify(SchedulerCommand::JobUpdated(record.clone()))?;
        Ok(record)
    }

    pub fn set_enabled(
        &self,
        project_id: &str,
        job_id: &str,
        enabled: bool,
    ) -> ScheduledServiceResult<ScheduledJobRecord> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let current = database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))?;
        if current.definition.enabled == enabled {
            return Ok(current);
        }
        let expected_updated_at = current.definition.updated_at;
        let mut definition = current.definition;
        definition.enabled = enabled;
        let now = chrono::Utc::now();
        // 重新启用时，所有 schedule 类型都从「当前时刻」计算下一次触发，而不是沿用停用前的
        // last_trigger_at 作基准——否则 Repeat/Cron 会算出停用期间的一个过去点作为 next_run_at，
        // 被 coordinator 当作 missed 回填（产生错过的历史记录 + 通知）。Every 同样从 now 起算，
        // 不再单独重置 anchor（next_occurrence_after(now) 已覆盖）。
        let next_run_at = if enabled {
            definition.schedule.next_occurrence_after(now)
        } else {
            None
        };
        definition.updated_at = if now > expected_updated_at {
            now
        } else {
            expected_updated_at + chrono::Duration::milliseconds(1)
        };
        let record = match database
            .update_job(&definition, expected_updated_at, next_run_at)
            .map_err(ScheduledServiceError::from_database)?
        {
            UpdateJobResult::Updated(record) => record,
            UpdateJobResult::Conflict(record) => return Err(conflict_error(&record)),
            UpdateJobResult::NotFound => {
                return Err(ScheduledServiceError::not_found(project_id, job_id));
            }
        };
        self.coordinator.notify(if enabled {
            SchedulerCommand::JobEnabled(record.clone())
        } else {
            SchedulerCommand::JobDisabled(record.clone())
        })?;
        Ok(record)
    }

    pub fn delete(&self, project_id: &str, job_id: &str) -> ScheduledServiceResult<()> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let record = database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))?;
        let input_dir = workspace
            .app
            .paths
            .scheduled_task_dir(job_id)
            .join("inputs");
        let deleted = delete_input_snapshot_transactionally(&input_dir, || {
            database
                .delete_job(&resolved_project_id, job_id)
                .map_err(ScheduledServiceError::from_database)
        })?;
        if !deleted {
            return Err(ScheduledServiceError::not_found(project_id, job_id));
        }
        self.coordinator
            .notify(SchedulerCommand::JobDeleted(record.definition))?;
        Ok(())
    }

    pub async fn run_now(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> ScheduledServiceResult<ManualRunResult> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        let record = database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))?;
        self.coordinator
            .run_now(workspace.app, record.definition)
            .await
    }
}

fn effective_optional_entry_choice(
    app: &App,
    run_mode: &str,
    template_id: Option<&str>,
    requested: Option<bool>,
) -> ScheduledServiceResult<Option<bool>> {
    if run_mode != "workflow" {
        return Ok(None);
    }
    let template_id = template_id.unwrap_or(DEFAULT_WORKFLOW_TEMPLATE_ID);
    let store = app.workflow_templates().map_err(|_| {
        ScheduledServiceError::invalid(
            "resolve-workflow-template",
            serde_json::json!({ "workflowTemplateId": template_id }),
        )
    })?;
    let template = store
        .templates
        .iter()
        .find(|template| template.id == template_id)
        .ok_or_else(|| {
            ScheduledServiceError::invalid(
                "resolve-workflow-template",
                serde_json::json!({ "workflowTemplateId": template_id }),
            )
        })?;
    Ok(template
        .optional_entry_stage
        .as_ref()
        .map(|stage| requested.unwrap_or(stage.default_enabled)))
}

struct DesktopScheduledCoordinator {
    app_handle: AppHandle,
}

impl ScheduledCoordinator for DesktopScheduledCoordinator {
    fn notify(&self, command: SchedulerCommand) -> ScheduledServiceResult<()> {
        let (definition, next_run_at, runtime_command) = match command {
            SchedulerCommand::JobCreated(record) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &record.definition)?;
                (
                    record.definition,
                    record.next_run_at,
                    crate::scheduled_runtime::SchedulerCommand::JobCreated { key },
                )
            }
            SchedulerCommand::JobUpdated(record) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &record.definition)?;
                (
                    record.definition,
                    record.next_run_at,
                    crate::scheduled_runtime::SchedulerCommand::JobUpdated { key },
                )
            }
            SchedulerCommand::JobEnabled(record) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &record.definition)?;
                (
                    record.definition,
                    record.next_run_at,
                    crate::scheduled_runtime::SchedulerCommand::JobEnabled { key },
                )
            }
            SchedulerCommand::JobDisabled(record) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &record.definition)?;
                (
                    record.definition,
                    record.next_run_at,
                    crate::scheduled_runtime::SchedulerCommand::JobDisabled { key },
                )
            }
            SchedulerCommand::JobDeleted(definition) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &definition)?;
                (
                    definition,
                    None,
                    crate::scheduled_runtime::SchedulerCommand::JobDeleted { key },
                )
            }
        };
        let deleted = matches!(
            &runtime_command,
            crate::scheduled_runtime::SchedulerCommand::JobDeleted { .. }
        );
        self.app_handle
            .state::<crate::state::DesktopState>()
            .scheduler_coordinator()
            .map_err(|_| ScheduledServiceError::internal("get-scheduler-coordinator"))?
            .send(runtime_command)?;
        if deleted {
            crate::scheduled_runtime::emit_scheduled_task_deleted(&self.app_handle, &definition);
        } else {
            crate::scheduled_runtime::emit_scheduled_task_updated(
                &self.app_handle,
                &definition,
                next_run_at,
            );
        }
        Ok(())
    }

    fn run_now(&self, app: App, definition: ScheduledTaskDefinition) -> CoordinatorRunFuture {
        let handle = self
            .app_handle
            .state::<crate::state::DesktopState>()
            .scheduler_coordinator()
            .map_err(|_| ScheduledServiceError::internal("get-scheduler-coordinator"));
        let key = gold_band::scheduler::coordinator::ScheduledJobKey::new(
            app.paths.repo_root.clone(),
            definition.project_id,
            definition.id,
        );
        Box::pin(async move { handle?.run_now(key).await })
    }
}

fn scheduled_job_key_for_definition(
    app_handle: &AppHandle,
    definition: &ScheduledTaskDefinition,
) -> ScheduledServiceResult<gold_band::scheduler::coordinator::ScheduledJobKey> {
    let workspace = resolve_conversation_workspace(app_handle, &definition.project_id)?;
    Ok(gold_band::scheduler::coordinator::ScheduledJobKey::new(
        workspace.app.paths.repo_root,
        definition.project_id.clone(),
        definition.id.clone(),
    ))
}

fn resolve_conversation_workspace(
    app_handle: &AppHandle,
    project_id: &str,
) -> ScheduledServiceResult<ResolvedWorkspace> {
    let state = app_handle.state::<crate::state::DesktopState>();
    let context = state
        .context()
        .map_err(|_| ScheduledServiceError::internal("read-desktop-context"))?;
    let global_app = state
        .app()
        .map_err(|_| ScheduledServiceError::internal("resolve-runtime-app"))?;
    let app_state = global_app
        .load_state()
        .map_err(|_| ScheduledServiceError::internal("read-workspace-state"))?;
    let Some((workspace_path, resolved_project_id)) =
        crate::conversation_workspace::workspace_entry_for_project(&app_state, project_id)
    else {
        return Err(ScheduledServiceError::not_found(project_id, ""));
    };
    let app = global_app.with_repo_root(
        camino::Utf8PathBuf::from(workspace_path),
        context.config.clone(),
    );
    let workspace_name = app_state
        .conversation_workspaces
        .iter()
        .find(|workspace| workspace.project_id == resolved_project_id)
        .map(|workspace| workspace.name.clone())
        .unwrap_or(resolved_project_id);
    Ok(ResolvedWorkspace {
        app,
        workspace_name,
    })
}

fn list_conversation_workspaces(
    app_handle: &AppHandle,
) -> ScheduledServiceResult<Vec<ResolvedWorkspace>> {
    let state = app_handle.state::<crate::state::DesktopState>();
    let context = state
        .context()
        .map_err(|_| ScheduledServiceError::internal("read-desktop-context"))?;
    let global_app = state
        .app()
        .map_err(|_| ScheduledServiceError::internal("resolve-runtime-app"))?;
    let app_state = global_app
        .load_state()
        .map_err(|_| ScheduledServiceError::internal("read-workspace-state"))?;
    app_state
        .conversation_workspaces
        .iter()
        .map(|workspace| {
            Ok(ResolvedWorkspace {
                app: global_app.with_repo_root(
                    camino::Utf8PathBuf::from(&workspace.workspace_path),
                    context.config.clone(),
                ),
                workspace_name: workspace.name.clone(),
            })
        })
        .collect()
}

fn persist_created_job_transactionally<T, F>(
    job_dir: &camino::Utf8Path,
    persist_database: F,
) -> ScheduledServiceResult<T>
where
    F: FnOnce() -> ScheduledServiceResult<T>,
{
    match persist_database() {
        Ok(value) => Ok(value),
        Err(error) => {
            if job_dir.exists() && std::fs::remove_dir_all(job_dir.as_std_path()).is_err() {
                return Err(ScheduledServiceError::new(
                    ScheduledErrorCode::AttachmentFailed,
                    serde_json::json!({ "operation": "rollback-created-inputs" }),
                ));
            }
            Err(error)
        }
    }
}

fn conflict_error(record: &ScheduledJobRecord) -> ScheduledServiceError {
    ScheduledServiceError::new(
        ScheduledErrorCode::Conflict,
        serde_json::json!({
            "scheduledTaskId": record.definition.id,
            "updatedAt": record.definition.updated_at,
            "revision": record.revision,
        }),
    )
}

fn scheduled_mode_from_run_mode(value: &str) -> ScheduledMode {
    match value {
        "workflow" => ScheduledMode::Workflow,
        "auto" => ScheduledMode::Auto,
        _ => ScheduledMode::Direct,
    }
}

fn should_reset_task_association(
    previous_mode: ScheduledMode,
    next_mode: ScheduledMode,
    content_changed: bool,
    session_policy_changed: bool,
) -> bool {
    previous_mode != next_mode
        || (next_mode != ScheduledMode::Direct && (content_changed || session_policy_changed))
}

struct InputSnapshotSwap {
    input_dir: camino::Utf8PathBuf,
    backup_dir: Option<camino::Utf8PathBuf>,
}

impl InputSnapshotSwap {
    fn commit(self) -> ScheduledServiceResult<()> {
        if let Some(backup) = self.backup_dir {
            if backup.exists() {
                std::fs::remove_dir_all(backup.as_std_path()).map_err(|_| {
                    ScheduledServiceError::new(
                        ScheduledErrorCode::AttachmentFailed,
                        serde_json::json!({ "operation": "cleanup-input-backup" }),
                    )
                })?;
            }
        }
        Ok(())
    }

    fn rollback(self) -> ScheduledServiceResult<()> {
        if self.input_dir.exists() {
            std::fs::remove_dir_all(self.input_dir.as_std_path()).map_err(|_| {
                ScheduledServiceError::new(
                    ScheduledErrorCode::AttachmentFailed,
                    serde_json::json!({ "operation": "rollback-inputs" }),
                )
            })?;
        }
        if let Some(backup) = self.backup_dir {
            std::fs::rename(backup.as_std_path(), self.input_dir.as_std_path()).map_err(|_| {
                ScheduledServiceError::new(
                    ScheduledErrorCode::AttachmentFailed,
                    serde_json::json!({ "operation": "restore-inputs" }),
                )
            })?;
        }
        Ok(())
    }
}

fn stage_replacement_inputs(
    app: &App,
    job_id: &str,
    sources: &[String],
    attachment_names: &mut Vec<String>,
) -> ScheduledServiceResult<InputSnapshotSwap> {
    let job_dir = app.paths.scheduled_task_dir(job_id);
    let input_dir = job_dir.join("inputs");
    let staging_dir = job_dir.join(format!("inputs.staging-{}", Uuid::new_v4().simple()));
    let backup_dir = job_dir.join(format!("inputs.backup-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(staging_dir.as_std_path()).map_err(|_| {
        ScheduledServiceError::new(
            ScheduledErrorCode::AttachmentFailed,
            serde_json::json!({ "operation": "stage-inputs" }),
        )
    })?;
    let mut names = Vec::new();
    if let Err(error) = copy_attachments(sources, &staging_dir, &mut names) {
        let _ = std::fs::remove_dir_all(staging_dir.as_std_path());
        return Err(error);
    }
    let backup = if input_dir.exists() {
        std::fs::rename(input_dir.as_std_path(), backup_dir.as_std_path()).map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::AttachmentFailed,
                serde_json::json!({ "operation": "backup-inputs" }),
            )
        })?;
        Some(backup_dir)
    } else {
        None
    };
    if std::fs::rename(staging_dir.as_std_path(), input_dir.as_std_path()).is_err() {
        if let Some(backup) = backup.as_ref() {
            let _ = std::fs::rename(backup.as_std_path(), input_dir.as_std_path());
        }
        let _ = std::fs::remove_dir_all(staging_dir.as_std_path());
        return Err(ScheduledServiceError::new(
            ScheduledErrorCode::AttachmentFailed,
            serde_json::json!({ "operation": "commit-inputs" }),
        ));
    }
    *attachment_names = names;
    Ok(InputSnapshotSwap {
        input_dir,
        backup_dir: backup,
    })
}

fn delete_input_snapshot_transactionally<F>(
    input_dir: &camino::Utf8Path,
    delete_database: F,
) -> ScheduledServiceResult<bool>
where
    F: FnOnce() -> ScheduledServiceResult<bool>,
{
    let tombstone =
        input_dir.with_file_name(format!("inputs.tombstone-{}", Uuid::new_v4().simple()));
    let moved = input_dir.exists();
    if moved {
        std::fs::rename(input_dir.as_std_path(), tombstone.as_std_path()).map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::AttachmentFailed,
                serde_json::json!({ "operation": "tombstone-inputs" }),
            )
        })?;
    }
    match delete_database() {
        Ok(true) => {
            if moved {
                std::fs::remove_dir_all(tombstone.as_std_path()).map_err(|_| {
                    ScheduledServiceError::new(
                        ScheduledErrorCode::AttachmentFailed,
                        serde_json::json!({ "operation": "cleanup-input-tombstone" }),
                    )
                })?;
            }
            Ok(true)
        }
        Ok(false) => {
            if moved {
                std::fs::rename(tombstone.as_std_path(), input_dir.as_std_path()).map_err(
                    |_| {
                        ScheduledServiceError::new(
                            ScheduledErrorCode::AttachmentFailed,
                            serde_json::json!({ "operation": "restore-inputs" }),
                        )
                    },
                )?;
            }
            Ok(false)
        }
        Err(error) => {
            if moved && std::fs::rename(tombstone.as_std_path(), input_dir.as_std_path()).is_err() {
                return Err(ScheduledServiceError::new(
                    ScheduledErrorCode::AttachmentFailed,
                    serde_json::json!({ "operation": "restore-inputs" }),
                ));
            }
            Err(error)
        }
    }
}

fn copy_attachments(
    sources: &[String],
    destination: &camino::Utf8Path,
    attachment_names: &mut Vec<String>,
) -> ScheduledServiceResult<()> {
    for source in sources {
        let source_path = Path::new(source);
        let Some(name) = source_path.file_name().and_then(|name| name.to_str()) else {
            return Err(ScheduledServiceError::new(
                ScheduledErrorCode::AttachmentFailed,
                serde_json::json!({ "operation": "copy-attachment" }),
            ));
        };
        if attachment_names.iter().any(|existing| existing == name) {
            return Err(ScheduledServiceError::new(
                ScheduledErrorCode::AttachmentFailed,
                serde_json::json!({
                    "operation": "copy-attachment",
                    "attachmentName": name,
                }),
            ));
        }
        std::fs::copy(source_path, destination.join(name).as_std_path()).map_err(|_| {
            ScheduledServiceError::new(
                ScheduledErrorCode::AttachmentFailed,
                serde_json::json!({
                    "operation": "copy-attachment",
                    "attachmentName": name,
                }),
            )
        })?;
        attachment_names.push(name.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use chrono::{Duration, TimeZone, Utc};
    use gold_band::app::{App, DEFAULT_WORKFLOW_TEMPLATE_ID, RuntimeLifecycleEvent};
    use gold_band::config::{ProviderDiagnosticSnapshot, RuntimeConfig};
    use gold_band::dsl::NodeDsl;
    use gold_band::scheduler::db::{
        AcceptExecutionResult, ScheduledJobRecord, ScheduledTaskDatabase, UpdateJobResult,
    };
    use gold_band::scheduler::execution::ScheduledExecutionSnapshot;
    use gold_band::scheduler::occurrence::{
        ClaimResult, OccurrenceLinks, OccurrenceTriggerKind, ScheduledErrorCode,
        ScheduledOccurrence,
    };
    use gold_band::scheduler::{
        LocalTimeDisambiguation, OverlapPolicy, RepeatPreset, ScheduleKind, ScheduledMode,
        ScheduledTaskDefinition, SessionPolicy,
    };
    use gold_band::workflow_model_binding::{WorkerModelBinding, WorkflowModelBindings};
    use tempfile::TempDir;
    use tokio::sync::Barrier;

    use super::{
        ManualRunResult, ScheduledCoordinator, ScheduledTaskService, SchedulerCommand,
        should_reset_task_association,
    };
    use crate::view_models_conversation::{
        ConversationDirectConfigVm, CreateScheduledTaskInputVm, ScheduledEveryInputVm,
        ScheduledScheduleInputVm, UpdateScheduledTaskInputVm,
    };

    #[derive(Default)]
    struct CoordinatorSpy {
        database: Mutex<Option<ScheduledTaskDatabase>>,
        start_count: Mutex<usize>,
        commands: Mutex<Vec<SchedulerCommand>>,
        fail_notify: bool,
    }

    impl CoordinatorSpy {
        fn with_database(database: ScheduledTaskDatabase) -> Self {
            Self {
                database: Mutex::new(Some(database)),
                ..Self::default()
            }
        }

        fn with_failing_notify(database: ScheduledTaskDatabase) -> Self {
            Self {
                database: Mutex::new(Some(database)),
                fail_notify: true,
                ..Self::default()
            }
        }

        fn start_count(&self) -> usize {
            *self.start_count.lock().unwrap()
        }

        fn command_count(&self) -> usize {
            self.commands.lock().unwrap().len()
        }

        fn last_command(&self) -> SchedulerCommand {
            self.commands.lock().unwrap().last().unwrap().clone()
        }
    }

    impl ScheduledCoordinator for CoordinatorSpy {
        fn notify(&self, command: SchedulerCommand) -> super::ScheduledServiceResult<()> {
            self.commands.lock().unwrap().push(command);
            if self.fail_notify {
                return Err(super::ScheduledServiceError::internal("notify-created-job"));
            }
            Ok(())
        }

        fn run_now(
            &self,
            _app: App,
            definition: ScheduledTaskDefinition,
        ) -> super::CoordinatorRunFuture {
            *self.start_count.lock().unwrap() += 1;
            let database = self.database.lock().unwrap().clone().unwrap();
            Box::pin(async move {
                let occurrence = database
                    .create_or_get_occurrence_for_existing_job(
                        &definition.project_id,
                        definition.id(),
                        Utc::now(),
                        OccurrenceTriggerKind::Manual,
                    )
                    .map_err(super::ScheduledServiceError::from_database)?
                    .ok_or_else(|| {
                        super::ScheduledServiceError::new(
                            ScheduledErrorCode::NotFound,
                            serde_json::json!({ "scheduledTaskId": definition.id() }),
                        )
                    })?;
                Ok(ManualRunResult {
                    occurrence,
                    immediate_links: None,
                })
            })
        }
    }

    struct Fixture {
        _temp: TempDir,
        app: App,
        database: ScheduledTaskDatabase,
        coordinator: Arc<CoordinatorSpy>,
        service: ScheduledTaskService,
    }

    fn valid_schedule_input() -> ScheduledScheduleInputVm {
        ScheduledScheduleInputVm::At {
            local_date: "2099-01-01".to_string(),
            local_time: "09:00".to_string(),
            timezone: "UTC".to_string(),
            disambiguation: LocalTimeDisambiguation::Earlier,
        }
    }

    impl Fixture {
        fn new() -> Self {
            Self::with_options(true, false)
        }

        fn with_notify_failure(fail_notify: bool) -> Self {
            Self::with_options(true, fail_notify)
        }

        fn with_configured_workflow(configured: bool) -> Self {
            Self::with_options(configured, false)
        }

        fn with_options(configured: bool, fail_notify: bool) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
            let config = RuntimeConfig::default().with_provider_diagnostics(BTreeMap::from([(
                "claude-acp".to_string(),
                ProviderDiagnosticSnapshot {
                    available: true,
                    reason: None,
                    checked_at: "2026-08-14T00:00:00Z".to_string(),
                    capabilities: Some(serde_json::json!({
                        "configOptions": [{
                            "id": "model",
                            "category": "model",
                            "options": [
                                { "value": "sonnet", "name": "Sonnet" },
                                { "value": "opus", "name": "Opus" }
                            ]
                        }]
                    })),
                },
            )]));
            let app = App::with_config(root, config);
            if configured {
                let template = app
                    .workflow_templates()
                    .unwrap()
                    .templates
                    .into_iter()
                    .find(|template| template.id == DEFAULT_WORKFLOW_TEMPLATE_ID)
                    .unwrap();
                let bindings = template
                    .workflow
                    .nodes
                    .iter()
                    .filter_map(|node| match node {
                        NodeDsl::Worker(worker) => Some(WorkerModelBinding {
                            execution_slot_id: worker.execution_slot_id.clone().unwrap(),
                            agent_id: "claude-acp".to_string(),
                            model_id: None,
                            permission_mode_id: None,
                            config_options: BTreeMap::new(),
                        }),
                        NodeDsl::AiDynamic(_) => None,
                    })
                    .collect();
                app.update_built_in_workflow_template_bindings(
                    DEFAULT_WORKFLOW_TEMPLATE_ID,
                    WorkflowModelBindings {
                        bindings,
                        ..WorkflowModelBindings::default()
                    },
                )
                .unwrap();
            }
            let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
            let coordinator = Arc::new(if fail_notify {
                CoordinatorSpy::with_failing_notify(database.clone())
            } else {
                CoordinatorSpy::with_database(database.clone())
            });
            let service =
                ScheduledTaskService::for_test(&app, "Test workspace", coordinator.clone());
            Self {
                _temp: temp,
                app,
                database,
                coordinator,
                service,
            }
        }

        fn create_input(&self) -> CreateScheduledTaskInputVm {
            CreateScheduledTaskInputVm {
                project_id: self.app.paths.project_id.clone(),
                content: "Generate the scheduled report".to_string(),
                run_mode: "workflow".to_string(),
                workflow_template_id: Some(DEFAULT_WORKFLOW_TEMPLATE_ID.to_string()),
                include_optional_entry: Some(false),
                direct_config: None,
                auto_config: None,
                attachment_paths: None,
                schedule: valid_schedule_input(),
                overlap_policy: OverlapPolicy::SkipWhenRunning,
                session_policy: None,
            }
        }

        fn update_input(
            &self,
            definition: &ScheduledTaskDefinition,
            content: &str,
        ) -> UpdateScheduledTaskInputVm {
            UpdateScheduledTaskInputVm {
                scheduled_task_id: definition.id.clone(),
                project_id: definition.project_id.clone(),
                expected_updated_at: definition.updated_at.to_rfc3339(),
                content: content.to_string(),
                run_mode: "workflow".to_string(),
                workflow_template_id: Some(DEFAULT_WORKFLOW_TEMPLATE_ID.to_string()),
                include_optional_entry: Some(false),
                direct_config: None,
                auto_config: None,
                attachment_paths: None,
                schedule: valid_schedule_input(),
                overlap_policy: definition.overlap_policy,
                session_policy: SessionPolicy::New,
            }
        }

        fn associate_task(
            &self,
            record: &ScheduledJobRecord,
            fingerprint: &str,
        ) -> ScheduledJobRecord {
            let mut definition = record.definition.clone();
            definition.task_id = Some("task-existing".to_string());
            definition.content_fingerprint = fingerprint.to_string();
            match self
                .database
                .update_job_runtime_projection(&definition, record.revision)
                .unwrap()
            {
                UpdateJobResult::Updated(record) => record,
                other => panic!("expected updated projection, got {other:?}"),
            }
        }
    }

    #[test]
    fn execution_history_service_works_after_definition_delete() {
        let fixture = Fixture::new();
        let now = Utc.with_ymd_and_hms(2026, 8, 25, 10, 0, 0).unwrap();
        let mut definition = ScheduledTaskDefinition::new(
            &fixture.app.paths.project_id,
            "scheduled-history",
            "direct",
            gold_band::scheduler::ScheduleSpec::at(now),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.task_id = Some("task-history".to_string());
        definition.content_snapshot.instruction = "history".to_string();
        definition.recompute_content_fingerprint().unwrap();
        let record = fixture.database.create_job(&definition, None).unwrap();
        let occurrence = fixture
            .database
            .create_or_get_occurrence_for_existing_job(
                &fixture.app.paths.project_id,
                definition.id(),
                now,
                OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();
        assert!(matches!(
            fixture
                .database
                .claim_occurrence(
                    &fixture.app.paths.project_id,
                    &occurrence.id,
                    "history-service-test",
                    now - Duration::seconds(1),
                    now + Duration::minutes(5),
                )
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let links = OccurrenceLinks {
            task_id: Some("task-history".to_string()),
            run_id: Some("run-history".to_string()),
            round_id: Some("round-1".to_string()),
            node_id: Some("node-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
        };
        let snapshot = ScheduledExecutionSnapshot {
            accepted_at: now,
            definition_revision: record.revision,
            content_fingerprint: definition.content_fingerprint.clone(),
            content: definition.content_snapshot.clone(),
            instruction_summary: "history".to_string(),
            automatic: None,
        };
        assert!(matches!(
            fixture
                .database
                .accept_occurrence_execution(
                    &fixture.app.paths.project_id,
                    &occurrence.id,
                    "history-service-test",
                    record.revision,
                    &links,
                    &snapshot,
                )
                .unwrap(),
            AcceptExecutionResult::Accepted(_)
        ));
        assert!(
            fixture
                .database
                .delete_job(&fixture.app.paths.project_id, definition.id())
                .unwrap()
        );

        let page = fixture
            .service
            .list_execution_history_page(&fixture.app.paths.project_id, definition.id(), None)
            .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].run_id, "run-history");
        let anchored = fixture
            .service
            .list_execution_history_page_anchored(
                &fixture.app.paths.project_id,
                definition.id(),
                None,
                Some(("task-history", "run-history")),
            )
            .unwrap();
        assert_eq!(anchored.items[0].run_id, "run-history");
        let missing = fixture
            .service
            .list_execution_history_page_anchored(
                &fixture.app.paths.project_id,
                definition.id(),
                None,
                Some(("task-missing", "run-missing")),
            )
            .unwrap_err();
        assert_eq!(missing.code, ScheduledErrorCode::NotFound);
        assert_eq!(missing.params["taskId"], "task-missing");
        assert_eq!(missing.params["runId"], "run-missing");
    }

    #[test]
    fn task_association_reset_policy_is_owned_by_the_service() {
        assert!(!should_reset_task_association(
            ScheduledMode::Direct,
            ScheduledMode::Direct,
            true,
            true,
        ));
        assert!(should_reset_task_association(
            ScheduledMode::Workflow,
            ScheduledMode::Workflow,
            true,
            false,
        ));
        assert!(should_reset_task_association(
            ScheduledMode::Direct,
            ScheduledMode::Workflow,
            false,
            false,
        ));
        assert!(should_reset_task_association(
            ScheduledMode::Workflow,
            ScheduledMode::Direct,
            false,
            false,
        ));
    }

    #[test]
    fn create_only_persists_definition_and_inputs() {
        let fixture = Fixture::new();
        let attachment = fixture.app.paths.repo_root.join("report.txt");
        std::fs::write(attachment.as_std_path(), b"stable scheduled input").unwrap();
        let mut input = fixture.create_input();
        input.attachment_paths = Some(vec![attachment.to_string()]);

        let result = fixture.service.create(input).unwrap();

        assert!(result.definition.task_id.is_none());
        assert!(
            fixture
                .database
                .list_occurrences(&result.definition.project_id, result.definition.id(), 10,)
                .unwrap()
                .is_empty()
        );
        assert_eq!(fixture.coordinator.start_count(), 0);
        assert_eq!(
            result.definition.content_snapshot.attachment_hashes.len(),
            1
        );
        assert!(result.definition.content_fingerprint.starts_with("sha256:"));
        assert!(
            fixture
                .app
                .paths
                .scheduled_task_dir(result.definition.id())
                .join("inputs/report.txt")
                .is_file()
        );
    }

    #[test]
    fn create_direct_preserves_the_user_selected_permission_mode_without_capability_gating() {
        let fixture = Fixture::new();
        let mut input = fixture.create_input();
        input.run_mode = "direct".to_string();
        input.workflow_template_id = None;
        input.include_optional_entry = None;
        input.direct_config = Some(ConversationDirectConfigVm {
            agent_type: "claude-acp".to_string(),
            model_id: Some("sonnet".to_string()),
            permission_mode: Some("plan".to_string()),
            config_options: BTreeMap::new(),
        });

        let created = fixture.service.create(input).unwrap();

        assert_eq!(created.definition.mode, ScheduledMode::Direct);
        assert_eq!(
            created.definition.execution_config["directConfig"]["permissionMode"],
            "plan"
        );
        assert_eq!(
            created
                .definition
                .content_snapshot
                .direct_agent_id
                .as_deref(),
            Some("claude-acp")
        );
    }

    #[test]
    fn durable_create_fact_is_published_before_coordinator_notification() {
        let fixture = Fixture::with_notify_failure(true);
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_subscriber = events.clone();
        fixture
            .app
            .lifecycle_bus
            .subscribe_inline(Arc::new(move |event| {
                if let RuntimeLifecycleEvent::ScheduledTaskCreated {
                    project_id,
                    scheduled_task_id,
                } = event
                {
                    events_for_subscriber
                        .lock()
                        .unwrap()
                        .push((project_id, scheduled_task_id));
                }
            }));

        let error = fixture.service.create(fixture.create_input()).unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::StorageFailed);
        let definitions = fixture
            .database
            .list_job_definitions_for_project(&fixture.app.paths.project_id)
            .unwrap();
        assert_eq!(
            definitions.len(),
            1,
            "durable create must survive notify failure"
        );
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[(definitions[0].project_id.clone(), definitions[0].id.clone())]
        );
    }

    #[test]
    fn create_notification_keeps_the_persisted_next_run_at() {
        let fixture = Fixture::new();

        let created = fixture.service.create(fixture.create_input()).unwrap();

        match fixture.coordinator.last_command() {
            SchedulerCommand::JobCreated(record) => {
                assert_eq!(record.next_run_at, created.next_run_at);
                assert!(record.next_run_at.is_some());
            }
            command => panic!("expected JobCreated, got {command:?}"),
        }
    }

    #[test]
    fn create_rejects_invalid_cron_before_persisting_or_notifying() {
        let fixture = Fixture::new();
        let mut input = fixture.create_input();
        input.schedule = ScheduledScheduleInputVm::Cron {
            expression: "not a cron".to_string(),
            timezone: "UTC".to_string(),
        };

        let error = fixture.service.create(input).unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert_eq!(error.params["field"], "schedule.cron");
        assert_eq!(error.params["reason"], "invalid-cron");
        assert!(
            fixture
                .database
                .list_job_definitions_for_project(&fixture.app.paths.project_id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(fixture.coordinator.command_count(), 0);
    }

    #[test]
    fn create_rejects_empty_weekly_days() {
        let fixture = Fixture::new();
        let mut input = fixture.create_input();
        input.schedule = ScheduledScheduleInputVm::Repeat {
            preset: RepeatPreset::Weekly {
                weekdays: Vec::new(),
            },
            hour: 9,
            minute: 0,
            timezone: "UTC".to_string(),
        };

        let error = fixture.service.create(input).unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert_eq!(error.params["field"], "schedule.weekdays");
        assert_eq!(error.params["reason"], "empty-weekdays");
    }

    #[test]
    fn create_rejects_zero_every_value() {
        let fixture = Fixture::new();
        let mut input = fixture.create_input();
        input.schedule = ScheduledScheduleInputVm::Every {
            every: ScheduledEveryInputVm {
                value: 0,
                unit: "minutes".to_string(),
            },
            anchor_at: Utc::now(),
            timezone: "UTC".to_string(),
        };

        let error = fixture.service.create(input).unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert_eq!(error.params["field"], "schedule.every");
        assert_eq!(error.params["reason"], "invalid-every-value");
    }

    #[test]
    fn create_normalizes_local_at_to_utc_and_keeps_timezone() {
        let fixture = Fixture::new();
        let mut input = fixture.create_input();
        input.schedule = ScheduledScheduleInputVm::At {
            local_date: "2026-11-01".to_string(),
            local_time: "01:30".to_string(),
            timezone: "America/New_York".to_string(),
            disambiguation: LocalTimeDisambiguation::Later,
        };

        let created = fixture.service.create(input).unwrap();
        let ScheduleKind::At { at, timezone } = created.definition.schedule.kind else {
            panic!("expected At schedule");
        };
        assert_eq!(at, Utc.with_ymd_and_hms(2026, 11, 1, 6, 30, 0).unwrap());
        assert_eq!(timezone, "America/New_York");
    }

    #[test]
    fn workflow_schedule_freezes_optional_entry_choice_and_effective_workflow() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();

        assert_eq!(
            created.definition.execution_config["includeOptionalEntry"],
            serde_json::json!(false)
        );
        let authoring: gold_band::workflow_model_binding::TaskAuthoringWorkflow =
            serde_json::from_value(
                created
                    .definition
                    .content_snapshot
                    .workflow_authoring
                    .clone()
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(authoring.workflow.entry, "plan");
        assert!(
            !authoring
                .workflow
                .nodes
                .iter()
                .any(|node| node.id() == "interview")
        );
        assert_eq!(
            authoring.model_bindings.bindings.len(),
            authoring
                .workflow
                .nodes
                .iter()
                .filter(|node| matches!(node, gold_band::dsl::NodeDsl::Worker(_)))
                .count()
        );

        let mut default_input = fixture.create_input();
        default_input.include_optional_entry = None;
        let defaulted = fixture.service.create(default_input).unwrap();
        assert_eq!(
            defaulted.definition.execution_config["includeOptionalEntry"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn workflow_schedule_rejects_an_unconfigured_model_binding() {
        let fixture = Fixture::with_configured_workflow(false);

        let error = fixture.service.create(fixture.create_input()).unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert!(
            error.params["details"]["codes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|code| code == "workflow-model-binding.agent-required")
        );
    }

    #[tokio::test]
    async fn run_now_creates_manual_occurrence_without_advancing_planned_deadline() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let job_id = created.definition.id().to_string();
        let before = fixture
            .database
            .get_job_definition(&fixture.app.paths.project_id, &job_id)
            .unwrap()
            .unwrap()
            .next_run_at;

        let run = fixture
            .service
            .run_now(&fixture.app.paths.project_id, &job_id)
            .await
            .unwrap();
        let after = fixture
            .database
            .get_job_definition(&fixture.app.paths.project_id, &job_id)
            .unwrap()
            .unwrap()
            .next_run_at;

        assert_eq!(run.occurrence.trigger_kind, OccurrenceTriggerKind::Manual);
        assert_eq!(before, after);
        assert_eq!(fixture.coordinator.start_count(), 1);
        assert_eq!(
            fixture
                .database
                .list_occurrences(&fixture.app.paths.project_id, &job_id, 10)
                .unwrap()
                .into_iter()
                .map(|occurrence: ScheduledOccurrence| occurrence.trigger_kind)
                .collect::<Vec<_>>(),
            vec![OccurrenceTriggerKind::Manual]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn schedule_edit_cancels_old_unaccepted_automatic_occurrence() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let scheduled_at = Utc::now();
        let occurrence = fixture
            .database
            .create_or_get_occurrence_for_existing_job(
                &fixture.app.paths.project_id,
                created.definition.id(),
                scheduled_at,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let claim_complete = Arc::new(Barrier::new(2));
        let authoring_complete = Arc::new(Barrier::new(2));
        let worker_database = fixture.database.clone();
        let worker_project_id = fixture.app.paths.project_id.clone();
        let worker_occurrence_id = occurrence.id.clone();
        let worker_claim_complete = claim_complete.clone();
        let worker_authoring_complete = authoring_complete.clone();
        let worker = tokio::spawn(async move {
            assert!(matches!(
                worker_database
                    .claim_occurrence(
                        &worker_project_id,
                        &worker_occurrence_id,
                        "schedule-edit-owner",
                        scheduled_at,
                        scheduled_at + Duration::minutes(5),
                    )
                    .unwrap(),
                ClaimResult::Claimed(_)
            ));
            worker_claim_complete.wait().await;
            worker_authoring_complete.wait().await;
            worker_database
                .get_occurrence(&worker_project_id, &worker_occurrence_id)
                .unwrap()
        });

        claim_complete.wait().await;
        let mut update = fixture.update_input(&created.definition, "new schedule instruction");
        update.schedule = ScheduledScheduleInputVm::At {
            local_date: "2099-01-02".to_string(),
            local_time: "09:00".to_string(),
            timezone: "UTC".to_string(),
            disambiguation: LocalTimeDisambiguation::Earlier,
        };
        fixture.service.update(update).unwrap();
        authoring_complete.wait().await;

        assert!(worker.await.unwrap().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disable_rejects_a_concurrent_unaccepted_automatic_claim() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let scheduled_at = Utc::now();
        let occurrence = fixture
            .database
            .create_or_get_occurrence_for_existing_job(
                &fixture.app.paths.project_id,
                created.definition.id(),
                scheduled_at,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap()
            .unwrap();
        let claim_complete = Arc::new(Barrier::new(2));
        let authoring_complete = Arc::new(Barrier::new(2));
        let worker_database = fixture.database.clone();
        let worker_project_id = fixture.app.paths.project_id.clone();
        let worker_occurrence_id = occurrence.id.clone();
        let worker_claim_complete = claim_complete.clone();
        let worker_authoring_complete = authoring_complete.clone();
        let worker = tokio::spawn(async move {
            assert!(matches!(
                worker_database
                    .claim_occurrence(
                        &worker_project_id,
                        &worker_occurrence_id,
                        "disable-owner",
                        scheduled_at,
                        scheduled_at + Duration::minutes(5),
                    )
                    .unwrap(),
                ClaimResult::Claimed(_)
            ));
            worker_claim_complete.wait().await;
            worker_authoring_complete.wait().await;
            worker_database
                .get_occurrence(&worker_project_id, &worker_occurrence_id)
                .unwrap()
        });

        claim_complete.wait().await;
        fixture
            .service
            .set_enabled(
                &fixture.app.paths.project_id,
                created.definition.id(),
                false,
            )
            .unwrap();
        authoring_complete.wait().await;

        assert!(worker.await.unwrap().is_none());
    }

    #[test]
    fn update_rejects_a_stale_optimistic_token() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let stale = fixture.update_input(&created.definition, "stale update");
        let first = fixture
            .service
            .update(fixture.update_input(&created.definition, "first update"))
            .unwrap();

        assert_ne!(first.definition.updated_at, created.definition.updated_at);
        let error = fixture.service.update(stale).unwrap_err();
        assert_eq!(error.code, ScheduledErrorCode::Conflict);
        assert_eq!(
            error.params["scheduledTaskId"],
            serde_json::json!(created.definition.id)
        );
        assert_eq!(
            error.params["updatedAt"],
            serde_json::json!(first.definition.updated_at)
        );
    }

    #[test]
    fn workflow_execution_binding_update_reuses_task_and_legacy_fingerprint() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let associated = fixture.associate_task(&created, "sha256:legacy-fingerprint");
        let mut template = fixture
            .app
            .workflow_templates()
            .unwrap()
            .templates
            .into_iter()
            .find(|template| template.id == DEFAULT_WORKFLOW_TEMPLATE_ID)
            .unwrap();
        for binding in &mut template.model_bindings.bindings {
            binding.model_id = Some("sonnet".to_string());
        }
        fixture
            .app
            .update_built_in_workflow_template_bindings(
                DEFAULT_WORKFLOW_TEMPLATE_ID,
                template.model_bindings,
            )
            .unwrap();

        let updated = fixture
            .service
            .update(
                fixture.update_input(&associated.definition, &associated.definition.instruction),
            )
            .unwrap();
        let authoring: gold_band::workflow_model_binding::TaskAuthoringWorkflow =
            serde_json::from_value(
                updated
                    .definition
                    .content_snapshot
                    .workflow_authoring
                    .clone()
                    .unwrap(),
            )
            .unwrap();

        assert_eq!(updated.definition.task_id.as_deref(), Some("task-existing"));
        assert_eq!(
            updated.definition.content_fingerprint,
            "sha256:legacy-fingerprint"
        );
        assert!(
            authoring
                .model_bindings
                .bindings
                .iter()
                .all(|binding| binding.model_id.as_deref() == Some("sonnet"))
        );
    }

    #[test]
    fn workflow_semantic_content_update_clears_task_and_recomputes_fingerprint() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let associated = fixture.associate_task(&created, "sha256:legacy-fingerprint");

        let updated = fixture
            .service
            .update(fixture.update_input(
                &associated.definition,
                "Generate a different scheduled report",
            ))
            .unwrap();

        assert_eq!(updated.definition.task_id, None);
        assert_ne!(
            updated.definition.content_fingerprint,
            "sha256:legacy-fingerprint"
        );
    }

    #[test]
    fn update_rejects_invalid_schedule_without_mutating_the_job() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let command_count = fixture.coordinator.command_count();
        let mut input = fixture.update_input(&created.definition, "unchanged");
        input.schedule = ScheduledScheduleInputVm::Cron {
            expression: "invalid".to_string(),
            timezone: "UTC".to_string(),
        };

        let error = fixture.service.update(input).unwrap_err();
        let persisted = fixture
            .database
            .get_job_definition(&created.definition.project_id, created.definition.id())
            .unwrap()
            .unwrap();

        assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
        assert_eq!(persisted.definition.schedule, created.definition.schedule);
        assert_eq!(fixture.coordinator.command_count(), command_count);
    }

    #[test]
    fn update_preserves_next_run_at_when_schedule_is_unchanged() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        // 模拟 scheduler 已经把 next_run_at 推进到一个明确值（例如 occurrence materialize 后）。
        let advanced = Utc.with_ymd_and_hms(2099, 6, 1, 12, 0, 0).unwrap();
        let mut definition = created.definition.clone();
        definition.updated_at = Utc::now();
        fixture
            .database
            .update_job(&definition, created.definition.updated_at, Some(advanced))
            .unwrap();
        // update_input 的 expected_updated_at 需基于当前 record（被上面 update_job 推进过）。
        let current = fixture
            .database
            .get_job_definition(&created.definition.project_id, created.definition.id())
            .unwrap()
            .unwrap();

        // 编辑 instruction（非 schedule 字段），schedule 保持与创建时一致。
        let updated = fixture
            .service
            .update(fixture.update_input(&current.definition, "edited content"))
            .unwrap();

        // next_run_at 必须保留为 scheduler 推进的值，不能被 derived 覆盖/倒退。
        assert_eq!(updated.next_run_at, Some(advanced));
    }

    #[test]
    fn schedule_edits_ignore_stale_last_trigger_for_repeat_cron_and_every() {
        let schedules = vec![
            ScheduledScheduleInputVm::Repeat {
                preset: RepeatPreset::Daily,
                hour: 9,
                minute: 0,
                timezone: "UTC".to_string(),
            },
            ScheduledScheduleInputVm::Cron {
                expression: "0 0 9 * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            ScheduledScheduleInputVm::Every {
                every: ScheduledEveryInputVm {
                    value: 1,
                    unit: "hours".to_string(),
                },
                anchor_at: Utc::now() - Duration::days(30),
                timezone: "UTC".to_string(),
            },
        ];

        for schedule in schedules {
            let fixture = Fixture::new();
            let created = fixture.service.create(fixture.create_input()).unwrap();
            let mut stale = created.definition.clone();
            stale.last_trigger_at = Some(Utc::now() - Duration::days(30));
            stale.updated_at = created.definition.updated_at + Duration::seconds(1);
            fixture
                .database
                .update_job(
                    &stale,
                    created.definition.updated_at,
                    Some(Utc::now() - Duration::days(29)),
                )
                .unwrap();
            let current = fixture
                .database
                .get_job_definition(&created.definition.project_id, created.definition.id())
                .unwrap()
                .unwrap();
            let mut input = fixture.update_input(&current.definition, "edited schedule");
            input.schedule = schedule;
            let before = Utc::now();

            let updated = fixture.service.update(input).unwrap();
            let next_run_at = updated
                .next_run_at
                .expect("an enabled recurring schedule must keep a future deadline");

            assert!(
                next_run_at >= before,
                "schedule edit must derive its deadline from now: {next_run_at} < {before}"
            );
        }
    }

    #[test]
    fn enable_and_pause_update_the_sqlite_deadline() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();

        let paused = fixture
            .service
            .set_enabled(
                &created.definition.project_id,
                created.definition.id(),
                false,
            )
            .unwrap();
        assert!(!paused.definition.enabled);
        assert_eq!(paused.next_run_at, None);

        let enabled = fixture
            .service
            .set_enabled(
                &created.definition.project_id,
                created.definition.id(),
                true,
            )
            .unwrap();
        assert!(enabled.definition.enabled);
        assert!(enabled.next_run_at.is_some());
    }

    #[test]
    fn setting_the_existing_enabled_state_is_idempotent() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let advanced = Utc.with_ymd_and_hms(2099, 6, 1, 12, 0, 0).unwrap();
        let mut definition = created.definition.clone();
        definition.updated_at = created.definition.updated_at + Duration::seconds(1);
        let current = match fixture
            .database
            .update_job(&definition, created.definition.updated_at, Some(advanced))
            .unwrap()
        {
            UpdateJobResult::Updated(record) => record,
            result => panic!("expected updated record, got {result:?}"),
        };
        let command_count = fixture.coordinator.command_count();

        let unchanged_enabled = fixture
            .service
            .set_enabled(
                &current.definition.project_id,
                current.definition.id(),
                true,
            )
            .unwrap();

        assert_eq!(unchanged_enabled, current);
        assert_eq!(unchanged_enabled.next_run_at, Some(advanced));
        assert_eq!(fixture.coordinator.command_count(), command_count);

        let disabled = fixture
            .service
            .set_enabled(
                &current.definition.project_id,
                current.definition.id(),
                false,
            )
            .unwrap();
        let disabled_command_count = fixture.coordinator.command_count();
        let unchanged_disabled = fixture
            .service
            .set_enabled(
                &current.definition.project_id,
                current.definition.id(),
                false,
            )
            .unwrap();

        assert_eq!(unchanged_disabled, disabled);
        assert_eq!(fixture.coordinator.command_count(), disabled_command_count);
    }

    #[test]
    fn reenabling_repeat_job_schedules_next_run_from_now_not_from_stale_last_trigger() {
        let fixture = Fixture::new();
        let mut input = fixture.create_input();
        input.schedule = ScheduledScheduleInputVm::Repeat {
            preset: RepeatPreset::Daily,
            hour: 9,
            minute: 0,
            timezone: "UTC".to_string(),
        };
        let created = fixture.service.create(input).unwrap();
        // 模拟「停用前」的状态：把 last_trigger_at 设到很远的过去。
        let mut stale = created.definition.clone();
        stale.last_trigger_at = Some(Utc::now() - Duration::days(30));
        stale.enabled = false;
        stale.updated_at = Utc::now();
        fixture
            .database
            .update_job(&stale, created.definition.updated_at, None)
            .unwrap();

        let before = Utc::now();
        let enabled = fixture
            .service
            .set_enabled(
                &created.definition.project_id,
                created.definition.id(),
                true,
            )
            .unwrap();

        // 重新启用后，next_run_at 必须从「当前时刻」起算（未来点），
        // 而不是基于 30 天前的 last_trigger_at 算出停用期间的一个过去点。
        let next_run_at = enabled
            .next_run_at
            .expect("enabled repeat job must have a future next_run_at");
        assert!(
            next_run_at >= before,
            "next_run_at must not regress into the disabled window: {next_run_at} < {before}"
        );
    }

    #[test]
    fn list_and_get_are_project_scoped() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();

        assert_eq!(
            fixture
                .service
                .list(Some(&created.definition.project_id))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fixture
                .service
                .list(Some("different-project"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            fixture
                .service
                .get(&created.definition.project_id, created.definition.id())
                .unwrap()
                .definition
                .id,
            created.definition.id
        );
        let error = fixture
            .service
            .get("different-project", created.definition.id())
            .unwrap_err();
        assert_eq!(error.code, ScheduledErrorCode::NotFound);
    }

    #[test]
    fn list_without_project_aggregates_every_workspace() {
        let first = Fixture::new();
        let second = Fixture::new();
        let first_app = &first.app;
        let second_app = &second.app;
        let coordinator = Arc::new(CoordinatorSpy::default());
        let service = ScheduledTaskService::for_test_workspaces(
            &[(first_app, "First"), (second_app, "Second")],
            coordinator,
        );
        let input_for = |app: &App, content: &str| CreateScheduledTaskInputVm {
            project_id: app.paths.project_id.clone(),
            content: content.to_string(),
            run_mode: "workflow".to_string(),
            workflow_template_id: Some(DEFAULT_WORKFLOW_TEMPLATE_ID.to_string()),
            include_optional_entry: Some(false),
            direct_config: None,
            auto_config: None,
            attachment_paths: None,
            schedule: valid_schedule_input(),
            overlap_policy: OverlapPolicy::SkipWhenRunning,
            session_policy: None,
        };
        service.create(input_for(first_app, "first")).unwrap();
        service.create(input_for(second_app, "second")).unwrap();

        assert_eq!(service.list(None).unwrap().len(), 2);
        assert_eq!(
            service
                .list(Some(&first_app.paths.project_id))
                .unwrap()
                .len(),
            1
        );
        let second = service.list(Some(&second_app.paths.project_id)).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].definition.instruction, "second");
    }

    #[test]
    fn delete_removes_definition_occurrences_and_inputs_but_keeps_task_history() {
        let fixture = Fixture::new();
        let created = fixture.service.create(fixture.create_input()).unwrap();
        let task_dir = fixture.app.paths.task_dir("task-history");
        std::fs::create_dir_all(task_dir.as_std_path()).unwrap();
        std::fs::write(task_dir.join("task.json").as_std_path(), b"history").unwrap();
        fixture
            .database
            .create_or_get_occurrence_for_existing_job(
                &created.definition.project_id,
                created.definition.id(),
                Utc::now(),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();

        fixture
            .service
            .delete(&created.definition.project_id, created.definition.id())
            .unwrap();

        assert!(
            fixture
                .database
                .get_job_definition(&created.definition.project_id, created.definition.id())
                .unwrap()
                .is_none()
        );
        assert!(
            fixture
                .database
                .list_occurrences(&created.definition.project_id, created.definition.id(), 10,)
                .unwrap()
                .is_empty()
        );
        assert!(
            !fixture
                .app
                .paths
                .scheduled_task_dir(created.definition.id())
                .join("inputs")
                .exists()
        );
        assert!(task_dir.join("task.json").is_file());
    }

    #[test]
    fn delete_tombstone_restores_inputs_when_database_delete_fails() {
        let fixture = Fixture::new();
        let input_dir = fixture
            .app
            .paths
            .scheduled_task_dir("job-delete-failure")
            .join("inputs");
        std::fs::create_dir_all(input_dir.as_std_path()).unwrap();
        std::fs::write(input_dir.join("input.txt").as_std_path(), b"keep me").unwrap();

        let error = super::delete_input_snapshot_transactionally(&input_dir, || {
            Err::<bool, _>(super::ScheduledServiceError::from_database(
                gold_band::scheduler::db::SchedulerDatabaseError::InvalidValue(
                    "forced-delete-failure".to_string(),
                ),
            ))
        })
        .unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::StorageFailed);
        assert!(input_dir.join("input.txt").is_file());
        let job_dir = input_dir.parent().unwrap();
        assert!(
            std::fs::read_dir(job_dir.as_std_path())
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().contains("tombstone"))
        );
    }

    #[test]
    fn create_database_open_failure_removes_only_the_new_job_directory() {
        let fixture = Fixture::new();
        let job_dir = fixture.app.paths.scheduled_task_dir("job-open-failure");
        let sibling_dir = fixture.app.paths.scheduled_task_dir("job-existing");
        std::fs::create_dir_all(job_dir.join("inputs").as_std_path()).unwrap();
        std::fs::create_dir_all(sibling_dir.join("inputs").as_std_path()).unwrap();

        let error = super::persist_created_job_transactionally(&job_dir, || {
            Err::<(), _>(super::ScheduledServiceError::from_database(
                gold_band::scheduler::db::SchedulerDatabaseError::InvalidValue(
                    "forced-open-failure".to_string(),
                ),
            ))
        })
        .unwrap_err();

        assert_eq!(error.code, ScheduledErrorCode::StorageFailed);
        assert!(!job_dir.exists());
        assert!(sibling_dir.join("inputs").is_dir());
    }
}
