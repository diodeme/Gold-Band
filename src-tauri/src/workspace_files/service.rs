use std::borrow::Cow;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

use atomic_write_file::AtomicWriteFile;
use ignore::WalkBuilder;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::commands::CommandResult;

use super::models::{
    ExternalFileAccessGrantVm, FileRevisionVm, WorkspaceDirectoryEntryVm, WorkspaceFileLocatorVm,
    WorkspaceFileSearchVm, WorkspaceFileSnapshotVm, WriteFileResourceInput,
};
use super::paths::{
    ResolvedWorkspaceRoot, display_path, error, io_path_error, locator_for_path, path_is_within,
    relative_display,
};
use super::runtime::WorkspaceFileRuntime;

pub(crate) fn revision_for_path(path: &Path) -> CommandResult<FileRevisionVm> {
    let metadata =
        std::fs::metadata(path).map_err(|io_error| io_path_error(io_error, path, "read"))?;
    let mut file = File::open(path).map_err(|io_error| io_path_error(io_error, path, "read"))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|io_error| io_path_error(io_error, path, "read"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(FileRevisionVm {
        byte_length: metadata.len(),
        modified_at_ns: modified_at_ns(&metadata).unwrap_or_else(|| "0".to_string()),
        content_hash: hasher.finalize().to_hex().to_string(),
    })
}

pub(crate) fn list_directory(
    root: &ResolvedWorkspaceRoot,
    directory: &Path,
) -> CommandResult<Vec<WorkspaceDirectoryEntryVm>> {
    if !directory.is_dir() {
        return Err(error(
            "workspace-file.not-a-directory",
            serde_json::json!({ "path": display_path(directory) }),
        ));
    }
    let mut entries = Vec::new();
    let reader = std::fs::read_dir(directory)
        .map_err(|io_error| io_path_error(io_error, directory, "read"))?;
    for entry in reader {
        let entry = entry.map_err(|io_error| io_path_error(io_error, directory, "read"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|io_error| io_path_error(io_error, &path, "read"))?;
        let (kind, canonical_path, has_children) = if file_type.is_symlink() {
            match std::fs::canonicalize(&path) {
                Ok(target) if path_is_within(&target, &root.path) && target.is_dir() => {
                    ("directory", target, directory_has_children(&path))
                }
                Ok(target) => ("symlink", target, false),
                Err(_) => ("symlink", path.clone(), false),
            }
        } else if file_type.is_dir() {
            ("directory", path.clone(), directory_has_children(&path))
        } else if file_type.is_file() {
            ("file", path.clone(), false)
        } else {
            ("other", path.clone(), false)
        };
        let metadata = entry.metadata().ok();
        entries.push(WorkspaceDirectoryEntryVm {
            name: entry.file_name().to_string_lossy().into_owned(),
            relative_path: relative_display(&path, &root.path),
            canonical_path: display_path(&canonical_path),
            kind: kind.to_string(),
            has_children,
            byte_length: metadata
                .as_ref()
                .filter(|_| kind == "file")
                .map(Metadata::len),
            modified_at_ns: metadata.as_ref().and_then(modified_at_ns),
        });
    }
    entries.sort_by(|left, right| {
        let left_rank = usize::from(left.kind != "directory");
        let right_rank = usize::from(right.kind != "directory");
        left_rank
            .cmp(&right_rank)
            .then_with(|| natord::compare_ignore_case(&left.name, &right.name))
    });
    Ok(entries)
}

pub(crate) fn search_files(
    root: &ResolvedWorkspaceRoot,
    query: &str,
    request_id: String,
    requested_limit: usize,
) -> CommandResult<WorkspaceFileSearchVm> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(WorkspaceFileSearchVm {
            request_id,
            entries: Vec::new(),
            truncated: false,
        });
    }
    let limit = requested_limit
        .max(1)
        .min(root.config.search_result_limit.max(1));
    let file_name_pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let relative_path_pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let exact_file_name_pattern = Atom::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Exact,
        false,
    );
    let prefix_file_name_pattern = Atom::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Prefix,
        false,
    );
    let mut file_name_matcher = Matcher::new(Config::DEFAULT);
    let mut relative_path_matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut file_name_buf = Vec::new();
    let mut relative_path_buf = Vec::new();
    let mut ranked_entries = BinaryHeap::with_capacity(limit);
    let mut truncated = false;
    let walker = WalkBuilder::new(&root.path)
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .build();
    for result in walker {
        let Ok(entry) = result else { continue };
        if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let relative_path = relative_display(entry.path(), &root.path);
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative_path_haystack = Utf32Str::new(&relative_path, &mut relative_path_buf);
        let Some(relative_path_score) =
            relative_path_pattern.score(relative_path_haystack, &mut relative_path_matcher)
        else {
            continue;
        };
        let file_name_haystack = Utf32Str::new(&name, &mut file_name_buf);
        let file_name_score = file_name_pattern.score(file_name_haystack, &mut file_name_matcher);
        let exact_file_name = file_name_score.is_some()
            && exact_file_name_pattern
                .score(file_name_haystack, &mut file_name_matcher)
                .is_some();
        let prefix_file_name = file_name_score.is_some()
            && prefix_file_name_pattern
                .score(file_name_haystack, &mut file_name_matcher)
                .is_some();
        let metadata = entry.metadata().ok();
        let candidate = RankedSearchEntry {
            exact_file_name,
            prefix_file_name,
            file_name_score,
            relative_path_score,
            path_depth: relative_path_depth(&relative_path),
            entry: WorkspaceDirectoryEntryVm {
                name,
                relative_path,
                canonical_path: display_path(entry.path()),
                kind: "file".to_string(),
                has_children: false,
                byte_length: metadata.as_ref().map(Metadata::len),
                modified_at_ns: metadata.as_ref().and_then(modified_at_ns),
            },
        };
        if ranked_entries.len() < limit {
            ranked_entries.push(Reverse(candidate));
        } else {
            truncated = true;
            let should_replace = ranked_entries
                .peek()
                .is_some_and(|worst| candidate > worst.0);
            if should_replace {
                ranked_entries.pop();
                ranked_entries.push(Reverse(candidate));
            }
        }
    }
    let mut ranked_entries = ranked_entries
        .into_iter()
        .map(|entry| entry.0)
        .collect::<Vec<_>>();
    ranked_entries.sort_by(|left, right| right.cmp(left));
    Ok(WorkspaceFileSearchVm {
        request_id,
        entries: ranked_entries
            .into_iter()
            .map(|entry| entry.entry)
            .collect(),
        truncated,
    })
}

