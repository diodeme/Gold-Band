<p align="center">
  <img src="web/public/logo.svg" alt="Gold Band logo" width="180" />
</p>

# Gold Band Harness

<!-- README-I18N:START -->

[English](./README.md) | **简体中文**

<!-- README-I18N:END -->

Gold Band 是一个桌面端 harness，用于编排、观测和恢复本地 AI agent workflow。

它用确定性的 workflow 控制来约束强大的 coding agent：task 会被转化为 workflow run，run 会产出 canonical state 与 artifacts，每个 round 都可以通过桌面客户端查看，而不是只依赖 terminal transcript。

> [!NOTE]
> Gold Band 当前是 desktop-first。Rust CLI/runtime 仍作为 backend 和诊断入口保留，但主要产品形态是 Tauri 桌面客户端。

## 为什么选择 Gold Band？

AI coding agent 擅长执行，但生产级工作需要的不只是 chat transcript：

- **Workflow control** — 模型驱动的工作通过显式 nodes、edges、retries 和 rounds 执行。
- **Observable execution** — runs、rounds、nodes、logs、ACP sessions、artifacts 和 attachments 可以在一个地方浏览。
- **Artifact-first verification** — 完成判断基于 runtime state 和声明式 outputs，而不是只依赖 agent 自报。
- **Provider isolation** — provider-specific 细节隔离在 adapters 后面；runtime 保持 provider-agnostic。

## 当前能力

- 使用 Tauri、React、Tailwind CSS 和 shadcn/ui 构建的桌面端 task orchestration workspace。
- Task list、workflow authoring、run history 和 round detail drill-down。
- 用于 authoring 和 execution inspection 的可视化 workflow graph。
- Agent management：维护已配置的 agent types、launch commands、environment variables 和 diagnostics。
- Context/profile management：管理可复用的 role prompts。
- ACP-first provider path，并使用 ACP session events 查看 agent conversation。
- 面向 tasks、runs、rounds、attempts、artifacts、attachments 和 logs 的 canonical runtime state。
- 通过 runtime contract 执行 continue、retry、stop/kill 等恢复操作。

## 架构

```text
Gold Band Desktop
├─ web/                 React + Vite desktop UI
├─ src-tauri/           Tauri 2 desktop shell and commands
├─ src/                 Rust runtime, DSL, storage, provider, CLI
├─ docs/gold-band/      Product design docs and development plans
└─ .gold-band/          Project-level presets/config overrides
```

运行时，Gold Band 分为三层：

| 层级 | 职责 |
|---|---|
| Desktop client | Workspace navigation、workflow authoring、runtime browsing 和直接用户操作 |
| Rust runtime | Workflow validation、execution control、state transitions 和 artifact normalization |
| Provider adapters | 启动 agent workers、交换 ACP/session data，并暴露 provider capabilities |

## 技术栈

| 领域 | 技术 |
|---|---|
| Desktop shell | Tauri 2 |
| Frontend | React 19, Vite, TypeScript |
| Styling | Tailwind CSS v4, shadcn/ui, Radix UI |
| Workflow graph | `@xyflow/react`, `dagre` |
| Runtime | Rust 2024, Tokio, Clap |
| Agent protocol | Agent Client Protocol (`agent-client-protocol`) |

## 前置要求

- Node.js and npm
- Rust toolchain with Cargo
- Tauri 2 所需的平台依赖
- 用于真实 workflow execution 的 ACP-compatible coding agent setup

对于默认 Claude ACP path，Gold Band 会通过 `npx` 启动配置好的 agent command。

## 快速开始

安装依赖：

```bash
npm install
```

以开发模式运行桌面应用：

```bash
npm run dev
```

构建桌面应用：

```bash
npm run build
```

仅运行 web UI，用于浏览器布局/调试：

```bash
npm run web:dev
```

> [!TIP]
> 验证真实桌面行为时请使用 Tauri app（`npm run dev`）。`npm run web:dev` 适合配合浏览器 mock view models 快速迭代 UI。

## 常用脚本

| 命令 | 说明 |
|---|---|
| `npm run dev` | 启动 Tauri desktop app 和 Vite dev server |
| `npm run build` | type-check/build web UI，并打包 Tauri app |
| `npm run web:dev` | 仅在 `127.0.0.1:1420` 启动 Vite web UI |
| `npm run web:build` | 仅构建 web frontend |
| `npm run web:preview` | 预览已构建的 web frontend |
| `cargo test` | 运行 core runtime 的 Rust tests |

## Workspace 与 runtime data

仓库级 `.gold-band/` 目录只用于项目配置和 presets。

执行状态会写入用户级 per-project runtime store：

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

完整 storage contract 见 [runtime layout](docs/gold-band/产品设计文档/runtime/layout.md)。

## 桌面端交互模型

Gold Band 被设计为原生 desktop workspace，而不是 CLI wrapper 或 chat app。

主要导航模型是：

```text
Task list
  -> Task workflow
    -> Round detail
```

桌面客户端优先采用：

- sidebar navigation，而不是 command bars；
- buttons、menus、tables、drawers 和 graph interactions，而不是 terminal input；
- canonical state、artifacts 和 logs，而不是只看 transcript；
- 直接 recovery actions，而不是手动编辑文件。

## CLI 与 runtime commands

CLI 仍可用于 scripts、tests 和 diagnostics，但它不再是 README 的主要用户路径。

示例：

```bash
cargo run -- --help
cargo run -- run status <task-id> <run-id>
cargo run -- artifact list <task-id> <run-id> --round <round-id> --node <node-id> --attempt <attempt-id>
```

CLI 定义位于 [src/cli/mod.rs](src/cli/mod.rs)。

## 文档

- [Product overview](docs/gold-band/产品设计文档/product/overview.md)
- [Desktop interaction overview](docs/gold-band/产品设计文档/interaction/app/overview.md)
- [Provider overview](docs/gold-band/产品设计文档/provider/overview.md)
- [Runtime overview](docs/gold-band/产品设计文档/runtime/overview.md)
- [Workflow DSL overview](docs/gold-band/产品设计文档/dsl/overview.md)
- [MVP development plan](docs/gold-band/开发计划/gold-band-mvp-plan.md)

## 许可证

Gold Band 仅基于 GNU Affero General Public License v3.0 授权。完整许可证文本见 [LICENSE](LICENSE)。

你可以在遵守 AGPL-3.0 的前提下使用、修改、分发本项目，或通过网络向用户提供本项目服务。分发修改版或将修改版作为网络服务提供时，需要按 AGPL-3.0 要求向对应用户提供相应源码。

## 故障排查

### `npm run dev` 无法启动桌面应用

检查 Node.js、npm、Rust、Cargo 和 Tauri 平台前置依赖是否已安装。然后在仓库根目录重新运行 `npm install`。

### Web UI 能启动，但真实 run 无法工作

`npm run web:dev` 会以浏览器/调试行为运行 frontend。请使用 `npm run dev` 来验证 Tauri command layer 和真实 runtime integration。

### Workflow run 暂停或失败，且没有 artifacts

在桌面客户端中打开对应 run/round，检查 node state、ACP session、logs 和 raw diagnostic data。最终 outcome 由 canonical runtime state 决定，而不是只由 provider text 决定。

### CLI 示例与桌面行为不一致

正常产品使用请优先使用桌面客户端。CLI commands 适合 automation 和 debugging，但桌面交互是当前产品方向。

## Community

本项目积极参与并认可 [linux.do社区](linux.do)
