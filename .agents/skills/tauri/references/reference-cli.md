---
name: tauri-reference-cli
description: Tauri CLI — dev, build, icon, init, migrate, bundle. Use when running or scripting Tauri commands.
---

# Command Line Interface (CLI)

The Tauri CLI is the main way to run and build Tauri apps. Install as a dev dependency or use globally (cargo).

## Installation

```bash
pnpm add -D @tauri-apps/cli@latest
# or: npm, yarn, deno, bun
# or globally: cargo install tauri-cli --version "^2.0.0" --locked
```

Run via package manager: `pnpm tauri <command>`, `npm run tauri -- <command>`, etc. (Use `"tauri": "tauri"` script in package.json for npm.)

## Main Commands

- **tauri dev** – Build and run the app in development; start the frontend dev server (from **beforeDevCommand**), open the window. Uses **devUrl** for the WebView.
- **tauri build** – Production build: compile Rust, run **beforeBuildCommand**, bundle frontend, create platform bundles. Use **--debug** for a debug build (faster, with devtools possible). Use **--no-bundle** to compile only.
- **tauri bundle** – Create installers/bundles from an existing build. Use **--bundles app,dmg** (etc.) and optionally **--config path/to/tauri.conf.json** for flavour-specific config.
- **tauri init** – Interactive setup: create **src-tauri/** with Cargo.toml, tauri.conf.json, capabilities, and entry files. Use when adding Tauri to an existing frontend.
- **tauri icon [INPUT]** – Generate app icons from a source image (default **./app-icon.png**) into **src-tauri/icons/** (or **-o** path). Options: **-p/--png** sizes, **--ios-color**.
- **tauri migrate** – Automated migration from Tauri 1.x (or 2 beta) to 2.x: config, capabilities, permissions. Always read the full migration guide as well.

## Plugin Commands

- **plugin new [name]** – Scaffold a new plugin (Rust + optional JS). Use **--no-api** to skip the NPM package, **--android** / **--ios** for mobile.
- **plugin android add** / **plugin ios add** – Add Android/iOS support to an existing plugin.

## Passing Options

- **--config &lt;path&gt;** – Merge an extra config file (e.g. `tauri.beta.conf.json`) for build/bundle.
- **--bundles &lt;list&gt;** – Comma-separated bundle formats (e.g. `app,dmg`).
- **--debug** – Build in debug mode (faster, larger, devtools available).
- **--no-bundle** – Build without creating bundles.

## Key Points

- CLI must be the same **minor version** as the **tauri** and **tauri-build** crates (e.g. 2.x). Run from the project root; the CLI locates the Rust project by **tauri.conf.json** (usually inside **src-tauri/**).

<!--
Source references:
- https://v2.tauri.app/reference/cli/
- https://github.com/tauri-apps/tauri-docs
-->