#[derive(Debug)]
struct RankedSearchEntry {
    exact_file_name: bool,
    prefix_file_name: bool,
    file_name_score: Option<u32>,
    relative_path_score: u32,
    path_depth: usize,
    entry: WorkspaceDirectoryEntryVm,
}

impl PartialEq for RankedSearchEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RankedSearchEntry {}

impl PartialOrd for RankedSearchEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedSearchEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.exact_file_name
            .cmp(&other.exact_file_name)
            .then_with(|| self.prefix_file_name.cmp(&other.prefix_file_name))
            .then_with(|| self.file_name_score.cmp(&other.file_name_score))
            .then_with(|| self.relative_path_score.cmp(&other.relative_path_score))
            .then_with(|| other.path_depth.cmp(&self.path_depth))
            .then_with(|| {
                natord::compare_ignore_case(&other.entry.relative_path, &self.entry.relative_path)
            })
            .then_with(|| other.entry.relative_path.cmp(&self.entry.relative_path))
    }
}

fn relative_path_depth(relative_path: &str) -> usize {
    relative_path
        .bytes()
        .filter(|separator| matches!(separator, b'/' | b'\\'))
        .count()
}

pub(crate) fn read_file(
    root: &ResolvedWorkspaceRoot,
    runtime: &WorkspaceFileRuntime,
    path: &Path,
    external_access_grant: Option<ExternalFileAccessGrantVm>,
    prefer_source: bool,
) -> CommandResult<WorkspaceFileSnapshotVm> {
    let metadata =
        std::fs::metadata(path).map_err(|io_error| io_path_error(io_error, path, "read"))?;
    let locator = locator_for_path(root, path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| display_path(path));
    let prefix = read_prefix(path, 64 * 1024)?;
    let image_kind = detect_image(path, &prefix);
    let detected_mime = infer::get(&prefix).map(|kind| kind.mime_type().to_string());

    if let Some(image) = image_kind
        && !(image.svg && prefer_source)
    {
        let revision = revision_for_path(path)?;
        return image_snapshot(
            root,
            runtime,
            path,
            metadata.len(),
            locator,
            name,
            revision,
            image,
            external_access_grant,
        );
    }

    if let Some(mime_type) = detected_mime.as_deref()
        && !(prefer_source && image_kind.is_some_and(|image| image.svg))
        && !mime_type.starts_with("text/")
        && !matches!(mime_type, "application/json" | "application/xml")
    {
        let revision = revision_for_path(path)?;
        return Ok(WorkspaceFileSnapshotVm::Unsupported {
            locator,
            name,
            revision,
            mime_type: Some(mime_type.to_string()),
            limitation_code: "workspace-file.format-unsupported".to_string(),
            external_access_grant,
        });
    }

    if metadata.len() > root.config.text_read_only_max_bytes {
        let revision = revision_for_path(path)?;
        return Ok(WorkspaceFileSnapshotVm::Unsupported {
            locator,
            name,
            revision,
            mime_type: image_kind.map(|image| image.mime_type.to_string()),
            limitation_code: "workspace-file.too-large".to_string(),
            external_access_grant,
        });
    }

    let bytes = std::fs::read(path).map_err(|io_error| io_path_error(io_error, path, "read"))?;
    let revision = revision_from_bytes(&bytes, &metadata);
    let (content, encoding) = match decode_text(&bytes) {
        Ok(decoded) => decoded,
        Err(code) => {
            return Ok(WorkspaceFileSnapshotVm::Unsupported {
                locator,
                name,
                revision,
                mime_type: detected_mime,
                limitation_code: code.to_string(),
                external_access_grant,
            });
        }
    };
    let editable = metadata.len() <= root.config.text_editable_max_bytes
        && (locator.scope == "workspace" || external_access_grant.is_some());
    Ok(WorkspaceFileSnapshotVm::Text {
        locator,
        name,
        revision,
        line_ending: detect_line_ending(&content).to_string(),
        language: language_for_path(path),
        content,
        encoding: encoding.to_string(),
        editable,
        limitation_code: (!editable).then(|| "workspace-file.read-only-size-limit".to_string()),
        external_access_grant,
    })
}

