//! 用户反馈上报领域。
//!
//! 反馈只接受业务标识与内存中的截图内容。工作区、任务目录和日志路径均由后端解析，
//! 避免把前端传入的文件系统路径当成可信边界。

use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use gold_band::config::{RuntimeConfig, StateConfig};
use gold_band::storage::GoldBandPaths;
use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tempfile::NamedTempFile;
use tokio_util::io::ReaderStream;
use walkdir::WalkDir;

use crate::channel::current_channel_config;
use crate::commands::{CommandErrorVm, CommandResult, command_error, spawn_blocking_command};
use crate::conversation_workspace::{app_for_workspace, workspace_entry_for_project};
use crate::metrics::{endpoint_from_base_url, get_api_key, metrics_base_url, metrics_log};
use crate::state::{DesktopContext, DesktopState};

pub const MAX_DESCRIPTION_CHARS: usize = 2_000;
pub const MAX_SCREENSHOTS: usize = 4;
pub const MAX_SCREENSHOT_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_SCREENSHOT_DIMENSION: u32 = 8_192;
pub const MAX_SCREENSHOT_DECODE_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_ARCHIVE_COMPRESSED_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_ARCHIVE_FILE_COUNT: usize = 5_000;
pub const MAX_REQUEST_BYTES: u64 = 30 * 1024 * 1024;
pub const LOG_TAIL_BYTES: usize = 512 * 1024;
pub const FEEDBACK_ENDPOINT_PATH: &str = "/api/client-report/feedback";

