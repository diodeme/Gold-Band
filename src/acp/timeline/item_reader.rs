use super::*;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Mutex, OnceLock};

const MAX_READ_INDEXES: usize = 4;
const MAX_READ_INDEX_BYTES: usize = 32 * 1024 * 1024;

struct Position {
    offset: u64,
    length: u64,
    revision: u64,
    started_seq: u64,
    tool: bool,
}
struct ReadIndex {
    key: String,
    signature: TimelineFileSignature,
    generation: u64,
    fingerprint: u64,
    positions: HashMap<String, Position>,
    tools: BTreeSet<(u64, String)>,
    string_bytes: usize,
}

impl ReadIndex {
    fn bytes(&self) -> usize {
        self.positions.capacity() * (std::mem::size_of::<(String, Position)>() + 1)
            + self.string_bytes
            + self.key.capacity()
            + self.tools.len() * 96
    }

    fn insert(&mut self, id: String, position: Position) {
        if let Some(previous) = self.positions.get_mut(&id) {
            if previous.tool && (!position.tool || previous.started_seq != position.started_seq) {
                self.tools.remove(&(previous.started_seq, id.clone()));
                self.string_bytes -= id.len();
            }
            if position.tool && (!previous.tool || previous.started_seq != position.started_seq) {
                self.tools.insert((position.started_seq, id.clone()));
                self.string_bytes += id.len();
            }
            *previous = position;
        } else {
            self.string_bytes += id.capacity();
            if position.tool {
                self.string_bytes += id.len();
                self.tools.insert((position.started_seq, id.clone()));
            }
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
                self.insert(
                    event.id,
                    Position {
                        offset,
                        length,
                        revision,
                        started_seq,
                        tool,
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

fn read_position(
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

fn with_index<T>(path: &Utf8Path, operation: impl FnOnce(&ReadIndex) -> Result<T>) -> Result<T> {
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
                string_bytes: 0,
            };
            for (id, locator) in index.item_locators {
                projection.insert(
                    id,
                    Position {
                        offset: locator.offset,
                        length: locator.line_length,
                        revision: locator.revision,
                        started_seq: locator.started_seq,
                        tool: matches!(locator.kind.as_str(), "toolCall" | "toolCallUpdate"),
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
            },
        );
        assert!(index.tools.is_empty());
    }
}
