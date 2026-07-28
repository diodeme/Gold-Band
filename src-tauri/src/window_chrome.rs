use serde::Serialize;

const WINDOWS_11_MINIMUM_BUILD: u32 = 22_000;

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
}
