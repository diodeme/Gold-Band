//! Remote SSH terminal sessions (Phase C: direct connect) for the managed
//! "Connections & Terminal" module.
//!
//! SSH sessions are transient, like local ones, and share the same global
//! session ceiling (`MAX_TERMINAL_SESSIONS`) via the `Arc<AtomicUsize>` counter.
//! Unlike local pty sessions, the transport is async (`russh`, tokio-based), so
//! this manager owns a dedicated multi-threaded [`tokio::runtime::Runtime`] and a
//! per-session async connect flow. Open is non-blocking: it reserves a slot,
//! records a `Connecting` session, and returns immediately while the connect /
//! auth / channel flow runs on the runtime, emitting `Output` / `AuthPrompt` /
//! `Failed` / `Exited` events through the shared [`TerminalEventSink`].
//!
//! Authenticating a remote shell follows the OpenSSH keyboard-interactive model:
//! password / public-key first, falling back to keyboard-interactive (used by
//! OTP / MFA logins). OTP prompts are pushed to the frontend as `AuthPrompt`
//! events and the answer is handed back via `provide_auth`. The answer is held in
//! a one-shot channel and never persisted.
//!
//! Host-key trust flows through [`crate::app::known_hosts::KnownHostsStore`]
//! (record-on-first-contact, reject-on-mismatch). Jump (bastion) forwarding uses
//! a fully-resolved nested gateway request (阶段 D)：connect + authenticate to the
//! gateway, `direct-tcpip` forward, then a nested SSH session over it. Sessions
//! are kept alive at two layers: russh transport keepalive (30s) and pty-level
//! activity injection (`space + backspace` every 60s) so bastion idle timers do
//! not log the user out mid-conversation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use russh::client::{AuthResult, Config, Handle, KeyboardInteractiveAuthResponse, Msg};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelReadHalf, ChannelWriteHalf};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

use super::ids::generate_uuid;
use super::known_hosts::{KnownHostsError, KnownHostsStore};
use super::terminal_sessions::{
    ConnectionRunLocator, TerminalEvent, TerminalEventSink, TerminalSessionKind,
    TerminalSessionStatus, TerminalSessionView, release_session_slot, try_reserve_session_slot,
};

const TERMINAL_ID_PREFIX: &str = "sess-";
/// Time budget for the TCP + SSH handshake before we surface a connect timeout.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// How long a keyboard-interactive (OTP / MFA) prompt waits for the user before
/// the connect flow gives up with `terminal.auth-required`. Bounds the wait so a
/// session driven by the agent's `open_connection` tool can never hang forever
/// on a prompt nobody answers. Long enough for a hardware token / SMS round trip.
const AUTH_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Retained terminal-failure codes kept after a session dies, so a consumer can
/// still learn why (`recent_failure_code`).
const RECENT_FAILURE_CAPACITY: usize = 64;
/// pty 层保活：周期性向会话注入 `space + backspace`——在 shell 提示符下不可见、
/// 不产生输出，但计入堡垒机的「用户活动」空闲计时，防止会话被自动登出后要求
/// 重新输入动态口令。字节序列做成常量，真机验证齐治是否将其计数，不计数则调整。
const PTY_KEEPALIVE_BYTES: &[u8] = b"\x20\x7f";
const PTY_KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// Initial terminal geometry requested on the remote pty.
const DEFAULT_COLS: u32 = 80;
const DEFAULT_ROWS: u32 = 24;

/// Credential material resolved by the command layer before opening a session.
/// Never carries anything that should be persisted; secrets come from the OS
/// credential store for the connection id.
#[derive(Debug, Clone)]
pub enum SshAuth {
    Password {
        password: String,
    },
    PublicKey {
        key_path: String,
        passphrase: Option<String>,
    },
}

/// Resolved, connection-level request to open a remote shell.
#[derive(Debug, Clone)]
pub struct SshOpenRequest {
    pub connection_id: String,
    pub connection_name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
    /// Fully-resolved bastion gateway to reach this target through (standard
    /// SSH gateway: connect + authenticate to the gateway, `direct-tcpip`
    /// forward to the target, then nest a second SSH session over it). The
    /// command layer resolves the stored jump reference — with its credentials
    /// — before the request reaches the SSH layer.
    pub jump: Option<Box<SshOpenRequest>>,
    /// 发起该会话的对话 run：由连接 MCP 工具（`open_connection`）打开时携带，
    /// 用户在终端面板手动打开时为 `None`。决定口令输入由谁承接（会话内输入框 /
    /// 壳层弹窗），随会话条目存续到会话结束。
    pub origin: Option<ConnectionRunLocator>,
}

impl SshOpenRequest {
    /// 打上「由该对话 run 发起」的来源标记。只作用于目标请求本身：跳板网关的
    /// 嵌套请求不携带来源（同一会话条目已持有 origin，口令提示按会话而非按跳路由）。
    pub fn with_origin(mut self, origin: ConnectionRunLocator) -> Self {
        self.origin = Some(origin);
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SshSessionCommandError {
    #[error("terminal.session-not-found")]
    SessionNotFound,
    #[error("terminal.session-limit")]
    SessionLimit,
    #[error("terminal.credential-missing")]
    CredentialMissing,
    #[error("terminal.ssh-auth-failed")]
    AuthFailed { reason: String },
    /// A keyboard-interactive (OTP / MFA) prompt went unanswered past
    /// `AUTH_PROMPT_TIMEOUT` — the connect flow aborted rather than waiting
    /// forever for input the user never provided.
    #[error("terminal.auth-required")]
    AuthRequired,
    #[error("terminal.host-key-mismatch")]
    HostKeyMismatch,
    #[error("terminal.connect-timeout")]
    ConnectTimeout,
    #[error("terminal.connection-lost")]
    ConnectionLost { reason: String },
    #[error("terminal.channel-open-failed")]
    ChannelFailed { reason: String },
    /// The gateway accepted the SSH session but refused / failed the
    /// `direct-tcpip` forward to the target (port closed, filtered, or the
    /// nested handshake over the forwarded channel failed).
    #[error("terminal.jump-forward-failed")]
    JumpForwardFailed { reason: String },
    #[error("terminal.io")]
    Io { reason: String },
}

impl SshSessionCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SessionNotFound => "terminal.session-not-found",
            Self::SessionLimit => "terminal.session-limit",
            Self::CredentialMissing => "terminal.credential-missing",
            Self::AuthFailed { .. } => "terminal.ssh-auth-failed",
            Self::AuthRequired => "terminal.auth-required",
            Self::HostKeyMismatch => "terminal.host-key-mismatch",
            Self::ConnectTimeout => "terminal.connect-timeout",
            Self::ConnectionLost { .. } => "terminal.connection-lost",
            Self::ChannelFailed { .. } => "terminal.channel-open-failed",
            Self::JumpForwardFailed { .. } => "terminal.jump-forward-failed",
            Self::Io { .. } => "terminal.io",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::SessionNotFound
            | Self::SessionLimit
            | Self::CredentialMissing
            | Self::HostKeyMismatch
            | Self::ConnectTimeout
            | Self::AuthRequired => serde_json::json!({}),
            Self::AuthFailed { reason } => serde_json::json!({ "reason": reason }),
            Self::ConnectionLost { reason } => serde_json::json!({ "reason": reason }),
            Self::ChannelFailed { reason } => serde_json::json!({ "reason": reason }),
            Self::JumpForwardFailed { reason } => serde_json::json!({ "reason": reason }),
            Self::Io { reason } => serde_json::json!({ "reason": reason }),
        }
    }
}

