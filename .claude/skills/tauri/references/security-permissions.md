---
name: tauri-security-permissions
description: Tauri permissions — allow/deny commands, scopes, permission sets. Use when defining or extending plugin/app permissions for capabilities.
---

# Permissions

Permissions describe **what** a capability grants: which commands are allowed (or denied) and which **scopes** apply. They are defined in TOML (or JSON in some contexts) and **referenced by identifier** in capability files.

## Permission Structure

- **identifier**: Unique name. Convention: `<plugin>:default` or `<plugin>:<command-name>` (e.g. `fs:read-files`, `fs:allow-mkdir`). Plugin crate name is without `tauri-plugin-` prefix.
- **commands.allow** / **commands.deny**: List of command names (snake_case) this permission enables or denies.
- **scope.allow** / **scope.deny**: Optional. Paths, URLs, or custom scope entries (plugin-defined) that this permission allows or denies. Used by plugins for fine-grained access (e.g. which paths a command can read).

Example (plugin permission):

```toml
[[permission]]
identifier = "read-files"
description = "Enables file read commands without pre-configured paths."
commands.allow = ["read_file", "read", "open"]

[[permission]]
identifier = "scope-home"
description = "Permits access to top-level $HOME."
[[scope.allow]]
path = "$HOME/*"
```

## Permission Sets

Group multiple permissions under one identifier for easier use in capabilities:

```toml
[[set]]
identifier = "allow-home-read-extended"
description = "Read $HOME and create directories."
permissions = ["fs:read-files", "fs:scope-home", "fs:allow-mkdir"]
```

In a capability, reference the set: `"permissions": ["allow-home-read-extended"]`.

## Where Permissions Live

- **Plugins**: In the plugin crate, under **permissions/** (e.g. `permissions/read-files.toml`). Default permission set (`default`) is often auto-included when the app adds the plugin via the CLI.
- **Application**: In **src-tauri/permissions/** (TOML). Define permissions for your own commands or extend plugin permissions (e.g. a set that combines `fs:read-files` and a custom scope).

Capability files (which reference permissions) can be JSON or TOML and live in **src-tauri/capabilities/**.

## Identifier Rules

- ASCII lowercase `[a-z]`, colons allowed for namespace (e.g. `fs:read-files`). Max length 116 (Cargo identifier rules + plugin prefix).
- Plugin prefix `tauri-plugin-` is added at compile time; use short names in identifiers (e.g. `fs`, not `tauri-plugin-fs`).

## Scopes in Commands

Plugins can define **scope** types (e.g. allowed paths or binaries). In the command handler they read:
- **CommandScope\<T\>** – per-command scope from the capability.
- **GlobalScope\<T\>** – plugin-wide scope when the permission has no commands, only scope entries.

Check both when implementing secure commands. Use `schemars` to generate a JSON schema for scope entries so app developers get autocomplete in capability files.

## Key Points

- Permissions are **referenced** in capabilities; they are not attached to windows directly. A capability ties (windows + platforms + remote) to a list of permissions.
- Use **permission sets** to bundle plugin permissions for common use cases (e.g. `fs:default` or a custom `allow-home-read-extended`).
- Application developers extend or combine plugin permissions in **src-tauri/permissions/** and reference them in **capabilities/**.

<!--
Source references:
- https://v2.tauri.app/security/permissions/
- https://v2.tauri.app/reference/acl/
- https://github.com/tauri-apps/tauri-docs
-->
