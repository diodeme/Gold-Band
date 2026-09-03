---
name: tauri-features-plugins
description: Tauri plugin development — Builder, lifecycle hooks, commands, permissions, state. Use when authoring or extending Tauri plugins.
---

# Plugin Development

Tauri plugins extend the app with Rust (and optional JS) APIs. A plugin is a Cargo crate (and optionally an NPM package for the frontend). Plugins hook into the lifecycle, expose commands, and can manage state.

## Naming and Layout

- Crate: **tauri-plugin-&lt;name&gt;** (e.g. `tauri-plugin-fs`). Name is used in `tauri.conf.json > plugins` and in permission identifiers (e.g. `fs:default`).
- NPM (optional): e.g. `@scope/plugin-&lt;name&gt;` or `tauri-plugin-&lt;name&gt;-api`. Use `plugin new` to scaffold; use `--no-api` if you don’t need the JS package.

Scaffold:

```bash
npx @tauri-apps/cli plugin new my-plugin
```

Typical layout: `src/` (lib.rs, commands.rs, desktop.rs, mobile.rs, error.rs, models.rs), **permissions/** (TOML), optional **guest-js/** and mobile projects.

## Plugin Definition

```rust
use tauri::plugin::{Builder, TauriPlugin};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config { timeout: usize }

pub fn init<R: tauri::Runtime>() -> TauriPlugin<R, Config> {
    Builder::<R, Config>::new("my-plugin")
        .setup(|app, api| {
            let timeout = api.config().timeout;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::my_command])
        .build()
}
```

App registers with `tauri::Builder::default().plugin(my_plugin::init())`. Config comes from `tauri.conf.json` under `plugins.my-plugin`.

## Lifecycle Hooks

- **setup**: When the plugin is initialized. Register state, mobile plugins, background tasks.
- **on_navigation**: When the webview navigates. Return `false` to cancel.
- **on_webview_ready**: When a new window is created. Run per-window init (e.g. inject scripts, register listeners).
- **on_event**: Event loop events (e.g. `RunEvent::ExitRequested`, `RunEvent::Exit`). Use for cleanup or preventing exit.
- **on_drop**: When the plugin is destroyed.

## Exposing Commands

Commands are normal Tauri commands. Define them in the plugin crate and pass to `Builder::invoke_handler(tauri::generate_handler![...])`. Frontend calls them with `invoke('plugin:my-plugin|command_name', args)`. Commands are **not** callable from the frontend until a **capability** grants a permission that allows them.

## Permissions

- Define permissions in **permissions/** as TOML. Each permission has an **identifier**, optional **description**, **commands.allow** / **commands.deny**, and optional **scope.allow** / **scope.deny**.
- **Permission sets** (`[[set]]`) group permissions. **Default** permission set is special: it’s often auto-added when the app adds the plugin.
- Use **build.rs** with `tauri_plugin::Builder::new(COMMANDS).build()` to autogenerate `allow-&lt;command&gt;` and `deny-&lt;command&gt;` permissions. For custom scopes, add `global_scope_schema(schemars::schema_for!(ScopeEntry))` (and optionally command scope schema).
- In commands, read **CommandScope\<T\>** or **GlobalScope\<T\>** to enforce allowed/denied scope entries. Validate both when implementing secure behavior.

## State and Extension Traits

Plugins can `app.manage(MyPluginState)` in `setup`. Expose an extension trait on `Manager` (e.g. `fn my_plugin(&self) -> State<MyPluginState>`) so the app can call `app.my_plugin()` in setup or in other code. Same pattern as official plugins (e.g. `GlobalShortcutExt`).

## Mobile

Use `plugin android add` / `plugin ios add` to add Android/iOS projects. Implement commands in Kotlin/Swift and call them from Rust. Mobile lifecycle hooks and permissions are documented in the mobile plugin guide.

## Key Points

- One plugin = one crate (and optional NPM). Name it `tauri-plugin-&lt;name&gt;` and configure under `plugins.&lt;name&gt;` in config.
- Commands must be allowed by a **capability** (via permissions) to be invokable from the frontend.
- Use **permissions** and **scopes** to constrain what each window can do; check scopes in command handlers.

<!--
Source references:
- https://v2.tauri.app/develop/plugins/
- https://v2.tauri.app/plugin/
- https://github.com/tauri-apps/tauri-docs
-->
