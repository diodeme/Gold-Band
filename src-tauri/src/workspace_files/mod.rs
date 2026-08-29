mod models;
mod paths;
mod runtime;
mod service;
mod watcher;

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{CommandResult, spawn_blocking_command};
use crate::state::DesktopState;

pub use models::*;
pub use runtime::WorkspaceFileRuntime;
pub use watcher::WorkspaceFileWatchRuntime;

pub const WORKSPACE_FILE_PREVIEW_PROTOCOL: &str = "gold-band-preview";

pub(crate) fn revision_for_preview(path: &Path) -> CommandResult<FileRevisionVm> {
    service::revision_for_path(path)
}

use paths::{
    canonicalize_file, locator_for_path, parse_file_link_from, path_is_within,
    resolve_workspace_relative_path, resolve_workspace_root,
};

#[tauri::command]
pub async fn list_workspace_directory(
    state: State<'_, DesktopState>,
    input: ListWorkspaceDirectoryInput,
) -> CommandResult<Vec<WorkspaceDirectoryEntryVm>> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    let directory = resolve_workspace_relative_path(&root, &input.relative_path)?;
    spawn_blocking_command(move || service::list_directory(&root, &directory)).await
}

/// Reveal an existing workspace entry in the native file manager. The path is
/// resolved relative to the registered workspace root so callers cannot ask
/// the desktop process to reveal arbitrary local paths.
#[tauri::command]
pub async fn open_workspace_path_in_file_manager(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    input: OpenWorkspacePathInFileManagerInput,
) -> CommandResult<()> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    let path = resolve_workspace_relative_path(&root, &input.relative_path)?;
    app_handle.opener().reveal_item_in_dir(&path).map_err(|error| {
        paths::error(
            "workspace-file.file-manager-open-failed",
            serde_json::json!({ "path": paths::display_path(&path), "reason": error.to_string() }),
        )
    })
}

pub(crate) fn read_file_from_directory_root(
    project_id: String,
    root_path: PathBuf,
    path: PathBuf,
    runtime: WorkspaceFileRuntime,
) -> CommandResult<WorkspaceFileSnapshotVm> {
    let root_path = std::fs::canonicalize(root_path).map_err(|error| {
        paths::error(
            "conversation-directory.not-found",
            serde_json::json!({ "reason": error.to_string() }),
        )
    })?;
    let path = canonicalize_file(&path, "read")?;
    if !path_is_within(&path, &root_path) {
        return Err(paths::error(
            "conversation-directory.path-outside-root",
            serde_json::json!({ "path": paths::display_path(&path) }),
        ));
    }
    let root = paths::ResolvedWorkspaceRoot {
        project_id,
        path: root_path,
        config: gold_band::config::WorkspaceFilesConfig::default(),
    };
    service::read_file(&root, &runtime, &path, None, false)
}

#[tauri::command]
pub async fn search_workspace_files(
    state: State<'_, DesktopState>,
    input: SearchWorkspaceFilesInput,
) -> CommandResult<WorkspaceFileSearchVm> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    spawn_blocking_command(move || {
        service::search_files(&root, &input.query, input.request_id, input.limit)
    })
    .await
}

#[tauri::command]
pub async fn resolve_workspace_file_link(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: ResolveWorkspaceFileLinkInput,
) -> CommandResult<ResolvedWorkspaceFileLinkVm> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    let parse_root = root.clone();
    let raw_href = input.raw_href;
    let base_canonical_path = input.base_canonical_path;
    let (path, target) = spawn_blocking_command(move || {
        parse_file_link_from(
            &parse_root,
            &raw_href,
            base_canonical_path.as_deref().map(Path::new),
        )
    })
    .await?;
    let locator = locator_for_path(&root, &path);
    let external_access_grant = if locator.scope == "external" {
        let grant = runtime.issue_external_grant(
            root.project_id.clone(),
            path.clone(),
            root.config.external_access_grant_ttl_seconds,
        )?;
        if let Err(error) = watch_runtime.start_external(
            app_handle,
            runtime.inner().clone(),
            grant.token.clone(),
            root.project_id.clone(),
            path,
            root.config.watch_debounce_ms,
        ) {
            let _ = runtime.release_external_grant(&grant.token);
            return Err(error);
        }
        Some(grant)
    } else {
        None
    };
    Ok(ResolvedWorkspaceFileLinkVm {
        locator,
        target,
        external_access_grant,
    })
}

