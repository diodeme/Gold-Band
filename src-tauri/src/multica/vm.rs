//! multica 远程任务展示 VM（开发设计 2.4 / line 650-658 TS 接口）。
//!
//! `RemoteTaskVm` / `RemoteConversationSidebarVm` 对齐 `ConversationSidebarVm` 形状
//! （`workspaces` / `tasksByWorkspace` 键名一致），前端复用 ConversationSidebar
//! 骨架直接渲染，TaskRow 零改复用。

use std::collections::BTreeMap;

use gold_band::config::{DesktopLanguage, RemoteCompletedTask, RemoteWorkspaceRef};
use gold_band::prompts::{
    RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL, RUNTIME_REMOTE_TASK_CONTEXT,
    RUNTIME_REMOTE_TASK_PARENT_OUTPUT, prompt_by_language, render,
};
use gold_band::provider::PromptHiddenSection;
use serde::Serialize;

use crate::multica::client::RemoteTask;
use crate::multica::state::ActiveRemoteRun;

/// 远程任务行（camelCase，对齐 line 650 TS）。`auth_token` 永不入 VM（不回显执行凭证）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTaskVm {
    pub id: String,
    /// 来源侧工作项引用（翻译自 wire `issue_id`，中立词汇 `issue_ref`，不暴露 issue 语义）。
    pub issue_ref: Option<String>,
    /// `queued` | `running` | `completed` | `failed`（由 `normalize_remote_status` 归一）。
    pub status: String,
    pub workspace_id: String,
    pub title: String,
    pub last_activity_at: Option<String>,
    /// 任务详情（claim-at-send 只读拉取 / claim 响应）才回填：远程任务需求正文（来自 `requirement_text()`），供 composer 预填输入框。
    ///
    /// pending 列表不回填（server pending 只给 thread_name，正文仅在任务详情里），
    /// 故 `from_pending` 留 None，`from_detail` 才覆盖。前端按 Some/None 决定是否预填。
    pub requirement: Option<String>,
    /// 本地 run 链接，供前端整行点击直达本地 conversation-run。
    /// queued 行（`from_pending`）恒 None（无可直达的本地会话）；running 行（`from_active_run`，
    /// 改动七：执行中任务也留在侧栏）与终态行（`from_completed`）构造时填入，其余构造器留 None。
    pub local_task_id: Option<String>,
    pub run_id: Option<String>,
    pub project_id: Option<String>,
    /// 工作项类型（中立词汇 `kind`，翻译自 wire `issue_kind`；`dev`|`test`|`bug`|`general`，
    /// story dev/test 拆分）。序列化为 `kind: string | null`。
    ///
    /// pending / detail 透传 wire 字段；active / completed 行取本地快照（`ActiveRemoteRun` /
    /// `RemoteCompletedTask` 的对应字段）。旧 server 不发、旧历史缺字段 → None，前端不渲染徽标。
    pub kind: Option<String>,
    /// test 任务就绪标记（服务端派生，翻译自 wire `is_ready`）。序列化为 `readiness: boolean | null`。
    ///
    /// 仅 pending / detail 行有意义（queued 才有「能否执行」的问题）；active（已领取执行中）
    /// 与 completed（终态）行恒 None，前端不渲染就绪态。旧 server 不发 → None。
    pub readiness: Option<bool>,
}

/// 远程任务列表 sidebar（对齐 ConversationSidebarVm 形状，line 652-658）。
///
/// - `tasks_by_workspace`：远程任务（active + 终态），按 workspace 分组（key = workspace id）。
///   终态行来自本地 `remote_completed_tasks` 历史，按 `workspace_id` 归入对应工作空间（改动六：
///   取代扁平全局「最近完成」桶，提升可读性；终态行带 `local_task_id`/`run_id`/`project_id` 可直达会话）。
/// - `connected`：未连接 → 前端显示空状态 + 连接入口（不另查 patSet）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationSidebarVm {
    /// 当前任务来源（`desktop_remote_task_source` 指针取值，如 "multica"）——前端来源选择器
    /// 与 draft 绑定的通用层路由键（不变量 3）。
    pub source: String,
    pub workspaces: Vec<RemoteWorkspaceRef>,
    pub tasks_by_workspace: BTreeMap<String, Vec<RemoteTaskVm>>,
    pub last_active_workspace_id: Option<String>,
    pub connected: bool,
}

impl RemoteTaskVm {
    /// 远程 pending 列表行（可领取）。`workspace_id` 来自其所属 workspace。
    pub fn from_pending(task: &RemoteTask, workspace_id: &str) -> Self {
        Self::from_remote(task, workspace_id)
    }

    /// 任务详情行（claim-at-send 只读拉取 / claim 响应）。回填 `requirement`（正文仅任务详情端点才有：
    /// pending 列表只给 thread_name）为 composer 预填 = **DPMS 溯源块（可选）+ 上游交付说明块（可选）+
    /// 需求正文**（见 [`detail_prefill`]）。溯源块（发布计划 / 负责人 / 需求链接）与上游交付说明（父任务
    /// 完成时留下的交付内容，子任务赖以执行）都是用户需在发送前核对的信息，与需求正文一起构成用户可见
    /// 可编辑的起始输入；完成输出协议不进预填，由 [`remote_task_hidden_section`] 在发送时隐式注入首条
    /// prompt。read 与 claim 共用此构造——二者响应同构、都带 requirement 来源字段；区别仅在调用时机
    /// （read 不改 server 状态、任务仍 queued；claim 置 dispatched）。
    pub fn from_detail(task: &RemoteTask, workspace_id: &str, language: DesktopLanguage) -> Self {
        let mut vm = Self::from_remote(task, workspace_id);
        vm.requirement = detail_prefill(task, language);
        vm
    }

