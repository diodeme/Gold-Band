use std::io::{BufRead, BufReader, Write};
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::{Context, Result, anyhow, bail};
use camino::Utf8PathBuf;
use serde_json::{Value, json};
use tracing::debug;

use crate::acp::adapter::{ResolvedAcpAdapter, spawn_adapter};
use crate::acp::events::{
    AcpAttemptPaths, AcpSessionMetadata, append_diagnostic, append_raw_frame, append_ui_event,
    current_timestamp, initial_acp_event_seq, normalize_session_update, permission_request_event,
    user_prompt_event, write_session_metadata,
};
use crate::acp::permission::{
    acp_permission_response_result, wait_for_permission_response, write_pending_permission,
};
use crate::config::AcpAdapterConfig;
use crate::provider::PromptBundle;
use crate::storage::ensure_parent_dir;

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

pub fn doctor(config: &AcpAdapterConfig, cwd: Utf8PathBuf) -> Result<()> {
    let mut runtime = AcpRuntime::start(config, cwd.clone(), cwd.join(".gold-band/doctor/acp"))?;
    runtime.initialize()?;
    runtime.shutdown();
    Ok(())
}

pub fn run_prompt(
    config: &AcpAdapterConfig,
    workspace_dir: Utf8PathBuf,
    attempt_dir: Utf8PathBuf,
    prompt: &PromptBundle,
    continue_ref: Option<Value>,
) -> Result<AcpPromptRun> {
    let mut runtime = AcpRuntime::start(config, workspace_dir.clone(), attempt_dir)?;
    let capabilities = runtime.initialize()?;
    let restored =
        runtime.setup_session(workspace_dir.clone(), continue_ref, &prompt.system_prompt)?;
    let session_id = runtime
        .session_id
        .clone()
        .ok_or_else(|| anyhow!("ACP session setup did not return a session id"))?;
    runtime.write_session("running", restored, None, capabilities.clone())?;
    let stop_reason = runtime.prompt(prompt)?;
    runtime.write_session("completed", restored, stop_reason.clone(), capabilities)?;
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
        let pid_path = paths.attempt_dir.join("provider.pid");
        ensure_parent_dir(&pid_path)?;
        std::fs::write(pid_path.as_std_path(), child.id().to_string())?;
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
        let result = self.request(
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
        system_prompt: &str,
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
                    return Ok(true);
                }
                Err(err) => {
                    append_diagnostic(
                        &self.paths.diagnostics,
                        "warn",
                        format!("failed to load ACP session `{session_id}`: {err}"),
                        None,
                    )?;
                }
            }
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
        Ok(false)
    }

    fn capture_session_config(&mut self, result: &Value) {
        self.models = result.get("models").cloned();
        self.modes = result.get("modes").cloned();
        self.config_options = result.get("configOptions").cloned();
    }

    fn prompt(&mut self, prompt: &PromptBundle) -> Result<Option<String>> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| anyhow!("ACP prompt requires a session id"))?;
        self.seq += 1;
        append_ui_event(
            &self.paths.events,
            &user_prompt_event(self.seq, session_id.clone(), prompt.user_prompt.clone()),
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
        let id = self.next_id;
        self.next_id += 1;
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_frame(&frame)?;
        loop {
            let value = self
                .rx
                .recv()
                .with_context(|| format!("ACP adapter closed before `{method}` response"))?;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    bail!("ACP `{method}` failed: {error}");
                }
                return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
            }
            self.handle_inbound(value)?;
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

    fn send_frame(&mut self, frame: &Value) -> Result<()> {
        append_raw_frame(&self.paths.raw, "outbound", frame.clone())?;
        let line = serde_json::to_string(frame)?;
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn write_session(
        &self,
        status: &str,
        restored: bool,
        stop_reason: Option<String>,
        capabilities: Value,
    ) -> Result<()> {
        write_session_metadata(
            &self.paths.session,
            &AcpSessionMetadata {
                session_id: self.session_id.clone(),
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
                created_at: current_timestamp(),
                updated_at: current_timestamp(),
            },
        )
    }

    fn shutdown(mut self) {
        debug!(adapter = %self.adapter.adapter_id, "shutting down ACP adapter");
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = std::fs::remove_file(self.paths.attempt_dir.join("provider.pid").as_std_path());
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

#[cfg(test)]
mod tests {
    use super::contributes_to_final_text;

    #[test]
    fn final_text_ignores_user_prompt_deltas() {
        assert!(contributes_to_final_text("textDelta"));
        assert!(!contributes_to_final_text("userTextDelta"));
        assert!(!contributes_to_final_text("thoughtDelta"));
    }
}
