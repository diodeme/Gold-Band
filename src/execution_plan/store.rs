use std::collections::BTreeSet;
use std::fs;

use chrono::Utc;

use crate::config::ConversationAutoConfig;
use crate::domain::{NodeType, RunOutcome, RunStatus};
use crate::dsl::{NodeDsl, WorkflowDsl, normalize_legacy_workflow_snapshot};
use crate::runtime::{NodeState, RoundState, RunState};
use crate::storage::{GoldBandPaths, read_json, write_json};
use crate::workflow_model_binding::WorkflowModelBindings;

use super::auto_workflow::compile_auto_workflow;
use super::error::{self, ExecutionPlanError, PlanResult};
use super::guard::CurrentPlanGuard;
use super::lock;
use super::model::{
    AuthoringRevisionFile, ExecutionPlanManifest, ExecutionPlanPayload, ExecutionPlanRunMode,
    ExecutionPlanSnapshot, OperationCommit, SCHEMA_VERSION,
};

pub fn lock_key(paths: &GoldBandPaths, task_id: &str, run_id: &str) -> String {
    lock::lock_key(&paths.project_id, task_id, run_id)
}

pub fn read_authoring_revision(paths: &GoldBandPaths, task_id: &str) -> u64 {
    let path = paths.task_authoring_revision_file(task_id);
    if !path.exists() {
        return 0;
    }
    read_json::<AuthoringRevisionFile>(&path)
        .map(|file| file.revision)
        .unwrap_or(0)
}

pub fn write_authoring_revision(
    paths: &GoldBandPaths,
    task_id: &str,
    revision: u64,
) -> PlanResult<()> {
    write_json(
        &paths.task_authoring_revision_file(task_id),
        &AuthoringRevisionFile {
            schema_version: SCHEMA_VERSION,
            revision,
        },
    )
    .map_err(|error| io_error("authoring-revision", &error.to_string()))
}

pub fn bump_authoring_revision(paths: &GoldBandPaths, task_id: &str) -> PlanResult<u64> {
    let next = read_authoring_revision(paths, task_id).saturating_add(1);
    write_authoring_revision(paths, task_id, next)?;
    Ok(next)
}

pub fn load_current(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
) -> PlanResult<ExecutionPlanSnapshot> {
    if paths.execution_plan_manifest_file(task_id, run_id).exists() {
        return read_manifest_snapshot(paths, task_id, run_id);
    }
    let _write = lock::acquire_plan_write_wait(&lock_key(paths, task_id, run_id))?;
    if paths.execution_plan_manifest_file(task_id, run_id).exists() {
        return read_manifest_snapshot(paths, task_id, run_id);
    }
    migrate_legacy_snapshot(paths, task_id, run_id)
}

pub fn load_workflow_projection(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
) -> PlanResult<WorkflowDsl> {
    Ok(workflow_from_snapshot(&load_current(
        paths, task_id, run_id,
    )?))
}

pub fn workflow_from_snapshot(snapshot: &ExecutionPlanSnapshot) -> WorkflowDsl {
    match &snapshot.payload {
        ExecutionPlanPayload::Workflow { workflow, .. } => workflow.clone(),
        ExecutionPlanPayload::Auto { config } => compile_auto_workflow(Some(config)),
    }
}

pub fn ai_dynamic_node_for_dispatch(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    node_id: &str,
) -> PlanResult<Option<crate::dsl::AiDynamicNode>> {
    // Migration takes the write lock. Do it before the dispatch read lock so the
    // same caller cannot wait on itself.
    load_current(paths, task_id, run_id)?;
    let key = lock_key(paths, task_id, run_id);
    let _read = lock::acquire_dispatch_read(&key)?;
    ai_dynamic_node_for_dispatch_under_lock(paths, task_id, run_id, node_id)
}

/// Reads the current dynamic node while the caller already owns the dispatch
/// read lock. Keeping this separate prevents a dynamic proposal from reading
/// the plan, releasing the lock, and then materializing against a different
/// plan revision.
pub(crate) fn ai_dynamic_node_for_dispatch_under_lock(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    node_id: &str,
) -> PlanResult<Option<crate::dsl::AiDynamicNode>> {
    let snapshot = read_manifest_snapshot(paths, task_id, run_id)?;
    let workflow = workflow_from_snapshot(&snapshot);
    Ok(workflow.nodes.into_iter().find_map(|node| match node {
        NodeDsl::AiDynamic(dynamic) if dynamic.id == node_id => Some(dynamic),
        _ => None,
    }))
}

