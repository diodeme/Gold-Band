use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::time::Instant;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::events::{
    AcpTimelineItem, AcpTimelinePatch, AcpTimingPatch, AcpUiEvent,
    extract_agent_transcript_relation, extract_usage_fields,
    load_timeline_items_for_storage_unlocked, merge_timeline_item_revision,
};
use crate::acp::turn_files::{FileVersionRef, TurnFileCaptureConfig, TurnFileStore};
use crate::artifacts::json_artifact_display_span;
use crate::storage::{
    append_jsonl_flushed_unlocked, atomic_write_file, ensure_parent_dir, with_jsonl_file_lock,
};

pub const DEFAULT_TIMELINE_COMPACT_MAX_SIZE_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_TIMELINE_COMPACT_PATCH_RATIO: usize = 4;
pub const TIMELINE_BLOB_MIN_BYTES: usize = 64 * 1024;
// V3 adds the retry-prompt role and its current pending identity. Treating a V2
// index as compatible would leave stop unable to settle a processing retry in
// the crash window between the timeline append and session metadata rewrite.
pub const TIMELINE_INDEX_FORMAT_VERSION: u32 = 3;
pub const DEFAULT_TIMELINE_CHECKPOINT_PATCH_INTERVAL: usize = 256;
pub const DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT: usize = 256;
const TIMELINE_BLOB_REF_KEY: &str = "$goldBandBlob";

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
    runtime_control_candidate: bool,
    #[serde(default)]
    retry_prompt: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<AcpUiEvent>,
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
    latest_runtime_control_candidate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_retry_prompt_id: Option<String>,
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
            latest_runtime_control_candidate: None,
            pending_retry_prompt_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimelineIndexedPage {
    pub generation: u64,
    pub covered_revision: u64,
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
    semantic_fingerprints: HashMap<String, u64>,
    canonical_items: HashMap<String, AcpUiEvent>,
    patch_count: usize,
    patch_bytes: u64,
    redundant_revision_count: usize,
    file_signature: TimelineFileSignature,
    blob_store: TurnFileStore,
    index: TimelineMaterializedIndex,
    checkpoint_policy: TimelineCheckpointPolicy,
    dirty_patch_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineFileSignature {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl TimelineStore {
    pub fn open(path: Utf8PathBuf, policy: TimelineCompactionPolicy) -> Result<Self> {
        let checkpoint_policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(&path);
        let (index, replayed) = with_jsonl_file_lock(&path, || {
            load_or_rebuild_index_unlocked(&path, &index_path, checkpoint_policy)
        })?;
        let semantic_fingerprints = index
            .item_locators
            .iter()
            .map(|(item_id, locator)| (item_id.clone(), locator.fingerprint))
            .collect();
        let stats = timeline_file_stats_from_index(&index);
        let file_signature = timeline_file_signature(&path);
        let blob_store = timeline_blob_store(&path);
        let mut store = Self {
            path,
            policy,
            semantic_fingerprints,
            canonical_items: HashMap::new(),
            patch_count: stats.patch_count,
            patch_bytes: stats.patch_bytes,
            redundant_revision_count: stats.redundant_revision_count,
            file_signature,
            blob_store,
            index,
            checkpoint_policy,
            dirty_patch_count: replayed,
        };
        if store.dirty_patch_count > 0 {
            store.force_checkpoint()?;
        }
        store.compact_if_needed()?;
        Ok(store)
    }

    pub fn upsert(&mut self, revision: u64, item: &AcpUiEvent) -> Result<TimelineUpsertOutcome> {
        let mut storage_item = item.clone();
        externalize_timeline_event(&self.blob_store, &mut storage_item)?;
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
            let existing = self
                .index
                .item_locators
                .get(&item.id)
                .map(|locator| read_event_at_locator(&path, locator))
                .transpose()?;
            let canonical_item = existing
                .as_ref()
                .map(|existing| merge_timeline_item_revision(existing, storage_item.clone()))
                .unwrap_or(storage_item);
            let fingerprint = semantic_fingerprint(&canonical_item)?;
            if self.semantic_fingerprints.get(&item.id) == Some(&fingerprint) {
                return Ok(None);
            }

            let revision = revision.max(self.index.covered_revision.saturating_add(1));
            let patch = AcpTimelinePatch {
                patch_type: "timelinePatch".to_string(),
                item_id: item.id.clone(),
                revision,
                op: "upsert".to_string(),
                item: canonical_item.clone(),
            };
            let offset = timeline_file_len(&path);
            let line_length = serde_json::to_vec(&patch)?.len().saturating_add(1) as u64;
            append_jsonl_flushed_unlocked(&path, &patch)?;
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
            if self.dirty_patch_count >= checkpoint_policy.patch_interval.max(1) {
                checkpoint_index_unlocked(&path, &mut self.index)?;
                self.dirty_patch_count = 0;
            }
            Ok(Some((canonical_item, fingerprint, line_length)))
        })?;
        let Some((canonical_item, fingerprint, line_length)) = mutation else {
            return Ok(TimelineUpsertOutcome::Unchanged);
        };
        self.semantic_fingerprints
            .insert(item.id.clone(), fingerprint);
        self.canonical_items.insert(item.id.clone(), canonical_item);
        self.patch_count = self.patch_count.saturating_add(1);
        self.patch_bytes = self.patch_bytes.saturating_add(line_length);
        self.file_signature = timeline_file_signature(&self.path);
        if self.compact_if_needed()? {
            Ok(TimelineUpsertOutcome::AppendedAndCompacted)
        } else {
            Ok(TimelineUpsertOutcome::Appended)
        }
    }

    pub fn compact_if_needed(&mut self) -> Result<bool> {
        let unique_items = self.semantic_fingerprints.len().max(1);
        let patch_heavy = self.patch_count > unique_items.saturating_mul(self.policy.patch_ratio);
        let patch_bytes_heavy = self.patch_bytes > self.policy.max_size_bytes;
        if !patch_bytes_heavy && !patch_heavy && self.redundant_revision_count == 0 {
            return Ok(false);
        }

        let index_path = timeline_index_path(&self.path);
        let (items, index, compacted) = with_jsonl_file_lock(&self.path, || {
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
                > current_unique_items.saturating_mul(self.policy.patch_ratio);
            let current_patch_bytes_heavy = current_stats.patch_bytes > self.policy.max_size_bytes;
            if !current_patch_bytes_heavy && !current_patch_heavy {
                return Ok((Vec::new(), current_index, false));
            }

            let items = load_timeline_items_for_storage_unlocked(&self.path)?;
            write_canonical_timeline_unlocked(&self.path, &self.blob_store, &items)?;
            let mut index = rebuild_index_unlocked(&self.path)?.0;
            index.generation = current_index.generation.saturating_add(1).max(1);
            persist_index_unlocked(&self.path, &index)?;
            Ok((items, index, true))
        })?;
        if !compacted {
            self.replace_index_projection(index);
            return Ok(false);
        }
        self.semantic_fingerprints = items
            .iter()
            .map(|item| Ok((item.id.clone(), semantic_fingerprint(item)?)))
            .collect::<Result<HashMap<_, _>>>()?;
        self.canonical_items = items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
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
        self.semantic_fingerprints = index
            .item_locators
            .iter()
            .map(|(item_id, locator)| (item_id.clone(), locator.fingerprint))
            .collect();
        self.canonical_items.clear();
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
                    return Ok((rebuilt, 0));
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
    Ok((index, 0))
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
            apply_index_event(&mut index, &item, revision, offset, line_length)?;
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
    let should_replace = index
        .item_locators
        .get(&item.id)
        .is_none_or(|locator| revision >= locator.revision);
    if !should_replace {
        return Ok(());
    }
    let locator = timeline_item_locator(item, revision, offset, line_length)?;
    index.covered_revision = index.covered_revision.max(revision).max(locator.ended_seq);
    apply_lightweight_projection(index, item);
    let replaced_latest_candidate =
        index.latest_runtime_control_candidate.as_deref() == Some(item.id.as_str());
    let runtime_control_candidate = locator.runtime_control_candidate;
    let replaced_pending_retry = index.pending_retry_prompt_id.as_deref() == Some(item.id.as_str());
    let processing_retry = locator.retry_prompt && locator.status.as_deref() == Some("processing");
    let candidate_order = (locator.ended_seq, locator.seq, locator.revision);
    index.item_locators.insert(item.id.clone(), locator);
    if runtime_control_candidate {
        let should_replace = index
            .latest_runtime_control_candidate
            .as_ref()
            .and_then(|candidate_id| index.item_locators.get(candidate_id))
            .is_none_or(|current| {
                candidate_order >= (current.ended_seq, current.seq, current.revision)
            });
        if should_replace {
            index.latest_runtime_control_candidate = Some(item.id.clone());
        }
    } else if replaced_latest_candidate {
        index.latest_runtime_control_candidate = index
            .item_locators
            .iter()
            .filter(|(_, locator)| locator.runtime_control_candidate)
            .max_by_key(|(_, locator)| (locator.ended_seq, locator.seq, locator.revision))
            .map(|(item_id, _)| item_id.clone());
    }
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
    let relation = raw.and_then(extract_agent_transcript_relation);
    let agent_launch = relation
        .as_ref()
        .is_some_and(|relation| relation.agent_launch);
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
        runtime_control_candidate: item.kind == "textDelta"
            && item
                .content
                .as_deref()
                .and_then(json_artifact_display_span)
                .is_some(),
        retry_prompt: item
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/retry/attempt"))
            .and_then(Value::as_u64)
            .is_some_and(|attempt| attempt > 0),
    })
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
    if let Some(relation) = item
        .raw
        .as_ref()
        .and_then(extract_agent_transcript_relation)
        && relation.agent_launch
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
                summary: None,
            }),
            TimelineSemanticKind::Activity => {
                if let Some(block) = blocks.last_mut().filter(|block| block.activity) {
                    block.item_ids.push(item_id.clone());
                    block.oldest_seq = block.oldest_seq.min(locator.started_seq);
                    block.newest_seq = block.newest_seq.max(locator.ended_seq);
                } else {
                    blocks.push(TimelineSemanticBlockIndex {
                        activity: true,
                        item_ids: vec![item_id.clone()],
                        oldest_seq: locator.started_seq,
                        newest_seq: locator.ended_seq,
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
    limit: usize,
) -> Result<TimelineIndexedPage> {
    let started_at = Instant::now();
    let policy = TimelineCheckpointPolicy::default();
    let index_path = timeline_index_path(path);
    let (index, processed_tail_records) = with_jsonl_file_lock(path, || {
        load_or_rebuild_index_unlocked(path, &index_path, policy)
    })?;
    let total = index.semantic_blocks.len();
    let selected = if let Some(cursor) = after_seq {
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
            .filter(|block| block.newest_seq < cursor)
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
        })
    })
}

