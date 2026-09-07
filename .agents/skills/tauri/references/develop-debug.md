---
name: tauri-develop-debug
description: Tauri debugging — dev vs build, Rust console, WebView devtools, open_devtools, RUST_BACKTRACE. Use when debugging Rust or frontend in Tauri apps.
---

# Debug

Tauri apps can be debugged on both the Rust (Core) side and the WebView (frontend) side. Use dev-only checks and devtools appropriately.

## Development vs Production

- **Rust**: `#[cfg(dev)]` / `cfg!(dev)` or `tauri::is_dev()` — true when run with `tauri dev`. `#[cfg(debug_assertions)]` / `cfg!(debug_assertions)` — true for `tauri dev` and `tauri build --debug`.
- Use these to gate debug-only code (e.g. open devtools, verbose logs) so it doesn’t ship in release.

## Rust Console

Output from `println!` and Rust errors appear in the **terminal** where you ran `tauri dev` (or when running the built binary from the terminal). For a stack trace on panic:

```bash
RUST_BACKTRACE=1 pnpm tauri dev
# Windows PowerShell:
$env:RUST_BACKTRACE=1; pnpm tauri dev
```

## WebView Devtools

- **Open**: Right-click in the WebView → Inspect Element, or shortcut **Ctrl+Shift+i** (Windows/Linux) / **Cmd+Option+i** (macOS). Inspector is platform-specific (Edge DevTools on Windows, Safari on macOS, WebInspector on Linux).
- **Programmatic**: `WebviewWindow::open_devtools()` / `close_devtools()`. Guard with `#[cfg(debug_assertions)]` so devtools don’t open in release builds.
- **Release**: Devtools are **off** in normal release builds. Use `tauri build --debug` to get a debug build with devtools, or enable the **devtools** Cargo feature (not recommended for App Store on macOS — private API).

## Debug Build

```bash
pnpm tauri build -- --debug
```

Output is in **src-tauri/target/debug/bundle**. Run the executable from the terminal to see Rust output and panics. Enabling the **devtools** feature in Cargo.toml allows devtools in production builds; avoid on macOS if targeting the App Store.

## Debugging the Core Process

Use standard Rust tooling (GDB, LLDB). See the [Debugging in VS Code](/develop/debug/vscode/) guide for setting up the LLDB extension to attach to the Core process.

## Key Points

- Prefer `cfg!(dev)` or `tauri::is_dev()` for runtime dev checks; `debug_assertions` for debug builds only.
- Always guard `open_devtools()` with `#[cfg(debug_assertions)]` unless you explicitly enable the devtools feature for production.

<!--
Source references:
- https://v2.tauri.app/develop/debug/
- https://github.com/tauri-apps/tauri-docs
-->
