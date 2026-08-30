---
name: tauri-develop-configuration-files
description: Tauri config (tauri.conf.json), platform overrides, Cargo.toml, package.json. Use when configuring build, dev server, bundle, or plugins.
---

# Configuration Files

Main files: **tauri.conf.json** (Tauri), **Cargo.toml** (Rust), **package.json** (frontend). Tauri config drives runtime and CLI behavior.

## Tauri Config (tauri.conf.json)

Defines: web app source, app metadata, bundle settings, plugin config, windows, tray, menus, runtime options.

- **build**: `devUrl`, `beforeDevCommand`, `beforeBuildCommand`, `frontendDist` (or equivalent). Dev server URL must match your frontend (e.g. `http://localhost:5173`).
- **bundle**: `active`, `icon`, `resources`, platform-specific bundle options.
- **app**: `windows`, `tray`, `menus`, `security` (e.g. capabilities list).
- **plugins**: Per-plugin config (e.g. `updater: { pubkey, endpoints }`).

Formats: JSON (default), JSON5 or TOML if you enable `config-json5` or `config-toml` in `tauri` and `tauri-build` in Cargo.toml.

## Platform-specific Config

Merge over base config (JSON Merge Patch RFC 7396):

- `tauri.linux.conf.json` / `Tauri.linux.toml`
- `tauri.windows.conf.json` / `Tauri.windows.toml`
- `tauri.macos.conf.json` / `Tauri.macos.toml`
- `tauri.android.conf.json`, `tauri.ios.conf.json`

Use for platform-only bundle or plugin settings.

## Extending Config via CLI

Pass extra config when building or running:

```bash
pnpm tauri build -- --config src-tauri/tauri.beta.conf.json
```

Use for flavours (e.g. beta app with different `productName` and `identifier`).

## Cargo.toml

- **tauri-build** (build-dependencies): same minor as CLI; required for `tauri_build::build()`.
- **tauri** (dependencies): same minor as CLI; features (e.g. `config-json5`) and optional crates. `tauri dev` / `tauri build` can auto-enable features from config.
- Pin exact version with `=2.0.0` if needed. Prefer committing `Cargo.lock`.

## package.json

- **scripts**: `dev`, `build` – hook into config’s `beforeDevCommand` and `beforeBuildCommand`.
- **dependencies**: `@tauri-apps/api`, optional `@tauri-apps/cli` (or global CLI). Use `"tauri": "tauri"` script when using npm so `npm run tauri` works.

## Key Points

- Config is read at **compile time** by `tauri-build` and at runtime by the app. Change dev URL or build commands in one place (tauri.conf.json).
- Capabilities are listed under `app.security.capabilities` (file names in `capabilities/` or inline objects). Only explicitly enabled capabilities apply once you set this.

<!--
Source references:
- https://v2.tauri.app/develop/configuration-files/
- https://v2.tauri.app/reference/config/
- https://github.com/tauri-apps/tauri-docs
-->
