use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use camino::Utf8Path;
use serde::Serialize;
use tracing::warn;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

use crate::config::RuntimeConfig;
use crate::domain::{NodeType, PauseReason, RunStatus, VERSION};
use crate::inspect::render_run_status;
use crate::runtime::RunState;
use crate::storage::{GoldBandPaths, append_jsonl, ensure_parent_dir, write_json};

const PROGRESS_TARGET: &str = "gold_band.progress";
static TRACE_ID: OnceLock<String> = OnceLock::new();
static TRACING_INITIALIZED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionContext {
    pub trace_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: Option<String>,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
}

impl ExecutionContext {
    pub fn for_run(task_id: &str, run_id: &str) -> Self {
        Self {
            trace_id: trace_id(),
            task_id: task_id.to_string(),
            run_id: run_id.to_string(),
            round_id: None,
            node_id: None,
            attempt_id: None,
        }
    }

    pub fn with_round(mut self, round_id: impl Into<String>) -> Self {
        self.round_id = Some(round_id.into());
        self
    }

    pub fn with_node(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_attempt(mut self, attempt_id: impl Into<String>) -> Self {
        self.attempt_id = Some(attempt_id.into());
        self
    }

    pub fn execution_key(&self) -> Option<String> {
        Some(format!(
            "{}/{}/{}/{}/{}",
            self.task_id,
            self.run_id,
            self.round_id.as_deref()?,
            self.node_id.as_deref()?,
            self.attempt_id.as_deref()?
        ))
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressStage {
    Starting,
    CallingProvider,
    Streaming,
    NormalizingArtifact,
    RunningCommand,
    Paused,
    Blocked,
    Completed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunProgressSnapshot {
    pub version: String,
    pub status: RunStatus,
    pub current_round_id: Option<String>,
    pub current_node_id: Option<String>,
    pub current_node_type: Option<NodeType>,
    pub current_attempt_id: Option<String>,
    pub current_stage: ProgressStage,
    pub summary: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunEventEnvelope<T: Serialize> {
    pub version: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: String,
    pub data: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawStreamEnvelope<'a> {
    pub timestamp: &'a str,
    pub stream: &'a str,
    pub content: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttemptProgressEventEnvelope<T: Serialize> {
    pub version: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: String,
    pub data: T,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptProgressEventData {
    pub stream: Option<String>,
    pub session_id: Option<String>,
    pub attempt_id: Option<String>,
    pub message_id: Option<String>,
    pub tool_name: Option<String>,
    pub content: Option<String>,
    pub raw_event_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventData {
    pub trace_id: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: Option<String>,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
    pub execution_key: Option<String>,
    pub stage: Option<ProgressStage>,
    pub status: Option<RunStatus>,
    pub summary: Option<String>,
    pub pause_reason: Option<PauseReason>,
}

pub fn init_tracing(paths: &GoldBandPaths, config: &RuntimeConfig, enable_stderr_progress: bool) {
    let _ = TRACE_ID.get_or_init(trace_id_seed);
    cleanup_old_logs(paths, config.log_retention_days);
    if TRACING_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    let logs_dir = paths.logs_dir();
    if let Err(err) = fs::create_dir_all(logs_dir.as_std_path()) {
        eprintln!("gold-band: failed to create logs dir {logs_dir}: {err}");
        return;
    }

    let log_path = paths.runtime_log_file();
    let file_writer_path = log_path.clone();
    let stderr_writer = BoxMakeWriter::new(std::io::stderr);

    let progress_filter = EnvFilter::new(format!("{PROGRESS_TARGET}=info"));
    let debug_filter = EnvFilter::new(format!("gold_band={}", config.log_level.as_directive()));

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(move || {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(file_writer_path.as_std_path())
                .expect("open runtime log")
        })
        .with_target(true)
        .with_filter(debug_filter);

    let stderr_layer = fmt::layer()
        .compact()
        .with_target(false)
        .with_writer(stderr_writer)
        .with_filter(progress_filter);

    let registry = tracing_subscriber::registry().with(file_layer);
    if enable_stderr_progress {
        registry.with(stderr_layer).init();
    } else {
        registry.init();
    }
}

pub fn progress(run_summary: &str) {
    tracing::info!(target: PROGRESS_TARGET, "{}", render_run_status(run_summary));
}

pub fn write_run_progress_best_effort(
    paths: &GoldBandPaths,
    _task_id: &str,
    run: &RunState,
    node_type: Option<NodeType>,
    stage: ProgressStage,
    summary: impl Into<String>,
) {
    let snapshot = RunProgressSnapshot {
        version: VERSION.to_string(),
        status: run.status,
        current_round_id: run.current_round.clone(),
        current_node_id: run.current_node.clone(),
        current_node_type: node_type,
        current_attempt_id: run.current_attempt.clone(),
        current_stage: stage,
        summary: summary.into(),
        updated_at: run.updated_at.clone(),
    };
    let path = paths.run_progress_file(&run.task_id, &run.id);
    if let Err(err) = write_json(&path, &snapshot) {
        warn!(path = %path, error = %err, "failed to write run progress");
    }
}

pub fn append_run_event_best_effort(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    event_type: impl Into<String>,
    timestamp: impl Into<String>,
    data: RunEventData,
) {
    let envelope = RunEventEnvelope {
        version: VERSION.to_string(),
        event_type: event_type.into(),
        timestamp: timestamp.into(),
        data,
    };
    let path = paths.run_events_file(task_id, run_id);
    if let Err(err) = append_jsonl(&path, &envelope) {
        warn!(path = %path, error = %err, "failed to append run event");
    }
}

pub fn append_raw_stream_best_effort(
    path: &Utf8Path,
    timestamp: &str,
    stream: &str,
    content: &str,
) {
    let envelope = RawStreamEnvelope {
        timestamp,
        stream,
        content,
    };
    if let Err(err) = append_jsonl(path, &envelope) {
        warn!(path = %path, error = %err, "failed to append raw stream envelope");
    }
}

pub fn append_progress_event_best_effort(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    event_type: impl Into<String>,
    timestamp: impl Into<String>,
    data: AttemptProgressEventData,
) {
    let envelope = AttemptProgressEventEnvelope {
        version: VERSION.to_string(),
        event_type: event_type.into(),
        timestamp: timestamp.into(),
        data,
    };
    let path = paths.progress_events_file(task_id, run_id, round_id, node_id, attempt_id);
    if let Err(err) = append_jsonl(&path, &envelope) {
        warn!(path = %path, error = %err, "failed to append attempt progress event");
    }
}

pub fn run_event_data(
    ctx: &ExecutionContext,
    stage: Option<ProgressStage>,
    status: Option<RunStatus>,
    summary: Option<String>,
    pause_reason: Option<PauseReason>,
) -> RunEventData {
    RunEventData {
        trace_id: ctx.trace_id.clone(),
        task_id: ctx.task_id.clone(),
        run_id: ctx.run_id.clone(),
        round_id: ctx.round_id.clone(),
        node_id: ctx.node_id.clone(),
        attempt_id: ctx.attempt_id.clone(),
        execution_key: ctx.execution_key(),
        stage,
        status,
        summary,
        pause_reason,
    }
}

fn cleanup_old_logs(paths: &GoldBandPaths, retention_days: u64) {
    let Ok(entries) = fs::read_dir(paths.logs_dir().as_std_path()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let max_age = std::time::Duration::from_secs(retention_days * 24 * 60 * 60);
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age > max_age {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn trace_id() -> String {
    TRACE_ID.get_or_init(trace_id_seed).clone()
}

fn trace_id_seed() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("trace-{millis}")
}

pub fn write_progress_hint(
    paths: &GoldBandPaths,
    task_id: &str,
    run_id: &str,
    node_raw_stream: Option<&Utf8Path>,
) {
    progress(&format!(
        "progress file: {}",
        paths.run_progress_file(task_id, run_id)
    ));
    progress(&format!(
        "events file: {}",
        paths.run_events_file(task_id, run_id)
    ));
    if let Some(raw_stream) = node_raw_stream {
        progress(&format!("raw stream: {raw_stream}"));
    }
}

pub fn touch_log_file_best_effort(paths: &GoldBandPaths) {
    let path = paths.runtime_log_file();
    if let Err(err) = ensure_parent_dir(&path) {
        warn!(path = %path, error = %err, "failed to prepare runtime log path");
        return;
    }
    if let Err(err) = File::options()
        .create(true)
        .append(true)
        .open(path.as_std_path())
        .and_then(|mut file| file.write_all(b""))
    {
        warn!(path = %path, error = %err, "failed to touch runtime log file");
    }
}
