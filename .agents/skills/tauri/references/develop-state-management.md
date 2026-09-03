---
name: tauri-develop-state-management
description: Tauri managed state — app.manage(), State<T> in commands, Mutex/async Mutex. Use when sharing global state between commands and lifecycle hooks.
---

# State Management

Tauri lets you store **global state** with `app.manage(T)` and read it in commands or anywhere you have a type that implements the **Manager** trait (e.g. `App`, `AppHandle`, `WebviewWindow`).

## Registering State

In `setup` or before `run()`:

```rust
use tauri::Manager;

struct AppState { counter: u32 }

tauri::Builder::default()
    .setup(|app| {
        app.manage(Mutex::new(AppState { counter: 0 }));
        Ok(())
    })
    .run(tauri::generate_context!())
    .unwrap();
```

Use `Mutex` (or `RwLock`, or Tokio’s `Mutex` for async) when state must be mutated. Tauri does **not** wrap your type in `Arc`; you can use `Arc<Mutex<T>>` if you need to share the same handle elsewhere, but for `State<T>` injection you usually just `manage(Mutex::new(T))`.

## Accessing in Commands

Inject `tauri::State<'_, T>`:

```rust
#[tauri::command]
fn increment(state: tauri::State<'_, Mutex<AppState>>) -> u32 {
    let mut s = state.lock().unwrap();
    s.counter += 1;
    s.counter
}
```

For async commands using Tokio’s `Mutex`: hold the guard across `.await` only if necessary; otherwise `std::sync::Mutex` is often enough.

## Accessing Outside Commands

Use the **Manager** trait. For example in a window event handler or another thread:

```rust
let app_handle = window.app_handle();
let state = app_handle.state::<Mutex<AppState>>();
let mut s = state.lock().unwrap();
s.counter += 1;
```

You can move `AppHandle` into a thread and call `app_handle.state::<T>()` there; no need to wrap state in `Arc` just for that.

## Mutability and Locking

- **Immutable state**: `app.manage(MyConfig { ... })` and use `State<'_, MyConfig>` (read-only).
- **Mutable state**: `app.manage(Mutex::new(MyState::default()))` and use `State<'_, Mutex<MyState>>`. Lock, modify, then drop the guard.
- Prefer `std::sync::Mutex` unless you need to hold the lock across an `.await`; then use Tokio’s `Mutex`.

## Mismatching Types

If you inject `State<'_, WrongType>` and the type doesn’t match what was passed to `manage()`, you get a **runtime panic**. Use a type alias (e.g. `type AppState = Mutex<AppStateInner>`) and consistently use that alias when calling `manage` and in `State<'_, AppState>` to avoid double-wrapping or wrong types.

## Key Points

- One registration per concrete type: `app.manage(T)`. Retrieve with `app.state::<T>()` or `State<'_, T>`.
- Prefer keeping business data and secrets in the Core (managed state) and expose only what’s needed via commands with capability checks.

<!--
Source references:
- https://v2.tauri.app/develop/state-management/
- https://github.com/tauri-apps/tauri-docs
-->
