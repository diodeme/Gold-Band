use std::{
    fmt,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
};

use camino::{Utf8Path, Utf8PathBuf};
use gold_band::storage::{GoldBandPaths, read_json, write_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::DesktopContext;

const DIAGNOSTIC_SCHEMA_VERSION: u8 = 1;
const LOOPBACK_HOST: &str = "127.0.0.1";
const MARKER_RELATIVE_PATH: &str = "diagnostics/webview-cdp.json";
const WRY_DEFAULT_BROWSER_ARGUMENTS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebviewHeapDiagnosticVm {
    pub schema_version: u8,
    pub app_pid: u32,
    pub host: String,
    pub port: u16,
    pub discovery_url: String,
    pub marker_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebviewHeapDiagnosticError {
    pub code: String,
    pub params: Value,
}

impl WebviewHeapDiagnosticError {
    fn new(code: impl Into<String>, params: Value) -> Self {
        Self {
            code: code.into(),
            params,
        }
    }
}

impl fmt::Display for WebviewHeapDiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.code)
    }
}

impl std::error::Error for WebviewHeapDiagnosticError {}

pub struct WebviewHeapDiagnosticState {
    snapshot: WebviewHeapDiagnosticVm,
    marker_path: Utf8PathBuf,
}

impl WebviewHeapDiagnosticState {
    pub(crate) fn snapshot(&self) -> WebviewHeapDiagnosticVm {
        self.snapshot.clone()
    }
}

impl Drop for WebviewHeapDiagnosticState {
    fn drop(&mut self) {
        remove_owned_marker(&self.marker_path, &self.snapshot);
    }
}

#[cfg(test)]
const fn is_webview_heap_diagnostic_enabled(debug_assertions: bool, windows: bool) -> bool {
    debug_assertions && windows
}

pub fn initialize(
    context: &DesktopContext,
) -> Result<WebviewHeapDiagnosticState, WebviewHeapDiagnosticError> {
    let marker_path = GoldBandPaths::new(context.repo_root.clone())
        .user_gold_band_dir()
        .join(MARKER_RELATIVE_PATH);
    let port = allocate_dynamic_loopback_port()?;
    let snapshot = WebviewHeapDiagnosticVm {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        app_pid: std::process::id(),
        host: LOOPBACK_HOST.to_string(),
        port,
        discovery_url: format!("http://{LOOPBACK_HOST}:{port}/json/list"),
        marker_path: marker_path.to_string(),
    };

    write_marker(&marker_path, &snapshot)?;

    Ok(WebviewHeapDiagnosticState {
        snapshot,
        marker_path,
    })
}

#[tauri::command]
pub fn get_webview_heap_diagnostic(
    state: tauri::State<'_, WebviewHeapDiagnosticState>,
) -> Result<WebviewHeapDiagnosticVm, WebviewHeapDiagnosticError> {
    Ok(state.snapshot())
}

fn allocate_dynamic_loopback_port() -> Result<u16, WebviewHeapDiagnosticError> {
    let listener =
        TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|error| {
            WebviewHeapDiagnosticError::new(
                "webview-diagnostic.loopback-bind-failed",
                json!({
                    "host": LOOPBACK_HOST,
                    "ioKind": format!("{:?}", error.kind()),
                }),
            )
        })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            WebviewHeapDiagnosticError::new(
                "webview-diagnostic.loopback-address-failed",
                json!({ "ioKind": format!("{:?}", error.kind()) }),
            )
        })?
        .port();
    drop(listener);
    Ok(port)
}

pub fn additional_browser_arguments(
    existing: Option<&str>,
    snapshot: &WebviewHeapDiagnosticVm,
) -> String {
    let mut arguments = existing
        .map(str::trim)
        .filter(|arguments| !arguments.is_empty())
        .unwrap_or(WRY_DEFAULT_BROWSER_ARGUMENTS)
        .to_string();
    arguments.push_str(&format!(
        " --js-flags=--expose-gc --remote-debugging-address={} --remote-debugging-port={}",
        snapshot.host, snapshot.port
    ));
    arguments
}

fn write_marker(
    marker_path: &Utf8Path,
    snapshot: &WebviewHeapDiagnosticVm,
) -> Result<(), WebviewHeapDiagnosticError> {
    write_json(marker_path, snapshot).map_err(|_| {
        WebviewHeapDiagnosticError::new(
            "webview-diagnostic.marker-write-failed",
            json!({ "path": marker_path.as_str() }),
        )
    })
}

