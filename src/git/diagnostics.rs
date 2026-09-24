//! Scoped, content-free timings for source-control reads. No process-global
//! current request: blocking workers and numstat threads carry their own scope.
use std::cell::RefCell;
use std::process::{Command, Output};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

thread_local! {
    static CURRENT: RefCell<Option<GitReadTrace>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub struct GitReadTrace {
    load_id: String,
    operation: &'static str,
    commands: Arc<AtomicU64>,
}

pub struct GitReadRequest {
    trace: GitReadTrace,
    started: Instant,
    outcome: &'static str,
}

impl GitReadRequest {
    pub fn new(load_id: Option<&str>, operation: &'static str) -> Self {
        // Only UUIDs may cross into the log, never arbitrary frontend strings.
        let load_id = load_id
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .unwrap_or_else(uuid::Uuid::new_v4)
            .to_string();
        let trace = GitReadTrace {
            load_id,
            operation,
            commands: Arc::new(AtomicU64::new(0)),
        };
        tracing::debug!(target: "gold_band::git::load", load_id = %trace.load_id,
            operation, event = "request_start");
        Self {
            trace,
            started: Instant::now(),
            outcome: "aborted",
        }
    }

    pub fn trace(&self) -> GitReadTrace {
        self.trace.clone()
    }

    pub fn finish<T, E>(&mut self, result: &Result<T, E>) {
        self.outcome = if result.is_ok() { "ok" } else { "error" };
    }
}

impl Drop for GitReadRequest {
    fn drop(&mut self) {
        tracing::debug!(target: "gold_band::git::load", load_id = %self.trace.load_id,
            operation = self.trace.operation, event = "request_end", outcome = self.outcome,
            elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0,
            git_commands = self.trace.commands.load(Ordering::Relaxed));
    }
}

impl GitReadTrace {
    #[cfg(test)]
    pub(crate) fn command_count(&self) -> u64 {
        self.commands.load(Ordering::Relaxed)
    }

    pub fn current() -> Option<Self> {
        CURRENT.with(|current| current.borrow().clone())
    }

    pub fn scope<T>(&self, action: impl FnOnce() -> T) -> T {
        struct Restore(Option<GitReadTrace>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT.with(|current| {
                    current.replace(self.0.take());
                });
            }
        }
        let _restore = Restore(CURRENT.with(|current| current.replace(Some(self.clone()))));
        action()
    }

    pub fn stage<T, E>(
        &self,
        stage: &'static str,
        action: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let started = Instant::now();
        tracing::debug!(target: "gold_band::git::load", load_id = %self.load_id,
            operation = self.operation, event = "stage_start", stage);
        let result = self.scope(action);
        tracing::debug!(target: "gold_band::git::load", load_id = %self.load_id,
            operation = self.operation, event = "stage_end", stage,
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, success = result.is_ok());
        result
    }
}

pub(crate) fn stage<T, E>(
    name: &'static str,
    action: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match GitReadTrace::current() {
        Some(trace) => trace.stage(name, action),
        None => action(),
    }
}

/// Includes executable lookup/PATH preparation separately from output(), which
/// includes OS process creation, execution, stream collection and process exit.
pub(crate) fn command_output(
    args: &[&str],
    prepare: impl FnOnce() -> anyhow::Result<Command>,
) -> anyhow::Result<Output> {
    let Some(trace) = GitReadTrace::current() else {
        return Ok(prepare()?.output()?);
    };
    let command_number = trace.commands.fetch_add(1, Ordering::Relaxed) + 1;
    let command = command_kind(args);
    let started = Instant::now();
    tracing::debug!(target: "gold_band::git::load", load_id = %trace.load_id,
        operation = trace.operation, event = "command_start", command_number, command);
    let prepared = prepare();
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
    let execution = Instant::now();
    let result = prepared.and_then(|mut command| command.output().map_err(Into::into));
    tracing::debug!(target: "gold_band::git::load", load_id = %trace.load_id,
        operation = trace.operation, event = "command_end", command_number, command,
        prepare_ms, execution_ms = execution.elapsed().as_secs_f64() * 1000.0,
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        success = result.as_ref().is_ok_and(|output| output.status.success()),
        exit_code = result.as_ref().ok().and_then(|output| output.status.code()),
        output_received = result.is_ok());
    result
}

