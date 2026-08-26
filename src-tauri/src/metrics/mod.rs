mod collector;
pub mod heartbeat;
pub mod identity;
mod uploader;

use std::io::Write;
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use gold_band::app::RuntimeLifecycleEvent;
use gold_band::app::observability::LifecycleTiming;
use gold_band::config::RuntimeConfig;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use url::Url;

use crate::{channel::current_channel_config, state::DesktopState};

static METRICS_LOG_PATH: OnceLock<Option<String>> = OnceLock::new();
pub(crate) const HEARTBEAT_ENDPOINT_PATH: &str = "/api/client-report/heartbeat";
const NODE_METRICS_ENDPOINT_PATH: &str = "/api/client-report/metrics/batch";
pub(super) const METRICS_BATCH_LIMIT: usize = 100;
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
            if let Err(error) = std::fs::create_dir_all(&log_dir) {
                eprintln!("[metrics] failed to create log dir {log_dir}: {error}");
                return None;
            }
            Some(format!("{log_dir}\\metrics.log"))
        })
        .as_deref()
}

pub(crate) fn metrics_log(message: &str) {
    eprintln!("{message}");
    let Some(log_path) = metrics_log_path() else {
        return;
    };
    let line = format!(
        "[{}] {message}\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
    );
    let line = if line.len() as u64 > METRICS_LOG_LIMIT_BYTES {
        format!(
            "[{}] payload-too-large actualBytes={}\n",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
            line.len()
        )
    } else {
        line
    };
    if let Ok(metadata) = std::fs::metadata(log_path)
        && metadata.len().saturating_add(line.len() as u64) > METRICS_LOG_LIMIT_BYTES
    {
        let reset = format!(
            "[{}] log-reset reason=size-limit\n",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f")
        );
        if let Err(error) = std::fs::write(log_path, reset) {
            eprintln!("[metrics] failed to reset log {log_path}: {error}");
        }
    }
    if let Err(error) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
    {
        eprintln!("[metrics] failed to write log {log_path}: {error}");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSettingsVm {
    pub enabled: bool,
    pub toggle_locked: bool,
    pub metrics_base_url: Option<String>,
    pub heartbeat_endpoint: Option<String>,
    pub node_metrics_endpoint: Option<String>,
    pub api_key_set: bool,
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
    Some(url.to_string().trim_end_matches('/').to_string())
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
    let enabled = config.desktop_metrics_enabled || channel_config.metrics_enabled;
    let metrics_base_url = metrics_base_url(config);
    let heartbeat_endpoint = metrics_base_url
        .as_deref()
        .and_then(|base_url| endpoint_from_base_url(base_url, HEARTBEAT_ENDPOINT_PATH));
    let node_metrics_endpoint = metrics_base_url
        .as_deref()
        .and_then(|base_url| endpoint_from_base_url(base_url, NODE_METRICS_ENDPOINT_PATH));
    MetricsSettingsVm {
        enabled: enabled && metrics_base_url.is_some(),
        toggle_locked: channel_config.metrics_toggle_locked,
        metrics_base_url,
        heartbeat_endpoint,
        node_metrics_endpoint,
        api_key_set: get_api_key(config).is_some(),
    }
}

pub(crate) fn heartbeat_settings(config: &RuntimeConfig) -> heartbeat::HeartbeatSettings {
    let vm = metrics_settings(config);
    let channel = current_channel_config();
    heartbeat::HeartbeatSettings {
        enabled: vm.enabled && heartbeat_channel_enabled(channel.channel),
        endpoint: vm.heartbeat_endpoint,
        api_key: get_api_key(config),
    }
}

fn heartbeat_channel_enabled(channel: &str) -> bool {
    channel == "wb"
}

pub(crate) fn get_api_key(config: &RuntimeConfig) -> Option<String> {
    let channel_config = current_channel_config();
    config
        .desktop_metrics_api_key
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            (!channel_config.metrics_api_key.is_empty())
                .then(|| channel_config.metrics_api_key.to_string())
        })
}

pub(crate) fn get_system_username() -> String {
    use identity::UserIdProvider;
    identity::WhoamiUserIdProvider
        .username()
        .unwrap_or_default()
}

