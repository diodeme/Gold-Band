use camino::Utf8PathBuf;
use gold_band::app::{App, is_run_continuable};
use gold_band::domain::{PauseReason, RunOutcome, RunStatus, SessionMode, VERSION};
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
};
use gold_band::runtime::{RunState, WorkerRefState};
use std::sync::{Arc, Mutex};
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
                primary_artifact: Some(PrimaryArtifactPayload {
                    name: "implementation-result".to_string(),
                    content: r#"{"version":"0.1","commands":[{"id":"build","run":"cargo test","purpose":"validate"}]}"#.to_string(),
                }),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-code".to_string(),
                mode: SessionMode::New,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({"sessionId":"session-123"})),
                open_command: Some("claude -c session-123".to_string()),
            }),
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
                primary_artifact: Some(PrimaryArtifactPayload {
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
                primary_artifact: Some(PrimaryArtifactPayload {
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
                primary_artifact: Some(PrimaryArtifactPayload {
                    name: format!("{node_id}-result"),
                    content: format!(r#"{{"result":{success},"reason":"{node_id} {attempt_id}"}}"#),
                }),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-code".to_string(),
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
              "provider": "claude-code",
              "profile": "{}",
              "goal": "Create an implementation result",
              "primary_artifact": "implementation-result"
            }}
          ],
          "edges": []
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
            {{"id":"accept","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"accept-result","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
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
            provider: "claude-code".to_string(),
            mode: SessionMode::Continue,
            supports_open_session: true,
            supports_continue_session: true,
            continue_ref: Some(serde_json::json!({"acpSessionId":"session-123"})),
            open_command: None,
        },
    )
    .unwrap();

    let completed = app
        .run_continue(task_id, "run-001", Some("prompt-continue-001".to_string()))
        .unwrap();
    assert_eq!(completed.outcome, Some(RunOutcome::Success));

    let invocations = provider.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[1].session_mode, SessionMode::Continue);
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
            {{"id":"dev","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"dev-result","output":{{"kind":"json","artifact":"dev-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},
            {{"id":"review","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"review-result","output":{{"kind":"json","artifact":"review-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
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
fn max_attempts_fails_workflow_when_edge_limit_is_exceeded() {
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
            {{"id":"dev","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"dev-result","output":{{"kind":"json","artifact":"dev-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},
            {{"id":"review","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"review-result","output":{{"kind":"json","artifact":"review-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
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
    assert_eq!(invocations.len(), 3);
    assert_eq!(invocations[0].runtime_context.node_id, "dev");
    assert_eq!(invocations[1].runtime_context.node_id, "review");
    assert_eq!(invocations[2].runtime_context.node_id, "dev");
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
            {{"id":"accept","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"accept-result","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"accept","to":"$new-round","on":"failure"}}
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
fn error_blocked_run_is_continuable() {
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
    };

    assert!(is_run_continuable(&run));
}
