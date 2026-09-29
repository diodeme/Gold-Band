use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

use crate::commands::{CommandErrorVm, CommandResult, spawn_blocking_command};
use crate::conversation_workspace::workspace_entry_for_project;
use crate::state::DesktopState;

use super::models::{FileTargetLocationVm, WorkspaceFileLocatorVm};
use super::runtime::WorkspaceFileRuntime;

#[derive(Debug, Clone)]
pub(crate) struct ResolvedWorkspaceRoot {
    pub project_id: String,
    pub path: PathBuf,
    /// `None` is the registered project root; linked worktrees use their canonical path.
    pub workspace_scope_path: Option<String>,
    pub config: gold_band::config::WorkspaceFilesConfig,
}

pub(crate) fn error(code: &str, params: serde_json::Value) -> CommandErrorVm {
    CommandErrorVm::new(code, params)
}

/// Resolve the file root for a project and an optional work location.
///
/// `workspace_path` is the session's linked Git worktree; `None` is the
/// registered project root. A worktree root is accepted only when it is the
/// top level of a worktree sharing the project's Git common directory. The
/// Git check runs once per root; later calls re-verify that the directory and
/// its `.git` pointer are unchanged, so a reclaimed or replaced worktree is
/// never served from a stale validation.
pub(crate) async fn resolve_workspace_root(
    state: &DesktopState,
    runtime: &WorkspaceFileRuntime,
    project_id: &str,
    workspace_path: Option<&str>,
) -> CommandResult<ResolvedWorkspaceRoot> {
    let project = resolve_project_root(state, project_id)?;
    resolve_work_location_root(project, runtime, workspace_path).await
}

async fn resolve_work_location_root(
    project: ResolvedWorkspaceRoot,
    runtime: &WorkspaceFileRuntime,
    workspace_path: Option<&str>,
) -> CommandResult<ResolvedWorkspaceRoot> {
    let Some(requested) = workspace_path.filter(|path| !path.trim().is_empty()) else {
        return Ok(project);
    };
    let requested = PathBuf::from(requested);
    let Ok(canonical) = std::fs::canonicalize(&requested) else {
        runtime.forget_workspace_root(&project.project_id, &requested)?;
        return Err(workspace_unavailable(&requested));
    };
    if same_path(&canonical, &project.path) {
        return Ok(project);
    }
    let Ok(git_pointer) = git_pointer_fingerprint(&canonical) else {
        runtime.forget_workspace_root(&project.project_id, &canonical)?;
        return Err(workspace_unavailable(&canonical));
    };
    if !runtime.workspace_root_validated(&project.project_id, &canonical, &git_pointer)? {
        let project_root = project.path.clone();
        let candidate = canonical.clone();
        spawn_blocking_command(move || validate_linked_worktree(&project_root, &candidate)).await?;
        runtime.remember_workspace_root(&project.project_id, &canonical, git_pointer)?;
    }
    Ok(ResolvedWorkspaceRoot {
        workspace_scope_path: Some(display_path(&canonical)),
        path: canonical,
        ..project
    })
}

/// Watch identity for a root without Git validation: stopping a watch must
/// still succeed after its worktree has been reclaimed.
pub(crate) fn workspace_watch_root(
    state: &DesktopState,
    project_id: &str,
    workspace_path: Option<&str>,
) -> CommandResult<(String, PathBuf)> {
    let project = resolve_project_root(state, project_id);
    match workspace_path.filter(|path| !path.trim().is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            let project_id =
                project.map_or_else(|_| project_id.to_string(), |root| root.project_id);
            Ok((project_id, path))
        }
        None => project.map(|root| (root.project_id, root.path)),
    }
}

fn resolve_project_root(
    state: &DesktopState,
    project_id: &str,
) -> CommandResult<ResolvedWorkspaceRoot> {
    let not_found = || {
        error(
            "workspace-file.project-not-found",
            serde_json::json!({ "projectId": project_id }),
        )
    };
    let context = state.context().map_err(|_| not_found())?;
    let persisted = context.app().load_state().map_err(|_| not_found())?;
    let (workspace_path, resolved_project_id) =
        workspace_entry_for_project(&persisted, project_id).ok_or_else(not_found)?;
    let canonical = std::fs::canonicalize(&workspace_path).map_err(|_| not_found())?;
    Ok(ResolvedWorkspaceRoot {
        project_id: resolved_project_id,
        path: canonical,
        workspace_scope_path: None,
        config: context.config.workspace_files,
    })
}

