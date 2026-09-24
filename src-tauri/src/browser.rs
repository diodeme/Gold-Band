// Windows WebView2 deadlocks if child webviews are created from a synchronous IPC
// command. The handlers below stay async on purpose even when they do not await,
// so Tauri runs them off the WebView2 callback thread.
#![allow(clippy::unused_async)]

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::http::{Method, Response, StatusCode, header};
use tauri::webview::Color as WebviewColor;
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, Window};
use tauri_plugin_dialog::DialogExt;
use tracing::{info, warn};
use url::Url;

use crate::commands::{CommandErrorVm, CommandResult};
use crate::state::DesktopState;

pub const BROWSER_PAGE_EVENT: &str = "gold-band://browser-page";
pub const BROWSER_ADDRESS_SUGGESTION_ACTION_EVENT: &str =
    "gold-band://browser-address-suggestion-action";
pub const BROWSER_LOCAL_FILE_PROTOCOL: &str = "gold-band-browser-file";
pub const BROWSER_LIVE_WEBVIEW_LIMIT: usize = 5;
const BROWSER_LOCAL_FILE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAIN_WEBVIEW_LABEL: &str = "main";
const BROWSER_PROFILE_DIR_NAME: &str = "browser-profile";
const BROWSER_WEBVIEW_LABEL_PREFIX: &str = "gb-b-";
const BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL: &str = "gb-browser-address-suggestions";
const BROWSER_ADDRESS_SUGGESTION_MAX_ITEMS: usize = 9;
const BROWSER_ADDRESS_SUGGESTION_MAX_FAVICON_BYTES: usize = 256 * 1024;
const BROWSER_ADDRESS_SUGGESTION_MAX_KEY_BYTES: usize = 16 * 1024;
const BROWSER_LINK_CLICK_SCRIPT: &str =
    include_str!("../../web/src/components/workspace/browser/browser-link-click.js");
/// Applied at document start inside the suggestion webview so the first paint already uses
/// the live theme instead of the stylesheet's dark fallback.
const BROWSER_ADDRESS_SUGGESTION_THEME_SCRIPT: &str =
    include_str!("../../web/src/components/workspace/browser/browser-address-suggestion-theme.js");
#[cfg(target_os = "macos")]
const BROWSER_DATA_STORE_IDENTIFIER: [u8; 16] = *b"GoldBandBrowse01";

#[derive(Debug, Default)]
pub struct BrowserHost {
    inner: Mutex<BrowserHostInner>,
    /// Serializes address-suggestion overlay mutations. The frontend may issue
    /// several show/hide requests while a focus or layout change settles; without
    /// this lock two requests can race the same webview label (one create fails)
    /// and a stale request can hide the overlay a newer one just displayed.
    overlay: tokio::sync::Mutex<()>,
}

#[derive(Debug, Default)]
struct BrowserHostInner {
    pages: HashMap<String, BrowserNativePage>,
    lru: VecDeque<String>,
    engine_desktop_user_agent: Option<String>,
    address_suggestion_revision: u64,
    address_suggestion_state_json: Option<String>,
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
        BrowserViewMode::Desktop => {
            engine_desktop_user_agent.unwrap_or(FALLBACK_DESKTOP_USER_AGENT)
        }
    }
}

