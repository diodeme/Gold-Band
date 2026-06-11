use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use gold_band::app::notification::InterventionNotification;
use tauri::{AppHandle, Emitter};

/// 通知去重器：确保同一暂停原因只发一次 OS 通知
pub struct NotificationDedup {
    sent: Mutex<HashSet<String>>,
}

impl NotificationDedup {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(HashSet::new()),
        }
    }

    /// 检查通知是否已发送，如未发送则标记并返回 true
    pub fn try_send(&self, dedup_key: &str) -> bool {
        let mut sent = self.sent.lock().unwrap();
        if sent.contains(dedup_key) {
            return false;
        }
        sent.insert(dedup_key.to_string());
        true
    }

    /// 干预已解决，清除该节点的所有去重记录
    pub fn clear_node(
        &self,
        run_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) {
        let prefix = format!("{}:{}:{}", run_id, node_id, attempt_id);
        self.sent
            .lock()
            .unwrap()
            .retain(|key| !key.starts_with(&prefix));
    }
}

/// 发送系统级 OS 通知并 emit 事件到前端
///
/// 从 orchestrator 回调中调用，直接在 Rust 层发送 OS 通知。
pub fn send_intervention_notification(
    app_handle: &AppHandle,
    dedup: &NotificationDedup,
    notification: &InterventionNotification,
) {
    if !dedup.try_send(&notification.dedup_key) {
        return; // 已发送过
    }

    // ① 系统 OS 通知
    let _ = notify_rust::Notification::new()
        .appname("Gold Band")
        .summary(&notification.title)
        .body(&notification.body)
        .timeout(60_000) // 1 分钟自动消失
        .show();

    // ② emit 事件到前端，用于管理通知队列和导航
    let _ = app_handle.emit("gold-band://intervention-required", notification);
}

/// 通知前端干预已解决
pub fn emit_intervention_resolved(
    app_handle: &AppHandle,
    dedup: &NotificationDedup,
    run_id: &str,
    node_id: &str,
    attempt_id: &str,
) {
    dedup.clear_node(run_id, node_id, attempt_id);
    let _ = app_handle.emit(
        "gold-band://intervention-resolved",
        serde_json::json!({
            "runId": run_id,
            "nodeId": node_id,
            "attemptId": attempt_id,
        }),
    );
}
