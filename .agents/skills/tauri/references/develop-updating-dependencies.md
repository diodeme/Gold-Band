---
name: tauri-develop-updating-dependencies
description: Updating Tauri and dependencies — npm/Cargo, version sync (tauri, tauri-build, @tauri-apps/api, plugins). Use when upgrading Tauri or keeping deps in sync.
---

# Updating Dependencies

Keep **tauri**, **tauri-build**, **@tauri-apps/cli**, **@tauri-apps/api**, and any **tauri-plugin-*** crates/npm packages on compatible versions. Mismatched minor versions can cause runtime or build errors.

## npm Packages

Update CLI and API to latest (or a target version):

```bash
pnpm update @tauri-apps/cli @tauri-apps/api --latest
# or: npm install @tauri-apps/cli@latest @tauri-apps/api@latest
```

Check for outdated:

```bash
pnpm outdated @tauri-apps/cli
```

## Cargo Packages

- Check [crates.io](https://crates.io/crates/tauri/versions) for **tauri** and **tauri-build**.
- In **src-tauri/Cargo.toml**, set the same **minor** version for both:

```toml
[build-dependencies]
tauri-build = "2.x"

[dependencies]
tauri = { version = "2.x" }
```

Then:

```bash
cd src-tauri
cargo update
```

Use [cargo-edit](https://github.com/killercup/cargo-edit) (`cargo upgrade`) to bump versions automatically if preferred.

## Version Sync Rules

- **@tauri-apps/api** (npm) and **tauri** (Cargo) should share the same **minor** version (e.g. 2.2.x) so the JS API matches the Rust runtime.
- **Plugins**: Keep **exact** versions in sync between npm and Cargo when the plugin ships both (e.g. `@tauri-apps/plugin-fs` 2.2.1 and `tauri-plugin-fs` 2.2.1). Patch releases may introduce coordinated changes.

## Key Points

- After upgrading, run `tauri dev` and `tauri build` and fix any breaking changes (config, capabilities, API renames). See the [migration guide](/start/migrate/from-tauri-1/) for major upgrades.

<!--
Source references:
- https://v2.tauri.app/develop/updating-dependencies/
- https://github.com/tauri-apps/tauri-docs
-->
