use gold_band::control::{decide_next_step, ControlDecision};
use gold_band::domain::{NodeOutcome, NodeType, RunStatus, VERSION};
use gold_band::dsl::WorkflowDsl;
use gold_band::runtime::{NodeState, RoundState, RunState};

#[test]
fn verify_success_completes_run() {
    let workflow: WorkflowDsl = serde_json::from_str(
        r#"{
            "version": "0.1",
            "id": "verify-only",
            "entry": "accept",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "accept", "type": "verify" }
            ],
            "edges": []
        }"#,
    )
    .unwrap();

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let run = RunState {
        version: VERSION.to_string(),
        id: "run-001".to_string(),
        task_id: "task-001".to_string(),
        status: RunStatus::Running,
        outcome: None,
        started_at: "0Z".to_string(),
        updated_at: "0Z".to_string(),
        workflow_snapshot: "workflow.snapshot.json".to_string(),
        current_round: Some("round-001".to_string()),
        current_node: Some("accept".to_string()),
        current_attempt: Some("attempt-001".to_string()),
        acceptance_loops_used: 0,
        pause_reason: None,
    };
    let round = RoundState {
        version: VERSION.to_string(),
        id: "round-001".to_string(),
        run_id: "run-001".to_string(),
        index: 1,
        status: RunStatus::Running,
        outcome: None,
        trigger: gold_band::domain::RoundTrigger::Initial,
        repair_loops_used: 0,
        started_at: "0Z".to_string(),
    };
    let node = NodeState {
        version: VERSION.to_string(),
        node_id: "accept".to_string(),
        node_type: NodeType::Verify,
        run_id: "run-001".to_string(),
        round_id: "round-001".to_string(),
        attempt_id: "attempt-001".to_string(),
        status: RunStatus::Completed,
        outcome: Some(NodeOutcome::Success),
        started_at: "0Z".to_string(),
        finished_at: Some("1Z".to_string()),
        resolved_config: Default::default(),
    };

    let decision = decide_next_step(&validated, &run, &round, &node);
    assert!(matches!(decision, ControlDecision::CompleteRun(gold_band::domain::RunOutcome::Success)));
}
