use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};

use anyhow::{Context, Result, anyhow};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use similar::{ChangeTag, TextDiff};

use crate::storage::{
    append_jsonl_durable, atomic_write_file, ensure_parent_dir, read_json, write_json,
};

pub const CHANGE_SET_NOT_FOUND: &str = "turn-files.change-set-not-found";
pub const VERSION_NOT_FOUND: &str = "turn-files.version-not-found";
pub const BLOB_CORRUPTED: &str = "turn-files.blob-corrupted";
pub const INVALID_TOOL_DIFF: &str = "turn-files.invalid-tool-diff";
pub const NON_LINEAR_MUTATION: &str = "turn-files.non-linear-mutation";
pub const CAPTURE_LIMIT_EXCEEDED: &str = "turn-files.capture-limit-exceeded";
pub const TURN_FILE_CHANGE_SET_SCHEMA_VERSION: u32 = 3;

const UNIFIED_DIFF_NO_NEWLINE_MARKERS: [&str; 2] =
    [r"\ No newline at end of file", " No newline at end of file"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnFileCaptureConfig {
    pub capture_max_entries: usize,
    pub capture_max_file_bytes: usize,
    pub capture_max_total_bytes: usize,
    pub diff_text_max_bytes: usize,
    pub diff_text_max_lines: usize,
}

impl Default for TurnFileCaptureConfig {
    fn default() -> Self {
        crate::config::TurnFilesConfig::default().into()
    }
}

impl From<crate::config::TurnFilesConfig> for TurnFileCaptureConfig {
    fn from(config: crate::config::TurnFilesConfig) -> Self {
        Self {
            capture_max_entries: config.capture_max_entries,
            capture_max_file_bytes: config.capture_max_file_bytes,
            capture_max_total_bytes: config.capture_max_total_bytes,
            diff_text_max_bytes: config.diff_text_max_bytes,
            diff_text_max_lines: config.diff_text_max_lines,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileVersionRef {
    pub id: String,
    pub storage_kind: FileVersionStorageKind,
    pub content_hash: String,
    pub byte_length: u64,
    pub encoding: Option<String>,
    pub line_ending: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileVersionStorageKind {
    CapturedBlob,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileChange {
    pub id: String,
    pub change_kind: FileChangeKind,
    pub logical_path: String,
    pub previous_logical_path: Option<String>,
    pub mime_type: Option<String>,
    pub text: bool,
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
    pub before_version: Option<FileVersionRef>,
    pub after_version: Option<FileVersionRef>,
    pub limitation_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileChangeSummary {
    pub file_count: usize,
    pub added_files: usize,
    pub modified_files: usize,
    pub deleted_files: usize,
    pub added_lines: u64,
    pub deleted_lines: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnFileChangeSetStatus {
    Capturing,
    Finalized,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileChangeSet {
    #[serde(default = "legacy_change_set_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub turn_id: String,
    pub prompt_event_id: String,
    pub branch_id: String,
    pub status: TurnFileChangeSetStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub summary: TurnFileChangeSummary,
    pub changes: Vec<TurnFileChange>,
    pub limitation_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFileMutation {
    pub idempotency_key: String,
    pub turn_id: String,
    pub prompt_event_id: String,
    pub branch_id: String,
    pub tool_call_id: String,
    pub event_seq: u64,
    pub content_index: usize,
    pub logical_path: String,
    pub before_version: Option<FileVersionRef>,
    pub after_version: Option<FileVersionRef>,
    pub captured_at: String,
    pub limitation_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedToolDiff {
    pub content_index: usize,
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileComparison {
    pub change_set_id: String,
    pub change_id: String,
    pub path: String,
    pub stats: FileComparisonStats,
    pub before: Option<CapturedTextSnapshot>,
    pub after: Option<CapturedTextSnapshot>,
    pub limitation_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileComparisonStats {
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturedTextSnapshot {
    pub version: FileVersionRef,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct TurnFileStore {
    attempt_dir: Utf8PathBuf,
    config: TurnFileCaptureConfig,
}

impl TurnFileStore {
    pub fn new(attempt_dir: Utf8PathBuf, config: TurnFileCaptureConfig) -> Self {
        Self {
            attempt_dir,
            config,
        }
    }

    pub fn capture_event_diffs(
        &self,
        turn_id: &str,
        prompt_event_id: &str,
        branch_id: &str,
        tool_call_id: &str,
        event_seq: u64,
        captured_at: &str,
        raw: &Value,
    ) -> Result<usize> {
        let diffs = extract_standard_tool_diffs(raw);
        if diffs.is_empty() {
            return Ok(0);
        }
        let existing = self.load_mutations()?;
        let existing_keys = existing
            .iter()
            .map(|mutation| mutation.idempotency_key.as_str())
            .collect::<HashSet<_>>();
        let existing_turn_bytes = existing
            .iter()
            .filter(|mutation| mutation.turn_id == turn_id)
            .map(|mutation| {
                mutation
                    .before_version
                    .as_ref()
                    .map_or(0, |version| version.byte_length as usize)
                    + mutation
                        .after_version
                        .as_ref()
                        .map_or(0, |version| version.byte_length as usize)
            })
            .sum::<usize>();
        let mut captured_turn_bytes = 0;
        let mut captured = 0;
        for diff in diffs {
            let logical_path = normalize_logical_path(&diff.path)?;
            let total_bytes = diff.old_text.as_ref().map_or(0, String::len)
                + diff.new_text.as_ref().map_or(0, String::len);
            let mut limitation_code = None;
            let (before_version, after_version) = if total_bytes
                > self.config.capture_max_file_bytes
                || existing_turn_bytes + captured_turn_bytes + total_bytes
                    > self.config.capture_max_total_bytes
            {
                limitation_code = Some(CAPTURE_LIMIT_EXCEEDED.to_string());
                (None, None)
            } else {
                (
                    diff.old_text
                        .as_deref()
                        .map(|text| self.write_blob(text))
                        .transpose()?,
                    diff.new_text
                        .as_deref()
                        .map(|text| self.write_blob(text))
                        .transpose()?,
                )
            };
            if limitation_code.is_none() {
                captured_turn_bytes += total_bytes;
            }
            let idempotency_key = mutation_key(
                turn_id,
                tool_call_id,
                event_seq,
                diff.content_index,
                before_version.as_ref(),
                after_version.as_ref(),
            );
            if existing_keys.contains(idempotency_key.as_str()) {
                continue;
            }
            append_jsonl_durable(
                &self.mutation_journal_path(),
                &TurnFileMutation {
                    idempotency_key,
                    turn_id: turn_id.to_string(),
                    prompt_event_id: prompt_event_id.to_string(),
                    branch_id: branch_id.to_string(),
                    tool_call_id: tool_call_id.to_string(),
                    event_seq,
                    content_index: diff.content_index,
                    logical_path,
                    before_version,
                    after_version,
                    captured_at: captured_at.to_string(),
                    limitation_code,
                },
            )?;
            captured += 1;
        }
        Ok(captured)
    }

    pub fn finalize_turn_branch(
        &self,
        turn_id: &str,
        prompt_event_id: &str,
        branch_id: &str,
        started_at: &str,
        finished_at: &str,
    ) -> Result<Option<TurnFileChangeSet>> {
        let mutations = self
            .load_mutations()?
            .into_iter()
            .filter(|mutation| mutation.turn_id == turn_id && mutation.branch_id == branch_id)
            .collect::<Vec<_>>();
        if mutations.is_empty() {
            return Ok(None);
        }
        let mut mutations = mutations
            .into_iter()
            .map(|mutation| self.normalize_mutation_versions(mutation))
            .collect::<Result<Vec<_>>>()?;
        mutations = latest_tool_diff_revisions(mutations);
        mutations.sort_by_key(|mutation| (mutation.event_seq, mutation.content_index));
        let entry_limit_exceeded = mutations.len() > self.config.capture_max_entries;
        let mut by_path = BTreeMap::<String, Vec<TurnFileMutation>>::new();
        for mutation in mutations.into_iter().take(self.config.capture_max_entries) {
            by_path
                .entry(mutation.logical_path.clone())
                .or_default()
                .push(mutation);
        }

        let mut limitations = entry_limit_exceeded
            .then(|| vec![CAPTURE_LIMIT_EXCEEDED.to_string()])
            .unwrap_or_default();
        let mut changes = Vec::new();
        for (path, chain) in by_path {
            let first = chain.first().expect("mutation chain is non-empty");
            let last = chain.last().expect("mutation chain is non-empty");
            let mut limitation = chain
                .iter()
                .find_map(|mutation| mutation.limitation_code.clone());
            for adjacent in chain.windows(2) {
                if version_hash(adjacent[0].after_version.as_ref())
                    != version_hash(adjacent[1].before_version.as_ref())
                {
                    limitation = Some(NON_LINEAR_MUTATION.to_string());
                    break;
                }
            }
            if let Some(code) = limitation.as_ref()
                && !limitations.contains(code)
            {
                limitations.push(code.clone());
            }
            let before = first.before_version.clone();
            let after = last.after_version.clone();
            if version_hash(before.as_ref()) == version_hash(after.as_ref()) {
                continue;
            }
            let change_kind = match (&before, &after) {
                (None, Some(_)) => FileChangeKind::Added,
                (Some(_), None) => FileChangeKind::Deleted,
                (Some(_), Some(_)) => FileChangeKind::Modified,
                (None, None) => continue,
            };
            let (added_lines, deleted_lines) = match (&before, &after, limitation.as_deref()) {
                (_, _, Some(CAPTURE_LIMIT_EXCEEDED)) => (None, None),
                (before, after, _) => {
                    let before_text = before
                        .as_ref()
                        .map(|version| self.read_blob(version))
                        .transpose()?;
                    let after_text = after
                        .as_ref()
                        .map(|version| self.read_blob(version))
                        .transpose()?;
                    line_stats(
                        before_text.as_deref().unwrap_or(""),
                        after_text.as_deref().unwrap_or(""),
                    )
                }
            };
            let change_id = stable_id(&format!("{turn_id}\0{branch_id}\0{path}"));
            changes.push(TurnFileChange {
                id: format!("turn-file-change-{change_id}"),
                change_kind,
                logical_path: path,
                previous_logical_path: None,
                mime_type: None,
                text: true,
                added_lines,
                deleted_lines,
                before_version: before,
                after_version: after,
                limitation_code: limitation,
            });
        }
        if changes.is_empty() {
            return Ok(None);
        }
        changes.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
        let summary = summarize(&changes);
        let id = change_set_id(turn_id, branch_id);
        let change_set = TurnFileChangeSet {
            schema_version: TURN_FILE_CHANGE_SET_SCHEMA_VERSION,
            id: id.clone(),
            turn_id: turn_id.to_string(),
            prompt_event_id: prompt_event_id.to_string(),
            branch_id: branch_id.to_string(),
            status: if limitations.is_empty() {
                TurnFileChangeSetStatus::Finalized
            } else {
                TurnFileChangeSetStatus::Partial
            },
            started_at: started_at.to_string(),
            finished_at: Some(finished_at.to_string()),
            summary,
            changes,
            limitation_codes: limitations,
        };
        write_json(&self.change_set_path(&id), &change_set)?;
        Ok(Some(change_set))
    }

    pub fn load_change_set(&self, change_set_id: &str) -> Result<TurnFileChangeSet> {
        validate_identifier(change_set_id)?;
        let path = self.change_set_path(change_set_id);
        if !path.exists() {
            return Err(anyhow!(CHANGE_SET_NOT_FOUND));
        }
        let change_set: TurnFileChangeSet =
            read_json(&path).map_err(|error| anyhow!("{CHANGE_SET_NOT_FOUND}: {error}"))?;
        if change_set.schema_version >= TURN_FILE_CHANGE_SET_SCHEMA_VERSION {
            return Ok(change_set);
        }
        self.finalize_turn_branch(
            &change_set.turn_id,
            &change_set.prompt_event_id,
            &change_set.branch_id,
            &change_set.started_at,
            change_set
                .finished_at
                .as_deref()
                .unwrap_or(&change_set.started_at),
        )?
        .ok_or_else(|| anyhow!(CHANGE_SET_NOT_FOUND))
    }

    pub fn comparison(&self, change_set_id: &str, change_id: &str) -> Result<FileComparison> {
        let change_set = self.load_change_set(change_set_id)?;
        let change = change_set
            .changes
            .iter()
            .find(|change| change.id == change_id)
            .ok_or_else(|| anyhow!(CHANGE_SET_NOT_FOUND))?;
        let before = change
            .before_version
            .as_ref()
            .map(|version| self.snapshot(version))
            .transpose()?;
        let after = change
            .after_version
            .as_ref()
            .map(|version| self.snapshot(version))
            .transpose()?;
        let rendered_bytes = before.as_ref().map_or(0, |snapshot| snapshot.content.len())
            + after.as_ref().map_or(0, |snapshot| snapshot.content.len());
        let rendered_lines = before
            .as_ref()
            .map_or(0, |snapshot| snapshot.content.lines().count())
            + after
                .as_ref()
                .map_or(0, |snapshot| snapshot.content.lines().count());
        let too_large = rendered_bytes > self.config.diff_text_max_bytes
            || rendered_lines > self.config.diff_text_max_lines;
        Ok(FileComparison {
            change_set_id: change_set_id.to_string(),
            change_id: change_id.to_string(),
            path: change.logical_path.clone(),
            stats: FileComparisonStats {
                added_lines: change.added_lines,
                deleted_lines: change.deleted_lines,
            },
            before: (!too_large).then_some(before).flatten(),
            after: (!too_large).then_some(after).flatten(),
            limitation_code: too_large
                .then(|| "turn-files.diff-too-large".to_string())
                .or_else(|| change.limitation_code.clone()),
        })
    }

    pub fn snapshot(&self, version: &FileVersionRef) -> Result<CapturedTextSnapshot> {
        Ok(CapturedTextSnapshot {
            version: version.clone(),
            content: self.read_blob(version)?,
        })
    }

    pub(crate) fn write_blob(&self, content: &str) -> Result<FileVersionRef> {
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let path = self.blob_path(&hash);
        if !path.exists() {
            ensure_parent_dir(&path)?;
            atomic_write_file(path.as_std_path(), |file| -> Result<()> {
                file.write_all(content.as_bytes())?;
                Ok(())
            })?;
        }
        Ok(FileVersionRef {
            id: format!("captured-{hash}"),
            storage_kind: FileVersionStorageKind::CapturedBlob,
            content_hash: hash,
            byte_length: content.len() as u64,
            encoding: Some("utf-8".to_string()),
            line_ending: Some(detect_line_ending(content).to_string()),
        })
    }

    pub(crate) fn read_blob(&self, version: &FileVersionRef) -> Result<String> {
        validate_hash(&version.content_hash)?;
        let path = self.blob_path(&version.content_hash);
        let bytes = std::fs::read(path.as_std_path()).map_err(|_| anyhow!(VERSION_NOT_FOUND))?;
        if blake3::hash(&bytes).to_hex().as_str() != version.content_hash {
            return Err(anyhow!(BLOB_CORRUPTED));
        }
        String::from_utf8(bytes).map_err(|_| anyhow!(BLOB_CORRUPTED))
    }

    fn normalize_mutation_versions(
        &self,
        mut mutation: TurnFileMutation,
    ) -> Result<TurnFileMutation> {
        mutation.before_version = self.normalize_version(mutation.before_version)?;
        mutation.after_version = self.normalize_version(mutation.after_version)?;
        Ok(mutation)
    }

    fn normalize_version(&self, version: Option<FileVersionRef>) -> Result<Option<FileVersionRef>> {
        let Some(version) = version else {
            return Ok(None);
        };
        let content = self.read_blob(&version)?;
        let normalized = strip_unified_diff_no_newline_markers(&content);
        if normalized == content {
            Ok(Some(version))
        } else {
            self.write_blob(&normalized).map(Some)
        }
    }

    fn load_mutations(&self) -> Result<Vec<TurnFileMutation>> {
        let path = self.mutation_journal_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(path.as_std_path())?;
        let mut mutations = Vec::new();
        let mut seen = HashSet::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let mutation: TurnFileMutation = serde_json::from_str(&line)
                .with_context(|| format!("invalid turn file mutation journal entry: {line}"))?;
            if seen.insert(mutation.idempotency_key.clone()) {
                mutations.push(mutation);
            }
        }
        Ok(mutations)
    }

    fn mutation_journal_path(&self) -> Utf8PathBuf {
        self.attempt_dir.join("acp.turn-file-mutations.jsonl")
    }

    fn change_set_path(&self, change_set_id: &str) -> Utf8PathBuf {
        self.attempt_dir
            .join("turn-file-change-sets")
            .join(format!("{change_set_id}.json"))
    }

    fn blob_path(&self, hash: &str) -> Utf8PathBuf {
        self.attempt_dir
            .join("acp.file-blobs")
            .join(&hash[..2])
            .join(hash)
    }
}

pub fn extract_standard_tool_diffs(raw: &Value) -> Vec<CapturedToolDiff> {
    let content = raw.get("content").or_else(|| {
        raw.get("toolCall")
            .and_then(|tool_call| tool_call.get("content"))
    });
    let Some(Value::Array(items)) = content else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(content_index, item)| {
            (item.get("type").and_then(Value::as_str) == Some("diff")).then(|| {
                let path = item.get("path")?.as_str()?.to_string();
                let old_text = optional_text(item, "oldText")?
                    .map(|text| strip_unified_diff_no_newline_markers(&text));
                let new_text = optional_text(item, "newText")?
                    .map(|text| strip_unified_diff_no_newline_markers(&text));
                if old_text.is_none() && new_text.is_none() {
                    return None;
                }
                Some(CapturedToolDiff {
                    content_index,
                    path,
                    old_text,
                    new_text,
                })
            })?
        })
        .collect()
}

fn optional_text(value: &Value, key: &str) -> Option<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    }
}

fn strip_unified_diff_no_newline_markers(content: &str) -> String {
    let ends_with_marker = content
        .lines()
        .next_back()
        .is_some_and(|line| UNIFIED_DIFF_NO_NEWLINE_MARKERS.contains(&line));
    let mut normalized = content
        .split_inclusive('\n')
        .filter(|line| {
            let text = line.strip_suffix('\n').unwrap_or(line);
            let text = text.strip_suffix('\r').unwrap_or(text);
            !UNIFIED_DIFF_NO_NEWLINE_MARKERS.contains(&text)
        })
        .collect::<String>();
    if ends_with_marker {
        if normalized.ends_with("\r\n") {
            normalized.truncate(normalized.len() - 2);
        } else if normalized.ends_with('\n') {
            normalized.pop();
        }
    }
    normalized
}

fn normalize_logical_path(path: &str) -> Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty()
        || trimmed.contains('\0')
        || trimmed.contains('\n')
        || trimmed.contains('\r')
    {
        return Err(anyhow!(INVALID_TOOL_DIFF));
    }
    Ok(trimmed.replace('\\', "/"))
}

fn latest_tool_diff_revisions(mutations: Vec<TurnFileMutation>) -> Vec<TurnFileMutation> {
    let mut latest_event_by_operation = HashMap::<(String, String), u64>::new();
    for mutation in &mutations {
        latest_event_by_operation
            .entry((mutation.tool_call_id.clone(), mutation.logical_path.clone()))
            .and_modify(|latest| *latest = (*latest).max(mutation.event_seq))
            .or_insert(mutation.event_seq);
    }
    mutations
        .into_iter()
        .filter(|mutation| {
            latest_event_by_operation
                .get(&(mutation.tool_call_id.clone(), mutation.logical_path.clone()))
                .is_some_and(|latest| *latest == mutation.event_seq)
        })
        .collect()
}

fn legacy_change_set_schema_version() -> u32 {
    1
}

fn mutation_key(
    turn_id: &str,
    tool_call_id: &str,
    event_seq: u64,
    content_index: usize,
    before: Option<&FileVersionRef>,
    after: Option<&FileVersionRef>,
) -> String {
    stable_id(&format!(
        "{turn_id}\0{tool_call_id}\0{event_seq}\0{content_index}\0{}\0{}",
        version_hash(before).unwrap_or("none"),
        version_hash(after).unwrap_or("none")
    ))
}

fn stable_id(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex()[..32].to_string()
}

pub fn change_set_id(turn_id: &str, branch_id: &str) -> String {
    format!(
        "turn-files-{}",
        stable_id(&format!("{turn_id}\0{branch_id}"))
    )
}

fn version_hash(version: Option<&FileVersionRef>) -> Option<&str> {
    version.map(|version| version.content_hash.as_str())
}

fn summarize(changes: &[TurnFileChange]) -> TurnFileChangeSummary {
    let mut summary = TurnFileChangeSummary {
        file_count: changes.len(),
        ..TurnFileChangeSummary::default()
    };
    for change in changes {
        match change.change_kind {
            FileChangeKind::Added => summary.added_files += 1,
            FileChangeKind::Modified | FileChangeKind::Renamed => summary.modified_files += 1,
            FileChangeKind::Deleted => summary.deleted_files += 1,
        }
        summary.added_lines += change.added_lines.unwrap_or_default();
        summary.deleted_lines += change.deleted_lines.unwrap_or_default();
    }
    summary
}

fn line_stats(before: &str, after: &str) -> (Option<u64>, Option<u64>) {
    let diff = TextDiff::from_lines(before, after);
    let mut added = 0;
    let mut deleted = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => deleted += 1,
            ChangeTag::Equal => {}
        }
    }
    (Some(added), Some(deleted))
}

fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "crlf"
    } else {
        "lf"
    }
}

fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(anyhow!(VERSION_NOT_FOUND))
    }
}

fn validate_identifier(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(anyhow!(CHANGE_SET_NOT_FOUND))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, TurnFileStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        (
            dir,
            TurnFileStore::new(path, TurnFileCaptureConfig::default()),
        )
    }

    fn raw(content: Value) -> Value {
        serde_json::json!({ "sessionUpdate": "tool_call_update", "content": content })
    }

    #[test]
    fn extracts_only_standard_diff_content() {
        let diffs = extract_standard_tool_diffs(&raw(serde_json::json!([
            { "type": "content", "content": { "type": "text", "text": "ignored" } },
            { "type": "diff", "path": "src/main.rs", "oldText": "a\n", "newText": "b\n" }
        ])));
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].content_index, 1);
        assert_eq!(diffs[0].path, "src/main.rs");
    }

    #[test]
    fn strips_unified_diff_no_newline_metadata_from_captured_text() {
        let diffs = extract_standard_tool_diffs(&raw(serde_json::json!([{
            "type": "diff",
            "path": "poem.md",
            "oldText": "静夜思\n[唐] 李白\n\n床前明月光，\n疑是地上霜。\n举头望明月，\n低头思故乡。\n No newline at end of file\n No newline at end of file",
            "newText": "春望\n[唐] 杜甫\n\n No newline at end of file\n国破山河在，城春草木深。\n感时花溅泪，恨别鸟惊心。\n烽火连三月，家书抵万金。\n白头搔更短，浑欲不胜簪。\n No newline at end of file"
        }])));

        assert_eq!(
            diffs[0].old_text.as_deref(),
            Some("静夜思\n[唐] 李白\n\n床前明月光，\n疑是地上霜。\n举头望明月，\n低头思故乡。")
        );
        assert_eq!(
            diffs[0].new_text.as_deref(),
            Some(
                "春望\n[唐] 杜甫\n\n国破山河在，城春草木深。\n感时花溅泪，恨别鸟惊心。\n烽火连三月，家书抵万金。\n白头搔更短，浑欲不胜簪。"
            )
        );
    }

    #[test]
    fn folds_write_then_edit_without_reading_workspace() {
        let (_dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "write",
                10,
                "10Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "new.txt", "oldText": null, "newText": "B\n" }
                ])),
            )
            .unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                11,
                "11Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "new.txt", "oldText": "B\n", "newText": "C\n" }
                ])),
            )
            .unwrap();
        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "12Z")
            .unwrap()
            .unwrap();
        assert_eq!(set.summary.added_files, 1);
        assert_eq!(set.changes[0].change_kind, FileChangeKind::Added);
        let comparison = store.comparison(&set.id, &set.changes[0].id).unwrap();
        assert!(comparison.before.is_none());
        assert_eq!(comparison.after.unwrap().content, "C\n");
    }

    #[test]
    fn folds_modified_chain_to_first_before_and_last_after() {
        let (_dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "write",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "A\n", "newText": "B\n" }
                ])),
            )
            .unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                3,
                "3Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "B\n", "newText": "C\nD\n" }
                ])),
            )
            .unwrap();
        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "4Z")
            .unwrap()
            .unwrap();
        let change = &set.changes[0];
        assert_eq!(change.change_kind, FileChangeKind::Modified);
        assert_eq!(
            (change.added_lines, change.deleted_lines),
            (Some(2), Some(1))
        );
        let comparison = store.comparison(&set.id, &change.id).unwrap();
        assert_eq!(comparison.before.unwrap().content, "A\n");
        assert_eq!(comparison.after.unwrap().content, "C\nD\n");
    }

    #[test]
    fn duplicate_update_is_idempotent() {
        let (_dir, store) = store();
        let update = raw(serde_json::json!([
            { "type": "diff", "path": "a.txt", "oldText": "A", "newText": "B" }
        ]));
        assert_eq!(
            store
                .capture_event_diffs("turn", "prompt", "root", "edit", 2, "2Z", &update)
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .capture_event_diffs("turn", "prompt", "root", "edit", 2, "2Z", &update)
                .unwrap(),
            0
        );
    }

    #[test]
    fn latest_update_of_one_tool_call_replaces_earlier_diff_context() {
        let (_dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "poem.md", "oldText": "author", "newText": "author\nnew poem" }
                ])),
            )
            .unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                3,
                "3Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "poem.md", "oldText": "last line\n\nauthor", "newText": "last line\n\nauthor\n\nnew poem" }
                ])),
            )
            .unwrap();

        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "4Z")
            .unwrap()
            .unwrap();
        assert_eq!(set.status, TurnFileChangeSetStatus::Finalized);
        assert!(set.limitation_codes.is_empty());
        let comparison = store.comparison(&set.id, &set.changes[0].id).unwrap();
        assert_eq!(comparison.before.unwrap().content, "last line\n\nauthor");
        assert_eq!(
            comparison.after.unwrap().content,
            "last line\n\nauthor\n\nnew poem"
        );
    }

    #[test]
    fn loading_legacy_change_set_rebuilds_it_from_the_durable_journal() {
        let (_dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "A", "newText": "B" }
                ])),
            )
            .unwrap();
        let mut legacy = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "4Z")
            .unwrap()
            .unwrap();
        legacy.schema_version = 1;
        write_json(&store.change_set_path(&legacy.id), &legacy).unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                3,
                "3Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "context\nA", "newText": "context\nB" }
                ])),
            )
            .unwrap();

        let rebuilt = store.load_change_set(&legacy.id).unwrap();
        assert_eq!(rebuilt.schema_version, TURN_FILE_CHANGE_SET_SCHEMA_VERSION);
        let comparison = store
            .comparison(&rebuilt.id, &rebuilt.changes[0].id)
            .unwrap();
        assert_eq!(comparison.before.unwrap().content, "context\nA");
        assert_eq!(comparison.after.unwrap().content, "context\nB");
    }

    #[test]
    fn loading_schema_two_change_set_repairs_no_newline_metadata_from_journal() {
        let (_dir, store) = store();
        let before_version = store
            .write_blob("静夜思\n No newline at end of file\n No newline at end of file")
            .unwrap();
        let after_version = store
            .write_blob(" No newline at end of file\n春望\n No newline at end of file")
            .unwrap();
        append_jsonl_durable(
            &store.mutation_journal_path(),
            &TurnFileMutation {
                idempotency_key: "legacy-mutation".to_string(),
                turn_id: "turn".to_string(),
                prompt_event_id: "prompt".to_string(),
                branch_id: "root".to_string(),
                tool_call_id: "edit".to_string(),
                event_seq: 2,
                content_index: 0,
                logical_path: "poem.md".to_string(),
                before_version: Some(before_version.clone()),
                after_version: Some(after_version.clone()),
                captured_at: "2Z".to_string(),
                limitation_code: None,
            },
        )
        .unwrap();
        let mut schema_two = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "3Z")
            .unwrap()
            .unwrap();
        schema_two.schema_version = 2;
        schema_two.changes[0].before_version = Some(before_version);
        schema_two.changes[0].after_version = Some(after_version);
        write_json(&store.change_set_path(&schema_two.id), &schema_two).unwrap();

        let rebuilt = store.load_change_set(&schema_two.id).unwrap();
        assert_eq!(rebuilt.schema_version, TURN_FILE_CHANGE_SET_SCHEMA_VERSION);
        let comparison = store
            .comparison(&rebuilt.id, &rebuilt.changes[0].id)
            .unwrap();
        assert_eq!(comparison.before.unwrap().content, "静夜思");
        assert_eq!(comparison.after.unwrap().content, "春望");
    }

    #[test]
    fn non_linear_chain_is_partial_and_keeps_evidence() {
        let (_dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "one",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "A", "newText": "B" }
                ])),
            )
            .unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "two",
                3,
                "3Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "X", "newText": "C" }
                ])),
            )
            .unwrap();
        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "4Z")
            .unwrap()
            .unwrap();
        assert_eq!(set.status, TurnFileChangeSetStatus::Partial);
        assert_eq!(
            set.changes[0].limitation_code.as_deref(),
            Some(NON_LINEAR_MUTATION)
        );
    }

    #[test]
    fn restored_original_is_not_rendered() {
        let (_dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "one",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "A", "newText": "B" }
                ])),
            )
            .unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "two",
                3,
                "3Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": "B", "newText": "A" }
                ])),
            )
            .unwrap();
        assert!(
            store
                .finalize_turn_branch("turn", "prompt", "root", "1Z", "4Z")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn captured_comparison_is_immutable_when_a_workspace_file_changes_later() {
        let (dir, store) = store();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "write",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "workspace.txt", "oldText": null, "newText": "captured\n" }
                ])),
            )
            .unwrap();
        std::fs::write(dir.path().join("workspace.txt"), "live-newer\n").unwrap();

        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "3Z")
            .unwrap()
            .unwrap();
        let comparison = store.comparison(&set.id, &set.changes[0].id).unwrap();

        assert_eq!(comparison.after.unwrap().content, "captured\n");
    }

    #[test]
    fn rejects_change_set_path_traversal_identifiers() {
        let (_dir, store) = store();
        let error = store.load_change_set("../outside").unwrap_err();
        assert!(error.to_string().starts_with(CHANGE_SET_NOT_FOUND));
    }

    #[test]
    fn entry_limit_marks_the_change_set_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = TurnFileStore::new(
            path,
            TurnFileCaptureConfig {
                capture_max_entries: 1,
                ..TurnFileCaptureConfig::default()
            },
        );
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "write",
                2,
                "2Z",
                &raw(serde_json::json!([
                    { "type": "diff", "path": "a.txt", "oldText": null, "newText": "a" },
                    { "type": "diff", "path": "b.txt", "oldText": null, "newText": "b" }
                ])),
            )
            .unwrap();

        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "3Z")
            .unwrap()
            .unwrap();
        assert_eq!(set.status, TurnFileChangeSetStatus::Partial);
        assert_eq!(set.summary.file_count, 1);
        assert!(
            set.limitation_codes
                .contains(&CAPTURE_LIMIT_EXCEEDED.to_string())
        );
    }
}
