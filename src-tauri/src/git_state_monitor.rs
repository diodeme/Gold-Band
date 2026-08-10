use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use camino::Utf8Path;
use gold_band::git::GitMetadataWatchTarget;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::{CommandErrorVm, CommandResult};

pub(crate) const GIT_STATE_CHANGED_EVENT: &str = "gold-band://git-state-changed";

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
        let key = monitor_key(repository_common_dir, workspace_path);
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
        repository_common_dir: &Utf8Path,
        workspace_path: &Utf8Path,
    ) -> CommandResult<()> {
        let key = monitor_key(repository_common_dir, workspace_path);
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
    let (sender, receiver) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
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
            let mut changed = first.is_ok();
            loop {
                match receiver.recv_timeout(debounce) {
                    Ok(event) => changed |= event.is_ok(),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            if changed {
                let _ = app_handle.emit(GIT_STATE_CHANGED_EVENT, payload.clone());
            }
        }
    });
    Ok(watcher)
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

fn monitor_key(repository_common_dir: &Utf8Path, workspace_path: &Utf8Path) -> String {
    let common = normalize_path(repository_common_dir);
    let workspace = normalize_path(workspace_path);
    format!("{common}\0{workspace}")
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
            monitor_key(common, Utf8Path::new("D:/repo")),
            monitor_key(common, Utf8Path::new("D:/worktree")),
        );
    }
}
