use std::{fs, thread, time::Duration};

use agent_client_protocol_schema::v1::CreateElicitationRequest;
use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    acp::{
        events::current_timestamp,
        timeline::{
            TimelineIndexedItem, TimelineItemIdentity, append_elicitation_response_item,
            read_indexed_pending_elicitation, read_indexed_timeline_item,
        },
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
    pub request: CreateElicitationRequest,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_identity: Option<TimelineItemIdentity>,
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

pub fn bind_pending_elicitation_timeline_identity(
    attempt_dir: &Utf8Path,
    elicitation_id: &str,
    identity: TimelineItemIdentity,
) -> Result<()> {
    let path = pending_elicitation_file(attempt_dir, elicitation_id);
    let mut pending: PendingElicitationState = read_json(&path)?;
    pending.timeline_identity = Some(identity);
    write_json(&path, &pending)
}

// ── 前端写入响应（由 Tauri command 调用）──

pub fn write_elicitation_response(
    attempt_dir: &Utf8Path,
    elicitation_id: &str,
    action: ElicitationAction,
    content: Option<Value>,
    decided_at: String,
) -> Result<()> {
    upsert_elicitation_response_event(attempt_dir, elicitation_id, &action, content.clone())?;
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
    )
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
    wait_for_elicitation_response_until_cancelled(attempt_dir, elicitation_id, timeout, || false)
}

