use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use camino::{Utf8Path, Utf8PathBuf};
use gold_band::config::{DesktopAvailableUpdate, DesktopLanguage, RuntimeConfig};
use gold_band::storage::atomic_write_file;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use tracing::warn;
use url::Url;

use crate::{channel::current_channel_config, state::DesktopState};

const POLL_INTERVAL_MINUTES: u64 = 240;
/// Lets startup restore and Agent diagnostics settle before the first background check.
const INITIAL_POLL_DELAY: Duration = Duration::from_secs(20);
const UPDATE_STATUS_EVENT: &str = "gold-band://update-status";
const UPDATE_PROGRESS_EVENT: &str = "gold-band://update-download-progress";
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterSettingsVm {
    pub channel: String,
    pub built_in_url: String,
    pub override_url: Option<String>,
    pub effective_url: String,
    pub poll_interval_minutes: u64,
}

/// Lifecycle of the single app-wide update. `update` always carries the newest known remote
/// release; failures are reported through `error` without forgetting that release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateCheckStatus {
    Idle,
    Checking,
    Available,
    NotAvailable,
    Downloading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdatePhase {
    Check,
    Download,
    Install,
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

#[derive(Debug, thiserror::Error)]
pub enum UpdaterError {
    #[error("updater.invalid-url")]
    InvalidUrl,
    #[error("updater.context-unavailable")]
    ContextUnavailable,
    #[error("updater.no-update")]
    NoUpdate,
    #[error("update package signature is invalid")]
    InvalidSignature,
    #[error("update package is not newer than the running version")]
    StalePackage,
    #[error("updater {phase:?} failed")]
    Plugin {
        phase: UpdatePhase,
        #[source]
        source: tauri_plugin_updater::Error,
    },
    #[error("updater {phase:?} io failed")]
    Io {
        phase: UpdatePhase,
        #[source]
        source: std::io::Error,
    },
}

impl UpdaterError {
    fn plugin(phase: UpdatePhase) -> impl FnOnce(tauri_plugin_updater::Error) -> Self {
        move |source| Self::Plugin { phase, source }
    }

    fn io(phase: UpdatePhase) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { phase, source }
    }

    pub fn phase(&self) -> UpdatePhase {
        match self {
            Self::InvalidUrl | Self::ContextUnavailable | Self::NoUpdate => UpdatePhase::Check,
            Self::InvalidSignature | Self::StalePackage => UpdatePhase::Install,
            Self::Plugin { phase, .. } | Self::Io { phase, .. } => *phase,
        }
    }

    pub fn code(&self) -> &'static str {
        use tauri_plugin_updater::Error as PluginError;
        match self {
            Self::InvalidUrl => "updater.invalid-url",
            Self::ContextUnavailable => "updater.context-unavailable",
            Self::NoUpdate => "updater.no-update",
            Self::InvalidSignature => "updater.signature-invalid",
            Self::StalePackage => "updater.stale-package",
            Self::Io { .. } => "updater.io",
            Self::Plugin { phase, source } => match source {
                PluginError::Reqwest(_) | PluginError::Network(_) => "updater.network",
                PluginError::Minisign(_)
                | PluginError::Base64(_)
                | PluginError::SignatureUtf8(_) => "updater.signature-invalid",
                PluginError::ReleaseNotFound
                | PluginError::Serialization(_)
                | PluginError::Semver(_)
                | PluginError::UrlParse(_)
                | PluginError::TargetNotFound(_)
                | PluginError::TargetsNotFound(_) => "updater.manifest-invalid",
                PluginError::Io(_) => "updater.io",
                _ => match phase {
                    UpdatePhase::Check => "updater.check-failed",
                    UpdatePhase::Download => "updater.download-failed",
                    UpdatePhase::Install => "updater.install-failed",
                },
            },
        }
    }

    /// Structured params only: the phase for UI copy and the technical cause chain for diagnostics.
    pub fn params(&self) -> serde_json::Value {
        serde_json::json!({ "phase": self.phase(), "detail": error_chain(self) })
    }

    pub fn vm(&self) -> UpdateErrorVm {
        UpdateErrorVm {
            code: self.code().to_string(),
            params: self.params(),
        }
    }
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}