#[derive(Debug, Clone)]
struct BrowserNativePage {
    label: String,
    allowed_file_root: Option<PathBuf>,
    view_mode: BrowserViewMode,
    last_http_url: Option<String>,
    /// Last top-level location already projected to the address bar. Same-document
    /// engine events compare against this so an unchanged URL is not published again.
    last_location: Option<String>,
    /// Latest document-location read. A script result from an older history change
    /// must not overwrite a newer load or a newer read.
    location_probe: u64,
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
pub struct BrowserResolveLocalHtmlInput {
    pub project_id: String,
    pub raw_href: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLocalHtmlTargetVm {
    pub canonical_path: String,
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBoundsVm {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAddressSuggestionItemVm {
    pub key: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub favicon_data_url: Option<String>,
    pub remove_label: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAddressSuggestionThemeVm {
    pub dark: bool,
    pub theme_id: String,
    pub color_scheme: String,
    pub visual_quality: String,
    pub material_model: String,
    pub variables: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAddressSuggestionOverlayInput {
    pub revision: u64,
    pub bounds: BrowserBoundsVm,
    pub active_index: Option<usize>,
    pub items: Vec<BrowserAddressSuggestionItemVm>,
    pub theme: BrowserAddressSuggestionThemeVm,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAddressSuggestionOverlayRevisionInput {
    pub revision: u64,
}

/// One address suggestion interaction coming back from the trusted overlay surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAddressSuggestionActionVm {
    pub revision: u64,
    pub kind: String,
    pub key: String,
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
    create_or_reuse_page(
        &app,
        &host,
        page_id,
        resolved,
        input.bounds,
        input.view_mode,
    )
}

#[tauri::command]
pub async fn browser_resolve_local_html(
    state: State<'_, DesktopState>,
    input: BrowserResolveLocalHtmlInput,
) -> CommandResult<BrowserLocalHtmlTargetVm> {
    let locator = crate::workspace_files::resolve_file_link_locator(
        state.inner(),
        &input.project_id,
        &input.raw_href,
    )
    .await?;
    let path = PathBuf::from(&locator.canonical_path);
    if !is_html_path(&path) {
        return Err(navigation_invalid());
    }
    Ok(BrowserLocalHtmlTargetVm {
        canonical_path: locator.canonical_path,
    })
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
    let _overlay_guard = host.overlay.lock().await;
    {
        let mut inner = lock_host(&host)?;
        invalidate_address_suggestion_projection(&mut inner);
    }
    hide_address_suggestions(&app)?;
    info!(target: "gold_band::browser", operation = "hide-all", "browser webview command completed");
    Ok(())
}

#[tauri::command]
pub async fn browser_show_address_suggestions(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserAddressSuggestionOverlayInput,
) -> CommandResult<()> {
    validate_address_suggestion_overlay(&input)?;
    info!(
        target: "gold_band::browser",
        operation = "show-address-suggestions",
        revision = input.revision,
        items = input.items.len(),
        x = input.bounds.x,
        y = input.bounds.y,
        width = input.bounds.width,
        height = input.bounds.height,
        "browser webview command started"
    );
    let state_json = serde_json::to_string(&input)
        .map_err(|error| webview_error("browser.address_suggestions.invalid", error))?;
    {
        let mut inner = lock_host(&host)?;
        if !update_address_suggestion_projection(
            &mut inner,
            input.revision,
            Some(state_json.clone()),
        ) {
            info!(
                target: "gold_band::browser",
                operation = "show-address-suggestions",
                revision = input.revision,
                "browser webview command skipped for stale revision"
            );
            return Ok(());
        }
    }

    let _overlay_guard = host.overlay.lock().await;
    // While this request was waiting for the overlay lock, a newer show or hide may
    // have replaced the projection. The overlay must converge to the newest
    // projection instead of hiding what a newer revision just displayed.
    let current_projection = {
        let inner = lock_host(&host)?;
        address_suggestion_overlay_target(&inner)
    };
    let Some(current_json) = current_projection else {
        info!(
            target: "gold_band::browser",
            operation = "show-address-suggestions",
            revision = input.revision,
            "browser webview command superseded by a newer hide"
        );
        return hide_address_suggestions(&app);
    };
    let current: BrowserAddressSuggestionOverlayInput = serde_json::from_str(&current_json)
        .map_err(|error| webview_error("browser.address_suggestions.invalid", error))?;
    let (webview, created) = match app.get_webview(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL) {
        Some(webview) => (webview, false),
        None => (
            create_address_suggestion_webview(&app, &current, &current_json)?,
            true,
        ),
    };
    apply_bounds(
        &app,
        BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
        current.bounds,
    )?;
    eval_address_suggestion_state(&webview, &current_json);
    if created {
        // A freshly created overlay renders hidden until React has synchronously committed
        // the rows and theme. A bounded fallback keeps a broken surface from remaining
        // invisible forever, but normal readiness never waits for an animation frame.
        info!(
            target: "gold_band::browser",
            operation = "show-address-suggestions",
            revision = current.revision,
            requested_revision = input.revision,
            label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
            "browser address suggestions created hidden pending committed content"
        );
        spawn_address_suggestion_reveal_fallback(app.clone());
        return Ok(());
    }
    show_address_suggestions_overlay(&app)?;
    info!(
        target: "gold_band::browser",
        operation = "show-address-suggestions",
        revision = current.revision,
        requested_revision = input.revision,
        created,
        label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
        "browser webview command completed"
    );
    Ok(())
}

#[tauri::command]
pub async fn browser_hide_address_suggestions(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserAddressSuggestionOverlayRevisionInput,
) -> CommandResult<()> {
    info!(
        target: "gold_band::browser",
        operation = "hide-address-suggestions",
        revision = input.revision,
        "browser webview command started"
    );
    {
        let mut inner = lock_host(&host)?;
        if !update_address_suggestion_projection(&mut inner, input.revision, None) {
            return Ok(());
        }
    }
    {
        let _overlay_guard = host.overlay.lock().await;
        hide_address_suggestions(&app)?;
    }
    info!(
        target: "gold_band::browser",
        operation = "hide-address-suggestions",
        revision = input.revision,
        "browser webview command completed"
    );
    Ok(())
}

/// Receives one suggestion interaction from the overlay webview and forwards it to the main
/// webview. Routed through a command instead of a cross-webview emit so the interaction is
/// validated and recorded in `runtime.log`; an overlay click can never fail silently again.
#[tauri::command]
pub async fn browser_address_suggestion_action(
    app: AppHandle,
    input: BrowserAddressSuggestionActionVm,
) -> CommandResult<()> {
    info!(
        target: "gold_band::browser",
        operation = "address-suggestion-action",
        revision = input.revision,
        kind = %input.kind,
        key_bytes = input.key.len(),
        "browser address suggestion action received"
    );
    validate_address_suggestion_action(&input)?;
    app.emit_to(
        MAIN_WEBVIEW_LABEL,
        BROWSER_ADDRESS_SUGGESTION_ACTION_EVENT,
        input.clone(),
    )
    .map_err(|error| {
        log_webview_failure(
            "forward-address-suggestion-action",
            None,
            MAIN_WEBVIEW_LABEL,
            &error,
        );
        webview_operation_error("forward-address-suggestion-action", error)
    })?;
    info!(
        target: "gold_band::browser",
        operation = "address-suggestion-action",
        revision = input.revision,
        kind = %input.kind,
        "browser address suggestion action forwarded"
    );
    Ok(())
}

/// The trusted overlay surface reports that React has committed the suggestion rows and theme
/// for a revision. The webview stays hidden while loading, so readiness cannot depend on
/// `requestAnimationFrame`: WebView2 may suspend animation frames for a hidden surface.
#[tauri::command]
pub async fn browser_address_suggestions_ready(
    app: AppHandle,
    host: State<'_, BrowserHost>,
    input: BrowserAddressSuggestionOverlayRevisionInput,
) -> CommandResult<()> {
    let wanted = {
        let inner = lock_host(&host)?;
        address_suggestion_revision_is_ready(&inner, input.revision)
    };
    if !wanted {
        return Ok(());
    }
    show_address_suggestions_overlay(&app)?;
    info!(
        target: "gold_band::browser",
        operation = "show-address-suggestions-ready",
        revision = input.revision,
        label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
        "browser address suggestions shown after the surface committed"
    );
    Ok(())
}

fn validate_address_suggestion_action(
    input: &BrowserAddressSuggestionActionVm,
) -> CommandResult<()> {
    if input.revision == 0
        || !matches!(input.kind.as_str(), "choose" | "remove" | "dismiss")
        || input.key.is_empty()
        || input.key.len() > BROWSER_ADDRESS_SUGGESTION_MAX_KEY_BYTES
    {
        return Err(address_suggestions_invalid());
    }
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
    navigate_label(&app, &label, &resolved.navigation_url)?;
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
pub async fn browser_discard_all(
    app: AppHandle,
    host: State<'_, BrowserHost>,
) -> CommandResult<()> {
    let candidates = lock_host(&host)?.entries();
    info!(target: "gold_band::browser", operation = "discard-all", count = candidates.len(), "browser webview command started");
    close_registered_pages(&app, host.inner(), candidates)?;
    {
        let _overlay_guard = host.overlay.lock().await;
        close_label(&app, BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL)?;
        let mut inner = lock_host(&host)?;
        invalidate_address_suggestion_projection(&mut inner);
    }
    let remaining = lock_host(&host)?.pages.len();
    info!(target: "gold_band::browser", operation = "discard-all", remaining, "browser webview command completed");
    Ok(())
}

fn create_address_suggestion_webview(
    app: &AppHandle,
    state: &BrowserAddressSuggestionOverlayInput,
    state_json: &str,
) -> CommandResult<tauri::Webview> {
    let bounds = state.bounds;
    let surface_url = address_suggestion_surface_url(app)?;
    info!(
        target: "gold_band::browser",
        operation = "create-address-suggestions",
        label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
        surface_url = %surface_url,
        x = bounds.x,
        y = bounds.y,
        width = bounds.width,
        height = bounds.height,
        "browser webview command started"
    );
    let window = main_window(app)?;
    let load_app = app.clone();
    let profile_dir = address_suggestion_profile_dir(app)?;
    let theme_json = serde_json::to_string(&state.theme)
        .map_err(|error| webview_error("browser.address_suggestions.invalid", error))?;
    let initialization_script = address_suggestion_initialization_script(state_json, &theme_json);
    // Mirror the proven browser page webviews: navigate a child webview to an absolute
    // URL instead of an app-relative path, and log every document transition so a blank
    // overlay can never be mistaken for a working one again.
    let builder = WebviewBuilder::new(
        BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
        WebviewUrl::External(surface_url.clone()),
    )
    .focused(false)
    // The overlay is a trusted application surface, but it must not share the main
    // webview environment: that environment belongs to the transparent main window and
    // produced a child webview without any WebView2 content. Own data directory mirrors
    // the browser page webviews, which do render correctly.
    .data_directory(profile_dir)
    // A newly created child webview paints its default background before the document has
    // content. Keep it transparent and hidden until the first load finished, so the first
    // address bar focus never flashes a black rectangle.
    .background_color(WebviewColor(0, 0, 0, 0))
    .initialization_script(initialization_script)
    .on_page_load(move |webview, payload| {
        info!(
            target: "gold_band::browser",
            operation = "address-suggestions-page",
            label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
            event = ?payload.event(),
            url = %payload.url(),
            "browser address suggestions page event"
        );
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        let Some(host) = load_app.try_state::<BrowserHost>() else {
            return;
        };
        let state_json = lock_host(host.inner())
            .ok()
            .and_then(|inner| inner.address_suggestion_state_json.clone());
        if let Some(state_json) = state_json {
            eval_address_suggestion_state(&webview, &state_json);
        }
    });
    window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x.max(0.0), bounds.y.max(0.0)),
            LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
        )
        .map_err(|error| webview_error("browser.webview.create_failed", error))?;
    crate::window_chrome::raise_undecorated_edge_resize_for_app(app);
    let webview = app
        .get_webview(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL)
        .ok_or_else(|| {
            webview_unavailable(
                "create-address-suggestions",
                None,
                Some(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL),
            )
        })?;
    // Hide before any navigation: the projection is already stored, so the page load
    // handler owns the first reveal and there is no window where an uninitialized overlay
    // could flash.
    hide_label(app, None, BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL)?;
    let document_url = webview.url().ok();
    info!(
        target: "gold_band::browser",
        operation = "create-address-suggestions",
        label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
        document_url = ?document_url.as_ref().map(Url::as_str),
        "browser webview command completed"
    );
    if document_url
        .as_ref()
        .and_then(|url| url.query())
        .is_none_or(|query| !query.contains("surface=browser-address-suggestions"))
    {
        webview.navigate(surface_url).map_err(|error| {
            log_webview_failure(
                "navigate-address-suggestions",
                None,
                BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
                &error,
            );
            webview_operation_error("navigate-address-suggestions", error)
        })?;
        info!(
            target: "gold_band::browser",
            operation = "navigate-address-suggestions",
            label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
            "browser address suggestions surface navigated explicitly"
        );
    }
    Ok(webview)
}

/// The address suggestion overlay loads the same application bundle as the main webview,
/// so its absolute URL is derived from the main webview origin. This works for the dev
/// server and for the packaged asset protocol without duplicating Tauri's URL resolution.
fn address_suggestion_surface_url(app: &AppHandle) -> CommandResult<Url> {
    let main = app.get_webview(MAIN_WEBVIEW_LABEL).ok_or_else(|| {
        webview_unavailable("resolve-surface-url", None, Some(MAIN_WEBVIEW_LABEL))
    })?;
    let mut url = main
        .url()
        .map_err(|error| webview_error("browser.address_suggestions.invalid", error))?;
    url.set_path("/index.html");
    url.set_query(Some("surface=browser-address-suggestions"));
    url.set_fragment(None);
    Ok(url)
}

fn address_suggestion_profile_dir(app: &AppHandle) -> CommandResult<PathBuf> {
    let dir = browser_profile_dir(app)?.join("address-suggestions");
    std::fs::create_dir_all(&dir)
        .map_err(|error| webview_error("browser.address_suggestions.profile_failed", error))?;
    Ok(dir)
}

fn address_suggestion_initialization_script(state_json: &str, theme_json: &str) -> String {
    format!(
        "window.__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__ = {state_json};\n{BROWSER_ADDRESS_SUGGESTION_THEME_SCRIPT}({theme_json});"
    )
}

const ADDRESS_SUGGESTION_REVEAL_FALLBACK_MS: u64 = 1_200;

/// Bounded safety net for readiness: if the suggestion document never reports a committed
/// revision, reveal the overlay anyway instead of leaving it invisible forever.
fn spawn_address_suggestion_reveal_fallback(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(
            ADDRESS_SUGGESTION_REVEAL_FALLBACK_MS,
        ))
        .await;
        let Some(host) = app.try_state::<BrowserHost>() else {
            return;
        };
        let wants_visible = lock_host(host.inner())
            .ok()
            .is_some_and(|inner| inner.address_suggestion_state_json.is_some());
        if !wants_visible {
            return;
        }
        if app
            .get_webview(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL)
            .is_none()
        {
            return;
        }
        match show_address_suggestions_overlay(&app) {
            Ok(()) => info!(
                target: "gold_band::browser",
                operation = "show-address-suggestions-fallback",
                label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
                "browser address suggestions shown by bounded fallback"
            ),
            Err(error) => log_webview_failure(
                "show-address-suggestions-fallback",
                None,
                BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
                &format!("{error:?}"),
            ),
        }
    });
}

fn eval_address_suggestion_state(webview: &tauri::Webview, state_json: &str) {
    let script = format!(
        "window.__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__ = {state_json}; window.__goldBandSetBrowserAddressSuggestions?.(window.__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__);"
    );
    if let Err(error) = webview.eval(&script) {
        log_webview_failure(
            "update-address-suggestions",
            None,
            BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
            &error,
        );
    }
}

fn attach_page_focus_dismisses_address_suggestions(app: &AppHandle, label: &str) {
    #[cfg(windows)]
    attach_windows_page_focus_dismisses_address_suggestions(app, label);
    #[cfg(not(windows))]
    let _ = (app, label);
}

/// Clicking a browsing page moves focus off the address field. That must dismiss the
/// suggestion overlay, but address-field blur cannot do it: overlay clicks also blur
/// the address field, and hiding there is what made delete flash.
#[cfg(windows)]
fn attach_windows_page_focus_dismisses_address_suggestions(app: &AppHandle, label: &str) {
    use tauri::webview::PlatformWebview;
    use webview2_com::FocusChangedEventHandler;

    let Some(webview) = app.get_webview(label) else {
        return;
    };
    let app = app.clone();
    let page_label = label.to_string();
    let _ = webview.with_webview(move |platform: PlatformWebview| {
        let mut token = 0;
        let app = app.clone();
        let result = unsafe {
            platform.controller().add_GotFocus(
                &FocusChangedEventHandler::create(Box::new(move |_, _| {
                    emit_address_suggestion_dismiss_for_page_focus(&app);
                    Ok(())
                })),
                &mut token,
            )
        };
        if let Err(error) = result {
            log_webview_failure(
                "watch-page-focus-address-suggestions",
                None,
                &page_label,
                &error,
            );
        }
    });
}

fn emit_address_suggestion_dismiss_for_page_focus(app: &AppHandle) {
    let Some(host) = app.try_state::<BrowserHost>() else {
        return;
    };
    let revision = match lock_host(&host) {
        Ok(inner) if inner.address_suggestion_state_json.is_some() => {
            inner.address_suggestion_revision
        }
        _ => return,
    };
    let payload = BrowserAddressSuggestionActionVm {
        revision,
        kind: "dismiss".into(),
        key: "page-focus".into(),
    };
    if let Err(error) = app.emit_to(
        MAIN_WEBVIEW_LABEL,
        BROWSER_ADDRESS_SUGGESTION_ACTION_EVENT,
        payload,
    ) {
        log_webview_failure(
            "forward-address-suggestion-dismiss",
            None,
            MAIN_WEBVIEW_LABEL,
            &error,
        );
    }
}

fn hide_address_suggestions(app: &AppHandle) -> CommandResult<()> {
    let Some(webview) = app.get_webview(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL) else {
        return Ok(());
    };
    webview.hide().map_err(|error| {
        log_webview_failure(
            "hide-address-suggestions",
            None,
            BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
            &error,
        );
        webview_operation_error("hide-address-suggestions", error)
    })
}

/// Shows the suggestion overlay above every other native layer.
///
/// Child webviews are inserted at the top of the window z-order when they are created, so a
/// browsing page created after the overlay would otherwise cover the suggestion list and let
/// only the strip above the page viewport show through.
fn show_address_suggestions_overlay(app: &AppHandle) -> CommandResult<()> {
    raise_address_suggestion_overlay(app);
    let webview = app
        .get_webview(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL)
        .ok_or_else(|| {
            webview_unavailable(
                "show-address-suggestions",
                None,
                Some(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL),
            )
        })?;
    webview.show().map_err(|error| {
        log_webview_failure(
            "show-address-suggestions",
            None,
            BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
            &error,
        );
        webview_operation_error("show-address-suggestions", error)
    })
}

#[cfg(target_os = "windows")]
fn raise_address_suggestion_overlay(app: &AppHandle) {
    use tauri::webview::PlatformWebview;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    let Some(webview) = app.get_webview(BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL) else {
        return;
    };
    let app_for_raise = app.clone();
    let _ = webview.with_webview(move |platform: PlatformWebview| {
        let mut parent = Default::default();
        if unsafe { platform.controller().ParentWindow(&mut parent) }.is_err() {
            return;
        }
        let container = HWND(parent.0);
        let moved = unsafe {
            SetWindowPos(
                container,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        match moved {
            Ok(()) => info!(
                target: "gold_band::browser",
                operation = "raise-address-suggestions",
                label = BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
                "browser address suggestions overlay raised above page webviews"
            ),
            Err(error) => log_webview_failure(
                "raise-address-suggestions",
                None,
                BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL,
                &error,
            ),
        }
        crate::window_chrome::raise_undecorated_edge_resize_for_app(&app_for_raise);
    });
}

#[cfg(not(target_os = "windows"))]
fn raise_address_suggestion_overlay(_app: &AppHandle) {}

fn validate_address_suggestion_overlay(
    input: &BrowserAddressSuggestionOverlayInput,
) -> CommandResult<()> {
    let bounds = input.bounds;
    let bounds_valid = [bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(f64::is_finite)
        && bounds.x >= 0.0
        && bounds.y >= 0.0
        && (1.0..=16_384.0).contains(&bounds.width)
        && (1.0..=16_384.0).contains(&bounds.height);
    if input.revision == 0
        || !bounds_valid
        || input.items.is_empty()
        || input.items.len() > BROWSER_ADDRESS_SUGGESTION_MAX_ITEMS
        || input
            .active_index
            .is_some_and(|index| index >= input.items.len())
        || input.theme.variables.len() > 32
        || [
            &input.theme.theme_id,
            &input.theme.color_scheme,
            &input.theme.visual_quality,
            &input.theme.material_model,
        ]
        .into_iter()
        .any(|value| value.len() > 128)
    {
        return Err(address_suggestions_invalid());
    }
    for (name, value) in &input.theme.variables {
        if !name.starts_with("--") || name.len() > 96 || value.len() > 512 {
            return Err(address_suggestions_invalid());
        }
    }
    for item in &input.items {
        if item.key.is_empty()
            || item.key.len() > 16_384
            || !matches!(item.kind.as_str(), "search" | "visit")
            || item.title.len() > 16_384
            || item.detail.len() > 16_384
            || item
                .remove_label
                .as_ref()
                .is_some_and(|value| value.len() > 512)
            || item.favicon_data_url.as_ref().is_some_and(|value| {
                value.len() > BROWSER_ADDRESS_SUGGESTION_MAX_FAVICON_BYTES
                    || !value.starts_with("data:image/")
            })
        {
            return Err(address_suggestions_invalid());
        }
    }
    Ok(())
}

fn update_address_suggestion_projection(
    inner: &mut BrowserHostInner,
    revision: u64,
    state_json: Option<String>,
) -> bool {
    if revision < inner.address_suggestion_revision {
        return false;
    }
    inner.address_suggestion_revision = revision;
    inner.address_suggestion_state_json = state_json;
    true
}

/// A request that lost an overlay race must converge to the newest projection: it
/// shows the newest payload, or hides when a newer hide won. It must never hide an
/// overlay that a newer revision just displayed.
fn address_suggestion_overlay_target(inner: &BrowserHostInner) -> Option<String> {
    inner.address_suggestion_state_json.clone()
}

fn address_suggestion_revision_is_ready(inner: &BrowserHostInner, revision: u64) -> bool {
    inner.address_suggestion_state_json.is_some() && inner.address_suggestion_revision == revision
}

fn invalidate_address_suggestion_projection(inner: &mut BrowserHostInner) {
    inner.address_suggestion_revision = inner.address_suggestion_revision.saturating_add(1);
    inner.address_suggestion_state_json = None;
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
        navigate_label(app, &existing, &resolved.navigation_url)?;
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
        let evicted_page_id = candidate.0.clone();
        close_registered_pages(app, host, vec![candidate])?;
        emit_page_event(
            app,
            BrowserPageEventVm {
                kind: "discarded".to_string(),
                page_id: evicted_page_id,
                url: None,
                title: None,
            },
        );
    }

    let window = main_window(app)?;
    let label = webview_label(page_id);
    let profile_dir = browser_profile_dir(app)?;
    let allowed_root = resolved.allowed_file_root.clone();
    let builder = browser_webview_builder(
        app,
        page_id,
        &label,
        resolved.navigation_url.clone(),
        view_mode,
    )?;
    let builder = builder.data_directory(profile_dir);
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(BROWSER_DATA_STORE_IDENTIFIER);

    {
        let mut inner = lock_host(host)?;
        inner.insert(
            page_id.to_string(),
            BrowserNativePage {
                label: label.clone(),
                allowed_file_root: allowed_root,
                view_mode,
                last_http_url: crate::browser_history::http_visit_url(&resolved.url),
                last_location: Some(resolved.url.to_string()),
                location_probe: 0,
            },
        );
    }

    if let Err(error) = window.add_child(
        builder,
        LogicalPosition::new(bounds.x.max(0.0), bounds.y.max(0.0)),
        LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
    ) {
        let _ = lock_host(host)?.remove_if_label(page_id, &label);
        return Err(webview_error("browser.webview.create_failed", error));
    }
    crate::window_chrome::raise_undecorated_edge_resize_for_app(app);

    hide_label(app, Some(page_id), &label)?;
    attach_page_focus_dismisses_address_suggestions(app, &label);
    crate::browser_location::attach_document_location_watch(app, page_id, &label);
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
    view_mode: BrowserViewMode,
) -> CommandResult<WebviewBuilder<tauri::Wry>> {
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
            let current_root = current_allowed_root(&host_app, &host_page);
            let allowed = navigation_allowed(target, current_root.as_deref());
            if allowed
                && matches!(target.scheme(), "http" | "https")
                && !is_browser_local_file_url(target)
            {
                clear_allowed_root(&host_app, &host_page);
            }
            allowed
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
            let display_url = current_allowed_root(&load_app, &load_page)
                .as_deref()
                .map(|root| browser_display_url(payload.url(), Some(root)))
                .unwrap_or_else(|| payload.url().clone());
            remember_page_url(&load_app, &load_page, &display_url);
            if payload.event() == PageLoadEvent::Finished {
                crate::browser_history::record_finished_url(&load_app, &display_url);
            }
            info!(
                target: "gold_band::browser",
                operation = "page-load",
                event = kind,
                page_id = load_page,
                label = load_label,
                target_url = %browser_log_target(&display_url),
                keep_visible = native_page_visible_during_load(&payload.event()),
                "browser webview page event"
            );
            emit_page_event(
                &load_app,
                BrowserPageEventVm {
                    kind: kind.into(),
                    page_id: load_page.clone(),
                    url: Some(display_url.to_string()),
                    title: None,
                },
            );
        })
        .on_new_window(move |url, _features| {
            let display_url = current_allowed_root(&window_app, &window_page)
                .as_deref()
                .map(|root| browser_display_url(&url, Some(root)))
                .unwrap_or(url);
            emit_page_event(
                &window_app,
                BrowserPageEventVm {
                    kind: "new-window".into(),
                    page_id: window_page.clone(),
                    url: Some(display_url.to_string()),
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
    navigation_url: Url,
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
            navigation_url: Url::parse("about:blank").expect("about:blank"),
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
    } else if is_browser_local_file_url(&url) {
        // Re-navigating an already authorized local page must keep its directory
        // instead of silently dropping the grant.
        current_file_root.map(Path::to_path_buf)
    } else {
        None
    };
    let navigation_url = browser_navigation_url(&url, allowed_file_root.as_deref())?;
    Ok(ResolvedBrowserTarget {
        url,
        navigation_url,
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
    let navigation_url = browser_navigation_url(&url, allowed_file_root.as_deref())?;
    Ok(ResolvedBrowserTarget {
        url,
        navigation_url,
        allowed_file_root,
    })
}

fn browser_navigation_url(url: &Url, allowed_file_root: Option<&Path>) -> CommandResult<Url> {
    let path = if url.scheme() == "file" {
        let root = allowed_file_root.ok_or_else(local_html_grant_failed)?;
        url.to_file_path()
            .map_err(|_| local_html_grant_failed())
            .and_then(|path| {
                path_is_within(&path, root)
                    .then_some(path)
                    .ok_or_else(local_html_grant_failed)
            })?
    } else if is_browser_local_file_url(url) {
        // Keep the directory the page was granted instead of dropping it when the
        // address is typed or reloaded directly.
        let root = allowed_file_root.ok_or_else(local_html_grant_failed)?;
        browser_local_path(url, root).map_err(|_| local_html_grant_failed())?
    } else {
        return Ok(url.clone());
    };
    let root = allowed_file_root.ok_or_else(local_html_grant_failed)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| local_html_grant_failed())?;
    // WebView2 cannot navigate directly to a non-standard scheme; wry only rewrites
    // the initial URL. Use the documented workaround form on Windows and the
    // registered scheme elsewhere.
    let base = if cfg!(windows) {
        format!("http://{BROWSER_LOCAL_FILE_PROTOCOL}.localhost/")
    } else {
        format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/")
    };
    let mut navigation_url = Url::parse(&base).map_err(|_| local_html_grant_failed())?;
    {
        let mut segments = navigation_url
            .path_segments_mut()
            .map_err(|_| local_html_grant_failed())?;
        segments.clear();
        for component in relative.components() {
            let std::path::Component::Normal(segment) = component else {
                return Err(local_html_grant_failed());
            };
            let segment = segment.to_str().ok_or_else(local_html_grant_failed)?;
            segments.push(segment);
        }
    }
    Ok(navigation_url)
}

pub(crate) fn is_browser_local_file_url(url: &Url) -> bool {
    url.scheme() == BROWSER_LOCAL_FILE_PROTOCOL
        || (matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case(&format!("{BROWSER_LOCAL_FILE_PROTOCOL}.localhost"))
            }))
}

fn browser_local_path(url: &Url, root: &Path) -> Result<PathBuf, ()> {
    if !is_browser_local_file_url(url) {
        return Err(());
    }
    let mut relative = PathBuf::new();
    for segment in url.path_segments().ok_or(())? {
        let decoded = percent_encoding::percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| ())?;
        if decoded.is_empty() || decoded == "." || decoded == ".." || decoded.contains(['/', '\\'])
        {
            return Err(());
        }
        relative.push(decoded.as_ref());
    }
    if relative.as_os_str().is_empty() {
        return Err(());
    }
    let path = canonicalize_display_path(&root.join(relative)).map_err(|_| ())?;
    path_is_within(&path, root).then_some(path).ok_or(())
}

fn browser_display_url(url: &Url, allowed_file_root: Option<&Path>) -> Url {
    let Some(root) = allowed_file_root else {
        return url.clone();
    };
    browser_local_path(url, root)
        .ok()
        .and_then(|path| Url::from_file_path(path).ok())
        .unwrap_or_else(|| url.clone())
}

pub fn browser_local_file_protocol_response(
    app: &AppHandle,
    webview_label: &str,
    method: &Method,
    uri: &str,
) -> Response<Vec<u8>> {
    let Some(root) = allowed_root_for_label(app, webview_label) else {
        return browser_protocol_error(StatusCode::NOT_FOUND);
    };
    browser_local_file_response(&root, method, uri)
}

pub(crate) fn browser_local_file_response(
    root: &Path,
    method: &Method,
    uri: &str,
) -> Response<Vec<u8>> {
    if method != Method::GET && method != Method::HEAD {
        return browser_protocol_error(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Ok(url) = Url::parse(uri) else {
        return browser_protocol_error(StatusCode::BAD_REQUEST);
    };
    let Ok(path) = browser_local_path(&url, root) else {
        return browser_protocol_error(StatusCode::NOT_FOUND);
    };
    let Ok(metadata) = std::fs::metadata(&path) else {
        return browser_protocol_error(StatusCode::NOT_FOUND);
    };
    if !metadata.is_file() || metadata.len() > BROWSER_LOCAL_FILE_MAX_BYTES {
        return browser_protocol_error(StatusCode::NOT_FOUND);
    }
    let body = if method == Method::HEAD {
        Vec::new()
    } else {
        match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => return browser_protocol_error(StatusCode::NOT_FOUND),
        }
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .header(header::CONTENT_LENGTH, metadata.len())
        .body(body)
        .unwrap_or_else(|_| browser_protocol_error(StatusCode::INTERNAL_SERVER_ERROR))
}

fn browser_protocol_error(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .body(Vec::new())
        .expect("static browser protocol response")
}

fn allowed_root_for_label(app: &AppHandle, label: &str) -> Option<PathBuf> {
    let host = app.try_state::<BrowserHost>()?;
    let inner = lock_host(host.inner()).ok()?;
    inner
        .pages
        .values()
        .find(|page| page.label == label)
        .and_then(|page| page.allowed_file_root.clone())
}

pub(crate) fn navigation_allowed(url: &Url, allowed_file_root: Option<&Path>) -> bool {
    if is_browser_local_file_url(url) {
        let Some(root) = allowed_file_root else {
            return false;
        };
        return browser_local_path(url, root).is_ok();
    }
    if is_privileged_url(url) {
        return false;
    }
    match url.scheme() {
        "https" | "http" | "blob" => true,
        "about" => url.as_str().eq_ignore_ascii_case("about:blank"),
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
    if value.contains("://") {
        // `file://` links must go through `Url` so percent-encoding and the
        // `file:///C:/...` authority form are parsed instead of being read as a literal
        // filesystem path. Treating the raw string as a path made every address-bar
        // `file://` HTML link fail with `browser.local_html.grant_failed` before any
        // native create/navigate ran.
        return false;
    }
    let path = Path::new(value);
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
        page.last_location = Some(url.to_string());
        page.location_probe = page.location_probe.wrapping_add(1);
    }
}

pub(crate) fn begin_location_probe(app: &AppHandle, page_id: &str) -> Option<u64> {
    let host = app.try_state::<BrowserHost>()?;
    let mut inner = lock_host(&host).ok()?;
    let page = inner.pages.get_mut(page_id)?;
    page.location_probe = page.location_probe.wrapping_add(1);
    Some(page.location_probe)
}

pub(crate) fn log_document_location_failure(label: &str, error: &dyn std::fmt::Display) {
    log_webview_failure("watch-document-location", None, label, error);
}

/// Project a top-level location change that did not create a new document.
/// `new_document` is the engine's own signal: WebView2 `SourceChanged.IsNewDocument`,
/// or "the view is in a full document load" on engines that only expose the current URI.
pub(crate) fn publish_same_document_location(
    app: &AppHandle,
    page_id: &str,
    raw_url: &str,
    new_document: bool,
    expected_probe: Option<u64>,
) {
    let Ok(parsed) = Url::parse(raw_url.trim()) else {
        return;
    };
    let location_url = current_allowed_root(app, page_id)
        .as_deref()
        .map(|root| browser_display_url(&parsed, Some(root)))
        .unwrap_or(parsed);
    let location_text = location_url.to_string();
    let decision = {
        let Some(host) = app.try_state::<BrowserHost>() else {
            return;
        };
        let Ok(mut inner) = lock_host(&host) else {
            return;
        };
        let Some(page) = inner.pages.get_mut(page_id) else {
            return;
        };
        if expected_probe.is_some_and(|probe| page.location_probe != probe) {
            return;
        }
        let decision = crate::browser_location::decide_document_location(
            new_document,
            &location_text,
            page.last_location.as_deref(),
            page.last_http_url.as_deref(),
        );
        if decision.publish_url.is_some() {
            page.last_location = decision.publish_url.clone();
            page.last_http_url = crate::browser_history::http_visit_url(&location_url);
        }
        decision
    };
    if decision.record_visit {
        crate::browser_history::record_finished_url(app, &location_url);
    }
    let Some(url) = decision.publish_url else {
        return;
    };
    info!(
        target: "gold_band::browser",
        operation = "document-location",
        page_id,
        target_url = %browser_log_target(&location_url),
        "browser webview same-document location"
    );
    emit_page_event(
        app,
        BrowserPageEventVm {
            kind: "url".into(),
            page_id: page_id.to_string(),
            url: Some(url),
            title: None,
        },
    );
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

fn clear_allowed_root(app: &AppHandle, page_id: &str) {
    let Some(host) = app.try_state::<BrowserHost>() else {
        return;
    };
    let Ok(mut inner) = lock_host(host.inner()) else {
        return;
    };
    if let Some(page) = inner.pages.get_mut(page_id) {
        page.allowed_file_root = None;
    }
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
            let _ = tx.send(crate::browser_ua::set_user_agent(
                &platform,
                &next_user_agent,
            ));
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
    crate::browser_location::detach_document_location_watch(app, label);
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
    error: &dyn std::fmt::Display,
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

fn address_suggestions_invalid() -> CommandErrorVm {
    CommandErrorVm::new("browser.address_suggestions.invalid", serde_json::json!({}))
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
        assert_eq!(target.url.scheme(), "file");
        assert!(is_browser_local_file_url(&target.navigation_url));
        let root = target.allowed_file_root.as_deref();
        assert!(navigation_allowed(
            &browser_navigation_url(&Url::from_file_path(&sibling).unwrap(), root).unwrap(),
            root
        ));
        assert!(!navigation_allowed(
            &Url::from_file_path(&outside).unwrap(),
            root
        ));
    }

    #[test]
    fn local_html_protocol_path_cannot_escape_the_authorized_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonicalize_display_path(dir.path()).unwrap();
        std::fs::write(root.join("index.html"), "<html></html>").unwrap();

        for raw in [
            // Encoded separators and backslashes survive URL parsing, so the
            // authorized-directory guard is what rejects them.
            format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/..%5Csecret.txt"),
            format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/%2Fsecret.txt"),
            format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/nested%2F..%2F..%2Fsecret.txt"),
            format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/"),
        ] {
            let url = Url::parse(&raw).unwrap();
            assert!(
                browser_local_path(&url, &root).is_err(),
                "should reject {raw}"
            );
            assert!(
                !navigation_allowed(&url, Some(&root)),
                "should reject {raw}"
            );
        }

        // WHATWG URL parsing resolves raw and percent-encoded dot segments before the
        // protocol handler sees them, so the result stays inside the authorized
        // directory instead of escaping it.
        for raw in [
            format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/../index.html"),
            format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/%2e%2e/index.html"),
        ] {
            let normalized = Url::parse(&raw).unwrap();
            assert_eq!(normalized.path(), "/index.html");
            assert_eq!(
                browser_local_path(&normalized, &root).unwrap(),
                root.join("index.html")
            );
        }

        let allowed = Url::parse(&format!(
            "{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/index.html"
        ))
        .unwrap();
        assert_eq!(
            browser_local_path(&allowed, &root).unwrap(),
            root.join("index.html")
        );
        assert!(navigation_allowed(&allowed, Some(&root)));
        assert!(!navigation_allowed(&allowed, None));
    }

    #[test]
    fn local_html_navigation_uses_a_webview_loadable_scheme() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("index.html");
        std::fs::write(&page, "<html></html>").unwrap();

        let target = resolve_browser_target(page.to_str().unwrap(), None).unwrap();

        assert!(is_browser_local_file_url(&target.navigation_url));
        assert_eq!(target.navigation_url.path(), "/index.html");
        assert_eq!(
            browser_display_url(&target.navigation_url, target.allowed_file_root.as_deref()),
            target.url
        );
    }

    #[test]
    fn renavigating_an_authorized_local_page_keeps_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonicalize_display_path(dir.path()).unwrap();
        std::fs::write(root.join("index.html"), "<html></html>").unwrap();

        let typed = resolve_browser_target(
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/index.html"),
            Some(&root),
        )
        .unwrap();
        assert_eq!(typed.allowed_file_root.as_deref(), Some(root.as_path()));
        assert!(is_browser_local_file_url(&typed.navigation_url));
        assert_eq!(typed.navigation_url.path(), "/index.html");

        let ungranted = resolve_browser_target(
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/index.html"),
            None,
        );
        assert!(ungranted.is_err());
    }

    #[test]
    fn local_html_protocol_serves_only_authorized_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonicalize_display_path(dir.path()).unwrap();
        std::fs::write(root.join("index.html"), "<html><body>pelican</body></html>").unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/app.js"), "console.log(1)").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "nope").unwrap();

        let page = browser_local_file_response(
            &root,
            &Method::GET,
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/index.html"),
        );
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(
            page.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
        assert_eq!(page.body(), b"<html><body>pelican</body></html>");

        let asset = browser_local_file_response(
            &root,
            &Method::GET,
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/assets/app.js"),
        );
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(
            asset.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/javascript"
        );

        let head = browser_local_file_response(
            &root,
            &Method::HEAD,
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/index.html"),
        );
        assert_eq!(head.status(), StatusCode::OK);
        assert!(head.body().is_empty());
        assert_eq!(head.headers().get(header::CONTENT_LENGTH).unwrap(), "33");

        let escaped = browser_local_file_response(
            &root,
            &Method::GET,
            &format!(
                "{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/{}",
                outside
                    .path()
                    .join("secret.txt")
                    .display()
                    .to_string()
                    .replace('\\', "/")
            ),
        );
        assert_eq!(escaped.status(), StatusCode::NOT_FOUND);

        let missing = browser_local_file_response(
            &root,
            &Method::GET,
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/missing.html"),
        );
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let wrong_method = browser_local_file_response(
            &root,
            &Method::POST,
            &format!("{BROWSER_LOCAL_FILE_PROTOCOL}://localhost/index.html"),
        );
        assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn address_bar_https_is_normalized() {
        let target = resolve_browser_target("example.com/path", None).unwrap();
        assert_eq!(target.url.as_str(), "https://example.com/path");
    }

    #[test]
    fn address_bar_file_url_resolves_to_the_real_html_file() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("pelican-bike.html");
        std::fs::write(&page, "<html></html>").unwrap();
        let raw = Url::from_file_path(canonicalize_display_path(&page).unwrap()).unwrap();

        let target = resolve_browser_target(raw.as_str(), None).unwrap();

        assert_eq!(target.url, raw);
        assert!(is_browser_local_file_url(&target.navigation_url));
        assert_eq!(target.navigation_url.path(), "/pelican-bike.html");
        assert!(target.allowed_file_root.is_some());
    }

    #[test]
    fn address_bar_file_url_still_rejects_non_html_targets() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("app.js");
        std::fs::write(&script, "console.log(1)").unwrap();
        let raw = Url::from_file_path(canonicalize_display_path(&script).unwrap()).unwrap();

        assert!(resolve_browser_target(raw.as_str(), None).is_err());

        let missing = dir.path().join("missing.html");
        let raw = Url::from_file_path(canonicalize_display_path(&missing).unwrap()).unwrap();
        assert!(resolve_browser_target(raw.as_str(), None).is_err());
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
    fn native_registry_evicts_the_least_recently_used_page() {
        let mut inner = BrowserHostInner::default();
        for page_id in ["page-1", "page-2", "page-3", "page-4", "page-5"] {
            inner.insert(
                page_id.to_string(),
                BrowserNativePage {
                    label: format!("gb-b-{page_id}"),
                    allowed_file_root: None,
                    view_mode: BrowserViewMode::Desktop,
                    last_http_url: None,
                    last_location: None,
                    location_probe: 0,
                },
            );
        }

        inner.touch("page-1");

        assert_eq!(
            inner.eviction_candidate("page-5").as_deref(),
            Some("page-2")
        );
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

    fn address_suggestion_overlay(item_count: usize) -> BrowserAddressSuggestionOverlayInput {
        BrowserAddressSuggestionOverlayInput {
            revision: 1,
            bounds: BrowserBoundsVm {
                x: 100.0,
                y: 80.0,
                width: 500.0,
                height: 264.0,
            },
            active_index: None,
            items: (0..item_count)
                .map(|index| BrowserAddressSuggestionItemVm {
                    key: format!("visit:https://example.com/{index}"),
                    kind: "visit".into(),
                    title: format!("Example {index}"),
                    detail: format!("example.com/{index}"),
                    favicon_data_url: None,
                    remove_label: Some("Remove".into()),
                })
                .collect(),
            theme: BrowserAddressSuggestionThemeVm {
                dark: false,
                theme_id: "builtin.gold-band".into(),
                color_scheme: "light".into(),
                visual_quality: "full".into(),
                material_model: "solid".into(),
                variables: HashMap::from([("--popover".into(), "oklch(1 0 0)".into())]),
            },
        }
    }

    #[test]
    fn address_suggestion_overlay_accepts_only_the_bounded_projection() {
        assert!(validate_address_suggestion_overlay(&address_suggestion_overlay(9)).is_ok());
        assert!(validate_address_suggestion_overlay(&address_suggestion_overlay(10)).is_err());

        let mut invalid_active = address_suggestion_overlay(1);
        invalid_active.active_index = Some(1);
        assert!(validate_address_suggestion_overlay(&invalid_active).is_err());
    }

    #[test]
    fn stale_address_suggestion_revision_cannot_reopen_a_hidden_overlay() {
        let mut inner = BrowserHostInner::default();
        assert!(update_address_suggestion_projection(
            &mut inner,
            2,
            Some("visible".into()),
        ));
        invalidate_address_suggestion_projection(&mut inner);
        assert!(!update_address_suggestion_projection(
            &mut inner,
            2,
            Some("stale".into()),
        ));
        assert_eq!(inner.address_suggestion_revision, 3);
        assert!(inner.address_suggestion_state_json.is_none());
    }

    #[test]
    fn overlapped_address_suggestion_requests_converge_to_the_newest_projection() {
        let mut inner = BrowserHostInner::default();

        // Nothing requested yet: an overlapped request must not invent an overlay.
        assert!(address_suggestion_overlay_target(&inner).is_none());

        // A newer show wins: the older in-flight request must show the newest payload
        // instead of hiding the overlay the newer revision just displayed.
        assert!(update_address_suggestion_projection(
            &mut inner,
            4,
            Some("older".into()),
        ));
        assert!(update_address_suggestion_projection(
            &mut inner,
            5,
            Some("newest".into()),
        ));
        assert_eq!(
            address_suggestion_overlay_target(&inner).as_deref(),
            Some("newest")
        );

        // A newer hide wins: the overlapped request must converge to hidden.
        assert!(update_address_suggestion_projection(&mut inner, 6, None));
        assert!(address_suggestion_overlay_target(&inner).is_none());
    }

    #[test]
    fn only_the_current_visible_address_suggestion_revision_can_become_ready() {
        let mut inner = BrowserHostInner::default();
        assert!(update_address_suggestion_projection(
            &mut inner,
            7,
            Some("visible".into()),
        ));

        assert!(address_suggestion_revision_is_ready(&inner, 7));
        assert!(!address_suggestion_revision_is_ready(&inner, 6));
        assert!(!address_suggestion_revision_is_ready(&inner, 8));

        assert!(update_address_suggestion_projection(&mut inner, 8, None));
        assert!(!address_suggestion_revision_is_ready(&inner, 7));
        assert!(!address_suggestion_revision_is_ready(&inner, 8));
    }

    #[test]
    fn address_suggestion_actions_are_validated_before_forwarding() {
        let action = |revision: u64, kind: &str, key: &str| BrowserAddressSuggestionActionVm {
            revision,
            kind: kind.to_string(),
            key: key.to_string(),
        };

        assert!(
            validate_address_suggestion_action(&action(1, "choose", "visit:https://example.com/"))
                .is_ok()
        );
        assert!(
            validate_address_suggestion_action(&action(1, "remove", "visit:https://a/")).is_ok()
        );
        assert!(validate_address_suggestion_action(&action(1, "dismiss", "page-focus")).is_ok());

        assert!(
            validate_address_suggestion_action(&action(0, "choose", "visit:https://a/")).is_err()
        );
        assert!(
            validate_address_suggestion_action(&action(1, "drop", "visit:https://a/")).is_err()
        );
        assert!(validate_address_suggestion_action(&action(1, "choose", "")).is_err());
        let too_long = "k".repeat(BROWSER_ADDRESS_SUGGESTION_MAX_KEY_BYTES + 1);
        assert!(validate_address_suggestion_action(&action(1, "choose", &too_long)).is_err());
    }

    #[test]
    fn address_suggestion_initialization_applies_the_theme_before_first_paint() {
        let script = address_suggestion_initialization_script(
            r#"{"revision":1}"#,
            r#"{"dark":false,"themeId":"builtin.gold-band"}"#,
        );

        assert!(
            script
                .contains(r#"window.__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__ = {"revision":1};"#)
        );
        assert!(script.contains("dataset.theme"));
        assert!(script.contains("setProperty"));
        assert!(script.contains(r#"{"dark":false,"themeId":"builtin.gold-band"}"#));
    }
}
