# Gold Band Console 概览

## 1. 一句话定义
Gold Band Console 是 Gold Band 的 workflow-first runtime console。

它面向人工控制台操作，提供：
- Welcome -> Task Picker -> Workspace 三段式进入流程
- 以 workflow DAG 为中心的可视化导航
- node -> attempt -> artifact / attachment 的逐级下钻
- 少量 slash command 驱动的全局辅助能力，command bar 直接显示可匹配提示

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

欢迎页不显示 command bar，也不允许 Input 获得焦点；入口选择只通过主视觉区高亮项完成。

真正的运行观察与操作发生在选中 task 后的 workspace 内。

Task Picker 中的 task 以目录卡片方式展示，卡片宽度应随终端宽度自适应，并强调 task id、description、workflow 校验结果和可恢复 run。

Task Picker 的视觉层级应稳定区分：
- task id：主强调信息
- `[selected]` 与当前选择 marker：交互强调
- workflow valid / invalid / missing：主状态语义
- `reason:` label 与具体 reason：次一级诊断信息
- latest / resumable run hint：元信息层级

其中 invalid / missing task 需要在视觉上表现为“不可进入”，并在 Enter 时给出说明 overlay，而不是进入 workspace。

### 5.2 主导航围绕 workflow DAG
workspace 的核心不是 task/run/round/attempt 层级树，而是 workflow DAG：
- DAG 节点是一级导航对象
- 边直接在画布中显示语义：`✔ / ✘ / ?`，标签位于连线中段，拐点使用更圆润的终端字符
- 节点卡片采用轻量 terminal-card 风格；当前选中节点使用更强的高亮边框与暖色品牌色
- node 选中后进入详情区
- 详情区再逐级展示 attempt、artifact、attachment

### 5.3 命令仍是显式命令，不是自然语言
用户通过 slash command 驱动少量全局动作，例如：

当前 console theme 采用“命名内建主题 + semantic token”模式，当前支持：
- `gold-band`
- `nord`
- `dracula`
- `cyber`
- `onyx`
- `mist`
- `high-contrast`

CLI 可通过 `gold-band console --theme <name>` 选择主题；若用户目录存在 `~/.gold-band/config.json`，启动时按 `default -> user config -> CLI` 合并。console 内执行 `/theme <name>` 会立即切换当前 session，并同步持久化到用户配置。`NO_COLOR` 仍作为最终颜色覆盖层，但不会移除 bold / border type / emphasis 等层级信息。

同时提供四个键盘快捷入口：
- `?`：直接打开 help overlay
- `/`：从非 Input pane 直接进入 command bar
- `s`：在 Task Picker 中直接启动当前选中 task，并切入 workspace
- `l`：在 attempt detail 中切换 `progress.events.jsonl` / `raw.stream.jsonl`

当前桌面端首版已经落地的键盘路径：
- `?`：打开帮助 overlay
- `/`：打开 command bar
- `Tab`：在 workspace 的 `dag -> detail -> content` 间轮转焦点
- `Esc`：优先关闭 overlay，否则从 workspace 返回 Task Picker，再返回 Welcome
- 方向键：Task Picker 中切换 task；workspace 的 DAG 焦点中按列/行精确切换 node
- `Enter`：在 Task Picker 中进入当前选中 task
- `s`：在 Task Picker 中启动当前选中 task

```text
/task
/log
/config
/theme [gold-band|nord|dracula]
/continue
/help
```

runtime passthrough 命令仍可保留，例如：

```text
/run continue task-001 run-001
/artifact show task-001 run-001 --round round-001 --node dev --attempt attempt-001 --name 节点输出产物
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

其中 workspace 是 run 过程信息的主显示面：header 展示 run progress 摘要，node / attempt detail 优先展示 provider output 与最近事件；`/log` 继续作为 debug / 辅助观测 overlay，而不是 run 启动后的主承载区。

若 console 中通过 `s` 或 `/run start` 发起后台启动失败，workspace / task picker 需要直接展示失败原因，而不是只显示一个长期 pending 的 background 标记。

Attempt detail 中，`progress.events.jsonl` 应按“provider input snapshot”理解；实际 provider 输出历史由 `raw.stream.jsonl` 承载。Attempt detail 聚焦时，方向键应可滚动查看历史内容。

重新进入 task workspace 时，若当前 task 仍有 running/resumable run，workspace 应优先对齐到当前 runtime 的 round/node/attempt，而不是停留在旧的默认 DAG 节点。

但控制流结论仍必须来自：
- `run.json`
- `round.json`
- `node.json`
- canonical artifacts

## 6. 与其他入口的关系
- scriptable CLI：面向脚本、自动化、插件调用
- console CLI：面向人工控制台操作
- desktop frontend（Tauri + React）：在保留 runtime 语义的前提下复用同一套三屏信息架构与 slash command 模型
- VSCode 插件：在 CLI 之上做可视化封装

### 6.1 当前桌面端实现状态（2026-04-29）
当前仓库中的桌面前端已经从基础可运行骨架升级为 refined desktop shell：
- Welcome / Task Picker / Workspace 三屏已接通
- Welcome 默认不显示 command bar
- Task Picker 采用桌面双栏结构：左侧搜索/筛选/任务列表，右侧 task preview 与进入/启动动作
- Task Picker 明确区分 running / resumable / valid / invalid / missing，invalid / missing 可选中但不可进入 workspace
- Workspace 采用顶部 context、左侧 DAG canvas、右侧 inspector/content 的桌面布局
- DAG 主导航已从列式按钮升级为 `dagre` 布局、SVG edges、节点卡片、zoom / pan / reset 的 canvas
- Inspector 以 Node Summary / Actions / Attempts / Content Tabs 组织，content tabs 继续区分 canonical artifacts、attachments 与 observability logs
- command bar 已升级为 slash-only command palette，默认只显示底部状态条，按 `/` 后展开命令输入与 suggestions

这意味着本节的信息架构已经不只服务于终端 TUI，也同时作为桌面前端 refined UI 的交互基线。

## 7. 一句话总结

> Gold Band Console 是 Gold Band 的 workflow-first runtime console：先选 task，再看 workflow，再进 node 详情，命令只负责少量全局辅助动作。