const REQUEST_TIMEOUT_SECS: u64 = 60;
const CONNECT_TIMEOUT_SECS: u64 = 10;
const MAX_SCREENSHOT_BASE64_CHARS: usize = (MAX_SCREENSHOT_BYTES * 4 / 3) + 8;
const LOG_MIME: &str = "text/plain";
const ARCHIVE_MIME: &str = "application/zip";
const SCREENSHOT_MIME: &str = "image/png";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackScreenshotInput {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub data_base64: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackInput {
    pub description: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub screenshots: Vec<FeedbackScreenshotInput>,
    #[serde(default = "default_true")]
    pub include_logs: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackResult {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackArchivePreview {
    pub uncompressed_bytes: u64,
    pub file_count: usize,
    pub within_limits: bool,
    pub max_uncompressed_bytes: u64,
    pub max_file_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedbackMetadata {
    user_id: String,
    client_version: String,
    reported_at: String,
    session_project_id: Option<String>,
    session_task_id: Option<String>,
    log_attached: bool,
    archive_attached: bool,
    archive_bytes: u64,
    archive_uncompressed_bytes: u64,
    archive_file_count: usize,
    screenshot_count: usize,
}

#[derive(Debug)]
struct PreparedScreenshot {
    file_name: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ArchiveFile {
    source: PathBuf,
    zip_name: String,
    size: u64,
}

#[derive(Debug, Clone)]
struct ArchivePlan {
    task_root: PathBuf,
    files: Vec<ArchiveFile>,
    uncompressed_bytes: u64,
    file_count: usize,
    within_limits: bool,
    policy: ArchivePolicy,
}

#[derive(Debug)]
struct PreparedArchive {
    file: NamedTempFile,
    compressed_bytes: u64,
    uncompressed_bytes: u64,
    file_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct ArchivePolicy {
    max_uncompressed_bytes: u64,
    max_compressed_bytes: u64,
    max_file_count: usize,
}

const ARCHIVE_POLICY: ArchivePolicy = ArchivePolicy {
    max_uncompressed_bytes: MAX_ARCHIVE_UNCOMPRESSED_BYTES,
    max_compressed_bytes: MAX_ARCHIVE_COMPRESSED_BYTES,
    max_file_count: MAX_ARCHIVE_FILE_COUNT,
};

#[tauri::command]
pub async fn submit_feedback(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    input: FeedbackInput,
) -> CommandResult<FeedbackResult> {
    ensure_feedback_enabled(current_channel_config().feedback_enabled)?;
    validate_input(&input)?;

    let context = state.context().map_err(command_error)?;
    let endpoint = resolve_endpoint(&context.config).ok_or_else(|| {
        CommandErrorVm::new("feedback.endpoint-unconfigured", serde_json::json!({}))
    })?;
    let api_key = get_api_key(&context.config);

    let FeedbackInput {
        description,
        project_id,
        task_id,
        screenshots,
        include_logs,
    } = input;

    let prepared_screenshots =
        spawn_blocking_command(move || prepare_screenshots(&screenshots)).await?;
    let log_bytes = if include_logs {
        read_log_tail(&GoldBandPaths::new(context.repo_root.clone()))
    } else {
        None
    };

    let prepared_archive = match (project_id.as_deref(), task_id.as_deref()) {
        (Some(project_id), Some(task_id)) => {
            let archive_context = context.clone();
            let project_id = project_id.to_string();
            let task_id = task_id.to_string();
            Some(
                spawn_blocking_command(move || {
                    let plan = resolve_session_archive_plan(
                        &archive_context,
                        &project_id,
                        &task_id,
                        ARCHIVE_POLICY,
                    )?;
                    if !plan.within_limits {
                        return Err(payload_too_large("session-archive"));
                    }
                    build_archive(plan)
                })
                .await?,
            )
        }
        _ => None,
    };

    let client_version = app_handle.package_info().version.to_string();
    let metadata = FeedbackMetadata {
        user_id: crate::metrics::get_system_username(),
        client_version,
        reported_at: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        session_project_id: project_id.clone(),
        session_task_id: task_id.clone(),
        log_attached: log_bytes.is_some(),
        archive_attached: prepared_archive.is_some(),
        archive_bytes: prepared_archive
            .as_ref()
            .map(|archive| archive.compressed_bytes)
            .unwrap_or(0),
        archive_uncompressed_bytes: prepared_archive
            .as_ref()
            .map(|archive| archive.uncompressed_bytes)
            .unwrap_or(0),
        archive_file_count: prepared_archive
            .as_ref()
            .map(|archive| archive.file_count)
            .unwrap_or(0),
        screenshot_count: prepared_screenshots.len(),
    };
    let metadata_json = serde_json::to_string(&metadata).map_err(|_| internal_error())?;
    ensure_request_size(
        metadata_json.len() as u64,
        description.len() as u64,
        log_bytes.as_deref(),
        &prepared_screenshots,
        prepared_archive.as_ref(),
    )?;

    let part_names = feedback_part_names(
        log_bytes.is_some(),
        prepared_archive.is_some(),
        prepared_screenshots.len(),
    );
    metrics_log(&format!(
        "[feedback] preparing POST {} parts={}",
        &endpoint,
        part_names.join(",")
    ));

    let mut form = reqwest::multipart::Form::new()
        .text("metadata", metadata_json)
        .text("description", description);
    if let Some(log_bytes) = log_bytes {
        form = form.part(
            "log",
            reqwest::multipart::Part::bytes(log_bytes)
                .file_name("runtime.log")
                .mime_str(LOG_MIME)
                .map_err(|_| internal_error())?,
        );
    }
    if let Some(archive) = prepared_archive.as_ref() {
        let archive_file = tokio::fs::File::open(archive.file.path())
            .await
            .map_err(|_| internal_error())?;
        let stream = ReaderStream::new(archive_file);
        form = form.part(
            "session_archive",
            reqwest::multipart::Part::stream(reqwest::Body::wrap_stream(stream))
                .file_name("task.zip")
                .mime_str(ARCHIVE_MIME)
                .map_err(|_| internal_error())?,
        );
    }
    for (index, screenshot) in prepared_screenshots.into_iter().enumerate() {
        form = form.part(
            format!("screenshot_{index}"),
            reqwest::multipart::Part::bytes(screenshot.bytes)
                .file_name(screenshot.file_name)
                .mime_str(SCREENSHOT_MIME)
                .map_err(|_| internal_error())?,
        );
    }

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|_| internal_error())?;
    let mut request = client.post(&endpoint).multipart(form);
    if let Some(api_key) = api_key {
        request = request.header("X-Maling-Report-Key", api_key);
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => {
            metrics_log(&format!("[feedback] response status={}", response.status()));
            Ok(FeedbackResult { success: true })
        }
        Ok(response) => {
            let status = response.status().as_u16();
            metrics_log(&format!("[feedback] non-success status={status}"));
            Err(CommandErrorVm::new(
                "feedback.server-error",
                serde_json::json!({ "status": status }),
            ))
        }
        Err(error) => {
            metrics_log(&format!("[feedback] network failure: {error}"));
            Err(CommandErrorVm::new(
                "feedback.network-failed",
                serde_json::json!({}),
            ))
        }
    }
}

#[tauri::command]
pub async fn preview_feedback_session_archive(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: Option<String>,
) -> CommandResult<Option<FeedbackArchivePreview>> {
    ensure_feedback_enabled(current_channel_config().feedback_enabled)?;
    match (&project_id, &task_id) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => {}
        _ => return Err(validation_error("session", "incomplete")),
    }
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let plan = resolve_session_archive_plan(
            &context,
            project_id.as_deref().unwrap_or_default(),
            task_id.as_deref().unwrap_or_default(),
            ARCHIVE_POLICY,
        )?;
        Ok(Some(FeedbackArchivePreview {
            uncompressed_bytes: plan.uncompressed_bytes,
            file_count: plan.file_count,
            within_limits: plan.within_limits,
            max_uncompressed_bytes: plan.policy.max_uncompressed_bytes,
            max_file_count: plan.policy.max_file_count,
        }))
    })
    .await
}

fn ensure_feedback_enabled(enabled: bool) -> CommandResult<()> {
    if enabled {
        Ok(())
    } else {
        Err(CommandErrorVm::new(
            "feedback.disabled",
            serde_json::json!({}),
        ))
    }
}

fn resolve_endpoint(config: &RuntimeConfig) -> Option<String> {
    metrics_base_url(config)
        .as_deref()
        .and_then(|base| endpoint_from_base_url(base, FEEDBACK_ENDPOINT_PATH))
}

fn validate_input(input: &FeedbackInput) -> CommandResult<()> {
    if input.description.trim().is_empty() {
        return Err(validation_error("description", "empty"));
    }
    if input.description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(validation_error("description", "too-long"));
    }
    match (&input.project_id, &input.task_id) {
        (None, None) => {}
        (Some(project_id), Some(task_id)) => {
            if project_id.trim().is_empty() {
                return Err(validation_error("session", "invalid-project"));
            }
            validate_task_id(task_id)?;
        }
        _ => return Err(validation_error("session", "incomplete")),
    }
    if input.screenshots.len() > MAX_SCREENSHOTS {
        return Err(validation_error("screenshots", "too-many"));
    }
    for screenshot in &input.screenshots {
        if screenshot.name.trim().is_empty() || screenshot.name.chars().count() > 255 {
            return Err(attachment_error("invalid-name"));
        }
        if screenshot.size > MAX_SCREENSHOT_BYTES as u64
            || screenshot.data_base64.len() > MAX_SCREENSHOT_BASE64_CHARS
        {
            return Err(payload_too_large("screenshot"));
        }
    }
    Ok(())
}

fn validate_task_id(task_id: &str) -> CommandResult<()> {
    let mut components = Path::new(task_id).components();
    let single_normal =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if task_id.trim().is_empty()
        || task_id.len() > 128
        || task_id.contains('/')
        || task_id.contains('\\')
        || Path::new(task_id).is_absolute()
        || !single_normal
    {
        return Err(validation_error("session", "invalid-task"));
    }
    Ok(())
}

fn prepare_screenshots(
    screenshots: &[FeedbackScreenshotInput],
) -> CommandResult<Vec<PreparedScreenshot>> {
    screenshots
        .iter()
        .enumerate()
        .map(|(index, screenshot)| normalize_screenshot(screenshot, index))
        .collect()
}

fn normalize_screenshot(
    screenshot: &FeedbackScreenshotInput,
    index: usize,
) -> CommandResult<PreparedScreenshot> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&screenshot.data_base64)
        .map_err(|_| attachment_error("invalid-base64"))?;
    if bytes.len() > MAX_SCREENSHOT_BYTES || screenshot.size != bytes.len() as u64 {
        return Err(attachment_error("size-mismatch"));
    }

    let format = image::guess_format(&bytes).map_err(|_| attachment_error("invalid-image"))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) || !mime_matches_format(&screenshot.mime, format)
    {
        return Err(attachment_error("unsupported-image"));
    }

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SCREENSHOT_DIMENSION);
    limits.max_image_height = Some(MAX_SCREENSHOT_DIMENSION);
    limits.max_alloc = Some(MAX_SCREENSHOT_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| attachment_error("invalid-image"))?;
    let mut output = Cursor::new(Vec::new());
    decoded
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| attachment_error("invalid-image"))?;
    let bytes = output.into_inner();
    if bytes.len() > MAX_SCREENSHOT_BYTES {
        return Err(payload_too_large("normalized-screenshot"));
    }
    Ok(PreparedScreenshot {
        file_name: format!("screenshot_{index}.png"),
        bytes,
    })
}

