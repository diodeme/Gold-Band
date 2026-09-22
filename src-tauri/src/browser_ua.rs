use tauri::webview::PlatformWebview;

pub fn set_user_agent(platform: &PlatformWebview, user_agent: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        return set_windows_user_agent(platform, user_agent);
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return set_webkit_user_agent(platform, user_agent);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        return set_unix_user_agent(platform, user_agent);
    }
    #[allow(unreachable_code)]
    {
        let _ = (platform, user_agent);
        Err("browser.webview.user_agent_unsupported".into())
    }
}

pub fn read_user_agent(platform: &PlatformWebview) -> Option<String> {
    #[cfg(windows)]
    {
        return read_windows_user_agent(platform);
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return read_webkit_user_agent(platform);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        return read_unix_user_agent(platform);
    }
    #[allow(unreachable_code)]
    {
        let _ = platform;
        None
    }
}

#[cfg(windows)]
fn windows_settings(
    platform: &PlatformWebview,
) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2, String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2;
    use windows::core::Interface;

    let webview =
        unsafe { platform.controller().CoreWebView2() }.map_err(|error| error.to_string())?;
    let settings = unsafe { webview.Settings() }.map_err(|error| error.to_string())?;
    settings
        .cast::<ICoreWebView2Settings2>()
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn set_windows_user_agent(platform: &PlatformWebview, user_agent: &str) -> Result<(), String> {
    use windows::core::HSTRING;

    let settings = windows_settings(platform)?;
    unsafe { settings.SetUserAgent(&HSTRING::from(user_agent)) }.map_err(|error| error.to_string())
}

#[cfg(windows)]
fn read_windows_user_agent(platform: &PlatformWebview) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::core::PWSTR;

    let settings = windows_settings(platform).ok()?;
    let mut value = PWSTR::null();
    unsafe { settings.UserAgent(&mut value) }.ok()?;
    if value.is_null() {
        return None;
    }
    let text = unsafe { value.to_string() }.ok();
    unsafe {
        CoTaskMemFree(Some(value.0.cast()));
    }
    text.filter(|value| !value.is_empty())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn webkit_view(platform: &PlatformWebview) -> Option<&objc2_web_kit::WKWebView> {
    let inner = platform.inner();
    if inner.is_null() {
        return None;
    }
    Some(unsafe { &*inner.cast::<objc2_web_kit::WKWebView>() })
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn set_webkit_user_agent(platform: &PlatformWebview, user_agent: &str) -> Result<(), String> {
    use objc2_foundation::NSString;

    let webview = webkit_view(platform).ok_or_else(|| "missing wkwebview".to_string())?;
    unsafe { webview.setCustomUserAgent(Some(&NSString::from_str(user_agent))) };
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn read_webkit_user_agent(platform: &PlatformWebview) -> Option<String> {
    let webview = webkit_view(platform)?;
    unsafe { webview.customUserAgent() }.map(|value| value.to_string())
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn set_unix_user_agent(platform: &PlatformWebview, user_agent: &str) -> Result<(), String> {
    use webkit2gtk::{SettingsExt, WebViewExt};

    let settings = platform
        .inner()
        .settings()
        .ok_or_else(|| "missing webkit settings".to_string())?;
    settings.set_user_agent(Some(user_agent));
    Ok(())
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn read_unix_user_agent(platform: &PlatformWebview) -> Option<String> {
    use webkit2gtk::{SettingsExt, WebViewExt};

    platform
        .inner()
        .settings()
        .and_then(|settings| settings.user_agent().map(|value| value.to_string()))
}
