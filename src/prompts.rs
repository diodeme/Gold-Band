use anyhow::{Result, anyhow};
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};

use crate::config::DesktopLanguage;

#[derive(Debug, Clone, Copy)]
pub struct LocalizedText {
    pub zh_cn: &'static str,
    pub en: &'static str,
    pub zh_tw: Option<&'static str>,
    pub ja_jp: Option<&'static str>,
    pub ko_kr: Option<&'static str>,
    pub pt_br: Option<&'static str>,
    pub es: Option<&'static str>,
}

impl LocalizedText {
    pub const fn all(
        zh_cn: &'static str,
        zh_tw: &'static str,
        en: &'static str,
        ja_jp: &'static str,
        ko_kr: &'static str,
        pt_br: &'static str,
        es: &'static str,
    ) -> Self {
        Self {
            zh_cn,
            en,
            zh_tw: Some(zh_tw),
            ja_jp: Some(ja_jp),
            ko_kr: Some(ko_kr),
            pt_br: Some(pt_br),
            es: Some(es),
        }
    }

    pub const fn zh_en(zh_cn: &'static str, en: &'static str) -> Self {
        Self {
            zh_cn,
            en,
            zh_tw: None,
            ja_jp: None,
            ko_kr: None,
            pt_br: None,
            es: None,
        }
    }

    pub fn resolve(self, language: DesktopLanguage) -> &'static str {
        match language {
            DesktopLanguage::ZhCn => self.zh_cn,
            DesktopLanguage::En => self.en,
            DesktopLanguage::ZhTw => self.zh_tw.unwrap_or(self.en),
            DesktopLanguage::JaJp => self.ja_jp.unwrap_or(self.en),
            DesktopLanguage::KoKr => self.ko_kr.unwrap_or(self.en),
            DesktopLanguage::PtBr => self.pt_br.unwrap_or(self.en),
            DesktopLanguage::Es => self.es.unwrap_or(self.en),
        }
    }
}

macro_rules! localized_prompt {
    ($path:literal) => {
        LocalizedText::all(
            include_str!(concat!("prompts/zh-CN/", $path)),
            include_str!(concat!("prompts/zh-TW/", $path)),
            include_str!(concat!("prompts/en/", $path)),
            include_str!(concat!("prompts/ja-JP/", $path)),
            include_str!(concat!("prompts/ko-KR/", $path)),
            include_str!(concat!("prompts/pt-BR/", $path)),
            include_str!(concat!("prompts/es/", $path)),
        )
    };
}

macro_rules! localized_prompt_zh_en {
    ($path:literal) => {
        LocalizedText::zh_en(
            include_str!(concat!("prompts/zh-CN/", $path)),
            include_str!(concat!("prompts/en/", $path)),
        )
    };
}

pub const PROFILE_PLAN: LocalizedText = localized_prompt!("profile/plan.md");
pub const PROFILE_DEV: LocalizedText = localized_prompt!("profile/dev.md");
pub const PROFILE_DEV_TEST: LocalizedText = localized_prompt!("profile/dev-test.md");
pub const PROFILE_REVIEW: LocalizedText = localized_prompt!("profile/review.md");
pub const PROFILE_TEST: LocalizedText = localized_prompt!("profile/test.md");
pub const PROFILE_CICD: LocalizedText = localized_prompt_zh_en!("profile/cicd.md");
pub const PROFILE_ACCEPT: LocalizedText = localized_prompt!("profile/accept.md");
pub const PROFILE_CLEAN: LocalizedText = localized_prompt!("profile/clean.md");
pub const PROFILE_INTERVIEW: LocalizedText = localized_prompt!("profile/interview.md");
pub const PROFILE_GRILLME: LocalizedText = localized_prompt!("profile/GrillMe.md");
pub const PROFILE_OVERLAY_REQUIREMENT_IDENTITY: LocalizedText =
    localized_prompt!("profile/overlays/requirement-identity.md");
