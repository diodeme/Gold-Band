use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
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
    diagnostic_id: Option<uuid::Uuid>,
    workspace_scope: Option<String>,
}

#[derive(Clone, Copy)]
struct WorkspaceWatchDiagnostic {
    id: uuid::Uuid,
    scope_hash: u64,
}

impl WorkspaceFileWatchRuntime {
    pub(crate) fn start_workspace(
        &self,
        app_handle: AppHandle,
        file_runtime: WorkspaceFileRuntime,
        project_id: String,
        root: PathBuf,
        workspace_path: Option<String>,
        debounce_ms: u64,
    ) -> CommandResult<()> {
        let mut inner = self.lock()?;
        let key = workspace_watch_key(&project_id, &root);
        let scope_hash = workspace_watch_scope_hash(&key);
        let workspace_scope = normalized_workspace_scope(workspace_path.as_deref());
        let scope_kind = workspace_scope_kind(&workspace_scope);
        if let Some(handle) = inner.workspace.get_mut(&key) {
            if let Err(error) =
                ensure_workspace_watch_scope(&handle.workspace_scope, &workspace_scope, &project_id)
            {
                tracing::debug!(target: "gold_band::git::load", event = "workspace_watch_scope_conflict",
                    watch_id = ?handle.diagnostic_id, %project_id, scope_hash,
                    existing_scope_kind = workspace_scope_kind(&handle.workspace_scope),
                    requested_scope_kind = scope_kind);
                return Err(error);
            }
            handle.refs = handle.refs.saturating_add(1);
            tracing::debug!(target: "gold_band::git::load", event = "workspace_watch_reuse",
                watch_id = ?handle.diagnostic_id, %project_id, scope_hash,
                scope_kind, refs = handle.refs);
            return Ok(());
        }
        let diagnostic = WorkspaceWatchDiagnostic {
            id: uuid::Uuid::new_v4(),
            scope_hash,
        };
        let watcher = create_watcher(
            app_handle,
            file_runtime,
            project_id.clone(),
            root.clone(),
            None,
            workspace_path,
            debounce_ms,
            RecursiveMode::Recursive,
            None,
            Some(diagnostic),
        )?;
        inner.workspace.insert(
            key,
            WatchHandle {
                _watcher: watcher,
                refs: 1,
                external_token: None,
                diagnostic_id: Some(diagnostic.id),
                workspace_scope,
            },
        );
        tracing::debug!(target: "gold_band::git::load", event = "workspace_watch_start",
            watch_id = %diagnostic.id, %project_id, scope_hash,
            scope_kind, refs = 1usize, active_watches = inner.workspace.len());
        Ok(())
    }

