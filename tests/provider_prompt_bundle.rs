use camino::Utf8PathBuf;
use gold_band::domain::{InvocationKind, SessionMode, TurnControlMode};
use gold_band::prompts::PromptExecutionSurface;
use gold_band::provider::{
    ColdFileRef, ConversationPromptInput, OutputEmissionMode, PromptArtifactRef,
    PromptAttachmentRef, PromptHiddenSection, PromptOutputContract, PromptPredecessorContext,
    PromptRuntimeContext, PromptVisibility, RuntimeControlIntent, StreamMode, UserPromptQuote,
    UserPromptRenderMode, WorkerInvocation, render_prompt_bundle,
};

fn runtime_context() -> PromptRuntimeContext {
    PromptRuntimeContext {
        project_id: "D--Projects-code-ai-Gold-Band".to_string(),
        task_id: "task-001".to_string(),
        run_id: "run-001".to_string(),
        round_id: "round-001".to_string(),
        node_id: "dev".to_string(),
        attempt_id: "attempt-001".to_string(),
        runtime_node_id: None,
        runtime_attempt_id: None,
        attempt_state_file: None,
        language: gold_band::config::DesktopLanguage::ZhCn,
        run_dir: Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/runs/run-001",
        ),
        round_dir: Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/runs/run-001/rounds/round-001",
        ),
        node_dir: Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev",
        ),
        attempt_dir: Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001",
        ),
        attachments_dir: Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/attachments",
        ),
        task_inputs_dir: None,
    }
}

fn invocation() -> WorkerInvocation {
    WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        turn_control_mode: TurnControlMode::RuntimeControlled,
        runtime_control_intent: RuntimeControlIntent::Unchanged,
        prompt_envelope: gold_band::dsl::PromptEnvelopeMode::RuntimeManaged,
        execution_surface: PromptExecutionSurface::Workflow,
        profile: Some("developer".to_string()),
        profile_content: Some("你是负责实现当前节点的开发角色。".to_string()),
        profile_dynamic_template: false,
        requirement_path: None,
        requirement_text: Some("Need an implementation".to_string()),
        adapter_workspace_dir: Utf8PathBuf::from("/repo"),
        workspace_dir: Utf8PathBuf::from("/repo"),
        attempt_dir: runtime_context().attempt_dir,
        output_contract: Some(PromptOutputContract {
            artifact: "dev-result".to_string(),
            kind: "json".to_string(),
            schema: Some(serde_json::json!({
                "result": "boolean",
                "reason": "string"
            })),
            schema_text: None,
            success_condition: Some("JSON field `$.result` equals `true`".to_string()),
            finalize_context: None,
            emission_mode: OutputEmissionMode::PostTurnProjection,
        }),
        runtime_context: runtime_context(),
        predecessors: vec![PromptPredecessorContext {
            round_id: "round-001".to_string(),
            node_id: "plan".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_type: "worker".to_string(),
            branch_kind: "节点输出检查".to_string(),
            outcome: Some("success".to_string()),
            branch_direction: Some("success".to_string()),
            output_artifact: Some(PromptArtifactRef {
                name: "plan-result".to_string(),
                path: Utf8PathBuf::from(
                    "/run/rounds/round-001/nodes/plan/attempt-001/artifacts/plan-result.json",
                ),
                preview: Some("{\"result\":true}".to_string()),
            }),
            branch_reason: None,
            attachments: Vec::new(),
        }],
        new_round_trigger: None,
        extra_system_sections: Vec::new(),
        extra_hidden_sections: Vec::new(),
        task_instruction: Some("Implement the requested change".to_string()),
        user_tips_instruction: None,
        resume_task_instruction: None,
        session_mode: SessionMode::New,
        user_prompt_render_mode: UserPromptRenderMode::RequirementTask,
        permission_mode: None,
        model: None,
        config_options: Default::default(),
        continue_ref: None,
        resume_prompt: None,
        resume_prompt_id: None,
        prompt_display: None,
        resume_prompt_visibility: PromptVisibility::Visible,
        stream_mode: StreamMode::None,
        log_prompts: false,
        log_provider_command: false,
        attachments_dir: Some(Utf8PathBuf::from(
            "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/.../attachments",
        )),
        cold_artifacts: vec![ColdFileRef {
            name: Some("review-result".to_string()),
            path: Utf8PathBuf::from(
                "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/.../review-result.json",
            ),
        }],
        cold_attachments: vec![ColdFileRef {
            name: None,
            path: Utf8PathBuf::from(
                "~/.gold-band/projects/D--Projects-code-ai-Gold-Band/.../report.md",
            ),
        }],
        task_input_attachment_paths: Vec::new(),
        user_input_attachment_paths: Vec::new(),
        attachment_projection_policy: gold_band::provider::AttachmentProjectionPolicy::from(
            &gold_band::config::RuntimeConfig::default(),
        ),
        mcp_servers: Vec::new(),
        scheduled_context: None,
    }
}

