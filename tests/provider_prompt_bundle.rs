use camino::Utf8PathBuf;
use gold_band::domain::{InvocationKind, SessionMode};
use gold_band::provider::{
    ColdFileRef, PromptArtifactRef, PromptOutputContract, PromptPredecessorContext,
    PromptRuntimeContext, StreamMode, WorkerInvocation, render_prompt_bundle,
};

fn runtime_context() -> PromptRuntimeContext {
    PromptRuntimeContext {
        project_id: "D--Projects-code-ai-Gold-Band".to_string(),
        task_id: "task-001".to_string(),
        run_id: "run-001".to_string(),
        round_id: "round-001".to_string(),
        node_id: "dev".to_string(),
        attempt_id: "attempt-001".to_string(),
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
    }
}

fn invocation() -> WorkerInvocation {
    WorkerInvocation {
        invocation_kind: InvocationKind::WorkerGeneric,
        profile: Some("developer".to_string()),
        profile_content: Some("你是负责实现当前节点的开发角色。".to_string()),
        requirement_path: None,
        requirement_text: Some("Need an implementation".to_string()),
        workspace_dir: Utf8PathBuf::from("/repo"),
        attempt_dir: runtime_context().attempt_dir,
        primary_artifact: Some("dev-result".to_string()),
        output_contract: Some(PromptOutputContract {
            artifact: "dev-result".to_string(),
            kind: "json".to_string(),
            schema: Some(serde_json::json!({
                "result": "boolean",
                "reason": "string"
            })),
            success_condition: Some("JSON field `$.result` equals `true`".to_string()),
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
                path: Utf8PathBuf::from("/run/rounds/round-001/nodes/plan/attempt-001/artifacts/plan-result.json"),
                preview: Some("{\"result\":true}".to_string()),
            }),
            branch_reason: Some("输出 DSL artifact=plan-result kind=json；success_condition=$.result == true".to_string()),
        }],
        task_instruction: Some("Implement the requested change".to_string()),
        session_mode: SessionMode::New,
        continue_ref: None,
        resume_prompt: None,
        resume_prompt_id: None,
        stream_mode: StreamMode::None,
        log_prompts: false,
        log_provider_command: false,
        feedback_summary: None,
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
    }
}

#[test]
fn worker_invocation_can_be_serialized_with_context_indexes() {
    let value = serde_json::to_value(invocation()).unwrap();
    assert_eq!(value["primary_artifact"], "dev-result");
    assert_eq!(value["runtime_context"]["task_id"], "task-001");
    assert_eq!(value["cold_artifacts"][0]["name"], "exec-result");
}

#[test]
fn render_prompt_bundle_uses_runtime_context_without_old_invocation_labels() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(prompt.system_prompt.contains("Project: D--Projects-code-ai-Gold-Band"));
    assert!(prompt.system_prompt.contains("Task: task-001"));
    assert!(prompt.system_prompt.contains("Run: run-001"));
    assert!(prompt.system_prompt.contains("Round: round-001"));
    assert!(prompt.system_prompt.contains("Node: dev"));
    assert!(prompt.system_prompt.contains("Attempt: attempt-001"));
    assert!(!prompt.system_prompt.contains("Invocation kind"));
    assert!(!prompt.system_prompt.contains("WorkerGeneric"));
    assert!(!prompt.system_prompt.contains("VerifyAcceptance"));
}

#[test]
fn render_prompt_bundle_moves_profile_content_to_system_prompt() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(prompt.system_prompt.contains("你是负责实现当前节点的开发角色"));
    assert!(!prompt.user_prompt.contains("你是负责实现当前节点的开发角色"));
}

#[test]
fn render_prompt_bundle_renders_predecessor_chain_and_output_dsl() {
    let prompt = render_prompt_bundle(&invocation()).unwrap();

    assert!(prompt.system_prompt.contains("plan/attempt-001 -success-> 当前节点"));
    assert!(prompt.system_prompt.contains("节点输出检查"));
    assert!(prompt.system_prompt.contains("plan-result"));
    assert!(prompt.system_prompt.contains("你必须在最后一步按照以下格式输出你的结果"));
    assert!(prompt.system_prompt.contains("\"result\": \"boolean\""));
    assert!(prompt.system_prompt.contains("\"reason\": \"string\""));
    assert!(prompt.system_prompt.contains("JSON field `$.result` equals `true`"));
}

#[test]
fn render_prompt_bundle_continue_keeps_system_prompt_empty() {
    let mut req = invocation();
    req.session_mode = SessionMode::Continue;
    req.resume_prompt = Some("继续".to_string());
    req.resume_prompt_id = Some("resume-001".to_string());

    let prompt = render_prompt_bundle(&req).unwrap();

    assert_eq!(prompt.system_prompt, "");
    assert_eq!(prompt.user_prompt, "继续");
    assert_eq!(prompt.prompt_id.as_deref(), Some("resume-001"));
}
