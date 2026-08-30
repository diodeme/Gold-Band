use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::events::{
    AcpTimelineItem, AcpTimelinePatch, AcpTimingPatch, AcpTimingStateSnapshot, AcpUiEvent,
    extract_agent_transcript_relation, extract_usage_fields,
    load_timeline_items_for_storage_unlocked, merge_timeline_item_revision,
    normalize_timeline_items_for_storage,
};
use crate::acp::turn_files::{FileVersionRef, TurnFileCaptureConfig, TurnFileStore};
use crate::artifacts::JsonArtifactSpan;
use crate::storage::{
    append_jsonl_flushed_unlocked, append_jsonl_lines_flushed_unlocked, atomic_write_file,
    ensure_parent_dir, with_jsonl_file_lock,
};

pub const DEFAULT_TIMELINE_COMPACT_MAX_SIZE_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_TIMELINE_COMPACT_PATCH_RATIO: usize = 4;
pub const DEFAULT_TIMELINE_COMPACT_MIN_PATCH_COUNT: usize = 4 * 1024;
pub const TIMELINE_BLOB_MIN_BYTES: usize = 64 * 1024;
// V10 removes runtime-control artifact inference from the generic timeline
// index. Artifact selection belongs to Runtime output evaluation and the
// selected source is annotated by its canonical branch and item identity.
// V9 guarantees that each indexed locator points at a canonical full item, so
// compaction can read only the latest locators instead of replaying every patch.
// V8 keeps Agent launch links as standalone semantic blocks and recognizes
// their canonical Gold Band conversation identity. V7 indexes grouped these
// links into ordinary activity when provider-only metadata was absent.
// V7 adds the durable bounded runtime projection. Index-hit restore can now
// select active streams, tools, compaction, and provider replay identities
// without scanning, grouping, or sorting every historical locator.
// V6 adds the durable Gold Band prompt identity to locators. Without it,
// usage repair would confuse the synthetic timeline item id with the logical
// turn id stored in prompt metadata. V5 adds provider replay identities to locators and the explicit timing
// fallback projection. Treating an older index as compatible would make
// provider replay suppression and legacy timing recovery depend on full event
// body reads.
// V4 adds the retry-prompt role and its current pending identity. Treating a V2
// index as compatible would leave stop unable to settle a processing retry in
// the crash window between the timeline append and session metadata rewrite.
pub const TIMELINE_INDEX_FORMAT_VERSION: u32 = 10;
pub const DEFAULT_TIMELINE_CHECKPOINT_PATCH_INTERVAL: usize = 256;
pub const DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT: usize = 256;
// Internal result marker: tail replay exceeded its bound and the index was
// rebuilt from the full timeline. Callers that expose diagnostics translate
// this marker into `FullRebuild` and do not report it as processed tail data.
const FULL_REBUILD_RESULT_MARKER: usize = usize::MAX;
const TIMELINE_BLOB_REF_KEY: &str = "$goldBandBlob";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimelineRestoreMode {
    #[default]
    IndexHit,
    TailReplay,
    FullRebuild,
}

