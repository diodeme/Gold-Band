use std::{fs, thread, time::Duration};

use anyhow::{Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{ensure_parent_dir, read_json, write_json};

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

pub fn cancel_requested_file(attempt_dir: &Utf8Path) -> Utf8PathBuf {
    attempt_dir.join("acp.cancel-requested")
}

pub fn request_cancel(attempt_dir: &Utf8Path, requested_at: String) -> Result<()> {
    let path = cancel_requested_file(attempt_dir);
    ensure_parent_dir(&path)?;
    fs::write(path.as_std_path(), requested_at)?;
    Ok(())
}

pub fn is_cancel_requested(attempt_dir: &Utf8Path) -> bool {
    cancel_requested_file(attempt_dir).exists()
}

pub fn clear_cancel_request(attempt_dir: &Utf8Path) -> Result<()> {
    let path = cancel_requested_file(attempt_dir);
    if path.exists() {
        fs::remove_file(path.as_std_path())?;
    }
    Ok(())
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
        let response_path = permission_response_file(attempt_dir, &pending.request_id);
        if response_path.exists() {
            continue;
        }
        write_permission_response(
            attempt_dir,
            &pending.request_id,
            None,
            true,
            decided_at.clone(),
        )?;
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

pub fn wait_for_permission_response(
    attempt_dir: &Utf8Path,
    request_id: &str,
) -> Result<PermissionResponseState> {
    let path = permission_response_file(attempt_dir, request_id);
    eprintln!(
        "[wait_for_permission_response] polling path={} attempt_dir={} request_id={}",
        path, attempt_dir, request_id
    );
    loop {
        if path.exists() {
            eprintln!("[wait_for_permission_response] FOUND response file at {}", path);
            let response = read_json(&path)?;
            let _ = fs::remove_file(path.as_std_path());
            return Ok(response);
        }
        if is_cancel_requested(attempt_dir) {
            return Ok(PermissionResponseState {
                request_id: request_id.to_string(),
                option_id: None,
                cancelled: true,
                decided_at: crate::acp::events::current_timestamp(),
            });
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
