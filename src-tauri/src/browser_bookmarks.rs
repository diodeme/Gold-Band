use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use url::Url;

use crate::browser_history::{
    ensure_origin_favicon, origin_favicon_data_url, profile_dir, visit_origin,
};
use crate::channel::current_channel_config;
use crate::commands::{CommandErrorVm, CommandResult};

pub const BROWSER_BOOKMARK_LIMIT: usize = 32;
const BOOKMARKS_FILE_NAME: &str = "bookmarks.json";
const BOOKMARKS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default)]
pub struct BrowserBookmarkHost {
    inner: Mutex<BrowserBookmarkInner>,
}

#[derive(Debug, Default)]
struct BrowserBookmarkInner {
    loaded: bool,
    seeded_channel: Option<String>,
    items: Vec<BrowserBookmarkRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct BrowserBookmarkFile {
    #[serde(default = "bookmarks_schema_version")]
    schema_version: u32,
    #[serde(default)]
    seeded_channel: Option<String>,
    #[serde(default)]
    items: Vec<BrowserBookmarkRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBookmarkRecord {
    pub bookmark_id: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBookmarkVm {
    pub bookmark_id: String,
    pub url: String,
    pub title: String,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon_data_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAddBookmarkInput {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRemoveBookmarkInput {
    pub bookmark_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserReorderBookmarksInput {
    pub ordered_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BuiltinBookmark {
    id: String,
    title: String,
    url: String,
}

fn bookmarks_schema_version() -> u32 {
    BOOKMARKS_SCHEMA_VERSION
}

pub fn bookmark_url(raw: &str) -> Option<String> {
    let origin = visit_origin(raw)?;
    Some(format!("{origin}/"))
}

pub fn bookmark_title(title: Option<&str>, url: &str) -> String {
    let trimmed = title.map(str::trim).filter(|value| !value.is_empty());
    if let Some(title) = trimmed {
        return title.to_string();
    }
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

pub fn add_bookmark(
    items: &mut Vec<BrowserBookmarkRecord>,
    url: String,
    title: String,
    bookmark_id: String,
) -> Result<String, CommandErrorVm> {
    let Some(canonical) = bookmark_url(&url) else {
        return Err(bookmark_error("browser.bookmark.invalid"));
    };
    let origin =
        visit_origin(&canonical).ok_or_else(|| bookmark_error("browser.bookmark.invalid"))?;
    if let Some(existing) = items
        .iter()
        .find(|item| visit_origin(&item.url).as_deref() == Some(origin.as_str()))
    {
        return Ok(existing.bookmark_id.clone());
    }
    if items.len() >= BROWSER_BOOKMARK_LIMIT {
        return Err(bookmark_error("browser.bookmark.limit_reached"));
    }
    items.push(BrowserBookmarkRecord {
        bookmark_id: bookmark_id.clone(),
        url: canonical,
        title,
    });
    Ok(bookmark_id)
}

pub fn remove_bookmark(items: &mut Vec<BrowserBookmarkRecord>, bookmark_id: &str) -> bool {
    let before = items.len();
    items.retain(|item| item.bookmark_id != bookmark_id);
    items.len() != before
}

pub fn remove_bookmark_by_origin(items: &mut Vec<BrowserBookmarkRecord>, origin: &str) -> bool {
    let before = items.len();
    items.retain(|item| visit_origin(&item.url).as_deref() != Some(origin));
    items.len() != before
}

pub fn reorder_bookmarks(
    items: &mut Vec<BrowserBookmarkRecord>,
    ordered_ids: &[String],
) -> Result<(), CommandErrorVm> {
    if ordered_ids.len() != items.len() {
        return Err(bookmark_error("browser.bookmark.invalid"));
    }
    let mut remaining: HashMap<String, BrowserBookmarkRecord> = items
        .iter()
        .cloned()
        .map(|item| (item.bookmark_id.clone(), item))
        .collect();
    if remaining.len() != items.len() {
        return Err(bookmark_error("browser.bookmark.invalid"));
    }
    let mut next = Vec::with_capacity(ordered_ids.len());
    for id in ordered_ids {
        let Some(item) = remaining.remove(id) else {
            return Err(bookmark_error("browser.bookmark.invalid"));
        };
        next.push(item);
    }
    if !remaining.is_empty() {
        return Err(bookmark_error("browser.bookmark.invalid"));
    }
    *items = next;
    Ok(())
}

pub fn seed_bookmarks(items: &mut Vec<BrowserBookmarkRecord>, catalog: &[BuiltinBookmark]) {
    for builtin in catalog {
        let _ = add_bookmark(
            items,
            builtin.url.clone(),
            builtin.title.clone(),
            builtin.id.clone(),
        );
    }
}

fn bookmark_error(code: &str) -> CommandErrorVm {
    CommandErrorVm::new(code, serde_json::json!({}))
}

fn builtin_catalog() -> Vec<BuiltinBookmark> {
    serde_json::from_str(option_env!("GOLD_BAND_BROWSER_BOOKMARKS").unwrap_or("[]"))
        .unwrap_or_default()
}

fn bookmarks_file(profile_dir: &Path) -> PathBuf {
    profile_dir.join(BOOKMARKS_FILE_NAME)
}

fn load_inner(inner: &mut BrowserBookmarkInner, profile_dir: &Path) {
    if inner.loaded {
        return;
    }
    inner.loaded = true;
    let path = bookmarks_file(profile_dir);
    let Some(utf8) = Utf8Path::from_path(&path) else {
        return;
    };
    match gold_band::storage::read_json::<BrowserBookmarkFile>(utf8) {
        Ok(file) => {
            inner.seeded_channel = file.seeded_channel;
            inner.items = file.items;
        }
        Err(_) if !path.exists() => {}
        Err(error) => {
            tracing::warn!(
                target: "gold_band::browser",
                operation = "bookmark-load",
                error = %error,
                "browser bookmarks file ignored"
            );
        }
    }
}

fn persist_inner(inner: &BrowserBookmarkInner, profile_dir: &Path) {
    let path = bookmarks_file(profile_dir);
    let Some(utf8) = Utf8Path::from_path(&path) else {
        return;
    };
    let file = BrowserBookmarkFile {
        schema_version: BOOKMARKS_SCHEMA_VERSION,
        seeded_channel: inner.seeded_channel.clone(),
        items: inner.items.clone(),
    };
    if let Err(error) = gold_band::storage::write_json(utf8, &file) {
        tracing::warn!(
            target: "gold_band::browser",
            operation = "bookmark-save",
            error = %error,
            "browser bookmarks persist failed"
        );
    }
}

fn current_channel() -> String {
    current_channel_config().channel.to_string()
}

fn ensure_seeded(inner: &mut BrowserBookmarkInner, profile_dir: &Path) {
    if inner.seeded_channel.is_some() {
        return;
    }
    seed_bookmarks(&mut inner.items, &builtin_catalog());
    inner.seeded_channel = Some(current_channel());
    persist_inner(inner, profile_dir);
}

fn project_vms(
    app: &AppHandle,
    items: &[BrowserBookmarkRecord],
    profile_dir: &Path,
) -> Vec<BrowserBookmarkVm> {
    items
        .iter()
        .filter_map(|item| {
            let origin = visit_origin(&item.url)?;
            ensure_origin_favicon(app, &origin);
            Some(BrowserBookmarkVm {
                bookmark_id: item.bookmark_id.clone(),
                url: item.url.clone(),
                title: item.title.clone(),
                origin: origin.clone(),
                favicon_data_url: origin_favicon_data_url(profile_dir, &origin),
            })
        })
        .collect()
}

fn lock_host(
    host: &BrowserBookmarkHost,
) -> CommandResult<std::sync::MutexGuard<'_, BrowserBookmarkInner>> {
    host.inner
        .lock()
        .map_err(|_| bookmark_error("browser.webview.unavailable"))
}

#[tauri::command]
pub async fn browser_list_bookmarks(
    app: AppHandle,
    host: State<'_, BrowserBookmarkHost>,
) -> CommandResult<Vec<BrowserBookmarkVm>> {
    let Some(profile_dir) = profile_dir(&app) else {
        return Ok(Vec::new());
    };
    let mut inner = lock_host(&host)?;
    load_inner(&mut inner, &profile_dir);
    ensure_seeded(&mut inner, &profile_dir);
    Ok(project_vms(&app, &inner.items, &profile_dir))
}

#[tauri::command]
pub async fn browser_add_bookmark(
    app: AppHandle,
    host: State<'_, BrowserBookmarkHost>,
    input: BrowserAddBookmarkInput,
) -> CommandResult<Vec<BrowserBookmarkVm>> {
    let Some(profile_dir) = profile_dir(&app) else {
        return Err(bookmark_error("browser.webview.unavailable"));
    };
    let mut inner = lock_host(&host)?;
    load_inner(&mut inner, &profile_dir);
    ensure_seeded(&mut inner, &profile_dir);
    let canonical =
        bookmark_url(&input.url).ok_or_else(|| bookmark_error("browser.bookmark.invalid"))?;
    let title = bookmark_title(input.title.as_deref(), &canonical);
    let bookmark_id = uuid_id();
    add_bookmark(&mut inner.items, canonical, title, bookmark_id)?;
    persist_inner(&inner, &profile_dir);
    Ok(project_vms(&app, &inner.items, &profile_dir))
}

#[tauri::command]
pub async fn browser_remove_bookmark(
    app: AppHandle,
    host: State<'_, BrowserBookmarkHost>,
    input: BrowserRemoveBookmarkInput,
) -> CommandResult<Vec<BrowserBookmarkVm>> {
    let Some(profile_dir) = profile_dir(&app) else {
        return Err(bookmark_error("browser.webview.unavailable"));
    };
    let mut inner = lock_host(&host)?;
    load_inner(&mut inner, &profile_dir);
    ensure_seeded(&mut inner, &profile_dir);
    remove_bookmark(&mut inner.items, &input.bookmark_id);
    persist_inner(&inner, &profile_dir);
    Ok(project_vms(&app, &inner.items, &profile_dir))
}

#[tauri::command]
pub async fn browser_reorder_bookmarks(
    app: AppHandle,
    host: State<'_, BrowserBookmarkHost>,
    input: BrowserReorderBookmarksInput,
) -> CommandResult<Vec<BrowserBookmarkVm>> {
    let Some(profile_dir) = profile_dir(&app) else {
        return Err(bookmark_error("browser.webview.unavailable"));
    };
    let mut inner = lock_host(&host)?;
    load_inner(&mut inner, &profile_dir);
    ensure_seeded(&mut inner, &profile_dir);
    reorder_bookmarks(&mut inner.items, &input.ordered_ids)?;
    persist_inner(&inner, &profile_dir);
    Ok(project_vms(&app, &inner.items, &profile_dir))
}

fn uuid_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, url: &str, title: &str) -> BrowserBookmarkRecord {
        BrowserBookmarkRecord {
            bookmark_id: id.into(),
            url: url.into(),
            title: title.into(),
        }
    }

    #[test]
    fn bookmark_url_uses_origin() {
        assert_eq!(
            bookmark_url("https://github.com/gold-band#hash").as_deref(),
            Some("https://github.com/")
        );
        assert!(bookmark_url("about:blank").is_none());
        assert!(bookmark_url("file:///tmp/index.html").is_none());
    }

    #[test]
    fn add_is_idempotent_per_origin_and_respects_limit() {
        let mut items = Vec::new();
        assert_eq!(
            add_bookmark(
                &mut items,
                "https://github.com/foo".into(),
                "GitHub".into(),
                "github".into()
            )
            .unwrap(),
            "github"
        );
        assert_eq!(
            add_bookmark(
                &mut items,
                "https://github.com/bar".into(),
                "Other".into(),
                "other".into()
            )
            .unwrap(),
            "github"
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "GitHub");
        for index in 0..BROWSER_BOOKMARK_LIMIT {
            let _ = add_bookmark(
                &mut items,
                format!("https://n{index}.example/"),
                format!("N{index}"),
                format!("n{index}"),
            );
        }
        assert_eq!(items.len(), BROWSER_BOOKMARK_LIMIT);
        assert_eq!(
            add_bookmark(
                &mut items,
                "https://overflow.example/".into(),
                "X".into(),
                "x".into()
            )
            .unwrap_err()
            .code,
            "browser.bookmark.limit_reached"
        );
    }

    #[test]
    fn remove_and_reorder_keep_stable_ids() {
        let mut items = vec![
            item("a", "https://a.example/", "A"),
            item("b", "https://b.example/", "B"),
            item("c", "https://c.example/", "C"),
        ];
        assert!(remove_bookmark(&mut items, "b"));
        assert_eq!(
            items
                .iter()
                .map(|item| item.bookmark_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        reorder_bookmarks(&mut items, &["c".into(), "a".into()]).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.bookmark_id.as_str())
                .collect::<Vec<_>>(),
            ["c", "a"]
        );
        assert!(remove_bookmark_by_origin(&mut items, "https://c.example"));
        assert_eq!(items[0].bookmark_id, "a");
        assert_eq!(
            reorder_bookmarks(&mut items, &["c".into()])
                .unwrap_err()
                .code,
            "browser.bookmark.invalid"
        );
    }

    #[test]
    fn seed_copies_catalog_once_per_origin() {
        let mut items = Vec::new();
        let catalog = vec![
            BuiltinBookmark {
                id: "github".into(),
                title: "GitHub".into(),
                url: "https://github.com/".into(),
            },
            BuiltinBookmark {
                id: "figma".into(),
                title: "Figma".into(),
                url: "https://www.figma.com/".into(),
            },
        ];
        seed_bookmarks(&mut items, &catalog);
        seed_bookmarks(&mut items, &catalog);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].bookmark_id, "github");
        assert_eq!(items[1].bookmark_id, "figma");
    }

    #[test]
    fn catalog_json_has_default_and_wb_channels() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../configs/browser-bookmarks.json")).unwrap();
        let default = catalog["default"].as_array().expect("default");
        assert!(default.iter().any(|item| item["id"] == "github"));
        assert!(default.iter().any(|item| item["id"] == "figma"));
        let wb = catalog["wb"].as_array().expect("wb");
        assert_eq!(wb.len(), 7);
        assert!(
            wb.iter()
                .any(|item| item["id"] == "maling" && item["url"] == "http://maling.weoa.com/")
        );
        assert!(wb.iter().any(|item| item["id"] == "cmdb"));
    }
}
