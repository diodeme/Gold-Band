use anyhow::{anyhow, bail, Result};
use camino::Utf8Path;

use crate::artifacts::{
    validate_exec_plan, validate_exec_result, validate_verify_result, ExecPlanArtifact, ExecResultArtifact, ExecResultStatus, VerifyResultArtifact,
    VerifyStatus,
};
use crate::domain::{InvocationKind, NodeOutcome, RunStatus, SessionMode, VERSION};
use crate::dsl::{NodeDsl, ValidatedWorkflow};
use crate::exec::run_exec_plan;
use crate::observability::{progress, ProgressStage};
use crate::provider::{ColdFileRef, ProviderRunResult, ProviderRunStatus, StreamMode, WorkerInvocation};
use crate::runtime::{validate_node_state, validate_worker_ref_state, NodeState, WorkerRefState};
use crate::storage::{read_json, write_json};

use super::ids::now_rfc3339_like;
use super::transition_context::{
    find_latest_artifact_path, find_verify_attachment_paths, find_verify_exec_result_path, find_verify_worker_primary_artifact,
};
use super::App;

pub(crate) fn execute_ai_node(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    attempt_id: &str,
    workflow: &ValidatedWorkflow,
    node_id: &str,
    node: NodeState,
    session_mode: SessionMode,
    continue_ref: Option<serde_json::Value>,
    feedback_summary: Option<String>,
    verify_result_path: Option<&Utf8Path>,
) -> Result<NodeState> {
    let node_dsl = workflow.get_node(node_id).expect("validated node exists");
    let (profile, primary_artifact, task_instruction, invocation_kind, cold_artifacts, cold_attachments) = match node_dsl {
        NodeDsl::Worker(worker) => {
            let kind = if verify_result_path.is_some() {
                InvocationKind::WorkerRepairVerify
            } else if feedback_summary.is_some() {
                InvocationKind::WorkerRepairExec
            } else {
                InvocationKind::WorkerGeneric
            };
            (worker.profile.clone(), worker.primary_artifact.clone(), worker.goal.clone(), kind, Vec::new(), Vec::new())
        }
        NodeDsl::Verify(verify) => {
            let mut artifacts = Vec::new();
            if let Some(path) = find_verify_exec_result_path(app, task_id, run_id, round_id, workflow, node_id)? {
                artifacts.push(ColdFileRef {
                    name: Some("exec-result".to_string()),
                    path,
                });
            }
            if let Some(path) = find_verify_worker_primary_artifact(app, task_id, run_id, round_id, workflow, node_id)? {
                artifacts.push(ColdFileRef {
                    name: Some("worker-primary-artifact".to_string()),
                    path,
                });
            }
            let cold_attachments = find_verify_attachment_paths(app, task_id, run_id, round_id, workflow, node_id)?
                .into_iter()
                .map(|path| ColdFileRef { name: None, path })
                .collect::<Vec<_>>();
            (
                verify.profile.clone(),
                Some("verify-result".to_string()),
                Some("Evaluate whether the requirement is satisfied based only on the provided evidence and produce a verify-result.".to_string()),
                InvocationKind::VerifyAcceptance,
                artifacts,
                cold_attachments,
            )
        }
        NodeDsl::Exec(_) => bail!("execute_ai_node cannot run exec nodes"),
    };

    let invocation = WorkerInvocation {
        invocation_kind,
        profile,
        requirement_path: Some(app.paths.requirement_file(task_id)),
        requirement_text: None,
        workspace_dir: app.paths.repo_root.clone(),
        attempt_dir: app.paths.attempt_dir(task_id, run_id, round_id, node_id, attempt_id),
        primary_artifact,
        task_instruction,
        session_mode,
        continue_ref,
        stream_mode: StreamMode::StreamJson,
        log_prompts: app.config.log_prompts,
        log_provider_command: app.config.log_provider_command,
        feedback_summary,
        verify_result_path: verify_result_path.map(ToOwned::to_owned),
        attachments_dir: matches!(node_dsl, NodeDsl::Worker(_))
            .then(|| app.paths.attachments_dir(task_id, run_id, round_id, node_id, attempt_id)),
        cold_artifacts,
        cold_attachments,
    };

    progress(&format!("calling provider for {}/{}/{}", round_id, node_id, attempt_id));
    progress(&format!("raw stream file: {}", app.paths.raw_stream_file(task_id, run_id, round_id, node_id, attempt_id)));
    tracing::debug!(task_id, run_id, round_id, node_id, attempt_id, stage = ?ProgressStage::CallingProvider, "calling provider");
    let result = app.provider.run_worker(invocation)?;
    progress(&format!("normalizing artifact for {}/{}/{}", round_id, node_id, attempt_id));
    tracing::debug!(task_id, run_id, round_id, node_id, attempt_id, stage = ?ProgressStage::NormalizingArtifact, "normalizing provider result");
    finalize_ai_attempt(app, task_id, run_id, round_id, attempt_id, node_id, node, result)
}

