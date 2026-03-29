use crate::domain::{ResolvedConfig, RunStatus, VERSION};
use crate::dsl::NodeDsl;
use crate::runtime::NodeState;

use super::ids::now_rfc3339_like;

pub(crate) fn create_node_state(run_id: &str, round_id: &str, node_id: &str, attempt_id: &str, node_dsl: &NodeDsl) -> NodeState {
    NodeState {
        version: VERSION.to_string(),
        node_id: node_id.to_string(),
        node_type: node_dsl.node_type(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        attempt_id: attempt_id.to_string(),
        status: RunStatus::Running,
        outcome: None,
        started_at: now_rfc3339_like(),
        finished_at: None,
        resolved_config: resolved_config_for_node(node_dsl),
    }
}

pub(crate) fn resolved_config_for_node(node: &NodeDsl) -> ResolvedConfig {
    let mut config = ResolvedConfig::new();
    match node {
        NodeDsl::Worker(worker) => {
            config.insert(
                "provider".to_string(),
                serde_json::Value::String(worker.provider.clone().unwrap_or_else(|| "claude-code".to_string())),
            );
            if let Some(profile) = &worker.profile {
                config.insert("profile".to_string(), serde_json::Value::String(profile.clone()));
            }
            if let Some(primary_artifact) = &worker.primary_artifact {
                config.insert("primaryArtifact".to_string(), serde_json::Value::String(primary_artifact.clone()));
            }
            config.insert("sessionMode".to_string(), serde_json::Value::String("new".to_string()));
        }
        NodeDsl::Exec(exec) => {
            config.insert("planFrom".to_string(), serde_json::Value::String(exec.plan_from.clone()));
        }
        NodeDsl::Verify(verify) => {
            config.insert(
                "provider".to_string(),
                serde_json::Value::String(verify.provider.clone().unwrap_or_else(|| "claude-code".to_string())),
            );
            if let Some(profile) = &verify.profile {
                config.insert("profile".to_string(), serde_json::Value::String(profile.clone()));
            }
            config.insert("primaryArtifact".to_string(), serde_json::Value::String("verify-result".to_string()));
            config.insert("evidenceScope".to_string(), serde_json::Value::String("current-round".to_string()));
        }
    }
    config
}
