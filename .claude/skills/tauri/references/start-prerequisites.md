---
name: tauri-start-prerequisites
description: Tauri prerequisites — system deps (Linux/macOS/Windows), Rust, Node, WebView2, mobile (Android/iOS). Use when setting up a dev environment for Tauri.
---

# Prerequisites

Tauri needs **Rust**, **system libraries** (for the WebView and build tools), and optionally **Node.js** (for JS frontends) and **mobile SDKs** (for Android/iOS).

## System Dependencies

- **Linux**: WebView and build deps vary by distro. Examples: Debian/Ubuntu — `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`. Arch — `webkit2gtk-4.1`, `base-devel`, `openssl`, `libappindicator-gtk3`, `librsvg`, `xdotool`. See [prerequisites](/start/prerequisites/) for Fedora, Gentoo, openSUSE, Alpine, NixOS.
- **macOS**: [Xcode](https://developer.apple.com/xcode/) (or Xcode Command Line Tools only if desktop-only: `xcode-select --install`).
- **Windows**: [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with “Desktop development with C++”; [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (often already on Windows 10/11). For MSI builds: enable **VBSCRIPT** in Optional features if you hit `light.exe` errors.

## Rust

Install via [rustup](https://rustup.rs/): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`. On Windows use the installer from rust-lang.org and ensure the **MSVC** toolchain is selected (`rustup default stable-msvc`) for Tauri and tools like Trunk.

## Node.js (optional)

Required only for JavaScript/TypeScript frontends. Install [Node.js LTS](https://nodejs.org); then `node -v` and `npm -v`. Use `corepack enable` if you want pnpm/yarn.

## Mobile (optional)

- **Android**: [Android Studio](https://developer.android.com/studio), set **JAVA_HOME**, install SDK Platform, Platform-Tools, NDK, Build-Tools, Command-line Tools. Set **ANDROID_HOME** and **NDK_HOME**. Add targets: `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`.
- **iOS** (macOS only): Full Xcode (not just CLI tools). Add targets: `rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim`. Install [Homebrew](https://brew.sh) and [Cocoapods](https://cocoapods.org): `brew install cocoapods`.

## Key Points

- Restart the terminal (or system) after installing Rust/Node so PATH is updated. If builds fail, check the [debug/troubleshooting](/develop/debug/) docs and platform-specific prerequisites.

<!--
Source references:
- https://v2.tauri.app/start/prerequisites/
- https://github.com/tauri-apps/tauri-docs
-->
