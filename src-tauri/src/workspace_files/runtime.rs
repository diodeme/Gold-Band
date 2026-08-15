use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::http::{Response, StatusCode, header};
use uuid::Uuid;

use crate::commands::{CommandErrorVm, CommandResult};

use super::models::{ExternalFileAccessGrantVm, FileRevisionVm, WorkspaceFilePreviewGrantVm};
use super::paths::{display_path, error};

#[derive(Clone, Default)]
pub struct WorkspaceFileRuntime {
    inner: Arc<Mutex<WorkspaceFileRuntimeInner>>,
}

#[derive(Default)]
struct WorkspaceFileRuntimeInner {
    external_grants: HashMap<String, ExternalGrant>,
    preview_grants: HashMap<String, PreviewGrant>,
    recent_writes: HashMap<PathBuf, RecentWrite>,
}

#[derive(Clone)]
struct ExternalGrant {
    project_id: String,
    path: PathBuf,
    expires_at: SystemTime,
    ttl: Duration,
}

#[derive(Clone)]
struct PreviewGrant {
    project_id: String,
    path: PathBuf,
    revision: FileRevisionVm,
    mime_type: String,
    svg: bool,
    expires_at: SystemTime,
}

#[derive(Clone)]
struct RecentWrite {
    operation_id: String,
    revision: FileRevisionVm,
    expires_at: SystemTime,
}

impl WorkspaceFileRuntime {
    pub(crate) fn issue_attachment_preview(
        &self,
        project_id: String,
        path: PathBuf,
        revision: FileRevisionVm,
        mime_type: String,
        ttl_seconds: u64,
    ) -> CommandResult<WorkspaceFilePreviewGrantVm> {
        self.issue_preview(project_id, path, revision, mime_type, false, ttl_seconds)
    }

    pub(crate) fn issue_external_grant(
        &self,
        project_id: String,
        path: PathBuf,
        ttl_seconds: u64,
    ) -> CommandResult<ExternalFileAccessGrantVm> {
        let ttl = Duration::from_secs(ttl_seconds.max(1));
        let expires_at = SystemTime::now() + ttl;
        let token = Uuid::new_v4().to_string();
        let mut inner = self.lock()?;
        inner.cleanup_expired();
        inner.external_grants.insert(
            token.clone(),
            ExternalGrant {
                project_id,
                path,
                expires_at,
                ttl,
            },
        );
        Ok(external_grant_vm(token, expires_at))
    }

    pub(crate) fn validate_external_grant(
        &self,
        token: Option<&str>,
        project_id: &str,
        path: &Path,
        operation: &str,
    ) -> CommandResult<ExternalFileAccessGrantVm> {
        let token = token.ok_or_else(|| external_access_error(path, operation, "missing"))?;
        let mut inner = self.lock()?;
        inner.cleanup_expired();
        let grant = inner
            .external_grants
            .get(token)
            .filter(|grant| grant.project_id == project_id && grant.path == path)
            .cloned()
            .ok_or_else(|| external_access_error(path, operation, "invalid-or-expired"))?;
        Ok(external_grant_vm(token.to_string(), grant.expires_at))
    }

    pub(crate) fn renew_external_grant(
        &self,
        token: &str,
    ) -> CommandResult<ExternalFileAccessGrantVm> {
        let mut inner = self.lock()?;
        inner.cleanup_expired();
        let mut grant = inner.external_grants.remove(token).ok_or_else(|| {
            error(
                "workspace-file.external-access-denied",
                serde_json::json!({ "operation": "renew", "reason": "invalid-or-expired" }),
            )
        })?;
        grant.expires_at = SystemTime::now() + grant.ttl;
        let next_token = Uuid::new_v4().to_string();
        let expires_at = grant.expires_at;
        inner.external_grants.insert(next_token.clone(), grant);
        Ok(external_grant_vm(next_token, expires_at))
    }

    pub(crate) fn release_external_grant(&self, token: &str) -> CommandResult<()> {
        self.lock()?.external_grants.remove(token);
        Ok(())
    }

