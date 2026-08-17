use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use gold_band::config::{PersonalizationPreference, ResolvedColorScheme, WallpaperImagePreference};
use gold_band::storage::{read_json, write_json};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::http::{Response, StatusCode, header};
use uuid::Uuid;

pub const WALLPAPER_ASSET_PROTOCOL: &str = "gold-band-wallpaper";

const WALLPAPER_STORE_VERSION: u32 = 1;
const MAX_RECENT_WALLPAPERS: usize = 10;
const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_NORMALIZED_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 4096;
const MAX_IMAGE_PIXELS: u64 = 16_000_000;
const THUMBNAIL_WIDTH: u32 = 320;
const THUMBNAIL_HEIGHT: u32 = 180;
const JPEG_QUALITY: u8 = 88;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDesktopWallpaperInput {
    pub source_path: String,
    pub color_scheme: ResolvedColorScheme,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectRecentDesktopWallpaperInput {
    pub wallpaper_id: String,
    pub color_scheme: ResolvedColorScheme,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDesktopWallpaperOpacityInput {
    pub opacity_percent: u8,
    pub color_scheme: ResolvedColorScheme,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreThemeDesktopWallpaperInput {
    pub color_scheme: ResolvedColorScheme,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperImageVm {
    pub id: String,
    pub image_url: String,
    pub thumbnail_url: String,
    pub created_at: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperPreferencesVm {
    pub recent_wallpapers: Vec<WallpaperImageVm>,
}

#[derive(Debug, Clone)]
pub struct WallpaperError {
    pub code: &'static str,
    pub params: Value,
}

impl WallpaperError {
    fn new(code: &'static str, params: Value) -> Self {
        Self { code, params }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WallpaperRecord {
    id: String,
    image_file_name: String,
    thumbnail_file_name: String,
    image_mime_type: String,
    thumbnail_mime_type: String,
    created_at: String,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WallpaperStore {
    version: u32,
    #[serde(default)]
    recent_wallpapers: Vec<WallpaperRecord>,
}

impl Default for WallpaperStore {
    fn default() -> Self {
        Self {
            version: WALLPAPER_STORE_VERSION,
            recent_wallpapers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SavedWallpaper {
    pub asset_id: String,
}

struct NormalizedWallpaper {
    raster: DynamicImage,
    bytes: Vec<u8>,
    extension: &'static str,
    mime_type: &'static str,
}

#[derive(Debug, Clone)]
pub struct WallpaperProtocolRuntime {
    root: Utf8PathBuf,
}

impl WallpaperProtocolRuntime {
    pub fn new(root: Utf8PathBuf) -> Self {
        Self { root }
    }

    pub fn protocol_response(&self, request_path: &str) -> Response<Vec<u8>> {
        protocol_response(&self.root, request_path)
    }
}

pub fn load_resolved_wallpaper_preferences(
    root: &Utf8Path,
) -> Result<WallpaperPreferencesVm, WallpaperError> {
    let store = load_store(root)?;
    Ok(wallpaper_preferences_vm(root, &store))
}

pub fn reconcile_wallpaper_personalization(
    root: &Utf8Path,
    personalization: &mut PersonalizationPreference,
) -> Result<bool, WallpaperError> {
    let store = load_store(root)?;
    let mut changed = false;
    for color_scheme in [ResolvedColorScheme::Light, ResolvedColorScheme::Dark] {
        let wallpaper = personalization.wallpaper.for_color_scheme_mut(color_scheme);
        let WallpaperImagePreference::User { asset_id } = &wallpaper.image else {
            continue;
        };
        let exists = store
            .recent_wallpapers
            .iter()
            .any(|record| record.id == *asset_id && record_is_valid(root, record));
        if !exists {
            wallpaper.image = WallpaperImagePreference::Theme;
            changed = true;
        }
    }
    Ok(changed)
}

pub fn import_wallpaper_image(
    root: &Utf8Path,
    input: ImportDesktopWallpaperInput,
    retained_asset_ids: &HashSet<String>,
) -> Result<SavedWallpaper, WallpaperError> {
    let source = canonical_source_path(&input.source_path)?;
    let metadata = fs::metadata(source.as_std_path())
        .map_err(|_| WallpaperError::new("wallpaper.source-unavailable", serde_json::json!({})))?;
    if !metadata.is_file() {
        return Err(WallpaperError::new(
            "wallpaper.source-unavailable",
            serde_json::json!({}),
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_SOURCE_BYTES {
        return Err(WallpaperError::new(
            "wallpaper.source-too-large",
            serde_json::json!({ "maxBytes": MAX_SOURCE_BYTES }),
        ));
    }

    let (format, width, height) = inspect_source_image(&source)?;
    if width > MAX_IMAGE_EDGE
        || height > MAX_IMAGE_EDGE
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(WallpaperError::new(
            "wallpaper.dimensions-too-large",
            serde_json::json!({
                "maxEdge": MAX_IMAGE_EDGE,
                "maxPixels": MAX_IMAGE_PIXELS,
            }),
        ));
    }
    let decoded = decode_source_image(&source, format)?;
    let preserve_alpha = decoded.color().has_alpha();
    let normalized = encode_normalized_image(decoded, preserve_alpha)?;
    let (normalized_width, normalized_height) = normalized.raster.dimensions();
    let thumbnail_image =
        normalized
            .raster
            .resize_to_fill(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT, FilterType::Triangle);
    let thumbnail = encode_webp(&thumbnail_image)?;

    let mut store = load_store(root)?;
    let id = Uuid::new_v4().to_string();
    let image_file_name = format!("{id}.{}", normalized.extension);
    let thumbnail_file_name = format!("{id}.thumb.webp");
    let directory = wallpapers_dir(root);
    fs::create_dir_all(directory.as_std_path())
        .map_err(|_| WallpaperError::new("wallpaper.save-failed", serde_json::json!({})))?;
    let image_path = directory.join(&image_file_name);
    let thumbnail_path = directory.join(&thumbnail_file_name);
    write_new_file(&image_path, &normalized.bytes)?;
    if let Err(error) = write_new_file(&thumbnail_path, &thumbnail) {
        let _ = fs::remove_file(image_path.as_std_path());
        return Err(error);
    }

    store.recent_wallpapers.retain(|record| record.id != id);
    store.recent_wallpapers.insert(
        0,
        WallpaperRecord {
            id: id.clone(),
            image_file_name,
            thumbnail_file_name,
            image_mime_type: normalized.mime_type.to_string(),
            thumbnail_mime_type: "image/webp".to_string(),
            created_at: Utc::now().to_rfc3339(),
            width: normalized_width,
            height: normalized_height,
        },
    );
    let mut removed = Vec::new();
    while store.recent_wallpapers.len() > MAX_RECENT_WALLPAPERS {
        let remove_index = (1..store.recent_wallpapers.len())
            .rev()
            .find(|index| !retained_asset_ids.contains(&store.recent_wallpapers[*index].id))
            .unwrap_or(store.recent_wallpapers.len() - 1);
        removed.push(store.recent_wallpapers.remove(remove_index));
    }
    if let Err(error) = persist_store(root, &store) {
        let _ = fs::remove_file(image_path.as_std_path());
        let _ = fs::remove_file(thumbnail_path.as_std_path());
        return Err(error);
    }
    for record in removed {
        remove_record_files(root, &record);
    }
    cleanup_orphan_assets(root, &store);
    Ok(SavedWallpaper { asset_id: id })
}

pub fn select_recent_wallpaper(root: &Utf8Path, wallpaper_id: &str) -> Result<(), WallpaperError> {
    let mut store = load_store(root)?;
    let Some(index) = store
        .recent_wallpapers
        .iter()
        .position(|record| record.id == wallpaper_id && record_is_valid(root, record))
    else {
        return Err(WallpaperError::new(
            "wallpaper.recent-not-found",
            serde_json::json!({ "wallpaperId": wallpaper_id }),
        ));
    };
    let selected = store.recent_wallpapers.remove(index);
    store.recent_wallpapers.insert(0, selected);
    persist_store(root, &store)
}

fn canonical_source_path(raw: &str) -> Result<Utf8PathBuf, WallpaperError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(WallpaperError::new(
            "wallpaper.source-unavailable",
            serde_json::json!({}),
        ));
    }
    let canonical = fs::canonicalize(trimmed)
        .map_err(|_| WallpaperError::new("wallpaper.source-unavailable", serde_json::json!({})))?;
    Utf8PathBuf::from_path_buf(canonical)
        .map_err(|_| WallpaperError::new("wallpaper.source-path-invalid", serde_json::json!({})))
}

fn inspect_source_image(path: &Utf8Path) -> Result<(ImageFormat, u32, u32), WallpaperError> {
    let reader = ImageReader::open(path.as_std_path())
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|_| WallpaperError::new("wallpaper.invalid-image-data", serde_json::json!({})))?;
    let Some(format @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)) = reader.format()
    else {
        return Err(WallpaperError::new(
            "wallpaper.unsupported-image-type",
            serde_json::json!({}),
        ));
    };
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| WallpaperError::new("wallpaper.invalid-image-data", serde_json::json!({})))?;
    Ok((format, width, height))
}

fn decode_source_image(
    path: &Utf8Path,
    format: ImageFormat,
) -> Result<DynamicImage, WallpaperError> {
    let mut reader = ImageReader::open(path.as_std_path())
        .map_err(|_| WallpaperError::new("wallpaper.source-unavailable", serde_json::json!({})))?;
    reader.set_format(format);
    reader
        .decode()
        .map_err(|_| WallpaperError::new("wallpaper.invalid-image-data", serde_json::json!({})))
}

fn encode_normalized_image(
    mut image: DynamicImage,
    preserve_alpha: bool,
) -> Result<NormalizedWallpaper, WallpaperError> {
    for _ in 0..12 {
        let encoded = if preserve_alpha {
            encode_webp(&image)?
        } else {
            encode_jpeg(&image)?
        };
        let (width, height) = image.dimensions();
        if encoded.len() <= MAX_NORMALIZED_BYTES {
            return Ok(NormalizedWallpaper {
                raster: image,
                bytes: encoded,
                extension: if preserve_alpha { "webp" } else { "jpg" },
                mime_type: if preserve_alpha {
                    "image/webp"
                } else {
                    "image/jpeg"
                },
            });
        }
        let ratio =
            ((MAX_NORMALIZED_BYTES as f64 / encoded.len() as f64).sqrt() * 0.94).clamp(0.5, 0.9);
        let next_width = ((f64::from(width) * ratio).floor() as u32).max(1);
        let next_height = ((f64::from(height) * ratio).floor() as u32).max(1);
        if next_width == width && next_height == height {
            break;
        }
        image = image.resize(next_width, next_height, FilterType::Lanczos3);
    }
    Err(WallpaperError::new(
        "wallpaper.image-processing-failed",
        serde_json::json!({ "maxBytes": MAX_NORMALIZED_BYTES }),
    ))
}

fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, WallpaperError> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode_image(image)
        .map_err(|_| {
            WallpaperError::new("wallpaper.image-processing-failed", serde_json::json!({}))
        })?;
    Ok(bytes)
}

fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>, WallpaperError> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::WebP).map_err(|_| {
        WallpaperError::new("wallpaper.image-processing-failed", serde_json::json!({}))
    })?;
    Ok(bytes.into_inner())
}

fn write_new_file(path: &Utf8Path, bytes: &[u8]) -> Result<(), WallpaperError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path.as_std_path())
        .map_err(|_| WallpaperError::new("wallpaper.save-failed", serde_json::json!({})))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| WallpaperError::new("wallpaper.save-failed", serde_json::json!({})))
}

fn load_store(root: &Utf8Path) -> Result<WallpaperStore, WallpaperError> {
    let path = wallpaper_store_file(root);
    if !path.exists() {
        return Ok(WallpaperStore::default());
    }
    let store: WallpaperStore = read_json(&path)
        .map_err(|_| WallpaperError::new("wallpaper.load-failed", serde_json::json!({})))?;
    if store.version != WALLPAPER_STORE_VERSION {
        return Err(WallpaperError::new(
            "wallpaper.store-version-unsupported",
            serde_json::json!({ "version": store.version }),
        ));
    }
    Ok(store)
}

fn persist_store(root: &Utf8Path, store: &WallpaperStore) -> Result<(), WallpaperError> {
    write_json(&wallpaper_store_file(root), store)
        .map_err(|_| WallpaperError::new("wallpaper.save-failed", serde_json::json!({})))
}

fn wallpaper_preferences_vm(root: &Utf8Path, store: &WallpaperStore) -> WallpaperPreferencesVm {
    let recent_wallpapers = store
        .recent_wallpapers
        .iter()
        .filter(|record| record_is_valid(root, record))
        .map(|record| WallpaperImageVm {
            id: record.id.clone(),
            // Keep each protocol token in one URL path segment. Tauri's Windows
            // convertFileSrc implementation percent-encodes embedded slashes.
            image_url: format!("{}.full", record.id),
            thumbnail_url: format!("{}.thumbnail", record.id),
            created_at: record.created_at.clone(),
            width: record.width,
            height: record.height,
        })
        .collect::<Vec<_>>();
    WallpaperPreferencesVm { recent_wallpapers }
}

fn protocol_response(root: &Utf8Path, request_path: &str) -> Response<Vec<u8>> {
    let token = request_path.trim_matches('/');
    let (id, variant) = if let Some(id) = token.strip_suffix(".full") {
        (id, "full")
    } else if let Some(id) = token.strip_suffix(".thumbnail") {
        (id, "thumbnail")
    } else {
        return empty_response(StatusCode::BAD_REQUEST);
    };
    if Uuid::parse_str(id).is_err() {
        return empty_response(StatusCode::BAD_REQUEST);
    }
    let Ok(store) = load_store(root) else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    let Some(record) = store
        .recent_wallpapers
        .iter()
        .find(|record| record.id == id && record_metadata_is_valid(record))
    else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    let (file_name, mime_type) = if variant == "full" {
        (&record.image_file_name, &record.image_mime_type)
    } else {
        (&record.thumbnail_file_name, &record.thumbnail_mime_type)
    };
    let Ok(bytes) = fs::read(wallpapers_dir(root).join(file_name).as_std_path()) else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header("x-content-type-options", "nosniff")
        .body(bytes)
        .unwrap_or_else(|_| empty_response(StatusCode::INTERNAL_SERVER_ERROR))
}

fn empty_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .body(Vec::new())
        .expect("empty wallpaper protocol response is valid")
}

fn record_is_valid(root: &Utf8Path, record: &WallpaperRecord) -> bool {
    if !record_metadata_is_valid(record) {
        return false;
    }
    let directory = wallpapers_dir(root);
    directory.join(&record.image_file_name).is_file()
        && directory.join(&record.thumbnail_file_name).is_file()
}

fn record_metadata_is_valid(record: &WallpaperRecord) -> bool {
    if Uuid::parse_str(&record.id).is_err()
        || record.thumbnail_file_name != format!("{}.thumb.webp", record.id)
        || record.thumbnail_mime_type != "image/webp"
    {
        return false;
    }
    matches!(
        (
            record.image_file_name.as_str(),
            record.image_mime_type.as_str()
        ),
        (file_name, "image/jpeg") if file_name == format!("{}.jpg", record.id)
    ) || matches!(
        (
            record.image_file_name.as_str(),
            record.image_mime_type.as_str()
        ),
        (file_name, "image/webp") if file_name == format!("{}.webp", record.id)
    )
}

fn remove_record_files(root: &Utf8Path, record: &WallpaperRecord) {
    let directory = wallpapers_dir(root);
    let _ = fs::remove_file(directory.join(&record.image_file_name).as_std_path());
    let _ = fs::remove_file(directory.join(&record.thumbnail_file_name).as_std_path());
}

fn cleanup_orphan_assets(root: &Utf8Path, store: &WallpaperStore) {
    let directory = wallpapers_dir(root);
    let Ok(entries) = fs::read_dir(directory.as_std_path()) else {
        return;
    };
    let retained = store
        .recent_wallpapers
        .iter()
        .flat_map(|record| [&record.image_file_name, &record.thumbnail_file_name])
        .collect::<std::collections::HashSet<_>>();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !retained.contains(&name) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn wallpaper_store_file(root: &Utf8Path) -> Utf8PathBuf {
    root.join("desktop/wallpaper-settings.json")
}

fn wallpapers_dir(root: &Utf8Path) -> Utf8PathBuf {
    root.join("desktop/wallpapers")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use tempfile::tempdir;

    fn root(temp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap()
    }

    fn write_fixture(path: &Utf8Path, seed: u8) {
        let image = ImageBuffer::from_fn(640, 360, |x, y| {
            Rgb([seed.wrapping_add(x as u8), seed.wrapping_add(y as u8), seed])
        });
        image
            .save_with_format(path.as_std_path(), ImageFormat::Png)
            .unwrap();
    }

    #[test]
    fn import_and_recent_selection_are_bounded_while_retaining_selected_assets() {
        let temp = tempdir().unwrap();
        let root = root(&temp);
        let mut first_id = String::new();
        let mut evicted_id = String::new();
        let mut retained_asset_ids = HashSet::new();
        for seed in 0..11 {
            let source = root.join(format!("source-{seed}.png"));
            write_fixture(&source, seed);
            let saved = import_wallpaper_image(
                &root,
                ImportDesktopWallpaperInput {
                    source_path: source.to_string(),
                    color_scheme: ResolvedColorScheme::Light,
                },
                &retained_asset_ids,
            )
            .unwrap();
            if seed == 0 {
                first_id = saved.asset_id.clone();
                retained_asset_ids.insert(saved.asset_id.clone());
            } else if seed == 1 {
                evicted_id = saved.asset_id.clone();
            }
        }

        let preferences = load_resolved_wallpaper_preferences(&root).unwrap();
        assert_eq!(preferences.recent_wallpapers.len(), MAX_RECENT_WALLPAPERS);
        assert!(preferences.recent_wallpapers.iter().all(|wallpaper| {
            wallpaper.image_url == format!("{}.full", wallpaper.id)
                && wallpaper.thumbnail_url == format!("{}.thumbnail", wallpaper.id)
                && !wallpaper.image_url.contains('/')
                && !wallpaper.thumbnail_url.contains('/')
        }));
        assert!(
            preferences
                .recent_wallpapers
                .iter()
                .any(|item| item.id == first_id)
        );
        assert!(
            !preferences
                .recent_wallpapers
                .iter()
                .any(|item| item.id == evicted_id)
        );
        let selected_id = preferences.recent_wallpapers[4].id.clone();
        select_recent_wallpaper(&root, &selected_id).unwrap();
        let selected = load_resolved_wallpaper_preferences(&root).unwrap();
        assert_eq!(selected.recent_wallpapers[0].id, selected_id);
        assert_eq!(selected.recent_wallpapers.len(), MAX_RECENT_WALLPAPERS);
    }

    #[test]
    fn missing_selected_asset_converges_to_theme_without_losing_other_history() {
        let temp = tempdir().unwrap();
        let root = root(&temp);
        let missing_source = root.join("missing.png");
        write_fixture(&missing_source, 3);
        let missing = import_wallpaper_image(
            &root,
            ImportDesktopWallpaperInput {
                source_path: missing_source.to_string(),
                color_scheme: ResolvedColorScheme::Light,
            },
            &HashSet::new(),
        )
        .unwrap();
        let valid_source = root.join("valid.png");
        write_fixture(&valid_source, 4);
        let valid = import_wallpaper_image(
            &root,
            ImportDesktopWallpaperInput {
                source_path: valid_source.to_string(),
                color_scheme: ResolvedColorScheme::Dark,
            },
            &HashSet::new(),
        )
        .unwrap();
        let store = load_store(&root).unwrap();
        let missing_record = store
            .recent_wallpapers
            .iter()
            .find(|record| record.id == missing.asset_id)
            .unwrap();
        fs::remove_file(
            wallpapers_dir(&root)
                .join(&missing_record.image_file_name)
                .as_std_path(),
        )
        .unwrap();
        let mut personalization = PersonalizationPreference::default();
        personalization
            .wallpaper
            .for_color_scheme_mut(ResolvedColorScheme::Light)
            .image = WallpaperImagePreference::User {
            asset_id: missing.asset_id,
        };
        personalization
            .wallpaper
            .for_color_scheme_mut(ResolvedColorScheme::Dark)
            .image = WallpaperImagePreference::User {
            asset_id: valid.asset_id.clone(),
        };

        assert!(reconcile_wallpaper_personalization(&root, &mut personalization).unwrap());
        assert_eq!(
            personalization
                .wallpaper
                .for_color_scheme(ResolvedColorScheme::Light)
                .image,
            WallpaperImagePreference::Theme
        );
        assert_eq!(
            personalization
                .wallpaper
                .for_color_scheme(ResolvedColorScheme::Dark)
                .image,
            WallpaperImagePreference::User {
                asset_id: valid.asset_id
            }
        );
    }

    #[test]
    fn protocol_only_serves_indexed_uuid_assets_and_rejects_traversal() {
        let temp = tempdir().unwrap();
        let root = root(&temp);
        let source = root.join("source.png");
        write_fixture(&source, 9);
        let saved = import_wallpaper_image(
            &root,
            ImportDesktopWallpaperInput {
                source_path: source.to_string(),
                color_scheme: ResolvedColorScheme::Light,
            },
            &HashSet::new(),
        )
        .unwrap();
        let runtime = WallpaperProtocolRuntime::new(root);

        let full_response = runtime.protocol_response(&format!("/{}.full", saved.asset_id));
        assert_eq!(full_response.status(), StatusCode::OK);
        let full_image =
            image::load_from_memory_with_format(full_response.body(), ImageFormat::Jpeg).unwrap();
        assert_eq!(full_image.dimensions(), (640, 360));

        let thumbnail_response =
            runtime.protocol_response(&format!("/{}.thumbnail", saved.asset_id));
        assert_eq!(thumbnail_response.status(), StatusCode::OK);
        let thumbnail_image =
            image::load_from_memory_with_format(thumbnail_response.body(), ImageFormat::WebP)
                .unwrap();
        assert_eq!(
            thumbnail_image.dimensions(),
            (THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT),
        );
        assert_eq!(
            runtime.protocol_response("/../settings.json.full").status(),
            StatusCode::BAD_REQUEST,
        );
        assert_eq!(
            runtime
                .protocol_response(&format!("/{}.unknown", saved.asset_id))
                .status(),
            StatusCode::BAD_REQUEST,
        );
    }

    #[test]
    fn protocol_rejects_a_tampered_index_file_name_even_for_an_indexed_uuid() {
        let temp = tempdir().unwrap();
        let root = root(&temp);
        let source = root.join("source.png");
        write_fixture(&source, 11);
        let saved = import_wallpaper_image(
            &root,
            ImportDesktopWallpaperInput {
                source_path: source.to_string(),
                color_scheme: ResolvedColorScheme::Light,
            },
            &HashSet::new(),
        )
        .unwrap();
        let mut store = load_store(&root).unwrap();
        store.recent_wallpapers[0].image_file_name = "../secret.webp".to_string();
        persist_store(&root, &store).unwrap();
        let runtime = WallpaperProtocolRuntime::new(root);

        assert_eq!(
            runtime
                .protocol_response(&format!("/{}.full", saved.asset_id))
                .status(),
            StatusCode::NOT_FOUND,
        );
    }
}
