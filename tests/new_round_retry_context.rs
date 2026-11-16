mod support;

use camino::Utf8PathBuf;
use gold_band::app::{App, ConversationMetadata};
use gold_band::domain::{
    NodeOutcome, NodeType, PauseReason, RoundTrigger, RunOutcome, RunStatus, SessionMode, VERSION,
};
use gold_band::dsl::WorkflowDsl;
use gold_band::provider::{
    DoctorResult, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderRunResult,
    ProviderRunStatus, SessionRef, WorkerInvocation,
};
use gold_band::runtime::{
    CURRENT_ACP_STORAGE_SCHEMA_VERSION, NodeState, RoundState, RoundTraceStep, RunState,
    RuntimeExecutionPhase, RuntimeExecutionState, TaskState, write_node_state,
};
use gold_band::storage::write_json;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

const TASK: &str = "task-new-round-retry";
const SOURCE_RUN: &str = "run-001";
const INITIAL_ROUND: &str = "round-001";
const CURRENT_ROUND: &str = "round-002";
const NOW: &str = "2026-11-13T00:00:00Z";
const SETTLEMENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
struct RecordingProvider(Arc<Mutex<Vec<WorkerInvocation>>>);

impl ProviderAdapter for RecordingProvider {
    fn describe_provider(&self) -> ProviderInfo {
        ProviderInfo {
            provider_id: "fake".to_string(),
            display_name: "Fake".to_string(),
            capabilities: ProviderCapabilities {
                supports_open_session: true,
                supports_continue_session: true,
                supports_system_prompt: true,
                supports_raw_stream: false,
            },
            is_default: false,
        }
    }

