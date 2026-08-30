---
name: tauri-develop-commands
description: Tauri commands — #[tauri::command], invoke from frontend, args, return values, errors, async, State, channels. Use when exposing Rust APIs to the frontend.
---

# Calling Rust from the Frontend (Commands)

Commands are Rust functions callable from the frontend via `invoke('command_name', { args })`. They are type-safe, support arguments and return values, and can be async.

## Defining Commands

Annotate a function with `#[tauri::command]` and register it with `invoke_handler(tauri::generate_handler![...])`:

```rust
// src-tauri/src/lib.rs
#[tauri::command]
fn my_custom_command(msg: String) -> String {
    format!("Echo: {}", msg)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![my_custom_command])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Frontend:

```js
import { invoke } from '@tauri-apps/api/core';
const result = await invoke('my_custom_command', { msg: 'Hello' });
```

- **Names**: Must be unique across the app. In `lib.rs` commands cannot be `pub` (macro limitation); in other modules use `pub fn` and register as `module::command_name`.
- **Arguments**: Pass a single object; keys map to Rust params. Use `rename_all = "snake_case"` (or similar) on the command if you want snake_case in JSON.
- **Serialization**: All arguments and return types must implement `serde::Deserialize` / `serde::Serialize`. For binary data use `tauri::ipc::Response`.

## Passing Arguments and Returning Data

- Args: any type that implements `Deserialize`. Frontend passes camelCase object by default.
- Return: any type that implements `Serialize`. Frontend gets the JSON value.
- **Array buffers**: Return `tauri::ipc::Response::new(bytes)` to avoid JSON overhead for large binary data.

## Error Handling

Return `Result<T, E>`; frontend Promise rejects on `Err`. `E` must implement `Serialize`. Use `map_err(|e| e.to_string())` for std errors, or a custom error type that implements `Serialize` (e.g. with `thiserror` + manual `Serialize`) for structured errors.

## Async Commands

Use `async fn` for heavy work so the main thread doesn’t block. Async commands run on the async runtime. Avoid borrowed arguments in async commands (e.g. use `String` instead of `&str`, or return `Result<T, ()>` to satisfy lifetime constraints). Inject `State<'_, T>`, `AppHandle`, or `WebviewWindow` as needed.

## Dependency Injection

Commands can receive:

- **tauri::State\<T\>** – Managed state from `app.manage(T)`.
- **tauri::AppHandle** – App handle (paths, global shortcut, etc.).
- **tauri::WebviewWindow** – Window that sent the invoke.
- **tauri::ipc::Request** – Raw body and headers (e.g. for uploads with custom headers).

Frontend can pass raw body + headers: `invoke('upload', data, { headers: { Authorization: '...' } })`.

## Channels (Streaming)

For streaming (e.g. progress), take a `tauri::ipc::Channel<T>` argument. Frontend creates a `Channel` and sets `onmessage`; pass it in the invoke args. Rust calls `channel.send(item)` for each chunk. Prefer channels over many small events for high-throughput or ordered streams.

## Multiple Commands

Pass all commands in a single `generate_handler!` call: `tauri::generate_handler![cmd_a, cmd_b, commands::cmd_c]`. Only the last `invoke_handler` wins; don’t call it twice.

## Capabilities

Commands are **not** callable from the frontend until they are allowed by a **capability** attached to the window (see security-capabilities). Use `AppManifest::commands(&["your_command"])` in `build.rs` to restrict which commands exist; capabilities then control which window can call which of those.

## Key Points

- One `invoke_handler` per app; list all commands in one `generate_handler![]`.
- Prefer async commands for I/O or CPU-heavy work.
- Keep business logic and secrets in Rust; validate and sanitize all frontend input in the command.

<!--
Source references:
- https://v2.tauri.app/develop/calling-rust/
- https://github.com/tauri-apps/tauri-docs
-->