/// Server key-verification handler: delegates trust to the managed known-hosts
/// store so we never consult the user's real `~/.ssh/known_hosts`.
struct ClientHandler {
    known_hosts: KnownHostsStore,
    host: String,
    port: u16,
}

#[derive(Debug, thiserror::Error)]
enum SshHandlerError {
    #[error("russh: {0}")]
    Russh(#[from] russh::Error),
    #[error("host key mismatch for `{host}`")]
    HostKeyMismatch { host: String },
}

impl russh::client::Handler for ClientHandler {
    type Error = SshHandlerError;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let known_hosts = self.known_hosts.clone();
        let host = self.host.clone();
        let port = self.port;
        async move {
            match server_public_key {
                PublicKeyOrCertificate::PublicKey { key, .. } => {
                    match known_hosts.check_or_record(&host, port, key) {
                        Ok(_) => Ok(true),
                        Err(KnownHostsError::KeyChanged { .. }) => {
                            Err(SshHandlerError::HostKeyMismatch { host })
                        }
                        // IO / parse failures should not silently accept a key.
                        Err(_) => Ok(false),
                    }
                }
                // Certificate auth is out of M1 scope; accept so the type is total.
                PublicKeyOrCertificate::Certificate(_) => Ok(true),
            }
        }
    }
}

/// Live state for one remote session. Split halves: the reader task owns the read
/// half; the write half lives behind an `AsyncMutex` so `write` / `resize` can be
/// awaited from the manager's blocking command context.
struct SshSessionEntry {
    session_id: String,
    connection_name: String,
    host: String,
    port: u16,
    status: TerminalSessionStatus,
    /// 发起该会话的对话 run（见 [`SshOpenRequest::origin`]）。存续到会话结束，
    /// 每次口令提示都据此路由输入界面。
    origin: Option<ConnectionRunLocator>,
    writer: Arc<tokio::sync::Mutex<Option<ChannelWriteHalf<Msg>>>>,
    auth_tx: Option<oneshot::Sender<String>>,
    reader_task: Option<tokio::task::JoinHandle<()>>,
    handle: Option<Handle<ClientHandler>>,
    /// The gateway session the target's `direct-tcpip` channel rides on. Held
    /// for the entry's lifetime so the forwarded channel (and with it the
    /// nested target session) stays open; dropped with the entry on close.
    gateway_handle: Option<Handle<ClientHandler>>,
    /// Whether the global session slot is still held by this entry.
    slot_held: bool,
}

/// Async SSH session manager. Owns the connect runtime and the live-session map.
/// It is shared (`Arc`) so the connect flow can reference itself while running.
pub struct SshSessionManager {
    runtime: Arc<Runtime>,
    sink: TerminalEventSink,
    known_hosts: KnownHostsStore,
    session_count: Arc<AtomicUsize>,
    inner: Mutex<HashMap<String, SshSessionEntry>>,
    /// How long a keyboard-interactive prompt waits for the user. Production
    /// uses `AUTH_PROMPT_TIMEOUT`; tests shorten it via the builder below so a
    /// timeout case can be exercised without a two-minute wait.
    auth_prompt_timeout: std::time::Duration,
    /// Failure code for a session that has already terminated, keyed by
    /// `session_id`. `fail_session` removes the live entry, so a consumer that
    /// observes a session disappearing (the agent's `read_output` tool) would
    /// otherwise lose the reason. Bounded to the most recent
    /// `RECENT_FAILURE_CAPACITY` entries — a session is only looked up right
    /// after it dies, so a small ring is sufficient and cannot grow unbounded.
    recent_failures: Mutex<std::collections::VecDeque<(String, String)>>,
    /// pty 层保活间隔（生产 60s；测试注入短间隔）。`with_pty_keepalive_interval`。
    pty_keepalive_interval: std::time::Duration,
}

impl SshSessionManager {
    pub fn new(
        sink: TerminalEventSink,
        session_count: Arc<AtomicUsize>,
        known_hosts_path: PathBuf,
        runtime: Runtime,
    ) -> Self {
        Self {
            runtime: Arc::new(runtime),
            sink,
            known_hosts: KnownHostsStore::new(known_hosts_path),
            session_count,
            inner: Mutex::new(HashMap::new()),
            auth_prompt_timeout: AUTH_PROMPT_TIMEOUT,
            pty_keepalive_interval: PTY_KEEPALIVE_INTERVAL,
            recent_failures: Mutex::new(std::collections::VecDeque::new()),
        }
    }

    /// Overrides the pty keepalive interval (tests use a short one to observe
    /// the injected bytes through the echo server).
    pub fn with_pty_keepalive_interval(mut self, interval: std::time::Duration) -> Self {
        self.pty_keepalive_interval = interval;
        self
    }

    /// The failure code that ended `session_id`, if that session has already
    /// terminated. Survives the entry removal in `fail_session` so a consumer
    /// observing the session disappear can still report *why*.
    pub fn recent_failure_code(&self, session_id: &str) -> Option<String> {
        self.recent_failures
            .lock()
            .ok()?
            .iter()
            .rev()
            .find(|(id, _)| id == session_id)
            .map(|(_, code)| code.clone())
    }

    pub(crate) fn record_terminal_failure(&self, session_id: &str, code: &str) {
        let Ok(mut failures) = self.recent_failures.lock() else {
            return;
        };
        failures.push_back((session_id.to_string(), code.to_string()));
        while failures.len() > RECENT_FAILURE_CAPACITY {
            failures.pop_front();
        }
    }

