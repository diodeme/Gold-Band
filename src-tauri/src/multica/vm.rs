//! multica 远程任务展示 VM（开发设计 2.4 / line 650-658 TS 接口）。
//!
//! `RemoteTaskVm` / `RemoteConversationSidebarVm` 对齐 `ConversationSidebarVm` 形状
//! （`workspaces` / `tasksByWorkspace` / `pinnedTasks` 键名一致），前端复用 ConversationSidebar
//! 骨架直接渲染，TaskRow 零改复用。

use std::collections::BTreeMap;

use gold_band::config::{MulticaCompletedTask, MulticaWorkspaceRef};
use serde::Serialize;

use crate::multica::client::RemoteTask;
use crate::multica::state::ActiveRemoteRun;

/// 远程任务行（camelCase，对齐 line 650 TS）。`auth_token` 永不入 VM（不回显执行凭证）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTaskVm {
    pub id: String,
    pub issue_id: Option<String>,
    /// `queued` | `running` | `completed` | `failed`（由 `normalize_remote_status` 归一）。
    pub status: String,
    pub retryable: bool,
    pub workspace_id: String,
    pub title: String,
    pub last_activity_at: Option<String>,
    /// claim 响应才回填：远程任务需求正文（来自 `requirement_text()`），供 composer 预填输入框。
    ///
    /// pending 列表不回填（server pending 只给 thread_name，正文仅在 claim 响应里），
    /// 故 `from_pending` 留 None，`from_claimed` 才覆盖。前端按 Some/None 决定是否预填。
    pub requirement: Option<String>,
    /// 本地 run 链接，供前端整行点击直达本地 conversation-run。
    /// queued 行（`from_pending`）恒 None（无可直达的本地会话）；running 行（`from_active_run`，
    /// 改动七：执行中任务也留在侧栏）与终态行（`from_completed`）构造时填入，其余构造器留 None。
    pub local_task_id: Option<String>,
    pub run_id: Option<String>,
    pub project_id: Option<String>,
}

/// 远程任务列表 sidebar（对齐 ConversationSidebarVm 形状，line 652-658）。
///
/// - `tasks_by_workspace`：远程任务（active + 终态），按 workspace 分组（key = workspace id）。
///   终态行来自本地 `multica_completed_tasks` 历史，按 `workspace_id` 归入对应工作空间（改动六：
///   取代扁平全局「最近完成」桶，提升可读性；终态行带 `local_task_id`/`run_id`/`project_id` 可直达会话）。
/// - `pinned_tasks`：本地失败回显（`multica_pending_issues`），retryable=true，不归属具体 workspace。
/// - `connected`：未连接 → 前端显示空状态 + 连接入口（不另查 patSet）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationSidebarVm {
    pub workspaces: Vec<MulticaWorkspaceRef>,
    pub tasks_by_workspace: BTreeMap<String, Vec<RemoteTaskVm>>,
    pub pinned_tasks: Vec<RemoteTaskVm>,
    pub last_active_workspace_id: Option<String>,
    pub connected: bool,
}

impl RemoteTaskVm {
    /// 远程 pending 列表行（可领取，retryable=false）。`workspace_id` 来自其所属 workspace。
    pub fn from_pending(task: &RemoteTask, workspace_id: &str) -> Self {
        Self::from_remote(task, workspace_id, false)
    }

    /// claim 响应行（领取成功，retryable=false）。回填 `requirement`（正文仅 claim 响应才有）。
    pub fn from_claimed(task: &RemoteTask, workspace_id: &str) -> Self {
        let mut vm = Self::from_remote(task, workspace_id, false);
        vm.requirement = task.requirement_text();
        vm
    }

    /// 失败回显行（`multica_pending_issues`，retryable=true）。`id` = issue_id（rerun 键）。
    pub fn from_failed_issue(issue_id: &str) -> Self {
        Self {
            id: issue_id.to_string(),
            issue_id: Some(issue_id.to_string()),
            status: "failed".to_string(),
            retryable: true,
            workspace_id: String::new(),
            title: issue_id.to_string(),
            last_activity_at: None,
            requirement: None,
            local_task_id: None,
            run_id: None,
            project_id: None,
        }
    }

    /// 终态回看行（`multica_completed_tasks` 本地历史，改动六）。`completed_at` → `last_activity_at`
    /// （前端复用既有时间渲染；终态时间即「最近活动」），并填入本地 run 链接供整行点击直达会话。
    /// `project_id` 由调用方经 workspaces 列表解析（未绑定 workspace 不进列表，调用前已过滤）。
    pub fn from_completed(c: &MulticaCompletedTask, project_id: &str) -> Self {
        Self {
            id: c.remote_task_id.clone(),
            issue_id: c.issue_id.clone(),
            // 本地历史 status 已是归一值（"completed" | "failed"），原样透传。
            status: c.status.clone(),
            retryable: false,
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
            issue_id: run.issue_id.clone(),
            status: "running".to_string(),
            retryable: false,
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
        }
    }

