<p align="center">
  <img src="web/public/logo.svg" alt="Gold Band logo" width="180" />
</p>

# Gold Band Harness

<!-- README-I18N:START -->

**English** | [简体中文](./README.zh-CN.md)

<!-- README-I18N:END -->

Gold Band is a desktop harness for orchestrating, observing, and recovering local AI-agent workflows.

It wraps powerful coding agents with deterministic workflow control: tasks become workflow runs, runs produce canonical state and artifacts, and every round can be inspected through a desktop client instead of a terminal transcript.

> [!NOTE]
> Gold Band is currently desktop-first. The Rust CLI/runtime still exists as a backend and diagnostic surface, but the primary product shape is the Tauri desktop client.

## Why Gold Band?

AI coding agents are strong at execution, but production work needs more than a chat transcript:

- **Workflow control** — model-driven work is executed through explicit nodes, edges, retries, and rounds.
- **Observable execution** — runs, rounds, nodes, logs, ACP sessions, artifacts, and attachments are browsable in one place.
- **Artifact-first verification** — completion is based on runtime state and declared outputs, not only on agent self-reporting.
- **Provider isolation** — provider-specific details stay behind adapters; the runtime remains provider-agnostic.

## Current capabilities

- Desktop task orchestration workspace built with Tauri, React, Tailwind CSS, and shadcn/ui.
- Task list, workflow authoring, run history, and round detail drill-down.
- Visual workflow graph for authoring and execution inspection.
- Agent management for configured agent types, launch commands, environment variables, and diagnostics.
- Context/profile management for reusable role prompts.
- ACP-first provider path, with ACP session events used for agent conversation inspection.
- Canonical runtime state for tasks, runs, rounds, attempts, artifacts, attachments, and logs.
- Recovery operations such as continue, retry, and stop/kill through the runtime contract.

## Architecture

```text
Gold Band Desktop
├─ web/                 React + Vite desktop UI
├─ src-tauri/           Tauri 2 desktop shell and commands
├─ src/                 Rust runtime, DSL, storage, provider, CLI
├─ docs/gold-band/      Product design docs and development plans
└─ .gold-band/          Project-level presets/config overrides
```

At runtime, Gold Band separates three layers:

| Layer | Responsibility |
|---|---|
| Desktop client | Workspace navigation, workflow authoring, runtime browsing, direct user operations |
| Rust runtime | Workflow validation, execution control, state transitions, artifact normalization |
| Provider adapters | Start agent workers, exchange ACP/session data, expose provider capabilities |

## Tech stack

| Area | Stack |
|---|---|
| Desktop shell | Tauri 2 |
| Frontend | React 19, Vite, TypeScript |
| Styling | Tailwind CSS v4, shadcn/ui, Radix UI |
| Workflow graph | `@xyflow/react`, `dagre` |
| Runtime | Rust 2024, Tokio, Clap |
| Agent protocol | Agent Client Protocol (`agent-client-protocol`) |

## Prerequisites

- Node.js and npm
- Rust toolchain with Cargo
- Platform dependencies required by Tauri 2
- An ACP-compatible coding agent setup for real workflow execution

For the default Claude ACP path, Gold Band launches the adapter through `npx` using the configured agent command.

## Quick start

Install dependencies:

```bash
npm install
```

Run the desktop app in development mode:

```bash
npm run dev
```

Build the desktop app:

```bash
npm run build
```

Run the web UI only for browser layout/debug work:

```bash
npm run web:dev
```

> [!TIP]
> Use the Tauri app (`npm run dev`) when validating real desktop behavior. `npm run web:dev` is useful for fast UI iteration with browser mock view models.

## Useful scripts

| Command | Description |
|---|---|
| `npm run dev` | Start the Tauri desktop app with the Vite dev server |
| `npm run build` | Type-check/build the web UI and package the Tauri app |
| `npm run web:dev` | Start only the Vite web UI on `127.0.0.1:1420` |
| `npm run web:build` | Build only the web frontend |
| `npm run web:preview` | Preview the built web frontend |
| `cargo test` | Run Rust tests for the core runtime |

## Workspace and runtime data

The repository-level `.gold-band/` directory is for project configuration and presets only.

Execution state is written to a user-level per-project runtime store:

```text
~/.gold-band/projects/{project-id}/
  project.json
  logs/runtime.log
  context/profiles/
  tasks/
    <task-id>/
      task.json
      authoring/
        requirement.md
        workflow.json
      runs/
        <run-id>/
          run.json
          workflow.snapshot.json
          rounds/
            <round-id>/
              round.json
              nodes/
                <node-id>/
                  <attempt-id>/
                    node.json
                    worker-ref.json
                    acp.events.jsonl
                    artifacts/
                    attachments/
```

See [runtime layout](docs/gold-band/产品设计文档/runtime/layout.md) for the complete storage contract.

## Desktop interaction model

Gold Band is designed as a native desktop workspace, not a CLI wrapper or chat app.

The main navigation model is:

```text
Task list
  -> Task workflow
    -> Round detail
```

The desktop client prioritizes:

- sidebar navigation over command bars;
- buttons, menus, tables, drawers, and graph interactions over terminal input;
- canonical state, artifacts, and logs over transcript-only inspection;
- direct recovery actions over manual file editing.

## CLI and runtime commands

The CLI remains available for scripts, tests, and diagnostics, but it is no longer the README's primary user path.

Examples:

```bash
cargo run -- --help
cargo run -- run status <task-id> <run-id>
cargo run -- artifact list <task-id> <run-id> --round <round-id> --node <node-id> --attempt <attempt-id>
```

CLI definitions live in [src/cli/mod.rs](src/cli/mod.rs).

## Documentation

- [Product overview](docs/gold-band/产品设计文档/product/overview.md)
- [Desktop interaction overview](docs/gold-band/产品设计文档/interaction/app/overview.md)
- [Provider overview](docs/gold-band/产品设计文档/provider/overview.md)
- [Runtime overview](docs/gold-band/产品设计文档/runtime/overview.md)
- [Workflow DSL overview](docs/gold-band/产品设计文档/dsl/overview.md)
- [MVP development plan](docs/gold-band/开发计划/gold-band-mvp-plan.md)

## License

Gold Band is licensed under the GNU Affero General Public License v3.0 only. See [LICENSE](LICENSE) for the full license text.

You may use, modify, distribute, and provide network access to this project under AGPL-3.0, provided that you comply with its copyleft obligations, including making the corresponding source code available for modified versions that you distribute or run as a network service.

## Troubleshooting

### `npm run dev` cannot start the desktop app

Check that Node.js, npm, Rust, Cargo, and the Tauri platform prerequisites are installed. Then run `npm install` again from the repository root.

### The web UI starts, but real runs do not work

`npm run web:dev` runs the frontend with browser/debug behavior. Use `npm run dev` to exercise the Tauri command layer and real runtime integration.

### A workflow run pauses or fails without artifacts

Open the run/round in the desktop client and inspect the node state, ACP session, logs, and raw diagnostic data. The final outcome is determined by canonical runtime state, not by provider text alone.

### CLI examples differ from desktop behavior

Prefer the desktop client for normal product use. CLI commands are useful for automation and debugging, but desktop interaction is the current product direction.
