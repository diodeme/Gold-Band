use camino::Utf8PathBuf;
use gold_band::app::{App, is_run_continuable};
use gold_band::domain::{PauseReason, RunOutcome, RunStatus, SessionMode, VERSION};
use gold_band::provider::{
    DoctorResult, OutputArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, UserPromptRenderMode,
    WorkerInvocation,
};
use gold_band::runtime::{RunState, WorkerRefState};
use std::sync::{Arc, Barrier, Mutex};
use tempfile::tempdir;

#[derive(Clone, Default)]
struct RecordingProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

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
            reason: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.invocations.lock().unwrap().push(req);
        Ok(success_result())
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        _worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-123".to_string()))
    }
}

#[derive(Clone)]
struct BlockingSuccessProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
    release: Arc<Barrier>,
}

impl BlockingSuccessProvider {
    fn new(participants: usize) -> Self {
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            release: Arc::new(Barrier::new(participants)),
        }
    }
}

impl ProviderAdapter for BlockingSuccessProvider {
    fn describe_provider(&self) -> ProviderInfo {
        RecordingProvider::default().describe_provider()
    }

    fn doctor(&self) -> DoctorResult {
        RecordingProvider::default().doctor()
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.invocations.lock().unwrap().push(req);
        self.release.wait();
        Ok(success_result())
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        _worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-123".to_string()))
    }
}

fn write_dev_only_workflow(app: &App, task_id: &str) {
    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Implement feature",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "dev-only",
          "entry": "dev",
          "control": {{
            "max_attempts": 1,
            "max_rounds": 1
          }},
          "nodes": [
            {{
              "id": "dev",
              "type": "worker",
              "provider": "claude-acp",
              "profile": "{}",
              "goal": "Create an implementation result",
              "output": {{ "kind": "json", "artifact": "implementation-result" }}
            }}
          ],
          "edges": [
            {{"from":"dev","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        format!(r#"{{"version":"0.1","id":"{}"}}"#, task_id),
    )
    .unwrap();
}

fn success_result() -> ProviderRunResult {
    ProviderRunResult {
        status: ProviderRunStatus::Success,
        exit_code: Some(0),
        result_payload: Some(ProviderResultPayload {
            output_artifact: Some(OutputArtifactPayload {
                name: "implementation-result".to_string(),
                content: r#"{"version":"0.1","commands":[{"id":"build","run":"cargo test","purpose":"validate"}]}"#.to_string(),
            }),
        }),
        worker_ref_seed: Some(SessionRef {
            provider: "claude-acp".to_string(),
            mode: SessionMode::New,
            supports_open_session: true,
            supports_continue_session: true,
            continue_ref: Some(serde_json::json!({"sessionId":"session-123"})),
            open_command: Some("claude -c session-123".to_string()),
        }),
        stream_path: None,
    }
}

#[derive(Clone, Default)]
struct InterruptThenSuccessProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

impl ProviderAdapter for InterruptThenSuccessProvider {
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
            reason: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let mut invocations = self.invocations.lock().unwrap();
        let status = if invocations.is_empty() {
            ProviderRunStatus::Interrupted
        } else {
            ProviderRunStatus::Success
        };
        invocations.push(req);

        Ok(ProviderRunResult {
            status,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: "accept-result".to_string(),
                    content: r#"{"result":true,"reason":"accepted"}"#.to_string(),
                }),
            }),
            worker_ref_seed: None,
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        _worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-123".to_string()))
    }
}

#[derive(Clone, Default)]
struct InterruptedThenContinueProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

impl ProviderAdapter for InterruptedThenContinueProvider {
    fn describe_provider(&self) -> ProviderInfo {
        RecordingProvider::default().describe_provider()
    }

