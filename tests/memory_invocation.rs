use camino::Utf8PathBuf;
use gold_band::domain::{InvocationKind, SessionMode, TurnControlMode};
use gold_band::prompts::PromptExecutionSurface;
use gold_band::provider::*;
use gold_band::runtime_error::RecoveryMode;

#[test]
fn memory_invocation_binding_covers_direct_workflow_auto_and_retry_refresh() {
    use gold_band::memory::{Entry, MemoryService, Scope, WriteCommand};
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = gold_band::storage::GoldBandPaths::new(root.clone());
    paths.provision_project_manifest().unwrap();
    gold_band::storage::write_json(
        &paths.task_file("task-001"),
        &serde_json::json!({"id":"task-001"}),
    )
    .unwrap();
    let service =
        MemoryService::new(paths.clone(), &paths.project_id, Some("task-001".into())).unwrap();
    service
        .write(WriteCommand {
            scope: Scope::Task,
            key: "plan".into(),
            expected_revision: None,
            entry: Some(Entry {
                key: "plan".into(),
                value: "B2".into(),
                desc: "build".into(),
            }),
        })
        .unwrap();
    let mut req = test_worker_invocation(root.join("attempt"));
    req.adapter_workspace_dir = root;
    req.runtime_context.project_id = paths.project_id.clone();
    req.mcp_servers = vec![serde_json::json!({
        "name": gold_band::memory::mcp::SERVER_NAME,
        "command": "gold-band",
        "args": [gold_band::memory::mcp::FLAG],
        "env": []
    })];
    for (surface, envelope) in [
        (
            PromptExecutionSurface::Workflow,
            gold_band::dsl::PromptEnvelopeMode::RawAgent,
        ),
        (
            PromptExecutionSurface::Workflow,
            gold_band::dsl::PromptEnvelopeMode::RuntimeManaged,
        ),
        (
            PromptExecutionSurface::AiDynamic,
            gold_band::dsl::PromptEnvelopeMode::RuntimeManaged,
        ),
    ] {
        req.execution_surface = surface;
        req.prompt_envelope = envelope;
        let rendered = gold_band::memory::prepare_invocation(&mut req)
            .unwrap()
            .expect("enabled memory MCP should render current memory");
        assert!(rendered.contains("B2"));
        assert!(
            !rendered.contains("memory_write"),
            "per-submission memory data must not repeat stable write rules"
        );
        for language in [
            gold_band::config::DesktopLanguage::ZhCn,
            gold_band::config::DesktopLanguage::En,
        ] {
            req.runtime_context.language = language;
            for mode in [SessionMode::New, SessionMode::Continue] {
                req.session_mode = mode;
                req.resume_prompt = Some("Continue the task".into());
                let prompt = prepare_prompt_bundle(&mut req).unwrap();
                assert!(prompt.system_prompt.contains("memory_write"));
                assert!(!prompt.system_prompt.contains("B2"));
                assert!(!prompt.system_prompt.contains(paths.runtime_root.as_str()));
                assert_eq!(prompt.user_prompt.matches("<memory-data>").count(), 1);
                assert_eq!(prompt.user_prompt.matches("B2").count(), 1);
                assert!(
                    prompt
                        .user_prompt
                        .contains("data-gold-band-hidden=\"true\"")
                );
                assert!(!prompt.user_prompt.contains("memory_write"));
            }
        }
        assert_eq!(req.mcp_servers.len(), 1);
        assert_eq!(req.mcp_servers[0]["args"].as_array().unwrap().len(), 2);
        assert_eq!(
            prepare_acp_mcp_servers(
                &req.mcp_servers,
                Some(&serde_json::json!({"mcpCapabilities":{"http":false,"sse":false}}))
            )
            .accepted
            .len(),
            1
        );
    }
    let before = prepare_prompt_bundle(&mut req).unwrap();
    let snapshot = service.read().unwrap();
    service
        .write(WriteCommand {
            scope: Scope::Task,
            key: "plan".into(),
            expected_revision: Some(snapshot.task[0].revision.clone()),
            entry: Some(Entry {
                key: "plan".into(),
                value: "B3".into(),
                desc: "build".into(),
            }),
        })
        .unwrap();
    req.session_mode = SessionMode::Continue;
    assert!(
        gold_band::memory::prepare_invocation(&mut req)
            .unwrap()
            .expect("enabled memory MCP should refresh current memory")
            .contains("B3")
    );
    let after = prepare_prompt_bundle(&mut req).unwrap();
    assert_eq!(before.system_prompt, after.system_prompt);
    assert!(after.user_prompt.contains("B3"));
    assert!(!after.user_prompt.contains("B2"));
    assert_eq!(after.user_prompt.matches("<memory-data>").count(), 1);
    std::fs::write(snapshot.task_path.unwrap(), "{broken").unwrap();
    let error = prepare_prompt_bundle(&mut req).unwrap_err();
    let info = gold_band::runtime_error::normalize_runtime_error(&error);
    assert_eq!(info.code_str(), "memory.corrupt");
    assert_eq!(info.recovery, RecoveryMode::Manual);
}

