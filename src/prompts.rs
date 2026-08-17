use anyhow::{Result, anyhow};
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};

use crate::config::DesktopLanguage;

pub const PROFILE_PLAN_ZH_CN: &str = include_str!("prompts/zh-CN/profile/plan.md");
pub const PROFILE_DEV_ZH_CN: &str = include_str!("prompts/zh-CN/profile/dev.md");
pub const PROFILE_DEV_TEST_ZH_CN: &str = include_str!("prompts/zh-CN/profile/dev-test.md");
pub const PROFILE_REVIEW_ZH_CN: &str = include_str!("prompts/zh-CN/profile/review.md");
pub const PROFILE_TEST_ZH_CN: &str = include_str!("prompts/zh-CN/profile/test.md");
pub const PROFILE_ACCEPT_ZH_CN: &str = include_str!("prompts/zh-CN/profile/accept.md");
pub const PROFILE_CLEAN_ZH_CN: &str = include_str!("prompts/zh-CN/profile/clean.md");
pub const PROFILE_INTERVIEW_ZH_CN: &str = include_str!("prompts/zh-CN/profile/interview.md");
pub const PROFILE_GRILLME_ZH_CN: &str = include_str!("prompts/zh-CN/profile/GrillMe.md");
pub const PROFILE_PLAN_EN: &str = include_str!("prompts/en/profile/plan.md");
pub const PROFILE_DEV_EN: &str = include_str!("prompts/en/profile/dev.md");
pub const PROFILE_DEV_TEST_EN: &str = include_str!("prompts/en/profile/dev-test.md");
pub const PROFILE_REVIEW_EN: &str = include_str!("prompts/en/profile/review.md");
pub const PROFILE_TEST_EN: &str = include_str!("prompts/en/profile/test.md");
pub const PROFILE_ACCEPT_EN: &str = include_str!("prompts/en/profile/accept.md");
pub const PROFILE_CLEAN_EN: &str = include_str!("prompts/en/profile/clean.md");
pub const PROFILE_INTERVIEW_EN: &str = include_str!("prompts/en/profile/interview.md");
pub const PROFILE_GRILLME_EN: &str = include_str!("prompts/en/profile/GrillMe.md");
pub const RUNTIME_SYSTEM_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/system.md");
pub const RUNTIME_SYSTEM_EN: &str = include_str!("prompts/en/runtime/system.md");
pub const RUNTIME_HIDDEN_CONTEXT_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/hidden_context.md");
pub const RUNTIME_HIDDEN_CONTEXT_EN: &str = include_str!("prompts/en/runtime/hidden_context.md");
pub const RUNTIME_USER_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/user.md");
pub const RUNTIME_USER_EN: &str = include_str!("prompts/en/runtime/user.md");
pub const RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/invalid_output_repair.md");
pub const RUNTIME_INVALID_OUTPUT_REPAIR_EN: &str =
    include_str!("prompts/en/runtime/invalid_output_repair.md");
pub const RUNTIME_SCHEDULED_TASK_CONTEXT_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/scheduled_task_context.md");
pub const RUNTIME_SCHEDULED_TASK_CONTEXT_EN: &str =
    include_str!("prompts/en/runtime/scheduled_task_context.md");
pub const RUNTIME_ARTIFACT_FINALIZE_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/artifact_finalize.md");
pub const RUNTIME_ARTIFACT_FINALIZE_EN: &str =
    include_str!("prompts/en/runtime/artifact_finalize.md");
pub const RUNTIME_CONTROL_RESUME_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/runtime_control_resume.md");
pub const RUNTIME_CONTROL_RESUME_EN: &str =
    include_str!("prompts/en/runtime/runtime_control_resume.md");
pub const RUNTIME_CONTROL_RESUME_WITH_MESSAGE_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/runtime_control_resume_with_message.md");
pub const RUNTIME_CONTROL_RESUME_WITH_MESSAGE_EN: &str =
    include_str!("prompts/en/runtime/runtime_control_resume_with_message.md");
