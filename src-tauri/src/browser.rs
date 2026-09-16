// Windows WebView2 deadlocks if child webviews are created from a synchronous IPC
// command. The handlers below stay async on purpose even when they do not await,
// so Tauri runs them off the WebView2 callback thread.
#![allow(clippy::unused_async)]

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, Window};
use tauri_plugin_dialog::DialogExt;
use tracing::{info, warn};
use url::Url;

use crate::commands::{CommandErrorVm, CommandResult};

pub const BROWSER_PAGE_EVENT: &str = "gold-band://browser-page";
pub const BROWSER_LIVE_WEBVIEW_LIMIT: usize = 5;
const MAIN_WEBVIEW_LABEL: &str = "main";
const BROWSER_PROFILE_DIR_NAME: &str = "browser-profile";
const BROWSER_WEBVIEW_LABEL_PREFIX: &str = "gb-b-";
const BROWSER_LINK_CLICK_SCRIPT: &str =
    include_str!("../../web/src/components/workspace/browser/browser-link-click.js");
#[cfg(target_os = "macos")]
const BROWSER_DATA_STORE_IDENTIFIER: [u8; 16] = *b"GoldBandBrowse01";

#[derive(Debug, Default)]
pub struct BrowserHost {
    inner: Mutex<BrowserHostInner>,
}

#[derive(Debug, Default)]
struct BrowserHostInner {
    pages: HashMap<String, BrowserNativePage>,
    lru: VecDeque<String>,
    engine_desktop_user_agent: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserViewMode {
    #[default]
    Desktop,
    Mobile,
}

const MOBILE_BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36";
const FALLBACK_DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

fn browser_user_agent(view_mode: BrowserViewMode) -> Option<&'static str> {
    match view_mode {
        BrowserViewMode::Desktop => None,
        BrowserViewMode::Mobile => Some(MOBILE_BROWSER_USER_AGENT),
    }
}

fn view_mode_user_agent<'a>(
    view_mode: BrowserViewMode,
    engine_desktop_user_agent: Option<&'a str>,
) -> &'a str {
    match view_mode {
        BrowserViewMode::Mobile => MOBILE_BROWSER_USER_AGENT,
        BrowserViewMode::Desktop => engine_desktop_user_agent.unwrap_or(FALLBACK_DESKTOP_USER_AGENT),
    }
}

