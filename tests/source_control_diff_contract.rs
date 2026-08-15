use camino::{Utf8Path, Utf8PathBuf};
use gold_band::git::{
    GitCommandRunner, GitComparisonSource, GitHistoryQuery, GitSourceControlService,
    GitWorkspaceDiffArea,
};

fn initialized_repository() -> (tempfile::TempDir, Utf8PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let runner = GitCommandRunner;
    for args in [
        vec!["init"],
        vec!["config", "user.name", "Gold Band Test"],
        vec!["config", "user.email", "test@gold-band.local"],
        vec!["config", "core.autocrlf", "false"],
    ] {
        assert!(runner.run(&root, &args).unwrap().success);
    }
    (temp, root)
}

fn commit_file(root: &Utf8Path, path: &str, content: &[u8]) {
    std::fs::write(root.join(path), content).unwrap();
    let runner = GitCommandRunner;
    assert!(runner.run(root, &["add", "--", path]).unwrap().success);
    assert!(
        runner
            .run(root, &["commit", "-m", "baseline"])
            .unwrap()
            .success
    );
}

#[test]
fn workspace_status_exposes_staged_and_unstaged_numstat() {
    let (_temp, root) = initialized_repository();
    commit_file(&root, "tracked.txt", b"first\nsecond\nthird\n");
    std::fs::write(root.join("tracked.txt"), b"first\nchanged\nthird\n").unwrap();
    let service = GitSourceControlService::default();

    let unstaged = service.status(&root).unwrap();
    let tracked = unstaged
        .unstaged
        .iter()
        .find(|change| change.path == "tracked.txt")
        .unwrap();
    assert_eq!(tracked.added_lines, Some(1));
    assert_eq!(tracked.deleted_lines, Some(1));

    let runner = GitCommandRunner;
    assert!(
        runner
            .run(&root, &["add", "--", "tracked.txt"])
            .unwrap()
            .success
    );
    std::fs::write(root.join("tracked.txt"), b"first\nchanged\nthird\nfourth\n").unwrap();

    let split = service.status(&root).unwrap();
    let staged = split
        .staged
        .iter()
        .find(|change| change.path == "tracked.txt")
        .unwrap();
    assert_eq!(
        (staged.added_lines, staged.deleted_lines),
        (Some(1), Some(1))
    );
    let unstaged = split
        .unstaged
        .iter()
        .find(|change| change.path == "tracked.txt")
        .unwrap();
    assert_eq!(
        (unstaged.added_lines, unstaged.deleted_lines),
        (Some(1), Some(0))
    );
}

#[test]
fn comparison_normalizes_large_crlf_content_before_counting_and_rendering() {
    let (_temp, root) = initialized_repository();
    let before = (0..8_000)
        .map(|line| format!("line {line}\r\n"))
        .collect::<String>();
    commit_file(&root, "large-source.rs", before.as_bytes());
    let mut after = (0..8_000)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    after.push_str("added one\nadded two\n");
    std::fs::write(root.join("large-source.rs"), after).unwrap();

    let comparison = GitSourceControlService::default()
        .comparison(
            &root,
            &GitComparisonSource::Workspace {
                workspace_path: None,
                path: "large-source.rs".to_string(),
                area: GitWorkspaceDiffArea::Unstaged,
            },
        )
        .unwrap();

    assert_eq!(comparison.stats.added_lines, 2);
    assert_eq!(comparison.stats.deleted_lines, 0);
    assert!(!comparison.before.unwrap().content.contains('\r'));
    assert!(!comparison.after.unwrap().content.contains('\r'));
}

#[test]
fn default_history_follows_head_and_pages_through_the_root_commit() {
    let (_temp, root) = initialized_repository();
    commit_file(&root, "root.txt", b"root\n");
    let runner = GitCommandRunner;
    let main_branch = runner
        .run(&root, &["branch", "--show-current"])
        .unwrap()
        .stdout;
    assert!(
        runner
            .run(&root, &["switch", "-c", "side-history"])
            .unwrap()
            .success
    );
    std::fs::write(root.join("side.txt"), b"side\n").unwrap();
    assert!(
        runner
            .run(&root, &["add", "--", "side.txt"])
            .unwrap()
            .success
    );
    assert!(
        runner
            .run(&root, &["commit", "-m", "side-only"])
            .unwrap()
            .success
    );
    let side_oid = runner.run(&root, &["rev-parse", "HEAD"]).unwrap().stdout;
    assert!(
        runner
            .run(&root, &["switch", main_branch.as_str()])
            .unwrap()
            .success
    );
    for index in 1..=4 {
        std::fs::write(root.join("root.txt"), format!("root {index}\n")).unwrap();
        assert!(
            runner
                .run(&root, &["add", "--", "root.txt"])
                .unwrap()
                .success
        );
        assert!(
            runner
                .run(&root, &["commit", "-m", &format!("main-{index}")])
                .unwrap()
                .success
        );
    }

    let service = GitSourceControlService::default();
    let mut cursor = None;
    let mut commits = Vec::new();
    loop {
        let page = service
            .history(
                &root,
                &GitHistoryQuery {
                    cursor,
                    limit: Some(2),
                    revision: None,
                    ref_name: None,
                },
            )
            .unwrap();
        commits.extend(page.commits);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(commits.len(), 5);
    assert!(commits.iter().all(|commit| commit.oid != side_oid));
    assert!(commits.last().unwrap().parent_oids.is_empty());
}