#[test]
fn worker_invocation_can_be_serialized_with_context_indexes() {
    let value = serde_json::to_value(invocation()).unwrap();
    assert_eq!(value["execution_surface"], "workflow");
    assert_eq!(value["output_contract"]["artifact"], "dev-result");
    assert_eq!(value["runtime_context"]["task_id"], "task-001");
    assert_eq!(value["cold_artifacts"][0]["name"], "review-result");
}

#[test]
fn worker_invocation_serializes_ai_dynamic_surface_as_camel_case() {
    let mut req = invocation();
    req.execution_surface = PromptExecutionSurface::AiDynamic;

    let value = serde_json::to_value(req).unwrap();

    assert_eq!(value["execution_surface"], "aiDynamic");
}

#[test]
fn render_prompt_bundle_uses_runtime_context_without_old_invocation_labels() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(
        prompt
            .system_prompt
            .contains("Project: D--Projects-code-ai-Gold-Band")
    );
    assert!(prompt.system_prompt.contains("Task: task-001"));
    assert!(prompt.system_prompt.contains("Run: run-001"));
    assert!(prompt.system_prompt.contains("Node: dev"));
    assert!(!prompt.system_prompt.contains("Round: round-001"));
    assert!(!prompt.system_prompt.contains("Attempt: attempt-001"));
    assert!(prompt.user_prompt.contains("会话模式: new"));
    assert!(prompt.user_prompt.contains("- Round: round-001"));
    assert!(prompt.user_prompt.contains("- Attempt: attempt-001"));
    assert!(!prompt.system_prompt.contains("Invocation kind"));
    assert!(!prompt.system_prompt.contains("WorkerGeneric"));
}

#[test]
fn render_prompt_bundle_routes_free_outputs_to_attachments_dir() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(
        prompt
            .system_prompt
            .contains("不要直接在 attempt 根目录写入你创建的文件")
    );
    assert!(
        prompt
            .system_prompt
            .contains("节点过程输出包括但不限于：报告、记录、临时脚本、验证脚本")
    );
    assert!(prompt.system_prompt.contains("默认写入 attachments 目录"));
    assert!(
        prompt
            .user_prompt
            .contains("附件目录（本节点报告、临时脚本、过程记录等自由输出默认写入这里）")
    );
}

#[test]
fn render_prompt_bundle_routes_english_free_outputs_to_attachments_dir() {
    let mut req = invocation();
    req.runtime_context.language = gold_band::config::DesktopLanguage::En;

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(
        prompt
            .system_prompt
            .contains("Do not write files you create directly into the attempt root")
    );
    assert!(prompt.system_prompt.contains(
        "Node process outputs include, but are not limited to: reports, records, temporary scripts"
    ));
    assert!(
        prompt
            .system_prompt
            .contains("write it to the attachments directory by default")
    );
    assert!(
        prompt
            .user_prompt
            .contains("Attachments directory (default location for this node's reports, temporary scripts, process notes, and other free-form outputs)")
    );
}

#[test]
fn render_prompt_bundle_guides_nodes_without_artifacts() {
    let mut req = invocation();
    req.output_contract = None;
    req.predecessors.clear();

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(
        prompt
            .system_prompt
            .contains("当前节点未声明 output DSL，不需要产出 canonical artifact")
    );
    assert!(
        prompt
            .system_prompt
            .contains("不需要查找、推断或读取 artifact/output 约束")
    );
    assert!(
        prompt
            .system_prompt
            .contains("当前节点所需上下文已在本 prompt 中给出")
    );
    assert!(
        prompt
            .system_prompt
            .contains("如需查阅前序节点产出，只读取本 prompt 明确给出的前序产出路径")
    );
    assert!(
        prompt
            .system_prompt
            .contains("当前 run 目录仅作为本 prompt 明确给出路径的父级上下文")
    );
    assert!(
        prompt
            .system_prompt
            .contains("不要主动扫描 run 目录来寻找未声明产物、理解当前任务或确认输出约束")
    );
}

