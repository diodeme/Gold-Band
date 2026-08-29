---
name: tauri-develop-windows
description: Creating and managing windows — tauri.conf.json app.windows, WebviewWindowBuilder, label, url, visible. Use when adding or controlling app windows.
---

# Creating and Managing Windows

Tauri apps can have multiple **webview windows**. Define them in **tauri.conf.json** (app.windows) or create them in Rust with **WebviewWindowBuilder**. Each window has a **label** (unique id); capabilities use labels to grant permissions per window.

## Config (tauri.conf.json)

Under **app.windows** (or equivalent), list window objects:

```json
{
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "My App",
        "width": 800,
        "height": 600,
        "visible": true,
        "url": "/"
      },
      {
        "label": "splashscreen",
        "title": "Loading",
        "visible": true,
        "url": "/splashscreen"
      }
    ]
  }
}
```

- **label** — Unique identifier; used in capabilities (windows array), `get_webview_window(label)`, and frontend `getWindow(label)`. Not the same as **title** (user-visible).
- **url** — Path or URL the webview loads (e.g. `/`, `/splashscreen`, or full URL). For dev, the dev server serves these.
- **visible** — If false, the window is created but hidden (e.g. main window hidden until splash is done). **title**, **width**, **height**, **decorations**, etc. are optional; see config reference.

## Creating Windows in Rust

Use **WebviewWindowBuilder** in **setup** or in a command:

```rust
use tauri::{WebviewUrl, WebviewWindowBuilder};

// In setup or command:
let url = WebviewUrl::App("index.html".into());           // bundled app
// WebviewUrl::External("https://...".parse().unwrap())   // external URL
// WebviewUrl::default()                                   // default entry

tauri::WebviewWindowBuilder::new(app, "my-label", url)
    .title("My Window")
    .inner_size(800.0, 600.0)
    .visible(false)
    .build()?;
```

- **WebviewWindowBuilder::new(app, label, url)** — label must be unique. **build()** creates the window.
- To create a window from the frontend, expose a **command** that calls **WebviewWindowBuilder::new** (and ensure the creating window has a capability that allows creating windows, if you restrict that).

## Getting and Controlling Windows

- **Rust**: **app.get_webview_window("main")** returns **Option<WebviewWindow>**. Use **show()**, **hide()**, **close()**, **set_title()**, **eval()**, etc.
- **Frontend**: **getWindow(label)** from `@tauri-apps/api/window` (or **getCurrentWindow()**). Call **close()**, **minimize()**, **toggleMaximize()**, etc. Permissions (e.g. **core:window:allow-close**) must be granted in a capability for that window.

## Key Points

- **Label** is the stable id for capabilities and API; **title** is the window bar text. Use labels in capability **windows** arrays.
- First window is often **main**; define it in config or create it in setup. Use **visible: false** plus **show()** when ready for splashscreen-style flows.

<!--
Source references:
- https://v2.tauri.app/reference/config/#windowconfig
- https://v2.tauri.app/learn/security/capabilities-for-windows-and-platforms/
- https://github.com/tauri-apps/tauri-docs
-->
