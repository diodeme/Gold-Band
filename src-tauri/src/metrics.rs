use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use gold_band::app::{MetricsEvent, MetricsEventContext};
use gold_band::config::RuntimeConfig;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::{channel::current_channel_config, state::DesktopState};

/// Write a metrics log line to the application data directory.
/// On Windows this is `%LOCALAPPDATA%\{app_key}\metrics.log`.
fn metrics_log(msg: &str) {
    eprintln!("{}", msg);
    // Also write to file in app data dir
    let config = current_channel_config();
    let app_key = config.app_key;
    let log_dir = if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        format!("{}\\{}", local_app_data, app_key)
    } else if let Ok(home) = std::env::var("USERPROFILE") {
        format!("{}\\.{}", home, app_key)
    } else {
        return;
    };
    // Best-effort: create dir and append to metrics.log
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("[metrics] failed to create log dir {}: {}", log_dir, e);
        return;
    }
    let log_path = format!("{}\\metrics.log", log_dir);
    let line = format!("[{}] {}\n", chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"), msg);
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
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
        let local = dt.with_timezone(&chrono::Local);
        local.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        ts.to_string()
    }
}

// ── Settings VM ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSettingsVm {
    pub enabled: bool,
    pub toggle_locked: bool,
    pub heartbeat_endpoint: Option<String>,
    pub node_metrics_endpoint: Option<String>,
    pub api_key_set: bool,         // true if api_key is non-empty (never expose the key itself)
}

pub fn metrics_settings(config: &RuntimeConfig) -> MetricsSettingsVm {
    let channel_config = current_channel_config();
    eprintln!(
        "[metrics] channel raw: ch_enabled={} ch_locked={} ch_heartbeat={} ch_node_metrics={} ch_apikey_empty={}",
        channel_config.metrics_enabled,
        channel_config.metrics_toggle_locked,
        channel_config.heartbeat_endpoint,
        channel_config.node_metrics_endpoint,
        channel_config.metrics_api_key.is_empty(),
    );
    let enabled = config.desktop_metrics_enabled || channel_config.metrics_enabled;
    let heartbeat_endpoint = config
        .desktop_heartbeat_endpoint
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let ep = channel_config.heartbeat_endpoint;
            if ep.is_empty() { None } else { Some(ep.to_string()) }
        });
    let node_metrics_endpoint = config
        .desktop_node_metrics_endpoint
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let ep = channel_config.node_metrics_endpoint;
            if ep.is_empty() { None } else { Some(ep.to_string()) }
        });
    let api_key = config
        .desktop_metrics_api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let k = channel_config.metrics_api_key;
            if k.is_empty() { None } else { Some(k.to_string()) }
        });
    MetricsSettingsVm {
        enabled: enabled && heartbeat_endpoint.is_some(),
        toggle_locked: channel_config.metrics_toggle_locked,
        heartbeat_endpoint,
        node_metrics_endpoint,
        api_key_set: api_key.is_some(),
    }
}

// ── Heartbeat ────────────────────────────────────────────────────────────────

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

fn get_system_username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string())
}

const HEARTBEAT_INTERVAL_SECS: u64 = 300; // 5 minutes

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
                                metrics_log(&format!("[heartbeat] timer fired, sending to {}", endpoint));
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

fn get_api_key(config: &RuntimeConfig) -> Option<String> {
    let channel_config = current_channel_config();
    config
        .desktop_metrics_api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let k = channel_config.metrics_api_key;
            if k.is_empty() { None } else { Some(k.to_string()) }
        })
}

// ── Node Metrics ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetricItem {
    pub workspace: String,
    pub user_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub attempt_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeMetricBatch {
    metrics: Vec<NodeMetricItem>,
}

pub async fn send_node_metrics_batch(endpoint: &str, api_key: &str, batch: NodeMetricBatch) {
    let body = serde_json::to_string(&batch).unwrap_or_default();
    metrics_log(&format!("[node-metrics] POST {} body: {}", endpoint, body));
    let client = reqwest::Client::new();
    let result = client
        .post(endpoint)
        .header("X-Maling-Report-Key", api_key)
        .header("Content-Type", "application/json;charset=UTF-8")
        .json(&batch)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    match result {
        Ok(resp) => {
            metrics_log(&format!("[node-metrics] response status={} count={}", resp.status(), batch.metrics.len()));
        }
        Err(err) => {
            metrics_log(&format!("[node-metrics] FAILED (ignored): {}", err));
        }
    }
}