fn iso_now() -> String {
    format_metrics_local_timestamp(Local::now())
}

fn format_metrics_local_timestamp(value: DateTime<Local>) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
}

fn normalize_metrics_timestamp(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(seconds) = value.strip_suffix('Z')
        && seconds.chars().all(|character| character.is_ascii_digit())
    {
        let seconds = seconds.parse::<i64>().ok()?;
        return DateTime::from_timestamp(seconds, 0)
            .map(|timestamp| format_metrics_local_timestamp(timestamp.with_timezone(&Local)));
    }
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Some(format_metrics_local_timestamp(
            timestamp.with_timezone(&Local),
        ));
    }
    let local = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    Local
        .from_local_datetime(&local)
        .earliest()
        .map(format_metrics_local_timestamp)
}

fn normalize_metrics_timing(timing: &mut Option<LifecycleTiming>) -> bool {
    let Some(current) = timing.as_ref() else {
        return true;
    };
    let Some(started_at) = normalize_metrics_timestamp(&current.started_at) else {
        *timing = None;
        return false;
    };
    let ended_at = match current.ended_at.as_deref() {
        Some(value) => {
            let Some(value) = normalize_metrics_timestamp(value) else {
                *timing = None;
                return false;
            };
            Some(value)
        }
        None => None,
    };
    *timing = Some(LifecycleTiming {
        started_at,
        ended_at,
        acp_session_elapsed_ms: current.acp_session_elapsed_ms,
    });
    true
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

pub fn create_metrics_subscriber<R: Runtime>(
    app: AppHandle<R>,
) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
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
        })
        .zip(
            app.path()
                .app_data_dir()
                .ok()
                .map(|dir| dir.join("metrics").join("metrics.sqlite3")),
        );
    let collector = gate.map(|((endpoint, api_key), database_path)| {
        uploader::start(database_path, endpoint, api_key)
    });
    if collector.is_none() {
        metrics_log("[lifecycle-metrics] disabled: requires wb channel, endpoint and api key");
    }
    Arc::new(move |event| {
        if let Some(reason) = heartbeat_reason_for_event(&event) {
            if let Some(state) = app.try_state::<DesktopState>() {
                let _ = state.record_heartbeat_reason(reason);
            }
            return;
        }
        let RuntimeLifecycleEvent::PendingMetricsFact(mut fact) = event else {
            return;
        };
        let Some(occurred_at) = normalize_metrics_timestamp(&fact.occurred_at) else {
            metrics_log("[lifecycle-metrics] dropped invalid event error=invalid-occurred-at");
            return;
        };
        fact.occurred_at = occurred_at;
        if !normalize_metrics_timing(&mut fact.payload.timing) {
            metrics_log(
                "[lifecycle-metrics] omitted invalid timing error=invalid-lifecycle-timing",
            );
        }
        if let Err(error) = fact.validate() {
            metrics_log(&format!(
                "[lifecycle-metrics] dropped invalid event error={error}"
            ));
            return;
        }
        let Some(sender) = collector.as_ref() else {
            return;
        };
        let execution_id = fact.key.execution_id.clone();
        let command = collector::CollectorCommand::Fact {
            fact,
            reported_at: iso_now(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            now_epoch_seconds: chrono::Utc::now().timestamp(),
        };
        if let Err(error) = sender.try_send(command) {
            let reason = match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => "queue-full",
                tokio::sync::mpsc::error::TrySendError::Closed(_) => "collector-closed",
            };
            metrics_log(&format!(
                "[lifecycle-metrics] dropped pending fact executionId={execution_id} reason={reason}"
            ));
        }
    })
}

