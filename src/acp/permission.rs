use std::{fs, thread, time::Duration};

use anyhow::{Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    acp::{
        events::current_timestamp,
        interaction::{
            AcpPromptInteractionIdentity, AcpPromptInteractionKind,
            PendingAcpPromptInteractionState, bind_pending_prompt_interaction_timeline_identity,
            write_pending_prompt_interaction,
        },
        timeline::{
            TimelineIndexedItem, TimelineItemIdentity, TimelineSettleOutcome,
            read_indexed_pending_permission, read_indexed_timeline_item, settle_permission_item,
        },
    },
    storage::{ensure_parent_dir, read_json, write_json},
};

pub type PendingPermissionState = PendingAcpPromptInteractionState<Value>;

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
        let request_id = &pending.identity.interaction_id;
        let response_path = permission_response_file(attempt_dir, request_id);
        if response_path.exists() {
            continue;
        }
        let Some((identity, indexed)) = resolve_permission_identity(
            attempt_dir,
            request_id,
            pending.timeline_identity.as_ref(),
        )?
        else {
            continue;
        };
        if indexed.event.status.as_deref() != Some("pending") {
            continue;
        }
        let timeline_path =
            crate::acp::branches::branch_timeline_path(attempt_dir, &identity.branch_id);
        if settle_permission_item(
            &timeline_path,
            &identity.item_id,
            Some(indexed.revision),
            request_id,
            None,
            true,
            decided_at.clone(),
        )? == TimelineSettleOutcome::Applied
        {
            write_permission_response(
                attempt_dir,
                request_id,
                None,
                true,
                decided_at.clone(),
            )?;
            remove_file_if_exists(&pending_permission_file(attempt_dir, request_id))?;
        }
    }
    Ok(())
}

pub fn write_pending_permission(
    attempt_dir: &Utf8Path,
    request_id: &str,
    turn_id: &str,
    prompt_event_id: &str,
    params: Value,
    created_at: String,
) -> Result<()> {
    let path = pending_permission_file(attempt_dir, request_id);
    write_pending_prompt_interaction(
        &path,
        &PendingAcpPromptInteractionState {
            identity: AcpPromptInteractionIdentity::new(
                request_id,
                AcpPromptInteractionKind::Permission,
                turn_id,
                prompt_event_id,
            ),
            payload: params,
            created_at,
            timeline_identity: None,
        },
    )
}

pub fn bind_pending_permission_timeline_identity(
    attempt_dir: &Utf8Path,
    request_id: &str,
    identity: TimelineItemIdentity,
) -> Result<()> {
    let path = pending_permission_file(attempt_dir, request_id);
    bind_pending_prompt_interaction_timeline_identity::<Value>(&path, identity)
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
    let pending_path = pending_permission_file(attempt_dir, request_id);
    if !pending_path.exists() {
        return Ok(false);
    }
    if permission_response_file(attempt_dir, request_id).exists() {
        return Ok(false);
    }
    // The provider waiter is the control-plane consumer. Persist its response
    // before attempting to settle the timeline projection: the permission
    // event can still be between the pending-file write and timeline/index
    // persistence when the user responds.
    let pending: PendingPermissionState = read_json(&pending_path)?;
    write_permission_response(
        attempt_dir,
        request_id,
        option_id.clone(),
        cancelled,
        decided_at.clone(),
    )?;
    if let Some((identity, indexed)) = resolve_permission_identity(
        attempt_dir,
        &pending.identity.interaction_id,
        pending.timeline_identity.as_ref(),
    )? {
        if indexed.event.status.as_deref() == Some("pending") {
            let timeline_path =
                crate::acp::branches::branch_timeline_path(attempt_dir, &identity.branch_id);
            let _ = settle_permission_item(
                &timeline_path,
                &identity.item_id,
                Some(indexed.revision),
                request_id,
                option_id,
                cancelled,
                decided_at,
            );
        }
    }
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
    wait_for_permission_response_until_cancelled(attempt_dir, request_id, || false)
}

