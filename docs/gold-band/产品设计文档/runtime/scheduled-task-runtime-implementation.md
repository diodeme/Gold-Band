# 定时任务运行时实现补充

## 当前基线

当前版本已经具备结构化 schedule、内容指纹、Composer 创建入口、全局管理页和 Direct/Workflow/AUTO 的基础 Task/Run 物化链路。现有 JSON definition、`lastTriggerAt` 游标和每秒全量轮询只作为迁移前基线，不能作为可靠性实现继续扩展。

## 目标运行时结构

调度器由四层组成：

1. scheduler repository：保存 `scheduled_jobs` 和 `scheduled_occurrences`，提供事务、唯一约束、claim、lease、heartbeat、finish、missed 和历史查询。
2. schedule service：为每个 job 设置独立 timer，负责启动恢复、系统唤醒检测、未来时间计算和调度定义更新后的重排。
3. queue policy：统一识别 active Task/Run/ACP 状态，执行 `skip_when_running` 或有限次 `retry_when_busy`。
4. execution adapter：接入 Direct/new、Direct/continuous、Workflow、AUTO，并要求执行链路发出带 occurrence ID 的真实完成事件。

### Phase 1 repository contract (2026-08-03)

`ScheduledTaskDatabase` 使用独立的 `GoldBandPaths::scheduler_db_path()`，与搜索数据库保持分离。数据库启用 WAL 和 busy timeout，维护单例 schema version、`scheduled_jobs` 与 `scheduled_occurrences`；occurrence 通过 `(job_id, scheduled_at, trigger_kind)` 唯一约束去重。claim、续租、终态回写和过期 lease 恢复均在事务中执行，并在写入条件中校验 owner 与 lease，且跨 SQLite 连接的竞争也只能产生一个 running owner。仓储只保存 `SCHEDULED_*` 错误码及结构化参数，不生成面向用户的错误文案。

### Phase 2 repository and time semantics (2026-08-03)

旧 `ScheduledTaskStore` 的 definition/trigger 可以按 definition 事务导入 SQLite；重复导入是 no-op，definition ID 或 `(job_id, scheduled_at, trigger_kind)` 冲突会返回类型化 migration error，Task/Run 链接和完整 content snapshot 保留在 definition/occurrence 数据中。默认时区通过系统 IANA 时区解析，Hourly 计算下一个本地整点；DST gap 跳过无效本地时间，DST overlap 按绝对时间选择第一个有效 occurrence。Every 只有在启用转换或 interval/unit 变化时重置 anchor。

### Scheduler contract baseline (2026-08-06)

`ScheduledErrorCode` 的迁移冲突、coordinator 不可用、sleep inhibitor 失败、通知失败和 Skill 校验失败使用稳定的 `SCHEDULED_*` wire code，并通过结构化 error/params 传递，不包含面向用户的错误文案。`src/scheduler/queue.rs` 是队列和 occurrence 保留策略的唯一来源：busy retry 为 30 秒、最多 3 次；late-fire grace 为 60 秒；终态 occurrence 默认保留 30 天，允许范围为 1 至 3650 天，每批删除 500 条。

## occurrence 生命周期

```text
pending
  -> running       原子 claim 成功并启动执行
  -> retrying      仅 busy 重试，续租下一次尝试
  -> succeeded     Task/Run/ACP 真实完成
  -> failed        执行错误或运行时 permission request
  -> skipped       active 冲突或重试上限
  -> missed        应用关闭/系统休眠造成的过期时间点
  -> attention_required  AskUserQuestion，Run 可恢复但 scheduler 释放 lease
```

`attention_required` 不会创建下一条同一问题的并发会话；用户回答后继续原 Run，完成时回写原 occurrence。Occurrence 的 claim 受 `(job_id, scheduled_at, trigger_kind)` 唯一约束保护。

## 无人值守策略

创建或更新时预检 Direct Agent 的 full-auto 能力。预检失败返回结构化错误码，不保存一个必然等待权限的定义。

