use camino::Utf8PathBuf;
use gold_band::storage::GoldBandPaths;

fn normalized(path: Utf8PathBuf) -> String {
    path.to_string().replace('\\', "/")
}

#[test]
fn builds_expected_runtime_paths() {
    let paths = GoldBandPaths::new(Utf8PathBuf::from("D:/repo"));

    assert_eq!(
        paths.repo_presets_dir(),
        Utf8PathBuf::from("D:/repo/.gold-band/presets")
    );
    assert_eq!(paths.project_id, "D--Repo");
    assert!(normalized(paths.task_file("task-001")).contains("/.gold-band/projects/D--Repo/"));
    assert!(
        normalized(paths.run_file("task-001", "run-001")).contains("/.gold-band/projects/D--Repo/")
    );
    assert!(
        normalized(paths.node_file("task-001", "run-001", "round-001", "dev", "attempt-001"))
            .contains("/.gold-band/projects/D--Repo/")
    );
}
