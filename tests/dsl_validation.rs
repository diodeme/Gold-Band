use gold_band::dsl::{validate_workflow, WorkflowDsl};

#[test]
fn validates_basic_workflow() {
    let workflow: WorkflowDsl = serde_json::from_str(
        r#"{
            "version": "0.1",
            "id": "dev-test-verify",
            "entry": "dev",
            "control": {
                "max_repair_loops": 3,
                "max_acceptance_loops": 2,
                "on_acceptance_failure": "auto-loop"
            },
            "nodes": [
                {
                    "id": "dev",
                    "type": "worker",
                    "provider": "claude-code",
                    "profile": "developer",
                    "goal": "implement requirement",
                    "primary_artifact": "exec-plan"
                },
                {
                    "id": "run-tests",
                    "type": "exec",
                    "plan_from": "dev"
                },
                {
                    "id": "accept",
                    "type": "verify"
                }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "accept", "on": "success" },
                { "from": "run-tests", "to": "dev", "on": "failure", "session": "continue" }
            ]
        }"#,
    )
    .expect("workflow should deserialize");

    let validated = validate_workflow(workflow).expect("workflow should validate");
    assert_eq!(validated.raw.entry, "dev");
    assert_eq!(validated.verify_node_id.as_deref(), Some("accept"));
}

#[test]
fn rejects_exec_plan_from_non_worker() {
    let workflow: WorkflowDsl = serde_json::from_str(
        r#"{
            "version": "0.1",
            "id": "invalid",
            "entry": "run-tests",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "run-tests", "type": "exec", "plan_from": "accept" },
                { "id": "accept", "type": "verify" }
            ],
            "edges": []
        }"#,
    )
    .expect("workflow should deserialize");

    assert!(validate_workflow(workflow).is_err());
}
