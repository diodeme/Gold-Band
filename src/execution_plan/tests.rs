use std::fs;
use std::sync::mpsc;
use std::thread;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::config::ConversationAutoConfig;
use crate::domain::{NodeType, RunOutcome, RunStatus, SessionMode};
use crate::dsl::{EdgeDsl, EdgeOutcome, NodeDsl, PromptEnvelopeMode, WorkerNode, WorkflowDsl};
use crate::runtime::{RunState, RuntimeExecutionPhase, RuntimeExecutionState};
use crate::storage::{GoldBandPaths, read_json, write_json};
use crate::workflow_model_binding::WorkflowModelBindings;

use super::error;
use super::guard::{CurrentPlanGuard, evaluate_current_workflow};
use super::lock::{self, dispatch_read_held};
use super::model::ExecutionPlanPayload;
use super::store::{self, load_current, load_workflow_projection, publish_revision};

fn paths() -> (tempfile::TempDir, GoldBandPaths) {
    let temp = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    (temp, GoldBandPaths::new(root))
}

fn worker(id: &str, provider: &str) -> NodeDsl {
    NodeDsl::Worker(WorkerNode {
        id: id.to_string(),
        execution_slot_id: None,
        provider: Some(provider.to_string()),
        model: None,
        profile: None,
        goal: Some(format!("{id} goal")),
        output: None,
        success_condition: None,
        permission_mode: None,
        auto_accept: false,
        config_options: Default::default(),
        manual_check: Some(false),
        prompt_envelope: PromptEnvelopeMode::RuntimeManaged,
    })
}

fn edge(from: &str, to: &str, session: Option<SessionMode>) -> EdgeDsl {
    EdgeDsl {
        from: from.to_string(),
        to: to.to_string(),
        on: EdgeOutcome::Success,
        session,
        new_round_entry: None,
    }
}

fn workflow(nodes: Vec<NodeDsl>, edges: Vec<EdgeDsl>) -> WorkflowDsl {
    WorkflowDsl {
        version: "0.1".to_string(),
        id: "wf".to_string(),
        entry: nodes
            .first()
            .map(|node| node.id().to_string())
            .unwrap_or_default(),
        control: Default::default(),
        nodes,
        edges,
    }
}

fn auto_config(agent: &str, fanout: u32) -> ConversationAutoConfig {
    ConversationAutoConfig {
        agent_strategy: Some("fixed".to_string()),
        agent_type: agent.to_string(),
        bootstrap_agent_type: None,
        bootstrap_model_id: None,
        bootstrap_config_options: Default::default(),
        bootstrap_model_bound_overrides: Default::default(),
        acceptance_model_id: None,
        acceptance_config_options: Default::default(),
        acceptance_model_bound_overrides: Default::default(),
        model_id: None,
        permission_mode: None,
        auto_accept: false,
        config_options: Default::default(),
        model_bound_overrides: Default::default(),
        available_agents: None,
        routing_prompt: Some("route".to_string()),
        allowed_workflows: None,
        allowed_profiles: None,
        global_goal: None,
        control: Some(crate::config::ConversationDynamicControl {
            max_dynamic_nodes: 20,
            max_fanout: fanout,
            max_depth: 6,
            max_parallel: 3,
            max_group_depth: 1,
            max_workflow_invocations: 10,
            allow_nested_dynamic: false,
        }),
        active_template_id: None,
        active_template_name: None,
    }
}

fn running_run(task_id: &str, run_id: &str, execution_revision: u64) -> RunState {
    let mut run = RunState {
        version: "0.1".to_string(),
        id: run_id.to_string(),
        task_id: task_id.to_string(),
        task_uuid: Some("task-uuid".to_string()),
        status: RunStatus::Running,
        outcome: None,
        started_at: "t".to_string(),
        updated_at: "t".to_string(),
        workflow_snapshot: "workflow.snapshot.json".to_string(),
        current_round: Some("round-001".to_string()),
        current_node: Some("a".to_string()),
        current_attempt: Some("attempt-001".to_string()),
        new_rounds_opened: 0,
        pause_reason: None,
        uuid: Some("run-uuid".to_string()),
        last_executed_node: None,
        worktree: None,
        execution: RuntimeExecutionState::new(RuntimeExecutionPhase::StartingNode, None, "t"),
    };
    run.execution.revision = execution_revision;
    run
}

fn guard(status: RunStatus, outcome: Option<RunOutcome>, current: &str) -> CurrentPlanGuard {
    CurrentPlanGuard {
        run_status: status,
        run_outcome: outcome,
        current_node_id: Some(current.to_string()),
        durable_node_type: Some(NodeType::Worker),
        occurred_node_ids: [current.to_string()].into_iter().collect(),
    }
}