fn workspace_unavailable(path: &Path) -> CommandErrorVm {
    error(
        "workspace-file.workspace-unavailable",
        serde_json::json!({ "workspacePath": display_path(path) }),
    )
}

/// Content of a linked worktree's `.git` file (its gitdir pointer); an empty
/// fingerprint stands for a `.git` directory. A missing `.git` means the
/// directory is no longer a Git worktree.
pub(crate) fn git_pointer_fingerprint(root: &Path) -> std::io::Result<Vec<u8>> {
    let git = root.join(".git");
    if std::fs::symlink_metadata(&git)?.is_dir() {
        return Ok(Vec::new());
    }
    std::fs::read(git)
}

fn validate_linked_worktree(project_root: &Path, candidate: &Path) -> CommandResult<()> {
    let outside_project = || {
        error(
            "workspace-file.workspace-outside-project",
            serde_json::json!({ "workspacePath": display_path(candidate) }),
        )
    };
    let (Some(project_root), Some(candidate_utf8)) = (
        camino::Utf8Path::from_path(project_root),
        camino::Utf8Path::from_path(candidate),
    ) else {
        return Err(workspace_unavailable(candidate));
    };
    let identity = gold_band::git::GitSourceControlService::default()
        .resolve_scoped_workspace(project_root, Some(candidate_utf8))
        .map_err(
            |error| match error.downcast_ref::<gold_band::git::GitServiceError>() {
                Some(error) if error.code == "git.workspace-outside-project" => outside_project(),
                _ => workspace_unavailable(candidate),
            },
        )?;
    if !same_path(identity.workspace_path.as_std_path(), candidate) {
        return Err(outside_project());
    }
    Ok(())
}

pub(crate) fn resolve_workspace_relative_path(
    root: &ResolvedWorkspaceRoot,
    relative_path: &str,
) -> CommandResult<PathBuf> {
    let relative = Path::new(relative_path);
    if relative_path.contains('\0')
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": relative_path }),
        ));
    }
    let candidate = if relative_path.trim().is_empty() {
        root.path.clone()
    } else {
        root.path.join(relative)
    };
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|io_error| io_path_error(io_error, &candidate, "read"))?;
    if !path_is_within(&canonical, &root.path) {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": relative_path }),
        ));
    }
    Ok(canonical)
}

/// Resolve an existing workspace entry without following a final symlink, so
/// mutating a link never mutates the target it points to.
pub(crate) fn resolve_workspace_entry_path(
    root: &ResolvedWorkspaceRoot,
    relative_path: &str,
) -> CommandResult<PathBuf> {
    let normalized = relative_path.replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    let Some((parent, name)) = trimmed
        .rsplit_once('/')
        .or_else(|| (!trimmed.is_empty()).then_some(("", trimmed)))
    else {
        return Err(error(
            "workspace-file.path-invalid",
            serde_json::json!({ "path": relative_path }),
        ));
    };
    if matches!(name, "." | "..") {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": relative_path }),
        ));
    }
    let parent = resolve_workspace_directory(root, parent)?;
    let entry = parent.join(name);
    std::fs::symlink_metadata(&entry)
        .map_err(|io_error| io_path_error(io_error, &entry, "read"))?;
    Ok(entry)
}

pub(crate) fn resolve_workspace_directory(
    root: &ResolvedWorkspaceRoot,
    relative_path: &str,
) -> CommandResult<PathBuf> {
    let directory = resolve_workspace_relative_path(root, relative_path)?;
    if !directory.is_dir() {
        return Err(error(
            "workspace-file.not-a-directory",
            serde_json::json!({ "path": display_path(&directory) }),
        ));
    }
    Ok(directory)
}

