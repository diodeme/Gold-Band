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
struct SequencedProvider {
    calls: Arc<Mutex<u32>>,
    invocations: Arc<Mutex<Vec<WorkerInvocation>>>,
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
        DoctorResult {
            available: true,
            reason: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        self.invocations.lock().unwrap().push(req.clone());

        let payload = match req.primary_artifact.as_deref() {
            Some("exec-plan") => {
                if req.invocation_kind == gold_band::domain::InvocationKind::WorkerGeneric {
                    std::fs::create_dir_all(req.attempt_dir.join("attachments").as_std_path()).unwrap();
                    std::fs::write(req.attempt_dir.join("attachments/context.md").as_std_path(), "attachment context").unwrap();
                }
                PrimaryArtifactPayload {
                    name: "exec-plan".to_string(),
                    content: r#"{"version":"0.1","commands":[{"id":"ok","run":"echo ok","purpose":"run checks"}]}"#.to_string(),
                }
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

    fn build_continue_command(
        &self,
        _worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-1".to_string()))
    }
}

fn write_happy_path_fixture(app: &App, repo_root: &Utf8PathBuf, task_id: &str) {
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
        repo_root
            .join(".gold-band/presets/profiles/verifier.md")
            .as_std_path(),
        "verifier profile",
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
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        format!(r#"{{"version":"0.1","id":"{task_id}"}}"#),
    )
    .unwrap();
}

#[test]
fn run_start_completes_worker_exec_verify_happy_path() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";
    let gold_band_home = repo_root.join("gold-band-home");
    unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
    let provider = SequencedProvider::default();
    let app = App::with_provider(repo_root.clone(), Box::new(provider.clone()));
    write_happy_path_fixture(&app, &repo_root, task_id);
    let run = app.run_start(task_id, None).unwrap();
    assert_eq!(run.id, "run-001");

    let run_state: RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, "run-001")).unwrap();
    assert_eq!(run_state.status, gold_band::domain::RunStatus::Completed);
    assert_eq!(
        run_state.outcome,
        Some(gold_band::domain::RunOutcome::Success)
    );

    assert!(
        app.paths
            .artifact_file(
                task_id,
                "run-001",
                "round-001",
                "accept",
                "attempt-001",
                "verify-result"
            )
            .exists()
    );

    let invocations = provider.invocations.lock().unwrap();
    let verify_call = invocations
        .iter()
        .find(|call| call.primary_artifact.as_deref() == Some("verify-result"))
        .unwrap();
    assert!(verify_call.attachments_dir.is_none());
    assert_eq!(verify_call.cold_artifacts.len(), 2);
    assert!(
        verify_call
            .cold_artifacts
            .iter()
            .any(|entry| entry.name.as_deref() == Some("exec-result"))
    );
    assert!(
        verify_call
            .cold_artifacts
            .iter()
            .any(|entry| entry.name.as_deref() == Some("worker-primary-artifact"))
    );
    assert_eq!(verify_call.cold_attachments.len(), 1);
    assert_eq!(
        verify_call.cold_attachments[0].path.file_name(),
        Some("context.md")
    );
}