    fn doctor(&self) -> DoctorResult {
        RecordingProvider::default().doctor()
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let mut invocations = self.invocations.lock().unwrap();
        let first = invocations.is_empty();
        let session_mode = req.session_mode;
        invocations.push(req);
        if first {
            return Ok(ProviderRunResult {
                status: ProviderRunStatus::Interrupted,
                exit_code: Some(0),
                result_payload: None,
                worker_ref_seed: Some(SessionRef {
                    provider: "claude-acp".to_string(),
                    mode: SessionMode::New,
                    supports_open_session: true,
                    supports_continue_session: true,
                    continue_ref: Some(serde_json::json!({"sessionId":"session-123"})),
                    open_command: Some("claude -c session-123".to_string()),
                }),
                stream_path: None,
            });
        }
        assert_eq!(session_mode, SessionMode::Continue);
        Ok(success_result())
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        _worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-123".to_string()))
    }
}

#[derive(Clone, Default)]
struct AlwaysFailAcceptanceProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

impl ProviderAdapter for AlwaysFailAcceptanceProvider {
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
            reason: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.invocations.lock().unwrap().push(req);
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: "accept-result".to_string(),
                    content: r#"{"result":false,"reason":"needs another round"}"#.to_string(),
                }),
            }),
            worker_ref_seed: None,
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

#[derive(Clone, Default)]
struct MultiAttemptContinueProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

impl ProviderAdapter for MultiAttemptContinueProvider {
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
            reason: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let node_id = req.runtime_context.node_id.clone();
        let attempt_id = req.runtime_context.attempt_id.clone();
        let session_mode = req.session_mode;
        let review_count = if node_id == "review" {
            self.invocations
                .lock()
                .unwrap()
                .iter()
                .filter(|invocation| invocation.runtime_context.node_id == "review")
                .count()
                + 1
        } else {
            0
        };
        self.invocations.lock().unwrap().push(req);
        let success = node_id != "review" || review_count >= 3;

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: format!("{node_id}-result"),
                    content: format!(r#"{{"result":{success},"reason":"{node_id} {attempt_id}"}}"#),
                }),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: session_mode,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(
                    serde_json::json!({"sessionId": format!("{node_id}-{attempt_id}")}),
                ),
                open_command: Some(format!("claude -c {node_id}-{attempt_id}")),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

#[derive(Clone, Default)]
struct OneRepairProvider {
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
}

impl ProviderAdapter for OneRepairProvider {
    fn describe_provider(&self) -> ProviderInfo {
        MultiAttemptContinueProvider::default().describe_provider()
    }

    fn doctor(&self) -> DoctorResult {
        MultiAttemptContinueProvider::default().doctor()
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let node_id = req.runtime_context.node_id.clone();
        let attempt_id = req.runtime_context.attempt_id.clone();
        let session_mode = req.session_mode;
        let review_count = if node_id == "review" {
            self.invocations
                .lock()
                .unwrap()
                .iter()
                .filter(|invocation| invocation.runtime_context.node_id == "review")
                .count()
                + 1
        } else {
            0
        };
        self.invocations.lock().unwrap().push(req);
        let success = node_id != "review" || review_count >= 2;

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(OutputArtifactPayload {
                    name: format!("{node_id}-result"),
                    content: format!(r#"{{"result":{success},"reason":"{node_id} {attempt_id}"}}"#),
                }),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: session_mode,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(
                    serde_json::json!({"sessionId": format!("{node_id}-{attempt_id}")}),
                ),
                open_command: Some(format!("claude -c {node_id}-{attempt_id}")),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

#[test]
fn run_start_executes_entry_worker_and_persists_outputs() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    let provider = RecordingProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Implement feature",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "dev-only",
          "entry": "dev",
          "control": {{
            "max_attempts": 1,
            "max_rounds": 1
          }},
          "nodes": [
            {{
              "id": "dev",
              "type": "worker",
              "provider": "claude-acp",
              "profile": "{}",
              "goal": "Create an implementation result",
              "output": {{ "kind": "json", "artifact": "implementation-result" }}
            }}
          ],
          "edges": [
            {{"from":"dev","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-001"}"#,
    )
    .unwrap();

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.id, "run-001");

    let invocation_count = provider.invocations.lock().unwrap().len();
    assert_eq!(invocation_count, 1);

    let run_state: RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-001")).unwrap();
    assert_eq!(run_state.id, "run-001");

    let artifact_path = app.paths.artifact_file(
        task_id,
        "run-001",
        "round-001",
        "dev",
        "attempt-001",
        "implementation-result",
    );
    assert!(artifact_path.exists());

    let worker_ref_path =
        app.paths
            .worker_ref_file(task_id, "run-001", "round-001", "dev", "attempt-001");
    assert!(worker_ref_path.exists());
}

#[test]
fn run_pause_keeps_current_worker_paused_not_killed() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-pause-worker";
    let app = App::with_provider(repo_root, Box::new(RecordingProvider::default()));
    write_dev_only_workflow(&app, task_id);

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);

