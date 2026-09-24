use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{NodeType, RunOutcome, RunStatus, SessionMode};
use crate::dsl::{NodeDsl, WorkflowDsl};
use crate::provider::supports_continue_session;

use super::error::{self, ExecutionPlanError};

#[derive(Debug, Clone)]
pub struct CurrentPlanGuard {
    pub run_status: RunStatus,
    pub run_outcome: Option<RunOutcome>,
    pub current_node_id: Option<String>,
    pub durable_node_type: Option<NodeType>,
    pub occurred_node_ids: BTreeSet<String>,
}

pub fn current_run_editable(status: RunStatus, outcome: Option<RunOutcome>) -> bool {
    matches!(status, RunStatus::Running | RunStatus::Paused) && outcome != Some(RunOutcome::Killed)
}

pub fn evaluate_current_workflow(
    previous: &WorkflowDsl,
    next: &WorkflowDsl,
    guard: &CurrentPlanGuard,
) -> Vec<ExecutionPlanError> {
    let mut issues = Vec::new();
    if !current_run_editable(guard.run_status, guard.run_outcome) {
        issues.push(ExecutionPlanError::new(
            error::CURRENT_RUN_NOT_EDITABLE,
            serde_json::json!({
                "runStatus": status_label(guard.run_status),
                "runOutcome": guard.run_outcome.map(outcome_label),
            }),
        ));
        return issues;
    }

    let previous_nodes = index_nodes(previous);
    let next_nodes = index_nodes(next);
    if let Some(current_node_id) = guard.current_node_id.as_deref() {
        let Some(next_node) = next_nodes.get(current_node_id) else {
            issues.push(ExecutionPlanError::new(
                error::CURRENT_NODE_REMOVED,
                serde_json::json!({ "nodeId": current_node_id }),
            ));
            return issues;
        };
        let next_type = next_node.node_type();
        if guard
            .durable_node_type
            .is_some_and(|durable| durable != next_type)
        {
            issues.push(ExecutionPlanError::new(
                error::CURRENT_NODE_IDENTITY_CHANGED,
                serde_json::json!({
                    "nodeId": current_node_id,
                    "field": "nodeType",
                }),
            ));
        }
        if previous_nodes
            .get(current_node_id)
            .is_some_and(|previous_node| previous_node.node_type() != next_type)
        {
            issues.push(ExecutionPlanError::new(
                error::CURRENT_NODE_IDENTITY_CHANGED,
                serde_json::json!({
                    "nodeId": current_node_id,
                    "field": "nodeType",
                }),
            ));
        }
    }

    let continue_targets = continue_targets(next);
    for node_id in &guard.occurred_node_ids {
        if !continue_targets.contains(node_id) {
            continue;
        }
        let (Some(previous_node), Some(next_node)) =
            (previous_nodes.get(node_id), next_nodes.get(node_id))
        else {
            continue;
        };
        let previous_identity = agent_identity(previous_node);
        let next_identity = agent_identity(next_node);
        if previous_identity != next_identity {
            issues.push(ExecutionPlanError::new(
                error::AGENT_IDENTITY_CHANGED,
                serde_json::json!({
                    "nodeId": node_id,
                    "previousAgentId": previous_identity,
                    "nextAgentId": next_identity,
                }),
            ));
        }
    }

    for edge in &next.edges {
        if edge.session != Some(SessionMode::Continue) {
            continue;
        }
        let Some(node) = next_nodes.get(&edge.to) else {
            continue;
        };
        let provider = node.provider().unwrap_or("");
        let supported = supports_continue_session(provider).unwrap_or(false);
        if provider.is_empty() || !supported {
            issues.push(ExecutionPlanError::new(
                error::CONTINUE_UNSUPPORTED,
                serde_json::json!({
                    "nodeId": edge.to,
                    "agentId": provider,
                }),
            ));
        }
    }
    issues
}

pub fn affected_node_ids(previous: &WorkflowDsl, next: &WorkflowDsl) -> Vec<String> {
    let mut ids = BTreeSet::new();
    let previous_nodes = index_nodes(previous);
    let next_nodes = index_nodes(next);
    for id in previous_nodes.keys().chain(next_nodes.keys()) {
        if previous_nodes.get(id).copied().map(node_signature)
            != next_nodes.get(id).copied().map(node_signature)
        {
            ids.insert(id.clone());
        }
    }
    let previous_edges = edge_signatures(previous);
    let next_edges = edge_signatures(next);
    for (from, _) in previous_edges.symmetric_difference(&next_edges) {
        ids.insert(from.clone());
    }
    ids.into_iter().collect()
}

pub fn agent_identity(node: &NodeDsl) -> String {
    match node {
        NodeDsl::Worker(worker) => worker.provider.clone().unwrap_or_default(),
        NodeDsl::AiDynamic(dynamic) => match &dynamic.agent_strategy {
            crate::dsl::AiDynamicAgentStrategy::Fixed { provider, .. } => provider.clone(),
            crate::dsl::AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider, ..
            } => bootstrap_provider.clone(),
        },
    }
}

fn continue_targets(workflow: &WorkflowDsl) -> BTreeSet<String> {
    workflow
        .edges
        .iter()
        .filter(|edge| edge.session == Some(SessionMode::Continue))
        .map(|edge| edge.to.clone())
        .collect()
}

fn index_nodes(workflow: &WorkflowDsl) -> BTreeMap<String, &NodeDsl> {
    workflow
        .nodes
        .iter()
        .map(|node| (node.id().to_string(), node))
        .collect()
}

fn node_signature(node: &NodeDsl) -> serde_json::Value {
    serde_json::to_value(node).unwrap_or(serde_json::Value::Null)
}

fn edge_signatures(workflow: &WorkflowDsl) -> BTreeSet<(String, String)> {
    workflow
        .edges
        .iter()
        .map(|edge| {
            (
                edge.from.clone(),
                serde_json::to_string(edge).unwrap_or_default(),
            )
        })
        .collect()
}

fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Paused => "paused",
        RunStatus::Completed => "completed",
    }
}

fn outcome_label(outcome: RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Success => "success",
        RunOutcome::Failure => "failure",
        RunOutcome::Killed => "killed",
    }
}
