<div align="center">

<img src="src-tauri/icons/icon.png" alt="Gold Band" width="128" />

# Gold Band

> A local-first desktop client for AI Agents
>
> Direct conversations, fixed workflows, and AI-driven orchestration for local Coding Agents

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[Download](https://github.com/diodeme/Gold-Band/releases)

<!-- README-I18N:START -->

**English** | [中文](./README.zh-CN.md)

<!-- README-I18N:END -->

</div>

---

Gold Band is a desktop AI Agent client for local projects. It connects to local Agents such as Claude Code and Codex through Agent Client Protocol (ACP), providing one place for conversations, workspaces, permissions, attachments, history, and runtime observability.

You can use it as a regular Agent client for continuous conversations, or run longer and more complex Coding tasks through fixed workflows or AI-driven orchestration with validation and failure recovery.

![alt text](docs/images/README.en/gold-band-readme-hero-wide.png)

> [!NOTE]
> Gold Band is still in **Developer Preview**. Claude Code and Codex are the recommended starting points. Availability of other Agents depends on the local environment and their ACP support.

## Why Gold Band

Local Coding Agents are already powerful, but their conversations, configuration, and execution models remain fragmented. Complex tasks also suffer from context drift, weak independent validation, and poor recovery after failure.

Gold Band provides a unified desktop entry point:

- Use an Agent directly for simple work without adding workflow constraints.
- Use fixed workflows to separate planning, development, review, testing, and acceptance.
- Let AI dynamically decompose open-ended goals while Gold Band manages runtime state and boundaries.
- Keep conversations, attachments, artifacts, tokens, duration, and interaction requests in one recoverable record.

## Core Capabilities

- **Direct Agent**: continuously chat with a selected Agent without injecting a Gold Band runtime system prompt.
- **WORKFLOW**: organize development, review, testing, acceptance, and failure loops in a visual workflow.
- **AUTO / AI-DYNAMIC**: let AI dynamically split, parallelize, merge, and validate work within runtime constraints.
- **Recoverable ACP sessions**: support streaming, follow-up prompts, history recovery, session reuse, and optional external session sync.
- **Unified session configuration**: choose models, thought levels, permission modes, and Agent Slash Commands from the composer.
- **Attachments and artifacts**: support file selection, drag-and-drop, pasted images, previews, and node artifact archival.
- **Runtime observability**: inspect Agent messages, tool calls, system prompts, raw frames, tokens, duration, and runtime state.
- **Agent and context management**: manage Agents, Profiles, MCP, SKILL, and user-level or project-level context.
- **Desktop experience**: system notifications, responsive windows, themes, and customizable user and Agent avatars.

## Quick Start

1. Download a desktop package from [Releases](https://github.com/diodeme/Gold-Band/releases), or build from source.
2. Open Gold Band and add a local workspace.
3. Configure Claude Code, Codex, or another available ACP Agent in Agent Management.
4. Return to the conversation home and choose a run mode:
   - `DIRECT`: continuously chat with a selected Agent. Recommended for first-time use.
   - `WORKFLOW`: use a fixed workflow for tasks with clear stages and stronger validation.
   - `AUTO`: let AI-DYNAMIC dynamically split and schedule open-ended or complex goals.
5. Enter a requirement and inspect output, interaction requests, attachments, artifacts, and runtime state in the conversation detail view.

## Run Modes

### DIRECT

DIRECT mode is close to using the Agent itself. Gold Band does not inject a workflow system prompt; it only provides a unified desktop UI, session storage, attachments, model and permission configuration, stop and recovery controls, and token and duration metrics.

It is suitable for everyday questions, code changes, debugging, and development conversations that need persistent context.

### WORKFLOW

WORKFLOW mode uses an explicit workflow. Each node represents one Agent execution, while edges define transitions after success, failure, or manual confirmation.

It is suitable for tasks that require clear development stages, independent review and testing, failure loops, and structured acceptance.

### AUTO / AI-DYNAMIC

AUTO mode lets AI-DYNAMIC propose the next nodes from the goal. It can split subtasks, run branches in parallel, merge results, and create acceptance or repair nodes. The Gold Band runtime validates proposals and owns the real runtime state; Agents cannot mutate the runtime directly.

It is suitable for complex tasks whose complete workflow cannot be determined in advance but still require execution boundaries and observability.

## Interface

### Conversation Home

Choose a workspace, run mode, Agent, model, and permission mode from one entry point, then start a task directly.

![alt text](docs/images/README.en/image-1.png)

### Direct Session and Runtime Observation

Inspect Agent output, thoughts, tool calls, structured questions, attachments, artifacts, tokens, and duration, then stop or continue the session when needed.

![alt text](docs/images/README.en/image-6.png)

### Agent Management

Configure Agent launch settings, directories, environment variables, external session sync, and environment diagnostics.

![alt text](docs/images/README.en/image-2.png)

### Workflow Orchestration

Maintain nodes, edges, Profiles, permissions, output contracts, and failure transition strategies on a visual canvas.

![alt text](docs/images/README.en/image-3.png)

### Context Management

Manage user-level and project-level Profiles, MCP, and SKILL assets, then reuse them as needed during execution.

![alt text](docs/images/README.en/image-4.png)

## Current Status

Main paths currently available:

- A conversation-first desktop experience.
- DIRECT, WORKFLOW, and AUTO run modes.
- The primary Claude Code and Codex ACP paths.
- ACP long-lived connections, history recovery, state preservation after context compaction, and optional external session sync.
- Slash Commands, model, thought-level, and permission-mode configuration.
- Multi-workspace conversations, search, attachments, artifacts, tokens, duration, and desktop notifications.
- Workflow, Agent, Profile, MCP, and SKILL management.

Areas still being improved:

- Compatibility with more ACP Agents.
- AUTO / AI-DYNAMIC stability and planning quality on complex real-world tasks.
- Error recovery, performance, and product details during Developer Preview.

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

Issues and pull requests about Agent integration, conversation UX, workflows, AUTO decomposition quality, and error recovery are welcome.

AGPL-3.0-only. See [LICENSE](LICENSE).
