use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use gold_band::app::intervention::InterventionLocator;
use gold_band::app::{App, RuntimeLifecycleEvent};
use gold_band::im::{
    IM_PAYLOAD_VERSION, ImActionResponseContext, ImActionTokenCodec, ImBindingSummary,
    ImChannelCleanupOperation, ImChannelCleanupPhase, ImChannelKind, ImChannelSettings,
    ImConnectionManager, ImConnector, ImConnectorEvent, ImCredentialStore, ImDelivery,
    ImDeliveryInsertResult, ImDeliveryPayload, ImDeliveryWorker, ImDestination,
    ImInboundActionService, ImIntegrationSettings, ImLifecycleProjectionJob,
    ImLifecycleProjectionQueueItem, ImLifecycleProjector, ImLocale, ImMaintenanceWorker,
    ImMessageState, ImNavigationLocator, ImNotificationKind, ImObservedBinding,
    ImProjectionCompletion, ImProjectionEnqueueOutcome, ImProjectionResult, ImProjectionTarget,
    ImRepository, ImRepositoryError, ImWorkerSignal, InterventionResolutionNotification,
    OsImCredentialStore, ResolvedImChannelConfig, WECOM_SCAN_SESSION_TTL, WeComConnector,
    WeComScanAuthClient, WeComScanAuthError, WeComScanCredentials, desktop_resolution_event_id,
    drain_projection_queue, im_lifecycle_projection_channel, im_worker_channel,
    try_enqueue_projection,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

use crate::commands::{
    CommandErrorVm, CommandResult, execute_intervention_command, resolve_command_app,
    resolve_command_app_with_emitters,
};
use crate::state::DesktopState;

pub const IM_CHANNEL_STATE_EVENT: &str = "im-channel-state-updated";
pub const IM_PROJECTION_DIAGNOSTIC_EVENT: &str = "im-projection-diagnostic";
const TERMINAL_CONFIRMATION_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
const BINDING_PERSISTENCE_RETRY_LIMIT: usize = 3;
const IM_PROJECTION_ALERT_COOLDOWN_MS: i64 = 30_000;
const IM_PROJECTION_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const IM_CHANNEL_CLEANUP_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImChannelSettingsVm {
    pub kind: ImChannelKind,
    pub enabled: bool,
    pub public_identity: String,
    pub credential_configured: bool,
    pub binding: Option<ImBindingSummary>,
    pub notifications: gold_band::im::ImNotificationPreferences,
    pub connection: Option<gold_band::im::ImChannelSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImSettingsVm {
    pub channels: Vec<ImChannelSettingsVm>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImChannelCleanupStatusVm {
    Complete,
    Pending,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteImChannelResultVm {
    pub settings: ImSettingsVm,
    pub operation_id: String,
    pub cleanup_status: ImChannelCleanupStatusVm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImProjectionDiagnosticVm {
    code: &'static str,
    canonical_event_id: String,
    project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetImChannelEnabledInput {
    pub kind: ImChannelKind,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveImNotificationPreferencesInput {
    pub kind: ImChannelKind,
    pub notifications: gold_band::im::ImNotificationPreferences,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImGenerationInput {
    pub kind: ImChannelKind,
    pub expected_generation: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImChannelInput {
    pub kind: ImChannelKind,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeComScanSessionInput {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeComScanAuthorizationVm {
    pub session_id: String,
    pub auth_url: String,
    pub expires_at_ms: i64,
}

struct ActiveWeComScanSession {
    session_id: String,
    scode: Option<String>,
    polling: bool,
    cancellation: CancellationToken,
}

struct ActiveBindingPersistenceRetry {
    generation: u64,
    cancellation: CancellationToken,
}

pub struct DesktopImRuntime {
    repository: Arc<ImRepository>,
    credential_store: OsImCredentialStore,
    token_codec: ImActionTokenCodec,
    connection_manager: Arc<ImConnectionManager>,
    connectors: HashMap<ImChannelKind, Arc<dyn ImConnector>>,
    targets: RwLock<Vec<ImProjectionTarget>>,
    projection_sender: mpsc::Sender<ImLifecycleProjectionQueueItem>,
    projection_receiver: Mutex<Option<mpsc::Receiver<ImLifecycleProjectionQueueItem>>>,
    connector_event_sender: mpsc::Sender<(ImChannelKind, ImConnectorEvent)>,
    connector_event_receiver: Mutex<Option<mpsc::Receiver<(ImChannelKind, ImConnectorEvent)>>>,
    worker_senders: HashMap<ImChannelKind, mpsc::Sender<ImWorkerSignal>>,
    worker_receivers: Mutex<HashMap<ImChannelKind, mpsc::Receiver<ImWorkerSignal>>>,
    wecom_scan_client: WeComScanAuthClient,
    wecom_scan_session: Mutex<Option<ActiveWeComScanSession>>,
    settings_write_lock: Mutex<()>,
    binding_persistence_retries: Mutex<HashMap<ImChannelKind, ActiveBindingPersistenceRetry>>,
    last_projection_alert_at_ms: AtomicI64,
    cancellation: CancellationToken,
}

impl DesktopImRuntime {
    pub fn new(core_db_path: camino::Utf8PathBuf) -> Result<Arc<Self>> {
        let credential_store = OsImCredentialStore;
        let signing_key = credential_store
            .load_or_create_action_signing_key()
            .context("initialize IM action signing key")?;
        let token_codec =
            ImActionTokenCodec::new(signing_key).map_err(|error| anyhow::anyhow!(error.code()))?;
        let repository = Arc::new(ImRepository::new(core_db_path));
        let wecom_scan_client =
            WeComScanAuthClient::new(gold_band::config::wecom_scan_auth_config().source.clone())
                .map_err(|error| anyhow::anyhow!(error.code()))?;
        let (projection_sender, projection_receiver) = im_lifecycle_projection_channel();
        let (raw_event_sender, connector_event_receiver) = mpsc::channel(128);
        let mut connectors = HashMap::<ImChannelKind, Arc<dyn ImConnector>>::new();
        connectors.insert(
            ImChannelKind::WeCom,
            Arc::new(WeComConnector::new(token_codec.clone())),
        );
        let mut worker_senders = HashMap::new();
        let mut worker_receivers = HashMap::new();
        for channel in ImChannelKind::ALL {
            let (sender, receiver) = im_worker_channel();
            worker_senders.insert(channel, sender);
            worker_receivers.insert(channel, receiver);
        }
        Ok(Arc::new(Self {
            repository,
            credential_store,
            token_codec,
            connection_manager: Arc::new(ImConnectionManager::default()),
            connectors,
            targets: RwLock::new(Vec::new()),
            projection_sender,
            projection_receiver: Mutex::new(Some(projection_receiver)),
            connector_event_sender: raw_event_sender,
            connector_event_receiver: Mutex::new(Some(connector_event_receiver)),
            worker_senders,
            worker_receivers: Mutex::new(worker_receivers),
            wecom_scan_client,
            wecom_scan_session: Mutex::new(None),
            settings_write_lock: Mutex::new(()),
            binding_persistence_retries: Mutex::new(HashMap::new()),
            last_projection_alert_at_ms: AtomicI64::new(i64::MIN),
            cancellation: CancellationToken::new(),
        }))
    }

    pub fn start(self: &Arc<Self>, app_handle: AppHandle) {
        let projection_receiver = self
            .projection_receiver
            .lock()
            .expect("IM projection receiver lock poisoned")
            .take();
        let worker_receivers = std::mem::take(
            &mut *self
                .worker_receivers
                .lock()
                .expect("IM worker receiver lock poisoned"),
        );
        let connector_event_receiver = self
            .connector_event_receiver
            .lock()
            .expect("IM event receiver lock poisoned")
            .take();
        let runtime = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            runtime
                .bootstrap(
                    app_handle,
                    projection_receiver,
                    worker_receivers,
                    connector_event_receiver,
                )
                .await;
        });
    }

    async fn bootstrap(
        self: Arc<Self>,
        app_handle: AppHandle,
        projection_receiver: Option<mpsc::Receiver<ImLifecycleProjectionQueueItem>>,
        mut worker_receivers: HashMap<ImChannelKind, mpsc::Receiver<ImWorkerSignal>>,
        connector_event_receiver: Option<mpsc::Receiver<(ImChannelKind, ImConnectorEvent)>>,
    ) {
        if let Err(error) = self.recover_channel_cleanups(&app_handle, false).await {
            warn!(error = %error, "recover IM channel cleanup operations failed");
        }
        let maintenance = ImMaintenanceWorker::new(Arc::clone(&self.repository));
        match maintenance
            .run_startup_pass(chrono::Utc::now().timestamp_millis())
            .await
        {
            Ok(result) => tracing::info!(
                recovered_leases = result.recovered_leases,
                removed_outbox = result.removed_outbox,
                removed_inbound = result.removed_inbound,
                "IM startup maintenance completed"
            ),
            Err(error) => warn!(error_code = error.code(), "IM startup maintenance failed"),
        }

        if let Some(receiver) = projection_receiver {
            let projector = ImLifecycleProjector::new(Arc::clone(&self.repository));
            let worker_senders = self.worker_senders.clone();
            tauri::async_runtime::spawn(projector.run(receiver, move |completion| {
                handle_projection_completion(&worker_senders, completion);
            }));
        }
        for channel in ImChannelKind::ALL {
            let Some(receiver) = worker_receivers.remove(&channel) else {
                continue;
            };
            let Some(connector) = self.connectors.get(&channel).cloned() else {
                continue;
            };
            let worker = ImDeliveryWorker::new(channel, Arc::clone(&self.repository), connector);
            let cancellation = self.cancellation.child_token();
            tauri::async_runtime::spawn(worker.run(receiver, cancellation));
        }

        if let Some(receiver) = connector_event_receiver {
            let runtime = Arc::clone(&self);
            let handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                runtime.consume_connector_events(handle, receiver).await;
            });
        }
        if let Err(error) = self.reconfigure(&app_handle).await {
            warn!(error = %error, "IM runtime configuration failed");
        }
        let maintenance = ImMaintenanceWorker::new(Arc::clone(&self.repository));
        tauri::async_runtime::spawn(maintenance.run(self.cancellation.child_token()));
        let cleanup_runtime = Arc::clone(&self);
        let cleanup_handle = app_handle.clone();
        let cleanup_cancellation = self.cancellation.child_token();
        tauri::async_runtime::spawn(async move {
            cleanup_runtime
                .run_channel_cleanup_recovery(cleanup_handle, cleanup_cancellation)
                .await;
        });
    }

    async fn run_channel_cleanup_recovery(
        self: Arc<Self>,
        app_handle: AppHandle,
        cancellation: CancellationToken,
    ) {
        let mut interval = tokio::time::interval(IM_CHANNEL_CLEANUP_RETRY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(error) = self.recover_channel_cleanups(&app_handle, true).await {
                        warn!(error = %error, "retry IM channel cleanup operations failed");
                    }
                }
            }
        }
    }

    async fn recover_channel_cleanups(
        &self,
        app_handle: &AppHandle,
        drain_projection: bool,
    ) -> Result<()> {
        let state = app_handle.state::<DesktopState>();
        let app = state.app()?;
        let settings = tauri::async_runtime::spawn_blocking(move || app.load_settings())
            .await
            .map_err(|_| anyhow::anyhow!("IM_STORAGE_UNAVAILABLE"))??;
        let repository = Arc::clone(&self.repository);
        let operations =
            tauri::async_runtime::spawn_blocking(move || repository.pending_channel_cleanups())
                .await
                .map_err(|_| anyhow::anyhow!("IM_STORAGE_UNAVAILABLE"))??;

        for operation in operations {
            let setting = settings
                .im_integrations
                .channels
                .iter()
                .find(|channel| channel.kind == operation.channel);
            if setting.is_some_and(|channel| channel.credential_ref == operation.credential_ref) {
                let repository = Arc::clone(&self.repository);
                let operation_id = operation.operation_id.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    repository.finish_channel_cleanup(&operation_id)
                })
                .await
                .map_err(|_| anyhow::anyhow!("IM_STORAGE_UNAVAILABLE"))??;
                continue;
            }
            if let Err(code) = self
                .complete_channel_cleanup(operation, drain_projection)
                .await
            {
                warn!(
                    error_code = code,
                    "IM channel cleanup recovery remains pending"
                );
            }
        }
        Ok(())
    }

    async fn complete_channel_cleanup(
        &self,
        operation: ImChannelCleanupOperation,
        drain_projection: bool,
    ) -> std::result::Result<(), &'static str> {
        let mut phase = operation.phase;
        if drain_projection {
            tokio::time::timeout(
                IM_PROJECTION_DRAIN_TIMEOUT,
                drain_projection_queue(&self.projection_sender),
            )
            .await
            .map_err(|_| "IM_PROJECTION_DRAIN_TIMEOUT")??;
        }
        if phase != ImChannelCleanupPhase::ProjectionDrained
            && phase != ImChannelCleanupPhase::OutboxRemoved
        {
            phase = ImChannelCleanupPhase::ProjectionDrained;
            self.update_cleanup_phase(&operation.operation_id, phase, None)
                .await?;
        }

        if phase != ImChannelCleanupPhase::OutboxRemoved {
            let repository = Arc::clone(&self.repository);
            let channel = operation.channel;
            match tauri::async_runtime::spawn_blocking(move || {
                repository.delete_active_for_channel(channel)
            })
            .await
            {
                Ok(Ok(_)) => {
                    phase = ImChannelCleanupPhase::OutboxRemoved;
                    self.update_cleanup_phase(&operation.operation_id, phase, None)
                        .await?;
                }
                Ok(Err(error)) => {
                    let code = error.code();
                    let _ = self
                        .update_cleanup_phase(&operation.operation_id, phase, Some(code))
                        .await;
                    return Err(code);
                }
                Err(_) => {
                    let code = "IM_STORAGE_UNAVAILABLE";
                    let _ = self
                        .update_cleanup_phase(&operation.operation_id, phase, Some(code))
                        .await;
                    return Err(code);
                }
            }
        }

        if let Some(reference) = operation.credential_ref {
            let channel = operation.channel;
            match tauri::async_runtime::spawn_blocking(move || {
                OsImCredentialStore.delete(channel, &reference)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let code = error.code();
                    let _ = self
                        .update_cleanup_phase(&operation.operation_id, phase, Some(code))
                        .await;
                    return Err(code);
                }
                Err(_) => {
                    let code = "IM_CREDENTIAL_UNAVAILABLE";
                    let _ = self
                        .update_cleanup_phase(&operation.operation_id, phase, Some(code))
                        .await;
                    return Err(code);
                }
            }
        }

        let repository = Arc::clone(&self.repository);
        let operation_id = operation.operation_id;
        tauri::async_runtime::spawn_blocking(move || {
            repository.finish_channel_cleanup(&operation_id)
        })
        .await
        .map_err(|_| "IM_STORAGE_UNAVAILABLE")?
        .map_err(|error| error.code())?;
        Ok(())
    }

    async fn update_cleanup_phase(
        &self,
        operation_id: &str,
        phase: ImChannelCleanupPhase,
        error_code: Option<&str>,
    ) -> std::result::Result<(), &'static str> {
        let repository = Arc::clone(&self.repository);
        let operation_id = operation_id.to_owned();
        let error_code = error_code.map(str::to_owned);
        tauri::async_runtime::spawn_blocking(move || {
            repository.update_channel_cleanup(
                &operation_id,
                phase,
                error_code.as_deref(),
                chrono::Utc::now().timestamp_millis(),
            )
        })
        .await
        .map_err(|_| "IM_STORAGE_UNAVAILABLE")?
        .map_err(|error| error.code())?;
        Ok(())
    }

    pub fn register_lifecycle_subscriber(self: &Arc<Self>, app: &App, app_handle: &AppHandle) {
        let runtime = Arc::clone(self);
        let handle = app_handle.clone();
        let registered = app.lifecycle_bus.subscribe_named(
            "desktop.im-projection",
            Arc::new(move |event| {
                let canonical_event_id = event_canonical_id(&event).to_string();
                let Some(project_id) = event_project_id(&event) else {
                    return;
                };
                let project_id = project_id.to_owned();
                let Some(state) = handle.try_state::<DesktopState>() else {
                    warn!(
                        canonical_event_id,
                        error_code = "IM_RUNTIME_UNAVAILABLE",
                        "IM lifecycle projection could not resolve desktop state"
                    );
                    return;
                };
                let runtime_app = match resolve_command_app(&state, Some(&project_id)) {
                    Ok(app) => app,
                    Err(error) => {
                        warn!(
                            canonical_event_id,
                            error_code = error.code,
                            "IM lifecycle projection could not resolve workspace"
                        );
                        return;
                    }
                };
                let targets = match runtime.targets.read() {
                    Ok(targets) => targets,
                    Err(_) => {
                        warn!(
                            canonical_event_id,
                            error_code = "IM_RUNTIME_UNAVAILABLE",
                            "IM lifecycle projection could not read targets"
                        );
                        return;
                    }
                };
                let job = ImLifecycleProjectionJob {
                    app: runtime_app,
                    event,
                    targets: targets.clone(),
                    now_ms: chrono::Utc::now().timestamp_millis(),
                };
                match try_enqueue_projection(&runtime.projection_sender, job) {
                    ImProjectionEnqueueOutcome::Enqueued => {}
                    ImProjectionEnqueueOutcome::Saturated => {
                        warn!(
                            canonical_event_id,
                            project_id,
                            error_code = "IM_PROJECTION_QUEUE_FULL",
                            "IM lifecycle projection queue is full"
                        );
                        runtime.emit_projection_diagnostic(
                            &handle,
                            "IM_PROJECTION_QUEUE_FULL",
                            canonical_event_id,
                            project_id,
                        );
                    }
                    ImProjectionEnqueueOutcome::Closed => {
                        warn!(
                            canonical_event_id,
                            error_code = "IM_PROJECTION_QUEUE_CLOSED",
                            "IM lifecycle projection queue is closed"
                        );
                    }
                }
            }),
        );
        if !registered {
            warn!(
                error_code = "IM_PROJECTION_SUBSCRIBER_DUPLICATE",
                "IM lifecycle projection subscriber was already registered"
            );
        }
    }

    pub async fn reconfigure(self: &Arc<Self>, app_handle: &AppHandle) -> Result<()> {
        let app = app_handle.state::<DesktopState>().app()?;
        let settings = tauri::async_runtime::spawn_blocking(move || app.load_settings())
            .await
            .map_err(|_| anyhow::anyhow!("IM_STORAGE_UNAVAILABLE"))??;
        let targets = projection_targets(&settings.im_integrations, &self.connectors);
        let target_count = targets.len();
        *self
            .targets
            .write()
            .map_err(|_| anyhow::anyhow!("IM_RUNTIME_UNAVAILABLE"))? = targets;
        tracing::info!(target_count, "IM projection targets configured");
        let locale = match settings.desktop_language {
            Some(gold_band::config::DesktopLanguage::En) => ImLocale::En,
            _ => ImLocale::ZhCn,
        };
        for channel in ImChannelKind::ALL {
            if let Err(error) = self
                .reconfigure_channel(app_handle, channel, &settings.im_integrations, locale)
                .await
            {
                warn!(
                    channel = channel.as_str(),
                    error = %error,
                    "IM channel reconfiguration failed"
                );
            }
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        self.cancel_wecom_scan(None);
        for retry in self
            .binding_persistence_retries
            .lock()
            .expect("IM binding retry lock poisoned")
            .drain()
            .map(|(_, retry)| retry)
        {
            retry.cancellation.cancel();
        }
        for channel in ImChannelKind::ALL {
            let capabilities = self
                .connectors
                .get(&channel)
                .map(|connector| connector.capabilities())
                .unwrap_or_default();
            let start = self
                .connection_manager
                .replace(channel, false, capabilities);
            if let Some(connector) = self.connectors.get(&channel) {
                connector.advance_generation(start.generation);
            }
            if let Some(sender) = self.worker_senders.get(&channel) {
                let _ = sender.send(ImWorkerSignal::Shutdown).await;
            }
        }
        self.cancellation.cancel();
    }

    async fn start_wecom_scan(
        &self,
        session_id: String,
    ) -> Result<WeComScanAuthorizationVm, WeComScanAuthError> {
        if Uuid::parse_str(&session_id).is_err() {
            return Err(WeComScanAuthError::SessionInvalid);
        }
        let cancellation = CancellationToken::new();
        {
            let mut active = self
                .wecom_scan_session
                .lock()
                .expect("WeCom scan session lock poisoned");
            if let Some(previous) = active.take() {
                previous.cancellation.cancel();
            }
            *active = Some(ActiveWeComScanSession {
                session_id: session_id.clone(),
                scode: None,
                polling: false,
                cancellation: cancellation.clone(),
            });
        }
        let authorization = match self.wecom_scan_client.generate(&cancellation).await {
            Ok(authorization) => authorization,
            Err(error) => {
                self.cancel_wecom_scan(Some(&session_id));
                return Err(error);
            }
        };
        {
            let mut active = self
                .wecom_scan_session
                .lock()
                .expect("WeCom scan session lock poisoned");
            let Some(current) = active.as_mut() else {
                return Err(WeComScanAuthError::Cancelled);
            };
            if current.session_id != session_id || cancellation.is_cancelled() {
                return Err(WeComScanAuthError::Cancelled);
            }
            current.scode = Some(authorization.scode);
        }
        Ok(WeComScanAuthorizationVm {
            session_id,
            auth_url: authorization.auth_url,
            expires_at_ms: chrono::Utc::now().timestamp_millis()
                + i64::try_from(WECOM_SCAN_SESSION_TTL.as_millis()).unwrap_or(300_000),
        })
    }

    async fn complete_wecom_scan(
        &self,
        session_id: &str,
    ) -> Result<WeComScanCredentials, WeComScanAuthError> {
        let (scode, cancellation) = {
            let mut active = self
                .wecom_scan_session
                .lock()
                .expect("WeCom scan session lock poisoned");
            let current = active
                .as_mut()
                .filter(|current| current.session_id == session_id)
                .ok_or(WeComScanAuthError::Expired)?;
            if current.polling {
                return Err(WeComScanAuthError::SessionConflict);
            }
            current.polling = true;
            (
                current
                    .scode
                    .clone()
                    .ok_or(WeComScanAuthError::ProtocolInvalid)?,
                current.cancellation.clone(),
            )
        };
        let result = self.wecom_scan_client.poll(&scode, &cancellation).await;
        self.cancel_wecom_scan(Some(session_id));
        result
    }

    fn cancel_wecom_scan(&self, session_id: Option<&str>) {
        let mut active = self
            .wecom_scan_session
            .lock()
            .expect("WeCom scan session lock poisoned");
        if active.as_ref().is_some_and(|current| {
            session_id.is_none_or(|session_id| current.session_id == session_id)
        }) && let Some(current) = active.take()
        {
            current.cancellation.cancel();
        }
    }

    pub fn project_desktop_intervention_resolution(
        &self,
        source_event_id: &str,
        notification_kind: ImNotificationKind,
        locator: &InterventionLocator,
    ) -> std::result::Result<ImProjectionResult, ImRepositoryError> {
        let targets = self
            .targets
            .read()
            .map_err(|_| ImRepositoryError::LockUnavailable)?
            .clone();
        project_desktop_intervention_resolution(
            &self.repository,
            &self.worker_senders,
            &targets,
            source_event_id,
            notification_kind,
            locator,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    fn emit_projection_diagnostic(
        &self,
        app_handle: &AppHandle,
        code: &'static str,
        canonical_event_id: String,
        project_id: String,
    ) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let previous = self.last_projection_alert_at_ms.load(Ordering::Relaxed);
        if now_ms.saturating_sub(previous) < IM_PROJECTION_ALERT_COOLDOWN_MS
            || self
                .last_projection_alert_at_ms
                .compare_exchange(previous, now_ms, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        let _ = app_handle.emit(
            IM_PROJECTION_DIAGNOSTIC_EVENT,
            ImProjectionDiagnosticVm {
                code,
                canonical_event_id,
                project_id,
            },
        );
    }

    async fn reconfigure_channel(
        self: &Arc<Self>,
        app_handle: &AppHandle,
        channel: ImChannelKind,
        settings: &ImIntegrationSettings,
        locale: ImLocale,
    ) -> Result<()> {
        let Some(connector) = self.connectors.get(&channel).cloned() else {
            return Ok(());
        };
        let channel_settings = settings
            .channels
            .iter()
            .find(|candidate| candidate.kind == channel)
            .cloned()
            .unwrap_or_else(|| disabled_channel(channel));
        self.cancel_binding_persistence_retry(channel);
        let existing_binding = channel_settings.binding.as_ref().map(observed_binding);
        let start = self.connection_manager.replace_with_binding(
            channel,
            channel_settings.enabled,
            connector.capabilities(),
            existing_binding,
        );
        connector.advance_generation(start.generation);
        emit_snapshot(app_handle, self.connection_manager.snapshot(channel));
        if !channel_settings.enabled {
            return Ok(());
        }
        let Some(credential_ref) = channel_settings.credential_ref.clone() else {
            self.settle_reconfigure_failure(
                app_handle,
                channel,
                start.generation,
                gold_band::im::ImErrorCode::ConfigInvalid,
            );
            return Ok(());
        };
        let credential_store = self.credential_store;
        let credentials = match tauri::async_runtime::spawn_blocking(move || {
            credential_store.load(channel, &credential_ref)
        })
        .await
        {
            Ok(Ok(credentials)) => credentials,
            Ok(Err(error)) => {
                warn!(
                    channel = channel.as_str(),
                    error_code = error.code(),
                    "load IM credential failed"
                );
                self.settle_reconfigure_failure(
                    app_handle,
                    channel,
                    start.generation,
                    gold_band::im::ImErrorCode::CredentialUnavailable,
                );
                return Ok(());
            }
            Err(_) => {
                self.settle_reconfigure_failure(
                    app_handle,
                    channel,
                    start.generation,
                    gold_band::im::ImErrorCode::CredentialUnavailable,
                );
                return Ok(());
            }
        };
        let config = ResolvedImChannelConfig {
            public_identity: channel_settings.public_identity,
            credential_fields: credentials.fields,
            locale,
        };
        let raw_events = self.connector_event_sender.clone();
        let cancellation = start.cancellation;
        tauri::async_runtime::spawn(async move {
            let (event_sender, mut event_receiver) = mpsc::channel(128);
            let forwarded_events = raw_events.clone();
            let forward = tauri::async_runtime::spawn(async move {
                while let Some(event) = event_receiver.recv().await {
                    if forwarded_events.send((channel, event)).await.is_err() {
                        break;
                    }
                }
            });
            let result = connector
                .connect(config, start.generation, event_sender, cancellation)
                .await;
            let _ = forward.await;
            if let Some(event) = terminal_connector_event(start.generation, result) {
                let _ = raw_events.send((channel, event)).await;
            }
        });
        Ok(())
    }

    fn settle_reconfigure_failure(
        &self,
        app_handle: &AppHandle,
        channel: ImChannelKind,
        generation: u64,
        code: gold_band::im::ImErrorCode,
    ) {
        self.connection_manager.apply_event(
            channel,
            ImConnectorEvent::Disconnected {
                generation,
                error: gold_band::im::ImIntegrationError::permanent(code),
            },
            chrono::Utc::now().timestamp_millis(),
        );
        emit_snapshot(app_handle, self.connection_manager.snapshot(channel));
    }

    async fn consume_connector_events(
        self: Arc<Self>,
        app_handle: AppHandle,
        mut receiver: mpsc::Receiver<(ImChannelKind, ImConnectorEvent)>,
    ) {
        while let Some((channel, event)) = receiver.recv().await {
            if let ImConnectorEvent::BindingObserved {
                generation,
                binding,
            } = event
            {
                let runtime = Arc::clone(&self);
                let handle = app_handle.clone();
                let binding_for_persist = binding.clone();
                let persist_result = tauri::async_runtime::spawn_blocking(move || {
                    persist_binding_then_commit(
                        &runtime.connection_manager,
                        channel,
                        generation,
                        binding_for_persist.clone(),
                        || {
                            runtime.persist_binding(
                                &handle,
                                channel,
                                generation,
                                &binding_for_persist,
                            )
                        },
                    )
                })
                .await;
                match persist_result {
                    Ok(Ok(true)) => {
                        self.cancel_binding_persistence_retry(channel);
                    }
                    Ok(Ok(false)) => {}
                    Ok(Err(error)) => {
                        self.connection_manager
                            .mark_binding_persistence_failed(channel, generation);
                        warn!(
                            channel = channel.as_str(),
                            generation,
                            error = %error,
                            "persist IM private binding failed"
                        );
                        self.schedule_binding_persistence_retry(
                            app_handle.clone(),
                            channel,
                            generation,
                            binding,
                        );
                    }
                    Err(error) => {
                        self.connection_manager
                            .mark_binding_persistence_failed(channel, generation);
                        warn!(
                            channel = channel.as_str(),
                            generation,
                            error = %error,
                            "join IM private binding persistence task failed"
                        );
                        self.schedule_binding_persistence_retry(
                            app_handle.clone(),
                            channel,
                            generation,
                            binding,
                        );
                    }
                }
                emit_snapshot(&app_handle, self.connection_manager.snapshot(channel));
                continue;
            }
            let accepted = self.connection_manager.apply_event(
                channel,
                event.clone(),
                chrono::Utc::now().timestamp_millis(),
            );
            if !accepted {
                continue;
            }
            match &event {
                ImConnectorEvent::Connected { generation, .. } => tracing::info!(
                    channel = channel.as_str(),
                    generation,
                    "IM channel connected"
                ),
                ImConnectorEvent::Disconnected { generation, error } => warn!(
                    channel = channel.as_str(),
                    generation,
                    error_code = error.code.as_str(),
                    retryable = error.retryable,
                    platform_code = ?error.platform_code,
                    "IM channel disconnected"
                ),
                _ => {}
            }
            match event {
                ImConnectorEvent::InboundAction {
                    generation,
                    envelope,
                    response_context,
                } => {
                    let runtime = Arc::clone(&self);
                    let handle = app_handle.clone();
                    let fallback_context = response_context.clone();
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        runtime.process_inbound(&handle, envelope, response_context)
                    })
                    .await;
                    let (response_context, state) =
                        settle_inbound_action(channel, generation, result, fallback_context);
                    if let Some(connector) = self.connectors.get(&channel)
                        && let Err(error) =
                            connector.respond_to_action(response_context, state).await
                    {
                        warn!(
                            channel = channel.as_str(),
                            generation,
                            error_code = error.code.as_str(),
                            retryable = error.retryable,
                            platform_code = ?error.platform_code,
                            "IM action card response failed"
                        );
                    }
                }
                _ => {}
            }
            emit_snapshot(&app_handle, self.connection_manager.snapshot(channel));
        }
    }

    fn schedule_binding_persistence_retry(
        self: &Arc<Self>,
        app_handle: AppHandle,
        channel: ImChannelKind,
        generation: u64,
        binding: ImObservedBinding,
    ) {
        let cancellation = {
            let mut retries = self
                .binding_persistence_retries
                .lock()
                .expect("IM binding retry lock poisoned");
            if retries
                .get(&channel)
                .is_some_and(|retry| retry.generation == generation)
            {
                return;
            }
            if let Some(previous) = retries.remove(&channel) {
                previous.cancellation.cancel();
            }
            let cancellation = CancellationToken::new();
            retries.insert(
                channel,
                ActiveBindingPersistenceRetry {
                    generation,
                    cancellation: cancellation.clone(),
                },
            );
            cancellation
        };
        let runtime = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            for attempt in 0..BINDING_PERSISTENCE_RETRY_LIMIT {
                let delay = std::time::Duration::from_secs(1_u64 << attempt);
                tokio::select! {
                    _ = cancellation.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
                let runtime_for_persist = Arc::clone(&runtime);
                let handle = app_handle.clone();
                let binding_for_persist = binding.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    persist_binding_then_commit(
                        &runtime_for_persist.connection_manager,
                        channel,
                        generation,
                        binding_for_persist.clone(),
                        || {
                            runtime_for_persist.persist_binding(
                                &handle,
                                channel,
                                generation,
                                &binding_for_persist,
                            )
                        },
                    )
                })
                .await;
                match result {
                    Ok(Ok(true)) => {
                        runtime.cancel_binding_persistence_retry(channel);
                        emit_snapshot(&app_handle, runtime.connection_manager.snapshot(channel));
                        return;
                    }
                    Ok(Ok(false)) => {
                        runtime.cancel_binding_persistence_retry(channel);
                        return;
                    }
                    Ok(Err(error)) => warn!(
                        channel = channel.as_str(),
                        generation,
                        attempt = attempt + 1,
                        error = %error,
                        "retry IM private binding persistence failed"
                    ),
                    Err(error) => warn!(
                        channel = channel.as_str(),
                        generation,
                        attempt = attempt + 1,
                        error = %error,
                        "join IM private binding persistence retry failed"
                    ),
                }
                if !runtime
                    .connection_manager
                    .mark_binding_persistence_failed(channel, generation)
                {
                    runtime.cancel_binding_persistence_retry(channel);
                    return;
                }
                emit_snapshot(&app_handle, runtime.connection_manager.snapshot(channel));
            }
            runtime.cancel_binding_persistence_retry(channel);
        });
    }

    fn cancel_binding_persistence_retry(&self, channel: ImChannelKind) {
        if let Some(retry) = self
            .binding_persistence_retries
            .lock()
            .expect("IM binding retry lock poisoned")
            .remove(&channel)
        {
            retry.cancellation.cancel();
        }
    }

    fn process_inbound(
        &self,
        app_handle: &AppHandle,
        envelope: gold_band::im::ImInboundEnvelope,
        mut response_context: ImActionResponseContext,
    ) -> (
        ImActionResponseContext,
        std::result::Result<(), anyhow::Error>,
    ) {
        let delivery = match self.repository.delivery(&envelope.delivery_id) {
            Ok(delivery) => delivery.context("IM delivery not found"),
            Err(error) => Err(anyhow::Error::new(error)),
        };
        let Ok(delivery) = delivery else {
            return (
                response_context,
                Err(anyhow::anyhow!("IM_DELIVERY_NOT_FOUND")),
            );
        };
        let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
            let service =
                ImInboundActionService::new(Arc::clone(&self.repository), self.token_codec.clone());
            let result = service.handle_with_delivery(
                &envelope,
                &delivery,
                chrono::Utc::now().timestamp_millis(),
                |_| unreachable!("information payload never reaches Runtime"),
            );
            return (
                response_context,
                result.map(|_| ()).map_err(anyhow::Error::new),
            );
        };
        enrich_wecom_vote_context(&mut response_context, &envelope, &delivery);
        enrich_wecom_form_context(&mut response_context, &envelope, &delivery);
        let state = app_handle.state::<DesktopState>();
        let Ok(app) = resolve_command_app_with_emitters(
            &app_handle,
            &state,
            Some(&reference.locator.project_id),
        )
        .map_err(|error| anyhow::anyhow!(error.code)) else {
            return (
                response_context,
                Err(anyhow::anyhow!("IM_WORKSPACE_UNAVAILABLE")),
            );
        };
        let service =
            ImInboundActionService::new(Arc::clone(&self.repository), self.token_codec.clone());
        let result = service.handle_with_delivery(
            &envelope,
            &delivery,
            chrono::Utc::now().timestamp_millis(),
            |command| {
                tauri::async_runtime::block_on(execute_intervention_command(
                    app_handle.clone(),
                    state.inner(),
                    app,
                    Some(reference.locator.project_id.clone()),
                    command,
                ))
            },
        );
        (response_context, result.map(|_| ()).map_err(Into::into))
    }

    fn persist_binding(
        &self,
        app_handle: &AppHandle,
        channel: ImChannelKind,
        generation: u64,
        binding: &ImObservedBinding,
    ) -> Result<()> {
        let _write_guard = self
            .settings_write_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("IM_STORAGE_UNAVAILABLE"))?;
        if self
            .connection_manager
            .snapshot(channel)
            .is_none_or(|snapshot| snapshot.generation != generation)
        {
            return Ok(());
        }
        let state = app_handle.state::<DesktopState>();
        let app = state.app()?;
        let mut settings = app.load_settings()?;
        let Some(channel_settings) = settings
            .im_integrations
            .channels
            .iter_mut()
            .find(|candidate| candidate.kind == channel)
        else {
            return Ok(());
        };
        if let Some(existing) = &channel_settings.binding {
            if existing.authorized_actor_id != binding.actor_id
                || existing.conversation_id != binding.conversation_id
                || existing.destination_id != binding.destination_id
            {
                anyhow::bail!("IM_BINDING_CONFLICT");
            }
        } else {
            channel_settings.binding = Some(ImBindingSummary {
                destination_id: binding.destination_id.clone(),
                conversation_id: binding.conversation_id.clone(),
                authorized_actor_id: binding.actor_id.clone(),
                display_name: binding.actor_id.clone(),
            });
            app.save_settings(&settings)?;
        }
        state.update_settings_config(&settings)?;
        *self.targets.write().expect("IM target lock poisoned") =
            projection_targets(&settings.im_integrations, &self.connectors);
        Ok(())
    }
}

