---
name: tauri-concept-ipc-patterns
description: Tauri IPC patterns — Brownfield (default) vs Isolation (sandbox, encrypt). Use when choosing frontend–Core security model.
---

# IPC Patterns: Brownfield vs Isolation

Tauri supports two **IPC patterns** that control how frontend messages reach the Core: **Brownfield** (default) and **Isolation**. Both use the same Commands and Events API; the difference is whether an extra sandbox layer sits between the frontend and Core.

## Brownfield (Default)

- **No** extra layer. Frontend sends IPC messages directly to Tauri Core.
- Easiest to use and best for trusted frontends (e.g. your own code, few dependencies).
- Set explicitly in config: `app.security.pattern.use: "brownfield"` (optional; it’s the default).

## Isolation

- A **sandboxed** “Isolation” app (run in an iframe) intercepts **all** IPC messages from the frontend. It can validate or modify them, then encrypt the message (AES-GCM, runtime key) and pass it to the main page, which forwards it to Core. Core decrypts and processes as usual.
- **Purpose**: Reduce risk from untrusted or complex frontend code (e.g. many dependencies, supply chain). The Isolation app should stay minimal (few deps, simple build).
- **When**: Recommended when you use external Tauri APIs or many frontend dependencies; use the Isolation app to validate IPC (e.g. paths, URLs, headers) before they reach Core.
- **Config**: Build an Isolation app (HTML/JS), point config to it (e.g. `frontendDist` and isolation dist), and set `app.security.pattern.use: "isolation"`. See the [Isolation](/concept/inter-process-communication/isolation/) doc for the hook format (`window.__TAURI_ISOLATION_HOOK__`) and build steps.
- **Limitations**: On Windows, scripts in the iframe are inlined at build time; ES modules in the Isolation app are not supported. Slight overhead from encrypt/decrypt; usually negligible. In headless/WebDriver environments, ensure enough entropy for key generation (e.g. `haveged` on Linux).

## Key Points

- **Brownfield**: Direct IPC; use for trusted, simple frontends.
- **Isolation**: Intercept, validate, and encrypt in a small sandbox; use when you want to lock down IPC from a complex or less-trusted frontend. Keep the Isolation app small and auditable.

<!--
Source references:
- https://v2.tauri.app/concept/inter-process-communication/brownfield/
- https://v2.tauri.app/concept/inter-process-communication/isolation/
- https://github.com/tauri-apps/tauri-docs
-->
