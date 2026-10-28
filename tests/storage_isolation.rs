use camino::Utf8PathBuf;
use gold_band::storage::GoldBandPaths;
use tempfile::tempdir;

/// Integration tests link the library without `cfg(test)`; the self dev-dependency
/// must enable `test-isolated-home` so user storage stays inside the test repo.
#[test]
fn integration_tests_keep_user_storage_inside_repo() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let paths = GoldBandPaths::new(repo_root.clone());

    assert!(paths.user_gold_band_root.starts_with(&repo_root));
    assert!(paths.user_state_file().starts_with(&repo_root));
}
