use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};

use agent_client_protocol_schema::v1::{CreateElicitationRequest, ElicitationScope};
use anyhow::Result;
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::control::AcpRuntimeControlCursor;
use crate::artifacts::json_artifact_display_span;
use crate::provider::UserPromptQuote;
use crate::storage::{
    append_jsonl, append_jsonl_unlocked, ensure_parent_dir, read_json, with_jsonl_file_lock,
    write_json,
};

const AGENT_TRANSCRIPT_META_KEY: &str = "agentTranscript";
const CLAUDE_CODE_META_KEY: &str = "claudeCode";
const CLAUDE_AGENT_TOOL_NAMES: [&str; 2] = ["agent", "task"];

fn is_false(value: &bool) -> bool {
    !*value
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub availability: AcpSessionAvailability,
    #[serde(default)]
    pub latest_turn_status: AcpLatestTurnStatus,
    pub restored: bool,
    pub stop_reason: Option<String>,
    pub capabilities: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode_override: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_option_overrides: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_append: Option<String>,
    /// Retry lifecycle of the latest logical prompt. Unlike session activity,
    /// this survives terminal session status so a rebuilt provider runtime can
    /// continue the same user turn without scanning the timeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_retry: Option<AcpPromptRetryState>,
    /// Latest invocation-level Runtime/NonRuntime transition. This lives in
    /// existing ACP session metadata so terminal snapshot rewrites preserve
    /// the one-time suspension-context cursor without a new state file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_control: Option<AcpRuntimeControlCursor>,
    /// Negative cache for legacy attempts without Runtime control metadata.
    /// Once set, ordinary conversation turns never rescan the full timeline.
    #[serde(default, skip_serializing_if = "is_false")]
    pub runtime_control_timeline_scan_complete: bool,
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
    /// Cumulative token usage across every prompt turn in this ACP attempt.
    /// The legacy fields above remain the latest prompt snapshot for metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_cached_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_cached_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<AcpSessionTiming>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpSessionAvailability {
    Established,
    Restorable,
    #[default]
    Unavailable,
    Closing,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpLatestTurnStatus {
    #[default]
    None,
    Completed,
    Cancelled,
    Failed,
}

/// Durable retry identity for the latest logical prompt turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptRetryState {
    pub prompt_id: String,
    pub retry_attempt: u32,
    /// Canonical timeline identity of this logical prompt. Provider runtime
    /// retries upsert this event instead of appending another user bubble.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_event_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_event_timestamp: Option<String>,
    /// Hidden repair prompts can reuse a visible promptId, but they are a
    /// different lifecycle and must never overwrite the visible user event.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden_from_chat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRawFrame {
    pub timestamp: String,
    pub direction: String,
    pub frame: Value,
}

/// Preserve durable tool-call evidence across provider revisions. Providers
/// commonly send input and diff content before a terminal status-only update;
/// the terminal revision must not erase fields that it does not replace.
pub fn merge_tool_revision_raw(incoming: &mut Value, previous: &Value) {
    let (Some(incoming_object), Some(previous_object)) =
        (incoming.as_object_mut(), previous.as_object())
    else {
        return;
    };
    for key in ["rawInput", "content", "locations"] {
        merge_missing_json_field(incoming_object, previous_object, key);
    }
    if let Some(previous_tool_call) = previous_object.get("toolCall").and_then(Value::as_object) {
        let incoming_tool_call = incoming_object
            .entry("toolCall")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(incoming_tool_call) = incoming_tool_call.as_object_mut() {
            for key in ["rawInput", "content", "locations"] {
                merge_missing_json_field(incoming_tool_call, previous_tool_call, key);
            }
        }
    }
}

fn merge_missing_json_field(
    incoming: &mut serde_json::Map<String, Value>,
    previous: &serde_json::Map<String, Value>,
    key: &str,
) {
    let incoming_has_value = incoming.get(key).is_some_and(json_value_has_payload);
    if incoming_has_value {
        return;
    }
    if let Some(previous_value) = previous
        .get(key)
        .filter(|value| json_value_has_payload(value))
    {
        incoming.insert(key.to_string(), previous_value.clone());
    }
}