/// Stable sub-reasons for `workspace-file.name-invalid`; the frontend owns the
/// user-facing wording.
pub(crate) fn validate_entry_name(name: &str) -> CommandResult<()> {
    let invalid = |reason: &str| {
        Err(error(
            "workspace-file.name-invalid",
            serde_json::json!({ "name": name, "reason": reason }),
        ))
    };
    if name.trim().is_empty() {
        return invalid("empty");
    }
    if name != name.trim() {
        return invalid("surrounding-whitespace");
    }
    if matches!(name, "." | "..") {
        return invalid("reserved");
    }
    if name.len() > MAX_ENTRY_NAME_BYTES {
        return invalid("too-long");
    }
    if name
        .chars()
        .any(|character| matches!(character, '/' | '\\' | '\0'))
    {
        return invalid("invalid-character");
    }
    if cfg!(windows) {
        if name.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        }) {
            return invalid("invalid-character");
        }
        if name.ends_with('.') {
            return invalid("trailing-dot");
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_ascii_uppercase();
        if WINDOWS_RESERVED_NAMES.contains(&stem.as_str()) {
            return invalid("reserved");
        }
    }
    Ok(())
}

const MAX_ENTRY_NAME_BYTES: usize = 255;
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether two paths name the same filesystem entry. Case-only renames on
/// case-insensitive filesystems report the target as already existing.
#[cfg(unix)]
pub(crate) fn same_entry(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (
        std::fs::symlink_metadata(left),
        std::fs::symlink_metadata(right),
    ) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
pub(crate) fn same_entry(left: &Path, right: &Path) -> bool {
    comparable_path(left) == comparable_path(right)
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    comparable_path(left) == comparable_path(right)
}

pub(crate) fn canonicalize_file(path: &Path, operation: &str) -> CommandResult<PathBuf> {
    let canonical =
        std::fs::canonicalize(path).map_err(|io_error| io_path_error(io_error, path, operation))?;
    if !canonical.is_file() {
        return Err(error(
            "workspace-file.not-a-file",
            serde_json::json!({ "path": display_path(&canonical) }),
        ));
    }
    Ok(canonical)
}

pub(crate) fn locator_for_path(
    root: &ResolvedWorkspaceRoot,
    canonical_path: &Path,
) -> WorkspaceFileLocatorVm {
    let in_workspace = path_is_within(canonical_path, &root.path);
    WorkspaceFileLocatorVm {
        project_id: root.project_id.clone(),
        canonical_path: display_path(canonical_path),
        relative_path: in_workspace.then(|| relative_display(canonical_path, &root.path)),
        scope: if in_workspace {
            "workspace"
        } else {
            "external"
        }
        .to_string(),
    }
}

pub(crate) fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = comparable_path(path);
    let root = comparable_path(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn relative_display(path: &Path, root: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return slash_path(relative);
    }
    let root_components = root.components().count();
    let relative = path.components().skip(root_components).collect::<PathBuf>();
    slash_path(&relative)
}

pub(crate) fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(network_path) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{network_path}");
    }
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
}

pub(crate) fn slash_path(path: &Path) -> String {
    display_path(path).replace('\\', "/")
}

pub(crate) fn parse_file_link_from(
    root: &ResolvedWorkspaceRoot,
    raw_href: &str,
    base_canonical_path: Option<&Path>,
) -> CommandResult<(PathBuf, Option<FileTargetLocationVm>)> {
    let trimmed = raw_href
        .trim()
        .trim_matches(|character| character == '<' || character == '>');
    if trimmed.is_empty() {
        return Err(error(
            "workspace-file.path-invalid",
            serde_json::json!({ "path": raw_href }),
        ));
    }

    let (without_fragment, fragment_target) = split_line_fragment(trimmed);
    let (path_part, suffix_target) = split_line_suffix(without_fragment);
    let path_part = normalize_platform_file_link_path(path_part);
    let target = fragment_target.or(suffix_target);

    let decoded = if looks_like_windows_absolute(path_part) {
        percent_decode(path_part)?
    } else if let Ok(url) = Url::parse(path_part) {
        if url.scheme() != "file" {
            return Err(error(
                "workspace-file.path-invalid",
                serde_json::json!({ "path": raw_href }),
            ));
        }
        url.to_file_path()
            .map_err(|_| {
                error(
                    "workspace-file.path-invalid",
                    serde_json::json!({ "path": raw_href }),
                )
            })?
            .to_string_lossy()
            .into_owned()
    } else {
        percent_decode(path_part)?
    };

    let path = PathBuf::from(decoded);
    if !path.is_absolute()
        && base_canonical_path.is_none()
        && path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": raw_href }),
        ));
    }
    let candidate = if path.is_absolute() {
        path
    } else if let Some(base_path) = base_canonical_path {
        let base_file = canonicalize_file(base_path, "read")?;
        base_file
            .parent()
            .ok_or_else(|| {
                error(
                    "workspace-file.path-invalid",
                    serde_json::json!({ "path": raw_href }),
                )
            })?
            .join(path)
    } else {
        root.path.join(path)
    };
    let canonical = canonicalize_file(&candidate, "read")?;
    Ok((canonical, target))
}