fn heartbeat_reason_for_event(event: &RuntimeLifecycleEvent) -> Option<heartbeat::HeartbeatReason> {
    use gold_band::config::ConversationRunMode;
    use heartbeat::HeartbeatReason;

    match event {
        RuntimeLifecycleEvent::ApplicationStarted => Some(HeartbeatReason::AppStarted),
        RuntimeLifecycleEvent::UserActivityObserved => Some(HeartbeatReason::Activity),
        RuntimeLifecycleEvent::ConversationRunStarted { run_mode, .. } => Some(match run_mode {
            ConversationRunMode::Direct => HeartbeatReason::DirectStarted,
            ConversationRunMode::Workflow => HeartbeatReason::WorkflowStarted,
            ConversationRunMode::Auto => HeartbeatReason::AutoStarted,
        }),
        RuntimeLifecycleEvent::ScheduledTaskCreated { .. } => {
            Some(HeartbeatReason::ScheduledTaskCreated)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::config::ConversationRunMode;

    #[test]
    fn normalizes_service_root_and_known_metrics_endpoints() {
        assert_eq!(
            normalize_metrics_base_url(" http://metrics.example.com/ ").as_deref(),
            Some("http://metrics.example.com")
        );
        assert_eq!(
            normalize_metrics_base_url(
                "http://metrics.example.com/api/client-report/metrics/batch"
            )
            .as_deref(),
            Some("http://metrics.example.com")
        );
        assert_eq!(
            normalize_metrics_base_url("ftp://metrics.example.com"),
            None
        );
    }

    #[test]
    fn collection_gate_requires_wb_endpoint_and_api_key() {
        assert!(metrics_collection_enabled(
            "wb",
            true,
            Some("http://metrics.example.com"),
            Some("key")
        ));
        assert!(!metrics_collection_enabled(
            "stable",
            true,
            Some("http://metrics.example.com"),
            Some("key")
        ));
        assert!(!metrics_collection_enabled("wb", true, None, Some("key")));
    }

    #[test]
    fn top_level_runs_map_to_heartbeat_reasons() {
        let event = |run_mode| RuntimeLifecycleEvent::ConversationRunStarted {
            project_id: "project".to_string(),
            task_id: "task".to_string(),
            run_id: "run-001".to_string(),
            run_mode,
        };
        assert_eq!(
            heartbeat_reason_for_event(&event(ConversationRunMode::Direct)),
            Some(heartbeat::HeartbeatReason::DirectStarted)
        );
        assert_eq!(
            heartbeat_reason_for_event(&event(ConversationRunMode::Workflow)),
            Some(heartbeat::HeartbeatReason::WorkflowStarted)
        );
        assert_eq!(
            heartbeat_reason_for_event(&event(ConversationRunMode::Auto)),
            Some(heartbeat::HeartbeatReason::AutoStarted)
        );
    }

    #[test]
    fn metrics_timestamps_use_one_local_millisecond_wire_format() {
        let epoch = DateTime::from_timestamp(1_787_572_869, 0)
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(
            normalize_metrics_timestamp("1787572869Z").unwrap(),
            format_metrics_local_timestamp(epoch)
        );
        assert_eq!(
            normalize_metrics_timestamp("2026-08-24T20:01:09.306").unwrap(),
            "2026-08-24T20:01:09.306"
        );
        assert!(normalize_metrics_timestamp("not-a-time").is_none());
    }

    #[test]
    fn lifecycle_timing_uses_the_same_local_millisecond_wire_format() {
        let started = DateTime::from_timestamp(1_787_656_092, 0)
            .unwrap()
            .with_timezone(&Local);
        let ended = DateTime::from_timestamp(1_787_656_690, 0)
            .unwrap()
            .with_timezone(&Local);
        let mut timing = Some(LifecycleTiming {
            started_at: "1787656092Z".to_string(),
            ended_at: Some("1787656690Z".to_string()),
            acp_session_elapsed_ms: Some(70_000),
        });

        assert!(normalize_metrics_timing(&mut timing));
        let timing = timing.unwrap();
        assert_eq!(timing.started_at, format_metrics_local_timestamp(started));
        let expected_ended_at = format_metrics_local_timestamp(ended);
        assert_eq!(timing.ended_at.as_deref(), Some(expected_ended_at.as_str()));
        assert_eq!(timing.acp_session_elapsed_ms, Some(70_000));
    }

    #[test]
    fn invalid_lifecycle_timing_is_omitted_without_dropping_the_event() {
        let mut timing = Some(LifecycleTiming {
            started_at: "not-a-time".to_string(),
            ended_at: Some("2026-08-25T19:18:10.000".to_string()),
            acp_session_elapsed_ms: Some(1),
        });

        assert!(!normalize_metrics_timing(&mut timing));
        assert!(timing.is_none());
    }
}