    let mut running_run: RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-001")).unwrap();
    running_run.status = RunStatus::Running;
    running_run.outcome = None;
    gold_band::storage::write_json(&app.paths.run_file(task_id, "run-001"), &running_run).unwrap();
    let node_path = app
        .paths
        .node_file(task_id, "run-001", "round-001", "dev", "attempt-001");
    let mut node: gold_band::runtime::NodeState =
        gold_band::storage::read_json(&node_path).unwrap();
    node.status = RunStatus::Running;
    node.outcome = None;
    gold_band::storage::write_json(&node_path, &node).unwrap();
    let pid_path =
        app.paths
            .provider_pid_file(task_id, "run-001", "round-001", "dev", "attempt-001");
    std::fs::write(pid_path.as_std_path(), "12345").unwrap();

    let paused = app
        .run_pause(task_id, "run-001", PauseReason::ProcessInterrupted)
        .unwrap();

    assert_eq!(paused.status, RunStatus::Paused);
    assert_eq!(paused.pause_reason, Some(PauseReason::ProcessInterrupted));
    assert!(pid_path.exists());
    let node: gold_band::runtime::NodeState = gold_band::storage::read_json(&node_path).unwrap();
    assert_eq!(node.status, RunStatus::Paused);
    assert_eq!(node.outcome, None);
}

#[test]
fn stopped_attempt_success_does_not_complete_workflow() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-stop-race";
    let provider = BlockingSuccessProvider::new(2);
    let app = Arc::new(App::with_provider(repo_root, Box::new(provider.clone())));
    write_dev_only_workflow(&app, task_id);

    let runner = {
        let app = app.clone();
        let task_id = task_id.to_string();
        std::thread::spawn(move || app.run_start(&task_id, None).unwrap())
    };

    while provider.invocations.lock().unwrap().is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let paused = app
        .run_pause(task_id, "run-001", PauseReason::ProcessInterrupted)
        .unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    provider.release.wait();

    let returned = runner.join().unwrap();
    assert_eq!(returned.status, RunStatus::Running);
    let run: RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-001")).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));
    assert!(
        !app.paths
            .artifact_file(
                task_id,
                "run-001",
                "round-001",
                "dev",
                "attempt-001",
                "implementation-result"
            )
            .exists()
    );
}

#[test]
fn run_continue_ignores_cancelled_session_snapshot() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-continue-after-cancel";
    let provider = InterruptedThenContinueProvider::default();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    write_dev_only_workflow(&app, task_id);

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));

    let attempt_dir = app
        .paths
        .attempt_dir(task_id, "run-001", "round-001", "dev", "attempt-001");
    gold_band::storage::write_json(
        &attempt_dir.join("acp.session.json"),
        &serde_json::json!({
            "sessionId": "attempt-001",
            "status": "cancelled",
            "stopReason": "cancelled",
        }),
    )
    .unwrap();

    let continued = app
        .run_continue(task_id, "run-001", None, Some("resume".to_string()))
        .unwrap();

    assert_eq!(continued.status, RunStatus::Completed);
    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[1].session_mode, SessionMode::Continue);
    assert_eq!(
        invocations[1]
            .continue_ref
            .as_ref()
            .and_then(|value| value.get("sessionId").or_else(|| value.get("acpSessionId")))
            .and_then(|value| value.as_str()),
        Some("session-123"),
    );
}

