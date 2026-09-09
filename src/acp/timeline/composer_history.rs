use super::*;

pub const COMPOSER_HISTORY_PAGE_SIZE: usize = 20;
pub const MAX_COMPOSER_HISTORY_PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCursor {
    pub generation: u64,
    pub message_id: String,
    pub position: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryDirection {
    #[default]
    Older,
    Newer,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub cursor: Option<HistoryCursor>,
    pub head: Option<HistoryCursor>,
    #[serde(default)]
    pub direction: HistoryDirection,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub cursor: HistoryCursor,
    pub text_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<HistorySummary>,
    pub head: Option<HistoryCursor>,
    pub next_cursor: Option<HistoryCursor>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryText {
    pub cursor: HistoryCursor,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("acp.composer-history-stale")]
    Stale,
    #[error("acp.composer-history-not-found")]
    NotFound,
}

fn eligible(locator: &TimelineItemLocator) -> bool {
    locator.gold_band_prompt
        && locator.composer_text_bytes.is_some()
        && !locator.hidden_from_chat
        && locator.branch_id == "root"
        && locator.kind == "userTextDelta"
}

fn cursor(generation: u64, id: &str, locator: &TimelineItemLocator) -> HistoryCursor {
    HistoryCursor {
        generation,
        message_id: id.to_owned(),
        position: locator.started_seq,
    }
}

fn validate_cursor(index: &TimelineMaterializedIndex, value: &HistoryCursor) -> Result<()> {
    if value.generation != index.generation
        || !index
            .item_locators
            .get(&value.message_id)
            .is_some_and(|item| eligible(item) && item.started_seq == value.position)
    {
        return Err(HistoryError::Stale.into());
    }
    Ok(())
}

pub(super) fn original_user_text(event: &AcpUiEvent) -> Option<&str> {
    // Control transitions can accompany a real user message. Provenance and visibility
    // determine eligibility; the presence of transition metadata does not.
    if !event
        .raw
        .as_ref()
        .is_some_and(|raw| raw.get("originalUserText").and_then(Value::as_bool) == Some(true))
    {
        return None;
    }
    event
        .content
        .as_deref()
        .filter(|text| !text.trim().is_empty())
}

/// Runs only during the existing attempt-schema migration, never on a history keypress.
pub(crate) fn migrate_raw_agent_initial_text(attempt_dir: &Utf8Path) -> Result<bool> {
    use crate::dsl::{NodeDsl, PromptEnvelopeMode, WorkflowDsl};
    let node_path = attempt_dir.join("node.json");
    if !node_path.exists() {
        return Ok(false);
    }
    let node: crate::runtime::NodeState = crate::storage::read_json(&node_path)?;
    if node
        .resolved_config
        .get("sessionMode")
        .and_then(Value::as_str)
        != Some("new")
    {
        return Ok(false);
    }
    // Verify the canonical run/round/node/attempt layout before resolving its immutable snapshot.
    let Some(run_dir) = attempt_dir.ancestors().nth(5) else {
        return Ok(false);
    };
    if run_dir.file_name() != Some(node.run_id.as_str())
        || run_dir
            .join("rounds")
            .join(&node.round_id)
            .join("nodes")
            .join(&node.node_id)
            .join(&node.attempt_id)
            != attempt_dir
    {
        return Ok(false);
    }
    let snapshot_path = run_dir.join("workflow.snapshot.json");
    if !snapshot_path.exists() {
        return Ok(false);
    }
    let workflow: WorkflowDsl = crate::storage::read_json(&snapshot_path)?;
    if !workflow.nodes.iter().any(|candidate| {
        matches!(candidate, NodeDsl::Worker(worker)
        if worker.id == node.node_id && worker.prompt_envelope == PromptEnvelopeMode::RawAgent)
    }) {
        return Ok(false);
    }
    let path = attempt_dir.join("acp.timeline.jsonl");
    if !path.exists() {
        return Ok(false);
    }
    with_jsonl_file_lock(&path, || {
        let (mut index, _) = load_or_rebuild_index_unlocked(
            &path,
            &timeline_index_path(&path),
            TimelineCheckpointPolicy::default(),
        )?;
        let first = index
            .item_locators
            .iter()
            .filter(|(_, item)| item.gold_band_prompt && item.branch_id == "root")
            .min_by_key(|(id, item)| (item.started_seq, id.as_str()));
        let Some((_, locator)) = first else {
            return Ok(false);
        };
        if locator.hidden_from_chat || locator.kind != "userTextDelta" {
            return Ok(false);
        }
        let mut event = read_event_at_locator(&path, locator)?;
        let Some(raw) = event.raw.as_mut() else {
            return Ok(false);
        };
        if raw.get("originalUserText").and_then(Value::as_bool) != Some(false)
            || raw.get("turnControlMode").and_then(Value::as_str) != Some("non-runtime-controlled")
            || raw.get("runtimeControl").is_some()
        {
            return Ok(false);
        }
        raw["originalUserText"] = Value::Bool(true);
        let revision = index.covered_revision.saturating_add(1);
        append_indexed_patch_unlocked(&path, &mut index, revision, event)?;
        Ok(true)
    })
}

pub fn read_page(path: &Utf8Path, query: HistoryQuery) -> Result<HistoryPage> {
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(
            path,
            &timeline_index_path(path),
            TimelineCheckpointPolicy::default(),
        )?;
        for value in query.cursor.iter().chain(query.head.iter()) {
            validate_cursor(&index, value)?;
        }
        let mut candidates = index
            .item_locators
            .iter()
            .filter(|(_, item)| eligible(item))
            .collect::<Vec<_>>();
        candidates.sort_by(|(a_id, a), (b_id, b)| {
            (b.started_seq, b_id.as_str()).cmp(&(a.started_seq, a_id.as_str()))
        });
        let head = query.head.clone().or_else(|| {
            candidates
                .first()
                .map(|(id, item)| cursor(index.generation, id, item))
        });
        candidates.retain(|(id, item)| {
            head.as_ref().is_none_or(|head| {
                (item.started_seq, id.as_str()) <= (head.position, head.message_id.as_str())
            })
        });
        let mut seen = HashSet::new();
        candidates
            .retain(|(id, item)| seen.insert(item.prompt_id.as_deref().unwrap_or(id.as_str())));
        candidates.retain(|(id, item)| {
            let position = (item.started_seq, id.as_str());
            query
                .cursor
                .as_ref()
                .is_none_or(|bound| match query.direction {
                    HistoryDirection::Older => {
                        position < (bound.position, bound.message_id.as_str())
                    }
                    HistoryDirection::Newer => {
                        position > (bound.position, bound.message_id.as_str())
                    }
                })
        });
        if matches!(query.direction, HistoryDirection::Newer) {
            candidates.reverse();
        }
        let limit = query
            .limit
            .unwrap_or(COMPOSER_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_COMPOSER_HISTORY_PAGE_SIZE);
        let more = candidates.len() > limit;
        candidates.truncate(limit);
        let next_cursor = more
            .then(|| {
                candidates
                    .last()
                    .map(|(id, item)| cursor(index.generation, id, item))
            })
            .flatten();
        let mut items = Vec::with_capacity(candidates.len());
        for (id, locator) in candidates {
            items.push(HistorySummary {
                cursor: cursor(index.generation, id, locator),
                text_bytes: locator.composer_text_bytes.unwrap_or_default(),
            });
        }
        Ok(HistoryPage {
            items,
            head,
            next_cursor,
        })
    })
}

