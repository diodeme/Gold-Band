# Gold Band Console 信息架构

## 1. 设计目标
Console 需要在不引入自然语言交互的前提下，提供比 scriptable CLI 更强的 task 选择、workflow 浏览与节点级下钻体验。

## 2. 页面结构
当前版采用分级布局策略：
- `Full`：Header + Workflow DAG + Detail + Command Input + Footer
- `Compact`：仍保持 workflow-first，但 workspace 同时只突出一个主 pane，优先 DAG，`Tab` 在 DAG / Detail 间切换
- `TooSmall`：低于最小终端尺寸时仅显示 resize 提示与最小 footer

标准尺寸下采用四段式主布局：

```text
┌ Header ─ task / run / round / restore / validation / status ─────────────────────┐
├ Workflow DAG Canvas ─────────────────────────────────────────────────────────────┤
│ node cards + edge markers                                                        │
├ Detail Panel ────────────────────────────────────────────────────────────────────┤
│ node -> attempts -> artifact/attachments -> content                              │
├ Command Input ───────────────────────────────────────────────────────────────────┤
│ /task   /log   /config   /continue   /help                                       │
└ Footer ─ key hints ──────────────────────────────────────────────────────────────┘
```

## 3. 顶层 screen
Console 不再是单一页面，而是 3 个显式 screen：

### 3.1 Welcome
- 显示产品欢迎信息
- 显示两个入口：
  - 新增 task（本期占位）
  - 选择现有 task
- 不显示 command bar
- 不允许 Input 获得焦点

### 3.2 Task Picker
- 列出 `~/.gold-band/projects/{project-id}/tasks/task-*`
- 当前桌面端采用 refined 双栏布局：左侧为 task list pane，右侧为 preview/action pane
- 每个 task 展示：
  - task id
  - title
  - description
  - workflow 校验结果
  - latest/resumable run 摘要
- Task Picker 卡片内部需要维持稳定层级：
  - 边框与 shell：低权重 chrome
  - task id：主标题层
  - `[selected]` / marker：交互强调层
  - running / resumable / valid / invalid / missing：主状态语义
  - `reason:` + reason body：次级诊断信息
  - run hint：低权重 meta 层
- 当前桌面端实现中，单击卡片只更新右侧预览；双击或点击预览区主按钮才进入 workspace
- valid task 在预览区同时支持 `进入 Workspace` 与 `Start Task`
- invalid / missing workflow 的 task 不可进入 workspace，触发进入或启动时改为说明型 overlay
- 搜索/筛选后的键盘上下选择必须基于当前 filtered task 集合，而不是未过滤的原始 task 列表

### 3.3 Workspace
- 以单个 task 为上下文
- 当前桌面端采用 refined workspace：顶部 context strip + 左侧 DAG canvas + 右侧 inspector/content panel
- DAG canvas 是唯一主导航面，支持节点卡片、SVG edge、edge label、zoom、pan、reset
- 右侧 inspector 展示当前 node 的详情、actions、attempts 与 content tabs

## 4. Header
Header 持续展示当前 task 的核心上下文：
- task id
- title
- description
- active run
- resumable run
- workflow 校验结果

## 5. Workflow DAG Canvas
DAG 是 workspace 的唯一主导航面：
- 节点按拓扑深度分列，桌面端当前通过 `dagre` 生成 LR 布局
- 同层节点纵向排列，布局失败时可退回 node DTO 的 `column / row`
- 当前选中节点高亮，当前运行节点使用独立 active marker
- 节点状态、latest attempt、node type 附着在节点卡片上
- 边通过 SVG path 渲染，并在连线中段显示 label
- 边状态直接表达语义：
  - `success`：成功路径
  - `failure`：失败路径
  - `invalid`：无效/不可用路径
- 桌面端 canvas 支持 zoom / pan / reset；这些是 frontend local state，不参与 runtime truth

node 是一级选择对象；edge 不作为一级可选择对象。

## 6. Detail Panel
Detail Panel 是 DAG 的下级观察区，采用层级下钻：

### 6.1 Node Home
进入某个 node 后，详情区首先展示：
- retry 操作项
- attempts 列表
- outgoing transitions 摘要

### 6.2 Attempt Items
进入某个 attempt 后，展示：
- artifact 列表
- attachment 列表
- attempt 摘要（状态、开始/结束时间等）
- 当前日志源标识：`progress.events.jsonl` 或 `raw.stream.jsonl`
- 若该节点仍为当前运行节点，可默认 follow live 并持续刷新当前 attempt 日志

### 6.3 Content View
进入某个 artifact 或 attachment 后，展示具体内容。

当前桌面端首版已实现最小 split view：
- 左侧为 artifact / attachment 列表
- 右侧为当前选中项的内容预览
- 未切换选择时默认打开列表中的第一项
- viewer 会按内容做最小格式识别：JSON 走 pretty print，Markdown 走文本型 markdown viewer，其余按 plain text 展示

### 6.4 返回规则
- `Esc`：content -> attempt -> node -> DAG
- 不再保留旧的 `/back` 主交互地位

## 7. 命令输入区
命令输入区只接受显式命令。

进入方式：
- 在支持 command bar 的 screen/pane 中按 `/`
- 桌面端默认只显示 slim status bar，按 `/` 后展开 command palette
- 或通过 `Tab` 切到 Input

命令输入区只接受显式命令：
- `/task`
- `/log`
- `/config`
- `/theme [gold-band|nord|dracula|cyber|onyx|mist|high-contrast]`
- `/continue`
- `/help`
- 以及 runtime passthrough commands

不接受自然语言句子。

## 8. 导航原则
- task 选择和 workflow 浏览分层
- DAG 是唯一主导航
- attempt / artifact / attachment 是详情区下钻
- command bar 只负责少量全局辅助动作
- 焦点必须显式可见，避免用户误判当前键盘输入作用对象
- theme 层必须通过 semantic token 提供层级，而不是在 widget 渲染处散落硬编码颜色
- 当前只支持按名称加载内建主题，不支持外部主题文件

### 8.1 当前快捷键约定
- `?`：直接打开 help overlay
- `/`：从非 Input pane 进入 command bar
- `Tab`：在当前 screen 的关键 pane 间循环焦点；compact workspace 中用于 DAG / Detail 切换
- `↑ / ↓`：Welcome / TaskPicker / DAG / Detail 中移动当前光标，overlay 中滚动内容
- `← / →`：DAG 中在 column 间切换节点
- `Enter`：进入当前选择
- `Esc`：逐级返回，overlay 下关闭 overlay，Welcome 下退出 console

## 9. 一句话总结

> Console 的信息架构是“Welcome 先选 task，Workspace 再围绕 workflow DAG 导航，Detail 负责 node 下钻，Command Input 只承担少量全局动作”。
