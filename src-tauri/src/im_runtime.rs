use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use anyhow::{Context, Result};
use gold_band::app::intervention::{
    InterventionCommandService, InterventionLocator, InterventionSettlement,
};
use gold_band::app::{App, RuntimeLifecycleEvent};
use gold_band::im::{
    FeishuConnector, FeishuScanAuthClient, FeishuScanAuthError, FeishuScanAuthorization,
    FeishuScanCredentials, IM_PAYLOAD_VERSION, ImActionResponse, ImActionResponseContext,
    ImActionResponseState, ImActionTokenCodec, ImBindingSummary, ImChannelCleanupOperation,
    ImChannelCleanupPhase, ImChannelKind, ImChannelSettings, ImConnectionManager, ImConnector,
    ImConnectorEvent, ImCredentialStore, ImDelivery, ImDeliveryInsertResult, ImDeliveryPayload,
    ImDeliveryWorker, ImDestination, ImErrorCode, ImInboundActionService, ImInboundEnvelope,
    ImInboundHandling, ImInboundOutcome, ImIntegrationSettings, ImLifecycleProjectionJob,
    ImLifecycleProjectionQueueItem, ImLifecycleProjector, ImLocale, ImMaintenanceWorker,
    ImMessageUpdateMode, ImNavigationLocator, ImNotificationKind, ImObservedBinding,
    ImProjectionCompletion, ImProjectionEnqueueOutcome, ImProjectionResult, ImProjectionTarget,
    ImRepository, ImRepositoryError, ImResolutionDispatch, ImResolutionOutcome, ImResolvedBy,
    ImWorkerSignal, InterventionRef, InterventionResolutionNotification, OsImCredentialStore,
    ResolvedImChannelConfig, WECOM_SCAN_SESSION_TTL, WeComConnector, WeComScanAuthClient,
    WeComScanAuthError, WeComScanCredentials, drain_projection_queue,
    im_lifecycle_projection_channel, im_worker_channel, resolution_event_id,
    try_enqueue_projection,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

use crate::commands::{
    CommandErrorVm, CommandResult, execute_intervention_command, resolve_command_app,
    resolve_command_app_with_emitters, resolve_command_app_with_workspace_label,
};
use crate::im_scan_session::{ScanSessionSlot, ScanSlotError};
use crate::state::DesktopState;

pub const IM_CHANNEL_STATE_EVENT: &str = "im-channel-state-updated";
pub const IM_PROJECTION_DIAGNOSTIC_EVENT: &str = "im-projection-diagnostic";
const TERMINAL_CONFIRMATION_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
const BINDING_PERSISTENCE_RETRY_LIMIT: usize = 3;
const IM_PROJECTION_ALERT_COOLDOWN_MS: i64 = 30_000;
const IM_PROJECTION_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const IM_CHANNEL_CLEANUP_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const IM_WORKER_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const IM_SETTLEMENT_RECONCILE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const IM_SETTLEMENT_SIGNAL_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(1);

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
pub struct ScanSessionInput {
    pub session_id: String,
}

/// The QR code a scan dialog renders, shared by every channel's scan flow.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImScanAuthorizationVm {
    pub session_id: String,
    pub qr_url: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuScanStartInput {
    pub session_id: String,
    /// Re-authorizes the configured app instead of creating a new one.
    pub reauthorize: bool,
}

impl From<ScanSlotError> for WeComScanAuthError {
    fn from(error: ScanSlotError) -> Self {
        match error {
            ScanSlotError::Cancelled => Self::Cancelled,
            ScanSlotError::Expired => Self::Expired,
            ScanSlotError::SessionConflict => Self::SessionConflict,
            ScanSlotError::ProtocolInvalid => Self::ProtocolInvalid,
        }
    }
}

impl From<ScanSlotError> for FeishuScanAuthError {
    fn from(error: ScanSlotError) -> Self {
        match error {
            ScanSlotError::Cancelled => Self::Cancelled,
            ScanSlotError::Expired => Self::Expired,
            ScanSlotError::SessionConflict => Self::SessionConflict,
            ScanSlotError::ProtocolInvalid => Self::ProtocolInvalid,
        }
    }
}

struct ActiveBindingPersistenceRetry {
    generation: u64,
    cancellation: CancellationToken,
}

/// Outcome of `process_inbound`. `delivery` is the record the click belongs to
/// whenever it could be loaded, including rejections, so the connector can
/// render the reply card from it.
struct ProcessedInboundAction {
    delivery: Option<ImDelivery>,
    handling: std::result::Result<ImInboundHandling, ImErrorCode>,
    settled_resolution: Option<SettledResult>,
}

/// The winner of a request that was already settled when the click arrived.
#[derive(Debug, Clone)]
struct SettledResult {
    resolved_by: ImResolvedBy,
    outcome: ImResolutionOutcome,
}

impl From<InterventionResolutionNotification> for SettledResult {
    fn from(resolution: InterventionResolutionNotification) -> Self {
        Self {
            resolved_by: resolution.resolved_by,
            outcome: resolution.outcome,
        }
    }
}

/// Resolution to broadcast after a channel's command was `Accepted`.
struct AcceptedInboundAction {
    source_event_id: String,
    source_delivery_id: String,
    outcome: ImResolutionOutcome,
}

#[derive(Debug, Default)]
struct InboundTiming {
    delivery_lookup_ms: u128,
    workspace_resolve_ms: u128,
    settings_read_ms: u128,
    inbound_service_ms: u128,
    domain_command_ms: u128,
}

impl InboundTiming {
    fn elapsed_ms(started: Instant) -> u128 {
        started.elapsed().as_millis()
    }
}

pub struct DesktopImRuntime {
    repository: Arc<ImRepository>,
    credential_store: OsImCredentialStore,
    inbound_service: Arc<ImInboundActionService>,
    connection_manager: Arc<ImConnectionManager>,
    connectors: HashMap<ImChannelKind, Arc<dyn ImConnector>>,
    targets: RwLock<Vec<ImProjectionTarget>>,
    projection_sender: mpsc::Sender<ImLifecycleProjectionQueueItem>,
    projection_receiver: Mutex<Option<mpsc::Receiver<ImLifecycleProjectionQueueItem>>>,
    connector_event_sender: mpsc::Sender<(ImChannelKind, ImConnectorEvent)>,
    connector_event_receiver: Mutex<Option<mpsc::Receiver<(ImChannelKind, ImConnectorEvent)>>>,
    worker_senders: HashMap<ImChannelKind, mpsc::Sender<ImWorkerSignal>>,
    worker_receivers: Mutex<HashMap<ImChannelKind, mpsc::Receiver<ImWorkerSignal>>>,
    worker_tasks: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
    wecom_scan_client: WeComScanAuthClient,
    wecom_scan_session: ScanSessionSlot<String>,
    feishu_scan_client: FeishuScanAuthClient,
    feishu_scan_session: ScanSessionSlot<FeishuScanAuthorization>,
    settings_write_lock: Mutex<()>,
    binding_persistence_retries: Mutex<HashMap<ImChannelKind, ActiveBindingPersistenceRetry>>,
    last_projection_alert_at_ms: AtomicI64,
    inbound_timing_sequence: AtomicU64,
    /// Coalescing wake-up for the settlement reconciler (capacity 1).
    settlement_signal: mpsc::Sender<()>,
    settlement_receiver: Mutex<Option<mpsc::Receiver<()>>>,
    cancellation: CancellationToken,
}

impl DesktopImRuntime {
    pub fn new(core_db_path: camino::Utf8PathBuf) -> Result<Arc<Self>> {
        let credential_store = OsImCredentialStore;
        let signing_key = credential_store
            .load_or_create_action_signing_key()
            .context("initialize IM action signing key")?;
        Self::with_signing_key(core_db_path, signing_key)
    }

    fn with_signing_key(
        core_db_path: camino::Utf8PathBuf,
        signing_key: Vec<u8>,
    ) -> Result<Arc<Self>> {
        let credential_store = OsImCredentialStore;
        let token_codec = ImActionTokenCodec::new(signing_key)
            .map_err(|error| anyhow::anyhow!(error.code().as_str()))?;
        let repository = Arc::new(ImRepository::new(core_db_path));
        let inbound_service = Arc::new(ImInboundActionService::new(
            Arc::clone(&repository),
            token_codec.clone(),
        ));
        let wecom_scan_client =
            WeComScanAuthClient::new(gold_band::config::wecom_scan_auth_config().source.clone())
                .map_err(|error| anyhow::anyhow!(error.code()))?;
        let feishu_scan_client =
            FeishuScanAuthClient::new(gold_band::config::feishu_scan_auth_config())
                .map_err(|error| anyhow::anyhow!(error.code()))?;
        let (projection_sender, projection_receiver) = im_lifecycle_projection_channel();
        let (raw_event_sender, connector_event_receiver) = mpsc::channel(128);
        let mut connectors = HashMap::<ImChannelKind, Arc<dyn ImConnector>>::new();
        connectors.insert(
            ImChannelKind::WeCom,
            Arc::new(WeComConnector::new(token_codec.clone())),
        );
        connectors.insert(
            ImChannelKind::Feishu,
            Arc::new(FeishuConnector::new(token_codec)),
        );
        let mut worker_senders = HashMap::new();
        let mut worker_receivers = HashMap::new();
        for channel in ImChannelKind::ALL {
            let (sender, receiver) = im_worker_channel();
            worker_senders.insert(channel, sender);
            worker_receivers.insert(channel, receiver);
        }
        let (settlement_signal, settlement_receiver) = mpsc::channel(1);
        Ok(Arc::new(Self {
            repository,
            credential_store,
            inbound_service,
            connection_manager: Arc::new(ImConnectionManager::default()),
            connectors,
            targets: RwLock::new(Vec::new()),
            projection_sender,
            projection_receiver: Mutex::new(Some(projection_receiver)),
            connector_event_sender: raw_event_sender,
            connector_event_receiver: Mutex::new(Some(connector_event_receiver)),
            worker_senders,
            worker_receivers: Mutex::new(worker_receivers),
            worker_tasks: Mutex::new(Vec::new()),
            wecom_scan_client,
            wecom_scan_session: ScanSessionSlot::default(),
            feishu_scan_client,
            feishu_scan_session: ScanSessionSlot::default(),
            settings_write_lock: Mutex::new(()),
            binding_persistence_retries: Mutex::new(HashMap::new()),
            last_projection_alert_at_ms: AtomicI64::new(i64::MIN),
            inbound_timing_sequence: AtomicU64::new(0),
            settlement_signal,
            settlement_receiver: Mutex::new(Some(settlement_receiver)),
            cancellation: CancellationToken::new(),
        }))
    }

    pub fn configure_projection_targets(&self, settings: &ImIntegrationSettings) -> Result<usize> {
        let targets = projection_targets(settings, &self.connectors);
        let target_count = targets.len();
        *self
            .targets
            .write()
            .map_err(|_| anyhow::anyhow!("IM_RUNTIME_UNAVAILABLE"))? = targets;
        Ok(target_count)
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
            let task = tauri::async_runtime::spawn(worker.run(receiver, cancellation));
            self.worker_tasks
                .lock()
                .expect("IM worker task lock poisoned")
                .push(task);
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
        let settlement_receiver = self
            .settlement_receiver
            .lock()
            .expect("IM settlement receiver lock poisoned")
            .take();
        if let Some(receiver) = settlement_receiver {
            let runtime = Arc::clone(&self);
            let handle = app_handle.clone();
            let cancellation = self.cancellation.child_token();
            tauri::async_runtime::spawn(async move {
                runtime
                    .run_settlement_reconciler(handle, receiver, cancellation)
                    .await;
            });
        }
        let cleanup_runtime = Arc::clone(&self);
        let cleanup_handle = app_handle.clone();
        let cleanup_cancellation = self.cancellation.child_token();
        tauri::async_runtime::spawn(async move {
            cleanup_runtime
                .run_channel_cleanup_recovery(cleanup_handle, cleanup_cancellation)
                .await;
        });
    }

    /// Finalizes cards of requests the runtime ended without an answer. A
    /// lifecycle boundary wakes it (debounced so teardown finishes first); the
    /// interval covers stop paths that emit no lifecycle event.
    async fn run_settlement_reconciler(
        self: Arc<Self>,
        app_handle: AppHandle,
        mut signals: mpsc::Receiver<()>,
        cancellation: CancellationToken,
    ) {
        let mut interval = tokio::time::interval(IM_SETTLEMENT_RECONCILE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = interval.tick() => {}
                signal = signals.recv() => {
                    if signal.is_none() {
                        break;
                    }
                    tokio::time::sleep(IM_SETTLEMENT_SIGNAL_DEBOUNCE).await;
                }
            }
            self.reconcile_runtime_cancellations(&app_handle).await;
        }
    }

    async fn reconcile_runtime_cancellations(self: &Arc<Self>, app_handle: &AppHandle) {
        let Ok(Some(targets)) = actionable_projection_target_snapshot(&self.targets) else {
            return;
        };
        let runtime = Arc::clone(self);
        let handle = app_handle.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let sources = runtime.repository.open_intervention_sources(now_ms)?;
            if sources.is_empty() {
                return Ok(0);
            }
            let state = handle.state::<DesktopState>();
            let mut apps = HashMap::new();
            project_runtime_cancellations(
                &runtime.repository,
                &runtime.worker_senders,
                &targets,
                &sources,
                None,
                now_ms,
                |reference| published_request_settlement(state.inner(), &mut apps, reference),
            )
        })
        .await;
        match result {
            Ok(Ok(0)) => {}
            Ok(Ok(projected)) => info!(projected, "IM runtime cancellations projected"),
            Ok(Err(error)) => warn!(
                error_code = error.code(),
                "IM runtime cancellation projection failed"
            ),
            Err(_) => warn!(
                error_code = "IM_RUNTIME_UNAVAILABLE",
                "IM runtime cancellation task failed"
            ),
        }
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
                if ends_pending_interventions(&event) {
                    let _ = runtime.settlement_signal.try_send(());
                }
                let canonical_event_id = event_canonical_id(&event).to_string();
                let Some(project_id) = event_project_id(&event) else {
                    return;
                };
                let project_id = project_id.to_owned();
                let targets = match actionable_projection_target_snapshot(&runtime.targets) {
                    Ok(Some(targets)) => targets,
                    Ok(None) => return,
                    Err(()) => {
                        warn!(
                            canonical_event_id,
                            error_code = "IM_RUNTIME_UNAVAILABLE",
                            "IM lifecycle projection could not read targets"
                        );
                        return;
                    }
                };
                let Some(state) = handle.try_state::<DesktopState>() else {
                    warn!(
                        canonical_event_id,
                        error_code = "IM_RUNTIME_UNAVAILABLE",
                        "IM lifecycle projection could not resolve desktop state"
                    );
                    return;
                };
                let (runtime_app, workspace_label) =
                    match resolve_command_app_with_workspace_label(&state, Some(&project_id)) {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            warn!(
                                canonical_event_id,
                                error_code = error.code,
                                "IM lifecycle projection could not resolve workspace"
                            );
                            return;
                        }
                    };
                let job = ImLifecycleProjectionJob {
                    app: runtime_app,
                    event,
                    targets,
                    now_ms: chrono::Utc::now().timestamp_millis(),
                    workspace_label,
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
        let target_count = self.configure_projection_targets(&settings.im_integrations)?;
        tracing::info!(target_count, "IM projection targets configured");
        let locale = match settings.desktop_language {
            Some(gold_band::config::DesktopLanguage::ZhCn) => ImLocale::ZhCn,
            _ => ImLocale::En,
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
        self.cancellation.cancel();
        self.wecom_scan_session.cancel(None);
        self.feishu_scan_session.cancel(None);
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
        }
        let tasks = std::mem::take(
            &mut *self
                .worker_tasks
                .lock()
                .expect("IM worker task lock poisoned"),
        );
        let deadline = tokio::time::Instant::now() + IM_WORKER_SHUTDOWN_TIMEOUT;
        for mut task in tasks {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() || tokio::time::timeout(remaining, &mut task).await.is_err() {
                task.abort();
                warn!("IM delivery worker exceeded the shutdown deadline");
            }
        }
    }

    async fn start_wecom_scan(
        &self,
        session_id: String,
    ) -> Result<ImScanAuthorizationVm, WeComScanAuthError> {
        if Uuid::parse_str(&session_id).is_err() {
            return Err(WeComScanAuthError::SessionInvalid);
        }
        let cancellation = self.wecom_scan_session.open(&session_id);
        let authorization = match self.wecom_scan_client.generate(&cancellation).await {
            Ok(authorization) => authorization,
            Err(error) => {
                self.wecom_scan_session.cancel(Some(&session_id));
                return Err(error);
            }
        };
        self.wecom_scan_session
            .attach(&session_id, &cancellation, authorization.scode)?;
        Ok(ImScanAuthorizationVm {
            session_id,
            qr_url: authorization.auth_url,
            expires_at_ms: chrono::Utc::now().timestamp_millis()
                + i64::try_from(WECOM_SCAN_SESSION_TTL.as_millis()).unwrap_or(300_000),
        })
    }

    async fn complete_wecom_scan(
        &self,
        session_id: &str,
    ) -> Result<WeComScanCredentials, WeComScanAuthError> {
        let (scode, cancellation) = self.wecom_scan_session.claim(session_id)?;
        let result = self.wecom_scan_client.poll(&scode, &cancellation).await;
        if result.is_err() {
            self.wecom_scan_session.cancel(Some(session_id));
        }
        result
    }

    /// `existing_app_id` re-authorizes that app; `None` creates a new one.
    async fn start_feishu_scan(
        &self,
        session_id: String,
        existing_app_id: Option<&str>,
    ) -> Result<ImScanAuthorizationVm, FeishuScanAuthError> {
        if Uuid::parse_str(&session_id).is_err() {
            return Err(FeishuScanAuthError::SessionInvalid);
        }
        let cancellation = self.feishu_scan_session.open(&session_id);
        let authorization = match self
            .feishu_scan_client
            .begin(existing_app_id, &cancellation)
            .await
        {
            Ok(authorization) => authorization,
            Err(error) => {
                self.feishu_scan_session.cancel(Some(&session_id));
                return Err(error);
            }
        };
        let qr_url = authorization.qr_url.clone();
        let expires_in = authorization.expires_in;
        self.feishu_scan_session
            .attach(&session_id, &cancellation, authorization)?;
        Ok(ImScanAuthorizationVm {
            session_id,
            qr_url,
            expires_at_ms: chrono::Utc::now().timestamp_millis()
                + i64::try_from(expires_in.as_millis()).unwrap_or(i64::MAX / 2),
        })
    }

    /// Polls until the user finishes scanning, then self-checks the new
    /// credentials under the same cancellation before anything is persisted.
    async fn complete_feishu_scan(
        &self,
        session_id: &str,
    ) -> Result<FeishuScanCredentials, FeishuScanAuthError> {
        let (authorization, cancellation) = self.feishu_scan_session.claim(session_id)?;
        let result = async {
            let credentials = self
                .feishu_scan_client
                .poll(&authorization, &cancellation)
                .await?;
            tokio::select! {
                () = cancellation.cancelled() => Err(FeishuScanAuthError::Cancelled),
                checked = self.feishu_scan_client.self_check(&credentials) => {
                    checked.map(|()| credentials)
                }
            }
        }
        .await;
        if result.is_err() {
            self.feishu_scan_session.cancel(Some(session_id));
        }
        result
    }

    fn commit_scan<T>(
        &self,
        kind: ImChannelKind,
        session_id: &str,
        commit: impl FnOnce() -> CommandResult<T>,
    ) -> CommandResult<T> {
        // Acquire the settings lock first so cancellation remains available
        // while another settings writer is busy. Then fence commit with the
        // session slot, using the same lock as open/cancel.
        let _write_guard = self
            .settings_write_lock
            .lock()
            .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
        match kind {
            ImChannelKind::WeCom => self
                .wecom_scan_session
                .commit(session_id, commit)
                .map_err(|error| wecom_scan_error(error.into()))?,
            ImChannelKind::Feishu => self
                .feishu_scan_session
                .commit(session_id, commit)
                .map_err(|error| feishu_scan_error(error.into()))?,
        }
    }

    fn cancel_scan(&self, kind: ImChannelKind, session_id: &str) {
        match kind {
            ImChannelKind::WeCom => self.wecom_scan_session.cancel(Some(session_id)),
            ImChannelKind::Feishu => self.feishu_scan_session.cancel(Some(session_id)),
        }
    }

    /// Broadcasts the terminal state of an intervention after the surface named
    /// by `resolved_by` got `Accepted`. `source_delivery_id` is the clicked
    /// delivery for a channel resolution and `None` for the desktop.
    pub fn project_intervention_resolution(
        &self,
        resolution: &InterventionResolution<'_>,
    ) -> std::result::Result<ImProjectionResult, ImRepositoryError> {
        let targets = self
            .targets
            .read()
            .map_err(|_| ImRepositoryError::LockUnavailable)?
            .clone();
        project_intervention_resolution(
            &self.repository,
            &self.worker_senders,
            &targets,
            resolution,
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
            ImConnectorEvent::ConnectionFailed {
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
                ImConnectorEvent::ReconnectScheduled { generation, error } => warn!(
                    channel = channel.as_str(),
                    generation,
                    error_code = error.code.as_str(),
                    retryable = error.retryable,
                    platform_code = ?error.platform_code,
                    "IM channel reconnect scheduled"
                ),
                ImConnectorEvent::ConnectionFailed { generation, error } => warn!(
                    channel = channel.as_str(),
                    generation,
                    error_code = error.code.as_str(),
                    retryable = error.retryable,
                    platform_code = ?error.platform_code,
                    "IM channel connection failed"
                ),
                _ => {}
            }
            match event {
                ImConnectorEvent::InboundAction {
                    generation,
                    envelope,
                    response_context,
                } => {
                    if let Some(connector) = self.connectors.get(&channel).cloned() {
                        let process_runtime = Arc::clone(&self);
                        let project_runtime = Arc::clone(&self);
                        let handle = app_handle.clone();
                        tauri::async_runtime::spawn(run_inbound_action(
                            connector,
                            channel,
                            generation,
                            response_context,
                            move || process_runtime.process_inbound(&handle, envelope),
                            move |accepted| {
                                project_runtime.project_channel_resolution(channel, accepted)
                            },
                        ));
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
        envelope: ImInboundEnvelope,
    ) -> ProcessedInboundAction {
        let total_started = Instant::now();
        let mut timing = InboundTiming::default();
        let delivery_started = Instant::now();
        let delivery = match self.repository.delivery(&envelope.delivery_id) {
            Ok(Some(delivery)) => delivery,
            Ok(None) => {
                timing.delivery_lookup_ms = InboundTiming::elapsed_ms(delivery_started);
                let processed = ProcessedInboundAction {
                    delivery: None,
                    handling: Err(ImErrorCode::DeliveryNotFound),
                    settled_resolution: None,
                };
                self.log_inbound_timing(
                    &envelope,
                    Some(ImErrorCode::DeliveryNotFound),
                    timing,
                    total_started,
                );
                return processed;
            }
            Err(_) => {
                timing.delivery_lookup_ms = InboundTiming::elapsed_ms(delivery_started);
                let processed = ProcessedInboundAction {
                    delivery: None,
                    handling: Err(ImErrorCode::StorageUnavailable),
                    settled_resolution: None,
                };
                self.log_inbound_timing(
                    &envelope,
                    Some(ImErrorCode::StorageUnavailable),
                    timing,
                    total_started,
                );
                return processed;
            }
        };
        timing.delivery_lookup_ms = InboundTiming::elapsed_ms(delivery_started);
        let handling = self.process_inbound_delivery(app_handle, &envelope, &delivery, &mut timing);
        let (handling, settled_resolution) = if handling
            .as_ref()
            .is_ok_and(|handling| handling.result.outcome == ImInboundOutcome::AlreadyApplied)
        {
            self.settled_inbound_result(app_handle, envelope.channel, &delivery, handling)
        } else {
            (handling, None)
        };
        let error_code = handling.as_ref().err().copied();
        self.log_inbound_timing(&envelope, error_code, timing, total_started);
        ProcessedInboundAction {
            delivery: Some(delivery),
            handling,
            settled_resolution,
        }
    }

    /// Result of a click on a request that was no longer open. A stored
    /// resolution answers it directly; otherwise the canonical settlement
    /// decides: a cancellation is projected now, an answer in flight renders as
    /// handled, and a still-pending request means this card's snapshot is
    /// stale and can never succeed, so it is reported as expired.
    fn settled_inbound_result(
        &self,
        app_handle: &AppHandle,
        channel: ImChannelKind,
        delivery: &ImDelivery,
        handling: std::result::Result<ImInboundHandling, ImErrorCode>,
    ) -> (
        std::result::Result<ImInboundHandling, ImErrorCode>,
        Option<SettledResult>,
    ) {
        match stored_intervention_resolution(&self.repository, delivery) {
            Ok(Some(resolution)) => return (handling, Some(resolution.into())),
            Ok(None) => {}
            Err(_) => {
                warn!(
                    channel = channel.as_str(),
                    error_code = ImErrorCode::StorageUnavailable.as_str(),
                    "IM settled result lookup failed"
                );
                return (handling, None);
            }
        }
        let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
            return (handling, None);
        };
        let state = app_handle.state::<DesktopState>();
        match published_request_settlement(state.inner(), &mut HashMap::new(), reference) {
            Some(InterventionSettlement::Pending) => (Err(ImErrorCode::ActionExpired), None),
            Some(InterventionSettlement::Cancelled) => {
                let targets = self
                    .targets
                    .read()
                    .map(|targets| targets.clone())
                    .unwrap_or_default();
                if let Err(error) = project_runtime_cancellations(
                    &self.repository,
                    &self.worker_senders,
                    &targets,
                    std::slice::from_ref(delivery),
                    Some(&delivery.delivery_id),
                    chrono::Utc::now().timestamp_millis(),
                    |_| Some(InterventionSettlement::Cancelled),
                ) {
                    warn!(
                        channel = channel.as_str(),
                        error_code = error.code(),
                        "IM runtime cancellation projection failed"
                    );
                }
                // The callback replaces this card itself, even where the
                // channel's own record was skipped as `CallbackOnly`.
                let cancelled = SettledResult {
                    resolved_by: ImResolvedBy::Runtime,
                    outcome: ImResolutionOutcome::Cancelled,
                };
                (handling, Some(cancelled))
            }
            Some(InterventionSettlement::Answered) | None => (handling, None),
        }
    }

    fn process_inbound_delivery(
        &self,
        app_handle: &AppHandle,
        envelope: &ImInboundEnvelope,
        delivery: &ImDelivery,
        timing: &mut InboundTiming,
    ) -> std::result::Result<ImInboundHandling, ImErrorCode> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
            let inbound_started = Instant::now();
            return self
                .inbound_service
                .handle_with_delivery(envelope, delivery, now_ms, |_| {
                    unreachable!("information payload never reaches Runtime")
                })
                .inspect(|_| {
                    timing.inbound_service_ms = InboundTiming::elapsed_ms(inbound_started);
                })
                .map_err(|error| {
                    timing.inbound_service_ms = InboundTiming::elapsed_ms(inbound_started);
                    error.code()
                });
        };
        let state = app_handle.state::<DesktopState>();
        let workspace_started = Instant::now();
        let app = resolve_command_app_with_emitters(
            app_handle,
            &state,
            Some(&reference.locator.project_id),
        )
        .inspect(|_| {
            timing.workspace_resolve_ms = InboundTiming::elapsed_ms(workspace_started);
        })
        .map_err(|_| {
            timing.workspace_resolve_ms = InboundTiming::elapsed_ms(workspace_started);
            ImErrorCode::WorkspaceUnavailable
        })?;
        let binding_authorized = {
            let settings_started = Instant::now();
            let _settings_guard = self
                .settings_write_lock
                .lock()
                .inspect_err(|_| {
                    timing.settings_read_ms = InboundTiming::elapsed_ms(settings_started);
                })
                .map_err(|_| ImErrorCode::StorageUnavailable)?;
            let settings = app
                .load_settings()
                .inspect_err(|_| {
                    timing.settings_read_ms = InboundTiming::elapsed_ms(settings_started);
                })
                .map_err(|_| ImErrorCode::StorageUnavailable)?;
            let authorized =
                current_binding_authorizes_delivery(&settings.im_integrations, delivery);
            timing.settings_read_ms = InboundTiming::elapsed_ms(settings_started);
            authorized
        };
        if !binding_authorized {
            return Err(ImErrorCode::BindingRevoked);
        }
        let inbound_started = Instant::now();
        self.inbound_service
            .handle_with_delivery(envelope, delivery, now_ms, |command| {
                let command_started = Instant::now();
                let result = tauri::async_runtime::block_on(execute_intervention_command(
                    app_handle.clone(),
                    state.inner(),
                    app,
                    Some(reference.locator.project_id.clone()),
                    command,
                ));
                timing.domain_command_ms = InboundTiming::elapsed_ms(command_started);
                result
            })
            .inspect(|_| {
                timing.inbound_service_ms = InboundTiming::elapsed_ms(inbound_started);
            })
            .map_err(|error| {
                timing.inbound_service_ms = InboundTiming::elapsed_ms(inbound_started);
                error.code()
            })
    }

    fn log_inbound_timing(
        &self,
        envelope: &ImInboundEnvelope,
        error_code: Option<ImErrorCode>,
        timing: InboundTiming,
        total_started: Instant,
    ) {
        let sequence = self.inbound_timing_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        info!(
            channel = envelope.channel.as_str(),
            sequence,
            cold = sequence == 1,
            delivery_lookup_ms = timing.delivery_lookup_ms,
            workspace_resolve_ms = timing.workspace_resolve_ms,
            settings_read_ms = timing.settings_read_ms,
            inbound_service_ms = timing.inbound_service_ms,
            domain_command_ms = timing.domain_command_ms,
            total_ms = InboundTiming::elapsed_ms(total_started),
            error_code = error_code.map(|code| code.as_str()),
            "IM inbound action timing"
        );
    }

    fn project_channel_resolution(&self, channel: ImChannelKind, accepted: AcceptedInboundAction) {
        let Some(source) = self
            .repository
            .delivery(&accepted.source_delivery_id)
            .ok()
            .flatten()
        else {
            warn!(
                channel = channel.as_str(),
                error_code = ImErrorCode::DeliveryNotFound.as_str(),
                "IM intervention resolution projection skipped"
            );
            return;
        };
        let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
            return;
        };
        let result = self.project_intervention_resolution(&InterventionResolution {
            source_event_id: &accepted.source_event_id,
            notification_kind: source.notification_kind,
            locator: &reference.locator,
            resolved_by: ImResolvedBy::Channel(channel),
            source_delivery_id: Some(&accepted.source_delivery_id),
            outcome: accepted.outcome,
        });
        if let Err(error) = result {
            warn!(
                channel = channel.as_str(),
                error_code = error.code(),
                "IM intervention resolution projection failed"
            );
        }
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

/// Inputs of one resolution broadcast; built by the surface that got `Accepted`.
pub struct InterventionResolution<'a> {
    pub source_event_id: &'a str,
    pub notification_kind: ImNotificationKind,
    pub locator: &'a InterventionLocator,
    pub resolved_by: ImResolvedBy,
    pub source_delivery_id: Option<&'a str>,
    pub outcome: ImResolutionOutcome,
}

