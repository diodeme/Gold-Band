use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::commands::{CommandErrorVm, CommandResult};

use super::models::WorkspaceFileChangedEventVm;
use super::paths::{display_path, error};
use super::runtime::WorkspaceFileRuntime;
use super::service::revision_for_path;

pub(crate) const WORKSPACE_FILE_CHANGED_EVENT: &str = "gold-band://workspace-file-changed";
const EVENT_QUEUE_CAPACITY: usize = 4_096;
const MAX_PENDING_PATHS: usize = 4_096;
const MAX_BATCH_LATENCY: Duration = Duration::from_secs(1);

#[derive(Clone, Default)]
pub struct WorkspaceFileWatchRuntime {
    inner: Arc<Mutex<WatchRuntimeInner>>,
}

#[derive(Default)]
struct WatchRuntimeInner {
    workspace: HashMap<String, WatchHandle>,
    external: HashMap<String, WatchHandle>,
}

struct WatchHandle {
    _watcher: RecommendedWatcher,
    refs: usize,
    external_token: Option<Arc<Mutex<String>>>,
}

impl WorkspaceFileWatchRuntime {
    pub(crate) fn start_workspace(
        &self,
        app_handle: AppHandle,
        file_runtime: WorkspaceFileRuntime,
        project_id: String,
        root: PathBuf,
        debounce_ms: u64,
    ) -> CommandResult<()> {
        let mut inner = self.lock()?;
        let key = workspace_watch_key(&project_id, &root);
        if let Some(handle) = inner.workspace.get_mut(&key) {
            handle.refs = handle.refs.saturating_add(1);
            return Ok(());
        }
        let watcher = create_watcher(
            app_handle,
            file_runtime,
            project_id.clone(),
            root.clone(),
            None,
            debounce_ms,
            RecursiveMode::Recursive,
            None,
        )?;
        inner.workspace.insert(
            key,
            WatchHandle {
                _watcher: watcher,
                refs: 1,
                external_token: None,
            },
        );
        Ok(())
    }

    pub(crate) fn stop_workspace(&self, project_id: &str, root: &Path) -> CommandResult<()> {
        let mut inner = self.lock()?;
        let key = workspace_watch_key(project_id, root);
        let remove = inner.workspace.get_mut(&key).is_some_and(|handle| {
            handle.refs = handle.refs.saturating_sub(1);
            handle.refs == 0
        });
        if remove {
            inner.workspace.remove(&key);
        }
        Ok(())
    }

    pub(crate) fn start_external(
        &self,
        app_handle: AppHandle,
        file_runtime: WorkspaceFileRuntime,
        token: String,
        project_id: String,
        path: PathBuf,
        debounce_ms: u64,
    ) -> CommandResult<()> {
        let mut inner = self.lock()?;
        if inner.external.contains_key(&token) {
            return Ok(());
        }
        let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let external_token = Arc::new(Mutex::new(token.clone()));
        let watcher = create_watcher(
            app_handle,
            file_runtime,
            project_id,
            parent,
            Some(path),
            debounce_ms,
            RecursiveMode::NonRecursive,
            Some(external_token.clone()),
        )?;
        inner.external.insert(
            token,
            WatchHandle {
                _watcher: watcher,
                refs: 1,
                external_token: Some(external_token),
            },
        );
        Ok(())
    }

    pub(crate) fn rotate_external(&self, old_token: &str, new_token: String) -> CommandResult<()> {
        let mut inner = self.lock()?;
        if let Some(handle) = inner.external.remove(old_token) {
            if let Some(token) = &handle.external_token
                && let Ok(mut token) = token.lock()
            {
                *token = new_token.clone();
            }
            inner.external.insert(new_token, handle);
        }
        Ok(())
    }

    pub(crate) fn stop_external(&self, token: &str) -> CommandResult<()> {
        self.lock()?.external.remove(token);
        Ok(())
    }

    fn lock(&self) -> CommandResult<std::sync::MutexGuard<'_, WatchRuntimeInner>> {
        self.inner
            .lock()
            .map_err(|_| CommandErrorVm::new("workspace-file.watch-failed", serde_json::json!({})))
    }
}

fn workspace_watch_key(project_id: &str, root: &Path) -> String {
    let path = display_path(root).replace('\\', "/");
    #[cfg(target_os = "windows")]
    let path = path.to_lowercase();
    format!("{project_id}\0{path}")
}