pub(crate) fn execute_exec_node(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    workflow: &ValidatedWorkflow,
    mut node: NodeState,
) -> Result<NodeState> {
    let node_dsl = workflow.get_node(&node.node_id).expect("validated node exists");
    let exec_node = match node_dsl {
        NodeDsl::Exec(exec) => exec,
        _ => bail!("execute_exec_node requires exec node"),
    };

    let exec_plan_path = find_latest_artifact_path(app, task_id, run_id, round_id, &exec_node.plan_from, "exec-plan")?
        .ok_or_else(|| anyhow!("exec-plan not found for current round"))?;
    let exec_plan: ExecPlanArtifact = read_json(&exec_plan_path)?;
    validate_exec_plan(&exec_plan)?;

    progress(&format!("running command for {}/{}/{}", round_id, node.node_id, node.attempt_id));
    tracing::debug!(task_id, run_id, round_id, node_id = %node.node_id, attempt_id = %node.attempt_id, stage = ?ProgressStage::RunningCommand, "running exec plan");
    let exec_result = run_exec_plan(
        &exec_plan,
        &app.paths.repo_root,
        &app.paths.attempt_dir(task_id, run_id, round_id, &node.node_id, &node.attempt_id),
    )?;
    validate_exec_result(&exec_result)?;
    write_json(
        &app.paths.artifact_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id, "exec-result"),
        &exec_result,
    )?;

    node.status = RunStatus::Completed;
    node.outcome = Some(match exec_result.status {
        ExecResultStatus::Success => NodeOutcome::Success,
        ExecResultStatus::Failure => NodeOutcome::Failure,
    });
    node.finished_at = Some(now_rfc3339_like());
    validate_node_state(&node)?;
    Ok(node)
}