/// A verified-at-download package waiting for the exit lifecycle to install it. The package and
/// its signature are persisted so a crash cannot lose a critical update; `update` is only
/// available when the package was downloaded by the running process.
#[derive(Clone)]
pub struct PendingUpdate {
    pub version: String,
    pub package: Utf8PathBuf,
    pub signature: String,
    pub update: Option<Update>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdateManifest {
    version: String,
    signature: String,
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

/// The persisted release is only meaningful while the app still runs the version it was
/// compared against; after an upgrade the record is stale.
pub fn persisted_update_info(
    update: Option<&DesktopAvailableUpdate>,
    running_version: &str,
) -> Option<UpdateInfoVm> {
    let update = update?;
    if update.current_version != running_version {
        return None;
    }
    Some(UpdateInfoVm {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.notes.clone(),
        pub_date: update.pub_date.clone(),
    })
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

pub fn validate_updater_url(value: &str) -> Result<(), UpdaterError> {
    let parsed = Url::parse(value).map_err(|_| UpdaterError::InvalidUrl)?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if current_channel_config().allow_http_updater || cfg!(debug_assertions) => Ok(()),
        _ => Err(UpdaterError::InvalidUrl),
    }
}

pub fn start_update_polling<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_POLL_DELAY).await;
        loop {
            poll_update_once(&app, current_channel_config().silent_update_enabled).await;
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_MINUTES * 60)).await;
        }
    });
}

pub async fn check_update<R: Runtime>(app: &AppHandle<R>, background: bool) -> UpdateStatusVm {
    perform_update_check(app, background).await.status
}

async fn poll_update_once<R: Runtime>(app: &AppHandle<R>, silent_update_enabled: bool) {
    let outcome = perform_update_check(app, true).await;
    if silent_update_enabled
        && let Some(update) = outcome.update
        && is_critical(&update)
        && begin_download(app, true).is_some()
    {
        finish_download(app, download_pending_update(app, Some(update)).await);
    }
}