/// Writes one resolution record per eligible target (§9.2): targets that showed
/// the card get a `Send` record, others a `TombstoneOnly` record that blocks a
/// late source projection. A `CallbackOnly` channel whose clicked card the
/// callback already replaced gets a `TombstoneOnly` record too: it keeps the
/// winner for later clicks and closes the source without a second card.
fn project_intervention_resolution(
    repository: &ImRepository,
    worker_senders: &HashMap<ImChannelKind, mpsc::Sender<ImWorkerSignal>>,
    targets: &[ImProjectionTarget],
    resolution: &InterventionResolution<'_>,
    now_ms: i64,
) -> std::result::Result<ImProjectionResult, ImRepositoryError> {
    let sources =
        repository.intervention_deliveries_for_canonical_event(resolution.source_event_id)?;
    let mut result = ImProjectionResult::default();
    for target in targets {
        if !eligible_resolution_target(target, resolution.notification_kind) {
            continue;
        }
        let destination = target
            .destination
            .clone()
            .expect("eligible IM target has a destination");
        let source = sources.iter().find(|source| {
            source.channel == target.channel
                && source.destination.destination_id == destination.destination_id
                && source.notification_kind == resolution.notification_kind
        });
        let replaced_in_callback = source.is_some_and(|source| {
            resolution.source_delivery_id == Some(source.delivery_id.as_str())
                && target.capabilities.message_update == ImMessageUpdateMode::CallbackOnly
        });
        let (dispatch, display_ref) = match source {
            Some(source) if !replaced_in_callback => {
                (ImResolutionDispatch::Send, source.display_ref)
            }
            Some(source) => (ImResolutionDispatch::TombstoneOnly, source.display_ref),
            None => (ImResolutionDispatch::TombstoneOnly, None),
        };
        let delivery =
            resolution_delivery(target.channel, destination, resolution, display_ref, now_ms);
        match repository.enqueue_resolution_and_expire_pending_source(
            resolution.source_event_id,
            &delivery,
            dispatch,
            now_ms,
        )? {
            ImDeliveryInsertResult::Inserted => {
                result.inserted += 1;
                if dispatch == ImResolutionDispatch::Send
                    && let Some(sender) = worker_senders.get(&target.channel)
                {
                    let _ = sender.try_send(ImWorkerSignal::DeliveryAvailable);
                }
            }
            ImDeliveryInsertResult::Duplicate => result.duplicates += 1,
        }
    }
    Ok(result)
}

