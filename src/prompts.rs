use anyhow::{Result, anyhow};
use minijinja::{Environment, UndefinedBehavior};
use serde::Serialize;

use crate::config::DesktopLanguage;

pub const PROFILE_PLAN_ZH_CN: &str = include_str!("prompts/zh-CN/profile/plan.md");
pub const PROFILE_DEV_ZH_CN: &str = include_str!("prompts/zh-CN/profile/dev.md");
pub const PROFILE_REVIEW_ZH_CN: &str = include_str!("prompts/zh-CN/profile/review.md");
pub const PROFILE_TEST_ZH_CN: &str = include_str!("prompts/zh-CN/profile/test.md");
pub const PROFILE_ACCEPT_ZH_CN: &str = include_str!("prompts/zh-CN/profile/accept.md");
pub const PROFILE_CLEAN_ZH_CN: &str = include_str!("prompts/zh-CN/profile/clean.md");
pub const PROFILE_PLAN_EN: &str = include_str!("prompts/en/profile/plan.md");
pub const PROFILE_DEV_EN: &str = include_str!("prompts/en/profile/dev.md");
pub const PROFILE_REVIEW_EN: &str = include_str!("prompts/en/profile/review.md");
pub const PROFILE_TEST_EN: &str = include_str!("prompts/en/profile/test.md");
pub const PROFILE_ACCEPT_EN: &str = include_str!("prompts/en/profile/accept.md");
pub const PROFILE_CLEAN_EN: &str = include_str!("prompts/en/profile/clean.md");
pub const RUNTIME_SYSTEM_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/system.md");
pub const RUNTIME_SYSTEM_EN: &str = include_str!("prompts/en/runtime/system.md");
pub const RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/invalid_output_repair.md");
pub const RUNTIME_INVALID_OUTPUT_REPAIR_EN: &str = include_str!("prompts/en/runtime/invalid_output_repair.md");
pub const AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/proposal_repair.md");
pub const AI_DYNAMIC_PROPOSAL_REPAIR_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/proposal_repair.md");
pub const AI_DYNAMIC_FANOUT_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/fanout.md");
pub const AI_DYNAMIC_FANOUT_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/fanout.md");
pub const AI_DYNAMIC_MERGE_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/merge.md");
pub const AI_DYNAMIC_MERGE_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/merge.md");
pub const AI_DYNAMIC_ACCEPTANCE_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/acceptance.md");
pub const AI_DYNAMIC_ACCEPTANCE_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/acceptance.md");
pub const AI_DYNAMIC_NODE_TASK_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/node_task.md");
pub const AI_DYNAMIC_NODE_TASK_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/node_task.md");
pub const AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/workflow_invocation.md");
pub const AI_DYNAMIC_WORKFLOW_INVOCATION_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/workflow_invocation.md");
pub const AI_DYNAMIC_SYSTEM_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/system.md");
pub const AI_DYNAMIC_SYSTEM_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/system.md");
pub const AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN: &str = include_str!("prompts/zh-CN/runtime/ai-dynamic/output_protocol.md");
pub const AI_DYNAMIC_OUTPUT_PROTOCOL_EN: &str = include_str!("prompts/en/runtime/ai-dynamic/output_protocol.md");

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
