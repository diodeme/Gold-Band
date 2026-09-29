use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use camino::Utf8Path;
use gold_band::git::GitMetadataWatchTarget;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::{CommandErrorVm, CommandResult};

pub(crate) const GIT_STATE_CHANGED_EVENT: &str = "gold-band://git-state-changed";
const EVENT_QUEUE_CAPACITY: usize = 1_024;
const MAX_BATCH_LATENCY: Duration = Duration::from_secs(1);
const MAX_DIAGNOSTIC_GROUPS: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStateChangedEventVm {
    pub project_id: String,
    pub repository_common_dir: String,
    pub workspace_path: String,
    pub reason: &'static str,
}

#[derive(Clone, Default)]
pub struct GitStateMonitorRuntime {
    inner: Arc<Mutex<HashMap<String, MonitorHandle>>>,
}

struct MonitorHandle {
    _watcher: RecommendedWatcher,
    refs: usize,
}

impl GitStateMonitorRuntime {
    pub(crate) fn start(
        &self,
        app_handle: AppHandle,
        project_id: String,
        repository_common_dir: &Utf8Path,
        workspace_path: &Utf8Path,
        targets: Vec<GitMetadataWatchTarget>,
        debounce_ms: u64,
    ) -> CommandResult<()> {
        let key = monitor_key(&project_id, repository_common_dir, workspace_path);
        let mut monitors = self.lock()?;
        if let Some(handle) = monitors.get_mut(&key) {
            handle.refs = handle.refs.saturating_add(1);
            return Ok(());
        }

        let payload = GitStateChangedEventVm {
            project_id,
            repository_common_dir: repository_common_dir.to_string(),
            workspace_path: workspace_path.to_string(),
            reason: "metadata",
        };
        let watcher = create_metadata_watcher(app_handle, payload, targets, debounce_ms)?;
        monitors.insert(
            key,
            MonitorHandle {
                _watcher: watcher,
                refs: 1,
            },
        );
        Ok(())
    }

    pub(crate) fn stop(
        &self,
        project_id: &str,
        repository_common_dir: &Utf8Path,
        workspace_path: &Utf8Path,
    ) -> CommandResult<()> {
        let key = monitor_key(project_id, repository_common_dir, workspace_path);
        let mut monitors = self.lock()?;
        let remove = monitors.get_mut(&key).is_some_and(|handle| {
            handle.refs = handle.refs.saturating_sub(1);
            handle.refs == 0
        });
        if remove {
            monitors.remove(&key);
        }
        Ok(())
    }

    fn lock(&self) -> CommandResult<std::sync::MutexGuard<'_, HashMap<String, MonitorHandle>>> {
        self.inner
            .lock()
            .map_err(|_| CommandErrorVm::new("git.state-monitor-failed", serde_json::json!({})))
    }
}

