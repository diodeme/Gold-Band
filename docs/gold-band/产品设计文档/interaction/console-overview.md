# Gold Band Console 概览

## 1. 一句话定义
Gold Band Console 是 Gold Band 的 workflow-first runtime console。

它面向人工控制台操作，提供：
- Welcome -> Task Picker -> Workspace 三段式进入流程
- 以 workflow DAG 为中心的可视化导航
- node -> attempt -> artifact / attachment 的逐级下钻
- 少量 slash command 驱动的全局辅助能力

它不是聊天框，也不是新的 authoring 工具。

## 2. 产品定位
Console 是 CLI 的一种交互模式，不是独立 backend。

它与 scriptable subcommand CLI 的关系是：
- 共享同一套 runtime 语义
- 共享同一套命令模型
- 共享同一套状态与产物目录
- 共享同一套 provider / worker-ref / progress 边界

## 3. 目标
当前版 console 的目标：
- 保持 command-first，不做自然语言解析
- 保持 workflow-first，不再以 runtime tree 作为主页面中心
- 提供比纯命令行更好的 task 选择、workflow 浏览与节点级下钻体验
- 为后续 VSCode 插件提供一致的信息架构与状态模型

## 4. 非目标
当前版 console 不做：
- 自然语言输入
- 聊天式对话气泡
- 在 Gold Band 内直接生成 task / workflow
- 在自身内部继续 provider 的交互式会话
- 让 progress 文件参与控制流判断
- 替代 scriptable CLI 的自动化用途

## 5. 核心交互原则

### 5.1 入口先选 task，再进入 workspace
用户进入 console 后先看到 Welcome，而不是直接看到运行时目录树。

欢迎页只保留两个入口，并用字符图案作为 Gold Band 视觉入口：
- 新增 task（本期占位）
- 选择现有 task

真正的运行观察与操作发生在选中 task 后的 workspace 内。

### 5.2 主导航围绕 workflow DAG
workspace 的核心不是 task/run/round/attempt 层级树，而是 workflow DAG：
- DAG 节点是一级导航对象
- 边直接在画布中显示语义：`√ / × / ？`，并附带目标 node 提示
- node 选中后进入详情区
- 详情区再逐级展示 attempt、artifact、attachment

### 5.3 命令仍是显式命令，不是自然语言
用户通过 slash command 驱动少量全局动作，例如：

```text
/task
/log
/config
/continue
/help
```

runtime passthrough 命令仍可保留，例如：

```text
/run continue task-001 run-001
/artifact show task-001 run-001 --round round-001 --node dev --attempt attempt-001 --name exec-result
```

### 5.4 查看与执行恢复分离
进入 task 时会：
- 校验 authoring/workflow
- 恢复 active run context

但不会自动执行 `continue`。继续执行必须由显式动作触发。

### 5.5 观测层和控制层分离
Console 可以展示：
- `run-progress.json`
- `events.jsonl`
- `progress.events.jsonl`
- `raw.stream.jsonl`
- `runtime.log`

但控制流结论仍必须来自：
- `run.json`
- `round.json`
- `node.json`
- canonical artifacts

## 6. 与其他入口的关系
- scriptable CLI：面向脚本、自动化、插件调用
- console CLI：面向人工控制台操作
- VSCode 插件：在 CLI 之上做可视化封装

## 7. 一句话总结

> Gold Band Console 是 Gold Band 的 workflow-first runtime console：先选 task，再看 workflow，再进 node 详情，命令只负责少量全局辅助动作。
