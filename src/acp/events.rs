use std::collections::HashMap;
use std::io::{BufRead, BufReader};

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{append_jsonl, ensure_parent_dir, write_json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMeta {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionMetadata {
    pub adapter_id: String,
    pub adapter_display_name: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    pub started_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimelineItem {
    pub item: AcpUiEvent,
}

#[derive(Debug, Clone)]
pub struct AcpAttemptPaths {
    pub attempt_dir: Utf8PathBuf,
    pub session: Utf8PathBuf,
    pub snapshot: Utf8PathBuf,
    pub events: Utf8PathBuf,
    pub timeline: Utf8PathBuf,
    pub raw: Utf8PathBuf,
    pub diagnostics: Utf8PathBuf,
    pub provider_pid: Utf8PathBuf,
}

impl AcpAttemptPaths {
    pub fn from_attempt_dir(attempt_dir: Utf8PathBuf) -> Self {
        Self {
            session: attempt_dir.join("acp.session.json"),
            snapshot: attempt_dir.join("acp.snapshot.json"),
            events: attempt_dir.join("acp.events.jsonl"),
            timeline: attempt_dir.join("acp.timeline.jsonl"),
            raw: attempt_dir.join("acp.raw.jsonl"),
            diagnostics: attempt_dir.join("acp.diagnostics.jsonl"),
            provider_pid: attempt_dir.join("provider.pid"),
            attempt_dir,
        }
    }
}

/// Read token totals from the ACP session metadata file and timeline.
/// First reads `acp.session.json`, then scans `acp.timeline.jsonl` for usage events
/// to pick up the latest accumulated totals. Returns (input, output, cache_read, total).
pub fn read_session_tokens(session_path: &Utf8Path) -> (u64, u64, u64, u64) {
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_read = 0u64;
    let mut total = 0u64;

    // 1. Read acp.snapshot.json (acp.session.json may not exist)
    let snapshot_path = session_path.parent().map(|p| p.join("acp.snapshot.json"));
    if let Some(ref sp) = snapshot_path {
        if let Ok(contents) = std::fs::read_to_string(sp.as_std_path()) {
            if let Ok(meta) = serde_json::from_str::<AcpSessionMetadata>(&contents) {
                input = meta.input_tokens.unwrap_or(0);
                output = meta.output_tokens.unwrap_or(0);
                cache_read = meta.cached_read_tokens.unwrap_or(0);
                total = meta.total_tokens.unwrap_or(0);
                eprintln!(
                    "[metrics] snapshot.json tokens: input={} output={} cacheRead={} total={}",
                    input, output, cache_read, total
                );
            }
        }
    }

    // 2. Scan timeline for usage events (may have more up-to-date data)
    let timeline_path = session_path.parent().map(|p| p.join("acp.timeline.jsonl"));
    if let Some(ref tp) = timeline_path {
        if let Ok(file) = std::fs::File::open(tp.as_std_path()) {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                if let Ok(line_val) = serde_json::from_str::<serde_json::Value>(&line) {
                    // Unwrap AcpTimelineItem wrapper if present
                    let event = line_val.get("item").unwrap_or(&line_val);
                    let kind = event.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    if kind == "usageUpdate" {
                        if let Some(v) = event.get("inputTokens").and_then(|v| v.as_u64()) {
                            input = input.max(v);
                        }
                        if let Some(v) = event.get("outputTokens").and_then(|v| v.as_u64()) {
                            output = output.max(v);
                        }
                        if let Some(v) = event.get("cachedReadTokens").and_then(|v| v.as_u64()) {
                            cache_read = cache_read.max(v);
                        }
                        if let Some(v) = event.get("totalTokens").and_then(|v| v.as_u64()) {
                            total = total.max(v);
                        }
                    }
                }
            }
            eprintln!(
                "[metrics] timeline tokens (after scan): input={} output={} cacheRead={} total={}",
                input, output, cache_read, total
            );
        } else {
            eprintln!("[metrics] failed to open timeline at {}", tp.as_str());
        }
    } else {
        eprintln!(
            "[metrics] no timeline file found for {}",
            session_path.as_str()
        );
    }

    // 3. Debug: list files in attempt dir and show first timeline line
    let dir = session_path.parent();
    if let Some(d) = dir {
        if let Ok(entries) = std::fs::read_dir(d.as_std_path()) {
            eprintln!("[metrics] attempt_dir files:");
            for e in entries.flatten() {
                eprintln!("[metrics]   {}", e.file_name().to_string_lossy());
            }
        }
    }
    if let Some(ref tp) = timeline_path {
        if let Ok(file) = std::fs::File::open(tp.as_std_path()) {
            let reader = BufReader::new(file);
            for (i, line) in reader.lines().enumerate() {
                if i >= 3 {
                    break;
                }
                if let Ok(l) = line {
                    let preview = l.chars().take(200).collect::<String>();
                    eprintln!("[metrics] timeline[{}]: {}", i, preview);
                }
            }
        }
    }

    (input, output, cache_read, total)
}

pub fn current_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}Z")
}

