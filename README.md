<div align="center">

<img src="web/public/logo.svg" alt="Gold Band" width="128" />

# Gold Band

> Aiming to be the last Agent desktop client you need
>
> The experience of mainstream Agent clients plus a complete workflow system, for everyday development and long-running unattended work on large requirements

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#platforms-and-languages)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[Download](https://github.com/diodeme/Gold-Band/releases) · [Live UI Preview](https://gold-band.dion.blue/en/demo#)

<!-- README-I18N:START -->

**English** | [简体中文](./docs/i18n/README.zh-CN.md) | [繁體中文](./docs/i18n/README.zh-TW.md) | [日本語](./docs/i18n/README.ja-JP.md) | [한국어](./docs/i18n/README.ko-KR.md) | [Português (Brasil)](./docs/i18n/README.pt-BR.md) | [Español](./docs/i18n/README.es.md)

<!-- README-I18N:END -->

</div>

---

Gold Band is a desktop AI Agent client for local projects. It connects to mainstream Agents such as Claude Code and Codex through Agent Client Protocol (ACP): one interaction design, many harnesses you can switch between. It also provides complete workflow and AUTO orchestration, so long-running tasks stay stable and observable instead of depending on a lucky single model run.

> [!TIP]
> Want to see it first? Open the [Live UI Preview](https://gold-band.dion.blue/en/demo#). A desktop browser is recommended. The online preview is limited; the desktop client is the reference experience.

> [!NOTE]
> Gold Band is still in **Developer Preview**. Core capabilities are stable, while interaction details are iterating quickly.

## Highlights

- **One client for mainstream Agents**: built-in support for Claude Code, Codex, Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp, and Pi, plus custom integration of any ACP-compatible Agent.
- **Three run modes**: DIRECT conversations, fixed WORKFLOW, and AUTO orchestration cover everything from quick questions to large requirements.
- **Engineered workflows**: configure the Agent, model, role, and result evaluation per node; go back to a previous session to repair it, or start a new round to keep implementing the requirement.
- **AUTO mode for large tasks**: a node splits the goal into subtasks and dispatches them; each subtask runs in its own Git worktree, a merge node combines the results, an accept node validates them, and the next round is dispatched based on the outcome.
- **What you expect from an Agent client**: SKILL, MCP, and role (Profile) management, scheduled tasks, requirement management (Multica integration), file viewing and editing, source control, a built-in browser, IM remote intervention and notifications, plus wallpapers, avatars, fonts, and themes.
- **Lightweight**: built with Tauri 2 and Rust. The installer is only tens of MB, and memory usage stays around 300 MB with multiple sessions running in parallel.

## Supported Agents

| Built-in Agents | |
| --- | --- |
| Claude Code, Codex | Recommended starting points |
| Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp, Pi | Built in; availability depends on the local environment and each Agent's ACP support |
| Custom Agents | Any ACP-compatible Agent can be added manually in Agent Management |

## Run Modes

### DIRECT

Close to using the Agent itself. Gold Band does not inject a workflow system prompt; it only provides a unified desktop UI, session storage, attachments, model and permission configuration, stop and recovery controls, and token and duration metrics.

Suitable for everyday questions, code changes, debugging, and development conversations that need persistent context.

### WORKFLOW

Uses an explicit workflow. Each node represents one Agent execution and can use its own Agent, model, role, and result evaluation; edges define transitions after success, failure, or manual confirmation. After a run, you can go back to a previous session to repair issues, or start a new round to keep going.

Suitable for tasks that need clear development stages, independent review and testing, failure loops, and structured acceptance.

### AUTO / AI-DYNAMIC

AI-DYNAMIC proposes the next nodes from the goal: it splits subtasks, runs them in parallel in separate worktrees, merges results through a merge node, validates them through an accept node, and dispatches a new round based on the current outcome. The Gold Band runtime validates proposals and owns the real runtime state; Agents cannot mutate the runtime directly.

Suitable for large or complex tasks whose complete workflow cannot be determined in advance but still require execution boundaries and observability.

## More Capabilities

- **Conversations**: streaming, follow-up prompts, history recovery, session reuse, and optional external session sync; choose models, thought levels, permission modes, and Slash Commands from the composer.
- **Attachments and artifacts**: file selection, drag-and-drop, pasted images, workspace file references, previews, and node artifact archival.
- **Runtime observability**: inspect Agent messages, tool calls, system prompts, raw frames, tokens, duration, and runtime state.
- **Workspace**: file browsing and live editing, Git source control, and a built-in browser.
- **Automation and collaboration**: scheduled tasks, Multica requirement management, IM remote intervention and notifications (WeCom for now), and system notifications.
- **Agent and context management**: manage Agents, Profiles, MCP, SKILL, and user-level or project-level context, with Agent environment diagnostics.
- **Personalization**: themes, wallpapers, fonts, custom user and Agent avatars, and personal usage analytics.

## Quick Start

1. Download a desktop package from [Releases](https://github.com/diodeme/Gold-Band/releases), or build from source.
2. Open Gold Band and add a local workspace.
3. Enable Claude Code, Codex, or another ACP Agent in Agent Management and make sure its environment diagnostics pass.
4. Return to the conversation home and choose a run mode:
   - `DIRECT`: continuously chat with a selected Agent. Recommended for first-time use.
   - `WORKFLOW`: use a fixed workflow for tasks with clear stages and stronger validation.
   - `AUTO`: let AI-DYNAMIC dynamically split and schedule open-ended or complex goals.
5. Enter a requirement and inspect output, interaction requests, attachments, artifacts, and runtime state in the conversation detail view.

> [!IMPORTANT]
> The project does not yet have Apple Developer Program credentials, so the macOS Release is not signed with Developer ID or notarized by Apple. Follow the [macOS Installation and Troubleshooting Guide](docs/guide/macos-install.md) for installation options and Gatekeeper troubleshooting.

## Platforms and Languages

- **Platforms**: packages are available for Windows, macOS, and Linux. Windows 10 / 11 is the primary target, followed by Apple Silicon and Intel Macs; the Linux build has not been fully tested yet.
- **UI languages**: Simplified Chinese, Traditional Chinese, English, Japanese, Korean, Portuguese (Brazil), and Spanish.

## FAQ

### How is this different from the workflows inside Coding Agents?

Workflows inside Coding Agents are usually "a main Agent orchestrating sub-Agents" or "scripts orchestrating an Agent"; they orchestrate **sessions**. Gold Band orchestrates **harnesses**. Any ACP-compatible Agent can become a node: for example, Codex with its built-in browser and computer use tools as an acceptance node, and the minimal, fast Pi as a development node. Nodes can differ by an entire harness, not just by context.

### How is this different from Agent clients like the Codex App?

Those clients are built around one fixed Agent. Gold Band sits above the Agents, so it can switch between and combine Agents with different architectures while inheriting their capabilities. The trade-off is that Gold Band cannot reach inside an Agent's loop; features such as steering an Agent mid-loop with a user prompt are harder to build.

### How is this different from other ACP clients?

Gold Band started from workflows, and the ACP client capabilities were built on top. Its workflows go beyond simple scheduling: acceptance criteria, prior-context summaries, stop and resume, and node merging in AUTO mode. The runtime drives node progression and failure handling, so each Agent can focus on its current node.

## Status and Roadmap

Known issues:

- There are still some minor interaction bugs, and they are being fixed continuously.
- WORKFLOW and AUTO rely on adversarial validation and loops, so they take more time and tokens than asking an Agent directly, but they reduce rework.
- A built-in terminal and mobile remote control are not available yet.

Roadmap:

1. Keep improving the client experience and fixing known UI bugs.
2. Make workflows more robust and easier to use, such as rerunning any node and creating workflows from natural language.
3. Complete a typical, highly complex requirement with WORKFLOW and AUTO to publicly demonstrate the value of orchestration.
4. Rebuild around a client → p2p / relay → host architecture, supporting local and remote directories as workspaces and multiple clients controlling one host.

## Good Fit

Gold Band is a good fit for:

- Users who want one desktop client for multiple local Coding Agents.
- Development tasks that need continuous conversations, history recovery, and attachment collaboration.
- Long-running work that separates development, review, testing, and acceptance.
- Tasks that need process records, artifacts, and failure recovery.

Gold Band is not yet a good fit for:

- Production environments that require a stable commercial SLA.
- Workloads that depend on ACP Agents or Provider features not yet fully supported.
- Users who do not want Developer Preview UI and behavior to change quickly.

## Local Development

```bash
npm install
npm run dev
```

Common verification commands:

```bash
cargo check
npm run web:test
npm run web:build
```

## Tech Stack

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## Community and Feedback

This project actively participates in and supports the [linux.do community](https://linux.do). Stars, trials, issues, and pull requests about Agent integration, conversation UX, workflows, AUTO decomposition quality, and error recovery are all welcome.

AGPL-3.0-only. See [LICENSE](LICENSE).