#[derive(Debug, Clone)]
struct BrowserNativePage {
    label: String,
    allowed_file_root: Option<PathBuf>,
    view_mode: BrowserViewMode,
    last_http_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCreatePageInput {
    pub page_id: String,
    pub url: String,
    pub bounds: BrowserBoundsVm,
    #[serde(default)]
    pub view_mode: BrowserViewMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPageIdInput {
    pub page_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNavigateInput {
    pub page_id: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSetViewModeInput {
    pub page_id: String,
    pub view_mode: BrowserViewMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBoundsInput {
    pub page_id: String,
    pub bounds: BrowserBoundsVm,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBoundsVm {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPageVm {
    pub page_id: String,
    pub url: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPageEventVm {
    pub kind: String,
    pub page_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

pub fn discard_all_browser_webviews(app: &AppHandle) {
    let host = app.state::<BrowserHost>();
    let candidates = {
        let Ok(inner) = host.inner.lock() else {
            warn!(target: "gold_band::browser", operation = "app-exit-discard", "browser registry lock poisoned");
            return;
        };
        inner.entries()
    };
    if let Err(error) = close_registered_pages(app, host.inner(), candidates) {
        warn!(
            target: "gold_band::browser",
            operation = "app-exit-discard",
            error = ?error,
            "failed to discard browser webviews during app exit"
        );
    }
}

/// Windows WebView2 deadlocks if a child webview is created from a synchronous
/// IPC command: wry waits inside a nested message pump while the main WebView
/// is still inside a `WebResourceRequested` callback. These commands must stay
/// async so Tauri runs them off that callback thread.
#[tauri::command]
pub async fn browser_create_page(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserCreatePageInput,
) -> CommandResult<BrowserPageVm> {
    let page_id = validate_page_id(&input.page_id)?;
    let resolved = resolve_browser_target(&input.url, None)?;
    info!(
        target: "gold_band::browser",
        operation = "create",
        page_id,
        target_url = %browser_log_target(&resolved.url),
        x = input.bounds.x,
        y = input.bounds.y,
        width = input.bounds.width,
        height = input.bounds.height,
        "browser webview command started"
    );
    create_or_reuse_page(&app, &host, page_id, resolved, input.bounds, input.view_mode)
}

#[tauri::command]
pub async fn browser_set_bounds(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserBoundsInput,
) -> CommandResult<()> {
    let page_id = validate_page_id(&input.page_id)?;
    let label = host_label(&host, page_id)?;
    apply_bounds(&app, &label, input.bounds)
}

#[tauri::command]
pub async fn browser_show_page(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    let page_id = validate_page_id(&input.page_id)?;
    let label = {
        let mut inner = lock_host(&host)?;
        inner.touch(page_id);
        inner.label_for(page_id)?.to_string()
    };
    hide_other_pages(&app, &host, page_id)?;
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| webview_unavailable("show", Some(page_id), Some(&label)))?;
    webview.show().map_err(|error| {
        log_webview_failure("show", Some(page_id), &label, &error);
        webview_operation_error("show", error)
    })?;
    info!(target: "gold_band::browser", operation = "show", page_id, label, "browser webview command completed");
    Ok(())
}

#[tauri::command]
pub async fn browser_hide_page(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    let page_id = validate_page_id(&input.page_id)?;
    let label = host_label(&host, page_id)?;
    hide_label(&app, Some(page_id), &label)?;
    Ok(())
}

#[tauri::command]
pub async fn browser_hide_all(app: AppHandle, host: State<'_, BrowserHost>) -> CommandResult<()> {
    let entries = lock_host(&host)?.entries();
    info!(target: "gold_band::browser", operation = "hide-all", count = entries.len(), "browser webview command started");
    for (page_id, label) in entries {
        hide_label(&app, Some(&page_id), &label)?;
    }
    info!(target: "gold_band::browser", operation = "hide-all", "browser webview command completed");
    Ok(())
}

#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserNavigateInput,
) -> CommandResult<BrowserPageVm> {
    let page_id = validate_page_id(&input.page_id)?;
    let current_root = lock_host(&host)?
        .pages
        .get(page_id)
        .and_then(|page| page.allowed_file_root.clone());
    let resolved = resolve_browser_target(&input.url, current_root.as_deref())?;
    let label = host_label(&host, page_id)?;
    info!(
        target: "gold_band::browser",
        operation = "navigate",
        page_id,
        label,
        target_url = %browser_log_target(&resolved.url),
        "browser webview command started"
    );
    {
        let mut inner = lock_host(&host)?;
        if let Some(page) = inner.pages.get_mut(page_id) {
            page.allowed_file_root = resolved.allowed_file_root.clone();
        }
        inner.touch(page_id);
    }
    navigate_label(&app, &label, &resolved.url)?;
    info!(target: "gold_band::browser", operation = "navigate", page_id, label, "browser webview command completed");
    Ok(BrowserPageVm {
        page_id: page_id.to_string(),
        url: resolved.url.to_string(),
        label,
    })
}

#[tauri::command]
pub async fn browser_go_back(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    eval_history(&app, &host, &input.page_id, "history.back()")
}

#[tauri::command]
pub async fn browser_go_forward(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    eval_history(&app, &host, &input.page_id, "history.forward()")
}

#[tauri::command]
pub async fn browser_reload(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    eval_history(&app, &host, &input.page_id, "location.reload()")
}

#[tauri::command]
pub async fn browser_stop(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    eval_history(&app, &host, &input.page_id, "window.stop()")
}

#[tauri::command]
pub async fn browser_set_view_mode(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserSetViewModeInput,
) -> CommandResult<()> {
    let page_id = validate_page_id(&input.page_id)?;
    let label = host_label(&host, page_id)?;
    apply_view_mode(&app, &host, page_id, &label, input.view_mode)?;
    reload_label(&app, page_id, &label)?;
    info!(
        target: "gold_band::browser",
        operation = "set-view-mode",
        page_id,
        label,
        view_mode = ?input.view_mode,
        "browser webview command completed"
    );
    Ok(())
}

#[tauri::command]
pub async fn browser_close_page(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserPageIdInput,
) -> CommandResult<()> {
    let page_id = validate_page_id(&input.page_id)?;
    let candidate = lock_host(&host)?.entry(page_id);
    if let Some(candidate) = candidate {
        close_registered_pages(&app, host.inner(), vec![candidate])?;
    }
    Ok(())
}

#[tauri::command]
pub async fn browser_discard_all(app: AppHandle, host: State<'_, BrowserHost>) -> CommandResult<()> {
    let candidates = lock_host(&host)?.entries();
    info!(target: "gold_band::browser", operation = "discard-all", count = candidates.len(), "browser webview command started");
    close_registered_pages(&app, host.inner(), candidates)?;
    let remaining = lock_host(&host)?.pages.len();
    info!(target: "gold_band::browser", operation = "discard-all", remaining, "browser webview command completed");
    Ok(())
}

fn create_or_reuse_page(
    app: &AppHandle,
    host: &BrowserHost,
    page_id: &str,
    resolved: ResolvedBrowserTarget,
    bounds: BrowserBoundsVm,
    view_mode: BrowserViewMode,
) -> CommandResult<BrowserPageVm> {
    if let Ok(existing) = host_label(host, page_id) {
        {
            let mut inner = lock_host(host)?;
            if let Some(page) = inner.pages.get_mut(page_id) {
                page.allowed_file_root = resolved.allowed_file_root.clone();
            }
            inner.touch(page_id);
        }
        apply_bounds(app, &existing, bounds)?;
        apply_view_mode(app, host, page_id, &existing, view_mode)?;
        navigate_label(app, &existing, &resolved.url)?;
        info!(
            target: "gold_band::browser",
            operation = "create-reuse",
            page_id,
            label = existing,
            view_mode = ?view_mode,
            target_url = %browser_log_target(&resolved.url),
            "browser webview command completed"
        );
        return Ok(BrowserPageVm {
            page_id: page_id.to_string(),
            url: resolved.url.to_string(),
            label: existing,
        });
    }

    let evict_candidate = {
        let inner = lock_host(host)?;
        inner
            .eviction_candidate(page_id)
            .and_then(|evict_page_id| inner.entry(&evict_page_id))
    };
    if let Some(candidate) = evict_candidate {
        close_registered_pages(app, host, vec![candidate])?;
    }

    let window = main_window(app)?;
    let label = webview_label(page_id);
    let profile_dir = browser_profile_dir(app)?;
    let allowed_root = resolved.allowed_file_root.clone();
    let builder = browser_webview_builder(
        app,
        page_id,
        &label,
        resolved.url.clone(),
        allowed_root.clone(),
        view_mode,
    )?;
    let builder = builder.data_directory(profile_dir);
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(BROWSER_DATA_STORE_IDENTIFIER);

    window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x.max(0.0), bounds.y.max(0.0)),
            LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
        )
        .map_err(|error| webview_error("browser.webview.create_failed", error))?;

    {
        let mut inner = lock_host(host)?;
        inner.insert(
            page_id.to_string(),
            BrowserNativePage {
                label: label.clone(),
                allowed_file_root: allowed_root,
                view_mode,
                last_http_url: crate::browser_history::http_visit_url(&resolved.url),
            },
        );
    }

    hide_label(app, Some(page_id), &label)?;
    info!(
        target: "gold_band::browser",
        operation = "create",
        page_id,
        label,
        view_mode = ?view_mode,
        target_url = %browser_log_target(&resolved.url),
        registry_count = lock_host(host)?.pages.len(),
        "browser webview command completed"
    );
    Ok(BrowserPageVm {
        page_id: page_id.to_string(),
        url: resolved.url.to_string(),
        label,
    })
}

fn browser_webview_builder(
    app: &AppHandle,
    page_id: &str,
    label: &str,
    url: Url,
    allowed_file_root: Option<PathBuf>,
    view_mode: BrowserViewMode,
) -> CommandResult<WebviewBuilder<tauri::Wry>> {
    let navigation_root = allowed_file_root.clone();
    let title_app = app.clone();
    let title_page = page_id.to_string();
    let load_app = app.clone();
    let load_page = page_id.to_string();
    let load_label = label.to_string();
    let window_app = app.clone();
    let window_page = page_id.to_string();
    let download_app = app.clone();
    let download_page = page_id.to_string();
    let host_app = app.clone();
    let host_page = page_id.to_string();

    let builder = WebviewBuilder::new(label, WebviewUrl::External(url)).focused(false);
    let builder = match browser_user_agent(view_mode) {
        Some(user_agent) => builder.user_agent(user_agent),
        None => builder,
    };
    Ok(builder
        .initialization_script_for_all_frames(BROWSER_LINK_CLICK_SCRIPT)
        .on_navigation(move |target| {
            let current_root =
                current_allowed_root(&host_app, &host_page).or(navigation_root.clone());
            navigation_allowed(target, current_root.as_deref())
        })
        .on_document_title_changed(move |_webview, title| {
            if let Some(url) = last_http_url_for_page(&title_app, &title_page) {
                crate::browser_history::record_title_for_url(&title_app, &url, &title);
            }
            emit_page_event(
                &title_app,
                BrowserPageEventVm {
                    kind: "title".into(),
                    page_id: title_page.clone(),
                    url: None,
                    title: Some(title),
                },
            );
        })
        .on_page_load(move |_webview, payload| {
            let kind = match payload.event() {
                PageLoadEvent::Started => "load-start",
                PageLoadEvent::Finished => "load-finish",
            };
            remember_page_url(&load_app, &load_page, payload.url());
            if payload.event() == PageLoadEvent::Finished {
                crate::browser_history::record_finished_url(&load_app, payload.url());
            }
            info!(
                target: "gold_band::browser",
                operation = "page-load",
                event = kind,
                page_id = load_page,
                label = load_label,
                target_url = %browser_log_target(payload.url()),
                keep_visible = native_page_visible_during_load(&payload.event()),
                "browser webview page event"
            );
            emit_page_event(
                &load_app,
                BrowserPageEventVm {
                    kind: kind.into(),
                    page_id: load_page.clone(),
                    url: Some(payload.url().to_string()),
                    title: None,
                },
            );
        })
        .on_new_window(move |url, _features| {
            emit_page_event(
                &window_app,
                BrowserPageEventVm {
                    kind: "new-window".into(),
                    page_id: window_page.clone(),
                    url: Some(url.to_string()),
                    title: None,
                },
            );
            NewWindowResponse::Deny
        })
        .on_download(move |webview, event| match event {
            DownloadEvent::Requested {
                url: _,
                destination,
            } => {
                let suggested = destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("download")
                    .to_string();
                match webview
                    .dialog()
                    .file()
                    .set_file_name(&suggested)
                    .blocking_save_file()
                {
                    Some(path) => match path.into_path() {
                        Ok(path) => {
                            *destination = path;
                            true
                        }
                        Err(_) => {
                            emit_page_event(
                                &download_app,
                                BrowserPageEventVm {
                                    kind: "download-unsupported".into(),
                                    page_id: download_page.clone(),
                                    url: None,
                                    title: None,
                                },
                            );
                            false
                        }
                    },
                    None => {
                        emit_page_event(
                            &download_app,
                            BrowserPageEventVm {
                                kind: "download-cancelled".into(),
                                page_id: download_page.clone(),
                                url: None,
                                title: None,
                            },
                        );
                        false
                    }
                }
            }
            DownloadEvent::Finished {
                success,
                url,
                path: _,
            } => {
                if !success {
                    emit_page_event(
                        &download_app,
                        BrowserPageEventVm {
                            kind: "download-unsupported".into(),
                            page_id: download_page.clone(),
                            url: Some(url.to_string()),
                            title: None,
                        },
                    );
                }
                true
            }
            _ => true,
        }))
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedBrowserTarget {
    url: Url,
    allowed_file_root: Option<PathBuf>,
}

pub(crate) fn resolve_browser_target(
    raw: &str,
    current_file_root: Option<&Path>,
) -> CommandResult<ResolvedBrowserTarget> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("about:blank") {
        return Ok(ResolvedBrowserTarget {
            url: Url::parse("about:blank").expect("about:blank"),
            allowed_file_root: None,
        });
    }
    if looks_like_local_path(trimmed) {
        return resolved_from_file_path(Path::new(trimmed));
    }
    if let Ok(url) = Url::parse(trimmed) {
        return resolved_from_url(url, current_file_root);
    }
    let prefixed = if trimmed.contains(' ') {
        return Err(navigation_invalid());
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&prefixed).map_err(|_| navigation_invalid())?;
    resolved_from_url(url, current_file_root)
}

fn resolved_from_url(
    url: Url,
    current_file_root: Option<&Path>,
) -> CommandResult<ResolvedBrowserTarget> {
    if !navigation_allowed(&url, current_file_root) {
        if url.scheme() == "file" {
            let path = url.to_file_path().map_err(|_| local_html_grant_failed())?;
            if is_html_path(&path) {
                return resolved_from_file_path(&path);
            }
        }
        return Err(navigation_invalid());
    }
    let allowed_file_root = if url.scheme() == "file" {
        url.to_file_path()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    } else {
        None
    };
    Ok(ResolvedBrowserTarget {
        url,
        allowed_file_root,
    })
}

fn resolved_from_file_path(path: &Path) -> CommandResult<ResolvedBrowserTarget> {
    let canonical = canonicalize_display_path(path).map_err(|_| local_html_grant_failed())?;
    if !is_html_path(&canonical) {
        return Err(navigation_invalid());
    }
    if !canonical.is_file() {
        return Err(local_html_grant_failed());
    }
    let url = Url::from_file_path(&canonical).map_err(|_| local_html_grant_failed())?;
    let allowed_file_root = canonical.parent().map(Path::to_path_buf);
    Ok(ResolvedBrowserTarget {
        url,
        allowed_file_root,
    })
}

pub(crate) fn navigation_allowed(url: &Url, allowed_file_root: Option<&Path>) -> bool {
    if is_privileged_url(url) {
        return false;
    }
    match url.scheme() {
        "https" | "http" | "blob" => true,
        "about" => url.as_str().eq_ignore_ascii_case("about:blank"),
        "file" => allowed_file_root.is_some_and(|root| {
            url.to_file_path()
                .ok()
                .and_then(|path| canonicalize_display_path(&path).ok())
                .is_some_and(|path| path_is_within(&path, root))
        }),
        _ => false,
    }
}

fn is_privileged_url(url: &Url) -> bool {
    matches!(url.scheme(), "tauri" | "ipc" | "javascript" | "data")
        || url.scheme().starts_with("gold-band")
        || url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("ipc.localhost"))
}

pub(crate) fn is_html_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
        })
}

fn looks_like_local_path(value: &str) -> bool {
    let path = Path::new(value);
    if value.contains("://") {
        return value.to_ascii_lowercase().starts_with("file:");
    }
    path.is_absolute()
        || value.starts_with("\\\\")
        || (value.len() >= 3
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':'
            && (value.as_bytes()[2] == b'\\' || value.as_bytes()[2] == b'/'))
        || is_html_path(path)
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let Ok(root) = canonicalize_display_path(root) else {
        return false;
    };
    path == root || path.starts_with(&root)
}

fn canonicalize_display_path(path: &Path) -> std::io::Result<PathBuf> {
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

fn last_http_url_for_page(app: &AppHandle, page_id: &str) -> Option<String> {
    let host = app.try_state::<BrowserHost>()?;
    let inner = host.inner.lock().ok()?;
    inner.pages.get(page_id)?.last_http_url.clone()
}

fn remember_page_url(app: &AppHandle, page_id: &str, url: &Url) {
    let Some(host) = app.try_state::<BrowserHost>() else {
        return;
    };
    let Ok(mut inner) = host.inner.lock() else {
        return;
    };
    if let Some(page) = inner.pages.get_mut(page_id) {
        page.last_http_url = crate::browser_history::http_visit_url(url);
    }
}

fn validate_page_id(page_id: &str) -> CommandResult<&str> {
    if page_id.len() < 8
        || page_id.len() > 64
        || !page_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(navigation_invalid());
    }
    Ok(page_id)
}

fn webview_label(page_id: &str) -> String {
    format!("{BROWSER_WEBVIEW_LABEL_PREFIX}{page_id}")
}

fn browser_profile_dir(app: &AppHandle) -> CommandResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| webview_error("browser.webview.create_failed", error))?
        .join(BROWSER_PROFILE_DIR_NAME);
    std::fs::create_dir_all(&dir)
        .map_err(|error| webview_error("browser.webview.create_failed", error))?;
    Ok(dir)
}

