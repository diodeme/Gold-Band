//! Pop-out "detached" windows for terminal sessions and the connections page.
//!
//! A detached window is a real OS window (Tauri v2 `WebviewWindow`) that loads the
//! same frontend entry with a query param telling `main.tsx` to render only that
//! resource (no full shell). Backend session state is a global singleton and
//! terminal events broadcast to all windows, so a detached window drives the SAME
//! session/manager as the main window for free — this module only creates and
//! tears down the window, and lets it route to the right isolated UI.
//!
//! Semantic model (agreed): the detached window's native close button ends the
//! session; a separate "return to main window" action closes the window (the
//! session keeps running and reappears in the main dock). Detached windows are
//! lifecycle children of the main window: they are enumerated + closed on app
//! exit, and they never participate in the main window exit/close handshake.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::debug;

use crate::commands::{CommandErrorVm, CommandResult};

/// Prefix for every detached window label so we can enumerate them on exit and
/// distinguish them from the main window.
pub const DETACHED_LABEL_PREFIX: &str = "detached-";

/// Frontend event emitted when a detached window is closed by its OS chrome. The
/// main window listens so it can refresh its session list (tab reappears / is
/// removed consistently).
pub const DETACHED_WINDOW_CLOSED_EVENT: &str = "gold-band://detached-window-closed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DetachedWindowKind {
    /// A single terminal session, keyed by `session_id`.
    Terminal,
    /// The connections management page (a whole page, not a session).
    Connections,
}

impl DetachedWindowKind {
    fn label(&self, session_id: Option<&str>) -> String {
        match self {
            Self::Connections => format!("{DETACHED_LABEL_PREFIX}connections"),
            Self::Terminal => format!("{DETACHED_LABEL_PREFIX}terminal-{}", session_id.unwrap_or("unknown")),
        }
    }
}

fn detached_url(kind: DetachedWindowKind, session_id: Option<&str>) -> String {
    match kind {
        DetachedWindowKind::Terminal => format!("index.html?window=terminal&session={}", session_id.unwrap_or("")),
        DetachedWindowKind::Connections => "index.html?window=connections".to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachedWindowVm {
    pub label: String,
}

#[derive(Debug, Default)]
pub struct DetachedWindowRegistry {
    labels: Mutex<HashSet<String>>,
}

impl DetachedWindowRegistry {
    fn insert(&self, label: &str) {
        if let Ok(mut set) = self.labels.lock() {
            set.insert(label.to_string());
        }
    }

    fn remove(&self, label: &str) {
        if let Ok(mut set) = self.labels.lock() {
            set.remove(label);
        }
    }

    pub fn open_labels(&self) -> Vec<String> {
        self.labels
            .lock()
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }
}

fn command_error(code: &str, message: impl std::fmt::Display) -> CommandErrorVm {
    CommandErrorVm::new(code, serde_json::json!({ "message": message.to_string() }))
}

/// Opens a detached window for a terminal session or the connections page.
/// `session_id` is used by the terminal kind; ignored for connections.
#[tauri::command]
pub fn open_detached_window(
    app: AppHandle,
    registry: tauri::State<'_, DetachedWindowRegistry>,
    kind: DetachedWindowKind,
    session_id: Option<String>,
) -> CommandResult<DetachedWindowVm> {
    let label = kind.label(session_id.as_deref());

    // Already open: bring it to front instead of stacking a duplicate.
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(DetachedWindowVm { label });
    }

    let url = detached_url(kind, session_id.as_deref());
    debug!(label = %label, %url, "opening detached window");

    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title("Gold Band")
        .inner_size(900.0, 640.0)
        .min_inner_size(480.0, 320.0)
        .build()
        .map_err(|error| command_error("window.create-failed", error))?;

    registry.insert(&label);

    let close_app = app.clone();
    let close_label = label.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            debug!(label = %close_label, "detached window close requested");
            let _ = close_app.emit_to("main", DETACHED_WINDOW_CLOSED_EVENT, DetachedWindowVm { label: close_label.clone() });
        }
    });

    Ok(DetachedWindowVm { label })
}

/// Closes a detached window without ending its session (the "return to main
/// window" action). The session keeps running; the main window refreshes and the
/// tab reappears.
#[tauri::command]
pub fn close_detached_window(
    app: AppHandle,
    registry: tauri::State<'_, DetachedWindowRegistry>,
    label: String,
) -> CommandResult<()> {
    registry.remove(&label);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
    Ok(())
}

/// Lists currently-open detached windows.
#[tauri::command]
pub fn list_detached_windows(
    registry: tauri::State<'_, DetachedWindowRegistry>,
) -> CommandResult<Vec<DetachedWindowVm>> {
    Ok(registry
        .open_labels()
        .into_iter()
        .map(|label| DetachedWindowVm { label })
        .collect())
}

/// Closes every detached window. Called during app cleanup so no detached OS
/// window outlives the main one. Enumerates by label prefix rather than trusting
/// the registry, so it is robust even if a registry entry was missed.
pub fn close_all_detached(app: &AppHandle) {
    for label in app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with(DETACHED_LABEL_PREFIX))
        .cloned()
        .collect::<Vec<_>>()
    {
        debug!(label = %label, "closing detached window on app cleanup");
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_label_embeds_session_id_and_prefix() {
        assert_eq!(
            DetachedWindowKind::Terminal.label(Some("sess-abc")),
            "detached-terminal-sess-abc"
        );
        assert_eq!(DetachedWindowKind::Connections.label(None), "detached-connections");
    }

    #[test]
    fn detached_url_carries_query_for_window_routing() {
        assert_eq!(
            detached_url(DetachedWindowKind::Terminal, Some("sess-abc")),
            "index.html?window=terminal&session=sess-abc"
        );
        assert_eq!(
            detached_url(DetachedWindowKind::Connections, None),
            "index.html?window=connections"
        );
    }

    #[test]
    fn registry_tracks_and_removes_labels() {
        let registry = DetachedWindowRegistry::default();
        registry.insert("detached-terminal-a");
        registry.insert("detached-connections");
        assert_eq!(registry.open_labels().len(), 2);
        registry.remove("detached-terminal-a");
        assert_eq!(registry.open_labels(), vec!["detached-connections".to_string()]);
    }
}