运行时仍出现 permission request 时，写入 `failed + SCHEDULED_PERMISSION_REQUIRED`，保留 ACP 请求现场并发通知。出现 `AskUserQuestion` 时，写入 `attention_required + SCHEDULED_USER_INPUT_REQUIRED`，暂停 Run、释放 lease，并通知用户进入详情回答。

## Task 与 Run 生命周期

- Direct/new：每个 scheduled occurrence 物化新的 Task、Run 和 ACP session。
- Direct/continuous：复用关联 Task/session chain，但每个 prompt 仍绑定唯一 occurrence；没有可恢复链路时创建新的 Task chain。
- Workflow/AUTO：content fingerprint 未变化时复用 Task、每次新 Run；authoring 变化时下一次触发新建 Task。
- model、thought level 和 permission 变化不改变 content fingerprint；Direct Agent、workspace、instruction、附件和 Workflow/AUTO authoring 变化会重新物化 Task。

调度器不直接写 Run canonical status。它只创建/认领 occurrence，调用现有 Task/Run API，并消费带 occurrence ID 的完成事件。

## 恢复与错过执行

应用启动时恢复未过期 lease，回收已过期 lease 并重新计算未来时间。系统唤醒时显式检查已经过去的计划时间点，将其标记为 `missed`，不自动补跑；之后只安排下一个未来时间点。

手动“立即执行”创建 `trigger_kind = manual` 的 occurrence，不推进 job 的 `next_run_at`，并返回 occurrence、Task/Run 引用供详情页跳转。

## 管理视图数据

管理页不展示 `scheduled-UUID` 或调度定义版本号。ViewModel 返回 instruction 首行摘要、中文调度摘要、IANA 时区、下次执行时间、启停状态、最近 occurrence 状态、错误码映射、运行计数和重试计数。

页面提供启用/暂停、编辑、删除、立即执行和详情/历史入口。没有独立名称输入；标题始终由 instruction 首行摘要生成。

## 迁移

首次初始化 scheduler database 时扫描旧 JSON definition 和 trigger 文件并幂等导入。迁移成功后旧 JSON 不再被 runtime 读写；删除调度定义只删除调度数据和输入快照，不删除 Gold Band Task/Run/ACP 历史。

## 验收重点

- 两个 scheduler worker 同时处理同一时间点只允许一个 occurrence claim 成功。
- claim 后进程崩溃，lease 到期后可以恢复，不产生重复 scheduled occurrence。
- ACP 启动成功但之后失败时 occurrence 最终为 `failed`，不能提前为 `completed`。
- 重启/唤醒后的过去时间点为 `missed`，不会追赶补跑。
- permission request 和 AskUserQuestion 分别进入 `failed` 与 `attention_required`，并可从通知进入详情。
- run-now 不改变下一次计划时间；Direct、Workflow、AUTO 都使用同一 occurrence 规则。
- run-now 不改变下一次计划时间；Direct、Workflow、AUTO 都使用同一 occurrence 规则。

### Phase 3 runtime lifecycle implementation (2026-08-03)

The runtime now uses the per-workspace SQLite scheduler database as its source of truth. Legacy JSON is imported only when the database is empty. Startup recovers expired leases and marks past schedule points as `missed` without backfill.

Each scheduled point is created and claimed transactionally. The scheduler keeps the lease alive while the Task/Run/ACP work is active, and completion is written only from lifecycle events carrying the scheduled occurrence ID. `RunCompleted` and successful ACP turns become `succeeded`; failures become `failed`.

Permission and elicitation interventions terminate the scheduler occurrence immediately: permission maps to `failed + SCHEDULED_PERMISSION_REQUIRED`, while user input maps to `attention_required + SCHEDULED_USER_INPUT_REQUIRED`. The lease is released before the occurrence update event is emitted, so the existing notification path can direct the user to the resumable Run.

Direct/new, Direct/continuous, Workflow and AUTO execution adapters now propagate the occurrence origin through background App clones. Scheduled task create, update, enable/disable and delete commands synchronize their definitions with SQLite.
