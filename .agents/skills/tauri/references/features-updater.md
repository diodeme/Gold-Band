---
name: tauri-features-updater
description: In-app updates — check, download, install, signing, endpoints (static JSON or dynamic server). Use when implementing auto-update in Tauri apps.
---

# Updater Plugin

The **updater** plugin lets your Tauri app check for, download, and install updates from an update server or a static JSON. Signing is **required**; the app validates update artifacts with a public key.

## Setup

- Add plugin: `pnpm tauri add updater` (or `cargo add tauri-plugin-updater` with `cfg(desktop)` + register with `.plugin(tauri_plugin_updater::Builder::new().build())` + npm install `@tauri-apps/plugin-updater`).
- Register only on desktop: use `.setup(|app| { #[cfg(desktop)] app.handle().plugin(tauri_plugin_updater::Builder::new().build()); Ok(()) })`.

## Signing

- Generate keys: `tauri signer generate -w ~/.tauri/myapp.key`. Keep the **private** key secret; put the **public** key in `tauri.conf.json` (content only, not a path).
- Build with private key in env: `TAURI_SIGNING_PRIVATE_KEY` (path or content), optionally `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `.env` is not used; set in shell or CI.
- Enable updater artifacts: `tauri.conf.json` → `bundle.createUpdaterArtifacts: true`. Outputs: per-platform updater bundle + `.sig` (e.g. macOS `.app.tar.gz` + `.sig`, Windows `.msi`/installer + `.sig`, Linux AppImage + `.sig`). Use `"v1Compatible"` if migrating from Tauri 1.

## Configuration

- **plugins.updater.pubkey**: Public key content (required).
- **plugins.updater.endpoints**: Array of URLs. TLS enforced in production. Tauri tries next URL on non-2XX. URL can use `{{current_version}}`, `{{target}}` (linux/windows/darwin), `{{arch}}` (x86_64, aarch64, etc.).
- **plugins.updater.dangerousInsecureTransportProtocol**: Allow non-HTTPS (use with caution).
- **Windows**: `plugins.updater.windows.installMode`: `"passive"` (default, progress bar), `"basicUi"`, or `"quiet"`.

## Server: Static JSON

Return JSON with `version` (SemVer), `notes`, `pub_date` (RFC 3339), `platforms`: keys like `linux-x86_64`, `darwin-aarch64`, `windows-x86_64`, each with `url` and `signature` (content of `.sig` file). Tauri validates the file before comparing version. [Tauri Action](https://github.com/tauri-apps/tauri-action) can generate this for GitHub Releases.

## Server: Dynamic

Server receives GET with URL template variables. Respond **204 No Content** if no update; **200 OK** with JSON `{ version, url, signature, notes?, pub_date? }` if update available. Signature must be the `.sig` content. You can customize version comparison in Rust (e.g. allow downgrades) via `UpdaterBuilder::version_comparator`.

## Checking and Installing (Frontend)

```ts
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

const update = await check();
if (update) {
  await update.downloadAndInstall((event) => {
    // event: Started { contentLength }, Progress { chunkLength }, Finished
  });
  await relaunch();
}
```

Optional: `check({ proxy, timeout, headers, target })` for custom target (e.g. `macos-universal`).

## Checking and Installing (Rust)

```rust
use tauri_plugin_updater::UpdaterExt;

if let Some(update) = app.updater()?.check().await? {
  update.download_and_install(|chunk, total| { /* progress */ }, || { /* finished */ }).await?;
  app.restart();
}
```

For custom endpoints or pubkey at runtime: `app.updater_builder().endpoints(vec![url])?.pubkey(key).build()?.check().await?`. Use `target("custom-target")` for custom platform key; use `version_comparator` to allow downgrades. On Windows, `on_before_exit` runs before the app exits for install.

## Permissions

Add `updater:default` (or the specific updater permissions you need) to the app capability that should be allowed to check/install updates.

## Key Points

- Signing cannot be disabled. Lose the private key and you cannot ship new updates to existing installs.
- On Windows the app exits automatically when the installer runs; use `on_before_exit` for cleanup or UI.
- Prefer static JSON + CDN (e.g. GitHub Releases + Tauri Action) for simplicity; use a dynamic server for channels or rollbacks.

<!--
Source references:
- https://v2.tauri.app/plugin/updater/
- https://github.com/tauri-apps/tauri-docs
-->
