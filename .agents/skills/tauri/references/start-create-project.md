---
name: tauri-start-create-project
description: Creating a Tauri project with create-tauri-app or manual tauri init. Use when scaffolding or adding Tauri to an existing frontend.
---

# Create a Tauri Project

Two main paths: **create-tauri-app** (scaffold new app) or **tauri init** (add Tauri to an existing frontend).

## create-tauri-app

Scaffolds a new project with a chosen frontend template (vanilla, Vue, Svelte, React, Solid, Angular, Preact, Yew, Leptos, Sycamore, Blazor). Prompts for name, identifier, language (Rust/TS/.NET), package manager, and UI template.

```bash
# Bash / PowerShell examples
npm create tauri-app@latest
pnpm create tauri-app@latest
```

After creation: install deps, then run dev:

```bash
cd <project-name>
pnpm install
pnpm tauri dev
```

## Manual Setup (tauri init)

When you already have a frontend (Vite, Next, Nuxt, etc.):

1. Create the frontend project and ensure it can be served (e.g. `pnpm run dev`).
2. Install the Tauri CLI in the project:
   ```bash
   pnpm add -D @tauri-apps/cli@latest
   ```
3. Run init:
   ```bash
   pnpm tauri init
   ```
   Prompts: app name, window title, **web assets path** (e.g. `..` if frontend is parent), **dev server URL** (e.g. `http://localhost:5173`), **dev command** (e.g. `pnpm run dev`), **build command** (e.g. `pnpm run build`).
4. This creates `src-tauri/` with `Cargo.toml`, `tauri.conf.json`, `build.rs`, `src/main.rs`, `src/lib.rs`, and `capabilities/default.json`.
5. Run:
   ```bash
   pnpm tauri dev
   ```

## Key Points

- **Dev server URL** in config must match your frontend dev server (e.g. Vite 5173). Tauri loads the app from that URL in dev.
- **Web assets path** is the directory containing built static files for production; often the frontend’s `dist` output.
- Commands: `tauri dev` (run app + dev server), `tauri build` (production build). Use `pnpm tauri` / `npm run tauri` etc. depending on package manager.

<!--
Source references:
- https://v2.tauri.app/start/create-project/
- https://github.com/tauri-apps/tauri-docs
-->
