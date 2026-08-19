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
pub const PERSONAL_ANALYTICS_SYSTEM_ZH_CN: &str =
    include_str!("prompts/zh-CN/personal-analytics/system.md");
pub const PERSONAL_ANALYTICS_SYSTEM_EN: &str =
    include_str!("prompts/en/personal-analytics/system.md");
pub const PERSONAL_ANALYTICS_USER_ZH_CN: &str =
    include_str!("prompts/zh-CN/personal-analytics/user.md");
pub const PERSONAL_ANALYTICS_USER_EN: &str = include_str!("prompts/en/personal-analytics/user.md");
pub const PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN: &str =
    include_str!("prompts/zh-CN/personal-analytics/repair_system.md");
pub const PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN: &str =
    include_str!("prompts/en/personal-analytics/repair_system.md");
pub const PERSONAL_ANALYTICS_REPAIR_USER_ZH_CN: &str =
    include_str!("prompts/zh-CN/personal-analytics/repair_user.md");
pub const PERSONAL_ANALYTICS_REPAIR_USER_EN: &str =
    include_str!("prompts/en/personal-analytics/repair_user.md");

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn assert_fully_rendered(template: &str, context: serde_json::Value) -> String {
        let rendered =
            render(template, context).expect("prompt should render with strict variables");
        assert!(!rendered.contains("{{"), "unresolved output expression");
        assert!(!rendered.contains("{%"), "unresolved control expression");
        rendered
    }

    #[test]
    fn personal_analytics_templates_render_with_strict_contexts() {
        let report_schema = r#"{"type":"object","required":["schemaVersion"]}"#;

        for template in [
            PERSONAL_ANALYTICS_SYSTEM_ZH_CN,
            PERSONAL_ANALYTICS_SYSTEM_EN,
        ] {
            assert_fully_rendered(template, json!({ "report_schema": report_schema }));
        }

        for template in [PERSONAL_ANALYTICS_USER_ZH_CN, PERSONAL_ANALYTICS_USER_EN] {
            assert_fully_rendered(
                template,
                json!({
                    "operation_id": "operation-001",
                    "report_schema_version": "1.0.0",
                    "source_watermark": "2026-08-17T12:00:00Z",
                    "index_revision": 7,
                    "date_range": "{\"start\":null,\"end\":null}",
                    "projection_path": "C:\\analytics\\projection.json",
                    "content_manifest_path": "C:\\analytics\\content-manifest.json",
                    "semantic_batch_manifest_path": "C:\\analytics\\semantic-batches.json",
                    "coverage_summary": "{\"parsed\": 90, \"skipped\": 2}"
                }),
            );
        }

        for template in [
            PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN,
            PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN,
        ] {
            assert_fully_rendered(template, json!({}));
        }

        for template in [
            PERSONAL_ANALYTICS_REPAIR_USER_ZH_CN,
            PERSONAL_ANALYTICS_REPAIR_USER_EN,
        ] {
            assert_fully_rendered(
                template,
                json!({
                    "operation_id": "operation-001",
                    "invalid_report_path": "C:\\analytics\\invalid-report.json",
                    "validation_errors": "$.overview: required property is missing",
                    "report_schema": report_schema
                }),
            );
        }
    }

    #[test]
    fn personal_analytics_system_prompts_lock_metric_and_evidence_contracts() {
        for (template, metric_names) in [
            (
                PERSONAL_ANALYTICS_SYSTEM_ZH_CN,
                [
                    "direct.reply_completion_rate",
                    "workflow.run_terminal_success_rate",
                    "auto.outer_run_terminal_success_rate",
                ],
            ),
            (
                PERSONAL_ANALYTICS_SYSTEM_EN,
                [
                    "direct.reply_completion_rate",
                    "workflow.run_terminal_success_rate",
                    "auto.outer_run_terminal_success_rate",
                ],
            ),
        ] {
            for metric_name in metric_names {
                assert!(
                    template.contains(metric_name),
                    "missing metric name: {metric_name}"
                );
            }
            assert!(template.to_ascii_lowercase().contains("evidence locator"));
            assert!(template.contains("sampleCount"));
            assert!(template.contains("confidence"));
            assert!(template.contains("acp.raw.jsonl"));
            assert!(template.contains("{{ report_schema }}"));
        }
    }

    #[test]
    fn personal_analytics_repair_prompts_forbid_unsupported_facts() {
        assert!(PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN.contains("不新增洞察"));
        assert!(PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN.contains("不得猜测"));
        assert!(PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN.contains("do not repeat the analysis"));
        assert!(PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN.contains("Never guess"));
    }

    #[test]
    fn personal_analytics_templates_reject_missing_variables() {
        let error = render(PERSONAL_ANALYTICS_USER_ZH_CN, json!({}));
        assert!(
            error.is_err(),
            "strict rendering must reject a missing operation context"
        );
    }
}
