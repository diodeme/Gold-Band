use camino::Utf8PathBuf;
use gold_band::storage::GoldBandPaths;

#[test]
fn builds_expected_runtime_paths() {
    let paths = GoldBandPaths::new(Utf8PathBuf::from("/repo"));

    assert_eq!(paths.task_file("task-001"), Utf8PathBuf::from("/repo/.gold-band/tasks/task-001/task.json"));
    assert_eq!(paths.run_file("task-001", "run-001"), Utf8PathBuf::from("/repo/.gold-band/tasks/task-001/runs/run-001/run.json"));
    assert_eq!(
        paths.node_file("task-001", "run-001", "round-001", "dev", "attempt-001"),
        Utf8PathBuf::from("/repo/.gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/node.json")
    );
}
