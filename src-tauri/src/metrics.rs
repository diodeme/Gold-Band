use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use gold_band::app::RuntimeLifecycleEvent;
use gold_band::app::observability::{
    LifecycleTiming, MetricsCounters as CoreMetricsCounters, MetricsLifecycleFact, ModelUsage,
    TokenUsage,
};
use gold_band::config::RuntimeConfig;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use url::Url;

use crate::{channel::current_channel_config, state::DesktopState};

/// Cached log path 鈥?resolved once, avoids env-var lookup + create_dir_all on every log line.
static METRICS_LOG_PATH: OnceLock<Option<String>> = OnceLock::new();
const HEARTBEAT_ENDPOINT_PATH: &str = "/api/client-report/heartbeat";
const NODE_METRICS_ENDPOINT_PATH: &str = "/api/client-report/metrics/batch";
const METRICS_QUEUE_CAPACITY: usize = 2048;
const METRICS_BATCH_LIMIT: usize = 100;
const METRICS_LOG_LIMIT_BYTES: u64 = 20 * 1024 * 1024;

fn metrics_log_path() -> Option<&'static str> {
    METRICS_LOG_PATH
        .get_or_init(|| {
            let config = current_channel_config();
            let app_key = config.app_key;
            let log_dir = if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
                format!("{}\\{}", local_app_data, app_key)
            } else if let Ok(home) = std::env::var("USERPROFILE") {
                format!("{}\\.{}", home, app_key)
            } else {
                return None;
            };
            if let Err(e) = std::fs::create_dir_all(&log_dir) {
                eprintln!("[metrics] failed to create log dir {}: {}", log_dir, e);
                return None;
            }
            Some(format!("{}\\metrics.log", log_dir))
        })
        .as_deref()
}

/// Write a metrics log line to the application data directory.
/// On Windows this is `%LOCALAPPDATA%\{app_key}\metrics.log`.
pub(crate) fn metrics_log(msg: &str) {
    eprintln!("{}", msg);
    let Some(log_path) = metrics_log_path() else {
        return;
    };
    let line = format!(
        "[{}] {}\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
        msg
    );
    let line = if line.len() as u64 > METRICS_LOG_LIMIT_BYTES {
        format!(
            "[{}] payload-too-large actualBytes={}\n",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
            line.len()
        )
    } else {
        line
    };
    if let Ok(metadata) = std::fs::metadata(log_path) {
        if metadata.len().saturating_add(line.len() as u64) > METRICS_LOG_LIMIT_BYTES {
            let reset = format!(
                "[{}] log-reset reason=size-limit\n",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
            );
            if let Err(error) = std::fs::write(log_path, reset) {
                eprintln!("[metrics] failed to reset log {}: {}", log_path, error);
            }
        }
    }
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| f.write_all(line.as_bytes()))
    {
        eprintln!("[metrics] failed to write log {}: {}", log_path, e);
    }
}

/// Convert a Gold Band internal timestamp (Unix secs like "1780990488Z") to local ISO-8601.
fn to_iso8601(ts: &str) -> String {
    let secs: i64 = ts.trim_end_matches('Z').parse().unwrap_or(0);
    if secs == 0 {
        return ts.to_string();
    }
    if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
        dt.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
    } else {
        ts.to_string()
    }
}

// 鈹€鈹€ Settings VM 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSettingsVm {
    pub enabled: bool,
    pub toggle_locked: bool,
    pub metrics_base_url: Option<String>,
    pub heartbeat_endpoint: Option<String>,
    pub node_metrics_endpoint: Option<String>,
    pub api_key_set: bool, // true if api_key is non-empty (never expose the key itself)
}

pub fn normalize_metrics_base_url(raw: &str) -> Option<String> {
    let mut value = raw.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return None;
    }
    for suffix in [HEARTBEAT_ENDPOINT_PATH, NODE_METRICS_ENDPOINT_PATH] {
        if value.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
            value = value.trim_end_matches('/').to_string();
            break;
        }
    }

    let mut url = Url::parse(&value).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    let mut normalized = url.to_string().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        normalized = value;
    }
    Some(normalized)
}

