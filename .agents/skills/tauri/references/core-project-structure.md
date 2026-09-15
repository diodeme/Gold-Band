---
name: tauri-core-project-structure
description: Tauri project layout — src-tauri (Rust, tauri.conf.json, capabilities), frontend root, and where to put config and permissions.
---

# Project Structure

A Tauri app is usually a **frontend project** (Node/JS) plus a **Rust project** under `src-tauri/`. The CLI looks for `tauri.conf.json` inside the Rust project.

## Typical Layout

```
.
├── package.json
├── index.html
├── src/                 # Frontend (Vite/React/etc.)
│   └── main.js
└── src-tauri/
    ├── Cargo.toml
    ├── Cargo.lock
    ├── build.rs
    ├── tauri.conf.json   # Main Tauri config; CLI marker
    ├── src/
    │   ├── main.rs       # Desktop entry; usually just calls app_lib::run()
    │   └── lib.rs        # App logic, commands, mobile_entry_point
    ├── icons/
    └── capabilities/    # ACL: which commands/perms per window
        └── default.json
```

## Important Paths

- **tauri.conf.json** – App id, build/dev URLs, bundle options, windows, plugins. Read at compile time. Platform overrides: `tauri.{linux|windows|macos|android|ios}.conf.json` (or `.toml`).
- **capabilities/** – JSON/TOML files that grant permissions to windows. Frontend can only call commands that are allowed by a capability attached to that window.
- **icons/** – Output of `tauri icon`; referenced in config for app and tray icons.
- **build.rs** – Must call `tauri_build::build()` (or `try_build`) for Tauri’s build steps.
- **lib.rs** – Register commands with `invoke_handler(tauri::generate_handler![...])`, `manage()` state, and (on mobile) `#[cfg_attr(mobile, tauri::mobile_entry_point)]`. Prefer editing `lib.rs` over `main.rs`.
- **main.rs** – Desktop entry: `fn main() { app_lib::run(); }` (or equivalent). Don’t duplicate app logic here.

## Build Flow

1. Build the frontend (e.g. `pnpm run build`) to static assets.
2. `tauri build` compiles the Rust app and bundles those assets. Config’s `beforeBuildCommand` / `beforeDevCommand` drive the frontend build.

## Rust-only or Monorepo

You can use only `src-tauri/` as the project root, or make it a Cargo workspace member; just ensure `tauri.conf.json` and `build.rs` are in the same crate as the one that calls `tauri_build::build()`.

<!--
Source references:
- https://v2.tauri.app/start/project-structure/
- https://github.com/tauri-apps/tauri-docs
-->