fn mime_matches_format(mime: &str, format: ImageFormat) -> bool {
    matches!(
        (mime.trim().to_ascii_lowercase().as_str(), format),
        ("image/png", ImageFormat::Png)
            | ("image/jpeg", ImageFormat::Jpeg)
            | ("image/jpg", ImageFormat::Jpeg)
            | ("image/webp", ImageFormat::WebP)
    )
}

fn resolve_session_archive_plan(
    context: &DesktopContext,
    project_id: &str,
    task_id: &str,
    policy: ArchivePolicy,
) -> CommandResult<ArchivePlan> {
    validate_task_id(task_id)?;
    let global_app = context.app();
    let state = global_app.load_state().map_err(|error| {
        metrics_log(&format!(
            "[feedback] failed to load workspace state: {error}"
        ));
        internal_error()
    })?;
    let workspace_path = resolve_feedback_workspace(&state, project_id)?;
    let workspace_app = app_for_workspace(context, &workspace_path).map_err(|error| {
        metrics_log(&format!("[feedback] failed to open workspace app: {error}"));
        internal_error()
    })?;
    plan_task_dir(&workspace_app.paths, task_id, policy)
}

fn resolve_feedback_workspace(state: &StateConfig, project_id: &str) -> CommandResult<String> {
    workspace_entry_for_project(state, project_id)
        .map(|(workspace_path, _)| workspace_path)
        .ok_or_else(session_not_found)
}

