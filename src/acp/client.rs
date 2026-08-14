use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex, mpsc::RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, info};

#[derive(Debug, Clone)]
struct AcpTimelineStreamState {
    item_id: String,
    source_id: Option<String>,
    started_seq: u64,
    started_at: String,
    content: String,
    content_chars: usize,
}

#[derive(Debug, Clone)]
struct AcpPromptTurnIdentity {
    id: String,
    prompt_event_id: String,
    usage_transaction_id: String,
    usage_transaction_seq: u64,
    started_at: String,
    /// The durable user event for this logical prompt. Completed user events
    /// deliberately leave `timeline_items`, so terminal settlement must not
    /// depend on that runtime-only hot cache.
    event: AcpUiEvent,
}

fn is_pending_retry_prompt_event(event: &AcpUiEvent) -> bool {
    event.status.as_deref() == Some("processing")
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/retry/attempt"))
            .and_then(Value::as_u64)
            .is_some_and(|attempt| attempt > 0)
}

fn same_prompt_retry_lifecycle(
    prior: &AcpPromptRetryState,
    prompt_id: &str,
    hidden_from_chat: bool,
) -> bool {
    prior.prompt_id == prompt_id && prior.hidden_from_chat == hidden_from_chat
}

fn next_prompt_retry_attempt(
    prior: Option<&AcpPromptRetryState>,
    prompt_id: &str,
    hidden_from_chat: bool,
) -> u32 {
    prior
        .filter(|state| same_prompt_retry_lifecycle(state, prompt_id, hidden_from_chat))
        .map(|state| state.retry_attempt.saturating_add(1))
        .unwrap_or_default()
}

fn canonical_prompt_event_identity(
    prior: Option<&AcpPromptRetryState>,
    prompt_id: &str,
    hidden_from_chat: bool,
    operation_seq: u64,
    operation_timestamp: &str,
) -> (String, u64, String) {
    prior
        .filter(|state| same_prompt_retry_lifecycle(state, prompt_id, hidden_from_chat))
        .and_then(|state| {
            Some((
                state.prompt_event_id.clone()?,
                state.prompt_event_seq?,
                state.prompt_event_timestamp.clone()?,
            ))
        })
        .unwrap_or_else(|| {
            (
                format!("gold-band-user-prompt-{operation_seq}"),
                operation_seq,
                operation_timestamp.to_string(),
            )
        })
}

fn prompt_usage_transaction_id(
    prompt_event_id: &str,
    retry_attempt: u32,
    operation_seq: u64,
) -> String {
    format!("{prompt_event_id}:attempt-{retry_attempt}:{operation_seq}")
}

fn settle_prompt_event(
    mut event: AcpUiEvent,
    terminal_status: &str,
    ended_seq: u64,
    failure: Option<&AcpPromptFailure>,
) -> AcpUiEvent {
    event.status = Some(terminal_status.to_string());
    event.ended_seq = Some(ended_seq);
    event.ended_at = Some(current_timestamp());
    let raw = event.raw.get_or_insert_with(|| json!({}));
    match failure {
        Some(failure) => {
            raw["terminalFailure"] = json!({
                "code": failure.code,
                "message": failure.diagnostic(),
            });
        }
        None => raw["cancelled"] = json!(true),
    }
    event
}

#[derive(Debug, Clone)]
struct AcpContextCompactionState {
    item_id: String,
    started_seq: u64,
    started_at: String,
    context_used_before: Option<u64>,
    context_size: Option<u64>,
    completed_seq: Option<u64>,
    completed_at: Option<String>,
    saw_post_completion_reset: bool,
}

#[derive(Debug, Clone, Default)]
struct AcpContextUsageGauge {
    confirmed_used: Option<u64>,
    window_size: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct AcpUsageState {
    context: AcpContextUsageGauge,
    total_cost_usd: Option<f64>,
    latest_prompt: AcpPromptTokenUsage,
    attempt_totals: AcpAttemptTokenTotals,
    compaction: Option<AcpContextCompactionState>,
}

impl AcpUsageState {
    fn from_prior(
        prior: PriorAttemptMetrics,
        compaction: Option<AcpContextCompactionState>,
    ) -> Self {
        Self {
            context: AcpContextUsageGauge {
                confirmed_used: prior.used_tokens.filter(|used| *used > 0),
                window_size: prior.context_window_size.filter(|size| *size > 0),
            },
            total_cost_usd: prior.total_cost_usd,
            latest_prompt: AcpPromptTokenUsage {
                input_tokens: prior.input_tokens,
                output_tokens: prior.output_tokens,
                cached_read_tokens: prior.cached_read_tokens,
                cached_write_tokens: prior.cached_write_tokens,
                total_tokens: prior.total_tokens,
            },
            attempt_totals: AcpAttemptTokenTotals {
                input_tokens: prior.attempt_input_tokens,
                output_tokens: prior.attempt_output_tokens,
                cached_read_tokens: prior.attempt_cached_read_tokens,
                cached_write_tokens: prior.attempt_cached_write_tokens,
                total_tokens: prior.attempt_total_tokens,
            },
            compaction,
        }
    }

    /// Fold one provider sample into the canonical session usage state.
    ///
    /// `used=0` is an adapter transition sample, not a confirmed empty context.
    /// It is retained in `acp.raw.jsonl`, while canonical timeline/snapshot state
    /// keeps the last confirmed positive gauge until a post-compaction value arrives.
    fn observe_provider_usage(
        &mut self,
        used: Option<u64>,
        size: Option<u64>,
        cost: Option<f64>,
    ) -> Option<u64> {
        if let Some(size) = size.filter(|size| *size > 0) {
            self.context.window_size = Some(size);
        }
        if let Some(cost) = cost {
            self.total_cost_usd = Some(cost);
        }

        let Some(used) = used else {
            return None;
        };
        let Some(compaction) = self.compaction.as_mut() else {
            if used > 0 {
                self.context.confirmed_used = Some(used);
            }
            return None;
        };

        if compaction.completed_seq.is_none() {
            return None;
        }
        if used == 0 {
            compaction.saw_post_completion_reset = true;
            return None;
        }
        let confirmed_after = compaction.saw_post_completion_reset
            || compaction
                .context_used_before
                .is_some_and(|before| used < before);
        if !confirmed_after {
            return None;
        }

        self.context.confirmed_used = Some(used);
        Some(used)
    }

    fn record_prompt_usage(&mut self, prompt_usage: AcpPromptTokenUsage) {
        self.attempt_totals.accumulate_prompt(&prompt_usage);
        self.latest_prompt = prompt_usage;
    }

    fn apply_recovered_attempt_usage(&mut self, recovery: AcpAttemptUsageRecovery) {
        if recovery.completed_turns == 0 {
            return;
        }
        self.attempt_totals = recovery.totals;
        self.latest_prompt = recovery.latest_prompt;
    }

    fn normalize_timeline_usage(&self, update: &mut Value) {
        let Some(object) = update.as_object_mut() else {
            return;
        };
        match self.context.confirmed_used {
            Some(used) => {
                object.insert("used".to_string(), Value::from(used));
            }
            None => {
                object.remove("used");
            }
        }
        match self.context.window_size {
            Some(size) => {
                object.insert("size".to_string(), Value::from(size));
            }
            None => {
                object.remove("size");
            }
        }
    }
}

use crate::acp::branches::{
    ROOT_BRANCH_ID, agent_prompt_event, agent_result_event, annotate_event_branch,
    branch_route_for_event, branch_timeline_path, event_branch_id, load_all_branch_events,
    migrate_legacy_agent_timeline,
};
use crate::acp::commands::{AcpCommandItem, parse_available_commands};
use crate::acp::connection::{
    AcpConnectionUnavailable, AdapterConnection, AdapterConnectionKey, AdapterConnectionManager,
    SessionEventPump, SessionRouteTryRecvError, SessionRouteWatermark,
};
use crate::acp::elicitation::{
    ELICITATION_DEFAULT_TIMEOUT, ElicitationAction, PendingElicitationState,
    cancel_pending_elicitation_requests, elicitation_response_result,
    remove_elicitation_signal_files, upsert_elicitation_response_event,
    wait_for_elicitation_response_until_cancelled, write_pending_elicitation,
};
use crate::acp::events::{
    AcpAttemptPaths, AcpLatestTurnStatus, AcpPromptRetryState, AcpSessionAvailability,
    AcpSessionMetadata, AcpSessionTiming, AcpTimingState, AcpUiEvent, append_diagnostic,
    append_raw_frame, append_structured_diagnostic, cancel_latest_processing_prompt_retry,
    current_timestamp, latest_timeline_source_seq, load_session_metadata, normalize_session_update,
    permission_request_event, user_prompt_event_with_quotes, write_session_metadata,
};
use crate::acp::history::{ProviderHistoryImport, ProviderHistoryReplay, ReplayUpdateDecision};
use crate::acp::permission::{
    PermissionResponseState, acp_permission_response_result, cancel_pending_permission_requests,
    permission_response_file, remove_permission_signal_files,
    wait_for_permission_response_until_cancelled, write_pending_permission,
};
use crate::acp::timeline::{TimelineCompactionPolicy, TimelineStore};
use crate::acp::usage::{
    AcpAttemptTokenTotals, AcpAttemptUsageRecovery, AcpPromptTokenUsage, append_prompt_completed,
    append_prompt_started, repair_attempt_usage,
};
use crate::config::{AcpAdapterConfig, ManagedAgentId, RuntimeConfig};
use crate::domain::{SessionMode, TurnControlMode, TurnControlTransitionCause, VERSION};
use crate::provider::{
    ACP_MCP_TRANSPORT_UNSUPPORTED_CODE, PromptBundle, PromptVisibility, SkippedAcpMcpServer,
    gold_band_hidden_block, prepare_acp_mcp_servers,
};
use crate::runtime::{WorkerRefState, validate_worker_ref_state};
use crate::runtime_error::{
    DEFAULT_AUTO_RETRY_MAX_ATTEMPTS, RuntimeErrorDomain, blocked_runtime_error_info, runtime_error,
};
use crate::storage::{GoldBandPaths, ensure_parent_dir, read_json, roll_jsonl, write_json};

const STOP_CHECK_INTERVAL: Duration = Duration::from_millis(100);
const LIVE_STREAM_UPDATE_INTERVAL: Duration = Duration::from_millis(75);
const LIVE_TIMING_UPDATE_INTERVAL: Duration = Duration::from_secs(1);
const DOCTOR_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const DOCTOR_DIAGNOSTIC_MAX_SIZE: u64 = 512 * 1024;
const DOCTOR_DIAGNOSTIC_TARGET_SIZE: u64 = 384 * 1024;
const DOCTOR_COMMAND_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(500);
const SESSION_TITLE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PROMPT_CANCEL_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_FRESHNESS_TIMEOUT: Duration = Duration::from_secs(5);
const PROMPT_TERMINAL_QUIET_PERIOD: Duration = Duration::from_millis(200);
const SESSION_REPLAY_QUIET_PERIOD: Duration = Duration::from_millis(200);
const SESSION_REPLAY_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_LIST_MAX_PAGES: usize = 8;
const SESSION_EVICTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const SESSION_SYSTEM_CONTEXT_VERSION: u32 = 1;
const NESTED_AGENT_TRANSCRIPT_CAPABILITY: &str = "subagent-transcript";
pub const ACP_SESSION_RESTORE_UNSUPPORTED_CODE: &str = "acp.session-restore-unsupported";
pub const ACP_HISTORY_SYNC_UNSUPPORTED_CODE: &str = "acp.history-sync-unsupported";

#[derive(Debug)]
struct AcpCancelled;

#[derive(Debug)]
struct AcpCancelDrainTimeout {
    timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptCancellationOutcome {
    observed: bool,
    drain_timed_out: bool,
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": 1,
        "clientCapabilities": {
            "_meta": {
                (NESTED_AGENT_TRANSCRIPT_CAPABILITY): true
            },
            "elicitation": {
                "form": {}
            }
        },
        "clientInfo": {
            "name": "gold-band",
            "title": "Gold Band",
            "version": crate::domain::VERSION,
        }
    })
}

impl std::fmt::Display for AcpCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ACP prompt cancelled")
    }
}

impl std::error::Error for AcpCancelled {}

impl std::fmt::Display for AcpCancelDrainTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "ACP cancelled prompt did not drain within {} seconds",
            self.timeout.as_secs()
        )
    }
}

impl std::error::Error for AcpCancelDrainTimeout {}

fn prompt_cancellation_outcome(
    cancel_requested: bool,
    error: Option<&anyhow::Error>,
) -> PromptCancellationOutcome {
    let drain_timed_out =
        error.is_some_and(|error| error.downcast_ref::<AcpCancelDrainTimeout>().is_some());
    let observed = cancel_requested
        || drain_timed_out
        || error.is_some_and(|error| error.downcast_ref::<AcpCancelled>().is_some());
    PromptCancellationOutcome {
        observed,
        drain_timed_out,
    }
}

#[derive(Debug)]
struct AcpTransportInterrupted;

impl std::fmt::Display for AcpTransportInterrupted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ACP adapter transport interrupted")
    }
}

impl std::error::Error for AcpTransportInterrupted {}

#[derive(Debug)]
struct AcpSessionReplayDrainTimeout {
    timeout: Duration,
}

impl std::fmt::Display for AcpSessionReplayDrainTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "ACP session history replay did not become idle within {} seconds",
            self.timeout.as_secs()
        )
    }
}

impl std::error::Error for AcpSessionReplayDrainTimeout {}

#[derive(Debug)]
struct AcpPromptRouteDrainTimeout {
    timeout: Duration,
}

impl std::fmt::Display for AcpPromptRouteDrainTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "ACP prompt terminal route timed out after {} milliseconds",
            self.timeout.as_millis()
        )
    }
}

impl std::error::Error for AcpPromptRouteDrainTimeout {}

#[derive(Debug)]
struct AcpPromptRouteUnavailable {
    reason: &'static str,
}

impl std::fmt::Display for AcpPromptRouteUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "ACP prompt terminal route transport interrupted: {}",
            self.reason
        )
    }
}

impl std::error::Error for AcpPromptRouteUnavailable {}

fn is_transport_interruption(error: &anyhow::Error) -> bool {
    error.downcast_ref::<AcpTransportInterrupted>().is_some()
        || error.downcast_ref::<AcpConnectionUnavailable>().is_some()
}

fn drain_frames_until_quiet<Receive, Observe>(
    quiet_period: Duration,
    timeout: Duration,
    mut receive: Receive,
    mut observe: Observe,
) -> Result<usize>
where
    Receive: FnMut(Duration) -> std::result::Result<Value, RecvTimeoutError>,
    Observe: FnMut(Value) -> Result<()>,
{
    drain_frames_until_quiet_with_timeout_error(
        quiet_period,
        timeout,
        &mut receive,
        &mut observe,
        |timeout| anyhow!(AcpSessionReplayDrainTimeout { timeout }),
    )
}

fn drain_frames_until_quiet_with_timeout_error<Receive, Observe, TimeoutError>(
    quiet_period: Duration,
    timeout: Duration,
    mut receive: Receive,
    mut observe: Observe,
    timeout_error: TimeoutError,
) -> Result<usize>
where
    Receive: FnMut(Duration) -> std::result::Result<Value, RecvTimeoutError>,
    Observe: FnMut(Value) -> Result<()>,
    TimeoutError: Fn(Duration) -> anyhow::Error,
{
    let started_at = Instant::now();
    let mut drained_frames = 0usize;
    loop {
        let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
            return Err(timeout_error(timeout));
        };
        let wait_for = quiet_period.min(remaining);
        match receive(wait_for) {
            Ok(value) => {
                observe(value)?;
                drained_frames = drained_frames.saturating_add(1);
            }
            Err(RecvTimeoutError::Timeout) if wait_for == quiet_period => {
                return Ok(drained_frames);
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(timeout_error(timeout));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(AcpTransportInterrupted));
            }
        }
    }
}

fn drain_frames_until_route_watermark<Reached, Receive, Observe>(
    timeout: Duration,
    mut reached: Reached,
    mut receive: Receive,
    mut observe: Observe,
) -> Result<usize>
where
    Reached: FnMut() -> bool,
    Receive: FnMut(Duration) -> std::result::Result<Value, RecvTimeoutError>,
    Observe: FnMut(Value) -> Result<()>,
{
    let started_at = Instant::now();
    let mut drained_frames = 0usize;
    while !reached() {
        let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
            return Err(anyhow!(AcpPromptRouteDrainTimeout { timeout }));
        };
        match receive(remaining.min(STOP_CHECK_INTERVAL)) {
            Ok(value) => {
                observe(value)?;
                drained_frames = drained_frames.saturating_add(1);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(AcpTransportInterrupted));
            }
        }
    }
    Ok(drained_frames)
}

fn session_list_is_unsupported(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("method not found")
        || message.contains("unknown method")
        || message.contains("unsupported method")
        || message.contains("-32601")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderControlState {
    Starting,
    Accepted,
    Running,
    CancelRequested,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelNotificationPhase {
    BeforeProviderActive,
    AfterProviderActive,
}

#[derive(Debug, Clone, Copy)]
struct ProviderControlInner {
    state: ProviderControlState,
    provider_active: bool,
    cancel_sent_before_active: bool,
    cancel_sent_after_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptActivity {
    Starting,
    Accepted,
    Running,
    CancelRequested,
}

#[derive(Debug)]
struct ProviderControl {
    inner: Mutex<ProviderControlInner>,
}

impl ProviderControl {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ProviderControlInner {
                state: ProviderControlState::Starting,
                provider_active: false,
                cancel_sent_before_active: false,
                cancel_sent_after_active: false,
            }),
        }
    }

    fn state(&self) -> ProviderControlState {
        self.inner
            .lock()
            .map(|inner| inner.state)
            .unwrap_or(ProviderControlState::Stopped)
    }

    fn request_prompt_cancel(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        match inner.state {
            ProviderControlState::Starting
            | ProviderControlState::Accepted
            | ProviderControlState::Running => {
                inner.state = ProviderControlState::CancelRequested;
                true
            }
            ProviderControlState::CancelRequested | ProviderControlState::Stopped => false,
        }
    }

    fn claim_cancel_notification(&self) -> Option<CancelNotificationPhase> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        if inner.state != ProviderControlState::CancelRequested {
            return None;
        }
        if inner.provider_active {
            if inner.cancel_sent_after_active {
                None
            } else {
                inner.cancel_sent_after_active = true;
                Some(CancelNotificationPhase::AfterProviderActive)
            }
        } else if inner.cancel_sent_before_active {
            None
        } else {
            inner.cancel_sent_before_active = true;
            Some(CancelNotificationPhase::BeforeProviderActive)
        }
    }

    fn mark_provider_active(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.provider_active = true;
        }
    }

    fn mark_running(&self) {
        if let Ok(mut inner) = self.inner.lock()
            && matches!(
                inner.state,
                ProviderControlState::Starting | ProviderControlState::Accepted
            )
        {
            inner.state = ProviderControlState::Running;
        }
    }

    fn mark_accepted(&self) {
        if let Ok(mut inner) = self.inner.lock()
            && inner.state == ProviderControlState::Starting
        {
            inner.state = ProviderControlState::Accepted;
        }
    }

    fn mark_stopped(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.state = ProviderControlState::Stopped;
        }
    }
}

static PROVIDER_CONTROLS: LazyLock<Mutex<HashMap<String, Arc<ProviderControl>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn attempt_control_key(attempt_dir: &Utf8Path) -> String {
    attempt_dir.to_string()
}

pub fn request_prompt_cancel(attempt_dir: &Utf8Path) -> bool {
    let key = attempt_control_key(attempt_dir);
    PROVIDER_CONTROLS
        .lock()
        .ok()
        .and_then(|controls| controls.get(&key).cloned())
        .map(|control| control.request_prompt_cancel())
        .unwrap_or(false)
}

pub fn prompt_activity(attempt_dir: &Utf8Path) -> Option<PromptActivity> {
    let key = attempt_control_key(attempt_dir);
    let state = PROVIDER_CONTROLS
        .lock()
        .ok()
        .and_then(|controls| controls.get(&key).cloned())
        .map(|control| control.state())?;
    match state {
        ProviderControlState::Starting => Some(PromptActivity::Starting),
        ProviderControlState::Accepted => Some(PromptActivity::Accepted),
        ProviderControlState::Running => Some(PromptActivity::Running),
        ProviderControlState::CancelRequested => Some(PromptActivity::CancelRequested),
        ProviderControlState::Stopped => None,
    }
}

pub fn prompt_activity_under(root: &Utf8Path) -> Option<PromptActivity> {
    let controls = PROVIDER_CONTROLS.lock().ok()?;
    controls
        .iter()
        .filter(|(key, _)| Utf8Path::new(key).starts_with(root))
        .filter_map(|(_, control)| match control.state() {
            ProviderControlState::Starting => Some(PromptActivity::Starting),
            ProviderControlState::Accepted => Some(PromptActivity::Accepted),
            ProviderControlState::Running => Some(PromptActivity::Running),
            ProviderControlState::CancelRequested => Some(PromptActivity::CancelRequested),
            ProviderControlState::Stopped => None,
        })
        .max_by_key(|activity| match activity {
            PromptActivity::Starting => 0,
            PromptActivity::Accepted => 1,
            PromptActivity::Running => 2,
            PromptActivity::CancelRequested => 3,
        })
}

pub fn cancel_attempt_prompt(attempt_dir: &Utf8Path) -> Result<bool> {
    let key = attempt_control_key(attempt_dir);
    let control = PROVIDER_CONTROLS
        .lock()
        .ok()
        .and_then(|controls| controls.get(&key).cloned());
    if let Some(control) = control.as_ref() {
        control.request_prompt_cancel();
    }
    let manager = AdapterConnectionManager::shared();
    let cancel_result = if manager.attempt_session(attempt_dir).is_none() {
        Ok(control.is_some())
    } else if control
        .as_ref()
        .is_some_and(|control| control.claim_cancel_notification().is_none())
    {
        Ok(true)
    } else {
        manager.cancel_attempt_prompt(attempt_dir)
    };
    // Timeline settlement is bookkeeping: report its failure only after the
    // lifecycle latch and live ACP cancel have had a chance to be delivered.
    let interaction_result = cancel_pending_prompt_interactions(attempt_dir, current_timestamp());
    let cancelled = cancel_result?;
    interaction_result?;
    Ok(cancelled)
}

pub fn close_attempt_session_bounded(attempt_dir: &Utf8Path) -> Result<bool> {
    if AcpSessionRuntimeRegistry::shared().invalidate(attempt_dir) {
        return Ok(true);
    }
    AdapterConnectionManager::shared()
        .close_attempt_session_bounded(attempt_dir, SESSION_CLOSE_TIMEOUT)
}

pub fn close_workspace_connections_bounded(workspace_root: &Utf8Path) -> Result<()> {
    AdapterConnectionManager::shared()
        .close_workspace_connections_bounded(workspace_root, SESSION_CLOSE_TIMEOUT)
}

pub fn close_all_connections_bounded() -> Result<()> {
    AdapterConnectionManager::shared().close_all_connections_bounded(SESSION_CLOSE_TIMEOUT)
}

pub fn close_provider_connections_bounded(provider_id: &str) -> Result<()> {
    AcpSessionRuntimeRegistry::shared().detach_provider(provider_id);
    AdapterConnectionManager::shared()
        .close_provider_connections_bounded(provider_id, SESSION_CLOSE_TIMEOUT)
}

pub fn has_active_prompts_in_workspace(workspace_root: &Utf8Path) -> bool {
    AdapterConnectionManager::shared().has_active_prompts_in_workspace(workspace_root)
}

pub fn has_active_prompts_in_provider(provider_id: &str) -> bool {
    AdapterConnectionManager::shared().has_active_prompts_in_provider(provider_id)
}

fn register_provider_control(attempt_dir: &Utf8Path) -> Arc<ProviderControl> {
    let key = attempt_control_key(attempt_dir);
    let control = Arc::new(ProviderControl::new());
    if let Ok(mut controls) = PROVIDER_CONTROLS.lock() {
        controls.insert(key, control.clone());
    }
    control
}

