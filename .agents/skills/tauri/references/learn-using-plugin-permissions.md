---
name: tauri-learn-using-plugin-permissions
description: Using plugin permissions — add plugin, capability, allow commands, default permissions. Use when enabling a plugin in a window.
---

# Using Plugin Permissions

To use a **plugin** from the frontend, you must: (1) add the plugin to the app (Cargo + Builder), (2) add the plugin’s **permissions** to a **capability** that applies to the window. Without the right capability, plugin commands are **denied**.

## Add the plugin

- **Official/workspace plugins**: Often `pnpm tauri add <name>` (e.g. `pnpm tauri add fs`) adds the Cargo dependency and registers the plugin. Or add the crate manually and call `.plugin(tauri_plugin_xyz::init())` on the Builder.
- **Community plugins**: `cargo add tauri-plugin-<name>` and `pnpm add @tauri-apps/plugin-<name>` (or the plugin’s npm package). Register with `.plugin(...)` in Rust.

## Grant permissions in a capability

- Open (or create) a capability file in **src-tauri/capabilities/** (e.g. `default.json`).
- In **permissions**, reference the plugin’s permission identifiers. Examples:
  - **core:default** — standard core API.
  - **fs:default** or **fs:allow-read-file**, **fs:scope-...** — file system plugin. See [plugin/file-system](/plugin/file-system/) for setup and scopes.
  - **shell:allow-execute** with scope for sidecar name/args — shell/sidecar.
- Plugin docs and **gen/schemas/desktop-schema.json** (after build) list the permission IDs. Use **$schema** in the capability file for IDE autocomplete.

## Default permissions

- Many plugins ship a **default** permission set (e.g. `fs:default`) that enables common commands. You can use **fs:default** or combine smaller permissions (e.g. **fs:allow-read-file** + **fs:scope-home**) for least privilege.
- **core:default** does **not** include every window command; e.g. **core:window:allow-start-dragging** is separate for custom titlebars.

## Key Points

- **Capability** = which windows get which permissions. **Permission** = what a command or scope is allowed to do. Always add the plugin to the Builder and then reference its permissions in a capability for the target window(s).
- If a plugin command returns a “denied” or permission error, add the matching **allow** permission (and scope if required) to the capability for that window.

<!--
Source references:
- https://v2.tauri.app/learn/security/using-plugin-permissions/
- https://v2.tauri.app/plugin/
- https://github.com/tauri-apps/tauri-docs
-->