/// Projects `Cancelled` for each source whose request the runtime ended without
/// an answer (§9.2). Every runtime stop path — run stop, session close, Direct
/// background stop, abandoned runs — converges on the canonical settlement, so
/// this is the single place that finalizes their cards. Answered requests are
/// left to the surface whose command was `Accepted` (one winner per event);
/// pending or unknown requests are untouched, which keeps passes idempotent.
/// `clicked_delivery_id` marks a source whose card the current callback
/// replaces, so a `CallbackOnly` channel is not sent a second card.
fn project_runtime_cancellations(
    repository: &ImRepository,
    worker_senders: &HashMap<ImChannelKind, mpsc::Sender<ImWorkerSignal>>,
    targets: &[ImProjectionTarget],
    sources: &[ImDelivery],
    clicked_delivery_id: Option<&str>,
    now_ms: i64,
    mut settle: impl FnMut(&InterventionRef) -> Option<InterventionSettlement>,
) -> std::result::Result<usize, ImRepositoryError> {
    let mut settled_events = std::collections::BTreeSet::new();
    let mut projected = 0;
    for source in sources {
        let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
            continue;
        };
        if !settled_events.insert(source.canonical_event_id.as_str()) {
            continue;
        }
        if settle(reference) != Some(InterventionSettlement::Cancelled) {
            continue;
        }
        let result = project_intervention_resolution(
            repository,
            worker_senders,
            targets,
            &InterventionResolution {
                source_event_id: &source.canonical_event_id,
                notification_kind: source.notification_kind,
                locator: &reference.locator,
                resolved_by: ImResolvedBy::Runtime,
                source_delivery_id: clicked_delivery_id
                    .filter(|clicked| *clicked == source.delivery_id),
                outcome: ImResolutionOutcome::Cancelled,
            },
            now_ms,
        )?;
        projected += result.inserted;
    }
    Ok(projected)
}