fn main_window(app: &AppHandle) -> CommandResult<Window> {
    app.get_window(MAIN_WEBVIEW_LABEL)
        .ok_or_else(|| CommandErrorVm::new("browser.webview.unavailable", serde_json::json!({})))
}

fn lock_host(host: &BrowserHost) -> CommandResult<std::sync::MutexGuard<'_, BrowserHostInner>> {
    host.inner
        .lock()
        .map_err(|_| CommandErrorVm::new("browser.webview.unavailable", serde_json::json!({})))
}

fn host_label(host: &BrowserHost, page_id: &str) -> CommandResult<String> {
    lock_host(host)?.label_for(page_id).map(str::to_string)
}

fn current_allowed_root(app: &AppHandle, page_id: &str) -> Option<PathBuf> {
    let host = app.try_state::<BrowserHost>()?;
    lock_host(host.inner())
        .ok()?
        .pages
        .get(page_id)
        .and_then(|page| page.allowed_file_root.clone())
}

fn apply_bounds(app: &AppHandle, label: &str, bounds: BrowserBoundsVm) -> CommandResult<()> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| webview_unavailable("set-bounds", None, Some(label)))?;
    webview
        .set_position(LogicalPosition::new(bounds.x.max(0.0), bounds.y.max(0.0)))
        .and_then(|_| {
            webview.set_size(LogicalSize::new(
                bounds.width.max(1.0),
                bounds.height.max(1.0),
            ))
        })
        .map_err(|error| {
            log_webview_failure("set-bounds", None, label, &error);
            webview_operation_error("set-bounds", error)
        })
}

