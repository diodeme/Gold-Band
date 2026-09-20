use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

pub const MAX_PROMPT_WORKSPACE_FILES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptWorkspaceFileRef {
    pub project_id: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorkspaceFileRef {
    pub project_id: String,
    pub relative_path: String,
    pub canonical_path: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceFileRefError {
    ProjectMismatch {
        project_id: String,
        expected_project_id: String,
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
            Self::ProjectMismatch { .. } => "conversation.workspace-file-project-mismatch",
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
            Self::ProjectMismatch {
                project_id,
                expected_project_id,
            } => serde_json::json!({
                "projectId": project_id,
                "expectedProjectId": expected_project_id,
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
    expected_project_id: &str,
    references: &[PromptWorkspaceFileRef],
    workspace_root: &Path,
    attachment_count: usize,
) -> Result<Vec<ResolvedWorkspaceFileRef>, WorkspaceFileRefError> {
    if references.len() + attachment_count > MAX_PROMPT_WORKSPACE_FILES {
        return Err(WorkspaceFileRefError::CountExceeded {
            max: MAX_PROMPT_WORKSPACE_FILES,
        });
    }
    let root =
        canonicalize_directory(workspace_root).map_err(|_| WorkspaceFileRefError::IoFailed {
            project_id: expected_project_id.to_string(),
            relative_path: String::new(),
        })?;
    let mut seen = HashSet::new();
    let mut resolved = Vec::with_capacity(references.len());
    for reference in references {
        let normalized = normalize_reference(reference, &root, expected_project_id)?;
        if !seen.insert((reference.project_id.clone(), normalized.1.clone())) {
            continue;
        }
        resolved.push(normalized.0);
    }
    Ok(resolved)
}

pub fn resolved_workspace_file_content_block(
    reference: &ResolvedWorkspaceFileRef,
) -> super::AcpContentBlock {
    super::AcpContentBlock::ResourceLink(super::AcpResourceLinkBlock {
        name: reference.name.clone(),
        uri: file_uri(&reference.canonical_path),
        mime_type: reference.mime_type.clone(),
        size: reference.size,
    })
}

fn normalize_reference(
    reference: &PromptWorkspaceFileRef,
    root: &Path,
    root_project_id: &str,
) -> Result<(ResolvedWorkspaceFileRef, String), WorkspaceFileRefError> {
    // Project identity is validated here because WorkerInvocation receives the
    // authoritative project separately from user-controlled reference payloads.
    if reference.project_id.trim().is_empty()
        || !reference.project_id.eq_ignore_ascii_case(root_project_id)
    {
        return Err(WorkspaceFileRefError::ProjectMismatch {
            project_id: reference.project_id.clone(),
            expected_project_id: root_project_id.to_string(),
        });
    }
    let relative_path = reference.relative_path.trim().replace('\\', "/");
    let relative = Path::new(&relative_path);
    if reference.project_id.trim().is_empty()
        || relative_path.is_empty()
        || reference.relative_path.contains('\0')
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkspaceFileRefError::InvalidPath {
            project_id: reference.project_id.clone(),
            relative_path: reference.relative_path.clone(),
        });
    }
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
    let mime_type = mime_type_for_path(&canonical);
    Ok((
        ResolvedWorkspaceFileRef {
            project_id: reference.project_id.clone(),
            relative_path: relative_path.clone(),
            canonical_path: display_path(&canonical),
            name,
            mime_type,
            size: metadata.len(),
        },
        relative_path,
    ))
}

fn canonicalize_directory(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if !canonical.is_dir() {
        return Err(std::io::Error::other("workspace root is not a directory"));
    }
    Ok(canonical)
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

fn file_uri(path: &str) -> String {
    Url::from_file_path(Path::new(path))
        .map(|url| url.to_string())
        .unwrap_or_else(|_| format!("file://{path}"))
}

fn mime_type_for_path(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "ts" | "tsx" => "text/typescript".to_string(),
        "js" | "jsx" => "text/javascript".to_string(),
        "md" | "markdown" => "text/markdown".to_string(),
        "json" | "jsonl" => "application/json".to_string(),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ConversationPromptInput;
    use tempfile::TempDir;

    fn input(project_id: &str, path: &str) -> ConversationPromptInput {
        ConversationPromptInput {
            display_text: String::new(),
            quotes: Vec::new(),
            role: None,
            workspace_files: vec![PromptWorkspaceFileRef {
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
        value.workspace_files.push(PromptWorkspaceFileRef {
            project_id: "p".to_string(),
            relative_path: "src/WorkspaceFileTree.tsx".to_string(),
        });

        let resolved =
            resolve_prompt_workspace_files("p", &value.workspace_files, root.path(), 0).unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].relative_path, "src/WorkspaceFileTree.tsx");
        assert_eq!(resolved[0].name, "WorkspaceFileTree.tsx");
        assert_eq!(resolved[0].mime_type, "text/typescript");
        assert_eq!(resolved[0].size, 1);
    }

    #[test]
    fn rejects_unsafe_project_and_path_boundaries() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("file.txt"), "x").unwrap();
        let mismatch = input("other", "file.txt");
        assert_eq!(
            resolve_prompt_workspace_files("p", &mismatch.workspace_files, root.path(), 0)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-project-mismatch"
        );
        for path in ["", "..", "/file.txt", "C:/file.txt"] {
            let mut invalid = input("p", "file.txt");
            invalid.workspace_files[0].relative_path = path.to_string();
            assert_eq!(
                resolve_prompt_workspace_files("p", &invalid.workspace_files, root.path(), 0)
                    .unwrap_err()
                    .code(),
                "conversation.workspace-file-path-invalid"
            );
        }
        let missing = input("p", "missing.txt");
        assert_eq!(
            resolve_prompt_workspace_files("p", &missing.workspace_files, root.path(), 0)
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
            resolve_prompt_workspace_files("p", &directory.workspace_files, root.path(), 0)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-not-a-file"
        );
        let mut over_limit = input("p", "file.txt");
        over_limit.workspace_files = (0..10)
            .map(|index| PromptWorkspaceFileRef {
                project_id: "p".to_string(),
                relative_path: format!("file-{index}.txt"),
            })
            .collect();
        assert_eq!(
            resolve_prompt_workspace_files("p", &over_limit.workspace_files, root.path(), 1)
                .unwrap_err()
                .code(),
            "conversation.workspace-file-count-exceeded"
        );
    }

    #[test]
    fn resolved_reference_projects_to_resource_link_without_reading_content() {
        let resolved = ResolvedWorkspaceFileRef {
            project_id: "p".to_string(),
            relative_path: "src/a.ts".to_string(),
            canonical_path: "D:/work/src/a.ts".to_string(),
            name: "a.ts".to_string(),
            mime_type: "text/typescript".to_string(),
            size: 12,
        };

        let block = resolved_workspace_file_content_block(&resolved);

        match block {
            crate::provider::AcpContentBlock::ResourceLink(link) => {
                assert_eq!(link.uri, "file:///D:/work/src/a.ts");
                assert_eq!(link.mime_type, "text/typescript");
                assert_eq!(link.size, 12);
            }
            other => panic!("unexpected content block: {other:?}"),
        }
    }
}
