use std::process::Output;
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::process::background_command;
use crate::runtime_error::{RuntimeErrorDomain, manual_runtime_error_info, runtime_error};

mod github;
mod source_control;

pub use github::*;
pub use source_control::*;

const CHECKPOINT_AUTHOR_NAME: &str = "Gold Band Runtime";
const CHECKPOINT_AUTHOR_EMAIL: &str = "runtime@gold-band.local";

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
    RepositoryRequired,
    HeadRequired,
    WorktreeRequired,
    RepositoryUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCapability {
    pub status: GitCapabilityStatus,
    pub repo_root: Option<Utf8PathBuf>,
    pub common_dir: Option<Utf8PathBuf>,
    pub head: Option<String>,
}

impl GitCapability {
    pub fn ready(&self) -> bool {
        self.status == GitCapabilityStatus::Ready
    }

    pub fn error_code(&self) -> Option<&'static str> {
        match self.status {
            GitCapabilityStatus::Ready => None,
            GitCapabilityStatus::NotInstalled => Some("run.git-not-installed"),
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
        })
    }
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
        let output = self.runner.run(cwd, &["init"])?;
        ensure!(output.success, "git init failed: {}", details(&output));
        Ok(self.probe(cwd))
    }

    pub fn probe(&self, cwd: &Utf8Path) -> GitCapability {
        let version = match background_command("git").arg("--version").output() {
            Ok(output) if output.status.success() => output,
            _ => {
                return GitCapability {
                    status: GitCapabilityStatus::NotInstalled,
                    repo_root: None,
                    common_dir: None,
                    head: None,
                };
            }
        };
        drop(version);

        let Some(inside) = self
            .runner
            .capture(cwd, &["rev-parse", "--is-inside-work-tree"])
        else {
            return GitCapability {
                status: GitCapabilityStatus::RepositoryRequired,
                repo_root: None,
                common_dir: None,
                head: None,
            };
        };
        if inside != "true" {
            return GitCapability {
                status: GitCapabilityStatus::RepositoryRequired,
                repo_root: None,
                common_dir: None,
                head: None,
            };
        }

        let repo_root = self
            .runner
            .capture(cwd, &["rev-parse", "--show-toplevel"])
            .map(Utf8PathBuf::from);
        let common_dir = self
            .runner
            .capture(
                cwd,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .map(Utf8PathBuf::from);
        if repo_root.is_none() || common_dir.is_none() {
            return GitCapability {
                status: GitCapabilityStatus::RepositoryUnavailable,
                repo_root,
                common_dir,
                head: None,
            };
        }
        let head = self.runner.capture(cwd, &["rev-parse", "--verify", "HEAD"]);
        if head.is_none() {
            return GitCapability {
                status: GitCapabilityStatus::HeadRequired,
                repo_root,
                common_dir,
                head,
            };
        }
        if self
            .runner
            .capture(cwd, &["worktree", "list", "--porcelain"])
            .is_none()
        {
            return GitCapability {
                status: GitCapabilityStatus::WorktreeRequired,
                repo_root,
                common_dir,
                head,
            };
        }
        GitCapability {
            status: GitCapabilityStatus::Ready,
            repo_root,
            common_dir,
            head,
        }
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
        let workspace = GitSourceControlService::default().repository_identity(path)?;
        GitCoordinationService.with_runtime_write(
            &repository.common_dir,
            Some(&workspace.workspace_path),
            "runtime-worktree-remove",
            || {
                self.ensure_worktree_registration_unlocked(
                    repository_root,
                    path,
                    &repository,
                    Some(&workspace),
                )?;
                self.remove_worktree_unlocked(repository_root, path, branch)
            },
        )
    }

    fn remove_worktree_unlocked(
        &self,
        repository_root: &Utf8Path,
        path: &Utf8Path,
        branch: &str,
    ) -> Result<()> {
        let remove = self.runner.run(
            repository_root,
            &["worktree", "remove", "--force", path.as_str()],
        )?;
        ensure!(
            remove.success,
            "git worktree remove failed: {}",
            details(&remove)
        );
        let branch_remove = self
            .runner
            .run(repository_root, &["branch", "-D", branch])?;
        ensure!(
            branch_remove.success,
            "git branch -D failed: {}",
            details(&branch_remove)
        );
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
                .capture(workspace, &["rev-parse", "--git-path", marker])
                .map(Utf8PathBuf::from);
            ensure!(
                !marker_path.as_ref().is_some_and(|path| path.exists()),
                "workspace has an in-progress Git operation: {marker}"
            );
        }
        Ok(())
    }
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
