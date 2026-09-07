---
name: tauri-learn-splashscreen
description: Splashscreen in Tauri — extra window, visible/hidden main, setup tasks, close splash when ready. Use when implementing a loading/splash window.
---

# Splashscreen

A **splashscreen** is a separate window shown while the app does heavy setup, then closed when ready. Implement by: (1) defining two windows in config — main (hidden) and splash (visible), (2) running setup (frontend and/or backend), (3) when both are done, closing the splash and showing the main window.

## Config

In **tauri.conf.json** (or equivalent), define:

- A window with **label: "main"** and **visible: false**.
- A window with **label: "splashscreen"** and **url: "/splashscreen"** (or the route that serves your splash UI).

Ensure the frontend can serve the splash URL (e.g. a `/splashscreen` route or a separate HTML file).

## Setup and Completion

- **Frontend**: Run any heavy init (e.g. load config, auth). When done, call a Tauri command e.g. `invoke('set_complete', { task: 'frontend' })`.
- **Backend**: In **setup**, spawn an async task that does heavy work (use `tokio::time::sleep` or real I/O; avoid `std::thread::sleep` in async). When done, call the same completion command (e.g. from Rust you can call the command function directly with `AppHandle` and `State`).
- **State**: Use **manage(Mutex::new(SetupState { frontend_task: false, backend_task: false }))** and a command **set_complete** that sets the corresponding flag. When **both** flags are true, get the splash and main windows from **AppHandle**, call **splash.close()** and **main.show()**.

## Key Points

- Use **tauri::async_runtime::spawn** in setup so the main loop and windows can start while the task runs. Don’t block the main thread with long work.
- Splashscreens are optional; often a single main window with a small loading indicator is enough. Use a splash when you must block showing the main UI until setup finishes.

<!--
Source references:
- https://v2.tauri.app/learn/splashscreen/
- https://github.com/tauri-apps/tauri-docs
-->
