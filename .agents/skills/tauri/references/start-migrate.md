---
name: tauri-start-migrate
description: Migrating from Tauri 1.x or 2.x beta — migrate command, config and API changes, mobile prep. Use when upgrading an existing Tauri app.
---

# Migrating to Tauri 2

The Tauri v2 CLI provides a **migrate** command that automates many config and code changes. It is **not** a substitute for reading the full migration guide; review the docs and apply remaining changes by hand.

## Automated Migration

```bash
pnpm update @tauri-apps/cli@latest
pnpm tauri migrate
```

(or npm/yarn/cargo equivalent). Run from the project root. The command updates config shape, capability/permission format, and other breaking changes where possible. It does **not** cover every API change; read the [upgrade guide](/start/migrate/from-tauri-1/) and [CLI reference](/reference/cli/#migrate) for details.

## Summary of Changes (1.x → 2.0)

- **Config**: `package.productName` and `package.version` move to top-level (e.g. `productName`, `version`). Other renames and moves; see config reference.
- **Mobile**: If targeting mobile, the crate must produce a library (`[lib]`, `crate-type = ["staticlib", "cdylib", "rlib"]`). Entry point moves to `lib.rs` with `#[cfg_attr(mobile, tauri::mobile_entry_point)] pub fn run() { ... }`; `main.rs` calls `app_lib::run()`.
- **Capabilities / permissions**: New ACL model; capabilities reference permissions by identifier. Sidecar and shell permissions use the new scope format; `migrate` updates many of these.
- **API renames and removals**: Various renames (e.g. `Window` → `WebviewWindow` in places), and deprecated APIs removed. Check the migration guide and release notes.

## From 2.x Beta

See [Upgrade from Tauri 2 beta](/start/migrate/from-tauri-2-beta/) for beta-specific changes.

## Key Points

- Always **read the full migration guide** and **back up** the project before running `migrate`. Fix any remaining breakages and test on all target platforms.
- After migrating, run `tauri dev` and `tauri build` and fix config or code issues; update frontend imports if APIs moved (e.g. `@tauri-apps/api` paths).

<!--
Source references:
- https://v2.tauri.app/start/migrate/from-tauri-1/
- https://v2.tauri.app/reference/cli/#migrate
- https://github.com/tauri-apps/tauri-docs
-->