pub(crate) fn resolve_trusted_file(
    app_handle: AppHandle,
    state: &DesktopState,
    runtime: &WorkspaceFileRuntime,
    watch_runtime: &WorkspaceFileWatchRuntime,
    project_id: &str,
    path: PathBuf,
) -> CommandResult<ResolvedWorkspaceFileLinkVm> {
    let root = resolve_workspace_root(state, project_id)?;
    let path = canonicalize_file(&path, "read")?;
    let locator = locator_for_path(&root, &path);
    let external_access_grant = if locator.scope == "external" {
        let grant = runtime.issue_external_grant(
            root.project_id.clone(),
            path.clone(),
            root.config.external_access_grant_ttl_seconds,
        )?;
        if let Err(error) = watch_runtime.start_external(
            app_handle,
            runtime.clone(),
            grant.token.clone(),
            root.project_id,
            path,
            root.config.watch_debounce_ms,
        ) {
            let _ = runtime.release_external_grant(&grant.token);
            return Err(error);
        }
        Some(grant)
    } else {
        None
    };
    Ok(ResolvedWorkspaceFileLinkVm {
        locator,
        target: None,
        external_access_grant,
    })
}

#[tauri::command]
pub async fn read_file_resource(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: ReadFileResourceInput,
) -> CommandResult<WorkspaceFileSnapshotVm> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    let path = canonicalize_file(Path::new(&input.canonical_path), "read")?;
    let external_access_grant = authorize_external_if_needed(
        runtime.inner(),
        &root.project_id,
        &root.path,
        &path,
        input.external_access_token.as_deref(),
        "read",
    )?;
    let runtime_clone = runtime.inner().clone();
    let read_root = root.clone();
    let read_path = path.clone();
    let prefer_source = input.prefer_source;
    let snapshot = spawn_blocking_command(move || {
        service::read_file(
            &read_root,
            &runtime_clone,
            &read_path,
            external_access_grant,
            prefer_source,
        )
    })
    .await?;
    if let Some(token) = input.external_access_token {
        watch_runtime.start_external(
            app_handle,
            runtime.inner().clone(),
            token,
            root.project_id,
            path,
            root.config.watch_debounce_ms,
        )?;
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn resolve_markdown_image(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: ResolveMarkdownImageInput,
) -> CommandResult<MarkdownImagePreviewVm> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    let markdown_path = canonicalize_file(Path::new(&input.markdown_canonical_path), "read")?;
    authorize_external_if_needed(
        runtime.inner(),
        &root.project_id,
        &root.path,
        &markdown_path,
        input.markdown_external_access_token.as_deref(),
        "read",
    )?;
    let image_path = resolve_markdown_image_path(&markdown_path, &input.raw_src)?;
    let markdown_directory = markdown_path.parent().ok_or_else(|| {
        paths::error(
            "workspace-file.markdown-image-src-invalid",
            serde_json::json!({ "src": input.raw_src }),
        )
    })?;
    let auto_allowed =
        path_is_within(&image_path, &root.path) || path_is_within(&image_path, markdown_directory);
    let explicitly_approved = input.approved_external_targets.iter().any(|candidate| {
        canonicalize_file(Path::new(candidate), "read").is_ok_and(|approved| approved == image_path)
    });
    if !auto_allowed && !explicitly_approved {
        return Ok(MarkdownImagePreviewVm::ApprovalRequired {
            canonical_path: paths::display_path(&image_path),
            reason: "outside-document-directory".to_owned(),
        });
    }

    let runtime = runtime.inner().clone();
    let read_root = root.clone();
    let snapshot = spawn_blocking_command(move || {
        service::read_file(&read_root, &runtime, &image_path, None, false)
    })
    .await?;
    Ok(match snapshot {
        WorkspaceFileSnapshotVm::Image {
            locator,
            preview_grant,
            mime_type,
            width,
            height,
            animated,
            ..
        } => MarkdownImagePreviewVm::Ready {
            canonical_path: locator.canonical_path,
            preview_grant,
            mime_type,
            width,
            height,
            animated,
        },
        WorkspaceFileSnapshotVm::Unsupported {
            limitation_code, ..
        } => MarkdownImagePreviewVm::Unsupported { limitation_code },
        WorkspaceFileSnapshotVm::Text { .. } => MarkdownImagePreviewVm::Unsupported {
            limitation_code: "workspace-file.format-unsupported".to_owned(),
        },
    })
}

