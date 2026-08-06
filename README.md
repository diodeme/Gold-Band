<div align="center">

<img src="src-tauri/icons/icon.png" alt="Gold Band" width="128" />

# Gold Band

> 本地优先的 AI Agent 桌面客户端
>
> 直接对话、固定工作流与 AI 动态编排，统一管理本地 Coding Agent

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[下载](https://github.com/diodeme/Gold-Band/releases)

<!-- README-I18N:START -->

**中文** | [English](./README.en.md)

<!-- README-I18N:END -->

</div>

---

Gold Band 是一个面向本地项目的 AI Agent 桌面客户端。它通过 Agent Client Protocol（ACP）连接 Claude Code、Codex 等本地 Agent，并提供统一的会话、工作空间、权限、附件、历史记录和运行观测体验。

你既可以像使用普通 Agent 客户端一样持续对话，也可以通过固定工作流或 AI 动态编排执行更长、更复杂、需要验证和失败恢复的 Coding 任务。

> [!NOTE]
> Gold Band 仍处于 **Developer Preview**。Claude Code 和 Codex 是当前推荐的体验入口；其他 Agent 的可用性取决于本机环境及其 ACP 支持情况。

## 为什么做 Gold Band

本地 Coding Agent 已经很强，但不同工具之间的会话、配置和任务执行方式仍然割裂；复杂任务还会遇到上下文漂移、缺少独立验收、失败后难以恢复等问题。

Gold Band 希望提供一个统一的桌面入口：

- 简单任务直接与 Agent 对话，不增加额外工作流约束。
- 复杂任务使用固定工作流，把方案、开发、审查、测试和验收分开。
- 开放目标交给 AI 动态拆解，同时由 Gold Band runtime 管理状态和边界。
- 会话、附件、产物、Token、耗时和交互请求保留在同一套可恢复记录中。

## 核心能力

- **Direct Agent**：选择一个 Agent 持续对话，不注入 Gold Band runtime system prompt。
- **WORKFLOW**：通过可视化工作流组织开发、审查、测试、验收和失败回环。
- **AUTO / AI-DYNAMIC**：由 AI 在约束内动态拆分任务、并行执行、合并和验收。
- **可恢复 ACP 会话**：支持流式输出、继续追问、历史恢复、会话复用和可选的外部会话同步。
- **统一会话配置**：在 composer 中选择模型、思考等级、权限模式和 Agent Slash Command。
- **附件与产物**：支持文件选择、拖拽、图片粘贴、附件预览和节点产物归档。
- **运行观测**：查看 Agent 消息、工具调用、系统提示、原始帧、Token、耗时和运行状态。
- **Agent 与上下文管理**：统一维护 Agent、Profile、MCP、SKILL 及用户级、项目级上下文。
- **桌面体验**：提供系统通知、响应式窗口、主题和用户/Agent 自定义头像。

## 快速开始

1. 从 [Releases](https://github.com/diodeme/Gold-Band/releases) 下载桌面安装包，或从源码构建。
2. 打开 Gold Band，添加一个本地工作空间。
3. 在 Agent 管理中配置 Claude Code、Codex 或其他可用 ACP Agent。
4. 回到会话首页，选择运行模式：
   - `DIRECT`：直接与指定 Agent 持续对话，推荐首次使用。
   - `WORKFLOW`：使用固定工作流，适合流程明确、需要强验证的任务。
   - `AUTO`：让 AI-DYNAMIC 动态拆分和调度，适合开放或复杂目标。
5. 输入需求，并在会话详情中查看输出、交互请求、附件、产物和运行状态。

## 运行模式

### DIRECT

DIRECT 模式接近直接使用 Agent 本身。Gold Band 不注入工作流 system prompt，只负责统一的桌面 UI、会话保存、附件、模型与权限配置、停止恢复、Token 和耗时统计。

适合日常问答、代码修改、调试，以及希望保留持续上下文的开发会话。

### WORKFLOW

WORKFLOW 模式使用显式工作流。每个节点代表一次 Agent 执行，边定义成功、失败或人工确认后的流转方向。

适合需要明确开发阶段、独立审查测试、失败回环和结构化验收的任务。

### AUTO / AI-DYNAMIC

AUTO 模式由 AI-DYNAMIC 根据目标动态提出后续节点，可以拆分子任务、并行处理、合并结果并创建验收或修复节点。Gold Band runtime 会校验 proposal 并管理真实运行状态，Agent 不能直接修改 runtime。

适合难以预先确定完整流程、但仍需要运行边界和可观测性的复杂任务。

## 界面

### 会话首页

从一个入口选择工作空间、运行模式、Agent、模型和权限，然后直接发起任务。

![alt text](docs/images/README/image.png)

### Direct 会话与运行观测

持续查看 Agent 输出、思考过程、工具调用、结构化提问、附件、产物、Token 和耗时，并在需要时停止或继续会话。

![alt text](docs/images/README/image-5.png)

### Agent 管理

配置 Agent 启动方式、目录、环境变量、外部会话同步，并查看环境诊断结果。

![alt text](docs/images/README/image-1.png)

### 工作流编排

在可视化画布中维护节点、边、Profile、权限、输出契约和失败流转策略。

![alt text](docs/images/README/image-3.png)

### 上下文管理

管理用户级和项目级 Profile、MCP 与 SKILL，并按运行需要复用。

![alt text](docs/images/README/image-4.png)

## 当前状态

当前已经可用的主路径：

- 以会话首页为核心的桌面交互。
- DIRECT、WORKFLOW 和 AUTO 三种运行模式。
- Claude Code 与 Codex 的 ACP 主路径。
- ACP 长连接、历史恢复、上下文压缩后的状态保持和可选外部会话同步。
- Slash Command、模型、思考等级和权限模式配置。
- 多工作空间会话、搜索、附件、产物、Token、耗时和桌面通知。
- 工作流、Agent、Profile、MCP 和 SKILL 管理。

仍在持续打磨：

- 多 ACP Agent 的兼容性。
- AUTO / AI-DYNAMIC 在复杂真实任务中的稳定性和规划质量。
- Developer Preview 阶段的异常恢复、性能和产品细节。

## 适合与不适合

适合：

- 希望用统一桌面客户端使用多个本地 Coding Agent。
- 需要持续对话、历史恢复和附件协作的开发任务。
- 需要把开发、审查、测试和验收分开的长程任务。
- 希望保留运行过程、产物并支持失败恢复的任务。

暂不适合：

- 要求稳定商用 SLA 的生产环境。
- 依赖尚未完整支持的 ACP Agent 或 Provider 特性。
- 不愿接受 Developer Preview 阶段 UI 和行为快速变化的用户。

## 本地开发

```bash
npm install
npm run dev
```

常用验证命令：

```bash
cargo check
npm run web:test
npm run web:build
```

## 技术栈

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## 社区与反馈

本项目积极参与和支持 [linux.do 社区](https://linux.do)。欢迎通过 Issue 和 Pull Request 反馈 Agent 接入、会话体验、工作流、AUTO 拆解质量及异常恢复问题。

AGPL-3.0-only，详见 [LICENSE](LICENSE)。
