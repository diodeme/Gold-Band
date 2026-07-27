use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin};
use std::sync::{
    Arc, Condvar, LazyLock, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Error, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::acp::adapter::{ResolvedAcpAdapter, spawn_adapter};
use crate::acp::elicitation::cancel_pending_elicitation_requests;
use crate::acp::events::{append_raw_frame, current_timestamp};
use crate::acp::permission::cancel_pending_permission_requests;
use crate::config::AcpAdapterConfig;
use crate::process::kill_process_tree;
use crate::storage::{ensure_parent_dir, read_json, write_json};

const CLOSE_RAW_MAX_SIZE: u64 = 5 * 1024 * 1024;
const CLOSE_RAW_TARGET_SIZE: u64 = 4 * 1024 * 1024;
const SESSION_ROUTE_MAX_BYTES: usize = 4 * 1024 * 1024;
const SESSION_ROUTE_MAX_FRAMES: usize = 256;
const SESSION_ROUTE_BACKPRESSURE_WARN_AFTER: Duration = Duration::from_millis(250);
const UNROUTED_WARNING_INTERVAL: Duration = Duration::from_secs(60);
const EARLY_SESSION_FRAME_TTL: Duration = Duration::from_secs(5);
const EARLY_SESSION_FRAME_MAX_BYTES: usize = 1024 * 1024;
const EARLY_SESSION_FRAME_MAX_FRAMES: usize = 64;
static NEXT_CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
struct SessionRouteFrame {
    value: Value,
    bytes: usize,
}

#[derive(Debug)]
struct EarlySessionFrame {
    frame: SessionRouteFrame,
    received_at: Instant,
}

#[derive(Debug, Default)]
struct EarlySessionFrames {
    by_session: HashMap<String, VecDeque<EarlySessionFrame>>,
    total_bytes: usize,
    total_frames: usize,
}

impl EarlySessionFrames {
    fn push(&mut self, session_id: &str, value: Value, bytes: usize, now: Instant) -> bool {
        self.purge_expired(now);
        if bytes > EARLY_SESSION_FRAME_MAX_BYTES {
            return false;
        }
        while self.total_frames >= EARLY_SESSION_FRAME_MAX_FRAMES
            || self.total_bytes.saturating_add(bytes) > EARLY_SESSION_FRAME_MAX_BYTES
        {
            if !self.evict_oldest() {
                break;
            }
        }
        self.by_session
            .entry(session_id.to_string())
            .or_default()
            .push_back(EarlySessionFrame {
                frame: SessionRouteFrame { value, bytes },
                received_at: now,
            });
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.total_frames = self.total_frames.saturating_add(1);
        true
    }

    fn take(&mut self, session_id: &str, now: Instant) -> Vec<SessionRouteFrame> {
        self.purge_expired(now);
        let Some(frames) = self.by_session.remove(session_id) else {
            return Vec::new();
        };
        let mut drained = Vec::with_capacity(frames.len());
        for frame in frames {
            self.total_bytes = self.total_bytes.saturating_sub(frame.frame.bytes);
            self.total_frames = self.total_frames.saturating_sub(1);
            drained.push(frame.frame);
        }
        drained
    }

    fn remove(&mut self, session_id: &str) {
        let _ = self.take(session_id, Instant::now());
    }

    fn purge_expired(&mut self, now: Instant) {
        let session_ids = self.by_session.keys().cloned().collect::<Vec<_>>();
        for session_id in session_ids {
            let mut remove_session = false;
            if let Some(frames) = self.by_session.get_mut(&session_id) {
                while frames.front().is_some_and(|frame| {
                    now.duration_since(frame.received_at) >= EARLY_SESSION_FRAME_TTL
                }) {
                    if let Some(expired) = frames.pop_front() {
                        self.total_bytes = self.total_bytes.saturating_sub(expired.frame.bytes);
                        self.total_frames = self.total_frames.saturating_sub(1);
                    }
                }
                remove_session = frames.is_empty();
            }
            if remove_session {
                self.by_session.remove(&session_id);
            }
        }
    }

    fn evict_oldest(&mut self) -> bool {
        let oldest_session = self
            .by_session
            .iter()
            .filter_map(|(session_id, frames)| {
                frames
                    .front()
                    .map(|frame| (session_id.clone(), frame.received_at))
            })
            .min_by_key(|(_, received_at)| *received_at)
            .map(|(session_id, _)| session_id);
        let Some(session_id) = oldest_session else {
            return false;
        };
        let mut remove_session = false;
        if let Some(frames) = self.by_session.get_mut(&session_id) {
            if let Some(frame) = frames.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(frame.frame.bytes);
                self.total_frames = self.total_frames.saturating_sub(1);
            }
            remove_session = frames.is_empty();
        }
        if remove_session {
            self.by_session.remove(&session_id);
        }
        true
    }
}

#[derive(Debug, Default)]
struct SessionRouteState {
    queue: VecDeque<SessionRouteFrame>,
    queued_bytes: usize,
    high_water_bytes: usize,
    high_water_frames: usize,
    closed: bool,
    receiver_alive: bool,
}

#[derive(Debug)]
struct SessionRouteInner {
    state: Mutex<SessionRouteState>,
    not_full: Condvar,
    not_empty: Condvar,
}

impl SessionRouteInner {
    fn new() -> Self {
        Self {
            state: Mutex::new(SessionRouteState {
                receiver_alive: true,
                ..SessionRouteState::default()
            }),
            not_full: Condvar::new(),
            not_empty: Condvar::new(),
        }
    }

    fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
        }
        self.not_full.notify_all();
        self.not_empty.notify_all();
    }
}

#[derive(Clone)]
struct SessionRouteSender {
    inner: Arc<SessionRouteInner>,
    adapter_id: String,
    session_id: String,
}