fn remove_owned_marker(marker_path: &Utf8Path, snapshot: &WebviewHeapDiagnosticVm) {
    let marker = read_json::<WebviewHeapDiagnosticVm>(marker_path);
    if marker.as_ref().is_ok_and(|marker| marker == snapshot) {
        let _ = std::fs::remove_file(marker_path.as_std_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(marker_path: &Utf8Path, port: u16, app_pid: u32) -> WebviewHeapDiagnosticVm {
        WebviewHeapDiagnosticVm {
            schema_version: DIAGNOSTIC_SCHEMA_VERSION,
            app_pid,
            host: LOOPBACK_HOST.to_string(),
            port,
            discovery_url: format!("http://{LOOPBACK_HOST}:{port}/json/list"),
            marker_path: marker_path.to_string(),
        }
    }

    #[test]
    fn enablement_is_limited_to_windows_debug_builds() {
        assert!(is_webview_heap_diagnostic_enabled(true, true));
        assert!(!is_webview_heap_diagnostic_enabled(false, true));
        assert!(!is_webview_heap_diagnostic_enabled(true, false));
        assert!(!is_webview_heap_diagnostic_enabled(false, false));
    }

    #[test]
    fn allocated_endpoint_is_dynamic_and_loopback_only() {
        let port = allocate_dynamic_loopback_port().unwrap();
        assert_ne!(port, 0);

        let marker_path = Utf8Path::new("diagnostics/webview-cdp.json");
        let value = snapshot(marker_path, port, 42);
        let arguments = additional_browser_arguments(None, &value);

        assert!(arguments.contains("--remote-debugging-address=127.0.0.1"));
        assert!(arguments.contains(&format!("--remote-debugging-port={port}")));
        assert!(!arguments.contains("--remote-debugging-address=0.0.0.0"));
    }

    #[test]
    fn browser_arguments_preserve_existing_webview2_options_and_expose_gc() {
        let marker_path = Utf8Path::new("diagnostics/webview-cdp.json");
        let value = snapshot(marker_path, 32123, 42);
        let arguments = additional_browser_arguments(
            Some("--disable-features=Example --custom-option=value"),
            &value,
        );

        assert!(arguments.starts_with("--disable-features=Example --custom-option=value "));
        assert!(arguments.contains("--js-flags=--expose-gc"));
        assert!(
            arguments
                .ends_with("--remote-debugging-address=127.0.0.1 --remote-debugging-port=32123")
        );
    }

    #[test]
    fn browser_arguments_keep_wry_security_defaults_when_config_has_no_override() {
        let marker_path = Utf8Path::new("diagnostics/webview-cdp.json");
        let value = snapshot(marker_path, 32127, 42);
        let arguments = additional_browser_arguments(None, &value);

        assert!(
            arguments.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection")
        );
        assert!(arguments.contains("--autoplay-policy=no-user-gesture-required"));
    }

    #[test]
    fn marker_and_read_only_snapshot_share_the_same_serialized_contract() {
        let temp = tempfile::tempdir().unwrap();
        let marker_path = Utf8PathBuf::from_path_buf(temp.path().join("webview-cdp.json")).unwrap();
        let expected = snapshot(&marker_path, 32124, 84);

        write_marker(&marker_path, &expected).unwrap();
        let persisted: WebviewHeapDiagnosticVm = read_json(&marker_path).unwrap();
        let serialized = serde_json::to_value(&persisted).unwrap();

        assert_eq!(persisted, expected);
        assert_eq!(serialized["schemaVersion"], 1);
        assert_eq!(serialized["appPid"], 84);
        assert_eq!(serialized["host"], "127.0.0.1");
        assert_eq!(serialized["port"], 32124);
        assert_eq!(
            serialized["discoveryUrl"],
            "http://127.0.0.1:32124/json/list"
        );
        assert_eq!(serialized["markerPath"], marker_path.as_str());
    }

    #[test]
    fn cleanup_never_removes_a_marker_replaced_by_another_process() {
        let temp = tempfile::tempdir().unwrap();
        let marker_path = Utf8PathBuf::from_path_buf(temp.path().join("webview-cdp.json")).unwrap();
        let previous = snapshot(&marker_path, 32125, 100);
        let replacement = snapshot(&marker_path, 32126, 200);

        write_marker(&marker_path, &replacement).unwrap();
        remove_owned_marker(&marker_path, &previous);
        assert!(marker_path.exists());

        remove_owned_marker(&marker_path, &replacement);
        assert!(!marker_path.exists());
    }
}
