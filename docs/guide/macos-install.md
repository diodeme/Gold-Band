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

The installer is published with each new GitHub Release, so you do not need to clone the repository. The first command installs the latest Release and asks before replacing an existing app; use `--yes` to confirm replacement non-interactively.

```bash
installer="${TMPDIR:-/tmp}/install-gold-band-macos.sh" && curl -fsSL https://github.com/diodeme/Gold-Band/releases/latest/download/install-gold-band-macos.sh -o "$installer" && bash "$installer"
installer="${TMPDIR:-/tmp}/install-gold-band-macos.sh" && curl -fsSL https://github.com/diodeme/Gold-Band/releases/latest/download/install-gold-band-macos.sh -o "$installer" && bash "$installer" --yes
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
