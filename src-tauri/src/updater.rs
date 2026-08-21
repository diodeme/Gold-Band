use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use gold_band::config::RuntimeConfig;
use gold_band::storage::atomic_write_file;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;

use crate::{channel::current_channel_config, state::DesktopState};

const POLL_INTERVAL_MINUTES: u64 = 240;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterSettingsVm {
    pub channel: String,
    pub built_in_url: String,
    pub override_url: Option<String>,
    pub effective_url: String,
    pub poll_interval_minutes: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateCheckStatus {
    Idle,
    Checking,
    Available,
    #[allow(dead_code)]
    Downloading,
    NotAvailable,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfoVm {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateErrorVm {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusVm {
    pub status: UpdateCheckStatus,
    pub checked_at: Option<String>,
    pub update: Option<UpdateInfoVm>,
    pub error: Option<UpdateErrorVm>,
    pub background: bool,
}

struct UpdateCheckOutcome {
    status: UpdateStatusVm,
    update: Option<Update>,
}

pub fn initial_update_status(checked_at: Option<String>) -> UpdateStatusVm {
    UpdateStatusVm {
        status: UpdateCheckStatus::Idle,
        checked_at,
        update: None,
        error: None,
        background: false,
    }
}

pub fn updater_settings(config: &RuntimeConfig) -> UpdaterSettingsVm {
    let channel_config = current_channel_config();
    let built_in_url = channel_config.updater_endpoint.to_string();
    let override_url = config.desktop_updater_url_override.clone();
    let effective_url = override_url.clone().unwrap_or_else(|| built_in_url.clone());
    UpdaterSettingsVm {
        channel: channel_config.channel.to_string(),
        built_in_url,
        override_url,
        effective_url,
        poll_interval_minutes: POLL_INTERVAL_MINUTES,
    }
}

pub fn normalize_updater_url_override(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
    else {
        return Ok(None);
    };
    validate_updater_url(&value)?;
    Ok(Some(value))
}

pub fn validate_updater_url(value: &str) -> Result<()> {
    let parsed = Url::parse(value).map_err(|_| anyhow!("updater.invalid-url"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if current_channel_config().allow_http_updater || cfg!(debug_assertions) => Ok(()),
        _ => Err(anyhow!("updater.invalid-url")),
    }
}

pub fn start_update_polling<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(90)).await;
        loop {
            if let Err(e) =
                poll_update_once(&app, current_channel_config().silent_update_enabled).await
            {
                eprintln!("Background critical download failed: {e}");
            }
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_MINUTES * 60)).await;
        }
    });
}

pub async fn check_update<R: Runtime>(app: &AppHandle<R>, background: bool) -> UpdateStatusVm {
    perform_update_check(app, background).await.status
}

async fn poll_update_once<R: Runtime>(
    app: &AppHandle<R>,
    silent_update_enabled: bool,
) -> Result<()> {
    let outcome = perform_update_check(app, true).await;
    if silent_update_enabled && let Some(update) = outcome.update.as_ref() {
        try_background_download(app, update).await?;
    }
    Ok(())
}

