//! Integration tests for entity UUID fields and serde compatibility.

use gold_band::domain::{NodeType, RoundTrigger, RunOutcome, RunStatus};
use gold_band::runtime::{LastExecutedNode, NodeState, RoundState, RunState, TaskState};

// ── TaskState ────────────────────────────────────────────────────────

#[test]
fn task_state_uuid_roundtrip() {
    let task = TaskState {
        version: "1".into(),
        id: "task-001".into(),
        title: Some("Test Task".into()),
        description: Some("A task for testing".into()),
        uuid: Some("abc123def456".into()),
    };
    let json = serde_json::to_string(&task).unwrap();
    let parsed: TaskState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.uuid, Some("abc123def456".into()));
    assert_eq!(parsed.id, "task-001");
}

#[test]
fn task_state_missing_uuid_deserializes_as_none() {
    let json = r#"{"version":"1","id":"task-001","title":"T"}"#;
    let task: TaskState = serde_json::from_str(json).unwrap();
    assert_eq!(task.uuid, None);
}

// ── RunState ─────────────────────────────────────────────────────────

#[test]
fn run_state_with_uuid_fields() {
    let run = RunState {
        version: "1".into(),
        id: "run-001".into(),
        task_id: "task-001".into(),
        task_uuid: Some("task-uuid-123".into()),
        uuid: Some("run-uuid-456".into()),
        status: RunStatus::Running,
        outcome: None,
        started_at: "1Z".into(),
        updated_at: "2Z".into(),
        workflow_snapshot: "w.snapshot.json".into(),
        current_round: None,
        current_node: None,
        current_attempt: None,
        new_rounds_opened: 0,
        pause_reason: None,
        last_executed_node: None,
    };
    let json = serde_json::to_string(&run).unwrap();
    let parsed: RunState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.uuid, Some("run-uuid-456".into()));
    assert_eq!(parsed.task_uuid, Some("task-uuid-123".into()));
    assert!(parsed.last_executed_node.is_none());
}

#[test]
fn run_state_with_last_executed_node_serde() {
    let run = RunState {
        version: "1".into(),
        id: "run-001".into(),
        task_id: "task-001".into(),
        task_uuid: None,
        uuid: None,
        status: RunStatus::Completed,
        outcome: Some(RunOutcome::Success),
        started_at: "1Z".into(),
        updated_at: "2Z".into(),
        workflow_snapshot: "w.json".into(),
        current_round: None,
        current_node: None,
        current_attempt: None,
        new_rounds_opened: 0,
        pause_reason: None,
        last_executed_node: Some(LastExecutedNode {
            node_id: "n1".into(),
            uuid: "u1".into(),
            round_uuid: "r1".into(),
            node_name: "测试2".into(),
            seq: Some(1),
            agent_type: Some("claude-acp".into()),
            status: "SUCCESS".into(),
            started_at: "100Z".into(),
            finished_at: Some("200Z".into()),
            input_tokens: 39781,
            output_tokens: 968,
            cache_read_tokens: 119552,
            total_tokens: 160301,
        }),
    };
    let json = serde_json::to_string(&run).unwrap();
    let parsed: RunState = serde_json::from_str(&json).unwrap();
    let pred = parsed.last_executed_node.unwrap();
    assert_eq!(pred.node_name, "测试2");
    assert_eq!(pred.input_tokens, 39781);
    assert_eq!(pred.total_tokens, 160301);
}

#[test]
fn run_state_backward_compat_no_new_fields() {
    // Simulates a run.json from before the metrics update (no uuid, taskUuid, lastExecutedNode)
    let json = r#"{"version":"1","id":"run-001","task_id":"task-001","status":"running","started_at":"1Z","updated_at":"2Z","workflow_snapshot":"w.json","new_rounds_opened":0}"#;
    let run: RunState = serde_json::from_str(json).unwrap();
    assert_eq!(run.uuid, None);
    assert_eq!(run.task_uuid, None);
    assert!(run.last_executed_node.is_none());
    assert_eq!(run.task_id, "task-001");
    assert_eq!(run.id, "run-001");
}

// ── RoundState ───────────────────────────────────────────────────────

#[test]
fn round_state_with_uuid() {
    let round = RoundState {
        version: "1".into(),
        id: "round-001".into(),
        run_id: "run-001".into(),
        uuid: Some("round-uuid-001".into()),
        index: 1,
        status: RunStatus::Running,
        outcome: None,
        trigger: RoundTrigger::Initial,
        started_at: "1Z".into(),
        trace: vec![],
    };
    let json = serde_json::to_string(&round).unwrap();
    let parsed: RoundState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.uuid, Some("round-uuid-001".into()));
}

#[test]
fn round_state_backward_compat_no_uuid() {
    let json = r#"{"version":"1","id":"round-001","run_id":"run-001","index":1,"status":"running","trigger":"initial","started_at":"1Z"}"#;
    let round: RoundState = serde_json::from_str(json).unwrap();
    assert_eq!(round.uuid, None);
}

// ── NodeState ────────────────────────────────────────────────────────

#[test]
fn node_state_with_uuid() {
    let node = NodeState {
        version: "1".into(),
        node_id: "node-1".into(),
        node_type: NodeType::Worker,
        run_id: "run-001".into(),
        round_id: "round-001".into(),
        attempt_id: "attempt-001".into(),
        uuid: Some("node-uuid-999".into()),
        status: RunStatus::Running,
        outcome: None,
        started_at: "1Z".into(),
        finished_at: None,
        manual_check_pending: false,
        resolved_config: Default::default(),
        pause_reason: None,
    };
    let json = serde_json::to_string(&node).unwrap();
    let parsed: NodeState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.uuid, Some("node-uuid-999".into()));
}

// ── LastExecutedNode ─────────────────────────────────────────────────

#[test]
fn last_executed_node_serde_roundtrip() {
    let pred = LastExecutedNode {
        node_id: "n1".into(),
        uuid: "u1".into(),
        round_uuid: "r1".into(),
        node_name: "测试".into(),
        seq: Some(1),
        agent_type: Some("claude-acp".into()),
        status: "SUCCESS".into(),
        started_at: "100Z".into(),
        finished_at: Some("200Z".into()),
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 200,
        total_tokens: 1700,
    };
    let json = serde_json::to_string(&pred).unwrap();
    let parsed: LastExecutedNode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.node_name, "测试");
    assert_eq!(parsed.input_tokens, 1000);
    assert_eq!(parsed.output_tokens, 500);
    assert_eq!(parsed.cache_read_tokens, 200);
    assert_eq!(parsed.total_tokens, 1700);
}

#[test]
fn last_executed_node_default_all_zeros() {
    let pred = LastExecutedNode::default();
    assert_eq!(pred.input_tokens, 0);
    assert_eq!(pred.output_tokens, 0);
    assert_eq!(pred.cache_read_tokens, 0);
    assert_eq!(pred.total_tokens, 0);
    assert!(pred.node_id.is_empty());
}