pub fn publish_initial_workflow(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    workflow: WorkflowDsl,
    model_bindings: WorkflowModelBindings,
) -> PlanResult<ExecutionPlanSnapshot> {
    publish_initial(
        paths,
        task_id,
        run_id,
        ExecutionPlanRunMode::Workflow,
        ExecutionPlanPayload::Workflow {
            workflow,
            model_bindings,
        },
    )
}

pub fn publish_initial_auto(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    config: ConversationAutoConfig,
) -> PlanResult<ExecutionPlanSnapshot> {
    publish_initial(
        paths,
        task_id,
        run_id,
        ExecutionPlanRunMode::Auto,
        ExecutionPlanPayload::Auto { config },
    )
}

#[cfg(test)]
pub fn publish_revision(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    expected_plan_revision: u64,
    run_mode: ExecutionPlanRunMode,
    source_authoring_revision: u64,
    payload: ExecutionPlanPayload,
) -> PlanResult<ExecutionPlanSnapshot> {
    publish_revision_checked(
        paths,
        task_id,
        run_id,
        expected_plan_revision,
        run_mode,
        source_authoring_revision,
        payload,
        |_| Ok(()),
    )
}

pub fn publish_revision_checked(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    expected_plan_revision: u64,
    run_mode: ExecutionPlanRunMode,
    source_authoring_revision: u64,
    payload: ExecutionPlanPayload,
    check: impl FnOnce(&ExecutionPlanSnapshot) -> PlanResult<()>,
) -> PlanResult<ExecutionPlanSnapshot> {
    let _write = lock::try_acquire_plan_write(&lock_key(paths, task_id, run_id))?;
    let current = read_manifest_snapshot(paths, task_id, run_id)?;
    if current.plan_revision != expected_plan_revision {
        return Err(ExecutionPlanError::new(
            error::REVISION_CONFLICT,
            serde_json::json!({
                "expectedPlanRevision": expected_plan_revision,
                "planRevision": current.plan_revision,
            }),
        ));
    }
    check(&current)?;
    write_next_revision(
        paths,
        task_id,
        run_id,
        &current,
        run_mode,
        source_authoring_revision,
        payload,
    )
}

pub fn read_run_state(paths: &GoldBandPaths, task_id: &str, run_id: &str) -> PlanResult<RunState> {
    let path = paths.run_file(task_id, run_id);
    if !path.exists() {
        return Err(ExecutionPlanError::new(
            error::NOT_FOUND,
            serde_json::json!({ "taskId": task_id, "runId": run_id }),
        ));
    }
    read_json(&path).map_err(|error| io_error("run", &error.to_string()))
}

pub fn current_guard(
    paths: &GoldBandPaths,
    task_id: &str,
    run: &RunState,
) -> PlanResult<CurrentPlanGuard> {
    Ok(CurrentPlanGuard {
        run_status: run.status,
        run_outcome: run.outcome,
        current_node_id: run.current_node.clone(),
        durable_node_type: durable_current_node_type(paths, task_id, run)?,
        occurred_node_ids: occurred_node_ids(paths, task_id, &run.id)?,
    })
}

pub fn occurred_node_ids(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
) -> PlanResult<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let rounds = paths.run_dir(task_id, run_id).join("rounds");
    if !rounds.exists() {
        return Ok(ids);
    }
    let entries = fs::read_dir(rounds.as_std_path())
        .map_err(|error| io_error("rounds", &error.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("rounds", &error.to_string()))?;
        let path = entry.path().join("round.json");
        if !path.is_file() {
            continue;
        }
        let round: RoundState =
            read_json(&camino::Utf8PathBuf::from_path_buf(path).map_err(|_| {
                ExecutionPlanError::new(
                    error::VALIDATION_FAILED,
                    serde_json::json!({ "cause": "round-path" }),
                )
            })?)
            .map_err(|error| {
                ExecutionPlanError::new(
                    error::VALIDATION_FAILED,
                    serde_json::json!({ "cause": "round-unreadable", "detail": error.to_string() }),
                )
            })?;
        for step in round.trace {
            ids.insert(step.node_id);
        }
    }
    Ok(ids)
}

pub fn write_operation_commit(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    commit: &OperationCommit,
) -> PlanResult<()> {
    let dir = paths.execution_plan_operation_dir(task_id, run_id, &commit.operation_id);
    write_json(&dir.join("commit.json"), commit)
        .map_err(|error| io_error("operation-commit", &error.to_string()))
}

pub fn read_operation_commit(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    operation_id: &str,
) -> PlanResult<Option<OperationCommit>> {
    let path = paths
        .execution_plan_operation_dir(task_id, run_id, operation_id)
        .join("commit.json");
    if !path.exists() {
        return Ok(None);
    }
    read_json(&path)
        .map(Some)
        .map_err(|error| io_error("operation-commit", &error.to_string()))
}