pub const PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT: LocalizedText =
    localized_prompt!("profile/overlays/dev-test-auto-commit.md");
pub const RUNTIME_SYSTEM: LocalizedText = localized_prompt!("runtime/system.md");
pub const RUNTIME_HIDDEN_CONTEXT: LocalizedText = localized_prompt!("runtime/hidden_context.md");
pub const RUNTIME_USER: LocalizedText = localized_prompt!("runtime/user.md");
pub const RUNTIME_INVALID_OUTPUT_REPAIR: LocalizedText =
    localized_prompt!("runtime/invalid_output_repair.md");
pub const RUNTIME_SCHEDULED_TASK_CONTEXT: LocalizedText =
    localized_prompt!("runtime/scheduled_task_context.md");
pub const RUNTIME_ARTIFACT_FINALIZE: LocalizedText =
    localized_prompt!("runtime/artifact_finalize.md");
pub const RUNTIME_CONTROL_RESUME: LocalizedText =
    localized_prompt!("runtime/runtime_control_resume.md");
pub const RUNTIME_CONTROL_RESUME_WITH_MESSAGE: LocalizedText =
    localized_prompt!("runtime/runtime_control_resume_with_message.md");
pub const RUNTIME_WORKFLOW_RESUME: LocalizedText = localized_prompt!("runtime/workflow_resume.md");
pub const RUNTIME_USER_ROLE_MESSAGE: LocalizedText =
    localized_prompt!("runtime/user_role_message.md");
pub const CICD_GOAL: LocalizedText = localized_prompt_zh_en!("runtime/cicd-goal.md");
pub const AI_DYNAMIC_PROPOSAL_REPAIR: LocalizedText =
    localized_prompt!("runtime/ai-dynamic/proposal_repair.md");
pub const AI_DYNAMIC_FANOUT: LocalizedText = localized_prompt!("runtime/ai-dynamic/fanout.md");
pub const AI_DYNAMIC_MERGE: LocalizedText = localized_prompt!("runtime/ai-dynamic/merge.md");
pub const AI_DYNAMIC_ACCEPTANCE: LocalizedText =
    localized_prompt!("runtime/ai-dynamic/acceptance.md");
pub const AI_DYNAMIC_NODE_TASK: LocalizedText =
    localized_prompt!("runtime/ai-dynamic/node_task.md");
pub const AI_DYNAMIC_HIDDEN_CONTEXT: LocalizedText =
    localized_prompt!("runtime/ai-dynamic/hidden_context.md");
pub const AI_DYNAMIC_WORKFLOW_INVOCATION: LocalizedText =
    localized_prompt!("runtime/ai-dynamic/workflow_invocation.md");
pub const AI_DYNAMIC_SYSTEM: LocalizedText = localized_prompt!("runtime/ai-dynamic/system.md");
pub const AI_DYNAMIC_OUTPUT_PROTOCOL: LocalizedText =
    localized_prompt!("runtime/ai-dynamic/output_protocol.md");
pub const PERSONAL_ANALYTICS_SYSTEM: LocalizedText =
    localized_prompt!("personal-analytics/system.md");
pub const PERSONAL_ANALYTICS_USER: LocalizedText = localized_prompt!("personal-analytics/user.md");
pub const PERSONAL_ANALYTICS_REPAIR_SYSTEM: LocalizedText =
    localized_prompt!("personal-analytics/repair_system.md");
pub const PERSONAL_ANALYTICS_REPAIR_USER: LocalizedText =
    localized_prompt!("personal-analytics/repair_user.md");
pub const MEMORY_RULES: LocalizedText = localized_prompt!("runtime/memory-rules.md");
pub const MEMORY_TOOLS: LocalizedText = localized_prompt!("runtime/memory-tools.json");

