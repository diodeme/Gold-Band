---
name: tauri-reference-webview-versions
description: WebView versions per platform — WebView2 (Windows), WKWebView (macOS/iOS), WebKitGTK (Linux). Use when checking browser/feature compatibility.
---

# WebView Versions

Tauri uses the **OS-provided** WebView; versions and behavior depend on the platform. Use this when checking if a web API or CSS feature is available (e.g. via caniuse or Safari/Chromium version maps).

## Windows: WebView2

- Based on **Microsoft Edge (Chromium)**. WebView2 can self-update; users typically have a recent Chromium build.
- Supported on **Windows 7+**. Preinstalled on Windows 11; on older Windows the Tauri-generated installer can ensure WebView2 is installed.

## macOS & iOS: WKWebView

- Uses the **WebKit** that ships with the OS (macOS 10.10+). Updated with OS updates; older OS versions do not get WebKit updates.
- To see the WebKit version on your Mac:  
  `awk '/CFBundleVersion/{getline;gsub(/<[^>]*>/,"");print}' /System/Library/Frameworks/WebKit.framework/Resources/Info.plist`
- Version format: **$(SYSTEM_VERSION_PREFIX)$(MAJOR).$(MINOR).$(TINY).$(MICRO).$(NANO)**. The prefix maps to macOS/iOS versions (e.g. 17 → macOS 12). See the [webview-versions](/reference/webview-versions/) doc for the full OS ↔ WebKit ↔ Safari version table.

## Linux: WebKitGTK

- Uses **webkit2gtk**. Version varies by distro (e.g. Debian/Ubuntu packages). Check your distro’s package for **webkit2gtk** or **libwebkit2gtk**.
- Docs include an incomplete table of distro ↔ webkitgtk version ↔ Safari equivalent (e.g. 2.36 → Safari 16.0-ish). Always verify against your target distro.

## Key Points

- **No embedded engine**: Tauri does not ship a browser; it uses the system WebView. Support for a given web feature depends on the OS (and on Linux, the distro).
- Use the **WebKit version** (macOS/Linux) or **Edge/Chromium** (Windows) to look up feature support on caniuse or similar.

<!--
Source references:
- https://v2.tauri.app/reference/webview-versions/
- https://github.com/tauri-apps/tauri-docs
-->
