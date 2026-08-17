//! Heartbeat reporter for startup, activity, and user-triggered business facts.
//!
//! Design:
//! - **appStarted**: sent once when metrics config is ready. Transient failures
//!   (network, timeout, 429, 5xx) trigger up to 2 retries at 30 s and 2 min.
//!   Deterministic failures (400/401/413) mark the request as rejected until
//!   the config changes.
//! - **activity**: triggered by window focus, pointer/keyboard interaction, and
//!   Direct/Workflow/AUTO business commands. Throttled to one successful send
//!   per 15 minutes; failures back off 1 minute. No retry beyond waiting for
//!   the next real activity signal.
//! - **business facts**: direct/workflow/AUTO run starts and durable scheduled
//!   task creation send independently with finite process-local retry.
//! - All paths share a single `reqwest::Client` and a `Mutex<HeartbeatState>`;
//!   business facts do not participate in activity throttle or in-flight state.
//! - No `MutexGuard` is ever held across an `.await` boundary (verified by the
//!   Rust compiler's Send analysis for spawned tasks).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::identity::{ClientOs, UserIdProvider, WhoamiUserIdProvider};
use super::metrics_log;

// ── Constants ────────────────────────────────────────────────────────────────

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const ACTIVITY_SUCCESS_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const ACTIVITY_FAILURE_BACKOFF: Duration = Duration::from_secs(60);
pub const APP_STARTED_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_secs(30), Duration::from_secs(2 * 60)];

// ── Clock abstraction ────────────────────────────────────────────────────────

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

// ── Request types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HeartbeatReason {
    AppStarted,
    Activity,
    DirectStarted,
    WorkflowStarted,
    AutoStarted,
    ScheduledTaskCreated,
}

