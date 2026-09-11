# Website and Browser Demo

The website navigation opens `/zh/demo` or `/en/demo` without replacing the outer document. These routes embed the independently built `/demo/` app below the persistent header; CSS, theme and portals remain isolated. Build both entries with `npm run website:build`. In development start both `site:dev` and `demo:dev`; `VITE_DEMO_URL` can specify a separately hosted Demo. Hash navigation is synchronized using source/origin-validated messages. Leaving the route unmounts the frame. Theme preferences remain independent between the website and Demo.

Message reference inventory: run `node scripts/audit-demo-message-references.mjs <archive-directory> <report.json>` and `node --test scripts/audit-demo-message-references.test.mjs`. The offline audit verifies consumed details/pages against manifest bytes and SHA-256, preserves full session/branch/event identity, and uses remark/parse5 to collect links, reference links, inline code and HTML resource attributes. Inline code and compound srcset values are review candidates, not automatically resolved file dependencies. Fenced code, tool payloads and directory document bodies are outside this report. Keep reports as local review artifacts; this command does not modify or publish the archive.

The site, recording source, and read-only browser Demo are separate entry points. The Demo imports the desktop frontend and replaces execution APIs at build time. The site loads only the current rrweb recording.

## Development

Run commands from the repository root with Node.js and installed npm dependencies:

```sh
npm run site:dev
npm run demo:dev
npm run site:record:dev
```

Defaults: site `127.0.0.1:1440`, Demo `127.0.0.1:1450`, recorder `127.0.0.1:1460`. Use another port when occupied. Recording requires the agent-browser CLI and its Chromium installation. On Windows, set `AGENT_BROWSER_BIN` to the actual agent-browser `.exe`, rather than the npm PowerShell shim.

```sh
npm run site:record
npm run site:test
```

`SITE_URL` selects the recorder origin; `SITE_SCENES` selects comma-separated scene IDs. `SITE_THEMES` selects `dark`, `light`, or `dark,light` (default `dark`). Light assets use the `-light` suffix. Both themes must have the same semantic checkpoint IDs and order; `site:test` validates the pairs and step-relative time mapping. Per-theme recording reports are written under `.codex-temp/site-recording/`. Recordings are versioned JSON and PNG files under `marketing/site/media`. Each recording is limited to 60 seconds, 12 MiB and 12,000 events. These limits do not apply to the complete session archive.

## Build and Deploy

After recording, run `node scripts/generate-site-checkpoints.mjs` against the site dev server (`SITE_URL`, default `http://127.0.0.1:1463`). It opens the development-only `checkpoints.html`, seeks each semantic step with rrweb, and writes static PNGs and camera metadata under `marketing/site/media/checkpoints/`. `SITE_SCENES` and `SITE_THEMES` can limit regeneration; the default covers both themes. Rebuild after generation. Only the selected step image is requested during playback loading or retry; the capture page and scripts are not production entries. `node scripts/verify-site-theme.mjs` verifies theme changes preserve focus, scroll, chapter, semantic step and pause intent, including narrow/wide restoration.

```sh
npm run website:build
npm run site:preview
```

The combined static output is `.codex-temp/site-dist/`: website at `/`, full Demo at `/demo/`. The composition step preserves independent HTML entries and merges fingerprinted assets, rejecting collisions. Both builds use the same `web/public` resources. Never include the recording server or `preview.html` in deployment.

### Hosting under a subpath

Asset URLs, page links and the embedded Demo are root-absolute, so a subpath deployment needs a build that carries the prefix:

```sh
npm run website:build:subpath                                  # /site-by-codex/ -> .codex-temp/site-by-codex-dist
WEBSITE_BASE=/docs/ WEBSITE_SITE_DIR=.codex-temp/site-docs npm run website:build:subpath
```

Upload the resulting directory and point the server at it. No rewrite rules are needed: the build writes a real directory index for every shell route (`zh/`, `en/`, `documentation/`, `zh|en/demo/`) in addition to `_redirects`, which only Netlify-style hosts read.

