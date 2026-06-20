//! 系统级干预通知的数据模型与去重器。
//!
//! 本模块是弹窗功能的「数据契约层」：定义 [`InterventionNotification`] /
//! [`InterventionType`] / [`NotificationDedup`]，纯逻辑、不依赖 Tauri，
//! 可被核心库单测覆盖，也可被桌面端（`gold-band-desktop`）复用。
//!
//! 生命周期语义见 `.claude/design/system-notification-intervention-reimpl-plan.md`：
//! 弹窗是一次性提醒，「点掉即消失」，无 resolved 闭环。去重器在用户点掉前拦截
//! 同节点同原因的重复信号，点掉后清理 key，使同节点可再次弹出。

use std::sync::{Mutex, MutexGuard};

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

use crate::domain::{PauseReason, RunOutcome};

use super::RuntimeInterventionKind;

/// 去重表软上限。常驻 EXE 必须有界，达到上限后按最旧淘汰，防止内存无限增长。
pub const NOTIFICATION_DEDUP_SOFT_CAP: usize = 5000;

/// 干预通知类型，决定前端「查看详情」后的导航目标与操作入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterventionType {
    /// 人工确认 → `submit_manual_check`。
    ManualCheck,
    /// 权限请求 → `respond_acp_permission`。
    PermissionRequest,
    /// 执行错误 / 进程中断 → `retry_run` / `kill_run`。
    ErrorBlocked,
    /// 任务完成 → 查看运行结果。
    RunCompleted,
}

/// 一次干预提醒的核心数据契约。
///
/// 同时承载结构化数据（`pause_reason` / `intervention_type`）与已渲染文案
/// （`title` / `body`）。文案与数据分离：前端 VM 与 OS 通知只消费最终字符串，
/// 后续 i18n 升级只需替换 [`InterventionNotification::new`] 内的文案生成逻辑，
/// 不波及数据模型与流程（见方案 §11.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionNotification {
    /// 去重键，格式 `run:round:node:attempt:reason`（统一路径 A/B，不含 request_id）。
    pub dedup_key: String,
    pub task_id: String,
    pub task_title: Option<String>,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    /// 路径 A：节点可读标签；路径 B：node_id 或一般性描述（低成本，方案 §6.2/§9.4）。
    pub node_label: String,
    pub pause_reason: PauseReason,
    /// 通知标题（本次中文硬编码；后续 i18n 由 `new` 按语言生成，方案 §11）。
    pub title: String,
    /// 通知正文，含 node_label 与任务标识（本次中文硬编码）。
    pub body: String,
    pub intervention_type: InterventionType,
}