fn create_metadata_watcher(
    app_handle: AppHandle,
    payload: GitStateChangedEventVm,
    targets: Vec<GitMetadataWatchTarget>,
    debounce_ms: u64,
) -> CommandResult<RecommendedWatcher> {
    let (sender, receiver) =
        mpsc::sync_channel::<notify::Result<notify::Event>>(EVENT_QUEUE_CAPACITY);
    let queue_overflowed = Arc::new(AtomicBool::new(false));
    let callback_overflowed = queue_overflowed.clone();
    let mut watcher = notify::recommended_watcher(move |event| match sender.try_send(event) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) => callback_overflowed.store(true, Ordering::Release),
        Err(mpsc::TrySendError::Disconnected(_)) => {}
    })
    .map_err(|_| monitor_error(&payload))?;
    for target in &targets {
        let mode = if target.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher
            .watch(target.path.as_std_path(), mode)
            .map_err(|_| monitor_error(&payload))?;
    }

    let debounce = Duration::from_millis(debounce_ms.max(1));
    // Diagnostic identity only; never a second repository/workspace identity.
    let monitor_id = uuid::Uuid::new_v4();
    let diagnostic_roots = targets
        .iter()
        .map(|target| normalize_path(&target.path))
        .collect::<Vec<_>>();
    tracing::debug!(target: "gold_band::git::load", event = "metadata_monitor_start",
        %monitor_id, watch_roots = targets.len());
    std::thread::spawn(move || {
        let mut batch_number = 0u64;
        while let Ok(first) = receiver.recv() {
            batch_number = batch_number.wrapping_add(1);
            let batch_started = Instant::now();
            let mut events = 1usize;
            let mut overflowed = queue_overflowed.swap(false, Ordering::AcqRel);
            let mut changed = overflowed;
            let mut diagnostic =
                tracing::enabled!(target: "gold_band::git::load", tracing::Level::DEBUG)
                    .then(MetadataBatchDiagnostic::default);
            if let Some(diagnostic) = &mut diagnostic {
                diagnostic.observe(&first, &diagnostic_roots, batch_started.elapsed());
            }
            changed |= monitor_event_invalidates(first);
            loop {
                let wait = next_batch_wait(debounce, batch_started.elapsed());
                if wait.is_zero() {
                    break;
                }
                match receiver.recv_timeout(wait) {
                    Ok(event) => {
                        events += 1;
                        if let Some(diagnostic) = &mut diagnostic {
                            diagnostic.observe(&event, &diagnostic_roots, batch_started.elapsed());
                        }
                        changed |= monitor_event_invalidates(event);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            overflowed |= queue_overflowed.swap(false, Ordering::AcqRel);
            changed |= overflowed;
            tracing::debug!(target: "gold_band::git::load", event = "metadata_batch",
                %monitor_id, batch_number, events, overflowed, invalidated = changed,
                elapsed_ms = batch_started.elapsed().as_secs_f64() * 1000.0);
            if let Some(diagnostic) = diagnostic {
                diagnostic.record(monitor_id, batch_number);
            }
            if changed {
                if let Err(error) = app_handle.emit(GIT_STATE_CHANGED_EVENT, payload.clone()) {
                    tracing::warn!(%error, "failed to emit Git state invalidation");
                }
            }
        }
    });
    Ok(watcher)
}

#[derive(Debug, Default)]
struct MetadataBatchDiagnostic {
    // Keys contain only enum values and static allowlisted categories.
    groups: HashMap<(notify::EventKind, &'static str), (usize, f64, f64)>,
    rescans: usize,
    errors: usize,
    omitted_paths: usize,
}

impl MetadataBatchDiagnostic {
    fn observe(
        &mut self,
        event: &notify::Result<notify::Event>,
        roots: &[String],
        elapsed: Duration,
    ) {
        let Ok(event) = event else {
            self.errors += 1;
            return;
        };
        self.rescans += usize::from(event.need_rescan());
        if event.paths.is_empty() {
            self.add(event.kind, "no-path", elapsed);
        }
        for path in &event.paths {
            let category = Utf8Path::from_path(path)
                .map(|path| metadata_category(&normalize_path(path), roots))
                .unwrap_or("other");
            self.add(event.kind, category, elapsed);
        }
    }

    fn add(&mut self, kind: notify::EventKind, category: &'static str, elapsed: Duration) {
        let key = (kind, category);
        if !self.groups.contains_key(&key) && self.groups.len() >= MAX_DIAGNOSTIC_GROUPS {
            self.omitted_paths += 1;
            return;
        }
        let ms = elapsed.as_secs_f64() * 1000.0;
        let entry = self.groups.entry(key).or_insert((0, ms, ms));
        entry.0 += 1;
        entry.2 = ms;
    }

    fn record(&self, monitor_id: uuid::Uuid, batch_number: u64) {
        tracing::debug!(target: "gold_band::git::load", event = "metadata_batch_details",
            %monitor_id, batch_number, rescans = self.rescans, errors = self.errors,
            omitted_paths = self.omitted_paths);
        for ((kind, category), (path_occurrences, first_ms, last_ms)) in &self.groups {
            tracing::debug!(target: "gold_band::git::load", event = "metadata_event_group",
                %monitor_id, batch_number, kind = ?kind, category,
                path_occurrences, first_ms, last_ms);
        }
    }
}

// Paths are compared lexically against already registered watch roots. No stat,
// canonicalization, branch names or arbitrary filenames enter the diagnostic.
fn metadata_category(path: &str, roots: &[String]) -> &'static str {
    let root = roots
        .iter()
        .filter(|root| {
            path == root.as_str()
                || path
                    .strip_prefix(root.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
        .max_by_key(|root| root.len());
    let Some(root) = root else {
        return "outside-watch-root";
    };
    let root_name = root.rsplit('/').next().unwrap_or("");
    if matches!(root_name, "refs" | "rebase-merge" | "rebase-apply") {
        return metadata_name_category(root_name);
    }
    let relative = path[root.len()..].trim_start_matches('/');
    if relative.is_empty() {
        return "watch-root";
    }
    metadata_name_category(relative.split('/').next().unwrap_or(""))
}

fn metadata_name_category(name: &str) -> &'static str {
    if name.eq_ignore_ascii_case("HEAD") {
        return "HEAD";
    }
    if name.eq_ignore_ascii_case("HEAD.lock") {
        return "HEAD.lock";
    }
    if name.eq_ignore_ascii_case("MERGE_HEAD") {
        return "MERGE_HEAD";
    }
    if name.eq_ignore_ascii_case("REBASE_HEAD") {
        return "REBASE_HEAD";
    }
    match name {
        "index" => "index",
        "index.lock" => "index.lock",
        "packed-refs" => "packed-refs",
        "packed-refs.lock" => "packed-refs.lock",
        "refs" => "refs",
        "rebase-merge" => "rebase-merge",
        "rebase-apply" => "rebase-apply",
        "config" => "config",
        "config.lock" => "config.lock",
        "objects" => "objects",
        "logs" => "logs",
        _ => "other",
    }
}

fn monitor_event_invalidates(event: notify::Result<notify::Event>) -> bool {
    match event {
        Ok(event) if event.need_rescan() => true,
        // Lock files are unpublished transactions. A rename to the destination
        // still invalidates because at least one path is not a lock file.
        Ok(event)
            if !event.paths.is_empty()
                && event.paths.iter().all(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "lock")
                }) =>
        {
            false
        }
        Ok(event) => match event.kind {
            notify::EventKind::Access(notify::event::AccessKind::Close(
                notify::event::AccessMode::Write,
            )) => true,
            notify::EventKind::Access(_) => false,
            _ => true,
        },
        Err(_) => {
            tracing::warn!("Git metadata watcher reported an error; invalidating repository state");
            true
        }
    }
}

fn next_batch_wait(debounce: Duration, elapsed: Duration) -> Duration {
    debounce.min(MAX_BATCH_LATENCY.saturating_sub(elapsed))
}

fn monitor_error(payload: &GitStateChangedEventVm) -> CommandErrorVm {
    CommandErrorVm::new(
        "git.state-monitor-failed",
        serde_json::json!({
            "projectId": payload.project_id,
            "workspacePath": payload.workspace_path,
        }),
    )
}

fn monitor_key(
    project_id: &str,
    repository_common_dir: &Utf8Path,
    workspace_path: &Utf8Path,
) -> String {
    let common = normalize_path(repository_common_dir);
    let workspace = normalize_path(workspace_path);
    format!("{project_id}\0{common}\0{workspace}")
}

fn normalize_path(path: &Utf8Path) -> String {
    let path = path.as_str().replace('\\', "/");
    #[cfg(target_os = "windows")]
    let path = path.to_lowercase();
    path.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_index_lock_does_not_invalidate_but_committed_index_does() {
        use notify::{
            Event, EventKind,
            event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
        };
        for kind in [
            EventKind::Create(CreateKind::Any),
            EventKind::Remove(RemoveKind::Any),
        ] {
            assert!(!monitor_event_invalidates(Ok(
                Event::new(kind).add_path("D:/repo/.git/index.lock".into())
            )));
        }
        assert!(monitor_event_invalidates(Ok(Event::new(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
        )
        .add_path("D:/repo/.git/index.lock".into())
        .add_path("D:/repo/.git/index".into()))));
    }

    #[test]
    fn metadata_diagnostics_classify_main_and_linked_worktree_without_private_names() {
        let roots = [
            "D:/PRIVATE/.git",
            "D:/PRIVATE/.git/refs",
            "D:/PRIVATE/.git/worktrees/SECRET",
        ]
        .map(|path| normalize_path(Utf8Path::new(path)));
        for (path, expected) in [
            ("D:/PRIVATE/.git/index", "index"),
            ("D:/PRIVATE/.git/index.lock", "index.lock"),
            ("D:/PRIVATE/.git/HEAD", "HEAD"),
            ("D:/PRIVATE/.git/refs/heads/SECRET", "refs"),
            ("D:/PRIVATE/.git/worktrees/SECRET/index.lock", "index.lock"),
            ("D:/PRIVATE/.git/rebase-merge/SECRET", "rebase-merge"),
            ("D:/PRIVATE/.git/SECRET", "other"),
            ("D:/PRIVATE/.git-other/index", "outside-watch-root"),
        ] {
            assert_eq!(
                metadata_category(&normalize_path(Utf8Path::new(path)), &roots),
                expected
            );
        }
    }

    #[test]
    fn metadata_diagnostics_aggregate_rename_paths_access_errors_and_rescans() {
        use notify::event::{AccessKind, Flag, ModifyKind, RenameMode};
        let roots = vec![normalize_path(Utf8Path::new("D:/PRIVATE/.git"))];
        let rename = notify::Event::new(notify::EventKind::Modify(ModifyKind::Name(
            RenameMode::Both,
        )))
        .add_path("D:/PRIVATE/.git/index.lock".into())
        .add_path("D:/PRIVATE/.git/index".into());
        let mut diagnostic = MetadataBatchDiagnostic::default();
        for ms in [0, 8] {
            diagnostic.observe(&Ok(rename.clone()), &roots, Duration::from_millis(ms));
        }
        let key = (rename.kind, "index.lock");
        assert_eq!(diagnostic.groups[&key], (2, 0.0, 8.0));
        assert_eq!(diagnostic.groups[&(rename.kind, "index")].0, 2);
        diagnostic.observe(
            &Ok(
                notify::Event::new(notify::EventKind::Access(AccessKind::Read))
                    .set_flag(Flag::Rescan),
            ),
            &roots,
            Duration::ZERO,
        );
        diagnostic.observe(
            &Err(notify::Error::generic("PRIVATE_ERROR")),
            &roots,
            Duration::ZERO,
        );
        assert_eq!(diagnostic.rescans, 1);
        assert_eq!(diagnostic.errors, 1);
        assert!(!format!("{diagnostic:?}").contains("PRIVATE"));
    }

    #[test]
    fn metadata_diagnostics_bound_groups_and_coalesce_repeated_paths() {
        use notify::event::{
            AccessKind, CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode,
        };
        let mut diagnostic = MetadataBatchDiagnostic::default();
        for kind in [
            notify::EventKind::Any,
            notify::EventKind::Other,
            notify::EventKind::Create(CreateKind::Any),
            notify::EventKind::Remove(RemoveKind::Any),
            notify::EventKind::Modify(ModifyKind::Any),
            notify::EventKind::Access(AccessKind::Any),
            notify::EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            notify::EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
        ] {
            for category in [
                "index",
                "index.lock",
                "HEAD",
                "refs",
                "config",
                "objects",
                "logs",
                "other",
            ] {
                diagnostic.add(kind, category, Duration::ZERO);
            }
        }
        diagnostic.add(notify::EventKind::Other, "no-path", Duration::ZERO);
        assert_eq!(diagnostic.groups.len(), MAX_DIAGNOSTIC_GROUPS);
        assert_eq!(diagnostic.omitted_paths, 1);
        diagnostic.add(notify::EventKind::Any, "index", Duration::from_millis(1));
        assert_eq!(diagnostic.groups[&(notify::EventKind::Any, "index")].0, 2);
    }

    #[test]
    fn read_access_does_not_invalidate_metadata() {
        assert!(!monitor_event_invalidates(Ok(notify::Event::new(
            notify::EventKind::Access(notify::event::AccessKind::Read),
        ))));
    }

    #[test]
    fn writes_renames_removals_and_uncertainty_still_invalidate() {
        use notify::event::{
            AccessKind, AccessMode, CreateKind, Flag, ModifyKind, RemoveKind, RenameMode,
        };
        for kind in [
            notify::EventKind::Create(CreateKind::File),
            notify::EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            notify::EventKind::Remove(RemoveKind::File),
            notify::EventKind::Access(AccessKind::Close(AccessMode::Write)),
        ] {
            assert!(monitor_event_invalidates(Ok(notify::Event::new(kind))));
        }
        assert!(monitor_event_invalidates(Ok(notify::Event::new(
            notify::EventKind::Access(AccessKind::Read),
        )
        .set_flag(Flag::Rescan))));
        assert!(monitor_event_invalidates(Err(notify::Error::generic(
            "test"
        ))));
    }

    #[test]
    fn monitor_identity_is_repository_and_worktree_scoped() {
        let common = Utf8Path::new("D:/repo/.git");
        assert_ne!(
            monitor_key("project-1", common, Utf8Path::new("D:/repo")),
            monitor_key("project-1", common, Utf8Path::new("D:/worktree")),
        );
        assert_ne!(
            monitor_key("project-1", common, Utf8Path::new("D:/repo")),
            monitor_key("project-2", common, Utf8Path::new("D:/repo")),
        );
    }

    #[test]
    fn continuous_metadata_events_cannot_starve_refresh() {
        assert_eq!(
            next_batch_wait(Duration::from_millis(150), Duration::from_millis(999)),
            Duration::from_millis(1),
        );
    }
}
