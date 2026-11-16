//! Failed attempt -> successor Run. The source seal is the handoff commit;
//! target run.json is published only after its bounded input snapshot is ready.
use std::{
    collections::HashSet,
    fs,
    io::Read,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, ensure};
use camino::Utf8PathBuf;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use serde::{Deserialize, Serialize};

use super::ids::{generate_uuid, now_rfc3339_like, reserve_next_run_dir};
use super::orchestrator::{
    PreparedRunData, drive_committed_run, retry_current_locator, retry_error, retry_state_error,
};
use super::profile_resolver::{resolve_profile_for_node, resolve_workflow_profiles};
use super::state_factory::create_node_state;
use super::{App, ConversationMetadata, RetryCurrentNodeAccepted, RuntimeCandidateRegistration};
use crate::domain::{NodeType, PauseReason, RunStatus, VERSION};
use crate::dsl::{WorkflowDsl, validate_workflow_snapshot, workflow_contains_ai_dynamic};
use crate::execution_plan;
use crate::runtime::{
    NodeState, RoundState, RoundTraceStep, RunRerunMetadata, RunRerunOrigin, RunRerunSeal,
    RunState, RuntimeExecutionPhase, RuntimeExecutionState, can_rerun_failed_attempt,
    canonical_trace_for_round, validate_node_state, validate_round_state, validate_run_state,
    write_node_state,
};
use crate::storage::{read_json, write_json};
use crate::workflow_model_binding::{WorkflowModelBindings, validate_and_inject};

const MAX_PREFIX_STEPS: usize = 4096;
const MAX_SOURCE_ROUNDS: usize = 4096;
const MAX_SOURCE_ROUND_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PREFIX_FILES: usize = 4096;
const MAX_PREFIX_BYTES: u64 = 256 * 1024 * 1024;
const QUIESCE_TIMEOUT: Duration = Duration::from_secs(10);
const QUIESCE_POLL: Duration = Duration::from_millis(25);
const MAX_QUIESCE_RUNS: usize = 256;
const MAX_QUIESCE_METADATA_BYTES: u64 = 8 * 1024 * 1024;
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRerun {
    source_run_id: String,
    operation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RerunOperation {
    initial_run: RunState,
    workflow: WorkflowDsl,
    model_bindings: WorkflowModelBindings,
}

/// Uses the existing dispatch lock: mutation admission and source sealing
/// share the same boundary. Never keep this guard over a provider call.
pub struct RunWriteAdmissionGuard {
    _guard: execution_plan::DispatchReadGuard,
}

fn failure(code: &'static str, task_id: &str, run_id: &str) -> anyhow::Error {
    retry_error(code, task_id, run_id, serde_json::json!({}))
}

fn public_rerun_error(task_id: &str, run_id: &str, error: anyhow::Error) -> anyhow::Error {
    let structured = error
        .downcast_ref::<crate::runtime_error::RuntimeError>()
        .is_some()
        || error
            .downcast_ref::<execution_plan::ExecutionPlanError>()
            .is_some()
        || error
            .downcast_ref::<crate::workflow_model_binding::WorkflowModelBindingError>()
            .is_some()
        || error
            .downcast_ref::<crate::git::GitPreflightError>()
            .is_some()
        || error
            .downcast_ref::<crate::git::GitServiceError>()
            .is_some();
    if structured {
        error
    } else {
        retry_state_error(task_id, run_id, &error)
    }
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', ':', '\0'])
}

fn validate_locator(task_id: &str, run_id: &str, parts: &[&str]) -> Result<()> {
    ensure!(
        parts.iter().all(|part| valid_segment(part)),
        failure("workflow.retry.invalid-locator", task_id, run_id)
    );
    Ok(())
}

fn operation_id(
    app: &App,
    run: &RunState,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> String {
    let mut hash = blake3::Hasher::new();
    for part in [
        &app.paths.project_id,
        &run.task_id,
        run.task_uuid.as_ref().unwrap_or(&run.task_id),
        &run.id,
        run.uuid.as_ref().unwrap_or(&run.id),
        round_id,
        node_id,
        attempt_id,
    ] {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("rerun-{}", hash.finalize().to_hex())
}

fn prefix(
    app: &App,
    task_id: &str,
    source: &RunState,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<Vec<RoundState>> {
    // Read only this source Run's round metadata, with a budget enforced
    // before JSON parsing. A far-future orphan or oversized round cannot turn
    // an explicit retry into an unbounded history load.
    let mut rounds: Vec<RoundState> = Vec::new();
    let mut metadata_bytes = 0_u64;
    let directory = app.paths.run_dir(task_id, &source.id).join("rounds");
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        ensure!(
            index < MAX_SOURCE_ROUNDS,
            failure("workflow.retry.prefix-too-large", task_id, &source.id)
        );
        let path = entry?.path().join("round.json");
        if !path.is_file() {
            continue;
        }
        metadata_bytes = metadata_bytes.saturating_add(fs::metadata(&path)?.len());
        ensure!(
            metadata_bytes <= MAX_SOURCE_ROUND_BYTES,
            failure("workflow.retry.prefix-too-large", task_id, &source.id)
        );
        let path = Utf8PathBuf::from_path_buf(path).map_err(|_| anyhow!("non-utf8 rerun round"))?;
        let round: RoundState = read_json(&path)?;
        validate_round_state(&round)?;
        rounds.push(round);
    }
    rounds.sort_by_key(|round| round.index);
    let current = rounds
        .iter()
        .find(|round| Some(round.id.as_str()) == source.current_round.as_deref())
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &source.id))?;
    ensure!(
        !rounds
            .iter()
            .any(|round| round.index == current.index && round.id != current.id),
        failure("workflow.retry.stale-locator", task_id, &source.id)
    );
    let target = rounds
        .iter()
        .find(|round| round.id == round_id)
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &source.id))?;
    let mut result = Vec::new();
    let mut total = 0;
    for round in rounds.iter().filter(|round| round.index <= target.index) {
        let mut inherited = round.clone();
        inherited.trace = canonical_trace_for_round(source, std::slice::from_ref(current), round)?;
        if round.id == round_id {
            let positions = inherited
                .trace
                .iter()
                .enumerate()
                .filter(|(_, step)| step.node_id == node_id && step.attempt_id == attempt_id)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            ensure!(
                positions.len() == 1,
                failure("workflow.retry.stale-locator", task_id, &source.id)
            );
            inherited.trace.truncate(positions[0] + 1);
        }
        validate_locator(task_id, &source.id, &[&inherited.id])?;
        for step in &inherited.trace {
            validate_locator(task_id, &source.id, &[&step.node_id, &step.attempt_id])?;
        }
        total += inherited.trace.len();
        ensure!(
            total <= MAX_PREFIX_STEPS,
            failure("workflow.retry.prefix-too-large", task_id, &source.id)
        );
        result.push(inherited);
    }
    result.sort_by_key(|round| round.index);
    Ok(result)
}

fn source_node(
    app: &App,
    task_id: &str,
    run_id: &str,
    round: &RoundState,
    step: &RoundTraceStep,
) -> Result<NodeState> {
    let path = app
        .paths
        .node_file(task_id, run_id, &round.id, &step.node_id, &step.attempt_id);
    // The path must stay within the selected source Run even with junctions.
    let root = fs::canonicalize(app.paths.run_dir(task_id, run_id))?;
    ensure!(
        fs::canonicalize(&path)?.starts_with(root),
        failure("workflow.retry.invalid-locator", task_id, run_id)
    );
    let node: NodeState = read_json(&path)?;
    validate_node_state(&node)?;
    ensure!(
        node.run_id == run_id
            && node.round_id == round.id
            && node.node_id == step.node_id
            && node.attempt_id == step.attempt_id,
        failure("workflow.retry.stale-locator", task_id, run_id)
    );
    Ok(node)
}

fn is_retryable_failed_worker(node: &NodeState) -> bool {
    node.node_type == NodeType::Worker
        && node.status == RunStatus::Completed
        && matches!(
            node.outcome,
            Some(crate::domain::NodeOutcome::Failure | crate::domain::NodeOutcome::Invalid)
        )
        && !node.manual_check_pending
}

fn admitted_prefix(
    app: &App,
    task_id: &str,
    run: &RunState,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> Result<Vec<RoundState>> {
    ensure!(
        run.pause_reason != Some(PauseReason::ErrorBlocked),
        failure("workflow.retry.error-blocked", task_id, &run.id)
    );
    let metadata: ConversationMetadata = read_json(
        &app.paths
            .task_dir(task_id)
            .join("authoring/conversation.json"),
    )?;
    let rounds = prefix(app, task_id, run, round_id, node_id, attempt_id)?;
    let round = rounds
        .last()
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &run.id))?;
    let step = round
        .trace
        .last()
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &run.id))?;
    let node = source_node(app, task_id, &run.id, round, step)?;
    ensure!(
        !run.is_inherited_step(round, step),
        failure("workflow.retry.inherited-attempt", task_id, &run.id)
    );
    ensure!(
        can_rerun_failed_attempt(run, round, step, &node, metadata.run_mode == "workflow"),
        failure("workflow.retry.node-not-retryable", task_id, &run.id)
    );
    Ok(rounds)
}

