use std::io::{BufRead, BufReader, Write};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::{Value, json};
use tracing::debug;

use crate::acp::adapter::{ResolvedAcpAdapter, spawn_adapter};
use crate::acp::events::{
    AcpAttemptPaths, AcpSessionMetadata, append_diagnostic, append_raw_frame, append_ui_event,
    current_timestamp, initial_acp_event_seq, normalize_session_update, permission_request_event,
    user_prompt_event, write_session_metadata,
};
use crate::acp::permission::{
    acp_permission_response_result, clear_cancel_request, is_cancel_requested,
    wait_for_permission_response, write_pending_permission,
};
use crate::config::AcpAdapterConfig;
use crate::domain::{DEFAULT_PROVIDER, SessionMode, VERSION};
use crate::process::kill_process_tree;
use crate::provider::PromptBundle;
use crate::runtime::{WorkerRefState, validate_worker_ref_state};
use crate::storage::{GoldBandPaths, ensure_parent_dir, read_json, write_json};

const CANCEL_CHECK_INTERVAL: Duration = Duration::from_millis(200);
const CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(5);
const DOCTOR_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug)]
struct AcpCancelled;

impl std::fmt::Display for AcpCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ACP prompt cancelled")
    }
}

impl std::error::Error for AcpCancelled {}

#[derive(Debug, Clone)]
pub struct AcpPromptRun {
    pub session_id: String,
    pub adapter_id: String,
    pub adapter_display_name: String,
    pub stop_reason: Option<String>,
    pub final_text: String,
    pub final_outputs: Vec<String>,
    pub restored: bool,
}

struct AcpRuntime {
    paths: AcpAttemptPaths,
    child: Child,
    adapter: ResolvedAcpAdapter,
    stdin: std::process::ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
    seq: u64,
    session_id: Option<String>,
    final_text: String,
    final_outputs: Vec<String>,
    collecting_text_output: bool,
    suppress_session_updates: bool,
    models: Option<Value>,
    modes: Option<Value>,
    config_options: Option<Value>,
}

pub fn doctor(config: &AcpAdapterConfig, cwd: Utf8PathBuf) -> Result<Value> {
    let paths = GoldBandPaths::new(cwd.clone());
    let mut runtime =
        AcpRuntime::start(config, cwd.clone(), paths.runtime_root.join("doctor/acp"))?;
    let result = (|| {
        let mut capabilities = runtime.initialize_with_timeout(Some(DOCTOR_REQUEST_TIMEOUT))?;
        runtime.setup_session(cwd, None, None, "", false)?;
        runtime.cleanup_diagnostic_session()?;
        runtime.merge_session_config_into_capabilities(&mut capabilities);
        Ok(capabilities)
    })();
    runtime.shutdown();
    result
}

pub fn run_prompt(
    config: &AcpAdapterConfig,
    workspace_dir: Utf8PathBuf,
    attempt_dir: Utf8PathBuf,
    prompt: &PromptBundle,
    session_mode: SessionMode,
    permission_mode: Option<String>,
    continue_ref: Option<Value>,
) -> Result<AcpPromptRun> {
    clear_cancel_request(&attempt_dir)?;
    let mut runtime = AcpRuntime::start(config, workspace_dir.clone(), attempt_dir)?;
    let capabilities = runtime.initialize()?;
    let strict_continue = session_mode == SessionMode::Continue && continue_ref.is_some();
    let restored = runtime.setup_session(
        workspace_dir.clone(),
        continue_ref,
        permission_mode.as_deref(),
        &prompt.system_prompt,
        strict_continue,
    )?;
    let session_id = runtime
        .session_id
        .clone()
        .ok_or_else(|| anyhow!("ACP session setup did not return a session id"))?;
    runtime.write_worker_ref(&workspace_dir, session_mode, restored, None)?;
    runtime.write_session("running", restored, None, capabilities.clone())?;
    let prompt_result = runtime.prompt(prompt);
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
                &workspace_dir,
                session_mode,
                restored,
                Some("error".to_string()),
            )?;
            runtime.write_session("failed", restored, Some("error".to_string()), capabilities)?;
            runtime.shutdown();
            return Err(error);
        }
    };
    runtime.write_worker_ref(&workspace_dir, session_mode, restored, stop_reason.clone())?;
    runtime.write_session(status, restored, stop_reason.clone(), capabilities)?;
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
    };
    runtime.shutdown();
    Ok(run)
}

