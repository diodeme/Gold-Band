use crate::domain::PauseReason;
use serde::{Deserialize, Serialize};

/// 干预通知，由 orchestrator → 桌面回调 → OS 通知系统
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionNotification {
    /// 去重键：run_id:node_id:attempt_id:pause_reason
    pub dedup_key: String,
    pub task_id: String,
    pub task_title: Option<String>,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub node_label: String,
    pub pause_reason: PauseReason,
    /// OS 通知标题
    pub title: String,
    /// OS 通知正文
    pub body: String,
    /// 干预类型（影响前端导航目标）
    pub intervention_type: InterventionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterventionType {
    /// manual_check_pending → submit_manual_check(success/failure)
    ManualCheck,
    /// provider 权限请求 → respond_acp_permission
    PermissionRequest,
    /// 错误阻塞／进程中断 → retry_run / kill_run
    ErrorBlocked,
}

impl InterventionNotification {
    pub fn new(
        task_id: &str,
        task_title: Option<&str>,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        node_label: &str,
        pause_reason: PauseReason,
    ) -> Self {
        let dedup_key = format!(
            "{}:{}:{}:{}:{:?}",
            run_id, round_id, node_id, attempt_id, pause_reason
        );
        let task_label = task_title.unwrap_or(task_id);
        let (title, body, intervention_type) = match pause_reason {
            PauseReason::WaitingForUserInput => (
                "工作流需要人工确认".to_string(),
                format!(
                    "任务 \"{task_label}\" 的节点 \"{node_label}\" 已完成执行，等待确认结果"
                ),
                InterventionType::ManualCheck,
            ),
            PauseReason::PermissionRequested => (
                "权限请求".to_string(),
                format!("任务 \"{task_label}\" 的节点 \"{node_label}\" 需要授权确认"),
                InterventionType::PermissionRequest,
            ),
            PauseReason::ErrorBlocked => (
                "工作流执行错误".to_string(),
                format!(
                    "任务 \"{task_label}\" 的节点 \"{node_label}\" 因错误被阻塞，需要手动处理"
                ),
                InterventionType::ErrorBlocked,
            ),
            PauseReason::ProcessInterrupted => (
                "工作流进程中断".to_string(),
                format!(
                    "任务 \"{task_label}\" 的节点 \"{node_label}\" 进程中断，需要手动处理"
                ),
                InterventionType::ErrorBlocked,
            ),
        };
        Self {
            dedup_key,
            task_id: task_id.to_string(),
            task_title: task_title.map(String::from),
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            node_label: node_label.to_string(),
            pause_reason,
            title,
            body,
            intervention_type,
        }
    }
}
