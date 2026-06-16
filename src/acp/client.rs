use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
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

use crate::acp::adapter::{ResolvedAcpAdapter, spawn_adapter};
use crate::acp::events::{
    AcpAttemptPaths, AcpSessionMetadata, AcpUiEvent, append_diagnostic, append_raw_frame,
    append_timeline_patch, append_ui_event, current_timestamp, initial_acp_event_seq,
    latest_timeline_source_seq, load_timeline_items, normalize_session_update,
    permission_request_event, user_prompt_event, write_session_metadata, write_timeline_items,
};
use crate::acp::permission::{
    acp_permission_response_result, clear_cancel_request, is_cancel_requested,
    wait_for_permission_response, write_pending_permission,
};
use crate::config::AcpAdapterConfig;
use crate::domain::{SessionMode, VERSION};
use crate::process::kill_process_tree;
use crate::provider::{PromptBundle, PromptVisibility};
use crate::runtime::{WorkerRefState, validate_worker_ref_state};
use crate::storage::{GoldBandPaths, ensure_parent_dir, read_json, write_json};

const CANCEL_CHECK_INTERVAL: Duration = Duration::from_millis(200);
const CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(5);
const LIVE_STREAM_UPDATE_INTERVAL: Duration = Duration::from_millis(75);
const TIMELINE_COMPACT_EVERY_REVISIONS: u64 = 128;
const DOCTOR_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const SESSION_TITLE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct AcpCancelled;

impl std::fmt::Display for AcpCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ACP prompt cancelled")
    }
}

impl std::error::Error for AcpCancelled {}

