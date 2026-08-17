# macOS Installation and Troubleshooting Guide

<!-- DOCS-I18N:START -->

**English** | [中文](./macos-install.zh-CN.md)

<!-- DOCS-I18N:END -->

Until Apple Developer Program credentials are configured, the Gold Band macOS Release is ad-hoc signed by the Tauri bundler but is not signed with Developer ID or notarized by Apple. Gatekeeper may therefore block the app the first time it is opened after downloading. This guide provides the native macOS manual installation flow and an installation script for new Releases that include `.sha256` checksum files.

> The Tauri updater private key and `.sig` files used by the release pipeline only verify the integrity of in-app updates. They are separate from Gatekeeper's Developer ID and Apple notarization trust system.

## 1. Choose the Correct Architecture

- Apple Silicon (M-series): download the `aarch64` build, for example `Gold.Band_x.y.z_aarch64.dmg`.
- Intel Mac: download the `x64` build.

The installation script selects the architecture from `uname -m` by default. You can explicitly set `GOLD_BAND_ARCH=aarch64` or `GOLD_BAND_ARCH=x64` instead.

## 2. Install Manually

1. Download the DMG for your Mac's architecture from the project's official GitHub Release.
2. Mount the DMG, drag `Gold Band.app` into `/Applications`, and eject the DMG.
3. In Finder, Control-click `Gold Band.app`, choose **Open**, and confirm **Open** again.

This is the exception flow macOS provides for an individual app that has not been notarized. Do not use `sudo spctl --master-disable` to disable Gatekeeper globally.

## 3. Use the Installation Script

`scripts/install-gold-band-macos.sh` does not require Python, jq, Homebrew, or Xcode Command Line Tools. It only uses the macOS built-in `curl`, `plutil`, `shasum`, `hdiutil`, `codesign`, `ditto`, and `xattr` commands.

```bash
bash scripts/install-gold-band-macos.sh
bash scripts/install-gold-band-macos.sh --yes
GOLD_BAND_VERSION=0.13.0 bash scripts/install-gold-band-macos.sh
GOLD_BAND_VERSION=0.13.0 bash scripts/install-gold-band-macos.sh ./Gold.Band_0.13.0_aarch64.dmg
```

The script follows this fixed sequence:

1. If no version is specified, read the latest version from the GitHub Release API `tag_name`.
2. Download the DMG or reuse an exactly matching file from `~/Downloads`.
3. Download `${DMG_NAME}.sha256` from the same Release, then verify both the filename and SHA256 digest.
4. Verify the DMG's internal integrity with `hdiutil verify`.
5. Accept only an app named `Gold Band.app` with bundle identifier `local.gold-band.desktop` that passes `codesign --verify --deep --strict`.
6. Stage the app on the `/Applications` filesystem, verify it again, and replace the old app. Restore the previous version if the switch fails.
7. Remove `com.apple.quarantine` only from the staged app after every check passes, then open it from Finder.

The script only supports new Releases published with a matching `.sha256` sidecar after the checksum workflow was introduced. Historical Releases do not have this asset; use the manual installation flow above for those versions.

## 4. Integrity Boundaries

After all platform assets are uploaded, both Release workflows generate same-name `.sha256` files with streaming SHA256 for DMG, macOS updater archives, EXE, MSI, AppImage, DEB, and RPM artifacts. The installer stops if the sidecar is missing, malformed, names a different file, or does not match the downloaded DMG. It does not provide a weak-verification fallback.

The DMG and `.sha256` file come from the same GitHub Release. This detects download corruption, truncation, and asset mismatches, but it cannot protect against compromise of the official GitHub repository or its Release publishing permissions. After Apple Developer Program credentials become available, the existing Tauri release flow should still perform Developer ID signing and notarization.

## 5. Historical Issue: Older Builds Exit Because of the Settings Schema

In v0.12.0 through v0.12.3, the application exits if a newer development build has already written `settingsSchemaVersion: 3` to `~/.gold-band/settings.json` while the older binary only supports schema v2. The application reports:

```text
failed to start Gold Band desktop: settings schema version 3 is newer than supported version 2
```

This issue was fixed in v0.12.4. Upgrade to v0.12.4 or later instead of downgrading the persisted settings schema to run an older build.

## 6. Future Official Release Path

The script is a temporary installation path while Apple Developer Program credentials are unavailable. Once credentials are ready, the existing release workflow will pass the certificate, Developer ID identity, Apple ID, app-specific password, and Team ID to the Tauri bundler so the same build path can perform official signing and notarization. Users should no longer need the Gatekeeper workaround script at that point.