/// Canonical settlement of a published request, read in the request's own
/// workspace. `apps` caches the workspace per project for one pass.
fn published_request_settlement(
    state: &DesktopState,
    apps: &mut HashMap<String, Option<App>>,
    reference: &InterventionRef,
) -> Option<InterventionSettlement> {
    let project_id = &reference.locator.project_id;
    let app = apps
        .entry(project_id.clone())
        .or_insert_with(|| resolve_command_app(state, Some(project_id)).ok())
        .as_ref()?;
    InterventionCommandService::new(app)
        .settlement(&reference.locator, &reference.request)
        .ok()
}

fn resolution_delivery(
    channel: ImChannelKind,
    destination: ImDestination,
    resolution: &InterventionResolution<'_>,
    display_ref: Option<u16>,
    now_ms: i64,
) -> ImDelivery {
    let InterventionResolution {
        source_event_id,
        notification_kind,
        locator,
        ..
    } = *resolution;
    let canonical_event_id = resolution_event_id(source_event_id);
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
                resolved_by: resolution.resolved_by,
                outcome: resolution.outcome.clone(),
            },
        },
        expires_at_ms: now_ms.saturating_add(TERMINAL_CONFIRMATION_TTL_MS),
        display_ref,
    }
}

fn eligible_resolution_target(
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
}

/// Runs one inbound action in its own task (§8): domain processing on the
/// blocking pool, the card response, then the resolution broadcast when the
/// command was `Accepted`. The dispatch loop never awaits it.
async fn run_inbound_action(
    connector: Arc<dyn ImConnector>,
    channel: ImChannelKind,
    generation: u64,
    response_context: ImActionResponseContext,
    process: impl FnOnce() -> ProcessedInboundAction + Send + 'static,
    project_resolution: impl FnOnce(AcceptedInboundAction) + Send + 'static,
) {
    let processed = tauri::async_runtime::spawn_blocking(process).await;
    let (response, accepted) = inbound_action_response(channel, generation, processed);
    if let Err(error) = connector
        .respond_to_action(response_context, response)
        .await
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
    if let Some(accepted) = accepted {
        let _ = tauri::async_runtime::spawn_blocking(move || project_resolution(accepted)).await;
    }
}

fn stored_intervention_resolution(
    repository: &ImRepository,
    source: &ImDelivery,
) -> Result<Option<InterventionResolutionNotification>, gold_band::im::ImRepositoryError> {
    let id = ImDelivery::deterministic_id(
        source.channel,
        &source.destination.destination_id,
        source.notification_kind,
        &resolution_event_id(&source.canonical_event_id),
    );
    Ok(repository
        .delivery(&id)?
        .and_then(|delivery| match delivery.payload {
            ImDeliveryPayload::InterventionResolution { resolution, .. } => Some(resolution),
            _ => None,
        }))
}

fn inbound_action_response<E: std::fmt::Display>(
    channel: ImChannelKind,
    generation: u64,
    processed: std::result::Result<ProcessedInboundAction, E>,
) -> (ImActionResponse, Option<AcceptedInboundAction>) {
    let processed = match processed {
        Ok(processed) => processed,
        Err(error) => {
            warn!(
                channel = channel.as_str(),
                generation,
                error_code = ImErrorCode::ActionProcessingFailed.as_str(),
                join_error = %error,
                "IM inbound action task failed"
            );
            return (
                rejected_response(ImErrorCode::ActionProcessingFailed, None),
                None,
            );
        }
    };
    let delivery = processed.delivery;
    let handling = match processed.handling {
        Ok(handling) => handling,
        Err(code) => {
            warn!(
                channel = channel.as_str(),
                generation,
                error_code = code.as_str(),
                "IM inbound action failed"
            );
            return (rejected_response(code, delivery), None);
        }
    };
    match (handling.result.outcome, handling.resolution, delivery) {
        (ImInboundOutcome::Accepted, Some(outcome), Some(delivery)) => {
            let accepted = AcceptedInboundAction {
                source_event_id: delivery.canonical_event_id.clone(),
                source_delivery_id: delivery.delivery_id.clone(),
                outcome: outcome.clone(),
            };
            let response = ImActionResponse {
                state: ImActionResponseState::Handled,
                resolved_by: Some(ImResolvedBy::Channel(channel)),
                outcome: Some(outcome),
                delivery: Some(delivery),
            };
            (response, Some(accepted))
        }
        (ImInboundOutcome::Rejected(code), _, delivery) => {
            warn!(
                channel = channel.as_str(),
                generation,
                error_code = code.as_str(),
                "IM inbound action rejected"
            );
            (rejected_response(code, delivery), None)
        }
        (_, _, delivery) => (
            ImActionResponse {
                state: ImActionResponseState::AlreadyHandled,
                resolved_by: processed
                    .settled_resolution
                    .as_ref()
                    .map(|resolution| resolution.resolved_by),
                outcome: processed
                    .settled_resolution
                    .map(|resolution| resolution.outcome),
                delivery,
            },
            None,
        ),
    }
}

fn rejected_response(code: ImErrorCode, delivery: Option<ImDelivery>) -> ImActionResponse {
    ImActionResponse {
        state: ImActionResponseState::Rejected(code),
        resolved_by: None,
        outcome: None,
        delivery,
    }
}

pub async fn initialize_im_runtime_if_required(app_handle: &AppHandle) -> Result<()> {
    let _ = ensure_im_runtime(app_handle, false).await?;
    Ok(())
}

async fn require_im_runtime(app_handle: &AppHandle) -> CommandResult<Arc<DesktopImRuntime>> {
    ensure_im_runtime(app_handle, true)
        .await
        .map_err(im_command_error)?
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))
}

async fn ensure_im_runtime(
    app_handle: &AppHandle,
    explicit_activation: bool,
) -> Result<Option<Arc<DesktopImRuntime>>> {
    if let Some(runtime) = app_handle.state::<DesktopState>().im_runtime() {
        return Ok(Some(runtime));
    }
    let handle = app_handle.clone();
    let (runtime, _) = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<DesktopState>();
        state.initialize_im_runtime(|| {
            let app = state.app()?;
            let settings = app.load_settings()?;
            let core_db_path = app.paths.core_db_path();
            if !explicit_activation
                && !im_settings_require_runtime(&settings.im_integrations)
                && !ImRepository::has_pending_channel_cleanups_at(&core_db_path)?
            {
                tracing::info!("IM runtime remains inactive because no durable work is configured");
                return Ok(None);
            }
            let runtime = DesktopImRuntime::new(core_db_path)?;
            let target_count = runtime.configure_projection_targets(&settings.im_integrations)?;
            tracing::info!(target_count, "initial IM projection targets configured");
            runtime.start(handle.clone());
            runtime.register_lifecycle_subscriber(&app, &handle);
            Ok(Some(runtime))
        })
    })
    .await
    .map_err(|_| anyhow::anyhow!("IM runtime initialization task failed"))??;
    Ok(runtime)
}

/// Runs an IM command's settings, SQLite or keyring work on the blocking pool
/// so the async runtime never waits on file or database I/O.
async fn run_blocking_command<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| im_error("IM_RUNTIME_UNAVAILABLE"))?
}

fn im_settings_require_runtime(settings: &ImIntegrationSettings) -> bool {
    settings
        .channels
        .iter()
        .any(|channel| channel.credential_ref.is_some() || channel.binding.is_some())
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
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ScanSessionInput,
) -> CommandResult<ImScanAuthorizationVm> {
    let _ = state;
    let runtime = require_im_runtime(&app_handle).await?;
    runtime
        .start_wecom_scan(input.session_id)
        .await
        .map_err(wecom_scan_error)
}

