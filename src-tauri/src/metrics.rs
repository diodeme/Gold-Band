use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use camino::Utf8PathBuf;
use gold_band::app::WorkflowEvent;
use gold_band::config::RuntimeConfig;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use url::Url;

use crate::{channel::current_channel_config, state::DesktopState};

/// Cached log path — resolved once, avoids env-var lookup + create_dir_all on every log line.
static METRICS_LOG_PATH: OnceLock<Option<String>> = OnceLock::new();
const HEARTBEAT_ENDPOINT_PATH: &str = "/api/client-report/heartbeat";
const NODE_METRICS_ENDPOINT_PATH: &str = "/api/client-report/metrics/batch";

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
fn metrics_log(msg: &str) {
    eprintln!("{}", msg);
    let Some(log_path) = metrics_log_path() else {
        return;
    };
    let line = format!(
        "[{}] {}\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
        msg
    );
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

fn metrics_base_url(config: &RuntimeConfig) -> Option<String> {
    let channel_config = current_channel_config();
    config
        .desktop_metrics_base_url
        .as_deref()
        .and_then(normalize_metrics_base_url)
        .or_else(|| normalize_metrics_base_url(channel_config.metrics_base_url))
}

fn endpoint_from_base_url(base_url: &str, path: &str) -> Option<String> {
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

fn get_api_key(config: &RuntimeConfig) -> Option<String> {
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
            metrics_log(&format!(
                "[node-metrics] response status={} count={}",
                resp.status(),
                batch.metrics.len()
            ));
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

/// Create a metrics subscriber for the ObservabilityBus.
/// Replaces the old `create_metrics_callback`.
///
/// The subscriber is called **synchronously** by the bus, so it does the
/// minimum work needed (settings lookup, token read, metric construction)
/// and then delegates the HTTP request to `tauri::async_runtime::spawn`.
pub fn create_metrics_subscriber<R: Runtime>(
    app: AppHandle<R>,
) -> Arc<dyn Fn(WorkflowEvent) + Send + Sync> {
    Arc::new(move |event: WorkflowEvent| {
        // ── Guard: settings check (shared by both branches) ──
        let settings = match app.try_state::<DesktopState>() {
            Some(state) => match state.context() {
                Ok(ctx) => metrics_settings(&ctx.config),
                Err(_) => return,
            },
            None => return,
        };
        if !settings.enabled {
            return;
        }
        let node_metrics_endpoint = match &settings.node_metrics_endpoint {
            Some(ep) => ep.clone(),
            None => return,
        };
        let api_key = match app.try_state::<DesktopState>() {
            Some(state) => match state.context() {
                Ok(ctx) => match get_api_key(&ctx.config) {
                    Some(k) => k,
                    None => return,
                },
                Err(_) => return,
            },
            None => return,
        };

        let user_id = get_system_username();
        let reported_at = chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();

        match event {
            WorkflowEvent::NodeStarted {
                repo_root,
                task_id,
                task_uuid,
                run_id,
                run_uuid,
                round_id,
                round_uuid,
                node_id,
                node_uuid,
                attempt_id,
                seq,
                node_name,
                agent_type,
                started_at,
                predecessor,
                ..
            } => {
                metrics_log(&format!(
                    "[node-metrics] NodeStarted task={} run={} node={} has_predecessor={}",
                    task_id,
                    run_id,
                    node_id,
                    predecessor.is_some()
                ));

                let attempt_count = attempt_id
                    .strip_prefix("attempt-")
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(0)
                    .saturating_sub(1);

                let node_status = if attempt_count > 0 {
                    "Reentrancy".to_string()
                } else {
                    "RUNNING".to_string()
                };

                // ── Build predecessor metric ──
                let predecessor_item = match &predecessor {
                    Some(pred) => {
                        // Read predecessor tokens from ITS attempt_dir
                        let (input_tokens, output_tokens, cache_read_tokens, total_tokens) =
                            pred.attempt_dir.as_ref().map(|d| {
                                let path =
                                    Utf8PathBuf::from(d).join("acp.session.json");
                                gold_band::acp::events::read_session_tokens(&path)
                            }).unwrap_or((0, 0, 0, 0));

                        NodeMetricItem {
                            workspace: repo_root.clone(),
                            user_id: user_id.clone(),
                            task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                            run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                            round_id: pred.round_uuid.clone(),
                            node_id: pred.uuid.clone(),
                            seq: pred.seq,
                            node_name: Some(pred.node_name.clone()),
                            agent_type: pred.agent_type.clone(),
                            attempt_count: 0,
                            started_at: Some(to_iso8601(&pred.started_at)),
                            ended_at: pred
                                .finished_at
                                .as_ref()
                                .map(|s| to_iso8601(s)),
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            total_tokens,
                            status: pred.status.clone(),
                            reported_at: Some(reported_at.clone()),
                        }
                    }
                    None => start_sentinel_metric(
                        &repo_root,
                        &user_id,
                        &task_uuid.clone().unwrap_or(task_id.clone()),
                        &run_uuid.clone().unwrap_or(run_id.clone()),
                        &round_uuid.clone().unwrap_or(round_id.clone()),
                        &to_iso8601(&started_at),
                        &reported_at,
                    ),
                };

                // ── Build current node metric (token=0 — hasn't executed yet) ──
                let current = NodeMetricItem {
                    workspace: repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                    run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                    round_id: round_uuid.clone().unwrap_or(round_id.clone()),
                    node_id: node_uuid.clone().unwrap_or(node_id.clone()),
                    seq,
                    node_name: node_name.clone(),
                    agent_type: agent_type.clone(),
                    attempt_count,
                    started_at: Some(to_iso8601(&started_at)),
                    ended_at: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    status: node_status,
                    reported_at: Some(reported_at.clone()),
                };

                let batch = NodeMetricBatch {
                    metrics: vec![predecessor_item, current],
                };

                metrics_log(&format!(
                    "[node-metrics] sending batch to {} (pred_status={}, cur_status={})",
                    node_metrics_endpoint,
                    batch
                        .metrics
                        .first()
                        .map(|m| m.status.as_str())
                        .unwrap_or("?"),
                    batch
                        .metrics
                        .get(1)
                        .map(|m| m.status.as_str())
                        .unwrap_or("?")
                ));

                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    send_node_metrics_batch(&node_metrics_endpoint, &api_key, batch).await;
                    let _ = app_handle;
                });
            }

            WorkflowEvent::NodeCompleted {
                repo_root,
                task_id,
                task_uuid,
                run_id,
                run_uuid,
                round_id,
                round_uuid,
                node_id,
                node_uuid,
                seq,
                node_name,
                agent_type,
                started_at,
                finished_at,
                outcome,
                attempt_dir,
                suppress_sentinel,
                ..
            } => {
                metrics_log(&format!(
                    "[node-metrics] WorkflowEnded: task={} run={} node={}",
                    task_id, run_id, node_id
                ));

                // Read tokens from this node's attempt_dir
                let path = Utf8PathBuf::from(&attempt_dir).join("acp.session.json");
                let (input_tokens, output_tokens, cache_read_tokens, total_tokens) =
                    gold_band::acp::events::read_session_tokens(&path);

                let last_node = NodeMetricItem {
                    workspace: repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                    run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                    round_id: round_uuid.clone().unwrap_or(round_id.clone()),
                    node_id: node_uuid.clone().unwrap_or(node_id.clone()),
                    seq,
                    node_name: Some(node_name.clone()),
                    agent_type: agent_type.clone(),
                    attempt_count: 0,
                    started_at: Some(to_iso8601(&started_at)),
                    ended_at: finished_at.as_ref().map(|s| to_iso8601(s)),
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    total_tokens,
                    status: outcome.clone(),
                    reported_at: Some(reported_at.clone()),
                };

                let end_started = finished_at
                    .as_ref()
                    .map(|s| to_iso8601(s))
                    .unwrap_or_else(|| reported_at.clone());

                let end_sentinel = NodeMetricItem {
                    workspace: repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                    run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                    round_id: round_uuid.clone().unwrap_or(round_id.clone()),
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
                    status: outcome,
                    reported_at: Some(reported_at.clone()),
                };

                let mut metrics = vec![last_node];
                if !suppress_sentinel {
                    metrics.push(end_sentinel);
                }
                let batch = NodeMetricBatch { metrics };

                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    send_node_metrics_batch(&node_metrics_endpoint, &api_key, batch).await;
                    let _ = app_handle;
                });
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{metrics_settings, normalize_metrics_base_url};
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
}