    fn doctor(&self) -> DoctorResult {
        DoctorResult {
            available: true,
            error: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, invocation: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.0.lock().unwrap().push(invocation);
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: None,
            worker_ref_seed: None,
            stream_path: None,
            runtime_error: None,
            runtime_control_output: None,
        })
    }

    fn open_session(&self, _: &SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(&self, _: &SessionRef) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

fn trace(
    sequence: u32,
    node: &str,
    attempt: &str,
    from: Option<&str>,
    on: Option<&str>,
) -> RoundTraceStep {
    RoundTraceStep {
        sequence,
        node_id: node.to_string(),
        attempt_id: attempt.to_string(),
        from_node_id: from.map(str::to_string),
        edge_outcome: on.map(str::to_string),
        entered_at: NOW.to_string(),
    }
}

fn write_fixture(app: &App, completed: bool, current_attempt: &str) {
    let profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .next()
        .unwrap()
        .id;
    let workflow: WorkflowDsl = serde_json::from_value(serde_json::json!({
        "version": VERSION,
        "id": "new-round-retry",
        "entry": "plan",
        "nodes": [
            { "id": "plan", "type": "worker", "provider": "claude-acp", "profile": profile,
              "goal": "Plan", "output": { "kind": "json", "artifact": "plan.json" } },
            { "id": "dev", "type": "worker", "provider": "claude-acp", "profile": profile, "goal": "Develop" },
            { "id": "accept", "type": "worker", "provider": "claude-acp", "profile": profile, "goal": "Accept" }
        ],
        "edges": [
            { "from": "plan", "to": "dev", "on": "success" },
            { "from": "dev", "to": "accept", "on": "success" },
            { "from": "accept", "to": "$end", "on": "success" },
            { "from": "accept", "to": "$new-round", "on": "failure", "new_round_entry": "dev" }
        ]
    })).unwrap();
    write_json(&app.paths.workflow_file(TASK), &workflow).unwrap();
    app.task_authoring_workflow(TASK).unwrap();
    gold_band::execution_plan::publish_initial_workflow(
        &app.paths,
        TASK,
        SOURCE_RUN,
        workflow,
        Default::default(),
    )
    .unwrap();
    write_json(
        &app.paths.task_file(TASK),
        &TaskState {
            version: VERSION.to_string(),
            id: TASK.to_string(),
            title: None,
            description: None,
            uuid: None,
        },
    )
    .unwrap();
    write_json(
        &app.paths.task_dir(TASK).join("authoring/conversation.json"),
        &ConversationMetadata {
            resident: false,
            version: VERSION.to_string(),
            source: "desktop".to_string(),
            run_mode: "workflow".to_string(),
            workflow_template_id: None,
            include_optional_entry: None,
            direct_config: None,
            agent_identity: None,
            title_auto_generated: true,
            initial_attachment_names: None,
            initial_workspace_files: Vec::new(),
            created_at: NOW.to_string(),
            last_activity_at: None,
            work_location: Default::default(),
            scheduled_task_id: None,
            scheduled_content_fingerprint: None,
        },
    )
    .unwrap();
    std::fs::write(
        app.paths.requirement_file(TASK),
        "Keep the stable plan and acceptance feedback",
    )
    .unwrap();
    let run = RunState {
        version: VERSION.to_string(),
        id: SOURCE_RUN.to_string(),
        task_id: TASK.to_string(),
        task_uuid: None,
        status: if completed {
            RunStatus::Completed
        } else {
            RunStatus::Paused
        },
        outcome: completed.then_some(RunOutcome::Failure),
        started_at: NOW.to_string(),
        updated_at: NOW.to_string(),
        workflow_snapshot: "workflow.snapshot.json".to_string(),
        current_round: Some(CURRENT_ROUND.to_string()),
        current_node: Some("dev".to_string()),
        current_attempt: Some(current_attempt.to_string()),
        new_rounds_opened: 1,
        pause_reason: (!completed).then_some(PauseReason::RuntimeAbnormal),
        uuid: None,
        rerun: None,
        last_executed_node: None,
        worktree: None,
        execution: RuntimeExecutionState {
            phase: if completed {
                RuntimeExecutionPhase::Terminal
            } else {
                RuntimeExecutionPhase::Paused
            },
            ..Default::default()
        },
    };
    let initial = RoundState {
        version: VERSION.to_string(),
        id: INITIAL_ROUND.to_string(),
        run_id: SOURCE_RUN.to_string(),
        index: 1,
        status: RunStatus::Completed,
        outcome: Some(RunOutcome::Failure),
        trigger: RoundTrigger::Initial,
        started_at: NOW.to_string(),
        trace: vec![
            trace(1, "plan", "attempt-001", None, None),
            trace(2, "dev", "attempt-001", Some("plan"), Some("success")),
            trace(3, "accept", "attempt-001", Some("dev"), Some("success")),
        ],
        uuid: None,
    };
    let mut current = RoundState {
        id: CURRENT_ROUND.to_string(),
        index: 2,
        status: run.status,
        outcome: run.outcome,
        trigger: RoundTrigger::NewRound,
        trace: vec![trace(1, "dev", "attempt-001", None, None)],
        ..initial.clone()
    };
    if current_attempt == "attempt-002" {
        current
            .trace
            .push(trace(2, "dev", "attempt-002", Some("dev"), Some("retry")));
    }
    for round in [initial, current] {
        write_json(&app.paths.round_file(TASK, SOURCE_RUN, &round.id), &round).unwrap();
        for step in &round.trace {
            let is_current = round.id == CURRENT_ROUND && step.attempt_id == current_attempt;
            let node = NodeState {
                version: VERSION.to_string(),
                acp_storage_schema_version: CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: step.node_id.clone(),
                node_type: NodeType::Worker,
                run_id: SOURCE_RUN.to_string(),
                round_id: round.id.clone(),
                attempt_id: step.attempt_id.clone(),
                status: if is_current {
                    run.status
                } else {
                    RunStatus::Completed
                },
                outcome: if is_current && !completed {
                    None
                } else {
                    Some(if step.node_id == "accept" || round.id == CURRENT_ROUND {
                        NodeOutcome::Failure
                    } else {
                        NodeOutcome::Success
                    })
                },
                started_at: NOW.to_string(),
                finished_at: Some(NOW.to_string()),
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: if step.node_id == "plan" {
                    [("outputArtifact".to_string(), serde_json::json!("plan.json"))].into()
                } else {
                    Default::default()
                },
                uuid: None,
            };
            write_node_state(
                &app.paths
                    .node_file(TASK, SOURCE_RUN, &round.id, &step.node_id, &step.attempt_id),
                &node,
            )
            .unwrap();
        }
    }
    write_json(&app.paths.run_file(TASK, SOURCE_RUN), &run).unwrap();
    write_json(
        &app.paths.artifact_file(
            TASK,
            SOURCE_RUN,
            INITIAL_ROUND,
            "plan",
            "attempt-001",
            "plan.json",
        ),
        &serde_json::json!({ "scope": "stable plan" }),
    )
    .unwrap();
    let attachments =
        app.paths
            .attachments_dir(TASK, SOURCE_RUN, INITIAL_ROUND, "accept", "attempt-001");
    std::fs::create_dir_all(&attachments).unwrap();
    std::fs::write(
        attachments.join("review.txt"),
        "Acceptance failed; revise the implementation",
    )
    .unwrap();
}

fn assert_reexecution_context(rerun: bool) {
    for current_attempt in ["attempt-001", "attempt-002"] {
        let temp = tempfile::tempdir().unwrap();
        let provider = RecordingProvider::default();
        let (settled_tx, settled) = mpsc::channel();
        let app = support::app_with_available_claude_provider(
            Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap(),
            Box::new(provider.clone()),
        )
        .with_inline_lifecycle_subscriber(Arc::new(move |event| {
            if matches!(
                event,
                gold_band::app::RuntimeLifecycleEvent::RunCompleted { .. }
                    | gold_band::app::RuntimeLifecycleEvent::RunPaused { .. }
            ) {
                let _ = settled_tx.send(());
            }
        }));
        write_fixture(&app, rerun, current_attempt);
        let accepted = if rerun {
            app.rerun_failed_node(TASK, SOURCE_RUN, CURRENT_ROUND, "dev", current_attempt)
        } else {
            app.retry_current_node(TASK, SOURCE_RUN, CURRENT_ROUND, "dev", current_attempt)
        }
        .unwrap();
        settled.recv_timeout(SETTLEMENT_TIMEOUT).unwrap();
        let invocations = provider.0.lock().unwrap();
        assert_eq!(invocations.len(), 2);
        let entry = &invocations[0];
        assert_eq!(entry.runtime_context.run_id, accepted.run.id);
        assert_eq!(entry.session_mode, SessionMode::New);
        assert!(entry.continue_ref.is_none());
        let predecessors = entry
            .predecessors
            .iter()
            .map(|context| {
                (
                    context.round_id.as_str(),
                    context.node_id.as_str(),
                    context.attempt_id.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let mut expected = vec![
            (INITIAL_ROUND, "plan", "attempt-001"),
            (CURRENT_ROUND, "dev", "attempt-001"),
        ];
        if current_attempt == "attempt-002" {
            expected.push((CURRENT_ROUND, "dev", "attempt-002"));
        }
        assert_eq!(predecessors, expected);
        let plan = entry.predecessors[0].output_artifact.as_ref().unwrap();
        assert_eq!(plan.name, "plan.json");
        assert!(plan.preview.as_ref().unwrap().contains("stable plan"));
        assert!(
            plan.path
                .starts_with(app.paths.run_dir(TASK, &accepted.run.id))
        );
        let trigger = entry
            .new_round_trigger
            .as_ref()
            .expect("new-round trigger context");
        assert_eq!(trigger.round_id, INITIAL_ROUND);
        assert_eq!(trigger.node_id, "accept");
        assert_eq!(trigger.outcome.as_deref(), Some("failure"));
        assert_eq!(trigger.branch_direction.as_deref(), Some("$new-round"));
        assert_eq!(trigger.attachments[0].name, "review.txt");
        let successor = &invocations[1];
        assert_eq!(successor.runtime_context.node_id, "accept");
        assert!(successor.new_round_trigger.is_none());
        assert_eq!(successor.predecessors.len(), entry.predecessors.len());
        assert!(
            successor
                .predecessors
                .iter()
                .all(|context| { context.round_id == CURRENT_ROUND && context.node_id == "dev" })
        );
        assert_eq!(
            app.run_status(TASK, &accepted.run.id).unwrap().outcome,
            Some(RunOutcome::Success)
        );
    }
}

#[test]
fn retry_new_round_entry_preserves_cross_round_context() {
    assert_reexecution_context(false);
}

#[test]
fn failed_node_rerun_new_round_entry_preserves_cross_round_context() {
    assert_reexecution_context(true);
}