pub(crate) fn metrics_base_url(config: &RuntimeConfig) -> Option<String> {
    let channel_config = current_channel_config();
    config
        .desktop_metrics_base_url
        .as_deref()
        .and_then(normalize_metrics_base_url)
        .or_else(|| normalize_metrics_base_url(channel_config.metrics_base_url))
}

pub(crate) fn endpoint_from_base_url(base_url: &str, path: &str) -> Option<String> {
    normalize_metrics_base_url(base_url)
        .map(|base| format!("{}{}", base.trim_end_matches('/'), path))
}

pub fn metrics_settings(config: &RuntimeConfig) -> MetricsSettingsVm {
    let channel_config = current_channel_config();
    eprintln!(
        "[metrics] channel raw: ch_enabled={} ch_locked={} ch_base_url={} ch_apikey_empty={}",
        channel_config.metrics_enabled,
        channel_config.metrics_toggle_locked,
        channel_config.metrics_base_url,
        channel_config.metrics_api_key.is_empty(),
    );
    let enabled = config.desktop_metrics_enabled || channel_config.metrics_enabled;
    let metrics_base_url = metrics_base_url(config);
    let heartbeat_endpoint = metrics_base_url
        .as_deref()
        .and_then(|base_url| endpoint_from_base_url(base_url, HEARTBEAT_ENDPOINT_PATH));
    let node_metrics_endpoint = metrics_base_url
        .as_deref()
        .and_then(|base_url| endpoint_from_base_url(base_url, NODE_METRICS_ENDPOINT_PATH));
    let api_key = config
        .desktop_metrics_api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let k = channel_config.metrics_api_key;
            if k.is_empty() {
                None
            } else {
                Some(k.to_string())
            }
        });
    MetricsSettingsVm {
        enabled: enabled && metrics_base_url.is_some(),
        toggle_locked: channel_config.metrics_toggle_locked,
        metrics_base_url,
        heartbeat_endpoint,
        node_metrics_endpoint,
        api_key_set: api_key.is_some(),
    }
}

// 鈹€鈹€ Heartbeat 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatPayload {
    user_id: String,
    workspace: String,
    client_version: String,
    reported_at: String,
}

async fn send_heartbeat(endpoint: &str, api_key: &str, workspace: &str, version: &str) {
    let user_id = get_system_username();
    let reported_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let payload = HeartbeatPayload {
        user_id,
        workspace: workspace.to_string(),
        client_version: version.to_string(),
        reported_at,
    };
    let body_str = serde_json::to_string(&payload).unwrap_or_default();
    metrics_log(&format!("[heartbeat] POST {} body: {}", endpoint, body_str));
    let client = reqwest::Client::new();
    let result = client
        .post(endpoint)
        .header("X-Maling-Report-Key", api_key)
        .header("Content-Type", "application/json;charset=UTF-8")
        .json(&payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    match result {
        Ok(resp) => {
            metrics_log(&format!("[heartbeat] response status={}", resp.status()));
        }
        Err(err) => {
            metrics_log(&format!("[heartbeat] FAILED (ignored): {}", err));
        }
    }
}

pub(crate) fn get_system_username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string())
}

const HEARTBEAT_INTERVAL_SECS: u64 = 900; // 15 minutes

pub fn start_heartbeat_polling<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(state) = app.try_state::<DesktopState>() {
                if let Ok(ctx) = state.context() {
                    let settings = metrics_settings(&ctx.config);
                    if settings.enabled {
                        if let Some(endpoint) = &settings.heartbeat_endpoint {
                            if let Some(api_key) = get_api_key(&ctx.config) {
                                let workspace = ctx.repo_root.to_string();
                                let version = env!("CARGO_PKG_VERSION").to_string();
                                metrics_log(&format!(
                                    "[heartbeat] timer fired, sending to {}",
                                    endpoint
                                ));
                                send_heartbeat(endpoint, &api_key, &workspace, &version).await;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
        }
    });
}

pub(crate) fn get_api_key(config: &RuntimeConfig) -> Option<String> {
    let channel_config = current_channel_config();
    config
        .desktop_metrics_api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let k = channel_config.metrics_api_key;
            if k.is_empty() {
                None
            } else {
                Some(k.to_string())
            }
        })
}