    /// 终态回看行（`remote_completed_tasks` 本地历史，改动六）。`completed_at` → `last_activity_at`
    /// （前端复用既有时间渲染；终态时间即「最近活动」），并填入本地 run 链接供整行点击直达会话。
    /// `project_id` 由调用方经 workspaces 列表解析（未绑定 workspace 不进列表，调用前已过滤）。
    pub fn from_completed(c: &RemoteCompletedTask, project_id: &str) -> Self {
        Self {
            id: c.remote_task_id.clone(),
            issue_ref: c.issue_ref.clone(),
            // 本地历史 status 已是归一值（"completed" | "failed"），原样透传。
            status: c.status.clone(),
            workspace_id: c.workspace_id.clone(),
            title: c
                .title
                .trim()
                .is_empty()
                .then(|| c.remote_task_id.clone())
                .unwrap_or_else(|| c.title.clone()),
            last_activity_at: Some(c.completed_at.clone()),
            requirement: None,
            local_task_id: Some(c.local_task_id.clone()),
            run_id: Some(c.local_run_id.clone()),
            project_id: Some(project_id.to_string()),
            // 类型取本地快照（multica C1：终态行类型徽标不丢）；终态无「就绪」语义 → 恒 None。
            kind: c.kind.clone(),
            readiness: None,
        }
    }

    /// 在飞执行行（`active_runs` 内存态，改动七）。status 固定 `running`——任务已被本 runtime 领取并 start，
    /// 处在执行中（既不在 server pending 池、也未进终态历史），补全后侧栏覆盖任务全生命周期
    /// （待领取 → 进行中 → 已完成）。`last_activity_at` 取 `started_at`（在飞任务的「最近活动」即启动时刻），
    /// 并填入本地 run 链接供整行点击直达进行中的会话（与终态行同路径）。title 空 → remote_task_id 兜底。
    /// `project_id` 由调用方经 workspaces 列表解析（与 `from_completed` 一致）。
    pub fn from_active_run(remote_task_id: &str, run: &ActiveRemoteRun, project_id: &str) -> Self {
        Self {
            id: remote_task_id.to_string(),
            issue_ref: run.issue_id.clone(),
            status: "running".to_string(),
            workspace_id: run.workspace_id.clone(),
            title: run
                .title
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| remote_task_id.to_string()),
            last_activity_at: Some(run.started_at.clone()),
            requirement: None,
            local_task_id: Some(run.local_task_id.clone()),
            run_id: Some(run.local_run_id.clone()),
            project_id: Some(project_id.to_string()),
            // 类型取 claim 时随 ActiveRemoteRun 落的快照；已领取执行中无「就绪」语义 → 恒 None。
            kind: run.issue_kind.clone(),
            readiness: None,
        }
    }

    fn from_remote(task: &RemoteTask, workspace_id: &str) -> Self {
        Self {
            id: task.id.clone(),
            issue_ref: task.issue_id.clone(),
            status: normalize_remote_status(&task.status),
            workspace_id: workspace_id.to_string(),
            // 兑现 client.rs 的兜底约定：thread_name 缺失/空白时用 task id 兜底，
            // 保证列表每行都有可辨识标签（即使 webank 未补全名字也不留空行）。
            title: task
                .title
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| task.id.clone()),
            last_activity_at: task.last_activity_at.clone(),
            // pending 列表无正文来源（server pending 只给 thread_name）；任务详情由 from_detail 覆盖。
            requirement: None,
            local_task_id: None,
            run_id: None,
            project_id: None,
            // wire 字段透传并翻译为中立词汇（from_pending / from_detail 共用本构造；claim-at-send 门控数据源）。
            kind: task.issue_kind.clone(),
            readiness: task.is_ready,
        }
    }
}

/// 远程任务首条 prompt 的隐式上下文区段标题（前端以链接按钮展示、点开右侧工作区只读面板，
/// 与「Gold Band runtime context」等既有区段标题同级）。
pub(crate) const REMOTE_TASK_HIDDEN_SECTION_TITLE: &str = "Gold Band remote task context";

/// composer 预填 = DPMS 溯源块（可选）+ 上游交付说明块（可选）+ 需求正文（来源优先级见
/// [`RemoteTask::requirement_text`]），块间空行分隔。
///
/// 溯源字段全缺省 / 纯空白 → 跳过溯源块；`parent_output` 缺省 / 纯空白 → 跳过上游块（行为与引入
/// 各块前一致，版本解耦）；正文缺失但任一块存在 → 只发块（不丢信息）。分层依据：DPMS 溯源是
/// **用户需在发送前核对的目标环境信息**（发布计划 / 负责人 / 需求链接），上游交付说明是**子任务赖以
/// 执行的父任务交付内容**（部署/验证地址、变更范围、测试要点），二者都归 user 侧可见预填。
fn detail_prefill(task: &RemoteTask, language: DesktopLanguage) -> Option<String> {
    let blocks = [
        dpms_context_block(task, language),
        parent_output_block(task, language),
        task.requirement_text(),
    ];
    let joined = blocks
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
    (!joined.trim().is_empty()).then_some(joined)
}

