use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};

use anyhow::{Context, Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use similar::{ChangeTag, TextDiff};
use walkdir::WalkDir;

use crate::storage::{
    append_jsonl_durable, atomic_write_file, ensure_parent_dir, read_json, write_json,
};

pub const CHANGE_SET_NOT_FOUND: &str = "turn-files.change-set-not-found";
pub const VERSION_NOT_FOUND: &str = "turn-files.version-not-found";
pub const BLOB_CORRUPTED: &str = "turn-files.blob-corrupted";
pub const INVALID_TOOL_DIFF: &str = "turn-files.invalid-tool-diff";
pub const NON_LINEAR_MUTATION: &str = "turn-files.non-linear-mutation";
pub const CAPTURE_LIMIT_EXCEEDED: &str = "turn-files.capture-limit-exceeded";
pub const ATTACHMENT_BASELINE_MISSING: &str = "turn-files.attachment-baseline-missing";
pub const ATTACHMENT_SCAN_FAILED: &str = "turn-files.attachment-scan-failed";
pub const ATTACHMENT_SCAN_LIMIT_EXCEEDED: &str = "turn-files.attachment-scan-limit-exceeded";
pub const ATTACHMENT_NOT_FOUND: &str = "turn-files.attachment-not-found";
pub const ATTACHMENT_ACCESS_DENIED: &str = "turn-files.attachment-access-denied";
pub const TURN_FILE_CHANGE_SET_SCHEMA_VERSION: u32 = 5;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnAttachment {
    pub id: String,
    pub relative_path: String,
    pub name: String,
    pub byte_length: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnFileToolTerminalOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl TurnFileToolTerminalOutcome {
    pub fn from_status(status: Option<&str>) -> Option<Self> {
        match status?.trim().to_ascii_lowercase().as_str() {
            "completed" | "success" | "succeeded" => Some(Self::Succeeded),
            "failed" | "error" => Some(Self::Failed),
            "cancelled" | "canceled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    fn committed(self) -> bool {
        self == Self::Succeeded
    }
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
    #[serde(default)]
    pub attachments: Vec<TurnAttachment>,
    pub limitation_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TurnAttachmentBaseline {
    schema_version: u32,
    turn_id: String,
    relative_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct ScannedAttachment {
    attachment: TurnAttachment,
    canonical_path_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct TurnAttachmentDelta {
    attachments: Vec<TurnAttachment>,
    canonical_path_keys: HashSet<String>,
    limitation_codes: Vec<String>,
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

    pub fn capture_attachment_baseline(&self, turn_id: &str) -> Result<()> {
        let path = self.attachment_baseline_path(turn_id);
        if path.exists() {
            return Ok(());
        }
        let relative_paths = self
            .scan_attachments(turn_id)?
            .into_iter()
            .map(|entry| entry.attachment.relative_path)
            .collect();
        write_json(
            &path,
            &TurnAttachmentBaseline {
                schema_version: 1,
                turn_id: turn_id.to_string(),
                relative_paths,
            },
        )
    }

    pub fn collect_turn_attachment_delta(&self, turn_id: &str) -> Result<TurnAttachmentDelta> {
        let baseline_path = self.attachment_baseline_path(turn_id);
        if !baseline_path.exists() {
            return Ok(TurnAttachmentDelta {
                limitation_codes: vec![ATTACHMENT_BASELINE_MISSING.to_string()],
                ..TurnAttachmentDelta::default()
            });
        }
        let baseline = match read_json::<TurnAttachmentBaseline>(&baseline_path) {
            Ok(baseline) if baseline.schema_version == 1 && baseline.turn_id == turn_id => baseline,
            Ok(_) | Err(_) => {
                return Ok(TurnAttachmentDelta {
                    limitation_codes: vec![ATTACHMENT_SCAN_FAILED.to_string()],
                    ..TurnAttachmentDelta::default()
                });
            }
        };
        let scanned = match self.scan_attachments(turn_id) {
            Ok(scanned) => scanned,
            Err(error) => {
                let code = if error.to_string().contains(ATTACHMENT_SCAN_LIMIT_EXCEEDED) {
                    ATTACHMENT_SCAN_LIMIT_EXCEEDED
                } else {
                    ATTACHMENT_SCAN_FAILED
                };
                return Ok(TurnAttachmentDelta {
                    limitation_codes: vec![code.to_string()],
                    ..TurnAttachmentDelta::default()
                });
            }
        };
        let baseline_paths = baseline.relative_paths.into_iter().collect::<HashSet<_>>();
        let mut attachments = Vec::new();
        let mut canonical_path_keys = HashSet::new();
        for entry in scanned {
            if baseline_paths.contains(&entry.attachment.relative_path) {
                continue;
            }
            canonical_path_keys.insert(entry.canonical_path_key);
            attachments.push(entry.attachment);
        }
        attachments.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(TurnAttachmentDelta {
            attachments,
            canonical_path_keys,
            limitation_codes: Vec::new(),
        })
    }

    pub fn finalize_turn_branch(
        &self,
        turn_id: &str,
        prompt_event_id: &str,
        branch_id: &str,
        started_at: &str,
        finished_at: &str,
        tool_outcomes: &HashMap<String, TurnFileToolTerminalOutcome>,
    ) -> Result<Option<TurnFileChangeSet>> {
        self.finalize_turn_branch_with_attachments(
            turn_id,
            prompt_event_id,
            branch_id,
            started_at,
            finished_at,
            tool_outcomes,
            None,
            &TurnAttachmentDelta::default(),
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finalize_turn_branch_with_attachments(
        &self,
        turn_id: &str,
        prompt_event_id: &str,
        branch_id: &str,
        started_at: &str,
        finished_at: &str,
        tool_outcomes: &HashMap<String, TurnFileToolTerminalOutcome>,
        workspace_dir: Option<&Utf8Path>,
        attachment_delta: &TurnAttachmentDelta,
        include_attachments: bool,
    ) -> Result<Option<TurnFileChangeSet>> {
        let mutations = self
            .load_mutations()?
            .into_iter()
            .filter(|mutation| {
                mutation.turn_id == turn_id
                    && mutation.branch_id == branch_id
                    && tool_outcomes
                        .get(&mutation.tool_call_id)
                        .is_some_and(|outcome| outcome.committed())
            })
            .collect::<Vec<_>>();
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
            let change = TurnFileChange {
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
            };
            let is_new_attachment = change.change_kind == FileChangeKind::Added
                && canonical_change_path_key(&change.logical_path, workspace_dir)
                    .is_some_and(|key| attachment_delta.canonical_path_keys.contains(&key));
            if !is_new_attachment {
                changes.push(change);
            }
        }
        changes.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
        let attachments = include_attachments
            .then(|| attachment_delta.attachments.clone())
            .unwrap_or_default();
        if include_attachments {
            for code in &attachment_delta.limitation_codes {
                if !limitations.contains(code) {
                    limitations.push(code.clone());
                }
            }
        }
        if changes.is_empty() && attachments.is_empty() {
            return Ok(None);
        }
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
            attachments,
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
        let rebuilt = self.finalize_turn_branch(
            &change_set.turn_id,
            &change_set.prompt_event_id,
            &change_set.branch_id,
            &change_set.started_at,
            change_set
                .finished_at
                .as_deref()
                .unwrap_or(&change_set.started_at),
            &self.legacy_tool_outcomes(&change_set.turn_id, &change_set.branch_id)?,
        )?;
        if let Some(rebuilt) = rebuilt {
            return Ok(rebuilt);
        }
        let empty = TurnFileChangeSet {
            schema_version: TURN_FILE_CHANGE_SET_SCHEMA_VERSION,
            id: change_set.id,
            turn_id: change_set.turn_id,
            prompt_event_id: change_set.prompt_event_id,
            branch_id: change_set.branch_id,
            status: TurnFileChangeSetStatus::Finalized,
            started_at: change_set.started_at,
            finished_at: change_set.finished_at,
            summary: TurnFileChangeSummary::default(),
            changes: Vec::new(),
            attachments: Vec::new(),
            limitation_codes: Vec::new(),
        };
        write_json(&path, &empty)?;
        Ok(empty)
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

    pub fn resolve_attachment_path(
        &self,
        change_set_id: &str,
        attachment_id: &str,
    ) -> Result<Utf8PathBuf> {
        let change_set = self.load_change_set(change_set_id)?;
        let attachment = change_set
            .attachments
            .iter()
            .find(|attachment| attachment.id == attachment_id)
            .ok_or_else(|| anyhow!(ATTACHMENT_NOT_FOUND))?;
        let relative = Utf8Path::new(&attachment.relative_path);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    camino::Utf8Component::ParentDir
                        | camino::Utf8Component::RootDir
                        | camino::Utf8Component::Prefix(_)
                )
            })
        {
            return Err(anyhow!(ATTACHMENT_ACCESS_DENIED));
        }
        let root = self.attempt_dir.join("attachments");
        if std::fs::symlink_metadata(root.as_std_path())
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(anyhow!(ATTACHMENT_ACCESS_DENIED));
        }
        let candidate = root.join(relative);
        if std::fs::symlink_metadata(candidate.as_std_path())
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
        {
            return Err(anyhow!(ATTACHMENT_ACCESS_DENIED));
        }
        let canonical_root = std::fs::canonicalize(root.as_std_path())
            .map_err(|_| anyhow!(ATTACHMENT_NOT_FOUND))?;
        let canonical_path = std::fs::canonicalize(candidate.as_std_path())
            .map_err(|_| anyhow!(ATTACHMENT_NOT_FOUND))?;
        if !canonical_path.is_file() || !canonical_path.starts_with(&canonical_root) {
            return Err(anyhow!(ATTACHMENT_ACCESS_DENIED));
        }
        Utf8PathBuf::from_path_buf(canonical_path).map_err(|_| anyhow!(ATTACHMENT_ACCESS_DENIED))
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

    fn legacy_tool_outcomes(
        &self,
        turn_id: &str,
        branch_id: &str,
    ) -> Result<HashMap<String, TurnFileToolTerminalOutcome>> {
        let tool_call_ids = self
            .load_mutations()?
            .into_iter()
            .filter(|mutation| mutation.turn_id == turn_id && mutation.branch_id == branch_id)
            .map(|mutation| mutation.tool_call_id)
            .collect::<HashSet<_>>();
        let timeline_path =
            crate::acp::branches::branch_timeline_path(&self.attempt_dir, branch_id);
        let mut outcomes = HashMap::new();
        for event in crate::acp::events::load_timeline_items(&timeline_path)? {
            let Some(tool_call_id) = event
                .tool_call_id
                .as_deref()
                .filter(|tool_call_id| tool_call_ids.contains(*tool_call_id))
            else {
                continue;
            };
            let Some(outcome) = TurnFileToolTerminalOutcome::from_status(event.status.as_deref())
            else {
                continue;
            };
            outcomes.entry(tool_call_id.to_string()).or_insert(outcome);
        }
        Ok(outcomes)
    }

    fn scan_attachments(&self, turn_id: &str) -> Result<Vec<ScannedAttachment>> {
        let root = self.attempt_dir.join("attachments");
        if !root.exists() {
            return Ok(Vec::new());
        }
        if std::fs::symlink_metadata(root.as_std_path())
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
        {
            return Err(anyhow!(ATTACHMENT_SCAN_FAILED));
        }
        let canonical_root = std::fs::canonicalize(root.as_std_path())
            .with_context(|| format!("{ATTACHMENT_SCAN_FAILED}: {root}"))?;
        let mut scanned = Vec::new();
        let walker = WalkDir::new(root.as_std_path())
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || (!entry.file_type().is_symlink()
                        && !entry.file_name().to_string_lossy().starts_with('.'))
            });
        let mut visited_entries = 0usize;
        for entry in walker {
            let entry = entry.with_context(|| ATTACHMENT_SCAN_FAILED.to_string())?;
            if entry.depth() > 0 {
                visited_entries = visited_entries.saturating_add(1);
                if visited_entries > self.config.capture_max_entries {
                    return Err(anyhow!(ATTACHMENT_SCAN_LIMIT_EXCEEDED));
                }
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let canonical_path = std::fs::canonicalize(entry.path())
                .with_context(|| ATTACHMENT_SCAN_FAILED.to_string())?;
            if !canonical_path.starts_with(&canonical_root) {
                continue;
            }
            let relative_path = entry
                .path()
                .strip_prefix(root.as_std_path())
                .with_context(|| ATTACHMENT_SCAN_FAILED.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let Some(name) = entry
                .path()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
            else {
                continue;
            };
            let byte_length = entry
                .metadata()
                .with_context(|| ATTACHMENT_SCAN_FAILED.to_string())?
                .len();
            let attachment_id = stable_id(&format!("{turn_id}\0{relative_path}"));
            scanned.push(ScannedAttachment {
                attachment: TurnAttachment {
                    id: format!("turn-attachment-{attachment_id}"),
                    relative_path,
                    name,
                    byte_length,
                },
                canonical_path_key: canonical_path_key(&canonical_path),
            });
        }
        Ok(scanned)
    }

    fn mutation_journal_path(&self) -> Utf8PathBuf {
        self.attempt_dir.join("acp.turn-file-mutations.jsonl")
    }

    fn change_set_path(&self, change_set_id: &str) -> Utf8PathBuf {
        self.attempt_dir
            .join("turn-file-change-sets")
            .join(format!("{change_set_id}.json"))
    }

    fn attachment_baseline_path(&self, turn_id: &str) -> Utf8PathBuf {
        self.attempt_dir
            .join("turn-attachment-baselines")
            .join(format!("{}.json", stable_id(turn_id)))
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

fn canonical_change_path_key(
    logical_path: &str,
    workspace_dir: Option<&Utf8Path>,
) -> Option<String> {
    let path = Utf8Path::new(logical_path);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_dir?.join(path)
    };
    std::fs::canonicalize(resolved.as_std_path())
        .ok()
        .map(|path| canonical_path_key(&path))
}

fn canonical_path_key(path: &std::path::Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
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

    fn succeeded_tools(tool_call_ids: &[&str]) -> HashMap<String, TurnFileToolTerminalOutcome> {
        tool_call_ids
            .iter()
            .map(|tool_call_id| {
                (
                    (*tool_call_id).to_string(),
                    TurnFileToolTerminalOutcome::Succeeded,
                )
            })
            .collect()
    }

    fn write_tool_terminal(store: &TurnFileStore, tool_call_id: &str, status: &str) {
        let event = crate::acp::events::normalize_session_update(
            100,
            Some("session".to_string()),
            &serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": tool_call_id,
                "status": status,
            }),
        );
        crate::acp::events::write_timeline_items(
            &store.attempt_dir.join("acp.timeline.jsonl"),
            &[event],
        )
        .unwrap();
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "12Z",
                &succeeded_tools(&["write", "edit"]),
            )
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "4Z",
                &succeeded_tools(&["write", "edit"]),
            )
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
    fn finalizes_new_attachments_and_regular_changes_as_mutually_exclusive_sets() {
        let (dir, store) = store();
        let attachments_dir = dir.path().join("attachments");
        let workspace_dir = dir.path().join("workspace");
        std::fs::create_dir_all(&attachments_dir).unwrap();
        std::fs::create_dir_all(&workspace_dir).unwrap();
        let existing_attachment = attachments_dir.join("existing.md");
        let new_attachment = attachments_dir.join("report.md");
        std::fs::write(&existing_attachment, "before\n").unwrap();
        store.capture_attachment_baseline("turn").unwrap();

        std::fs::write(&existing_attachment, "after\n").unwrap();
        std::fs::write(&new_attachment, "report\n").unwrap();
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "write",
                2,
                "2Z",
                &raw(serde_json::json!([
                    {
                        "type": "diff",
                        "path": existing_attachment.to_string_lossy().to_string(),
                        "oldText": "before\n",
                        "newText": "after\n"
                    },
                    {
                        "type": "diff",
                        "path": new_attachment.to_string_lossy().to_string(),
                        "oldText": null,
                        "newText": "report\n"
                    }
                ])),
            )
            .unwrap();

        let attachment_delta = store.collect_turn_attachment_delta("turn").unwrap();
        let workspace_dir = Utf8PathBuf::from_path_buf(workspace_dir).unwrap();
        let set = store
            .finalize_turn_branch_with_attachments(
                "turn",
                "prompt",
                "root",
                "1Z",
                "3Z",
                &succeeded_tools(&["write"]),
                Some(&workspace_dir),
                &attachment_delta,
                true,
            )
            .unwrap()
            .unwrap();

        assert_eq!(set.attachments.len(), 1);
        assert_eq!(set.attachments[0].relative_path, "report.md");
        assert_eq!(set.changes.len(), 1);
        assert_eq!(set.changes[0].change_kind, FileChangeKind::Modified);
        assert!(set.changes[0].logical_path.ends_with("existing.md"));
        assert!(
            set.changes
                .iter()
                .all(|change| !change.logical_path.ends_with("report.md"))
        );
    }

    #[test]
    fn finalizes_an_attachment_only_turn_and_keeps_the_baseline_idempotent() {
        let (dir, store) = store();
        let attachments_dir = dir.path().join("attachments");
        std::fs::create_dir_all(&attachments_dir).unwrap();
        store.capture_attachment_baseline("turn").unwrap();
        std::fs::write(attachments_dir.join("report.md"), "report\n").unwrap();
        store.capture_attachment_baseline("turn").unwrap();

        let attachment_delta = store.collect_turn_attachment_delta("turn").unwrap();
        let set = store
            .finalize_turn_branch_with_attachments(
                "turn",
                "prompt",
                "root",
                "1Z",
                "2Z",
                &HashMap::new(),
                None,
                &attachment_delta,
                true,
            )
            .unwrap()
            .unwrap();

        assert_eq!(set.summary, TurnFileChangeSummary::default());
        assert!(set.changes.is_empty());
        assert_eq!(set.attachments.len(), 1);
        assert_eq!(set.attachments[0].relative_path, "report.md");
    }

    #[test]
    fn does_not_promote_existing_or_deleted_attachment_paths() {
        let (dir, store) = store();
        let attachments_dir = dir.path().join("attachments");
        std::fs::create_dir_all(&attachments_dir).unwrap();
        std::fs::write(attachments_dir.join("existing.md"), "before\n").unwrap();
        store.capture_attachment_baseline("turn").unwrap();
        std::fs::write(attachments_dir.join("existing.md"), "after\n").unwrap();
        std::fs::write(attachments_dir.join("transient.md"), "temporary\n").unwrap();
        std::fs::remove_file(attachments_dir.join("transient.md")).unwrap();

        let attachment_delta = store.collect_turn_attachment_delta("turn").unwrap();

        assert!(attachment_delta.attachments.is_empty());
        assert!(attachment_delta.canonical_path_keys.is_empty());
    }

    #[test]
    fn attachment_scan_limit_fails_closed_without_guessing_membership() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = TurnFileStore::new(
            path,
            TurnFileCaptureConfig {
                capture_max_entries: 1,
                ..TurnFileCaptureConfig::default()
            },
        );
        std::fs::create_dir_all(dir.path().join("attachments")).unwrap();
        store.capture_attachment_baseline("turn").unwrap();
        std::fs::write(dir.path().join("attachments/one.md"), "one").unwrap();
        std::fs::write(dir.path().join("attachments/two.md"), "two").unwrap();

        let attachment_delta = store.collect_turn_attachment_delta("turn").unwrap();

        assert!(attachment_delta.attachments.is_empty());
        assert!(attachment_delta.canonical_path_keys.is_empty());
        assert_eq!(
            attachment_delta.limitation_codes,
            vec![ATTACHMENT_SCAN_LIMIT_EXCEEDED.to_string()]
        );
    }

    #[test]
    fn resolves_only_attachment_manifest_members_inside_the_attempt_root() {
        let (dir, store) = store();
        let attachments_dir = dir.path().join("attachments");
        std::fs::create_dir_all(&attachments_dir).unwrap();
        store.capture_attachment_baseline("turn").unwrap();
        std::fs::write(attachments_dir.join("report.md"), "report\n").unwrap();
        let attachment_delta = store.collect_turn_attachment_delta("turn").unwrap();
        let set = store
            .finalize_turn_branch_with_attachments(
                "turn",
                "prompt",
                "root",
                "1Z",
                "2Z",
                &HashMap::new(),
                None,
                &attachment_delta,
                true,
            )
            .unwrap()
            .unwrap();

        let resolved = store
            .resolve_attachment_path(&set.id, &set.attachments[0].id)
            .unwrap();
        assert!(resolved.ends_with("attachments/report.md"));
        assert!(
            store
                .resolve_attachment_path(&set.id, "missing")
                .unwrap_err()
                .to_string()
                .starts_with(ATTACHMENT_NOT_FOUND)
        );
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
    fn intermediate_diff_requires_a_successful_tool_terminal_outcome() {
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
                    { "type": "diff", "path": "progress.txt", "oldText": "before", "newText": "after" }
                ])),
            )
            .unwrap();

        assert!(
            store
                .finalize_turn_branch("turn", "prompt", "root", "1Z", "3Z", &HashMap::new(),)
                .unwrap()
                .is_none()
        );

        let completed = HashMap::from([(
            "edit".to_string(),
            TurnFileToolTerminalOutcome::from_status(Some("completed")).unwrap(),
        )]);
        let set = store
            .finalize_turn_branch("turn", "prompt", "root", "1Z", "3Z", &completed)
            .unwrap()
            .unwrap();
        assert_eq!(set.summary.file_count, 1);
        assert_eq!(set.summary.added_lines, 1);
    }

    #[test]
    fn failed_tool_terminal_outcome_excludes_repeated_diff_evidence() {
        let (_dir, store) = store();
        let diff = serde_json::json!([
            { "type": "diff", "path": "progress.txt", "oldText": "before", "newText": "before\nafter" }
        ]);
        store
            .capture_event_diffs(
                "turn",
                "prompt",
                "root",
                "edit",
                2,
                "2Z",
                &raw(diff.clone()),
            )
            .unwrap();
        store
            .capture_event_diffs("turn", "prompt", "root", "edit", 3, "3Z", &raw(diff))
            .unwrap();
        let failed = HashMap::from([(
            "edit".to_string(),
            TurnFileToolTerminalOutcome::from_status(Some("failed")).unwrap(),
        )]);

        assert!(
            store
                .finalize_turn_branch("turn", "prompt", "root", "1Z", "4Z", &failed)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn tool_terminal_status_mapping_is_explicit_and_permission_independent() {
        assert_eq!(
            TurnFileToolTerminalOutcome::from_status(Some("succeeded")),
            Some(TurnFileToolTerminalOutcome::Succeeded)
        );
        assert_eq!(
            TurnFileToolTerminalOutcome::from_status(Some("error")),
            Some(TurnFileToolTerminalOutcome::Failed)
        );
        assert_eq!(
            TurnFileToolTerminalOutcome::from_status(Some("canceled")),
            Some(TurnFileToolTerminalOutcome::Cancelled)
        );
        assert_eq!(
            TurnFileToolTerminalOutcome::from_status(Some("in_progress")),
            None
        );
        assert_eq!(TurnFileToolTerminalOutcome::from_status(None), None);
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "4Z",
                &succeeded_tools(&["edit"]),
            )
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "4Z",
                &succeeded_tools(&["edit"]),
            )
            .unwrap()
            .unwrap();
        legacy.schema_version = 1;
        write_json(&store.change_set_path(&legacy.id), &legacy).unwrap();
        write_tool_terminal(&store, "edit", "completed");
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "3Z",
                &succeeded_tools(&["edit"]),
            )
            .unwrap()
            .unwrap();
        schema_two.schema_version = 2;
        schema_two.changes[0].before_version = Some(before_version);
        schema_two.changes[0].after_version = Some(after_version);
        write_json(&store.change_set_path(&schema_two.id), &schema_two).unwrap();
        write_tool_terminal(&store, "edit", "completed");

        let rebuilt = store.load_change_set(&schema_two.id).unwrap();
        assert_eq!(rebuilt.schema_version, TURN_FILE_CHANGE_SET_SCHEMA_VERSION);
        let comparison = store
            .comparison(&rebuilt.id, &rebuilt.changes[0].id)
            .unwrap();
        assert_eq!(comparison.before.unwrap().content, "静夜思");
        assert_eq!(comparison.after.unwrap().content, "春望");
    }

    #[test]
    fn loading_legacy_change_set_removes_diff_without_successful_tool_terminal() {
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
                    { "type": "diff", "path": "progress.txt", "oldText": "before", "newText": "after" }
                ])),
            )
            .unwrap();
        let mut legacy = store
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "3Z",
                &succeeded_tools(&["edit"]),
            )
            .unwrap()
            .unwrap();
        legacy.schema_version = 3;
        write_json(&store.change_set_path(&legacy.id), &legacy).unwrap();
        write_tool_terminal(&store, "edit", "failed");

        let rebuilt = store.load_change_set(&legacy.id).unwrap();
        assert_eq!(rebuilt.schema_version, TURN_FILE_CHANGE_SET_SCHEMA_VERSION);
        assert_eq!(rebuilt.summary, TurnFileChangeSummary::default());
        assert!(rebuilt.changes.is_empty());
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "4Z",
                &succeeded_tools(&["one", "two"]),
            )
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
                .finalize_turn_branch(
                    "turn",
                    "prompt",
                    "root",
                    "1Z",
                    "4Z",
                    &succeeded_tools(&["one", "two"]),
                )
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "3Z",
                &succeeded_tools(&["write"]),
            )
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
            .finalize_turn_branch(
                "turn",
                "prompt",
                "root",
                "1Z",
                "3Z",
                &succeeded_tools(&["write"]),
            )
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
