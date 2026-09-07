---
name: tauri-core-architecture
description: Tauri architecture, core crates (tauri, tauri-runtime, tauri-build), and ecosystem (WRY, TAO). Use when understanding how Tauri apps are structured or integrating tooling.
---

# Tauri Architecture

Tauri is a polyglot toolkit for building desktop (and mobile) apps: a Rust backend plus HTML/CSS/JS in the system WebView. The frontend talks to Rust via message passing (IPC). The Core process has full OS access; WebView processes run your UI.

## Core Crates

- **tauri** – Main crate: reads `tauri.conf.json` at compile time, hosts the API, script injection, updater. Use for app setup and runtime.
- **tauri-runtime** – Glue between Tauri and webview libraries.
- **tauri-macros** – Macros for context, handler, and commands (via `tauri-codegen`).
- **tauri-utils** – Shared utilities: config parsing, platform detection, CSP injection, assets.
- **tauri-build** – Build-time macros for Cargo integration.
- **tauri-codegen** – Embeds/hashes/compresses assets and icons, parses config, generates `Config` struct.
- **tauri-runtime-wry** – WRY-specific systems (printing, monitors, windowing).

## Upstream

- **TAO** – Cross-platform window creation (fork of winit); menus, tray.
- **WRY** – Cross-platform WebView rendering. Tauri uses WRY to choose and drive the OS webview (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux).

## Tooling

- **@tauri-apps/api** (JS/TS) – Endpoints for `invoke`, events, etc.; uses webview message passing.
- **tauri-bundler** – Builds the app for the detected or specified platform.
- **tauri-cli** (Rust) – CLI for dev, build, icon, etc.
- **@tauri-apps/cli** (npm) – NAPI wrapper around the Rust CLI.
- **create-tauri-app** – Scaffolds a new Tauri project with a chosen frontend template.

## Plugins

Plugins (1) expose Rust behavior, (2) provide integration glue, (3) expose a JS API. They are separate crates (e.g. `tauri-plugin-fs`, `tauri-plugin-sql`). Register with `tauri::Builder::plugin(...)` and configure in `tauri.conf.json > plugins`.

## Key Points

- Config is read at compile time from `tauri.conf.json` (and optional JSON5/TOML via feature flags).
- Frontend and backend communicate only via IPC (commands and events); no direct FFI from the webview.
- Binary size stays small because the webview is provided by the OS, not bundled.

<!--
Source references:
- https://v2.tauri.app/concept/architecture/
- https://github.com/tauri-apps/tauri-docs
-->