#[derive(Default)]
struct AssetBudget {
    bytes: u64,
    files: usize,
}

fn assets(
    app: &App,
    task_id: &str,
    run_id: &str,
    rounds: &[RoundState],
) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>> {
    let root = app.paths.run_dir(task_id, run_id);
    let canonical_root = fs::canonicalize(&root)?;
    let mut budget = AssetBudget::default();
    let mut files = Vec::new();
    for round in rounds {
        for step in &round.trace {
            source_node(app, task_id, run_id, round, step)?;
            for directory in [
                app.paths.artifacts_dir(
                    task_id,
                    run_id,
                    &round.id,
                    &step.node_id,
                    &step.attempt_id,
                ),
                app.paths.attachments_dir(
                    task_id,
                    run_id,
                    &round.id,
                    &step.node_id,
                    &step.attempt_id,
                ),
            ] {
                let mut pending = vec![directory];
                while let Some(path) = pending.pop() {
                    if !path.exists() {
                        continue;
                    }
                    let metadata = fs::symlink_metadata(&path)?;
                    ensure!(
                        !metadata.file_type().is_symlink()
                            && fs::canonicalize(&path)?.starts_with(&canonical_root),
                        failure("workflow.retry.invalid-locator", task_id, run_id)
                    );
                    if metadata.is_dir() {
                        for entry in fs::read_dir(&path)? {
                            let child = Utf8PathBuf::from_path_buf(entry?.path())
                                .map_err(|_| anyhow!("non-utf8 rerun asset"))?;
                            pending.push(child);
                            budget.files += 1;
                            ensure!(
                                budget.files <= MAX_PREFIX_FILES,
                                failure("workflow.retry.prefix-too-large", task_id, run_id)
                            );
                        }
                    } else {
                        ensure!(
                            metadata.is_file(),
                            failure("workflow.retry.invalid-locator", task_id, run_id)
                        );
                        budget.bytes = budget.bytes.saturating_add(metadata.len());
                        ensure!(
                            budget.bytes <= MAX_PREFIX_BYTES,
                            failure("workflow.retry.prefix-too-large", task_id, run_id)
                        );
                        files.push((path.clone(), path.strip_prefix(&root)?.to_path_buf()));
                    }
                }
            }
        }
    }
    Ok(files)
}

fn origin(operation: &RerunOperation) -> Result<&RunRerunOrigin> {
    operation
        .initial_run
        .rerun
        .as_ref()
        .and_then(|rerun| rerun.origin.as_ref())
        .ok_or_else(|| {
            failure(
                "workflow.retry.stale-locator",
                &operation.initial_run.task_id,
                &operation.initial_run.id,
            )
        })
}

struct CommittedDynamicGraph {
    round_id: String,
    node_id: String,
    attempt_id: String,
    path: Utf8PathBuf,
}

fn check_quiesce_directory(
    path: &camino::Utf8Path,
    root: &camino::Utf8Path,
    task_id: &str,
    source_run_id: &str,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
    ensure!(
        !metadata.file_type().is_symlink() && metadata.is_dir(),
        failure("workflow.retry.invalid-locator", task_id, source_run_id)
    );
    let canonical_root = fs::canonicalize(root)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
    let canonical_path = fs::canonicalize(path)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
    ensure!(
        canonical_path.starts_with(&canonical_root),
        failure("workflow.retry.invalid-locator", task_id, source_run_id)
    );
    Ok(())
}

fn quiesce_rounds(
    app: &App,
    task_id: &str,
    run_id: &str,
    total_bytes: &mut u64,
    source_run_id: &str,
) -> Result<Vec<RoundState>> {
    let directory = app.paths.run_dir(task_id, run_id).join("rounds");
    check_quiesce_directory(&directory, &app.paths.runtime_root, task_id, source_run_id)?;
    let mut rounds = Vec::new();
    for (index, entry) in fs::read_dir(&directory)?.enumerate() {
        ensure!(
            index < MAX_SOURCE_ROUNDS,
            failure("workflow.retry.prefix-too-large", task_id, source_run_id)
        );
        let entry = entry?;
        let round_dir = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|_| anyhow!("non-utf8 rerun round directory"))?;
        check_quiesce_directory(&round_dir, &app.paths.runtime_root, task_id, source_run_id)?;
        let path = round_dir.join("round.json");
        check_quiesce_metadata(
            &path,
            &app.paths.runtime_root,
            total_bytes,
            task_id,
            source_run_id,
        )?;
        let round: RoundState = read_json(&path)?;
        validate_round_state(&round)?;
        ensure!(
            round.run_id == run_id,
            failure("workflow.retry.stale-locator", task_id, source_run_id)
        );
        let directory_name = round_dir
            .file_name()
            .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
        ensure!(
            directory_name == round.id,
            failure("workflow.retry.stale-locator", task_id, source_run_id)
        );
        validate_locator(task_id, source_run_id, &[&round.id])?;
        rounds.push(round);
    }
    rounds.sort_by_key(|round| round.index);
    Ok(rounds)
}

fn committed_dynamic_graphs(
    app: &App,
    task_id: &str,
    run_id: &str,
    run: &RunState,
    rounds: &[RoundState],
    total_bytes: &mut u64,
    source_run_id: &str,
) -> Result<Vec<CommittedDynamicGraph>> {
    let mut graphs = Vec::new();
    for round in rounds {
        let trace = canonical_trace_for_round(run, rounds, round)?;
        for step in &trace {
            validate_locator(task_id, source_run_id, &[&step.node_id, &step.attempt_id])?;
            let node_path =
                app.paths
                    .node_file(task_id, run_id, &round.id, &step.node_id, &step.attempt_id);
            check_quiesce_metadata(
                &node_path,
                &app.paths.runtime_root,
                total_bytes,
                task_id,
                source_run_id,
            )?;
            let node: NodeState = read_json(&node_path)?;
            validate_node_state(&node)?;
            ensure!(
                node.run_id == run_id
                    && node.round_id == round.id
                    && node.node_id == step.node_id
                    && node.attempt_id == step.attempt_id,
                failure("workflow.retry.stale-locator", task_id, source_run_id)
            );
            if node.node_type != NodeType::AiDynamic {
                continue;
            }
            let path = app.paths.dynamic_graph_file(
                task_id,
                run_id,
                &round.id,
                &step.node_id,
                &step.attempt_id,
            );
            check_quiesce_metadata(
                &path,
                &app.paths.runtime_root,
                total_bytes,
                task_id,
                source_run_id,
            )?;
            graphs.push(CommittedDynamicGraph {
                round_id: round.id.clone(),
                node_id: step.node_id.clone(),
                attempt_id: step.attempt_id.clone(),
                path,
            });
        }
    }
    Ok(graphs)
}

// A stopped parent cannot publish a new child after its graph lock has
// converged. Follow the durable child IDs instead of the task's Run history.
fn stopped_execution_roots(
    app: &App,
    task_id: &str,
    source_run_id: &str,
) -> Result<HashSet<Utf8PathBuf>> {
    let mut pending = vec![source_run_id.to_string()];
    let mut discovered = HashSet::from([source_run_id.to_string()]);
    let mut roots = HashSet::new();
    let mut metadata_bytes = 0_u64;
    while let Some(run_id) = pending.pop() {
        validate_locator(task_id, source_run_id, &[&run_id])?;
        let run_path = app.paths.run_file(task_id, &run_id);
        check_quiesce_metadata(
            &run_path,
            &app.paths.runtime_root,
            &mut metadata_bytes,
            task_id,
            source_run_id,
        )?;
        let observed = app
            .run_status(task_id, &run_id)
            .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
        ensure!(
            observed.id == run_id && observed.task_id == task_id,
            failure("workflow.retry.stale-locator", task_id, source_run_id)
        );
        let run = app
            .pause_run_state(task_id, &run_id, PauseReason::ProcessInterrupted)
            .map_err(|error| retry_state_error(task_id, source_run_id, &error))?;
        roots.insert(app.paths.run_dir(task_id, &run_id));
        let rounds = quiesce_rounds(app, task_id, &run_id, &mut metadata_bytes, source_run_id)?;
        let graphs = committed_dynamic_graphs(
            app,
            task_id,
            &run_id,
            &run,
            &rounds,
            &mut metadata_bytes,
            source_run_id,
        )?;
        for locator in graphs {
            validate_locator(
                task_id,
                source_run_id,
                &[&locator.round_id, &locator.node_id, &locator.attempt_id],
            )?;
            let state_lock = super::orchestrator::dynamic_state_lock_for(
                &app.paths.runtime_root,
                task_id,
                &run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            )
            .map_err(|error| retry_state_error(task_id, source_run_id, &error))?;
            let _guard = state_lock.lock();
            check_quiesce_metadata(
                &locator.path,
                &app.paths.runtime_root,
                &mut metadata_bytes,
                task_id,
                source_run_id,
            )?;
            let mut graph =
                crate::dynamic_store::load_dynamic_graph(&locator.path, &app.paths.repo_root)
                    .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
            ensure!(
                graph.run.parent_run_id == run_id
                    && graph.run.parent_round_id == locator.round_id
                    && graph.run.parent_node_id == locator.node_id
                    && graph.run.parent_attempt_id == locator.attempt_id,
                failure("workflow.retry.stale-locator", task_id, source_run_id)
            );
            for child_run_id in graph
                .nodes
                .iter()
                .filter_map(|node| node.child_run_id.as_ref())
            {
                validate_locator(task_id, source_run_id, &[child_run_id])?;
                ensure!(
                    discovered.insert(child_run_id.clone()),
                    failure("workflow.retry.stale-locator", task_id, source_run_id)
                );
                ensure!(
                    discovered.len() <= MAX_QUIESCE_RUNS,
                    failure("workflow.retry.prefix-too-large", task_id, source_run_id)
                );
                pending.push(child_run_id.clone());
            }
            if graph.run.status != crate::dynamic::DynamicRunStatus::Completed {
                super::orchestrator::pause_dynamic_graph_runtime_state(
                    &mut graph,
                    PauseReason::ProcessInterrupted,
                );
                super::orchestrator::persist_dynamic_graph_for_resume_unlocked(
                    app,
                    task_id,
                    &run_id,
                    &locator.round_id,
                    &locator.node_id,
                    &locator.attempt_id,
                    &graph,
                )
                .map_err(|error| retry_state_error(task_id, source_run_id, &error))?;
            }
        }
    }
    Ok(roots)
}

