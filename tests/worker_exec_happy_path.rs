use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
};
use tempfile::tempdir;

#[derive(Clone, Default)]
struct FakeProvider;

impl ProviderAdapter for FakeProvider {
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

    fn run_worker(&self, _req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                primary_artifact: Some(PrimaryArtifactPayload {
                    name: "exec-plan".to_string(),
                    content: r#"{"version":"0.1","commands":[{"id":"write-ok","run":"echo ok","purpose":"validate happy path"}]}"#.to_string(),
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
fn run_start_executes_worker_then_exec() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    let gold_band_home = repo_root.join("gold-band-home");
    unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
    let app = App::with_provider(repo_root.clone(), Box::new(FakeProvider));

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
          "id": "dev-exec",
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
            },
            {
              "id": "run-tests",
              "type": "exec",
              "plan_from": "dev"
            }
          ],
          "edges": [
            { "from": "dev", "to": "run-tests", "on": "success" }
          ]
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

    let exec_result_path = app.paths.artifact_file(
        task_id,
        "run-001",
        "round-001",
        "run-tests",
        "attempt-001",
        "exec-result",
    );
    assert!(exec_result_path.exists());

    let stdout_log = app
        .paths
        .attempt_dir(task_id, "run-001", "round-001", "run-tests", "attempt-001")
        .join("commands/01-write-ok/stdout.log");
    assert!(stdout_log.exists());
}
