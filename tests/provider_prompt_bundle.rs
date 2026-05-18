use camino::Utf8PathBuf;
use gold_band::domain::{InvocationKind, SessionMode};
use gold_band::provider::{ColdFileRef, StreamMode, WorkerInvocation};

#[test]
fn worker_invocation_can_be_serialized_with_context_indexes() {
    let invocation = WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        profile: Some("developer".to_string()),
        profile_content: None,
        requirement_path: Some(Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/authoring/requirement.md",
        )),
        requirement_text: None,
        workspace_dir: Utf8PathBuf::from("/repo"),
        attempt_dir: Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001",
        ),
        primary_artifact: Some("exec-plan".to_string()),
        task_instruction: Some("Create an exec plan".to_string()),
        session_mode: SessionMode::New,
        continue_ref: None,
        resume_prompt: None,
        resume_prompt_id: None,
        stream_mode: StreamMode::None,
        log_prompts: false,
        log_provider_command: false,
        feedback_summary: None,
        verify_result_path: None,
        attachments_dir: Some(Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/.../attachments",
        )),
        cold_artifacts: vec![ColdFileRef {
            name: Some("exec-result".to_string()),
            path: Utf8PathBuf::from(
                "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/.../exec-result.json",
            ),
        }],
        cold_attachments: vec![ColdFileRef {
            name: None,
            path: Utf8PathBuf::from(
                "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/.../report.md",
            ),
        }],
    };

    let value = serde_json::to_value(invocation).unwrap();
    assert_eq!(value["primary_artifact"], "exec-plan");
    assert_eq!(value["cold_artifacts"][0]["name"], "exec-result");
}
