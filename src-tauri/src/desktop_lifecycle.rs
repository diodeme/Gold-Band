use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, WebviewWindow, WebviewWindowBuilder};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::commands::{CommandErrorVm, CommandResult, prepare_app_exit_inner};
use crate::state::DesktopState;

pub const APP_EXIT_REQUESTED_EVENT: &str = "gold-band://app-exit-requested";
pub const APP_EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(15);
const APP_EXIT_FRONTEND_TIMEOUT: Duration = Duration::from_secs(15);
const WINDOW_FOCUS_TIMEOUT: Duration = Duration::from_secs(15);
const WINDOW_FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DesktopLifecycleState {
    Running,
    #[allow(dead_code)]
    ClosingMainWindow,
    AwaitingFrontend,
    Cleaning,
    ReadyToExit,
}

#[derive(Debug)]
struct LifecycleInner {
    state: DesktopLifecycleState,
    exit_request_id: Option<String>,
    completion_action: ExitCompletionAction,
}

impl Default for LifecycleInner {
    fn default() -> Self {
        Self {
            state: DesktopLifecycleState::Running,
            exit_request_id: None,
            completion_action: ExitCompletionAction::Exit,
        }
    }
}

#[derive(Debug, Default)]
pub struct DesktopLifecycleCoordinator {
    inner: Mutex<LifecycleInner>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExitRequestPayload {
    pub request_id: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AppExitDecision {
    Proceed,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitCompletionAction {
    Exit,
    Restart,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAppExitInput {
    pub request_id: String,
    pub decision: AppExitDecision,
}

enum ExitRequestedAction {
    Prevent,
    RequestFrontend(AppExitRequestPayload),
    Cleanup,
    Allow,
}

impl DesktopLifecycleCoordinator {
    #[cfg(test)]
    fn state(&self) -> DesktopLifecycleState {
        self.inner
            .lock()
            .map(|inner| inner.state)
            .unwrap_or(DesktopLifecycleState::Cleaning)
    }

    #[cfg(any(target_os = "macos", test))]
    fn begin_main_window_close(&self) -> CommandResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lifecycle_error("state-poisoned"))?;
        if inner.state != DesktopLifecycleState::Running {
            return Err(lifecycle_error("close-not-allowed"));
        }
        inner.state = DesktopLifecycleState::ClosingMainWindow;
        Ok(())
    }

    #[cfg(any(target_os = "macos", test))]
    fn cancel_main_window_close(&self) {
        if let Ok(mut inner) = self.inner.lock()
            && inner.state == DesktopLifecycleState::ClosingMainWindow
        {
            inner.state = DesktopLifecycleState::Running;
        }
    }

    fn begin_cleanup(&self, completion_action: ExitCompletionAction) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if matches!(
            inner.state,
            DesktopLifecycleState::Cleaning | DesktopLifecycleState::ReadyToExit
        ) {
            return false;
        }
        inner.state = DesktopLifecycleState::Cleaning;
        inner.exit_request_id = None;
        inner.completion_action = completion_action;
        true
    }

    fn on_exit_requested(
        &self,
        has_window: bool,
        completion_action: ExitCompletionAction,
    ) -> ExitRequestedAction {
        let Ok(mut inner) = self.inner.lock() else {
            return ExitRequestedAction::Prevent;
        };
        match inner.state {
            DesktopLifecycleState::ReadyToExit => ExitRequestedAction::Allow,
            DesktopLifecycleState::ClosingMainWindow => {
                inner.state = DesktopLifecycleState::Running;
                ExitRequestedAction::Prevent
            }
            DesktopLifecycleState::AwaitingFrontend | DesktopLifecycleState::Cleaning => {
                ExitRequestedAction::Prevent
            }
            DesktopLifecycleState::Running if has_window => {
                let request_id = Uuid::new_v4().to_string();
                inner.state = DesktopLifecycleState::AwaitingFrontend;
                inner.exit_request_id = Some(request_id.clone());
                inner.completion_action = completion_action;
                ExitRequestedAction::RequestFrontend(AppExitRequestPayload { request_id })
            }
            DesktopLifecycleState::Running => {
                inner.state = DesktopLifecycleState::Cleaning;
                inner.completion_action = completion_action;
                ExitRequestedAction::Cleanup
            }
        }
    }

    fn resolve_exit(&self, input: &ResolveAppExitInput) -> CommandResult<bool> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lifecycle_error("state-poisoned"))?;
        if inner.state != DesktopLifecycleState::AwaitingFrontend
            || inner.exit_request_id.as_deref() != Some(input.request_id.as_str())
        {
            return Err(lifecycle_error("exit-request-stale"));
        }
        inner.exit_request_id = None;
        match input.decision {
            AppExitDecision::Proceed => {
                inner.state = DesktopLifecycleState::Cleaning;
                Ok(true)
            }
            AppExitDecision::Cancel => {
                inner.state = DesktopLifecycleState::Running;
                inner.completion_action = ExitCompletionAction::Exit;
                Ok(false)
            }
        }
    }

    fn cancel_expired_request(&self, request_id: &str) {
        if let Ok(mut inner) = self.inner.lock()
            && inner.state == DesktopLifecycleState::AwaitingFrontend
            && inner.exit_request_id.as_deref() == Some(request_id)
        {
            inner.state = DesktopLifecycleState::Running;
            inner.exit_request_id = None;
            inner.completion_action = ExitCompletionAction::Exit;
            warn!(
                request_id,
                "application exit cancelled after frontend handshake timeout"
            );
        }
    }

    fn mark_ready_to_exit(&self) -> ExitCompletionAction {
        if let Ok(mut inner) = self.inner.lock() {
            inner.state = DesktopLifecycleState::ReadyToExit;
            inner.exit_request_id = None;
            return inner.completion_action;
        }
        ExitCompletionAction::Exit
    }

    #[cfg(target_os = "macos")]
    fn reset_after_reopen(&self) {
        if let Ok(mut inner) = self.inner.lock()
            && inner.state == DesktopLifecycleState::ClosingMainWindow
        {
            inner.state = DesktopLifecycleState::Running;
        }
    }
}