// 鈹€鈹€ Lifecycle Metrics 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum MetricsEventType {
    #[serde(rename = "execution.started")]
    ExecutionStarted,
    #[serde(rename = "execution.completed")]
    ExecutionCompleted,
    #[serde(rename = "execution.paused")]
    ExecutionPaused,
    #[serde(rename = "intervention.requested")]
    InterventionRequested,
    #[serde(rename = "execution.resumed")]
    ExecutionResumed,
    #[serde(rename = "acceptance.completed")]
    AcceptanceCompleted,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsSessionMode {
    Direct,
    Workflow,
    Auto,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsExecutionKind {
    Turn,
    Run,
    NodeAttempt,
    OuterRun,
    UnitAttempt,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MetricsCounters {
    pub pause_count: u32,
    pub resume_count: u32,
    pub permission_request_count: u32,
    pub elicitation_count: u32,
    pub manual_continue_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsEventItem {
    pub event_id: String,
    pub event_revision: u64,
    pub event_type: MetricsEventType,
    pub occurred_at: String,
    pub reported_at: String,
    pub user_id: String,
    pub workspace: String,
    pub client_version: String,
    pub session_mode: MetricsSessionMode,
    pub task_id: String,
    pub execution_kind: MetricsExecutionKind,
    pub execution_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counters: Option<MetricsCounters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_pass: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervention_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_pause_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_usages: Option<Vec<ModelUsage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<LifecycleTiming>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_state_recovered: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricsEventBatch {
    events: Vec<MetricsEventItem>,
}

fn iso_now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
}

fn map_runtime_event(event: RuntimeLifecycleEvent) -> Option<MetricsEventItem> {
    match event {
        RuntimeLifecycleEvent::MetricsFact(fact) => Some(map_metrics_fact(fact, iso_now())),
        _ => None,
    }
}
fn wire<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn map_metrics_fact(fact: MetricsLifecycleFact, reported_at: String) -> MetricsEventItem {
    let event_type = match fact.event_type {
        gold_band::app::observability::LifecycleEventType::ExecutionStarted => {
            MetricsEventType::ExecutionStarted
        }
        gold_band::app::observability::LifecycleEventType::ExecutionCompleted => {
            MetricsEventType::ExecutionCompleted
        }
        gold_band::app::observability::LifecycleEventType::ExecutionPaused => {
            MetricsEventType::ExecutionPaused
        }
        gold_band::app::observability::LifecycleEventType::ExecutionResumed => {
            MetricsEventType::ExecutionResumed
        }
        gold_band::app::observability::LifecycleEventType::InterventionRequested => {
            MetricsEventType::InterventionRequested
        }
        gold_band::app::observability::LifecycleEventType::AcceptanceCompleted => {
            MetricsEventType::AcceptanceCompleted
        }
    };
    let session_mode = match fact.session_mode {
        gold_band::app::observability::MetricsSessionMode::Direct => MetricsSessionMode::Direct,
        gold_band::app::observability::MetricsSessionMode::Workflow => MetricsSessionMode::Workflow,
        gold_band::app::observability::MetricsSessionMode::Auto => MetricsSessionMode::Auto,
    };
    let execution_kind = match fact.execution_kind {
        gold_band::app::observability::ExecutionKind::Turn => MetricsExecutionKind::Turn,
        gold_band::app::observability::ExecutionKind::Run => MetricsExecutionKind::Run,
        gold_band::app::observability::ExecutionKind::NodeAttempt => {
            MetricsExecutionKind::NodeAttempt
        }
        gold_band::app::observability::ExecutionKind::OuterRun => MetricsExecutionKind::OuterRun,
        gold_band::app::observability::ExecutionKind::UnitAttempt => {
            MetricsExecutionKind::UnitAttempt
        }
    };
    MetricsEventItem {
        event_id: fact.event_id,
        event_revision: fact.event_revision,
        event_type,
        occurred_at: to_iso8601(&fact.occurred_at),
        reported_at,
        user_id: fact.user_id,
        workspace: fact.workspace,
        client_version: env!("CARGO_PKG_VERSION").into(),
        session_mode,
        task_id: fact.task_id,
        execution_kind,
        execution_id: fact.execution_id,
        parent_execution_id: fact.parent_execution_id,
        node_id: fact.node_id,
        attempt_id: fact.attempt_id,
        attempt_index: fact.attempt_index,
        round_index: fact.round_index,
        role_name: fact.role_name,
        outcome: fact.outcome.map(wire),
        terminal_reason: fact.terminal_reason.map(wire),
        counters: fact.counters.map(|c: CoreMetricsCounters| MetricsCounters {
            pause_count: c.pause_count,
            resume_count: c.resume_count,
            permission_request_count: c.permission_request_count,
            elicitation_count: c.elicitation_count,
            manual_continue_count: c.manual_continue_count,
        }),
        unit_id: fact.unit_id,
        unit_kind: fact.unit_kind.map(wire),
        child_run_id: fact.child_run_id,
        terminal_reason_code: fact.terminal_reason_code,
        failed_attempt_id: fact.failed_attempt_id,
        round_count: fact.round_count,
        passed: fact.passed,
        acceptance_attempt: fact.acceptance_attempt,
        first_pass: fact.first_pass,
        intervention_kind: fact.intervention_kind.map(wire),
        pause_reason: fact.pause_reason.map(wire),
        previous_pause_reason: fact.previous_pause_reason.map(wire),
        provider: fact.provider,
        model: fact.model,
        usage: fact.usage,
        model_usages: fact.model_usages,
        timing: fact.timing.map(|t| LifecycleTiming { started_at: to_iso8601(&t.started_at), ended_at: t.ended_at.map(|e| to_iso8601(&e)), acp_session_elapsed_ms: t.acp_session_elapsed_ms }),
        collection_state_recovered: fact.collection_state_recovered,
    }
}

fn same_report_month(left: &str, right: &str) -> bool {
    left.get(..7) == right.get(..7)
}

async fn send_metrics_batch(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    events: Vec<MetricsEventItem>,
) {
    let batch = MetricsEventBatch { events };
    let body = serde_json::to_string(&batch).unwrap_or_default();
    let request_id = uuid::Uuid::new_v4().to_string();
    for attempt in 1..=3 {
        if body.len() as u64 > METRICS_LOG_LIMIT_BYTES {
            metrics_log(&format!(
                "[lifecycle-metrics] request requestId={request_id} attempt={attempt} url={endpoint} actualBytes={} payload-too-large",
                body.len()
            ));
        } else {
            metrics_log(&format!(
                "[lifecycle-metrics] request requestId={request_id} attempt={attempt} url={endpoint} body={body}"
            ));
        }
        let result = client
            .post(endpoint)
            .header("X-Maling-Report-Key", api_key)
            .header("Content-Type", "application/json;charset=UTF-8")
            .body(body.clone())
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        match result {
            Ok(response) => {
                let status = response.status();
                let response_body = response.text().await.unwrap_or_default();
                metrics_log(&format!(
                    "[lifecycle-metrics] response requestId={request_id} status={status} body={response_body}"
                ));
                if !should_retry_status(status) {
                    return;
                }
            }
            Err(error) => metrics_log(&format!(
                "[lifecycle-metrics] error requestId={request_id} attempt={attempt} error={error}"
            )),
        }
    }
}

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
}

fn metrics_collection_enabled(
    channel: &str,
    enabled: bool,
    endpoint: Option<&str>,
    api_key: Option<&str>,
) -> bool {
    channel == "wb"
        && enabled
        && endpoint.is_some_and(|value| !value.trim().is_empty())
        && api_key.is_some_and(|value| !value.trim().is_empty())
}

pub(crate) fn core_metrics_collection_enabled(config: &RuntimeConfig) -> bool {
    let settings = metrics_settings(config);
    metrics_collection_enabled(
        current_channel_config().channel,
        settings.enabled,
        settings.node_metrics_endpoint.as_deref(),
        get_api_key(config).as_deref(),
    )
}

async fn run_metrics_reporter(
    mut receiver: tokio::sync::mpsc::Receiver<MetricsEventItem>,
    endpoint: String,
    api_key: String,
) {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    let mut pending = Vec::with_capacity(METRICS_BATCH_LIMIT);
    let mut shutdown_deadline = None;
    loop {
        if receiver.is_closed() && shutdown_deadline.is_none() {
            shutdown_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(500));
        }
        if pending.is_empty() && shutdown_deadline.is_none() {
            let deadline = tokio::time::sleep(Duration::from_secs(2));
            tokio::pin!(deadline);
            tokio::select! {
                value = receiver.recv() => match value { Some(event) => pending.push(event), None => continue },
                _ = &mut deadline => {}
            }
        }
        while pending.len() < METRICS_BATCH_LIMIT
            && !shutdown_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
        {
            match receiver.try_recv() {
                Ok(event) => pending.push(event),
                Err(_) => break,
            }
        }
        if pending.is_empty() {
            if shutdown_deadline.is_some() {
                break;
            }
            continue;
        }
        let batch = take_next_batch(&mut pending);
        if let Some(deadline) = shutdown_deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let _ = tokio::time::timeout(
                remaining,
                send_metrics_batch(&client, &endpoint, &api_key, batch),
            )
            .await;
        } else {
            send_metrics_batch(&client, &endpoint, &api_key, batch).await;
        }
    }
}

