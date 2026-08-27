use anyhow::{Context, Result, anyhow};

use crate::app::App;
use crate::domain::RunStatus;
use crate::scheduler::db::{
    HistoryDeletionStatus, ScheduledHistoryDeletionOperation, ScheduledTaskDatabase,
    UpdateJobResult,
};
use crate::scheduler::occurrence::{ScheduledError, ScheduledErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDeletionAction {
    StopRequired,
    Deleted,
    Completed,
}

#[derive(Debug, Default)]
pub struct HistoryDeletionReconcileResult {
    pub completed: usize,
    pub stop_required: Vec<ScheduledHistoryDeletionOperation>,
}

pub fn request_run_history_deletion(
    app: &App,
    operation: &ScheduledHistoryDeletionOperation,
) -> Result<HistoryDeletionAction> {
    if operation.status == HistoryDeletionStatus::Completed {
        return Ok(HistoryDeletionAction::Completed);
    }
    let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
    if operation.status == HistoryDeletionStatus::Deleting {
        return attempt_finalize_operation(app, &database, operation)
            .map(|()| HistoryDeletionAction::Deleted);
    }
    let run = match app.run_status(&operation.task_id, &operation.run_id) {
        Ok(run) => run,
        Err(error) => {
            persist_failure(&database, operation, &error);
            return Err(error);
        }
    };
    if run.status != RunStatus::Completed {
        database
            .transition_history_deletion(&operation.operation_id, HistoryDeletionStatus::Stopping)?
            .ok_or_else(|| anyhow!("history deletion operation was not found"))?;
        return Ok(HistoryDeletionAction::StopRequired);
    }
    attempt_finalize_operation(app, &database, operation).map(|()| HistoryDeletionAction::Deleted)
}

pub fn finalize_terminal_run_history_deletion(
    app: &App,
    scheduled_task_id: &str,
    task_id: &str,
    run_id: &str,
) -> Result<()> {
    let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
    let Some(operation) = database.get_history_deletion_for_run(
        &app.paths.project_id,
        scheduled_task_id,
        task_id,
        run_id,
    )?
    else {
        return Ok(());
    };
    if operation.status == HistoryDeletionStatus::Completed {
        return Ok(());
    }
    match app.run_status(task_id, run_id) {
        Ok(run) if run.status == RunStatus::Completed => {}
        Ok(_) => return Ok(()),
        Err(_) if operation.status == HistoryDeletionStatus::Deleting => {
            return attempt_finalize_operation(app, &database, &operation);
        }
        Err(error) => {
            persist_failure(&database, &operation, &error);
            return Err(error);
        }
    }
    attempt_finalize_operation(app, &database, &operation)
}

pub fn finalize_no_writer_run_history_deletion(
    app: &App,
    scheduled_task_id: &str,
    task_id: &str,
    run_id: &str,
) -> Result<()> {
    let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
    let Some(operation) = database.get_history_deletion_for_run(
        &app.paths.project_id,
        scheduled_task_id,
        task_id,
        run_id,
    )?
    else {
        return Ok(());
    };
    if operation.status == HistoryDeletionStatus::Stopping {
        attempt_finalize_operation(app, &database, &operation)?;
    }
    Ok(())
}

pub fn finalize_terminal_run_history_deletions(
    app: &App,
    task_id: &str,
    run_id: &str,
) -> Result<usize> {
    let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
    let operations =
        database.list_pending_history_deletions_for_run(&app.paths.project_id, task_id, run_id)?;
    let mut completed = 0usize;
    for operation in operations {
        finalize_terminal_run_history_deletion(app, &operation.scheduled_task_id, task_id, run_id)?;
        completed = completed.saturating_add(1);
    }
    Ok(completed)
}

pub fn reconcile_pending_run_history_deletions(
    app: &App,
) -> Result<HistoryDeletionReconcileResult> {
    let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path())?;
    let operations = database.list_pending_history_deletions(
        &app.paths.project_id,
        crate::scheduler::db::EXECUTION_HISTORY_BATCH_MAX,
    )?;
    let mut result = HistoryDeletionReconcileResult::default();
    for operation in operations {
        match request_run_history_deletion(app, &operation) {
            Ok(HistoryDeletionAction::Deleted | HistoryDeletionAction::Completed) => {
                result.completed = result.completed.saturating_add(1);
            }
            Ok(HistoryDeletionAction::StopRequired) => {
                result.stop_required.push(
                    database
                        .get_history_deletion(&operation.operation_id)?
                        .ok_or_else(|| anyhow!("history deletion operation was not found"))?,
                );
            }
            Err(_) => {}
        }
    }
    Ok(result)
}

