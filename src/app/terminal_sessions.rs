//! In-memory terminal session manager for the "Connections & Terminal" module.
//!
//! Sessions are transient: they live only in process memory and are destroyed
//! when their tab is closed or the app exits. This module owns PTY spawning /
//! interaction for local shells and is decoupled from Tauri via a
//! [`TerminalEventSink`] so the desktop crate can wire output to `app.emit`.
//!
//! Local sessions are the kind implemented here; remote SSH sessions (Phases
//! C/D) are intended to plug into the same manager through the same event
//! surface. Byte streams are binary-safe: output is chunked and base64-encoded,
//! input is base64-decoded before writing to the pty, so arbitrary UTF-8 and
//! wide-character output never round-trips through JSON number arrays.
//!
//! Backpressure comes from the pty itself: the reader thread blocks on `read`,
//! so a slow consumer (e.g. a large `tail -f`) throttles the shell's writes by
//! not draining the pty buffer. We never buffer unboundedly in memory.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::ids::generate_uuid;
use super::ssh_sessions::{SshOpenRequest, SshSessionManager};

/// Upper bound for concurrent terminal sessions (design: 4–8). Reaching it
/// disables new session creation until a tab is closed.
pub const MAX_TERMINAL_SESSIONS: usize = 8;

const TERMINAL_ID_PREFIX: &str = "sess-";
/// Read buffer for pty output. Chunks are capped so a single IPC emit never
/// carries an arbitrarily large payload.
const READ_BUF_SIZE: usize = 32 * 1024;

/// Sync sink that receives every terminal event. The desktop crate wraps this in
/// `app.emit`; tests capture events via a channel.
pub type TerminalEventSink = Arc<dyn Fn(TerminalEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalSessionKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalSessionStatus {
    Connecting,
    AwaitingAuth,
    Connected,
    Closed,
}

/// 打开远程会话的对话 run 定位。会话级作用域，用完整 locator（project/task/run）
/// 而非末级 id：口令输入要按「哪个对话的哪一轮」路由回会话内输入框。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRunLocator {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
}