#[test]
fn render_prompt_bundle_marks_new_round_transitions() {
    let mut req = invocation();
    req.runtime_context.round_id = "round-002".to_string();
    req.runtime_context.node_id = "plan".to_string();
    req.runtime_context.attempt_id = "attempt-001".to_string();
    req.predecessors = vec![PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "accept".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "节点输出检查".to_string(),
        outcome: Some("failure".to_string()),
        branch_direction: None,
        output_artifact: Some(PromptArtifactRef {
            name: "accept-result".to_string(),
            path: Utf8PathBuf::from(
                "/run/rounds/round-001/nodes/accept/attempt-001/artifacts/accept-result.json",
            ),
            preview: Some("{\"result\":false}".to_string()),
        }),
        branch_reason: None,
        attachments: Vec::new(),
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.user_prompt.contains(
        "round-001/accept/attempt-001 -$new-round-> 当前节点(round-002/plan/attempt-001)"
    ));
    assert!(prompt.user_prompt.contains("输出 artifact=accept-result"));
    assert!(prompt.user_prompt.contains("输出预览={\"result\":false}"));
    assert!(!prompt.system_prompt.contains("输出 artifact=accept-result"));
}

#[test]
fn render_prompt_bundle_removes_skill_catalog_and_cold_indexes() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(!prompt.system_prompt.contains("skill_catalog"));
    assert!(!prompt.system_prompt.contains("Agent Skills"));
    assert!(!prompt.user_prompt.contains("Cold Artifact Index"));
    assert!(!prompt.user_prompt.contains("Cold Attachment Index"));
    assert!(!prompt.user_prompt.contains("review-result"));
    assert!(!prompt.user_prompt.contains("report.md"));
}

#[test]
fn render_prompt_bundle_keeps_extra_system_sections_in_system_prompt() {
    let mut req = invocation();
    req.extra_system_sections = vec!["AI dynamic rule".to_string()];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.system_prompt.contains("AI dynamic rule"));
    assert!(!prompt.user_prompt.contains("AI dynamic rule"));
}

#[test]
fn render_prompt_bundle_moves_profile_content_to_system_prompt() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(
        prompt
            .system_prompt
            .contains("你是负责实现当前节点的开发角色")
    );
    assert!(
        !prompt
            .user_prompt
            .contains("你是负责实现当前节点的开发角色")
    );
}

#[test]
fn render_prompt_bundle_keeps_disabled_profile_templates_literal() {
    let mut req = invocation();
    req.profile_content = Some(
        "{% if execution.surface == \"workflow\" %}workflow{% else %}dynamic{% endif %}"
            .to_string(),
    );

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.system_prompt.contains("{% if execution.surface"));
    assert!(prompt.system_prompt.contains("workflow{% else %}dynamic"));
}

#[test]
fn render_prompt_bundle_renders_enabled_profile_for_workflow_surface() {
    let mut req = invocation();
    req.profile_dynamic_template = true;
    req.runtime_context.runtime_node_id = Some("identity-must-not-select-surface".to_string());
    req.profile_content = Some(
        "{% if execution.surface == \"workflow\" %}workflow-role{% else %}dynamic-role{% endif %}"
            .to_string(),
    );

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.system_prompt.contains("workflow-role"));
    assert!(!prompt.system_prompt.contains("dynamic-role"));
    assert!(!prompt.system_prompt.contains("{% if"));
}

#[test]
fn render_prompt_bundle_defers_dynamic_profile_routing_for_post_turn_projection() {
    let mut req = invocation();
    req.profile_dynamic_template = true;
    req.execution_surface = PromptExecutionSurface::AiDynamic;
    req.profile_content = Some(
        "{% if execution.can_route_next %}route-next{% else %}stop-here{% endif %}".to_string(),
    );

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(!prompt.system_prompt.contains("route-next"));
    assert!(prompt.system_prompt.contains("stop-here"));
}

