use std::process::Output;
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use semver::{Prerelease, Version};
use serde::{Deserialize, Serialize};

use crate::process::background_command;
use crate::runtime_error::{RuntimeErrorDomain, manual_runtime_error_info, runtime_error};

mod github;
mod source_control;

pub use github::*;
pub use source_control::*;

const CHECKPOINT_AUTHOR_NAME: &str = "Gold Band Runtime";
const CHECKPOINT_AUTHOR_EMAIL: &str = "runtime@gold-band.local";
pub const MINIMUM_SUPPORTED_GIT_VERSION: &str = "2.36.0";

fn minimum_supported_git_version() -> Version {
    Version::new(2, 36, 0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for GitCommandOutput {
    fn from(output: Output) -> Self {
        Self {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitCapabilityStatus {
    Ready,
    NotInstalled,
    VersionUnsupported,
    VersionUnavailable,
    RepositoryRequired,
    HeadRequired,
    WorktreeRequired,
    RepositoryUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCapability {
    pub status: GitCapabilityStatus,
    pub installed_version: Option<String>,
    pub minimum_version: String,
    pub repo_root: Option<Utf8PathBuf>,
    pub common_dir: Option<Utf8PathBuf>,
    pub head: Option<String>,
}

impl GitCapability {
    fn new(status: GitCapabilityStatus, installed_version: Option<String>) -> Self {
        Self {
            status,
            installed_version,
            minimum_version: MINIMUM_SUPPORTED_GIT_VERSION.to_string(),
            repo_root: None,
            common_dir: None,
            head: None,
        }
    }

    pub fn ready(&self) -> bool {
        self.status == GitCapabilityStatus::Ready
    }

    pub fn error_code(&self) -> Option<&'static str> {
        match self.status {
            GitCapabilityStatus::Ready => None,
            GitCapabilityStatus::NotInstalled => Some("run.git-not-installed"),
            GitCapabilityStatus::VersionUnsupported => Some("run.git-version-unsupported"),
            GitCapabilityStatus::VersionUnavailable => Some("run.git-version-unavailable"),
            GitCapabilityStatus::RepositoryRequired => Some("run.git-repository-required"),
            GitCapabilityStatus::HeadRequired => Some("run.git-head-required"),
            GitCapabilityStatus::WorktreeRequired => Some("run.git-worktree-required"),
            GitCapabilityStatus::RepositoryUnavailable => Some("run.git-repository-unavailable"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct GitPreflightError {
    pub code: &'static str,
    pub capability: GitCapability,
}

impl GitPreflightError {
    pub fn params(&self) -> serde_json::Value {
        serde_json::json!({
            "repoRoot": self.capability.repo_root,
            "commonDir": self.capability.common_dir,
            "head": self.capability.head,
            "installedVersion": self.capability.installed_version,
            "minimumVersion": self.capability.minimum_version,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstalledGitVersion {
    display: String,
    semantic: Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GitVersionProbe {
    NotInstalled,
    VersionUnavailable,
    Installed(InstalledGitVersion),
}

fn probe_git_version() -> GitVersionProbe {
    let output = match background_command("git")
        .arg("--version")
        .env("LC_ALL", "C")
        .output()
    {
        Ok(output) => output,
        Err(_) => return GitVersionProbe::NotInstalled,
    };
    if !output.status.success() {
        return GitVersionProbe::VersionUnavailable;
    }
    let Ok(stdout) = std::str::from_utf8(&output.stdout) else {
        return GitVersionProbe::VersionUnavailable;
    };
    parse_git_version(stdout)
        .map(GitVersionProbe::Installed)
        .unwrap_or(GitVersionProbe::VersionUnavailable)
}

fn parse_git_version(output: &str) -> Option<InstalledGitVersion> {
    let display = output
        .trim()
        .strip_prefix("git version ")?
        .split_whitespace()
        .next()?
        .to_string();
    let bytes = display.as_bytes();
    let mut cursor = 0;
    let major = parse_version_component(bytes, &mut cursor)?;
    if bytes.get(cursor) != Some(&b'.') {
        return None;
    }
    cursor += 1;
    let minor = parse_version_component(bytes, &mut cursor)?;
    if bytes.get(cursor) != Some(&b'.') {
        return None;
    }
    cursor += 1;
    let patch = parse_version_component(bytes, &mut cursor)?;
    let mut semantic = Version::new(major, minor, patch);
    let suffix = &display[cursor..];
    if !suffix.is_empty() && !suffix.starts_with(['.', '-']) {
        return None;
    }
    if let Some(rc) = suffix
        .strip_prefix("-rc")
        .or_else(|| suffix.strip_prefix(".rc"))
    {
        semantic.pre = Prerelease::new(&format!("rc{rc}")).ok()?;
    }
    Some(InstalledGitVersion { display, semantic })
}

fn parse_version_component(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let start = *cursor;
    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
        *cursor += 1;
    }
    (start != *cursor).then(|| {
        std::str::from_utf8(&bytes[start..*cursor])
            .ok()?
            .parse()
            .ok()
    })?
}

fn supported_git_version() -> std::result::Result<InstalledGitVersion, GitCapability> {
    match probe_git_version() {
        GitVersionProbe::NotInstalled => {
            Err(GitCapability::new(GitCapabilityStatus::NotInstalled, None))
        }
        GitVersionProbe::VersionUnavailable => Err(GitCapability::new(
            GitCapabilityStatus::VersionUnavailable,
            None,
        )),
        GitVersionProbe::Installed(version)
            if version.semantic < minimum_supported_git_version() =>
        {
            Err(GitCapability::new(
                GitCapabilityStatus::VersionUnsupported,
                Some(version.display),
            ))
        }
        GitVersionProbe::Installed(version) => Ok(version),
    }
}

pub(crate) fn require_supported_git_version_for_service() -> Result<()> {
    match supported_git_version() {
        Ok(_) => Ok(()),
        Err(capability) => {
            let code = match capability.status {
                GitCapabilityStatus::NotInstalled => "git.not-installed",
                GitCapabilityStatus::VersionUnsupported => "git.version-unsupported",
                GitCapabilityStatus::VersionUnavailable => "git.version-unavailable",
                _ => "git.version-unavailable",
            };
            Err(GitServiceError::new(code, capability_version_params(&capability)).into())
        }
    }
}

fn capability_version_params(capability: &GitCapability) -> serde_json::Value {
    serde_json::json!({
        "installedVersion": capability.installed_version,
        "minimumVersion": capability.minimum_version,
    })
}

#[derive(Debug, Clone, Default)]
pub struct GitCommandRunner;

impl GitCommandRunner {
    pub fn run(&self, cwd: &Utf8Path, args: &[&str]) -> Result<GitCommandOutput> {
        background_command("git")
            .arg("-C")
            .arg(cwd.as_str())
            .args(args)
            .output()
            .map(GitCommandOutput::from)
            .with_context(|| format!("failed to execute Git in `{cwd}`"))
    }

    fn capture(&self, cwd: &Utf8Path, args: &[&str]) -> Option<String> {
        let output = self.run(cwd, args).ok()?;
        output.success.then_some(output.stdout)
    }
}

#[derive(Debug, Clone, Default)]
pub struct GitRepositoryService {
    runner: GitCommandRunner,
}

impl GitRepositoryService {
    pub fn initialize(&self, cwd: &Utf8Path) -> Result<GitCapability> {
        if let Err(capability) = supported_git_version() {
            return Ok(capability);
        }
        let output = self.runner.run(cwd, &["init"])?;
        ensure!(output.success, "git init failed: {}", details(&output));
        Ok(self.probe(cwd))
    }

    pub fn probe(&self, cwd: &Utf8Path) -> GitCapability {
        let version = match supported_git_version() {
            Ok(version) => version,
            Err(capability) => return capability,
        };
        let installed_version = Some(version.display);

        let Some(inside) = self
            .runner
            .capture(cwd, &["rev-parse", "--is-inside-work-tree"])
        else {
            return GitCapability::new(GitCapabilityStatus::RepositoryRequired, installed_version);
        };
        if inside != "true" {
            return GitCapability::new(GitCapabilityStatus::RepositoryRequired, installed_version);
        }

        let repo_root = self
            .runner
            .capture(cwd, &["rev-parse", "--show-toplevel"])
            .and_then(|path| canonical_git_path(&path));
        let common_dir = self
            .runner
            .capture(
                cwd,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .and_then(|path| canonical_git_path(&path));
        if repo_root.is_none() || common_dir.is_none() {
            let mut capability = GitCapability::new(
                GitCapabilityStatus::RepositoryUnavailable,
                installed_version,
            );
            capability.repo_root = repo_root;
            capability.common_dir = common_dir;
            return capability;
        }
        let head = self.runner.capture(cwd, &["rev-parse", "--verify", "HEAD"]);
        if head.is_none() {
            let mut capability =
                GitCapability::new(GitCapabilityStatus::HeadRequired, installed_version);
            capability.repo_root = repo_root;
            capability.common_dir = common_dir;
            capability.head = head;
            return capability;
        }
        if self
            .runner
            .capture(cwd, &["worktree", "list", "--porcelain", "-z"])
            .is_none()
        {
            let mut capability =
                GitCapability::new(GitCapabilityStatus::WorktreeRequired, installed_version);
            capability.repo_root = repo_root;
            capability.common_dir = common_dir;
            capability.head = head;
            return capability;
        }
        let mut capability = GitCapability::new(GitCapabilityStatus::Ready, installed_version);
        capability.repo_root = repo_root;
        capability.common_dir = common_dir;
        capability.head = head;
        capability
    }

    pub fn require_worktree(&self, cwd: &Utf8Path) -> Result<GitCapability> {
        let capability = self.probe(cwd);
        if let Some(code) = capability.error_code() {
            return Err(GitPreflightError { code, capability }.into());
        }
        Ok(capability)
    }

    pub fn head(&self, cwd: &Utf8Path) -> Result<String> {
        let output = self.runner.run(cwd, &["rev-parse", "HEAD"])?;
        ensure!(
            output.success,
            "git rev-parse HEAD failed: {}",
            details(&output)
        );
        Ok(output.stdout)
    }

    pub fn status_porcelain(&self, cwd: &Utf8Path) -> Result<String> {
        let output = self
            .runner
            .run(cwd, &["status", "--porcelain=v1", "--untracked-files=all"])?;
        ensure!(output.success, "git status failed: {}", details(&output));
        Ok(output.stdout)
    }
}

#[derive(Debug, Clone, Default)]
pub struct GitWorkspaceManager {
    runner: GitCommandRunner,
    repository: GitRepositoryService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeRegistrationStatus {
    AlreadyRegistered,
    Repaired,
}

impl GitWorkspaceManager {
    /// Ensures that Git's main-repository worktree catalog points back to an
    /// existing linked worktree. Parent-directory moves can leave the
    /// worktree usable from inside while the main repository still records
    /// its previous path.
    pub fn ensure_worktree_registration(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
    ) -> Result<WorktreeRegistrationStatus> {
        let repository = GitSourceControlService::default().repository_identity(repository_root)?;
        GitCoordinationService.with_runtime_write(
            &repository.common_dir,
            Some(path),
            "runtime-worktree-registration-repair",
            || self.ensure_worktree_registration_unlocked(repository_root, path, &repository, None),
        )
    }

    fn ensure_worktree_registration_unlocked(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        repository: &GitRepositoryIdentity,
        known_workspace: Option<&GitRepositoryIdentity>,
    ) -> Result<WorktreeRegistrationStatus> {
        ensure!(path.is_dir(), "Git worktree path is missing: {path}");
        let source_control = GitSourceControlService::default();
        if let Some(worktree) = registered_worktree(&source_control, repository_root, path)? {
            ensure!(!worktree.main, "workspace path is the main Git worktree");
            return Ok(WorktreeRegistrationStatus::AlreadyRegistered);
        }
        let workspace = known_workspace
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| source_control.repository_identity(path))?;
        ensure!(
            same_filesystem_path(&workspace.workspace_path, path)?,
            "workspace path is not the expected Git worktree"
        );
        ensure!(
            same_filesystem_path(&repository.common_dir, &workspace.common_dir)?,
            "Git worktree belongs to a different repository"
        );
        let git_dir = self
            .runner
            .run(path, &["rev-parse", "--path-format=absolute", "--git-dir"])?;
        ensure!(
            git_dir.success,
            "git rev-parse --git-dir failed: {}",
            details(&git_dir)
        );
        ensure!(
            !same_filesystem_path(Utf8Path::new(&git_dir.stdout), &workspace.common_dir)?,
            "workspace path is the main Git worktree"
        );

        let repair = self
            .runner
            .run(repository_root, &["worktree", "repair", path.as_str()])?;
        ensure!(
            repair.success,
            "git worktree repair failed: {}",
            details(&repair)
        );
        ensure!(
            registered_worktree(&source_control, repository_root, path)?.is_some(),
            "git worktree repair did not register the requested path"
        );
        Ok(WorktreeRegistrationStatus::Repaired)
    }

    /// Creates a runtime-owned worktree, or validates the durable identity
    /// when a previous attempt already completed the Git operation.
    pub fn ensure_worktree(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        branch: &str,
        fork_commit: &str,
    ) -> Result<()> {
        let existed = path.exists();
        if !existed {
            if let Err(error) = self.create_worktree(repository_root, path, branch, fork_commit) {
                if let Ok(identity) =
                    GitSourceControlService::default().repository_identity(repository_root)
                {
                    let _ = GitCoordinationService.with_runtime_write(
                        &identity.common_dir,
                        None,
                        "runtime-worktree-create-cleanup",
                        || {
                            let _ = self.runner.run(
                                repository_root,
                                &["worktree", "remove", "--force", path.as_str()],
                            );
                            let _ = self.runner.run(repository_root, &["branch", "-D", branch]);
                            Ok(())
                        },
                    );
                }
                return Err(error);
            }
        }

        let validation_started_at = Instant::now();
        let validation_result = self.validate_worktree(path, branch);
        tracing::info!(
            target: "gold_band::perf",
            repository_root = repository_root.as_str(),
            workspace_root = path.as_str(),
            branch,
            mode = if existed { "existing" } else { "created" },
            elapsed_ms = validation_started_at.elapsed().as_millis(),
            status = if validation_result.is_ok() { "ok" } else { "error" },
            "Git worktree validation completed"
        );
        validation_result
    }

    pub fn checkpoint(
        &self,
        workspace: &Utf8Path,
        workspace_id: &str,
        group_id: Option<&str>,
    ) -> Result<Option<String>> {
        let identity = GitSourceControlService::default().repository_identity(workspace)?;
        GitCoordinationService.with_runtime_write(
            &identity.common_dir,
            Some(&identity.workspace_path),
            "runtime-checkpoint",
            || self.checkpoint_unlocked(workspace, workspace_id, group_id),
        )
    }

    fn checkpoint_unlocked(
        &self,
        workspace: &Utf8Path,
        workspace_id: &str,
        group_id: Option<&str>,
    ) -> Result<Option<String>> {
        if self.repository.status_porcelain(workspace)?.is_empty() {
            return Ok(None);
        }
        self.ensure_no_in_progress_operation(workspace)?;
        let add = self.runner.run(workspace, &["add", "-A"])?;
        ensure!(add.success, "git add -A failed: {}", details(&add));

        let mut message = format!(
            "Gold Band checkpoint: {workspace_id}\n\nGold-Band-Internal: checkpoint\nGold-Band-Workspace: {workspace_id}"
        );
        if let Some(group_id) = group_id {
            message.push_str(&format!("\nGold-Band-Group: {group_id}"));
        }
        let commit = self.runner.run(
            workspace,
            &[
                "-c",
                &format!("user.name={CHECKPOINT_AUTHOR_NAME}"),
                "-c",
                &format!("user.email={CHECKPOINT_AUTHOR_EMAIL}"),
                "-c",
                "commit.gpgSign=false",
                "commit",
                "--no-verify",
                "--no-gpg-sign",
                "-m",
                &message,
            ],
        )?;
        ensure!(
            commit.success,
            "checkpoint commit failed: {}",
            details(&commit)
        );
        ensure!(
            self.repository.status_porcelain(workspace)?.is_empty(),
            "workspace remained dirty after checkpoint"
        );
        self.repository.head(workspace).map(Some)
    }

    pub fn create_worktree(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        branch: &str,
        fork_commit: &str,
    ) -> Result<()> {
        let identity = GitSourceControlService::default().repository_identity(repository_root)?;
        let lock_wait_started_at = Instant::now();
        GitCoordinationService.with_runtime_write(
            &identity.common_dir,
            None,
            "runtime-worktree-create",
            || {
                tracing::info!(
                    target: "gold_band::perf",
                    repository_root = repository_root.as_str(),
                    workspace_root = path.as_str(),
                    branch,
                    lock_wait_ms = lock_wait_started_at.elapsed().as_millis(),
                    "Git worktree create lock acquired"
                );
                self.create_worktree_unlocked(repository_root, path, branch, fork_commit)
            },
        )
    }

    fn create_worktree_unlocked(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        branch: &str,
        fork_commit: &str,
    ) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }
        let command_started_at = Instant::now();
        let output_result = self.runner.run(
            repository_root,
            &["worktree", "add", "-b", branch, path.as_str(), fork_commit],
        );
        tracing::info!(
            target: "gold_band::perf",
            repository_root = repository_root.as_str(),
            workspace_root = path.as_str(),
            branch,
            elapsed_ms = command_started_at.elapsed().as_millis(),
            status = match &output_result {
                Ok(output) if output.success => "ok",
                _ => "error",
            },
            "git worktree add completed"
        );
        let output = output_result.map_err(|error| {
            runtime_error(manual_runtime_error_info(
                RuntimeErrorDomain::Workspace,
                "workspace.worktree-create-failed",
                format!("git worktree add failed: {error:#}"),
                serde_json::json!({ "branch": branch }),
            ))
        })?;
        if !output.success {
            return Err(runtime_error(manual_runtime_error_info(
                RuntimeErrorDomain::Workspace,
                "workspace.worktree-create-failed",
                format!("git worktree add failed: {}", details(&output)),
                serde_json::json!({ "branch": branch }),
            )));
        }
        Ok(())
    }

    pub fn remove_worktree(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        branch: &str,
    ) -> Result<()> {
        let repository = GitSourceControlService::default().repository_identity(repository_root)?;
        GitCoordinationService.with_runtime_write(
            &repository.common_dir,
            Some(path),
            "runtime-worktree-remove",
            || self.remove_worktree_unlocked(repository_root, path, branch, &repository),
        )
    }

    fn remove_worktree_unlocked(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        branch: &str,
        repository: &GitRepositoryIdentity,
    ) -> Result<()> {
        let source_control = GitSourceControlService::default();
        if registered_worktree(&source_control, repository_root, path)?.is_none()
            && let Ok(workspace) = source_control.repository_identity(path)
            && same_filesystem_path(&workspace.workspace_path, path)?
            && same_filesystem_path(&repository.common_dir, &workspace.common_dir)?
        {
            self.ensure_worktree_registration_unlocked(
                repository_root,
                path,
                repository,
                Some(&workspace),
            )?;
        }

        if registered_worktree(&source_control, repository_root, path)?.is_some() {
            let remove = self.runner.run(
                repository_root,
                &["worktree", "remove", "--force", path.as_str()],
            )?;
            if !remove.success
                && registered_worktree(&source_control, repository_root, path)?.is_some()
            {
                return Err(runtime_error(manual_runtime_error_info(
                    RuntimeErrorDomain::Workspace,
                    "workspace.worktree-remove-failed",
                    format!("git worktree remove failed: {}", details(&remove)),
                    serde_json::json!({ "branch": branch }),
                )));
            }
            if !remove.success {
                tracing::warn!(
                    repository_root = repository_root.as_str(),
                    workspace_path = path.as_str(),
                    branch,
                    error = %details(&remove),
                    "git worktree removal reported an error after unregistering the workspace"
                );
            }
        }

        let branch_ref = format!("refs/heads/{branch}");
        let branch_exists = self.runner.run(
            repository_root,
            &["show-ref", "--verify", "--quiet", &branch_ref],
        )?;
        if branch_exists.success {
            let branch_remove = self
                .runner
                .run(repository_root, &["branch", "-D", branch])?;
            if !branch_remove.success {
                tracing::warn!(
                    repository_root = repository_root.as_str(),
                    workspace_path = path.as_str(),
                    branch,
                    error = %details(&branch_remove),
                    "Git branch cleanup failed after the workspace lifecycle was released"
                );
            }
        }
        Ok(())
    }

    pub fn validate_worktree(&self, path: &Utf8Path, branch: &str) -> Result<()> {
        let root = self.runner.run(path, &["rev-parse", "--show-toplevel"])?;
        ensure!(
            root.success,
            "workspace path is not a Git worktree: {}",
            details(&root)
        );
        ensure!(
            same_filesystem_path(Utf8Path::new(&root.stdout), path)?,
            "workspace path is not the expected Git worktree"
        );
        let actual = self.runner.run(path, &["branch", "--show-current"])?;
        ensure!(
            actual.success && actual.stdout == branch,
            "workspace branch changed outside runtime"
        );
        Ok(())
    }

    fn ensure_no_in_progress_operation(&self, workspace: &Utf8Path) -> Result<()> {
        for marker in [
            "MERGE_HEAD",
            "REBASE_HEAD",
            "rebase-merge",
            "rebase-apply",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
        ] {
            let marker_path = self
                .runner
                .capture(
                    workspace,
                    &["rev-parse", "--path-format=absolute", "--git-path", marker],
                )
                .map(Utf8PathBuf::from);
            ensure!(
                !marker_path.as_ref().is_some_and(|path| path.exists()),
                "workspace has an in-progress Git operation: {marker}"
            );
        }
        Ok(())
    }
}

fn canonical_git_path(path: &str) -> Option<Utf8PathBuf> {
    let canonical = std::fs::canonicalize(path).ok()?;
    Utf8PathBuf::from_path_buf(canonical).ok()
}

fn registered_worktree(
    source_control: &GitSourceControlService,
    repository_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<Option<GitWorktree>> {
    Ok(source_control
        .worktrees(repository_root)?
        .into_iter()
        .find(|worktree| same_filesystem_path(&worktree.path, path).unwrap_or(false)))
}

fn same_filesystem_path(left: &Utf8Path, right: &Utf8Path) -> Result<bool> {
    let normalize = |path: &Utf8Path| -> Result<String> {
        let canonical = std::fs::canonicalize(path.as_std_path())
            .with_context(|| format!("failed to canonicalize Git workspace path `{path}`"))?;
        let value = canonical.to_string_lossy().replace('\\', "/");
        #[cfg(windows)]
        let value = value.to_lowercase();
        Ok(value)
    };
    Ok(normalize(left)? == normalize(right)?)
}

pub fn details(output: &GitCommandOutput) -> String {
    match (output.stdout.is_empty(), output.stderr.is_empty()) {
        (true, true) => "no git output".to_string(),
        (false, true) => format!("stdout: {}", output.stdout),
        (true, false) => format!("stderr: {}", output.stderr),
        (false, false) => format!("stdout: {}; stderr: {}", output.stdout, output.stderr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_git_version_accepts_stable_and_vendor_builds() {
        for output in [
            "git version 2.36.0\n",
            "git version 2.36.0.windows.1\n",
            "git version 2.39.3 (Apple Git-145)\n",
        ] {
            let version = parse_git_version(output).expect("version should parse");
            assert!(version.semantic >= minimum_supported_git_version());
        }
    }

    #[test]
    fn minimum_git_version_rejects_older_and_release_candidate_versions() {
        for output in [
            "git version 2.35.9\n",
            "git version 2.36.0-rc1\n",
            "git version 2.36.0.rc1\n",
        ] {
            let version = parse_git_version(output).expect("version should parse");
            assert!(version.semantic < minimum_supported_git_version());
        }
    }

    #[test]
    fn malformed_git_version_output_is_unavailable() {
        for output in ["", "git version unknown", "git version 2.36"] {
            assert_eq!(parse_git_version(output), None);
        }
    }

    #[test]
    fn git_preflight_version_error_exposes_machine_readable_versions() {
        let capability = GitCapability::new(
            GitCapabilityStatus::VersionUnsupported,
            Some("2.35.9.windows.1".to_string()),
        );
        let error = GitPreflightError {
            code: "run.git-version-unsupported",
            capability,
        };

        assert_eq!(error.code, "run.git-version-unsupported");
        assert_eq!(error.params()["installedVersion"], "2.35.9.windows.1");
        assert_eq!(error.params()["minimumVersion"], "2.36.0");
    }

    fn initialized_repository() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().join("repository")).unwrap();
        std::fs::create_dir_all(root.as_std_path()).unwrap();
        let runner = GitCommandRunner::default();
        assert!(runner.run(&root, &["init"]).unwrap().success);
        std::fs::write(root.join("README.md"), "initial\n").unwrap();
        assert!(runner.run(&root, &["add", "README.md"]).unwrap().success);
        assert!(
            runner
                .run(
                    &root,
                    &[
                        "-c",
                        "user.name=Gold Band Test",
                        "-c",
                        "user.email=test@gold-band.local",
                        "commit",
                        "--no-verify",
                        "-m",
                        "initial",
                    ],
                )
                .unwrap()
                .success
        );
        (dir, root)
    }

    #[test]
    fn non_repository_has_repository_required_capability() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8Path::from_path(dir.path()).unwrap();
        let capability = GitRepositoryService::default().probe(path);
        if capability.status != GitCapabilityStatus::NotInstalled {
            assert_eq!(capability.status, GitCapabilityStatus::RepositoryRequired);
        }
    }

    #[test]
    fn repository_without_head_is_rejected_before_run_creation() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();
        assert!(
            GitCommandRunner::default()
                .run(root, &["init"])
                .unwrap()
                .success
        );
        let capability = GitRepositoryService::default().probe(root);
        assert_eq!(capability.status, GitCapabilityStatus::HeadRequired);
        let error = GitRepositoryService::default()
            .require_worktree(root)
            .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<GitPreflightError>()
                .map(|error| error.code),
            Some("run.git-head-required")
        );
    }

    #[test]
    fn runtime_worktree_checkpoint_captures_tracked_and_untracked_changes() {
        let (_dir, root) = initialized_repository();
        let manager = GitWorkspaceManager::default();
        let head = GitRepositoryService::default().head(&root).unwrap();
        let worktree = root.join("runtime-worktrees").join("child");
        manager
            .create_worktree(&root, &worktree, "gb-test-child", &head)
            .unwrap();
        manager
            .validate_worktree(&worktree, "gb-test-child")
            .unwrap();
        std::fs::write(worktree.join("README.md"), "changed\n").unwrap();
        std::fs::write(worktree.join("new.txt"), "new\n").unwrap();

        let checkpoint = manager
            .checkpoint(&worktree, "workspace-child", Some("group-1"))
            .unwrap()
            .unwrap();
        assert_eq!(
            checkpoint,
            GitRepositoryService::default().head(&worktree).unwrap()
        );
        assert!(
            GitRepositoryService::default()
                .status_porcelain(&worktree)
                .unwrap()
                .is_empty()
        );
        let message = GitCommandRunner::default()
            .run(&worktree, &["log", "-1", "--pretty=%B"])
            .unwrap();
        assert!(message.stdout.contains("Gold-Band-Internal: checkpoint"));
        assert!(message.stdout.contains("Gold-Band-Group: group-1"));
    }

    #[test]
    fn ensure_worktree_creates_from_the_requested_head_and_is_idempotent() {
        let (_dir, root) = initialized_repository();
        let manager = GitWorkspaceManager::default();
        let head = GitRepositoryService::default().head(&root).unwrap();
        let worktree = root.join("runtime-worktrees").join("conversation");

        manager
            .ensure_worktree(&root, &worktree, "gb-test-conversation", &head)
            .unwrap();
        assert_eq!(
            GitRepositoryService::default().head(&worktree).unwrap(),
            head
        );

        manager
            .ensure_worktree(&root, &worktree, "gb-test-conversation", &head)
            .unwrap();
        manager
            .validate_worktree(&worktree, "gb-test-conversation")
            .unwrap();
    }

    #[test]
    fn worktree_create_failure_preserves_structured_workspace_error() {
        let (_dir, root) = initialized_repository();
        let runner = GitCommandRunner::default();
        let head = GitRepositoryService::default().head(&root).unwrap();
        assert!(
            runner
                .run(&root, &["branch", "gb-test-existing", &head])
                .unwrap()
                .success
        );

        let error = GitWorkspaceManager::default()
            .create_worktree(
                &root,
                &root.join("runtime-worktrees/conflict"),
                "gb-test-existing",
                &head,
            )
            .unwrap_err();
        let info = crate::runtime_error::normalize_runtime_error(&error);

        assert_eq!(info.code_str(), "workspace.worktree-create-failed");
        assert_eq!(info.domain, RuntimeErrorDomain::Workspace);
        assert_eq!(info.params["branch"], "gb-test-existing");
    }

    #[test]
    fn registration_repair_handles_parent_move_and_is_idempotent() {
        let (_dir, root) = initialized_repository();
        let manager = GitWorkspaceManager::default();
        let head = GitRepositoryService::default().head(&root).unwrap();
        let runtime_parent = root.parent().unwrap().join("runtime-old");
        let old_worktree = runtime_parent.join("conversation");
        manager
            .create_worktree(&root, &old_worktree, "gb-test-registration", &head)
            .unwrap();
        std::fs::write(old_worktree.join("dirty.txt"), "preserve me\n").unwrap();
        let new_runtime_parent = root.parent().unwrap().join("runtime-new");
        std::fs::rename(
            runtime_parent.as_std_path(),
            new_runtime_parent.as_std_path(),
        )
        .unwrap();
        let new_worktree = new_runtime_parent.join("conversation");

        assert!(
            registered_worktree(&GitSourceControlService::default(), &root, &new_worktree)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            manager
                .ensure_worktree_registration(&root, &new_worktree)
                .unwrap(),
            WorktreeRegistrationStatus::Repaired
        );
        assert_eq!(
            manager
                .ensure_worktree_registration(&root, &new_worktree)
                .unwrap(),
            WorktreeRegistrationStatus::AlreadyRegistered
        );
        assert_eq!(
            std::fs::read_to_string(new_worktree.join("dirty.txt")).unwrap(),
            "preserve me\n"
        );
        manager
            .remove_worktree(&root, &new_worktree, "gb-test-registration")
            .unwrap();
        assert!(!new_worktree.exists());
    }

    #[test]
    fn remove_worktree_converges_after_git_already_unregistered_workspace() {
        let (_dir, root) = initialized_repository();
        let manager = GitWorkspaceManager::default();
        let runner = GitCommandRunner::default();
        let head = GitRepositoryService::default().head(&root).unwrap();
        let worktree = root.join("runtime-worktrees").join("partially-removed");
        let branch = "gb-test-partially-removed";
        manager
            .create_worktree(&root, &worktree, branch, &head)
            .unwrap();

        let partial_remove = runner
            .run(&root, &["worktree", "remove", "--force", worktree.as_str()])
            .unwrap();
        assert!(partial_remove.success, "{}", details(&partial_remove));
        std::fs::create_dir_all(worktree.as_std_path()).unwrap();
        assert!(
            registered_worktree(&GitSourceControlService::default(), &root, &worktree)
                .unwrap()
                .is_none()
        );
        assert!(
            runner
                .run(
                    &root,
                    &[
                        "show-ref",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{branch}")
                    ],
                )
                .unwrap()
                .success
        );

        manager.remove_worktree(&root, &worktree, branch).unwrap();
        manager.remove_worktree(&root, &worktree, branch).unwrap();

        assert!(
            registered_worktree(&GitSourceControlService::default(), &root, &worktree)
                .unwrap()
                .is_none()
        );
        assert!(
            !runner
                .run(
                    &root,
                    &[
                        "show-ref",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{branch}")
                    ],
                )
                .unwrap()
                .success
        );
    }

    #[test]
    fn nested_worktree_can_fork_from_parent_checkpoint() {
        let (_dir, root) = initialized_repository();
        let manager = GitWorkspaceManager::default();
        let head = GitRepositoryService::default().head(&root).unwrap();
        let parent = root.join("runtime-worktrees").join("parent");
        manager
            .create_worktree(&root, &parent, "gb-test-parent", &head)
            .unwrap();
        std::fs::write(parent.join("parent.txt"), "checkpointed\n").unwrap();
        let parent_checkpoint = manager
            .checkpoint(&parent, "workspace-parent", Some("outer"))
            .unwrap()
            .unwrap();
        let child = root.join("runtime-worktrees").join("nested-child");
        manager
            .create_worktree(&root, &child, "gb-test-nested", &parent_checkpoint)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(child.join("parent.txt"))
                .unwrap()
                .trim_end(),
            "checkpointed"
        );
    }
}
