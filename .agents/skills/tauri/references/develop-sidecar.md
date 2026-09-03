---
name: tauri-develop-sidecar
description: Tauri sidecar binaries — externalBin, shell plugin, spawn from Rust/JS, args and permissions. Use when bundling and running external executables.
---

# Embedding External Binaries (Sidecar)

A **sidecar** is an external executable bundled with the app (e.g. a Python CLI or Node tool). Configure in **tauri.conf.json > bundle > externalBin** and run via the **shell** plugin.

## Configuration

```json
{
  "bundle": {
    "externalBin": [
      "/absolute/path/to/sidecar",
      "../relative/path/to/binary",
      "binaries/my-sidecar"
    ]
  }
}
```

Paths are relative to **tauri.conf.json** (usually `src-tauri/`). For each supported platform you need a binary named **&lt;name&gt;-&lt;TARGET_TRIPLE&gt;** (e.g. `my-sidecar-x86_64-unknown-linux-gnu`, `my-sidecar-aarch64-apple-darwin`). Get the host triple: `rustc --print host-tuple`.

## Running from Rust

Requires **tauri-plugin-shell** (setup and capabilities as per shell plugin docs). Use the **ShellExt** trait:

```rust
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandEvent;

let sidecar_command = app.shell().sidecar("my-sidecar").unwrap();
let (mut rx, mut child) = sidecar_command.spawn().expect("Failed to spawn sidecar");

tauri::async_runtime::spawn(async move {
  while let Some(event) = rx.recv().await {
    if let CommandEvent::Stdout(line_bytes) = event {
      let line = String::from_utf8_lossy(&line_bytes);
      app.emit("message", Some(format!("'{}'", line))).ok();
      child.write("message from Rust\n".as_bytes()).ok();
    }
  }
});
```

**sidecar(name)** takes the **base name** from the config (e.g. `"my-sidecar"` for `binaries/my-sidecar`), not the full path.

## Running from JavaScript

Grant permission in a capability (e.g. **default.json**). Use **shell:allow-execute** or **shell:allow-spawn** with a scope that allows the sidecar by name and (optionally) args:

```json
{
  "permissions": [
    {
      "identifier": "shell:allow-execute",
      "allow": [
        {
          "name": "binaries/my-sidecar",
          "sidecar": true,
          "args": ["arg1", "-a", "--arg2", { "validator": "\\S+" }]
        }
      ]
    }
  ]
}
```

Then in JS:

```javascript
import { Command } from '@tauri-apps/plugin-shell';
const command = Command.sidecar('binaries/my-sidecar', ['arg1', '-a', '--arg2', 'value']);
const output = await command.execute();
```

The string passed to `Command.sidecar` must match one of the entries in **externalBin** (e.g. `binaries/my-sidecar`). Args must match the capability (static args as-is; dynamic with regex validator).

## Key Points

- One binary per target triple: **&lt;config-name&gt;-&lt;TARGET_TRIPLE&gt;** (plus `.exe` on Windows).
- **sidecar()** in Rust and **Command.sidecar()** in JS use the **config name** (path string in externalBin), not the filesystem path. Args in JS must be in the same order as in the capability.
- Migrating from Tauri v1: use `tauri migrate` to update sidecar permission shape.

<!--
Source references:
- https://v2.tauri.app/develop/sidecar/
- https://github.com/tauri-apps/tauri-docs
-->
