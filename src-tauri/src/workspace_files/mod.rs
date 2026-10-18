mod models;
mod mutations;
mod paths;
mod runtime;
mod service;
mod trash_bin;
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

pub(crate) async fn resolve_file_link_locator(
    state: &DesktopState,
    runtime: &WorkspaceFileRuntime,
    project_id: &str,
    workspace_path: Option<&str>,
    raw_href: &str,
) -> CommandResult<WorkspaceFileLocatorVm> {
    let root = resolve_workspace_root(state, runtime, project_id, workspace_path).await?;
    let parse_root = root.clone();
    let raw_href = raw_href.to_string();
    let (path, _) =
        spawn_blocking_command(move || parse_file_link_from(&parse_root, &raw_href, None)).await?;
    Ok(locator_for_path(&root, &path))
}

pub(crate) fn revision_for_preview(path: &Path) -> CommandResult<FileRevisionVm> {
    service::revision_for_path(path)
}

use paths::{
    canonicalize_file, locator_for_path, parse_file_link_from, path_is_within,
    resolve_workspace_directory, resolve_workspace_entry_path, resolve_workspace_relative_path,
    resolve_workspace_root, workspace_watch_root,
};

#[tauri::command]
pub async fn list_workspace_directory(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: ListWorkspaceDirectoryInput,
) -> CommandResult<Vec<WorkspaceDirectoryEntryVm>> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
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
    runtime: State<'_, WorkspaceFileRuntime>,
    input: OpenWorkspacePathInFileManagerInput,
) -> CommandResult<()> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
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
        workspace_scope_path: None,
        config: gold_band::config::WorkspaceFilesConfig::default(),
    };
    service::read_file(&root, &runtime, &path, None, false)
}

#[tauri::command]
pub async fn search_workspace_files(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: SearchWorkspaceFilesInput,
) -> CommandResult<WorkspaceFileSearchVm> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
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
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
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

pub(crate) async fn resolve_trusted_file(
    app_handle: AppHandle,
    state: &DesktopState,
    runtime: &WorkspaceFileRuntime,
    watch_runtime: &WorkspaceFileWatchRuntime,
    project_id: &str,
    path: PathBuf,
) -> CommandResult<ResolvedWorkspaceFileLinkVm> {
    let root = resolve_workspace_root(state, runtime, project_id, None).await?;
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
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
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
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    let runtime = runtime.inner().clone();
    spawn_blocking_command(move || resolve_markdown_image_from_root(&root, &runtime, &input)).await
}

