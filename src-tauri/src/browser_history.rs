use std::collections::HashSet;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use camino::Utf8Path;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use url::Url;

use crate::commands::{CommandErrorVm, CommandResult};

pub const BROWSER_HISTORY_LIMIT: usize = 200;
pub const BROWSER_HISTORY_EVENT: &str = "gold-band://browser-history";
const HISTORY_FILE_NAME: &str = "history.json";
const FAVICON_DIR_NAME: &str = "favicons";
const FAVICON_PIXELS: u32 = 32;
const FAVICON_MAX_BYTES: usize = 256 * 1024;
const HISTORY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default)]
pub struct BrowserHistoryHost {
    inner: Mutex<BrowserHistoryInner>,
}

#[derive(Debug, Default)]
struct BrowserHistoryInner {
    loaded: bool,
    visits: Vec<BrowserVisitRecord>,
    fetching: HashSet<String>,
    missing: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct BrowserHistoryFile {
    #[serde(default = "history_schema_version")]
    schema_version: u32,
    #[serde(default)]
    visits: Vec<BrowserVisitRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserVisitRecord {
    pub url: String,
    #[serde(default)]
    pub title: String,
    pub last_visited_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserVisitVm {
    pub url: String,
    pub title: String,
    pub origin: String,
    pub last_visited_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon_data_url: Option<String>,
}

fn history_schema_version() -> u32 {
    HISTORY_SCHEMA_VERSION
}

pub fn http_visit_url(url: &Url) -> Option<String> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    let mut canonical = url.clone();
    canonical.set_fragment(None);
    Some(canonical.to_string())
}

pub fn visit_origin(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

pub fn upsert_visit(
    visits: &mut Vec<BrowserVisitRecord>,
    url: String,
    title: Option<&str>,
    now: i64,
) {
    let title = title.map(str::trim).filter(|value| !value.is_empty());
    if let Some(index) = visits.iter().position(|visit| visit.url == url) {
        let mut visit = visits.remove(index);
        visit.last_visited_at = now;
        if let Some(title) = title {
            visit.title = title.to_string();
        }
        visits.insert(0, visit);
    } else {
        visits.insert(
            0,
            BrowserVisitRecord {
                url,
                title: title.unwrap_or("").to_string(),
                last_visited_at: now,
            },
        );
    }
    visits.truncate(BROWSER_HISTORY_LIMIT);
}

pub fn remove_visit(visits: &mut Vec<BrowserVisitRecord>, url: &str) -> bool {
    let Some(index) = visits.iter().position(|visit| visit.url == url) else {
        return false;
    };
    visits.remove(index);
    true
}

fn origin_in_use(visits: &[BrowserVisitRecord], origin: &str) -> bool {
    visits
        .iter()
        .any(|visit| visit_origin(&visit.url).as_deref() == Some(origin))
}

pub fn rasterize_favicon(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > FAVICON_MAX_BYTES {
        return None;
    }
    if let Ok(image) = image::load_from_memory(bytes) {
        return encode_png32(&image);
    }
    rasterize_svg(bytes)
}

fn encode_png32(image: &DynamicImage) -> Option<Vec<u8>> {
    let resized = image.resize_to_fill(FAVICON_PIXELS, FAVICON_PIXELS, FilterType::Triangle);
    let mut encoded = Cursor::new(Vec::new());
    resized.write_to(&mut encoded, ImageFormat::Png).ok()?;
    Some(encoded.into_inner())
}

fn rasterize_svg(bytes: &[u8]) -> Option<Vec<u8>> {
    let trimmed = std::str::from_utf8(bytes).ok()?.trim_start();
    if !trimmed.starts_with('<') {
        return None;
    }
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let longest = size.width().max(size.height()).max(1.0);
    let scale = FAVICON_PIXELS as f32 / longest;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(FAVICON_PIXELS, FAVICON_PIXELS)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn history_file(profile_dir: &Path) -> PathBuf {
    profile_dir.join(HISTORY_FILE_NAME)
}

fn favicon_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join(FAVICON_DIR_NAME)
}

fn favicon_path(profile_dir: &Path, origin: &str) -> PathBuf {
    let hash = blake3::hash(origin.as_bytes()).to_hex();
    favicon_dir(profile_dir).join(format!("{}.png", &hash.as_str()[..16]))
}

fn load_inner(inner: &mut BrowserHistoryInner, profile_dir: &Path) {
    if inner.loaded {
        return;
    }
    inner.loaded = true;
    let path = history_file(profile_dir);
    let Some(utf8) = Utf8Path::from_path(&path) else {
        return;
    };
    match gold_band::storage::read_json::<BrowserHistoryFile>(utf8) {
        Ok(file) => inner.visits = file.visits,
        Err(_) if !path.exists() => {}
        Err(error) => {
            tracing::warn!(
                target: "gold_band::browser",
                operation = "history-load",
                error = %error,
                "browser history file ignored"
            );
        }
    }
}

fn persist_inner(inner: &BrowserHistoryInner, profile_dir: &Path) {
    let path = history_file(profile_dir);
    let Some(utf8) = Utf8Path::from_path(&path) else {
        return;
    };
    let file = BrowserHistoryFile {
        schema_version: HISTORY_SCHEMA_VERSION,
        visits: inner.visits.clone(),
    };
    if let Err(error) = gold_band::storage::write_json(utf8, &file) {
        tracing::warn!(
            target: "gold_band::browser",
            operation = "history-save",
            error = %error,
            "browser history persist failed"
        );
    }
}

fn favicon_data_url(profile_dir: &Path, origin: &str) -> Option<String> {
    let bytes = std::fs::read(favicon_path(profile_dir, origin)).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(format!("data:image/png;base64,{}", BASE64.encode(bytes)))
}

fn record_visit(app: &AppHandle, url: &str, title: Option<&str>) {
    let Some(origin) = visit_origin(url) else {
        return;
    };
    let Some(profile_dir) = history_profile_dir(app) else {
        return;
    };
    let host = app.state::<BrowserHistoryHost>();
    let should_fetch = {
        let Ok(mut inner) = host.inner.lock() else {
            return;
        };
        load_inner(&mut inner, &profile_dir);
        upsert_visit(&mut inner.visits, url.to_string(), title, now_ms());
        persist_inner(&inner, &profile_dir);
        should_start_favicon_fetch(&mut inner, &profile_dir, &origin)
    };
    emit_history_changed(app);
    if should_fetch {
        spawn_favicon_fetch(app.clone(), origin, profile_dir);
    }
}

pub(crate) fn origin_favicon_data_url(profile_dir: &Path, origin: &str) -> Option<String> {
    favicon_data_url(profile_dir, origin)
}

pub(crate) fn profile_dir(app: &AppHandle) -> Option<PathBuf> {
    history_profile_dir(app)
}

pub(crate) fn ensure_origin_favicon(app: &AppHandle, origin: &str) {
    let Some(origin) = visit_origin(origin) else {
        return;
    };
    let Some(profile_dir) = history_profile_dir(app) else {
        return;
    };
    let host = app.state::<BrowserHistoryHost>();
    let should_fetch = {
        let Ok(mut inner) = host.inner.lock() else {
            return;
        };
        should_start_favicon_fetch(&mut inner, &profile_dir, &origin)
    };
    if should_fetch {
        spawn_favicon_fetch(app.clone(), origin, profile_dir);
    }
}

fn should_start_favicon_fetch(
    inner: &mut BrowserHistoryInner,
    profile_dir: &Path,
    origin: &str,
) -> bool {
    let path = favicon_path(profile_dir, origin);
    if path.exists() || inner.missing.contains(origin) || inner.fetching.contains(origin) {
        false
    } else {
        inner.fetching.insert(origin.to_string());
        true
    }
}

pub fn record_finished_url(app: &AppHandle, url: &Url) {
    if let Some(visit) = http_visit_url(url) {
        record_visit(app, &visit, None);
    }
}

pub fn record_title_for_url(app: &AppHandle, url: &str, title: &str) {
    if title.trim().is_empty() || visit_origin(url).is_none() {
        return;
    }
    record_visit(app, url, Some(title));
}

#[tauri::command]
pub async fn browser_list_history(
    app: AppHandle,
    host: State<'_, BrowserHistoryHost>,
) -> CommandResult<Vec<BrowserVisitVm>> {
    let Some(profile_dir) = history_profile_dir(&app) else {
        return Ok(Vec::new());
    };
    let Ok(mut inner) = host.inner.lock() else {
        return Ok(Vec::new());
    };
    load_inner(&mut inner, &profile_dir);
    Ok(visit_vms(&inner, &profile_dir))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDeleteHistoryInput {
    pub url: String,
}

#[tauri::command]
pub async fn browser_delete_history(
    app: AppHandle,
    host: State<'_, BrowserHistoryHost>,
    input: BrowserDeleteHistoryInput,
) -> CommandResult<Vec<BrowserVisitVm>> {
    let Some(url) = Url::parse(&input.url)
        .ok()
        .as_ref()
        .and_then(http_visit_url)
    else {
        return Err(CommandErrorVm::new(
            "browser.history.url_invalid",
            serde_json::json!({}),
        ));
    };
    let Some(profile_dir) = history_profile_dir(&app) else {
        return Ok(Vec::new());
    };
    let Ok(mut inner) = host.inner.lock() else {
        return Err(CommandErrorVm::new(
            "browser.history.unavailable",
            serde_json::json!({}),
        ));
    };
    load_inner(&mut inner, &profile_dir);
    let origin = visit_origin(&url);
    remove_visit(&mut inner.visits, &url);
    persist_inner(&inner, &profile_dir);
    if let Some(origin) = origin.as_deref() {
        if !origin_in_use(&inner.visits, origin) {
            let _ = std::fs::remove_file(favicon_path(&profile_dir, origin));
            inner.missing.remove(origin);
            inner.fetching.remove(origin);
        }
    }
    let visits = visit_vms(&inner, &profile_dir);
    drop(inner);
    emit_history_changed(&app);
    Ok(visits)
}

fn visit_vms(inner: &BrowserHistoryInner, profile_dir: &Path) -> Vec<BrowserVisitVm> {
    inner
        .visits
        .iter()
        .filter_map(|visit| {
            let origin = visit_origin(&visit.url)?;
            Some(BrowserVisitVm {
                url: visit.url.clone(),
                title: visit.title.clone(),
                origin: origin.clone(),
                last_visited_at: visit.last_visited_at,
                favicon_data_url: favicon_data_url(profile_dir, &origin),
            })
        })
        .collect()
}

fn emit_history_changed(app: &AppHandle) {
    let _ = app.emit(BROWSER_HISTORY_EVENT, ());
}

fn history_profile_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("browser-profile");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn spawn_favicon_fetch(app: AppHandle, origin: String, profile_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let png = fetch_origin_favicon(&origin).await;
        let host = app.state::<BrowserHistoryHost>();
        if let Ok(mut inner) = host.inner.lock() {
            inner.fetching.remove(&origin);
            match png {
                Some(bytes) => {
                    let path = favicon_path(&profile_dir, &origin);
                    if save_favicon(&path, &bytes) {
                        inner.missing.remove(&origin);
                        drop(inner);
                        emit_history_changed(&app);
                    } else {
                        inner.missing.insert(origin);
                    }
                }
                None => {
                    inner.missing.insert(origin);
                }
            }
        }
    });
}

fn save_favicon(path: &Path, bytes: &[u8]) -> bool {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    gold_band::storage::atomic_write_file(path, |file| file.write_all(bytes)).is_ok()
}

async fn fetch_origin_favicon(origin: &str) -> Option<Vec<u8>> {
    let origin_url = Url::parse(origin).ok()?;
    let candidates = [
        origin_url.join("/favicon.ico").ok()?,
        origin_url.join("/favicon.png").ok()?,
        origin_url.join("/favicon.svg").ok()?,
    ];
    for candidate in candidates {
        if let Some(png) = fetch_favicon_url(candidate).await {
            return Some(png);
        }
    }
    None
}

async fn fetch_favicon_url(url: Url) -> Option<Vec<u8>> {
    let response = favicon_client().get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    rasterize_favicon(&bytes)
}

fn favicon_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::limited(4))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn http_visits_drop_hash_and_ignore_blank() {
        let github = Url::parse("https://github.com/gold-band#section").unwrap();
        assert_eq!(
            http_visit_url(&github).as_deref(),
            Some("https://github.com/gold-band")
        );
        assert_eq!(
            visit_origin("https://github.com/gold-band"),
            Some("https://github.com".into())
        );
        assert!(http_visit_url(&Url::parse("about:blank").unwrap()).is_none());
        assert!(visit_origin("file:///tmp/index.html").is_none());
    }