impl SessionRouteSender {
    fn send(&self, value: Value, bytes: usize) -> bool {
        let started_waiting = Instant::now();
        let mut waited = false;
        let Ok(mut state) = self.inner.state.lock() else {
            return false;
        };
        while !state.closed && state.receiver_alive && !session_route_has_capacity(&state, bytes) {
            waited = true;
            let Ok(next) = self.inner.not_full.wait(state) else {
                return false;
            };
            state = next;
        }
        if state.closed || !state.receiver_alive {
            return false;
        }
        state.queued_bytes = state.queued_bytes.saturating_add(bytes);
        state.queue.push_back(SessionRouteFrame { value, bytes });
        state.high_water_bytes = state.high_water_bytes.max(state.queued_bytes);
        state.high_water_frames = state.high_water_frames.max(state.queue.len());
        let queued_bytes = state.queued_bytes;
        let queued_frames = state.queue.len();
        let high_water_bytes = state.high_water_bytes;
        let high_water_frames = state.high_water_frames;
        drop(state);
        self.inner.not_empty.notify_one();
        if waited && started_waiting.elapsed() >= SESSION_ROUTE_BACKPRESSURE_WARN_AFTER {
            warn!(
                adapter = %self.adapter_id,
                session_id = %self.session_id,
                wait_ms = started_waiting.elapsed().as_millis(),
                queued_bytes,
                queued_frames,
                high_water_bytes,
                high_water_frames,
                "ACP session route applied bounded backpressure"
            );
        }
        true
    }

    fn close(&self) {
        self.inner.close();
    }
}

fn session_route_has_capacity(state: &SessionRouteState, incoming_bytes: usize) -> bool {
    if state.queue.is_empty() {
        return true;
    }
    state.queue.len() < SESSION_ROUTE_MAX_FRAMES
        && state.queued_bytes.saturating_add(incoming_bytes) <= SESSION_ROUTE_MAX_BYTES
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRouteTryRecvError {
    Empty,
    Disconnected,
}

pub struct SessionRouteReceiver {
    inner: Arc<SessionRouteInner>,
}

impl SessionRouteReceiver {
    pub fn try_recv(&self) -> std::result::Result<Value, SessionRouteTryRecvError> {
        let Ok(mut state) = self.inner.state.lock() else {
            return Err(SessionRouteTryRecvError::Disconnected);
        };
        if let Some(frame) = state.queue.pop_front() {
            state.queued_bytes = state.queued_bytes.saturating_sub(frame.bytes);
            drop(state);
            self.inner.not_full.notify_all();
            return Ok(frame.value);
        }
        if state.closed || !state.receiver_alive {
            Err(SessionRouteTryRecvError::Disconnected)
        } else {
            Err(SessionRouteTryRecvError::Empty)
        }
    }

    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Value, mpsc::RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let Ok(mut state) = self.inner.state.lock() else {
            return Err(mpsc::RecvTimeoutError::Disconnected);
        };
        loop {
            if let Some(frame) = state.queue.pop_front() {
                state.queued_bytes = state.queued_bytes.saturating_sub(frame.bytes);
                drop(state);
                self.inner.not_full.notify_all();
                return Ok(frame.value);
            }
            if state.closed || !state.receiver_alive {
                return Err(mpsc::RecvTimeoutError::Disconnected);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(mpsc::RecvTimeoutError::Timeout);
            };
            let Ok((next, wait)) = self.inner.not_empty.wait_timeout(state, remaining) else {
                return Err(mpsc::RecvTimeoutError::Disconnected);
            };
            state = next;
            if wait.timed_out() && state.queue.is_empty() {
                return Err(mpsc::RecvTimeoutError::Timeout);
            }
        }
    }
}

impl Drop for SessionRouteReceiver {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.receiver_alive = false;
            state.queue.clear();
            state.queued_bytes = 0;
        }
        self.inner.not_full.notify_all();
        self.inner.not_empty.notify_all();
    }
}

#[derive(Debug, Default)]
struct SessionEventPumpState {
    queue: VecDeque<SessionRouteFrame>,
    queued_bytes: usize,
    closed: bool,
}

#[derive(Debug)]
struct SessionEventPumpInner {
    state: Mutex<SessionEventPumpState>,
    not_full: Condvar,
}

pub struct SessionEventPump {
    inner: Arc<SessionEventPumpInner>,
    shutdown: Arc<AtomicBool>,
}

