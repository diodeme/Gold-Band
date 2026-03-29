# Gold Band 交互层概览

## 核心判断
Gold Band 的交互层采用三面分工：

- 默认 authoring / deep-dive 工具：当前默认是 Claude Code
- CLI：核心 runtime 与完整可独立运行的交互入口
- VSCode 插件 / 面板：在 CLI 之上的可视化与控制层

## 交互原则

### 1. 不做新的聊天框
- 聊需求、聊方案、聊 DSL：在默认 authoring 工具中做
- 跑 workflow、看状态、做控制：在 Gold Band 中做

### 2. CLI 是一等公民
CLI 必须能独立完成：
- task 管理
- run 生命周期管理
- artifact 查看
- 验收失败后的自动进入下一轮或直接停止控制
- 原始 worker 会话打开/继续

### 3. VSCode 插件不重写 backend
插件应主要：
- 调用 CLI
- 展示 CLI 管理的状态与产物
- 提供更好的 diff、日志与时间线视图

### 4. 查看与接管分离
Gold Band 默认支持：
- 查看 attempt 的输入、产物、events、progress
- 基于 `worker-ref` 打开原始会话

Gold Band 不默认支持：
- 直接接管一个正在运行的 provider 会话

## 细分文档
- [CLI 规范](cli.md)
- [Progress 规范](progress.md)
