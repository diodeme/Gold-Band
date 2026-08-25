use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::sync::{LazyLock, Mutex};

use agent_client_protocol_schema::v1::{CreateElicitationRequest, ElicitationScope};
use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

use crate::acp::control::AcpRuntimeControlCursor;
use crate::provider::{ConversationPromptInput, UserPromptQuote};
use crate::storage::{
    append_jsonl, append_jsonl_unlocked, atomic_write_file, ensure_parent_dir, read_json,
    with_jsonl_file_lock, write_json,
};

const AGENT_TRANSCRIPT_META_KEY: &str = "agentTranscript";
const CLAUDE_CODE_META_KEY: &str = "claudeCode";
const CLAUDE_AGENT_TOOL_NAMES: [&str; 2] = ["agent", "task"];

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &u64) -> bool {
    *value == 0
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
    /// Monotonic ownership generation for the ACP lifecycle facet. This is
    /// intentionally independent from timeline item, runtime and prompt-queue
    /// revisions. It only advances when lifecycle ownership changes or a turn
    /// reaches its terminal state, so an executor can use it as a CAS token.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub acp_revision: u64,
    /// Stable logical turn identity used to reject a late non-terminal update
    /// for a turn that has already reached a terminal state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_event_id: Option<String>,
    #[serde(default)]
    pub live_turn_activity: AcpLiveTurnActivity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_operation_id: Option<String>,
    pub restored: bool,
    pub stop_reason: Option<String>,
    pub capabilities: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modes: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Value>,
    /// Time when this session last observed a model/mode/config-option catalog
    /// from the ACP provider. Catalog freshness must not be inferred from the
    /// session's general `updated_at`, which also changes for ordinary turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_catalog_observed_at: Option<String>,
    /// A newer successful Doctor catalog selected by the user must be checked
    /// against this concrete session before its override is applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_catalog_refresh_required_at: Option<String>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpLiveTurnActivity {
    #[default]
    Idle,
    Starting,
    Accepted,
    Running,
    CancelRequested,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpLifecycleHeader {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_event_id: Option<String>,
    pub availability: AcpSessionAvailability,
    pub live_turn_activity: AcpLiveTurnActivity,
    pub latest_turn_status: AcpLatestTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptSubmission {
    pub turn_id: String,
    pub operation_id: String,
    pub adapter_id: String,
    pub adapter_display_name: String,
    pub cwd: String,
    pub input: ConversationPromptInput,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_paths: Vec<String>,
    pub admitted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpTurnAdmission {
    Started(AcpLifecycleHeader),
    ExistingActive(AcpLifecycleHeader),
    ExistingTerminal(AcpLifecycleHeader),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpLifecycleOwner {
    pub turn_id: String,
    pub operation_id: String,
    pub revision: u64,
}

/// Owns the terminal settlement fallback for one claimed provider turn.
///
/// Admission and terminal settlement use the same durable owner/CAS tuple, so
/// a late Drop from an older executor can only become a no-op after a newer
/// turn has taken over. Keeping this guard at the shared client boundary makes
/// Direct and provider-orchestrated prompts obey the same failure contract.
pub(crate) struct AcpLifecycleTerminalGuard {
    path: Utf8PathBuf,
    owner: AcpLifecycleOwner,
    armed: bool,
}

impl AcpLifecycleTerminalGuard {
    pub(crate) fn new(path: Utf8PathBuf, owner: AcpLifecycleOwner) -> Self {
        Self {
            path,
            owner,
            armed: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AcpLifecycleTerminalGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = persist_session_turn_terminal_owned(
            &self.path,
            &self.owner.turn_id,
            Some(&self.owner.operation_id),
            self.owner.revision,
            AcpLatestTurnStatus::Failed,
            "runtime-error",
            &current_timestamp(),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpStopRequestOutcome {
    pub lifecycle: AcpLifecycleHeader,
    /// Set only when this invocation durably acquired cancellation ownership.
    /// A duplicate stop or a terminal/no-op stop must not dispatch provider
    /// cancellation for the header's historical turn.
    pub owner: Option<AcpLifecycleOwner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpTurnExecutionClaim {
    Claimed(AcpLifecycleOwner),
    AlreadySettled(AcpLifecycleHeader),
    Stale,
}

impl AcpTurnAdmission {
    pub fn header(&self) -> &AcpLifecycleHeader {
        match self {
            Self::Started(header)
            | Self::ExistingActive(header)
            | Self::ExistingTerminal(header) => header,
        }
    }

    pub fn into_header(self) -> AcpLifecycleHeader {
        match self {
            Self::Started(header)
            | Self::ExistingActive(header)
            | Self::ExistingTerminal(header) => header,
        }
    }

    pub fn started(&self) -> bool {
        matches!(self, Self::Started(_))
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTriggerPayload {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: String,
    pub trigger_kind: crate::scheduler::occurrence::OccurrenceTriggerKind,
    pub scheduled_at: Option<String>,
    pub accepted_at: String,
    pub instruction_summary: String,
    pub content_fingerprint: String,
    pub links: crate::scheduler::occurrence::OccurrenceLinks,
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
    /// Exact runtime accumulator state used to restore timing without
    /// replaying the complete timeline. Older patches omit this field and
    /// continue to use the legacy projection as a read-only fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_snapshot: Option<AcpTimingStateSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpTimingStateSnapshot {
    pub elapsed_seconds: u64,
    pub active_turn_started_at: Option<u64>,
    pub active_turn_last_activity_at: Option<u64>,
    pub revision: Option<u64>,
    pub saw_turn: bool,
    pub pending_permission_ids: Vec<String>,
    pub pending_elicitation_ids: Vec<String>,
    pub user_wait_started_at: Option<u64>,
    pub user_wait_seconds: u64,
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
    pub fn state_snapshot(&self) -> AcpTimingStateSnapshot {
        let mut pending_permission_ids = self
            .pending_permission_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        pending_permission_ids.sort();
        let mut pending_elicitation_ids = self
            .pending_elicitation_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        pending_elicitation_ids.sort();
        AcpTimingStateSnapshot {
            elapsed_seconds: self.elapsed_seconds,
            active_turn_started_at: self.active_turn_started_at,
            active_turn_last_activity_at: self.active_turn_last_activity_at,
            revision: self.revision,
            saw_turn: self.saw_turn,
            pending_permission_ids,
            pending_elicitation_ids,
            user_wait_started_at: self.user_wait_started_at,
            user_wait_seconds: self.user_wait_seconds,
        }
    }

    pub fn from_state_snapshot(snapshot: AcpTimingStateSnapshot) -> Self {
        Self {
            elapsed_seconds: snapshot.elapsed_seconds,
            active_turn_started_at: snapshot.active_turn_started_at,
            active_turn_last_activity_at: snapshot.active_turn_last_activity_at,
            revision: snapshot.revision,
            saw_turn: snapshot.saw_turn,
            pending_permission_ids: snapshot.pending_permission_ids.into_iter().collect(),
            pending_elicitation_ids: snapshot.pending_elicitation_ids.into_iter().collect(),
            user_wait_started_at: snapshot.user_wait_started_at,
            user_wait_seconds: snapshot.user_wait_seconds,
        }
    }

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
                state_snapshot: Some(self.state_snapshot()),
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

/// Optional cumulative totals owned by one persisted ACP attempt. Missing
/// provider data remains `None`; metrics callers must never guess it as zero.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttemptMetrics {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

pub fn read_attempt_metrics(session_path: &Utf8Path) -> AttemptMetrics {
    let Some(snapshot_path) = session_path
        .parent()
        .map(|path| path.join("acp.snapshot.json"))
    else {
        return AttemptMetrics::default();
    };
    let Ok(contents) = std::fs::read_to_string(snapshot_path.as_std_path()) else {
        return AttemptMetrics::default();
    };
    let Ok(metadata) = serde_json::from_str::<AcpSessionMetadata>(&contents) else {
        return AttemptMetrics::default();
    };
    AttemptMetrics {
        input_tokens: metadata.attempt_input_tokens,
        output_tokens: metadata.attempt_output_tokens,
        cache_read_tokens: metadata.attempt_cached_read_tokens,
        total_tokens: metadata.attempt_total_tokens,
        elapsed_ms: metadata
            .timing
            .map(|timing| timing.session_elapsed_seconds.saturating_mul(1000)),
    }
}

pub fn read_attempt_session_model(session_path: &Utf8Path) -> Option<String> {
    let snapshot_path = session_path.parent()?.join("acp.snapshot.json");
    let metadata = read_attempt_session_metadata(&snapshot_path)
        .or_else(|| read_attempt_session_metadata(session_path))?;
    metadata
        .model_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| config_option_current_value(metadata.config_options.as_ref(), "model"))
        .or_else(|| {
            metadata
                .models
                .as_ref()
                .and_then(|value| value.get("currentModelId"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

pub fn read_attempt_session_model_name(session_path: &Utf8Path) -> Option<String> {
    let snapshot_path = session_path.parent()?.join("acp.snapshot.json");
    let metadata = read_attempt_session_metadata(&snapshot_path)
        .or_else(|| read_attempt_session_metadata(session_path))?;
    let selected = metadata
        .models
        .as_ref()
        .and_then(|value| value.get("currentModelId"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            metadata
                .model_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| config_option_current_value(metadata.config_options.as_ref(), "model"))?;
    metrics_model_display_name(&metadata, &selected)
}

pub(crate) fn metrics_model_display_name(
    metadata: &AcpSessionMetadata,
    model_id: &str,
) -> Option<String> {
    let model_id = model_id.trim();
    if model_id.eq_ignore_ascii_case("default") {
        return default_model_display_name(metadata.config_options.as_ref())
            .or_else(|| {
                config_option_display_name(metadata.config_options.as_ref(), "model", model_id)
            })
            .or_else(|| model_display_name(metadata.models.as_ref(), model_id))
            .or_else(|| Some(model_id.to_string()));
    }
    config_option_display_name(metadata.config_options.as_ref(), "model", model_id)
        .or_else(|| model_display_name(metadata.models.as_ref(), model_id))
        .or_else(|| Some(model_id.to_string()))
}

pub(crate) fn read_attempt_session_metadata(path: &Utf8Path) -> Option<AcpSessionMetadata> {
    let contents = std::fs::read_to_string(path.as_std_path()).ok()?;
    serde_json::from_str(&contents).ok()
}

fn config_option_current_value(config_options: Option<&Value>, option_id: &str) -> Option<String> {
    config_options?
        .as_array()?
        .iter()
        .find(|option| option.get("id").and_then(|value| value.as_str()) == Some(option_id))
        .and_then(|option| option.get("currentValue"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn config_option_display_name(
    config_options: Option<&Value>,
    option_id: &str,
    value: &str,
) -> Option<String> {
    find_config_option(config_options, option_id)
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
        .and_then(|options| {
            options
                .iter()
                .find(|option| option.get("value").and_then(Value::as_str) == Some(value))
        })
        .and_then(|option| option.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn model_display_name(models: Option<&Value>, model_id: &str) -> Option<String> {
    models
        .and_then(|value| value.get("availableModels"))
        .and_then(Value::as_array)
        .and_then(|models| {
            models
                .iter()
                .find(|model| model.get("modelId").and_then(Value::as_str) == Some(model_id))
        })
        .and_then(|model| model.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn default_model_display_name(config_options: Option<&Value>) -> Option<String> {
    let description = find_config_option(config_options, "model")?
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .find(|option| option.get("value").and_then(Value::as_str) == Some("default"))?
        .get("description")
        .and_then(Value::as_str)?;
    parse_current_model_from_description(description)
}

fn parse_current_model_from_description(description: &str) -> Option<String> {
    let marker = "currently ";
    let start = description.to_ascii_lowercase().find(marker)? + marker.len();
    let tail = &description[start..];
    let candidate = tail.split(['[', ')', '|', ',']).next()?.trim();
    (!candidate.is_empty()).then(|| candidate.to_string())
}

fn find_config_option<'a>(config_options: Option<&'a Value>, option_id: &str) -> Option<&'a Value> {
    config_options?.as_array()?.iter().find(|option| {
        option.get("id").and_then(Value::as_str) == Some(option_id)
            || option.get("category").and_then(Value::as_str) == Some(option_id)
    })
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
        atomic_write_file(path.as_std_path(), |file| -> Result<()> {
            for item in items {
                let mut item = item.clone();
                crate::acp::timeline::externalize_timeline_event_for_storage(path, &mut item)?;
                serde_json::to_writer(&mut *file, &AcpTimelineItem { item })?;
                use std::io::Write as _;
                file.write_all(b"\n")?;
            }
            Ok(())
        })
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
    let attempt_dir = path.parent().unwrap_or(path);
    let snapshot = attempt_dir.join("acp.snapshot.json");
    let session = attempt_dir.join("acp.session.json");
    let prompt_event_id = [snapshot.as_path(), session.as_path()]
        .into_iter()
        .find_map(|metadata_path| {
            read_json::<Value>(metadata_path).ok().and_then(|metadata| {
                metadata
                    .get("promptEventId")
                    .or_else(|| metadata.pointer("/promptRetry/promptEventId"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        });
    Ok(matches!(
        crate::acp::timeline::settle_latest_processing_retry_prompt(
            path,
            prompt_event_id.as_deref(),
            decided_at,
        )?,
        crate::acp::timeline::TimelineSettleOutcome::Applied
    ))
}

pub fn load_timeline_items(path: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    with_jsonl_file_lock(path, || load_timeline_items_unlocked(path))
}

pub fn annotate_latest_runtime_control_output(
    path: &Utf8Path,
    artifact_name: &str,
    kind: &str,
) -> Result<bool> {
    crate::acp::timeline::annotate_latest_runtime_control_output(path, artifact_name, kind)
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
    if existing.kind == "scheduledTrigger" {
        return existing.clone();
    }
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

const SESSION_METADATA_LOCK_STRIPES: usize = 64;
static SESSION_METADATA_LOCKS: LazyLock<Vec<Mutex<()>>> = LazyLock::new(|| {
    (0..SESSION_METADATA_LOCK_STRIPES)
        .map(|_| Mutex::new(()))
        .collect()
});
static ACTIVE_SESSION_TURNS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn active_session_turn_key(path: &Utf8Path, turn_id: &str) -> String {
    // `acp.session.json` is the legacy metadata source while
    // `acp.snapshot.json` is the current write target.  Both represent the
    // same attempt lifecycle, so an in-process admission must survive a
    // metadata-file migration during provider startup.
    let attempt_dir = path.parent().unwrap_or(path);
    format!("{attempt_dir}::{turn_id}")
}

fn admission_metadata_base(path: &Utf8Path) -> Result<Option<Value>> {
    if path.exists() {
        return read_json(path).map(Some);
    }
    if path.file_name() == Some("acp.snapshot.json") {
        let legacy_session = path.with_file_name("acp.session.json");
        if legacy_session.exists() {
            return read_json(&legacy_session).map(Some);
        }
    }
    Ok(None)
}

fn mark_session_turn_active(path: &Utf8Path, turn_id: &str) -> Result<()> {
    ACTIVE_SESSION_TURNS
        .lock()
        .map_err(|_| anyhow::anyhow!("ACP active session turn registry poisoned"))?
        .insert(active_session_turn_key(path, turn_id));
    Ok(())
}

fn clear_session_turn_active(path: &Utf8Path, turn_id: Option<&str>) {
    let Some(turn_id) = turn_id else { return };
    if let Ok(mut active) = ACTIVE_SESSION_TURNS.lock() {
        active.remove(&active_session_turn_key(path, turn_id));
    }
}

fn session_turn_is_active(path: &Utf8Path, turn_id: Option<&str>) -> bool {
    let Some(turn_id) = turn_id else { return false };
    ACTIVE_SESSION_TURNS
        .lock()
        .is_ok_and(|active| active.contains(&active_session_turn_key(path, turn_id)))
}

fn session_metadata_lock(path: &Utf8Path) -> &Mutex<()> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    &SESSION_METADATA_LOCKS[(hasher.finish() as usize) % SESSION_METADATA_LOCK_STRIPES]
}

fn lifecycle_header_from_value(value: &Value) -> AcpLifecycleHeader {
    let persisted_availability = value
        .get("availability")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let latest_turn_status = value
        .get("latestTurnStatus")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let live_turn_activity = value
        .get("liveTurnActivity")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| {
            if latest_turn_status != AcpLatestTurnStatus::None {
                AcpLiveTurnActivity::Idle
            } else if persisted_availability == AcpSessionAvailability::Closing {
                AcpLiveTurnActivity::CancelRequested
            } else {
                AcpLiveTurnActivity::Idle
            }
        });
    let mut header = AcpLifecycleHeader {
        revision: value
            .get("acpRevision")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        turn_id: value
            .get("turnId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .pointer("/promptRetry/promptId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        prompt_event_id: value
            .get("promptEventId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .pointer("/promptRetry/promptEventId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        availability: persisted_availability,
        live_turn_activity,
        latest_turn_status,
        stop_reason: value
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_string),
        operation_id: value
            .get("lifecycleOperationId")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    normalize_lifecycle_header(value, &mut header);
    header
}

fn session_availability_from_value(
    value: &Value,
    current: AcpSessionAvailability,
) -> AcpSessionAvailability {
    if current == AcpSessionAvailability::Restorable {
        return AcpSessionAvailability::Restorable;
    }
    if value
        .get("sessionId")
        .or_else(|| value.get("acpSessionId"))
        .and_then(Value::as_str)
        .is_some_and(|session_id| !session_id.trim().is_empty())
    {
        AcpSessionAvailability::Established
    } else {
        AcpSessionAvailability::Unavailable
    }
}

/// Enforces the canonical split between session availability, live turn
/// activity and the previous turn outcome. `closing` is a legacy transport
/// value and must never survive as the availability of a turn lifecycle.
fn normalize_lifecycle_header(value: &Value, header: &mut AcpLifecycleHeader) {
    if header.availability == AcpSessionAvailability::Closing {
        header.availability = session_availability_from_value(value, header.availability);
    }
    if header.live_turn_activity != AcpLiveTurnActivity::Idle {
        header.latest_turn_status = AcpLatestTurnStatus::None;
        header.stop_reason = None;
    } else if header.latest_turn_status != AcpLatestTurnStatus::None {
        header.availability = session_availability_from_value(value, header.availability);
    }
}

enum AcpLifecycleTransition<'a> {
    PromptAdmitted(&'a AcpPromptSubmission),
    ExecutionClaimed,
    StopRequested {
        operation_id: &'a str,
    },
    TurnSettled {
        status: AcpLatestTurnStatus,
        reason: &'a str,
    },
}

fn reduce_lifecycle_header(
    value: &Value,
    current: AcpLifecycleHeader,
    transition: AcpLifecycleTransition<'_>,
) -> Result<AcpLifecycleHeader> {
    let was_cancel_requested = current.live_turn_activity == AcpLiveTurnActivity::CancelRequested;
    let mut next = current;
    next.revision = next.revision.saturating_add(1).max(1);
    match transition {
        AcpLifecycleTransition::PromptAdmitted(submission) => {
            next.turn_id = Some(submission.turn_id.clone());
            next.prompt_event_id = None;
            next.live_turn_activity = AcpLiveTurnActivity::Starting;
            next.latest_turn_status = AcpLatestTurnStatus::None;
            next.stop_reason = None;
            next.operation_id = Some(submission.operation_id.clone());
        }
        AcpLifecycleTransition::ExecutionClaimed => {
            next.live_turn_activity = AcpLiveTurnActivity::Accepted;
            next.latest_turn_status = AcpLatestTurnStatus::None;
            next.stop_reason = None;
        }
        AcpLifecycleTransition::StopRequested { operation_id } => {
            next.live_turn_activity = AcpLiveTurnActivity::CancelRequested;
            next.latest_turn_status = AcpLatestTurnStatus::None;
            next.stop_reason = None;
            next.operation_id = Some(operation_id.to_string());
            if next.turn_id.is_none() {
                next.turn_id = Some(format!("stop:{operation_id}"));
            }
        }
        AcpLifecycleTransition::TurnSettled { status, reason } => {
            if status == AcpLatestTurnStatus::None {
                anyhow::bail!("acp.lifecycle-terminal-status-required");
            }
            next.live_turn_activity = AcpLiveTurnActivity::Idle;
            // A durable cancellation request is the canonical user intent.
            // Provider completion/error callbacks may race with it, so they
            // cannot turn a cancelled turn into a successful terminal result.
            next.latest_turn_status = if was_cancel_requested {
                AcpLatestTurnStatus::Cancelled
            } else {
                status
            };
            next.stop_reason = Some(
                if next.latest_turn_status == AcpLatestTurnStatus::Cancelled {
                    "cancelled".to_string()
                } else {
                    reason.to_string()
                },
            );
        }
    }
    normalize_lifecycle_header(value, &mut next);
    validate_lifecycle_header(&next)?;
    Ok(next)
}

fn validate_lifecycle_header(header: &AcpLifecycleHeader) -> Result<()> {
    if header.live_turn_activity != AcpLiveTurnActivity::Idle
        && header.latest_turn_status != AcpLatestTurnStatus::None
    {
        anyhow::bail!("acp.lifecycle-active-turn-has-terminal-status");
    }
    if lifecycle_is_terminal(header) && header.availability == AcpSessionAvailability::Closing {
        anyhow::bail!("acp.lifecycle-terminal-session-closing");
    }
    Ok(())
}

fn lifecycle_is_terminal(header: &AcpLifecycleHeader) -> bool {
    header.live_turn_activity == AcpLiveTurnActivity::Idle
        && header.latest_turn_status != AcpLatestTurnStatus::None
}

fn lifecycle_is_stopping(header: &AcpLifecycleHeader) -> bool {
    header.live_turn_activity == AcpLiveTurnActivity::CancelRequested
}

fn lifecycle_fingerprint(
    header: &AcpLifecycleHeader,
) -> (
    Option<&str>,
    Option<&str>,
    AcpSessionAvailability,
    AcpLiveTurnActivity,
    AcpLatestTurnStatus,
    Option<&str>,
) {
    (
        header.turn_id.as_deref(),
        header.prompt_event_id.as_deref(),
        header.availability,
        header.live_turn_activity,
        header.latest_turn_status,
        header.stop_reason.as_deref(),
    )
}

fn apply_lifecycle_header(value: &mut Value, header: &AcpLifecycleHeader) {
    let mut header = header.clone();
    normalize_lifecycle_header(value, &mut header);
    value["acpRevision"] = serde_json::json!(header.revision);
    value["availability"] = serde_json::to_value(header.availability).unwrap_or(Value::Null);
    value["liveTurnActivity"] =
        serde_json::to_value(header.live_turn_activity).unwrap_or(Value::Null);
    value["latestTurnStatus"] =
        serde_json::to_value(header.latest_turn_status).unwrap_or(Value::Null);
    for (key, field) in [
        ("turnId", header.turn_id.as_ref()),
        ("promptEventId", header.prompt_event_id.as_ref()),
        ("stopReason", header.stop_reason.as_ref()),
        ("lifecycleOperationId", header.operation_id.as_ref()),
    ] {
        if let Some(field) = field {
            value[key] = Value::String(field.clone());
        } else if let Some(object) = value.as_object_mut() {
            object.remove(key);
        }
    }
}

/// Applies the Runtime-control projection without replacing the canonical ACP
/// lifecycle stored in the same metadata file. Runtime control and turn
/// lifecycle are separate domains, but sharing one JSON document means every
/// writer must participate in the same file-level transaction.
pub fn patch_session_runtime_control(
    path: &Utf8Path,
    cursor: Option<&AcpRuntimeControlCursor>,
    timeline_scan_complete: bool,
) -> Result<()> {
    patch_session_metadata(path, |value| {
        if let Some(cursor) = cursor {
            value["runtimeControl"] = serde_json::to_value(cursor)?;
        }
        if timeline_scan_complete {
            value["runtimeControlTimelineScanComplete"] = Value::Bool(true);
        }
        Ok(())
    })
    .map(drop)
}

/// Applies a non-lifecycle metadata update against the latest snapshot while
/// holding the same metadata transaction used by lifecycle admission and
/// settlement. Callers must only patch their own projection fields; the
/// canonical lifecycle header is restored from the value read under the lock
/// before the file is written.
pub fn patch_session_metadata<F>(path: &Utf8Path, patch: F) -> Result<Value>
where
    F: FnOnce(&mut Value) -> Result<()>,
{
    let _guard = session_metadata_lock(path).lock().unwrap();
    let mut value = if let Some(value) = admission_metadata_base(path)? {
        value
    } else {
        serde_json::json!({
            "availability": "unavailable",
            "liveTurnActivity": "idle",
            "latestTurnStatus": "none",
            "restored": false,
            "createdAt": current_timestamp(),
        })
    };
    let canonical_header = lifecycle_header_from_value(&value);
    patch(&mut value)?;
    apply_lifecycle_header(&mut value, &canonical_header);
    validate_lifecycle_header(&canonical_header)?;
    ensure_parent_dir(path)?;
    write_json(path, &value)?;
    Ok(value)
}

fn prompt_submission_from_value(value: &Value) -> Result<Option<AcpPromptSubmission>> {
    value
        .get("promptSubmission")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn classify_existing_turn(
    value: &Value,
    submission: &AcpPromptSubmission,
) -> Result<Option<AcpTurnAdmission>> {
    let current = lifecycle_header_from_value(value);
    if current.turn_id.as_deref() != Some(submission.turn_id.as_str()) {
        return Ok(None);
    }
    if let Some(persisted) = prompt_submission_from_value(value)?
        && (persisted.input != submission.input
            || persisted.attachment_paths != submission.attachment_paths)
    {
        anyhow::bail!("acp.prompt-submission-conflict");
    }
    if lifecycle_is_terminal(&current) || lifecycle_is_stopping(&current) {
        Ok(Some(AcpTurnAdmission::ExistingTerminal(current)))
    } else {
        Ok(Some(AcpTurnAdmission::ExistingActive(current)))
    }
}

pub fn inspect_session_turn(
    path: &Utf8Path,
    submission: &AcpPromptSubmission,
) -> Result<Option<AcpTurnAdmission>> {
    // Admission must not preserve a non-terminal turn from a prior process.
    // The active registry is intentionally process-local, so reconcile before
    // classifying a retried request as an in-flight duplicate.
    let _ = reconcile_orphaned_session_turn(path)?;
    let _guard = session_metadata_lock(path).lock().unwrap();
    admission_metadata_base(path)?
        .as_ref()
        .map(|value| classify_existing_turn(value, submission))
        .transpose()
        .map(Option::flatten)
}

pub fn read_session_prompt_submission(
    path: &Utf8Path,
    turn_id: &str,
) -> Result<Option<AcpPromptSubmission>> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(None);
    }
    let value = read_json::<Value>(path)?;
    let submission = prompt_submission_from_value(&value)?;
    Ok(submission.filter(|submission| submission.turn_id == turn_id))
}

/// Atomically claims an admitted turn for provider execution. The admission
/// revision and operation identity fence a delayed executor from a newer turn
/// or a duplicate executor that has already claimed this one.
pub fn claim_session_turn_for_execution(
    path: &Utf8Path,
    turn_id: &str,
    expected_revision: u64,
    expected_operation_id: &str,
) -> Result<AcpTurnExecutionClaim> {
    if expected_operation_id.trim().is_empty() {
        return Ok(AcpTurnExecutionClaim::Stale);
    }
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(AcpTurnExecutionClaim::Stale);
    }
    let mut value = read_json::<Value>(path)?;
    let current = lifecycle_header_from_value(&value);
    if current.turn_id.as_deref() != Some(turn_id)
        || current.revision != expected_revision
        || current.operation_id.as_deref() != Some(expected_operation_id)
    {
        return Ok(AcpTurnExecutionClaim::Stale);
    }
    if lifecycle_is_terminal(&current) {
        return Ok(AcpTurnExecutionClaim::AlreadySettled(current));
    }
    if current.latest_turn_status != AcpLatestTurnStatus::None
        || current.live_turn_activity != AcpLiveTurnActivity::Starting
    {
        return Ok(AcpTurnExecutionClaim::Stale);
    }
    let accepted =
        reduce_lifecycle_header(&value, current, AcpLifecycleTransition::ExecutionClaimed)?;
    apply_lifecycle_header(&mut value, &accepted);
    value["updatedAt"] = Value::String(current_timestamp());
    write_json(path, &value)?;
    Ok(AcpTurnExecutionClaim::Claimed(AcpLifecycleOwner {
        turn_id: turn_id.to_string(),
        operation_id: expected_operation_id.to_string(),
        revision: accepted.revision,
    }))
}

/// Admits and claims an orchestrated ACP prompt at the provider boundary.
///
/// A scheduler may rebuild the provider runtime for the same logical turn
/// after a recoverable startup/transport failure. In that case the durable
/// lifecycle is already `accepted`; the process-local active-turn registry
/// proves that this is the same in-process execution generation, so the exact
/// owner can be reused. A stop or terminal transition always wins because the
/// header is rechecked under the metadata lock before an existing owner is
/// returned.
pub fn admit_session_turn_for_execution(
    path: &Utf8Path,
    submission: &AcpPromptSubmission,
) -> Result<AcpTurnExecutionClaim> {
    let admission = begin_session_turn(path, submission)?;
    let header = admission.header();
    match admission {
        AcpTurnAdmission::ExistingTerminal(header) => {
            Ok(AcpTurnExecutionClaim::AlreadySettled(header))
        }
        AcpTurnAdmission::Started(_)
            if header.live_turn_activity == AcpLiveTurnActivity::Starting =>
        {
            claim_session_turn_for_execution(
                path,
                &submission.turn_id,
                header.revision,
                &submission.operation_id,
            )
        }
        AcpTurnAdmission::ExistingActive(_)
            if header.live_turn_activity == AcpLiveTurnActivity::Starting =>
        {
            let Some(operation_id) = header.operation_id.as_deref() else {
                return Ok(AcpTurnExecutionClaim::Stale);
            };
            claim_session_turn_for_execution(
                path,
                &submission.turn_id,
                header.revision,
                operation_id,
            )
        }
        AcpTurnAdmission::ExistingActive(_)
            if matches!(
                header.live_turn_activity,
                AcpLiveTurnActivity::Accepted | AcpLiveTurnActivity::Running
            ) =>
        {
            let expected_revision = header.revision;
            let expected_operation_id = header.operation_id.clone();
            let _guard = session_metadata_lock(path).lock().unwrap();
            if !path.exists() || !session_turn_is_active(path, Some(&submission.turn_id)) {
                return Ok(AcpTurnExecutionClaim::Stale);
            }
            let current = lifecycle_header_from_value(&read_json::<Value>(path)?);
            if current.turn_id.as_deref() != Some(submission.turn_id.as_str())
                || current.operation_id != expected_operation_id
                || current.revision != expected_revision
                || !matches!(
                    current.live_turn_activity,
                    AcpLiveTurnActivity::Accepted | AcpLiveTurnActivity::Running
                )
            {
                return Ok(AcpTurnExecutionClaim::Stale);
            }
            let Some(operation_id) = current.operation_id else {
                return Ok(AcpTurnExecutionClaim::Stale);
            };
            Ok(AcpTurnExecutionClaim::Claimed(AcpLifecycleOwner {
                turn_id: submission.turn_id.clone(),
                operation_id,
                revision: current.revision,
            }))
        }
        _ => Ok(AcpTurnExecutionClaim::Stale),
    }
}

pub fn reconcile_orphaned_session_turn(path: &Utf8Path) -> Result<Option<AcpLifecycleHeader>> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(None);
    }
    let mut value = read_json::<Value>(path)?;
    if value.get("promptSubmission").is_none() {
        return Ok(None);
    }
    let current = lifecycle_header_from_value(&value);
    if current.live_turn_activity == AcpLiveTurnActivity::Idle
        || session_turn_is_active(path, current.turn_id.as_deref())
    {
        return Ok(None);
    }
    let was_stopping = lifecycle_is_stopping(&current);
    let (latest_turn_status, stop_reason) = if was_stopping {
        // A durable stop is already user intent. Once its owning process is
        // gone, preserve that intent instead of leaving CancelRequested
        // without a possible finalizer.
        (AcpLatestTurnStatus::Cancelled, "cancelled")
    } else {
        (AcpLatestTurnStatus::Failed, "process-interrupted")
    };
    let terminal = reduce_lifecycle_header(
        &value,
        current,
        AcpLifecycleTransition::TurnSettled {
            status: latest_turn_status,
            reason: stop_reason,
        },
    )?;
    apply_lifecycle_header(&mut value, &terminal);
    value["updatedAt"] = Value::String(current_timestamp());
    write_json(path, &value)?;
    Ok(Some(terminal))
}

/// Merge a metadata rewrite with the durable ACP lifecycle header. A late
/// accepted/running write for the same logical turn can never overwrite an
/// already committed terminal state.
fn merge_session_lifecycle(current: Option<&Value>, incoming: &mut Value) {
    let Some(current) = current else {
        let mut header = lifecycle_header_from_value(incoming);
        header.revision = header.revision.max(1);
        apply_lifecycle_header(incoming, &header);
        return;
    };
    let current_header = lifecycle_header_from_value(current);
    let mut incoming_header = lifecycle_header_from_value(incoming);
    let incoming_turn_id = incoming_header.turn_id.clone();
    let incoming_operation_id = incoming_header.operation_id.clone();
    let has_lifecycle_owner = incoming_turn_id.is_some() && incoming_operation_id.is_some();
    if let Some(prompt_submission) = current.get("promptSubmission") {
        incoming["promptSubmission"] = prompt_submission.clone();
    }
    // Runtime control is a separate domain sharing this file. Provider
    // metadata is assembled before it acquires the file transaction, so its
    // copy may be older than a just-committed continue/interruption cursor.
    for key in ["runtimeControl", "runtimeControlTimelineScanComplete"] {
        if let Some(field) = current.get(key) {
            incoming[key] = field.clone();
        } else if let Some(object) = incoming.as_object_mut() {
            object.remove(key);
        }
    }
    // The effective launch configuration becomes the initial mutable session
    // override when provider metadata first establishes the ACP session. Once
    // a session id is durable, these fields are command-owned: their absence
    // means the user selected "unspecified" and a stale provider rewrite must
    // not resurrect its older copy.
    let session_config_initialized = current
        .get("sessionId")
        .and_then(Value::as_str)
        .is_some_and(|session_id| !session_id.trim().is_empty());
    for key in [
        "modelOverride",
        "permissionModeOverride",
        "configOptionOverrides",
        "configCatalogRefreshRequiredAt",
    ] {
        if let Some(field) = current.get(key) {
            incoming[key] = field.clone();
        } else if session_config_initialized && let Some(object) = incoming.as_object_mut() {
            object.remove(key);
        }
    }
    if has_lifecycle_owner
        && incoming_turn_id.is_some()
        && current_header.turn_id.is_some()
        && incoming_turn_id != current_header.turn_id
    {
        // A provider object created before a later admission may finish after
        // the newer turn is durable. Preserve the newer canonical lifecycle;
        // its non-lifecycle fields are still allowed to merge below.
        apply_lifecycle_header(incoming, &current_header);
        return;
    }
    if has_lifecycle_owner
        && incoming_operation_id.is_some()
        && current_header.operation_id.is_some()
        && incoming_operation_id != current_header.operation_id
    {
        apply_lifecycle_header(incoming, &current_header);
        return;
    }
    if !has_lifecycle_owner {
        // Catalog/diagnostic rewrites do not own turn lifecycle. Keep the
        // current header verbatim instead of inferring identity from it.
        apply_lifecycle_header(incoming, &current_header);
        return;
    }
    let same_owner = incoming_header.turn_id == current_header.turn_id
        && incoming_header.operation_id == current_header.operation_id;
    if same_owner
        && lifecycle_is_terminal(&current_header)
        && (!lifecycle_is_terminal(&incoming_header)
            || incoming_header.latest_turn_status != current_header.latest_turn_status)
    {
        apply_lifecycle_header(incoming, &current_header);
        return;
    }
    if same_owner
        && lifecycle_is_stopping(&current_header)
        && !lifecycle_is_terminal(&incoming_header)
        && !lifecycle_is_stopping(&incoming_header)
    {
        apply_lifecycle_header(incoming, &current_header);
        return;
    }
    if same_owner {
        // Running metadata is produced by the executor that owns this
        // generation. Keeping its revision stable lets that executor settle a
        // provider failure with an exact CAS check. A terminal write closes
        // the generation and invalidates any later callback from it.
        incoming_header.revision =
            if lifecycle_is_terminal(&incoming_header) && !lifecycle_is_terminal(&current_header) {
                current_header.revision.saturating_add(1).max(1)
            } else {
                current_header.revision
            };
    } else if lifecycle_fingerprint(&incoming_header) == lifecycle_fingerprint(&current_header) {
        incoming_header.revision = incoming_header.revision.max(current_header.revision);
    } else {
        incoming_header.revision = incoming_header
            .revision
            .max(current_header.revision.saturating_add(1));
    }
    apply_lifecycle_header(incoming, &incoming_header);
}

pub fn write_session_metadata(path: &Utf8Path, metadata: &AcpSessionMetadata) -> Result<()> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    let current = path
        .exists()
        .then(|| read_json::<Value>(path))
        .transpose()?;
    let mut incoming = serde_json::to_value(metadata)?;
    merge_session_lifecycle(current.as_ref(), &mut incoming);
    write_json(path, &incoming)?;
    let header = lifecycle_header_from_value(&incoming);
    if lifecycle_is_terminal(&header) {
        clear_session_turn_active(path, header.turn_id.as_deref());
    }
    Ok(())
}

/// Writes provider-owned session metadata only while the execution claim is
/// still the canonical lifecycle owner. A stop, terminal transition, or newer
/// turn advances ownership and turns every delayed provider write into a
/// stale no-op.
pub fn write_session_metadata_owned(
    path: &Utf8Path,
    metadata: &AcpSessionMetadata,
    owner: &AcpLifecycleOwner,
) -> Result<Option<AcpLifecycleHeader>> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(None);
    }
    let current = read_json::<Value>(path)?;
    let current_header = lifecycle_header_from_value(&current);
    if current_header.turn_id.as_deref() != Some(owner.turn_id.as_str())
        || current_header.operation_id.as_deref() != Some(owner.operation_id.as_str())
        || current_header.revision != owner.revision
    {
        return Ok(None);
    }

    let mut incoming = serde_json::to_value(metadata)?;
    let mut incoming_header = lifecycle_header_from_value(&incoming);
    incoming_header.turn_id = Some(owner.turn_id.clone());
    incoming_header.operation_id = Some(owner.operation_id.clone());
    incoming_header.revision = owner.revision;
    apply_lifecycle_header(&mut incoming, &incoming_header);
    merge_session_lifecycle(Some(&current), &mut incoming);
    write_json(path, &incoming)?;
    let updated = lifecycle_header_from_value(&incoming);
    if lifecycle_is_terminal(&updated) {
        clear_session_turn_active(path, updated.turn_id.as_deref());
    }
    Ok(Some(updated))
}

/// Reliably accepts one logical prompt turn before provider initialization.
/// The durable header is the admission record used by UI projection and stop;
/// provider work must not start until this write succeeds.
pub fn begin_session_turn(
    path: &Utf8Path,
    submission: &AcpPromptSubmission,
) -> Result<AcpTurnAdmission> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    let mut value = admission_metadata_base(path)?.unwrap_or_else(|| {
        serde_json::json!({
            "adapterId": submission.adapter_id,
            "adapterDisplayName": submission.adapter_display_name,
            "cwd": submission.cwd,
            "availability": "unavailable",
            "latestTurnStatus": "none",
            "restored": false,
            "capabilities": {},
            "createdAt": submission.admitted_at,
            "updatedAt": submission.admitted_at,
        })
    });
    if let Some(existing) = classify_existing_turn(&value, submission)? {
        return Ok(existing);
    }
    let current = lifecycle_header_from_value(&value);
    if current.live_turn_activity != AcpLiveTurnActivity::Idle {
        anyhow::bail!("acp.prompt-session-busy");
    }
    let starting = reduce_lifecycle_header(
        &value,
        current,
        AcpLifecycleTransition::PromptAdmitted(submission),
    )?;
    apply_lifecycle_header(&mut value, &starting);
    value["promptSubmission"] = serde_json::to_value(submission)?;
    value["updatedAt"] = Value::String(submission.admitted_at.clone());
    write_json(path, &value)?;
    mark_session_turn_active(path, &submission.turn_id)?;
    Ok(AcpTurnAdmission::Started(starting))
}

/// Settles a background submission only while the same logical turn still
/// owns the lifecycle header. A late failure can therefore never terminate a
/// newer follow-up admitted after it.
pub fn persist_session_turn_terminal_owned(
    path: &Utf8Path,
    turn_id: &str,
    operation_id: Option<&str>,
    expected_revision: u64,
    latest_turn_status: AcpLatestTurnStatus,
    stop_reason: &str,
    decided_at: &str,
) -> Result<Option<AcpLifecycleHeader>> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(None);
    }
    let mut value = read_json::<Value>(path)?;
    let current = lifecycle_header_from_value(&value);
    if current.turn_id.as_deref() != Some(turn_id)
        || current.operation_id.as_deref() != operation_id
        || current.revision != expected_revision
    {
        return Ok(None);
    }
    if lifecycle_is_terminal(&current) {
        return Ok(Some(current));
    }
    let terminal = reduce_lifecycle_header(
        &value,
        current,
        AcpLifecycleTransition::TurnSettled {
            status: latest_turn_status,
            reason: stop_reason,
        },
    )?;
    apply_lifecycle_header(&mut value, &terminal);
    value["updatedAt"] = Value::String(decided_at.to_string());
    write_json(path, &value)?;
    clear_session_turn_active(path, terminal.turn_id.as_deref());
    Ok(Some(terminal))
}

/// Persist the stop-accepted control fact without opening timeline, raw or
/// diagnostics files. The provider finalizer later advances this same header
/// to an idle terminal revision.
pub fn request_session_stop(
    path: &Utf8Path,
    operation_id: &str,
    decided_at: &str,
) -> Result<AcpLifecycleHeader> {
    Ok(request_session_stop_outcome(path, operation_id, decided_at)?.lifecycle)
}

/// Persist a stop intent and report whether this invocation acquired the
/// durable owner for provider cancellation. The owner is intentionally
/// returned only for a newly accepted stop; terminal and duplicate requests
/// are idempotent no-ops and must not touch a later turn's provider control.
pub fn request_session_stop_outcome(
    path: &Utf8Path,
    operation_id: &str,
    decided_at: &str,
) -> Result<AcpStopRequestOutcome> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    let mut value = if let Some(value) = admission_metadata_base(path)? {
        value
    } else {
        serde_json::json!({
            "availability": "unavailable",
            "latestTurnStatus": "none",
            "createdAt": decided_at,
        })
    };
    let current = lifecycle_header_from_value(&value);
    if lifecycle_is_terminal(&current)
        || lifecycle_is_stopping(&current)
        || (current.live_turn_activity == AcpLiveTurnActivity::Idle
            && current.latest_turn_status == AcpLatestTurnStatus::None)
    {
        return Ok(AcpStopRequestOutcome {
            lifecycle: current,
            owner: None,
        });
    }
    let accepted = reduce_lifecycle_header(
        &value,
        current,
        AcpLifecycleTransition::StopRequested { operation_id },
    )?;
    apply_lifecycle_header(&mut value, &accepted);
    value["updatedAt"] = Value::String(decided_at.to_string());
    write_json(path, &value)?;
    let owner = accepted
        .turn_id
        .clone()
        .zip(accepted.operation_id.clone())
        .map(|(turn_id, operation_id)| AcpLifecycleOwner {
            turn_id,
            operation_id,
            revision: accepted.revision,
        });
    Ok(AcpStopRequestOutcome {
        lifecycle: accepted,
        owner,
    })
}

pub fn persist_session_terminal(
    path: &Utf8Path,
    latest_turn_status: AcpLatestTurnStatus,
    stop_reason: &str,
    decided_at: &str,
) -> Result<AcpLifecycleHeader> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    let mut value = if let Some(value) = admission_metadata_base(path)? {
        value
    } else {
        serde_json::json!({
            "availability": "unavailable",
            "latestTurnStatus": "none",
            "createdAt": decided_at,
        })
    };
    let current = lifecycle_header_from_value(&value);
    if lifecycle_is_terminal(&current) {
        return Ok(current);
    }
    let terminal = reduce_lifecycle_header(
        &value,
        current,
        AcpLifecycleTransition::TurnSettled {
            status: latest_turn_status,
            reason: stop_reason,
        },
    )?;
    apply_lifecycle_header(&mut value, &terminal);
    value["updatedAt"] = Value::String(decided_at.to_string());
    write_json(path, &value)?;
    clear_session_turn_active(path, terminal.turn_id.as_deref());
    Ok(terminal)
}

pub fn read_lifecycle_header(path: &Utf8Path) -> Result<Option<AcpLifecycleHeader>> {
    if !path.exists() {
        return Ok(None);
    }
    let _ = reconcile_orphaned_session_turn(path)?;
    let _guard = session_metadata_lock(path).lock().unwrap();
    Ok(Some(lifecycle_header_from_value(&read_json::<Value>(
        path,
    )?)))
}

/// Reads the durable lifecycle projection without performing orphan recovery
/// or any migration write. UI/ViewModel queries must use this API so reads
/// cannot race runtime-owned lifecycle settlement.
pub fn read_lifecycle_header_snapshot(path: &Utf8Path) -> Result<Option<AcpLifecycleHeader>> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(lifecycle_header_from_value(&read_json::<Value>(
        path,
    )?)))
}

/// Check whether a previously accepted stop still owns the live cancellation
/// transition before an out-of-lock provider dispatch.
pub fn lifecycle_owner_still_cancelling(
    path: &Utf8Path,
    owner: &AcpLifecycleOwner,
) -> Result<bool> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    if !path.exists() {
        return Ok(false);
    }
    let header = lifecycle_header_from_value(&read_json::<Value>(path)?);
    Ok(header.turn_id.as_deref() == Some(owner.turn_id.as_str())
        && header.operation_id.as_deref() == Some(owner.operation_id.as_str())
        && header.revision == owner.revision
        && lifecycle_is_stopping(&header)
        && header.latest_turn_status == AcpLatestTurnStatus::None)
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
    let _ = reconcile_orphaned_session_turn(path)?;
    let _guard = session_metadata_lock(path).lock().unwrap();
    let mut value: Value = read_json(path)?;
    normalize_loaded_session_metadata(&mut value, established_session_id);
    let before_canonical = value.clone();
    let mut header = lifecycle_header_from_value(&value);
    normalize_lifecycle_header(&value, &mut header);
    apply_lifecycle_header(&mut value, &header);
    if value != before_canonical {
        write_json(path, &value)?;
    }
    Ok(value)
}

/// Read-only variant for ViewModels and status queries. Legacy fields are
/// normalized in memory, but the query never mutates the shared snapshot.
pub fn read_session_metadata_value(
    path: &Utf8Path,
    established_session_id: Option<String>,
) -> Result<Value> {
    let _guard = session_metadata_lock(path).lock().unwrap();
    let mut value: Value = read_json(path)?;
    normalize_loaded_session_metadata(&mut value, established_session_id);
    let mut header = lifecycle_header_from_value(&value);
    normalize_lifecycle_header(&value, &mut header);
    apply_lifecycle_header(&mut value, &header);
    Ok(value)
}

fn normalize_loaded_session_metadata(value: &mut Value, established_session_id: Option<String>) {
    if let Some(legacy_status) = value
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        let normalized = legacy_status.trim().to_ascii_lowercase().replace('_', "-");
        if value.get("sessionId").is_none()
            && let Some(session_id) = established_session_id
        {
            value["sessionId"] = Value::String(session_id);
        }
        value["availability"] = Value::String(
            match normalized.as_str() {
                "cancelling" | "cancel-requested" => "established",
                "closing" => "established",
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
        value["liveTurnActivity"] = Value::String(
            match normalized.as_str() {
                "cancelling" | "cancel-requested" | "closing" => "cancelRequested",
                _ => "idle",
            }
            .to_string(),
        );
        if let Some(object) = value.as_object_mut() {
            object.remove("status");
        }
    }
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
                "completed" => "completed".to_string(),
                _ => "interrupted".to_string(),
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

/// Returns true when an Agent text/thought chunk cannot produce any visible
/// content by itself. The original provider frame remains in `acp.raw.jsonl`;
/// callers use this predicate only to keep placeholder chunks out of canonical
/// output until the same stream accumulates real text.
pub fn is_semantically_empty_agent_content(event: &AcpUiEvent) -> bool {
    if !matches!(event.kind.as_str(), "textDelta" | "thoughtDelta") {
        return false;
    }
    let Some(content) = event.content.as_deref() else {
        return true;
    };
    content.is_empty()
        || content
            .chars()
            .all(|character| character.general_category() == GeneralCategory::Format)
}

const CLAUDE_COMPACTION_STARTED_MESSAGE: &str = "Compacting...";
const CLAUDE_COMPACTION_COMPLETED_MESSAGE: &str = "Compacting completed.";
const PROVIDER_CONTEXT_COMPACTION_META_POINTER: &str = "/_meta/contextCompaction";

/// Normalize provider compaction signals at the ACP boundary so consumers only
/// observe the canonical context-compaction lifecycle. Structured metadata is
/// preferred; Claude-compatible standalone control messages remain a narrow
/// compatibility fallback until ACP standardizes compaction updates.
pub fn context_compaction_phase(update: &Value) -> Option<&'static str> {
    let update_kind = update.get("sessionUpdate").and_then(Value::as_str)?;
    if matches!(update_kind, "tool_call" | "tool_call_update")
        && update
            .pointer(PROVIDER_CONTEXT_COMPACTION_META_POINTER)
            .is_some_and(Value::is_object)
    {
        return match extract_status(update)?.to_ascii_lowercase().as_str() {
            "in_progress" => Some("started"),
            "completed" | "success" | "succeeded" => Some("completed"),
            "failed" | "error" | "cancelled" | "canceled" => Some("interrupted"),
            _ => None,
        };
    }
    if update_kind != "agent_message_chunk" {
        return None;
    }
    match extract_text(update)?.trim() {
        CLAUDE_COMPACTION_STARTED_MESSAGE => Some("started"),
        CLAUDE_COMPACTION_COMPLETED_MESSAGE => Some("completed"),
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

pub fn scheduled_trigger_event(seq: u64, payload: &ScheduledTriggerPayload) -> AcpUiEvent {
    AcpUiEvent {
        id: format!("scheduled-trigger:{}", payload.occurrence_id),
        seq,
        timestamp: payload.accepted_at.clone(),
        kind: "scheduledTrigger".to_string(),
        session_id: None,
        content: None,
        title: None,
        tool_call_id: None,
        status: Some("completed".to_string()),
        started_seq: Some(seq),
        ended_seq: Some(seq),
        started_at: Some(payload.accepted_at.clone()),
        ended_at: Some(payload.accepted_at.clone()),
        timing: None,
        raw: Some(serde_json::json!({
            "source": "goldBandScheduledTrigger",
            "scheduledTrigger": payload,
        })),
    }
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
        AcpLatestTurnStatus, AcpLiveTurnActivity, AcpPromptSubmission, AcpSessionAvailability,
        AcpSessionMetadata, AcpTimingState, AcpTurnAdmission, AcpUiEvent, ScheduledTriggerPayload,
        agent_transcript_tool_output, annotate_latest_runtime_control_output, append_raw_frame,
        append_structured_diagnostic, append_timeline_patch, begin_session_turn,
        cancel_latest_processing_prompt_retry, compact_live_conversation_event,
        context_compaction_phase, elicitation_request_event, elicitation_response_event,
        extract_usage_fields, inspect_session_turn, is_semantically_empty_agent_content,
        kind_to_ui_kind, latest_timeline_source_seq, load_session_metadata, load_timeline_items,
        normalize_session_update, permission_request_event, scheduled_trigger_event,
        user_prompt_event, user_prompt_event_with_quotes, write_timeline_items,
    };
    use crate::provider::UserPromptQuote;
    use crate::storage::{read_json, write_json};
    use camino::Utf8PathBuf;
    use serde_json::{Value, json};

    fn scheduled_trigger_payload(
        trigger_kind: crate::scheduler::occurrence::OccurrenceTriggerKind,
    ) -> ScheduledTriggerPayload {
        ScheduledTriggerPayload {
            project_id: "project-001".to_string(),
            scheduled_task_id: "scheduled-task-001".to_string(),
            occurrence_id: "occurrence-001".to_string(),
            trigger_kind: trigger_kind.clone(),
            scheduled_at: (trigger_kind
                == crate::scheduler::occurrence::OccurrenceTriggerKind::Scheduled)
                .then(|| "2026-08-25T01:30:00Z".to_string()),
            accepted_at: "2026-08-25T01:29:59Z".to_string(),
            instruction_summary: "检查主分支状态".to_string(),
            content_fingerprint: "sha256:accepted".to_string(),
            links: crate::scheduler::occurrence::OccurrenceLinks {
                task_id: Some("task-001".to_string()),
                run_id: Some("run-001".to_string()),
                round_id: Some("round-001".to_string()),
                node_id: Some("dev".to_string()),
                attempt_id: Some("attempt-001".to_string()),
            },
        }
    }

    #[test]
    fn scheduled_trigger_event_has_deterministic_occurrence_identity() {
        let payload = scheduled_trigger_payload(
            crate::scheduler::occurrence::OccurrenceTriggerKind::Scheduled,
        );

        let first = scheduled_trigger_event(10, &payload);
        let retry = scheduled_trigger_event(20, &payload);

        assert_eq!(first.id, "scheduled-trigger:occurrence-001");
        assert_eq!(retry.id, first.id);
        assert_eq!(first.kind, "scheduledTrigger");
        assert!(first.content.is_none());
        assert_eq!(
            first.raw.as_ref().unwrap()["scheduledTrigger"],
            json!(payload)
        );
        assert_ne!(first.seq, retry.seq);
    }

    #[test]
    fn automatic_and_manual_trigger_events_have_distinct_kinds() {
        let automatic = scheduled_trigger_event(
            1,
            &scheduled_trigger_payload(
                crate::scheduler::occurrence::OccurrenceTriggerKind::Scheduled,
            ),
        );
        let manual = scheduled_trigger_event(
            2,
            &scheduled_trigger_payload(crate::scheduler::occurrence::OccurrenceTriggerKind::Manual),
        );

        assert_eq!(
            automatic.raw.as_ref().unwrap()["scheduledTrigger"]["triggerKind"],
            "scheduled"
        );
        assert_eq!(
            manual.raw.as_ref().unwrap()["scheduledTrigger"]["triggerKind"],
            "manual"
        );
        assert!(automatic.raw.as_ref().unwrap()["scheduledTrigger"]["scheduledAt"].is_string());
        assert!(manual.raw.as_ref().unwrap()["scheduledTrigger"]["scheduledAt"].is_null());
    }

    #[test]
    fn metadata_patch_preserves_the_latest_canonical_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        write_json(
            &path,
            &json!({
                "sessionId": "session-1",
                "availability": "established",
                "liveTurnActivity": "running",
                "latestTurnStatus": "none",
                "acpRevision": 7,
                "turnId": "turn-1",
                "lifecycleOperationId": "operation-1"
            }),
        )
        .unwrap();

        let patched = super::patch_session_metadata(&path, |value| {
            value["modelOverride"] = json!("model-new");
            value["liveTurnActivity"] = json!("idle");
            value["latestTurnStatus"] = json!("completed");
            value["acpRevision"] = json!(2);
            Ok(())
        })
        .unwrap();

        assert_eq!(patched["modelOverride"], "model-new");
        assert_eq!(patched["liveTurnActivity"], "running");
        assert_eq!(patched["latestTurnStatus"], "none");
        assert_eq!(patched["acpRevision"], 7);
        assert_eq!(patched["turnId"], "turn-1");
    }

    #[test]
    fn read_only_metadata_normalization_never_writes_the_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        write_json(
            &path,
            &json!({
                "sessionId": "session-1",
                "status": "completed",
                "createdAt": "1Z"
            }),
        )
        .unwrap();
        let before = std::fs::read(path.as_std_path()).unwrap();

        let projected = super::read_session_metadata_value(&path, None).unwrap();

        assert_eq!(projected["latestTurnStatus"], "completed");
        assert!(projected.get("status").is_none());
        assert_eq!(std::fs::read(path.as_std_path()).unwrap(), before);
    }

    #[test]
    fn timing_state_snapshot_round_trips_exact_wait_accumulator() {
        let mut state = AcpTimingState::default();
        state.observe_event(&super::user_prompt_event(
            1,
            "session".to_string(),
            "hello".to_string(),
            Some("prompt".to_string()),
            false,
            Vec::new(),
        ));
        let pending =
            super::permission_request_event(2, "request-1".to_string(), json!({"kind": "allow"}));
        state.observe_event(&pending);
        let snapshot = state.state_snapshot();
        let restored = AcpTimingState::from_state_snapshot(snapshot.clone());
        assert_eq!(restored.state_snapshot(), snapshot);
        assert_eq!(
            restored.snapshot_at_with_revision(true, Some(10), Some(3), None),
            state.snapshot_at_with_revision(true, Some(10), Some(3), None)
        );
    }

    #[test]
    fn terminal_lifecycle_dominates_a_late_stop_accepted_write_for_the_same_turn() {
        let current = json!({
            "acpRevision": 8,
            "turnId": "turn-1",
            "lifecycleOperationId": "operation-1",
            "promptEventId": "prompt-event-1",
            "availability": "established",
            "liveTurnActivity": "idle",
            "latestTurnStatus": "cancelled",
            "stopReason": "cancelled"
        });
        let mut late_accepted = json!({
            "acpRevision": 7,
            "turnId": "turn-1",
            "lifecycleOperationId": "operation-1",
            "promptEventId": "prompt-event-1",
            "availability": "closing",
            "liveTurnActivity": "cancelRequested",
            "latestTurnStatus": "none"
        });

        super::merge_session_lifecycle(Some(&current), &mut late_accepted);

        let header = super::lifecycle_header_from_value(&late_accepted);
        assert_eq!(header.revision, 8);
        assert_eq!(header.live_turn_activity, AcpLiveTurnActivity::Idle);
        assert_eq!(header.latest_turn_status, AcpLatestTurnStatus::Cancelled);
        assert_eq!(header.stop_reason.as_deref(), Some("cancelled"));
    }

    #[test]
    fn terminal_lifecycle_advances_the_stop_accepted_revision() {
        let current = json!({
            "acpRevision": 11,
            "turnId": "turn-1",
            "lifecycleOperationId": "stop-operation-1",
            "availability": "closing",
            "liveTurnActivity": "cancelRequested",
            "latestTurnStatus": "none"
        });
        let mut terminal = json!({
            "acpRevision": 11,
            "turnId": "turn-1",
            "lifecycleOperationId": "stop-operation-1",
            "availability": "established",
            "liveTurnActivity": "idle",
            "latestTurnStatus": "cancelled",
            "stopReason": "cancelled"
        });

        super::merge_session_lifecycle(Some(&current), &mut terminal);

        let header = super::lifecycle_header_from_value(&terminal);
        assert_eq!(header.revision, 12);
        assert_eq!(header.latest_turn_status, AcpLatestTurnStatus::Cancelled);
    }

    #[test]
    fn owned_running_metadata_keeps_the_claim_generation() {
        let current = json!({
            "acpRevision": 7,
            "turnId": "turn-1",
            "lifecycleOperationId": "operation-1",
            "availability": "established",
            "liveTurnActivity": "accepted",
            "latestTurnStatus": "none"
        });
        let mut running = json!({
            "acpRevision": 7,
            "turnId": "turn-1",
            "lifecycleOperationId": "operation-1",
            "availability": "established",
            "liveTurnActivity": "running",
            "latestTurnStatus": "none"
        });

        super::merge_session_lifecycle(Some(&current), &mut running);

        let header = super::lifecycle_header_from_value(&running);
        assert_eq!(header.revision, 7);
        assert_eq!(header.live_turn_activity, AcpLiveTurnActivity::Running);
    }

    #[test]
    fn cancel_requested_lifecycle_dominates_late_running_for_the_same_turn() {
        let current = json!({
            "acpRevision": 5,
            "turnId": "turn-1",
            "lifecycleOperationId": "operation-1",
            "availability": "closing",
            "liveTurnActivity": "cancelRequested",
            "latestTurnStatus": "none"
        });
        let mut late_running = json!({
            "acpRevision": 5,
            "turnId": "turn-1",
            "lifecycleOperationId": "operation-1",
            "availability": "established",
            "liveTurnActivity": "running",
            "latestTurnStatus": "none"
        });

        super::merge_session_lifecycle(Some(&current), &mut late_running);

        let header = super::lifecycle_header_from_value(&late_running);
        assert_eq!(header.revision, 5);
        assert_eq!(
            header.live_turn_activity,
            AcpLiveTurnActivity::CancelRequested
        );
    }

    #[test]
    fn prompt_admission_and_terminal_settlement_are_scoped_to_turn_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission =
            |turn_id: &str, operation_id: &str, admitted_at: &str| AcpPromptSubmission {
                turn_id: turn_id.to_string(),
                operation_id: operation_id.to_string(),
                adapter_id: "codex-acp".to_string(),
                adapter_display_name: "Codex".to_string(),
                cwd: "C:/tmp/attempt".to_string(),
                input: crate::provider::ConversationPromptInput {
                    display_text: format!("message for {turn_id}"),
                    quotes: Vec::new(),
                },
                attachment_paths: vec![format!("{turn_id}.txt")],
                admitted_at: admitted_at.to_string(),
            };

        let first = submission("turn-a", "operation-a", "2026-08-19T10:00:00Z");
        let AcpTurnAdmission::Started(started) = begin_session_turn(&path, &first).unwrap() else {
            panic!("first admission must start the turn");
        };
        assert_eq!(started.revision, 1);
        assert_eq!(started.turn_id.as_deref(), Some("turn-a"));
        assert_eq!(started.live_turn_activity, AcpLiveTurnActivity::Starting);
        assert_eq!(
            super::read_session_prompt_submission(&path, "turn-a").unwrap(),
            Some(first.clone()),
        );

        let duplicate_submission = AcpPromptSubmission {
            operation_id: "operation-duplicate".to_string(),
            admitted_at: "2026-08-19T10:00:01Z".to_string(),
            ..first.clone()
        };
        let AcpTurnAdmission::ExistingActive(duplicate) =
            begin_session_turn(&path, &duplicate_submission).unwrap()
        else {
            panic!("duplicate active admission must be classified without starting");
        };
        assert_eq!(duplicate.revision, 1);
        assert_eq!(duplicate.operation_id.as_deref(), Some("operation-a"));
        let mut conflicting = first.clone();
        conflicting.input.display_text = "different payload".to_string();
        assert!(
            begin_session_turn(&path, &conflicting)
                .unwrap_err()
                .to_string()
                .starts_with("acp.prompt-submission-conflict")
        );

        assert!(
            begin_session_turn(
                &path,
                &submission("turn-b", "operation-b", "2026-08-19T10:00:02Z"),
            )
            .unwrap_err()
            .to_string()
            .starts_with("acp.prompt-session-busy")
        );

        assert!(
            super::persist_session_turn_terminal_owned(
                &path,
                "turn-stale",
                Some("operation-a"),
                started.revision,
                AcpLatestTurnStatus::Failed,
                "provider-error",
                "2026-08-19T10:00:03Z",
            )
            .unwrap()
            .is_none()
        );

        let terminal = super::persist_session_turn_terminal_owned(
            &path,
            "turn-a",
            Some("operation-a"),
            started.revision,
            AcpLatestTurnStatus::Cancelled,
            "cancelled",
            "2026-08-19T10:00:04Z",
        )
        .unwrap()
        .unwrap();
        assert_eq!(terminal.revision, 2);
        assert_eq!(terminal.latest_turn_status, AcpLatestTurnStatus::Cancelled);
        assert_eq!(terminal.live_turn_activity, AcpLiveTurnActivity::Idle);
        assert!(matches!(
            begin_session_turn(&path, &duplicate_submission).unwrap(),
            AcpTurnAdmission::ExistingTerminal(_)
        ));

        let AcpTurnAdmission::Started(next) = begin_session_turn(
            &path,
            &submission("turn-b", "operation-b", "2026-08-19T10:00:05Z"),
        )
        .unwrap() else {
            panic!("terminal predecessor must allow a new turn");
        };
        assert_eq!(next.revision, 3);
        assert_eq!(next.turn_id.as_deref(), Some("turn-b"));

        assert!(
            super::persist_session_turn_terminal_owned(
                &path,
                "turn-a",
                Some("operation-a"),
                started.revision,
                AcpLatestTurnStatus::Failed,
                "late-provider-error",
                "2026-08-19T10:00:06Z",
            )
            .unwrap()
            .is_none()
        );
        let current = super::read_lifecycle_header(&path).unwrap().unwrap();
        assert_eq!(current.revision, 3);
        assert_eq!(current.turn_id.as_deref(), Some("turn-b"));
        assert_eq!(current.live_turn_activity, AcpLiveTurnActivity::Starting);
    }

    #[test]
    fn orphaned_durable_submission_converges_after_process_state_is_lost() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-orphan".to_string(),
            operation_id: "operation-orphan".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "survive restart".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: vec!["evidence.txt".to_string()],
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        assert!(matches!(
            begin_session_turn(&path, &submission).unwrap(),
            AcpTurnAdmission::Started(_)
        ));
        super::clear_session_turn_active(&path, Some("turn-orphan"));

        assert!(matches!(
            inspect_session_turn(&path, &submission).unwrap(),
            Some(AcpTurnAdmission::ExistingTerminal(_))
        ));

        let header = super::read_lifecycle_header(&path).unwrap().unwrap();

        assert_eq!(header.live_turn_activity, AcpLiveTurnActivity::Idle);
        assert_eq!(header.latest_turn_status, AcpLatestTurnStatus::Failed);
        assert_eq!(header.stop_reason.as_deref(), Some("process-interrupted"));
        assert_eq!(
            super::read_session_prompt_submission(&path, "turn-orphan").unwrap(),
            Some(submission),
        );
    }

    #[test]
    fn orphaned_durable_stop_converges_to_cancelled() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-stopped-orphan".to_string(),
            operation_id: "operation-stopped-orphan".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "stop before provider startup".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        begin_session_turn(&path, &submission).unwrap();
        super::request_session_stop(&path, "stop-operation", "2026-08-19T10:00:01Z").unwrap();
        super::clear_session_turn_active(&path, Some(&submission.turn_id));

        assert!(matches!(
            inspect_session_turn(&path, &submission).unwrap(),
            Some(AcpTurnAdmission::ExistingTerminal(_))
        ));
        let header = super::read_lifecycle_header(&path).unwrap().unwrap();
        assert_eq!(header.live_turn_activity, AcpLiveTurnActivity::Idle);
        assert_eq!(header.latest_turn_status, AcpLatestTurnStatus::Cancelled);
        assert_eq!(header.stop_reason.as_deref(), Some("cancelled"));
    }

    #[test]
    fn stop_preserves_session_availability_and_duplicate_operation() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        write_json(
            &path,
            &json!({
                "sessionId": "provider-session",
                "availability": "established",
                "liveTurnActivity": "running",
                "latestTurnStatus": "none"
            }),
        )
        .unwrap();

        let accepted =
            super::request_session_stop(&path, "stop-1", "2026-08-19T10:00:00Z").unwrap();
        assert_eq!(accepted.availability, AcpSessionAvailability::Established);
        assert_eq!(
            accepted.live_turn_activity,
            AcpLiveTurnActivity::CancelRequested
        );
        assert_eq!(accepted.revision, 1);

        let duplicate =
            super::request_session_stop(&path, "stop-2", "2026-08-19T10:00:01Z").unwrap();
        assert_eq!(duplicate.operation_id, accepted.operation_id);
        assert_eq!(duplicate.revision, accepted.revision);
        let duplicate_outcome =
            super::request_session_stop_outcome(&path, "stop-3", "2026-08-19T10:00:01Z").unwrap();
        assert!(duplicate_outcome.owner.is_none());

        let terminal = super::persist_session_terminal(
            &path,
            AcpLatestTurnStatus::Cancelled,
            "cancelled",
            "2026-08-19T10:00:02Z",
        )
        .unwrap();
        assert_eq!(terminal.availability, AcpSessionAvailability::Established);
        assert_eq!(terminal.live_turn_activity, AcpLiveTurnActivity::Idle);
        assert_eq!(terminal.latest_turn_status, AcpLatestTurnStatus::Cancelled);
    }

    #[test]
    fn terminal_stop_is_noop_and_cannot_claim_a_later_turn() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let first = AcpPromptSubmission {
            turn_id: "turn-1".to_string(),
            operation_id: "operation-1".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "first".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        begin_session_turn(&path, &first).unwrap();
        let accepted =
            super::request_session_stop_outcome(&path, "stop-1", "2026-08-19T10:00:01Z").unwrap();
        let old_owner = accepted.owner.clone().unwrap();
        super::persist_session_terminal(
            &path,
            AcpLatestTurnStatus::Cancelled,
            "cancelled",
            "2026-08-19T10:00:02Z",
        )
        .unwrap();

        let second = AcpPromptSubmission {
            turn_id: "turn-2".to_string(),
            operation_id: "operation-2".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "second".to_string(),
                quotes: Vec::new(),
            },
            admitted_at: "2026-08-19T10:00:03Z".to_string(),
            ..first
        };
        begin_session_turn(&path, &second).unwrap();
        assert!(!super::lifecycle_owner_still_cancelling(&path, &old_owner).unwrap());
    }

    #[test]
    fn idle_session_stop_does_not_create_a_synthetic_turn() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        write_json(
            &path,
            &json!({
                "sessionId": "provider-session",
                "availability": "established",
                "liveTurnActivity": "idle",
                "latestTurnStatus": "none"
            }),
        )
        .unwrap();

        let outcome =
            super::request_session_stop_outcome(&path, "stop-idle", "2026-08-19T10:00:00Z")
                .unwrap();
        assert!(outcome.owner.is_none());
        assert_eq!(
            outcome.lifecycle.live_turn_activity,
            AcpLiveTurnActivity::Idle
        );
        assert_eq!(
            outcome.lifecycle.latest_turn_status,
            AcpLatestTurnStatus::None
        );
        assert!(outcome.lifecycle.turn_id.is_none());
    }

    #[test]
    fn prompt_admission_migrates_legacy_session_metadata_to_the_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let session_path = attempt_dir.join("acp.session.json");
        let snapshot_path = attempt_dir.join("acp.snapshot.json");
        write_json(
            &session_path,
            &json!({
                "adapterId": "codex-acp",
                "adapterDisplayName": "Codex",
                "cwd": "C:/tmp/attempt",
                "sessionId": "legacy-session",
                "availability": "established",
                "liveTurnActivity": "idle",
                "latestTurnStatus": "completed",
                "restored": true,
                "capabilities": {},
                "createdAt": "2026-08-19T09:00:00Z",
                "updatedAt": "2026-08-19T09:00:00Z"
            }),
        )
        .unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-migrated".to_string(),
            operation_id: "operation-migrated".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "continue legacy session".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };

        assert!(matches!(
            begin_session_turn(&snapshot_path, &submission).unwrap(),
            AcpTurnAdmission::Started(_)
        ));

        let snapshot: Value = read_json(&snapshot_path).unwrap();
        assert_eq!(
            snapshot.get("sessionId").and_then(Value::as_str),
            Some("legacy-session")
        );
        assert_eq!(
            snapshot
                .get("promptSubmission")
                .and_then(|value| value.get("turnId"))
                .and_then(Value::as_str),
            Some("turn-migrated")
        );
        assert_eq!(
            read_json::<Value>(&session_path)
                .unwrap()
                .get("liveTurnActivity")
                .and_then(Value::as_str),
            Some("idle")
        );
    }

    #[test]
    fn active_turn_identity_is_shared_by_session_and_snapshot_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let session_path = attempt_dir.join("acp.session.json");
        let snapshot_path = attempt_dir.join("acp.snapshot.json");
        let submission = AcpPromptSubmission {
            turn_id: "turn-shared".to_string(),
            operation_id: "operation-shared".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: attempt_dir.to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "shared lifecycle".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        assert!(matches!(
            begin_session_turn(&session_path, &submission).unwrap(),
            AcpTurnAdmission::Started(_)
        ));
        let snapshot: Value = read_json(&session_path).unwrap();
        write_json(&snapshot_path, &snapshot).unwrap();

        assert!(
            super::reconcile_orphaned_session_turn(&snapshot_path)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            super::read_lifecycle_header(&snapshot_path)
                .unwrap()
                .unwrap()
                .live_turn_activity,
            AcpLiveTurnActivity::Starting
        );
        super::clear_session_turn_active(&snapshot_path, Some("turn-shared"));
    }

    #[test]
    fn execution_claim_requires_admission_identity_and_is_single_use() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-claim".to_string(),
            operation_id: "operation-claim".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "claim me".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        let AcpTurnAdmission::Started(started) = begin_session_turn(&path, &submission).unwrap()
        else {
            panic!("claim test admission must start");
        };
        assert!(matches!(
            super::claim_session_turn_for_execution(&path, "turn-claim", started.revision, "",)
                .unwrap(),
            super::AcpTurnExecutionClaim::Stale
        ));
        let claimed = match super::claim_session_turn_for_execution(
            &path,
            "turn-claim",
            started.revision,
            "operation-claim",
        )
        .unwrap()
        {
            super::AcpTurnExecutionClaim::Claimed(header) => header,
            claim => panic!("expected ownership claim, got {claim:?}"),
        };
        assert!(matches!(
            super::claim_session_turn_for_execution(
                &path,
                "turn-claim",
                started.revision,
                "operation-claim",
            )
            .unwrap(),
            super::AcpTurnExecutionClaim::Stale
        ));
        assert!(matches!(
            super::persist_session_turn_terminal_owned(
                &path,
                "turn-claim",
                Some("operation-claim"),
                started.revision,
                AcpLatestTurnStatus::Failed,
                "provider-error",
                "2026-08-19T10:00:01Z",
            )
            .unwrap(),
            None
        ));
        assert!(matches!(
            super::persist_session_turn_terminal_owned(
                &path,
                "turn-claim",
                Some("operation-claim"),
                claimed.revision,
                AcpLatestTurnStatus::Failed,
                "provider-error",
                "2026-08-19T10:00:01Z",
            )
            .unwrap(),
            Some(_)
        ));
        assert!(matches!(
            super::persist_session_turn_terminal_owned(
                &path,
                "turn-claim",
                Some("operation-claim"),
                claimed.revision,
                AcpLatestTurnStatus::Failed,
                "late-provider-error",
                "2026-08-19T10:00:02Z",
            )
            .unwrap(),
            None
        ));
    }

    #[test]
    fn owned_metadata_write_advances_terminal_and_rejects_reuse() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-owned".to_string(),
            operation_id: "operation-owned".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "run owned".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        let AcpTurnAdmission::Started(started) = begin_session_turn(&path, &submission).unwrap()
        else {
            panic!("owned metadata admission must start");
        };
        let owner = match super::claim_session_turn_for_execution(
            &path,
            &submission.turn_id,
            started.revision,
            &submission.operation_id,
        )
        .unwrap()
        {
            super::AcpTurnExecutionClaim::Claimed(owner) => owner,
            claim => panic!("expected ownership claim, got {claim:?}"),
        };

        let mut running = load_session_metadata(&path, None).unwrap();
        running.availability = AcpSessionAvailability::Established;
        running.live_turn_activity = AcpLiveTurnActivity::Running;
        running.latest_turn_status = AcpLatestTurnStatus::None;
        let running_header = super::write_session_metadata_owned(&path, &running, &owner)
            .unwrap()
            .expect("owner should write running metadata");
        assert_eq!(running_header.revision, owner.revision);
        assert_eq!(
            running_header.live_turn_activity,
            AcpLiveTurnActivity::Running
        );

        let mut completed = running;
        completed.live_turn_activity = AcpLiveTurnActivity::Idle;
        completed.latest_turn_status = AcpLatestTurnStatus::Completed;
        let terminal = super::write_session_metadata_owned(&path, &completed, &owner)
            .unwrap()
            .expect("owner should settle terminal metadata");
        assert_eq!(terminal.revision, owner.revision + 1);
        assert_eq!(terminal.latest_turn_status, AcpLatestTurnStatus::Completed);
        assert!(
            super::write_session_metadata_owned(&path, &completed, &owner)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn stop_takeover_rejects_old_provider_metadata_owner() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-stop".to_string(),
            operation_id: "operation-provider".to_string(),
            adapter_id: "codex-acp".to_string(),
            adapter_display_name: "Codex".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "stop me".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-19T10:00:00Z".to_string(),
        };
        let AcpTurnAdmission::Started(started) = begin_session_turn(&path, &submission).unwrap()
        else {
            panic!("stop takeover admission must start");
        };
        let owner = match super::claim_session_turn_for_execution(
            &path,
            &submission.turn_id,
            started.revision,
            &submission.operation_id,
        )
        .unwrap()
        {
            super::AcpTurnExecutionClaim::Claimed(owner) => owner,
            claim => panic!("expected ownership claim, got {claim:?}"),
        };
        let stopped =
            super::request_session_stop(&path, "operation-stop", "2026-08-19T10:00:01Z").unwrap();

        let mut stale = load_session_metadata(&path, None).unwrap();
        stale.availability = AcpSessionAvailability::Established;
        stale.live_turn_activity = AcpLiveTurnActivity::Idle;
        stale.latest_turn_status = AcpLatestTurnStatus::Failed;
        assert!(
            super::write_session_metadata_owned(&path, &stale, &owner)
                .unwrap()
                .is_none()
        );
        let current = super::read_lifecycle_header(&path)
            .unwrap()
            .expect("stop lifecycle should remain durable");
        assert_eq!(current.revision, stopped.revision);
        assert_eq!(current.operation_id.as_deref(), Some("operation-stop"));
        assert_eq!(
            current.live_turn_activity,
            AcpLiveTurnActivity::CancelRequested
        );
        assert_eq!(current.latest_turn_status, AcpLatestTurnStatus::None);
        let terminal = super::persist_session_turn_terminal_owned(
            &path,
            current.turn_id.as_deref().unwrap(),
            current.operation_id.as_deref(),
            current.revision,
            AcpLatestTurnStatus::Cancelled,
            "cancelled",
            "2026-08-19T10:00:02Z",
        )
        .unwrap()
        .expect("stop owner should settle the cancelled terminal");
        assert_eq!(terminal.revision, stopped.revision + 1);
        assert_eq!(terminal.latest_turn_status, AcpLatestTurnStatus::Cancelled);
    }

    #[test]
    fn cancellation_intent_wins_over_a_racing_provider_completion() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        write_json(
            &path,
            &json!({
                "sessionId": "session-active",
                "availability": "established",
                "liveTurnActivity": "running",
                "latestTurnStatus": "none"
            }),
        )
        .unwrap();

        let stopped =
            super::request_session_stop(&path, "stop-operation", "2026-08-19T10:00:01Z").unwrap();
        let terminal = super::persist_session_turn_terminal_owned(
            &path,
            stopped.turn_id.as_deref().unwrap(),
            stopped.operation_id.as_deref(),
            stopped.revision,
            AcpLatestTurnStatus::Completed,
            "provider-completed",
            "2026-08-19T10:00:02Z",
        )
        .unwrap()
        .expect("the stop owner must settle its turn");

        assert_eq!(terminal.live_turn_activity, AcpLiveTurnActivity::Idle);
        assert_eq!(terminal.latest_turn_status, AcpLatestTurnStatus::Cancelled);
        assert_eq!(terminal.stop_reason.as_deref(), Some("cancelled"));
        assert_eq!(terminal.availability, AcpSessionAvailability::Established);
    }

    #[test]
    fn metadata_without_identity_cannot_reset_new_turn_lifecycle() {
        let current = json!({
            "acpRevision": 4,
            "turnId": "turn-new",
            "lifecycleOperationId": "operation-new",
            "liveTurnActivity": "starting",
            "latestTurnStatus": "none"
        });
        let mut stale = json!({
            "availability": "established",
            "liveTurnActivity": "idle",
            "latestTurnStatus": "completed"
        });
        super::merge_session_lifecycle(Some(&current), &mut stale);
        let header = super::lifecycle_header_from_value(&stale);
        assert_eq!(header.turn_id.as_deref(), Some("turn-new"));
        assert_eq!(header.live_turn_activity, AcpLiveTurnActivity::Starting);
        assert_eq!(header.latest_turn_status, AcpLatestTurnStatus::None);
    }

    #[test]
    fn first_provider_metadata_write_keeps_explicit_launch_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-new".to_string(),
            operation_id: "operation-new".to_string(),
            adapter_id: "claude-acp".to_string(),
            adapter_display_name: "Claude".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "hi".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-21T10:00:00Z".to_string(),
        };
        let AcpTurnAdmission::Started(started) = begin_session_turn(&path, &submission).unwrap()
        else {
            panic!("launch override test admission must start");
        };
        let owner = match super::claim_session_turn_for_execution(
            &path,
            &submission.turn_id,
            started.revision,
            &submission.operation_id,
        )
        .unwrap()
        {
            super::AcpTurnExecutionClaim::Claimed(owner) => owner,
            claim => panic!("expected ownership claim, got {claim:?}"),
        };
        let mut running = load_session_metadata(&path, None).unwrap();
        running.session_id = Some("session-new".to_string());
        running.availability = AcpSessionAvailability::Established;
        running.live_turn_activity = AcpLiveTurnActivity::Running;
        running.model_override = Some("sonnet".to_string());
        running.permission_mode_override = Some("default".to_string());
        running.config_option_overrides =
            std::collections::BTreeMap::from([("effort".to_string(), "high".to_string())]);

        super::write_session_metadata_owned(&path, &running, &owner)
            .unwrap()
            .expect("first provider metadata write must keep its owner");
        let persisted = read_json::<Value>(&path).unwrap();

        assert_eq!(persisted["modelOverride"], "sonnet");
        assert_eq!(persisted["permissionModeOverride"], "default");
        assert_eq!(persisted["configOptionOverrides"]["effort"], "high");
    }

    #[test]
    fn established_session_keeps_command_owned_override_clear() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("acp.snapshot.json")).unwrap();
        let submission = AcpPromptSubmission {
            turn_id: "turn-existing".to_string(),
            operation_id: "operation-existing".to_string(),
            adapter_id: "claude-acp".to_string(),
            adapter_display_name: "Claude".to_string(),
            cwd: "C:/tmp/attempt".to_string(),
            input: crate::provider::ConversationPromptInput {
                display_text: "follow up".to_string(),
                quotes: Vec::new(),
            },
            attachment_paths: Vec::new(),
            admitted_at: "2026-08-21T10:00:00Z".to_string(),
        };
        let AcpTurnAdmission::Started(started) = begin_session_turn(&path, &submission).unwrap()
        else {
            panic!("command clear test admission must start");
        };
        let owner = match super::claim_session_turn_for_execution(
            &path,
            &submission.turn_id,
            started.revision,
            &submission.operation_id,
        )
        .unwrap()
        {
            super::AcpTurnExecutionClaim::Claimed(owner) => owner,
            claim => panic!("expected ownership claim, got {claim:?}"),
        };
        let mut stale_provider = load_session_metadata(&path, None).unwrap();
        stale_provider.session_id = Some("session-existing".to_string());
        stale_provider.availability = AcpSessionAvailability::Established;
        stale_provider.live_turn_activity = AcpLiveTurnActivity::Running;
        stale_provider.model_override = Some("stale-model".to_string());
        stale_provider.permission_mode_override = Some("stale-mode".to_string());
        stale_provider.config_option_overrides =
            std::collections::BTreeMap::from([("effort".to_string(), "stale".to_string())]);
        let mut command_owned = read_json::<Value>(&path).unwrap();
        command_owned["sessionId"] = json!("session-existing");
        if let Some(object) = command_owned.as_object_mut() {
            object.remove("modelOverride");
            object.remove("permissionModeOverride");
            object.remove("configOptionOverrides");
        }
        write_json(&path, &command_owned).unwrap();

        super::write_session_metadata_owned(&path, &stale_provider, &owner)
            .unwrap()
            .expect("same owner provider write must merge command state");
        let persisted = read_json::<Value>(&path).unwrap();

        assert!(persisted.get("modelOverride").is_none());
        assert!(persisted.get("permissionModeOverride").is_none());
        assert!(persisted.get("configOptionOverrides").is_none());
    }

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
    fn semantic_empty_agent_content_recognizes_empty_and_unicode_format_chunks() {
        for update in [
            json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "\u{200b}" }
            }),
            json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": { "type": "text", "text": "" }
            }),
        ] {
            let event = normalize_session_update(1, Some("session-1".to_string()), &update);
            assert!(is_semantically_empty_agent_content(&event));
        }
    }

    #[test]
    fn semantic_empty_agent_content_preserves_whitespace_and_visible_text() {
        for text in [" ", "he\u{200b}llo"] {
            let update = json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": text }
            });
            let event = normalize_session_update(1, Some("session-1".to_string()), &update);
            assert!(!is_semantically_empty_agent_content(&event));
            assert_eq!(event.content.as_deref(), Some(text));
        }
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
    fn normalizes_structured_tool_compaction_as_the_same_typed_lifecycle() {
        let started_update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "compact-1",
            "kind": "think",
            "title": "Compact conversation",
            "status": "in_progress",
            "_meta": {"contextCompaction": {"version": 1}},
        });
        let completed_update = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "compact-1",
            "title": "Compact conversation",
            "status": "completed",
            "_meta": {"contextCompaction": {"version": 1}},
        });

        assert_eq!(context_compaction_phase(&started_update), Some("started"));
        assert_eq!(
            context_compaction_phase(&completed_update),
            Some("completed")
        );

        let started = normalize_session_update(10, Some("session-1".to_string()), &started_update);
        assert_eq!(started.kind, "contextCompaction");
        assert_eq!(started.status.as_deref(), Some("running"));
        assert_eq!(started.tool_call_id.as_deref(), Some("compact-1"));
        assert_eq!(started.content, None);

        let completed =
            normalize_session_update(20, Some("session-1".to_string()), &completed_update);
        assert_eq!(completed.kind, "contextCompaction");
        assert_eq!(completed.status.as_deref(), Some("completed"));
        assert_eq!(completed.tool_call_id.as_deref(), Some("compact-1"));
        assert_eq!(completed.content, None);
    }

    #[test]
    fn does_not_infer_compaction_from_a_tool_title_without_structured_metadata() {
        let update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "ordinary-tool",
            "title": "Compact conversation",
            "status": "in_progress",
        });

        assert_eq!(context_compaction_phase(&update), None);
        let event = normalize_session_update(11, None, &update);
        assert_eq!(event.kind, "toolCall");
        assert_eq!(event.status.as_deref(), Some("in_progress"));
    }

    #[test]
    fn maps_structured_failed_compaction_to_interrupted() {
        let update = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "compact-1",
            "status": "failed",
            "_meta": {"contextCompaction": {"version": 1}},
        });

        assert_eq!(context_compaction_phase(&update), Some("interrupted"));
        let event = normalize_session_update(21, None, &update);
        assert_eq!(event.kind, "contextCompaction");
        assert_eq!(event.status.as_deref(), Some("interrupted"));
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
    fn attempt_metrics_use_attempt_totals_and_preserve_unknowns() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "inputTokens":100,"totalTokens":100,
            "attemptInputTokens":260,"attemptTotalTokens":300,
            "timing":{"sessionElapsedSeconds":12,"paused":false}
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        let metrics = super::read_attempt_metrics(&session_path);
        assert_eq!(metrics.input_tokens, Some(260));
        assert_eq!(metrics.output_tokens, None);
        assert_eq!(metrics.cache_read_tokens, None);
        assert_eq!(metrics.total_tokens, Some(300));
        assert_eq!(metrics.elapsed_ms, Some(12_000));

        let missing_dir = TempDir::new().unwrap();
        let missing = super::read_attempt_metrics(
            &camino::Utf8Path::from_path(missing_dir.path())
                .unwrap()
                .join("acp.session.json"),
        );
        assert_eq!(missing.output_tokens, None);
    }

    #[test]
    fn read_attempt_session_model_falls_back_to_config_current_value() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "configOptions":[{"id":"model","currentValue":"config-model"}],
            "models":{"currentModelId":"models-model"}
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        assert_eq!(
            super::read_attempt_session_model(&session_path).as_deref(),
            Some("config-model")
        );

        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "modelOverride":"override-model",
            "configOptions":[{"id":"model","currentValue":"config-model"}]
        }"#,
        )
        .unwrap();
        assert_eq!(
            super::read_attempt_session_model(&session_path).as_deref(),
            Some("override-model")
        );
    }

    #[test]
    fn read_attempt_session_model_name_resolves_config_option_display_name() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "modelOverride":"opus",
            "configOptions":[
                {
                    "id":"model",
                    "currentValue":"opus",
                    "options":[
                        {"value":"default","name":"Default (recommended)","description":"Use the default model (currently glm-5.1[1m])"},
                        {"value":"opus","name":"glm-5.2"}
                    ]
                }
            ]
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        assert_eq!(
            super::read_attempt_session_model(&session_path).as_deref(),
            Some("opus")
        );
        assert_eq!(
            super::read_attempt_session_model_name(&session_path).as_deref(),
            Some("glm-5.2")
        );
    }

    #[test]
    fn read_attempt_session_model_name_parses_default_from_description() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("acp.snapshot.json"),
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "configOptions":[
                {
                    "id":"model",
                    "currentValue":"default",
                    "options":[
                        {"value":"default","name":"Default (recommended)","description":"Use the default model (currently deepseek-v4-pro[1m])"}
                    ]
                }
            ]
        }"#,
        )
        .unwrap();
        let session_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("acp.session.json");
        assert_eq!(
            super::read_attempt_session_model_name(&session_path).as_deref(),
            Some("deepseek-v4-pro")
        );
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
