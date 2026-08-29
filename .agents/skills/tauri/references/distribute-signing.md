---
name: tauri-distribute-signing
description: Code signing per platform — macOS (cert, notarize), Windows (OV/EV, SmartScreen), Linux, Android, iOS. Use when configuring signing for distribution.
---

# Code Signing by Platform

Code signing is required (or strongly recommended) for store distribution and to avoid “untrusted app” warnings. Each platform has different requirements and config; use the platform-specific Sign docs for full steps.

## macOS

- **Required** for App Store and to avoid “broken” app warnings when downloaded from the web. **Notarization** is required for distribution outside the App Store (direct download).
- **Prerequisites**: Apple Developer account (paid or free; free cannot notarize). Signing is done on an Apple device.
- **Certificate**: Create a CSR, then in Apple Developer create **Apple Distribution** (App Store) or **Developer ID Application** (outside store). Install the downloaded `.cer` in Keychain. **Signing identity** = keychain entry name; find with `security find-identity -v -p codesigning`.
- **Config**: `tauri.conf.json > bundle > macOS > signingIdentity` (or env **APPLE_SIGNING_IDENTITY**). For notarization: **APPLE_ID**, **APPLE_PASSWORD**, **APPLE_TEAM_ID** (or **APPLE_API_KEY**, **APPLE_API_ISSUER**, **APPLE_API_KEY_PATH**). **APPLE_CERTIFICATE** (base64 .p12) for CI.
- See [Sign macOS](/distribute/sign/macos/) for notarization and hardening.

## Windows

- **Required** for Microsoft Store. **Recommended** to avoid SmartScreen “unknown publisher” when downloading from the web; unsigned apps can still run if the user bypasses the warning.
- **Certificate**: Use a **code signing** certificate (not SSL). OV (Organization Validated) or EV (Extended Validation); EV gets faster SmartScreen reputation. Convert `.cer` to `.pfx` for Tauri/signtool.
- **Config / env**: **TAURI_SIGNING_PRIVATE_KEY** (path or content), **TAURI_SIGNING_PRIVATE_KEY_PASSWORD**. **TAURI_WINDOWS_SIGNTOOL_PATH** to point to `signtool.exe`.
- See [Sign Windows](/distribute/sign/windows/) for OV/EV and custom sign commands.

## Linux

- Signing is platform-specific (e.g. Flatpak, Snap, package signing). See [Sign Linux](/distribute/sign/linux/) and the format docs (e.g. Debian, RPM) for GPG or other requirements. **TAURI_SIGNING_RPM_KEY** / **TAURI_SIGNING_RPM_KEY_PASSPHRASE** for RPM.

## Android

- **Required** for Play Store and updates. Use a keystore (e.g. `keytool` to create). Configure in the Android project and Tauri/bundle docs. See [Sign Android](/distribute/sign/android/).

## iOS

- **Required** for App Store and device install. Certificates and **provisioning profiles** in Apple Developer; configure in Xcode and Tauri. **APPLE_DEVELOPMENT_TEAM**, **APPLE_API_KEY_PATH** (and related) for CI. See [Sign iOS](/distribute/sign/ios/).

## Key Points

- **Never** commit private keys or passwords; use env vars or CI secrets. Use **TAURI_SIGNING_*** and **APPLE_*** env vars for automation.
- Match certificate type to distribution path (e.g. Developer ID for direct macOS download; Apple Distribution for App Store).

<!--
Source references:
- https://v2.tauri.app/distribute/sign/macos/
- https://v2.tauri.app/distribute/sign/windows/
- https://v2.tauri.app/distribute/sign/linux/
- https://v2.tauri.app/distribute/sign/android/
- https://v2.tauri.app/distribute/sign/ios/
- https://github.com/tauri-apps/tauri-docs
-->
