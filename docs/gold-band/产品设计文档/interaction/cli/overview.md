# Gold Band 交互层概览

## 核心判断
Gold Band 的交互层采用三面分工：

- 默认 authoring / deep-dive 工具：当前默认是 Claude Code
- CLI：Gold Band 的核心 runtime 接口，同时包含 scriptable subcommand CLI 与 command-driven console CLI 两种入口
- VSCode 插件 / 面板：在 CLI 之上的可视化与控制层

## 交互原则

### 1. 不做新的聊天框
- 聊需求、聊方案、聊 DSL：在默认 authoring 工具中做
- 跑 workflow、看状态、做控制：在 Gold Band 中做
- console CLI 前期不做自然语言输入或自然语言解析

### 2. CLI 是一等公民
CLI 必须能独立完成：
- task 管理
- run 生命周期管理
- artifact 查看
- 日志、配置、provider 输出等详细信息下钻
- 验收失败后的自动进入下一轮或直接停止控制
- 原始 worker 会话打开/继续

其中：
- scriptable CLI 面向自动化、脚本、插件调用
- console CLI 面向人工控制台操作与可视化浏览
- 两者共享同一套 runtime 语义与命令模型

### 3. console CLI 是 runtime console，不是 chat CLI
console CLI 的输入模型是显式命令驱动：
- 用户输入 slash command 或命令参数
- Gold Band 渲染可视化 help、状态视图、时间线和详情面板
- 不做自然语言意图推断

### 4. VSCode 插件不重写 backend
插件应主要：
- 调用 CLI
- 展示 CLI 管理的状态与产物
- 提供更好的 diff、日志与时间线视图

### 5. 查看与接管分离
Gold Band 默认支持：
- 查看 attempt 的输入、产物、events、progress
- 查看日志、配置、artifact、worker-ref、provider 输出
- 基于 `worker-ref` 把控制权交还给 provider，并打开原始会话

Gold Band 不默认支持：
- 在 Gold Band 内直接接管一个正在运行的 provider 会话
- 在 console CLI 内直接与 provider 做聊天式交互

说明：
- 查看 attempt 产物/事件仍是 Gold Band 自己完成的只读能力
- `open-session` 是 handoff 给 provider；打开后是否表现为交互式 continue，由 provider 自身决定
- `run continue` 仍是 Gold Band runtime 控制动作，即使底层可能触发 provider resume

## 细分文档
- [CLI 规范](cli.md)
- [Console 概览](console-overview.md)
- [Console 信息架构](console-information-architecture.md)
- [Console 命令模型](console-command-model.md)
- [Console 状态与事件](console-state-and-events.md)
- [Progress 规范](progress.md)
- [桌面客户端交互概览](../app/overview.md)
