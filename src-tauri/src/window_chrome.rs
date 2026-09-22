use serde::Serialize;
use tauri::{Manager, Runtime, WebviewWindow};
use tracing::warn;

#[cfg(windows)]
use std::sync::Mutex;

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

/// DWM drop-shadow margins in left, right, top, bottom order.
///
/// One bottom pixel is enough for DWM to draw the outer shadow. TAO's
/// undecorated shadow instead insets the client by the resize frame on three
/// sides, and Windows 10 keeps the top inset at 0, so the WebView leaves black
/// bars. Win11 already enables that TAO path. A maximized or fullscreen window
/// has no desktop around it, so the margin is cleared.
pub fn compositor_shadow_margins(uses_tao_native_shadow: bool, occludes_desktop: bool) -> [i32; 4] {
    if uses_tao_native_shadow || occludes_desktop {
        [0, 0, 0, 0]
    } else {
        [0, 0, 0, 1]
    }
}

/// Applies the Win10 DWM shadow and keeps it in sync when the window is
/// maximized, restored, or recreated. Win11 is unchanged.
pub fn install_win10_compositor_shadow<R: Runtime>(window: &WebviewWindow<R>) {
    if current_desktop_window_chrome().native_shadow {
        return;
    }
    sync_win10_compositor_shadow(window);
    #[cfg(windows)]
    {
        let tracked = window.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Resized(_)) {
                sync_win10_compositor_shadow(&tracked);
            }
        });
    }
}

pub fn sync_win10_compositor_shadow<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(windows)]
    sync_win10_compositor_shadow_hwnd(window);
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
struct AppliedCompositorShadow {
    hwnd: isize,
    margins: [i32; 4],
}

#[cfg(windows)]
static APPLIED_COMPOSITOR_SHADOW: Mutex<Option<AppliedCompositorShadow>> = Mutex::new(None);

#[cfg(windows)]
fn sync_win10_compositor_shadow_hwnd<R: Runtime>(window: &WebviewWindow<R>) {
    if current_desktop_window_chrome().native_shadow || window.is_decorated().unwrap_or(true) {
        return;
    }
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let occludes_desktop =
        window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false);
    let margins = compositor_shadow_margins(false, occludes_desktop);
    let hwnd_key = hwnd.0 as isize;
    if applied_shadow_matches(hwnd_key, margins) {
        return;
    }
    // Remember the request even when DWM rejects it. Otherwise every resize
    // pixel would call SetWindowPos again.
    let _ = apply_compositor_shadow(hwnd, margins);
    remember_applied_shadow(hwnd_key, margins);
}

#[cfg(windows)]
fn applied_shadow_matches(hwnd: isize, margins: [i32; 4]) -> bool {
    APPLIED_COMPOSITOR_SHADOW
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .is_some_and(|applied| applied.hwnd == hwnd && applied.margins == margins)
}

#[cfg(windows)]
fn remember_applied_shadow(hwnd: isize, margins: [i32; 4]) {
    *APPLIED_COMPOSITOR_SHADOW
        .lock()
        .unwrap_or_else(|error| error.into_inner()) =
        Some(AppliedCompositorShadow { hwnd, margins });
}

#[cfg(windows)]
fn apply_compositor_shadow(hwnd: windows::Win32::Foundation::HWND, margins: [i32; 4]) -> bool {
    use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };

    let inset = MARGINS {
        cxLeftWidth: margins[0],
        cxRightWidth: margins[1],
        cyTopHeight: margins[2],
        cyBottomHeight: margins[3],
    };
    unsafe {
        if let Err(error) = DwmExtendFrameIntoClientArea(hwnd, &inset) {
            warn!(
                error = %error,
                "failed to extend the window frame for the Win10 drop shadow"
            );
            return false;
        }
        if let Err(error) = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        ) {
            warn!(
                error = %error,
                "failed to refresh the window frame after enabling the Win10 drop shadow"
            );
            return false;
        }
    }
    true
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
    fn windows_10_requests_a_one_pixel_dwm_shadow_without_tao_insets() {
        let chrome = windows_window_chrome(10, 19_045);
        assert!(!chrome.native_shadow);
        assert_eq!(
            compositor_shadow_margins(chrome.native_shadow, false),
            [0, 0, 0, 1]
        );
        assert_eq!(
            compositor_shadow_margins(chrome.native_shadow, true),
            [0, 0, 0, 0]
        );
        assert_eq!(
            compositor_shadow_margins(
                windows_window_chrome(10, WINDOWS_11_MINIMUM_BUILD).native_shadow,
                false
            ),
            [0, 0, 0, 0]
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