fn split_line_fragment(input: &str) -> (&str, Option<FileTargetLocationVm>) {
    let Some((path, fragment)) = input.rsplit_once('#') else {
        return (input, None);
    };
    let Some(rest) = fragment
        .strip_prefix('L')
        .or_else(|| fragment.strip_prefix('l'))
    else {
        return (input, None);
    };
    let (line, end_line) = rest
        .split_once('-')
        .map(|(line, end)| (line, end.trim_start_matches(['L', 'l'])))
        .unwrap_or((rest, ""));
    let Ok(line) = line.parse::<u32>() else {
        return (input, None);
    };
    if line == 0 {
        return (input, None);
    }
    let end_line = end_line.parse::<u32>().ok().filter(|value| *value >= line);
    (
        path,
        Some(FileTargetLocationVm {
            line: Some(line),
            column: None,
            end_line,
        }),
    )
}

fn split_line_suffix(input: &str) -> (&str, Option<FileTargetLocationVm>) {
    let Some((without_last, last)) = trailing_number(input) else {
        return (input, None);
    };
    if let Some((without_line, line)) = trailing_number(without_last) {
        return (
            without_line,
            Some(FileTargetLocationVm {
                line: Some(line),
                column: Some(last),
                end_line: None,
            }),
        );
    }
    (
        without_last,
        Some(FileTargetLocationVm {
            line: Some(last),
            column: None,
            end_line: None,
        }),
    )
}

fn trailing_number(input: &str) -> Option<(&str, u32)> {
    let (prefix, suffix) = input.rsplit_once(':')?;
    let value = suffix.parse::<u32>().ok()?;
    (value > 0).then_some((prefix, value))
}

fn looks_like_windows_absolute(value: &str) -> bool {
    looks_like_windows_drive_absolute(value) || value.starts_with("\\\\")
}

fn looks_like_windows_drive_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn normalize_platform_file_link_path(value: &str) -> &str {
    #[cfg(windows)]
    if let Some(without_url_root) = value.strip_prefix('/') {
        if looks_like_windows_drive_absolute(without_url_root) {
            return without_url_root;
        }
    }
    value
}

fn percent_decode(value: &str) -> CommandResult<String> {
    percent_decode_str(value)
        .decode_utf8()
        .map(Cow::into_owned)
        .map_err(|_| {
            error(
                "workspace-file.path-invalid",
                serde_json::json!({ "path": value }),
            )
        })
}

