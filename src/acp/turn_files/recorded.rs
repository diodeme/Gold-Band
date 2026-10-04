//! Comparisons of captured evidence only. Logical paths are labels, never read targets.
use super::*;

#[derive(Debug, Default)]
pub(super) struct RecordedStatsCache {
    entries: HashMap<String, (Option<u64>, Option<u64>)>,
}

/// A maximal run of captured mutations whose endpoints connect exactly.
struct RecordedRun<'a> {
    source: &'a TurnFileMutation,
    before: Option<FileVersionRef>,
    after: Option<FileVersionRef>,
    /// Content produced by fragment replay; written to CAS only when the run is emitted.
    replayed_after: Option<String>,
    /// Whole-file content of `after`, known only when the run starts from a captured creation.
    whole_after: Option<String>,
}

impl TurnFileStore {
    pub(super) fn recorded_changes(
        &self,
        path: &str,
        chain: &[TurnFileMutation],
    ) -> Result<Vec<TurnFileChange>> {
        let mut runs = Vec::<RecordedRun>::new();
        for mutation in chain {
            if let Some(run) = runs.last_mut()
                && mutation.limitation_code.is_none()
                && run.source.limitation_code.is_none()
            {
                // Equality is exact and applies to the entire captured comparison range.
                // A missing endpoint means absence; never confuse it with an empty blob.
                if run.after == mutation.before_version {
                    run.after = mutation.after_version.clone();
                    run.replayed_after = None;
                    run.whole_after = match (&run.whole_after, &mutation.after_version) {
                        (Some(_), Some(after)) => self.read_blob(after).ok(),
                        _ => None,
                    };
                    continue;
                }
                if let Some(replayed) = self.replay_fragment(run.whole_after.as_deref(), mutation) {
                    run.after = Some(text_version_ref(&replayed));
                    run.replayed_after = Some(replayed.clone());
                    run.whole_after = Some(replayed);
                    continue;
                }
            }
            // A creation captures the whole file, so later fragments can be located in it.
            let whole_after = match (&mutation.before_version, &mutation.after_version) {
                (None, Some(after)) if mutation.limitation_code.is_none() => {
                    self.read_blob(after).ok()
                }
                _ => None,
            };
            runs.push(RecordedRun {
                source: mutation,
                before: mutation.before_version.clone(),
                after: mutation.after_version.clone(),
                replayed_after: None,
                whole_after,
            });
        }
        let independent = runs.len() > 1;
        let mut changes = Vec::new();
        for run in runs {
            if let Some(content) = &run.replayed_after {
                self.write_blob(content)?;
            }
            changes.extend(self.recorded_change(
                path,
                run.source,
                run.before,
                run.after,
                independent,
            )?);
        }
        Ok(changes)
    }

