use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderResultPayload, ProviderRunResult,
    ProviderRunStatus, WorkerInvocation, SessionRef,
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
        DoctorResult { available: true, reason: None }
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

    fn build_continue_command(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-123".to_string()))
    }
}

#[test]
fn run_start_executes_entry_worker_and_persists_outputs() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    std::fs::create_dir_all(repo_root.join(".gold-band/tasks/task-001/authoring").as_std_path()).unwrap();
    std::fs::create_dir_all(repo_root.join(".gold-band/presets/profiles").as_std_path()).unwrap();
    std::fs::write(repo_root.join(".gold-band/presets/profiles/developer.md").as_std_path(), "developer profile").unwrap();
    std::fs::write(
        repo_root.join(".gold-band/tasks/task-001/authoring/requirement.md").as_std_path(),
        "Implement feature",
    )
    .unwrap();
    std::fs::write(
        repo_root.join(".gold-band/tasks/task-001/authoring/workflow.json").as_std_path(),
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
        repo_root.join(".gold-band/tasks/task-001/task.json").as_std_path(),
        r#"{"version":"0.1","id":"task-001"}"#,
    )
    .unwrap();

    let provider = RecordingProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));

    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.id, "run-001");

    let invocation_count = provider.invocations.lock().unwrap().len();
    assert_eq!(invocation_count, 1);

    let run_state: RunState = gold_band::storage::read_json(&repo_root.join(".gold-band/tasks/task-001/runs/run-001/run.json")).unwrap();
    assert_eq!(run_state.id, "run-001");

    let artifact_path = repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/artifacts/exec-plan.json");
    assert!(artifact_path.exists());

    let worker_ref_path = repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/worker-ref.json");
    assert!(worker_ref_path.exists());

}
