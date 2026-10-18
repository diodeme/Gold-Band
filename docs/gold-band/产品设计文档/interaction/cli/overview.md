# Gold Band 交互层概览

## 核心判断
- 桌面客户端（EXE）是唯一的人工交互入口：会话、工作流、AUTO、查看、控制和接管均在桌面完成。
- CLI 只保留无人值守的 headless runner，面向 benchmark 与 CI 等自动化场景，与桌面共享同一 core 创建与运行权威。

## 交互原则
- 人工路径不保留 command bar / terminal 心智，使用菜单、按钮、对话框等原生可视化交互。
- 自动化路径不做交互：待处理的 permission / elicitation 由 runtime 按无人值守规则结算，需人工决策的暂停以退出码返回。
- 查看与接管分离：Gold Band 展示 ACP session events、产物与状态；接管原始会话由 provider 自身决定。

## 细分文档
- [CLI 规范](cli.md)
- [Agent 会话观测规范](progress.md)