fn finalize_operation(
    app: &App,
    database: &ScheduledTaskDatabase,
    operation: &ScheduledHistoryDeletionOperation,
) -> Result<()> {
    database
        .transition_history_deletion(&operation.operation_id, HistoryDeletionStatus::Deleting)?
        .ok_or_else(|| anyhow!("history deletion operation was not found"))?;

    let run_dir = app.paths.run_dir(&operation.task_id, &operation.run_id);
    let run_path_identity = crate::storage::normalize_workspace_path(&run_dir);
    if run_dir.exists() {
        trash::delete(run_dir.as_std_path()).with_context(|| {
            format!(
                "failed to move run history to trash: {}/{}",
                operation.task_id, operation.run_id
            )
        })?;
    }
    if let Some(index) = crate::storage::sqlite::search_index() {
        index.delete_run_by_normalized_path(&run_path_identity)?;
    }
    database.delete_execution_history_run(
        &operation.project_id,
        &operation.scheduled_task_id,
        &operation.task_id,
        &operation.run_id,
    )?;

    let task_dir = app.paths.task_dir(&operation.task_id);
    crate::storage::with_file_lock(&task_dir, || {
        if task_has_no_runs(app, &operation.task_id)? {
            if task_dir.exists() {
                trash::delete(task_dir.as_std_path()).with_context(|| {
                    format!(
                        "failed to move empty task shell to trash: {}",
                        operation.task_id
                    )
                })?;
                crate::storage::sqlite::delete_task(&task_dir);
            }
            clear_definition_task_binding(database, operation)?;
        }
        Ok(())
    })?;

    database
        .transition_history_deletion(&operation.operation_id, HistoryDeletionStatus::Completed)?
        .ok_or_else(|| anyhow!("history deletion operation was not found"))?;
    Ok(())
}

fn attempt_finalize_operation(
    app: &App,
    database: &ScheduledTaskDatabase,
    operation: &ScheduledHistoryDeletionOperation,
) -> Result<()> {
    match finalize_operation(app, database, operation) {
        Ok(()) => Ok(()),
        Err(error) => {
            persist_failure(database, operation, &error);
            Err(error)
        }
    }
}

