use crate::domain::{PauseReason, RunOutcome, SessionMode};
use crate::dsl::{END_NODE, EdgeOutcome, NEW_ROUND_NODE, NodeDsl, ValidatedWorkflow};
use crate::provider::supports_continue_session;
use crate::runtime::{NodeState, RoundState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlDecision {
    TransitionToNode {
        node_id: String,
        session: SessionMode,
    },
    OpenNewRound,
    CompleteRun(RunOutcome),
    PauseRun(PauseReason),
}

pub fn decide_next_step(
    workflow: &ValidatedWorkflow,
    _run: &crate::runtime::RunState,
    round: &RoundState,
    node: &NodeState,
) -> ControlDecision {
    match node.outcome {
        Some(crate::domain::NodeOutcome::Success) => {
            match_edge_or_default(workflow, &node.node_id, EdgeOutcome::Success, || {
                ControlDecision::PauseRun(PauseReason::ErrorBlocked)
            })
        }
        Some(crate::domain::NodeOutcome::Failure) => match node.node_type {
            crate::domain::NodeType::Exec => decide_exec_failure(workflow, round, &node.node_id),
            crate::domain::NodeType::Worker => {
                match_edge_or_default(workflow, &node.node_id, EdgeOutcome::Failure, || {
                    ControlDecision::PauseRun(PauseReason::ErrorBlocked)
                })
            }
        },
        Some(crate::domain::NodeOutcome::Invalid) => match node.node_type {
            crate::domain::NodeType::Exec => decide_exec_invalid(workflow, round, &node.node_id),
            crate::domain::NodeType::Worker => {
                match_edge_or_default(workflow, &node.node_id, EdgeOutcome::Invalid, || {
                    ControlDecision::PauseRun(PauseReason::ErrorBlocked)
                })
            }
        },
        Some(crate::domain::NodeOutcome::Killed) => {
            ControlDecision::CompleteRun(RunOutcome::Killed)
        }
        None => ControlDecision::PauseRun(PauseReason::ProcessInterrupted),
    }
}

fn decide_exec_failure(
    workflow: &ValidatedWorkflow,
    round: &RoundState,
    node_id: &str,
) -> ControlDecision {
    match_edge_or_default(workflow, node_id, EdgeOutcome::Failure, || {
        if round.repair_loops_used >= workflow.raw.control.max_repair_loops {
            ControlDecision::CompleteRun(RunOutcome::Failure)
        } else {
            ControlDecision::PauseRun(PauseReason::ErrorBlocked)
        }
    })
}

fn decide_exec_invalid(
    workflow: &ValidatedWorkflow,
    round: &RoundState,
    node_id: &str,
) -> ControlDecision {
    if let Some(decision) = find_edge_decision(workflow, node_id, EdgeOutcome::Invalid) {
        return decision;
    }
    if round.repair_loops_used >= workflow.raw.control.max_repair_loops {
        return ControlDecision::CompleteRun(RunOutcome::Failure);
    }
    let Some(NodeDsl::Exec(exec)) = workflow.get_node(node_id) else {
        return ControlDecision::PauseRun(PauseReason::ErrorBlocked);
    };
    let session = session_for_target(workflow, &exec.plan_from, Some(SessionMode::Continue));
    ControlDecision::TransitionToNode {
        node_id: exec.plan_from.clone(),
        session,
    }
}

fn match_edge_or_default<F>(
    workflow: &ValidatedWorkflow,
    node_id: &str,
    outcome: EdgeOutcome,
    default: F,
) -> ControlDecision
where
    F: FnOnce() -> ControlDecision,
{
    find_edge_decision(workflow, node_id, outcome).unwrap_or_else(default)
}

fn find_edge_decision(
    workflow: &ValidatedWorkflow,
    node_id: &str,
    outcome: EdgeOutcome,
) -> Option<ControlDecision> {
    workflow
        .raw
        .edges
        .iter()
        .find(|edge| edge.from == node_id && edge.on == outcome)
        .map(|edge| {
            if edge.to == END_NODE {
                ControlDecision::CompleteRun(match outcome {
                    EdgeOutcome::Success => RunOutcome::Success,
                    EdgeOutcome::Failure | EdgeOutcome::Invalid => RunOutcome::Failure,
                })
            } else if edge.to == NEW_ROUND_NODE {
                ControlDecision::OpenNewRound
            } else {
                ControlDecision::TransitionToNode {
                    node_id: edge.to.clone(),
                    session: session_for_target(workflow, &edge.to, edge.session),
                }
            }
        })
}

fn session_for_target(
    workflow: &ValidatedWorkflow,
    target_node_id: &str,
    requested: Option<SessionMode>,
) -> SessionMode {
    match requested.unwrap_or(SessionMode::New) {
        SessionMode::New => SessionMode::New,
        SessionMode::Continue => workflow
            .get_node(target_node_id)
            .and_then(|node| node.provider())
            .map(|provider| supports_continue_session(provider).unwrap_or(false))
            .unwrap_or(false)
            .then_some(SessionMode::Continue)
            .unwrap_or(SessionMode::New),
    }
}