    /// Applies one fragment edit to known whole-file content. Absent, empty, missing or
    /// repeated anchors end the run instead of guessing where the fragment belongs.
    fn replay_fragment(&self, whole: Option<&str>, mutation: &TurnFileMutation) -> Option<String> {
        let whole = whole?;
        let old = self.read_blob(mutation.before_version.as_ref()?).ok()?;
        let new = self.read_blob(mutation.after_version.as_ref()?).ok()?;
        if old.is_empty() {
            return None;
        }
        let mut matches = whole.match_indices(old.as_str());
        let (offset, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let replayed = format!("{}{new}{}", &whole[..offset], &whole[offset + old.len()..]);
        (replayed.len() <= self.config.capture_max_file_bytes).then_some(replayed)
    }

    fn recorded_change(
        &self,
        path: &str,
        source: &TurnFileMutation,
        before: Option<FileVersionRef>,
        after: Option<FileVersionRef>,
        independent: bool,
    ) -> Result<Option<TurnFileChange>> {
        let mut limitation = source.limitation_code.clone();
        if limitation.is_none() && before == after {
            return Ok(None);
        }
        let mut stats = (None, None);
        if limitation.is_none() {
            let key = format!(
                "{}\0{}",
                version_hash(before.as_ref()).unwrap_or("absent"),
                version_hash(after.as_ref()).unwrap_or("absent")
            );
            let cached = self.recorded_stats.lock().entries.get(&key).copied();
            if let Some(cached) = cached {
                stats = cached;
            } else {
                let read = || -> Result<_> {
                    let old = before.as_ref().map(|v| self.read_blob(v)).transpose()?;
                    let new = after.as_ref().map(|v| self.read_blob(v)).transpose()?;
                    Ok(line_stats(
                        old.as_deref().unwrap_or_default(),
                        new.as_deref().unwrap_or_default(),
                    ))
                };
                match read() {
                    Ok(value) => {
                        stats = value;
                        let mut cache = self.recorded_stats.lock();
                        if cache.entries.len() >= self.config.capture_max_entries {
                            cache.entries.clear();
                        }
                        if self.config.capture_max_entries > 0 {
                            cache.entries.insert(key, value);
                        }
                    }
                    Err(_) => limitation = Some(BLOB_CORRUPTED.into()),
                }
            }
        }
        let identity = if independent {
            format!(
                "{}\0{}\0{path}\0{}\0{}",
                source.turn_id, source.branch_id, source.tool_call_id, source.content_index
            )
        } else {
            format!("{}\0{}\0{path}", source.turn_id, source.branch_id)
        };
        let change_kind = match (&before, &after) {
            (None, Some(_)) => FileChangeKind::Added,
            (Some(_), None) => FileChangeKind::Deleted,
            _ => FileChangeKind::Modified,
        };
        let valid = limitation.is_none();
        Ok(Some(TurnFileChange {
            id: format!("turn-file-change-{}", stable_id(&identity)),
            change_kind,
            logical_path: path.into(),
            previous_logical_path: None,
            mime_type: None,
            text: true,
            added_lines: stats.0,
            deleted_lines: stats.1,
            before_version: valid.then_some(before).flatten(),
            after_version: valid.then_some(after).flatten(),
            limitation_code: limitation,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation(
        store: &TurnFileStore,
        seq: u64,
        before: Option<&str>,
        after: Option<&str>,
    ) -> TurnFileMutation {
        TurnFileMutation {
            idempotency_key: seq.to_string(),
            turn_id: "turn".into(),
            prompt_event_id: "prompt".into(),
            branch_id: "root".into(),
            tool_call_id: seq.to_string(),
            event_seq: seq,
            content_index: 0,
            logical_path: "unreadable/outside.txt".into(),
            before_version: before.map(|s| store.write_blob(s).unwrap()),
            after_version: after.map(|s| store.write_blob(s).unwrap()),
            captured_at: "now".into(),
            limitation_code: None,
        }
    }

    #[test]
    fn recorded_chains_fold_exactly_and_preserve_independent_edits() {
        let temp = tempfile::tempdir().unwrap();
        let store = TurnFileStore::new(
            Utf8PathBuf::from_path_buf(temp.path().into()).unwrap(),
            Default::default(),
        );
        let mut chain = (0..9)
            .map(|n| {
                mutation(
                    &store,
                    n,
                    Some(&format!("v{n}\n")),
                    Some(&format!("v{}\n", n + 1)),
                )
            })
            .collect::<Vec<_>>();
        let changes = store.recorded_changes("outside.txt", &chain).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            store
                .read_blob(changes[0].before_version.as_ref().unwrap())
                .unwrap(),
            "v0\n"
        );
        assert_eq!(
            store
                .read_blob(changes[0].after_version.as_ref().unwrap())
                .unwrap(),
            "v9\n"
        );
        chain.push(mutation(&store, 10, Some("v9\n"), Some("v0\n")));
        assert!(
            store
                .recorded_changes("outside.txt", &chain)
                .unwrap()
                .is_empty()
        );
        let separate = [
            mutation(&store, 1, Some("A"), Some("B")),
            mutation(&store, 2, Some("X"), Some("Y")),
        ];
        let changes = store.recorded_changes("outside.txt", &separate).unwrap();
        assert_eq!(changes.len(), 2);
        assert_ne!(changes[0].id, changes[1].id);
        assert!(changes.iter().all(|c| c.limitation_code.is_none()));
        assert_eq!(
            store
                .read_blob(changes[1].before_version.as_ref().unwrap())
                .unwrap(),
            "X"
        );
    }

    #[test]
    fn captured_absence_empty_and_invalid_evidence_remain_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let store = TurnFileStore::new(
            Utf8PathBuf::from_path_buf(temp.path().into()).unwrap(),
            Default::default(),
        );
        let created = mutation(&store, 1, None, Some(""));
        assert_eq!(
            store.recorded_changes("empty", &[created.clone()]).unwrap()[0].change_kind,
            FileChangeKind::Added
        );
        let deleted = mutation(&store, 2, Some(""), None);
        assert!(
            store
                .recorded_changes("empty", &[created, deleted])
                .unwrap()
                .is_empty()
        );
        let mut invalid = mutation(&store, 3, None, None);
        invalid.limitation_code = Some(INVALID_TOOL_DIFF.into());
        let changes = store.recorded_changes("invalid", &[invalid]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].added_lines, None);
        assert_eq!(
            changes[0].limitation_code.as_deref(),
            Some(INVALID_TOOL_DIFF)
        );
    }

    #[test]
    fn precomputed_stats_are_bounded_and_corrupt_blobs_fail_locally() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = TurnFileStore::new(
            Utf8PathBuf::from_path_buf(temp.path().into()).unwrap(),
            Default::default(),
        );
        store.config.capture_max_entries = 2;
        for n in 0..10 {
            let m = mutation(&store, n, Some("a"), Some(&n.to_string()));
            store.recorded_changes("label", &[m]).unwrap();
        }
        assert!(store.recorded_stats.lock().entries.len() <= 2);
        let mut broken = mutation(&store, 11, Some("old"), Some("new"));
        broken.after_version.as_mut().unwrap().content_hash = "0".repeat(64);
        let changes = store.recorded_changes("label", &[broken]).unwrap();
        assert_eq!(changes[0].limitation_code.as_deref(), Some(BLOB_CORRUPTED));
    }
}
