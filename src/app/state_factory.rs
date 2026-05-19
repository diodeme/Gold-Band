use crate::config::ResolvedProfileRef;
use crate::domain::{ResolvedConfig, RunStatus, VERSION};
use crate::dsl::{JsonConditionDsl, NodeDsl};
use crate::runtime::NodeState;

use super::ids::now_rfc3339_like;

pub(crate) fn create_node_state(
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    node_dsl: &NodeDsl,
    resolved_profile: Option<ResolvedProfileRef>,
) -> NodeState {
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
        manual_check_pending: false,
        resolved_config: resolved_config_for_node(node_dsl, resolved_profile),
    }
}

pub(crate) fn resolved_config_for_node(
    node: &NodeDsl,
    resolved_profile: Option<ResolvedProfileRef>,
) -> ResolvedConfig {
    let mut config = ResolvedConfig::new();
    match node {
        NodeDsl::Worker(worker) => {
            config.insert(
                "provider".to_string(),
                serde_json::Value::String(
                    worker
                        .provider
                        .clone()
                        .expect("validated worker provider must exist"),
                ),
            );
            if let Some(profile) = &worker.profile {
                config.insert(
                    "profile".to_string(),
                    serde_json::Value::String(profile.clone()),
                );
            }
            if let Some(profile) = resolved_profile.as_ref() {
                config.insert(
                    "profileSource".to_string(),
                    serde_json::to_value(&profile.source).expect("serialize profile source"),
                );
                config.insert(
                    "profilePath".to_string(),
                    serde_json::Value::String(profile.path.clone()),
                );
            }
            if let Some(primary_artifact) = &worker.primary_artifact {
                config.insert(
                    "primaryArtifact".to_string(),
                    serde_json::Value::String(primary_artifact.clone()),
                );
            }
            if let Some(output) = &worker.output {
                config.insert(
                    "outputKind".to_string(),
                    serde_json::to_value(output.kind).expect("serialize output kind"),
                );
                config.insert(
                    "outputArtifact".to_string(),
                    serde_json::Value::String(output.artifact.clone()),
                );
                if let Some(schema) = &output.schema {
                    config.insert("outputSchema".to_string(), schema.clone());
                }
            }
            if let Some(condition) = &worker.success_condition {
                match condition {
                    JsonConditionDsl::Expression { expression } => {
                        config.insert(
                            "successConditionExpression".to_string(),
                            serde_json::Value::String(expression.clone()),
                        );
                    }
                    JsonConditionDsl::PathEquals { path, equals } => {
                        config.insert(
                            "successConditionPath".to_string(),
                            serde_json::Value::String(path.clone()),
                        );
                        config.insert("successConditionEquals".to_string(), equals.clone());
                    }
                }
            }
            config.insert(
                "manualCheck".to_string(),
                serde_json::Value::Bool(worker.manual_check.unwrap_or(false)),
            );
            config.insert(
                "sessionMode".to_string(),
                serde_json::Value::String("new".to_string()),
            );
        }
        NodeDsl::Exec(exec) => {
            config.insert(
                "planFrom".to_string(),
                serde_json::Value::String(exec.plan_from.clone()),
            );
        }
    }
    config
}
