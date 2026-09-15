---
name: tauri-start-frontend
description: Tauri frontend integration — static host model, SSG/SPA/MPA, Vite/Next/Nuxt/SvelteKit/Leptos/Trunk. Use when integrating a frontend framework with Tauri.
---

# Frontend Configuration

Tauri acts as a **static host**: you provide a folder of HTML, CSS, JS (and optionally WASM) that the WebView loads. No built-in SSR; use SSG, SPA, or MPA.

## Checklist

- Use **static site generation (SSG)**, **single-page app (SPA)**, or **multi-page app (MPA)**. Tauri does not run a server for SSR.
- **Mobile**: A dev server must serve the frontend on an IP reachable by the device (e.g. LAN URL in config).
- Keep a clear **client–server** boundary between the app and any backend API (no hybrid SSR in the Tauri process).

## Recommended Stacks

- **JavaScript/TypeScript**: [Vite](https://vitejs.dev/) is recommended for React, Vue, Svelte, Solid, or plain TS/JS. Meta-frameworks (Next, Nuxt, SvelteKit, Qwik) often target SSR; they need extra config to output a static build and point Tauri at it.
- **Rust frontend**: [Leptos](https://github.com/leptos-rs/leptos), [Trunk](https://trunkrs.dev/) for WASM. Include `'wasm-unsafe-eval'` in CSP **script-src** if using WASM.

## Framework Guides

Docs include setup for:

- **JS**: Vite (recommended), Next.js, Nuxt, Qwik, SvelteKit.
- **Rust**: Leptos, Trunk.

If a framework isn’t listed, it may work with no extra config or may not be documented yet. Ensure the app builds to static assets and set **build.frontendDist** (or equivalent) and **build.devUrl** / **build.beforeDevCommand** in **tauri.conf.json** to match your dev and build outputs.

## Key Points

- **devUrl** must match your dev server (e.g. `http://localhost:5173` for Vite). **beforeBuildCommand** should produce the static output that **frontendDist** (or default path) points to.
- For meta-frameworks, disable SSR and use static export where possible; then point Tauri at the exported directory.

<!--
Source references:
- https://v2.tauri.app/start/frontend/
- https://github.com/tauri-apps/tauri-docs
-->