/// 远程任务首条 prompt 的隐式上下文（M5-bk 引入，2026-09-21 二次修正收敛为两块、三次调整收敛为单块）：
///
/// 仅完成输出协议块（issue 关联任务才注入）——写侧协议指令收尾。
///
/// 其余上下文均不在此列——DPMS 溯源块、上游交付说明块与需求正文由 [`detail_prefill`] 预填 composer、
/// 用户可见可编辑，**同一份上下文只下发一次**（任何块若同时进区段会重复注入）。完成输出协议块**必须**
/// 留在隐式侧：它落在用户可编辑文本里时，误删即静默断链（agent 不再产出 `completion-output`，
/// issue done 时无输出可中继）。协议块属 runtime 决定的稳定执行上下文（AGENTS.md system/user prompt
/// 划分标准），随发送时 claim 响应取值，以 `<hidden>` 区段附在首条 prompt 需求正文之后（对齐
/// `scheduled_task_context` 隐式注入先例；发送前用户不再可见，会话内可点开审计）。
/// 非 issue 关联（协议块缺省）→ None（无非隐式上下文，行为与引入区段前一致）。
pub(crate) fn remote_task_hidden_section(
    task: &RemoteTask,
    language: DesktopLanguage,
) -> Option<PromptHiddenSection> {
    let content = completion_protocol_block(task, language)?;
    Some(PromptHiddenSection {
        title: REMOTE_TASK_HIDDEN_SECTION_TITLE.to_string(),
        content,
    })
}

/// 上游交付说明块（`src/prompts/{zh-CN,en}/runtime/remote_task_parent_output.md` 按语言渲染）。
///
/// wire `parent_output` 缺省 / 纯空白 → None（不渲染块）。三次调整后随 DPMS 溯源与需求正文一起进
/// composer 预填（[`detail_prefill`]，用户可见可编辑），不进隐式区段。纯上下文语义，无门控语义——
/// 是否执行仍按 `is_ready` 判定（父曾 done 又 reopen 时本块仍可能渲染）。
fn parent_output_block(task: &RemoteTask, language: DesktopLanguage) -> Option<String> {
    let parent_output = blank_to_none(task.parent_output.as_deref())?;
    let template = prompt_by_language(
        language,
        RUNTIME_REMOTE_TASK_PARENT_OUTPUT,
    );
    let rendered = render(
        template,
        &ParentOutputTemplateContext {
            parent_output: &parent_output,
        },
    )
    .expect("remote task parent output template renders");
    Some(rendered.trim().to_string())
}

/// 完成输出协议块（`src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md`）。
///
/// **注入条件：issue 关联任务**（`issue_id` 非空白）。非 issue 来源任务（chat / autopilot /
/// quick-create）无 issue done 流转，指令无人消费 → 不注入。不看 `issue_kind`——是否真有下游
/// 子任务无法从 claim 响应得知，顶层无子的输出无人消费、无害（multica 侧「一律带」亦成立）。
fn completion_protocol_block(task: &RemoteTask, language: DesktopLanguage) -> Option<String> {
    blank_to_none(task.issue_id.as_deref())?;
    let template = prompt_by_language(
        language,
        RUNTIME_REMOTE_TASK_COMPLETION_PROTOCOL,
    );
    // 模板无变量（协议指令恒定文案），空上下文渲染即原文。
    let rendered =
        render(template, serde_json::json!({})).expect("remote task completion protocol renders");
    Some(rendered.trim().to_string())
}

/// 上游交付说明模板上下文（minijinja strict：`{{ parent_output }}` 单变量）。
#[derive(Serialize)]
struct ParentOutputTemplateContext<'a> {
    parent_output: &'a str,
}

/// DPMS 溯源块（`src/prompts/{zh-CN,en}/runtime/remote_task_context.md` 按语言渲染）。
///
/// 5 个溯源字段全部缺省 / 纯空白 → None（不渲染块、不拼正文）。字符串字段逐个空白过滤，
/// 与 [`RemoteTask::requirement_text`] 的空白过滤惯例一致。
fn dpms_context_block(task: &RemoteTask, language: DesktopLanguage) -> Option<String> {
    let context = RemoteTaskContextTemplateContext {
        release_plan_id: task.release_plan_id,
        dev_user: blank_to_none(task.dev_user.as_deref()),
        test_user: blank_to_none(task.test_user.as_deref()),
        business_story_id: task.business_story_id,
        origin_url: blank_to_none(task.origin_url.as_deref()),
    };
    if context.is_empty() {
        return None;
    }
    let template = prompt_by_language(
        language,
        RUNTIME_REMOTE_TASK_CONTEXT,
    );
    let rendered = render(template, &context).expect("remote task context template renders");
    Some(rendered.trim().to_string())
}

/// 纯空白字符串 → None（模板按 `{% if %}` 跳过该行）。
fn blank_to_none(value: Option<&str>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty()).map(str::to_string)
}

/// 远程任务溯源模板上下文（minijinja strict：字段恒序列化为 null/值，`{% if null %}` 为假）。
#[derive(Serialize)]
struct RemoteTaskContextTemplateContext {
    release_plan_id: Option<i64>,
    dev_user: Option<String>,
    test_user: Option<String>,
    business_story_id: Option<i64>,
    origin_url: Option<String>,
}

