use gold_band::dsl::{WorkflowDsl, validate_workflow};

fn parse_workflow(json: &str) -> WorkflowDsl {
    serde_json::from_str(json).expect("workflow should deserialize")
}

#[test]
fn validates_basic_workflow() {
    let workflow: WorkflowDsl = serde_json::from_str(
        r#"{
            "version": "0.1",
            "id": "dev-test-accept",
            "entry": "dev",
            "control": { "max_repair_loops": 3 },
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
                    "type": "worker",
                    "provider": "claude-code",
                    "profile": "acceptance",
                    "primary_artifact": "accept-result",
                    "output": { "kind": "json", "artifact": "accept-result" },
                    "success_condition": { "path": "result", "equals": true }
                }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "accept", "on": "success" },
                { "from": "run-tests", "to": "dev", "on": "failure", "session": "continue" },
                { "from": "accept", "to": "$new-round", "on": "failure" }
            ]
        }"#,
    )
    .expect("workflow should deserialize");

    let validated = validate_workflow(workflow).expect("workflow should validate");
    assert_eq!(validated.raw.entry, "dev");
    assert!(validated.get_node("accept").is_some());
}

#[test]
fn rejects_exec_plan_from_non_worker() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "invalid",
            "entry": "run-tests",
            "control": { "max_repair_loops": 1 },
            "nodes": [
                { "id": "run-tests", "type": "exec", "plan_from": "accept" },
                { "id": "accept", "type": "worker", "provider": "claude-code" }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_reserved_node_ids() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "reserved-id",
            "entry": "worker",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "worker", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_zero_loop_limits() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "zero-loops",
            "entry": "dev",
            "control": {
                "max_repair_loops": 0,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_legacy_verify_node_type() {
    let workflow = serde_json::from_str::<WorkflowDsl>(
        r#"{
            "version": "0.1",
            "id": "legacy-verify",
            "entry": "accept",
            "control": { "max_repair_loops": 1 },
            "nodes": [
                { "id": "accept", "type": "verify", "provider": "claude-code" }
            ],
            "edges": []
        }"#,
    );

    assert!(workflow.is_err());
}

#[test]
fn rejects_invalid_edges_to_end() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "invalid-end",
            "entry": "dev",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" },
                { "id": "run-tests", "type": "exec", "plan_from": "dev" }
            ],
            "edges": [
                { "from": "dev", "to": "run-tests", "on": "success" },
                { "from": "run-tests", "to": "$end", "on": "invalid" }
            ]
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_continue_edges_to_unsupported_provider() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "unsupported-provider",
            "entry": "dev",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "dev", "type": "worker", "provider": "claude-code", "primary_artifact": "exec-plan" },
                { "id": "review", "type": "worker", "provider": "other-provider", "primary_artifact": "exec-plan" }
            ],
            "edges": [
                { "from": "dev", "to": "review", "on": "success", "session": "continue" }
            ]
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn accepts_worker_json_output_validation() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": { "kind": "json", "artifact": "review-result" },
                    "success_condition": { "path": "passed", "equals": true }
                },
                {
                    "id": "test",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "test-result",
                    "output": { "kind": "json", "artifact": "test-result" },
                    "success_condition": { "path": "passed", "equals": true }
                }
            ],
            "edges": [
                { "from": "review", "to": "test", "on": "success" },
                { "from": "test", "to": "$new-round", "on": "failure" }
            ]
        }"#,
    );

    let validated = validate_workflow(workflow).expect("workflow should validate");
    assert_eq!(validated.raw.nodes.len(), 2);
}

#[test]
fn accepts_simplified_output_schema_with_matching_expression() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": {
                        "kind": "json",
                        "artifact": "review-result",
                        "schema": { "reason": "String", "result": "boolean" }
                    },
                    "success_condition": { "expression": "$.result == true" }
                }
            ],
            "edges": []
        }"#,
    );

    validate_workflow(workflow).expect("workflow should validate");
}

#[test]
fn rejects_success_expression_missing_from_simplified_schema() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": {
                        "kind": "json",
                        "artifact": "review-result",
                        "schema": { "reason": "String" }
                    },
                    "success_condition": { "expression": "$.result == true" }
                }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn accepts_nested_simplified_schema_path() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": {
                        "kind": "json",
                        "artifact": "review-result",
                        "schema": { "xx": { "yy": [{ "zz": "boolean" }] } }
                    },
                    "success_condition": { "expression": "$.xx.yy[0].zz == true" }
                }
            ],
            "edges": []
        }"#,
    );

    validate_workflow(workflow).expect("workflow should validate");
}

#[test]
fn rejects_malformed_success_expression_path() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": {
                        "kind": "json",
                        "artifact": "review-result",
                        "schema": { "xx": { "yy": "boolean" } }
                    },
                    "success_condition": { "expression": "$.xx..yy == true" }
                }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_legacy_json_schema_output_constraint() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": {
                        "kind": "json",
                        "artifact": "review-result",
                        "schema": {
                            "type": "object",
                            "properties": { "result": { "type": "boolean" } },
                            "required": ["result"]
                        }
                    },
                    "success_condition": { "expression": "$.result == true" }
                }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_worker_output_mismatch() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "worker-validation",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                {
                    "id": "review",
                    "type": "worker",
                    "provider": "claude-code",
                    "primary_artifact": "review-result",
                    "output": { "kind": "json", "artifact": "other-result" },
                    "success_condition": { "path": "passed", "equals": true }
                }
            ],
            "edges": []
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}

#[test]
fn rejects_continue_to_new_round_target() {
    let workflow = parse_workflow(
        r#"{
            "version": "0.1",
            "id": "new-round-session",
            "entry": "review",
            "control": {
                "max_repair_loops": 1,
                "max_acceptance_loops": 1,
                "on_acceptance_failure": "stop"
            },
            "nodes": [
                { "id": "review", "type": "worker", "provider": "claude-code" }
            ],
            "edges": [
                { "from": "review", "to": "$new-round", "on": "failure", "session": "continue" }
            ]
        }"#,
    );

    assert!(validate_workflow(workflow).is_err());
}
