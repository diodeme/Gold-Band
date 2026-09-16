---
name: tauri-distribute-packaging
description: Tauri distribution — build, bundle, installers (DMG, MSI, AppImage, etc.), code signing overview. Use when packaging or releasing Tauri apps.
---

# Distribute (Packaging and Signing)

Tauri provides **build** and **bundle** steps to produce platform installers and app bundles. Most distribution paths require **code signing**; see platform-specific signing docs.

## Building

```bash
pnpm tauri build
```

Builds the Rust app and bundles the frontend. Output format depends on **bundle** config. Use **tauri.conf.json > version** (or Cargo.toml `package.version`) for app version; some platforms have version-string rules.

## Build vs Bundle

- **tauri build**: Full build + default bundle formats.
- **tauri build --no-bundle**: Compile only; no installer/bundle.
- **tauri bundle**: Create bundles from an already-built app. Use `--bundles app,dmg` (or other formats) and optionally `--config path/to/tauri.appstore.conf.json` for store-specific config.

Example (macOS): build once, then bundle for direct download vs App Store:

```bash
pnpm tauri build -- --no-bundle
pnpm tauri bundle -- --bundles app,dmg
pnpm tauri bundle -- --bundles app --config src-tauri/tauri.appstore.conf.json
```

## Platform Outputs

- **Linux**: Debian (.deb), RPM, AppImage, Flatpak, Snap, AUR. Signing: see Linux signing docs.
- **macOS**: App bundle (.app), DMG. Code signing and notarization required for distribution.
- **Windows**: MSI or other installer (see windows-installer docs). Code signing required for trust.
- **Android**: APK/AAB; signing required for Play Store.
- **iOS**: App Store; signing and provisioning required.

## Signing

Code signing validates the provider and (on some platforms) is required for installation or store submission. Configure per platform:

- **macOS**: Sign + notarize for distribution outside the App Store.
- **Windows**: Sign executables/installers.
- **Linux**: Platform-specific (e.g. Flatpak, Snap).
- **Android / iOS**: Keystore and provisioning profiles.

See the [Sign](/distribute/sign/) section for each platform.

## Key Points

- Use **--no-bundle** and **tauri bundle** when you need multiple bundle flavours (e.g. beta, store vs direct) or different configs.
- Version is read from config or Cargo.toml; keep them in sync. Signing is mandatory for most store and user-trusted distribution.

<!--
Source references:
- https://v2.tauri.app/distribute/
- https://github.com/tauri-apps/tauri-docs
-->
