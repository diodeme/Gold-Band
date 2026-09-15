---
name: tauri-core-process-model
description: Tauri multi-process model — Core process (full OS access) vs WebView processes (UI). Use when reasoning about security, state, or window lifecycle.
---

# Process Model

Tauri uses a **multi-process** layout similar to Electron: one **Core** process and one or more **WebView** processes. This improves resilience and allows least-privilege design.

## Core Process

- **Single** entry point; **full** access to the OS.
- Creates and manages windows, system tray, menus, notifications.
- Routes **all** IPC: every command and event goes through the Core. You can intercept, filter, or modify messages in one place.
- Should own **global state** (e.g. DB, settings) so it can sync across windows and keep sensitive data out of the frontend.

## WebView Process(es)

- Run your HTML/CSS/JS (and optionally WASM).
- One WebView per window (conceptually); same tech and tooling as normal web apps (Vite, Svelte, React, etc.).
- **No** direct OS access; only what you expose via **commands** and **events**.
- WebView is provided by the OS (WebView2, WKWebView, WebKitGTK), not shipped in the binary.

## Security Implications

- Frontend is untrusted: validate and sanitize all input; don’t handle secrets in the frontend; keep business logic in the Core.
- Give each window only the **capabilities** it needs (via capability files and permissions).
- Prefer **window labels** (not titles) for security boundaries; restrict window-creation to higher-privilege code.

## Key Points

- Core = Rust, single process, full authority. WebView(s) = UI only, message-passing only.
- State and heavy work belong in the Core; use `app.manage()` and inject `State<T>` in commands.
- Crashes or compromises in the WebView don’t bypass the Core; the Core can refuse or sanitize IPC.

<!--
Source references:
- https://v2.tauri.app/concept/process-model/
- https://github.com/tauri-apps/tauri-docs
-->