/// Build a "start sentinel" node metric item for the first node in a run (no predecessor).
pub fn start_sentinel_metric(
    workspace: &str,
    user_id: &str,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    started_at: &str,
    reported_at: &str,
) -> NodeMetricItem {
    let sentinel_uuid = uuid::Uuid::new_v4().simple().to_string();
    NodeMetricItem {
        workspace: workspace.to_string(),
        user_id: user_id.to_string(),
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: sentinel_uuid,
        seq: None,
        node_name: Some("开始".to_string()),
        agent_type: None,
        attempt_count: 0,
        started_at: Some(started_at.to_string()),
        ended_at: Some(started_at.to_string()),
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: 0,
        status: "SUCCESS".to_string(),
        reported_at: Some(reported_at.to_string()),
    }
}

// ── Orchestrator Callback ────────────────────────────────────────────────────

/// Create a metrics callback that can be passed to `App::with_metrics_callback`.
/// This bridges the library-crate orchestrator events to the src-tauri metrics module.
pub fn create_metrics_callback<R: Runtime>(app: AppHandle<R>) -> Arc<dyn Fn(MetricsEventContext, MetricsEvent) + Send + Sync> {
    Arc::new(move |ctx: MetricsEventContext, event: MetricsEvent| {
        match event {
            MetricsEvent::NodeStarted { predecessor } => {
                metrics_log(&format!("[node-metrics] NodeStarted task={} run={} node={} has_predecessor={}",
                    ctx.task_id, ctx.run_id, ctx.node_id, predecessor.is_some()));
                let settings = if let Some(state) = app.try_state::<DesktopState>() {
                    if let Ok(desktop_ctx) = state.context() {
                        metrics_settings(&desktop_ctx.config)
                    } else {
                        return;
                    }
                } else {
                    return;
                };

                if !settings.enabled {
                    return;
                }
                let node_metrics_endpoint = match &settings.node_metrics_endpoint {
                    Some(ep) => ep.clone(),
                    None => return,
                };
                let api_key = if let Some(state) = app.try_state::<DesktopState>() {
                    if let Ok(desktop_ctx) = state.context() {
                        match get_api_key(&desktop_ctx.config) {
                            Some(k) => k,
                            None => return,
                        }
                    } else {
                        return;
                    }
                } else {
                    return;
                };

                let user_id = get_system_username();
                let reported_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

                // Determine reentrancy: parse attempt number from attempt_id
                let attempt_count = ctx.attempt_id
                    .strip_prefix("attempt-")
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(0)
                    .saturating_sub(1); // attempt-001 → 0, attempt-002 → 1

                let node_status = if attempt_count > 0 {
                    "Reentrancy".to_string()
                } else {
                    "RUNNING".to_string()
                };

                // Build current node metric (use UUIDs for IDs)
                let current = NodeMetricItem {
                    workspace: ctx.repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: ctx.task_uuid.clone().unwrap_or(ctx.task_id.clone()),
                    run_id: ctx.run_uuid.clone().unwrap_or(ctx.run_id.clone()),
                    round_id: ctx.round_uuid.clone().unwrap_or(ctx.round_id.clone()),
                    node_id: ctx.node_uuid.clone().unwrap_or(ctx.node_id.clone()),
                    seq: ctx.seq,
                    node_name: ctx.node_name.clone(),
                    agent_type: ctx.agent_type.clone(),
                    attempt_count,
                    started_at: Some(to_iso8601(&ctx.started_at)),
                    ended_at: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    status: node_status,
                    reported_at: Some(reported_at.clone()),
                };

                // Build predecessor metric
                let predecessor_item = match &predecessor {
                    Some(pred) => NodeMetricItem {
                        workspace: ctx.repo_root.clone(),
                        user_id: user_id.clone(),
                        task_id: ctx.task_uuid.clone().unwrap_or(ctx.task_id.clone()),
                        run_id: ctx.run_uuid.clone().unwrap_or(ctx.run_id.clone()),
                        round_id: String::new(),
                        node_id: pred.uuid.clone(),
                        seq: None,
                        node_name: Some(pred.node_name.clone()),
                        agent_type: None,
                        attempt_count: 0,
                        started_at: Some(to_iso8601(&pred.started_at)),
                        ended_at: pred.finished_at.as_ref().map(|s| to_iso8601(s)),
                        input_tokens: pred.input_tokens,
                        output_tokens: pred.output_tokens,
                        cache_read_tokens: pred.cache_read_tokens,
                        total_tokens: pred.total_tokens,
                        status: pred.status.clone(),
                        reported_at: Some(reported_at.clone()),
                    },
                    None => start_sentinel_metric(
                        &ctx.repo_root,
                        &user_id,
                        &current.task_id,
                        &current.run_id,
                        &current.round_id,
                        &to_iso8601(&ctx.started_at),
                        &reported_at,
                    ),
                };

                let batch = NodeMetricBatch {
                    metrics: vec![predecessor_item, current],
                };

                metrics_log(&format!("[node-metrics] sending batch to {} (pred_status={}, cur_status={})",
                    node_metrics_endpoint,
                    batch.metrics.first().map(|m| m.status.as_str()).unwrap_or("?"),
                    batch.metrics.get(1).map(|m| m.status.as_str()).unwrap_or("?")));
                // Fire-and-forget: spawn async task
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    send_node_metrics_batch(&node_metrics_endpoint, &api_key, batch).await;
                    let _ = app_handle; // keep handle alive
                });
            }
            MetricsEvent::NodeCompleted => {
                // Workflow ended — send last completed node + end sentinel.
                metrics_log(&format!("[node-metrics] WorkflowEnded: task={} run={} node={}",
                    ctx.task_id, ctx.run_id, ctx.node_id));
                let settings = if let Some(state) = app.try_state::<DesktopState>() {
                    if let Ok(desktop_ctx) = state.context() {
                        metrics_settings(&desktop_ctx.config)
                    } else { return; }
                } else { return; };

                if !settings.enabled { return; }
                let node_metrics_endpoint = match &settings.node_metrics_endpoint {
                    Some(ep) => ep.clone(),
                    None => return,
                };
                let api_key = if let Some(state) = app.try_state::<DesktopState>() {
                    if let Ok(desktop_ctx) = state.context() {
                        match get_api_key(&desktop_ctx.config) {
                            Some(k) => k, None => return,
                        }
                    } else { return; }
                } else { return; };

                let user_id = get_system_username();
                let reported_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

                // Last completed node
                let last_node = NodeMetricItem {
                    workspace: ctx.repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: ctx.task_uuid.clone().unwrap_or(ctx.task_id.clone()),
                    run_id: ctx.run_uuid.clone().unwrap_or(ctx.run_id.clone()),
                    round_id: ctx.round_uuid.clone().unwrap_or(ctx.round_id.clone()),
                    node_id: ctx.node_uuid.clone().unwrap_or(ctx.node_id.clone()),
                    seq: ctx.seq,
                    node_name: ctx.node_name.clone(),
                    agent_type: ctx.agent_type.clone(),
                    attempt_count: 0,
                    started_at: Some(to_iso8601(&ctx.started_at)),
                    ended_at: ctx.finished_at.as_ref().map(|s| to_iso8601(s)),
                    input_tokens: ctx.input_tokens,
                    output_tokens: ctx.output_tokens,
                    cache_read_tokens: ctx.cache_read_tokens,
                    total_tokens: ctx.total_tokens,
                    status: "SUCCESS".to_string(),
                    reported_at: Some(reported_at.clone()),
                };
                let end_started = ctx.finished_at.as_ref()
                    .map(|s| to_iso8601(s))
                    .unwrap_or_else(|| reported_at.clone());
                // End sentinel
                let end_sentinel = NodeMetricItem {
                    workspace: ctx.repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: ctx.task_uuid.clone().unwrap_or(ctx.task_id.clone()),
                    run_id: ctx.run_uuid.clone().unwrap_or(ctx.run_id.clone()),
                    round_id: ctx.round_uuid.clone().unwrap_or(ctx.round_id.clone()),
                    node_id: uuid::Uuid::new_v4().simple().to_string(),
                    seq: None,
                    node_name: Some("结束".to_string()),
                    agent_type: None,
                    attempt_count: 0,
                    started_at: Some(end_started),
                    ended_at: Some(reported_at.clone()),
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    status: "SUCCESS".to_string(),
                    reported_at: Some(reported_at.clone()),
                };
                let batch = NodeMetricBatch { metrics: vec![last_node, end_sentinel] };
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    send_node_metrics_batch(&node_metrics_endpoint, &api_key, batch).await;
                    let _ = app_handle;
                });
            }
        }
    })
}