impl TimelineRestoreMode {
    fn rank(self) -> u8 {
        match self {
            Self::IndexHit => 0,
            Self::TailReplay => 1,
            Self::FullRebuild => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::IndexHit => "index-hit",
            Self::TailReplay => "tail-replay",
            Self::FullRebuild => "full-rebuild",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineCheckpointPolicy {
    pub patch_interval: usize,
    pub tail_replay_limit: usize,
}

impl Default for TimelineCheckpointPolicy {
    fn default() -> Self {
        Self {
            patch_interval: DEFAULT_TIMELINE_CHECKPOINT_PATCH_INTERVAL,
            tail_replay_limit: DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineUsageProjection {
    pub used: Option<u64>,
    pub size: Option<u64>,
    pub cost_amount_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineItemLocator {
    offset: u64,
    line_length: u64,
    revision: u64,
    fingerprint: u64,
    seq: u64,
    started_seq: u64,
    ended_seq: u64,
    timestamp: String,
    started_at: Option<String>,
    ended_at: Option<String>,
    kind: String,
    status: Option<String>,
    title: Option<String>,
    tool_call_id: Option<String>,
    semantic_kind: TimelineSemanticKind,
    hidden_from_chat: bool,
    session_timeline_event: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    elicitation_id: Option<String>,
    #[serde(default)]
    elicitation_response: bool,
    #[serde(default)]
    read_files: Vec<String>,
    #[serde(default)]
    written_files: Vec<String>,
    #[serde(default)]
    agent_launch: bool,
    #[serde(default)]
    agent_prompt: bool,
    #[serde(default)]
    agent_result: bool,
    #[serde(default)]
    retry_prompt: bool,
    #[serde(default)]
    branch_id: String,
    #[serde(default)]
    gold_band_prompt: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_history_item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum TimelineSemanticKind {
    #[default]
    None,
    Standalone,
    Activity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineSemanticBlockIndex {
    activity: bool,
    item_ids: Vec<String>,
    oldest_seq: u64,
    newest_seq: u64,
    last_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<AcpUiEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRestoreStreamSlot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current: Option<(String, bool)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suspended_stable: Option<String>,
}

impl RuntimeRestoreStreamSlot {
    fn restore(&mut self, item_id: &str, stable: bool) {
        if stable {
            self.current = Some((item_id.to_string(), true));
            self.suspended_stable = None;
            return;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|(_, current_stable)| *current_stable)
        {
            self.suspended_stable = self.current.as_ref().map(|(id, _)| id.clone());
        }
        self.current = Some((item_id.to_string(), false));
    }

    fn close_anonymous(&mut self) {
        if self.current.as_ref().is_some_and(|(_, stable)| !*stable) {
            self.current = None;
        }
    }

    fn clear(&mut self) {
        self.current = None;
        self.suspended_stable = None;
    }

    fn selected_ids(&self) -> impl Iterator<Item = &str> {
        self.current
            .as_ref()
            .map(|(id, _)| id.as_str())
            .into_iter()
            .chain(self.suspended_stable.as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeProjectionOrder {
    ended_seq: u64,
    seq: u64,
    revision: u64,
}

impl From<&TimelineItemLocator> for RuntimeProjectionOrder {
    fn from(locator: &TimelineItemLocator) -> Self {
        Self {
            ended_seq: locator.ended_seq,
            seq: locator.seq,
            revision: locator.revision,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRestoreBranchProjection {
    #[serde(default)]
    text: RuntimeRestoreStreamSlot,
    #[serde(default)]
    thought: RuntimeRestoreStreamSlot,
    #[serde(default)]
    plan: RuntimeRestoreStreamSlot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    watermark: Option<RuntimeProjectionOrder>,
}

impl RuntimeRestoreBranchProjection {
    fn slots(&self) -> [&RuntimeRestoreStreamSlot; 3] {
        [&self.text, &self.thought, &self.plan]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineRuntimeProjection {
    latest_seq: u64,
    #[serde(default)]
    active_tool_item_ids: HashSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_context_compaction_id: Option<String>,
    #[serde(default)]
    streams_by_branch: HashMap<String, RuntimeRestoreBranchProjection>,
    /// Reference counts preserve membership when more than one local item
    /// points at the same stable provider history identity.
    #[serde(default)]
    provider_history_identity_counts: HashMap<String, u32>,
}

impl TimelineRuntimeProjection {
    fn contains_provider_history_identity(&self, identity: &str) -> bool {
        self.provider_history_identity_counts
            .get(identity)
            .is_some_and(|count| *count > 0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineMaterializedIndex {
    format_version: u32,
    generation: u64,
    covered_offset: u64,
    covered_revision: u64,
    covered_prefix_fingerprint: u64,
    observed_length: u64,
    event_count: usize,
    #[serde(default)]
    patch_count: usize,
    #[serde(default)]
    patch_bytes: u64,
    #[serde(default)]
    item_locators: HashMap<String, TimelineItemLocator>,
    #[serde(default)]
    semantic_blocks: Vec<TimelineSemanticBlockIndex>,
    #[serde(default)]
    pending_permissions: HashMap<String, AcpUiEvent>,
    #[serde(default)]
    pending_elicitations: HashMap<String, AcpUiEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    available_commands: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<TimelineUsageProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timing: Option<AcpTimingPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_plan: Option<AcpUiEvent>,
    #[serde(default)]
    agent_launches: HashMap<String, AcpUiEvent>,
    #[serde(default)]
    accepted_prompt_ids: HashSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_retry_prompt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timing_state_snapshot: Option<AcpTimingStateSnapshot>,
    #[serde(default)]
    runtime_projection: TimelineRuntimeProjection,
}

impl Default for TimelineMaterializedIndex {
    fn default() -> Self {
        Self {
            format_version: TIMELINE_INDEX_FORMAT_VERSION,
            generation: 1,
            covered_offset: 0,
            covered_revision: 0,
            covered_prefix_fingerprint: 0,
            observed_length: 0,
            event_count: 0,
            patch_count: 0,
            patch_bytes: 0,
            item_locators: HashMap::new(),
            semantic_blocks: Vec::new(),
            pending_permissions: HashMap::new(),
            pending_elicitations: HashMap::new(),
            available_commands: None,
            usage: None,
            timing: None,
            latest_plan: None,
            agent_launches: HashMap::new(),
            accepted_prompt_ids: HashSet::new(),
            pending_retry_prompt_id: None,
            timing_state_snapshot: None,
            runtime_projection: TimelineRuntimeProjection::default(),
        }
    }
}

/// Bounded runtime state reconstructed from the materialized index. Event
/// bodies returned here still contain blob references; callers must opt into
/// a locator read for the small hot set and must never hydrate the full log.
#[derive(Debug, Clone, Default)]
pub struct TimelineRuntimeRestore {
    pub restore_mode: TimelineRestoreMode,
    pub covered_revision: u64,
    pub latest_seq: u64,
    pub processed_tail_records: usize,
    pub locator_reads: usize,
    pub index_bytes: u64,
    pub index_locator_count: usize,
    /// Index-hit restore must remain zero here. Full locator scans are allowed
    /// only while rebuilding or migrating the materialized index.
    pub projection_locator_scans: usize,
    pub pending_permissions: Vec<AcpUiEvent>,
    pub pending_elicitations: Vec<AcpUiEvent>,
    pub pending_retry_prompt: Option<AcpUiEvent>,
    pub prompt_anchors: Vec<AcpUiEvent>,
    pub hot_items: Vec<AcpUiEvent>,
    pub active_stream_items: Vec<AcpUiEvent>,
    pub active_context_compaction: Option<AcpUiEvent>,
    pub timing_state_snapshot: Option<AcpTimingStateSnapshot>,
}

impl TimelineRuntimeRestore {
    pub fn merge(&mut self, mut other: Self) {
        if other.restore_mode.rank() > self.restore_mode.rank() {
            self.restore_mode = other.restore_mode;
        }
        self.covered_revision = self.covered_revision.max(other.covered_revision);
        self.latest_seq = self.latest_seq.max(other.latest_seq);
        self.processed_tail_records = self
            .processed_tail_records
            .saturating_add(other.processed_tail_records);
        self.index_bytes = self.index_bytes.saturating_add(other.index_bytes);
        self.index_locator_count = self
            .index_locator_count
            .saturating_add(other.index_locator_count);
        self.projection_locator_scans = self
            .projection_locator_scans
            .saturating_add(other.projection_locator_scans);
        merge_runtime_events(
            &mut self.pending_permissions,
            other.pending_permissions.drain(..),
        );
        merge_runtime_events(
            &mut self.pending_elicitations,
            other.pending_elicitations.drain(..),
        );
        merge_runtime_events(&mut self.prompt_anchors, other.prompt_anchors.drain(..));
        merge_runtime_events(&mut self.hot_items, other.hot_items.drain(..));
        merge_runtime_events(
            &mut self.active_stream_items,
            other.active_stream_items.drain(..),
        );
        merge_latest_runtime_event(&mut self.pending_retry_prompt, other.pending_retry_prompt);
        merge_latest_runtime_event(
            &mut self.active_context_compaction,
            other.active_context_compaction,
        );
        let replace_timing = match (
            self.timing_state_snapshot.as_ref(),
            other.timing_state_snapshot.as_ref(),
        ) {
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (Some(current), Some(candidate)) => {
                candidate.revision.unwrap_or(self.covered_revision)
                    > current.revision.unwrap_or_default()
            }
            (None, None) => false,
        };
        if replace_timing {
            self.timing_state_snapshot = other.timing_state_snapshot;
        }
    }
}

fn runtime_event_key(event: &AcpUiEvent) -> String {
    let branch_id = event
        .raw
        .as_ref()
        .and_then(|raw| raw.pointer("/_meta/goldBandConversation/branchId"))
        .and_then(Value::as_str)
        .filter(|branch_id| !branch_id.trim().is_empty())
        .unwrap_or("root");
    format!("{branch_id}:{}", event.id)
}

fn runtime_event_order(event: &AcpUiEvent) -> (u64, u64) {
    (event.ended_seq.unwrap_or(event.seq), event.seq)
}

fn merge_runtime_events(
    target: &mut Vec<AcpUiEvent>,
    incoming: impl IntoIterator<Item = AcpUiEvent>,
) {
    let mut by_key = target
        .drain(..)
        .map(|event| (runtime_event_key(&event), event))
        .collect::<HashMap<_, _>>();
    for event in incoming {
        let key = runtime_event_key(&event);
        if by_key
            .get(&key)
            .is_none_or(|current| runtime_event_order(&event) >= runtime_event_order(current))
        {
            by_key.insert(key, event);
        }
    }
    let mut values = by_key.into_values().collect::<Vec<_>>();
    values.sort_by_key(runtime_event_order);
    target.extend(values);
}

fn merge_latest_runtime_event(target: &mut Option<AcpUiEvent>, incoming: Option<AcpUiEvent>) {
    let Some(incoming) = incoming else { return };
    if target
        .as_ref()
        .is_none_or(|current| runtime_event_order(&incoming) >= runtime_event_order(current))
    {
        *target = Some(incoming);
    }
}

#[derive(Debug, Clone)]
pub struct TimelineIndexedPage {
    pub generation: u64,
    pub covered_revision: u64,
    pub newest_revision: Option<u64>,
    pub processed_tail_records: usize,
    pub events: Vec<AcpUiEvent>,
    pub loaded_semantic_blocks: usize,
    pub total_semantic_blocks: usize,
    pub oldest_seq: Option<u64>,
    pub newest_seq: Option<u64>,
    pub has_older: bool,
    pub has_newer: bool,
    pub event_count: usize,
    pub pending_permissions: Vec<AcpUiEvent>,
    pub pending_elicitations: Vec<AcpUiEvent>,
    pub available_commands: Option<Vec<Value>>,
    pub usage: Option<TimelineUsageProjection>,
    pub timing: Option<AcpTimingPatch>,
    pub latest_plan: Option<AcpUiEvent>,
}

#[derive(Debug, Clone)]
pub struct TimelineIndexedItem {
    pub event: AcpUiEvent,
    pub generation: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemIdentity {
    pub branch_id: String,
    pub item_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub struct TimelineBranchProjection {
    pub generation: u64,
    pub covered_revision: u64,
    pub processed_tail_records: usize,
    pub execution_event_count: usize,
    pub tool_call_count: usize,
    pub read_file_count: usize,
    pub written_file_count: usize,
    pub has_pending_interaction: bool,
    pub latest_seq: Option<u64>,
    pub latest_timestamp: Option<String>,
    pub has_completion_evidence: bool,
    pub latest_plan_entries: Vec<Value>,
    pub agent_launches: Vec<AcpUiEvent>,
    pub prompt_turns: Vec<TimelinePromptTurnProjection>,
}

/// Lightweight prompt-turn boundaries derived from the canonical root
/// timeline index. Agent projections use these boundaries to keep historical
/// executions attached to the turn that launched them without loading prompt
/// bodies or introducing a second lifecycle state store.
#[derive(Debug, Clone)]
pub struct TimelinePromptTurnProjection {
    pub started_seq: u64,
    pub started_at: String,
    pub terminal_seq: Option<u64>,
    pub terminal_at: Option<String>,
    pub terminal_status: Option<String>,
}

fn prompt_terminal_status(status: Option<&str>, ended_at: Option<&str>) -> Option<String> {
    status.map(str::to_ascii_lowercase).filter(|status| {
        ended_at.is_some()
            && matches!(
                status.as_str(),
                "completed"
                    | "success"
                    | "succeeded"
                    | "failed"
                    | "error"
                    | "cancelled"
                    | "canceled"
                    | "interrupted"
            )
    })
}

#[cfg(test)]
pub(crate) fn prompt_turn_projection(event: &AcpUiEvent) -> Option<TimelinePromptTurnProjection> {
    let is_prompt = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("source"))
        .and_then(Value::as_str)
        == Some("goldBandPrompt");
    if !is_prompt {
        return None;
    }
    let terminal_status =
        prompt_terminal_status(event.status.as_deref(), event.ended_at.as_deref());
    Some(TimelinePromptTurnProjection {
        started_seq: event.started_seq.unwrap_or(event.seq),
        started_at: event
            .started_at
            .clone()
            .unwrap_or_else(|| event.timestamp.clone()),
        terminal_seq: terminal_status
            .as_ref()
            .map(|_| event.ended_seq.unwrap_or(event.seq)),
        terminal_at: terminal_status
            .as_ref()
            .and_then(|_| event.ended_at.clone()),
        terminal_status,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineSettleOutcome {
    Applied,
    AlreadyTerminal,
    RevisionConflict,
    IdentityMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineCompactionPolicy {
    pub max_size_bytes: u64,
    pub patch_ratio: usize,
}

impl Default for TimelineCompactionPolicy {
    fn default() -> Self {
        Self {
            max_size_bytes: DEFAULT_TIMELINE_COMPACT_MAX_SIZE_BYTES,
            patch_ratio: DEFAULT_TIMELINE_COMPACT_PATCH_RATIO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineUpsertOutcome {
    Unchanged,
    Appended,
    AppendedAndCompacted,
}

#[derive(Debug)]
pub struct TimelineStore {
    path: Utf8PathBuf,
    policy: TimelineCompactionPolicy,
    patch_count: usize,
    patch_bytes: u64,
    redundant_revision_count: usize,
    file_signature: TimelineFileSignature,
    blob_store: TurnFileStore,
    index: TimelineMaterializedIndex,
    checkpoint_policy: TimelineCheckpointPolicy,
    dirty_patch_count: usize,
    restore_mode: TimelineRestoreMode,
    last_compaction_elapsed: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineFileSignature {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl TimelineStore {
    pub fn durable_watermark_for_item_id(&self, item_id: &str) -> Option<(u64, u64)> {
        self.index
            .item_locators
            .get(item_id)
            .map(|locator| (self.index.generation, locator.revision))
    }

    pub fn open(path: Utf8PathBuf, policy: TimelineCompactionPolicy) -> Result<Self> {
        let checkpoint_policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(&path);
        let (index, replayed) = with_jsonl_file_lock(&path, || {
            load_or_rebuild_index_unlocked(&path, &index_path, checkpoint_policy)
        })?;
        let restore_mode = timeline_restore_mode(replayed);
        let stats = timeline_file_stats_from_index(&index);
        let file_signature = timeline_file_signature(&path);
        let blob_store = timeline_blob_store(&path);
        let mut store = Self {
            path,
            policy,
            patch_count: stats.patch_count,
            patch_bytes: stats.patch_bytes,
            redundant_revision_count: stats.redundant_revision_count,
            file_signature,
            blob_store,
            index,
            checkpoint_policy,
            dirty_patch_count: if replayed == FULL_REBUILD_RESULT_MARKER {
                0
            } else {
                replayed
            },
            restore_mode,
            last_compaction_elapsed: None,
        };
        if store.dirty_patch_count > 0 {
            store.force_checkpoint()?;
        }
        if store.compact_if_needed()? {
            // Compaction intentionally reads the canonical timeline and
            // rewrites it. Keep startup diagnostics honest about this
            // full-history path even when the preceding index was current.
            store.restore_mode = TimelineRestoreMode::FullRebuild;
        }
        Ok(store)
    }

    /// Builds the bounded runtime projection from the index already loaded by
    /// `open`. Startup callers use this for the root timeline so the index is
    /// not parsed a second time before the provider session reuse decision.
    pub fn runtime_restore(&self) -> Result<TimelineRuntimeRestore> {
        let mut restore =
            runtime_restore_from_index(&self.path, &self.index, 0, self.restore_mode)?;
        restore.index_bytes = std::fs::metadata(timeline_index_path(&self.path))
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        restore.index_locator_count = self.index.item_locators.len();
        Ok(restore)
    }

    pub fn runtime_restore_for_branch(&self, branch_id: &str) -> Result<TimelineRuntimeRestore> {
        let mut restore = self.runtime_restore()?;
        annotate_restore_branch(&mut restore, branch_id);
        Ok(restore)
    }

    pub fn contains_provider_history_identity(&self, identity: &str) -> bool {
        self.index
            .runtime_projection
            .contains_provider_history_identity(identity)
    }

    pub fn upsert(&mut self, revision: u64, item: &AcpUiEvent) -> Result<TimelineUpsertOutcome> {
        self.upsert_batch(&[(revision, item.clone())])
            .map(|mut outcomes| outcomes.pop().unwrap_or(TimelineUpsertOutcome::Unchanged))
    }

    /// Applies a set of distinct timeline identities with one timeline lock,
    /// one append file open, one flush, and at most one checkpoint/compaction.
    /// The returned outcomes are aligned with `updates`.
    pub fn upsert_batch(
        &mut self,
        updates: &[(u64, AcpUiEvent)],
    ) -> Result<Vec<TimelineUpsertOutcome>> {
        self.last_compaction_elapsed = None;
        if updates.is_empty() {
            return Ok(Vec::new());
        }
        let mut identities = HashSet::with_capacity(updates.len());
        let mut storage_updates = Vec::with_capacity(updates.len());
        for (revision, item) in updates {
            ensure!(
                identities.insert(item.id.as_str()),
                "acp.timeline-batch-duplicate-item"
            );
            let mut storage_item = item.clone();
            externalize_timeline_event(&self.blob_store, &mut storage_item)?;
            storage_updates.push((*revision, storage_item));
        }
        let path = self.path.clone();
        let index_path = timeline_index_path(&path);
        let checkpoint_policy = self.checkpoint_policy;
        let mutation = with_jsonl_file_lock(&path, || {
            let current_signature = timeline_file_signature(&path);
            if current_signature != self.file_signature {
                let (index, _) =
                    load_or_rebuild_index_unlocked(&path, &index_path, checkpoint_policy)?;
                self.replace_index_projection(index);
            }
            let mut outcomes = vec![TimelineUpsertOutcome::Unchanged; storage_updates.len()];
            let mut next_offset = timeline_file_len(&path);
            let mut covered_revision = self.index.covered_revision;
            let mut prepared = Vec::with_capacity(storage_updates.len());
            let mut encoded = Vec::with_capacity(storage_updates.len());
            let mut existing_reader = storage_updates
                .iter()
                .any(|(_, item)| self.index.item_locators.contains_key(&item.id))
                .then(|| File::open(path.as_std_path()))
                .transpose()?;
            for (input_index, (requested_revision, storage_item)) in
                storage_updates.iter().enumerate()
            {
                let existing = if let Some(locator) = self.index.item_locators.get(&storage_item.id)
                {
                    Some(read_event_from_file_at_locator(
                        existing_reader
                            .as_mut()
                            .expect("reader exists when an indexed item is present"),
                        locator,
                    )?)
                } else {
                    None
                };
                let canonical_item = existing
                    .as_ref()
                    .map(|existing| merge_timeline_item_revision(existing, storage_item.clone()))
                    .unwrap_or_else(|| storage_item.clone());
                let fingerprint = semantic_fingerprint(&canonical_item)?;
                if self
                    .index
                    .item_locators
                    .get(&storage_item.id)
                    .map(|locator| locator.fingerprint)
                    == Some(fingerprint)
                {
                    continue;
                }

                let revision = (*requested_revision).max(covered_revision.saturating_add(1));
                covered_revision = revision;
                let patch = AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: storage_item.id.clone(),
                    revision,
                    op: "upsert".to_string(),
                    item: canonical_item.clone(),
                };
                let line = serde_json::to_vec(&patch)?;
                let line_length = line.len().saturating_add(1) as u64;
                prepared.push((
                    input_index,
                    revision,
                    canonical_item,
                    next_offset,
                    line_length,
                ));
                encoded.push(line);
                next_offset = next_offset.saturating_add(line_length);
            }

            append_jsonl_lines_flushed_unlocked(&path, &encoded)?;
            let mut appended_bytes = 0u64;
            for (input_index, revision, canonical_item, offset, line_length) in prepared {
                apply_index_event(
                    &mut self.index,
                    &canonical_item,
                    revision,
                    offset,
                    line_length,
                )?;
                self.index.event_count = self.index.event_count.saturating_add(1);
                self.index.patch_count = self.index.patch_count.saturating_add(1);
                self.index.patch_bytes = self.index.patch_bytes.saturating_add(line_length);
                self.dirty_patch_count = self.dirty_patch_count.saturating_add(1);
                appended_bytes = appended_bytes.saturating_add(line_length);
                outcomes[input_index] = TimelineUpsertOutcome::Appended;
            }
            if self.dirty_patch_count >= checkpoint_policy.patch_interval.max(1) {
                checkpoint_index_unlocked(&path, &mut self.index)?;
                self.dirty_patch_count = 0;
            }
            Ok((outcomes, appended_bytes))
        })?;
        let (mut outcomes, appended_bytes) = mutation;
        let appended_count = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, TimelineUpsertOutcome::Appended))
            .count();
        if appended_count == 0 {
            return Ok(outcomes);
        }
        self.patch_count = self.patch_count.saturating_add(appended_count);
        self.patch_bytes = self.patch_bytes.saturating_add(appended_bytes);
        self.file_signature = timeline_file_signature(&self.path);
        let compaction_started_at = Instant::now();
        if self.compact_if_needed()? {
            self.last_compaction_elapsed = Some(compaction_started_at.elapsed());
            for outcome in &mut outcomes {
                if matches!(outcome, TimelineUpsertOutcome::Appended) {
                    *outcome = TimelineUpsertOutcome::AppendedAndCompacted;
                }
            }
        }
        Ok(outcomes)
    }

    pub fn take_last_compaction_elapsed(&mut self) -> Option<Duration> {
        self.last_compaction_elapsed.take()
    }

    pub fn compact_if_needed(&mut self) -> Result<bool> {
        let unique_items = self.index.item_locators.len().max(1);
        let patch_heavy = self.patch_count >= DEFAULT_TIMELINE_COMPACT_MIN_PATCH_COUNT
            && self.patch_count > unique_items.saturating_mul(self.policy.patch_ratio);
        let patch_bytes_heavy = self.patch_bytes > self.policy.max_size_bytes;
        if !patch_bytes_heavy && !patch_heavy && self.redundant_revision_count == 0 {
            return Ok(false);
        }

        let index_path = timeline_index_path(&self.path);
        let (index, compacted) = with_jsonl_file_lock(&self.path, || {
            // Another TimelineStore may have appended or compacted after this
            // instance released its last write lock. Re-read the projection
            // while holding the same timeline lock so compaction never moves
            // generation backwards and two stale writers do not compact the
            // same already-canonical file in succession.
            let (current_index, _) =
                load_or_rebuild_index_unlocked(&self.path, &index_path, self.checkpoint_policy)?;
            let current_stats = timeline_file_stats_from_index(&current_index);
            let current_unique_items = current_index.item_locators.len().max(1);
            let current_patch_heavy = current_stats.patch_count
                >= DEFAULT_TIMELINE_COMPACT_MIN_PATCH_COUNT
                && current_stats.patch_count
                    > current_unique_items.saturating_mul(self.policy.patch_ratio);
            let current_patch_bytes_heavy = current_stats.patch_bytes > self.policy.max_size_bytes;
            if !current_patch_bytes_heavy && !current_patch_heavy {
                return Ok((current_index, false));
            }

            let items =
                load_indexed_timeline_items_for_storage_unlocked(&self.path, &current_index)?;
            write_canonical_timeline_unlocked(&self.path, &self.blob_store, &items)?;
            let mut index = rebuild_index_unlocked(&self.path)?.0;
            index.generation = current_index.generation.saturating_add(1).max(1);
            persist_index_unlocked(&self.path, &index)?;
            Ok((index, true))
        })?;
        if !compacted {
            self.replace_index_projection(index);
            return Ok(false);
        }
        self.patch_count = 0;
        self.patch_bytes = 0;
        self.redundant_revision_count = 0;
        self.file_signature = timeline_file_signature(&self.path);
        self.index = index;
        self.dirty_patch_count = 0;
        Ok(true)
    }

    pub fn force_checkpoint(&mut self) -> Result<()> {
        let path = self.path.clone();
        let index_path = timeline_index_path(&path);
        let checkpoint_policy = self.checkpoint_policy;
        with_jsonl_file_lock(&path, || {
            if timeline_file_signature(&path) != self.file_signature {
                let (index, _) =
                    load_or_rebuild_index_unlocked(&path, &index_path, checkpoint_policy)?;
                self.replace_index_projection(index);
            }
            checkpoint_index_unlocked(&path, &mut self.index)
        })?;
        self.dirty_patch_count = 0;
        self.file_signature = timeline_file_signature(&self.path);
        Ok(())
    }

    fn replace_index_projection(&mut self, index: TimelineMaterializedIndex) {
        let stats = timeline_file_stats_from_index(&index);
        self.patch_count = stats.patch_count;
        self.patch_bytes = stats.patch_bytes;
        self.redundant_revision_count = stats.redundant_revision_count;
        self.index = index;
        self.file_signature = timeline_file_signature(&self.path);
        self.dirty_patch_count = 0;
    }
}

fn checkpoint_index_unlocked(path: &Utf8Path, index: &mut TimelineMaterializedIndex) -> Result<()> {
    rebuild_semantic_blocks(index);
    index.covered_offset = timeline_file_len(path);
    index.observed_length = index.covered_offset;
    index.covered_prefix_fingerprint = timeline_prefix_fingerprint(path, index.covered_offset)?;
    persist_index_unlocked(path, index)
}

fn timeline_index_path(path: &Utf8Path) -> Utf8PathBuf {
    path.with_extension("index.json")
}

fn timeline_file_len(path: &Utf8Path) -> u64 {
    std::fs::metadata(path.as_std_path())
        .map(|metadata| metadata.len())
        .unwrap_or_default()
}

fn timeline_file_stats_from_index(index: &TimelineMaterializedIndex) -> TimelineFileStats {
    TimelineFileStats {
        patch_count: index.patch_count,
        patch_bytes: index.patch_bytes,
        redundant_revision_count: 0,
    }
}

fn timeline_restore_mode(replayed: usize) -> TimelineRestoreMode {
    if replayed == FULL_REBUILD_RESULT_MARKER {
        TimelineRestoreMode::FullRebuild
    } else if replayed > 0 {
        TimelineRestoreMode::TailReplay
    } else {
        TimelineRestoreMode::IndexHit
    }
}

fn load_or_rebuild_index_unlocked(
    timeline_path: &Utf8Path,
    index_path: &Utf8Path,
    policy: TimelineCheckpointPolicy,
) -> Result<(TimelineMaterializedIndex, usize)> {
    let timeline_len = timeline_file_len(timeline_path);
    let existing_index = index_path
        .exists()
        .then(|| crate::storage::read_json::<TimelineMaterializedIndex>(index_path))
        .transpose()
        .ok()
        .flatten();
    if let Some(mut index) = existing_index.clone()
        && index.format_version == TIMELINE_INDEX_FORMAT_VERSION
        && index.covered_offset <= timeline_len
        && timeline_prefix_fingerprint(timeline_path, index.covered_offset)?
            == index.covered_prefix_fingerprint
    {
        let replayed =
            match replay_index_tail_unlocked(timeline_path, &mut index, policy.tail_replay_limit) {
                Ok(replayed) => replayed,
                Err(_) => {
                    let items = load_timeline_items_for_storage_unlocked(timeline_path)?;
                    write_canonical_timeline_unlocked(
                        timeline_path,
                        &timeline_blob_store(timeline_path),
                        &items,
                    )?;
                    let (mut rebuilt, _) = rebuild_index_unlocked(timeline_path)?;
                    rebuilt.generation = index.generation.saturating_add(1).max(1);
                    persist_index_unlocked(timeline_path, &rebuilt)?;
                    return Ok((rebuilt, FULL_REBUILD_RESULT_MARKER));
                }
            };
        if replayed > 0 {
            index.covered_offset = timeline_file_len(timeline_path);
            index.observed_length = index.covered_offset;
            index.covered_prefix_fingerprint =
                timeline_prefix_fingerprint(timeline_path, index.covered_offset)?;
            persist_index_unlocked(timeline_path, &index)?;
        }
        return Ok((index, replayed));
    }
    if timeline_len > 0 {
        let items = load_timeline_items_for_storage_unlocked(timeline_path)?;
        write_canonical_timeline_unlocked(
            timeline_path,
            &timeline_blob_store(timeline_path),
            &items,
        )?;
    }
    let (mut index, _) = rebuild_index_unlocked(timeline_path)?;
    index.generation = existing_index
        .map(|index| index.generation.saturating_add(1).max(1))
        .unwrap_or(1);
    persist_index_unlocked(timeline_path, &index)?;
    // The caller must distinguish an index hit from an index reconstruction;
    // using one result marker avoids a second full index deserialize solely for
    // diagnostics on every continue/follow-up startup.
    Ok((index, FULL_REBUILD_RESULT_MARKER))
}

fn rebuild_index_unlocked(path: &Utf8Path) -> Result<(TimelineMaterializedIndex, usize)> {
    let mut index = TimelineMaterializedIndex::default();
    if !path.exists() {
        return Ok((index, 0));
    }
    let mut reader = BufReader::new(File::open(path.as_std_path())?);
    let mut line = String::new();
    let mut offset = 0u64;
    let mut processed = 0usize;
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        let line_length = bytes as u64;
        if let Some((revision, item, is_patch)) = parse_timeline_record(&line) {
            apply_index_event_deferred(&mut index, &item, revision, offset, line_length)?;
            processed = processed.saturating_add(1);
            if is_patch {
                index.patch_count = index.patch_count.saturating_add(1);
                index.patch_bytes = index.patch_bytes.saturating_add(line_length);
            }
        }
        offset = offset.saturating_add(line_length);
    }
    index.covered_offset = offset;
    index.observed_length = offset;
    index.event_count = processed;
    rebuild_runtime_projection(&mut index);
    rebuild_semantic_blocks(&mut index);
    index.covered_prefix_fingerprint = timeline_prefix_fingerprint(path, offset)?;
    Ok((index, processed))
}

fn replay_index_tail_unlocked(
    path: &Utf8Path,
    index: &mut TimelineMaterializedIndex,
    tail_limit: usize,
) -> Result<usize> {
    let current_len = timeline_file_len(path);
    if current_len == index.covered_offset {
        index.observed_length = current_len;
        return Ok(0);
    }
    let mut file = File::open(path.as_std_path())?;
    file.seek(SeekFrom::Start(index.covered_offset))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut offset = index.covered_offset;
    let mut processed = 0usize;
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        if processed >= tail_limit {
            anyhow::bail!("acp.timeline-index-tail-limit-exceeded");
        }
        let line_length = bytes as u64;
        if let Some((revision, item, is_patch)) = parse_timeline_record(&line) {
            apply_index_event(index, &item, revision, offset, line_length)?;
            processed = processed.saturating_add(1);
            if is_patch {
                index.patch_count = index.patch_count.saturating_add(1);
                index.patch_bytes = index.patch_bytes.saturating_add(line_length);
            }
        }
        offset = offset.saturating_add(line_length);
    }
    index.observed_length = current_len;
    index.event_count = index.event_count.saturating_add(processed);
    rebuild_semantic_blocks(index);
    Ok(processed)
}

fn parse_timeline_record(line: &str) -> Option<(u64, AcpUiEvent, bool)> {
    if line.trim().is_empty() {
        return None;
    }
    if let Ok(patch) = serde_json::from_str::<AcpTimelinePatch>(line) {
        if patch.patch_type == "timelinePatch" && patch.op == "upsert" {
            return Some((patch.revision, patch.item, true));
        }
        return None;
    }
    serde_json::from_str::<AcpTimelineItem>(line)
        .ok()
        .map(|entry| {
            let revision = entry
                .item
                .ended_seq
                .or(entry.item.started_seq)
                .unwrap_or(entry.item.seq);
            (revision, entry.item, false)
        })
}

fn persist_index_unlocked(path: &Utf8Path, index: &TimelineMaterializedIndex) -> Result<()> {
    crate::storage::write_json(&timeline_index_path(path), index)
}

fn timeline_prefix_fingerprint(path: &Utf8Path, covered_offset: u64) -> Result<u64> {
    if covered_offset == 0 || !path.exists() {
        return Ok(0);
    }
    const SAMPLE_BYTES: u64 = 4096;
    let mut file = File::open(path.as_std_path())?;
    let start = covered_offset.saturating_sub(SAMPLE_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut sample = vec![0u8; (covered_offset - start) as usize];
    file.read_exact(&mut sample)?;
    let mut hasher = DefaultHasher::new();
    covered_offset.hash(&mut hasher);
    sample.hash(&mut hasher);
    Ok(hasher.finish())
}

fn apply_index_event(
    index: &mut TimelineMaterializedIndex,
    item: &AcpUiEvent,
    revision: u64,
    offset: u64,
    line_length: u64,
) -> Result<()> {
    apply_index_event_inner(index, item, revision, offset, line_length, true)
}

fn apply_index_event_deferred(
    index: &mut TimelineMaterializedIndex,
    item: &AcpUiEvent,
    revision: u64,
    offset: u64,
    line_length: u64,
) -> Result<()> {
    apply_index_event_inner(index, item, revision, offset, line_length, false)
}

fn apply_index_event_inner(
    index: &mut TimelineMaterializedIndex,
    item: &AcpUiEvent,
    revision: u64,
    offset: u64,
    line_length: u64,
    rebuild_late_runtime_projection: bool,
) -> Result<()> {
    let should_replace = index
        .item_locators
        .get(&item.id)
        .is_none_or(|locator| revision >= locator.revision);
    if !should_replace {
        return Ok(());
    }
    let locator = timeline_item_locator(item, revision, offset, line_length)?;
    let previous_locator = index.item_locators.get(&item.id).cloned();
    index.covered_revision = index.covered_revision.max(revision).max(locator.ended_seq);
    apply_lightweight_projection(index, item);
    let replaced_pending_retry = index.pending_retry_prompt_id.as_deref() == Some(item.id.as_str());
    let processing_retry = locator.retry_prompt && locator.status.as_deref() == Some("processing");
    let candidate_order = (locator.ended_seq, locator.seq, locator.revision);
    index.item_locators.insert(item.id.clone(), locator);
    update_runtime_projection(
        index,
        &item.id,
        previous_locator.as_ref(),
        rebuild_late_runtime_projection,
    );
    if processing_retry {
        let should_replace = index
            .pending_retry_prompt_id
            .as_ref()
            .and_then(|candidate_id| index.item_locators.get(candidate_id))
            .is_none_or(|current| {
                candidate_order >= (current.ended_seq, current.seq, current.revision)
            });
        if should_replace {
            index.pending_retry_prompt_id = Some(item.id.clone());
        }
    } else if replaced_pending_retry {
        // A terminal update for the current retry closes that lifecycle. Do
        // not resurrect an older processing retry from another logical turn.
        index.pending_retry_prompt_id = None;
    }
    Ok(())
}

fn locator_affects_stream_reducer(locator: &TimelineItemLocator) -> (&str, &str, bool) {
    (
        locator.branch_id.as_str(),
        locator.kind.as_str(),
        locator.provider_history_item_id.is_some(),
    )
}

fn update_runtime_projection(
    index: &mut TimelineMaterializedIndex,
    item_id: &str,
    previous: Option<&TimelineItemLocator>,
    rebuild_late_projection: bool,
) {
    let Some(current) = index.item_locators.get(item_id).cloned() else {
        return;
    };
    let current_order = RuntimeProjectionOrder::from(&current);
    let previous_latest_seq = previous.map(|locator| locator.ended_seq.max(locator.seq));
    let current_latest_seq = current.ended_seq.max(current.seq);
    let current_branch_watermark = index
        .runtime_projection
        .streams_by_branch
        .get(&current.branch_id)
        .and_then(|branch| branch.watermark);
    let replacing_stream_semantics = previous.is_some_and(|previous| {
        locator_affects_stream_reducer(previous) != locator_affects_stream_reducer(&current)
    });
    let moved_before_projection_watermark =
        current_branch_watermark.is_some_and(|watermark| current_order < watermark);
    let moved_latest_seq_backwards = previous_latest_seq.is_some_and(|previous_latest_seq| {
        previous_latest_seq == index.runtime_projection.latest_seq
            && current_latest_seq < previous_latest_seq
    });
    let replaced_latest_context_with_older_value = previous.is_some_and(|previous| {
        index
            .runtime_projection
            .latest_context_compaction_id
            .as_deref()
            == Some(item_id)
            && (current.kind != "contextCompaction"
                || RuntimeProjectionOrder::from(&current) < RuntimeProjectionOrder::from(previous))
    });
    let needs_rebuild = replacing_stream_semantics
        || moved_before_projection_watermark
        || moved_latest_seq_backwards
        || replaced_latest_context_with_older_value;
    if needs_rebuild {
        if rebuild_late_projection {
            rebuild_runtime_projection(index);
        }
        return;
    }

    if let Some(previous_identity) =
        previous.and_then(|locator| locator.provider_history_item_id.as_deref())
    {
        decrement_provider_history_identity(
            &mut index.runtime_projection.provider_history_identity_counts,
            previous_identity,
        );
    }
    if let Some(identity) = current.provider_history_item_id.as_deref() {
        *index
            .runtime_projection
            .provider_history_identity_counts
            .entry(identity.to_string())
            .or_default() += 1;
    }

    index
        .runtime_projection
        .active_tool_item_ids
        .remove(item_id);
    if locator_is_active_tool(&current) {
        index
            .runtime_projection
            .active_tool_item_ids
            .insert(item_id.to_string());
    }
    index.runtime_projection.latest_seq =
        index.runtime_projection.latest_seq.max(current_latest_seq);
    if current.kind == "contextCompaction" {
        let should_replace = index
            .runtime_projection
            .latest_context_compaction_id
            .as_ref()
            .and_then(|current_id| index.item_locators.get(current_id))
            .is_none_or(|latest| current_order >= RuntimeProjectionOrder::from(latest));
        if should_replace {
            index.runtime_projection.latest_context_compaction_id = Some(item_id.to_string());
        }
    }
    let branch = index
        .runtime_projection
        .streams_by_branch
        .entry(current.branch_id.clone())
        .or_default();
    reduce_runtime_stream_projection(branch, item_id, &current);
}

fn decrement_provider_history_identity(counts: &mut HashMap<String, u32>, identity: &str) {
    let Some(count) = counts.get_mut(identity) else {
        return;
    };
    *count = count.saturating_sub(1);
    if *count == 0 {
        counts.remove(identity);
    }
}

fn locator_is_active_tool(locator: &TimelineItemLocator) -> bool {
    matches!(locator.kind.as_str(), "toolCall" | "toolCallUpdate")
        && !timeline_tool_is_terminal(locator.status.as_deref())
}

fn reduce_runtime_stream_projection(
    branch: &mut RuntimeRestoreBranchProjection,
    item_id: &str,
    locator: &TimelineItemLocator,
) {
    let stable = locator.provider_history_item_id.is_some();
    match locator.kind.as_str() {
        "textDelta" => {
            branch.text.restore(item_id, stable);
            branch.thought.close_anonymous();
            branch.plan.close_anonymous();
        }
        "thoughtDelta" => {
            branch.text.close_anonymous();
            branch.thought.restore(item_id, stable);
            branch.plan.close_anonymous();
        }
        "plan" => {
            branch.text.close_anonymous();
            branch.thought.close_anonymous();
            branch.plan.restore(item_id, stable);
        }
        "elicitationRequest" => {}
        "userTextDelta" | "contextCompaction" => {
            branch.text.clear();
            branch.thought.clear();
            branch.plan.clear();
        }
        _ => {
            branch.text.close_anonymous();
            branch.thought.close_anonymous();
            branch.plan.close_anonymous();
        }
    }
    branch.watermark = Some(RuntimeProjectionOrder::from(locator));
}

fn rebuild_runtime_projection(index: &mut TimelineMaterializedIndex) {
    let mut projection = TimelineRuntimeProjection::default();
    let mut ordered = Vec::with_capacity(index.item_locators.len());
    for (item_id, locator) in &index.item_locators {
        projection.latest_seq = projection
            .latest_seq
            .max(locator.ended_seq.max(locator.seq));
        if locator_is_active_tool(locator) {
            projection.active_tool_item_ids.insert(item_id.clone());
        }
        if let Some(identity) = locator.provider_history_item_id.as_deref() {
            *projection
                .provider_history_identity_counts
                .entry(identity.to_string())
                .or_default() += 1;
        }
        if locator.kind == "contextCompaction" {
            let order = RuntimeProjectionOrder::from(locator);
            let should_replace = projection
                .latest_context_compaction_id
                .as_ref()
                .and_then(|current_id| index.item_locators.get(current_id))
                .is_none_or(|current| order >= RuntimeProjectionOrder::from(current));
            if should_replace {
                projection.latest_context_compaction_id = Some(item_id.clone());
            }
        }
        ordered.push((item_id, locator));
    }
    ordered.sort_by(|(left_id, left), (right_id, right)| {
        left.branch_id
            .cmp(&right.branch_id)
            .then_with(|| {
                RuntimeProjectionOrder::from(*left).cmp(&RuntimeProjectionOrder::from(*right))
            })
            .then_with(|| left_id.cmp(right_id))
    });
    for (item_id, locator) in ordered {
        let branch = projection
            .streams_by_branch
            .entry(locator.branch_id.clone())
            .or_default();
        reduce_runtime_stream_projection(branch, item_id, locator);
    }
    index.runtime_projection = projection;
}

fn timeline_item_locator(
    item: &AcpUiEvent,
    revision: u64,
    offset: u64,
    line_length: u64,
) -> Result<TimelineItemLocator> {
    let raw = item.raw.as_ref();
    let hidden_from_chat = raw
        .and_then(|raw| raw.get("hiddenFromChat"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let elicitation_id = raw
        .and_then(|raw| raw.get("elicitationId"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            matches!(
                item.kind.as_str(),
                "elicitationRequest" | "elicitationResponse"
            )
            .then(|| item.id.trim_end_matches("-response").to_string())
        });
    let agent_launch = is_agent_launch(item);
    let activity = matches!(
        item.kind.as_str(),
        "thoughtDelta" | "toolCall" | "toolCallUpdate" | "error"
    ) && !agent_launch;
    let standalone = agent_launch
        || matches!(
            item.kind.as_str(),
            "userTextDelta"
                | "textDelta"
                | "fileChangeSet"
                | "attemptSeparator"
                | "contextCompaction"
        )
        || (item.kind == "permissionRequest" && item.status.as_deref() == Some("pending"))
        || (item.kind == "elicitationRequest"
            && item.status.as_deref().unwrap_or("pending") == "pending");
    let semantic_kind = if activity {
        TimelineSemanticKind::Activity
    } else if standalone {
        TimelineSemanticKind::Standalone
    } else {
        TimelineSemanticKind::None
    };
    Ok(TimelineItemLocator {
        offset,
        line_length,
        revision,
        fingerprint: semantic_fingerprint(item)?,
        seq: item.seq,
        started_seq: item.started_seq.unwrap_or(item.seq),
        ended_seq: item.ended_seq.unwrap_or(item.seq),
        timestamp: item.timestamp.clone(),
        started_at: item.started_at.clone(),
        ended_at: item.ended_at.clone(),
        kind: item.kind.clone(),
        status: item.status.clone(),
        title: item.title.clone(),
        tool_call_id: item.tool_call_id.clone(),
        semantic_kind,
        hidden_from_chat,
        session_timeline_event: is_session_timeline_event(item),
        elicitation_id,
        elicitation_response: item.kind == "elicitationResponse",
        read_files: structured_tool_paths(item, true),
        written_files: structured_tool_paths(item, false),
        agent_launch,
        agent_prompt: item
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            == Some("agentBranchPrompt"),
        agent_result: item
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            == Some("agentBranchResult"),
        retry_prompt: item
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/retry/attempt"))
            .and_then(Value::as_u64)
            .is_some_and(|attempt| attempt > 0),
        branch_id: item
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/_meta/goldBandConversation/branchId"))
            .and_then(Value::as_str)
            .filter(|branch_id| !branch_id.trim().is_empty())
            .unwrap_or("root")
            .to_string(),
        gold_band_prompt: item
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            == Some("goldBandPrompt"),
        provider_history_item_id: timeline_provider_history_item_id(item),
        prompt_id: timeline_prompt_id(item),
    })
}

fn is_agent_launch(item: &AcpUiEvent) -> bool {
    item.raw.as_ref().is_some_and(|raw| {
        extract_agent_transcript_relation(raw).is_some_and(|relation| relation.agent_launch)
            || [
                "/_meta/goldBandConversation/launchedAgentExecutionId",
                "/toolCall/_meta/goldBandConversation/launchedAgentExecutionId",
            ]
            .into_iter()
            .filter_map(|pointer| raw.pointer(pointer))
            .filter_map(Value::as_str)
            .any(|value| !value.trim().is_empty())
    })
}

fn timeline_prompt_id(item: &AcpUiEvent) -> Option<String> {
    item.raw
        .as_ref()
        .and_then(|raw| raw.get("promptId"))
        .and_then(Value::as_str)
        .filter(|prompt_id| !prompt_id.trim().is_empty())
        .map(str::to_string)
}

fn timeline_provider_history_item_id(item: &AcpUiEvent) -> Option<String> {
    let raw = item.raw.as_ref()?;
    if let Some(item_id) = raw
        .get("providerHistoryItemId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return Some(item_id.to_string());
    }
    match raw.get("sessionUpdate").and_then(Value::as_str)? {
        "agent_message_chunk" => raw
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("assistant-message-{value}")),
        "agent_thought_chunk" => raw
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("assistant-thought-{value}")),
        "tool_call" | "tool_call_update" => raw
            .get("toolCallId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("tool-call-{value}")),
        "plan" => item
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("session-plan-{value}")),
        _ => None,
    }
}

fn is_session_timeline_event(item: &AcpUiEvent) -> bool {
    if item.kind == "permissionRequest" {
        return item.status.as_deref() == Some("pending");
    }
    if matches!(
        item.kind.as_str(),
        "availableCommands"
            | "usageUpdate"
            | "sessionInfo"
            | "modeUpdate"
            | "configUpdate"
            | "rawDiagnostic"
    ) {
        return false;
    }
    let session_update = item
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionUpdate"))
        .and_then(Value::as_str);
    !matches!(
        session_update,
        Some(
            "user_message_chunk"
                | "available_commands_update"
                | "usage_update"
                | "session_info_update"
                | "current_mode_update"
                | "config_option_update"
        )
    ) || item
        .raw
        .as_ref()
        .is_some_and(|raw| raw.get("source").and_then(Value::as_str) == Some("providerHistory"))
}

fn structured_tool_paths(item: &AcpUiEvent, reads: bool) -> Vec<String> {
    if !matches!(item.kind.as_str(), "toolCall" | "toolCallUpdate") {
        return Vec::new();
    }
    let raw = item.raw.as_ref();
    let tool_name = raw
        .and_then(|raw| raw.pointer("/_meta/goldBandConversation/toolName"))
        .and_then(Value::as_str)
        .or_else(|| {
            item.title
                .as_deref()
                .and_then(|title| title.split_whitespace().next())
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let matches_operation = if reads {
        matches!(tool_name.as_str(), "read" | "get-content" | "read_file")
    } else {
        matches!(
            tool_name.as_str(),
            "write" | "edit" | "applypatch" | "apply_patch" | "set-content" | "write_file"
        )
    };
    if !matches_operation {
        return Vec::new();
    }
    let mut paths = Vec::new();
    let input = raw
        .and_then(|raw| {
            raw.pointer("/toolCall/rawInput")
                .or_else(|| raw.get("rawInput"))
        })
        .and_then(Value::as_object);
    if let Some(input) = input {
        for key in ["file_path", "path"] {
            if let Some(path) = input.get(key).and_then(Value::as_str) {
                paths.push(normalize_metric_path(path));
            }
        }
    }
    let locations = raw
        .and_then(|raw| {
            raw.pointer("/toolCall/locations")
                .or_else(|| raw.get("locations"))
        })
        .and_then(Value::as_array);
    if let Some(locations) = locations {
        for location in locations {
            if let Some(path) = location.get("path").and_then(Value::as_str) {
                paths.push(normalize_metric_path(path));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn normalize_metric_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn parse_legacy_timing_epoch(value: Option<&str>) -> Option<u64> {
    value?.trim_end_matches('Z').parse::<u64>().ok()
}

/// Converts a legacy timing projection into the explicit runtime snapshot
/// shape. The old patch cannot recover the exact accumulated wait duration, so
/// an existing exact snapshot is retained for fields it did not represent.
fn legacy_timing_state_snapshot(
    index: &TimelineMaterializedIndex,
    timing: &AcpTimingPatch,
    previous: Option<&AcpTimingStateSnapshot>,
) -> AcpTimingStateSnapshot {
    let active_turn_started_at =
        parse_legacy_timing_epoch(timing.active_turn_started_at.as_deref());
    let mut pending_permission_ids = index
        .pending_permissions
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    pending_permission_ids.sort();
    let mut pending_elicitation_ids = index
        .pending_elicitations
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    pending_elicitation_ids.sort();
    AcpTimingStateSnapshot {
        elapsed_seconds: timing.session_elapsed_seconds,
        active_turn_started_at,
        active_turn_last_activity_at: parse_legacy_timing_epoch(
            timing.active_turn_last_activity_at.as_deref(),
        ),
        revision: timing
            .revision
            .or_else(|| previous.and_then(|snapshot| snapshot.revision)),
        saw_turn: previous.is_some_and(|snapshot| snapshot.saw_turn)
            || active_turn_started_at.is_some(),
        pending_permission_ids,
        pending_elicitation_ids,
        user_wait_started_at: parse_legacy_timing_epoch(timing.user_wait_started_at.as_deref()),
        user_wait_seconds: previous
            .map(|snapshot| snapshot.user_wait_seconds)
            .unwrap_or_default(),
    }
}

fn apply_lightweight_projection(index: &mut TimelineMaterializedIndex, item: &AcpUiEvent) {
    if let Some(prompt_id) = item
        .raw
        .as_ref()
        .and_then(|raw| raw.get("promptId"))
        .and_then(Value::as_str)
        .filter(|prompt_id| !prompt_id.trim().is_empty())
    {
        index.accepted_prompt_ids.insert(prompt_id.to_string());
    }
    if is_agent_launch(item)
        && let Some(tool_call_id) = item.tool_call_id.as_ref()
    {
        let should_replace = index
            .agent_launches
            .get(tool_call_id)
            .is_none_or(|current| {
                item.ended_seq.unwrap_or(item.seq) >= current.ended_seq.unwrap_or(current.seq)
            });
        if should_replace {
            index
                .agent_launches
                .insert(tool_call_id.clone(), item.clone());
        }
    }
    if item.kind == "permissionRequest" {
        let request_id = item
            .raw
            .as_ref()
            .and_then(|raw| raw.get("requestId"))
            .and_then(Value::as_str)
            .unwrap_or(&item.id)
            .to_string();
        if item.status.as_deref() == Some("pending") {
            index.pending_permissions.insert(request_id, item.clone());
        } else {
            index.pending_permissions.remove(&request_id);
        }
    }
    if item.kind == "elicitationRequest" {
        if item.status.as_deref().unwrap_or("pending") == "pending" {
            index
                .pending_elicitations
                .insert(item.id.clone(), item.clone());
        } else {
            index.pending_elicitations.remove(&item.id);
        }
    } else if item.kind == "elicitationResponse" {
        let id = item
            .raw
            .as_ref()
            .and_then(|raw| raw.get("elicitationId"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| item.id.trim_end_matches("-response").to_string());
        index.pending_elicitations.remove(&id);
    }
    if let Some(raw) = item.raw.as_ref() {
        match raw.get("sessionUpdate").and_then(Value::as_str) {
            Some("available_commands_update") => {
                index.available_commands = raw
                    .get("availableCommands")
                    .and_then(Value::as_array)
                    .cloned();
            }
            Some("usage_update") => {
                let (used, size, cost_amount_usd) = extract_usage_fields(raw);
                let usage = index
                    .usage
                    .get_or_insert_with(TimelineUsageProjection::default);
                if used.is_some() {
                    usage.used = used;
                }
                if size.is_some() {
                    usage.size = size;
                }
                if cost_amount_usd.is_some() {
                    usage.cost_amount_usd = cost_amount_usd;
                }
            }
            _ => {}
        }
    }
    if let Some(timing) = item.timing.as_ref()
        && index.timing.as_ref().is_none_or(|current| {
            timing.revision.unwrap_or_default() >= current.revision.unwrap_or_default()
        })
    {
        index.timing = Some(timing.clone());
        index.timing_state_snapshot = timing.state_snapshot.clone().or_else(|| {
            Some(legacy_timing_state_snapshot(
                index,
                timing,
                index.timing_state_snapshot.as_ref(),
            ))
        });
    }
    if item.kind == "plan"
        && index.latest_plan.as_ref().is_none_or(|current| {
            item.ended_seq.unwrap_or(item.seq) >= current.ended_seq.unwrap_or(current.seq)
        })
    {
        index.latest_plan = Some(item.clone());
    }
}

fn rebuild_semantic_blocks(index: &mut TimelineMaterializedIndex) {
    let resolved_elicitations = index
        .item_locators
        .values()
        .filter(|locator| locator.elicitation_response)
        .filter_map(|locator| locator.elicitation_id.clone())
        .collect::<HashSet<_>>();
    let mut ordered = index.item_locators.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(_, locator)| (locator.started_seq, locator.seq));
    let mut blocks = Vec::<TimelineSemanticBlockIndex>::new();
    for (item_id, locator) in ordered {
        if locator.hidden_from_chat || !locator.session_timeline_event {
            continue;
        }
        if locator.kind == "elicitationRequest"
            && locator
                .elicitation_id
                .as_ref()
                .is_some_and(|id| resolved_elicitations.contains(id))
        {
            continue;
        }
        match locator.semantic_kind {
            TimelineSemanticKind::None => {}
            TimelineSemanticKind::Standalone => blocks.push(TimelineSemanticBlockIndex {
                activity: false,
                item_ids: vec![item_id.clone()],
                oldest_seq: locator.started_seq,
                newest_seq: locator.ended_seq,
                last_revision: locator.revision,
                summary: None,
            }),
            TimelineSemanticKind::Activity => {
                if let Some(block) = blocks.last_mut().filter(|block| block.activity) {
                    block.item_ids.push(item_id.clone());
                    block.oldest_seq = block.oldest_seq.min(locator.started_seq);
                    block.newest_seq = block.newest_seq.max(locator.ended_seq);
                    block.last_revision = block.last_revision.max(locator.revision);
                } else {
                    blocks.push(TimelineSemanticBlockIndex {
                        activity: true,
                        item_ids: vec![item_id.clone()],
                        oldest_seq: locator.started_seq,
                        newest_seq: locator.ended_seq,
                        last_revision: locator.revision,
                        summary: None,
                    });
                }
            }
        }
    }
    for block in blocks.iter_mut().filter(|block| block.activity) {
        block.summary = build_activity_summary(index, block);
    }
    index.semantic_blocks = blocks;
}

fn build_activity_summary(
    index: &TimelineMaterializedIndex,
    block: &TimelineSemanticBlockIndex,
) -> Option<AcpUiEvent> {
    let first = index.item_locators.get(block.item_ids.first()?)?;
    let latest = index.item_locators.get(block.item_ids.last()?)?;
    let mut tool_ids = HashSet::new();
    let mut thought_count = 0usize;
    let mut error_count = 0usize;
    let mut read_files = HashSet::new();
    let mut written_files = HashSet::new();
    for item_id in &block.item_ids {
        let locator = index.item_locators.get(item_id)?;
        match locator.kind.as_str() {
            "thoughtDelta" => thought_count = thought_count.saturating_add(1),
            "error" => error_count = error_count.saturating_add(1),
            "toolCall" | "toolCallUpdate" => {
                tool_ids.insert(
                    locator
                        .tool_call_id
                        .as_deref()
                        .unwrap_or(item_id)
                        .to_string(),
                );
                read_files.extend(locator.read_files.iter().cloned());
                written_files.extend(locator.written_files.iter().cloned());
            }
            _ => {}
        }
    }
    Some(AcpUiEvent {
        id: format!("activity-{}", block.oldest_seq),
        seq: block.oldest_seq,
        timestamp: first.timestamp.clone(),
        kind: "activitySummary".to_string(),
        session_id: None,
        content: None,
        title: latest.title.clone(),
        tool_call_id: None,
        status: latest.status.clone(),
        started_seq: Some(block.oldest_seq),
        ended_seq: Some(block.newest_seq),
        started_at: first
            .started_at
            .clone()
            .or_else(|| Some(first.timestamp.clone())),
        ended_at: latest
            .ended_at
            .clone()
            .or_else(|| Some(latest.timestamp.clone())),
        timing: None,
        raw: Some(serde_json::json!({
            "goldBandActivity": {
                "activityStartSeq": block.oldest_seq,
                "activityEndSeq": block.newest_seq,
                "totalEventCount": block.item_ids.len(),
                "toolCallCount": tool_ids.len(),
                "thoughtCount": thought_count,
                "errorCount": error_count,
                "readFileCount": read_files.len(),
                "writtenFileCount": written_files.len(),
                "detailAvailable": !block.item_ids.is_empty(),
            }
        })),
    })
}

fn read_event_at_locator(path: &Utf8Path, locator: &TimelineItemLocator) -> Result<AcpUiEvent> {
    let mut file = File::open(path.as_std_path())?;
    read_event_from_file_at_locator(&mut file, locator)
}

fn load_indexed_timeline_items_for_storage_unlocked(
    path: &Utf8Path,
    index: &TimelineMaterializedIndex,
) -> Result<Vec<AcpUiEvent>> {
    if index.item_locators.is_empty() {
        return Ok(Vec::new());
    }
    let mut ordered = index.item_locators.iter().collect::<Vec<_>>();
    ordered.sort_by(|(left_id, left), (right_id, right)| {
        (left.started_seq, left.seq, left.revision)
            .cmp(&(right.started_seq, right.seq, right.revision))
            .then_with(|| left_id.cmp(right_id))
    });
    let mut file = File::open(path.as_std_path())?;
    let mut items = Vec::with_capacity(ordered.len());
    for (_, locator) in ordered {
        items.push(read_event_from_file_at_locator(&mut file, locator)?);
    }
    normalize_timeline_items_for_storage(&mut items);
    Ok(items)
}

fn read_event_from_file_at_locator(
    file: &mut File,
    locator: &TimelineItemLocator,
) -> Result<AcpUiEvent> {
    file.seek(SeekFrom::Start(locator.offset))?;
    let mut bytes = vec![0u8; locator.line_length as usize];
    file.read_exact(&mut bytes)?;
    let line = std::str::from_utf8(&bytes)?;
    parse_timeline_record(line)
        .map(|(_, item, _)| item)
        .ok_or_else(|| anyhow::anyhow!("acp.timeline-index-locator-corrupt"))
}

pub fn read_indexed_timeline_page(
    path: &Utf8Path,
    before_seq: Option<u64>,
    after_seq: Option<u64>,
    after_revision: Option<u64>,
    limit: usize,
) -> Result<TimelineIndexedPage> {
    let started_at = Instant::now();
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    let (index, processed_tail_records) = with_jsonl_file_lock(path, || {
        load_or_rebuild_index_unlocked(path, &index_path, policy)
    })?;
    let total = index.semantic_blocks.len();
    let processed_tail_records = if processed_tail_records == FULL_REBUILD_RESULT_MARKER {
        0
    } else {
        processed_tail_records
    };
    let selected = if let Some(cursor) = after_revision {
        let mut changed = index
            .semantic_blocks
            .iter()
            .filter(|block| block.last_revision > cursor)
            .collect::<Vec<_>>();
        changed.sort_by_key(|block| (block.last_revision, block.oldest_seq));
        let boundary_revision = changed
            .get(limit.saturating_sub(1))
            .map(|block| block.last_revision);
        changed
            .into_iter()
            .take_while(|block| {
                boundary_revision.is_none_or(|boundary| block.last_revision <= boundary)
            })
            .collect::<Vec<_>>()
    } else if let Some(cursor) = after_seq {
        let mut changed = index
            .semantic_blocks
            .iter()
            .filter(|block| block.newest_seq > cursor)
            .collect::<Vec<_>>();
        changed.sort_by_key(|block| (block.newest_seq, block.oldest_seq));
        changed.into_iter().take(limit).collect::<Vec<_>>()
    } else if let Some(cursor) = before_seq {
        let candidates = index
            .semantic_blocks
            .iter()
            // Backward pagination follows the stable visual order established
            // by rebuild_semantic_blocks. A cumulative item can start before
            // the cursor and finish much later; its end revision must not move
            // it out of the older page.
            .filter(|block| block.oldest_seq < cursor)
            .collect::<Vec<_>>();
        let candidate_count = candidates.len();
        candidates
            .into_iter()
            .skip(candidate_count.saturating_sub(limit))
            .collect::<Vec<_>>()
    } else {
        index
            .semantic_blocks
            .iter()
            .skip(total.saturating_sub(limit))
            .collect::<Vec<_>>()
    };
    let mut events = Vec::with_capacity(selected.len());
    for block in &selected {
        if let Some(summary) = block.summary.as_ref() {
            events.push(summary.clone());
            continue;
        }
        for item_id in &block.item_ids {
            if let Some(locator) = index.item_locators.get(item_id) {
                events.push(read_event_at_locator(path, locator)?);
            }
        }
    }
    let oldest_seq = selected.first().map(|block| block.oldest_seq);
    let newest_seq = selected.last().map(|block| block.newest_seq);
    let newest_revision = selected.iter().map(|block| block.last_revision).max();
    let first_ordinal = selected.first().and_then(|selected| {
        index
            .semantic_blocks
            .iter()
            .position(|block| std::ptr::eq(block, *selected))
    });
    let last_ordinal = selected.last().and_then(|selected| {
        index
            .semantic_blocks
            .iter()
            .position(|block| std::ptr::eq(block, *selected))
    });
    tracing::info!(
        target: "gold_band::timeline_index",
        timeline_path = %path,
        processed_tail_records,
        page_locator_reads = events.len(),
        total_semantic_blocks = total,
        elapsed_ms = started_at.elapsed().as_millis(),
        "read indexed ACP timeline page"
    );
    Ok(TimelineIndexedPage {
        generation: index.generation,
        covered_revision: index.covered_revision,
        newest_revision,
        processed_tail_records,
        events,
        loaded_semantic_blocks: selected.len(),
        total_semantic_blocks: total,
        oldest_seq,
        newest_seq,
        has_older: first_ordinal.is_some_and(|ordinal| ordinal > 0),
        has_newer: last_ordinal.is_some_and(|ordinal| ordinal + 1 < total),
        event_count: index.event_count,
        pending_permissions: index.pending_permissions.into_values().collect(),
        pending_elicitations: index.pending_elicitations.into_values().collect(),
        available_commands: index.available_commands,
        usage: index.usage,
        timing: index.timing,
        latest_plan: index.latest_plan,
    })
}

pub fn timeline_has_agent_launches(path: &Utf8Path) -> Result<bool> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        Ok(!index.agent_launches.is_empty())
    })
}

pub fn read_indexed_accepted_prompt_ids(path: &Utf8Path) -> Result<HashSet<String>> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        Ok(index.accepted_prompt_ids)
    })
}

/// Reads only the materialized runtime projection and a bounded set of hot
/// locators. This is the startup path for continue/resume; it deliberately
/// does not call `load_timeline_items` and therefore never hydrates timeline
/// Blob values.
pub fn read_indexed_runtime_restore(path: &Utf8Path) -> Result<TimelineRuntimeRestore> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, processed_tail_records) =
            load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let restore_mode = timeline_restore_mode(processed_tail_records);
        let processed_tail_records = if processed_tail_records == FULL_REBUILD_RESULT_MARKER {
            0
        } else {
            processed_tail_records
        };
        let mut restore =
            runtime_restore_from_index(path, &index, processed_tail_records, restore_mode)?;
        restore.index_bytes = std::fs::metadata(&index_path)
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        restore.index_locator_count = index.item_locators.len();
        Ok(restore)
    })
}

/// Same bounded restore as [`read_indexed_runtime_restore`], with the branch
/// path treated as the authoritative identity when an old event body omitted
/// its branch metadata. This keeps cross-branch merges from collapsing equal
/// item IDs without reading any additional timeline records.
pub fn read_indexed_runtime_restore_for_branch(
    path: &Utf8Path,
    branch_id: &str,
) -> Result<TimelineRuntimeRestore> {
    let mut restore = read_indexed_runtime_restore(path)?;
    annotate_restore_branch(&mut restore, branch_id);
    Ok(restore)
}

fn annotate_restore_branch(restore: &mut TimelineRuntimeRestore, branch_id: &str) {
    let branch_id = branch_id.trim();
    if branch_id.is_empty() {
        return;
    }
    for event in restore
        .pending_permissions
        .iter_mut()
        .chain(restore.pending_elicitations.iter_mut())
        .chain(restore.prompt_anchors.iter_mut())
        .chain(restore.hot_items.iter_mut())
        .chain(restore.active_stream_items.iter_mut())
    {
        annotate_event_branch(event, branch_id);
    }
    if let Some(event) = restore.pending_retry_prompt.as_mut() {
        annotate_event_branch(event, branch_id);
    }
    if let Some(event) = restore.active_context_compaction.as_mut() {
        annotate_event_branch(event, branch_id);
    }
}

fn annotate_event_branch(event: &mut AcpUiEvent, branch_id: &str) {
    let raw = event.raw.get_or_insert_with(|| serde_json::json!({}));
    if !raw.is_object() {
        *raw = serde_json::json!({});
    }
    let object = raw.as_object_mut().expect("runtime event raw object");
    let meta = object
        .entry("_meta".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !meta.is_object() {
        *meta = serde_json::json!({});
    }
    let meta_object = meta.as_object_mut().expect("runtime event metadata object");
    let conversation = meta_object
        .entry("goldBandConversation".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !conversation.is_object() {
        *conversation = serde_json::json!({});
    }
    conversation
        .as_object_mut()
        .expect("runtime event conversation metadata object")
        .insert("branchId".to_string(), Value::String(branch_id.to_string()));
}

fn runtime_restore_from_index(
    path: &Utf8Path,
    index: &TimelineMaterializedIndex,
    processed_tail_records: usize,
    restore_mode: TimelineRestoreMode,
) -> Result<TimelineRuntimeRestore> {
    let mut restore = TimelineRuntimeRestore {
        restore_mode,
        covered_revision: index.covered_revision,
        latest_seq: index.runtime_projection.latest_seq,
        processed_tail_records,
        projection_locator_scans: if restore_mode == TimelineRestoreMode::FullRebuild {
            index.item_locators.len()
        } else {
            0
        },
        pending_permissions: index.pending_permissions.values().cloned().collect(),
        pending_elicitations: index.pending_elicitations.values().cloned().collect(),
        timing_state_snapshot: index.timing_state_snapshot.clone(),
        ..TimelineRuntimeRestore::default()
    };

    let mut selected = HashSet::<String>::new();
    if let Some(item_id) = index.pending_retry_prompt_id.as_deref()
        && index.item_locators.contains_key(item_id)
    {
        selected.insert(item_id.to_string());
    }

    selected.extend(
        index
            .runtime_projection
            .active_tool_item_ids
            .iter()
            .cloned(),
    );
    for branch in index.runtime_projection.streams_by_branch.values() {
        for stream in branch.slots() {
            selected.extend(stream.selected_ids().map(str::to_string));
        }
    }
    if let Some(item_id) = index
        .runtime_projection
        .latest_context_compaction_id
        .as_deref()
    {
        selected.insert(item_id.to_string());
    }

    let mut selected_items = selected
        .into_iter()
        .filter_map(|item_id| {
            index
                .item_locators
                .get(&item_id)
                .map(|locator| (item_id, locator.clone()))
        })
        .collect::<Vec<_>>();
    selected_items.sort_by_key(|(_, locator)| (locator.started_seq, locator.seq));
    for (item_id, locator) in selected_items {
        let event = read_event_at_locator(path, &locator)?;
        restore.locator_reads = restore.locator_reads.saturating_add(1);
        let latest_context_candidate = index
            .runtime_projection
            .latest_context_compaction_id
            .as_deref()
            == Some(item_id.as_str());
        let active_context = latest_context_candidate && runtime_context_is_active(&event);
        if latest_context_candidate && !active_context {
            continue;
        }
        if matches!(event.kind.as_str(), "textDelta" | "thoughtDelta" | "plan") {
            restore.active_stream_items.push(event.clone());
        }
        restore.hot_items.push(event);
        if restore.pending_retry_prompt.is_none()
            && index.pending_retry_prompt_id.as_deref() == Some(item_id.as_str())
        {
            restore.pending_retry_prompt = restore.hot_items.last().cloned();
        }
        if active_context {
            restore.active_context_compaction = restore.hot_items.last().cloned();
        }
    }
    Ok(restore)
}

fn runtime_context_is_active(event: &AcpUiEvent) -> bool {
    event.kind == "contextCompaction"
        && matches!(event.status.as_deref(), Some("running" | "completed"))
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/contextCompaction/contextUsedAfter"))
            .and_then(Value::as_u64)
            .is_none()
}

/// Prompt locators are enough for usage recovery and provider history anchors;
/// only the small prompt records are read and their raw Blob references remain
/// untouched.
pub fn read_indexed_prompt_starts(path: &Utf8Path) -> Result<Vec<(String, u64, String)>> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let mut starts = index
            .item_locators
            .iter()
            .filter(|(_, locator)| locator.gold_band_prompt)
            .map(|(item_id, locator)| (item_id.clone(), locator))
            .collect::<Vec<_>>();
        starts.sort_by_key(|(_, locator)| (locator.started_seq, locator.seq));
        Ok(starts
            .into_iter()
            .map(|(item_id, locator)| {
                (
                    locator.prompt_id.clone().unwrap_or(item_id),
                    locator.started_seq,
                    locator.timestamp.clone(),
                )
            })
            .collect())
    })
}

/// Reads only Gold Band prompt anchor records from their indexed locators.
/// This is intentionally separate from the normal runtime restore path: the
/// provider replay state is needed only for an explicit `session/load` history
/// synchronization, while attached reuse and `session/resume` must not read
/// historical prompt bodies.
pub fn read_indexed_prompt_anchor_events(path: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let mut locators = index
            .item_locators
            .values()
            .filter(|locator| locator.gold_band_prompt)
            .collect::<Vec<_>>();
        locators.sort_by_key(|locator| (locator.started_seq, locator.seq));
        locators
            .into_iter()
            .map(|locator| read_event_at_locator(path, locator))
            .collect()
    })
}

fn timeline_tool_is_terminal(status: Option<&str>) -> bool {
    matches!(
        status.unwrap_or_default().to_ascii_lowercase().as_str(),
        "completed" | "success" | "succeeded" | "failed" | "error" | "cancelled" | "canceled"
    )
}

pub fn read_indexed_timeline_item(
    path: &Utf8Path,
    item_id: &str,
) -> Result<Option<TimelineIndexedItem>> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let Some(locator) = index.item_locators.get(item_id) else {
            return Ok(None);
        };
        Ok(Some(TimelineIndexedItem {
            event: read_event_at_locator(path, locator)?,
            generation: index.generation,
            revision: locator.revision,
        }))
    })
}

pub fn read_indexed_pending_permission(
    path: &Utf8Path,
    request_id: &str,
) -> Result<Option<TimelineIndexedItem>> {
    read_indexed_pending_interaction(path, request_id, true)
}

pub fn read_indexed_pending_elicitation(
    path: &Utf8Path,
    elicitation_id: &str,
) -> Result<Option<TimelineIndexedItem>> {
    read_indexed_pending_interaction(path, elicitation_id, false)
}

fn read_indexed_pending_interaction(
    path: &Utf8Path,
    identity: &str,
    permission: bool,
) -> Result<Option<TimelineIndexedItem>> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let event = if permission {
            index.pending_permissions.get(identity)
        } else {
            index.pending_elicitations.get(identity)
        };
        let Some(event) = event else {
            return Ok(None);
        };
        let Some(locator) = index.item_locators.get(&event.id) else {
            return Ok(None);
        };
        Ok(Some(TimelineIndexedItem {
            event: read_event_at_locator(path, locator)?,
            generation: index.generation,
            revision: locator.revision,
        }))
    })
}

pub fn read_indexed_timeline_projection(path: &Utf8Path) -> Result<TimelineBranchProjection> {
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    with_jsonl_file_lock(path, || {
        let (index, processed_tail_records) =
            load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let processed_tail_records = if processed_tail_records == FULL_REBUILD_RESULT_MARKER {
            0
        } else {
            processed_tail_records
        };
        let execution = index
            .item_locators
            .values()
            .filter(|locator| !locator.agent_prompt)
            .collect::<Vec<_>>();
        let latest = execution
            .iter()
            .max_by_key(|locator| (locator.ended_seq, locator.seq));
        let tool_call_count = index
            .item_locators
            .iter()
            .filter(|(_, locator)| locator.kind == "toolCall" && !locator.agent_launch)
            .map(|(item_id, locator)| locator.tool_call_id.as_deref().unwrap_or(item_id))
            .collect::<HashSet<_>>()
            .len();
        let read_file_count = execution
            .iter()
            .flat_map(|locator| locator.read_files.iter())
            .collect::<HashSet<_>>()
            .len();
        let written_file_count = execution
            .iter()
            .flat_map(|locator| locator.written_files.iter())
            .collect::<HashSet<_>>()
            .len();
        let latest_plan_entries = index
            .latest_plan
            .as_ref()
            .and_then(|event| event.raw.as_ref())
            .and_then(|raw| raw.get("entries").or_else(|| raw.pointer("/plan/entries")))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut prompt_turns = index
            .item_locators
            .values()
            .filter(|locator| locator.gold_band_prompt)
            .map(|locator| {
                let terminal_status =
                    prompt_terminal_status(locator.status.as_deref(), locator.ended_at.as_deref());
                TimelinePromptTurnProjection {
                    started_seq: locator.started_seq,
                    started_at: locator
                        .started_at
                        .clone()
                        .unwrap_or_else(|| locator.timestamp.clone()),
                    terminal_seq: terminal_status.as_ref().map(|_| locator.ended_seq),
                    terminal_at: terminal_status
                        .as_ref()
                        .and_then(|_| locator.ended_at.clone()),
                    terminal_status,
                }
            })
            .collect::<Vec<_>>();
        prompt_turns.sort_by_key(|turn| turn.started_seq);
        Ok(TimelineBranchProjection {
            generation: index.generation,
            covered_revision: index.covered_revision,
            processed_tail_records,
            execution_event_count: execution.len(),
            tool_call_count,
            read_file_count,
            written_file_count,
            has_pending_interaction: !index.pending_permissions.is_empty()
                || !index.pending_elicitations.is_empty(),
            latest_seq: latest.map(|locator| locator.ended_seq),
            latest_timestamp: latest.map(|locator| {
                locator
                    .ended_at
                    .clone()
                    .unwrap_or_else(|| locator.timestamp.clone())
            }),
            has_completion_evidence: execution.iter().any(|locator| locator.agent_result),
            latest_plan_entries,
            agent_launches: index.agent_launches.into_values().collect(),
            prompt_turns,
        })
    })
}

pub fn annotate_runtime_control_output(
    path: &Utf8Path,
    item_id: &str,
    artifact_name: &str,
    kind: &str,
    span: &JsonArtifactSpan,
) -> Result<bool> {
    with_jsonl_file_lock(path, || {
        let policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(path);
        let (mut index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let Some(locator) = index.item_locators.get(item_id).cloned() else {
            return Ok(false);
        };
        let mut event = read_event_at_locator(path, &locator)?;
        let Some(content) = event.content.as_deref() else {
            return Ok(false);
        };
        ensure!(
            event.id == item_id,
            "runtime-control source locator identity mismatch"
        );
        ensure!(
            span.start <= span.json_start
                && span.json_start <= span.json_end
                && span.json_end <= span.end
                && span.end <= content.len(),
            "runtime-control source span is outside the selected message"
        );
        ensure!(
            [span.start, span.json_start, span.json_end, span.end]
                .into_iter()
                .all(|index| content.is_char_boundary(index)),
            "runtime-control source span is not on UTF-8 character boundaries"
        );
        ensure!(
            content[span.json_start..span.json_end] == span.json_text,
            "runtime-control source content changed after output evaluation"
        );
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
        let raw = event.raw.get_or_insert_with(|| serde_json::json!({}));
        if !raw.is_object() {
            *raw = serde_json::json!({});
        }
        raw["runtimeControlOutputDisplay"] = display;
        let revision = index.covered_revision.saturating_add(1);
        append_indexed_patch_unlocked(path, &mut index, revision, event)?;
        Ok(true)
    })
}

fn utf16_index(content: &str, byte_index: usize) -> usize {
    content[..byte_index].encode_utf16().count()
}

pub fn settle_timeline_item_status(
    path: &Utf8Path,
    item_id: &str,
    expected_revision: Option<u64>,
    expected_status: &str,
    terminal_status: &str,
    decided_at: String,
) -> Result<TimelineSettleOutcome> {
    with_jsonl_file_lock(path, || {
        let policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(path);
        let (mut index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        settle_timeline_item_status_unlocked(
            path,
            &mut index,
            item_id,
            expected_revision,
            expected_status,
            terminal_status,
            decided_at,
        )
    })
}

/// Settles the one retry prompt projected as processing by the timeline index.
/// A metadata identity is accepted as a recovery hint, but the index projection
/// wins when a newer timeline append reached disk before the metadata rewrite.
pub fn settle_latest_processing_retry_prompt(
    path: &Utf8Path,
    metadata_prompt_event_id: Option<&str>,
    decided_at: String,
) -> Result<TimelineSettleOutcome> {
    with_jsonl_file_lock(path, || {
        let policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(path);
        let (mut index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let item_id = index
            .pending_retry_prompt_id
            .clone()
            .or_else(|| metadata_prompt_event_id.map(str::to_string));
        let Some(item_id) = item_id else {
            return Ok(TimelineSettleOutcome::IdentityMissing);
        };
        let Some(locator) = index.item_locators.get(&item_id) else {
            return Ok(TimelineSettleOutcome::IdentityMissing);
        };
        if !locator.retry_prompt || locator.status.as_deref() != Some("processing") {
            return Ok(TimelineSettleOutcome::AlreadyTerminal);
        }
        settle_timeline_item_status_unlocked(
            path,
            &mut index,
            &item_id,
            None,
            "processing",
            "cancelled",
            decided_at,
        )
    })
}

fn settle_timeline_item_status_unlocked(
    path: &Utf8Path,
    index: &mut TimelineMaterializedIndex,
    item_id: &str,
    expected_revision: Option<u64>,
    expected_status: &str,
    terminal_status: &str,
    decided_at: String,
) -> Result<TimelineSettleOutcome> {
    let Some(locator) = index.item_locators.get(item_id).cloned() else {
        return Ok(TimelineSettleOutcome::IdentityMissing);
    };
    if expected_revision.is_some_and(|revision| revision != locator.revision) {
        return Ok(TimelineSettleOutcome::RevisionConflict);
    }
    let mut event = read_event_at_locator(path, &locator)?;
    if event.status.as_deref() != Some(expected_status) {
        return Ok(TimelineSettleOutcome::AlreadyTerminal);
    }
    let revision = index.covered_revision.saturating_add(1);
    event.status = Some(terminal_status.to_string());
    event.ended_seq = Some(revision);
    event.ended_at = Some(decided_at);
    let raw = event.raw.get_or_insert_with(|| serde_json::json!({}));
    if !raw.is_object() {
        *raw = serde_json::json!({});
    }
    raw["cancelled"] = Value::Bool(terminal_status == "cancelled");
    append_indexed_patch_unlocked(path, index, revision, event)?;
    Ok(TimelineSettleOutcome::Applied)
}

pub fn settle_permission_item(
    path: &Utf8Path,
    item_id: &str,
    expected_revision: Option<u64>,
    request_id: &str,
    option_id: Option<String>,
    cancelled: bool,
    decided_at: String,
) -> Result<TimelineSettleOutcome> {
    with_jsonl_file_lock(path, || {
        let policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(path);
        let (mut index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let Some(locator) = index.item_locators.get(item_id).cloned() else {
            return Ok(TimelineSettleOutcome::IdentityMissing);
        };
        if expected_revision.is_some_and(|revision| revision != locator.revision) {
            return Ok(TimelineSettleOutcome::RevisionConflict);
        }
        let mut event = read_event_at_locator(path, &locator)?;
        if event.kind != "permissionRequest"
            || event
                .status
                .as_deref()
                .is_none_or(|status| status != "pending")
        {
            return Ok(TimelineSettleOutcome::AlreadyTerminal);
        }
        let revision = index.covered_revision.saturating_add(1);
        event.status = Some(if cancelled { "cancelled" } else { "selected" }.to_string());
        event.ended_seq = Some(revision);
        event.ended_at = Some(decided_at);
        let raw = event.raw.get_or_insert_with(|| serde_json::json!({}));
        if !raw.is_object() {
            *raw = serde_json::json!({});
        }
        raw["requestId"] = Value::String(request_id.to_string());
        if cancelled {
            raw["cancelled"] = Value::Bool(true);
            if let Some(object) = raw.as_object_mut() {
                object.remove("optionId");
            }
        } else {
            if let Some(object) = raw.as_object_mut() {
                object.remove("cancelled");
            }
            raw["optionId"] = option_id.map(Value::String).unwrap_or(Value::Null);
        }
        append_indexed_patch_unlocked(path, &mut index, revision, event)?;
        Ok(TimelineSettleOutcome::Applied)
    })
}

pub fn append_elicitation_response_item(
    path: &Utf8Path,
    request_item_id: &str,
    expected_revision: Option<u64>,
    elicitation_id: &str,
    action: &str,
    content: Option<Value>,
    decided_at: String,
) -> Result<TimelineSettleOutcome> {
    with_jsonl_file_lock(path, || {
        let policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(path);
        let (mut index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let response_id = format!("{elicitation_id}-response");
        if index.item_locators.contains_key(&response_id) {
            return Ok(TimelineSettleOutcome::AlreadyTerminal);
        }
        let Some(locator) = index.item_locators.get(request_item_id).cloned() else {
            return Ok(TimelineSettleOutcome::IdentityMissing);
        };
        if expected_revision.is_some_and(|revision| revision != locator.revision) {
            return Ok(TimelineSettleOutcome::RevisionConflict);
        }
        let request = read_event_at_locator(path, &locator)?;
        if request.kind != "elicitationRequest"
            || request
                .status
                .as_deref()
                .is_some_and(|status| status != "pending")
        {
            return Ok(TimelineSettleOutcome::AlreadyTerminal);
        }
        let revision = index.covered_revision.saturating_add(1);
        let mut event = crate::acp::events::elicitation_response_event(
            revision,
            elicitation_id.to_string(),
            action.to_string(),
            content,
        );
        event.timestamp = decided_at.clone();
        event.started_seq = Some(revision);
        event.ended_seq = Some(revision);
        event.started_at = Some(decided_at.clone());
        event.ended_at = Some(decided_at);
        if let (Some(request_meta), Some(event_raw)) = (
            request.raw.as_ref().and_then(|raw| raw.get("_meta")),
            event.raw.as_mut(),
        ) {
            event_raw["_meta"] = request_meta.clone();
        }
        append_indexed_patch_unlocked(path, &mut index, revision, event)?;
        Ok(TimelineSettleOutcome::Applied)
    })
}

fn append_indexed_patch_unlocked(
    path: &Utf8Path,
    index: &mut TimelineMaterializedIndex,
    revision: u64,
    event: AcpUiEvent,
) -> Result<()> {
    let patch = AcpTimelinePatch {
        patch_type: "timelinePatch".to_string(),
        item_id: event.id.clone(),
        revision,
        op: "upsert".to_string(),
        item: event.clone(),
    };
    let offset = timeline_file_len(path);
    let line_length = serde_json::to_vec(&patch)?.len().saturating_add(1) as u64;
    append_jsonl_flushed_unlocked(path, &patch)?;
    apply_index_event(index, &event, revision, offset, line_length)?;
    index.event_count = index.event_count.saturating_add(1);
    index.patch_count = index.patch_count.saturating_add(1);
    index.patch_bytes = index.patch_bytes.saturating_add(line_length);
    checkpoint_index_unlocked(path, index)
}

fn timeline_file_signature(path: &Utf8Path) -> TimelineFileSignature {
    std::fs::metadata(path.as_std_path())
        .map(|metadata| TimelineFileSignature {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
        .unwrap_or(TimelineFileSignature {
            len: 0,
            modified: None,
        })
}

pub fn upsert_timeline_item(
    path: &Utf8Path,
    revision: u64,
    item: &AcpUiEvent,
    policy: TimelineCompactionPolicy,
) -> Result<TimelineUpsertOutcome> {
    let mut store = TimelineStore::open(path.to_path_buf(), policy)?;
    let outcome = store.upsert(revision, item)?;
    store.force_checkpoint()?;
    Ok(outcome)
}

fn write_canonical_timeline_unlocked(
    path: &Utf8Path,
    blob_store: &TurnFileStore,
    items: &[AcpUiEvent],
) -> Result<()> {
    ensure_parent_dir(path)?;
    atomic_write_file(path.as_std_path(), |file| -> Result<()> {
        let mut writer = BufWriter::new(file);
        for item in items {
            let mut item = item.clone();
            externalize_timeline_event(blob_store, &mut item)?;
            serde_json::to_writer(&mut writer, &AcpTimelineItem { item })?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
        Ok(())
    })
}

#[derive(Debug, Default)]
struct TimelineFileStats {
    patch_count: usize,
    patch_bytes: u64,
    redundant_revision_count: usize,
}

fn timeline_attempt_dir(path: &Utf8Path) -> Utf8PathBuf {
    if path.file_name() == Some("timeline.jsonl")
        && path
            .parent()
            .and_then(Utf8Path::parent)
            .and_then(Utf8Path::parent)
            .is_some()
    {
        return path
            .parent()
            .and_then(Utf8Path::parent)
            .and_then(Utf8Path::parent)
            .expect("checked branch timeline ancestors")
            .to_path_buf();
    }
    path.parent().unwrap_or(path).to_path_buf()
}

fn timeline_blob_store(path: &Utf8Path) -> TurnFileStore {
    TurnFileStore::new(timeline_attempt_dir(path), TurnFileCaptureConfig::default())
}

pub(crate) fn externalize_timeline_event_for_storage(
    path: &Utf8Path,
    item: &mut AcpUiEvent,
) -> Result<()> {
    externalize_timeline_event(&timeline_blob_store(path), item)
}

fn externalize_timeline_event(store: &TurnFileStore, item: &mut AcpUiEvent) -> Result<()> {
    if let Some(raw) = item.raw.as_mut() {
        externalize_large_strings(store, raw)?;
    }
    Ok(())
}

fn externalize_large_strings(store: &TurnFileStore, value: &mut Value) -> Result<()> {
    match value {
        Value::String(content) if content.len() >= TIMELINE_BLOB_MIN_BYTES => {
            let version = store.write_blob(content)?;
            *value = serde_json::json!({ TIMELINE_BLOB_REF_KEY: version });
        }
        Value::Array(values) => {
            for value in values {
                externalize_large_strings(store, value)?;
            }
        }
        Value::Object(object) => {
            if object.contains_key(TIMELINE_BLOB_REF_KEY) {
                return Ok(());
            }
            for value in object.values_mut() {
                externalize_large_strings(store, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn hydrate_timeline_value(path: &Utf8Path, value: &mut Value) -> Result<()> {
    hydrate_large_strings(&timeline_blob_store(path), value)
}

fn hydrate_large_strings(store: &TurnFileStore, value: &mut Value) -> Result<()> {
    if let Some(reference) = value
        .as_object()
        .and_then(|object| object.get(TIMELINE_BLOB_REF_KEY))
        .cloned()
    {
        let version: FileVersionRef = serde_json::from_value(reference)?;
        *value = Value::String(store.read_blob(&version)?);
        return Ok(());
    }
    match value {
        Value::Array(values) => {
            for value in values {
                hydrate_large_strings(store, value)?;
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                hydrate_large_strings(store, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn semantic_fingerprint(item: &AcpUiEvent) -> Result<u64> {
    let mut value = serde_json::to_value(item)?;
    if let Some(object) = value.as_object_mut() {
        for key in [
            "seq",
            "timestamp",
            "startedSeq",
            "endedSeq",
            "startedAt",
            "endedAt",
        ] {
            object.remove(key);
        }
        if let Some(raw) = object.get_mut("raw") {
            remove_replay_audit_fields(raw);
        }
    }
    let bytes = serde_json::to_vec(&value)?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(hasher.finish())
}

fn remove_replay_audit_fields(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for key in [
                "replaySeq",
                "replayTimestamp",
                "transportSeq",
                "transportTimestamp",
            ] {
                object.remove(key);
            }
            for child in object.values_mut() {
                remove_replay_audit_fields(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                remove_replay_audit_fields(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io::Write as _;
    use std::time::Instant;

    use camino::Utf8PathBuf;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{
        DEFAULT_TIMELINE_COMPACT_MIN_PATCH_COUNT, DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT,
        TIMELINE_BLOB_MIN_BYTES, TIMELINE_INDEX_FORMAT_VERSION, TimelineCompactionPolicy,
        TimelineRestoreMode, TimelineSettleOutcome, TimelineStore, TimelineUpsertOutcome,
        read_indexed_prompt_anchor_events, read_indexed_runtime_restore,
        read_indexed_runtime_restore_for_branch, read_indexed_timeline_page,
        read_indexed_timeline_projection, settle_latest_processing_retry_prompt,
        settle_timeline_item_status, timeline_index_path,
    };
    use crate::acp::events::{AcpTimelinePatch, AcpTimingPatch, AcpUiEvent, load_timeline_items};

    fn event(id: &str, seq: u64, content: &str) -> AcpUiEvent {
        AcpUiEvent {
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            kind: "textDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some(content.to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: Some(seq),
            ended_seq: Some(seq),
            started_at: Some(format!("{seq}Z")),
            ended_at: Some(format!("{seq}Z")),
            timing: None,
            raw: Some(json!({ "source": "providerHistory" })),
        }
    }

    #[test]
    fn replay_audit_sequence_does_not_append_duplicate_patch() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let policy = TimelineCompactionPolicy {
            max_size_bytes: u64::MAX,
            patch_ratio: usize::MAX,
        };
        let mut store = TimelineStore::open(path.clone(), policy).unwrap();
        assert_eq!(
            store.upsert(1, &event("message-1", 1, "hello")).unwrap(),
            TimelineUpsertOutcome::Appended
        );
        assert_eq!(
            store.upsert(2, &event("message-1", 99, "hello")).unwrap(),
            TimelineUpsertOutcome::Unchanged
        );
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn content_change_appends_revision() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let policy = TimelineCompactionPolicy {
            max_size_bytes: u64::MAX,
            patch_ratio: usize::MAX,
        };
        let mut store = TimelineStore::open(path.clone(), policy).unwrap();
        store.upsert(1, &event("message-1", 1, "hel")).unwrap();
        assert_eq!(
            store.durable_watermark_for_item_id("message-1"),
            Some((1, 1))
        );
        store.upsert(2, &event("message-1", 2, "hello")).unwrap();
        assert_eq!(
            store.durable_watermark_for_item_id("message-1"),
            Some((1, 2)),
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
        assert_eq!(
            load_timeline_items(&path).unwrap()[0].content.as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn batch_upsert_commits_distinct_identities_and_preserves_aligned_outcomes() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let policy = TimelineCompactionPolicy {
            max_size_bytes: u64::MAX,
            patch_ratio: usize::MAX,
        };
        let mut store = TimelineStore::open(path.clone(), policy).unwrap();
        let updates = (1..=512)
            .map(|seq| (seq, event(&format!("message-{seq}"), seq, "hello")))
            .collect::<Vec<_>>();

        let outcomes = store.upsert_batch(&updates).unwrap();

        assert_eq!(outcomes.len(), updates.len());
        assert!(
            outcomes
                .iter()
                .all(|outcome| *outcome == TimelineUpsertOutcome::Appended)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 512);
        assert_eq!(
            store.durable_watermark_for_item_id("message-512"),
            Some((1, 512))
        );

        let unchanged = updates
            .iter()
            .map(|(revision, item)| {
                let mut item = item.clone();
                item.seq = item.seq.saturating_add(10_000);
                (revision.saturating_add(10_000), item)
            })
            .collect::<Vec<_>>();
        assert!(
            store
                .upsert_batch(&unchanged)
                .unwrap()
                .iter()
                .all(|outcome| *outcome == TimelineUpsertOutcome::Unchanged)
        );
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 512);
    }

    #[test]
    fn upsert_exposes_exact_compaction_observation_once() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store = TimelineStore::open(
            path,
            TimelineCompactionPolicy {
                max_size_bytes: u64::MAX,
                patch_ratio: 1,
            },
        )
        .unwrap();
        assert_eq!(
            store.upsert(1, &event("message-1", 1, "hel")).unwrap(),
            TimelineUpsertOutcome::Appended
        );
        assert!(store.take_last_compaction_elapsed().is_none());
        store.policy.max_size_bytes = 0;

        assert_eq!(
            store.upsert(2, &event("message-1", 2, "hello")).unwrap(),
            TimelineUpsertOutcome::AppendedAndCompacted
        );
        assert!(store.take_last_compaction_elapsed().is_some());
        assert!(store.take_last_compaction_elapsed().is_none());
    }

    #[test]
    fn agent_launches_are_standalone_semantic_blocks_from_canonical_metadata() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &event("prompt", 1, "delegate")).unwrap();
        for (revision, tool_call_id) in [(2, "agent-a"), (3, "agent-b")] {
            let mut launch = event(tool_call_id, revision, "");
            launch.kind = "toolCall".to_string();
            launch.tool_call_id = Some(tool_call_id.to_string());
            launch.raw = Some(json!({
                "_meta": {
                    "goldBandConversation": {
                        "branchId": "root",
                        "launchedAgentExecutionId": format!("execution-{tool_call_id}"),
                        "toolName": "Agent"
                    }
                },
                "rawInput": { "description": tool_call_id }
            }));
            store.upsert(revision, &launch).unwrap();
        }
        store.force_checkpoint().unwrap();

        let page = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();

        assert_eq!(page.total_semantic_blocks, 3);
        assert_eq!(page.loaded_semantic_blocks, 3);
        assert_eq!(page.events.len(), 3);
        assert_eq!(
            read_indexed_timeline_projection(&path)
                .unwrap()
                .agent_launches
                .len(),
            2
        );
    }

    #[test]
    fn revision_delta_returns_only_blocks_changed_after_snapshot_watermark() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &event("message-1", 1, "one")).unwrap();
        store.upsert(2, &event("message-2", 2, "two")).unwrap();
        store
            .upsert(3, &event("message-1", 1, "one updated"))
            .unwrap();
        store.force_checkpoint().unwrap();

        let page = read_indexed_timeline_page(&path, None, None, Some(2), 30).unwrap();

        assert_eq!(page.covered_revision, 3);
        assert_eq!(page.newest_revision, Some(3));
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].id, "message-1");
        assert_eq!(page.events[0].content.as_deref(), Some("one updated"));
    }

    #[test]
    fn revision_delta_does_not_split_blocks_with_the_same_revision() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &event("seed", 0, "seed")).unwrap();
        store.force_checkpoint().unwrap();
        drop(store);
        for item in [event("message-1", 1, "one"), event("message-2", 2, "two")] {
            crate::storage::append_jsonl(
                &path,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: item.id.clone(),
                    revision: 2,
                    op: "upsert".to_string(),
                    item,
                },
            )
            .unwrap();
        }

        let page = read_indexed_timeline_page(&path, None, None, Some(1), 1).unwrap();

        assert_eq!(page.newest_revision, Some(2));
        assert_eq!(page.events.len(), 2);
    }

    #[test]
    fn checkpoint_recovery_replays_only_the_uncovered_tail_once() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let policy = TimelineCompactionPolicy {
            max_size_bytes: u64::MAX,
            patch_ratio: usize::MAX,
        };
        let mut store = TimelineStore::open(path.clone(), policy).unwrap();
        store.upsert(1, &event("message-1", 1, "first")).unwrap();
        store.force_checkpoint().unwrap();

        let second = event("message-2", 2, "second");
        crate::storage::append_jsonl(
            &path,
            &AcpTimelinePatch {
                patch_type: "timelinePatch".to_string(),
                item_id: second.id.clone(),
                revision: 2,
                op: "upsert".to_string(),
                item: second,
            },
        )
        .unwrap();

        let recovered = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        assert_eq!(recovered.processed_tail_records, 1);
        assert_eq!(recovered.events.len(), 2);
        let reopened = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        assert_eq!(reopened.processed_tail_records, 0);
    }

    #[test]
    fn runtime_restore_reports_full_rebuild_when_tail_bound_is_exceeded() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store = TimelineStore::open(
            path.clone(),
            TimelineCompactionPolicy {
                max_size_bytes: u64::MAX,
                patch_ratio: usize::MAX,
            },
        )
        .unwrap();
        store.upsert(1, &event("seed", 1, "seed")).unwrap();
        store.force_checkpoint().unwrap();
        drop(store);

        for seq in 2..=(DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT as u64 + 2) {
            crate::storage::append_jsonl(
                &path,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: format!("tail-{seq}"),
                    revision: seq,
                    op: "upsert".to_string(),
                    item: event(&format!("tail-{seq}"), seq, "tail"),
                },
            )
            .unwrap();
        }

        let restore = read_indexed_runtime_restore(&path).unwrap();
        assert_eq!(restore.restore_mode, TimelineRestoreMode::FullRebuild);
        assert_eq!(restore.processed_tail_records, 0);
        assert_eq!(
            restore.latest_seq,
            DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT as u64 + 2
        );
    }

    #[test]
    fn indexed_settlement_is_cas_guarded_and_idempotent() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut prompt = event("prompt-1", 1, "retry");
        prompt.kind = "userTextDelta".to_string();
        prompt.status = Some("processing".to_string());
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &prompt).unwrap();
        store.force_checkpoint().unwrap();

        assert_eq!(
            settle_timeline_item_status(
                &path,
                "prompt-1",
                Some(1),
                "processing",
                "cancelled",
                "2Z".to_string(),
            )
            .unwrap(),
            TimelineSettleOutcome::Applied,
        );
        assert_eq!(
            settle_timeline_item_status(
                &path,
                "prompt-1",
                None,
                "processing",
                "cancelled",
                "3Z".to_string(),
            )
            .unwrap(),
            TimelineSettleOutcome::AlreadyTerminal,
        );
        assert_eq!(
            read_indexed_timeline_page(&path, None, None, None, 30)
                .unwrap()
                .events[0]
                .status
                .as_deref(),
            Some("cancelled"),
        );
    }

    #[test]
    fn pending_retry_projection_outranks_stale_session_metadata_identity() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut stale = event("prompt-stale", 1, "old retry");
        stale.kind = "userTextDelta".to_string();
        stale.raw = Some(json!({ "retry": { "attempt": 1, "maxAttempts": 3 } }));
        let mut active = event("prompt-active", 2, "current retry");
        active.kind = "userTextDelta".to_string();
        active.status = Some("processing".to_string());
        active.raw = Some(json!({ "retry": { "attempt": 2, "maxAttempts": 3 } }));
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &stale).unwrap();
        store.upsert(2, &active).unwrap();
        store.force_checkpoint().unwrap();

        assert_eq!(
            settle_latest_processing_retry_prompt(&path, Some("prompt-stale"), "3Z".to_string(),)
                .unwrap(),
            TimelineSettleOutcome::Applied,
        );
        assert_eq!(
            read_indexed_timeline_page(&path, None, None, None, 30)
                .unwrap()
                .events
                .into_iter()
                .find(|event| event.id == "prompt-active")
                .unwrap()
                .status
                .as_deref(),
            Some("cancelled"),
        );
    }

    #[test]
    fn ten_thousand_revisions_query_from_checkpoint_without_full_replay() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let file = std::fs::File::create(path.as_std_path()).unwrap();
        let mut writer = std::io::BufWriter::new(file);
        for revision in 1..=10_000u64 {
            let item = event("message-1", revision, &format!("revision-{revision}"));
            serde_json::to_writer(
                &mut writer,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: item.id.clone(),
                    revision,
                    op: "upsert".to_string(),
                    item,
                },
            )
            .unwrap();
            writer.write_all(b"\n").unwrap();
        }
        writer.flush().unwrap();

        let migrated = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        assert_eq!(migrated.events.len(), 1);
        let page = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        assert_eq!(page.processed_tail_records, 0);
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].content.as_deref(), Some("revision-10000"));
    }

    #[test]
    fn store_refreshes_index_after_an_external_writer_changes_the_file() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let policy = TimelineCompactionPolicy {
            max_size_bytes: u64::MAX,
            patch_ratio: usize::MAX,
        };
        let mut first = TimelineStore::open(path.clone(), policy).unwrap();
        let mut second = TimelineStore::open(path.clone(), policy).unwrap();
        first.upsert(1, &event("message-1", 1, "hello")).unwrap();
        assert_eq!(
            second.upsert(2, &event("message-1", 50, "hello")).unwrap(),
            TimelineUpsertOutcome::Unchanged
        );
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn compaction_preserves_canonical_projection() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut file = std::fs::File::create(path.as_std_path()).unwrap();
        for revision in 1..=6 {
            let item = event("message-1", revision, &format!("hello-{revision}"));
            serde_json::to_writer(
                &mut file,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: item.id.clone(),
                    revision,
                    op: "upsert".to_string(),
                    item,
                },
            )
            .unwrap();
            file.write_all(b"\n").unwrap();
        }
        drop(file);
        let before = load_timeline_items(&path).unwrap();
        let _store = TimelineStore::open(
            path.clone(),
            TimelineCompactionPolicy {
                max_size_bytes: 0,
                patch_ratio: 2,
            },
        )
        .unwrap();
        let after = load_timeline_items(&path).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(&after).unwrap()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn opening_legacy_replay_duplicates_compacts_without_changing_projection() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut file = std::fs::File::create(path.as_std_path()).unwrap();
        for (revision, seq) in [(1, 1), (2, 99)] {
            let item = event("message-1", seq, "hello");
            serde_json::to_writer(
                &mut file,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: item.id.clone(),
                    revision,
                    op: "upsert".to_string(),
                    item,
                },
            )
            .unwrap();
            file.write_all(b"\n").unwrap();
        }
        drop(file);

        let before = load_timeline_items(&path).unwrap();
        let _store = TimelineStore::open(
            path.clone(),
            TimelineCompactionPolicy {
                max_size_bytes: u64::MAX,
                patch_ratio: usize::MAX,
            },
        )
        .unwrap();
        let after = load_timeline_items(&path).unwrap();

        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(&after).unwrap()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn canonical_file_larger_than_compaction_budget_does_not_recompact_every_patch() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let oversized = event("large-message", 1, &"x".repeat(9 * 1024 * 1024));
        crate::acp::events::write_timeline_items(&path, &[oversized]).unwrap();
        let mut store = TimelineStore::open(
            path,
            TimelineCompactionPolicy {
                max_size_bytes: 1024,
                patch_ratio: usize::MAX,
            },
        )
        .unwrap();

        assert_eq!(
            store.upsert(2, &event("message-2", 2, "second")).unwrap(),
            TimelineUpsertOutcome::Appended
        );
        assert_eq!(
            store.upsert(3, &event("message-3", 3, "third")).unwrap(),
            TimelineUpsertOutcome::Appended
        );
    }

    #[test]
    fn large_raw_strings_are_blob_backed_and_hydrated_on_load() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let large_output = "terminal-output".repeat(TIMELINE_BLOB_MIN_BYTES / 8);
        let mut item = event("tool-1", 1, "");
        item.kind = "toolCall".to_string();
        item.raw = Some(json!({
            "sessionUpdate": "tool_call_update",
            "content": [{ "type": "content", "text": large_output }]
        }));

        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &item).unwrap();

        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(stored.contains("$goldBandBlob"));
        assert!(stored.len() < TIMELINE_BLOB_MIN_BYTES);
        assert_eq!(load_timeline_items(&path).unwrap()[0].raw, item.raw);
        let blob_dir = dir.path().join("acp.file-blobs");
        assert!(blob_dir.exists());
        std::fs::remove_dir_all(blob_dir).unwrap();
        TimelineStore::open(path, TimelineCompactionPolicy::default()).unwrap();
    }

    #[test]
    fn runtime_restore_keeps_large_raw_values_as_blob_references() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut item = event("stream", 1, "streamed text");
        item.raw = Some(json!({
            "source": "provider",
            "large": "x".repeat(TIMELINE_BLOB_MIN_BYTES + 1024),
        }));
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &item).unwrap();
        store.force_checkpoint().unwrap();

        let restore = read_indexed_runtime_restore(&path).unwrap();
        let restored = restore
            .hot_items
            .iter()
            .find(|item| item.id == "stream")
            .expect("active stream must be in the hot projection");
        let large = restored
            .raw
            .as_ref()
            .and_then(|raw| raw.get("large"))
            .and_then(|value| value.as_object())
            .expect("startup restore must not hydrate the Blob");
        assert!(large.contains_key("$goldBandBlob"));
    }

    #[test]
    fn runtime_restore_defers_prompt_anchor_bodies_until_explicit_load() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut prompt = event("prompt-1", 1, "hello");
        prompt.kind = "userTextDelta".to_string();
        prompt.raw = Some(json!({ "source": "goldBandPrompt" }));
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &prompt).unwrap();
        store.force_checkpoint().unwrap();

        let restore = read_indexed_runtime_restore(&path).unwrap();
        assert!(restore.prompt_anchors.is_empty());
        let anchors = read_indexed_prompt_anchor_events(&path).unwrap();
        assert_eq!(
            anchors
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["prompt-1"]
        );
    }

    #[test]
    fn runtime_restore_indexes_provider_replay_identity_without_loading_history() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut item = event("local-item", 1, "provider text");
        item.raw = Some(json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "provider-message-1",
        }));
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &item).unwrap();
        store.force_checkpoint().unwrap();

        let restore = read_indexed_runtime_restore(&path).unwrap();
        assert_eq!(restore.projection_locator_scans, 0);
        assert!(store.contains_provider_history_identity("assistant-message-provider-message-1"));
        assert!(!store.contains_provider_history_identity("local-item"));
    }