#[test]
fn disabled_memory_mcp_omits_memory_prompt_and_session_binding() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = gold_band::storage::GoldBandPaths::new(root.clone());
    paths.provision_project_manifest().unwrap();
    gold_band::storage::write_json(
        &paths.task_file("task-001"),
        &serde_json::json!({"id":"task-001"}),
    )
    .unwrap();
    let mut req = test_worker_invocation(root.join("attempt"));
    req.adapter_workspace_dir = root;
    req.runtime_context.project_id = paths.project_id;

    let prompt = prepare_prompt_bundle(&mut req).unwrap();

    assert!(req.mcp_servers.is_empty());
    assert!(!prompt.system_prompt.contains("memory_write"));
    assert!(!prompt.user_prompt.contains("<memory-data>"));
}

fn test_worker_invocation(attempt_dir: Utf8PathBuf) -> WorkerInvocation {
    let runtime_context = PromptRuntimeContext {
        project_id: "project-001".to_string(),
        task_id: "task-001".to_string(),
        run_id: "run-001".to_string(),
        round_id: "round-001".to_string(),
        node_id: "dev".to_string(),
        attempt_id: "attempt-001".to_string(),
        runtime_node_id: None,
        runtime_attempt_id: None,
        attempt_state_file: None,
        language: gold_band::config::DesktopLanguage::ZhCn,
        run_dir: attempt_dir.join("../../.."),
        round_dir: attempt_dir.join("../.."),
        node_dir: attempt_dir.join(".."),
        attempt_dir: attempt_dir.clone(),
        attachments_dir: attempt_dir.join("attachments"),
        task_inputs_dir: None,
    };
    WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        turn_control_mode: TurnControlMode::RuntimeControlled,
        runtime_control_intent: RuntimeControlIntent::Unchanged,
        prompt_envelope: gold_band::dsl::PromptEnvelopeMode::RuntimeManaged,
        execution_surface: PromptExecutionSurface::Workflow,
        profile: None,
        profile_content: None,
        profile_dynamic_template: false,
        requirement_path: None,
        requirement_text: Some("Need a structured result".to_string()),
        adapter_workspace_dir: Utf8PathBuf::from("/repo"),
        workspace_dir: Utf8PathBuf::from("/repo"),
        attempt_dir,
        output_contract: None,
        runtime_context,
        predecessors: Vec::new(),
        new_round_trigger: None,
        extra_system_sections: Vec::new(),
        extra_hidden_sections: Vec::new(),
        task_instruction: Some("Create a structured result".to_string()),
        user_tips_instruction: None,
        resume_task_instruction: None,
        session_mode: SessionMode::New,
        user_prompt_render_mode: UserPromptRenderMode::RequirementTask,
        permission_mode: None,
        auto_accept: false,
        model: None,
        config_options: Default::default(),
        continue_ref: None,
        resume_prompt: None,
        resume_prompt_id: None,
        prompt_display: None,
        resume_prompt_visibility: PromptVisibility::Visible,
        stream_mode: StreamMode::StreamJson,
        log_prompts: false,
        log_provider_command: false,
        attachments_dir: None,
        cold_artifacts: Vec::new(),
        cold_attachments: Vec::new(),
        task_input_attachment_paths: Vec::new(),
        user_input_attachment_paths: Vec::new(),
        attachment_projection_policy: AttachmentProjectionPolicy::from(
            &gold_band::config::RuntimeConfig::default(),
        ),
        mcp_servers: Vec::new(),
        scheduled_context: None,
    }
}