fn navigate_label(app: &AppHandle, label: &str, url: &Url) -> CommandResult<()> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| webview_unavailable("navigate", None, Some(label)))?;
    webview.navigate(url.clone()).map_err(|error| {
        log_webview_failure("navigate", None, label, &error);
        webview_operation_error("navigate", error)
    })
}

fn apply_view_mode(
    app: &AppHandle,
    host: &BrowserHost,
    page_id: &str,
    label: &str,
    view_mode: BrowserViewMode,
) -> CommandResult<()> {
    let current_mode = lock_host(host)?
        .pages
        .get(page_id)
        .map(|page| page.view_mode);
    if current_mode == Some(view_mode) {
        return Ok(());
    }
    if current_mode == Some(BrowserViewMode::Desktop) {
        capture_engine_desktop_user_agent(app, host, label);
    }
    let engine_desktop = lock_host(host)?.engine_desktop_user_agent.clone();
    let user_agent = view_mode_user_agent(view_mode, engine_desktop.as_deref()).to_string();
    set_label_user_agent(app, page_id, label, &user_agent)?;
    if let Some(page) = lock_host(host)?.pages.get_mut(page_id) {
        page.view_mode = view_mode;
    }
    Ok(())
}

fn capture_engine_desktop_user_agent(app: &AppHandle, host: &BrowserHost, label: &str) {
    if lock_host(host)
        .ok()
        .is_some_and(|inner| inner.engine_desktop_user_agent.is_some())
    {
        return;
    }
    let Some(webview) = app.get_webview(label) else {
        return;
    };
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if webview
        .with_webview(move |platform| {
            let _ = tx.send(crate::browser_ua::read_user_agent(&platform));
        })
        .is_err()
    {
        return;
    }
    let Ok(Some(user_agent)) = rx.recv() else {
        return;
    };
    if user_agent.is_empty() {
        return;
    }
    if let Ok(mut inner) = lock_host(host) {
        if inner.engine_desktop_user_agent.is_none() {
            inner.engine_desktop_user_agent = Some(user_agent);
        }
    }
}

