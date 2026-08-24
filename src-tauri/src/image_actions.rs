use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(not(windows))]
use std::borrow::Cow;

use base64::Engine;
use gold_band::storage::atomic_write_file;
use serde::Deserialize;

use crate::commands::{CommandErrorVm, CommandResult, spawn_blocking_command};

const MAX_IMAGE_ACTION_BYTES: u64 = crate::view_models_conversation::MAX_ATTACHMENT_PER_FILE;
const MAX_IMAGE_ACTION_BASE64_CHARS: usize = ((MAX_IMAGE_ACTION_BYTES as usize + 2) / 3) * 4 + 8;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ImageActionSourceInput {
    Path {
        path: String,
    },
    Bytes {
        #[serde(rename = "dataBase64")]
        data_base64: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveImageAsInput {
    pub source: ImageActionSourceInput,
    pub destination_path: String,
}

#[tauri::command]
pub async fn copy_image_to_clipboard(source: ImageActionSourceInput) -> CommandResult<()> {
    spawn_blocking_command(move || {
        let bytes = read_image_source(source)?;
        write_image_to_clipboard(&bytes)
    })
    .await
}

#[tauri::command]
pub async fn save_image_as(input: SaveImageAsInput) -> CommandResult<()> {
    spawn_blocking_command(move || {
        let bytes = read_image_source(input.source)?;
        validate_image(&bytes)?;
        write_image_atomically(Path::new(&input.destination_path), &bytes)
    })
    .await
}

fn read_image_source(source: ImageActionSourceInput) -> CommandResult<Vec<u8>> {
    match source {
        ImageActionSourceInput::Path { path } => read_path_source(PathBuf::from(path)),
        ImageActionSourceInput::Bytes { data_base64 } => read_base64_source(data_base64),
    }
}

fn read_path_source(path: PathBuf) -> CommandResult<Vec<u8>> {
    let metadata = std::fs::metadata(&path).map_err(|_| {
        image_action_error(
            "image-action.source-unreadable",
            serde_json::json!({ "path": path.display().to_string() }),
        )
    })?;
    if !metadata.is_file() {
        return Err(image_action_error(
            "image-action.source-unreadable",
            serde_json::json!({ "path": path.display().to_string() }),
        ));
    }
    ensure_size_allowed(metadata.len())?;
    std::fs::read(&path).map_err(|_| {
        image_action_error(
            "image-action.source-unreadable",
            serde_json::json!({ "path": path.display().to_string() }),
        )
    })
}

fn read_base64_source(data_base64: String) -> CommandResult<Vec<u8>> {
    if data_base64.len() > MAX_IMAGE_ACTION_BASE64_CHARS {
        return Err(image_action_error(
            "image-action.source-too-large",
            serde_json::json!({ "maxBytes": MAX_IMAGE_ACTION_BYTES }),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    ensure_size_allowed(bytes.len() as u64)?;
    Ok(bytes)
}

fn ensure_size_allowed(size: u64) -> CommandResult<()> {
    if size == 0 {
        return Err(image_action_error(
            "image-action.source-invalid",
            serde_json::json!({}),
        ));
    }
    if size > MAX_IMAGE_ACTION_BYTES {
        return Err(image_action_error(
            "image-action.source-too-large",
            serde_json::json!({ "maxBytes": MAX_IMAGE_ACTION_BYTES }),
        ));
    }
    Ok(())
}

fn validate_image(bytes: &[u8]) -> CommandResult<()> {
    image::guess_format(bytes)
        .and_then(|format| image::load_from_memory_with_format(bytes, format).map(|_| ()))
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))
}

fn decode_clipboard_image(bytes: &[u8]) -> CommandResult<(usize, usize, Vec<u8>)> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))?
        .into_rgba8();
    let width = usize::try_from(image.width())
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    let height = usize::try_from(image.height())
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    Ok((width, height, image.into_raw()))
}

