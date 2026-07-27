use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;

use anyhow::Result;
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::Value;

use crate::acp::events::{
    AcpTimelineItem, AcpTimelinePatch, AcpUiEvent, load_timeline_items_unlocked,
    merge_timeline_item_revision,
};
use crate::storage::{append_jsonl_unlocked, ensure_parent_dir, with_jsonl_file_lock};

pub const DEFAULT_TIMELINE_COMPACT_MAX_SIZE_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_TIMELINE_COMPACT_PATCH_RATIO: usize = 4;

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
    redundant_revision_count: usize,
    file_signature: TimelineFileSignature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineFileSignature {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl TimelineStore {
    pub fn open(path: Utf8PathBuf, policy: TimelineCompactionPolicy) -> Result<Self> {
        let (semantic_fingerprints, canonical_items, patch_count, redundant_revision_count) =
            with_jsonl_file_lock(&path, || {
                let items = load_timeline_items_unlocked(&path)?;
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
                    stats.redundant_revision_count,
                ))
            })?;
        let file_signature = timeline_file_signature(&path);
        let mut store = Self {
            path,
            policy,
            semantic_fingerprints,
            canonical_items,
            patch_count,
            redundant_revision_count,
            file_signature,
        };
        store.compact_if_needed()?;
        Ok(store)
    }

    pub fn upsert(&mut self, revision: u64, item: &AcpUiEvent) -> Result<TimelineUpsertOutcome> {
        self.refresh_if_changed()?;
        let canonical_item = self
            .canonical_items
            .get(&item.id)
            .map(|existing| merge_timeline_item_revision(existing, item.clone()))
            .unwrap_or_else(|| item.clone());
        let fingerprint = semantic_fingerprint(&canonical_item)?;
        if self.semantic_fingerprints.get(&item.id) == Some(&fingerprint) {
            return Ok(TimelineUpsertOutcome::Unchanged);
        }

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
        if self.compact_if_needed()? {
            Ok(TimelineUpsertOutcome::AppendedAndCompacted)
        } else {
            Ok(TimelineUpsertOutcome::Appended)
        }
    }

    pub fn compact_if_needed(&mut self) -> Result<bool> {
        let bytes = std::fs::metadata(self.path.as_std_path())
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        let unique_items = self.semantic_fingerprints.len().max(1);
        let patch_heavy = self.patch_count > unique_items.saturating_mul(self.policy.patch_ratio);
        if bytes <= self.policy.max_size_bytes && !patch_heavy && self.redundant_revision_count == 0
        {
            return Ok(false);
        }

        let items = with_jsonl_file_lock(&self.path, || {
            let items = load_timeline_items_unlocked(&self.path)?;
            write_canonical_timeline_unlocked(&self.path, &items)?;
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
                load_timeline_items_unlocked(&self.path)?,
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

fn write_canonical_timeline_unlocked(path: &Utf8Path, items: &[AcpUiEvent]) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = AtomicWriteFile::open(path.as_std_path())?;
    for item in items {
        serde_json::to_writer(&mut file, &AcpTimelineItem { item: item.clone() })?;
        file.write_all(b"\n")?;
    }
    file.commit()?;
    Ok(())
}

#[derive(Debug, Default)]
struct TimelineFileStats {
    patch_count: usize,
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

    use super::{TimelineCompactionPolicy, TimelineStore, TimelineUpsertOutcome};
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
}