fn plan_task_dir(
    paths: &GoldBandPaths,
    task_id: &str,
    policy: ArchivePolicy,
) -> CommandResult<ArchivePlan> {
    validate_task_id(task_id)?;
    let task_dir = paths.task_dir(task_id);
    if !task_dir.is_dir() || !paths.task_file(task_id).is_file() {
        return Err(session_not_found());
    }
    let tasks_root =
        fs::canonicalize(paths.tasks_dir().as_std_path()).map_err(|_| session_not_found())?;
    let task_root = fs::canonicalize(task_dir.as_std_path()).map_err(|_| session_not_found())?;
    if !task_root.starts_with(&tasks_root) {
        return Err(session_not_found());
    }

    let mut files = Vec::new();
    let mut uncompressed_bytes = 0_u64;
    let mut file_count = 0_usize;
    let mut within_limits = true;
    for entry in WalkDir::new(&task_root).follow_links(false) {
        let entry = entry.map_err(|_| session_not_found())?;
        let file_type = entry.file_type();
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let source = fs::canonicalize(entry.path()).map_err(|_| session_not_found())?;
        if !source.starts_with(&task_root) || !source.starts_with(&tasks_root) {
            return Err(session_not_found());
        }
        let metadata = fs::metadata(&source).map_err(|_| session_not_found())?;
        file_count = file_count.saturating_add(1);
        uncompressed_bytes = uncompressed_bytes.saturating_add(metadata.len());
        if file_count > policy.max_file_count || uncompressed_bytes > policy.max_uncompressed_bytes
        {
            within_limits = false;
            break;
        }
        let relative = source
            .strip_prefix(&task_root)
            .map_err(|_| session_not_found())?;
        let zip_name = relative.to_string_lossy().replace('\\', "/");
        if zip_name.is_empty() {
            continue;
        }
        files.push(ArchiveFile {
            source,
            zip_name,
            size: metadata.len(),
        });
    }

    Ok(ArchivePlan {
        task_root,
        files,
        uncompressed_bytes,
        file_count,
        within_limits,
        policy,
    })
}