/// Shared production boundary: source authorization and all image filesystem I/O
/// run together on the blocking pool, before a revision-bound preview is issued.
fn resolve_markdown_image_from_root(
    root: &paths::ResolvedWorkspaceRoot,
    runtime: &WorkspaceFileRuntime,
    input: &ResolveMarkdownImageInput,
) -> CommandResult<MarkdownImagePreviewVm> {
    let image_path = match &input.source {
        MarkdownImageSource::Workspace => {
            let image_path = resolve_markdown_image_path(&root.path, &input.raw_src)?;
            if !path_is_within(&image_path, &root.path) {
                return Err(paths::error(
                    "workspace-file.path-outside-workspace",
                    serde_json::json!({ "path": paths::display_path(&image_path) }),
                ));
            }
            image_path
        }
        MarkdownImageSource::Document {
            canonical_path,
            external_access_token,
            approved_external_targets,
        } => {
            let markdown_path = canonicalize_file(Path::new(canonical_path), "read")?;
            authorize_external_if_needed(
                runtime,
                &root.project_id,
                &root.path,
                &markdown_path,
                external_access_token.as_deref(),
                "read",
            )?;
            let directory = markdown_path.parent().ok_or_else(|| {
                paths::error(
                    "workspace-file.markdown-image-src-invalid",
                    serde_json::json!({ "src": input.raw_src }),
                )
            })?;
            let image_path = resolve_markdown_image_path(directory, &input.raw_src)?;
            let auto_allowed =
                path_is_within(&image_path, &root.path) || path_is_within(&image_path, directory);
            let explicitly_approved = approved_external_targets.iter().any(|candidate| {
                canonicalize_file(Path::new(candidate), "read")
                    .is_ok_and(|approved| paths::same_path(&approved, &image_path))
            });
            if !auto_allowed && !explicitly_approved {
                return Ok(MarkdownImagePreviewVm::ApprovalRequired {
                    canonical_path: paths::display_path(&image_path),
                    reason: "outside-document-directory".to_owned(),
                });
            }
            image_path
        }
    };
    let snapshot = service::read_file(root, runtime, &image_path, None, false)?;
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

fn resolve_markdown_image_path(base_directory: &Path, raw_src: &str) -> CommandResult<PathBuf> {
    let invalid = || {
        paths::error(
            "workspace-file.markdown-image-src-invalid",
            serde_json::json!({ "src": raw_src }),
        )
    };
    // rawSrc is a Markdown URL: remove URL suffixes before decoding, so %23
    // names a literal '#' while an unescaped '#' starts a fragment.
    let raw = raw_src.trim().split(['?', '#']).next().unwrap_or("");
    validate_markdown_image_reference(raw, raw_src, true)?;
    let decoded = if raw.to_ascii_lowercase().starts_with("file:") {
        let url = url::Url::parse(raw).map_err(|_| invalid())?;
        if url
            .host_str()
            .is_some_and(|host| !host.is_empty() && host != "localhost")
        {
            return Err(paths::error(
                "workspace-file.markdown-image-unc-blocked",
                serde_json::json!({ "src": raw_src }),
            ));
        }
        // to_file_path already percent-decodes: do not decode the result again.
        url.to_file_path().map_err(|_| invalid())?
    } else {
        let decoded = percent_decode_str(raw)
            .decode_utf8()
            .map_err(|_| invalid())?;
        PathBuf::from(paths::normalize_platform_file_link_path(&decoded))
    };
    // Recheck decoded prefixes before any filesystem access: encoded UNC and
    // schemes must not reach canonicalize (which can access a network share).
    validate_markdown_image_reference(&decoded.to_string_lossy(), raw_src, false)?;
    let candidate = if decoded.is_absolute() {
        decoded
    } else {
        base_directory.join(decoded)
    };
    canonicalize_file(&candidate, "read")
}

fn validate_markdown_image_reference(
    value: &str,
    raw_src: &str,
    allow_file: bool,
) -> CommandResult<()> {
    let lower = value.to_ascii_lowercase();
    let slash = value.replace('\\', "/");
    let code = if slash.starts_with("//") {
        Some("workspace-file.markdown-image-unc-blocked")
    } else if lower.starts_with("http:") || lower.starts_with("https:") {
        Some("workspace-file.markdown-image-network-blocked")
    } else if value.is_empty()
        || value.contains('\0')
        || (!paths::looks_like_windows_drive_absolute(paths::normalize_platform_file_link_path(
            value,
        )) && value
            .split(['/', '\\'])
            .next()
            .is_some_and(|prefix| prefix.contains(':'))
            && !(allow_file && lower.starts_with("file:")))
    {
        Some("workspace-file.markdown-image-src-invalid")
    } else {
        None
    };
    match code {
        Some(code) => Err(paths::error(code, serde_json::json!({ "src": raw_src }))),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn write_file_resource(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: WriteFileResourceInput,
) -> CommandResult<FileRevisionVm> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
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
pub async fn create_workspace_entry(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: CreateWorkspaceEntryInput,
) -> CommandResult<WorkspaceDirectoryEntryVm> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    let parent = resolve_workspace_directory(&root, &input.parent_relative_path)?;
    spawn_blocking_command(move || mutations::create_entry(&root, &parent, &input.name, input.kind))
        .await
}

#[tauri::command]
pub async fn rename_workspace_entry(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: RenameWorkspaceEntryInput,
) -> CommandResult<WorkspaceDirectoryEntryVm> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    let source = resolve_workspace_entry_path(&root, &input.relative_path)?;
    spawn_blocking_command(move || mutations::rename_entry(&root, &source, &input.new_name)).await
}

/// Move a workspace entry to the system trash. The returned receipt backs a
/// later `restore_workspace_entry` for undo within this app session.
#[tauri::command]
pub async fn delete_workspace_entry(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: DeleteWorkspaceEntryInput,
) -> CommandResult<WorkspaceEntryDeletionVm> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    let path = resolve_workspace_entry_path(&root, &input.relative_path)?;
    let project_id = root.project_id.clone();
    let (entry, trashed) =
        spawn_blocking_command(move || mutations::delete_entry(&root, &path)).await?;
    let receipt_id = runtime.record_trash_receipt(project_id, trashed)?;
    Ok(WorkspaceEntryDeletionVm { receipt_id, entry })
}