#[tauri::command]
pub async fn complete_wecom_scan_authorization(
    app_handle: AppHandle,
    _state: tauri::State<'_, DesktopState>,
    input: ScanSessionInput,
) -> CommandResult<ImSettingsVm> {
    let runtime = require_im_runtime(&app_handle).await?;
    let credentials = runtime
        .complete_wecom_scan(&input.session_id)
        .await
        .map_err(wecom_scan_error)?;
    install_scanned_credentials(
        &app_handle,
        &runtime,
        &input.session_id,
        ImChannelKind::WeCom,
        credentials.bot_id,
        BTreeMap::from([("secret".to_owned(), credentials.secret)]),
    )
    .await
}

#[tauri::command]
pub fn cancel_wecom_scan_authorization(
    state: tauri::State<'_, DesktopState>,
    input: ScanSessionInput,
) -> CommandResult<()> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    runtime.wecom_scan_session.cancel(Some(&input.session_id));
    Ok(())
}

#[tauri::command]
pub async fn start_feishu_scan_authorization(
    app_handle: AppHandle,
    _state: tauri::State<'_, DesktopState>,
    input: FeishuScanStartInput,
) -> CommandResult<ImScanAuthorizationVm> {
    let runtime = require_im_runtime(&app_handle).await?;
    let existing_app_id = if input.reauthorize {
        let handle = app_handle.clone();
        let app_id = run_blocking_command(move || {
            let settings = handle
                .state::<DesktopState>()
                .app()
                .and_then(|app| app.load_settings())
                .map_err(im_command_error)?;
            settings
                .im_integrations
                .channels
                .into_iter()
                .find(|channel| channel.kind == ImChannelKind::Feishu)
                .map(|channel| channel.public_identity)
                .filter(|app_id| !app_id.is_empty())
                .ok_or_else(|| feishu_scan_error(FeishuScanAuthError::ConfigInvalid))
        })
        .await?;
        Some(app_id)
    } else {
        None
    };
    runtime
        .start_feishu_scan(input.session_id, existing_app_id.as_deref())
        .await
        .map_err(feishu_scan_error)
}

#[tauri::command]
pub async fn complete_feishu_scan_authorization(
    app_handle: AppHandle,
    _state: tauri::State<'_, DesktopState>,
    input: ScanSessionInput,
) -> CommandResult<ImSettingsVm> {
    let runtime = require_im_runtime(&app_handle).await?;
    let credentials = runtime
        .complete_feishu_scan(&input.session_id)
        .await
        .map_err(feishu_scan_error)?;
    let (app_id, fields) = credentials.into_credential_fields();
    install_scanned_credentials(
        &app_handle,
        &runtime,
        &input.session_id,
        ImChannelKind::Feishu,
        app_id,
        fields,
    )
    .await
}

#[tauri::command]
pub fn cancel_feishu_scan_authorization(
    state: tauri::State<'_, DesktopState>,
    input: ScanSessionInput,
) -> CommandResult<()> {
    let runtime = state
        .im_runtime()
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    runtime.feishu_scan_session.cancel(Some(&input.session_id));
    Ok(())
}

#[tauri::command]
pub async fn set_im_channel_enabled(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: SetImChannelEnabledInput,
) -> CommandResult<ImSettingsVm> {
    let _ = state;
    let runtime = require_im_runtime(&app_handle).await?;
    let writer = Arc::clone(&runtime);
    let handle = app_handle.clone();
    let (settings, changed) = run_blocking_command(move || {
        let state = handle.state::<DesktopState>();
        let app = state.app().map_err(im_command_error)?;
        let _write_guard = writer
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
        Ok((settings, changed))
    })
    .await?;
    if changed {
        runtime
            .reconfigure(&app_handle)
            .await
            .map_err(im_command_error)?;
    }
    Ok(settings_vm(&settings.im_integrations, Some(&runtime)))
}

#[tauri::command]
pub async fn save_im_notification_preferences(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: SaveImNotificationPreferencesInput,
) -> CommandResult<ImSettingsVm> {
    let _ = state;
    let runtime = require_im_runtime(&app_handle).await?;
    let writer = Arc::clone(&runtime);
    let handle = app_handle.clone();
    let settings = run_blocking_command(move || {
        let state = handle.state::<DesktopState>();
        let app = state.app().map_err(im_command_error)?;
        let _write_guard = writer
            .settings_write_lock
            .lock()
            .map_err(|_| im_error("IM_STORAGE_UNAVAILABLE"))?;
        let mut settings = app.load_settings().map_err(im_command_error)?;
        if apply_notification_update(&mut settings.im_integrations, &input)? {
            app.save_settings(&settings).map_err(im_command_error)?;
            state
                .update_settings_config(&settings)
                .map_err(im_command_error)?;
            *writer
                .targets
                .write()
                .map_err(|_| im_error("IM_RUNTIME_UNAVAILABLE"))? =
                projection_targets(&settings.im_integrations, &writer.connectors);
        }
        Ok(settings)
    })
    .await?;
    Ok(settings_vm(&settings.im_integrations, Some(&runtime)))
}

#[tauri::command]
pub async fn reset_im_channel_binding(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ImGenerationInput,
) -> CommandResult<ImSettingsVm> {
    let _ = state;
    let runtime = require_im_runtime(&app_handle).await?;
    let writer = Arc::clone(&runtime);
    let handle = app_handle.clone();
    let (settings, changed) = run_blocking_command(move || {
        let state = handle.state::<DesktopState>();
        let app = state.app().map_err(im_command_error)?;
        let _write_guard = writer
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
            return Ok((settings, false));
        }
        let current_generation = writer
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
        *writer
            .targets
            .write()
            .map_err(|_| im_error("IM_RUNTIME_UNAVAILABLE"))? =
            projection_targets(&settings.im_integrations, &writer.connectors);
        Ok((settings, true))
    })
    .await?;
    if changed {
        runtime
            .reconfigure(&app_handle)
            .await
            .map_err(im_command_error)?;
    }
    Ok(settings_vm(&settings.im_integrations, Some(&runtime)))
}

#[tauri::command]
pub async fn reconnect_im_channel(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ImGenerationInput,
) -> CommandResult<gold_band::im::ImChannelSnapshot> {
    let runtime = require_im_runtime(&app_handle).await?;
    let current = runtime
        .connection_manager
        .snapshot(input.kind)
        .ok_or_else(|| im_error("IM_RUNTIME_UNAVAILABLE"))?;
    if current.generation != input.expected_generation {
        return Err(im_error("IM_STALE_GENERATION"));
    }
    let _ = state;
    let handle = app_handle.clone();
    let kind = input.kind;
    run_blocking_command(move || {
        let app = handle
            .state::<DesktopState>()
            .app()
            .map_err(im_command_error)?;
        let settings = app.load_settings().map_err(im_command_error)?;
        let channel = settings
            .im_integrations
            .channels
            .iter()
            .find(|channel| channel.kind == kind)
            .ok_or_else(|| im_error("IM_CREDENTIAL_REQUIRED"))?;
        if !channel.enabled || channel.credential_ref.is_none() {
            return Err(im_error("IM_CREDENTIAL_REQUIRED"));
        }
        Ok(())
    })
    .await?;
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
    _state: tauri::State<'_, DesktopState>,
    input: ImChannelInput,
) -> CommandResult<DeleteImChannelResultVm> {
    let runtime = require_im_runtime(&app_handle).await?;
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

fn feishu_scan_error(error: FeishuScanAuthError) -> CommandErrorVm {
    CommandErrorVm::new(error.code(), serde_json::json!({}))
}

/// Stages a new credential reference and rolls it back if configuration commit
/// fails. The caller runs this blocking work outside the async executor.
fn persist_scanned_credentials<T>(
    store: &dyn ImCredentialStore,
    kind: ImChannelKind,
    fields: BTreeMap<String, String>,
    commit: impl FnOnce(String) -> CommandResult<T>,
) -> CommandResult<T> {
    let reference = OsImCredentialStore::new_reference();
    let payload = gold_band::im::ImCredentialPayload::new(fields);
    store
        .save(kind, &reference, &payload)
        .map_err(|error| im_error(error.code()))?;
    let result = commit(reference.clone());
    if result.is_err()
        && let Err(error) = store.delete(kind, &reference)
    {
        warn!(
            channel = kind.as_str(),
            code = error.code(),
            "rollback scanned IM credential failed"
        );
    }
    result
}

/// Persists scanned credentials and reconfigures the runtime after commit.
async fn install_scanned_credentials(
    app_handle: &AppHandle,
    runtime: &Arc<DesktopImRuntime>,
    session_id: &str,
    kind: ImChannelKind,
    public_identity: String,
    fields: BTreeMap<String, String>,
) -> CommandResult<ImSettingsVm> {
    let handle = app_handle.clone();
    let runtime_for_install = Arc::clone(runtime);
    let session_for_install = session_id.to_owned();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let runtime = runtime_for_install;
        let state = handle.state::<DesktopState>();
        let app = state.app().map_err(im_command_error)?;
        let (settings, old_reference) =
            persist_scanned_credentials(&OsImCredentialStore, kind, fields, |reference| {
                runtime.commit_scan(kind, &session_for_install, || {
                    if runtime
                        .repository
                        .channel_cleanup(kind)
                        .map_err(|error| im_error(error.code()))?
                        .is_some()
                    {
                        return Err(im_error("IM_CHANNEL_CLEANUP_PENDING"));
                    }
                    let mut settings = app.load_settings().map_err(im_command_error)?;
                    let original_settings = settings.clone();
                    let existing = settings
                        .im_integrations
                        .channels
                        .iter()
                        .find(|channel| channel.kind == kind)
                        .cloned();
                    let old_reference = existing
                        .as_ref()
                        .and_then(|channel| channel.credential_ref.clone());
                    upsert_channel(
                        &mut settings.im_integrations,
                        scanned_channel_settings(
                            kind,
                            public_identity,
                            reference,
                            existing.as_ref(),
                        ),
                    );
                    app.save_settings(&settings).map_err(im_command_error)?;
                    if let Err(error) = state.update_settings_config(&settings) {
                        let _ = app.save_settings(&original_settings);
                        let _ = state.update_settings_config(&original_settings);
                        return Err(im_command_error(error));
                    }
                    Ok((settings, old_reference))
                })
            })?;
        if let Some(old_reference) = old_reference
            && OsImCredentialStore.delete(kind, &old_reference).is_err()
        {
            warn!(
                channel = kind.as_str(),
                "cleanup previous IM credential reference failed"
            );
        }
        Ok::<_, CommandErrorVm>(settings)
    })
    .await
    .map_err(|_| im_error("IM_CREDENTIAL_UNAVAILABLE"));
    // Also retire this session if staging or loading configuration failed.
    // A replacement session is deliberately unaffected.
    runtime.cancel_scan(kind, session_id);
    let settings = result??;
    runtime
        .reconfigure(app_handle)
        .await
        .map_err(im_command_error)?;
    Ok(settings_vm(&settings.im_integrations, Some(runtime)))
}

