//! Create, rename, delete (to system trash) and restore workspace entries.
//! Callers resolve and authorize paths; these functions only mutate.

use std::fs::OpenOptions;
use std::path::Path;

use crate::commands::CommandResult;

use super::models::{WorkspaceDirectoryEntryVm, WorkspaceEntryKindInput};
use super::paths::{
    ResolvedWorkspaceRoot, display_path, error, io_path_error, path_is_within, same_entry,
    validate_entry_name,
};
use super::service::entry_vm_for_path;
use super::trash_bin::{TrashError, TrashedEntry, move_to_trash, restore_from_trash};

pub(crate) fn create_entry(
    root: &ResolvedWorkspaceRoot,
    parent: &Path,
    name: &str,
    kind: WorkspaceEntryKindInput,
) -> CommandResult<WorkspaceDirectoryEntryVm> {
    validate_entry_name(name)?;
    let target = parent.join(name);
    match kind {
        WorkspaceEntryKindInput::File => OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map(drop),
        WorkspaceEntryKindInput::Directory => std::fs::create_dir(&target),
    }
    .map_err(|io_error| io_path_error(io_error, &target, "write"))?;
    entry_vm_for_path(root, &target)
}

pub(crate) fn rename_entry(
    root: &ResolvedWorkspaceRoot,
    source: &Path,
    new_name: &str,
) -> CommandResult<WorkspaceDirectoryEntryVm> {
    validate_entry_name(new_name)?;
    let target = source
        .parent()
        .ok_or_else(|| {
            error(
                "workspace-file.path-invalid",
                serde_json::json!({ "path": display_path(source) }),
            )
        })?
        .join(new_name);
    if target.as_os_str() == source.as_os_str() {
        return entry_vm_for_path(root, source);
    }
    // `rename` silently replaces an existing target on Unix, so an occupied
    // name is rejected unless it is this entry under another letter case.
    if std::fs::symlink_metadata(&target).is_ok() && !same_entry(source, &target) {
        return Err(error(
            "workspace-file.already-exists",
            serde_json::json!({ "path": display_path(&target) }),
        ));
    }
    std::fs::rename(source, &target)
        .map_err(|io_error| io_path_error(io_error, source, "write"))?;
    entry_vm_for_path(root, &target)
}

pub(crate) fn delete_entry(
    root: &ResolvedWorkspaceRoot,
    path: &Path,
) -> CommandResult<(WorkspaceDirectoryEntryVm, TrashedEntry)> {
    let entry = entry_vm_for_path(root, path)?;
    let trashed =
        move_to_trash(path).map_err(|trash_error| trash_command_error(path, trash_error))?;
    Ok((entry, trashed))
}

pub(crate) fn restore_entry(
    root: &ResolvedWorkspaceRoot,
    trashed: &TrashedEntry,
) -> CommandResult<WorkspaceDirectoryEntryVm> {
    let original = &trashed.original_path;
    let parent_within_root = original
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .is_some_and(|parent| path_is_within(&parent, &root.path));
    if !parent_within_root {
        return Err(error(
            "workspace-file.restore-unavailable",
            serde_json::json!({ "path": display_path(original), "reason": "parent-missing" }),
        ));
    }
    restore_from_trash(trashed)
        .map_err(|trash_error| trash_command_error(original, trash_error))?;
    entry_vm_for_path(root, original)
}

