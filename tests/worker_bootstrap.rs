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
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.invocations.lock().unwrap().push(req);
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                primary_artifact: Some(PrimaryArtifactPayload {
                    name: "exec-plan".to_string(),
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
                    name: "verify-result".to_string(),
                    content: r#"{"version":"0.1","status":"success","summary":"accepted","unmet_requirements":[],"validation_gaps":[]}"#.to_string(),
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
            "max_repair_loops": 1,
            "max_acceptance_loops": 1,
            "on_acceptance_failure": "stop"
          }},
          "nodes": [
            {{
              "id": "dev",
              "type": "worker",
              "provider": "claude-code",
              "profile": "{}",
              "goal": "Create an exec plan",
              "primary_artifact": "exec-plan"
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
        "exec-plan",
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
        "Verify feature",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        format!(
            r#"{{
          "version": "0.1",
          "id": "continue-flow",
          "entry": "accept",
          "control": {{
            "max_repair_loops": 1,
            "max_acceptance_loops": 1,
            "on_acceptance_failure": "stop"
          }},
          "nodes": [
            {{"id":"accept","type":"verify","provider":"claude-code","profile":"{}"}}
          ],
          "edges": []
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
        acceptance_loops_used: 0,
        pause_reason: Some(PauseReason::ErrorBlocked),
    };

    assert!(is_run_continuable(&run));
}
