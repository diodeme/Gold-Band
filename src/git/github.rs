use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use camino::Utf8Path;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::source_control::{
    GitDiffStats, GitFileChangeKind, GitFileComparison, comparison_from_versions,
    validate_repo_relative_path,
};
use crate::git::{GitRemote, GitSourceControlService};
use crate::process::{
    ManagedProcessGroup, PROCESS_GROUP_TERMINATION_GRACE, background_command,
    find_executable_in_path,
};

const GITHUB_LIST_LIMIT: usize = 50;
const GITHUB_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const GITHUB_PR_BODY_LIMIT: usize = 1024 * 1024;
const OPERATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct GitHubServiceError {
    pub code: &'static str,
    pub params: serde_json::Value,
    pub diagnostic: Option<String>,
}

impl GitHubServiceError {
    fn new(code: &'static str, params: serde_json::Value) -> Self {
        Self {
            code,
            params,
            diagnostic: None,
        }
    }

    fn command(code: &'static str, output: &GitHubCommandOutput) -> Self {
        Self {
            code,
            params: serde_json::json!({
                "exitCode": output.exit_code,
                "outputTruncated": output.stdout_truncated || output.stderr_truncated,
            }),
            diagnostic: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitHubCapabilityStatus {
    NotInstalled,
    NotAuthenticated,
    RepositoryUnresolved,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCapability {
    pub status: GitHubCapabilityStatus,
    pub version: Option<String>,
    pub host: Option<String>,
    pub account: Option<String>,
    pub repository: Option<String>,
    pub remote: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitHubOperationStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubOperation {
    pub operation_id: String,
    pub kind: GitHubOperationKind,
    pub host: String,
    pub status: GitHubOperationStatus,
    pub cancelable: bool,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<GitHubOperationError>,
    pub result_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitHubOperationKind {
    Login,
    PrCreate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubOperationError {
    pub code: String,
    pub params: serde_json::Value,
}

struct GitHubOperationCell {
    state: Mutex<GitHubOperation>,
    process: Mutex<Option<ManagedProcessGroup>>,
    cancel_requested: AtomicBool,
    update_sink: Option<GitHubOperationUpdateSink>,
}

#[derive(Default)]
struct GitHubOperationRegistry {
    operations: Mutex<std::collections::HashMap<String, Arc<GitHubOperationCell>>>,
}

static GITHUB_OPERATION_REGISTRY: OnceLock<GitHubOperationRegistry> = OnceLock::new();

pub type GitHubOperationUpdateSink = Arc<dyn Fn(GitHubOperation) + Send + Sync + 'static>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubActor {
    pub login: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubLabel {
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestSummary {
    pub number: u64,
    pub title: String,
    pub state: String,
    #[serde(rename = "isDraft")]
    pub draft: bool,
    pub author: Option<GitHubActor>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub updated_at: String,
    pub url: String,
    pub review_decision: Option<String>,
    pub labels: Vec<GitHubLabel>,
    #[serde(rename = "statusCheckRollup", default)]
    pub status_checks: Vec<GitHubStatusCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubStatusCheck {
    #[serde(rename = "__typename", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubReview {
    pub author: Option<GitHubActor>,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestFile {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: GitFileChangeKind,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubApiPullRequestFile {
    filename: String,
    status: String,
    previous_filename: Option<String>,
    additions: u64,
    deletions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestDetail {
    #[serde(flatten)]
    pub summary: GitHubPullRequestSummary,
    pub base_ref_oid: String,
    pub head_ref_oid: String,
    pub body: String,
    pub mergeable: Option<String>,
    pub merge_state_status: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    #[serde(default)]
    pub files: Vec<GitHubPullRequestFile>,
    #[serde(default)]
    pub latest_reviews: Vec<GitHubReview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIssueSummary {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub author: Option<GitHubActor>,
    pub assignees: Vec<GitHubActor>,
    pub labels: Vec<GitHubLabel>,
    pub updated_at: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIssueDetail {
    #[serde(flatten)]
    pub summary: GitHubIssueSummary,
    pub body: String,
    pub milestone: Option<GitHubMilestone>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubMilestone {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitHubListState {
    Open,
    Closed,
    All,
}

impl GitHubListState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestQuery {
    pub state: GitHubListState,
    pub author: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub label: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIssueQuery {
    pub state: GitHubListState,
    pub author: Option<String>,
    pub assignee: Option<String>,
    pub label: Option<String>,
    pub milestone: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestPreflightInput {
    pub host: String,
    pub repository: String,
    pub head: String,
    pub base: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestCreateInput {
    pub host: String,
    pub repository: String,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

impl GitHubPullRequestCreateInput {
    fn preflight_input(&self) -> GitHubPullRequestPreflightInput {
        GitHubPullRequestPreflightInput {
            host: self.host.clone(),
            repository: self.repository.clone(),
            head: self.head.clone(),
            base: self.base.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestPreflight {
    pub remote: String,
    pub head: String,
    pub base: String,
    pub ahead_by: u64,
    pub head_published: bool,
    pub existing_pull_request: Option<GitHubPullRequestSummary>,
}

#[derive(Debug, Clone)]
struct GitHubCommandOutput {
    success: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

#[derive(Debug, Default)]
struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubRepositoryView {
    name_with_owner: String,
    default_branch_ref: Option<GitHubDefaultBranchRef>,
}

#[derive(Debug)]
enum GitHubFileVersion {
    Missing,
    Content(Vec<u8>),
    TooLarge,
}

#[derive(Debug, Deserialize)]
struct GitHubDefaultBranchRef {
    name: String,
}

#[derive(Debug, Clone)]
pub struct GitHubCliService {
    executable: Option<PathBuf>,
    git_executable: Option<PathBuf>,
}

impl Default for GitHubCliService {
    fn default() -> Self {
        Self {
            executable: find_executable_in_path("gh"),
            git_executable: find_executable_in_path("git"),
        }
    }
}

impl GitHubCliService {
    pub fn start_login(&self, cwd: &Utf8Path, host: &str) -> Result<GitHubOperation> {
        self.start_login_with_update_sink(cwd, host, None)
    }

    pub fn start_login_with_update_sink(
        &self,
        cwd: &Utf8Path,
        host: &str,
        update_sink: Option<GitHubOperationUpdateSink>,
    ) -> Result<GitHubOperation> {
        let executable = self.executable.clone().ok_or_else(|| {
            GitHubServiceError::new("github.gh-not-installed", serde_json::json!({}))
        })?;
        validate_host(host)?;
        let operation = GitHubOperation {
            operation_id: Uuid::new_v4().to_string(),
            kind: GitHubOperationKind::Login,
            host: host.to_string(),
            status: GitHubOperationStatus::Queued,
            cancelable: true,
            started_at: None,
            completed_at: None,
            error: None,
            result_url: None,
        };
        let cell = Arc::new(GitHubOperationCell {
            state: Mutex::new(operation.clone()),
            process: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
            update_sink,
        });
        github_operation_registry()
            .operations
            .lock()
            .map_err(|_| {
                GitHubServiceError::new("github.operation-registry-poisoned", serde_json::json!({}))
            })?
            .insert(operation.operation_id.clone(), cell.clone());
        let cwd = cwd.to_path_buf();
        let host = host.to_string();
        thread::spawn(move || run_github_login(executable, cwd, host, cell));
        Ok(operation)
    }

    pub fn get_operation(&self, operation_id: &str) -> Result<GitHubOperation> {
        let cell = github_operation_cell(operation_id)?;
        let state = cell.state.lock().map_err(|_| {
            GitHubServiceError::new("github.operation-state-poisoned", serde_json::json!({}))
        })?;
        Ok(state.clone())
    }

    pub fn cancel_operation(&self, operation_id: &str) -> Result<GitHubOperation> {
        let cell = github_operation_cell(operation_id)?;
        cell.cancel_requested.store(true, Ordering::SeqCst);
        {
            let mut process = cell.process.lock().map_err(|_| {
                GitHubServiceError::new("github.operation-process-poisoned", serde_json::json!({}))
            })?;
            if let Some(process) = process.as_mut() {
                process.terminate(PROCESS_GROUP_TERMINATION_GRACE)?;
            }
        }
        finish_github_operation(&cell, GitHubOperationStatus::Cancelled, None, None);
        self.get_operation(operation_id)
    }

    pub fn capability(&self, cwd: &Utf8Path) -> Result<GitHubCapability> {
        let Some(_) = self.executable else {
            return Ok(GitHubCapability {
                status: GitHubCapabilityStatus::NotInstalled,
                version: None,
                host: None,
                account: None,
                repository: None,
                remote: None,
                default_branch: None,
            });
        };
        let version_output = self.run(cwd, &["--version"])?;
        if !version_output.success {
            return Ok(GitHubCapability {
                status: GitHubCapabilityStatus::NotInstalled,
                version: None,
                host: None,
                account: None,
                repository: None,
                remote: None,
                default_branch: None,
            });
        }
        let version = String::from_utf8_lossy(&version_output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let auth_output = self.run(cwd, &["auth", "status", "--json", "hosts"])?;
        let accounts = parse_auth_hosts(&auth_output.stdout);
        if accounts.is_empty() {
            return Ok(GitHubCapability {
                status: GitHubCapabilityStatus::NotAuthenticated,
                version,
                host: None,
                account: None,
                repository: None,
                remote: None,
                default_branch: None,
            });
        }
        let git = GitSourceControlService::default();
        let status = git.status(cwd)?;
        let remotes = git.remotes(cwd)?;
        let Some(mapping) = resolve_repository_mapping(&remotes, status.branch.upstream.as_deref())
        else {
            let (host, account) = accounts.first().cloned().unwrap();
            return Ok(GitHubCapability {
                status: GitHubCapabilityStatus::RepositoryUnresolved,
                version,
                host: Some(host),
                account: Some(account),
                repository: None,
                remote: None,
                default_branch: None,
            });
        };
        let account = accounts
            .iter()
            .find(|(host, _)| host == &mapping.host)
            .map(|(_, login)| login.clone());
        let Some(account) = account else {
            return Ok(GitHubCapability {
                status: GitHubCapabilityStatus::NotAuthenticated,
                version,
                host: Some(mapping.host),
                account: None,
                repository: Some(mapping.repository),
                remote: mapping.remote,
                default_branch: None,
            });
        };
        let repository = repository_selector(&mapping.host, &mapping.repository)?;
        let repo_args = github_repository_view_args(&repository);
        let repo_arg_refs = repo_args.iter().map(String::as_str).collect::<Vec<_>>();
        let repo_output = self.run(cwd, &repo_arg_refs)?;
        if !repo_output.success {
            return Ok(GitHubCapability {
                status: GitHubCapabilityStatus::RepositoryUnresolved,
                version,
                host: Some(mapping.host),
                account: Some(account),
                repository: Some(mapping.repository),
                remote: mapping.remote,
                default_branch: None,
            });
        }
        let repository_view = serde_json::from_slice::<GitHubRepositoryView>(&repo_output.stdout)
            .map_err(|_| {
            GitHubServiceError::new("github.invalid-json", serde_json::json!({}))
        })?;
        Ok(GitHubCapability {
            status: GitHubCapabilityStatus::Ready,
            version,
            host: Some(mapping.host),
            account: Some(account),
            repository: Some(repository_view.name_with_owner),
            remote: mapping.remote,
            default_branch: repository_view.default_branch_ref.map(|branch| branch.name),
        })
    }

    pub fn list_pull_requests(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        query: &GitHubPullRequestQuery,
    ) -> Result<Vec<GitHubPullRequestSummary>> {
        validate_repository(repository)?;
        let repository = repository_selector(host, repository)?;
        let mut args = vec!["pr".to_string(), "list".to_string(), "--repo".to_string(), repository, "--state".to_string(), query.state.as_str().to_string(), "--limit".to_string(), GITHUB_LIST_LIMIT.to_string(), "--json".to_string(), "number,title,state,isDraft,author,headRefName,baseRefName,updatedAt,url,reviewDecision,labels,statusCheckRollup".to_string()];
        push_optional_flag(&mut args, "--author", query.author.as_deref())?;
        push_optional_flag(&mut args, "--base", query.base.as_deref())?;
        push_optional_flag(&mut args, "--head", query.head.as_deref())?;
        push_optional_flag(&mut args, "--label", query.label.as_deref())?;
        push_optional_flag(&mut args, "--search", query.search.as_deref())?;
        self.require_json(cwd, &args, "github.pr-list-failed")
    }

    pub fn pull_request_detail(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        number: u64,
    ) -> Result<GitHubPullRequestDetail> {
        validate_repository(repository)?;
        validate_host(host)?;
        let selector = repository_selector(host, repository)?;
        let mut detail: GitHubPullRequestDetail = self.require_json(cwd, &["pr".into(), "view".into(), number.to_string(), "--repo".into(), selector, "--json".into(), "number,title,body,state,isDraft,author,headRefName,baseRefName,baseRefOid,headRefOid,updatedAt,url,reviewDecision,labels,statusCheckRollup,latestReviews,mergeable,mergeStateStatus,additions,deletions,changedFiles".into()], "github.pr-detail-failed")?;
        let file_args = github_pull_request_files_args(host, repository, number);
        let pages: Vec<Vec<GitHubApiPullRequestFile>> =
            self.require_json(cwd, &file_args, "github.pr-detail-files-failed")?;
        detail.files = pages
            .into_iter()
            .flatten()
            .map(github_pull_request_file)
            .collect::<Result<Vec<_>>>()?;
        Ok(detail)
    }

    pub fn pull_request_revision_comparison(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        number: u64,
        base_oid: &str,
        head_oid: &str,
        path: &str,
        before_path: Option<&str>,
    ) -> Result<GitFileComparison> {
        validate_host(host)?;
        validate_repository(repository)?;
        validate_github_oid(base_oid)?;
        validate_github_oid(head_oid)?;
        validate_repo_relative_path(path)?;
        if let Some(before_path) = before_path {
            validate_repo_relative_path(before_path)?;
        }
        if number == 0 {
            return Err(GitHubServiceError::new(
                "github.pr-number-invalid",
                serde_json::json!({ "number": number }),
            )
            .into());
        }
        let (before, after) = std::thread::scope(|scope| {
            let before = scope.spawn(|| {
                self.pull_request_file_version(
                    cwd,
                    host,
                    repository,
                    base_oid,
                    before_path.unwrap_or(path),
                )
            });
            let after = scope
                .spawn(|| self.pull_request_file_version(cwd, host, repository, head_oid, path));
            let before = before.join().map_err(|_| {
                GitHubServiceError::new(
                    "github.pr-file-content-failed",
                    serde_json::json!({ "number": number, "path": path, "revision": "base" }),
                )
            })??;
            let after = after.join().map_err(|_| {
                GitHubServiceError::new(
                    "github.pr-file-content-failed",
                    serde_json::json!({ "number": number, "path": path, "revision": "head" }),
                )
            })??;
            Ok::<_, anyhow::Error>((before, after))
        })?;
        if matches!(before, GitHubFileVersion::TooLarge)
            || matches!(after, GitHubFileVersion::TooLarge)
        {
            return Ok(GitFileComparison {
                path: path.to_string(),
                stats: GitDiffStats {
                    added_lines: 0,
                    deleted_lines: 0,
                },
                before: None,
                after: None,
                limitation_code: Some("git.diff-too-large".to_string()),
            });
        }
        let before = match before {
            GitHubFileVersion::Missing => None,
            GitHubFileVersion::Content(content) => Some(content),
            GitHubFileVersion::TooLarge => unreachable!("handled above"),
        };
        let after = match after {
            GitHubFileVersion::Missing => None,
            GitHubFileVersion::Content(content) => Some(content),
            GitHubFileVersion::TooLarge => unreachable!("handled above"),
        };
        if before.is_none() && after.is_none() {
            return Err(GitHubServiceError::new(
                "github.pr-file-content-unavailable",
                serde_json::json!({ "number": number, "path": path }),
            )
            .into());
        }
        comparison_from_versions(path.to_string(), before, after)
    }

    pub fn preflight_pull_request(
        &self,
        cwd: &Utf8Path,
        input: &GitHubPullRequestPreflightInput,
    ) -> Result<GitHubPullRequestPreflight> {
        validate_host(&input.host)?;
        validate_repository(&input.repository)?;
        self.validate_branch_name(cwd, &input.head, "github.pr-head-invalid")?;
        self.validate_branch_name(cwd, &input.base, "github.pr-base-invalid")?;
        if input.head == input.base {
            return Err(GitHubServiceError::new(
                "github.pr-head-equals-base",
                serde_json::json!({ "head": input.head, "base": input.base }),
            )
            .into());
        }

        let capability = self.capability(cwd)?;
        if capability.status != GitHubCapabilityStatus::Ready {
            return Err(GitHubServiceError::new(
                capability_error_code(capability.status),
                serde_json::json!({}),
            )
            .into());
        }
        let capability_host = capability.host.as_deref().unwrap_or_default();
        let capability_repository = capability.repository.as_deref().unwrap_or_default();
        if !capability_host.eq_ignore_ascii_case(&input.host)
            || !capability_repository.eq_ignore_ascii_case(&input.repository)
        {
            return Err(GitHubServiceError::new(
                "github.repository-mismatch",
                serde_json::json!({
                    "expectedHost": capability_host,
                    "expectedRepository": capability_repository,
                }),
            )
            .into());
        }
        let remote = capability.remote.ok_or_else(|| {
            GitHubServiceError::new("github.repository-unresolved", serde_json::json!({}))
        })?;

        let head_revision = format!("refs/heads/{}^{{commit}}", input.head);
        let head_oid = self.require_git_revision(
            cwd,
            &head_revision,
            "github.pr-head-not-found",
            &input.head,
        )?;
        let remote_base_revision = format!("refs/remotes/{remote}/{}^{{commit}}", input.base);
        let local_base_revision = format!("refs/heads/{}^{{commit}}", input.base);
        let base_oid = match self.git_revision(cwd, &remote_base_revision)? {
            Some(oid) => Some(oid),
            None => self.git_revision(cwd, &local_base_revision)?,
        }
        .ok_or_else(|| {
            GitHubServiceError::new(
                "github.pr-base-not-found",
                serde_json::json!({ "base": input.base }),
            )
        })?;
        let range = format!("{base_oid}..{head_oid}");
        let count_output = self.run_git(cwd, &["rev-list", "--count", &range])?;
        if !count_output.success {
            return Err(GitHubServiceError::command(
                "github.pr-commit-range-failed",
                &count_output,
            )
            .into());
        }
        let ahead_by = String::from_utf8_lossy(&count_output.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|_| {
                GitHubServiceError::new("github.invalid-git-output", serde_json::json!({}))
            })?;
        if ahead_by == 0 {
            return Err(GitHubServiceError::new(
                "github.pr-no-commits-ahead",
                serde_json::json!({ "head": input.head, "base": input.base }),
            )
            .into());
        }

        let head_published =
            self.github_branch_exists(cwd, &input.host, &input.repository, &input.head)?;
        let existing_pull_request = if head_published {
            self.list_pull_requests(
                cwd,
                &input.host,
                &input.repository,
                &GitHubPullRequestQuery {
                    state: GitHubListState::Open,
                    author: None,
                    base: Some(input.base.clone()),
                    head: Some(input.head.clone()),
                    label: None,
                    search: None,
                },
            )?
            .into_iter()
            .next()
        } else {
            None
        };

        Ok(GitHubPullRequestPreflight {
            remote,
            head: input.head.clone(),
            base: input.base.clone(),
            ahead_by,
            head_published,
            existing_pull_request,
        })
    }

    pub fn start_pull_request_create(
        &self,
        cwd: &Utf8Path,
        input: GitHubPullRequestCreateInput,
    ) -> Result<GitHubOperation> {
        self.start_pull_request_create_with_update_sink(cwd, input, None)
    }

    pub fn start_pull_request_create_with_update_sink(
        &self,
        cwd: &Utf8Path,
        input: GitHubPullRequestCreateInput,
        update_sink: Option<GitHubOperationUpdateSink>,
    ) -> Result<GitHubOperation> {
        validate_pull_request_text(&input)?;
        let preflight = self.preflight_pull_request(cwd, &input.preflight_input())?;
        if !preflight.head_published {
            return Err(GitHubServiceError::new(
                "github.pr-head-not-published",
                serde_json::json!({ "head": input.head, "remote": preflight.remote }),
            )
            .into());
        }
        if let Some(existing) = preflight.existing_pull_request {
            return Err(GitHubServiceError::new(
                "github.pr-already-exists",
                serde_json::json!({ "number": existing.number, "url": existing.url }),
            )
            .into());
        }
        let executable = self.executable.clone().ok_or_else(|| {
            GitHubServiceError::new("github.gh-not-installed", serde_json::json!({}))
        })?;
        let operation = GitHubOperation {
            operation_id: Uuid::new_v4().to_string(),
            kind: GitHubOperationKind::PrCreate,
            host: input.host.clone(),
            status: GitHubOperationStatus::Queued,
            cancelable: true,
            started_at: None,
            completed_at: None,
            error: None,
            result_url: None,
        };
        let cell = Arc::new(GitHubOperationCell {
            state: Mutex::new(operation.clone()),
            process: Mutex::new(None),
            cancel_requested: AtomicBool::new(false),
            update_sink,
        });
        github_operation_registry()
            .operations
            .lock()
            .map_err(|_| {
                GitHubServiceError::new("github.operation-registry-poisoned", serde_json::json!({}))
            })?
            .insert(operation.operation_id.clone(), cell.clone());
        let cwd = cwd.to_path_buf();
        thread::spawn(move || run_github_pr_create(executable, cwd, input, cell));
        Ok(operation)
    }

    pub fn list_issues(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        query: &GitHubIssueQuery,
    ) -> Result<Vec<GitHubIssueSummary>> {
        validate_repository(repository)?;
        let repository = repository_selector(host, repository)?;
        let mut args = vec![
            "issue".to_string(),
            "list".to_string(),
            "--repo".to_string(),
            repository,
            "--state".to_string(),
            query.state.as_str().to_string(),
            "--limit".to_string(),
            GITHUB_LIST_LIMIT.to_string(),
            "--json".to_string(),
            "number,title,state,author,assignees,labels,updatedAt,url".to_string(),
        ];
        push_optional_flag(&mut args, "--author", query.author.as_deref())?;
        push_optional_flag(&mut args, "--assignee", query.assignee.as_deref())?;
        push_optional_flag(&mut args, "--label", query.label.as_deref())?;
        push_optional_flag(&mut args, "--milestone", query.milestone.as_deref())?;
        push_optional_flag(&mut args, "--search", query.search.as_deref())?;
        self.require_json(cwd, &args, "github.issue-list-failed")
    }

    pub fn issue_detail(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        number: u64,
    ) -> Result<GitHubIssueDetail> {
        validate_repository(repository)?;
        let repository = repository_selector(host, repository)?;
        self.require_json(
            cwd,
            &[
                "issue".into(),
                "view".into(),
                number.to_string(),
                "--repo".into(),
                repository,
                "--json".into(),
                "number,title,body,state,author,assignees,labels,updatedAt,url,milestone".into(),
            ],
            "github.issue-detail-failed",
        )
    }

    fn require_json<T: for<'de> Deserialize<'de>>(
        &self,
        cwd: &Utf8Path,
        args: &[String],
        code: &'static str,
    ) -> Result<T> {
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = self.run(cwd, &refs)?;
        if !output.success {
            return Err(classify_github_error(code, &output).into());
        }
        if output.stdout_truncated {
            return Err(
                GitHubServiceError::new("github.output-too-large", serde_json::json!({})).into(),
            );
        }
        serde_json::from_slice(&output.stdout).map_err(|_| {
            GitHubServiceError::new("github.invalid-json", serde_json::json!({})).into()
        })
    }

    fn validate_branch_name(&self, cwd: &Utf8Path, branch: &str, code: &'static str) -> Result<()> {
        if branch.trim() != branch
            || branch.is_empty()
            || branch.starts_with('-')
            || branch.contains('\0')
        {
            return Err(
                GitHubServiceError::new(code, serde_json::json!({ "branch": branch })).into(),
            );
        }
        let output = self.run_git(cwd, &["check-ref-format", "--branch", branch])?;
        if output.success {
            Ok(())
        } else {
            Err(GitHubServiceError::new(code, serde_json::json!({ "branch": branch })).into())
        }
    }

    fn git_revision(&self, cwd: &Utf8Path, revision: &str) -> Result<Option<String>> {
        let output = self.run_git(cwd, &["rev-parse", "--verify", "--quiet", revision])?;
        Ok(output
            .success
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty()))
    }

    fn require_git_revision(
        &self,
        cwd: &Utf8Path,
        revision: &str,
        code: &'static str,
        branch: &str,
    ) -> Result<String> {
        self.git_revision(cwd, revision)?.ok_or_else(|| {
            GitHubServiceError::new(code, serde_json::json!({ "branch": branch })).into()
        })
    }

    fn github_branch_exists(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<bool> {
        let endpoint = format!(
            "repos/{repository}/branches/{}",
            percent_encode_path_segment(branch)
        );
        let output = self.run(
            cwd,
            &[
                "api",
                "--hostname",
                host,
                "--method",
                "GET",
                &endpoint,
                "--silent",
            ],
        )?;
        if output.success {
            Ok(true)
        } else if github_output_is_not_found(&output) {
            Ok(false)
        } else {
            Err(classify_github_error("github.pr-head-query-failed", &output).into())
        }
    }

    fn pull_request_file_version(
        &self,
        cwd: &Utf8Path,
        host: &str,
        repository: &str,
        oid: &str,
        path: &str,
    ) -> Result<GitHubFileVersion> {
        let endpoint = github_contents_endpoint(repository, path, oid);
        let output = self.run(
            cwd,
            &[
                "api",
                "--hostname",
                host,
                "--method",
                "GET",
                "--header",
                "Accept: application/vnd.github.raw+json",
                &endpoint,
            ],
        )?;
        if output.success {
            if output.stdout_truncated {
                Ok(GitHubFileVersion::TooLarge)
            } else {
                Ok(GitHubFileVersion::Content(output.stdout))
            }
        } else if github_output_is_not_found(&output) {
            Ok(GitHubFileVersion::Missing)
        } else {
            Err(classify_github_error("github.pr-file-content-failed", &output).into())
        }
    }

    fn run_git(&self, cwd: &Utf8Path, args: &[&str]) -> Result<GitHubCommandOutput> {
        let executable = self
            .git_executable
            .as_ref()
            .ok_or_else(|| GitHubServiceError::new("git.not-installed", serde_json::json!({})))?;
        run_bounded_command(executable, cwd, args, None)
    }

    fn run(&self, cwd: &Utf8Path, args: &[&str]) -> Result<GitHubCommandOutput> {
        let executable = self.executable.as_ref().ok_or_else(|| {
            GitHubServiceError::new("github.gh-not-installed", serde_json::json!({}))
        })?;
        run_bounded_command(executable, cwd, args, None)
    }
}

fn run_bounded_command(
    executable: &std::path::Path,
    cwd: &Utf8Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<GitHubCommandOutput> {
    let mut command = background_command(executable);
    command
        .current_dir(cwd.as_std_path())
        .args(args)
        .env("LC_ALL", "C")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ManagedProcessGroup::spawn(&mut command)
        .with_context(|| "failed to execute background command")?;
    if let Some(input) = input {
        let mut stdin = child.take_stdin().ok_or_else(|| {
            GitHubServiceError::new("github.stdin-unavailable", serde_json::json!({}))
        })?;
        stdin.write_all(input)?;
        drop(stdin);
    }
    let stdout = child.take_stdout().ok_or_else(|| {
        GitHubServiceError::new("github.stdout-unavailable", serde_json::json!({}))
    })?;
    let stderr = child.take_stderr().ok_or_else(|| {
        GitHubServiceError::new("github.stderr-unavailable", serde_json::json!({}))
    })?;
    let stdout_reader = thread::spawn(move || read_bounded_output(stdout));
    let stderr_reader = thread::spawn(move || read_bounded_output(stderr));
    let status = child.wait()?;
    let stdout = stdout_reader.join().map_err(|_| {
        GitHubServiceError::new("github.output-reader-failed", serde_json::json!({}))
    })??;
    let stderr = stderr_reader.join().map_err(|_| {
        GitHubServiceError::new("github.output-reader-failed", serde_json::json!({}))
    })??;
    Ok(GitHubCommandOutput {
        success: status.success(),
        exit_code: status.code(),
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

fn read_bounded_output(mut reader: impl Read) -> std::io::Result<BoundedOutput> {
    let mut output = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = GITHUB_OUTPUT_LIMIT.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        truncated |= read > remaining;
    }
    Ok(BoundedOutput {
        bytes: output,
        truncated,
    })
}

fn github_operation_registry() -> &'static GitHubOperationRegistry {
    GITHUB_OPERATION_REGISTRY.get_or_init(GitHubOperationRegistry::default)
}

fn github_operation_cell(operation_id: &str) -> Result<Arc<GitHubOperationCell>> {
    github_operation_registry()
        .operations
        .lock()
        .map_err(|_| {
            GitHubServiceError::new("github.operation-registry-poisoned", serde_json::json!({}))
        })?
        .get(operation_id)
        .cloned()
        .ok_or_else(|| {
            GitHubServiceError::new(
                "github.operation-not-found",
                serde_json::json!({ "operationId": operation_id }),
            )
            .into()
        })
}

fn run_github_login(
    executable: PathBuf,
    cwd: camino::Utf8PathBuf,
    host: String,
    cell: Arc<GitHubOperationCell>,
) {
    if cell.cancel_requested.load(Ordering::SeqCst) {
        finish_github_operation(&cell, GitHubOperationStatus::Cancelled, None, None);
        return;
    }
    let running = if let Ok(mut state) = cell.state.lock() {
        state.status = GitHubOperationStatus::Running;
        state.started_at = Some(github_timestamp());
        Some(state.clone())
    } else {
        None
    };
    if let Some(operation) = running {
        emit_github_operation_update(&cell, operation);
    }
    let mut command = background_command(executable);
    command
        .current_dir(cwd.as_std_path())
        .args([
            "auth",
            "login",
            "--hostname",
            host.as_str(),
            "--git-protocol",
            "https",
            "--web",
            "--clipboard",
        ])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let process = ManagedProcessGroup::spawn(&mut command);
    let Ok(process) = process else {
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: "github.login-start-failed".to_string(),
                params: serde_json::json!({}),
            }),
            None,
        );
        return;
    };
    if let Ok(mut slot) = cell.process.lock() {
        *slot = Some(process);
    } else {
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: "github.operation-process-poisoned".to_string(),
                params: serde_json::json!({}),
            }),
            None,
        );
        return;
    }
    let status = loop {
        if cell.cancel_requested.load(Ordering::SeqCst) {
            if let Ok(mut slot) = cell.process.lock()
                && let Some(process) = slot.as_mut()
            {
                let _ = process.terminate(PROCESS_GROUP_TERMINATION_GRACE);
            }
        }
        let status = cell.process.lock().ok().and_then(|mut slot| {
            slot.as_mut()
                .and_then(|process| process.try_wait().ok().flatten())
        });
        if let Some(status) = status {
            break status;
        }
        thread::sleep(OPERATION_POLL_INTERVAL);
    };
    if let Ok(mut slot) = cell.process.lock() {
        drop(slot.take());
    }
    if cell.cancel_requested.load(Ordering::SeqCst) {
        finish_github_operation(&cell, GitHubOperationStatus::Cancelled, None, None);
    } else if status.success() {
        finish_github_operation(&cell, GitHubOperationStatus::Succeeded, None, None);
    } else {
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: "github.not-authenticated".to_string(),
                params: serde_json::json!({ "exitCode": status.code() }),
            }),
            None,
        );
    }
}

fn run_github_pr_create(
    executable: PathBuf,
    cwd: camino::Utf8PathBuf,
    input: GitHubPullRequestCreateInput,
    cell: Arc<GitHubOperationCell>,
) {
    if cell.cancel_requested.load(Ordering::SeqCst) {
        finish_github_operation(&cell, GitHubOperationStatus::Cancelled, None, None);
        return;
    }
    let running = if let Ok(mut state) = cell.state.lock() {
        state.status = GitHubOperationStatus::Running;
        state.started_at = Some(github_timestamp());
        Some(state.clone())
    } else {
        None
    };
    if let Some(operation) = running {
        emit_github_operation_update(&cell, operation);
    }
    let repository = match repository_selector(&input.host, &input.repository) {
        Ok(repository) => repository,
        Err(error) => {
            finish_github_operation(
                &cell,
                GitHubOperationStatus::Failed,
                Some(anyhow_github_operation_error(&error)),
                None,
            );
            return;
        }
    };
    let args = github_pr_create_args(&repository, &input);
    let mut command = background_command(executable);
    command
        .current_dir(cwd.as_std_path())
        .args(&args)
        .env("LC_ALL", "C")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = match ManagedProcessGroup::spawn(&mut command) {
        Ok(process) => process,
        Err(_) => {
            finish_github_operation(
                &cell,
                GitHubOperationStatus::Failed,
                Some(GitHubOperationError {
                    code: "github.pr-create-start-failed".to_string(),
                    params: serde_json::json!({}),
                }),
                None,
            );
            return;
        }
    };
    let stdin_result: Result<()> = (|| {
        let mut stdin = process.take_stdin().ok_or_else(|| {
            GitHubServiceError::new("github.stdin-unavailable", serde_json::json!({}))
        })?;
        stdin.write_all(input.body.as_bytes())?;
        drop(stdin);
        Ok(())
    })();
    if let Err(error) = stdin_result {
        let _ = process.terminate(PROCESS_GROUP_TERMINATION_GRACE);
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(anyhow_github_operation_error(&error)),
            None,
        );
        return;
    }
    let Some(stdout) = process.take_stdout() else {
        let _ = process.terminate(PROCESS_GROUP_TERMINATION_GRACE);
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: "github.stdout-unavailable".to_string(),
                params: serde_json::json!({}),
            }),
            None,
        );
        return;
    };
    let Some(stderr) = process.take_stderr() else {
        let _ = process.terminate(PROCESS_GROUP_TERMINATION_GRACE);
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: "github.stderr-unavailable".to_string(),
                params: serde_json::json!({}),
            }),
            None,
        );
        return;
    };
    let stdout_reader = thread::spawn(move || read_bounded_output(stdout));
    let stderr_reader = thread::spawn(move || read_bounded_output(stderr));
    if let Ok(mut slot) = cell.process.lock() {
        *slot = Some(process);
    } else {
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: "github.operation-process-poisoned".to_string(),
                params: serde_json::json!({}),
            }),
            None,
        );
        return;
    }
    let status = loop {
        if cell.cancel_requested.load(Ordering::SeqCst)
            && let Ok(mut slot) = cell.process.lock()
            && let Some(process) = slot.as_mut()
        {
            let _ = process.terminate(PROCESS_GROUP_TERMINATION_GRACE);
        }
        let status = cell.process.lock().ok().and_then(|mut slot| {
            slot.as_mut()
                .and_then(|process| process.try_wait().ok().flatten())
        });
        if let Some(status) = status {
            break status;
        }
        thread::sleep(OPERATION_POLL_INTERVAL);
    };
    if let Ok(mut slot) = cell.process.lock() {
        drop(slot.take());
    }
    let stdout = stdout_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let stderr = stderr_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    if cell.cancel_requested.load(Ordering::SeqCst) {
        finish_github_operation(&cell, GitHubOperationStatus::Cancelled, None, None);
    } else if status.success() {
        if stdout.truncated {
            finish_github_operation(
                &cell,
                GitHubOperationStatus::Failed,
                Some(GitHubOperationError {
                    code: "github.output-too-large".to_string(),
                    params: serde_json::json!({}),
                }),
                None,
            );
            return;
        }
        match pull_request_url(&stdout.bytes) {
            Some(url) => {
                finish_github_operation(&cell, GitHubOperationStatus::Succeeded, None, Some(url))
            }
            None => finish_github_operation(
                &cell,
                GitHubOperationStatus::Failed,
                Some(GitHubOperationError {
                    code: "github.pr-create-invalid-output".to_string(),
                    params: serde_json::json!({}),
                }),
                None,
            ),
        }
    } else {
        let output = GitHubCommandOutput {
            success: false,
            exit_code: status.code(),
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        };
        let error = classify_github_error("github.pr-create-failed", &output);
        finish_github_operation(
            &cell,
            GitHubOperationStatus::Failed,
            Some(GitHubOperationError {
                code: error.code.to_string(),
                params: error.params,
            }),
            None,
        );
    }
}

fn finish_github_operation(
    cell: &GitHubOperationCell,
    status: GitHubOperationStatus,
    error: Option<GitHubOperationError>,
    result_url: Option<String>,
) {
    let operation = if let Ok(mut state) = cell.state.lock() {
        state.status = status;
        state.cancelable = false;
        state.completed_at = Some(github_timestamp());
        state.error = error;
        state.result_url = result_url;
        Some(state.clone())
    } else {
        None
    };
    if let Some(operation) = operation {
        emit_github_operation_update(cell, operation);
    }
}

fn emit_github_operation_update(cell: &GitHubOperationCell, operation: GitHubOperation) {
    if let Some(sink) = &cell.update_sink {
        sink(operation);
    }
}

fn github_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn push_optional_flag(args: &mut Vec<String>, flag: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        if value.contains('\0') {
            return Err(
                GitHubServiceError::new("github.invalid-query", serde_json::json!({})).into(),
            );
        }
        args.extend([flag.to_string(), value.to_string()]);
    }
    Ok(())
}

fn validate_pull_request_text(input: &GitHubPullRequestCreateInput) -> Result<()> {
    let title = input.title.trim();
    if title.is_empty() || title.contains('\0') || title.contains(['\r', '\n']) {
        return Err(
            GitHubServiceError::new("github.pr-title-invalid", serde_json::json!({})).into(),
        );
    }
    if input.body.contains('\0') || input.body.len() > GITHUB_PR_BODY_LIMIT {
        return Err(GitHubServiceError::new(
            "github.pr-body-invalid",
            serde_json::json!({ "maxBytes": GITHUB_PR_BODY_LIMIT }),
        )
        .into());
    }
    Ok(())
}

fn github_pr_create_args(repository: &str, input: &GitHubPullRequestCreateInput) -> Vec<String> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        repository.to_string(),
        "--head".to_string(),
        input.head.clone(),
        "--base".to_string(),
        input.base.clone(),
        "--title".to_string(),
        input.title.trim().to_string(),
        "--body-file".to_string(),
        "-".to_string(),
    ];
    if input.draft {
        args.push("--draft".to_string());
    }
    args
}

fn pull_request_url(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| {
            url::Url::parse(line)
                .map(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
                .unwrap_or(false)
        })
        .map(str::to_string)
}

fn anyhow_github_operation_error(error: &anyhow::Error) -> GitHubOperationError {
    error
        .downcast_ref::<GitHubServiceError>()
        .map(|error| GitHubOperationError {
            code: error.code.to_string(),
            params: error.params.clone(),
        })
        .unwrap_or_else(|| GitHubOperationError {
            code: "github.operation-failed".to_string(),
            params: serde_json::json!({}),
        })
}

fn capability_error_code(status: GitHubCapabilityStatus) -> &'static str {
    match status {
        GitHubCapabilityStatus::NotInstalled => "github.gh-not-installed",
        GitHubCapabilityStatus::NotAuthenticated => "github.not-authenticated",
        GitHubCapabilityStatus::RepositoryUnresolved => "github.repository-unresolved",
        GitHubCapabilityStatus::Ready => "github.operation-failed",
    }
}

fn github_output_is_not_found(output: &GitHubCommandOutput) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("http 404") || stderr.contains("not found")
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn github_contents_endpoint(repository: &str, path: &str, oid: &str) -> String {
    let repository = repository
        .split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    let path = path
        .split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!(
        "repos/{repository}/contents/{path}?ref={}",
        percent_encode_path_segment(oid)
    )
}

fn validate_repository(repository: &str) -> Result<()> {
    let valid = repository.split('/').count() == 2
        && !repository.is_empty()
        && !repository.starts_with('-')
        && !repository.contains('\0');
    if valid {
        Ok(())
    } else {
        Err(GitHubServiceError::new("github.repository-unresolved", serde_json::json!({})).into())
    }
}

fn github_pull_request_file(file: GitHubApiPullRequestFile) -> Result<GitHubPullRequestFile> {
    validate_repo_relative_path(&file.filename)?;
    if let Some(previous_filename) = file.previous_filename.as_deref() {
        validate_repo_relative_path(previous_filename)?;
    }
    let kind = match file.status.as_str() {
        "added" => GitFileChangeKind::Added,
        "removed" => GitFileChangeKind::Deleted,
        "renamed" => GitFileChangeKind::Renamed,
        "copied" => GitFileChangeKind::Copied,
        "modified" | "changed" | "unchanged" => GitFileChangeKind::Modified,
        _ => {
            return Err(GitHubServiceError::new(
                "github.pr-file-status-unsupported",
                serde_json::json!({ "status": file.status }),
            )
            .into());
        }
    };
    Ok(GitHubPullRequestFile {
        path: file.filename,
        old_path: file.previous_filename,
        kind,
        additions: file.additions,
        deletions: file.deletions,
    })
}

fn github_pull_request_files_args(host: &str, repository: &str, number: u64) -> Vec<String> {
    vec![
        "api".to_string(),
        "--hostname".to_string(),
        host.to_string(),
        format!("repos/{repository}/pulls/{number}/files"),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn validate_github_oid(oid: &str) -> Result<()> {
    if matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitHubServiceError::new("github.pr-revision-invalid", serde_json::json!({})).into())
    }
}

fn repository_selector(host: &str, repository: &str) -> Result<String> {
    validate_host(host)?;
    let host = host.trim();
    Ok(if host.eq_ignore_ascii_case("github.com") {
        repository.to_string()
    } else {
        format!("{host}/{repository}")
    })
}

fn github_repository_view_args(repository: &str) -> Vec<String> {
    vec![
        "repo".to_string(),
        "view".to_string(),
        repository.to_string(),
        "--json".to_string(),
        "nameWithOwner,defaultBranchRef".to_string(),
    ]
}

fn validate_host(host: &str) -> Result<()> {
    let host = host.trim();
    if host.is_empty() || host.starts_with('-') || host.contains('/') || host.contains('\0') {
        Err(GitHubServiceError::new("github.repository-unresolved", serde_json::json!({})).into())
    } else {
        Ok(())
    }
}

fn classify_github_error(
    fallback: &'static str,
    output: &GitHubCommandOutput,
) -> GitHubServiceError {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let code = if stderr.contains("authentication") || stderr.contains("not logged") {
        "github.not-authenticated"
    } else if stderr.contains("rate limit") {
        "github.rate-limited"
    } else if stderr.contains("could not resolve to a repository") || stderr.contains("not found") {
        "github.repository-not-found"
    } else if stderr.contains("permission") || stderr.contains("forbidden") {
        "github.permission-denied"
    } else {
        fallback
    };
    GitHubServiceError::command(code, output)
}

fn parse_auth_hosts(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let hosts = value.get("hosts").unwrap_or(&value);
    let mut accounts = Vec::new();
    if let Some(map) = hosts.as_object() {
        for (host, entries) in map {
            collect_host_accounts(host, entries, &mut accounts);
        }
    }
    accounts.sort();
    accounts.dedup();
    accounts
}

fn collect_host_accounts(
    host: &str,
    value: &serde_json::Value,
    accounts: &mut Vec<(String, String)>,
) {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .for_each(|value| collect_host_accounts(host, value, accounts)),
        serde_json::Value::Object(map) => {
            if let Some(login) = map.get("login").and_then(serde_json::Value::as_str) {
                let authenticated = map
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    .map(|state| state.eq_ignore_ascii_case("success"))
                    .unwrap_or(true);
                if authenticated {
                    accounts.push((
                        map.get("host")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(host)
                            .to_string(),
                        login.to_string(),
                    ));
                }
            }
            map.values()
                .for_each(|value| collect_host_accounts(host, value, accounts));
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryMapping {
    host: String,
    repository: String,
    remote: Option<String>,
}

fn resolve_repository_mapping(
    remotes: &[GitRemote],
    upstream: Option<&str>,
) -> Option<RepositoryMapping> {
    if let Some(upstream) = upstream {
        if let Some(remote) = remotes
            .iter()
            .filter(|remote| upstream.starts_with(&format!("{}/", remote.name)))
            .max_by_key(|remote| remote.name.len())
        {
            if let Some(mapping) = remote_mapping(remote) {
                return Some(mapping);
            }
        }
    }
    if let Some(mapping) = remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .and_then(remote_mapping)
    {
        return Some(mapping);
    }
    let mut mappings = remotes
        .iter()
        .filter_map(remote_mapping)
        .collect::<Vec<_>>();
    mappings.sort_by(|left, right| {
        (&left.host, &left.repository, &left.remote).cmp(&(
            &right.host,
            &right.repository,
            &right.remote,
        ))
    });
    mappings.dedup_by(|left, right| left.host == right.host && left.repository == right.repository);
    (mappings.len() == 1).then(|| mappings.remove(0))
}

fn remote_mapping(remote: &GitRemote) -> Option<RepositoryMapping> {
    remote
        .fetch_urls
        .iter()
        .chain(&remote.push_urls)
        .find_map(|url| parse_github_remote(url))
        .map(|mut mapping| {
            mapping.remote = Some(remote.name.clone());
            mapping
        })
}

fn parse_github_remote(url: &str) -> Option<RepositoryMapping> {
    let normalized = url.trim().trim_end_matches('/');
    let (host, path) = if let Some(rest) = normalized.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        (host, path)
    } else if let Some(rest) = normalized.strip_prefix("ssh://") {
        let rest = rest.strip_prefix("git@").unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        (host, path)
    } else if let Some(rest) = normalized
        .strip_prefix("https://")
        .or_else(|| normalized.strip_prefix("http://"))
    {
        let (host, path) = rest.split_once('/')?;
        (host, path)
    } else {
        return None;
    };
    let repository = path.trim_start_matches('/').trim_end_matches(".git");
    if repository.split('/').count() != 2 {
        return None;
    }
    Some(RepositoryMapping {
        host: host.to_ascii_lowercase(),
        repository: repository.to_string(),
        remote: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn run_test_command(executable: &std::path::Path, cwd: &Utf8Path, args: &[&str]) -> String {
        let output = background_command(executable)
            .current_dir(cwd.as_std_path())
            .args(args)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn create_fake_gh(directory: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
        let args_path = directory.join("pr-create-args.txt");
        let body_path = directory.join("pr-create-body.md");
        #[cfg(windows)]
        let executable = directory.join("gh.cmd");
        #[cfg(not(windows))]
        let executable = directory.join("gh");

        #[cfg(windows)]
        let script = format!(
            "@echo off\r\n\
             if \"%1\"==\"--version\" (echo gh version 9.9.9& exit /b 0)\r\n\
             if \"%1\"==\"auth\" (echo {{\"hosts\":{{\"github.com\":[{{\"host\":\"github.com\",\"login\":\"octocat\",\"state\":\"success\"}}]}}}}& exit /b 0)\r\n\
             if \"%1\"==\"repo\" (echo {{\"nameWithOwner\":\"acme/widgets\",\"defaultBranchRef\":{{\"name\":\"main\"}}}}& exit /b 0)\r\n\
             if \"%1\"==\"api\" (exit /b 0)\r\n\
             if \"%1\"==\"pr\" if \"%2\"==\"list\" (echo []& exit /b 0)\r\n\
             if \"%1\"==\"pr\" if \"%2\"==\"create\" (echo %* > \"{}\"& more > \"{}\"& echo https://github.com/acme/widgets/pull/42& exit /b 0)\r\n\
             exit /b 1\r\n",
            args_path.display(),
            body_path.display()
        );
        #[cfg(not(windows))]
        let script = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then echo 'gh version 9.9.9'; exit 0; fi\n\
             if [ \"$1\" = \"auth\" ]; then echo '{{\"hosts\":{{\"github.com\":[{{\"host\":\"github.com\",\"login\":\"octocat\",\"state\":\"success\"}}]}}}}'; exit 0; fi\n\
             if [ \"$1\" = \"repo\" ]; then echo '{{\"nameWithOwner\":\"acme/widgets\",\"defaultBranchRef\":{{\"name\":\"main\"}}}}'; exit 0; fi\n\
             if [ \"$1\" = \"api\" ]; then exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf '%s\\n' \"$*\" > '{}'; cat > '{}'; echo 'https://github.com/acme/widgets/pull/42'; exit 0; fi\n\
             exit 1\n",
            args_path.display(),
            body_path.display()
        );
        std::fs::write(&executable, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).unwrap();
        }
        (executable, args_path, body_path)
    }

    fn create_fake_pr_diff_gh(directory: &std::path::Path) -> PathBuf {
        #[cfg(windows)]
        let executable = directory.join("gh-pr-diff.cmd");
        #[cfg(not(windows))]
        let executable = directory.join("gh-pr-diff");

        #[cfg(windows)]
        let script = "@echo off\r\n\
            if \"%1\"==\"api\" goto api\r\n\
            exit /b 1\r\n\
            :api\r\n\
            echo %8 | findstr /C:\"missing.txt\" >nul && goto notfound\r\n\
            echo %8 | findstr /C:\"added.txt\" >nul && echo %8 | findstr /C:\"ref=1111111111111111111111111111111111111111\" >nul && goto notfound\r\n\
            echo %8 | findstr /C:\"removed.txt\" >nul && echo %8 | findstr /C:\"ref=2222222222222222222222222222222222222222\" >nul && goto notfound\r\n\
            echo %8 | findstr /C:\"ref=1111111111111111111111111111111111111111\" >nul && goto base\r\n\
            echo head-content\r\n\
            exit /b 0\r\n\
            :base\r\n\
            echo base-content\r\n\
            exit /b 0\r\n\
            :notfound\r\n\
            echo HTTP 404: Not Found 1>&2\r\n\
            exit /b 1\r\n";
        #[cfg(not(windows))]
        let script = "#!/bin/sh\n\
            if [ \"$1\" = \"api\" ]; then\n\
              case \"$8\" in *missing.txt*) echo 'HTTP 404: Not Found' >&2; exit 1;; esac\n\
              case \"$8\" in *added.txt*ref=1111111111111111111111111111111111111111*) echo 'HTTP 404: Not Found' >&2; exit 1;; esac\n\
              case \"$8\" in *removed.txt*ref=2222222222222222222222222222222222222222*) echo 'HTTP 404: Not Found' >&2; exit 1;; esac\n\
              case \"$8\" in *ref=1111111111111111111111111111111111111111*) echo 'base-content'; exit 0;; esac\n\
              echo 'head-content'; exit 0\n\
            fi\n\
            exit 1\n";
        std::fs::write(&executable, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).unwrap();
        }
        executable
    }

    #[test]
    fn parses_auth_hosts_without_token_fields() {
        let accounts = parse_auth_hosts(br#"{"hosts":{"github.com":[{"active":true,"host":"github.com","login":"octocat","state":"success"}]}}"#);
        assert_eq!(
            accounts,
            vec![("github.com".to_string(), "octocat".to_string())]
        );
    }

    #[test]
    fn maps_https_scp_and_ssh_github_remotes() {
        for url in [
            "https://github.com/openai/codex.git",
            "git@github.com:openai/codex.git",
            "ssh://git@github.com/openai/codex.git",
        ] {
            assert_eq!(
                parse_github_remote(url).unwrap(),
                RepositoryMapping {
                    host: "github.com".to_string(),
                    repository: "openai/codex".to_string(),
                    remote: None,
                }
            );
        }
    }

    #[test]
    fn repository_mapping_prefers_upstream_then_origin() {
        let remote = |name: &str, url: &str| GitRemote {
            name: name.to_string(),
            fetch_urls: vec![url.to_string()],
            push_urls: vec![],
        };
        let remotes = vec![
            remote("origin", "https://github.com/acme/origin.git"),
            remote("fork", "https://github.com/me/fork.git"),
        ];
        assert_eq!(
            resolve_repository_mapping(&remotes, Some("fork/main"))
                .unwrap()
                .repository,
            "me/fork"
        );
        assert_eq!(
            resolve_repository_mapping(&remotes, Some("fork/main"))
                .unwrap()
                .remote
                .as_deref(),
            Some("fork")
        );
        assert_eq!(
            resolve_repository_mapping(&remotes, None)
                .unwrap()
                .repository,
            "acme/origin"
        );
    }

    #[test]
    fn repository_view_uses_the_supported_positional_repository_argument() {
        assert_eq!(
            github_repository_view_args("acme/widgets"),
            vec![
                "repo",
                "view",
                "acme/widgets",
                "--json",
                "nameWithOwner,defaultBranchRef",
            ]
        );
    }

    #[test]
    fn pull_request_create_contract_uses_body_stdin() {
        let input = GitHubPullRequestCreateInput {
            host: "github.com".to_string(),
            repository: "acme/widgets".to_string(),
            head: "feature/atomic-body".to_string(),
            base: "main".to_string(),
            title: "  Add atomic body  ".to_string(),
            body: "Body with\nmultiple lines".to_string(),
            draft: true,
        };
        validate_pull_request_text(&input).unwrap();
        let args = github_pr_create_args("acme/widgets", &input);
        assert_eq!(
            args,
            vec![
                "pr",
                "create",
                "--repo",
                "acme/widgets",
                "--head",
                "feature/atomic-body",
                "--base",
                "main",
                "--title",
                "Add atomic body",
                "--body-file",
                "-",
                "--draft",
            ]
        );
        assert!(!args.iter().any(|arg| arg.contains("multiple lines")));
    }

    #[test]
    fn parses_created_pull_request_url_and_encodes_branch() {
        assert_eq!(
            pull_request_url(b"https://github.com/acme/widgets/pull/42\n").as_deref(),
            Some("https://github.com/acme/widgets/pull/42")
        );
        assert_eq!(
            percent_encode_path_segment("feature/中文"),
            "feature%2F%E4%B8%AD%E6%96%87"
        );
        assert_eq!(
            github_contents_endpoint("acme/widgets", "docs/hello world.md", "base/oid"),
            "repos/acme/widgets/contents/docs/hello%20world.md?ref=base%2Foid"
        );
    }

    #[test]
    fn maps_pull_request_file_statuses_to_typed_diff_kinds() {
        for (status, expected) in [
            ("added", GitFileChangeKind::Added),
            ("removed", GitFileChangeKind::Deleted),
            ("renamed", GitFileChangeKind::Renamed),
            ("copied", GitFileChangeKind::Copied),
            ("modified", GitFileChangeKind::Modified),
        ] {
            let file = github_pull_request_file(GitHubApiPullRequestFile {
                filename: "src/new.rs".to_string(),
                status: status.to_string(),
                previous_filename: (status == "renamed").then(|| "src/old.rs".to_string()),
                additions: 3,
                deletions: 1,
            })
            .unwrap();
            assert_eq!(file.kind, expected);
            assert_eq!(
                file.old_path.as_deref(),
                (status == "renamed").then_some("src/old.rs")
            );
        }
    }

    #[test]
    fn pull_request_files_contract_uses_one_paginated_api_query() {
        assert_eq!(
            github_pull_request_files_args("github.example.com", "acme/widgets", 42),
            vec![
                "api",
                "--hostname",
                "github.example.com",
                "repos/acme/widgets/pulls/42/files",
                "--paginate",
                "--slurp",
            ]
        );
    }

    #[test]
    fn rejects_unknown_pull_request_file_status() {
        let error = github_pull_request_file(GitHubApiPullRequestFile {
            filename: "src/app.rs".to_string(),
            status: "mystery".to_string(),
            previous_filename: None,
            additions: 0,
            deletions: 0,
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("github.pr-file-status-unsupported")
        );
    }

    #[test]
    fn bounded_output_reports_truncation_without_stopping_the_reader() {
        let bytes = vec![b'x'; GITHUB_OUTPUT_LIMIT + 17];
        let output = read_bounded_output(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(output.bytes.len(), GITHUB_OUTPUT_LIMIT);
        assert!(output.truncated);
    }

    #[test]
    fn fake_gh_pull_request_comparison_covers_modified_added_and_deleted_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let service = GitHubCliService {
            executable: Some(create_fake_pr_diff_gh(temp.path())),
            git_executable: None,
        };
        let base_oid = "1111111111111111111111111111111111111111";
        let head_oid = "2222222222222222222222222222222222222222";

        let modified = service
            .pull_request_revision_comparison(
                &root,
                "github.com",
                "acme/widgets",
                42,
                base_oid,
                head_oid,
                "src/app.ts",
                None,
            )
            .unwrap();
        assert_eq!(modified.before.unwrap().content.trim(), "base-content");
        assert_eq!(modified.after.unwrap().content.trim(), "head-content");
        assert_eq!(
            (modified.stats.added_lines, modified.stats.deleted_lines),
            (1, 1)
        );

        let added = service
            .pull_request_revision_comparison(
                &root,
                "github.com",
                "acme/widgets",
                42,
                base_oid,
                head_oid,
                "added.txt",
                None,
            )
            .unwrap();
        assert!(added.before.is_none());
        assert_eq!(added.after.unwrap().content.trim(), "head-content");
        assert_eq!((added.stats.added_lines, added.stats.deleted_lines), (1, 0));

        let removed = service
            .pull_request_revision_comparison(
                &root,
                "github.com",
                "acme/widgets",
                42,
                base_oid,
                head_oid,
                "removed.txt",
                None,
            )
            .unwrap();
        assert_eq!(removed.before.unwrap().content.trim(), "base-content");
        assert!(removed.after.is_none());
        assert_eq!(
            (removed.stats.added_lines, removed.stats.deleted_lines),
            (0, 1)
        );

        let error = service
            .pull_request_revision_comparison(
                &root,
                "github.com",
                "acme/widgets",
                42,
                base_oid,
                head_oid,
                "missing.txt",
                None,
            )
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<GitHubServiceError>().unwrap().code,
            "github.pr-file-content-unavailable"
        );

        let invalid_revision = service
            .pull_request_revision_comparison(
                &root,
                "github.com",
                "acme/widgets",
                42,
                "not-an-oid",
                head_oid,
                "src/app.ts",
                None,
            )
            .unwrap_err();
        assert_eq!(
            invalid_revision
                .downcast_ref::<GitHubServiceError>()
                .unwrap()
                .code,
            "github.pr-revision-invalid"
        );
    }

    #[test]
    fn renamed_pull_request_file_reads_the_previous_path_at_the_base_revision() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let service = GitHubCliService {
            executable: Some(create_fake_pr_diff_gh(temp.path())),
            git_executable: None,
        };
        let comparison = service
            .pull_request_revision_comparison(
                &root,
                "github.com",
                "acme/widgets",
                42,
                "1111111111111111111111111111111111111111",
                "2222222222222222222222222222222222222222",
                "renamed.txt",
                Some("src/app.ts"),
            )
            .unwrap();
        assert_eq!(comparison.before.unwrap().content.trim(), "base-content");
        assert_eq!(comparison.after.unwrap().content.trim(), "head-content");
    }

    #[test]
    fn fake_gh_preflight_and_create_preserve_typed_contract() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        std::fs::create_dir_all(root.as_std_path()).unwrap();
        let git = find_executable_in_path("git").expect("git is required for repository tests");
        run_test_command(&git, &root, &["init", "--initial-branch=main"]);
        run_test_command(&git, &root, &["config", "user.name", "Gold Band Test"]);
        run_test_command(
            &git,
            &root,
            &["config", "user.email", "gold-band@example.test"],
        );
        std::fs::write(root.join("README.md"), "base\n").unwrap();
        run_test_command(&git, &root, &["add", "--", "README.md"]);
        run_test_command(&git, &root, &["commit", "-m", "base"]);
        let base_oid = run_test_command(&git, &root, &["rev-parse", "HEAD"]);
        run_test_command(&git, &root, &["switch", "-c", "feature/pr-create"]);
        std::fs::write(root.join("README.md"), "base\nfeature\n").unwrap();
        run_test_command(&git, &root, &["add", "--", "README.md"]);
        run_test_command(&git, &root, &["commit", "-m", "feature"]);
        let head_oid = run_test_command(&git, &root, &["rev-parse", "HEAD"]);
        run_test_command(
            &git,
            &root,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widgets.git",
            ],
        );
        run_test_command(
            &git,
            &root,
            &["update-ref", "refs/remotes/origin/main", &base_oid],
        );
        run_test_command(
            &git,
            &root,
            &[
                "update-ref",
                "refs/remotes/origin/feature/pr-create",
                &head_oid,
            ],
        );
        let (fake_gh, args_path, body_path) = create_fake_gh(temp.path());
        let service = GitHubCliService {
            executable: Some(fake_gh),
            git_executable: Some(git),
        };
        let preflight_input = GitHubPullRequestPreflightInput {
            host: "github.com".to_string(),
            repository: "acme/widgets".to_string(),
            head: "feature/pr-create".to_string(),
            base: "main".to_string(),
        };
        let preflight = service
            .preflight_pull_request(&root, &preflight_input)
            .unwrap();
        assert_eq!(preflight.remote, "origin");
        assert_eq!(preflight.ahead_by, 1);
        assert!(preflight.head_published);
        assert!(preflight.existing_pull_request.is_none());

        let body = "## Summary\n\nCreated through stdin.";
        let (sender, receiver) = std::sync::mpsc::channel();
        let update_sink: GitHubOperationUpdateSink = Arc::new(move |operation| {
            let _ = sender.send(operation);
        });
        let operation = service
            .start_pull_request_create_with_update_sink(
                &root,
                GitHubPullRequestCreateInput {
                    host: preflight_input.host,
                    repository: preflight_input.repository,
                    head: preflight_input.head,
                    base: preflight_input.base,
                    title: "Create from typed input".to_string(),
                    body: body.to_string(),
                    draft: true,
                },
                Some(update_sink),
            )
            .unwrap();
        let mut updates = Vec::new();
        let completed = loop {
            let current = receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("fake gh create operation should emit updates");
            let terminal = matches!(
                current.status,
                GitHubOperationStatus::Succeeded
                    | GitHubOperationStatus::Failed
                    | GitHubOperationStatus::Cancelled
            );
            updates.push(current.clone());
            if terminal {
                break current;
            }
        };
        assert!(
            updates
                .iter()
                .any(|update| update.status == GitHubOperationStatus::Running)
        );
        assert_eq!(completed.operation_id, operation.operation_id);
        assert_eq!(completed.status, GitHubOperationStatus::Succeeded);
        assert_eq!(
            completed.result_url.as_deref(),
            Some("https://github.com/acme/widgets/pull/42")
        );
        let args = std::fs::read_to_string(args_path).unwrap();
        assert!(args.contains("--body-file -"));
        assert!(args.contains("--draft"));
        assert!(!args.contains("Created through stdin"));
        let actual_body = std::fs::read_to_string(body_path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(actual_body.trim_end(), body);
    }
}
