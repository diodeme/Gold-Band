---
name: tauri-develop-plugins-mobile
description: Tauri mobile plugin development — Android (Kotlin/Java), iOS (Swift), desktop vs mobile impl, lifecycle. Use when authoring plugins for Android/iOS.
---

# Mobile Plugin Development

Plugins can run **native mobile code**: Kotlin (or Java) on Android and Swift on iOS. The default plugin template can include Android and iOS projects; use **plugin android init** / **plugin ios init** to add mobile support to an existing plugin.

## Structure

- **desktop.rs** — Desktop implementation (Rust).
- **mobile.rs** — Mobile implementation: Rust code that forwards to native (Kotlin/Swift). Commands are invoked from JS/Rust; native code runs the actual logic.
- **lib.rs** — Shared API and plugin registration; use `impl<R: Runtime> PluginName<R>` for logic shared between desktop and mobile.

## Android

- Plugin is a **Kotlin class** extending `app.tauri.plugin.Plugin`, annotated with `@TauriPlugin`. Methods annotated with `@Command` can be called from Rust or JavaScript.
- Config: use `getConfig(Config::class.java)` in the plugin’s `load(webView)` to read plugin config (parsed from `tauri.conf.json > plugins.<name>`).
- Register the plugin in the app’s Android project and expose commands; see the [Mobile Plugin Development](/develop/plugins/develop-mobile/) guide for lifecycle and project setup.

## iOS

- Plugin is a **Swift class** extending `Plugin` from the Tauri package. Functions with `@objc` and `(_ invoke: Invoke)` (e.g. `@objc private func download(_ invoke: Invoke)`) are callable from Rust or JavaScript.
- Config: implement `Decodable` for a config struct and use `parseConfig(Config.self)` in `load(webview:)`.
- Ship as a **Swift package**; the app’s iOS project depends on it. See the guide for lifecycle and Xcode integration.

## Key Points

- Desktop and mobile can expose the **same command names**; the plugin picks the right implementation per platform. Use **plugin android init** / **plugin ios init** to scaffold mobile projects and wiring.
- Mobile lifecycle hooks (e.g. when the WebView is ready, when the app is backgrounded) are documented in the mobile plugin guide; use them for native setup and teardown.

<!--
Source references:
- https://v2.tauri.app/develop/plugins/develop-mobile/
- https://github.com/tauri-apps/tauri-docs
-->
