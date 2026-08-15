# Gold Band Theme SDK

The Theme SDK is the development-time compiler and validator for declarative Gold Band theme packages.
It uses the DTCG token format, Style Dictionary for reference resolution, and JSON Schema/Ajv for the
closed runtime contract. Theme packages cannot contain JavaScript, HTML, arbitrary CSS, remote URLs,
or executable extensions.

Theme Contract v2 represents typography as ordered font data instead of a pre-serialized CSS string:

```json
{
  "typography": {
    "ui": {
      "families": ["Inter Variable", "Gold Band MiSans"],
      "fallback": "sans-serif",
      "size": 14
    },
    "editor": {
      "families": ["JetBrains Mono", "SFMono-Regular", "Consolas"],
      "fallback": "monospace",
      "size": 12
    }
  }
}
```

Family order is significant. Each stack contains 1–16 unique names; a family is at most 128 characters
and cannot contain CSS list or block delimiters (`,`, `;`, `{`, `}`). The compiler owns quoting and
serialization, so packages must not embed generic fallbacks or comma-separated CSS in `families`.

## Authoring a package

Copy an existing directory under `themes/` and change only package-owned files:

- `manifest.json` declares stable identity, paired light/dark schemes, and capabilities.
- `tokens/*.tokens.json` contains DTCG primitives, semantic aliases, and scheme values.
- `recipes.json` maps stable component roles to the closed surface recipe vocabulary.
- `presets.json` declares typography and avatar defaults for both schemes.
- `visual-quality/performance.json` is allowed only with the matching manifest capability.
- `assets/` may contain bounded PNG, WebP, WOFF, and WOFF2 resources; symbolic links are rejected.

Material tokens use a closed model vocabulary. `solid` is the backward-compatible default,
`frosted` enables blur/saturation without optical highlights, and `liquid` additionally projects
backdrop brightness/contrast, a specular highlight layer, and edge shadow. Performance profiles may
reduce only those bounded material effects; they cannot alter layout, typography, semantic color, or content.

Run `npm run themes:build`. The compiler validates and emits each package's `dist/runtime-theme.json`,
`dist/builtin-theme.css`, and `dist/asset-manifest.json`, plus the shared web and Rust catalogs. The
desktop runtime consumes only those generated artifacts; Style Dictionary and Ajv are not part of the
theme-switching hot path.