    #[test]
    fn index_hit_runtime_restore_work_is_constant_when_history_grows_tenfold() {
        fn build_and_restore(path: &Utf8PathBuf, item_count: u64) -> super::TimelineRuntimeRestore {
            let file = std::fs::File::create(path.as_std_path()).unwrap();
            let mut writer = std::io::BufWriter::new(file);
            for seq in 1..=item_count {
                let mut item = event(&format!("history-{seq}"), seq, "history");
                item.raw = Some(json!({
                    "providerHistoryItemId": format!("provider-history-{seq}"),
                }));
                serde_json::to_writer(
                    &mut writer,
                    &AcpTimelinePatch {
                        patch_type: "timelinePatch".to_string(),
                        item_id: item.id.clone(),
                        revision: seq,
                        op: "upsert".to_string(),
                        item,
                    },
                )
                .unwrap();
                writer.write_all(b"\n").unwrap();
            }
            writer.flush().unwrap();

            // First read is the allowed current-index construction. The second is
            // the steady-state index-hit path measured by this regression.
            read_indexed_runtime_restore(path).unwrap();
            read_indexed_runtime_restore(path).unwrap()
        }

        let dir = tempdir().unwrap();
        let small_path =
            Utf8PathBuf::from_path_buf(dir.path().join("small.timeline.jsonl")).unwrap();
        let large_path =
            Utf8PathBuf::from_path_buf(dir.path().join("large.timeline.jsonl")).unwrap();
        let small = build_and_restore(&small_path, 100);
        let large = build_and_restore(&large_path, 1_000);

        assert_eq!(small.restore_mode, TimelineRestoreMode::IndexHit);
        assert_eq!(large.restore_mode, TimelineRestoreMode::IndexHit);
        assert_eq!(small.index_locator_count, 100);
        assert_eq!(large.index_locator_count, 1_000);
        assert_eq!(small.projection_locator_scans, 0);
        assert_eq!(large.projection_locator_scans, 0);
        assert_eq!(small.locator_reads, 1);
        assert_eq!(large.locator_reads, 1);
        assert_eq!(small.active_stream_items.len(), 1);
        assert_eq!(large.active_stream_items.len(), 1);
        assert_eq!(large.active_stream_items[0].id, "history-1000");
    }