fn is_critical(update: &Update) -> bool {
    update
        .raw_json
        .get("critical")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

async fn perform_update_check<R: Runtime>(
    app: &AppHandle<R>,
    background: bool,
) -> UpdateCheckOutcome {
    let Some(state) = app.try_state::<DesktopState>() else {
        return UpdateCheckOutcome {
            status: initial_update_status(None),
            update: None,
        };
    };
    let known_update = known_update(app);
    let started = state.transition_update_status(|current| {
        if install_in_flight(current.status) {
            return None;
        }
        Some(UpdateStatusVm {
            status: UpdateCheckStatus::Checking,
            checked_at: current.checked_at.clone(),
            update: known_update.clone(),
            error: None,
            background,
        })
    });
    let Some(checking) = started else {
        return UpdateCheckOutcome {
            status: state
                .update_status()
                .unwrap_or_else(|_| initial_update_status(None)),
            update: None,
        };
    };
    let _ = app.emit(UPDATE_STATUS_EVENT, &checking);

    let checked_at = Some(current_timestamp());
    let result = check_remote_update(app).await;
    let update = result.as_ref().ok().and_then(|update| update.clone());
    let status = checked_status(result, known_update, checked_at.clone(), background);
    let _ = state.persist_updater_last_checked_at(checked_at);
    if !matches!(status.status, UpdateCheckStatus::Error) && status.error.is_none() {
        let _ = state.persist_available_update(status.update.clone());
    }
    publish_status(app, status.clone());
    UpdateCheckOutcome { status, update }
}

/// Version comparison is the only source of truth: a failed check keeps the last known release.
fn checked_status(
    result: Result<Option<Update>, UpdaterError>,
    known_update: Option<UpdateInfoVm>,
    checked_at: Option<String>,
    background: bool,
) -> UpdateStatusVm {
    let (status, update, error) = match result {
        Ok(Some(update)) => (
            UpdateCheckStatus::Available,
            Some(update_info(&update)),
            None,
        ),
        Ok(None) => (UpdateCheckStatus::NotAvailable, None, None),
        Err(error) => {
            let status = if known_update.is_some() {
                UpdateCheckStatus::Available
            } else {
                UpdateCheckStatus::Error
            };
            (status, known_update, Some(error.vm()))
        }
    };
    UpdateStatusVm {
        status,
        checked_at,
        update,
        error,
        background,
    }
}

fn install_in_flight(status: UpdateCheckStatus) -> bool {
    matches!(
        status,
        UpdateCheckStatus::Downloading | UpdateCheckStatus::Ready
    )
}

fn known_update<R: Runtime>(app: &AppHandle<R>) -> Option<UpdateInfoVm> {
    let state = app.try_state::<DesktopState>()?;
    if let Some(update) = state.update_status().ok().and_then(|status| status.update) {
        return Some(update);
    }
    let config = state.context().ok()?.config;
    persisted_update_info(
        config.desktop_available_update.as_ref(),
        &app.package_info().version.to_string(),
    )
}

fn publish_status<R: Runtime>(app: &AppHandle<R>, status: UpdateStatusVm) {
    if let Some(state) = app.try_state::<DesktopState>() {
        let _ = state.set_update_status(status.clone());
    }
    let _ = app.emit(UPDATE_STATUS_EVENT, &status);
}

/// Starts the one app-wide install: an already downloaded package goes straight to the exit
/// lifecycle, otherwise a single download runs in the background and every entry point observes
/// it through status events.
pub fn start_update_install(app: &AppHandle) -> Result<UpdateStatusVm, UpdaterError> {
    let state = app.state::<DesktopState>();
    if state.has_pending_update() {
        request_install_restart(app);
        return state
            .update_status()
            .map_err(|_| UpdaterError::ContextUnavailable);
    }
    let Some(status) = begin_download(app, false) else {
        return state
            .update_status()
            .map_err(|_| UpdaterError::ContextUnavailable);
    };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = download_pending_update(&handle, None).await;
        let ready = result.is_ok();
        finish_download(&handle, result);
        if ready {
            request_install_restart(&handle);
        }
    });
    Ok(status)
}

fn request_install_restart(app: &AppHandle) {
    if let Err(error) = crate::desktop_lifecycle::request_app_restart(app) {
        warn!(code = %error.code, "update install restart was not started");
    }
}

fn begin_download<R: Runtime>(app: &AppHandle<R>, background: bool) -> Option<UpdateStatusVm> {
    let state = app.try_state::<DesktopState>()?;
    let known_update = known_update(app);
    let status = state.transition_update_status(|current| {
        if install_in_flight(current.status) {
            return None;
        }
        Some(UpdateStatusVm {
            status: UpdateCheckStatus::Downloading,
            checked_at: current.checked_at.clone(),
            update: current.update.clone().or(known_update),
            error: None,
            background,
        })
    })?;
    let _ = app.emit(UPDATE_STATUS_EVENT, &status);
    Some(status)
}

fn finish_download<R: Runtime>(app: &AppHandle<R>, result: Result<UpdateInfoVm, UpdaterError>) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    let Ok(current) = state.update_status() else {
        return;
    };
    let next = match result {
        Ok(update) => {
            let _ = state.persist_available_update(Some(update.clone()));
            UpdateStatusVm {
                status: UpdateCheckStatus::Ready,
                update: Some(update),
                error: None,
                ..current
            }
        }
        Err(UpdaterError::NoUpdate) => {
            let _ = state.persist_available_update(None);
            UpdateStatusVm {
                status: UpdateCheckStatus::NotAvailable,
                update: None,
                error: None,
                ..current
            }
        }
        Err(error) => {
            warn!(code = error.code(), detail = %error_chain(&error), "update download failed");
            let status = if current.update.is_some() {
                UpdateCheckStatus::Available
            } else {
                UpdateCheckStatus::Error
            };
            UpdateStatusVm {
                status,
                error: Some(error.vm()),
                ..current
            }
        }
    };
    publish_status(app, next);
}

