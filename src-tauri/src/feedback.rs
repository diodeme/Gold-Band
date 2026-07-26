//! 用户反馈上报领域。
//!
//! 与 metrics 共享上报通道（endpoint 拼接 + 认证 + multipart 构造），
//! 但领域模型独立：本模块只负责主动反馈，不包含崩溃收集。

use std::fs;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub workspace: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackInput {
    pub description: String,
    #[serde(default)]
    pub session_ref: Option<SessionRef>,
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
    client_version: String,
    reported_at: String,
    workspace: String,
    session_ref: Option<SessionRef>,
    log_attached: bool,
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

    let paths = GoldBandPaths::new(repo_root.clone());
    let client_version = app_handle.package_info().version.to_string();
    let reported_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    let log_bytes = if input.include_logs {
        read_log_tail(&paths)
    } else {
        None
    };
    let snapshot_bytes = match &input.session_ref {
        Some(session_ref) => read_session_snapshot(&paths, &session_ref.task_id),
        None => None,
    };

    let metadata = FeedbackMetadata {
        client_version,
        reported_at,
        workspace: repo_root.to_string(),
        session_ref: input.session_ref.clone(),
        log_attached: log_bytes.is_some(),
        screenshot_count: input.screenshot_paths.len(),
    };
    let metadata_json = serde_json::to_string(&metadata).unwrap_or_default();

    let parts = collect_feedback_parts(
        metadata_json,
        input.description.clone(),
        log_bytes.clone(),
        snapshot_bytes.clone(),
        &input.screenshot_paths,
    );
    let form = build_feedback_form(
        &parts,
        &input.description,
        log_bytes.as_deref(),
        snapshot_bytes.as_deref(),
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
    snapshot_bytes: Option<Vec<u8>>,
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
    if snapshot_bytes.is_some() {
        parts.push(PartSpec::File {
            name: "session_snapshot".to_string(),
            file_name: "acp.snapshot.json".to_string(),
            mime: "application/json".to_string(),
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
    snapshot_bytes: Option<&[u8]>,
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
                    "session_snapshot" => snapshot_bytes.map(|b| b.to_vec()).unwrap_or_default(),
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

fn read_session_snapshot(paths: &GoldBandPaths, task_id: &str) -> Option<Vec<u8>> {
    let task_dir = paths.task_dir(task_id);
    let mut latest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
    for entry in walkdir(&task_dir) {
        if entry.file_name().to_str() == Some("acp.snapshot.json") {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if latest.as_ref().map_or(true, |(_, t)| modified > *t) {
                        latest = Some((entry.path().to_path_buf(), modified));
                    }
                }
            }
        }
    }
    let (path, _) = latest?;
    fs::read(&path).ok()
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
            session_ref: None,
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
        let config = RuntimeConfig::default();
        assert!(resolve_endpoint(&config).is_none());
    }
}