#[tauri::command]
pub async fn restore_workspace_entry(
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: RestoreWorkspaceEntryInput,
) -> CommandResult<WorkspaceDirectoryEntryVm> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    let receipt = runtime.trash_receipt(&root.project_id, &input.receipt_id)?;
    let entry =
        spawn_blocking_command(move || mutations::restore_entry(&root, &receipt.entry)).await?;
    runtime.forget_trash_receipt(&input.receipt_id)?;
    Ok(entry)
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
pub async fn start_workspace_file_watch(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: WorkspaceFileWatchInput,
) -> CommandResult<()> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    watch_runtime.start_workspace(
        app_handle,
        runtime.inner().clone(),
        root.project_id,
        root.path,
        root.workspace_scope_path,
        root.config.watch_debounce_ms,
    )
}

#[tauri::command]
pub fn stop_workspace_file_watch(
    state: State<'_, DesktopState>,
    watch_runtime: State<'_, WorkspaceFileWatchRuntime>,
    input: WorkspaceFileWatchInput,
) -> CommandResult<()> {
    let (project_id, root) = workspace_watch_root(
        state.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )?;
    watch_runtime.stop_workspace(&project_id, &root)
}

/// The main WebView's opener scope does not grant `open_path`; the file must pass the
/// same workspace / external-grant authorization as a read before the OS launches it.
#[tauri::command]
pub async fn open_file_with_system_app(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    runtime: State<'_, WorkspaceFileRuntime>,
    input: OpenFileWithSystemAppInput,
) -> CommandResult<()> {
    let root = resolve_workspace_root(
        state.inner(),
        runtime.inner(),
        &input.project_id,
        input.workspace_path.as_deref(),
    )
    .await?;
    let path = system_open_target(
        runtime.inner(),
        &root.project_id,
        &root.path,
        Path::new(&input.canonical_path),
        input.external_access_token.as_deref(),
    )?;
    app_handle
        .opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| {
            paths::error(
                "workspace-file.system-open-failed",
                serde_json::json!({ "path": paths::display_path(&path), "reason": error.to_string() }),
            )
        })
}