fn resolve_markdown_image_path(markdown_path: &Path, raw_src: &str) -> CommandResult<PathBuf> {
    let raw = raw_src.trim();
    let lower = raw.to_ascii_lowercase();
    if raw.is_empty()
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || raw.starts_with("\\\\")
        || raw.starts_with("//")
    {
        return Err(paths::error(
            if lower.starts_with("http://") || lower.starts_with("https://") {
                "workspace-file.markdown-image-network-blocked"
            } else if raw.starts_with("\\\\") || raw.starts_with("//") {
                "workspace-file.markdown-image-unc-blocked"
            } else {
                "workspace-file.markdown-image-src-invalid"
            },
            serde_json::json!({ "src": raw }),
        ));
    }
    if lower.starts_with("file:") {
        let url = url::Url::parse(raw).map_err(|_| {
            paths::error(
                "workspace-file.markdown-image-src-invalid",
                serde_json::json!({ "src": raw }),
            )
        })?;
        if url
            .host_str()
            .is_some_and(|host| !host.is_empty() && host != "localhost")
        {
            return Err(paths::error(
                "workspace-file.markdown-image-unc-blocked",
                serde_json::json!({ "src": raw }),
            ));
        }
        let path = url.to_file_path().map_err(|_| {
            paths::error(
                "workspace-file.markdown-image-src-invalid",
                serde_json::json!({ "src": raw }),
            )
        })?;
        return canonicalize_file(&path, "read");
    }
    let decoded = percent_decode_str(raw).decode_utf8().map_err(|_| {
        paths::error(
            "workspace-file.markdown-image-src-invalid",
            serde_json::json!({ "src": raw }),
        )
    })?;
    let path = Path::new(decoded.as_ref());
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        markdown_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(path)
    };
    canonicalize_file(&candidate, "read")
}

#[tauri::command]
pub async fn write_file_resource(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: WriteFileResourceInput,
) -> CommandResult<FileRevisionVm> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    let path = canonicalize_file(Path::new(&input.canonical_path), "write")?;
    authorize_external_if_needed(
        runtime.inner(),
        &root.project_id,
        &root.path,
        &path,
        input.external_access_token.as_deref(),
        "write",
    )?;
    let runtime = runtime.inner().clone();
    spawn_blocking_command(move || service::write_file(&runtime, &path, &input)).await
}

#[tauri::command]
pub fn release_workspace_file_preview(
    runtime: State<'_, WorkspaceFileRuntime>,
    input: WorkspaceFileTokenInput,
) -> CommandResult<()> {
    runtime.release_preview(&input.token)
}

#[tauri::command]
pub fn renew_external_file_access(
    runtime: State<'_, WorkspaceFileRuntime>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: WorkspaceFileTokenInput,
) -> CommandResult<ExternalFileAccessGrantVm> {
    let next = runtime.renew_external_grant(&input.token)?;
    watch_runtime.rotate_external(&input.token, next.token.clone())?;
    Ok(next)
}

#[tauri::command]
pub fn release_external_file_access(
    runtime: State<'_, WorkspaceFileRuntime>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: WorkspaceFileTokenInput,
) -> CommandResult<()> {
    watch_runtime.stop_external(&input.token)?;
    runtime.release_external_grant(&input.token)
}

#[tauri::command]
pub fn start_workspace_file_watch(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: WorkspaceFileWatchInput,
) -> CommandResult<()> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    watch_runtime.start_workspace(
        app_handle,
        runtime.inner().clone(),
        root.project_id,
        root.path,
        root.config.watch_debounce_ms,
    )
}

#[tauri::command]
pub fn stop_workspace_file_watch(
    state: State<'_, DesktopState>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: WorkspaceFileWatchInput,
) -> CommandResult<()> {
    let root = resolve_workspace_root(state.inner(), &input.project_id)?;
    watch_runtime.stop_workspace(&root.project_id, &root.path)
}

fn authorize_external_if_needed(
    runtime: &WorkspaceFileRuntime,
    project_id: &str,
    root: &Path,
    path: &Path,
    external_access_token: Option<&str>,
    operation: &str,
) -> CommandResult<Option<ExternalFileAccessGrantVm>> {
    if path_is_within(path, root) {
        return Ok(None);
    }
    runtime
        .validate_external_grant(external_access_token, project_id, path, operation)
        .map(Some)
}