fn set_label_user_agent(
    app: &AppHandle,
    page_id: &str,
    label: &str,
    user_agent: &str,
) -> CommandResult<()> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| webview_unavailable("set-user-agent", Some(page_id), Some(label)))?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let next_user_agent = user_agent.to_string();
    webview
        .with_webview(move |platform| {
            let _ = tx.send(crate::browser_ua::set_user_agent(&platform, &next_user_agent));
        })
        .map_err(|error| {
            log_webview_failure("set-user-agent", Some(page_id), label, &error);
            webview_operation_error("set-user-agent", error)
        })?;
    rx.recv()
        .map_err(|_| webview_operation_error("set-user-agent", "user-agent callback dropped"))?
        .map_err(|reason| webview_operation_error("set-user-agent", reason))
}

fn reload_label(app: &AppHandle, page_id: &str, label: &str) -> CommandResult<()> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| webview_unavailable("reload", Some(page_id), Some(label)))?;
    webview.reload().map_err(|error| {
        log_webview_failure("reload", Some(page_id), label, &error);
        webview_operation_error("reload", error)
    })
}

fn native_page_visible_during_load(event: &PageLoadEvent) -> bool {
    matches!(event, PageLoadEvent::Started | PageLoadEvent::Finished)
}

fn settle_registered_closes<Q, F, G>(
    candidates: Vec<(String, String)>,
    mut quiesce: Q,
    mut close: F,
    mut mark_closed: G,
) -> CommandResult<()>
where
    Q: FnMut(&str),
    F: FnMut(&str) -> CommandResult<()>,
    G: FnMut(&str, &str) -> CommandResult<()>,
{
    for (page_id, label) in candidates {
        quiesce(&label);
        close(&label)?;
        mark_closed(&page_id, &label)?;
    }
    Ok(())
}

