use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::events::AcpUiEvent;
use super::timeline::{hydrate_timeline_value, read_indexed_timeline_item};

pub const MAX_IMAGE_BASE64_BYTES: usize = 24 * 1024 * 1024;
pub const MAX_PROJECTED_IMAGES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpImageRef {
    pub event_id: String,
    pub pointer: String,
    pub content_hash: String,
    pub mime_type: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AcpImageError {
    #[error("acp.image-not-found")]
    NotFound,
    #[error("acp.image-invalid")]
    Invalid,
    #[error("acp.image-too-large")]
    TooLarge,
}

// These are protocol positions, not a recursive search through arbitrary tool JSON.
pub fn image_pointers(kind: &str, raw: &Value) -> Vec<String> {
    let is_image = |value: &Value| value.get("type").and_then(Value::as_str) == Some("image");
    if !matches!(kind, "toolCall" | "toolCallUpdate") {
        return Vec::new();
    }
    let standard = raw
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, item)| {
            item.get("type").and_then(Value::as_str) == Some("content")
                && item.get("content").is_some_and(is_image)
        })
        .take(MAX_PROJECTED_IMAGES)
        .map(|(index, _)| format!("/content/{index}/content"))
        .collect::<Vec<_>>();
    if !standard.is_empty() {
        return standard;
    }
    raw.pointer("/rawOutput/result/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, item)| is_image(item))
        .take(MAX_PROJECTED_IMAGES)
        .map(|(index, _)| format!("/rawOutput/result/content/{index}"))
        .collect()
}

pub fn image_refs(event: &AcpUiEvent) -> Vec<AcpImageRef> {
    let Some(raw) = event.raw.as_ref() else {
        return Vec::new();
    };
    image_refs_from_raw(&event.id, &event.kind, raw)
}

pub fn image_refs_from_raw(event_id: &str, kind: &str, raw: &Value) -> Vec<AcpImageRef> {
    image_pointers(kind, raw)
        .into_iter()
        .map(|pointer| {
            let block = raw.pointer(&pointer).expect("selected image exists");
            let data = &block["data"];
            let content_hash = data
                .as_str()
                .map(|text| blake3::hash(text.as_bytes()).to_hex().to_string())
                .or_else(|| {
                    data.pointer("/$goldBandBlob/contentHash")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            AcpImageRef {
                event_id: event_id.to_string(),
                pointer,
                content_hash,
                mime_type: block
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("image/unknown")
                    .to_string(),
            }
        })
        .collect()
}

pub fn project_image_refs(event: &mut AcpUiEvent) {
    let images = image_refs(event);
    if let Some(raw) = event.raw.as_mut().and_then(Value::as_object_mut) {
        // A previously compacted event may be compacted again at another boundary.
        if !images.is_empty() {
            raw.insert(
                "goldBandImages".into(),
                serde_json::to_value(images).expect("image references serialize"),
            );
        }
    }
}

pub fn read_image_base64(
    path: &Utf8Path,
    requested: &AcpImageRef,
) -> Result<String, AcpImageError> {
    let item = read_indexed_timeline_item(path, &requested.event_id)
        .map_err(|_| AcpImageError::NotFound)?
        .ok_or(AcpImageError::NotFound)?
        .event;
    if !image_refs(&item).contains(requested) {
        return Err(AcpImageError::NotFound);
    }
    let block = item
        .raw
        .as_ref()
        .and_then(|raw| raw.pointer(&requested.pointer))
        .ok_or(AcpImageError::NotFound)?;
    if !requested.mime_type.starts_with("image/") {
        return Err(AcpImageError::Invalid);
    }
    let mut data = block.get("data").cloned().ok_or(AcpImageError::Invalid)?;
    let size = data
        .as_str()
        .map(|s| s.len() as u64)
        .or_else(|| {
            data.pointer("/$goldBandBlob/byteLength")
                .and_then(Value::as_u64)
        })
        .ok_or(AcpImageError::Invalid)?;
    if size > MAX_IMAGE_BASE64_BYTES as u64 {
        return Err(AcpImageError::TooLarge);
    }
    if let Some(reference) = data.get("$goldBandBlob") {
        let version: super::turn_files::FileVersionRef =
            serde_json::from_value(reference.clone()).map_err(|_| AcpImageError::Invalid)?;
        let store = super::turn_files::TurnFileStore::new(
            super::timeline::timeline_attempt_dir(path),
            Default::default(),
        );
        store
            .validate_blob_scope(&version)
            .map_err(|_| AcpImageError::Invalid)?;
    }
    hydrate_timeline_value(path, &mut data).map_err(|_| AcpImageError::Invalid)?;
    data.as_str()
        .map(str::to_string)
        .ok_or(AcpImageError::Invalid)
}

pub fn strip_image_bodies(raw: &mut Value) {
    for (pointer, wrapped) in [("/content", true), ("/rawOutput/result/content", false)] {
        if let Some(items) = raw.pointer_mut(pointer).and_then(Value::as_array_mut) {
            for item in items {
                let block = if wrapped {
                    if item.get("type").and_then(Value::as_str) != Some("content") {
                        continue;
                    }
                    item.get_mut("content")
                } else {
                    Some(item)
                };
                if let Some(block) = block.and_then(Value::as_object_mut)
                    && block.get("type").and_then(Value::as_str) == Some("image")
                {
                    block.remove("data");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_detail_strips_both_image_sources_without_removing_text() {
        let mut raw = json!({
            "content": [{"type":"content", "content":{"type":"image", "data":"standard"}},
                {"type":"content", "content":{"type":"text", "text":"keep"}}],
            "rawOutput":{"result":{"content":[{"type":"image", "data":"extension"},
                {"type":"text", "text":"also keep"}]}}
        });
        strip_image_bodies(&mut raw);
        assert!(raw.pointer("/content/0/content/data").is_none());
        assert!(raw.pointer("/rawOutput/result/content/0/data").is_none());
        assert_eq!(raw.pointer("/content/1/content/text"), Some(&json!("keep")));
        assert_eq!(
            raw.pointer("/rawOutput/result/content/1/text"),
            Some(&json!("also keep"))
        );
        assert_eq!(image_pointers("toolCall", &raw), ["/content/0/content"]);
    }

    #[test]
    fn standard_images_win_even_when_invalid_and_text_allows_extension() {
        let mut raw = json!({ "content": [{ "type": "content", "content": { "type": "image" } }],
            "rawOutput": { "result": { "content": [{ "type": "text" }, { "type": "image" }] } } });
        assert_eq!(image_pointers("toolCall", &raw), ["/content/0/content"]);
        raw["content"][0]["content"]["type"] = json!("text");
        assert_eq!(
            image_pointers("toolCallUpdate", &raw),
            ["/rawOutput/result/content/1"]
        );
        raw["content"][0]["type"] = json!("image");
        assert_eq!(
            image_pointers("toolCallUpdate", &raw),
            ["/rawOutput/result/content/1"]
        );
        assert!(image_pointers("thoughtDelta", &raw).is_empty());
        assert!(image_pointers("toolCall", &json!({"output":{"type":"image"}})).is_empty());
    }
}