pub fn append_raw_frame(
    path: &Utf8Path,
    direction: &str,
    frame: Value,
    max_size: u64,
    target_size: u64,
) -> Result<()> {
    append_jsonl(
        path,
        &AcpRawFrame {
            timestamp: current_timestamp(),
            direction: direction.to_string(),
            frame,
        },
    )?;
    let _ = roll_raw_log(path, max_size, target_size);
    Ok(())
}

/// Roll the raw log file, preserving init handshake frames (everything before the first
/// `session/update`) and only trimming the streaming update section.
fn roll_raw_log(path: &Utf8Path, max_size: u64, target_size: u64) -> Result<()> {
    use std::io::Write;
    let meta = match std::fs::metadata(path.as_std_path()) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.len() <= max_size {
        return Ok(());
    }
    let content = std::fs::read(path.as_std_path())?;

    // Find byte offset of the first session/update line — only trim from there onward.
    let mut pinned_bytes = 0usize;
    let marker = br#""method":"session/update""#;
    let mut found_updatable = false;
    for line in content.split_inclusive(|byte| *byte == b'\n') {
        if line.windows(marker.len()).any(|window| window == marker) {
            found_updatable = true;
            break;
        }
        pinned_bytes += line.len();
    }
    if !found_updatable {
        return Ok(());
    }

    let updatable_start = pinned_bytes;
    let updatable_len = content.len().saturating_sub(updatable_start) as u64;
    let pinned_len = pinned_bytes as u64;
    let effective_target = target_size.saturating_sub(pinned_len);
    if updatable_len <= effective_target {
        return Ok(());
    }
    let excess = updatable_len.saturating_sub(effective_target);

    let updatable = &content[updatable_start..];
    let mut cumulative = 0u64;
    let mut drop_bytes = 0usize;
    for line in updatable.split_inclusive(|byte| *byte == b'\n') {
        if cumulative >= excess {
            break;
        }
        cumulative += line.len() as u64;
        drop_bytes += line.len();
    }
    let drop_bytes = drop_bytes.min(updatable.len());

    let mut file = std::fs::File::create(path.as_std_path())?;
    file.write_all(&content[..updatable_start])?;
    file.write_all(&updatable[drop_bytes..])?;
    Ok(())
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

pub fn write_timeline_items(path: &Utf8Path, items: &[AcpUiEvent]) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = std::fs::File::create(path.as_std_path())?;
    for item in items {
        serde_json::to_writer(&mut file, &AcpTimelineItem { item: item.clone() })?;
        use std::io::Write as _;
        file.write_all(b"\n")?;
    }
    Ok(())
}

pub fn load_timeline_items(path: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return Ok(Vec::new());
    };
    let mut legacy_latest_by_item = HashMap::<String, (u64, AcpUiEvent)>::new();
    let mut final_items = Vec::<AcpUiEvent>::new();
    let mut saw_legacy_patch = false;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<AcpTimelineItem>(&line) {
            final_items.push(entry.item);
            continue;
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacyPatch {
            patch_type: String,
            item_id: String,
            revision: u64,
            op: String,
            item: AcpUiEvent,
        }
        let Ok(patch) = serde_json::from_str::<LegacyPatch>(&line) else {
            continue;
        };
        if patch.patch_type != "timelinePatch" || patch.op != "upsert" {
            continue;
        }
        saw_legacy_patch = true;
        let should_replace = legacy_latest_by_item
            .get(&patch.item_id)
            .map(|(revision, _)| patch.revision >= *revision)
            .unwrap_or(true);
        if should_replace {
            legacy_latest_by_item.insert(patch.item_id, (patch.revision, patch.item));
        }
    }
    if saw_legacy_patch {
        let mut items = legacy_latest_by_item
            .into_values()
            .map(|(_, item)| item)
            .collect::<Vec<_>>();
        items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
        return Ok(items);
    }
    final_items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
    Ok(final_items)
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

pub fn latest_timeline_source_seq(path: &Utf8Path) -> u64 {
    load_timeline_items(path)
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.ended_seq.or(item.started_seq).unwrap_or(item.seq))
        .max()
        .unwrap_or(0)
}

