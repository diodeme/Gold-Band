# Gold Band Console 命令模型

## 1. 设计原则
- command-first
- workflow-first
- 与 scriptable CLI 语义一致
- 不做自然语言解析

## 2. 基本输入形式
console 统一使用 slash command：

```text
/task
/log
/config
/continue
/help
```

同时保留 runtime passthrough：

```text
/run ...
/artifact ...
```

## 3. 命令分类

### 3.1 workspace-local commands
这些命令只改变 console 内部浏览状态或打开全局辅助视图：
- `/task`
- `/log`
- `/config`
- `/continue`
- `/help`

这些辅助视图应以全屏 overlay 方式打开，并通过 `Esc` 返回 workspace。

说明：
- `/config` 指 runtime 配置，而不是 workflow snapshot
- attempt / artifact / attachment 不做独立命令入口，统一通过 node 详情内 Enter/Esc 下钻
- retry 作为 node 详情页内操作项，不作为主 slash command

### 3.2 runtime passthrough commands
这些命令直接映射到 runtime / app 动作：
- `/run start <task-id> [--workflow <path>]`
- `/run status <task-id> <run-id>`
- `/run continue <task-id> <run-id>`
- `/run retry <task-id> <run-id>`
- `/run kill <task-id> <run-id>`
- `/run open-session <task-id> <run-id> --round <round> --node <node> --attempt <attempt>`
- `/artifact list <task-id> <run-id> --round <round> --node <node> --attempt <attempt>`
- `/artifact show <task-id> <run-id> --round <round> --node <node> --attempt <attempt> --name <name>`

### 3.3 help commands
- `/help`
- `/run --help`
- `/artifact --help`
- `/provider --help`

它们只负责帮助展示，不改变命令含义。

## 4. 命令模型约束
- slash command 与 scriptable CLI 共享同一套命令语义
- 参数顺序与命名尽量保持一致
- console 不额外引入自然语言别名
- command bar 应提供可检索提示：`/` 显示一级命令，`/r` 提示 `/run`，`/run ` 提示二级命令
- 补全只作用于命令和参数，不作用于自然语言

## 5. 错误处理
命令错误应分为：
- parse error
- missing argument
- invalid selection/context
- runtime execution error

console 应优先把错误渲染为结构化帮助或状态提示，而不是单纯 panic。

## 6. 一句话总结

> Console 命令模型的核心是：主交互靠 DAG 与 Enter/Esc 下钻，slash command 只负责少量全局辅助动作与 runtime passthrough。
