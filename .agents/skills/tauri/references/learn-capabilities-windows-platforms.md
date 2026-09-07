---
name: tauri-learn-capabilities-windows-platforms
description: Different capabilities per window and platform — multi-window, capability files per window, platforms array. Use when assigning capabilities by window or OS.
---

# Capabilities for Different Windows and Platforms

You can give **different capabilities** to different **windows** and restrict capabilities to specific **platforms**. Use this for least privilege: e.g. one window gets fs + dialog, another only dialog; or fs only on Linux/Windows.

## Multiple windows

- **Config**: Define windows in **tauri.conf.json > app.windows** (label, title, size, etc.) or create them in Rust with **WebviewWindowBuilder::new(app, label, url)**.
- **Capability files**: In **src-tauri/capabilities/**, use the **windows** field to list which window **labels** get this capability:

```json
{
  "identifier": "fs-read-home",
  "description": "File access to home",
  "windows": ["first"],
  "permissions": ["fs:allow-home-read"]
}
```

- Give another window a different set: e.g. a second capability file with **"windows": ["second"]** and only **dialog:allow-ask**. Each window gets the union of all capabilities that list its label (and match platform).

## Platform-specific capabilities

- Use the **platforms** field to restrict a capability to certain OSes: **linux**, **windows**, **macos**, **android**, **ios**.

```json
{
  "identifier": "fs-read-home",
  "description": "File access to home (desktop only)",
  "windows": ["first"],
  "permissions": ["fs:allow-home-read"],
  "platforms": ["linux", "windows"]
}
```

- On macOS, android, or ios this capability is ignored; on Linux and Windows it applies to the listed windows.

## Organizing capability files

- **One file per concern** (e.g. `filesystem.json`, `dialog.json`) makes it easy to see which window gets which permissions. Reference the same permission sets across files by identifier; use **windows** and **platforms** so each file describes a slice of (window, platform) space.
- Enable only what you need per window: e.g. main window gets full app + fs scope; settings window gets only dialog and store.

## Key Points

- **windows** = which window labels get this capability. **platforms** = which OSes. Both are optional; omit for “all windows” or “all platforms.”
- When you explicitly set **app.security.capabilities** in config, only those capabilities are active; list each capability file identifier (or inline capability) there.

<!--
Source references:
- https://v2.tauri.app/learn/security/capabilities-for-windows-and-platforms/
- https://github.com/tauri-apps/tauri-docs
-->
