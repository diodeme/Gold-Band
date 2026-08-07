use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use gold_band::app::App;
use gold_band::scheduler::db::{
    ScheduledJobRecord, ScheduledTaskDatabase, UpdateJobResult, derived_next_run_at,
};
use gold_band::scheduler::occurrence::{OccurrenceLinks, ScheduledErrorCode, ScheduledOccurrence};
use gold_band::scheduler::{ScheduleKind, ScheduledMode, ScheduledTaskDefinition, SessionPolicy};
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

#[derive(Debug, Clone)]
pub struct ManualRunResult {
    pub occurrence: ScheduledOccurrence,
    pub immediate_links: Option<OccurrenceLinks>,
}

#[derive(Debug, Clone)]
pub enum SchedulerCommand {
    JobCreated(ScheduledTaskDefinition),
    JobUpdated(ScheduledTaskDefinition),
    JobEnabled(ScheduledTaskDefinition),
    JobDisabled(ScheduledTaskDefinition),
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
}

impl ScheduledTaskService {
    pub fn desktop(app_handle: AppHandle) -> Self {
        let resolve_handle = app_handle.clone();
        let resolve_workspace = Arc::new(move |project_id: &str| {
            resolve_desktop_workspace(&resolve_handle, project_id)
        });
        let list_handle = app_handle.clone();
        let list_workspaces = Arc::new(move || list_desktop_workspaces(&list_handle));
        Self {
            resolve_workspace,
            list_workspaces,
            coordinator: Arc::new(DesktopScheduledCoordinator { app_handle }),
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
        }
    }

    pub fn list(
        &self,
        project_id: Option<&str>,
    ) -> ScheduledServiceResult<Vec<ScheduledTaskDefinition>> {
        let workspaces = match project_id {
            Some(project_id) => match (self.resolve_workspace)(project_id) {
                Ok(workspace) => vec![workspace],
                Err(error) if error.code == ScheduledErrorCode::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error),
            },
            None => (self.list_workspaces)()?,
        };
        let mut definitions = Vec::new();
        for workspace in workspaces {
            let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
                .map_err(ScheduledServiceError::from_database)?;
            definitions.extend(
                database
                    .list_job_definitions_for_project(&workspace.app.paths.project_id)
                    .map_err(ScheduledServiceError::from_database)?,
            );
        }
        definitions.sort_by_key(|definition| definition.created_at);
        Ok(definitions)
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