#[test]
fn render_prompt_bundle_rejects_unknown_enabled_profile_template_variables() {
    let mut req = invocation();
    req.profile_dynamic_template = true;
    req.profile_content = Some("{{ execution.unknown }}".to_string());

    let error = render_prompt_bundle(&req).unwrap_err();

    assert!(!error.to_string().is_empty());
}

#[test]
fn render_prompt_bundle_renders_predecessor_chain_and_defers_output_dsl() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(
        prompt
            .user_prompt
            .contains("round-001/plan/attempt-001 -success-> 当前节点(round-001/dev/attempt-001)")
    );
    assert!(prompt.user_prompt.contains("节点输出检查"));
    assert!(prompt.user_prompt.contains("plan-result"));
    assert!(!prompt.system_prompt.contains("plan-result"));
    assert!(
        prompt
            .system_prompt
            .contains("当前业务执行 turn 不需要输出 canonical artifact")
    );
    assert!(!prompt.system_prompt.contains("\"result\": \"boolean\""));
    assert!(!prompt.system_prompt.contains("\"reason\": \"string\""));
    assert!(
        !prompt
            .system_prompt
            .contains("JSON field `$.result` equals `true`")
    );
}

#[test]
fn render_post_turn_control_prompts_keep_contract_out_of_system_prompt() {
    let business_prompt = render_prompt_bundle(&invocation()).unwrap();
    let control_protocol = "artifact: dev-result\nCONTROL_SCHEMA_MARKER\n$.result == true";

    for (render_mode, hidden_reason) in [
        (UserPromptRenderMode::RuntimeFinalize, "artifactFinalize"),
        (UserPromptRenderMode::RuntimeRepair, "invalidOutputRepair"),
    ] {
        let mut req = invocation();
        req.session_mode = SessionMode::Continue;
        req.user_prompt_render_mode = render_mode;
        req.resume_prompt_visibility = PromptVisibility::Hidden;
        req.resume_prompt = Some(control_protocol.to_string());

        let prompt = render_prompt_bundle(&req).unwrap();

        assert_eq!(prompt.visibility, PromptVisibility::Hidden);
        assert_eq!(prompt.hidden_reason.as_deref(), Some(hidden_reason));
        assert_eq!(prompt.system_prompt, business_prompt.system_prompt);
        assert!(!prompt.system_prompt.contains("CONTROL_SCHEMA_MARKER"));
        assert_eq!(prompt.user_prompt, control_protocol);
    }
}

#[test]
fn render_prompt_bundle_workflow_resume_uses_hidden_context_and_goal() {
    let mut req = invocation();
    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::WorkflowResume;
    req.resume_prompt = Some("继续".to_string());
    req.resume_prompt_id = Some("resume-001".to_string());

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(
        prompt
            .system_prompt
            .contains("Project: D--Projects-code-ai-Gold-Band")
    );
    assert!(
        prompt
            .system_prompt
            .contains("当前业务执行 turn 不需要输出 canonical artifact")
    );
    assert!(!prompt.system_prompt.contains("\"result\": \"boolean\""));
    assert!(
        !prompt
            .system_prompt
            .contains("JSON field `$.result` equals `true`")
    );
    assert!(
        prompt.user_prompt.contains(
            "<hidden data-gold-band-hidden=\"true\" title=\"Gold Band runtime context\">"
        )
    );
    assert!(prompt.user_prompt.contains("会话模式: continue"));
    assert!(prompt.user_prompt.contains("调用原因"));
    assert!(prompt.user_prompt.contains("继续"));
    assert!(prompt.user_prompt.contains("# 目标"));
    assert!(prompt.user_prompt.contains("根据最新反馈进行调整"));
    assert!(!prompt.user_prompt.contains("# 需求"));
    assert_eq!(prompt.prompt_id.as_deref(), Some("resume-001"));
}

