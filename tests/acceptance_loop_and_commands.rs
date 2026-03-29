use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderResultPayload, ProviderRunResult,
    ProviderRunStatus, WorkerInvocation, SessionRef,
};
use tempfile::tempdir;

#[derive(Clone, Default)]
struct LoopingProvider {
    call_count: std::sync::Arc<std::sync::Mutex<u32>>,
}

impl ProviderAdapter for LoopingProvider {
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
        let mut count = self.call_count.lock().unwrap();
        *count += 1;
        let payload = match req.primary_artifact.as_deref() {
            Some("exec-plan") => PrimaryArtifactPayload {
                name: "exec-plan".to_string(),
                content: r#"{"version":"0.1","commands":[{"id":"ok","run":"echo ok","purpose":"run checks"}]}"#.to_string(),
            },
            Some("verify-result") if *count < 3 => PrimaryArtifactPayload {
                name: "verify-result".to_string(),
                content: r#"{"version":"0.1","status":"failure","summary":"not yet","unmet_requirements":["missing requirement"],"validation_gaps":[]}"#.to_string(),
            },
            Some("verify-result") => PrimaryArtifactPayload {
                name: "verify-result".to_string(),
                content: r#"{"version":"0.1","status":"success","summary":"accepted","unmet_requirements":[],"validation_gaps":[]}"#.to_string(),
            },
            _ => unreachable!(),
        };

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                primary_artifact: Some(payload),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-code".to_string(),
                mode: SessionMode::New,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({"sessionId": format!("session-{}", *count)})),
                open_command: Some(format!("claude -c session-{}", *count)),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(&self, worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

#[test]
fn acceptance_loop_creates_new_round_and_commands_work() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    std::fs::create_dir_all(repo_root.join(".gold-band/tasks/task-001/authoring").as_std_path()).unwrap();
    std::fs::write(repo_root.join(".gold-band/tasks/task-001/authoring/requirement.md").as_std_path(), "Implement feature").unwrap();
    std::fs::write(
        repo_root.join(".gold-band/tasks/task-001/authoring/workflow.json").as_std_path(),
        r#"{
          "version": "0.1",
          "id": "full-flow",
          "entry": "dev",
          "control": {
            "max_repair_loops": 1,
            "max_acceptance_loops": 2,
            "on_acceptance_failure": "auto-loop"
          },
          "nodes": [
            {"id":"dev","type":"worker","provider":"claude-code","profile":"developer","goal":"Create an exec plan","primary_artifact":"exec-plan"},
            {"id":"run-tests","type":"exec","plan_from":"dev"},
            {"id":"accept","type":"verify","provider":"claude-code","profile":"verifier"}
          ],
          "edges": [
            {"from":"dev","to":"run-tests","on":"success"},
            {"from":"run-tests","to":"accept","on":"success"}
          ]
        }"#,
    )
    .unwrap();
    std::fs::write(repo_root.join(".gold-band/tasks/task-001/task.json").as_std_path(), r#"{"version":"0.1","id":"task-001"}"#).unwrap();

    let app = App::with_provider(repo_root.clone(), Box::new(LoopingProvider::default()));
    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.id, "run-001");

    let continued = app.run_status(task_id, "run-001").unwrap();
    assert_eq!(continued.outcome, Some(gold_band::domain::RunOutcome::Success));
    assert!(repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-002").exists());

    let command = app.run_open_session(task_id, "run-001", "round-002", "accept", "attempt-001").unwrap();
    assert!(command.starts_with("claude -c session-"));

    let artifacts = app.artifact_list(task_id, "run-001", "round-002", "accept", "attempt-001").unwrap();
    assert!(artifacts.iter().any(|name| name == "verify-result.json"));

    let killed = app.run_kill(task_id, "run-001").unwrap();
    assert_eq!(killed.outcome, Some(gold_band::domain::RunOutcome::Killed));
}
