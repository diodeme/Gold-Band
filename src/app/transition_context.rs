use anyhow::Result;
use camino::Utf8PathBuf;
use std::fs;

use crate::domain::{NodeType, SessionMode};
use crate::runtime::NodeState;

use super::App;
use super::ids::latest_attempt_id;

pub(crate) fn find_latest_artifact_path(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    name: &str,
) -> Result<Option<Utf8PathBuf>> {
    let node_dir = app.paths.node_dir(task_id, run_id, round_id, node_id);
    if !node_dir.exists() {
        return Ok(None);
    }
    let attempt_id = latest_attempt_id(&node_dir)?;
    Ok(attempt_id.map(|attempt_id| {
        app.paths
            .artifact_file(task_id, run_id, round_id, node_id, &attempt_id, name)
    }))
}

pub(crate) fn find_latest_worker_ref_for_transition(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    previous_node: &NodeState,
    target_node_id: &str,
    session_mode: SessionMode,
) -> Result<Option<Utf8PathBuf>> {
    if session_mode != SessionMode::Continue {
        return Ok(None);
    }
    if previous_node.node_type != NodeType::Exec {
        return Ok(None);
    }
    let path = app
        .paths
        .worker_ref_file(task_id, run_id, round_id, target_node_id, "attempt-001");
    if path.exists() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

pub(crate) fn feedback_summary_from_previous_node(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node: &NodeState,
) -> Result<Option<String>> {
    match node.node_type {
        NodeType::Exec => {
            let path = app.paths.artifact_file(
                task_id,
                run_id,
                round_id,
                &node.node_id,
                &node.attempt_id,
                "exec-result",
            );
            if path.exists() {
                Ok(Some(fs::read_to_string(path)?))
            } else {
                Ok(None)
            }
        }
        NodeType::Worker => {
            let node_state_path = app
                .paths
                .node_file(task_id, run_id, round_id, &node.node_id, &node.attempt_id);
            let node_state: NodeState = crate::storage::read_json(&node_state_path)?;
            let Some(artifact) = node_state
                .resolved_config
                .get("primaryArtifact")
                .or_else(|| node_state.resolved_config.get("outputArtifact"))
                .and_then(|value| value.as_str())
            else {
                return Ok(None);
            };
            let path = app.paths.artifact_file(
                task_id,
                run_id,
                round_id,
                &node.node_id,
                &node.attempt_id,
                artifact,
            );
            if path.exists() {
                Ok(Some(fs::read_to_string(path)?))
            } else {
                Ok(None)
            }
        }
    }
}