impl InterventionNotification {
    #[allow(clippy::too_many_arguments)]
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
        let kind = RuntimeInterventionKind::from(pause_reason);
        Self::from_intervention_kind(
            task_id, task_title, run_id, round_id, node_id, attempt_id, node_label, kind,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_intervention_kind(
        task_id: &str,
        task_title: Option<&str>,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        node_label: &str,
        kind: RuntimeInterventionKind,
    ) -> Self {
        let pause_reason = pause_reason_for_intervention(kind);
        let (title, body_template, intervention_type) = match kind {
            RuntimeInterventionKind::ManualDecisionRequired => (
                "人工确认",
                "需要判断是否成功",
                InterventionType::ManualCheck,
            ),
            RuntimeInterventionKind::PermissionRequested => {
                ("权限请求", "需要授权", InterventionType::PermissionRequest)
            }
            RuntimeInterventionKind::ErrorBlocked => (
                "执行错误",
                "执行出错，需要处理",
                InterventionType::ErrorBlocked,
            ),
            RuntimeInterventionKind::ProcessInterrupted => {
                ("进程中断", "进程异常中断", InterventionType::ErrorBlocked)
            }
        };
        let dedup_key = make_dedup_key(run_id, round_id, node_id, attempt_id, pause_reason);
        let task_label = task_title.unwrap_or(task_id);
        let body = format!("{} · {} {}", task_label, node_label, body_template);

        Self {
            dedup_key,
            task_id: task_id.to_string(),
            task_title: task_title.map(str::to_string),
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            node_label: node_label.to_string(),
            pause_reason,
            title: title.to_string(),
            body,
            intervention_type,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run_completed(
        task_id: &str,
        task_title: Option<&str>,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
        node_label: &str,
        outcome: RunOutcome,
    ) -> Self {
        let task_label = task_title.unwrap_or(task_id);
        let outcome_label = match outcome {
            RunOutcome::Success => "已完成",
            RunOutcome::Failure => "执行失败",
            RunOutcome::Killed => "已终止",
        };
        let pause_reason = PauseReason::WaitingForUserInput;
        Self {
            dedup_key: make_completion_dedup_key(run_id, round_id, node_id, attempt_id),
            task_id: task_id.to_string(),
            task_title: task_title.map(str::to_string),
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            node_label: node_label.to_string(),
            pause_reason,
            title: "任务完成".to_string(),
            body: format!("{} · {} {}", task_label, node_label, outcome_label),
            intervention_type: InterventionType::RunCompleted,
        }
    }
}

pub fn pause_reason_for_intervention(kind: RuntimeInterventionKind) -> PauseReason {
    kind.into()
}

/// Dedup suffix used by both `InterventionNotification::run_completed` and
/// `emit_run_completed_lifecycle_event` in orchestrator.
pub const RUN_COMPLETED_DEDUP_SUFFIX: &str = "run-completed";

pub fn make_completion_dedup_key(
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) -> String {
    format!("{run_id}:{round_id}:{node_id}:{attempt_id}:{RUN_COMPLETED_DEDUP_SUFFIX}")
}

/// 生成统一去重键 `run:round:node:attempt:reason`（不含 request_id，方案 §8.2）。
pub fn make_dedup_key(
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    reason: PauseReason,
) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        run_id,
        round_id,
        node_id,
        attempt_id,
        reason_key(reason)
    )
}

/// 返回 `PauseReason` 的稳定键名，与 `PauseReason` 的 serde kebab-case 表示一致。
pub fn reason_key(reason: PauseReason) -> &'static str {
    match reason {
        PauseReason::ProcessInterrupted => "process-interrupted",
        PauseReason::ErrorBlocked => "error-blocked",
        PauseReason::WaitingForUserInput => "waiting-for-user-input",
        PauseReason::PermissionRequested => "permission-requested",
    }
}

/// 有界去重表：在用户点掉前拦截同节点同原因的重复信号。
///
/// - [`NotificationDedup::try_send`]：首次返回 `true` 并记入；已存在返回 `false`。
/// - [`NotificationDedup::clear_key`]：用户点掉时清理单个 key，使同节点可再次弹出。
/// - [`NotificationDedup::clear_run`]：run 终态时批量清理，防常驻 EXE 内存泄漏。
///
/// 软上限淘汰按最旧（最早插入）优先，常驻 EXE 内存有界。Mutex 中毒时容错取内部数据，
/// 不影响主干（方案 §12）。
#[derive(Debug, Default)]
pub struct NotificationDedup {
    sent: Mutex<IndexSet<String>>,
}