    #[test]
    fn upsert_moves_existing_url_to_front_and_caps_history() {
        let mut visits = Vec::new();
        upsert_visit(&mut visits, "https://a.example/".into(), Some("A"), 1);
        upsert_visit(&mut visits, "https://b.example/".into(), Some("B"), 2);
        upsert_visit(&mut visits, "https://a.example/".into(), Some("A2"), 3);
        assert_eq!(visits[0].url, "https://a.example/");
        assert_eq!(visits[0].title, "A2");
        assert_eq!(visits.len(), 2);
        for index in 0..BROWSER_HISTORY_LIMIT + 5 {
            upsert_visit(
                &mut visits,
                format!("https://n{index}.example/"),
                None,
                10 + index as i64,
            );
        }
        assert_eq!(visits.len(), BROWSER_HISTORY_LIMIT);
        assert!(visits[0].url.ends_with("n204.example/"));
    }

    #[test]
    fn remove_visit_drops_only_that_url_and_is_idempotent() {
        let mut visits = Vec::new();
        upsert_visit(&mut visits, "https://a.example/".into(), Some("A"), 1);
        upsert_visit(&mut visits, "https://b.example/".into(), Some("B"), 2);
        assert!(remove_visit(&mut visits, "https://a.example/"));
        assert_eq!(visits.len(), 1);
        assert_eq!(visits[0].url, "https://b.example/");
        assert!(!remove_visit(&mut visits, "https://a.example/"));
        assert!(origin_in_use(&visits, "https://b.example"));
        assert!(!origin_in_use(&visits, "https://a.example"));
    }

    #[test]
    fn png_and_svg_favicons_rasterize_to_png() {
        let png = rasterize_favicon(MINIMAL_PNG).expect("png");
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="black"/></svg>"#;
        let from_svg = rasterize_favicon(svg).expect("svg");
        assert!(from_svg.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(rasterize_favicon(b"not-an-image").is_none());
    }

    #[test]
    fn favicon_fetch_gate_skips_cached_and_inflight_origins() {
        let dir = std::env::temp_dir().join(format!(
            "gb-favicon-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(favicon_dir(&dir)).unwrap();
        let origin = "https://github.com";
        let mut inner = BrowserHistoryInner::default();
        assert!(should_start_favicon_fetch(&mut inner, &dir, origin));
        assert!(!should_start_favicon_fetch(&mut inner, &dir, origin));
        inner.fetching.clear();
        inner.missing.insert(origin.into());
        assert!(!should_start_favicon_fetch(&mut inner, &dir, origin));
        inner.missing.clear();
        std::fs::write(favicon_path(&dir, origin), MINIMAL_PNG).unwrap();
        assert!(!should_start_favicon_fetch(&mut inner, &dir, origin));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