    pub fn list_occurrences(
        &self,
        project_id: &str,
        job_id: &str,
        limit: usize,
    ) -> ScheduledServiceResult<Vec<ScheduledOccurrence>> {
        let workspace = (self.resolve_workspace)(project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let database = ScheduledTaskDatabase::open(workspace.app.paths.scheduler_db_path())
            .map_err(ScheduledServiceError::from_database)?;
        database
            .get_job_definition(&resolved_project_id, job_id)
            .map_err(ScheduledServiceError::from_database)?
            .ok_or_else(|| ScheduledServiceError::not_found(project_id, job_id))?;
        database
            .list_occurrences(job_id, limit)
            .map_err(ScheduledServiceError::from_database)
    }

    pub fn create(
        &self,
        input: CreateScheduledTaskInputVm,
    ) -> ScheduledServiceResult<ScheduledJobRecord> {
        let workspace = (self.resolve_workspace)(&input.project_id)?;
        let resolved_project_id = workspace.app.paths.project_id.clone();
        let validation_input = ConversationCreateInputVm {
            project_id: resolved_project_id.clone(),
            content: input.content.clone(),
            run_mode: input.run_mode.clone(),
            workflow_template_id: input.workflow_template_id.clone(),
            include_interview: input.include_interview,
            direct_config: input.direct_config.clone(),
            auto_config: input.auto_config.clone(),
            attachment_paths: input.attachment_paths.clone(),
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
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
            input.schedule,
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
        definition.execution_config = serde_json::json!({
            "runMode": input.run_mode,
            "workflowTemplateId": input.workflow_template_id,
            "includeInterview": input.include_interview,
            "directConfig": input.direct_config,
            "autoConfig": input.auto_config,
        });
        if let Some(error) =
            crate::scheduled_runtime::scheduled_agent_unattended_error(&workspace.app, &definition)
        {
            return Err(ScheduledServiceError::new(
                error.code,
                error.params.unwrap_or_else(|| serde_json::json!({})),
            ));
        }

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
        self.coordinator
            .notify(SchedulerCommand::JobCreated(record.definition.clone()))?;
        let _ = workspace.workspace_name;
        Ok(record)
    }

    pub fn update(
        &self,
        input: UpdateScheduledTaskInputVm,
    ) -> ScheduledServiceResult<ScheduledJobRecord> {
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
            include_interview: input.include_interview,
            direct_config: input.direct_config.clone(),
            auto_config: input.auto_config.clone(),
            attachment_paths: Some(attachment_paths),
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
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
        let previous_mode = definition.mode;
        let new_snapshot =
            scheduled_content_snapshot(&workspace.app, &validation_input).map_err(|_| {
                ScheduledServiceError::invalid(
                    "build-content-snapshot",
                    serde_json::json!({ "scheduledTaskId": input.scheduled_task_id }),
                )
            })?;
        let content_changed = new_snapshot != definition.content_snapshot;
        definition.content_snapshot = new_snapshot;
        definition.recompute_content_fingerprint().map_err(|_| {
            ScheduledServiceError::invalid(
                "fingerprint-content",
                serde_json::json!({ "scheduledTaskId": input.scheduled_task_id }),
            )
        })?;
        definition.instruction = input.content;
        definition.schedule = input.schedule;
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
        definition.execution_config = serde_json::json!({
            "runMode": input.run_mode,
            "workflowTemplateId": input.workflow_template_id,
            "includeInterview": input.include_interview,
            "directConfig": input.direct_config,
            "autoConfig": input.auto_config,
        });
        let now = chrono::Utc::now();
        definition.updated_at = if now > expected_updated_at {
            now
        } else {
            expected_updated_at + chrono::Duration::milliseconds(1)
        };
        if let Some(error) =
            crate::scheduled_runtime::scheduled_agent_unattended_error(&workspace.app, &definition)
        {
            return Err(ScheduledServiceError::new(
                error.code,
                error.params.unwrap_or_else(|| serde_json::json!({})),
            ));
        }

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
        let update_result = database
            .update_job(
                &definition,
                expected_updated_at,
                derived_next_run_at(&definition),
            )
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
            .notify(SchedulerCommand::JobUpdated(record.definition.clone()))?;
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
        let expected_updated_at = current.definition.updated_at;
        let mut definition = current.definition;
        let was_enabled = definition.enabled;
        definition.enabled = enabled;
        if enabled && !was_enabled {
            if let ScheduleKind::Every { anchor_at, .. } = &mut definition.schedule.kind {
                *anchor_at = chrono::Utc::now();
            }
        }
        let now = chrono::Utc::now();
        definition.updated_at = if now > expected_updated_at {
            now
        } else {
            expected_updated_at + chrono::Duration::milliseconds(1)
        };
        let record = match database
            .update_job(
                &definition,
                expected_updated_at,
                derived_next_run_at(&definition),
            )
            .map_err(ScheduledServiceError::from_database)?
        {
            UpdateJobResult::Updated(record) => record,
            UpdateJobResult::Conflict(record) => return Err(conflict_error(&record)),
            UpdateJobResult::NotFound => {
                return Err(ScheduledServiceError::not_found(project_id, job_id));
            }
        };
        self.coordinator.notify(if enabled {
            SchedulerCommand::JobEnabled(record.definition.clone())
        } else {
            SchedulerCommand::JobDisabled(record.definition.clone())
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

struct DesktopScheduledCoordinator {
    app_handle: AppHandle,
}

impl ScheduledCoordinator for DesktopScheduledCoordinator {
    fn notify(&self, command: SchedulerCommand) -> ScheduledServiceResult<()> {
        let (definition, runtime_command) = match command {
            SchedulerCommand::JobCreated(definition) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &definition)?;
                (
                    definition,
                    crate::scheduled_runtime::SchedulerCommand::JobCreated { key },
                )
            }
            SchedulerCommand::JobUpdated(definition) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &definition)?;
                (
                    definition,
                    crate::scheduled_runtime::SchedulerCommand::JobUpdated { key },
                )
            }
            SchedulerCommand::JobEnabled(definition) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &definition)?;
                (
                    definition,
                    crate::scheduled_runtime::SchedulerCommand::JobEnabled { key },
                )
            }
            SchedulerCommand::JobDisabled(definition) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &definition)?;
                (
                    definition,
                    crate::scheduled_runtime::SchedulerCommand::JobDisabled { key },
                )
            }
            SchedulerCommand::JobDeleted(definition) => {
                let key = scheduled_job_key_for_definition(&self.app_handle, &definition)?;
                (
                    definition,
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
            crate::scheduled_runtime::emit_scheduled_task_updated(&self.app_handle, &definition);
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
    let workspace = resolve_desktop_workspace(app_handle, &definition.project_id)?;
    Ok(gold_band::scheduler::coordinator::ScheduledJobKey::new(
        workspace.app.paths.repo_root,
        definition.project_id.clone(),
        definition.id.clone(),
    ))
}

fn resolve_desktop_workspace(
    app_handle: &AppHandle,
    project_id: &str,
) -> ScheduledServiceResult<ResolvedWorkspace> {
    let state = app_handle.state::<crate::state::DesktopState>();
    let context = state
        .context()
        .map_err(|_| ScheduledServiceError::internal("read-desktop-context"))?;
    let global_app = context.app();
    let app_state = global_app
        .load_state()
        .map_err(|_| ScheduledServiceError::internal("read-workspace-state"))?;
    let Some((workspace_path, resolved_project_id)) =
        crate::conversation_workspace::workspace_entry_for_project(&app_state, project_id)
    else {
        return Err(ScheduledServiceError::not_found(project_id, ""));
    };
    let app = crate::conversation_workspace::app_for_workspace(&context, &workspace_path)
        .map_err(|_| ScheduledServiceError::internal("resolve-workspace"))?;
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

fn list_desktop_workspaces(
    app_handle: &AppHandle,
) -> ScheduledServiceResult<Vec<ResolvedWorkspace>> {
    let state = app_handle.state::<crate::state::DesktopState>();
    let context = state
        .context()
        .map_err(|_| ScheduledServiceError::internal("read-desktop-context"))?;
    let app_state = context
        .app()
        .load_state()
        .map_err(|_| ScheduledServiceError::internal("read-workspace-state"))?;
    app_state
        .conversation_workspaces
        .iter()
        .map(|workspace| {
            crate::conversation_workspace::app_for_workspace(&context, &workspace.workspace_path)
                .map(|app| ResolvedWorkspace {
                    app,
                    workspace_name: workspace.name.clone(),
                })
                .map_err(|_| ScheduledServiceError::internal("resolve-workspace"))
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
    use std::sync::{Arc, Mutex};

    use chrono::{Duration, Utc};
    use gold_band::app::{App, DEFAULT_WORKFLOW_TEMPLATE_ID};
    use gold_band::scheduler::db::ScheduledTaskDatabase;
    use gold_band::scheduler::occurrence::{
        OccurrenceTriggerKind, ScheduledErrorCode, ScheduledOccurrence,
    };
    use gold_band::scheduler::{
        OverlapPolicy, ScheduleSpec, ScheduledMode, ScheduledTaskDefinition, SessionPolicy,
    };
    use tempfile::TempDir;

    use super::{
        ManualRunResult, ScheduledCoordinator, ScheduledTaskService, SchedulerCommand,
        should_reset_task_association,
    };
    use crate::view_models_conversation::{CreateScheduledTaskInputVm, UpdateScheduledTaskInputVm};

    #[derive(Default)]
    struct CoordinatorSpy {
        database: Mutex<Option<ScheduledTaskDatabase>>,
        start_count: Mutex<usize>,
        commands: Mutex<Vec<SchedulerCommand>>,
    }

    impl CoordinatorSpy {
        fn with_database(database: ScheduledTaskDatabase) -> Self {
            Self {
                database: Mutex::new(Some(database)),
                ..Self::default()
            }
        }

        fn start_count(&self) -> usize {
            *self.start_count.lock().unwrap()
        }
    }

    impl ScheduledCoordinator for CoordinatorSpy {
        fn notify(&self, command: SchedulerCommand) -> super::ScheduledServiceResult<()> {
            self.commands.lock().unwrap().push(command);
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

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
            let app = App::new(root);
            let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
            let coordinator = Arc::new(CoordinatorSpy::with_database(database.clone()));
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
                include_interview: Some(false),
                direct_config: None,
                auto_config: None,
                attachment_paths: None,
                schedule: ScheduleSpec::at(Utc::now() + Duration::hours(1)),
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
                include_interview: Some(false),
                direct_config: None,
                auto_config: None,
                attachment_paths: None,
                schedule: definition.schedule.clone(),
                overlap_policy: definition.overlap_policy,
                session_policy: SessionPolicy::New,
            }
        }
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
                .list_occurrences(result.definition.id(), 10)
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
                .list_occurrences(&job_id, 10)
                .unwrap()
                .into_iter()
                .map(|occurrence: ScheduledOccurrence| occurrence.trigger_kind)
                .collect::<Vec<_>>(),
            vec![OccurrenceTriggerKind::Manual]
        );
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
        let first_temp = tempfile::tempdir().unwrap();
        let second_temp = tempfile::tempdir().unwrap();
        let first_app =
            App::new(camino::Utf8PathBuf::from_path_buf(first_temp.path().to_path_buf()).unwrap());
        let second_app =
            App::new(camino::Utf8PathBuf::from_path_buf(second_temp.path().to_path_buf()).unwrap());
        let coordinator = Arc::new(CoordinatorSpy::default());
        let service = ScheduledTaskService::for_test_workspaces(
            &[(&first_app, "First"), (&second_app, "Second")],
            coordinator,
        );
        let input_for = |app: &App, content: &str| CreateScheduledTaskInputVm {
            project_id: app.paths.project_id.clone(),
            content: content.to_string(),
            run_mode: "workflow".to_string(),
            workflow_template_id: Some(DEFAULT_WORKFLOW_TEMPLATE_ID.to_string()),
            include_interview: Some(false),
            direct_config: None,
            auto_config: None,
            attachment_paths: None,
            schedule: ScheduleSpec::at(Utc::now() + Duration::hours(1)),
            overlap_policy: OverlapPolicy::SkipWhenRunning,
            session_policy: None,
        };
        service.create(input_for(&first_app, "first")).unwrap();
        service.create(input_for(&second_app, "second")).unwrap();

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
        assert_eq!(second[0].instruction, "second");
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
                .list_occurrences(created.definition.id(), 10)
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
