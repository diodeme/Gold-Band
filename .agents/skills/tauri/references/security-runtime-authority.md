---
name: tauri-security-runtime-authority
description: Runtime authority — Core enforces permissions and scopes on every invoke; denied requests never reach the command. Use when reasoning about Tauri security model.
---

# Runtime Authority

The **runtime authority** lives in the Tauri Core. It holds all **permissions**, **capabilities**, and **scopes** at runtime and decides whether an invoke from the WebView is allowed before any command runs.

## Flow

1. The frontend calls **invoke(command, args)**.
2. The **runtime authority** receives the request.
3. It checks: Is this **origin** (window/URL) allowed to call this **command**? Is it covered by a **capability** that grants the required **permission**? If the command has **scopes**, are they satisfied for this request?
4. If **not** allowed, the request is **denied** and the Tauri command is **never** invoked.
5. If allowed, scopes (if any) are attached to the request and the command is invoked with the usual arguments and injected scope (e.g. **CommandScope** / **GlobalScope**).

## Implications

- **Frontend cannot bypass** the runtime authority. Even if the frontend calls `invoke('dangerous_command')`, the Core will deny it unless a capability grants that command to that window.
- **Capabilities** define which windows (and optionally remote URLs and platforms) get which permissions. **Permissions** define which commands (and scopes) are allowed. **Scopes** are enforced in the command implementation using the injected scope data.
- Security relies on: (1) correct capability/permission/scope configuration, (2) command implementations that actually check the injected scopes (e.g. path or URL) and reject violations.

## Key Points

- Denials happen **before** the command runs. Use capabilities and scopes to enforce least privilege; implement scope checks inside commands for path/URL-sensitive behavior.
- See [Capabilities](/security/capabilities/), [Permissions](/security/permissions/), and [Scopes](/security/scope/) for configuration; see the security [lifecycle](/security/lifecycle/) for the full threat model.

<!--
Source references:
- https://v2.tauri.app/security/runtime-authority/
- https://github.com/tauri-apps/tauri-docs
-->