pub fn wait_for_permission_response_until_cancelled(
    attempt_dir: &Utf8Path,
    request_id: &str,
    is_cancel_requested: impl Fn() -> bool,
) -> Result<PermissionResponseState> {
    let path = permission_response_file(attempt_dir, request_id);
    loop {
        if is_cancel_requested() {
            return Ok(PermissionResponseState {
                request_id: request_id.to_string(),
                option_id: None,
                cancelled: true,
                decided_at: current_timestamp(),
            });
        }
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

pub fn upsert_permission_decision_event(
    attempt_dir: &Utf8Path,
    request_id: &str,
    option_id: Option<String>,
    cancelled: bool,
) -> Result<()> {
    let timeline_identity =
        read_json::<PendingPermissionState>(&pending_permission_file(attempt_dir, request_id))
            .ok()
            .and_then(|pending| pending.timeline_identity);
    let Some((identity, indexed)) =
        resolve_permission_identity(attempt_dir, request_id, timeline_identity.as_ref())?
    else {
        return Ok(());
    };
    let timeline_path =
        crate::acp::branches::branch_timeline_path(attempt_dir, &identity.branch_id);
    let _ = settle_permission_item(
        &timeline_path,
        &identity.item_id,
        Some(indexed.revision),
        request_id,
        option_id,
        cancelled,
        current_timestamp(),
    )?;
    Ok(())
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

fn resolve_permission_identity(
    attempt_dir: &Utf8Path,
    request_id: &str,
    timeline_identity: Option<&TimelineItemIdentity>,
) -> Result<Option<(TimelineItemIdentity, TimelineIndexedItem)>> {
    crate::acp::branches::prepare_agent_timeline_storage(attempt_dir)?;
    if let Some(identity) = timeline_identity {
        let path = crate::acp::branches::branch_timeline_path(attempt_dir, &identity.branch_id);
        if let Some(indexed) = read_indexed_timeline_item(&path, &identity.item_id)? {
            return Ok(Some((identity.clone(), indexed)));
        }
    }
    for (branch_id, path) in crate::acp::branches::existing_branch_timeline_paths(attempt_dir)? {
        if let Some(indexed) = read_indexed_pending_permission(&path, request_id)? {
            return Ok(Some((
                TimelineItemIdentity {
                    branch_id,
                    item_id: indexed.event.id.clone(),
                    revision: indexed.revision,
                },
                indexed,
            )));
        }
        let item_id = format!("permission-{request_id}");
        if let Some(indexed) = read_indexed_timeline_item(&path, &item_id)? {
            return Ok(Some((
                TimelineItemIdentity {
                    branch_id,
                    item_id,
                    revision: indexed.revision,
                },
                indexed,
            )));
        }
    }
    Ok(None)
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
        acp::events::{load_timeline_items, permission_request_event, write_timeline_items},
        storage::append_jsonl,
    };
    use tempfile::tempdir;

    fn test_attempt_dir(storage_version: u32) -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        write_json(
            &attempt_dir.join("node.json"),
            &serde_json::json!({
                "version": crate::domain::VERSION,
                "acp_storage_schema_version": storage_version,
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
        (dir, attempt_dir)
    }

    #[test]
    fn pending_permission_persists_owning_prompt_turn_identity() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        write_pending_permission(
            &attempt_dir,
            "request-1",
            "turn-2",
            "prompt-event-2",
            serde_json::json!({ "sessionId": "session-1" }),
            "2Z".to_string(),
        )
        .unwrap();

        let pending: PendingPermissionState =
            read_json(&pending_permission_file(&attempt_dir, "request-1")).unwrap();
        assert_eq!(pending.identity.interaction_id, "request-1");
        assert_eq!(pending.identity.kind, AcpPromptInteractionKind::Permission);
        assert_eq!(pending.identity.turn_id, "turn-2");
        assert_eq!(pending.identity.prompt_event_id, "prompt-event-2");
    }

    #[test]
    fn permission_wait_returns_cancelled_when_turn_cancel_is_requested() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let checks = std::cell::Cell::new(0_u8);

        let response =
            wait_for_permission_response_until_cancelled(&attempt_dir, "late-after-cancel", || {
                let next = checks.get().saturating_add(1);
                checks.set(next);
                next >= 2
            })
            .unwrap();

        assert!(response.cancelled);
        assert_eq!(response.request_id, "late-after-cancel");
        assert_eq!(response.option_id, None);
        assert!(!permission_response_file(&attempt_dir, "late-after-cancel").exists());
    }

    #[test]
    fn permission_cancel_wins_over_a_simultaneous_user_response() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        write_permission_response(
            &attempt_dir,
            "permission-race",
            Some("allow-once".to_string()),
            false,
            current_timestamp(),
        )
        .unwrap();

        let response =
            wait_for_permission_response_until_cancelled(&attempt_dir, "permission-race", || true)
                .unwrap();

        assert!(response.cancelled);
        assert_eq!(response.option_id, None);
    }

    #[test]
    fn cancel_pending_permission_updates_timeline_status() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let request_id = "42";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let request_id = "context";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
    fn cancel_pending_permission_migrates_legacy_audit_without_writing_it() {
        let (_dir, attempt_dir) = test_attempt_dir(0);
        let request_id = "legacy";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
        assert!(events.contains("\"status\":\"pending\""));
        assert!(!events.contains("\"status\":\"cancelled\""));
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "permission-legacy");
        assert_eq!(items[0].status.as_deref(), Some("cancelled"));
    }

    #[test]
    fn cancel_pending_permission_keeps_selected_permission_unchanged() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let request_id = "selected";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
    fn write_permission_response_if_pending_updates_timeline_status() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let request_id = "allow";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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

        let written = write_permission_response_if_pending(
            &attempt_dir,
            request_id,
            Some("allow".to_string()),
            false,
            "2Z".to_string(),
        )
        .unwrap();

        assert!(written);
        let response: PermissionResponseState =
            read_json(&permission_response_file(&attempt_dir, request_id)).unwrap();
        assert_eq!(response.option_id.as_deref(), Some("allow"));
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        let event = items
            .iter()
            .find(|item| item.id == "permission-allow")
            .unwrap();
        assert_eq!(event.status.as_deref(), Some("selected"));
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(event.title.as_deref(), Some("Write file"));
        assert_eq!(event.started_seq, Some(7));
        assert!(event.ended_seq.is_some_and(|seq| seq > 7));
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("optionId"))
                .and_then(|value| value.as_str()),
            Some("allow")
        );
    }

    #[test]
    fn write_permission_response_if_pending_does_not_revive_cancelled_permission() {
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let request_id = "cancelled";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
        let (_dir, attempt_dir) =
            test_attempt_dir(crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION);
        let request_id = "cleanup";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
