use super::{
    PromptWorkspaceRoot, ResolvedWorkspaceFileRef, WorkspaceFileRefError, WorkspacePathFileRef,
    display_path, lexical_relative_path, normalize_reference, path_is_within,
    workspace_file_authoring_identity,
};
use crate::storage::{GoldBandPaths, read_json};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFileAttemptLocator {
    pub project_id: String,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_uuid: Option<String>,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub branch_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_attempt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum PromptWorkspaceFileRef {
    Workspace {
        project_id: String,
        relative_path: String,
    },
    Artifact {
        locator: PromptFileAttemptLocator,
        name: String,
    },
    TurnFile {
        locator: PromptFileAttemptLocator,
        change_set_id: String,
        change_id: String,
    },
    TurnAttachment {
        locator: PromptFileAttemptLocator,
        change_set_id: String,
        attachment_id: String,
    },
    GitFile {
        project_id: String,
        workspace_path: Option<String>,
        relative_path: String,
    },
}

impl PromptWorkspaceFileRef {
    pub fn project_id(&self) -> &str {
        match self {
            Self::Workspace { project_id, .. } | Self::GitFile { project_id, .. } => project_id,
            Self::Artifact { locator, .. }
            | Self::TurnFile { locator, .. }
            | Self::TurnAttachment { locator, .. } => &locator.project_id,
        }
    }

    pub fn authoring_identity(&self) -> String {
        let mut normalized = self.clone();
        match &mut normalized {
            Self::Workspace {
                project_id,
                relative_path,
            }
            | Self::GitFile {
                project_id,
                relative_path,
                ..
            } => {
                (*project_id, *relative_path) =
                    workspace_file_authoring_identity(project_id, relative_path);
            }
            _ => {}
        }
        if let Self::GitFile {
            workspace_path: Some(path),
            ..
        } = &mut normalized
        {
            *path = workspace_file_authoring_identity("", path).1;
        }
        serde_json::to_string(&normalized).expect("file source serialization is infallible")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FileReferenceAvailability {
    Available,
    Unavailable {
        code: &'static str,
        params: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReferenceTarget {
    pub source: PromptWorkspaceFileRef,
    pub project_id: String,
    pub name: String,
    pub absolute_path: Option<String>,
    pub relative_path: Option<String>,
    pub relative_base: &'static str,
    pub workspace_path: Option<String>,
    pub byte_length: Option<u64>,
    pub mime_type: String,
    pub availability: FileReferenceAvailability,
}

struct TargetPaths {
    project_id: String,
    base: PathBuf,
    relative: String,
    workspace_path: Option<String>,
    is_attempt: bool,
    blocked: Option<WorkspaceFileRefError>,
}

impl TargetPaths {
    fn reference(&self) -> WorkspacePathFileRef {
        WorkspacePathFileRef {
            project_id: self.project_id.clone(),
            relative_path: self.relative.clone(),
        }
    }
    fn resolve(
        &self,
        source: &PromptWorkspaceFileRef,
    ) -> Result<ResolvedWorkspaceFileRef, WorkspaceFileRefError> {
        if let Some(error) = &self.blocked {
            return Err(error.clone());
        }
        let root =
            std::fs::canonicalize(&self.base).map_err(|_| WorkspaceFileRefError::NotFound {
                project_id: self.project_id.clone(),
                relative_path: self.relative.clone(),
            })?;
        let mut resolved = normalize_reference(&self.reference(), &root)?;
        resolved.source = source.clone();
        Ok(resolved)
    }
}

pub fn resolve_file_reference_target(
    roots: &[PromptWorkspaceRoot],
    source: &PromptWorkspaceFileRef,
) -> Result<FileReferenceTarget, WorkspaceFileRefError> {
    let target = target_paths(roots, source)?;
    let path: PathBuf = target.base.join(&target.relative).components().collect();
    let (byte_length, mime_type, availability) = match target.resolve(source) {
        Ok(file) => (
            Some(file.size),
            file.mime_type,
            FileReferenceAvailability::Available,
        ),
        Err(error) => (
            None,
            crate::provider::file_mime_type(&path),
            FileReferenceAvailability::Unavailable {
                code: error.code(),
                params: error.params(),
            },
        ),
    };
    Ok(FileReferenceTarget {
        source: source.clone(),
        project_id: target.project_id,
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        absolute_path: Some(display_path(&path)),
        relative_path: (!Path::new(&target.relative).is_absolute()).then_some(target.relative),
        relative_base: if matches!(source, PromptWorkspaceFileRef::TurnAttachment { .. }) {
            "attachment"
        } else if target.is_attempt {
            "attempt"
        } else {
            "workspace"
        },
        workspace_path: target.workspace_path,
        byte_length,
        mime_type,
        availability,
    })
}

pub(super) fn resolve_file_reference(
    roots: &[PromptWorkspaceRoot],
    source: &PromptWorkspaceFileRef,
) -> Result<ResolvedWorkspaceFileRef, WorkspaceFileRefError> {
    target_paths(roots, source)?.resolve(source)
}

fn target_paths(
    roots: &[PromptWorkspaceRoot],
    source: &PromptWorkspaceFileRef,
) -> Result<TargetPaths, WorkspaceFileRefError> {
    let project_id = source.project_id();
    let root = roots
        .iter()
        .find(|root| root.project_id == project_id)
        .ok_or_else(|| WorkspaceFileRefError::ProjectUnavailable {
            project_id: project_id.into(),
        })?;
    let invalid = || WorkspaceFileRefError::InvalidPath {
        project_id: project_id.into(),
        relative_path: String::new(),
    };
    let mut result = TargetPaths {
        project_id: project_id.into(),
        base: root.root.clone(),
        relative: String::new(),
        workspace_path: None,
        is_attempt: false,
        blocked: None,
    };
    match source {
        PromptWorkspaceFileRef::Workspace { relative_path, .. } => {
            result.relative = relative_path.clone();
        }
        PromptWorkspaceFileRef::GitFile {
            workspace_path,
            relative_path,
            ..
        } => {
            let paths = root.paths.as_ref().ok_or_else(invalid)?;
            result.base = workspace_path.as_ref().map_or_else(
                || paths.repo_root.as_std_path().to_path_buf(),
                PathBuf::from,
            );
            result.workspace_path = workspace_path.clone();
            result.relative = relative_path.clone();
            result.blocked = trusted_workspace(paths, &result.base).err();
        }
        PromptWorkspaceFileRef::Artifact { locator, .. }
        | PromptWorkspaceFileRef::TurnFile { locator, .. }
        | PromptWorkspaceFileRef::TurnAttachment { locator, .. } => {
            validate_locator(locator)?;
            let paths = root.paths.as_ref().ok_or_else(invalid)?;
            if paths.project_id != project_id {
                return Err(invalid());
            }
            // Read the canonical run identity only; never migrate or load full task history.
            #[derive(Deserialize)]
            struct RunLocation {
                id: String,
                task_id: String,
                task_uuid: Option<String>,
                worktree: Option<crate::runtime::RunWorktreeState>,
            }
            let run: RunLocation = read_json(&paths.run_file(&locator.task_id, &locator.run_id))
                .map_err(|_| invalid())?;
            if run.id != locator.run_id
                || run.task_id != locator.task_id
                || locator
                    .task_uuid
                    .as_ref()
                    .is_some_and(|uuid| Some(uuid) != run.task_uuid.as_ref())
            {
                return Err(invalid());
            }
            let attempt = attempt_dir(paths, locator);
            // A symlinked attempt must not escape the runtime root, even if its child exists.
            let canonical_attempt = std::fs::canonicalize(&attempt).map_err(|_| invalid())?;
            let runtime_root = std::fs::canonicalize(&paths.runtime_root).map_err(|_| invalid())?;
            if !path_is_within(&canonical_attempt, &runtime_root) {
                return Err(invalid());
            }
            if let PromptWorkspaceFileRef::Artifact { name, .. } = source {
                validate_segment(name).map_err(|_| invalid())?;
                let name = name.strip_suffix(".json").unwrap_or(name);
                let artifact = match (&locator.outer_node_id, &locator.outer_attempt_id) {
                    (Some(outer_node), Some(outer_attempt)) => paths.dynamic_node_artifact_file(
                        &locator.task_id,
                        &locator.run_id,
                        &locator.round_id,
                        outer_node,
                        outer_attempt,
                        &locator.node_id,
                        &locator.attempt_id,
                        name,
                    ),
                    _ => paths.artifact_file(
                        &locator.task_id,
                        &locator.run_id,
                        &locator.round_id,
                        &locator.node_id,
                        &locator.attempt_id,
                        name,
                    ),
                };
                result.relative = artifact
                    .strip_prefix(&attempt)
                    .map_err(|_| invalid())?
                    .as_str()
                    .replace('\\', "/");
                result.base = attempt.into_std_path_buf();
                result.is_attempt = true;
            } else if let PromptWorkspaceFileRef::TurnAttachment {
                change_set_id,
                attachment_id,
                ..
            } = source
            {
                let store =
                    crate::acp::turn_files::TurnFileStore::new(attempt.clone(), Default::default());
                let set = store
                    .read_change_set_metadata(change_set_id)
                    .map_err(|_| invalid())?;
                if set.branch_id != locator.branch_id || set.id != *change_set_id {
                    return Err(invalid());
                }
                let relative = store
                    .attachment_relative_path_from_change_set(&set, attachment_id)
                    .map_err(|_| invalid())?;
                result.base = attempt.join("attachments").into_std_path_buf();
                result.relative = relative.replace('\\', &std::path::MAIN_SEPARATOR.to_string());
                result.is_attempt = true;
            } else if let PromptWorkspaceFileRef::TurnFile {
                change_set_id,
                change_id,
                ..
            } = source
            {
                let store = crate::acp::turn_files::TurnFileStore::new(attempt, Default::default());
                let set = store
                    .read_change_set_metadata(change_set_id)
                    .map_err(|_| invalid())?;
                if set.branch_id != locator.branch_id || set.id != *change_set_id {
                    return Err(invalid());
                }
                let change = set
                    .changes
                    .iter()
                    .find(|change| change.id == *change_id)
                    .ok_or_else(invalid)?;
                result.base = run.worktree.map_or_else(
                    || paths.repo_root.as_std_path().to_path_buf(),
                    |w| w.path.into_std_path_buf(),
                );
                if let (Some(outer_node), Some(outer_attempt)) =
                    (&locator.outer_node_id, &locator.outer_attempt_id)
                {
                    // Direct deserialize is intentional: menu reads must not invoke graph migration.
                    let graph: crate::dynamic::DynamicGraphState =
                        read_json(&paths.dynamic_graph_file(
                            &locator.task_id,
                            &locator.run_id,
                            &locator.round_id,
                            outer_node,
                            outer_attempt,
                        ))
                        .map_err(|_| invalid())?;
                    let node = graph
                        .nodes
                        .iter()
                        .find(|node| node.id == locator.node_id)
                        .ok_or_else(invalid)?;
                    let workspace = graph
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == node.workspace_id)
                        .ok_or_else(invalid)?;
                    result.base = workspace.path.as_std_path().to_path_buf();
                    if workspace.status == crate::dynamic::WorkspaceStatus::Released {
                        result.blocked = Some(WorkspaceFileRefError::WorkspaceUnavailable {
                            workspace_path: display_path(&result.base),
                        });
                    }
                }
                result.workspace_path = (result.base != paths.repo_root.as_std_path())
                    .then(|| display_path(&result.base));
                let logical = PathBuf::from(&change.logical_path);
                result.relative = if logical.is_absolute() {
                    // A recorded display root never authorizes access to an unrelated directory.
                    relative_inside(&logical, &result.base).unwrap_or_else(|| {
                        result.blocked = Some(WorkspaceFileRefError::OutsideWorkspace {
                            project_id: project_id.into(),
                            relative_path: change.logical_path.clone(),
                        });
                        change.logical_path.clone()
                    })
                } else {
                    change.logical_path.clone()
                };
                if result.blocked.is_none() {
                    result.blocked = trusted_workspace(paths, &result.base).err();
                }
            }
        }
    }
    if !Path::new(&result.relative).is_absolute() {
        result.relative = lexical_relative_path(&result.reference())?;
    } else if result.blocked.is_none() {
        return Err(invalid());
    }
    Ok(result)
}

fn trusted_workspace(paths: &GoldBandPaths, candidate: &Path) -> Result<(), WorkspaceFileRefError> {
    let unavailable = || WorkspaceFileRefError::WorkspaceUnavailable {
        workspace_path: display_path(candidate),
    };
    let canonical = std::fs::canonicalize(candidate).map_err(|_| unavailable())?;
    if std::fs::canonicalize(&paths.repo_root).is_ok_and(|root| root == canonical) {
        return Ok(());
    }
    let candidate = camino::Utf8Path::from_path(&canonical).ok_or_else(unavailable)?;
    let identity = crate::git::GitSourceControlService::default()
        .resolve_scoped_workspace(&paths.repo_root, Some(candidate))
        .map_err(|_| unavailable())?;
    if identity.workspace_path == candidate {
        Ok(())
    } else {
        Err(unavailable())
    }
}

fn relative_inside(path: &Path, root: &Path) -> Option<String> {
    let path = display_path(path).replace('\\', "/");
    let root = display_path(root).replace('\\', "/");
    let root = root.trim_end_matches('/');
    let key = |value: &str| {
        if cfg!(windows) {
            value.to_ascii_lowercase()
        } else {
            value.into()
        }
    };
    key(&path).strip_prefix(&(key(root) + "/"))?;
    Some(path[root.len() + 1..].to_string())
}

fn validate_segment(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\\', ':', '\0', '\n', '\r'])
    {
        Err(())
    } else {
        Ok(())
    }
}

fn validate_locator(locator: &PromptFileAttemptLocator) -> Result<(), WorkspaceFileRefError> {
    let valid = [
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
    ]
    .into_iter()
    .chain(locator.outer_node_id.iter())
    .chain(locator.outer_attempt_id.iter())
    .all(|value| validate_segment(value).is_ok());
    if !valid
        || locator.outer_node_id.is_some() != locator.outer_attempt_id.is_some()
        || crate::acp::branches::validate_conversation_branch_id(&locator.branch_id).is_err()
    {
        return Err(WorkspaceFileRefError::InvalidPath {
            project_id: locator.project_id.clone(),
            relative_path: String::new(),
        });
    }
    Ok(())
}

fn attempt_dir(paths: &GoldBandPaths, l: &PromptFileAttemptLocator) -> camino::Utf8PathBuf {
    match (&l.outer_node_id, &l.outer_attempt_id) {
        (Some(node), Some(attempt)) => paths.dynamic_node_attempt_dir(
            &l.task_id,
            &l.run_id,
            &l.round_id,
            node,
            attempt,
            &l.node_id,
            &l.attempt_id,
        ),
        _ => paths.attempt_dir(
            &l.task_id,
            &l.run_id,
            &l.round_id,
            &l.node_id,
            &l.attempt_id,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::write_json;
    use tempfile::TempDir;

    fn fixture() -> (
        TempDir,
        GoldBandPaths,
        PromptFileAttemptLocator,
        Vec<PromptWorkspaceRoot>,
    ) {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("repo");
        let receiver = dir.path().join("receiver");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&receiver).unwrap();
        let mut paths = GoldBandPaths::new(camino::Utf8PathBuf::from_path_buf(repo).unwrap());
        paths.runtime_root =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("runtime")).unwrap();
        let locator = PromptFileAttemptLocator {
            project_id: paths.project_id.clone(),
            task_id: "task".into(),
            task_uuid: Some("task-uuid".into()),
            run_id: "run".into(),
            round_id: "round".into(),
            node_id: "node".into(),
            attempt_id: "attempt".into(),
            branch_id: "root".into(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        write_json(
            &paths.run_file("task", "run"),
            &serde_json::json!({
                "id":"run", "task_id":"task", "task_uuid":"task-uuid", "worktree":null
            }),
        )
        .unwrap();
        std::fs::create_dir_all(attempt_dir(&paths, &locator)).unwrap();
        let roots = vec![PromptWorkspaceRoot {
            project_id: paths.project_id.clone(),
            root: receiver,
            paths: Some(paths.clone()),
        }];
        (dir, paths, locator, roots)
    }

    fn record_change(
        paths: &GoldBandPaths,
        locator: &PromptFileAttemptLocator,
        path: &str,
        schema: u32,
    ) -> camino::Utf8PathBuf {
        use crate::acp::turn_files::*;
        let set = TurnFileChangeSet {
            schema_version: schema,
            id: "set".into(),
            turn_id: "turn".into(),
            prompt_event_id: "event".into(),
            branch_id: "root".into(),
            status: TurnFileChangeSetStatus::Finalized,
            started_at: "time".into(),
            finished_at: None,
            summary: Default::default(),
            changes: vec![TurnFileChange {
                id: "change".into(),
                change_kind: FileChangeKind::Modified,
                logical_path: path.into(),
                previous_logical_path: None,
                mime_type: None,
                text: true,
                added_lines: None,
                deleted_lines: None,
                before_version: None,
                after_version: None,
                limitation_code: None,
            }],
            attachments: vec![],
            limitation_codes: vec![],
            workspace_root: Some("/untrusted-display-root".into()),
        };
        let file = attempt_dir(paths, locator).join("turn-file-change-sets/set.json");
        write_json(&file, &set).unwrap();
        file
    }

    #[test]
    fn artifact_uses_its_attempt_root_and_does_not_rebase_on_the_receiver() {
        let (_dir, paths, locator, roots) = fixture();
        let file = paths.artifact_file("task", "run", "round", "node", "attempt", "review");
        crate::storage::ensure_parent_dir(&file).unwrap();
        std::fs::write(&file, "{}").unwrap();
        let source = PromptWorkspaceFileRef::Artifact {
            locator,
            name: "review.json".into(),
        };
        let target = resolve_file_reference_target(&roots, &source).unwrap();
        assert_eq!(
            target.relative_path.as_deref(),
            Some("artifacts/review.json")
        );
        assert!(matches!(
            target.availability,
            FileReferenceAvailability::Available
        ));
        let resolved =
            super::super::resolve_prompt_workspace_files(&roots, &[source.clone()], 0).unwrap();
        assert_eq!(resolved[0].source, source);
        assert_eq!(
            resolved[0].canonical_path,
            display_path(&std::fs::canonicalize(file).unwrap())
        );
    }

    #[test]
    fn dynamic_artifact_uses_the_nested_attempt() {
        let (_dir, paths, mut locator, roots) = fixture();
        locator.outer_node_id = Some("outer".into());
        locator.outer_attempt_id = Some("outer-attempt".into());
        let file = paths.dynamic_node_artifact_file(
            "task",
            "run",
            "round",
            "outer",
            "outer-attempt",
            "node",
            "attempt",
            "review",
        );
        crate::storage::ensure_parent_dir(&file).unwrap();
        std::fs::write(&file, "{}").unwrap();
        let source = PromptWorkspaceFileRef::Artifact {
            locator,
            name: "review.json".into(),
        };
        let target = resolve_file_reference_target(&roots, &source).unwrap();
        assert_eq!(target.absolute_path, Some(display_path(file.as_std_path())));
        assert_eq!(
            target.relative_path.as_deref(),
            Some("artifacts/review.json")
        );
    }

    fn record_attachment(paths: &GoldBandPaths, locator: &PromptFileAttemptLocator, schema: u32) {
        use crate::acp::turn_files::*;
        let set = TurnFileChangeSet {
            schema_version: schema,
            id: "set".into(),
            turn_id: "turn".into(),
            prompt_event_id: "event".into(),
            branch_id: "root".into(),
            status: TurnFileChangeSetStatus::Finalized,
            started_at: "time".into(),
            finished_at: None,
            summary: Default::default(),
            changes: vec![],
            attachments: vec![TurnAttachment {
                id: "attachment".into(),
                relative_path: "reports/report.md".into(),
                name: "report.md".into(),
                byte_length: 27,
            }],
            limitation_codes: vec![],
            workspace_root: None,
        };
        let file = attempt_dir(paths, locator).join("turn-file-change-sets/set.json");
        write_json(&file, &set).unwrap();
    }

    #[test]
    fn turn_attachment_uses_finalized_manifest_and_attempt_attachment_root() {
        let (_dir, paths, locator, roots) = fixture();
        let file = attempt_dir(&paths, &locator).join("attachments/reports/report.md");
        crate::storage::ensure_parent_dir(&file).unwrap();
        std::fs::write(&file, "workflow attachment").unwrap();
        record_attachment(&paths, &locator, 6);
        let manifest = attempt_dir(&paths, &locator).join("turn-file-change-sets/set.json");
        let source = PromptWorkspaceFileRef::TurnAttachment {
            locator: locator.clone(),
            change_set_id: "set".into(),
            attachment_id: "attachment".into(),
        };
        let target = resolve_file_reference_target(&roots, &source).unwrap();
        let expected_absolute = std::fs::canonicalize(&file).unwrap();
        assert_eq!(
            target.absolute_path.as_deref(),
            Some(display_path(&expected_absolute).as_str())
        );
        assert_eq!(target.relative_path.as_deref(), Some("reports/report.md"));
        assert_eq!(target.relative_base, "attachment");
        assert!(matches!(
            target.availability,
            FileReferenceAvailability::Available
        ));
        let raw: serde_json::Value = read_json(&manifest).unwrap();
        assert_eq!(
            raw["schemaVersion"],
            serde_json::json!(6),
            "menu reads must not migrate the manifest"
        );
        let resolved =
            super::super::resolve_prompt_workspace_files(&roots, &[source.clone()], 0).unwrap();
        assert_eq!(
            resolved[0].canonical_path,
            display_path(&std::fs::canonicalize(&file).unwrap())
        );

        std::fs::remove_file(&file).unwrap();
        let missing = resolve_file_reference_target(&roots, &source).unwrap();
        assert_eq!(missing.absolute_path, target.absolute_path);
        let FileReferenceAvailability::Unavailable { code, .. } = missing.availability else {
            panic!("missing attachment must remain unavailable");
        };
        assert_eq!(code, "conversation.workspace-file-not-found");
    }

    #[test]
    fn missing_artifact_keeps_copy_paths_but_cannot_be_sent() {
        let (_dir, _paths, locator, roots) = fixture();
        let source = PromptWorkspaceFileRef::Artifact {
            locator,
            name: "missing".into(),
        };
        let target = resolve_file_reference_target(&roots, &source).unwrap();
        assert!(target.absolute_path.is_some());
        assert!(matches!(
            target.availability,
            FileReferenceAvailability::Unavailable { .. }
        ));
        assert!(resolve_file_reference(&roots, &source).is_err());
    }

    #[test]
    fn turn_reference_reads_current_file_metadata_without_cas_or_legacy_migration() {
        let (_dir, paths, locator, roots) = fixture();
        std::fs::write(paths.repo_root.join("current.txt"), "current version").unwrap();
        let manifest = record_change(&paths, &locator, "current.txt", 1);
        let before = std::fs::read(&manifest).unwrap();
        let source = PromptWorkspaceFileRef::TurnFile {
            locator,
            change_set_id: "set".into(),
            change_id: "change".into(),
        };
        let resolved = resolve_file_reference(&roots, &source).unwrap();
        assert_eq!(resolved.size, 15);
        assert_eq!(resolved.source, source);
        assert!(resolved.canonical_path.ends_with("current.txt"));
        assert_eq!(std::fs::read(manifest).unwrap(), before);
    }

    #[test]
    fn released_worktree_does_not_fall_back_to_a_same_named_main_file() {
        let (dir, paths, locator, roots) = fixture();
        std::fs::write(paths.repo_root.join("current.txt"), "main").unwrap();
        let missing = dir.path().join("released-worktree");
        write_json(
            &paths.run_file("task", "run"),
            &serde_json::json!({ "id":"run", "task_id":"task", "task_uuid":"task-uuid",
            "worktree": {"path":missing, "branch":"old", "forkCommit":"commit"} }),
        )
        .unwrap();
        record_change(&paths, &locator, "current.txt", 7);
        let source = PromptWorkspaceFileRef::TurnFile {
            locator,
            change_set_id: "set".into(),
            change_id: "change".into(),
        };
        let target = resolve_file_reference_target(&roots, &source).unwrap();
        assert!(target.absolute_path.unwrap().contains("released-worktree"));
        assert!(matches!(
            target.availability,
            FileReferenceAvailability::Unavailable { .. }
        ));
        assert!(resolve_file_reference(&roots, &source).is_err());
    }

    #[test]
    fn display_root_and_external_logical_path_do_not_authorize_external_reads() {
        let (dir, paths, locator, roots) = fixture();
        let external = dir.path().join("private.txt");
        std::fs::write(&external, "private").unwrap();
        record_change(&paths, &locator, external.to_str().unwrap(), 7);
        let source = PromptWorkspaceFileRef::TurnFile {
            locator,
            change_set_id: "set".into(),
            change_id: "change".into(),
        };
        let target = resolve_file_reference_target(&roots, &source).unwrap();
        assert!(target.relative_path.is_none());
        assert!(matches!(
            target.availability,
            FileReferenceAvailability::Unavailable { .. }
        ));
        assert!(resolve_file_reference(&roots, &source).is_err());
    }

    #[test]
    fn rejects_source_traversal_wrong_task_identity_and_branch_ownership() {
        let (_dir, paths, locator, roots) = fixture();
        record_change(&paths, &locator, "current.txt", 7);
        let mut wrong = locator.clone();
        wrong.task_uuid = Some("another-task".into());
        assert!(
            resolve_file_reference_target(
                &roots,
                &PromptWorkspaceFileRef::Artifact {
                    locator: wrong,
                    name: "x".into()
                }
            )
            .is_err()
        );
        assert!(
            resolve_file_reference_target(
                &roots,
                &PromptWorkspaceFileRef::Artifact {
                    locator: locator.clone(),
                    name: "../x".into()
                }
            )
            .is_err()
        );
        let mut wrong = locator;
        wrong.branch_id = format!("agent-{}", "a".repeat(32));
        assert!(
            resolve_file_reference_target(
                &roots,
                &PromptWorkspaceFileRef::TurnFile {
                    locator: wrong,
                    change_set_id: "set".into(),
                    change_id: "change".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn git_and_turn_references_use_a_real_linked_worktree_and_reject_an_unrelated_root() {
        let (dir, paths, locator, roots) = fixture();
        let git = |args: &[&str]| {
            let output = crate::process::background_command("git")
                .current_dir(&paths.repo_root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        std::fs::write(paths.repo_root.join("current.txt"), "main").unwrap();
        git(&["add", "current.txt"]);
        git(&[
            "-c",
            "user.name=File Ref Test",
            "-c",
            "user.email=file-ref@example.invalid",
            "commit",
            "-m",
            "fixture",
        ]);
        let worktree = dir.path().join("linked-worktree");
        git(&["worktree", "add", "--detach", worktree.to_str().unwrap()]);
        std::fs::write(worktree.join("current.txt"), "current worktree contents").unwrap();
        let source = PromptWorkspaceFileRef::GitFile {
            project_id: paths.project_id.clone(),
            workspace_path: Some(display_path(&worktree)),
            relative_path: "current.txt".into(),
        };
        let resolved = resolve_file_reference(&roots, &source).unwrap();
        assert_eq!(resolved.size, 25);
        assert!(resolved.canonical_path.contains("linked-worktree"));
        write_json(
            &paths.run_file("task", "run"),
            &serde_json::json!({ "id":"run", "task_id":"task", "task_uuid":"task-uuid",
            "worktree": {"path":worktree, "branch":"old", "forkCommit":"commit"} }),
        )
        .unwrap();
        record_change(&paths, &locator, "current.txt", 7);
        let turn = PromptWorkspaceFileRef::TurnFile {
            locator,
            change_set_id: "set".into(),
            change_id: "change".into(),
        };
        assert_eq!(
            resolve_file_reference(&roots, &turn)
                .unwrap()
                .canonical_path,
            resolved.canonical_path
        );
        std::fs::write(worktree.join("current.txt"), "changed again").unwrap();
        assert_eq!(resolve_file_reference(&roots, &turn).unwrap().size, 13);
        let unrelated = PromptWorkspaceFileRef::GitFile {
            project_id: paths.project_id.clone(),
            workspace_path: Some(display_path(dir.path())),
            relative_path: "repo/current.txt".into(),
        };
        assert!(resolve_file_reference(&roots, &unrelated).is_err());
        std::fs::remove_file(worktree.join("current.txt")).unwrap();
        assert!(resolve_file_reference(&roots, &turn).is_err());
        git(&["worktree", "remove", "--force", worktree.to_str().unwrap()]);
    }
}