fn read_prefix(path: &Path, limit: u64) -> CommandResult<Vec<u8>> {
    let file = File::open(path).map_err(|io_error| io_path_error(io_error, path, "read"))?;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024) as usize);
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|io_error| io_path_error(io_error, path, "read"))?;
    Ok(bytes)
}

pub(crate) fn write_file(
    runtime: &WorkspaceFileRuntime,
    path: &Path,
    input: &WriteFileResourceInput,
) -> CommandResult<FileRevisionVm> {
    let current = revision_for_path(path)?;
    if !input.force && current != input.expected_revision {
        return Err(error(
            "workspace-file.changed-on-disk",
            serde_json::json!({
                "path": display_path(path),
                "expectedRevision": input.expected_revision,
                "actualRevision": current,
            }),
        ));
    }
    let content = normalize_line_endings(&input.content, &input.line_ending);
    let bytes = encode_text(&content, &input.encoding).ok_or_else(|| {
        error(
            "workspace-file.encoding-unsupported",
            serde_json::json!({ "path": display_path(path), "encoding": input.encoding }),
        )
    })?;
    let permissions = std::fs::metadata(path)
        .map_err(|io_error| io_path_error(io_error, path, "write"))?
        .permissions();
    let mut file =
        AtomicWriteFile::open(path).map_err(|io_error| io_path_error(io_error, path, "write"))?;
    file.write_all(&bytes)
        .map_err(|io_error| io_path_error(io_error, path, "write"))?;
    file.set_permissions(permissions.clone())
        .map_err(|io_error| io_path_error(io_error, path, "write"))?;
    file.commit()
        .map_err(|io_error| io_path_error(io_error, path, "write"))?;
    let _ = std::fs::set_permissions(path, permissions);
    let revision = revision_for_path(path)?;
    runtime.record_write(
        path.to_path_buf(),
        input.operation_id.clone(),
        revision.clone(),
    )?;
    Ok(revision)
}