fn take_next_batch(pending: &mut Vec<MetricsEventItem>) -> Vec<MetricsEventItem> {
    let month = pending[0].reported_at.clone();
    let split = pending
        .iter()
        .position(|event| !same_report_month(&month, &event.reported_at))
        .unwrap_or(pending.len())
        .min(METRICS_BATCH_LIMIT);
    pending.drain(..split).collect()
}

pub fn create_metrics_subscriber<R: Runtime>(
    app: AppHandle<R>,
) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
    let (sender, receiver) = tokio::sync::mpsc::channel(METRICS_QUEUE_CAPACITY);
    let gate = app
        .try_state::<DesktopState>()
        .and_then(|state| state.context().ok())
        .and_then(|context| {
            let settings = metrics_settings(&context.config);
            metrics_collection_enabled(
                current_channel_config().channel,
                settings.enabled,
                settings.node_metrics_endpoint.as_deref(),
                get_api_key(&context.config).as_deref(),
            )
            .then(|| {
                Some((
                    settings.node_metrics_endpoint?,
                    get_api_key(&context.config)?,
                ))
            })
            .flatten()
        });
    if let Some((endpoint, api_key)) = gate {
        tauri::async_runtime::spawn(run_metrics_reporter(receiver, endpoint, api_key));
    } else {
        drop(receiver);
        metrics_log("[lifecycle-metrics] disabled: requires wb channel, endpoint and api key");
    }
    Arc::new(move |event| {
        let RuntimeLifecycleEvent::MetricsFact(fact) = &event else {
            return;
        };
        if let Err(error) = fact.validate() {
            metrics_log(&format!(
                "[lifecycle-metrics] dropped invalid event error={error}"
            ));
            return;
        }
        let Some(item) = map_runtime_event(event) else {
            return;
        };
        if let Err(error) = sender.try_send(item) {
            let reason = error.to_string();
            let item = error.into_inner();
            metrics_log(&format!(
                "[lifecycle-metrics] dropped event eventId={} executionId={} reason={reason}",
                item.event_id, item.execution_id,
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        METRICS_BATCH_LIMIT, MetricsEventType, MetricsExecutionKind, MetricsSessionMode,
        map_metrics_fact, metrics_collection_enabled, metrics_settings, normalize_metrics_base_url,
        run_metrics_reporter, same_report_month, should_retry_status, take_next_batch,
    };
    use gold_band::config::RuntimeConfig;

    #[test]
    fn normalizes_metrics_base_url_from_service_root_or_known_endpoint() {
        assert_eq!(
            normalize_metrics_base_url(" http://maling.weoa.com/ ").as_deref(),
            Some("http://maling.weoa.com")
        );
        assert_eq!(
            normalize_metrics_base_url("http://maling.weoa.com/api/client-report/heartbeat")
                .as_deref(),
            Some("http://maling.weoa.com")
        );
        assert_eq!(
            normalize_metrics_base_url("http://maling.weoa.com/api/client-report/metrics/batch")
                .as_deref(),
            Some("http://maling.weoa.com")
        );
        assert_eq!(normalize_metrics_base_url("ftp://maling.weoa.com"), None);
    }

    #[test]
    fn metrics_settings_derives_fixed_endpoints_from_base_url() {
        let mut config = RuntimeConfig::default();
        config.desktop_metrics_enabled = true;
        config.desktop_metrics_base_url =
            Some("http://metrics.example.com/api/client-report/metrics/batch".to_string());

        let settings = metrics_settings(&config);

        assert!(settings.enabled);
        assert_eq!(
            settings.metrics_base_url.as_deref(),
            Some("http://metrics.example.com")
        );
        assert_eq!(
            settings.heartbeat_endpoint.as_deref(),
            Some("http://metrics.example.com/api/client-report/heartbeat")
        );
        assert_eq!(
            settings.node_metrics_endpoint.as_deref(),
            Some("http://metrics.example.com/api/client-report/metrics/batch")
        );
    }

    #[test]
    fn lifecycle_contract_enums_use_protocol_values() {
        assert_eq!(
            serde_json::to_string(&MetricsEventType::ExecutionCompleted).unwrap(),
            "\"execution.completed\""
        );
        assert_eq!(
            serde_json::to_string(&MetricsSessionMode::Workflow).unwrap(),
            "\"workflow\""
        );
        assert_eq!(
            serde_json::to_string(&MetricsExecutionKind::NodeAttempt).unwrap(),
            "\"node-attempt\""
        );
    }

    #[test]
    fn reporter_groups_by_frozen_report_month_and_caps_batches() {
        assert!(same_report_month(
            "2026-07-31T23:59:59+08:00",
            "2026-07-01T00:00:00+08:00"
        ));
        assert!(!same_report_month(
            "2026-07-31T23:59:59+08:00",
            "2026-08-01T00:00:00+08:00"
        ));
        assert_eq!(METRICS_BATCH_LIMIT, 100);
    }

    #[test]
    fn wb_gate_requires_all_credentials_and_default_never_collects() {
        assert!(metrics_collection_enabled(
            "wb",
            true,
            Some("https://metrics.example"),
            Some("key")
        ));
        assert!(!metrics_collection_enabled(
            "default",
            true,
            Some("https://metrics.example"),
            Some("key")
        ));
        assert!(!metrics_collection_enabled("wb", true, None, Some("key")));
        assert!(!metrics_collection_enabled(
            "wb",
            true,
            Some("https://metrics.example"),
            None
        ));
    }

    #[test]
    fn retry_policy_retries_only_server_failures() {
        assert!(should_retry_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(should_retry_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(!should_retry_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(!should_retry_status(reqwest::StatusCode::OK));
    }

    #[tokio::test]
    async fn bounded_queue_drops_new_event_without_waiting() {
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);
        sender.try_send(1).unwrap();
        assert!(matches!(
            sender.try_send(2),
            Err(tokio::sync::mpsc::error::TrySendError::Full(2))
        ));
    }

    #[test]
    fn every_shutdown_batch_respects_the_hundred_event_limit() {
        let mut pending = (0..205)
            .map(|_| {
                let fact = gold_band::app::observability::MetricsLifecycleFact::new(
                    gold_band::app::observability::LifecycleEventType::ExecutionStarted,
                    1,
                    "2026-08-01T00:00:00Z".into(),
                    "user".into(),
                    "workspace".into(),
                    gold_band::app::observability::MetricsSessionMode::Workflow,
                    "task".into(),
                    gold_band::app::observability::ExecutionKind::Run,
                    uuid::Uuid::new_v4().to_string(),
                );
                map_metrics_fact(fact, "2026-08-01T00:00:00Z".into())
            })
            .collect::<Vec<_>>();
        assert_eq!(take_next_batch(&mut pending).len(), 100);
        assert_eq!(take_next_batch(&mut pending).len(), 100);
        assert_eq!(take_next_batch(&mut pending).len(), 5);
    }

    #[test]
    fn dto_uses_failed_attempt_id_for_delivery_failure() {
        let mut fact = gold_band::app::observability::MetricsLifecycleFact::new(
            gold_band::app::observability::LifecycleEventType::ExecutionCompleted,
            2,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            gold_band::app::observability::MetricsSessionMode::Auto,
            "task".into(),
            gold_band::app::observability::ExecutionKind::OuterRun,
            uuid::Uuid::new_v4().to_string(),
        );
        fact.outcome = Some(gold_band::app::observability::ExecutionOutcome::Failure);
        fact.terminal_reason =
            Some(gold_band::app::observability::TerminalReason::AcceptanceRejected);
        fact.failed_attempt_id = Some(uuid::Uuid::new_v4().to_string());
        fact.counters = Some(Default::default());
        let value =
            serde_json::to_value(map_metrics_fact(fact, "2026-08-01T00:00:00Z".into())).unwrap();
        assert!(value.get("failedAttemptId").is_some());
        assert!(value.get("failedExecutionId").is_none());
    }

    #[test]
    fn dto_serializes_attempt_index_only_for_attempt_events() {
        let execution_id = uuid::Uuid::new_v4().to_string();
        let mut fact = gold_band::app::observability::MetricsLifecycleFact::new(
            gold_band::app::observability::LifecycleEventType::ExecutionStarted,
            1,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            gold_band::app::observability::MetricsSessionMode::Direct,
            "task".into(),
            gold_band::app::observability::ExecutionKind::Turn,
            execution_id.clone(),
        );
        fact.attempt_id = Some(execution_id);
        fact.attempt_index = Some(1);
        let value =
            serde_json::to_value(map_metrics_fact(fact, "2026-08-01T00:00:00Z".into())).unwrap();
        assert_eq!(value.get("attemptIndex").and_then(|v| v.as_u64()), Some(1));
    }

    #[tokio::test]
    async fn reporter_shutdown_is_bounded_to_five_hundred_milliseconds() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
        let fact = gold_band::app::observability::MetricsLifecycleFact::new(
            gold_band::app::observability::LifecycleEventType::ExecutionStarted,
            1,
            "2026-08-01T00:00:00Z".into(),
            "user".into(),
            "workspace".into(),
            gold_band::app::observability::MetricsSessionMode::Workflow,
            "task".into(),
            gold_band::app::observability::ExecutionKind::Run,
            uuid::Uuid::new_v4().to_string(),
        );
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        sender
            .send(map_metrics_fact(fact, "2026-08-01T00:00:00Z".into()))
            .await
            .unwrap();
        drop(sender);
        let started = std::time::Instant::now();
        tokio::time::timeout(
            std::time::Duration::from_millis(900),
            run_metrics_reporter(receiver, format!("http://{address}"), "key".into()),
        )
        .await
        .expect("reporter must honor the shutdown budget");
        assert!(started.elapsed() < std::time::Duration::from_millis(900));
    }
}