fn json_value_has_payload(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpDiagnostic {
    pub timestamp: String,
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
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
    pub timing: Option<AcpTimingPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTranscriptRelation {
    #[serde(default, skip_serializing_if = "is_false")]
    pub agent_launch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
}

impl AgentTranscriptRelation {
    fn is_empty(&self) -> bool {
        !self.agent_launch && self.tool_name.is_none() && self.parent_tool_call_id.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimingPatch {
    pub session_elapsed_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_last_activity_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_wait_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_wait_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_reason: Option<String>,
    pub paused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpSessionTiming {
    pub session_elapsed_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_turn_last_activity_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_wait_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_wait_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_reason: Option<String>,
    pub paused: bool,
}

#[derive(Debug, Default, Clone)]
pub struct AcpTimingState {
    elapsed_seconds: u64,
    active_turn_started_at: Option<u64>,
    active_turn_last_activity_at: Option<u64>,
    revision: Option<u64>,
    saw_turn: bool,
    pending_permission_ids: HashSet<String>,
    pending_elicitation_ids: HashSet<String>,
    user_wait_started_at: Option<u64>,
    user_wait_seconds: u64,
}

impl AcpTimingState {
    pub fn from_timeline_items(items: impl IntoIterator<Item = AcpUiEvent>) -> Self {
        let mut state = Self::default();
        let mut items = items.into_iter().collect::<Vec<_>>();
        items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
        for item in &items {
            state.observe_event(item);
        }
        state
    }

    pub fn from_timeline_item_refs<'a>(items: impl IntoIterator<Item = &'a AcpUiEvent>) -> Self {
        let mut state = Self::default();
        let mut items = items.into_iter().collect::<Vec<_>>();
        items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
        for item in items {
            state.observe_event(item);
        }
        state
    }

    pub fn observe_event(&mut self, event: &AcpUiEvent) {
        self.revision = Some(
            self.revision
                .unwrap_or_default()
                .max(timing_revision_for_event(event)),
        );
        if is_gold_band_user_prompt_event(event) {
            self.elapsed_seconds = self
                .elapsed_seconds
                .saturating_add(self.finish_current_turn(false, None));
            self.active_turn_started_at = parse_epoch_timestamp(&event.timestamp);
            self.active_turn_last_activity_at = None;
            self.pending_permission_ids.clear();
            self.pending_elicitation_ids.clear();
            self.user_wait_started_at = None;
            self.user_wait_seconds = 0;
            self.saw_turn = self.active_turn_started_at.is_some();
            return;
        }
        if self.active_turn_started_at.is_none() {
            return;
        }
        let Some(timestamp) = parse_epoch_timestamp(&event.timestamp) else {
            return;
        };
        self.observe_permission_event(event, timestamp);
        self.observe_elicitation_event(event, timestamp);
        if is_session_elapsed_progress_event(event) {
            self.active_turn_last_activity_at = Some(timestamp);
        }
    }

    pub fn patch_at(&self, now: u64, reason: impl Into<String>) -> Option<AcpTimingPatch> {
        self.patch_at_with_revision(
            now,
            reason,
            self.revision,
            Some(format_epoch_timestamp(now)),
        )
    }

    pub fn patch_at_with_revision(
        &self,
        now: u64,
        reason: impl Into<String>,
        revision: Option<u64>,
        observed_at: Option<String>,
    ) -> Option<AcpTimingPatch> {
        self.snapshot_at_with_revision(true, Some(now), revision, observed_at)
            .map(|snapshot| AcpTimingPatch {
                session_elapsed_seconds: snapshot.session_elapsed_seconds,
                revision: snapshot.revision,
                observed_at: snapshot.observed_at,
                active_turn_started_at: snapshot.active_turn_started_at,
                active_turn_last_activity_at: snapshot.active_turn_last_activity_at,
                permission_wait_started_at: snapshot.permission_wait_started_at,
                user_wait_started_at: snapshot.user_wait_started_at,
                wait_reason: snapshot.wait_reason,
                paused: snapshot.paused,
                reason: Some(reason.into()),
            })
    }

    pub fn snapshot(&self, session_active: bool) -> Option<AcpSessionTiming> {
        self.snapshot_at(session_active, None)
    }

    pub fn terminal_snapshot(&self) -> Option<AcpSessionTiming> {
        self.terminal_snapshot_at(None)
    }

    pub fn snapshot_at(&self, session_active: bool, now: Option<u64>) -> Option<AcpSessionTiming> {
        self.snapshot_at_with_revision(
            session_active,
            now,
            self.revision,
            now.map(format_epoch_timestamp),
        )
    }

    pub fn snapshot_at_with_revision(
        &self,
        session_active: bool,
        now: Option<u64>,
        revision: Option<u64>,
        observed_at: Option<String>,
    ) -> Option<AcpSessionTiming> {
        if !self.saw_turn {
            return None;
        }
        let paused = self.user_wait_started_at.is_some();
        let anchor = if session_active {
            if paused {
                self.user_wait_started_at
                    .or(self.active_turn_last_activity_at)
                    .or(self.active_turn_started_at)
            } else {
                Some(now.unwrap_or_else(current_epoch_seconds))
            }
        } else {
            None
        };
        let current_turn_elapsed_seconds = if session_active {
            self.finish_current_turn(true, anchor)
        } else {
            self.finish_current_turn(false, None)
        };
        let session_elapsed_seconds = self
            .elapsed_seconds
            .saturating_add(current_turn_elapsed_seconds);
        Some(AcpSessionTiming {
            session_elapsed_seconds,
            revision,
            observed_at,
            active_turn_started_at: session_active
                .then_some(self.active_turn_started_at)
                .flatten()
                .map(format_epoch_timestamp),
            active_turn_last_activity_at: anchor.map(format_epoch_timestamp),
            permission_wait_started_at: self
                .user_wait_started_at
                .filter(|_| !self.pending_permission_ids.is_empty())
                .map(format_epoch_timestamp),
            user_wait_started_at: self.user_wait_started_at.map(format_epoch_timestamp),
            wait_reason: self.wait_reason().map(str::to_string),
            paused: paused || !session_active,
        })
    }

    pub fn terminal_snapshot_at(&self, now: Option<u64>) -> Option<AcpSessionTiming> {
        self.terminal_snapshot_at_with_revision(now, self.revision, now.map(format_epoch_timestamp))
    }

    pub fn terminal_snapshot_at_with_revision(
        &self,
        now: Option<u64>,
        revision: Option<u64>,
        observed_at: Option<String>,
    ) -> Option<AcpSessionTiming> {
        let mut snapshot = self.snapshot_at_with_revision(true, now, revision, observed_at)?;
        snapshot.active_turn_started_at = None;
        snapshot.active_turn_last_activity_at = None;
        snapshot.permission_wait_started_at = None;
        snapshot.user_wait_started_at = None;
        snapshot.wait_reason = None;
        snapshot.paused = true;
        Some(snapshot)
    }

    fn finish_current_turn(&self, session_active: bool, now: Option<u64>) -> u64 {
        let Some(started_at) = self.active_turn_started_at else {
            return 0;
        };
        let end_at = if session_active {
            now.unwrap_or_else(current_epoch_seconds)
        } else {
            self.active_turn_last_activity_at.unwrap_or(started_at)
        };
        let base_elapsed = end_at.saturating_sub(started_at);
        base_elapsed.saturating_sub(
            self.user_wait_seconds
                .saturating_add(self.open_user_wait(end_at)),
        )
    }

    fn open_user_wait(&self, end_at: u64) -> u64 {
        self.user_wait_started_at
            .map(|started_at| end_at.saturating_sub(started_at))
            .unwrap_or_default()
    }

    fn observe_permission_event(&mut self, event: &AcpUiEvent, timestamp: u64) {
        if event.kind != "permissionRequest" {
            return;
        }
        let is_pending = event
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("pending"));
        let request_id = canonical_permission_request_id(event);
        if is_pending {
            let was_waiting = self.is_waiting_for_user();
            if self.pending_permission_ids.insert(request_id) && !was_waiting {
                self.user_wait_started_at = Some(timestamp);
            }
            return;
        }
        if !self.pending_permission_ids.remove(&request_id) {
            if let Some(started_at) = compacted_wait_started_at(event, timestamp) {
                self.add_closed_user_wait(started_at, timestamp);
            }
            return;
        }
        self.close_user_wait_if_idle(timestamp);
    }

    fn observe_elicitation_event(&mut self, event: &AcpUiEvent, timestamp: u64) {
        match event.kind.as_str() {
            "elicitationRequest"
                if event
                    .status
                    .as_deref()
                    .is_some_and(|status| status.eq_ignore_ascii_case("pending")) =>
            {
                let was_waiting = self.is_waiting_for_user();
                if self.pending_elicitation_ids.insert(event.id.clone()) && !was_waiting {
                    self.user_wait_started_at = Some(timestamp);
                }
            }
            "elicitationResponse" => {
                let raw_id = event
                    .raw
                    .as_ref()
                    .and_then(|raw| raw.get("elicitationId"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| event.id.trim_end_matches("-response").to_string());
                if self.pending_elicitation_ids.remove(&raw_id) {
                    self.close_user_wait_if_idle(timestamp);
                } else if let Some(started_at) = compacted_wait_started_at(event, timestamp) {
                    self.add_closed_user_wait(started_at, timestamp);
                }
            }
            "elicitationRequest" => {
                if let Some(started_at) = compacted_wait_started_at(event, timestamp) {
                    self.add_closed_user_wait(started_at, timestamp);
                }
            }
            _ => {}
        }
    }

    fn is_waiting_for_user(&self) -> bool {
        !self.pending_permission_ids.is_empty() || !self.pending_elicitation_ids.is_empty()
    }

    fn close_user_wait_if_idle(&mut self, timestamp: u64) {
        if self.is_waiting_for_user() {
            return;
        }
        if let Some(started_at) = self.user_wait_started_at.take() {
            self.user_wait_seconds = self
                .user_wait_seconds
                .saturating_add(timestamp.saturating_sub(started_at));
        }
    }

    fn add_closed_user_wait(&mut self, started_at: u64, ended_at: u64) {
        if ended_at <= started_at {
            return;
        }
        let effective_end = self
            .user_wait_started_at
            .map(|open_started_at| ended_at.min(open_started_at))
            .unwrap_or(ended_at);
        if effective_end <= started_at {
            return;
        }
        self.user_wait_seconds = self
            .user_wait_seconds
            .saturating_add(effective_end.saturating_sub(started_at));
    }

    fn wait_reason(&self) -> Option<&'static str> {
        if !self.pending_permission_ids.is_empty() {
            Some("permission")
        } else if !self.pending_elicitation_ids.is_empty() {
            Some("elicitation")
        } else {
            None
        }
    }
}

fn compacted_wait_started_at(event: &AcpUiEvent, ended_at: u64) -> Option<u64> {
    let started_at = event
        .started_at
        .as_deref()
        .and_then(parse_epoch_timestamp)?;
    (started_at < ended_at).then_some(started_at)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimelineItem {
    pub item: AcpUiEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimelinePatch {
    pub patch_type: String,
    pub item_id: String,
    pub revision: u64,
    pub op: String,
    pub item: AcpUiEvent,
}

#[derive(Debug, Clone)]
pub struct AcpAttemptPaths {
    pub attempt_dir: Utf8PathBuf,
    pub session: Utf8PathBuf,
    pub snapshot: Utf8PathBuf,
    pub events: Utf8PathBuf,
    pub timeline: Utf8PathBuf,
    pub prompt_usage: Utf8PathBuf,
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
            prompt_usage: attempt_dir.join("acp.prompt-usage.jsonl"),
            raw: attempt_dir.join("acp.raw.jsonl"),
            diagnostics: attempt_dir.join("acp.diagnostics.jsonl"),
            provider_pid: attempt_dir.join("provider.pid"),
            attempt_dir,
        }
    }
}

fn parse_epoch_timestamp(value: &str) -> Option<u64> {
    value.trim_end_matches('Z').parse::<u64>().ok()
}

fn format_epoch_timestamp(value: u64) -> String {
    format!("{value}Z")
}

fn timing_revision_for_event(event: &AcpUiEvent) -> u64 {
    event
        .ended_seq
        .or(event.started_seq)
        .unwrap_or(event.seq)
        .max(event.seq)
}

fn current_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn is_gold_band_user_prompt_event(event: &AcpUiEvent) -> bool {
    event.kind == "userTextDelta"
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            == Some("goldBandPrompt")
}

fn is_session_elapsed_progress_event(event: &AcpUiEvent) -> bool {
    let session_update = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionUpdate"))
        .and_then(Value::as_str);
    !matches!(
        session_update,
        Some("available_commands_update" | "current_mode_update" | "session_info_update")
    )
}

fn canonical_permission_request_id(event: &AcpUiEvent) -> String {
    let value = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("requestId"))
        .and_then(Value::as_str)
        .unwrap_or(event.id.as_str());
    let mut current = value;
    while let Some(next) = current.strip_prefix("permission-") {
        current = next;
    }
    current.to_string()
}

/// Read token totals from the ACP session metadata file and timeline.
/// First reads `acp.snapshot.json`, then scans `acp.timeline.jsonl` for usage events
/// to pick up the latest accumulated totals. Returns (input, output, cache_read, total).
/// Token and timing metrics read from an ACP session's persisted files.
pub struct SessionMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    pub session_elapsed_seconds: u64,
}

/// Read token totals from the ACP session metadata file and timeline.
/// First reads `acp.snapshot.json`, then scans `acp.timeline.jsonl` for usage events
/// to pick up the latest accumulated totals. Returns (input, output, cache_read, total).
pub fn read_session_tokens(session_path: &Utf8Path) -> (u64, u64, u64, u64) {
    let m = read_session_metrics(session_path);
    (
        m.input_tokens,
        m.output_tokens,
        m.cache_read_tokens,
        m.total_tokens,
    )
}

/// Read token totals and session elapsed seconds from the ACP session metadata
/// file and timeline. Token counts are taken as the max of snapshot and timeline
/// `usageUpdate` events. `session_elapsed_seconds` comes from the snapshot's
/// `timing` field.
pub fn read_session_metrics(session_path: &Utf8Path) -> SessionMetrics {
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_read = 0u64;
    let mut total = 0u64;
    let mut session_elapsed_seconds = 0u64;

    // 1. Read acp.snapshot.json (acp.session.json may not exist)
    let snapshot_path = session_path.parent().map(|p| p.join("acp.snapshot.json"));
    if let Some(ref sp) = snapshot_path {
        if let Ok(meta) = load_session_metadata(sp, None) {
            input = meta.input_tokens.unwrap_or(0);
            output = meta.output_tokens.unwrap_or(0);
            cache_read = meta.cached_read_tokens.unwrap_or(0);
            total = meta.total_tokens.unwrap_or(0);
            if let Some(t) = &meta.timing {
                session_elapsed_seconds = t.session_elapsed_seconds;
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
        }
    }

    SessionMetrics {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        total_tokens: total,
        session_elapsed_seconds,
    }
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
    with_jsonl_file_lock(path, || {
        append_jsonl_unlocked(
            path,
            &AcpRawFrame {
                timestamp: current_timestamp(),
                direction: direction.to_string(),
                frame,
            },
        )?;
        let _ = roll_raw_log(path, max_size, target_size);
        Ok(())
    })
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
            code: None,
            message: message.into(),
            data,
        },
    )
}

pub fn append_structured_diagnostic(
    path: &Utf8Path,
    level: impl Into<String>,
    code: impl Into<String>,
    data: Option<Value>,
) -> Result<()> {
    let code = code.into();
    append_jsonl(
        path,
        &AcpDiagnostic {
            timestamp: current_timestamp(),
            level: level.into(),
            code: Some(code.clone()),
            message: code,
            data,
        },
    )
}

pub fn append_ui_event(path: &Utf8Path, event: &AcpUiEvent) -> Result<()> {
    append_jsonl(path, event)
}

pub fn write_timeline_items(path: &Utf8Path, items: &[AcpUiEvent]) -> Result<()> {
    with_jsonl_file_lock(path, || {
        ensure_parent_dir(path)?;
        let mut file = AtomicWriteFile::open(path.as_std_path())?;
        for item in items {
            let mut item = item.clone();
            crate::acp::timeline::externalize_timeline_event_for_storage(path, &mut item)?;
            serde_json::to_writer(&mut file, &AcpTimelineItem { item })?;
            use std::io::Write as _;
            file.write_all(b"\n")?;
        }
        file.commit()?;
        Ok(())
    })
}

pub fn append_timeline_patch(
    path: &Utf8Path,
    item_id: impl Into<String>,
    revision: u64,
    item: &AcpUiEvent,
) -> Result<()> {
    let item_id = item_id.into();
    let mut item = item.clone();
    item.id = item_id;
    crate::acp::timeline::upsert_timeline_item(
        path,
        revision,
        &item,
        crate::acp::timeline::TimelineCompactionPolicy::default(),
    )?;
    Ok(())
}

