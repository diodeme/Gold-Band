use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::ChildStdin;
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
use crate::acp::events::{
    AcpLatestTurnStatus, append_raw_frame, current_timestamp, persist_session_terminal,
};
use crate::acp::permission::cancel_pending_permission_requests;
use crate::config::AcpAdapterConfig;
use crate::process::{ManagedProcessGroup, PROCESS_GROUP_TERMINATION_GRACE};

const CLOSE_RAW_MAX_SIZE: u64 = 5 * 1024 * 1024;
const CLOSE_RAW_TARGET_SIZE: u64 = 4 * 1024 * 1024;
const SESSION_ROUTE_MAX_BYTES: usize = 4 * 1024 * 1024;
const SESSION_ROUTE_MAX_FRAMES: usize = 256;
const SESSION_ROUTE_INGRESS_HARD_MAX_BYTES: usize = 64 * 1024 * 1024;
const SESSION_ROUTE_INGRESS_HARD_MAX_FRAMES: usize = 16_384;
const UNROUTED_WARNING_INTERVAL: Duration = Duration::from_secs(60);
const EARLY_SESSION_FRAME_TTL: Duration = Duration::from_secs(5);
const EARLY_SESSION_FRAME_MAX_BYTES: usize = 1024 * 1024;
const EARLY_SESSION_FRAME_MAX_FRAMES: usize = 64;
const STDERR_READ_BUFFER_SIZE: usize = 4096;
const STDERR_LINE_MAX_BYTES: usize = 16 * 1024;
const STDERR_RAW_PREVIEW_BYTES: usize = 256;
static NEXT_CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_SESSION_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRouteWatermark {
    route_generation: u64,
    sequence: u64,
    closed: bool,
}

impl SessionRouteWatermark {
    pub fn route_generation(self) -> u64 {
        self.route_generation
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }

    pub fn is_closed(self) -> bool {
        self.closed
    }
}

#[derive(Debug)]
struct SessionRouteFrame {
    value: Value,
    bytes: usize,
    sequence: u64,
}