pub const RUNTIME_WORKFLOW_RESUME_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/workflow_resume.md");
pub const RUNTIME_WORKFLOW_RESUME_EN: &str = include_str!("prompts/en/runtime/workflow_resume.md");
pub const AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/proposal_repair.md");
pub const AI_DYNAMIC_PROPOSAL_REPAIR_EN: &str =
    include_str!("prompts/en/runtime/ai-dynamic/proposal_repair.md");
pub const AI_DYNAMIC_FANOUT_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/fanout.md");
pub const AI_DYNAMIC_FANOUT_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/fanout.md");
pub const AI_DYNAMIC_MERGE_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/merge.md");
pub const AI_DYNAMIC_MERGE_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/merge.md");
pub const AI_DYNAMIC_ACCEPTANCE_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/acceptance.md");
pub const AI_DYNAMIC_ACCEPTANCE_EN: &str =
    include_str!("prompts/en/runtime/ai-dynamic/acceptance.md");
pub const AI_DYNAMIC_NODE_TASK_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/node_task.md");
pub const AI_DYNAMIC_NODE_TASK_EN: &str =
    include_str!("prompts/en/runtime/ai-dynamic/node_task.md");
pub const AI_DYNAMIC_HIDDEN_CONTEXT_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/hidden_context.md");
pub const AI_DYNAMIC_HIDDEN_CONTEXT_EN: &str =
    include_str!("prompts/en/runtime/ai-dynamic/hidden_context.md");
pub const AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/workflow_invocation.md");
pub const AI_DYNAMIC_WORKFLOW_INVOCATION_EN: &str =
    include_str!("prompts/en/runtime/ai-dynamic/workflow_invocation.md");
pub const AI_DYNAMIC_SYSTEM_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/system.md");
pub const AI_DYNAMIC_SYSTEM_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/system.md");
pub const AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/ai-dynamic/output_protocol.md");
pub const AI_DYNAMIC_OUTPUT_PROTOCOL_EN: &str =
    include_str!("prompts/en/runtime/ai-dynamic/output_protocol.md");

#[derive(Debug, Clone, Serialize)]
pub struct ProfileTemplateContext {
    pub execution: ProfileExecutionTemplateContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptExecutionSurface {
    Workflow,
    AiDynamic,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileExecutionTemplateContext {
    pub surface: PromptExecutionSurface,
    pub can_route_next: bool,
    pub has_output_contract: bool,
    pub session_mode: String,
}

pub fn profile_template_context(
    surface: PromptExecutionSurface,
    has_output_contract: bool,
    session_mode: &str,
) -> ProfileTemplateContext {
    ProfileTemplateContext {
        execution: ProfileExecutionTemplateContext {
            surface,
            can_route_next: surface == PromptExecutionSurface::AiDynamic && has_output_contract,
            has_output_contract,
            session_mode: session_mode.to_string(),
        },
    }
}

pub fn profile_template_validation_contexts() -> [ProfileTemplateContext; 4] {
    [
        profile_template_context(PromptExecutionSurface::Workflow, false, "new"),
        profile_template_context(PromptExecutionSurface::Workflow, true, "continue"),
        profile_template_context(PromptExecutionSurface::AiDynamic, true, "new"),
        profile_template_context(PromptExecutionSurface::AiDynamic, true, "continue"),
    ]
}

pub fn render<T: Serialize>(template: &str, context: T) -> Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let template = env
        .template_from_str(template)
        .map_err(|error| anyhow!(error.to_string()))?;
    template
        .render(context)
        .map_err(|error| anyhow!(error.to_string()))
}

pub fn prompt_by_language<'a>(language: DesktopLanguage, zh_cn: &'a str, en: &'a str) -> &'a str {
    match language {
        DesktopLanguage::ZhCn => zh_cn,
        DesktopLanguage::En => en,
    }
}