fn scanned_channel_settings(
    kind: ImChannelKind,
    public_identity: String,
    credential_ref: String,
    existing: Option<&ImChannelSettings>,
) -> ImChannelSettings {
    ImChannelSettings {
        kind,
        enabled: true,
        public_identity,
        credential_ref: Some(credential_ref),
        // A scanned authorization establishes a new receiver trust boundary,
        // even when it reuses the same bot/application identity.
        binding: None,
        notifications: existing
            .map(|channel| channel.notifications.clone())
            .unwrap_or_default(),
    }
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

fn actionable_projection_target_snapshot(
    targets: &RwLock<Vec<ImProjectionTarget>>,
) -> std::result::Result<Option<Vec<ImProjectionTarget>>, ()> {
    let targets = targets.read().map_err(|_| ())?;
    let actionable = targets.iter().any(|target| {
        target.enabled
            && target.credential_available
            && target.capabilities.proactive_delivery
            && target.capabilities.private_chat
            && target.destination.is_some()
            && ImNotificationKind::ALL.into_iter().any(|kind| {
                target.notifications.enabled(kind)
                    && (!kind.is_intervention() || target.capabilities.card_actions)
            })
    });
    Ok(actionable.then(|| targets.clone()))
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
        .map(|error| ImConnectorEvent::ConnectionFailed { generation, error })
}

fn current_binding_authorizes_delivery(
    settings: &ImIntegrationSettings,
    delivery: &ImDelivery,
) -> bool {
    settings.channels.iter().any(|channel| {
        channel.kind == delivery.channel
            && channel.binding.as_ref().is_some_and(|binding| {
                binding.destination_id == delivery.destination.destination_id
                    && binding.conversation_id == delivery.destination.conversation_id
                    && binding.authorized_actor_id == delivery.destination.authorized_actor_id
            })
    })
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

/// Boundaries after which a runtime may have ended requests without an answer.
fn ends_pending_interventions(event: &RuntimeLifecycleEvent) -> bool {
    matches!(
        event,
        RuntimeLifecycleEvent::RunCompleted { .. }
            | RuntimeLifecycleEvent::NodeCompleted { .. }
            | RuntimeLifecycleEvent::AcpTurnFinished { .. }
            | RuntimeLifecycleEvent::DirectTurnLifecycle(_)
    )
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
    fn runtime_activation_requires_durable_im_work() {
        assert!(!im_settings_require_runtime(
            &ImIntegrationSettings::default()
        ));
        assert!(!im_settings_require_runtime(&ImIntegrationSettings {
            channels: vec![disabled_channel(ImChannelKind::WeCom)],
        }));
        let mut enabled_without_credential = disabled_channel(ImChannelKind::WeCom);
        enabled_without_credential.enabled = true;
        assert!(!im_settings_require_runtime(&ImIntegrationSettings {
            channels: vec![enabled_without_credential],
        }));
        let mut configured = disabled_channel(ImChannelKind::WeCom);
        configured.credential_ref = Some("credential-ref".into());
        assert!(im_settings_require_runtime(&ImIntegrationSettings {
            channels: vec![configured],
        }));
    }

    #[tokio::test]
    async fn shutdown_does_not_wait_for_a_full_delivery_signal_queue() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = DesktopImRuntime::with_signing_key(
            camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
            vec![7; 32],
        )
        .unwrap();
        let sender = runtime.worker_senders.get(&ImChannelKind::WeCom).unwrap();
        while sender.try_send(ImWorkerSignal::DeliveryAvailable).is_ok() {}

        tokio::time::timeout(std::time::Duration::from_millis(200), runtime.shutdown())
            .await
            .expect("shutdown control must not share a full delivery queue");
    }

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
        let targets = RwLock::new(targets);
        assert!(
            actionable_projection_target_snapshot(&targets)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn scan_install_commits_once_and_rolls_back_staged_credentials_on_failure() {
        for kind in ImChannelKind::ALL {
            for fail_commit in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let runtime = DesktopImRuntime::with_signing_key(
                    camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
                    vec![7; 32],
                )
                .unwrap();
                let token = open_claimed_scan(&runtime, kind, "current");
                let store = ScanTestStore {
                    references: Mutex::new(vec!["existing-credential".into()]),
                    on_save: Box::new(|| {
                        assert!(
                            !token.is_cancelled(),
                            "the scan must remain active throughout credential staging"
                        )
                    }),
                };
                let mut configured_reference = "existing-credential".to_owned();
                let result =
                    persist_scanned_credentials(&store, kind, BTreeMap::new(), |reference| {
                        runtime.commit_scan(kind, "current", || {
                            if fail_commit {
                                return Err(im_error("IM_STORAGE_UNAVAILABLE"));
                            }
                            configured_reference = reference;
                            Ok(())
                        })
                    });
                assert!(
                    token.is_cancelled(),
                    "commit must retire the session on success and failure"
                );
                assert_eq!(
                    runtime
                        .commit_scan(kind, "current", || Ok(()))
                        .unwrap_err()
                        .code,
                    "IM_SCAN_CANCELLED"
                );
                if fail_commit {
                    assert_eq!(result.unwrap_err().code, "IM_STORAGE_UNAVAILABLE");
                    assert_eq!(configured_reference, "existing-credential");
                    assert_eq!(
                        *store.references.lock().unwrap(),
                        vec!["existing-credential"]
                    );
                } else {
                    result.unwrap();
                    assert_ne!(configured_reference, "existing-credential");
                    assert!(
                        store
                            .references
                            .lock()
                            .unwrap()
                            .contains(&configured_reference)
                    );
                }
            }
        }
    }

    #[test]
    fn scan_install_rejects_cancellation_or_replacement_during_credential_save() {
        for kind in ImChannelKind::ALL {
            for replacement in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let runtime = DesktopImRuntime::with_signing_key(
                    camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
                    vec![7; 32],
                )
                .unwrap();
                open_claimed_scan(&runtime, kind, "old");
                let store = ScanTestStore {
                    references: Mutex::new(vec!["existing-credential".into()]),
                    on_save: Box::new(|| {
                        if replacement {
                            open_claimed_scan(&runtime, kind, "new");
                        } else {
                            match kind {
                                ImChannelKind::WeCom => {
                                    runtime.wecom_scan_session.cancel(Some("old"))
                                }
                                ImChannelKind::Feishu => {
                                    runtime.feishu_scan_session.cancel(Some("old"))
                                }
                            }
                        }
                    }),
                };
                let mut configured_reference = "existing-credential".to_owned();
                let result =
                    persist_scanned_credentials(&store, kind, BTreeMap::new(), |reference| {
                        runtime.commit_scan(kind, "old", || {
                            configured_reference = reference;
                            Ok(())
                        })
                    });
                assert_eq!(
                    configured_reference, "existing-credential",
                    "cancelled or replaced scan must not overwrite channel settings"
                );
                assert_eq!(result.unwrap_err().code, "IM_SCAN_CANCELLED");
                assert_eq!(
                    *store.references.lock().unwrap(),
                    vec!["existing-credential"]
                );
                if replacement {
                    assert!(
                        runtime.commit_scan(kind, "new", || Ok(())).is_ok(),
                        "finishing the old request must preserve the replacement session"
                    );
                }
            }
        }
    }

    fn open_claimed_scan(
        runtime: &DesktopImRuntime,
        kind: ImChannelKind,
        id: &str,
    ) -> CancellationToken {
        match kind {
            ImChannelKind::WeCom => {
                let token = runtime.wecom_scan_session.open(id);
                runtime
                    .wecom_scan_session
                    .attach(id, &token, "ticket".into())
                    .unwrap();
                runtime.wecom_scan_session.claim(id).unwrap().1
            }
            ImChannelKind::Feishu => {
                let token = runtime.feishu_scan_session.open(id);
                runtime
                    .feishu_scan_session
                    .attach(
                        id,
                        &token,
                        FeishuScanAuthorization {
                            device_code: "ticket".into(),
                            qr_url: "https://example.test/scan".into(),
                            interval: std::time::Duration::from_secs(1),
                            expires_in: std::time::Duration::from_secs(300),
                        },
                    )
                    .unwrap();
                runtime.feishu_scan_session.claim(id).unwrap().1
            }
        }
    }

    struct ScanTestStore<'a> {
        references: Mutex<Vec<String>>,
        on_save: Box<dyn Fn() + Send + Sync + 'a>,
    }

    impl ImCredentialStore for ScanTestStore<'_> {
        fn save(
            &self,
            _kind: ImChannelKind,
            reference: &str,
            _payload: &gold_band::im::ImCredentialPayload,
        ) -> Result<(), gold_band::im::ImCredentialError> {
            self.references.lock().unwrap().push(reference.to_owned());
            (self.on_save)();
            Ok(())
        }

        fn load(
            &self,
            _kind: ImChannelKind,
            _reference: &str,
        ) -> Result<gold_band::im::ImCredentialPayload, gold_band::im::ImCredentialError> {
            Err(gold_band::im::ImCredentialError::NotFound)
        }

        fn delete(
            &self,
            _kind: ImChannelKind,
            reference: &str,
        ) -> Result<(), gold_band::im::ImCredentialError> {
            self.references
                .lock()
                .unwrap()
                .retain(|stored| stored != reference);
            Ok(())
        }
    }

    #[test]
    fn scanned_credentials_require_fresh_binding_even_for_the_same_app() {
        for kind in ImChannelKind::ALL {
            let mut existing = disabled_channel(kind);
            existing.public_identity = "same-app".into();
            existing.credential_ref = Some("old-reference".into());
            existing.binding = Some(ImBindingSummary {
                destination_id: "old-owner".into(),
                conversation_id: "old-chat".into(),
                authorized_actor_id: "old-owner".into(),
                display_name: "Old owner".into(),
            });
            existing.notifications.manual_check = false;
            let installed = scanned_channel_settings(
                kind,
                "same-app".into(),
                "new-reference".into(),
                Some(&existing),
            );
            assert!(
                installed.binding.is_none(),
                "a new authorization must not inherit the previous receiver"
            );
            assert!(installed.enabled);
            assert_eq!(installed.notifications, existing.notifications);
            assert_eq!(installed.credential_ref.as_deref(), Some("new-reference"));
            assert!(existing.binding.is_some());
        }
    }

    #[test]
    fn startup_configures_projection_targets_before_lifecycle_subscription() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = DesktopImRuntime::with_signing_key(
            camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
            vec![7; 32],
        )
        .unwrap();
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

        assert!(runtime.targets.read().unwrap().is_empty());
        assert_eq!(runtime.configure_projection_targets(&settings).unwrap(), 1);
        let targets = runtime.targets.read().unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].destination.as_ref().unwrap().authorized_actor_id,
            "user-1"
        );
    }

    #[test]
    fn current_binding_must_match_the_complete_historical_delivery_destination() {
        let delivery = vote_delivery();
        let matching = ImBindingSummary {
            destination_id: "user-1".into(),
            conversation_id: "user-1".into(),
            authorized_actor_id: "user-1".into(),
            display_name: "User".into(),
        };
        let settings = |binding| ImIntegrationSettings {
            channels: vec![ImChannelSettings {
                kind: ImChannelKind::WeCom,
                enabled: true,
                public_identity: "bot-id".into(),
                credential_ref: Some("credential-ref".into()),
                binding,
                notifications: Default::default(),
            }],
        };

        assert!(current_binding_authorizes_delivery(
            &settings(Some(matching.clone())),
            &delivery,
        ));
        assert!(!current_binding_authorizes_delivery(
            &settings(None),
            &delivery,
        ));
        for mismatched in [
            ImBindingSummary {
                destination_id: "user-2".into(),
                ..matching.clone()
            },
            ImBindingSummary {
                conversation_id: "chat-2".into(),
                ..matching.clone()
            },
            ImBindingSummary {
                authorized_actor_id: "user-2".into(),
                ..matching.clone()
            },
        ] {
            assert!(!current_binding_authorizes_delivery(
                &settings(Some(mismatched)),
                &delivery,
            ));
        }
        assert_eq!(
            gold_band::im::ImInboundError::BindingRevoked.code(),
            ImErrorCode::BindingRevoked
        );
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
        assert_eq!(
            view.channels
                .iter()
                .map(|channel| channel.kind)
                .collect::<Vec<_>>(),
            ImChannelKind::ALL
        );
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
        let serialized = serde_json::to_string(&ImScanAuthorizationVm {
            session_id: Uuid::new_v4().to_string(),
            qr_url: "https://work.weixin.qq.com/ai/qc/c?s=short-lived".into(),
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
        let ImConnectorEvent::ConnectionFailed { generation, error } = event else {
            panic!("expected terminal connection failure event")
        };
        assert_eq!(generation, 7);
        assert_eq!(error.code.as_str(), "IM_AUTHENTICATION_REQUIRED");
        assert!(!error.retryable);
        assert!(terminal_connector_event(8, Ok(())).is_none());
    }

    fn wecom_target(destination: &ImDestination) -> ImProjectionTarget {
        ImProjectionTarget {
            channel: ImChannelKind::WeCom,
            enabled: true,
            credential_available: true,
            capabilities: WeComConnector::new(ImActionTokenCodec::new(vec![7; 32]).unwrap())
                .capabilities(),
            destination: Some(destination.clone()),
            notifications: Default::default(),
        }
    }

    fn resolution_repository() -> (tempfile::TempDir, ImRepository) {
        let temp = tempfile::tempdir().unwrap();
        let repository = ImRepository::new(
            camino::Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        );
        (temp, repository)
    }

    fn source_locator(source: &ImDelivery) -> InterventionLocator {
        let ImDeliveryPayload::Intervention { reference, .. } = &source.payload else {
            unreachable!("fixture is an intervention")
        };
        reference.locator.clone()
    }

    fn resolution_record(repository: &ImRepository, source: &ImDelivery) -> ImDelivery {
        let terminal_id = ImDelivery::deterministic_id(
            source.channel,
            &source.destination.destination_id,
            source.notification_kind,
            &resolution_event_id(&source.canonical_event_id),
        );
        repository.delivery(&terminal_id).unwrap().unwrap()
    }

    #[test]
    fn desktop_resolution_projects_all_intervention_kinds_and_wakes_the_worker() {
        let (_temp, repository) = resolution_repository();
        let (sender, mut receiver) = mpsc::channel(8);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);

        for (index, (kind, outcome)) in [
            (
                ImNotificationKind::Permission,
                ImResolutionOutcome::PermissionOption {
                    option_id: "allow".into(),
                },
            ),
            (
                ImNotificationKind::Elicitation,
                ImResolutionOutcome::ElicitationDeclined,
            ),
            (
                ImNotificationKind::ManualCheck,
                ImResolutionOutcome::ManualCheck {
                    outcome: gold_band::domain::NodeOutcome::Success,
                },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let source_event_id = format!("event-{index}");
            let mut source = vote_delivery();
            source.notification_kind = kind;
            source.canonical_event_id = source_event_id.clone();
            source.refresh_id();
            let locator = source_locator(&source);
            repository.enqueue(&source, 100 + index as i64).unwrap();
            let expected_display_ref = repository
                .intervention_deliveries_for_canonical_event(&source_event_id)
                .unwrap()[0]
                .display_ref;
            let targets = [wecom_target(&source.destination)];
            let resolution = InterventionResolution {
                source_event_id: &source_event_id,
                notification_kind: kind,
                locator: &locator,
                resolved_by: ImResolvedBy::Desktop,
                source_delivery_id: None,
                outcome: outcome.clone(),
            };

            let result = project_intervention_resolution(
                &repository,
                &worker_senders,
                &targets,
                &resolution,
                200 + index as i64,
            )
            .unwrap();
            assert_eq!(result.inserted, 1);
            assert_eq!(
                receiver.try_recv().unwrap(),
                ImWorkerSignal::DeliveryAvailable
            );

            let terminal = resolution_record(&repository, &source);
            let ImDeliveryPayload::InterventionResolution {
                resolution: payload,
                ..
            } = terminal.payload
            else {
                panic!("expected resolution payload");
            };
            assert_eq!(payload.notification_kind, kind);
            assert_eq!(payload.display_ref, expected_display_ref);
            assert_eq!(payload.resolved_by, ImResolvedBy::Desktop);
            assert_eq!(payload.outcome, outcome);

            let replay = project_intervention_resolution(
                &repository,
                &worker_senders,
                &targets,
                &resolution,
                300 + index as i64,
            )
            .unwrap();
            assert_eq!(replay.duplicates, 1);
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn desktop_resolution_wins_when_it_is_persisted_before_the_source_projection() {
        let (_temp, repository) = resolution_repository();
        let (sender, mut receiver) = mpsc::channel(2);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);
        let mut source = vote_delivery();
        source.refresh_id();
        let locator = source_locator(&source);

        let result = project_intervention_resolution(
            &repository,
            &worker_senders,
            &[wecom_target(&source.destination)],
            &InterventionResolution {
                source_event_id: &source.canonical_event_id,
                notification_kind: source.notification_kind,
                locator: &locator,
                resolved_by: ImResolvedBy::Desktop,
                source_delivery_id: None,
                outcome: ImResolutionOutcome::PermissionOption {
                    option_id: "allow".into(),
                },
            },
            100,
        )
        .unwrap();
        assert_eq!(result.inserted, 1);
        assert!(
            receiver.try_recv().is_err(),
            "a tombstone must not wake the worker"
        );
        assert!(
            repository
                .claim_due(
                    ImChannelKind::WeCom,
                    101,
                    std::time::Duration::from_secs(30),
                    10
                )
                .unwrap()
                .is_empty(),
            "a tombstone is never sent"
        );
        let tombstone = resolution_record(&repository, &source);
        assert!(matches!(
            tombstone.payload,
            ImDeliveryPayload::InterventionResolution { .. }
        ));

        assert_eq!(
            repository.enqueue(&source, 102).unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
        assert!(
            repository
                .intervention_deliveries_for_canonical_event(&source.canonical_event_id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn callback_only_source_clicked_on_its_own_channel_is_not_projected_again() {
        let (_temp, repository) = resolution_repository();
        let (sender, mut receiver) = mpsc::channel(2);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);
        let mut source = vote_delivery();
        source.refresh_id();
        repository.enqueue(&source, 100).unwrap();
        let locator = source_locator(&source);

        let result = project_intervention_resolution(
            &repository,
            &worker_senders,
            &[wecom_target(&source.destination)],
            &InterventionResolution {
                source_event_id: &source.canonical_event_id,
                notification_kind: source.notification_kind,
                locator: &locator,
                resolved_by: ImResolvedBy::Channel(ImChannelKind::WeCom),
                source_delivery_id: Some(&source.delivery_id),
                outcome: ImResolutionOutcome::PermissionOption {
                    option_id: "allow".into(),
                },
            },
            101,
        )
        .unwrap();
        assert_eq!(result.inserted, 1, "the winner is kept as a tombstone");
        assert!(receiver.try_recv().is_err(), "no second card is sent");
        assert_eq!(
            stored_intervention_resolution(&repository, &source)
                .unwrap()
                .map(|stored| stored.resolved_by),
            Some(ImResolvedBy::Channel(ImChannelKind::WeCom))
        );
        assert!(
            repository
                .open_intervention_sources(102)
                .unwrap()
                .is_empty(),
            "the replaced card is closed"
        );
    }

    fn source_for_request(event_id: &str, request_id: &str) -> ImDelivery {
        let mut source = vote_delivery();
        source.canonical_event_id = event_id.into();
        if let ImDeliveryPayload::Intervention { reference, .. } = &mut source.payload {
            reference.request =
                gold_band::app::intervention::InterventionRequestIdentity::Permission {
                    request_id: request_id.into(),
                };
        }
        source.refresh_id();
        source
    }

    fn settle_by_request(reference: &InterventionRef) -> Option<InterventionSettlement> {
        let gold_band::app::intervention::InterventionRequestIdentity::Permission { request_id } =
            &reference.request
        else {
            return None;
        };
        match request_id.as_str() {
            "cancelled" => Some(InterventionSettlement::Cancelled),
            "answered" => Some(InterventionSettlement::Answered),
            "pending" => Some(InterventionSettlement::Pending),
            _ => None,
        }
    }

    #[test]
    fn runtime_cancellation_finalizes_only_cancelled_open_sources_once() {
        let (_temp, repository) = resolution_repository();
        let (sender, mut receiver) = mpsc::channel(8);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);
        let cancelled = source_for_request("event-cancelled", "cancelled");
        let answered = source_for_request("event-answered", "answered");
        let pending = source_for_request("event-pending", "pending");
        let unknown = source_for_request("event-unknown", "unknown");
        for source in [&cancelled, &answered, &pending, &unknown] {
            repository.enqueue(source, 100).unwrap();
        }
        let targets = [wecom_target(&cancelled.destination)];

        let sources = repository.open_intervention_sources(200).unwrap();
        assert_eq!(sources.len(), 4);
        let projected = project_runtime_cancellations(
            &repository,
            &worker_senders,
            &targets,
            &sources,
            None,
            200,
            settle_by_request,
        )
        .unwrap();

        assert_eq!(projected, 1);
        assert_eq!(
            receiver.try_recv().unwrap(),
            ImWorkerSignal::DeliveryAvailable
        );
        let ImDeliveryPayload::InterventionResolution { resolution, .. } =
            resolution_record(&repository, &cancelled).payload
        else {
            panic!("expected resolution payload");
        };
        assert_eq!(resolution.resolved_by, ImResolvedBy::Runtime);
        assert_eq!(resolution.outcome, ImResolutionOutcome::Cancelled);
        for open in [&answered, &pending, &unknown] {
            assert!(
                stored_intervention_resolution(&repository, open)
                    .unwrap()
                    .is_none(),
                "only the surface that won writes an answered result"
            );
        }

        let remaining = repository.open_intervention_sources(300).unwrap();
        assert_eq!(remaining.len(), 3, "a resolved source is no longer open");
        let replay = project_runtime_cancellations(
            &repository,
            &worker_senders,
            &targets,
            &sources,
            None,
            300,
            settle_by_request,
        )
        .unwrap();
        assert_eq!(replay, 0, "a second pass is idempotent");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn runtime_cancellation_for_a_clicked_callback_only_card_is_not_sent_twice() {
        let (_temp, repository) = resolution_repository();
        let (sender, mut receiver) = mpsc::channel(2);
        let worker_senders = HashMap::from([(ImChannelKind::WeCom, sender)]);
        let source = source_for_request("event-cancelled", "cancelled");
        repository.enqueue(&source, 100).unwrap();

        let projected = project_runtime_cancellations(
            &repository,
            &worker_senders,
            &[wecom_target(&source.destination)],
            std::slice::from_ref(&source),
            Some(&source.delivery_id),
            200,
            settle_by_request,
        )
        .unwrap();

        assert_eq!(projected, 1, "the cancellation is kept as a tombstone");
        assert!(
            receiver.try_recv().is_err(),
            "the callback replaces the clicked card itself"
        );
        assert!(
            repository
                .open_intervention_sources(300)
                .unwrap()
                .is_empty(),
            "a later periodic pass cannot send a second card"
        );
    }

    #[test]
    fn three_surface_resolution_matrix_preserves_winner_and_dispatch_intent() {
        for winner in [
            ImResolvedBy::Desktop,
            ImResolvedBy::Channel(ImChannelKind::WeCom),
            ImResolvedBy::Channel(ImChannelKind::Feishu),
        ] {
            let (_temp, repository) = resolution_repository();
            let mut sources = Vec::new();
            let mut targets = Vec::new();
            let mut senders = HashMap::new();
            let mut receivers = Vec::new();
            for channel in [ImChannelKind::WeCom, ImChannelKind::Feishu] {
                let mut source = vote_delivery();
                source.channel = channel;
                source.refresh_id();
                repository.enqueue(&source, 100).unwrap();
                let mut target = wecom_target(&source.destination);
                target.channel = channel;
                target.capabilities.message_update = match channel {
                    ImChannelKind::WeCom => ImMessageUpdateMode::CallbackOnly,
                    ImChannelKind::Feishu => ImMessageUpdateMode::Anytime,
                };
                let (sender, receiver) = mpsc::channel(2);
                senders.insert(channel, sender);
                receivers.push(receiver);
                sources.push(source);
                targets.push(target);
            }
            let clicked = sources
                .iter()
                .find(|s| winner == ImResolvedBy::Channel(s.channel));
            let locator = source_locator(&sources[0]);
            let outcome = ImResolutionOutcome::PermissionOption {
                option_id: "allow".into(),
            };
            let resolution = InterventionResolution {
                source_event_id: &sources[0].canonical_event_id,
                notification_kind: sources[0].notification_kind,
                locator: &locator,
                resolved_by: winner,
                source_delivery_id: clicked.map(|s| s.delivery_id.as_str()),
                outcome: outcome.clone(),
            };
            project_intervention_resolution(&repository, &senders, &targets, &resolution, 101)
                .unwrap();
            for (index, source) in sources.iter().enumerate() {
                let callback_only_winner =
                    index == 0 && winner == ImResolvedBy::Channel(ImChannelKind::WeCom);
                let stored = stored_intervention_resolution(&repository, source)
                    .unwrap()
                    .unwrap_or_else(|| panic!("{winner:?} -> {:?}", source.channel));
                assert_eq!(receivers[index].try_recv().is_ok(), !callback_only_winner);
                assert_eq!(stored.resolved_by, winner);
                assert_eq!(stored.outcome, outcome);
            }
            // Replaying the winner cannot send or mutate another terminal result.
            let replay =
                project_intervention_resolution(&repository, &senders, &targets, &resolution, 102)
                    .unwrap();
            assert_eq!(replay.inserted, 0);
            assert!(receivers.iter_mut().all(|r| r.try_recv().is_err()));
        }
    }

    fn inbound_result(outcome: ImInboundOutcome) -> gold_band::im::ImInboundActionResult {
        gold_band::im::ImInboundActionResult {
            action_id: "action-1".into(),
            channel: ImChannelKind::WeCom,
            canonical_event_id: Some("event-1".into()),
            actor_id_hash: "actor-hash".into(),
            platform_event_id: "platform-event-1".into(),
            outcome,
            result_revision: None,
            received_at_ms: 1,
            completed_at_ms: 2,
        }
    }

    fn processed(
        outcome: ImInboundOutcome,
        resolution: Option<ImResolutionOutcome>,
    ) -> std::result::Result<ProcessedInboundAction, String> {
        Ok(ProcessedInboundAction {
            settled_resolution: None,
            delivery: Some(vote_delivery()),
            handling: Ok(ImInboundHandling {
                result: inbound_result(outcome),
                resolution,
            }),
        })
    }

    #[test]
    fn late_submission_uses_the_persisted_winner_and_does_not_broadcast() {
        let (_temp, repository) = resolution_repository();
        let mut source = vote_delivery();
        source.channel = ImChannelKind::Feishu;
        source.notification_kind = ImNotificationKind::Elicitation;
        source.refresh_id();
        repository.enqueue(&source, 100).unwrap();
        let mut target = wecom_target(&source.destination);
        target.channel = source.channel;
        target.capabilities.message_update = gold_band::im::ImMessageUpdateMode::Anytime;
        let winner = ImResolutionOutcome::ElicitationSubmitted {
            content: serde_json::json!({"choice": "accepted"}),
        };
        let locator = source_locator(&source);
        project_intervention_resolution(
            &repository,
            &HashMap::new(),
            &[target],
            &InterventionResolution {
                source_event_id: &source.canonical_event_id,
                notification_kind: source.notification_kind,
                locator: &locator,
                resolved_by: ImResolvedBy::Desktop,
                source_delivery_id: None,
                outcome: winner.clone(),
            },
            101,
        )
        .unwrap();
        let settled_resolution = stored_intervention_resolution(&repository, &source)
            .unwrap()
            .map(Into::into);
        let losing = ProcessedInboundAction {
            delivery: Some(source),
            handling: Ok(ImInboundHandling {
                result: inbound_result(ImInboundOutcome::AlreadyApplied),
                resolution: Some(ImResolutionOutcome::ElicitationSubmitted {
                    content: serde_json::json!({"choice": "loser"}),
                }),
            }),
            settled_resolution,
        };
        let (response, accepted) =
            inbound_action_response::<String>(ImChannelKind::Feishu, 1, Ok(losing));
        assert_eq!(response.state, ImActionResponseState::AlreadyHandled);
        assert_eq!(response.outcome, Some(winner));
        assert_eq!(response.resolved_by, Some(ImResolvedBy::Desktop));
        assert!(accepted.is_none());
    }

    #[test]
    fn inbound_action_response_maps_every_processing_result_to_a_structured_state() {
        let allow = ImResolutionOutcome::PermissionOption {
            option_id: "allow".into(),
        };
        let channel = ImChannelKind::WeCom;

        let (response, accepted) = inbound_action_response(
            channel,
            1,
            processed(ImInboundOutcome::Accepted, Some(allow.clone())),
        );
        assert_eq!(response.state, ImActionResponseState::Handled);
        assert_eq!(response.resolved_by, Some(ImResolvedBy::Channel(channel)));
        assert_eq!(response.outcome, Some(allow.clone()));
        assert_eq!(response.delivery, Some(vote_delivery()));
        let accepted = accepted.expect("accepted command broadcasts its resolution");
        assert_eq!(accepted.source_event_id, "event-1");
        assert_eq!(accepted.source_delivery_id, "delivery-1");
        assert_eq!(accepted.outcome, allow);

        let (response, accepted) = inbound_action_response(
            channel,
            1,
            processed(ImInboundOutcome::AlreadyApplied, None),
        );
        assert_eq!(response.state, ImActionResponseState::AlreadyHandled);
        assert_eq!(response.resolved_by, None);
        assert!(accepted.is_none());

        let (response, accepted) = inbound_action_response(
            channel,
            1,
            processed(ImInboundOutcome::Rejected(ImErrorCode::ActionExpired), None),
        );
        assert_eq!(
            response.state,
            ImActionResponseState::Rejected(ImErrorCode::ActionExpired)
        );
        assert_eq!(response.delivery, Some(vote_delivery()));
        assert!(accepted.is_none());

        let (response, accepted) = inbound_action_response(
            channel,
            1,
            Ok::<_, String>(ProcessedInboundAction {
                settled_resolution: None,
                delivery: Some(vote_delivery()),
                handling: Err(ImErrorCode::BindingRevoked),
            }),
        );
        assert_eq!(
            response.state,
            ImActionResponseState::Rejected(ImErrorCode::BindingRevoked)
        );
        assert_eq!(response.delivery, Some(vote_delivery()));
        assert!(accepted.is_none());

        let (response, accepted) =
            inbound_action_response::<String>(channel, 1, Err("join failed".into()));
        assert_eq!(
            response.state,
            ImActionResponseState::Rejected(ImErrorCode::ActionProcessingFailed)
        );
        assert_eq!(response.delivery, None);
        assert!(accepted.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slow_inbound_action_does_not_block_other_actions() {
        let connector: Arc<dyn ImConnector> = Arc::new(WeComConnector::new(
            ImActionTokenCodec::new(vec![7; 32]).unwrap(),
        ));
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (slow_projected, slow_projection) = std::sync::mpsc::channel::<()>();
        let slow = tauri::async_runtime::spawn(run_inbound_action(
            Arc::clone(&connector),
            ImChannelKind::WeCom,
            1,
            ImActionResponseContext::new(()),
            move || {
                released.recv().unwrap();
                processed(
                    ImInboundOutcome::Accepted,
                    Some(ImResolutionOutcome::ElicitationDeclined),
                )
                .unwrap()
            },
            move |_| slow_projected.send(()).unwrap(),
        ));
        let (fast_projected, fast_projection) = std::sync::mpsc::channel::<()>();
        let fast = tauri::async_runtime::spawn(run_inbound_action(
            connector,
            ImChannelKind::WeCom,
            1,
            ImActionResponseContext::new(()),
            || {
                processed(
                    ImInboundOutcome::Accepted,
                    Some(ImResolutionOutcome::ElicitationDeclined),
                )
                .unwrap()
            },
            move |_| fast_projected.send(()).unwrap(),
        ));

        tokio::time::timeout(std::time::Duration::from_secs(5), fast)
            .await
            .expect("a fast action must finish while another action is still processing")
            .unwrap();
        fast_projection.try_recv().unwrap();
        assert!(slow_projection.try_recv().is_err());

        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), slow)
            .await
            .expect("the slow action finishes once released")
            .unwrap();
        slow_projection.try_recv().unwrap();
    }
}