#[test]
fn render_prompt_bundle_renders_user_tips_as_separate_section() {
    let mut req = invocation();
    req.user_tips_instruction = Some("先做 A，再做 B。".to_string());

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.user_prompt.contains("# 用户提示\n先做 A，再做 B。"));
    assert!(
        prompt
            .user_prompt
            .contains("# 任务\nImplement the requested change")
    );
    let task_section = prompt
        .user_prompt
        .rsplit_once("# 任务")
        .map(|(_, task)| task)
        .unwrap_or(&prompt.user_prompt);
    assert!(!task_section.contains("先做 A，再做 B。"));
}

#[test]
fn render_prompt_bundle_user_message_sends_user_text_without_hidden_context() {
    let mut req = invocation();
    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
    req.resume_prompt = Some("请继续检查这个会话".to_string());
    req.resume_prompt_id = Some("resume-user-001".to_string());

    let prompt = render_prompt_bundle(&req).unwrap();

    assert_eq!(prompt.user_prompt, "请继续检查这个会话");
    assert!(!prompt.user_prompt.contains("data-gold-band-hidden"));
    assert!(!prompt.user_prompt.contains("# 目标"));
    assert!(!prompt.user_prompt.contains("# 需求"));
    assert_eq!(prompt.prompt_id.as_deref(), Some("resume-user-001"));
}

