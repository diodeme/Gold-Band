use camino::Utf8PathBuf;
use gold_band::domain::{InvocationKind, SessionMode, TurnControlMode};
use gold_band::prompts::PromptExecutionSurface;
use gold_band::provider::*;

fn invocation_with_memory(
    value: &str,
) -> (
    tempfile::TempDir,
    gold_band::storage::GoldBandPaths,
    WorkerInvocation,
) {
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
                value: value.into(),
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
    (temp, paths, req)
}

fn assert_memory_mcp_bound(req: &WorkerInvocation, project_id: &str) {
    assert_eq!(req.mcp_servers.len(), 1);
    assert_eq!(
        req.mcp_servers[0]["name"],
        gold_band::memory::mcp::SERVER_NAME
    );
    let args = req.mcp_servers[0]["args"].as_array().unwrap();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], gold_band::memory::mcp::FLAG);
    let binding: serde_json::Value = serde_json::from_str(args[1].as_str().unwrap()).unwrap();
    assert_eq!(binding["project_id"], project_id);
    assert_eq!(binding["task_id"], "task-001");
}

fn assert_no_memory_projection(text: &str, saved_value: &str, runtime_root: &str) {
    assert!(!text.contains("Gold Band current memory"));
    assert!(!text.contains("<memory-data>"));
    assert!(!text.contains(saved_value));
    assert!(!text.contains(runtime_root));
    assert!(!text.contains("memory.json"));
    assert!(!text.contains("\"revision\""));
}

#[test]
fn bind_invocation_mcp_reports_whether_enabled_memory_was_bound() {
    let (_temp, paths, mut req) = invocation_with_memory("B2-confirmed-value");

    assert!(gold_band::memory::bind_invocation_mcp(&mut req).unwrap());
    assert_memory_mcp_bound(&req, &paths.project_id);

    req.mcp_servers.clear();
    assert!(!gold_band::memory::bind_invocation_mcp(&mut req).unwrap());
    assert!(req.mcp_servers.is_empty());
}

#[test]
fn runtime_managed_binds_memory_and_receives_only_generic_rules() {
    let (_temp, paths, mut req) = invocation_with_memory("B2-confirmed-value");

    let prompt = prepare_prompt_bundle(&mut req).unwrap();

    assert_memory_mcp_bound(&req, &paths.project_id);
    assert!(prompt.system_prompt.contains("memory_read"));
    assert!(prompt.system_prompt.contains("memory_write"));
    assert!(prompt.system_prompt.contains("角色契约"));
    assert_no_memory_projection(
        &prompt.user_prompt,
        "B2-confirmed-value",
        paths.runtime_root.as_str(),
    );
    assert!(prompt.user_prompt.contains("Need a structured result"));
    assert!(prompt.user_prompt.contains("Create a structured result"));
}

#[test]
fn raw_agent_preserves_exact_prompts_and_memory_binding_without_memory_prompt() {
    let (_temp, paths, mut req) = invocation_with_memory("B2-confirmed-value");
    req.prompt_envelope = gold_band::dsl::PromptEnvelopeMode::RawAgent;
    req.requirement_text = Some("  第一轮用户原文\nsecond line  ".to_string());

    for language in [
        gold_band::config::DesktopLanguage::ZhCn,
        gold_band::config::DesktopLanguage::En,
    ] {
        req.runtime_context.language = language;
        let original = req.requirement_text.clone().unwrap();
        let prompt = prepare_prompt_bundle(&mut req).unwrap();

        assert!(prompt.system_prompt.is_empty());
        assert_eq!(prompt.user_prompt, original);
        assert_no_memory_projection(
            &prompt.system_prompt,
            "B2-confirmed-value",
            paths.runtime_root.as_str(),
        );
        assert_no_memory_projection(
            &prompt.user_prompt,
            "B2-confirmed-value",
            paths.runtime_root.as_str(),
        );
        assert!(!prompt.system_prompt.contains("memory_read"));
        assert!(!prompt.system_prompt.contains("memory_write"));
        assert_memory_mcp_bound(&req, &paths.project_id);
    }

    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
    let follow_up = "\n  本轮追问原文  ".to_string();
    req.resume_prompt = Some(follow_up.clone());
    let prompt = prepare_prompt_bundle(&mut req).unwrap();

    assert!(prompt.system_prompt.is_empty());
    assert_eq!(prompt.user_prompt, follow_up);
    assert_no_memory_projection(
        &prompt.system_prompt,
        "B2-confirmed-value",
        paths.runtime_root.as_str(),
    );
    assert_no_memory_projection(
        &prompt.user_prompt,
        "B2-confirmed-value",
        paths.runtime_root.as_str(),
    );
    assert_memory_mcp_bound(&req, &paths.project_id);
}

#[test]
fn disabled_memory_mcp_omits_rules_data_and_session_binding() {
    let (_temp, _paths, mut req) = invocation_with_memory("B2-confirmed-value");
    req.mcp_servers.clear();

    let runtime_prompt = prepare_prompt_bundle(&mut req).unwrap();
    assert!(req.mcp_servers.is_empty());
    assert!(!runtime_prompt.system_prompt.contains("memory_read"));
    assert!(!runtime_prompt.system_prompt.contains("memory_write"));
    assert!(!runtime_prompt.user_prompt.contains("<memory-data>"));

    req.prompt_envelope = gold_band::dsl::PromptEnvelopeMode::RawAgent;
    req.requirement_text = Some("raw original".to_string());
    let raw_prompt = prepare_prompt_bundle(&mut req).unwrap();
    assert!(req.mcp_servers.is_empty());
    assert!(raw_prompt.system_prompt.is_empty());
    assert_eq!(raw_prompt.user_prompt, "raw original");
}

#[test]
fn corrupt_memory_does_not_block_runtime_prompt_preparation() {
    let (_temp, paths, mut req) = invocation_with_memory("B2-confirmed-value");
    let task_memory = paths.task_dir("task-001").join("memory.json");
    std::fs::write(&task_memory, "{broken").unwrap();

    let prompt = prepare_prompt_bundle(&mut req).unwrap();

    assert_memory_mcp_bound(&req, &paths.project_id);
    assert!(prompt.system_prompt.contains("memory_read"));
    assert_no_memory_projection(
        &prompt.user_prompt,
        "B2-confirmed-value",
        paths.runtime_root.as_str(),
    );
    assert_eq!(std::fs::read_to_string(task_memory).unwrap(), "{broken");
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
        automatic_prompt_retry: false,
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
        workspace_file_roots: Vec::new(),
    }
}
