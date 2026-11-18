use super::*;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
thread_local! { static POSITION_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

const MAX_READ_INDEXES: usize = 4;
const MAX_READ_INDEX_BYTES: usize = 32 * 1024 * 1024;

pub(super) struct ComposerPosition {
    pub(super) text_bytes: usize,
    pub(super) prompt_identity: String,
}
pub(super) struct Position {
    pub(super) offset: u64,
    pub(super) length: u64,
    pub(super) revision: u64,
    pub(super) started_seq: u64,
    tool: bool,
    navigation_rank: Option<usize>,
    pub(super) composer: Option<ComposerPosition>,
}
pub(super) struct ReadIndex {
    key: String,
    signature: TimelineFileSignature,
    pub(super) generation: u64,
    fingerprint: u64,
    pub(super) positions: HashMap<String, Position>,
    tools: BTreeSet<(u64, String)>,
    navigation: [BTreeSet<(u64, String)>; 4],
    pub(super) composer: BTreeSet<(u64, String)>,
    pub(super) composer_by_prompt: HashMap<String, BTreeSet<(u64, String)>>,
    string_bytes: usize,
}

impl ReadIndex {
    fn bytes(&self) -> usize {
        self.positions.capacity() * (std::mem::size_of::<(String, Position)>() + 1)
            + self.string_bytes
            + self.key.capacity()
            + self
                .navigation
                .iter()
                .map(|set| set.len() * 96)
                .sum::<usize>()
            + self.tools.len() * 96
            + self.composer.len() * 192
            + self.composer_by_prompt.capacity()
                * (std::mem::size_of::<(String, BTreeSet<(u64, String)>)>() + 1)
    }

    fn remove_secondary_indexes(&mut self, id: &str, position: &Position) {
        if let Some(rank) = position.navigation_rank {
            self.navigation[rank].remove(&(position.started_seq, id.to_owned()));
            self.string_bytes = self.string_bytes.saturating_sub(id.len());
        }
        if position.tool {
            self.tools.remove(&(position.started_seq, id.to_owned()));
            self.string_bytes = self.string_bytes.saturating_sub(id.len());
        }
        let Some(composer) = position.composer.as_ref() else {
            return;
        };
        let order = (position.started_seq, id.to_owned());
        if self.composer.remove(&order) {
            self.string_bytes = self.string_bytes.saturating_sub(id.len());
        }
        let remove_prompt = self
            .composer_by_prompt
            .get_mut(&composer.prompt_identity)
            .is_some_and(|positions| {
                if positions.remove(&order) {
                    self.string_bytes = self.string_bytes.saturating_sub(id.len());
                }
                positions.is_empty()
            });
        if remove_prompt
            && let Some((prompt, _)) = self
                .composer_by_prompt
                .remove_entry(&composer.prompt_identity)
        {
            self.string_bytes = self.string_bytes.saturating_sub(prompt.capacity());
        }
        self.string_bytes = self
            .string_bytes
            .saturating_sub(composer.prompt_identity.capacity());
    }

    fn add_secondary_indexes(&mut self, id: &str, position: &Position) {
        if let Some(rank) = position.navigation_rank {
            self.navigation[rank].insert((position.started_seq, id.to_owned()));
            self.string_bytes += id.len();
        }
        if position.tool {
            self.tools.insert((position.started_seq, id.to_owned()));
            self.string_bytes += id.len();
        }
        let Some(composer) = position.composer.as_ref() else {
            return;
        };
        let order = (position.started_seq, id.to_owned());
        self.composer.insert(order.clone());
        self.string_bytes += id.len() + composer.prompt_identity.capacity();
        match self
            .composer_by_prompt
            .entry(composer.prompt_identity.clone())
        {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().insert(order);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                self.string_bytes += composer.prompt_identity.len();
                entry.insert(BTreeSet::from([order]));
            }
        }
        self.string_bytes += id.len();
    }

    fn insert(&mut self, id: String, position: Position) {
        if let Some((stored_id, previous)) = self.positions.remove_entry(&id) {
            self.remove_secondary_indexes(&stored_id, &previous);
            self.add_secondary_indexes(&stored_id, &position);
            self.positions.insert(stored_id, position);
        } else {
            self.string_bytes += id.capacity();
            self.add_secondary_indexes(&id, &position);
            self.positions.insert(id, position);
        }
    }

    fn refresh(&mut self, path: &Utf8Path) -> Result<bool> {
        let signature = timeline_file_signature(path);
        if signature == self.signature {
            return Ok(true);
        }
        if signature.len <= self.signature.len
            || timeline_prefix_fingerprint(path, self.signature.len)? != self.fingerprint
        {
            return Ok(false);
        }
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(self.signature.len))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut offset = self.signature.len;
        for _ in 0..DEFAULT_TIMELINE_TAIL_REPLAY_LIMIT {
            line.clear();
            let length = reader.read_line(&mut line)? as u64;
            if length == 0 {
                break;
            }
            if let Some((revision, event, _)) = parse_timeline_record(&line) {
                let started_seq = event.started_seq.unwrap_or(event.seq);
                let tool = matches!(event.kind.as_str(), "toolCall" | "toolCallUpdate");
                let composer = composer_position_from_event(&event);
                let navigation_rank = navigation_rank(&event.id, &event.kind);
                self.insert(
                    event.id,
                    Position {
                        offset,
                        length,
                        revision,
                        started_seq,
                        tool,
                        navigation_rank,
                        composer,
                    },
                );
            }
            offset += length;
        }
        if offset != signature.len {
            return Ok(false);
        }
        self.fingerprint = timeline_prefix_fingerprint(path, offset)?;
        self.signature = signature;
        Ok(true)
    }
}