fn build_archive(plan: ArchivePlan) -> CommandResult<PreparedArchive> {
    if !plan.within_limits {
        return Err(payload_too_large("session-archive"));
    }
    let mut temp = NamedTempFile::new().map_err(|_| internal_error())?;
    let mut actual_uncompressed = 0_u64;
    {
        let mut zip = zip::ZipWriter::new(temp.as_file_mut());
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for entry in &plan.files {
            let canonical = fs::canonicalize(&entry.source).map_err(|_| session_not_found())?;
            if !canonical.starts_with(&plan.task_root) {
                return Err(session_not_found());
            }
            let metadata = fs::metadata(&canonical).map_err(|_| session_not_found())?;
            if metadata.len() != entry.size {
                return Err(session_not_found());
            }
            actual_uncompressed = actual_uncompressed.saturating_add(metadata.len());
            if actual_uncompressed > plan.policy.max_uncompressed_bytes {
                return Err(payload_too_large("session-archive"));
            }
            zip.start_file(&entry.zip_name, options)
                .map_err(|_| internal_error())?;
            let mut source = File::open(&canonical).map_err(|_| session_not_found())?;
            std::io::copy(&mut source, &mut zip).map_err(|_| internal_error())?;
        }
        zip.finish().map_err(|_| internal_error())?;
    }
    temp.as_file_mut().flush().map_err(|_| internal_error())?;
    let compressed_bytes = temp
        .as_file()
        .metadata()
        .map_err(|_| internal_error())?
        .len();
    if compressed_bytes > plan.policy.max_compressed_bytes {
        return Err(payload_too_large("session-archive"));
    }
    Ok(PreparedArchive {
        file: temp,
        compressed_bytes,
        uncompressed_bytes: actual_uncompressed,
        file_count: plan.files.len(),
    })
}

fn ensure_request_size(
    metadata_bytes: u64,
    description_bytes: u64,
    log_bytes: Option<&[u8]>,
    screenshots: &[PreparedScreenshot],
    archive: Option<&PreparedArchive>,
) -> CommandResult<()> {
    let screenshots_bytes = screenshots.iter().fold(0_u64, |total, item| {
        total.saturating_add(item.bytes.len() as u64)
    });
    let total = metadata_bytes
        .saturating_add(description_bytes)
        .saturating_add(log_bytes.map(|bytes| bytes.len() as u64).unwrap_or(0))
        .saturating_add(screenshots_bytes)
        .saturating_add(archive.map(|item| item.compressed_bytes).unwrap_or(0));
    if total > MAX_REQUEST_BYTES {
        Err(payload_too_large("request"))
    } else {
        Ok(())
    }
}

fn feedback_part_names(has_log: bool, has_archive: bool, screenshot_count: usize) -> Vec<String> {
    let mut names = vec!["metadata".to_string(), "description".to_string()];
    if has_log {
        names.push("log".to_string());
    }
    if has_archive {
        names.push("session_archive".to_string());
    }
    names.extend((0..screenshot_count).map(|index| format!("screenshot_{index}")));
    names
}

/// 只读取日志尾部，避免为大日志分配完整文件大小的内存。
fn read_log_tail(paths: &GoldBandPaths) -> Option<Vec<u8>> {
    let mut file = File::open(paths.runtime_log_file().as_std_path()).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes).ok()?;
    if start > 0 {
        while bytes.first().is_some_and(|byte| byte & 0xC0 == 0x80) {
            bytes.remove(0);
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned().into_bytes())
}

fn validation_error(field: &str, reason: &str) -> CommandErrorVm {
    CommandErrorVm::new(
        "feedback.validation-failed",
        serde_json::json!({ "field": field, "reason": reason }),
    )
}

fn attachment_error(reason: &str) -> CommandErrorVm {
    CommandErrorVm::new(
        "feedback.attachment-invalid",
        serde_json::json!({ "reason": reason }),
    )
}

fn payload_too_large(part: &str) -> CommandErrorVm {
    CommandErrorVm::new(
        "feedback.payload-too-large",
        serde_json::json!({ "part": part }),
    )
}