pub const PROFILE_PLAN_ZH_CN: &str = PROFILE_PLAN.zh_cn;
pub const PROFILE_PLAN_EN: &str = PROFILE_PLAN.en;
pub const PROFILE_DEV_ZH_CN: &str = PROFILE_DEV.zh_cn;
pub const PROFILE_DEV_EN: &str = PROFILE_DEV.en;
pub const PROFILE_DEV_TEST_ZH_CN: &str = PROFILE_DEV_TEST.zh_cn;
pub const PROFILE_DEV_TEST_EN: &str = PROFILE_DEV_TEST.en;
pub const PROFILE_REVIEW_ZH_CN: &str = PROFILE_REVIEW.zh_cn;
pub const PROFILE_REVIEW_EN: &str = PROFILE_REVIEW.en;
pub const PROFILE_TEST_ZH_CN: &str = PROFILE_TEST.zh_cn;
pub const PROFILE_TEST_EN: &str = PROFILE_TEST.en;
pub const PROFILE_CICD_ZH_CN: &str = PROFILE_CICD.zh_cn;
pub const PROFILE_CICD_EN: &str = PROFILE_CICD.en;
pub const PROFILE_ACCEPT_ZH_CN: &str = PROFILE_ACCEPT.zh_cn;
pub const PROFILE_ACCEPT_EN: &str = PROFILE_ACCEPT.en;
pub const PROFILE_CLEAN_ZH_CN: &str = PROFILE_CLEAN.zh_cn;
pub const PROFILE_CLEAN_EN: &str = PROFILE_CLEAN.en;
pub const PROFILE_INTERVIEW_ZH_CN: &str = PROFILE_INTERVIEW.zh_cn;
pub const PROFILE_INTERVIEW_EN: &str = PROFILE_INTERVIEW.en;
pub const PROFILE_GRILLME_ZH_CN: &str = PROFILE_GRILLME.zh_cn;
pub const PROFILE_GRILLME_EN: &str = PROFILE_GRILLME.en;
pub const PROFILE_OVERLAY_REQUIREMENT_IDENTITY_ZH_CN: &str =
    PROFILE_OVERLAY_REQUIREMENT_IDENTITY.zh_cn;
pub const PROFILE_OVERLAY_REQUIREMENT_IDENTITY_EN: &str = PROFILE_OVERLAY_REQUIREMENT_IDENTITY.en;
pub const PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT_ZH_CN: &str =
    PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT.zh_cn;
pub const PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT_EN: &str = PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT.en;
pub const RUNTIME_SYSTEM_ZH_CN: &str = RUNTIME_SYSTEM.zh_cn;
pub const RUNTIME_SYSTEM_EN: &str = RUNTIME_SYSTEM.en;
pub const RUNTIME_HIDDEN_CONTEXT_ZH_CN: &str = RUNTIME_HIDDEN_CONTEXT.zh_cn;
pub const RUNTIME_HIDDEN_CONTEXT_EN: &str = RUNTIME_HIDDEN_CONTEXT.en;
pub const RUNTIME_USER_ZH_CN: &str = RUNTIME_USER.zh_cn;
pub const RUNTIME_USER_EN: &str = RUNTIME_USER.en;
pub const RUNTIME_INVALID_OUTPUT_REPAIR_ZH_CN: &str = RUNTIME_INVALID_OUTPUT_REPAIR.zh_cn;
pub const RUNTIME_INVALID_OUTPUT_REPAIR_EN: &str = RUNTIME_INVALID_OUTPUT_REPAIR.en;
pub const RUNTIME_SCHEDULED_TASK_CONTEXT_ZH_CN: &str = RUNTIME_SCHEDULED_TASK_CONTEXT.zh_cn;
pub const RUNTIME_SCHEDULED_TASK_CONTEXT_EN: &str = RUNTIME_SCHEDULED_TASK_CONTEXT.en;
pub const RUNTIME_ARTIFACT_FINALIZE_ZH_CN: &str = RUNTIME_ARTIFACT_FINALIZE.zh_cn;
pub const RUNTIME_ARTIFACT_FINALIZE_EN: &str = RUNTIME_ARTIFACT_FINALIZE.en;
pub const RUNTIME_CONTROL_RESUME_ZH_CN: &str = RUNTIME_CONTROL_RESUME.zh_cn;
pub const RUNTIME_CONTROL_RESUME_EN: &str = RUNTIME_CONTROL_RESUME.en;
pub const RUNTIME_CONTROL_RESUME_WITH_MESSAGE_ZH_CN: &str =
    RUNTIME_CONTROL_RESUME_WITH_MESSAGE.zh_cn;
