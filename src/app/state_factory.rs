use crate::config::ResolvedProfileRef;
use crate::domain::{ResolvedConfig, RunStatus, VERSION};
use crate::dsl::{JsonConditionDsl, NodeDsl};
use crate::runtime::NodeState;

use super::ids::{generate_uuid, now_rfc3339_like};

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
        uuid: Some(generate_uuid()),
        pause_reason: None,
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
            if let Some(permission_mode) = &worker.permission_mode {
                config.insert(
                    "permissionMode".to_string(),
                    serde_json::Value::String(permission_mode.clone()),
                );
            }
            if let Some(profile) = resolved_profile.as_ref() {
                config.insert(
                    "profileName".to_string(),
                    serde_json::Value::String(profile.display_name.clone()),
                );
                config.insert(
                    "profileSource".to_string(),
                    serde_json::to_value(&profile.source).expect("serialize profile source"),
                );
                config.insert(
                    "profilePath".to_string(),
                    serde_json::Value::String(profile.path.clone()),
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
        NodeDsl::AiDynamic(dynamic) => {
            config.insert(
                "agentStrategy".to_string(),
                serde_json::to_value(&dynamic.agent_strategy)
                    .expect("serialize ai-dynamic agent strategy"),
            );
            if let Some(permission_mode) = &dynamic.permission_mode {
                config.insert(
                    "permissionMode".to_string(),
                    serde_json::Value::String(permission_mode.clone()),
                );
            }
            if let Some(profile) = resolved_profile.as_ref() {
                config.insert(
                    "profileName".to_string(),
                    serde_json::Value::String(profile.display_name.clone()),
                );
                config.insert(
                    "profileSource".to_string(),
                    serde_json::to_value(&profile.source).expect("serialize profile source"),
                );
                config.insert(
                    "profilePath".to_string(),
                    serde_json::Value::String(profile.path.clone()),
                );
            }
            config.insert(
                "dynamicControl".to_string(),
                serde_json::to_value(&dynamic.control).expect("serialize dynamic control"),
            );
            config.insert(
                "allowedWorkflows".to_string(),
                serde_json::to_value(&dynamic.allowed_workflows)
                    .expect("serialize allowed workflows"),
            );
            config.insert(
                "allowedProfiles".to_string(),
                serde_json::to_value(&dynamic.allowed_profiles)
                    .expect("serialize allowed profiles"),
            );
            if let Some(global_goal) = &dynamic.global_goal {
                config.insert(
                    "globalGoal".to_string(),
                    serde_json::Value::String(global_goal.clone()),
                );
            }
            config.insert("manualCheck".to_string(), serde_json::Value::Bool(false));
            config.insert(
                "sessionMode".to_string(),
                serde_json::Value::String("new".to_string()),
            );
        }
    }
    config
}
