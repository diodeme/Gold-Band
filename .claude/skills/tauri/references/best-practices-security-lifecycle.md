---
name: tauri-best-practices-security-lifecycle
description: Security across the lifecycle — upstream, development, build, runtime; keep deps updated, audit, dev server. Use when hardening or auditing a Tauri app.
---

# Security Lifecycle

Security depends on every stage: **upstream** (Tauri and deps), **development** (dev machine and tooling), **build** (artifacts and supply chain), and **runtime** (permissions and frontend). The weakest link defines your security.

## Upstream

- **Keep Tauri and deps updated.** Vulnerabilities in Tauri or the compiler (rustc) / transpilers (Node) can affect your app. Update regularly and watch security advisories.
- **Evaluate dependencies.** Prefer maintained, audited crates and npm packages. Use **cargo audit**, **npm audit**, and consider **cargo-vet** / **cargo-crev** / **cargo supply-chain** for supply-chain visibility. For critical deps, prefer git hash or tag pins.
- **Trust and maintenance.** Consider health and authorship of direct and transitive deps; unmaintained or unreviewed code is a risk.

## Development

- **Dev server.** The frontend dev server often binds to localhost or the network. Treat it as sensitive; avoid exposing it to untrusted networks and lock down when not needed.
- **Development machine.** Supply-chain and “dev machine” attacks are real. Keep OS and toolchains updated and limit what runs with high privilege or has access to secrets and signing keys.
- **Isolation pattern.** When the frontend has many dependencies, use the [Isolation](/concept/ipc-patterns/) pattern so a small, auditable layer validates IPC before it reaches the Core.

## Build and distribution

- **Reproducible and clean builds.** Use lockfiles (Cargo.lock, pnpm-lock.yaml, etc.) and a clean CI environment so builds are reproducible and free of local-only or accidental dependencies.
- **Signing and integrity.** Sign binaries and installers; protect signing keys and use them only in secure environments (e.g. CI with secrets, not on dev machines).

## Runtime

- **Least privilege.** Give each window only the [capabilities](/security/capabilities/) and [permissions](/security/permissions/) it needs. Use [scopes](/security/scope/) to restrict paths/URLs.
- **Validate in Core.** Treat the frontend as untrusted; validate and sanitize all inputs in Rust commands. Don’t trust the frontend for security decisions.
- **Runtime authority.** The Core’s runtime authority enforces capabilities and scopes on every invoke; if the origin isn’t allowed, the command is never run. Rely on this instead of frontend-only checks.

## Key Points

- Update Tauri and tooling; audit and vet dependencies; lock down dev and build environments; use capabilities and scopes at runtime.
- Use the [security](/security/) docs and [lifecycle](/security/lifecycle/) doc for the full threat overview and checklist.

<!--
Source references:
- https://v2.tauri.app/security/lifecycle/
- https://github.com/tauri-apps/tauri-docs
-->