pub fn preview_protocol_response(
    runtime: &WorkspaceFileRuntime,
    request_path: &str,
) -> tauri::http::Response<Vec<u8>> {
    let path = request_path.trim_matches('/');
    let (token, static_frame) = path
        .strip_suffix("/static")
        .map(|token| (token, true))
        .unwrap_or((path, false));
    runtime.preview_protocol_response(token, static_frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn external_grant_is_bound_to_exact_project_and_path() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let grant = runtime
            .issue_external_grant("project-a".to_string(), first.clone(), 30)
            .unwrap();

        assert!(
            runtime
                .validate_external_grant(Some(&grant.token), "project-a", &first, "write")
                .is_ok()
        );
        assert!(
            runtime
                .validate_external_grant(Some(&grant.token), "project-a", &second, "write")
                .is_err()
        );
        assert!(
            runtime
                .validate_external_grant(Some(&grant.token), "project-b", &first, "write")
                .is_err()
        );
    }

    #[test]
    fn released_external_grant_cannot_be_reused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("value.txt");
        std::fs::write(&path, "value").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let grant = runtime
            .issue_external_grant("project-a".to_string(), path.clone(), 30)
            .unwrap();
        runtime.release_external_grant(&grant.token).unwrap();
        assert!(
            runtime
                .validate_external_grant(Some(&grant.token), "project-a", &path, "read")
                .is_err()
        );
    }

    #[test]
    fn markdown_image_paths_are_resolved_relative_to_the_document_after_percent_decoding() {
        let dir = tempdir().unwrap();
        let markdown = dir.path().join("README.md");
        let image_dir = dir.path().join("assets");
        std::fs::create_dir(&image_dir).unwrap();
        let image = image_dir.join("screen shot.png");
        std::fs::write(&markdown, "![screen](assets/screen%20shot.png)").unwrap();
        std::fs::write(&image, b"image").unwrap();

        assert_eq!(
            resolve_markdown_image_path(&markdown, "assets/screen%20shot.png").unwrap(),
            std::fs::canonicalize(image).unwrap(),
        );
        let file_url = url::Url::from_file_path(image_dir.join("screen shot.png")).unwrap();
        assert_eq!(
            resolve_markdown_image_path(&markdown, file_url.as_str()).unwrap(),
            std::fs::canonicalize(image_dir.join("screen shot.png")).unwrap(),
        );
    }

    #[test]
    fn markdown_image_paths_reject_network_and_dangerous_sources() {
        let dir = tempdir().unwrap();
        let markdown = dir.path().join("README.md");
        std::fs::write(&markdown, "# Readme").unwrap();

        assert_eq!(
            resolve_markdown_image_path(&markdown, "https://tracker.example/image.png")
                .unwrap_err()
                .code,
            "workspace-file.markdown-image-network-blocked",
        );
        assert_eq!(
            resolve_markdown_image_path(&markdown, "data:image/png;base64,AAAA")
                .unwrap_err()
                .code,
            "workspace-file.markdown-image-src-invalid",
        );
        assert_eq!(
            resolve_markdown_image_path(&markdown, r"\\server\share\image.png")
                .unwrap_err()
                .code,
            "workspace-file.markdown-image-unc-blocked",
        );
    }

    #[test]
    fn external_text_write_requires_and_accepts_the_exact_file_grant() {
        let workspace = tempdir().unwrap();
        let external = tempdir().unwrap();
        let path = external.path().join("external.txt");
        std::fs::write(&path, "before").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        assert!(
            authorize_external_if_needed(
                &runtime,
                "project-a",
                workspace.path(),
                &path,
                None,
                "write",
            )
            .is_err()
        );
        let grant = runtime
            .issue_external_grant("project-a".to_string(), path.clone(), 30)
            .unwrap();
        authorize_external_if_needed(
            &runtime,
            "project-a",
            workspace.path(),
            &path,
            Some(&grant.token),
            "write",
        )
        .unwrap();
        let expected_revision = service::revision_for_path(&path).unwrap();
        service::write_file(
            &runtime,
            &path,
            &WriteFileResourceInput {
                project_id: "project-a".to_string(),
                canonical_path: path.to_string_lossy().into_owned(),
                external_access_token: Some(grant.token),
                content: "after".to_string(),
                encoding: "utf-8".to_string(),
                line_ending: "lf".to_string(),
                expected_revision,
                operation_id: "external-write".to_string(),
                force: false,
            },
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "after");
    }
}