fn create_watcher(
    app_handle: AppHandle,
    file_runtime: WorkspaceFileRuntime,
    project_id: String,
    watched_path: PathBuf,
    target_file: Option<PathBuf>,
    debounce_ms: u64,
    recursive_mode: RecursiveMode,
    external_token: Option<Arc<Mutex<String>>>,
) -> CommandResult<RecommendedWatcher> {
    let (sender, receiver) = mpsc::sync_channel::<notify::Result<Event>>(EVENT_QUEUE_CAPACITY);
    let queue_overflowed = Arc::new(AtomicBool::new(false));
    let callback_overflowed = queue_overflowed.clone();
    let mut watcher = notify::recommended_watcher(move |event| match sender.try_send(event) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) => callback_overflowed.store(true, Ordering::Release),
        Err(mpsc::TrySendError::Disconnected(_)) => {}
    })
    .map_err(|_| {
        error(
            "workspace-file.watch-failed",
            serde_json::json!({ "projectId": project_id }),
        )
    })?;
    watcher.watch(&watched_path, recursive_mode).map_err(|_| {
        error(
            "workspace-file.watch-failed",
            serde_json::json!({
                "projectId": project_id,
                "path": display_path(&watched_path),
            }),
        )
    })?;

    let debounce = Duration::from_millis(debounce_ms.max(1));
    let invalidation_path = target_file.clone().unwrap_or_else(|| watched_path.clone());
    std::thread::spawn(move || {
        while let Ok(first) = receiver.recv() {
            let batch_started = Instant::now();
            let mut pending = HashMap::<PathBuf, String>::new();
            let mut invalidated = queue_overflowed.swap(false, Ordering::AcqRel)
                | collect_event(first, target_file.as_deref(), &mut pending);
            loop {
                let wait = next_batch_wait(debounce, batch_started.elapsed());
                if wait.is_zero() {
                    break;
                }
                match receiver.recv_timeout(wait) {
                    Ok(event) => {
                        invalidated |= collect_event(event, target_file.as_deref(), &mut pending);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            invalidated |= queue_overflowed.swap(false, Ordering::AcqRel);
            if invalidated {
                pending.clear();
                emit_change(
                    &app_handle,
                    WorkspaceFileChangedEventVm {
                        project_id: project_id.clone(),
                        canonical_path: display_path(&invalidation_path),
                        kind: "invalidated".to_string(),
                        revision: None,
                        operation_id: None,
                    },
                );
                continue;
            }
            for (path, kind) in pending {
                if let Some(token) = &external_token {
                    if !external_watch_authorized(&file_runtime, token, &project_id, &path) {
                        continue;
                    }
                }
                let recent_write = file_runtime.recent_write_for(&path);
                let revision = recent_write
                    .as_ref()
                    .filter(|_| path.is_file())
                    .and_then(|_| revision_for_path(&path).ok());
                let operation_id = revision.as_ref().and_then(|revision| {
                    recent_write
                        .filter(|(_, written_revision)| written_revision == revision)
                        .map(|(operation_id, _)| operation_id)
                });
                emit_change(
                    &app_handle,
                    WorkspaceFileChangedEventVm {
                        project_id: project_id.clone(),
                        canonical_path: display_path(&path),
                        kind,
                        revision,
                        operation_id,
                    },
                );
            }
        }
    });
    Ok(watcher)
}

fn emit_change(app_handle: &AppHandle, event: WorkspaceFileChangedEventVm) {
    if let Err(error) = app_handle.emit(WORKSPACE_FILE_CHANGED_EVENT, event) {
        tracing::warn!(%error, "failed to emit workspace file invalidation");
    }
}

fn next_batch_wait(debounce: Duration, elapsed: Duration) -> Duration {
    debounce.min(MAX_BATCH_LATENCY.saturating_sub(elapsed))
}

fn external_watch_authorized(
    runtime: &WorkspaceFileRuntime,
    token: &Arc<Mutex<String>>,
    project_id: &str,
    path: &Path,
) -> bool {
    token.lock().ok().is_some_and(|token| {
        runtime
            .validate_external_grant(Some(token.as_str()), project_id, path, "watch")
            .is_ok()
    })
}

fn collect_event(
    event: notify::Result<Event>,
    target_file: Option<&Path>,
    pending: &mut HashMap<PathBuf, String>,
) -> bool {
    let Ok(event) = event else {
        pending.clear();
        return true;
    };
    let kind = event_kind(&event.kind).to_string();
    for path in event.paths {
        if let Some(target) = target_file
            && path != target
            && std::fs::canonicalize(&path).ok().as_deref() != Some(target)
        {
            continue;
        }
        if !pending.contains_key(&path) && pending.len() >= MAX_PENDING_PATHS {
            pending.clear();
            return true;
        }
        pending.insert(path, kind.clone());
    }
    false
}

fn event_kind(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "created",
        EventKind::Remove(_) => "removed",
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => "renamed",
        _ => "modified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
    use tempfile::tempdir;

    #[test]
    fn maps_notify_events_to_the_public_change_kinds() {
        assert_eq!(event_kind(&EventKind::Create(CreateKind::File)), "created");
        assert_eq!(event_kind(&EventKind::Remove(RemoveKind::File)), "removed");
        assert_eq!(
            event_kind(&EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
            "renamed"
        );
        assert_eq!(event_kind(&EventKind::Modify(ModifyKind::Any)), "modified");
    }

    #[test]
    fn workspace_watch_identity_includes_the_canonical_root() {
        assert_ne!(
            workspace_watch_key("project-1", Path::new("D:/repo/worktree-a")),
            workspace_watch_key("project-1", Path::new("D:/repo/worktree-b")),
        );
    }

    #[test]
    fn external_watcher_filters_sibling_events_to_the_granted_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let sibling = dir.path().join("sibling.txt");
        std::fs::write(&target, "target").unwrap();
        std::fs::write(&sibling, "sibling").unwrap();
        let event = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(target.clone())
            .add_path(sibling);
        let mut pending = HashMap::new();

        collect_event(Ok(event), Some(&target), &mut pending);

        assert_eq!(pending.len(), 1);
        assert_eq!(pending.get(&target).map(String::as_str), Some("modified"));
    }

    #[test]
    fn debounced_event_collection_keeps_the_latest_kind_per_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("value.txt");
        let mut pending = HashMap::new();
        collect_event(
            Ok(Event::new(EventKind::Create(CreateKind::File)).add_path(path.clone())),
            None,
            &mut pending,
        );
        collect_event(
            Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path.clone())),
            None,
            &mut pending,
        );

        assert_eq!(pending.get(&path).map(String::as_str), Some("modified"));
    }

    #[test]
    fn watcher_errors_and_path_overflow_invalidate_the_scope() {
        let dir = tempdir().unwrap();
        let mut pending = HashMap::new();
        assert!(collect_event(
            Err(notify::Error::generic("watch failed")),
            None,
            &mut pending,
        ));

        let mut invalidated = false;
        for index in 0..=MAX_PENDING_PATHS {
            invalidated |= collect_event(
                Ok(Event::new(EventKind::Modify(ModifyKind::Any))
                    .add_path(dir.path().join(format!("{index}.txt")))),
                None,
                &mut pending,
            );
        }

        assert!(invalidated);
        assert!(pending.is_empty());
    }

    #[test]
    fn continuous_events_cannot_extend_a_batch_past_the_max_latency() {
        assert_eq!(
            next_batch_wait(Duration::from_millis(150), Duration::from_millis(999)),
            Duration::from_millis(1),
        );
        assert_eq!(
            next_batch_wait(Duration::from_millis(150), Duration::from_millis(1_000)),
            Duration::ZERO,
        );
    }

    #[test]
    fn external_event_authorization_tracks_token_rotation_and_release() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("external.txt");
        std::fs::write(&path, "external").unwrap();
        let runtime = WorkspaceFileRuntime::default();
        let first = runtime
            .issue_external_grant("project-1".to_string(), path.clone(), 30)
            .unwrap();
        let token = Arc::new(Mutex::new(first.token.clone()));
        assert!(external_watch_authorized(
            &runtime,
            &token,
            "project-1",
            &path
        ));

        let second = runtime.renew_external_grant(&first.token).unwrap();
        assert!(!external_watch_authorized(
            &runtime,
            &token,
            "project-1",
            &path
        ));
        *token.lock().unwrap() = second.token.clone();
        assert!(external_watch_authorized(
            &runtime,
            &token,
            "project-1",
            &path
        ));

        runtime.release_external_grant(&second.token).unwrap();
        assert!(!external_watch_authorized(
            &runtime,
            &token,
            "project-1",
            &path
        ));
    }
}