fn persist_binding_then_commit(
    manager: &ImConnectionManager,
    channel: ImChannelKind,
    generation: u64,
    binding: ImObservedBinding,
    persist: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    if !manager.prepare_binding(channel, generation, &binding) {
        return Ok(false);
    }
    persist()?;
    Ok(manager.commit_binding(channel, generation, binding))
}

fn project_desktop_intervention_resolution(
    repository: &ImRepository,
    worker_senders: &HashMap<ImChannelKind, mpsc::Sender<ImWorkerSignal>>,
    targets: &[ImProjectionTarget],
    source_event_id: &str,
    notification_kind: ImNotificationKind,
    locator: &InterventionLocator,
    now_ms: i64,
) -> std::result::Result<ImProjectionResult, ImRepositoryError> {
    let sources = repository.intervention_deliveries_for_canonical_event(source_event_id)?;
    let mut result = ImProjectionResult::default();
    let mut projected_channels = std::collections::HashSet::new();
    for source in sources {
        let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
            result.skipped += 1;
            continue;
        };
        projected_channels.insert(source.channel);
        let resolution = desktop_resolution_delivery(
            source.channel,
            source.destination.clone(),
            source.notification_kind,
            source_event_id,
            &reference.locator,
            source.display_ref,
            now_ms,
        );
        match repository.enqueue_resolution_and_expire_pending_source(
            source_event_id,
            &resolution,
            now_ms,
        )? {
            ImDeliveryInsertResult::Inserted => {
                result.inserted += 1;
                if let Some(sender) = worker_senders.get(&source.channel) {
                    let _ = sender.try_send(ImWorkerSignal::DeliveryAvailable);
                }
            }
            ImDeliveryInsertResult::Duplicate => result.duplicates += 1,
        }
    }
    for target in targets {
        if projected_channels.contains(&target.channel)
            || !eligible_desktop_resolution_target(target, notification_kind)
        {
            continue;
        }
        let destination = target
            .destination
            .clone()
            .expect("eligible IM target has a destination");
        let resolution = desktop_resolution_delivery(
            target.channel,
            destination,
            notification_kind,
            source_event_id,
            locator,
            None,
            now_ms,
        );
        match repository.enqueue_resolution_and_expire_pending_source(
            source_event_id,
            &resolution,
            now_ms,
        )? {
            ImDeliveryInsertResult::Inserted => {
                result.inserted += 1;
                if let Some(sender) = worker_senders.get(&target.channel) {
                    let _ = sender.try_send(ImWorkerSignal::DeliveryAvailable);
                }
            }
            ImDeliveryInsertResult::Duplicate => result.duplicates += 1,
        }
    }
    Ok(result)
}