```nginx
location = /site-by-codex { return 301 /site-by-codex/; }

location /site-by-codex/ {
    alias /data/app/gold-band-site/;
    index index.html;

    location ~ ^/site-by-codex/(?<asset>assets/.+)$ {
        alias /data/app/gold-band-site/$asset;
        expires 1y;
        add_header Cache-Control "public, max-age=31536000, immutable";
    }
}
```

`add_header` inside a location replaces inherited `add_header` directives from the enclosing server block, so repeat any security headers you rely on there. `gzip`/`gzip_vary` settings are inherited normally.

With the combined production preview running, `npm run site:verify` checks five widths, both languages, four playback lifecycles, downloads, documentation and full Demo links. Set `SITE_URL` when using another port. It writes screenshots and a technical report to `.codex-temp/site-verification/`; the report explicitly keeps scene/reference visual acceptance pending.

`node scripts/verify-site-scroll.mjs` checks desktop sticky positioning and natural mobile document scrolling at five widths in both languages/themes. Each case loads independently, waits for deep-link scrolling to settle, pauses the active replay, then verifies that scrolling preserves its renderer, camera and content. Measurements and screenshots are written to `.codex-temp/site-scroll-verification/`. Set `SITE_URL` and `AGENT_BROWSER_BIN` as above. `node --test scripts/verify-site-scroll.test.mjs` verifies rejection of clock advancement, remounts, overlap and incorrect positioning. This is layout verification, not complete scene readability or motion acceptance.

`node scripts/verify-site-control-layout.mjs` checks that play/pause controls occupy the reserved space below the recording, retain a44px hit target, and never overlap or resize the1440/880 media surface. It covers five widths, both languages/themes, and autoplay/reduced-motion poster entry (40 cases). Set `SITE_URL` for the production host and optionally `CONTROL_REPORT_DIR` to retain a separate baseline; reports and screenshots default to `.codex-temp/site-control-layout/`.

`node scripts/verify-site-scenes.mjs` captures stable semantic checkpoints throughout actual playback, for both languages at 1440/390/320px. `SITE_SCENES` and `SITE_WIDTHS` can narrow an inspection. It checks nonblank playback, image decoding and overflow, saving evidence to `.codex-temp/site-scene-verification/`. Review the images before accepting framing; automatic checks do not establish visual quality. The personalization recorder selects theme, bundled font and a seeded recent avatar through client controls, validates their conversation effects and actual viewport transitions, and restores the starting preferences before saving.

Scene verification defaults to both themes; `SITE_THEMES` can limit it. `node scripts/verify-site-reference.mjs` captures fixed-viewport Pi/Gold Band first-screen pairs in both themes and site languages. The current evidence, verified scope and outstanding visual findings are recorded in `marketing/ACCEPTANCE.md`.

`npm run site:verify:motion` measures uninterrupted production playback through all four scenes and a loop restart. Configure `SITE_URL`, `AGENT_BROWSER_BIN`, `SITE_LANGUAGE` (zh/en), `SITE_THEME` (dark/light), `SITE_WIDTH` and optional `SITE_MOTION_OUTPUT`. Each report pins source recording hashes and retains ordered100ms DOM samples, RAF intervals and Long Tasks after the player becomes ready. Run measurements sequentially without other browser recordings or CPU-intensive builds. This measures local steady playback, not cold initialization, throttled devices or subjective motion quality. `npm run site:motion:test` checks that incomplete or nonmoving playback cannot pass the verifier.

`npm run site:verify:motion-preference` checks native runtime reduced-motion changes in both languages at1280/390px, unchanged paused content, explicit resume and poster-only entry with no recording request. It uses the same browser/origin variables and writes screenshots plus a report under `.codex-temp/site-motion-preference/`. Media query changes must drive a local snapshot; polling the native `matches` getter from RAF can suppress change delivery in Chrome and is covered by the replay DOM regression.

