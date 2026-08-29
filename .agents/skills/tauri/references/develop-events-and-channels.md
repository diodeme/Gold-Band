---
name: tauri-develop-events-and-channels
description: Tauri events (emit, listen, global vs webview-specific) and channels for streaming. Use when pushing data from Rust to frontend or coordinating between windows.
---

# Calling the Frontend from Rust (Events and Channels)

Rust can push data to the frontend via **events** (fire-and-forget, JSON) or **channels** (streaming, ordered). Use events for lifecycle and small payloads; use channels for progress or high-throughput streams.

## Event System

- **Global events**: Delivered to **all** listeners. Emit: `app.emit("event-name", &payload)`. Frontend: `listen('event-name', (event) => { ... })`.
- **Webview-specific events**: Delivered only to listeners in a given webview. Emit: `app.emit_to("window-label", "event-name", &payload)` or `Emitter::emit_filter` for multiple targets. Frontend: `getCurrentWebviewWindow().listen(...)` or `listen(..., { target: { kind: 'Any' } })` to receive webview-specific events from any window.

Payload: any type that is `Clone + Serialize`. Frontend receives the JSON in `event.payload`.

## Emitting from Rust

```rust
use tauri::{AppHandle, Emitter};

app.emit("download-started", &url)?;
app.emit_to("settings", "settings-update", &data)?;
app.emit_filter("open-file", path, |target| matches!(target, EventTarget::WebviewWindow { label } if *label == "main"))?;
```

## Listening on the Frontend

```ts
import { listen, once } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

const unlisten = await listen<PayloadType>('event-name', (event) => { ... });
unlisten(); // call when component unmounts or scope ends

once('ready', (event) => { ... });
getCurrentWebviewWindow().listen('logged-in', (event) => { ... });
```

Always call `unlisten()` when the listener is no longer needed (e.g. component unmount). Use `once` for one-off events.

## Listening on Rust

- Global: `app.listen("event-name", |event| { ... })`. Returns an id; call `app.unlisten(id)` to remove.
- Webview: `app.get_webview_window("main")?.listen("event-name", |event| { ... })`.
- One-off: `app.once("event-name", |event| { ... })`.

## Channels (Streaming)

Use when you need **ordered, high-throughput** data (e.g. download progress, process stdout). In the command, take a `tauri::ipc::Channel<T>` argument; frontend creates a `Channel<T>` and passes it in the invoke payload. Rust calls `channel.send(item)` for each chunk. Frontend sets `channel.onmessage = (msg) => { ... }`. Prefer channels over many small events when sending lots of data.

## Evaluating JavaScript

For one-off scripts: `webview.eval("console.log('hello')")`. For complex data use a crate like `serialize-to-javascript` to pass Rust values. Prefer events or channels for structured communication.

## Key Points

- Events are **not** capability-gated; use them for internal app coordination, not as the primary security boundary.
- Global events go to all listeners; webview-specific events only to that window. Use `listen(..., { target: { kind: 'Any' } })` to hear webview-specific events from any window.
- Unregister listeners when they go out of scope to avoid leaks and stale handlers.

<!--
Source references:
- https://v2.tauri.app/develop/calling-frontend/
- https://github.com/tauri-apps/tauri-docs
-->