async fn download_pending_update<R: Runtime>(
    app: &AppHandle<R>,
    update: Option<Update>,
) -> Result<UpdateInfoVm, UpdaterError> {
    let update = match update {
        Some(update) => update,
        None => check_remote_update(app)
            .await?
            .ok_or(UpdaterError::NoUpdate)?,
    };
    let info = update_info(&update);
    let progress_app = app.clone();
    let mut progress = DownloadProgress::default();
    let bytes = update
        .download(
            move |chunk, total| {
                if let Some(payload) = progress.record(chunk, total, Instant::now()) {
                    let _ = progress_app.emit(UPDATE_PROGRESS_EVENT, payload);
                }
            },
            || {},
        )
        .await
        .map_err(UpdaterError::plugin(UpdatePhase::Download))?;

    let dir = pending_update_dir();
    let version = update.version.clone();
    let signature = update.signature.clone();
    let package = tokio::task::spawn_blocking(move || {
        write_pending_update(&dir, &version, &signature, &bytes)
    })
    .await
    .map_err(|error| UpdaterError::Io {
        phase: UpdatePhase::Download,
        source: std::io::Error::other(error),
    })?
    .map_err(UpdaterError::io(UpdatePhase::Download))?;

    if let Some(state) = app.try_state::<DesktopState>() {
        let _ = state.store_pending_update(PendingUpdate {
            version: update.version.clone(),
            package,
            signature: update.signature.clone(),
            update: Some(update),
        });
    }
    Ok(info)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDownloadProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// Coalesces per-chunk callbacks so the UI receives at most one progress event per interval,
/// plus the first and the completing chunk.
#[derive(Default)]
struct DownloadProgress {
    downloaded: u64,
    last_emit: Option<Instant>,
}

impl DownloadProgress {
    fn record(
        &mut self,
        chunk: usize,
        total: Option<u64>,
        now: Instant,
    ) -> Option<UpdateDownloadProgress> {
        self.downloaded += chunk as u64;
        let complete = total.is_some_and(|total| self.downloaded >= total);
        let due = self
            .last_emit
            .is_none_or(|last| now.duration_since(last) >= PROGRESS_EMIT_INTERVAL);
        if !complete && !due {
            return None;
        }
        self.last_emit = Some(now);
        Some(UpdateDownloadProgress {
            downloaded: self.downloaded,
            total,
        })
    }
}

async fn check_remote_update<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<Update>, UpdaterError> {
    build_updater(app)?
        .check()
        .await
        .map_err(UpdaterError::plugin(UpdatePhase::Check))
}

fn update_info(update: &Update) -> UpdateInfoVm {
    UpdateInfoVm {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        pub_date: update.date.map(|date| date.to_string()),
    }
}

fn build_updater<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri_plugin_updater::Updater, UpdaterError> {
    let state = app
        .try_state::<DesktopState>()
        .ok_or(UpdaterError::ContextUnavailable)?;
    let config = state
        .context()
        .map_err(|_| UpdaterError::ContextUnavailable)?
        .config;
    let channel_config = current_channel_config();
    let endpoints = updater_endpoint_strings(
        channel_config.channel,
        channel_config.updater_endpoint,
        config.desktop_updater_url_override.as_deref(),
        config.desktop_language,
    )
    .into_iter()
    .map(|endpoint| {
        validate_updater_url(&endpoint)?;
        Url::parse(&endpoint).map_err(|_| UpdaterError::InvalidUrl)
    })
    .collect::<Result<Vec<_>, _>>()?;
    app.updater_builder()
        .pubkey(channel_config.updater_public_key)
        .endpoints(endpoints)
        .map_err(|_| UpdaterError::InvalidUrl)?
        .build()
        .map_err(UpdaterError::plugin(UpdatePhase::Check))
}

fn updater_endpoint_strings(
    channel: &str,
    built_in_endpoint: &str,
    override_url: Option<&str>,
    language: DesktopLanguage,
) -> Vec<String> {
    if let Some(override_url) = override_url {
        return vec![override_url.to_string()];
    }

    let localized = localized_default_updater_endpoint(channel, built_in_endpoint, language);
    if localized == built_in_endpoint {
        vec![localized]
    } else {
        vec![localized, built_in_endpoint.to_string()]
    }
}

fn localized_default_updater_endpoint(
    channel: &str,
    endpoint: &str,
    language: DesktopLanguage,
) -> String {
    if channel != "default" {
        return endpoint.to_string();
    }
    let Ok(mut url) = Url::parse(endpoint) else {
        return endpoint.to_string();
    };
    if url.path_segments().and_then(Iterator::last) != Some("latest.json") {
        return endpoint.to_string();
    }
    let filename = format!("latest.{}.json", release_notes_locale(language));
    {
        let Ok(mut segments) = url.path_segments_mut() else {
            return endpoint.to_string();
        };
        segments.pop().push(&filename);
    }
    url.into()
}

fn release_notes_locale(language: DesktopLanguage) -> &'static str {
    match language {
        DesktopLanguage::ZhCn => "zh-CN",
        DesktopLanguage::ZhTw => "zh-TW",
        DesktopLanguage::En => "en",
        DesktopLanguage::JaJp => "ja-JP",
        DesktopLanguage::KoKr => "ko-KR",
        DesktopLanguage::PtBr => "pt-BR",
        DesktopLanguage::Es => "es",
    }
}