fn eval_history(
    app: &AppHandle,
    host: &BrowserHost,
    page_id: &str,
    script: &str,
) -> CommandResult<()> {
    let page_id = validate_page_id(page_id)?;
    let label = host_label(host, page_id)?;
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| webview_unavailable("eval", Some(page_id), Some(&label)))?;
    webview.eval(script).map_err(|error| {
        log_webview_failure("eval", Some(page_id), &label, &error);
        webview_operation_error("eval", error)
    })
}

fn hide_other_pages(app: &AppHandle, host: &BrowserHost, keep: &str) -> CommandResult<()> {
    let labels = lock_host(host)?
        .pages
        .iter()
        .filter(|(page_id, _)| page_id.as_str() != keep)
        .map(|(_, page)| page.label.clone())
        .collect::<Vec<_>>();
    for label in labels {
        hide_label(app, None, &label)?;
    }
    Ok(())
}

fn hide_label(app: &AppHandle, page_id: Option<&str>, label: &str) -> CommandResult<()> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| webview_unavailable("hide", page_id, Some(label)))?;
    webview.hide().map_err(|error| {
        log_webview_failure("hide", page_id, label, &error);
        webview_operation_error("hide", error)
    })?;
    info!(
        target: "gold_band::browser",
        operation = "hide",
        page_id = page_id.unwrap_or(""),
        label,
        "browser webview command completed"
    );
    Ok(())
}

