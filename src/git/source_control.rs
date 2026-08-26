use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::process::{ManagedProcessGroup, PROCESS_GROUP_TERMINATION_GRACE, background_command};

const HISTORY_PAGE_DEFAULT: usize = 300;
const HISTORY_PAGE_MAX: usize = 1_000;
const COMMIT_REVIEW_SELECTION_MAX: usize = 32;
const MACHINE_COMMAND_CAPTURE_LIMIT: usize = 4 * 1024 * 1024;
const OPERATION_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const OPERATION_POLL_INTERVAL: Duration = Duration::from_millis(40);
const UNBORN_HEAD_SENTINEL: &str = "(initial)";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct GitServiceError {
    pub code: &'static str,
    pub params: serde_json::Value,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitLockOwner {
    User,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLockSnapshot {
    pub locked: bool,
    pub owner: Option<GitLockOwner>,
    pub operation: Option<String>,
}

impl GitLockSnapshot {
    fn unlocked() -> Self {
        Self {
            locked: false,
            owner: None,
            operation: None,
        }
    }
}

#[derive(Debug, Clone)]
struct GitWriterState {
    owner: GitLockOwner,
    operation: String,
}

#[derive(Debug, Default)]
struct GitLockCell {
    gate: Mutex<()>,
    state: Mutex<Option<GitWriterState>>,
}

#[derive(Debug, Default)]
struct GitCoordinationRegistry {
    cells: Mutex<HashMap<String, Arc<GitLockCell>>>,
}

static GIT_COORDINATION_REGISTRY: OnceLock<GitCoordinationRegistry> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
pub struct GitCoordinationService;

impl GitCoordinationService {
    pub fn repository_lock(&self, common_dir: &Utf8Path) -> GitLockSnapshot {
        self.lock_snapshot("repository", common_dir)
    }

    pub fn workspace_lock(&self, workspace_path: &Utf8Path) -> GitLockSnapshot {
        self.lock_snapshot("workspace", workspace_path)
    }

    pub fn with_runtime_write<T>(
        &self,
        common_dir: &Utf8Path,
        workspace_path: Option<&Utf8Path>,
        operation: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_write_locks(
            common_dir,
            workspace_path,
            GitLockOwner::Runtime,
            operation,
            true,
            action,
        )
    }

    pub fn try_with_user_write<T>(
        &self,
        common_dir: &Utf8Path,
        workspace_path: Option<&Utf8Path>,
        operation: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_write_locks(
            common_dir,
            workspace_path,
            GitLockOwner::User,
            operation,
            false,
            action,
        )
    }

    pub fn try_with_user_workspace_write<T>(
        &self,
        workspace_path: &Utf8Path,
        operation: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_one_lock(
            "workspace",
            workspace_path,
            GitLockOwner::User,
            operation,
            false,
            "git.workspace-locked",
            action,
        )
    }

    pub fn try_with_user_repository_write<T>(
        &self,
        common_dir: &Utf8Path,
        operation: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_one_lock(
            "repository",
            common_dir,
            GitLockOwner::User,
            operation,
            false,
            "git.repository-locked",
            action,
        )
    }

    fn with_one_lock<T>(
        &self,
        kind: &str,
        path: &Utf8Path,
        owner: GitLockOwner,
        operation: &str,
        wait: bool,
        locked_code: &'static str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let cell = self.cell(kind, path)?;
        let guard = acquire_gate(&cell, wait).map_err(|_| {
            GitServiceError::new(
                locked_code,
                serde_json::json!({ "lock": lock_cell_snapshot(&cell) }),
            )
        })?;
        set_lock_cell_state(
            &cell,
            Some(GitWriterState {
                owner,
                operation: operation.to_string(),
            }),
        )?;
        let result = action();
        let _ = set_lock_cell_state(&cell, None);
        drop(guard);
        result
    }

    fn with_write_locks<T>(
        &self,
        common_dir: &Utf8Path,
        workspace_path: Option<&Utf8Path>,
        owner: GitLockOwner,
        operation: &str,
        wait: bool,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let repository = self.cell("repository", common_dir)?;
        let workspace = workspace_path
            .map(|path| self.cell("workspace", path))
            .transpose()?;
        let repository_guard = acquire_gate(&repository, wait).map_err(|_| {
            GitServiceError::new(
                "git.repository-locked",
                serde_json::json!({ "lock": lock_cell_snapshot(&repository) }),
            )
        })?;
        let workspace_guard = workspace
            .as_ref()
            .map(|cell| {
                acquire_gate(cell, wait).map_err(|_| {
                    GitServiceError::new(
                        "git.workspace-locked",
                        serde_json::json!({ "lock": lock_cell_snapshot(cell) }),
                    )
                })
            })
            .transpose()?;
        let writer = GitWriterState {
            owner,
            operation: operation.to_string(),
        };
        set_lock_cell_state(&repository, Some(writer.clone()))?;
        if let Some(workspace) = workspace.as_ref() {
            set_lock_cell_state(workspace, Some(writer))?;
        }
        let result = action();
        if let Some(workspace) = workspace.as_ref() {
            let _ = set_lock_cell_state(workspace, None);
        }
        let _ = set_lock_cell_state(&repository, None);
        drop(workspace_guard);
        drop(repository_guard);
        result
    }

    fn lock_snapshot(&self, kind: &str, path: &Utf8Path) -> GitLockSnapshot {
        self.cell(kind, path)
            .map(|cell| lock_cell_snapshot(&cell))
            .unwrap_or_else(|_| GitLockSnapshot::unlocked())
    }

    fn cell(&self, kind: &str, path: &Utf8Path) -> Result<Arc<GitLockCell>> {
        let registry = GIT_COORDINATION_REGISTRY.get_or_init(GitCoordinationRegistry::default);
        let key = format!("{kind}:{}", normalized_lock_path(path)?);
        let mut cells = registry.cells.lock().map_err(|_| {
            GitServiceError::new("git.lock-registry-poisoned", serde_json::json!({}))
        })?;
        Ok(cells.entry(key).or_default().clone())
    }
}

fn acquire_gate(cell: &GitLockCell, wait: bool) -> std::result::Result<MutexGuard<'_, ()>, ()> {
    if wait {
        cell.gate.lock().map_err(|_| ())
    } else {
        match cell.gate.try_lock() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::WouldBlock | TryLockError::Poisoned(_)) => Err(()),
        }
    }
}

fn set_lock_cell_state(cell: &GitLockCell, state: Option<GitWriterState>) -> Result<()> {
    *cell
        .state
        .lock()
        .map_err(|_| GitServiceError::new("git.lock-state-poisoned", serde_json::json!({})))? =
        state;
    Ok(())
}

fn lock_cell_snapshot(cell: &GitLockCell) -> GitLockSnapshot {
    let Ok(state) = cell.state.lock() else {
        return GitLockSnapshot::unlocked();
    };
    state
        .as_ref()
        .map_or_else(GitLockSnapshot::unlocked, |state| GitLockSnapshot {
            locked: true,
            owner: Some(state.owner),
            operation: Some(state.operation.clone()),
        })
}

fn normalized_lock_path(path: &Utf8Path) -> Result<String> {
    let mut value = canonical_utf8_path(path)?.as_str().replace('\\', "/");
    #[cfg(windows)]
    value.make_ascii_lowercase();
    Ok(value)
}

impl GitServiceError {
    pub fn new(code: &'static str, params: serde_json::Value) -> Self {
        Self {
            code,
            params,
            diagnostic: None,
        }
    }

