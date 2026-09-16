use base64::{Engine, engine::general_purpose::STANDARD};
use gold_band::acp::images::{AcpImageError, MAX_IMAGE_BASE64_BYTES};
use image::{ImageFormat, ImageReader, Limits};
use serde::Serialize;
use std::io::Cursor;

const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const THUMBNAIL_DIMENSION: u32 = 192;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpImageContentVm {
    data_url: String,
    mime_type: String,
    width: u32,
    height: u32,
}

pub fn decode_image(data: &str, thumbnail: bool) -> Result<AcpImageContentVm, AcpImageError> {
    if data.len() > MAX_IMAGE_BASE64_BYTES {
        return Err(AcpImageError::TooLarge);
    }
    let bytes = STANDARD.decode(data).map_err(|_| AcpImageError::Invalid)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(AcpImageError::TooLarge);
    }
    let reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| AcpImageError::Invalid)?;
    let format = reader.format().ok_or(AcpImageError::Invalid)?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| AcpImageError::Invalid)?;
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
    {
        return Err(AcpImageError::TooLarge);
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_PIXELS * 8);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| AcpImageError::Invalid)?;
    let (output, mime_type) = if thumbnail {
        let resized = decoded.thumbnail(THUMBNAIL_DIMENSION, THUMBNAIL_DIMENSION);
        let mut output = Cursor::new(Vec::new());
        resized
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| AcpImageError::Invalid)?;
        (output.into_inner(), "image/png")
    } else {
        (bytes, format.to_mime_type())
    };
    Ok(AcpImageContentVm {
        data_url: format!("data:{mime_type};base64,{}", STANDARD.encode(output)),
        mime_type: mime_type.into(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_real_format_preserves_original_and_bounds_thumbnail() {
        let image = image::DynamicImage::new_rgb8(800, 400);
        let mut original = Cursor::new(Vec::new());
        image.write_to(&mut original, ImageFormat::Jpeg).unwrap();
        let data = STANDARD.encode(original.get_ref());
        let full = decode_image(&data, false).unwrap();
        assert_eq!(full.mime_type, "image/jpeg");
        assert_eq!(full.data_url, format!("data:image/jpeg;base64,{data}"));
        let preview = decode_image(&data, true).unwrap();
        let preview_bytes = STANDARD
            .decode(preview.data_url.split_once(',').unwrap().1)
            .unwrap();
        let preview_image = image::load_from_memory(&preview_bytes).unwrap();
        assert_eq!((preview_image.width(), preview_image.height()), (192, 96));
        assert!(matches!(
            decode_image("invalid", false),
            Err(AcpImageError::Invalid)
        ));
        let mut large = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(MAX_IMAGE_DIMENSION + 1, 1)
            .write_to(&mut large, ImageFormat::Png)
            .unwrap();
        assert!(matches!(
            decode_image(&STANDARD.encode(large.get_ref()), true),
            Err(AcpImageError::TooLarge)
        ));
    }

    #[test]
    #[ignore = "requires an explicitly selected local ACP transcript"]
    fn replay_tool_images_from_local_transcript() {
        use gold_band::acp::{events, images, timeline};
        use std::io::BufRead;
        let input = std::env::var("GOLD_BAND_IMAGE_REPLAY_FILE").expect("set transcript path");
        let temp = tempfile::tempdir().unwrap();
        let path =
            camino::Utf8PathBuf::from_path_buf(temp.path().join("acp.timeline.jsonl")).unwrap();
        let mut store = timeline::TimelineStore::open(path.clone(), Default::default()).unwrap();
        let mut count = 0;
        for (index, line) in std::io::BufReader::new(std::fs::File::open(input).unwrap())
            .lines()
            .enumerate()
        {
            let frame: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            let Some(update) = frame.pointer("/frame/params/update") else {
                continue;
            };
            let event = events::normalize_session_update(index as u64 + 1, None, update);
            let refs = images::image_refs(&event);
            if refs.is_empty() {
                continue;
            }
            store.upsert(index as u64 + 1, &event).unwrap();
            for reference in refs {
                let data = images::read_image_base64(&path, &reference).unwrap();
                let original = decode_image(&data, false).unwrap();
                let thumbnail = decode_image(&data, true).unwrap();
                eprintln!(
                    "image bytes={} dimensions={}x{} declared={} actual={} thumbnail-url-bytes={}",
                    data.len() * 3 / 4,
                    original.width,
                    original.height,
                    reference.mime_type,
                    original.mime_type,
                    thumbnail.data_url.len()
                );
                count += 1;
            }
        }
        assert!(count > 0);
        eprintln!("Validated {count} image(s) through timeline storage and image decoding");
    }
}