pub fn read_text(path: &Utf8Path, value: HistoryCursor) -> Result<HistoryText> {
    with_jsonl_file_lock(path, || {
        let (index, _) = load_or_rebuild_index_unlocked(
            path,
            &timeline_index_path(path),
            TimelineCheckpointPolicy::default(),
        )?;
        validate_cursor(&index, &value)?;
        let locator = index
            .item_locators
            .get(&value.message_id)
            .ok_or(HistoryError::NotFound)?;
        let event = read_event_at_locator(path, locator)?;
        let text = original_user_text(&event)
            .ok_or(HistoryError::NotFound)?
            .to_owned();
        Ok(HistoryText {
            cursor: value,
            text,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn user_prompt_event(
        seq: u64,
        session: String,
        text: String,
        prompt: Option<String>,
        hidden: bool,
        attachments: Vec<crate::acp::events::AttachmentMeta>,
    ) -> AcpUiEvent {
        let mut event =
            crate::acp::events::user_prompt_event(seq, session, text, prompt, hidden, attachments);
        event.raw.as_mut().unwrap()["originalUserText"] = Value::Bool(true);
        event
    }
    use tempfile::tempdir;

    #[test]
    fn composer_history_pages_original_user_text_and_rejects_stale_cursors() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        for seq in 1..=6 {
            let mut event = user_prompt_event(
                seq,
                "session".into(),
                format!("hello{seq}"),
                Some(format!("prompt-{seq}")),
                seq == 4,
                vec![],
            );
            if seq == 5 {
                event.content = None;
            }
            if seq == 6 {
                event.raw.as_mut().unwrap()["source"] = Value::String("providerHistory".into());
            }
            store.upsert(seq, &event).unwrap();
        }
        let first = read_page(
            &path,
            HistoryQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(
            read_text(&path, first.items[0].cursor.clone())
                .unwrap()
                .text,
            "hello3"
        );
        let older = read_page(
            &path,
            HistoryQuery {
                cursor: first.next_cursor,
                head: first.head.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(older.items.len(), 2);
        assert_eq!(
            read_text(&path, older.items[1].cursor.clone())
                .unwrap()
                .text,
            "hello1"
        );
        let newer = read_page(
            &path,
            HistoryQuery {
                cursor: Some(older.items[1].cursor.clone()),
                head: first.head,
                direction: HistoryDirection::Newer,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            read_text(&path, newer.items[0].cursor.clone())
                .unwrap()
                .text,
            "hello2"
        );
        let mut stale = older.items[0].cursor.clone();
        stale.generation += 1;
        assert!(
            read_text(&path, stale)
                .unwrap_err()
                .downcast_ref::<HistoryError>()
                .is_some()
        );
    }

    #[test]
    fn composer_history_deduplicates_identity_not_text_and_freezes_head() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        for (seq, prompt_id) in [(1, "a"), (2, "b"), (3, "b")] {
            store
                .upsert(
                    seq,
                    &user_prompt_event(
                        seq,
                        "s".into(),
                        "same".into(),
                        Some(prompt_id.into()),
                        false,
                        vec![],
                    ),
                )
                .unwrap();
        }
        let page = read_page(&path, HistoryQuery::default()).unwrap();
        assert_eq!(page.items.len(), 2);
        store
            .upsert(
                4,
                &user_prompt_event(4, "s".into(), "new".into(), Some("c".into()), false, vec![]),
            )
            .unwrap();
        store
            .upsert(
                5,
                &user_prompt_event(
                    5,
                    "s".into(),
                    "same".into(),
                    Some("b".into()),
                    false,
                    vec![],
                ),
            )
            .unwrap();
        assert_eq!(
            read_page(
                &path,
                HistoryQuery {
                    head: page.head,
                    ..Default::default()
                }
            )
            .unwrap()
            .items
            .len(),
            2
        );
    }

    #[test]
    fn composer_history_excludes_runtime_legacy_and_branch_records_and_preserves_long_text() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        let long_text = "original\n".repeat(20_000);
        for seq in 1..=5 {
            let mut event = user_prompt_event(
                seq,
                "s".into(),
                long_text.clone(),
                Some(format!("p{seq}")),
                false,
                vec![],
            );
            let raw = event.raw.as_mut().unwrap();
            match seq {
                2 => {
                    raw["originalUserText"] = Value::Bool(false);
                    raw["runtimeControl"] = serde_json::json!({"transitionId": "control"});
                }
                3 => {
                    raw.as_object_mut().unwrap().remove("originalUserText");
                }
                4 => {
                    raw["_meta"] =
                        serde_json::json!({"goldBandConversation": {"branchId": "child"}});
                }
                5 => {
                    event.content = Some(" \n ".into());
                }
                _ => {}
            }
            store.upsert(seq, &event).unwrap();
        }
        let page = read_page(&path, HistoryQuery::default()).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            read_text(&path, page.items[0].cursor.clone()).unwrap().text,
            long_text
        );
        drop(store);
        assert_eq!(
            read_page(&path, HistoryQuery::default())
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn composer_history_keeps_manual_follow_up_across_reopen_and_old_index() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut store =
            TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
        let mut event = user_prompt_event(
            1,
            "s".into(),
            "follow up".into(),
            Some("p1".into()),
            false,
            vec![],
        );
        event.raw.as_mut().unwrap()["runtimeControl"] = serde_json::json!({
            "currentMode": "non-runtime-controlled", "transitionCause": "manual-follow-up", "transitionId": "t1"
        });
        store.upsert(1, &event).unwrap();
        drop(store);
        let page = read_page(&path, HistoryQuery::default()).unwrap();
        assert_eq!(
            page.items.len(),
            1,
            "manual follow-up is user input, not a control prompt"
        );
        assert_eq!(
            read_text(&path, page.items[0].cursor.clone()).unwrap().text,
            "follow up"
        );
        let index_path = timeline_index_path(&path);
        let mut old: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
        old["formatVersion"] = serde_json::json!(11);
        for locator in old["itemLocators"].as_object_mut().unwrap().values_mut() {
            locator.as_object_mut().unwrap().remove("composerTextBytes");
        }
        std::fs::write(&index_path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(
            read_page(&path, HistoryQuery::default())
                .unwrap()
                .items
                .len(),
            1,
            "old derived eligibility must be rebuilt"
        );
    }

    #[test]
    fn composer_history_migrates_verified_raw_agent_first_input_once() {
        for (envelope, mode, marker, hidden, expected) in [
            ("raw-agent", "new", Some(false), false, true),
            ("runtime-managed", "new", Some(false), false, false),
            ("raw-agent", "continue", Some(false), false, false),
            ("raw-agent", "new", None, false, false),
            ("raw-agent", "new", Some(false), true, false),
        ] {
            let dir = tempdir().unwrap();
            let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
            let attempt = root.join("runs/run-001/rounds/round-001/nodes/worker/attempt-001");
            std::fs::create_dir_all(&attempt).unwrap();
            let node_path = attempt.join("node.json");
            crate::storage::write_json(&node_path, &serde_json::json!({
            "version": crate::domain::VERSION, "acp_storage_schema_version": 2,
            "node_id": "worker", "node_type": "worker", "run_id": "run-001", "round_id": "round-001",
            "attempt_id": "attempt-001", "status": "completed", "outcome": "success",
            "started_at": "1Z", "finished_at": "2Z", "manual_check_pending": false,
            "runtime_execution_id": "e1", "resolved_config": {"sessionMode": mode}
        })).unwrap();
            crate::storage::write_json(
                &root.join("runs/run-001/workflow.snapshot.json"),
                &serde_json::json!({
                    "version": crate::domain::VERSION, "id": "w", "entry": "worker", "edges": [],
                    "nodes": [{"id": "worker", "type": "worker", "prompt_envelope": envelope}]
                }),
            )
            .unwrap();
            let path = attempt.join("acp.timeline.jsonl");
            let mut store =
                TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
            let mut event = user_prompt_event(
                1,
                "s".into(),
                "first input\n".into(),
                Some("p1".into()),
                hidden,
                vec![],
            );
            if let Some(marker) = marker {
                event.raw.as_mut().unwrap()["originalUserText"] = Value::Bool(marker);
            } else {
                event
                    .raw
                    .as_mut()
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove("originalUserText");
            }
            event.raw.as_mut().unwrap()["turnControlMode"] =
                Value::String("non-runtime-controlled".into());
            store.upsert(1, &event).unwrap();
            drop(store);
            assert_eq!(
                crate::acp::branches::prepare_agent_timeline_storage(&attempt).unwrap(),
                expected
            );
            let page = read_page(&path, HistoryQuery::default()).unwrap();
            assert_eq!(page.items.len(), usize::from(expected));
            if expected {
                assert_eq!(
                    read_text(&path, page.items[0].cursor.clone()).unwrap().text,
                    "first input\n"
                );
            }
            assert!(!crate::acp::branches::prepare_agent_timeline_storage(&attempt).unwrap());
        }
    }

    #[test]
    fn composer_history_ten_thousand_messages_return_bounded_pages() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
        let mut writer = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        for revision in 1..=10_000 {
            let item = user_prompt_event(
                revision,
                "s".into(),
                format!("hello{revision}"),
                Some(format!("p{revision}")),
                false,
                vec![],
            );
            serde_json::to_writer(
                &mut writer,
                &AcpTimelinePatch {
                    patch_type: "timelinePatch".into(),
                    item_id: item.id.clone(),
                    revision,
                    op: "upsert".into(),
                    item,
                },
            )
            .unwrap();
            writer.write_all(b"\n").unwrap();
        }
        writer.flush().unwrap();
        read_page(&path, HistoryQuery::default()).unwrap();
        let started = std::time::Instant::now();
        let page = read_page(
            &path,
            HistoryQuery {
                limit: Some(usize::MAX),
                ..Default::default()
            },
        )
        .unwrap();
        let page_elapsed = started.elapsed();
        assert_eq!(page.items.len(), MAX_COMPOSER_HISTORY_PAGE_SIZE);
        let started = std::time::Instant::now();
        assert_eq!(
            read_text(&path, page.items[0].cursor.clone()).unwrap().text,
            "hello10000"
        );
        eprintln!(
            "composer history 10000 messages: page={page_elapsed:?}, text={:?}",
            started.elapsed()
        );
        assert!(page.next_cursor.is_some());
        assert!(serde_json::to_vec(&page).unwrap().len() < 16 * 1024);
    }
}