fn desktop_resolution_delivery(
    channel: ImChannelKind,
    destination: ImDestination,
    notification_kind: ImNotificationKind,
    source_event_id: &str,
    locator: &InterventionLocator,
    display_ref: Option<u16>,
    now_ms: i64,
) -> ImDelivery {
    let canonical_event_id = desktop_resolution_event_id(source_event_id);
    ImDelivery {
        delivery_id: ImDelivery::deterministic_id(
            channel,
            &destination.destination_id,
            notification_kind,
            &canonical_event_id,
        ),
        channel,
        destination,
        notification_kind,
        canonical_event_id: canonical_event_id.clone(),
        payload: ImDeliveryPayload::InterventionResolution {
            version: IM_PAYLOAD_VERSION,
            resolution: InterventionResolutionNotification {
                canonical_event_id,
                source_event_id: source_event_id.to_string(),
                notification_kind,
                locator: ImNavigationLocator {
                    project_id: locator.project_id.clone(),
                    task_id: Some(locator.task_id.clone()),
                    run_id: Some(locator.run_id.clone()),
                    round_id: Some(locator.round_id.clone()),
                    node_id: Some(locator.node_id.clone()),
                    attempt_id: Some(locator.attempt_id.clone()),
                    outer_node_id: locator.outer_node_id.clone(),
                    outer_attempt_id: locator.outer_attempt_id.clone(),
                    scheduled_occurrence_id: None,
                },
                display_ref,
            },
        },
        expires_at_ms: now_ms.saturating_add(TERMINAL_CONFIRMATION_TTL_MS),
        display_ref,
    }
}