fn normalize_line_endings<'a>(content: &'a str, line_ending: &str) -> Cow<'a, str> {
    match line_ending {
        "crlf" => Cow::Owned(content.replace("\r\n", "\n").replace('\n', "\r\n")),
        "lf" => Cow::Owned(content.replace("\r\n", "\n")),
        // Mixed files cannot be reconstructed after arbitrary line edits without a line map.
        // Preserve the editor payload rather than guessing and rewriting unrelated lines.
        _ => Cow::Borrowed(content),
    }
}

fn image_snapshot(
    root: &ResolvedWorkspaceRoot,
    runtime: &WorkspaceFileRuntime,
    path: &Path,
    byte_length: u64,
    locator: WorkspaceFileLocatorVm,
    name: String,
    revision: FileRevisionVm,
    image: DetectedImage,
    external_access_grant: Option<ExternalFileAccessGrantVm>,
) -> CommandResult<WorkspaceFileSnapshotVm> {
    if byte_length > root.config.image_preview_max_bytes {
        return Ok(WorkspaceFileSnapshotVm::Unsupported {
            locator,
            name,
            revision,
            mime_type: Some(image.mime_type.to_string()),
            limitation_code: "workspace-file.too-large".to_string(),
            external_access_grant,
        });
    }
    let (width, height) = image_dimensions(path, image.svg)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels > root.config.image_preview_max_pixels {
        return Ok(WorkspaceFileSnapshotVm::Unsupported {
            locator,
            name,
            revision,
            mime_type: Some(image.mime_type.to_string()),
            limitation_code: "workspace-file.image-dimensions-too-large".to_string(),
            external_access_grant,
        });
    }
    let preview_grant = runtime.issue_preview(
        root.project_id.clone(),
        path.to_path_buf(),
        revision.clone(),
        image.mime_type.to_string(),
        image.svg,
        root.config.preview_token_ttl_seconds,
    )?;
    let source_editable = image.svg
        && byte_length <= root.config.text_editable_max_bytes
        && (locator.scope == "workspace" || external_access_grant.is_some());
    Ok(WorkspaceFileSnapshotVm::Image {
        locator,
        name,
        revision,
        mime_type: image.mime_type.to_string(),
        width,
        height,
        animated: image.mime_type == "image/gif",
        preview_grant,
        source_editable,
        external_access_grant,
    })
}

#[derive(Clone, Copy)]
struct DetectedImage {
    mime_type: &'static str,
    svg: bool,
}