    #[test]
    fn late_historical_patch_does_not_resurrect_a_closed_runtime_stream() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut stream = event("answer-1", 1, "draft");
        stream.raw = Some(json!({ "providerHistoryItemId": "provider-answer-1" }));
        let mut user = event("prompt-2", 10, "next prompt");
        user.kind = "userTextDelta".to_string();
        user.raw = Some(json!({ "source": "goldBandPrompt" }));
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &stream).unwrap();
        store.upsert(2, &user).unwrap();

        stream.content = Some("late corrected draft".to_string());
        store.upsert(3, &stream).unwrap();
        store.force_checkpoint().unwrap();

        let restore = read_indexed_runtime_restore(&path).unwrap();
        assert_eq!(restore.projection_locator_scans, 0);
        assert!(restore.active_stream_items.is_empty());
    }

    #[test]
    fn legacy_timing_patch_builds_explicit_snapshot_without_replaying_history() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut item = event("timing-1", 1, "turn");
        item.timing = Some(AcpTimingPatch {
            session_elapsed_seconds: 42,
            revision: Some(7),
            observed_at: Some("100Z".to_string()),
            active_turn_started_at: Some("90Z".to_string()),
            active_turn_last_activity_at: Some("99Z".to_string()),
            permission_wait_started_at: None,
            user_wait_started_at: Some("95Z".to_string()),
            wait_reason: Some("permission".to_string()),
            paused: false,
            reason: Some("legacy".to_string()),
            state_snapshot: None,
        });
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        store.upsert(1, &item).unwrap();
        store.force_checkpoint().unwrap();

        let restore = read_indexed_runtime_restore(&path).unwrap();
        let snapshot = restore
            .timing_state_snapshot
            .expect("legacy timing must have an explicit fallback snapshot");
        assert_eq!(snapshot.elapsed_seconds, 42);
        assert_eq!(snapshot.active_turn_started_at, Some(90));
        assert_eq!(snapshot.active_turn_last_activity_at, Some(99));
        assert_eq!(snapshot.user_wait_started_at, Some(95));
        assert_eq!(snapshot.revision, Some(7));
    }

    #[test]
    fn branch_runtime_restore_keeps_equal_item_ids_in_separate_scopes() {
        let dir = tempdir().unwrap();
        let root_path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let branch_dir = dir.path().join("agents").join("branch-a");
        std::fs::create_dir_all(&branch_dir).unwrap();
        let branch_path = Utf8PathBuf::from_path_buf(branch_dir.join("timeline.jsonl")).unwrap();
        let mut root_item = event("same-item", 1, "root");
        root_item.raw = Some(json!({
            "_meta": { "goldBandConversation": { "branchId": "root" } }
        }));
        let mut branch_item = event("same-item", 1, "branch");
        branch_item.raw = Some(json!({
            "_meta": { "goldBandConversation": { "branchId": "branch-a" } }
        }));
        let mut root_store =
            TimelineStore::open(root_path.clone(), TimelineCompactionPolicy::default()).unwrap();
        root_store.upsert(1, &root_item).unwrap();
        root_store.force_checkpoint().unwrap();
        let mut branch_store =
            TimelineStore::open(branch_path.clone(), TimelineCompactionPolicy::default()).unwrap();
        branch_store.upsert(1, &branch_item).unwrap();
        branch_store.force_checkpoint().unwrap();

        let mut merged = read_indexed_runtime_restore(&root_path).unwrap();
        merged.merge(read_indexed_runtime_restore_for_branch(&branch_path, "branch-a").unwrap());
        let values = merged
            .hot_items
            .iter()
            .map(|item| {
                (
                    item.raw
                        .as_ref()
                        .and_then(|raw| raw.pointer("/_meta/goldBandConversation/branchId"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    item.content.clone().unwrap_or_default(),
                )
            })
            .collect::<HashSet<_>>();
        assert_eq!(values.len(), 2);
        assert!(values.contains(&("root".to_string(), "root".to_string())));
        assert!(values.contains(&("branch-a".to_string(), "branch".to_string())));
    }

    #[test]
    fn concurrent_stores_do_not_overwrite_each_others_checkpoint_projection() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut first =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        let mut second =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();

        first.upsert(1, &event("message-1", 1, "first")).unwrap();
        second.upsert(2, &event("message-2", 2, "second")).unwrap();
        first.force_checkpoint().unwrap();

        let page = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        let ids = page
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            ids,
            std::collections::HashSet::from(["message-1", "message-2"])
        );
        assert_eq!(page.processed_tail_records, 0);
    }

    #[test]
    fn older_index_format_is_rebuilt_before_new_projection_fields_are_used() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut prompt = event("agent-prompt", 1, "delegated task");
        prompt.raw = Some(json!({ "source": "agentBranchPrompt" }));
        let mut retry = event("retry-prompt", 2, "retrying task");
        retry.kind = "userTextDelta".to_string();
        retry.status = Some("processing".to_string());
        retry.raw = Some(json!({ "retry": { "attempt": 1, "maxAttempts": 3 } }));
        let mut store = TimelineStore::open(
            path.clone(),
            TimelineCompactionPolicy {
                max_size_bytes: u64::MAX,
                patch_ratio: usize::MAX,
            },
        )
        .unwrap();
        store.upsert(1, &prompt).unwrap();
        store.upsert(2, &retry).unwrap();
        store.force_checkpoint().unwrap();

        let index_path = timeline_index_path(&path);
        let mut legacy: serde_json::Value = crate::storage::read_json(&index_path).unwrap();
        legacy["formatVersion"] = json!(TIMELINE_INDEX_FORMAT_VERSION - 1);
        legacy["generation"] = json!(7);
        legacy["itemLocators"]["agent-prompt"]
            .as_object_mut()
            .unwrap()
            .remove("agentPrompt");
        legacy["itemLocators"]["retry-prompt"]
            .as_object_mut()
            .unwrap()
            .remove("retryPrompt");
        legacy
            .as_object_mut()
            .unwrap()
            .remove("pendingRetryPromptId");
        legacy.as_object_mut().unwrap().remove("runtimeProjection");
        crate::storage::write_json(&index_path, &legacy).unwrap();

        let projection = read_indexed_timeline_projection(&path).unwrap();
        assert_eq!(projection.execution_event_count, 1);
        assert_eq!(projection.generation, 8);
        let rebuilt: serde_json::Value = crate::storage::read_json(&index_path).unwrap();
        assert_eq!(
            rebuilt["formatVersion"].as_u64(),
            Some(u64::from(TIMELINE_INDEX_FORMAT_VERSION))
        );
        assert_eq!(
            rebuilt["itemLocators"]["agent-prompt"]["agentPrompt"].as_bool(),
            Some(true)
        );
        assert_eq!(
            rebuilt["itemLocators"]["retry-prompt"]["retryPrompt"].as_bool(),
            Some(true)
        );
        assert_eq!(
            rebuilt["pendingRetryPromptId"].as_str(),
            Some("retry-prompt")
        );
        assert_eq!(rebuilt["runtimeProjection"]["latestSeq"].as_u64(), Some(2));
    }

    #[test]
    fn compaction_allocates_generation_from_latest_checkpoint() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store = TimelineStore::open(
            path.clone(),
            TimelineCompactionPolicy {
                max_size_bytes: u64::MAX,
                patch_ratio: usize::MAX,
            },
        )
        .unwrap();
        store.upsert(1, &event("message-1", 1, "one")).unwrap();
        store.upsert(2, &event("message-1", 2, "two")).unwrap();

        let index_path = timeline_index_path(&path);
        let mut latest: serde_json::Value = crate::storage::read_json(&index_path).unwrap();
        latest["generation"] = json!(41);
        latest["patchCount"] = json!(DEFAULT_TIMELINE_COMPACT_MIN_PATCH_COUNT);
        crate::storage::write_json(&index_path, &latest).unwrap();
        store.policy.patch_ratio = 1;
        store.index.generation = 1;
        store.patch_count = DEFAULT_TIMELINE_COMPACT_MIN_PATCH_COUNT;
        store.patch_bytes = 1;

        assert!(store.compact_if_needed().unwrap());
        let compacted: serde_json::Value = crate::storage::read_json(&index_path).unwrap();
        assert_eq!(compacted["generation"].as_u64(), Some(42));
    }

    #[test]
    #[ignore = "set GOLD_BAND_TIMELINE_FIXTURE to a real acp.timeline.jsonl"]
    fn release_fixture_query_is_bounded_after_one_time_migration() {
        let source = Utf8PathBuf::from(
            std::env::var("GOLD_BAND_TIMELINE_FIXTURE")
                .expect("GOLD_BAND_TIMELINE_FIXTURE must point to acp.timeline.jsonl"),
        );
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        std::fs::copy(source.as_std_path(), path.as_std_path()).unwrap();

        let first_started = Instant::now();
        let first = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        let first_elapsed = first_started.elapsed();
        let second_started = Instant::now();
        let second = read_indexed_timeline_page(&path, None, None, None, 30).unwrap();
        let second_elapsed = second_started.elapsed();
        let index_bytes = timeline_index_path(&path)
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or_default();

        eprintln!(
            "timeline fixture: first_ms={} second_ms={} canonical_records={} semantic_blocks={} returned_events={} first_tail={} second_tail={} index_bytes={}",
            first_elapsed.as_millis(),
            second_elapsed.as_millis(),
            second.event_count,
            second.total_semantic_blocks,
            second.events.len(),
            first.processed_tail_records,
            second.processed_tail_records,
            index_bytes,
        );
        assert!(second.events.len() <= 30);
        assert_eq!(second.processed_tail_records, 0);
        assert!(
            second_elapsed < std::time::Duration::from_secs(1),
            "checkpoint-backed query took {second_elapsed:?}"
        );
    }
}