fn session_not_found() -> CommandErrorVm {
    CommandErrorVm::new("feedback.session-not-found", serde_json::json!({}))
}

fn internal_error() -> CommandErrorVm {
    CommandErrorVm::new("feedback.server-error", serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(description: &str) -> FeedbackInput {
        FeedbackInput {
            description: description.to_string(),
            project_id: None,
            task_id: None,
            screenshots: vec![],
            include_logs: false,
        }
    }

    fn session_paths(root: &tempfile::TempDir) -> GoldBandPaths {
        GoldBandPaths::new_with_path_config(
            camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap(),
            gold_band::storage::StoragePathConfig {
                app_key: "gold-band",
                config_dir_name: ".gold-band",
                home_env_var: "GOLD_BAND_HOME",
            },
        )
    }

    fn create_task(paths: &GoldBandPaths, task_id: &str) {
        fs::create_dir_all(paths.task_dir(task_id).as_std_path()).unwrap();
        fs::write(paths.task_file(task_id).as_std_path(), b"{}").unwrap();
    }

    fn encoded_image(format: ImageFormat, mime: &str) -> FeedbackScreenshotInput {
        let image = image::DynamicImage::new_rgb8(2, 2);
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, format).unwrap();
        let bytes = output.into_inner();
        FeedbackScreenshotInput {
            name: "shot.png".to_string(),
            mime: mime.to_string(),
            size: bytes.len() as u64,
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    #[test]
    fn feedback_capability_is_enforced_by_backend() {
        assert!(ensure_feedback_enabled(true).is_ok());
        assert_eq!(
            ensure_feedback_enabled(false).unwrap_err().code,
            "feedback.disabled"
        );
    }

    #[test]
    fn rejects_invalid_descriptions_and_incomplete_sessions() {
        assert_eq!(
            validate_input(&input("   ")).unwrap_err().params["reason"],
            "empty"
        );
        assert_eq!(
            validate_input(&input(&"x".repeat(MAX_DESCRIPTION_CHARS + 1)))
                .unwrap_err()
                .params["reason"],
            "too-long"
        );
        let mut incomplete = input("ok");
        incomplete.project_id = Some("project-a".to_string());
        assert_eq!(
            validate_input(&incomplete).unwrap_err().params["reason"],
            "incomplete"
        );
    }

    #[test]
    fn rejects_absolute_and_traversal_task_ids() {
        for task_id in ["../task", "a/b", "a\\b", ".", "..", "/tmp/task", "C:\\task"] {
            assert!(validate_task_id(task_id).is_err(), "accepted {task_id}");
        }
        assert!(validate_task_id("task-015").is_ok());
    }

    #[test]
    fn normalizes_supported_images_to_png() {
        let input = encoded_image(ImageFormat::Jpeg, "image/jpeg");
        let prepared = normalize_screenshot(&input, 0).unwrap();
        assert_eq!(&prepared.bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(prepared.file_name, "screenshot_0.png");
    }

    #[test]
    fn rejects_invalid_or_mismatched_images() {
        let invalid = FeedbackScreenshotInput {
            name: "shot.png".to_string(),
            mime: "image/png".to_string(),
            size: 4,
            data_base64: base64::engine::general_purpose::STANDARD.encode(b"nope"),
        };
        assert_eq!(
            normalize_screenshot(&invalid, 0).unwrap_err().code,
            "feedback.attachment-invalid"
        );
        let mismatched = encoded_image(ImageFormat::Jpeg, "image/png");
        assert_eq!(
            normalize_screenshot(&mismatched, 0).unwrap_err().params["reason"],
            "unsupported-image"
        );
    }

    #[test]
    fn archive_plan_rejects_missing_task_and_respects_limits() {
        let root = tempfile::tempdir().unwrap();
        let paths = session_paths(&root);
        assert_eq!(
            plan_task_dir(&paths, "task-missing", ARCHIVE_POLICY)
                .unwrap_err()
                .code,
            "feedback.session-not-found"
        );

        create_task(&paths, "task-015");
        fs::write(
            paths
                .task_dir("task-015")
                .join("events.jsonl")
                .as_std_path(),
            b"12345",
        )
        .unwrap();
        let plan = plan_task_dir(
            &paths,
            "task-015",
            ArchivePolicy {
                max_uncompressed_bytes: 4,
                max_compressed_bytes: 1024,
                max_file_count: 10,
            },
        )
        .unwrap();
        assert!(!plan.within_limits);
        assert!(plan.uncompressed_bytes > 4);

        let count_limited = plan_task_dir(
            &paths,
            "task-015",
            ArchivePolicy {
                max_uncompressed_bytes: 1024,
                max_compressed_bytes: 1024,
                max_file_count: 1,
            },
        )
        .unwrap();
        assert!(!count_limited.within_limits);
        assert_eq!(count_limited.file_count, 2);
    }

    #[test]
    fn unknown_project_is_rejected_before_filesystem_resolution() {
        let state = StateConfig::default();
        assert_eq!(
            resolve_feedback_workspace(&state, "missing-project")
                .unwrap_err()
                .code,
            "feedback.session-not-found"
        );
    }

    #[test]
    fn archive_streams_whole_task_without_following_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let paths = session_paths(&root);
        create_task(&paths, "task-015");
        fs::write(
            paths
                .task_dir("task-015")
                .join("events.jsonl")
                .as_std_path(),
            b"events",
        )
        .unwrap();

        let outside = root.path().join("outside.txt");
        fs::write(&outside, b"secret").unwrap();
        let link = paths.task_dir("task-015").join("outside-link.txt");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, link.as_std_path()).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, link.as_std_path()).is_ok();

        let plan = plan_task_dir(&paths, "task-015", ARCHIVE_POLICY).unwrap();
        if linked {
            assert_eq!(plan.file_count, 2);
        }
        let archive = build_archive(plan).unwrap();
        assert!(archive.compressed_bytes > 0);
        assert_eq!(archive.file_count, 2);
        assert!(archive.uncompressed_bytes >= 8);

        let file = File::open(archive.file.path()).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|index| zip.by_index(index).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|name| name == "task.json"));
        assert!(names.iter().any(|name| name == "events.jsonl"));
        assert!(!names.iter().any(|name| name == "outside-link.txt"));
    }

    #[test]
    fn compressed_archive_limit_is_enforced() {
        let root = tempfile::tempdir().unwrap();
        let paths = session_paths(&root);
        create_task(&paths, "task-015");
        let plan = plan_task_dir(
            &paths,
            "task-015",
            ArchivePolicy {
                max_uncompressed_bytes: 1024,
                max_compressed_bytes: 1,
                max_file_count: 10,
            },
        )
        .unwrap();
        assert_eq!(
            build_archive(plan).unwrap_err().code,
            "feedback.payload-too-large"
        );
    }

    #[test]
    fn multipart_contract_includes_description_and_png_screenshots() {
        assert_eq!(
            feedback_part_names(true, true, 2),
            vec![
                "metadata",
                "description",
                "log",
                "session_archive",
                "screenshot_0",
                "screenshot_1"
            ]
        );
        assert_eq!(LOG_MIME, "text/plain");
        assert_eq!(ARCHIVE_MIME, "application/zip");
        assert_eq!(SCREENSHOT_MIME, "image/png");
    }

    #[test]
    fn log_tail_is_bounded_and_utf8() {
        let root = tempfile::tempdir().unwrap();
        let paths = session_paths(&root);
        fs::create_dir_all(paths.logs_dir()).unwrap();
        let content = "界".repeat(LOG_TAIL_BYTES);
        fs::write(paths.runtime_log_file().as_std_path(), content.as_bytes()).unwrap();
        let tail = read_log_tail(&paths).unwrap();
        assert!(tail.len() <= LOG_TAIL_BYTES + 3);
        String::from_utf8(tail).unwrap();
    }

    #[test]
    fn endpoint_resolution_reuses_metrics_base_url() {
        let mut config = RuntimeConfig::default();
        config.desktop_metrics_base_url = Some("https://maling.example.com".to_string());
        assert_eq!(
            resolve_endpoint(&config).as_deref(),
            Some("https://maling.example.com/api/client-report/feedback")
        );
    }
}