fn eligible_desktop_resolution_target(
    target: &ImProjectionTarget,
    notification_kind: ImNotificationKind,
) -> bool {
    target.enabled
        && target.credential_available
        && target.capabilities.proactive_delivery
        && target.capabilities.private_chat
        && target.capabilities.card_actions
        && target.destination.is_some()
        && target.notifications.enabled(notification_kind)
        && (notification_kind != ImNotificationKind::Elicitation
            || target.channel == ImChannelKind::WeCom)
}

fn settle_inbound_action<E>(
    channel: ImChannelKind,
    generation: u64,
    result: Result<
        (
            ImActionResponseContext,
            std::result::Result<(), anyhow::Error>,
        ),
        E,
    >,
    fallback_context: ImActionResponseContext,
) -> (ImActionResponseContext, ImMessageState)
where
    E: std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok((processed_context, Ok(()))) => (processed_context, ImMessageState::Handled),
        Ok((processed_context, Err(error))) => {
            let error_code = inbound_processing_error_code(&error);
            warn!(
                channel = channel.as_str(),
                generation, error_code, "IM inbound action failed"
            );
            (processed_context, inbound_failure_state(error_code))
        }
        Err(error) => {
            warn!(
                channel = channel.as_str(),
                generation,
                error_code = "IM_ACTION_TASK_FAILED",
                join_error = %error,
                "IM inbound action task failed"
            );
            (fallback_context, ImMessageState::Failed)
        }
    }
}