fn detect_image(path: &Path, bytes: &[u8]) -> Option<DetectedImage> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("svg") && looks_like_svg(bytes) {
        return Some(DetectedImage {
            mime_type: "image/svg+xml",
            svg: true,
        });
    }
    let mime = infer::get(bytes)?.mime_type();
    let supported = match mime {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        "image/bmp" | "image/x-ms-bmp" => "image/bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "image/x-icon",
        _ => return None,
    };
    Some(DetectedImage {
        mime_type: supported,
        svg: false,
    })
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let prefix = text.trim_start_matches('\u{feff}').trim_start();
    prefix.starts_with("<svg") || (prefix.starts_with("<?xml") && prefix.contains("<svg"))
}

fn image_dimensions(path: &Path, svg: bool) -> CommandResult<(u32, u32)> {
    if svg {
        let bytes =
            std::fs::read(path).map_err(|io_error| io_path_error(io_error, path, "read"))?;
        let mut options = resvg::usvg::Options::default();
        options.image_href_resolver = resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        };
        let tree = resvg::usvg::Tree::from_data(&bytes, &options).map_err(|_| {
            error(
                "workspace-file.image-decode-failed",
                serde_json::json!({ "path": display_path(path), "mimeType": "image/svg+xml" }),
            )
        })?;
        let size = tree.size().to_int_size();
        return Ok((size.width(), size.height()));
    }
    let size = imagesize::size(path).map_err(|_| {
        error(
            "workspace-file.image-decode-failed",
            serde_json::json!({ "path": display_path(path) }),
        )
    })?;
    let width = u32::try_from(size.width).map_err(|_| {
        error(
            "workspace-file.image-dimensions-too-large",
            serde_json::json!({ "path": display_path(path) }),
        )
    })?;
    let height = u32::try_from(size.height).map_err(|_| {
        error(
            "workspace-file.image-dimensions-too-large",
            serde_json::json!({ "path": display_path(path) }),
        )
    })?;
    Ok((width, height))
}

fn decode_text(bytes: &[u8]) -> Result<(String, &'static str), &'static str> {
    if let Some(content) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return std::str::from_utf8(content)
            .map(|content| (content.to_string(), "utf-8-bom"))
            .map_err(|_| "workspace-file.encoding-unsupported");
    }
    if let Some(content) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (decoded, _, had_errors) = encoding_rs::UTF_16LE.decode(content);
        return (!had_errors)
            .then(|| (decoded.into_owned(), "utf-16le-bom"))
            .ok_or("workspace-file.encoding-unsupported");
    }
    if let Some(content) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let (decoded, _, had_errors) = encoding_rs::UTF_16BE.decode(content);
        return (!had_errors)
            .then(|| (decoded.into_owned(), "utf-16be-bom"))
            .ok_or("workspace-file.encoding-unsupported");
    }
    std::str::from_utf8(bytes)
        .map(|content| (content.to_string(), "utf-8"))
        .map_err(|_| "workspace-file.encoding-unsupported")
}

fn encode_text(content: &str, encoding: &str) -> Option<Vec<u8>> {
    match encoding {
        "utf-8" => Some(content.as_bytes().to_vec()),
        "utf-8-bom" => {
            let mut bytes = Vec::with_capacity(content.len() + 3);
            bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
            bytes.extend_from_slice(content.as_bytes());
            Some(bytes)
        }
        "utf-16le-bom" => Some(encode_utf16(content, true)),
        "utf-16be-bom" => Some(encode_utf16(content, false)),
        _ => None,
    }
}

fn encode_utf16(content: &str, little_endian: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(content.len() * 2 + 2);
    bytes.extend_from_slice(if little_endian {
        &[0xff, 0xfe]
    } else {
        &[0xfe, 0xff]
    });
    for unit in content.encode_utf16() {
        let encoded = if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        };
        bytes.extend_from_slice(&encoded);
    }
    bytes
}

fn detect_line_ending(content: &str) -> &'static str {
    let has_crlf = content.contains("\r\n");
    let without_crlf: Cow<'_, str> = if has_crlf {
        Cow::Owned(content.replace("\r\n", ""))
    } else {
        Cow::Borrowed(content)
    };
    let has_lf = without_crlf.contains('\n');
    match (has_crlf, has_lf) {
        (true, true) => "mixed",
        (true, false) => "crlf",
        _ => "lf",
    }
}

