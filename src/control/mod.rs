use crate::domain::{PauseReason, RunOutcome, SessionMode};
use crate::dsl::{EdgeOutcome, ValidatedWorkflow, END_NODE};
use crate::runtime::{NodeState, RoundState, RunState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlDecision {
    TransitionToNode { node_id: String, session: SessionMode },
    OpenNewRound,
    CompleteRun(RunOutcome),
    PauseRun(PauseReason),
}

pub fn decide_next_step(workflow: &ValidatedWorkflow, run: &RunState, round: &RoundState, node: &NodeState) -> ControlDecision {
    match node.outcome {
        Some(crate::domain::NodeOutcome::Success) => {
            if let Some(edge) = workflow.raw.edges.iter().find(|edge| edge.from == node.node_id && edge.on == EdgeOutcome::Success) {
                if edge.to == END_NODE {
                    ControlDecision::CompleteRun(RunOutcome::Success)
                } else {
                    ControlDecision::TransitionToNode {
                        node_id: edge.to.clone(),
                        session: edge.session.unwrap_or(SessionMode::New),
                    }
                }
            } else if matches!(node.node_type, crate::domain::NodeType::Verify) {
                ControlDecision::CompleteRun(RunOutcome::Success)
            } else {
                ControlDecision::PauseRun(PauseReason::ErrorBlocked)
            }
        }
        Some(crate::domain::NodeOutcome::Failure) => match node.node_type {
            crate::domain::NodeType::Exec => {
                if let Some(edge) = workflow.raw.edges.iter().find(|edge| edge.from == node.node_id && edge.on == EdgeOutcome::Failure) {
                    if edge.to == END_NODE {
                        ControlDecision::CompleteRun(RunOutcome::Failure)
                    } else {
                        ControlDecision::TransitionToNode {
                            node_id: edge.to.clone(),
                            session: edge.session.unwrap_or(SessionMode::New),
                        }
                    }
                } else if round.repair_loops_used >= workflow.raw.control.max_repair_loops {
                    ControlDecision::CompleteRun(RunOutcome::Failure)
                } else {
                    ControlDecision::PauseRun(PauseReason::ErrorBlocked)
                }
            }
            crate::domain::NodeType::Verify => match workflow.raw.control.on_acceptance_failure {
                crate::domain::AcceptanceFailurePolicy::AutoLoop => {
                    if run.acceptance_loops_used >= workflow.raw.control.max_acceptance_loops {
                        ControlDecision::CompleteRun(RunOutcome::Failure)
                    } else {
                        ControlDecision::OpenNewRound
                    }
                }
                crate::domain::AcceptanceFailurePolicy::Stop => ControlDecision::CompleteRun(RunOutcome::Failure),
            },
            crate::domain::NodeType::Worker => ControlDecision::PauseRun(PauseReason::ErrorBlocked),
        },
        Some(crate::domain::NodeOutcome::Invalid) => ControlDecision::PauseRun(PauseReason::ErrorBlocked),
        Some(crate::domain::NodeOutcome::Killed) => ControlDecision::CompleteRun(RunOutcome::Killed),
        None => ControlDecision::PauseRun(PauseReason::ProcessInterrupted),
    }
}
