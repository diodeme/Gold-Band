---
name: tauri-security-capabilities
description: Tauri capabilities — which permissions apply to which windows; capability files, remote URLs, platforms. Use when restricting frontend API access per window.
---

# Capabilities

Capabilities define **which permissions** are granted (or denied) for **which windows or webviews**. They are the main way to constrain what the frontend can do per window.

## Capability Files

- Stored in **src-tauri/capabilities/** as JSON or TOML.
- Each file defines an **identifier**, optional **description**, **windows** (or webviews), optional **platforms**, and a list of **permissions**.
- By default, **all** capability files in that directory are enabled. Once you set `app.security.capabilities` in `tauri.conf.json`, **only** those listed are used.

Example:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "main-capability",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:path:default",
    "core:event:default",
    "core:window:default",
    "core:window:allow-set-title"
  ]
}
```

Reference in config:

```json
{
  "app": {
    "security": {
      "capabilities": ["main-capability"]
    }
  }
}
```

You can mix **inline** capability objects and **references** (by identifier) in `app.security.capabilities`.

## Target: Windows and Platforms

- **windows**: Array of window **labels** (not titles). Use `"*"` for all windows. Security is keyed by label.
- **platforms**: Optional. One or more of `linux`, `macOS`, `windows`, `iOS`, `android`. Omit to apply to all platforms.

Use separate capability files per platform or per window when you need different permission sets.

## Remote API Access

To allow **remote** origins (e.g. a loaded URL) to call certain commands, add a **remote** block in the capability:

```json
{
  "identifier": "remote-capability",
  "windows": ["main"],
  "remote": {
    "urls": ["https://*.tauri.app"]
  },
  "permissions": ["nfc:allow-scan"]
}
```

Use sparingly. On Linux and Android, Tauri cannot distinguish iframe vs top-level; consider platform-specific implications.

## Default Command Allowlist

By default, **all** commands you register with `invoke_handler` are allowed for all windows. To restrict which commands exist at all, use **build.rs**:

```rust
tauri_build::try_build(
    tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["your_command"])),
)
.unwrap();
```

Capabilities then control **which window** can call which of those commands.

## Security Boundaries

- Capabilities **do** limit what the frontend can invoke and what scopes apply (when permissions define scopes).
- They **do not** protect against: malicious or buggy Rust code, overly broad scopes, missing scope checks in commands, intentional bypasses, WebView 0-days, or compromised dev machines.
- Prefer **window labels** for privilege boundaries; expose window-creation only to higher-privilege windows.

## Schema and Autocomplete

Run a build to generate **gen/schemas/desktop-schema.json** (and mobile if applicable). Set `"$schema": "../gen/schemas/desktop-schema.json"` in capability files for IDE autocomplete of permission identifiers.

## Key Points

- One capability can apply to multiple windows; one window can have multiple capabilities (permissions merge).
- Always set `app.security.capabilities` explicitly when you want least-privilege; otherwise all files in `capabilities/` are used.
- Restrict window creation and high-privilege commands to a small set of windows.

<!--
Source references:
- https://v2.tauri.app/security/capabilities/
- https://v2.tauri.app/reference/config/
- https://github.com/tauri-apps/tauri-docs
-->