/// A read model of a live session for the frontend tab bar.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionView {
    pub session_id: String,
    pub kind: TerminalSessionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Free-form tab title override (used by remote sessions: `连接名 (host:port)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub status: TerminalSessionStatus,
    /// 发起该会话的对话 run（连接 MCP 工具路径）；用户在终端面板手动打开时为
    /// `None`。口令输入据此选择承接界面：会话驱动的连接在会话内给出输入框，
    /// 手动连接走壳层弹窗。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ConnectionRunLocator>,
}

/// Every event pushed to the sink for a session. Internally tagged so the
/// frontend can discriminate on a stable `event` field.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TerminalEvent {
    /// Base64-encoded pty output chunk for `session_id`.
    Output {
        session_id: String,
        base64: String,
    },
    /// `status` is a terminal state (Closed) once the process ends.
    Exited {
        session_id: String,
        exit_code: i32,
        status: TerminalSessionStatus,
    },
    /// The server needs keyboard-interactive input (e.g. an OTP) before the
    /// connection completes. The frontend prompts once and replies via
    /// `provide_terminal_auth`; the answer never persists.
    AuthPrompt {
        session_id: String,
        message: String,
        /// 发起会话的对话 run（见 [`TerminalSessionView::origin`]）。事件携带它，
        /// 会话视图无需先查列表即可直接路由口令输入。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<ConnectionRunLocator>,
    },
    /// A remote session failed before reaching a shell (connect / auth / channel).
    /// `code` + `params` are structured and never carry user-facing copy.
    Failed {
        session_id: String,
        code: String,
        params: serde_json::Value,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum TerminalSessionCommandError {
    #[error("terminal.session-not-found")]
    SessionNotFound,
    #[error("terminal.session-limit")]
    SessionLimit,
    #[error("terminal.local-start-failed")]
    LocalStartFailed { reason: String },
    #[error("terminal.invalid-cwd")]
    InvalidCwd { cwd: String },
    #[error("terminal.io")]
    Io { reason: String },
}

impl TerminalSessionCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SessionNotFound => "terminal.session-not-found",
            Self::SessionLimit => "terminal.session-limit",
            Self::LocalStartFailed { .. } => "terminal.local-start-failed",
            Self::InvalidCwd { .. } => "terminal.invalid-cwd",
            Self::Io { .. } => "terminal.io",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::SessionNotFound | Self::SessionLimit => serde_json::json!({}),
            Self::LocalStartFailed { reason } => serde_json::json!({ "reason": reason }),
            Self::InvalidCwd { cwd } => serde_json::json!({ "cwd": cwd }),
            Self::Io { reason } => serde_json::json!({ "reason": reason }),
        }
    }
}

struct LocalTerminalEntry {
    session_id: String,
    cwd: Option<String>,
    master: Box<dyn MasterPty>,
    writer: Box<dyn Write + Send>,
    child: Option<Box<dyn portable_pty::Child>>,
    status: TerminalSessionStatus,
}

/// Owns all live terminal sessions. Interior state is guarded by the caller
/// (the desktop crate typically behind a `Mutex`), so `write`/`resize` take
/// `&self` while `close`-style mutations are expressed through methods that
/// return the entry's state rather than holding a long-lived lock internally.
pub struct TerminalSessionManager {
    sink: TerminalEventSink,
    local: HashMap<String, LocalTerminalEntry>,
    /// Remote SSH sessions share this manager's slot ceiling and event sink but
    /// run their own async connect runtime internally.
    remote: Arc<SshSessionManager>,
    /// Shared count of live sessions across local + remote so the design's
    /// 4–8 ceiling is enforced globally, not per transport.
    session_count: Arc<AtomicUsize>,
}

impl TerminalSessionManager {
    pub fn new(
        sink: TerminalEventSink,
        known_hosts_path: std::path::PathBuf,
    ) -> Self {
        // One shared counter drives the global 4–8 ceiling across local + remote.
        let session_count = Arc::new(AtomicUsize::new(0));
        let runtime = tokio::runtime::Runtime::new().expect("failed to build ssh runtime");
        let remote = Arc::new(SshSessionManager::new(
            Arc::clone(&sink),
            Arc::clone(&session_count),
            known_hosts_path,
            runtime,
        ));
        Self {
            sink,
            local: HashMap::new(),
            remote,
            session_count,
        }
    }

    pub fn len(&self) -> usize {
        self.local.len() + self.remote.len()
    }

    pub fn is_empty(&self) -> bool {
        self.local.is_empty() && self.remote.is_empty()
    }

    /// Merged read model for the tab bar: local + remote, ordered by session id.
    pub fn list(&self) -> Vec<TerminalSessionView> {
        let mut views = self
            .local
            .values()
            .map(|entry| TerminalSessionView {
                session_id: entry.session_id.clone(),
                kind: TerminalSessionKind::Local,
                cwd: entry.cwd.clone(),
                label: None,
                status: entry.status,
                origin: None,
            })
            .collect::<Vec<_>>();
        views.extend(self.remote.list());
        views.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        views
    }

    /// Opens a new local shell session. `cwd` defaults to the user's home when
    /// `None`.
    pub fn open_local(&mut self, cwd: Option<String>) -> Result<TerminalSessionView> {
        if !try_reserve_session_slot(&self.session_count) {
            return Err(TerminalSessionCommandError::SessionLimit.into());
        }

        let session_id = format!("{TERMINAL_ID_PREFIX}{}", generate_uuid());
        let cwd = match cwd {
            Some(cwd) if cwd.trim().is_empty() => None,
            Some(cwd) => {
                let path = std::path::Path::new(&cwd);
                if !path.is_dir() {
                    return Err(TerminalSessionCommandError::InvalidCwd { cwd }.into());
                }
                Some(cwd)
            }
            None => None,
        };
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalSessionCommandError::LocalStartFailed {
                reason: format!("openpty failed: {error}"),
            })?;

        let mut command = CommandBuilder::new(default_shell());
        command.set_controlling_tty(true);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        if let Some(cwd) = cwd.as_deref() {
            command.cwd(cwd);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalSessionCommandError::LocalStartFailed {
                reason: format!("spawn failed: {error}"),
            })?;
        drop(pair.slave);

        let master = pair.master;
        let reader = master
            .try_clone_reader()
            .map_err(|error| TerminalSessionCommandError::LocalStartFailed {
                reason: format!("reader failed: {error}"),
            })?;
        let writer = master
            .take_writer()
            .map_err(|error| TerminalSessionCommandError::LocalStartFailed {
                reason: format!("writer failed: {error}"),
            })?;

        // Reader thread: drains pty output, base64-encodes each chunk, and feeds
        // the sink. It ends on EOF (shell exited or was killed), then emits the
        // Exited event.
        let sink = Arc::clone(&self.sink);
        let reader_session_id = session_id.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = vec![0u8; READ_BUF_SIZE];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(count) => {
                        let base64 = STANDARD.encode(&buf[..count]);
                        (sink)(TerminalEvent::Output {
                            session_id: reader_session_id.clone(),
                            base64,
                        });
                    }
                    Err(error) => {
                        if error.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        break;
                    }
                }
            }
            (sink)(TerminalEvent::Exited {
                session_id: reader_session_id,
                exit_code: 0,
                status: TerminalSessionStatus::Closed,
            });
        });

        let entry = LocalTerminalEntry {
            session_id: session_id.clone(),
            cwd: cwd.clone(),
            master,
            writer,
            child: Some(child),
            status: TerminalSessionStatus::Connected,
        };
        self.local.insert(session_id.clone(), entry);

        Ok(TerminalSessionView {
            session_id,
            kind: TerminalSessionKind::Local,
            cwd,
            label: None,
            status: TerminalSessionStatus::Connected,
            origin: None,
        })
    }

    /// Writes base64-encoded input bytes to the session's pty (keys / paste).
    pub fn write(&mut self, session_id: &str, base64: &str) -> Result<()> {
        if let Some(entry) = self.local.get_mut(session_id) {
            let bytes = STANDARD
                .decode(base64)
                .map_err(|error| TerminalSessionCommandError::Io {
                    reason: format!("invalid base64 input: {error}"),
                })?;
            entry
                .writer
                .write_all(&bytes)
                .map_err(|error| TerminalSessionCommandError::Io {
                    reason: format!("write failed: {error}"),
                })?;
            entry
                .writer
                .flush()
                .map_err(|error| TerminalSessionCommandError::Io {
                    reason: format!("flush failed: {error}"),
                })?;
            return Ok(());
        }
        // Not a local session: forward to the SSH manager.
        self.remote.write(session_id, base64)
    }

    /// Synchronizes the pty window size with the frontend terminal view.
    pub fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<()> {
        if let Some(entry) = self.local.get(session_id) {
            entry
                .master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|error| TerminalSessionCommandError::Io {
                    reason: format!("resize failed: {error}"),
                })?;
            return Ok(());
        }
        self.remote.resize(session_id, rows, cols)
    }

    /// Closes a session: terminates the child process group and removes the
    /// entry. The reader thread sees the pty EOF and emits `Exited`. For remote
    /// sessions the SSH connection is dropped and the slot released.
    pub fn close(&mut self, session_id: &str) -> Result<()> {
        let Some(mut entry) = self.local.remove(session_id) else {
            return self.remote.close(session_id);
        };
        entry.status = TerminalSessionStatus::Closed;
        let mut child = entry.child.take();
        if let Some(child) = child.as_mut() {
            terminate_child(entry.master.as_ref(), child.as_mut());
            let _ = child.wait();
        }
        release_session_slot(&self.session_count);
        Ok(())
    }

    /// Closes every session. Called on app teardown so no pty child is orphaned
    /// and no SSH connection is left hanging.
    pub fn close_all(&mut self) {
        let ids = self.local.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let _ = self.close(&id);
        }
        self.remote.close_all();
    }

    /// Opens a remote SSH session (Phase C: direct). Non-blocking — the connect
    /// flow runs on the manager's runtime and emits `Output` / `AuthPrompt` /
    /// `Failed` / `Exited` events.
    pub fn open_remote(&self, request: SshOpenRequest) -> Result<TerminalSessionView> {
        self.remote.open_remote(request)
    }

    /// Hands a keyboard-interactive / OTP answer back to a connecting remote
    /// session.
    pub fn provide_auth(&self, session_id: &str, answer: &str) -> Result<()> {
        self.remote.provide_auth(session_id, answer)
    }

    /// The failure code that ended `session_id`, if it already terminated.
    /// Lets a consumer that saw the session disappear (the agent's `read_output`
    /// tool) still report *why* it failed.
    pub fn recent_failure_code(&self, session_id: &str) -> Option<String> {
        self.remote.recent_failure_code(session_id)
    }

    /// Records a terminal failure for `session_id`. Production records via the
    /// connect flow's failure path (`fail_session`); exposed within the crate so
    /// a consumer's failure-reporting can be exercised without a live SSH target.
    pub(crate) fn record_terminal_failure(&self, session_id: &str, code: &str) {
        self.remote.record_terminal_failure(session_id, code);
    }
}

impl Drop for TerminalSessionManager {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// Atomically reserves a session slot under the shared `MAX_TERMINAL_SESSIONS`
/// ceiling. Returns `false` when the ceiling is already reached.
pub(crate) fn try_reserve_session_slot(count: &AtomicUsize) -> bool {
    count
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if current < MAX_TERMINAL_SESSIONS {
                Some(current + 1)
            } else {
                None
            }
        })
        .is_ok()
}

/// Releases a previously reserved session slot.
pub(crate) fn release_session_slot(count: &AtomicUsize) {
    count.fetch_sub(1, Ordering::SeqCst);
}

fn default_shell() -> std::ffi::OsString {
    #[cfg(windows)]
    {
        std::env::var_os("COMSPEC")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "cmd.exe".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("SHELL")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/bin/sh".into())
    }
}

/// Terminates a pty child, preferring the whole foreground process group on
/// Unix so background jobs spawned by the shell are reaped too.
fn terminate_child(master: &dyn MasterPty, child: &mut dyn portable_pty::Child) {
    #[cfg(unix)]
    {
        if let Some(pgid) = master.process_group_leader() {
            unsafe {
                libc::kill(-pgid, libc::SIGHUP);
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_sink() -> (
        TerminalEventSink,
        std::sync::mpsc::Receiver<TerminalEvent>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Arc::new(move |event| {
            let _ = tx.send(event);
        }), rx)
    }

    fn manager(sink: TerminalEventSink) -> TerminalSessionManager {
        // Unique per-call known-hosts path so tests never collide. Local-only
        // tests never touch the file (it is created lazily on first SSH use).
        let known_hosts = std::env::temp_dir()
            .join(format!("gold-band-ssh-test-{}", generate_uuid()))
            .join("known_hosts");
        TerminalSessionManager::new(sink, known_hosts)
    }

    fn session_output_contains(rx: &std::sync::mpsc::Receiver<TerminalEvent>, needle: &str) -> bool {
        let mut acc = String::new();
        let deadline = std::time::Duration::from_secs(10);
        while let Ok(event) = rx.recv_timeout(deadline) {
            match event {
                TerminalEvent::Output { base64, .. } => {
                    let bytes = STANDARD.decode(&base64).unwrap();
                    acc.push_str(&String::from_utf8_lossy(&bytes));
                    if acc.contains(needle) {
                        return true;
                    }
                }
                TerminalEvent::Exited { .. } => return acc.contains(needle),
                TerminalEvent::AuthPrompt { .. } => {}
                TerminalEvent::Failed { .. } => return acc.contains(needle),
            }
        }
        acc.contains(needle)
    }

    #[test]
    fn open_local_spawns_shell_and_streams_echo_output() {
        let (sink, rx) = channel_sink();
        let mut manager = manager(sink);

        let view = manager.open_local(None).unwrap();
        assert_eq!(view.kind, TerminalSessionKind::Local);
        assert_eq!(view.status, TerminalSessionStatus::Connected);
        assert_eq!(manager.len(), 1);

        let echo = "printf 'HELLO_TERMINAL\\n'\r";
        manager
            .write(&view.session_id, &STANDARD.encode(echo.as_bytes()))
            .unwrap();

        let ok = session_output_contains(&rx, "HELLO_TERMINAL");
        manager.close(&view.session_id).unwrap();
        assert!(ok, "expected pty output to echo the prompt marker");

        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn open_local_rejects_missing_cwd() {
        let (sink, _rx) = channel_sink();
        let mut manager = manager(sink);
        let error = manager.open_local(Some("/definitely/not/here".to_string())).unwrap_err();
        assert!(error.downcast_ref::<TerminalSessionCommandError>().is_some());
    }

    #[test]
    fn session_limit_is_enforced() {
        let (sink, _rx) = channel_sink();
        let mut manager = manager(sink);

        let mut opened = Vec::new();
        for _ in 0..MAX_TERMINAL_SESSIONS {
            opened.push(manager.open_local(None).unwrap().session_id);
        }
        let error = manager.open_local(None).unwrap_err();
        assert!(error.downcast_ref::<TerminalSessionCommandError>().is_some());

        // Closing one frees a slot.
        manager.close(&opened[0]).unwrap();
        assert!(manager.open_local(None).is_ok());
    }

    #[test]
    fn write_to_unknown_session_is_not_found() {
        let (sink, _rx) = channel_sink();
        let mut manager = manager(sink);
        let error = manager.write("sess-missing", &STANDARD.encode(b"x")).unwrap_err();
        // The manager routes unknown sessions to the (now remote-backed) handler,
        // which reports the same structured `session-not-found` code.
        let code = if let Some(error) = error.downcast_ref::<TerminalSessionCommandError>() {
            error.code()
        } else if let Some(error) =
            error.downcast_ref::<crate::app::ssh_sessions::SshSessionCommandError>()
        {
            error.code()
        } else {
            panic!("expected a structured session-not-found error");
        };
        assert_eq!(code, "terminal.session-not-found");
    }

    #[test]
    fn terminal_event_payload_uses_camel_case_fields() {
        // The frontend discriminates events on `event` and reads camelCase fields
        // (`sessionId`, `exitCode`). A snake_case `session_id` in the wired payload
        // silently fails the `sessionId !== sessionId` filter and renders nothing.
        let output = serde_json::to_value(TerminalEvent::Output {
            session_id: "sess-abc".to_string(),
            base64: "aGk=".to_string(),
        })
        .unwrap();
        assert_eq!(output["event"], "output");
        assert_eq!(output["sessionId"], "sess-abc");
        assert!(output.get("session_id").is_none());

        let exited = serde_json::to_value(TerminalEvent::Exited {
            session_id: "sess-def".to_string(),
            exit_code: 0,
            status: TerminalSessionStatus::Closed,
        })
        .unwrap();
        assert_eq!(exited["sessionId"], "sess-def");
        assert_eq!(exited["exitCode"], 0);
        assert!(exited.get("session_id").is_none());
        assert!(exited.get("exit_code").is_none());
    }
}