fn indexes() -> &'static Mutex<VecDeque<ReadIndex>> {
    static INDEXES: OnceLock<Mutex<VecDeque<ReadIndex>>> = OnceLock::new();
    INDEXES.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn retain_index(entries: &mut VecDeque<ReadIndex>, index: ReadIndex) {
    let bytes = index.bytes();
    if bytes > MAX_READ_INDEX_BYTES {
        return;
    }
    while entries.len() >= MAX_READ_INDEXES
        || entries.iter().map(ReadIndex::bytes).sum::<usize>() + bytes > MAX_READ_INDEX_BYTES
    {
        entries.pop_front();
    }
    entries.push_back(index);
}

pub(super) fn read(path: &Utf8Path, id: &str) -> Result<Option<TimelineIndexedItem>> {
    with_index(path, |index| read_position(path, index, id))
}

pub(super) fn read_position(
    path: &Utf8Path,
    index: &ReadIndex,
    id: &str,
) -> Result<Option<TimelineIndexedItem>> {
    index
        .positions
        .get(id)
        .map(|position| {
            #[cfg(test)]
            POSITION_READS.set(POSITION_READS.get() + 1);
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(position.offset))?;
            let mut bytes = vec![0; position.length as usize];
            file.read_exact(&mut bytes)?;
            let (_, event, _) = parse_timeline_record(std::str::from_utf8(&bytes)?)
                .ok_or_else(|| anyhow::anyhow!("acp.timeline-index-locator-corrupt"))?;
            ensure!(event.id == id, "acp.timeline-index-locator-corrupt");
            Ok(TimelineIndexedItem {
                event,
                generation: index.generation,
                revision: position.revision,
            })
        })
        .transpose()
}