#[test]
fn legacy_snapshot_migrates_once_to_revision_one() {
    let (_temp, paths) = paths();
    let task_id = "task-migrate";
    let run_id = "run-001";
    let original = workflow(
        vec![worker("a", "claude-acp")],
        vec![edge("a", "$end", None)],
    );
    write_json(&paths.workflow_snapshot_file(task_id, run_id), &original).unwrap();

    let first = load_current(&paths, task_id, run_id).unwrap();
    let second = load_current(&paths, task_id, run_id).unwrap();
    assert_eq!(first.plan_revision, 1);
    assert_eq!(second.plan_revision, 1);
    let revisions = fs::read_dir(
        paths
            .execution_plan_revisions_dir(task_id, run_id)
            .as_std_path(),
    )
    .unwrap()
    .filter_map(|entry| entry.ok())
    .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
    .count();
    assert_eq!(revisions, 1);
    assert!(paths.execution_plan_manifest_file(task_id, run_id).exists());
}

#[test]
fn updating_authoring_does_not_change_the_current_plan() {
    let (_temp, paths) = paths();
    let task_id = "task-authoring";
    let run_id = "run-001";
    let current = workflow(
        vec![worker("a", "claude-acp")],
        vec![edge("a", "$end", None)],
    );
    let authoring = workflow(
        vec![worker("a", "other-agent")],
        vec![edge("a", "$end", None)],
    );
    store::publish_initial_workflow(
        &paths,
        task_id,
        run_id,
        current,
        WorkflowModelBindings::default(),
    )
    .unwrap();
    write_json(&paths.workflow_file(task_id), &authoring).unwrap();

    let loaded = load_workflow_projection(&paths, task_id, run_id).unwrap();
    assert_eq!(loaded.nodes[0].provider(), Some("claude-acp"));
    let stored_authoring: WorkflowDsl = read_json(&paths.workflow_file(task_id)).unwrap();
    assert_eq!(stored_authoring.nodes[0].provider(), Some("other-agent"));
}

#[test]
fn current_publish_leaves_attempt_and_execution_revision_unchanged() {
    let (_temp, paths) = paths();
    let task_id = "task-attempt";
    let run_id = "run-001";
    let original = workflow(
        vec![worker("a", "claude-acp")],
        vec![edge("a", "$end", None)],
    );
    store::publish_initial_workflow(
        &paths,
        task_id,
        run_id,
        original,
        WorkflowModelBindings::default(),
    )
    .unwrap();
    write_json(
        &paths.run_file(task_id, run_id),
        &running_run(task_id, run_id, 7),
    )
    .unwrap();
    let node_path = paths.node_file(task_id, run_id, "round-001", "a", "attempt-001");
    fs::create_dir_all(node_path.parent().unwrap().as_std_path()).unwrap();
    let node_bytes = br#"{"resolvedConfig":{"provider":"claude-acp"}}"#;
    fs::write(node_path.as_std_path(), node_bytes).unwrap();

    let replacement = workflow(
        vec![worker("a", "claude-acp")],
        vec![edge("a", "$end", None)],
    );
    let mut replacement = replacement;
    if let NodeDsl::Worker(worker) = &mut replacement.nodes[0] {
        worker.goal = Some("changed goal".to_string());
    }
    let published = publish_revision(
        &paths,
        task_id,
        run_id,
        1,
        super::model::ExecutionPlanRunMode::Workflow,
        0,
        ExecutionPlanPayload::Workflow {
            workflow: replacement,
            model_bindings: WorkflowModelBindings::default(),
        },
    )
    .unwrap();
    assert_eq!(published.plan_revision, 2);
    let run: RunState = read_json(&paths.run_file(task_id, run_id)).unwrap();
    assert_eq!(run.execution.revision, 7);
    assert_eq!(fs::read(node_path.as_std_path()).unwrap(), node_bytes);
    let projected = load_workflow_projection(&paths, task_id, run_id).unwrap();
    assert_eq!(
        projected.nodes[0].profile().or(Some("changed")),
        Some("changed")
    );
    match &projected.nodes[0] {
        NodeDsl::Worker(worker) => assert_eq!(worker.goal.as_deref(), Some("changed goal")),
        _ => panic!("worker"),
    }
}

