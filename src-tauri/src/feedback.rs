//! 用户反馈上报领域。
//!
//! 与 metrics 共享上报通道（endpoint 拼接 + 认证 + multipart 构造），
//! 但领域模型独立：本模块只负责主动反馈，不包含崩溃收集。

use std::fs;
use std::io::Write;

use camino::Utf8PathBuf;
use gold_band::config::RuntimeConfig;
use gold_band::storage::GoldBandPaths;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::{CommandErrorVm, CommandResult, command_error};
use crate::metrics::{endpoint_from_base_url, get_api_key, metrics_base_url};
use crate::state::DesktopState;

pub const MAX_DESCRIPTION_CHARS: usize = 2000;
pub const MAX_SCREENSHOTS: usize = 4;
pub const MAX_SCREENSHOT_BYTES: usize = 5 * 1024 * 1024;
pub const LOG_TAIL_BYTES: usize = 512 * 1024;
pub const FEEDBACK_ENDPOINT_PATH: &str = "/api/client-report/feedback";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackInput {
    pub description: String,
    #[serde(default)]
    pub session_workspace: Option<String>,
    #[serde(default)]
    pub session_task_id: Option<String>,
    #[serde(default)]
    pub screenshot_paths: Vec<String>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedbackMetadata {
    user_id: String,
    client_version: String,
    reported_at: String,
    session_workspace: Option<String>,
    session_task_id: Option<String>,
    log_attached: bool,
    archive_attached: bool,
    archive_bytes: u64,
    screenshot_count: usize,
}

#[tauri::command]
pub async fn submit_feedback(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    input: FeedbackInput,
) -> CommandResult<FeedbackResult> {
    let context = state.context().map_err(command_error)?;
    let repo_root = context.repo_root.clone();
    let config = context.config.clone();

    validate_input(&input)?;

    let endpoint = resolve_endpoint(&config);
    let api_key = get_api_key(&config);
    let Some(endpoint) = endpoint else {
        return Err(CommandErrorVm::new(
            "feedback.endpoint-unconfigured",
            serde_json::json!({}),
        ));
    };

    let log_paths = GoldBandPaths::new(repo_root.clone());
    let client_version = app_handle.package_info().version.to_string();
    let reported_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let user_id = crate::metrics::get_system_username();

    let log_bytes = if input.include_logs {
        read_log_tail(&log_paths)
    } else {
        None
    };
    let archive_bytes = match (&input.session_workspace, &input.session_task_id) {
        (Some(workspace), Some(task_id)) => {
            let session_paths = GoldBandPaths::new(Utf8PathBuf::from(workspace));
            archive_task_dir(&session_paths, task_id)
        }
        _ => None,
    };

    let metadata = FeedbackMetadata {
        user_id,
        client_version,
        reported_at,
        session_workspace: input.session_workspace.clone(),
        session_task_id: input.session_task_id.clone(),
        log_attached: log_bytes.is_some(),
        archive_attached: archive_bytes.is_some(),
        archive_bytes: archive_bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0),
        screenshot_count: input.screenshot_paths.len(),
    };
    let metadata_json = serde_json::to_string(&metadata).unwrap_or_default();

    let parts = collect_feedback_parts(
        metadata_json,
        input.description.clone(),
        log_bytes.clone(),
        archive_bytes.clone(),
        &input.screenshot_paths,
    );
    let form = build_feedback_form(
        &parts,
        &input.description,
        log_bytes.as_deref(),
        archive_bytes.as_deref(),
        &input.screenshot_paths,
    );

    let client = reqwest::Client::new();
    let mut request = client.post(&endpoint).multipart(form);
    if let Some(api_key) = api_key {
        request = request.header("X-Maling-Report-Key", api_key);
    }

    match request.send().await {
        Ok(resp) if resp.status().is_success() => Ok(FeedbackResult { success: true }),
        Ok(resp) => {
            tracing::warn!(status = %resp.status(), "feedback upload non-success");
            Err(CommandErrorVm::new(
                "feedback.server-error",
                serde_json::json!({ "status": resp.status().as_u16() }),
            ))
        }
        Err(err) => {
            tracing::warn!(%err, "feedback upload network failed");
            Err(CommandErrorVm::new(
                "feedback.network-failed",
                serde_json::json!({ "message": err.to_string() }),
            ))
        }
    }
}

