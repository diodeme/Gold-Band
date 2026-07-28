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
}

pub fn desktop_window_chrome_vm() -> DesktopWindowChromeVm {
    DesktopWindowChromeVm {
        frame_style: current_desktop_window_frame_style(),
    }
}

#[cfg(target_os = "windows")]
fn current_desktop_window_frame_style() -> DesktopWindowFrameStyle {
    let version = windows_version::OsVersion::current();
    windows_frame_style(version.major, version.build)
}

#[cfg(not(target_os = "windows"))]
fn current_desktop_window_frame_style() -> DesktopWindowFrameStyle {
    DesktopWindowFrameStyle::NativeCompositor
}

fn windows_frame_style(major: u32, build: u32) -> DesktopWindowFrameStyle {
    if major > 10 || (major == 10 && build >= WINDOWS_11_MINIMUM_BUILD) {
        DesktopWindowFrameStyle::NativeCompositor
    } else {
        DesktopWindowFrameStyle::AppOutline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_10_uses_app_outline() {
        assert_eq!(
            windows_frame_style(10, 19_045),
            DesktopWindowFrameStyle::AppOutline
        );
    }

    #[test]
    fn windows_11_and_later_use_native_compositor() {
        assert_eq!(
            windows_frame_style(10, WINDOWS_11_MINIMUM_BUILD),
            DesktopWindowFrameStyle::NativeCompositor
        );
        assert_eq!(
            windows_frame_style(11, 0),
            DesktopWindowFrameStyle::NativeCompositor
        );
    }

    #[test]
    fn frame_style_serializes_as_stable_interface_values() {
        assert_eq!(
            serde_json::to_value(DesktopWindowFrameStyle::NativeCompositor).unwrap(),
            "native-compositor"
        );
        assert_eq!(
            serde_json::to_value(DesktopWindowFrameStyle::AppOutline).unwrap(),
            "app-outline"
        );
    }
}
