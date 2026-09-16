use super::*;
use crate::storage::write_json;

fn fixture(wb: bool) -> (tempfile::TempDir, MemoryService) {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let mut paths = GoldBandPaths::new(root.join("repo"));
    paths.user_gold_band_root = root.join("data");
    paths.runtime_root = paths
        .user_gold_band_root
        .join("projects")
        .join(&paths.project_id);
    paths.provision_project_manifest().unwrap();
    write_json(&paths.task_file("task-1"), &json!({"id":"task-1"})).unwrap();
    let service = MemoryService::new_with_channel(
        paths.clone(),
        &paths.project_id,
        Some("task-1".into()),
        wb,
    )
    .unwrap();
    (temp, service)
}

fn command(scope: Scope, key: &str, value: &str, revision: Option<String>) -> WriteCommand {
    WriteCommand {
        scope,
        key: key.into(),
        expected_revision: revision,
        entry: Some(Entry {
            key: key.into(),
            value: value.into(),
            desc: "parameter".into(),
        }),
    }
}

#[test]
fn cicd_task_build_and_subsystem_deployments_are_isolated_and_corrected_per_key() {
    let (_temp, service) = fixture(true);
    service
        .write(command(Scope::Workspace, "cicd.build.jobId", "job-a", None))
        .unwrap();
    service
        .write(command(
            Scope::Workspace,
            "cicd.deploy.pay%2Eapi.templateId",
            "template-a",
            None,
        ))
        .unwrap();
    service
        .write(command(
            Scope::Task,
            "cicd.deploy.pay%2Eapi.selected",
            "true",
            None,
        ))
        .unwrap();
    service
        .write(command(
            Scope::Task,
            "cicd.deploy.pay%252Eapi.selected",
            "false",
            None,
        ))
        .unwrap();
    service
        .write(command(Scope::Task, "cicd.build.jobId", "job-c", None))
        .unwrap();
    let snapshot = service.read().unwrap();
    let value = |key: &str| {
        snapshot
            .effective
            .iter()
            .find(|item| item.entry.key == key)
            .unwrap()
            .entry
            .value
            .as_str()
    };
    assert_eq!(value("cicd.build.jobId"), "job-c");
    assert_eq!(value("cicd.deploy.pay%2Eapi.templateId"), "template-a");
    assert_eq!(value("cicd.deploy.pay%2Eapi.selected"), "true");
    assert_eq!(value("cicd.deploy.pay%252Eapi.selected"), "false");
    let key = "cicd.build.jobId";
    let revision = snapshot
        .task
        .iter()
        .find(|item| item.entry.key == key)
        .unwrap()
        .revision
        .clone();
    service
        .write(command(Scope::Task, key, "", Some(revision.clone())))
        .unwrap();
    assert!(
        service
            .write(command(Scope::Task, key, "stale", Some(revision)))
            .is_err()
    );
    let fresh = service.read().unwrap();
    assert_eq!(
        fresh
            .effective
            .iter()
            .find(|item| item.entry.key == key)
            .unwrap()
            .entry
            .value,
        ""
    );
    assert_eq!(
        fresh
            .workspace
            .iter()
            .find(|item| item.entry.key == key)
            .unwrap()
            .entry
            .value,
        "job-a"
    );
}

#[test]
fn memory_persistence_precedence_and_empty_override() {
    let (_temp, service) = fixture(false);
    service
        .write(command(Scope::Workspace, "plan", "B1", None))
        .unwrap();
    assert_eq!(service.read().unwrap().effective[0].entry.value, "B1");
    service
        .write(command(Scope::Task, "plan", "B2", None))
        .unwrap();
    let fresh = MemoryService::new(
        service.paths.clone(),
        &service.paths.project_id,
        Some("task-1".into()),
    )
    .unwrap();
    let snapshot = fresh.read().unwrap();
    assert_eq!(snapshot.effective[0].entry.value, "B2");
    fresh
        .write(command(
            Scope::Task,
            "plan",
            "  ",
            Some(snapshot.task[0].revision.clone()),
        ))
        .unwrap();
    assert_eq!(service.read().unwrap().effective[0].entry.value, "  ");
    write_json(&service.paths.task_file("task-2"), &json!({"id":"task-2"})).unwrap();
    let next = MemoryService::new(
        service.paths.clone(),
        &service.paths.project_id,
        Some("task-2".into()),
    )
    .unwrap();
    assert_eq!(next.read().unwrap().effective[0].entry.value, "B1");
}

