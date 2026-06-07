use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use gold_band::config::RuntimeConfig;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;
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
    let Some(value) = value.map(|item| item.trim().to_string()).filter(|item| !item.is_empty()) else {
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
            let _ = check_update(&app, true).await;
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_MINUTES * 60)).await;
        }
    });
}

pub async fn check_update<R: Runtime>(app: &AppHandle<R>, background: bool) -> UpdateStatusVm {
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
    let status = match check_update_inner(app).await {
        Ok(Some(update)) => UpdateStatusVm {
            status: UpdateCheckStatus::Available,
            checked_at: Some(checked_at.clone()),
            update: Some(update),
            error: None,
            background,
        },
        Ok(None) => UpdateStatusVm {
            status: UpdateCheckStatus::NotAvailable,
            checked_at: Some(checked_at.clone()),
            update: None,
            error: None,
            background,
        },
        Err(error) => UpdateStatusVm {
            status: UpdateCheckStatus::Error,
            checked_at: Some(checked_at.clone()),
            update: None,
            error: Some(UpdateErrorVm {
                code: updater_error_code(&error),
                params: serde_json::json!({ "message": error.to_string() }),
            }),
            background,
        },
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
    status
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDownloadProgress {
    downloaded: usize,
    total: Option<u64>,
}

pub async fn download_and_install_update<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
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
                        UpdateDownloadProgress { downloaded: *acc, total },
                    );
                }
            },
            || {},
        )
        .await
        .context("updater.install-failed")?;
    app.request_restart();
    Ok(())
}

async fn check_update_inner<R: Runtime>(app: &AppHandle<R>) -> Result<Option<UpdateInfoVm>> {
    let updater = build_updater(app)?;
    let update = updater.check().await.context("updater.check-failed")?;
    Ok(update.map(|update| UpdateInfoVm {
        version: update.version,
        current_version: update.current_version,
        notes: update.body,
        pub_date: update.date.map(|date| date.to_string()),
    }))
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

// ── Silent / startup critical update ──

/// latest.json 顶层结构（仅取需要的字段）
#[derive(Debug, Deserialize)]
struct LatestManifest {
    version: String,
    #[serde(default)]
    critical: bool,
}

/// 启动时关键更新检查结果，发送给前端 splash 画面
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupCheckResult {
    pub critical: bool,
    pub error: Option<String>,
}

/// HTTP GET latest.json，返回解析后的 manifest
async fn fetch_manifest(endpoint: &Url) -> Result<LatestManifest> {
    let response = reqwest::get(endpoint.as_str().to_owned()).await?;
    let manifest: LatestManifest = response.json().await?;
    Ok(manifest)
}

/// 持久化结果到 DesktopState 并 emit 事件（避免竞态）
fn emit_startup_check<R: Runtime>(app: &AppHandle<R>, result: &StartupCheckResult) {
    if let Some(state) = app.try_state::<DesktopState>() {
        let _ = state.set_startup_check(result.clone());
    }
    let _ = app.emit("gold-band://startup-update-check", result);
}

/// 获取当前 RuntimeConfig（从 DesktopState 中读取）
fn get_runtime_config<R: Runtime>(app: &AppHandle<R>) -> Option<RuntimeConfig> {
    let state = app.state::<DesktopState>();
    let context = state.context().ok()?;
    Some(context.config)
}

/// 启动时关键更新检查 —— 在 splash 阶段执行
///
/// - 渠道未开启静默更新：立即通知前端放行
/// - 网络超时/错误：降级放行，不阻塞启动
/// - critical = false 或已是最新版本：放行，后续由 start_update_polling 处理普通更新
/// - critical = true 且有新版本：保持 splash，自动下载安装并重启
pub async fn startup_critical_check<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let channel = current_channel_config();
    if !channel.silent_update_enabled {
        emit_startup_check(app, &StartupCheckResult { critical: false, error: None });
        return Ok(());
    }

    // 保证 splash 至少展示一小段时间，避免闪烁
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // 解析 endpoint
    let settings = match get_runtime_config(app) {
        Some(config) => updater_settings(&config),
        None => {
            emit_startup_check(app, &StartupCheckResult { critical: false, error: None });
            return Ok(());
        }
    };
    let endpoint = match Url::parse(&settings.effective_url) {
        Ok(url) => url,
        Err(_) => {
            emit_startup_check(app, &StartupCheckResult { critical: false, error: None });
            return Ok(());
        }
    };

    // 获取 latest.json manifest（10s 超时，超时即降级）
    let manifest = match tokio::time::timeout(
        Duration::from_secs(10),
        fetch_manifest(&endpoint),
    )
    .await
    {
        Ok(Ok(manifest)) => manifest,
        _ => {
            emit_startup_check(app, &StartupCheckResult { critical: false, error: None });
            return Ok(());
        }
    };

    // 已是最新版本 → 放行（防止 latest.json 版本号不匹配导致的死循环）
    let current_version = app.package_info().version.to_string();
    if !version_is_newer(&manifest.version, &current_version) {
        emit_startup_check(app, &StartupCheckResult { critical: false, error: None });
        return Ok(());
    }

    if !manifest.critical {
        emit_startup_check(app, &StartupCheckResult { critical: false, error: None });
        return Ok(());
    }

    emit_startup_check(app, &StartupCheckResult { critical: true, error: None });

    // 自动下载并安装（复用现有链路，download-progress 事件正常发送）
    if let Err(e) = download_and_install_update(app).await {
        eprintln!("Startup critical update failed: {e}");
        emit_startup_check(app, &StartupCheckResult {
            critical: false,
            error: Some(format!("Update install failed: {e}")),
        });
    }

    Ok(())
}

/// 简单语义版本比较：b 是否比 a 更新
/// 仅比较 major.minor.patch，忽略 pre-release 和 build metadata
fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse_trio = |v: &str| -> Option<(u32, u32, u32)> {
        let digits = v.split(&['-', '+']).next()?;
        let mut parts = digits.split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    };
    let Some((a_maj, a_min, a_pat)) = parse_trio(latest) else { return false };
    let Some((b_maj, b_min, b_pat)) = parse_trio(current) else { return false };
    (a_maj, a_min, a_pat) > (b_maj, b_min, b_pat)
}

#[cfg(test)]
mod tests {
    use super::validate_updater_url;

    #[test]
    fn accepts_https_updater_url() {
        validate_updater_url("https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json").unwrap();
    }

    #[test]
    fn rejects_invalid_updater_url() {
        assert!(validate_updater_url("not a url").is_err());
    }
}
