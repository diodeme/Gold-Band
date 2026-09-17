use serde::Serialize;
use tauri::{Manager, Runtime, WebviewWindow};
use tracing::warn;

const WINDOWS_11_MINIMUM_BUILD: u32 = 22_000;

#[cfg(windows)]
#[allow(dead_code)]
const UNDECORATED_RESIZE_OVERLAY_CLASS: &str = "TAURI_DRAG_RESIZE_BORDERS";
#[cfg(windows)]
#[allow(dead_code)]
const UNDECORATED_RESIZE_OVERLAY_NAME: &str = "TAURI_DRAG_RESIZE_WINDOW";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopWindowFrameStyle {
    NativeCompositor,
    AppOutline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWindowChromeVm {
    pub frame_style: DesktopWindowFrameStyle,
    pub native_shadow: bool,
}

pub fn desktop_window_chrome_vm() -> DesktopWindowChromeVm {
    current_desktop_window_chrome()
}

/// Tauri's `unstable` feature builds the primary webview as `WindowChild`, so the
/// runtime skips `attach_resize_handler`. Re-asserting `resizable` hits the same
/// attach path used for undecorated windows, which Win10 needs because it has no
/// DWM outer resize frame.
pub fn ensure_undecorated_edge_resize<R: Runtime>(window: &WebviewWindow<R>) {
    if window.is_decorated().unwrap_or(true) {
        return;
    }
    if let Err(error) = window.set_resizable(true) {
        warn!(error = %error, "failed to attach the undecorated resize overlay");
    }
    raise_undecorated_edge_resize(window);
}

pub fn raise_undecorated_edge_resize<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(windows)]
    raise_undecorated_edge_resize_hwnd(window);
    #[cfg(not(windows))]
    let _ = window;
}

pub fn raise_undecorated_edge_resize_for_app<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    raise_undecorated_edge_resize(&window);
}

#[cfg(windows)]
fn raise_undecorated_edge_resize_hwnd<R: Runtime>(window: &WebviewWindow<R>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    use windows::core::w;

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let Ok(overlay) = (unsafe {
        FindWindowExW(
            Some(hwnd),
            None,
            w!("TAURI_DRAG_RESIZE_BORDERS"),
            w!("TAURI_DRAG_RESIZE_WINDOW"),
        )
    }) else {
        return;
    };
    if overlay.is_invalid() {
        return;
    }
    if let Err(error) = unsafe {
        SetWindowPos(
            overlay,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    } {
        warn!(error = %error, "failed to raise the undecorated resize overlay");
    }
}

#[cfg(target_os = "windows")]
fn current_desktop_window_chrome() -> DesktopWindowChromeVm {
    let version = windows_version::OsVersion::current();
    windows_window_chrome(version.major, version.build)
}

#[cfg(not(target_os = "windows"))]
fn current_desktop_window_chrome() -> DesktopWindowChromeVm {
    DesktopWindowChromeVm {
        frame_style: DesktopWindowFrameStyle::NativeCompositor,
        native_shadow: true,
    }
}

fn windows_window_chrome(major: u32, build: u32) -> DesktopWindowChromeVm {
    if major > 10 || (major == 10 && build >= WINDOWS_11_MINIMUM_BUILD) {
        DesktopWindowChromeVm {
            frame_style: DesktopWindowFrameStyle::NativeCompositor,
            native_shadow: true,
        }
    } else {
        DesktopWindowChromeVm {
            frame_style: DesktopWindowFrameStyle::AppOutline,
            native_shadow: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_10_uses_app_outline_without_asymmetric_native_shadow() {
        assert_eq!(
            windows_window_chrome(10, 19_045),
            DesktopWindowChromeVm {
                frame_style: DesktopWindowFrameStyle::AppOutline,
                native_shadow: false,
            }
        );
    }

    #[test]
    fn windows_11_and_later_use_native_compositor_shadow() {
        assert_eq!(
            windows_window_chrome(10, WINDOWS_11_MINIMUM_BUILD),
            DesktopWindowChromeVm {
                frame_style: DesktopWindowFrameStyle::NativeCompositor,
                native_shadow: true,
            }
        );
        assert_eq!(
            windows_window_chrome(11, 0),
            DesktopWindowChromeVm {
                frame_style: DesktopWindowFrameStyle::NativeCompositor,
                native_shadow: true,
            }
        );
    }

    #[test]
    fn window_chrome_serializes_as_stable_interface_values() {
        assert_eq!(
            serde_json::to_value(windows_window_chrome(10, 19_045)).unwrap(),
            serde_json::json!({
                "frameStyle": "app-outline",
                "nativeShadow": false,
            })
        );
    }

    #[cfg(windows)]
    #[test]
    fn undecorated_resize_overlay_matches_tauri_runtime_names() {
        assert_eq!(
            UNDECORATED_RESIZE_OVERLAY_CLASS,
            "TAURI_DRAG_RESIZE_BORDERS"
        );
        assert_eq!(UNDECORATED_RESIZE_OVERLAY_NAME, "TAURI_DRAG_RESIZE_WINDOW");
    }
}