#[derive(Debug)]
struct EarlySessionFrame {
    value: Value,
    bytes: usize,
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
                value,
                bytes,
                received_at: now,
            });
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.total_frames = self.total_frames.saturating_add(1);
        true
    }

    fn take(&mut self, session_id: &str, now: Instant) -> Vec<(Value, usize)> {
        self.purge_expired(now);
        let Some(frames) = self.by_session.remove(session_id) else {
            return Vec::new();
        };
        let mut drained = Vec::with_capacity(frames.len());
        for frame in frames {
            self.total_bytes = self.total_bytes.saturating_sub(frame.bytes);
            self.total_frames = self.total_frames.saturating_sub(1);
            drained.push((frame.value, frame.bytes));
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
                        self.total_bytes = self.total_bytes.saturating_sub(expired.bytes);
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
                self.total_bytes = self.total_bytes.saturating_sub(frame.bytes);
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
    last_enqueued_sequence: u64,
    high_water_bytes: usize,
    high_water_frames: usize,
    closed: bool,
    receiver_alive: bool,
}

#[derive(Debug)]
struct SessionRouteInner {
    generation: u64,
    state: Mutex<SessionRouteState>,
    not_full: Condvar,
    not_empty: Condvar,
}

impl SessionRouteInner {
    fn new() -> Self {
        Self {
            generation: NEXT_SESSION_ROUTE_GENERATION.fetch_add(1, Ordering::Relaxed),
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
        let Ok(mut state) = self.inner.state.lock() else {
            return false;
        };
        if state.closed || !state.receiver_alive {
            return false;
        }
        let exceeds_hard_limit = !state.queue.is_empty()
            && (state.queue.len() >= SESSION_ROUTE_INGRESS_HARD_MAX_FRAMES
                || state.queued_bytes.saturating_add(bytes) > SESSION_ROUTE_INGRESS_HARD_MAX_BYTES);
        if exceeds_hard_limit {
            state.closed = true;
            let queued_bytes = state.queued_bytes;
            let queued_frames = state.queue.len();
            drop(state);
            self.inner.not_empty.notify_all();
            warn!(
                adapter = %self.adapter_id,
                session_id = %self.session_id,
                queued_bytes,
                queued_frames,
                "ACP session route exceeded isolated ingress limit"
            );
            return false;
        }
        state.last_enqueued_sequence = state.last_enqueued_sequence.saturating_add(1);
        let sequence = state.last_enqueued_sequence;
        state.queued_bytes = state.queued_bytes.saturating_add(bytes);
        state.queue.push_back(SessionRouteFrame {
            value,
            bytes,
            sequence,
        });
        state.high_water_bytes = state.high_water_bytes.max(state.queued_bytes);
        state.high_water_frames = state.high_water_frames.max(state.queue.len());
        drop(state);
        self.inner.not_empty.notify_one();
        true
    }

    fn watermark(&self) -> Option<SessionRouteWatermark> {
        let state = self.inner.state.lock().ok()?;
        Some(SessionRouteWatermark {
            route_generation: self.inner.generation,
            sequence: state.last_enqueued_sequence,
            closed: state.closed || !state.receiver_alive,
        })
    }

    fn close(&self) {
        self.inner.close();
    }
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

    fn recv_frame_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<SessionRouteFrame, mpsc::RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let Ok(mut state) = self.inner.state.lock() else {
            return Err(mpsc::RecvTimeoutError::Disconnected);
        };
        loop {
            if let Some(frame) = state.queue.pop_front() {
                state.queued_bytes = state.queued_bytes.saturating_sub(frame.bytes);
                drop(state);
                self.inner.not_full.notify_all();
                return Ok(frame);
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
    last_consumed_sequence: u64,
    closed: bool,
}

#[derive(Debug)]
struct SessionEventPumpInner {
    state: Mutex<SessionEventPumpState>,
    not_empty: Condvar,
    not_full: Condvar,
}

pub struct SessionEventPump {
    inner: Arc<SessionEventPumpInner>,
    shutdown: Arc<AtomicBool>,
    route_generation: u64,
}

impl SessionEventPump {
    fn start(receiver: SessionRouteReceiver) -> Arc<Self> {
        let inner = Arc::new(SessionEventPumpInner {
            state: Mutex::new(SessionEventPumpState::default()),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        });
        let shutdown = Arc::new(AtomicBool::new(false));
        let route_generation = receiver.inner.generation;
        let pump = Arc::new(Self {
            inner: Arc::clone(&inner),
            shutdown: Arc::clone(&shutdown),
            route_generation,
        });
        thread::spawn(move || {
            while !shutdown.load(Ordering::Acquire) {
                match receiver.recv_frame_timeout(Duration::from_millis(100)) {
                    Ok(frame) => {
                        let bytes = frame.bytes;
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
                        state.queue.push_back(frame);
                        drop(state);
                        inner.not_empty.notify_one();
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            if let Ok(mut state) = inner.state.lock() {
                state.closed = true;
            }
            inner.not_empty.notify_all();
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
            state.last_consumed_sequence = frame.sequence;
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

    pub fn recv_timeout(
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
                state.last_consumed_sequence = frame.sequence;
                drop(state);
                self.inner.not_full.notify_all();
                return Ok(frame.value);
            }
            if state.closed {
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

    pub fn has_consumed(&self, watermark: SessionRouteWatermark) -> bool {
        if self.route_generation != watermark.route_generation {
            return false;
        }
        self.inner
            .state
            .lock()
            .map(|state| state.last_consumed_sequence >= watermark.sequence)
            .unwrap_or(false)
    }

    pub fn route_generation(&self) -> u64 {
        self.route_generation
    }

    pub fn close(&self) {
        self.shutdown.store(true, Ordering::Release);
        if let Ok(mut state) = self.inner.state.lock() {
            state.closed = true;
        }
        self.inner.not_empty.notify_all();
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

fn is_same_connection_generation<T>(
    current: &Arc<T>,
    expected: &Arc<T>,
    current_generation: u64,
    expected_generation: u64,
) -> bool {
    Arc::ptr_eq(current, expected) && current_generation == expected_generation
}

#[derive(Debug, Clone)]
pub struct AdapterInitializationOutcome {
    pub capabilities: Value,
    pub performed: bool,
}

#[derive(Debug)]
pub struct PendingRequest {
    pub id: u64,
    pub frame: Value,
    rx: mpsc::Receiver<PendingRequestResponse>,
}

#[derive(Debug)]
pub struct PendingRequestResponse {
    pub frame: Value,
    pub session_route_watermark: Option<SessionRouteWatermark>,
}

impl PendingRequest {
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Value, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout).map(|response| response.frame)
    }

    pub fn recv_timeout_with_session_route_watermark(
        &self,
        timeout: Duration,
    ) -> std::result::Result<PendingRequestResponse, mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

struct PendingRequestSender {
    tx: mpsc::Sender<PendingRequestResponse>,
    route_session_id: Option<String>,
}

pub struct AdapterConnection {
    key: Option<AdapterConnectionKey>,
    provider_id: String,
    adapter: ResolvedAcpAdapter,
    signature: AdapterConfigSignature,
    child: Mutex<ManagedProcessGroup>,
    stdin: Mutex<ChildStdin>,
    next_id: Mutex<u64>,
    pending: Mutex<HashMap<u64, PendingRequestSender>>,
    session_routes: Mutex<HashMap<String, SessionRouteSender>>,
    early_session_frames: Mutex<EarlySessionFrames>,
    unrouted_warnings: Mutex<HashMap<String, UnroutedWarningState>>,
    initialization: ConnectionInitialization,
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

#[derive(Debug, Default)]
struct ConnectionInitialization {
    state: Mutex<ConnectionInitializationState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
enum ConnectionInitializationState {
    #[default]
    Uninitialized,
    Initializing,
    Initialized(Value),
    Failed,
}

struct ConnectionInitializationAttempt<'a> {
    initialization: &'a ConnectionInitialization,
    settled: bool,
}

impl Drop for ConnectionInitializationAttempt<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        if let Ok(mut state) = self.initialization.state.lock() {
            if matches!(*state, ConnectionInitializationState::Initializing) {
                *state = ConnectionInitializationState::Failed;
            }
        }
        self.initialization.changed.notify_all();
    }
}

impl ConnectionInitialization {
    fn capabilities(&self) -> Option<Value> {
        self.state.lock().ok().and_then(|state| match &*state {
            ConnectionInitializationState::Initialized(capabilities) => Some(capabilities.clone()),
            ConnectionInitializationState::Uninitialized
            | ConnectionInitializationState::Initializing
            | ConnectionInitializationState::Failed => None,
        })
    }

    fn initialize_once(
        &self,
        initialize: impl FnOnce() -> Result<Value>,
    ) -> Result<AdapterInitializationOutcome> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("ACP connection initialization lock poisoned"))?;
        loop {
            match &*state {
                ConnectionInitializationState::Initialized(capabilities) => {
                    return Ok(AdapterInitializationOutcome {
                        capabilities: capabilities.clone(),
                        performed: false,
                    });
                }
                ConnectionInitializationState::Failed => {
                    bail!("ACP connection initialization previously failed");
                }
                ConnectionInitializationState::Initializing => {
                    state = self
                        .changed
                        .wait(state)
                        .map_err(|_| anyhow!("ACP connection initialization lock poisoned"))?;
                }
                ConnectionInitializationState::Uninitialized => {
                    *state = ConnectionInitializationState::Initializing;
                    break;
                }
            }
        }
        drop(state);

        let mut attempt = ConnectionInitializationAttempt {
            initialization: self,
            settled: false,
        };
        match initialize() {
            Ok(capabilities) => {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| anyhow!("ACP connection initialization lock poisoned"))?;
                *state = ConnectionInitializationState::Initialized(capabilities.clone());
                attempt.settled = true;
                drop(state);
                self.changed.notify_all();
                Ok(AdapterInitializationOutcome {
                    capabilities,
                    performed: true,
                })
            }
            Err(error) => {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| anyhow!("ACP connection initialization lock poisoned"))?;
                *state = ConnectionInitializationState::Failed;
                attempt.settled = true;
                drop(state);
                self.changed.notify_all();
                Err(error)
            }
        }
    }
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
        provider_id: &str,
        config: &AcpAdapterConfig,
        cwd: &Utf8Path,
        use_local_claude: bool,
        require_local_claude_executable: bool,
    ) -> Result<Arc<Self>> {
        Self::spawn(
            None,
            provider_id,
            config,
            cwd,
            use_local_claude,
            require_local_claude_executable,
        )
    }

    fn spawn(
        key: Option<AdapterConnectionKey>,
        provider_id: &str,
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
            .take_stdin()
            .ok_or_else(|| anyhow!("failed to capture ACP adapter stdin"))?;
        let stdout = child
            .take_stdout()
            .ok_or_else(|| anyhow!("failed to capture ACP adapter stdout"))?;
        let stderr = child
            .take_stderr()
            .ok_or_else(|| anyhow!("failed to capture ACP adapter stderr"))?;
        let provider_id = if provider_id.trim().is_empty() {
            key.as_ref()
                .map(|key| key.provider_id.clone())
                .unwrap_or_else(|| adapter.adapter_id.clone())
        } else {
            provider_id.to_string()
        };
        let connection = Arc::new(Self {
            key,
            provider_id,
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
            initialization: ConnectionInitialization::default(),
            active_prompts: ActivePromptTracker::default(),
            generation: NEXT_CONNECTION_GENERATION.fetch_add(1, Ordering::Relaxed),
            last_activity_at: Mutex::new(Instant::now()),
            session_config_transaction: SessionConfigTransaction::default(),
            state: Mutex::new(AdapterConnectionState::Open),
        });

        let stdout_connection = Arc::clone(&connection);
        thread::spawn(move || read_stdout(stdout_connection, stdout));

        let stderr_connection = Arc::clone(&connection);
        thread::spawn(move || {
            if let Err(error) = read_stderr(stderr, |line| {
                log_stderr_line(&stderr_connection, line);
            }) {
                warn!(
                    provider = %stderr_connection.provider_id,
                    adapter = %stderr_connection.adapter.adapter_id,
                    command = %stderr_connection.adapter.command,
                    %error,
                    "failed reading ACP adapter stderr"
                );
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
        self.initialization.capabilities()
    }

    pub fn initialize_once(
        &self,
        initialize: impl FnOnce() -> Result<Value>,
    ) -> Result<AdapterInitializationOutcome> {
        self.initialization.initialize_once(initialize)
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
        let route_session_id = (method == "session/prompt")
            .then(|| frame.pointer("/params/sessionId").and_then(Value::as_str))
            .flatten()
            .map(str::to_string);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| anyhow!("ACP pending request lock poisoned"))?
            .insert(
                id,
                PendingRequestSender {
                    tx,
                    route_session_id,
                },
            );
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
        if let Ok(mut child) = self.child.lock() {
            let _ = child.terminate(PROCESS_GROUP_TERMINATION_GRACE);
        }
    }
}

fn read_stdout(connection: Arc<AdapterConnection>, stdout: impl Read + Send + 'static) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) if line.iter().all(u8::is_ascii_whitespace) => {}
            Ok(frame_bytes) => match serde_json::from_slice::<Value>(&line) {
                Ok(value) => route_inbound_frame(&connection, value, frame_bytes),
                Err(error) => warn!(
                    provider = %connection.provider_id,
                    adapter = %connection.adapter.adapter_id,
                    command = %connection.adapter.command,
                    %error,
                    frame_bytes,
                    "invalid ACP stdout frame"
                ),
            },
            Err(error) => {
                warn!(
                    provider = %connection.provider_id,
                    adapter = %connection.adapter.adapter_id,
                    command = %connection.adapter.command,
                    %error,
                    "failed reading ACP stdout"
                );
                break;
            }
        }
    }
    let transport_was_already_closed = connection.is_transport_closed();
    connection.mark_transport_closed();
    log_adapter_exit(&connection, transport_was_already_closed);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StderrLine {
    text: String,
    byte_len: usize,
    encoding: &'static str,
    raw_bytes_hex: Option<String>,
    truncated: bool,
}

fn read_stderr<R, Emit>(mut reader: R, mut emit: Emit) -> std::io::Result<()>
where
    R: Read,
    Emit: FnMut(StderrLine),
{
    let mut buffer = [0_u8; STDERR_READ_BUFFER_SIZE];
    let mut line = Vec::with_capacity(STDERR_LINE_MAX_BYTES.min(1024));
    let mut byte_len = 0_usize;
    let mut truncated = false;

    let mut flush_line = |line: &mut Vec<u8>, byte_len: &mut usize, truncated: &mut bool| {
        if *byte_len == 0 {
            line.clear();
            *truncated = false;
            return;
        }
        let content = line.strip_suffix(b"\r").unwrap_or(line);
        if !content.is_empty() {
            let encoding = if std::str::from_utf8(content).is_ok() {
                "utf-8"
            } else {
                "non-utf8"
            };
            let raw_bytes_hex = (encoding == "non-utf8").then(|| {
                content
                    .iter()
                    .take(STDERR_RAW_PREVIEW_BYTES)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            });
            emit(StderrLine {
                text: String::from_utf8_lossy(content).into_owned(),
                byte_len: *byte_len,
                encoding,
                raw_bytes_hex,
                truncated: *truncated,
            });
        }
        line.clear();
        *byte_len = 0;
        *truncated = false;
    };

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            flush_line(&mut line, &mut byte_len, &mut truncated);
            return Ok(());
        }
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                flush_line(&mut line, &mut byte_len, &mut truncated);
            } else {
                byte_len = byte_len.saturating_add(1);
                if line.len() < STDERR_LINE_MAX_BYTES {
                    line.push(*byte);
                } else {
                    truncated = true;
                }
            }
        }
    }
}

fn log_stderr_line(connection: &AdapterConnection, line: StderrLine) {
    let raw_bytes_hex = line.raw_bytes_hex.as_deref().unwrap_or("");
    if line.encoding == "non-utf8" {
        debug!(
            provider = %connection.provider_id,
            adapter = %connection.adapter.adapter_id,
            command = %connection.adapter.command,
            stderr_bytes = line.byte_len,
            stderr_encoding = line.encoding,
            stderr_truncated = line.truncated,
            "ACP adapter stderr used non-UTF-8 bytes; decoded output is available when detailed logs are enabled"
        );
    }
    debug!(
        provider = %connection.provider_id,
        adapter = %connection.adapter.adapter_id,
        command = %connection.adapter.command,
        stderr = %line.text,
        stderr_bytes = line.byte_len,
        stderr_encoding = line.encoding,
        stderr_bytes_hex_prefix = raw_bytes_hex,
        stderr_truncated = line.truncated,
        "ACP adapter stderr"
    );
}

fn log_adapter_exit(connection: &AdapterConnection, transport_was_already_closed: bool) {
    match connection.try_wait() {
        Ok(Some(status)) => {
            if !transport_was_already_closed {
                warn!(
                    provider = %connection.provider_id,
                    adapter = %connection.adapter.adapter_id,
                    command = %connection.adapter.command,
                    exit_code = status.code(),
                    status = %status,
                    "ACP adapter process exited after stdout closed"
                );
            } else {
                debug!(
                    provider = %connection.provider_id,
                    adapter = %connection.adapter.adapter_id,
                    command = %connection.adapter.command,
                    exit_code = status.code(),
                    status = %status,
                    "ACP adapter process exited"
                );
            }
        }
        Ok(None) if !transport_was_already_closed => warn!(
            provider = %connection.provider_id,
            adapter = %connection.adapter.adapter_id,
            command = %connection.adapter.command,
            "ACP adapter stdout closed before process exit status was available"
        ),
        Ok(None) => {}
        Err(error) => warn!(
            provider = %connection.provider_id,
            adapter = %connection.adapter.adapter_id,
            command = %connection.adapter.command,
            %error,
            "failed to read ACP adapter exit status"
        ),
    }
}

fn route_inbound_frame(connection: &AdapterConnection, value: Value, frame_bytes: usize) {
    if value.get("method").is_none() {
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            if let Some(pending) = connection
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id))
            {
                let session_route_watermark = pending
                    .route_session_id
                    .as_deref()
                    .and_then(|session_id| connection.session_route_watermark(session_id));
                let _ = pending.tx.send(PendingRequestResponse {
                    frame: value,
                    session_route_watermark,
                });
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

impl AdapterConnection {
    fn session_route_watermark(&self, session_id: &str) -> Option<SessionRouteWatermark> {
        self.session_routes
            .lock()
            .ok()
            .and_then(|routes| routes.get(session_id).cloned())
            .and_then(|route| route.watermark())
    }
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
    for (value, bytes) in buffered {
        if !tx.send(value, bytes) {
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
struct ConnectionCreationGate {
    in_flight: Mutex<HashSet<AdapterConnectionKey>>,
    changed: Condvar,
}

struct ConnectionCreationGuard<'a> {
    gate: &'a ConnectionCreationGate,
    key: AdapterConnectionKey,
}

impl ConnectionCreationGate {
    fn enter(&self, key: &AdapterConnectionKey) -> Result<ConnectionCreationGuard<'_>> {
        let mut in_flight = self
            .in_flight
            .lock()
            .map_err(|_| anyhow!("ACP connection creation gate lock poisoned"))?;
        while in_flight.contains(key) {
            in_flight = self
                .changed
                .wait(in_flight)
                .map_err(|_| anyhow!("ACP connection creation gate lock poisoned"))?;
        }
        in_flight.insert(key.clone());
        Ok(ConnectionCreationGuard {
            gate: self,
            key: key.clone(),
        })
    }
}

impl Drop for ConnectionCreationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut in_flight) = self.gate.in_flight.lock() {
            in_flight.remove(&self.key);
        }
        self.gate.changed.notify_all();
    }
}

#[derive(Default)]
pub struct AdapterConnectionManager {
    connections: Mutex<HashMap<AdapterConnectionKey, Arc<AdapterConnection>>>,
    attempt_sessions: Mutex<HashMap<String, LiveAcpSession>>,
    creation_gate: ConnectionCreationGate,
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

        let _creation_guard = self.creation_gate.enter(&key)?;
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
            provider_id,
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

    pub fn evict_if_current(
        &self,
        key: &AdapterConnectionKey,
        expected: &Arc<AdapterConnection>,
    ) -> bool {
        let removed = self.connections.lock().ok().and_then(|mut connections| {
            let matches = connections.get(key).is_some_and(|current| {
                is_same_connection_generation(
                    current,
                    expected,
                    current.generation(),
                    expected.generation(),
                )
            });
            matches.then(|| connections.remove(key)).flatten()
        });
        if let Some(connection) = removed {
            connection.shutdown();
            true
        } else {
            false
        }
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
        let has_active_prompt = connection.active_prompt_count_for_session(&session.session_id) > 0;
        if has_active_prompt {
            if let Err(error) = connection.send_cancel_notification(&session.session_id) {
                warn!(%attempt_dir, %error, "failed to cancel ACP prompt before session close");
            }
        }
        settle_attempt_for_session_close(attempt_dir);
        if has_active_prompt {
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
            if connection.active_prompt_count_for_session(session_id) > 0
                && let Err(error) = connection.send_cancel_notification(session_id)
            {
                warn!(%attempt_dir, %session_id, %error, "failed to cancel ACP prompt while draining connection");
            }
        }
        for (attempt_dir, _) in &sessions {
            let attempt_path = Utf8PathBuf::from(attempt_dir);
            settle_attempt_for_session_close(attempt_path.as_path());
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
    let path = attempt_dir.join("acp.snapshot.json");
    if let Err(error) = persist_cancelled_session_file(&path) {
        warn!(%path, %error, "failed to persist cancelled ACP session metadata after session close");
    }
}

fn persist_cancelled_session_file(path: &Utf8Path) -> Result<()> {
    let now = current_timestamp();
    persist_session_terminal(path, AcpLatestTurnStatus::Cancelled, "cancelled", &now)?;
    Ok(())
}

static ADAPTER_CONNECTION_MANAGER: LazyLock<AdapterConnectionManager> =
    LazyLock::new(AdapterConnectionManager::default);

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    use super::{
        AcpConnectionUnavailable, ActivePromptTracker, AdapterConnectionKey,
        AdapterConnectionState, ConnectionCreationGate, ConnectionInitialization,
        EarlySessionFrames, STDERR_LINE_MAX_BYTES, SessionConfigTransaction, SessionEventPump,
        SessionRouteTryRecvError, is_same_connection_generation,
        persist_cancelled_session_snapshot, read_stderr, record_unrouted_warning,
        register_session_route_state, request_unavailability, route_or_buffer_session_frame,
        select_provider_connection_keys, session_id_from_frame, session_route_pair,
        settle_attempt_for_session_close,
    };

    fn write_current_attempt_node(attempt_dir: &Utf8PathBuf) {
        crate::storage::write_json(
            &attempt_dir.join("node.json"),
            &json!({
                "version": crate::domain::VERSION,
                "acp_storage_schema_version": crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                "node_id": "worker",
                "node_type": "worker",
                "run_id": "run-001",
                "round_id": "round-001",
                "attempt_id": "attempt-001",
                "status": "running",
                "outcome": null,
                "started_at": "1Z",
                "finished_at": null,
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();
    }

    #[test]
    fn parallel_callers_share_one_connection_initialize() {
        let initialization = Arc::new(ConnectionInitialization::default());
        let started = Arc::new(Barrier::new(3));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let initialization = Arc::clone(&initialization);
            let started = Arc::clone(&started);
            let calls = Arc::clone(&calls);
            workers.push(thread::spawn(move || {
                started.wait();
                initialization
                    .initialize_once(|| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(30));
                        Ok(json!({ "loadSession": true }))
                    })
                    .unwrap()
            }));
        }
        started.wait();
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.performed).count(),
            1
        );
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.capabilities == json!({ "loadSession": true }))
        );
    }

    #[test]
    fn stderr_reader_recovers_non_utf8_output_and_continues() {
        let mut input = b"npm error: ".to_vec();
        input.extend_from_slice(&[0x81, 0x40]);
        input.extend_from_slice(b"\r\nnext line\n");
        let mut lines = Vec::new();

        read_stderr(Cursor::new(input), |line| lines.push(line)).unwrap();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].encoding, "non-utf8");
        assert!(lines[0].text.starts_with("npm error:"));
        assert_eq!(
            lines[0].raw_bytes_hex.as_deref(),
            Some("6e706d206572726f723a208140")
        );
        assert!(!lines[0].truncated);
        assert_eq!(lines[1].text, "next line");
        assert_eq!(lines[1].encoding, "utf-8");
    }

    #[test]
    fn stderr_reader_bounds_unterminated_lines_without_losing_following_output() {
        let mut input = vec![b'x'; STDERR_LINE_MAX_BYTES + 32];
        input.extend_from_slice(b"\nfinal line\n");
        let mut lines = Vec::new();

        read_stderr(Cursor::new(input), |line| lines.push(line)).unwrap();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].byte_len, STDERR_LINE_MAX_BYTES + 32);
        assert_eq!(lines[0].text.len(), STDERR_LINE_MAX_BYTES);
        assert!(lines[0].truncated);
        assert_eq!(lines[1].text, "final line");
    }

    #[test]
    fn failed_initialize_poison_is_confined_to_the_old_connection() {
        let old_initialization = ConnectionInitialization::default();
        let first = old_initialization.initialize_once(|| anyhow::bail!("ambiguous transport"));

        assert!(first.is_err());
        let repeated = old_initialization
            .initialize_once(|| panic!("a failed connection must never initialize again"));
        assert!(repeated.is_err());

        let replacement_initialization = ConnectionInitialization::default();
        let second = replacement_initialization
            .initialize_once(|| Ok(json!({ "resumeSession": true })))
            .unwrap();
        assert!(second.performed);
        assert_eq!(second.capabilities, json!({ "resumeSession": true }));
    }

    #[test]
    fn panicking_initialize_wakes_waiters_and_poisons_the_connection() {
        let initialization = ConnectionInitialization::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = initialization.initialize_once(|| panic!("initialize panic"));
        }));

        assert!(panic.is_err());
        let repeated = initialization
            .initialize_once(|| panic!("a failed connection must never initialize again"));
        assert!(repeated.is_err());
    }

    #[test]
    fn cancelled_waiter_does_not_abort_shared_initialize_for_another_attempt() {
        let initialization = Arc::new(ConnectionInitialization::default());
        let attempt_a_cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let session_new_count = Arc::new(AtomicUsize::new(0));
        let (initialize_started_tx, initialize_started_rx) = mpsc::channel();
        let (release_initialize_tx, release_initialize_rx) = mpsc::channel();

        let a_initialization = Arc::clone(&initialization);
        let a_cancelled = Arc::clone(&attempt_a_cancelled);
        let a_session_new_count = Arc::clone(&session_new_count);
        let attempt_a = thread::spawn(move || {
            let outcome = a_initialization
                .initialize_once(|| {
                    initialize_started_tx.send(()).unwrap();
                    release_initialize_rx.recv().unwrap();
                    Ok(json!({ "loadSession": true }))
                })
                .unwrap();
            if !a_cancelled.load(Ordering::SeqCst) {
                a_session_new_count.fetch_add(1, Ordering::SeqCst);
            }
            outcome
        });
        initialize_started_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        let b_initialization = Arc::clone(&initialization);
        let b_session_new_count = Arc::clone(&session_new_count);
        let attempt_b = thread::spawn(move || {
            let outcome = b_initialization
                .initialize_once(|| panic!("attempt B must consume the shared initialize result"))
                .unwrap();
            b_session_new_count.fetch_add(1, Ordering::SeqCst);
            outcome
        });
        attempt_a_cancelled.store(true, Ordering::SeqCst);
        release_initialize_tx.send(()).unwrap();

        let a_outcome = attempt_a.join().unwrap();
        let b_outcome = attempt_b.join().unwrap();
        assert!(a_outcome.performed);
        assert!(!b_outcome.performed);
        assert_eq!(session_new_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_old_generation_cannot_match_its_replacement() {
        let old = Arc::new(());
        let replacement = Arc::new(());

        assert!(!is_same_connection_generation(&replacement, &old, 2, 1));
        assert!(!is_same_connection_generation(&old, &old, 2, 1));
        assert!(is_same_connection_generation(&old, &old, 1, 1));
    }

    #[test]
    fn connection_creation_is_single_flight_per_key() {
        let gate = Arc::new(ConnectionCreationGate::default());
        let key = AdapterConnectionKey::new("codex-acp", Utf8PathBuf::from("/repo"));
        let first_gate = Arc::clone(&gate);
        let first_key = key.clone();
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = thread::spawn(move || {
            let _guard = first_gate.enter(&first_key).unwrap();
            first_entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        first_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        let second_gate = Arc::clone(&gate);
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second = thread::spawn(move || {
            let _guard = second_gate.enter(&key).unwrap();
            second_entered_tx.send(()).unwrap();
        });
        assert!(
            second_entered_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        release_tx.send(()).unwrap();
        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        first.join().unwrap();
        second.join().unwrap();
    }

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
                ELICITATION_DEFAULT_TIMEOUT, ElicitationAction, pending_elicitation_state,
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
        write_current_attempt_node(&attempt_dir);
        let session_id = "session-ask".to_string();
        let elicitation_id = "elicit-close";
        write_pending_elicitation(
            &attempt_dir,
            &pending_elicitation_state(
                elicitation_id,
                "turn-1",
                "prompt-turn-1",
                json!(1),
                serde_json::from_value(json!({
                    "mode": "form",
                    "sessionId": session_id,
                    "message": "Choose",
                    "requestedSchema": { "type": "object", "properties": {} }
                }))
                .unwrap(),
                "1Z".to_string(),
            ),
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
    fn settling_one_established_session_does_not_affect_another_session() {
        let prompts = ActivePromptTracker::default();
        prompts.mark_active("session-a");
        prompts.mark_active("session-b");

        prompts.mark_inactive("session-a");

        assert_eq!(prompts.count_for_session("session-a"), 0);
        assert_eq!(prompts.count_for_session("session-b"), 1);
        assert_eq!(prompts.count(), 1);
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
    fn session_event_pump_waits_for_frame_arriving_after_response_queue_is_empty() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-delayed-replay");
        let pump = SessionEventPump::start(receiver);
        let producer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            assert!(sender.send(
                json!({
                    "method": "session/update",
                    "params": {
                        "sessionId": "session-delayed-replay",
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": "No response requested." }
                        }
                    }
                }),
                128,
            ));
        });

        let value = pump.recv_timeout(Duration::from_millis(250)).unwrap();
        assert_eq!(
            value
                .pointer("/params/update/content/text")
                .and_then(Value::as_str),
            Some("No response requested.")
        );
        producer.join().unwrap();
        pump.close();
    }

    #[test]
    fn session_event_pump_consumes_route_watermarks_in_order() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-watermark");
        let pump = SessionEventPump::start(receiver);
        let empty_watermark = sender.watermark().expect("empty route watermark");
        assert_eq!(empty_watermark.sequence(), 0);
        assert!(!empty_watermark.is_closed());
        assert!(pump.has_consumed(empty_watermark));

        assert!(sender.send(json!({ "kind": "systemError" }), 32));
        let terminal_watermark = sender.watermark().expect("terminal watermark");
        assert!(sender.send(json!({ "kind": "response-adjacent" }), 32));
        let response_watermark = sender.watermark().expect("response watermark");

        assert_eq!(
            terminal_watermark.route_generation(),
            response_watermark.route_generation()
        );
        assert_eq!(terminal_watermark.sequence(), 1);
        assert_eq!(response_watermark.sequence(), 2);
        assert!(!pump.has_consumed(terminal_watermark));

        assert_eq!(
            pump.recv_timeout(Duration::from_secs(1)).unwrap()["kind"],
            json!("systemError")
        );
        assert!(pump.has_consumed(terminal_watermark));
        assert!(!pump.has_consumed(response_watermark));

        assert_eq!(
            pump.recv_timeout(Duration::from_secs(1)).unwrap()["kind"],
            json!("response-adjacent")
        );
        assert!(pump.has_consumed(response_watermark));
        pump.close();
    }

    #[test]
    fn session_event_pump_rejects_watermark_from_replaced_route() {
        let (first_sender, first_receiver) = session_route_pair("test-adapter", "session-replaced");
        let first_pump = SessionEventPump::start(first_receiver);
        assert!(first_sender.send(json!({ "route": "first" }), 16));
        let first_watermark = first_sender.watermark().unwrap();
        let _ = first_pump.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(first_pump.has_consumed(first_watermark));

        let (second_sender, second_receiver) =
            session_route_pair("test-adapter", "session-replaced");
        let second_pump = SessionEventPump::start(second_receiver);
        assert!(second_sender.send(json!({ "route": "second" }), 16));
        let second_watermark = second_sender.watermark().unwrap();
        let _ = second_pump.recv_timeout(Duration::from_secs(1)).unwrap();

        assert!(!second_pump.has_consumed(first_watermark));
        assert!(second_pump.has_consumed(second_watermark));
        first_pump.close();
        second_pump.close();
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
    fn session_route_does_not_block_shared_reader_at_pump_frame_limit() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        for index in 0..256_u64 {
            assert!(sender.send(json!({ "index": index }), 1));
        }
        let started = std::time::Instant::now();
        assert!(sender.send(json!({ "index": 256 }), 1));
        assert!(started.elapsed() < Duration::from_millis(50));
        assert!(receiver.try_recv().is_ok());
    }

    #[test]
    fn session_route_does_not_block_shared_reader_at_pump_byte_limit() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        assert!(sender.send(json!({ "index": 0 }), 4 * 1024 * 1024));
        let started = std::time::Instant::now();
        assert!(sender.send(json!({ "index": 1 }), 1));
        assert!(started.elapsed() < Duration::from_millis(50));
        assert!(receiver.try_recv().is_ok());
    }

    #[test]
    fn session_route_allows_one_oversized_frame_when_empty() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        assert!(sender.send(json!({ "large": true }), 8 * 1024 * 1024));
        assert_eq!(receiver.try_recv().unwrap()["large"], json!(true));
    }

    #[test]
    fn dropping_receiver_rejects_subsequent_send() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        drop(receiver);
        assert!(!sender.send(json!({ "afterDrop": true }), 1));
    }

    #[test]
    fn closing_route_rejects_new_frames_and_allows_receiver_to_drain() {
        let (sender, receiver) = session_route_pair("test-adapter", "session-1");
        for index in 0..256_u64 {
            assert!(sender.send(json!({ "index": index }), 1));
        }
        let closing_sender = sender.clone();
        closing_sender.close();
        assert!(!sender.send(json!({ "afterClose": true }), 1));
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
    }

    #[test]
    fn saturated_session_route_does_not_delay_another_session() {
        let routes = Mutex::new(HashMap::new());
        let early_frames = Mutex::new(EarlySessionFrames::default());
        let receiver_a =
            register_session_route_state("test-adapter", "session-a", &routes, &early_frames);
        let receiver_b =
            register_session_route_state("test-adapter", "session-b", &routes, &early_frames);
        for index in 0..1_024_u64 {
            assert!(route_or_buffer_session_frame(
                &routes,
                &early_frames,
                "session-a",
                json!({ "index": index }),
                16 * 1024,
                std::time::Instant::now(),
            ));
        }

        let started = std::time::Instant::now();
        assert!(route_or_buffer_session_frame(
            &routes,
            &early_frames,
            "session-b",
            json!({ "response": "ready" }),
            32,
            std::time::Instant::now(),
        ));
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(receiver_b.try_recv().unwrap()["response"], json!("ready"));
        drop(receiver_a);
    }

    #[test]
    fn session_close_settles_pending_permission_and_snapshot() {
        let dir = tempdir().unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        write_current_attempt_node(&attempt_dir);
        let request_id = "close";
        write_pending_permission(
            &attempt_dir,
            request_id,
            "turn-1",
            "prompt-event-1",
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
            snapshot
                .get("latestTurnStatus")
                .and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert_eq!(
            snapshot.get("stopReason").and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert!(!attempt_dir.join("acp.session.json").exists());
    }
}