    pub(crate) fn stop_workspace(&self, project_id: &str, root: &Path) -> CommandResult<()> {
        let mut inner = self.lock()?;
        let key = workspace_watch_key(project_id, root);
        let scope_hash = workspace_watch_scope_hash(&key);
        let (watch_id, refs, found) = match inner.workspace.get_mut(&key) {
            Some(handle) => {
                handle.refs = handle.refs.saturating_sub(1);
                (handle.diagnostic_id, handle.refs, true)
            }
            None => (None, 0, false),
        };
        let remove = found && refs == 0;
        if remove {
            inner.workspace.remove(&key);
        }
        tracing::debug!(target: "gold_band::git::load", event = "workspace_watch_stop",
            watch_id = ?watch_id, %project_id, scope_hash, found, removed = remove,
            refs, active_watches = inner.workspace.len());
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
            None,
            debounce_ms,
            RecursiveMode::NonRecursive,
            Some(external_token.clone()),
            None,
        )?;
        inner.external.insert(
            token,
            WatchHandle {
                _watcher: watcher,
                refs: 1,
                external_token: Some(external_token),
                diagnostic_id: None,
                workspace_scope: None,
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

fn workspace_watch_scope_hash(key: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

fn normalized_workspace_scope(workspace_path: Option<&str>) -> Option<String> {
    workspace_path.map(|path| {
        let path = display_path(Path::new(path)).replace('\\', "/");
        #[cfg(target_os = "windows")]
        let path = path.to_lowercase();
        path.trim_end_matches('/').to_string()
    })
}

fn workspace_scope_kind(scope: &Option<String>) -> &'static str {
    if scope.is_some() { "linked" } else { "main" }
}

fn ensure_workspace_watch_scope(
    existing: &Option<String>,
    requested: &Option<String>,
    project_id: &str,
) -> CommandResult<()> {
    if existing == requested {
        return Ok(());
    }
    Err(error(
        "workspace-file.watch-failed",
        serde_json::json!({ "projectId": project_id }),
    ))
}

fn create_watcher(
    app_handle: AppHandle,
    file_runtime: WorkspaceFileRuntime,
    project_id: String,
    watched_path: PathBuf,
    target_file: Option<PathBuf>,
    workspace_path: Option<String>,
    debounce_ms: u64,
    recursive_mode: RecursiveMode,
    external_token: Option<Arc<Mutex<String>>>,
    diagnostic: Option<WorkspaceWatchDiagnostic>,
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
    // Events carry the work location identity (None is the project root) so
    // consumers route them exactly; a vanished root (e.g. a reclaimed
    // worktree) invalidates the whole watch.
    let workspace_root =
        (target_file.is_none() && external_token.is_none()).then(|| watched_path.clone());
    std::thread::spawn(move || {
        let mut batch_number = 0u64;
        loop {
            let first = match receiver.recv() {
                Ok(first) => first,
                Err(_) => {
                    if let Some(diagnostic) = diagnostic {
                        tracing::debug!(target: "gold_band::git::load",
                            event = "workspace_watch_thread_stop", watch_id = %diagnostic.id,
                            scope_hash = diagnostic.scope_hash, reason = "channel-disconnected");
                    }
                    return;
                }
            };
            batch_number = batch_number.wrapping_add(1);
            let batch_started = Instant::now();
            let mut events = 1usize;
            let mut pending = HashMap::<PathBuf, String>::new();
            let mut overflowed = queue_overflowed.swap(false, Ordering::AcqRel);
            let mut invalidated =
                overflowed | collect_event(first, target_file.as_deref(), &mut pending);
            loop {
                let wait = next_batch_wait(debounce, batch_started.elapsed());
                if wait.is_zero() {
                    break;
                }
                match receiver.recv_timeout(wait) {
                    Ok(event) => {
                        events += 1;
                        invalidated |= collect_event(event, target_file.as_deref(), &mut pending);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(diagnostic) = diagnostic {
                            tracing::debug!(target: "gold_band::git::load",
                                event = "workspace_watch_thread_stop", watch_id = %diagnostic.id,
                                scope_hash = diagnostic.scope_hash, reason = "channel-disconnected");
                        }
                        return;
                    }
                }
            }
            overflowed |= queue_overflowed.swap(false, Ordering::AcqRel);
            invalidated |= overflowed;
            invalidated |= workspace_root.as_deref().is_some_and(|root| !root.is_dir());
            if let Some(diagnostic) = diagnostic {
                tracing::debug!(target: "gold_band::git::load", event = "workspace_batch",
                    watch_id = %diagnostic.id, scope_hash = diagnostic.scope_hash,
                    batch_number, events, overflowed, invalidated,
                    emitted_paths = if invalidated { 1 } else { pending.len() },
                    elapsed_ms = batch_started.elapsed().as_secs_f64() * 1000.0);
            }
            if invalidated {
                pending.clear();
                emit_change(
                    &app_handle,
                    WorkspaceFileChangedEventVm {
                        project_id: project_id.clone(),
                        workspace_path: workspace_path.clone(),
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
                        workspace_path: workspace_path.clone(),
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
    if event.need_rescan() {
        pending.clear();
        return true;
    }
    if matches!(event.kind, EventKind::Access(kind)
        if kind != notify::event::AccessKind::Close(notify::event::AccessMode::Write))
    {
        return false;
    }
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
    fn read_access_does_not_publish_workspace_changes() {
        let event = Event::new(EventKind::Access(notify::event::AccessKind::Read))
            .add_path(PathBuf::from("workspace/file.txt"));
        let mut pending = HashMap::new();
        assert!(!collect_event(Ok(event), None, &mut pending));
        assert!(pending.is_empty());
    }

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
    fn workspace_watch_reuse_requires_the_same_semantic_scope_in_either_start_order() {
        let linked = normalized_workspace_scope(Some(r"D:\repo\worktree"));
        let linked_with_slashes = normalized_workspace_scope(Some("D:/repo/worktree/"));
        let linked_with_verbatim_prefix = normalized_workspace_scope(Some(r"\\?\D:\repo\worktree"));
        let main = normalized_workspace_scope(None);

        assert_eq!(linked, linked_with_slashes);
        assert_eq!(linked, linked_with_verbatim_prefix);
        assert!(ensure_workspace_watch_scope(&main, &main, "project-1").is_ok());
        assert!(ensure_workspace_watch_scope(&linked, &linked_with_slashes, "project-1").is_ok());
        assert_eq!(
            ensure_workspace_watch_scope(&main, &linked, "project-1")
                .unwrap_err()
                .code,
            "workspace-file.watch-failed"
        );
        assert_eq!(
            ensure_workspace_watch_scope(&linked, &main, "project-1")
                .unwrap_err()
                .code,
            "workspace-file.watch-failed"
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