#[test]
fn render_non_runtime_message_keeps_contract_but_adds_suspension_context() {
    let mut req = invocation();
    req.output_contract.as_mut().unwrap().emission_mode = OutputEmissionMode::InlineControl;
    req.turn_control_mode = TurnControlMode::NonRuntimeControlled;
    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
    req.resume_prompt = Some("先解释一下刚才的选择".to_string());
    req.extra_hidden_sections = vec![PromptHiddenSection {
        title: "Gold Band runtime context".to_string(),
        content: "当前工作流已停止；无需遵循 artifact 契约。".to_string(),
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert_eq!(
        prompt.turn_control_mode,
        TurnControlMode::NonRuntimeControlled
    );
    assert!(
        prompt
            .system_prompt
            .contains("你必须在最后一步按照以下格式输出你的结果")
    );
    assert!(prompt.user_prompt.starts_with("先解释一下刚才的选择"));
    assert!(prompt.user_prompt.contains("Gold Band runtime context"));
    assert!(prompt.user_prompt.contains("无需遵循 artifact 契约"));
}

#[test]
fn render_runtime_resume_is_a_hidden_control_turn() {
    let mut req = invocation();
    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::RuntimeResume;
    req.resume_prompt = Some("用户已选择继续工作流。".to_string());
    req.resume_prompt_visibility = PromptVisibility::Hidden;

    let prompt = render_prompt_bundle(&req).unwrap();

    assert_eq!(prompt.visibility, PromptVisibility::Hidden);
    assert_eq!(
        prompt.hidden_reason.as_deref(),
        Some("runtimeControlResume")
    );
    assert!(prompt.user_prompt.contains("用户已选择继续工作流"));
}

#[test]
fn render_runtime_resume_with_message_keeps_internal_prompt_out_of_display_projection() {
    let mut req = invocation();
    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::UserMessage;
    req.runtime_control_intent = RuntimeControlIntent::Resume;
    req.resume_prompt = Some(
        "请继续检查\n\n<hidden data-gold-band-hidden=\"true\" show=\"false\" title=\"Gold Band runtime control\">resume</hidden>"
            .to_string(),
    );
    req.resume_prompt_visibility = PromptVisibility::Visible;
    req.prompt_display = Some(ConversationPromptInput {
        display_text: "请继续检查".to_string(),
        quotes: vec![UserPromptQuote {
            id: "quote-1".to_string(),
            source_message_key: "answer-1".to_string(),
            text: "引用内容".to_string(),
        }],
    });

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.user_prompt.contains("show=\"false\""));
    assert_eq!(prompt.display_text.as_deref(), Some("请继续检查"));
    assert_eq!(prompt.quotes.len(), 1);
    assert_eq!(prompt.visibility, PromptVisibility::Visible);
    assert_eq!(prompt.runtime_control_intent, RuntimeControlIntent::Resume);
}

#[test]
fn render_prompt_bundle_runtime_repair_sends_repair_prompt_without_hidden_context() {
    let mut req = invocation();
    req.session_mode = SessionMode::Continue;
    req.user_prompt_render_mode = UserPromptRenderMode::RuntimeRepair;
    req.resume_prompt = Some("请修复刚才输出的 JSON。".to_string());
    req.resume_prompt_visibility = PromptVisibility::Hidden;

    let prompt = render_prompt_bundle(&req).unwrap();

    assert_eq!(prompt.user_prompt, "请修复刚才输出的 JSON。");
    assert_eq!(prompt.visibility, PromptVisibility::Hidden);
    assert!(!prompt.user_prompt.contains("data-gold-band-hidden"));
    assert!(!prompt.user_prompt.contains("# 目标"));
    assert!(!prompt.user_prompt.contains("# 需求"));
}

#[test]
fn render_prompt_bundle_shows_predecessor_attachments() {
    let mut req = invocation();
    req.predecessors = vec![PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "dev".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "普通".to_string(),
        outcome: Some("success".to_string()),
        branch_direction: Some("success".to_string()),
        output_artifact: None,
        branch_reason: None,
        attachments: vec![PromptAttachmentRef {
            name: "dev-report.md".to_string(),
        }],
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.user_prompt.contains("## 最新前序附件"));
    assert!(
        prompt
            .user_prompt
            .contains("- round-001/dev/attempt-001: attachments/dev-report.md")
    );
}

#[test]
fn render_prompt_bundle_shows_multi_file_attachments() {
    let mut req = invocation();
    req.predecessors = vec![PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "dev".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "普通".to_string(),
        outcome: Some("success".to_string()),
        branch_direction: Some("success".to_string()),
        output_artifact: None,
        branch_reason: None,
        attachments: vec![
            PromptAttachmentRef {
                name: "a.md".to_string(),
            },
            PromptAttachmentRef {
                name: "b.md".to_string(),
            },
        ],
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(
        prompt
            .user_prompt
            .contains("- round-001/dev/attempt-001: attachments/a.md, attachments/b.md")
    );
}

#[test]
fn render_prompt_bundle_shows_reflow_attachments() {
    let mut req = invocation();
    req.predecessors = vec![
        PromptPredecessorContext {
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_type: "worker".to_string(),
            branch_kind: "节点输出检查".to_string(),
            outcome: Some("failure".to_string()),
            branch_direction: Some("failure".to_string()),
            output_artifact: None,
            branch_reason: None,
            attachments: vec![PromptAttachmentRef {
                name: "dev-report.md".to_string(),
            }],
        },
        PromptPredecessorContext {
            round_id: "round-001".to_string(),
            node_id: "review".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_type: "worker".to_string(),
            branch_kind: "节点输出检查".to_string(),
            outcome: Some("failure".to_string()),
            branch_direction: Some("failure".to_string()),
            output_artifact: None,
            branch_reason: None,
            attachments: vec![PromptAttachmentRef {
                name: "review-result.md".to_string(),
            }],
        },
        PromptPredecessorContext {
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-002".to_string(),
            node_type: "worker".to_string(),
            branch_kind: "节点输出检查".to_string(),
            outcome: Some("success".to_string()),
            branch_direction: Some("success".to_string()),
            output_artifact: None,
            branch_reason: None,
            attachments: vec![PromptAttachmentRef {
                name: "dev-report.md".to_string(),
            }],
        },
    ];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(
        prompt
            .user_prompt
            .contains("- round-001/dev/attempt-001: attachments/dev-report.md")
    );
    assert!(
        prompt
            .user_prompt
            .contains("- round-001/dev/attempt-002: attachments/dev-report.md")
    );
    assert!(
        prompt
            .user_prompt
            .contains("- round-001/review/attempt-001: attachments/review-result.md")
    );
}

#[test]
fn render_prompt_bundle_shows_new_round_trigger_reason() {
    let mut req = invocation();
    req.runtime_context.round_id = "round-002".to_string();
    req.runtime_context.node_id = "dev".to_string();
    req.predecessors = vec![PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "plan".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "普通".to_string(),
        outcome: Some("success".to_string()),
        branch_direction: None,
        output_artifact: None,
        branch_reason: None,
        attachments: vec![PromptAttachmentRef {
            name: "tech-plan.md".to_string(),
        }],
    }];
    req.new_round_trigger = Some(PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "accept".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "节点输出检查".to_string(),
        outcome: Some("failure".to_string()),
        branch_direction: Some("$new-round".to_string()),
        output_artifact: Some(PromptArtifactRef {
            name: "accept-result".to_string(),
            path: Utf8PathBuf::from(
                "/run/rounds/round-001/nodes/accept/attempt-001/artifacts/accept-result.json",
            ),
            preview: Some(r#"{"result":false,"reason":"needs another round"}"#.to_string()),
        }),
        branch_reason: None,
        attachments: vec![PromptAttachmentRef {
            name: "accept-report.md".to_string(),
        }],
    });

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.user_prompt.contains("## 最新前序流转原因"));
    assert!(prompt.user_prompt.contains("$new-round 由该节点触发"));
    assert!(prompt.user_prompt.contains("round-001/accept/attempt-001"));
    assert!(prompt.user_prompt.contains("输出 artifact=accept-result"));
    assert!(prompt.user_prompt.contains("attachments/accept-report.md"));
    assert!(
        prompt
            .user_prompt
            .contains("- round-001/plan/attempt-001: attachments/tech-plan.md")
    );
    assert!(
        !prompt
            .user_prompt
            .contains("- round-001/accept/attempt-001: attachments/accept-report.md")
    );

    req.runtime_context.language = gold_band::config::DesktopLanguage::En;
    let english_prompt = render_prompt_bundle(&req).unwrap();
    assert!(
        english_prompt
            .user_prompt
            .contains("$new-round was triggered by this node")
    );
    assert!(
        english_prompt
            .user_prompt
            .contains("output artifact=accept-result")
    );
    assert!(
        english_prompt
            .user_prompt
            .contains("output preview={\"result\":false")
    );
}

