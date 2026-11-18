use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_PROMPT_WORKSPACE_FILES: usize = 10;

pub fn workspace_file_authoring_identity(project_id: &str, path: &str) -> (String, String) {
    let project_id = project_id.trim().to_string();
    let path = path.trim().replace('\\', "/");
    if cfg!(windows) {
        (project_id.to_ascii_lowercase(), path.to_ascii_lowercase())
    } else {
        (project_id, path)
    }
}

mod sources;
pub use sources::{
    FileReferenceTarget, PromptFileAttemptLocator, PromptWorkspaceFileRef,
    resolve_file_reference_target,
};

#[derive(Debug, Clone)]
struct WorkspacePathFileRef {
    project_id: String,
    relative_path: String,
}

/// A registered workspace the resolver may read. A reference resolves to the
/// canonical absolute path inside this root; callers do not supply absolute
/// paths as reference identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptWorkspaceRoot {
    pub project_id: String,
    pub root: PathBuf,
    #[serde(skip)]
    pub paths: Option<crate::storage::GoldBandPaths>,
}

pub fn prompt_workspace_roots(
    current_project_id: &str,
    current_root: &Path,
    registered: &[(String, PathBuf)],
) -> Vec<PromptWorkspaceRoot> {
    let mut roots = registered
        .iter()
        .filter(|(project_id, root)| !project_id.trim().is_empty() && !root.as_os_str().is_empty())
        .map(|(project_id, root)| PromptWorkspaceRoot {
            project_id: project_id.clone(),
            root: root.clone(),
            paths: camino::Utf8Path::from_path(root).map(crate::storage::GoldBandPaths::new),
        })
        .collect::<Vec<_>>();
    if current_project_id.trim().is_empty() {
        return roots;
    }
    if let Some(existing) = roots
        .iter_mut()
        .find(|root| root.project_id == current_project_id)
    {
        existing.root = current_root.to_path_buf();
    } else {
        roots.push(PromptWorkspaceRoot {
            project_id: current_project_id.to_string(),
            root: current_root.to_path_buf(),
            paths: None,
        });
    }
    roots
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorkspaceFileRef {
    pub source: PromptWorkspaceFileRef,
    pub project_id: String,
    pub relative_path: String,
    pub canonical_path: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceFileRefError {
    WorkspaceUnavailable {
        workspace_path: String,
    },
    ProjectUnavailable {
        project_id: String,
    },
    InvalidPath {
        project_id: String,
        relative_path: String,
    },
    OutsideWorkspace {
        project_id: String,
        relative_path: String,
    },
    NotFound {
        project_id: String,
        relative_path: String,
    },
    NotAFile {
        project_id: String,
        relative_path: String,
    },
    PermissionDenied {
        project_id: String,
        relative_path: String,
    },
    IoFailed {
        project_id: String,
        relative_path: String,
    },
    CountExceeded {
        max: usize,
    },
}

impl WorkspaceFileRefError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::WorkspaceUnavailable { .. } => "workspace-file.workspace-unavailable",
            Self::ProjectUnavailable { .. } => "conversation.workspace-file-project-unavailable",
            Self::InvalidPath { .. } => "conversation.workspace-file-path-invalid",
            Self::OutsideWorkspace { .. } => "conversation.workspace-file-outside-workspace",
            Self::NotFound { .. } => "conversation.workspace-file-not-found",
            Self::NotAFile { .. } => "conversation.workspace-file-not-a-file",
            Self::PermissionDenied { .. } => "conversation.workspace-file-permission-denied",
            Self::IoFailed { .. } => "conversation.workspace-file-read-failed",
            Self::CountExceeded { .. } => "conversation.workspace-file-count-exceeded",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::WorkspaceUnavailable { workspace_path } => {
                serde_json::json!({ "workspacePath": workspace_path })
            }
            Self::ProjectUnavailable { project_id } => serde_json::json!({
                "projectId": project_id,
            }),
            Self::InvalidPath {
                project_id,
                relative_path,
            }
            | Self::OutsideWorkspace {
                project_id,
                relative_path,
            }
            | Self::NotFound {
                project_id,
                relative_path,
            }
            | Self::NotAFile {
                project_id,
                relative_path,
            }
            | Self::PermissionDenied {
                project_id,
                relative_path,
            }
            | Self::IoFailed {
                project_id,
                relative_path,
            } => serde_json::json!({
                "projectId": project_id,
                "relativePath": relative_path,
            }),
            Self::CountExceeded { max } => serde_json::json!({ "max": max }),
        }
    }

    pub fn diagnostic(&self) -> String {
        format!("workspace file reference rejected: {}", self.code())
    }
}

