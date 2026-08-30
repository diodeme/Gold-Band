---
name: tauri-learn-windows-menus-tray
description: Tauri window menus and system tray — Menu, Submenu, TrayIcon, predefined items, events. Use when adding native menus or tray icons.
---

# Window Menu and System Tray

Tauri supports **native window menus** and a **system tray** (desktop). Menus can be attached to the app or to a window; the tray shows an icon and optional menu.

## System Tray

- **Cargo**: Enable `tauri` feature **tray-icon**.
- **Rust**: `use tauri::tray::TrayIconBuilder;` then `TrayIconBuilder::new().menu(&menu).menu_on_left_click(true).build(app)?`. Use `on_menu_event` and `on_tray_icon_event` for click/menu handling.
- **JS**: `import { TrayIcon } from '@tauri-apps/api/tray';` then `await TrayIcon.new({ menu, menuOnLeftClick: true, action: (event) => { ... } })`. Use `defaultWindowIcon()` for the tray icon.
- **Events**: Click, DoubleClick, Enter, Move, Leave (Linux: tray events not fully supported). Menu items trigger menu events; handle in `on_menu_event` (Rust) or per-item `action` (JS).

## Window / App Menu

- **Rust**: `use tauri::menu::MenuBuilder;` — `MenuBuilder::new(app).text("id", "Label").check("id", "Label").separator().build()?` then `app.set_menu(menu)?`. Listen with `app.on_menu_event(|app, event| { ... })`. Use **SubmenuBuilder** for submenus; **PredefinedMenuItem** for Copy, Paste, Undo, etc.
- **JS**: `import { Menu, Submenu, MenuItem, PredefinedMenuItem } from '@tauri-apps/api/menu';` — `Menu.new({ items: [...] })` then `menu.setAsAppMenu()`. Items: `{ id, text, action }`, `{ id, text, checked }`, `{ type: 'Separator' }`, `{ id, text, enabled: false }`. Use **Submenu.new({ text, items })** for submenus; **PredefinedMenuItem.new({ item: 'Copy' })** etc. for built-in items.
- **macOS**: On macOS, top-level items are often under submenus; first submenu can appear under the app name. Use an “About” submenu first if needed.
- **Updates**: Get item by id and call `setText`, `setChecked`, `setIcon` (Rust: `menu.get("id").unwrap().as_menuitem_unchecked().set_text(...)`).

## Key Points

- Menus and tray are desktop-only. Use **core:menu:default** and **core:tray:default** (or specific permissions) in capabilities if the frontend creates or updates menus/tray.
- Tray: use **menu_on_left_click(false)** (Rust) or **menuOnLeftClick: false** (JS) to show menu only on right-click. Unlisten or drop handlers when no longer needed.

<!--
Source references:
- https://v2.tauri.app/learn/system-tray/
- https://v2.tauri.app/learn/window-menu/
- https://github.com/tauri-apps/tauri-docs
-->