fn unregister_provider_control(attempt_dir: &Utf8Path, control: &Arc<ProviderControl>) {
    control.mark_stopped();
    let key = attempt_control_key(attempt_dir);
    if let Ok(mut controls) = PROVIDER_CONTROLS.lock() {
        if controls
            .get(&key)
            .is_some_and(|existing| Arc::ptr_eq(existing, control))
        {
            controls.remove(&key);
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeStopProbe {
    pub run_file: Utf8PathBuf,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub attempt_state_file: Option<Utf8PathBuf>,
    pub turn_control_mode: TurnControlMode,
}

impl RuntimeStopProbe {
    fn is_stopped(&self) -> bool {
        if self.turn_control_mode == TurnControlMode::NonRuntimeControlled {
            return false;
        }
        self.attempt_state_file
            .as_ref()
            .is_some_and(|path| self.attempt_state_is_stopped(path))
            || self.run_state_is_stopped()
    }

    fn attempt_state_is_stopped(&self, path: &Utf8PathBuf) -> bool {
        read_json::<serde_json::Value>(path)
            .ok()
            .is_some_and(|attempt| {
                let manual_check_pending = attempt
                    .get("manualCheckPending")
                    .or_else(|| attempt.get("manual_check_pending"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if manual_check_pending {
                    return false;
                }
                let status = attempt
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                status.eq_ignore_ascii_case("paused")
                    && attempt
                        .get("outcome")
                        .is_none_or(|outcome| outcome.is_null())
            })
    }

    fn run_state_is_stopped(&self) -> bool {
        read_json::<serde_json::Value>(&self.run_file)
            .ok()
            .is_some_and(|run| {
                let status = run
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let pause_reason = run
                    .get("pauseReason")
                    .or_else(|| run.get("pause_reason"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                status.eq_ignore_ascii_case("paused")
                    && normalize_stop_code(pause_reason) == "process-interrupted"
                    && run
                        .get("currentRound")
                        .or_else(|| run.get("current_round"))
                        .and_then(Value::as_str)
                        == Some(self.round_id.as_str())
                    && run
                        .get("currentNode")
                        .or_else(|| run.get("current_node"))
                        .and_then(Value::as_str)
                        == Some(self.node_id.as_str())
                    && run
                        .get("currentAttempt")
                        .or_else(|| run.get("current_attempt"))
                        .and_then(Value::as_str)
                        == Some(self.attempt_id.as_str())
            })
    }
}

fn normalize_stop_code(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn provider_thread_is_active(update: &Value) -> bool {
    update
        .pointer("/_meta/codex/threadStatus/type")
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("active"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpOutputPolicy {
    Conversation,
    ArtifactContract,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcpPromptOutput {
    /// Every visible agent text chunk, including ACP v1 chunks without a
    /// provider-owned message identity. Direct conversation surfaces consume
    /// this projection.
    pub visible_text: String,
    /// Text backed by a non-empty provider-owned `messageId`. Artifact
    /// contracts consume only this projection.
    pub identified_text: String,
    pub identified_outputs: Vec<String>,
    /// Anonymous output remains visible in the timeline but is never promoted
    /// to a strict artifact candidate.
    pub anonymous_text: String,
    pub anonymous_chunk_count: u64,
}

impl AcpPromptOutput {
    fn has_identified_text(&self) -> bool {
        !self.identified_text.trim().is_empty()
    }

    fn has_anonymous_text(&self) -> bool {
        self.anonymous_chunk_count > 0 && !self.anonymous_text.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default)]
struct AcpPromptOutputAccumulator {
    output: AcpPromptOutput,
    collecting_identified_output: bool,
    visible_chars: usize,
    identified_chars: usize,
    identified_output_chars: usize,
    anonymous_chars: usize,
}

impl AcpPromptOutputAccumulator {
    fn observe(&mut self, update: &Value, event: &AcpUiEvent) {
        if event.kind != "textDelta" {
            self.collecting_identified_output = false;
            return;
        }
        let content = event.content.as_deref().unwrap_or_default();
        append_bounded(
            &mut self.output.visible_text,
            &mut self.visible_chars,
            content,
            256_000,
        );

        if agent_message_id(update).is_some() {
            if !self.collecting_identified_output {
                self.output.identified_outputs.push(String::new());
                self.collecting_identified_output = true;
                self.identified_output_chars = 0;
            }
            append_bounded(
                &mut self.output.identified_text,
                &mut self.identified_chars,
                content,
                256_000,
            );
            if let Some(output) = self.output.identified_outputs.last_mut() {
                append_bounded(output, &mut self.identified_output_chars, content, 64_000);
            }
        } else {
            self.collecting_identified_output = false;
            self.identified_output_chars = 0;
            self.output.anonymous_chunk_count = self.output.anonymous_chunk_count.saturating_add(1);
            append_bounded(
                &mut self.output.anonymous_text,
                &mut self.anonymous_chars,
                content,
                64_000,
            );
        }
    }
}

pub const ACP_UNIDENTIFIED_AGENT_OUTPUT_CODE: &str = "acp.unidentified-agent-output";

fn unidentified_agent_output_failure(
    output_policy: AcpOutputPolicy,
    output: &AcpPromptOutput,
) -> Option<AcpPromptFailure> {
    (output_policy == AcpOutputPolicy::ArtifactContract
        && !output.has_identified_text()
        && output.has_anonymous_text())
    .then(|| AcpPromptFailure {
        code: ACP_UNIDENTIFIED_AGENT_OUTPUT_CODE.to_string(),
        message: "ACP artifact prompt produced only agent output without messageId".to_string(),
        details: Some(output.anonymous_text.chars().take(2_000).collect()),
        raw: json!({
            "outputPolicy": "artifact-contract",
            "anonymousChunkCount": output.anonymous_chunk_count,
        }),
    })
}

fn agent_message_id(update: &Value) -> Option<&str> {
    (update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk"))
        .then(|| update.get("messageId").and_then(Value::as_str))
        .flatten()
        .filter(|message_id| !message_id.trim().is_empty())
}

#[derive(Debug, Clone)]
pub struct AcpPromptRun {
    pub session_id: String,
    pub adapter_id: String,
    pub adapter_display_name: String,
    pub stop_reason: Option<String>,
    pub terminal_failure: Option<AcpPromptFailure>,
    pub output: AcpPromptOutput,
    pub restored: bool,
    pub used_tokens: Option<u64>,
    pub context_window_size: Option<u64>,
    pub total_cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_read_tokens: Option<u64>,
    pub cached_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptFailure {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub raw: Value,
}

impl AcpPromptFailure {
    pub fn diagnostic(&self) -> String {
        match self.details.as_deref() {
            Some(details) if !details.trim().is_empty() && details != self.message => {
                format!("{}: {details}", self.message)
            }
            _ => self.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpRuntimePolicy {
    pub foreground_lease_ttl: Duration,
    pub foreground_lease_renew_interval: Duration,
    pub prompt_terminal_route_timeout: Duration,
    pub session_idle_ttl: Duration,
    pub adapter_connection_idle_ttl: Duration,
    pub max_idle_session_runtimes: usize,
    pub max_idle_adapter_connections: usize,
    pub timeline_compaction: TimelineCompactionPolicy,
    pub external_session_sync_enabled: bool,
    pub supports_system_prompt: bool,
    pub turn_file_capture: crate::acp::turn_files::TurnFileCaptureConfig,
}

impl Default for AcpRuntimePolicy {
    fn default() -> Self {
        Self {
            foreground_lease_ttl: Duration::from_secs(90),
            foreground_lease_renew_interval: Duration::from_secs(30),
            prompt_terminal_route_timeout: Duration::from_millis(5_000),
            session_idle_ttl: Duration::from_secs(600),
            adapter_connection_idle_ttl: Duration::from_secs(600),
            max_idle_session_runtimes: 8,
            max_idle_adapter_connections: 4,
            timeline_compaction: TimelineCompactionPolicy::default(),
            external_session_sync_enabled: false,
            supports_system_prompt: false,
            turn_file_capture: crate::acp::turn_files::TurnFileCaptureConfig::default(),
        }
    }
}

impl From<&RuntimeConfig> for AcpRuntimePolicy {
    fn from(config: &RuntimeConfig) -> Self {
        Self {
            foreground_lease_ttl: Duration::from_secs(config.acp_session_foreground_lease_ttl_secs),
            foreground_lease_renew_interval: Duration::from_secs(
                config.acp_session_foreground_lease_renew_interval_secs,
            ),
            prompt_terminal_route_timeout: Duration::from_millis(
                config.acp_prompt_terminal_route_timeout_ms,
            ),
            session_idle_ttl: Duration::from_secs(config.acp_session_idle_ttl_secs),
            adapter_connection_idle_ttl: Duration::from_secs(
                config.acp_adapter_connection_idle_ttl_secs,
            ),
            max_idle_session_runtimes: config.acp_max_idle_session_runtimes,
            max_idle_adapter_connections: config.acp_max_idle_adapter_connections,
            timeline_compaction: TimelineCompactionPolicy {
                max_size_bytes: config.acp_timeline_compact_max_size_bytes,
                patch_ratio: config.acp_timeline_compact_patch_ratio,
            },
            external_session_sync_enabled: false,
            supports_system_prompt: false,
            turn_file_capture: config.turn_files.into(),
        }
    }
}

impl AcpRuntimePolicy {
    pub fn with_external_session_sync_enabled(mut self, enabled: bool) -> Self {
        self.external_session_sync_enabled = enabled;
        self
    }

    pub fn with_system_prompt_support(mut self, supported: bool) -> Self {
        self.supports_system_prompt = supported;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderFreshnessBaseline {
    Known(String),
    Unsupported,
    Unknown,
}

#[derive(Debug)]
enum ProviderFreshnessProbe {
    Found {
        revision: Option<String>,
        title: Option<String>,
    },
    NotFound,
    Unsupported,
    TemporarilyUnavailable(anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachedSessionReusePlan {
    Reuse,
    ProbeFreshness,
    Reload(&'static str),
}

fn attached_sync_required(
    attached_sync_enabled: bool,
    pending_sync_required: bool,
    current_sync_enabled: bool,
) -> bool {
    current_sync_enabled && (pending_sync_required || !attached_sync_enabled)
}

fn plan_attached_session_reuse(
    config_changed: bool,
    sync_required: bool,
    external_session_sync_enabled: bool,
    provider_freshness: &ProviderFreshnessBaseline,
) -> AttachedSessionReusePlan {
    if config_changed {
        return AttachedSessionReusePlan::Reload("session-config-changed");
    }
    if sync_required {
        return AttachedSessionReusePlan::Reload("external-session-sync-required");
    }
    if external_session_sync_enabled
        && provider_freshness != &ProviderFreshnessBaseline::Unsupported
    {
        return AttachedSessionReusePlan::ProbeFreshness;
    }
    AttachedSessionReusePlan::Reuse
}

fn evaluate_provider_revision(
    baseline: &ProviderFreshnessBaseline,
    revision: Option<String>,
) -> (ProviderFreshnessBaseline, Option<&'static str>) {
    match (baseline, revision) {
        (ProviderFreshnessBaseline::Known(previous), Some(current)) if previous == &current => {
            (ProviderFreshnessBaseline::Known(current), None)
        }
        (ProviderFreshnessBaseline::Known(_), Some(current)) => (
            ProviderFreshnessBaseline::Known(current),
            Some("provider-revision-changed"),
        ),
        (ProviderFreshnessBaseline::Unknown, Some(current)) => (
            ProviderFreshnessBaseline::Known(current),
            Some("provider-revision-baseline-unknown"),
        ),
        (ProviderFreshnessBaseline::Unsupported, Some(current)) => (
            ProviderFreshnessBaseline::Known(current),
            Some("provider-revision-capability-recovered"),
        ),
        (_, None) => (ProviderFreshnessBaseline::Unsupported, None),
    }
}

#[derive(Clone)]
struct AttachedSessionRuntime {
    attempt_dir: Utf8PathBuf,
    connection: Arc<AdapterConnection>,
    connection_generation: u64,
    session_id: String,
    event_pump: Arc<SessionEventPump>,
    models: Option<Value>,
    modes: Option<Value>,
    config_options: Option<Value>,
    config_fingerprint: u64,
    provider_freshness: ProviderFreshnessBaseline,
    connection_key: AdapterConnectionKey,
    external_session_sync_enabled: bool,
    sync_required: bool,
    last_activity_at: Instant,
    foreground_lease_until: Instant,
    active: bool,
}

#[derive(Default)]
struct AcpSessionRuntimeRegistry {
    sessions: Mutex<HashMap<String, AttachedSessionRuntime>>,
    prompt_locks: Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>,
}

impl AcpSessionRuntimeRegistry {
    fn shared() -> &'static Self {
        static REGISTRY: LazyLock<AcpSessionRuntimeRegistry> =
            LazyLock::new(AcpSessionRuntimeRegistry::default);
        &REGISTRY
    }

    fn prompt_lock(&self, attempt_dir: &Utf8Path) -> Arc<Mutex<()>> {
        let key = attempt_dir.to_string();
        let mut locks = self
            .prompt_locks
            .lock()
            .expect("ACP prompt lock registry poisoned");
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }

    fn acquire(
        &self,
        attempt_dir: &Utf8Path,
        session_id: &str,
        connection: &Arc<AdapterConnection>,
        policy: AcpRuntimePolicy,
    ) -> Option<AttachedSessionRuntime> {
        self.prune(policy);
        let key = attempt_dir.to_string();
        let mut sessions = self.sessions.lock().ok()?;
        let entry = sessions.get_mut(&key)?;
        if entry.session_id != session_id
            || entry.connection_generation != connection.generation()
            || !Arc::ptr_eq(&entry.connection, connection)
            || connection.is_transport_closed()
            || connection.is_exited()
        {
            let stale = sessions.remove(&key);
            drop(sessions);
            if let Some(stale) = stale {
                evict_attached_session(stale);
            }
            return None;
        }
        entry.active = true;
        entry.last_activity_at = Instant::now();
        Some(entry.clone())
    }

    fn release(&self, mut entry: AttachedSessionRuntime, policy: AcpRuntimePolicy) {
        entry.active = false;
        entry.last_activity_at = Instant::now();
        entry.foreground_lease_until = Instant::now() + policy.foreground_lease_ttl;
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(entry.attempt_dir.to_string(), entry);
        }
        self.prune(policy);
    }

    fn invalidate(&self, attempt_dir: &Utf8Path) -> bool {
        let stale = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(attempt_dir.as_str()));
        if let Some(stale) = stale {
            evict_attached_session(stale);
            true
        } else {
            false
        }
    }

    fn detach_for_reload(&self, attempt_dir: &Utf8Path) -> bool {
        let stale = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(attempt_dir.as_str()));
        if let Some(stale) = stale {
            stale.event_pump.close();
            stale.connection.unregister_session_route(&stale.session_id);
            AdapterConnectionManager::shared().unregister_attempt_session(&stale.attempt_dir);
            true
        } else {
            false
        }
    }

    fn detach_provider(&self, provider_id: &str) -> usize {
        let detached = self
            .sessions
            .lock()
            .ok()
            .map(|mut sessions| {
                let keys = sessions
                    .iter()
                    .filter(|(_, entry)| entry.connection_key.provider_id == provider_id)
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                keys.into_iter()
                    .filter_map(|key| sessions.remove(&key))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let count = detached.len();
        for entry in detached {
            entry.event_pump.close();
            entry.connection.unregister_session_route(&entry.session_id);
        }
        count
    }

    fn renew_foreground_lease(&self, attempt_dir: &Utf8Path, ttl: Duration) -> bool {
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let Some(entry) = sessions.get_mut(attempt_dir.as_str()) else {
            return false;
        };
        entry.foreground_lease_until = Instant::now() + ttl;
        true
    }

    fn prune(&self, policy: AcpRuntimePolicy) {
        let now = Instant::now();
        let mut evicted = Vec::new();
        if let Ok(mut sessions) = self.sessions.lock() {
            let expired = sessions
                .iter()
                .filter(|(_, entry)| {
                    !entry.active
                        && now >= entry.foreground_lease_until
                        && now.duration_since(entry.last_activity_at) >= policy.session_idle_ttl
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in expired {
                if let Some(entry) = sessions.remove(&key) {
                    evicted.push(entry);
                }
            }

            let mut idle = sessions
                .iter()
                .filter(|(_, entry)| !entry.active && now >= entry.foreground_lease_until)
                .map(|(key, entry)| (key.clone(), entry.last_activity_at))
                .collect::<Vec<_>>();
            idle.sort_by_key(|(_, last_activity_at)| *last_activity_at);
            let overflow = idle.len().saturating_sub(policy.max_idle_session_runtimes);
            for (key, _) in idle.into_iter().take(overflow) {
                if let Some(entry) = sessions.remove(&key) {
                    evicted.push(entry);
                }
            }
        }
        for entry in evicted {
            evict_attached_session(entry);
        }
        AdapterConnectionManager::shared().prune_idle_connections(
            policy.adapter_connection_idle_ttl,
            policy.max_idle_adapter_connections,
        );
    }
}

pub fn renew_session_foreground_lease(attempt_dir: &Utf8Path, ttl: Duration) -> bool {
    AcpSessionRuntimeRegistry::shared().renew_foreground_lease(attempt_dir, ttl)
}

fn evict_attached_session(entry: AttachedSessionRuntime) {
    let _ = entry
        .connection
        .close_session_bounded(&entry.session_id, SESSION_EVICTION_CLOSE_TIMEOUT);
    entry.event_pump.close();
    entry.connection.unregister_session_route(&entry.session_id);
    AdapterConnectionManager::shared().unregister_attempt_session(&entry.attempt_dir);
}

struct AcpRuntime<'a> {
    paths: AcpAttemptPaths,
    connection_key: Option<AdapterConnectionKey>,
    connection: Arc<AdapterConnection>,
    rx: Option<Arc<SessionEventPump>>,
    seq: u64,
    timeline_revision: u64,
    timeline_store: TimelineStore,
    branch_timeline_stores: HashMap<String, TimelineStore>,
    timeline_items: HashMap<String, AcpUiEvent>,
    session_id: Option<String>,
    prompt_output: AcpPromptOutputAccumulator,
    session_update_phase: SessionUpdatePhase,
    provider_history_replay: ProviderHistoryReplay,
    historical_timeline_item_ids: HashSet<String>,
    current_turn_item_ids: HashSet<String>,
    active_turn_file_branches: HashSet<String>,
    active_prompt_turn: Option<AcpPromptTurnIdentity>,
    pending_retry_prompt_event: Option<AcpUiEvent>,
    prompt_retry: Option<AcpPromptRetryState>,
    models: Option<Value>,
    modes: Option<Value>,
    config_options: Option<Value>,
    model_override: Option<String>,
    permission_mode_override: Option<String>,
    config_option_overrides: BTreeMap<String, String>,
    available_commands: Option<Vec<AcpCommandItem>>,
    system_prompt_append: Option<String>,
    session_title: Option<String>,
    usage: AcpUsageState,
    active_timeline_streams: HashMap<String, AcpBranchTimelineStreams>,
    timing_state: AcpTimingState,
    live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
    pending_live_update: Option<AcpUiEvent>,
    last_live_update_at: Option<Instant>,
    last_live_timing_update_at: Option<Instant>,
    last_live_timing: Option<crate::acp::events::AcpTimingPatch>,
    pending_timeline_patch: Option<(u64, AcpUiEvent)>,
    last_timeline_patch_at: Option<Instant>,
    raw_max_size: u64,
    raw_target_size: u64,
    control: Arc<ProviderControl>,
    stop_probe: Option<RuntimeStopProbe>,
    runtime_policy: AcpRuntimePolicy,
    output_policy: AcpOutputPolicy,
    attached_config_fingerprint: Option<u64>,
    provider_freshness: ProviderFreshnessBaseline,
    sync_required: bool,
    retain_session_route: bool,
}

#[derive(Default)]
struct AcpBranchTimelineStreams {
    text: Option<AcpTimelineStreamState>,
    thought: Option<AcpTimelineStreamState>,
    plan: Option<AcpTimelineStreamState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionUpdatePhase {
    Live,
    RestoringWithoutReplay,
    ReplayingHistory,
    AwaitingTurnStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRestoreIntent {
    ContinueOnly,
    SyncHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRestoreMethod {
    Resume,
    Load,
}

impl SessionRestoreMethod {
    fn rpc_method(self) -> &'static str {
        match self {
            Self::Resume => "session/resume",
            Self::Load => "session/load",
        }
    }

    fn replays_history(self) -> bool {
        self == Self::Load
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SessionRestoreCapabilities {
    resume: bool,
    load: bool,
}

impl SessionRestoreCapabilities {
    fn from_agent_capabilities(capabilities: &Value) -> Self {
        let capabilities = serde_json::from_value::<
            agent_client_protocol_schema::v1::AgentCapabilities,
        >(capabilities.clone())
        .unwrap_or_default();
        Self {
            resume: capabilities.session_capabilities.resume.is_some(),
            load: capabilities.load_session,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRestorePlan {
    Restore(SessionRestoreMethod),
    StartNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum SessionRestorePlanError {
    #[error("ACP session restore is unsupported by the agent")]
    RestoreUnsupported,
    #[error("ACP full-history synchronization is unsupported by the agent")]
    HistorySyncUnsupported,
}

impl SessionRestorePlanError {
    fn code(self) -> &'static str {
        match self {
            Self::RestoreUnsupported => ACP_SESSION_RESTORE_UNSUPPORTED_CODE,
            Self::HistorySyncUnsupported => ACP_HISTORY_SYNC_UNSUPPORTED_CODE,
        }
    }
}

fn plan_session_restore(
    intent: SessionRestoreIntent,
    capabilities: SessionRestoreCapabilities,
    strict_continue: bool,
) -> std::result::Result<SessionRestorePlan, SessionRestorePlanError> {
    match intent {
        SessionRestoreIntent::SyncHistory if capabilities.load => {
            Ok(SessionRestorePlan::Restore(SessionRestoreMethod::Load))
        }
        SessionRestoreIntent::SyncHistory if capabilities.resume => {
            Err(SessionRestorePlanError::HistorySyncUnsupported)
        }
        SessionRestoreIntent::ContinueOnly if capabilities.resume => {
            Ok(SessionRestorePlan::Restore(SessionRestoreMethod::Resume))
        }
        SessionRestoreIntent::ContinueOnly if capabilities.load => {
            Ok(SessionRestorePlan::Restore(SessionRestoreMethod::Load))
        }
        _ if strict_continue => Err(SessionRestorePlanError::RestoreUnsupported),
        _ => Ok(SessionRestorePlan::StartNew),
    }
}

fn session_restore_plan_error(
    error: SessionRestorePlanError,
    intent: SessionRestoreIntent,
    capabilities: SessionRestoreCapabilities,
) -> anyhow::Error {
    runtime_error(blocked_runtime_error_info(
        RuntimeErrorDomain::Provider,
        error.code(),
        error.to_string(),
        json!({
            "intent": match intent {
                SessionRestoreIntent::ContinueOnly => "continue-only",
                SessionRestoreIntent::SyncHistory => "sync-history",
            },
            "capabilities": {
                "resume": capabilities.resume,
                "load": capabilities.load,
            },
        }),
    ))
}

#[derive(Debug, Clone)]
pub struct AcpDoctorProbe {
    pub capabilities: Value,
    pub commands: Vec<AcpCommandItem>,
}

pub fn doctor(
    agent_id: &ManagedAgentId,
    config: &AcpAdapterConfig,
    cwd: Utf8PathBuf,
    use_local_claude: bool,
    require_local_claude_executable: bool,
) -> Result<AcpDoctorProbe> {
    let paths = GoldBandPaths::new(cwd.clone());
    let doctor_acp_dir = paths.doctor_acp_dir(agent_id);
    cleanup_doctor_acp_dir_before_run(&doctor_acp_dir);
    let mut runtime = AcpRuntime::start_standalone(
        agent_id.as_str(),
        config,
        cwd.clone(),
        doctor_acp_dir.clone(),
        use_local_claude,
        require_local_claude_executable,
        DOCTOR_DIAGNOSTIC_MAX_SIZE,
        DOCTOR_DIAGNOSTIC_TARGET_SIZE,
        None,
        None,
    )?;
    let result = (|| {
        let mut capabilities = runtime.initialize_with_timeout(Some(DOCTOR_REQUEST_TIMEOUT))?;
        runtime.setup_session(
            agent_id.as_str(),
            cwd,
            None,
            None,
            None,
            &BTreeMap::new(),
            "",
            false,
            &capabilities,
            &[],
            &[],
        )?;
        runtime.wait_for_available_commands(DOCTOR_COMMAND_DISCOVERY_TIMEOUT)?;
        let commands = runtime.available_commands.clone().unwrap_or_default();
        runtime.cleanup_diagnostic_session()?;
        runtime.merge_session_config_into_capabilities(&mut capabilities);
        Ok(AcpDoctorProbe {
            capabilities,
            commands,
        })
    })();
    runtime.shutdown();
    if result.is_ok() {
        cleanup_doctor_acp_dir_after_success(&doctor_acp_dir);
    } else {
        retain_bounded_doctor_acp_failure_bundle(&doctor_acp_dir);
    }
    result
}

fn cleanup_doctor_acp_dir_before_run(dir: &Utf8Path) {
    let _ = std::fs::remove_dir_all(dir.as_std_path());
}

fn cleanup_doctor_acp_dir_after_success(dir: &Utf8Path) {
    let _ = std::fs::remove_dir_all(dir.as_std_path());
}

fn retain_bounded_doctor_acp_failure_bundle(dir: &Utf8Path) {
    let paths = AcpAttemptPaths::from_attempt_dir(dir.to_path_buf());
    let _ = std::fs::remove_file(paths.provider_pid.as_std_path());
    for path in [
        &paths.events,
        &paths.timeline,
        &paths.diagnostics,
        &paths.raw,
    ] {
        let _ = roll_jsonl(
            path,
            DOCTOR_DIAGNOSTIC_MAX_SIZE,
            DOCTOR_DIAGNOSTIC_TARGET_SIZE,
        );
    }
}

pub fn run_prompt(
    provider_id: &str,
    config: &AcpAdapterConfig,
    adapter_workspace_dir: Utf8PathBuf,
    workspace_dir: Utf8PathBuf,
    attempt_dir: Utf8PathBuf,
    prompt: &PromptBundle,
    session_mode: SessionMode,
    permission_mode: Option<String>,
    model: Option<String>,
    config_options: BTreeMap<String, String>,
    continue_ref: Option<Value>,
    use_local_claude: bool,
    require_local_claude_executable: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
    runtime_policy: AcpRuntimePolicy,
    output_policy: AcpOutputPolicy,
    live_update: Option<&dyn Fn(&AcpUiEvent) -> Result<()>>,
    mcp_servers: &[Value],
    session_update: Option<&dyn Fn() -> Result<()>>,
    prompt_accepted: Option<&dyn Fn(&str) -> Result<()>>,
    stop_probe: Option<RuntimeStopProbe>,
) -> Result<AcpPromptRun> {
    let run_prompt_started_at = Instant::now();
    let prompt_lock = AcpSessionRuntimeRegistry::shared().prompt_lock(&attempt_dir);
    let _prompt_guard = prompt_lock
        .lock()
        .map_err(|_| anyhow!("ACP session prompt lock poisoned"))?;
    let mut prompt = prompt.clone();
    prepare_runtime_control_prompt(&attempt_dir, &mut prompt)?;
    let prompt = &prompt;
    let mut runtime = AcpRuntime::start(
        provider_id,
        config,
        adapter_workspace_dir,
        attempt_dir,
        use_local_claude,
        require_local_claude_executable,
        acp_raw_max_size_bytes,
        acp_raw_target_size_bytes,
        runtime_policy,
        live_update,
        stop_probe,
    )?;
    runtime.output_policy = output_policy;
    runtime.model_override = model.clone();
    runtime.permission_mode_override = permission_mode.clone();
    runtime.config_option_overrides = config_options.clone();
    let initialize_started_at = Instant::now();
    let initialize_result = runtime.initialize();
    info!(
        target: "gold_band::perf",
        provider_id,
        elapsed_ms = initialize_started_at.elapsed().as_millis(),
        status = if initialize_result.is_ok() { "ok" } else { "error" },
        "ACP initialize completed"
    );
    let capabilities = match initialize_result {
        Ok(capabilities) => capabilities,
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            let _ = runtime.mark_pending_retry_cancelled();
            let run = runtime.interrupted_run(false, "cancelled");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) if is_transport_interruption(&error) => {
            let run = runtime.interrupted_run(false, "interrupted");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) => return Err(error),
    };
    let mcp_preparation = prepare_acp_mcp_servers(mcp_servers, Some(&capabilities));
    let strict_continue = session_mode == SessionMode::Continue && continue_ref.is_some();
    let restored = match runtime.setup_session(
        provider_id,
        workspace_dir.clone(),
        continue_ref,
        permission_mode.as_deref(),
        model.as_deref(),
        &config_options,
        &prompt.system_prompt,
        strict_continue,
        &capabilities,
        &mcp_preparation.accepted,
        &mcp_preparation.skipped,
    ) {
        Ok(restored) => restored,
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            let _ = runtime.mark_pending_retry_cancelled();
            let run = runtime.interrupted_run(false, "cancelled");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) if is_transport_interruption(&error) => {
            let run = runtime.interrupted_run(false, "interrupted");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) => {
            let _ = append_diagnostic(
                &runtime.paths.diagnostics,
                "error",
                format!("ACP session setup failed: {error}"),
                None,
            );
            runtime.shutdown();
            return Err(error);
        }
    };
    let session_id = runtime
        .session_id
        .clone()
        .ok_or_else(|| anyhow!("ACP session setup did not return a session id"))?;
    runtime.write_worker_ref(
        provider_id,
        &workspace_dir,
        session_mode,
        restored,
        None,
        true,
    )?;
    let prompt_turn = runtime.record_user_prompt_event(provider_id, prompt, restored)?;
    commit_runtime_control_prompt(&runtime.paths.attempt_dir, prompt)?;
    runtime.control.mark_accepted();
    if let Some(prompt_accepted) = prompt_accepted {
        let _ = prompt_accepted(&prompt_turn.id);
    }
    runtime.write_session("running", restored, None, capabilities.clone())?;
    if acp_session_title_refresh_enabled {
        runtime.refresh_session_title_and_persist(
            &workspace_dir,
            "running",
            restored,
            None,
            &capabilities,
        );
    }
    if let Some(session_update) = session_update {
        let _ = session_update();
    }
    info!(
        target: "gold_band::perf",
        provider_id,
        session_id = %session_id,
        restored,
        elapsed_ms = run_prompt_started_at.elapsed().as_millis(),
        "first ready ACP session update emitted"
    );
    let prompt_result = runtime.prompt(
        provider_id,
        &workspace_dir,
        prompt,
        &prompt_turn,
        restored,
        &capabilities,
        acp_session_title_refresh_enabled,
    );
    runtime.refresh_provider_freshness_best_effort(&workspace_dir);
    let cancellation = prompt_cancellation_outcome(
        runtime.is_prompt_cancel_requested(),
        prompt_result.as_ref().err(),
    );
    // Cancellation owns the user-visible terminal state. Anonymous ACP v1
    // output is a strict-contract failure only after the bounded terminal
    // drain has made the complete prompt output observable.
    let terminal_failure = (!cancellation.observed && prompt_result.is_ok())
        .then(|| {
            unidentified_agent_output_failure(runtime.output_policy, &runtime.prompt_output.output)
        })
        .flatten();
    let (status, stop_reason) = match prompt_result {
        Ok(stop_reason) => {
            let status = if cancellation.observed {
                "cancelled"
            } else if terminal_failure.is_some() {
                "failed"
            } else if stop_reason.as_deref().is_some_and(|reason| {
                matches!(
                    normalize_stop_code(reason).as_str(),
                    "cancelled" | "canceled" | "interrupted"
                )
            }) {
                "cancelled"
            } else {
                "completed"
            };
            if status == "cancelled" {
                let _ = runtime.cancel_pending_prompt_interactions(current_timestamp());
            }
            (status, stop_reason)
        }
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            let _ = runtime.cancel_pending_prompt_interactions(current_timestamp());
            ("cancelled", Some("cancelled".to_string()))
        }
        Err(error) if error.downcast_ref::<AcpCancelDrainTimeout>().is_some() => {
            let _ = runtime.cancel_pending_prompt_interactions(current_timestamp());
            ("cancelled", Some("cancelled".to_string()))
        }
        Err(error) if is_transport_interruption(&error) => {
            let _ = runtime.cancel_pending_prompt_interactions(current_timestamp());
            ("cancelled", Some("interrupted".to_string()))
        }
        Err(error) => {
            let _ = runtime.cancel_pending_prompt_interactions(current_timestamp());
            let _ = runtime.interrupt_active_context_compaction("prompt_failed");
            append_diagnostic(
                &runtime.paths.diagnostics,
                "error",
                format!("ACP prompt failed: {error}"),
                None,
            )?;
            runtime.write_worker_ref(
                provider_id,
                &workspace_dir,
                session_mode,
                restored,
                Some("error".to_string()),
                true,
            )?;
            if let Err(capture_error) = runtime.finalize_turn_file_changes(&prompt_turn) {
                append_structured_diagnostic(
                    &runtime.paths.diagnostics,
                    "error",
                    "turn-files.finalize-failed",
                    Some(json!({
                        "error": capture_error.to_string(),
                        "turnId": prompt_turn.id,
                    })),
                )?;
            }
            runtime.control.mark_stopped();
            runtime.write_session("failed", restored, Some("error".to_string()), capabilities)?;
            if let Some(session_update) = session_update {
                let _ = session_update();
            }
            runtime.shutdown();
            return Err(error);
        }
    };
    if let Some(failure) = terminal_failure.as_ref() {
        runtime.mark_prompt_terminal_failure(&prompt_turn, failure)?;
        append_diagnostic(
            &runtime.paths.diagnostics,
            "error",
            format!("ACP prompt failed: {}", failure.diagnostic()),
            Some(json!({
                "code": failure.code,
                "details": failure.details,
                "raw": failure.raw,
            })),
        )?;
    } else if status == "cancelled" {
        runtime.mark_prompt_cancelled(&prompt_turn)?;
    } else if status == "completed"
        && runtime
            .prompt_retry
            .as_ref()
            .is_some_and(|state| state.retry_attempt > 0)
    {
        runtime.mark_prompt_completed(&prompt_turn)?;
    }
    runtime.write_worker_ref(
        provider_id,
        &workspace_dir,
        session_mode,
        restored,
        stop_reason.clone(),
        !cancellation.drain_timed_out,
    )?;
    runtime
        .interrupt_active_context_compaction(stop_reason.as_deref().unwrap_or("prompt_finished"))?;
    runtime.finalize_turn_file_changes(&prompt_turn)?;
    runtime.control.mark_stopped();
    runtime.write_session(status, restored, stop_reason.clone(), capabilities)?;
    if let Some(session_update) = session_update {
        let _ = session_update();
    }
    let run = AcpPromptRun {
        session_id,
        adapter_id: runtime.connection.adapter().adapter_id.clone(),
        adapter_display_name: runtime.connection.adapter().display_name.clone(),
        stop_reason,
        terminal_failure,
        output: runtime.prompt_output.output.clone(),
        restored,
        used_tokens: runtime.usage.context.confirmed_used,
        context_window_size: runtime.usage.context.window_size,
        total_cost_usd: runtime.usage.total_cost_usd,
        input_tokens: runtime.usage.latest_prompt.input_tokens,
        output_tokens: runtime.usage.latest_prompt.output_tokens,
        cached_read_tokens: runtime.usage.latest_prompt.cached_read_tokens,
        cached_write_tokens: runtime.usage.latest_prompt.cached_write_tokens,
        total_tokens: runtime.usage.latest_prompt.total_tokens,
    };
    if cancellation.drain_timed_out {
        // The adapter process may host other reusable sessions, so keep the
        // process alive while quarantining only this undrained session.
        runtime.shutdown();
    } else {
        runtime.release_managed_session();
    }
    Ok(run)
}

fn prepare_runtime_control_prompt(attempt_dir: &Utf8Path, prompt: &mut PromptBundle) -> Result<()> {
    prompt.runtime_control_transition_id = None;
    prompt.runtime_control_source_transition_id = None;
    prompt.runtime_control_transition_cause = None;

    if prompt.turn_control_mode == TurnControlMode::NonRuntimeControlled {
        return Ok(());
    }

    if prompt.runtime_control_intent == crate::provider::RuntimeControlIntent::Resume
        && let Some((source_transition_id, transition_id)) =
            crate::acp::control::prepare_workflow_continued(attempt_dir)?
    {
        prompt.runtime_control_source_transition_id = Some(source_transition_id);
        prompt.runtime_control_transition_id = Some(transition_id);
        prompt.runtime_control_transition_cause =
            Some(TurnControlTransitionCause::WorkflowContinued);
    }
    Ok(())
}

fn commit_runtime_control_prompt(attempt_dir: &Utf8Path, prompt: &PromptBundle) -> Result<()> {
    let Some(cause) = prompt.runtime_control_transition_cause else {
        return Ok(());
    };
    let transition_id = prompt
        .runtime_control_transition_id
        .as_deref()
        .ok_or_else(|| anyhow!("runtime control transition is missing its identity"))?;
    let committed = match cause {
        TurnControlTransitionCause::RuntimeInterrupted => {
            bail!("runtime interruption is not committed by an ACP prompt")
        }
        TurnControlTransitionCause::WorkflowContinued => {
            let source_transition_id = prompt
                .runtime_control_source_transition_id
                .as_deref()
                .ok_or_else(|| anyhow!("runtime continue transition is missing its source"))?;
            crate::acp::control::commit_workflow_continued(
                attempt_dir,
                source_transition_id,
                transition_id,
            )?
        }
        TurnControlTransitionCause::RuntimeTerminal => true,
    };
    if !committed {
        bail!("runtime control transition changed before prompt acceptance");
    }
    Ok(())
}

#[cfg(test)]
fn latest_visible_turn_id<'a>(events: impl IntoIterator<Item = &'a AcpUiEvent>) -> Option<String> {
    events
        .into_iter()
        .filter(|event| {
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("source"))
                .and_then(Value::as_str)
                == Some("goldBandPrompt")
                && !event
                    .raw
                    .as_ref()
                    .and_then(|raw| raw.get("hiddenFromChat"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .max_by_key(|event| event.seq)
        .map(|event| {
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("promptId"))
                .and_then(Value::as_str)
                .filter(|prompt_id| !prompt_id.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| event.id.clone())
        })
}

fn cancel_pending_prompt_interactions(attempt_dir: &Utf8Path, decided_at: String) -> Result<()> {
    let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.to_path_buf());
    cancel_latest_processing_prompt_retry(&paths.timeline, decided_at.clone())?;
    cancel_pending_permission_requests(attempt_dir, decided_at.clone())?;
    cancel_pending_elicitation_requests(attempt_dir, decided_at)
}

fn session_new_params(cwd: &Utf8Path, system_prompt: &str, mcp_servers: &[Value]) -> Value {
    let mut params = json!({
        "cwd": cwd.as_str(),
        "mcpServers": mcp_servers,
    });
    if !system_prompt.trim().is_empty() {
        params["_meta"] = json!({
            "systemPrompt": {
                "append": system_prompt,
            },
        });
    }
    params
}

fn session_load_params(
    cwd: &Utf8Path,
    session_id: &str,
    system_prompt: &str,
    mcp_servers: &[Value],
) -> Value {
    let mut params = json!({
        "cwd": cwd.as_str(),
        "mcpServers": mcp_servers,
        "sessionId": session_id,
    });
    if !system_prompt.trim().is_empty() {
        params["_meta"] = json!({
            "systemPrompt": {
                "append": system_prompt,
            },
        });
    }
    params
}

fn session_resume_params(
    cwd: &Utf8Path,
    session_id: &str,
    system_prompt: &str,
    mcp_servers: &[Value],
) -> Value {
    let mut params = json!({
        "cwd": cwd.as_str(),
        "mcpServers": mcp_servers,
        "sessionId": session_id,
    });
    if !system_prompt.trim().is_empty() {
        params["_meta"] = json!({
            "systemPrompt": {
                "append": system_prompt,
            },
        });
    }
    params
}

fn session_config_fingerprint(
    provider_id: &str,
    cwd: &Utf8Path,
    _system_prompt: &str,
    mcp_servers: &[Value],
) -> Result<u64> {
    let mut normalized_mcp = mcp_servers
        .iter()
        .cloned()
        .map(canonicalize_json)
        .collect::<Vec<_>>();
    normalized_mcp.sort_by_key(|value| serde_json::to_string(value).unwrap_or_default());
    let canonical = json!({
        "providerId": provider_id,
        "cwd": cwd.as_str().replace('\\', "/"),
        "mcpServers": normalized_mcp,
        "sessionSystemContextVersion": SESSION_SYSTEM_CONTEXT_VERSION,
    });
    let bytes = serde_json::to_vec(&canonical)?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(hasher.finish())
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted = object
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            serde_json::to_value(sorted).unwrap_or_else(|_| json!({}))
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        other => other,
    }
}

fn session_prompt_params(
    provider_id: &str,
    session_id: &str,
    prompt: &PromptBundle,
    restored: bool,
    supports_system_prompt: bool,
) -> Value {
    let mut prompt_blocks: Vec<Value> = Vec::new();

    // Add attachment content blocks first (images, resources)
    for block in &prompt.content_blocks {
        prompt_blocks.push(serde_json::to_value(block).unwrap_or_default());
    }

    // Add the text block with user prompt
    let text = session_prompt_text(provider_id, prompt, restored, supports_system_prompt);
    if !text.is_empty() {
        prompt_blocks.push(json!({
            "type": "text",
            "text": text,
        }));
    }

    json!({
        "sessionId": session_id,
        "prompt": prompt_blocks,
    })
}

fn session_prompt_text(
    _provider_id: &str,
    prompt: &PromptBundle,
    restored: bool,
    supports_system_prompt: bool,
) -> String {
    if !restored && !supports_system_prompt && !prompt.system_prompt.trim().is_empty() {
        let system_prompt =
            gold_band_hidden_block("Gold Band stable system prompt", &prompt.system_prompt);
        return format!("{}\n\n{}", system_prompt, prompt.user_prompt);
    }

    prompt.user_prompt.clone()
}

fn is_cancel_stop_reason(result: &Value) -> bool {
    result
        .get("stopReason")
        .or_else(|| result.get("stop_reason"))
        .and_then(Value::as_str)
        .is_some_and(|reason| {
            matches!(
                normalize_stop_code(reason).as_str(),
                "cancelled" | "canceled" | "interrupted"
            )
        })
}

fn is_runtime_session_active(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "running" | "active" | "pending" | "permission_requested" | "permission-requested"
    )
}

fn parse_event_epoch_seconds(value: &str) -> Option<u64> {
    value.trim_end_matches('Z').parse::<u64>().ok()
}

fn timing_patch_reason(event: &AcpUiEvent) -> &'static str {
    if event.kind == "permissionRequest"
        && event
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("pending"))
    {
        return "permission-wait";
    }
    if event.kind == "elicitationRequest"
        && event
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("pending"))
    {
        return "elicitation-wait";
    }
    let session_update = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionUpdate"))
        .and_then(Value::as_str);
    if matches!(
        session_update,
        Some("available_commands_update" | "current_mode_update" | "session_info_update")
    ) {
        return "metadata";
    }
    "active"
}

fn timing_patch_display_values_equal(
    left: &crate::acp::events::AcpTimingPatch,
    right: &crate::acp::events::AcpTimingPatch,
) -> bool {
    left.session_elapsed_seconds == right.session_elapsed_seconds
        && left.active_turn_started_at == right.active_turn_started_at
        && left.active_turn_last_activity_at == right.active_turn_last_activity_at
        && left.permission_wait_started_at == right.permission_wait_started_at
        && left.user_wait_started_at == right.user_wait_started_at
        && left.wait_reason == right.wait_reason
        && left.paused == right.paused
}

/// Token and cost fields recovered from a prior `acp.snapshot.json` when
/// resuming a session. All fields default to `None` so that a fresh session
/// (no prior snapshot) behaves exactly as before.
struct PriorAttemptMetrics {
    used_tokens: Option<u64>,
    context_window_size: Option<u64>,
    total_cost_usd: Option<f64>,
    /// Last completed prompt usage. Kept separate because node metrics currently
    /// consume these legacy snapshot fields.
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cached_read_tokens: Option<u64>,
    cached_write_tokens: Option<u64>,
    total_tokens: Option<u64>,
    /// Canonical cumulative usage for every prompt turn in this ACP attempt.
    attempt_input_tokens: Option<u64>,
    attempt_output_tokens: Option<u64>,
    attempt_cached_read_tokens: Option<u64>,
    attempt_cached_write_tokens: Option<u64>,
    attempt_total_tokens: Option<u64>,
}

impl Default for PriorAttemptMetrics {
    fn default() -> Self {
        Self {
            used_tokens: None,
            context_window_size: None,
            total_cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            cached_read_tokens: None,
            cached_write_tokens: None,
            total_tokens: None,
            attempt_input_tokens: None,
            attempt_output_tokens: None,
            attempt_cached_read_tokens: None,
            attempt_cached_write_tokens: None,
            attempt_total_tokens: None,
        }
    }
}

/// Read token/cost fields from the current attempt's `acp.snapshot.json` so
/// runtime recreation preserves that attempt's cumulative prompt usage.
/// Returns `Default` (all `None`) when the file is missing or unreadable.
fn read_prior_attempt_metrics(snapshot_path: &Utf8Path) -> PriorAttemptMetrics {
    if !snapshot_path.exists() {
        return PriorAttemptMetrics::default();
    }
    let Ok(meta) = load_session_metadata(snapshot_path, None) else {
        return PriorAttemptMetrics::default();
    };
    PriorAttemptMetrics {
        used_tokens: meta.used_tokens,
        context_window_size: meta.context_window_size,
        total_cost_usd: meta.total_cost_usd,
        input_tokens: meta.input_tokens,
        output_tokens: meta.output_tokens,
        cached_read_tokens: meta.cached_read_tokens,
        cached_write_tokens: meta.cached_write_tokens,
        total_tokens: meta.total_tokens,
        attempt_input_tokens: meta.attempt_input_tokens,
        attempt_output_tokens: meta.attempt_output_tokens,
        attempt_cached_read_tokens: meta.attempt_cached_read_tokens,
        attempt_cached_write_tokens: meta.attempt_cached_write_tokens,
        attempt_total_tokens: meta.attempt_total_tokens,
    }
}

impl<'a> AcpRuntime<'a> {
    fn cancel_pending_prompt_interactions(&mut self, decided_at: String) -> Result<()> {
        cancel_pending_prompt_interactions(&self.paths.attempt_dir, decided_at)?;
        let timeline_items = load_all_branch_events(&self.paths.attempt_dir)?;
        self.timing_state = AcpTimingState::from_timeline_item_refs(&timeline_items);
        self.active_timeline_streams = active_timeline_streams_by_branch(&timeline_items);
        self.timeline_items = runtime_hot_timeline_items(timeline_items);
        Ok(())
    }

    fn append_timing_diagnostic(&self, event: &str, data: Value) {
        let _ = append_diagnostic(
            &self.paths.diagnostics,
            "info",
            format!("acp timing: {event}"),
            Some(data),
        );
    }

    fn start(
        provider_id: &str,
        config: &AcpAdapterConfig,
        cwd: Utf8PathBuf,
        attempt_dir: Utf8PathBuf,
        use_local_claude: bool,
        require_local_claude_executable: bool,
        raw_max_size: u64,
        raw_target_size: u64,
        runtime_policy: AcpRuntimePolicy,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
        stop_probe: Option<RuntimeStopProbe>,
    ) -> Result<Self> {
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        ensure_parent_dir(&paths.raw)?;
        ensure_parent_dir(&paths.diagnostics)?;
        let control = register_provider_control(&paths.attempt_dir);
        let key = AdapterConnectionKey::new(provider_id, cwd.clone());
        let adapter_started_at = Instant::now();
        let resolution = AdapterConnectionManager::shared()
            .get_or_spawn_with_outcome(
                provider_id,
                config,
                cwd.clone(),
                use_local_claude,
                require_local_claude_executable,
            )
            .map_err(|error| {
                unregister_provider_control(&paths.attempt_dir, &control);
                let _ = append_diagnostic(
                    &paths.diagnostics,
                    "error",
                    format!("failed to start ACP adapter: {error}"),
                    Some(json!({
                        "command": config.command,
                        "args": config.args,
                        "displayName": config.display_name,
                    })),
                );
                error
            })?;
        let connection = resolution.connection;
        info!(
            target: "gold_band::perf",
            provider_id,
            workspace_root = cwd.as_str(),
            outcome = resolution.outcome.as_str(),
            elapsed_ms = adapter_started_at.elapsed().as_millis(),
            "ACP adapter connection resolved"
        );
        let _ = append_diagnostic(
            &paths.diagnostics,
            "info",
            "acp timing: adapter connection resolved",
            Some(json!({
                "event": "acp_adapter_resolved",
                "elapsedMs": adapter_started_at.elapsed().as_millis(),
                "providerId": provider_id,
                "workspaceRoot": cwd.as_str(),
                "outcome": resolution.outcome.as_str(),
                "pid": connection.pid(),
            })),
        );
        Self::from_connection(
            provider_id,
            cwd,
            Some(key),
            connection,
            paths,
            control,
            raw_max_size,
            raw_target_size,
            runtime_policy,
            live_update,
            stop_probe,
        )
    }

    fn start_standalone(
        provider_id: &str,
        config: &AcpAdapterConfig,
        cwd: Utf8PathBuf,
        attempt_dir: Utf8PathBuf,
        use_local_claude: bool,
        require_local_claude_executable: bool,
        raw_max_size: u64,
        raw_target_size: u64,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
        stop_probe: Option<RuntimeStopProbe>,
    ) -> Result<Self> {
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        ensure_parent_dir(&paths.raw)?;
        ensure_parent_dir(&paths.diagnostics)?;
        let control = register_provider_control(&paths.attempt_dir);
        let connection = AdapterConnection::spawn_standalone(
            config,
            &cwd,
            use_local_claude,
            require_local_claude_executable,
        )
        .map_err(|error| {
            unregister_provider_control(&paths.attempt_dir, &control);
            let _ = append_diagnostic(
                &paths.diagnostics,
                "error",
                format!("failed to start ACP adapter: {error}"),
                Some(json!({
                    "command": config.command,
                    "args": config.args,
                    "displayName": config.display_name,
                })),
            );
            error
        })?;
        Self::from_connection(
            provider_id,
            cwd,
            None,
            connection,
            paths,
            control,
            raw_max_size,
            raw_target_size,
            AcpRuntimePolicy::default(),
            live_update,
            stop_probe,
        )
    }

    fn from_connection(
        _provider_id: &str,
        _workspace_dir: Utf8PathBuf,
        connection_key: Option<AdapterConnectionKey>,
        connection: Arc<AdapterConnection>,
        paths: AcpAttemptPaths,
        control: Arc<ProviderControl>,
        raw_max_size: u64,
        raw_target_size: u64,
        runtime_policy: AcpRuntimePolicy,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
        stop_probe: Option<RuntimeStopProbe>,
    ) -> Result<Self> {
        ensure_parent_dir(&paths.provider_pid)?;
        std::fs::write(
            paths.provider_pid.as_std_path(),
            connection.pid().to_string(),
        )?;
        migrate_legacy_agent_timeline(&paths.attempt_dir)?;
        let timeline_store =
            TimelineStore::open(paths.timeline.clone(), runtime_policy.timeline_compaction)?;
        let loaded_timeline_items = load_all_branch_events(&paths.attempt_dir)?;
        let seq = initial_acp_source_seq(&paths).max(
            loaded_timeline_items
                .iter()
                .map(|item| item.ended_seq.unwrap_or(item.seq))
                .max()
                .unwrap_or_default(),
        );
        let timing_state = AcpTimingState::from_timeline_item_refs(&loaded_timeline_items);
        let provider_history_replay = ProviderHistoryReplay::from_timeline(&loaded_timeline_items);
        let active_timeline_streams = active_timeline_streams_by_branch(&loaded_timeline_items);
        let context_compaction = active_context_compaction(&loaded_timeline_items);
        let historical_timeline_item_ids = loaded_timeline_items
            .iter()
            .map(|item| item.id.clone())
            .collect();
        let prompt_retry = load_session_metadata(&paths.snapshot, None)
            .ok()
            .and_then(|metadata| metadata.prompt_retry);
        let pending_retry_prompt_event = prompt_retry
            .as_ref()
            .and_then(|state| state.prompt_event_id.as_deref())
            .and_then(|event_id| {
                loaded_timeline_items
                    .iter()
                    .find(|event| event.id == event_id && is_pending_retry_prompt_event(event))
                    .cloned()
            });
        let timeline_items = runtime_hot_timeline_items(loaded_timeline_items);
        let timeline_revision = seq;
        let prior = read_prior_attempt_metrics(&paths.snapshot);
        let recovered_usage = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )?;
        let mut usage = AcpUsageState::from_prior(prior, context_compaction);
        usage.apply_recovered_attempt_usage(recovered_usage);
        Ok(Self {
            paths,
            connection_key,
            connection,
            rx: None,
            seq,
            timeline_revision,
            timeline_store,
            branch_timeline_stores: HashMap::new(),
            timeline_items,
            session_id: None,
            prompt_output: AcpPromptOutputAccumulator::default(),
            session_update_phase: SessionUpdatePhase::Live,
            provider_history_replay,
            historical_timeline_item_ids,
            current_turn_item_ids: HashSet::new(),
            active_turn_file_branches: HashSet::new(),
            active_prompt_turn: None,
            pending_retry_prompt_event,
            prompt_retry,
            models: None,
            modes: None,
            config_options: None,
            model_override: None,
            permission_mode_override: None,
            config_option_overrides: BTreeMap::new(),
            available_commands: None,
            system_prompt_append: None,
            session_title: None,
            usage,
            active_timeline_streams,
            timing_state,
            live_update,
            pending_live_update: None,
            last_live_update_at: None,
            last_live_timing_update_at: None,
            last_live_timing: None,
            pending_timeline_patch: None,
            last_timeline_patch_at: None,
            raw_max_size,
            raw_target_size,
            control,
            stop_probe,
            runtime_policy,
            output_policy: AcpOutputPolicy::Conversation,
            attached_config_fingerprint: None,
            provider_freshness: ProviderFreshnessBaseline::Unknown,
            sync_required: false,
            retain_session_route: false,
        })
    }

    fn initialize(&mut self) -> Result<Value> {
        self.initialize_with_timeout(None)
    }

    fn interrupted_run(&self, restored: bool, stop_reason: &str) -> AcpPromptRun {
        AcpPromptRun {
            session_id: self.session_id.clone().unwrap_or_else(|| {
                self.paths
                    .attempt_dir
                    .file_name()
                    .unwrap_or("session")
                    .to_string()
            }),
            adapter_id: self.connection.adapter().adapter_id.clone(),
            adapter_display_name: self.connection.adapter().display_name.clone(),
            stop_reason: Some(stop_reason.to_string()),
            terminal_failure: None,
            output: self.prompt_output.output.clone(),
            restored,
            used_tokens: self.usage.context.confirmed_used,
            context_window_size: self.usage.context.window_size,
            total_cost_usd: self.usage.total_cost_usd,
            input_tokens: self.usage.latest_prompt.input_tokens,
            output_tokens: self.usage.latest_prompt.output_tokens,
            cached_read_tokens: self.usage.latest_prompt.cached_read_tokens,
            cached_write_tokens: self.usage.latest_prompt.cached_write_tokens,
            total_tokens: self.usage.latest_prompt.total_tokens,
        }
    }

    fn mark_pending_retry_cancelled(&mut self) -> Result<()> {
        let Some(event) = self.pending_retry_prompt_event.take() else {
            return Ok(());
        };
        self.seq = self.seq.saturating_add(1);
        let event = settle_prompt_event(event, "cancelled", self.seq, None);
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(event.clone())?;
        update_runtime_hot_timeline_items(&mut self.timeline_items, &event);
        self.emit_timeline_live_update(event)
    }

    fn initialize_with_timeout(&mut self, timeout: Option<Duration>) -> Result<Value> {
        if let Some(capabilities) = self.connection.initialized_capabilities() {
            self.append_timing_diagnostic(
                "acp_initialize_cached",
                json!({
                    "event": "acp_initialize_cached",
                    "status": "ok",
                }),
            );
            return Ok(capabilities);
        }
        let result = self.request_with_timeout("initialize", initialize_params(), timeout)?;
        let capabilities = result
            .get("agentCapabilities")
            .cloned()
            .unwrap_or_else(|| json!({}));
        self.connection
            .set_initialized_capabilities(capabilities.clone());
        Ok(capabilities)
    }

    fn setup_session(
        &mut self,
        provider_id: &str,
        cwd: Utf8PathBuf,
        continue_ref: Option<Value>,
        permission_mode: Option<&str>,
        model: Option<&str>,
        config_options: &BTreeMap<String, String>,
        system_prompt: &str,
        strict_continue: bool,
        agent_capabilities: &Value,
        mcp_servers: &[Value],
        skipped_mcp_servers: &[SkippedAcpMcpServer],
    ) -> Result<bool> {
        let adapter_system_prompt = if self.runtime_policy.supports_system_prompt {
            system_prompt
        } else {
            ""
        };
        self.system_prompt_append = if adapter_system_prompt.trim().is_empty() {
            None
        } else {
            Some(adapter_system_prompt.to_string())
        };
        let desired_config_fingerprint =
            session_config_fingerprint(provider_id, &cwd, adapter_system_prompt, mcp_servers)?;
        self.attached_config_fingerprint = Some(desired_config_fingerprint);
        let mut skipped_mcp_diagnostic_recorded = false;
        if let Some(session_id) = continue_ref
            .as_ref()
            .and_then(|value| value.get("acpSessionId"))
            .and_then(Value::as_str)
        {
            if self.try_reuse_attached_session(
                session_id,
                &cwd,
                desired_config_fingerprint,
                permission_mode,
                model,
                config_options,
            )? {
                return Ok(true);
            }
            let restore_capabilities =
                SessionRestoreCapabilities::from_agent_capabilities(agent_capabilities);
            let restore_intent = if self.runtime_policy.external_session_sync_enabled {
                SessionRestoreIntent::SyncHistory
            } else {
                SessionRestoreIntent::ContinueOnly
            };
            let restore_plan =
                plan_session_restore(restore_intent, restore_capabilities, strict_continue)
                    .map_err(|error| {
                        session_restore_plan_error(error, restore_intent, restore_capabilities)
                    })?;
            let SessionRestorePlan::Restore(restore_method) = restore_plan else {
                if strict_continue {
                    unreachable!("strict continue cannot plan a new ACP session");
                }
                self.record_skipped_mcp_servers(provider_id, skipped_mcp_servers);
                return self.start_new_session(
                    provider_id,
                    &cwd,
                    permission_mode,
                    model,
                    config_options,
                    adapter_system_prompt,
                    mcp_servers,
                );
            };
            self.session_update_phase = match restore_method {
                SessionRestoreMethod::Resume => SessionUpdatePhase::RestoringWithoutReplay,
                SessionRestoreMethod::Load => SessionUpdatePhase::ReplayingHistory,
            };
            if restore_method.replays_history() && self.runtime_policy.external_session_sync_enabled
            {
                self.provider_history_replay.begin(provider_id, session_id);
            }
            self.record_skipped_mcp_servers(provider_id, skipped_mcp_servers);
            skipped_mcp_diagnostic_recorded = true;
            let restore_params = match restore_method {
                SessionRestoreMethod::Resume => {
                    session_resume_params(&cwd, session_id, adapter_system_prompt, mcp_servers)
                }
                SessionRestoreMethod::Load => {
                    session_load_params(&cwd, session_id, adapter_system_prompt, mcp_servers)
                }
            };
            let restore = self.request(restore_method.rpc_method(), restore_params);
            let required_sync = restore_intent == SessionRestoreIntent::SyncHistory;
            match restore {
                Ok(result) => {
                    self.capture_session_config(&result);
                    self.set_session_id(session_id.to_string());
                    if restore_method == SessionRestoreMethod::Resume {
                        self.session_update_phase = SessionUpdatePhase::AwaitingTurnStart;
                    }
                    self.apply_session_mode_options(permission_mode, model, config_options)?;
                    if restore_method.replays_history() {
                        self.drain_session_replay_until_quiet(session_id)?;
                        self.finish_provider_history_replay(Some(session_id.to_string()))?;
                    }
                    self.sync_required = false;
                    self.refresh_provider_freshness_best_effort(&cwd);
                    return Ok(true);
                }
                Err(err) => {
                    self.session_update_phase = SessionUpdatePhase::Live;
                    append_diagnostic(
                        &self.paths.diagnostics,
                        "warn",
                        format!(
                            "failed to {} ACP session `{session_id}`: {err}",
                            match restore_method {
                                SessionRestoreMethod::Resume => "resume",
                                SessionRestoreMethod::Load => "load",
                            }
                        ),
                        None,
                    )?;
                    if is_transport_interruption(&err) {
                        self.set_session_id(session_id.to_string());
                        return Err(err);
                    }
                    if required_sync {
                        bail!("failed to synchronize existing ACP session before prompt: {err}");
                    }
                    if strict_continue {
                        bail!(
                            "failed to restore existing ACP session for continue via {}: {err}",
                            restore_method.rpc_method()
                        );
                    }
                }
            }
        }

        if strict_continue {
            bail!("ACP continue requires an existing session id");
        }

        if !skipped_mcp_diagnostic_recorded {
            self.record_skipped_mcp_servers(provider_id, skipped_mcp_servers);
        }
        self.start_new_session(
            provider_id,
            &cwd,
            permission_mode,
            model,
            config_options,
            adapter_system_prompt,
            mcp_servers,
        )
    }

    fn start_new_session(
        &mut self,
        provider_id: &str,
        cwd: &Utf8Path,
        permission_mode: Option<&str>,
        model: Option<&str>,
        config_options: &BTreeMap<String, String>,
        adapter_system_prompt: &str,
        mcp_servers: &[Value],
    ) -> Result<bool> {
        let session_new_started_at = Instant::now();
        let session_new_result = self.request(
            "session/new",
            session_new_params(cwd, adapter_system_prompt, mcp_servers),
        );
        info!(
            target: "gold_band::perf",
            provider_id,
            workspace_root = cwd.as_str(),
            mcp_server_count = mcp_servers.len(),
            elapsed_ms = session_new_started_at.elapsed().as_millis(),
            status = if session_new_result.is_ok() { "ok" } else { "error" },
            "ACP session/new completed"
        );
        let result = session_new_result?;
        self.capture_session_config(&result);
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("ACP session/new response missing sessionId"))?;
        self.set_session_id(session_id.to_string());
        self.session_update_phase = SessionUpdatePhase::Live;
        self.sync_required = false;
        self.apply_session_mode_options(permission_mode, model, config_options)?;
        self.refresh_provider_freshness_best_effort(cwd);
        Ok(false)
    }

    fn record_skipped_mcp_servers(
        &self,
        provider_id: &str,
        skipped_mcp_servers: &[SkippedAcpMcpServer],
    ) {
        if skipped_mcp_servers.is_empty() {
            return;
        }
        if let Err(error) = append_structured_diagnostic(
            &self.paths.diagnostics,
            "warning",
            ACP_MCP_TRANSPORT_UNSUPPORTED_CODE,
            Some(json!({
                "agentType": provider_id,
                "skippedServers": skipped_mcp_servers,
            })),
        ) {
            debug!(
                provider_id,
                skipped_count = skipped_mcp_servers.len(),
                %error,
                "failed to persist skipped MCP transport diagnostic"
            );
        }
    }

    fn try_reuse_attached_session(
        &mut self,
        session_id: &str,
        cwd: &Utf8Path,
        desired_config_fingerprint: u64,
        permission_mode: Option<&str>,
        model: Option<&str>,
        config_options: &BTreeMap<String, String>,
    ) -> Result<bool> {
        let Some(entry) = AcpSessionRuntimeRegistry::shared().acquire(
            &self.paths.attempt_dir,
            session_id,
            &self.connection,
            self.runtime_policy,
        ) else {
            return Ok(false);
        };
        self.session_id = Some(entry.session_id.clone());
        self.rx = Some(Arc::clone(&entry.event_pump));
        self.models = entry.models.clone();
        self.modes = entry.modes.clone();
        self.config_options = entry.config_options.clone();
        self.provider_freshness = entry.provider_freshness.clone();
        self.sync_required = attached_sync_required(
            entry.external_session_sync_enabled,
            entry.sync_required,
            self.runtime_policy.external_session_sync_enabled,
        );
        self.retain_session_route = true;

        let reuse_plan = plan_attached_session_reuse(
            entry.config_fingerprint != desired_config_fingerprint,
            self.sync_required,
            self.runtime_policy.external_session_sync_enabled,
            &entry.provider_freshness,
        );
        let mut reload_reason = match reuse_plan {
            AttachedSessionReusePlan::Reload(reason) => Some(reason),
            AttachedSessionReusePlan::Reuse | AttachedSessionReusePlan::ProbeFreshness => None,
        };
        if reuse_plan == AttachedSessionReusePlan::ProbeFreshness {
            match self.probe_session_freshness(cwd) {
                ProviderFreshnessProbe::Found { revision, title } => {
                    if title.is_some() {
                        self.session_title = title;
                    }
                    let (next_baseline, reason) =
                        evaluate_provider_revision(&entry.provider_freshness, revision);
                    self.provider_freshness = next_baseline;
                    reload_reason = reason;
                }
                ProviderFreshnessProbe::Unsupported => {
                    self.provider_freshness = ProviderFreshnessBaseline::Unsupported;
                }
                ProviderFreshnessProbe::TemporarilyUnavailable(error) => {
                    self.provider_freshness = ProviderFreshnessBaseline::Unknown;
                    let _ = append_diagnostic(
                        &self.paths.diagnostics,
                        "warn",
                        format!("ACP session freshness probe unavailable: {error}"),
                        Some(json!({ "event": "acp_freshness_probe", "result": "unavailable" })),
                    );
                }
                ProviderFreshnessProbe::NotFound => {
                    AcpSessionRuntimeRegistry::shared().detach_for_reload(&self.paths.attempt_dir);
                    self.retain_session_route = false;
                    self.rx = None;
                    self.session_id = None;
                    bail!("ACP session `{session_id}` was not found by session/list");
                }
            }
        } else if entry.provider_freshness == ProviderFreshnessBaseline::Unsupported {
            self.provider_freshness = ProviderFreshnessBaseline::Unsupported;
        }

        if let Some(reason) = reload_reason {
            let _ = append_diagnostic(
                &self.paths.diagnostics,
                "info",
                "ACP session reload required",
                Some(json!({ "event": "acp_reload_decision", "reason": reason })),
            );
            AcpSessionRuntimeRegistry::shared().detach_for_reload(&self.paths.attempt_dir);
            self.retain_session_route = false;
            self.rx = None;
            self.session_id = None;
            return Ok(false);
        }

        self.apply_session_mode_options(permission_mode, model, config_options)?;
        self.session_update_phase = SessionUpdatePhase::Live;
        let _ = append_diagnostic(
            &self.paths.diagnostics,
            "info",
            "ACP attached session runtime reused",
            Some(json!({
                "event": "acp_session_runtime_reused",
                "sessionId": session_id,
                "connectionGeneration": self.connection.generation(),
            })),
        );
        Ok(true)
    }

    fn set_session_id(&mut self, session_id: String) {
        if let Some(existing) = self.session_id.take() {
            self.connection.unregister_session_route(&existing);
        }
        self.rx = Some(self.connection.register_session_event_pump(&session_id));
        if let Some(key) = self.connection_key.clone() {
            AdapterConnectionManager::shared().register_attempt_session(
                &self.paths.attempt_dir,
                key,
                session_id.clone(),
            );
        }
        self.session_id = Some(session_id);
    }

    fn capture_session_config(&mut self, result: &Value) {
        if let Some(models) = result.get("models") {
            self.models = Some(models.clone());
        }
        if let Some(modes) = result.get("modes") {
            self.modes = Some(modes.clone());
        }
        if let Some(config_options) = result.get("configOptions") {
            self.config_options = Some(config_options.clone());
        }
    }

    /// Applies the effective session configuration for the ACP session.
    fn apply_session_mode_options(
        &mut self,
        permission_mode: Option<&str>,
        model: Option<&str>,
        config_options: &BTreeMap<String, String>,
    ) -> Result<()> {
        // Some adapters persist both options into one process-global config file.
        // Keep the pair atomic across all sessions sharing this adapter process.
        let connection = Arc::clone(&self.connection);
        let _transaction = connection.lock_session_config_transaction()?;
        if let Some(m) = model.filter(|v| !v.trim().is_empty()) {
            self.set_session_model(m)?;
        }
        if let Some(pm) = permission_mode.filter(|v| !v.trim().is_empty()) {
            self.apply_permission_mode(pm)?;
        }
        for (config_id, value) in config_options {
            self.apply_generic_config_option(config_id, value)?;
        }
        Ok(())
    }

    fn apply_generic_config_option(&mut self, config_id: &str, value: &str) -> Result<()> {
        let config_id = config_id.trim();
        let value = value.trim();
        if config_id.is_empty() || value.is_empty() {
            return Ok(());
        }
        let Some(option) = self
            .config_options
            .as_ref()
            .and_then(Value::as_array)
            .and_then(|options| {
                options
                    .iter()
                    .find(|option| option.get("id").and_then(Value::as_str) == Some(config_id))
            })
        else {
            bail!("ACP session does not expose config option `{config_id}`");
        };
        let category = option.get("category").and_then(Value::as_str);
        if matches!(category, Some("model" | "mode")) {
            return Ok(());
        }
        let valid = option
            .get("options")
            .and_then(Value::as_array)
            .is_some_and(|options| {
                options
                    .iter()
                    .any(|item| item.get("value").and_then(Value::as_str) == Some(value))
            });
        if !valid {
            bail!("ACP config option `{config_id}` does not support value `{value}`");
        }
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP config selection requires a session id"))?;
        let result = self.request(
            "session/set_config_option",
            json!({
                "sessionId": session_id,
                "configId": config_id,
                "value": value,
            }),
        )?;
        self.capture_session_config(&result);
        set_config_option_current_value(self.config_options.as_mut(), config_id, value);
        Ok(())
    }

    fn set_session_model(&mut self, model: &str) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP model selection requires a session id"))?;
        let model = match resolve_session_model(model, self.config_options.as_ref()) {
            SessionModelResolution::Unspecified => return Ok(()),
            SessionModelResolution::Selected(model) => model,
            SessionModelResolution::Stale {
                requested,
                available,
            } => {
                append_diagnostic(
                    &self.paths.diagnostics,
                    "warn",
                    format!(
                        "configured ACP model `{requested}` is no longer available; using the provider default"
                    ),
                    Some(json!({
                        "event": "acp_model_config_normalized",
                        "requestedModel": requested,
                        "availableModels": available,
                    })),
                )?;
                self.model_override = None;
                return Ok(());
            }
        };
        if has_model_config_option(self.config_options.as_ref()) {
            let config_id = self
                .config_options
                .as_ref()
                .and_then(find_model_config_option)
                .and_then(|option| option.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("model")
                .to_string();
            let result = self.request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": config_id,
                    "value": model,
                }),
            )?;
            self.capture_session_config(&result);
            self.set_current_model(&model);
            return Ok(());
        }
        if self.modes.is_some() {
            let result = self.request(
                "session/set_mode",
                json!({
                    "sessionId": session_id,
                    "modeId": model,
                }),
            )?;
            self.capture_session_config(&result);
            self.set_current_model(&model);
        }
        Ok(())
    }

    fn set_current_model(&mut self, model: &str) {
        if let Some(models) = self.models.as_mut().and_then(Value::as_object_mut) {
            models.insert(
                "currentModelId".to_string(),
                Value::String(model.to_string()),
            );
        }
        if let Some(options) = self.config_options.as_mut().and_then(Value::as_array_mut) {
            if let Some(option) = options.iter_mut().find(|option| {
                option.get("id").and_then(Value::as_str) == Some("model")
                    || option.get("category").and_then(Value::as_str) == Some("model")
            }) {
                if let Some(object) = option.as_object_mut() {
                    object.insert("currentValue".to_string(), Value::String(model.to_string()));
                }
            }
        }
    }

    fn apply_permission_mode(&mut self, permission_mode: &str) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP permission mode requires a session id"))?;
        let permission_mode = resolve_permission_mode(
            permission_mode,
            self.config_options.as_ref(),
            self.modes.as_ref(),
        )?;
        if permission_mode.is_empty() {
            return Ok(());
        }

        if has_mode_config_option(self.config_options.as_ref()) {
            let config_id = self
                .config_options
                .as_ref()
                .and_then(find_mode_config_option)
                .and_then(|option| option.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("mode")
                .to_string();
            let result = self.request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": config_id,
                    "value": permission_mode,
                }),
            )?;
            self.capture_session_config(&result);
            self.set_current_mode(&permission_mode);
            return Ok(());
        }

        if self.modes.is_some() {
            let result = self.request(
                "session/set_mode",
                json!({
                    "sessionId": session_id,
                    "modeId": permission_mode,
                }),
            )?;
            self.capture_session_config(&result);
            self.set_current_mode(&permission_mode);
            return Ok(());
        }

        bail!("ACP session does not expose mode configuration APIs")
    }

    fn set_current_mode(&mut self, permission_mode: &str) {
        if let Some(modes) = self.modes.as_mut().and_then(Value::as_object_mut) {
            modes.insert(
                "currentModeId".to_string(),
                Value::String(permission_mode.to_string()),
            );
        }
        if let Some(options) = self.config_options.as_mut().and_then(Value::as_array_mut) {
            if let Some(option) = options.iter_mut().find(|option| {
                option.get("id").and_then(Value::as_str) == Some("mode")
                    || option.get("category").and_then(Value::as_str) == Some("mode")
            }) {
                if let Some(object) = option.as_object_mut() {
                    object.insert(
                        "currentValue".to_string(),
                        Value::String(permission_mode.to_string()),
                    );
                }
            }
        }
    }

    fn merge_session_config_into_capabilities(&self, capabilities: &mut Value) {
        let object = capabilities.as_object_mut();
        if let Some(object) = object {
            if let Some(models) = &self.models {
                object.insert("models".to_string(), models.clone());
            }
            if let Some(modes) = &self.modes {
                object.insert("modes".to_string(), modes.clone());
            }
            if let Some(config_options) = &self.config_options {
                object.insert("configOptions".to_string(), config_options.clone());
            }
            return;
        }

        *capabilities = json!({
            "models": self.models.clone(),
            "modes": self.modes.clone(),
            "configOptions": self.config_options.clone(),
        });
    }

    fn cleanup_diagnostic_session(&mut self) -> Result<()> {
        let Some(session_id) = self.session_id.clone() else {
            return Ok(());
        };
        if self
            .delete_session_bounded(&session_id, SESSION_CLOSE_TIMEOUT)
            .is_ok()
        {
            return Ok(());
        }
        let _ = self.close_session_bounded(&session_id, SESSION_CLOSE_TIMEOUT);
        Ok(())
    }

    fn wait_for_available_commands(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            self.drain_available_inbound()?;
            if self.available_commands.is_some() || Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn close_session_bounded(&mut self, session_id: &str, timeout: Duration) -> Result<()> {
        self.request_with_timeout(
            "session/close",
            json!({
                "sessionId": session_id,
            }),
            Some(timeout),
        )?;
        Ok(())
    }

    fn delete_session_bounded(&mut self, session_id: &str, timeout: Duration) -> Result<()> {
        self.request_with_timeout(
            "session/delete",
            json!({
                "sessionId": session_id,
            }),
            Some(timeout),
        )?;
        Ok(())
    }

    fn probe_session_freshness(&mut self, workspace_dir: &Utf8Path) -> ProviderFreshnessProbe {
        let Some(session_id) = self.session_id.clone() else {
            return ProviderFreshnessProbe::NotFound;
        };
        let mut cursor: Option<String> = None;
        for _ in 0..SESSION_LIST_MAX_PAGES {
            let mut params = json!({ "cwd": workspace_dir.as_str() });
            if let Some(cursor) = cursor.as_ref() {
                params["cursor"] = json!(cursor);
            }
            let result = match self.request_with_timeout(
                "session/list",
                params,
                Some(SESSION_FRESHNESS_TIMEOUT),
            ) {
                Ok(result) => result,
                Err(error) if session_list_is_unsupported(&error) => {
                    return ProviderFreshnessProbe::Unsupported;
                }
                Err(error) => return ProviderFreshnessProbe::TemporarilyUnavailable(error),
            };
            if let Some(session) =
                result
                    .get("sessions")
                    .and_then(Value::as_array)
                    .and_then(|sessions| {
                        sessions.iter().find(|session| {
                            session.get("sessionId").and_then(Value::as_str)
                                == Some(session_id.as_str())
                        })
                    })
            {
                let revision = session
                    .get("updatedAt")
                    .or_else(|| session.get("updated_at"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let title = session
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(str::to_string);
                return ProviderFreshnessProbe::Found { revision, title };
            }
            cursor = result
                .get("nextCursor")
                .or_else(|| result.get("next_cursor"))
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                return ProviderFreshnessProbe::NotFound;
            }
        }
        ProviderFreshnessProbe::NotFound
    }

    fn refresh_provider_freshness(&mut self, workspace_dir: &Utf8Path) -> Result<()> {
        match self.probe_session_freshness(workspace_dir) {
            ProviderFreshnessProbe::Found { revision, title } => {
                self.session_title = title;
                self.provider_freshness = revision
                    .map(ProviderFreshnessBaseline::Known)
                    .unwrap_or(ProviderFreshnessBaseline::Unsupported);
                Ok(())
            }
            ProviderFreshnessProbe::Unsupported => {
                self.provider_freshness = ProviderFreshnessBaseline::Unsupported;
                Ok(())
            }
            ProviderFreshnessProbe::NotFound => {
                self.provider_freshness = ProviderFreshnessBaseline::Unknown;
                bail!("ACP session was not found by session/list")
            }
            ProviderFreshnessProbe::TemporarilyUnavailable(error) => {
                self.provider_freshness = ProviderFreshnessBaseline::Unknown;
                Err(error)
            }
        }
    }

    fn refresh_session_title(&mut self, workspace_dir: &Utf8Path) -> Result<()> {
        match self.probe_session_freshness(workspace_dir) {
            ProviderFreshnessProbe::Found { title, .. } => {
                self.session_title = title;
                Ok(())
            }
            ProviderFreshnessProbe::Unsupported => Ok(()),
            ProviderFreshnessProbe::NotFound => {
                bail!("ACP session was not found by session/list")
            }
            ProviderFreshnessProbe::TemporarilyUnavailable(error) => Err(error),
        }
    }

    fn refresh_provider_freshness_best_effort(&mut self, workspace_dir: &Utf8Path) {
        if !self.runtime_policy.external_session_sync_enabled || self.sync_required {
            return;
        }
        if let Err(error) = self.refresh_provider_freshness(workspace_dir) {
            let _ = append_diagnostic(
                &self.paths.diagnostics,
                "warn",
                format!("failed to probe ACP session freshness via session/list: {error}"),
                Some(json!({ "event": "acp_freshness_probe", "result": "unavailable" })),
            );
        }
    }

    fn refresh_session_title_best_effort(&mut self, workspace_dir: &Utf8Path) {
        if let Err(error) = self.refresh_session_title(workspace_dir) {
            let _ = append_diagnostic(
                &self.paths.diagnostics,
                "warn",
                format!("failed to refresh ACP session title via session/list: {error}"),
                None,
            );
        }
    }

    fn refresh_session_title_and_persist(
        &mut self,
        workspace_dir: &Utf8Path,
        status: &str,
        restored: bool,
        stop_reason: Option<String>,
        capabilities: &Value,
    ) {
        self.refresh_session_title_best_effort(workspace_dir);
        let _ = self.write_session(status, restored, stop_reason, capabilities.clone());
    }

    fn record_user_prompt_event(
        &mut self,
        provider_id: &str,
        prompt: &PromptBundle,
        restored: bool,
    ) -> Result<AcpPromptTurnIdentity> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP prompt requires a session id"))?;
        let prior_retry = self.prompt_retry.clone();
        let prompt_id = prompt
            .prompt_id
            .as_deref()
            .filter(|prompt_id| !prompt_id.trim().is_empty())
            .map(str::to_string)
            // An automatic retry reopens the provider session, but it is
            // still the same user turn. Reuse its durable identity instead
            // of comparing the user-visible text.
            .or_else(|| prior_retry.as_ref().map(|state| state.prompt_id.clone()))
            .unwrap_or_else(|| format!("acp-turn-{}", self.seq + 1));
        let hidden_from_chat = prompt.visibility == PromptVisibility::Hidden;
        let retry_attempt =
            next_prompt_retry_attempt(prior_retry.as_ref(), &prompt_id, hidden_from_chat);
        self.seq += 1;
        let operation_seq = self.seq;
        let operation_timestamp = current_timestamp();
        let (prompt_event_id, prompt_event_seq, prompt_event_timestamp) =
            canonical_prompt_event_identity(
                prior_retry.as_ref(),
                &prompt_id,
                hidden_from_chat,
                operation_seq,
                &operation_timestamp,
            );
        self.prompt_retry = Some(AcpPromptRetryState {
            prompt_id: prompt_id.clone(),
            retry_attempt,
            prompt_event_id: Some(prompt_event_id.clone()),
            prompt_event_seq: Some(prompt_event_seq),
            prompt_event_timestamp: Some(prompt_event_timestamp.clone()),
            hidden_from_chat,
        });
        let mut user_event = user_prompt_event_with_quotes(
            prompt_event_seq,
            session_id,
            prompt.display_text.clone().unwrap_or_else(|| {
                session_prompt_text(
                    provider_id,
                    prompt,
                    restored,
                    self.runtime_policy.supports_system_prompt,
                )
            }),
            Some(prompt_id.clone()),
            hidden_from_chat,
            prompt.attachment_metas.clone(),
            prompt.quotes.clone(),
        );
        if hidden_from_chat
            && let Some(reason) = prompt.hidden_reason.as_deref()
            && let Some(raw) = user_event.raw.as_mut()
        {
            raw["reason"] = Value::String(reason.to_string());
        }
        if let Some(raw) = user_event.raw.as_mut() {
            raw["turnControlMode"] = serde_json::to_value(prompt.turn_control_mode)?;
            if let (Some(transition_id), Some(transition_cause)) = (
                prompt.runtime_control_transition_id.as_deref(),
                prompt.runtime_control_transition_cause,
            ) {
                let runtime_control = json!({
                    "currentMode": prompt.turn_control_mode,
                    "transitionId": transition_id,
                    "transitionCause": transition_cause,
                    "changedAt": user_event.timestamp,
                });
                raw["runtimeControl"] = runtime_control;
            }
        }
        user_event.id = prompt_event_id.clone();
        user_event.timestamp = prompt_event_timestamp;
        if retry_attempt > 0 {
            // Scheduling the retry already moved the canonical prompt to
            // processing. Rebuilding the provider runtime must advance that
            // same lifecycle, not momentarily reset it to completed.
            user_event.status = Some("processing".to_string());
            user_event.started_seq = Some(prompt_event_seq);
            user_event.started_at = Some(user_event.timestamp.clone());
            user_event.ended_seq = Some(operation_seq);
            user_event.ended_at = Some(operation_timestamp.clone());
            if let Some(raw) = user_event.raw.as_mut() {
                raw["retry"] = json!({
                    "attempt": retry_attempt,
                    "maxAttempts": DEFAULT_AUTO_RETRY_MAX_ATTEMPTS,
                });
            }
        }
        self.persist_event(&user_event)?;
        let usage_transaction_id =
            prompt_usage_transaction_id(&prompt_event_id, retry_attempt, operation_seq);
        append_prompt_started(
            &self.paths.prompt_usage,
            &usage_transaction_id,
            operation_seq,
            &operation_timestamp,
        )?;
        let turn_id = prompt_id;
        let identity = AcpPromptTurnIdentity {
            id: turn_id,
            prompt_event_id: user_event.id.clone(),
            usage_transaction_id,
            usage_transaction_seq: operation_seq,
            started_at: user_event.timestamp.clone(),
            event: user_event,
        };
        self.active_turn_file_branches.clear();
        self.pending_retry_prompt_event = None;
        self.active_prompt_turn = Some(identity.clone());
        Ok(identity)
    }

    fn mark_prompt_terminal_failure(
        &mut self,
        prompt_turn: &AcpPromptTurnIdentity,
        failure: &AcpPromptFailure,
    ) -> Result<()> {
        let event =
            settle_prompt_event(prompt_turn.event.clone(), "failed", self.seq, Some(failure));
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(event.clone())?;
        update_runtime_hot_timeline_items(&mut self.timeline_items, &event);
        self.emit_timeline_live_update(event)
    }

    fn mark_prompt_cancelled(&mut self, prompt_turn: &AcpPromptTurnIdentity) -> Result<()> {
        let event = settle_prompt_event(prompt_turn.event.clone(), "cancelled", self.seq, None);
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(event.clone())?;
        update_runtime_hot_timeline_items(&mut self.timeline_items, &event);
        self.emit_timeline_live_update(event)
    }

    fn mark_prompt_completed(&mut self, prompt_turn: &AcpPromptTurnIdentity) -> Result<()> {
        let mut event = prompt_turn.event.clone();
        event.status = Some("completed".to_string());
        event.ended_seq = Some(self.seq);
        event.ended_at = Some(current_timestamp());
        if let Some(raw) = event.raw.as_mut().and_then(Value::as_object_mut) {
            raw.remove("retry");
            raw.remove("cancelled");
            raw.remove("terminalFailure");
        }
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(event.clone())?;
        update_runtime_hot_timeline_items(&mut self.timeline_items, &event);
        self.emit_timeline_live_update(event)
    }

    fn prompt(
        &mut self,
        provider_id: &str,
        workspace_dir: &Utf8Path,
        prompt: &PromptBundle,
        prompt_turn: &AcpPromptTurnIdentity,
        restored: bool,
        capabilities: &Value,
        acp_session_title_refresh_enabled: bool,
    ) -> Result<Option<String>> {
        self.prompt_output = AcpPromptOutputAccumulator::default();
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP prompt requires a session id"))?;
        let result = self.request_prompt_with_cancel(
            provider_id,
            &session_id,
            prompt,
            prompt_turn,
            restored,
            acp_session_title_refresh_enabled.then_some((
                workspace_dir,
                "running",
                restored,
                None,
                capabilities,
            )),
        )?;
        if acp_session_title_refresh_enabled {
            self.refresh_session_title_and_persist(
                workspace_dir,
                "running",
                restored,
                None,
                capabilities,
            );
        }
        Ok(result
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_with_progress(method, params, None, None)
    }

    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        self.request_with_progress(method, params, timeout, None)
    }

    fn request_with_progress(
        &mut self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
        title_refresh: Option<(&Utf8Path, &str, bool, Option<String>, &Value)>,
    ) -> Result<Value> {
        if self.is_prompt_cancel_requested() {
            self.observe_prompt_cancel_request()?;
            return Err(anyhow!(AcpCancelled));
        }
        let diagnostic_started_at = Instant::now();
        self.append_timing_diagnostic(
            "acp_rpc_begin",
            json!({
                "event": "acp_rpc_begin",
                "method": method,
                "sessionId": self.session_id,
            }),
        );
        let request = self.connection.begin_request(method, params)?;
        self.append_outbound_frame(&request.frame)?;
        let started_at = Instant::now();
        let mut last_title_refresh_at = Instant::now();
        loop {
            if self.is_prompt_cancel_requested() {
                self.observe_prompt_cancel_request()?;
                self.connection.cancel_pending(request.id);
                return Err(anyhow!(AcpCancelled));
            }
            let wait_for = match timeout {
                Some(timeout) => match timeout.checked_sub(started_at.elapsed()) {
                    Some(remaining) => remaining.min(STOP_CHECK_INTERVAL),
                    None => {
                        self.connection.cancel_pending(request.id);
                        self.append_timing_diagnostic(
                            "acp_rpc_end",
                            json!({
                                "event": "acp_rpc_end",
                                "method": method,
                                "requestId": request.id,
                                "elapsedMs": diagnostic_started_at.elapsed().as_millis(),
                                "status": "timeout",
                                "timeoutSeconds": timeout.as_secs(),
                                "sessionId": self.session_id,
                            }),
                        );
                        bail!(
                            "ACP `{method}` timed out after {} seconds",
                            timeout.as_secs()
                        );
                    }
                },
                None => STOP_CHECK_INTERVAL,
            };
            match request.recv_timeout(wait_for) {
                Ok(value) => {
                    self.append_inbound_frame(&value)?;
                    self.drain_available_inbound()?;
                    if self.is_prompt_cancel_requested() {
                        self.observe_prompt_cancel_request()?;
                        return Err(anyhow!(AcpCancelled));
                    }
                    if let Some(error) = value.get("error") {
                        self.append_timing_diagnostic(
                            "acp_rpc_end",
                            json!({
                                "event": "acp_rpc_end",
                                "method": method,
                                "requestId": request.id,
                                "elapsedMs": diagnostic_started_at.elapsed().as_millis(),
                                "status": "error",
                                "error": error,
                                "sessionId": self.session_id,
                            }),
                        );
                        bail!("ACP `{method}` failed: {error}");
                    }
                    self.append_timing_diagnostic(
                        "acp_rpc_end",
                        json!({
                            "event": "acp_rpc_end",
                            "method": method,
                            "requestId": request.id,
                            "elapsedMs": diagnostic_started_at.elapsed().as_millis(),
                            "status": "ok",
                            "sessionId": self.session_id,
                        }),
                    );
                    return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.refresh_session_title_if_due(&title_refresh, &mut last_title_refresh_at);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.connection.cancel_pending(request.id);
                    self.append_timing_diagnostic(
                        "acp_rpc_end",
                        json!({
                            "event": "acp_rpc_end",
                            "method": method,
                            "requestId": request.id,
                            "elapsedMs": diagnostic_started_at.elapsed().as_millis(),
                            "status": "disconnected",
                            "sessionId": self.session_id,
                        }),
                    );
                    return Err(anyhow!(AcpTransportInterrupted));
                }
            }

            if self.connection.is_transport_closed() {
                self.connection.cancel_pending(request.id);
                return Err(anyhow!(AcpTransportInterrupted));
            }
            if self.connection.try_wait()?.is_some() {
                self.connection.cancel_pending(request.id);
                return Err(anyhow!(AcpTransportInterrupted));
            }
        }
    }

    fn request_prompt_with_cancel(
        &mut self,
        provider_id: &str,
        session_id: &str,
        prompt: &PromptBundle,
        prompt_turn: &AcpPromptTurnIdentity,
        restored: bool,
        title_refresh: Option<(&Utf8Path, &str, bool, Option<String>, &Value)>,
    ) -> Result<Value> {
        if self.is_prompt_cancel_requested() {
            self.observe_prompt_cancel_request()?;
            return Err(anyhow!(AcpCancelled));
        }
        if self.session_update_phase == SessionUpdatePhase::ReplayingHistory {
            self.drain_session_replay_until_quiet(session_id)?;
            self.finish_provider_history_replay(Some(session_id.to_string()))?;
            self.session_update_phase = SessionUpdatePhase::AwaitingTurnStart;
        }
        self.control.mark_running();
        let diagnostic_started_at = Instant::now();
        self.append_timing_diagnostic(
            "acp_rpc_begin",
            json!({
                "event": "acp_rpc_begin",
                "method": "session/prompt",
                "sessionId": session_id,
                "providerId": provider_id,
            }),
        );
        let _prompt_guard = self.connection.begin_prompt(session_id)?;
        let request = self.connection.begin_request(
            "session/prompt",
            session_prompt_params(
                provider_id,
                session_id,
                prompt,
                restored,
                self.runtime_policy.supports_system_prompt,
            ),
        )?;
        self.append_outbound_frame(&request.frame)?;
        let result = (|| {
            let mut cancel_started_at: Option<Instant> = None;
            let mut last_title_refresh_at = Instant::now();
            loop {
                if self.is_prompt_cancel_requested() {
                    self.observe_prompt_cancel_request()?;
                    cancel_started_at.get_or_insert_with(Instant::now);
                }
                let wait_for = cancel_started_at
                    .and_then(|started| PROMPT_CANCEL_TIMEOUT.checked_sub(started.elapsed()))
                    .map(|remaining| remaining.min(STOP_CHECK_INTERVAL))
                    .unwrap_or(STOP_CHECK_INTERVAL);
                self.drain_available_inbound()?;
                self.maybe_emit_live_timing_update(Instant::now(), "tick")?;
                match request.recv_timeout_with_session_route_watermark(wait_for) {
                    Ok(response) => {
                        let value = response.frame;
                        self.append_inbound_frame(&value)?;
                        let result = value.get("result").cloned().unwrap_or_else(|| json!({}));
                        if value.get("error").is_none()
                            && let Some(prompt_usage) =
                                AcpPromptTokenUsage::from_prompt_result(&result)
                        {
                            append_prompt_completed(
                                &self.paths.prompt_usage,
                                &prompt_turn.usage_transaction_id,
                                prompt_turn.usage_transaction_seq,
                                &current_timestamp(),
                                Some(Value::from(request.id)),
                                &prompt_usage,
                            )?;
                            self.usage.record_prompt_usage(prompt_usage);
                        }
                        self.drain_available_inbound()?;
                        if value.get("error").is_none() {
                            let watermark = response.session_route_watermark.ok_or_else(|| {
                                anyhow!(AcpPromptRouteUnavailable {
                                    reason:
                                        "session route was not registered when the response arrived",
                                })
                            })?;
                            if watermark.is_closed() {
                                return Err(anyhow!(AcpPromptRouteUnavailable {
                                    reason: "session route closed before terminal convergence",
                                }));
                            }
                            self.drain_inbound_through_route_watermark(watermark)?;
                            self.drain_prompt_terminal_until_quiet(session_id)?;
                        }
                        self.maybe_emit_live_timing_update(Instant::now(), "tick")?;
                        if let Some(error) = value.get("error") {
                            if cancel_started_at.is_some() {
                                break Err(anyhow!(AcpCancelled));
                            }
                            break Err(anyhow!("ACP `session/prompt` failed: {error}"));
                        }
                        if cancel_started_at.is_some() && !is_cancel_stop_reason(&result) {
                            break Err(anyhow!(AcpCancelled));
                        }
                        break Ok(result);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        self.refresh_session_title_if_due(
                            &title_refresh,
                            &mut last_title_refresh_at,
                        );
                        self.maybe_emit_live_timing_update(Instant::now(), "tick")?;
                        if cancel_started_at
                            .is_some_and(|started| started.elapsed() >= PROMPT_CANCEL_TIMEOUT)
                        {
                            self.connection.cancel_pending(request.id);
                            let _ = append_structured_diagnostic(
                                &self.paths.diagnostics,
                                "warn",
                                "acp.cancel-drain-timeout",
                                Some(json!({
                                    "timeoutSeconds": PROMPT_CANCEL_TIMEOUT.as_secs(),
                                    "sessionId": session_id,
                                    "requestId": request.id,
                                })),
                            );
                            break Err(anyhow!(AcpCancelDrainTimeout {
                                timeout: PROMPT_CANCEL_TIMEOUT,
                            }));
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        self.connection.cancel_pending(request.id);
                        break Err(anyhow!(AcpTransportInterrupted));
                    }
                }

                if self.connection.is_transport_closed() {
                    self.connection.cancel_pending(request.id);
                    break Err(anyhow!(AcpTransportInterrupted));
                }
                if self.connection.try_wait()?.is_some() {
                    self.connection.cancel_pending(request.id);
                    break Err(anyhow!(AcpTransportInterrupted));
                }
            }
        })();
        let status = if result.is_ok() { "ok" } else { "error" };
        let stop_reason = result
            .as_ref()
            .ok()
            .and_then(|value| value.get("stopReason"))
            .and_then(Value::as_str)
            .map(str::to_string);
        self.append_timing_diagnostic(
            "acp_rpc_end",
            json!({
                "event": "acp_rpc_end",
                "method": "session/prompt",
                "requestId": request.id,
                "elapsedMs": diagnostic_started_at.elapsed().as_millis(),
                "status": status,
                "stopReason": stop_reason,
                "sessionId": session_id,
                "providerId": provider_id,
            }),
        );
        result
    }

    fn refresh_session_title_if_due(
        &mut self,
        title_refresh: &Option<(&Utf8Path, &str, bool, Option<String>, &Value)>,
        last_title_refresh_at: &mut Instant,
    ) {
        let Some((workspace_dir, status, restored, stop_reason, capabilities)) = title_refresh
        else {
            return;
        };
        if last_title_refresh_at.elapsed() < SESSION_TITLE_REFRESH_INTERVAL {
            return;
        }
        self.refresh_session_title_and_persist(
            workspace_dir,
            status,
            *restored,
            stop_reason.clone(),
            capabilities,
        );
        *last_title_refresh_at = Instant::now();
    }

    fn handle_inbound(&mut self, value: Value) -> Result<()> {
        match value.get("method").and_then(Value::as_str) {
            Some("session/update") => self.handle_session_update(value),
            Some("session/request_permission") => self.handle_permission_request(value),
            Some("elicitation/create") => self.handle_elicitation_request(value),
            Some(method) => {
                append_diagnostic(
                    &self.paths.diagnostics,
                    "warn",
                    format!("unsupported ACP adapter request/notification `{method}`"),
                    Some(value),
                )?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn handle_session_update(&mut self, value: Value) -> Result<()> {
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut update = params.get("update").cloned().unwrap_or(params);

        if let Some(commands) = parse_available_commands(&update) {
            self.available_commands = Some(commands);
        }

        if self.session_update_phase == SessionUpdatePhase::RestoringWithoutReplay {
            if is_current_turn_content_update(&update) {
                let _ = append_structured_diagnostic(
                    &self.paths.diagnostics,
                    "warning",
                    "acp.unexpected-resume-replay",
                    Some(json!({
                        "sessionId": session_id,
                        "sessionUpdate": update.get("sessionUpdate"),
                    })),
                );
            }
            return Ok(());
        }
        if self.session_update_phase == SessionUpdatePhase::ReplayingHistory {
            if !self.runtime_policy.external_session_sync_enabled {
                return Ok(());
            }
            return self.handle_provider_history_replay(session_id, update);
        }
        if update.get("sessionUpdate").and_then(Value::as_str) == Some("user_message_chunk") {
            return Ok(());
        }
        if self.should_suppress_session_replay(&session_id, &update) {
            return Ok(());
        }

        if provider_thread_is_active(&update) {
            self.control.mark_provider_active();
        }
        if provider_thread_is_active(&update) && self.is_prompt_cancel_requested() {
            // Some providers ignore a cancel delivered before their turn is
            // active. The cancellation latch remains set, so active is the
            // synchronization point for one meaningful redelivery.
            self.send_cancel_notification_best_effort();
        }
        if self.is_prompt_cancel_requested() && is_current_turn_content_update(&update) {
            return Ok(());
        }

        // Fold raw provider samples into a stable context gauge before the event
        // enters the canonical timeline. The untouched raw frame is already in
        // acp.raw.jsonl, so transient zeroes never need to leak into UI state.
        let usage_after_compaction =
            if update.get("sessionUpdate").and_then(Value::as_str) == Some("usage_update") {
                let (used, size, cost) = crate::acp::events::extract_usage_fields(&update);
                let after = self.usage.observe_provider_usage(used, size, cost);
                self.usage.normalize_timeline_usage(&mut update);
                after
            } else {
                None
            };

        self.seq += 1;
        let event = normalize_session_update(self.seq, session_id, &update);
        self.capture_turn_file_diffs(&event)?;
        self.prompt_output.observe(&update, &event);
        self.persist_event(&event)?;
        if event.kind == "contextCompaction" {
            append_diagnostic(
                &self.paths.diagnostics,
                "info",
                format!(
                    "ACP context compaction {}",
                    event.status.as_deref().unwrap_or("updated")
                ),
                Some(json!({
                    "code": "acp.context_compaction",
                    "sessionId": event.session_id,
                    "sourceSeq": event.seq,
                    "status": event.status,
                    "contextCompaction": event.raw.as_ref().and_then(|raw| raw.get("contextCompaction")),
                })),
            )?;
        }
        if let Some(used) = usage_after_compaction {
            self.maybe_persist_context_compaction_usage(used)?;
        }
        Ok(())
    }

    fn capture_turn_file_diffs(&mut self, event: &AcpUiEvent) -> Result<()> {
        if !matches!(event.kind.as_str(), "toolCall" | "toolCallUpdate") {
            return Ok(());
        }
        let (Some(turn), Some(tool_call_id), Some(raw)) = (
            self.active_prompt_turn.as_ref(),
            event.tool_call_id.as_deref(),
            event.raw.as_ref(),
        ) else {
            return Ok(());
        };
        let branch_id = event_branch_id(event);
        let store = crate::acp::turn_files::TurnFileStore::new(
            self.paths.attempt_dir.clone(),
            self.runtime_policy.turn_file_capture,
        );
        let captured = store.capture_event_diffs(
            &turn.id,
            &turn.prompt_event_id,
            &branch_id,
            tool_call_id,
            event.seq,
            &event.timestamp,
            raw,
        )?;
        if captured > 0 {
            self.active_turn_file_branches.insert(branch_id);
        }
        Ok(())
    }

    fn finalize_turn_file_changes(&mut self, turn: &AcpPromptTurnIdentity) -> Result<()> {
        let store = crate::acp::turn_files::TurnFileStore::new(
            self.paths.attempt_dir.clone(),
            self.runtime_policy.turn_file_capture,
        );
        let finished_at = current_timestamp();
        let branches = self
            .active_turn_file_branches
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for branch_id in branches {
            let Some(change_set) = store.finalize_turn_branch(
                &turn.id,
                &turn.prompt_event_id,
                &branch_id,
                &turn.started_at,
                &finished_at,
            )?
            else {
                continue;
            };
            self.seq = self.seq.saturating_add(1);
            let event = AcpUiEvent {
                id: format!("turn-file-change-set:{}", change_set.id),
                seq: self.seq,
                timestamp: finished_at.clone(),
                kind: "fileChangeSet".to_string(),
                session_id: self.session_id.clone(),
                content: None,
                title: None,
                tool_call_id: None,
                status: Some(
                    match change_set.status {
                        crate::acp::turn_files::TurnFileChangeSetStatus::Finalized => "finalized",
                        crate::acp::turn_files::TurnFileChangeSetStatus::Partial => "partial",
                        crate::acp::turn_files::TurnFileChangeSetStatus::Capturing => "capturing",
                    }
                    .to_string(),
                ),
                // A finalized change set belongs at the end of its prompt turn.
                // Persisting the prompt start here makes reload sorting move the
                // card above the tool calls even though live state appended it last.
                started_seq: Some(self.seq),
                ended_seq: Some(self.seq),
                started_at: Some(finished_at.clone()),
                ended_at: Some(finished_at.clone()),
                timing: None,
                raw: Some(json!({
                    "changeSetId": change_set.id,
                    "turnId": change_set.turn_id,
                    "promptEventId": change_set.prompt_event_id,
                    "summary": change_set.summary,
                    "limitationCodes": change_set.limitation_codes,
                    "_meta": {
                        "conversation": { "branchId": branch_id }
                    }
                })),
            };
            self.persist_event(&event)?;
        }
        self.active_prompt_turn = None;
        self.active_turn_file_branches.clear();
        Ok(())
    }

    fn maybe_persist_context_compaction_usage(&mut self, used: u64) -> Result<()> {
        let Some(state) = self.usage.compaction.clone() else {
            return Ok(());
        };
        let Some(mut item) = self.timeline_items.get(&state.item_id).cloned() else {
            return Ok(());
        };
        self.seq = self.seq.saturating_add(1);
        item.seq = self.seq;
        item.timestamp = current_timestamp();
        item.status = Some("completed".to_string());
        item.ended_seq = state.completed_seq;
        item.ended_at = state.completed_at.clone();
        upsert_context_compaction_raw(
            &mut item,
            "completed",
            state.context_used_before,
            state.context_size,
            Some(used),
        );
        self.persist_event(&item)?;
        self.usage.compaction = None;
        append_diagnostic(
            &self.paths.diagnostics,
            "info",
            "ACP context compaction usage observed",
            Some(json!({
                "code": "acp.context_compaction_usage",
                "sourceSeq": self.seq,
                "contextUsedBefore": state.context_used_before,
                "contextUsedAfter": used,
                "contextSize": state.context_size,
            })),
        )
    }

    fn interrupt_active_context_compaction(&mut self, reason: &str) -> Result<()> {
        let Some(state) = self.usage.compaction.clone() else {
            return Ok(());
        };
        if state.completed_seq.is_some() {
            return Ok(());
        }
        let Some(mut item) = self.timeline_items.get(&state.item_id).cloned() else {
            return Ok(());
        };
        self.seq = self.seq.saturating_add(1);
        let ended_at = current_timestamp();
        item.seq = self.seq;
        item.timestamp = ended_at.clone();
        item.status = Some("interrupted".to_string());
        item.ended_seq = Some(self.seq);
        item.ended_at = Some(ended_at);
        upsert_context_compaction_raw(
            &mut item,
            "interrupted",
            state.context_used_before,
            state.context_size,
            None,
        );
        if let Some(raw) = item.raw.as_mut().and_then(Value::as_object_mut)
            && let Some(compaction) = raw
                .get_mut("contextCompaction")
                .and_then(Value::as_object_mut)
        {
            compaction.insert("reason".to_string(), Value::String(reason.to_string()));
        }
        self.persist_event(&item)?;
        self.usage.compaction = None;
        append_diagnostic(
            &self.paths.diagnostics,
            "warn",
            "ACP context compaction interrupted",
            Some(json!({
                "code": "acp.context_compaction_interrupted",
                "sourceSeq": self.seq,
                "reason": reason,
            })),
        )?;
        Ok(())
    }

    fn handle_provider_history_replay(
        &mut self,
        session_id: Option<String>,
        update: Value,
    ) -> Result<()> {
        let ReplayUpdateDecision::Import { items } = self.provider_history_replay.observe(&update)
        else {
            return Ok(());
        };
        self.persist_provider_history_imports(session_id, items)
    }

    fn finish_provider_history_replay(&mut self, session_id: Option<String>) -> Result<()> {
        if !self.runtime_policy.external_session_sync_enabled {
            return Ok(());
        }
        let ReplayUpdateDecision::Import { items } = self.provider_history_replay.finish() else {
            return Ok(());
        };
        self.persist_provider_history_imports(session_id, items)
    }

    fn persist_provider_history_imports(
        &mut self,
        session_id: Option<String>,
        items: Vec<ProviderHistoryImport>,
    ) -> Result<()> {
        for ProviderHistoryImport { update, event_id } in items {
            self.seq = self.seq.saturating_add(1);
            let mut event = normalize_session_update(self.seq, session_id.clone(), &update);
            if let Some(event_id) = event_id {
                event.id = event_id;
            }
            if event.kind == "userTextDelta" {
                event.status = Some("completed".to_string());
                event.title = Some("External user prompt".to_string());
            }
            if let Some(identity) =
                stable_session_update_item_id(event.session_id.as_deref(), &update)
            {
                self.historical_timeline_item_ids.insert(identity);
            }
            self.persist_event_inner(&event, false)?;
        }
        Ok(())
    }

    fn should_suppress_session_replay(
        &mut self,
        session_id: &Option<String>,
        update: &Value,
    ) -> bool {
        should_suppress_session_update(
            &mut self.session_update_phase,
            &self.historical_timeline_item_ids,
            &mut self.current_turn_item_ids,
            session_id.as_deref(),
            update,
        )
    }

    fn handle_permission_request(&mut self, value: Value) -> Result<()> {
        self.session_update_phase = SessionUpdatePhase::Live;
        let rpc_id = value
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow!("ACP permission request missing JSON-RPC id"))?;
        let request_id = rpc_id_to_string(&rpc_id);
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        if self.is_prompt_cancel_requested() {
            return self.send_cancelled_permission_response(rpc_id, &request_id);
        }
        self.seq += 1;
        write_pending_permission(
            &self.paths.attempt_dir,
            &request_id,
            params.clone(),
            current_timestamp(),
        )?;
        let mut event = permission_request_event(self.seq, request_id.clone(), params);
        if read_json::<PermissionResponseState>(&permission_response_file(
            &self.paths.attempt_dir,
            &request_id,
        ))
        .ok()
        .is_some_and(|response| response.cancelled)
        {
            event.status = Some("cancelled".to_string());
            event.raw.get_or_insert_with(|| json!({}))["cancelled"] = json!(true);
        }
        self.persist_event(&event)?;
        let response = wait_for_permission_response_until_cancelled(
            &self.paths.attempt_dir,
            &request_id,
            || self.is_prompt_cancel_requested(),
        )?;
        self.seq += 1;
        let decision_event = permission_decision_timeline_event(
            self.seq,
            &request_id,
            &response,
            self.timeline_items.get(&format!("permission-{request_id}")),
        );
        self.persist_event(&decision_event)?;
        let _ = remove_permission_signal_files(&self.paths.attempt_dir, &request_id);
        let result = acp_permission_response_result(response)?;
        let frame = json!({
            "jsonrpc": "2.0",
            "id": rpc_id.clone(),
            "result": result.clone(),
        });
        self.append_outbound_frame(&frame)?;
        self.connection.send_response(rpc_id, result)
    }

    fn send_cancelled_permission_response(&self, rpc_id: Value, request_id: &str) -> Result<()> {
        let result = acp_permission_response_result(PermissionResponseState {
            request_id: request_id.to_string(),
            option_id: None,
            cancelled: true,
            decided_at: current_timestamp(),
        })?;
        let frame = json!({
            "jsonrpc": "2.0",
            "id": rpc_id.clone(),
            "result": result.clone(),
        });
        self.append_outbound_frame(&frame)?;
        self.connection.send_response(rpc_id, result)
    }

    fn is_prompt_cancel_requested(&self) -> bool {
        self.control.state() == ProviderControlState::CancelRequested
            || self
                .stop_probe
                .as_ref()
                .is_some_and(RuntimeStopProbe::is_stopped)
    }

    fn observe_prompt_cancel_request(&mut self) -> Result<()> {
        if self.session_id.is_some() {
            self.send_cancel_notification_best_effort();
        }
        self.drain_available_inbound()
    }

    fn send_cancel_notification_best_effort(&mut self) {
        let Some(phase) = self.control.claim_cancel_notification() else {
            return;
        };
        let Some(session_id) = self.session_id.clone() else {
            return;
        };
        let frame = json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {
                "sessionId": session_id,
            },
        });
        if let Err(error) = self.append_outbound_frame(&frame).and_then(|_| {
            self.connection.send_notification(
                "session/cancel",
                frame.get("params").cloned().unwrap_or_else(|| json!({})),
            )
        }) {
            let _ = append_diagnostic(
                &self.paths.diagnostics,
                "warn",
                format!("failed to send ACP session/cancel notification: {error}"),
                Some(frame),
            );
        } else {
            let _ = append_structured_diagnostic(
                &self.paths.diagnostics,
                "info",
                "acp.cancel-notification-sent",
                Some(json!({
                    "sessionId": session_id,
                    "phase": match phase {
                        CancelNotificationPhase::BeforeProviderActive => "before-provider-active",
                        CancelNotificationPhase::AfterProviderActive => "after-provider-active",
                    },
                })),
            );
        }
    }

    fn handle_elicitation_request(&mut self, value: Value) -> Result<()> {
        self.session_update_phase = SessionUpdatePhase::Live;
        let rpc_id = value
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow!("ACP elicitation request missing JSON-RPC id"))?;
        let elicitation_id = format!("elicit-{}", uuid::Uuid::new_v4().simple());
        if self.is_prompt_cancel_requested() {
            return self.send_declined_elicitation_response(rpc_id, &elicitation_id);
        }
        let params = value
            .get("params")
            .cloned()
            .ok_or_else(|| anyhow!("ACP elicitation request missing params"))?;
        let request = serde_json::from_value::<
            agent_client_protocol_schema::v1::CreateElicitationRequest,
        >(params)
        .context("invalid ACP elicitation/create params")?;

        // 1. 持久化请求到 attempt dir
        write_pending_elicitation(
            &self.paths.attempt_dir,
            &PendingElicitationState {
                elicitation_id: elicitation_id.clone(),
                jsonrpc_id: rpc_id.clone(),
                request: request.clone(),
                created_at: current_timestamp(),
            },
        )?;

        // 2. 发送 UI 事件给前端
        self.seq += 1;
        let event = crate::acp::events::elicitation_request_event(
            self.seq,
            elicitation_id.clone(),
            &request,
        );
        self.persist_event(&event)?;

        // 3. 同步阻塞等待用户响应（含超时保护）
        let response = wait_for_elicitation_response_until_cancelled(
            &self.paths.attempt_dir,
            &elicitation_id,
            ELICITATION_DEFAULT_TIMEOUT,
            || self.is_prompt_cancel_requested(),
        )?;
        upsert_elicitation_response_event(
            &self.paths.attempt_dir,
            &elicitation_id,
            &response.action,
            response.content.clone(),
        )?;

        // 4. 构造 JSON-RPC response 并发送
        let result = elicitation_response_result(&response);
        let response_frame = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "result": result,
        });
        self.append_outbound_frame(&response_frame)?;
        self.connection.send_response(rpc_id, result)?;

        self.seq += 1;
        let response_event = crate::acp::events::elicitation_response_event(
            self.seq,
            elicitation_id.clone(),
            match response.action {
                crate::acp::elicitation::ElicitationAction::Accept => "accept".to_string(),
                crate::acp::elicitation::ElicitationAction::Decline => "decline".to_string(),
            },
            response.content.clone(),
        );
        self.persist_event(&response_event)?;
        let _ = remove_elicitation_signal_files(&self.paths.attempt_dir, &elicitation_id);

        Ok(())
    }

    fn send_declined_elicitation_response(
        &self,
        rpc_id: Value,
        elicitation_id: &str,
    ) -> Result<()> {
        let response = crate::acp::elicitation::ElicitationResponseState {
            elicitation_id: elicitation_id.to_string(),
            action: ElicitationAction::Decline,
            content: None,
            decided_at: current_timestamp(),
        };
        let result = elicitation_response_result(&response);
        let frame = json!({
            "jsonrpc": "2.0",
            "id": rpc_id.clone(),
            "result": result.clone(),
        });
        self.append_outbound_frame(&frame)?;
        self.connection.send_response(rpc_id, result)
    }

    fn drain_available_inbound(&mut self) -> Result<()> {
        loop {
            if self.is_prompt_cancel_requested() {
                self.send_cancel_notification_best_effort();
            }
            let value = match self.rx.as_ref().map(|receiver| receiver.try_recv()) {
                Some(Ok(value)) => value,
                Some(Err(SessionRouteTryRecvError::Empty)) | None => return Ok(()),
                Some(Err(SessionRouteTryRecvError::Disconnected)) => {
                    return Err(anyhow!(AcpTransportInterrupted));
                }
            };
            self.append_inbound_frame(&value)?;
            self.handle_inbound(value)?;
        }
    }

    fn drain_inbound_through_route_watermark(
        &mut self,
        watermark: SessionRouteWatermark,
    ) -> Result<()> {
        let receiver = self
            .rx
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!(AcpTransportInterrupted))?;
        if receiver.route_generation() != watermark.route_generation() {
            return Err(anyhow!(AcpPromptRouteUnavailable {
                reason: "session route generation changed before terminal convergence",
            }));
        }
        let started_at = Instant::now();
        let drained_frames = drain_frames_until_route_watermark(
            self.runtime_policy.prompt_terminal_route_timeout,
            || receiver.has_consumed(watermark),
            |wait_for| receiver.recv_timeout(wait_for),
            |value| {
                self.append_inbound_frame(&value)?;
                self.handle_inbound(value)
            },
        )?;
        if drained_frames > 0 {
            self.append_timing_diagnostic(
                "acp_prompt_terminal_route_drained",
                json!({
                    "event": "acp_prompt_terminal_route_drained",
                    "routeGeneration": watermark.route_generation(),
                    "targetRouteSeq": watermark.sequence(),
                    "drainedFrames": drained_frames,
                    "elapsedMs": started_at.elapsed().as_millis(),
                    "sessionId": self.session_id,
                }),
            );
        }
        Ok(())
    }

    fn drain_prompt_terminal_until_quiet(&mut self, session_id: &str) -> Result<()> {
        let receiver = self.rx.as_ref().cloned().ok_or_else(|| {
            anyhow!(AcpPromptRouteUnavailable {
                reason: "session route was unavailable during terminal quiet drain",
            })
        })?;
        let started_at = Instant::now();
        let drained_frames = drain_frames_until_quiet_with_timeout_error(
            PROMPT_TERMINAL_QUIET_PERIOD,
            self.runtime_policy.prompt_terminal_route_timeout,
            |wait_for| receiver.recv_timeout(wait_for),
            |value| {
                self.append_inbound_frame(&value)?;
                self.handle_inbound(value)
            },
            |timeout| anyhow!(AcpPromptRouteDrainTimeout { timeout }),
        )?;
        self.append_timing_diagnostic(
            "acp_prompt_terminal_quiet_drained",
            json!({
                "event": "acp_prompt_terminal_quiet_drained",
                "sessionId": session_id,
                "frames": drained_frames,
                "elapsedMs": started_at.elapsed().as_millis(),
            }),
        );
        Ok(())
    }

    fn drain_session_replay_until_quiet(&mut self, session_id: &str) -> Result<()> {
        let receiver = self
            .rx
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("ACP session replay route is unavailable"))?;
        let started_at = Instant::now();
        let drained_frames = drain_frames_until_quiet(
            SESSION_REPLAY_QUIET_PERIOD,
            SESSION_REPLAY_DRAIN_TIMEOUT,
            |wait_for| receiver.recv_timeout(wait_for),
            |value| {
                self.append_inbound_frame(&value)?;
                self.handle_inbound(value)
            },
        )?;
        self.append_timing_diagnostic(
            "acp_session_replay_drained",
            json!({
                "event": "acp_session_replay_drained",
                "sessionId": session_id,
                "frames": drained_frames,
                "elapsedMs": started_at.elapsed().as_millis(),
                "externalSessionSyncEnabled": self.runtime_policy.external_session_sync_enabled,
            }),
        );
        Ok(())
    }

    fn append_outbound_frame(&self, frame: &Value) -> Result<()> {
        append_raw_frame(
            &self.paths.raw,
            "outbound",
            frame.clone(),
            self.raw_max_size,
            self.raw_target_size,
        )
    }

    fn append_inbound_frame(&self, frame: &Value) -> Result<()> {
        append_raw_frame(
            &self.paths.raw,
            "inbound",
            frame.clone(),
            self.raw_max_size,
            self.raw_target_size,
        )
    }

    fn write_worker_ref(
        &self,
        provider_id: &str,
        workspace_dir: &Utf8Path,
        session_mode: SessionMode,
        restored: bool,
        stop_reason: Option<String>,
        reusable: bool,
    ) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP worker-ref requires a session id"))?;
        let worker_ref = WorkerRefState {
            version: VERSION.to_string(),
            provider: provider_id.to_string(),
            mode: session_mode,
            supports_open_session: true,
            supports_continue_session: reusable,
            continue_ref: reusable.then(|| {
                json!({
                    "acpSessionId": session_id,
                    "adapterId": self.connection.adapter().adapter_id.clone(),
                    "adapterDisplayName": self.connection.adapter().display_name.clone(),
                    "cwd": workspace_dir.as_str(),
                    "snapshotFile": self.paths.snapshot.as_str(),
                    "lastStopReason": stop_reason,
                    "restored": restored,
                })
            }),
            open_command: None,
        };
        validate_worker_ref_state(&worker_ref)?;
        write_json(&self.paths.attempt_dir.join("worker-ref.json"), &worker_ref)
    }

    fn write_session(
        &mut self,
        status: &str,
        restored: bool,
        stop_reason: Option<String>,
        capabilities: Value,
    ) -> Result<()> {
        self.flush_pending_live_update()?;
        self.flush_pending_timeline_patch()?;
        let metadata = self.session_metadata(status, restored, stop_reason, capabilities);
        write_session_metadata(&self.paths.snapshot, &metadata)
    }

    fn session_metadata(
        &self,
        status: &str,
        restored: bool,
        stop_reason: Option<String>,
        capabilities: Value,
    ) -> AcpSessionMetadata {
        let now = current_timestamp();
        let previous_metadata = if self.paths.snapshot.exists() {
            load_session_metadata(&self.paths.snapshot, self.session_id.clone()).ok()
        } else if self.paths.session.exists() {
            load_session_metadata(&self.paths.session, self.session_id.clone()).ok()
        } else {
            None
        };
        let created_at = previous_metadata
            .as_ref()
            .map(|session| session.created_at.clone())
            .unwrap_or_else(|| now.clone());
        let runtime_control = previous_metadata
            .as_ref()
            .and_then(|session| session.runtime_control.clone());
        let runtime_control_timeline_scan_complete = previous_metadata
            .as_ref()
            .is_some_and(|session| session.runtime_control_timeline_scan_complete);
        AcpSessionMetadata {
            adapter_id: self.connection.adapter().adapter_id.clone(),
            adapter_display_name: self.connection.adapter().display_name.clone(),
            cwd: self.paths.attempt_dir.to_string(),
            title: self.session_title.clone(),
            session_id: self.session_id.clone(),
            availability: match status {
                "cancelling" | "cancel-requested" | "closing" => AcpSessionAvailability::Closing,
                "failed" if self.session_id.is_some() => AcpSessionAvailability::Restorable,
                _ if self.session_id.is_some() => AcpSessionAvailability::Established,
                _ => AcpSessionAvailability::Unavailable,
            },
            latest_turn_status: match status {
                "completed" => AcpLatestTurnStatus::Completed,
                "cancelled" | "canceled" => AcpLatestTurnStatus::Cancelled,
                "failed" => AcpLatestTurnStatus::Failed,
                _ => previous_metadata
                    .as_ref()
                    .map(|metadata| metadata.latest_turn_status)
                    .unwrap_or_default(),
            },
            restored,
            stop_reason,
            capabilities,
            models: self.models.clone(),
            modes: self.modes.clone(),
            config_options: self.config_options.clone(),
            model_override: self.model_override.clone(),
            permission_mode_override: self.permission_mode_override.clone(),
            config_option_overrides: self.config_option_overrides.clone(),
            system_prompt_append: self.system_prompt_append.clone(),
            prompt_retry: self.prompt_retry.clone(),
            runtime_control,
            runtime_control_timeline_scan_complete,
            used_tokens: self.usage.context.confirmed_used,
            context_window_size: self.usage.context.window_size,
            total_cost_usd: self.usage.total_cost_usd,
            input_tokens: self.usage.latest_prompt.input_tokens,
            output_tokens: self.usage.latest_prompt.output_tokens,
            cached_read_tokens: self.usage.latest_prompt.cached_read_tokens,
            cached_write_tokens: self.usage.latest_prompt.cached_write_tokens,
            total_tokens: self.usage.latest_prompt.total_tokens,
            attempt_input_tokens: self.usage.attempt_totals.input_tokens,
            attempt_output_tokens: self.usage.attempt_totals.output_tokens,
            attempt_cached_read_tokens: self.usage.attempt_totals.cached_read_tokens,
            attempt_cached_write_tokens: self.usage.attempt_totals.cached_write_tokens,
            attempt_total_tokens: self.usage.attempt_totals.total_tokens,
            timing: self.session_timing_snapshot(status, &now),
            created_at,
            updated_at: now,
        }
    }

    fn session_timing_snapshot(&self, status: &str, observed_at: &str) -> Option<AcpSessionTiming> {
        let observed_epoch = parse_event_epoch_seconds(observed_at);
        let revision = Some(self.next_timing_revision());
        if is_runtime_session_active(status) {
            self.timing_state.snapshot_at_with_revision(
                true,
                observed_epoch,
                revision,
                Some(observed_at.to_string()),
            )
        } else {
            self.timing_state.terminal_snapshot_at_with_revision(
                observed_epoch,
                revision,
                Some(observed_at.to_string()),
            )
        }
    }

    fn next_timing_revision(&self) -> u64 {
        self.seq.saturating_add(1)
    }

    fn persist_event(&mut self, event: &crate::acp::events::AcpUiEvent) -> Result<()> {
        self.persist_event_inner(event, true)
    }

    fn persist_event_inner(
        &mut self,
        event: &crate::acp::events::AcpUiEvent,
        emit_live_update: bool,
    ) -> Result<()> {
        let mut timeline_item = self.timeline_item_for_event(event);
        let agent_prompt = agent_prompt_event(&timeline_item);
        let agent_result = agent_result_event(&timeline_item).filter(|result| {
            !self.timeline_items.values().any(|existing| {
                event_branch_id(existing) == event_branch_id(result)
                    && existing.kind == "textDelta"
                    && existing.content.as_deref().map(str::trim)
                        == result.content.as_deref().map(str::trim)
            })
        });
        annotate_event_branch(&mut timeline_item);
        self.timing_state.observe_event(&timeline_item);
        if let Some(timestamp) = parse_event_epoch_seconds(&timeline_item.timestamp) {
            timeline_item.timing = self
                .timing_state
                .patch_at(timestamp, timing_patch_reason(&timeline_item));
        }
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(timeline_item.clone())?;
        update_runtime_hot_timeline_items(&mut self.timeline_items, &timeline_item);
        if emit_live_update {
            self.emit_timeline_live_update(timeline_item)?;
        }
        if let Some(agent_prompt) = agent_prompt {
            self.timeline_revision = self.timeline_revision.saturating_add(1);
            self.persist_timeline_update(agent_prompt.clone())?;
            update_runtime_hot_timeline_items(&mut self.timeline_items, &agent_prompt);
            if emit_live_update {
                self.emit_timeline_live_update(agent_prompt)?;
            }
        }
        if let Some(agent_result) = agent_result {
            self.timeline_revision = self.timeline_revision.saturating_add(1);
            self.persist_timeline_update(agent_result.clone())?;
            update_runtime_hot_timeline_items(&mut self.timeline_items, &agent_result);
            if emit_live_update {
                self.emit_timeline_live_update(agent_result)?;
            }
        }
        Ok(())
    }

    fn persist_timeline_update(&mut self, item: crate::acp::events::AcpUiEvent) -> Result<()> {
        if is_streaming_timeline_update(&item) {
            let now = Instant::now();
            let should_write = self
                .last_timeline_patch_at
                .map(|last| now.duration_since(last) >= LIVE_STREAM_UPDATE_INTERVAL)
                .unwrap_or(true);
            if should_write {
                if self
                    .pending_timeline_patch
                    .as_ref()
                    .map(|(_, pending)| pending.id.as_str() != item.id.as_str())
                    .unwrap_or(false)
                {
                    self.flush_pending_timeline_patch()?;
                } else {
                    self.pending_timeline_patch = None;
                }
                self.persist_timeline_item_patch_now(self.timeline_revision, &item, now)?;
            } else {
                if self
                    .pending_timeline_patch
                    .as_ref()
                    .map(|(_, pending)| pending.id.as_str() != item.id.as_str())
                    .unwrap_or(false)
                {
                    self.flush_pending_timeline_patch()?;
                }
                self.pending_timeline_patch = Some((self.timeline_revision, item));
            }
            return Ok(());
        }

        self.flush_pending_timeline_patch()?;
        self.persist_timeline_item_patch_now(self.timeline_revision, &item, Instant::now())
    }

    fn flush_pending_timeline_patch(&mut self) -> Result<()> {
        if let Some((revision, item)) = self.pending_timeline_patch.take() {
            self.persist_timeline_item_patch_now(revision, &item, Instant::now())?;
        }
        Ok(())
    }

    fn persist_timeline_item_patch_now(
        &mut self,
        revision: u64,
        item: &crate::acp::events::AcpUiEvent,
        now: Instant,
    ) -> Result<()> {
        let branch_id = event_branch_id(item);
        let outcome = if branch_id == ROOT_BRANCH_ID {
            self.timeline_store.upsert(revision, item)?
        } else {
            let policy = self.runtime_policy.timeline_compaction;
            let attempt_dir = self.paths.attempt_dir.clone();
            self.branch_timeline_stores
                .entry(branch_id.clone())
                .or_insert(TimelineStore::open(
                    branch_timeline_path(&attempt_dir, &branch_id),
                    policy,
                )?)
                .upsert(revision, item)?
        };
        if !matches!(
            outcome,
            crate::acp::timeline::TimelineUpsertOutcome::Unchanged
        ) {
            self.last_timeline_patch_at = Some(now);
        }
        Ok(())
    }

    fn emit_timeline_live_update(&mut self, item: crate::acp::events::AcpUiEvent) -> Result<()> {
        if self.live_update.is_none() {
            return Ok(());
        }
        if is_streaming_timeline_update(&item) {
            if let Some(pending) =
                take_pending_live_update_for_stream_switch(&mut self.pending_live_update, &item)
            {
                self.emit_live_update_now(&pending, Instant::now())?;
            }
            let now = Instant::now();
            let should_emit = self
                .last_live_update_at
                .map(|last| now.duration_since(last) >= LIVE_STREAM_UPDATE_INTERVAL)
                .unwrap_or(true);
            if should_emit {
                self.pending_live_update = None;
                self.emit_live_update_now(&item, now)?;
            } else {
                self.pending_live_update = Some(item);
            }
            return Ok(());
        }
        self.flush_pending_live_update()?;
        self.emit_live_update_now(&item, Instant::now())
    }

    fn flush_pending_live_update(&mut self) -> Result<()> {
        if let Some(item) = self.pending_live_update.take() {
            self.emit_live_update_now(&item, Instant::now())?;
        }
        Ok(())
    }

    fn maybe_emit_live_timing_update(&mut self, now: Instant, reason: &'static str) -> Result<()> {
        if self.live_update.is_none() {
            return Ok(());
        }
        if self
            .last_live_timing_update_at
            .is_some_and(|last| now.duration_since(last) < LIVE_TIMING_UPDATE_INTERVAL)
        {
            return Ok(());
        }
        let timestamp = current_timestamp();
        let Some(epoch_seconds) = parse_event_epoch_seconds(&timestamp) else {
            return Ok(());
        };
        let Some(timing) = self.timing_state.patch_at_with_revision(
            epoch_seconds,
            reason,
            Some(self.next_timing_revision()),
            Some(timestamp.clone()),
        ) else {
            return Ok(());
        };
        if self
            .last_live_timing
            .as_ref()
            .is_some_and(|last| timing_patch_display_values_equal(last, &timing))
        {
            self.last_live_timing_update_at = Some(now);
            return Ok(());
        }
        let event = AcpUiEvent {
            id: format!("acp-timing-{}-{epoch_seconds}", self.seq),
            seq: self.seq,
            timestamp,
            kind: "timingUpdate".to_string(),
            session_id: self.session_id.clone(),
            content: None,
            title: None,
            tool_call_id: None,
            status: Some("active".to_string()),
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: Some(timing),
            raw: Some(json!({
                "source": "acpTiming",
                "reason": reason,
            })),
        };
        self.flush_pending_live_update()?;
        self.emit_live_update_now(&event, now)
    }

    fn emit_live_update_now(
        &mut self,
        item: &crate::acp::events::AcpUiEvent,
        now: Instant,
    ) -> Result<()> {
        if let Some(live_update) = self.live_update {
            live_update(item)?;
            self.last_live_update_at = Some(now);
            if let Some(timing) = item.timing.as_ref() {
                self.last_live_timing_update_at = Some(now);
                self.last_live_timing = Some(timing.clone());
            }
        }
        Ok(())
    }

    /// Apply a streaming delta — get-or-create the stream, append content,
    /// and stamp the item with stream identity + sequence bounds.
    fn apply_streaming_delta(
        stream: &mut Option<AcpTimelineStreamState>,
        item: &mut crate::acp::events::AcpUiEvent,
        stable_id: &str,
        source_id: Option<&str>,
        max_chars: usize,
        seq: u64,
        timestamp: &str,
    ) {
        let source_changed =
            stream
                .as_ref()
                .is_some_and(|active| match (active.source_id.as_deref(), source_id) {
                    (Some(active), Some(incoming)) => active != incoming,
                    (None, None) => false,
                    _ => true,
                });
        if source_changed {
            *stream = None;
        }
        let stream = stream.get_or_insert_with(|| AcpTimelineStreamState {
            item_id: stable_id.to_string(),
            source_id: source_id.map(str::to_string),
            started_seq: seq,
            started_at: timestamp.to_string(),
            content: String::new(),
            content_chars: 0,
        });
        if let Some(content) = item.content.as_deref() {
            if should_separate_streaming_thought_chunks(
                item.kind.as_str(),
                &stream.content,
                content,
            ) {
                append_bounded(
                    &mut stream.content,
                    &mut stream.content_chars,
                    "\n",
                    max_chars,
                );
            }
            append_bounded(
                &mut stream.content,
                &mut stream.content_chars,
                content,
                max_chars,
            );
        }
        item.id = stream.item_id.clone();
        item.content = Some(stream.content.clone());
        item.started_seq = Some(stream.started_seq);
        item.ended_seq = Some(seq);
        item.started_at = Some(stream.started_at.clone());
        item.ended_at = Some(timestamp.to_string());
    }

    /// Stamp a non-streaming event with sequence bounds and clear all streams.
    fn finalize_non_streaming_event(
        streams: (
            &mut Option<AcpTimelineStreamState>,
            &mut Option<AcpTimelineStreamState>,
            &mut Option<AcpTimelineStreamState>,
        ),
        item: &mut crate::acp::events::AcpUiEvent,
        seq: u64,
        timestamp: &str,
    ) {
        *streams.0 = None;
        *streams.1 = None;
        *streams.2 = None;
        item.started_seq = Some(item.started_seq.unwrap_or(seq));
        item.ended_seq = Some(seq);
        item.started_at = Some(
            item.started_at
                .clone()
                .unwrap_or_else(|| timestamp.to_string()),
        );
        item.ended_at = Some(timestamp.to_string());
    }

    fn apply_context_compaction_event(
        &mut self,
        item: &mut crate::acp::events::AcpUiEvent,
        seq: u64,
        timestamp: &str,
    ) {
        let status = item.status.as_deref().unwrap_or("running");
        let context_used_after = item
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/contextCompaction/contextUsedAfter"))
            .and_then(Value::as_u64);
        let interruption_reason = item
            .raw
            .as_ref()
            .and_then(|raw| raw.pointer("/contextCompaction/reason"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let mut state = if status == "running" {
            AcpContextCompactionState {
                item_id: format!("context-compaction-{seq}"),
                started_seq: seq,
                started_at: timestamp.to_string(),
                context_used_before: self.usage.context.confirmed_used,
                context_size: self.usage.context.window_size,
                completed_seq: None,
                completed_at: None,
                saw_post_completion_reset: false,
            }
        } else {
            self.usage
                .compaction
                .clone()
                .unwrap_or_else(|| AcpContextCompactionState {
                    item_id: format!("context-compaction-{seq}"),
                    started_seq: seq,
                    started_at: timestamp.to_string(),
                    context_used_before: self.usage.context.confirmed_used,
                    context_size: self.usage.context.window_size,
                    completed_seq: None,
                    completed_at: None,
                    saw_post_completion_reset: false,
                })
        };

        item.id = state.item_id.clone();
        item.started_seq = Some(state.started_seq);
        item.started_at = Some(state.started_at.clone());
        if status == "running" {
            item.ended_seq = None;
            item.ended_at = None;
        } else {
            state.completed_seq = Some(state.completed_seq.unwrap_or(seq));
            state.completed_at = Some(
                state
                    .completed_at
                    .clone()
                    .unwrap_or_else(|| timestamp.to_string()),
            );
            item.ended_seq = state.completed_seq;
            item.ended_at = state.completed_at.clone();
        }
        upsert_context_compaction_raw(
            item,
            match status {
                "running" => "started",
                "interrupted" => "interrupted",
                _ => "completed",
            },
            state.context_used_before,
            state.context_size,
            context_used_after,
        );
        if let Some(reason) = interruption_reason
            && let Some(compaction) = item
                .raw
                .as_mut()
                .and_then(|raw| raw.get_mut("contextCompaction"))
                .and_then(Value::as_object_mut)
        {
            compaction.insert("reason".to_string(), Value::String(reason));
        }
        self.usage.compaction = context_used_after.is_none().then_some(state);
    }

    fn timeline_item_for_event(
        &mut self,
        event: &crate::acp::events::AcpUiEvent,
    ) -> crate::acp::events::AcpUiEvent {
        let mut item = event.clone();
        let branch_id = event_branch_id(&item);
        let mut streams = self
            .active_timeline_streams
            .remove(&branch_id)
            .unwrap_or_default();
        let timestamp = item.timestamp.clone();
        let seq = item.seq;
        match item.kind.as_str() {
            "textDelta" => {
                let stable_id = stable_message_item_id(&item);
                let source_id = stable_message_stream_identity(&item);
                Self::apply_streaming_delta(
                    &mut streams.text,
                    &mut item,
                    &stable_id,
                    source_id.as_deref(),
                    256_000,
                    seq,
                    &timestamp,
                );
            }
            "thoughtDelta" => {
                let stable_id = stable_thought_item_id(&item);
                let source_id = stable_thought_stream_identity(&item);
                Self::apply_streaming_delta(
                    &mut streams.thought,
                    &mut item,
                    &stable_id,
                    source_id.as_deref(),
                    256_000,
                    seq,
                    &timestamp,
                );
            }
            "plan" => {
                let stable_id = stable_plan_item_id(&item);
                let source_id = stable_plan_stream_identity(&item);
                Self::apply_streaming_delta(
                    &mut streams.plan,
                    &mut item,
                    &stable_id,
                    source_id.as_deref(),
                    64_000,
                    seq,
                    &timestamp,
                );
            }
            "contextCompaction" => {
                streams.text = None;
                streams.thought = None;
                streams.plan = None;
                self.apply_context_compaction_event(&mut item, seq, &timestamp);
            }
            "usageUpdate" => {
                item.id = item
                    .session_id
                    .as_deref()
                    .map(|session_id| format!("session-usage-{session_id}"))
                    .unwrap_or_else(|| "session-usage-current".to_string());
                Self::finalize_non_streaming_event(
                    (&mut streams.text, &mut streams.thought, &mut streams.plan),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
            "toolCall" | "toolCallUpdate" => {
                if let Some(tool_call_id) = item.tool_call_id.clone() {
                    item.id = format!("tool-call-{tool_call_id}");
                }
                // Preserve input and diff evidence from earlier revisions when
                // the provider's terminal update only carries status/output.
                if let Some(prev) = self.timeline_items.get(&item.id) {
                    merge_tool_revision(&mut item, prev);
                }
                item.kind = "toolCall".to_string();
                Self::finalize_non_streaming_event(
                    (&mut streams.text, &mut streams.thought, &mut streams.plan),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
            "permissionRequest" => {
                item.id = format!("permission-{}", item.id);
                Self::finalize_non_streaming_event(
                    (&mut streams.text, &mut streams.thought, &mut streams.plan),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
            "elicitationRequest" => {
                // 不关闭 text/thought/plan 流 — elicitation 穿插在对话中
                // 不设 ended_at/ended_seq，保持"进行中"状态，等待用户响应
                item.started_seq = Some(item.started_seq.unwrap_or(seq));
                item.started_at =
                    Some(item.started_at.clone().unwrap_or_else(|| timestamp.clone()));
            }
            "elicitationResponse" => {
                // 关闭对应的 elicitationRequest
                item.started_seq = Some(seq);
                item.ended_seq = Some(seq);
                item.started_at =
                    Some(item.started_at.clone().unwrap_or_else(|| timestamp.clone()));
                item.ended_at = Some(timestamp);
            }
            _ => {
                Self::finalize_non_streaming_event(
                    (&mut streams.text, &mut streams.thought, &mut streams.plan),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
        }
        // An elicitation request is an intervention inside the current turn,
        // not a transcript boundary. Every other non-stream event closes the
        // active stream, matching restart recovery below.
        if item.kind != "elicitationRequest" {
            if item.kind != "textDelta" {
                streams.text = None;
            }
            if item.kind != "thoughtDelta" {
                streams.thought = None;
            }
            if item.kind != "plan" {
                streams.plan = None;
            }
        }
        if streams.text.is_some() || streams.thought.is_some() || streams.plan.is_some() {
            self.active_timeline_streams.insert(branch_id, streams);
        }
        item
    }

    fn shutdown(mut self) {
        debug!(adapter = %self.connection.adapter().adapter_id, "releasing ACP runtime session");
        let _ = self.flush_pending_live_update();
        let _ = self.flush_pending_timeline_patch();
        if self.connection_key.is_some() {
            AcpSessionRuntimeRegistry::shared().invalidate(&self.paths.attempt_dir);
        }
        if let Some(session_id) = self.session_id.as_deref() {
            self.connection.unregister_session_route(session_id);
        }
        AdapterConnectionManager::shared().unregister_attempt_session(&self.paths.attempt_dir);
        if self.connection_key.is_none() {
            self.connection.shutdown();
        }
        unregister_provider_control(&self.paths.attempt_dir, &self.control);
    }

    fn release_managed_session(mut self) {
        let _ = self.flush_pending_live_update();
        let _ = self.flush_pending_timeline_patch();
        if self.connection_key.is_none() {
            self.shutdown();
            return;
        }
        let (Some(connection_key), Some(session_id), Some(event_pump), Some(config_fingerprint)) = (
            self.connection_key.clone(),
            self.session_id.clone(),
            self.rx.clone(),
            self.attached_config_fingerprint,
        ) else {
            self.shutdown();
            return;
        };
        self.retain_session_route = true;
        let now = Instant::now();
        AcpSessionRuntimeRegistry::shared().release(
            AttachedSessionRuntime {
                attempt_dir: self.paths.attempt_dir.clone(),
                connection: Arc::clone(&self.connection),
                connection_generation: self.connection.generation(),
                session_id: session_id.clone(),
                event_pump,
                models: self.models.clone(),
                modes: self.modes.clone(),
                config_options: self.config_options.clone(),
                config_fingerprint,
                provider_freshness: self.provider_freshness.clone(),
                connection_key,
                external_session_sync_enabled: self.runtime_policy.external_session_sync_enabled,
                sync_required: self.sync_required,
                last_activity_at: now,
                foreground_lease_until: now + self.runtime_policy.foreground_lease_ttl,
                active: false,
            },
            self.runtime_policy,
        );
        let _ = append_diagnostic(
            &self.paths.diagnostics,
            "info",
            "ACP session runtime retained",
            Some(json!({
                "event": "acp_session_runtime_attached",
                "sessionId": session_id,
                "connectionGeneration": self.connection.generation(),
            })),
        );
    }
}

impl Drop for AcpRuntime<'_> {
    fn drop(&mut self) {
        let _ = self.flush_pending_live_update();
        let _ = self.flush_pending_timeline_patch();
        if !self.retain_session_route
            && let Some(session_id) = self.session_id.as_deref()
        {
            self.connection.unregister_session_route(session_id);
        }
        unregister_provider_control(&self.paths.attempt_dir, &self.control);
    }
}

fn upsert_context_compaction_raw(
    item: &mut crate::acp::events::AcpUiEvent,
    phase: &str,
    context_used_before: Option<u64>,
    context_size: Option<u64>,
    context_used_after: Option<u64>,
) {
    let raw = item.raw.get_or_insert_with(|| json!({}));
    if !raw.is_object() {
        *raw = json!({});
    }
    raw["contextCompaction"] = json!({
        "phase": phase,
        "detectionSource": "providerControlMessage",
        "contextUsedBefore": context_used_before,
        "contextSize": context_size,
        "contextUsedAfter": context_used_after,
    });
}

/// Merge durable input and diff fields from a previous tool-call revision when
/// a later revision does not replace them.
fn merge_tool_revision(
    new_item: &mut crate::acp::events::AcpUiEvent,
    prev: &crate::acp::events::AcpUiEvent,
) {
    if new_item.title.is_none() {
        new_item.title.clone_from(&prev.title);
    }
    let Some(previous_raw) = prev.raw.as_ref() else {
        return;
    };
    let incoming_raw = new_item.raw.get_or_insert_with(|| json!({}));
    crate::acp::events::merge_tool_revision_raw(incoming_raw, previous_raw);
}

fn is_streaming_timeline_update(event: &crate::acp::events::AcpUiEvent) -> bool {
    matches!(event.kind.as_str(), "textDelta" | "thoughtDelta" | "plan")
}

fn runtime_hot_timeline_items(
    items: Vec<crate::acp::events::AcpUiEvent>,
) -> HashMap<String, crate::acp::events::AcpUiEvent> {
    let mut hot = HashMap::new();
    for item in items {
        update_runtime_hot_timeline_items(&mut hot, &item);
    }
    hot
}

#[cfg(test)]
fn active_timeline_streams(
    items: &[crate::acp::events::AcpUiEvent],
) -> (
    Option<AcpTimelineStreamState>,
    Option<AcpTimelineStreamState>,
    Option<AcpTimelineStreamState>,
) {
    active_timeline_streams_from_refs(items.iter().collect())
}

fn active_timeline_streams_from_refs(
    mut ordered: Vec<&crate::acp::events::AcpUiEvent>,
) -> (
    Option<AcpTimelineStreamState>,
    Option<AcpTimelineStreamState>,
    Option<AcpTimelineStreamState>,
) {
    ordered.sort_by_key(|item| (item.ended_seq.unwrap_or(item.seq), item.seq));
    let mut streams = AcpBranchTimelineStreams::default();
    for item in ordered {
        let stream = || {
            let content = item.content.clone().unwrap_or_default();
            let content_chars = content.chars().count();
            AcpTimelineStreamState {
                item_id: item.id.clone(),
                source_id: match item.kind.as_str() {
                    "textDelta" => stable_message_stream_identity(item),
                    "thoughtDelta" => stable_thought_stream_identity(item),
                    "plan" => stable_plan_stream_identity(item),
                    _ => None,
                },
                started_seq: item.started_seq.unwrap_or(item.seq),
                started_at: item
                    .started_at
                    .clone()
                    .unwrap_or_else(|| item.timestamp.clone()),
                content,
                content_chars,
            }
        };
        match item.kind.as_str() {
            "textDelta" => {
                streams.text = Some(stream());
                streams.thought = None;
                streams.plan = None;
            }
            "thoughtDelta" => {
                streams.text = None;
                streams.thought = Some(stream());
                streams.plan = None;
            }
            "plan" => {
                streams.text = None;
                streams.thought = None;
                streams.plan = Some(stream());
            }
            "elicitationRequest" => {}
            _ => streams = AcpBranchTimelineStreams::default(),
        }
    }
    (streams.text, streams.thought, streams.plan)
}

fn active_timeline_streams_by_branch(
    items: &[crate::acp::events::AcpUiEvent],
) -> HashMap<String, AcpBranchTimelineStreams> {
    let mut events_by_branch = HashMap::<String, Vec<&crate::acp::events::AcpUiEvent>>::new();
    for item in items {
        events_by_branch
            .entry(event_branch_id(item))
            .or_default()
            .push(item);
    }
    events_by_branch
        .into_iter()
        .filter_map(|(branch_id, branch_items)| {
            let (text, thought, plan) = active_timeline_streams_from_refs(branch_items);
            (text.is_some() || thought.is_some() || plan.is_some()).then_some((
                branch_id,
                AcpBranchTimelineStreams {
                    text,
                    thought,
                    plan,
                },
            ))
        })
        .collect()
}

fn active_context_compaction(
    items: &[crate::acp::events::AcpUiEvent],
) -> Option<AcpContextCompactionState> {
    let item = items
        .iter()
        .filter(|item| item.kind == "contextCompaction")
        .max_by_key(|item| (item.ended_seq.unwrap_or(item.seq), item.seq))?;
    if !matches!(item.status.as_deref(), Some("running" | "completed")) {
        return None;
    }
    let raw = item.raw.as_ref()?.get("contextCompaction")?;
    if raw
        .get("contextUsedAfter")
        .and_then(Value::as_u64)
        .is_some()
    {
        return None;
    }
    let completed = item.status.as_deref() == Some("completed");
    Some(AcpContextCompactionState {
        item_id: item.id.clone(),
        started_seq: item.started_seq.unwrap_or(item.seq),
        started_at: item
            .started_at
            .clone()
            .unwrap_or_else(|| item.timestamp.clone()),
        context_used_before: raw.get("contextUsedBefore").and_then(Value::as_u64),
        context_size: raw.get("contextSize").and_then(Value::as_u64),
        completed_seq: completed.then_some(item.ended_seq.unwrap_or(item.seq)),
        completed_at: completed.then(|| {
            item.ended_at
                .clone()
                .unwrap_or_else(|| item.timestamp.clone())
        }),
        saw_post_completion_reset: false,
    })
}

fn update_runtime_hot_timeline_items(
    items: &mut HashMap<String, crate::acp::events::AcpUiEvent>,
    item: &crate::acp::events::AcpUiEvent,
) {
    if retains_runtime_timeline_context(item) {
        items.insert(item.id.clone(), item.clone());
    } else {
        items.remove(&item.id);
    }
    if item.kind == "elicitationResponse" {
        let request_id = item
            .raw
            .as_ref()
            .and_then(|raw| raw.get("elicitationId"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| item.id.trim_end_matches("-response"));
        items.remove(request_id);
    }
}

fn retains_runtime_timeline_context(item: &crate::acp::events::AcpUiEvent) -> bool {
    match item.kind.as_str() {
        "toolCall" => !is_terminal_tool_status(item.status.as_deref()),
        "contextCompaction" => {
            matches!(item.status.as_deref(), Some("running" | "completed"))
                && item
                    .raw
                    .as_ref()
                    .and_then(|raw| raw.pointer("/contextCompaction/contextUsedAfter"))
                    .and_then(Value::as_u64)
                    .is_none()
        }
        "permissionRequest" | "elicitationRequest" => item
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("pending")),
        _ => false,
    }
}

fn is_terminal_tool_status(status: Option<&str>) -> bool {
    matches!(
        status.unwrap_or_default().to_ascii_lowercase().as_str(),
        "completed" | "success" | "succeeded" | "failed" | "error" | "cancelled" | "canceled"
    )
}

fn take_pending_live_update_for_stream_switch(
    pending: &mut Option<crate::acp::events::AcpUiEvent>,
    item: &crate::acp::events::AcpUiEvent,
) -> Option<crate::acp::events::AcpUiEvent> {
    if pending
        .as_ref()
        .is_some_and(|pending| pending.id != item.id)
    {
        return pending.take();
    }
    None
}

fn initial_acp_source_seq(paths: &AcpAttemptPaths) -> u64 {
    latest_timeline_source_seq(&paths.timeline)
}

fn stable_message_item_id(event: &crate::acp::events::AcpUiEvent) -> String {
    stable_message_stream_identity(event)
        .unwrap_or_else(|| format!("assistant-message-{}", event.id))
}

fn stable_message_stream_identity(event: &crate::acp::events::AcpUiEvent) -> Option<String> {
    if let Some(item_id) = provider_history_item_id(event) {
        return Some(item_id.to_string());
    }
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("messageId"))
        .and_then(Value::as_str)
        .filter(|message_id| !message_id.trim().is_empty())
        .map(|message_id| {
            format!(
                "assistant-message-{}-{message_id}",
                branch_route_for_event(event).branch_id,
            )
        })
}

fn stable_session_update_item_id(session_id: Option<&str>, update: &Value) -> Option<String> {
    if let Some(item_id) = update
        .get("providerHistoryItemId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return Some(item_id.to_string());
    }
    match update.get("sessionUpdate").and_then(Value::as_str)? {
        "agent_message_chunk" => update
            .get("messageId")
            .and_then(Value::as_str)
            .map(|message_id| format!("assistant-message-{message_id}")),
        "agent_thought_chunk" => update
            .get("messageId")
            .and_then(Value::as_str)
            .map(|message_id| format!("assistant-thought-{message_id}")),
        "tool_call" | "tool_call_update" => update
            .get("toolCallId")
            .and_then(Value::as_str)
            .map(|tool_call_id| format!("tool-call-{tool_call_id}")),
        "plan" => session_id.map(|session_id| format!("session-plan-{session_id}")),
        _ => None,
    }
}

fn is_current_turn_content_update(update: &Value) -> bool {
    matches!(
        update.get("sessionUpdate").and_then(Value::as_str),
        Some("agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update")
    )
}

fn should_suppress_session_update(
    phase: &mut SessionUpdatePhase,
    historical_item_ids: &HashSet<String>,
    current_turn_item_ids: &mut HashSet<String>,
    session_id: Option<&str>,
    update: &Value,
) -> bool {
    let identity = stable_session_update_item_id(session_id, update);
    match *phase {
        SessionUpdatePhase::RestoringWithoutReplay | SessionUpdatePhase::ReplayingHistory => true,
        SessionUpdatePhase::AwaitingTurnStart => {
            let starts_current_turn = is_current_turn_content_update(update)
                && identity
                    .as_ref()
                    .is_none_or(|id| !historical_item_ids.contains(id));
            if !starts_current_turn {
                return true;
            }
            *phase = SessionUpdatePhase::Live;
            if let Some(identity) = identity {
                current_turn_item_ids.insert(identity);
            }
            false
        }
        SessionUpdatePhase::Live => {
            let Some(identity) = identity else {
                return false;
            };
            if historical_item_ids.contains(&identity) && !current_turn_item_ids.contains(&identity)
            {
                return true;
            }
            current_turn_item_ids.insert(identity);
            false
        }
    }
}

fn stable_thought_item_id(event: &crate::acp::events::AcpUiEvent) -> String {
    stable_thought_stream_identity(event)
        .unwrap_or_else(|| format!("assistant-thought-{}", event.id))
}

fn stable_thought_stream_identity(event: &crate::acp::events::AcpUiEvent) -> Option<String> {
    if let Some(item_id) = provider_history_item_id(event) {
        return Some(item_id.to_string());
    }
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("messageId"))
        .and_then(Value::as_str)
        .filter(|message_id| !message_id.trim().is_empty())
        .map(|message_id| {
            format!(
                "assistant-thought-{}-{message_id}",
                branch_route_for_event(event).branch_id,
            )
        })
}

fn stable_plan_item_id(event: &crate::acp::events::AcpUiEvent) -> String {
    stable_plan_stream_identity(event).unwrap_or_else(|| format!("session-plan-{}", event.id))
}

fn stable_plan_stream_identity(event: &crate::acp::events::AcpUiEvent) -> Option<String> {
    if let Some(item_id) = provider_history_item_id(event) {
        return Some(item_id.to_string());
    }
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionId"))
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.trim().is_empty())
        .map(|session_id| {
            format!(
                "session-plan-{session_id}-{}",
                branch_route_for_event(event).branch_id,
            )
        })
}

fn provider_history_item_id(event: &crate::acp::events::AcpUiEvent) -> Option<&str> {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("providerHistoryItemId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn append_bounded(target: &mut String, target_chars: &mut usize, content: &str, max_chars: usize) {
    if *target_chars >= max_chars {
        return;
    }
    let remaining = max_chars - *target_chars;
    let mut chars = content.char_indices();
    let mut end = 0;
    let mut appended = 0;
    while appended < remaining {
        let Some((index, ch)) = chars.next() else {
            target.push_str(content);
            *target_chars = target_chars.saturating_add(appended);
            return;
        };
        end = index + ch.len_utf8();
        appended += 1;
    }
    target.push_str(&content[..end]);
    *target_chars = target_chars.saturating_add(appended);
    if chars.next().is_some() {
        target.push('…');
        *target_chars = target_chars.saturating_add(1);
    }
}

fn should_separate_streaming_thought_chunks(kind: &str, accumulated: &str, incoming: &str) -> bool {
    if kind != "thoughtDelta" || accumulated.is_empty() {
        return false;
    }
    if accumulated.ends_with('\n') || incoming.starts_with('\n') {
        return false;
    }
    let accumulated = accumulated.trim_end_matches(|ch| matches!(ch, ' ' | '\t' | '\r'));
    let incoming = incoming.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\r'));
    accumulated.ends_with("**")
        && incoming.len() > 4
        && incoming.starts_with("**")
        && incoming.ends_with("**")
}

fn rpc_id_to_string(id: &Value) -> String {
    id.as_str()
        .map(str::to_string)
        .or_else(|| id.as_u64().map(|value| value.to_string()))
        .unwrap_or_else(|| id.to_string())
}

fn resolve_permission_mode(
    permission_mode: &str,
    config_options: Option<&Value>,
    modes: Option<&Value>,
) -> Result<String> {
    let permission_mode = permission_mode.trim();
    if permission_mode.is_empty() {
        return Ok(String::new());
    }

    let available = available_mode_ids(config_options, modes);
    if available.is_empty() || available.iter().any(|mode| mode == permission_mode) {
        return Ok(permission_mode.to_string());
    }

    bail!(
        "ACP permission mode `{}` is not supported by this agent; available modes: {}",
        permission_mode,
        available.join(", ")
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionModelResolution {
    Unspecified,
    Selected(String),
    Stale {
        requested: String,
        available: Vec<String>,
    },
}

fn resolve_session_model(model: &str, config_options: Option<&Value>) -> SessionModelResolution {
    let model = model.trim();
    if model.is_empty() {
        return SessionModelResolution::Unspecified;
    }
    let available = config_options
        .and_then(find_model_config_option)
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("value").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if available.is_empty() || available.iter().any(|candidate| candidate == model) {
        return SessionModelResolution::Selected(model.to_string());
    }
    SessionModelResolution::Stale {
        requested: model.to_string(),
        available,
    }
}

fn available_mode_ids(config_options: Option<&Value>, modes: Option<&Value>) -> Vec<String> {
    if let Some(options) = config_options
        .and_then(find_mode_config_option)
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
    {
        return options
            .iter()
            .filter_map(|option| option.get("value").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }

    modes
        .and_then(|value| value.get("availableModes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|mode| mode.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn find_mode_config_option(config_options: &Value) -> Option<&Value> {
    config_options.as_array().and_then(|options| {
        options.iter().find(|option| {
            option.get("id").and_then(Value::as_str) == Some("mode")
                || option.get("category").and_then(Value::as_str) == Some("mode")
        })
    })
}

fn find_model_config_option(config_options: &Value) -> Option<&Value> {
    config_options.as_array().and_then(|options| {
        options.iter().find(|option| {
            option.get("id").and_then(Value::as_str) == Some("model")
                || option.get("category").and_then(Value::as_str) == Some("model")
        })
    })
}

fn has_mode_config_option(config_options: Option<&Value>) -> bool {
    config_options.and_then(find_mode_config_option).is_some()
}

fn has_model_config_option(config_options: Option<&Value>) -> bool {
    config_options.and_then(find_model_config_option).is_some()
}

fn set_config_option_current_value(
    config_options: Option<&mut Value>,
    config_id: &str,
    value: &str,
) {
    let Some(options) = config_options.and_then(Value::as_array_mut) else {
        return;
    };
    if let Some(option) = options
        .iter_mut()
        .find(|option| option.get("id").and_then(Value::as_str) == Some(config_id))
    {
        if let Some(object) = option.as_object_mut() {
            object.insert("currentValue".to_string(), Value::String(value.to_string()));
        }
    }
}

fn permission_decision_timeline_event(
    seq: u64,
    request_id: &str,
    response: &PermissionResponseState,
    existing: Option<&AcpUiEvent>,
) -> AcpUiEvent {
    let mut raw = existing
        .and_then(|event| event.raw.clone())
        .unwrap_or_else(|| json!({}));
    if !raw.is_object() {
        raw = json!({});
    }
    if let Some(object) = raw.as_object_mut() {
        object.insert("requestId".to_string(), json!(request_id));
        if response.cancelled {
            object.insert("cancelled".to_string(), json!(true));
            object.remove("optionId");
        } else {
            object.insert("optionId".to_string(), json!(response.option_id.clone()));
            object.remove("cancelled");
        }
    }

    AcpUiEvent {
        id: request_id.to_string(),
        seq,
        timestamp: current_timestamp(),
        kind: "permissionRequest".to_string(),
        session_id: existing.and_then(|event| event.session_id.clone()),
        content: None,
        title: existing
            .and_then(|event| event.title.clone())
            .or_else(|| Some("Permission answered".to_string())),
        tool_call_id: existing.and_then(|event| event.tool_call_id.clone()),
        status: Some(if response.cancelled {
            "cancelled".to_string()
        } else {
            "selected".to_string()
        }),
        started_seq: existing.and_then(|event| event.started_seq),
        ended_seq: None,
        started_at: existing.and_then(|event| event.started_at.clone()),
        ended_at: None,
        timing: None,
        raw: Some(raw),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use serde_json::{Value, json};

    use crate::domain::TurnControlMode;

    use super::{
        ACP_UNIDENTIFIED_AGENT_OUTPUT_CODE, AcpCancelDrainTimeout, AcpContextCompactionState,
        AcpOutputPolicy, AcpPromptFailure, AcpPromptOutputAccumulator, AcpPromptRetryState,
        AcpPromptRouteDrainTimeout, AcpPromptRouteUnavailable, AcpPromptTokenUsage, AcpRuntime,
        AcpRuntimePolicy, AcpUsageState, AttachedSessionReusePlan, CancelNotificationPhase,
        DOCTOR_DIAGNOSTIC_TARGET_SIZE, NESTED_AGENT_TRANSCRIPT_CAPABILITY, PriorAttemptMetrics,
        PromptActivity, PromptBundle, PromptVisibility, ProviderFreshnessBaseline,
        RuntimeStopProbe, SessionModelResolution, SessionRestoreCapabilities, SessionRestoreIntent,
        SessionRestoreMethod, SessionRestorePlan, SessionRestorePlanError, SessionUpdatePhase,
        active_context_compaction, active_timeline_streams, active_timeline_streams_by_branch,
        append_bounded, attached_sync_required, cancel_attempt_prompt,
        canonical_prompt_event_identity, cleanup_doctor_acp_dir_after_success,
        drain_frames_until_quiet, drain_frames_until_quiet_with_timeout_error,
        drain_frames_until_route_watermark, evaluate_provider_revision, initialize_params,
        is_pending_retry_prompt_event, is_transport_interruption, latest_visible_turn_id,
        merge_tool_revision, next_prompt_retry_attempt, permission_decision_timeline_event,
        plan_attached_session_reuse, plan_session_restore, prompt_activity,
        prompt_cancellation_outcome, prompt_usage_transaction_id, provider_thread_is_active,
        register_provider_control, request_prompt_cancel, resolve_permission_mode,
        resolve_session_model, retain_bounded_doctor_acp_failure_bundle,
        runtime_hot_timeline_items, session_config_fingerprint, session_load_params,
        session_new_params, session_prompt_params, session_prompt_text, session_resume_params,
        settle_prompt_event, should_suppress_session_update, stable_message_item_id,
        take_pending_live_update_for_stream_switch, unidentified_agent_output_failure,
        unregister_provider_control,
    };

    fn non_runtime_control_test_prompt(prompt_id: &str) -> PromptBundle {
        PromptBundle {
            system_prompt: String::new(),
            user_prompt: "clarify".to_string(),
            display_text: None,
            quotes: Vec::new(),
            prompt_id: Some(prompt_id.to_string()),
            visibility: PromptVisibility::Visible,
            hidden_reason: None,
            turn_control_mode: TurnControlMode::NonRuntimeControlled,
            runtime_control_intent: crate::provider::RuntimeControlIntent::Unchanged,
            runtime_control_transition_id: None,
            runtime_control_source_transition_id: None,
            runtime_control_transition_cause: None,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        }
    }

    #[test]
    fn non_runtime_prompt_stays_unchanged_after_runtime_interruption() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        crate::acp::control::mark_runtime_interrupted(&attempt_dir).unwrap();
        let mut prompt = non_runtime_control_test_prompt("prompt-1");

        super::prepare_runtime_control_prompt(&attempt_dir, &mut prompt).unwrap();

        assert_eq!(prompt.user_prompt, "clarify");
        assert!(prompt.runtime_control_transition_id.is_none());
        assert!(prompt.runtime_control_transition_cause.is_none());
    }

    #[test]
    fn only_explicit_resume_intent_prepares_runtime_control_transition() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        crate::acp::control::mark_runtime_interrupted(&attempt_dir).unwrap();
        let mut prompt = non_runtime_control_test_prompt("prompt-1");
        prompt.turn_control_mode = TurnControlMode::RuntimeControlled;

        super::prepare_runtime_control_prompt(&attempt_dir, &mut prompt).unwrap();
        assert!(prompt.runtime_control_transition_id.is_none());
        assert!(prompt.runtime_control_transition_cause.is_none());

        prompt.runtime_control_intent = crate::provider::RuntimeControlIntent::Resume;
        super::prepare_runtime_control_prompt(&attempt_dir, &mut prompt).unwrap();
        assert!(prompt.runtime_control_transition_id.is_some());
        assert_eq!(
            prompt.runtime_control_transition_cause,
            Some(crate::domain::TurnControlTransitionCause::WorkflowContinued)
        );
    }

    use crate::acp::{
        connection::AcpConnectionUnavailable,
        events::{
            AcpAttemptPaths, AcpTimingState, AcpUiEvent, append_timeline_patch,
            load_timeline_items, normalize_session_update, user_prompt_event,
        },
        permission::PermissionResponseState,
    };
    use crate::config::RuntimeConfig;
    use crate::provider::prepare_acp_mcp_servers;
    use crate::runtime_error::{RecoveryMode, normalize_runtime_error};

    #[test]
    fn initialize_requests_nested_agent_transcripts_at_the_adapter_boundary() {
        let params = initialize_params();

        assert_eq!(
            params
                .pointer("/clientCapabilities/_meta")
                .and_then(|meta| meta.get(NESTED_AGENT_TRANSCRIPT_CAPABILITY))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(
            params
                .pointer("/clientCapabilities/elicitation/form")
                .is_some()
        );
    }

    #[test]
    fn hidden_repair_reuses_the_latest_visible_prompt_turn_identity() {
        let event = |id: &str, seq: u64, prompt_id: &str, hidden: bool| AcpUiEvent {
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("prompt".to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: Some(seq),
            ended_seq: Some(seq),
            started_at: Some(format!("{seq}Z")),
            ended_at: Some(format!("{seq}Z")),
            timing: None,
            raw: Some(json!({
                "source": "goldBandPrompt",
                "promptId": prompt_id,
                "hiddenFromChat": hidden,
            })),
        };
        let first = event("visible-1", 1, "turn-1", false);
        let hidden = event("repair-2", 2, "repair-2", true);

        assert_eq!(
            latest_visible_turn_id([&first, &hidden]),
            Some("turn-1".to_string()),
        );
    }

    #[test]
    fn prompt_retry_counter_survives_terminal_session_snapshot_state() {
        let prior = AcpPromptRetryState {
            prompt_id: "runtime-turn-001".to_string(),
            retry_attempt: 1,
            prompt_event_id: Some("gold-band-user-prompt-7".to_string()),
            prompt_event_seq: Some(7),
            prompt_event_timestamp: Some("100Z".to_string()),
            hidden_from_chat: false,
        };
        assert_eq!(
            next_prompt_retry_attempt(Some(&prior), "runtime-turn-001", false),
            2
        );
        assert_eq!(
            next_prompt_retry_attempt(Some(&prior), "runtime-turn-002", false),
            0
        );
        assert_eq!(
            canonical_prompt_event_identity(Some(&prior), "runtime-turn-001", false, 12, "200Z",),
            ("gold-band-user-prompt-7".to_string(), 7, "100Z".to_string())
        );
    }

    #[test]
    fn hidden_repair_cannot_reuse_visible_prompt_timeline_identity() {
        let visible = AcpPromptRetryState {
            prompt_id: "runtime-turn-001".to_string(),
            retry_attempt: 1,
            prompt_event_id: Some("gold-band-user-prompt-7".to_string()),
            prompt_event_seq: Some(7),
            prompt_event_timestamp: Some("100Z".to_string()),
            hidden_from_chat: false,
        };

        assert_eq!(
            next_prompt_retry_attempt(Some(&visible), "runtime-turn-001", true),
            0
        );
        assert_eq!(
            canonical_prompt_event_identity(Some(&visible), "runtime-turn-001", true, 12, "200Z",),
            (
                "gold-band-user-prompt-12".to_string(),
                12,
                "200Z".to_string()
            )
        );
    }

    #[test]
    fn provider_retry_usage_transactions_are_distinct_from_timeline_identity() {
        let prompt_event_id = "gold-band-user-prompt-7";

        let first = prompt_usage_transaction_id(prompt_event_id, 0, 7);
        let retry = prompt_usage_transaction_id(prompt_event_id, 1, 12);

        assert_ne!(first, retry);
        assert!(first.starts_with(prompt_event_id));
        assert!(retry.starts_with(prompt_event_id));
    }

    #[test]
    fn only_processing_retry_prompt_is_recovered_for_early_stop_settlement() {
        let event = |status: &str, retry_attempt: Option<u64>| AcpUiEvent {
            id: "gold-band-user-prompt-7".to_string(),
            seq: 7,
            timestamp: "100Z".to_string(),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("hi".to_string()),
            title: Some("User prompt".to_string()),
            tool_call_id: None,
            status: Some(status.to_string()),
            started_seq: Some(7),
            ended_seq: Some(20),
            started_at: Some("100Z".to_string()),
            ended_at: Some("120Z".to_string()),
            timing: None,
            raw: Some(json!({
                "source": "goldBandPrompt",
                "retry": retry_attempt.map(|attempt| json!({
                    "attempt": attempt,
                    "maxAttempts": 3,
                })),
            })),
        };

        assert!(is_pending_retry_prompt_event(&event("processing", Some(2))));
        assert!(!is_pending_retry_prompt_event(&event("completed", Some(2))));
        assert!(!is_pending_retry_prompt_event(&event("cancelled", Some(2))));
        assert!(!is_pending_retry_prompt_event(&event("processing", None)));
    }

    #[test]
    fn attempt_cancel_settles_retry_prompt_even_without_an_active_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8Path::from_path(dir.path()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.to_path_buf());
        let mut prompt = user_prompt_event(
            1,
            "session-1".to_string(),
            "hi".to_string(),
            Some("prompt-1".to_string()),
            false,
            Vec::new(),
        );
        prompt.status = Some("processing".to_string());
        prompt.started_seq = Some(1);
        prompt.ended_seq = Some(22);
        prompt.raw.as_mut().unwrap()["retry"] = json!({
            "attempt": 2,
            "maxAttempts": 3,
        });
        append_timeline_patch(&paths.timeline, prompt.id.clone(), 22, &prompt).unwrap();

        assert!(!cancel_attempt_prompt(attempt_dir).unwrap());

        let settled = load_timeline_items(&paths.timeline)
            .unwrap()
            .into_iter()
            .find(|event| event.id == prompt.id)
            .expect("settled prompt");
        assert_eq!(settled.status.as_deref(), Some("cancelled"));
        assert_eq!(settled.ended_seq, Some(23));
        assert_eq!(settled.raw.as_ref().unwrap()["retry"]["attempt"], 2);
        assert_eq!(settled.raw.as_ref().unwrap()["cancelled"], true);
    }

    #[test]
    fn terminal_prompt_settlement_preserves_retry_after_event_leaves_hot_cache() {
        let prompt_event = AcpUiEvent {
            id: "gold-band-user-prompt-12".to_string(),
            seq: 12,
            timestamp: "2026-08-06T00:00:00Z".to_string(),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("hi".to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: Some(12),
            ended_seq: Some(12),
            started_at: Some("2026-08-06T00:00:00Z".to_string()),
            ended_at: Some("2026-08-06T00:00:00Z".to_string()),
            timing: None,
            raw: Some(json!({ "retry": { "attempt": 1, "maxAttempts": 3 } })),
        };
        let hot = runtime_hot_timeline_items(vec![prompt_event.clone()]);
        assert!(!hot.contains_key(&prompt_event.id));

        let cancelled = settle_prompt_event(prompt_event.clone(), "cancelled", 13, None);
        assert_eq!(cancelled.status.as_deref(), Some("cancelled"));
        assert_eq!(cancelled.raw.as_ref().unwrap()["retry"]["attempt"], 1);
        assert_eq!(cancelled.raw.as_ref().unwrap()["cancelled"], true);

        let failure = AcpPromptFailure {
            code: "provider_error".to_string(),
            message: "provider failed".to_string(),
            details: None,
            raw: json!({}),
        };
        let failed = settle_prompt_event(prompt_event, "failed", 13, Some(&failure));
        assert_eq!(failed.status.as_deref(), Some("failed"));
        assert_eq!(failed.raw.as_ref().unwrap()["retry"]["attempt"], 1);
        assert_eq!(
            failed.raw.as_ref().unwrap()["terminalFailure"]["code"],
            "provider_error"
        );
    }

    #[test]
    fn attempt_token_totals_accumulate_prompt_turns_even_when_latest_input_drops() {
        let mut state = AcpUsageState::default();

        state.record_prompt_usage(
            AcpPromptTokenUsage::from_prompt_result(&json!({
                "usage": {
                    "inputTokens": 9_057,
                    "outputTokens": 15,
                    "cachedReadTokens": 7_680,
                    "totalTokens": 16_752
                }
            }))
            .unwrap(),
        );
        state.record_prompt_usage(
            AcpPromptTokenUsage::from_prompt_result(&json!({
                "usage": {
                    "inputTokens": 7_453,
                    "outputTokens": 315,
                    "cachedReadTokens": 16_896,
                    "totalTokens": 24_664
                }
            }))
            .unwrap(),
        );

        assert_eq!(state.latest_prompt.input_tokens, Some(7_453));
        assert_eq!(state.latest_prompt.total_tokens, Some(24_664));
        assert_eq!(state.attempt_totals.input_tokens, Some(16_510));
        assert_eq!(state.attempt_totals.output_tokens, Some(330));
        assert_eq!(state.attempt_totals.cached_read_tokens, Some(24_576));
        assert_eq!(state.attempt_totals.total_tokens, Some(41_416));
    }

    #[test]
    fn restored_attempt_totals_continue_accumulating_after_runtime_recreation() {
        let mut state = AcpUsageState::from_prior(
            PriorAttemptMetrics {
                attempt_input_tokens: Some(9_057),
                attempt_output_tokens: Some(15),
                attempt_cached_read_tokens: Some(7_680),
                attempt_total_tokens: Some(16_752),
                ..Default::default()
            },
            None,
        );

        state.record_prompt_usage(
            AcpPromptTokenUsage::from_prompt_result(&json!({
                "usage": {
                    "inputTokens": 7_453,
                    "outputTokens": 315,
                    "cachedReadTokens": 16_896,
                    "totalTokens": 24_664
                }
            }))
            .unwrap(),
        );

        assert_eq!(state.attempt_totals.input_tokens, Some(16_510));
        assert_eq!(state.attempt_totals.output_tokens, Some(330));
        assert_eq!(state.attempt_totals.cached_read_tokens, Some(24_576));
        assert_eq!(state.attempt_totals.total_tokens, Some(41_416));
    }

    #[test]
    fn prompt_usage_derives_total_when_provider_omits_total_tokens() {
        let mut state = AcpUsageState::default();
        state.record_prompt_usage(
            AcpPromptTokenUsage::from_prompt_result(&json!({
                "usage": {
                    "inputTokens": 100,
                    "outputTokens": 20,
                    "cachedReadTokens": 300,
                    "cachedWriteTokens": 40
                }
            }))
            .unwrap(),
        );

        assert_eq!(state.attempt_totals.total_tokens, Some(460));
    }

    #[test]
    fn prompt_terminal_quiet_drain_observes_frame_after_rpc_response() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender_keepalive = sender.clone();
        let producer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            sender
                .send(json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "502 Bad Gateway" }
                }))
                .unwrap();
        });
        let mut observed = Vec::new();

        let drained = drain_frames_until_quiet_with_timeout_error(
            Duration::from_millis(30),
            Duration::from_millis(200),
            |wait_for| receiver.recv_timeout(wait_for),
            |update| {
                observed.push(update);
                Ok(())
            },
            |timeout| anyhow::anyhow!(AcpPromptRouteDrainTimeout { timeout }),
        )
        .unwrap();

        producer.join().unwrap();
        drop(sender_keepalive);
        assert_eq!(drained, 1);
        assert_eq!(
            observed[0].pointer("/content/text"),
            Some(&json!("502 Bad Gateway"))
        );
    }

    #[test]
    fn prompt_terminal_watermark_drain_does_not_delay_when_already_consumed() {
        let drained = drain_frames_until_route_watermark(
            Duration::from_secs(5),
            || true,
            |_| panic!("an already-consumed watermark must not wait for another frame"),
            |_| Ok(()),
        )
        .unwrap();

        assert_eq!(drained, 0);
    }

    #[test]
    fn prompt_terminal_watermark_timeout_cannot_be_classified_as_success() {
        let error = drain_frames_until_route_watermark(
            Duration::from_millis(1),
            || false,
            |_| Err(RecvTimeoutError::Timeout),
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(error.downcast_ref::<AcpPromptRouteDrainTimeout>().is_some());
    }

    #[test]
    fn prompt_terminal_quiet_drain_is_bounded_when_frames_never_become_quiet() {
        let error = drain_frames_until_quiet_with_timeout_error(
            Duration::from_millis(10),
            Duration::from_millis(1),
            |_| Err(RecvTimeoutError::Timeout),
            |_| Ok(()),
            |timeout| anyhow::anyhow!(AcpPromptRouteDrainTimeout { timeout }),
        )
        .unwrap_err();

        assert!(error.downcast_ref::<AcpPromptRouteDrainTimeout>().is_some());
    }

    #[test]
    fn prompt_terminal_route_convergence_errors_are_auto_recoverable() {
        for error in [
            anyhow::anyhow!(AcpPromptRouteDrainTimeout {
                timeout: Duration::from_secs(5),
            }),
            anyhow::anyhow!(AcpPromptRouteUnavailable {
                reason: "route replaced",
            }),
        ] {
            let info = normalize_runtime_error(&error);
            assert_eq!(info.recovery, RecoveryMode::Auto);
            assert_eq!(info.code_str(), "runtime.transport-interrupted");
        }
    }

    #[test]
    fn external_session_sync_policy_is_agent_opt_in() {
        let default_policy = AcpRuntimePolicy::default();
        assert!(!default_policy.external_session_sync_enabled);

        let enabled = default_policy.with_external_session_sync_enabled(true);
        assert!(enabled.external_session_sync_enabled);
        assert_eq!(enabled.session_idle_ttl, default_policy.session_idle_ttl);
        assert_eq!(
            enabled.max_idle_session_runtimes,
            default_policy.max_idle_session_runtimes
        );
    }

    #[test]
    fn prompt_terminal_route_timeout_comes_from_runtime_config() {
        let mut config = RuntimeConfig::default();
        config.acp_prompt_terminal_route_timeout_ms = 2_750;

        let policy = AcpRuntimePolicy::from(&config);

        assert_eq!(
            policy.prompt_terminal_route_timeout,
            Duration::from_millis(2_750)
        );
    }

    #[test]
    fn first_enable_sync_still_loads_when_session_list_would_timeout() {
        let sync_required = attached_sync_required(false, false, true);
        assert!(sync_required);

        let plan = plan_attached_session_reuse(
            false,
            sync_required,
            true,
            &ProviderFreshnessBaseline::Unknown,
        );

        assert_eq!(
            plan,
            AttachedSessionReusePlan::Reload("external-session-sync-required")
        );
        assert_ne!(plan, AttachedSessionReusePlan::ProbeFreshness);
    }

    #[test]
    fn attached_session_reuse_only_probes_freshness_without_required_sync() {
        assert_eq!(
            plan_attached_session_reuse(false, false, true, &ProviderFreshnessBaseline::Unknown,),
            AttachedSessionReusePlan::ProbeFreshness
        );
        assert_eq!(
            plan_attached_session_reuse(false, false, false, &ProviderFreshnessBaseline::Unknown,),
            AttachedSessionReusePlan::Reuse
        );
        assert_eq!(
            plan_attached_session_reuse(true, false, false, &ProviderFreshnessBaseline::Unknown,),
            AttachedSessionReusePlan::Reload("session-config-changed")
        );
    }

    #[test]
    fn provider_control_exposes_prompt_activity_phases() {
        let attempt_dir = camino::Utf8Path::new("test/provider-control-activity");
        let control = register_provider_control(attempt_dir);
        assert_eq!(prompt_activity(attempt_dir), Some(PromptActivity::Starting));

        control.mark_accepted();
        assert_eq!(prompt_activity(attempt_dir), Some(PromptActivity::Accepted));

        control.mark_running();
        assert_eq!(prompt_activity(attempt_dir), Some(PromptActivity::Running));

        assert!(request_prompt_cancel(attempt_dir));
        assert_eq!(
            prompt_activity(attempt_dir),
            Some(PromptActivity::CancelRequested)
        );

        unregister_provider_control(attempt_dir, &control);
        assert_eq!(prompt_activity(attempt_dir), None);
    }

    #[test]
    fn provider_control_redelivers_cancel_once_after_provider_becomes_active() {
        let control = super::ProviderControl::new();
        assert!(control.request_prompt_cancel());
        assert_eq!(
            control.claim_cancel_notification(),
            Some(CancelNotificationPhase::BeforeProviderActive)
        );
        assert_eq!(control.claim_cancel_notification(), None);

        control.mark_provider_active();
        assert_eq!(
            control.claim_cancel_notification(),
            Some(CancelNotificationPhase::AfterProviderActive)
        );
        assert_eq!(control.claim_cancel_notification(), None);
        control.mark_provider_active();
        assert_eq!(control.claim_cancel_notification(), None);
    }

    #[test]
    fn cancel_drain_timeout_remains_a_cancellation_terminal_outcome() {
        let error = anyhow::anyhow!(AcpCancelDrainTimeout {
            timeout: std::time::Duration::from_secs(30),
        });

        let outcome = prompt_cancellation_outcome(false, Some(&error));

        assert!(outcome.observed);
        assert!(outcome.drain_timed_out);
    }

    #[test]
    fn codex_active_thread_status_is_the_cancel_redelivery_boundary() {
        assert!(provider_thread_is_active(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": { "codex": { "threadStatus": { "type": "active" } } }
        })));
        assert!(!provider_thread_is_active(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": { "codex": { "threadStatus": { "type": "systemError" } } }
        })));
    }

    #[test]
    fn provider_control_exposes_activity_below_task_root() {
        let task_dir = camino::Utf8Path::new("test/provider-control-task");
        let attempt_dir =
            task_dir.join("runs/run-001/rounds/round-001/nodes/direct/attempts/attempt-001");
        let unrelated_dir =
            camino::Utf8Path::new("test/provider-control-other/runs/run-001/attempt-001");
        let control = register_provider_control(&attempt_dir);
        let unrelated = register_provider_control(unrelated_dir);

        control.mark_running();
        assert_eq!(
            super::prompt_activity_under(task_dir),
            Some(PromptActivity::Running)
        );

        assert!(request_prompt_cancel(&attempt_dir));
        assert_eq!(
            super::prompt_activity_under(task_dir),
            Some(PromptActivity::CancelRequested)
        );

        unregister_provider_control(&attempt_dir, &control);
        unregister_provider_control(unrelated_dir, &unrelated);
        assert_eq!(super::prompt_activity_under(task_dir), None);
    }

    #[test]
    fn restored_session_suppresses_replay_until_new_turn_identity_arrives() {
        let historical = HashSet::from([
            "assistant-message-old".to_string(),
            "assistant-thought-old".to_string(),
        ]);
        let mut current = HashSet::new();
        let mut phase = SessionUpdatePhase::ReplayingHistory;
        let old_message = json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "old",
            "content": { "type": "text", "text": "old answer" }
        });
        assert!(should_suppress_session_update(
            &mut phase,
            &historical,
            &mut current,
            Some("session-1"),
            &old_message,
        ));

        phase = SessionUpdatePhase::AwaitingTurnStart;
        assert!(should_suppress_session_update(
            &mut phase,
            &historical,
            &mut current,
            Some("session-1"),
            &old_message,
        ));
        let new_thought = json!({
            "sessionUpdate": "agent_thought_chunk",
            "messageId": "new",
            "content": { "type": "text", "text": "thinking" }
        });
        assert!(!should_suppress_session_update(
            &mut phase,
            &historical,
            &mut current,
            Some("session-1"),
            &new_thought,
        ));
        assert_eq!(phase, SessionUpdatePhase::Live);
        assert!(current.contains("assistant-thought-new"));

        assert!(should_suppress_session_update(
            &mut phase,
            &historical,
            &mut current,
            Some("session-1"),
            &old_message,
        ));
    }

    #[test]
    fn resume_restore_suppresses_unexpected_content_until_prompt_turn_starts() {
        let historical = HashSet::new();
        let mut current = HashSet::new();
        let mut phase = SessionUpdatePhase::RestoringWithoutReplay;
        let unexpected = json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "resume-replay",
            "content": { "type": "text", "text": "old answer" }
        });

        assert!(should_suppress_session_update(
            &mut phase,
            &historical,
            &mut current,
            Some("session-1"),
            &unexpected,
        ));
        assert_eq!(phase, SessionUpdatePhase::RestoringWithoutReplay);
        assert!(current.is_empty());

        phase = SessionUpdatePhase::AwaitingTurnStart;
        let current_turn = json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "current",
            "content": { "type": "text", "text": "new answer" }
        });
        assert!(!should_suppress_session_update(
            &mut phase,
            &historical,
            &mut current,
            Some("session-1"),
            &current_turn,
        ));
        assert_eq!(phase, SessionUpdatePhase::Live);
    }

    #[test]
    fn load_response_does_not_end_replay_before_delayed_agent_chunks_arrive() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender_keepalive = sender.clone();
        let producer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(15));
            sender
                .send(json!({
                    "sessionUpdate": "agent_message_chunk",
                    "messageId": "synthetic-after-load-response",
                    "content": { "type": "text", "text": "No response requested." }
                }))
                .unwrap();
        });
        let mut phase = SessionUpdatePhase::ReplayingHistory;
        let historical = HashSet::new();
        let mut current = HashSet::new();
        let mut suppressed = Vec::new();

        let drained = drain_frames_until_quiet(
            std::time::Duration::from_millis(40),
            std::time::Duration::from_secs(1),
            |wait_for| receiver.recv_timeout(wait_for),
            |update| {
                suppressed.push(should_suppress_session_update(
                    &mut phase,
                    &historical,
                    &mut current,
                    Some("session-1"),
                    &update,
                ));
                Ok(())
            },
        )
        .unwrap();

        producer.join().unwrap();
        drop(sender_keepalive);
        assert_eq!(drained, 1);
        assert_eq!(suppressed, vec![true]);
        assert_eq!(phase, SessionUpdatePhase::ReplayingHistory);
        assert!(current.is_empty());
    }

    #[test]
    fn draining_and_closed_connections_are_transport_interruptions() {
        assert!(is_transport_interruption(&anyhow::anyhow!(
            AcpConnectionUnavailable::Draining
        )));
        assert!(is_transport_interruption(&anyhow::anyhow!(
            AcpConnectionUnavailable::Closed
        )));
        assert!(!is_transport_interruption(&anyhow::anyhow!(
            "provider rejected prompt"
        )));
    }

    #[test]
    fn session_config_fingerprint_normalizes_mcp_order_and_object_keys() {
        let cwd = camino::Utf8Path::new("D:/repo");
        let first = vec![
            json!({ "name": "b", "env": { "B": "2", "A": "1" } }),
            json!({ "name": "a", "command": "server" }),
        ];
        let second = vec![
            json!({ "command": "server", "name": "a" }),
            json!({ "env": { "A": "1", "B": "2" }, "name": "b" }),
        ];
        assert_eq!(
            session_config_fingerprint("claude-acp", cwd, "stable-a", &first).unwrap(),
            session_config_fingerprint("claude-acp", cwd, "stable-b", &second).unwrap()
        );
    }

    #[test]
    fn session_config_fingerprint_changes_when_mcp_changes() {
        let cwd = camino::Utf8Path::new("D:/repo");
        let first = vec![json!({ "name": "server", "command": "one" })];
        let second = vec![json!({ "name": "server", "command": "two" })];
        assert_ne!(
            session_config_fingerprint("claude-acp", cwd, "", &first).unwrap(),
            session_config_fingerprint("claude-acp", cwd, "", &second).unwrap()
        );
    }

    #[test]
    fn provider_revision_matrix_reuses_equal_and_reloads_changed_or_recovered() {
        let (_, equal_reason) = evaluate_provider_revision(
            &ProviderFreshnessBaseline::Known("rev-1".to_string()),
            Some("rev-1".to_string()),
        );
        assert_eq!(equal_reason, None);

        let (_, changed_reason) = evaluate_provider_revision(
            &ProviderFreshnessBaseline::Known("rev-1".to_string()),
            Some("rev-2".to_string()),
        );
        assert_eq!(changed_reason, Some("provider-revision-changed"));

        let (_, recovered_reason) = evaluate_provider_revision(
            &ProviderFreshnessBaseline::Unknown,
            Some("rev-2".to_string()),
        );
        assert_eq!(recovered_reason, Some("provider-revision-baseline-unknown"));

        let (unsupported, no_reason) = evaluate_provider_revision(
            &ProviderFreshnessBaseline::Known("rev-1".to_string()),
            None,
        );
        assert_eq!(unsupported, ProviderFreshnessBaseline::Unsupported);
        assert_eq!(no_reason, None);
    }

    fn timeline_event(
        id: &str,
        seq: u64,
        kind: &str,
        status: Option<&str>,
        content: Option<&str>,
        raw: Option<Value>,
    ) -> AcpUiEvent {
        AcpUiEvent {
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            kind: kind.to_string(),
            session_id: Some("session-1".to_string()),
            content: content.map(str::to_string),
            title: None,
            tool_call_id: None,
            status: status.map(str::to_string),
            started_seq: Some(seq),
            ended_seq: Some(seq),
            started_at: Some(format!("{seq}Z")),
            ended_at: Some(format!("{seq}Z")),
            timing: None,
            raw,
        }
    }

    #[test]
    fn runtime_hot_timeline_keeps_only_unfinished_interactions() {
        let completed_tool = timeline_event(
            "tool-call-finished",
            1,
            "toolCall",
            Some("completed"),
            None,
            Some(json!({ "rawInput": { "path": "done.txt" } })),
        );
        let pending_tool = timeline_event(
            "tool-call-pending",
            2,
            "toolCall",
            Some("in_progress"),
            None,
            Some(json!({ "rawInput": { "path": "pending.txt" } })),
        );
        let pending_permission = timeline_event(
            "permission-1",
            3,
            "permissionRequest",
            Some("pending"),
            None,
            None,
        );
        let selected_permission = timeline_event(
            "permission-2",
            4,
            "permissionRequest",
            Some("selected"),
            None,
            None,
        );

        let hot = runtime_hot_timeline_items(vec![
            completed_tool,
            pending_tool,
            pending_permission,
            selected_permission,
        ]);

        assert_eq!(hot.len(), 2);
        assert!(hot.contains_key("tool-call-pending"));
        assert!(hot.contains_key("permission-1"));
    }

    #[test]
    fn active_timeline_stream_restores_only_latest_open_stream() {
        let prior_text = timeline_event("message-1", 1, "textDelta", None, Some("old"), None);
        let current_thought =
            timeline_event("thought-1", 2, "thoughtDelta", None, Some("thinking"), None);

        let (text, thought, plan) = active_timeline_streams(&[prior_text, current_thought]);

        assert!(text.is_none());
        assert_eq!(thought.unwrap().content, "thinking");
        assert!(plan.is_none());
    }

    #[test]
    fn active_timeline_stream_survives_an_elicitation_request() {
        let text = timeline_event("message-1", 1, "textDelta", None, Some("draft"), None);
        let elicitation = timeline_event(
            "elicitation-1",
            2,
            "elicitationRequest",
            Some("pending"),
            None,
            None,
        );

        let (text, thought, plan) = active_timeline_streams(&[text, elicitation]);

        assert_eq!(text.unwrap().content, "draft");
        assert!(thought.is_none());
        assert!(plan.is_none());
    }

    #[test]
    fn active_timeline_stream_closes_at_a_real_transcript_boundary() {
        let text = timeline_event("message-1", 1, "textDelta", None, Some("done"), None);
        let tool = timeline_event("tool-1", 2, "toolCall", Some("running"), None, None);

        let (text, thought, plan) = active_timeline_streams(&[text, tool]);

        assert!(text.is_none());
        assert!(thought.is_none());
        assert!(plan.is_none());
    }

    #[test]
    fn terminal_tool_update_preserves_intermediate_input_and_diff_before_release() {
        let intermediate = timeline_event(
            "tool-call-1",
            1,
            "toolCall",
            Some("in_progress"),
            None,
            Some(json!({
                "rawInput": { "path": "report.md" },
                "content": [{
                    "type": "diff",
                    "path": "report.md",
                    "oldText": "before",
                    "newText": "after"
                }],
                "locations": [{ "path": "report.md" }]
            })),
        );
        let mut terminal = timeline_event(
            "tool-call-1",
            2,
            "toolCall",
            Some("completed"),
            None,
            Some(json!({ "rawOutput": "done" })),
        );

        merge_tool_revision(&mut terminal, &intermediate);

        assert_eq!(
            terminal
                .raw
                .as_ref()
                .and_then(|raw| raw.get("rawInput"))
                .cloned(),
            Some(json!({ "path": "report.md" }))
        );
        assert_eq!(
            terminal
                .raw
                .as_ref()
                .and_then(|raw| raw.get("content"))
                .and_then(Value::as_array)
                .and_then(|content| content.first())
                .and_then(|content| content.get("type"))
                .and_then(Value::as_str),
            Some("diff")
        );
        assert_eq!(
            terminal
                .raw
                .as_ref()
                .and_then(|raw| raw.get("rawOutput"))
                .and_then(Value::as_str),
            Some("done")
        );
        assert!(runtime_hot_timeline_items(vec![terminal]).is_empty());
    }

    #[test]
    fn prompt_output_separates_visible_identified_and_anonymous_text() {
        let identified = json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "answer-1",
            "content": { "type": "text", "text": "answer" }
        });
        let anonymous = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "warning" }
        });
        let mut output = AcpPromptOutputAccumulator::default();
        output.observe(
            &identified,
            &normalize_session_update(1, Some("session-1".to_string()), &identified),
        );
        output.observe(
            &anonymous,
            &normalize_session_update(2, Some("session-1".to_string()), &anonymous),
        );

        assert_eq!(output.output.visible_text, "answerwarning");
        assert_eq!(output.output.identified_text, "answer");
        assert_eq!(output.output.identified_outputs, vec!["answer"]);
        assert_eq!(output.output.anonymous_text, "warning");
        assert_eq!(output.output.anonymous_chunk_count, 1);
    }

    #[test]
    fn bounded_text_append_tracks_many_small_chunks_incrementally() {
        let mut target = String::new();
        let mut target_chars = 0;

        for _ in 0..10_000 {
            append_bounded(&mut target, &mut target_chars, "界", 20_000);
        }

        assert_eq!(target_chars, 10_000);
        assert_eq!(target.chars().count(), 10_000);
    }

    #[test]
    fn bounded_text_append_preserves_unicode_and_existing_truncation_semantics() {
        let mut exact = String::new();
        let mut exact_chars = 0;
        append_bounded(&mut exact, &mut exact_chars, "你🙂好", 3);
        assert_eq!(exact, "你🙂好");
        assert_eq!(exact_chars, 3);

        let mut truncated = String::new();
        let mut truncated_chars = 0;
        append_bounded(&mut truncated, &mut truncated_chars, "你🙂好呀", 3);
        append_bounded(&mut truncated, &mut truncated_chars, "ignored", 3);
        assert_eq!(truncated, "你🙂好…");
        assert_eq!(truncated_chars, 4);
    }

    #[test]
    fn streaming_delta_accumulates_content_and_sequence_bounds() {
        let mut stream = None;
        let mut first = AcpUiEvent {
            id: "event-1".to_string(),
            seq: 10,
            timestamp: "10Z".to_string(),
            kind: "textDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("hello".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        };
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut first,
            "assistant-message-1",
            Some("assistant-message-1"),
            256_000,
            10,
            "10Z",
        );

        let mut second = AcpUiEvent {
            id: "event-2".to_string(),
            seq: 11,
            timestamp: "11Z".to_string(),
            kind: "textDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some(" world".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        };
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut second,
            "assistant-message-1",
            Some("assistant-message-1"),
            256_000,
            11,
            "11Z",
        );

        assert_eq!(first.id, "assistant-message-1");
        assert_eq!(first.content.as_deref(), Some("hello"));
        assert_eq!(first.started_seq, Some(10));
        assert_eq!(first.ended_seq, Some(10));
        assert_eq!(second.id, "assistant-message-1");
        assert_eq!(second.content.as_deref(), Some("hello world"));
        assert_eq!(second.started_seq, Some(10));
        assert_eq!(second.ended_seq, Some(11));
        assert_eq!(second.started_at.as_deref(), Some("10Z"));
        assert_eq!(second.ended_at.as_deref(), Some("11Z"));
    }

    #[test]
    fn streaming_delta_starts_a_new_stream_when_message_identity_changes() {
        let mut stream = None;
        let mut warning = timeline_event(
            "event-warning",
            10,
            "textDelta",
            None,
            Some("warning"),
            None,
        );
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut warning,
            "assistant-message-warning",
            Some("assistant-message-warning"),
            256_000,
            10,
            "10Z",
        );

        let mut answer =
            timeline_event("event-answer", 11, "textDelta", None, Some("answer"), None);
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut answer,
            "assistant-message-answer",
            Some("assistant-message-answer"),
            256_000,
            11,
            "11Z",
        );

        assert_eq!(warning.content.as_deref(), Some("warning"));
        assert_eq!(answer.id, "assistant-message-answer");
        assert_eq!(answer.content.as_deref(), Some("answer"));
        assert_eq!(answer.started_seq, Some(11));
    }

    #[test]
    fn streaming_delta_without_provider_identity_keeps_contiguous_fallback_stream() {
        let mut stream = None;
        let mut first = timeline_event("event-1", 10, "textDelta", None, Some("hel"), None);
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut first,
            "assistant-message-event-1",
            None,
            256_000,
            10,
            "10Z",
        );
        let mut second = timeline_event("event-2", 11, "textDelta", None, Some("lo"), None);
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut second,
            "assistant-message-event-2",
            None,
            256_000,
            11,
            "11Z",
        );

        assert_eq!(second.id, "assistant-message-event-1");
        assert_eq!(second.content.as_deref(), Some("hello"));
    }

    #[test]
    fn agent_branch_stream_ids_and_restart_state_are_isolated() {
        let branch_text = |id: &str, seq: u64, parent_tool_call_id: &str, content: &str| {
            let mut event = timeline_event(id, seq, "textDelta", None, Some(content), None);
            event.raw = Some(json!({
                "_meta": {
                    "agentTranscript": {
                        "parentToolCallId": parent_tool_call_id
                    }
                },
                "messageId": id
            }));
            event
        };
        let agent_a = branch_text("message-a", 10, "provider-agent-a", "A");
        let agent_b = branch_text("message-b", 11, "provider-agent-b", "B");

        assert_ne!(
            stable_message_item_id(&agent_a),
            stable_message_item_id(&agent_b)
        );
        let streams = active_timeline_streams_by_branch(&[agent_a.clone(), agent_b.clone()]);
        let branch_a =
            crate::acp::branches::stable_agent_execution_id("session-1", "provider-agent-a");
        let branch_b =
            crate::acp::branches::stable_agent_execution_id("session-1", "provider-agent-b");
        assert_eq!(streams.len(), 2);
        assert_eq!(
            streams
                .get(&branch_a)
                .and_then(|stream| stream.text.as_ref())
                .map(|stream| stream.content.as_str()),
            Some("A")
        );
        assert_eq!(
            streams
                .get(&branch_b)
                .and_then(|stream| stream.text.as_ref())
                .map(|stream| stream.content.as_str()),
            Some("B")
        );
    }

    #[test]
    fn output_policy_is_provider_agnostic_and_requires_identified_artifact_output() {
        let anonymous = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "provider warning" }
        });
        let identified = json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "answer-1",
            "content": { "type": "text", "text": "answer" }
        });
        let mut anonymous_only = AcpPromptOutputAccumulator::default();
        anonymous_only.observe(
            &anonymous,
            &normalize_session_update(1, Some("session-1".to_string()), &anonymous),
        );

        assert!(
            unidentified_agent_output_failure(
                AcpOutputPolicy::Conversation,
                &anonymous_only.output,
            )
            .is_none()
        );
        let failure = unidentified_agent_output_failure(
            AcpOutputPolicy::ArtifactContract,
            &anonymous_only.output,
        )
        .expect("anonymous-only artifact output must fail strict settlement");
        assert_eq!(failure.code, ACP_UNIDENTIFIED_AGENT_OUTPUT_CODE);

        anonymous_only.observe(
            &identified,
            &normalize_session_update(2, Some("session-1".to_string()), &identified),
        );
        assert!(
            unidentified_agent_output_failure(
                AcpOutputPolicy::ArtifactContract,
                &anonymous_only.output,
            )
            .is_none()
        );
    }

    #[test]
    fn streaming_thought_blocks_preserve_chunk_boundaries_as_paragraphs() {
        let mut stream = None;
        let mut first = AcpUiEvent {
            id: "thought-1".to_string(),
            seq: 10,
            timestamp: "10Z".to_string(),
            kind: "thoughtDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("**Designing routes**".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        };
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut first,
            "assistant-thought-1",
            Some("assistant-thought-1"),
            256_000,
            10,
            "10Z",
        );

        let mut second = AcpUiEvent {
            id: "thought-2".to_string(),
            seq: 11,
            timestamp: "11Z".to_string(),
            kind: "thoughtDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("**Planning branches**".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        };
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut second,
            "assistant-thought-1",
            Some("assistant-thought-1"),
            256_000,
            11,
            "11Z",
        );

        assert_eq!(
            second.content.as_deref(),
            Some("**Designing routes**\n**Planning branches**")
        );
    }

    #[test]
    fn streaming_thought_token_chunks_remain_contiguous() {
        let mut stream = None;
        let mut first = AcpUiEvent {
            id: "thought-1".to_string(),
            seq: 10,
            timestamp: "10Z".to_string(),
            kind: "thoughtDelta".to_string(),
            session_id: None,
            content: Some("thinking ".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        };
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut first,
            "assistant-thought-1",
            Some("assistant-thought-1"),
            256_000,
            10,
            "10Z",
        );
        let mut second = AcpUiEvent {
            id: "thought-2".to_string(),
            seq: 11,
            timestamp: "11Z".to_string(),
            kind: "thoughtDelta".to_string(),
            session_id: None,
            content: Some("more".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        };
        AcpRuntime::apply_streaming_delta(
            &mut stream,
            &mut second,
            "assistant-thought-1",
            Some("assistant-thought-1"),
            256_000,
            11,
            "11Z",
        );

        assert_eq!(second.content.as_deref(), Some("thinking more"));
    }

    #[test]
    fn stream_switch_takes_pending_live_update_before_overwrite() {
        let mut pending = Some(AcpUiEvent {
            id: "assistant-message-1".to_string(),
            seq: 20,
            timestamp: "20Z".to_string(),
            kind: "textDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("完整文本快照".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: Some(10),
            ended_seq: Some(20),
            started_at: Some("10Z".to_string()),
            ended_at: Some("20Z".to_string()),
            timing: None,
            raw: None,
        });
        let next_stream = AcpUiEvent {
            id: "session-plan-1".to_string(),
            seq: 21,
            timestamp: "21Z".to_string(),
            kind: "plan".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some(String::new()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: Some(21),
            ended_seq: Some(21),
            started_at: Some("21Z".to_string()),
            ended_at: Some("21Z".to_string()),
            timing: None,
            raw: None,
        };

        let flushed =
            take_pending_live_update_for_stream_switch(&mut pending, &next_stream).unwrap();

        assert_eq!(flushed.id, "assistant-message-1");
        assert_eq!(flushed.content.as_deref(), Some("完整文本快照"));
        assert!(pending.is_none());
    }

    #[test]
    fn same_stream_keeps_pending_live_update_buffered() {
        let mut pending = Some(AcpUiEvent {
            id: "assistant-message-1".to_string(),
            seq: 20,
            timestamp: "20Z".to_string(),
            kind: "textDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("partial".to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: Some(10),
            ended_seq: Some(20),
            started_at: Some("10Z".to_string()),
            ended_at: Some("20Z".to_string()),
            timing: None,
            raw: None,
        });
        let same_stream = pending.as_ref().unwrap().clone();

        let flushed = take_pending_live_update_for_stream_switch(&mut pending, &same_stream);

        assert!(flushed.is_none());
        assert_eq!(
            pending.and_then(|event| event.content),
            Some("partial".to_string())
        );
    }

    #[test]
    fn permission_decision_event_preserves_pending_timeline_identity() {
        let existing = AcpUiEvent {
            id: "permission-0".to_string(),
            seq: 10,
            timestamp: "1Z".to_string(),
            kind: "permissionRequest".to_string(),
            session_id: Some("session-1".to_string()),
            content: None,
            title: Some("Write file".to_string()),
            tool_call_id: Some("tool-1".to_string()),
            status: Some("pending".to_string()),
            started_seq: Some(10),
            ended_seq: Some(10),
            started_at: Some("1Z".to_string()),
            ended_at: Some("1Z".to_string()),
            timing: None,
            raw: Some(json!({
                "requestId": "0",
                "options": [{ "optionId": "allow", "name": "Allow" }]
            })),
        };
        let response = PermissionResponseState {
            request_id: "0".to_string(),
            option_id: Some("allow".to_string()),
            cancelled: false,
            decided_at: "2Z".to_string(),
        };

        let event = permission_decision_timeline_event(11, "0", &response, Some(&existing));

        assert_eq!(event.id, "0");
        assert_eq!(event.kind, "permissionRequest");
        assert_eq!(event.status.as_deref(), Some("selected"));
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(event.title.as_deref(), Some("Write file"));
        assert_eq!(event.started_seq, Some(10));
        assert_eq!(event.started_at.as_deref(), Some("1Z"));
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("requestId"))
                .and_then(Value::as_str),
            Some("0")
        );
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("optionId"))
                .and_then(Value::as_str),
            Some("allow")
        );
    }

    #[test]
    fn timing_rebuild_with_permission_decision_excludes_wait_interval() {
        let prompt_1 = AcpUiEvent {
            id: "gold-band-user-prompt-1".to_string(),
            seq: 1,
            timestamp: "100Z".to_string(),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("first".to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: Some(1),
            ended_seq: Some(1),
            started_at: Some("100Z".to_string()),
            ended_at: Some("100Z".to_string()),
            timing: None,
            raw: Some(json!({ "source": "goldBandPrompt" })),
        };
        let usage_1 = AcpUiEvent {
            id: "acp-event-2".to_string(),
            seq: 2,
            timestamp: "130Z".to_string(),
            kind: "usageUpdate".to_string(),
            session_id: Some("session-1".to_string()),
            content: None,
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: Some(2),
            ended_seq: Some(2),
            started_at: Some("130Z".to_string()),
            ended_at: Some("130Z".to_string()),
            timing: None,
            raw: Some(json!({ "sessionUpdate": "usage_update" })),
        };
        let prompt_2 = AcpUiEvent {
            id: "gold-band-user-prompt-3".to_string(),
            seq: 3,
            timestamp: "200Z".to_string(),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("second".to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: Some(3),
            ended_seq: Some(3),
            started_at: Some("200Z".to_string()),
            ended_at: Some("200Z".to_string()),
            timing: None,
            raw: Some(json!({ "source": "goldBandPrompt" })),
        };
        let selected_permission = AcpUiEvent {
            id: "permission-1".to_string(),
            seq: 5,
            timestamp: "230Z".to_string(),
            kind: "permissionRequest".to_string(),
            session_id: Some("session-1".to_string()),
            content: None,
            title: Some("Write file".to_string()),
            tool_call_id: Some("tool-1".to_string()),
            status: Some("selected".to_string()),
            started_seq: Some(4),
            ended_seq: Some(5),
            started_at: Some("214Z".to_string()),
            ended_at: Some("230Z".to_string()),
            timing: None,
            raw: Some(json!({ "requestId": "1", "optionId": "allow" })),
        };
        let timeline_items = vec![prompt_1, usage_1, prompt_2, selected_permission];
        let timing_state = AcpTimingState::from_timeline_item_refs(&timeline_items);
        let snapshot = timing_state.snapshot_at(false, None).unwrap();

        assert_eq!(snapshot.session_elapsed_seconds, 44);
    }

    #[test]
    fn session_setup_params_append_system_prompt() {
        let new_params =
            session_new_params(camino::Utf8Path::new("/repo"), "node constraints", &[]);
        assert_eq!(
            new_params["_meta"]["systemPrompt"]["append"],
            "node constraints"
        );

        let load_params = session_load_params(
            camino::Utf8Path::new("/repo"),
            "session-123",
            "node constraints",
            &[],
        );
        assert_eq!(load_params["sessionId"], "session-123");
        assert_eq!(
            load_params["_meta"]["systemPrompt"]["append"],
            "node constraints"
        );

        let resume_params = session_resume_params(
            camino::Utf8Path::new("/repo"),
            "session-123",
            "node constraints",
            &[],
        );
        assert_eq!(resume_params["sessionId"], "session-123");
        assert_eq!(resume_params["cwd"], "/repo");
        assert_eq!(resume_params["mcpServers"], json!([]));
        assert_eq!(
            resume_params["_meta"]["systemPrompt"]["append"],
            "node constraints"
        );
    }

    #[test]
    fn session_restore_capabilities_follow_v1_advertisement_shape() {
        assert_eq!(
            SessionRestoreCapabilities::from_agent_capabilities(&json!({
                "loadSession": true,
                "sessionCapabilities": { "resume": {} }
            })),
            SessionRestoreCapabilities {
                resume: true,
                load: true,
            }
        );
        assert_eq!(
            SessionRestoreCapabilities::from_agent_capabilities(&json!({})),
            SessionRestoreCapabilities::default()
        );
    }

    #[test]
    fn restore_planning_prefers_resume_for_context_only_continuation() {
        let both = SessionRestoreCapabilities {
            resume: true,
            load: true,
        };
        assert_eq!(
            plan_session_restore(SessionRestoreIntent::ContinueOnly, both, true),
            Ok(SessionRestorePlan::Restore(SessionRestoreMethod::Resume))
        );
        assert!(!SessionRestoreMethod::Resume.replays_history());
    }

    #[test]
    fn restore_planning_falls_back_to_load_when_resume_is_unavailable() {
        let load_only = SessionRestoreCapabilities {
            resume: false,
            load: true,
        };
        assert_eq!(
            plan_session_restore(SessionRestoreIntent::ContinueOnly, load_only, true),
            Ok(SessionRestorePlan::Restore(SessionRestoreMethod::Load))
        );
        assert!(SessionRestoreMethod::Load.replays_history());
    }

    #[test]
    fn restore_planning_uses_load_for_external_history_sync() {
        let both = SessionRestoreCapabilities {
            resume: true,
            load: true,
        };
        assert_eq!(
            plan_session_restore(SessionRestoreIntent::SyncHistory, both, true),
            Ok(SessionRestorePlan::Restore(SessionRestoreMethod::Load))
        );
    }

    #[test]
    fn restore_planning_rejects_history_sync_when_only_resume_is_available() {
        let resume_only = SessionRestoreCapabilities {
            resume: true,
            load: false,
        };
        assert_eq!(
            plan_session_restore(SessionRestoreIntent::SyncHistory, resume_only, false),
            Err(SessionRestorePlanError::HistorySyncUnsupported)
        );
        assert_eq!(
            SessionRestorePlanError::HistorySyncUnsupported.code(),
            super::ACP_HISTORY_SYNC_UNSUPPORTED_CODE
        );
    }

    #[test]
    fn restore_planning_distinguishes_strict_and_non_strict_missing_capabilities() {
        let neither = SessionRestoreCapabilities::default();
        assert_eq!(
            plan_session_restore(SessionRestoreIntent::ContinueOnly, neither, true),
            Err(SessionRestorePlanError::RestoreUnsupported)
        );
        assert_eq!(
            plan_session_restore(SessionRestoreIntent::ContinueOnly, neither, false),
            Ok(SessionRestorePlan::StartNew)
        );
    }

    #[test]
    fn session_setup_params_only_include_mcp_servers_accepted_by_live_capabilities() {
        let servers = vec![
            json!({"name": "local", "command": "node"}),
            json!({"type": "http", "name": "docs", "url": "https://example.com/mcp"}),
            json!({"type": "sse", "name": "legacy", "url": "https://example.com/sse"}),
        ];
        let capabilities = json!({
            "mcpCapabilities": {
                "http": true,
                "sse": false
            }
        });
        let prepared = prepare_acp_mcp_servers(&servers, Some(&capabilities));

        let new_params = session_new_params(camino::Utf8Path::new("/repo"), "", &prepared.accepted);
        let load_params = session_load_params(
            camino::Utf8Path::new("/repo"),
            "session-123",
            "",
            &prepared.accepted,
        );
        let resume_params = session_resume_params(
            camino::Utf8Path::new("/repo"),
            "session-123",
            "",
            &prepared.accepted,
        );

        assert_eq!(new_params["mcpServers"], json!([servers[0], servers[1]]));
        assert_eq!(load_params["mcpServers"], json!([servers[0], servers[1]]));
        assert_eq!(resume_params["mcpServers"], json!([servers[0], servers[1]]));
        assert_eq!(prepared.skipped.len(), 1);
        assert_eq!(prepared.skipped[0].name, "legacy");
    }

    #[test]
    fn direct_session_setup_omits_empty_system_prompt_metadata() {
        let new_params = session_new_params(camino::Utf8Path::new("/repo"), "", &[]);
        assert!(new_params.get("_meta").is_none());

        let load_params =
            session_load_params(camino::Utf8Path::new("/repo"), "session-123", "", &[]);
        assert!(load_params.get("_meta").is_none());

        let resume_params =
            session_resume_params(camino::Utf8Path::new("/repo"), "session-123", "", &[]);
        assert!(resume_params.get("_meta").is_none());
    }

    #[test]
    fn codex_session_prompt_inlines_system_prompt() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "do the task".to_string(),
            display_text: None,
            quotes: Vec::new(),
            prompt_id: Some("prompt-001".to_string()),
            visibility: PromptVisibility::Visible,
            hidden_reason: None,
            turn_control_mode: TurnControlMode::RuntimeControlled,
            runtime_control_intent: crate::provider::RuntimeControlIntent::Unchanged,
            runtime_control_transition_id: None,
            runtime_control_source_transition_id: None,
            runtime_control_transition_cause: None,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        let text = session_prompt_text("codex-acp", &prompt, false, false);
        assert!(text.contains(
            "<hidden data-gold-band-hidden=\"true\" title=\"Gold Band stable system prompt\">"
        ));
        assert!(text.contains("node constraints"));
        assert!(text.ends_with("do the task"));

        let params = session_prompt_params("codex-acp", "session-123", &prompt, false, false);
        assert_eq!(params["sessionId"], "session-123");
        assert_eq!(params["prompt"][0]["text"], text);
    }

    #[test]
    fn codex_restored_session_prompt_does_not_inline_system_prompt() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "follow up".to_string(),
            display_text: None,
            quotes: Vec::new(),
            prompt_id: Some("prompt-002".to_string()),
            visibility: PromptVisibility::Visible,
            hidden_reason: None,
            turn_control_mode: TurnControlMode::RuntimeControlled,
            runtime_control_intent: crate::provider::RuntimeControlIntent::Unchanged,
            runtime_control_transition_id: None,
            runtime_control_source_transition_id: None,
            runtime_control_transition_cause: None,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        let text = session_prompt_text("codex-acp", &prompt, true, false);
        assert_eq!(text, "follow up");
        assert!(!text.contains("Gold Band stable system prompt"));
        assert!(!text.contains("node constraints"));

        let params = session_prompt_params("codex-acp", "session-123", &prompt, true, false);
        assert_eq!(params["sessionId"], "session-123");
        assert_eq!(params["prompt"][0]["text"], "follow up");
    }

    #[test]
    fn claude_session_prompt_keeps_user_prompt_only() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "do the task".to_string(),
            display_text: None,
            quotes: Vec::new(),
            prompt_id: None,
            visibility: PromptVisibility::Visible,
            hidden_reason: None,
            turn_control_mode: TurnControlMode::RuntimeControlled,
            runtime_control_intent: crate::provider::RuntimeControlIntent::Unchanged,
            runtime_control_transition_id: None,
            runtime_control_source_transition_id: None,
            runtime_control_transition_cause: None,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        assert_eq!(
            session_prompt_text("claude-acp", &prompt, false, true),
            "do the task"
        );
        assert_eq!(
            session_prompt_text("claude-acp", &prompt, true, true),
            "do the task"
        );
    }

    #[test]
    fn unsupported_permission_mode_reports_available_modes() {
        let modes = json!({
            "availableModes": [
                { "id": "read-only", "name": "Read Only" },
                { "id": "auto", "name": "Default" }
            ]
        });

        let error = resolve_permission_mode("unknown", None, Some(&modes))
            .expect_err("unknown mode should fail before sending it to the agent")
            .to_string();

        assert!(error.contains("unknown"));
        assert!(error.contains("read-only, auto"));
    }

    #[test]
    fn stale_session_model_is_normalized_to_unspecified_before_rpc() {
        let config_options = json!([{
            "id": "model",
            "category": "model",
            "options": [
                { "value": "gpt-5.6-sol", "name": "GPT-5.6-Sol" },
                { "value": "gpt-5.6-terra", "name": "GPT-5.6-Terra" }
            ]
        }]);

        assert_eq!(
            resolve_session_model("gpt-5.4", Some(&config_options)),
            SessionModelResolution::Stale {
                requested: "gpt-5.4".to_string(),
                available: vec!["gpt-5.6-sol".to_string(), "gpt-5.6-terra".to_string()],
            }
        );
        assert_eq!(
            resolve_session_model("gpt-5.6-sol", Some(&config_options)),
            SessionModelResolution::Selected("gpt-5.6-sol".to_string())
        );
    }

    #[test]
    fn doctor_success_cleanup_removes_acp_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let acp_dir = dir.join("doctor/acp");
        std::fs::create_dir_all(acp_dir.as_std_path()).unwrap();
        std::fs::write(acp_dir.join("provider.pid").as_std_path(), "123").unwrap();
        std::fs::write(acp_dir.join("acp.raw.jsonl").as_std_path(), "{}\n").unwrap();

        cleanup_doctor_acp_dir_after_success(&acp_dir);

        assert!(!acp_dir.exists());
        drop(temp);
    }

    #[test]
    fn doctor_failure_bundle_removes_pid_and_bounds_jsonl() {
        let temp = tempfile::tempdir().unwrap();
        let dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let acp_dir = dir.join("doctor/acp");
        std::fs::create_dir_all(acp_dir.as_std_path()).unwrap();
        std::fs::write(acp_dir.join("provider.pid").as_std_path(), "123").unwrap();
        let large = (0..4096)
            .map(|index| format!(r#"{{"index":{index},"payload":"{}"}}"#, "x".repeat(256)))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(acp_dir.join("acp.diagnostics.jsonl").as_std_path(), large).unwrap();

        retain_bounded_doctor_acp_failure_bundle(&acp_dir);

        assert!(!acp_dir.join("provider.pid").exists());
        let size = std::fs::metadata(acp_dir.join("acp.diagnostics.jsonl").as_std_path())
            .unwrap()
            .len();
        assert!(size <= DOCTOR_DIAGNOSTIC_TARGET_SIZE + 512);
        drop(temp);
    }

    #[test]
    fn runtime_stop_probe_uses_runtime_locator() {
        let dir = tempfile::tempdir().unwrap();
        let run_file = camino::Utf8PathBuf::from_path_buf(dir.path().join("run.json")).unwrap();
        std::fs::write(
            run_file.as_std_path(),
            serde_json::to_string(&json!({
                "status": "paused",
                "pause_reason": "process-interrupted",
                "current_round": "round-001",
                "current_node": "ai-dynamic1",
                "current_attempt": "attempt-001"
            }))
            .unwrap(),
        )
        .unwrap();

        let outer_probe = RuntimeStopProbe {
            turn_control_mode: TurnControlMode::RuntimeControlled,
            run_file: run_file.clone(),
            round_id: "round-001".to_string(),
            node_id: "ai-dynamic1".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: None,
        };
        let inner_probe = RuntimeStopProbe {
            turn_control_mode: TurnControlMode::RuntimeControlled,
            run_file,
            round_id: "round-001".to_string(),
            node_id: "bootstrap".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: None,
        };

        assert!(outer_probe.is_stopped());
        assert!(!inner_probe.is_stopped());
    }

    #[test]
    fn runtime_stop_probe_uses_own_dynamic_attempt_state() {
        let dir = tempfile::tempdir().unwrap();
        let run_file = camino::Utf8PathBuf::from_path_buf(dir.path().join("run.json")).unwrap();
        let own_state =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("own-node.json")).unwrap();
        let sibling_state =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("sibling-node.json")).unwrap();
        std::fs::write(
            run_file.as_std_path(),
            serde_json::to_string(&json!({
                "status": "running",
                "current_round": "round-001",
                "current_node": "ai-dynamic",
                "current_attempt": "attempt-001"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            own_state.as_std_path(),
            serde_json::to_string(&json!({
                "status": "running",
                "outcome": null
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            sibling_state.as_std_path(),
            serde_json::to_string(&json!({
                "status": "paused",
                "outcome": null
            }))
            .unwrap(),
        )
        .unwrap();

        let running_leaf_probe = RuntimeStopProbe {
            turn_control_mode: TurnControlMode::RuntimeControlled,
            run_file: run_file.clone(),
            round_id: "round-001".to_string(),
            node_id: "ai-dynamic".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: Some(own_state),
        };
        let paused_leaf_probe = RuntimeStopProbe {
            turn_control_mode: TurnControlMode::RuntimeControlled,
            run_file,
            round_id: "round-001".to_string(),
            node_id: "ai-dynamic".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: Some(sibling_state),
        };

        assert!(!running_leaf_probe.is_stopped());
        assert!(paused_leaf_probe.is_stopped());
    }

    #[test]
    fn runtime_stop_probe_keeps_manual_check_attempt_alive() {
        let dir = tempfile::tempdir().unwrap();
        let run_file = camino::Utf8PathBuf::from_path_buf(dir.path().join("run.json")).unwrap();
        let manual_check_state =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("manual-check-node.json")).unwrap();
        std::fs::write(
            run_file.as_std_path(),
            serde_json::to_string(&json!({
                "status": "running",
                "current_round": "round-001",
                "current_node": "plan",
                "current_attempt": "attempt-001"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            manual_check_state.as_std_path(),
            serde_json::to_string(&json!({
                "status": "paused",
                "outcome": null,
                "manual_check_pending": true
            }))
            .unwrap(),
        )
        .unwrap();

        let probe = RuntimeStopProbe {
            turn_control_mode: TurnControlMode::RuntimeControlled,
            run_file,
            round_id: "round-001".to_string(),
            node_id: "plan".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: Some(manual_check_state),
        };

        assert!(!probe.is_stopped());
    }

    #[test]
    fn runtime_stop_probe_does_not_cancel_non_runtime_conversation_turns() {
        let dir = tempfile::tempdir().unwrap();
        let run_file = camino::Utf8PathBuf::from_path_buf(dir.path().join("run.json")).unwrap();
        std::fs::write(
            run_file.as_std_path(),
            serde_json::to_string(&json!({
                "status": "paused",
                "pause_reason": "process-interrupted",
                "current_round": "round-001",
                "current_node": "dev",
                "current_attempt": "attempt-001"
            }))
            .unwrap(),
        )
        .unwrap();

        let probe = RuntimeStopProbe {
            turn_control_mode: TurnControlMode::NonRuntimeControlled,
            run_file,
            round_id: "round-001".to_string(),
            node_id: "dev".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: None,
        };

        assert!(!probe.is_stopped());
    }

    #[test]
    fn prior_attempt_metrics_reads_tokens_from_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let snapshot_path = dir.path().join("acp.snapshot.json");
        std::fs::write(
            &snapshot_path,
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "usedTokens":5000,"contextWindowSize":200000,
            "totalCostUsd":0.05,
            "inputTokens":3000,"outputTokens":2000,
            "cachedReadTokens":500,"cachedWriteTokens":100,"totalTokens":5100,
            "attemptInputTokens":7000,"attemptOutputTokens":2500,
            "attemptCachedReadTokens":1500,"attemptCachedWriteTokens":200,
            "attemptTotalTokens":11200
        }"#,
        )
        .unwrap();
        let path = camino::Utf8Path::from_path(&snapshot_path).unwrap();
        let prior = super::read_prior_attempt_metrics(path);
        assert_eq!(prior.used_tokens, Some(5000));
        assert_eq!(prior.context_window_size, Some(200000));
        assert!((prior.total_cost_usd.unwrap() - 0.05).abs() < 0.0001);
        assert_eq!(prior.input_tokens, Some(3000));
        assert_eq!(prior.output_tokens, Some(2000));
        assert_eq!(prior.cached_read_tokens, Some(500));
        assert_eq!(prior.cached_write_tokens, Some(100));
        assert_eq!(prior.total_tokens, Some(5100));
        assert_eq!(prior.attempt_input_tokens, Some(7000));
        assert_eq!(prior.attempt_output_tokens, Some(2500));
        assert_eq!(prior.attempt_cached_read_tokens, Some(1500));
        assert_eq!(prior.attempt_cached_write_tokens, Some(200));
        assert_eq!(prior.attempt_total_tokens, Some(11200));
    }

    #[test]
    fn prior_attempt_metrics_defaults_when_no_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let snapshot_path = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join("nonexistent.snapshot.json");
        let prior = super::read_prior_attempt_metrics(&snapshot_path);
        assert_eq!(prior.used_tokens, None);
        assert_eq!(prior.context_window_size, None);
        assert_eq!(prior.total_cost_usd, None);
        assert_eq!(prior.input_tokens, None);
        assert_eq!(prior.output_tokens, None);
        assert_eq!(prior.cached_read_tokens, None);
        assert_eq!(prior.cached_write_tokens, None);
        assert_eq!(prior.total_tokens, None);
        assert_eq!(prior.attempt_input_tokens, None);
        assert_eq!(prior.attempt_output_tokens, None);
        assert_eq!(prior.attempt_cached_read_tokens, None);
        assert_eq!(prior.attempt_cached_write_tokens, None);
        assert_eq!(prior.attempt_total_tokens, None);
    }

    #[test]
    fn prior_attempt_metrics_reads_null_token_fields_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let snapshot_path = dir.path().join("acp.snapshot.json");
        std::fs::write(
            &snapshot_path,
            r#"{
            "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
            "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
            "inputTokens":null,"outputTokens":null,"totalTokens":null,
            "timing":{"sessionElapsedSeconds":300}
        }"#,
        )
        .unwrap();
        let path = camino::Utf8Path::from_path(&snapshot_path).unwrap();
        let prior = super::read_prior_attempt_metrics(path);
        assert_eq!(prior.input_tokens, None);
        assert_eq!(prior.output_tokens, None);
        assert_eq!(prior.total_tokens, None);
    }

    #[test]
    fn restores_running_and_completed_compaction_until_usage_after_is_known() {
        let event = |status: &str, after: Option<u64>| AcpUiEvent {
            id: "context-compaction-10".to_string(),
            seq: 10,
            timestamp: "100Z".to_string(),
            kind: "contextCompaction".to_string(),
            session_id: Some("session-1".to_string()),
            content: None,
            title: None,
            tool_call_id: None,
            status: Some(status.to_string()),
            started_seq: Some(10),
            ended_seq: (status == "completed").then_some(20),
            started_at: Some("100Z".to_string()),
            ended_at: (status == "completed").then(|| "120Z".to_string()),
            timing: None,
            raw: Some(json!({
                "contextCompaction": {
                    "phase": status,
                    "contextUsedBefore": 169_052,
                    "contextSize": 200_000,
                    "contextUsedAfter": after,
                }
            })),
        };

        let running = active_context_compaction(&[event("running", None)]).unwrap();
        assert_eq!(running.context_used_before, Some(169_052));
        assert_eq!(running.completed_seq, None);

        let completed = active_context_compaction(&[event("completed", None)]).unwrap();
        assert_eq!(completed.completed_seq, Some(20));

        assert!(active_context_compaction(&[event("completed", Some(23_825))]).is_none());
        assert!(active_context_compaction(&[event("interrupted", None)]).is_none());
    }

    fn completed_compaction_state() -> AcpContextCompactionState {
        AcpContextCompactionState {
            item_id: "context-compaction-31".to_string(),
            started_seq: 31,
            started_at: "1785153896Z".to_string(),
            context_used_before: Some(32_606),
            context_size: Some(1_000_000),
            completed_seq: Some(32),
            completed_at: Some("1785153938Z".to_string()),
            saw_post_completion_reset: false,
        }
    }

    #[test]
    fn transient_zero_does_not_replace_confirmed_context_usage() {
        let mut state = AcpUsageState::default();

        for used in [0, 28_084, 0, 34_791, 0, 34_864, 0] {
            assert_eq!(
                state.observe_provider_usage(Some(used), Some(1_000_000), None),
                None
            );
        }

        assert_eq!(state.context.confirmed_used, Some(34_864));
        assert_eq!(state.context.window_size, Some(1_000_000));
    }

    #[test]
    fn compaction_accepts_first_positive_usage_after_reset_even_when_it_increases() {
        let mut state = AcpUsageState::default();
        state.context.confirmed_used = Some(32_606);
        state.context.window_size = Some(1_000_000);
        state.compaction = Some(completed_compaction_state());

        assert_eq!(
            state.observe_provider_usage(Some(36_881), Some(1_000_000), None),
            None
        );
        assert_eq!(state.context.confirmed_used, Some(32_606));
        assert_eq!(
            state.observe_provider_usage(Some(0), Some(1_000_000), None),
            None
        );
        assert_eq!(
            state.observe_provider_usage(Some(33_792), Some(1_000_000), None),
            Some(33_792)
        );
        assert_eq!(state.context.confirmed_used, Some(33_792));
    }

    #[test]
    fn compaction_accepts_lower_positive_usage_as_no_reset_fallback() {
        let mut state = AcpUsageState::default();
        state.context.confirmed_used = Some(169_052);
        state.compaction = Some(completed_compaction_state());

        assert_eq!(
            state.observe_provider_usage(Some(23_825), Some(200_000), None),
            Some(23_825)
        );
        assert_eq!(state.context.confirmed_used, Some(23_825));
    }

    #[test]
    fn canonical_usage_omits_unconfirmed_zero() {
        let mut state = AcpUsageState::default();
        state.observe_provider_usage(Some(0), Some(1_000_000), Some(0.25));
        let mut update = json!({
            "sessionUpdate": "usage_update",
            "used": 0,
            "size": 1_000_000,
            "cost": {"amount": 0.25, "currency": "USD"}
        });

        state.normalize_timeline_usage(&mut update);

        assert_eq!(update.get("used"), None);
        assert_eq!(update.get("size").and_then(Value::as_u64), Some(1_000_000));
        assert_eq!(
            update.pointer("/cost/amount").and_then(Value::as_f64),
            Some(0.25)
        );
    }
}