pub(crate) fn finalize_ai_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    attempt_id: &str,
    node_id: &str,
    mut node: NodeState,
    result: ProviderRunResult,
) -> Result<NodeState> {
    node.finished_at = Some(now_rfc3339_like());
    match result.status {
        ProviderRunStatus::Success => {
            if let Some(payload) = result.result_payload {
                if let Some(primary_artifact) = payload.primary_artifact {
                    let artifact_path = app.paths.artifact_file(task_id, run_id, round_id, node_id, attempt_id, &primary_artifact.name);
                    match primary_artifact.name.as_str() {
                        "exec-plan" => {
                            let plan: ExecPlanArtifact = serde_json::from_str(&primary_artifact.content)?;
                            validate_exec_plan(&plan)?;
                            write_json(&artifact_path, &plan)?;
                        }
                        "verify-result" => {
                            let verify: VerifyResultArtifact = serde_json::from_str(&primary_artifact.content)?;
                            validate_verify_result(&verify)?;
                            write_json(&artifact_path, &verify)?;
                        }
                        _ => {
                            std::fs::create_dir_all(app.paths.artifacts_dir(task_id, run_id, round_id, node_id, attempt_id).as_std_path())?;
                            std::fs::write(artifact_path.as_std_path(), primary_artifact.content)?;
                        }
                    }
                }
            }

            if let Some(seed) = result.worker_ref_seed {
                let worker_ref = WorkerRefState {
                    version: VERSION.to_string(),
                    provider: seed.provider,
                    mode: seed.mode,
                    supports_open_session: seed.supports_open_session,
                    supports_continue_session: seed.supports_continue_session,
                    continue_ref: seed.continue_ref,
                    open_command: seed.open_command,
                };
                validate_worker_ref_state(&worker_ref)?;
                write_json(&app.paths.worker_ref_file(task_id, run_id, round_id, node_id, attempt_id), &worker_ref)?;
            }

            let needs_primary_artifact = node.resolved_config.contains_key("primaryArtifact");
            let expected_artifact = node
                .resolved_config
                .get("primaryArtifact")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let has_artifact = expected_artifact.as_ref().is_some_and(|artifact| {
                app.paths.artifact_file(task_id, run_id, round_id, node_id, attempt_id, artifact).exists()
            });
            node.status = RunStatus::Completed;
            node.outcome = Some(if needs_primary_artifact && !has_artifact {
                NodeOutcome::Invalid
            } else if matches!(node.node_type, crate::domain::NodeType::Verify) {
                let verify: VerifyResultArtifact = read_json(
                    &app.paths.artifact_file(task_id, run_id, round_id, node_id, attempt_id, "verify-result"),
                )?;
                match verify.status {
                    VerifyStatus::Success => NodeOutcome::Success,
                    VerifyStatus::Failure => NodeOutcome::Failure,
                }
            } else {
                NodeOutcome::Success
            });
        }
        ProviderRunStatus::Failure => {
            node.status = RunStatus::Completed;
            node.outcome = Some(NodeOutcome::Failure);
        }
        ProviderRunStatus::Interrupted => {
            node.status = RunStatus::Paused;
            node.outcome = None;
        }
    }
    validate_node_state(&node)?;
    Ok(node)
}

pub(crate) fn re_evaluate_attempt(app: &App, task_id: &str, run_id: &str, round_id: &str, mut node: NodeState) -> Result<NodeState> {
    let artifact_name = match node.node_type {
        crate::domain::NodeType::Worker => node.resolved_config.get("primaryArtifact").and_then(|value| value.as_str()).map(str::to_string),
        crate::domain::NodeType::Exec => Some("exec-result".to_string()),
        crate::domain::NodeType::Verify => Some("verify-result".to_string()),
    };

    if let Some(artifact_name) = artifact_name {
        let path = app.paths.artifact_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id, &artifact_name);
        if !path.exists() {
            node.status = RunStatus::Completed;
            node.outcome = Some(NodeOutcome::Invalid);
            validate_node_state(&node)?;
            write_json(&app.paths.node_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id), &node)?;
            return Ok(node);
        }

        match artifact_name.as_str() {
            "exec-plan" => {
                let value: ExecPlanArtifact = read_json(&path)?;
                validate_exec_plan(&value)?;
                node.outcome = Some(NodeOutcome::Success);
            }
            "exec-result" => {
                let value: ExecResultArtifact = read_json(&path)?;
                validate_exec_result(&value)?;
                node.outcome = Some(match value.status {
                    ExecResultStatus::Success => NodeOutcome::Success,
                    ExecResultStatus::Failure => NodeOutcome::Failure,
                });
            }
            "verify-result" => {
                let value: VerifyResultArtifact = read_json(&path)?;
                validate_verify_result(&value)?;
                node.outcome = Some(match value.status {
                    VerifyStatus::Success => NodeOutcome::Success,
                    VerifyStatus::Failure => NodeOutcome::Failure,
                });
            }
            _ => node.outcome = Some(NodeOutcome::Success),
        }
    }

    node.status = RunStatus::Completed;
    node.finished_at = Some(now_rfc3339_like());
    validate_node_state(&node)?;
    write_json(&app.paths.node_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id), &node)?;
    Ok(node)
}