fn current_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ── Pending package: download → persist → install from the exit lifecycle ──

fn pending_update_dir() -> Utf8PathBuf {
    let dir = std::env::temp_dir().join(format!("{}-update", current_channel_config().app_key));
    Utf8PathBuf::from_path_buf(dir)
        .unwrap_or_else(|dir| Utf8PathBuf::from(dir.to_string_lossy().as_ref()))
}

fn pending_package_path(dir: &Utf8Path, version: &str) -> Utf8PathBuf {
    dir.join(format!("update-{version}.pkg"))
}

fn pending_manifest_path(dir: &Utf8Path) -> Utf8PathBuf {
    dir.join("pending.json")
}

/// Keeps exactly one pending package: the manifest is committed last, so a crash mid-write
/// never leaves a manifest pointing at a partial package.
fn write_pending_update(
    dir: &Utf8Path,
    version: &str,
    signature: &str,
    bytes: &[u8],
) -> std::io::Result<Utf8PathBuf> {
    clear_pending_dir(dir);
    std::fs::create_dir_all(dir.as_std_path())?;
    let package = pending_package_path(dir, version);
    atomic_write_file(package.as_std_path(), |file| file.write_all(bytes))?;
    let manifest = serde_json::to_vec(&PendingUpdateManifest {
        version: version.to_string(),
        signature: signature.to_string(),
    })
    .map_err(std::io::Error::other)?;
    atomic_write_file(pending_manifest_path(dir).as_std_path(), |file| {
        file.write_all(&manifest)
    })?;
    Ok(package)
}

fn read_pending_update(dir: &Utf8Path) -> Option<PendingUpdate> {
    let manifest = std::fs::read(pending_manifest_path(dir).as_std_path()).ok()?;
    let manifest: PendingUpdateManifest = serde_json::from_slice(&manifest).ok()?;
    let package = pending_package_path(dir, &manifest.version);
    package.is_file().then_some(PendingUpdate {
        version: manifest.version,
        package,
        signature: manifest.signature,
        update: None,
    })
}

fn clear_pending_dir(dir: &Utf8Path) {
    let _ = std::fs::remove_dir_all(dir.as_std_path());
}

fn verify_package_signature(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
) -> Result<(), UpdaterError> {
    let decode = |value: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .ok()
            .and_then(|decoded| String::from_utf8(decoded).ok())
            .ok_or(UpdaterError::InvalidSignature)
    };
    let public_key = minisign_verify::PublicKey::decode(&decode(public_key)?)
        .map_err(|_| UpdaterError::InvalidSignature)?;
    let signature = minisign_verify::Signature::decode(&decode(signature)?)
        .map_err(|_| UpdaterError::InvalidSignature)?;
    public_key
        .verify(bytes, &signature, true)
        .map_err(|_| UpdaterError::InvalidSignature)
}

