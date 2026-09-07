---
name: tauri-develop-tests-webdriver
description: WebDriver E2E with Tauri — tauri-driver, platform drivers (WebKitWebDriver, Edge), Selenium/WebdriverIO, CI. Use when automating UI tests.
---

# WebDriver E2E Testing

Tauri supports the **WebDriver** interface for automated UI testing via **[tauri-driver](https://crates.io/crates/tauri-driver)**. It wraps the platform’s native WebDriver server. **Desktop**: Windows and Linux only (macOS has no WKWebView driver). **Mobile**: iOS and Android via Appium 2 (process not yet streamlined).

## Install tauri-driver

```bash
cargo install tauri-driver --locked
```

## Platform Drivers

- **Linux**: Use **WebKitWebDriver**. Check with `which WebKitWebDriver`; install if needed (e.g. `webkit2gtk-driver` on Debian).
- **Windows**: Use **Microsoft Edge Driver** (msedgedriver). Version must match the Edge version on the machine. Download from [Edge WebDriver](https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/) or use [msedgedriver-tool](https://github.com/chippers/msedgedriver-tool). Put `msedgedriver.exe` on PATH or pass **--native-driver** to tauri-driver.
- **macOS**: No desktop WebDriver client for WKWebView; use other platforms or headless where available.

## Running Tests

1. Build the app: `tauri build` (or `tauri build --debug`).
2. Start **tauri-driver** (it connects to the app’s WebView).
3. Run your WebDriver client (Selenium, WebdriverIO, etc.) against the driver. See the [Selenium](/develop/tests/webdriver/example/selenium/) and [WebdriverIO](/develop/tests/webdriver/example/webdriverio/) guides for minimal examples.
4. Example repo: [webdriver-example](https://github.com/tauri-apps/webdriver-example).

## CI

- Use **tauri-action** or your own CI that installs Rust, Node, and the **platform driver** (WebKitWebDriver on Linux, Edge driver on Windows). Match Edge and Edge driver versions on Windows to avoid hangs.
- See the [WebDriver CI](/develop/tests/webdriver/ci/) doc for a step-by-step CI setup.

## Key Points

- **tauri-driver** is the bridge; the actual driver is platform-specific (WebKitWebDriver, msedgedriver). Install and version-match per platform.
- For **frontend-only** tests without a real window, use **mockIPC** / **mockWindows**; use WebDriver when you need real UI or full app flow.

<!--
Source references:
- https://v2.tauri.app/develop/tests/webdriver/
- https://github.com/tauri-apps/tauri-docs
-->