pub fn validate_operation_id(operation_id: &str) -> PlanResult<()> {
    let valid = !operation_id.is_empty()
        && operation_id.len() <= 80
        && operation_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        Ok(())
    } else {
        Err(ExecutionPlanError::new(
            error::VALIDATION_FAILED,
            serde_json::json!({ "field": "operationId" }),
        ))
    }
}

pub fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Paused => "paused",
        RunStatus::Completed => "completed",
    }
}

pub fn outcome_label(outcome: RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Success => "success",
        RunOutcome::Failure => "failure",
        RunOutcome::Killed => "killed",
    }
}

fn publish_initial(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    run_mode: ExecutionPlanRunMode,
    payload: ExecutionPlanPayload,
) -> PlanResult<ExecutionPlanSnapshot> {
    let _write = lock::acquire_plan_write_wait(&lock_key(paths, task_id, run_id))?;
    if paths.execution_plan_manifest_file(task_id, run_id).exists() {
        return read_manifest_snapshot(paths, task_id, run_id);
    }
    let snapshot = ExecutionPlanSnapshot {
        schema_version: SCHEMA_VERSION,
        plan_revision: 1,
        run_mode,
        source_authoring_revision: read_authoring_revision(paths, task_id),
        published_at: Utc::now().to_rfc3339(),
        payload,
    };
    write_revision_and_manifest(paths, task_id, run_id, &snapshot)?;
    Ok(snapshot)
}

fn migrate_legacy_snapshot(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
) -> PlanResult<ExecutionPlanSnapshot> {
    let snapshot_path = paths.workflow_snapshot_file(task_id, run_id);
    if !snapshot_path.exists() {
        return Err(ExecutionPlanError::new(
            error::NOT_FOUND,
            serde_json::json!({ "taskId": task_id, "runId": run_id }),
        ));
    }
    let legacy: WorkflowDsl = read_json(&snapshot_path)
        .map_err(|error| io_error("legacy-snapshot", &error.to_string()))?;
    let workflow = normalize_legacy_workflow_snapshot(legacy);
    let payload = ExecutionPlanPayload::Workflow {
        workflow,
        model_bindings: WorkflowModelBindings::default(),
    };
    let revision_path = paths.execution_plan_revision_file(task_id, run_id, 1);
    if revision_path.exists() {
        let existing: ExecutionPlanSnapshot = read_json(&revision_path)
            .map_err(|error| io_error("plan-revision", &error.to_string()))?;
        if !payload_eq(&existing.payload, &payload) || existing.plan_revision != 1 {
            return Err(ExecutionPlanError::new(
                error::RECOVERY_REQUIRED,
                serde_json::json!({ "planRevision": 1, "reason": "migration-diverged" }),
            ));
        }
        ensure_manifest(paths, task_id, run_id, &existing)?;
        return Ok(existing);
    }
    let snapshot = ExecutionPlanSnapshot {
        schema_version: SCHEMA_VERSION,
        plan_revision: 1,
        run_mode: legacy_run_mode(paths, task_id),
        source_authoring_revision: read_authoring_revision(paths, task_id),
        published_at: Utc::now().to_rfc3339(),
        payload,
    };
    write_revision_and_manifest(paths, task_id, run_id, &snapshot)?;
    Ok(snapshot)
}

fn legacy_run_mode(paths: &GoldBandPaths, task_id: &str) -> ExecutionPlanRunMode {
    let path = paths.task_dir(task_id).join("authoring/conversation.json");
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ConversationModeFile {
        #[serde(default)]
        run_mode: String,
    }
    if !path.exists() {
        return ExecutionPlanRunMode::Workflow;
    }
    match read_json::<ConversationModeFile>(&path) {
        Ok(file) if file.run_mode == "auto" => ExecutionPlanRunMode::Auto,
        _ => ExecutionPlanRunMode::Workflow,
    }
}

