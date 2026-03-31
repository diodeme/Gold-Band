# Gold Band Console 信息架构

## 1. 设计目标
Console 需要在不引入自然语言交互的前提下，提供比 scriptable CLI 更强的 task 选择、workflow 浏览与节点级下钻体验。

## 2. 页面结构
当前版采用四段式主布局：

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

### 3.2 Task Picker
- 列出 `.gold-band/tasks/task-*`
- 每个 task 展示：
  - task id
  - title
  - description
  - workflow 校验结果
  - latest/resumable run 摘要

### 3.3 Workspace
- 以单个 task 为上下文
- 上方展示 workflow DAG
- 下方展示当前 node 的详情与下钻内容

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
- 节点按拓扑深度分列
- 同层节点纵向排列
- 当前选中节点高亮
- 节点状态可附着在节点卡片上
- 边上直接标记：
  - `√` = success
  - `×` = failure
  - `？` = invalid

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

### 6.3 Content View
进入某个 artifact 或 attachment 后，展示具体内容。

### 6.4 返回规则
- `Esc`：content -> attempt -> node -> DAG
- 不再保留旧的 `/back` 主交互地位

## 7. 命令输入区
命令输入区只接受显式命令：
- `/task`
- `/log`
- `/config`
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

### 8.1 当前快捷键约定
- `Tab`：在当前 screen 的关键 pane 间循环焦点
- `↑ / ↓`：Welcome / TaskPicker / DAG / Detail 中移动当前光标
- `← / →`：DAG 中在 column 间切换节点
- `Enter`：进入当前选择
- `Esc`：逐级返回，Welcome 下退出 console

## 9. 一句话总结

> Console 的信息架构是“Welcome 先选 task，Workspace 再围绕 workflow DAG 导航，Detail 负责 node 下钻，Command Input 只承担少量全局动作”。
