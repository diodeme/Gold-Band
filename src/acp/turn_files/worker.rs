//! One bounded worker per active prompt. The ingress lock only exchanges small
//! batches; filesystem I/O, hashing and diffing always run outside that lock.
use super::*;
use crate::acp::{branches::ROOT_BRANCH_ID, events::current_timestamp};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

#[derive(Default)]
struct PendingTool {
    branch: String,
    tool: String,
    seq: u64,
    timestamp: String,
    diffs: Vec<CapturedToolDiff>,
    outcome: Option<TurnFileToolTerminalOutcome>,
}

impl PendingTool {
    fn bytes(&self) -> usize {
        self.diffs
            .iter()
            .map(|d| {
                d.old_text.as_ref().map_or(0, String::len)
                    + d.new_text.as_ref().map_or(0, String::len)
            })
            .sum()
    }
}

#[derive(Default)]
struct Mailbox {
    pending: HashMap<(String, String), PendingTool>,
    editing_tools: HashSet<(String, String)>,
    bytes: usize,
    limited: bool,
    finish: bool,
    abort: bool,
}

pub(crate) struct TurnFileWorker {
    mailbox: Arc<parking_lot::Mutex<Mailbox>>,
    wake: Option<mpsc::SyncSender<()>>,
    thread: Option<JoinHandle<Result<Vec<TurnFileChangeSet>>>>,
    config: TurnFileCaptureConfig,
}

impl TurnFileWorker {
    pub(crate) fn start(
        store: TurnFileStore,
        workspace: Utf8PathBuf,
        turn: String,
        prompt: String,
        started_at: String,
    ) -> Result<Self> {
        let config = store.config;
        let mailbox = Arc::new(parking_lot::Mutex::new(Mailbox::default()));
        let (wake, receive) = mpsc::sync_channel(1);
        let shared = mailbox.clone();
        let thread = std::thread::Builder::new()
            .name("turn-file-diff".into())
            .spawn(move || {
                run_worker(store, workspace, turn, prompt, started_at, shared, receive)
            })?;
        Ok(Self {
            mailbox,
            wake: Some(wake),
            thread: Some(thread),
            config,
        })
    }

    pub(crate) fn submit(
        &self,
        branch: String,
        tool: String,
        seq: u64,
        timestamp: String,
        raw: &Value,
        outcome: Option<TurnFileToolTerminalOutcome>,
    ) {
        let diffs = extract_standard_tool_diffs(raw);
        if diffs.is_empty() && outcome.is_none() {
            return;
        }
        let key = (branch.clone(), tool.clone());
        let mut mailbox = self.mailbox.lock();
        if mailbox.abort || mailbox.finish {
            return;
        }
        if diffs.is_empty() && !mailbox.editing_tools.contains(&key) {
            return;
        }
        if !diffs.is_empty() && !mailbox.editing_tools.contains(&key) {
            if mailbox.editing_tools.len() >= self.config.capture_max_entries {
                mailbox.limited = true;
                drop(mailbox);
                if let Some(wake) = &self.wake {
                    let _ = wake.try_send(());
                }
                return;
            }
            mailbox.editing_tools.insert(key.clone());
        }
        let previous = mailbox.pending.remove(&key);
        mailbox.bytes = mailbox
            .bytes
            .saturating_sub(previous.as_ref().map_or(0, PendingTool::bytes));
        let mut next = PendingTool {
            branch,
            tool,
            seq,
            timestamp,
            diffs,
            outcome,
        };
        if let Some(previous) = previous {
            next.outcome = next.outcome.or(previous.outcome);
            if next.diffs.is_empty() {
                next.diffs = previous.diffs;
                next.seq = previous.seq;
                next.timestamp = previous.timestamp;
            }
        }
        let bytes = next.bytes();
        if mailbox.pending.len() >= self.config.capture_max_entries
            || mailbox.bytes + bytes > self.config.capture_max_total_bytes
        {
            // Fail closed rather than dropping evidence and publishing a false net diff.
            mailbox.limited = true;
        } else {
            mailbox.bytes += bytes;
            mailbox.pending.insert(key, next);
        }
        drop(mailbox);
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
    }

    pub(crate) fn finish(mut self) -> Result<Vec<TurnFileChangeSet>> {
        self.mailbox.lock().finish = true;
        if let Some(wake) = self.wake.take() {
            let _ = wake.try_send(());
        }
        self.thread
            .take()
            .expect("worker exists")
            .join()
            .map_err(|_| anyhow!("turn-files.worker-failed"))?
    }
}