pub fn wait_for_elicitation_response_until_cancelled(
    attempt_dir: &Utf8Path,
    elicitation_id: &str,
    timeout: Duration,
    is_cancel_requested: impl Fn() -> bool,
) -> Result<ElicitationResponseState> {
    let path = elicitation_response_file(attempt_dir, elicitation_id);
    let started_at = std::time::Instant::now();
    loop {
        if is_cancel_requested() {
            return Ok(ElicitationResponseState {
                elicitation_id: elicitation_id.to_string(),
                action: ElicitationAction::Decline,
                content: None,
                decided_at: current_timestamp(),
            });
        }
        if path.exists() {
            // The response file is a durable hand-off signal. The runtime must
            // keep it until the ACP response has been persisted and sent, then
            // remove request and response files together.
            return read_json(&path);
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
    let Ok(pending) = read_json::<PendingElicitationState>(&pending_elicitation_file(
        attempt_dir,
        elicitation_id,
    )) else {
        return Ok(());
    };
    let Some((identity, indexed)) = resolve_elicitation_identity(attempt_dir, &pending)? else {
        return Ok(());
    };
    let timeline_path =
        crate::acp::branches::branch_timeline_path(attempt_dir, &identity.branch_id);
    let action_value = match action {
        ElicitationAction::Accept => "accept",
        ElicitationAction::Decline => "decline",
    };
    let _ = append_elicitation_response_item(
        &timeline_path,
        &identity.item_id,
        Some(indexed.revision),
        elicitation_id,
        action_value,
        content,
        current_timestamp(),
    )?;
    Ok(())
}

fn resolve_elicitation_identity(
    attempt_dir: &Utf8Path,
    pending: &PendingElicitationState,
) -> Result<Option<(TimelineItemIdentity, TimelineIndexedItem)>> {
    crate::acp::branches::prepare_agent_timeline_storage(attempt_dir)?;
    if let Some(identity) = pending.timeline_identity.as_ref() {
        let path = crate::acp::branches::branch_timeline_path(attempt_dir, &identity.branch_id);
        if let Some(indexed) = read_indexed_timeline_item(&path, &identity.item_id)? {
            return Ok(Some((identity.clone(), indexed)));
        }
    }
    for (branch_id, path) in crate::acp::branches::existing_branch_timeline_paths(attempt_dir)? {
        if let Some(indexed) = read_indexed_pending_elicitation(&path, &pending.elicitation_id)? {
            return Ok(Some((
                TimelineItemIdentity {
                    branch_id,
                    item_id: indexed.event.id.clone(),
                    revision: indexed.revision,
                },
                indexed,
            )));
        }
        if let Some(indexed) = read_indexed_timeline_item(&path, &pending.elicitation_id)? {
            return Ok(Some((
                TimelineItemIdentity {
                    branch_id,
                    item_id: pending.elicitation_id.clone(),
                    revision: indexed.revision,
                },
                indexed,
            )));
        }
    }
    Ok(None)
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
    use crate::acp::events::{
        elicitation_request_event, load_timeline_items, write_timeline_items,
    };
    use tempfile::TempDir;

    fn dummy_attempt_dir() -> (TempDir, Utf8PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        write_json(
            &path.join("node.json"),
            &serde_json::json!({
                "version": crate::domain::VERSION,
                "acp_storage_schema_version": crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                "node_id": "worker",
                "node_type": "worker",
                "run_id": "run-001",
                "round_id": "round-001",
                "attempt_id": "attempt-001",
                "status": "running",
                "outcome": null,
                "started_at": "1Z",
                "finished_at": null,
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();
        (dir, path)
    }

    fn test_elicitation_request(message: &str) -> CreateElicitationRequest {
        serde_json::from_value(serde_json::json!({
            "mode": "form",
            "sessionId": "session-test",
            "toolCallId": "tool-test",
            "message": message,
            "requestedSchema": {
                "type": "object",
                "properties": {},
                "_meta": { "schemaSource": "test" }
            },
            "_meta": { "requestSource": "test" }
        }))
        .unwrap()
    }

    #[test]
    fn deserializes_claude_agent_acp_044_form_request_without_line_parsing() {
        let message = "Round 11 | 组件：反馈列表管理页 + 反馈详情页 | 歧义：23.5%\n\n管理端 API 与菜单的权限标识如何设计？";
        let raw = serde_json::json!({
            "message": message,
            "mode": "form",
            "requestedSchema": {
                "properties": {
                    "customAnswer": {
                        "description": "Type your own answer instead of choosing an option above (optional).",
                        "title": "Other",
                        "type": "string"
                    },
                    "question_0": {
                        "oneOf": [{
                            "const": "admin-only 无 perm",
                            "title": "admin-only 无 perm — 菜单放 admin 块"
                        }],
                        "title": "管理端权限标识",
                        "type": "string"
                    }
                },
                "type": "object"
            },
            "sessionId": "dff9dc64-77bb-4562-9fa5-960516f8540d",
            "toolCallId": "call_a05f15e6f68b4cc78b3b4014"
        });

        let request: CreateElicitationRequest = serde_json::from_value(raw).unwrap();
        let serialized = serde_json::to_value(&request).unwrap();

        assert_eq!(request.message, message);
        assert_eq!(
            serialized["sessionId"],
            "dff9dc64-77bb-4562-9fa5-960516f8540d"
        );
        assert_eq!(serialized["toolCallId"], "call_a05f15e6f68b4cc78b3b4014");
        assert_eq!(
            serialized["requestedSchema"]["properties"]["customAnswer"]["title"],
            "Other"
        );
        assert_eq!(
            serialized["requestedSchema"]["properties"]["question_0"]["oneOf"][0]["const"],
            "admin-only 无 perm"
        );
    }

    #[test]
    fn write_and_read_pending_elicitation() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let state = PendingElicitationState {
            elicitation_id: "elicit-abc123".to_string(),
            jsonrpc_id: serde_json::json!(42),
            request: test_elicitation_request("请选择数据库"),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            timeline_identity: None,
        };
        write_pending_elicitation(&attempt_dir, &state).unwrap();
        let path = pending_elicitation_file(&attempt_dir, "elicit-abc123");
        assert!(path.exists());
        let read_back: PendingElicitationState = read_json(&path).unwrap();
        assert_eq!(read_back.elicitation_id, "elicit-abc123");
        assert_eq!(read_back.request.message, "请选择数据库");
        let request = serde_json::to_value(read_back.request).unwrap();
        assert_eq!(request["toolCallId"], "tool-test");
        assert_eq!(request["_meta"]["requestSource"], "test");
        assert_eq!(request["requestedSchema"]["_meta"]["schemaSource"], "test");
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
        assert!(elicitation_response_file(&attempt_dir, elicitation_id).exists());
        remove_elicitation_signal_files(&attempt_dir, elicitation_id).unwrap();
        assert!(!elicitation_response_file(&attempt_dir, elicitation_id).exists());
    }

    #[test]
    fn response_signal_survives_timeline_persistence_until_runtime_cleanup() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let elicitation_id = "elicit-completed-session-follow-up";
        write_pending_elicitation(
            &attempt_dir,
            &PendingElicitationState {
                elicitation_id: elicitation_id.to_string(),
                jsonrpc_id: serde_json::json!(42),
                request: test_elicitation_request("Continue the completed session"),
                created_at: "1Z".to_string(),
                timeline_identity: None,
            },
        )
        .unwrap();
        write_elicitation_response(
            &attempt_dir,
            elicitation_id,
            ElicitationAction::Accept,
            Some(serde_json::json!({ "answer": "continue" })),
            "2Z".to_string(),
        )
        .unwrap();

        let response =
            wait_for_elicitation_response(&attempt_dir, elicitation_id, Duration::from_millis(10))
                .unwrap();
        assert!(matches!(response.action, ElicitationAction::Accept));
        assert!(pending_elicitation_file(&attempt_dir, elicitation_id).exists());
        assert!(elicitation_response_file(&attempt_dir, elicitation_id).exists());

        remove_elicitation_signal_files(&attempt_dir, elicitation_id).unwrap();
        assert!(!pending_elicitation_file(&attempt_dir, elicitation_id).exists());
        assert!(!elicitation_response_file(&attempt_dir, elicitation_id).exists());
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
    fn elicitation_wait_declines_when_turn_cancel_is_requested() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        let checks = std::cell::Cell::new(0_u8);

        let response = wait_for_elicitation_response_until_cancelled(
            &attempt_dir,
            "elicit-late-after-cancel",
            Duration::from_secs(10),
            || {
                let next = checks.get().saturating_add(1);
                checks.set(next);
                next >= 2
            },
        )
        .unwrap();

        assert!(matches!(response.action, ElicitationAction::Decline));
        assert_eq!(response.content, None);
        assert!(!elicitation_response_file(&attempt_dir, "elicit-late-after-cancel").exists());
    }

    #[test]
    fn elicitation_cancel_wins_over_a_simultaneous_user_response() {
        let (_dir, attempt_dir) = dummy_attempt_dir();
        write_json(
            &elicitation_response_file(&attempt_dir, "elicit-race"),
            &ElicitationResponseState {
                elicitation_id: "elicit-race".to_string(),
                action: ElicitationAction::Accept,
                content: Some(serde_json::json!({ "confirmed": true })),
                decided_at: current_timestamp(),
            },
        )
        .unwrap();

        let response = wait_for_elicitation_response_until_cancelled(
            &attempt_dir,
            "elicit-race",
            Duration::from_secs(10),
            || true,
        )
        .unwrap();

        assert!(matches!(response.action, ElicitationAction::Decline));
        assert_eq!(response.content, None);
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
        let request = test_elicitation_request("Choose a database");
        write_pending_elicitation(
            &attempt_dir,
            &PendingElicitationState {
                elicitation_id: "elicit-answered".to_string(),
                jsonrpc_id: serde_json::json!(1),
                request: request.clone(),
                created_at: "1Z".to_string(),
                timeline_identity: None,
            },
        )
        .unwrap();
        write_timeline_items(
            &attempt_dir.join("acp.timeline.jsonl"),
            &[elicitation_request_event(
                1,
                "elicit-answered".to_string(),
                &request,
            )],
        )
        .unwrap();
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
                request: test_elicitation_request("test"),
                created_at: "t".to_string(),
                timeline_identity: None,
            },
        )
        .unwrap();
        write_timeline_items(
            &attempt_dir.join("acp.timeline.jsonl"),
            &[elicitation_request_event(
                1,
                "elicit-cancel-me".to_string(),
                &test_elicitation_request("test"),
            )],
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
                request: test_elicitation_request("Question"),
                created_at: "1Z".to_string(),
                timeline_identity: None,
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