#[test]
fn auto_plan_freezes_dispatch_limits_without_rewriting_authoring() {
    let (_temp, paths) = paths();
    let task_id = "task-auto";
    let run_id = "run-001";
    let original = auto_config("claude-acp", 2);
    store::publish_initial_auto(&paths, task_id, run_id, original.clone()).unwrap();
    write_json(&paths.workflow_file(task_id), &original).unwrap();
    let mut next = original.clone();
    next.control.as_mut().unwrap().max_fanout = 9;
    next.routing_prompt = Some("new route".to_string());
    publish_revision(
        &paths,
        task_id,
        run_id,
        1,
        super::model::ExecutionPlanRunMode::Auto,
        0,
        ExecutionPlanPayload::Auto { config: next },
    )
    .unwrap();

    let node = store::ai_dynamic_node_for_dispatch(&paths, task_id, run_id, "ai-dynamic")
        .unwrap()
        .unwrap();
    assert_eq!(node.control.max_fanout, 9);
    let authoring: ConversationAutoConfig = read_json(&paths.workflow_file(task_id)).unwrap();
    assert_eq!(authoring.control.unwrap().max_fanout, 2);
    assert_eq!(authoring.routing_prompt.as_deref(), Some("route"));
}

#[test]
fn dispatch_read_blocks_plan_publish_and_provider_boundary_is_unlocked() {
    let (_temp, paths) = paths();
    let task_id = "task-lock";
    let run_id = "run-001";
    let original = workflow(
        vec![worker("a", "claude-acp")],
        vec![edge("a", "$end", None)],
    );
    store::publish_initial_workflow(
        &paths,
        task_id,
        run_id,
        original.clone(),
        WorkflowModelBindings::default(),
    )
    .unwrap();
    let key = store::lock_key(&paths, task_id, run_id);
    assert!(!dispatch_read_held(&key));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let key_for_thread = key.clone();
    let reader = thread::spawn(move || {
        let _guard = lock::acquire_dispatch_read(&key_for_thread).unwrap();
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    started_rx.recv().unwrap();
    assert!(dispatch_read_held(&key));
    let conflict = publish_revision(
        &paths,
        task_id,
        run_id,
        1,
        super::model::ExecutionPlanRunMode::Workflow,
        0,
        ExecutionPlanPayload::Workflow {
            workflow: original,
            model_bindings: WorkflowModelBindings::default(),
        },
    )
    .unwrap_err();
    assert_eq!(conflict.code(), error::REVISION_CONFLICT);
    release_tx.send(()).unwrap();
    reader.join().unwrap();
    assert!(!dispatch_read_held(&key));
    lock::with_dispatch_read(&key, || assert!(dispatch_read_held(&key))).unwrap();
    assert!(!dispatch_read_held(&key));
}

#[test]
fn current_guards_reject_closed_runs_removed_nodes_and_continue_identity_changes() {
    let previous = workflow(
        vec![worker("a", "claude-acp"), worker("b", "claude-acp")],
        vec![
            edge("a", "b", Some(SessionMode::Continue)),
            EdgeDsl {
                from: "b".to_string(),
                to: "$end".to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            },
        ],
    );
    let closed = evaluate_current_workflow(
        &previous,
        &previous,
        &guard(RunStatus::Completed, Some(RunOutcome::Killed), "a"),
    );
    assert_eq!(closed[0].code(), error::CURRENT_RUN_NOT_EDITABLE);

    let mut removed = previous.clone();
    removed.nodes.retain(|node| node.id() != "a");
    removed.entry = "b".to_string();
    removed.edges.retain(|edge| edge.from != "a");
    let removed_issues =
        evaluate_current_workflow(&previous, &removed, &guard(RunStatus::Running, None, "a"));
    assert_eq!(removed_issues[0].code(), error::CURRENT_NODE_REMOVED);

    let mut identity = previous.clone();
    if let NodeDsl::Worker(worker) = &mut identity.nodes[1] {
        worker.provider = Some("other-agent".to_string());
    }
    let mut occurred = guard(RunStatus::Running, None, "a");
    occurred.occurred_node_ids.insert("b".to_string());
    occurred.current_node_id = Some("a".to_string());
    let identity_issues = evaluate_current_workflow(&previous, &identity, &occurred);
    assert!(
        identity_issues
            .iter()
            .any(|issue| issue.code() == error::AGENT_IDENTITY_CHANGED)
    );

    let mut goal = previous.clone();
    if let NodeDsl::Worker(worker) = &mut goal.nodes[0] {
        worker.goal = Some("new goal".to_string());
    }
    goal.edges[0].to = "$end".to_string();
    goal.edges[0].session = None;
    goal.edges.truncate(1);
    let allowed = evaluate_current_workflow(&previous, &goal, &guard(RunStatus::Paused, None, "a"));
    assert!(
        allowed
            .iter()
            .all(|issue| issue.code() != error::CURRENT_NODE_REMOVED
                && issue.code() != error::AGENT_IDENTITY_CHANGED)
    );
}
