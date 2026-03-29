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
struct SequencedProvider {
    calls: Arc<Mutex<u32>>,
}

impl ProviderAdapter for SequencedProvider {
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
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;

        let payload = match req.primary_artifact.as_deref() {
            Some("exec-plan") => PrimaryArtifactPayload {
                name: "exec-plan".to_string(),
                content: r#"{"version":"0.1","commands":[{"id":"ok","run":"echo ok","purpose":"run checks"}]}"#.to_string(),
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
                continue_ref: Some(serde_json::json!({"sessionId": format!("session-{}", *calls)})),
                open_command: Some(format!("claude -c session-{}", *calls)),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-1".to_string()))
    }
}

#[test]
fn run_start_completes_worker_exec_verify_happy_path() {
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
            "max_acceptance_loops": 1,
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

    let app = App::with_provider(repo_root.clone(), Box::new(SequencedProvider::default()));
    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.id, "run-001");

    let run_state: RunState = gold_band::storage::read_json(&repo_root.join(".gold-band/tasks/task-001/runs/run-001/run.json")).unwrap();
    assert_eq!(run_state.status, gold_band::domain::RunStatus::Completed);
    assert_eq!(run_state.outcome, Some(gold_band::domain::RunOutcome::Success));

    assert!(repo_root.join(".gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/accept/attempt-001/artifacts/verify-result.json").exists());
}
