---
name: tauri-learn-window-customization
description: Tauri window customization — decorations, custom titlebar, data-tauri-drag-region, transparent titlebar (macOS). Use when building custom window chrome.
---

# Window Customization

Window appearance and behavior can be set in **tauri.conf.json** (e.g. `app.windows`), via the **JavaScript window API** (`@tauri-apps/api/window`), or in **Rust** (`WebviewWindowBuilder`, `Window`). Common use: **custom titlebar** (no native decorations) or **transparent titlebar** on macOS.

## Custom Titlebar (No Decorations)

1. **Config**: In `tauri.conf.json` set `decorations: false` for the window.
2. **Capabilities**: Grant window permissions, e.g. `core:window:default` and `core:window:allow-start-dragging` (for drag region).
3. **HTML**: Add a titlebar div; use **data-tauri-drag-region** on the draggable area so the user can move the window. Put buttons (minimize, maximize, close) outside the drag region or handle clicks explicitly.
4. **JS**: `getCurrentWindow()` then `minimize()`, `toggleMaximize()`, `close()`. For custom drag behavior (e.g. double-click to maximize), use `startDragging()` on mousedown instead of `data-tauri-drag-region`.
5. **CSS**: Position the titlebar (e.g. `position: fixed; top: 0; left: 0; right: 0`), reserve space so content isn’t covered. On Windows, `app-region: drag` on the drag region can improve touch/pen.

**Note**: `data-tauri-drag-region` applies only to the element it’s on; add it to each draggable child if needed. Buttons/inputs should not be inside a drag region so they remain clickable.

## Transparent Titlebar (macOS)

Keep the native titlebar but make it transparent and set a custom window background: create the window in Rust with **WebviewWindowBuilder**, set **TitleBarStyle::Transparent**, and use the **cocoa** crate to set the **NSWindow** background color. See the [window customization](/learn/window-customization/) doc for the full Rust snippet. On macOS, a full custom titlebar (decorations off) loses native move/align behaviors; transparent titlebar is an alternative.

## Key Points

- **core:window:allow-start-dragging** is required for the frontend to call `startDragging()` (or for `data-tauri-drag-region` to work). Other window permissions (close, minimize, toggleMaximize) are listed in the capabilities/window docs.
- Window options (size, position, resizable, etc.) are in the config reference under **WindowConfig**.

<!--
Source references:
- https://v2.tauri.app/learn/window-customization/
- https://github.com/tauri-apps/tauri-docs
-->
