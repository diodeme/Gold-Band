use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Error, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::acp::adapter::{ResolvedAcpAdapter, spawn_adapter};
use crate::acp::events::append_raw_frame;
use crate::config::AcpAdapterConfig;
use crate::process::kill_process_tree;

const CLOSE_RAW_MAX_SIZE: u64 = 5 * 1024 * 1024;
const CLOSE_RAW_TARGET_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AdapterConnectionKey {
    pub provider_id: String,
    pub workspace_root: Utf8PathBuf,
}

impl AdapterConnectionKey {
    pub fn new(provider_id: impl Into<String>, workspace_root: Utf8PathBuf) -> Self {
        Self {
            provider_id: provider_id.into(),
            workspace_root,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdapterConfigSignature {
    command: String,
    args: Vec<String>,
    display_name: String,
    env: BTreeMap<String, String>,
    use_local_claude: bool,
}

impl AdapterConfigSignature {
    fn new(config: &AcpAdapterConfig, use_local_claude: bool) -> Self {
        Self {
            command: config.command.clone(),
            args: config.args.clone(),
            display_name: config.display_name.clone(),
            env: config.env.clone(),
            use_local_claude,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveAcpSession {
    pub key: AdapterConnectionKey,
    pub session_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterConnectionOutcome {
    Reused,
    Spawned,
    ReplacedStale,
}

impl AdapterConnectionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reused => "reused",
            Self::Spawned => "spawned",
            Self::ReplacedStale => "replaced-stale",
        }
    }
}

pub struct AdapterConnectionResolution {
    pub connection: Arc<AdapterConnection>,
    pub outcome: AdapterConnectionOutcome,
}

#[derive(Debug)]
pub struct PendingRequest {
    pub id: u64,
    pub frame: Value,
    rx: mpsc::Receiver<Value>,
}

impl PendingRequest {
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Value, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

pub struct AdapterConnection {
    key: Option<AdapterConnectionKey>,
    adapter: ResolvedAcpAdapter,
    signature: AdapterConfigSignature,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    next_id: Mutex<u64>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    session_routes: Mutex<HashMap<String, mpsc::Sender<Value>>>,
    initialized_capabilities: Mutex<Option<Value>>,
    active_prompts: Mutex<usize>,
    transport_closed: Mutex<bool>,
}

impl AdapterConnection {
    pub fn spawn_standalone(
        config: &AcpAdapterConfig,
        cwd: &Utf8Path,
        use_local_claude: bool,
    ) -> Result<Arc<Self>> {
        Self::spawn(None, config, cwd, use_local_claude)
    }

    fn spawn(
        key: Option<AdapterConnectionKey>,
        config: &AcpAdapterConfig,
        cwd: &Utf8Path,
        use_local_claude: bool,
    ) -> Result<Arc<Self>> {
        let (adapter, mut child) = spawn_adapter(config, cwd.as_std_path(), use_local_claude)?;
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
        let connection = Arc::new(Self {
            key,
            adapter,
            signature: AdapterConfigSignature::new(config, use_local_claude),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            next_id: Mutex::new(1),
            pending: Mutex::new(HashMap::new()),
            session_routes: Mutex::new(HashMap::new()),
            initialized_capabilities: Mutex::new(None),
            active_prompts: Mutex::new(0),
            transport_closed: Mutex::new(false),
        });

        let stdout_connection = Arc::clone(&connection);
        thread::spawn(move || read_stdout(stdout_connection, stdout));

        let stderr_adapter_id = connection.adapter.adapter_id.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) if line.trim().is_empty() => {}
                    Ok(line) => {
                        debug!(adapter = %stderr_adapter_id, stderr = %line, "ACP adapter stderr")
                    }
                    Err(error) => {
                        warn!(adapter = %stderr_adapter_id, %error, "failed reading ACP adapter stderr");
                        break;
                    }
                }
            }
        });

        Ok(connection)
    }

    pub fn adapter(&self) -> &ResolvedAcpAdapter {
        &self.adapter
    }

    pub fn pid(&self) -> u32 {
        self.child
            .lock()
            .map(|child| child.id())
            .unwrap_or_default()
    }