pub const RUNTIME_CONTROL_RESUME_WITH_MESSAGE_EN: &str = RUNTIME_CONTROL_RESUME_WITH_MESSAGE.en;
pub const RUNTIME_WORKFLOW_RESUME_ZH_CN: &str = RUNTIME_WORKFLOW_RESUME.zh_cn;
pub const RUNTIME_WORKFLOW_RESUME_EN: &str = RUNTIME_WORKFLOW_RESUME.en;
pub const RUNTIME_USER_ROLE_MESSAGE_ZH_CN: &str = RUNTIME_USER_ROLE_MESSAGE.zh_cn;
pub const RUNTIME_USER_ROLE_MESSAGE_EN: &str = RUNTIME_USER_ROLE_MESSAGE.en;
pub const AI_DYNAMIC_PROPOSAL_REPAIR_ZH_CN: &str = AI_DYNAMIC_PROPOSAL_REPAIR.zh_cn;
pub const AI_DYNAMIC_PROPOSAL_REPAIR_EN: &str = AI_DYNAMIC_PROPOSAL_REPAIR.en;
pub const AI_DYNAMIC_FANOUT_ZH_CN: &str = AI_DYNAMIC_FANOUT.zh_cn;
pub const AI_DYNAMIC_FANOUT_EN: &str = AI_DYNAMIC_FANOUT.en;
pub const AI_DYNAMIC_MERGE_ZH_CN: &str = AI_DYNAMIC_MERGE.zh_cn;
pub const AI_DYNAMIC_MERGE_EN: &str = AI_DYNAMIC_MERGE.en;
pub const AI_DYNAMIC_ACCEPTANCE_ZH_CN: &str = AI_DYNAMIC_ACCEPTANCE.zh_cn;
pub const AI_DYNAMIC_ACCEPTANCE_EN: &str = AI_DYNAMIC_ACCEPTANCE.en;
pub const AI_DYNAMIC_NODE_TASK_ZH_CN: &str = AI_DYNAMIC_NODE_TASK.zh_cn;
pub const AI_DYNAMIC_NODE_TASK_EN: &str = AI_DYNAMIC_NODE_TASK.en;
pub const AI_DYNAMIC_HIDDEN_CONTEXT_ZH_CN: &str = AI_DYNAMIC_HIDDEN_CONTEXT.zh_cn;
pub const AI_DYNAMIC_HIDDEN_CONTEXT_EN: &str = AI_DYNAMIC_HIDDEN_CONTEXT.en;
pub const AI_DYNAMIC_WORKFLOW_INVOCATION_ZH_CN: &str = AI_DYNAMIC_WORKFLOW_INVOCATION.zh_cn;
pub const AI_DYNAMIC_WORKFLOW_INVOCATION_EN: &str = AI_DYNAMIC_WORKFLOW_INVOCATION.en;
pub const AI_DYNAMIC_SYSTEM_ZH_CN: &str = AI_DYNAMIC_SYSTEM.zh_cn;
pub const AI_DYNAMIC_SYSTEM_EN: &str = AI_DYNAMIC_SYSTEM.en;
pub const AI_DYNAMIC_OUTPUT_PROTOCOL_ZH_CN: &str = AI_DYNAMIC_OUTPUT_PROTOCOL.zh_cn;
pub const AI_DYNAMIC_OUTPUT_PROTOCOL_EN: &str = AI_DYNAMIC_OUTPUT_PROTOCOL.en;
pub const PERSONAL_ANALYTICS_SYSTEM_ZH_CN: &str = PERSONAL_ANALYTICS_SYSTEM.zh_cn;
pub const PERSONAL_ANALYTICS_SYSTEM_EN: &str = PERSONAL_ANALYTICS_SYSTEM.en;
pub const PERSONAL_ANALYTICS_USER_ZH_CN: &str = PERSONAL_ANALYTICS_USER.zh_cn;
pub const PERSONAL_ANALYTICS_USER_EN: &str = PERSONAL_ANALYTICS_USER.en;
pub const PERSONAL_ANALYTICS_REPAIR_SYSTEM_ZH_CN: &str = PERSONAL_ANALYTICS_REPAIR_SYSTEM.zh_cn;
pub const PERSONAL_ANALYTICS_REPAIR_SYSTEM_EN: &str = PERSONAL_ANALYTICS_REPAIR_SYSTEM.en;
pub const PERSONAL_ANALYTICS_REPAIR_USER_ZH_CN: &str = PERSONAL_ANALYTICS_REPAIR_USER.zh_cn;
pub const PERSONAL_ANALYTICS_REPAIR_USER_EN: &str = PERSONAL_ANALYTICS_REPAIR_USER.en;

