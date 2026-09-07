---
name: tauri-develop-resources
description: Tauri bundled resources — bundle.resources, resolve path (Rust/JS), $RESOURCE, fs plugin permissions. Use when embedding extra files in the app bundle.
---

# Embedding Additional Files (Resources)

Resources are extra files bundled with the app (not part of `frontendDist`). Configure them in **tauri.conf.json > bundle > resources** and resolve paths at runtime.

## Configuration

**List form** — paths are copied preserving structure; `..` becomes `_up_`, absolute root becomes `_root_`:

```json
{
  "bundle": {
    "resources": [
      "./path/to/some-file.txt",
      "some-folder/",
      "resources/**/*.md"
    ]
  }
}
```

**Map form** — source path → destination under `$RESOURCE`:

```json
{
  "bundle": {
    "resources": {
      "/absolute/path/to/file.txt": "resources/file.txt",
      "relative/path/": "",
      "docs/**/*.md": "website-docs/"
    }
  }
}
```

Glob rules: `dir/` copies directory recursively; `dir/**/*` copies all files recursively; `dir/**` is invalid (no files).

## Resolving Paths

- **Rust**: `app.path().resolve("path/within/resources", BaseDirectory::Resource)?` or `handle.path().resolve(...)` in a command. Returns filesystem path (or Android asset URI).
- **JavaScript**: `import { resolveResource } from '@tauri-apps/api/path';` then `await resolveResource('path/within/resources')`. Use with **fs plugin** to read; grant `fs:allow-resource-read-recursive` or scoped permissions (e.g. `$RESOURCE/**/*`) in capabilities.

On **Android**, resources live in the APK as assets; resolve returns a special URI (`asset://localhost/...`). Read via the fs plugin (`app.fs().read_to_string(&resource_path)` in Rust).

## Permissions

To allow the frontend to read resources via the fs plugin, add to the capability: `fs:allow-read-text-file` (or equivalent) and a scope like `fs:allow-resource-read-recursive` or a custom scope `{ "allow": ["$RESOURCE/**/*"] }`. For opener plugin, use `opener:allow-open-path` with `path: "$RESOURCE/**/*"`.

## Key Points

- Paths in config are relative to the project (or absolute); resolved paths are under `$RESOURCE` (platform-specific location).
- Use **PathResolver::resolve** / **resolveResource**; don’t hardcode resource paths. For scoped fs access, use `$RESOURCE/**/*` to allow recursive access when list form uses `..` or `/`.

<!--
Source references:
- https://v2.tauri.app/develop/resources/
- https://github.com/tauri-apps/tauri-docs
-->