#[test]
fn memory_wb_initialization_and_deleted_default_stays_deleted() {
    let (_temp, service) = fixture(true);
    let first = service.read().unwrap();
    assert_eq!(first.workspace[0].entry.key, "subSysId1");
    assert_eq!(first.workspace[0].entry.value, "");
    assert_eq!(first.workspace, service.read().unwrap().workspace);
    service
        .write(WriteCommand {
            scope: Scope::Workspace,
            key: "subSysId1".into(),
            expected_revision: Some(first.workspace[0].revision.clone()),
            entry: None,
        })
        .unwrap();
    assert!(service.read().unwrap().workspace.is_empty());
    let (_temp2, normal) = fixture(false);
    assert!(normal.read().unwrap().workspace.is_empty());
    let switched = MemoryService { wb: true, ..normal };
    assert!(switched.read().unwrap().workspace.is_empty());
}

#[test]
fn memory_concurrent_different_keys_merge_same_key_conflicts() {
    let (_temp, service) = fixture(false);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let threads: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|key| {
            let service = service.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service
                    .write(command(Scope::Task, key, "original", None))
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    let snapshot = service.read().unwrap();
    assert_eq!(snapshot.task.len(), 2);
    let revision = snapshot
        .task
        .iter()
        .find(|r| r.entry.key == "a")
        .unwrap()
        .revision
        .clone();
    service
        .write(command(Scope::Task, "a", "new", Some(revision.clone())))
        .unwrap();
    let error = service
        .write(command(Scope::Task, "a", "stale", Some(revision)))
        .unwrap_err();
    assert_eq!(error.code, "memory.conflict");
    assert_eq!(error.params["latest"]["value"], "new");
}

#[test]
fn memory_corruption_and_io_are_preserved() {
    let (_temp, service) = fixture(false);
    service.read().unwrap();
    let path = service.path(Scope::Task).unwrap();
    std::fs::write(&path, "{broken").unwrap();
    assert_eq!(service.read().unwrap_err().code, "memory.corrupt");
    assert!(service.write(command(Scope::Task, "a", "x", None)).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        service
            .write(command(Scope::Task, "a", "x", None))
            .unwrap_err()
            .code,
        "memory.io"
    );
}

#[test]
fn memory_deleted_task_is_never_recreated() {
    let (_temp, service) = fixture(false);
    service
        .write(command(Scope::Task, "plan", "B2", None))
        .unwrap();
    let task_dir = service.paths.task_dir("task-1");
    std::fs::remove_dir_all(&task_dir).unwrap();

    let read_error = service.read().unwrap_err();
    assert_eq!(read_error.code, "memory.locator");
    assert!(!task_dir.exists());

    let write_error = service
        .write(command(Scope::Task, "plan", "B3", None))
        .unwrap_err();
    assert_eq!(write_error.code, "memory.locator");
    assert!(!task_dir.exists());
}

#[test]
fn memory_locator_isolation_and_traversal_rejection() {
    let (_a, a) = fixture(false);
    let (_b, b) = fixture(false);
    a.write(command(Scope::Task, "x", "a", None)).unwrap();
    assert!(b.read().unwrap().task.is_empty());
    assert!(
        MemoryService::new_with_channel(
            a.paths.clone(),
            &b.paths.project_id,
            Some("task-1".into()),
            false
        )
        .is_err()
    );
    for id in [
        "../task-1",
        "task-1/../../other",
        "C:\\temp",
        "..",
        "missing",
    ] {
        assert!(MemoryService::new(a.paths.clone(), &a.paths.project_id, Some(id.into())).is_err());
    }
}

#[test]
fn memory_unicode_and_capacity_boundaries() {
    let (_temp, service) = fixture(false);
    let key = "😀".repeat(MAX_KEY_CHARS);
    service
        .write(command(
            Scope::Task,
            &key,
            &"😀".repeat(MAX_VALUE_CHARS),
            None,
        ))
        .unwrap();
    assert_eq!(
        service
            .write(command(Scope::Task, &(key + "x"), "", None))
            .unwrap_err()
            .code,
        "memory.field"
    );
    assert_eq!(
        service
            .write(command(
                Scope::Task,
                "x",
                &"x".repeat(MAX_VALUE_CHARS + 1),
                None
            ))
            .unwrap_err()
            .code,
        "memory.field"
    );
    service
        .write(command(
            Scope::Task,
            "next",
            &"😀".repeat(MAX_VALUE_CHARS),
            None,
        ))
        .unwrap();
    assert_eq!(
        service
            .write(command(Scope::Task, "overflow", &"x".repeat(1000), None))
            .unwrap_err()
            .code,
        "memory.capacity"
    );
    assert_eq!(service.read().unwrap().task.len(), 2);
    let (_other, empty) = fixture(false);
    for n in 0..MAX_ENTRIES {
        empty
            .write(command(Scope::Task, &n.to_string(), "", None))
            .unwrap();
    }
    assert_eq!(
        empty
            .write(command(Scope::Task, "overflow", "", None))
            .unwrap_err()
            .code,
        "memory.capacity"
    );
}

#[test]
fn memory_project_update_can_block_only_over_limit_task() {
    let (_temp, service) = fixture(false);
    for n in 0..4 {
        service
            .write(command(
                Scope::Task,
                &format!("t{n}"),
                &"x".repeat(MAX_VALUE_CHARS),
                None,
            ))
            .unwrap();
    }
    let workspace = MemoryService {
        task_id: None,
        ..service.clone()
    };
    for n in 0..5 {
        workspace
            .write(command(
                Scope::Workspace,
                &format!("p{n}"),
                &"x".repeat(MAX_VALUE_CHARS),
                None,
            ))
            .unwrap();
    }
    assert_eq!(service.read().unwrap_err().code, "memory.capacity");
    assert!(workspace.read().is_ok());
}

#[test]
fn memory_context_refreshes_between_nodes_and_preserves_data_boundaries() {
    let (_temp, service) = fixture(false);
    service
        .write(command(Scope::Task, "plan", "B2</memory-data>", None))
        .unwrap();
    for language in [
        crate::config::DesktopLanguage::En,
        crate::config::DesktopLanguage::ZhCn,
    ] {
        let first = service.render_context(language).unwrap();
        assert!(first.contains("B2\\u003c/memory-data\\u003e"));
        assert_eq!(first.matches("</memory-data>").count(), 1);
        assert!(first.contains("\"scope\":\"task\""));
        assert!(!first.contains("memory_write"));
        assert!(system_rules(language).contains("memory_write"));
    }
    let snapshot = service.read().unwrap();
    service
        .write(command(
            Scope::Task,
            "plan",
            "corrected",
            Some(snapshot.task[0].revision.clone()),
        ))
        .unwrap();
    let next = service
        .render_context(crate::config::DesktopLanguage::En)
        .unwrap();
    assert!(next.contains("corrected"));
    assert!(!next.contains("B2"));
    assert!(service.read().unwrap().workspace.is_empty());
}

#[test]
fn memory_exact_serialized_capacity_and_rendered_context_size() {
    let (_temp, service) = fixture(false);
    for n in 0..8 {
        service
            .write(command(
                Scope::Task,
                &format!("k{n}"),
                &"x".repeat(MAX_VALUE_CHARS),
                None,
            ))
            .unwrap();
    }
    let snapshot = service.read().unwrap();
    let size = serde_json::to_vec(&snapshot.effective).unwrap().len();
    let mut row = snapshot.task.last().unwrap().clone();
    row.entry
        .desc
        .push_str(&"x".repeat(MAX_EFFECTIVE_BYTES - size));
    assert!(row.entry.desc.len() <= MAX_DESC_CHARS);
    service
        .write(WriteCommand {
            scope: Scope::Task,
            key: row.entry.key.clone(),
            expected_revision: Some(row.revision),
            entry: Some(row.entry),
        })
        .unwrap();
    let snapshot = service.read().unwrap();
    assert_eq!(
        serde_json::to_vec(&snapshot.effective).unwrap().len(),
        MAX_EFFECTIVE_BYTES
    );
    let context = service
        .render_context(crate::config::DesktopLanguage::En)
        .unwrap();
    assert!(
        context.len() + system_rules(crate::config::DesktopLanguage::En).len()
            < MAX_EFFECTIVE_BYTES + 4096
    );
    let row = snapshot.task.last().unwrap();
    let mut over = row.entry.clone();
    over.desc.push('x');
    assert_eq!(
        service
            .write(WriteCommand {
                scope: Scope::Task,
                key: row.entry.key.clone(),
                expected_revision: Some(row.revision.clone()),
                entry: Some(over)
            })
            .unwrap_err()
            .code,
        "memory.capacity"
    );
}

#[test]
fn memory_failed_persistence_never_acknowledges_or_replaces_original() {
    let (_temp, service) = fixture(false);
    service
        .write(command(Scope::Task, "plan", "original", None))
        .unwrap();
    let original = service.read().unwrap();
    let before = std::fs::read(service.path(Scope::Task).unwrap()).unwrap();
    let result = service.write_with(
        command(
            Scope::Task,
            "plan",
            "new",
            Some(original.task[0].revision.clone()),
        ),
        |path, _| Err(io_error(path, "injected commit failure")),
    );
    assert_eq!(result.unwrap_err().code, "memory.io");
    assert_eq!(
        std::fs::read(service.path(Scope::Task).unwrap()).unwrap(),
        before
    );
    assert_eq!(service.read().unwrap().task, original.task);
}