impl SessionEventPump {
    fn start(receiver: SessionRouteReceiver) -> Arc<Self> {
        let inner = Arc::new(SessionEventPumpInner {
            state: Mutex::new(SessionEventPumpState::default()),
            not_full: Condvar::new(),
        });
        let shutdown = Arc::new(AtomicBool::new(false));
        let pump = Arc::new(Self {
            inner: Arc::clone(&inner),
            shutdown: Arc::clone(&shutdown),
        });
        thread::spawn(move || {
            while !shutdown.load(Ordering::Acquire) {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(value) => {
                        let bytes = serde_json::to_vec(&value)
                            .map(|bytes| bytes.len())
                            .unwrap_or(0);
                        let Ok(mut state) = inner.state.lock() else {
                            break;
                        };
                        while !state.closed
                            && (state.queue.len() >= SESSION_ROUTE_MAX_FRAMES
                                || (!state.queue.is_empty()
                                    && state.queued_bytes.saturating_add(bytes)
                                        > SESSION_ROUTE_MAX_BYTES))
                        {
                            let Ok(next) = inner.not_full.wait(state) else {
                                return;
                            };
                            state = next;
                        }
                        if state.closed {
                            break;
                        }
                        state.queued_bytes = state.queued_bytes.saturating_add(bytes);
                        state.queue.push_back(SessionRouteFrame { value, bytes });
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            if let Ok(mut state) = inner.state.lock() {
                state.closed = true;
            }
            inner.not_full.notify_all();
        });
        pump
    }

    pub fn try_recv(&self) -> std::result::Result<Value, SessionRouteTryRecvError> {
        let Ok(mut state) = self.inner.state.lock() else {
            return Err(SessionRouteTryRecvError::Disconnected);
        };
        if let Some(frame) = state.queue.pop_front() {
            state.queued_bytes = state.queued_bytes.saturating_sub(frame.bytes);
            drop(state);
            self.inner.not_full.notify_all();
            return Ok(frame.value);
        }
        if state.closed {
            Err(SessionRouteTryRecvError::Disconnected)
        } else {
            Err(SessionRouteTryRecvError::Empty)
        }
    }

    pub fn close(&self) {
        self.shutdown.store(true, Ordering::Release);
        if let Ok(mut state) = self.inner.state.lock() {
            state.closed = true;
        }
        self.inner.not_full.notify_all();
    }
}

fn session_route_pair(
    adapter_id: impl Into<String>,
    session_id: impl Into<String>,
) -> (SessionRouteSender, SessionRouteReceiver) {
    let inner = Arc::new(SessionRouteInner::new());
    (
        SessionRouteSender {
            inner: Arc::clone(&inner),
            adapter_id: adapter_id.into(),
            session_id: session_id.into(),
        },
        SessionRouteReceiver { inner },
    )
}

#[derive(Debug)]
struct UnroutedWarningState {
    last_logged_at: Instant,
    suppressed: u64,
}

fn record_unrouted_warning(
    warnings: &mut HashMap<String, UnroutedWarningState>,
    warning_key: String,
    now: Instant,
) -> Option<u64> {
    if let Some(state) = warnings.get_mut(&warning_key) {
        if now.duration_since(state.last_logged_at) < UNROUTED_WARNING_INTERVAL {
            state.suppressed = state.suppressed.saturating_add(1);
            return None;
        }
        let suppressed = state.suppressed;
        state.last_logged_at = now;
        state.suppressed = 0;
        return Some(suppressed);
    }
    warnings.insert(
        warning_key,
        UnroutedWarningState {
            last_logged_at: now,
            suppressed: 0,
        },
    );
    Some(0)
}

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
    require_local_claude_executable: bool,
}

impl AdapterConfigSignature {
    fn new(
        config: &AcpAdapterConfig,
        use_local_claude: bool,
        require_local_claude_executable: bool,
    ) -> Self {
        Self {
            command: config.command.clone(),
            args: config.args.clone(),
            display_name: config.display_name.clone(),
            env: config.env.clone(),
            use_local_claude,
            require_local_claude_executable,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterConnectionState {
    Open,
    Draining,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpConnectionUnavailable {
    Draining,
    Closed,
}

impl std::fmt::Display for AcpConnectionUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Draining => "ACP adapter connection is draining",
            Self::Closed => "ACP adapter transport is closed",
        })
    }
}

impl std::error::Error for AcpConnectionUnavailable {}

fn request_unavailability(
    state: AdapterConnectionState,
    allow_draining: bool,
) -> Option<AcpConnectionUnavailable> {
    match state {
        AdapterConnectionState::Open => None,
        AdapterConnectionState::Draining if allow_draining => None,
        AdapterConnectionState::Draining => Some(AcpConnectionUnavailable::Draining),
        AdapterConnectionState::Closed => Some(AcpConnectionUnavailable::Closed),
    }
}

#[derive(Debug, Default)]
struct ActivePromptTracker {
    counts: Mutex<HashMap<String, usize>>,
    drained: Condvar,
}

impl ActivePromptTracker {
    fn mark_active(&self, session_id: &str) {
        if let Ok(mut counts) = self.counts.lock() {
            let count = counts.entry(session_id.to_string()).or_default();
            *count = count.saturating_add(1);
        }
    }

    fn mark_inactive(&self, session_id: &str) {
        if let Ok(mut counts) = self.counts.lock() {
            if let Some(count) = counts.get_mut(session_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(session_id);
                }
            }
        }
        self.drained.notify_all();
    }

    fn count(&self) -> usize {
        self.counts
            .lock()
            .map(|counts| counts.values().copied().sum())
            .unwrap_or(0)
    }

    fn count_for_session(&self, session_id: &str) -> usize {
        self.counts
            .lock()
            .ok()
            .and_then(|counts| counts.get(session_id).copied())
            .unwrap_or(0)
    }

    fn wait_for_sessions(&self, session_ids: &[String], timeout: Duration) -> Result<bool> {
        let counts = self
            .counts
            .lock()
            .map_err(|_| anyhow!("ACP active prompt lock poisoned"))?;
        let (counts, _) = self
            .drained
            .wait_timeout_while(counts, timeout, |counts| {
                session_ids
                    .iter()
                    .any(|session_id| counts.get(session_id).copied().unwrap_or(0) > 0)
            })
            .map_err(|_| anyhow!("ACP active prompt lock poisoned"))?;
        Ok(!session_ids
            .iter()
            .any(|session_id| counts.get(session_id).copied().unwrap_or(0) > 0))
    }
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
    session_routes: Mutex<HashMap<String, SessionRouteSender>>,
    early_session_frames: Mutex<EarlySessionFrames>,
    unrouted_warnings: Mutex<HashMap<String, UnroutedWarningState>>,
    initialized_capabilities: Mutex<Option<Value>>,
    active_prompts: ActivePromptTracker,
    generation: u64,
    last_activity_at: Mutex<Instant>,
    session_config_transaction: SessionConfigTransaction,
    state: Mutex<AdapterConnectionState>,
}

pub struct ActivePromptGuard {
    connection: Arc<AdapterConnection>,
    session_id: String,
}

impl Drop for ActivePromptGuard {
    fn drop(&mut self) {
        self.connection
            .active_prompts
            .mark_inactive(&self.session_id);
    }
}

#[derive(Debug, Default)]
struct SessionConfigTransaction {
    lock: Mutex<()>,
}