    pub(crate) fn issue_preview(
        &self,
        project_id: String,
        path: PathBuf,
        revision: FileRevisionVm,
        mime_type: String,
        svg: bool,
        ttl_seconds: u64,
    ) -> CommandResult<WorkspaceFilePreviewGrantVm> {
        let token = Uuid::new_v4().to_string();
        let expires_at = SystemTime::now() + Duration::from_secs(ttl_seconds.max(1));
        let mut inner = self.lock()?;
        inner.cleanup_expired();
        inner.preview_grants.insert(
            token.clone(),
            PreviewGrant {
                project_id,
                path,
                revision,
                mime_type,
                svg,
                expires_at,
            },
        );
        Ok(WorkspaceFilePreviewGrantVm {
            token,
            expires_at_ms: system_time_ms(expires_at),
        })
    }

    pub(crate) fn release_preview(&self, token: &str) -> CommandResult<()> {
        self.lock()?.preview_grants.remove(token);
        Ok(())
    }

    pub(crate) fn record_write(
        &self,
        path: PathBuf,
        operation_id: String,
        revision: FileRevisionVm,
    ) -> CommandResult<()> {
        let mut inner = self.lock()?;
        inner.cleanup_expired();
        inner.recent_writes.insert(
            path,
            RecentWrite {
                operation_id,
                revision,
                expires_at: SystemTime::now() + Duration::from_secs(10),
            },
        );
        Ok(())
    }

    pub(crate) fn recent_write_for(&self, path: &Path) -> Option<(String, FileRevisionVm)> {
        let mut inner = self.inner.lock().ok()?;
        inner.cleanup_expired();
        inner
            .recent_writes
            .get(path)
            .map(|write| (write.operation_id.clone(), write.revision.clone()))
    }

    pub fn preview_protocol_response(&self, token: &str, static_frame: bool) -> Response<Vec<u8>> {
        match self.read_preview(token, static_frame) {
            Ok((bytes, mime_type)) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type)
                .header(header::CACHE_CONTROL, "no-store")
                .header("X-Content-Type-Options", "nosniff")
                .header(
                    "Content-Security-Policy",
                    "default-src 'none'; img-src 'self'; sandbox",
                )
                .body(bytes)
                .unwrap_or_else(|_| empty_response(StatusCode::INTERNAL_SERVER_ERROR)),
            Err(status) => empty_response(status),
        }
    }

    fn read_preview(
        &self,
        token: &str,
        static_frame: bool,
    ) -> Result<(Vec<u8>, String), StatusCode> {
        let grant = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            inner.cleanup_expired();
            inner
                .preview_grants
                .get(token)
                .cloned()
                .ok_or(StatusCode::GONE)?
        };
        if grant.project_id.is_empty() {
            return Err(StatusCode::FORBIDDEN);
        }
        let revision =
            super::service::revision_for_path(&grant.path).map_err(|_| StatusCode::NOT_FOUND)?;
        if revision != grant.revision {
            return Err(StatusCode::CONFLICT);
        }
        let bytes = std::fs::read(&grant.path).map_err(|_| StatusCode::NOT_FOUND)?;
        if grant.svg {
            let png = rasterize_svg(&bytes).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            return Ok((png, "image/png".to_string()));
        }
        if static_frame && grant.mime_type == "image/gif" {
            let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Gif)
                .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            let mut png = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            return Ok((png.into_inner(), "image/png".to_string()));
        }
        Ok((bytes, grant.mime_type))
    }

    fn lock(&self) -> CommandResult<std::sync::MutexGuard<'_, WorkspaceFileRuntimeInner>> {
        self.inner.lock().map_err(|_| {
            CommandErrorVm::new("workspace-file.runtime-unavailable", serde_json::json!({}))
        })
    }
}

impl WorkspaceFileRuntimeInner {
    fn cleanup_expired(&mut self) {
        let now = SystemTime::now();
        self.external_grants
            .retain(|_, grant| grant.expires_at > now);
        self.preview_grants
            .retain(|_, grant| grant.expires_at > now);
        self.recent_writes.retain(|_, write| write.expires_at > now);
    }
}

fn rasterize_svg(bytes: &[u8]) -> Result<Vec<u8>, ()> {
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver = resvg::usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|_| ())?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or(())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|_| ())
}

fn external_grant_vm(token: String, expires_at: SystemTime) -> ExternalFileAccessGrantVm {
    ExternalFileAccessGrantVm {
        token,
        permissions: vec!["read".to_string(), "write".to_string()],
        expires_at_ms: system_time_ms(expires_at),
    }
}