fn session_file_is_cancelled(path: &Utf8Path) -> bool {
    read_json::<Value>(path)
        .ok()
        .and_then(|session| {
            let status = session
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let stop_reason = session
                .get("stopReason")
                .or_else(|| session.get("stop_reason"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            (status.eq_ignore_ascii_case("cancelled")
                || status.eq_ignore_ascii_case("canceled")
                || stop_reason.eq_ignore_ascii_case("cancelled")
                || stop_reason.eq_ignore_ascii_case("canceled"))
            .then_some(())
        })
        .is_some()
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
    child: Child,
    adapter: ResolvedAcpAdapter,
    stdin: std::process::ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
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
}

pub fn doctor(
    config: &AcpAdapterConfig,
    cwd: Utf8PathBuf,
    use_local_claude: bool,
) -> Result<Value> {
    let paths = GoldBandPaths::new(cwd.clone());
    let mut runtime = AcpRuntime::start(
        config,
        cwd.clone(),
        paths.doctor_acp_dir(),
        use_local_claude,
        5 * 1024 * 1024,
        4 * 1024 * 1024,
        None,
    )?;
    let result = (|| {
        let mut capabilities = runtime.initialize_with_timeout(Some(DOCTOR_REQUEST_TIMEOUT))?;
        runtime.setup_session(cwd, None, None, None, "", false)?;
        runtime.cleanup_diagnostic_session()?;
        runtime.merge_session_config_into_capabilities(&mut capabilities);
        Ok(capabilities)
    })();
    runtime.shutdown();
    result
}

pub fn run_prompt(
    provider_id: &str,
    config: &AcpAdapterConfig,
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
    session_update: Option<&dyn Fn() -> Result<()>>,
) -> Result<AcpPromptRun> {
    clear_cancel_request(&attempt_dir)?;
    let mut runtime = AcpRuntime::start(
        config,
        workspace_dir.clone(),
        attempt_dir,
        use_local_claude,
        acp_raw_max_size_bytes,
        acp_raw_target_size_bytes,
        live_update,
    )?;
    let capabilities = runtime.initialize()?;
    let strict_continue = session_mode == SessionMode::Continue && continue_ref.is_some();
    let restored = runtime.setup_session(
        workspace_dir.clone(),
        continue_ref,
        permission_mode.as_deref(),
        model.as_deref(),
        &prompt.system_prompt,
        strict_continue,
    )?;
    let session_id = runtime
        .session_id
        .clone()
        .ok_or_else(|| anyhow!("ACP session setup did not return a session id"))?;
    runtime.write_worker_ref(provider_id, &workspace_dir, session_mode, restored, None)?;
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
        Ok(stop_reason) => ("completed", stop_reason),
        Err(error) if error.downcast_ref::<AcpCancelled>().is_some() => {
            ("cancelled", Some("cancelled".to_string()))
        }
        Err(error) => {
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
    if status != "running" {
        let _ = clear_cancel_request(&runtime.paths.attempt_dir);
    }
    let run = AcpPromptRun {
        session_id,
        adapter_id: runtime.adapter.adapter_id.clone(),
        adapter_display_name: runtime.adapter.display_name.clone(),
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

fn session_new_params(cwd: &Utf8Path, system_prompt: &str) -> Value {
    let mut params = json!({
        "cwd": cwd.as_str(),
        "mcpServers": [],
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

fn session_load_params(cwd: &Utf8Path, session_id: &str, system_prompt: &str) -> Value {
    let mut params = json!({
        "cwd": cwd.as_str(),
        "mcpServers": [],
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
    if provider_id == "codex-acp" && !prompt.system_prompt.trim().is_empty() {
        return format!(
            "# Gold Band System Prompt\n{}\n\n# User Prompt\n{}",
            prompt.system_prompt, prompt.user_prompt
        );
    }

    prompt.user_prompt.clone()
}

impl<'a> AcpRuntime<'a> {
    fn start(
        config: &AcpAdapterConfig,
        cwd: Utf8PathBuf,
        attempt_dir: Utf8PathBuf,
        use_local_claude: bool,
        raw_max_size: u64,
        raw_target_size: u64,
        live_update: Option<&'a dyn Fn(&AcpUiEvent) -> Result<()>>,
    ) -> Result<Self> {
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        ensure_parent_dir(&paths.raw)?;
        ensure_parent_dir(&paths.diagnostics)?;
        let (adapter, mut child) = match spawn_adapter(config, cwd.as_std_path(), use_local_claude)
        {
            Ok(result) => result,
            Err(error) => {
                append_diagnostic(
                    &paths.diagnostics,
                    "error",
                    format!("failed to start ACP adapter: {error}"),
                    Some(json!({
                        "command": config.command,
                        "args": config.args,
                        "displayName": config.display_name,
                    })),
                )?;
                return Err(error);
            }
        };
        ensure_parent_dir(&paths.provider_pid)?;
        std::fs::write(paths.provider_pid.as_std_path(), child.id().to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to capture ACP adapter stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture ACP adapter stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture ACP adapter stderr"))?;
        let (tx, rx) = mpsc::sync_channel(1024);
        let raw_path = paths.raw.clone();
        let diagnostics_path = paths.diagnostics.clone();
        let raw_max = raw_max_size;
        let raw_target = raw_target_size;
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) if line.trim().is_empty() => {}
                    Ok(line) => match serde_json::from_str::<Value>(&line) {
                        Ok(value) => {
                            let _ = append_raw_frame(
                                &raw_path,
                                "inbound",
                                value.clone(),
                                raw_max,
                                raw_target,
                            );
                            if tx.send(value).is_err() {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = append_diagnostic(
                                &diagnostics_path,
                                "error",
                                format!("invalid ACP stdout frame: {err}"),
                                Some(json!({ "line": line })),
                            );
                        }
                    },
                    Err(err) => {
                        let _ = append_diagnostic(
                            &diagnostics_path,
                            "error",
                            format!("failed reading ACP stdout: {err}"),
                            None,
                        );
                        break;
                    }
                }
            }
        });
        let stderr_diagnostics_path = paths.diagnostics.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) if line.trim().is_empty() => {}
                    Ok(line) => {
                        let _ = append_diagnostic(
                            &stderr_diagnostics_path,
                            "info",
                            truncate_text(line, 8_000),
                            None,
                        );
                    }
                    Err(err) => {
                        let _ = append_diagnostic(
                            &stderr_diagnostics_path,
                            "error",
                            format!("failed reading ACP stderr: {err}"),
                            None,
                        );
                        break;
                    }
                }
            }
        });
        let seq = initial_acp_source_seq(&paths);
        let timeline_items = load_timeline_items(&paths.timeline)?
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect::<HashMap<_, _>>();
        let timeline_revision = seq;
        Ok(Self {
            paths,
            child,
            adapter,
            stdin,
            rx,
            next_id: 1,
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
        })
    }

    fn initialize(&mut self) -> Result<Value> {
        self.initialize_with_timeout(None)
    }

    fn initialize_with_timeout(&mut self, timeout: Option<Duration>) -> Result<Value> {
        let result = self.request_with_timeout(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "gold-band",
                    "title": "Gold Band",
                    "version": crate::domain::VERSION,
                }
            }),
            timeout,
        )?;
        Ok(result
            .get("agentCapabilities")
            .cloned()
            .unwrap_or_else(|| json!({})))
    }

    fn setup_session(
        &mut self,
        cwd: Utf8PathBuf,
        continue_ref: Option<Value>,
        permission_mode: Option<&str>,
        model: Option<&str>,
        system_prompt: &str,
        strict_continue: bool,
    ) -> Result<bool> {
        if let Some(session_id) = continue_ref
            .as_ref()
            .and_then(|value| value.get("acpSessionId"))
            .and_then(Value::as_str)
        {
            self.suppress_session_updates = true;
            let load = self.request(
                "session/load",
                session_load_params(&cwd, session_id, system_prompt),
            );
            self.suppress_session_updates = false;
            match load {
                Ok(result) => {
                    self.capture_session_config(&result);
                    self.session_id = Some(session_id.to_string());
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
                    if strict_continue {
                        bail!("failed to load existing ACP session for continue: {err}");
                    }
                }
            }
        }

        if strict_continue {
            bail!("ACP continue requires an existing session id");
        }

        let result = self.request("session/new", session_new_params(&cwd, system_prompt))?;
        self.capture_session_config(&result);
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("ACP session/new response missing sessionId"))?;
        self.session_id = Some(session_id.to_string());
        self.apply_session_mode_options(permission_mode, model)?;
        Ok(false)
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
            .request(
                "session/delete",
                json!({
                    "sessionId": session_id,
                }),
            )
            .is_ok()
        {
            self.session_id = None;
            return Ok(());
        }
        if self
            .request(
                "session/close",
                json!({
                    "sessionId": session_id,
                }),
            )
            .is_ok()
        {
            self.session_id = None;
        }
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
        self.seq += 1;
        let user_event = user_prompt_event(
            self.seq,
            session_id.clone(),
            prompt.user_prompt.clone(),
            prompt.prompt_id.clone(),
            prompt.visibility == PromptVisibility::Hidden,
            prompt.attachment_metas.clone(),
        );
        self.persist_event(&user_event)?;
        let result = self.request_with_progress(
            "session/prompt",
            session_prompt_params(provider_id, &session_id, prompt),
            None,
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
        let id = self.next_id;
        self.next_id += 1;
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_frame(&frame)?;
        let started_at = Instant::now();
        let mut cancel_started_at = None;
        let mut cancel_notified = false;
        let mut last_title_refresh_at = Instant::now();
        loop {
            let wait_for = match timeout {
                Some(timeout) => match timeout.checked_sub(started_at.elapsed()) {
                    Some(remaining) => remaining.min(CANCEL_CHECK_INTERVAL),
                    None => {
                        self.kill_adapter_process();
                        bail!(
                            "ACP `{method}` timed out after {} seconds",
                            timeout.as_secs()
                        );
                    }
                },
                None => CANCEL_CHECK_INTERVAL,
            };
            match self.rx.recv_timeout(wait_for) {
                Ok(value) => {
                    if value.get("method").is_some() {
                        self.handle_inbound(value)?;
                        continue;
                    }

                    if response_matches_request(&value, id) {
                        if let Some(error) = value.get("error") {
                            if self.is_cancellation_observed() {
                                return Err(anyhow!(AcpCancelled));
                            }
                            bail!("ACP `{method}` failed: {error}");
                        }
                        return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
                    }
                    self.handle_inbound(value)?;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some((workspace_dir, status, restored, ref stop_reason, capabilities)) =
                        title_refresh
                    {
                        if last_title_refresh_at.elapsed() >= SESSION_TITLE_REFRESH_INTERVAL {
                            self.refresh_session_title_and_persist(
                                workspace_dir,
                                status,
                                restored,
                                stop_reason.clone(),
                                capabilities,
                            );
                            last_title_refresh_at = Instant::now();
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if self.is_cancellation_observed() {
                        return Err(anyhow!(AcpCancelled));
                    }
                    bail!("ACP adapter closed before `{method}` response");
                }
            }

            if is_cancel_requested(&self.paths.attempt_dir) {
                if cancel_started_at.is_none() {
                    cancel_started_at = Some(Instant::now());
                }
                if !cancel_notified {
                    append_diagnostic(
                        &self.paths.diagnostics,
                        "info",
                        "ACP cancellation requested".to_string(),
                        Some(json!({ "method": method })),
                    )?;
                    self.send_cancel_notification()?;
                    cancel_notified = true;
                }
                if cancel_started_at
                    .is_some_and(|started_at| started_at.elapsed() >= CANCEL_GRACE_PERIOD)
                {
                    self.kill_adapter_process();
                    return Err(anyhow!(AcpCancelled));
                }
            }

            if let Some(status) = self.child.try_wait()? {
                if self.is_cancellation_observed() {
                    return Err(anyhow!(AcpCancelled));
                }
                bail!("ACP adapter exited before `{method}` response with status {status}");
            }
        }
    }

    fn handle_inbound(&mut self, value: Value) -> Result<()> {
        match value.get("method").and_then(Value::as_str) {
            Some("session/update") => self.handle_session_update(value),
            Some("session/request_permission") => self.handle_permission_request(value),
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
        self.send_frame(&json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "result": result,
        }))
    }

    fn send_cancel_notification(&mut self) -> Result<()> {
        let Some(session_id) = self.session_id.clone() else {
            return Ok(());
        };
        self.send_frame(&cancel_notification_frame(&session_id))
    }

    fn kill_adapter_process(&mut self) {
        let pid = self.child.id();
        let _ = kill_process_tree(pid);
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(self.paths.provider_pid.as_std_path());
    }

    fn is_cancellation_observed(&self) -> bool {
        is_cancel_requested(&self.paths.attempt_dir)
            || session_file_is_cancelled(&self.paths.snapshot)
            || session_file_is_cancelled(&self.paths.session)
    }

    fn send_frame(&mut self, frame: &Value) -> Result<()> {
        append_raw_frame(
            &self.paths.raw,
            "outbound",
            frame.clone(),
            self.raw_max_size,
            self.raw_target_size,
        )?;
        let line = serde_json::to_string(frame)?;
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
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
                "adapterId": self.adapter.adapter_id.clone(),
                "adapterDisplayName": self.adapter.display_name.clone(),
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
            adapter_id: self.adapter.adapter_id.clone(),
            adapter_display_name: self.adapter.display_name.clone(),
            cwd: self.paths.attempt_dir.to_string(),
            title: self.session_title.clone(),
            status: status.to_string(),
            restored,
            stop_reason,
            capabilities,
            models: self.models.clone(),
            modes: self.modes.clone(),
            config_options: self.config_options.clone(),
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
        if should_write_legacy_events(&self.paths) {
            append_ui_event(&self.paths.events, event)?;
        }
        let timeline_item = self.timeline_item_for_event(event);
        self.timeline_items
            .insert(timeline_item.id.clone(), timeline_item.clone());
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        self.persist_timeline_update(timeline_item.clone())?;
        self.emit_timeline_live_update(timeline_item)?;
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
        debug!(adapter = %self.adapter.adapter_id, "shutting down ACP adapter");
        let _ = self.flush_pending_live_update();
        let _ = self.flush_pending_timeline_patch();
        let _ = self.persist_timeline_items();
        let _ = self.stdin.flush();
        self.kill_adapter_process();
    }
}

