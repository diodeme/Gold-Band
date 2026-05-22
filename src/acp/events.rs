use std::io::{BufRead, BufReader};

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{append_jsonl, write_json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionMetadata {
    pub adapter_id: String,
    pub adapter_display_name: String,
    pub cwd: String,
    pub status: String,
    pub restored: bool,
    pub stop_reason: Option<String>,
    pub capabilities: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRawFrame {
    pub timestamp: String,
    pub direction: String,
    pub frame: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpDiagnostic {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpUiEvent {
    pub id: String,
    pub seq: u64,
    pub timestamp: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct AcpAttemptPaths {
    pub attempt_dir: Utf8PathBuf,
    pub session: Utf8PathBuf,
    pub events: Utf8PathBuf,
    pub raw: Utf8PathBuf,
    pub diagnostics: Utf8PathBuf,
    pub provider_pid: Utf8PathBuf,
}

impl AcpAttemptPaths {
    pub fn from_attempt_dir(attempt_dir: Utf8PathBuf) -> Self {
        Self {
            session: attempt_dir.join("acp.session.json"),
            events: attempt_dir.join("acp.events.jsonl"),
            raw: attempt_dir.join("acp.raw.jsonl"),
            diagnostics: attempt_dir.join("acp.diagnostics.jsonl"),
            provider_pid: attempt_dir.join("provider.pid"),
            attempt_dir,
        }
    }
}

pub fn current_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}Z")
}

pub fn append_raw_frame(path: &Utf8Path, direction: &str, frame: Value) -> Result<()> {
    append_jsonl(
        path,
        &AcpRawFrame {
            timestamp: current_timestamp(),
            direction: direction.to_string(),
            frame,
        },
    )
}

pub fn append_diagnostic(
    path: &Utf8Path,
    level: impl Into<String>,
    message: impl Into<String>,
    data: Option<Value>,
) -> Result<()> {
    append_jsonl(
        path,
        &AcpDiagnostic {
            timestamp: current_timestamp(),
            level: level.into(),
            message: message.into(),
            data,
        },
    )
}

pub fn append_ui_event(path: &Utf8Path, event: &AcpUiEvent) -> Result<()> {
    append_jsonl(path, event)
}

pub fn initial_acp_event_seq(path: &Utf8Path) -> u64 {
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return 0;
    };
    BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .count() as u64
}

pub fn write_session_metadata(path: &Utf8Path, metadata: &AcpSessionMetadata) -> Result<()> {
    write_json(path, metadata)
}

pub fn normalize_session_update(
    seq: u64,
    session_id: Option<String>,
    update: &Value,
) -> Vec<AcpUiEvent> {
    let timestamp = current_timestamp();
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let raw = Some(update.clone());
    let id = format!("acp-event-{seq}");
    let mut event = AcpUiEvent {
        id,
        seq,
        timestamp,
        kind: kind_to_ui_kind(kind).to_string(),
        session_id,
        content: extract_text(update),
        title: extract_title(update),
        tool_call_id: extract_tool_call_id(update),
        status: extract_status(update),
        raw,
    };

    if event.content.is_none()
        && matches!(
            kind,
            "agent_message_chunk" | "user_message_chunk" | "agent_thought_chunk"
        )
    {
        event.content = Some(String::new());
    }

    vec![event]
}

pub fn permission_request_event(seq: u64, request_id: String, params: Value) -> AcpUiEvent {
    AcpUiEvent {
        id: request_id,
        seq,
        timestamp: current_timestamp(),
        kind: "permissionRequest".to_string(),
        session_id: params
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
        content: None,
        title: extract_title(&params).or_else(|| Some("Permission required".to_string())),
        tool_call_id: extract_tool_call_id(&params),
        status: Some("pending".to_string()),
        raw: Some(params),
    }
}

pub fn permission_decision_event(
    seq: u64,
    request_id: String,
    option_id: Option<String>,
) -> AcpUiEvent {
    AcpUiEvent {
        id: request_id.clone(),
        seq,
        timestamp: current_timestamp(),
        kind: "permissionRequest".to_string(),
        session_id: None,
        content: None,
        title: Some("Permission answered".to_string()),
        tool_call_id: None,
        status: Some("selected".to_string()),
        raw: Some(serde_json::json!({
            "requestId": request_id,
            "optionId": option_id,
        })),
    }
}

pub fn user_prompt_event(
    seq: u64,
    session_id: String,
    content: String,
    prompt_id: Option<String>,
) -> AcpUiEvent {
    let mut raw = serde_json::json!({
        "source": "goldBandPrompt",
        "synthetic": true,
    });
    if let Some(prompt_id) = prompt_id {
        raw["promptId"] = Value::String(prompt_id);
    }
    AcpUiEvent {
        id: format!("gold-band-user-prompt-{seq}"),
        seq,
        timestamp: current_timestamp(),
        kind: "userTextDelta".to_string(),
        session_id: Some(session_id),
        content: Some(content),
        title: Some("User prompt".to_string()),
        tool_call_id: None,
        status: Some("completed".to_string()),
        raw: Some(raw),
    }
}

fn kind_to_ui_kind(kind: &str) -> &str {
    match kind {
        "agent_message_chunk" => "textDelta",
        "user_message_chunk" => "userTextDelta",
        "agent_thought_chunk" => "thoughtDelta",
        "tool_call" => "toolCall",
        "tool_call_update" => "toolCallUpdate",
        "plan" => "plan",
        "available_commands_update" => "availableCommands",
        "usage_update" => "usageUpdate",
        "current_mode_update" => "modeUpdate",
        "config_option_update" => "configUpdate",
        "session_info_update" => "sessionInfo",
        _ => "rawDiagnostic",
    }
}

fn extract_text(value: &Value) -> Option<String> {
    value
        .pointer("/content/text")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/content/content/text")
                .and_then(Value::as_str)
        })
        .or_else(|| value.get("text").and_then(Value::as_str))
        .map(str::to_string)
}

fn extract_title(value: &Value) -> Option<String> {
    value
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/toolCall/title").and_then(Value::as_str))
        .or_else(|| {
            value
                .pointer("/toolCall/fields/title")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

fn extract_tool_call_id(value: &Value) -> Option<String> {
    value
        .get("toolCallId")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/toolCallId").and_then(Value::as_str))
        .or_else(|| {
            value
                .pointer("/toolCall/toolCallId")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .pointer("/toolCall/toolCallId")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

fn extract_status(value: &Value) -> Option<String> {
    value
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/fields/status").and_then(Value::as_str))
        .or_else(|| value.pointer("/toolCall/status").and_then(Value::as_str))
        .or_else(|| {
            value
                .pointer("/toolCall/fields/status")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::user_prompt_event;

    #[test]
    fn user_prompt_event_persists_prompt_id_metadata() {
        let event = user_prompt_event(
            7,
            "session-123".to_string(),
            "继续".to_string(),
            Some("prompt-123".to_string()),
        );
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("promptId"))
                .and_then(|value| value.as_str()),
            Some("prompt-123")
        );
    }

    #[test]
    fn user_prompt_event_omits_prompt_id_when_absent() {
        let event = user_prompt_event(7, "session-123".to_string(), "继续".to_string(), None);
        assert_eq!(event.raw.as_ref().and_then(|raw| raw.get("promptId")), None);
    }
}