pub(super) fn with_index<T>(
    path: &Utf8Path,
    operation: impl FnOnce(&ReadIndex) -> Result<T>,
) -> Result<T> {
    let key = crate::storage::normalize_workspace_path(path);
    with_jsonl_file_lock(path, || {
        // The per-file lock provides single-flight. The LRU lock never covers I/O.
        let mut cached = {
            let mut entries = indexes().lock().unwrap_or_else(|e| e.into_inner());
            entries
                .iter()
                .position(|entry| entry.key == key)
                .and_then(|index| entries.remove(index))
        };
        if !cached
            .as_mut()
            .map(|index| index.refresh(path))
            .transpose()?
            .unwrap_or(false)
        {
            let (index, _) = load_or_rebuild_index_unlocked(
                path,
                &timeline_index_path(path),
                Default::default(),
            )?;
            let mut projection = ReadIndex {
                key,
                signature: timeline_file_signature(path),
                generation: index.generation,
                fingerprint: timeline_prefix_fingerprint(path, timeline_file_len(path))?,
                positions: HashMap::with_capacity(index.item_locators.len()),
                tools: BTreeSet::new(),
                navigation: Default::default(),
                composer: BTreeSet::new(),
                composer_by_prompt: HashMap::new(),
                string_bytes: 0,
            };
            for (id, locator) in index.item_locators {
                let composer = composer_position_from_locator(&id, &locator);
                projection.insert(
                    id.clone(),
                    Position {
                        offset: locator.offset,
                        length: locator.line_length,
                        revision: locator.revision,
                        started_seq: locator.started_seq,
                        tool: matches!(locator.kind.as_str(), "toolCall" | "toolCallUpdate"),
                        navigation_rank: navigation_rank(&id, &locator.kind),
                        composer,
                    },
                );
            }
            cached = Some(projection);
        }
        let index = cached.unwrap();
        let result = operation(&index);
        retain_index(
            &mut indexes().lock().unwrap_or_else(|e| e.into_inner()),
            index,
        );
        result
    })
}

fn composer_position_from_locator(
    id: &str,
    locator: &TimelineItemLocator,
) -> Option<ComposerPosition> {
    super::composer_history::eligible(locator).then(|| ComposerPosition {
        text_bytes: locator.composer_text_bytes.unwrap_or_default(),
        prompt_identity: locator.prompt_id.clone().unwrap_or_else(|| id.to_owned()),
    })
}

fn composer_position_from_event(event: &AcpUiEvent) -> Option<ComposerPosition> {
    let raw = event.raw.as_ref()?;
    if event.kind != "userTextDelta"
        || raw.get("source").and_then(Value::as_str) != Some("goldBandPrompt")
        || raw
            .get("hiddenFromChat")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || raw
            .pointer("/_meta/goldBandConversation/branchId")
            .and_then(Value::as_str)
            .filter(|branch_id| !branch_id.trim().is_empty())
            .unwrap_or("root")
            != "root"
    {
        return None;
    }
    Some(ComposerPosition {
        text_bytes: super::composer_history::original_user_text(event)?.len(),
        prompt_identity: timeline_prompt_id(event).unwrap_or_else(|| event.id.clone()),
    })
}

fn navigation_rank(id: &str, kind: &str) -> Option<usize> {
    match kind {
        "userTextDelta" if id.starts_with("gold-band-user-prompt-") => Some(0),
        "permissionRequest" | "permission" => Some(3),
        kind if kind.starts_with("compaction") => Some(2),
        "toolCall" | "toolCallUpdate" => Some(1),
        _ => None,
    }
}

pub const TURN_INDEX_PAGE_SIZE: usize = 64;