fn is_newer_version(candidate: &str, running: &str) -> bool {
    match (
        semver::Version::parse(candidate),
        semver::Version::parse(running),
    ) {
        (Ok(candidate), Ok(running)) => candidate > running,
        _ => false,
    }
}

/// Final step of the exit cleanup and the crash-recovery path at startup. The package is
/// re-verified against the channel key and the running version before the installer takes over.
pub async fn install_pending_update<R: Runtime>(
    app: &AppHandle<R>,
    pending: PendingUpdate,
) -> Result<(), UpdaterError> {
    let dir = pending_update_dir();
    let running_version = app.package_info().version.to_string();
    if !is_newer_version(&pending.version, &running_version) {
        clear_pending_dir(&dir);
        return Err(UpdaterError::StalePackage);
    }
    let bytes = std::fs::read(pending.package.as_std_path())
        .map_err(UpdaterError::io(UpdatePhase::Install))?;
    if let Err(error) = verify_package_signature(
        &bytes,
        &pending.signature,
        current_channel_config().updater_public_key,
    ) {
        clear_pending_dir(&dir);
        return Err(error);
    }
    let update = match pending.update {
        Some(update) => update,
        None => match check_remote_update(app).await? {
            Some(update) if update.version == pending.version => update,
            _ => {
                clear_pending_dir(&dir);
                return Err(UpdaterError::StalePackage);
            }
        },
    };
    // Windows installers terminate this process; drop the package first so it cannot loop.
    clear_pending_dir(&dir);
    update
        .install(bytes)
        .map_err(UpdaterError::plugin(UpdatePhase::Install))
}