    /// Overrides the keyboard-interactive prompt timeout (tests use a short one).
    pub fn with_auth_prompt_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.auth_prompt_timeout = timeout;
        self
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |inner| inner.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read-model views for every live remote session.
    pub fn list(&self) -> Vec<TerminalSessionView> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        let mut views = inner
            .values()
            .map(|entry| TerminalSessionView {
                session_id: entry.session_id.clone(),
                kind: TerminalSessionKind::Remote,
                cwd: None,
                label: Some(remote_label(
                    &entry.connection_name,
                    &entry.host,
                    entry.port,
                )),
                status: entry.status,
                origin: entry.origin.clone(),
            })
            .collect::<Vec<_>>();
        views.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        views
    }

    /// Opens a remote session — direct, or nested through the request's
    /// resolved gateway when one is attached. Non-blocking: the connect flow
    /// runs on the runtime. The returned view is in `Connecting` state.
    pub fn open_remote(self: &Arc<Self>, request: SshOpenRequest) -> Result<TerminalSessionView> {
        if !try_reserve_session_slot(&self.session_count) {
            return Err(SshSessionCommandError::SessionLimit.into());
        }

        let session_id = format!("{TERMINAL_ID_PREFIX}{}", generate_uuid());
        let entry = SshSessionEntry {
            session_id: session_id.clone(),
            connection_name: request.connection_name.clone(),
            host: request.host.clone(),
            port: request.port,
            status: TerminalSessionStatus::Connecting,
            origin: request.origin.clone(),
            writer: Arc::new(tokio::sync::Mutex::new(None)),
            auth_tx: None,
            reader_task: None,
            handle: None,
            gateway_handle: None,
            slot_held: true,
        };
        {
            let mut inner = self.inner.lock().map_err(poisoned)?;
            inner.insert(session_id.clone(), entry);
        }

        // Compute the label + origin before `request` is moved into the connect flow.
        let label = remote_label(&request.connection_name, &request.host, request.port);
        let request_origin = request.origin.clone();
        let manager = Arc::clone(self);
        let sid = session_id.clone();
        self.runtime.spawn(async move {
            if let Err(error) = connect_and_serve(manager.clone(), sid.clone(), request).await {
                manager.fail_session(&sid, error);
            }
        });

        Ok(TerminalSessionView {
            session_id,
            kind: TerminalSessionKind::Remote,
            cwd: None,
            label: Some(label),
            status: TerminalSessionStatus::Connecting,
            origin: request_origin,
        })
    }

    /// Hands an OTP / keyboard-interactive answer back to the connecting flow.
    pub fn provide_auth(&self, session_id: &str, answer: &str) -> Result<()> {
        let tx = {
            let mut inner = self.inner.lock().map_err(poisoned)?;
            let entry = inner
                .get_mut(session_id)
                .ok_or_else(|| SshSessionCommandError::SessionNotFound)?;
            entry
                .auth_tx
                .take()
                .ok_or_else(|| SshSessionCommandError::Io {
                    reason: "no pending authentication prompt".into(),
                })?
        };
        // If the receiver is gone the connect flow ended; nothing to alert on.
        let _ = tx.send(answer.to_string());
        Ok(())
    }

    /// Writes base64-encoded input bytes to the remote shell.
    pub fn write(&self, session_id: &str, base64: &str) -> Result<()> {
        let bytes = STANDARD
            .decode(base64)
            .map_err(|error| SshSessionCommandError::Io {
                reason: format!("invalid base64 input: {error}"),
            })?;
        let writer = self.writer_for(session_id)?;
        self.runtime.block_on(async move {
            let mut guard = writer.lock().await;
            let Some(writer) = guard.as_mut() else {
                return Err(SshSessionCommandError::SessionNotFound);
            };
            writer
                .data_bytes(bytes)
                .await
                .map_err(|error| SshSessionCommandError::ConnectionLost {
                    reason: error.to_string(),
                })
        })?;
        Ok(())
    }

    /// Synchronizes the remote pty window size.
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let writer = self.writer_for(session_id)?;
        self.runtime.block_on(async move {
            let mut guard = writer.lock().await;
            let Some(writer) = guard.as_mut() else {
                return Err(SshSessionCommandError::SessionNotFound);
            };
            writer
                .window_change(cols as u32, rows as u32, 0, 0)
                .await
                .map_err(|error| SshSessionCommandError::ConnectionLost {
                    reason: error.to_string(),
                })
        })?;
        Ok(())
    }

    /// Closes a session: aborts the reader task, drops the SSH handle so the
    /// connection is torn down, releases the slot, and emits a terminal `Exited`.
    pub fn close(&self, session_id: &str) -> Result<()> {
        let entry = {
            let mut inner = self.inner.lock().map_err(poisoned)?;
            inner.remove(session_id)
        };
        let Some(mut entry) = entry else {
            return Err(SshSessionCommandError::SessionNotFound.into());
        };
        if let Some(task) = entry.reader_task.take() {
            task.abort();
        }
        // Dropping auth_tx unblocks a pending auth exchange so it errors out.
        let _ = entry.auth_tx.take();
        if entry.slot_held {
            entry.slot_held = false;
            release_session_slot(&self.session_count);
        }
        drop(entry);
        (self.sink)(TerminalEvent::Exited {
            session_id: session_id.to_string(),
            exit_code: 0,
            status: TerminalSessionStatus::Closed,
        });
        Ok(())
    }

    /// Closes every live session. Called on app teardown.
    pub fn close_all(&self) {
        let ids = {
            let Ok(inner) = self.inner.lock() else { return };
            inner.keys().cloned().collect::<Vec<_>>()
        };
        for id in ids {
            let _ = self.close(&id);
        }
    }

    /// Reader task exit path: mark the session closed, drop connection resources,
    /// and release the slot. Kept separate from `close` so a naturally-terminated
    /// shell clears the cap without needing an explicit tab close.
    async fn on_shell_exit(&self, session_id: &str) {
        let writer = {
            let mut inner = self.inner.lock().map_err(poisoned).ok();
            let Some(entry) = inner.as_mut().and_then(|inner| inner.get_mut(session_id)) else {
                return;
            };
            if entry.slot_held {
                entry.slot_held = false;
                release_session_slot(&self.session_count);
            }
            entry.handle = None;
            entry.reader_task = None;
            entry.status = TerminalSessionStatus::Closed;
            Arc::clone(&entry.writer)
        };
        *writer.lock().await = None;
        (self.sink)(TerminalEvent::Exited {
            session_id: session_id.to_string(),
            exit_code: 0,
            status: TerminalSessionStatus::Closed,
        });
    }

    /// Connect-flow failure path: remove the failed attempt, release the slot, and
    /// surface a structured `Failed` event plus a terminal `Exited`.
    fn fail_session(&self, session_id: &str, error: SshSessionCommandError) {
        let entry = {
            let inner = self.inner.lock().map_err(poisoned).ok();
            inner.and_then(|mut inner| inner.remove(session_id))
        };
        let Some(mut entry) = entry else {
            return;
        };
        if let Some(task) = entry.reader_task.take() {
            task.abort();
        }
        if entry.slot_held {
            release_session_slot(&self.session_count);
        }
        let code = error.code().to_string();
        let params = error.params();
        drop(entry);
        // Retain the reason so a consumer that sees the session disappear (the
        // agent's `read_output` tool) can still report why it failed.
        self.record_terminal_failure(session_id, &code);
        (self.sink)(TerminalEvent::Failed {
            session_id: session_id.to_string(),
            code,
            params,
        });
        (self.sink)(TerminalEvent::Exited {
            session_id: session_id.to_string(),
            exit_code: 0,
            status: TerminalSessionStatus::Closed,
        });
    }

    fn writer_for(
        &self,
        session_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<Option<ChannelWriteHalf<Msg>>>>> {
        let inner = self.inner.lock().map_err(poisoned)?;
        let entry = inner
            .get(session_id)
            .ok_or_else(|| SshSessionCommandError::SessionNotFound)?;
        Ok(Arc::clone(&entry.writer))
    }
}