Reference capture waits for visible finite entrance animations to finish, without waiting for infinite decoration. With the local checkpoint server running on1463, `node scripts/verify-site-skill.mjs` checks all four setup recordings contain a mobile close-up of the full Skill source/synchronization icon group at readable scale. Production scene verification applies the same visible-icon boundary check to `skill-sync`; a nonblank recording alone does not establish this requirement. Regenerate setup checkpoint images after changing this storyboard.

With the same checkpoint server, `node scripts/verify-site-output.mjs` checks output schema and success-expression fields, including labels, against their recorded camera bounds in both languages/themes. Desktop shots retain both fields; mobile uses sequential close-ups. Production scene verification checks actual screen containment and readable scale for the same steps. Re-record all four workflow assets and regenerate their checkpoints after changing this storyboard.

`node scripts/verify-site-interactions.mjs` checks the complete question and permission controls against both camera tracks in all four workflow recordings, rejecting dialog occlusion and mobile scale below0.8. Set `SITE_URL` to the checkpoint server (default1460). The workflow storyboard uses real360px viewport recording for question through approval, closes the workflow workspace before shrinking, then restores the original desktop viewport and workspace before showing results and branches. `verify-site-scenes.mjs` repeats containment, occlusion and scale checks against actual production playback. Inspect captured images as well; geometry alone cannot establish visual acceptance.

`node scripts/verify-site-avatar.mjs` uses the checkpoint server to check the entire avatar menu, including recent-avatar and upload choices, against both camera tracks in all four personalization assets. Production scene verification also checks menu containment and readable scale. Re-record personalization assets and regenerate their checkpoint images after changing these camera bounds.

`node scripts/verify-site-headings.mjs` checks actual rendered heading lines and overflow at 320/390/768/1280/1440px in both languages. It rejects orphan Chinese final lines and saves measured lines under `.codex-temp/site-heading-verification/`. Theme verification includes open setup and validation menus, checking their opacity, dimensions and option count after restoration; `SITE_SCENES` may select a subset.

Netlify-style `_redirects` and `_headers` are generated. On another static host, serve existing files first, route `/demo` and `/demo/*` to `/demo/index.html`, and route the known website home/documentation paths to `/index.html`. The final fallback serves `/index.html` with status404 so its existing missing-page view renders without claiming that a missing file exists. Do not use a catch-all200 rewrite. Enable Brotli or gzip. Cache fingerprinted `/assets/*` for one year with `immutable`; revalidate HTML and any future archive catalog. Deploy HTML and referenced assets as a single release, retaining old fingerprinted files while clients may still reference them.

`marketing/Caddyfile` provides a tested static-server configuration. Install Caddy from its official release and verify the archive against the release checksum list (Caddy2.11.4 publishes SHA-512 checksums), then run `caddy validate --config marketing/Caddyfile --adapter caddyfile` and `caddy run --config marketing/Caddyfile --adapter caddyfile` from the repository root. Defaults are `http://127.0.0.1:1467`, an explicit loopback bind, `.codex-temp/site-dist` and server storage under `.codex-temp/caddy-storage`. The admin API and config persistence are disabled. `SITE_ADDRESS`, `SITE_BIND`, `SITE_ROOT` and `SITE_STORAGE` configure these separately; a site hostname alone does not restrict listening interfaces. For an actual public host, explicitly configure its hostname/bind and a persistent certificate-storage directory, then verify that host. Local HTTP preview does not prove TLS or public deployment.

With Caddy running, set `SITE_URL=http://127.0.0.1:1467` and run `npm run site:verify:deployment` and `npm run site:verify`. The first checks response bytes, MIME, cache, compression and404 contracts; the second exercises actual browser navigation and replay. Caddy serves only the composed build and does not copy or publish the reviewed session archive. Stop the preview process when verification finishes.

For a standalone Demo, use `caddy run --config marketing/demo/Caddyfile --adapter caddyfile`. It serves only `.codex-temp/demo-dist` on loopback1468; `DEMO_ADDRESS`, `DEMO_BIND`, `DEMO_ROOT` and `DEMO_STORAGE` override its defaults. Demo navigation uses existing hash routes, so no unknown-path SPA rewrite is needed. Missing files retain404. Point the website's build-time `VITE_DEMO_URL` at the chosen Demo origin when deploying these separately.

