---
name: tauri-reference-acl-core-permissions
description: Core permissions overview — core:default, app, event, window, path, menu, tray, resources, webview, image. Use when granting or restricting core API access.
---

# Core Permissions (ACL)

**Core permissions** control access to built-in Tauri APIs (app, event, window, path, menu, tray, resources, webview, image). Reference them in **capability** files by identifier (e.g. `core:window:default`, `core:window:allow-set-title`).

## core:default

Including **core:default** adds the default permission set for core, which typically includes:

- **core:app:default**, **core:event:default**, **core:image:default**, **core:menu:default**, **core:path:default**, **core:resources:default**, **core:tray:default**, **core:webview:default**, **core:window:default**

Use **core:default** when a window should have standard core API access; then allow or deny specific commands (e.g. **core:window:allow-set-title**) as needed.

## Per-domain permissions

Each domain (app, event, window, path, menu, tray, resources, webview, image) has:

- A **default** permission (e.g. `core:app:default`) that enables a common set of commands.
- **allow-&lt;command&gt;** and **deny-&lt;command&gt;** permissions (e.g. `core:app:allow-version`, `core:event:allow-emit`, `core:window:allow-close`, `core:window:allow-start-dragging`). Deny overrides allow.

Refer to the [Core Permissions](/reference/acl/core-permissions/) reference for the full table of identifiers and descriptions. Generated **gen/schemas/desktop-schema.json** (after a build) lists all available permissions for autocomplete in capability files.

## Key Points

- **Capabilities** assign permissions to windows; list core permissions (e.g. `core:window:default`, `core:window:allow-set-title`) in the capability’s **permissions** array.
- For **plugin** permissions (fs, shell, etc.), see the [Plugins](/plugin/) section and each plugin’s permission docs. Core permissions only cover the built-in Tauri API.

<!--
Source references:
- https://v2.tauri.app/reference/acl/core-permissions/
- https://github.com/tauri-apps/tauri-docs
-->