impl Drop for SshSessionManager {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// Runs the async connect + serve flow for `session_id`. On any failure the error
/// is propagated to `connect_and_serve`'s caller, which reports it via
/// `fail_session`.
async fn connect_and_serve(
    manager: Arc<SshSessionManager>,
    session_id: String,
    request: SshOpenRequest,
) -> Result<(), SshSessionCommandError> {
    let config = Arc::new(Config {
        nodelay: true,
        // 传输层保活：服务器 30s 无数据即发 keepalive，保 TCP/NAT 路径。注意它
        // 不计入堡垒机的「用户活动」空闲计时——那由 pty 层 keepalive 负责。
        keepalive_interval: Some(std::time::Duration::from_secs(30)),
        ..Config::default()
    });
    // Standard SSH gateway path (阶段 D): connect + authenticate to the gateway,
    // forward a `direct-tcpip` channel to the target, then nest a second SSH
    // session over the forwarded stream. The gateway handle stays open for the
    // entry's lifetime — dropping it closes the forwarded channel and with it
    // the nested session.
    let (mut handle, gateway_handle) = match request.jump.as_deref() {
        Some(gateway) => {
            let mut gateway_handler = ClientHandler {
                known_hosts: manager.known_hosts.clone(),
                host: gateway.host.clone(),
                port: gateway.port,
            };
            let mut outer = tokio::time::timeout(
                CONNECT_TIMEOUT,
                russh::client::connect(
                    Arc::clone(&config),
                    (gateway.host.as_str(), gateway.port),
                    gateway_handler,
                ),
            )
            .await
            .map_err(|_| SshSessionCommandError::ConnectTimeout)?
            .map_err(|error| SshSessionCommandError::ConnectionLost {
                reason: error.to_string(),
            })?;
            authenticate(&manager, &session_id, &mut outer, gateway).await?;
            let forwarded = outer
                .channel_open_direct_tcpip(
                    request.host.as_str(),
                    u32::from(request.port),
                    "127.0.0.1",
                    0,
                )
                .await
                .map_err(|error| SshSessionCommandError::JumpForwardFailed {
                    reason: format!("direct-tcpip to {}:{}: {error}", request.host, request.port),
                })?;
            let target_handler = ClientHandler {
                known_hosts: manager.known_hosts.clone(),
                host: request.host.clone(),
                port: request.port,
            };
            let stream = forwarded.into_stream();
            let inner = tokio::time::timeout(
                CONNECT_TIMEOUT,
                russh::client::connect_stream(config, stream, target_handler),
            )
            .await
            .map_err(|_| SshSessionCommandError::ConnectTimeout)?
            .map_err(|error| SshSessionCommandError::JumpForwardFailed {
                reason: format!("nested ssh handshake over forwarded channel: {error}"),
            })?;
            (inner, Some(outer))
        }
        None => {
            let target_handler = ClientHandler {
                known_hosts: manager.known_hosts.clone(),
                host: request.host.clone(),
                port: request.port,
            };
            let handle = tokio::time::timeout(
                CONNECT_TIMEOUT,
                russh::client::connect(
                    config,
                    (request.host.as_str(), request.port),
                    target_handler,
                ),
            )
            .await
            .map_err(|_| SshSessionCommandError::ConnectTimeout)?
            .map_err(|error| SshSessionCommandError::ConnectionLost {
                reason: error.to_string(),
            })?;
            (handle, None)
        }
    };

    authenticate(&manager, &session_id, &mut handle, &request).await?;

    let channel = handle.channel_open_session().await.map_err(|error| {
        SshSessionCommandError::ChannelFailed {
            reason: format!("open session: {error}"),
        }
    })?;
    channel
        .request_pty(
            true,
            "xterm-256color",
            DEFAULT_COLS,
            DEFAULT_ROWS,
            0,
            0,
            &[],
        )
        .await
        .map_err(|error| SshSessionCommandError::ChannelFailed {
            reason: format!("request pty: {error}"),
        })?;
    channel
        .request_shell(true)
        .await
        .map_err(|error| SshSessionCommandError::ChannelFailed {
            reason: format!("request shell: {error}"),
        })?;
    let (read_half, write_half) = channel.split();

    // Stream remote output; on natural exit the reader finalizes the session. The
    // read half is owned entirely by the reader task.
    let reader = spawn_reader(
        manager.sink.clone(),
        session_id.clone(),
        manager.clone(),
        read_half,
    );

    // Hand the write half to the entry, then publish the live handle + status.
    let writer = {
        let inner = manager.inner.lock().map_err(poisoned)?;
        inner
            .get(&session_id)
            .map(|entry| Arc::clone(&entry.writer))
            .ok_or_else(|| SshSessionCommandError::SessionNotFound)?
    };
    *writer.lock().await = Some(write_half);
    {
        let mut inner = manager.inner.lock().map_err(poisoned)?;
        let entry = inner
            .get_mut(&session_id)
            .ok_or_else(|| SshSessionCommandError::SessionNotFound)?;
        entry.handle = Some(handle);
        entry.gateway_handle = gateway_handle;
        entry.reader_task = Some(reader);
        entry.status = TerminalSessionStatus::Connected;
    }
    // pty 层保活（连接→对话）：周期性注入 space+backspace，计入堡垒机的用户活动
    // 空闲计时，防止动态口令会话因无动作被自动登出后要求重新输入 OTP。任务自终止：
    // 会话关闭/失败时 writer 消失或写入出错，循环退出。
    spawn_pty_keepalive(manager, session_id);
    Ok(())
}

/// 周期性向存活远程会话的 pty 注入保活字节。会话结束（entry 移除 / 写入失败）时
/// 任务自行退出，无需显式清理。
fn spawn_pty_keepalive(manager: Arc<SshSessionManager>, session_id: String) {
    let runtime = Arc::clone(&manager.runtime);
    let interval = manager.pty_keepalive_interval;
    runtime.spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // 首个 tick 立即返回，跳过
        loop {
            ticker.tick().await;
            let writer = match manager.writer_for(&session_id) {
                Ok(writer) => writer,
                Err(_) => break,
            };
            let mut guard = writer.lock().await;
            let Some(writer) = guard.as_mut() else { break };
            if writer
                .data_bytes(PTY_KEEPALIVE_BYTES.to_vec())
                .await
                .is_err()
            {
                break;
            }
        }
    });
}