fn language_for_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let lower_name = file_name.to_ascii_lowercase();
    let language = match lower_name.as_str() {
        "dockerfile" => "dockerfile",
        "makefile" => "makefile",
        _ => match path
            .extension()?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" => "rust",
            "js" | "mjs" | "cjs" => "javascript",
            "jsx" => "jsx",
            "ts" | "mts" | "cts" => "typescript",
            "tsx" => "tsx",
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "kt" | "kts" => "kotlin",
            "c" | "h" => "c",
            "cc" | "cpp" | "cxx" | "hpp" | "hxx" => "cpp",
            "cs" => "csharp",
            "swift" => "swift",
            "php" => "php",
            "rb" => "ruby",
            "sh" | "bash" | "zsh" => "shell",
            "ps1" => "powershell",
            "bat" | "cmd" => "batch",
            "sql" => "sql",
            "graphql" | "gql" => "graphql",
            "proto" => "protobuf",
            "json" | "jsonc" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "xml" => "xml",
            "html" | "htm" => "html",
            "css" => "css",
            "scss" => "scss",
            "less" => "less",
            "md" | "mdx" => "markdown",
            "csv" => "csv",
            "tsv" => "tsv",
            "ini" | "properties" | "env" => "properties",
            "svg" => "xml",
            _ => return None,
        },
    };
    Some(language.to_string())
}

fn revision_from_bytes(bytes: &[u8], metadata: &Metadata) -> FileRevisionVm {
    FileRevisionVm {
        byte_length: bytes.len() as u64,
        modified_at_ns: modified_at_ns(metadata).unwrap_or_else(|| "0".to_string()),
        content_hash: blake3::hash(bytes).to_hex().to_string(),
    }
}

fn modified_at_ns(metadata: &Metadata) -> Option<String> {
    Some(
        metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos()
            .to_string(),
    )
}

