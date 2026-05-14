use camino::Utf8PathBuf;
use gold_band::app::{App, LogSource};
use tempfile::tempdir;

#[test]
fn prefers_progress_events_over_raw_stream() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let attempt_dir =
        app.paths
            .attempt_dir("task-001", "run-001", "round-001", "dev", "attempt-001");
    std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
    std::fs::write(
        attempt_dir.join("progress.events.jsonl").as_std_path(),
        "progress-event",
    )
    .unwrap();
    std::fs::write(
        attempt_dir.join("raw.stream.jsonl").as_std_path(),
        "raw-stream",
    )
    .unwrap();

    let output = app
        .provider_output("task-001", "run-001", "round-001", "dev", "attempt-001")
        .unwrap()
        .unwrap();

    assert_eq!(output, "progress-event");
}

#[test]
fn falls_back_to_raw_stream_when_progress_events_missing() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let attempt_dir =
        app.paths
            .attempt_dir("task-001", "run-001", "round-001", "dev", "attempt-001");
    std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
    std::fs::write(
        attempt_dir.join("raw.stream.jsonl").as_std_path(),
        "raw-stream",
    )
    .unwrap();

    let output = app
        .provider_output("task-001", "run-001", "round-001", "dev", "attempt-001")
        .unwrap()
        .unwrap();

    assert_eq!(output, "raw-stream");
}

#[test]
fn explicit_attempt_log_source_reads_requested_file() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);

    let attempt_dir =
        app.paths
            .attempt_dir("task-001", "run-001", "round-001", "dev", "attempt-001");
    std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
    std::fs::write(
        attempt_dir.join("progress.events.jsonl").as_std_path(),
        "progress-event",
    )
    .unwrap();
    std::fs::write(
        attempt_dir.join("raw.stream.jsonl").as_std_path(),
        "raw-stream",
    )
    .unwrap();

    let progress = app
        .attempt_log(
            "task-001",
            "run-001",
            "round-001",
            "dev",
            "attempt-001",
            LogSource::ProgressEvents,
        )
        .unwrap()
        .unwrap();
    let raw = app
        .attempt_log(
            "task-001",
            "run-001",
            "round-001",
            "dev",
            "attempt-001",
            LogSource::RawStream,
        )
        .unwrap()
        .unwrap();

    assert_eq!(progress, "progress-event");
    assert_eq!(raw, "raw-stream");
}
