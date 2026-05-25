use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, OutputArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
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
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        self.invocations.lock().unwrap().push(req.clone());

        let payload = match req
            .output_contract
            .as_ref()
            .map(|contract| contract.artifact.as_str())
        {
            Some("implementation-result") => {
                std::fs::create_dir_all(req.attempt_dir.join("attachments").as_std_path()).unwrap();
                std::fs::write(
                    req.attempt_dir.join("attachments/context.md").as_std_path(),
                    "attachment context",
                )
                .unwrap();
                OutputArtifactPayload {
                    name: "implementation-result".to_string(),
                    content: r#"{"summary":"implemented"}"#.to_string(),
                }
            }
            Some("test-result") => OutputArtifactPayload {
                name: "test-result".to_string(),
                content: r#"{"result":true,"reason":"checks passed"}"#.to_string(),
            },
            Some("accept-result") => OutputArtifactPayload {
                name: "accept-result".to_string(),
                content: r#"{"result":true,"reason":"accepted"}"#.to_string(),
            },
            _ => unreachable!(),
        };

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                output_artifact: Some(payload),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
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

fn write_happy_path_fixture(app: &App, _repo_root: &Utf8PathBuf, task_id: &str) {
    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let profiles = app.profiles().unwrap();
    let dev_profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id
        .clone();
    let accept_profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.name == "验收")
        .unwrap()
        .id
        .clone();
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
          "id": "full-flow",
          "entry": "dev",
          "control": {{ "max_attempts": 1 }},
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-acp","profile":"{}","goal":"Implement the requirement","output":{{"kind":"json","artifact":"implementation-result"}}}},
            {{"id":"test","type":"worker","provider":"claude-acp","profile":"{}","goal":"Check the implementation and return JSON with result and reason fields","output":{{"kind":"json","artifact":"test-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},
            {{"id":"accept","type":"worker","provider":"claude-acp","profile":"{}","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"dev","to":"test","on":"success"}},
            {{"from":"test","to":"accept","on":"success"}},
            {{"from":"accept","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile, dev_profile, accept_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        format!(r#"{{"version":"0.1","id":"{task_id}"}}"#),
    )
    .unwrap();
}

#[test]
fn run_start_completes_worker_test_accept_happy_path() {
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
                "accept-result"
            )
            .exists()
    );

    let invocations = provider.invocations.lock().unwrap();
    let accept_call = invocations
        .iter()
        .find(|call| {
            call.output_contract
                .as_ref()
                .is_some_and(|contract| contract.artifact == "accept-result")
        })
        .unwrap();
    assert!(accept_call.attachments_dir.is_some());
    assert!(accept_call.output_contract.is_some());
    assert!(accept_call.cold_artifacts.is_empty());
    assert!(accept_call.cold_attachments.is_empty());
}