fn write_image_to_clipboard(bytes: &[u8]) -> CommandResult<()> {
    let (width, height, rgba) = decode_clipboard_image(bytes)?;
    write_decoded_image_to_clipboard(bytes, width, height, rgba)
}

#[cfg(not(windows))]
fn write_decoded_image_to_clipboard(
    _encoded: &[u8],
    width: usize,
    height: usize,
    rgba: Vec<u8>,
) -> CommandResult<()> {
    let mut clipboard = arboard::Clipboard::new().map_err(|_| {
        image_action_error("image-action.clipboard-unavailable", serde_json::json!({}))
    })?;
    clipboard
        .set_image(arboard::ImageData {
            width,
            height,
            bytes: Cow::Owned(rgba),
        })
        .map_err(|_| {
            image_action_error("image-action.clipboard-write-failed", serde_json::json!({}))
        })
}

#[cfg(windows)]
fn write_decoded_image_to_clipboard(
    encoded: &[u8],
    width: usize,
    height: usize,
    rgba: Vec<u8>,
) -> CommandResult<()> {
    let dibv5 = encode_windows_dibv5(width, height, rgba)?;
    let _clipboard = clipboard_win::Clipboard::new_attempts(5).map_err(|_| {
        image_action_error("image-action.clipboard-unavailable", serde_json::json!({}))
    })?;
    clipboard_win::raw::empty().map_err(|_| clipboard_write_error())?;

    if matches!(image::guess_format(encoded), Ok(image::ImageFormat::Png)) {
        let png_format = clipboard_win::register_format("PNG").ok_or_else(clipboard_write_error)?;
        clipboard_win::raw::set_without_clear(png_format.get(), encoded)
            .map_err(|_| clipboard_write_error())?;
    }

    clipboard_win::raw::set_without_clear(clipboard_win::formats::CF_DIBV5, &dibv5)
        .map_err(|_| clipboard_write_error())
}

#[cfg(windows)]
fn encode_windows_dibv5(width: usize, height: usize, mut rgba: Vec<u8>) -> CommandResult<Vec<u8>> {
    const HEADER_SIZE: usize = 124;
    let width_i32 = i32::try_from(width)
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    let height_i32 = i32::try_from(height)
        .map_err(|_| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    let pixel_bytes = row_bytes
        .checked_mul(height)
        .ok_or_else(|| image_action_error("image-action.source-invalid", serde_json::json!({})))?;
    if rgba.len() != pixel_bytes || pixel_bytes > u32::MAX as usize {
        return Err(image_action_error(
            "image-action.source-invalid",
            serde_json::json!({}),
        ));
    }

    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    for top_row in 0..height / 2 {
        let top_start = top_row * row_bytes;
        let bottom_start = (height - top_row - 1) * row_bytes;
        let (head, tail) = rgba.split_at_mut(bottom_start);
        head[top_start..top_start + row_bytes].swap_with_slice(&mut tail[..row_bytes]);
    }

    let mut dibv5 = Vec::with_capacity(HEADER_SIZE + pixel_bytes);
    dibv5.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    dibv5.extend_from_slice(&width_i32.to_le_bytes());
    dibv5.extend_from_slice(&height_i32.to_le_bytes());
    dibv5.extend_from_slice(&1_u16.to_le_bytes());
    dibv5.extend_from_slice(&32_u16.to_le_bytes());
    dibv5.extend_from_slice(&3_u32.to_le_bytes()); // BI_BITFIELDS
    dibv5.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    dibv5.extend_from_slice(&0_i32.to_le_bytes()); // XPelsPerMeter
    dibv5.extend_from_slice(&0_i32.to_le_bytes()); // YPelsPerMeter
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // ClrUsed
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // ClrImportant
    dibv5.extend_from_slice(&0x00ff_0000_u32.to_le_bytes());
    dibv5.extend_from_slice(&0x0000_ff00_u32.to_le_bytes());
    dibv5.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
    dibv5.extend_from_slice(&0xff00_0000_u32.to_le_bytes());
    dibv5.extend_from_slice(&0x7352_4742_u32.to_le_bytes()); // LCS_sRGB
    dibv5.extend_from_slice(&[0_u8; 36]); // CIEXYZTRIPLE
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // GammaRed
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // GammaGreen
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // GammaBlue
    dibv5.extend_from_slice(&4_u32.to_le_bytes()); // LCS_GM_IMAGES
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // ProfileData
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // ProfileSize
    dibv5.extend_from_slice(&0_u32.to_le_bytes()); // Reserved
    debug_assert_eq!(dibv5.len(), HEADER_SIZE);
    dibv5.extend_from_slice(&rgba);
    Ok(dibv5)
}

#[cfg(windows)]
fn clipboard_write_error() -> CommandErrorVm {
    image_action_error("image-action.clipboard-write-failed", serde_json::json!({}))
}

fn write_image_atomically(destination: &Path, bytes: &[u8]) -> CommandResult<()> {
    if destination.file_name().is_none() || destination.parent().is_none() {
        return Err(image_action_error(
            "image-action.destination-invalid",
            serde_json::json!({}),
        ));
    }
    let existing_permissions = std::fs::metadata(destination)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.permissions());
    atomic_write_file(destination, |file| -> std::io::Result<()> {
        file.write_all(bytes)?;
        if let Some(permissions) = existing_permissions.as_ref() {
            file.set_permissions(permissions.clone())?;
        }
        Ok(())
    })
    .map_err(|_| {
        image_action_error(
            "image-action.save-failed",
            serde_json::json!({ "path": destination.display().to_string() }),
        )
    })?;
    Ok(())
}