impl SessionConfigTransaction {
    fn lock(&self) -> Result<MutexGuard<'_, ()>> {
        self.lock
            .lock()
            .map_err(|_| anyhow!("ACP session config transaction lock poisoned"))
    }
}

impl AdapterConnection {
    pub fn spawn_standalone(
        config: &AcpAdapterConfig,
        cwd: &Utf8Path,
        use_local_claude: bool,
        require_local_claude_executable: bool,
    ) -> Result<Arc<Self>> {
        Self::spawn(
            None,
            config,
            cwd,
            use_local_claude,
            require_local_claude_executable,
        )
    }

    fn spawn(
        key: Option<AdapterConnectionKey>,
        config: &AcpAdapterConfig,
        cwd: &Utf8Path,
        use_local_claude: bool,
        require_local_claude_executable: bool,
    ) -> Result<Arc<Self>> {
        let (adapter, mut child) = spawn_adapter(
            config,
            cwd.as_std_path(),
            use_local_claude,
            require_local_claude_executable,
        )?;
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
            signature: AdapterConfigSignature::new(
                config,
                use_local_claude,
                require_local_claude_executable,
            ),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            next_id: Mutex::new(1),
            pending: Mutex::new(HashMap::new()),
            session_routes: Mutex::new(HashMap::new()),
            early_session_frames: Mutex::new(EarlySessionFrames::default()),
            unrouted_warnings: Mutex::new(HashMap::new()),
            initialized_capabilities: Mutex::new(None),
            active_prompts: ActivePromptTracker::default(),
            generation: NEXT_CONNECTION_GENERATION.fetch_add(1, Ordering::Relaxed),
            last_activity_at: Mutex::new(Instant::now()),
            session_config_transaction: SessionConfigTransaction::default(),
            state: Mutex::new(AdapterConnectionState::Open),
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

    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn touch(&self) {
        if let Ok(mut last_activity_at) = self.last_activity_at.lock() {
            *last_activity_at = Instant::now();
        }
    }

    fn last_activity_at(&self) -> Instant {
        self.last_activity_at
            .lock()
            .map(|value| *value)
            .unwrap_or_else(|_| Instant::now())
    }

    pub(crate) fn lock_session_config_transaction(&self) -> Result<MutexGuard<'_, ()>> {
        self.session_config_transaction.lock()
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
        self.begin_request_with_policy(method, params, false)
    }

    fn begin_shutdown_request(&self, method: &str, params: Value) -> Result<PendingRequest> {
        self.begin_request_with_policy(method, params, true)
    }