impl Drop for TurnFileWorker {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.mailbox.lock().abort = true;
            self.wake.take();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn run_worker(
    store: TurnFileStore,
    workspace: Utf8PathBuf,
    turn: String,
    prompt: String,
    started_at: String,
    mailbox: Arc<parking_lot::Mutex<Mailbox>>,
    receive: mpsc::Receiver<()>,
) -> Result<Vec<TurnFileChangeSet>> {
    let mut mutations = HashMap::<(String, String, String, usize), TurnFileMutation>::new();
    let mut outcomes = HashMap::<(String, String), TurnFileToolTerminalOutcome>::new();
    let mut captured_bytes = 0usize;
    let mut captured_entries = 0usize;
    let mut limited = false;
    // Attachment discovery is also off the ACP event thread. The initial baseline
    // is captured before prompt dispatch by the existing lifecycle owner.
    loop {
        let _ = receive.recv();
        let (pending, finish, abort) = {
            let mut state = mailbox.lock();
            limited |= state.limited;
            state.bytes = 0;
            (
                std::mem::take(&mut state.pending),
                state.finish,
                state.abort,
            )
        };
        if abort {
            return Ok(Vec::new());
        }
        let mut pending = pending.into_values().collect::<Vec<_>>();
        pending.sort_by_key(|event| event.seq);
        let mut dirty = HashSet::<(String, String)>::new();
        for event in pending {
            let tool_key = (event.branch.clone(), event.tool.clone());
            if let Some(outcome) = event.outcome
                && (!event.diffs.is_empty()
                    || mutations
                        .values()
                        .any(|m| m.branch_id == event.branch && m.tool_call_id == event.tool))
            {
                if outcomes.len() < store.config.capture_max_entries
                    || outcomes.contains_key(&tool_key)
                {
                    outcomes.insert(tool_key.clone(), outcome);
                } else {
                    limited = true;
                }
                for entry in mutations
                    .values()
                    .filter(|m| m.branch_id == event.branch && m.tool_call_id == event.tool)
                {
                    dirty.insert((entry.branch_id.clone(), entry.logical_path.clone()));
                }
            }
            if event.diffs.is_empty() {
                continue;
            }
            // A tool update revises that operation; it is not another filesystem edit.
            mutations.retain(|_, m| {
                let keep = m.branch_id != event.branch || m.tool_call_id != event.tool;
                if !keep {
                    dirty.insert((m.branch_id.clone(), m.logical_path.clone()));
                }
                keep
            });
            for diff in event.diffs {
                if captured_entries >= store.config.capture_max_entries {
                    limited = true;
                    break;
                }
                captured_entries += 1;
                let path = match recorded_logical_path(&workspace, &diff.path) {
                    Ok(path) => path,
                    Err(_) => {
                        limited = true;
                        continue;
                    }
                };
                let bytes = diff.old_text.as_ref().map_or(0, String::len)
                    + diff.new_text.as_ref().map_or(0, String::len);
                let exceeded = bytes > store.config.capture_max_file_bytes
                    || captured_bytes.saturating_add(bytes) > store.config.capture_max_total_bytes;
                let (before, after) = if exceeded {
                    (None, None)
                } else {
                    captured_bytes += bytes;
                    (
                        diff.old_text
                            .as_deref()
                            .map(|s| store.write_blob(s))
                            .transpose()?,
                        diff.new_text
                            .as_deref()
                            .map(|s| store.write_blob(s))
                            .transpose()?,
                    )
                };
                let mutation = TurnFileMutation {
                    idempotency_key: mutation_key(
                        &turn,
                        &event.tool,
                        event.seq,
                        diff.content_index,
                        before.as_ref(),
                        after.as_ref(),
                    ),
                    turn_id: turn.clone(),
                    prompt_event_id: prompt.clone(),
                    branch_id: event.branch.clone(),
                    tool_call_id: event.tool.clone(),
                    event_seq: event.seq,
                    content_index: diff.content_index,
                    logical_path: path.clone(),
                    before_version: before,
                    after_version: after,
                    captured_at: event.timestamp.clone(),
                    limitation_code: exceeded
                        .then(|| CAPTURE_LIMIT_EXCEEDED.into())
                        .or(diff.limitation_code),
                };
                append_jsonl_durable(&store.mutation_journal_path(), &mutation)?;
                dirty.insert((event.branch.clone(), path.clone()));
                mutations.insert(
                    (
                        event.branch.clone(),
                        event.tool.clone(),
                        path,
                        diff.content_index,
                    ),
                    mutation,
                );
            }
        }
        for (branch, path) in dirty {
            if mailbox.lock().abort {
                return Ok(Vec::new());
            }
            let mut chain = mutations
                .values()
                .filter(|m| {
                    m.branch_id == branch
                        && m.logical_path == path
                        && outcomes
                            .get(&(branch.clone(), m.tool_call_id.clone()))
                            .is_some_and(|o| o.committed())
                })
                .cloned()
                .collect::<Vec<_>>();
            chain.sort_by_key(|m| (m.event_seq, m.content_index));
            if !chain.is_empty() && !limited {
                let _ = store.recorded_changes(&path, &chain);
            }
        }
        if finish {
            break;
        }
    }
    let finished_at = current_timestamp();
    let attachments = store.collect_turn_attachment_delta(&turn)?;
    let mut branches = mutations
        .values()
        .map(|m| m.branch_id.clone())
        .collect::<HashSet<_>>();
    branches.insert(ROOT_BRANCH_ID.into());
    let mut branches = branches.into_iter().collect::<Vec<_>>();
    branches.sort();
    let mut sets = Vec::new();
    for branch in branches {
        let confirmed = mutations
            .values()
            .filter(|m| {
                m.branch_id == branch
                    && outcomes
                        .get(&(branch.clone(), m.tool_call_id.clone()))
                        .is_some_and(|o| o.committed())
            })
            .cloned()
            .collect();
        let set = if limited {
            None
        } else {
            store.finalize_mutations(
                &turn,
                &prompt,
                &branch,
                &started_at,
                &finished_at,
                confirmed,
                Some(&workspace),
                &attachments,
                branch == ROOT_BRANCH_ID,
            )?
        };
        if let Some(set) = set {
            sets.push(set);
        } else if limited && branch == ROOT_BRANCH_ID {
            let set = TurnFileChangeSet {
                schema_version: TURN_FILE_CHANGE_SET_SCHEMA_VERSION,
                id: change_set_id(&turn, &branch),
                turn_id: turn.clone(),
                prompt_event_id: prompt.clone(),
                branch_id: branch,
                status: TurnFileChangeSetStatus::Partial,
                started_at: started_at.clone(),
                finished_at: Some(finished_at.clone()),
                summary: TurnFileChangeSummary::default(),
                changes: Vec::new(),
                attachments: attachments.attachments.clone(),
                limitation_codes: vec![CAPTURE_LIMIT_EXCEEDED.into()],
            };
            write_json(&store.change_set_path(&set.id), &set)?;
            sets.push(set);
        }
    }
    Ok(sets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_write_survives_unreported_disk_changes_and_outside_paths() {
        for outside in [false, true] {
            let (_temp, store, root) = fixture();
            let outside_dir = tempfile::tempdir().unwrap();
            let path = if outside {
                Utf8PathBuf::from_path_buf(outside_dir.path().join("created.ts")).unwrap()
            } else {
                root.join("created.ts")
            };
            std::fs::write(&path, "changed by Python; not ACP evidence").unwrap();
            let worker = TurnFileWorker::start(
                store.clone(),
                root,
                "turn".into(),
                "prompt".into(),
                "start".into(),
            )
            .unwrap();
            worker.submit("root".into(), "write".into(), 1, "now".into(),
                &serde_json::json!({"content":[{"type":"diff", "path":path, "oldText":null,"newText":"recorded creation\n"}]}),
                Some(TurnFileToolTerminalOutcome::Succeeded));
            let sets = worker.finish().unwrap();
            assert_eq!(sets.len(), 1);
            assert_eq!(sets[0].status, TurnFileChangeSetStatus::Finalized);
            let change = &sets[0].changes[0];
            assert_eq!(change.change_kind, FileChangeKind::Added);
            let comparison = store.comparison(&sets[0].id, &change.id).unwrap();
            assert_eq!(comparison.after.unwrap().content, "recorded creation\n");
            assert!(comparison.before.is_none());
        }
    }

    fn fixture() -> (tempfile::TempDir, TurnFileStore, Utf8PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_owned()).unwrap();
        let store = TurnFileStore::new(root.join("attempt"), TurnFileCaptureConfig::default());
        store.capture_attachment_baseline("turn").unwrap();
        (temp, store, root)
    }

    fn diff(old: &str, new: &str) -> Value {
        serde_json::json!({ "content": [{ "type": "diff", "path": "a.txt", "oldText": old, "newText": new }] })
    }

    #[test]
    fn deletion_metadata_worker_preserves_presence_and_rejects_conflicts() {
        for (old, new, kind, witness, failed, expected) in [
            (
                Some("old\n"),
                Some(""),
                "delete",
                None,
                false,
                Some(FileChangeKind::Deleted),
            ),
            (
                Some(""),
                Some(""),
                "delete",
                None,
                false,
                Some(FileChangeKind::Deleted),
            ),
            (
                Some("old\n"),
                None,
                "delete",
                None,
                false,
                Some(FileChangeKind::Deleted),
            ),
            (
                Some("old\n"),
                Some(""),
                "update",
                Some(""),
                false,
                Some(FileChangeKind::Modified),
            ),
            (
                None,
                Some(""),
                "add",
                Some(""),
                false,
                Some(FileChangeKind::Added),
            ),
            (Some("old\n"), Some(""), "delete", Some("old\n"), true, None),
            (
                Some("old\n"),
                Some("unexpected"),
                "delete",
                Some("unexpected"),
                false,
                None,
            ),
            (None, Some(""), "delete", None, false, None),
        ] {
            let (_temp, store, root) = fixture();
            if let Some(text) = witness {
                std::fs::write(root.join("a.txt"), text).unwrap();
            }
            let worker = TurnFileWorker::start(
                store.clone(),
                root,
                "turn".into(),
                "prompt".into(),
                "start".into(),
            )
            .unwrap();
            worker.submit("root".into(), "edit".into(), 1, "now".into(),
                &serde_json::json!({"content":[{"type":"diff", "path":"a.txt", "oldText":old, "newText":new, "_meta":{"kind":kind}}]}),
                Some(if failed { TurnFileToolTerminalOutcome::Failed } else { TurnFileToolTerminalOutcome::Succeeded }));
            let sets = worker.finish().unwrap();
            if failed {
                assert!(sets.is_empty());
                continue;
            }
            assert_eq!(sets.len(), 1);
            let set = &sets[0];
            let change = &set.changes[0];
            let comparison = store.comparison(&set.id, &change.id).unwrap();
            if let Some(expected) = expected {
                assert_eq!(set.status, TurnFileChangeSetStatus::Finalized);
                assert_eq!(change.change_kind, expected);
                assert_eq!(comparison.before.as_ref().map(|s| s.content.as_str()), old);
                assert_eq!(
                    comparison.after.as_ref().map(|s| s.content.as_str()),
                    witness
                );
                assert_eq!(
                    change.deleted_lines,
                    Some(if old == Some("old\n") { 1 } else { 0 })
                );
                assert_eq!(change.added_lines, Some(0));
            } else {
                assert_eq!(set.status, TurnFileChangeSetStatus::Partial);
                assert_eq!(change.limitation_code.as_deref(), Some(INVALID_TOOL_DIFF));
                assert!(comparison.before.is_none() && comparison.after.is_none());
                assert_eq!(change.added_lines, None);
                assert_eq!(change.deleted_lines, None);
            }
        }
    }

    #[test]
    fn worker_create_then_explicit_delete_has_no_net_change() {
        let (_temp, store, root) = fixture();
        let worker =
            TurnFileWorker::start(store, root, "turn".into(), "prompt".into(), "start".into())
                .unwrap();
        for (seq, old, new, kind) in [
            (1, None, "first\n", "add"),
            (2, Some("first\n"), "last\n", "update"),
            (3, Some("last\n"), "", "delete"),
        ] {
            worker.submit("root".into(), format!("edit-{seq}"), seq, "now".into(),
                &serde_json::json!({"content":[{"type":"diff", "path":"a.txt", "oldText":old, "newText":new, "_meta":{"kind":kind}}]}),
                Some(TurnFileToolTerminalOutcome::Succeeded));
        }
        assert!(worker.finish().unwrap().is_empty());
    }

    #[test]
    fn ingress_coalesces_revisions_and_has_a_byte_and_entry_bound() {
        let config = TurnFileCaptureConfig {
            capture_max_entries: 2,
            capture_max_total_bytes: 12,
            ..Default::default()
        };
        let worker = TurnFileWorker {
            mailbox: Default::default(),
            wake: None,
            thread: None,
            config,
        };
        for seq in 1..=9 {
            worker.submit(
                "root".into(),
                "edit".into(),
                seq,
                "now".into(),
                &diff("old", "new"),
                None,
            );
        }
        {
            let state = worker.mailbox.lock();
            assert_eq!(state.pending.len(), 1);
            assert_eq!(state.pending.values().next().unwrap().seq, 9);
            assert_eq!(state.bytes, 6);
        }
        worker.submit(
            "root".into(),
            "second".into(),
            10,
            "now".into(),
            &diff("old", "new"),
            None,
        );
        worker.submit(
            "root".into(),
            "third".into(),
            11,
            "now".into(),
            &diff("old", "new"),
            None,
        );
        let state = worker.mailbox.lock();
        assert_eq!(state.pending.len(), 2);
        assert_eq!(state.bytes, 12);
        assert!(state.limited);
    }

    #[test]
    fn unrelated_tool_outcomes_do_not_consume_the_edit_queue_budget() {
        let worker = TurnFileWorker {
            mailbox: Default::default(),
            wake: None,
            thread: None,
            config: TurnFileCaptureConfig {
                capture_max_entries: 1,
                ..Default::default()
            },
        };
        for seq in 0..1000 {
            worker.submit(
                "root".into(),
                format!("read-{seq}"),
                seq,
                "now".into(),
                &serde_json::json!({}),
                Some(TurnFileToolTerminalOutcome::Succeeded),
            );
        }
        let state = worker.mailbox.lock();
        assert!(state.pending.is_empty());
        assert!(!state.limited);
    }

    #[test]
    fn final_watermark_includes_latest_tool_revision_and_excludes_failed_tools() {
        let (_temp, store, root) = fixture();
        std::fs::write(root.join("a.txt"), "header\nnew\nfooter").unwrap();
        let worker = TurnFileWorker::start(
            store.clone(),
            root,
            "turn".into(),
            "prompt".into(),
            "start".into(),
        )
        .unwrap();
        worker.submit(
            "root".into(),
            "edit".into(),
            1,
            "now".into(),
            &diff("old", "new"),
            None,
        );
        worker.submit(
            "root".into(),
            "edit".into(),
            2,
            "now".into(),
            &diff("header\nold", "header\nnew"),
            None,
        );
        worker.submit(
            "root".into(),
            "edit".into(),
            3,
            "now".into(),
            &serde_json::json!({}),
            Some(TurnFileToolTerminalOutcome::Succeeded),
        );
        worker.submit(
            "root".into(),
            "failed".into(),
            4,
            "now".into(),
            &diff("missing", "new"),
            Some(TurnFileToolTerminalOutcome::Failed),
        );
        let sets = worker.finish().unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].summary.file_count, 1);
        let comparison = store
            .comparison(&sets[0].id, &sets[0].changes[0].id)
            .unwrap();
        assert_eq!(comparison.before.unwrap().content, "header\nold");
        assert_eq!(comparison.after.unwrap().content, "header\nnew");
        assert!(sets[0].limitation_codes.is_empty());
    }

    #[test]
    fn ingress_does_not_wait_for_background_diff_and_drop_closes_worker() {
        let (_temp, store, root) = fixture();
        std::fs::write(root.join("a.txt"), "new").unwrap();
        let worker = TurnFileWorker::start(
            store.clone(),
            root,
            "turn".into(),
            "prompt".into(),
            "start".into(),
        )
        .unwrap();
        // Holding the computation cache makes a worker computation wait, but
        // cannot make event submission wait. No timing/sleep race is needed.
        let guard = store.recorded_stats.lock();
        worker.submit(
            "root".into(),
            "edit".into(),
            1,
            "now".into(),
            &diff("old", "new"),
            Some(TurnFileToolTerminalOutcome::Succeeded),
        );
        worker.submit(
            "root".into(),
            "edit".into(),
            2,
            "now".into(),
            &diff("old", "new"),
            Some(TurnFileToolTerminalOutcome::Succeeded),
        );
        drop(guard);
        drop(worker);
        assert!(
            !store
                .change_set_path(&change_set_id("turn", "root"))
                .exists()
        );
    }

    #[test]
    fn unknown_or_overflowed_evidence_cannot_publish_a_successful_zero_diff() {
        let (_temp, mut store, root) = fixture();
        store.config.capture_max_total_bytes = 1;
        std::fs::write(root.join("a.txt"), "new").unwrap();
        let worker =
            TurnFileWorker::start(store, root, "turn".into(), "prompt".into(), "start".into())
                .unwrap();
        worker.submit(
            "root".into(),
            "edit".into(),
            1,
            "now".into(),
            &diff("old", "new"),
            Some(TurnFileToolTerminalOutcome::Succeeded),
        );
        let sets = worker.finish().unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].status, TurnFileChangeSetStatus::Partial);
        assert_eq!(sets[0].limitation_codes, vec![CAPTURE_LIMIT_EXCEEDED]);
    }
}