pub(crate) fn comparable_path(path: &Path) -> String {
    let normalized = slash_path(path).trim_end_matches('/').to_string();
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

pub(crate) fn io_path_error(
    io_error: std::io::Error,
    path: &Path,
    operation: &str,
) -> CommandErrorVm {
    let code = match io_error.kind() {
        std::io::ErrorKind::NotFound => "workspace-file.not-found",
        std::io::ErrorKind::PermissionDenied => "workspace-file.permission-denied",
        std::io::ErrorKind::AlreadyExists => "workspace-file.already-exists",
        _ if operation == "write" => "workspace-file.write-failed",
        _ => "workspace-file.read-failed",
    };
    error(
        code,
        serde_json::json!({
            "path": display_path(path),
            "operation": operation,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn root(path: &Path) -> ResolvedWorkspaceRoot {
        ResolvedWorkspaceRoot {
            project_id: "project-1".to_string(),
            path: std::fs::canonicalize(path).unwrap(),
            workspace_scope_path: None,
            config: gold_band::config::WorkspaceFilesConfig::default(),
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let cwd = camino::Utf8Path::from_path(cwd).unwrap();
        let output = gold_band::git::GitCommandRunner::default()
            .run(cwd, args)
            .unwrap();
        assert!(output.success, "git {args:?} failed: {}", output.stderr);
    }

    /// A committed repository plus one linked worktree beside it.
    fn repository_with_worktree(dir: &Path) -> (PathBuf, PathBuf) {
        let repo = dir.join("repo");
        let worktree = dir.join("worktree");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init"]);
        std::fs::write(repo.join("README.md"), "main\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(
            &repo,
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
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "session",
                worktree.to_str().unwrap(),
            ],
        );
        (
            std::fs::canonicalize(repo).unwrap(),
            std::fs::canonicalize(worktree).unwrap(),
        )
    }

    fn resolve(
        project: &ResolvedWorkspaceRoot,
        runtime: &WorkspaceFileRuntime,
        workspace_path: Option<&Path>,
    ) -> CommandResult<ResolvedWorkspaceRoot> {
        let workspace_path = workspace_path.map(display_path);
        tauri::async_runtime::block_on(resolve_work_location_root(
            project.clone(),
            runtime,
            workspace_path.as_deref(),
        ))
    }

    #[test]
    fn work_location_root_resolves_main_and_linked_worktree_separately() {
        let dir = tempdir().unwrap();
        let (repo, worktree) = repository_with_worktree(dir.path());
        let project = root(&repo);
        let runtime = WorkspaceFileRuntime::default();

        let resolved_main = resolve(&project, &runtime, None).unwrap();
        assert!(same_path(&resolved_main.path, &repo));
        assert_eq!(resolved_main.workspace_scope_path, None);
        let resolved_explicit_main = resolve(&project, &runtime, Some(&repo)).unwrap();
        assert!(same_path(&resolved_explicit_main.path, &repo));
        assert_eq!(resolved_explicit_main.workspace_scope_path, None);
        let resolved = resolve(&project, &runtime, Some(&worktree)).unwrap();
        assert!(same_path(&resolved.path, &worktree));
        assert_eq!(resolved.workspace_scope_path, Some(display_path(&worktree)));
        assert_eq!(resolved.project_id, "project-1");
        assert!(
            runtime
                .workspace_root_validated(
                    "project-1",
                    &worktree,
                    &git_pointer_fingerprint(&worktree).unwrap()
                )
                .unwrap()
        );
    }

    #[test]
    fn work_location_root_rejects_subdirectories_and_other_repositories() {
        let dir = tempdir().unwrap();
        let (repo, worktree) = repository_with_worktree(dir.path());
        let other_dir = tempdir().unwrap();
        let (other_repo, _) = repository_with_worktree(other_dir.path());
        std::fs::create_dir(worktree.join("nested")).unwrap();
        let project = root(&repo);
        let runtime = WorkspaceFileRuntime::default();

        // A directory without its own `.git` is not a worktree root.
        assert_eq!(
            resolve(&project, &runtime, Some(&worktree.join("nested")))
                .unwrap_err()
                .code,
            "workspace-file.workspace-unavailable"
        );
        assert_eq!(
            resolve(&project, &runtime, Some(&other_repo))
                .unwrap_err()
                .code,
            "workspace-file.workspace-outside-project"
        );
        assert!(
            !runtime
                .workspace_root_validated(
                    "project-1",
                    &other_repo,
                    &git_pointer_fingerprint(&other_repo).unwrap()
                )
                .unwrap()
        );
    }

    #[test]
    fn reclaimed_worktree_is_unavailable_and_forgotten() {
        let dir = tempdir().unwrap();
        let (repo, worktree) = repository_with_worktree(dir.path());
        let project = root(&repo);
        let runtime = WorkspaceFileRuntime::default();
        resolve(&project, &runtime, Some(&worktree)).unwrap();
        let pointer = git_pointer_fingerprint(&worktree).unwrap();

        git(
            &repo,
            &["worktree", "remove", "--force", worktree.to_str().unwrap()],
        );

        let error = resolve(&project, &runtime, Some(&worktree)).unwrap_err();
        assert_eq!(error.code, "workspace-file.workspace-unavailable");
        assert_eq!(error.params["workspacePath"], display_path(&worktree));
        assert!(
            !runtime
                .workspace_root_validated("project-1", &worktree, &pointer)
                .unwrap()
        );
    }

    #[test]
    fn replaced_directory_at_a_validated_path_is_revalidated() {
        let dir = tempdir().unwrap();
        let (repo, worktree) = repository_with_worktree(dir.path());
        let project = root(&repo);
        let runtime = WorkspaceFileRuntime::default();
        resolve(&project, &runtime, Some(&worktree)).unwrap();

        // Same path, but no longer a Git worktree: the cached validation must
        // not be reused.
        std::fs::remove_file(worktree.join(".git")).unwrap();
        assert_eq!(
            resolve(&project, &runtime, Some(&worktree))
                .unwrap_err()
                .code,
            "workspace-file.workspace-unavailable"
        );
    }

    #[test]
    fn validated_root_cache_is_bounded_and_keyed_by_project() {
        let runtime = WorkspaceFileRuntime::default();
        for index in 0..40 {
            runtime
                .remember_workspace_root("project-1", Path::new(&format!("D:/wt/{index}")), vec![1])
                .unwrap();
        }
        assert!(
            !runtime
                .workspace_root_validated("project-1", Path::new("D:/wt/0"), &[1])
                .unwrap()
        );
        assert!(
            runtime
                .workspace_root_validated("project-1", Path::new("D:/wt/39"), &[1])
                .unwrap()
        );
        assert!(
            !runtime
                .workspace_root_validated("project-2", Path::new("D:/wt/39"), &[1])
                .unwrap()
        );
        assert!(
            !runtime
                .workspace_root_validated("project-1", Path::new("D:/wt/39"), &[2])
                .unwrap()
        );
    }

    #[test]
    fn parses_line_and_column_suffix_without_consuming_windows_drive() {
        let (path, target) = split_line_suffix("D:/repo/src/client.rs:2727:8");
        assert_eq!(path, "D:/repo/src/client.rs");
        let target = target.unwrap();
        assert_eq!(target.line, Some(2727));
        assert_eq!(target.column, Some(8));
    }

    #[test]
    fn parses_line_fragment_range() {
        let (path, target) = split_line_fragment("file:///D:/repo/client.rs#L10-L20");
        assert_eq!(path, "file:///D:/repo/client.rs");
        let target = target.unwrap();
        assert_eq!(target.line, Some(10));
        assert_eq!(target.end_line, Some(20));
    }

    #[test]
    fn relative_path_uses_forward_slashes() {
        let root = Path::new("root");
        let path = root.join("src").join("main.rs");
        assert_eq!(relative_display(&path, root), "src/main.rs");
    }

    #[test]
    fn display_path_removes_windows_extended_length_prefixes() {
        assert_eq!(
            display_path(Path::new(r"\\?\D:\repo\README.md")),
            r"D:\repo\README.md"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\README.md")),
            r"\\server\share\README.md"
        );
    }

    #[test]
    fn parses_url_encoded_relative_file_and_target() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("space dir")).unwrap();
        let path = dir.path().join("space dir").join("你好.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        let workspace = root(dir.path());

        let (resolved, target) =
            parse_file_link_from(&workspace, "space%20dir/%E4%BD%A0%E5%A5%BD.rs:12:4", None)
                .unwrap();
        assert_eq!(resolved, std::fs::canonicalize(path).unwrap());
        let target = target.unwrap();
        assert_eq!(target.line, Some(12));
        assert_eq!(target.column, Some(4));
    }

    #[test]
    fn resolves_markdown_links_relative_to_the_current_document() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        let markdown = dir.path().join("docs").join("guide.md");
        let license = dir.path().join("LICENSE");
        std::fs::write(&markdown, "[License](../LICENSE)").unwrap();
        std::fs::write(&license, "AGPL-3.0").unwrap();
        let workspace = root(dir.path());

        let (resolved, target) =
            parse_file_link_from(&workspace, "../LICENSE", Some(&markdown)).unwrap();

        assert_eq!(resolved, std::fs::canonicalize(license).unwrap());
        assert!(target.is_none());
    }

    #[test]
    fn rejects_relative_parent_traversal() {
        let workspace_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let outside = outside_dir.path().join("outside-workspace-file.txt");
        std::fs::write(&outside, "outside").unwrap();
        let workspace = root(workspace_dir.path());

        let outside_name = outside_dir.path().file_name().unwrap().to_string_lossy();
        let href = format!("../{outside_name}/outside-workspace-file.txt");
        let result = parse_file_link_from(&workspace, &href, None);
        assert_eq!(
            result.unwrap_err().code,
            "workspace-file.path-outside-workspace"
        );
    }

    #[test]
    fn locator_classifies_an_absolute_file_outside_the_workspace() {
        let workspace_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let path = outside_dir.path().join("outside.txt");
        std::fs::write(&path, "outside").unwrap();
        let workspace = root(workspace_dir.path());
        let canonical = std::fs::canonicalize(path).unwrap();

        let locator = locator_for_path(&workspace, &canonical);
        assert_eq!(locator.scope, "external");
        assert_eq!(locator.relative_path, None);
    }

    #[test]
    fn parses_file_url_with_line_fragment() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("client.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        let workspace = root(dir.path());
        let href = format!("{}#L10-L20", Url::from_file_path(&path).unwrap());

        let (resolved, target) = parse_file_link_from(&workspace, &href, None).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(path).unwrap());
        let target = target.unwrap();
        assert_eq!(target.line, Some(10));
        assert_eq!(target.end_line, Some(20));
    }

    #[test]
    fn entry_resolution_rejects_the_root_and_parent_traversal() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "").unwrap();
        let workspace = root(dir.path());

        assert_eq!(
            resolve_workspace_entry_path(&workspace, "src/main.rs").unwrap(),
            workspace.path.join("src").join("main.rs")
        );
        assert_eq!(
            resolve_workspace_entry_path(&workspace, "")
                .unwrap_err()
                .code,
            "workspace-file.path-invalid"
        );
        for path in ["..", "src/..", "../outside"] {
            assert_eq!(
                resolve_workspace_entry_path(&workspace, path)
                    .unwrap_err()
                    .code,
                "workspace-file.path-outside-workspace",
                "{path}"
            );
        }
        assert_eq!(
            resolve_workspace_entry_path(&workspace, "src/missing.rs")
                .unwrap_err()
                .code,
            "workspace-file.not-found"
        );
    }

    #[cfg(unix)]
    #[test]
    fn entry_resolution_keeps_a_final_symlink_instead_of_its_target() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("target.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path().join("target.txt"), dir.path().join("link"))
            .unwrap();
        let workspace = root(dir.path());

        assert_eq!(
            resolve_workspace_entry_path(&workspace, "link").unwrap(),
            workspace.path.join("link")
        );
    }

    #[test]
    fn preserves_regular_slash_prefixed_paths() {
        assert_eq!(
            normalize_platform_file_link_path("/Users/dev/readme.md"),
            "/Users/dev/readme.md"
        );
        assert_eq!(
            normalize_platform_file_link_path("/home/dev/readme.md"),
            "/home/dev/readme.md"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn preserves_slash_prefixed_drive_names_on_unix() {
        assert_eq!(
            normalize_platform_file_link_path("/E:/repo/readme.md"),
            "/E:/repo/readme.md"
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_slash_prefixed_windows_drive_pathnames() {
        assert_eq!(
            normalize_platform_file_link_path("/E:/repo/readme.md"),
            "E:/repo/readme.md"
        );
        assert_eq!(
            normalize_platform_file_link_path(r"/E:\repo\readme.md"),
            r"E:\repo\readme.md"
        );
    }

    #[cfg(windows)]
    #[test]
    fn parses_windows_drive_pathname_with_line_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("roadmap.md");
        std::fs::write(&path, "# Roadmap").unwrap();
        let workspace = root(dir.path());
        let canonical = std::fs::canonicalize(path).unwrap();
        let href = format!("/{}:12", slash_path(&canonical));

        let (resolved, target) = parse_file_link_from(&workspace, &href, None).unwrap();

        assert_eq!(resolved, canonical);
        assert_eq!(target.unwrap().line, Some(12));
    }
}