impl HeartbeatReason {
    fn is_business_fact(self) -> bool {
        matches!(
            self,
            Self::DirectStarted
                | Self::WorkflowStarted
                | Self::AutoStarted
                | Self::ScheduledTaskCreated
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRequest {
    pub heartbeat_id: Uuid,
    pub user_id: String,
    pub reason: HeartbeatReason,
    pub client_version: String,
    pub os: ClientOs,
}

// ── Settings snapshot ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HeartbeatSettings {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
}

impl HeartbeatSettings {
    pub fn is_ready(&self) -> bool {
        self.enabled && self.endpoint.is_some() && self.api_key.is_some()
    }
}

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum AppStartedState {
    NotReady,
    Pending { attempts: u8 },
    Delivered,
    RejectedUntilConfigChanges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RequestLease {
    config_generation: u64,
    request_generation: u64,
}

#[derive(Debug)]
struct HeartbeatState {
    app_started: AppStartedState,
    app_started_request: Option<HeartbeatRequest>,
    config_fingerprint: Option<String>,
    config_generation: u64,
    current_settings: Option<HeartbeatSettings>,
    last_success_at: Option<Instant>,
    last_failed_attempt_at: Option<Instant>,
    request_in_flight: Option<RequestLease>,
    next_request_generation: u64,
    app_started_retry_scheduled: bool,
}

impl Default for HeartbeatState {
    fn default() -> Self {
        Self {
            app_started: AppStartedState::NotReady,
            app_started_request: None,
            config_fingerprint: None,
            config_generation: 0,
            current_settings: None,
            last_success_at: None,
            last_failed_attempt_at: None,
            request_in_flight: None,
            next_request_generation: 0,
            app_started_retry_scheduled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SendOutcome {
    Accepted,
    Duplicate,
    TransientFailure,
    DeterministicFailure,
}

#[derive(Debug, Deserialize)]
struct HeartbeatEnvelope {
    code: u32,
    data: HeartbeatResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatResponse {
    accepted: bool,
    duplicate: bool,
}

// ── Reporter ─────────────────────────────────────────────────────────────────

pub struct HeartbeatReporter {
    client: reqwest::Client,
    os: ClientOs,
    client_version: String,
    user_id_provider: Box<dyn UserIdProvider>,
    clock: Box<dyn Clock>,
    state: Mutex<HeartbeatState>,
}

impl HeartbeatReporter {
    pub fn new(client_version: String) -> Arc<Self> {
        Self::with_providers(
            client_version,
            Box::new(WhoamiUserIdProvider),
            Box::new(SystemClock),
        )
    }

    pub fn with_providers(
        client_version: String,
        user_id_provider: Box<dyn UserIdProvider>,
        clock: Box<dyn Clock>,
    ) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Arc::new(Self {
            client,
            os: super::identity::detect_os(),
            client_version,
            user_id_provider,
            clock,
            state: Mutex::new(HeartbeatState::default()),
        })
    }

    pub fn record(self: &Arc<Self>, settings: &HeartbeatSettings, reason: HeartbeatReason) {
        match reason {
            HeartbeatReason::AppStarted => self.handle_config_snapshot(settings),
            HeartbeatReason::Activity => self.record_activity(settings),
            business_reason => self.record_business_fact(settings, business_reason),
        }
    }

    /// Called on startup and whenever metrics config changes.
    pub fn handle_config_snapshot(self: &Arc<Self>, settings: &HeartbeatSettings) {
        let mut state = self.state.lock().unwrap();
        refresh_config_state(&mut state, settings);

        if !settings.is_ready() {
            if !matches!(state.app_started, AppStartedState::Delivered) {
                state.app_started = AppStartedState::NotReady;
            }
            state.app_started_retry_scheduled = false;
            return;
        }

        if matches!(state.app_started, AppStartedState::Delivered) {
            return;
        }

        if state.app_started_retry_scheduled {
            return;
        }

        let request = match &state.app_started_request {
            Some(request) => request.clone(),
            None => {
                let id = match self.user_id_provider.username() {
                    Some(id) => id,
                    None => {
                        metrics_log("[heartbeat] identity_unavailable");
                        state.app_started = AppStartedState::NotReady;
                        return;
                    }
                };
                let request = HeartbeatRequest {
                    heartbeat_id: Uuid::new_v4(),
                    user_id: id,
                    reason: HeartbeatReason::AppStarted,
                    client_version: self.client_version.clone(),
                    os: self.os,
                };
                state.app_started_request = Some(request.clone());
                request
            }
        };

        if matches!(
            state.app_started,
            AppStartedState::RejectedUntilConfigChanges
        ) {
            return;
        }
        if !matches!(state.app_started, AppStartedState::Pending { .. }) {
            state.app_started = AppStartedState::Pending { attempts: 0 };
        }
        // appStarted is a delivery fact, so it must survive an activity request
        // that won the shared slot. Activity completion resumes this pending send.
        let Some(lease) = try_acquire_request(&mut state) else {
            return;
        };
        drop(state);

        let reporter = self.clone();
        let endpoint = settings.endpoint.clone().unwrap();
        let api_key = settings.api_key.clone().unwrap();
        tauri::async_runtime::spawn(async move {
            reporter
                .send_app_started(&endpoint, &api_key, &request, lease)
                .await;
        });
    }

    /// Called on any UI activity signal.
    pub fn record_activity(self: &Arc<Self>, settings: &HeartbeatSettings) {
        let user_id = self.user_id_provider.username();
        let now = self.clock.now();

        let mut state = self.state.lock().unwrap();
        refresh_config_state(&mut state, settings);

        if !settings.is_ready() {
            return;
        }

        if let Some(last) = state.last_success_at {
            if now.duration_since(last) < ACTIVITY_SUCCESS_INTERVAL {
                metrics_log("[heartbeat] activity skipped: throttled_by_success_interval");
                return;
            }
        }

        if let Some(last_fail) = state.last_failed_attempt_at {
            if now.duration_since(last_fail) < ACTIVITY_FAILURE_BACKOFF {
                metrics_log("[heartbeat] activity skipped: failure_backoff");
                return;
            }
        }

        let id = match &user_id {
            Some(id) => id.clone(),
            None => {
                metrics_log("[heartbeat] activity skipped: identity_unavailable");
                return;
            }
        };

        let request = HeartbeatRequest {
            heartbeat_id: Uuid::new_v4(),
            user_id: id,
            reason: HeartbeatReason::Activity,
            client_version: self.client_version.clone(),
            os: self.os,
        };

        let Some(lease) = try_acquire_request(&mut state) else {
            metrics_log("[heartbeat] activity skipped: in_flight");
            return;
        };
        drop(state);

        let reporter = self.clone();
        let endpoint = settings.endpoint.clone().unwrap();
        let api_key = settings.api_key.clone().unwrap();
        tauri::async_runtime::spawn(async move {
            reporter
                .send_activity(&endpoint, &api_key, &request, lease)
                .await;
        });
    }

    /// Business facts are independent from activity throttle and in-flight state.
    fn record_business_fact(
        self: &Arc<Self>,
        settings: &HeartbeatSettings,
        reason: HeartbeatReason,
    ) {
        debug_assert!(reason.is_business_fact());

        let user_id = self.user_id_provider.username();
        let mut state = self.state.lock().unwrap();
        refresh_config_state(&mut state, settings);
        if !settings.is_ready() {
            return;
        }
        let generation = state.config_generation;
        drop(state);

        let Some(user_id) = user_id else {
            metrics_log("[heartbeat] business fact skipped: identity_unavailable");
            return;
        };
        let request = HeartbeatRequest {
            heartbeat_id: Uuid::new_v4(),
            user_id,
            reason,
            client_version: self.client_version.clone(),
            os: self.os,
        };
        let endpoint = settings.endpoint.clone().unwrap();
        let api_key = settings.api_key.clone().unwrap();
        let reporter = self.clone();
        tauri::async_runtime::spawn(async move {
            reporter
                .send_business_fact_with_retry(
                    endpoint,
                    api_key,
                    request,
                    generation,
                    &APP_STARTED_RETRY_DELAYS,
                )
                .await;
        });
    }

    // ── Async send: appStarted ───────────────────────────────────────────────

    /// Send an appStarted heartbeat and update state. On transient failure,
    /// schedule a retry. Must be called from a Send context (spawned task).
    async fn send_app_started(
        self: Arc<Self>,
        endpoint: &str,
        api_key: &str,
        request: &HeartbeatRequest,
        lease: RequestLease,
    ) {
        let outcome = self.send_once(endpoint, api_key, request).await;
        self.apply_app_started_outcome(request, lease, outcome);
    }

    /// Apply the outcome of an appStarted send, scheduling retries as needed.
    /// No await is called while holding a MutexGuard.
    fn apply_app_started_outcome(
        self: &Arc<Self>,
        request: &HeartbeatRequest,
        lease: RequestLease,
        outcome: SendOutcome,
    ) {
        let mut state = self.state.lock().unwrap();
        if !release_request(&mut state, lease) {
            return;
        }

        match outcome {
            SendOutcome::Accepted | SendOutcome::Duplicate => {
                state.last_success_at = Some(self.clock.now());
                state.last_failed_attempt_at = None;
                state.app_started = AppStartedState::Delivered;
                state.app_started_retry_scheduled = false;
            }
            SendOutcome::TransientFailure => {
                let attempts = match &state.app_started {
                    AppStartedState::Pending { attempts } => *attempts,
                    _ => 0,
                };
                let next_attempt = attempts + 1;
                state.last_failed_attempt_at = Some(self.clock.now());

                if (next_attempt as usize) <= APP_STARTED_RETRY_DELAYS.len() {
                    let delay = APP_STARTED_RETRY_DELAYS[(next_attempt - 1) as usize];
                    state.app_started = AppStartedState::Pending {
                        attempts: next_attempt,
                    };
                    state.app_started_retry_scheduled = true;
                    drop(state);

                    let reporter = self.clone();
                    let request = request.clone();
                    let generation = lease.config_generation;
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(delay).await;
                        reporter.send_app_started_retry(&request, generation).await;
                    });
                } else {
                    state.app_started = AppStartedState::Pending {
                        attempts: next_attempt,
                    };
                    state.app_started_retry_scheduled = false;
                }
            }
            SendOutcome::DeterministicFailure => {
                state.app_started = AppStartedState::RejectedUntilConfigChanges;
                state.app_started_retry_scheduled = false;
            }
        }
    }

    /// Retry send after a delay. Guards against double-send if config changed.
    async fn send_app_started_retry(self: Arc<Self>, request: &HeartbeatRequest, generation: u64) {
        let (endpoint, api_key, lease) = {
            let mut state = self.state.lock().unwrap();
            if state.config_generation != generation
                || !matches!(state.app_started, AppStartedState::Pending { .. })
                || !state.app_started_retry_scheduled
            {
                return;
            }
            let Some(settings) = state.current_settings.clone() else {
                return;
            };
            state.app_started_retry_scheduled = false;
            let Some(lease) = try_acquire_request(&mut state) else {
                return;
            };
            (settings.endpoint.unwrap(), settings.api_key.unwrap(), lease)
        };

        // Send and apply outcome. No guard held across await.
        let outcome = self.send_once(&endpoint, &api_key, request).await;
        self.apply_app_started_outcome(request, lease, outcome);
    }

    // ── Async send: activity ─────────────────────────────────────────────────

    async fn send_activity(
        self: Arc<Self>,
        endpoint: &str,
        api_key: &str,
        request: &HeartbeatRequest,
        lease: RequestLease,
    ) {
        let outcome = self.send_once(endpoint, api_key, request).await;

        let pending_app_started_settings = {
            let mut state = self.state.lock().unwrap();
            if !release_request(&mut state, lease) {
                return;
            }

            match outcome {
                SendOutcome::Accepted | SendOutcome::Duplicate => {
                    state.last_success_at = Some(self.clock.now());
                    state.last_failed_attempt_at = None;
                }
                SendOutcome::TransientFailure | SendOutcome::DeterministicFailure => {
                    state.last_failed_attempt_at = Some(self.clock.now());
                }
            }

            (matches!(state.app_started, AppStartedState::Pending { .. })
                && !state.app_started_retry_scheduled)
                .then(|| state.current_settings.clone())
                .flatten()
        };

        if let Some(settings) = pending_app_started_settings {
            self.handle_config_snapshot(&settings);
        }
    }

    // ── Async send: business facts ──────────────────────────────────────────

    async fn send_business_fact_with_retry(
        self: Arc<Self>,
        mut endpoint: String,
        mut api_key: String,
        request: HeartbeatRequest,
        generation: u64,
        retry_delays: &[Duration],
    ) {
        let mut retry_index = 0;
        loop {
            let outcome = self.send_once(&endpoint, &api_key, &request).await;
            if outcome != SendOutcome::TransientFailure || retry_index >= retry_delays.len() {
                return;
            }

            tokio::time::sleep(retry_delays[retry_index]).await;
            retry_index += 1;

            let Some((current_endpoint, current_api_key)) =
                self.delivery_settings_for_generation(generation)
            else {
                return;
            };
            endpoint = current_endpoint;
            api_key = current_api_key;
        }
    }

    fn delivery_settings_for_generation(&self, generation: u64) -> Option<(String, String)> {
        let state = self.state.lock().unwrap();
        if state.config_generation != generation {
            return None;
        }
        let settings = state.current_settings.as_ref()?;
        Some((settings.endpoint.clone()?, settings.api_key.clone()?))
    }

    // ── HTTP send core ───────────────────────────────────────────────────────

    async fn send_once(
        &self,
        endpoint: &str,
        api_key: &str,
        request: &HeartbeatRequest,
    ) -> SendOutcome {
        let started = Instant::now();
        let result = self
            .client
            .post(endpoint)
            .header("X-Maling-Report-Key", api_key)
            .header("Content-Type", "application/json;charset=UTF-8")
            .json(request)
            .send()
            .await;

        let latency_ms = started.elapsed().as_millis();

        match result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    let outcome = match resp.json::<HeartbeatEnvelope>().await {
                        Ok(envelope)
                            if envelope.code == 200
                                && envelope.data.accepted
                                && !envelope.data.duplicate =>
                        {
                            SendOutcome::Accepted
                        }
                        Ok(envelope)
                            if envelope.code == 200
                                && !envelope.data.accepted
                                && envelope.data.duplicate =>
                        {
                            SendOutcome::Duplicate
                        }
                        _ => SendOutcome::TransientFailure,
                    };
                    metrics_log(&format!(
                        "[heartbeat] result={} reason={} http={} latency_ms={}",
                        match outcome {
                            SendOutcome::Accepted => "accepted",
                            SendOutcome::Duplicate => "duplicate",
                            _ => "invalid_envelope",
                        },
                        reason_str(request.reason),
                        status,
                        latency_ms,
                    ));
                    outcome
                } else if status == 429 || status >= 500 {
                    metrics_log(&format!(
                        "[heartbeat] result=transient_failure reason={} http={} latency_ms={}",
                        reason_str(request.reason),
                        status,
                        latency_ms
                    ));
                    SendOutcome::TransientFailure
                } else {
                    metrics_log(&format!(
                        "[heartbeat] result=deterministic_failure reason={} http={} latency_ms={}",
                        reason_str(request.reason),
                        status,
                        latency_ms
                    ));
                    SendOutcome::DeterministicFailure
                }
            }
            Err(err) => {
                metrics_log(&format!(
                    "[heartbeat] result=transient_failure reason={} error={} latency_ms={}",
                    reason_str(request.reason),
                    err,
                    latency_ms,
                ));
                SendOutcome::TransientFailure
            }
        }
    }

    // ── Test-only state inspection ───────────────────────────────────────────

    #[cfg(test)]
    pub fn is_delivered(&self) -> bool {
        matches!(
            self.state.lock().unwrap().app_started,
            AppStartedState::Delivered
        )
    }

    #[cfg(test)]
    pub fn is_request_in_flight(&self) -> bool {
        self.state.lock().unwrap().request_in_flight.is_some()
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn config_fingerprint(settings: &HeartbeatSettings) -> String {
    format!(
        "{}|{}|{}",
        settings.enabled,
        settings.endpoint.as_deref().unwrap_or(""),
        settings.api_key.as_deref().unwrap_or(""),
    )
}

fn try_acquire_request(state: &mut HeartbeatState) -> Option<RequestLease> {
    if state.request_in_flight.is_some() {
        return None;
    }
    state.next_request_generation = state.next_request_generation.wrapping_add(1);
    let lease = RequestLease {
        config_generation: state.config_generation,
        request_generation: state.next_request_generation,
    };
    state.request_in_flight = Some(lease);
    Some(lease)
}

fn release_request(state: &mut HeartbeatState, lease: RequestLease) -> bool {
    if state.request_in_flight != Some(lease) {
        return false;
    }
    state.request_in_flight = None;
    true
}

fn refresh_config_state(state: &mut HeartbeatState, settings: &HeartbeatSettings) -> bool {
    let fingerprint = config_fingerprint(settings);
    let config_changed = state.config_fingerprint.as_deref() != Some(&fingerprint);
    if config_changed {
        state.config_generation = state.config_generation.wrapping_add(1);
        state.app_started_retry_scheduled = false;
        // In-flight HTTP cannot be force-cancelled, but generation-tagged
        // outcomes are ignored and the logical slot can be reused.
        state.request_in_flight = None;
        if matches!(
            state.app_started,
            AppStartedState::RejectedUntilConfigChanges
        ) {
            state.app_started = AppStartedState::NotReady;
        }
    }
    state.current_settings = settings.is_ready().then(|| settings.clone());
    state.config_fingerprint = Some(fingerprint);
    if !settings.is_ready() {
        if !matches!(state.app_started, AppStartedState::Delivered) {
            state.app_started = AppStartedState::NotReady;
        }
        state.app_started_retry_scheduled = false;
    }
    config_changed
}

fn reason_str(reason: HeartbeatReason) -> &'static str {
    match reason {
        HeartbeatReason::AppStarted => "appStarted",
        HeartbeatReason::Activity => "activity",
        HeartbeatReason::DirectStarted => "directStarted",
        HeartbeatReason::WorkflowStarted => "workflowStarted",
        HeartbeatReason::AutoStarted => "autoStarted",
        HeartbeatReason::ScheduledTaskCreated => "scheduledTaskCreated",
    }
}

#[cfg(test)]
pub fn build_heartbeat_settings(
    enabled: bool,
    endpoint: Option<&str>,
    api_key: Option<&str>,
) -> HeartbeatSettings {
    HeartbeatSettings {
        enabled,
        endpoint: endpoint.map(String::from),
        api_key: api_key.map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[derive(Clone)]
    struct FakeClock {
        now: Arc<Mutex<Instant>>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Arc::new(Mutex::new(Instant::now())),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.now.lock().unwrap();
            *now += duration;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.now.lock().unwrap()
        }
    }

    fn request(reason: HeartbeatReason) -> HeartbeatRequest {
        HeartbeatRequest {
            heartbeat_id: Uuid::new_v4(),
            user_id: "testuser".to_string(),
            reason,
            client_version: "0.1.0".to_string(),
            os: ClientOs::Windows,
        }
    }

    fn test_reporter() -> Arc<HeartbeatReporter> {
        let mut reporter = HeartbeatReporter::new("0.1.0".to_string());
        Arc::get_mut(&mut reporter).unwrap().client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        reporter
    }

    fn fixed_test_reporter() -> Arc<HeartbeatReporter> {
        let mut reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        Arc::get_mut(&mut reporter).unwrap().client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        reporter
    }

    fn mock_http_response(status: &str, body: &str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut buffer = [0_u8; 4096];
                        let _ = stream.read(&mut buffer);
                        stream.write_all(response.as_bytes()).unwrap();
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            panic!("mock heartbeat server did not receive a request");
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("mock heartbeat server failed: {error}"),
                }
            }
        });
        (format!("http://{address}/heartbeat"), handle)
    }

    fn mock_http_responses(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            for (status, body) in responses {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                panic!("mock heartbeat server did not receive all requests");
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("mock heartbeat server failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                let mut buffer = [0_u8; 4096];
                let size = stream.read(&mut buffer).unwrap();
                requests.push(String::from_utf8_lossy(&buffer[..size]).into_owned());
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}/heartbeat"), handle)
    }

    fn ready_settings() -> HeartbeatSettings {
        build_heartbeat_settings(true, Some("http://test.local/heartbeat"), Some("test-key"))
    }

    #[test]
    fn heartbeat_request_has_exactly_five_camel_case_fields() {
        let req = request(HeartbeatReason::AppStarted);
        let json = serde_json::to_value(&req).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 5, "payload must have exactly 5 fields");
        assert!(obj.contains_key("heartbeatId"));
        assert!(obj.contains_key("userId"));
        assert!(obj.contains_key("reason"));
        assert!(obj.contains_key("clientVersion"));
        assert!(obj.contains_key("os"));
    }

    #[test]
    fn reason_serializes_to_camel_case() {
        for (reason, expected) in [
            (HeartbeatReason::AppStarted, "appStarted"),
            (HeartbeatReason::Activity, "activity"),
            (HeartbeatReason::DirectStarted, "directStarted"),
            (HeartbeatReason::WorkflowStarted, "workflowStarted"),
            (HeartbeatReason::AutoStarted, "autoStarted"),
            (
                HeartbeatReason::ScheduledTaskCreated,
                "scheduledTaskCreated",
            ),
        ] {
            assert_eq!(
                serde_json::to_string(&reason).unwrap(),
                format!("\"{expected}\"")
            );
        }
    }

    #[test]
    fn settings_is_ready_requires_all_three() {
        assert!(build_heartbeat_settings(true, Some("http://x"), Some("k")).is_ready());
        assert!(!build_heartbeat_settings(false, Some("http://x"), Some("k")).is_ready());
        assert!(!build_heartbeat_settings(true, None, Some("k")).is_ready());
        assert!(!build_heartbeat_settings(true, Some("http://x"), None).is_ready());
    }

    #[test]
    fn app_started_sends_when_config_ready() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        reporter.handle_config_snapshot(&ready_settings());
        let state = reporter.state.lock().unwrap();
        assert!(matches!(state.app_started, AppStartedState::Pending { .. }));
        assert!(state.request_in_flight.is_some());
    }

    #[test]
    fn app_started_skipped_when_disabled() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        reporter.handle_config_snapshot(&build_heartbeat_settings(
            false,
            Some("http://x"),
            Some("k"),
        ));
        let state = reporter.state.lock().unwrap();
        assert!(matches!(state.app_started, AppStartedState::NotReady));
        assert!(state.request_in_flight.is_none());
    }

    #[test]
    fn app_started_skipped_when_identity_unavailable() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider { value: None }),
            Box::new(SystemClock),
        );
        reporter.handle_config_snapshot(&ready_settings());
        let state = reporter.state.lock().unwrap();
        assert!(matches!(state.app_started, AppStartedState::NotReady));
        assert!(state.request_in_flight.is_none());
    }

    #[test]
    fn app_started_not_resent_after_delivery() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        {
            let mut state = reporter.state.lock().unwrap();
            state.app_started = AppStartedState::Delivered;
        }
        reporter.handle_config_snapshot(&ready_settings());
        assert!(reporter.is_delivered());
    }

    #[test]
    fn activity_throttled_by_success_interval() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        {
            let mut state = reporter.state.lock().unwrap();
            state.last_success_at = Some(reporter.clock.now());
        }
        reporter.record_activity(&ready_settings());
        assert!(!reporter.is_request_in_flight());
    }

    #[test]
    fn activity_merged_when_in_flight() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        {
            let mut state = reporter.state.lock().unwrap();
            refresh_config_state(&mut state, &ready_settings());
            try_acquire_request(&mut state).unwrap();
        }
        reporter.record_activity(&ready_settings());
        assert!(reporter.is_request_in_flight());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_started_retry_waits_for_the_activity_lease_then_resumes() {
        let reporter = fixed_test_reporter();
        let accepted = r#"{"code":200,"msg":"","data":{"accepted":true,"duplicate":false}}"#;
        let (endpoint, server) =
            mock_http_responses(vec![("200 OK", accepted), ("200 OK", accepted)]);
        let settings = build_heartbeat_settings(true, Some(&endpoint), Some("test-key"));
        let app_started_request = request(HeartbeatReason::AppStarted);
        let (generation, activity_lease) = {
            let mut state = reporter.state.lock().unwrap();
            refresh_config_state(&mut state, &settings);
            state.app_started = AppStartedState::Pending { attempts: 1 };
            state.app_started_request = Some(app_started_request.clone());
            state.app_started_retry_scheduled = true;
            let lease = try_acquire_request(&mut state).unwrap();
            (state.config_generation, lease)
        };

        reporter
            .clone()
            .send_app_started_retry(&app_started_request, generation)
            .await;

        {
            let state = reporter.state.lock().unwrap();
            assert_eq!(state.request_in_flight, Some(activity_lease));
            assert!(!state.app_started_retry_scheduled);
            assert!(matches!(
                state.app_started,
                AppStartedState::Pending { attempts: 1 }
            ));
        }

        reporter
            .clone()
            .send_activity(
                &endpoint,
                "test-key",
                &request(HeartbeatReason::Activity),
                activity_lease,
            )
            .await;

        let requests = tokio::task::spawn_blocking(move || server.join().unwrap())
            .await
            .unwrap();
        let reasons = requests
            .iter()
            .map(|raw_request| {
                let body = raw_request.split("\r\n\r\n").nth(1).unwrap();
                serde_json::from_str::<serde_json::Value>(body).unwrap()["reason"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(reasons, vec!["activity", "appStarted"]);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !reporter.is_delivered() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_started_waiting_behind_activity_is_sent_after_activity_completes() {
        let reporter = fixed_test_reporter();
        let accepted = r#"{"code":200,"msg":"","data":{"accepted":true,"duplicate":false}}"#;
        let (endpoint, server) =
            mock_http_responses(vec![("200 OK", accepted), ("200 OK", accepted)]);
        let settings = build_heartbeat_settings(true, Some(&endpoint), Some("test-key"));
        let activity_lease = {
            let mut state = reporter.state.lock().unwrap();
            refresh_config_state(&mut state, &settings);
            try_acquire_request(&mut state).unwrap()
        };

        reporter.handle_config_snapshot(&settings);
        {
            let state = reporter.state.lock().unwrap();
            assert!(matches!(
                state.app_started,
                AppStartedState::Pending { attempts: 0 }
            ));
            assert_eq!(
                state
                    .app_started_request
                    .as_ref()
                    .map(|request| request.reason),
                Some(HeartbeatReason::AppStarted)
            );
            assert_eq!(state.request_in_flight, Some(activity_lease));
        }

        reporter
            .clone()
            .send_activity(
                &endpoint,
                "test-key",
                &request(HeartbeatReason::Activity),
                activity_lease,
            )
            .await;

        let requests = tokio::task::spawn_blocking(move || server.join().unwrap())
            .await
            .unwrap();
        let reasons = requests
            .iter()
            .map(|raw_request| {
                let body = raw_request.split("\r\n\r\n").nth(1).unwrap();
                serde_json::from_str::<serde_json::Value>(body).unwrap()["reason"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(reasons, vec!["activity", "appStarted"]);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !reporter.is_delivered() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn activity_backoff_after_failure() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        {
            let mut state = reporter.state.lock().unwrap();
            state.last_failed_attempt_at = Some(Instant::now());
        }
        reporter.record_activity(&ready_settings());
        assert!(!reporter.is_request_in_flight());
    }

    #[test]
    fn config_change_un_rejects_app_started() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        let original_request = request(HeartbeatReason::AppStarted);
        let original_id = original_request.heartbeat_id;
        {
            let mut state = reporter.state.lock().unwrap();
            state.app_started = AppStartedState::RejectedUntilConfigChanges;
            state.app_started_request = Some(original_request);
            state.config_fingerprint = Some("old-fingerprint".to_string());
        }
        reporter.handle_config_snapshot(&ready_settings());
        let state = reporter.state.lock().unwrap();
        assert!(matches!(state.app_started, AppStartedState::Pending { .. }));
        assert_eq!(
            state
                .app_started_request
                .as_ref()
                .map(|request| request.heartbeat_id),
            Some(original_id)
        );
    }

    #[test]
    fn disabled_metrics_cancels_pending_app_started() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        {
            let mut state = reporter.state.lock().unwrap();
            state.app_started_request = Some(HeartbeatRequest {
                heartbeat_id: Uuid::new_v4(),
                user_id: "testuser".to_string(),
                reason: HeartbeatReason::AppStarted,
                client_version: "0.1.0".to_string(),
                os: ClientOs::Windows,
            });
            state.app_started = AppStartedState::Pending { attempts: 1 };
            state.config_fingerprint = Some("x".to_string());
        }
        reporter.handle_config_snapshot(&build_heartbeat_settings(
            false,
            Some("http://x"),
            Some("k"),
        ));
        let state = reporter.state.lock().unwrap();
        assert!(matches!(state.app_started, AppStartedState::NotReady));
        assert!(state.current_settings.is_none());
        assert!(!state.app_started_retry_scheduled);
    }

    #[test]
    fn stale_request_completion_cannot_release_the_current_request_generation() {
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(SystemClock),
        );
        let request = request(HeartbeatReason::AppStarted);
        let (stale_lease, current_lease) = {
            let mut state = reporter.state.lock().unwrap();
            state.config_generation = 2;
            state.app_started_request = Some(request.clone());
            state.app_started = AppStartedState::Pending { attempts: 0 };
            let stale_lease = try_acquire_request(&mut state).unwrap();
            assert!(release_request(&mut state, stale_lease));
            let current_lease = try_acquire_request(&mut state).unwrap();
            (stale_lease, current_lease)
        };

        reporter.apply_app_started_outcome(&request, stale_lease, SendOutcome::Accepted);

        let state = reporter.state.lock().unwrap();
        assert_eq!(state.request_in_flight, Some(current_lease));
        assert!(matches!(state.app_started, AppStartedState::Pending { .. }));
        assert!(state.last_success_at.is_none());
    }

    #[test]
    fn fake_clock_releases_activity_after_success_interval() {
        let clock = FakeClock::new();
        let reporter = HeartbeatReporter::with_providers(
            "0.1.0".to_string(),
            Box::new(super::super::identity::FixedUserIdProvider {
                value: Some("testuser".to_string()),
            }),
            Box::new(clock.clone()),
        );
        {
            let mut state = reporter.state.lock().unwrap();
            state.last_success_at = Some(clock.now());
        }
        clock.advance(ACTIVITY_SUCCESS_INTERVAL + Duration::from_secs(1));
        reporter.record_activity(&ready_settings());
        assert!(reporter.is_request_in_flight());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_http_requires_a_valid_success_envelope() {
        let reporter = test_reporter();
        let (endpoint, server) = mock_http_response("200 OK", "{}");
        let outcome = reporter
            .send_once(&endpoint, "test-key", &request(HeartbeatReason::AppStarted))
            .await;
        server.join().unwrap();
        assert_eq!(outcome, SendOutcome::TransientFailure);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_http_recognizes_duplicate_envelope() {
        let reporter = test_reporter();
        let body = r#"{"code":200,"msg":"","data":{"accepted":false,"duplicate":true,"receivedAt":"2026-07-30T00:00:00Z"}}"#;
        let (endpoint, server) = mock_http_response("200 OK", body);
        let outcome = reporter
            .send_once(&endpoint, "test-key", &request(HeartbeatReason::AppStarted))
            .await;
        server.join().unwrap();
        assert_eq!(outcome, SendOutcome::Duplicate);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn business_fact_reuses_id_across_retry_without_touching_activity_state() {
        let reporter = fixed_test_reporter();
        let accepted = r#"{"code":200,"msg":"","data":{"accepted":true,"duplicate":false}}"#;
        let (endpoint, server) = mock_http_responses(vec![
            ("500 Internal Server Error", "{}"),
            ("200 OK", accepted),
        ]);
        let settings = build_heartbeat_settings(true, Some(&endpoint), Some("test-key"));
        let last_success_at = Instant::now();
        let last_failed_attempt_at = last_success_at - Duration::from_secs(1);
        let occupied_lease = {
            let mut state = reporter.state.lock().unwrap();
            refresh_config_state(&mut state, &settings);
            state.last_success_at = Some(last_success_at);
            state.last_failed_attempt_at = Some(last_failed_attempt_at);
            let lease = try_acquire_request(&mut state).unwrap();
            (state.config_generation, lease)
        };
        let request = request(HeartbeatReason::DirectStarted);
        let heartbeat_id = request.heartbeat_id.to_string();

        reporter
            .clone()
            .send_business_fact_with_retry(
                endpoint,
                "test-key".to_string(),
                request,
                occupied_lease.0,
                &[Duration::ZERO],
            )
            .await;

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        for raw_request in requests {
            let body = raw_request.split("\r\n\r\n").nth(1).unwrap();
            let json: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(json["heartbeatId"], heartbeat_id);
            assert_eq!(json["reason"], "directStarted");
        }
        let state = reporter.state.lock().unwrap();
        assert_eq!(state.request_in_flight, Some(occupied_lease.1));
        assert_eq!(state.last_success_at, Some(last_success_at));
        assert_eq!(state.last_failed_attempt_at, Some(last_failed_attempt_at));
    }

    #[test]
    fn config_generation_invalidates_pending_business_retry() {
        let reporter = fixed_test_reporter();
        let first = build_heartbeat_settings(true, Some("http://first/heartbeat"), Some("key-1"));
        let generation = {
            let mut state = reporter.state.lock().unwrap();
            refresh_config_state(&mut state, &first);
            state.config_generation
        };
        let second = build_heartbeat_settings(true, Some("http://second/heartbeat"), Some("key-2"));
        {
            let mut state = reporter.state.lock().unwrap();
            refresh_config_state(&mut state, &second);
        }

        assert!(
            reporter
                .delivery_settings_for_generation(generation)
                .is_none()
        );
    }

    #[test]
    fn retry_delay_schedule_is_correct() {
        assert_eq!(APP_STARTED_RETRY_DELAYS.len(), 2);
        assert_eq!(APP_STARTED_RETRY_DELAYS[0], Duration::from_secs(30));
        assert_eq!(APP_STARTED_RETRY_DELAYS[1], Duration::from_secs(2 * 60));
    }

    #[test]
    fn throttle_intervals_are_correct() {
        assert_eq!(ACTIVITY_SUCCESS_INTERVAL, Duration::from_secs(15 * 60));
        assert_eq!(ACTIVITY_FAILURE_BACKOFF, Duration::from_secs(60));
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(10));
    }
}