    fn command(code: &'static str, output: &MachineCommandOutput) -> Self {
        let mut params = serde_json::json!({ "exitCode": output.exit_code });
        if let Some(reason) = output.failure_reason() {
            params["reason"] = serde_json::Value::String(reason);
        }
        Self {
            code,
            params,
            diagnostic: Some(output.diagnostic()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineCommandOutput {
    success: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl MachineCommandOutput {
    fn diagnostic(&self) -> String {
        let stdout = String::from_utf8_lossy(&self.stdout);
        let stderr = String::from_utf8_lossy(&self.stderr);
        match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
            (true, true) => "no command output".to_string(),
            (false, true) => format!("stdout: {}", stdout.trim()),
            (true, false) => format!("stderr: {}", stderr.trim()),
            (false, false) => format!("stdout: {}; stderr: {}", stdout.trim(), stderr.trim()),
        }
    }

    fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }

    fn failure_reason(&self) -> Option<String> {
        const MAX_REASON_CHARS: usize = 2_000;
        let stderr = String::from_utf8_lossy(&self.stderr);
        let stdout = String::from_utf8_lossy(&self.stdout);
        let normalized = [stderr.as_ref(), stdout.as_ref()]
            .into_iter()
            .flat_map(str::lines)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if normalized.is_empty() {
            return None;
        }
        let redacted = redact_url_credentials(&normalized);
        let mut chars = redacted.chars();
        let reason = chars.by_ref().take(MAX_REASON_CHARS).collect::<String>();
        Some(if chars.next().is_some() {
            format!("{reason}…")
        } else {
            reason
        })
    }
}

fn redact_url_credentials(input: &str) -> String {
    let mut output = input.to_string();
    let mut search_start = 0;
    while let Some(relative_scheme_end) = output[search_start..].find("://") {
        let authority_start = search_start + relative_scheme_end + 3;
        let authority_end = output[authority_start..]
            .find(|character: char| {
                character == '/'
                    || character == '\\'
                    || character.is_whitespace()
                    || character == '\''
                    || character == '"'
            })
            .map_or(output.len(), |offset| authority_start + offset);
        if let Some(relative_at) = output[authority_start..authority_end].rfind('@') {
            let credential_end = authority_start + relative_at;
            output.replace_range(authority_start..credential_end, "***");
            search_start = authority_start + 4;
        } else {
            search_start = authority_end;
        }
        if search_start >= output.len() {
            break;
        }
    }
    output
}

impl From<Output> for MachineCommandOutput {
    fn from(output: Output) -> Self {
        Self {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct GitMachineRunner;

impl GitMachineRunner {
    fn run(&self, cwd: &Utf8Path, args: &[&str]) -> Result<MachineCommandOutput> {
        background_command("git")
            .arg("-C")
            .arg(cwd.as_str())
            .args(args)
            .env("LC_ALL", "C")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .map(MachineCommandOutput::from)
            .with_context(|| format!("failed to execute Git in `{cwd}`"))
    }

    fn require(
        &self,
        cwd: &Utf8Path,
        args: &[&str],
        code: &'static str,
    ) -> Result<MachineCommandOutput> {
        let output = self.run(cwd, args)?;
        if output.success {
            Ok(output)
        } else {
            Err(GitServiceError::command(code, &output).into())
        }
    }

    fn run_with_input(
        &self,
        cwd: &Utf8Path,
        args: &[&str],
        input: &[u8],
    ) -> Result<MachineCommandOutput> {
        let mut command = background_command("git");
        command
            .arg("-C")
            .arg(cwd.as_str())
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut process = ManagedProcessGroup::spawn(&mut command)
            .with_context(|| format!("failed to execute Git in `{cwd}`"))?;
        let mut stdin = process
            .take_stdin()
            .ok_or_else(|| GitServiceError::new("git.stdin-unavailable", serde_json::json!({})))?;
        stdin.write_all(input)?;
        drop(stdin);
        let stdout = process
            .take_stdout()
            .ok_or_else(|| GitServiceError::new("git.stdout-unavailable", serde_json::json!({})))?;
        let stderr = process
            .take_stderr()
            .ok_or_else(|| GitServiceError::new("git.stderr-unavailable", serde_json::json!({})))?;
        let stdout_reader = std::thread::spawn(move || read_command_stream(stdout));
        let stderr_reader = std::thread::spawn(move || read_command_stream(stderr));
        let status = process.wait()?;
        let stdout = stdout_reader.join().map_err(|_| {
            GitServiceError::new("git.output-reader-failed", serde_json::json!({}))
        })??;
        let stderr = stderr_reader.join().map_err(|_| {
            GitServiceError::new("git.output-reader-failed", serde_json::json!({}))
        })??;
        Ok(MachineCommandOutput {
            success: status.success(),
            exit_code: status.code(),
            stdout,
            stderr,
        })
    }

    fn require_with_input(
        &self,
        cwd: &Utf8Path,
        args: &[&str],
        input: &[u8],
        code: &'static str,
    ) -> Result<MachineCommandOutput> {
        let output = self.run_with_input(cwd, args, input)?;
        if output.success {
            Ok(output)
        } else {
            Err(git_command_error(code, &output).into())
        }
    }
}

fn read_command_stream(mut stream: impl Read) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = MACHINE_COMMAND_CAPTURE_LIMIT.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(captured)
}

fn git_command_error(fallback: &'static str, output: &MachineCommandOutput) -> GitServiceError {
    let command_output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    )
    .to_ascii_lowercase();
    let code = if command_output.contains("non-fast-forward")
        || command_output.contains("fetch first")
    {
        "git.non-fast-forward"
    } else if command_output.contains("authentication failed")
        || command_output.contains("could not read username")
    {
        "git.authentication-failed"
    } else if command_output.contains("permission denied (publickey)")
        || command_output.contains("permission to") && command_output.contains("denied")
    {
        "git.permission-denied"
    } else if command_output.contains("repository not found")
        || command_output.contains("does not appear to be a git repository")
    {
        "git.remote-repository-not-found"
    } else if command_output.contains("could not resolve host") {
        "git.remote-host-unreachable"
    } else if command_output.contains("failed to connect")
        || command_output.contains("connection timed out")
        || command_output.contains("connection refused")
    {
        "git.remote-unreachable"
    } else if command_output.contains("remote rejected") {
        "git.remote-rejected"
    } else if command_output.contains("is already checked out at")
        || command_output.contains("used by worktree")
    {
        "git.branch-in-use-by-worktree"
    } else if command_output.contains("local changes") && command_output.contains("overwritten") {
        "git.workspace-dirty"
    } else if command_output.contains("hook") && fallback == "git.commit-failed" {
        "git.hook-failed"
    } else if command_output.contains("conflict") {
        "git.merge-conflict"
    } else {
        fallback
    };
    GitServiceError::command(code, output)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositoryIdentity {
    pub repo_root: Utf8PathBuf,
    pub common_dir: Utf8PathBuf,
    pub workspace_path: Utf8PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitMetadataWatchTarget {
    pub path: Utf8PathBuf,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositorySnapshot {
    pub project_id: String,
    pub repo_root: Utf8PathBuf,
    pub common_dir: Utf8PathBuf,
    pub workspace_path: Utf8PathBuf,
    pub head_oid: Option<String>,
    pub current_branch: Option<String>,
    pub detached: bool,
    pub unborn: bool,
    pub upstream: Option<GitUpstream>,
    pub remotes: Vec<GitRemote>,
    pub lock: GitLockSnapshot,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitUpstream {
    pub name: String,
    pub ahead: u64,
    pub behind: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    pub name: String,
    pub fetch_urls: Vec<String>,
    pub push_urls: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitFileChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: GitFileChangeKind,
    pub index_status: Option<String>,
    pub worktree_status: Option<String>,
    pub binary: bool,
    pub submodule: bool,
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchStatus {
    pub oid: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInProgressOperation {
    pub kind: GitInProgressOperationKind,
    pub current_oid: Option<String>,
    pub current_subject: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitInProgressOperationKind {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorkspaceStatus {
    pub snapshot_revision: String,
    pub branch: GitBranchStatus,
    pub conflicts: Vec<GitFileChange>,
    pub staged: Vec<GitFileChange>,
    pub unstaged: Vec<GitFileChange>,
    pub untracked: Vec<GitFileChange>,
    pub operation_in_progress: Option<GitInProgressOperation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitRefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRef {
    pub full_name: String,
    pub short_name: String,
    pub kind: GitRefKind,
    pub target_oid: String,
    pub peeled_oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub checked_out_worktree_paths: Vec<Utf8PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitWorktreeOwnership {
    User,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktree {
    pub path: Utf8PathBuf,
    pub head_oid: String,
    pub branch: Option<String>,
    pub main: bool,
    pub detached: bool,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub prunable: bool,
    pub ownership: GitWorktreeOwnership,
    pub runtime_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSignature {
    pub name: String,
    pub email: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub ref_name: String,
    pub oid: String,
    pub base_oid: String,
    pub message: String,
    pub author: GitSignature,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRefLabel {
    pub full_name: String,
    pub short_name: String,
    pub kind: GitRefKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub oid: String,
    pub parent_oids: Vec<String>,
    pub subject: String,
    pub body: String,
    pub author: GitSignature,
    pub committer: GitSignature,
    pub refs: Vec<GitRefLabel>,
    pub source_ref: Option<String>,
    pub runtime_checkpoint: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub revision: Option<String>,
    pub ref_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryPage {
    pub commits: Vec<GitCommit>,
    pub next_cursor: Option<String>,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: GitFileChangeKind,
    pub binary: bool,
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDetail {
    pub commit: GitCommit,
    pub files: Vec<GitCommitFileChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReviewQuery {
    pub selected_oids: Vec<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReviewFile {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: GitFileChangeKind,
    pub binary: bool,
    pub before_oid: Option<String>,
    pub before_path: Option<String>,
    pub after_oid: String,
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReviewTotals {
    pub commit_count: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReview {
    pub selected_oids: Vec<String>,
    pub revision: String,
    pub files: Vec<GitCommitReviewFile>,
    pub totals: GitCommitReviewTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReachabilityQuery {
    pub oid: String,
    pub target_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitCommitTargetPath {
    Tip,
    Direct,
    Merged,
    NotContained,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitReachability {
    pub oid: String,
    pub containing_refs: Vec<GitRefLabel>,
    pub target_ref: String,
    pub target_oid: String,
    pub target_path: GitCommitTargetPath,
    pub first_merge_oid: Option<String>,
    pub parent_oids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSourceControlSnapshot {
    pub repository: GitRepositorySnapshot,
    pub status: GitWorkspaceStatus,
    pub refs: Vec<GitRef>,
    pub worktrees: Vec<GitWorktree>,
    pub stashes: Vec<GitStashEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitTagStyle {
    Annotated,
    Lightweight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum GitMutation {
    StagePaths {
        paths: Vec<String>,
    },
    StageAll,
    UnstagePaths {
        paths: Vec<String>,
    },
    UnstageAll,
    Commit {
        subject: String,
        body: Option<String>,
    },
    BranchCreate {
        name: String,
        start_point: Option<String>,
        checkout: bool,
    },
    BranchSwitch {
        name: String,
    },
    BranchRename {
        old_name: Option<String>,
        new_name: String,
    },
    BranchDeleteSafe {
        name: String,
    },
    TagCreate {
        name: String,
        target: Option<String>,
        style: GitTagStyle,
        message: Option<String>,
    },
    TagDeleteLocal {
        name: String,
    },
    WorktreeCreate {
        path: Utf8PathBuf,
        source_ref: String,
        new_branch: Option<String>,
    },
    WorktreeRemove {
        path: Utf8PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitMutationRequest {
    pub expected_revision: Option<String>,
    #[serde(flatten)]
    pub mutation: GitMutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "kebab-case")]
pub enum GitMutationResult {
    Workspace {
        status: GitWorkspaceStatus,
        #[serde(rename = "repositoryRevision")]
        repository_revision: String,
    },
    Repository,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitPullStrategy {
    FastForwardOnly,
    Merge,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitOperationKind {
    Fetch,
    Pull,
    Push,
    PushTag,
    StashCreate,
    StashApply,
    MergeContinue,
    MergeAbort,
    RebaseContinue,
    RebaseSkip,
    RebaseAbort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum GitOperationInput {
    Fetch {
        remote: Option<String>,
        prune: bool,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
        strategy: GitPullStrategy,
    },
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
    },
    PushTag {
        remote: String,
        tag: String,
    },
    StashCreate {
        message: Option<String>,
        include_untracked: bool,
    },
    StashApply {
        stash_ref: String,
        restore_index: bool,
    },
    MergeContinue,
    MergeAbort,
    RebaseContinue,
    RebaseSkip,
    RebaseAbort,
}

impl GitOperationInput {
    fn kind(&self) -> GitOperationKind {
        match self {
            Self::Fetch { .. } => GitOperationKind::Fetch,
            Self::Pull { .. } => GitOperationKind::Pull,
            Self::Push { .. } => GitOperationKind::Push,
            Self::PushTag { .. } => GitOperationKind::PushTag,
            Self::StashCreate { .. } => GitOperationKind::StashCreate,
            Self::StashApply { .. } => GitOperationKind::StashApply,
            Self::MergeContinue => GitOperationKind::MergeContinue,
            Self::MergeAbort => GitOperationKind::MergeAbort,
            Self::RebaseContinue => GitOperationKind::RebaseContinue,
            Self::RebaseSkip => GitOperationKind::RebaseSkip,
            Self::RebaseAbort => GitOperationKind::RebaseAbort,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOperationRequest {
    pub expected_revision: Option<String>,
    #[serde(flatten)]
    pub operation: GitOperationInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitOperationStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOperationError {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOperation {
    pub operation_id: String,
    pub kind: GitOperationKind,
    pub repository_common_dir: Utf8PathBuf,
    pub workspace_path: Option<Utf8PathBuf>,
    pub status: GitOperationStatus,
    pub cancelable: bool,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<GitOperationError>,
}

struct GitOperationCell {
    state: Mutex<GitOperation>,
    process: Mutex<Option<ManagedProcessGroup>>,
    cancel_requested: AtomicBool,
    update_sink: Option<GitOperationUpdateSink>,
}

#[derive(Default)]
struct GitOperationRegistry {
    operations: Mutex<HashMap<String, Arc<GitOperationCell>>>,
}

static GIT_OPERATION_REGISTRY: OnceLock<GitOperationRegistry> = OnceLock::new();

pub type GitOperationUpdateSink = Arc<dyn Fn(GitOperation) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitWorkspaceDiffArea {
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum GitComparisonSource {
    Workspace {
        workspace_path: Option<Utf8PathBuf>,
        path: String,
        area: GitWorkspaceDiffArea,
    },
    Commit {
        workspace_path: Option<Utf8PathBuf>,
        path: String,
        before_oid: Option<String>,
        before_path: Option<String>,
        after_oid: String,
    },
    #[serde(rename = "github-pr")]
    GitHubPr {
        workspace_path: Option<Utf8PathBuf>,
        host: String,
        repository: String,
        pr_number: u64,
        base_oid: String,
        head_oid: String,
        path: String,
        before_path: Option<String>,
    },
}

impl GitComparisonSource {
    pub fn workspace_path(&self) -> Option<&str> {
        match self {
            Self::Workspace { workspace_path, .. }
            | Self::Commit { workspace_path, .. }
            | Self::GitHubPr { workspace_path, .. } => {
                workspace_path.as_deref().map(Utf8Path::as_str)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTextVersion {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffStats {
    pub added_lines: u64,
    pub deleted_lines: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBaselineDiffStats {
    pub added_lines: u64,
    pub deleted_lines: u64,
    pub changed_files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileComparison {
    pub path: String,
    pub stats: GitDiffStats,
    pub before: Option<GitTextVersion>,
    pub after: Option<GitTextVersion>,
    pub limitation_code: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GitSourceControlService {
    runner: GitMachineRunner,
}

impl GitSourceControlService {
    pub fn resolve_scoped_workspace(
        &self,
        project_root: &Utf8Path,
        requested_workspace: Option<&Utf8Path>,
    ) -> Result<GitRepositoryIdentity> {
        let project = self.repository_identity(project_root)?;
        let workspace = self.repository_identity(requested_workspace.unwrap_or(project_root))?;
        let project_common_dir = canonical_utf8_path(&project.common_dir)?;
        let workspace_common_dir = canonical_utf8_path(&workspace.common_dir)?;
        if normalized_lock_path(&project_common_dir)?
            != normalized_lock_path(&workspace_common_dir)?
        {
            return Err(GitServiceError::new(
                "git.workspace-outside-project",
                serde_json::json!({ "workspacePath": workspace.workspace_path }),
            )
            .into());
        }
        Ok(workspace)
    }

    pub fn repository_identity(&self, cwd: &Utf8Path) -> Result<GitRepositoryIdentity> {
        let repo_root = self
            .runner
            .require(
                cwd,
                &["rev-parse", "--show-toplevel"],
                "git.repository-not-found",
            )?
            .stdout_text();
        let common_dir = self
            .runner
            .require(
                cwd,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                "git.repository-not-found",
            )?
            .stdout_text();
        let workspace_path = canonical_utf8_path(Utf8Path::new(&repo_root))?;
        Ok(GitRepositoryIdentity {
            repo_root: Utf8PathBuf::from(repo_root),
            common_dir: Utf8PathBuf::from(common_dir),
            workspace_path,
        })
    }

    pub fn metadata_watch_targets(&self, cwd: &Utf8Path) -> Result<Vec<GitMetadataWatchTarget>> {
        let mut targets = BTreeMap::<String, GitMetadataWatchTarget>::new();
        for (marker, wants_recursive) in [
            ("HEAD", false),
            ("index", false),
            ("packed-refs", false),
            ("refs", true),
            ("MERGE_HEAD", false),
            ("REBASE_HEAD", false),
            ("rebase-merge", true),
            ("rebase-apply", true),
        ] {
            let output = self.runner.require(
                cwd,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
                "git.repository-not-found",
            )?;
            let marker_path = Utf8PathBuf::from(output.stdout_text());
            let recursive = wants_recursive && marker_path.is_dir();
            let watch_path = if recursive {
                marker_path
            } else {
                marker_path.parent().unwrap_or(cwd).to_path_buf()
            };
            let canonical = canonical_utf8_path(&watch_path)?;
            let key = normalized_lock_path(&canonical)?;
            targets
                .entry(key)
                .and_modify(|target| target.recursive |= recursive)
                .or_insert(GitMetadataWatchTarget {
                    path: canonical,
                    recursive,
                });
        }
        Ok(targets.into_values().collect())
    }

    pub fn snapshot(&self, project_id: &str, cwd: &Utf8Path) -> Result<GitSourceControlSnapshot> {
        let identity = self.repository_identity(cwd)?;
        let mut status = self.status(cwd)?;
        let refs = self.refs(cwd)?;
        let worktrees = self.worktrees(cwd)?;
        let stashes = self.stashes(cwd)?;
        let remotes = self.remotes(cwd)?;
        let revision = workspace_snapshot_revision(&status, &refs);
        status.snapshot_revision.clone_from(&revision);
        let current_branch = status
            .branch
            .head
            .clone()
            .filter(|head| head != "(detached)");
        let head_oid = status.branch.oid.clone();
        let lock = combined_lock_snapshot(&identity);
        let repository = GitRepositorySnapshot {
            project_id: project_id.to_string(),
            repo_root: identity.repo_root,
            common_dir: identity.common_dir,
            workspace_path: identity.workspace_path,
            detached: status.branch.head.as_deref() == Some("(detached)"),
            unborn: head_oid.is_none(),
            head_oid,
            current_branch,
            upstream: status.branch.upstream.as_ref().map(|name| GitUpstream {
                name: name.clone(),
                ahead: status.branch.ahead,
                behind: status.branch.behind,
            }),
            remotes,
            lock,
            revision,
        };
        let mut refs = refs;
        let worktree_paths = worktrees
            .iter()
            .filter_map(|worktree| {
                worktree
                    .branch
                    .as_ref()
                    .map(|branch| (branch, &worktree.path))
            })
            .collect::<HashMap<_, _>>();
        for git_ref in &mut refs {
            if let Some(path) = worktree_paths.get(&git_ref.full_name) {
                git_ref.checked_out_worktree_paths.push((*path).clone());
            }
        }
        Ok(GitSourceControlSnapshot {
            repository,
            status,
            refs,
            worktrees,
            stashes,
        })
    }

    pub fn status(&self, cwd: &Utf8Path) -> Result<GitWorkspaceStatus> {
        let mut status = self.status_without_stats(cwd)?;
        let (staged_stats, unstaged_stats) = std::thread::scope(|scope| {
            let staged = scope.spawn(|| self.workspace_numstat(cwd, true));
            let unstaged = scope.spawn(|| self.workspace_numstat(cwd, false));
            let staged = staged.join().map_err(|_| {
                GitServiceError::new("git.status-diff-query-failed", serde_json::json!({}))
            })??;
            let unstaged = unstaged.join().map_err(|_| {
                GitServiceError::new("git.status-diff-query-failed", serde_json::json!({}))
            })??;
            Ok::<_, anyhow::Error>((staged, unstaged))
        })?;
        apply_workspace_stats(&mut status.staged, &staged_stats);
        apply_workspace_stats(&mut status.unstaged, &unstaged_stats);
        apply_workspace_stats(&mut status.conflicts, &unstaged_stats);
        apply_workspace_stats(&mut status.conflicts, &staged_stats);
        Ok(status)
    }

    pub fn baseline_diff_stats(
        &self,
        cwd: &Utf8Path,
        baseline: &str,
    ) -> Result<GitBaselineDiffStats> {
        ensure!(!baseline.trim().is_empty(), "Git baseline cannot be empty");
        self.runner.require(
            cwd,
            &["cat-file", "-e", &format!("{baseline}^{{commit}}")],
            "git.baseline-not-found",
        )?;
        let output = self.runner.require(
            cwd,
            &[
                "diff",
                "--numstat",
                "-z",
                "--no-ext-diff",
                "--no-textconv",
                "-M",
                "-C",
                baseline,
                "--",
            ],
            "git.baseline-diff-query-failed",
        )?;
        let mut files = parse_numstat(&output.stdout, "git.baseline-diff-parse-failed")?;
        let untracked = self.runner.require(
            cwd,
            &["ls-files", "--others", "--exclude-standard", "-z", "--"],
            "git.untracked-files-query-failed",
        )?;
        for field in nul_fields(&untracked.stdout) {
            let relative = text(&field);
            if relative.is_empty() || files.contains_key(&relative) {
                continue;
            }
            let path = Utf8Path::new(&relative);
            ensure!(
                path.is_relative()
                    && !path
                        .components()
                        .any(|component| matches!(component, camino::Utf8Component::ParentDir)),
                "Git returned an invalid untracked path"
            );
            let stats = untracked_file_stats(&cwd.join(path))?;
            files.insert(relative, stats);
        }
        Ok(GitBaselineDiffStats {
            added_lines: files
                .values()
                .filter_map(|stats| stats.added_lines)
                .fold(0, u64::saturating_add),
            deleted_lines: files
                .values()
                .filter_map(|stats| stats.deleted_lines)
                .fold(0, u64::saturating_add),
            changed_files: files.len() as u64,
        })
    }

    fn status_without_stats(&self, cwd: &Utf8Path) -> Result<GitWorkspaceStatus> {
        let output = self.runner.require(
            cwd,
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ],
            "git.status-failed",
        )?;
        let mut status = parse_porcelain_v2(&output.stdout)?;
        status.operation_in_progress = self.in_progress_operation(cwd)?;
        Ok(status)
    }

    fn workspace_numstat(
        &self,
        cwd: &Utf8Path,
        staged: bool,
    ) -> Result<HashMap<String, CommitFileStats>> {
        let mut args = vec!["diff"];
        if staged {
            args.push("--cached");
        }
        args.extend([
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "-M",
            "-C",
            "--",
        ]);
        let output = self
            .runner
            .require(cwd, &args, "git.status-diff-query-failed")?;
        parse_numstat(&output.stdout, "git.status-diff-parse-failed")
    }

    pub fn refs(&self, cwd: &Utf8Path) -> Result<Vec<GitRef>> {
        let output = self.runner.require(
            cwd,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(refname:short)%00%(objectname)%00%(*objectname)%00%(upstream:short)%00",
                "refs/heads",
                "refs/remotes",
                "refs/tags",
            ],
            "git.refs-query-failed",
        )?;
        parse_refs(&output.stdout)
    }

    pub fn worktrees(&self, cwd: &Utf8Path) -> Result<Vec<GitWorktree>> {
        let output = self.runner.require(
            cwd,
            &["worktree", "list", "--porcelain", "-z"],
            "git.worktree-query-failed",
        )?;
        parse_worktrees(&output.stdout)
    }

    pub fn stashes(&self, cwd: &Utf8Path) -> Result<Vec<GitStashEntry>> {
        let output = self.runner.require(
            cwd,
            &[
                "stash",
                "list",
                "-z",
                "--format=%gd%x00%H%x00%P%x00%gs%x00%an%x00%ae%x00%aI",
            ],
            "git.stash-query-failed",
        )?;
        parse_stashes(&output.stdout)
    }

    pub fn history(&self, cwd: &Utf8Path, query: &GitHistoryQuery) -> Result<GitHistoryPage> {
        let refs = self.refs(cwd)?;
        let status = self.status_without_stats(cwd)?;
        let revision = repository_revision(&status.branch, &refs);
        if let Some(expected) = query.revision.as_deref()
            && expected != revision
        {
            return Err(GitServiceError::new(
                "git.ref-changed",
                serde_json::json!({ "expectedRevision": expected, "actualRevision": revision }),
            )
            .into());
        }
        if status.branch.oid.is_none() && refs.is_empty() {
            return Ok(GitHistoryPage {
                commits: Vec::new(),
                next_cursor: None,
                revision,
            });
        }
        let offset = query
            .cursor
            .as_deref()
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| {
                GitServiceError::new("git.invalid-history-cursor", serde_json::json!({}))
            })?;
        let limit = query
            .limit
            .unwrap_or(HISTORY_PAGE_DEFAULT)
            .clamp(1, HISTORY_PAGE_MAX);
        let max_count = (limit + 1).to_string();
        let skip = offset.to_string();
        let mut owned_args = vec![
            "log".to_string(),
            "--topo-order".to_string(),
            "--date-order".to_string(),
            "--parents".to_string(),
            "--source".to_string(),
            "-z".to_string(),
            format!("--max-count={max_count}"),
            format!("--skip={skip}"),
            "--format=%H%x00%P%x00%s%x00%b%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%S"
                .to_string(),
        ];
        if let Some(ref_name) = query.ref_name.as_deref() {
            validate_revision(ref_name)?;
            owned_args.push(ref_name.to_string());
        } else {
            owned_args.push("HEAD".to_string());
        }
        let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = self
            .runner
            .require(cwd, &args, "git.history-query-failed")?;
        let labels = refs_by_oid(&refs);
        let mut commits = parse_history(&output.stdout, &labels)?;
        let has_more = commits.len() > limit;
        commits.truncate(limit);
        Ok(GitHistoryPage {
            commits,
            next_cursor: has_more.then(|| (offset + limit).to_string()),
            revision,
        })
    }

    pub fn commit_detail(&self, cwd: &Utf8Path, oid: &str) -> Result<GitCommitDetail> {
        let oid = self.resolve_commit_oid(cwd, oid)?;
        let labels = refs_by_oid(&self.refs(cwd)?);
        let commit = self.commit_metadata(cwd, &oid, &labels)?;
        let files = self.commit_file_changes(
            cwd,
            commit.parent_oids.first().map(String::as_str),
            &commit.oid,
        )?;
        Ok(GitCommitDetail { commit, files })
    }

    fn commit_metadata(
        &self,
        cwd: &Utf8Path,
        oid: &str,
        labels: &HashMap<String, Vec<GitRefLabel>>,
    ) -> Result<GitCommit> {
        let mut commits = self.commit_metadata_batch(cwd, &[oid.to_string()], labels)?;
        Ok(commits.remove(0))
    }

    fn commit_metadata_batch(
        &self,
        cwd: &Utf8Path,
        oids: &[String],
        labels: &HashMap<String, Vec<GitRefLabel>>,
    ) -> Result<Vec<GitCommit>> {
        let mut owned_args = vec![
            "show".to_string(),
            "--no-patch".to_string(),
            "--date-order".to_string(),
            "--parents".to_string(),
            "--source".to_string(),
            "-z".to_string(),
            "--format=%H%x00%P%x00%s%x00%b%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%S"
                .to_string(),
        ];
        owned_args.extend(oids.iter().cloned());
        let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = self
            .runner
            .require(cwd, &args, "git.commit-detail-query-failed")?;
        let parsed = parse_history(&output.stdout, labels)?;
        let mut by_oid = parsed
            .into_iter()
            .map(|commit| (commit.oid.clone(), commit))
            .collect::<HashMap<_, _>>();
        let commits = oids
            .iter()
            .filter_map(|oid| by_oid.remove(oid))
            .collect::<Vec<_>>();
        if commits.len() != oids.len() {
            return Err(GitServiceError::new(
                "git.commit-detail-parse-failed",
                serde_json::json!({ "oids": oids }),
            )
            .into());
        }
        Ok(commits)
    }

    pub fn commit_review(
        &self,
        cwd: &Utf8Path,
        query: &GitCommitReviewQuery,
    ) -> Result<GitCommitReview> {
        if query.selected_oids.is_empty() {
            return Err(GitServiceError::new(
                "git.commit-selection-too-small",
                serde_json::json!({ "minimum": 1 }),
            )
            .into());
        }
        if query.selected_oids.len() > COMMIT_REVIEW_SELECTION_MAX {
            return Err(GitServiceError::new(
                "git.commit-selection-limit",
                serde_json::json!({ "maximum": COMMIT_REVIEW_SELECTION_MAX }),
            )
            .into());
        }

        let refs = self.refs(cwd)?;
        let status = self.status_without_stats(cwd)?;
        let revision = repository_revision(&status.branch, &refs);
        if let Some(expected) = query.revision.as_deref()
            && expected != revision
        {
            return Err(GitServiceError::new(
                "git.ref-changed",
                serde_json::json!({ "expectedRevision": expected, "actualRevision": revision }),
            )
            .into());
        }
        let mut selected_oids = Vec::with_capacity(query.selected_oids.len());
        for revision in &query.selected_oids {
            let oid = self.resolve_commit_oid(cwd, revision)?;
            if !selected_oids.contains(&oid) {
                selected_oids.push(oid);
            }
        }
        let labels = refs_by_oid(&refs);
        let metadata = self.commit_metadata_batch(cwd, &selected_oids, &labels)?;
        let worker_count = metadata.len().min(4);
        let chunk_size = metadata.len().div_ceil(worker_count);
        let indexed = metadata.into_iter().enumerate().collect::<Vec<_>>();
        let mut commits = std::thread::scope(|scope| {
            let handles = indexed
                .chunks(chunk_size)
                .map(|chunk| {
                    let service = self.clone();
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .map(|(index, commit)| {
                                let before_oid = commit.parent_oids.first().cloned();
                                let files = service.commit_file_changes(
                                    cwd,
                                    before_oid.as_deref(),
                                    &commit.oid,
                                )?;
                                Ok((
                                    *index,
                                    CommitReviewPatch {
                                        after_oid: commit.oid.clone(),
                                        before_oid,
                                        files,
                                    },
                                ))
                            })
                            .collect::<Result<Vec<_>>>()
                    })
                })
                .collect::<Vec<_>>();
            let mut entries = Vec::with_capacity(indexed.len());
            for handle in handles {
                entries.extend(handle.join().map_err(|_| {
                    GitServiceError::new("git.commit-review-worker-failed", serde_json::json!({}))
                })??);
            }
            Ok::<_, anyhow::Error>(entries)
        })?;
        commits.sort_by_key(|(index, _)| *index);
        let mut files =
            self.aggregate_commit_review_files(cwd, commits.into_iter().map(|(_, entry)| entry))?;
        self.populate_commit_review_stats(cwd, &mut files)?;
        let totals = GitCommitReviewTotals {
            commit_count: selected_oids.len(),
            file_count: files.len(),
        };

        Ok(GitCommitReview {
            selected_oids,
            revision,
            files,
            totals,
        })
    }

    pub fn commit_reachability(
        &self,
        cwd: &Utf8Path,
        query: &GitCommitReachabilityQuery,
    ) -> Result<GitCommitReachability> {
        let oid = self.resolve_commit_oid(cwd, &query.oid)?;
        let target_oid = self.resolve_commit_oid(cwd, &query.target_ref)?;
        let refs = self.refs(cwd)?;
        let contains_arg = format!("--contains={oid}");
        let containing_output = self.runner.require(
            cwd,
            &[
                "for-each-ref",
                contains_arg.as_str(),
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
                "refs/tags",
            ],
            "git.commit-reachability-query-failed",
        )?;
        let containing_names = containing_output
            .stdout_text()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<HashSet<_>>();
        let containing_refs = refs
            .iter()
            .filter(|git_ref| containing_names.contains(git_ref.full_name.as_str()))
            .map(|git_ref| GitRefLabel {
                full_name: git_ref.full_name.clone(),
                short_name: git_ref.short_name.clone(),
                kind: git_ref.kind,
            })
            .collect();
        let labels = refs_by_oid(&refs);
        let commit = self.commit_metadata(cwd, &oid, &labels)?;
        let (target_path, first_merge_oid) = if oid == target_oid {
            (GitCommitTargetPath::Tip, None)
        } else if !self.is_ancestor(
            cwd,
            &oid,
            &target_oid,
            "git.commit-reachability-query-failed",
        )? {
            (GitCommitTargetPath::NotContained, None)
        } else {
            let ancestry_range = format!("{oid}..{target_oid}");
            let first_merge_oid = self
                .optional_revision_lines(
                    cwd,
                    &[
                        "rev-list",
                        "--first-parent",
                        "--merges",
                        "--reverse",
                        "--ancestry-path",
                        ancestry_range.as_str(),
                    ],
                    "git.commit-reachability-query-failed",
                )?
                .into_iter()
                .next();
            if first_merge_oid.is_some() {
                (GitCommitTargetPath::Merged, first_merge_oid)
            } else {
                (GitCommitTargetPath::Direct, None)
            }
        };
        Ok(GitCommitReachability {
            oid,
            containing_refs,
            target_ref: query.target_ref.clone(),
            target_oid,
            target_path,
            first_merge_oid,
            parent_oids: commit.parent_oids,
        })
    }

    fn resolve_commit_oid(&self, cwd: &Utf8Path, revision: &str) -> Result<String> {
        validate_revision(revision)?;
        let commit_revision = format!("{revision}^{{commit}}");
        self.runner
            .require(
                cwd,
                &["rev-parse", "--verify", commit_revision.as_str()],
                "git.commit-not-found",
            )
            .map(|output| output.stdout_text())
    }

    fn is_ancestor(
        &self,
        cwd: &Utf8Path,
        ancestor: &str,
        descendant: &str,
        error_code: &'static str,
    ) -> Result<bool> {
        let output = self
            .runner
            .run(cwd, &["merge-base", "--is-ancestor", ancestor, descendant])?;
        match output.exit_code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(GitServiceError::command(error_code, &output).into()),
        }
    }

    fn aggregate_commit_review_files(
        &self,
        cwd: &Utf8Path,
        patches_newest_first: impl IntoIterator<Item = CommitReviewPatch>,
    ) -> Result<Vec<GitCommitReviewFile>> {
        let mut chains = Vec::<CommitReviewFileChain>::new();
        let mut chains_by_before_path = HashMap::<String, Vec<usize>>::new();
        let mut ancestry = HashMap::<(String, String), bool>::new();
        let mut patch_ids = HashMap::<CommitReviewFilePatchIdentity, String>::new();

        for patch in patches_newest_first {
            for change in patch.files {
                let file_patch = CommitReviewFilePatch {
                    before_oid: patch.before_oid.clone(),
                    after_oid: patch.after_oid.clone(),
                    change,
                };
                let mut connected_chain = None;
                let candidate_chain_indexes = chains_by_before_path
                    .get(file_patch.change.path.as_str())
                    .cloned()
                    .unwrap_or_default();
                for index in candidate_chain_indexes {
                    let chain = &chains[index];
                    let oldest = chain
                        .patches
                        .last()
                        .expect("review file chain is not empty");
                    let connected =
                        if oldest.before_oid.as_deref() == Some(file_patch.after_oid.as_str()) {
                            true
                        } else {
                            let key = (file_patch.after_oid.clone(), oldest.after_oid.clone());
                            if let Some(connected) = ancestry.get(&key) {
                                *connected
                            } else {
                                let connected = self.is_ancestor(
                                    cwd,
                                    &file_patch.after_oid,
                                    &oldest.after_oid,
                                    "git.commit-review-topology-query-failed",
                                )?;
                                ancestry.insert(key, connected);
                                connected
                            }
                        };
                    if connected {
                        connected_chain = Some(index);
                        break;
                    }
                }

                if let Some(index) = connected_chain {
                    let previous_before_path = file_chain_before_path(&chains[index]).to_string();
                    chains[index].patches.push(file_patch);
                    if let Some(indexes) = chains_by_before_path.get_mut(&previous_before_path) {
                        indexes.retain(|candidate| *candidate != index);
                    }
                    chains_by_before_path
                        .entry(file_chain_before_path(&chains[index]).to_string())
                        .or_default()
                        .push(index);
                    continue;
                }

                let before_path = file_patch_before_path(&file_patch).to_string();
                let chain_index = chains.len();
                chains.push(CommitReviewFileChain {
                    patches: vec![file_patch],
                });
                chains_by_before_path
                    .entry(before_path)
                    .or_default()
                    .push(chain_index);
            }
        }

        let mut duplicate_patches = HashSet::<(usize, usize)>::new();
        let mut equivalent_candidates =
            HashMap::<CommitReviewFilePatchSignature, Vec<(usize, usize)>>::new();
        for (chain_index, chain) in chains.iter().enumerate() {
            for (patch_index, patch) in chain.patches.iter().enumerate() {
                if !patch.change.binary {
                    equivalent_candidates
                        .entry(CommitReviewFilePatchSignature::from(patch))
                        .or_default()
                        .push((chain_index, patch_index));
                }
            }
        }
        for candidates in equivalent_candidates.values() {
            for left_index in 0..candidates.len() {
                for right_index in (left_index + 1)..candidates.len() {
                    let (left_chain_index, left_patch_index) = candidates[left_index];
                    let (right_chain_index, right_patch_index) = candidates[right_index];
                    if left_chain_index == right_chain_index {
                        continue;
                    }
                    let left_patch = &chains[left_chain_index].patches[left_patch_index];
                    let right_patch = &chains[right_chain_index].patches[right_patch_index];
                    let left_id =
                        self.commit_review_file_patch_id(cwd, left_patch, &mut patch_ids)?;
                    let right_id =
                        self.commit_review_file_patch_id(cwd, right_patch, &mut patch_ids)?;
                    if left_id.is_none() || left_id != right_id {
                        continue;
                    }
                    let keep_left = chains[left_chain_index].patches.len()
                        >= chains[right_chain_index].patches.len();
                    duplicate_patches.insert(if keep_left {
                        (right_chain_index, right_patch_index)
                    } else {
                        (left_chain_index, left_patch_index)
                    });
                }
            }
        }
        for (chain_index, chain) in chains.iter_mut().enumerate() {
            let mut patch_index = 0;
            chain.patches.retain(|_| {
                let keep = !duplicate_patches.contains(&(chain_index, patch_index));
                patch_index += 1;
                keep
            });
        }
        chains.retain(|chain| !chain.patches.is_empty());

        let mut files = chains
            .into_iter()
            .flat_map(|chain| {
                aggregate_commit_review_files(chain.patches.into_iter().rev().map(|file_patch| {
                    CommitReviewPatch {
                        before_oid: file_patch.before_oid,
                        after_oid: file_patch.after_oid,
                        files: vec![file_patch.change],
                    }
                }))
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.after_oid.cmp(&right.after_oid))
        });
        Ok(files)
    }

    fn populate_commit_review_stats(
        &self,
        cwd: &Utf8Path,
        files: &mut [GitCommitReviewFile],
    ) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let mut endpoint_groups = HashMap::<(Option<String>, String), Vec<usize>>::new();
        for (index, file) in files.iter().enumerate() {
            endpoint_groups
                .entry((file.before_oid.clone(), file.after_oid.clone()))
                .or_default()
                .push(index);
        }
        for ((before_oid, after_oid), indexes) in endpoint_groups {
            let mut paths = Vec::new();
            for index in &indexes {
                let file = &files[*index];
                if let Some(before_path) = file.before_path.as_ref()
                    && !paths.contains(before_path)
                {
                    paths.push(before_path.clone());
                }
                if !paths.contains(&file.path) {
                    paths.push(file.path.clone());
                }
            }
            let stats = self.commit_numstat(cwd, before_oid.as_deref(), &after_oid, &paths)?;
            for index in indexes {
                let file = &mut files[index];
                let Some(file_stats) = stats.get(&file.path) else {
                    continue;
                };
                file.binary = file_stats.binary;
                file.added_lines = file_stats.added_lines;
                file.deleted_lines = file_stats.deleted_lines;
            }
        }
        Ok(())
    }

    fn commit_numstat(
        &self,
        cwd: &Utf8Path,
        before_oid: Option<&str>,
        after_oid: &str,
        paths: &[String],
    ) -> Result<HashMap<String, CommitFileStats>> {
        let empty_tree_oid;
        let before_oid = if let Some(before_oid) = before_oid {
            before_oid
        } else {
            empty_tree_oid = self
                .runner
                .require_with_input(
                    cwd,
                    &["hash-object", "-t", "tree", "--stdin"],
                    &[],
                    "git.commit-diff-query-failed",
                )?
                .stdout_text();
            empty_tree_oid.as_str()
        };
        let mut args = vec![
            "diff".to_string(),
            "--numstat".to_string(),
            "-z".to_string(),
            "-M".to_string(),
            "-C".to_string(),
            before_oid.to_string(),
            after_oid.to_string(),
            "--".to_string(),
        ];
        args.extend(paths.iter().cloned());
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = self
            .runner
            .require(cwd, &args, "git.commit-diff-query-failed")?;
        parse_commit_numstat(&output.stdout)
    }

    fn commit_review_file_patch_id(
        &self,
        cwd: &Utf8Path,
        patch: &CommitReviewFilePatch,
        cache: &mut HashMap<CommitReviewFilePatchIdentity, String>,
    ) -> Result<Option<String>> {
        let identity = CommitReviewFilePatchIdentity::from(patch);
        if let Some(patch_id) = cache.get(&identity) {
            return Ok(Some(patch_id.clone()));
        }
        let mut pathspecs = vec![patch.change.path.as_str()];
        if let Some(old_path) = patch.change.old_path.as_deref()
            && old_path != patch.change.path
        {
            pathspecs.push(old_path);
        }
        let diff = if let Some(before_oid) = patch.before_oid.as_deref() {
            let mut args = vec![
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--binary",
                "--unified=0",
                before_oid,
                patch.after_oid.as_str(),
                "--",
            ];
            args.extend(pathspecs);
            self.runner
                .require(cwd, &args, "git.commit-review-patch-identity-failed")?
        } else {
            let mut args = vec![
                "show",
                "--format=",
                "--no-color",
                "--no-ext-diff",
                "--binary",
                "--unified=0",
                patch.after_oid.as_str(),
                "--",
            ];
            args.extend(pathspecs);
            self.runner
                .require(cwd, &args, "git.commit-review-patch-identity-failed")?
        };
        if diff.stdout.len() >= MACHINE_COMMAND_CAPTURE_LIMIT {
            return Ok(None);
        }
        let patch_id_output = self.runner.require_with_input(
            cwd,
            &["patch-id", "--stable"],
            &diff.stdout,
            "git.commit-review-patch-identity-failed",
        )?;
        let patch_id = patch_id_output
            .stdout_text()
            .split_whitespace()
            .next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                GitServiceError::new(
                    "git.commit-review-patch-identity-failed",
                    serde_json::json!({ "afterOid": patch.after_oid, "path": patch.change.path }),
                )
            })?
            .to_string();
        cache.insert(identity, patch_id.clone());
        Ok(Some(patch_id))
    }

    fn optional_revision_lines(
        &self,
        cwd: &Utf8Path,
        args: &[&str],
        code: &'static str,
    ) -> Result<Vec<String>> {
        let output = self.runner.run(cwd, args)?;
        if output.success {
            Ok(String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect())
        } else if output.exit_code == Some(1) {
            Ok(Vec::new())
        } else {
            Err(GitServiceError::command(code, &output).into())
        }
    }

    fn commit_file_changes(
        &self,
        cwd: &Utf8Path,
        before_oid: Option<&str>,
        after_oid: &str,
    ) -> Result<Vec<GitCommitFileChange>> {
        let name_output = if let Some(before_oid) = before_oid {
            self.runner.require(
                cwd,
                &[
                    "diff",
                    "--name-status",
                    "-z",
                    "-M",
                    "-C",
                    before_oid,
                    after_oid,
                    "--",
                ],
                "git.commit-diff-query-failed",
            )?
        } else {
            self.runner.require(
                cwd,
                &[
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "-r",
                    "--name-status",
                    "-z",
                    "-M",
                    "-C",
                    after_oid,
                    "--",
                ],
                "git.commit-diff-query-failed",
            )?
        };
        merge_commit_file_changes(
            parse_commit_name_status(&name_output.stdout)?,
            self.commit_numstat(cwd, before_oid, after_oid, &[])?,
        )
    }

    pub fn execute_mutation(
        &self,
        cwd: &Utf8Path,
        request: &GitMutationRequest,
    ) -> Result<GitMutationResult> {
        let identity = self.repository_identity(cwd)?;
        if matches!(
            &request.mutation,
            GitMutation::StagePaths { .. }
                | GitMutation::StageAll
                | GitMutation::UnstagePaths { .. }
                | GitMutation::UnstageAll
        ) {
            return GitCoordinationService.try_with_user_workspace_write(
                &identity.workspace_path,
                "update-index",
                || {
                    let current_status = self.status_without_stats(cwd)?;
                    let refs = self.refs(cwd)?;
                    if let Some(expected_revision) = request.expected_revision.as_deref() {
                        let actual_revision = workspace_snapshot_revision(&current_status, &refs);
                        if expected_revision != actual_revision {
                            return Err(GitServiceError::new(
                                "git.ref-changed",
                                serde_json::json!({
                                    "expectedRevision": expected_revision,
                                    "actualRevision": actual_revision,
                                }),
                            )
                            .into());
                        }
                    }
                    match &request.mutation {
                        GitMutation::StagePaths { paths } => self.stage_paths(cwd, paths)?,
                        GitMutation::StageAll => {
                            self.runner
                                .require(cwd, &["add", "-A"], "git.stage-failed")?;
                        }
                        GitMutation::UnstagePaths { paths } => self.unstage_paths(cwd, paths)?,
                        GitMutation::UnstageAll => self.unstage_all(cwd)?,
                        _ => unreachable!("workspace mutation kind was checked above"),
                    }
                    let mut status = self.status(cwd)?;
                    let repository_revision = workspace_snapshot_revision(&status, &refs);
                    status.snapshot_revision.clone_from(&repository_revision);
                    Ok(GitMutationResult::Workspace {
                        status,
                        repository_revision,
                    })
                },
            );
        }
        if let Some(expected_revision) = request.expected_revision.as_deref() {
            let status = self.status_without_stats(cwd)?;
            let refs = self.refs(cwd)?;
            let actual_revision = workspace_snapshot_revision(&status, &refs);
            if expected_revision != actual_revision {
                return Err(GitServiceError::new(
                    "git.ref-changed",
                    serde_json::json!({
                        "expectedRevision": expected_revision,
                        "actualRevision": actual_revision,
                    }),
                )
                .into());
            }
        }
        match &request.mutation {
            GitMutation::StagePaths { .. }
            | GitMutation::StageAll
            | GitMutation::UnstagePaths { .. }
            | GitMutation::UnstageAll => {
                unreachable!("workspace mutations return before repository mutations")
            }
            GitMutation::Commit { subject, body } => GitCoordinationService
                .try_with_user_workspace_write(&identity.workspace_path, "commit", || {
                    self.commit(cwd, subject, body.as_deref())
                })?,
            GitMutation::BranchCreate {
                name,
                start_point,
                checkout,
            } => GitCoordinationService.try_with_user_repository_write(
                &identity.common_dir,
                "branch-create",
                || self.branch_create(cwd, name, start_point.as_deref(), *checkout),
            )?,
            GitMutation::BranchSwitch { name } => GitCoordinationService
                .try_with_user_workspace_write(&identity.workspace_path, "branch-switch", || {
                    self.branch_switch(cwd, name)
                })?,
            GitMutation::BranchRename { old_name, new_name } => GitCoordinationService
                .try_with_user_write(
                    &identity.common_dir,
                    Some(&identity.workspace_path),
                    "branch-rename",
                    || self.branch_rename(cwd, old_name.as_deref(), new_name),
                )?,
            GitMutation::BranchDeleteSafe { name } => GitCoordinationService
                .try_with_user_repository_write(
                    &identity.common_dir,
                    "branch-delete-safe",
                    || self.branch_delete_safe(cwd, name),
                )?,
            GitMutation::TagCreate {
                name,
                target,
                style,
                message,
            } => GitCoordinationService.try_with_user_repository_write(
                &identity.common_dir,
                "tag-create",
                || self.tag_create(cwd, name, target.as_deref(), *style, message.as_deref()),
            )?,
            GitMutation::TagDeleteLocal { name } => GitCoordinationService
                .try_with_user_repository_write(&identity.common_dir, "tag-delete-local", || {
                    self.tag_delete(cwd, name)
                })?,
            GitMutation::WorktreeCreate {
                path,
                source_ref,
                new_branch,
            } => GitCoordinationService.try_with_user_repository_write(
                &identity.common_dir,
                "worktree-create",
                || self.worktree_create(cwd, path, source_ref, new_branch.as_deref()),
            )?,
            GitMutation::WorktreeRemove { path } => GitCoordinationService
                .try_with_user_repository_write(&identity.common_dir, "worktree-remove", || {
                    self.worktree_remove(cwd, &identity, path)
                })?,
        }
        Ok(GitMutationResult::Repository)
    }

    pub fn start_operation(
        &self,
        cwd: &Utf8Path,
        request: &GitOperationRequest,
    ) -> Result<GitOperation> {
        self.start_operation_with_update_sink(cwd, request, None)
    }

    pub fn start_operation_with_update_sink(
        &self,
        cwd: &Utf8Path,
        request: &GitOperationRequest,
        update_sink: Option<GitOperationUpdateSink>,
    ) -> Result<GitOperation> {
        let identity = self.repository_identity(cwd)?;
        if let Some(expected_revision) = request.expected_revision.as_deref() {
            let status = self.status_without_stats(cwd)?;
            let refs = self.refs(cwd)?;
            let actual_revision = workspace_snapshot_revision(&status, &refs);
            if expected_revision != actual_revision {
                return Err(GitServiceError::new(
                    "git.ref-changed",
                    serde_json::json!({
                        "expectedRevision": expected_revision,
                        "actualRevision": actual_revision,
                    }),
                )
                .into());
            }
        }
        self.validate_operation(cwd, &request.operation)?;
        let kind = request.operation.kind();
        let workspace_path =
            operation_uses_workspace(kind).then(|| identity.workspace_path.clone());
        let operation = GitOperation {
            operation_id: Uuid::new_v4().to_string(),
            kind,
            repository_common_dir: identity.common_dir.clone(),
            workspace_path,
            status: GitOperationStatus::Queued,
            cancelable: true,
            started_at: None,
            completed_at: None,
            error: None,
        };
        let cell = Arc::new(GitOperationCell {
            state: Mutex::new(operation.clone()),
            process: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
            update_sink,
        });
        operation_registry()
            .operations
            .lock()
            .map_err(|_| {
                GitServiceError::new("git.operation-registry-poisoned", serde_json::json!({}))
            })?
            .insert(operation.operation_id.clone(), cell.clone());
        let input = request.operation.clone();
        thread::spawn(move || run_git_operation(identity, input, cell));
        Ok(operation)
    }

    pub fn get_operation(&self, operation_id: &str) -> Result<GitOperation> {
        let cell = operation_cell(operation_id)?;
        let state = cell.state.lock().map_err(|_| {
            GitServiceError::new("git.operation-state-poisoned", serde_json::json!({}))
        })?;
        Ok(state.clone())
    }

    pub fn cancel_operation(&self, operation_id: &str) -> Result<GitOperation> {
        let cell = operation_cell(operation_id)?;
        cell.cancel_requested.store(true, Ordering::SeqCst);
        {
            let mut process = cell.process.lock().map_err(|_| {
                GitServiceError::new("git.operation-process-poisoned", serde_json::json!({}))
            })?;
            if let Some(process) = process.as_mut() {
                process.terminate(PROCESS_GROUP_TERMINATION_GRACE)?;
            }
        }
        let operation = {
            let mut state = cell.state.lock().map_err(|_| {
                GitServiceError::new("git.operation-state-poisoned", serde_json::json!({}))
            })?;
            if matches!(
                state.status,
                GitOperationStatus::Queued | GitOperationStatus::Running
            ) {
                state.status = GitOperationStatus::Cancelled;
                state.cancelable = false;
                state.completed_at = Some(operation_timestamp());
                state.error = None;
            }
            state.clone()
        };
        emit_operation_update(&cell, operation.clone());
        Ok(operation)
    }

    fn validate_operation(&self, cwd: &Utf8Path, input: &GitOperationInput) -> Result<()> {
        match input {
            GitOperationInput::Fetch { remote, .. } => {
                if let Some(remote) = remote {
                    self.validate_remote(cwd, remote)?;
                }
            }
            GitOperationInput::Pull { remote, branch, .. } => {
                if remote.is_some() != branch.is_some() {
                    return Err(GitServiceError::new(
                        "git.pull-remote-branch-required",
                        serde_json::json!({}),
                    )
                    .into());
                }
                if let Some(remote) = remote {
                    self.validate_remote(cwd, remote)?;
                }
                if let Some(branch) = branch {
                    validate_revision(branch)?;
                }
            }
            GitOperationInput::Push { remote, branch, .. } => {
                self.validate_remote(cwd, remote)?;
                self.validate_branch_name(cwd, branch)?;
                let full_name = format!("refs/heads/{branch}");
                if is_runtime_branch(&full_name) {
                    return Err(GitServiceError::new(
                        "git.runtime-branch-push-forbidden",
                        serde_json::json!({ "branch": branch }),
                    )
                    .into());
                }
                if !self.refs(cwd)?.iter().any(|git_ref| {
                    git_ref.kind == GitRefKind::LocalBranch && git_ref.short_name == *branch
                }) {
                    return Err(GitServiceError::new(
                        "git.branch-not-found",
                        serde_json::json!({ "branch": branch }),
                    )
                    .into());
                }
            }
            GitOperationInput::PushTag { remote, tag } => {
                self.validate_remote(cwd, remote)?;
                self.validate_tag_name(cwd, tag)?;
                if !self
                    .refs(cwd)?
                    .iter()
                    .any(|git_ref| git_ref.kind == GitRefKind::Tag && git_ref.short_name == *tag)
                {
                    return Err(GitServiceError::new(
                        "git.tag-not-found",
                        serde_json::json!({ "tag": tag }),
                    )
                    .into());
                }
            }
            GitOperationInput::StashCreate {
                include_untracked, ..
            } => {
                let status = self.status_without_stats(cwd)?;
                let tracked_changes = !status.staged.is_empty()
                    || !status.unstaged.is_empty()
                    || !status.conflicts.is_empty();
                if !tracked_changes && (!include_untracked || status.untracked.is_empty()) {
                    return Err(GitServiceError::new(
                        "git.nothing-to-stash",
                        serde_json::json!({}),
                    )
                    .into());
                }
            }
            GitOperationInput::StashApply { stash_ref, .. } => {
                if !self
                    .stashes(cwd)?
                    .iter()
                    .any(|stash| stash.ref_name == *stash_ref)
                {
                    return Err(GitServiceError::new(
                        "git.stash-not-found",
                        serde_json::json!({ "stashRef": stash_ref }),
                    )
                    .into());
                }
            }
            GitOperationInput::MergeContinue | GitOperationInput::MergeAbort => {
                self.require_in_progress_operation(cwd, GitInProgressOperationKind::Merge)?;
            }
            GitOperationInput::RebaseContinue
            | GitOperationInput::RebaseSkip
            | GitOperationInput::RebaseAbort => {
                self.require_in_progress_operation(cwd, GitInProgressOperationKind::Rebase)?;
            }
        }
        Ok(())
    }

    fn require_in_progress_operation(
        &self,
        cwd: &Utf8Path,
        expected: GitInProgressOperationKind,
    ) -> Result<()> {
        let actual = self.in_progress_operation(cwd)?;
        if actual
            .as_ref()
            .is_some_and(|operation| operation.kind == expected)
        {
            Ok(())
        } else {
            Err(GitServiceError::new(
                "git.operation-state-mismatch",
                serde_json::json!({
                    "expected": expected,
                    "actual": actual.map(|operation| operation.kind),
                }),
            )
            .into())
        }
    }

    fn validate_remote(&self, cwd: &Utf8Path, remote: &str) -> Result<()> {
        if remote.is_empty() || remote.starts_with('-') || remote.contains('\0') {
            return Err(GitServiceError::new(
                "git.invalid-remote",
                serde_json::json!({ "remote": remote }),
            )
            .into());
        }
        if self.remotes(cwd)?.iter().any(|item| item.name == remote) {
            Ok(())
        } else {
            Err(GitServiceError::new(
                "git.remote-not-found",
                serde_json::json!({ "remote": remote }),
            )
            .into())
        }
    }

    pub fn comparison(
        &self,
        cwd: &Utf8Path,
        source: &GitComparisonSource,
    ) -> Result<GitFileComparison> {
        let (path, before, after) = match source {
            GitComparisonSource::Workspace { path, area, .. } => {
                validate_repo_relative_path(path)?;
                match area {
                    GitWorkspaceDiffArea::Staged => {
                        let before = if self.has_head(cwd)? {
                            self.read_git_text(cwd, &format!("HEAD:{path}"))?
                        } else {
                            None
                        };
                        let after = self.read_git_text(cwd, &format!(":{path}"))?;
                        (path.clone(), before, after)
                    }
                    GitWorkspaceDiffArea::Unstaged => {
                        let before = self.read_git_text(cwd, &format!(":{path}"))?;
                        let after = read_worktree_text(cwd, path)?;
                        (path.clone(), before, after)
                    }
                }
            }
            GitComparisonSource::Commit {
                path,
                before_oid,
                before_path,
                after_oid,
                ..
            } => {
                validate_repo_relative_path(path)?;
                if let Some(before_path) = before_path {
                    validate_repo_relative_path(before_path)?;
                }
                validate_revision(after_oid)?;
                if let Some(before_oid) = before_oid {
                    validate_revision(before_oid)?;
                }
                let before = before_oid
                    .as_ref()
                    .map(|oid| {
                        let path = before_path.as_deref().unwrap_or(path);
                        self.read_git_text(cwd, &format!("{oid}:{path}"))
                    })
                    .transpose()?
                    .flatten();
                let after = self.read_git_text(cwd, &format!("{after_oid}:{path}"))?;
                (path.clone(), before, after)
            }
            GitComparisonSource::GitHubPr { .. } => {
                return Err(GitServiceError::new(
                    "git.comparison-source-unsupported",
                    serde_json::json!({ "kind": "github-pr" }),
                )
                .into());
            }
        };
        comparison_from_versions(path, before, after)
    }

    fn stage_paths(&self, cwd: &Utf8Path, paths: &[String]) -> Result<()> {
        let input = pathspec_input(paths)?;
        self.runner.require_with_input(
            cwd,
            &[
                "--literal-pathspecs",
                "add",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            &input,
            "git.stage-failed",
        )?;
        Ok(())
    }

    fn unstage_paths(&self, cwd: &Utf8Path, paths: &[String]) -> Result<()> {
        let input = pathspec_input(paths)?;
        if self.has_head(cwd)? {
            self.runner.require_with_input(
                cwd,
                &[
                    "--literal-pathspecs",
                    "restore",
                    "--staged",
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                &input,
                "git.unstage-failed",
            )?;
        } else {
            self.runner.require_with_input(
                cwd,
                &[
                    "--literal-pathspecs",
                    "rm",
                    "--cached",
                    "-r",
                    "--ignore-unmatch",
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                &input,
                "git.unstage-failed",
            )?;
        }
        Ok(())
    }

    fn unstage_all(&self, cwd: &Utf8Path) -> Result<()> {
        let args = if self.has_head(cwd)? {
            vec!["restore", "--staged", "--", "."]
        } else {
            vec!["rm", "--cached", "-r", "--ignore-unmatch", "--", "."]
        };
        self.runner.require(cwd, &args, "git.unstage-failed")?;
        Ok(())
    }

    fn commit(&self, cwd: &Utf8Path, subject: &str, body: Option<&str>) -> Result<()> {
        let subject = subject.trim();
        if subject.is_empty() || subject.contains('\n') || subject.contains('\r') {
            return Err(
                GitServiceError::new("git.invalid-commit-subject", serde_json::json!({})).into(),
            );
        }
        if self.status_without_stats(cwd)?.staged.is_empty() {
            return Err(GitServiceError::new("git.nothing-staged", serde_json::json!({})).into());
        }
        let mut message = subject.to_string();
        if let Some(body) = body.map(str::trim).filter(|body| !body.is_empty()) {
            message.push_str("\n\n");
            message.push_str(body);
        }
        message.push('\n');
        self.runner.require_with_input(
            cwd,
            &["commit", "--file=-"],
            message.as_bytes(),
            "git.commit-failed",
        )?;
        Ok(())
    }

    fn branch_create(
        &self,
        cwd: &Utf8Path,
        name: &str,
        start_point: Option<&str>,
        checkout: bool,
    ) -> Result<()> {
        self.validate_branch_name(cwd, name)?;
        if let Some(start_point) = start_point {
            validate_revision(start_point)?;
        }
        let command = if checkout { "switch" } else { "branch" };
        let mut args = vec![command, if checkout { "-c" } else { "--" }, name];
        if let Some(start_point) = start_point {
            args.push(start_point);
        }
        self.runner
            .require(cwd, &args, "git.branch-create-failed")?;
        Ok(())
    }

    fn branch_switch(&self, cwd: &Utf8Path, name: &str) -> Result<()> {
        self.validate_branch_name(cwd, name)?;
        let output = self.runner.run(cwd, &["switch", "--", name])?;
        if !output.success {
            return Err(git_command_error("git.branch-switch-failed", &output).into());
        }
        Ok(())
    }

    fn branch_rename(&self, cwd: &Utf8Path, old_name: Option<&str>, new_name: &str) -> Result<()> {
        self.validate_branch_name(cwd, new_name)?;
        let mut args = vec!["branch", "-m"];
        if let Some(old_name) = old_name {
            self.validate_branch_name(cwd, old_name)?;
            args.push(old_name);
        }
        args.push(new_name);
        self.runner
            .require(cwd, &args, "git.branch-rename-failed")?;
        Ok(())
    }

    fn branch_delete_safe(&self, cwd: &Utf8Path, name: &str) -> Result<()> {
        self.validate_branch_name(cwd, name)?;
        let output = self.runner.run(cwd, &["branch", "-d", "--", name])?;
        if !output.success {
            return Err(git_command_error("git.branch-delete-not-merged", &output).into());
        }
        Ok(())
    }

    fn tag_create(
        &self,
        cwd: &Utf8Path,
        name: &str,
        target: Option<&str>,
        style: GitTagStyle,
        message: Option<&str>,
    ) -> Result<()> {
        self.validate_tag_name(cwd, name)?;
        let target = target.unwrap_or("HEAD");
        validate_revision(target)?;
        match style {
            GitTagStyle::Annotated => {
                let message = message
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(name);
                self.runner.require_with_input(
                    cwd,
                    &["tag", "-a", name, target, "--file=-"],
                    message.as_bytes(),
                    "git.tag-create-failed",
                )?;
            }
            GitTagStyle::Lightweight => {
                self.runner
                    .require(cwd, &["tag", name, target], "git.tag-create-failed")?;
            }
        }
        Ok(())
    }

    fn tag_delete(&self, cwd: &Utf8Path, name: &str) -> Result<()> {
        self.validate_tag_name(cwd, name)?;
        self.runner
            .require(cwd, &["tag", "-d", "--", name], "git.tag-delete-failed")?;
        Ok(())
    }

    fn worktree_create(
        &self,
        cwd: &Utf8Path,
        path: &Utf8Path,
        source_ref: &str,
        new_branch: Option<&str>,
    ) -> Result<()> {
        validate_revision(source_ref)?;
        if path.exists() {
            return Err(GitServiceError::new(
                "git.worktree-path-exists",
                serde_json::json!({ "path": path }),
            )
            .into());
        }
        if let Some(branch) = new_branch {
            self.validate_branch_name(cwd, branch)?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }
        let mut args = vec!["worktree", "add"];
        if let Some(branch) = new_branch {
            args.extend(["-b", branch]);
        }
        args.extend([path.as_str(), source_ref]);
        let output = self.runner.run(cwd, &args)?;
        if !output.success {
            return Err(git_command_error("git.worktree-create-failed", &output).into());
        }
        Ok(())
    }

    fn worktree_remove(
        &self,
        cwd: &Utf8Path,
        identity: &GitRepositoryIdentity,
        requested_path: &Utf8Path,
    ) -> Result<()> {
        let requested_key = normalized_lock_path(requested_path).map_err(|_| {
            GitServiceError::new(
                "git.worktree-not-found",
                serde_json::json!({ "path": requested_path }),
            )
        })?;
        let target = self
            .worktrees(cwd)?
            .into_iter()
            .find(|worktree| {
                normalized_lock_path(&worktree.path)
                    .is_ok_and(|candidate| candidate == requested_key)
            })
            .ok_or_else(|| {
                GitServiceError::new(
                    "git.worktree-not-found",
                    serde_json::json!({ "path": requested_path }),
                )
            })?;
        if normalized_lock_path(&target.path)? == normalized_lock_path(&identity.workspace_path)? {
            return Err(GitServiceError::new(
                "git.worktree-current-remove-forbidden",
                serde_json::json!({ "path": target.path }),
            )
            .into());
        }

        let output = self
            .runner
            .run(cwd, &["worktree", "remove", "--", target.path.as_str()])?;
        if output.success {
            return Ok(());
        }
        let command_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
        .to_ascii_lowercase();
        let code = if command_output.contains("contains modified or untracked files")
            || command_output.contains("contains modified files")
        {
            "git.worktree-remove-dirty"
        } else {
            "git.worktree-remove-failed"
        };
        Err(GitServiceError::command(code, &output).into())
    }

    fn validate_branch_name(&self, cwd: &Utf8Path, name: &str) -> Result<()> {
        let output = self
            .runner
            .run(cwd, &["check-ref-format", "--branch", name])?;
        if output.success {
            Ok(())
        } else {
            Err(GitServiceError::new(
                "git.invalid-branch-name",
                serde_json::json!({ "name": name }),
            )
            .into())
        }
    }

    fn validate_tag_name(&self, cwd: &Utf8Path, name: &str) -> Result<()> {
        let full_name = format!("refs/tags/{name}");
        let output = self
            .runner
            .run(cwd, &["check-ref-format", full_name.as_str()])?;
        if output.success {
            Ok(())
        } else {
            Err(
                GitServiceError::new("git.invalid-tag-name", serde_json::json!({ "name": name }))
                    .into(),
            )
        }
    }

    fn has_head(&self, cwd: &Utf8Path) -> Result<bool> {
        Ok(self
            .runner
            .run(cwd, &["rev-parse", "--verify", "HEAD"])?
            .success)
    }

    fn read_git_text(&self, cwd: &Utf8Path, object: &str) -> Result<Option<Vec<u8>>> {
        let exists = self.runner.run(cwd, &["cat-file", "-e", object])?;
        if !exists.success {
            return Ok(None);
        }
        let output = self.runner.run(cwd, &["show", object])?;
        if !output.success {
            return Err(GitServiceError::command("git.file-read-failed", &output).into());
        }
        Ok(Some(output.stdout))
    }

    pub(crate) fn remotes(&self, cwd: &Utf8Path) -> Result<Vec<GitRemote>> {
        let names = self
            .runner
            .require(cwd, &["remote"], "git.remote-query-failed")?
            .stdout_text();
        let mut remotes = Vec::new();
        for name in names.lines().filter(|name| !name.trim().is_empty()) {
            let fetch_urls = self
                .runner
                .require(
                    cwd,
                    &["remote", "get-url", "--all", name],
                    "git.remote-query-failed",
                )?
                .stdout_text()
                .lines()
                .map(str::to_string)
                .collect();
            let push_output = self
                .runner
                .run(cwd, &["remote", "get-url", "--push", "--all", name])?;
            let push_urls = if push_output.success {
                push_output
                    .stdout_text()
                    .lines()
                    .map(str::to_string)
                    .collect()
            } else {
                Vec::new()
            };
            remotes.push(GitRemote {
                name: name.to_string(),
                fetch_urls,
                push_urls,
            });
        }
        Ok(remotes)
    }

    fn in_progress_operation(&self, cwd: &Utf8Path) -> Result<Option<GitInProgressOperation>> {
        for (marker, kind) in [
            ("MERGE_HEAD", GitInProgressOperationKind::Merge),
            ("rebase-merge", GitInProgressOperationKind::Rebase),
            ("rebase-apply", GitInProgressOperationKind::Rebase),
            ("CHERRY_PICK_HEAD", GitInProgressOperationKind::CherryPick),
            ("REVERT_HEAD", GitInProgressOperationKind::Revert),
        ] {
            let output = self.runner.run(
                cwd,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
            )?;
            if output.success && Utf8Path::new(&output.stdout_text()).exists() {
                let current_oid = (kind == GitInProgressOperationKind::Rebase)
                    .then(|| self.runner.run(cwd, &["rev-parse", "REBASE_HEAD"]))
                    .transpose()?
                    .filter(|output| output.success)
                    .map(|output| output.stdout_text());
                let current_subject = current_oid
                    .as_deref()
                    .map(|oid| self.runner.run(cwd, &["show", "-s", "--format=%s", oid]))
                    .transpose()?
                    .filter(|output| output.success)
                    .map(|output| output.stdout_text());
                return Ok(Some(GitInProgressOperation {
                    kind,
                    current_oid,
                    current_subject,
                }));
            }
        }
        Ok(None)
    }
}

fn operation_registry() -> &'static GitOperationRegistry {
    GIT_OPERATION_REGISTRY.get_or_init(GitOperationRegistry::default)
}

fn operation_cell(operation_id: &str) -> Result<Arc<GitOperationCell>> {
    operation_registry()
        .operations
        .lock()
        .map_err(|_| {
            GitServiceError::new("git.operation-registry-poisoned", serde_json::json!({}))
        })?
        .get(operation_id)
        .cloned()
        .ok_or_else(|| {
            GitServiceError::new(
                "git.operation-not-found",
                serde_json::json!({ "operationId": operation_id }),
            )
            .into()
        })
}

fn operation_uses_workspace(kind: GitOperationKind) -> bool {
    matches!(
        kind,
        GitOperationKind::Pull
            | GitOperationKind::StashCreate
            | GitOperationKind::StashApply
            | GitOperationKind::MergeContinue
            | GitOperationKind::MergeAbort
            | GitOperationKind::RebaseContinue
            | GitOperationKind::RebaseSkip
            | GitOperationKind::RebaseAbort
    )
}

fn operation_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn run_git_operation(
    identity: GitRepositoryIdentity,
    input: GitOperationInput,
    cell: Arc<GitOperationCell>,
) {
    if cell.cancel_requested.load(Ordering::SeqCst) {
        finish_operation(&cell, GitOperationStatus::Cancelled, None);
        return;
    }
    let running = if let Ok(mut state) = cell.state.lock() {
        state.status = GitOperationStatus::Running;
        state.started_at = Some(operation_timestamp());
        Some(state.clone())
    } else {
        None
    };
    if let Some(operation) = running {
        emit_operation_update(&cell, operation);
    }
    let kind = input.kind();
    let action = || execute_managed_git_operation(&identity.workspace_path, &input, &cell);
    let result = match kind {
        GitOperationKind::Fetch | GitOperationKind::Push | GitOperationKind::PushTag => {
            GitCoordinationService.try_with_user_repository_write(
                &identity.common_dir,
                operation_name(kind),
                action,
            )
        }
        GitOperationKind::Pull
        | GitOperationKind::MergeContinue
        | GitOperationKind::MergeAbort
        | GitOperationKind::RebaseContinue
        | GitOperationKind::RebaseSkip
        | GitOperationKind::RebaseAbort => GitCoordinationService.try_with_user_write(
            &identity.common_dir,
            Some(&identity.workspace_path),
            operation_name(kind),
            action,
        ),
        GitOperationKind::StashCreate | GitOperationKind::StashApply => GitCoordinationService
            .try_with_user_workspace_write(&identity.workspace_path, operation_name(kind), action),
    };
    if cell.cancel_requested.load(Ordering::SeqCst) {
        finish_operation(&cell, GitOperationStatus::Cancelled, None);
        return;
    }
    match result {
        Ok(output) if output.success => {
            finish_operation(&cell, GitOperationStatus::Succeeded, None);
        }
        Ok(output) => {
            let conflicts = GitSourceControlService::default()
                .status(&identity.workspace_path)
                .map(|status| !status.conflicts.is_empty())
                .unwrap_or(false);
            if conflicts
                && matches!(
                    kind,
                    GitOperationKind::Pull
                        | GitOperationKind::StashApply
                        | GitOperationKind::RebaseContinue
                )
            {
                let code = if matches!(
                    kind,
                    GitOperationKind::Pull | GitOperationKind::RebaseContinue
                ) {
                    "git.pull-conflict"
                } else {
                    "git.stash-apply-conflict"
                };
                finish_operation(
                    &cell,
                    GitOperationStatus::Conflicted,
                    Some(GitOperationError {
                        code: code.to_string(),
                        params: serde_json::json!({}),
                    }),
                );
            } else {
                let error = git_command_error(operation_failure_code(kind), &output);
                finish_operation(
                    &cell,
                    GitOperationStatus::Failed,
                    Some(service_operation_error(&error)),
                );
            }
        }
        Err(error) => {
            finish_operation(
                &cell,
                GitOperationStatus::Failed,
                Some(anyhow_operation_error(&error)),
            );
        }
    }
}

fn execute_managed_git_operation(
    cwd: &Utf8Path,
    input: &GitOperationInput,
    cell: &GitOperationCell,
) -> Result<MachineCommandOutput> {
    if matches!(
        input,
        GitOperationInput::MergeContinue | GitOperationInput::RebaseContinue
    ) {
        stage_unmerged_paths(cwd)?;
    }
    let args = operation_args(input);
    let mut command = background_command("git");
    command
        .arg("-C")
        .arg(cwd.as_str())
        .args(&args)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = ManagedProcessGroup::spawn(&mut command)
        .with_context(|| format!("failed to start Git operation in `{cwd}`"))?;
    let stdout = process
        .take_stdout()
        .ok_or_else(|| GitServiceError::new("git.stdout-unavailable", serde_json::json!({})))?;
    let stderr = process
        .take_stderr()
        .ok_or_else(|| GitServiceError::new("git.stderr-unavailable", serde_json::json!({})))?;
    let stdout_reader = thread::spawn(move || read_bounded_output(stdout));
    let stderr_reader = thread::spawn(move || read_bounded_output(stderr));
    {
        let mut slot = cell.process.lock().map_err(|_| {
            GitServiceError::new("git.operation-process-poisoned", serde_json::json!({}))
        })?;
        *slot = Some(process);
    }
    let status = loop {
        if cell.cancel_requested.load(Ordering::SeqCst) {
            let mut slot = cell.process.lock().map_err(|_| {
                GitServiceError::new("git.operation-process-poisoned", serde_json::json!({}))
            })?;
            if let Some(process) = slot.as_mut() {
                let _ = process.terminate(PROCESS_GROUP_TERMINATION_GRACE);
            }
        }
        let status = {
            let mut slot = cell.process.lock().map_err(|_| {
                GitServiceError::new("git.operation-process-poisoned", serde_json::json!({}))
            })?;
            slot.as_mut()
                .map(ManagedProcessGroup::try_wait)
                .transpose()?
        };
        if let Some(status) = status.flatten() {
            break status;
        }
        thread::sleep(OPERATION_POLL_INTERVAL);
    };
    let process = cell
        .process
        .lock()
        .map_err(|_| GitServiceError::new("git.operation-process-poisoned", serde_json::json!({})))?
        .take();
    drop(process);
    let stdout = stdout_reader
        .join()
        .map_err(|_| GitServiceError::new("git.output-reader-failed", serde_json::json!({})))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| GitServiceError::new("git.output-reader-failed", serde_json::json!({})))??;
    Ok(MachineCommandOutput {
        success: status.success(),
        exit_code: status.code(),
        stdout,
        stderr,
    })
}

fn stage_unmerged_paths(cwd: &Utf8Path) -> Result<()> {
    let runner = GitMachineRunner;
    let output = runner.require(
        cwd,
        &["diff", "--name-only", "--diff-filter=U", "-z"],
        "git.conflict-paths-query-failed",
    )?;
    let paths = nul_fields(&output.stdout);
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add".to_string(), "--".to_string()];
    for path in paths {
        args.push(String::from_utf8_lossy(&path).to_string());
    }
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    runner.require(cwd, &refs, "git.conflict-stage-failed")?;
    Ok(())
}

fn read_bounded_output(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = OPERATION_OUTPUT_LIMIT.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(output)
}

fn operation_args(input: &GitOperationInput) -> Vec<String> {
    match input {
        GitOperationInput::Fetch { remote, prune } => {
            let mut args = vec!["fetch".to_string()];
            if *prune {
                args.push("--prune".to_string());
            }
            if let Some(remote) = remote {
                args.extend(["--".to_string(), remote.clone()]);
            } else {
                args.push("--all".to_string());
            }
            args
        }
        GitOperationInput::Pull {
            remote,
            branch,
            strategy,
        } => {
            let mut args = vec!["pull".to_string(), "--no-edit".to_string()];
            args.push(
                match strategy {
                    GitPullStrategy::FastForwardOnly => "--ff-only",
                    GitPullStrategy::Merge => "--no-rebase",
                    GitPullStrategy::Rebase => "--rebase",
                }
                .to_string(),
            );
            if let (Some(remote), Some(branch)) = (remote, branch) {
                args.extend(["--".to_string(), remote.clone(), branch.clone()]);
            }
            args
        }
        GitOperationInput::Push {
            remote,
            branch,
            set_upstream,
        } => {
            let mut args = vec!["push".to_string(), "--porcelain".to_string()];
            if *set_upstream {
                args.push("--set-upstream".to_string());
            }
            args.extend([
                "--".to_string(),
                remote.clone(),
                format!("refs/heads/{branch}:refs/heads/{branch}"),
            ]);
            args
        }
        GitOperationInput::PushTag { remote, tag } => vec![
            "push".to_string(),
            "--porcelain".to_string(),
            "--".to_string(),
            remote.clone(),
            format!("refs/tags/{tag}:refs/tags/{tag}"),
        ],
        GitOperationInput::StashCreate {
            message,
            include_untracked,
        } => {
            let mut args = vec!["stash".to_string(), "push".to_string()];
            if *include_untracked {
                args.push("--include-untracked".to_string());
            }
            if let Some(message) = message
                .as_ref()
                .map(|value| value.trim())
                .filter(|v| !v.is_empty())
            {
                args.extend(["--message".to_string(), message.to_string()]);
            }
            args
        }
        GitOperationInput::StashApply {
            stash_ref,
            restore_index,
        } => {
            let mut args = vec!["stash".to_string(), "apply".to_string()];
            if *restore_index {
                args.push("--index".to_string());
            }
            args.push(stash_ref.clone());
            args
        }
        GitOperationInput::MergeContinue => vec!["merge".to_string(), "--continue".to_string()],
        GitOperationInput::MergeAbort => vec!["merge".to_string(), "--abort".to_string()],
        GitOperationInput::RebaseContinue => vec!["rebase".to_string(), "--continue".to_string()],
        GitOperationInput::RebaseSkip => vec!["rebase".to_string(), "--skip".to_string()],
        GitOperationInput::RebaseAbort => vec!["rebase".to_string(), "--abort".to_string()],
    }
}

fn operation_name(kind: GitOperationKind) -> &'static str {
    match kind {
        GitOperationKind::Fetch => "fetch",
        GitOperationKind::Pull => "pull",
        GitOperationKind::Push => "push",
        GitOperationKind::PushTag => "push-tag",
        GitOperationKind::StashCreate => "stash-create",
        GitOperationKind::StashApply => "stash-apply",
        GitOperationKind::MergeContinue => "merge-continue",
        GitOperationKind::MergeAbort => "merge-abort",
        GitOperationKind::RebaseContinue => "rebase-continue",
        GitOperationKind::RebaseSkip => "rebase-skip",
        GitOperationKind::RebaseAbort => "rebase-abort",
    }
}

fn operation_failure_code(kind: GitOperationKind) -> &'static str {
    match kind {
        GitOperationKind::Fetch => "git.fetch-failed",
        GitOperationKind::Pull => "git.pull-failed",
        GitOperationKind::Push => "git.push-failed",
        GitOperationKind::PushTag => "git.push-tag-failed",
        GitOperationKind::StashCreate => "git.stash-create-failed",
        GitOperationKind::StashApply => "git.stash-apply-failed",
        GitOperationKind::MergeContinue => "git.merge-continue-failed",
        GitOperationKind::MergeAbort => "git.merge-abort-failed",
        GitOperationKind::RebaseContinue => "git.rebase-continue-failed",
        GitOperationKind::RebaseSkip => "git.rebase-skip-failed",
        GitOperationKind::RebaseAbort => "git.rebase-abort-failed",
    }
}

fn service_operation_error(error: &GitServiceError) -> GitOperationError {
    GitOperationError {
        code: error.code.to_string(),
        params: error.params.clone(),
    }
}

fn anyhow_operation_error(error: &anyhow::Error) -> GitOperationError {
    error
        .downcast_ref::<GitServiceError>()
        .map(service_operation_error)
        .unwrap_or_else(|| GitOperationError {
            code: "git.operation-failed".to_string(),
            params: serde_json::json!({}),
        })
}

fn finish_operation(
    cell: &GitOperationCell,
    status: GitOperationStatus,
    error: Option<GitOperationError>,
) {
    let operation = if let Ok(mut state) = cell.state.lock() {
        state.status = status;
        state.cancelable = false;
        state.completed_at = Some(operation_timestamp());
        state.error = error;
        Some(state.clone())
    } else {
        None
    };
    if let Some(operation) = operation {
        emit_operation_update(cell, operation);
    }
}

fn emit_operation_update(cell: &GitOperationCell, operation: GitOperation) {
    if let Some(sink) = &cell.update_sink {
        sink(operation);
    }
}

fn canonical_utf8_path(path: &Utf8Path) -> Result<Utf8PathBuf> {
    let canonical = std::fs::canonicalize(path.as_std_path())
        .with_context(|| format!("failed to canonicalize Git workspace path `{path}`"))?;
    Utf8PathBuf::from_path_buf(canonical)
        .map_err(|_| GitServiceError::new("git.path-not-utf8", serde_json::json!({})).into())
}

fn validate_revision(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && !value.starts_with('-') && !value.contains('\0'),
        GitServiceError::new(
            "git.invalid-revision",
            serde_json::json!({ "revision": value })
        )
    );
    Ok(())
}

fn pathspec_input(paths: &[String]) -> Result<Vec<u8>> {
    if paths.is_empty() {
        return Err(GitServiceError::new("git.paths-required", serde_json::json!({})).into());
    }
    let mut input = Vec::new();
    for value in paths {
        let path = Utf8Path::new(value);
        let valid = !value.is_empty()
            && !value.contains('\0')
            && !path.is_absolute()
            && !path.components().any(|component| {
                matches!(
                    component,
                    camino::Utf8Component::ParentDir | camino::Utf8Component::RootDir
                )
            });
        if !valid {
            return Err(GitServiceError::new(
                "git.invalid-pathspec",
                serde_json::json!({ "path": value }),
            )
            .into());
        }
        input.extend_from_slice(value.as_bytes());
        input.push(0);
    }
    Ok(input)
}

pub(crate) fn validate_repo_relative_path(value: &str) -> Result<()> {
    let path = Utf8Path::new(value);
    let valid = !value.is_empty()
        && !value.contains('\0')
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                camino::Utf8Component::ParentDir | camino::Utf8Component::RootDir
            )
        });
    if valid {
        Ok(())
    } else {
        Err(
            GitServiceError::new("git.invalid-pathspec", serde_json::json!({ "path": value }))
                .into(),
        )
    }
}

fn read_worktree_text(cwd: &Utf8Path, relative_path: &str) -> Result<Option<Vec<u8>>> {
    let path = cwd.join(relative_path);
    let metadata = match std::fs::symlink_metadata(path.as_std_path()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(path.as_std_path())?;
        return Ok(Some(target.to_string_lossy().as_bytes().to_vec()));
    }
    ensure!(
        metadata.is_file(),
        GitServiceError::new(
            "git.file-not-regular",
            serde_json::json!({ "path": relative_path })
        )
    );
    Ok(Some(std::fs::read(path.as_std_path())?))
}

pub(crate) fn comparison_from_versions(
    path: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
) -> Result<GitFileComparison> {
    const MAX_TEXT_DIFF_BYTES: usize = 4 * 1024 * 1024;
    let versions = [before.as_deref(), after.as_deref()];
    let limitation_code = if versions
        .iter()
        .flatten()
        .any(|content| content.len() > MAX_TEXT_DIFF_BYTES)
    {
        Some("git.diff-too-large")
    } else if versions
        .iter()
        .flatten()
        .any(|content| content.contains(&0))
    {
        Some("git.binary-diff-unsupported")
    } else if versions
        .iter()
        .flatten()
        .any(|content| std::str::from_utf8(content).is_err())
    {
        Some("git.text-encoding-unsupported")
    } else {
        None
    };
    if let Some(code) = limitation_code {
        return Ok(GitFileComparison {
            path,
            stats: GitDiffStats {
                added_lines: 0,
                deleted_lines: 0,
            },
            before: None,
            after: None,
            limitation_code: Some(code.to_string()),
        });
    }
    let before = before
        .map(String::from_utf8)
        .transpose()
        .context("validated Git text was not UTF-8")?
        .map(normalize_line_endings);
    let after = after
        .map(String::from_utf8)
        .transpose()
        .context("validated Git text was not UTF-8")?
        .map(normalize_line_endings);
    let diff = similar::TextDiff::from_lines(
        before.as_deref().unwrap_or_default(),
        after.as_deref().unwrap_or_default(),
    );
    let mut added_lines = 0;
    let mut deleted_lines = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added_lines += 1,
            similar::ChangeTag::Delete => deleted_lines += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    Ok(GitFileComparison {
        path,
        stats: GitDiffStats {
            added_lines,
            deleted_lines,
        },
        before: before.map(|content| GitTextVersion { content }),
        after: after.map(|content| GitTextVersion { content }),
        limitation_code: None,
    })
}

fn normalize_line_endings(content: String) -> String {
    if !content.contains('\r') {
        return content;
    }
    let mut normalized = String::with_capacity(content.len());
    let mut characters = content.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn parse_porcelain_v2(bytes: &[u8]) -> Result<GitWorkspaceStatus> {
    let mut offset = 0;
    let mut branch = GitBranchStatus {
        oid: None,
        head: None,
        upstream: None,
        ahead: 0,
        behind: 0,
    };
    while bytes
        .get(offset..)
        .is_some_and(|remaining| remaining.starts_with(b"# "))
    {
        let remaining = &bytes[offset..];
        let line_end = remaining
            .iter()
            .position(|byte| *byte == b'\n' || *byte == 0)
            .unwrap_or(remaining.len());
        parse_branch_header(
            &String::from_utf8_lossy(&remaining[..line_end]),
            &mut branch,
        )?;
        offset += line_end;
        while bytes
            .get(offset)
            .is_some_and(|byte| *byte == b'\n' || *byte == 0)
        {
            offset += 1;
        }
    }

    let records = bytes[offset..]
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8_lossy(record).to_string())
        .collect::<Vec<_>>();
    let mut conflicts = Vec::new();
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = &records[index];
        match record.as_bytes().first().copied() {
            Some(b'1') => {
                let fields = record.splitn(9, ' ').collect::<Vec<_>>();
                ensure!(fields.len() == 9, "invalid porcelain v2 ordinary record");
                classify_change(
                    change_from_xy(fields[8], None, fields[1], fields[2]),
                    &mut conflicts,
                    &mut staged,
                    &mut unstaged,
                );
            }
            Some(b'2') => {
                let fields = record.splitn(10, ' ').collect::<Vec<_>>();
                ensure!(fields.len() == 10, "invalid porcelain v2 rename record");
                let old_path = records.get(index + 1).cloned().ok_or_else(|| {
                    GitServiceError::new("git.status-parse-failed", serde_json::json!({}))
                })?;
                classify_change(
                    change_from_xy(fields[9], Some(old_path), fields[1], fields[2]),
                    &mut conflicts,
                    &mut staged,
                    &mut unstaged,
                );
                index += 1;
            }
            Some(b'u') => {
                let fields = record.splitn(11, ' ').collect::<Vec<_>>();
                ensure!(fields.len() == 11, "invalid porcelain v2 unmerged record");
                conflicts.push(GitFileChange {
                    path: fields[10].to_string(),
                    old_path: None,
                    kind: GitFileChangeKind::Unmerged,
                    index_status: status_char(fields[1].chars().next()),
                    worktree_status: status_char(fields[1].chars().nth(1)),
                    binary: false,
                    submodule: fields[2] != "N...",
                    added_lines: None,
                    deleted_lines: None,
                });
            }
            Some(b'?') => untracked.push(GitFileChange {
                path: record.get(2..).unwrap_or_default().to_string(),
                old_path: None,
                kind: GitFileChangeKind::Untracked,
                index_status: None,
                worktree_status: Some("?".to_string()),
                binary: false,
                submodule: false,
                added_lines: None,
                deleted_lines: None,
            }),
            Some(b'#') => parse_branch_header(record, &mut branch)?,
            Some(b'!') | None => {}
            _ => {
                return Err(GitServiceError::new(
                    "git.status-parse-failed",
                    serde_json::json!({ "recordKind": record.chars().next().map(|value| value.to_string()) }),
                )
                .into());
            }
        }
        index += 1;
    }
    Ok(GitWorkspaceStatus {
        snapshot_revision: String::new(),
        branch,
        conflicts,
        staged,
        unstaged,
        untracked,
        operation_in_progress: None,
    })
}

fn parse_branch_header(line: &str, branch: &mut GitBranchStatus) -> Result<()> {
    if let Some(value) = line.strip_prefix("# branch.oid ") {
        branch.oid = (value != UNBORN_HEAD_SENTINEL).then(|| value.to_string());
    } else if let Some(value) = line.strip_prefix("# branch.head ") {
        branch.head = Some(value.to_string());
    } else if let Some(value) = line.strip_prefix("# branch.upstream ") {
        branch.upstream = Some(value.to_string());
    } else if let Some(value) = line.strip_prefix("# branch.ab ") {
        let mut fields = value.split_whitespace();
        branch.ahead = fields
            .next()
            .and_then(|value| value.strip_prefix('+'))
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| {
                GitServiceError::new("git.status-parse-failed", serde_json::json!({}))
            })?;
        branch.behind = fields
            .next()
            .and_then(|value| value.strip_prefix('-'))
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| {
                GitServiceError::new("git.status-parse-failed", serde_json::json!({}))
            })?;
    }
    Ok(())
}

fn change_from_xy(
    path: &str,
    old_path: Option<String>,
    xy: &str,
    submodule: &str,
) -> GitFileChange {
    let index = xy.chars().next().unwrap_or('.');
    let worktree = xy.chars().nth(1).unwrap_or('.');
    let kind = change_kind(index, worktree);
    GitFileChange {
        path: path.to_string(),
        old_path,
        kind,
        index_status: status_char(Some(index)),
        worktree_status: status_char(Some(worktree)),
        binary: false,
        submodule: submodule != "N...",
        added_lines: None,
        deleted_lines: None,
    }
}

fn change_kind(index: char, worktree: char) -> GitFileChangeKind {
    for value in [index, worktree] {
        match value {
            'R' => return GitFileChangeKind::Renamed,
            'C' => return GitFileChangeKind::Copied,
            'A' => return GitFileChangeKind::Added,
            'D' => return GitFileChangeKind::Deleted,
            'T' => return GitFileChangeKind::TypeChanged,
            _ => {}
        }
    }
    GitFileChangeKind::Modified
}

fn status_char(value: Option<char>) -> Option<String> {
    value
        .filter(|value| *value != '.')
        .map(|value| value.to_string())
}

fn classify_change(
    change: GitFileChange,
    conflicts: &mut Vec<GitFileChange>,
    staged: &mut Vec<GitFileChange>,
    unstaged: &mut Vec<GitFileChange>,
) {
    let xy = format!(
        "{}{}",
        change.index_status.as_deref().unwrap_or("."),
        change.worktree_status.as_deref().unwrap_or(".")
    );
    if matches!(xy.as_str(), "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU") {
        conflicts.push(GitFileChange {
            kind: GitFileChangeKind::Unmerged,
            ..change
        });
        return;
    }
    if change.index_status.is_some() {
        staged.push(change.clone());
    }
    if change.worktree_status.is_some() {
        unstaged.push(change);
    }
}

fn parse_refs(bytes: &[u8]) -> Result<Vec<GitRef>> {
    let mut refs = Vec::new();
    for record in bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
    {
        let fields = record.split(|byte| *byte == 0).collect::<Vec<_>>();
        ensure!(fields.len() >= 5, "invalid git ref record");
        let full_name = text(fields[0]);
        let kind = if full_name.starts_with("refs/heads/") {
            GitRefKind::LocalBranch
        } else if full_name.starts_with("refs/remotes/") {
            GitRefKind::RemoteBranch
        } else if full_name.starts_with("refs/tags/") {
            GitRefKind::Tag
        } else {
            continue;
        };
        refs.push(GitRef {
            full_name,
            short_name: text(fields[1]),
            kind,
            target_oid: text(fields[2]),
            peeled_oid: non_empty_text(fields[3]),
            upstream: non_empty_text(fields[4]),
            ahead: None,
            behind: None,
            checked_out_worktree_paths: Vec::new(),
        });
    }
    Ok(refs)
}

fn parse_worktrees(bytes: &[u8]) -> Result<Vec<GitWorktree>> {
    let fields = bytes
        .split(|byte| *byte == 0)
        .map(text)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut records = Vec::<BTreeMap<String, Option<String>>>::new();
    let mut current = BTreeMap::new();
    for field in fields {
        if field.starts_with("worktree ") && !current.is_empty() {
            records.push(std::mem::take(&mut current));
        }
        let (key, value) = field
            .split_once(' ')
            .map(|(key, value)| (key.to_string(), Some(value.to_string())))
            .unwrap_or_else(|| (field, None));
        current.insert(key, value);
    }
    if !current.is_empty() {
        records.push(current);
    }
    records
        .into_iter()
        .enumerate()
        .map(|(index, record)| {
            let path = record
                .get("worktree")
                .and_then(Clone::clone)
                .ok_or_else(|| {
                    GitServiceError::new("git.worktree-parse-failed", serde_json::json!({}))
                })?;
            let branch = record.get("branch").and_then(Clone::clone);
            let ownership = if branch.as_deref().is_some_and(is_runtime_branch) {
                GitWorktreeOwnership::Runtime
            } else {
                GitWorktreeOwnership::User
            };
            Ok(GitWorktree {
                path: Utf8PathBuf::from(path),
                head_oid: record
                    .get("HEAD")
                    .and_then(Clone::clone)
                    .unwrap_or_default(),
                branch,
                main: index == 0,
                detached: record.contains_key("detached"),
                locked: record.contains_key("locked"),
                lock_reason: record.get("locked").and_then(Clone::clone),
                prunable: record.contains_key("prunable"),
                ownership,
                runtime_status: None,
            })
        })
        .collect()
}

fn parse_stashes(bytes: &[u8]) -> Result<Vec<GitStashEntry>> {
    let mut entries = Vec::new();
    let fields = nul_fields(bytes);
    ensure!(fields.len().is_multiple_of(7), "invalid git stash record");
    for fields in fields.chunks_exact(7) {
        let parents = text(&fields[2]);
        let created_at = text(&fields[6]).trim().to_string();
        entries.push(GitStashEntry {
            ref_name: text(&fields[0]).trim().to_string(),
            oid: text(&fields[1]),
            base_oid: parents
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string(),
            message: text(&fields[3]),
            author: GitSignature {
                name: text(&fields[4]),
                email: non_empty_text(&fields[5]),
                timestamp: created_at.clone(),
            },
            created_at,
        });
    }
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitFileStatusRecord {
    path: String,
    old_path: Option<String>,
    kind: GitFileChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitFileStats {
    binary: bool,
    added_lines: Option<u64>,
    deleted_lines: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitReviewPatch {
    before_oid: Option<String>,
    after_oid: String,
    files: Vec<GitCommitFileChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitReviewFilePatch {
    before_oid: Option<String>,
    after_oid: String,
    change: GitCommitFileChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitReviewFileChain {
    // Newest to oldest. This matches the history selection order and lets an equivalent
    // side-branch patch defer to the chain that can continue toward the newest selection.
    patches: Vec<CommitReviewFilePatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CommitReviewFilePatchIdentity {
    before_oid: Option<String>,
    after_oid: String,
    path: String,
    old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CommitReviewFilePatchSignature {
    path: String,
    old_path: Option<String>,
    kind: GitFileChangeKind,
    added_lines: Option<u64>,
    deleted_lines: Option<u64>,
}

impl From<&CommitReviewFilePatch> for CommitReviewFilePatchSignature {
    fn from(patch: &CommitReviewFilePatch) -> Self {
        Self {
            path: patch.change.path.clone(),
            old_path: patch.change.old_path.clone(),
            kind: patch.change.kind,
            added_lines: patch.change.added_lines,
            deleted_lines: patch.change.deleted_lines,
        }
    }
}

impl From<&CommitReviewFilePatch> for CommitReviewFilePatchIdentity {
    fn from(patch: &CommitReviewFilePatch) -> Self {
        Self {
            before_oid: patch.before_oid.clone(),
            after_oid: patch.after_oid.clone(),
            path: patch.change.path.clone(),
            old_path: patch.change.old_path.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AggregatedCommitReviewFile {
    before_oid: Option<String>,
    before_path: Option<String>,
    after_oid: String,
    path: String,
    after_exists: bool,
    binary: bool,
}

fn file_patch_before_path(patch: &CommitReviewFilePatch) -> &str {
    patch
        .change
        .old_path
        .as_deref()
        .unwrap_or(patch.change.path.as_str())
}

fn file_chain_before_path(chain: &CommitReviewFileChain) -> &str {
    file_patch_before_path(
        chain
            .patches
            .last()
            .expect("review file chain is not empty"),
    )
}

fn aggregate_commit_review_files(
    patches_oldest_first: impl IntoIterator<Item = CommitReviewPatch>,
) -> Vec<GitCommitReviewFile> {
    let mut files = BTreeMap::<String, AggregatedCommitReviewFile>::new();
    for patch in patches_oldest_first {
        for change in patch.files {
            let source_path = change.old_path.as_deref().unwrap_or(change.path.as_str());
            let after_exists = change.kind != GitFileChangeKind::Deleted;
            let mut aggregate = if change.kind == GitFileChangeKind::Copied {
                None
            } else {
                files.remove(source_path)
            }
            .unwrap_or_else(|| AggregatedCommitReviewFile {
                before_oid: (change.kind != GitFileChangeKind::Added)
                    .then(|| patch.before_oid.clone())
                    .flatten(),
                before_path: (change.kind != GitFileChangeKind::Added)
                    .then(|| source_path.to_string()),
                after_oid: patch.after_oid.clone(),
                path: change.path.clone(),
                after_exists,
                binary: change.binary,
            });

            aggregate.after_oid.clone_from(&patch.after_oid);
            aggregate.path.clone_from(&change.path);
            aggregate.after_exists = after_exists;
            aggregate.binary |= change.binary;

            // A file created and then deleted inside the selected changes has no net endpoint.
            if aggregate.before_oid.is_none() && !aggregate.after_exists {
                continue;
            }
            files.insert(aggregate.path.clone(), aggregate);
        }
    }

    files
        .into_values()
        .map(|file| {
            let kind = match (file.before_oid.is_some(), file.after_exists) {
                (false, true) => GitFileChangeKind::Added,
                (true, false) => GitFileChangeKind::Deleted,
                (true, true) if file.before_path.as_deref() != Some(file.path.as_str()) => {
                    GitFileChangeKind::Renamed
                }
                _ => GitFileChangeKind::Modified,
            };
            let old_path = (file.before_path.as_deref() != Some(file.path.as_str()))
                .then(|| file.before_path.clone())
                .flatten();
            GitCommitReviewFile {
                old_path,
                path: file.path,
                kind,
                binary: file.binary,
                before_oid: file.before_oid,
                before_path: file.before_path,
                after_oid: file.after_oid,
                added_lines: None,
                deleted_lines: None,
            }
        })
        .collect()
}

fn parse_commit_name_status(bytes: &[u8]) -> Result<Vec<CommitFileStatusRecord>> {
    let fields = nul_fields(bytes);
    let mut index = 0;
    let mut changes = Vec::new();
    while index < fields.len() {
        let status = text(&fields[index]);
        index += 1;
        let status_code = status.chars().next().ok_or_else(|| {
            GitServiceError::new("git.commit-diff-parse-failed", serde_json::json!({}))
        })?;
        let kind = match status_code {
            'A' => GitFileChangeKind::Added,
            'M' => GitFileChangeKind::Modified,
            'D' => GitFileChangeKind::Deleted,
            'R' => GitFileChangeKind::Renamed,
            'C' => GitFileChangeKind::Copied,
            'T' => GitFileChangeKind::TypeChanged,
            _ => {
                return Err(GitServiceError::new(
                    "git.commit-diff-parse-failed",
                    serde_json::json!({ "status": status }),
                )
                .into());
            }
        };
        let (old_path, path) =
            if matches!(kind, GitFileChangeKind::Renamed | GitFileChangeKind::Copied) {
                if index + 1 >= fields.len() {
                    return Err(GitServiceError::new(
                        "git.commit-diff-parse-failed",
                        serde_json::json!({ "status": status }),
                    )
                    .into());
                }
                let old_path = text(&fields[index]);
                let path = text(&fields[index + 1]);
                index += 2;
                (Some(old_path), path)
            } else {
                let path = fields.get(index).ok_or_else(|| {
                    GitServiceError::new(
                        "git.commit-diff-parse-failed",
                        serde_json::json!({ "status": status }),
                    )
                })?;
                index += 1;
                (None, text(path))
            };
        changes.push(CommitFileStatusRecord {
            path,
            old_path,
            kind,
        });
    }
    Ok(changes)
}

fn parse_commit_numstat(bytes: &[u8]) -> Result<HashMap<String, CommitFileStats>> {
    parse_numstat(bytes, "git.commit-diff-parse-failed")
}

fn untracked_file_stats(path: &Utf8Path) -> Result<CommitFileStats> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open untracked file `{path}` for Git metrics"))?;
    let mut buffer = [0_u8; 8192];
    let mut added_lines = 0_u64;
    let mut any = false;
    let mut last = 0_u8;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if chunk.contains(&0) {
            return Ok(CommitFileStats {
                binary: true,
                added_lines: None,
                deleted_lines: None,
            });
        }
        added_lines =
            added_lines.saturating_add(chunk.iter().filter(|byte| **byte == b'\n').count() as u64);
        any = true;
        last = chunk[read - 1];
    }
    if any && last != b'\n' {
        added_lines = added_lines.saturating_add(1);
    }
    Ok(CommitFileStats {
        binary: false,
        added_lines: Some(added_lines),
        deleted_lines: Some(0),
    })
}

fn parse_numstat(
    bytes: &[u8],
    error_code: &'static str,
) -> Result<HashMap<String, CommitFileStats>> {
    let fields = nul_fields(bytes);
    let mut index = 0;
    let mut stats = HashMap::new();
    while index < fields.len() {
        let header = text(&fields[index]);
        index += 1;
        let mut header_fields = header.splitn(3, '\t');
        let added = header_fields.next().unwrap_or_default();
        let deleted = header_fields.next().unwrap_or_default();
        let inline_path = header_fields
            .next()
            .ok_or_else(|| GitServiceError::new(error_code, serde_json::json!({})))?;
        let path = if inline_path.is_empty() {
            if index + 1 >= fields.len() {
                return Err(GitServiceError::new(error_code, serde_json::json!({})).into());
            }
            let path = text(&fields[index + 1]);
            index += 2;
            path
        } else {
            inline_path.to_string()
        };
        let binary = added == "-" && deleted == "-";
        let parse_count = |value: &str| -> Result<Option<u64>> {
            if value == "-" {
                Ok(None)
            } else {
                value.parse::<u64>().map(Some).map_err(|_| {
                    GitServiceError::new(error_code, serde_json::json!({ "value": value })).into()
                })
            }
        };
        stats.insert(
            path,
            CommitFileStats {
                binary,
                added_lines: parse_count(added)?,
                deleted_lines: parse_count(deleted)?,
            },
        );
    }
    Ok(stats)
}

fn apply_workspace_stats(changes: &mut [GitFileChange], stats: &HashMap<String, CommitFileStats>) {
    for change in changes {
        let Some(file_stats) = stats.get(&change.path) else {
            continue;
        };
        change.binary = file_stats.binary;
        change.added_lines = file_stats.added_lines;
        change.deleted_lines = file_stats.deleted_lines;
    }
}

fn merge_commit_file_changes(
    records: Vec<CommitFileStatusRecord>,
    mut stats: HashMap<String, CommitFileStats>,
) -> Result<Vec<GitCommitFileChange>> {
    records
        .into_iter()
        .map(|record| {
            let file_stats = stats.remove(&record.path).unwrap_or(CommitFileStats {
                binary: false,
                added_lines: None,
                deleted_lines: None,
            });
            Ok(GitCommitFileChange {
                path: record.path,
                old_path: record.old_path,
                kind: record.kind,
                binary: file_stats.binary,
                added_lines: file_stats.added_lines,
                deleted_lines: file_stats.deleted_lines,
            })
        })
        .collect()
}

fn parse_history(bytes: &[u8], refs: &HashMap<String, Vec<GitRefLabel>>) -> Result<Vec<GitCommit>> {
    let mut commits = Vec::new();
    let fields = nul_fields(bytes);
    ensure!(
        fields.len().is_multiple_of(11),
        "invalid git history record"
    );
    for fields in fields.chunks_exact(11) {
        let oid = text(&fields[0]).trim().to_string();
        let body = text(&fields[3]);
        commits.push(GitCommit {
            parent_oids: text(&fields[1])
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            subject: text(&fields[2]),
            runtime_checkpoint: body.contains("Gold-Band-Internal: checkpoint"),
            body,
            author: GitSignature {
                name: text(&fields[4]),
                email: non_empty_text(&fields[5]),
                timestamp: text(&fields[6]),
            },
            committer: GitSignature {
                name: text(&fields[7]),
                email: non_empty_text(&fields[8]),
                timestamp: text(&fields[9]),
            },
            refs: refs.get(&oid).cloned().unwrap_or_default(),
            source_ref: non_empty_text(&fields[10]),
            oid,
        });
    }
    Ok(commits)
}

fn refs_by_oid(refs: &[GitRef]) -> HashMap<String, Vec<GitRefLabel>> {
    let mut result = HashMap::<String, Vec<GitRefLabel>>::new();
    for git_ref in refs {
        let oid = git_ref
            .peeled_oid
            .as_ref()
            .unwrap_or(&git_ref.target_oid)
            .clone();
        result.entry(oid).or_default().push(GitRefLabel {
            full_name: git_ref.full_name.clone(),
            short_name: git_ref.short_name.clone(),
            kind: git_ref.kind,
        });
    }
    result
}

fn repository_revision(branch: &GitBranchStatus, refs: &[GitRef]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(branch.oid.as_deref().unwrap_or_default().as_bytes());
    hasher.update(branch.head.as_deref().unwrap_or_default().as_bytes());
    hasher.update(branch.upstream.as_deref().unwrap_or_default().as_bytes());
    hasher.update(&branch.ahead.to_le_bytes());
    hasher.update(&branch.behind.to_le_bytes());
    for git_ref in refs {
        hasher.update(git_ref.full_name.as_bytes());
        hasher.update(git_ref.target_oid.as_bytes());
        hasher.update(git_ref.peeled_oid.as_deref().unwrap_or_default().as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn workspace_snapshot_revision(status: &GitWorkspaceStatus, refs: &[GitRef]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(repository_revision(&status.branch, refs).as_bytes());
    for (area, changes) in [
        ("conflicts", &status.conflicts),
        ("staged", &status.staged),
        ("unstaged", &status.unstaged),
        ("untracked", &status.untracked),
    ] {
        hasher.update(area.as_bytes());
        for change in changes {
            hasher.update(change.path.as_bytes());
            hasher.update(change.old_path.as_deref().unwrap_or_default().as_bytes());
            hasher.update(
                change
                    .index_status
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            );
            hasher.update(
                change
                    .worktree_status
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            );
        }
    }
    if let Some(operation) = &status.operation_in_progress {
        hasher.update(match operation.kind {
            GitInProgressOperationKind::Merge => b"merge".as_slice(),
            GitInProgressOperationKind::Rebase => b"rebase".as_slice(),
            GitInProgressOperationKind::CherryPick => b"cherry-pick".as_slice(),
            GitInProgressOperationKind::Revert => b"revert".as_slice(),
        });
    }
    hasher.finalize().to_hex().to_string()
}

fn combined_lock_snapshot(identity: &GitRepositoryIdentity) -> GitLockSnapshot {
    let coordination = GitCoordinationService;
    let workspace = coordination.workspace_lock(&identity.workspace_path);
    if workspace.locked {
        workspace
    } else {
        coordination.repository_lock(&identity.common_dir)
    }
}

fn is_runtime_branch(branch: &str) -> bool {
    branch.starts_with("refs/heads/gb-dyn-")
        || branch.starts_with("refs/heads/gb-dyn/")
        || branch.starts_with("refs/heads/gb-runtime/")
        || branch.starts_with("refs/heads/codex/")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

fn non_empty_text(bytes: &[u8]) -> Option<String> {
    let value = text(bytes).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn nul_fields(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .map(|field| field.strip_prefix(b"\n").unwrap_or(field).to_vec())
        .collect::<Vec<_>>();
    while fields.last().is_some_and(Vec::is_empty) {
        fields.pop();
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitCommandRunner;

    fn initialized_repository() -> (tempfile::TempDir, Utf8PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let runner = GitCommandRunner;
        assert!(runner.run(&root, &["init"]).unwrap().success);
        assert!(
            runner
                .run(&root, &["config", "user.name", "Gold Band Test"])
                .unwrap()
                .success
        );
        assert!(
            runner
                .run(&root, &["config", "user.email", "test@gold-band.local"])
                .unwrap()
                .success
        );
        (temp, root)
    }

    fn commit_file(root: &Utf8Path, path: &str, content: &str, message: &str) -> String {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&target, content).unwrap();
        let runner = GitCommandRunner;
        assert!(runner.run(root, &["add", "--", path]).unwrap().success);
        assert!(
            runner
                .run(root, &["commit", "-m", message])
                .unwrap()
                .success
        );
        runner.run(root, &["rev-parse", "HEAD"]).unwrap().stdout
    }

    fn wait_operation(service: &GitSourceControlService, operation_id: &str) -> GitOperation {
        for _ in 0..250 {
            let operation = service.get_operation(operation_id).unwrap();
            if !matches!(
                operation.status,
                GitOperationStatus::Queued | GitOperationStatus::Running
            ) {
                return operation;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("Git operation {operation_id} did not finish");
    }

    #[test]
    fn porcelain_v2_preserves_spaces_newlines_and_rename_paths() {
        let bytes = b"# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n1 M. N... 100644 100644 100644 abc def file with spaces.txt\0\
2 R. N... 100644 100644 100644 abc def R100 renamed.txt\0old\nname.txt\0? new file.txt\0";
        let status = parse_porcelain_v2(bytes).unwrap();
        assert_eq!(status.branch.oid.as_deref(), Some("abc"));
        assert_eq!(status.branch.head.as_deref(), Some("main"));
        assert_eq!(status.branch.ahead, 2);
        assert_eq!(status.branch.behind, 1);
        assert_eq!(status.staged.len(), 2);
        assert_eq!(status.staged[0].path, "file with spaces.txt");
        assert_eq!(status.staged[1].old_path.as_deref(), Some("old\nname.txt"));
        assert_eq!(status.untracked[0].path, "new file.txt");
    }

    #[test]
    fn porcelain_v2_normalizes_initial_oid_to_unborn() {
        let status = parse_porcelain_v2(b"# branch.oid (initial)\n# branch.head main\0").unwrap();

        assert_eq!(status.branch.oid, None);
        assert_eq!(status.branch.head.as_deref(), Some("main"));
    }

    #[test]
    fn source_control_snapshot_reads_real_repository_state() {
        let (_temp, root) = initialized_repository();
        let first = commit_file(&root, "tracked.txt", "one\n", "first");
        std::fs::write(root.join("tracked.txt"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("untracked file.txt"), "new\n").unwrap();

        let snapshot = GitSourceControlService::default()
            .snapshot("project-test", &root)
            .unwrap();

        assert_eq!(snapshot.repository.project_id, "project-test");
        assert_eq!(
            snapshot.repository.head_oid.as_deref(),
            Some(first.as_str())
        );
        assert_eq!(snapshot.status.unstaged.len(), 1);
        assert_eq!(snapshot.status.untracked[0].path, "untracked file.txt");
        assert!(!snapshot.refs.is_empty());
        assert_eq!(snapshot.worktrees.len(), 1);
    }

    #[test]
    fn unborn_repository_has_an_empty_history_page_instead_of_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        assert!(GitCommandRunner.run(&root, &["init"]).unwrap().success);
        std::fs::write(root.join("untracked.txt"), "not committed\n").unwrap();
        let service = GitSourceControlService::default();

        let snapshot = service.snapshot("project-unborn", &root).unwrap();
        assert!(snapshot.repository.unborn);
        assert!(snapshot.repository.head_oid.is_none());
        assert!(snapshot.status.branch.oid.is_none());
        assert_eq!(snapshot.status.untracked.len(), 1);
        assert_eq!(snapshot.status.untracked[0].path, "untracked.txt");

        let page = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(300),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap();

        assert!(page.commits.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn metadata_watch_targets_cover_worktree_and_shared_refs() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "tracked.txt", "one\n", "base");
        let targets = GitSourceControlService::default()
            .metadata_watch_targets(&root)
            .unwrap();

        assert!(!targets.is_empty());
        assert!(targets.iter().all(|target| target.path.is_dir()));
        assert!(targets.iter().any(|target| target.recursive));
    }

    #[test]
    fn merge_continue_stages_only_unmerged_paths_and_completes_the_merge() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "conflict.txt", "base\n", "base");
        let runner = GitCommandRunner;
        let base_branch = runner
            .run(&root, &["branch", "--show-current"])
            .unwrap()
            .stdout;
        assert!(
            runner
                .run(&root, &["checkout", "-b", "topic"])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "topic\n", "topic change");
        assert!(
            runner
                .run(&root, &["checkout", &base_branch])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "main\n", "main change");
        assert!(!runner.run(&root, &["merge", "topic"]).unwrap().success);
        std::fs::write(root.join("conflict.txt"), "resolved\n").unwrap();
        std::fs::write(root.join("ordinary.txt"), "do not stage\n").unwrap();

        let service = GitSourceControlService::default();
        let snapshot = service.snapshot("project-test", &root).unwrap();
        assert_eq!(
            snapshot
                .status
                .operation_in_progress
                .as_ref()
                .map(|value| value.kind),
            Some(GitInProgressOperationKind::Merge)
        );
        let started = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: Some(snapshot.repository.revision),
                    operation: GitOperationInput::MergeContinue,
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &started.operation_id).status,
            GitOperationStatus::Succeeded
        );

        let completed = service.snapshot("project-test", &root).unwrap();
        assert!(completed.status.operation_in_progress.is_none());
        assert!(
            completed
                .status
                .untracked
                .iter()
                .any(|change| change.path == "ordinary.txt")
        );
    }

    #[test]
    fn merge_abort_restores_the_pre_merge_state() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "conflict.txt", "base\n", "base");
        let runner = GitCommandRunner;
        let base_branch = runner
            .run(&root, &["branch", "--show-current"])
            .unwrap()
            .stdout;
        assert!(
            runner
                .run(&root, &["checkout", "-b", "topic"])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "topic\n", "topic");
        assert!(
            runner
                .run(&root, &["checkout", &base_branch])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "main\n", "main");
        assert!(!runner.run(&root, &["merge", "topic"]).unwrap().success);
        let service = GitSourceControlService::default();
        let snapshot = service.snapshot("project-test", &root).unwrap();
        let started = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: Some(snapshot.repository.revision),
                    operation: GitOperationInput::MergeAbort,
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &started.operation_id).status,
            GitOperationStatus::Succeeded
        );
        assert!(
            service
                .snapshot("project-test", &root)
                .unwrap()
                .status
                .operation_in_progress
                .is_none()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("conflict.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "main\n"
        );
    }

    #[test]
    fn rebase_continue_reports_current_commit_and_preserves_ordinary_changes() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "conflict.txt", "base\n", "base");
        let runner = GitCommandRunner;
        let base_branch = runner
            .run(&root, &["branch", "--show-current"])
            .unwrap()
            .stdout;
        assert!(
            runner
                .run(&root, &["checkout", "-b", "topic"])
                .unwrap()
                .success
        );
        let topic_oid = commit_file(&root, "conflict.txt", "topic\n", "topic conflict");
        assert!(
            runner
                .run(&root, &["checkout", &base_branch])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "upstream\n", "upstream conflict");
        assert!(runner.run(&root, &["checkout", "topic"]).unwrap().success);
        assert!(
            !runner
                .run(&root, &["rebase", &base_branch])
                .unwrap()
                .success
        );
        std::fs::write(root.join("conflict.txt"), "resolved\n").unwrap();
        std::fs::write(root.join("ordinary.txt"), "do not stage\n").unwrap();

        let service = GitSourceControlService::default();
        let snapshot = service.snapshot("project-test", &root).unwrap();
        let progress = snapshot.status.operation_in_progress.as_ref().unwrap();
        assert_eq!(progress.kind, GitInProgressOperationKind::Rebase);
        assert_eq!(progress.current_oid.as_deref(), Some(topic_oid.as_str()));
        assert_eq!(progress.current_subject.as_deref(), Some("topic conflict"));
        let started = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: Some(snapshot.repository.revision),
                    operation: GitOperationInput::RebaseContinue,
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &started.operation_id).status,
            GitOperationStatus::Succeeded
        );
        let completed = service.snapshot("project-test", &root).unwrap();
        assert!(completed.status.operation_in_progress.is_none());
        assert!(
            completed
                .status
                .untracked
                .iter()
                .any(|change| change.path == "ordinary.txt")
        );
    }

    #[test]
    fn rebase_skip_discards_the_current_commit_and_continues() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "conflict.txt", "base\n", "base");
        let runner = GitCommandRunner;
        let base_branch = runner
            .run(&root, &["branch", "--show-current"])
            .unwrap()
            .stdout;
        assert!(
            runner
                .run(&root, &["checkout", "-b", "topic"])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "topic\n", "discard me");
        commit_file(&root, "after.txt", "after\n", "keep me");
        assert!(
            runner
                .run(&root, &["checkout", &base_branch])
                .unwrap()
                .success
        );
        commit_file(&root, "conflict.txt", "upstream\n", "upstream");
        assert!(runner.run(&root, &["checkout", "topic"]).unwrap().success);
        assert!(
            !runner
                .run(&root, &["rebase", &base_branch])
                .unwrap()
                .success
        );

        let service = GitSourceControlService::default();
        let snapshot = service.snapshot("project-test", &root).unwrap();
        let started = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: Some(snapshot.repository.revision),
                    operation: GitOperationInput::RebaseSkip,
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &started.operation_id).status,
            GitOperationStatus::Succeeded
        );
        let completed = service.snapshot("project-test", &root).unwrap();
        assert!(completed.status.operation_in_progress.is_none());
        assert_eq!(
            std::fs::read_to_string(root.join("conflict.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "upstream\n"
        );
        assert!(root.join("after.txt").exists());
    }

    #[test]
    fn managed_operation_emits_running_and_terminal_updates() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "tracked.txt", "one\n", "base");
        std::fs::write(root.join("tracked.txt"), "one\ntwo\n").unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let sink: GitOperationUpdateSink = Arc::new(move |operation| {
            let _ = sender.send(operation);
        });
        let service = GitSourceControlService::default();
        let started = service
            .start_operation_with_update_sink(
                &root,
                &GitOperationRequest {
                    expected_revision: None,
                    operation: GitOperationInput::StashCreate {
                        message: Some("event test".to_string()),
                        include_untracked: false,
                    },
                },
                Some(sink),
            )
            .unwrap();

        let mut updates = Vec::new();
        while let Ok(update) = receiver.recv_timeout(Duration::from_secs(5)) {
            let terminal = !matches!(
                update.status,
                GitOperationStatus::Queued | GitOperationStatus::Running
            );
            updates.push(update);
            if terminal {
                break;
            }
        }
        assert!(
            updates
                .iter()
                .any(|update| update.status == GitOperationStatus::Running)
        );
        assert_eq!(
            updates.last().map(|update| update.operation_id.as_str()),
            Some(started.operation_id.as_str())
        );
        assert_eq!(
            updates.last().map(|update| update.status),
            Some(GitOperationStatus::Succeeded)
        );
    }

    #[test]
    fn history_returns_real_parent_dag_and_runtime_checkpoint_marker() {
        let (_temp, root) = initialized_repository();
        let first = commit_file(&root, "one.txt", "one\n", "first");
        let second = commit_file(
            &root,
            "two.txt",
            "two\n",
            "checkpoint\n\nGold-Band-Internal: checkpoint",
        );
        let page = GitSourceControlService::default()
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(10),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap();
        assert_eq!(page.commits.len(), 2);
        assert_eq!(page.commits[0].oid, second);
        assert_eq!(page.commits[0].parent_oids, vec![first]);
        assert!(page.commits[0].runtime_checkpoint);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn commit_detail_and_range_files_use_typed_name_status_and_numstat() {
        let (_temp, root) = initialized_repository();
        let first = commit_file(&root, "old name.txt", "one\n", "first");
        let runner = GitCommandRunner;
        assert!(
            runner
                .run(&root, &["mv", "old name.txt", "new name.txt"])
                .unwrap()
                .success
        );
        std::fs::write(root.join("new name.txt"), "one\ntwo\n").unwrap();
        assert!(runner.run(&root, &["add", "-A"]).unwrap().success);
        assert!(
            runner
                .run(&root, &["commit", "-m", "rename"])
                .unwrap()
                .success
        );
        let second = runner.run(&root, &["rev-parse", "HEAD"]).unwrap().stdout;
        let service = GitSourceControlService::default();

        let detail = service.commit_detail(&root, &second).unwrap();

        assert_eq!(detail.commit.oid, second);
        assert_eq!(detail.commit.parent_oids, vec![first]);
        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.files[0].kind, GitFileChangeKind::Renamed);
        assert_eq!(detail.files[0].old_path.as_deref(), Some("old name.txt"));
        assert_eq!(detail.files[0].path, "new name.txt");
        assert_eq!(detail.files[0].added_lines, Some(1));
        assert_eq!(detail.files[0].deleted_lines, Some(0));
    }

    #[test]
    fn commit_review_aggregates_duplicate_paths_into_one_endpoint_diff() {
        let (_temp, root) = initialized_repository();
        let first = commit_file(&root, "shared.txt", "one\n", "first");
        let second = commit_file(&root, "shared.txt", "one\ntwo\n", "second");
        let service = GitSourceControlService::default();
        let revision = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(10),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap()
            .revision;

        let review = service
            .commit_review(
                &root,
                &GitCommitReviewQuery {
                    // The UI sends selected OIDs in the same newest-to-oldest order as history.
                    selected_oids: vec![second.clone(), first.clone(), second.clone()],
                    revision: Some(revision),
                },
            )
            .unwrap();

        assert_eq!(review.selected_oids, vec![second.clone(), first]);
        assert_eq!(review.files.len(), 1);
        assert_eq!(review.files[0].before_oid, None);
        assert_eq!(review.files[0].after_oid, second);
        assert_eq!(review.files[0].path, "shared.txt");
        assert_eq!(review.files[0].kind, GitFileChangeKind::Added);
        assert_eq!(review.files[0].added_lines, Some(2));
        assert_eq!(review.files[0].deleted_lines, Some(0));
        assert_eq!(review.totals.commit_count, 2);
        assert_eq!(review.totals.file_count, 1);
    }

    #[test]
    fn commit_review_deduplicates_equivalent_side_branch_patch_into_connected_chain() {
        let (_temp, root) = initialized_repository();
        let base = commit_file(&root, "shared.txt", "target=old\n", "base");
        let runner = GitCommandRunner;
        assert!(
            runner
                .run(&root, &["branch", "equivalent-side", base.as_str()])
                .unwrap()
                .success
        );
        let main_baseline = commit_file(
            &root,
            "shared.txt",
            "target=old\nmain baseline one\nmain baseline two\n",
            "main baseline",
        );
        let main_equivalent = commit_file(
            &root,
            "shared.txt",
            "target=new\nselected change\nmain baseline one\nmain baseline two\n",
            "equivalent change",
        );
        let latest = commit_file(
            &root,
            "shared.txt",
            "target=new\nselected change\nfollow up\nmain baseline one\nmain baseline two\n",
            "follow up",
        );
        assert!(
            runner
                .run(&root, &["switch", "equivalent-side"])
                .unwrap()
                .success
        );
        let side_equivalent = commit_file(
            &root,
            "shared.txt",
            "target=new\nselected change\n",
            "equivalent change",
        );
        assert!(runner.run(&root, &["switch", "-"]).unwrap().success);

        let service = GitSourceControlService::default();
        let revision = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(20),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap()
            .revision;
        let review = service
            .commit_review(
                &root,
                &GitCommitReviewQuery {
                    // Equal timestamps can place the side-branch equivalent before the mainline
                    // equivalent in the visible list. Deduplication must not depend on that order.
                    selected_oids: vec![latest.clone(), side_equivalent, main_equivalent],
                    revision: Some(revision),
                },
            )
            .unwrap();

        assert_eq!(review.files.len(), 1);
        assert_eq!(review.files[0].path, "shared.txt");
        assert_eq!(
            review.files[0].before_oid.as_deref(),
            Some(main_baseline.as_str())
        );
        assert_eq!(review.files[0].after_oid, latest);
        let comparison = service
            .comparison(
                &root,
                &GitComparisonSource::Commit {
                    workspace_path: None,
                    path: review.files[0].path.clone(),
                    before_oid: review.files[0].before_oid.clone(),
                    before_path: review.files[0].before_path.clone(),
                    after_oid: review.files[0].after_oid.clone(),
                },
            )
            .unwrap();
        assert_eq!(comparison.stats.added_lines, 3);
        assert_eq!(comparison.stats.deleted_lines, 1);
        assert_eq!(review.files[0].added_lines, Some(3));
        assert_eq!(review.files[0].deleted_lines, Some(1));
    }

    #[test]
    fn commit_review_keeps_distinct_side_branch_file_patches_as_separate_chains() {
        let (_temp, root) = initialized_repository();
        let base = commit_file(&root, "shared.txt", "target=old\n", "base");
        let runner = GitCommandRunner;
        assert!(
            runner
                .run(&root, &["branch", "distinct-side", base.as_str()])
                .unwrap()
                .success
        );
        let main_change = commit_file(&root, "shared.txt", "target=main\n", "main change");
        assert!(
            runner
                .run(&root, &["switch", "distinct-side"])
                .unwrap()
                .success
        );
        let side_change = commit_file(&root, "shared.txt", "target=side\n", "side change");

        let service = GitSourceControlService::default();
        let revision = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(20),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap()
            .revision;
        let review = service
            .commit_review(
                &root,
                &GitCommitReviewQuery {
                    selected_oids: vec![side_change.clone(), main_change.clone()],
                    revision: Some(revision),
                },
            )
            .unwrap();

        assert_eq!(review.files.len(), 2);
        assert!(review.files.iter().all(|file| file.path == "shared.txt"));
        assert_eq!(
            review
                .files
                .iter()
                .map(|file| file.after_oid.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from([main_change.as_str(), side_change.as_str()])
        );
        for file in review.files {
            let comparison = service
                .comparison(
                    &root,
                    &GitComparisonSource::Commit {
                        workspace_path: None,
                        path: file.path,
                        before_oid: file.before_oid,
                        before_path: file.before_path,
                        after_oid: file.after_oid,
                    },
                )
                .unwrap();
            assert_eq!(comparison.stats.added_lines, 1);
            assert_eq!(comparison.stats.deleted_lines, 1);
        }
    }

    #[test]
    fn aggregate_commit_review_files_removes_created_then_deleted_files() {
        let files = aggregate_commit_review_files([
            CommitReviewPatch {
                before_oid: Some("parent".to_string()),
                after_oid: "created".to_string(),
                files: vec![GitCommitFileChange {
                    path: "temporary.txt".to_string(),
                    old_path: None,
                    kind: GitFileChangeKind::Added,
                    binary: false,
                    added_lines: Some(1),
                    deleted_lines: Some(0),
                }],
            },
            CommitReviewPatch {
                before_oid: Some("created".to_string()),
                after_oid: "deleted".to_string(),
                files: vec![GitCommitFileChange {
                    path: "temporary.txt".to_string(),
                    old_path: None,
                    kind: GitFileChangeKind::Deleted,
                    binary: false,
                    added_lines: Some(0),
                    deleted_lines: Some(1),
                }],
            },
        ]);

        assert!(files.is_empty());
    }

    #[test]
    fn aggregate_commit_review_files_follows_a_rename_chain() {
        let files = aggregate_commit_review_files([
            CommitReviewPatch {
                before_oid: Some("parent".to_string()),
                after_oid: "rename-one".to_string(),
                files: vec![GitCommitFileChange {
                    path: "middle.txt".to_string(),
                    old_path: Some("original.txt".to_string()),
                    kind: GitFileChangeKind::Renamed,
                    binary: false,
                    added_lines: Some(0),
                    deleted_lines: Some(0),
                }],
            },
            CommitReviewPatch {
                before_oid: Some("rename-one".to_string()),
                after_oid: "rename-two".to_string(),
                files: vec![GitCommitFileChange {
                    path: "final.txt".to_string(),
                    old_path: Some("middle.txt".to_string()),
                    kind: GitFileChangeKind::Renamed,
                    binary: false,
                    added_lines: Some(1),
                    deleted_lines: Some(0),
                }],
            },
        ]);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path.as_deref(), Some("original.txt"));
        assert_eq!(files[0].before_path.as_deref(), Some("original.txt"));
        assert_eq!(files[0].path, "final.txt");
        assert_eq!(files[0].before_oid.as_deref(), Some("parent"));
        assert_eq!(files[0].after_oid, "rename-two");
        assert_eq!(files[0].kind, GitFileChangeKind::Renamed);
    }

    #[test]
    fn aggregate_commit_review_files_only_contains_explicitly_collected_changes() {
        let files = aggregate_commit_review_files([
            CommitReviewPatch {
                before_oid: Some("before-selected-one".to_string()),
                after_oid: "selected-one".to_string(),
                files: vec![GitCommitFileChange {
                    path: "selected.txt".to_string(),
                    old_path: None,
                    kind: GitFileChangeKind::Modified,
                    binary: false,
                    added_lines: Some(1),
                    deleted_lines: Some(0),
                }],
            },
            CommitReviewPatch {
                // This parent may contain unselected commits. Only files collected from the
                // explicitly selected commit are inputs to the aggregate.
                before_oid: Some("parent-after-unselected-commit".to_string()),
                after_oid: "selected-two".to_string(),
                files: vec![GitCommitFileChange {
                    path: "selected.txt".to_string(),
                    old_path: None,
                    kind: GitFileChangeKind::Modified,
                    binary: false,
                    added_lines: Some(1),
                    deleted_lines: Some(1),
                }],
            },
        ]);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "selected.txt");
        assert_eq!(files[0].before_oid.as_deref(), Some("before-selected-one"));
        assert_eq!(files[0].after_oid, "selected-two");
    }

    #[test]
    fn deleted_commit_review_file_compares_before_content_to_missing_after_version() {
        let (_temp, root) = initialized_repository();
        let first = commit_file(&root, "removed.txt", "removed line\n", "add file");
        let runner = GitCommandRunner;
        assert!(
            runner
                .run(&root, &["rm", "--", "removed.txt"])
                .unwrap()
                .success
        );
        assert!(
            runner
                .run(&root, &["commit", "-m", "remove file"])
                .unwrap()
                .success
        );
        let deleted = runner.run(&root, &["rev-parse", "HEAD"]).unwrap().stdout;
        let service = GitSourceControlService::default();
        let revision = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(10),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap()
            .revision;
        let review = service
            .commit_review(
                &root,
                &GitCommitReviewQuery {
                    selected_oids: vec![deleted.clone()],
                    revision: Some(revision),
                },
            )
            .unwrap();

        assert_eq!(review.files.len(), 1);
        assert_eq!(review.files[0].kind, GitFileChangeKind::Deleted);
        assert_eq!(review.files[0].before_oid.as_deref(), Some(first.as_str()));
        let comparison = service
            .comparison(
                &root,
                &GitComparisonSource::Commit {
                    workspace_path: None,
                    path: review.files[0].path.clone(),
                    before_oid: review.files[0].before_oid.clone(),
                    before_path: review.files[0].before_path.clone(),
                    after_oid: review.files[0].after_oid.clone(),
                },
            )
            .unwrap();
        assert_eq!(comparison.before.unwrap().content, "removed line\n");
        assert!(comparison.after.is_none());
        assert_eq!(comparison.stats.added_lines, 0);
        assert_eq!(comparison.stats.deleted_lines, 1);
    }

    #[test]
    fn commit_reachability_reports_first_merge_and_containing_refs() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "base.txt", "base\n", "base");
        let runner = GitCommandRunner;
        let main_branch = runner
            .run(&root, &["branch", "--show-current"])
            .unwrap()
            .stdout;
        assert!(
            runner
                .run(&root, &["switch", "-c", "feature/reachability"])
                .unwrap()
                .success
        );
        let feature = commit_file(&root, "feature.txt", "feature\n", "feature");
        assert!(
            runner
                .run(&root, &["tag", "feature-point", feature.as_str()])
                .unwrap()
                .success
        );
        assert!(
            runner
                .run(&root, &["switch", main_branch.as_str()])
                .unwrap()
                .success
        );
        commit_file(&root, "main.txt", "main\n", "main");
        assert!(
            runner
                .run(
                    &root,
                    &[
                        "merge",
                        "--no-ff",
                        "feature/reachability",
                        "-m",
                        "merge feature"
                    ]
                )
                .unwrap()
                .success
        );
        let merge_oid = runner.run(&root, &["rev-parse", "HEAD"]).unwrap().stdout;

        let reachability = GitSourceControlService::default()
            .commit_reachability(
                &root,
                &GitCommitReachabilityQuery {
                    oid: feature,
                    target_ref: "HEAD".to_string(),
                },
            )
            .unwrap();

        assert_eq!(reachability.target_path, GitCommitTargetPath::Merged);
        assert_eq!(
            reachability.first_merge_oid.as_deref(),
            Some(merge_oid.as_str())
        );
        assert!(
            reachability
                .containing_refs
                .iter()
                .any(|git_ref| git_ref.short_name == "feature/reachability")
        );
        assert!(
            reachability
                .containing_refs
                .iter()
                .any(|git_ref| git_ref.short_name == "feature-point")
        );
    }

    #[test]
    fn commit_diff_parsers_preserve_rename_paths_and_binary_stats() {
        let statuses =
            parse_commit_name_status(b"R100\0old name.txt\0new name.txt\0A\0asset.bin\0").unwrap();
        let stats =
            parse_commit_numstat(b"1\t2\t\0old name.txt\0new name.txt\0-\t-\tasset.bin\0").unwrap();
        let changes = merge_commit_file_changes(statuses, stats).unwrap();

        assert_eq!(changes[0].kind, GitFileChangeKind::Renamed);
        assert_eq!(changes[0].old_path.as_deref(), Some("old name.txt"));
        assert_eq!(changes[0].path, "new name.txt");
        assert_eq!(changes[0].added_lines, Some(1));
        assert_eq!(changes[0].deleted_lines, Some(2));
        assert!(changes[1].binary);
        assert_eq!(changes[1].added_lines, None);
        assert_eq!(changes[1].deleted_lines, None);
    }

    #[test]
    fn stale_history_revision_is_rejected() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "one.txt", "one\n", "first");
        let error = GitSourceControlService::default()
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(10),
                    revision: Some("stale".to_string()),
                    ref_name: None,
                },
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<GitServiceError>().unwrap().code,
            "git.ref-changed"
        );
    }

    #[test]
    fn git_command_errors_keep_a_safe_actionable_reason() {
        let output = MachineCommandOutput {
            success: false,
            exit_code: Some(128),
            stdout: Vec::new(),
            stderr:
                b"fatal: Authentication failed for 'https://user:secret@example.com/repo.git/'\n"
                    .to_vec(),
        };

        let error = git_command_error("git.push-failed", &output);

        assert_eq!(error.code, "git.authentication-failed");
        assert_eq!(error.params["exitCode"], 128);
        assert_eq!(
            error.params["reason"],
            "fatal: Authentication failed for 'https://***@example.com/repo.git/'"
        );
        assert!(!error.params["reason"].as_str().unwrap().contains("secret"));
    }

    #[test]
    fn runtime_and_user_git_writes_share_the_same_repository_lock() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "one.txt", "one\n", "first");
        let identity = GitSourceControlService::default()
            .repository_identity(&root)
            .unwrap();
        let thread_identity = identity.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            GitCoordinationService
                .with_runtime_write(
                    &thread_identity.common_dir,
                    Some(&thread_identity.workspace_path),
                    "runtime-test",
                    || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(())
                    },
                )
                .unwrap();
        });
        entered_rx.recv().unwrap();

        let lock = GitCoordinationService.workspace_lock(&identity.workspace_path);
        assert!(lock.locked);
        assert_eq!(lock.owner, Some(GitLockOwner::Runtime));
        assert_eq!(lock.operation.as_deref(), Some("runtime-test"));
        let error = GitCoordinationService
            .try_with_user_write(
                &identity.common_dir,
                Some(&identity.workspace_path),
                "user-test",
                || Ok(()),
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<GitServiceError>().unwrap().code,
            "git.repository-locked"
        );

        release_tx.send(()).unwrap();
        handle.join().unwrap();
        assert!(
            !GitCoordinationService
                .workspace_lock(&identity.workspace_path)
                .locked
        );
    }

    #[test]
    fn typed_stage_and_unstage_preserve_special_paths() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "base.txt", "base\n", "base");
        let path = if cfg!(windows) {
            "space and 中文.txt"
        } else {
            "space and\nline.txt"
        };
        std::fs::write(root.join(path), "content\n").unwrap();
        let service = GitSourceControlService::default();
        let staged = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::StagePaths {
                        paths: vec![path.to_string()],
                    },
                },
            )
            .unwrap();
        let (staged_status, staged_revision) = match staged {
            GitMutationResult::Workspace {
                status,
                repository_revision,
            } => (status, repository_revision),
            GitMutationResult::Repository => panic!("stage must return a workspace result"),
        };
        assert_eq!(staged_status.staged[0].path, path);

        let unstaged = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: Some(staged_revision),
                    mutation: GitMutation::UnstagePaths {
                        paths: vec![path.to_string()],
                    },
                },
            )
            .unwrap();
        let GitMutationResult::Workspace { status, .. } = unstaged else {
            panic!("unstage must return a workspace result");
        };
        assert!(status.staged.is_empty());
        assert_eq!(status.untracked[0].path, path);
    }

    #[test]
    fn typed_commit_only_commits_the_index() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "tracked.txt", "one\n", "base");
        std::fs::write(root.join("tracked.txt"), "two\n").unwrap();
        std::fs::write(root.join("keep-untracked.txt"), "untracked\n").unwrap();
        let service = GitSourceControlService::default();
        service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::StagePaths {
                        paths: vec!["tracked.txt".to_string()],
                    },
                },
            )
            .unwrap();
        let committed = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::Commit {
                        subject: "feat: typed commit".to_string(),
                        body: Some("body from stdin".to_string()),
                    },
                },
            )
            .unwrap();
        assert_eq!(committed, GitMutationResult::Repository);
        let snapshot = service.snapshot("project-test", &root).unwrap();
        assert!(snapshot.status.staged.is_empty());
        assert_eq!(snapshot.status.untracked[0].path, "keep-untracked.txt");
        let history = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor: None,
                    limit: Some(1),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap();
        assert_eq!(history.commits[0].subject, "feat: typed commit");
        assert_eq!(history.commits[0].body.trim(), "body from stdin");
    }

    #[test]
    fn typed_branch_tag_and_worktree_creation_use_safe_git_operations() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "base.txt", "base\n", "base");
        let service = GitSourceControlService::default();
        service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::BranchCreate {
                        name: "feature/test".to_string(),
                        start_point: Some("HEAD".to_string()),
                        checkout: false,
                    },
                },
            )
            .unwrap();
        service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::TagCreate {
                        name: "v-test".to_string(),
                        target: Some("HEAD".to_string()),
                        style: GitTagStyle::Annotated,
                        message: Some("test tag".to_string()),
                    },
                },
            )
            .unwrap();
        let worktree_parent = tempfile::tempdir().unwrap();
        let worktree_path =
            Utf8PathBuf::from_path_buf(worktree_parent.path().join("child")).unwrap();
        let result = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::WorktreeCreate {
                        path: worktree_path.clone(),
                        source_ref: "HEAD".to_string(),
                        new_branch: Some("worktree/test".to_string()),
                    },
                },
            )
            .unwrap();
        assert_eq!(result, GitMutationResult::Repository);
        let snapshot = service.snapshot("project-test", &root).unwrap();
        assert!(snapshot.refs.iter().any(|git_ref| {
            git_ref.kind == GitRefKind::LocalBranch && git_ref.short_name == "feature/test"
        }));
        assert!(
            snapshot.refs.iter().any(|git_ref| {
                git_ref.kind == GitRefKind::Tag && git_ref.short_name == "v-test"
            })
        );
        assert!(
            snapshot
                .worktrees
                .iter()
                .any(|worktree| worktree.path == worktree_path)
        );
    }

    #[test]
    fn worktree_remove_deletes_a_clean_linked_worktree_and_preserves_its_branch() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "base.txt", "base\n", "base");
        let service = GitSourceControlService::default();
        let worktree_parent = tempfile::tempdir().unwrap();
        let worktree_path =
            Utf8PathBuf::from_path_buf(worktree_parent.path().join("clean-child")).unwrap();
        service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::WorktreeCreate {
                        path: worktree_path.clone(),
                        source_ref: "HEAD".to_string(),
                        new_branch: Some("worktree/removable".to_string()),
                    },
                },
            )
            .unwrap();

        let result = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::WorktreeRemove {
                        path: worktree_path.clone(),
                    },
                },
            )
            .unwrap();

        assert_eq!(result, GitMutationResult::Repository);
        assert!(!worktree_path.exists());
        assert!(
            service
                .worktrees(&root)
                .unwrap()
                .iter()
                .all(|worktree| worktree.path != worktree_path)
        );
        assert!(service.refs(&root).unwrap().iter().any(|git_ref| {
            git_ref.kind == GitRefKind::LocalBranch && git_ref.short_name == "worktree/removable"
        }));
    }

    #[test]
    fn worktree_remove_refuses_dirty_and_current_worktrees_without_force() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "base.txt", "base\n", "base");
        let service = GitSourceControlService::default();
        let worktree_parent = tempfile::tempdir().unwrap();
        let worktree_path =
            Utf8PathBuf::from_path_buf(worktree_parent.path().join("dirty-child")).unwrap();
        service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::WorktreeCreate {
                        path: worktree_path.clone(),
                        source_ref: "HEAD".to_string(),
                        new_branch: Some("worktree/dirty".to_string()),
                    },
                },
            )
            .unwrap();
        std::fs::write(worktree_path.join("untracked.txt"), "keep\n").unwrap();

        let dirty_error = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::WorktreeRemove {
                        path: worktree_path.clone(),
                    },
                },
            )
            .unwrap_err();
        assert_eq!(
            dirty_error.downcast_ref::<GitServiceError>().unwrap().code,
            "git.worktree-remove-dirty"
        );
        assert!(worktree_path.exists());

        let current_error = service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::WorktreeRemove { path: root.clone() },
                },
            )
            .unwrap_err();
        assert_eq!(
            current_error
                .downcast_ref::<GitServiceError>()
                .unwrap()
                .code,
            "git.worktree-current-remove-forbidden"
        );
        assert!(root.exists());
    }

    #[test]
    fn workspace_and_commit_comparisons_share_one_text_contract() {
        let (_temp, root) = initialized_repository();
        let first = commit_file(&root, "tracked.txt", "one\n", "first");
        std::fs::write(root.join("tracked.txt"), "one\ntwo\n").unwrap();
        let service = GitSourceControlService::default();
        let unstaged = service
            .comparison(
                &root,
                &GitComparisonSource::Workspace {
                    workspace_path: None,
                    path: "tracked.txt".to_string(),
                    area: GitWorkspaceDiffArea::Unstaged,
                },
            )
            .unwrap();
        assert_eq!(unstaged.before.unwrap().content, "one\n");
        assert_eq!(unstaged.after.unwrap().content, "one\ntwo\n");
        assert_eq!(unstaged.stats.added_lines, 1);

        service
            .execute_mutation(
                &root,
                &GitMutationRequest {
                    expected_revision: None,
                    mutation: GitMutation::StagePaths {
                        paths: vec!["tracked.txt".to_string()],
                    },
                },
            )
            .unwrap();
        let staged = service
            .comparison(
                &root,
                &GitComparisonSource::Workspace {
                    workspace_path: None,
                    path: "tracked.txt".to_string(),
                    area: GitWorkspaceDiffArea::Staged,
                },
            )
            .unwrap();
        assert_eq!(staged.before.unwrap().content, "one\n");
        assert_eq!(staged.after.unwrap().content, "one\ntwo\n");

        let second = commit_file(&root, "tracked.txt", "one\ntwo\nthree\n", "second");
        let committed = service
            .comparison(
                &root,
                &GitComparisonSource::Commit {
                    workspace_path: None,
                    path: "tracked.txt".to_string(),
                    before_oid: Some(first),
                    before_path: None,
                    after_oid: second,
                },
            )
            .unwrap();
        assert_eq!(committed.stats.added_lines, 2);
        assert_eq!(committed.stats.deleted_lines, 0);
    }

    #[test]
    fn workspace_status_reports_staged_and_unstaged_numstat_in_batches() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "tracked.txt", "first\nsecond\nthird\n", "baseline");
        std::fs::write(root.join("tracked.txt"), "first\nchanged\nthird\n").unwrap();
        let service = GitSourceControlService::default();

        let unstaged = service.status(&root).unwrap();
        let tracked = unstaged
            .unstaged
            .iter()
            .find(|change| change.path == "tracked.txt")
            .unwrap();
        assert_eq!(tracked.added_lines, Some(1));
        assert_eq!(tracked.deleted_lines, Some(1));
        assert!(!tracked.binary);

        let runner = GitCommandRunner;
        assert!(
            runner
                .run(&root, &["add", "--", "tracked.txt"])
                .unwrap()
                .success
        );
        std::fs::write(root.join("tracked.txt"), "first\nchanged\nthird\nfourth\n").unwrap();

        let split = service.status(&root).unwrap();
        let staged = split
            .staged
            .iter()
            .find(|change| change.path == "tracked.txt")
            .unwrap();
        assert_eq!(staged.added_lines, Some(1));
        assert_eq!(staged.deleted_lines, Some(1));
        let unstaged = split
            .unstaged
            .iter()
            .find(|change| change.path == "tracked.txt")
            .unwrap();
        assert_eq!(unstaged.added_lines, Some(1));
        assert_eq!(unstaged.deleted_lines, Some(0));
    }

    #[test]
    fn baseline_diff_stats_include_committed_index_workspace_and_untracked_state() {
        let (_temp, root) = initialized_repository();
        let baseline = commit_file(&root, "tracked.txt", "one\ntwo\n", "baseline");
        let service = GitSourceControlService::default();
        assert_eq!(
            service.baseline_diff_stats(&root, &baseline).unwrap(),
            GitBaselineDiffStats::default()
        );

        std::fs::write(root.join("tracked.txt"), "one\nthree\nfour\n").unwrap();
        std::fs::write(root.join("staged.txt"), "a\nb\n").unwrap();
        assert!(
            GitCommandRunner
                .run(&root, &["add", "--", "staged.txt"])
                .unwrap()
                .success
        );
        std::fs::write(root.join("untracked.txt"), "x\ny\nz\n").unwrap();
        std::fs::write(root.join("untracked.bin"), [0, 1, 2]).unwrap();

        assert_eq!(
            service.baseline_diff_stats(&root, &baseline).unwrap(),
            GitBaselineDiffStats {
                added_lines: 7,
                deleted_lines: 1,
                changed_files: 4,
            }
        );
    }

    #[test]
    fn baseline_diff_stats_count_a_pure_rename_as_one_changed_file() {
        let (_temp, root) = initialized_repository();
        let baseline = commit_file(&root, "before.txt", "one\ntwo\n", "baseline");
        assert!(
            GitCommandRunner
                .run(&root, &["mv", "--", "before.txt", "after.txt"])
                .unwrap()
                .success
        );

        assert_eq!(
            GitSourceControlService::default()
                .baseline_diff_stats(&root, &baseline)
                .unwrap(),
            GitBaselineDiffStats {
                added_lines: 0,
                deleted_lines: 0,
                changed_files: 1,
            }
        );
    }

    #[test]
    fn comparison_normalizes_line_endings_before_rendering_and_counting() {
        let comparison = comparison_from_versions(
            "mixed.txt".to_string(),
            Some(b"first\r\nsecond\rthird\r\n".to_vec()),
            Some(b"first\nsecond\nthird\nadded\n".to_vec()),
        )
        .unwrap();

        assert_eq!(comparison.before.unwrap().content, "first\nsecond\nthird\n");
        assert_eq!(
            comparison.after.unwrap().content,
            "first\nsecond\nthird\nadded\n"
        );
        assert_eq!(comparison.stats.added_lines, 1);
        assert_eq!(comparison.stats.deleted_lines, 0);
    }

    #[test]
    fn large_line_ending_only_changes_do_not_expand_the_whole_diff() {
        let before = (0..8_000)
            .map(|line| format!("line {line}\r\n"))
            .collect::<String>();
        let mut after = (0..8_000)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        after.push_str("added one\nadded two\n");

        let comparison = comparison_from_versions(
            "large-source.rs".to_string(),
            Some(before.into_bytes()),
            Some(after.into_bytes()),
        )
        .unwrap();

        assert_eq!(comparison.stats.added_lines, 2);
        assert_eq!(comparison.stats.deleted_lines, 0);
        assert!(!comparison.before.unwrap().content.contains('\r'));
    }

    #[test]
    fn comparison_contract_uses_stable_limitation_codes() {
        let binary = comparison_from_versions(
            "binary.dat".to_string(),
            Some(vec![b'a', 0, b'b']),
            Some(vec![b'a', 0, b'c']),
        )
        .unwrap();
        assert_eq!(
            binary.limitation_code.as_deref(),
            Some("git.binary-diff-unsupported")
        );
        assert!(binary.before.is_none());
        assert!(binary.after.is_none());

        let too_large = comparison_from_versions(
            "large.txt".to_string(),
            None,
            Some(vec![b'x'; 4 * 1024 * 1024 + 1]),
        )
        .unwrap();
        assert_eq!(
            too_large.limitation_code.as_deref(),
            Some("git.diff-too-large")
        );

        let invalid_encoding =
            comparison_from_versions("legacy.txt".to_string(), Some(vec![0xff]), Some(vec![0xfe]))
                .unwrap();
        assert_eq!(
            invalid_encoding.limitation_code.as_deref(),
            Some("git.text-encoding-unsupported")
        );
    }

    #[test]
    fn mutation_and_comparison_json_contracts_match_frontend_tagged_unions() {
        let mutation: GitMutationRequest = serde_json::from_value(serde_json::json!({
            "kind": "tag-create",
            "expectedRevision": "revision-1",
            "name": "v1.0.0",
            "target": "HEAD",
            "style": "annotated",
            "message": "release"
        }))
        .unwrap();
        assert_eq!(mutation.expected_revision.as_deref(), Some("revision-1"));
        assert!(matches!(
            mutation.mutation,
            GitMutation::TagCreate {
                style: GitTagStyle::Annotated,
                ..
            }
        ));
        assert_eq!(
            serde_json::to_value(mutation).unwrap(),
            serde_json::json!({
                "kind": "tag-create",
                "expectedRevision": "revision-1",
                "name": "v1.0.0",
                "target": "HEAD",
                "style": "annotated",
                "message": "release"
            })
        );

        let comparison: GitComparisonSource = serde_json::from_value(serde_json::json!({
            "kind": "workspace",
            "workspacePath": "D:/repo/worktree",
            "path": "src/main.rs",
            "area": "staged"
        }))
        .unwrap();
        assert_eq!(comparison.workspace_path(), Some("D:/repo/worktree"));

        let github_comparison: GitComparisonSource = serde_json::from_value(serde_json::json!({
            "kind": "github-pr",
            "workspacePath": "D:/repo/worktree",
            "host": "github.com",
            "repository": "acme/widgets",
            "prNumber": 42,
            "baseOid": "1111111111111111111111111111111111111111",
            "headOid": "2222222222222222222222222222222222222222",
            "path": "src/main.rs",
            "beforePath": null
        }))
        .unwrap();
        assert!(matches!(
            github_comparison,
            GitComparisonSource::GitHubPr { pr_number: 42, .. }
        ));
        assert_eq!(
            serde_json::to_value(github_comparison).unwrap(),
            serde_json::json!({
                "kind": "github-pr",
                "workspacePath": "D:/repo/worktree",
                "host": "github.com",
                "repository": "acme/widgets",
                "prNumber": 42,
                "baseOid": "1111111111111111111111111111111111111111",
                "headOid": "2222222222222222222222222222222222222222",
                "path": "src/main.rs",
                "beforePath": null
            })
        );
    }

    #[test]
    fn scoped_workspace_accepts_linked_worktree_and_rejects_other_repository() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "base.txt", "base\n", "base");
        let worktree_parent = tempfile::tempdir().unwrap();
        let worktree = Utf8PathBuf::from_path_buf(worktree_parent.path().join("child")).unwrap();
        assert!(
            GitCommandRunner
                .run(
                    &root,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        "scoped/test",
                        worktree.as_str(),
                        "HEAD"
                    ],
                )
                .unwrap()
                .success
        );
        let service = GitSourceControlService::default();
        let resolved = service
            .resolve_scoped_workspace(&root, Some(&worktree))
            .unwrap();
        assert_eq!(
            resolved.workspace_path,
            canonical_utf8_path(&worktree).unwrap()
        );

        let (_other_temp, other_root) = initialized_repository();
        let error = service
            .resolve_scoped_workspace(&root, Some(&other_root))
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<GitServiceError>().unwrap().code,
            "git.workspace-outside-project"
        );
    }

    #[test]
    fn runtime_branch_detection_matches_dynamic_branch_naming_contract() {
        assert!(is_runtime_branch("refs/heads/gb-dyn-task-run-dyn-id"));
        assert!(is_runtime_branch("refs/heads/gb-runtime/checkpoint"));
        assert!(!is_runtime_branch("refs/heads/feature/gb-dyn-example"));
    }

    #[test]
    fn managed_stash_create_and_apply_preserve_the_stash_entry() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "tracked.txt", "one\n", "base");
        std::fs::write(root.join("tracked.txt"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
        let service = GitSourceControlService::default();
        let started = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: None,
                    operation: GitOperationInput::StashCreate {
                        message: Some("workspace snapshot".to_string()),
                        include_untracked: true,
                    },
                },
            )
            .unwrap();
        let completed = wait_operation(&service, &started.operation_id);
        assert_eq!(completed.status, GitOperationStatus::Succeeded);
        let snapshot = service.snapshot("project-test", &root).unwrap();
        assert!(snapshot.status.unstaged.is_empty());
        assert!(snapshot.status.untracked.is_empty());
        assert_eq!(snapshot.stashes.len(), 1);

        let applied = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: Some(snapshot.repository.revision),
                    operation: GitOperationInput::StashApply {
                        stash_ref: snapshot.stashes[0].ref_name.clone(),
                        restore_index: false,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &applied.operation_id).status,
            GitOperationStatus::Succeeded
        );
        let restored = service.snapshot("project-test", &root).unwrap();
        assert_eq!(restored.stashes.len(), 1);
        assert_eq!(restored.status.unstaged[0].path, "tracked.txt");
        assert_eq!(restored.status.untracked[0].path, "untracked.txt");
    }

    #[test]
    fn managed_push_fetch_and_ff_only_pull_use_typed_arguments() {
        let (_temp, root) = initialized_repository();
        commit_file(&root, "tracked.txt", "one\n", "base");
        let bare_temp = tempfile::tempdir().unwrap();
        let bare = Utf8PathBuf::from_path_buf(bare_temp.path().join("remote.git")).unwrap();
        let runner = GitCommandRunner;
        assert!(
            runner
                .run(&root, &["init", "--bare", bare.as_str()])
                .unwrap()
                .success
        );
        assert!(
            runner
                .run(&root, &["remote", "add", "origin", bare.as_str()])
                .unwrap()
                .success
        );
        let branch = runner
            .run(&root, &["branch", "--show-current"])
            .unwrap()
            .stdout;
        let service = GitSourceControlService::default();
        let push = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: None,
                    operation: GitOperationInput::Push {
                        remote: "origin".to_string(),
                        branch: branch.clone(),
                        set_upstream: true,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &push.operation_id).status,
            GitOperationStatus::Succeeded
        );
        let fetch = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: None,
                    operation: GitOperationInput::Fetch {
                        remote: Some("origin".to_string()),
                        prune: true,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &fetch.operation_id).status,
            GitOperationStatus::Succeeded
        );
        let pull = service
            .start_operation(
                &root,
                &GitOperationRequest {
                    expected_revision: None,
                    operation: GitOperationInput::Pull {
                        remote: None,
                        branch: None,
                        strategy: GitPullStrategy::FastForwardOnly,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            wait_operation(&service, &pull.operation_id).status,
            GitOperationStatus::Succeeded
        );
    }
}
