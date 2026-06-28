use std::collections::HashMap;
use std::sync::{
    Arc, LazyLock, Mutex,
    mpsc::{Receiver, RecvTimeoutError, TryRecvError},
};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::{Value, json};
use tracing::debug;

#[derive(Debug, Clone)]
struct AcpTimelineStreamState {
    item_id: String,
    started_seq: u64,
    started_at: String,
    content: String,
}

use crate::acp::connection::{AdapterConnection, AdapterConnectionKey, AdapterConnectionManager};
use crate::acp::elicitation::{
    ELICITATION_DEFAULT_TIMEOUT, PendingElicitationState, cancel_pending_elicitation_requests,
    elicitation_response_result, wait_for_elicitation_response, write_pending_elicitation,
};
use crate::acp::events::{
    AcpAttemptPaths, AcpSessionMetadata, AcpUiEvent, append_diagnostic, append_raw_frame,
    append_timeline_patch, append_ui_event, current_timestamp, initial_acp_event_seq,
    latest_timeline_source_seq, load_timeline_items, normalize_session_update,
    permission_request_event, user_prompt_event, write_session_metadata, write_timeline_items,
};
use crate::acp::permission::{
    acp_permission_response_result, wait_for_permission_response, write_pending_permission,
};
use crate::config::AcpAdapterConfig;
use crate::domain::{SessionMode, VERSION};
use crate::provider::{
    PromptBundle, PromptVisibility, gold_band_hidden_block, supports_system_prompt,
};
use crate::runtime::{WorkerRefState, validate_worker_ref_state};
use crate::storage::{GoldBandPaths, ensure_parent_dir, read_json, roll_jsonl, write_json};

const STOP_CHECK_INTERVAL: Duration = Duration::from_millis(100);
const LIVE_STREAM_UPDATE_INTERVAL: Duration = Duration::from_millis(75);
const TIMELINE_COMPACT_EVERY_REVISIONS: u64 = 128;
const DOCTOR_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const DOCTOR_DIAGNOSTIC_MAX_SIZE: u64 = 512 * 1024;
const DOCTOR_DIAGNOSTIC_TARGET_SIZE: u64 = 384 * 1024;
const SESSION_TITLE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const PROMPT_CANCEL_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct AcpCancelled;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderControlState {
    Running,
    CancelRequested,
    Stopped,
}

#[derive(Debug)]
struct ProviderControl {
    state: Mutex<ProviderControlState>,
    cancel_sent: Mutex<bool>,
}

