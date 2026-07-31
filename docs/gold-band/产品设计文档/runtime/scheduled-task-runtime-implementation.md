# 定时任务运行时实现补充

## 管理视图数据

管理页面不展示 `scheduled-UUID` 或调度定义版本号。ViewModel 返回任务内容首行摘要、中文调度摘要、中文时区、下次执行时间、调度状态和最近触发状态；内部 ID 只用于启停操作。

所有 wall-clock 时间按任务时区格式化，默认时区为中国（上海）。`version` 仅表示 `scheduled-task.json` 的数据格式版本，不是用户可见的任务版本。

## Task 与 Run 生命周期

创建定时任务时只保存定时任务定义、指令、执行配置和附件快照，不创建 Gold Band `Task`，也不启动 `Run`。第一次到点且通过队列保护后，调度器才物化正式 `Task` 并启动 `Run`，随后将 `taskId` 写回定时任务定义。

- Workflow/AUTO：内容未变时复用该 Task，每次到点创建新的 Run。
- Direct new：每次到点物化新的 Task、Run 和 ACP 会话。
- Direct continuous：复用该 Task、Run、Round 和可恢复的 ACP attempt 发送新的 prompt；没有可恢复 attempt 时才物化新的 Task 链。
- 修改 instruction、附件或 workflow/AUTO 内容时创建新的 Task；模型、强度、权限变化不改变 Task。

## 后台调度器

Tauri 启动单例轮询器，每秒扫描当前及已登记工作区的 enabled 定义。调度器先检查队列保护，再调用现有 Task/Run/ACP 创建或继续接口；它不直接写入 run canonical 状态。每次触发都会写入不可变 trigger record，并更新定义的最近触发状态。当前版本不补跑错过的时间点，也不生成 `missed` trigger。

## 全局管理与刷新

- `list_scheduled_tasks(null)` 聚合所有已登记工作区的定时任务，并为每条任务返回 `workspaceName`。
- 管理页使用全局扁平列表，可按工作区筛选；启停操作使用任务自身的 `projectId`。
- 创建、启停以及调度成功、失败、跳过、重试后的状态保存都会发布 `gold-band://scheduled-task-updated`。
- App 根层订阅该事件刷新左侧会话列表；管理页不订阅该事件，避免后台调度打断筛选和当前操作。

Tauri 启动时启动单例后台轮询器，每秒扫描当前及已配置工作区的启用任务。轮询使用 `lastTriggerAt` 作为幂等游标，任务触发后写入触发状态，应用重启不会重复执行同一个时间点。

- 单次任务成功触发后自动停用。
- 队列保护在同一任务存在活动 Run 时生效；跳过策略记录 `skipped`，重试策略按 30 秒间隔最多重试 3 次。