fn enrich_wecom_vote_context(
    context: &mut ImActionResponseContext,
    envelope: &gold_band::im::ImInboundEnvelope,
    delivery: &gold_band::im::ImDelivery,
) {
    let ImActionResponseContext::WeCom {
        vote_selection: Some(vote_selection),
        ..
    } = context
    else {
        return;
    };
    let gold_band::im::ImInboundActionSelection::LocalIndex { index } = &envelope.action else {
        return;
    };
    let Ok(selected_index) = vote_selection.selected_option_id.parse::<usize>() else {
        return;
    };
    if selected_index != *index {
        return;
    }
    let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
        return;
    };
    vote_selection.vote_actions = reference.allowed_actions.clone();
    vote_selection.display_ref = delivery.display_ref;
}

fn enrich_wecom_form_context(
    context: &mut ImActionResponseContext,
    envelope: &gold_band::im::ImInboundEnvelope,
    delivery: &gold_band::im::ImDelivery,
) {
    let ImActionResponseContext::WeCom {
        form_selection: Some(form_selection),
        ..
    } = context
    else {
        return;
    };
    let gold_band::im::ImInboundActionSelection::Form { selections } = &envelope.action else {
        return;
    };
    if form_selection.selections != *selections {
        return;
    }
    let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
        return;
    };
    let Some(form) = reference
        .allowed_actions
        .iter()
        .find_map(|action| match action {
            gold_band::app::intervention::InterventionAllowedAction::ElicitationFixedForm {
                form,
            } => Some(form),
            _ => None,
        })
    else {
        return;
    };
    form_selection.card_kind = match form {
        gold_band::app::intervention::RemoteElicitationForm::SingleScalarChoice { .. } => {
            gold_band::im::ImWeComFormCardKind::VoteSingle
        }
        gold_band::app::intervention::RemoteElicitationForm::MultiScalarChoice { .. } => {
            gold_band::im::ImWeComFormCardKind::VoteMulti
        }
        gold_band::app::intervention::RemoteElicitationForm::ScalarChoiceQuestions { .. } => {
            gold_band::im::ImWeComFormCardKind::Multiple
        }
    };
    form_selection.form = Some(form.clone());
    form_selection.display_ref = delivery.display_ref;
}

fn inbound_processing_error_code(error: &anyhow::Error) -> &'static str {
    error
        .downcast_ref::<gold_band::im::ImInboundError>()
        .map(gold_band::im::ImInboundError::code)
        .unwrap_or("IM_ACTION_PROCESSING_FAILED")
}

fn inbound_failure_state(error_code: &str) -> ImMessageState {
    match error_code {
        "IM_ACTION_EXPIRED" | "INTERVENTION_EXPIRED" => ImMessageState::Expired,
        "INTERVENTION_ALREADY_HANDLED" | "INTERVENTION_REVISION_CONFLICT" => {
            ImMessageState::Handled
        }
        _ => ImMessageState::Failed,
    }
}

#[tauri::command]
pub fn get_im_settings(state: tauri::State<'_, DesktopState>) -> CommandResult<ImSettingsVm> {
    let app = state.app().map_err(im_command_error)?;
    let settings = app.load_settings().map_err(im_command_error)?;
    Ok(settings_vm(
        &settings.im_integrations,
        state.im_runtime().as_deref(),
    ))
}

#[tauri::command]
pub async fn start_wecom_scan_authorization(
    state: tauri::State<'_, DesktopState>,
    input: WeComScanSessionInput,
) -> CommandResult<WeComScanAuthorizationVm> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    runtime
        .start_wecom_scan(input.session_id)
        .await
        .map_err(wecom_scan_error)
}

#[tauri::command]
pub async fn complete_wecom_scan_authorization(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: WeComScanSessionInput,
) -> CommandResult<ImSettingsVm> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    let credentials = runtime
        .complete_wecom_scan(&input.session_id)
        .await
        .map_err(wecom_scan_error)?;
    install_scanned_wecom_credentials(&app_handle, &state, &runtime, credentials).await
}

#[tauri::command]
pub fn cancel_wecom_scan_authorization(
    state: tauri::State<'_, DesktopState>,
    input: WeComScanSessionInput,
) -> CommandResult<()> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    runtime.cancel_wecom_scan(Some(&input.session_id));
    Ok(())
}

#[tauri::command]
pub async fn set_im_channel_enabled(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: SetImChannelEnabledInput,
) -> CommandResult<ImSettingsVm> {
    let app = state.app().map_err(im_command_error)?;
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    let (settings, changed) = {
        let _write_guard = runtime
            .settings_write_lock
            .lock()
            .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
        let mut settings = app.load_settings().map_err(im_command_error)?;
        let changed = apply_enabled_update(&mut settings.im_integrations, &input)?;
        if changed {
            app.save_settings(&settings).map_err(im_command_error)?;
            state
                .update_settings_config(&settings)
                .map_err(im_command_error)?;
        }
        (settings, changed)
    };
    if changed {
        runtime
            .reconfigure(&app_handle)
            .await
            .map_err(im_command_error)?;
    }
    Ok(settings_vm(&settings.im_integrations, Some(&runtime)))
}

#[tauri::command]
pub fn save_im_notification_preferences(
    state: tauri::State<'_, DesktopState>,
    input: SaveImNotificationPreferencesInput,
) -> CommandResult<ImSettingsVm> {
    let app = state.app().map_err(im_command_error)?;
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    let _write_guard = runtime
        .settings_write_lock
        .lock()
        .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
    let mut settings = app.load_settings().map_err(im_command_error)?;
    if apply_notification_update(&mut settings.im_integrations, &input)? {
        app.save_settings(&settings).map_err(im_command_error)?;
        state
            .update_settings_config(&settings)
            .map_err(im_command_error)?;
        *runtime
            .targets
            .write()
            .map_err(|_| im_error("IM_RUNTIME_UNAVAILABLE"))? =
            projection_targets(&settings.im_integrations, &runtime.connectors);
    }
    Ok(settings_vm(&settings.im_integrations, Some(&runtime)))
}

#[tauri::command]
pub async fn reset_im_channel_binding(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ImGenerationInput,
) -> CommandResult<ImSettingsVm> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    let app = state.app().map_err(im_command_error)?;
    let (settings, changed) = {
        let _write_guard = runtime
            .settings_write_lock
            .lock()
            .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
        let mut settings = app.load_settings().map_err(im_command_error)?;
        let has_binding = settings
            .im_integrations
            .channels
            .iter()
            .find(|channel| channel.kind == input.kind)
            .ok_or_else(|| im_error("IM_CREDENTIAL_REQUIRED"))?
            .binding
            .is_some();
        if !has_binding {
            return Ok(settings_vm(&settings.im_integrations, Some(&runtime)));
        }
        let current_generation = runtime
            .connection_manager
            .snapshot(input.kind)
            .map(|snapshot| snapshot.generation)
            .unwrap_or(0);
        if current_generation != input.expected_generation {
            return Err(im_error("IM_STALE_GENERATION"));
        }
        let reset = apply_binding_reset(&mut settings.im_integrations, input.kind)?;
        debug_assert!(reset);
        app.save_settings(&settings).map_err(im_command_error)?;
        state
            .update_settings_config(&settings)
            .map_err(im_command_error)?;
        *runtime
            .targets
            .write()
            .map_err(|_| im_error("IM_RUNTIME_UNAVAILABLE"))? =
            projection_targets(&settings.im_integrations, &runtime.connectors);
        (settings, true)
    };
    debug_assert!(changed);
    runtime
        .reconfigure(&app_handle)
        .await
        .map_err(im_command_error)?;
    Ok(settings_vm(&settings.im_integrations, Some(&runtime)))
}

#[tauri::command]
pub async fn reconnect_im_channel(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ImGenerationInput,
) -> CommandResult<gold_band::im::ImChannelSnapshot> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    let current = runtime
        .connection_manager
        .snapshot(input.kind)
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    if current.generation != input.expected_generation {
        return Err(im_error("IM_STALE_GENERATION"));
    }
    let app = state.app().map_err(im_command_error)?;
    let settings = app.load_settings().map_err(im_command_error)?;
    let channel = settings
        .im_integrations
        .channels
        .iter()
        .find(|channel| channel.kind == input.kind)
        .ok_or_else(|| im_error("IM_CREDENTIAL_REQUIRED"))?;
    if !channel.enabled || channel.credential_ref.is_none() {
        return Err(im_error("IM_CREDENTIAL_REQUIRED"));
    }
    runtime
        .reconfigure(&app_handle)
        .await
        .map_err(im_command_error)?;
    runtime
        .connection_manager
        .snapshot(input.kind)
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))
}

#[tauri::command]
pub async fn delete_im_channel(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ImChannelInput,
) -> CommandResult<DeleteImChannelResultVm> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    let transition_runtime = Arc::clone(&runtime);
    let transition_handle = app_handle.clone();
    let channel = input.kind;
    let (settings, operation) = tauri::async_runtime::spawn_blocking(move || {
        let state = transition_handle.state::<DesktopState>();
        let app = state.app().map_err(im_command_error)?;
        let _write_guard = transition_runtime
            .settings_write_lock
            .lock()
            .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
        let mut settings = app.load_settings().map_err(im_command_error)?;
        let configured = settings
            .im_integrations
            .channels
            .iter()
            .find(|candidate| candidate.kind == channel)
            .cloned();
        let existing = transition_runtime
            .repository
            .channel_cleanup(channel)
            .map_err(|error| im_error(error.code()))?;
        let operation = if let Some(operation) = existing {
            operation
        } else {
            let operation = ImChannelCleanupOperation {
                operation_id: Uuid::new_v4().to_string(),
                channel,
                credential_ref: configured
                    .as_ref()
                    .and_then(|configured| configured.credential_ref.clone()),
                phase: ImChannelCleanupPhase::Prepared,
                last_error_code: None,
            };
            transition_runtime
                .repository
                .create_channel_cleanup(
                    &operation.operation_id,
                    channel,
                    operation.credential_ref.as_deref(),
                    chrono::Utc::now().timestamp_millis(),
                )
                .map_err(|error| im_error(error.code()))?;
            operation
        };
        settings
            .im_integrations
            .channels
            .retain(|candidate| candidate.kind != channel);
        if configured.is_some() {
            if let Err(error) = app.save_settings(&settings) {
                if operation.phase == ImChannelCleanupPhase::Prepared {
                    let _ = transition_runtime
                        .repository
                        .finish_channel_cleanup(&operation.operation_id);
                }
                return Err(im_command_error(error));
            }
        }
        *transition_runtime
            .targets
            .write()
            .map_err(|_| im_error("IM_RUNTIME_UNAVAILABLE"))? =
            projection_targets(&settings.im_integrations, &transition_runtime.connectors);
        let capabilities = transition_runtime
            .connectors
            .get(&channel)
            .map(|connector| connector.capabilities())
            .unwrap_or_default();
        let start = transition_runtime
            .connection_manager
            .replace(channel, false, capabilities);
        if let Some(connector) = transition_runtime.connectors.get(&channel) {
            connector.advance_generation(start.generation);
        }
        emit_snapshot(
            &transition_handle,
            transition_runtime.connection_manager.snapshot(channel),
        );
        if let Err(error) = state.update_settings_config(&settings) {
            warn!(error = %error, "update in-memory IM settings after durable delete failed");
        }
        transition_runtime
            .repository
            .update_channel_cleanup(
                &operation.operation_id,
                ImChannelCleanupPhase::SettingsRemoved,
                None,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| im_error(error.code()))?;
        Ok::<_, CommandErrorVm>((
            settings,
            ImChannelCleanupOperation {
                phase: ImChannelCleanupPhase::SettingsRemoved,
                ..operation
            },
        ))
    })
    .await
    .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))??;

    let operation_id = operation.operation_id.clone();
    let cleanup_status = match runtime.complete_channel_cleanup(operation, true).await {
        Ok(()) => ImChannelCleanupStatusVm::Complete,
        Err(code) => {
            warn!(
                channel = channel.as_str(),
                operation_id,
                error_code = code,
                "IM channel cleanup remains pending"
            );
            ImChannelCleanupStatusVm::Pending
        }
    };
    Ok(DeleteImChannelResultVm {
        settings: settings_vm(&settings.im_integrations, Some(&runtime)),
        operation_id,
        cleanup_status,
    })
}