/// Reads a bounded navigation projection from the existing locator LRU. Tool
/// bodies and artifacts are never opened; category membership is a range lookup.
pub fn read_conversation_turn_page(
    path: &Utf8Path,
    before_seq: Option<u64>,
    after_seq: Option<u64>,
) -> Result<Vec<crate::acp::events::ConversationTurnIndexEntry>> {
    use crate::acp::events::{
        CONVERSATION_TURN_PREVIEW_CHARS, ConversationTurnIndexEntry, ConversationTurnKind,
        normalize_timeline_timestamp,
    };
    use std::ops::Bound;
    with_index(path, |index| {
        let prompts = &index.navigation[0];
        let mut selected: Vec<_> = if let Some(after) = after_seq {
            prompts
                .range((
                    Bound::Included((after.saturating_add(1), String::new())),
                    Bound::Unbounded,
                ))
                .take(TURN_INDEX_PAGE_SIZE)
                .collect()
        } else {
            let upper = before_seq
                .map(|seq| Bound::Excluded((seq, String::new())))
                .unwrap_or(Bound::Unbounded);
            prompts
                .range((Bound::Unbounded, upper))
                .rev()
                .take(TURN_INDEX_PAGE_SIZE)
                .collect()
        };
        selected.sort();
        selected
            .into_iter()
            .map(|order @ (seq, id)| {
                let end = prompts
                    .range((Bound::Excluded(order.clone()), Bound::Unbounded))
                    .next()
                    .map(|(seq, _)| *seq)
                    .unwrap_or(u64::MAX);
                let rank = (1..=3)
                    .rev()
                    .find(|rank| {
                        index.navigation[*rank]
                            .range((
                                Bound::Included((*seq, String::new())),
                                Bound::Excluded((end, String::new())),
                            ))
                            .next()
                            .is_some()
                    })
                    .unwrap_or(0);
                let event = read_position(path, index, id)?
                    .ok_or_else(|| anyhow::anyhow!("acp.timeline-index-locator-corrupt"))?
                    .event;
                Ok(ConversationTurnIndexEntry {
                    seq: *seq,
                    timestamp: normalize_timeline_timestamp(&event.timestamp),
                    kind: match rank {
                        3 => ConversationTurnKind::Permission,
                        2 => ConversationTurnKind::Compaction,
                        1 => ConversationTurnKind::Tool,
                        _ => ConversationTurnKind::Prompt,
                    },
                    prompt_preview: event
                        .content
                        .map(|text| text.chars().take(CONVERSATION_TURN_PREVIEW_CHARS).collect()),
                })
            })
            .collect()
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityImagePage {
    pub images: Vec<crate::acp::images::AcpImageRef>,
    pub next_cursor: Option<String>,
    pub generation: u64,
}

pub fn read_activity_image_page(
    path: &Utf8Path,
    start: u64,
    end: u64,
    after: Option<&str>,
    generation: Option<u64>,
) -> Result<ActivityImagePage> {
    use std::ops::Bound;
    with_index(path, |index| {
        ensure!(
            generation.is_none_or(|generation| generation == index.generation),
            "acp.image-stale-generation"
        );
        ensure!(start <= end, "acp.image-invalid-range");
        let lower = match after {
            Some(id) => {
                let position = index
                    .positions
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("acp.image-invalid-cursor"))?;
                ensure!(
                    position.tool && position.started_seq >= start && position.started_seq <= end,
                    "acp.image-invalid-cursor"
                );
                Bound::Excluded((position.started_seq, id.to_string()))
            }
            None => Bound::Included((start, String::new())),
        };
        let mut candidates = index
            .tools
            .range((lower, Bound::Unbounded))
            .take_while(|(seq, _)| *seq <= end);
        let mut images = Vec::new();
        let mut last = None;
        for (_, id) in candidates.by_ref().take(32) {
            let item = read_position(path, index, id)?.unwrap();
            images.extend(
                crate::acp::images::image_refs(&item.event)
                    .into_iter()
                    .take(crate::acp::images::MAX_PROJECTED_IMAGES.saturating_sub(images.len())),
            );
            last = Some(id.clone());
            if images.len() == crate::acp::images::MAX_PROJECTED_IMAGES {
                break;
            }
        }
        Ok(ActivityImagePage {
            images,
            next_cursor: candidates.next().and(last),
            generation: index.generation,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_evicts_old_indexes_and_rejects_oversized_projections() {
        let make_index = |key: String| ReadIndex {
            key,
            signature: TimelineFileSignature {
                len: 0,
                modified: None,
            },
            generation: 1,
            fingerprint: 0,
            positions: HashMap::new(),
            tools: BTreeSet::new(),
            navigation: Default::default(),
            composer: BTreeSet::new(),
            composer_by_prompt: HashMap::new(),
            string_bytes: 0,
        };
        let mut entries = VecDeque::new();
        for id in 0..6 {
            retain_index(&mut entries, make_index(id.to_string()));
        }
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>(),
            ["2", "3", "4", "5"]
        );
        retain_index(
            &mut entries,
            make_index("x".repeat(MAX_READ_INDEX_BYTES + 1)),
        );
        assert_eq!(entries.len(), 4);
        retain_index(&mut entries, make_index("y".repeat(MAX_READ_INDEX_BYTES)));
        assert_eq!(entries.len(), 1);
        assert!(entries.iter().map(ReadIndex::bytes).sum::<usize>() <= MAX_READ_INDEX_BYTES);
    }

    #[test]
    fn tool_order_tracks_replaced_locators() {
        let mut index = ReadIndex {
            key: String::new(),
            signature: TimelineFileSignature {
                len: 0,
                modified: None,
            },
            generation: 1,
            fingerprint: 0,
            positions: HashMap::new(),
            tools: BTreeSet::new(),
            navigation: Default::default(),
            composer: BTreeSet::new(),
            composer_by_prompt: HashMap::new(),
            string_bytes: 0,
        };
        index.insert(
            "tool".into(),
            Position {
                offset: 0,
                length: 10,
                revision: 1,
                started_seq: 1,
                tool: true,
                navigation_rank: Some(1),
                composer: None,
            },
        );
        index.insert(
            "tool".into(),
            Position {
                offset: 10,
                length: 10,
                revision: 2,
                started_seq: 2,
                tool: true,
                navigation_rank: Some(1),
                composer: None,
            },
        );
        assert_eq!(
            index.tools.iter().cloned().collect::<Vec<_>>(),
            vec![(2, "tool".into())]
        );
        index.insert(
            "tool".into(),
            Position {
                offset: 20,
                length: 10,
                revision: 3,
                started_seq: 2,
                tool: false,
                navigation_rank: None,
                composer: None,
            },
        );
        assert!(index.tools.is_empty());
    }
    #[test]
    fn turn_navigation_pages_bound_reads_and_refresh_categories() {
        use crate::acp::events::{ConversationTurnKind, user_prompt_event};
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        for turn in 0..130_u64 {
            let seq = turn * 3 + 1;
            let prompt = user_prompt_event(
                seq,
                "s".into(),
                "多字节预览".repeat(100),
                Some(format!("prompt-{seq}")),
                false,
                vec![],
            );
            store.upsert(seq, &prompt).unwrap();
            let mut tool = prompt.clone();
            tool.id = format!("tool-{seq}");
            tool.kind = "toolCall".into();
            tool.seq = seq + 1;
            tool.started_seq = Some(seq + 1);
            tool.ended_seq = Some(seq + 1);
            tool.raw = None;
            tool.content = Some("heavy tool body".repeat(1000));
            store.upsert(seq + 1, &tool).unwrap();
        }
        POSITION_READS.set(0);
        let latest = read_conversation_turn_page(&path, None, None).unwrap();
        assert_eq!(latest.len(), TURN_INDEX_PAGE_SIZE);
        assert_eq!(
            POSITION_READS.get(),
            TURN_INDEX_PAGE_SIZE,
            "only selected prompt records may be read"
        );
        assert!(
            latest
                .iter()
                .all(|item| item.kind == ConversationTurnKind::Tool)
        );
        assert!(
            latest
                .iter()
                .all(|item| item.prompt_preview.as_ref().unwrap().chars().count() == 80)
        );
        let older = read_conversation_turn_page(&path, Some(latest[0].seq), None).unwrap();
        assert_eq!(older.len(), TURN_INDEX_PAGE_SIZE);
        assert!(older.last().unwrap().seq < latest[0].seq);
        let first = read_conversation_turn_page(&path, Some(older[0].seq), None).unwrap();
        assert_eq!(first.len(), 2);
        let last_seq = latest.last().unwrap().seq;
        let mut permission =
            user_prompt_event(last_seq + 2, "s".into(), "".into(), None, false, vec![]);
        permission.id = "permission-last".into();
        permission.kind = "permissionRequest".into();
        store.upsert(last_seq + 2, &permission).unwrap();
        POSITION_READS.set(0);
        let refreshed = read_conversation_turn_page(&path, None, Some(last_seq - 1)).unwrap();
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].kind, ConversationTurnKind::Permission);
        assert_eq!(POSITION_READS.get(), 1);
    }
}