fn close_label(app: &AppHandle, label: &str) -> CommandResult<()> {
    let Some(webview) = app.get_webview(label) else {
        info!(
            target: "gold_band::browser",
            operation = "close",
            label,
            already_absent = true,
            "browser webview command completed"
        );
        return Ok(());
    };
    webview.close().map_err(|error| {
        log_webview_failure("close", None, label, &error);
        webview_operation_error("close", error)
    })?;
    info!(target: "gold_band::browser", operation = "close", label, "browser webview command completed");
    Ok(())
}

fn close_registered_pages(
    app: &AppHandle,
    host: &BrowserHost,
    candidates: Vec<(String, String)>,
) -> CommandResult<()> {
    settle_registered_closes(
        candidates,
        |label| hide_label_best_effort(app, label),
        |label| close_label(app, label),
        |page_id, label| {
            let mut inner = lock_host(host)?;
            inner.remove_if_label(page_id, label);
            Ok(())
        },
    )
}

fn hide_label_best_effort(app: &AppHandle, label: &str) {
    let Some(webview) = app.get_webview(label) else {
        return;
    };
    match webview.hide() {
        Ok(()) => info!(
            target: "gold_band::browser",
            operation = "hide-before-close",
            label,
            "browser webview command completed"
        ),
        Err(error) => log_webview_failure("hide-before-close", None, label, &error),
    }
}

fn log_webview_failure(
    operation: &'static str,
    page_id: Option<&str>,
    label: &str,
    error: &impl std::fmt::Display,
) {
    warn!(
        target: "gold_band::browser",
        operation,
        page_id = page_id.unwrap_or(""),
        label,
        error = %error,
        "browser webview command failed"
    );
}

fn browser_log_target(url: &Url) -> String {
    match url.scheme() {
        "http" | "https" => {
            let host = url.host_str().unwrap_or("<unknown>");
            match url.port() {
                Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
                None => format!("{}://{}", url.scheme(), host),
            }
        }
        "file" => "file://<local>".to_string(),
        scheme => format!("{scheme}:"),
    }
}

fn emit_page_event(app: &AppHandle, event: BrowserPageEventVm) {
    if let Err(error) = app.emit(BROWSER_PAGE_EVENT, event) {
        warn!(error = %error, "failed to emit browser page event");
    }
}

fn webview_error(code: &str, error: impl std::fmt::Display) -> CommandErrorVm {
    CommandErrorVm::new(code, serde_json::json!({ "reason": error.to_string() }))
}

fn webview_operation_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> CommandErrorVm {
    CommandErrorVm::new(
        "browser.webview.operation_failed",
        serde_json::json!({ "operation": operation, "reason": error.to_string() }),
    )
}

fn webview_unavailable(
    operation: &'static str,
    page_id: Option<&str>,
    label: Option<&str>,
) -> CommandErrorVm {
    warn!(
        target: "gold_band::browser",
        operation,
        page_id = page_id.unwrap_or(""),
        label = label.unwrap_or(""),
        "browser webview unavailable"
    );
    CommandErrorVm::new(
        "browser.webview.unavailable",
        serde_json::json!({ "operation": operation }),
    )
}

fn navigation_invalid() -> CommandErrorVm {
    CommandErrorVm::new("browser.navigation.invalid", serde_json::json!({}))
}

fn local_html_grant_failed() -> CommandErrorVm {
    CommandErrorVm::new("browser.local_html.grant_failed", serde_json::json!({}))
}

impl BrowserHostInner {
    fn insert(&mut self, page_id: String, page: BrowserNativePage) {
        self.pages.insert(page_id.clone(), page);
        self.touch(&page_id);
    }

    fn touch(&mut self, page_id: &str) {
        self.lru.retain(|id| id != page_id);
        self.lru.push_back(page_id.to_string());
    }

    fn label_for(&self, page_id: &str) -> CommandResult<&str> {
        self.pages
            .get(page_id)
            .map(|page| page.label.as_str())
            .ok_or_else(|| {
                CommandErrorVm::new("browser.webview.unavailable", serde_json::json!({}))
            })
    }

    fn entries(&self) -> Vec<(String, String)> {
        self.pages
            .iter()
            .map(|(page_id, page)| (page_id.clone(), page.label.clone()))
            .collect()
    }

    fn entry(&self, page_id: &str) -> Option<(String, String)> {
        self.pages
            .get(page_id)
            .map(|page| (page_id.to_string(), page.label.clone()))
    }

    fn remove_if_label(&mut self, page_id: &str, label: &str) -> bool {
        let matches = self
            .pages
            .get(page_id)
            .is_some_and(|page| page.label == label);
        if matches {
            self.lru.retain(|id| id != page_id);
            self.pages.remove(page_id);
        }
        matches
    }