impl RemoteTaskContextTemplateContext {
    fn is_empty(&self) -> bool {
        self.release_plan_id.is_none()
            && self.dev_user.is_none()
            && self.test_user.is_none()
            && self.business_story_id.is_none()
            && self.origin_url.is_none()
    }
}

/// 归一 server 侧状态字符串为 VM 状态枚举（不同 server 版本字段值可能不同）。
///
/// - 含 `fail` → `failed`；含 `run` → `running`；含 `complet`/`succe` → `completed`；
/// - 其余（`queued`/`dispatched`/空）→ `queued`。
///
/// 用词干子串（`complet` 覆盖 complete/completed，`succe` 覆盖 success/succeeded/succeed），
/// 容忍 server 不同版本的状态拼写差异。
fn normalize_remote_status(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("fail") {
        "failed"
    } else if lower.contains("run") {
        "running"
    } else if lower.contains("complet") || lower.contains("succe") {
        "completed"
    } else {
        "queued"
    }
    .to_string()
}

// ── 「从 Multica 同步」SKILL 拉取 VM（设计 §6；camelCase 对齐 web/src/types.ts）──────

/// 同步列表行。`local_state`："new"（本地无，默认勾选）/ "exists"（全局自有库已有清洗后同名
/// 目录，同步将覆盖，默认不勾选）。列表阶段不预取详情/文件（无 N+1），文件数判空在拉取时逐项做。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaSkillListItemVm {
    pub id: String,
    pub name: String,
    pub description: String,
    pub local_state: &'static str,
}

/// 单项同步结果。`outcome`："created" | "overwritten" | "skipped" | "failed"；`reason` 为
/// 结构化错误码（`multica.skill.not-found` 等，前端 i18n 映射）或本地落库错误原文透传。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaPullItemResultVm {
    pub id: String,
    pub name: String,
    pub outcome: &'static str,
    pub reason: Option<String>,
}