pub const BUNDLED_PROMPTS: &[(&str, LocalizedText)] = &[
    ("profile/plan.md", PROFILE_PLAN),
    ("profile/dev.md", PROFILE_DEV),
    ("profile/dev-test.md", PROFILE_DEV_TEST),
    ("profile/review.md", PROFILE_REVIEW),
    ("profile/test.md", PROFILE_TEST),
    ("profile/cicd.md", PROFILE_CICD),
    ("profile/accept.md", PROFILE_ACCEPT),
    ("profile/clean.md", PROFILE_CLEAN),
    ("profile/interview.md", PROFILE_INTERVIEW),
    ("profile/GrillMe.md", PROFILE_GRILLME),
    (
        "profile/overlays/requirement-identity.md",
        PROFILE_OVERLAY_REQUIREMENT_IDENTITY,
    ),
    (
        "profile/overlays/dev-test-auto-commit.md",
        PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT,
    ),
    ("runtime/system.md", RUNTIME_SYSTEM),
    ("runtime/hidden_context.md", RUNTIME_HIDDEN_CONTEXT),
    ("runtime/user.md", RUNTIME_USER),
    (
        "runtime/invalid_output_repair.md",
        RUNTIME_INVALID_OUTPUT_REPAIR,
    ),
    (
        "runtime/scheduled_task_context.md",
        RUNTIME_SCHEDULED_TASK_CONTEXT,
    ),
    ("runtime/artifact_finalize.md", RUNTIME_ARTIFACT_FINALIZE),
    ("runtime/runtime_control_resume.md", RUNTIME_CONTROL_RESUME),
    (
        "runtime/runtime_control_resume_with_message.md",
        RUNTIME_CONTROL_RESUME_WITH_MESSAGE,
    ),
    ("runtime/workflow_resume.md", RUNTIME_WORKFLOW_RESUME),
    ("runtime/user_role_message.md", RUNTIME_USER_ROLE_MESSAGE),
    ("runtime/cicd-goal.md", CICD_GOAL),
    (
        "runtime/ai-dynamic/proposal_repair.md",
        AI_DYNAMIC_PROPOSAL_REPAIR,
    ),
    ("runtime/ai-dynamic/fanout.md", AI_DYNAMIC_FANOUT),
    ("runtime/ai-dynamic/merge.md", AI_DYNAMIC_MERGE),
    ("runtime/ai-dynamic/acceptance.md", AI_DYNAMIC_ACCEPTANCE),
    ("runtime/ai-dynamic/node_task.md", AI_DYNAMIC_NODE_TASK),
    (
        "runtime/ai-dynamic/hidden_context.md",
        AI_DYNAMIC_HIDDEN_CONTEXT,
    ),
    (
        "runtime/ai-dynamic/workflow_invocation.md",
        AI_DYNAMIC_WORKFLOW_INVOCATION,
    ),
    ("runtime/ai-dynamic/system.md", AI_DYNAMIC_SYSTEM),
    (
        "runtime/ai-dynamic/output_protocol.md",
        AI_DYNAMIC_OUTPUT_PROTOCOL,
    ),
    ("personal-analytics/system.md", PERSONAL_ANALYTICS_SYSTEM),
    ("personal-analytics/user.md", PERSONAL_ANALYTICS_USER),
    (
        "personal-analytics/repair_system.md",
        PERSONAL_ANALYTICS_REPAIR_SYSTEM,
    ),
    (
        "personal-analytics/repair_user.md",
        PERSONAL_ANALYTICS_REPAIR_USER,
    ),
    ("runtime/memory-rules.md", MEMORY_RULES),
    ("runtime/memory-tools.json", MEMORY_TOOLS),
];