    fn begin_request_with_policy(
        &self,
        method: &str,
        params: Value,
        allow_draining: bool,
    ) -> Result<PendingRequest> {
        self.touch();
        self.ensure_request_allowed(allow_draining)?;
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

    fn ensure_request_allowed(&self, allow_draining: bool) -> Result<()> {
        let state = *self
            .state
            .lock()
            .map_err(|_| anyhow!("ACP adapter connection state lock poisoned"))?;
        match request_unavailability(state, allow_draining) {
            Some(error) => Err(anyhow!(error)),
            None => Ok(()),
        }
    }

    pub fn cancel_pending(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    pub fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        self.touch();
        self.send_raw_frame(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    pub fn send_response(&self, id: Value, result: Value) -> Result<()> {
        self.touch();
        self.send_raw_frame(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    pub fn send_raw_frame(&self, frame: &Value) -> Result<()> {
        if self.is_transport_closed() {
            return Err(anyhow!(AcpConnectionUnavailable::Closed));
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
            return Err(anyhow!(AcpConnectionUnavailable::Closed)
                .context(format!("failed to write ACP adapter frame: {error}")));
        }
        Ok(())
    }

    pub fn register_session_route(&self, session_id: &str) -> SessionRouteReceiver {
        register_session_route_state(
            &self.adapter.adapter_id,
            session_id,
            &self.session_routes,
            &self.early_session_frames,
        )
    }

    pub fn register_session_event_pump(&self, session_id: &str) -> Arc<SessionEventPump> {
        SessionEventPump::start(self.register_session_route(session_id))
    }

    pub fn unregister_session_route(&self, session_id: &str) {
        let route = if let Ok(mut routes) = self.session_routes.lock() {
            let route = routes.remove(session_id);
            if let Ok(mut early_frames) = self.early_session_frames.lock() {
                early_frames.remove(session_id);
            }
            route
        } else {
            None
        };
        if let Some(route) = route {
            route.close();
        }
    }

    pub fn begin_prompt(self: &Arc<Self>, session_id: &str) -> Result<ActivePromptGuard> {
        let state = *self
            .state
            .lock()
            .map_err(|_| anyhow!("ACP adapter connection state lock poisoned"))?;
        match request_unavailability(state, false) {
            None => {
                self.active_prompts.mark_active(session_id);
                Ok(ActivePromptGuard {
                    connection: Arc::clone(self),
                    session_id: session_id.to_string(),
                })
            }
            Some(error) => Err(anyhow!(error)),
        }
    }

    pub fn active_prompt_count(&self) -> usize {
        self.active_prompts.count()
    }

    pub fn active_prompt_count_for_session(&self, session_id: &str) -> usize {
        self.active_prompts.count_for_session(session_id)
    }

    pub fn wait_for_prompt_drain(&self, session_ids: &[String], timeout: Duration) -> Result<bool> {
        self.active_prompts.wait_for_sessions(session_ids, timeout)
    }

    fn begin_draining(&self) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("ACP adapter connection state lock poisoned"))?;
        match *state {
            AdapterConnectionState::Open => {
                *state = AdapterConnectionState::Draining;
                Ok(true)
            }
            AdapterConnectionState::Draining => Ok(true),
            AdapterConnectionState::Closed => Ok(false),
        }
    }

    pub fn is_transport_closed(&self) -> bool {
        self.state
            .lock()
            .map(|state| *state == AdapterConnectionState::Closed)
            .unwrap_or(true)
    }

    fn mark_transport_closed(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = AdapterConnectionState::Closed;
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        if let Ok(mut routes) = self.session_routes.lock() {
            for route in routes.drain().map(|(_, route)| route) {
                route.close();
            }
            if let Ok(mut early_frames) = self.early_session_frames.lock() {
                *early_frames = EarlySessionFrames::default();
            }
        }
    }

    fn warn_unrouted_frame(&self, value: &Value, frame_bytes: usize) {
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let session_id = session_id_from_frame(value).unwrap_or("unknown");
        let session_update = value
            .pointer("/params/update/sessionUpdate")
            .or_else(|| value.pointer("/params/sessionUpdate"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let warning_key = format!("{method}:{session_update}");
        let now = Instant::now();
        let suppressed = match self.unrouted_warnings.lock() {
            Ok(mut warnings) => {
                let Some(suppressed) = record_unrouted_warning(&mut warnings, warning_key, now)
                else {
                    return;
                };
                suppressed
            }
            Err(_) => 0,
        };
        warn!(
            adapter = %self.adapter.adapter_id,
            session_id,
            method,
            session_update,
            frame_bytes,
            suppressed,
            "ACP inbound frame had no registered session route"
        );
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
        let request = self.begin_shutdown_request(
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
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) if line.iter().all(u8::is_ascii_whitespace) => {}
            Ok(frame_bytes) => match serde_json::from_slice::<Value>(&line) {
                Ok(value) => route_inbound_frame(&connection, value, frame_bytes),
                Err(error) => warn!(%error, frame_bytes, "invalid ACP stdout frame"),
            },
            Err(error) => {
                warn!(%error, "failed reading ACP stdout");
                break;
            }
        }
    }
    connection.mark_transport_closed();
}

fn route_inbound_frame(connection: &AdapterConnection, value: Value, frame_bytes: usize) {
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
        if route_or_buffer_session_frame(
            &connection.session_routes,
            &connection.early_session_frames,
            session_id,
            value.clone(),
            frame_bytes,
            Instant::now(),
        ) {
            return;
        }
    }

    connection.warn_unrouted_frame(&value, frame_bytes);
}

fn register_session_route_state(
    adapter_id: &str,
    session_id: &str,
    session_routes: &Mutex<HashMap<String, SessionRouteSender>>,
    early_session_frames: &Mutex<EarlySessionFrames>,
) -> SessionRouteReceiver {
    let (tx, rx) = session_route_pair(adapter_id.to_string(), session_id.to_string());
    let (previous, buffered) = match session_routes.lock() {
        Ok(mut routes) => {
            let buffered = early_session_frames
                .lock()
                .map(|mut frames| frames.take(session_id, Instant::now()))
                .unwrap_or_default();
            (routes.insert(session_id.to_string(), tx.clone()), buffered)
        }
        Err(_) => {
            tx.close();
            return rx;
        }
    };
    if let Some(previous) = previous {
        previous.close();
    }
    for frame in buffered {
        if !tx.send(frame.value, frame.bytes) {
            break;
        }
    }
    rx
}

fn route_or_buffer_session_frame(
    session_routes: &Mutex<HashMap<String, SessionRouteSender>>,
    early_session_frames: &Mutex<EarlySessionFrames>,
    session_id: &str,
    value: Value,
    frame_bytes: usize,
    now: Instant,
) -> bool {
    let Ok(routes) = session_routes.lock() else {
        return false;
    };
    if let Some(route) = routes.get(session_id).cloned() {
        drop(routes);
        return route.send(value, frame_bytes);
    }
    let buffered = early_session_frames
        .lock()
        .map(|mut frames| frames.push(session_id, value, frame_bytes, now))
        .unwrap_or(false);
    drop(routes);
    buffered
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
        require_local_claude_executable: bool,
    ) -> Result<Arc<AdapterConnection>> {
        Ok(self
            .get_or_spawn_with_outcome(
                provider_id,
                config,
                workspace_root,
                use_local_claude,
                require_local_claude_executable,
            )?
            .connection)
    }

    pub fn get_or_spawn_with_outcome(
        &self,
        provider_id: &str,
        config: &AcpAdapterConfig,
        workspace_root: Utf8PathBuf,
        use_local_claude: bool,
        require_local_claude_executable: bool,
    ) -> Result<AdapterConnectionResolution> {
        let key = AdapterConnectionKey::new(provider_id, workspace_root);
        let signature =
            AdapterConfigSignature::new(config, use_local_claude, require_local_claude_executable);
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
            require_local_claude_executable,
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

    pub fn prune_idle_connections(&self, idle_ttl: Duration, max_idle: usize) {
        let attached_keys = self
            .attempt_sessions
            .lock()
            .map(|attempts| {
                attempts
                    .values()
                    .map(|session| session.key.clone())
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        let now = Instant::now();
        let mut removed = Vec::new();
        if let Ok(mut connections) = self.connections.lock() {
            let mut idle = connections
                .iter()
                .filter(|(key, connection)| {
                    !attached_keys.contains(*key) && connection.active_prompt_count() == 0
                })
                .map(|(key, connection)| (key.clone(), connection.last_activity_at()))
                .collect::<Vec<_>>();
            idle.sort_by_key(|(_, last_activity_at)| *last_activity_at);
            let overflow = idle.len().saturating_sub(max_idle);
            for (index, (key, last_activity_at)) in idle.into_iter().enumerate() {
                if now.duration_since(last_activity_at) >= idle_ttl || index < overflow {
                    if let Some(connection) = connections.remove(&key) {
                        removed.push(connection);
                    }
                }
            }
        }
        for connection in removed {
            connection.shutdown();
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
        settle_attempt_for_session_close(attempt_dir);
        if connection.active_prompt_count_for_session(&session.session_id) > 0 {
            if let Err(error) = connection.send_cancel_notification(&session.session_id) {
                warn!(%attempt_dir, %error, "failed to cancel ACP prompt before session close");
            }
            let drained = connection
                .wait_for_prompt_drain(std::slice::from_ref(&session.session_id), timeout)?;
            if !drained {
                warn!(
                    %attempt_dir,
                    session_id = %session.session_id,
                    active_prompts = connection.active_prompt_count_for_session(&session.session_id),
                    "ACP prompt drain timed out before session close"
                );
            }
        }
        connection.close_session_bounded(&session.session_id, timeout)?;
        persist_cancelled_session_snapshot(attempt_dir);
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

    pub fn close_provider_connections_bounded(
        &self,
        provider_id: &str,
        timeout: Duration,
    ) -> Result<()> {
        let connections = self
            .connections
            .lock()
            .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?;
        let keys = select_provider_connection_keys(connections.keys(), provider_id);
        drop(connections);
        for key in keys {
            self.close_connection_bounded(&key, timeout)?;
        }
        Ok(())
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
        let connection = {
            let mut connections = self
                .connections
                .lock()
                .map_err(|_| anyhow!("ACP connection manager lock poisoned"))?;
            let Some(connection) = connections.get(key).cloned() else {
                return Ok(());
            };
            connection.begin_draining()?;
            connections.remove(key);
            connection
        };
        let sessions = self
            .attempt_sessions
            .lock()
            .map_err(|_| anyhow!("ACP attempt session lock poisoned"))?
            .iter()
            .filter(|(_, session)| &session.key == key)
            .map(|(attempt_dir, session)| (attempt_dir.clone(), session.session_id.clone()))
            .collect::<Vec<_>>();
        let session_ids = sessions
            .iter()
            .map(|(_, session_id)| session_id.clone())
            .collect::<Vec<_>>();
        let mut closed_attempts = Vec::new();
        let mut close_errors = Vec::new();
        for (attempt_dir, session_id) in &sessions {
            let attempt_path = Utf8PathBuf::from(&attempt_dir);
            settle_attempt_for_session_close(attempt_path.as_path());
            if connection.active_prompt_count_for_session(session_id) > 0
                && let Err(error) = connection.send_cancel_notification(session_id)
            {
                warn!(%attempt_dir, %session_id, %error, "failed to cancel ACP prompt while draining connection");
            }
        }
        if !connection.wait_for_prompt_drain(&session_ids, timeout)? {
            warn!(
                provider = %key.provider_id,
                workspace = %key.workspace_root,
                active_prompts = connection.active_prompt_count(),
                "ACP prompt drain timed out before adapter shutdown"
            );
        }
        for (attempt_dir, session_id) in sessions {
            let attempt_path = Utf8PathBuf::from(&attempt_dir);
            let raw_path = attempt_path.join("acp.raw.jsonl");
            if let Err(error) = connection.close_session_bounded_with_raw_log(
                &session_id,
                timeout,
                Some(raw_path.as_path()),
            ) {
                close_errors.push(format!("{attempt_dir}: {error}"));
            }
            persist_cancelled_session_snapshot(attempt_path.as_path());
            closed_attempts.push(attempt_dir);
        }
        if let Ok(mut attempts) = self.attempt_sessions.lock() {
            for attempt_dir in closed_attempts {
                attempts.remove(&attempt_dir);
            }
        }
        connection.shutdown();
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

    pub fn has_active_prompts_in_provider(&self, provider_id: &str) -> bool {
        self.connections
            .lock()
            .map(|connections| {
                connections.iter().any(|(key, connection)| {
                    key.provider_id == provider_id && connection.active_prompt_count() > 0
                })
            })
            .unwrap_or(false)
    }
}

fn select_provider_connection_keys<'a>(
    keys: impl Iterator<Item = &'a AdapterConnectionKey>,
    provider_id: &str,
) -> Vec<AdapterConnectionKey> {
    keys.filter(|key| key.provider_id == provider_id)
        .cloned()
        .collect()
}

fn settle_attempt_for_session_close(attempt_dir: &Utf8Path) {
    let decided_at = current_timestamp();
    if let Err(error) = cancel_pending_permission_requests(attempt_dir, decided_at.clone()) {
        warn!(%attempt_dir, %error, "failed to cancel pending ACP permission requests before session close");
    }
    if let Err(error) = cancel_pending_elicitation_requests(attempt_dir, decided_at) {
        warn!(%attempt_dir, %error, "failed to cancel pending ACP elicitation requests before session close");
    }
}

fn persist_cancelled_session_snapshot(attempt_dir: &Utf8Path) {
    for file_name in ["acp.snapshot.json", "acp.session.json"] {
        let path = attempt_dir.join(file_name);
        if let Err(error) = persist_cancelled_session_file(&path) {
            warn!(%path, %error, "failed to persist cancelled ACP session metadata after session close");
        }
    }
}

fn persist_cancelled_session_file(path: &Utf8Path) -> Result<()> {
    let mut session = if path.exists() {
        read_json::<Value>(path)?
    } else {
        let session_id = path
            .parent()
            .and_then(|attempt_dir| attempt_dir.file_name())
            .unwrap_or("session");
        json!({
            "sessionId": session_id,
            "status": "cancelled",
            "restored": false,
            "createdAt": current_timestamp(),
        })
    };
    let now = current_timestamp();
    session["status"] = json!("cancelled");
    session["stopReason"] = json!("cancelled");
    session["updatedAt"] = json!(now.clone());
    if session.get("updated_at").is_some() {
        session["updated_at"] = json!(now);
    }
    ensure_parent_dir(path)?;
    write_json(path, &session)
}

static ADAPTER_CONNECTION_MANAGER: LazyLock<AdapterConnectionManager> =
    LazyLock::new(AdapterConnectionManager::default);

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    use super::{
        AcpConnectionUnavailable, ActivePromptTracker, AdapterConnectionKey,
        AdapterConnectionState, EarlySessionFrames, SessionConfigTransaction, SessionEventPump,
        SessionRouteTryRecvError, persist_cancelled_session_snapshot, record_unrouted_warning,
        register_session_route_state, request_unavailability, route_or_buffer_session_frame,
        select_provider_connection_keys, session_id_from_frame, session_route_pair,
        settle_attempt_for_session_close,
    };

    #[test]
    fn session_config_transactions_do_not_overlap() {
        let transaction = Arc::new(SessionConfigTransaction::default());
        let first_guard = transaction.lock().unwrap();
        let waiting_transaction = Arc::clone(&transaction);
        let (entered_tx, entered_rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let _guard = waiting_transaction.lock().unwrap();
            entered_tx.send(()).unwrap();
        });

        assert!(entered_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(first_guard);
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        waiter.join().unwrap();
    }
    use crate::{
        acp::{
            elicitation::{
                ELICITATION_DEFAULT_TIMEOUT, ElicitationAction, PendingElicitationState,
                wait_for_elicitation_response, write_pending_elicitation,
            },
            events::{load_timeline_items, permission_request_event, write_timeline_items},
            permission::{pending_permission_file, write_pending_permission},
        },
        storage::{read_json, write_json},
    };

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
    fn provider_connection_selection_spans_workspaces() {
        let keys = [
            AdapterConnectionKey::new("claude-acp", Utf8PathBuf::from("/repo-a")),
            AdapterConnectionKey::new("claude-acp", Utf8PathBuf::from("/repo-b")),
            AdapterConnectionKey::new("codex-acp", Utf8PathBuf::from("/repo-a")),
        ];

        let selected = select_provider_connection_keys(keys.iter(), "claude-acp");

        assert_eq!(selected, vec![keys[0].clone(), keys[1].clone()]);
    }

    #[test]
    fn draining_rejects_new_requests_but_allows_shutdown_requests() {
        assert_eq!(
            request_unavailability(AdapterConnectionState::Open, false),
            None
        );
        assert_eq!(
            request_unavailability(AdapterConnectionState::Draining, false),
            Some(AcpConnectionUnavailable::Draining)
        );
        assert_eq!(
            request_unavailability(AdapterConnectionState::Draining, true),
            None
        );
        assert_eq!(
            request_unavailability(AdapterConnectionState::Closed, true),
            Some(AcpConnectionUnavailable::Closed)
        );
    }

    #[test]
    fn ask_user_question_close_drains_prompt_before_transport_shutdown() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let session_id = "session-ask".to_string();
        let elicitation_id = "elicit-close";
        write_pending_elicitation(
            &attempt_dir,
            &PendingElicitationState {
                elicitation_id: elicitation_id.to_string(),
                jsonrpc_id: json!(1),
                message: "Choose".to_string(),
                requested_schema: json!({ "type": "object", "properties": {} }),
                created_at: "1Z".to_string(),
            },
        )
        .unwrap();

        let prompts = Arc::new(ActivePromptTracker::default());
        prompts.mark_active(&session_id);
        let worker_prompts = Arc::clone(&prompts);
        let worker_attempt_dir = attempt_dir.clone();
        let worker_session_id = session_id.clone();
        let worker = thread::spawn(move || {
            let response = wait_for_elicitation_response(
                &worker_attempt_dir,
                elicitation_id,
                ELICITATION_DEFAULT_TIMEOUT,
            )
            .unwrap();
            worker_prompts.mark_inactive(&worker_session_id);
            response
        });

        settle_attempt_for_session_close(&attempt_dir);
        assert!(
            prompts
                .wait_for_sessions(std::slice::from_ref(&session_id), Duration::from_secs(1))
                .unwrap()
        );
        let response = worker.join().unwrap();
        assert!(matches!(response.action, ElicitationAction::Decline));
        assert_eq!(prompts.count(), 0);
    }

    #[test]
    fn prompt_drain_is_bounded_when_worker_does_not_finish() {
        let prompts = ActivePromptTracker::default();
        let session_id = "session-stuck".to_string();
        prompts.mark_active(&session_id);

        assert!(
            !prompts
                .wait_for_sessions(std::slice::from_ref(&session_id), Duration::from_millis(20))
                .unwrap()
        );
        assert_eq!(prompts.count_for_session(&session_id), 1);
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

    #[test]
    fn session_frame_arriving_before_route_registration_is_delivered() {
        let routes = Mutex::new(HashMap::new());
        let early_frames = Mutex::new(EarlySessionFrames::default());
        let frame = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "session-early",
                "update": {
                    "sessionUpdate": "available_commands_update",
                    "availableCommands": [{ "name": "review" }]
                }
            }
        });

        assert!(route_or_buffer_session_frame(
            &routes,
            &early_frames,
            "session-early",
            frame.clone(),
            128,
            std::time::Instant::now(),
        ));
        let receiver =
            register_session_route_state("test-adapter", "session-early", &routes, &early_frames);

        assert_eq!(receiver.try_recv().unwrap(), frame);
        assert_eq!(receiver.try_recv(), Err(SessionRouteTryRecvError::Empty));
    }

    #[test]
    fn session_event_pump_drains_route_while_runtime_is_idle() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-pump");
        let pump = SessionEventPump::start(receiver);
        for index in 0..32 {
            assert!(sender.send(json!({ "index": index }), 32));
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut observed = Vec::new();
        while observed.len() < 32 && std::time::Instant::now() < deadline {
            match pump.try_recv() {
                Ok(value) => observed.push(value["index"].as_u64().unwrap()),
                Err(SessionRouteTryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(SessionRouteTryRecvError::Disconnected) => break,
            }
        }
        assert_eq!(observed, (0..32).collect::<Vec<_>>());
        pump.close();
    }

    #[test]
    fn unrouted_warning_rate_limit_summarizes_repeated_frames() {
        let mut warnings = std::collections::HashMap::new();
        let started = std::time::Instant::now();

        assert_eq!(
            record_unrouted_warning(
                &mut warnings,
                "session/update:agent_message_chunk".to_string(),
                started,
            ),
            Some(0)
        );
        for _ in 0..9_999 {
            assert_eq!(
                record_unrouted_warning(
                    &mut warnings,
                    "session/update:agent_message_chunk".to_string(),
                    started + Duration::from_secs(1),
                ),
                None
            );
        }
        assert_eq!(
            record_unrouted_warning(
                &mut warnings,
                "session/update:agent_message_chunk".to_string(),
                started + Duration::from_secs(60),
            ),
            Some(9_999)
        );
    }

    #[test]
    fn session_route_preserves_order_under_sustained_load() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        let producer = thread::spawn(move || {
            for index in 0..10_000_u64 {
                let value = json!({
                    "index": index,
                    "payload": format!("frame-{index:05}-abcdefghijklmnopqrstuvwxyz")
                });
                let frame_bytes = serde_json::to_vec(&value).unwrap().len();
                assert!(sender.send(value, frame_bytes));
            }
        });

        for expected in 0..10_000_u64 {
            loop {
                match receiver.try_recv() {
                    Ok(value) => {
                        let expected_value = json!({
                            "index": expected,
                            "payload": format!("frame-{expected:05}-abcdefghijklmnopqrstuvwxyz")
                        });
                        assert_eq!(
                            serde_json::to_vec(&value).unwrap(),
                            serde_json::to_vec(&expected_value).unwrap()
                        );
                        break;
                    }
                    Err(SessionRouteTryRecvError::Empty) => thread::yield_now(),
                    Err(SessionRouteTryRecvError::Disconnected) => {
                        panic!("session route disconnected before all frames were received")
                    }
                }
            }
        }
        producer.join().unwrap();
    }

    #[test]
    fn session_route_blocks_at_frame_limit_and_resumes_after_receive() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        for index in 0..256_u64 {
            assert!(sender.send(json!({ "index": index }), 1));
        }
        let (done_tx, done_rx) = mpsc::channel();
        let producer = thread::spawn(move || {
            let sent = sender.send(json!({ "index": 256 }), 1);
            done_tx.send(sent).unwrap();
        });

        assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(receiver.try_recv().is_ok());
        assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap(), true);
        producer.join().unwrap();
    }

    #[test]
    fn session_route_blocks_at_byte_limit_and_resumes_after_receive() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        assert!(sender.send(json!({ "index": 0 }), 4 * 1024 * 1024));
        let (done_tx, done_rx) = mpsc::channel();
        let producer = thread::spawn(move || {
            done_tx.send(sender.send(json!({ "index": 1 }), 1)).unwrap();
        });

        assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(receiver.try_recv().is_ok());
        assert!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        producer.join().unwrap();
    }