fn lifecycle_error(suffix: &str) -> CommandErrorVm {
    CommandErrorVm::new(format!("desktop-lifecycle.{suffix}"), serde_json::json!({}))
}

#[tauri::command]
pub fn complete_main_window_close(
    app_handle: AppHandle,
    lifecycle: tauri::State<'_, DesktopLifecycleCoordinator>,
) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        lifecycle.begin_main_window_close()?;
        let Some(window) = app_handle.get_webview_window("main") else {
            lifecycle.cancel_main_window_close();
            return Err(lifecycle_error("main-window-missing"));
        };
        if let Err(error) = window.destroy() {
            lifecycle.cancel_main_window_close();
            return Err(CommandErrorVm::new(
                "desktop-lifecycle.main-window-close-failed",
                serde_json::json!({ "reason": error.to_string() }),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        if lifecycle.begin_cleanup(ExitCompletionAction::Exit) {
            spawn_cleanup(app_handle);
        }
        Ok(())
    }
}

#[tauri::command]
pub fn resolve_app_exit(
    app_handle: AppHandle,
    lifecycle: tauri::State<'_, DesktopLifecycleCoordinator>,
    input: ResolveAppExitInput,
) -> CommandResult<()> {
    if lifecycle.resolve_exit(&input)? {
        spawn_cleanup(app_handle);
    }
    Ok(())
}

pub fn ensure_main_window<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> anyhow::Result<WebviewWindow<R>> {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(window);
    }

    let config = app_handle
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("main window config is missing"))?;
    let window = WebviewWindowBuilder::from_config(app_handle, &config)?.build()?;
    focus_window_after_bootstrap(app_handle.clone());
    Ok(window)
}

pub fn request_app_restart(app_handle: &AppHandle) -> CommandResult<()> {
    let lifecycle = app_handle.state::<DesktopLifecycleCoordinator>();
    match lifecycle.on_exit_requested(
        app_handle.get_webview_window("main").is_some(),
        ExitCompletionAction::Restart,
    ) {
        ExitRequestedAction::RequestFrontend(payload) => {
            emit_frontend_exit_request(app_handle, &lifecycle, payload)
        }
        ExitRequestedAction::Cleanup => {
            spawn_cleanup(app_handle.clone());
            Ok(())
        }
        ExitRequestedAction::Prevent => Err(lifecycle_error("exit-already-in-progress")),
        ExitRequestedAction::Allow => {
            app_handle.request_restart();
            Ok(())
        }
    }
}

fn focus_window_after_bootstrap<R: Runtime>(app_handle: AppHandle<R>) {
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + WINDOW_FOCUS_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let Some(window) = app_handle.get_webview_window("main") else {
                return;
            };
            if window.is_visible().unwrap_or(false) {
                let _ = window.unminimize();
                let _ = window.set_focus();
                return;
            }
            std::thread::sleep(WINDOW_FOCUS_POLL_INTERVAL);
        }
    });
}

pub fn handle_run_event(app_handle: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { api, .. } => {
            let lifecycle = app_handle.state::<DesktopLifecycleCoordinator>();
            let action = lifecycle.on_exit_requested(
                app_handle.get_webview_window("main").is_some(),
                ExitCompletionAction::Exit,
            );
            match action {
                ExitRequestedAction::Allow => {}
                ExitRequestedAction::Prevent => api.prevent_exit(),
                ExitRequestedAction::Cleanup => {
                    api.prevent_exit();
                    spawn_cleanup(app_handle.clone());
                }
                ExitRequestedAction::RequestFrontend(payload) => {
                    api.prevent_exit();
                    let _ = emit_frontend_exit_request(app_handle, &lifecycle, payload);
                }
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            app_handle
                .state::<DesktopLifecycleCoordinator>()
                .reset_after_reopen();
            if let Err(error) = ensure_main_window(app_handle) {
                warn!(?error, "failed to reopen main window from macOS Dock");
            }
        }
        RunEvent::Resumed => {
            if let Ok(coordinator) = app_handle.state::<DesktopState>().scheduler_coordinator() {
                let _ = coordinator.send(crate::scheduled_runtime::SchedulerCommand::Reconcile {
                    reason: gold_band::scheduler::coordinator::ReconcileReason::SystemResume,
                });
            }
        }
        _ => {}
    }
}

