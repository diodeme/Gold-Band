---
name: tauri-features-official-plugins-overview
description: Official Tauri plugins overview — dialog, fs, shell, store, updater, and others. Use when choosing which plugin to use for a capability.
---

# Official Plugins Overview

Tauri maintains a set of **official plugins** in the [plugins workspace](https://github.com/tauri-apps/plugins-workspace). Add them with `tauri add <name>` (or Cargo + npm manually). Each plugin has its own permissions; grant them in a **capability** for the window that needs them.

## Common plugins

- **dialog** — Native open/save dialogs, message boxes. Use for file picker or confirmations.
- **fs** — Read/write files and directories. Scopes (path allow/deny) control where the frontend can access. Use for reading config, saving user data (with scoped permissions).
- **shell** — Execute commands, **sidecar** binaries, open URLs in the default app. Use for opening links, running a sidecar (e.g. Node binary). Requires **allow-execute** or **allow-spawn** with scope for command/sidecar and args.
- **store** — Persistent key-value store (e.g. app preferences). Use for settings that survive restarts.
- **updater** — In-app updates (check, download, install). Configure endpoints and signing; use for shipping updates outside the store.
- **http** — HTTP client from the frontend. Scope by URL. Use for API calls when you want them behind permissions.
- **notification** — System notifications (desktop). Use for tray or background alerts.
- **global-shortcut** — Register global hotkeys. Use for app-wide shortcuts.
- **clipboard** — Read/write clipboard. Use for copy/paste.
- **opener** — Open paths/URLs with the default app (e.g. open link in browser). Scope by path/URL.
- **window-state** — Persist window position/size. Use for restoring window geometry.
- **single-instance** — Only one app instance; optional secondary instance handling.
- **process** — Reload, exit, get PID. Use for dev or lifecycle control.
- **sql** — SQLite (and other backends). Use for local DB.
- **stronghold** — Encrypted vault. Use for secrets.
- **deep-linking** — Custom URL scheme / app links. Use for “open with” or OAuth callbacks.
- **cli** — Custom CLI subcommands when the app is run from the terminal.
- **logging** — Routing Rust logs (e.g. to a file or the frontend).

## Platform-specific

- **autostart** — Start at login (desktop).
- **positioner** — Window positioning (e.g. near tray).
- **barcode-scanner**, **biometric**, **geolocation**, **haptics**, **nfc** — Mobile or platform-specific; see plugin docs for support.

## Key Points

- Find full list and setup in the [Plugins](/plugin/) section. Each plugin page lists **permissions** and **scopes** (if any). Add the plugin, then add the required permissions to a capability.
- Prefer **least privilege**: grant only the permissions (and scopes) the window needs (e.g. `fs:allow-read-file` + path scope instead of full `fs:default` when possible).

<!--
Source references:
- https://v2.tauri.app/plugin/
- https://github.com/tauri-apps/plugins-workspace
- https://github.com/tauri-apps/tauri-docs
-->