pub fn resolve_prompt_workspace_files(
    roots: &[PromptWorkspaceRoot],
    references: &[PromptWorkspaceFileRef],
    attachment_count: usize,
) -> Result<Vec<ResolvedWorkspaceFileRef>, WorkspaceFileRefError> {
    let mut authoring_seen = HashSet::new();
    let mut canonical_seen = HashSet::new();
    let mut resolved = Vec::new();
    for reference in references {
        if !authoring_seen.insert(reference.authoring_identity()) {
            continue;
        }
        let normalized = sources::resolve_file_reference(roots, reference)?;
        if canonical_seen.insert(canonical_file_identity(&normalized.canonical_path)) {
            resolved.push(normalized);
        }
    }
    if resolved.len() + attachment_count > MAX_PROMPT_WORKSPACE_FILES {
        return Err(WorkspaceFileRefError::CountExceeded {
            max: MAX_PROMPT_WORKSPACE_FILES,
        });
    }
    Ok(resolved)
}

/// Workspace files are always referenced by path; the Agent reads what it needs.
pub fn resolved_workspace_file_content_block(
    reference: &ResolvedWorkspaceFileRef,
) -> super::AcpContentBlock {
    super::AcpContentBlock::File(super::AcpFileRef {
        path: reference.canonical_path.clone(),
        size: reference.size,
    })
}

fn canonical_file_identity(canonical_path: &str) -> String {
    let path = canonical_path.replace('\\', "/");
    if cfg!(windows) {
        path.to_ascii_lowercase()
    } else {
        path
    }
}

fn normalize_reference(
    reference: &WorkspacePathFileRef,
    root: &Path,
) -> Result<ResolvedWorkspaceFileRef, WorkspaceFileRefError> {
    let relative_path = reference.relative_path.clone();
    let canonical =
        std::fs::canonicalize(root.join(&relative_path)).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => WorkspaceFileRefError::NotFound {
                project_id: reference.project_id.clone(),
                relative_path: reference.relative_path.clone(),
            },
            std::io::ErrorKind::PermissionDenied => WorkspaceFileRefError::PermissionDenied {
                project_id: reference.project_id.clone(),
                relative_path: reference.relative_path.clone(),
            },
            _ => WorkspaceFileRefError::IoFailed {
                project_id: reference.project_id.clone(),
                relative_path: reference.relative_path.clone(),
            },
        })?;
    if !path_is_within(&canonical, root) {
        return Err(WorkspaceFileRefError::OutsideWorkspace {
            project_id: reference.project_id.clone(),
            relative_path: reference.relative_path.clone(),
        });
    }
    let metadata = std::fs::metadata(&canonical).map_err(|_| WorkspaceFileRefError::IoFailed {
        project_id: reference.project_id.clone(),
        relative_path: reference.relative_path.clone(),
    })?;
    if !metadata.is_file() {
        return Err(WorkspaceFileRefError::NotAFile {
            project_id: reference.project_id.clone(),
            relative_path: reference.relative_path.clone(),
        });
    }
    let name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| relative_path.clone());
    let mime_type = super::file_mime_type(&canonical);
    Ok(ResolvedWorkspaceFileRef {
        source: PromptWorkspaceFileRef::Workspace {
            project_id: reference.project_id.clone(),
            relative_path: relative_path.clone(),
        },
        project_id: reference.project_id.clone(),
        relative_path,
        canonical_path: display_path(&canonical),
        name,
        mime_type,
        size: metadata.len(),
    })
}

