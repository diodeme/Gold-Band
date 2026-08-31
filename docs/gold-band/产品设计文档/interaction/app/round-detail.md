# Round 详情页（已废弃）

## 1. 当前结论

旧 workbench `RoundDetailPage` 已删除，不再作为 Gold Band 的执行详情入口。

Round、node、attempt、ACP timeline、artifact、attachment、日志与 Runtime 操作统一由会话模式的 `ConversationRunPage` 承载。产品只保留一套会话生命周期消费路径，避免 workbench 页面用不完整的 `runtimeStatus` 重新推导 composer 状态。

## 2. 当前入口

- 任务工作流页点击 Round：切换到 `conversation-run`，携带 `projectId / taskId / runId / roundId`。
- 系统干预通知：切换到 `conversation-run`，再按完整 session locator 选择目标 node/attempt。
- 会话 deep link：使用 `/chat/projects/:projectId/tasks/:taskId/runs/:runId/rounds/:roundId/...`。

旧 `/tasks/:taskId/runs/:runId/rounds/:roundId` workbench 路由不再解析为详情页，也不提供兼容页面或旁路状态。

## 3. 状态与数据边界

- `ConversationAttemptLifecycleVm` 是 composer 与 Runtime 展示的 canonical 输入。
- 全局 branch snapshot 只能缓存独立 revision 的 ACP/queue 事实和有界 timeline replay，不缓存 `runtimeDisplay`、`composer` 等派生投影。
- `runtime-abnormal` 是可继续的异常暂停：保留错误展示，但 `blockingError=false`，composer 输入保持可用。
- 旧 `get_round_detail` view model 可继续作为后端内部工作流图装配的实现细节；前端产品路径不得调用它加载执行详情。

## 4. 删除范围

- 删除 `web/src/pages/RoundDetailPage.tsx`。
- 删除 `TaskPage.round-detail`、对应 workbench route、页面状态与刷新分支。
- 工作流 Round 行和干预导航统一进入会话运行页。
- 不保留旧 UI、旧数据结构消费或 fallback。