    pub fn is_exited(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|mut child| child.try_wait().ok().flatten())
            .is_some()
    }

    pub fn try_wait(&self) -> Result<Option<std::process::ExitStatus>> {
        self.child
            .lock()
            .map_err(|_| anyhow!("ACP adapter child lock poisoned"))?
            .try_wait()
            .map_err(Into::into)
    }

    pub fn initialized_capabilities(&self) -> Option<Value> {
        self.initialized_capabilities
            .lock()
            .ok()
            .and_then(|capabilities| capabilities.clone())
    }

    pub fn set_initialized_capabilities(&self, capabilities: Value) {
        if let Ok(mut cached) = self.initialized_capabilities.lock() {
            *cached = Some(capabilities);
        }
    }

    pub fn begin_request(&self, method: &str, params: Value) -> Result<PendingRequest> {
        if self.is_transport_closed() {
            bail!("ACP adapter transport is closed");
        }
        let id = {
            let mut next_id = self
                .next_id
                .lock()
                .map_err(|_| anyhow!("ACP adapter request id lock poisoned"))?;
            let id = *next_id;
            *next_id = next_id.saturating_add(1);
            id
        };
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| anyhow!("ACP pending request lock poisoned"))?
            .insert(id, tx);
        if let Err(error) = self.send_raw_frame(&frame) {
            self.cancel_pending(id);
            return Err(error);
        }
        Ok(PendingRequest { id, frame, rx })
    }

    pub fn cancel_pending(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    pub fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        self.send_raw_frame(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    pub fn send_response(&self, id: Value, result: Value) -> Result<()> {
        self.send_raw_frame(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    pub fn send_raw_frame(&self, frame: &Value) -> Result<()> {
        if self.is_transport_closed() {
            bail!("ACP adapter transport is closed");
        }
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("ACP adapter stdin lock poisoned"))?;
        let line = serde_json::to_string(frame)?;
        let write_result = stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush());
        if let Err(error) = write_result {
            drop(stdin);
            self.mark_transport_closed();
            return Err(error.into());
        }
        Ok(())
    }

    pub fn register_session_route(&self, session_id: &str) -> mpsc::Receiver<Value> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut routes) = self.session_routes.lock() {
            routes.insert(session_id.to_string(), tx);
        }
        rx
    }

    pub fn unregister_session_route(&self, session_id: &str) {
        if let Ok(mut routes) = self.session_routes.lock() {
            routes.remove(session_id);
        }
    }

    pub fn mark_prompt_active(&self) {
        if let Ok(mut count) = self.active_prompts.lock() {
            *count = count.saturating_add(1);
        }
    }

    pub fn mark_prompt_inactive(&self) {
        if let Ok(mut count) = self.active_prompts.lock() {
            *count = count.saturating_sub(1);
        }
    }

    pub fn active_prompt_count(&self) -> usize {
        self.active_prompts.lock().map(|count| *count).unwrap_or(0)
    }

    pub fn is_transport_closed(&self) -> bool {
        self.transport_closed
            .lock()
            .map(|closed| *closed)
            .unwrap_or(true)
    }

    fn mark_transport_closed(&self) {
        if let Ok(mut closed) = self.transport_closed.lock() {
            *closed = true;
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        if let Ok(mut routes) = self.session_routes.lock() {
            routes.clear();
        }
    }

    pub fn close_session_bounded(&self, session_id: &str, timeout: Duration) -> Result<()> {
        self.close_session_bounded_with_raw_log(session_id, timeout, None)
    }

    pub fn close_session_bounded_with_raw_log(
        &self,
        session_id: &str,
        timeout: Duration,
        raw_path: Option<&Utf8Path>,
    ) -> Result<()> {
        let request = self.begin_request(
            "session/close",
            json!({
                "sessionId": session_id,
            }),
        )?;
        if let Some(raw_path) = raw_path {
            let _ = append_raw_frame(
                raw_path,
                "outbound",
                request.frame.clone(),
                CLOSE_RAW_MAX_SIZE,
                CLOSE_RAW_TARGET_SIZE,
            );
        }
        match request.recv_timeout(timeout) {
            Ok(value) => {
                if let Some(raw_path) = raw_path {
                    let _ = append_raw_frame(
                        raw_path,
                        "inbound",
                        value.clone(),
                        CLOSE_RAW_MAX_SIZE,
                        CLOSE_RAW_TARGET_SIZE,
                    );
                }
                if let Some(error) = value.get("error") {
                    bail!("ACP `session/close` failed: {error}");
                }
                self.unregister_session_route(session_id);
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.cancel_pending(request.id);
                bail!(
                    "ACP `session/close` timed out after {} seconds",
                    timeout.as_secs()
                )
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.cancel_pending(request.id);
                bail!("ACP adapter closed before `session/close` response")
            }
        }
    }

    pub fn delete_session_bounded(&self, session_id: &str, timeout: Duration) -> Result<()> {
        self.request_bounded(
            "session/delete",
            json!({
                "sessionId": session_id,
            }),
            timeout,
        )?;
        self.unregister_session_route(session_id);
        Ok(())
    }

    pub fn send_cancel_notification(&self, session_id: &str) -> Result<Value> {
        let frame = json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {
                "sessionId": session_id,
            },
        });
        self.send_notification(
            "session/cancel",
            frame.get("params").cloned().unwrap_or_else(|| json!({})),
        )?;
        Ok(frame)
    }

    fn request_bounded(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let request = self.begin_request(method, params)?;
        match request.recv_timeout(timeout) {
            Ok(value) => {
                if let Some(error) = value.get("error") {
                    bail!("ACP `{method}` failed: {error}");
                }
                Ok(value.get("result").cloned().unwrap_or_else(|| json!({})))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.cancel_pending(request.id);
                bail!(
                    "ACP `{method}` timed out after {} seconds",
                    timeout.as_secs()
                )
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.cancel_pending(request.id);
                bail!("ACP adapter closed before `{method}` response")
            }
        }
    }

    pub fn shutdown(&self) {
        self.mark_transport_closed();
        if let Some(key) = &self.key {
            debug!(provider = %key.provider_id, workspace = %key.workspace_root, "shutting down ACP adapter connection");
        }
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = stdin.flush();
        }
        let pid = self.pid();
        if pid != 0 {
            let _ = kill_process_tree(pid);
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_stdout(connection: Arc<AdapterConnection>, stdout: impl std::io::Read + Send + 'static) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        match line {
            Ok(line) if line.trim().is_empty() => {}
            Ok(line) => match serde_json::from_str::<Value>(&line) {
                Ok(value) => route_inbound_frame(&connection, value),
                Err(error) => warn!(%error, line = %line, "invalid ACP stdout frame"),
            },
            Err(error) => {
                warn!(%error, "failed reading ACP stdout");
                break;
            }
        }
    }
    connection.mark_transport_closed();
}