fn check_quiesce_metadata(
    path: &camino::Utf8Path,
    root: &camino::Utf8Path,
    total_bytes: &mut u64,
    task_id: &str,
    source_run_id: &str,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        failure("workflow.retry.invalid-locator", task_id, source_run_id)
    );
    ensure!(
        metadata.is_file(),
        failure("workflow.retry.stale-locator", task_id, source_run_id)
    );
    let canonical_root = fs::canonicalize(root)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
    let canonical_path = fs::canonicalize(path)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, source_run_id))?;
    ensure!(
        canonical_path.starts_with(&canonical_root),
        failure("workflow.retry.invalid-locator", task_id, source_run_id)
    );
    *total_bytes = total_bytes.saturating_add(metadata.len());
    ensure!(
        *total_bytes <= MAX_QUIESCE_METADATA_BYTES,
        failure("workflow.retry.prefix-too-large", task_id, source_run_id)
    );
    Ok(())
}

fn quiesce(app: &App, task_id: &str, source_run_id: &str) -> Result<()> {
    let roots = stopped_execution_roots(app, task_id, source_run_id)?;
    let deadline = Instant::now() + QUIESCE_TIMEOUT;
    loop {
        let active = crate::acp::client::active_prompt_attempts_under_roots(&roots)
            .map_err(|_| failure("workflow.retry.source-busy", task_id, source_run_id))?;
        if active.is_empty() {
            return Ok(());
        }
        for attempt_dir in active {
            // Live cancellation is independent of timeline settlement. The
            // provider registry, not the cancel acknowledgement, decides when
            // it is safe to hand the workspace to the successor.
            let _ = crate::acp::client::dispatch_attempt_prompt_cancel(&attempt_dir);
        }
        ensure!(
            Instant::now() < deadline,
            failure("workflow.retry.source-busy", task_id, source_run_id)
        );
        thread::sleep(QUIESCE_POLL);
    }
}

fn materialize(app: &App, task_id: &str, operation: &RerunOperation) -> Result<PreparedRunData> {
    validate_run_state(&operation.initial_run)?;
    let origin = origin(operation)?;
    validate_locator(
        task_id,
        &origin.source_run_id,
        &[
            &operation.initial_run.id,
            &origin.source_run_id,
            &origin.source_round_id,
            &origin.source_node_id,
            &origin.source_attempt_id,
            &origin.operation_id,
        ],
    )?;
    ensure!(
        operation.initial_run.task_id == task_id,
        failure(
            "workflow.retry.stale-locator",
            task_id,
            &origin.source_run_id
        )
    );
    let source = app.run_status(task_id, &origin.source_run_id)?;
    ensure!(
        source.is_sealed() && source.uuid == origin.source_run_uuid,
        failure("workflow.retry.stale-locator", task_id, &source.id)
    );
    ensure_source_sealed_for_operation(&source, operation, task_id)?;
    quiesce(app, task_id, &source.id)?;
    let rounds = prefix(
        app,
        task_id,
        &source,
        &origin.source_round_id,
        &origin.source_node_id,
        &origin.source_attempt_id,
    )?;
    let target_round = rounds
        .last()
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &source.id))?;
    let anchor = target_round
        .trace
        .last()
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &source.id))?;
    let anchor_node = source_node(app, task_id, &source.id, target_round, anchor)?;
    ensure!(
        !source.is_inherited_step(target_round, anchor),
        failure("workflow.retry.inherited-attempt", task_id, &source.id)
    );
    ensure!(
        anchor.sequence == origin.inherited_trace_sequence
            && target_round.index == origin.source_round_index
            && is_retryable_failed_worker(&anchor_node),
        failure("workflow.retry.stale-locator", task_id, &source.id)
    );
    let files = assets(app, task_id, &source.id, &rounds)?;
    let mut run = operation.initial_run.clone();
    let validated = validate_workflow_snapshot(operation.workflow.clone())?;
    let resolved_profiles =
        resolve_workflow_profiles(&app.paths, &validated.raw, app.config.desktop_language)?;
    for inherited in &rounds {
        let mut round = inherited.clone();
        round.run_id = run.id.clone();
        round.uuid = Some(generate_uuid());
        for step in &round.trace {
            let mut node = source_node(app, task_id, &source.id, inherited, step)?;
            node.run_id = run.id.clone();
            node.uuid = Some(generate_uuid());
            node.runtime_execution_id = None;
            write_node_state(
                &app.paths
                    .node_file(task_id, &run.id, &round.id, &step.node_id, &step.attempt_id),
                &node,
            )?;
        }
        if round.id == origin.source_round_id {
            round.status = RunStatus::Running;
            round.outcome = None;
            round.trace.push(RoundTraceStep {
                sequence: origin
                    .inherited_trace_sequence
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("trace sequence exhausted"))?,
                node_id: origin.source_node_id.clone(),
                attempt_id: run.current_attempt.clone().expect("rerun attempt"),
                from_node_id: Some(origin.source_node_id.clone()),
                edge_outcome: Some("retry".to_string()),
                entered_at: run.started_at.clone(),
            });
        }
        validate_round_state(&round)?;
        write_json(&app.paths.round_file(task_id, &run.id, &round.id), &round)?;
    }
    copy_assets(app, task_id, &run.id, &source.id, &files)?;
    let round: RoundState = read_json(&app.paths.round_file(
        task_id,
        &run.id,
        &origin.source_round_id,
    ))?;
    let node_dsl = validated
        .get_node(&origin.source_node_id)
        .ok_or_else(|| failure("workflow.retry.target-missing", task_id, &source.id))?;
    ensure!(
        node_dsl.node_type() == NodeType::Worker,
        failure("workflow.retry.unsupported-node", task_id, &source.id)
    );
    let profile = node_dsl
        .profile()
        .and_then(|name| resolve_profile_for_node(&resolved_profiles, name));
    let node = create_node_state(
        &run.id,
        &round.id,
        &origin.source_node_id,
        run.current_attempt.as_deref().expect("rerun attempt"),
        node_dsl,
        profile,
    );
    write_node_state(
        &app.paths
            .node_file(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id),
        &node,
    )?;
    execution_plan::publish_initial_workflow_at_revision(
        &app.paths,
        task_id,
        &run.id,
        validated.raw.clone(),
        operation.model_bindings.clone(),
        origin.source_authoring_revision,
    )?;
    write_json(
        &app.paths.workflow_snapshot_file(task_id, &run.id),
        &validated.raw,
    )?;
    run.updated_at = now_rfc3339_like();
    validate_run_state(&run)?;
    // Readers cannot see a partially inherited Run: this is the publish point.
    write_json(&app.paths.run_file(task_id, &run.id), &run)?;
    Ok(PreparedRunData {
        validated,
        resolved_profiles,
        run,
        round,
        node,
    })
}

fn copy_assets(
    app: &App,
    task_id: &str,
    target_run_id: &str,
    source_run_id: &str,
    files: &[(Utf8PathBuf, Utf8PathBuf)],
) -> Result<()> {
    let runtime_dir = Dir::open_ambient_dir(&app.paths.runtime_root, ambient_authority())
        .map_err(|_| failure("workflow.retry.invalid-locator", task_id, source_run_id))?;
    let source_root = app.paths.run_dir(task_id, source_run_id);
    let source_relative = source_root
        .strip_prefix(&app.paths.runtime_root)
        .map_err(|_| failure("workflow.retry.invalid-locator", task_id, source_run_id))?;
    let source_dir = runtime_dir
        .open_dir(source_relative)
        .map_err(|_| failure("workflow.retry.invalid-locator", task_id, source_run_id))?;
    let mut copied_bytes = 0_u64;
    for (_source_file, relative) in files {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        #[cfg(windows)]
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        let source = source_dir
            .open_with(relative, &options)
            .map_err(|_| failure("workflow.retry.invalid-locator", task_id, source_run_id))?;
        let metadata = source
            .metadata()
            .map_err(|_| failure("workflow.retry.invalid-locator", task_id, source_run_id))?;
        ensure!(
            metadata.is_file(),
            failure("workflow.retry.invalid-locator", task_id, source_run_id)
        );
        let remaining = MAX_PREFIX_BYTES - copied_bytes;
        ensure!(
            metadata.len() <= remaining,
            failure("workflow.retry.prefix-too-large", task_id, source_run_id)
        );

        let target = app.paths.run_dir(task_id, target_run_id).join(relative);
        fs::create_dir_all(target.parent().expect("asset parent"))?;
        let copied = std::io::copy(
            &mut source.take(remaining.saturating_add(1)),
            &mut fs::File::create(target)?,
        )?;
        ensure!(
            copied <= remaining,
            failure("workflow.retry.prefix-too-large", task_id, source_run_id)
        );
        copied_bytes += copied;
    }
    Ok(())
}

