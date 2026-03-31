# Gold Band Console 状态与事件

## 1. 设计目标
Console 必须有独立、可测试的 UI 状态模型，但不能篡改 runtime 语义。

## 2. 顶层状态模型
当前版采用显式 screen 状态：

- `ConsoleState`
  - `screen`
  - `focus`
  - `input`
  - `history`
  - `message`
  - `autoRefreshEnabled`
  - `lastRefreshLabel`
  - `welcomeAction`
  - `taskList`
  - `taskIndex`
  - `workspace`

## 3. Screen 模型
- `Welcome`
- `TaskPicker`
- `Workspace`

screen 决定当前可见布局与可响应键位。

## 4. Workspace 状态模型
`WorkspaceState` 负责表达单个 task 的当前工作上下文，建议至少包括：
- `taskId`
- `taskSummary`
- `activeRunId`
- `selectedRoundId`
- `selection`
- `dagPositions`
- `dagColumn`
- `dagRow`
- `detailLevel`
- `detailItems`
- `detailIndex`
- `detailScroll`
- `commandView`

## 5. 选择模型

### 5.1 WorkspaceSelection
DAG 一级选择对象：
- `TaskOverview`
- `Node { nodeId }`

### 5.2 DetailSelection
详情区下钻对象：
- `RetryAction`
- `Attempt { attemptId }`
- `Artifact { attemptId, name }`
- `Attachment { attemptId, name }`

### 5.3 DetailLevel
详情区分层：
- `NodeHome`
- `AttemptItems { attemptId }`
- `Content`
- `CommandView`

## 6. UI 事件模型
UI 事件建议包括：
- `InputChanged`
- `CommandSubmitted`
- `SelectionChanged`
- `RefreshTick`
- `BackRequested`
- `DetailEntered`
- `DetailEscaped`

## 7. runtime 数据读取规则

### 7.1 canonical state
- `task.json`
- `run.json`
- `round.json`
- `node.json`
- canonical artifacts

### 7.2 observability state
- `run-progress.json`
- `events.jsonl`
- `progress.events.jsonl`
- `raw.stream.jsonl`
- `runtime.log`

## 8. 刷新模型
首版建议：
- 轮询优先
- TaskPicker 支持自动刷新 task summaries
- Workspace 刷新只更新 UI 读取结果，不改变 runtime canonical truth
- refresh 不能替代 canonical 状态确认

## 9. 事件边界
需要区分两类事件：
- runtime observability events：落盘到 `events.jsonl` / `progress.events.jsonl`
- console UI events：只存在于内存状态机中

两者不能混淆。

## 10. 视觉状态约束
- Welcome / TaskPicker / Workspace 必须是分离 screen，而不是旧 selection 的特例
- DAG 与 Detail 必须是分离 UI 区块
- Input 区为空时可渲染 placeholder / command hint，但不能改变其“只接受显式命令”的约束
- welcome / empty state 只服务于引导，不引入新的 runtime 语义

## 11. 一句话总结

> Console 需要自己的 screen-aware UI 状态机，但它只是 runtime 的读取与控制外壳；canonical state 来自 runtime，UI 事件只服务于展示与交互。