fn lexical_relative_path(
    reference: &WorkspacePathFileRef,
) -> Result<String, WorkspaceFileRefError> {
    let relative_path = reference.relative_path.trim().replace('\\', "/");
    let windows_drive_prefix = relative_path
        .as_bytes()
        .get(0)
        .is_some_and(u8::is_ascii_alphabetic)
        && relative_path.as_bytes().get(1) == Some(&b':');
    let invalid_components = Path::new(&relative_path).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if relative_path.is_empty()
        || reference.relative_path.contains('\0')
        || relative_path.starts_with('/')
        || windows_drive_prefix
        || invalid_components
    {
        return Err(WorkspaceFileRefError::InvalidPath {
            project_id: reference.project_id.clone(),
            relative_path: reference.relative_path.clone(),
        });
    }
    Ok(relative_path)
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let normalize = |value: &Path| -> String {
        let mut text = value.to_string_lossy().replace('\\', "/");
        while text.len() > 3 && text.ends_with('/') {
            text.pop();
        }
        if cfg!(windows) {
            text.to_ascii_lowercase()
        } else {
            text
        }
    };
    let path = normalize(path);
    let root = normalize(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(network_path) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{network_path}");
    }
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ConversationPromptInput;
    use tempfile::TempDir;

    #[test]
    fn artifact_reference_retains_its_origin_in_prompt_input() {
        let raw = serde_json::json!({
            "displayText": "", "quotes": [],
            "workspaceFiles": [{"kind": "artifact", "name": "review.json", "locator": {
                "projectId": "p", "taskId": "task", "runId": "run", "roundId": "round",
                "nodeId": "node", "attemptId": "attempt", "branchId": "main"
            }}]
        });
        let input: ConversationPromptInput = serde_json::from_value(raw.clone()).unwrap();
        let serialized = serde_json::to_value(input).unwrap();
        assert_eq!(serialized["workspaceFiles"], raw["workspaceFiles"]);
    }

    fn resolve_at(
        project_id: &str,
        references: &[PromptWorkspaceFileRef],
        root: &std::path::Path,
        attachment_count: usize,
    ) -> Result<Vec<ResolvedWorkspaceFileRef>, WorkspaceFileRefError> {
        resolve_prompt_workspace_files(
            &[PromptWorkspaceRoot {
                project_id: project_id.to_string(),
                root: root.to_path_buf(),
                paths: None,
            }],
            references,
            attachment_count,
        )
    }

    fn input(project_id: &str, path: &str) -> ConversationPromptInput {
        ConversationPromptInput {
            display_text: String::new(),
            quotes: Vec::new(),
            role: None,
            workspace_files: vec![PromptWorkspaceFileRef::Workspace {
                project_id: project_id.to_string(),
                relative_path: path.to_string(),
            }],
        }
    }

    #[test]
    fn workspace_file_reference_is_a_valid_empty_text_prompt() {
        let value = input("p", "src/a.ts");
        assert!(crate::provider::conversation_prompt_has_payload(
            &value.display_text,
            0,
            value.role.as_ref(),
            value.workspace_files.len()
        ));
    }

    #[test]
    fn resolves_normalizes_and_deduplicates_workspace_files() {
        let root = TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src").join("WorkspaceFileTree.tsx"), "x").unwrap();
        let mut value = input("p", "src\\WorkspaceFileTree.tsx");
        value
            .workspace_files
            .push(PromptWorkspaceFileRef::Workspace {
                project_id: "p".to_string(),
                relative_path: "src/WorkspaceFileTree.tsx".to_string(),
            });

        let resolved = resolve_at("p", &value.workspace_files, root.path(), 0).unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].relative_path, "src/WorkspaceFileTree.tsx");
        assert_eq!(resolved[0].name, "WorkspaceFileTree.tsx");
        assert_eq!(resolved[0].mime_type, "text/typescript");
        assert_eq!(resolved[0].size, 1);
    }

    #[test]
    #[cfg(windows)]
    fn windows_authoring_identity_normalizes_case_and_separators() {
        assert_eq!(
            workspace_file_authoring_identity("P", "SRC\\WorkspaceFile.TSX"),
            workspace_file_authoring_identity("p", "src/workspacefile.tsx")
        );
    }

    #[test]
    fn rejects_unsafe_project_and_path_boundaries() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "x").unwrap();
        let mismatch = input("other", "file.txt");
        assert_eq!(
            resolve_at("p", &mismatch.workspace_files, root.path(), 0)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-project-unavailable"
        );
        for path in ["", "..", "/file.txt", "C:/file.txt", "c:\\file.txt"] {
            let mut invalid = input("p", "file.txt");
            invalid.workspace_files[0] = PromptWorkspaceFileRef::Workspace {
                project_id: "p".into(),
                relative_path: path.to_string(),
            };
            assert_eq!(
                resolve_at("p", &invalid.workspace_files, root.path(), 0)
                    .unwrap_err()
                    .code(),
                "conversation.workspace-file-path-invalid"
            );
        }
        let missing = input("p", "missing.txt");
        assert_eq!(
            resolve_at("p", &missing.workspace_files, root.path(), 0)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-not-found"
        );
    }

    #[test]
    fn directories_and_combined_context_limit_are_rejected() {
        let root = TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("dir")).unwrap();
        let directory = input("p", "dir");
        assert_eq!(
            resolve_at("p", &directory.workspace_files, root.path(), 0)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-not-a-file"
        );
        let mut over_limit = input("p", "file.txt");
        over_limit.workspace_files = (0..10)
            .map(|index| PromptWorkspaceFileRef::Workspace {
                project_id: "p".to_string(),
                relative_path: format!("file-{index}.txt"),
            })
            .collect();
        for index in 0..10 {
            std::fs::write(root.path().join(format!("file-{index}.txt")), "x").unwrap();
        }
        assert_eq!(
            resolve_at("p", &over_limit.workspace_files, root.path(), 1)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-count-exceeded"
        );
    }

    #[test]
    fn duplicate_references_do_not_consume_the_context_limit() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "x").unwrap();
        let mut duplicate_limit = input("p", "file.txt");
        duplicate_limit.workspace_files = (0..11)
            .map(|_| PromptWorkspaceFileRef::Workspace {
                project_id: "p".to_string(),
                relative_path: "file.txt".to_string(),
            })
            .collect();

        let resolved = resolve_at("p", &duplicate_limit.workspace_files, root.path(), 0).unwrap();

        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn project_identity_is_compared_by_its_authoritative_value() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "x").unwrap();
        let mismatch = input("P", "file.txt");

        assert_eq!(
            resolve_at("p", &mismatch.workspace_files, root.path(), 0)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-project-unavailable"
        );
    }

    #[test]
    fn other_registered_workspace_reference_uses_its_absolute_path() {
        let conversation = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        std::fs::write(other.path().join("foreign.ts"), "export {}").unwrap();
        let reference = input("other-project", "foreign.ts");

        let resolved = resolve_prompt_workspace_files(
            &[
                PromptWorkspaceRoot {
                    project_id: "conversation".to_string(),
                    root: conversation.path().to_path_buf(),
                    paths: None,
                },
                PromptWorkspaceRoot {
                    project_id: "other-project".to_string(),
                    root: other.path().to_path_buf(),
                    paths: None,
                },
            ],
            &reference.workspace_files,
            0,
        )
        .unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].project_id, "other-project");
        assert_eq!(resolved[0].relative_path, "foreign.ts");
        assert!(std::path::Path::new(&resolved[0].canonical_path).is_absolute());
        let block = resolved_workspace_file_content_block(&resolved[0]);
        match block {
            crate::provider::AcpContentBlock::File(file) => {
                assert_eq!(file.path, resolved[0].canonical_path);
                assert!(file.path.ends_with("foreign.ts"));
            }
            other => panic!("unexpected content block: {other:?}"),
        }
    }

    #[test]
    fn resolved_reference_projects_to_a_path_without_reading_content() {
        let resolved = ResolvedWorkspaceFileRef {
            source: PromptWorkspaceFileRef::Workspace {
                project_id: "p".into(),
                relative_path: "src/a.ts".into(),
            },
            project_id: "p".to_string(),
            relative_path: "src/a.ts".to_string(),
            canonical_path: "D:/work/src/a.ts".to_string(),
            name: "a.ts".to_string(),
            mime_type: "text/typescript".to_string(),
            size: 12,
        };

        let block = resolved_workspace_file_content_block(&resolved);

        match block {
            crate::provider::AcpContentBlock::File(file) => {
                assert_eq!(
                    file,
                    crate::provider::AcpFileRef {
                        path: "D:/work/src/a.ts".to_string(),
                        size: 12,
                    }
                );
            }
            other => panic!("unexpected content block: {other:?}"),
        }
    }
}
