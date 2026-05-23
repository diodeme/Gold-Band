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
            capabilities: None,
        }
    }

    fn run_worker(&self, _req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                primary_artifact: Some(PrimaryArtifactPayload {
                    name: "implementation-result".to_string(),
                    content: r#"{"summary":"implemented"}"#.to_string(),
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
fn run_start_executes_worker_node() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    let app = App::with_provider(repo_root.clone(), Box::new(FakeProvider));

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
          "id": "dev-worker",
          "entry": "dev",
          "control": {{ "max_attempts": 1 }},
          "nodes": [
            {{
              "id": "dev",
              "type": "worker",
              "provider": "claude-acp",
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

    let artifact_path = app.paths.artifact_file(
        task_id,
        "run-001",
        "round-001",
        "dev",
        "attempt-001",
        "implementation-result",
    );
    assert!(artifact_path.exists());
}