fn write_next_revision(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    current: &ExecutionPlanSnapshot,
    run_mode: ExecutionPlanRunMode,
    source_authoring_revision: u64,
    payload: ExecutionPlanPayload,
) -> PlanResult<ExecutionPlanSnapshot> {
    let next_revision = current.plan_revision.saturating_add(1);
    let revision_path = paths.execution_plan_revision_file(task_id, run_id, next_revision);
    if revision_path.exists() {
        let existing: ExecutionPlanSnapshot = read_json(&revision_path)
            .map_err(|error| io_error("plan-revision", &error.to_string()))?;
        if existing.plan_revision == next_revision && payload_eq(&existing.payload, &payload) {
            ensure_manifest(paths, task_id, run_id, &existing)?;
            return Ok(existing);
        }
        return Err(ExecutionPlanError::new(
            error::RECOVERY_REQUIRED,
            serde_json::json!({
                "planRevision": next_revision,
                "reason": "orphan-revision",
            }),
        ));
    }
    let snapshot = ExecutionPlanSnapshot {
        schema_version: SCHEMA_VERSION,
        plan_revision: next_revision,
        run_mode,
        source_authoring_revision,
        published_at: Utc::now().to_rfc3339(),
        payload,
    };
    write_revision_and_manifest(paths, task_id, run_id, &snapshot)?;
    Ok(snapshot)
}

fn write_revision_and_manifest(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    snapshot: &ExecutionPlanSnapshot,
) -> PlanResult<()> {
    let revision_path = paths.execution_plan_revision_file(task_id, run_id, snapshot.plan_revision);
    write_json(&revision_path, snapshot)
        .map_err(|error| io_error("plan-revision", &error.to_string()))?;
    ensure_manifest(paths, task_id, run_id, snapshot)
}

fn ensure_manifest(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    snapshot: &ExecutionPlanSnapshot,
) -> PlanResult<()> {
    let relative = format!("revisions/plan-{:06}.json", snapshot.plan_revision);
    let manifest = ExecutionPlanManifest {
        schema_version: SCHEMA_VERSION,
        plan_revision: snapshot.plan_revision,
        run_mode: snapshot.run_mode,
        source_authoring_revision: snapshot.source_authoring_revision,
        current_plan_file: relative,
        published_at: snapshot.published_at.clone(),
    };
    write_json(
        &paths.execution_plan_manifest_file(task_id, run_id),
        &manifest,
    )
    .map_err(|error| io_error("plan-manifest", &error.to_string()))
}

fn read_manifest_snapshot(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
) -> PlanResult<ExecutionPlanSnapshot> {
    let manifest_path = paths.execution_plan_manifest_file(task_id, run_id);
    if !manifest_path.exists() {
        return Err(ExecutionPlanError::new(
            error::NOT_FOUND,
            serde_json::json!({ "taskId": task_id, "runId": run_id }),
        ));
    }
    let manifest: ExecutionPlanManifest =
        read_json(&manifest_path).map_err(|error| io_error("plan-manifest", &error.to_string()))?;
    let revision_path = paths.execution_plan_revision_file(task_id, run_id, manifest.plan_revision);
    if !revision_path.exists() {
        return Err(ExecutionPlanError::new(
            error::RECOVERY_REQUIRED,
            serde_json::json!({
                "planRevision": manifest.plan_revision,
                "reason": "revision-missing",
            }),
        ));
    }
    let snapshot: ExecutionPlanSnapshot =
        read_json(&revision_path).map_err(|error| io_error("plan-revision", &error.to_string()))?;
    if snapshot.plan_revision != manifest.plan_revision {
        return Err(ExecutionPlanError::new(
            error::RECOVERY_REQUIRED,
            serde_json::json!({
                "planRevision": manifest.plan_revision,
                "reason": "revision-mismatch",
            }),
        ));
    }
    Ok(snapshot)
}

fn durable_current_node_type(
    paths: &GoldBandPaths,
    task_id: &str,
    run: &RunState,
) -> PlanResult<Option<NodeType>> {
    let (Some(round_id), Some(node_id), Some(attempt_id)) = (
        run.current_round.as_deref(),
        run.current_node.as_deref(),
        run.current_attempt.as_deref(),
    ) else {
        return Ok(None);
    };
    let path = paths.node_file(task_id, &run.id, round_id, node_id, attempt_id);
    if !path.exists() {
        return Ok(None);
    }
    let node: NodeState =
        read_json(&path).map_err(|error| io_error("current-node", &error.to_string()))?;
    if node.node_id != node_id {
        return Err(ExecutionPlanError::new(
            error::CURRENT_LOCATOR_CONFLICT,
            serde_json::json!({ "nodeId": node_id, "durableNodeId": node.node_id }),
        ));
    }
    Ok(Some(node.node_type))
}

pub fn payload_eq(left: &ExecutionPlanPayload, right: &ExecutionPlanPayload) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

fn io_error(scope: &str, detail: &str) -> ExecutionPlanError {
    ExecutionPlanError::new(
        error::RECOVERY_REQUIRED,
        serde_json::json!({ "scope": scope, "detail": detail }),
    )
}