#[test]
fn run_continue_ignores_stale_provider_pid_metadata() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-continue-stale-pid";
    let provider = InterruptedThenContinueProvider::default();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    write_dev_only_workflow(&app, task_id);

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));

    let pid_path =
        app.paths
            .provider_pid_file(task_id, "run-001", "round-001", "dev", "attempt-001");
    std::fs::write(pid_path.as_std_path(), "12345").unwrap();

    let continued = app
        .run_continue(task_id, "run-001", None, Some("resume".to_string()))
        .unwrap();

    assert_eq!(continued.status, RunStatus::Completed);
    assert_eq!(provider.invocations.lock().unwrap().len(), 2);
}

#[test]
fn run_continue_after_process_interrupted_user_input_uses_user_message_render_mode() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-continue-user-message";
    let provider = InterruptedThenContinueProvider::default();
    let app = App::with_provider(repo_root, Box::new(provider.clone()));
    write_dev_only_workflow(&app, task_id);
    let inputs_dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
    std::fs::create_dir_all(inputs_dir.as_std_path()).unwrap();
    let input_path = inputs_dir.join("测试需求.txt");
    std::fs::write(input_path.as_std_path(), "attached task input").unwrap();

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.pause_reason, Some(PauseReason::ProcessInterrupted));

    let continued = app
        .run_continue(
            task_id,
            "run-001",
            Some("resume-user-001".to_string()),
            Some("请继续检查这个会话".to_string()),
        )
        .unwrap();

    assert_eq!(continued.status, RunStatus::Completed);
    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[1].session_mode, SessionMode::Continue);
    assert_eq!(
        invocations[1].user_prompt_render_mode,
        UserPromptRenderMode::UserMessage
    );
    assert_eq!(
        invocations[1].resume_prompt.as_deref(),
        Some("请继续检查这个会话")
    );
    assert_eq!(
        invocations[1].resume_prompt_id.as_deref(),
        Some("resume-user-001")
    );
}

