# Gold Band Theme SDK

The Theme SDK is the development-time compiler and validator for declarative Gold Band theme packages.
It uses the DTCG token format, Style Dictionary for reference resolution, and JSON Schema/Ajv for the
closed runtime contract. Theme packages cannot contain JavaScript, HTML, arbitrary CSS, remote URLs,
or executable extensions.

Theme Contract v2 keeps asset identity separate from physical files. `resources.json` declares stable IDs,
types, package-relative paths, and license references; the compiler derives signatures, media metadata,
content hashes, and same-origin output URLs. Font faces and locale/script-aware stacks in `fonts.json` refer
to those IDs. A face's CSS `family` must retain the canonical family embedded in its font asset; a package may
add a namespace prefix or a `Variable`/`VF` marker, while unrelated aliases are rejected. Product-facing stack
labels belong in localized `displayName`. The compiler adds `runtimeFamily` only to generated packages: it is
the browser-global font-face identity and is scoped when another theme could otherwise compete for the same
family/weight/style key. Theme authors must not declare it. `presets.json` selects stacks for each scheme.

Recipe colors, materials, states, elevation, motion, border width, border style, and radius are emitted in the
CSS `components` cascade layer. They are the role defaults declared by the active theme; an application-level
Tailwind utility remains an explicit component or variant override. This preserves focus rings, one-sided
dividers, borderless surfaces, deliberate shadows, pills, circles, joined controls, and component-owned
transitions without preventing another theme from choosing different role defaults. Use `radius: "none"` with
`borderWidth: "none"` for structural surfaces that do not own a perimeter:

```json
{
  "faces": [
    {
      "id": "ui-regular",
      "family": "Example UI",
      "assetId": "example-ui-variable",
      "weight": 400,
      "style": "normal",
      "display": "swap",
      "coverage": { "scripts": ["Latn"] }
    }
  ],
  "stacks": [
    {
      "id": "theme-ui",
      "displayName": { "zh-CN": "Example UI", "en": "Example UI" },
      "defaultFaces": ["ui-regular"],
      "systemFallbacks": ["sans-serif"]
    }
  ]
}
```

The compiler validates font metadata with `fontkit`, raster dimensions with `image-size`, resource limits,
license references, capability/file consistency, path containment, symlinks, signatures, kinds, semantic
slots, and every cross-file reference. Unknown fields are rejected by the generated closed JSON Schemas.

## Authoring a package

Copy an existing directory under `themes/` and change only package-owned files:

- `manifest.json` declares stable identity, paired light/dark schemes, and capabilities.
- `tokens/*.tokens.json` contains DTCG primitives, semantic aliases, and scheme values.
- `recipes.json` maps stable component roles to the closed surface recipe vocabulary.
- `presets.json` selects typography stacks and avatar defaults for both schemes.
- `resources.json` declares every local asset and its license ID.
- `fonts.json`, `icons.json`, and `wallpapers.json` are present only when their matching capability is declared.
- `LICENSES.json` contains every license referenced by `resources.json`.
- `visual-quality/performance.json` is allowed only with the matching manifest capability.
- `assets/` may contain bounded PNG, WebP, WOFF, and WOFF2 resources; symbolic links are rejected.

Material tokens use a closed model vocabulary. `solid` is the backward-compatible default,
`frosted` enables blur/saturation without optical highlights, and `liquid` additionally projects
backdrop brightness/contrast, a specular highlight layer, and edge shadow. Performance profiles may
reduce only those bounded material effects; they cannot alter layout, typography, semantic color, or content.

Run `npm run themes:build`. The compiler validates and stages each package's `dist/runtime-theme.json`,
`dist/builtin-theme.css`, and `dist/asset-manifest.json`, plus shared Web/Rust catalogs and immutable
hash-named files under `web/public/theme-assets`. Each successful build synchronizes that directory to the complete
staged asset snapshot, then removes unreferenced hashes after catalog activation. Catalogs activate only after all resources are ready.
The desktop runtime consumes only those generated artifacts; Style Dictionary, Ajv, `fontkit`, and
`image-size` are not part of the theme-switching hot path.
