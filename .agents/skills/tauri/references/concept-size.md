---
name: tauri-concept-size
description: Tauri app size — why small, Cargo release profile (LTO, strip, opt-level), trimming. Use when optimizing binary size.
---

# App Size

Tauri apps stay small because the WebView is provided by the OS, not bundled. You can further reduce binary size with Cargo **release profile** settings.

## Why Tauri Apps Are Small

- The **webview** is not shipped; the OS supplies it (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux). Only your Rust code, frontend assets, and any bundled resources/sidecars are in the app.
- No embedded browser engine; typical desktop app can be on the order of a few MB or less for the binary plus assets.

## Cargo Release Profile

Add or adjust **src-tauri/Cargo.toml** `[profile.release]`:

```toml
[profile.release]
codegen-units = 1   # Better optimization (slower compile).
lto = true          # Link-time optimization.
opt-level = "s"     # Optimize for size; use "3" for speed.
panic = "abort"     # No unwinding; smaller binary.
strip = true        # Remove debug symbols.
```

- **opt-level "s"**: Prefer size over speed; use **"3"** if you care more about performance.
- **strip = true**: Removes debug symbols from the binary.
- **panic = "abort"**: Reduces code size by not including panic unwinding.

Optional (nightly): **trim-paths = "all"**, **rustflags = ["-Cdebuginfo=0", ...]** for smaller size and fewer paths in the binary. See the [App Size](/concept/size/) docs for full options and references.

## Key Points

- Size gains come mainly from **release profile** (LTO, strip, opt-level) and avoiding large dependencies. The WebView being external is the biggest factor.
- Use **opt-level "s"** for minimal size; **"3"** for maximum speed. Re-measure after changes.

<!--
Source references:
- https://v2.tauri.app/concept/size/
- https://github.com/tauri-apps/tauri-docs
-->
