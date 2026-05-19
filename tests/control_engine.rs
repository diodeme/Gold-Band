use gold_band::control::{ControlDecision, decide_next_step};
use gold_band::domain::{NodeOutcome, NodeType, RunStatus, SessionMode, VERSION};
use gold_band::dsl::WorkflowDsl;
use gold_band::runtime::{NodeState, RoundState, RunState};

fn parse_workflow(json: &str) -> WorkflowDsl {
    serde_json::from_str(json).unwrap()
}

fn sample_run() -> RunState {
    RunState {
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
        new_rounds_opened: 0,
        pause_reason: None,
    }
}

fn sample_round() -> RoundState {
    RoundState {
        version: VERSION.to_string(),
        id: "round-001".to_string(),
        run_id: "run-001".to_string(),
        index: 1,
        status: RunStatus::Running,
        outcome: None,
        trigger: gold_band::domain::RoundTrigger::Initial,
        repair_loops_used: 0,
        started_at: "0Z".to_string(),
        trace: Vec::new(),
    }
}

fn sample_node(node_id: &str, node_type: NodeType, outcome: NodeOutcome) -> NodeState {
    NodeState {
        version: VERSION.to_string(),
        node_id: node_id.to_string(),
        node_type,
        run_id: "run-001".to_string(),
        round_id: "round-001".to_string(),
        attempt_id: "attempt-001".to_string(),
        status: RunStatus::Completed,
        outcome: Some(outcome),
        started_at: "0Z".to_string(),
        finished_at: Some("1Z".to_string()),
        manual_check_pending: false,
        resolved_config: Default::default(),
    }
}

#[test]
fn worker_success_to_end_completes_run() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-accept",
            "entry": "accept",
            "control": { "max_repair_loops": 1 },
            "nodes": [
                { "id": "accept", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "accept", "to": "$end", "on": "success" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &sample_round(),
        &sample_node("accept", NodeType::Worker, NodeOutcome::Success),
    );
    assert!(matches!(
        decision,
        ControlDecision::CompleteRun(gold_band::domain::RunOutcome::Success)
    ));
}

#[test]
fn exec_invalid_prefers_explicit_edge() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "exec-invalid-edge",
            "entry": "dev",
            "control": {
                "max_repair_loops": 2,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" },
                { "id": "run-tests", "type": "exec", "plan_from": "dev" },
                { "id": "fix", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" },
                { "id": "accept", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "fix", "on": "invalid", "session": "continue" },
                { "from": "fix", "to": "accept", "on": "success" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &sample_round(),
        &sample_node("run-tests", NodeType::Exec, NodeOutcome::Invalid),
    );
    assert!(
        matches!(decision, ControlDecision::TransitionToNode { node_id, session: SessionMode::Continue } if node_id == "fix")
    );
}

#[test]
fn exec_invalid_defaults_back_to_plan_from() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "exec-invalid-default",
            "entry": "dev",
            "control": {
                "max_repair_loops": 2,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" },
                { "id": "run-tests", "type": "exec", "plan_from": "dev" },
                { "id": "accept", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "accept", "on": "success" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &sample_round(),
        &sample_node("run-tests", NodeType::Exec, NodeOutcome::Invalid),
    );
    assert!(
        matches!(decision, ControlDecision::TransitionToNode { node_id, session: SessionMode::Continue } if node_id == "dev")
    );
}

#[test]
fn exec_invalid_downgrades_continue_when_provider_cannot_continue() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "exec-invalid-new",
            "entry": "dev",
            "control": {
                "max_repair_loops": 2,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "other-provider", "primary_artifact": "exec-plan" },
                { "id": "run-tests", "type": "exec", "plan_from": "dev" },
                { "id": "accept", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "accept", "on": "success" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &sample_round(),
        &sample_node("run-tests", NodeType::Exec, NodeOutcome::Invalid),
    );
    assert!(
        matches!(decision, ControlDecision::TransitionToNode { node_id, session: SessionMode::New } if node_id == "dev")
    );
}

#[test]
fn exec_invalid_completes_failure_when_repair_budget_is_exhausted() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "exec-invalid-budget",
            "entry": "dev",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" },
                { "id": "run-tests", "type": "exec", "plan_from": "dev" },
                { "id": "accept", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "accept", "on": "success" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let mut round = sample_round();
    round.repair_loops_used = 1;
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &round,
        &sample_node("run-tests", NodeType::Exec, NodeOutcome::Invalid),
    );
    assert!(matches!(
        decision,
        ControlDecision::CompleteRun(gold_band::domain::RunOutcome::Failure)
    ));
}

#[test]
fn worker_manual_check_rejects_output_validation() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "manual-check-exclusive",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "review", "type": "worker", "provider": "claude-code", "manual_check": true, "primary_artifact": "review-result", "output": { "kind": "json", "artifact": "review-result" }, "success_condition": { "path": "passed", "equals": true } }
            ],
            "edges": []
        }"#,
    );

    let err = gold_band::dsl::validate_workflow(workflow).unwrap_err();
    assert!(err.to_string().contains("cannot enable manual_check together with output validation"));
}

#[test]
fn worker_failure_uses_explicit_edge() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-failure-edge",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "review", "type": "worker", "provider": "claude-code", "primary_artifact": "review-result", "output": { "kind": "json", "artifact": "review-result" }, "success_condition": { "path": "passed", "equals": true } },
                { "id": "dev", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "review", "to": "dev", "on": "failure", "session": "continue" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &sample_round(),
        &sample_node("review", NodeType::Worker, NodeOutcome::Failure),
    );
    assert!(
        matches!(decision, ControlDecision::TransitionToNode { node_id, session: SessionMode::Continue } if node_id == "dev")
    );
}

#[test]
fn edge_to_new_round_opens_round() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "new-round-edge",
            "entry": "accept",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "accept", "type": "worker", "provider": "claude-code", "primary_artifact": "accept-result", "output": { "kind": "json", "artifact": "accept-result" }, "success_condition": { "path": "passed", "equals": true } }
            ],
            "edges": [
                { "from": "accept", "to": "$new-round", "on": "failure" }
            ]
        }"#,
    );

    let validated = gold_band::dsl::validate_workflow(workflow).unwrap();
    let decision = decide_next_step(
        &validated,
        &sample_run(),
        &sample_round(),
        &sample_node("accept", NodeType::Worker, NodeOutcome::Failure),
    );
    assert!(matches!(decision, ControlDecision::OpenNewRound));
}