async fn perform_update_check<R: Runtime>(
    app: &AppHandle<R>,
    background: bool,
) -> UpdateCheckOutcome {
    let checking = UpdateStatusVm {
        status: UpdateCheckStatus::Checking,
        checked_at: None,
        update: None,
        error: None,
        background,
    };
    if let Some(state) = app.try_state::<DesktopState>() {
        let _ = state.set_update_status(checking);
    }

    let checked_at = current_timestamp();
    let (status, update) = match check_update_inner(app).await {
        Ok(Some(update)) => {
            let info = update_info(&update);
            (
                UpdateStatusVm {
                    status: UpdateCheckStatus::Available,
                    checked_at: Some(checked_at.clone()),
                    update: Some(info),
                    error: None,
                    background,
                },
                Some(update),
            )
        }
        Ok(None) => (
            UpdateStatusVm {
                status: UpdateCheckStatus::NotAvailable,
                checked_at: Some(checked_at.clone()),
                update: None,
                error: None,
                background,
            },
            None,
        ),
        Err(error) => (
            UpdateStatusVm {
                status: UpdateCheckStatus::Error,
                checked_at: Some(checked_at.clone()),
                update: None,
                error: Some(UpdateErrorVm {
                    code: updater_error_code(&error),
                    params: serde_json::json!({ "message": error.to_string() }),
                }),
                background,
            },
            None,
        ),
    };

    if let Some(state) = app.try_state::<DesktopState>() {
        let _ = state.persist_updater_last_checked_at(Some(checked_at));
        let _ = state.set_update_status(status.clone());
        let _ = state.persist_available_update(status.update.clone());
    }
    if matches!(
        status.status,
        UpdateCheckStatus::Available | UpdateCheckStatus::NotAvailable | UpdateCheckStatus::Error
    ) {
        let _ = app.emit("gold-band://update-status", &status);
    }
    UpdateCheckOutcome { status, update }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDownloadProgress {
    downloaded: usize,
    total: Option<u64>,
}

pub async fn download_and_install_update<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    // 清除后台静默下载的文件，防止退出时重复安装
    if let Some(state) = app.try_state::<DesktopState>() {
        if let Some(path) = state.take_pending_update() {
            let _ = std::fs::remove_file(path.as_std_path());
            let _ = std::fs::remove_dir(pending_update_dir());
        }
    }

    let updater = build_updater(app)?;
    let Some(update) = updater.check().await.context("updater.check-failed")? else {
        return Err(anyhow!("updater.no-update"));
    };
    let app_handle = app.clone();
    let cumulative = Arc::new(Mutex::new(0usize));
    update
        .download_and_install(
            {
                let cumulative = cumulative.clone();
                move |chunk_size, total| {
                    let mut acc = cumulative.lock().unwrap();
                    *acc += chunk_size;
                    let _ = app_handle.emit(
                        "gold-band://update-download-progress",
                        UpdateDownloadProgress {
                            downloaded: *acc,
                            total,
                        },
                    );
                }
            },
            || {},
        )
        .await
        .context("updater.install-failed")?;
    Ok(())
}

async fn check_update_inner<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Update>> {
    let updater = build_updater(app)?;
    updater.check().await.context("updater.check-failed")
}

fn update_info(update: &Update) -> UpdateInfoVm {
    UpdateInfoVm {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        pub_date: update.date.map(|date| date.to_string()),
    }
}

fn build_updater<R: Runtime>(app: &AppHandle<R>) -> Result<tauri_plugin_updater::Updater> {
    let state = app.state::<DesktopState>();
    let context = state.context().context("updater.context-unavailable")?;
    let config = context.config;
    let settings = updater_settings(&config);
    validate_updater_url(&settings.effective_url)?;
    let endpoint = Url::parse(&settings.effective_url).context("updater.invalid-url")?;
    app.updater_builder()
        .pubkey(current_channel_config().updater_public_key)
        .endpoints(vec![endpoint])
        .context("updater.invalid-url")?
        .build()
        .context("updater.check-failed")
}

fn updater_error_code(error: &anyhow::Error) -> String {
    let message = error.to_string();
    if message.contains("updater.invalid-url") {
        "updater.invalid-url".to_string()
    } else if message.contains("updater.context-unavailable") {
        "updater.context-unavailable".to_string()
    } else if message.contains("updater.no-update") {
        "updater.no-update".to_string()
    } else if message.contains("updater.install-failed") {
        "updater.install-failed".to_string()
    } else {
        "updater.check-failed".to_string()
    }
}

fn current_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ── Silent / background critical update ──

fn pending_update_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("gold-band-update")
}

/// 复用本轮检查到的关键更新并静默下载到文件，不安装
async fn try_background_download<R: Runtime>(app: &AppHandle<R>, update: &Update) -> Result<()> {
    let is_critical = update
        .raw_json
        .get("critical")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_critical {
        return Ok(());
    }

    let dir = pending_update_dir();
    let path = dir.join(format!("update-{}.pkg", update.version));
    let path =
        camino::Utf8PathBuf::from_path_buf(path).map_err(|_| anyhow::anyhow!("non-UTF-8 path"))?;
    let state = app.state::<DesktopState>();
    let pending = state.pending_update_path()?;
    if pending_update_is_ready(pending.as_deref(), &path) {
        return Ok(());
    }

    let bytes = update.download(|_chunk, _total| {}, || {}).await?;
    let write_path = path.clone();
    tokio::task::spawn_blocking(move || write_pending_update(&write_path, &bytes))
        .await
        .context("updater.pending-write-task-failed")??;
    state.store_pending_update(path)?;

    Ok(())
}

fn pending_update_is_ready(
    pending: Option<&camino::Utf8Path>,
    expected: &camino::Utf8Path,
) -> bool {
    pending == Some(expected) && expected.is_file()
}

fn write_pending_update(path: &camino::Utf8Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_write_file(path.as_std_path(), |file| -> Result<()> {
        file.write_all(bytes)?;
        Ok(())
    })
}