fn settings_vm(
    settings: &ImIntegrationSettings,
    runtime: Option<&DesktopImRuntime>,
) -> ImSettingsVm {
    ImSettingsVm {
        channels: ImChannelKind::ALL
            .into_iter()
            .map(|kind| {
                let configured = settings
                    .channels
                    .iter()
                    .find(|channel| channel.kind == kind)
                    .cloned()
                    .unwrap_or_else(|| disabled_channel(kind));
                ImChannelSettingsVm {
                    kind,
                    enabled: configured.enabled,
                    public_identity: configured.public_identity,
                    credential_configured: configured.credential_ref.is_some(),
                    binding: configured.binding,
                    notifications: configured.notifications,
                    connection: runtime
                        .and_then(|runtime| runtime.connection_manager.snapshot(kind)),
                }
            })
            .collect(),
    }
}

fn upsert_channel(settings: &mut ImIntegrationSettings, channel: ImChannelSettings) {
    settings.channels.retain(|item| item.kind != channel.kind);
    settings.channels.push(channel);
}

fn apply_enabled_update(
    settings: &mut ImIntegrationSettings,
    input: &SetImChannelEnabledInput,
) -> CommandResult<bool> {
    let channel = settings
        .channels
        .iter_mut()
        .find(|channel| channel.kind == input.kind)
        .ok_or_else(|| im_error("IM_CREDENTIAL_REQUIRED"))?;
    if input.enabled && channel.credential_ref.is_none() {
        return Err(im_error("IM_CREDENTIAL_REQUIRED"));
    }
    if channel.enabled == input.enabled {
        return Ok(false);
    }
    channel.enabled = input.enabled;
    Ok(true)
}

fn apply_notification_update(
    settings: &mut ImIntegrationSettings,
    input: &SaveImNotificationPreferencesInput,
) -> CommandResult<bool> {
    let channel = settings
        .channels
        .iter_mut()
        .find(|channel| channel.kind == input.kind)
        .ok_or_else(|| im_error("IM_CREDENTIAL_REQUIRED"))?;
    if channel.notifications == input.notifications {
        return Ok(false);
    }
    channel.notifications = input.notifications.clone();
    Ok(true)
}

fn apply_binding_reset(
    settings: &mut ImIntegrationSettings,
    kind: ImChannelKind,
) -> CommandResult<bool> {
    let channel = settings
        .channels
        .iter_mut()
        .find(|channel| channel.kind == kind)
        .ok_or_else(|| im_error("IM_CREDENTIAL_REQUIRED"))?;
    if channel.binding.is_none() {
        return Ok(false);
    }
    channel.binding = None;
    Ok(true)
}

fn im_error(code: &str) -> CommandErrorVm {
    CommandErrorVm::new(code, serde_json::json!({}))
}

fn im_command_error(error: impl std::fmt::Display) -> CommandErrorVm {
    warn!(error = %error, "IM command failed");
    im_error("IM_OPERATION_FAILED")
}

fn wecom_scan_error(error: WeComScanAuthError) -> CommandErrorVm {
    CommandErrorVm::new(error.code(), serde_json::json!({}))
}

async fn install_scanned_wecom_credentials(
    app_handle: &AppHandle,
    state: &DesktopState,
    runtime: &Arc<DesktopImRuntime>,
    credentials: WeComScanCredentials,
) -> CommandResult<ImSettingsVm> {
    let app = state.app().map_err(im_command_error)?;
    let reference = OsImCredentialStore::new_reference();
    let payload = gold_band::im::ImCredentialPayload::new(BTreeMap::from([(
        "secret".to_owned(),
        credentials.secret,
    )]));
    let reference_for_save = reference.clone();
    tauri::async_runtime::spawn_blocking(move || {
        OsImCredentialStore.save(ImChannelKind::WeCom, &reference_for_save, &payload)
    })
    .await
    .map_err(|_| im_error("IM_CREDENTIAL_UNAVAILABLE"))?
    .map_err(|error| im_error(error.code()))?;

    let write_guard = runtime
        .settings_write_lock
        .lock()
        .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
    if runtime
        .repository
        .channel_cleanup(ImChannelKind::WeCom)
        .map_err(|error| im_error(error.code()))?
        .is_some()
    {
        drop(write_guard);
        let reference_for_delete = reference.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            OsImCredentialStore.delete(ImChannelKind::WeCom, &reference_for_delete)
        })
        .await;
        return Err(im_error("IM_CHANNEL_CLEANUP_PENDING"));
    }
    let mut settings = match app.load_settings() {
        Ok(settings) => settings,
        Err(error) => {
            drop(write_guard);
            let reference_for_delete = reference.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                OsImCredentialStore.delete(ImChannelKind::WeCom, &reference_for_delete)
            })
            .await;
            return Err(im_command_error(error));
        }
    };
    let original_settings = settings.clone();
    let existing = settings
        .im_integrations
        .channels
        .iter()
        .find(|channel| channel.kind == ImChannelKind::WeCom)
        .cloned();
    let old_reference = existing
        .as_ref()
        .and_then(|channel| channel.credential_ref.clone());
    let identity_changed = existing
        .as_ref()
        .is_some_and(|channel| channel.public_identity != credentials.bot_id);
    upsert_channel(
        &mut settings.im_integrations,
        ImChannelSettings {
            kind: ImChannelKind::WeCom,
            enabled: true,
            public_identity: credentials.bot_id,
            credential_ref: Some(reference.clone()),
            binding: if identity_changed {
                None
            } else {
                existing
                    .as_ref()
                    .and_then(|channel| channel.binding.clone())
            },
            notifications: existing
                .map(|channel| channel.notifications)
                .unwrap_or_default(),
        },
    );
    if let Err(error) = app.save_settings(&settings) {
        let _ = OsImCredentialStore.delete(ImChannelKind::WeCom, &reference);
        return Err(im_command_error(error));
    }
    if let Err(error) = state.update_settings_config(&settings) {
        let _ = app.save_settings(&original_settings);
        let _ = state.update_settings_config(&original_settings);
        let _ = OsImCredentialStore.delete(ImChannelKind::WeCom, &reference);
        return Err(im_command_error(error));
    }
    drop(write_guard);
    if let Some(old_reference) = old_reference {
        let delete_result = tauri::async_runtime::spawn_blocking(move || {
            OsImCredentialStore.delete(ImChannelKind::WeCom, &old_reference)
        })
        .await;
        if !matches!(delete_result, Ok(Ok(()))) {
            warn!("cleanup previous WeCom credential reference failed");
        }
    }
    runtime
        .reconfigure(app_handle)
        .await
        .map_err(im_command_error)?;
    Ok(settings_vm(&settings.im_integrations, Some(runtime)))
}

fn projection_targets(
    settings: &ImIntegrationSettings,
    connectors: &HashMap<ImChannelKind, Arc<dyn ImConnector>>,
) -> Vec<ImProjectionTarget> {
    settings
        .channels
        .iter()
        .map(|channel| ImProjectionTarget {
            channel: channel.kind,
            enabled: channel.enabled,
            credential_available: channel.credential_ref.is_some(),
            capabilities: connectors
                .get(&channel.kind)
                .map(|connector| connector.capabilities())
                .unwrap_or_default(),
            destination: channel.binding.as_ref().map(|binding| ImDestination {
                destination_id: binding.destination_id.clone(),
                conversation_id: binding.conversation_id.clone(),
                authorized_actor_id: binding.authorized_actor_id.clone(),
            }),
            notifications: channel.notifications.clone(),
        })
        .collect()
}

fn disabled_channel(kind: ImChannelKind) -> ImChannelSettings {
    ImChannelSettings {
        kind,
        enabled: false,
        public_identity: String::new(),
        credential_ref: None,
        binding: None,
        notifications: Default::default(),
    }
}

fn observed_binding(binding: &ImBindingSummary) -> ImObservedBinding {
    ImObservedBinding {
        destination_id: binding.destination_id.clone(),
        conversation_id: binding.conversation_id.clone(),
        actor_id: binding.authorized_actor_id.clone(),
        is_private: true,
    }
}

fn emit_snapshot(app_handle: &AppHandle, snapshot: Option<gold_band::im::ImChannelSnapshot>) {
    if let Some(snapshot) = snapshot {
        let _ = app_handle.emit(IM_CHANNEL_STATE_EVENT, snapshot);
    }
}

fn terminal_connector_event(
    generation: u64,
    result: Result<(), gold_band::im::ImIntegrationError>,
) -> Option<ImConnectorEvent> {
    result
        .err()
        .map(|error| ImConnectorEvent::Disconnected { generation, error })
}

fn event_project_id(event: &RuntimeLifecycleEvent) -> Option<&str> {
    match event {
        RuntimeLifecycleEvent::RunPaused { project_id, .. }
        | RuntimeLifecycleEvent::InterventionRequested { project_id, .. }
        | RuntimeLifecycleEvent::RunCompleted { project_id, .. }
        | RuntimeLifecycleEvent::AcpTurnFinished { project_id, .. } => Some(project_id),
        _ => None,
    }
}

fn event_canonical_id(event: &RuntimeLifecycleEvent) -> &str {
    match event {
        RuntimeLifecycleEvent::RunPaused { event_id, .. }
        | RuntimeLifecycleEvent::InterventionRequested { event_id, .. }
        | RuntimeLifecycleEvent::RunCompleted { event_id, .. }
        | RuntimeLifecycleEvent::AcpTurnFinished { event_id, .. } => event_id,
        _ => "unsupported-lifecycle-event",
    }
}

