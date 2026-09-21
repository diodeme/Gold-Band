use anyhow::{Result, anyhow};
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};

use crate::config::DesktopLanguage;

pub const PROFILE_PLAN_ZH_CN: &str = include_str!("prompts/zh-CN/profile/plan.md");
pub const PROFILE_DEV_ZH_CN: &str = include_str!("prompts/zh-CN/profile/dev.md");
pub const PROFILE_DEV_TEST_ZH_CN: &str = include_str!("prompts/zh-CN/profile/dev-test.md");
pub const PROFILE_REVIEW_ZH_CN: &str = include_str!("prompts/zh-CN/profile/review.md");
pub const PROFILE_TEST_ZH_CN: &str = include_str!("prompts/zh-CN/profile/test.md");
pub const PROFILE_CICD_ZH_CN: &str = include_str!("prompts/zh-CN/profile/cicd.md");
pub const PROFILE_ACCEPT_ZH_CN: &str = include_str!("prompts/zh-CN/profile/accept.md");
pub const PROFILE_CLEAN_ZH_CN: &str = include_str!("prompts/zh-CN/profile/clean.md");
pub const PROFILE_INTERVIEW_ZH_CN: &str = include_str!("prompts/zh-CN/profile/interview.md");
pub const PROFILE_GRILLME_ZH_CN: &str = include_str!("prompts/zh-CN/profile/GrillMe.md");
pub const PROFILE_PLAN_EN: &str = include_str!("prompts/en/profile/plan.md");
pub const PROFILE_DEV_EN: &str = include_str!("prompts/en/profile/dev.md");
pub const PROFILE_DEV_TEST_EN: &str = include_str!("prompts/en/profile/dev-test.md");
pub const PROFILE_REVIEW_EN: &str = include_str!("prompts/en/profile/review.md");
pub const PROFILE_TEST_EN: &str = include_str!("prompts/en/profile/test.md");
pub const PROFILE_CICD_EN: &str = include_str!("prompts/en/profile/cicd.md");
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
pub const RUNTIME_REMOTE_TASK_CONTEXT_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/remote_task_context.md");
pub const RUNTIME_REMOTE_TASK_CONTEXT_EN: &str =
    include_str!("prompts/en/runtime/remote_task_context.md");
pub const RUNTIME_REMOTE_TASK_PARENT_OUTPUT_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/remote_task_parent_output.md");
pub const RUNTIME_REMOTE_TASK_PARENT_OUTPUT_EN: &str =
    include_str!("prompts/en/runtime/remote_task_parent_output.md");
pub const RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL_ZH_CN: &str =
    include_str!("prompts/zh-CN/runtime/remote_task_completion_protocol.md");