fn directory_has_children(path: &Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn root(path: &Path) -> ResolvedWorkspaceRoot {
        ResolvedWorkspaceRoot {
            project_id: "project-1".to_string(),
            path: std::fs::canonicalize(path).unwrap(),
            config: gold_band::config::WorkspaceFilesConfig::default(),
        }
    }

    #[test]
    fn utf16_roundtrip_preserves_bom() {
        for encoding in ["utf-16le-bom", "utf-16be-bom"] {
            let bytes = encode_text("你好\r\nGold Band", encoding).unwrap();
            let (decoded, detected) = decode_text(&bytes).unwrap();
            assert_eq!(decoded, "你好\r\nGold Band");
            assert_eq!(detected, encoding);
        }
    }

    #[test]
    fn revision_changes_with_content_even_when_length_is_equal() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("value.txt");
        std::fs::write(&path, "one").unwrap();
        let first = revision_for_path(&path).unwrap();
        std::fs::write(&path, "two").unwrap();
        let second = revision_for_path(&path).unwrap();
        assert_ne!(first.content_hash, second.content_hash);
    }

    #[test]
    fn line_endings_are_classified_without_counting_crlf_as_lf() {
        assert_eq!(detect_line_ending("a\r\nb\r\n"), "crlf");
        assert_eq!(detect_line_ending("a\nb\n"), "lf");
        assert_eq!(detect_line_ending("a\r\nb\n"), "mixed");
    }

    #[test]
    fn directory_listing_is_one_level_with_directories_first_and_natural_order() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("folder10")).unwrap();
        std::fs::create_dir(dir.path().join("folder2")).unwrap();
        std::fs::write(dir.path().join("file10.txt"), "ten").unwrap();
        std::fs::write(dir.path().join("file2.txt"), "two").unwrap();
        std::fs::write(dir.path().join("folder2").join("nested.txt"), "nested").unwrap();
        let workspace = root(dir.path());

        let entries = list_directory(&workspace, &workspace.path).unwrap();
        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["folder2", "folder10", "file2.txt", "file10.txt"]);
        assert!(entries[0].has_children);
        assert_eq!(entries.len(), 4);
    }

    #[test]
    fn search_respects_gitignore_and_result_limit() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir(dir.path().join("ignored")).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("ignored").join("match.rs"), "ignored").unwrap();
        std::fs::write(dir.path().join("src").join("match.rs"), "visible").unwrap();
        std::fs::write(dir.path().join("src").join("match2.rs"), "visible").unwrap();
        let workspace = root(dir.path());

        let result = search_files(&workspace, "match", "request-1".to_string(), 1).unwrap();
        assert_eq!(result.request_id, "request-1");
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].relative_path.starts_with("src/"));
        assert!(result.truncated);
    }

    #[test]
    fn search_ranks_exact_prefix_and_file_name_matches_before_path_only_matches() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("read"), "exact").unwrap();
        std::fs::write(dir.path().join("reader.rs"), "prefix").unwrap();
        std::fs::write(dir.path().join("thread.rs"), "file-name").unwrap();
        std::fs::create_dir(dir.path().join("read-guides")).unwrap();
        std::fs::write(dir.path().join("read-guides").join("value.rs"), "path").unwrap();
        let workspace = root(dir.path());

        let result = search_files(&workspace, "read", "request-rank".to_string(), 10).unwrap();
        let paths = result
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            ["read", "reader.rs", "thread.rs", "read-guides/value.rs"]
        );
    }

    #[test]
    fn search_supports_non_contiguous_fuzzy_file_name_matches() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("WorkspaceFileTree.tsx"), "tree").unwrap();
        std::fs::write(dir.path().join("工作区文件树.tsx"), "unicode-tree").unwrap();
        std::fs::write(dir.path().join("workspace.ts"), "other").unwrap();
        let workspace = root(dir.path());

        let result = search_files(&workspace, "wft", "request-fuzzy".to_string(), 10).unwrap();
        let unicode_result =
            search_files(&workspace, "工区树", "request-unicode".to_string(), 10).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "WorkspaceFileTree.tsx");
        assert_eq!(unicode_result.entries.len(), 1);
        assert_eq!(unicode_result.entries[0].name, "工作区文件树.tsx");
    }

    #[test]
    fn search_applies_the_limit_after_global_relevance_ranking() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("wft-notes")).unwrap();
        std::fs::write(
            dir.path().join("wft-notes").join("unrelated.txt"),
            "path-only",
        )
        .unwrap();
        std::fs::write(dir.path().join("WorkspaceFileTree.tsx"), "file-name").unwrap();
        let workspace = root(dir.path());

        let result = search_files(&workspace, "wft", "request-top-k".to_string(), 1).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "WorkspaceFileTree.tsx");
        assert!(result.truncated);
    }

    #[test]
    fn revision_conflict_never_changes_the_original_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("value.txt");
        std::fs::write(&path, "disk version").unwrap();
        let stale_revision = FileRevisionVm {
            byte_length: 1,
            modified_at_ns: "0".to_string(),
            content_hash: "stale".to_string(),
        };
        let result = write_file(
            &WorkspaceFileRuntime::default(),
            &path,
            &WriteFileResourceInput {
                project_id: "project-1".to_string(),
                canonical_path: display_path(&path),
                external_access_token: None,
                content: "local version".to_string(),
                encoding: "utf-8".to_string(),
                line_ending: "lf".to_string(),
                expected_revision: stale_revision,
                operation_id: "write-1".to_string(),
                force: false,
            },
        );

        assert_eq!(result.unwrap_err().code, "workspace-file.changed-on-disk");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "disk version");
    }

    #[test]
    fn oversized_svg_pixels_are_rejected_before_preview_token_issuance() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("large.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"/>"#,
        )
        .unwrap();
        let mut workspace = root(dir.path());
        workspace.config.image_preview_max_pixels = 100;
        let snapshot = read_file(
            &workspace,
            &WorkspaceFileRuntime::default(),
            &path,
            None,
            false,
        )
        .unwrap();

        assert!(
            matches!(snapshot, WorkspaceFileSnapshotVm::Unsupported { limitation_code, .. } if limitation_code == "workspace-file.image-dimensions-too-large")
        );
    }

    #[test]
    fn unsupported_binary_is_returned_as_a_structured_snapshot() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("payload.bin");
        std::fs::write(&path, [0xff, 0x00, 0xfe, 0x01]).unwrap();
        let workspace = root(dir.path());

        let snapshot = read_file(
            &workspace,
            &WorkspaceFileRuntime::default(),
            &path,
            None,
            false,
        )
        .unwrap();

        assert!(matches!(
            snapshot,
            WorkspaceFileSnapshotVm::Unsupported { limitation_code, .. }
                if limitation_code == "workspace-file.encoding-unsupported"
        ));
    }

    #[test]
    fn recognized_non_text_signature_is_never_opened_as_editable_utf8() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("document.txt");
        std::fs::write(&path, b"%PDF-1.7\n1 0 obj\n").unwrap();
        let workspace = root(dir.path());

        let snapshot = read_file(
            &workspace,
            &WorkspaceFileRuntime::default(),
            &path,
            None,
            false,
        )
        .unwrap();

        assert!(matches!(
            snapshot,
            WorkspaceFileSnapshotVm::Unsupported { limitation_code, mime_type, .. }
                if limitation_code == "workspace-file.format-unsupported"
                    && mime_type.as_deref() == Some("application/pdf")
        ));
    }

    #[test]
    fn file_signature_wins_over_a_text_extension_for_image_detection() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("actually-image.txt");
        image::DynamicImage::new_rgba8(3, 2)
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let workspace = root(dir.path());

        let snapshot = read_file(
            &workspace,
            &WorkspaceFileRuntime::default(),
            &path,
            None,
            false,
        )
        .unwrap();

        assert!(matches!(
            snapshot,
            WorkspaceFileSnapshotVm::Image { width: 3, height: 2, mime_type, .. }
                if mime_type == "image/png"
        ));
    }

    #[test]
    fn oversized_text_is_not_returned_to_the_webview() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("large.txt");
        std::fs::write(&path, "0123456789").unwrap();
        let mut workspace = root(dir.path());
        workspace.config.text_read_only_max_bytes = 5;

        let snapshot = read_file(
            &workspace,
            &WorkspaceFileRuntime::default(),
            &path,
            None,
            false,
        )
        .unwrap();

        assert!(matches!(
            snapshot,
            WorkspaceFileSnapshotVm::Unsupported { limitation_code, .. }
                if limitation_code == "workspace-file.too-large"
        ));
    }

    #[test]
    fn versioned_write_preserves_utf8_bom_and_crlf_policy() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("windows.txt");
        std::fs::write(&path, b"\xef\xbb\xbfa\r\nb\r\n").unwrap();
        let expected_revision = revision_for_path(&path).unwrap();

        let revision = write_file(
            &WorkspaceFileRuntime::default(),
            &path,
            &WriteFileResourceInput {
                project_id: "project-1".to_string(),
                canonical_path: display_path(&path),
                external_access_token: None,
                content: "a\nb changed\n".to_string(),
                encoding: "utf-8-bom".to_string(),
                line_ending: "crlf".to_string(),
                expected_revision,
                operation_id: "write-crlf".to_string(),
                force: false,
            },
        )
        .unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, b"\xef\xbb\xbfa\r\nb changed\r\n");
        assert_eq!(
            revision.content_hash,
            blake3::hash(&bytes).to_hex().to_string()
        );
    }
}