#[test]
fn render_prompt_bundle_shows_empty_attachments() {
    let mut req = invocation();
    req.predecessors = vec![PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "plan".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "普通".to_string(),
        outcome: Some("success".to_string()),
        branch_direction: Some("success".to_string()),
        output_artifact: None,
        branch_reason: None,
        attachments: Vec::new(),
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(!prompt.user_prompt.contains("## 最新前序附件"));
}

#[test]
fn render_prompt_bundle_attachment_section_in_hidden_block() {
    let mut req = invocation();
    req.predecessors = vec![PromptPredecessorContext {
        round_id: "round-001".to_string(),
        node_id: "dev".to_string(),
        attempt_id: "attempt-001".to_string(),
        node_type: "worker".to_string(),
        branch_kind: "普通".to_string(),
        outcome: Some("success".to_string()),
        branch_direction: Some("success".to_string()),
        output_artifact: None,
        branch_reason: None,
        attachments: vec![PromptAttachmentRef {
            name: "notes.md".to_string(),
        }],
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    // Verify the attachment section is inside the hidden block
    let hidden_start = prompt
        .user_prompt
        .find("<hidden data-gold-band-hidden=\"true\"")
        .unwrap();
    let hidden_end = prompt.user_prompt[hidden_start..]
        .find("</hidden>")
        .unwrap();
    let hidden_content = &prompt.user_prompt[hidden_start..hidden_start + hidden_end];
    assert!(
        hidden_content.contains("## 最新前序附件"),
        "attachment section should be inside hidden block"
    );
    assert!(
        hidden_content.contains("- round-001/dev/attempt-001: attachments/notes.md"),
        "attachment content should be inside hidden block"
    );
}

#[test]
fn render_prompt_bundle_ai_dynamic_hidden_section_suppresses_base_predecessor_context() {
    let mut req = invocation();
    req.extra_hidden_sections = vec![PromptHiddenSection {
        title: "Gold Band AI-DYNAMIC runtime context".to_string(),
        content: "# 本次 AI-DYNAMIC 运行上下文\n\n## 直接前序节点\n- bootstrap\n\n\n\n\n## 会话复用\n- Session mode：new".to_string(),
    }];

    let prompt = render_prompt_bundle(&req).unwrap();

    assert!(prompt.user_prompt.contains("# 本次 AI-DYNAMIC 运行上下文"));
    assert!(prompt.user_prompt.contains("## 直接前序节点"));
    assert!(!prompt.user_prompt.contains("## 最新前序执行链"));
    assert!(!prompt.user_prompt.contains("当前节点的前序运行节点：无"));
    assert!(!prompt.user_prompt.contains("\n\n\n"));
}
