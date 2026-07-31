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
}

#[derive(Debug, Clone)]
struct AcpPromptTurnIdentity {
    id: String,
    seq: u64,
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

use crate::acp::commands::{AcpCommandItem, parse_available_commands};
use crate::acp::connection::{
    AcpConnectionUnavailable, AdapterConnection, AdapterConnectionKey, AdapterConnectionManager,
    SessionEventPump, SessionRouteTryRecvError,
};
use crate::acp::elicitation::{
    ELICITATION_DEFAULT_TIMEOUT, PendingElicitationState, cancel_pending_elicitation_requests,
    elicitation_response_result, remove_elicitation_signal_files,
    upsert_elicitation_response_event, wait_for_elicitation_response, write_pending_elicitation,
};
use crate::acp::events::{
    AcpAttemptPaths, AcpSessionMetadata, AcpSessionTiming, AcpTimingState, AcpUiEvent,
    append_diagnostic, append_raw_frame, append_structured_diagnostic, append_ui_event,
    current_timestamp, initial_acp_event_seq, latest_timeline_source_seq, load_timeline_items,
    normalize_session_update, permission_request_event, user_prompt_event, write_session_metadata,
};
use crate::acp::history::{ProviderHistoryImport, ProviderHistoryReplay, ReplayUpdateDecision};
use crate::acp::permission::{
    PermissionResponseState, acp_permission_response_result, cancel_pending_permission_requests,
    permission_response_file, remove_permission_signal_files, wait_for_permission_response,
    write_pending_permission,
};
use crate::acp::timeline::{TimelineCompactionPolicy, TimelineStore};
use crate::acp::usage::{
    AcpAttemptTokenTotals, AcpAttemptUsageRecovery, AcpPromptTokenUsage, append_prompt_completed,
    append_prompt_started, repair_attempt_usage,
};
use crate::config::{AcpAdapterConfig, RuntimeConfig};
use crate::domain::{SessionMode, VERSION};
use crate::provider::{
    ACP_MCP_TRANSPORT_UNSUPPORTED_CODE, PromptBundle, PromptVisibility, SkippedAcpMcpServer,
    gold_band_hidden_block, prepare_acp_mcp_servers, supports_system_prompt,
};
use crate::runtime::{WorkerRefState, validate_worker_ref_state};
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
const SESSION_REPLAY_QUIET_PERIOD: Duration = Duration::from_millis(200);
const SESSION_REPLAY_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_LIST_MAX_PAGES: usize = 8;
const SESSION_EVICTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const SESSION_SYSTEM_CONTEXT_VERSION: u32 = 1;

#[derive(Debug)]
struct AcpCancelled;

fn initialize_params() -> Value {
    json!({
        "protocolVersion": 1,
        "clientCapabilities": {
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
    let started_at = Instant::now();
    let mut drained_frames = 0usize;
    loop {
        let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
            return Err(anyhow!(AcpSessionReplayDrainTimeout { timeout }));
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
                return Err(anyhow!(AcpSessionReplayDrainTimeout { timeout }));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(AcpTransportInterrupted));
            }
        }
    }
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
pub enum PromptActivity {
    Starting,
    Accepted,
    Running,
    CancelRequested,
}

#[derive(Debug)]
struct ProviderControl {
    state: Mutex<ProviderControlState>,
    cancel_sent: Mutex<bool>,
}

impl ProviderControl {
    fn new() -> Self {
        Self {
            state: Mutex::new(ProviderControlState::Starting),
            cancel_sent: Mutex::new(false),
        }
    }

    fn state(&self) -> ProviderControlState {
        self.state
            .lock()
            .map(|state| *state)
            .unwrap_or(ProviderControlState::Stopped)
    }

    fn request_prompt_cancel(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        match *state {
            ProviderControlState::Starting
            | ProviderControlState::Accepted
            | ProviderControlState::Running => {
                *state = ProviderControlState::CancelRequested;
                true
            }
            ProviderControlState::CancelRequested | ProviderControlState::Stopped => false,
        }
    }

    fn mark_cancel_sent(&self) -> bool {
        let Ok(mut sent) = self.cancel_sent.lock() else {
            return false;
        };
        if *sent {
            false
        } else {
            *sent = true;
            true
        }
    }

    fn mark_running(&self) {
        if let Ok(mut state) = self.state.lock()
            && matches!(
                *state,
                ProviderControlState::Starting | ProviderControlState::Accepted
            )
        {
            *state = ProviderControlState::Running;
        }
    }

    fn mark_accepted(&self) {
        if let Ok(mut state) = self.state.lock()
            && *state == ProviderControlState::Starting
        {
            *state = ProviderControlState::Accepted;
        }
    }

    fn mark_stopped(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = ProviderControlState::Stopped;
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
    cancel_pending_prompt_interactions(attempt_dir, current_timestamp())?;
    AdapterConnectionManager::shared().cancel_attempt_prompt(attempt_dir)
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
}

impl RuntimeStopProbe {
    fn is_stopped(&self) -> bool {
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

#[derive(Debug, Clone)]
pub struct AcpPromptRun {
    pub session_id: String,
    pub adapter_id: String,
    pub adapter_display_name: String,
    pub stop_reason: Option<String>,
    pub terminal_failure: Option<AcpPromptFailure>,
    pub final_text: String,
    pub final_outputs: Vec<String>,
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

#[derive(Debug, Clone, Default)]
struct AcpPromptLifecycle {
    last_error: Option<AcpPromptFailure>,
    terminal_failure: Option<AcpPromptFailure>,
}

impl AcpPromptLifecycle {
    fn reset(&mut self) {
        self.last_error = None;
        self.terminal_failure = None;
    }

    fn observe_session_update(&mut self, update: &Value) {
        if let Some(error) = update.pointer("/_meta/codex/error") {
            let failure = codex_prompt_failure(error);
            let is_terminal = error.get("willRetry").and_then(Value::as_bool) == Some(false);
            self.last_error = Some(failure.clone());
            if is_terminal {
                self.terminal_failure = Some(failure);
            }
        }

        let thread_status = update
            .pointer("/_meta/codex/threadStatus/type")
            .and_then(Value::as_str)
            .map(normalize_stop_code);
        if thread_status.as_deref() == Some("systemerror") {
            let last_error = self.last_error.clone();
            let message = last_error
                .as_ref()
                .map(|failure| failure.message.clone())
                .unwrap_or_else(|| "ACP session entered systemError".to_string());
            let details = last_error
                .as_ref()
                .and_then(|failure| failure.details.clone());
            self.terminal_failure = Some(AcpPromptFailure {
                code: "acp.session-system-error".to_string(),
                message,
                details,
                raw: json!({
                    "terminalUpdate": update,
                    "lastError": last_error.map(|failure| failure.raw),
                }),
            });
        }
    }
}

fn codex_prompt_failure(error: &Value) -> AcpPromptFailure {
    let message = error
        .get("additionalDetails")
        .and_then(Value::as_str)
        .or_else(|| error.get("message").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Codex ACP reported a prompt error")
        .to_string();
    let details = error
        .get("message")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && *value != message)
        .map(str::to_string);
    let code = error
        .get("codexErrorInfo")
        .and_then(Value::as_object)
        .and_then(|info| info.keys().next())
        .map(|code| format!("codex.{code}"))
        .unwrap_or_else(|| "codex.prompt-error".to_string());
    AcpPromptFailure {
        code,
        message,
        details,
        raw: error.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpRuntimePolicy {
    pub foreground_lease_ttl: Duration,
    pub foreground_lease_renew_interval: Duration,
    pub session_idle_ttl: Duration,
    pub adapter_connection_idle_ttl: Duration,
    pub max_idle_session_runtimes: usize,
    pub max_idle_adapter_connections: usize,
    pub timeline_compaction: TimelineCompactionPolicy,
    pub external_session_sync_enabled: bool,
}

impl Default for AcpRuntimePolicy {
    fn default() -> Self {
        Self {
            foreground_lease_ttl: Duration::from_secs(90),
            foreground_lease_renew_interval: Duration::from_secs(30),
            session_idle_ttl: Duration::from_secs(600),
            adapter_connection_idle_ttl: Duration::from_secs(600),
            max_idle_session_runtimes: 8,
            max_idle_adapter_connections: 4,
            timeline_compaction: TimelineCompactionPolicy::default(),
            external_session_sync_enabled: false,
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
        }
    }
}

impl AcpRuntimePolicy {
    pub fn with_external_session_sync_enabled(mut self, enabled: bool) -> Self {
        self.external_session_sync_enabled = enabled;
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
    provider_id: String,
    paths: AcpAttemptPaths,
    connection_key: Option<AdapterConnectionKey>,
    connection: Arc<AdapterConnection>,
    rx: Option<Arc<SessionEventPump>>,
    seq: u64,
    timeline_revision: u64,
    timeline_store: TimelineStore,
    timeline_items: HashMap<String, AcpUiEvent>,
    session_id: Option<String>,
    final_text: String,
    final_outputs: Vec<String>,
    collecting_text_output: bool,
    prompt_lifecycle: AcpPromptLifecycle,
    session_update_phase: SessionUpdatePhase,
    provider_history_replay: ProviderHistoryReplay,
    historical_timeline_item_ids: HashSet<String>,
    current_turn_item_ids: HashSet<String>,
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
    active_text_stream: Option<AcpTimelineStreamState>,
    active_thought_stream: Option<AcpTimelineStreamState>,
    active_plan_stream: Option<AcpTimelineStreamState>,
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
    attached_config_fingerprint: Option<u64>,
    provider_freshness: ProviderFreshnessBaseline,
    sync_required: bool,
    retain_session_route: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionUpdatePhase {
    Live,
    Replaying,
    AwaitingTurnStart,
}

#[derive(Debug, Clone)]
pub struct AcpDoctorProbe {
    pub capabilities: Value,
    pub commands: Vec<AcpCommandItem>,
}

pub fn doctor(
    config: &AcpAdapterConfig,
    cwd: Utf8PathBuf,
    use_local_claude: bool,
    require_local_claude_executable: bool,
) -> Result<AcpDoctorProbe> {
    let paths = GoldBandPaths::new(cwd.clone());
    let doctor_acp_dir = paths.doctor_acp_dir();
    cleanup_doctor_acp_dir_before_run(&doctor_acp_dir);
    let mut runtime = AcpRuntime::start_standalone(
        "doctor",
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
            "doctor",
            cwd,
            None,
            None,
            None,
            &BTreeMap::new(),
            "",
            false,
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
    live_update: Option<&dyn Fn(&AcpUiEvent) -> Result<()>>,
    mcp_servers: &[Value],
    session_update: Option<&dyn Fn() -> Result<()>>,
    stop_probe: Option<RuntimeStopProbe>,
) -> Result<AcpPromptRun> {
    let run_prompt_started_at = Instant::now();
    let prompt_lock = AcpSessionRuntimeRegistry::shared().prompt_lock(&attempt_dir);
    let _prompt_guard = prompt_lock
        .lock()
        .map_err(|_| anyhow!("ACP session prompt lock poisoned"))?;
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
        &mcp_preparation.accepted,
        &mcp_preparation.skipped,
    ) {
        Ok(restored) => restored,
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
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
    runtime.write_worker_ref(provider_id, &workspace_dir, session_mode, restored, None)?;
    let prompt_turn = runtime.record_user_prompt_event(provider_id, prompt, restored)?;
    runtime.control.mark_accepted();
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
    let terminal_failure = runtime.prompt_lifecycle.terminal_failure.clone();
    let (status, stop_reason) = match prompt_result {
        Ok(stop_reason) => {
            let status = if terminal_failure.is_some() {
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
            )?;
            runtime.control.mark_stopped();
            runtime.write_session("failed", restored, Some("error".to_string()), capabilities)?;
            if let Some(session_update) = session_update {
                let _ = session_update();
            }
            runtime.shutdown();
            return Err(error);
        }
    };
    runtime.write_worker_ref(
        provider_id,
        &workspace_dir,
        session_mode,
        restored,
        stop_reason.clone(),
    )?;
    runtime
        .interrupt_active_context_compaction(stop_reason.as_deref().unwrap_or("prompt_finished"))?;
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
        final_text: runtime.final_text.clone(),
        final_outputs: runtime.final_outputs.clone(),
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
    runtime.release_managed_session();
    Ok(run)
}

fn cancel_pending_prompt_interactions(attempt_dir: &Utf8Path, decided_at: String) -> Result<()> {
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
) -> Value {
    let mut prompt_blocks: Vec<Value> = Vec::new();

    // Add attachment content blocks first (images, resources)
    for block in &prompt.content_blocks {
        prompt_blocks.push(serde_json::to_value(block).unwrap_or_default());
    }

    // Add the text block with user prompt
    let text = session_prompt_text(provider_id, prompt, restored);
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

fn session_prompt_text(provider_id: &str, prompt: &PromptBundle, restored: bool) -> String {
    if !restored
        && !supports_system_prompt(provider_id).unwrap_or(false)
        && !prompt.system_prompt.trim().is_empty()
    {
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
    let Ok(meta) = read_json::<crate::acp::events::AcpSessionMetadata>(snapshot_path) else {
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
        let timeline_items = load_timeline_items(&self.paths.timeline)?;
        self.timing_state = AcpTimingState::from_timeline_item_refs(&timeline_items);
        (
            self.active_text_stream,
            self.active_thought_stream,
            self.active_plan_stream,
        ) = active_timeline_streams(&timeline_items);
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
        provider_id: &str,
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
        let timeline_store =
            TimelineStore::open(paths.timeline.clone(), runtime_policy.timeline_compaction)?;
        let seq = initial_acp_source_seq(&paths);
        let loaded_timeline_items = load_timeline_items(&paths.timeline)?;
        let timing_state = AcpTimingState::from_timeline_item_refs(&loaded_timeline_items);
        let provider_history_replay = ProviderHistoryReplay::from_timeline(&loaded_timeline_items);
        let (active_text_stream, active_thought_stream, active_plan_stream) =
            active_timeline_streams(&loaded_timeline_items);
        let context_compaction = active_context_compaction(&loaded_timeline_items);
        let historical_timeline_item_ids = loaded_timeline_items
            .iter()
            .map(|item| item.id.clone())
            .collect();
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
            provider_id: provider_id.to_string(),
            paths,
            connection_key,
            connection,
            rx: None,
            seq,
            timeline_revision,
            timeline_store,
            timeline_items,
            session_id: None,
            final_text: String::new(),
            final_outputs: Vec::new(),
            collecting_text_output: false,
            prompt_lifecycle: AcpPromptLifecycle::default(),
            session_update_phase: SessionUpdatePhase::Live,
            provider_history_replay,
            historical_timeline_item_ids,
            current_turn_item_ids: HashSet::new(),
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
            active_text_stream,
            active_thought_stream,
            active_plan_stream,
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
            final_text: self.final_text.clone(),
            final_outputs: self.final_outputs.clone(),
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
        mcp_servers: &[Value],
        skipped_mcp_servers: &[SkippedAcpMcpServer],
    ) -> Result<bool> {
        let adapter_system_prompt = if supports_system_prompt(provider_id).unwrap_or(false) {
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
            self.session_update_phase = SessionUpdatePhase::Replaying;
            if self.runtime_policy.external_session_sync_enabled {
                self.provider_history_replay.begin(provider_id, session_id);
            }
            self.record_skipped_mcp_servers(provider_id, skipped_mcp_servers);
            skipped_mcp_diagnostic_recorded = true;
            let load = self.request(
                "session/load",
                session_load_params(&cwd, session_id, adapter_system_prompt, mcp_servers),
            );
            let required_sync = self.sync_required;
            match load {
                Ok(result) => {
                    self.capture_session_config(&result);
                    self.set_session_id(session_id.to_string());
                    self.apply_session_mode_options(permission_mode, model, config_options)?;
                    self.drain_session_replay_until_quiet(session_id)?;
                    self.finish_provider_history_replay(Some(session_id.to_string()))?;
                    self.sync_required = false;
                    self.refresh_provider_freshness_best_effort(&cwd);
                    return Ok(true);
                }
                Err(err) => {
                    self.session_update_phase = SessionUpdatePhase::Live;
                    append_diagnostic(
                        &self.paths.diagnostics,
                        "warn",
                        format!("failed to load ACP session `{session_id}`: {err}"),
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
                        bail!("failed to load existing ACP session for continue: {err}");
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
        let session_new_started_at = Instant::now();
        let session_new_result = self.request(
            "session/new",
            session_new_params(&cwd, adapter_system_prompt, mcp_servers),
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
        self.refresh_provider_freshness_best_effort(&cwd);
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
        self.seq += 1;
        let user_event = user_prompt_event(
            self.seq,
            session_id,
            session_prompt_text(provider_id, prompt, restored),
            prompt.prompt_id.clone(),
            prompt.visibility == PromptVisibility::Hidden,
            prompt.attachment_metas.clone(),
        );
        self.persist_event(&user_event)?;
        append_prompt_started(
            &self.paths.prompt_usage,
            &user_event.id,
            user_event.seq,
            &user_event.timestamp,
        )?;
        Ok(AcpPromptTurnIdentity {
            id: user_event.id,
            seq: user_event.seq,
        })
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
        self.prompt_lifecycle.reset();
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
        if self.session_update_phase == SessionUpdatePhase::Replaying {
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
            session_prompt_params(provider_id, session_id, prompt, restored),
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
                match request.recv_timeout(wait_for) {
                    Ok(value) => {
                        self.append_inbound_frame(&value)?;
                        let result = value.get("result").cloned().unwrap_or_else(|| json!({}));
                        if value.get("error").is_none()
                            && let Some(prompt_usage) =
                                AcpPromptTokenUsage::from_prompt_result(&result)
                        {
                            append_prompt_completed(
                                &self.paths.prompt_usage,
                                &prompt_turn.id,
                                prompt_turn.seq,
                                &current_timestamp(),
                                Some(Value::from(request.id)),
                                &prompt_usage,
                            )?;
                            self.usage.record_prompt_usage(prompt_usage);
                        }
                        self.drain_available_inbound()?;
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
                            break Err(anyhow!(
                                "ACP `session/cancel` timed out after {} seconds",
                                PROMPT_CANCEL_TIMEOUT.as_secs()
                            ));
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

        if self.session_update_phase == SessionUpdatePhase::Replaying {
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

        self.prompt_lifecycle.observe_session_update(&update);

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

        if is_unscoped_codex_diagnostic_update(
            &self.provider_id,
            &self.connection.adapter().args,
            &update,
        ) {
            self.seq += 1;
            let message = session_update_text(&update)
                .filter(|message| !message.trim().is_empty())
                .unwrap_or_else(|| "Codex ACP emitted an unscoped diagnostic".to_string());
            append_diagnostic(
                &self.paths.diagnostics,
                "warn",
                message,
                Some(json!({
                    "code": "codex_acp.warning",
                    "providerId": self.provider_id,
                    "sessionId": session_id,
                    "sourceSeq": self.seq,
                    "update": update,
                })),
            )?;
            return Ok(());
        }

        self.seq += 1;
        let event = normalize_session_update(self.seq, session_id, &update);
        if contributes_to_final_text(&event.kind) {
            if !self.collecting_text_output {
                self.final_outputs.push(String::new());
                self.collecting_text_output = true;
            }
            if let Some(content) = &event.content {
                append_bounded(&mut self.final_text, content, 256_000);
                if let Some(output) = self.final_outputs.last_mut() {
                    append_bounded(output, content, 64_000);
                }
            }
        } else {
            self.collecting_text_output = false;
        }
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
        let response = wait_for_permission_response(&self.paths.attempt_dir, &request_id)?;
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
        if !self.control.mark_cancel_sent() {
            return;
        }
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
        }
    }

    fn handle_elicitation_request(&mut self, value: Value) -> Result<()> {
        self.session_update_phase = SessionUpdatePhase::Live;
        let rpc_id = value
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow!("ACP elicitation request missing JSON-RPC id"))?;
        let params = value
            .get("params")
            .cloned()
            .ok_or_else(|| anyhow!("ACP elicitation request missing params"))?;
        let request = serde_json::from_value::<
            agent_client_protocol_schema::v1::CreateElicitationRequest,
        >(params)
        .context("invalid ACP elicitation/create params")?;

        let elicitation_id = format!("elicit-{}", uuid::Uuid::new_v4().simple());

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
        let response = wait_for_elicitation_response(
            &self.paths.attempt_dir,
            &elicitation_id,
            ELICITATION_DEFAULT_TIMEOUT,
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
            supports_continue_session: true,
            continue_ref: Some(json!({
                "acpSessionId": session_id,
                "adapterId": self.connection.adapter().adapter_id.clone(),
                "adapterDisplayName": self.connection.adapter().display_name.clone(),
                "cwd": workspace_dir.as_str(),
                "snapshotFile": self.paths.snapshot.as_str(),
                "lastStopReason": stop_reason,
                "restored": restored,
            })),
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
        let created_at = if self.paths.snapshot.exists() {
            read_json::<AcpSessionMetadata>(&self.paths.snapshot)
                .map(|session| session.created_at)
                .unwrap_or_else(|_| now.clone())
        } else if self.paths.session.exists() {
            read_json::<AcpSessionMetadata>(&self.paths.session)
                .map(|session| session.created_at)
                .unwrap_or_else(|_| now.clone())
        } else {
            now.clone()
        };
        AcpSessionMetadata {
            adapter_id: self.connection.adapter().adapter_id.clone(),
            adapter_display_name: self.connection.adapter().display_name.clone(),
            cwd: self.paths.attempt_dir.to_string(),
            title: self.session_title.clone(),
            status: status.to_string(),
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
        if should_write_legacy_events(&self.paths) {
            append_ui_event(&self.paths.events, event)?;
        }
        let mut timeline_item = self.timeline_item_for_event(event);
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
        if !matches!(
            self.timeline_store.upsert(revision, item)?,
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
        });
        if let Some(content) = item.content.as_deref() {
            if should_separate_streaming_thought_chunks(
                item.kind.as_str(),
                &stream.content,
                content,
            ) {
                append_bounded(&mut stream.content, "\n\n", max_chars);
            }
            append_bounded(&mut stream.content, content, max_chars);
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
        let timestamp = item.timestamp.clone();
        let seq = item.seq;
        match item.kind.as_str() {
            "textDelta" => {
                let stable_id = stable_message_item_id(&item);
                let source_id = stable_message_stream_identity(&item);
                Self::apply_streaming_delta(
                    &mut self.active_text_stream,
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
                    &mut self.active_thought_stream,
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
                    &mut self.active_plan_stream,
                    &mut item,
                    &stable_id,
                    source_id.as_deref(),
                    64_000,
                    seq,
                    &timestamp,
                );
            }
            "contextCompaction" => {
                self.active_text_stream = None;
                self.active_thought_stream = None;
                self.active_plan_stream = None;
                self.apply_context_compaction_event(&mut item, seq, &timestamp);
            }
            "usageUpdate" => {
                item.id = item
                    .session_id
                    .as_deref()
                    .map(|session_id| format!("session-usage-{session_id}"))
                    .unwrap_or_else(|| "session-usage-current".to_string());
                Self::finalize_non_streaming_event(
                    (
                        &mut self.active_text_stream,
                        &mut self.active_thought_stream,
                        &mut self.active_plan_stream,
                    ),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
            "toolCall" | "toolCallUpdate" => {
                if let Some(tool_call_id) = item.tool_call_id.clone() {
                    item.id = format!("tool-call-{tool_call_id}");
                }
                // Merge rawInput from the previous event for this tool call
                // if the new event doesn't carry it. The adapter typically sends
                // rawInput on an intermediate toolCallUpdate but not on the
                // final completed event, so without merging the input is lost.
                if let Some(prev) = self.timeline_items.get(&item.id) {
                    merge_tool_raw_input(&mut item, prev);
                }
                item.kind = "toolCall".to_string();
                Self::finalize_non_streaming_event(
                    (
                        &mut self.active_text_stream,
                        &mut self.active_thought_stream,
                        &mut self.active_plan_stream,
                    ),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
            "permissionRequest" => {
                item.id = format!("permission-{}", item.id);
                Self::finalize_non_streaming_event(
                    (
                        &mut self.active_text_stream,
                        &mut self.active_thought_stream,
                        &mut self.active_plan_stream,
                    ),
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
                    (
                        &mut self.active_text_stream,
                        &mut self.active_thought_stream,
                        &mut self.active_plan_stream,
                    ),
                    &mut item,
                    seq,
                    &timestamp,
                );
            }
        }
        // Clear streams whose kind no longer matches — the next delta of a
        // different kind will create a fresh stream with a new stable id.
        if item.kind != "textDelta" {
            self.active_text_stream = None;
        }
        if item.kind != "thoughtDelta" {
            self.active_thought_stream = None;
        }
        if item.kind != "plan" {
            self.active_plan_stream = None;
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

fn is_non_empty_object(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => !map.is_empty(),
        _ => false,
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

/// Merge `raw.rawInput` and `raw.title` from a previous tool-call timeline
/// item into the current one when the current item doesn't have a non-empty
/// value. This preserves tool input across adapter updates that overwrite the
/// timeline slot — the final "completed" event often carries the output
/// but no longer carries the input or title.
fn merge_tool_raw_input(
    new_item: &mut crate::acp::events::AcpUiEvent,
    prev: &crate::acp::events::AcpUiEvent,
) {
    let new_raw = match &new_item.raw {
        Some(v) => v,
        None => return,
    };
    let prev_raw = match &prev.raw {
        Some(v) => v,
        None => return,
    };
    // Merge title if new event lacks one.
    if new_item.title.is_none() {
        if let Some(prev_title) = &prev.title {
            new_item.title = Some(prev_title.clone());
        }
    }
    // Merge rawInput.
    let new_direct = new_raw.get("rawInput");
    let new_nested = new_raw.get("toolCall").and_then(|tc| tc.get("rawInput"));
    if new_direct.map_or(false, |v| is_non_empty_object(v))
        || new_nested.map_or(false, |v| is_non_empty_object(v))
    {
        return;
    }
    if let Some(prev_direct) = prev_raw.get("rawInput") {
        if is_non_empty_object(prev_direct) {
            if let Some(raw_mut) = new_item.raw.as_mut() {
                raw_mut["rawInput"] = prev_direct.clone();
            }
            return;
        }
    }
    if let Some(prev_nested) = prev_raw.get("toolCall").and_then(|tc| tc.get("rawInput")) {
        if is_non_empty_object(prev_nested) {
            if let Some(raw_mut) = new_item.raw.as_mut() {
                if let Some(tc_mut) = raw_mut.get_mut("toolCall") {
                    tc_mut["rawInput"] = prev_nested.clone();
                } else {
                    let mut tc = serde_json::Map::new();
                    tc.insert("rawInput".to_string(), prev_nested.clone());
                    raw_mut["toolCall"] = serde_json::Value::Object(tc);
                }
            }
        }
    }
}

fn should_write_legacy_events(paths: &AcpAttemptPaths) -> bool {
    paths.events.exists() && !paths.timeline.exists()
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

fn active_timeline_streams(
    items: &[crate::acp::events::AcpUiEvent],
) -> (
    Option<AcpTimelineStreamState>,
    Option<AcpTimelineStreamState>,
    Option<AcpTimelineStreamState>,
) {
    let Some(item) = items
        .iter()
        .max_by_key(|item| (item.ended_seq.unwrap_or(item.seq), item.seq))
    else {
        return (None, None, None);
    };
    let stream = || AcpTimelineStreamState {
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
        content: item.content.clone().unwrap_or_default(),
    };
    match item.kind.as_str() {
        "textDelta" => (Some(stream()), None, None),
        "thoughtDelta" => (None, Some(stream()), None),
        "plan" => (None, None, Some(stream())),
        _ => (None, None, None),
    }
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
    if paths.timeline.exists() || !paths.events.exists() {
        latest_timeline_source_seq(&paths.timeline)
    } else {
        initial_acp_event_seq(&paths.events)
    }
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
        .map(|message_id| format!("assistant-message-{message_id}"))
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
        SessionUpdatePhase::Replaying => true,
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
        .map(|message_id| format!("assistant-thought-{message_id}"))
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
        .map(|session_id| format!("session-plan-{session_id}"))
}

fn provider_history_item_id(event: &crate::acp::events::AcpUiEvent) -> Option<&str> {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("providerHistoryItemId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn contributes_to_final_text(kind: &str) -> bool {
    kind == "textDelta"
}

fn is_unscoped_codex_diagnostic_update(
    provider_id: &str,
    adapter_args: &[String],
    update: &Value,
) -> bool {
    provider_id == "codex-acp"
        && adapter_args
            .iter()
            .any(|arg| arg.starts_with("@agentclientprotocol/codex-acp"))
        && update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
        && update
            .get("messageId")
            .and_then(Value::as_str)
            .is_none_or(|message_id| message_id.trim().is_empty())
        && update.get("providerHistoryItemId").is_none()
}

fn session_update_text(update: &Value) -> Option<String> {
    update
        .pointer("/content/text")
        .and_then(Value::as_str)
        .or_else(|| {
            update
                .pointer("/content/content/text")
                .and_then(Value::as_str)
        })
        .or_else(|| update.get("text").and_then(Value::as_str))
        .map(str::to_string)
}

fn append_bounded(target: &mut String, content: &str, max_chars: usize) {
    if target.chars().count() >= max_chars {
        return;
    }
    let remaining = max_chars - target.chars().count();
    if content.chars().count() <= remaining {
        target.push_str(content);
        return;
    }
    target.extend(content.chars().take(remaining));
    target.push('…');
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

    use serde_json::{Value, json};

    use super::{
        AcpContextCompactionState, AcpPromptLifecycle, AcpPromptTokenUsage, AcpRuntime,
        AcpRuntimePolicy, AcpUsageState, AttachedSessionReusePlan, DOCTOR_DIAGNOSTIC_TARGET_SIZE,
        PriorAttemptMetrics, PromptActivity, PromptBundle, PromptVisibility,
        ProviderFreshnessBaseline, RuntimeStopProbe, SessionModelResolution, SessionUpdatePhase,
        active_context_compaction, active_timeline_streams, attached_sync_required,
        cleanup_doctor_acp_dir_after_success, contributes_to_final_text, drain_frames_until_quiet,
        evaluate_provider_revision, initialize_params, is_transport_interruption,
        is_unscoped_codex_diagnostic_update, merge_tool_raw_input,
        permission_decision_timeline_event, plan_attached_session_reuse, prompt_activity,
        register_provider_control, request_prompt_cancel,
        resolve_permission_mode, resolve_session_model, retain_bounded_doctor_acp_failure_bundle,
        runtime_hot_timeline_items, session_config_fingerprint, session_load_params,
        session_new_params, session_prompt_params, session_prompt_text,
        should_suppress_session_update, take_pending_live_update_for_stream_switch,
        unregister_provider_control,
    };
    use crate::acp::{
        connection::AcpConnectionUnavailable,
        events::{AcpTimingState, AcpUiEvent},
        permission::PermissionResponseState,
    };
    use crate::provider::prepare_acp_mcp_servers;

    #[test]
    fn initialize_keeps_private_subagent_capabilities_out_of_the_wire_contract() {
        let params = initialize_params();

        assert!(params.pointer("/clientCapabilities/_meta").is_none());
        assert!(
            params
                .pointer("/clientCapabilities/elicitation/form")
                .is_some()
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
    fn prompt_lifecycle_promotes_retry_error_when_session_becomes_system_error() {
        let mut lifecycle = AcpPromptLifecycle::default();
        lifecycle.observe_session_update(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": {
                "codex": {
                    "error": {
                        "message": "Reconnecting... 5/5",
                        "additionalDetails": "We're currently experiencing high demand, which may cause temporary errors.",
                        "codexErrorInfo": {
                            "responseStreamDisconnected": { "httpStatusCode": null }
                        },
                        "willRetry": true
                    }
                }
            }
        }));
        assert!(lifecycle.terminal_failure.is_none());

        lifecycle.observe_session_update(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": {
                "codex": {
                    "threadStatus": { "type": "systemError" }
                }
            }
        }));

        let failure = lifecycle.terminal_failure.unwrap();
        assert_eq!(failure.code, "acp.session-system-error");
        assert!(failure.message.contains("high demand"));
        assert_eq!(failure.details.as_deref(), Some("Reconnecting... 5/5"));
    }

    #[test]
    fn prompt_lifecycle_does_not_promote_recovered_retry_signal() {
        let mut lifecycle = AcpPromptLifecycle::default();
        lifecycle.observe_session_update(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": {
                "codex": {
                    "error": {
                        "message": "Reconnecting... 1/5",
                        "willRetry": true
                    }
                }
            }
        }));
        lifecycle.observe_session_update(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": {
                "codex": {
                    "threadStatus": { "type": "active", "activeFlags": [] }
                }
            }
        }));

        assert!(lifecycle.terminal_failure.is_none());
    }

    #[test]
    fn prompt_lifecycle_promotes_non_retryable_error_immediately() {
        let mut lifecycle = AcpPromptLifecycle::default();
        lifecycle.observe_session_update(&json!({
            "sessionUpdate": "session_info_update",
            "_meta": {
                "codex": {
                    "error": {
                        "message": "Request failed",
                        "additionalDetails": "Provider rejected the prompt",
                        "willRetry": false
                    }
                }
            }
        }));

        let failure = lifecycle.terminal_failure.expect("terminal failure");
        assert_eq!(failure.code, "codex.prompt-error");
        assert_eq!(failure.message, "Provider rejected the prompt");
        assert_eq!(failure.details.as_deref(), Some("Request failed"));
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
        let mut phase = SessionUpdatePhase::Replaying;
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
        let mut phase = SessionUpdatePhase::Replaying;
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
        assert_eq!(phase, SessionUpdatePhase::Replaying);
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
    fn terminal_tool_update_preserves_intermediate_raw_input_before_release() {
        let intermediate = timeline_event(
            "tool-call-1",
            1,
            "toolCall",
            Some("in_progress"),
            None,
            Some(json!({ "rawInput": { "path": "report.md" } })),
        );
        let mut terminal = timeline_event(
            "tool-call-1",
            2,
            "toolCall",
            Some("completed"),
            None,
            Some(json!({ "content": "done" })),
        );

        merge_tool_raw_input(&mut terminal, &intermediate);

        assert_eq!(
            terminal
                .raw
                .as_ref()
                .and_then(|raw| raw.get("rawInput"))
                .cloned(),
            Some(json!({ "path": "report.md" }))
        );
        assert!(runtime_hot_timeline_items(vec![terminal]).is_empty());
    }

    #[test]
    fn final_text_ignores_user_prompt_deltas() {
        assert!(contributes_to_final_text("textDelta"));
        assert!(!contributes_to_final_text("userTextDelta"));
        assert!(!contributes_to_final_text("thoughtDelta"));
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
    fn agentclientprotocol_codex_unscoped_text_is_a_diagnostic() {
        let warning = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "provider warning" }
        });
        let answer = json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": "answer-1",
            "content": { "type": "text", "text": "answer" }
        });
        let current_args = vec![
            "-y".to_string(),
            "@agentclientprotocol/codex-acp@latest".to_string(),
        ];
        let legacy_args = vec![
            "-y".to_string(),
            "@zed-industries/codex-acp@latest".to_string(),
        ];

        assert!(is_unscoped_codex_diagnostic_update(
            "codex-acp",
            &current_args,
            &warning,
        ));
        assert!(!is_unscoped_codex_diagnostic_update(
            "codex-acp",
            &current_args,
            &answer,
        ));
        assert!(!is_unscoped_codex_diagnostic_update(
            "codex-acp",
            &legacy_args,
            &warning,
        ));
        assert!(!is_unscoped_codex_diagnostic_update(
            "claude-acp",
            &current_args,
            &warning,
        ));
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
            Some("**Designing routes**\n\n**Planning branches**")
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

        assert_eq!(new_params["mcpServers"], json!([servers[0], servers[1]]));
        assert_eq!(load_params["mcpServers"], json!([servers[0], servers[1]]));
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
    }

    #[test]
    fn codex_session_prompt_inlines_system_prompt() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "do the task".to_string(),
            prompt_id: Some("prompt-001".to_string()),
            visibility: PromptVisibility::Visible,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        let text = session_prompt_text("codex-acp", &prompt, false);
        assert!(text.contains(
            "<hidden data-gold-band-hidden=\"true\" title=\"Gold Band stable system prompt\">"
        ));
        assert!(text.contains("node constraints"));
        assert!(text.ends_with("do the task"));

        let params = session_prompt_params("codex-acp", "session-123", &prompt, false);
        assert_eq!(params["sessionId"], "session-123");
        assert_eq!(params["prompt"][0]["text"], text);
    }

    #[test]
    fn codex_restored_session_prompt_does_not_inline_system_prompt() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "follow up".to_string(),
            prompt_id: Some("prompt-002".to_string()),
            visibility: PromptVisibility::Visible,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        let text = session_prompt_text("codex-acp", &prompt, true);
        assert_eq!(text, "follow up");
        assert!(!text.contains("Gold Band stable system prompt"));
        assert!(!text.contains("node constraints"));

        let params = session_prompt_params("codex-acp", "session-123", &prompt, true);
        assert_eq!(params["sessionId"], "session-123");
        assert_eq!(params["prompt"][0]["text"], "follow up");
    }

    #[test]
    fn claude_session_prompt_keeps_user_prompt_only() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "do the task".to_string(),
            prompt_id: None,
            visibility: PromptVisibility::Visible,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        assert_eq!(
            session_prompt_text("claude-acp", &prompt, false),
            "do the task"
        );
        assert_eq!(
            session_prompt_text("claude-acp", &prompt, true),
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
            run_file: run_file.clone(),
            round_id: "round-001".to_string(),
            node_id: "ai-dynamic1".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: None,
        };
        let inner_probe = RuntimeStopProbe {
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
            run_file: run_file.clone(),
            round_id: "round-001".to_string(),
            node_id: "ai-dynamic".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: Some(own_state),
        };
        let paused_leaf_probe = RuntimeStopProbe {
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
            run_file,
            round_id: "round-001".to_string(),
            node_id: "plan".to_string(),
            attempt_id: "attempt-001".to_string(),
            attempt_state_file: Some(manual_check_state),
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