#[test]
fn run_start_background_allocates_from_max_run_id_under_concurrency() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-concurrent-rerun";

    let provider = BlockingSuccessProvider::new(2);
    let app = Arc::new(App::with_provider(
        repo_root.clone(),
        Box::new(provider.clone()),
    ));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Implement feature",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "dev-only",
          "entry": "dev",
          "control": {{
            "max_attempts": 1,
            "max_rounds": 1
          }},
          "nodes": [
            {{
              "id": "dev",
              "type": "worker",
              "provider": "claude-acp",
              "profile": "{}",
              "goal": "Create an implementation result",
              "output": {{ "kind": "json", "artifact": "implementation-result" }}
            }}
          ],
          "edges": [
            {{"from":"dev","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-concurrent-rerun"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(app.paths.run_dir(task_id, "run-001").as_std_path()).unwrap();
    std::fs::create_dir_all(app.paths.run_dir(task_id, "run-002").as_std_path()).unwrap();
    std::fs::create_dir_all(app.paths.run_dir(task_id, "run-005").as_std_path()).unwrap();

    let start = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let app = app.clone();
            let start = start.clone();
            let task_id = task_id.to_string();
            std::thread::spawn(move || {
                start.wait();
                app.run_start_background(&task_id, None).unwrap().id
            })
        })
        .collect::<Vec<_>>();
    start.wait();
    let mut run_ids = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    run_ids.sort();

    assert_eq!(run_ids, vec!["run-006".to_string(), "run-007".to_string()]);
    assert!(app.paths.run_file(task_id, "run-006").exists());
    assert!(app.paths.run_file(task_id, "run-007").exists());

    for _ in 0..100 {
        if provider.invocations.lock().unwrap().len() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(provider.invocations.lock().unwrap().len(), 2);
}

#[test]
fn run_continue_sends_localized_resume_prompt_to_existing_session() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-continue";

    let provider = InterruptThenSuccessProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let accept_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "验收")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Check feature",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "continue-flow",
          "entry": "accept",
          "control": {{ "max_attempts": 1 }},
          "nodes": [
            {{"id":"accept","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"accept","to":"$end","on":"success"}}
          ]
        }}"#,
            accept_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-continue"}"#,
    )
    .unwrap();

    let paused = app.run_start(task_id, None).unwrap();
    assert_eq!(paused.status, RunStatus::Paused);
    assert!(is_run_continuable(&paused));

    gold_band::storage::write_json(
        &app.paths
            .worker_ref_file(task_id, "run-001", "round-001", "accept", "attempt-001"),
        &WorkerRefState {
            version: gold_band::domain::VERSION.to_string(),
            provider: "claude-acp".to_string(),
            mode: SessionMode::Continue,
            supports_open_session: true,
            supports_continue_session: true,
            continue_ref: Some(serde_json::json!({"acpSessionId":"session-123"})),
            open_command: None,
        },
    )
    .unwrap();

    let manual_prompt = app
        .acp_prompt_bundle_for_attempt(
            task_id,
            "run-001",
            "round-001",
            "accept",
            "attempt-001",
            "手动追问".to_string(),
            Some("manual-prompt-001".to_string()),
            Some(serde_json::json!({"acpSessionId":"session-123"})),
        )
        .unwrap();
    assert!(manual_prompt.system_prompt.contains("Run: run-001"));
    assert!(
        manual_prompt
            .system_prompt
            .contains("你必须在最后一步按照以下格式输出你的结果")
    );
    assert_eq!(manual_prompt.user_prompt, "手动追问");
    assert_eq!(
        manual_prompt.prompt_id.as_deref(),
        Some("manual-prompt-001")
    );

    let completed = app
        .run_continue(
            task_id,
            "run-001",
            Some("prompt-continue-001".to_string()),
            None,
        )
        .unwrap();
    assert_eq!(completed.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[1].session_mode, SessionMode::Continue);
    assert_eq!(
        invocations[1].user_prompt_render_mode,
        UserPromptRenderMode::WorkflowResume
    );
    assert_eq!(invocations[1].resume_prompt.as_deref(), Some("继续"));
    assert_eq!(
        invocations[1].resume_prompt_id.as_deref(),
        Some("prompt-continue-001")
    );
    assert_eq!(
        invocations[1]
            .continue_ref
            .as_ref()
            .and_then(|value| value.get("acpSessionId"))
            .and_then(|value| value.as_str()),
        Some("session-123"),
    );
}

#[test]
fn transition_continue_uses_latest_target_attempt_ref() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-continue-lineage";

    let provider = MultiAttemptContinueProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Exercise continue lineage",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "continue-lineage-flow",
          "entry": "dev",
          "control": {{ "max_attempts": 3 }},
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"dev-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},
            {{"id":"review","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"review-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"dev","to":"review","on":"success"}},
            {{"from":"review","to":"dev","on":"failure","session":"continue"}},
            {{"from":"review","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile, dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-continue-lineage"}"#,
    )
    .unwrap();

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    let dev_invocations = invocations
        .iter()
        .filter(|invocation| invocation.runtime_context.node_id == "dev")
        .collect::<Vec<_>>();
    assert_eq!(dev_invocations.len(), 3);
    assert_eq!(dev_invocations[1].session_mode, SessionMode::Continue);
    assert_eq!(
        dev_invocations[1]
            .continue_ref
            .as_ref()
            .and_then(|value| value.get("sessionId"))
            .and_then(|value| value.as_str()),
        Some("dev-attempt-001"),
    );
    assert_eq!(dev_invocations[2].session_mode, SessionMode::Continue);
    assert_eq!(
        dev_invocations[2]
            .continue_ref
            .as_ref()
            .and_then(|value| value.get("sessionId"))
            .and_then(|value| value.as_str()),
        Some("dev-attempt-002"),
    );
}

