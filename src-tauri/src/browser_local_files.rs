//! Local documents in the built-in browser.
//!
//! Address space and authorization are separate concerns. Every protocol URL maps
//! one-to-one onto an absolute filesystem path, so relative references such as
//! `../node_modules/x.js` resolve to the file the page actually means. Whether that
//! file may be served is decided afterwards by the page's [`LocalFileGrant`].
//!
//! All local files share one origin under this protocol, so anything inside a grant
//! is readable by page script (`fetch`). The default grant therefore stays at the
//! workspace that contains the document; wider directories need the user's consent.

use std::path::{Path, PathBuf};

use tauri::http::{Method, Response, StatusCode, header};
use url::Url;

pub const BROWSER_LOCAL_FILE_PROTOCOL: &str = "gold-band-browser-file";
const BROWSER_LOCAL_FILE_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Directories a page may ask for before the user answers. Further requests are
/// still refused; they just stop growing the pending list.
const MAX_DENIED_DIRECTORIES: usize = 16;
const MANAGED_WORKTREES_DIR: &str = "worktrees";

/// Directories one browser page may read through the local file protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LocalFileGrant {
    roots: Vec<PathBuf>,
    denied: Vec<PathBuf>,
}

/// Outcome of mapping one protocol request onto the page's grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalFileAccess {
    Granted(PathBuf),
    /// The file exists outside every granted root; `directory` is what the user may allow.
    Denied {
        directory: PathBuf,
    },
    Unavailable,
}

impl LocalFileGrant {
    /// Grant for a freshly opened document: the innermost workspace containing it,
    /// otherwise its own directory.
    pub(crate) fn for_document(document: &Path, workspace_roots: &[PathBuf]) -> Option<Self> {
        let parent = document.parent()?;
        let root = workspace_roots
            .iter()
            .filter(|root| path_is_within(document, root))
            .max_by_key(|root| root.components().count())
            .cloned()
            .unwrap_or_else(|| parent.to_path_buf());
        Some(Self {
            roots: vec![root],
            denied: Vec::new(),
        })
    }

    pub(crate) fn covers(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path_is_within(path, root))
    }

    pub(crate) fn access(&self, url: &Url) -> LocalFileAccess {
        let Ok(path) = local_path_for_url(url) else {
            return LocalFileAccess::Unavailable;
        };
        let Ok(canonical) = dunce_canonicalize(&path) else {
            return LocalFileAccess::Unavailable;
        };
        if self.covers(&canonical) {
            return LocalFileAccess::Granted(canonical);
        }
        match canonical.parent() {
            Some(directory) if canonical.is_file() => LocalFileAccess::Denied {
                directory: directory.to_path_buf(),
            },
            _ => LocalFileAccess::Unavailable,
        }
    }

    /// Remembers a refused directory for the user to allow. Returns `true` only
    /// when the pending list changed, so callers notify once per directory.
    pub(crate) fn record_denied(&mut self, directory: PathBuf) -> bool {
        if self.covers(&directory)
            || self
                .denied
                .iter()
                .any(|pending| path_is_within(&directory, pending))
        {
            return false;
        }
        let before = self.denied.len();
        self.denied
            .retain(|pending| !path_is_within(pending, &directory));
        if self.denied.len() == before && self.denied.len() >= MAX_DENIED_DIRECTORIES {
            return false;
        }
        self.denied.push(directory);
        true
    }

    pub(crate) fn denied_directories(&self) -> &[PathBuf] {
        &self.denied
    }

    /// Moves the requested directories from pending to granted. Only directories
    /// the page itself asked for are accepted; anything else is ignored.
    pub(crate) fn allow(&mut self, directories: &[PathBuf]) -> usize {
        let (allowed, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.denied)
            .into_iter()
            .partition(|pending| directories.iter().any(|requested| requested == pending));
        self.denied = pending;
        let count = allowed.len();
        self.roots.extend(allowed);
        count
    }
}

pub(crate) fn is_browser_local_file_url(url: &Url) -> bool {
    url.scheme() == BROWSER_LOCAL_FILE_PROTOCOL
        || (matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case(&format!("{BROWSER_LOCAL_FILE_PROTOCOL}.localhost"))
            }))
}

/// Protocol URL for an absolute local path. The URL path is the `file://` path, so
/// relative references resolve exactly as they would against the real file.
pub(crate) fn local_document_navigation_url(path: &Path) -> Result<Url, ()> {
    let file_url = Url::from_file_path(path)?;
    // A host means a UNC share; serving it would reach the network.
    if file_url.host_str().is_some_and(|host| !host.is_empty()) {
        return Err(());
    }
    // WebView2 cannot navigate directly to a non-standard scheme; wry only rewrites
    // the initial URL. Use the documented workaround form on Windows and the
    // registered scheme elsewhere.
    let base = if cfg!(windows) {
        format!("http://{BROWSER_LOCAL_FILE_PROTOCOL}.localhost/")
    } else {
        format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/")
    };
    let mut url = Url::parse(&base).map_err(|_| ())?;
    url.set_path(file_url.path());
    Ok(url)
}