fn system_time_ms(value: SystemTime) -> String {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn external_access_error(path: &Path, operation: &str, reason: &str) -> CommandErrorVm {
    error(
        "workspace-file.external-access-denied",
        serde_json::json!({
            "path": display_path(path),
            "operation": operation,
            "reason": reason,
        }),
    )
}

fn empty_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(Vec::new())
        .expect("empty preview response is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renewing_external_access_rotates_the_token() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("outside.txt");
        std::fs::write(&path, "outside").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let first = runtime
            .issue_external_grant("project-1".to_string(), path.clone(), 30)
            .unwrap();
        let second = runtime.renew_external_grant(&first.token).unwrap();

        assert_ne!(first.token, second.token);
        assert!(
            runtime
                .validate_external_grant(Some(&first.token), "project-1", &path, "read")
                .is_err()
        );
        assert!(
            runtime
                .validate_external_grant(Some(&second.token), "project-1", &path, "read")
                .is_ok()
        );
    }

    #[test]
    fn expired_external_and_preview_tokens_are_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.png");
        let image = image::DynamicImage::new_rgba8(1, 1);
        image
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let external = runtime
            .issue_external_grant("project-1".to_string(), path.clone(), 30)
            .unwrap();
        let revision = super::super::service::revision_for_path(&path).unwrap();
        let preview = runtime
            .issue_preview(
                "project-1".to_string(),
                path.clone(),
                revision,
                "image/png".to_string(),
                false,
                30,
            )
            .unwrap();
        {
            let mut inner = runtime.inner.lock().unwrap();
            inner
                .external_grants
                .get_mut(&external.token)
                .unwrap()
                .expires_at = UNIX_EPOCH;
            inner
                .preview_grants
                .get_mut(&preview.token)
                .unwrap()
                .expires_at = UNIX_EPOCH;
        }

        assert!(
            runtime
                .validate_external_grant(Some(&external.token), "project-1", &path, "read")
                .is_err()
        );
        assert_eq!(
            runtime
                .preview_protocol_response(&preview.token, false)
                .status(),
            StatusCode::GONE
        );
    }

    #[test]
    fn preview_token_is_invalidated_when_file_revision_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.png");
        image::DynamicImage::new_rgba8(1, 1)
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let revision = super::super::service::revision_for_path(&path).unwrap();
        let preview = runtime
            .issue_preview(
                "project-1".to_string(),
                path.clone(),
                revision,
                "image/png".to_string(),
                false,
                30,
            )
            .unwrap();
        std::fs::write(&path, b"changed").unwrap();

        assert_eq!(
            runtime
                .preview_protocol_response(&preview.token, false)
                .status(),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn attachment_picker_preview_uses_the_same_revision_bound_protocol() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("selected.png");
        image::DynamicImage::new_rgba8(2, 2)
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let revision = super::super::service::revision_for_path(&path).unwrap();
        let preview = runtime
            .issue_attachment_preview(
                "attachment-picker".to_string(),
                path,
                revision,
                "image/png".to_string(),
                30,
            )
            .unwrap();

        let response = runtime.preview_protocol_response(&preview.token, false);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        assert!(response.body().starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn static_gif_preview_is_returned_as_png() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("animated.gif");
        image::DynamicImage::new_rgba8(2, 2)
            .save_with_format(&path, image::ImageFormat::Gif)
            .unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let revision = super::super::service::revision_for_path(&path).unwrap();
        let preview = runtime
            .issue_preview(
                "project-1".to_string(),
                path,
                revision,
                "image/gif".to_string(),
                false,
                30,
            )
            .unwrap();

        let response = runtime.preview_protocol_response(&preview.token, true);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        assert!(response.body().starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn svg_preview_is_rasterized_and_never_returns_source_dom() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("safe.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="red"/></svg>"#,
        )
        .unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let revision = super::super::service::revision_for_path(&path).unwrap();
        let preview = runtime
            .issue_preview(
                "project-1".to_string(),
                path,
                revision,
                "image/svg+xml".to_string(),
                true,
                30,
            )
            .unwrap();

        let response = runtime.preview_protocol_response(&preview.token, false);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        assert!(response.body().starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(!response.body().windows(4).any(|window| window == b"<svg"));
    }
}
