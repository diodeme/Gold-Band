use super::*;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Mutex, OnceLock};

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
    pub(super) composer: Option<ComposerPosition>,
}
pub(super) struct ReadIndex {
    key: String,
    signature: TimelineFileSignature,
    pub(super) generation: u64,
    fingerprint: u64,
    pub(super) positions: HashMap<String, Position>,
    tools: BTreeSet<(u64, String)>,
    pub(super) composer: BTreeSet<(u64, String)>,
    pub(super) composer_by_prompt: HashMap<String, BTreeSet<(u64, String)>>,
    string_bytes: usize,
}

impl ReadIndex {
    fn bytes(&self) -> usize {
        self.positions.capacity() * (std::mem::size_of::<(String, Position)>() + 1)
            + self.string_bytes
            + self.key.capacity()
            + self.tools.len() * 96
            + self.composer.len() * 192
            + self.composer_by_prompt.capacity()
                * (std::mem::size_of::<(String, BTreeSet<(u64, String)>)>() + 1)
    }

    fn remove_secondary_indexes(&mut self, id: &str, position: &Position) {
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
                self.insert(
                    event.id,
                    Position {
                        offset,
                        length,
                        revision,
                        started_seq,
                        tool,
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
                composer: BTreeSet::new(),
                composer_by_prompt: HashMap::new(),
                string_bytes: 0,
            };
            for (id, locator) in index.item_locators {
                let composer = composer_position_from_locator(&id, &locator);
                projection.insert(
                    id,
                    Position {
                        offset: locator.offset,
                        length: locator.line_length,
                        revision: locator.revision,
                        started_seq: locator.started_seq,
                        tool: matches!(locator.kind.as_str(), "toolCall" | "toolCallUpdate"),
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
                composer: None,
            },
        );
        assert!(index.tools.is_empty());
    }
}
