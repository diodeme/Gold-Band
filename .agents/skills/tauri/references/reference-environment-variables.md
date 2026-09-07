---
name: tauri-reference-environment-variables
description: Tauri and CLI environment variables — CI, config depth, dev server, signing, Apple/Windows/Linux build. Use when scripting or CI/CD for Tauri.
---

# Environment Variables

Tauri core and the CLI use several environment variables for behavior and signing. **CLI flags override** the same option when both are set.

## CLI Behavior

- **CI** — When set, CLI runs in CI mode (no interactive prompts).
- **TAURI_CLI_CONFIG_DEPTH** — How many directory levels to search for `tauri.conf.json`.
- **TAURI_CLI_PORT** — Port for the CLI built-in dev server.
- **TAURI_CLI_WATCHER_IGNORE_FILENAME** — Name of a `.gitignore`-style file for the dev watcher.
- **TAURI_CLI_NO_DEV_SERVER_WAIT** — Skip waiting for the frontend dev server before building.
- **TAURI_LINUX_AYATANA_APPINDICATOR** — Set to `true` or `1` to force `libayatana-appindicator` for system tray on Linux.
- **TAURI_WEBVIEW_AUTOMATION** — Enable webview automation (Linux only).
- **TAURI_ANDROID_PROJECT_PATH** / **TAURI_IOS_PROJECT_PATH** — Override path to the generated Android/iOS project.

## Build and Bundle

- **TAURI_BUNDLER_WIX_FIPS_COMPLIANT** — WiX FipsCompliant option.
- **TAURI_BUNDLER_TOOLS_GITHUB_MIRROR** / **TAURI_BUNDLER_TOOLS_GITHUB_MIRROR_TEMPLATE** — GitHub mirror for bundler downloads.
- **TAURI_SKIP_SIDECAR_SIGNATURE_CHECK** — Skip signing sidecars.

## Signing

- **TAURI_SIGNING_PRIVATE_KEY** — Private key (string or path) for app signing.
- **TAURI_SIGNING_PRIVATE_KEY_PASSWORD** — Password for the key.
- **TAURI_SIGNING_RPM_KEY** / **TAURI_SIGNING_RPM_KEY_PASSPHRASE** — GPG key for RPM.
- **TAURI_WINDOWS_SIGNTOOL_PATH** — Path to `signtool.exe` on Windows.
- **APPLE_CERTIFICATE** (base64 .p12), **APPLE_CERTIFICATE_PASSWORD**, **APPLE_ID**, **APPLE_PASSWORD**, **APPLE_TEAM_ID** — macOS/iOS signing and notarization. Alternatively **APPLE_API_KEY**, **APPLE_API_ISSUER**, **API_PRIVATE_KEYS_DIR**, **APPLE_API_KEY_PATH**.
- **APPLE_SIGNING_IDENTITY**, **APPLE_PROVIDER_SHORT_NAME**, **APPLE_DEVELOPMENT_TEAM** — Override signing identity and team.

## Hook Commands (beforeDevCommand, beforeBuildCommand, etc.)

Set by the CLI for each hook; useful for conditional frontend builds:

- **TAURI_ENV_DEBUG** — `true` for `dev` or `build --debug`, else `false`.
- **TAURI_ENV_TARGET_TRIPLE**, **TAURI_ENV_ARCH**, **TAURI_ENV_PLATFORM**, **TAURI_ENV_PLATFORM_VERSION**, **TAURI_ENV_FAMILY** — Target triple, arch, platform, version, and family (`unix` / `windows`).

## Key Points

- Use **CI=1** in CI/CD to avoid prompts. Use **TAURI_ENV_*** in frontend scripts to branch on target (e.g. platform, debug). Keep signing keys in secrets, not in repo.

<!--
Source references:
- https://v2.tauri.app/reference/environment-variables/
- https://github.com/tauri-apps/tauri-docs
-->
