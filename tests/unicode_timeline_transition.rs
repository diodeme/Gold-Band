use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::domain::SessionMode;
use gold_band::provider::{
    DoctorResult, ProviderAdapter, ProviderCapabilities, ProviderInfo, ProviderResultPayload,
    ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
};
use gold_band::runtime::RunState;
use tempfile::tempdir;

#[derive(Clone, Default)]
struct UnicodeTimelineProvider;

impl ProviderAdapter for UnicodeTimelineProvider {
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
        let timeline_path = req.attempt_dir.join("acp.timeline.jsonl");
        let prefix = r#"{"item":{"kind":"message","text":""#;
        let unicode_payload = "你".repeat(120);
        let suffix = r#""}}"#;
        let mut line = format!("{prefix}{unicode_payload}{suffix}");
        let mut ascii_pad = String::new();
        while line.is_char_boundary(200) {
            ascii_pad.push('a');
            line = format!("{prefix}{ascii_pad}{unicode_payload}{suffix}");
        }
        std::fs::write(timeline_path.as_std_path(), format!("{line}\n"))?;
        std::fs::write(
            req.attempt_dir.join("acp.snapshot.json").as_std_path(),
            r#"{"inputTokens":1,"outputTokens":2,"cachedReadTokens":0,"totalTokens":3}"#,
        )?;

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                output_artifact: None,
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
fn run_start_transitions_past_completed_worker_with_unicode_timeline() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let gold_band_home = repo_root.join("gold-band-home");
    unsafe { std::env::set_var("GOLD_BAND_HOME", gold_band_home.as_str()) };

    let app = App::with_provider(repo_root.clone(), Box::new(UnicodeTimelineProvider));
    let task_id = "task-001";

    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    let profiles = app.profiles().unwrap();
    let dev_profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id
        .clone();
    let review_profile = profiles
        .profiles
        .iter()
        .find(|profile| profile.name == "审查")
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
          "id": "unicode-transition",
          "entry": "dev",
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-acp","profile":"{}","goal":"Implement the requirement"}},
            {{"id":"review","type":"worker","provider":"claude-acp","profile":"{}","goal":"Review the implementation"}}
          ],
          "edges": [
            {{"from":"dev","to":"review","on":"success"}},
            {{"from":"review","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile, review_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        r#"{"version":"0.1","id":"task-001"}"#,
    )
    .unwrap();

    let run = app.run_start(task_id, None).unwrap();
    let run_state: RunState =
        gold_band::storage::read_json(&app.paths.run_file(task_id, &run.id)).unwrap();

    assert_eq!(run_state.status, gold_band::domain::RunStatus::Completed);
    assert_eq!(
        run_state.outcome,
        Some(gold_band::domain::RunOutcome::Success)
    );
    assert_eq!(run_state.current_node.as_deref(), Some("review"));
}
