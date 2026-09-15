---
name: tauri-develop-icons
description: Tauri app icons — tauri icon command, bundle.icon config, platform formats (icns, ico, png). Use when generating or configuring app and tray icons.
---

# App Icons

Tauri ships a default iconset; for production you should replace it. Use the **`tauri icon`** command to generate platform icons from a single source image, or configure `bundle.icon` manually.

## Command

```bash
pnpm tauri icon [INPUT] [OPTIONS]
# INPUT: path to source (default ./app-icon.png) — squared PNG or SVG with transparency
# -o, --output <OUTPUT>  default: icons dir next to tauri.conf.json
# -p, --png <PNG>        custom PNG sizes; when set, default PNGs are not generated
# --ios-color <COLOR>    iOS icon background [default: #fff]
```

Output goes to **src-tauri/icons/** by default. Desktop icons are picked up automatically; mobile icons are written into Xcode/Android Studio projects.

## Config (bundle.icon)

To use a custom path or subset of files:

```json
{
  "bundle": {
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

## Platform Formats

- **macOS**: `icon.icns` (see Tauri repo for required layer sizes).
- **Windows**: `icon.ico` — layers for 16, 24, 32, 48, 64, 256 px; 32px first for dev display.
- **Linux**: PNG — width === height, RGBA, 32bpp; common sizes 32, 128, 256, 512; recommend at least `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`.
- **Android**: PNG in `src-tauri/gen/android/app/src/main/res/mipmap-*` (see docs for per-density sizes).
- **iOS**: PNG in `Assets.xcassets/AppIcon.appiconset/`; no transparency; multiple sizes (20–512px, 1x/2x/3x).

## Key Points

- Use `tauri icon` for one-shot generation; override paths with `bundle.icon` if needed.
- Source image should be square, transparent (except iOS). Manually built icons must match platform requirements in the docs.

<!--
Source references:
- https://v2.tauri.app/develop/icons/
- https://github.com/tauri-apps/tauri-docs
-->