fn command_kind(args: &[&str]) -> &'static str {
    match args.first().copied() {
        Some("--version") => "version",
        Some("rev-parse") if args.contains(&"--git-path") => "metadata-path",
        Some("rev-parse") => "repository-identity",
        Some("status") => "status",
        Some("diff") if args.contains(&"--cached") => "staged-diff",
        Some("diff") => "unstaged-diff",
        Some("for-each-ref") => "refs",
        Some("worktree") => "worktrees",
        Some("stash") => "stashes",
        Some("remote") => "remotes",
        Some("log") => "history",
        _ => "other",
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitLoadReport {
    load_id: String,
    outcome: GitLoadOutcome,
    initial: bool,
    elapsed_ms: f64,
    capability_ms: Option<f64>,
    subscriptions_ms: Option<f64>,
    monitor_ms: Option<f64>,
    snapshot_ms: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum GitLoadOutcome {
    Ready,
    Unavailable,
    Error,
    Superseded,
}

impl GitLoadReport {
    pub fn record(&self) {
        let Ok(load_id) = uuid::Uuid::parse_str(&self.load_id) else {
            return;
        };
        if [
            Some(self.elapsed_ms),
            self.capability_ms,
            self.subscriptions_ms,
            self.monitor_ms,
            self.snapshot_ms,
        ]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value < 0.0)
        {
            return;
        }
        tracing::debug!(target: "gold_band::git::load", %load_id, event = "frontend_load_end",
            outcome = ?self.outcome, initial = self.initial, elapsed_ms = self.elapsed_ms,
            capability_ms = self.capability_ms, subscriptions_ms = self.subscriptions_ms,
            monitor_ms = self.monitor_ms, snapshot_ms = self.snapshot_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A local subscriber changes tracing's shared callsite-interest cache. Keep
    // these tests from first-registering the same callsite on an untraced thread.
    static LOG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn debug_records_are_correlated_content_free_and_hidden_at_info() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        for level in [tracing::Level::DEBUG, tracing::Level::INFO] {
            let buffer = LogBuffer::default();
            let writer = buffer.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_ansi(false)
                .without_time()
                .with_max_level(level)
                .with_writer(move || writer.clone())
                .finish();
            tracing::subscriber::with_default(subscriber, || {
                let mut request =
                    GitReadRequest::new(Some("00000000-0000-4000-8000-000000000001"), "snapshot");
                let result = request.trace().stage("identity", || {
                    command_output(&["rev-parse", "PRIVATE_PATH"], || {
                        anyhow::bail!("PRIVATE_ERROR")
                    })
                });
                request.finish(&result);
            });
            let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
            if level == tracing::Level::INFO {
                assert!(log.is_empty(), "{log}");
            } else {
                for field in [
                    "DEBUG",
                    "gold_band::git::load",
                    "00000000-0000-4000-8000-000000000001",
                    "prepare_ms=",
                    "execution_ms=",
                    "request_end",
                    "stage_end",
                    "outcome=\"error\"",
                    "git_commands=1",
                ] {
                    assert!(log.contains(field), "missing {field}: {log}");
                }
                assert!(!log.contains("PRIVATE_"), "{log}");
            }
        }
    }

    #[test]
    fn scope_restores_after_nested_request_and_panic() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        let outer = GitReadRequest::new(None, "snapshot");
        let inner = GitReadRequest::new(None, "monitor");
        outer.trace().scope(|| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                inner.trace().scope(|| panic!("test unwind"));
            }));
            assert_eq!(
                GitReadTrace::current().unwrap().load_id,
                outer.trace.load_id
            );
        });
        assert!(GitReadTrace::current().is_none());
    }

    #[test]
    fn command_labels_never_include_arguments_or_unknown_commands() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        assert_eq!(
            command_kind(&["remote", "https://secret@example.test"]),
            "remotes"
        );
        assert_eq!(command_kind(&["secret"]), "other");
        assert_eq!(command_kind(&["diff", "--cached"]), "staged-diff");
        let request = GitReadRequest::new(Some("secret\nlog injection"), "snapshot");
        assert!(uuid::Uuid::parse_str(&request.trace.load_id).is_ok());
    }

    #[test]
    fn failed_preparation_is_counted_and_workers_share_only_their_request() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        let request = GitReadRequest::new(None, "snapshot");
        let trace = request.trace();
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let trace = trace.clone();
                scope.spawn(move || {
                    trace.scope(|| {
                        assert!(
                            command_output(&["status"], || anyhow::bail!("unavailable")).is_err()
                        );
                    })
                });
            }
        });
        assert_eq!(trace.commands.load(Ordering::Relaxed), 2);
        assert_eq!(
            GitReadRequest::new(None, "monitor")
                .trace
                .commands
                .load(Ordering::Relaxed),
            0
        );
        assert!(GitReadTrace::current().is_none());
    }
}