    fn eviction_candidate(&self, keep: &str) -> Option<String> {
        if self.pages.len() < BROWSER_LIVE_WEBVIEW_LIMIT {
            return None;
        }
        self.lru
            .iter()
            .find(|page_id| page_id.as_str() != keep)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_and_blank_are_allowed() {
        let https = Url::parse("https://example.com/docs").unwrap();
        let blank = Url::parse("about:blank").unwrap();
        assert!(navigation_allowed(&https, None));
        assert!(navigation_allowed(&blank, None));
    }

    #[test]
    fn privileged_schemes_are_denied() {
        for raw in [
            "tauri://localhost",
            "http://ipc.localhost/",
            "https://ipc.localhost/",
            "gold-band-preview://token",
            "javascript:alert(1)",
            "file:///C:/Windows/notepad.exe",
        ] {
            let url = Url::parse(raw).unwrap();
            assert!(!navigation_allowed(&url, None), "{raw}");
        }
    }

    #[test]
    fn local_html_is_limited_to_authorized_directory() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("index.html");
        std::fs::write(&page, "<html></html>").unwrap();
        let sibling = dir.path().join("app.js");
        std::fs::write(&sibling, "console.log(1)").unwrap();
        let outside = dir.path().parent().unwrap().join("secret.txt");
        std::fs::write(&outside, "nope").unwrap();

        let target = resolve_browser_target(page.to_str().unwrap(), None).unwrap();
        assert!(target.url.scheme() == "file");
        let root = target.allowed_file_root.as_deref();
        assert!(navigation_allowed(
            &Url::from_file_path(&sibling).unwrap(),
            root
        ));
        assert!(!navigation_allowed(
            &Url::from_file_path(&outside).unwrap(),
            root
        ));
    }

    #[test]
    fn address_bar_https_is_normalized() {
        let target = resolve_browser_target("example.com/path", None).unwrap();
        assert_eq!(target.url.as_str(), "https://example.com/path");
    }

    #[test]
    fn mailto_is_rejected() {
        assert!(resolve_browser_target("mailto:a@b.com", None).is_err());
    }

    #[test]
    fn load_start_keeps_native_webview_visible_for_progressive_rendering() {
        assert!(native_page_visible_during_load(&PageLoadEvent::Started));
        assert!(native_page_visible_during_load(&PageLoadEvent::Finished));
    }

    #[test]
    fn failed_close_is_not_marked_closed_before_native_confirmation() {
        let candidates = vec![("page-1".to_string(), "gb-b-page-1".to_string())];
        let mut quiesced = Vec::new();
        let mut marked = Vec::new();
        let result = settle_registered_closes(
            candidates,
            |label| quiesced.push(label.to_string()),
            |_label| {
                Err(CommandErrorVm::new(
                    "browser.webview.close_failed",
                    serde_json::json!({}),
                ))
            },
            |page_id, _label| {
                marked.push(page_id.to_string());
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(quiesced, vec!["gb-b-page-1"]);
        assert!(marked.is_empty());
    }

    #[test]
    fn browser_log_target_omits_paths_queries_and_local_file_names() {
        let remote = Url::parse("https://example.com/private/path?token=secret#fragment").unwrap();
        let local = Url::parse("file:///C:/Users/example/private.html").unwrap();
        assert_eq!(browser_log_target(&remote), "https://example.com");
        assert_eq!(browser_log_target(&local), "file://<local>");
    }

    #[test]
    fn desktop_view_keeps_engine_user_agent_and_mobile_uses_android_chrome() {
        assert_eq!(browser_user_agent(BrowserViewMode::Desktop), None);
        let mobile = browser_user_agent(BrowserViewMode::Mobile).expect("mobile ua");
        assert!(mobile.contains("Mobile"));
        assert!(mobile.contains("Android"));
        assert_eq!(
            view_mode_user_agent(BrowserViewMode::Mobile, Some("engine-desktop")),
            MOBILE_BROWSER_USER_AGENT
        );
        assert_eq!(
            view_mode_user_agent(BrowserViewMode::Desktop, Some("engine-desktop")),
            "engine-desktop"
        );
        assert_eq!(
            view_mode_user_agent(BrowserViewMode::Desktop, None),
            FALLBACK_DESKTOP_USER_AGENT
        );
        assert!(!FALLBACK_DESKTOP_USER_AGENT.contains("Mobile"));
    }

    #[test]
    fn in_page_http_link_clicks_are_forced_into_same_document_navigation() {
        assert!(BROWSER_LINK_CLICK_SCRIPT.contains("stopImmediatePropagation"));
        assert!(BROWSER_LINK_CLICK_SCRIPT.contains("location.assign"));
        assert!(BROWSER_LINK_CLICK_SCRIPT.contains("a[href]"));
    }
}