/// Authenticates the connection. Password / public-key first; any failure falls
/// back to keyboard-interactive so OTP / MFA logins still work.
async fn authenticate(
    manager: &SshSessionManager,
    session_id: &str,
    handle: &mut Handle<ClientHandler>,
    request: &SshOpenRequest,
) -> Result<(), SshSessionCommandError> {
    let auth = match &request.auth {
        SshAuth::Password { password } => {
            handle
                .authenticate_password(request.username.as_str(), password.as_str())
                .await
        }
        SshAuth::PublicKey {
            key_path,
            passphrase,
        } => {
            let key =
                russh::keys::load_secret_key(key_path, passphrase.as_deref()).map_err(|error| {
                    SshSessionCommandError::AuthFailed {
                        reason: format!("load private key: {error}"),
                    }
                })?;
            let hash = key.algorithm().is_rsa().then_some(HashAlg::Sha256);
            handle
                .authenticate_publickey(
                    request.username.as_str(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
        }
    };
    match auth {
        Ok(AuthResult::Success) => Ok(()),
        Ok(AuthResult::Failure { .. }) => {
            keyboard_interactive(manager, session_id, handle, request.username.as_str()).await
        }
        Err(error) => Err(SshSessionCommandError::AuthFailed {
            reason: error.to_string(),
        }),
    }
}

/// Keyboard-interactive (OTP / MFA) auth. Each `InfoRequest` is surfaced to the
/// frontend as an `AuthPrompt` event; the answer is awaited on a one-shot channel.
async fn keyboard_interactive(
    manager: &SshSessionManager,
    session_id: &str,
    handle: &mut Handle<ClientHandler>,
    username: &str,
) -> Result<(), SshSessionCommandError> {
    let mut response = handle
        .authenticate_keyboard_interactive_start(username, None::<String>)
        .await
        .map_err(|error| SshSessionCommandError::AuthFailed {
            reason: error.to_string(),
        })?;
    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure { .. } => {
                return Err(SshSessionCommandError::AuthFailed {
                    reason: "server rejected keyboard-interactive authentication".into(),
                });
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts: _,
            } => {
                let message = prompt_message(&name, &instructions);
                let (tx, rx) = oneshot::channel();
                let origin = {
                    let mut inner = manager.inner.lock().map_err(poisoned)?;
                    let entry = inner
                        .get_mut(session_id)
                        .ok_or_else(|| SshSessionCommandError::SessionNotFound)?;
                    entry.auth_tx = Some(tx);
                    entry.status = TerminalSessionStatus::AwaitingAuth;
                    entry.origin.clone()
                };
                // 事故可审计：口令等待是 120s 的持续状态，但界面只由本事件触发；
                // 没有这条日志时「没弹输入框」无法区分「事件没发」和「前端没收」。
                tracing::debug!(
                    target: "gold_band::app::terminal",
                    session_id,
                    message = %message,
                    origin = origin.as_ref().map(|o| format!("{}/{}/{}", o.project_id, o.task_id, o.run_id)).as_deref().unwrap_or("-"),
                    timeout_ms = manager.auth_prompt_timeout.as_millis() as u64,
                    "terminal auth prompt awaited"
                );
                (manager.sink)(TerminalEvent::AuthPrompt {
                    session_id: session_id.to_string(),
                    message,
                    origin,
                });
                // Bounded wait: a prompt nobody answers (e.g. an agent-driven
                // `open_connection` with no user in front of the UI) must fail
                // with a structured `terminal.auth-required` rather than hang the
                // connect flow — and, through it, the agent's tool call forever.
                let answer = match tokio::time::timeout(manager.auth_prompt_timeout, rx).await {
                    Ok(Ok(answer)) => {
                        tracing::debug!(
                            target: "gold_band::app::terminal",
                            session_id,
                            "terminal auth prompt answered"
                        );
                        answer
                    }
                    Ok(Err(_)) => {
                        return Err(SshSessionCommandError::AuthFailed {
                            reason: "authentication prompt was cancelled".into(),
                        });
                    }
                    Err(_elapsed) => {
                        tracing::debug!(
                            target: "gold_band::app::terminal",
                            session_id,
                            "terminal auth prompt timed out"
                        );
                        return Err(SshSessionCommandError::AuthRequired);
                    }
                };
                response = handle
                    .authenticate_keyboard_interactive_respond(vec![answer])
                    .await
                    .map_err(|error| SshSessionCommandError::AuthFailed {
                        reason: error.to_string(),
                    })?;
            }
        }
    }
}

fn spawn_reader(
    sink: TerminalEventSink,
    session_id: String,
    manager: Arc<SshSessionManager>,
    read_half: ChannelReadHalf,
) -> tokio::task::JoinHandle<()> {
    let runtime = Arc::clone(&manager.runtime);
    runtime.spawn(serve_reader(sink, session_id, manager, read_half))
}

async fn serve_reader(
    sink: TerminalEventSink,
    session_id: String,
    manager: Arc<SshSessionManager>,
    mut read_half: ChannelReadHalf,
) {
    loop {
        match read_half.wait().await {
            Some(ChannelMsg::Data { data }) => {
                let base64 = STANDARD.encode(data.as_ref());
                (sink)(TerminalEvent::Output {
                    session_id: session_id.clone(),
                    base64,
                });
            }
            Some(ChannelMsg::Eof) => continue,
            Some(ChannelMsg::Close) | None => break,
            Some(_) => continue,
        }
    }
    manager.on_shell_exit(&session_id).await;
}

/// Tab label for a remote session: `连接名 (host:port)`.
fn remote_label(name: &str, host: &str, port: u16) -> String {
    format!("{name} ({host}:{port})")
}

/// Unified poison-error mapping for the internal session map.
fn poisoned<T>(_: std::sync::PoisonError<T>) -> SshSessionCommandError {
    SshSessionCommandError::Io {
        reason: "session state poisoned".into(),
    }
}

