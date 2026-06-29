use std::{fs, thread, time::Duration};

use anyhow::{Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    acp::events::{
        AcpUiEvent, append_ui_event, current_timestamp, latest_timeline_source_seq,
        load_timeline_items, write_timeline_items,
    },
    storage::{ensure_parent_dir, read_json, write_json},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionState {
    pub request_id: String,
    pub params: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponseState {
    pub request_id: String,
    pub option_id: Option<String>,
    #[serde(default)]
    pub cancelled: bool,
    pub decided_at: String,
}

pub fn pending_permission_file(attempt_dir: &Utf8Path, request_id: &str) -> Utf8PathBuf {
    attempt_dir.join(format!(
        "acp.permission-request.{}.json",
        sanitize_id(request_id)
    ))
}

pub fn permission_response_file(attempt_dir: &Utf8Path, request_id: &str) -> Utf8PathBuf {
    attempt_dir.join(format!(
        "acp.permission-response.{}.json",
        sanitize_id(request_id)
    ))
}

pub fn cancel_pending_permission_requests(
    attempt_dir: &Utf8Path,
    decided_at: String,
) -> Result<()> {
    let mut cancelled_request_ids = Vec::new();
    let Ok(entries) = fs::read_dir(attempt_dir.as_std_path()) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with("acp.permission-request.") || !file_name.ends_with(".json") {
            continue;
        }
        let Ok(path) = Utf8PathBuf::from_path_buf(path) else {
            continue;
        };
        let Ok(pending) = read_json::<PendingPermissionState>(&path) else {
            continue;
        };
        if latest_permission_status(attempt_dir, &pending.request_id)
            .as_deref()
            .is_some_and(|status| status != "pending")
        {
            continue;
        }
        let response_path = permission_response_file(attempt_dir, &pending.request_id);
        if response_path.exists() {
            if read_json::<PermissionResponseState>(&response_path)
                .ok()
                .is_some_and(|response| response.cancelled)
            {
                cancelled_request_ids.push(pending.request_id);
            }
            continue;
        }
        write_permission_response(
            attempt_dir,
            &pending.request_id,
            None,
            true,
            decided_at.clone(),
        )?;
        cancelled_request_ids.push(pending.request_id);
    }
    for request_id in cancelled_request_ids {
        upsert_cancelled_permission_event(attempt_dir, &request_id)?;
        remove_file_if_exists(&pending_permission_file(attempt_dir, &request_id))?;
    }
    Ok(())
}

pub fn write_pending_permission(
    attempt_dir: &Utf8Path,
    request_id: &str,
    params: Value,
    created_at: String,
) -> Result<()> {
    let path = pending_permission_file(attempt_dir, request_id);
    write_json(
        &path,
        &PendingPermissionState {
            request_id: request_id.to_string(),
            params,
            created_at,
        },
    )
}

pub fn write_permission_response(
    attempt_dir: &Utf8Path,
    request_id: &str,
    option_id: Option<String>,
    cancelled: bool,
    decided_at: String,
) -> Result<()> {
    let path = permission_response_file(attempt_dir, request_id);
    ensure_parent_dir(&path)?;
    write_json(
        &path,
        &PermissionResponseState {
            request_id: request_id.to_string(),
            option_id,
            cancelled,
            decided_at,
        },
    )
}

pub fn write_permission_response_if_pending(
    attempt_dir: &Utf8Path,
    request_id: &str,
    option_id: Option<String>,
    cancelled: bool,
    decided_at: String,
) -> Result<bool> {
    if !pending_permission_file(attempt_dir, request_id).exists() {
        return Ok(false);
    }
    if permission_response_file(attempt_dir, request_id).exists() {
        return Ok(false);
    }
    if latest_permission_status(attempt_dir, request_id)
        .as_deref()
        .is_some_and(|status| status != "pending")
    {
        return Ok(false);
    }
    write_permission_response(attempt_dir, request_id, option_id, cancelled, decided_at)?;
    Ok(true)
}

pub fn remove_permission_signal_files(attempt_dir: &Utf8Path, request_id: &str) -> Result<()> {
    remove_file_if_exists(&pending_permission_file(attempt_dir, request_id))?;
    remove_file_if_exists(&permission_response_file(attempt_dir, request_id))
}

pub fn wait_for_permission_response(
    attempt_dir: &Utf8Path,
    request_id: &str,
) -> Result<PermissionResponseState> {
    let path = permission_response_file(attempt_dir, request_id);
    loop {
        if path.exists() {
            let response = read_json(&path)?;
            let _ = fs::remove_file(path.as_std_path());
            return Ok(response);
        }
        thread::sleep(Duration::from_millis(200));
    }
}

pub fn acp_permission_response_result(response: PermissionResponseState) -> Result<Value> {
    if response.cancelled {
        return Ok(serde_json::json!({ "outcome": { "outcome": "cancelled" } }));
    }
    let option_id = response
        .option_id
        .ok_or_else(|| anyhow!("permission response requires optionId unless cancelled"))?;
    Ok(serde_json::json!({
        "outcome": {
            "outcome": "selected",
            "optionId": option_id,
        }
    }))
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

fn upsert_cancelled_permission_event(attempt_dir: &Utf8Path, request_id: &str) -> Result<()> {
    let timeline_path = attempt_dir.join("acp.timeline.jsonl");
    let events_path = attempt_dir.join("acp.events.jsonl");
    let source_seq = if timeline_path.exists() || !events_path.exists() {
        latest_timeline_source_seq(&timeline_path) + 1
    } else {
        legacy_event_count(&events_path) + 1
    };
    let existing = latest_permission_event(attempt_dir, request_id);
    let mut event =
        cancelled_permission_event(source_seq, request_id.to_string(), existing.as_ref());
    event.id = format!("permission-{request_id}");
    event.started_seq = Some(source_seq);
    event.ended_seq = Some(source_seq);
    event.started_at = Some(event.timestamp.clone());
    event.ended_at = Some(event.timestamp.clone());

    if events_path.exists() && !timeline_path.exists() {
        append_ui_event(&events_path, &event)?;
    }

    let mut items = load_timeline_items(&timeline_path)?;
    if let Some(existing) = items.iter_mut().find(|item| item.id == event.id) {
        event.started_seq = existing.started_seq.or(event.started_seq);
        event.started_at = existing.started_at.clone().or(event.started_at.clone());
        *existing = event;
    } else {
        items.push(event);
    }
    items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
    write_timeline_items(&timeline_path, &items)
}

fn cancelled_permission_event(
    seq: u64,
    request_id: String,
    existing: Option<&AcpUiEvent>,
) -> AcpUiEvent {
    let mut raw = existing
        .and_then(|event| event.raw.clone())
        .unwrap_or_default();
    if !raw.is_object() {
        raw = serde_json::json!({});
    }
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "requestId".to_string(),
            serde_json::json!(request_id.clone()),
        );
        object.insert("cancelled".to_string(), serde_json::json!(true));
    }
    AcpUiEvent {
        id: request_id.clone(),
        seq,
        timestamp: current_timestamp(),
        kind: "permissionRequest".to_string(),
        session_id: existing.and_then(|event| event.session_id.clone()),
        content: None,
        title: existing
            .and_then(|event| event.title.clone())
            .or_else(|| Some("Permission cancelled".to_string())),
        tool_call_id: existing.and_then(|event| event.tool_call_id.clone()),
        status: Some("cancelled".to_string()),
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
        raw: Some(raw),
    }
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

fn latest_permission_status(attempt_dir: &Utf8Path, request_id: &str) -> Option<String> {
    latest_permission_event(attempt_dir, request_id).and_then(|event| event.status)
}

fn latest_permission_event(attempt_dir: &Utf8Path, request_id: &str) -> Option<AcpUiEvent> {
    let timeline_event = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl"))
        .ok()
        .into_iter()
        .flatten()
        .filter(|event| permission_event_matches(event, request_id))
        .max_by_key(|event| event.ended_seq.or(event.started_seq).unwrap_or(event.seq));
    if timeline_event.is_some() {
        return timeline_event;
    }

    let events_path = attempt_dir.join("acp.events.jsonl");
    let Ok(content) = fs::read_to_string(events_path.as_std_path()) else {
        return None;
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<AcpUiEvent>(line).ok())
        .filter(|event| permission_event_matches(event, request_id))
        .max_by_key(|event| event.ended_seq.or(event.started_seq).unwrap_or(event.seq))
}

fn permission_event_matches(event: &AcpUiEvent, request_id: &str) -> bool {
    if event.kind != "permissionRequest" {
        return false;
    }
    let event_id = strip_permission_prefix(&event.id);
    if event_id == request_id {
        return true;
    }
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("requestId"))
        .and_then(|value| value.as_str())
        .map(strip_permission_prefix)
        .is_some_and(|raw_id| raw_id == request_id)
}

