---
name: tauri-security-csp
description: Tauri Content Security Policy — csp config, script-src, style-src, nonces and hashes. Use when locking down WebView loading and scripts.
---

# Content Security Policy (CSP)

Tauri can set a **Content Security Policy** on the WebView to reduce XSS and unauthorized resource loading. CSP is applied only when configured in **tauri.conf.json**.

## Configuration

Configure **app.csp** (or equivalent in your config schema). Tauri appends **nonces and hashes** at compile time for bundled scripts/styles; you only need to allow what’s specific to your app.

Example (from Tauri api example):

```json
{
  "csp": {
    "default-src": "'self' customprotocol: asset:",
    "connect-src": "ipc: http://ipc.localhost",
    "font-src": ["https://fonts.gstatic.com"],
    "img-src": "'self' asset: http://asset.localhost blob: data:",
    "style-src": "'unsafe-inline' 'self' https://fonts.googleapis.com"
  }
}
```

- **default-src**: Fallback for other directives. Restrict to `'self'`, Tauri protocols (`asset:`, custom protocol if used), and trusted hosts.
- **connect-src**: Allow IPC and API origins (e.g. `ipc:`, dev server).
- **script-src** / **style-src**: Tauri injects nonces/hashes for bundle; add `'wasm-unsafe-eval'` in **script-src** if using Rust/WASM frontend.

## Best Practices

- Make CSP as **restrictive** as possible; only allow hosts you trust (ideally own).
- **Avoid** loading remote scripts (e.g. CDN) when possible; they add attack surface.
- Use **script-src** and **style-src** (and related directives) per [MDN CSP](https://developer.mozilla.org/en-US/docs/Web/HTTP/CSP). See Tauri docs for **CSP Sources** and directive details.

## Key Points

- CSP is **opt-in** via config. Without it, the WebView has no CSP.
- Tauri manages nonces/hashes for bundled content; you configure policy for your app’s own sources and third-party origins.

<!--
Source references:
- https://v2.tauri.app/security/csp/
- https://github.com/tauri-apps/tauri-docs
-->