pub fn prompt_by_language(language: DesktopLanguage, text: LocalizedText) -> &'static str {
    text.resolve(language)
}

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

    #[test]
    fn bundled_prompts_keep_marker_order_and_cicd_falls_back_to_english() {
        assert_eq!(PROFILE_CICD.resolve(DesktopLanguage::JaJp), PROFILE_CICD.en);
        assert_eq!(CICD_GOAL.resolve(DesktopLanguage::ZhTw), CICD_GOAL.en);
        assert_ne!(PROFILE_PLAN.resolve(DesktopLanguage::JaJp), PROFILE_PLAN.en);
        assert_ne!(
            PROFILE_PLAN.resolve(DesktopLanguage::ZhTw),
            PROFILE_PLAN.zh_cn
        );

        for (path, text) in BUNDLED_PROMPTS {
            let english_markers = template_markers(text.en);
            for (language, value) in [
                ("zh-CN", Some(text.zh_cn)),
                ("zh-TW", text.zh_tw),
                ("ja-JP", text.ja_jp),
                ("ko-KR", text.ko_kr),
                ("pt-BR", text.pt_br),
                ("es", text.es),
            ] {
                let Some(value) = value else {
                    continue;
                };
                assert_eq!(
                    template_markers(value),
                    english_markers,
                    "{path} {language} MiniJinja markers diverged"
                );
                assert!(
                    !value.to_ascii_lowercase().contains("wb"),
                    "{path} {language} contains a channel token"
                );
            }
            if *path == "runtime/memory-tools.json" {
                let english_keys = json_object_keys(text.en);
                for value in [
                    text.zh_cn,
                    text.zh_tw.unwrap(),
                    text.ja_jp.unwrap(),
                    text.ko_kr.unwrap(),
                    text.pt_br.unwrap(),
                    text.es.unwrap(),
                ] {
                    assert_eq!(json_object_keys(value), english_keys);
                }
            }
        }
    }

    fn template_markers(text: &str) -> Vec<String> {
        let mut markers = Vec::new();
        let mut rest = text;
        loop {
            let open_expr = rest.find("{{");
            let open_stmt = rest.find("{%");
            let start = match (open_expr, open_stmt) {
                (Some(expr), Some(stmt)) => expr.min(stmt),
                (Some(expr), None) => expr,
                (None, Some(stmt)) => stmt,
                (None, None) => break,
            };
            let after = &rest[start..];
            let close = if after.starts_with("{{") { "}}" } else { "%}" };
            let end = after.find(close).expect("MiniJinja marker closes");
            markers.push(after[..end + close.len()].to_string());
            rest = &after[end + close.len()..];
        }
        markers
    }

    fn json_object_keys(text: &str) -> Vec<String> {
        let mut keys = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text)
            .expect("memory tool descriptions are json")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }
}
