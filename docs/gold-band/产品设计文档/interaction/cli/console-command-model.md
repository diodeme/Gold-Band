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
/theme [gold-band|nord|dracula|cyber|onyx|mist|high-contrast]
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
- `/theme [gold-band|nord|dracula|cyber|onyx|mist|high-contrast]`
- `/continue`
- `/help`

这些辅助视图应以单面板 modal overlay 方式打开，占用 workspace 主区域，并通过 `Esc` 返回 workspace。overlay 打开后背景 workspace 不再接收键盘输入。长内容必须支持 `↑/↓` 滚动。`/log` 默认显示最新 500 行，并优先按文件尾部读取，避免整份 runtime log 全量载入内存。

Task Picker 与 Workspace 对 `/log`、`/config`、`/help` 的辅助视图行为必须一致，统一使用 modal overlay，不允许一个 screen 内联替换、另一个 screen 弹窗。

说明：
- `/config` 指 runtime 配置与当前 console session 生效结果，而不是 workflow snapshot
- `/theme <name>` 立即切换当前 session theme，并持久化写入 `~/.gold-band/config.json`
- Task Picker 支持 `s` 直接启动当前选中 task；成功后直接进入对应 workspace
- attempt detail 支持 `l` 在 `progress.events.jsonl` / `raw.stream.jsonl` 之间切换
- `/run start <task-id>` 在 console 内仍保留，但成功后应优先切入对应 workspace，而不是把原始文本结果塞进 Task Picker 主区
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
- `?` 是 UI help shortcut，不是新的 parser 命令
- `/` 可作为进入 command bar 的快捷键；进入后仍只接受显式 slash command
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
