---
name: tauri-security-http-headers
description: HTTP headers for WebView responses — CORS, COOP, Permissions-Policy, config. Use when locking down webview response headers.
---

# HTTP Headers

Tauri can send **custom HTTP headers** with responses to the WebView. Headers are configured in the Tauri config and apply to every response sent via the protocol’s **get_response** path (not to raw IPC messages or error responses).

## Allowed header names

Only these names are accepted:

- **Access-Control-Allow-Credentials**, **Access-Control-Allow-Headers**, **Access-Control-Allow-Methods**, **Access-Control-Expose-Headers**, **Access-Control-Max-Age**
- **Cross-Origin-Embedder-Policy**, **Cross-Origin-Opener-Policy**, **Cross-Origin-Resource-Policy**
- **Permissions-Policy**
- **Service-Worker-Allowed**, **Timing-Allow-Origin**
- **X-Content-Type-Options**
- **Tauri-Custom-Header** (not for production)

**Content-Security-Policy** is configured separately (see [CSP](/security/csp/)).

## Value format

- **string**: used as-is for the header value.
- **array**: items joined by `, `.
- **key-value**: items as `key + " " + value`, joined by `; `.
- **null**: header is omitted.

See the [HTTP Headers](/security/http-headers/) doc for the exact config shape and examples.

## Key Points

- Use for CORS, COOP/CORP, or Permissions-Policy when loading app or asset URLs in the WebView. Headers are applied by the Tauri protocol layer for qualifying responses.
- CSP is configured under **app.csp** (or equivalent), not in this headers list.

<!--
Source references:
- https://v2.tauri.app/security/http-headers/
- https://github.com/tauri-apps/tauri-docs
-->
