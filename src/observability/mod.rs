use std::fs::{self, File};
use std::io::Write as _;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use camino::Utf8Path;
use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use serde::Serialize;
use tracing::warn;
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

use crate::config::{RuntimeConfig, RuntimeLogLevel};
use crate::domain::{NodeType, PauseReason, RunStatus, VERSION};
use crate::inspect::render_run_status;
use crate::runtime::RunState;
use crate::storage::{GoldBandPaths, append_jsonl, ensure_parent_dir, write_json};

const PROGRESS_TARGET: &str = "gold_band.progress";
const RUNTIME_LOG_MAX_BYTES: usize = 8 * 1024 * 1024;
const RUNTIME_LOG_ROTATED_FILES: usize = 4;
const RUNTIME_LOG_BUFFERED_LINES_LIMIT: usize = 1_024;
static TRACE_ID: OnceLock<String> = OnceLock::new();
static TRACING_INITIALIZED: AtomicBool = AtomicBool::new(false);
static RUNTIME_LOG_LEVEL: AtomicU8 = AtomicU8::new(RuntimeLogLevel::Info.as_u8());

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
    pub runtime_revision: u64,
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
    pub control_failure: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

struct LocalTimer;
impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(
            w,
            "{}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f")
        )
    }
}

#[derive(Debug)]
pub struct RuntimeLogGuard {
    _worker_guard: WorkerGuard,
    dropped_lines: ErrorCounter,
}

impl RuntimeLogGuard {
    fn new(worker_guard: WorkerGuard, dropped_lines: ErrorCounter) -> Self {
        Self {
            _worker_guard: worker_guard,
            dropped_lines,
        }
    }

    pub fn dropped_lines(&self) -> usize {
        self.dropped_lines.dropped_lines()
    }
}

impl Drop for RuntimeLogGuard {
    fn drop(&mut self) {
        let dropped_lines = self.dropped_lines();
        if dropped_lines > 0 {
            warn!(
                target: "gold_band::observability",
                dropped_lines,
                "runtime log queue dropped diagnostic lines"
            );
            eprintln!("gold-band: runtime log queue dropped {dropped_lines} lines");
        }
    }
}

pub fn init_tracing(
    paths: &GoldBandPaths,
    config: &RuntimeConfig,
    enable_stderr_progress: bool,
) -> Option<RuntimeLogGuard> {
    let _ = TRACE_ID.get_or_init(trace_id_seed);
    cleanup_old_logs(paths, config.log_retention_days);
    set_runtime_log_level(config.log_level);
    if TRACING_INITIALIZED.swap(true, Ordering::SeqCst) {
        return None;
    }

    let logs_dir = paths.logs_dir();
    if let Err(err) = fs::create_dir_all(logs_dir.as_std_path()) {
        eprintln!("gold-band: failed to create logs dir {logs_dir}: {err}");
        return None;
    }

    let log_path = paths.runtime_log_file();
    let stderr_writer = BoxMakeWriter::new(std::io::stderr);

    let progress_filter = EnvFilter::new(format!("{PROGRESS_TARGET}=info"));

    let (runtime_log_writer, runtime_log_guard) = runtime_log_channel(
        runtime_log_writer(
            log_path.as_std_path(),
            RUNTIME_LOG_MAX_BYTES,
            RUNTIME_LOG_ROTATED_FILES,
        ),
        RUNTIME_LOG_BUFFERED_LINES_LIMIT,
    );
    let dropped_lines = runtime_log_writer.error_counter();

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(runtime_log_writer)
        .with_target(true)
        .with_timer(LocalTimer)
        .with_filter(FilterFn::new(runtime_log_filter));

    let stderr_layer = fmt::layer()
        .compact()
        .with_target(false)
        .with_timer(LocalTimer)
        .with_writer(stderr_writer)
        .with_filter(progress_filter);

    let registry = tracing_subscriber::registry().with(file_layer);
    if enable_stderr_progress {
        registry.with(stderr_layer).init();
    } else {
        registry.init();
    }

    Some(RuntimeLogGuard::new(runtime_log_guard, dropped_lines))
}

fn runtime_log_channel<T: std::io::Write + Send + 'static>(
    writer: T,
    buffered_lines_limit: usize,
) -> (NonBlocking, WorkerGuard) {
    NonBlockingBuilder::default()
        .buffered_lines_limit(buffered_lines_limit)
        .lossy(true)
        .thread_name("gold-band-runtime-log")
        .finish(writer)
}

fn runtime_log_writer(
    path: &std::path::Path,
    max_bytes: usize,
    rotated_files: usize,
) -> FileRotate<AppendCount> {
    FileRotate::new(
        path,
        AppendCount::new(rotated_files),
        ContentLimit::Bytes(max_bytes),
        Compression::None,
        None,
    )
}

pub fn set_runtime_log_level(level: RuntimeLogLevel) {
    RUNTIME_LOG_LEVEL.store(level.as_u8(), Ordering::SeqCst);
}

pub fn runtime_log_level() -> RuntimeLogLevel {
    RuntimeLogLevel::from_u8(RUNTIME_LOG_LEVEL.load(Ordering::SeqCst))
}

pub fn runtime_log_filter(metadata: &tracing::Metadata<'_>) -> bool {
    if metadata.target() == PROGRESS_TARGET {
        return false;
    }
    if !matches!(metadata.target(), target if target.starts_with("gold_band") || target.starts_with("gold_band_desktop"))
    {
        return false;
    }
    runtime_log_level().allows(metadata.level())
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
        runtime_revision: run.execution.revision,
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
        control_failure: None,
        details: None,
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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{runtime_log_channel, runtime_log_writer};

    struct GatedWriter {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        block_next_write: bool,
    }

    impl Write for GatedWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            if self.block_next_write {
                self.block_next_write = false;
                let _ = self.entered.send(());
                let _ = self.release.recv();
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn runtime_log_queue_drops_at_capacity_without_backpressure() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (mut writer, guard) = runtime_log_channel(
            GatedWriter {
                entered: entered_tx,
                release: release_rx,
                block_next_write: true,
            },
            1,
        );
        let dropped_lines = writer.error_counter();

        writer.write_all(b"first\n").expect("enqueue first line");
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker starts first write");
        writer.write_all(b"second\n").expect("fill queue");
        writer
            .write_all(b"third\n")
            .expect("drop without surfacing writer failure");

        assert_eq!(dropped_lines.dropped_lines(), 1);
        release_tx.send(()).expect("release worker");
        drop(writer);
        drop(guard);
    }

    #[test]
    fn async_runtime_log_flushes_and_keeps_configured_rotated_backups() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let log_path = temp.path().join("runtime.log");
        let (mut writer, guard) = runtime_log_channel(runtime_log_writer(&log_path, 8, 4), 16);

        writer
            .write_all(b"abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH")
            .expect("enqueue log data");
        drop(writer);
        drop(guard);

        assert!(log_path.exists());
        assert!(std::fs::metadata(&log_path).unwrap().len() <= 8);
        for suffix in 1..=4 {
            let rotated = temp.path().join(format!("runtime.log.{suffix}"));
            assert!(rotated.exists(), "missing {}", rotated.display());
            assert!(std::fs::metadata(rotated).unwrap().len() <= 8);
        }
        assert!(!temp.path().join("runtime.log.5").exists());
    }
}