/// Settle the latest durable retry prompt at the attempt boundary. Stop can
/// arrive while no ACP runtime owns an active prompt (for example during
/// retry backoff), so cancellation cannot depend on runtime-local state.
pub fn cancel_latest_processing_prompt_retry(path: &Utf8Path, decided_at: String) -> Result<bool> {
    with_jsonl_file_lock(path, || {
        let mut items = load_timeline_items_unlocked(path)?;
        let Some(event) = items
            .iter_mut()
            .filter(|event| {
                is_gold_band_user_prompt_event(event)
                    && event.status.as_deref() == Some("processing")
                    && event
                        .raw
                        .as_ref()
                        .and_then(|raw| raw.pointer("/retry/attempt"))
                        .and_then(Value::as_u64)
                        .is_some_and(|attempt| attempt > 0)
            })
            .max_by_key(|event| event.ended_seq.unwrap_or(event.seq))
        else {
            return Ok(false);
        };
        let revision = latest_timeline_source_seq_unlocked(path)?.saturating_add(1);
        event.status = Some("cancelled".to_string());
        event.ended_seq = Some(revision);
        event.ended_at = Some(decided_at);
        let raw = event.raw.get_or_insert_with(|| serde_json::json!({}));
        if !raw.is_object() {
            *raw = serde_json::json!({});
        }
        raw["cancelled"] = Value::Bool(true);
        let mut storage_event = event.clone();
        crate::acp::timeline::externalize_timeline_event_for_storage(path, &mut storage_event)?;
        append_jsonl_unlocked(
            path,
            &AcpTimelinePatch {
                patch_type: "timelinePatch".to_string(),
                item_id: storage_event.id.clone(),
                revision,
                op: "upsert".to_string(),
                item: storage_event,
            },
        )?;
        Ok(true)
    })
}

pub fn load_timeline_items(path: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    with_jsonl_file_lock(path, || load_timeline_items_unlocked(path))
}

pub fn annotate_latest_runtime_control_output(
    path: &Utf8Path,
    artifact_name: &str,
    kind: &str,
) -> Result<bool> {
    let mut items = load_timeline_items(path)?;
    let Some(index) = items
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| {
            if item.kind != "textDelta" {
                return None;
            }
            let content = item.content.as_deref()?;
            json_artifact_display_span(content).map(|span| (index, span))
        })
        .map(|(index, _)| index)
    else {
        return Ok(false);
    };

    let content = items[index].content.as_deref().unwrap_or_default();
    let Some(span) = json_artifact_display_span(content) else {
        return Ok(false);
    };
    let display = serde_json::json!({
        "artifactName": artifact_name,
        "kind": kind,
        "jsonText": span.json_text,
        "start": utf16_index(content, span.start),
        "end": utf16_index(content, span.end),
        "jsonStart": utf16_index(content, span.json_start),
        "jsonEnd": utf16_index(content, span.json_end),
        "fenced": span.fenced,
        "parseStatus": span.parse_status,
    });

    let item = &mut items[index];
    let raw = item.raw.get_or_insert_with(|| serde_json::json!({}));
    if !raw.is_object() {
        *raw = serde_json::json!({});
    }
    if let Some(object) = raw.as_object_mut() {
        object.insert("runtimeControlOutputDisplay".to_string(), display);
    }
    let revision = latest_timeline_source_seq(path).saturating_add(1);
    append_timeline_patch(path, item.id.clone(), revision, item)?;
    Ok(true)
}

fn utf16_index(content: &str, byte_index: usize) -> usize {
    content[..byte_index].encode_utf16().count()
}

pub(crate) fn load_timeline_items_unlocked(path: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    let mut items = load_timeline_items_for_storage_unlocked(path)?;
    for item in &mut items {
        if let Some(raw) = item.raw.as_mut() {
            crate::acp::timeline::hydrate_timeline_value(path, raw)?;
        }
    }
    Ok(items)
}

pub(crate) fn load_timeline_items_for_storage_unlocked(path: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return Ok(Vec::new());
    };
    let mut latest_by_item = HashMap::<String, (u64, AcpUiEvent)>::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(patch) = serde_json::from_str::<AcpTimelinePatch>(&line) {
            if patch.patch_type != "timelinePatch" || patch.op != "upsert" {
                continue;
            }
            let should_replace = latest_by_item
                .get(&patch.item_id)
                .map(|(revision, _)| patch.revision >= *revision)
                .unwrap_or(true);
            if should_replace {
                let item = latest_by_item
                    .get(&patch.item_id)
                    .map(|(_, existing)| merge_timeline_item_revision(existing, patch.item.clone()))
                    .unwrap_or(patch.item);
                latest_by_item.insert(patch.item_id, (patch.revision, item));
            }
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<AcpTimelineItem>(&line) {
            let item_id = entry.item.id.clone();
            let should_replace = latest_by_item
                .get(&item_id)
                .map(|(revision, _)| *revision == 0)
                .unwrap_or(true);
            if should_replace {
                let item = latest_by_item
                    .get(&item_id)
                    .map(|(_, existing)| merge_timeline_item_revision(existing, entry.item.clone()))
                    .unwrap_or(entry.item);
                latest_by_item.insert(item_id, (0, item));
            }
        }
    }
    let mut items = latest_by_item
        .into_values()
        .map(|(_, item)| item)
        .filter(|item| !is_provider_user_echo_event(item))
        .collect::<Vec<_>>();
    items.sort_by_key(|item| (item.started_seq.unwrap_or(item.seq), item.seq));
    remove_reclassified_local_provider_history(&mut items);
    Ok(items)
}

pub(crate) fn merge_timeline_item_revision(
    existing: &AcpUiEvent,
    mut incoming: AcpUiEvent,
) -> AcpUiEvent {
    if is_provider_history_event(&incoming) && !is_provider_history_event(existing) {
        return existing.clone();
    }
    let existing_start = existing.started_seq.unwrap_or(existing.seq);
    let incoming_start = incoming.started_seq.unwrap_or(incoming.seq);
    if existing_start > incoming_start {
        return incoming;
    }

    let repeated_payload = existing.kind == incoming.kind
        && existing.content == incoming.content
        && existing.title == incoming.title
        && existing.tool_call_id == incoming.tool_call_id
        && existing.status == incoming.status
        && raw_equal_ignoring_history_placement(existing.raw.as_ref(), incoming.raw.as_ref());
    incoming.started_seq = Some(existing_start);
    incoming.started_at = existing
        .started_at
        .clone()
        .or_else(|| Some(existing.timestamp.clone()));
    incoming.timestamp = existing.timestamp.clone();
    if repeated_payload {
        incoming.seq = existing.seq;
        incoming.ended_seq = existing.ended_seq;
        incoming.ended_at = existing.ended_at.clone();
        incoming.timing = existing.timing.clone();
    }
    incoming
}

fn raw_equal_ignoring_history_placement(
    existing: Option<&Value>,
    incoming: Option<&Value>,
) -> bool {
    fn without_placement(raw: Option<&Value>) -> Option<Value> {
        raw.map(|raw| {
            let mut raw = raw.clone();
            if let Some(object) = raw.as_object_mut() {
                object.remove("historyPlacement");
            }
            raw
        })
    }

    without_placement(existing) == without_placement(incoming)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProviderHistoryTurnKey {
    session_id: Option<String>,
    provider: String,
    turn_index: u64,
}

fn remove_reclassified_local_provider_history(items: &mut Vec<AcpUiEvent>) {
    let mut local_prompts = HashMap::<Option<String>, Vec<String>>::new();
    for item in items.iter().filter(|item| is_gold_band_prompt_event(item)) {
        let Some(content) = item.content.as_deref() else {
            continue;
        };
        local_prompts
            .entry(item.session_id.clone())
            .or_default()
            .push(normalize_history_prompt(content));
    }

    let mut cursors = HashMap::<(Option<String>, String), usize>::new();
    let mut stale_turns = HashSet::<ProviderHistoryTurnKey>::new();
    for item in items.iter().filter(|item| {
        item.kind == "userTextDelta"
            && is_provider_history_event(item)
            && !has_provider_history_placement(item.raw.as_ref())
    }) {
        let Some(turn) = provider_history_turn_key(item) else {
            continue;
        };
        let Some(content) = item.content.as_deref() else {
            continue;
        };
        let Some(anchors) = local_prompts.get(&turn.session_id) else {
            continue;
        };
        let cursor_key = (turn.session_id.clone(), turn.provider.clone());
        let cursor = cursors.entry(cursor_key).or_default();
        let normalized = normalize_history_prompt(content);
        let Some(relative_index) = anchors[*cursor..]
            .iter()
            .position(|anchor| anchor == &normalized)
        else {
            continue;
        };
        *cursor = cursor.saturating_add(relative_index).saturating_add(1);
        stale_turns.insert(turn);
    }

    if stale_turns.is_empty() {
        return;
    }
    items.retain(|item| {
        provider_history_turn_key(item).is_none_or(|turn| !stale_turns.contains(&turn))
    });
}

fn has_provider_history_placement(raw: Option<&Value>) -> bool {
    raw.and_then(|raw| raw.get("historyPlacement"))
        .and_then(Value::as_object)
        .and_then(|placement| placement.get("version"))
        .and_then(Value::as_u64)
        == Some(1)
}

fn is_gold_band_prompt_event(event: &AcpUiEvent) -> bool {
    event.kind == "userTextDelta"
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            == Some("goldBandPrompt")
}

fn is_provider_history_event(event: &AcpUiEvent) -> bool {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("source"))
        .and_then(Value::as_str)
        == Some("providerHistory")
}

fn provider_history_turn_key(event: &AcpUiEvent) -> Option<ProviderHistoryTurnKey> {
    let raw = event.raw.as_ref()?;
    if raw.get("source").and_then(Value::as_str) != Some("providerHistory") {
        return None;
    }
    Some(ProviderHistoryTurnKey {
        session_id: event.session_id.clone(),
        provider: raw
            .get("historyProvider")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        turn_index: raw.get("historyTurnIndex").and_then(Value::as_u64)?,
    })
}

fn normalize_history_prompt(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn is_provider_user_echo_event(event: &AcpUiEvent) -> bool {
    event.kind == "userTextDelta"
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            != Some("providerHistory")
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.get("sessionUpdate"))
            .and_then(Value::as_str)
            == Some("user_message_chunk")
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
    with_jsonl_file_lock(path, || latest_timeline_source_seq_unlocked(path)).unwrap_or(0)
}