impl NotificationDedup {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(IndexSet::new()),
        }
    }

    /// 尝试登记一个 key。`true` 表示首次（应发送通知），`false` 表示已存在（应跳过）。
    pub fn try_send(&self, key: &str) -> bool {
        let mut guard = lock(&self.sent);
        if guard.contains(key) {
            return false;
        }
        insert_bounded(&mut guard, key.to_string());
        true
    }

    /// 清理单个 key（用户点掉时调用），使该节点同原因可再次弹出。
    pub fn clear_key(&self, key: &str) {
        let mut guard = lock(&self.sent);
        guard.shift_remove(key);
    }

    /// 批量清理某 run 的所有 key（run 终态治理）。幂等。
    pub fn clear_run(&self, run_id: &str) {
        let mut guard = lock(&self.sent);
        let prefix = format!("{run_id}:");
        let to_remove: Vec<String> = guard
            .iter()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in to_remove {
            guard.shift_remove(&key);
        }
    }

    /// 当前已登记 key 数量（测试 / 观测用）。
    pub fn len(&self) -> usize {
        lock(&self.sent).len()
    }

    /// 是否为空（测试用）。
    pub fn is_empty(&self) -> bool {
        lock(&self.sent).is_empty()
    }
}

/// 插入并维护软上限：超出时按最旧淘汰。
fn insert_bounded(set: &mut IndexSet<String>, key: String) {
    while set.len() >= NOTIFICATION_DEDUP_SOFT_CAP {
        // 移除最早插入的元素（IndexSet 保持插入顺序）。
        set.shift_remove_index(0);
    }
    set.insert(key);
}

