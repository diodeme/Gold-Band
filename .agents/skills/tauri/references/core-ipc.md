---
name: tauri-core-ipc
description: Tauri IPC primitives — Events (fire-and-forget) and Commands (invoke with request/response). Use when wiring frontend to Rust or implementing secure message passing.
---

# Inter-Process Communication (IPC)

Tauri uses **asynchronous message passing**: requests and responses are serialized (e.g. JSON). The Core process can reject or ignore requests, which keeps the model safer than shared memory or direct FFI.

## Events

- **Fire-and-forget**, one-way.
- Can be emitted by **both** the frontend and the Core.
- Good for: lifecycle, state changes, notifications.
- No return value; payload is JSON.

Frontend: `emit` / `emitTo` from `@tauri-apps/api/event` or `getCurrentWebviewWindow().emit(...)`.  
Rust: `AppHandle::emit`, `Emitter::emit_to`, `Emitter::emit_filter` on windows.

## Commands

- **Request–response**: frontend calls Rust and gets a value back.
- Only the **frontend** invokes; Rust implements handlers.
- Similar to the browser `fetch` API: `invoke('command_name', { args })` returns a Promise.
- Under the hood: JSON-RPC–style serialization; arguments and return types must be serializable (e.g. `serde`).

Flow: Frontend → IPC request → Core → invoke handler → serialize return → response to frontend.

## When to Use Which

- **Commands**: Type-safe API from frontend to Rust (CRUD, file access, system calls). Use for anything that needs a return value or structured errors.
- **Events**: Decouple components, broadcast state, progress, or one-off signals. Use when you don’t need a response.
- **Channels**: For streaming or high-throughput data (e.g. download progress, process stdout); prefer over many small events when pushing lots of data.

## Key Points

- All command arguments and return values must be JSON-serializable (or use `tauri::ipc::Response` for binary).
- Commands are registered with `invoke_handler(tauri::generate_handler![...])` and must be allowed in **capabilities** to be callable from the frontend.
- Events are not capability-gated; use them for internal coordination, not as the primary security boundary.

<!--
Source references:
- https://v2.tauri.app/concept/inter-process-communication/
- https://github.com/tauri-apps/tauri-docs
-->