fn strip_permission_prefix(value: &str) -> String {
    let mut current = value;
    while let Some(next) = current.strip_prefix("permission-") {
        current = next;
    }
    current.to_string()
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
    use crate::{
        acp::events::{permission_request_event, write_timeline_items},
        storage::append_jsonl,
    };
    use tempfile::tempdir;

    #[test]
    fn cancel_pending_permission_updates_timeline_status() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "42";
        write_pending_permission(
            &attempt_dir,
            request_id,
            serde_json::json!({
                "toolCall": {
                    "title": "Write file"
                },
                "options": [
                    { "optionId": "allow", "name": "Allow" }
                ]
            }),
            "1Z".to_string(),
        )
        .unwrap();
        let mut pending =
            permission_request_event(7, request_id.to_string(), serde_json::json!({}));
        pending.id = format!("permission-{request_id}");
        pending.started_seq = Some(7);
        pending.ended_seq = Some(7);
        write_timeline_items(&attempt_dir.join("acp.timeline.jsonl"), &[pending]).unwrap();

        cancel_pending_permission_requests(&attempt_dir, "2Z".to_string()).unwrap();

        let response: PermissionResponseState =
            read_json(&permission_response_file(&attempt_dir, request_id)).unwrap();
        assert!(response.cancelled);
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        let event = items
            .iter()
            .find(|item| item.id == "permission-42")
            .unwrap();
        assert_eq!(event.status.as_deref(), Some("cancelled"));
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("cancelled"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn cancel_pending_permission_preserves_event_context() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "context";
        write_pending_permission(
            &attempt_dir,
            request_id,
            serde_json::json!({
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": "tool-1",
                    "title": "Write file"
                },
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                ]
            }),
            "1Z".to_string(),
        )
        .unwrap();
        let mut pending = permission_request_event(
            7,
            request_id.to_string(),
            serde_json::json!({
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": "tool-1",
                    "title": "Write file"
                },
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" }
                ]
            }),
        );
        pending.id = format!("permission-{request_id}");
        pending.started_seq = Some(7);
        pending.ended_seq = Some(7);
        write_timeline_items(&attempt_dir.join("acp.timeline.jsonl"), &[pending]).unwrap();

        cancel_pending_permission_requests(&attempt_dir, "2Z".to_string()).unwrap();

        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        let event = items
            .iter()
            .find(|item| item.id == "permission-context")
            .unwrap();
        assert_eq!(event.status.as_deref(), Some("cancelled"));
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(event.title.as_deref(), Some("Write file"));
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("options"))
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn cancel_pending_permission_appends_legacy_event_when_no_timeline_exists() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "legacy";
        write_pending_permission(
            &attempt_dir,
            request_id,
            serde_json::json!({}),
            "1Z".to_string(),
        )
        .unwrap();
        let events_path = attempt_dir.join("acp.events.jsonl");
        append_jsonl(
            &events_path,
            &permission_request_event(1, request_id.to_string(), serde_json::json!({})),
        )
        .unwrap();

        cancel_pending_permission_requests(&attempt_dir, "2Z".to_string()).unwrap();

        let events = fs::read_to_string(events_path.as_std_path()).unwrap();
        assert!(events.contains("\"status\":\"cancelled\""));
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "permission-legacy");
        assert_eq!(items[0].status.as_deref(), Some("cancelled"));
    }

    #[test]
    fn cancel_pending_permission_keeps_selected_permission_unchanged() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "selected";
        write_pending_permission(
            &attempt_dir,
            request_id,
            serde_json::json!({}),
            "1Z".to_string(),
        )
        .unwrap();
        let mut selected =
            permission_request_event(5, request_id.to_string(), serde_json::json!({}));
        selected.id = format!("permission-{request_id}");
        selected.status = Some("selected".to_string());
        selected.started_seq = Some(5);
        selected.ended_seq = Some(5);
        write_timeline_items(&attempt_dir.join("acp.timeline.jsonl"), &[selected]).unwrap();

        cancel_pending_permission_requests(&attempt_dir, "2Z".to_string()).unwrap();

        assert!(!permission_response_file(&attempt_dir, request_id).exists());
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status.as_deref(), Some("selected"));
    }

    #[test]
    fn write_permission_response_if_pending_does_not_revive_cancelled_permission() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "cancelled";
        write_pending_permission(
            &attempt_dir,
            request_id,
            serde_json::json!({}),
            "1Z".to_string(),
        )
        .unwrap();
        let mut pending =
            permission_request_event(3, request_id.to_string(), serde_json::json!({}));
        pending.id = format!("permission-{request_id}");
        pending.started_seq = Some(3);
        pending.ended_seq = Some(3);
        write_timeline_items(&attempt_dir.join("acp.timeline.jsonl"), &[pending]).unwrap();

        cancel_pending_permission_requests(&attempt_dir, "2Z".to_string()).unwrap();
        let cancelled_response_path = permission_response_file(&attempt_dir, request_id);
        assert!(cancelled_response_path.exists());
        fs::remove_file(cancelled_response_path.as_std_path()).unwrap();

        let written = write_permission_response_if_pending(
            &attempt_dir,
            request_id,
            Some("allow".to_string()),
            false,
            "3Z".to_string(),
        )
        .unwrap();

        assert!(!written);
        assert!(!permission_response_file(&attempt_dir, request_id).exists());
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        assert_eq!(items[0].status.as_deref(), Some("cancelled"));
    }

    #[test]
    fn remove_permission_signal_files_removes_request_and_response() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "cleanup";
        write_pending_permission(
            &attempt_dir,
            request_id,
            serde_json::json!({}),
            "1Z".to_string(),
        )
        .unwrap();
        write_permission_response(
            &attempt_dir,
            request_id,
            Some("allow".to_string()),
            false,
            "2Z".to_string(),
        )
        .unwrap();

        remove_permission_signal_files(&attempt_dir, request_id).unwrap();

        assert!(!pending_permission_file(&attempt_dir, request_id).exists());
        assert!(!permission_response_file(&attempt_dir, request_id).exists());
    }
}