fn accepted(app: &App, operation: &RerunOperation) -> Result<RetryCurrentNodeAccepted> {
    Ok(RetryCurrentNodeAccepted {
        operation_id: origin(operation)?.operation_id.clone(),
        locator: retry_current_locator(
            app,
            &operation.initial_run.task_id,
            &operation.initial_run,
        )?,
        run: app.run_status(&operation.initial_run.task_id, &operation.initial_run.id)?,
    })
}

fn ensure_source_sealed_for_operation(
    run: &RunState,
    operation: &RerunOperation,
    task_id: &str,
) -> Result<()> {
    let origin = origin(operation)?;
    let seal = run
        .rerun
        .as_ref()
        .and_then(|rerun| rerun.sealed_by.as_ref())
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &run.id))?;
    ensure!(
        seal.operation_id == origin.operation_id
            && seal.successor_run_id == operation.initial_run.id,
        failure("workflow.retry.source-sealed", task_id, &run.id)
    );
    Ok(())
}

fn seal_matches_operation(run: &RunState, operation: &RerunOperation) -> bool {
    let Some(origin) = operation
        .initial_run
        .rerun
        .as_ref()
        .and_then(|rerun| rerun.origin.as_ref())
    else {
        return false;
    };
    run.rerun
        .as_ref()
        .and_then(|rerun| rerun.sealed_by.as_ref())
        .is_some_and(|seal| {
            seal.operation_id == origin.operation_id
                && seal.successor_run_id == operation.initial_run.id
        })
}

fn ensure_sealed_successor_settled(app: &App, task_id: &str, source: &RunState) -> Result<()> {
    let seal = source
        .rerun
        .as_ref()
        .and_then(|rerun| rerun.sealed_by.as_ref())
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, &source.id))?;
    let successor = app
        .run_status(task_id, &seal.successor_run_id)
        .map_err(|_| failure("workflow.retry.stale-locator", task_id, &source.id))?;
    ensure!(
        successor.status == RunStatus::Completed,
        failure("workflow.retry.source-sealed", task_id, &source.id)
    );
    Ok(())
}

struct CommittedRerunOperation {
    operation: RerunOperation,
    candidate: Option<RuntimeCandidateRegistration>,
}

/// Commits the source handoff while the source dispatch write guard is held.
/// Provider cancellation, prefix materialization, asset copying, and target
/// publication deliberately happen later under the operation-specific lock.
fn commit_operation(
    app: &App,
    task_id: &str,
    run_id: &str,
    operation: &RerunOperation,
    operation_path: &Utf8PathBuf,
) -> Result<CommittedRerunOperation> {
    validate_run_state(&operation.initial_run)?;
    let source_origin = origin(operation)?;
    validate_locator(task_id, run_id, &[&operation.initial_run.id])?;
    ensure!(
        operation.initial_run.task_id == task_id && source_origin.source_run_id == run_id,
        failure("workflow.retry.stale-locator", task_id, run_id)
    );
    if app
        .paths
        .run_file(task_id, &operation.initial_run.id)
        .exists()
    {
        return Ok(CommittedRerunOperation {
            operation: operation.clone(),
            candidate: None,
        });
    }

    let mut staged = operation.clone();
    let operation_id = source_origin.operation_id.clone();
    let observed = app.run_status(task_id, run_id)?;
    if observed.is_sealed() {
        if seal_matches_operation(&observed, &staged) {
            return Ok(CommittedRerunOperation {
                operation: staged,
                candidate: None,
            });
        }
        if let Err(error) = ensure_sealed_successor_settled(app, task_id, &observed) {
            // A losing locator was never accepted; do not leave an operation
            // record that a later duplicate could recover as if it had been.
            let _ = fs::remove_file(operation_path);
            return Err(error);
        }
    }

    write_json(
        &app.paths
            .pending_node_rerun_file(task_id, &staged.initial_run.id),
        &PendingRerun {
            source_run_id: run_id.to_string(),
            operation_id: operation_id.clone(),
        },
    )?;
    let candidate = app.begin_runtime_candidate(task_id, &staged.initial_run.id)?;
    if let Some(candidate) = candidate.as_ref() {
        staged.initial_run.execution.recovery_candidate_token = Some(candidate.token().to_string());
        write_json(operation_path, &staged)?;
    }
    app.pause_run_state(task_id, run_id, PauseReason::ProcessInterrupted)
        .map_err(|error| retry_state_error(task_id, run_id, &error))?;
    // Stop and seal share the canonical current attempt lock. Late
    // continue/provider commits re-read the same source lifecycle.
    let stopped = app.run_status(task_id, run_id)?;
    let locator = stopped
        .current_execution_locator()
        .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, run_id))?;
    let lock = super::attempt_runtime_state_lock(
        app,
        task_id,
        run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
    );
    {
        let _state = lock
            .lock()
            .map_err(|_| anyhow!("attempt runtime state lock poisoned"))?;
        let mut stopped = app.run_status(task_id, run_id)?;
        ensure!(
            stopped.status != RunStatus::Running
                && stopped.pause_reason != Some(PauseReason::ErrorBlocked),
            failure("workflow.retry.source-busy", task_id, run_id)
        );
        if stopped.is_sealed() && !seal_matches_operation(&stopped, &staged) {
            ensure_sealed_successor_settled(app, task_id, &stopped)?;
        }
        if !seal_matches_operation(&stopped, &staged) {
            stopped.rerun.get_or_insert_with(Default::default).sealed_by = Some(RunRerunSeal {
                operation_id: operation_id.clone(),
                successor_run_id: staged.initial_run.id.clone(),
                sealed_at: now_rfc3339_like(),
            });
            write_json(&app.paths.run_file(task_id, run_id), &stopped)?;
        }
    }
    Ok(CommittedRerunOperation {
        operation: staged,
        candidate,
    })
}

fn launch_committed_operation(
    app: &App,
    task_id: &str,
    committed: CommittedRerunOperation,
) -> Result<RetryCurrentNodeAccepted> {
    let operation = committed.operation;
    let origin = origin(&operation)?;
    let operation_path =
        app.paths
            .node_rerun_operation_file(task_id, &origin.source_run_id, &origin.operation_id);
    crate::storage::with_file_lock(&operation_path, || {
        if app
            .paths
            .run_file(task_id, &operation.initial_run.id)
            .exists()
        {
            return accepted(app, &operation);
        }
        let data = materialize(app, task_id, &operation)?;
        if let Some(candidate) = committed.candidate {
            candidate.commit();
        }
        app.emit_lifecycle_event(super::RuntimeLifecycleEvent::ConversationRunStarted {
            project_id: app.paths.project_id.clone(),
            task_id: task_id.to_string(),
            run_id: operation.initial_run.id.clone(),
            run_mode: crate::config::ConversationRunMode::Workflow,
        });
        let result = accepted(app, &operation)?;
        let background = app.clone_for_background();
        let background_task = task_id.to_string();
        let fallback = data.run.clone();
        if let Err(error) = thread::Builder::new()
            .name("gold-band-node-rerun".to_string())
            .spawn(move || drive_committed_run(&background, &background_task, data))
        {
            app.run_pause(task_id, &fallback.id, PauseReason::RuntimeAbnormal)?;
            tracing::error!(%error, "failed to launch node rerun driver");
        }
        Ok(result)
    })
}

impl App {
    pub fn begin_run_write_admission(
        &self,
        task_id: &str,
        run_id: &str,
    ) -> Result<RunWriteAdmissionGuard> {
        let guard = execution_plan::acquire_dispatch_read(&execution_plan::lock_key(
            &self.paths,
            task_id,
            run_id,
        ))?;
        self.ensure_run_writable(task_id, run_id)?;
        Ok(RunWriteAdmissionGuard { _guard: guard })
    }

    pub fn ensure_run_writable(&self, task_id: &str, run_id: &str) -> Result<()> {
        ensure!(
            !self.run_status(task_id, run_id)?.is_sealed(),
            failure("workflow.retry.source-sealed", task_id, run_id)
        );
        Ok(())
    }