/// 容错获取锁：Mutex 中毒时返回内部数据，避免主干因通知去重失败而中断。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(reason: PauseReason) -> InterventionNotification {
        InterventionNotification::new(
            "task-1",
            Some("登录模块"),
            "run-1",
            "round-1",
            "node-1",
            "attempt-1",
            "登录节点",
            reason,
        )
    }

    // 13.1 数据契约（纯函数）

    #[test]
    fn dedup_key_contains_all_segments() {
        let n = sample(PauseReason::WaitingForUserInput);
        assert_eq!(
            n.dedup_key,
            "run-1:round-1:node-1:attempt-1:waiting-for-user-input"
        );
    }

    #[test]
    fn pause_reason_maps_to_title_and_type() {
        assert_eq!(sample(PauseReason::WaitingForUserInput).title, "人工确认");
        assert!(
            sample(PauseReason::WaitingForUserInput)
                .body
                .contains("需要判断是否成功")
        );
        assert_eq!(
            sample(PauseReason::WaitingForUserInput).intervention_type,
            InterventionType::ManualCheck
        );
        assert_eq!(sample(PauseReason::PermissionRequested).title, "权限请求");
        assert_eq!(
            sample(PauseReason::PermissionRequested).intervention_type,
            InterventionType::PermissionRequest
        );
        assert_eq!(sample(PauseReason::ErrorBlocked).title, "执行错误");
        assert_eq!(
            sample(PauseReason::ErrorBlocked).intervention_type,
            InterventionType::ErrorBlocked
        );
        assert_eq!(sample(PauseReason::ProcessInterrupted).title, "进程中断");
        assert_eq!(
            sample(PauseReason::ProcessInterrupted).intervention_type,
            InterventionType::ErrorBlocked
        );
    }

    #[test]
    fn error_blocked_and_process_interrupted_share_type_differ_title() {
        let err = sample(PauseReason::ErrorBlocked);
        let interrupted = sample(PauseReason::ProcessInterrupted);
        assert_eq!(err.intervention_type, interrupted.intervention_type);
        assert_ne!(err.title, interrupted.title);
    }

    #[test]
    fn run_completed_notification_has_dedicated_type_and_key() {
        let n = InterventionNotification::run_completed(
            "task-1",
            Some("登录模块"),
            "run-1",
            "round-1",
            "node-1",
            "attempt-1",
            "登录节点",
            RunOutcome::Success,
        );
        assert_eq!(n.title, "任务完成");
        assert_eq!(n.intervention_type, InterventionType::RunCompleted);
        assert_eq!(n.dedup_key, "run-1:round-1:node-1:attempt-1:run-completed");
    }

    #[test]
    fn task_title_none_falls_back_to_task_id_in_body() {
        let n = InterventionNotification::new(
            "task-9",
            None,
            "run-1",
            "round-1",
            "node-1",
            "attempt-1",
            "节点A",
            PauseReason::ErrorBlocked,
        );
        assert!(n.body.contains("task-9"));
        assert!(!n.body.contains("登录模块"));
    }

    #[test]
    fn different_reason_produces_different_dedup_key() {
        let a = sample(PauseReason::ErrorBlocked).dedup_key;
        let b = sample(PauseReason::WaitingForUserInput).dedup_key;
        assert_ne!(a, b);
    }

    #[test]
    fn body_contains_node_label() {
        let n = sample(PauseReason::WaitingForUserInput);
        assert!(n.body.contains("登录节点"));
    }

    // 13.2 去重器

    #[test]
    fn try_send_dedups_same_key_once() {
        let dedup = NotificationDedup::new();
        let key = "run-1:round-1:node-1:attempt-1:error-blocked";
        assert!(dedup.try_send(key));
        assert!(!dedup.try_send(key));
        assert_eq!(dedup.len(), 1);
    }

    #[test]
    fn clear_key_allows_resend() {
        let dedup = NotificationDedup::new();
        let key = "run-1:round-1:node-1:attempt-1:waiting-for-user-input";
        assert!(dedup.try_send(key));
        assert!(!dedup.try_send(key));
        dedup.clear_key(key);
        // 点掉后同 key 可再次弹出 —— 核心契约。
        assert!(dedup.try_send(key));
    }

    #[test]
    fn clear_key_does_not_affect_other_keys() {
        let dedup = NotificationDedup::new();
        let a = "run-1:round-1:node-1:attempt-1:error-blocked";
        let b = "run-1:round-1:node-2:attempt-1:error-blocked";
        assert!(dedup.try_send(a));
        assert!(dedup.try_send(b));
        dedup.clear_key(a);
        assert!(!dedup.try_send(b), "clear_key 不应影响其他 key");
        assert!(dedup.try_send(a), "被清理的 key 应可再次发送");
    }

    #[test]
    fn clear_run_evicts_all_keys_of_run() {
        let dedup = NotificationDedup::new();
        dedup.try_send("run-1:round-1:node-1:attempt-1:error-blocked");
        dedup.try_send("run-1:round-2:node-2:attempt-1:waiting-for-user-input");
        dedup.try_send("run-2:round-1:node-1:attempt-1:error-blocked");
        dedup.clear_run("run-1");
        assert!(dedup.try_send("run-1:round-1:node-1:attempt-1:error-blocked"));
        assert!(dedup.try_send("run-1:round-2:node-2:attempt-1:waiting-for-user-input"));
        // 其他 run 的 key 不受影响。
        assert!(!dedup.try_send("run-2:round-1:node-1:attempt-1:error-blocked"));
    }

    #[test]
    fn clear_run_does_not_match_prefix_only_run_ids() {
        let dedup = NotificationDedup::new();
        dedup.try_send("run-1x:round-1:node-1:attempt-1:error-blocked");
        dedup.clear_run("run-1");
        // "run-1:" 不应误清 "run-1x:..."。
        assert!(!dedup.try_send("run-1x:round-1:node-1:attempt-1:error-blocked"));
    }

    #[test]
    fn soft_cap_evicts_oldest() {
        let dedup = NotificationDedup::new();
        // 填满到软上限。
        for i in 0..NOTIFICATION_DEDUP_SOFT_CAP {
            assert!(dedup.try_send(&format!("run-{i}:r:n:a:error-blocked")));
        }
        assert_eq!(dedup.len(), NOTIFICATION_DEDUP_SOFT_CAP);
        // 插入第 cap+1 个应淘汰最旧的 run-0。
        assert!(dedup.try_send("run-new:r:n:a:error-blocked"));
        assert_eq!(dedup.len(), NOTIFICATION_DEDUP_SOFT_CAP);
        assert!(
            dedup.try_send("run-0:r:n:a:error-blocked"),
            "最旧的 key 应已被淘汰，可再次登记"
        );
    }
}
