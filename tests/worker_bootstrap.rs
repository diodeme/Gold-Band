use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
};
use gold_band::runtime::RunState;
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

#[test]
fn run_start_executes_entry_worker_and_persists_outputs() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    let gold_band_home = repo_root.join("gold-band-home");
    unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
    let provider = RecordingProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    std::fs::create_dir_all(repo_root.join(".gold-band/presets/profiles").as_std_path()).unwrap();
    std::fs::write(
        repo_root
            .join(".gold-band/presets/profiles/developer.md")
            .as_std_path(),
        "developer profile",
    )
    .unwrap();
    std::fs::write(
        app.paths.requirement_file(task_id).as_std_path(),
        "Implement feature",
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        r#"{
          "version": "0.1",
          "id": "dev-only",
          "entry": "dev",
          "control": {
            "max_repair_loops": 1,
            "max_acceptance_loops": 1,
            "on_acceptance_failure": "stop"
          },
          "nodes": [
            {
              "id": "dev",
              "type": "worker",
              "provider": "claude-code",
              "profile": "developer",
              "goal": "Create an exec plan",
              "primary_artifact": "exec-plan"
            }
          ],
          "edges": []
        }"#,
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