    #[test]
    fn session_route_allows_one_oversized_frame_when_empty() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        assert!(sender.send(json!({ "large": true }), 8 * 1024 * 1024));
        assert_eq!(receiver.try_recv().unwrap()["large"], json!(true));
    }

    #[test]
    fn dropping_receiver_unblocks_waiting_sender() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        for index in 0..256_u64 {
            assert!(sender.send(json!({ "index": index }), 1));
        }
        let (done_tx, done_rx) = mpsc::channel();
        let producer = thread::spawn(move || {
            done_tx
                .send(sender.send(json!({ "afterDrop": true }), 1))
                .unwrap();
        });

        drop(receiver);

        assert!(!done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        producer.join().unwrap();
    }

    #[test]
    fn closing_route_unblocks_waiting_sender_and_allows_receiver_to_drain() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        for index in 0..256_u64 {
            assert!(sender.send(json!({ "index": index }), 1));
        }
        let closing_sender = sender.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let producer = thread::spawn(move || {
            done_tx
                .send(sender.send(json!({ "afterClose": true }), 1))
                .unwrap();
        });

        closing_sender.close();

        assert!(!done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        for expected in 0..256_u64 {
            assert_eq!(
                receiver.try_recv().unwrap()["index"].as_u64(),
                Some(expected)
            );
        }
        assert_eq!(
            receiver.try_recv(),
            Err(SessionRouteTryRecvError::Disconnected)
        );
        producer.join().unwrap();
    }

    #[test]
    fn session_close_settles_pending_permission_and_snapshot() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let request_id = "close";
        write_pending_permission(
            &attempt_dir,
            request_id,
            json!({
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": "tool-1",
                    "title": "Read file"
                },
                "options": [{ "optionId": "allow", "name": "Allow" }]
            }),
            "1Z".to_string(),
        )
        .unwrap();
        let mut pending = permission_request_event(
            1,
            request_id.to_string(),
            json!({
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": "tool-1",
                    "title": "Read file"
                },
                "options": [{ "optionId": "allow", "name": "Allow" }]
            }),
        );
        pending.id = format!("permission-{request_id}");
        write_timeline_items(&attempt_dir.join("acp.timeline.jsonl"), &[pending]).unwrap();
        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &json!({
                "sessionId": "session-1",
                "status": "running",
                "stopReason": null,
                "createdAt": "1Z"
            }),
        )
        .unwrap();

        settle_attempt_for_session_close(&attempt_dir);
        persist_cancelled_session_snapshot(&attempt_dir);

        assert!(!pending_permission_file(&attempt_dir, request_id).exists());
        let items = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl")).unwrap();
        let permission = items
            .iter()
            .find(|item| item.id == "permission-close")
            .unwrap();
        assert_eq!(permission.status.as_deref(), Some("cancelled"));
        let snapshot: serde_json::Value =
            read_json(&attempt_dir.join("acp.snapshot.json")).unwrap();
        assert_eq!(
            snapshot.get("status").and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert_eq!(
            snapshot.get("stopReason").and_then(|value| value.as_str()),
            Some("cancelled")
        );
    }
}