/// 同步报告（逐项结果，前端汇总统计 + 逐行展示）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaPullReportVm {
    pub results: Vec<MulticaPullItemResultVm>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_remote_status_maps_known_and_unknown() {
        assert_eq!(normalize_remote_status("queued"), "queued");
        assert_eq!(normalize_remote_status("dispatched"), "queued");
        assert_eq!(normalize_remote_status(""), "queued");
        assert_eq!(normalize_remote_status("RUNNING"), "running");
        assert_eq!(normalize_remote_status("running"), "running");
        assert_eq!(normalize_remote_status("failed"), "failed");
        assert_eq!(normalize_remote_status("FailTimeout"), "failed");
        assert_eq!(normalize_remote_status("completed"), "completed");
        assert_eq!(normalize_remote_status("succeeded"), "completed");
    }

    #[test]
    fn from_pending_maps_fields() {
        let task = RemoteTask {
            id: "t-1".into(),
            issue_id: Some("iss-1".into()),
            status: "queued".into(),
            auth_token: Some("secret".into()),
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Fix bug".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: None,
            last_activity_at: Some("2026-08-04T10:00:00Z".into()),
            issue_kind: Some("dev".into()),
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        let vm = RemoteTaskVm::from_pending(&task, "ws-1");
        assert_eq!(vm.id, "t-1");
        assert_eq!(vm.issue_ref.as_deref(), Some("iss-1"));
        assert_eq!(vm.status, "queued");
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "Fix bug");
        // pending 列表无正文来源 → requirement 留 None（预填只在 claim 后才有）。
        assert!(vm.requirement.is_none());
        // story 拆分：pending 行透传 wire 类型；无就绪标记（dev 不受门控）→ None。
        assert_eq!(vm.kind.as_deref(), Some("dev"));
        assert!(vm.readiness.is_none());
        // auth_token 永不入 VM（执行凭证不回显）。
    }

    #[test]
    fn from_pending_passes_through_test_readiness() {
        // test 行：issue_kind 与 is_ready 均透传（看板置灰 + canClaim 谓词的数据源）。
        let task = RemoteTask {
            id: "t-9".into(),
            issue_id: None,
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Test login".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: None,
            last_activity_at: None,
            issue_kind: Some("test".into()),
            is_ready: Some(false),
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        let vm = RemoteTaskVm::from_pending(&task, "ws-1");
        assert_eq!(vm.kind.as_deref(), Some("test"));
        assert_eq!(vm.readiness, Some(false));

        // detail 同源（claim-at-send 门控读它）——两构造共用 from_remote，一并锁定。
        let detail = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn);
        assert_eq!(detail.kind.as_deref(), Some("test"));
        assert_eq!(detail.readiness, Some(false));
    }

    #[test]
    fn from_detail_fills_requirement_from_source_priority() {
        // 任务详情（claim-at-send read / claim 响应）：requirement 取来源优先级首个非空
        // （quick_create > chat > ... > title）；本任务无 DPMS 字段 → 预填即纯正文。
        let task = RemoteTask {
            id: "t-1".into(),
            issue_id: Some("iss-1".into()),
            status: "queued".into(),
            auth_token: Some("secret".into()),
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Thread name".into()),
            quick_create_prompt: Some("Full prompt body".into()),
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: None,
            last_activity_at: Some("2026-08-04T10:00:00Z".into()),
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        let vm = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn);
        let requirement = vm.requirement.expect("requirement prefilled");
        assert_eq!(requirement, "Full prompt body");
        // 完成输出协议块不进预填（隐式注入，见 remote_task_hidden_section 测试；上游交付说明进预填）。
        assert!(!requirement.contains("completion-output"));
        // title 与 requirement 各司其职（title 仍是 thread_name，不混进正文）。
        assert_eq!(vm.title, "Thread name");

        // issue 场景（无来源字段）→ requirement 回退 title（issue 预填标题）。
        let issue = RemoteTask {
            id: "t-2".into(),
            issue_id: None,
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Login bug".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: None,
            last_activity_at: None,
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        assert_eq!(
            RemoteTaskVm::from_detail(&issue, "ws-1", DesktopLanguage::ZhCn)
                .requirement
                .as_deref(),
            Some("Login bug")
        );

        // issue 带 body（改动四：任务详情端点回填 issue_description）→ requirement 取正文而非标题。
        let issue_body = RemoteTask {
            id: "t-3".into(),
            issue_id: Some("iss-3".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Login bug".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: Some("Steps to repro...".into()),
            last_activity_at: None,
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        let vm_body = RemoteTaskVm::from_detail(&issue_body, "ws-1", DesktopLanguage::ZhCn);
        // 无 DPMS 字段 → 正文即预填全部内容（issue_description 优先级不变；协议块走隐式注入）。
        assert_eq!(vm_body.requirement.as_deref(), Some("Steps to repro..."));
        // title 仍是 thread_name，不混进正文。
        assert_eq!(vm_body.title, "Login bug");
    }

    #[test]
    fn from_detail_prepends_dpms_context_block_to_requirement() {
        // DPMS 溯源字段存在（issue 任务 + server 已同步 DPMS，2026-09-17 任务接口新增字段）
        // → composer 预填 = 溯源块 + 空行 + 需求正文（用户发送前可核对目标环境）。
        let task = RemoteTask {
            id: "t-1".into(),
            issue_id: Some("iss-1".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Fix login".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: Some("Steps to repro...".into()),
            last_activity_at: None,
            issue_kind: Some("dev".into()),
            is_ready: Some(true),
            release_plan_id: Some(538181),
            dev_user: Some("alice,bob".into()),
            test_user: Some("carol".into()),
            business_story_id: Some(674290),
            origin_url: Some("https://dpms.example.com/story/674290".into()),
            parent_output: None,
        };
        let vm = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn);
        let requirement = vm.requirement.expect("requirement prefilled");
        // 溯源块在前、正文在后（用户先核对目标环境，再读/改需求）。
        assert!(requirement.starts_with("本任务来自 DPMS 关联的工作项"));
        assert!(requirement.contains("- 发布计划 ID: 538181"));
        assert!(requirement.contains("- 开发负责人: alice,bob"));
        assert!(requirement.contains("- 测试负责人: carol"));
        assert!(requirement.contains("- 业务需求 ID: 674290"));
        assert!(requirement.contains("- 需求链接: https://dpms.example.com/story/674290"));
        assert!(requirement.ends_with("Steps to repro..."));
        assert!(
            requirement.find("需求溯源信息").unwrap()
                < requirement.find("Steps to repro...").unwrap()
        );

        // 预填不得包含隐式侧协议块（落在用户可编辑文本里 = 误删即静默断链）。
        assert!(!requirement.contains("completion-output"));

        // 英文语言 → 同一数据渲染英文模板（模板双语同构）。
        let requirement_en = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::En)
            .requirement
            .expect("requirement prefilled");
        assert!(requirement_en.starts_with("This task comes from a DPMS-linked work item"));
        assert!(requirement_en.contains("- Release plan ID: 538181"));
        assert!(
            requirement_en.contains("- Requirement link: https://dpms.example.com/story/674290")
        );
        assert!(requirement_en.ends_with("Steps to repro..."));
    }

    #[test]
    fn from_detail_without_dpms_fields_keeps_plain_requirement() {
        // DPMS 字段全缺省（非 issue 任务 / 旧 server / 未同步 DPMS 的 issue）→ 预填退化为纯正文
        // （行为与引入溯源块前一致，版本解耦）；issue 关联时协议块仍走隐式区段。
        let task = RemoteTask {
            id: "t-2".into(),
            issue_id: Some("iss-2".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Login bug".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: Some("Steps...".into()),
            last_activity_at: None,
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        assert_eq!(
            RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn).requirement,
            Some("Steps...".into())
        );

        // 纯空白字符串字段视为缺失（逐字段空白过滤）：只剩一个非空白字段也渲染，空白字段不出行。
        let task_blank = RemoteTask {
            dev_user: Some("   ".into()),
            origin_url: Some("https://dpms.example.com/story/1".into()),
            ..task
        };
        let requirement_blank =
            RemoteTaskVm::from_detail(&task_blank, "ws-1", DesktopLanguage::ZhCn)
                .requirement
                .expect("requirement prefilled");
        assert!(requirement_blank.contains("- 需求链接: https://dpms.example.com/story/1"));
        assert!(!requirement_blank.contains("开发负责人"));

        // 正文缺失但溯源存在 → 只发块（不丢身份信息）。
        let task_block_only = RemoteTask {
            issue_description: None,
            title: None,
            ..task_blank
        };
        let block_only = RemoteTaskVm::from_detail(&task_block_only, "ws-1", DesktopLanguage::ZhCn)
            .requirement
            .expect("dpms block only");
        assert!(block_only.contains("- 需求链接: https://dpms.example.com/story/1"));
        assert!(!block_only.contains("Steps..."));
    }

    #[test]
    fn remote_task_hidden_section_excludes_prefilled_dpms_block() {
        // 同一份上下文只下发一次：DPMS 溯源已在 composer 预填（用户可见），区段里必须不含它，
        // 否则模型会收到两份。区段只承载完成输出协议（上游交付说明亦在预填侧，
        // 见 from_detail_prefills_upstream_handoff_between_dpms_and_body）。
        let task = RemoteTask {
            id: "t-1".into(),
            issue_id: Some("iss-1".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Fix login".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: Some("Steps to repro...".into()),
            last_activity_at: None,
            issue_kind: Some("dev".into()),
            is_ready: Some(true),
            release_plan_id: Some(538181),
            dev_user: Some("alice,bob".into()),
            test_user: Some("carol".into()),
            business_story_id: Some(674290),
            origin_url: Some("https://dpms.example.com/story/674290".into()),
            parent_output: None,
        };
        let section = remote_task_hidden_section(&task, DesktopLanguage::ZhCn)
            .expect("hidden section assembled");
        assert_eq!(section.title, REMOTE_TASK_HIDDEN_SECTION_TITLE);
        let content = section.content;
        assert!(!content.contains("DPMS"));
        assert!(!content.contains("538181"));
        assert!(!content.contains("alice,bob"));
        assert!(!content.contains("Steps to repro..."));
        assert!(content.contains("completion-output"));
        // 英文侧同构（溯源块同样不进区段）。
        let section_en =
            remote_task_hidden_section(&task, DesktopLanguage::En).expect("hidden section");
        assert!(!section_en.content.contains("DPMS"));
        assert!(section_en.content.contains("completion-output"));
    }

    // ===== 上游交付说明预填 + 完成输出协议区段（issue 完成输出传递特性）=====

    #[test]
    fn from_detail_prefills_upstream_handoff_between_dpms_and_body() {
        // 三次调整：上游交付说明（父任务 completion-output）进 composer 预填，位于 DPMS 溯源块之后、
        // 需求正文之前；隐式区段只剩完成输出协议块（同一份上下文只下发一次）。
        let task = RemoteTask {
            id: "t-5".into(),
            issue_id: Some("iss-5".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Fix login".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: Some("Steps to repro...".into()),
            last_activity_at: None,
            issue_kind: Some("test".into()),
            is_ready: Some(true),
            release_plan_id: Some(538181),
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: Some("部署地址: https://t.example.com\n测试要点: 回归登录链路".into()),
        };
        let vm = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn);
        let requirement = vm.requirement.as_deref().expect("requirement prefilled");
        // 预填三段锚点按出现位置严格递增：DPMS 溯源 → 上游交付说明 → 需求正文（正文收尾）。
        let anchors = [
            requirement.find("本任务来自 DPMS").expect("dpms block"),
            requirement
                .find("上游（父工作项）")
                .expect("upstream handoff block"),
            requirement
                .find("Steps to repro...")
                .expect("requirement body"),
        ];
        assert!(anchors.windows(2).all(|w| w[0] < w[1]));
        assert!(requirement.ends_with("Steps to repro..."));
        // 上游交付说明正文渲染进预填（父输出文本不丢，用户发送前可核对）。
        assert!(requirement.contains("部署地址: https://t.example.com"));
        assert!(requirement.contains("测试要点: 回归登录链路"));

        // 隐式区段只剩协议块：不含上游块、不含父输出文本。
        let section =
            remote_task_hidden_section(&task, DesktopLanguage::ZhCn).expect("hidden section");
        assert!(!section.content.contains("上游"));
        assert!(!section.content.contains("部署地址: https://t.example.com"));
        assert!(section.content.contains("completion-output"));

        // 英文语言 → 同一数据渲染英文模板（预填上游块 + 区段协议块双语同构）。
        let requirement_en = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::En)
            .requirement
            .expect("requirement prefilled");
        assert!(requirement_en.contains("upstream (parent) work item"));
        assert!(requirement_en.ends_with("Steps to repro..."));
        let section_en =
            remote_task_hidden_section(&task, DesktopLanguage::En).expect("hidden section");
        assert!(!section_en.content.contains("upstream (parent)"));
        assert!(section_en.content.contains("completion-output"));
    }

    #[test]
    fn from_detail_without_parent_output_omits_upstream_block() {
        // parent_output 缺省（旧 server / 父无输出 / 无父 / 非 issue 任务）→ 预填不渲染上游块，
        // 行为与引入该块前一致（版本解耦）；协议块不受影响仍进区段（issue 关联）。
        let task = RemoteTask {
            id: "t-6".into(),
            issue_id: Some("iss-6".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: Some("Fix login".into()),
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: Some("Steps...".into()),
            last_activity_at: None,
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        let vm = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn);
        assert_eq!(vm.requirement.as_deref(), Some("Steps..."));
        let section =
            remote_task_hidden_section(&task, DesktopLanguage::ZhCn).expect("hidden section");
        assert!(section.content.contains("completion-output"));

        // 纯空白 parent_output 同样视为缺失（与 DPMS 字段的逐字段空白过滤惯例一致）。
        let blank = RemoteTask {
            parent_output: Some("   ".into()),
            ..task
        };
        assert_eq!(
            RemoteTaskVm::from_detail(&blank, "ws-1", DesktopLanguage::ZhCn).requirement,
            Some("Steps...".into())
        );
    }

    #[test]
    fn remote_task_hidden_section_issue_linked_without_body_yields_protocol_only() {
        // 正文缺失但 issue 关联（无来源字段、无 title、无 DPMS 字段）→ 预填 None，
        // 隐式区段只含协议块（issue done 流转的写侧指令不依赖正文存在）。
        let task = RemoteTask {
            id: "t-8".into(),
            issue_id: Some("iss-8".into()),
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: None,
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: None,
            last_activity_at: None,
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        let vm = RemoteTaskVm::from_detail(&task, "ws-1", DesktopLanguage::ZhCn);
        assert_eq!(vm.requirement, None);
        let section = remote_task_hidden_section(&task, DesktopLanguage::ZhCn)
            .expect("protocol-only section");
        assert!(section.content.starts_with("本工作项关联远程工作项"));
        assert!(section.content.contains("completion-output"));

        // 正文缺失但父输出存在 → 预填只剩上游交付说明块（不丢父交付内容；协议块仍在区段）。
        let with_parent_output = RemoteTask {
            parent_output: Some("部署地址: https://t.example.com".into()),
            ..task.clone()
        };
        let prefill = RemoteTaskVm::from_detail(&with_parent_output, "ws-1", DesktopLanguage::ZhCn)
            .requirement
            .expect("upstream-block-only prefill");
        assert!(prefill.starts_with("本任务的上游（父工作项）"));
        assert!(prefill.contains("部署地址: https://t.example.com"));
        assert!(!prefill.contains("completion-output"));
        assert!(
            remote_task_hidden_section(&with_parent_output, DesktopLanguage::ZhCn)
                .expect("protocol section")
                .content
                .starts_with("本工作项关联远程工作项")
        );

        // 全段缺席（非 issue 任务、无任何正文来源）→ 预填与隐式区段均为 None，与历史行为一致。
        let non_issue = RemoteTask {
            issue_id: None,
            ..task
        };
        assert_eq!(
            RemoteTaskVm::from_detail(&non_issue, "ws-1", DesktopLanguage::ZhCn).requirement,
            None
        );
        assert!(remote_task_hidden_section(&non_issue, DesktopLanguage::ZhCn).is_none());
    }

    #[test]
    fn from_pending_falls_back_to_id_when_title_missing() {
        // title: None → 用 task id 兜底（兑现 client.rs 的兜底约定）。
        let mut task = RemoteTask {
            id: "t-7".into(),
            issue_id: None,
            status: "queued".into(),
            auth_token: None,
            prior_session_id: None,
            parent_task_id: None,
            title: None,
            quick_create_prompt: None,
            chat_message: None,
            trigger_comment_content: None,
            autopilot_description: None,
            handoff_note: None,
            issue_description: None,
            last_activity_at: None,
            issue_kind: None,
            is_ready: None,
            release_plan_id: None,
            dev_user: None,
            test_user: None,
            business_story_id: None,
            origin_url: None,
            parent_output: None,
        };
        assert_eq!(RemoteTaskVm::from_pending(&task, "ws-1").title, "t-7");

        // title: Some("  ")（纯空白）→ 同样兜底，不留空标签。
        task.title = Some("   ".into());
        assert_eq!(RemoteTaskVm::from_pending(&task, "ws-1").title, "t-7");

        // title: 非空白 → 原样返回。
        task.title = Some("Real name".into());
        assert_eq!(RemoteTaskVm::from_pending(&task, "ws-1").title, "Real name");
    }

    #[test]
    fn remote_task_vm_serializes_camel_case_keys() {
        let vm = RemoteTaskVm {
            id: "t-1".into(),
            issue_ref: Some("iss-1".into()),
            status: "queued".into(),
            workspace_id: "ws-1".into(),
            title: "Fix bug".into(),
            last_activity_at: Some("2026-08-04T10:00:00Z".into()),
            requirement: Some("prefill body".into()),
            local_task_id: None,
            run_id: None,
            project_id: None,
            kind: Some("test".into()),
            readiness: Some(true),
        };
        let json = serde_json::to_value(&vm).unwrap();
        // 锁定 camelCase 键名（对齐 line 650 TS，前端按这些键取值）。
        assert_eq!(json["id"], "t-1");
        assert_eq!(json["issueRef"], "iss-1");
        assert_eq!(json["status"], "queued");
        assert_eq!(json["workspaceId"], "ws-1");
        assert_eq!(json["title"], "Fix bug");
        assert_eq!(json["lastActivityAt"], "2026-08-04T10:00:00Z");
        assert_eq!(json["requirement"], "prefill body");
        // 中立词汇字段（web/src/types.ts 的 kind / readiness，翻译自 wire issueKind / isReady）。
        assert_eq!(json["kind"], "test");
        assert_eq!(json["readiness"], true);
        // active 行无本地 run 链接（终态行才填）。
        assert!(json["localTaskId"].is_null());
        assert!(json["runId"].is_null());
        assert!(json["projectId"].is_null());
        // retryable / pinnedTasks 死管线已删（M1）：序列化结果不含这些键。
        assert!(json.get("retryable").is_none());
    }

    #[test]
    fn sidebar_vm_serializes_aligned_sidebar_keys() {
        let sidebar = RemoteConversationSidebarVm {
            source: "multica".to_string(),
            workspaces: Vec::new(),
            tasks_by_workspace: BTreeMap::new(),
            last_active_workspace_id: Some("ws-1".into()),
            connected: true,
        };
        let json = serde_json::to_value(&sidebar).unwrap();
        // 键名与 ConversationSidebarVm 一致（前端复用骨架，line 652-658）。
        assert!(json["workspaces"].is_array());
        assert!(json["tasksByWorkspace"].is_object());
        // retryable / pinnedTasks 死管线已删（M1）：sidebar 不再含 pinnedTasks 键。
        assert!(json.get("pinnedTasks").is_none());
        // 改动六：扁平「最近完成」桶已删，终态行并入 tasksByWorkspace 对应工作空间组。
        assert!(json.get("recentlyCompleted").is_none());
        assert_eq!(json["lastActiveWorkspaceId"], "ws-1");
        assert_eq!(json["connected"], true);
    }

    #[test]
    fn from_completed_carries_local_run_link_and_terminal_status() {
        // 终态回看行（改动六）：本地历史 → RemoteTaskVm，带本地 run 链接供整行点击直达会话。
        let c = RemoteCompletedTask {
            remote_task_id: "rt-1".into(),
            local_task_id: "task-1".into(),
            local_run_id: "run-1".into(),
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            issue_ref: Some("iss-1".into()),
            kind: Some("test".into()),
            status: "completed".into(),
            title: "Done thing".into(),
            completed_at: "2026-08-06T01:23:45Z".into(),
        };
        let vm = RemoteTaskVm::from_completed(&c, "proj-1");
        assert_eq!(vm.id, "rt-1");
        assert_eq!(vm.issue_ref.as_deref(), Some("iss-1"));
        assert_eq!(vm.status, "completed"); // 本地历史 status 原样透传
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "Done thing");
        // completed_at → last_activity_at（前端复用既有时间渲染）。
        assert_eq!(vm.last_activity_at.as_deref(), Some("2026-08-06T01:23:45Z"));
        // 本地 run 链接齐备（前端 onSelectRun(projectId, taskId, runId) 直达会话）。
        assert_eq!(vm.local_task_id.as_deref(), Some("task-1"));
        assert_eq!(vm.run_id.as_deref(), Some("run-1"));
        assert_eq!(vm.project_id.as_deref(), Some("proj-1"));
        // story 拆分：终态行类型取本地快照（徽标不丢）；终态无就绪语义 → 恒 None。
        assert_eq!(vm.kind.as_deref(), Some("test"));
        assert!(vm.readiness.is_none());

        // 旧历史条目（缺 kind）→ None，徽标不渲染（无迁移、无兼容层）。
        let legacy = RemoteCompletedTask {
            kind: None,
            ..c.clone()
        };
        let legacy_vm = RemoteTaskVm::from_completed(&legacy, "proj-1");
        assert!(legacy_vm.kind.is_none());
        assert!(legacy_vm.readiness.is_none());

        // title 空白 → 用 remote_task_id 兜底（不留空标签）。
        let blank = RemoteCompletedTask {
            title: "  ".into(),
            ..c.clone()
        };
        assert_eq!(RemoteTaskVm::from_completed(&blank, "proj-1").title, "rt-1");

        // failed 终态同样进列表（status 原样透传）。
        let failed = RemoteCompletedTask {
            status: "failed".into(),
            ..c
        };
        assert_eq!(
            RemoteTaskVm::from_completed(&failed, "proj-1").status,
            "failed"
        );
    }

    #[test]
    fn from_active_run_marks_running_and_carries_local_link() {
        // 改动七：在飞任务（active_runs）→ running 行，带本地 run 链接供整行点击直达进行中的会话。
        let run = ActiveRemoteRun {
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            local_task_id: "task-9".into(),
            local_run_id: "run-9".into(),
            issue_id: Some("iss-9".into()),
            title: Some("In flight".into()),
            started_at: "2026-08-07T03:00:00Z".into(),
            issue_kind: Some("test".into()),
        };
        let vm = RemoteTaskVm::from_active_run("remote-9", &run, "proj-1");
        assert_eq!(vm.id, "remote-9");
        assert_eq!(vm.issue_ref.as_deref(), Some("iss-9"));
        assert_eq!(vm.status, "running"); // 进行中固定标识
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "In flight");
        // started_at → last_activity_at（在飞任务的「最近活动」即启动时刻）。
        assert_eq!(vm.last_activity_at.as_deref(), Some("2026-08-07T03:00:00Z"));
        assert!(vm.requirement.is_none()); // 在飞不回填正文（仅 claim 响应有）
        // 本地 run 链接齐备（前端 onSelectRun(projectId, taskId, runId) 直达进行中会话）。
        assert_eq!(vm.local_task_id.as_deref(), Some("task-9"));
        assert_eq!(vm.run_id.as_deref(), Some("run-9"));
        assert_eq!(vm.project_id.as_deref(), Some("proj-1"));
        // story 拆分：类型取 ActiveRemoteRun 快照（claim 时落盘）；已领取执行中无就绪语义 → 恒 None。
        assert_eq!(vm.kind.as_deref(), Some("test"));
        assert!(vm.readiness.is_none());

        // title 空/纯空白 → remote_task_id 兜底（不留空标签，与其它构造器一致）。
        let blank = ActiveRemoteRun {
            title: None,
            ..run.clone()
        };
        assert_eq!(
            RemoteTaskVm::from_active_run("remote-9", &blank, "proj-1").title,
            "remote-9"
        );
        let ws = ActiveRemoteRun {
            title: Some("   ".into()),
            ..run.clone()
        };
        assert_eq!(
            RemoteTaskVm::from_active_run("remote-9", &ws, "proj-1").title,
            "remote-9"
        );
    }
}
