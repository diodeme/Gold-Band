use std::{fs, thread, time::Duration};

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    acp::events::{
        append_ui_event, current_timestamp, elicitation_response_event, latest_timeline_source_seq,
        load_timeline_items, write_timeline_items,
    },
    storage::{ensure_parent_dir, read_json, write_json},
};

/// 默认 elicitation 超时时间：无超时（与 Claude Code TUI 行为对齐）。
/// 用户可通过取消 session 随时中断等待。
pub const ELICITATION_DEFAULT_TIMEOUT: Duration = Duration::MAX;
const ELICITATION_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 用户决策枚举 —— 杜绝字符串硬编码
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationAction {
    Accept,
    Decline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingElicitationState {
    pub elicitation_id: String,
    pub jsonrpc_id: Value,
    pub message: String,
    pub requested_schema: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationResponseState {
    pub elicitation_id: String,
    pub action: ElicitationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    pub decided_at: String,
}

// ── 文件路径工具 ──

pub fn pending_elicitation_file(attempt_dir: &Utf8Path, elicitation_id: &str) -> Utf8PathBuf {
    attempt_dir.join(format!(
        "acp.elicitation-request.{}.json",
        sanitize_id(elicitation_id)
    ))
}

pub fn elicitation_response_file(attempt_dir: &Utf8Path, elicitation_id: &str) -> Utf8PathBuf {
    attempt_dir.join(format!(
        "acp.elicitation-response.{}.json",
        sanitize_id(elicitation_id)
    ))
}

// ── 写入待处理请求 ──

pub fn write_pending_elicitation(
    attempt_dir: &Utf8Path,
    state: &PendingElicitationState,
) -> Result<()> {
    let path = pending_elicitation_file(attempt_dir, &state.elicitation_id);
    write_json(&path, state)
}

// ── 前端写入响应（由 Tauri command 调用）──

pub fn write_elicitation_response(
    attempt_dir: &Utf8Path,
    elicitation_id: &str,
    action: ElicitationAction,
    content: Option<Value>,
    decided_at: String,
) -> Result<()> {
    let path = elicitation_response_file(attempt_dir, elicitation_id);
    ensure_parent_dir(&path)?;
    write_json(
        &path,
        &ElicitationResponseState {
            elicitation_id: elicitation_id.to_string(),
            action: action.clone(),
            content: content.clone(),
            decided_at,
        },
    )?;
    upsert_elicitation_response_event(attempt_dir, elicitation_id, &action, content)
}

pub fn remove_elicitation_signal_files(attempt_dir: &Utf8Path, elicitation_id: &str) -> Result<()> {
    remove_file_if_exists(&pending_elicitation_file(attempt_dir, elicitation_id))?;
    remove_file_if_exists(&elicitation_response_file(attempt_dir, elicitation_id))
}

// ── Runtime 侧轮询等待响应 ──

pub fn wait_for_elicitation_response(
    attempt_dir: &Utf8Path,
    elicitation_id: &str,
    timeout: Duration,
) -> Result<ElicitationResponseState> {
    let path = elicitation_response_file(attempt_dir, elicitation_id);
    let started_at = std::time::Instant::now();
    loop {
        if path.exists() {
            let response = read_json(&path)?;
            let _ = fs::remove_file(path.as_std_path());
            return Ok(response);
        }
        if is_elicitation_cancel_requested(attempt_dir) {
            return Ok(ElicitationResponseState {
                elicitation_id: elicitation_id.to_string(),
                action: ElicitationAction::Decline,
                content: None,
                decided_at: current_timestamp(),
            });
        }
        if started_at.elapsed() >= timeout {
            return Ok(ElicitationResponseState {
                elicitation_id: elicitation_id.to_string(),
                action: ElicitationAction::Decline,
                content: None,
                decided_at: current_timestamp(),
            });
        }
        thread::sleep(ELICITATION_POLL_INTERVAL);
    }
}

/// 取消所有待处理的 elicitation 请求（在 cancel/错误路径中调用）
pub fn cancel_pending_elicitation_requests(
    attempt_dir: &Utf8Path,
    decided_at: String,
) -> Result<()> {
    let Ok(entries) = fs::read_dir(attempt_dir.as_std_path()) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with("acp.elicitation-request.") || !file_name.ends_with(".json") {
            continue;
        }
        let Ok(path) = Utf8PathBuf::from_path_buf(path) else {
            continue;
        };
        let Ok(pending) = read_json::<PendingElicitationState>(&path) else {
            continue;
        };
        let response_path = elicitation_response_file(attempt_dir, &pending.elicitation_id);
        if response_path.exists() {
            if let Ok(response) = read_json::<ElicitationResponseState>(&response_path) {
                upsert_elicitation_response_event(
                    attempt_dir,
                    &pending.elicitation_id,
                    &response.action,
                    response.content,
                )?;
            }
            continue;
        }
        write_elicitation_response(
            attempt_dir,
            &pending.elicitation_id,
            ElicitationAction::Decline,
            None,
            decided_at.clone(),
        )?;
    }
    Ok(())
}

pub fn upsert_elicitation_response_event(
    attempt_dir: &Utf8Path,
    elicitation_id: &str,
    action: &ElicitationAction,
    content: Option<Value>,
) -> Result<()> {
    let timeline_path = attempt_dir.join("acp.timeline.jsonl");
    let events_path = attempt_dir.join("acp.events.jsonl");
    let source_seq = if timeline_path.exists() || !events_path.exists() {
        latest_timeline_source_seq(&timeline_path) + 1
    } else {
        legacy_event_count(&events_path) + 1
    };
    let action_value = match action {
        ElicitationAction::Accept => "accept",
        ElicitationAction::Decline => "decline",
    };
    let mut event = elicitation_response_event(
        source_seq,
        elicitation_id.to_string(),
        action_value.to_string(),
        content,
    );
    event.started_seq = Some(source_seq);
    event.ended_seq = Some(source_seq);
    event.started_at = Some(event.timestamp.clone());
    event.ended_at = Some(event.timestamp.clone());

    if events_path.exists() && !timeline_path.exists() {
        append_ui_event(&events_path, &event)?;
    }

    let mut items = load_timeline_items(&timeline_path)?;
    if let Some(existing) = items.iter_mut().find(|item| item.id == event.id) {
        *existing = event;
    } else {
        items.push(event);
    }
    items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
    write_timeline_items(&timeline_path, &items)
}
// ── Elicitation-specific cancel mechanism ──
// Separate from permission domain to avoid semantic coupling.

fn elicitation_cancel_request_file(attempt_dir: &Utf8Path) -> Utf8PathBuf {
    attempt_dir.join("acp.elicitation-cancel.json")
}

/// Write a cancel marker file to notify the blocking poll loop.
pub fn request_elicitation_cancel(attempt_dir: &Utf8Path, at: String) -> Result<()> {
    let path = elicitation_cancel_request_file(attempt_dir);
    write_json(&path, &serde_json::json!({ "cancelledAt": at }))
}

/// Clear the cancel marker file.
pub fn clear_elicitation_cancel_request(attempt_dir: &Utf8Path) -> Result<()> {
    let path = elicitation_cancel_request_file(attempt_dir);
    if path.exists() {
        std::fs::remove_file(path.as_std_path())?;
    }
    Ok(())
}

/// Check if cancel has been requested.
pub fn is_elicitation_cancel_requested(attempt_dir: &Utf8Path) -> bool {
    elicitation_cancel_request_file(attempt_dir).exists()
}

/// 根据 elicitation 响应构造 JSON-RPC result
pub fn elicitation_response_result(response: &ElicitationResponseState) -> Value {
    let action_str = match response.action {
        ElicitationAction::Accept => "accept",
        ElicitationAction::Decline => "decline",
    };
    let mut result = serde_json::json!({ "action": action_str });
    if let Some(content) = &response.content {
        result["content"] = content.clone();
    }
    result
}


fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn legacy_event_count(path: &Utf8Path) -> u64 {
    let Ok(content) = fs::read_to_string(path.as_std_path()) else {
        return 0;
    };
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u64
}

fn remove_file_if_exists(path: &Utf8Path) -> Result<()> {
    match fs::remove_file(path.as_std_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn dummy_attempt_dir() -> (TempDir, Utf8PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        (dir, path)
    }

    #[test]
    fn write_and_read_pending_elicitation() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let state = PendingElicitationState {
            elicitation_id: "elicit-abc123".to_string(),
            jsonrpc_id: serde_json::json!(42),
            message: "请选择数据库".to_string(),
            requested_schema: serde_json::json!({"type": "object"}),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        write_pending_elicitation(&attempt_dir, &state).unwrap();
        let path = pending_elicitation_file(&attempt_dir, "elicit-abc123");
        assert!(path.exists());
        let read_back: PendingElicitationState = read_json(&path).unwrap();
        assert_eq!(read_back.elicitation_id, "elicit-abc123");
        assert_eq!(read_back.message, "请选择数据库");
    }

    #[test]
    fn wait_for_elicitation_response_normal() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let elicitation_id = "elicit-test-normal";
        // 先在另一个线程写入响应
        let attempt_dir_clone = attempt_dir.clone();
        let eid = elicitation_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            write_elicitation_response(
                &attempt_dir_clone,
                &eid,
                ElicitationAction::Accept,
                Some(serde_json::json!({"answer": "mysql"})),
                "2026-01-01T00:00:01Z".to_string(),
            )
            .unwrap();
        });
        let response =
            wait_for_elicitation_response(&attempt_dir, elicitation_id, Duration::from_secs(10))
                .unwrap();
        assert!(matches!(response.action, ElicitationAction::Accept));
        assert_eq!(
            response.content,
            Some(serde_json::json!({"answer": "mysql"}))
        );
    }

    #[test]
    fn wait_for_elicitation_response_timeout() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let response = wait_for_elicitation_response(
            &attempt_dir,
            "elicit-timeout",
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(matches!(response.action, ElicitationAction::Decline));
        assert_eq!(response.content, None);
    }

    #[test]
    fn wait_for_elicitation_response_cancelled() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        // 写入取消标记
        request_elicitation_cancel(&attempt_dir, "2026-01-01T00:00:00Z".to_string()).unwrap();
        let response = wait_for_elicitation_response(
            &attempt_dir,
            "elicit-cancelled",
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(matches!(response.action, ElicitationAction::Decline));
        // 清理
        let _ = clear_elicitation_cancel_request(&attempt_dir);
    }

    #[test]
    fn elicitation_response_result_accept() {
        let response = ElicitationResponseState {
            elicitation_id: "elicit-1".to_string(),
            action: ElicitationAction::Accept,
            content: Some(serde_json::json!({"answer": "pg"})),
            decided_at: "t".to_string(),
        };
        let result = elicitation_response_result(&response);
        assert_eq!(result["action"], "accept");
        assert_eq!(result["content"]["answer"], "pg");
    }

    #[test]
    fn elicitation_response_result_decline() {
        let response = ElicitationResponseState {
            elicitation_id: "elicit-1".to_string(),
            action: ElicitationAction::Decline,
            content: None,
            decided_at: "t".to_string(),
        };
        let result = elicitation_response_result(&response);
        assert_eq!(result["action"], "decline");
        assert!(result.get("content").is_none());
    }

    #[test]
    fn write_elicitation_response_persists_timeline_response() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        write_elicitation_response(
            &attempt_dir,
            "elicit-answered",
            ElicitationAction::Accept,
            Some(serde_json::json!({ "answer": "mysql" })),
            "2Z".to_string(),
        )
        .unwrap();

        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        let event = items
            .iter()
            .find(|item| item.id == "elicit-answered-response")
            .unwrap();
        assert_eq!(event.kind, "elicitationResponse");
        assert_eq!(event.status.as_deref(), Some("completed"));
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("elicitationId"))
                .and_then(|value| value.as_str()),
            Some("elicit-answered")
        );
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("action"))
                .and_then(|value| value.as_str()),
            Some("accept")
        );
        assert!(!items.iter().any(|item| item.kind == "userTextDelta"));
    }

    #[test]
    fn cancel_pending_elicitation_requests_writes_decline_for_unanswered() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        // 写入一个 pending 请求
        write_pending_elicitation(
            &attempt_dir,
            &PendingElicitationState {
                elicitation_id: "elicit-cancel-me".to_string(),
                jsonrpc_id: serde_json::json!(1),
                message: "test".to_string(),
                requested_schema: serde_json::json!({}),
                created_at: "t".to_string(),
            },
        )
        .unwrap();
        // 取消所有
        cancel_pending_elicitation_requests(&attempt_dir, "now".to_string()).unwrap();
        // 验证响应文件已存在
        let response_path = elicitation_response_file(&attempt_dir, "elicit-cancel-me");
        assert!(response_path.exists());
        let response: ElicitationResponseState = read_json(&response_path).unwrap();
        assert!(matches!(response.action, ElicitationAction::Decline));
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        let event = items
            .iter()
            .find(|item| item.id == "elicit-cancel-me-response")
            .unwrap();
        assert_eq!(event.kind, "elicitationResponse");
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("action"))
                .and_then(|value| value.as_str()),
            Some("decline")
        );
    }

    #[test]
    fn remove_elicitation_signal_files_removes_request_and_response() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let elicitation_id = "elicit-cleanup";
        write_pending_elicitation(
            &attempt_dir,
            &PendingElicitationState {
                elicitation_id: elicitation_id.to_string(),
                jsonrpc_id: serde_json::json!(1),
                message: "Question".to_string(),
                requested_schema: serde_json::json!({}),
                created_at: "1Z".to_string(),
            },
        )
        .unwrap();
        write_elicitation_response(
            &attempt_dir,
            elicitation_id,
            ElicitationAction::Accept,
            Some(serde_json::json!({ "answer": "yes" })),
            "2Z".to_string(),
        )
        .unwrap();

        remove_elicitation_signal_files(&attempt_dir, elicitation_id).unwrap();

        assert!(!pending_elicitation_file(&attempt_dir, elicitation_id).exists());
        assert!(!elicitation_response_file(&attempt_dir, elicitation_id).exists());
    }

    #[test]
    fn sanitize_id_replaces_special_chars() {
        let path = pending_elicitation_file(&Utf8PathBuf::from("/tmp"), "elicit:a/b?c=d");
        let file_name = path.file_name().unwrap();
        assert!(!file_name.contains(':'));
        assert!(!file_name.contains('/'));
        assert!(!file_name.contains('?'));
        assert!(!file_name.contains('='));
    }
}
