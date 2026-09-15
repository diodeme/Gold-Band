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
    for target in targets {
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
    std::thread::spawn(move || {
        while let Ok(first) = receiver.recv() {
            let batch_started = Instant::now();
            let mut changed = queue_overflowed.swap(false, Ordering::AcqRel);
            changed |= monitor_event_invalidates(first);
            loop {
                let wait = next_batch_wait(debounce, batch_started.elapsed());
                if wait.is_zero() {
                    break;
                }
                match receiver.recv_timeout(wait) {
                    Ok(event) => changed |= monitor_event_invalidates(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            changed |= queue_overflowed.swap(false, Ordering::AcqRel);
            if changed {
                if let Err(error) = app_handle.emit(GIT_STATE_CHANGED_EVENT, payload.clone()) {
                    tracing::warn!(%error, "failed to emit Git state invalidation");
                }
            }
        }
    });
    Ok(watcher)
}

fn monitor_event_invalidates(event: notify::Result<notify::Event>) -> bool {
    if let Err(error) = event {
        tracing::warn!(%error, "Git metadata watcher reported an error; invalidating repository state");
    }
    true
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