fn route_inbound_frame(connection: &AdapterConnection, value: Value) {
    if value.get("method").is_none() {
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            if let Some(tx) = connection
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id))
            {
                let _ = tx.send(value);
                return;
            }
        }
        return;
    }

    if let Some(session_id) = session_id_from_frame(&value) {
        if let Some(tx) = connection
            .session_routes
            .lock()
            .ok()
            .and_then(|routes| routes.get(session_id).cloned())
        {
            let _ = tx.send(value);
            return;
        }
    }

    warn!(frame = %value, "ACP inbound frame had no registered session route");
}

fn session_id_from_frame(value: &Value) -> Option<&str> {
    let params = value.get("params")?;
    params
        .get("sessionId")
        .or_else(|| params.get("session_id"))
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("update")
                .and_then(|update| update.get("sessionId").or_else(|| update.get("session_id")))
                .and_then(Value::as_str)
        })
}

#[derive(Default)]
pub struct AdapterConnectionManager {
    connections: Mutex<HashMap<AdapterConnectionKey, Arc<AdapterConnection>>>,
    attempt_sessions: Mutex<HashMap<String, LiveAcpSession>>,
}

impl AdapterConnectionManager {
    pub fn shared() -> &'static Self {
        &ADAPTER_CONNECTION_MANAGER
    }

    pub fn get_or_spawn(
        &self,
        provider_id: &str,
        config: &AcpAdapterConfig,
        workspace_root: Utf8PathBuf,
        use_local_claude: bool,
    ) -> Result<Arc<AdapterConnection>> {
        Ok(self
            .get_or_spawn_with_outcome(provider_id, config, workspace_root, use_local_claude)?
            .connection)
    }

    pub fn get_or_spawn_with_outcome(
        &self,
        provider_id: &str,
        config: &AcpAdapterConfig,
        workspace_root: Utf8PathBuf,
        use_local_claude: bool,
    ) -> Result<AdapterConnectionResolution> {
        let key = AdapterConnectionKey::new(provider_id, workspace_root);
        let signature = AdapterConfigSignature::new(config, use_local_claude);
        if let Some(existing) = self.existing_ready_connection(&key, &signature) {
            return Ok(AdapterConnectionResolution {
                connection: existing,
                outcome: AdapterConnectionOutcome::Reused,
            });
        }

        let stale = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .remove(&key);
        let outcome = if let Some(stale) = stale {
            stale.shutdown();
            AdapterConnectionOutcome::ReplacedStale
        } else {
            AdapterConnectionOutcome::Spawned
        };

        let connection = AdapterConnection::spawn(
            Some(key.clone()),
            config,
            &key.workspace_root,
            use_local_claude,
        )?;
        self.connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .insert(key, Arc::clone(&connection));
        Ok(AdapterConnectionResolution {
            connection,
            outcome,
        })
    }

    fn existing_ready_connection(
        &self,
        key: &AdapterConnectionKey,
        signature: &AdapterConfigSignature,
    ) -> Option<Arc<AdapterConnection>> {
        let connection = self.connections.lock().ok()?.get(key).cloned()?;
        if connection.signature != *signature
            || connection.is_exited()
            || connection.is_transport_closed()
        {
            return None;
        }
        Some(connection)
    }

    pub fn register_attempt_session(
        &self,
        attempt_dir: &Utf8Path,
        key: AdapterConnectionKey,
        session_id: String,
    ) {
        if let Ok(mut attempts) = self.attempt_sessions.lock() {
            attempts.insert(attempt_dir.to_string(), LiveAcpSession { key, session_id });
        }
    }

    pub fn unregister_attempt_session(&self, attempt_dir: &Utf8Path) {
        if let Ok(mut attempts) = self.attempt_sessions.lock() {
            attempts.remove(attempt_dir.as_str());
        }
    }

    pub fn attempt_session(&self, attempt_dir: &Utf8Path) -> Option<LiveAcpSession> {
        self.attempt_sessions
            .lock()
            .ok()
            .and_then(|attempts| attempts.get(attempt_dir.as_str()).cloned())
    }

    pub fn cancel_attempt_prompt(&self, attempt_dir: &Utf8Path) -> Result<bool> {
        let Some(session) = self.attempt_session(attempt_dir) else {
            return Ok(false);
        };
        let Some(connection) = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .get(&session.key)
            .cloned()
        else {
            self.unregister_attempt_session(attempt_dir);
            return Ok(false);
        };
        let frame = connection.send_cancel_notification(&session.session_id)?;
        let raw_path = attempt_dir.join("acp.raw.jsonl");
        let _ = append_raw_frame(
            raw_path.as_path(),
            "outbound",
            frame,
            CLOSE_RAW_MAX_SIZE,
            CLOSE_RAW_TARGET_SIZE,
        );
        Ok(true)
    }

    pub fn close_attempt_session_bounded(
        &self,
        attempt_dir: &Utf8Path,
        timeout: Duration,
    ) -> Result<bool> {
        let Some(session) = self.attempt_session(attempt_dir) else {
            return Ok(false);
        };
        let Some(connection) = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .get(&session.key)
            .cloned()
        else {
            self.unregister_attempt_session(attempt_dir);
            return Ok(false);
        };
        connection.close_session_bounded(&session.session_id, timeout)?;
        self.unregister_attempt_session(attempt_dir);
        Ok(true)
    }

    pub fn close_workspace_connections_bounded(
        &self,
        workspace_root: &Utf8Path,
        timeout: Duration,
    ) -> Result<()> {
        let keys = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .keys()
            .filter(|key| key.workspace_root == workspace_root)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.close_connection_bounded(&key, timeout)?;
        }
        Ok(())
    }

    pub fn close_provider_workspace_bounded(
        &self,
        provider_id: &str,
        workspace_root: &Utf8Path,
        timeout: Duration,
    ) -> Result<()> {
        let key = AdapterConnectionKey::new(provider_id, workspace_root.to_path_buf());
        self.close_connection_bounded(&key, timeout)
    }

    pub fn close_all_connections_bounded(&self, timeout: Duration) -> Result<()> {
        let keys = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.close_connection_bounded(&key, timeout)?;
        }
        Ok(())
    }

    fn close_connection_bounded(
        &self,
        key: &AdapterConnectionKey,
        timeout: Duration,
    ) -> Result<()> {
        let Some(connection) = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .get(key)
            .cloned()
        else {
            return Ok(());
        };
        let sessions = self
            .attempt_sessions
            .lock()
            .map_err(|_| anyhow!("ACP attempt session lock poisoned"))?
            .iter()
            .filter(|(_, session)| &session.key == key)
            .map(|(attempt_dir, session)| (attempt_dir.clone(), session.session_id.clone()))
            .collect::<Vec<_>>();
        let mut closed_attempts = Vec::new();
        let mut close_errors = Vec::new();
        for (attempt_dir, session_id) in sessions {
            let raw_path = Utf8PathBuf::from(&attempt_dir).join("acp.raw.jsonl");
            if let Err(error) = connection.close_session_bounded_with_raw_log(
                &session_id,
                timeout,
                Some(raw_path.as_path()),
            ) {
                close_errors.push(format!("{attempt_dir}: {error}"));
            }
            closed_attempts.push(attempt_dir);
        }
        if let Ok(mut attempts) = self.attempt_sessions.lock() {
            for attempt_dir in closed_attempts {
                attempts.remove(&attempt_dir);
            }
        }
        let removed = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?
            .remove(key);
        if let Some(connection) = removed {
            connection.shutdown();
        }
        if close_errors.is_empty() {
            Ok(())
        } else {
            Err(Error::msg(format!(
                "failed to close ACP sessions: {}",
                close_errors.join("; ")
            )))
        }
    }

    pub fn has_active_prompts_in_workspace(&self, workspace_root: &Utf8Path) -> bool {
        self.connections
            .lock()
            .map(|connections| {
                connections.iter().any(|(key, connection)| {
                    key.workspace_root == workspace_root && connection.active_prompt_count() > 0
                })
            })
            .unwrap_or(false)
    }

    pub fn has_active_prompts_in_provider_workspace(
        &self,
        provider_id: &str,
        workspace_root: &Utf8Path,
    ) -> bool {
        let key = AdapterConnectionKey::new(provider_id, workspace_root.to_path_buf());
        self.connections
            .lock()
            .ok()
            .and_then(|connections| connections.get(&key).cloned())
            .is_some_and(|connection| connection.active_prompt_count() > 0)
    }
}

static ADAPTER_CONNECTION_MANAGER: LazyLock<AdapterConnectionManager> =
    LazyLock::new(AdapterConnectionManager::default);

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use serde_json::json;

    use super::{AdapterConnectionKey, session_id_from_frame};

    #[test]
    fn connection_key_is_provider_and_workspace_only() {
        let workspace = Utf8PathBuf::from("/repo");

        let first = AdapterConnectionKey::new("claude-acp", workspace.clone());
        let second = AdapterConnectionKey::new("claude-acp", workspace.clone());
        let other_provider = AdapterConnectionKey::new("codex-acp", workspace.clone());
        let other_workspace = AdapterConnectionKey::new("claude-acp", Utf8PathBuf::from("/other"));

        assert_eq!(first, second);
        assert_ne!(first, other_provider);
        assert_ne!(first, other_workspace);
    }

    #[test]
    fn session_id_routes_direct_and_nested_updates() {
        let direct = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "sessionId": "session-a" }
        });
        let nested = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": { "update": { "sessionId": "session-b" } }
        });

        assert_eq!(session_id_from_frame(&direct), Some("session-a"));
        assert_eq!(session_id_from_frame(&nested), Some("session-b"));
    }
}