/// Builds the user-facing text for a keyboard-interactive prompt. Kept as pure
/// data (name + instructions) so the frontend can format it; no secret material
/// is included.
fn prompt_message(name: &str, instructions: &str) -> String {
    match (name.trim(), instructions.trim()) {
        (name, instructions) if !name.is_empty() && !instructions.is_empty() => {
            format!("{name}\n{instructions}")
        }
        (name, "") if !name.is_empty() => name.to_string(),
        ("", instructions) if !instructions.is_empty() => instructions.to_string(),
        _ => "Server requests authentication".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::terminal_sessions::{MAX_TERMINAL_SESSIONS, TerminalSessionManager};
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};
    use russh::server::Server as _;
    use russh::server::{self, Auth, ChannelOpenHandle, Msg, Session};
    use russh::{Channel, ChannelId, MethodKind, MethodSet, Pty};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    // ---- helpers -----------------------------------------------------------

    fn channel_sink() -> (TerminalEventSink, std::sync::mpsc::Receiver<TerminalEvent>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Arc::new(move |event: TerminalEvent| {
                let _ = tx.send(event);
            }),
            rx,
        )
    }

    fn request(host: &str, port: u16, jump: Option<String>) -> SshOpenRequest {
        // The `jump` argument is a gateway host:port pair encoded as "host:port"
        // (or None for direct), resolved into a nested gateway request the same
        // way the command layer resolves a stored jump reference.
        let jump = jump.map(|gateway| {
            let (gateway_host, gateway_port) = gateway.rsplit_once(':').expect("host:port");
            Box::new(SshOpenRequest {
                connection_id: "conn-gateway".to_string(),
                connection_name: "Gateway".to_string(),
                host: gateway_host.to_string(),
                port: gateway_port.parse().expect("gateway port"),
                username: "user".to_string(),
                auth: SshAuth::Password {
                    password: "secret".to_string(),
                },
                jump: None,
                origin: None,
            })
        });
        SshOpenRequest {
            connection_id: "conn-test".to_string(),
            connection_name: "Test Host".to_string(),
            host: host.to_string(),
            port,
            username: "user".to_string(),
            auth: SshAuth::Password {
                password: "secret".to_string(),
            },
            jump,
            origin: None,
        }
    }

    /// 连接 MCP 工具路径的请求：带上发起它的对话 run 定位。
    fn request_for_run(host: &str, port: u16, run_id: &str) -> SshOpenRequest {
        request(host, port, None).with_origin(ConnectionRunLocator {
            project_id: "project-a".to_string(),
            task_id: "task-1".to_string(),
            run_id: run_id.to_string(),
        })
    }

    fn ssh_manager(sink: TerminalEventSink) -> (Arc<SshSessionManager>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let runtime = tokio::runtime::Runtime::new().expect("ssh runtime");
        let known_hosts = std::env::temp_dir()
            .join(format!(
                "gold-band-ssh-mgr-{}",
                super::super::ids::generate_uuid()
            ))
            .join("known_hosts");
        // Sync-only tests never connect, so the file stays lazily uncreated.
        let manager = Arc::new(SshSessionManager::new(
            sink,
            Arc::clone(&count),
            known_hosts,
            runtime,
        ));
        (manager, count)
    }

    fn terminal_manager(sink: TerminalEventSink) -> TerminalSessionManager {
        let known_hosts = std::env::temp_dir()
            .join(format!(
                "gold-band-ssh-test-{}",
                super::super::ids::generate_uuid()
            ))
            .join("known_hosts");
        TerminalSessionManager::new(sink, known_hosts)
    }

    fn unused_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        port
    }

    fn session_output_contains(
        rx: &std::sync::mpsc::Receiver<TerminalEvent>,
        needle: &str,
    ) -> bool {
        let mut acc = String::new();
        let deadline = std::time::Duration::from_secs(10);
        while let Ok(event) = rx.recv_timeout(deadline) {
            match event {
                TerminalEvent::Output { base64, .. } => {
                    let bytes = STANDARD.decode(&base64).expect("valid base64");
                    acc.push_str(&String::from_utf8_lossy(&bytes));
                    if acc.contains(needle) {
                        return true;
                    }
                }
                TerminalEvent::Failed { code, .. } => {
                    panic!("session failed before shell output: {code}")
                }
                TerminalEvent::Exited { .. } => return acc.contains(needle),
                TerminalEvent::AuthPrompt { .. } => {}
            }
        }
        acc.contains(needle)
    }

    fn wait_for_failed_event(rx: &std::sync::mpsc::Receiver<TerminalEvent>) -> bool {
        let deadline = std::time::Duration::from_secs(5);
        while let Ok(event) = rx.recv_timeout(deadline) {
            match event {
                TerminalEvent::Failed { .. } => return true,
                TerminalEvent::Exited { .. } => return false,
                _ => {}
            }
        }
        false
    }

    fn wait_for_slot_release(count: &AtomicUsize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while count.load(Ordering::SeqCst) != 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    // ---- synchronous rejections --------------------------------------------
    // (Phase C's jump rejection was replaced by the Phase D gateway path; jump
    // connectivity is covered end-to-end below against in-process servers.)

    /// Spawns an in-process SSH gateway that accepts password auth and bridges
    /// every `direct-tcpip` request to the given TCP target address, pumping
    /// bytes bidirectionally between the forwarded channel and the target.
    /// Returns the gateway's loopback port.
    fn spawn_jump_gateway(target_host: String, target_port: u16) -> std::sync::mpsc::Receiver<u16> {
        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("gateway runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
                let port = listener.local_addr().expect("local addr").port();
                let _ = port_tx.send(port);
                let config = Arc::new(server::Config {
                    keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()],
                    auth_rejection_time: std::time::Duration::ZERO,
                    auth_rejection_time_initial: Some(std::time::Duration::ZERO),
                    ..Default::default()
                });
                let mut gateway = JumpServer {
                    target_host,
                    target_port,
                };
                gateway
                    .run_on_socket(config, &listener)
                    .await
                    .expect("gateway run");
            });
        });
        port_rx
    }

    #[test]
    fn opens_remote_session_through_a_jump_gateway() {
        // Target: the ordinary echo SSH server below. Gateway: an SSH server
        // that only forwards direct-tcpip. The client must nest: gateway auth →
        // direct-tcpip → nested SSH auth → shell output reaches the sink.
        let (target_port_tx, target_port_rx) = std::sync::mpsc::channel::<u16>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("target runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
                let port = listener.local_addr().expect("local addr").port();
                let _ = target_port_tx.send(port);
                let config = Arc::new(server::Config {
                    keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()],
                    auth_rejection_time: std::time::Duration::ZERO,
                    auth_rejection_time_initial: Some(std::time::Duration::ZERO),
                    ..Default::default()
                });
                let mut echo = EchoServer;
                echo.run_on_socket(config, &listener)
                    .await
                    .expect("target run");
            });
        });
        let target_port = target_port_rx.recv().expect("target reported a port");
        let gateway_port_rx = spawn_jump_gateway("127.0.0.1".to_string(), target_port);
        let gateway_port = gateway_port_rx.recv().expect("gateway reported a port");

        let (sink, rx) = channel_sink();
        let mut manager = terminal_manager(sink);
        let view = manager
            .open_remote(request(
                "127.0.0.1",
                target_port,
                Some(format!("127.0.0.1:{gateway_port}")),
            ))
            .unwrap();
        assert_eq!(view.kind, TerminalSessionKind::Remote);

        assert!(
            session_output_contains(&rx, "HELLO_SSH"),
            "expected the target's shell output through the gateway"
        );
        manager
            .write(&view.session_id, &STANDARD.encode(b"echo x\r\n".as_slice()))
            .unwrap();
        assert!(
            session_output_contains(&rx, "echo x"),
            "expected the target to echo input through the gateway"
        );
        manager.close(&view.session_id).unwrap();
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn jump_forward_to_an_unreachable_target_fails_structured() {
        // Gateway up, but the forwarded target port has nothing listening: the
        // connect flow must surface `terminal.jump-forward-failed` (or a
        // connect timeout mapped from the handshake), not hang or misreport.
        let unreachable = unused_port();
        let gateway_port_rx = spawn_jump_gateway("127.0.0.1".to_string(), unreachable);
        let gateway_port = gateway_port_rx.recv().expect("gateway reported a port");

        let (sink, rx) = channel_sink();
        let manager = terminal_manager(sink);
        let view = manager
            .open_remote(request(
                "127.0.0.1",
                unreachable,
                Some(format!("127.0.0.1:{gateway_port}")),
            ))
            .unwrap();
        assert!(session_failed_with(&rx, "terminal.jump-forward-failed"));
        drop(view);
    }

    fn session_failed_with(rx: &std::sync::mpsc::Receiver<TerminalEvent>, code: &str) -> bool {
        let deadline = std::time::Duration::from_secs(30);
        while let Ok(event) = rx.recv_timeout(deadline) {
            if let TerminalEvent::Failed {
                code: failed_code, ..
            } = event
            {
                return failed_code == code;
            }
            if let TerminalEvent::Exited { .. } = event {
                return false;
            }
        }
        false
    }

    #[test]
    fn open_remote_respects_global_session_limit() {
        let (sink, _rx) = channel_sink();
        let (manager, count) = ssh_manager(sink);
        for _ in 0..MAX_TERMINAL_SESSIONS {
            assert!(try_reserve_session_slot(&count));
        }
        let error = manager
            .open_remote(request("127.0.0.1", 22, None))
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<SshSessionCommandError>(),
            Some(SshSessionCommandError::SessionLimit)
        ));
    }

    #[test]
    fn write_to_unknown_session_is_not_found() {
        let (sink, _rx) = channel_sink();
        let (manager, _count) = ssh_manager(sink);
        let error = manager
            .write("sess-missing", &STANDARD.encode(b"x"))
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<SshSessionCommandError>(),
            Some(SshSessionCommandError::SessionNotFound)
        ));
    }

    #[test]
    fn provide_auth_with_unknown_session_is_not_found() {
        let (sink, _rx) = channel_sink();
        let (manager, _count) = ssh_manager(sink);
        let error = manager.provide_auth("sess-missing", "123456").unwrap_err();
        assert!(matches!(
            error.downcast_ref::<SshSessionCommandError>(),
            Some(SshSessionCommandError::SessionNotFound)
        ));
    }

    #[test]
    fn unreachable_server_emits_failed_and_releases_slot() {
        let (sink, rx) = channel_sink();
        let (manager, count) = ssh_manager(sink);
        let view = manager
            .open_remote(request("127.0.0.1", unused_port(), None))
            .unwrap();
        assert_eq!(view.kind, TerminalSessionKind::Remote);
        assert_eq!(view.status, TerminalSessionStatus::Connecting);
        assert_eq!(manager.len(), 1);

        assert!(
            wait_for_failed_event(&rx),
            "expected a Failed event for the unreachable server"
        );
        wait_for_slot_release(&count);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    // ---- in-process SSH server end-to-end ----------------------------------

    #[test]
    #[test]
    fn pty_keepalive_injects_activity_that_the_bastion_echoes() {
        // 保活回归：短间隔 keepalive 向 pty 注入 space+backspace；echo 服务器把
        // 它回显回来——证明字节真实到达了远端（即堡垒机会将其计入活动计时）。
        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        let server_thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("server runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
                let port = listener.local_addr().expect("local addr").port();
                let _ = port_tx.send(port);
                let config = Arc::new(server::Config {
                    keys: vec![PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()],
                    auth_rejection_time: std::time::Duration::ZERO,
                    auth_rejection_time_initial: Some(std::time::Duration::ZERO),
                    ..Default::default()
                });
                let mut echo = EchoServer;
                echo.run_on_socket(config, &listener)
                    .await
                    .expect("server run");
            });
        });
        let port = port_rx.recv().expect("server reported a port");

        let (sink, rx) = channel_sink();
        let count = Arc::new(AtomicUsize::new(0));
        let runtime = tokio::runtime::Runtime::new().expect("ssh runtime");
        let known_hosts = std::env::temp_dir()
            .join(format!(
                "gold-band-ssh-ka-{}",
                super::super::ids::generate_uuid()
            ))
            .join("known_hosts");
        let manager = Arc::new(
            SshSessionManager::new(sink, count, known_hosts, runtime)
                .with_pty_keepalive_interval(std::time::Duration::from_millis(300)),
        );
        let view = manager
            .open_remote(request("127.0.0.1", port, None))
            .unwrap();

        // Keepalive 注入的 space+backspace 会被 echo 服务器原样回显。
        assert!(
            session_output_contains(&rx, " \x7f"),
            "expected the echoed keepalive bytes (space + backspace)"
        );
        manager.close(&view.session_id).unwrap();
    }

    fn opens_remote_session_and_streams_shell_output() {
        // An in-process SSH server that accepts a password and echoes back the
        // client's input on a bound loopback channel. It runs on its own OS
        // thread + runtime (run_on_socket borrows the listener, so it cannot be
        // spawned on the caller's runtime) and reports its loopback port back.
        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let config = Arc::new(server::Config {
            keys: vec![host_key],
            auth_rejection_time: std::time::Duration::ZERO,
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            ..Default::default()
        });
        let server_thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("server runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
                let port = listener.local_addr().expect("local addr").port();
                let _ = port_tx.send(port);
                let mut echo = EchoServer;
                echo.run_on_socket(config, &listener)
                    .await
                    .expect("server run");
            });
        });
        let _ = server_thread;
        let port = port_rx.recv().expect("server reported a port");

        let (sink, rx) = channel_sink();
        let mut manager = terminal_manager(sink);

        let view = manager
            .open_remote(request("127.0.0.1", port, None))
            .unwrap();
        assert_eq!(view.kind, TerminalSessionKind::Remote);
        assert_eq!(view.status, TerminalSessionStatus::Connecting);
        assert_eq!(manager.len(), 1);

        // Shell output from the server reaches the sink.
        assert!(
            session_output_contains(&rx, "HELLO_SSH"),
            "expected the server's shell output"
        );

        // Input round-trips: write to the channel, expect the server's echo back.
        manager
            .write(&view.session_id, &STANDARD.encode(b"echo x\r\n".as_slice()))
            .unwrap();
        assert!(
            session_output_contains(&rx, "echo x"),
            "expected the server to echo written input"
        );

        manager.close(&view.session_id).unwrap();
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn keyboard_interactive_prompt_times_out_as_auth_required() {
        // An in-process SSH server that only offers keyboard-interactive and
        // never accepts, so the client reaches the OTP wait and then has no
        // answer to give. With a short prompt timeout this must surface
        // `terminal.auth-required` instead of hanging the connect flow forever.
        let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
        let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let config = Arc::new(server::Config {
            keys: vec![host_key],
            auth_rejection_time: std::time::Duration::ZERO,
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            ..Default::default()
        });
        let server_thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("server runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
                let port = listener.local_addr().expect("local addr").port();
                let _ = port_tx.send(port);
                let mut otp = OtpServer;
                otp.run_on_socket(config, &listener)
                    .await
                    .expect("server run");
            });
        });
        let port = port_rx.recv().expect("server reported a port");

        let (sink, rx) = channel_sink();
        let count = Arc::new(AtomicUsize::new(0));
        let runtime = tokio::runtime::Runtime::new().expect("ssh runtime");
        let known_hosts = std::env::temp_dir()
            .join(format!(
                "gold-band-ssh-otp-{}",
                super::super::ids::generate_uuid()
            ))
            .join("known_hosts");
        let manager = Arc::new(
            SshSessionManager::new(sink, Arc::clone(&count), known_hosts, runtime)
                .with_auth_prompt_timeout(std::time::Duration::from_millis(300)),
        );

        // 会话驱动路径（连接 MCP 工具）：origin 必须随会话建立、视图与口令事件
        // 全程带出——前端据此把口令输入路由回发起它的对话。
        let view = manager
            .open_remote(request_for_run("127.0.0.1", port, "run-otp"))
            .unwrap();
        assert_eq!(
            view.origin.as_ref().map(|origin| origin.run_id.as_str()),
            Some("run-otp"),
            "open view must carry the conversation run origin"
        );

        let mut saw_prompt = false;
        let mut prompt_origin_run = None;
        let mut failed_code = None;
        let deadline = std::time::Duration::from_secs(10);
        while let Ok(event) = rx.recv_timeout(deadline) {
            match event {
                TerminalEvent::AuthPrompt { origin, .. } => {
                    saw_prompt = true;
                    prompt_origin_run = origin.map(|origin| origin.run_id);
                }
                TerminalEvent::Failed { code, .. } => {
                    failed_code = Some(code);
                    break;
                }
                TerminalEvent::Exited { .. } => break,
                TerminalEvent::Output { .. } => {}
            }
        }
        assert!(
            saw_prompt,
            "server should have prompted for keyboard-interactive input"
        );
        assert_eq!(
            prompt_origin_run.as_deref(),
            Some("run-otp"),
            "auth prompt event must carry the conversation run origin"
        );
        assert_eq!(failed_code.as_deref(), Some("terminal.auth-required"));
        // The aborted session is cleaned up: removed from the map and its slot freed.
        assert_eq!(manager.len(), 0, "timed-out session must be cleaned up");
        assert_eq!(
            count.load(Ordering::Relaxed),
            0,
            "timed-out session must release its global session slot"
        );
        // The failure reason survives the entry removal, so a consumer that saw
        // the session disappear can still report *why*.
        assert_eq!(
            manager.recent_failure_code(&view.session_id).as_deref(),
            Some("terminal.auth-required"),
        );

        drop(view);
        drop(server_thread);
    }

    // ---- test SSH server ---------------------------------------------------

    /// An in-process SSH gateway: accepts the password, then bridges every
    /// `direct-tcpip` request to a fixed TCP target, pumping bytes both ways so
    /// a nested SSH client behind the gateway reaches the target.
    struct JumpServer {
        target_host: String,
        target_port: u16,
    }

    impl server::Server for JumpServer {
        type Handler = JumpHandler;

        fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self::Handler {
            JumpHandler {
                target_host: self.target_host.clone(),
                target_port: self.target_port,
            }
        }
    }

    struct JumpHandler {
        target_host: String,
        target_port: u16,
    }

    impl server::Handler for JumpHandler {
        type Error = russh::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            password: &str,
        ) -> Result<Auth, Self::Error> {
            Ok(if password == "secret" {
                Auth::Accept
            } else {
                Auth::reject()
            })
        }

        async fn channel_open_direct_tcpip(
            &mut self,
            channel: Channel<Msg>,
            host_to_connect: &str,
            port_to_connect: u32,
            _originator_address: &str,
            _originator_port: u32,
            reply: ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            let Ok(target_port) = u16::try_from(port_to_connect) else {
                return Ok(());
            };
            let target = tokio::net::TcpStream::connect((host_to_connect, target_port)).await;
            let mut channel_stream = channel.into_stream();
            let Ok(mut target_stream) = target else {
                // Forwarding failed: close the channel so the client's nested
                // handshake hits EOF and surfaces a structured error.
                return Ok(());
            };
            tokio::spawn(async move {
                let _ =
                    tokio::io::copy_bidirectional(&mut channel_stream, &mut target_stream).await;
            });
            Ok(())
        }
    }

    /// A server that rejects password and only speaks keyboard-interactive: the
    /// client falls back to the OTP prompt, which this server then never accepts.
    struct OtpServer;

    impl server::Server for OtpServer {
        type Handler = OtpHandler;

        fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self::Handler {
            OtpHandler
        }
    }

    struct OtpHandler;

    impl server::Handler for OtpHandler {
        type Error = russh::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<Auth, Self::Error> {
            Ok(Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                partial_success: false,
            })
        }

        async fn auth_keyboard_interactive(
            &mut self,
            _user: &str,
            _submethods: &str,
            response: Option<russh::server::Response<'_>>,
        ) -> Result<Auth, Self::Error> {
            match response {
                // First round: issue the OTP challenge so the client surfaces it.
                None => Ok(Auth::Partial {
                    name: std::borrow::Cow::Borrowed("OTP"),
                    instructions: std::borrow::Cow::Borrowed("Enter the one-time code"),
                    prompts: std::borrow::Cow::Borrowed(
                        &[(std::borrow::Cow::Borrowed("Code: "), false)][..],
                    ),
                }),
                // Never accept the answer — the client must time out.
                Some(_) => Ok(Auth::reject()),
            }
        }
    }

    #[derive(Clone)]
    struct EchoServer;

    impl server::Server for EchoServer {
        type Handler = EchoHandler;

        fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self::Handler {
            EchoHandler
        }
    }

    struct EchoHandler;

    impl server::Handler for EchoHandler {
        type Error = russh::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            password: &str,
        ) -> Result<Auth, Self::Error> {
            Ok(if password == "secret" {
                Auth::Accept
            } else {
                Auth::reject()
            })
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<Msg>,
            reply: ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        async fn pty_request(
            &mut self,
            channel: ChannelId,
            _term: &str,
            _cw: u32,
            _rh: u32,
            _pw: u32,
            _ph: u32,
            _modes: &[(Pty, u32)],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            Ok(())
        }

        async fn shell_request(
            &mut self,
            channel: ChannelId,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            session.data(channel, b"HELLO_SSH\r\n".to_vec())?;
            Ok(())
        }

        async fn data(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            // Echo the client's input so a write round-trip can be observed.
            session.data(channel, data.to_vec())?;
            Ok(())
        }
    }
}
