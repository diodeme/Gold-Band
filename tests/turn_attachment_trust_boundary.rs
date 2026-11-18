use std::collections::HashMap;
use std::path::Path;

use camino::Utf8PathBuf;
use gold_band::acp::turn_files::{
    ATTACHMENT_ACCESS_DENIED, TurnFileCaptureConfig, TurnFileChangeSet, TurnFileStore,
};

fn attachment_store() -> (tempfile::TempDir, TurnFileStore, TurnFileChangeSet) {
    let dir = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let store = TurnFileStore::new(root.clone(), TurnFileCaptureConfig::default());
    std::fs::create_dir(root.join("attachments")).unwrap();
    store.capture_attachment_baseline("turn").unwrap();
    std::fs::write(root.join("attachments/report.md"), "original\n").unwrap();
    let delta = store.collect_turn_attachment_delta("turn").unwrap();
    let set = store
        .finalize_turn_branch_with_attachments(
            "turn",
            "prompt",
            "root",
            "1Z",
            "2Z",
            &HashMap::new(),
            None,
            &delta,
            true,
        )
        .unwrap()
        .unwrap();
    (dir, store, set)
}

// Junctions exercise the Windows directory-link boundary without requiring
// Developer Mode or the privilege needed to create symbolic links.
fn link_directory(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let output = gold_band::process::background_command("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    assert!(
        std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

fn unlink_directory(link: &Path) {
    #[cfg(windows)]
    std::fs::remove_dir(link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(link).unwrap();
}

#[test]
fn preview_and_metadata_reject_a_replaced_attachment_root() {
    let (dir, store, set) = attachment_store();
    let id = &set.attachments[0].id;
    assert_eq!(
        std::fs::read_to_string(store.resolve_attachment_path(&set.id, id).unwrap()).unwrap(),
        "original\n"
    );
    assert_eq!(
        store
            .attachment_relative_path_metadata(&set.id, id)
            .unwrap(),
        "report.md"
    );

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("report.md"), "outside\n").unwrap();
    let root = dir.path().join("attachments");
    std::fs::remove_dir_all(&root).unwrap();
    link_directory(outside.path(), &root);
    let preview = store.resolve_attachment_path(&set.id, id);
    let metadata = store.attachment_relative_path_metadata(&set.id, id);
    unlink_directory(&root);

    assert_eq!(metadata.unwrap_err().to_string(), ATTACHMENT_ACCESS_DENIED);
    assert_eq!(preview.unwrap_err().to_string(), ATTACHMENT_ACCESS_DENIED);
    assert_eq!(
        std::fs::read_to_string(outside.path().join("report.md")).unwrap(),
        "outside\n"
    );
}

#[test]
fn preview_rejects_an_attachment_subdirectory_link_escape() {
    let (dir, store, mut set) = attachment_store();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("report.md"), "outside\n").unwrap();
    let link = dir.path().join("attachments").join("nested");
    link_directory(outside.path(), &link);
    set.attachments[0].relative_path = "nested/report.md".into();
    let manifest = dir
        .path()
        .join("turn-file-change-sets")
        .join(format!("{}.json", set.id));
    std::fs::write(manifest, serde_json::to_vec(&set).unwrap()).unwrap();
    let preview = store.resolve_attachment_path(&set.id, &set.attachments[0].id);
    unlink_directory(&link);

    assert_eq!(preview.unwrap_err().to_string(), ATTACHMENT_ACCESS_DENIED);
}
