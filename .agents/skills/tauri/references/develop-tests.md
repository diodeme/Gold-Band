---
name: tauri-develop-tests
description: Tauri testing — mock runtime (unit/integration), WebDriver E2E, CI. Use when writing or running tests for Tauri apps.
---

# Tests

Tauri supports **unit and integration tests** using a **mock runtime** (no real webview), and **end-to-end tests** using the **WebDriver** protocol. Use CI (e.g. tauri-action) to run tests per platform.

## Mock Runtime

- No native webview is run; useful for testing Rust logic, commands, and app setup without a window.
- See the [mocking](/develop/tests/mocking/) docs for how to create a test context and invoke handlers under the mock runtime.
- Use for: command handlers, state management, plugin setup, and any code that doesn’t depend on a live WebView.

## WebDriver (E2E)

- Drives the app like a real user (click, type, etc.) via the WebDriver protocol.
- Supported on desktop and mobile; **macOS desktop** does not provide a WebDriver client (use other platforms or headless where available).
- See [WebDriver](/develop/tests/webdriver/) for setup and examples (e.g. Selenium, WebdriverIO). Typically you start the built app and connect a WebDriver client to it.

## CI

- **tauri-action** (GitHub Actions) can build Tauri apps for multiple platforms; use it or any CI that installs the required libs per platform to compile and run tests.
- Ensure each runner has dependencies needed to build the app (e.g. Rust, Node, platform SDKs for mobile).

## Key Points

- Prefer the **mock runtime** for fast, deterministic tests of Rust/command logic; use **WebDriver** only when you need real UI or webview behavior.
- Run `tauri build` (or `tauri build --debug`) before E2E so the WebDriver client launches the actual bundle.

<!--
Source references:
- https://v2.tauri.app/develop/tests/
- https://github.com/tauri-apps/tauri-docs
-->