fn latest_timeline_source_seq_unlocked(path: &Utf8Path) -> Result<u64> {
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return Ok(0);
    };
    let mut latest = 0;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(patch) = serde_json::from_str::<AcpTimelinePatch>(&line) {
            latest = latest.max(patch.revision).max(
                patch
                    .item
                    .ended_seq
                    .or(patch.item.started_seq)
                    .unwrap_or(patch.item.seq),
            );
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<AcpTimelineItem>(&line) {
            latest = latest.max(
                entry
                    .item
                    .ended_seq
                    .or(entry.item.started_seq)
                    .unwrap_or(entry.item.seq),
            );
        }
    }
    Ok(latest)
}

pub fn write_session_metadata(path: &Utf8Path, metadata: &AcpSessionMetadata) -> Result<()> {
    write_json(path, metadata)
}

/// Loads ACP metadata and performs the development-stage, one-time migration
/// away from the ambiguous legacy `status` field. The migrated representation
/// is written back immediately so every later reader sees the split schema.
pub fn load_session_metadata(
    path: &Utf8Path,
    established_session_id: Option<String>,
) -> Result<AcpSessionMetadata> {
    Ok(serde_json::from_value(load_session_metadata_value(
        path,
        established_session_id,
    )?)?)
}

/// Value-level metadata loader used by lightweight control/read-model paths.
/// Migration happens before typed deserialization so old minimal metadata
/// shells can be upgraded without inventing unrelated adapter fields.
pub fn load_session_metadata_value(
    path: &Utf8Path,
    established_session_id: Option<String>,
) -> Result<Value> {
    let mut value: Value = read_json(path)?;
    let Some(legacy_status) = value
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(value);
    };
    let normalized = legacy_status.trim().to_ascii_lowercase().replace('_', "-");
    if value.get("sessionId").is_none()
        && let Some(session_id) = established_session_id
    {
        value["sessionId"] = Value::String(session_id);
    }
    value["availability"] = Value::String(
        match normalized.as_str() {
            "closing" | "cancelling" | "cancel-requested" => "closing",
            "failed" | "failure" | "error" | "killed" => "restorable",
            _ => "established",
        }
        .to_string(),
    );
    value["latestTurnStatus"] = Value::String(
        match normalized.as_str() {
            "completed" | "complete" => "completed",
            "cancelled" | "canceled" => "cancelled",
            "failed" | "failure" | "error" | "killed" => "failed",
            _ => "none",
        }
        .to_string(),
    );
    if let Some(object) = value.as_object_mut() {
        object.remove("status");
    }
    write_json(path, &value)?;
    Ok(value)
}

pub fn normalize_session_update(
    seq: u64,
    session_id: Option<String>,
    update: &Value,
) -> AcpUiEvent {
    let timestamp = current_timestamp();
    let provider_kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let compaction_phase = context_compaction_phase(update);
    let mut raw_value = update.clone();
    normalize_agent_transcript_metadata(&mut raw_value);
    if let Some(phase) = compaction_phase
        && let Some(object) = raw_value.as_object_mut()
    {
        object.insert(
            "contextCompaction".to_string(),
            serde_json::json!({
                "phase": phase,
                "detectionSource": "providerControlMessage",
            }),
        );
    }
    let raw = Some(raw_value);
    let id = format!("acp-event-{seq}");
    let mut event = AcpUiEvent {
        id,
        seq,
        timestamp,
        kind: compaction_phase
            .map(|_| "contextCompaction")
            .unwrap_or_else(|| kind_to_ui_kind(provider_kind))
            .to_string(),
        session_id,
        content: compaction_phase
            .is_none()
            .then(|| extract_text(update))
            .flatten(),
        title: extract_title(update),
        tool_call_id: extract_tool_call_id(update),
        status: compaction_phase
            .map(|phase| match phase {
                "started" => "running".to_string(),
                _ => "completed".to_string(),
            })
            .or_else(|| extract_status(update)),
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
        timing: None,
        raw,
    };

    if event.content.is_none()
        && matches!(
            provider_kind,
            "agent_message_chunk" | "user_message_chunk" | "agent_thought_chunk"
        )
        && compaction_phase.is_none()
    {
        event.content = Some(String::new());
    }

    event
}

/// Claude-compatible ACP adapters currently expose context compaction as two
/// standalone agent control messages. Normalize them at the provider boundary
/// so consumers do not need to interpret assistant prose.
pub fn context_compaction_phase(update: &Value) -> Option<&'static str> {
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("agent_message_chunk") {
        return None;
    }
    match extract_text(update)?.trim() {
        "Compacting..." => Some("started"),
        "Compacting completed." => Some("completed"),
        _ => None,
    }
}

pub fn permission_request_event(seq: u64, request_id: String, params: Value) -> AcpUiEvent {
    let mut raw = params;
    normalize_agent_transcript_metadata(&mut raw);
    if let Some(object) = raw.as_object_mut() {
        object
            .entry("requestId".to_string())
            .or_insert_with(|| Value::String(request_id.clone()));
    }
    AcpUiEvent {
        id: request_id,
        seq,
        timestamp: current_timestamp(),
        kind: "permissionRequest".to_string(),
        session_id: raw
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
        content: None,
        title: extract_title(&raw).or_else(|| Some("Permission required".to_string())),
        tool_call_id: extract_tool_call_id(&raw),
        status: Some("pending".to_string()),
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
        timing: None,
        raw: Some(raw),
    }
}

pub fn normalize_agent_transcript_metadata(value: &mut Value) -> Option<AgentTranscriptRelation> {
    let relation = extract_agent_transcript_relation(value);
    let tool_output = agent_transcript_tool_output(value).cloned();
    if relation.is_none() && tool_output.is_none() {
        return None;
    }
    let object = value.as_object_mut()?;
    let meta = object
        .entry("_meta".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !meta.is_object() {
        *meta = Value::Object(serde_json::Map::new());
    }
    let mut normalized = relation
        .as_ref()
        .map(|relation| {
            serde_json::to_value(relation).expect("AgentTranscriptRelation must serialize")
        })
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(tool_output) = tool_output {
        ensure_object(&mut normalized).insert("toolOutput".to_string(), tool_output);
    }
    meta.as_object_mut()?
        .insert(AGENT_TRANSCRIPT_META_KEY.to_string(), normalized);
    relation
}

pub fn agent_transcript_tool_output(raw: &Value) -> Option<&Value> {
    raw.pointer(&format!("/_meta/{AGENT_TRANSCRIPT_META_KEY}/toolOutput"))
        .or_else(|| {
            raw.pointer(&format!(
                "/toolCall/_meta/{AGENT_TRANSCRIPT_META_KEY}/toolOutput"
            ))
        })
        .or_else(|| {
            raw.pointer(&format!(
                "/_meta/{CLAUDE_CODE_META_KEY}/toolResponse/content"
            ))
        })
        .or_else(|| {
            raw.pointer(&format!(
                "/toolCall/_meta/{CLAUDE_CODE_META_KEY}/toolResponse/content"
            ))
        })
}

/// Remove provider-only Agent metadata and heavyweight tool output before a
/// canonical live event crosses the desktop IPC boundary.
///
/// The complete event remains available in the branch timeline and can be
/// queried through the single-tool detail API. Live consumers only need the
/// stable Gold Band relation metadata, tool input, status, and summary fields.
pub fn compact_live_conversation_event(event: &mut AcpUiEvent) {
    let Some(raw) = event.raw.as_mut() else {
        return;
    };
    remove_provider_agent_metadata(raw);
    if !matches!(event.kind.as_str(), "toolCall" | "toolCallUpdate") {
        return;
    }
    for path in [
        &["output"][..],
        &["fields", "output"][..],
        &["content", "output"][..],
        &["toolCall", "output"][..],
        &["toolCall", "content"][..],
        &["toolCall", "fields", "output"][..],
        &["_meta", AGENT_TRANSCRIPT_META_KEY, "toolOutput"][..],
        &["_meta", "goldBandConversation", "toolOutput"][..],
    ] {
        remove_nested_json_key(raw, path);
    }
    if raw
        .get("content")
        .is_some_and(|content| !content.is_object())
        && let Some(object) = raw.as_object_mut()
    {
        object.remove("content");
    }
    let raw_object = ensure_object(raw);
    let meta = raw_object
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}));
    let meta_object = ensure_object(meta);
    let conversation = meta_object
        .entry("goldBandConversation")
        .or_insert_with(|| serde_json::json!({}));
    ensure_object(conversation).insert("toolDetailAvailable".to_string(), Value::Bool(true));
}

fn remove_provider_agent_metadata(raw: &mut Value) {
    for path in [
        &["_meta", CLAUDE_CODE_META_KEY][..],
        &["_meta", AGENT_TRANSCRIPT_META_KEY][..],
        &["toolCall", "_meta", CLAUDE_CODE_META_KEY][..],
        &["toolCall", "_meta", AGENT_TRANSCRIPT_META_KEY][..],
    ] {
        remove_nested_json_key(raw, path);
    }
}

fn remove_nested_json_key(value: &mut Value, path: &[&str]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = value;
    for key in parents {
        let Some(next) = current.get_mut(*key) else {
            return;
        };
        current = next;
    }
    if let Some(object) = current.as_object_mut() {
        object.remove(*last);
    }
}

fn ensure_object(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !value.is_object() {
        *value = serde_json::json!({});
    }
    value
        .as_object_mut()
        .expect("normalized conversation metadata must be an object")
}

