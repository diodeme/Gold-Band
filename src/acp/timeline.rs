use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;

use anyhow::Result;
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::Value;

use crate::acp::events::{
    AcpTimelineItem, AcpTimelinePatch, AcpUiEvent, load_timeline_items_for_storage_unlocked,
    merge_timeline_item_revision,
};
use crate::acp::turn_files::{FileVersionRef, TurnFileCaptureConfig, TurnFileStore};
use crate::storage::{append_jsonl_unlocked, ensure_parent_dir, with_jsonl_file_lock};

pub const DEFAULT_TIMELINE_COMPACT_MAX_SIZE_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_TIMELINE_COMPACT_PATCH_RATIO: usize = 4;
pub const TIMELINE_BLOB_MIN_BYTES: usize = 64 * 1024;
const TIMELINE_BLOB_REF_KEY: &str = "$goldBandBlob";

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineFileSignature {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl TimelineStore {
    pub fn open(path: Utf8PathBuf, policy: TimelineCompactionPolicy) -> Result<Self> {
        let (
            semantic_fingerprints,
            canonical_items,
            patch_count,
            patch_bytes,
            redundant_revision_count,
        ) = with_jsonl_file_lock(&path, || {
            let items = load_timeline_items_for_storage_unlocked(&path)?;
            let semantic_fingerprints = items
                .iter()
                .map(|item| Ok((item.id.clone(), semantic_fingerprint(item)?)))
                .collect::<Result<HashMap<_, _>>>()?;
            let canonical_items = items
                .into_iter()
                .map(|item| (item.id.clone(), item))
                .collect();
            let stats = timeline_file_stats(&path)?;
            Ok((
                semantic_fingerprints,
                canonical_items,
                stats.patch_count,
                stats.patch_bytes,
                stats.redundant_revision_count,
            ))
        })?;
        let file_signature = timeline_file_signature(&path);
        let blob_store = timeline_blob_store(&path);
        let mut store = Self {
            path,
            policy,
            semantic_fingerprints,
            canonical_items,
            patch_count,
            patch_bytes,
            redundant_revision_count,
            file_signature,
            blob_store,
        };
        store.compact_if_needed()?;
        Ok(store)
    }

    pub fn upsert(&mut self, revision: u64, item: &AcpUiEvent) -> Result<TimelineUpsertOutcome> {
        self.refresh_if_changed()?;
        let mut storage_item = item.clone();
        externalize_timeline_event(&self.blob_store, &mut storage_item)?;
        let canonical_item = self
            .canonical_items
            .get(&item.id)
            .map(|existing| merge_timeline_item_revision(existing, storage_item.clone()))
            .unwrap_or(storage_item);
        let fingerprint = semantic_fingerprint(&canonical_item)?;
        if self.semantic_fingerprints.get(&item.id) == Some(&fingerprint) {
            return Ok(TimelineUpsertOutcome::Unchanged);
        }

        let before_len = self.file_signature.len;
        with_jsonl_file_lock(&self.path, || {
            append_jsonl_unlocked(
                &self.path,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".to_string(),
                    item_id: item.id.clone(),
                    revision,
                    op: "upsert".to_string(),
                    item: canonical_item.clone(),
                },
            )
        })?;
        self.semantic_fingerprints
            .insert(item.id.clone(), fingerprint);
        self.canonical_items.insert(item.id.clone(), canonical_item);
        self.patch_count = self.patch_count.saturating_add(1);
        self.file_signature = timeline_file_signature(&self.path);
        self.patch_bytes = self
            .patch_bytes
            .saturating_add(self.file_signature.len.saturating_sub(before_len));
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

        let items = with_jsonl_file_lock(&self.path, || {
            let items = load_timeline_items_for_storage_unlocked(&self.path)?;
            write_canonical_timeline_unlocked(&self.path, &self.blob_store, &items)?;
            Ok(items)
        })?;
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
        Ok(true)
    }

    fn refresh_if_changed(&mut self) -> Result<()> {
        let current = timeline_file_signature(&self.path);
        if current == self.file_signature {
            return Ok(());
        }
        let (items, stats) = with_jsonl_file_lock(&self.path, || {
            Ok((
                load_timeline_items_for_storage_unlocked(&self.path)?,
                timeline_file_stats(&self.path)?,
            ))
        })?;
        self.semantic_fingerprints = items
            .iter()
            .map(|item| Ok((item.id.clone(), semantic_fingerprint(item)?)))
            .collect::<Result<HashMap<_, _>>>()?;
        self.canonical_items = items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        self.patch_count = stats.patch_count;
        self.patch_bytes = stats.patch_bytes;
        self.redundant_revision_count = stats.redundant_revision_count;
        self.file_signature = current;
        Ok(())
    }
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
    TimelineStore::open(path.to_path_buf(), policy)?.upsert(revision, item)
}

fn write_canonical_timeline_unlocked(
    path: &Utf8Path,
    blob_store: &TurnFileStore,
    items: &[AcpUiEvent],
) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = AtomicWriteFile::open(path.as_std_path())?;
    for item in items {
        let mut item = item.clone();
        externalize_timeline_event(blob_store, &mut item)?;
        serde_json::to_writer(&mut file, &AcpTimelineItem { item })?;
        file.write_all(b"\n")?;
    }
    file.commit()?;
    Ok(())
}

#[derive(Debug, Default)]
struct TimelineFileStats {
    patch_count: usize,
    patch_bytes: u64,
    redundant_revision_count: usize,
}

fn timeline_file_stats(path: &Utf8Path) -> Result<TimelineFileStats> {
    let Ok(content) = std::fs::read_to_string(path.as_std_path()) else {
        return Ok(TimelineFileStats::default());
    };
    let mut stats = TimelineFileStats::default();
    let mut canonical = HashMap::<String, (AcpUiEvent, u64)>::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let (item_id, item) = if let Ok(patch) = serde_json::from_str::<AcpTimelinePatch>(line) {
            if patch.patch_type != "timelinePatch" || patch.op != "upsert" {
                continue;
            }
            stats.patch_count = stats.patch_count.saturating_add(1);
            stats.patch_bytes = stats
                .patch_bytes
                .saturating_add(line.len().saturating_add(1) as u64);
            (patch.item_id, patch.item)
        } else if let Ok(entry) = serde_json::from_str::<AcpTimelineItem>(line) {
            (entry.item.id.clone(), entry.item)
        } else {
            continue;
        };
        let merged = canonical
            .get(&item_id)
            .map(|(existing, _)| merge_timeline_item_revision(existing, item.clone()))
            .unwrap_or(item);
        let fingerprint = semantic_fingerprint(&merged)?;
        if canonical
            .get(&item_id)
            .is_some_and(|(_, existing)| *existing == fingerprint)
        {
            stats.redundant_revision_count = stats.redundant_revision_count.saturating_add(1);
        }
        canonical.insert(item_id, (merged, fingerprint));
    }
    Ok(stats)
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

    use camino::Utf8PathBuf;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        TIMELINE_BLOB_MIN_BYTES, TimelineCompactionPolicy, TimelineStore, TimelineUpsertOutcome,
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
}