pub fn annotate_latest_runtime_control_output(
    path: &Utf8Path,
    artifact_name: &str,
    kind: &str,
) -> Result<bool> {
    with_jsonl_file_lock(path, || {
        let policy = TimelineCheckpointPolicy::default();
        let index_path = timeline_index_path(path);
        let (mut index, _) = load_or_rebuild_index_unlocked(path, &index_path, policy)?;
        let Some(item_id) = index.latest_runtime_control_candidate.clone() else {
            return Ok(false);
        };
        let Some(locator) = index.item_locators.get(&item_id).cloned() else {
            return Ok(false);
        };
        let mut event = read_event_at_locator(path, &locator)?;
        let Some(content) = event.content.as_deref() else {
            return Ok(false);
        };
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
        for item in items {
            let mut item = item.clone();
            externalize_timeline_event(blob_store, &mut item)?;
            serde_json::to_writer(&mut *file, &AcpTimelineItem { item })?;
            file.write_all(b"\n")?;
        }
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
    use std::io::Write as _;
    use std::time::Instant;

    use camino::Utf8PathBuf;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        TIMELINE_BLOB_MIN_BYTES, TIMELINE_INDEX_FORMAT_VERSION, TimelineCompactionPolicy,
        TimelineSettleOutcome, TimelineStore, TimelineUpsertOutcome, read_indexed_timeline_page,
        read_indexed_timeline_projection, settle_latest_processing_retry_prompt,
        settle_timeline_item_status, timeline_index_path,
    };
    use crate::acp::events::{AcpTimelinePatch, AcpUiEvent, load_timeline_items};

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
        store.upsert(2, &event("message-1", 2, "hello")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
        assert_eq!(
            load_timeline_items(&path).unwrap()[0].content.as_deref(),
            Some("hello")
        );
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

        let recovered = read_indexed_timeline_page(&path, None, None, 30).unwrap();
        assert_eq!(recovered.processed_tail_records, 1);
        assert_eq!(recovered.events.len(), 2);
        let reopened = read_indexed_timeline_page(&path, None, None, 30).unwrap();
        assert_eq!(reopened.processed_tail_records, 0);
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
            read_indexed_timeline_page(&path, None, None, 30)
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
            read_indexed_timeline_page(&path, None, None, 30)
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

        let migrated = read_indexed_timeline_page(&path, None, None, 30).unwrap();
        assert_eq!(migrated.events.len(), 1);
        let page = read_indexed_timeline_page(&path, None, None, 30).unwrap();
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
                max_size_bytes: u64::MAX,
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

        let page = read_indexed_timeline_page(&path, None, None, 30).unwrap();
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
        crate::storage::write_json(&index_path, &latest).unwrap();
        store.policy.patch_ratio = 1;
        store.index.generation = 1;
        store.patch_count = 2;
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
        let first = read_indexed_timeline_page(&path, None, None, 30).unwrap();
        let first_elapsed = first_started.elapsed();
        let second_started = Instant::now();
        let second = read_indexed_timeline_page(&path, None, None, 30).unwrap();
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