impl AcpRuntime {
    fn start(
        config: &AcpAdapterConfig,
        cwd: Utf8PathBuf,
        attempt_dir: Utf8PathBuf,
    ) -> Result<Self> {
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        ensure_parent_dir(&paths.raw)?;
        ensure_parent_dir(&paths.diagnostics)?;
        let (adapter, mut child) = match spawn_adapter(config, cwd.as_std_path()) {
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
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) if line.trim().is_empty() => {}
                    Ok(line) => match serde_json::from_str::<Value>(&line) {
                        Ok(value) => {
                            let _ = append_raw_frame(&raw_path, "inbound", value.clone());
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
        let seq = initial_acp_event_seq(&paths.events);
        Ok(Self {
            paths,
            child,
            adapter,
            stdin,
            rx,
            next_id: 1,
            seq,
            session_id: None,
            final_text: String::new(),
            final_outputs: Vec::new(),
            collecting_text_output: false,
            suppress_session_updates: false,
            models: None,
            modes: None,
            config_options: None,
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
                json!({
                    "cwd": cwd.as_str(),
                    "mcpServers": [],
                    "sessionId": session_id,
                }),
            );
            self.suppress_session_updates = false;
            match load {
                Ok(result) => {
                    self.capture_session_config(&result);
                    self.session_id = Some(session_id.to_string());
                    if let Some(permission_mode) =
                        permission_mode.filter(|value| !value.trim().is_empty())
                    {
                        self.apply_permission_mode(permission_mode)?;
                    }
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
        let result = self.request("session/new", params)?;
        self.capture_session_config(&result);
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("ACP session/new response missing sessionId"))?;
        self.session_id = Some(session_id.to_string());
        if let Some(permission_mode) = permission_mode.filter(|value| !value.trim().is_empty()) {
            self.apply_permission_mode(permission_mode)?;
        }
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

    fn apply_permission_mode(&mut self, permission_mode: &str) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP permission mode requires a session id"))?;
        let permission_mode = permission_mode.trim();
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
            self.set_current_mode(permission_mode);
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
            self.set_current_mode(permission_mode);
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

    fn prompt(&mut self, prompt: &PromptBundle) -> Result<Option<String>> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP prompt requires a session id"))?;
        self.seq += 1;
        append_ui_event(
            &self.paths.events,
            &user_prompt_event(
                self.seq,
                session_id.clone(),
                prompt.user_prompt.clone(),
                prompt.prompt_id.clone(),
            ),
        )?;
        let result = self.request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": prompt.user_prompt.clone(),
                }]
            }),
        )?;
        Ok(result
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_with_timeout(method, params, None)
    }

    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
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
                            if cancel_started_at.is_some() {
                                return Err(anyhow!(AcpCancelled));
                            }
                            bail!("ACP `{method}` failed: {error}");
                        }
                        return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
                    }
                    self.handle_inbound(value)?;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if cancel_started_at.is_some() || is_cancel_requested(&self.paths.attempt_dir) {
                        return Err(anyhow!(AcpCancelled));
                    }
                    bail!("ACP adapter closed before `{method}` response");
                }
            }

            if is_cancel_requested(&self.paths.attempt_dir) {
                if cancel_started_at.is_none() {
                    cancel_started_at = Some(Instant::now());
                    self.send_cancel_notification()?;
                    append_diagnostic(
                        &self.paths.diagnostics,
                        "info",
                        "ACP cancellation requested".to_string(),
                        Some(json!({ "method": method })),
                    )?;
                }
                if cancel_started_at
                    .is_some_and(|started_at| started_at.elapsed() >= CANCEL_GRACE_PERIOD)
                {
                    self.kill_adapter_process();
                    return Err(anyhow!(AcpCancelled));
                }
            }

            if let Some(status) = self.child.try_wait()? {
                if cancel_started_at.is_some() || is_cancel_requested(&self.paths.attempt_dir) {
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
        self.seq += 1;
        for event in normalize_session_update(self.seq, session_id, &update) {
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
            append_ui_event(&self.paths.events, &event)?;
        }
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
        append_ui_event(&self.paths.events, &event)?;
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

    fn send_frame(&mut self, frame: &Value) -> Result<()> {
        append_raw_frame(&self.paths.raw, "outbound", frame.clone())?;
        let line = serde_json::to_string(frame)?;
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn write_worker_ref(
        &self,
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
            provider: DEFAULT_PROVIDER.to_string(),
            mode: session_mode,
            supports_open_session: true,
            supports_continue_session: true,
            continue_ref: Some(json!({
                "acpSessionId": session_id,
                "adapterId": self.adapter.adapter_id.clone(),
                "adapterDisplayName": self.adapter.display_name.clone(),
                "cwd": workspace_dir.as_str(),
                "sessionFile": self.paths.session.as_str(),
                "lastStopReason": stop_reason,
                "restored": restored,
            })),
            open_command: None,
        };
        validate_worker_ref_state(&worker_ref)?;
        write_json(&self.paths.attempt_dir.join("worker-ref.json"), &worker_ref)
    }

    fn write_session(
        &self,
        status: &str,
        restored: bool,
        stop_reason: Option<String>,
        capabilities: Value,
    ) -> Result<()> {
        let now = current_timestamp();
        let created_at = if self.paths.session.exists() {
            read_json::<AcpSessionMetadata>(&self.paths.session)
                .map(|session| session.created_at)
                .unwrap_or_else(|_| now.clone())
        } else {
            now.clone()
        };
        write_session_metadata(
            &self.paths.session,
            &AcpSessionMetadata {
                adapter_id: self.adapter.adapter_id.clone(),
                adapter_display_name: self.adapter.display_name.clone(),
                cwd: self.paths.attempt_dir.to_string(),
                status: status.to_string(),
                restored,
                stop_reason,
                capabilities,
                models: self.models.clone(),
                modes: self.modes.clone(),
                config_options: self.config_options.clone(),
                created_at,
                updated_at: now,
            },
        )
    }

    fn shutdown(mut self) {
        debug!(adapter = %self.adapter.adapter_id, "shutting down ACP adapter");
        let _ = self.stdin.flush();
        self.kill_adapter_process();
    }
}

impl Drop for AcpRuntime {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.kill_adapter_process();
        }
    }
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

fn has_mode_config_option(config_options: Option<&Value>) -> bool {
    config_options
        .and_then(Value::as_array)
        .is_some_and(|options| {
            options.iter().any(|option| {
                option.get("id").and_then(Value::as_str) == Some("mode")
                    || option.get("category").and_then(Value::as_str) == Some("mode")
            })
        })
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

    use super::{cancel_notification_frame, contributes_to_final_text, response_matches_request};

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
}