fn image_action_error(code: &'static str, params: serde_json::Value) -> CommandErrorVm {
    CommandErrorVm::new(code, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::io::Cursor;

    fn png_bytes() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 1, Rgba([12, 34, 56, 78])));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn clipboard_projection_decodes_the_image_into_rgba_pixels() {
        let (width, height, rgba) = decode_clipboard_image(&png_bytes()).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, vec![12, 34, 56, 78, 12, 34, 56, 78]);
    }

    #[test]
    fn save_preserves_the_original_encoded_image_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("copy.png");
        let bytes = png_bytes();

        write_image_atomically(&destination, &bytes).unwrap();

        assert_eq!(std::fs::read(destination).unwrap(), bytes);
    }

    #[test]
    fn invalid_base64_and_non_image_bytes_return_stable_error_codes() {
        let base64_error = read_base64_source("%%%".to_string()).unwrap_err();
        assert_eq!(base64_error.code, "image-action.source-invalid");

        let image_error = validate_image(b"not an image").unwrap_err();
        assert_eq!(image_error.code, "image-action.source-invalid");
    }

    #[test]
    fn in_memory_source_accepts_the_frontend_camel_case_contract() {
        let source: ImageActionSourceInput = serde_json::from_value(serde_json::json!({
            "kind": "bytes",
            "dataBase64": "AQIDBA=="
        }))
        .unwrap();

        match source {
            ImageActionSourceInput::Bytes { data_base64 } => {
                assert_eq!(data_base64, "AQIDBA==");
            }
            ImageActionSourceInput::Path { .. } => panic!("expected in-memory image source"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_dibv5_projection_uses_bgra_bottom_up_pixels() {
        let rgba = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

        let dibv5 = encode_windows_dibv5(2, 2, rgba).unwrap();

        assert_eq!(u32::from_le_bytes(dibv5[0..4].try_into().unwrap()), 124);
        assert_eq!(i32::from_le_bytes(dibv5[4..8].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(dibv5[8..12].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(dibv5[16..20].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(dibv5[20..24].try_into().unwrap()), 16);
        assert_eq!(
            &dibv5[124..],
            &[11, 10, 9, 12, 15, 14, 13, 16, 3, 2, 1, 4, 7, 6, 5, 8],
        );
    }
}
