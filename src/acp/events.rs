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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
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
    hidden_from_chat: bool,
) -> AcpUiEvent {
    let mut raw = serde_json::json!({
        "source": "goldBandPrompt",
        "synthetic": true,
    });
    if let Some(prompt_id) = prompt_id {
        raw["promptId"] = Value::String(prompt_id);
    }
    if hidden_from_chat {
        raw["hiddenFromChat"] = Value::Bool(true);
        raw["reason"] = Value::String("invalidOutputRepair".to_string());
    }
    AcpUiEvent {
        id: format!("gold-band-user-prompt-{seq}"),
        seq,
        timestamp: current_timestamp(),
        kind: "userTextDelta".to_string(),
        session_id: Some(session_id),
        content: (!hidden_from_chat).then_some(content),
        title: Some(if hidden_from_chat {
            "Hidden prompt".to_string()
        } else {
            "User prompt".to_string()
        }),
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

/// 从 usage_update 事件的 raw JSON 中提取结构化 usage 字段。
/// 返回 (used, size, cost_amount_usd)
pub fn extract_usage_fields(raw: &Value) -> (Option<u64>, Option<u64>, Option<f64>) {
    let used = raw.get("used").and_then(Value::as_u64);
    let size = raw.get("size").and_then(Value::as_u64);
    let cost_amount = raw
        .pointer("/cost/amount")
        .and_then(Value::as_f64);
    (used, size, cost_amount)
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
    use super::{extract_usage_fields, kind_to_ui_kind, user_prompt_event};
    use serde_json::json;

    // --- extract_usage_fields ---

    #[test]
    fn extract_usage_all_fields() {
        let raw = json!({"used": 12345, "size": 200000, "cost": {"amount": 0.1234, "currency": "USD"}});
        let (used, size, cost) = extract_usage_fields(&raw);
        assert_eq!(used, Some(12345));
        assert_eq!(size, Some(200000));
        assert!(cost.is_some());
        assert!((cost.unwrap() - 0.1234).abs() < 0.0001);
    }

    #[test]
    fn extract_usage_only_used_and_size() {
        let raw = json!({"used": 5000, "size": 200000});
        let (used, size, cost) = extract_usage_fields(&raw);
        assert_eq!(used, Some(5000));
        assert_eq!(size, Some(200000));
        assert_eq!(cost, None);
    }

    #[test]
    fn extract_usage_post_compaction() {
        let raw = json!({"used": 0, "size": 200000});
        let (used, size, cost) = extract_usage_fields(&raw);
        assert_eq!(used, Some(0));
        assert_eq!(size, Some(200000));
        assert_eq!(cost, None);
    }

    #[test]
    fn extract_usage_empty_object() {
        let raw = json!({});
        let (used, size, cost) = extract_usage_fields(&raw);
        assert_eq!(used, None);
        assert_eq!(size, None);
        assert_eq!(cost, None);
    }

    #[test]
    fn extract_usage_missing_cost_amount() {
        let raw = json!({"used": 100, "cost": {"currency": "USD"}});
        let (used, size, cost) = extract_usage_fields(&raw);
        assert_eq!(used, Some(100));
        assert_eq!(size, None);
        assert_eq!(cost, None);
    }

    #[test]
    fn extract_usage_used_is_not_string() {
        // used is a string instead of a number — should return None
        let raw = json!({"used": "abc", "size": 200000});
        let (used, size, _cost) = extract_usage_fields(&raw);
        assert_eq!(used, None);
        assert_eq!(size, Some(200000));
    }

    // --- kind_to_ui_kind ---

    #[test]
    fn kind_to_ui_agent_message_chunk() {
        assert_eq!(kind_to_ui_kind("agent_message_chunk"), "textDelta");
    }

    #[test]
    fn kind_to_ui_user_message_chunk() {
        assert_eq!(kind_to_ui_kind("user_message_chunk"), "userTextDelta");
    }

    #[test]
    fn kind_to_ui_agent_thought_chunk() {
        assert_eq!(kind_to_ui_kind("agent_thought_chunk"), "thoughtDelta");
    }

    #[test]
    fn kind_to_ui_tool_call() {
        assert_eq!(kind_to_ui_kind("tool_call"), "toolCall");
    }

    #[test]
    fn kind_to_ui_tool_call_update() {
        assert_eq!(kind_to_ui_kind("tool_call_update"), "toolCallUpdate");
    }

    #[test]
    fn kind_to_ui_plan() {
        assert_eq!(kind_to_ui_kind("plan"), "plan");
    }

    #[test]
    fn kind_to_ui_usage_update() {
        assert_eq!(kind_to_ui_kind("usage_update"), "usageUpdate");
    }

    #[test]
    fn kind_to_ui_available_commands_update() {
        assert_eq!(kind_to_ui_kind("available_commands_update"), "availableCommands");
    }

    #[test]
    fn kind_to_ui_current_mode_update() {
        assert_eq!(kind_to_ui_kind("current_mode_update"), "modeUpdate");
    }

    #[test]
    fn kind_to_ui_config_option_update() {
        assert_eq!(kind_to_ui_kind("config_option_update"), "configUpdate");
    }

    #[test]
    fn kind_to_ui_session_info_update() {
        assert_eq!(kind_to_ui_kind("session_info_update"), "sessionInfo");
    }

    #[test]
    fn kind_to_ui_unknown_falls_back_to_raw_diagnostic() {
        assert_eq!(kind_to_ui_kind("some_future_event"), "rawDiagnostic");
    }

    #[test]
    fn kind_to_ui_empty_string_is_raw_diagnostic() {
        assert_eq!(kind_to_ui_kind(""), "rawDiagnostic");
    }

    // --- existing tests ---

    #[test]
    fn user_prompt_event_persists_prompt_id_metadata() {
        let event = user_prompt_event(
            7,
            "session-123".to_string(),
            "继续".to_string(),
            Some("prompt-123".to_string()),
            false,
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
        let event = user_prompt_event(
            7,
            "session-123".to_string(),
            "继续".to_string(),
            None,
            false,
        );
        assert_eq!(event.raw.as_ref().and_then(|raw| raw.get("promptId")), None);
    }

    #[test]
    fn hidden_user_prompt_event_redacts_content() {
        let event = user_prompt_event(
            7,
            "session-123".to_string(),
            "repair".to_string(),
            None,
            true,
        );
        assert_eq!(event.content, None);
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("hiddenFromChat"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }
}