pub fn extract_agent_transcript_relation(value: &Value) -> Option<AgentTranscriptRelation> {
    let standard = value
        .pointer(&format!("/_meta/{AGENT_TRANSCRIPT_META_KEY}"))
        .or_else(|| value.pointer(&format!("/toolCall/_meta/{AGENT_TRANSCRIPT_META_KEY}")));
    let claude = value
        .pointer(&format!("/_meta/{CLAUDE_CODE_META_KEY}"))
        .or_else(|| value.pointer(&format!("/toolCall/_meta/{CLAUDE_CODE_META_KEY}")));

    let standard_launch = standard
        .and_then(|meta| meta.get("agentLaunch"))
        .and_then(Value::as_bool);
    let standard_parent = standard
        .and_then(|meta| meta.get("parentToolCallId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let standard_tool_name = standard
        .and_then(|meta| meta.get("toolName"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let claude_subagent = claude
        .and_then(|meta| meta.get("subagent"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let claude_tool_name = claude
        .and_then(|meta| meta.get("toolName"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let claude_agent_tool = claude_tool_name.as_deref().is_some_and(|name| {
        CLAUDE_AGENT_TOOL_NAMES.contains(&name.trim().to_ascii_lowercase().as_str())
    });
    let claude_parent = claude
        .and_then(|meta| meta.get("parentToolUseId"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let relation = AgentTranscriptRelation {
        agent_launch: standard_launch.unwrap_or(claude_subagent || claude_agent_tool),
        tool_name: standard_tool_name.or(claude_tool_name),
        parent_tool_call_id: standard_parent.or(claude_parent),
    };
    (!relation.is_empty()).then_some(relation)
}

pub fn elicitation_request_event(
    seq: u64,
    elicitation_id: String,
    request: &CreateElicitationRequest,
) -> AcpUiEvent {
    let (session_id, tool_call_id) = match request.scope() {
        ElicitationScope::Session(scope) => (
            Some(scope.session_id.to_string()),
            scope.tool_call_id.as_ref().map(ToString::to_string),
        ),
        ElicitationScope::Request(_) => (None, None),
        _ => (None, None),
    };
    AcpUiEvent {
        id: elicitation_id,
        seq,
        timestamp: current_timestamp(),
        kind: "elicitationRequest".to_string(),
        session_id,
        content: Some(request.message.clone()),
        title: None,
        tool_call_id,
        status: Some("pending".to_string()),
        // 不设 ended_seq/ended_at — 保持"进行中"直到用户响应
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
        timing: None,
        raw: Some(
            serde_json::to_value(request).expect("CreateElicitationRequest must serialize to JSON"),
        ),
    }
}

pub fn elicitation_response_event(
    seq: u64,
    elicitation_id: String,
    action: String,
    content: Option<Value>,
) -> AcpUiEvent {
    AcpUiEvent {
        id: format!("{}-response", elicitation_id),
        seq,
        timestamp: current_timestamp(),
        kind: "elicitationResponse".to_string(),
        session_id: None,
        content: content.map(|v| v.to_string()),
        title: None,
        tool_call_id: None,
        status: Some("completed".to_string()),
        started_seq: None,
        ended_seq: None,
        started_at: None,
        ended_at: None,
        timing: None,
        raw: Some(serde_json::json!({
            "elicitationId": elicitation_id,
            "action": action,
        })),
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
        timing: None,
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
    user_prompt_event_with_quotes(
        seq,
        session_id,
        content,
        prompt_id,
        hidden_from_chat,
        attachments,
        Vec::new(),
    )
}

pub fn user_prompt_event_with_quotes(
    seq: u64,
    session_id: String,
    content: String,
    prompt_id: Option<String>,
    hidden_from_chat: bool,
    attachments: Vec<AttachmentMeta>,
    quotes: Vec<UserPromptQuote>,
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
    if !quotes.is_empty() {
        raw["quotes"] = serde_json::to_value(&quotes).unwrap_or_default();
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
        timing: None,
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
    use super::{
        AcpLatestTurnStatus, AcpSessionAvailability, AcpSessionMetadata, AcpTimingState,
        AcpUiEvent, agent_transcript_tool_output, annotate_latest_runtime_control_output,
        append_raw_frame, append_structured_diagnostic, append_timeline_patch,
        cancel_latest_processing_prompt_retry, compact_live_conversation_event,
        context_compaction_phase, elicitation_request_event, elicitation_response_event,
        extract_usage_fields, kind_to_ui_kind, latest_timeline_source_seq, load_session_metadata,
        load_timeline_items, normalize_session_update, permission_request_event, user_prompt_event,
        user_prompt_event_with_quotes, write_timeline_items,
    };
    use crate::provider::UserPromptQuote;
    use crate::storage::{read_json, write_json};
    use camino::Utf8PathBuf;
    use serde_json::{Value, json};

    #[test]
    fn live_tool_event_keeps_input_but_defers_output_and_provider_metadata() {
        let mut event = test_timeline_event("tool-1", 1, "");
        event.kind = "toolCall".to_string();
        event.raw = Some(json!({
            "rawInput": { "path": "src/main.rs" },
            "output": "large output",
            "_meta": {
                "goldBandConversation": {
                    "branchId": "agent-internal",
                    "toolName": "Read",
                    "toolOutput": "large normalized output"
                },
                "claudeCode": {
                    "subagent": true,
                    "toolResponse": { "content": "large provider output" }
                },
                "agentTranscript": { "parentToolCallId": "provider-parent" }
            }
        }));

        compact_live_conversation_event(&mut event);

        let raw = event.raw.as_ref().unwrap();
        assert_eq!(
            raw.pointer("/rawInput/path").and_then(Value::as_str),
            Some("src/main.rs")
        );
        assert_eq!(
            raw.pointer("/_meta/goldBandConversation/branchId")
                .and_then(Value::as_str),
            Some("agent-internal")
        );
        assert_eq!(
            raw.pointer("/_meta/goldBandConversation/toolDetailAvailable"),
            Some(&Value::Bool(true))
        );
        assert!(raw.get("output").is_none());
        assert!(
            raw.pointer("/_meta/goldBandConversation/toolOutput")
                .is_none()
        );
        assert!(raw.pointer("/_meta/claudeCode").is_none());
        assert!(raw.pointer("/_meta/agentTranscript").is_none());
    }

    #[test]
    fn structured_diagnostic_persists_stable_code_and_params() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = camino::Utf8PathBuf::from_path_buf(temp.path().join("acp.diagnostics.jsonl"))
            .expect("utf8 path");

        append_structured_diagnostic(
            &path,
            "warning",
            "acp.mcp-transport-unsupported",
            Some(json!({
                "agentType": "codex-acp",
                "skippedServers": [{
                    "name": "legacy",
                    "transport": "sse",
                    "capability": "mcpCapabilities.sse"
                }]
            })),
        )
        .expect("append diagnostic");

        let line = std::fs::read_to_string(path.as_std_path()).expect("read diagnostic");
        let value: Value = serde_json::from_str(line.trim()).expect("parse diagnostic");
        assert_eq!(value["level"], "warning");
        assert_eq!(value["code"], "acp.mcp-transport-unsupported");
        assert_eq!(value["message"], "acp.mcp-transport-unsupported");
        assert_eq!(value["data"]["skippedServers"][0]["transport"], "sse");
    }

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

    #[test]
    fn normalizes_context_compaction_control_messages_as_typed_events() {
        let started_update = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Compacting..."},
        });
        let completed_update = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "\n\nCompacting completed."},
        });

        assert_eq!(context_compaction_phase(&started_update), Some("started"));
        assert_eq!(
            context_compaction_phase(&completed_update),
            Some("completed")
        );

        let started = normalize_session_update(7, Some("session-1".to_string()), &started_update);
        assert_eq!(started.kind, "contextCompaction");
        assert_eq!(started.status.as_deref(), Some("running"));
        assert_eq!(started.content, None);
        assert_eq!(
            started
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/contextCompaction/phase"))
                .and_then(Value::as_str),
            Some("started")
        );

        let completed =
            normalize_session_update(8, Some("session-1".to_string()), &completed_update);
        assert_eq!(completed.kind, "contextCompaction");
        assert_eq!(completed.status.as_deref(), Some("completed"));
        assert_eq!(completed.content, None);
    }

    #[test]
    fn does_not_reclassify_regular_agent_text_that_mentions_compacting() {
        let update = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "The log says Compacting... but work continues."},
        });

        assert_eq!(context_compaction_phase(&update), None);
        let event = normalize_session_update(9, None, &update);
        assert_eq!(event.kind, "textDelta");
        assert_eq!(
            event.content.as_deref(),
            Some("The log says Compacting... but work continues.")
        );
    }

    #[test]
    fn normalizes_claude_agent_launch_into_internal_transcript_metadata() {
        let update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-agent",
            "_meta": {
                "claudeCode": {
                    "toolName": "Agent",
                    "subagent": true
                }
            }
        });

        let event = normalize_session_update(10, Some("session-1".to_string()), &update);

        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/agentTranscript/agentLaunch"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/agentTranscript/toolName"))
                .and_then(Value::as_str),
            Some("Agent")
        );
    }

    #[test]
    fn normalizes_claude_tool_response_into_internal_transcript_metadata() {
        let update = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-read",
            "_meta": {
                "claudeCode": {
                    "toolName": "Read",
                    "toolResponse": { "content": "normalized output" }
                }
            }
        });

        let event = normalize_session_update(10, Some("session-1".to_string()), &update);
        let raw = event.raw.as_ref().expect("normalized raw event");
        assert_eq!(
            raw.pointer("/_meta/agentTranscript/toolOutput"),
            Some(&json!("normalized output"))
        );
        assert_eq!(
            agent_transcript_tool_output(raw),
            Some(&json!("normalized output"))
        );
    }

    #[test]
    fn normalizes_claude_tool_name_for_a_parented_event_without_reclassifying_it_as_an_agent_launch()
     {
        let update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-read",
            "_meta": {
                "claudeCode": {
                    "toolName": "Read",
                    "parentToolUseId": "call-agent"
                }
            }
        });

        let event = normalize_session_update(10, Some("session-1".to_string()), &update);
        let transcript = event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/agentTranscript"))
            .expect("normalized transcript metadata");
        assert!(transcript.get("agentLaunch").is_none());
        assert_eq!(transcript["toolName"], "Read");
        assert_eq!(transcript["parentToolCallId"], "call-agent");
    }

    #[test]
    fn normalizes_claude_parent_tool_use_id_for_nested_events() {
        let update = json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": "Inspecting"},
            "_meta": {
                "claudeCode": {
                    "parentToolUseId": "call-parent"
                }
            }
        });

        let event = normalize_session_update(11, Some("session-1".to_string()), &update);

        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/agentTranscript/parentToolCallId"))
                .and_then(Value::as_str),
            Some("call-parent")
        );
    }

    #[test]
    fn normalizes_nested_claude_agent_launch_with_both_relationship_fields() {
        let update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-child",
            "_meta": {
                "claudeCode": {
                    "toolName": "Agent",
                    "subagent": true,
                    "parentToolUseId": "call-parent"
                }
            }
        });

        let event = normalize_session_update(12, Some("session-1".to_string()), &update);
        let transcript = event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/agentTranscript"))
            .expect("normalized transcript metadata");

        assert_eq!(transcript["agentLaunch"], true);
        assert_eq!(transcript["parentToolCallId"], "call-parent");
    }

    #[test]
    fn preserves_standard_agent_transcript_metadata_over_provider_extensions() {
        let update = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-child",
            "_meta": {
                "agentTranscript": {
                    "agentLaunch": false,
                    "parentToolCallId": "standard-parent"
                },
                "claudeCode": {
                    "toolName": "Agent",
                    "subagent": true,
                    "parentToolUseId": "claude-parent"
                }
            }
        });

        let event = normalize_session_update(13, Some("session-1".to_string()), &update);
        let transcript = event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/agentTranscript"))
            .expect("normalized transcript metadata");

        assert!(transcript.get("agentLaunch").is_none());
        assert_eq!(transcript["parentToolCallId"], "standard-parent");
    }

    #[test]
    fn normalizes_nested_permission_tool_metadata_at_the_same_boundary() {
        let event = permission_request_event(
            14,
            "permission-1".to_string(),
            json!({
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": "call-bash",
                    "_meta": {
                        "claudeCode": {
                            "toolName": "Bash",
                            "parentToolUseId": "call-agent"
                        }
                    }
                }
            }),
        );

        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/agentTranscript/parentToolCallId"))
                .and_then(Value::as_str),
            Some("call-agent")
        );
    }

    #[test]
    fn normalizes_top_level_claude_tool_name_without_marking_an_agent_launch() {
        let update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-read",
            "_meta": {
                "claudeCode": {
                    "toolName": "Read"
                }
            }
        });

        let event = normalize_session_update(15, Some("session-1".to_string()), &update);
        let transcript = event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/agentTranscript"))
            .expect("normalized transcript metadata");
        assert_eq!(transcript["toolName"], "Read");
        assert!(transcript.get("agentLaunch").is_none());
        assert!(transcript.get("parentToolCallId").is_none());
    }

    // --- existing tests ---

    #[test]
    fn session_metadata_system_prompt_append_is_optional() {
        let metadata: AcpSessionMetadata = serde_json::from_value(json!({
            "adapterId": "npx",
            "adapterDisplayName": "Claude",
            "cwd": "C:/tmp/attempt",
            "status": "running",
            "restored": false,
            "stopReason": null,
            "capabilities": {},
            "createdAt": "1778771541Z",
            "updatedAt": "1778771542Z"
        }))
        .unwrap();

        assert!(metadata.system_prompt_append.is_none());
    }

    #[test]
    fn session_metadata_serializes_system_prompt_append() {
        let metadata: AcpSessionMetadata = serde_json::from_value(json!({
            "adapterId": "npx",
            "adapterDisplayName": "Claude",
            "cwd": "C:/tmp/attempt",
            "status": "running",
            "restored": false,
            "stopReason": null,
            "capabilities": {},
            "systemPromptAppend": "You are Gold Band.",
            "createdAt": "1778771541Z",
            "updatedAt": "1778771542Z"
        }))
        .unwrap();

        let value = serde_json::to_value(metadata).unwrap();
        assert_eq!(
            value
                .get("systemPromptAppend")
                .and_then(|value| value.as_str()),
            Some("You are Gold Band.")
        );
    }

    #[test]
    fn legacy_session_status_migrates_once_to_split_lifecycle_facets() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.snapshot.json")).unwrap();
        write_json(
            &path,
            &json!({
                "adapterId": "codex-acp",
                "adapterDisplayName": "Codex",
                "cwd": "C:/tmp/attempt",
                "status": "completed",
                "restored": true,
                "stopReason": "end_turn",
                "capabilities": {},
                "createdAt": "1785232749Z",
                "updatedAt": "1785233025Z"
            }),
        )
        .unwrap();

        let metadata = load_session_metadata(&path, Some("session-123".to_string())).unwrap();
        assert_eq!(metadata.session_id.as_deref(), Some("session-123"));
        assert_eq!(metadata.availability, AcpSessionAvailability::Established);
        assert_eq!(metadata.latest_turn_status, AcpLatestTurnStatus::Completed);

        let persisted: Value = read_json(&path).unwrap();
        assert!(persisted.get("status").is_none());
        assert_eq!(persisted["availability"], "established");
        assert_eq!(persisted["latestTurnStatus"], "completed");
    }

    #[test]
    fn session_metadata_serializes_cumulative_attempt_token_totals_separately() {
        let metadata: AcpSessionMetadata = serde_json::from_value(json!({
            "adapterId": "codex-acp",
            "adapterDisplayName": "Codex",
            "cwd": "C:/tmp/attempt",
            "status": "completed",
            "restored": true,
            "stopReason": "end_turn",
            "capabilities": {},
            "inputTokens": 7453,
            "outputTokens": 315,
            "cachedReadTokens": 16896,
            "totalTokens": 24664,
            "attemptInputTokens": 16510,
            "attemptOutputTokens": 330,
            "attemptCachedReadTokens": 24576,
            "attemptTotalTokens": 41416,
            "createdAt": "1785232749Z",
            "updatedAt": "1785233025Z"
        }))
        .unwrap();

        assert_eq!(metadata.input_tokens, Some(7453));
        assert_eq!(metadata.attempt_input_tokens, Some(16510));
        assert_eq!(metadata.attempt_total_tokens, Some(41416));

        let value = serde_json::to_value(metadata).unwrap();
        assert_eq!(
            value.get("attemptCachedReadTokens").and_then(Value::as_u64),
            Some(24576)
        );
    }

    #[test]
    fn permission_request_event_preserves_original_request_id_in_raw() {
        let event = permission_request_event(
            9,
            "0".to_string(),
            json!({
                "sessionId": "session-123",
                "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }]
            }),
        );

        assert_eq!(event.id, "0");
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("requestId"))
                .and_then(|value| value.as_str()),
            Some("0")
        );
    }

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
    fn user_prompt_event_persists_explicit_quotes_without_changing_display_content() {
        let event = user_prompt_event_with_quotes(
            7,
            "session-123".to_string(),
            "> 用户自己输入的正文".to_string(),
            Some("prompt-123".to_string()),
            false,
            Vec::new(),
            vec![UserPromptQuote {
                id: "quote-1".to_string(),
                source_message_key: "message-1".to_string(),
                text: "Agent 原文".to_string(),
            }],
        );

        assert_eq!(event.content.as_deref(), Some("> 用户自己输入的正文"));
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/quotes/0/text"))
                .and_then(Value::as_str),
            Some("Agent 原文")
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

    fn test_timeline_event(id: &str, seq: u64, content: &str) -> AcpUiEvent {
        AcpUiEvent {
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            kind: "textDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some(content.to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: Some(seq),
            ended_seq: Some(seq),
            started_at: Some(format!("{seq}Z")),
            ended_at: Some(format!("{seq}Z")),
            timing: None,
            raw: None,
        }
    }

    fn prompt_event_at(seq: u64, timestamp: u64) -> AcpUiEvent {
        let mut event = user_prompt_event(
            seq,
            "session-1".to_string(),
            "继续".to_string(),
            Some(format!("prompt-{seq}")),
            false,
            Vec::new(),
        );
        event.timestamp = format!("{timestamp}Z");
        event
    }

    fn session_update_event_at(seq: u64, session_update: &str, timestamp: u64) -> AcpUiEvent {
        let mut event = test_timeline_event(&format!("event-{seq}"), seq, "delta");
        event.timestamp = format!("{timestamp}Z");
        event.raw = Some(json!({ "sessionUpdate": session_update }));
        event
    }

    fn permission_event_at(seq: u64, request_id: &str, status: &str, timestamp: u64) -> AcpUiEvent {
        let mut event = permission_request_event(
            seq,
            request_id.to_string(),
            json!({ "requestId": request_id }),
        );
        event.timestamp = format!("{timestamp}Z");
        event.status = Some(status.to_string());
        event
    }

    fn elicitation_request_event_at(seq: u64, elicitation_id: &str, timestamp: u64) -> AcpUiEvent {
        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-test",
            "message": "Choose",
            "requestedSchema": { "type": "object", "properties": {} }
        }))
        .unwrap();
        let mut event = elicitation_request_event(seq, elicitation_id.to_string(), &request);
        event.timestamp = format!("{timestamp}Z");
        event
    }

    fn elicitation_response_event_at(seq: u64, elicitation_id: &str, timestamp: u64) -> AcpUiEvent {
        let mut event = elicitation_response_event(
            seq,
            elicitation_id.to_string(),
            "accept".to_string(),
            Some(json!({})),
        );
        event.timestamp = format!("{timestamp}Z");
        event
    }

    #[test]
    fn elicitation_request_event_preserves_the_full_typed_request() {
        let request = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "session-1",
            "toolCallId": "tool-1",
            "message": "Context\n\nComplete question?",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "oneOf": [{
                            "const": "A",
                            "title": "A",
                            "description": "First choice",
                            "_meta": {
                                "_claude/askUserQuestionOption": {
                                    "preview": "preview"
                                }
                            }
                        }]
                    }
                }
            },
            "_meta": { "source": "claude-agent-acp" }
        }))
        .unwrap();

        let event = elicitation_request_event(7, "elicit-1".to_string(), &request);
        let raw = event.raw.unwrap();

        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(
            event.content.as_deref(),
            Some("Context\n\nComplete question?")
        );
        assert_eq!(raw["mode"], "form");
        assert_eq!(raw["_meta"]["source"], "claude-agent-acp");
        assert_eq!(
            raw["requestedSchema"]["properties"]["question_0"]["oneOf"][0]["description"],
            "First choice"
        );
        assert_eq!(
            raw["requestedSchema"]["properties"]["question_0"]["oneOf"][0]["_meta"]["_claude/askUserQuestionOption"]
                ["preview"],
            "preview"
        );
    }

    #[test]
    fn acp_timing_patch_uses_last_activity_anchor() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&session_update_event_at(2, "agent_message_chunk", 112));

        let patch = state.patch_at(112, "active").unwrap();

        assert_eq!(patch.session_elapsed_seconds, 12);
        assert_eq!(patch.revision, Some(2));
        assert_eq!(patch.observed_at.as_deref(), Some("112Z"));
        assert_eq!(patch.active_turn_started_at.as_deref(), Some("100Z"));
        assert_eq!(patch.active_turn_last_activity_at.as_deref(), Some("112Z"));
        assert!(!patch.paused);
    }

    #[test]
    fn acp_timing_metadata_update_does_not_advance_elapsed() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&session_update_event_at(2, "agent_message_chunk", 105));
        state.observe_event(&session_update_event_at(3, "current_mode_update", 500));

        let snapshot = state.snapshot_at(false, None).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 5);
    }

    #[test]
    fn acp_timing_terminal_snapshot_counts_prompt_with_only_live_ticks() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&session_update_event_at(2, "agent_message_chunk", 110));
        state.observe_event(&prompt_event_at(3, 139));

        let snapshot = state.terminal_snapshot_at(Some(148)).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 19);
        assert_eq!(snapshot.revision, Some(3));
        assert_eq!(snapshot.observed_at.as_deref(), Some("148Z"));
        assert!(snapshot.paused);
        assert!(snapshot.active_turn_started_at.is_none());
        assert!(snapshot.active_turn_last_activity_at.is_none());
    }

    #[test]
    fn acp_timing_terminal_snapshot_accepts_synthetic_revision() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));

        let snapshot = state
            .terminal_snapshot_at_with_revision(Some(120), Some(99), Some("120Z".to_string()))
            .unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 20);
        assert_eq!(snapshot.revision, Some(99));
        assert_eq!(snapshot.observed_at.as_deref(), Some("120Z"));
    }

    #[test]
    fn acp_timing_terminal_snapshot_excludes_open_user_wait() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&permission_event_at(2, "permission-1", "pending", 105));

        let snapshot = state.terminal_snapshot_at(Some(150)).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 5);
        assert_eq!(snapshot.revision, Some(2));
        assert_eq!(snapshot.observed_at.as_deref(), Some("150Z"));
        assert!(snapshot.paused);
        assert!(snapshot.user_wait_started_at.is_none());
        assert!(snapshot.wait_reason.is_none());
    }

    #[test]
    fn acp_timing_permission_wait_pauses_and_is_excluded() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&session_update_event_at(2, "agent_message_chunk", 110));
        state.observe_event(&permission_event_at(3, "permission-1", "pending", 120));

        let waiting = state.patch_at(120, "permission-wait").unwrap();
        assert_eq!(waiting.session_elapsed_seconds, 20);
        assert!(waiting.paused);
        assert_eq!(waiting.wait_reason.as_deref(), Some("permission"));

        state.observe_event(&permission_event_at(4, "permission-1", "selected", 170));
        state.observe_event(&session_update_event_at(5, "agent_message_chunk", 180));
        let snapshot = state.snapshot_at(false, None).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 30);
    }

    #[test]
    fn acp_timing_reconstructs_compacted_permission_wait() {
        let mut selected = permission_event_at(3, "permission-1", "selected", 170);
        selected.started_at = Some("120Z".to_string());
        selected.ended_at = Some("170Z".to_string());

        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&session_update_event_at(2, "agent_message_chunk", 110));
        state.observe_event(&selected);
        state.observe_event(&session_update_event_at(4, "agent_message_chunk", 180));

        let snapshot = state.snapshot_at(false, None).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 30);
    }

    #[test]
    fn acp_timing_elicitation_wait_pauses_and_is_excluded() {
        let mut state = AcpTimingState::default();
        state.observe_event(&prompt_event_at(1, 100));
        state.observe_event(&session_update_event_at(2, "agent_message_chunk", 110));
        state.observe_event(&elicitation_request_event_at(3, "elicit-1", 120));

        let waiting = state.patch_at(150, "tick").unwrap();
        assert_eq!(waiting.session_elapsed_seconds, 20);
        assert!(waiting.paused);
        assert_eq!(waiting.wait_reason.as_deref(), Some("elicitation"));

        state.observe_event(&elicitation_response_event_at(4, "elicit-1", 170));
        state.observe_event(&session_update_event_at(5, "agent_message_chunk", 180));
        let snapshot = state.snapshot_at(false, None).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 30);
    }

    #[test]
    fn load_timeline_items_merges_snapshot_and_patch() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        write_timeline_items(
            &path,
            &[
                test_timeline_event("message-1", 10, "old"),
                test_timeline_event("message-2", 20, "keep"),
            ],
        )
        .unwrap();
        let mut updated = test_timeline_event("message-1", 30, "new");
        updated.started_seq = Some(10);
        updated.started_at = Some("10Z".to_string());
        append_timeline_patch(&path, "message-1", 1, &updated).unwrap();

        let items = load_timeline_items(&path).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "message-1");
        assert_eq!(items[0].content.as_deref(), Some("new"));
        assert_eq!(items[1].id, "message-2");
        assert_eq!(items[1].content.as_deref(), Some("keep"));
    }

    #[test]
    fn attempt_stop_settles_latest_processing_retry_prompt() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut prompt = user_prompt_event(
            7,
            "session-1".to_string(),
            "hi".to_string(),
            Some("prompt-1".to_string()),
            false,
            Vec::new(),
        );
        prompt.status = Some("processing".to_string());
        prompt.started_seq = Some(7);
        prompt.ended_seq = Some(12);
        prompt.started_at = Some("100Z".to_string());
        prompt.ended_at = Some("120Z".to_string());
        prompt.raw.as_mut().unwrap()["retry"] = json!({
            "attempt": 2,
            "maxAttempts": 3,
        });
        append_timeline_patch(&path, prompt.id.clone(), 12, &prompt).unwrap();

        assert!(cancel_latest_processing_prompt_retry(&path, "130Z".to_string()).unwrap());

        let items = load_timeline_items(&path).unwrap();
        let settled = items
            .iter()
            .find(|event| event.id == prompt.id)
            .expect("settled prompt");
        assert_eq!(settled.status.as_deref(), Some("cancelled"));
        assert_eq!(settled.started_seq, Some(7));
        assert_eq!(settled.ended_seq, Some(13));
        assert_eq!(settled.ended_at.as_deref(), Some("130Z"));
        assert_eq!(settled.raw.as_ref().unwrap()["retry"]["attempt"], 2);
        assert_eq!(settled.raw.as_ref().unwrap()["retry"]["maxAttempts"], 3);
        assert_eq!(settled.raw.as_ref().unwrap()["cancelled"], true);
        assert_eq!(latest_timeline_source_seq(&path), 13);

        assert!(!cancel_latest_processing_prompt_retry(&path, "140Z".to_string()).unwrap());
        assert_eq!(latest_timeline_source_seq(&path), 13);
    }

    #[test]
    fn attempt_stop_does_not_rewrite_non_retry_or_terminal_prompts() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut processing = user_prompt_event(
            3,
            "session-1".to_string(),
            "plain".to_string(),
            Some("prompt-plain".to_string()),
            false,
            Vec::new(),
        );
        processing.status = Some("processing".to_string());
        processing.ended_seq = Some(4);
        let mut completed_retry = user_prompt_event(
            5,
            "session-1".to_string(),
            "done".to_string(),
            Some("prompt-done".to_string()),
            false,
            Vec::new(),
        );
        completed_retry.ended_seq = Some(8);
        completed_retry.raw.as_mut().unwrap()["retry"] = json!({
            "attempt": 1,
            "maxAttempts": 3,
        });
        write_timeline_items(&path, &[processing.clone(), completed_retry.clone()]).unwrap();

        assert!(!cancel_latest_processing_prompt_retry(&path, "20Z".to_string()).unwrap());

        let items = load_timeline_items(&path).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, processing.id);
        assert_eq!(items[0].status, processing.status);
        assert_eq!(items[0].ended_seq, processing.ended_seq);
        assert_eq!(items[0].raw, processing.raw);
        assert_eq!(items[1].id, completed_retry.id);
        assert_eq!(items[1].status, completed_retry.status);
        assert_eq!(items[1].ended_seq, completed_retry.ended_seq);
        assert_eq!(items[1].raw, completed_retry.raw);
        assert_eq!(latest_timeline_source_seq(&path), 8);
    }

    #[test]
    fn load_timeline_items_keeps_original_position_for_replayed_message() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut original = test_timeline_event("assistant-message-1", 10, "hello");
        original.started_seq = Some(10);
        original.ended_seq = Some(12);
        original.started_at = Some("10Z".to_string());
        original.ended_at = Some("12Z".to_string());
        original.raw = Some(serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "1"
        }));
        append_timeline_patch(&path, original.id.clone(), 12, &original).unwrap();

        let mut replayed = original.clone();
        replayed.seq = 80;
        replayed.timestamp = "80Z".to_string();
        replayed.started_seq = Some(80);
        replayed.ended_seq = Some(80);
        replayed.started_at = Some("80Z".to_string());
        replayed.ended_at = Some("80Z".to_string());
        append_timeline_patch(&path, replayed.id.clone(), 80, &replayed).unwrap();

        let items = load_timeline_items(&path).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].seq, 10);
        assert_eq!(items[0].started_seq, Some(10));
        assert_eq!(items[0].ended_seq, Some(12));
        assert_eq!(items[0].timestamp, "10Z");
        assert_eq!(latest_timeline_source_seq(&path), 12);
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);
    }

    #[test]
    fn placement_only_history_patch_keeps_original_audit_position() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut original = test_timeline_event("provider-user-external", 10, "external");
        original.kind = "userTextDelta".to_string();
        original.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 2,
            "historyItemIndex": 1,
            "providerHistoryItemId": "provider-user-external",
            "sessionUpdate": "user_message_chunk"
        }));
        append_timeline_patch(&path, original.id.clone(), 10, &original).unwrap();

        let mut placed = original.clone();
        placed.seq = 80;
        placed.timestamp = "80Z".to_string();
        placed.started_seq = Some(80);
        placed.ended_seq = Some(80);
        placed.started_at = Some("80Z".to_string());
        placed.ended_at = Some("80Z".to_string());
        placed.raw.as_mut().unwrap()["historyPlacement"] = json!({
            "version": 1,
            "afterPromptId": "prompt-1",
            "beforePromptId": "prompt-2",
            "gapTurnIndex": 1
        });
        append_timeline_patch(&path, placed.id.clone(), 80, &placed).unwrap();

        let items = load_timeline_items(&path).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].seq, 10);
        assert_eq!(items[0].started_seq, Some(10));
        assert_eq!(items[0].ended_seq, Some(10));
        assert_eq!(items[0].timestamp, "10Z");
        assert_eq!(
            items[0].raw.as_ref().unwrap()["historyPlacement"]["beforePromptId"],
            json!("prompt-2")
        );
        assert_eq!(latest_timeline_source_seq(&path), 80);
    }

    #[test]
    fn load_timeline_items_hides_unclassified_echoes_and_keeps_external_history() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let local = test_timeline_event("gold-band-user-prompt-1", 1, "hello");
        let mut echoed = test_timeline_event("acp-event-2", 2, "hello");
        echoed.kind = "userTextDelta".to_string();
        echoed.raw = Some(serde_json::json!({
            "sessionUpdate": "user_message_chunk",
            "messageId": "echo-1"
        }));
        let mut interrupted = test_timeline_event(
            "acp-event-3",
            3,
            "[Request interrupted by user for tool use]",
        );
        interrupted.kind = "userTextDelta".to_string();
        interrupted.raw = Some(serde_json::json!({
            "sessionUpdate": "user_message_chunk",
            "messageId": "interrupt-1"
        }));
        let mut external = test_timeline_event("provider-user-external", 4, "external message");
        external.kind = "userTextDelta".to_string();
        external.raw = Some(serde_json::json!({
            "source": "providerHistory",
            "historyOrigin": "external",
            "sessionUpdate": "user_message_chunk",
            "messageId": "external-1"
        }));
        write_timeline_items(
            &path,
            &[local.clone(), echoed, interrupted, external.clone()],
        )
        .unwrap();

        let items = load_timeline_items(&path).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, local.id);
        assert_eq!(items[1].id, external.id);
    }

    #[test]
    fn load_timeline_items_repairs_reclassified_local_provider_turn() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut first_hi = test_timeline_event("gold-band-user-prompt-1", 1, "hi");
        first_hi.kind = "userTextDelta".to_string();
        first_hi.raw = Some(json!({ "source": "goldBandPrompt", "promptId": "prompt-1" }));
        let mut second_hi = test_timeline_event("gold-band-user-prompt-2", 2, "hi");
        second_hi.kind = "userTextDelta".to_string();
        second_hi.raw = Some(json!({ "source": "goldBandPrompt", "promptId": "prompt-2" }));
        let mut ask_prompt = test_timeline_event(
            "gold-band-user-prompt-3",
            3,
            "用askUserQuestion工具随便问几个问题给我",
        );
        ask_prompt.kind = "userTextDelta".to_string();
        ask_prompt.raw = Some(json!({ "source": "goldBandPrompt", "promptId": "prompt-3" }));
        let mut original_tool = test_timeline_event("tool-call-ask", 4, "");
        original_tool.kind = "toolCall".to_string();
        original_tool.title = Some("Asking for your input".to_string());
        original_tool.tool_call_id = Some("ask".to_string());
        original_tool.status = Some("completed".to_string());
        original_tool.raw = Some(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "ask",
            "rawInput": { "questions": [{ "question": "Question" }] }
        }));
        write_timeline_items(
            &path,
            &[first_hi, second_hi, ask_prompt, original_tool.clone()],
        )
        .unwrap();

        let mut external = test_timeline_event("provider-user-external", 10, "这是我追加的信息");
        external.kind = "userTextDelta".to_string();
        external.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 2,
            "sessionUpdate": "user_message_chunk"
        }));
        append_timeline_patch(&path, external.id.clone(), 10, &external).unwrap();

        let mut stale_ask = test_timeline_event(
            "provider-user-ask",
            11,
            "用askUserQuestion工具随便问几个问题给我",
        );
        stale_ask.kind = "userTextDelta".to_string();
        stale_ask.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 3,
            "sessionUpdate": "user_message_chunk"
        }));
        append_timeline_patch(&path, stale_ask.id.clone(), 11, &stale_ask).unwrap();

        let mut replayed_tool = original_tool.clone();
        replayed_tool.seq = 12;
        replayed_tool.started_seq = Some(12);
        replayed_tool.ended_seq = Some(12);
        replayed_tool.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 3,
            "sessionUpdate": "tool_call_update",
            "toolCallId": "ask",
            "rawOutput": "answered"
        }));
        append_timeline_patch(&path, replayed_tool.id.clone(), 12, &replayed_tool).unwrap();

        let mut stale_answer = test_timeline_event("provider-answer-ask", 13, "answered");
        stale_answer.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 3,
            "sessionUpdate": "agent_message_chunk"
        }));
        append_timeline_patch(&path, stale_answer.id.clone(), 13, &stale_answer).unwrap();

        let items = load_timeline_items(&path).unwrap();
        assert!(items.iter().any(|item| item.id == external.id));
        assert!(!items.iter().any(|item| item.id == stale_ask.id));
        assert!(!items.iter().any(|item| item.id == stale_answer.id));
        let tool = items
            .iter()
            .find(|item| item.id == original_tool.id)
            .unwrap();
        assert_eq!(tool.raw, original_tool.raw);
        assert_eq!(tool.seq, original_tool.seq);
    }

    #[test]
    fn structured_external_history_is_not_removed_when_text_matches_local_prompt() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut local = test_timeline_event("gold-band-user-prompt-1", 1, "same text");
        local.kind = "userTextDelta".to_string();
        local.raw = Some(json!({ "source": "goldBandPrompt", "promptId": "prompt-1" }));
        let mut external = test_timeline_event("provider-user-external", 2, "same text");
        external.kind = "userTextDelta".to_string();
        external.raw = Some(json!({
            "source": "providerHistory",
            "historyProvider": "claude-acp",
            "historyTurnIndex": 1,
            "historyItemIndex": 1,
            "historyPlacement": {
                "version": 1,
                "afterPromptId": "prompt-1",
                "beforePromptId": null,
                "gapTurnIndex": 1
            }
        }));
        write_timeline_items(&path, &[local, external.clone()]).unwrap();

        let items = load_timeline_items(&path).unwrap();
        assert!(items.iter().any(|item| item.id == external.id));
    }

    #[test]
    fn annotates_latest_runtime_control_text_delta() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        write_timeline_items(
            &path,
            &[
                test_timeline_event("message-1", 10, "earlier {\"old\":true}"),
                test_timeline_event("message-2", 20, "你好\n```json\n{\"a\":\"b\"}\n```"),
            ],
        )
        .unwrap();

        assert!(
            annotate_latest_runtime_control_output(
                &path,
                "dynamic-node-completion",
                "dynamic-node-completion",
            )
            .unwrap()
        );

        let items = load_timeline_items(&path).unwrap();
        assert!(
            items[0]
                .raw
                .as_ref()
                .and_then(|raw| raw.get("runtimeControlOutputDisplay"))
                .is_none()
        );
        let raw = items[1].raw.as_ref().unwrap();
        let display = raw.get("runtimeControlOutputDisplay").unwrap();
        assert_eq!(
            display.get("artifactName").and_then(Value::as_str),
            Some("dynamic-node-completion")
        );
        assert_eq!(
            display.get("kind").and_then(Value::as_str),
            Some("dynamic-node-completion")
        );
        assert_eq!(
            display.get("jsonText").and_then(Value::as_str),
            Some("{\"a\":\"b\"}")
        );
        assert_eq!(display.get("fenced").and_then(Value::as_bool), Some(true));
        let start = display.get("start").and_then(Value::as_u64).unwrap() as usize;
        let end = display.get("end").and_then(Value::as_u64).unwrap() as usize;
        let content = items[1].content.as_ref().unwrap();
        let display_text = content
            .encode_utf16()
            .skip(start)
            .take(end - start)
            .collect::<Vec<_>>();
        assert_eq!(
            String::from_utf16(&display_text).unwrap(),
            "```json\n{\"a\":\"b\"}\n```"
        );
    }

    #[test]
    fn annotates_invalid_runtime_control_text_delta() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        write_timeline_items(
            &path,
            &[test_timeline_event(
                "message-1",
                10,
                "修复前\n```json\n{\"a\":\"unterminated}\n```",
            )],
        )
        .unwrap();

        assert!(
            annotate_latest_runtime_control_output(&path, "accept-result", "workflow-output")
                .unwrap()
        );

        let items = load_timeline_items(&path).unwrap();
        let display = items[0]
            .raw
            .as_ref()
            .and_then(|raw| raw.get("runtimeControlOutputDisplay"))
            .unwrap();
        assert_eq!(
            display.get("kind").and_then(Value::as_str),
            Some("workflow-output")
        );
        assert_eq!(
            display.get("parseStatus").and_then(Value::as_str),
            Some("invalid")
        );
        assert_eq!(
            display.get("jsonText").and_then(Value::as_str),
            Some("{\"a\":\"unterminated}")
        );
        assert_eq!(display.get("fenced").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn timeline_overwrite_and_patch_are_same_path_serialized() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.timeline.jsonl");
        let mut handles = Vec::new();

        for thread_index in 0..8_u64 {
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                for write_index in 0..16_u64 {
                    let base = test_timeline_event(
                        &format!("message-{thread_index}-{write_index}"),
                        thread_index * 100 + write_index,
                        "base",
                    );
                    write_timeline_items(&path, &[base]).unwrap();
                    let patch = test_timeline_event(
                        &format!("message-{thread_index}-{write_index}"),
                        thread_index * 100 + write_index + 1,
                        "patch",
                    );
                    append_timeline_patch(
                        &path,
                        format!("message-{thread_index}-{write_index}"),
                        write_index,
                        &patch,
                    )
                    .unwrap();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let contents = std::fs::read_to_string(path.as_std_path()).unwrap();
        for line in contents.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        load_timeline_items(&path).unwrap();
    }

    #[test]
    fn raw_append_and_roll_are_same_path_serialized() {
        let dir = TempDir::new().unwrap();
        let path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.raw.jsonl");
        append_raw_frame(
            &path,
            "in",
            json!({"method": "initialize", "payload": "pinned"}),
            u64::MAX,
            u64::MAX,
        )
        .unwrap();
        let mut handles = Vec::new();
        for thread_index in 0..8_u64 {
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                for write_index in 0..24_u64 {
                    append_raw_frame(
                        &path,
                        "out",
                        json!({
                            "method": "session/update",
                            "thread": thread_index,
                            "write": write_index,
                            "payload": "x".repeat(2048),
                        }),
                        64 * 1024,
                        48 * 1024,
                    )
                    .unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        let contents = std::fs::read_to_string(path.as_std_path()).unwrap();
        for line in contents.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        assert!(contents.contains("initialize"));
    }

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
    fn metrics_reads_session_elapsed_seconds_from_snapshot() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "inputTokens":1000,"outputTokens":500,"cachedReadTokens":200,"totalTokens":1700,
            "timing":{"sessionElapsedSeconds":842,"paused":false}
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let m = super::read_session_metrics(&session_path);
        assert_eq!(m.input_tokens, 1000);
        assert_eq!(m.output_tokens, 500);
        assert_eq!(m.cache_read_tokens, 200);
        assert_eq!(m.total_tokens, 1700);
        assert_eq!(m.session_elapsed_seconds, 842);
    }

    #[test]
    fn metrics_returns_zero_for_elapsed_when_no_timing() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "inputTokens":100,"outputTokens":50,"cachedReadTokens":0,"totalTokens":150
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let m = super::read_session_metrics(&session_path);
        assert_eq!(m.session_elapsed_seconds, 0);
    }

    #[test]
    fn metrics_no_files_returns_zeros() {
        let dir = TempDir::new().unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let m = super::read_session_metrics(&session_path);
        assert_eq!(m.input_tokens, 0);
        assert_eq!(m.output_tokens, 0);
        assert_eq!(m.cache_read_tokens, 0);
        assert_eq!(m.total_tokens, 0);
        assert_eq!(m.session_elapsed_seconds, 0);
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