fn system_open_target(
    runtime: &WorkspaceFileRuntime,
    project_id: &str,
    root: &Path,
    raw_path: &Path,
    external_access_token: Option<&str>,
) -> CommandResult<PathBuf> {
    let path = canonicalize_file(raw_path, "open")?;
    authorize_external_if_needed(
        runtime,
        project_id,
        root,
        &path,
        external_access_token,
        "open",
    )?;
    Ok(path)
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
    fn markdown_image_workspace_source_requires_no_markdown_document() {
        let input = serde_json::from_value::<ResolveMarkdownImageInput>(serde_json::json!({
            "projectId": "project-a",
            "workspacePath": null,
            "source": { "kind": "workspace" },
            "rawSrc": "docs/overview.png"
        }));
        assert!(
            input.is_ok(),
            "workspace image input must not require a document: {input:?}"
        );
        let dir = tempdir().unwrap();
        let root = image_test_root(dir.path());
        let image_path = root.path.join("docs/overview.png");
        std::fs::create_dir(image_path.parent().unwrap()).unwrap();
        write_test_png(&image_path);
        let runtime = WorkspaceFileRuntime::default();
        let preview = resolve_markdown_image_from_root(&root, &runtime, &input.unwrap()).unwrap();
        let MarkdownImagePreviewVm::Ready {
            canonical_path,
            preview_grant,
            width,
            height,
            mime_type,
            ..
        } = preview
        else {
            panic!("expected ready PNG preview");
        };
        assert_eq!(
            canonical_path,
            paths::display_path(&std::fs::canonicalize(&image_path).unwrap())
        );
        assert_eq!((width, height, mime_type.as_str()), (3, 2, "image/png"));
        assert_eq!(
            preview_protocol_response(&runtime, &preview_grant.token).body(),
            &std::fs::read(image_path).unwrap()
        );
        runtime.release_preview(&preview_grant.token).unwrap();
        assert!(
            !preview_protocol_response(&runtime, &preview_grant.token)
                .status()
                .is_success()
        );
    }

    fn image_test_root(path: &Path) -> paths::ResolvedWorkspaceRoot {
        paths::ResolvedWorkspaceRoot {
            project_id: "project-a".to_owned(),
            path: std::fs::canonicalize(path).unwrap(),
            workspace_scope_path: None,
            config: gold_band::config::WorkspaceFilesConfig::default(),
        }
    }

    fn write_test_png(path: &Path) {
        image::RgbImage::from_pixel(3, 2, image::Rgb([20, 40, 60]))
            .save(path)
            .unwrap();
    }

    fn workspace_image_input(raw_src: &str) -> ResolveMarkdownImageInput {
        ResolveMarkdownImageInput {
            project_id: "project-a".to_owned(),
            workspace_path: None,
            source: MarkdownImageSource::Workspace,
            raw_src: raw_src.to_owned(),
        }
    }

    #[test]
    fn markdown_image_workspace_boundary_rejects_escape_without_external_authorization() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        // Invalid content proves authorization fails before the image reader.
        let outside = dir.path().join("outside.png");
        std::fs::write(&outside, b"not an image").unwrap();
        let root = image_test_root(&workspace);
        let runtime = WorkspaceFileRuntime::default();
        for raw in [
            "../outside.png".to_owned(),
            url::Url::from_file_path(&outside).unwrap().to_string(),
        ] {
            assert_eq!(
                resolve_markdown_image_from_root(&root, &runtime, &workspace_image_input(&raw))
                    .unwrap_err()
                    .code,
                "workspace-file.path-outside-workspace"
            );
        }
        assert!(
            runtime
                .validate_external_grant(None, "project-a", &outside, "read")
                .is_err()
        );
    }

    #[test]
    #[cfg_attr(
        windows,
        ignore = "requires Windows symlink privilege or Developer Mode"
    )]
    fn markdown_image_workspace_rejects_symlink_escape() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let outside = dir.path().join("outside.png");
        write_test_png(&outside);
        let link = workspace.join("linked.png");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &link).unwrap();
        let root = image_test_root(&workspace);
        assert_eq!(
            resolve_markdown_image_from_root(
                &root,
                &WorkspaceFileRuntime::default(),
                &workspace_image_input("linked.png")
            )
            .unwrap_err()
            .code,
            "workspace-file.path-outside-workspace"
        );
    }

    #[test]
    fn markdown_image_document_authorization_and_exact_approval_are_preserved() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let root = image_test_root(workspace.path());
        let directory = outside.path().join("docs");
        std::fs::create_dir(&directory).unwrap();
        let document = directory.join("README.md");
        std::fs::write(&document, "# Readme").unwrap();
        let document = std::fs::canonicalize(document).unwrap();
        write_test_png(&directory.join("inside.png"));
        write_test_png(&outside.path().join("outside.png"));
        write_test_png(&outside.path().join("other.png"));
        let target = std::fs::canonicalize(outside.path().join("outside.png")).unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let mut input = workspace_image_input("inside.png");
        input.source = MarkdownImageSource::Document {
            canonical_path: paths::display_path(&document),
            external_access_token: None,
            approved_external_targets: vec![],
        };
        assert_eq!(
            resolve_markdown_image_from_root(&root, &runtime, &input)
                .unwrap_err()
                .code,
            "workspace-file.external-access-denied"
        );
        let grant = runtime
            .issue_external_grant(root.project_id.clone(), document, 30)
            .unwrap();
        if let MarkdownImageSource::Document {
            external_access_token,
            ..
        } = &mut input.source
        {
            *external_access_token = Some(grant.token.clone());
        }
        assert!(matches!(
            resolve_markdown_image_from_root(&root, &runtime, &input).unwrap(),
            MarkdownImagePreviewVm::Ready { .. }
        ));
        input.raw_src = "../outside.png".to_owned();
        assert!(matches!(
            resolve_markdown_image_from_root(&root, &runtime, &input).unwrap(),
            MarkdownImagePreviewVm::ApprovalRequired { .. }
        ));
        if let MarkdownImageSource::Document {
            approved_external_targets,
            ..
        } = &mut input.source
        {
            approved_external_targets.push(paths::display_path(&target));
        }
        assert!(matches!(
            resolve_markdown_image_from_root(&root, &runtime, &input).unwrap(),
            MarkdownImagePreviewVm::Ready { .. }
        ));
        input.raw_src = "../other.png".to_owned();
        assert!(matches!(
            resolve_markdown_image_from_root(&root, &runtime, &input).unwrap(),
            MarkdownImagePreviewVm::ApprovalRequired { .. }
        ));
        runtime.release_external_grant(&grant.token).unwrap();
        assert_eq!(
            resolve_markdown_image_from_root(&root, &runtime, &input)
                .unwrap_err()
                .code,
            "workspace-file.external-access-denied"
        );
    }

    #[test]
    fn markdown_image_workspace_uses_existing_format_and_size_limits() {
        let dir = tempdir().unwrap();
        let mut root = image_test_root(dir.path());
        write_test_png(&root.path.join("image.png"));
        std::fs::write(root.path.join("fake.png"), "not an image").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        assert_eq!(
            resolve_markdown_image_from_root(&root, &runtime, &workspace_image_input("fake.png"))
                .unwrap(),
            MarkdownImagePreviewVm::Unsupported {
                limitation_code: "workspace-file.format-unsupported".to_owned()
            }
        );
        root.config.image_preview_max_bytes = 1;
        assert_eq!(
            resolve_markdown_image_from_root(&root, &runtime, &workspace_image_input("image.png"))
                .unwrap(),
            MarkdownImagePreviewVm::Unsupported {
                limitation_code: "workspace-file.too-large".to_owned()
            }
        );
        root.config.image_preview_max_bytes = 1024;
        root.config.image_preview_max_pixels = 5;
        assert_eq!(
            resolve_markdown_image_from_root(&root, &runtime, &workspace_image_input("image.png"))
                .unwrap(),
            MarkdownImagePreviewVm::Unsupported {
                limitation_code: "workspace-file.image-dimensions-too-large".to_owned()
            }
        );
    }

    #[test]
    fn markdown_image_url_suffix_is_removed_before_single_percent_decode() {
        let dir = tempdir().unwrap();
        let markdown = dir.path().join("README.md");
        let image = dir.path().join("中文 %20 # image.png");
        std::fs::write(&markdown, "# Readme").unwrap();
        std::fs::write(&image, b"image").unwrap();
        assert_eq!(
            resolve_markdown_image_path(
                markdown.parent().unwrap(),
                "%E4%B8%AD%E6%96%87%20%2520%20%23%20image.png?v=1#preview"
            )
            .unwrap(),
            std::fs::canonicalize(image).unwrap(),
        );
    }

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
    fn system_open_accepts_workspace_files_and_granted_external_files() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let root = std::fs::canonicalize(workspace.path()).unwrap();
        let inside = root.join("pelican.svg");
        let external = std::fs::canonicalize(outside.path())
            .unwrap()
            .join("report.pdf");
        std::fs::write(&inside, "<svg/>").unwrap();
        std::fs::write(&external, "pdf").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let grant = runtime
            .issue_external_grant("project-a".to_string(), external.clone(), 30)
            .unwrap();

        assert_eq!(
            system_open_target(&runtime, "project-a", &root, &inside, None).unwrap(),
            inside
        );
        assert_eq!(
            system_open_target(&runtime, "project-a", &root, &external, Some(&grant.token))
                .unwrap(),
            external
        );
    }

    #[test]
    fn system_open_rejects_ungranted_missing_and_directory_targets() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let root = std::fs::canonicalize(workspace.path()).unwrap();
        let external = std::fs::canonicalize(outside.path())
            .unwrap()
            .join("setup.exe");
        std::fs::write(&external, "MZ").unwrap();
        let other = root.join("other.txt");
        std::fs::write(&other, "other").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let other_grant = runtime
            .issue_external_grant("project-a".to_string(), other, 30)
            .unwrap();

        let code = |result: CommandResult<PathBuf>| result.unwrap_err().code;
        assert_eq!(
            code(system_open_target(
                &runtime,
                "project-a",
                &root,
                &external,
                None
            )),
            "workspace-file.external-access-denied"
        );
        assert_eq!(
            code(system_open_target(
                &runtime,
                "project-a",
                &root,
                &external,
                Some(&other_grant.token)
            )),
            "workspace-file.external-access-denied"
        );
        assert_eq!(
            code(system_open_target(
                &runtime,
                "project-a",
                &root,
                &root.join("missing.svg"),
                None
            )),
            "workspace-file.not-found"
        );
        assert_eq!(
            code(system_open_target(
                &runtime,
                "project-a",
                &root,
                &root,
                None
            )),
            "workspace-file.not-a-file"
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
            resolve_markdown_image_path(markdown.parent().unwrap(), "assets/screen%20shot.png")
                .unwrap(),
            std::fs::canonicalize(image).unwrap(),
        );
        let file_url = url::Url::from_file_path(image_dir.join("screen shot.png")).unwrap();
        assert_eq!(
            resolve_markdown_image_path(markdown.parent().unwrap(), file_url.as_str()).unwrap(),
            std::fs::canonicalize(image_dir.join("screen shot.png")).unwrap(),
        );
    }

    #[test]
    fn markdown_image_encoded_network_and_schemes_are_rejected_before_io() {
        let dir = tempdir().unwrap();
        for (raw, code) in [
            (
                "%2F%2Fserver/share/image.png",
                "workspace-file.markdown-image-unc-blocked",
            ),
            (
                "%5C%5Cserver/share/image.png",
                "workspace-file.markdown-image-unc-blocked",
            ),
            (
                "/\\server/share/image.png",
                "workspace-file.markdown-image-unc-blocked",
            ),
            (
                "file://server/share/image.png",
                "workspace-file.markdown-image-unc-blocked",
            ),
            (
                "https%3A%2F%2Fexample.com/a.png",
                "workspace-file.markdown-image-network-blocked",
            ),
            (
                "javascript%3Aalert(1)",
                "workspace-file.markdown-image-src-invalid",
            ),
            (
                "ftp://example.com/a.png",
                "workspace-file.markdown-image-src-invalid",
            ),
            ("blob:local", "workspace-file.markdown-image-src-invalid"),
        ] {
            assert_eq!(
                resolve_markdown_image_path(dir.path(), raw)
                    .unwrap_err()
                    .code,
                code,
                "{raw}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn markdown_image_windows_url_pathname_and_file_url_share_one_decode() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("中文 %20 # image.png");
        write_test_png(&file);
        let url = url::Url::from_file_path(&file).unwrap();
        let expected = std::fs::canonicalize(&file).unwrap();
        for raw in [
            format!("{}?v=1#image", url.as_str()),
            format!("{}?v=1#image", url.path()),
        ] {
            assert_eq!(
                resolve_markdown_image_path(dir.path(), &raw).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn markdown_image_paths_reject_network_and_dangerous_sources() {
        let dir = tempdir().unwrap();
        let markdown = dir.path().join("README.md");
        std::fs::write(&markdown, "# Readme").unwrap();

        assert_eq!(
            resolve_markdown_image_path(
                markdown.parent().unwrap(),
                "https://tracker.example/image.png"
            )
            .unwrap_err()
            .code,
            "workspace-file.markdown-image-network-blocked",
        );
        assert_eq!(
            resolve_markdown_image_path(markdown.parent().unwrap(), "data:image/png;base64,AAAA")
                .unwrap_err()
                .code,
            "workspace-file.markdown-image-src-invalid",
        );
        assert_eq!(
            resolve_markdown_image_path(markdown.parent().unwrap(), r"\\server\share\image.png")
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
                workspace_path: None,
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