    pub fn ensure_attempt_writable(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        let run = self.run_status(task_id, run_id)?;
        ensure!(
            !run.is_sealed(),
            failure("workflow.retry.source-sealed", task_id, run_id)
        );
        if run
            .rerun
            .as_ref()
            .and_then(|rerun| rerun.origin.as_ref())
            .is_some()
        {
            let round: RoundState = read_json(&self.paths.round_file(task_id, run_id, round_id))?;
            let step = round
                .trace
                .iter()
                .find(|step| step.node_id == node_id && step.attempt_id == attempt_id)
                .ok_or_else(|| failure("workflow.retry.stale-locator", task_id, run_id))?;
            ensure!(
                !run.is_inherited_step(&round, step),
                failure("workflow.retry.inherited-attempt", task_id, run_id)
            );
        }
        Ok(())
    }

    /// Complete a committed handoff without invoking a provider. Called only
    /// by explicit startup recovery, never by a summary/detail read.
    pub fn recover_pending_node_rerun(&self, task_id: &str, run_id: &str) -> Result<bool> {
        let pending_path = self.paths.pending_node_rerun_file(task_id, run_id);
        if self.paths.run_file(task_id, run_id).exists() || !pending_path.exists() {
            return Ok(false);
        }
        let pending: PendingRerun = read_json(&pending_path)?;
        validate_locator(
            task_id,
            run_id,
            &[
                task_id,
                run_id,
                &pending.source_run_id,
                &pending.operation_id,
            ],
        )?;
        let operation = {
            let _guard = execution_plan::acquire_plan_write_wait(&execution_plan::lock_key(
                &self.paths,
                task_id,
                &pending.source_run_id,
            ))?;
            let source = self.run_status(task_id, &pending.source_run_id)?;
            if !source.is_sealed() {
                return Ok(false);
            }
            let operation: RerunOperation = read_json(&self.paths.node_rerun_operation_file(
                task_id,
                &source.id,
                &pending.operation_id,
            ))?;
            ensure!(
                operation.initial_run.id == run_id,
                failure("workflow.retry.stale-locator", task_id, &source.id)
            );
            ensure_source_sealed_for_operation(&source, &operation, task_id)?;
            operation
        };
        let operation_path = self.paths.node_rerun_operation_file(
            task_id,
            &pending.source_run_id,
            &pending.operation_id,
        );
        crate::storage::with_file_lock(&operation_path, || {
            if !self.paths.run_file(task_id, run_id).exists() {
                materialize(self, task_id, &operation)?;
                self.run_pause(task_id, run_id, PauseReason::ProcessInterrupted)?;
            }
            Ok(true)
        })
    }

