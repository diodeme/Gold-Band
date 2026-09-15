---
name: tauri-security-scope
description: Tauri permission scopes — allow/deny per command or global, path/URL patterns, plugin scope types. Use when restricting command behavior (e.g. fs paths, http URLs).
---

# Command Scopes

**Scopes** define what is allowed or denied for a command (e.g. which paths or URLs). They are **allow** or **deny**; **deny** overrides **allow**. Scope types are plugin- or app-defined (e.g. path strings, URL patterns) and must be enforced inside the command implementation.

## Defining Scopes in Permissions

In a permission file (TOML), use **scope.allow** and **scope.deny**:

```toml
[[permission]]
identifier = "scope-applocaldata-recursive"
description = "Recursive access to $APPLOCALDATA."

[[permission.scope.allow]]
path = "$APPLOCALDATA/**"
```

```toml
[[permission]]
identifier = "deny-webview-data-linux"
description = "Deny read to webview data on Linux."
platforms = ["linux"]

[[scope.deny]]
path = "$APPLOCALDATA/**"
```

- **path**: Often a glob (e.g. `$HOME/*`, `$RESOURCE/**/*`). Plugins like **fs** use path scopes; **http** uses URL scopes.
- **platforms**: Optional; restrict the permission to specific OSes.

## Command vs Global Scope

- **Command scope**: Permission allows specific **commands** and optionally **scope.allow/deny** for those commands. In the command handler, read **CommandScope\<T\>** to get allowed/denied entries and validate the request (e.g. path or URL) against them.
- **Global scope**: Permission has **no** commands.allow/deny, only **scope.allow/deny**. Applies to the whole plugin. In the command handler, read **GlobalScope\<T\>** and enforce the same way.

Check **both** command and global scope when implementing secure commands.

## Merging and Sets

- Combine permissions into a **set** (e.g. allow a directory but deny a subfolder) and reference the set in capabilities. Example: a set that includes `scope-applocaldata-recursive` and `deny-webview-data-linux` so the app can read app data but not webview data on Linux.
- Deny scopes take precedence; order and merging follow the permission/set rules in the docs.

## Key Points

- Scopes are **enforced in Rust** (or plugin code). The permission file only declares what is allowed/denied; the command must call `scope.allows()` / `scope.denies()` (or equivalent) and reject requests that violate the scope. Audit the implementation for bypasses.
- Use **$RESOURCE**, **$APPLOCALDATA**, **$HOME**, etc. for portable paths; plugins document their scope schema (e.g. path vs URL, glob rules).

<!--
Source references:
- https://v2.tauri.app/security/scope/
- https://github.com/tauri-apps/tauri-docs
-->
