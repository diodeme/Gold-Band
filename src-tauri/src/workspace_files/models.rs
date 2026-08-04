use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileRevisionVm {
    pub byte_length: u64,
    pub modified_at_ns: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileLocatorVm {
    pub project_id: String,
    pub canonical_path: String,
    pub relative_path: Option<String>,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalFileAccessGrantVm {
    pub token: String,
    pub permissions: Vec<String>,
    pub expires_at_ms: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileTargetLocationVm {
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorkspaceFileLinkVm {
    pub locator: WorkspaceFileLocatorVm,
    pub target: Option<FileTargetLocationVm>,
    pub external_access_grant: Option<ExternalFileAccessGrantVm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirectoryEntryVm {
    pub name: String,
    pub relative_path: String,
    pub canonical_path: String,
    pub kind: String,
    pub has_children: bool,
    pub byte_length: Option<u64>,
    pub modified_at_ns: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileSearchVm {
    pub request_id: String,
    pub entries: Vec<WorkspaceDirectoryEntryVm>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum WorkspaceFileSnapshotVm {
    Text {
        locator: WorkspaceFileLocatorVm,
        name: String,
        revision: FileRevisionVm,
        content: String,
        encoding: String,
        language: Option<String>,
        line_ending: String,
        editable: bool,
        limitation_code: Option<String>,
        external_access_grant: Option<ExternalFileAccessGrantVm>,
    },
    Image {
        locator: WorkspaceFileLocatorVm,
        name: String,
        revision: FileRevisionVm,
        mime_type: String,
        width: u32,
        height: u32,
        animated: bool,
        preview_grant: WorkspaceFilePreviewGrantVm,
        source_editable: bool,
        external_access_grant: Option<ExternalFileAccessGrantVm>,
    },
    Unsupported {
        locator: WorkspaceFileLocatorVm,
        name: String,
        revision: FileRevisionVm,
        mime_type: Option<String>,
        limitation_code: String,
        external_access_grant: Option<ExternalFileAccessGrantVm>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWorkspaceDirectoryInput {
    pub project_id: String,
    #[serde(default)]
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchWorkspaceFilesInput {
    pub project_id: String,
    pub query: String,
    pub request_id: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveWorkspaceFileLinkInput {
    pub project_id: String,
    pub raw_href: String,
    #[serde(default)]
    pub base_canonical_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileResourceInput {
    pub project_id: String,
    pub canonical_path: String,
    pub external_access_token: Option<String>,
    #[serde(default)]
    pub prefer_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveMarkdownImageInput {
    pub project_id: String,
    pub markdown_canonical_path: String,
    pub markdown_external_access_token: Option<String>,
    pub raw_src: String,
    #[serde(default)]
    pub approved_external_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilePreviewGrantVm {
    pub token: String,
    pub expires_at_ms: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MarkdownImagePreviewVm {
    Ready {
        canonical_path: String,
        preview_grant: WorkspaceFilePreviewGrantVm,
        mime_type: String,
        width: u32,
        height: u32,
        animated: bool,
    },
    ApprovalRequired {
        canonical_path: String,
        reason: String,
    },
    Unsupported {
        limitation_code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileResourceInput {
    pub project_id: String,
    pub canonical_path: String,
    pub external_access_token: Option<String>,
    pub content: String,
    pub encoding: String,
    pub line_ending: String,
    pub expected_revision: FileRevisionVm,
    pub operation_id: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileChangedEventVm {
    pub project_id: String,
    pub canonical_path: String,
    pub kind: String,
    pub revision: Option<FileRevisionVm>,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileWatchInput {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileTokenInput {
    pub token: String,
}
