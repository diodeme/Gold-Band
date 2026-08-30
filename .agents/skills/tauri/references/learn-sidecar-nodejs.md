---
name: tauri-learn-sidecar-nodejs
description: Node.js as a Tauri sidecar — pkg/yao-pkg, externalBin, shell plugin, argv/stdout. Use when bundling a Node app as a sidecar binary.
---

# Node.js as a Sidecar

You can package a **Node.js** app as a **single binary** and run it as a Tauri **sidecar**, so users don’t need Node installed. Use a tool like **pkg** (e.g. [@yao-pkg/pkg](https://github.com/yao-pkg/pkg)) to compile JS/TS to a binary; then add it to **externalBin** and run it via the **shell** plugin.

## Flow

1. **Separate Node project** (e.g. `sidecar-app/`) with your Node entry (e.g. `index.js` that reads `process.argv`, writes to stdout).
2. **Build binary** with pkg (or similar) for each target triple (e.g. `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`). Name output **&lt;name&gt;-&lt;TARGET_TRIPLE&gt;** (plus `.exe` on Windows).
3. **Configure sidecar**: In **tauri.conf.json > bundle > externalBin**, add paths to those binaries (e.g. `sidecar-app/node-sidecar`).
4. **Capabilities**: Grant **shell:allow-execute** or **shell:allow-spawn** with a scope that allows the sidecar by name and (optionally) args.
5. **Run**: From Rust use **ShellExt::sidecar("node-sidecar")** and spawn; from JS use **Command.sidecar("sidecar-app/node-sidecar", args)**.

## Communication

- **Short-lived**: Pass command and args via **process.argv**; read result from **stdout**. Good for one-shot commands.
- **Long-lived**: Use a localhost server, stdin/stdout streams, or local sockets. Each has trade-offs and security implications; document what the sidecar is allowed to do.

## Alternatives

- **Embed Node runtime** and ship JS as **resources**: Simpler in some setups but ships readable JS and usually a larger runtime than a pkg-style binary.
- Prefer the [general sidecar guide](/develop/sidecar/) for binary layout, permissions, and Rust/JS usage.

## Key Points

- Use **rustc --print host-tuple** (or parse `rustc -Vv`) to get the target triple for the current platform. Build one binary per triple you support.
- The string passed to **sidecar()** / **Command.sidecar()** must match an entry in **externalBin** (e.g. the path you used in config). Args must match the capability scope.

<!--
Source references:
- https://v2.tauri.app/learn/sidecar-nodejs/
- https://github.com/tauri-apps/tauri-docs
-->