For separate hosts, use `npm run site:build` and `npm run demo:build`. Their outputs are `.codex-temp/site-dist` and `.codex-temp/demo-dist`. Set `VITE_DEMO_URL` to the independently deployed Demo URL before building the site. `VITE_DOWNLOAD_URL` overrides both download links. Demo receives the selected `language` query parameter. Sample session navigation uses hash routes and preserves project/run/round/node/attempt/outer/branch identity.

Deployment verification: run `npm run site:deployment:test`, then `npm run site:verify:deployment` with `SITE_URL` set to the candidate host and `SITE_DIST` to the matching combined build. It checks HTML byte hashes, language/documentation URLs, entry JS/CSS type, gzip/Brotli, immutable caching, and missing-asset 404. It neither publishes nor establishes browser acceptance. Vite preview ignores deployment header files. Netlify CLI 27.5.2 on Windows returns 403 for nested existing files because its proxy puts backslashes from `path.relative()` in URLs; direct static-server requests return 200. Actual host acceptance remains pending.

## Complete Session Archive

```sh
node scripts/export-demo-session.mjs <source-run-directory> <archive-directory>
npm run demo:archive:prepare -- <archive-directory>
node scripts/verify-demo-session.mjs <archive-directory>
node scripts/audit-demo-privacy.mjs <archive-directory> <audit-report.json>
```

Set `GITLEAKS_BIN` to Gitleaks 8.30.1 or a reviewed later executable. The audit forces default rules, ignores in-file allow comments, redacts scanner output and retains only finding locations. A zero finding count is not a publication approval. Review references, text, images, binaries and all public redaction differences separately.

The audit explicitly enables archive/decode depth5 with a1800-second timeout per scanner invocation. Because Gitleaks uses extensions to identify containers, manifest resources ending in tar.gz/tgz/gz/zip/jar/war/ear/tar receive temporary format aliases after hash and byte checks. Same-volume aliases are hard links; cross-volume scans copy only these resources. Nested findings are mapped back to original resource/member locations, and temporary aliases are removed afterward. Unsupported formats, deeper nesting and damaged content remain review limitations; bare repeated gzip compression is not established as supported by this integration.

After reviewing exact sensitive values, use `npm run demo:archive:public -- <private-archive> <new-candidate-directory>`. Supply a JSON array of `{ "id": "reviewed-1", "value": "..." }` through stdin, or call `createPublicDemoArchive()` with values held in memory. Never put actual values in command arguments, repository files or review logs. The output must not exist and must be outside the source archive. The converter checks source hashes, replaces reviewed values in text and JSON keys/values, rebuilds content addresses and runtime projections, and verifies the staged archive before renaming it to the candidate directory. Original captured version identities and source byte lengths remain intact. Sensitive binary content is rejected for separate review. Failed staging directories retain `incomplete.json` and must not be served.

`disclosure-review.json` records replacement labels, locations and counts without sensitive values. A generated candidate remains `pending-disclosure-review`: independently scan every delivered file for the reviewed values, rerun the privacy audit, and review images, binaries, references and redaction differences before publication. `npm run demo:archive:test` includes conversion, unchanged-source, raw-pagination, dictionary-key collision and corruption regressions.

Historical source references can be imported after public-copy preparation with `npm run demo:archive:sources -- <reviewed-archive> <source-supplement> <new-output>`. The supplement manifest must bind the exact input manifest SHA-256 and identify each original link by full session locator, branch, event and href. Each source file needs its content-addressed resource plus captured evidence and either Git blob provenance or an exact complete-read record. Generated files without historical bytes remain explicit unresolved entries. Repeated identical occurrences are deduplicated; conflicts, scope mismatches, changed originals and partial-read substitutions are rejected.