fn task_has_no_runs(app: &App, task_id: &str) -> Result<bool> {
    let runs_dir = app.paths.runs_dir(task_id);
    if !runs_dir.exists() {
        return Ok(true);
    }
    for entry in std::fs::read_dir(runs_dir.as_std_path())? {
        if entry?.file_type()?.is_dir() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn clear_definition_task_binding(
    database: &ScheduledTaskDatabase,
    operation: &ScheduledHistoryDeletionOperation,
) -> Result<()> {
    let Some(record) =
        database.get_job_definition(&operation.project_id, &operation.scheduled_task_id)?
    else {
        return Ok(());
    };
    if record.definition.task_id.as_deref() != Some(operation.task_id.as_str()) {
        return Ok(());
    }
    let mut definition = record.definition;
    definition.task_id = None;
    match database.update_job_runtime_projection(&definition, record.revision)? {
        UpdateJobResult::Updated(_) => Ok(()),
        UpdateJobResult::Conflict(_) => Err(anyhow!(
            "scheduled definition changed while clearing deleted task binding"
        )),
        UpdateJobResult::NotFound => Ok(()),
    }
}

fn persist_failure(
    database: &ScheduledTaskDatabase,
    operation: &ScheduledHistoryDeletionOperation,
    error: &anyhow::Error,
) {
    tracing::warn!(
        %error,
        operation_id = %operation.operation_id,
        project_id = %operation.project_id,
        task_id = %operation.task_id,
        run_id = %operation.run_id,
        "scheduled history deletion failed"
    );
    let structured = ScheduledError::with_params(
        ScheduledErrorCode::StorageFailed,
        serde_json::json!({
            "operation": "delete-execution-history",
            "projectId": operation.project_id,
            "scheduledTaskId": operation.scheduled_task_id,
            "taskId": operation.task_id,
            "runId": operation.run_id,
        }),
    );
    let _ = database.record_history_deletion_failure(&operation.operation_id, &structured);
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryDeletionAction, HistoryDeletionStatus, finalize_no_writer_run_history_deletion,
        finalize_terminal_run_history_deletion, reconcile_pending_run_history_deletions,
        request_run_history_deletion,
    };
    use crate::app::App;
    use crate::domain::{NodeType, RoundTrigger, RunOutcome, RunStatus, VERSION};
    use crate::runtime::{NodeState, RoundState, RunState};
    use crate::scheduler::db::{AcceptExecutionResult, ScheduledTaskDatabase};
    use crate::scheduler::execution::ScheduledExecutionSnapshot;
    use crate::scheduler::occurrence::{ClaimResult, OccurrenceLinks, OccurrenceTriggerKind};
    use crate::scheduler::{OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition};
    use crate::storage::write_json;
    use camino::Utf8PathBuf;
    use chrono::{Duration, Utc};

    fn fixture() -> (tempfile::TempDir, App, ScheduledTaskDatabase) {
        let temp = tempfile::tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = App::new(repo_root);
        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        (temp, app, database)
    }

    fn seed_accepted_history(
        app: &App,
        database: &ScheduledTaskDatabase,
        scheduled_task_id: &str,
        task_id: &str,
        run_id: &str,
    ) {
        let now = Utc::now();
        let mut definition = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            scheduled_task_id,
            "direct",
            ScheduleSpec::at(now),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.task_id = Some(task_id.to_string());
        definition.content_snapshot.instruction = "history".to_string();
        definition.recompute_content_fingerprint().unwrap();
        let record = database.create_job(&definition, None).unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &app.paths.project_id,
                scheduled_task_id,
                now,
                OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();
        assert!(matches!(
            database
                .claim_occurrence(
                    &app.paths.project_id,
                    &occurrence.id,
                    "history-delete-test",
                    now - Duration::seconds(1),
                    now + Duration::minutes(5),
                )
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let links = OccurrenceLinks {
            task_id: Some(task_id.to_string()),
            run_id: Some(run_id.to_string()),
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
            database
                .accept_occurrence_execution(
                    &app.paths.project_id,
                    &occurrence.id,
                    "history-delete-test",
                    record.revision,
                    &links,
                    &snapshot,
                )
                .unwrap(),
            AcceptExecutionResult::Accepted(_)
        ));
    }

    fn write_running_run(app: &App, task_id: &str, run_id: &str) {
        let run = RunState {
            version: VERSION.to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-25T00:00:00Z".to_string(),
            updated_at: "2026-08-25T00:00:01Z".to_string(),
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
        let round = RoundState {
            version: VERSION.to_string(),
            id: "round-1".to_string(),
            run_id: run_id.to_string(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: RoundTrigger::Initial,
            started_at: "2026-08-25T00:00:00Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        };
        let node = NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "node-1".to_string(),
            node_type: NodeType::Worker,
            run_id: run_id.to_string(),
            round_id: "round-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-08-25T00:00:00Z".to_string(),
            finished_at: None,
            manual_check_pending: false,
            runtime_execution_id: Some("execution-1".to_string()),
            resolved_config: Default::default(),
            uuid: None,
        };
        write_json(&app.paths.run_file(task_id, run_id), &run).unwrap();
        write_json(&app.paths.round_file(task_id, run_id, "round-1"), &round).unwrap();
        write_json(
            &app.paths
                .node_file(task_id, run_id, "round-1", "node-1", "attempt-1"),
            &node,
        )
        .unwrap();
    }

    fn write_completed_run(app: &App, task_id: &str, run_id: &str) {
        let run = RunState {
            version: VERSION.to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Completed,
            outcome: Some(RunOutcome::Success),
            started_at: "2026-08-25T00:00:00Z".to_string(),
            updated_at: "2026-08-25T00:00:01Z".to_string(),
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
        write_json(&app.paths.run_file(task_id, run_id), &run).unwrap();
    }

    #[test]
    fn terminal_history_delete_moves_only_the_target_run_to_trash() {
        let (_temp, app, database) = fixture();
        let target = app.paths.run_dir("task-1", "run-a");
        let preserved = app.paths.run_dir("task-1", "run-b");
        std::fs::create_dir_all(target.as_std_path()).unwrap();
        std::fs::create_dir_all(preserved.as_std_path()).unwrap();
        std::fs::write(target.join("marker").as_std_path(), "target").unwrap();
        std::fs::write(preserved.join("marker").as_std_path(), "preserved").unwrap();
        write_completed_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .expect("fixture must contain accepted run history");

        finalize_terminal_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a")
            .unwrap();

        assert!(!target.exists());
        assert!(preserved.exists());
        assert_eq!(
            database
                .get_history_deletion(&operation.operation_id)
                .unwrap()
                .unwrap()
                .status,
            HistoryDeletionStatus::Completed
        );
    }

    #[test]
    fn deleting_the_last_run_removes_the_empty_task_shell_and_is_idempotent() {
        let (_temp, app, database) = fixture();
        let task_dir = app.paths.task_dir("task-1");
        write_completed_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .expect("fixture must contain accepted run history");

        finalize_terminal_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a")
            .unwrap();
        finalize_terminal_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a")
            .unwrap();

        assert!(!task_dir.exists());
        assert!(
            database
                .get_job_definition(&app.paths.project_id, "scheduled-task-1")
                .unwrap()
                .unwrap()
                .definition
                .task_id
                .is_none()
        );
    }

    #[test]
    fn active_history_delete_persists_stopping_and_waits_for_terminal_acknowledgement() {
        let (_temp, app, database) = fixture();
        write_running_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();
        let action = request_run_history_deletion(&app, &operation).unwrap();

        assert_eq!(action, HistoryDeletionAction::StopRequired);
        let stopping = database
            .get_history_deletion(&operation.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(stopping.status, HistoryDeletionStatus::Stopping);
        assert_eq!(stopping.revision, 2);
        assert!(app.paths.run_dir("task-1", "run-a").exists());
    }

    #[test]
    fn paused_run_history_is_not_deleted_until_the_run_is_completed() {
        let (_temp, app, database) = fixture();
        let run_dir = app.paths.run_dir("task-1", "run-a");
        write_running_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();

        assert_eq!(
            request_run_history_deletion(&app, &operation).unwrap(),
            HistoryDeletionAction::StopRequired
        );
        app.run_pause(
            "task-1",
            "run-a",
            crate::domain::PauseReason::ProcessInterrupted,
        )
        .unwrap();

        finalize_terminal_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a")
            .unwrap();

        assert!(run_dir.exists());
        assert_eq!(
            database
                .get_history_deletion(&operation.operation_id)
                .unwrap()
                .unwrap()
                .status,
            HistoryDeletionStatus::Stopping
        );

        write_completed_run(&app, "task-1", "run-a");
        finalize_terminal_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a")
            .unwrap();

        assert!(!run_dir.exists());
    }

    #[test]
    fn no_writer_settlement_deletes_a_stopping_active_run_history() {
        let (_temp, app, database) = fixture();
        let run_dir = app.paths.run_dir("task-1", "run-a");
        write_running_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();

        assert_eq!(
            request_run_history_deletion(&app, &operation).unwrap(),
            HistoryDeletionAction::StopRequired
        );
        app.run_pause(
            "task-1",
            "run-a",
            crate::domain::PauseReason::ProcessInterrupted,
        )
        .unwrap();

        finalize_no_writer_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a")
            .unwrap();

        assert!(!run_dir.exists());
        assert!(
            database
                .list_execution_history(&app.paths.project_id, "scheduled-task-1", 20)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            database
                .get_history_deletion(&operation.operation_id)
                .unwrap()
                .unwrap()
                .status,
            HistoryDeletionStatus::Completed
        );
    }

    #[test]
    fn no_writer_finalization_failure_is_recorded_once() {
        let (_temp, app, database) = fixture();
        write_running_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            request_run_history_deletion(&app, &operation).unwrap(),
            HistoryDeletionAction::StopRequired
        );
        std::fs::remove_dir_all(app.paths.runs_dir("task-1").as_std_path()).unwrap();
        std::fs::write(
            app.paths.runs_dir("task-1").as_std_path(),
            "not-a-directory",
        )
        .unwrap();

        assert!(
            finalize_no_writer_run_history_deletion(&app, "scheduled-task-1", "task-1", "run-a",)
                .is_err()
        );

        let failed = database
            .get_history_deletion(&operation.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, HistoryDeletionStatus::Deleting);
        assert_eq!(failed.attempt, 1);
        assert!(failed.last_error.is_some());
    }

    #[test]
    fn unreadable_run_status_preserves_history_and_records_retryable_failure() {
        let (_temp, app, database) = fixture();
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let run_dir = app.paths.run_dir("task-1", "run-a");
        write_completed_run(&app, "task-1", "run-a");
        std::fs::write(app.paths.run_file("task-1", "run-a").as_std_path(), "{").unwrap();
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();

        assert!(request_run_history_deletion(&app, &operation).is_err());

        assert!(run_dir.exists());
        let failed = database
            .get_history_deletion(&operation.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, HistoryDeletionStatus::Accepted);
        assert_eq!(failed.attempt, 1);
        assert!(failed.last_error.is_some());
    }

    #[test]
    fn missing_run_status_preserves_fresh_history_deletion_for_retry() {
        let (_temp, app, database) = fixture();
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();

        assert!(request_run_history_deletion(&app, &operation).is_err());

        assert_eq!(
            database
                .list_execution_history(&app.paths.project_id, "scheduled-task-1", 20)
                .unwrap()
                .len(),
            1
        );
        let failed = database
            .get_history_deletion(&operation.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, HistoryDeletionStatus::Accepted);
        assert_eq!(failed.attempt, 1);
        assert!(failed.last_error.is_some());
    }

    #[test]
    fn deleting_retry_records_one_failure_and_remains_retryable() {
        let (_temp, app, database) = fixture();
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();
        let deleting = database
            .transition_history_deletion(&operation.operation_id, HistoryDeletionStatus::Deleting)
            .unwrap()
            .unwrap();
        std::fs::create_dir_all(app.paths.task_dir("task-1").as_std_path()).unwrap();
        std::fs::write(
            app.paths.runs_dir("task-1").as_std_path(),
            "not-a-directory",
        )
        .unwrap();

        assert!(request_run_history_deletion(&app, &deleting).is_err());

        let failed = database
            .get_history_deletion(&operation.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, HistoryDeletionStatus::Deleting);
        assert_eq!(failed.attempt, 1);
        assert!(failed.last_error.is_some());

        std::fs::remove_file(app.paths.runs_dir("task-1").as_std_path()).unwrap();
        assert_eq!(
            request_run_history_deletion(&app, &failed).unwrap(),
            HistoryDeletionAction::Deleted
        );
        assert_eq!(
            database
                .get_history_deletion(&operation.operation_id)
                .unwrap()
                .unwrap()
                .status,
            HistoryDeletionStatus::Completed
        );
    }

    #[test]
    fn startup_reconciles_a_pending_terminal_history_delete() {
        let (_temp, app, database) = fixture();
        let run_dir = app.paths.run_dir("task-1", "run-a");
        write_completed_run(&app, "task-1", "run-a");
        seed_accepted_history(&app, &database, "scheduled-task-1", "task-1", "run-a");
        let operation = database
            .create_or_get_history_deletion(
                &app.paths.project_id,
                "scheduled-task-1",
                "task-1",
                "run-a",
            )
            .unwrap()
            .unwrap();

        assert_eq!(
            reconcile_pending_run_history_deletions(&app)
                .unwrap()
                .completed,
            1
        );

        assert!(!run_dir.exists());
        assert_eq!(
            database
                .get_history_deletion(&operation.operation_id)
                .unwrap()
                .unwrap()
                .status,
            HistoryDeletionStatus::Completed
        );
    }
}