/// A package that survived a crash is installed before any session starts, so no cleanup is
/// needed; everything else waits for the exit lifecycle.
pub fn retry_pending_startup_install<R: Runtime>(app: &AppHandle<R>) {
    let Some(pending) = read_pending_update(&pending_update_dir()) else {
        return;
    };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = install_pending_update(&handle, pending).await {
            warn!(code = error.code(), detail = %error_chain(&error), "pending update was not installed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadProgress, PROGRESS_EMIT_INTERVAL, UpdateCheckStatus, UpdateInfoVm, UpdatePhase,
        UpdaterError, checked_status, is_newer_version, localized_default_updater_endpoint,
        poll_update_once, read_pending_update, updater_endpoint_strings, updater_settings,
        validate_updater_url, verify_package_signature, write_pending_update,
    };
    use crate::state::{DesktopContext, DesktopState};
    use gold_band::config::DesktopLanguage;
    use gold_band::config::RuntimeConfig;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;
    use std::time::{Duration, Instant};
    use tauri::Manager;

    // Throwaway key pair generated with `tauri signer generate`; the private key was discarded.
    const FIXTURE_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFGMDM3ODNCRDA2ODY2RUQKUldUdFptalFPM2dEcjdJc1BheDZUVHR4MHIraXU1ZTBEZTdmM0hPNXhnM2UxV0xyYS9SQ01qR2IK";
    const FIXTURE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUdFptalFPM2dEcnlucFM4K093VUQ2aHo4ZVpCUHJXL0hpWTg2YnpxR2xueVozUi92eWoxcFpuN1RwRVNNUSt6Qit4M1N3Q2hYWU05M3BsaW9iWWNsUmdmNFF1djRyQmdVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkwNzA1ODk1CWZpbGU6cGtnLmJpbgpIaThjR3ByTDFSMjVYOVNJY0NNRURCWmYwVFZTWDhxU3RlSkxIdC9tZGZCWjQ1VTAvLzdERFp0aTVvajdDcG1xaHFIK0xvSlJMa2Vxa2tvbk9tdmpBUT09Cg==";
    const FIXTURE_PACKAGE: &[u8] = b"gold-band pending update fixture";

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

    fn known_update() -> UpdateInfoVm {
        UpdateInfoVm {
            version: "99.0.0".to_string(),
            current_version: "0.17.2".to_string(),
            notes: Some("notes".to_string()),
            pub_date: None,
        }
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
    fn default_updater_endpoint_follows_the_saved_desktop_language() {
        let endpoint = "https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json";
        for (language, locale) in [
            (DesktopLanguage::ZhCn, "zh-CN"),
            (DesktopLanguage::ZhTw, "zh-TW"),
            (DesktopLanguage::En, "en"),
            (DesktopLanguage::JaJp, "ja-JP"),
            (DesktopLanguage::KoKr, "ko-KR"),
            (DesktopLanguage::PtBr, "pt-BR"),
            (DesktopLanguage::Es, "es"),
        ] {
            assert_eq!(
                localized_default_updater_endpoint("default", endpoint, language),
                format!(
                    "https://github.com/diodeme/Gold-Band/releases/latest/download/latest.{locale}.json"
                )
            );
        }
    }

    #[test]
    fn wb_and_custom_updater_urls_remain_single_manifest_endpoints() {
        let wb_endpoint = "http://maling.weoa.com/api/file/download/latest.json";
        assert_eq!(
            updater_endpoint_strings("wb", wb_endpoint, None, DesktopLanguage::ZhCn),
            vec![wb_endpoint.to_string()]
        );

        let custom_endpoint = "https://updates.example.com/latest.json";
        assert_eq!(
            updater_endpoint_strings(
                "default",
                "https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json",
                Some(custom_endpoint),
                DesktopLanguage::JaJp,
            ),
            vec![custom_endpoint.to_string()]
        );
    }

    #[test]
    fn updater_settings_show_the_configured_manifest_for_every_language() {
        let configured = crate::channel::current_channel_config().updater_endpoint;
        for language in [
            DesktopLanguage::ZhCn,
            DesktopLanguage::ZhTw,
            DesktopLanguage::En,
            DesktopLanguage::JaJp,
            DesktopLanguage::KoKr,
            DesktopLanguage::PtBr,
            DesktopLanguage::Es,
        ] {
            let config = RuntimeConfig {
                desktop_language: language,
                ..RuntimeConfig::default()
            };
            let settings = updater_settings(&config);
            assert_eq!(settings.built_in_url, configured);
            assert_eq!(settings.effective_url, configured);
        }
    }

    #[test]
    fn default_updater_uses_legacy_manifest_only_as_locale_fallback() {
        let endpoint = "https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json";
        assert_eq!(
            updater_endpoint_strings("default", endpoint, None, DesktopLanguage::Es),
            vec![
                "https://github.com/diodeme/Gold-Band/releases/latest/download/latest.es.json"
                    .to_string(),
                endpoint.to_string(),
            ]
        );
    }

    #[test]
    fn failed_check_keeps_the_known_release_and_reports_the_cause() {
        let error = UpdaterError::Plugin {
            phase: UpdatePhase::Check,
            source: tauri_plugin_updater::Error::Network("connection refused".to_string()),
        };
        let status = checked_status(Err(error), Some(known_update()), None, true);

        assert_eq!(status.status, UpdateCheckStatus::Available);
        assert_eq!(status.update.unwrap().version, "99.0.0");
        let error = status.error.unwrap();
        assert_eq!(error.code, "updater.network");
        assert_eq!(error.params["phase"], "check");
        assert!(
            error.params["detail"]
                .as_str()
                .unwrap()
                .contains("connection refused")
        );
    }

    #[test]
    fn failed_check_without_a_known_release_is_an_error() {
        let status = checked_status(Err(UpdaterError::InvalidUrl), None, None, false);

        assert_eq!(status.status, UpdateCheckStatus::Error);
        assert_eq!(status.error.unwrap().code, "updater.invalid-url");
    }

    #[test]
    fn updater_errors_map_plugin_causes_to_stable_codes() {
        use tauri_plugin_updater::Error as PluginError;
        let code = |phase, source| UpdaterError::Plugin { phase, source }.code();

        assert_eq!(
            code(UpdatePhase::Download, PluginError::Network("503".into())),
            "updater.network"
        );
        assert_eq!(
            code(UpdatePhase::Check, PluginError::ReleaseNotFound),
            "updater.manifest-invalid"
        );
        assert_eq!(
            code(
                UpdatePhase::Download,
                PluginError::SignatureUtf8("sig".into())
            ),
            "updater.signature-invalid"
        );
        assert_eq!(
            code(UpdatePhase::Install, PluginError::EmptyEndpoints),
            "updater.install-failed"
        );
        assert_eq!(UpdaterError::StalePackage.code(), "updater.stale-package");
        assert_eq!(UpdaterError::InvalidSignature.params()["phase"], "install");
    }

    #[test]
    fn download_progress_emits_first_due_and_final_chunks_only() {
        let start = Instant::now();
        let mut progress = DownloadProgress::default();

        assert_eq!(
            progress.record(10, Some(100), start).unwrap().downloaded,
            10
        );
        assert!(
            progress
                .record(10, Some(100), start + Duration::from_millis(5))
                .is_none()
        );
        let due = progress
            .record(10, Some(100), start + PROGRESS_EMIT_INTERVAL)
            .unwrap();
        assert_eq!(due.downloaded, 30);
        let complete = progress
            .record(
                70,
                Some(100),
                start + PROGRESS_EMIT_INTERVAL + Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!((complete.downloaded, complete.total), (100, Some(100)));
    }

    #[test]
    fn pending_package_signature_is_verified_against_the_channel_key() {
        verify_package_signature(FIXTURE_PACKAGE, FIXTURE_SIGNATURE, FIXTURE_PUBLIC_KEY).unwrap();

        let tampered =
            verify_package_signature(b"tampered package", FIXTURE_SIGNATURE, FIXTURE_PUBLIC_KEY);
        assert!(matches!(tampered, Err(UpdaterError::InvalidSignature)));
        let foreign_key = verify_package_signature(
            FIXTURE_PACKAGE,
            FIXTURE_SIGNATURE,
            crate::channel::current_channel_config().updater_public_key,
        );
        assert!(matches!(foreign_key, Err(UpdaterError::InvalidSignature)));
    }

    #[test]
    fn pending_package_must_be_newer_than_the_running_version() {
        assert!(is_newer_version("0.17.3", "0.17.2"));
        assert!(is_newer_version(
            "0.17.2-wb.202609301230+abc1234",
            "0.17.2-wb.202609301200+abc1234"
        ));
        assert!(!is_newer_version("0.17.2", "0.17.2"));
        assert!(!is_newer_version("0.17.1", "0.17.2"));
        assert!(!is_newer_version("not-a-version", "0.17.2"));
    }

    #[test]
    fn pending_package_round_trips_and_keeps_a_single_package() {
        let root = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::from_path_buf(root.path().join("app-update")).unwrap();

        write_pending_update(&dir, "0.17.3", "old-signature", b"old").unwrap();
        let package =
            write_pending_update(&dir, "0.17.4", FIXTURE_SIGNATURE, FIXTURE_PACKAGE).unwrap();

        let pending = read_pending_update(&dir).unwrap();
        assert_eq!(pending.version, "0.17.4");
        assert_eq!(pending.package, package);
        assert_eq!(pending.signature, FIXTURE_SIGNATURE);
        assert!(pending.update.is_none());
        assert_eq!(
            std::fs::read(package.as_std_path()).unwrap(),
            FIXTURE_PACKAGE
        );
        assert_eq!(std::fs::read_dir(dir.as_std_path()).unwrap().count(), 2);
    }

    #[test]
    fn pending_package_without_a_committed_manifest_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        std::fs::write(dir.join("update-0.17.3.pkg").as_std_path(), b"partial").unwrap();

        assert!(read_pending_update(&dir).is_none());
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

        tauri::async_runtime::block_on(poll_update_once(&app.handle(), true));
        server.join().unwrap();

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let status = app.state::<DesktopState>().update_status().unwrap();
        assert_eq!(status.status, UpdateCheckStatus::Available);
        assert_eq!(status.update.unwrap().version, "999.0.0");
    }
}