/// Preview the session archive (uncompressed size + file count) so the
/// feedback dialog can show the user how much will be uploaded before they
/// commit. Returns null when the task directory does not exist.
#[tauri::command]
pub fn preview_feedback_session_archive(
    state: State<'_, DesktopState>,
    session_workspace: Option<String>,
    session_task_id: Option<String>,
) -> CommandResult<Option<FeedbackArchivePreview>> {
    let (Some(workspace), Some(task_id)) = (session_workspace, session_task_id) else {
        return Ok(None);
    };
    let context = state.context().map_err(command_error)?;
    let _ = context.repo_root.clone();
    let session_paths = GoldBandPaths::new(Utf8PathBuf::from(workspace));
    Ok(preview_task_dir(&session_paths, &task_id).map(|(bytes, file_count)| FeedbackArchivePreview {
        uncompressed_bytes: bytes,
        file_count,
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackArchivePreview {
    pub uncompressed_bytes: u64,
    pub file_count: usize,
}

fn resolve_endpoint(config: &RuntimeConfig) -> Option<String> {
    metrics_base_url(config)
        .as_deref()
        .and_then(|base| endpoint_from_base_url(base, FEEDBACK_ENDPOINT_PATH))
}

/// One upload part, described independently of reqwest so the part contract is
/// unit-testable without a live HTTP endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PartSpec {
    Text { name: String, value: String },
    File { name: String, file_name: String, mime: String },
}

/// Decide which parts to upload, in order. metadata + description are always
/// present; log / session_snapshot / screenshots are optional. Screenshots that
/// cannot be read are silently skipped (best-effort), matching prior behaviour.
#[allow(clippy::too_many_arguments)]
fn collect_feedback_parts(
    metadata_json: String,
    description: String,
    log_bytes: Option<Vec<u8>>,
    archive_bytes: Option<Vec<u8>>,
    screenshot_paths: &[String],
) -> Vec<PartSpec> {
    let mut parts = vec![
        PartSpec::Text { name: "metadata".to_string(), value: metadata_json },
        // description was previously validated then dropped — never uploaded.
        // Keep it as an explicit, always-present text part so the console
        // receives the user's problem description.
        PartSpec::Text { name: "description".to_string(), value: description },
    ];
    if log_bytes.is_some() {
        parts.push(PartSpec::File {
            name: "log".to_string(),
            file_name: "runtime.log".to_string(),
            mime: "text/plain".to_string(),
        });
    }
    if archive_bytes.is_some() {
        parts.push(PartSpec::File {
            name: "session_archive".to_string(),
            file_name: "task.zip".to_string(),
            mime: "application/zip".to_string(),
        });
    }
    for (idx, path) in screenshot_paths.iter().enumerate() {
        if fs::metadata(path).is_ok() {
            parts.push(PartSpec::File {
                name: format!("screenshot_{idx}"),
                file_name: format!("screenshot_{idx}.png"),
                mime: "image/png".to_string(),
            });
        }
    }
    parts
}

/// Turn part specs into a reqwest multipart form. Bytes are sourced from the
/// same inputs the specs were derived from (kept in lockstep by caller).
fn build_feedback_form(
    parts: &[PartSpec],
    description: &str,
    log_bytes: Option<&[u8]>,
    archive_bytes: Option<&[u8]>,
    screenshot_paths: &[String],
) -> reqwest::multipart::Form {
    let mut form = reqwest::multipart::Form::new();
    for spec in parts {
        match spec {
            PartSpec::Text { name, value } => {
                form = form.text(name.clone(), value.clone());
            }
            PartSpec::File { name, file_name, mime } => {
                let bytes: Vec<u8> = match name.as_str() {
                    "log" => log_bytes.map(|b| b.to_vec()).unwrap_or_default(),
                    "session_archive" => archive_bytes.map(|b| b.to_vec()).unwrap_or_default(),
                    other if other.starts_with("screenshot_") => {
                        let idx: usize = other
                            .trim_start_matches("screenshot_")
                            .parse()
                            .unwrap_or(0);
                        screenshot_paths
                            .get(idx)
                            .and_then(|p| fs::read(p).ok())
                            .unwrap_or_default()
                    }
                    _ => {
                        if name == "description" {
                            description.as_bytes().to_vec()
                        } else {
                            Vec::new()
                        }
                    }
                };
                let part = reqwest::multipart::Part::bytes(bytes)
                    .file_name(file_name.clone())
                    .mime_str(mime)
                    .unwrap_or_else(|_| reqwest::multipart::Part::bytes(Vec::new()));
                form = form.part(name.clone(), part);
            }
        }
    }
    form
}
fn validate_input(input: &FeedbackInput) -> CommandResult<()> {
    if input.description.trim().is_empty() {
        return Err(CommandErrorVm::new(
            "feedback.validation-failed",
            serde_json::json!({ "field": "description", "reason": "empty" }),
        ));
    }
    if input.description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(CommandErrorVm::new(
            "feedback.validation-failed",
            serde_json::json!({ "field": "description", "reason": "too-long" }),
        ));
    }
    if input.screenshot_paths.len() > MAX_SCREENSHOTS {
        return Err(CommandErrorVm::new(
            "feedback.validation-failed",
            serde_json::json!({ "field": "screenshots", "reason": "too-many" }),
        ));
    }
    for path in &input.screenshot_paths {
        match fs::metadata(path) {
            Ok(meta) if (meta.len() as usize) > MAX_SCREENSHOT_BYTES => {
                return Err(CommandErrorVm::new(
                    "feedback.validation-failed",
                    serde_json::json!({ "field": "screenshots", "reason": "file-too-large", "path": path }),
                ));
            }
            Err(_) => {
                return Err(CommandErrorVm::new(
                    "feedback.validation-failed",
                    serde_json::json!({ "field": "screenshots", "reason": "not-found", "path": path }),
                ));
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

/// 读取运行日志尾部 LOG_TAIL_BYTES 字节，保证截断点在 UTF-8 字符边界上。
fn read_log_tail(paths: &GoldBandPaths) -> Option<Vec<u8>> {
    let log_path = paths.runtime_log_file();
    let bytes = fs::read(log_path.as_std_path()).ok()?;
    if bytes.len() <= LOG_TAIL_BYTES {
        return Some(bytes);
    }
    let start = bytes.len() - LOG_TAIL_BYTES;
    let mut cut = start;
    while cut < bytes.len() && (bytes[cut] & 0xC0) == 0x80 {
        cut += 1;
    }
    Some(bytes[cut..].to_vec())
}

/// Zip the entire task directory into bytes for upload. The console receives
/// the full session context (task/workflow/run/round/node metadata, events,
/// snapshots, attachments) so it can reconstruct what happened. This replaces
/// the previous single-file snapshot upload.
fn archive_task_dir(paths: &GoldBandPaths, task_id: &str) -> Option<Vec<u8>> {
    let task_dir = paths.task_dir(task_id);
    if !task_dir.is_dir() {
        return None;
    }
    let entries = walkdir(&task_dir);
    let mut buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::SimpleFileOptions =
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
        for entry in &entries {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(task_dir.as_std_path()) else {
                continue;
            };
            let Some(rel_str) = rel.to_str() else { continue };
            let zip_name = rel_str.replace('\\', "/");
            if path.is_file() {
                if zip.start_file(&zip_name, opts).is_err() {
                    continue;
                }
                if let Ok(data) = fs::read(path) {
                    let _ = zip.write_all(&data);
                }
            } else if rel_str.is_empty() {
                // skip root
            }
        }
        let _ = zip.finish();
    }
    let bytes = buf.into_inner();
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Preview the archive without building it: total uncompressed size + file
/// count, so the feedback dialog can show the user how much will be uploaded.
fn preview_task_dir(paths: &GoldBandPaths, task_id: &str) -> Option<(u64, usize)> {
    let task_dir = paths.task_dir(task_id);
    if !task_dir.is_dir() {
        return None;
    }
    let entries = walkdir(&task_dir);
    let mut total: u64 = 0;
    let mut count: usize = 0;
    for entry in &entries {
        if entry.path().is_file() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
                count += 1;
            }
        }
    }
    if count == 0 {
        None
    } else {
        Some((total, count))
    }
}

fn walkdir(root: &Utf8PathBuf) -> Vec<std::fs::DirEntry> {
    let mut out = Vec::new();
    walkdir_into(root.as_std_path(), &mut out);
    out
}

fn walkdir_into(dir: &std::path::Path, out: &mut Vec<std::fs::DirEntry>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walkdir_into(&path, out);
        } else {
            out.push(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(description: &str) -> FeedbackInput {
        FeedbackInput {
            description: description.to_string(),
            session_workspace: None,
            session_task_id: None,
            screenshot_paths: vec![],
            include_logs: false,
        }
    }

    #[test]
    fn rejects_empty_description() {
        let err = validate_input(&input("   ")).unwrap_err();
        assert_eq!(err.code, "feedback.validation-failed");
        assert_eq!(err.params["field"], "description");
        assert_eq!(err.params["reason"], "empty");
    }

    #[test]
    fn rejects_too_long_description() {
        let long = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
        let err = validate_input(&input(&long)).unwrap_err();
        assert_eq!(err.params["reason"], "too-long");
    }


    #[test]
    fn feedback_parts_always_include_description_and_metadata() {
        // Regression guard: the user's description MUST be uploaded. It was
        // previously validated then dropped, so the console never received it.
        let parts = collect_feedback_parts(
            "{}".to_string(),
            "界面卡住了".to_string(),
            None,
            None,
            &[],
        );
        let names: Vec<String> = parts
            .iter()
            .filter_map(|p| match p {
                PartSpec::Text { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"description".to_string()), "description part missing");
        assert!(names.contains(&"metadata".to_string()), "metadata part missing");

        let desc_value = parts.iter().find_map(|p| match p {
            PartSpec::Text { name, value } if name == "description" => Some(value.clone()),
            _ => None,
        });
        assert_eq!(desc_value.as_deref(), Some("界面卡住了"));
    }

    #[test]
    fn accepts_valid_description() {
        assert!(validate_input(&input("界面卡住了")).is_ok());
    }

    #[test]
    fn rejects_too_many_screenshots() {
        let mut i = input("ok");
        i.screenshot_paths = (0..MAX_SCREENSHOTS + 1).map(|n| format!("/tmp/{n}.png")).collect();
        let err = validate_input(&i).unwrap_err();
        assert_eq!(err.params["field"], "screenshots");
        assert_eq!(err.params["reason"], "too-many");
    }

    #[test]
    fn log_tail_truncates_and_stays_at_char_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let paths_config = gold_band::storage::StoragePathConfig {
            app_key: "gold-band",
            config_dir_name: ".gold-band",
            home_env_var: "GOLD_BAND_HOME",
        };
        let paths = GoldBandPaths::new_with_path_config(
            camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap(),
            paths_config,
        );
        let _ = fs::create_dir_all(paths.logs_dir());
        let big: Vec<u8> = (0..LOG_TAIL_BYTES + 1024)
            .map(|i| b'a' + (i % 26) as u8)
            .collect();
        fs::write(paths.runtime_log_file().as_std_path(), &big).unwrap();
        let tail = read_log_tail(&paths).unwrap();
        assert!(tail.len() <= LOG_TAIL_BYTES);
        assert!(!tail.is_empty());
        String::from_utf8(tail).unwrap();
    }

    #[test]
    fn endpoint_resolution_reuses_metrics_base_url() {
        let mut config = RuntimeConfig::default();
        config.desktop_metrics_base_url = Some("https://maling.example.com".to_string());
        let endpoint = resolve_endpoint(&config);
        assert_eq!(
            endpoint.as_deref(),
            Some("https://maling.example.com/api/client-report/feedback")
        );
    }

   #[test]
   fn endpoint_resolution_returns_none_when_unconfigured() {
        // On channels with a compile-time locked metrics base URL (e.g. "wb"),
        // metrics_base_url always falls back to the channel config, so
        // resolve_endpoint can never return None. The "unconfigured -> None"
        // contract is only reachable on channels that leave the base URL empty.
        if !crate::channel::current_channel_config().metrics_base_url.is_empty() {
            return;
        }
        let config = RuntimeConfig::default();
        assert!(resolve_endpoint(&config).is_none());
   }

    #[test]
    fn metadata_flattens_session_ref_and_includes_user_id() {
        // The console receives a flat metadata JSON: no nested sessionRef,
        // no outer workspace, and userId is always present.
        let metadata = FeedbackMetadata {
            user_id: "alice".to_string(),
            client_version: "0.9.0".to_string(),
            reported_at: "2026-07-28T10:00:00".to_string(),
            session_workspace: Some("/work/B".to_string()),
            session_task_id: Some("task-015".to_string()),
            log_attached: true,
            archive_attached: false,
            archive_bytes: 0,
            screenshot_count: 2,
        };
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["userId"], "alice");
        assert_eq!(json["sessionWorkspace"], "/work/B");
        assert_eq!(json["sessionTaskId"], "task-015");
        // No legacy nested or outer workspace keys.
        assert!(json.get("workspace").is_none());
        assert!(json.get("sessionRef").is_none());
        assert!(json.get("session_ref").is_none());
    }

    #[test]
    fn metadata_omits_session_fields_when_no_session() {
        let metadata = FeedbackMetadata {
            user_id: "bob".to_string(),
            client_version: "0.9.0".to_string(),
            reported_at: "2026-07-28T10:00:00".to_string(),
            session_workspace: None,
            session_task_id: None,
            log_attached: false,
            archive_attached: false,
            archive_bytes: 0,
            screenshot_count: 0,
        };
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["userId"], "bob");
        // Optional fields serialize as null when absent (serde default).
        assert!(json["sessionWorkspace"].is_null());
        assert!(json["sessionTaskId"].is_null());
    }

    fn session_paths_config() -> gold_band::storage::StoragePathConfig {
        gold_band::storage::StoragePathConfig {
            app_key: "gold-band",
            config_dir_name: ".gold-band",
            home_env_var: "GOLD_BAND_HOME",
        }
    }

    fn session_paths(root: &tempfile::TempDir) -> GoldBandPaths {
        GoldBandPaths::new_with_path_config(
            camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap(),
            session_paths_config(),
        )
    }

    #[test]
    fn archive_packs_whole_task_dir_and_resolves_in_session_workspace() {
        // Regression: archive_task_dir must look under sessionWorkspace, not
        // the global repo_root, so cross-workspace feedback can find the task.
        let root = tempfile::tempdir().unwrap();
        let paths = session_paths(&root);
        let attempt_dir = paths
            .task_dir("task-015")
            .join("run-001")
            .join("round-001")
            .join("node-001")
            .join("attempt-001");
        let _ = fs::create_dir_all(attempt_dir.as_std_path());
        fs::write(attempt_dir.join("acp.snapshot.json").as_std_path(), b"{}").unwrap();
        fs::write(paths.task_dir("task-015").join("task.json").as_std_path(), b"{}").unwrap();

        let zip_bytes = archive_task_dir(&paths, "task-015").expect("archive should exist");
        assert!(!zip_bytes.is_empty());
        // The zip should contain both files we created.
        let mut reader = std::io::Cursor::new(&zip_bytes);
        let mut archive = zip::ZipArchive::new(&mut reader).unwrap();
        let names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .collect();
        assert!(names.iter().any(|n| n.contains("task.json")), "names: {names:?}");
        assert!(names.iter().any(|n| n.contains("acp.snapshot.json")), "names: {names:?}");

        // A different workspace's paths do not find the task.
        let other_root = tempfile::tempdir().unwrap();
        let other_paths = session_paths(&other_root);
        assert!(archive_task_dir(&other_paths, "task-015").is_none());
    }

    #[test]
    fn preview_reports_uncompressed_size_and_file_count() {
        let root = tempfile::tempdir().unwrap();
        let paths = session_paths(&root);
        fs::create_dir_all(paths.task_dir("task-015").as_std_path()).unwrap();
        fs::write(paths.task_dir("task-015").join("task.json").as_std_path(), b"hello world").unwrap();
        fs::write(paths.task_dir("task-015").join("events.jsonl").as_std_path(), b"line1\nline2\n").unwrap();

        let (size, count) = preview_task_dir(&paths, "task-015").expect("preview should exist");
        assert_eq!(count, 2);
        assert_eq!(size, (b"hello world".len() + b"line1\nline2\n".len()) as u64);

        // Empty / missing task dir returns None.
        assert!(preview_task_dir(&paths, "task-999").is_none());
    }

    #[test]
    fn parts_include_session_archive_when_present() {
        let parts = collect_feedback_parts(
            "{}".to_string(),
            "desc".to_string(),
            None,
            Some(vec![1, 2, 3]),
            &[],
        );
        assert!(parts.iter().any(|p| matches!(p, PartSpec::File { name, .. } if name == "session_archive")));
        // No archive part when absent.
        let parts2 = collect_feedback_parts("{}".into(), "d".into(), None, None, &[]);
        assert!(!parts2.iter().any(|p| matches!(p, PartSpec::File { name, .. } if name == "session_archive")));
    }
}
