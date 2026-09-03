---
name: tauri-features-deep-linking
description: Deep linking — custom URL scheme, App Links (Android), Universal Links (iOS), desktop argv. Use when handling custom URLs or OAuth callbacks.
---

# Deep Linking

The **deep-link** plugin lets your app be opened via a **custom URL** (e.g. `myapp://path`) or **https** links (with verification on mobile). Use for OAuth callbacks, “open with” handlers, or in-app routing from external links.

## Setup

- Add the plugin: `pnpm tauri add deep-link` (or `cargo add tauri-plugin-deep-link` + register with `.plugin(tauri_plugin_deep_link::init())` + npm install `@tauri-apps/plugin-deep-link`).
- Configure **tauri.conf.json > plugins > deep-link**: define **schemes** (and on mobile, optional **domains** for verified links).

## Custom URI scheme (all platforms)

- **No server required.** Define a scheme (e.g. `myapp`). Links like **myapp://path?query** open your app. On Android you can use **scheme** without **host**; on iOS use **appLink: false** (or omit) so the plugin adds **CFBundleURLTypes** and no `.well-known` host is needed.
- **Desktop**: On Linux and Windows the link is passed as a **command-line argument** to a new process. Use the **single-instance** plugin with the **deep-link** feature so one instance receives the link; in the single-instance callback you get **argv** (and the deep-link event is already emitted). Validate the URL format; Tauri matches argv against configured schemes, but users could pass a fake URL in argv.
- **Listen**: Use the deep-link plugin’s API (e.g. **onOpenUrl** or the event it emits) in the frontend or Rust to handle the URL and route/navigate.

## Verified links (mobile)

- **Android App Links (https)**: Host **.well-known/assetlinks.json** on your domain. Include **package_name** (app id with `-` → `_`) and **sha256_cert_fingerprints** of your signing certs. See [verify Android app links](https://developer.android.com/training/app-links/verify-android-applinks).
- **iOS Universal Links (https)**: Host **.well-known/apple-app-site-association** (Content-Type: application/json, HTTPS). Include **appIDs** (`$DEVELOPMENT_TEAM_ID.$BUNDLE_ID`) and **components** (path patterns). Use **tauri.conf.json > identifier** and bundle iOS **developmentTeam** (or **APPLE_DEVELOPMENT_TEAM**).

## Key Points

- **Custom scheme** = no verification; **https + .well-known** = verified (no custom scheme prompt). Prefer verified links on mobile when you control the domain.
- On desktop, **single-instance + deep-link** ensures one process handles the link; validate URL shape in code. Only **statically** configured schemes are handled by Tauri; runtime-registered schemes must be checked via **argv** in the single-instance callback.

<!--
Source references:
- https://v2.tauri.app/plugin/deep-linking/
- https://github.com/tauri-apps/tauri-docs
-->