fn emit_frontend_exit_request(
    app_handle: &AppHandle,
    lifecycle: &DesktopLifecycleCoordinator,
    payload: AppExitRequestPayload,
) -> CommandResult<()> {
    if let Err(error) = app_handle.emit_to("main", APP_EXIT_REQUESTED_EVENT, &payload) {
        warn!(?error, "failed to request frontend application exit");
        lifecycle.cancel_expired_request(&payload.request_id);
        return Err(lifecycle_error("frontend-request-failed"));
    }
    let handle = app_handle.clone();
    let request_id = payload.request_id;
    std::thread::spawn(move || {
        std::thread::sleep(APP_EXIT_FRONTEND_TIMEOUT);
        handle
            .state::<DesktopLifecycleCoordinator>()
            .cancel_expired_request(&request_id);
    });
    Ok(())
}

fn spawn_cleanup(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let cleanup =
            prepare_app_exit_inner(&app_handle, app_handle.state::<DesktopState>().inner());
        match tokio::time::timeout(APP_EXIT_CLEANUP_TIMEOUT, cleanup).await {
            Ok(result) => {
                for warning in result.warnings {
                    warn!(warning_code = %warning.code, "application exit completed with warning");
                }
            }
            Err(_) => {
                warn!("application exit cleanup timed out; forcing managed process groups to stop");
                gold_band::process::force_terminate_all_managed_process_groups();
            }
        }
        let completion_action = app_handle
            .state::<DesktopLifecycleCoordinator>()
            .mark_ready_to_exit();
        debug!("application exit cleanup complete");
        match completion_action {
            ExitCompletionAction::Exit => app_handle.exit(0),
            ExitCompletionAction::Restart => app_handle.request_restart(),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::WebviewUrl;

    #[test]
    fn window_only_close_prevents_last_window_exit() {
        let lifecycle = DesktopLifecycleCoordinator::default();
        lifecycle.begin_main_window_close().unwrap();
        assert!(matches!(
            lifecycle.on_exit_requested(false, ExitCompletionAction::Exit),
            ExitRequestedAction::Prevent
        ));
        assert_eq!(lifecycle.state(), DesktopLifecycleState::Running);
    }

    #[test]
    fn native_quit_waits_for_frontend_and_cleans_once() {
        let lifecycle = DesktopLifecycleCoordinator::default();
        let ExitRequestedAction::RequestFrontend(request) =
            lifecycle.on_exit_requested(true, ExitCompletionAction::Exit)
        else {
            panic!("expected frontend request");
        };
        assert!(matches!(
            lifecycle.on_exit_requested(true, ExitCompletionAction::Exit),
            ExitRequestedAction::Prevent
        ));
        assert!(
            lifecycle
                .resolve_exit(&ResolveAppExitInput {
                    request_id: request.request_id,
                    decision: AppExitDecision::Proceed,
                })
                .unwrap()
        );
        assert!(!lifecycle.begin_cleanup(ExitCompletionAction::Exit));
    }

    #[test]
    fn cancelling_native_quit_returns_to_running() {
        let lifecycle = DesktopLifecycleCoordinator::default();
        let ExitRequestedAction::RequestFrontend(request) =
            lifecycle.on_exit_requested(true, ExitCompletionAction::Exit)
        else {
            panic!("expected frontend request");
        };
        assert!(
            !lifecycle
                .resolve_exit(&ResolveAppExitInput {
                    request_id: request.request_id,
                    decision: AppExitDecision::Cancel,
                })
                .unwrap()
        );
        assert_eq!(lifecycle.state(), DesktopLifecycleState::Running);
    }

    #[test]
    fn updater_restart_is_preserved_until_cleanup_finishes() {
        let lifecycle = DesktopLifecycleCoordinator::default();
        let ExitRequestedAction::RequestFrontend(request) =
            lifecycle.on_exit_requested(true, ExitCompletionAction::Restart)
        else {
            panic!("expected frontend request");
        };
        assert!(
            lifecycle
                .resolve_exit(&ResolveAppExitInput {
                    request_id: request.request_id,
                    decision: AppExitDecision::Proceed,
                })
                .unwrap()
        );
        assert_eq!(
            lifecycle.mark_ready_to_exit(),
            ExitCompletionAction::Restart
        );
    }

    #[test]
    fn second_launch_reuses_the_existing_main_window() {
        let app = tauri::test::mock_app();
        WebviewWindowBuilder::new(app.handle(), "main", WebviewUrl::App("index.html".into()))
            .visible(false)
            .build()
            .unwrap();

        let restored = ensure_main_window(app.handle()).unwrap();

        assert_eq!(restored.label(), "main");
        assert_eq!(app.webview_windows().len(), 1);
    }
}
