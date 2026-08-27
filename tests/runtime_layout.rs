use camino::Utf8PathBuf;
use gold_band::storage::GoldBandPaths;

fn normalized(path: Utf8PathBuf) -> String {
    path.to_string().replace('\\', "/")
}

#[test]
fn builds_expected_runtime_paths() {
    let paths = GoldBandPaths::new(Utf8PathBuf::from("gold-band-runtime-layout-test/Repo"));

    assert_eq!(
        paths.repo_presets_dir(),
        Utf8PathBuf::from("gold-band-runtime-layout-test/Repo/.gold-band/presets")
    );
    let project_id = paths.project_id.clone();
    let (slug, hash) = project_id.rsplit_once("--").unwrap();
    assert_eq!(
        slug.to_ascii_lowercase(),
        "gold-band-runtime-layout-test-repo"
    );
    assert_eq!(hash.len(), 8);
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(
        normalized(paths.task_file("task-001"))
            .contains(&format!("/.gold-band/projects/{project_id}/"))
    );
    assert!(
        normalized(paths.run_file("task-001", "run-001"))
            .contains(&format!("/.gold-band/projects/{project_id}/"))
    );
    assert!(
        normalized(paths.node_file("task-001", "run-001", "round-001", "dev", "attempt-001"))
            .contains(&format!("/.gold-band/projects/{project_id}/"))
    );
}