pub const RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL_EN: &str =
    include_str!("prompts/en/runtime/remote_task_completion_protocol.md");
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
    fn remote_task_context_templates_render_all_and_partial_fields() {
        // 远程任务 DPMS 溯源块（multica 预填 composer / 会话起始输入）：全字段与部分字段
        // （其余 null，对应 wire 缺省键）都需完整渲染，且中文标签锁定（发布计划 ID / 业务需求 ID / 需求链接）。
        let full = json!({
            "release_plan_id": 538181,
            "dev_user": "alice,bob",
            "test_user": "carol",
            "business_story_id": 674290,
            "origin_url": "https://dpms.example.com/story/674290"
        });
        let zh = assert_fully_rendered(RUNTIME_REMOTE_TASK_CONTEXT_ZH_CN, full.clone());
        assert!(zh.contains("- 发布计划 ID: 538181"));
        assert!(zh.contains("- 开发负责人: alice,bob"));
        assert!(zh.contains("- 业务需求 ID: 674290"));
        assert!(zh.contains("- 需求链接: https://dpms.example.com/story/674290"));

        let en = assert_fully_rendered(RUNTIME_REMOTE_TASK_CONTEXT_EN, full);
        assert!(en.contains("- Release plan ID: 538181"));
        assert!(en.contains("- Business story ID: 674290"));
        assert!(en.contains("- Requirement link: https://dpms.example.com/story/674290"));

        // 部分字段（其余 null）：仅渲染存在的行，缺席字段不出行、无空行残留。
        let partial = json!({
            "release_plan_id": 538181,
            "dev_user": null,
            "test_user": null,
            "business_story_id": null,
            "origin_url": null
        });
        let zh_partial = assert_fully_rendered(RUNTIME_REMOTE_TASK_CONTEXT_ZH_CN, partial.clone());
        assert!(zh_partial.contains("- 发布计划 ID: 538181"));
        assert!(!zh_partial.contains("开发负责人"));
        assert!(!zh_partial.contains("\n\n"));
        let en_partial = assert_fully_rendered(RUNTIME_REMOTE_TASK_CONTEXT_EN, partial);
        assert!(!en_partial.contains("Dev owner"));
    }

    #[test]
    fn remote_task_parent_output_templates_render_handoff() {
        // 上游交付说明块（issue 完成输出传递特性）：parent_output 渲染进模板正文，
        // 中英文都需完整渲染且锁定关键语义（执行上下文 + 交付说明）。
        let ctx = json!({"parent_output": "部署地址: https://t.example.com\n测试要点: 回归登录链路"});
        let zh = assert_fully_rendered(RUNTIME_REMOTE_TASK_PARENT_OUTPUT_ZH_CN, ctx.clone());
        assert!(zh.contains("执行上下文"));
        assert!(zh.contains("部署地址: https://t.example.com"));

        let en = assert_fully_rendered(RUNTIME_REMOTE_TASK_PARENT_OUTPUT_EN, ctx);
        assert!(en.contains("execution context"));
        assert!(en.contains("部署地址: https://t.example.com"));
    }

    #[test]
    fn remote_task_completion_protocol_templates_lock_fence_contract() {
        // 完成输出协议（写侧）：模板不含变量、无条件渲染；中英文都必须锁定围栏块 info 串
        // `completion-output`（提取函数按该 info 串精确匹配，两语言漂移即提取失效）。
        // 服务端上限 16k 收紧（2026-09-21）后另锁三点篇幅契约：非 run 日志（最终交付说明）、
        // 2k 字符以内、超长走工作区文件 + 块内路径引用（超长 400 会卡整个 done 请求）。
        for template in [
            RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL_ZH_CN,
            RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL_EN,
        ] {
            let rendered = assert_fully_rendered(template, json!({}));
            assert!(rendered.contains("completion-output"));
        }
        let zh = RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL_ZH_CN;
        let en = RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL_EN;
        assert!(zh.contains("交付说明"));
        assert!(zh.contains("run 日志"));
        assert!(zh.contains("2000 字符"));
        assert!(zh.contains("文件路径引用"));
        assert!(en.contains("deliverable handoff"));
        assert!(en.contains("run log"));
        assert!(en.contains("2,000 characters"));
        assert!(en.contains("reference them by path"));
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

    fn assert_prompt_contract(prompt: &str, required_fragments: &[&str]) {
        for fragment in required_fragments {
            assert!(
                prompt.contains(fragment),
                "prompt contract is missing `{fragment}`"
            );
        }
    }

    #[test]
    fn ai_dynamic_system_prompts_define_scope_authority_and_the_scope_gate() {
        assert_prompt_contract(
            AI_DYNAMIC_SYSTEM_ZH_CN,
            &[
                "人类最新指令 > 原始需求与明确非目标",
                "运行前已纳入范围的项目契约",
                "低层内容只能细化执行，不能扩大高层范围",
                "runtime 任务可以拆解已授权工作",
                "本轮新增内容只能提供证据或建议",
                "省略后会失败的既定结果",
                "内部手段无需在需求中逐字出现",
                "可归因到本轮变更的范围漂移",
                "恢复最小范围内方案",
            ],
        );
        assert_prompt_contract(
            AI_DYNAMIC_SYSTEM_EN,
            &[
                "latest relevant human instruction > original requirement and explicit non-goals",
                "pre-run project contracts already in scope",
                "cannot expand higher-authority scope",
                "runtime tasks may decompose authorized work",
                "content added during this run provide evidence or suggestions",
                "established outcome that would fail without it",
                "need not appear verbatim in the requirement",
                "scope drift proven by change evidence attributable to this run",
                "Restore the minimum in-scope solution",
            ],
        );
    }

    #[test]
    fn acceptance_prompts_separate_scope_or_regression_backed_blockers_from_follow_ups() {
        for prompt in [AI_DYNAMIC_ACCEPTANCE_ZH_CN, PROFILE_ACCEPT_ZH_CN] {
            assert_prompt_contract(
                prompt,
                &[
                    "BLOCKER",
                    "FOLLOW_UP",
                    "范围内结果失败或无法验证",
                    "可达回归",
                    "可归因到本轮变更",
                    "范围依据",
                    "当前证据",
                    "失败因果",
                    "不影响通过",
                    "不创建修复",
                    "恢复最小范围内方案",
                ],
            );
        }
        for prompt in [AI_DYNAMIC_ACCEPTANCE_EN, PROFILE_ACCEPT_EN] {
            assert_prompt_contract(
                prompt,
                &[
                    "BLOCKER",
                    "FOLLOW_UP",
                    "in-scope outcome that fails or cannot be verified",
                    "reachable regression",
                    "change evidence attributable to this run",
                    "scope basis",
                    "current evidence",
                    "failure causality",
                    "does not affect acceptance",
                    "create repair",
                    "minimum in-scope solution",
                ],
            );
        }

        assert_prompt_contract(AI_DYNAMIC_ACCEPTANCE_ZH_CN, &["不得修改业务代码或测试代码"]);
        assert_prompt_contract(
            AI_DYNAMIC_ACCEPTANCE_EN,
            &["Do not modify business code or test code"],
        );
    }

    #[test]
    fn development_role_removes_unnecessary_self_selected_mechanisms() {
        assert_prompt_contract(
            PROFILE_DEV_TEST_ZH_CN,
            &[
                "节点任务和反馈只能细化执行，不能扩大范围",
                "省略后会失败的既定结果",
                "内部手段无需在需求中逐字出现",
                "首次执行完成既定范围",
                "只修复有范围依据、当前证据和失败因果的 `BLOCKER`",
                "恢复最小范围内方案",
            ],
        );
        assert_prompt_contract(
            PROFILE_DEV_TEST_EN,
            &[
                "Node tasks and feedback may refine execution, but cannot expand scope",
                "established outcome that would fail without it",
                "need not appear verbatim in the requirement",
                "On initial execution, complete the established scope",
                "repair only a `BLOCKER` with a scope basis, current evidence, and failure causality",
                "minimum in-scope solution",
            ],
        );
    }

    #[test]
    fn routing_prompts_do_not_promote_suggestions_into_successor_scope() {
        assert_prompt_contract(
            AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN,
            &[
                "后继任务",
                "既定范围内结果",
                "BLOCKER",
                "FOLLOW_UP",
                "前序建议",
            ],
        );
        assert_prompt_contract(
            AI_DYNAMIC_OUTPUT_PROTOCOL_EN,
            &[
                "successor task",
                "established in-scope outcome",
                "BLOCKER",
                "FOLLOW_UP",
                "predecessor suggestion",
            ],
        );
        assert_prompt_contract(
            AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN,
            &[
                "只修复协议校验错误",
                "不重新执行任务",
                "符合范围契约",
                "删除或收窄",
            ],
        );
        assert_prompt_contract(
            AI_DYNAMIC_PROPOSAL_REPAIR_EN,
            &[
                "Repair only protocol validation errors",
                "do not re-execute the task",
                "satisfy the scope contract",
                "remove or narrow",
            ],
        );
    }

    #[test]
    fn workflow_resume_optimizes_for_established_scope_not_reviewer_approval() {
        assert_prompt_contract(
            RUNTIME_WORKFLOW_RESUME_ZH_CN,
            &["继续当前节点任务", "反馈是证据，不是新增授权", "既定范围"],
        );
        assert_prompt_contract(
            RUNTIME_WORKFLOW_RESUME_EN,
            &[
                "Continue the current node task",
                "Feedback is evidence, not new authorization",
                "established scope",
            ],
        );
        assert!(RUNTIME_WORKFLOW_RESUME_ZH_CN.chars().count() <= 60);
        assert!(RUNTIME_WORKFLOW_RESUME_EN.chars().count() <= 180);
    }
}