The importer creates a separate static archive, copies the original immutable resources, preserves message bodies and disclosure records, adds per-session `sourceReferences` assets, and runs full archive verification before exposing its output directory. The supplement audit is preserved byte-for-byte and checked against all projected mappings. This command performs no publication and does not resolve pending disclosure approval. Browser message-origin propagation and source-link reading must also pass their acceptance checks before marking source navigation complete. Rebuild this derived archive from the newly reviewed base whenever redaction or source evidence changes; do not reuse mappings against another manifest revision.

Gzip signatures are checked regardless of filename. Expanded bytes are streamed through Node's standard decompressor with a 2 GiB per-resource budget; reviewed values, corrupt streams and budget overruns reject conversion. This check does not inspect compressed members nested inside the expanded stream or establish approval for other binary formats. The real archive has two incomplete gzip resources. A later deterministic recomputation also proved reviewed group3 is a test vector to retain: regenerate the initial candidate with only groups1 and2. Incomplete-resource handling and nested-content review remain required; see `marketing/ARCHIVE_REVIEW.md`.

For a reviewed source gzip that was captured truncated, an optional third CLI argument supplies a JSON array of `{ sha256, bytes, expandedSha256, expandedBytes, reason: "captured-truncated-gzip" }`; the exported function accepts the same array as its fifth argument. Both source and all recoverable expanded bytes must match this evidence. Only an actual truncated-stream error permits recovery scanning, and sensitive values still reject it. The original compressed bytes are retained and listed in `disclosure-review.json.incompleteGzip`; this records a source limitation, not a repaired or valid release artifact. Unreviewed corruption remains an error.

Preparation adds `runtimeVersion: 1`, source diagnostics, worker identity and byte indexes to immutable session details. Run it after export, then verify the resulting archive. `npm run demo:archive:test` covers export, preparation and privacy interfaces. Raw resource hosting must support HTTP byte ranges with status 206 and an exact Content-Range header; the reader rejects HTTP 200 full-body responses. Storage chunks contain 96 records; raw UI pages independently support up to 200 rows. Filter queries scan chunks in a temporary Web Worker and retain only the requested page and total. No browser-side full-log index is built.

Privacy/reference review and UI/resource adapters remain incomplete. The build does not copy the private archive automatically. This README does not claim complete real-session delivery or final visual acceptance.

Set `VITE_DEMO_ARCHIVE_URL` when building the Demo to connect an independently hosted, prepared archive. The catalog is fetched once after the client shell mounts; selecting its original task loads the separate `runView`, then only the selected history page. Exact links include `project`, `run`, `round`, `node`, `attempt`, `outerNode`, `outerAttempt` and `branch`. The archive API supports history, activity, tool details, raw filtering, captured Diff versions, and text/image attachments. Preparation emits per-change-set metadata without embedding file bodies; verification checks branch ownership and references against the hashed manifest. Local Markdown resource links and full directory adapters remain unfinished. Do not publish the private working archive as part of a preview deployment.

## Shared Frontend Updates

`node scripts/create-demo-reference-fixture.mjs` generates an isolated Markdown image/link acceptance archive at `.codex-temp/demo-reference-fixture`. Serve it with a static server and build a separate Demo output with `VITE_DEMO_ARCHIVE_URL` pointing to that server. Its deep link is `#reference-test?project=reference-fixture&run=run-1`; open the run directory, reports, then report.md. The sample covers an existing product PNG, a line-targeted sibling link and a missing file. It is test data, never a substitute for the complete source archive. Do not serve extensionless archived bytes through Vite development transforms; use static preview hosting.

After changes to `web/src`, rebuild the Demo and run its API, routes, management and archive tests. Check both languages, settings, management pages, read-only actions, exact session links, history pagination, tool/file details, and wide/narrow/wide layout restoration in the production browser. Re-record affected scenes, validate media, rebuild the site and deploy both outputs together. Desktop executable, Agent execution, Git writes and native window behavior require separate EXE verification; browser simulations do not establish those results.