    fn from_remote(task: &RemoteTask, workspace_id: &str, retryable: bool) -> Self {
        Self {
            id: task.id.clone(),
            issue_id: task.issue_id.clone(),
            status: normalize_remote_status(&task.status),
            retryable,
            workspace_id: workspace_id.to_string(),
            // 兑现 client.rs 的兜底约定：thread_name 缺失/空白时用 task id 兜底，
            // 保证列表每行都有可辨识标签（即使 webank 未补全名字也不留空行）。
            title: task
                .title
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| task.id.clone()),
            last_activity_at: task.last_activity_at.clone(),
            // pending 列表无正文来源（server pending 只给 thread_name）；claim 响应由 from_claimed 覆盖。
            requirement: None,
            local_task_id: None,
            run_id: None,
            project_id: None,
        }
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
    fn from_pending_maps_fields_and_marks_not_retryable() {
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
        };
        let vm = RemoteTaskVm::from_pending(&task, "ws-1");
        assert_eq!(vm.id, "t-1");
        assert_eq!(vm.issue_id.as_deref(), Some("iss-1"));
        assert_eq!(vm.status, "queued");
        assert!(!vm.retryable);
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "Fix bug");
        // pending 列表无正文来源 → requirement 留 None（预填只在 claim 后才有）。
        assert!(vm.requirement.is_none());
        // auth_token 永不入 VM（执行凭证不回显）。
    }

    #[test]
    fn from_claimed_fills_requirement_from_source_priority() {
        // claim 响应：requirement 取来源优先级首个非空（quick_create > chat > ... > title）。
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
        };
        let vm = RemoteTaskVm::from_claimed(&task, "ws-1");
        assert_eq!(vm.requirement.as_deref(), Some("Full prompt body"));
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
        };
        assert_eq!(
            RemoteTaskVm::from_claimed(&issue, "ws-1")
                .requirement
                .as_deref(),
            Some("Login bug")
        );

        // issue 带 body（改动四：webank claim 响应回填 issue_description）→ requirement 取正文而非标题。
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
        };
        let vm_body = RemoteTaskVm::from_claimed(&issue_body, "ws-1");
        assert_eq!(vm_body.requirement.as_deref(), Some("Steps to repro..."));
        // title 仍是 thread_name，不混进正文。
        assert_eq!(vm_body.title, "Login bug");
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
    fn from_failed_issue_marks_retryable_and_failed() {
        let vm = RemoteTaskVm::from_failed_issue("iss-9");
        assert_eq!(vm.id, "iss-9");
        assert_eq!(vm.issue_id.as_deref(), Some("iss-9"));
        assert_eq!(vm.status, "failed");
        assert!(vm.retryable);
        // 失败回显不归属具体 workspace（进 pinned_tasks，前端不按 workspace 分组）。
        assert!(vm.workspace_id.is_empty());
    }

    #[test]
    fn remote_task_vm_serializes_camel_case_keys() {
        let vm = RemoteTaskVm {
            id: "t-1".into(),
            issue_id: Some("iss-1".into()),
            status: "queued".into(),
            retryable: false,
            workspace_id: "ws-1".into(),
            title: "Fix bug".into(),
            last_activity_at: Some("2026-08-04T10:00:00Z".into()),
            requirement: Some("prefill body".into()),
            local_task_id: None,
            run_id: None,
            project_id: None,
        };
        let json = serde_json::to_value(&vm).unwrap();
        // 锁定 camelCase 键名（对齐 line 650 TS，前端按这些键取值）。
        assert_eq!(json["id"], "t-1");
        assert_eq!(json["issueId"], "iss-1");
        assert_eq!(json["status"], "queued");
        assert_eq!(json["retryable"], false);
        assert_eq!(json["workspaceId"], "ws-1");
        assert_eq!(json["title"], "Fix bug");
        assert_eq!(json["lastActivityAt"], "2026-08-04T10:00:00Z");
        assert_eq!(json["requirement"], "prefill body");
        // active 行无本地 run 链接（终态行才填）。
        assert!(json["localTaskId"].is_null());
        assert!(json["runId"].is_null());
        assert!(json["projectId"].is_null());
    }

    #[test]
    fn sidebar_vm_serializes_aligned_sidebar_keys() {
        let sidebar = RemoteConversationSidebarVm {
            workspaces: Vec::new(),
            tasks_by_workspace: BTreeMap::new(),
            pinned_tasks: Vec::new(),
            last_active_workspace_id: Some("ws-1".into()),
            connected: true,
        };
        let json = serde_json::to_value(&sidebar).unwrap();
        // 键名与 ConversationSidebarVm 一致（前端复用骨架，line 652-658）。
        assert!(json["workspaces"].is_array());
        assert!(json["tasksByWorkspace"].is_object());
        assert!(json["pinnedTasks"].is_array());
        // 改动六：扁平「最近完成」桶已删，终态行并入 tasksByWorkspace 对应工作空间组。
        assert!(json.get("recentlyCompleted").is_none());
        assert_eq!(json["lastActiveWorkspaceId"], "ws-1");
        assert_eq!(json["connected"], true);
    }

    #[test]
    fn from_completed_carries_local_run_link_and_terminal_status() {
        // 终态回看行（改动六）：本地历史 → RemoteTaskVm，带本地 run 链接供整行点击直达会话。
        let c = MulticaCompletedTask {
            remote_task_id: "rt-1".into(),
            local_task_id: "task-1".into(),
            local_run_id: "run-1".into(),
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            issue_id: Some("iss-1".into()),
            status: "completed".into(),
            title: "Done thing".into(),
            completed_at: "2026-08-06T01:23:45Z".into(),
        };
        let vm = RemoteTaskVm::from_completed(&c, "proj-1");
        assert_eq!(vm.id, "rt-1");
        assert_eq!(vm.issue_id.as_deref(), Some("iss-1"));
        assert_eq!(vm.status, "completed"); // 本地历史 status 原样透传
        assert!(!vm.retryable); // 终态行不可重试（重试走 pinned 失败回显）
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "Done thing");
        // completed_at → last_activity_at（前端复用既有时间渲染）。
        assert_eq!(vm.last_activity_at.as_deref(), Some("2026-08-06T01:23:45Z"));
        // 本地 run 链接齐备（前端 onSelectRun(projectId, taskId, runId) 直达会话）。
        assert_eq!(vm.local_task_id.as_deref(), Some("task-1"));
        assert_eq!(vm.run_id.as_deref(), Some("run-1"));
        assert_eq!(vm.project_id.as_deref(), Some("proj-1"));

        // title 空白 → 用 remote_task_id 兜底（不留空标签）。
        let blank = MulticaCompletedTask { title: "  ".into(), ..c.clone() };
        assert_eq!(RemoteTaskVm::from_completed(&blank, "proj-1").title, "rt-1");

        // failed 终态同样进列表（status 原样透传）。
        let failed = MulticaCompletedTask {
            status: "failed".into(),
            ..c
        };
        assert_eq!(RemoteTaskVm::from_completed(&failed, "proj-1").status, "failed");
    }

    #[test]
    fn from_active_run_marks_running_and_carries_local_link() {
        // 改动七：在飞任务（active_runs）→ running 行，带本地 run 链接供整行点击直达进行中的会话。
        let run = ActiveRemoteRun {
            runtime_id: "rt-rt-1".into(),
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            local_task_id: "task-9".into(),
            local_run_id: "run-9".into(),
            issue_id: Some("iss-9".into()),
            title: Some("In flight".into()),
            started_at: "2026-08-07T03:00:00Z".into(),
        };
        let vm = RemoteTaskVm::from_active_run("remote-9", &run, "proj-1");
        assert_eq!(vm.id, "remote-9");
        assert_eq!(vm.issue_id.as_deref(), Some("iss-9"));
        assert_eq!(vm.status, "running"); // 进行中固定标识
        assert!(!vm.retryable); // 在跑不可重试（取消走 Cancel 按钮，不进 rerun）
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "In flight");
        // started_at → last_activity_at（在飞任务的「最近活动」即启动时刻）。
        assert_eq!(vm.last_activity_at.as_deref(), Some("2026-08-07T03:00:00Z"));
        assert!(vm.requirement.is_none()); // 在飞不回填正文（仅 claim 响应有）
        // 本地 run 链接齐备（前端 onSelectRun(projectId, taskId, runId) 直达进行中会话）。
        assert_eq!(vm.local_task_id.as_deref(), Some("task-9"));
        assert_eq!(vm.run_id.as_deref(), Some("run-9"));
        assert_eq!(vm.project_id.as_deref(), Some("proj-1"));

        // title 空/纯空白 → remote_task_id 兜底（不留空标签，与其它构造器一致）。
        let blank = ActiveRemoteRun { title: None, ..run.clone() };
        assert_eq!(RemoteTaskVm::from_active_run("remote-9", &blank, "proj-1").title, "remote-9");
        let ws = ActiveRemoteRun { title: Some("   ".into()), ..run.clone() };
        assert_eq!(RemoteTaskVm::from_active_run("remote-9", &ws, "proj-1").title, "remote-9");
    }
}
