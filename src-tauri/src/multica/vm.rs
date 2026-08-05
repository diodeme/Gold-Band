//! multica 远程任务展示 VM（开发设计 2.4 / line 650-658 TS 接口）。
//!
//! `RemoteTaskVm` / `RemoteConversationSidebarVm` 对齐 `ConversationSidebarVm` 形状
//! （`workspaces` / `tasksByWorkspace` / `pinnedTasks` 键名一致），前端复用 ConversationSidebar
//! 骨架直接渲染，TaskRow 零改复用。

use std::collections::BTreeMap;

use gold_band::config::MulticaWorkspaceRef;
use serde::Serialize;

use crate::multica::client::RemoteTask;

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
}

/// 远程任务列表 sidebar（对齐 ConversationSidebarVm 形状，line 652-658）。
///
/// - `tasks_by_workspace`：远程 queued，按 workspace 分组（key = workspace id）。
/// - `pinned_tasks`：本地失败回显（`multica_pending_issues`），retryable=true，不归属具体 workspace。
/// - `connected`：未连接 → 前端显示空状态 + 连接入口（不另查 patSet）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationSidebarVm {
    pub workspaces: Vec<MulticaWorkspaceRef>,
    pub tasks_by_workspace: BTreeMap<String, Vec<RemoteTaskVm>>,
    pub pinned_tasks: Vec<RemoteTaskVm>,
    /// 终态快照（multica_completed_tasks → 本地历史），远程 tab「最近完成」分区，点击直达本地会话。
    pub recently_completed: Vec<MulticaCompletedTaskVm>,
    pub last_active_workspace_id: Option<String>,
    pub connected: bool,
}

/// `start_multica_remote_task` 启动结果（camelCase → `{localTaskId, runId}`）。
///
/// 与 `create_conversation_run` 对齐：既回本地 task id（侧栏 key / 磁盘 `<id>/`），
/// 又回 run id（直达会话页 `conversation-run`）。之前只回 local_task_id 而丢弃
/// 内部全程持有的 local_run_id，导致前端无法按 run 直达会话（Issue 3 根因之一）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaRemoteTaskStartedVm {
    pub local_task_id: String,
    pub run_id: String,
}

/// 远程 tab「最近完成」回看行（Issue 3C，对齐 `onSelectRun(projectId, taskId, runId)` 直达本地会话）。
///
/// `project_id` 由 `workspace_id → local_project_id`（经 workspaces 列表）解析，未绑定/已解绑的
/// workspace 不进列表（无可直达的本地会话）。`status` = `completed` | `failed`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaCompletedTaskVm {
    pub remote_task_id: String,
    pub local_task_id: String,
    pub run_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub title: String,
    pub status: String,
    pub completed_at: String,
}

impl RemoteTaskVm {
    /// 远程 pending 列表行（可领取，retryable=false）。`workspace_id` 来自其所属 workspace。
    pub fn from_pending(task: &RemoteTask, workspace_id: &str) -> Self {
        Self::from_remote(task, workspace_id, false)
    }

    /// claim 响应行（领取成功，retryable=false）。
    pub fn from_claimed(task: &RemoteTask, workspace_id: &str) -> Self {
        Self::from_remote(task, workspace_id, false)
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
            title: Some("Fix bug".into()),
            requirement: None,
            last_activity_at: Some("2026-08-04T10:00:00Z".into()),
        };
        let vm = RemoteTaskVm::from_pending(&task, "ws-1");
        assert_eq!(vm.id, "t-1");
        assert_eq!(vm.issue_id.as_deref(), Some("iss-1"));
        assert_eq!(vm.status, "queued");
        assert!(!vm.retryable);
        assert_eq!(vm.workspace_id, "ws-1");
        assert_eq!(vm.title, "Fix bug");
        // auth_token 永不入 VM（执行凭证不回显）。
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
            title: None,
            requirement: None,
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
    }

    #[test]
    fn remote_task_started_vm_serializes_camel_case_keys() {
        let vm = MulticaRemoteTaskStartedVm {
            local_task_id: "task-abc".into(),
            run_id: "run-xyz".into(),
        };
        let json = serde_json::to_value(&vm).unwrap();
        // 锁定 camelCase + 同时回 localTaskId 与 runId（前端按 runId 直达会话）。
        assert_eq!(json["localTaskId"], "task-abc");
        assert_eq!(json["runId"], "run-xyz");
    }

    #[test]
    fn sidebar_vm_serializes_aligned_sidebar_keys() {
        let sidebar = RemoteConversationSidebarVm {
            workspaces: Vec::new(),
            tasks_by_workspace: BTreeMap::new(),
            pinned_tasks: Vec::new(),
            recently_completed: Vec::new(),
            last_active_workspace_id: Some("ws-1".into()),
            connected: true,
        };
        let json = serde_json::to_value(&sidebar).unwrap();
        // 键名与 ConversationSidebarVm 一致（前端复用骨架，line 652-658）。
        assert!(json["workspaces"].is_array());
        assert!(json["tasksByWorkspace"].is_object());
        assert!(json["pinnedTasks"].is_array());
        assert!(json["recentlyCompleted"].is_array());
        assert_eq!(json["lastActiveWorkspaceId"], "ws-1");
        assert_eq!(json["connected"], true);
    }

    #[test]
    fn completed_task_vm_serializes_camel_case_keys() {
        let vm = MulticaCompletedTaskVm {
            remote_task_id: "rt-1".into(),
            local_task_id: "task-1".into(),
            run_id: "run-1".into(),
            workspace_id: "ws-1".into(),
            project_id: "proj-1".into(),
            title: "Done thing".into(),
            status: "completed".into(),
            completed_at: "2026-08-06T01:23:45Z".into(),
        };
        let json = serde_json::to_value(&vm).unwrap();
        // 锁定 camelCase 键（前端 onSelectRun 按 projectId/taskId/runId 直达会话）。
        assert_eq!(json["remoteTaskId"], "rt-1");
        assert_eq!(json["localTaskId"], "task-1");
        assert_eq!(json["runId"], "run-1");
        assert_eq!(json["workspaceId"], "ws-1");
        assert_eq!(json["projectId"], "proj-1");
        assert_eq!(json["title"], "Done thing");
        assert_eq!(json["status"], "completed");
        assert_eq!(json["completedAt"], "2026-08-06T01:23:45Z");
    }
}
