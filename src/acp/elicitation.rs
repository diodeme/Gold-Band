use std::{fs, thread, time::Duration};

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{ensure_parent_dir, read_json, write_json};

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
            action,
            content,
            decided_at,
        },
    )
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
                decided_at: crate::acp::events::current_timestamp(),
            });
        }
        if started_at.elapsed() >= timeout {
            return Ok(ElicitationResponseState {
                elicitation_id: elicitation_id.to_string(),
                action: ElicitationAction::Decline,
                content: None,
                decided_at: crate::acp::events::current_timestamp(),
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

/// 根据 schema 中的 label 将用户回答的 JSON content 格式化为人类可读文本。
///
/// 单问题：直接返回答案值（如 `"MySQL"` 或 `"用户认证、日志系统"`）。
/// 多问题：逐行格式化，每行 `{field title}：{value}`，用换行分隔。
///
/// 示例（多问题）：
/// ```text
/// 数据库：MySQL
/// 功能模块：用户认证、日志系统
/// ```
pub fn format_elicitation_answer(schema: &Value, content: &Value) -> String {
    let properties = schema.get("properties").and_then(|v| v.as_object());
    let content_obj = content.as_object();

    let mut parts: Vec<String> = Vec::new();

    if let (Some(props), Some(obj)) = (properties, content_obj) {
        // Collect titles of "real" questions (select fields) for context.
        // When a custom/Other field has a value, we prefer the parent
        // question's title over the generic "Other" label.
        let question_titles: Vec<&str> = props
            .iter()
            .filter(|(k, v)| {
                !k.ends_with("_custom")
                    && *k != "customAnswer"
                    && *k != "other"
                    && *k != "custom"
                    && (v.get("oneOf").is_some() || v.get("anyOf").is_some())
            })
            .filter_map(|(_, v)| v.get("title").and_then(|t| t.as_str()))
            .collect();

        for (key, prop_schema) in props {
            let Some(val) = obj.get(key) else { continue };
            let value_str = format_single_value(prop_schema, val);

            if value_str.is_empty() {
                continue;
            }

            // 多问题时带上 field 标题，单问题时直接用值
            if props.len() > 1 {
                let label = resolve_elicitation_label(key, prop_schema, props, &question_titles);
                parts.push(format!("{label}：{value_str}"));
            } else {
                parts.push(value_str);
            }
        }
    }

    if parts.is_empty() {
        // 回退：直接提取 content 中的字符串值
        if let Some(obj) = content_obj {
            parts = obj
                .values()
                .filter_map(|v| {
                    v.as_str()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .collect();
        }
    }

    if parts.is_empty() {
        "已选择".to_string()
    } else {
        parts.join("\n")
    }
}

/// Resolve a human-readable label for a schema property.  For custom/Other
/// fields (`_custom`, `customAnswer`, `other`, `custom`), use the parent
/// question's title so the answer reads ``拓扑确认：xxx`` instead of
/// ``Other：xxx``.
fn resolve_elicitation_label<'a>(
    key: &'a str,
    prop_schema: &'a Value,
    props: &'a serde_json::Map<String, Value>,
    question_titles: &'a [&'a str],
) -> &'a str {
    // _custom suffix → derive parent key, use parent's title
    if let Some(base_key) = key.strip_suffix("_custom") {
        if let Some(parent_title) = props
            .get(base_key)
            .and_then(|v| v.get("title"))
            .and_then(|v| v.as_str())
        {
            return parent_title;
        }
    }
    // Generic custom keys → use first available question title
    if key == "customAnswer" || key == "other" || key == "custom" {
        if let Some(title) = question_titles.first().copied() {
            return title;
        }
    }
    // Default: use the property's own title
    prop_schema
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(key)
}

/// 将单个字段的值格式化为可读字符串
fn format_single_value(prop_schema: &Value, val: &Value) -> String {
    // oneOf 单选 → 用 label 替代 const 值
    if let Some(one_of) = prop_schema.get("oneOf").and_then(|v| v.as_array()) {
        if let Some(s) = val.as_str() {
            return one_of
                .iter()
                .find(|opt| opt.get("const").and_then(|v| v.as_str()) == Some(s))
                .and_then(|opt| opt.get("title").and_then(|v| v.as_str()))
                .unwrap_or(s)
                .to_string();
        }
    }

    // anyOf 多选 → 用 label 列表，中文顿号连接
    if let Some(any_of) = prop_schema.get("anyOf").and_then(|v| v.as_array()) {
        if let Some(arr) = val.as_array() {
            let labels: Vec<&str> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| {
                    any_of
                        .iter()
                        .find(|opt| opt.get("const").and_then(|v| v.as_str()) == Some(s))
                        .and_then(|opt| opt.get("title").and_then(|v| v.as_str()))
                        .unwrap_or(s)
                })
                .collect();
            if !labels.is_empty() {
                return labels.join("、");
            }
        }
    }

    // 普通字符串
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // 兜底
    val.to_string()
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