    pub fn rerun_failed_node(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<RetryCurrentNodeAccepted> {
        Self::rerun_failed_node_inner(self, task_id, run_id, round_id, node_id, attempt_id)
            .map_err(|error| public_rerun_error(task_id, run_id, error))
    }

    fn rerun_failed_node_inner(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<RetryCurrentNodeAccepted> {
        validate_locator(
            task_id,
            run_id,
            &[task_id, run_id, round_id, node_id, attempt_id],
        )?;
        let source = self.run_status(task_id, run_id)?;
        ensure!(
            source.pause_reason != Some(PauseReason::ErrorBlocked),
            failure("workflow.retry.error-blocked", task_id, run_id)
        );
        let operation_id = operation_id(self, &source, round_id, node_id, attempt_id);
        let operation_path = self
            .paths
            .node_rerun_operation_file(task_id, run_id, &operation_id);
        if source.is_sealed() && operation_path.exists() {
            let committed = {
                let _dispatch = execution_plan::acquire_plan_write_wait(
                    &execution_plan::lock_key(&self.paths, task_id, run_id),
                )?;
                let operation = read_json::<RerunOperation>(&operation_path)?;
                commit_operation(self, task_id, run_id, &operation, &operation_path)?
            };
            return launch_committed_operation(self, task_id, committed);
        }
        if source.is_sealed() {
            ensure_sealed_successor_settled(self, task_id, &source)?;
        }
        // Snapshot authoring under its existing lock, executable preflight outside
        // source dispatch/attempt locks. The revision is checked again at commit.
        let authoring_key = execution_plan::authoring_lock_key(&self.paths.project_id, task_id);
        let (authoring, revision) = {
            let _guard = execution_plan::acquire_dispatch_read(&authoring_key)?;
            (
                self.task_authoring_workflow(task_id)?,
                execution_plan::read_authoring_revision(&self.paths, task_id),
            )
        };
        let workflow = validate_and_inject(
            &authoring.workflow,
            &authoring.model_bindings,
            &self.config.agents,
            &self.provider_diagnostics(),
        )?;
        let validated = validate_workflow_snapshot(workflow.clone())?;
        self.validate_workflow_agents(&validated)?;
        resolve_workflow_profiles(&self.paths, &validated.raw, self.config.desktop_language)?;
        let target = validated
            .get_node(node_id)
            .ok_or_else(|| failure("workflow.retry.target-missing", task_id, run_id))?;
        ensure!(
            target.node_type() == NodeType::Worker,
            failure("workflow.retry.unsupported-node", task_id, run_id)
        );
        let source_prefix = admitted_prefix(self, task_id, &source, round_id, node_id, attempt_id)?;
        assets(self, task_id, run_id, &source_prefix)?;
        // Existing worktree identities are reused only after source fencing.
        // A missing worktree is an error, never a fallback to the main root.
        if let Some(worktree) = source.worktree.as_ref() {
            ensure!(
                worktree.path.is_dir(),
                failure("workflow.retry.workspace-unavailable", task_id, run_id)
            );
            crate::git::GitRepositoryService::default().require_worktree(&worktree.path)?;
        } else if workflow_contains_ai_dynamic(&validated.raw) {
            crate::git::GitRepositoryService::default().require_worktree(&self.paths.repo_root)?;
        }

        let operation;
        let committed;
        {
            let _authoring = execution_plan::acquire_dispatch_read(&authoring_key)?;
            let _dispatch = execution_plan::acquire_plan_write_wait(&execution_plan::lock_key(
                &self.paths,
                task_id,
                run_id,
            ))?;
            if operation_path.exists() {
                operation = read_json::<RerunOperation>(&operation_path)?;
                if self
                    .paths
                    .run_file(task_id, &operation.initial_run.id)
                    .exists()
                {
                    return accepted(self, &operation);
                }
            } else {
                ensure!(
                    execution_plan::read_authoring_revision(&self.paths, task_id) == revision,
                    failure("workflow.retry.plan-changed", task_id, run_id)
                );
                let current = self.run_status(task_id, run_id)?;
                let rounds =
                    admitted_prefix(self, task_id, &current, round_id, node_id, attempt_id)?;
                let round = rounds.last().expect("admitted target round");
                let sequence = round.trace.last().expect("admitted target step").sequence;
                let next_attempt = attempt_id
                    .strip_prefix("attempt-")
                    .and_then(|value| value.parse::<u32>().ok())
                    .and_then(|value| value.checked_add(1))
                    .ok_or_else(|| failure("workflow.retry.invalid-locator", task_id, run_id))?;
                let (new_id, _) = reserve_next_run_dir(&self.paths.runs_dir(task_id))?;
                let now = now_rfc3339_like();
                let mut run = RunState {
                    version: VERSION.to_string(),
                    id: new_id,
                    task_id: task_id.to_string(),
                    task_uuid: current.task_uuid.clone(),
                    status: RunStatus::Running,
                    outcome: None,
                    started_at: now.clone(),
                    updated_at: now.clone(),
                    workflow_snapshot: "workflow.snapshot.json".to_string(),
                    current_round: Some(round_id.to_string()),
                    current_node: Some(node_id.to_string()),
                    current_attempt: Some(format!("attempt-{next_attempt:03}")),
                    new_rounds_opened: 0,
                    pause_reason: None,
                    uuid: Some(generate_uuid()),
                    last_executed_node: None,
                    worktree: current.worktree.clone(),
                    rerun: Some(RunRerunMetadata {
                        sealed_by: None,
                        origin: Some(RunRerunOrigin {
                            source_run_id: run_id.to_string(),
                            source_run_uuid: current.uuid.clone(),
                            source_round_id: round_id.to_string(),
                            source_round_index: round.index,
                            source_node_id: node_id.to_string(),
                            source_attempt_id: attempt_id.to_string(),
                            operation_id: operation_id.clone(),
                            inherited_trace_sequence: sequence,
                            source_authoring_revision: revision,
                        }),
                    }),
                    execution: RuntimeExecutionState::default(),
                };
                run.execution = RuntimeExecutionState::new(
                    RuntimeExecutionPhase::StartingNode,
                    run.current_execution_locator(),
                    now,
                );
                operation = RerunOperation {
                    initial_run: run,
                    workflow,
                    model_bindings: authoring.model_bindings,
                };
                write_json(&operation_path, &operation)?;
            }
            drop(_authoring);
            committed = commit_operation(self, task_id, run_id, &operation, &operation_path)?;
        }
        launch_committed_operation(self, task_id, committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::client::{PromptActivity, register_test_prompt_activity};
    use crate::domain::SessionMode;
    use crate::dsl::DynamicControlDsl;
    use crate::dynamic::{
        DynamicGraphState, DynamicNodeKind, DynamicNodeState, DynamicNodeStatus, DynamicRunPhase,
        DynamicRunState,
    };
    use crate::dynamic_store::CURRENT_DYNAMIC_GRAPH_VERSION;
    use crate::runtime::{CURRENT_ACP_STORAGE_SCHEMA_VERSION, NodeState, RoundState};
    use crate::storage::GoldBandPaths;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::tempdir;

    fn child_round() -> RoundState {
        RoundState {
            version: VERSION.to_string(),
            id: "round-001".to_string(),
            run_id: "run-child".to_string(),
            index: 1,
            status: RunStatus::Running,
            outcome: None,
            trigger: crate::domain::RoundTrigger::Initial,
            started_at: "2026-11-13T00:00:00Z".to_string(),
            trace: Vec::new(),
            uuid: None,
        }
    }

    fn child_node() -> NodeState {
        NodeState {
            version: VERSION.to_string(),
            acp_storage_schema_version: CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "child".to_string(),
            node_type: NodeType::Worker,
            run_id: "run-child".to_string(),
            round_id: "round-001".to_string(),
            attempt_id: "attempt-001".to_string(),
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-11-13T00:00:00Z".to_string(),
            finished_at: None,
            manual_check_pending: false,
            runtime_execution_id: Some("execution-child".to_string()),
            resolved_config: Default::default(),
            uuid: None,
        }
    }

    fn source_graph(repo_root: Utf8PathBuf) -> DynamicGraphState {
        DynamicGraphState {
            version: CURRENT_DYNAMIC_GRAPH_VERSION.to_string(),
            run: DynamicRunState {
                version: VERSION.to_string(),
                id: "dynamic-run-001".to_string(),
                parent_run_id: "run-source".to_string(),
                parent_round_id: "round-001".to_string(),
                parent_node_id: "dynamic".to_string(),
                parent_attempt_id: "attempt-001".to_string(),
                status: crate::dynamic::DynamicRunStatus::Running,
                phase: DynamicRunPhase::Executing,
                outcome: None,
                pause_reason: None,
                started_at: "2026-11-13T00:00:00Z".to_string(),
                updated_at: "2026-11-13T00:00:00Z".to_string(),
                control: DynamicControlDsl::default(),
                allowed_workflow_snapshots: Vec::new(),
                current_node_ids: vec!["child-flow".to_string()],
            },
            nodes: vec![DynamicNodeState {
                version: VERSION.to_string(),
                acp_storage_schema_version: CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                id: "child-flow".to_string(),
                dynamic_run_id: "dynamic-run-001".to_string(),
                kind: DynamicNodeKind::WorkflowInvocation,
                title: "child-flow".to_string(),
                task: "child-flow".to_string(),
                status: DynamicNodeStatus::Running,
                outcome: None,
                pause_reason: None,
                runtime_error: None,
                runtime_execution_id: Some("execution-dynamic".to_string()),
                runtime_execution_phase: Some(RuntimeExecutionPhase::RunningNode),
                runtime_lifecycle_revision: 1,
                runtime_lifecycle_updated_at: Some("2026-11-13T00:00:00Z".to_string()),
                group_id: None,
                chain_id: "child-flow".to_string(),
                depth: 1,
                depends_on: Vec::new(),
                workspace_id: "workspace-main".to_string(),
                provider: None,
                profile: None,
                permission_mode: None,
                auto_accept: false,
                model: None,
                session_mode: SessionMode::New,
                continue_from_node_id: None,
                workflow_id: Some("child-workflow".to_string()),
                workflow_snapshot_id: Some("snapshot-child".to_string()),
                child_run_id: Some("run-child".to_string()),
                started_at: Some("2026-11-13T00:00:00Z".to_string()),
                finished_at: None,
                uuid: None,
            }],
            groups: Vec::new(),
            workspaces: vec![crate::dynamic::WorkspaceState {
                version: VERSION.to_string(),
                id: "workspace-main".to_string(),
                dynamic_run_id: "dynamic-run-001".to_string(),
                kind: crate::dynamic::WorkspaceKind::Main,
                ownership: crate::dynamic::WorkspaceOwnership::User,
                repo_root: repo_root.clone(),
                path: repo_root,
                branch: None,
                parent_workspace_id: None,
                created_by_group_id: None,
                fork_commit: "test-head".to_string(),
                checkpoint_commit: None,
                status: crate::dynamic::WorkspaceStatus::Active,
                created_at: "2026-11-13T00:00:00Z".to_string(),
                updated_at: "2026-11-13T00:00:00Z".to_string(),
            }],
            proposals: Vec::new(),
        }
    }

    fn tree_run(id: &str, node_id: &str, status: RunStatus) -> RunState {
        RunState {
            version: VERSION.to_string(),
            id: id.to_string(),
            task_id: "task-tree".to_string(),
            task_uuid: None,
            status,
            outcome: None,
            started_at: "2026-11-13T00:00:00Z".to_string(),
            updated_at: "2026-11-13T00:00:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some(node_id.to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: (status == RunStatus::Paused).then_some(PauseReason::ProcessInterrupted),
            uuid: None,
            rerun: None,
            last_executed_node: None,
            worktree: None,
            execution: RuntimeExecutionState {
                phase: if status == RunStatus::Paused {
                    RuntimeExecutionPhase::Paused
                } else {
                    RuntimeExecutionPhase::RunningNode
                },
                ..Default::default()
            },
        }
    }

    #[test]
    fn quiesce_cancels_child_from_historical_dynamic_graph() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let app = App::with_config(repo_root.clone(), crate::config::RuntimeConfig::default());
        let paths = GoldBandPaths::new(app.paths.repo_root.clone());
        let task_id = "task-tree";

        let mut source = tree_run("run-source", "worker", RunStatus::Paused);
        source.current_round = Some("round-002".to_string());
        source.current_node = Some("worker".to_string());
        write_json(&paths.run_file(task_id, "run-source"), &source).unwrap();

        let mut historical_round = child_round();
        historical_round.run_id = "run-source".to_string();
        historical_round.status = RunStatus::Completed;
        historical_round.outcome = Some(crate::domain::RunOutcome::Success);
        historical_round.trace = vec![RoundTraceStep {
            sequence: 1,
            node_id: "dynamic".to_string(),
            attempt_id: "attempt-001".to_string(),
            from_node_id: None,
            edge_outcome: None,
            entered_at: "2026-11-13T00:00:00Z".to_string(),
        }];
        write_json(
            &paths.round_file(task_id, "run-source", "round-001"),
            &historical_round,
        )
        .unwrap();

        let mut dynamic = child_node();
        dynamic.run_id = "run-source".to_string();
        dynamic.round_id = "round-001".to_string();
        dynamic.node_id = "dynamic".to_string();
        dynamic.node_type = NodeType::AiDynamic;
        dynamic.status = RunStatus::Completed;
        dynamic.outcome = Some(crate::domain::NodeOutcome::Success);
        dynamic.runtime_execution_id = None;
        dynamic.finished_at = Some("2026-11-13T00:00:01Z".to_string());
        write_node_state(
            &paths.node_file(task_id, "run-source", "round-001", "dynamic", "attempt-001"),
            &dynamic,
        )
        .unwrap();

        let mut current_round = child_round();
        current_round.id = "round-002".to_string();
        current_round.run_id = "run-source".to_string();
        current_round.index = 2;
        current_round.status = RunStatus::Paused;
        current_round.trace = vec![RoundTraceStep {
            sequence: 1,
            node_id: "worker".to_string(),
            attempt_id: "attempt-001".to_string(),
            from_node_id: None,
            edge_outcome: None,
            entered_at: "2026-11-13T00:00:02Z".to_string(),
        }];
        write_json(
            &paths.round_file(task_id, "run-source", "round-002"),
            &current_round,
        )
        .unwrap();

        let mut worker = child_node();
        worker.run_id = "run-source".to_string();
        worker.round_id = "round-002".to_string();
        worker.node_id = "worker".to_string();
        worker.status = RunStatus::Paused;
        worker.runtime_execution_id = None;
        write_node_state(
            &paths.node_file(task_id, "run-source", "round-002", "worker", "attempt-001"),
            &worker,
        )
        .unwrap();

        let mut graph = source_graph(app.paths.repo_root.clone());
        graph.run.status = crate::dynamic::DynamicRunStatus::Completed;
        graph.run.outcome = Some(crate::domain::RunOutcome::Success);
        graph.run.current_node_ids.clear();
        graph.nodes[0].status = DynamicNodeStatus::Completed;
        graph.nodes[0].outcome = Some(crate::domain::NodeOutcome::Success);
        graph.nodes[0].runtime_execution_id = None;
        graph.nodes[0].runtime_execution_phase = Some(RuntimeExecutionPhase::Terminal);
        graph.nodes[0].finished_at = Some("2026-11-13T00:00:01Z".to_string());
        let graph_path =
            paths.dynamic_graph_file(task_id, "run-source", "round-001", "dynamic", "attempt-001");
        write_json(&graph_path, &graph).unwrap();
        let history = fs::read(&graph_path).unwrap();

        write_json(
            &paths.run_file(task_id, "run-child"),
            &tree_run("run-child", "child", RunStatus::Running),
        )
        .unwrap();
        let mut child_round = child_round();
        child_round.trace = vec![RoundTraceStep {
            sequence: 1,
            node_id: "child".to_string(),
            attempt_id: "attempt-001".to_string(),
            from_node_id: None,
            edge_outcome: None,
            entered_at: "2026-11-13T00:00:00Z".to_string(),
        }];
        write_json(
            &paths.round_file(task_id, "run-child", "round-001"),
            &child_round,
        )
        .unwrap();
        write_node_state(
            &paths.node_file(task_id, "run-child", "round-001", "child", "attempt-001"),
            &child_node(),
        )
        .unwrap();
        let child_attempt =
            paths.attempt_dir(task_id, "run-child", "round-001", "child", "attempt-001");
        let activity =
            register_test_prompt_activity(child_attempt.clone(), PromptActivity::Running);
        let background = app.clone_for_background();
        let worker = thread::spawn(move || quiesce(&background, task_id, "run-source"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let cancelled = loop {
            if crate::acp::client::prompt_activity(&child_attempt)
                == Some(PromptActivity::CancelRequested)
            {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        };
        activity.mark_stopped();
        worker.join().unwrap().unwrap();
        assert!(
            cancelled,
            "historical committed graph child must be cancelled"
        );
        assert_eq!(fs::read(&graph_path).unwrap(), history);
        assert_eq!(
            app.run_status(task_id, "run-child").unwrap().status,
            RunStatus::Paused
        );
    }

    #[test]
    fn asset_copy_rejects_a_directory_replaced_after_preflight() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let app = App::with_config(repo_root, crate::config::RuntimeConfig::default());
        let paths = GoldBandPaths::new(app.paths.repo_root.clone());
        let task_id = "task-assets";
        let mut round = child_round();
        round.run_id = "run-source".to_string();
        round.trace = vec![RoundTraceStep {
            sequence: 1,
            node_id: "worker".to_string(),
            attempt_id: "attempt-001".to_string(),
            from_node_id: None,
            edge_outcome: None,
            entered_at: "2026-11-13T00:00:00Z".to_string(),
        }];
        write_json(
            &paths.round_file(task_id, "run-source", "round-001"),
            &round,
        )
        .unwrap();
        let mut node = child_node();
        node.run_id = "run-source".to_string();
        node.node_id = "worker".to_string();
        write_node_state(
            &paths.node_file(task_id, "run-source", "round-001", "worker", "attempt-001"),
            &node,
        )
        .unwrap();

        let artifact = paths.artifact_file(
            task_id,
            "run-source",
            "round-001",
            "worker",
            "attempt-001",
            "secret.txt",
        );
        fs::create_dir_all(artifact.parent().unwrap()).unwrap();
        fs::write(&artifact, "safe").unwrap();
        let files = assets(&app, task_id, "run-source", &[round.clone()]).unwrap();
        assert_eq!(files.len(), 1);

        let outside = Utf8PathBuf::from_path_buf(temp.path().join("outside")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt.json"), "external").unwrap();
        fs::remove_dir_all(artifact.parent().unwrap()).unwrap();
        let link = artifact.parent().unwrap().to_path_buf();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            let output = crate::process::background_command("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "failed to create test junction: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        assert!(link.is_dir(), "test replacement link must exist");
        assert_eq!(
            fs::read_to_string(&files[0].0).unwrap(),
            "external",
            "link={} outside={} source={}",
            link,
            outside,
            files[0].0
        );
        let result = copy_assets(&app, task_id, "run-target", "run-source", &files);
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
        let error = result.unwrap_err();
        assert_eq!(
            crate::runtime_error::normalize_runtime_error(&error).code_str(),
            "workflow.retry.invalid-locator",
            "asset replacement must be rejected as invalid locator: {error:#}"
        );
        let copied = paths.artifact_file(
            task_id,
            "run-target",
            "round-001",
            "worker",
            "attempt-001",
            "secret.txt",
        );
        assert!(
            !copied.exists(),
            "an replaced external asset must not be copied"
        );
    }

    fn traced_round(run_id: &str, node_id: &str) -> RoundState {
        let mut round = child_round();
        round.run_id = run_id.to_string();
        round.trace = vec![RoundTraceStep {
            sequence: 1,
            node_id: node_id.to_string(),
            attempt_id: "attempt-001".to_string(),
            from_node_id: None,
            edge_outcome: None,
            entered_at: "2026-11-13T00:00:00Z".to_string(),
        }];
        round
    }

    fn traced_node(run_id: &str, node_id: &str, node_type: NodeType) -> NodeState {
        let mut node = child_node();
        node.run_id = run_id.to_string();
        node.node_id = node_id.to_string();
        node.node_type = node_type;
        node
    }

    #[test]
    fn quiesce_cancels_and_waits_for_sibling_child_run() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let app = App::with_config(repo_root, crate::config::RuntimeConfig::default());
        let paths = GoldBandPaths::new(app.paths.repo_root.clone());
        let task_id = "task-tree";
        let source_run_id = "run-source";
        write_json(
            &paths.run_file(task_id, source_run_id),
            &tree_run(source_run_id, "dynamic", RunStatus::Paused),
        )
        .unwrap();
        write_json(
            &paths.round_file(task_id, source_run_id, "round-001"),
            &traced_round(source_run_id, "dynamic"),
        )
        .unwrap();
        write_node_state(
            &paths.node_file(
                task_id,
                source_run_id,
                "round-001",
                "dynamic",
                "attempt-001",
            ),
            &traced_node(source_run_id, "dynamic", NodeType::AiDynamic),
        )
        .unwrap();
        write_json(
            &paths.run_file(task_id, "run-child"),
            &tree_run("run-child", "child", RunStatus::Running),
        )
        .unwrap();
        let child_attempt =
            paths.attempt_dir(task_id, "run-child", "round-001", "child", "attempt-001");
        write_json(
            &paths.round_file(task_id, "run-child", "round-001"),
            &traced_round("run-child", "child"),
        )
        .unwrap();
        write_node_state(
            &paths.node_file(task_id, "run-child", "round-001", "child", "attempt-001"),
            &traced_node("run-child", "child", NodeType::AiDynamic),
        )
        .unwrap();
        write_json(
            &paths.dynamic_graph_file(
                task_id,
                source_run_id,
                "round-001",
                "dynamic",
                "attempt-001",
            ),
            &source_graph(app.paths.repo_root.clone()),
        )
        .unwrap();

        let mut child_graph = source_graph(app.paths.repo_root.clone());
        child_graph.run.parent_run_id = "run-child".to_string();
        child_graph.run.parent_node_id = "child".to_string();
        child_graph.nodes[0].child_run_id = Some("run-grandchild".to_string());
        write_json(
            &paths.dynamic_graph_file(task_id, "run-child", "round-001", "child", "attempt-001"),
            &child_graph,
        )
        .unwrap();
        let mut nested_node = child_node();
        nested_node.node_type = NodeType::AiDynamic;
        write_node_state(
            &paths.node_file(task_id, "run-child", "round-001", "child", "attempt-001"),
            &nested_node,
        )
        .unwrap();
        write_json(
            &paths.run_file(task_id, "run-grandchild"),
            &tree_run("run-grandchild", "child", RunStatus::Running),
        )
        .unwrap();
        let grandchild_round = traced_round("run-grandchild", "child");
        write_json(
            &paths.round_file(task_id, "run-grandchild", "round-001"),
            &grandchild_round,
        )
        .unwrap();
        let mut grandchild_node = child_node();
        grandchild_node.run_id = "run-grandchild".to_string();
        write_node_state(
            &paths.node_file(
                task_id,
                "run-grandchild",
                "round-001",
                "child",
                "attempt-001",
            ),
            &grandchild_node,
        )
        .unwrap();
        let grandchild_attempt = paths.attempt_dir(
            task_id,
            "run-grandchild",
            "round-001",
            "child",
            "attempt-001",
        );
        let grandchild_activity =
            register_test_prompt_activity(grandchild_attempt.clone(), PromptActivity::Starting);

        let registration =
            register_test_prompt_activity(child_attempt.clone(), PromptActivity::Running);
        let unrelated_attempt = paths.attempt_dir(
            task_id,
            "run-unrelated",
            "round-001",
            "other",
            "attempt-001",
        );
        let _unrelated =
            register_test_prompt_activity(unrelated_attempt.clone(), PromptActivity::Running);
        let (done, completion) = mpsc::channel();
        let background_app = app.clone_for_background();
        let worker = std::thread::spawn(move || {
            let result = quiesce(&background_app, task_id, source_run_id);
            done.send(()).unwrap();
            result
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let cancelled = loop {
            if [&child_attempt, &grandchild_attempt].iter().all(|attempt| {
                crate::acp::client::prompt_activity(attempt)
                    == Some(PromptActivity::CancelRequested)
            }) {
                break true;
            }
            if Instant::now() >= deadline || completion.try_recv().is_ok() {
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        };
        let returned_while_cancelling = completion.recv_timeout(Duration::from_millis(100)).is_ok();
        registration.mark_stopped();
        let returned_with_active_grandchild =
            completion.recv_timeout(Duration::from_millis(100)).is_ok();
        grandchild_activity.mark_stopped();
        worker.join().unwrap().unwrap();
        assert!(
            cancelled,
            "quiesce must cancel the active sibling child run before returning"
        );
        assert!(
            !returned_while_cancelling,
            "cancel requested is not provider stopped"
        );
        assert!(
            !returned_with_active_grandchild,
            "every descendant must stop before handoff"
        );
        assert_eq!(
            app.run_status(task_id, "run-grandchild").unwrap().status,
            RunStatus::Paused
        );
        assert_eq!(
            crate::acp::client::prompt_activity(&unrelated_attempt),
            Some(PromptActivity::Running)
        );
    }

    #[test]
    fn quiesce_preserves_completed_execution_tree_history() {
        let temp = tempdir().unwrap();
        let app = App::with_config(
            Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap(),
            crate::config::RuntimeConfig::default(),
        );
        for (run_id, node_id) in [("run-source", "dynamic"), ("run-child", "child")] {
            let mut run = tree_run(run_id, node_id, RunStatus::Completed);
            run.outcome = Some(crate::domain::RunOutcome::Success);
            run.execution.phase = RuntimeExecutionPhase::Terminal;
            write_json(&app.paths.run_file("task-tree", run_id), &run).unwrap();
        }
        for (run_id, node_id) in [("run-source", "dynamic"), ("run-child", "child")] {
            let mut round = traced_round(run_id, node_id);
            round.status = RunStatus::Completed;
            round.outcome = Some(crate::domain::RunOutcome::Success);
            write_json(
                &app.paths.round_file("task-tree", run_id, "round-001"),
                &round,
            )
            .unwrap();
            let mut node = traced_node(
                run_id,
                node_id,
                if run_id == "run-source" {
                    NodeType::AiDynamic
                } else {
                    NodeType::Worker
                },
            );
            node.status = RunStatus::Completed;
            node.outcome = Some(crate::domain::NodeOutcome::Success);
            node.runtime_execution_id = None;
            node.finished_at = Some("2026-11-13T00:00:01Z".to_string());
            write_node_state(
                &app.paths
                    .node_file("task-tree", run_id, "round-001", node_id, "attempt-001"),
                &node,
            )
            .unwrap();
        }
        let mut graph = source_graph(app.paths.repo_root.clone());
        graph.run.status = crate::dynamic::DynamicRunStatus::Completed;
        graph.run.outcome = Some(crate::domain::RunOutcome::Success);
        graph.run.current_node_ids.clear();
        graph.nodes[0].status = DynamicNodeStatus::Completed;
        graph.nodes[0].outcome = Some(crate::domain::NodeOutcome::Success);
        graph.nodes[0].runtime_execution_id = None;
        graph.nodes[0].runtime_execution_phase = Some(RuntimeExecutionPhase::Terminal);
        graph.nodes[0].finished_at = Some("2026-11-13T00:00:01Z".to_string());
        let graph_path = app.paths.dynamic_graph_file(
            "task-tree",
            "run-source",
            "round-001",
            "dynamic",
            "attempt-001",
        );
        write_json(&graph_path, &graph).unwrap();
        let history = fs::read(&graph_path).unwrap();
        let child_attempt = app.paths.attempt_dir(
            "task-tree",
            "run-child",
            "round-001",
            "child",
            "attempt-001",
        );
        let activity =
            register_test_prompt_activity(child_attempt.clone(), PromptActivity::Running);
        let background = app.clone_for_background();
        let worker = thread::spawn(move || quiesce(&background, "task-tree", "run-source"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let cancelled = loop {
            if crate::acp::client::prompt_activity(&child_attempt)
                == Some(PromptActivity::CancelRequested)
            {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(5));
        };
        activity.mark_stopped();
        worker.join().unwrap().unwrap();
        assert!(
            cancelled,
            "completed graph must still expose child activity"
        );
        assert_eq!(
            fs::read(&graph_path).unwrap(),
            history,
            "handoff must preserve the completed graph and its outcome"
        );
        for run_id in ["run-source", "run-child"] {
            let run = app.run_status("task-tree", run_id).unwrap();
            assert_eq!(run.status, RunStatus::Completed);
            assert_eq!(run.outcome, Some(crate::domain::RunOutcome::Success));
        }
    }

    #[test]
    fn quiesce_rejects_invalid_and_oversized_execution_trees() {
        for (case, expected) in [
            ("cycle", "workflow.retry.stale-locator"),
            ("missing-child", "workflow.retry.stale-locator"),
            ("foreign-parent", "workflow.retry.stale-locator"),
            ("invalid-path", "workflow.retry.invalid-locator"),
            ("run-limit", "workflow.retry.prefix-too-large"),
            ("metadata-limit", "workflow.retry.prefix-too-large"),
            ("pause-state-invalid", "workflow.retry.workflow-invalid"),
            ("graph-write-failed", "workflow.retry.workflow-invalid"),
        ] {
            let temp = tempdir().unwrap();
            let app = App::with_config(
                Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap(),
                crate::config::RuntimeConfig::default(),
            );
            write_json(
                &app.paths.run_file("task-tree", "run-source"),
                &tree_run(
                    "run-source",
                    "dynamic",
                    if case == "pause-state-invalid" {
                        RunStatus::Running
                    } else {
                        RunStatus::Paused
                    },
                ),
            )
            .unwrap();
            if case != "pause-state-invalid" {
                write_json(
                    &app.paths.round_file("task-tree", "run-source", "round-001"),
                    &traced_round("run-source", "dynamic"),
                )
                .unwrap();
                write_node_state(
                    &app.paths.node_file(
                        "task-tree",
                        "run-source",
                        "round-001",
                        "dynamic",
                        "attempt-001",
                    ),
                    &traced_node("run-source", "dynamic", NodeType::AiDynamic),
                )
                .unwrap();
            }
            let mut graph = source_graph(app.paths.repo_root.clone());
            match case {
                "cycle" => graph.nodes[0].child_run_id = Some("run-source".to_string()),
                "foreign-parent" => graph.run.parent_run_id = "run-other".to_string(),
                "invalid-path" => graph.nodes[0].child_run_id = Some("../run-other".to_string()),
                "graph-write-failed" => {
                    graph.nodes[0].child_run_id = None;
                    fs::create_dir_all(app.paths.dynamic_run_file(
                        "task-tree",
                        "run-source",
                        "round-001",
                        "dynamic",
                        "attempt-001",
                    ))
                    .unwrap();
                }
                "run-limit" => {
                    let template = graph.nodes[0].clone();
                    graph.nodes = (0..MAX_QUIESCE_RUNS)
                        .map(|index| {
                            let mut node = template.clone();
                            node.id = format!("child-{index}");
                            node.child_run_id = Some(format!("run-child-{index}"));
                            node
                        })
                        .collect();
                    graph.run.current_node_ids =
                        graph.nodes.iter().map(|node| node.id.clone()).collect();
                }
                _ => {}
            }
            let graph_path = app.paths.dynamic_graph_file(
                "task-tree",
                "run-source",
                "round-001",
                "dynamic",
                "attempt-001",
            );
            write_json(&graph_path, &graph).unwrap();
            if case == "metadata-limit" {
                fs::File::options()
                    .write(true)
                    .open(&graph_path)
                    .unwrap()
                    .set_len(MAX_QUIESCE_METADATA_BYTES + 1)
                    .unwrap();
            }
            let error = quiesce(&app, "task-tree", "run-source").unwrap_err();
            assert_eq!(
                crate::runtime_error::normalize_runtime_error(&error).code_str(),
                expected,
                "{case}: {error:#}"
            );
        }
    }

    #[test]
    fn quiesce_rejects_a_child_run_link_outside_runtime_storage() {
        let temp = tempdir().unwrap();
        let app = App::with_config(
            Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap(),
            crate::config::RuntimeConfig::default(),
        );
        write_json(
            &app.paths.run_file("task-tree", "run-source"),
            &tree_run("run-source", "dynamic", RunStatus::Paused),
        )
        .unwrap();
        write_json(
            &app.paths.round_file("task-tree", "run-source", "round-001"),
            &traced_round("run-source", "dynamic"),
        )
        .unwrap();
        write_node_state(
            &app.paths.node_file(
                "task-tree",
                "run-source",
                "round-001",
                "dynamic",
                "attempt-001",
            ),
            &traced_node("run-source", "dynamic", NodeType::AiDynamic),
        )
        .unwrap();
        let graph = source_graph(app.paths.repo_root.clone());
        write_json(
            &app.paths.dynamic_graph_file(
                "task-tree",
                "run-source",
                "round-001",
                "dynamic",
                "attempt-001",
            ),
            &graph,
        )
        .unwrap();
        let outside = Utf8PathBuf::from_path_buf(temp.path().join("outside-run")).unwrap();
        write_json(
            &outside.join("run.json"),
            &tree_run("run-child", "child", RunStatus::Paused),
        )
        .unwrap();
        let link = app.paths.run_dir("task-tree", "run-child");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            let output = crate::process::background_command("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "failed to create test junction: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let result = quiesce(&app, "task-tree", "run-source");
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
        assert_eq!(
            crate::runtime_error::normalize_runtime_error(&result.unwrap_err()).code_str(),
            "workflow.retry.invalid-locator"
        );
    }
}
