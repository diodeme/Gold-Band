use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
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
        DoctorResult {
            available: true,
            reason: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let mut count = self.call_count.lock().unwrap();
        *count += 1;
        let payload = match req.primary_artifact.as_deref() {
            Some("exec-plan") => PrimaryArtifactPayload {
                name: "exec-plan".to_string(),
                content: r#"{"version":"0.1","commands":[{"id":"ok","run":"echo ok","purpose":"run checks"}]}"#.to_string(),
            },
            Some("accept-result") if *count < 4 => PrimaryArtifactPayload {
                name: "accept-result".to_string(),
                content: r#"{"result":false,"reason":"not yet"}"#.to_string(),
            },
            Some("accept-result") => PrimaryArtifactPayload {
                name: "accept-result".to_string(),
                content: r#"{"result":true,"reason":"accepted"}"#.to_string(),
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

    fn build_continue_command(
        &self,
        worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(worker_ref.open_command.clone())
    }
}

#[test]
fn acceptance_loop_creates_new_round_and_commands_work() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let task_id = "task-001";

    let gold_band_home = repo_root.join("gold-band-home");
    unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };
    let app = App::with_provider(repo_root.clone(), Box::new(LoopingProvider::default()));

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
          "control": {{ "max_repair_loops": 1 }},
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-code","profile":"{}","goal":"Create an exec plan","primary_artifact":"exec-plan"}},
            {{"id":"run-tests","type":"exec","plan_from":"dev"}},
            {{"id":"accept","type":"worker","provider":"claude-code","profile":"{}","primary_artifact":"accept-result","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"dev","to":"run-tests","on":"success"}},
            {{"from":"run-tests","to":"accept","on":"success"}},
            {{"from":"accept","to":"$end","on":"success"}},
            {{"from":"accept","to":"$new-round","on":"failure"}}
          ]
        }}"#,
            dev_profile, accept_profile
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

    let continued = app.run_status(task_id, "run-001").unwrap();
    assert_eq!(
        continued.outcome,
        Some(gold_band::domain::RunOutcome::Success)
    );
    assert!(
        app.paths
            .round_dir(task_id, "run-001", "round-002")
            .exists()
    );

    let command = app
        .run_open_session(task_id, "run-001", "round-002", "accept", "attempt-001")
        .unwrap();
    assert!(command.starts_with("claude -c session-"));

    let artifacts = app
        .artifact_list(task_id, "run-001", "round-002", "accept", "attempt-001")
        .unwrap();
    assert!(artifacts.iter().any(|name| name == "accept-result"));
    assert!(
        app.artifact_show(
            task_id,
            "run-001",
            "round-002",
            "accept",
            "attempt-001",
            "accept-result"
        )
        .unwrap()
        .contains("accepted")
    );
    assert!(
        app.artifact_show(
            task_id,
            "run-001",
            "round-002",
            "accept",
            "attempt-001",
            "accept-result.json"
        )
        .unwrap()
        .contains("accepted")
    );

    let killed = app.run_kill(task_id, "run-001").unwrap();
    assert_eq!(killed.outcome, Some(gold_band::domain::RunOutcome::Killed));
}