fn trash_command_error(path: &Path, trash_error: TrashError) -> crate::commands::CommandErrorVm {
    let path = display_path(path);
    match trash_error {
        TrashError::Collision => error(
            "workspace-file.already-exists",
            serde_json::json!({ "path": path }),
        ),
        TrashError::Unavailable(reason) => error(
            "workspace-file.restore-unavailable",
            serde_json::json!({ "path": path, "reason": reason }),
        ),
        TrashError::Failed(reason) => error(
            "workspace-file.trash-failed",
            serde_json::json!({ "path": path, "reason": reason }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_files::paths::resolve_workspace_entry_path;
    use tempfile::tempdir;

    fn root(path: &Path) -> ResolvedWorkspaceRoot {
        ResolvedWorkspaceRoot {
            project_id: "project-1".to_string(),
            path: std::fs::canonicalize(path).unwrap(),
            config: gold_band::config::WorkspaceFilesConfig::default(),
        }
    }

    #[test]
    fn creates_an_empty_file_and_a_directory_without_overwriting() {
        let dir = tempdir().unwrap();
        let workspace = root(dir.path());

        let file = create_entry(
            &workspace,
            &workspace.path,
            "notes.md",
            WorkspaceEntryKindInput::File,
        )
        .unwrap();
        assert_eq!(file.kind, "file");
        assert_eq!(file.relative_path, "notes.md");
        assert_eq!(file.byte_length, Some(0));

        let folder = create_entry(
            &workspace,
            &workspace.path,
            "docs",
            WorkspaceEntryKindInput::Directory,
        )
        .unwrap();
        assert_eq!(folder.kind, "directory");
        assert!(!folder.has_children);

        std::fs::write(workspace.path.join("notes.md"), "keep").unwrap();
        let duplicate = create_entry(
            &workspace,
            &workspace.path,
            "notes.md",
            WorkspaceEntryKindInput::File,
        );
        assert_eq!(duplicate.unwrap_err().code, "workspace-file.already-exists");
        assert_eq!(
            std::fs::read_to_string(workspace.path.join("notes.md")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn rejects_names_that_escape_the_parent_or_are_empty() {
        let dir = tempdir().unwrap();
        let workspace = root(dir.path());
        for name in ["", "  ", "..", "a/b", "a\\b", " padded "] {
            let result = create_entry(
                &workspace,
                &workspace.path,
                name,
                WorkspaceEntryKindInput::File,
            );
            assert_eq!(
                result.unwrap_err().code,
                "workspace-file.name-invalid",
                "{name:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_reserved_names_and_characters() {
        let dir = tempdir().unwrap();
        let workspace = root(dir.path());
        for name in ["CON", "nul.txt", "a:b", "what?", "trailing."] {
            let result = create_entry(
                &workspace,
                &workspace.path,
                name,
                WorkspaceEntryKindInput::File,
            );
            assert_eq!(
                result.unwrap_err().code,
                "workspace-file.name-invalid",
                "{name:?}"
            );
        }
    }

    #[test]
    fn renames_within_the_same_parent_and_refuses_to_replace_another_entry() {
        let dir = tempdir().unwrap();
        let workspace = root(dir.path());
        std::fs::create_dir(workspace.path.join("src")).unwrap();
        std::fs::write(workspace.path.join("src").join("a.rs"), "a").unwrap();
        std::fs::write(workspace.path.join("src").join("b.rs"), "b").unwrap();
        let source = resolve_workspace_entry_path(&workspace, "src/a.rs").unwrap();

        let collision = rename_entry(&workspace, &source, "b.rs");
        assert_eq!(collision.unwrap_err().code, "workspace-file.already-exists");
        assert_eq!(
            std::fs::read_to_string(workspace.path.join("src").join("b.rs")).unwrap(),
            "b"
        );

        let renamed = rename_entry(&workspace, &source, "c.rs").unwrap();
        assert_eq!(renamed.relative_path, "src/c.rs");
        assert!(!workspace.path.join("src").join("a.rs").exists());
    }

    #[test]
    fn allows_a_case_only_rename() {
        let dir = tempdir().unwrap();
        let workspace = root(dir.path());
        std::fs::write(workspace.path.join("readme.md"), "doc").unwrap();
        let source = resolve_workspace_entry_path(&workspace, "readme.md").unwrap();

        let renamed = rename_entry(&workspace, &source, "README.md").unwrap();

        assert_eq!(renamed.name, "README.md");
        let names = std::fs::read_dir(&workspace.path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["README.md".to_string()]);
    }

    #[test]
    fn renames_a_directory_with_its_contents() {
        let dir = tempdir().unwrap();
        let workspace = root(dir.path());
        std::fs::create_dir_all(workspace.path.join("old").join("nested")).unwrap();
        std::fs::write(workspace.path.join("old").join("nested").join("x.txt"), "x").unwrap();
        let source = resolve_workspace_entry_path(&workspace, "old").unwrap();

        let renamed = rename_entry(&workspace, &source, "new").unwrap();

        assert_eq!(renamed.kind, "directory");
        assert_eq!(
            std::fs::read_to_string(workspace.path.join("new").join("nested").join("x.txt"))
                .unwrap(),
            "x"
        );
    }

    #[test]
    fn restore_rejects_an_original_parent_outside_the_workspace() {
        let workspace_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let workspace = root(workspace_dir.path());
        let trashed = TrashedEntry {
            original_path: std::fs::canonicalize(outside_dir.path())
                .unwrap()
                .join("x.txt"),
            deleted_after_unix: 0,
            trashed_path: None,
        };

        let result = restore_entry(&workspace, &trashed);

        assert_eq!(
            result.unwrap_err().code,
            "workspace-file.restore-unavailable"
        );
    }
}