#[test]
fn max_attempts_allows_one_repair_loop_then_forward_success() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-max-attempts-success";

    let provider = OneRepairProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Exercise one repair loop",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "attempt-limit-success-flow",
          "entry": "dev",
          "control": {{ "max_attempts": 1 }},
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"dev-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},
            {{"id":"review","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"review-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"dev","to":"review","on":"success"}},
            {{"from":"review","to":"dev","on":"failure","session":"continue"}},
            {{"from":"review","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile, dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-max-attempts-success"}"#,
    )
    .unwrap();

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 4);
    assert_eq!(invocations[0].runtime_context.node_id, "dev");
    assert_eq!(invocations[1].runtime_context.node_id, "review");
    assert_eq!(invocations[2].runtime_context.node_id, "dev");
    assert_eq!(invocations[3].runtime_context.node_id, "review");
}

#[test]
fn max_attempts_fails_when_repair_budget_is_exceeded() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-max-attempts";

    let provider = MultiAttemptContinueProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Exercise attempt limit",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "attempt-limit-flow",
          "entry": "dev",
          "control": {{ "max_attempts": 1 }},
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"dev-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},
            {{"id":"review","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"review-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"dev","to":"review","on":"success"}},
            {{"from":"review","to":"dev","on":"failure","session":"continue"}},
            {{"from":"review","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile, dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-max-attempts"}"#,
    )
    .unwrap();

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Failure));

    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 4);
    assert_eq!(invocations[0].runtime_context.node_id, "dev");
    assert_eq!(invocations[1].runtime_context.node_id, "review");
    assert_eq!(invocations[2].runtime_context.node_id, "dev");
    assert_eq!(invocations[3].runtime_context.node_id, "review");
}

#[test]
fn max_rounds_fails_workflow_when_new_round_limit_is_exceeded() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-max-rounds";

    let provider = AlwaysFailAcceptanceProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let accept_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "验收")
        .unwrap()
        .id;
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Exercise round limit",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "round-limit-flow",
          "entry": "accept",
          "control": {{ "max_rounds": 1 }},
          "nodes": [
            {{"id":"accept","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"accept","to":"$new-round","on":"failure"}},
            {{"from":"accept","to":"$end","on":"success"}}
          ]
        }}"#,
            accept_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-max-rounds"}"#,
    )
    .unwrap();

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.outcome, Some(RunOutcome::Failure));
    assert_eq!(run.new_rounds_opened, 1);
    assert_eq!(provider.invocations.lock().unwrap().len(), 2);
}

#[test]
fn error_blocked_run_is_not_continuable() {
    let run = RunState {
        version: VERSION.to_string(),
        id: "run-001".to_string(),
        task_id: "task-001".to_string(),
        status: RunStatus::Paused,
        outcome: None,
        started_at: "2026-05-15T10:00:00Z".to_string(),
        updated_at: "2026-05-15T10:01:00Z".to_string(),
        workflow_snapshot: "workflow.snapshot.json".to_string(),
        current_round: Some("round-001".to_string()),
        current_node: Some("dev".to_string()),
        current_attempt: Some("attempt-001".to_string()),
        new_rounds_opened: 0,
        pause_reason: Some(PauseReason::ErrorBlocked),
        task_uuid: None,
        uuid: None,
        last_executed_node: None,
    };

    assert!(!is_run_continuable(&run));
}