/// Absolute path a protocol URL addresses. Not canonical; callers authorize the
/// canonical form. Dot segments and encoded separators are rejected outright.
pub(crate) fn local_path_for_url(url: &Url) -> Result<PathBuf, ()> {
    if !is_browser_local_file_url(url) {
        return Err(());
    }
    for segment in url.path_segments().ok_or(())? {
        let decoded = percent_encoding::percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| ())?;
        if decoded.is_empty() || decoded == "." || decoded == ".." || decoded.contains(['/', '\\'])
        {
            return Err(());
        }
    }
    let mut file_url = Url::parse("file:///").map_err(|_| ())?;
    file_url.set_path(url.path());
    file_url.to_file_path()
}

/// `file://` form shown in the address bar for a protocol URL; other URLs pass through.
pub(crate) fn browser_display_url(url: &Url) -> Url {
    local_path_for_url(url)
        .ok()
        .and_then(|path| Url::from_file_path(path).ok())
        .unwrap_or_else(|| url.clone())
}

/// Serves one protocol request. Files outside the grant answer exactly like missing
/// files so a page cannot probe which paths exist; the access result tells the
/// caller what to record.
pub(crate) fn local_file_response(
    grant: &LocalFileGrant,
    method: &Method,
    uri: &str,
) -> (Response<Vec<u8>>, LocalFileAccess) {
    if method != Method::GET && method != Method::HEAD {
        return (
            local_file_error(StatusCode::METHOD_NOT_ALLOWED),
            LocalFileAccess::Unavailable,
        );
    }
    let Ok(url) = Url::parse(uri) else {
        return (
            local_file_error(StatusCode::BAD_REQUEST),
            LocalFileAccess::Unavailable,
        );
    };
    let access = grant.access(&url);
    let LocalFileAccess::Granted(path) = &access else {
        return (local_file_error(StatusCode::NOT_FOUND), access);
    };
    (serve_file(path, method), access)
}

fn serve_file(path: &Path, method: &Method) -> Response<Vec<u8>> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return local_file_error(StatusCode::NOT_FOUND);
    };
    if !metadata.is_file() || metadata.len() > BROWSER_LOCAL_FILE_MAX_BYTES {
        return local_file_error(StatusCode::NOT_FOUND);
    }
    let body = if method == Method::HEAD {
        Vec::new()
    } else {
        match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return local_file_error(StatusCode::NOT_FOUND),
        }
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .header(header::CONTENT_LENGTH, metadata.len())
        .body(body)
        .unwrap_or_else(|_| local_file_error(StatusCode::INTERNAL_SERVER_ERROR))
}

pub(crate) fn local_file_error(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .body(Vec::new())
        .expect("static browser protocol response")
}

/// Workspace roots that contain `document`: registered workspaces and the
/// Gold Band-managed worktree the document lives in. No directory is listed.
pub(crate) fn workspace_roots_containing(
    document: &Path,
    workspace_paths: &[String],
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for workspace_path in workspace_paths {
        if let Ok(root) = dunce_canonicalize(Path::new(workspace_path))
            && path_is_within(document, &root)
        {
            roots.push(root);
        }
        let worktrees = gold_band::storage::GoldBandPaths::new(workspace_path.as_str())
            .runtime_root
            .join(MANAGED_WORKTREES_DIR);
        let Ok(worktrees) = dunce_canonicalize(worktrees.as_std_path()) else {
            continue;
        };
        if let Some(name) = document
            .strip_prefix(&worktrees)
            .ok()
            .and_then(|relative| relative.components().next())
        {
            roots.push(worktrees.join(name));
        }
    }
    roots
}

pub(crate) fn path_is_within(path: &Path, root: &Path) -> bool {
    let Ok(root) = canonicalize_display_path(root) else {
        return false;
    };
    path == root || path.starts_with(&root)
}

pub(crate) fn canonicalize_display_path(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = if path.exists() {
        dunce_canonicalize(path)?
    } else {
        strip_verbatim_prefix(path.to_path_buf())
    };
    Ok(canonical)
}

fn dunce_canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    Ok(strip_verbatim_prefix(std::fs::canonicalize(path)?))
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_directories_collapse_into_their_ancestor_and_stay_bounded() {
        let base = tempfile::tempdir().unwrap();
        let base = canonicalize_display_path(base.path()).unwrap();
        let mut grant = LocalFileGrant {
            roots: vec![base.join("site")],
            denied: Vec::new(),
        };

        assert!(
            !grant.record_denied(base.join("site/assets")),
            "already granted"
        );
        assert!(grant.record_denied(base.join("shared/css")));
        assert!(grant.record_denied(base.join("shared")));
        assert_eq!(grant.denied_directories(), [base.join("shared")]);
        assert!(
            !grant.record_denied(base.join("shared/js")),
            "covered by pending"
        );

        for index in 1..MAX_DENIED_DIRECTORIES {
            assert!(grant.record_denied(base.join(format!("extra-{index}"))));
        }
        assert!(!grant.record_denied(base.join("one-too-many")));
        assert_eq!(grant.denied_directories().len(), MAX_DENIED_DIRECTORIES);
    }
}
