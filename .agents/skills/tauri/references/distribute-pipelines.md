---
name: tauri-distribute-pipelines
description: CI/CD for Tauri — tauri-action (GitHub Actions), projectPath, triggers, signing. Use when building and releasing Tauri apps in CI.
---

# Build Pipelines (CI/CD)

Use **tauri-action** in [GitHub Actions](https://github.com/tauri-apps/tauri-action) to build your Tauri app and create GitHub releases (or artifacts). For code signing on Windows/macOS, configure the platform-specific signing and set the required secrets; see [Sign Windows](/distribute/sign/windows/) and [Sign macOS](/distribute/sign/macos/).

## tauri-action (GitHub Actions)

- **Repository**: [tauri-apps/tauri-action](https://github.com/tauri-apps/tauri-action). Use the action in a workflow; it can also initialize Tauri in a repo that doesn’t have it yet (see action readme).
- **Inputs**: See the action’s [readme](https://github.com/tauri-apps/tauri-action/#inputs). **projectPath** — use when the app is not at the repo root.
- **Typical steps**: Checkout → install Linux deps (if matrix includes Linux) → setup Node (with cache) → setup Rust (e.g. `dtolnay/rust-toolchain@stable`, `swatinem/rust-cache@v2`) → install frontend deps / run build if not using **beforeBuildCommand** → run **tauri-action** (builds and creates release).
- **Permissions**: Job needs `contents: write` to create releases and upload assets.
- **Triggers**: Common options: push to a branch (e.g. `release`), push of a version tag (e.g. `app-v*`), or `workflow_dispatch`. The action can create a git tag and release from the app version.
- **Matrix**: Use a strategy matrix for multiple targets (e.g. Windows x64, Linux x64, Linux Arm64, macOS x64, macOS Arm64). Install platform-specific deps per runner (e.g. WebKit/Edge driver for tests).

## Signing in CI

- **Windows**: Set **TAURI_SIGNING_PRIVATE_KEY** (and **TAURI_SIGNING_PRIVATE_KEY_PASSWORD**) as secrets; optionally **TAURI_WINDOWS_SIGNTOOL_PATH**.
- **macOS**: Set **APPLE_CERTIFICATE** (base64 .p12), **APPLE_CERTIFICATE_PASSWORD**, **APPLE_ID**, **APPLE_PASSWORD**, **APPLE_TEAM_ID** (or use **APPLE_API_KEY** / **APPLE_API_ISSUER** / **APPLE_API_KEY_PATH** for notarization). Use a macOS runner.
- See [distribute-signing](references/distribute-signing.md) and the platform Sign docs for full lists.

## Other CI and CrabNebula Cloud

- You can run **tauri build** (and **tauri bundle**) in any CI (GitLab, Jenkins, etc.) as long as the runner has Rust, Node, and the required system libs per platform. Use **CI=1** (or the CLI’s CI mode) to avoid interactive prompts.
- **CrabNebula Cloud** offers hosted build and distribution; see [CrabNebula Cloud](/distribute/pipelines/crabnebula-cloud/) for setup.

## Key Points

- Keep the step that runs **tauri-action** (or `tauri build` + upload) so artifacts are produced and published. Add lint/test steps as needed; preserve **contents: write** for release jobs.
- For **updater**: Configure the updater endpoints so the app can query the same release (e.g. GitHub releases) for updates; see the [updater](/plugin/updater/) plugin and pipeline docs.

<!--
Source references:
- https://v2.tauri.app/distribute/pipelines/github/
- https://github.com/tauri-apps/tauri-action
- https://github.com/tauri-apps/tauri-docs
-->