impl Drop for AcpRuntime<'_> {
    fn drop(&mut self) {
        let _ = self.flush_pending_live_update();
        let _ = self.flush_pending_timeline_patch();
        if self.child.try_wait().ok().flatten().is_none() {
            self.kill_adapter_process();
        }
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

fn truncate_text(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push('…');
    truncated
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

fn cancel_notification_frame(session_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {
            "sessionId": session_id,
        }
    })
}

fn response_matches_request(value: &Value, request_id: u64) -> bool {
    value.get("method").is_none()
        && value.get("id").and_then(Value::as_u64) == Some(request_id)
        && (value.get("result").is_some() || value.get("error").is_some())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        PromptBundle, PromptVisibility, cancel_notification_frame, contributes_to_final_text,
        resolve_permission_mode, response_matches_request, session_load_params, session_new_params,
        session_prompt_params, session_prompt_text,
    };

    #[test]
    fn final_text_ignores_user_prompt_deltas() {
        assert!(contributes_to_final_text("textDelta"));
        assert!(!contributes_to_final_text("userTextDelta"));
        assert!(!contributes_to_final_text("thoughtDelta"));
    }

    #[test]
    fn cancel_frame_is_notification_without_id() {
        let frame = cancel_notification_frame("session-123");
        assert_eq!(frame.get("id"), None);
        assert_eq!(
            frame,
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": {
                    "sessionId": "session-123",
                }
            })
        );
    }

    #[test]
    fn inbound_request_with_matching_id_is_not_response() {
        let permission_request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/request_permission",
            "params": {}
        });
        let prompt_response = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": { "stopReason": "end_turn" }
        });

        assert!(!response_matches_request(&permission_request, 3));
        assert!(response_matches_request(&prompt_response, 3));
    }

    #[test]
    fn session_setup_params_append_system_prompt() {
        let new_params = session_new_params(camino::Utf8Path::new("/repo"), "node constraints");
        assert_eq!(
            new_params["_meta"]["systemPrompt"]["append"],
            "node constraints"
        );

        let load_params = session_load_params(
            camino::Utf8Path::new("/repo"),
            "session-123",
            "node constraints",
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
        assert!(text.contains("# Gold Band System Prompt\nnode constraints"));
        assert!(text.contains("# User Prompt\ndo the task"));

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
}