impl ProviderControl {
    fn new() -> Self {
        Self {
            state: Mutex::new(ProviderControlState::Running),
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
            ProviderControlState::Running => {
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

pub fn cancel_attempt_prompt(attempt_dir: &Utf8Path) -> Result<bool> {
    AdapterConnectionManager::shared().cancel_attempt_prompt(attempt_dir)
}

pub fn close_attempt_session_bounded(attempt_dir: &Utf8Path) -> Result<bool> {
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

pub fn close_provider_workspace_bounded(
    provider_id: &str,
    workspace_root: &Utf8Path,
) -> Result<()> {
    AdapterConnectionManager::shared().close_provider_workspace_bounded(
        provider_id,
        workspace_root,
        SESSION_CLOSE_TIMEOUT,
    )
}

pub fn has_active_prompts_in_workspace(workspace_root: &Utf8Path) -> bool {
    AdapterConnectionManager::shared().has_active_prompts_in_workspace(workspace_root)
}

pub fn has_active_prompts_in_provider_workspace(
    provider_id: &str,
    workspace_root: &Utf8Path,
) -> bool {
    AdapterConnectionManager::shared()
        .has_active_prompts_in_provider_workspace(provider_id, workspace_root)
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
    pub final_text: String,
    pub final_outputs: Vec<String>,
    pub restored: bool,
    pub used_tokens: Option<u64>,
    pub context_window_size: Option<u64>,
    pub total_cost_usd: Option<f64>,
    pub accumulated_used_tokens: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_read_tokens: Option<u64>,
    pub cached_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

struct AcpRuntime<'a> {
    paths: AcpAttemptPaths,
    connection_key: Option<AdapterConnectionKey>,
    connection: Arc<AdapterConnection>,
    rx: Option<Receiver<Value>>,
    seq: u64,
    timeline_revision: u64,
    timeline_items: HashMap<String, AcpUiEvent>,
    session_id: Option<String>,
    final_text: String,
    final_outputs: Vec<String>,
    collecting_text_output: bool,
    suppress_session_updates: bool,
    models: Option<Value>,
    modes: Option<Value>,
    config_options: Option<Value>,
    system_prompt_append: Option<String>,
    session_title: Option<String>,
    used_tokens: Option<u64>,
    context_window_size: Option<u64>,
    total_cost_usd: Option<f64>,
    accumulated_used_tokens: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cached_read_tokens: Option<u64>,
    cached_write_tokens: Option<u64>,
    total_tokens: Option<u64>,
    active_text_stream: Option<AcpTimelineStreamState>,
    active_thought_stream: Option<AcpTimelineStreamState>,
    active_plan_stream: Option<AcpTimelineStreamState>,
    live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
    pending_live_update: Option<AcpUiEvent>,
    last_live_update_at: Option<Instant>,
    pending_timeline_patch: Option<(u64, AcpUiEvent)>,
    last_timeline_patch_at: Option<Instant>,
    raw_max_size: u64,
    raw_target_size: u64,
    control: Arc<ProviderControl>,
    stop_probe: Option<RuntimeStopProbe>,
}

pub fn doctor(
    config: &AcpAdapterConfig,
    cwd: Utf8PathBuf,
    use_local_claude: bool,
) -> Result<Value> {
    let paths = GoldBandPaths::new(cwd.clone());
    let doctor_acp_dir = paths.doctor_acp_dir();
    cleanup_doctor_acp_dir_before_run(&doctor_acp_dir);
    let mut runtime = AcpRuntime::start_standalone(
        "doctor",
        config,
        cwd.clone(),
        doctor_acp_dir.clone(),
        use_local_claude,
        DOCTOR_DIAGNOSTIC_MAX_SIZE,
        DOCTOR_DIAGNOSTIC_TARGET_SIZE,
        None,
        None,
    )?;
    let result = (|| {
        let mut capabilities = runtime.initialize_with_timeout(Some(DOCTOR_REQUEST_TIMEOUT))?;
        runtime.setup_session("doctor", cwd, None, None, None, "", false, &[])?;
        runtime.cleanup_diagnostic_session()?;
        runtime.merge_session_config_into_capabilities(&mut capabilities);
        Ok(capabilities)
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
    continue_ref: Option<Value>,
    use_local_claude: bool,
    acp_session_title_refresh_enabled: bool,
    acp_raw_max_size_bytes: u64,
    acp_raw_target_size_bytes: u64,
    live_update: Option<&dyn Fn(&AcpUiEvent) -> Result<()>>,
    mcp_servers: &[Value],
    session_update: Option<&dyn Fn() -> Result<()>>,
    stop_probe: Option<RuntimeStopProbe>,
) -> Result<AcpPromptRun> {
    let mut runtime = AcpRuntime::start(
        provider_id,
        config,
        adapter_workspace_dir,
        attempt_dir,
        use_local_claude,
        acp_raw_max_size_bytes,
        acp_raw_target_size_bytes,
        live_update,
        stop_probe,
    )?;
    let capabilities = match runtime.initialize() {
        Ok(capabilities) => capabilities,
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            let run = runtime.interrupted_run(false, "cancelled");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) if error.downcast_ref::<AcpTransportInterrupted>().is_some() => {
            let run = runtime.interrupted_run(false, "interrupted");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) => return Err(error),
    };
    let strict_continue = session_mode == SessionMode::Continue && continue_ref.is_some();
    let restored = match runtime.setup_session(
        provider_id,
        workspace_dir.clone(),
        continue_ref,
        permission_mode.as_deref(),
        model.as_deref(),
        &prompt.system_prompt,
        strict_continue,
        mcp_servers,
    ) {
        Ok(restored) => restored,
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            let run = runtime.interrupted_run(false, "cancelled");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) if error.downcast_ref::<AcpTransportInterrupted>().is_some() => {
            let run = runtime.interrupted_run(false, "interrupted");
            runtime.shutdown();
            return Ok(run);
        }
        Err(error) => return Err(error),
    };
    let session_id = runtime
        .session_id
        .clone()
        .ok_or_else(|| anyhow!("ACP session setup did not return a session id"))?;
    runtime.write_worker_ref(provider_id, &workspace_dir, session_mode, restored, None)?;
    runtime.record_user_prompt_event(provider_id, prompt, session_update.is_none())?;
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
    let prompt_result = runtime.prompt(
        provider_id,
        &workspace_dir,
        prompt,
        restored,
        &capabilities,
        acp_session_title_refresh_enabled,
    );
    let (status, stop_reason) = match prompt_result {
        Ok(stop_reason) => {
            let status = if stop_reason.as_deref().is_some_and(|reason| {
                matches!(
                    normalize_stop_code(reason).as_str(),
                    "cancelled" | "canceled" | "interrupted"
                )
            }) {
                "cancelled"
            } else {
                "completed"
            };
            (status, stop_reason)
        }
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            let _ = cancel_pending_elicitation_requests(
                &runtime.paths.attempt_dir,
                current_timestamp(),
            );
            ("cancelled", Some("cancelled".to_string()))
        }
        Err(error) if error.downcast_ref::<AcpTransportInterrupted>().is_some() => {
            ("cancelled", Some("interrupted".to_string()))
        }
        Err(error) => {
            let _ = cancel_pending_elicitation_requests(
                &runtime.paths.attempt_dir,
                current_timestamp(),
            );
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
    runtime.write_session(status, restored, stop_reason.clone(), capabilities)?;
    if let Some(session_update) = session_update {
        let _ = session_update();
    }
    let run = AcpPromptRun {
        session_id,
        adapter_id: runtime.connection.adapter().adapter_id.clone(),
        adapter_display_name: runtime.connection.adapter().display_name.clone(),
        stop_reason,
        final_text: runtime.final_text.clone(),
        final_outputs: runtime.final_outputs.clone(),
        restored,
        used_tokens: runtime.used_tokens,
        context_window_size: runtime.context_window_size,
        total_cost_usd: runtime.total_cost_usd,
        accumulated_used_tokens: runtime.accumulated_used_tokens,
        input_tokens: runtime.input_tokens,
        output_tokens: runtime.output_tokens,
        cached_read_tokens: runtime.cached_read_tokens,
        cached_write_tokens: runtime.cached_write_tokens,
        total_tokens: runtime.total_tokens,
    };
    runtime.shutdown();
    Ok(run)
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

fn session_prompt_params(provider_id: &str, session_id: &str, prompt: &PromptBundle) -> Value {
    let mut prompt_blocks: Vec<Value> = Vec::new();

    // Add attachment content blocks first (images, resources)
    for block in &prompt.content_blocks {
        prompt_blocks.push(serde_json::to_value(block).unwrap_or_default());
    }

    // Add the text block with user prompt
    let text = session_prompt_text(provider_id, prompt);
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

fn session_prompt_text(provider_id: &str, prompt: &PromptBundle) -> String {
    if !supports_system_prompt(provider_id).unwrap_or(false)
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

impl<'a> AcpRuntime<'a> {
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
        raw_max_size: u64,
        raw_target_size: u64,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
        stop_probe: Option<RuntimeStopProbe>,
    ) -> Result<Self> {
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        ensure_parent_dir(&paths.raw)?;
        ensure_parent_dir(&paths.diagnostics)?;
        let key = AdapterConnectionKey::new(provider_id, cwd.clone());
        let adapter_started_at = Instant::now();
        let resolution = AdapterConnectionManager::shared()
            .get_or_spawn_with_outcome(provider_id, config, cwd.clone(), use_local_claude)
            .map_err(|error| {
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
            raw_max_size,
            raw_target_size,
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
        raw_max_size: u64,
        raw_target_size: u64,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
        stop_probe: Option<RuntimeStopProbe>,
    ) -> Result<Self> {
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        ensure_parent_dir(&paths.raw)?;
        ensure_parent_dir(&paths.diagnostics)?;
        let connection = AdapterConnection::spawn_standalone(config, &cwd, use_local_claude)
            .map_err(|error| {
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
            raw_max_size,
            raw_target_size,
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
        raw_max_size: u64,
        raw_target_size: u64,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
        stop_probe: Option<RuntimeStopProbe>,
    ) -> Result<Self> {
        ensure_parent_dir(&paths.provider_pid)?;
        std::fs::write(
            paths.provider_pid.as_std_path(),
            connection.pid().to_string(),
        )?;
        let control = register_provider_control(&paths.attempt_dir);
        let seq = initial_acp_source_seq(&paths);
        let timeline_items = load_timeline_items(&paths.timeline)?
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect::<HashMap<_, _>>();
        let timeline_revision = seq;
        Ok(Self {
            paths,
            connection_key,
            connection,
            rx: None,
            seq,
            timeline_revision,
            timeline_items,
            session_id: None,
            final_text: String::new(),
            final_outputs: Vec::new(),
            collecting_text_output: false,
            suppress_session_updates: false,
            models: None,
            modes: None,
            config_options: None,
            system_prompt_append: None,
            session_title: None,
            used_tokens: None,
            context_window_size: None,
            total_cost_usd: None,
            accumulated_used_tokens: 0,
            input_tokens: None,
            output_tokens: None,
            cached_read_tokens: None,
            cached_write_tokens: None,
            total_tokens: None,
            active_text_stream: None,
            active_thought_stream: None,
            active_plan_stream: None,
            live_update,
            pending_live_update: None,
            last_live_update_at: None,
            pending_timeline_patch: None,
            last_timeline_patch_at: None,
            raw_max_size,
            raw_target_size,
            control,
            stop_probe,
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
            final_text: self.final_text.clone(),
            final_outputs: self.final_outputs.clone(),
            restored,
            used_tokens: self.used_tokens,
            context_window_size: self.context_window_size,
            total_cost_usd: self.total_cost_usd,
            accumulated_used_tokens: self.accumulated_used_tokens,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cached_read_tokens: self.cached_read_tokens,
            cached_write_tokens: self.cached_write_tokens,
            total_tokens: self.total_tokens,
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
        let result = self.request_with_timeout(
            "initialize",
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
            }),
            timeout,
        )?;
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
        system_prompt: &str,
        strict_continue: bool,
        mcp_servers: &[Value],
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
        if let Some(session_id) = continue_ref
            .as_ref()
            .and_then(|value| value.get("acpSessionId"))
            .and_then(Value::as_str)
        {
            self.suppress_session_updates = true;
            let load = self.request(
                "session/load",
                session_load_params(&cwd, session_id, adapter_system_prompt, mcp_servers),
            );
            self.suppress_session_updates = false;
            match load {
                Ok(result) => {
                    self.capture_session_config(&result);
                    self.set_session_id(session_id.to_string());
                    self.apply_session_mode_options(permission_mode, model)?;
                    return Ok(true);
                }
                Err(err) => {
                    append_diagnostic(
                        &self.paths.diagnostics,
                        "warn",
                        format!("failed to load ACP session `{session_id}`: {err}"),
                        None,
                    )?;
                    if err.downcast_ref::<AcpTransportInterrupted>().is_some() {
                        self.set_session_id(session_id.to_string());
                        return Err(err);
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

        let result = self.request(
            "session/new",
            session_new_params(&cwd, adapter_system_prompt, mcp_servers),
        )?;
        self.capture_session_config(&result);
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("ACP session/new response missing sessionId"))?;
        self.set_session_id(session_id.to_string());
        self.apply_session_mode_options(permission_mode, model)?;
        Ok(false)
    }

    fn set_session_id(&mut self, session_id: String) {
        if let Some(existing) = self.session_id.take() {
            self.connection.unregister_session_route(&existing);
        }
        self.rx = Some(self.connection.register_session_route(&session_id));
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
    ) -> Result<()> {
        if let Some(m) = model.filter(|v| !v.trim().is_empty()) {
            self.set_session_model(m)?;
        }
        if let Some(pm) = permission_mode.filter(|v| !v.trim().is_empty()) {
            self.apply_permission_mode(pm)?;
        }
        Ok(())
    }

    fn set_session_model(&mut self, model: &str) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP model selection requires a session id"))?;
        let model = model.trim();
        if model.is_empty() {
            return Ok(());
        }
        if has_model_config_option(self.config_options.as_ref()) {
            let result = self.request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": "model",
                    "value": model,
                }),
            )?;
            self.capture_session_config(&result);
            self.set_current_model(model);
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
            self.set_current_model(model);
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
            let result = self.request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": "mode",
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

    fn refresh_session_title(&mut self, workspace_dir: &Utf8Path) -> Result<()> {
        let Some(session_id) = self.session_id.clone() else {
            return Ok(());
        };
        let result = self.request(
            "session/list",
            json!({
                "cwd": workspace_dir.as_str(),
            }),
        )?;
        let title = result
            .get("sessions")
            .and_then(Value::as_array)
            .and_then(|sessions| {
                sessions.iter().find(|session| {
                    session.get("sessionId").and_then(Value::as_str) == Some(session_id.as_str())
                })
            })
            .and_then(|session| session.get("title"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string);
        self.session_title = title;
        Ok(())
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
        emit_live_update: bool,
    ) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP prompt requires a session id"))?;
        self.seq += 1;
        let user_event = user_prompt_event(
            self.seq,
            session_id,
            session_prompt_text(provider_id, prompt),
            prompt.prompt_id.clone(),
            prompt.visibility == PromptVisibility::Hidden,
            prompt.attachment_metas.clone(),
        );
        if emit_live_update {
            self.persist_event(&user_event)
        } else {
            self.persist_event_without_live_update(&user_event)
        }
    }

    fn prompt(
        &mut self,
        provider_id: &str,
        workspace_dir: &Utf8Path,
        prompt: &PromptBundle,
        restored: bool,
        capabilities: &Value,
        acp_session_title_refresh_enabled: bool,
    ) -> Result<Option<String>> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP prompt requires a session id"))?;
        let result = self.request_prompt_with_cancel(
            provider_id,
            &session_id,
            prompt,
            acp_session_title_refresh_enabled.then_some((
                workspace_dir,
                "running",
                restored,
                None,
                capabilities,
            )),
        )?;
        // Capture session-end usage breakdown (inputTokens / outputTokens / …)
        // that the adapter returns alongside the stopReason.
        if let Some(usage) = result.get("usage") {
            self.input_tokens = usage.get("inputTokens").and_then(Value::as_u64);
            self.output_tokens = usage.get("outputTokens").and_then(Value::as_u64);
            self.cached_read_tokens = usage.get("cachedReadTokens").and_then(Value::as_u64);
            self.cached_write_tokens = usage.get("cachedWriteTokens").and_then(Value::as_u64);
            self.total_tokens = usage.get("totalTokens").and_then(Value::as_u64);
        }
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
                "method": "session/prompt",
                "sessionId": session_id,
                "providerId": provider_id,
            }),
        );
        let request = self.connection.begin_request(
            "session/prompt",
            session_prompt_params(provider_id, session_id, prompt),
        )?;
        self.append_outbound_frame(&request.frame)?;
        self.connection.mark_prompt_active();
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
                match request.recv_timeout(wait_for) {
                    Ok(value) => {
                        self.append_inbound_frame(&value)?;
                        self.drain_available_inbound()?;
                        if let Some(error) = value.get("error") {
                            if cancel_started_at.is_some() {
                                break Err(anyhow!(AcpCancelled));
                            }
                            break Err(anyhow!("ACP `session/prompt` failed: {error}"));
                        }
                        let result = value.get("result").cloned().unwrap_or_else(|| json!({}));
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
        self.connection.mark_prompt_inactive();
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
        if self.suppress_session_updates {
            return Ok(());
        }
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let update = params.get("update").cloned().unwrap_or(params);

        // Track usage from usage_update events so we can persist them at prompt end.
        if update.get("sessionUpdate").and_then(Value::as_str) == Some("usage_update") {
            let (used, size, cost) = crate::acp::events::extract_usage_fields(&update);
            if let Some(u) = used {
                // Accumulate positive deltas so compaction-driven resets
                // don't lose track of the session's total token spend.
                let prev = self.used_tokens.unwrap_or(0);
                if u > prev {
                    self.accumulated_used_tokens += u - prev;
                }
                self.used_tokens = Some(u);
            }
            if size.is_some() {
                self.context_window_size = size;
            }
            if cost.is_some() {
                self.total_cost_usd = cost;
            }
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
        Ok(())
    }

    fn handle_permission_request(&mut self, value: Value) -> Result<()> {
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
        let event = permission_request_event(self.seq, request_id.clone(), params);
        self.persist_event(&event)?;
        let response = wait_for_permission_response(&self.paths.attempt_dir, &request_id)?;
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
        let rpc_id = value
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow!("ACP elicitation request missing JSON-RPC id"))?;
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let schema = params
            .get("requestedSchema")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let elicitation_id = format!("elicit-{}", uuid::Uuid::new_v4().simple());

        // 1. 持久化请求到 attempt dir
        write_pending_elicitation(
            &self.paths.attempt_dir,
            &PendingElicitationState {
                elicitation_id: elicitation_id.clone(),
                jsonrpc_id: rpc_id.clone(),
                message: message.clone(),
                requested_schema: schema.clone(),
                created_at: current_timestamp(),
            },
        )?;

        // 2. 发送 UI 事件给前端
        self.seq += 1;
        let event = crate::acp::events::elicitation_request_event(
            self.seq,
            elicitation_id.clone(),
            message,
            schema.clone(),
        );
        self.persist_event(&event)?;

        // 3. 同步阻塞等待用户响应（含超时保护）
        let response = wait_for_elicitation_response(
            &self.paths.attempt_dir,
            &elicitation_id,
            ELICITATION_DEFAULT_TIMEOUT,
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

        // 5. 将用户回答格式化为可读文本，作为 userTextDelta 事件写入 timeline
        //    这样前端无需合成事件，直接走正常消息渲染管道：用户头像 + 右侧气泡
        self.seq += 1;
        let answer_text = match &response.action {
            crate::acp::elicitation::ElicitationAction::Accept => {
                crate::acp::elicitation::format_elicitation_answer(
                    &schema,
                    &response.content.clone().unwrap_or_else(|| json!({})),
                )
            }
            crate::acp::elicitation::ElicitationAction::Decline => "已跳过".to_string(),
        };
        let user_delta = crate::acp::events::user_prompt_event(
            self.seq,
            self.session_id.clone().unwrap_or_default(),
            answer_text,
            None,       // prompt_id
            false,      // hidden_from_chat
            Vec::new(), // attachments
        );
        self.persist_event(&user_delta)?;

        Ok(())
    }

    fn drain_available_inbound(&mut self) -> Result<()> {
        loop {
            if self.is_prompt_cancel_requested() {
                self.send_cancel_notification_best_effort();
            }
            let value = match self.rx.as_ref().map(Receiver::try_recv) {
                Some(Ok(value)) => value,
                Some(Err(TryRecvError::Empty)) | None => return Ok(()),
                Some(Err(TryRecvError::Disconnected)) => {
                    return Err(anyhow!(AcpTransportInterrupted));
                }
            };
            self.append_inbound_frame(&value)?;
            self.handle_inbound(value)?;
        }
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
            system_prompt_append: self.system_prompt_append.clone(),
            used_tokens: self.used_tokens,
            context_window_size: self.context_window_size,
            total_cost_usd: self.total_cost_usd,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cached_read_tokens: self.cached_read_tokens,
            cached_write_tokens: self.cached_write_tokens,
            total_tokens: self.total_tokens,
            created_at,
            updated_at: now,
        }
    }

    fn persist_event(&mut self, event: &crate::acp::events::AcpUiEvent) -> Result<()> {
        self.persist_event_inner(event, true)
    }

    fn persist_event_without_live_update(
        &mut self,
        event: &crate::acp::events::AcpUiEvent,
    ) -> Result<()> {
        self.persist_event_inner(event, false)
    }

    fn persist_event_inner(
        &mut self,
        event: &crate::acp::events::AcpUiEvent,
        emit_live_update: bool,
    ) -> Result<()> {
        if should_write_legacy_events(&self.paths) {
            append_ui_event(&self.paths.events, event)?;
        }
        let timeline_item = self.timeline_item_for_event(event);
        self.timeline_items
            .insert(timeline_item.id.clone(), timeline_item.clone());
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(timeline_item.clone())?;
        if emit_live_update {
            self.emit_timeline_live_update(timeline_item)?;
        }
        Ok(())
    }

    fn persist_timeline_items(&self) -> Result<()> {
        let mut items = self.timeline_items.values().cloned().collect::<Vec<_>>();
        items.sort_by_key(|item| item.started_seq.unwrap_or(item.seq));
        write_timeline_items(&self.paths.timeline, &items)
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
        if revision % TIMELINE_COMPACT_EVERY_REVISIONS == 0 {
            self.persist_timeline_items()
        } else {
            append_timeline_patch(&self.paths.timeline, item.id.clone(), revision, item)
        }?;
        self.last_timeline_patch_at = Some(now);
        Ok(())
    }

    fn emit_timeline_live_update(&mut self, item: crate::acp::events::AcpUiEvent) -> Result<()> {
        if self.live_update.is_none() {
            return Ok(());
        }
        if is_streaming_timeline_update(&item) {
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

    fn emit_live_update_now(
        &mut self,
        item: &crate::acp::events::AcpUiEvent,
        now: Instant,
    ) -> Result<()> {
        if let Some(live_update) = self.live_update {
            live_update(item)?;
            self.last_live_update_at = Some(now);
        }
        Ok(())
    }

    /// Apply a streaming delta — get-or-create the stream, append content,
    /// and stamp the item with stream identity + sequence bounds.
    fn apply_streaming_delta(
        stream: &mut Option<AcpTimelineStreamState>,
        item: &mut crate::acp::events::AcpUiEvent,
        stable_id: &str,
        max_chars: usize,
        seq: u64,
        timestamp: &str,
    ) {
        let stream = stream.get_or_insert_with(|| AcpTimelineStreamState {
            item_id: stable_id.to_string(),
            started_seq: seq,
            started_at: timestamp.to_string(),
            content: String::new(),
        });
        if let Some(content) = item.content.as_deref() {
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
                Self::apply_streaming_delta(
                    &mut self.active_text_stream,
                    &mut item,
                    &stable_id,
                    256_000,
                    seq,
                    &timestamp,
                );
            }
            "thoughtDelta" => {
                let stable_id = stable_thought_item_id(&item);
                Self::apply_streaming_delta(
                    &mut self.active_thought_stream,
                    &mut item,
                    &stable_id,
                    256_000,
                    seq,
                    &timestamp,
                );
            }
            "plan" => {
                let stable_id = stable_plan_item_id(&item);
                Self::apply_streaming_delta(
                    &mut self.active_plan_stream,
                    &mut item,
                    &stable_id,
                    64_000,
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
        let _ = self.persist_timeline_items();
        if let Some(session_id) = self.session_id.as_deref() {
            self.connection.unregister_session_route(session_id);
        }
        if self.connection_key.is_none() {
            self.connection.shutdown();
        }
        unregister_provider_control(&self.paths.attempt_dir, &self.control);
    }
}

impl Drop for AcpRuntime<'_> {
    fn drop(&mut self) {
        let _ = self.flush_pending_live_update();
        let _ = self.flush_pending_timeline_patch();
        if let Some(session_id) = self.session_id.as_deref() {
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

fn initial_acp_source_seq(paths: &AcpAttemptPaths) -> u64 {
    if paths.timeline.exists() || !paths.events.exists() {
        latest_timeline_source_seq(&paths.timeline)
    } else {
        initial_acp_event_seq(&paths.events)
    }
}

fn stable_message_item_id(event: &crate::acp::events::AcpUiEvent) -> String {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("messageId"))
        .and_then(Value::as_str)
        .map(|message_id| format!("assistant-message-{message_id}"))
        .unwrap_or_else(|| format!("assistant-message-{}", event.id))
}

fn stable_thought_item_id(event: &crate::acp::events::AcpUiEvent) -> String {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("messageId"))
        .and_then(Value::as_str)
        .map(|message_id| format!("assistant-thought-{message_id}"))
        .unwrap_or_else(|| format!("assistant-thought-{}", event.id))
}

fn stable_plan_item_id(event: &crate::acp::events::AcpUiEvent) -> String {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("sessionId"))
        .and_then(Value::as_str)
        .map(|session_id| format!("session-plan-{session_id}"))
        .unwrap_or_else(|| format!("session-plan-{}", event.id))
}

fn contributes_to_final_text(kind: &str) -> bool {
    kind == "textDelta"
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        DOCTOR_DIAGNOSTIC_TARGET_SIZE, PromptBundle, PromptVisibility, RuntimeStopProbe,
        cleanup_doctor_acp_dir_after_success, contributes_to_final_text, resolve_permission_mode,
        retain_bounded_doctor_acp_failure_bundle, session_load_params, session_new_params,
        session_prompt_params, session_prompt_text,
    };

    #[test]
    fn final_text_ignores_user_prompt_deltas() {
        assert!(contributes_to_final_text("textDelta"));
        assert!(!contributes_to_final_text("userTextDelta"));
        assert!(!contributes_to_final_text("thoughtDelta"));
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
    fn codex_session_prompt_inlines_system_prompt() {
        let prompt = PromptBundle {
            system_prompt: "node constraints".to_string(),
            user_prompt: "do the task".to_string(),
            prompt_id: Some("prompt-001".to_string()),
            visibility: PromptVisibility::Visible,
            attachment_metas: Vec::new(),
            content_blocks: Vec::new(),
        };

        let text = session_prompt_text("codex-acp", &prompt);
        assert!(text.contains(
            "<hidden data-gold-band-hidden=\"true\" title=\"Gold Band stable system prompt\">"
        ));
        assert!(text.contains("node constraints"));
        assert!(text.ends_with("do the task"));

        let params = session_prompt_params("codex-acp", "session-123", &prompt);
        assert_eq!(params["sessionId"], "session-123");
        assert_eq!(params["prompt"][0]["text"], text);
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

        assert_eq!(session_prompt_text("claude-acp", &prompt), "do the task");
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
}