/// 从文件路径安装更新包
/// 先删文件再 install：Windows NSIS 安装器会重启 App 杀死当前进程，
/// 若 install 后删文件可能没机会执行，残留文件导致下次启动死循环
pub async fn install_pending_file<R: Runtime>(
    app: &AppHandle<R>,
    path: &camino::Utf8Path,
) -> Result<()> {
    let bytes = std::fs::read(path.as_std_path()).context("failed to read pending update file")?;
    let updater = build_updater(app)?;
    let Some(update) = updater.check().await.context("updater.check-failed")? else {
        let _ = std::fs::remove_file(path.as_std_path());
        let _ = std::fs::remove_dir(pending_update_dir());
        return Err(anyhow!("updater.no-update"));
    };
    // 先删再装——即使 install 内 App 被重启，文件也不残留
    let _ = std::fs::remove_file(path.as_std_path());
    let _ = std::fs::remove_dir(pending_update_dir());
    update.install(bytes).context("updater.install-failed")?;
    Ok(())
}

/// 启动时检查 /tmp 是否有上次未安装成功的残留包
pub fn retry_pending_startup_install<R: Runtime>(app: &AppHandle<R>) {
    let dir = pending_update_dir();
    if !dir.is_dir() {
        return;
    }
    // 清理空目录（之前完全处理完的）
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let handle = app.clone();
                let utf8_path = match camino::Utf8PathBuf::from_path_buf(path.clone()) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                tauri::async_runtime::spawn(async move {
                    let _ = install_pending_file(&handle, &utf8_path).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        pending_update_is_ready, poll_update_once, validate_updater_url, write_pending_update,
    };
    use crate::state::{DesktopContext, DesktopState};
    use gold_band::config::RuntimeConfig;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;
    use tauri::Manager;

    fn mock_app(endpoint: String) -> (tauri::App<tauri::test::MockRuntime>, tempfile::TempDir) {
        let root = tempfile::tempdir().unwrap();
        let repo_root = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        let mut config = RuntimeConfig::default();
        config.desktop_updater_url_override = Some(endpoint);
        let context = DesktopContext {
            repo_root,
            config,
            recent_workspaces: Vec::new(),
            needs_workspace: false,
        };
        let mut tauri_context = tauri::test::mock_context(tauri::test::noop_assets());
        tauri_context.config_mut().plugins.0.insert(
            "updater".to_string(),
            serde_json::json!({
                "pubkey": crate::channel::current_channel_config().updater_public_key,
                "endpoints": [],
                "windows": null,
            }),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(tauri_context)
            .unwrap();
        app.manage(DesktopState::new(context));
        (app, root)
    }

    fn update_server(response: String) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}/latest.json", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_thread = requests.clone();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            requests_for_thread.fetch_add(1, Ordering::SeqCst);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });
        (endpoint, requests, handle)
    }

    #[test]
    fn accepts_https_updater_url() {
        validate_updater_url(
            "https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json",
        )
        .unwrap();
    }

    #[test]
    fn rejects_invalid_updater_url() {
        assert!(validate_updater_url("not a url").is_err());
    }

    #[test]
    fn polling_reuses_one_manifest_check_for_silent_channel() {
        let (endpoint, requests, server) = update_server(
            serde_json::json!({
                "version": "999.0.0",
                "notes": "test",
                "pub_date": "2025-01-01T00:00:00Z",
                "url": "http://127.0.0.1/package",
                "signature": "test-signature",
                "critical": false,
            })
            .to_string(),
        );
        let (app, _root) = mock_app(endpoint);

        tauri::async_runtime::block_on(poll_update_once(&app.handle(), true)).unwrap();
        server.join().unwrap();

        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pending_update_is_ready_only_for_existing_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let expected = camino::Utf8PathBuf::from_path_buf(dir.path().join("update.pkg")).unwrap();
        std::fs::write(expected.as_std_path(), b"complete").unwrap();
        let other = camino::Utf8PathBuf::from_path_buf(dir.path().join("other.pkg")).unwrap();

        assert!(pending_update_is_ready(Some(&expected), &expected));
        assert!(!pending_update_is_ready(Some(&other), &expected));

        std::fs::remove_file(expected.as_std_path()).unwrap();
        assert!(!pending_update_is_ready(Some(&expected), &expected));
    }

    #[test]
    fn pending_update_write_commits_complete_bytes_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("nested/update.pkg")).unwrap();

        write_pending_update(&path, b"complete package").unwrap();

        assert_eq!(
            std::fs::read(path.as_std_path()).unwrap(),
            b"complete package"
        );
    }
}