fn handle_projection_completion(
    worker_senders: &HashMap<ImChannelKind, mpsc::Sender<ImWorkerSignal>>,
    completion: ImProjectionCompletion,
) {
    match completion.result {
        Ok(result) => {
            tracing::info!(
                canonical_event_id = completion.canonical_event_id,
                inserted = result.inserted,
                duplicates = result.duplicates,
                skipped = result.skipped,
                "IM lifecycle projection completed"
            );
            if result.inserted > 0 {
                for sender in worker_senders.values() {
                    let _ = sender.try_send(ImWorkerSignal::DeliveryAvailable);
                }
            }
        }
        Err(error_code) => warn!(
            canonical_event_id = completion.canonical_event_id,
            error_code, "IM lifecycle projection failed"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_channel_result_uses_stable_cleanup_status_contract() {
        let result = DeleteImChannelResultVm {
            settings: ImSettingsVm {
                channels: Vec::new(),
            },
            operation_id: "cleanup-1".into(),
            cleanup_status: ImChannelCleanupStatusVm::Pending,
        };

        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({
                "settings": { "channels": [] },
                "operationId": "cleanup-1",
                "cleanupStatus": "pending",
            })
        );
        assert_eq!(
            serde_json::to_value(ImChannelCleanupStatusVm::Complete).unwrap(),
            serde_json::json!("complete")
        );
    }

    fn vote_delivery() -> gold_band::im::ImDelivery {
        gold_band::im::ImDelivery {
            delivery_id: "delivery-1".into(),
            channel: ImChannelKind::WeCom,
            destination: ImDestination {
                destination_id: "user-1".into(),
                conversation_id: "user-1".into(),
                authorized_actor_id: "user-1".into(),
            },
            notification_kind: gold_band::im::ImNotificationKind::Permission,
            canonical_event_id: "event-1".into(),
            payload: ImDeliveryPayload::Intervention {
                version: gold_band::im::IM_PAYLOAD_VERSION,
                reference: gold_band::im::InterventionRef {
                    locator: gold_band::app::intervention::InterventionLocator {
                        project_id: "p".into(),
                        task_id: "t".into(),
                        run_id: "r".into(),
                        round_id: "round".into(),
                        node_id: "n".into(),
                        attempt_id: "a".into(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    request:
                        gold_band::app::intervention::InterventionRequestIdentity::Permission {
                            request_id: "permission-1".into(),
                        },
                    expected_state: "state-1".into(),
                    allowed_actions: vec![
                        gold_band::app::intervention::InterventionAllowedAction::PermissionOption {
                            option_id: "allow".into(),
                            name: "Allow".into(),
                            permission_kind:
                                gold_band::app::intervention::PermissionActionKind::AllowOnce,
                        },
                        gold_band::app::intervention::InterventionAllowedAction::PermissionOption {
                            option_id: "allow_for_session".into(),
                            name: "Allow for session".into(),
                            permission_kind:
                                gold_band::app::intervention::PermissionActionKind::AllowAlways,
                        },
                        gold_band::app::intervention::InterventionAllowedAction::PermissionOption {
                            option_id: "cancel".into(),
                            name: "Cancel".into(),
                            permission_kind:
                                gold_band::app::intervention::PermissionActionKind::RejectOnce,
                        },
                    ],
                    expires_at_ms: None,
                },
                presentation: gold_band::im::InterventionPresentation {
                    title_key: "im.notification.permission.title".into(),
                    summary_key: "im.notification.permission.summary".into(),
                    title: None,
                    summary: None,
                    body: None,
                    context: None,
                    fields: BTreeMap::new(),
                    questions: Vec::new(),
                },
            },
            expires_at_ms: i64::MAX,
            display_ref: None,
        }
    }

    fn vote_envelope(index: usize) -> gold_band::im::ImInboundEnvelope {
        gold_band::im::ImInboundEnvelope {
            action_id: "action-1".into(),
            channel: ImChannelKind::WeCom,
            platform_event_id: "platform-event-1".into(),
            conversation_id: "user-1".into(),
            actor_id: "user-1".into(),
            delivery_id: "delivery-1".into(),
            action: gold_band::im::ImInboundActionSelection::LocalIndex { index },
        }
    }

    fn processed_wecom_vote_context() -> ImActionResponseContext {
        ImActionResponseContext::WeCom {
            req_id: "callback-req-1".into(),
            task_id: "delivery-1".into(),
            vote_selection: Some(gold_band::im::ImWeComVoteSelection {
                selected_option_id: "1".into(),
                vote_actions: vec![
                    gold_band::app::intervention::InterventionAllowedAction::PermissionOption {
                        option_id: "allow".into(),
                        name: "Allow".into(),
                        permission_kind:
                            gold_band::app::intervention::PermissionActionKind::AllowOnce,
                    },
                ],
                display_ref: None,
            }),
            form_selection: None,
        }
    }

    #[test]
    fn wecom_vote_context_is_enriched_only_from_the_current_outbox_selection() {
        let mut delivery = vote_delivery();
        delivery.display_ref = Some(80);
        let mut context = ImActionResponseContext::WeCom {
            req_id: "callback-req-1".into(),
            task_id: "delivery-1".into(),
            vote_selection: Some(gold_band::im::ImWeComVoteSelection {
                selected_option_id: "1".into(),
                vote_actions: Vec::new(),
                display_ref: None,
            }),
            form_selection: None,
        };
        enrich_wecom_vote_context(&mut context, &vote_envelope(1), &delivery);
        let ImActionResponseContext::WeCom {
            vote_selection: Some(selection),
            ..
        } = context
        else {
            panic!("expected WeCom vote context");
        };
        assert_eq!(
            selection
                .vote_actions
                .iter()
                .map(|action| match action {
                    gold_band::app::intervention::InterventionAllowedAction::PermissionOption {
                        option_id,
                        ..
                    } => option_id.clone(),
                    _ => unreachable!("fixture only contains permission options"),
                })
                .collect::<Vec<_>>(),
            vec![
                "allow".to_owned(),
                "allow_for_session".into(),
                "cancel".into()
            ]
        );
        assert_eq!(selection.display_ref, Some(80));

        let mut mismatch = ImActionResponseContext::WeCom {
            req_id: "callback-req-2".into(),
            task_id: "delivery-1".into(),
            vote_selection: Some(gold_band::im::ImWeComVoteSelection {
                selected_option_id: "1".into(),
                vote_actions: Vec::new(),
                display_ref: None,
            }),
            form_selection: None,
        };
        enrich_wecom_vote_context(&mut mismatch, &vote_envelope(2), &delivery);
        let ImActionResponseContext::WeCom {
            vote_selection: Some(selection),
            ..
        } = mismatch
        else {
            panic!("expected WeCom vote context");
        };
        assert!(selection.vote_actions.is_empty());
    }

    #[test]
    fn wecom_form_context_is_enriched_from_the_current_outbox_form() {
        let mut delivery = vote_delivery();
        delivery.display_ref = Some(80);
        let form = gold_band::app::intervention::RemoteElicitationForm::MultiScalarChoice {
            question: gold_band::app::intervention::RemoteScalarChoiceQuestion {
                selector_key: "q0".into(),
                field_name: "features".into(),
                title: "features".into(),
                description: None,
                required: true,
                options: vec![gold_band::app::intervention::RemoteElicitationOption {
                    value: serde_json::json!("auth"),
                    label: "auth".into(),
                    description: None,
                }],
            },
            allows_empty: false,
        };
        if let gold_band::im::ImDeliveryPayload::Intervention { reference, .. } =
            &mut delivery.payload
        {
            reference.allowed_actions = vec![
                gold_band::app::intervention::InterventionAllowedAction::ElicitationFixedForm {
                    form: form.clone(),
                },
            ];
        }
        let selections = vec![
            gold_band::app::intervention::RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["0".into()],
            },
        ];
        let mut envelope = vote_envelope(0);
        envelope.action = gold_band::im::ImInboundActionSelection::Form {
            selections: selections.clone(),
        };
        let mut context = ImActionResponseContext::WeCom {
            req_id: "callback-req-1".into(),
            task_id: "delivery-1".into(),
            vote_selection: None,
            form_selection: Some(gold_band::im::ImWeComFormSelection {
                card_kind: gold_band::im::ImWeComFormCardKind::VoteSingle,
                selections,
                form: None,
                display_ref: None,
            }),
        };
        enrich_wecom_form_context(&mut context, &envelope, &delivery);
        let ImActionResponseContext::WeCom {
            form_selection: Some(form_selection),
            ..
        } = context
        else {
            panic!("expected WeCom form context");
        };
        assert_eq!(
            form_selection.card_kind,
            gold_band::im::ImWeComFormCardKind::VoteMulti
        );
        assert_eq!(form_selection.form.as_ref(), Some(&form));
        assert_eq!(form_selection.display_ref, Some(80));
    }

    #[test]
    fn inbound_action_settlement_preserves_the_processed_wecom_context() {
        let processed = processed_wecom_vote_context();
        let expected = processed.clone();
        let (context, state) = settle_inbound_action(
            ImChannelKind::WeCom,
            1,
            Ok::<_, std::io::Error>((processed, Ok(()))),
            processed_wecom_vote_context(),
        );
        assert_eq!(context, expected);
        assert_eq!(state, ImMessageState::Handled);

        let processed = processed_wecom_vote_context();
        let expected = processed.clone();
        let (context, state) = settle_inbound_action(
            ImChannelKind::WeCom,
            1,
            Ok::<_, std::io::Error>((
                processed,
                Err(anyhow::Error::new(
                    gold_band::im::ImInboundError::InformationNotActionable,
                )),
            )),
            processed_wecom_vote_context(),
        );
        assert_eq!(context, expected);
        assert_eq!(state, ImMessageState::Failed);
    }

    #[tokio::test]
    async fn inbound_action_join_failure_keeps_the_original_channel_context() {
        let task = tokio::spawn(async {});
        task.abort();
        let join_error = task.await.unwrap_err();
        let fallback = processed_wecom_vote_context();
        let expected = fallback.clone();

        let (context, state) =
            settle_inbound_action(ImChannelKind::WeCom, 1, Err(join_error), fallback);

        assert_eq!(context, expected);
        assert_eq!(state, ImMessageState::Failed);
    }

    #[test]
    fn projection_targets_keep_the_wecom_installation_binding() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let connectors = HashMap::from([(
            ImChannelKind::WeCom,
            Arc::new(WeComConnector::new(codec)) as Arc<dyn ImConnector>,
        )]);
        let settings = ImIntegrationSettings {
            channels: vec![ImChannelSettings {
                kind: ImChannelKind::WeCom,
                enabled: true,
                public_identity: "bot-id".into(),
                credential_ref: Some(uuid::Uuid::new_v4().to_string()),
                binding: Some(ImBindingSummary {
                    destination_id: "user-1".into(),
                    conversation_id: "private-chat".into(),
                    authorized_actor_id: "user-1".into(),
                    display_name: "User".into(),
                }),
                notifications: Default::default(),
            }],
        };
        let targets = projection_targets(&settings, &connectors);
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].destination.as_ref().unwrap().authorized_actor_id,
            "user-1"
        );
    }

    #[test]
    fn desktop_resolution_projects_all_intervention_kinds_and_wakes_the_worker() {
        let temp = tempfile::tempdir().unwrap();
        let repository = ImRepository::new(
            camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        );
        let (sender, mut receiver) = mpsc::channel(8);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);

        for (index, kind) in [
            gold_band::im::ImNotificationKind::Permission,
            gold_band::im::ImNotificationKind::Elicitation,
            gold_band::im::ImNotificationKind::ManualCheck,
        ]
        .into_iter()
        .enumerate()
        {
            let source_event_id = format!("event-{index}");
            let mut source = vote_delivery();
            source.notification_kind = kind;
            source.canonical_event_id = source_event_id.clone();
            source.refresh_id();
            let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
                unreachable!("fixture is an intervention")
            };
            let locator = reference.locator.clone();
            repository.enqueue(&source, 100 + index as i64).unwrap();
            let expected_display_ref = repository
                .intervention_deliveries_for_canonical_event(&source_event_id)
                .unwrap()[0]
                .display_ref;

            let result = project_desktop_intervention_resolution(
                &repository,
                &worker_senders,
                &[],
                &source_event_id,
                kind,
                &locator,
                200 + index as i64,
            )
            .unwrap();
            assert_eq!(result.inserted, 1);
            assert_eq!(
                receiver.try_recv().unwrap(),
                ImWorkerSignal::DeliveryAvailable
            );

            let terminal_event_id = format!("{source_event_id}:desktop-resolved");
            let terminal_id = ImDelivery::deterministic_id(
                source.channel,
                &source.destination.destination_id,
                kind,
                &terminal_event_id,
            );
            let terminal = repository.delivery(&terminal_id).unwrap().unwrap();
            let ImDeliveryPayload::InterventionResolution { resolution, .. } = terminal.payload
            else {
                panic!("expected desktop resolution payload");
            };
            assert_eq!(resolution.notification_kind, kind);
            assert_eq!(resolution.display_ref, expected_display_ref);

            let replay = project_desktop_intervention_resolution(
                &repository,
                &worker_senders,
                &[],
                &source_event_id,
                kind,
                &locator,
                300 + index as i64,
            )
            .unwrap();
            assert_eq!(replay.duplicates, 1);
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn desktop_resolution_wins_when_it_is_persisted_before_the_source_projection() {
        let temp = tempfile::tempdir().unwrap();
        let repository = ImRepository::new(
            camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        );
        let (sender, mut receiver) = mpsc::channel(2);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);
        let mut source = vote_delivery();
        source.refresh_id();
        let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
            unreachable!("fixture is an intervention")
        };
        let target = ImProjectionTarget {
            channel: ImChannelKind::WeCom,
            enabled: true,
            credential_available: true,
            capabilities: WeComConnector::new(ImActionTokenCodec::new(vec![7; 32]).unwrap())
                .capabilities(),
            destination: Some(source.destination.clone()),
            notifications: Default::default(),
        };

        let result = project_desktop_intervention_resolution(
            &repository,
            &worker_senders,
            &[target],
            &source.canonical_event_id,
            source.notification_kind,
            &reference.locator,
            100,
        )
        .unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(
            receiver.try_recv().unwrap(),
            ImWorkerSignal::DeliveryAvailable
        );

        assert_eq!(
            repository.enqueue(&source, 101).unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
        assert!(
            repository
                .intervention_deliveries_for_canonical_event(&source.canonical_event_id)
                .unwrap()
                .is_empty()
        );
        let terminal_id = ImDelivery::deterministic_id(
            source.channel,
            &source.destination.destination_id,
            source.notification_kind,
            &desktop_resolution_event_id(&source.canonical_event_id),
        );
        let terminal = repository.delivery(&terminal_id).unwrap().unwrap();
        assert!(matches!(
            terminal.payload,
            ImDeliveryPayload::InterventionResolution { .. }
        ));
        assert_eq!(terminal.display_ref, Some(1));
    }

    #[test]
    fn settings_view_model_exposes_only_credential_presence() {
        let credential_reference = "credential-reference-must-stay-backend";
        let settings = ImIntegrationSettings {
            channels: vec![ImChannelSettings {
                kind: ImChannelKind::WeCom,
                enabled: false,
                public_identity: "bot-id".into(),
                credential_ref: Some(credential_reference.into()),
                binding: None,
                notifications: Default::default(),
            }],
        };
        let view = settings_vm(&settings, None);
        assert_eq!(view.channels.len(), 1);
        assert_eq!(view.channels[0].kind, ImChannelKind::WeCom);
        let serialized = serde_json::to_string(&view).unwrap();
        assert!(serialized.contains("\"credentialConfigured\":true"));
        assert!(!serialized.contains(credential_reference));
        assert!(!serialized.contains("credentialRef"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("token"));
    }

    #[test]
    fn enabled_input_rejects_removed_settings_and_manual_credentials_fields() {
        let input = serde_json::json!({
            "kind": "weCom",
            "enabled": true,
            "credentials": { "secret": "value" },
        });
        assert!(serde_json::from_value::<SetImChannelEnabledInput>(input).is_err());
    }

    #[test]
    fn settings_input_rejects_removed_scheduled_notification_fields() {
        for field in [
            "scheduledCompletion",
            "scheduledFailure",
            "scheduledAttention",
            "scheduledMissed",
        ] {
            let mut input = serde_json::json!({
                "kind": "weCom",
                "notifications": gold_band::im::ImNotificationPreferences::default(),
            });
            input["notifications"][field] = serde_json::json!(true);
            assert!(
                serde_json::from_value::<SaveImNotificationPreferencesInput>(input).is_err(),
                "accepted removed field {field}"
            );
        }
    }

    #[test]
    fn narrow_setting_updates_are_idempotent_and_do_not_overwrite_other_owners() {
        let original_notifications = gold_band::im::ImNotificationPreferences::default();
        let binding = ImBindingSummary {
            destination_id: "user-1".into(),
            conversation_id: "chat-1".into(),
            authorized_actor_id: "user-1".into(),
            display_name: "User".into(),
        };
        let mut settings = ImIntegrationSettings {
            channels: vec![ImChannelSettings {
                kind: ImChannelKind::WeCom,
                enabled: true,
                public_identity: "bot-id".into(),
                credential_ref: Some("credential-ref".into()),
                binding: Some(binding.clone()),
                notifications: original_notifications.clone(),
            }],
        };

        assert!(
            apply_enabled_update(
                &mut settings,
                &SetImChannelEnabledInput {
                    kind: ImChannelKind::WeCom,
                    enabled: false,
                },
            )
            .unwrap()
        );
        assert!(
            !apply_enabled_update(
                &mut settings,
                &SetImChannelEnabledInput {
                    kind: ImChannelKind::WeCom,
                    enabled: false,
                },
            )
            .unwrap()
        );
        let channel = &settings.channels[0];
        assert_eq!(channel.public_identity, "bot-id");
        assert_eq!(channel.binding, Some(binding.clone()));
        assert_eq!(channel.notifications, original_notifications);

        let mut changed_notifications = original_notifications.clone();
        changed_notifications.run_success = true;
        assert!(
            apply_notification_update(
                &mut settings,
                &SaveImNotificationPreferencesInput {
                    kind: ImChannelKind::WeCom,
                    notifications: changed_notifications.clone(),
                },
            )
            .unwrap()
        );
        assert!(
            !apply_notification_update(
                &mut settings,
                &SaveImNotificationPreferencesInput {
                    kind: ImChannelKind::WeCom,
                    notifications: changed_notifications.clone(),
                },
            )
            .unwrap()
        );
        let channel = &settings.channels[0];
        assert!(!channel.enabled);
        assert_eq!(channel.binding, Some(binding));
        assert_eq!(channel.notifications, changed_notifications);

        assert!(apply_binding_reset(&mut settings, ImChannelKind::WeCom).unwrap());
        assert!(!apply_binding_reset(&mut settings, ImChannelKind::WeCom).unwrap());
        let channel = &settings.channels[0];
        assert!(!channel.enabled);
        assert_eq!(channel.public_identity, "bot-id");
        assert_eq!(channel.credential_ref.as_deref(), Some("credential-ref"));
        assert_eq!(channel.notifications, changed_notifications);
        assert!(channel.binding.is_none());
    }

    #[test]
    fn binding_is_committed_only_after_persistence_succeeds() {
        let manager = ImConnectionManager::default();
        let start = manager.replace(ImChannelKind::WeCom, true, Default::default());
        let binding = ImObservedBinding {
            destination_id: "user-1".into(),
            conversation_id: "chat-1".into(),
            actor_id: "user-1".into(),
            is_private: true,
        };

        let failed = persist_binding_then_commit(
            &manager,
            ImChannelKind::WeCom,
            start.generation,
            binding.clone(),
            || anyhow::bail!("storage unavailable"),
        );
        assert!(failed.is_err());
        assert!(
            manager
                .snapshot(ImChannelKind::WeCom)
                .unwrap()
                .binding
                .is_none()
        );

        assert!(
            persist_binding_then_commit(
                &manager,
                ImChannelKind::WeCom,
                start.generation,
                binding.clone(),
                || Ok(()),
            )
            .unwrap()
        );
        assert_eq!(
            manager.snapshot(ImChannelKind::WeCom).unwrap().binding,
            Some(binding)
        );
    }

    #[test]
    fn scan_authorization_view_model_contains_no_source_or_credentials() {
        let serialized = serde_json::to_string(&WeComScanAuthorizationVm {
            session_id: Uuid::new_v4().to_string(),
            auth_url: "https://work.weixin.qq.com/ai/qc/c?s=short-lived".into(),
            expires_at_ms: 123,
        })
        .unwrap();
        for forbidden in ["secret", "botId", "source", "halo"] {
            assert!(!serialized.contains(forbidden), "found {forbidden}");
        }
    }

    #[test]
    fn connector_terminal_error_keeps_generation_and_stable_error_code() {
        let event = terminal_connector_event(
            7,
            Err(gold_band::im::ImIntegrationError::permanent(
                gold_band::im::ImErrorCode::AuthenticationRequired,
            )),
        )
        .expect("terminal connector failure must update connection state");
        let ImConnectorEvent::Disconnected { generation, error } = event else {
            panic!("expected disconnected event")
        };
        assert_eq!(generation, 7);
        assert_eq!(error.code.as_str(), "IM_AUTHENTICATION_REQUIRED");
        assert!(!error.retryable);
        assert!(terminal_connector_event(8, Ok(())).is_none());
    }

    #[test]
    fn inbound_error_codes_map_to_sanitized_card_states() {
        assert_eq!(
            inbound_failure_state("IM_ACTION_EXPIRED"),
            ImMessageState::Expired
        );
        assert_eq!(
            inbound_failure_state("INTERVENTION_ALREADY_HANDLED"),
            ImMessageState::Handled
        );
        assert_eq!(
            inbound_failure_state("IM_ACTION_TOKEN_INVALID"),
            ImMessageState::Failed
        );
    }
}