pub fn write_session_metadata(path: &Utf8Path, metadata: &AcpSessionMetadata) -> Result<()> {
    write_json(path, metadata)
}

pub fn normalize_session_update(
    seq: u64,
    session_id: Option<String>,
    update: &Value,
) -> AcpUiEvent {
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
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
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

    event
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
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
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
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
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
    attachments: Vec<AttachmentMeta>,
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
    if !attachments.is_empty() {
        raw["attachments"] = serde_json::to_value(&attachments).unwrap_or_default();
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
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
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
        .map(str::to_string)
}

/// 从 usage_update 事件的 raw JSON 中提取结构化 usage 字段。
/// 返回 (used, size, cost_amount_usd)
pub fn extract_usage_fields(raw: &Value) -> (Option<u64>, Option<u64>, Option<f64>) {
    let used = raw.get("used").and_then(Value::as_u64);
    let size = raw.get("size").and_then(Value::as_u64);
    let cost_amount = raw.pointer("/cost/amount").and_then(Value::as_f64);
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
        let raw =
            json!({"used": 12345, "size": 200000, "cost": {"amount": 0.1234, "currency": "USD"}});
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
        assert_eq!(
            kind_to_ui_kind("available_commands_update"),
            "availableCommands"
        );
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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

    // ── read_session_tokens tests ──
    use std::io::Write as _;
    use tempfile::TempDir;

    #[test]
    fn tokens_from_snapshot() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "inputTokens":1000,"outputTokens":500,"cachedReadTokens":200,"totalTokens":1700
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let (i, o, c, t) = super::read_session_tokens(&session_path);
        assert_eq!(i, 1000);
        assert_eq!(o, 500);
        assert_eq!(c, 200);
        assert_eq!(t, 1700);
    }

    #[test]
    fn tokens_no_files_returns_zero() {
        let dir = TempDir::new().unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let (i, o, c, t) = super::read_session_tokens(&session_path);
        assert_eq!((i, o, c, t), (0, 0, 0, 0));
    }

    #[test]
    fn tokens_from_timeline_usage_update_camelcase() {
        let dir = TempDir::new().unwrap();
        let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
        writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":99,"outputTokens":33,"totalTokens":132}}}}"#).unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let (i, o, _c, t) = super::read_session_tokens(&session_path);
        assert_eq!(i, 99);
        assert_eq!(o, 33);
        assert_eq!(t, 132);
    }

    #[test]
    fn tokens_timeline_takes_max_across_events() {
        let dir = TempDir::new().unwrap();
        let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
        writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":100,"outputTokens":10,"totalTokens":110}}}}"#).unwrap();
        writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":500,"outputTokens":20,"totalTokens":520}}}}"#).unwrap();
        writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":300,"outputTokens":5,"totalTokens":305}}}}"#).unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let (i, o, _c, t) = super::read_session_tokens(&session_path);
        assert_eq!(i, 500);
        assert_eq!(o, 20);
        assert_eq!(t, 520);
    }

    #[test]
    fn tokens_ignores_non_usage_events() {
        let dir = TempDir::new().unwrap();
        let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
        writeln!(
            f,
            r#"{{"item":{{"kind":"userTextDelta","content":"hello"}}}}"#
        )
        .unwrap();
        writeln!(f, r#"{{"item":{{"kind":"availableCommands"}}}}"#).unwrap();
        writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":77,"outputTokens":7,"totalTokens":84}}}}"#).unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let (i, o, _c, t) = super::read_session_tokens(&session_path);
        assert_eq!(i, 77);
        assert_eq!(o, 7);
        assert_eq!(t, 84);
    }

    #[test]
    fn roll_raw_log_trims_by_line_bytes_with_unicode_without_trailing_newline() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.raw.jsonl");
        let pinned = r#"{"method":"initialize","content":"固定握手"}"#;
        let update_one = r#"{"method":"session/update","content":"本次任务包含中文内容一"}"#;
        let update_two = r#"{"method":"session/update","content":"本次任务包含中文内容二"}"#;
        std::fs::write(
            path.as_std_path(),
            format!("{pinned}\n{update_one}\n{update_two}"),
        )
        .unwrap();

        super::roll_raw_log(&path, 1, (pinned.len() + 1 + update_two.len()) as u64).unwrap();

        let rolled = std::fs::read_to_string(path.as_std_path()).unwrap();
        assert!(rolled.contains(pinned));
        assert!(rolled.contains(update_two));
        assert!(!rolled.contains(update_one));
    }
}
