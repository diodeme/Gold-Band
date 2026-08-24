# 定时任务运行时实现补充

## 历史基线

统一运行时改造前已经具备结构化 schedule、内容指纹、Composer 创建入口、全局管理页和 Direct/Workflow/AUTO 的基础 Task/Run 物化链路。当时的 JSON definition、`lastTriggerAt` 游标和每秒全量轮询只作为迁移前基线，不能作为可靠性实现继续扩展；当前活跃路径已经由 SQLite 与 deadline coordinator 取代。

## 当前持久化契约（2026-08-19）

`scheduled_jobs` 与 `scheduled_occurrences` 统一存放在用户级 `~/.gold-band/core.db`。`GoldBandPaths::scheduler_db_path()` 直接复用 `core_db_path()`；Scheduler 通过 `core_schema(component = 'scheduler', version = 1)` 管理自己的表版本，与 `core`、`workspace_identity` 等 component 共用物理数据库但保持独立 schema ownership。

`project_id` 是应用内 workspace 的唯一业务身份。job 主键、occurrence 主键、外键、唯一约束、索引、repository API、coordinator key、heartbeat/active guard registry key 与 lifecycle event 都显式包含 `project_id`；`workspaceKey` 已废弃，workspace path 仅用于定位和归属校验。`scheduled_jobs` 使用 `(project_id, id)` 主键，`scheduled_occurrences` 使用 `(project_id, id)` 主键并以 `(project_id, job_id)` 外键级联到 job，计划点以 `(project_id, job_id, scheduled_at, trigger_kind)` 去重。

这是开发阶段的破坏式切换。旧项目级/用户级 `scheduled-tasks.db`、旧 JSON store、`scheduler_schema`、`scheduler_migrations` 以及所有 import/fallback 路径均已删除；runtime 不打开旧文件，也不迁移已有任务。下文 Phase 1 至 Phase 10.9 保留实施历史；与本节冲突的旧迁移或 per-workspace database 描述均由本节和 Phase 10.10 取代。

## 目标运行时结构

调度器由四层组成：

1. scheduler repository：保存 `scheduled_jobs` 和 `scheduled_occurrences`，提供事务、唯一约束、claim、lease、heartbeat、finish、missed 和历史查询。
2. schedule service：为每个 job 设置独立 timer，负责启动恢复、系统唤醒检测、未来时间计算和调度定义更新后的重排。
3. queue policy：统一识别 active Task/Run/ACP 状态，执行 `skip_when_running` 或有限次 `retry_when_busy`。
4. execution adapter：接入 Direct/new、Direct/continuous、Workflow、AUTO，并要求执行链路发出带 occurrence ID 的真实完成事件。

### Phase 1 repository contract (2026-08-03)

`ScheduledTaskDatabase` 通过 `GoldBandPaths::scheduler_db_path()` 打开用户级 `core.db`。数据库启用 WAL、foreign keys、`synchronous = FULL` 和 3 秒 busy timeout，使用 component-scoped schema version 管理 `scheduled_jobs` 与 `scheduled_occurrences`；occurrence 通过 `(project_id, job_id, scheduled_at, trigger_kind)` 唯一约束去重。claim、续租、终态回写和过期 lease 恢复均在事务中执行，并在写入条件中校验 `project_id`、owner 与 lease，且跨 SQLite 连接的竞争也只能产生一个 running owner。仓储只保存 `SCHEDULED_*` 错误码及结构化参数，不生成面向用户的错误文案。

### Phase 2 repository and time semantics (2026-08-03)

旧 `ScheduledTaskStore` 和导入接口已经删除，不再作为启动输入或冲突来源。Task/Run 链接和完整 content snapshot 保留在 definition/occurrence 数据中。默认时区通过系统 IANA 时区解析，Hourly 计算下一个本地整点；DST gap 跳过无效本地时间，DST overlap 按绝对时间选择第一个有效 occurrence。Every 只有在启用转换或 interval/unit 变化时重置 anchor。

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

`attention_required` 不会创建下一条同一问题的并发会话；用户回答后继续原 Run，完成时回写原 occurrence。Occurrence 的 claim 和去重均受完整 `project_id` 作用域保护。

## 无人值守策略

创建或更新时预检 Direct Agent 的 full-auto 能力。预检失败返回结构化错误码，不保存一个必然等待权限的定义。

运行时仍出现 permission request 时，写入 `failed + SCHEDULED_PERMISSION_REQUIRED`，保留 ACP 请求现场并发通知。出现 `AskUserQuestion` 时，写入 `attention_required + SCHEDULED_USER_INPUT_REQUIRED`，暂停 Run、释放 lease，并通知用户进入详情回答。

## Task 与 Run 生命周期

- Direct/new：每个 scheduled occurrence 物化新的 Task、Run 和 ACP session。
- Direct/continuous：复用关联 Task/session chain，但每个 prompt 仍绑定唯一 occurrence；没有可恢复链路时创建新的 Task chain。
- Direct 更新 instruction、附件或 session policy 时保留既有 `task_id`；Direct 的内容指纹变化不会单独触发 Task 重建。`New -> Continuous` 从既有链路继续，`Continuous -> New` 保留关联但由 New 策略在下一次触发时物化新的 Task。
- Workflow/AUTO：content fingerprint 未变化时复用 Task、每次新 Run；authoring 变化时下一次触发新建 Task。
- model、thought level 和 permission 变化不改变 content fingerprint；Direct Agent 与 workspace 在编辑入口中保持不可变，Workflow/AUTO authoring 变化会重新物化 Task。

Direct 与 Workflow/AUTO 之间切换属于执行模式边界，更新时会清除旧 `task_id`，避免跨模式复用不兼容的 Task 链路。

调度器不直接写 Run canonical status。它只创建/认领 occurrence，调用现有 Task/Run API，并消费带 occurrence ID 的完成事件。

## 恢复与错过执行

应用启动时恢复未过期 lease，回收已过期 lease 并重新计算未来时间。系统唤醒时显式检查已经过去的计划时间点：早于 `now - LATE_FIRE_GRACE` 的点写为 `missed`，grace 内的近迟到点仍可正常物化并执行；不会对更早的历史点自动补跑。

手动“立即执行”是创建成功后的独立显式操作。它会立即创建 `trigger_kind = manual` 的 occurrence 并进入与计划触发相同的 claim、queue 和 execution adapter 链路，不需要等待首次计划时间；它不推进 job 的 `next_run_at`，并返回 occurrence、Task/Run 引用供详情页跳转。

## 管理视图数据

管理页不展示 `scheduled-UUID` 或调度定义版本号。ViewModel 返回 instruction 首行摘要、中文调度摘要、IANA 时区、下次执行时间、启停状态、最近 occurrence 状态、错误码映射、运行计数和重试计数。

页面提供启用/暂停、编辑、删除、立即执行和详情/历史入口。没有独立名称输入；标题始终由 instruction 首行摘要生成。

## 破坏式切换

首次初始化只在 `core.db` 创建当前 scheduler component 和表，不扫描旧 JSON 或旧 SQLite。旧 `scheduled-tasks.db` 即使存在或损坏也不会被打开。删除调度定义只删除当前项目的调度数据和输入快照，不删除 Gold Band Task/Run/ACP 历史。

## 验收重点

- 两个 scheduler worker 同时处理同一时间点只允许一个 occurrence claim 成功。
- claim 后进程崩溃，lease 到期后可以恢复，不产生重复 scheduled occurrence。
- ACP 启动成功但之后失败时 occurrence 最终为 `failed`，不能提前为 `completed`。
- 重启/唤醒后的过去时间点为 `missed`，不会追赶补跑。
- permission request 和 AskUserQuestion 分别进入 `failed` 与 `attention_required`，并可从通知进入详情。
- run-now 不改变下一次计划时间；Direct、Workflow、AUTO 都使用同一 occurrence 规则。

### Phase 3 runtime lifecycle implementation (2026-08-03)

The execution runtime now reads per-workspace SQLite definitions and occurrences. At this phase, command-layer CRUD still synchronized legacy JSON with SQLite and therefore had not yet established a single authority. Legacy JSON import ran only when the database was empty. Startup recovers expired leases and marks past schedule points as `missed` without backfill.

Each scheduled point is created and claimed transactionally. The scheduler keeps the lease alive while the Task/Run/ACP work is active, and completion is written only from lifecycle events carrying the scheduled occurrence ID. `RunCompleted` and successful ACP turns become `succeeded`; failures become `failed`.

Permission and elicitation interventions terminate the scheduler occurrence immediately: permission maps to `failed + SCHEDULED_PERMISSION_REQUIRED`, while user input maps to `attention_required + SCHEDULED_USER_INPUT_REQUIRED`. The lease is released before the occurrence update event is emitted, so the existing notification path can direct the user to the resumable Run.

Direct/new, Direct/continuous, Workflow and AUTO execution adapters now propagate the occurrence origin through background App clones. Scheduled task create, update, enable/disable and delete commands synchronize their definitions with SQLite.

### Phase 4 frontend occurrence diagnostics (2026-08-03)

The web runtime facade now exposes occurrence history, diagnostics, manual run-now, and occurrence update subscriptions for both Tauri and browser preview runtimes. The browser preview keeps an in-memory occurrence history so the management surface exercises the same contract as the desktop app.

The scheduled-task management page provides a run-now action, a selected-task execution detail area, status-aware occurrence history, retry/run counters, next-run and last-error diagnostics, and live refresh when the backend emits an occurrence or scheduled-task update. `failed` and `attention_required` remain visible terminal states; they are not converted into indefinite loading or waiting UI states.

### Phase 5 workspace isolation and startup migration（历史，已由 Phase 10.10 取代）(2026-08-03)

The scheduler SQLite database is scoped to `GoldBandPaths::runtime_root` so each workspace has an independent definition and occurrence store. The scheduler loop queries only definitions whose `project_id` matches the current workspace, and the execution adapter rejects a mismatched definition before creating a Task, Run, or ACP session.

On startup, definitions and occurrence history from the former shared `scheduled-tasks.db` are copied idempotently into the matching workspace database. This migration does not move historical Task/Run/ACP files that were already materialized in the wrong workspace. Successful scheduled materialization now emits `scheduled-task-updated` after persisting the definition, so the conversation sidebar can show the newly created Task immediately while ACP initialization continues in the background.

### Phase 7 queue protection and lifecycle hardening (2026-08-04)

Queue protection treats a task as busy when any associated Run is `running` or `paused`, or when an ACP prompt is still active for the Run's current attempt. The prompt check covers normal attempts and persisted dynamic-node attempt directories, so a Direct/continuous prompt is not duplicated after the Run has already reached `completed` while the ACP turn is still waiting or executing. Manual run-now uses the same busy decision.

`RunPaused` is an intermediate lifecycle event. The scheduler keeps the occurrence active and its lease renewable until `InterventionRequested`, `RunCompleted`, or `AcpTurnFinished` finishes the occurrence. This preserves the `RunPaused -> InterventionRequested` event order and prevents an occurrence from being left in `running` without an owner.

User-initiated session termination does not disable the scheduled definition, reset its `task_id`, or create a replacement Task as part of this fix. Future occurrences are still evaluated by the existing Direct session and overlap policies; while the associated Run remains `paused`, `skip_when_running` skips them and `retry_when_busy` performs only its bounded retries. Resuming the Run or explicitly disabling/deleting the scheduled task is required to change that outcome.
### Phase 8 management surface UX separation and disabled-state suppression (2026-08-04)

The scheduled-task management page no longer reloads the full task list when a scheduled-task-updated event arrives. The backend enriches `ScheduledTaskUpdatedEventVm` with a full `ScheduledTaskVm` snapshot, so the frontend merges only the matching row in place. This eliminates the page-wide refresh that reset scroll position and selection state on every trigger.

Disabled tasks no longer expose a future `next_at`. Both `ScheduledTaskVm` and the diagnostics command return `next_at = null` when the definition is not enabled, so the management list and detail surface show "已停用" instead of a phantom next-run time.

Execution details (diagnostics, occurrence history, run-now, edit, enable/disable, delete) moved from the management page into a dedicated `ScheduledTaskDetailPage`. The detail route is deep-linkable at `/chat/scheduled-tasks/:id` and provides a back button to the management list. The management page keeps only the list, filtering, and quick actions (run-now, enable/disable, edit, delete, open detail).

### Phase 9 codex unattended gate, task_id preservation, and scheduled context injection (2026-08-05)

The unattended-mode gate in `scheduled_agent_unattended_error` now treats an empty ACP mode list as "provider-managed permissions" and skips the check. This allows Codex ACP and similar providers that manage permissions internally (no standard `mode` config option) to run scheduled Direct tasks without false rejection.

`set_scheduled_task_enabled` now loads the definition from the SQLite database (authoritative source) instead of the JSON store. The JSON store's `task_id` was always `None`, so the previous flow overwrote the DB's `task_id` back to `None` on every toggle, causing the next trigger to materialize a new Task instead of continuing the session. The DB-first approach preserves `task_id` and all other runtime fields.

Execution history now filters out `Skipped` and `Missed` occurrences so the detail page shows only genuine execution attempts. Error codes are translated to Chinese labels for user-facing display.

The scheduled runtime injects a `ScheduledTaskContextInfo` into the `App` clone at execution time. This carries task title, mode, session policy, trigger kind, trigger time, and instruction. The provider rendering pipeline renders this as a hidden context section (`scheduled_task_context.md` template in `src/prompts/`), so the agent is aware it is executing in a scheduled task environment and should work autonomously.

### Phase 10 unified scheduler hardening design (2026-08-05)

The SQLite repository becomes the only definition and occurrence authority. Create, read, update, enable/disable and delete commands must stop reading or writing the legacy JSON store. A durable migration marker replaces the previous "database is empty" heuristic so legacy JSON can be imported exactly once without becoming a runtime fallback.

The one-second workspace/job polling loop is replaced by a single scheduler coordinator backed by one independent deadline per enabled job. CRUD commits notify the coordinator to add, reset or remove a deadline. Startup, resume and overdue timer callbacks share one reconcile path for expired leases and missed schedule points. Stale callbacks re-read the persisted job version and become no-ops when the job changed or was disabled.

Direct/new, Direct/continuous, Workflow and AUTO continue to use separate execution adapters, but all adapters share occurrence claim, heartbeat, queue policy, missed recovery, notification and retention behavior. Busy retry count and delay come exclusively from `src/scheduler/queue.rs`.

A global keep-awake preference acquires one process-level system-sleep inhibitor only while at least one job is enabled. It allows display sleep and never changes an occurrence result when the platform inhibitor fails. Scheduled notifications extend the existing intervention/native notification pipeline and deep-link to the task detail, linked Run or pending question.

Task 6 implements that preference with `keepawake 0.6.0` and one `ScheduledPowerManager` owned by `DesktopState`. The activation predicate is exactly `keep_awake_enabled && enabled_job_count > 0 && app_is_running`; enabled counts are summed from every registered workspace after registration, CRUD changes, settings changes, and reconciliation. Shutdown reconciles with `app_is_running = false` before acknowledgement. The shared builder uses `display(false)`, `idle(true)`, and `sleep(false)`: Windows is backed by the System Power API, macOS by IOKit, and Linux by its inhibit backend. No platform branch launches an external command.

Persisted settings schema version 3 adds keep-awake, scheduled completion notifications, and occurrence retention days. The desktop commands return both configured and effective power state, enabled-job count, the retention value, and an optional stable power error code. Retention validation reuses the queue-domain `1..=3650` constants and returns `SCHEDULED_VALIDATION_FAILED` with structured parameters.

The detail API returns all occurrence statuses, including `skipped` and `missed`, plus Task/Run/session links. Terminal occurrences are retained for 30 days by default without deleting materialized execution history. Complete IANA timezone selection and visible-text i18n complete the management surface.

### Phase 10.1 SQLite repository and exactly-once migration (2026-08-06)

The repository schema is now version 2. `scheduled_jobs.revision` and `scheduled_jobs.next_run_at` are persisted with an enabled-deadline partial index, while `scheduler_migrations` records durable completion markers. Schema v1 upgrades inside one transaction without losing jobs or occurrences. That transaction parses every existing definition and derives missing enabled deadlines from `last_trigger_at`, or `created_at - 1s` when no trigger exists, through `ScheduleSpec::next_occurrence_after`. Legacy JSON and shared-database imports reuse the same helper when their deadline is null. Disabled jobs and completed one-shot schedules naturally remain unscheduled. A schema version newer than the binary is rejected before mutation.

Legacy JSON is read into a `LegacySchedulerSnapshot` before the destination write transaction begins. JSON definitions and triggers are imported in one immediate transaction, `legacy-json-v1` is inserted last, and a conflict rolls back both imported rows and the marker. The former shared SQLite database follows the same rule with `legacy-shared-db-v1`, copying only the current project. A completed marker remains authoritative even when the destination contains no jobs, so an intentionally empty database is not repopulated on restart.

Definition updates compare `project_id + id + expected updated_at` and increment `revision`. Because SQLite persists scheduler timestamps in milliseconds, an authoring update must advance `updated_at` by at least one millisecond; otherwise it is rejected instead of leaving a reusable optimistic token. A runtime projection transaction first checks `expected_revision`, then normalizes its copied definition to at least the current `updated_at + 1ms` and writes that exact value to both JSON and the SQL column. This invalidates any older authoring token while preserving `next_run_at`. Deadline callbacks compare the expected revision and atomically create or recover the scheduled occurrence at the stored deadline while advancing `next_run_at`; stale callbacks do nothing. Manual run-now creates a manual occurrence immediately and preserves the persisted deadline; saving execution metadata such as Task association or last result may increment `revision`.

Retention requires a strictly positive batch size and rejects zero before opening a transaction, preventing a maintenance loop from reporting `has_more` without progress. It deletes bounded batches of old `succeeded`, `failed`, `skipped`, and `missed` occurrences only. Attention-required records, nonterminal occurrences, and occurrences linked to caller-supplied active Run IDs are protected. Task/Run/ACP files are outside this transaction and are never deleted by scheduler retention.

Coordinator maintenance now invokes retention after startup registration and after occurrence processing or lifecycle completion. Each pass uses `RETENTION_DELETE_BATCH_SIZE`, yields to Tokio when `has_more` is true, and derives protected Run IDs from existing Task/Run state. Cleanup errors are logged with `SCHEDULED_STORAGE_FAILED` diagnostics and never rewrite the completed occurrence.

This phase established the repository contract. The following command-service and coordinator phases complete the active SQLite-only cutover; legacy JSON remains migration input only.

### Phase 10.2 deadline-driven coordinator (2026-08-06)

The desktop runtime now owns one process-level scheduler coordinator on Tauri's async runtime. A `tokio_util::time::DelayQueue` contains exactly one wakeup per enabled SQLite job, while `DesktopState` owns the command handle. Create, update, enable, disable and delete commits send explicit coordinator commands; workspace add/sync/remove sends register or unregister commands. App exit sends `Shutdown`, and system resume sends an explicit reconcile request. The former named scheduler thread, one-second workspace scan and `thread::sleep` loop are removed.

Timer registration and firing always re-read the persisted job. Each registration records the SQLite `revision` and planned `next_run_at`; if either changed before the callback is handled, the callback creates no occurrence and the persisted deadline is registered again. A due callback uses `materialize_due_occurrence`, so the scheduled occurrence and advancement of `next_run_at` remain one transaction. Creation itself remains persistence-only and cannot materialize a Task, Run or ACP session.

CRUD commands are invalidation signals, not authoritative state transitions inside the coordinator. `JobDisabled` and `JobDeleted` therefore re-read the final SQLite row just like create/update/enable: only a row that is still absent or disabled cancels the deadline. A stale disable/delete notification cannot cancel a job that a later durable write already re-enabled or recreated.

Workspace registration runs schema open/migration, expired-lease recovery and reconciliation. Both legacy import paths consult their durable `scheduler_migrations` marker before parsing legacy JSON or opening the former shared database. Once a marker is complete, corrupt obsolete source files are ignored rather than becoming a startup dependency. Pending or retrying work is selected by a direct indexed SQLite query for the oldest runnable occurrence; history-list limits cannot hide it behind newer terminal rows. Reconcile materializes points older than `LATE_FIRE_GRACE` only to mark them `missed`, but processes at most `MISSED_RECONCILE_BATCH_SIZE` points in one coordinator turn. It then returns to the command/deadline select; the persisted past `next_run_at` is immediately registered again, preserving progress without delaying heartbeat, unrelated deadlines, or shutdown acknowledgement. A point inside grace is left runnable.

Manual run-now is an explicit coordinator command with a oneshot reply. It re-reads SQLite, creates and starts a `manual` occurrence immediately, then refreshes the same persisted scheduled deadline. `next_run_at` must remain unchanged; successful execution metadata persistence may increment `revision`, after which the coordinator refreshes its registration from the latest record. Create remains persistence-only and invokes no model, Task, Run or ACP session.

All runtime definition projections now use `update_job_runtime_projection` with the `ScheduledJobRecord.revision` captured when the occurrence is claimed. Task association, running/result metadata, immediate failures and lifecycle completion therefore share one CAS rule. `Conflict` means a concurrent authoring update won and `NotFound` means the job was deleted; both are stale no-ops. Neither case may overwrite authoring fields or recreate a deleted job. Lifecycle completion loads the definition by `project_id + job_id` before applying its projection and never scans every job in the workspace.

The repository exposes a project-scoped recoverable-job view that unions enabled jobs with jobs owning `pending`, `retrying`, or `running` occurrences. Each coordinator entry chooses the earliest of the runnable retry time, the earliest running `lease_until`, and the enabled job's planned `next_run_at`. A disabled one-shot job can therefore still finish crash recovery. Deadline handling first recovers expired leases, then processes the resulting retry; a non-expired crash lease wakes exactly at `lease_until` rather than waiting for an unrelated future business deadline. Graceful shutdown releases occurrences still owned by this process as `retrying + SCHEDULED_LEASE_LOST`. Per-occurrence heartbeat guards remain Task 5 work; Task 4 continues to renew active leases through the existing process-level heartbeat.

Workspace registration is replace-on-success. A transient open, migration, or App construction failure keeps the prior registration and deadline set intact, and one deduplicated retry is scheduled with the named `WORKSPACE_REGISTRATION_RETRY_DELAY`. The retry is canceled logically when registration later succeeds or the workspace is unregistered.

The coordinator also compares process-level wall-clock progress with Tokio's monotonic clock at `CLOCK_DRIFT_CHECK_INTERVAL`. Only a difference greater than `CLOCK_DRIFT_TOLERANCE` triggers `ReconcileReason::TimerDrift`; the check does not enumerate jobs during normal ticks and does not restore fixed one-second polling. Paused-time command-loop tests cover registration plus `JobCreated`, real deadline expiry, SQLite materialization/processing/re-registration, lease expiry recovery, registration retry, shutdown release and wall-clock jumps.

The coordinator's event selection is intentionally unbiased. A control-command backlog may delay neither expired `DelayQueue` work nor lease heartbeat and wall-clock checks indefinitely. Busy scheduled occurrences use the single `scheduler::queue::decide_queue` policy source for retry count, retry delay, and skip behavior; runtime code does not duplicate those thresholds.

Workspace refresh is an in-memory replace-on-success transaction. The coordinator constructs and reconciles a candidate `WorkspaceRegistration` without replacing the active registration first. It snapshots that workspace's logical deadline records before reconciliation; an open, migration, query, missed-recovery, or occurrence-processing failure cancels candidate timers and reconstructs the prior deadline set with the original revision and `wake_at`. Only a complete reconcile commits the candidate registration. The deduplicated two-second retry follows the same rule, so partial cancellation can never become the active scheduler state.

Desktop exit uses a shutdown completion handshake. The first Tauri `ExitRequested` is prevented and atomically changes the exit phase from running to started. `SchedulerCoordinatorHandle` shares both the command sender and ownership of the unique coordinator task handle. Its `shutdown()` sends a oneshot acknowledgement request; the command loop releases every occurrence lease owned by the process before acknowledging, then exits, and `shutdown()` joins that task before returning. Reentrant exit requests remain prevented while shutdown is in progress. After completion, the phase becomes completed and one programmatic exit request is allowed through. The desktop therefore never assumes that a detached coordinator task completed merely because a fire-and-forget command was sent.

Active runtime occurrence creation is definition-bound. Manual run-now and missed-point recovery use project-scoped repository methods that verify a non-null definition row inside the same immediate SQLite transaction as the occurrence insert/update. A stale run-now record or a due callback racing with delete returns not found and performs no write. Placeholder `scheduled_jobs` rows are available only to isolated repository tests for low-level occurrence state transitions; production runtime paths cannot recreate a deleted job or leave ghost occurrences.

Deadline consumption is failure-atomic at the in-memory registry boundary. The coordinator retains the popped `RegisteredDeadline`; if database open, due materialization, occurrence processing, or post-processing refresh fails, it re-arms that same revision and scheduled deadline with `wake_at = now + DEADLINE_FAILURE_RETRY_DELAY`. The retry always re-reads SQLite, so a concurrent update, disable, or delete is resolved by the normal stale/deadline refresh rules without requiring an external CRUD or resume signal.

An occurrence claim is protected until execution handoff by one `ClaimToHandoffGuard` shared by manual and scheduled paths. The guard registers the active lease immediately after a successful claim. Any error before a real Task/Run/ACP execution accepts the occurrence removes the active entry and releases the owned occurrence as `retrying`; immediate terminal decisions explicitly finish and disarm it, while successful execution hands ownership to lifecycle completion. This closes every early-return branch without duplicating lease cleanup logic.

Lease shutdown is a lifecycle barrier rather than a best-effort cancellation. `stop()` publishes cancellation before returning its future, then waits for an in-flight heartbeat worker to leave. The worker checks that cancellation before renew, before release, and before state inspection; `Drop` requests cancellation but does not abort a blocking SQLite worker. Terminal, attention, failed-handoff, and shutdown paths therefore stop and await the guard before their durable occurrence transition or release. A successful execution handoff retains the guard until a real lifecycle terminal/intervention event arrives.

### Task 5 execution adapters and attention resume

`src-tauri/src/scheduled_runtime/execution.rs` defines the shared `ScheduledExecutionContext`, `ExecutionBinding`, and `ScheduledExecutionAdapter` contract. Four adapters are selected from the persisted definition: Direct/new materializes a new Task and Run, Direct/continuous continues the associated attempt chain, Workflow starts a new Run on an unchanged authoring Task, and AUTO follows the same Task/Run binding while preserving its authoring identity. Every adapter returns occurrence links only after the existing accept-then-launch boundary succeeds; the runtime never marks an occurrence succeeded at start time.

Queue decisions use the shared `ActiveExecution` classification for running, permission waiting, user-input waiting, and resumable paused Runs. Retry decisions use the claimed occurrence attempt, while the definition projection remains diagnostic. When an elicitation response arrives, the coordinator finds the matching `attention_required` occurrence by Task/Run/round/attempt, claims it, installs a fresh heartbeat guard, and only then allows the response file to unblock ACP. The resumed Run keeps the original occurrence id and lifecycle completion updates that same record.

Manual and planned occurrences both classify the associated Task/Run/ACP state as `Idle`, `Running`, `PermissionWaiting`, `WaitingForUserInput`, or `ResumablePaused` before calling the single `scheduler::queue::decide_queue` policy. ACP prompt activity remains `Running` even when the persisted Run has reached a completed state, preventing duplicate continuous-session prompts.

Shutdown acknowledgement carries `ScheduledServiceResult<()>`, not a bare completion signal. Lease-release failure is returned as a structured coordinator error, and `SchedulerCoordinatorHandle::shutdown()` still joins the stored coordinator task for successful, failed, or lost acknowledgements before returning. When release and join both fail, the release/ack error remains the primary result because it describes whether durable occurrence ownership was relinquished.

Clock drift uses a long-lived wall/monotonic baseline. Residuals below `CLOCK_DRIFT_TOLERANCE` accumulate across clock-check intervals instead of being erased at every sample; a negative wall-clock interval or accumulated residual above tolerance triggers one reconcile and resets the baseline. Equal wall and monotonic progress after that reset does not repeatedly reconcile. A detected drift also sets coordinator-owned pending state: only a successful `TimerDrift` reconcile clears it, while a failed reconcile is retried at the next `CLOCK_DRIFT_CHECK_INTERVAL` even when the clock has returned to normal progression. This preserves failure recovery without reintroducing job polling.

### Task 7 structured scheduled notifications

`src-tauri/src/scheduled_runtime/notification.rs` maps durable occurrence outcomes into the copy-free `gold-band://scheduled-notification` event. Completion respects `scheduled_completion_notifications_enabled`; failure and attention-required emit immediately; skipped and retrying remain history-only; missed points emit one aggregate event per reconcile batch. Repeated runtime/lifecycle observations are harmless because the native sender uses `scheduled:{occurrence_id}:{kind}` with the existing process-level `NotificationDedup`.

The Web hook localizes title and body from the `scheduled.notifications.*` trees and calls `send_scheduled_native_notification`. The command extends the existing Windows Toast / macOS and Linux notify-rust path, including the same `view:` and `dismiss:` action codec. Scheduled action payloads carry project/job/occurrence and optional Task/Run/Round/Attempt links. Failed and missed actions open scheduled detail; attention and completion prefer the linked Run and fall back to detail. Backend code contains no scheduled customer-facing copy.

### Task 8 management surface and typed Web contract

The Web contract now returns a typed `ScheduleSpec`, the original timezone, and RFC 3339 timestamps. The removed `scheduleLabel`, `timezoneLabel`, and `lastTriggerLabel` fields have no compatibility path; schedule and status labels are derived from the shared zh-CN/en i18n trees. The timezone picker uses `Intl.supportedValuesOf('timeZone')` with maintained `@vvo/tzdb` data as its fallback, always including UTC and the system timezone.

Occurrence history includes every durable status, including `skipped` and `missed`, and supports status filtering. Rows with Task/Run links can open the linked conversation; Round/Attempt identifiers are carried through the route and select the matching session after the Run loads. `ScheduledRuntimeSettings` remains the shared control implementation for keep-awake, completion notifications, and retention, but is mounted only on the Settings page so the management surface stays task-focused.

### Task 9 verification record (2026-08-07)

- `cargo test -p gold-band scheduler --offline` passed 86 scheduler/repository tests; `cargo test -p gold-band-desktop scheduled_runtime --offline` passed 77 runtime, adapter, power, notification, and retention tests.
- `cargo check -p gold-band-desktop --tests --offline`, `cargo fmt --all -- --check`, and `git diff --check` passed. The desktop check retains three pre-existing warnings outside this scheduled-task change.
- `npm run web:test` passed 91 files and 578 tests; `npm run web:build` passed with only the existing large-chunk warning.
- The running Web app was verified at desktop and 390px widths for the management page, shared Settings controls, responsive Shell, zh-CN/en layout, and IANA search for `Pacific/Apia`; the temporary dev server was stopped afterward without creating a persistent scheduled definition, Task, Run, or occurrence.
- The full `cargo test --workspace --no-fail-fast --offline` gate is not green: two unchanged `src-tauri/src/view_models.rs` tests failed. `timeline_permission_decision_replaces_pending_by_request_id` passed on isolated retry; `round_graph_connects_ai_dynamic_exit_to_next_workflow_node` completed its graph assertions and then failed because its legacy cleanup `remove_dir_all(...).unwrap()` found the temporary directory already absent. These results are recorded as unrelated residual failures, not as a full-suite pass.
- macOS remains a first-class Task 6 target, but this Windows verification is not macOS compile evidence. Release acceptance still requires a macOS CI or real-device desktop compile plus keep-awake enable/disable smoke test against the IOKit backend.

### Phase 10.3 mainline lifecycle merge contract (2026-08-11)

The scheduler and the conversation lifecycle share the existing Task/Run/ACP model, but scheduler ownership is scoped to one scheduler-originated prompt turn. `scheduled_occurrence_id` and `ScheduledTaskContextInfo` are present while that turn is dispatched and observed. For Direct/continuous, user-authored prompts received during the turn remain in the ordinary prompt queue; immediately before automatic dispatch of a queued user prompt, both scheduler fields are removed from the App clone. The later turn is therefore an ordinary conversation turn and cannot complete, notify for, or inherit prompt context from the occurrence that triggered the first turn. Workflow and AUTO continue to follow their Run lifecycle instead of using this Direct turn boundary.

User stop keeps the existing merge-time semantics. A scheduled Workflow stopped midway remains `Paused + ProcessInterrupted`; it is not rewritten to a cancelled occurrence and the runtime is not automatically resumed. Later triggers classify the associated execution as `ResumablePaused`, then follow the persisted overlap policy (`skip_when_running` or bounded `retry_when_busy`). Only an explicit user “continue workflow” action resumes runtime control. Redesigning stopped scheduled workflows as detached non-runtime conversations is intentionally deferred to a dedicated lifecycle change.

Tauri `RunEvent` has exactly one owner: `desktop_lifecycle::handle_run_event`. Exit cleanup first calls `SchedulerCoordinatorHandle::shutdown()`, which waits for durable lease release acknowledgement and joins the coordinator task, and only then stops ACP/runtime sessions and performs the remaining desktop cleanup. Reentrant exit requests are deduplicated by `DesktopLifecycleCoordinator`; the scheduler-specific parallel exit phase was removed. `RunEvent::Resumed` is forwarded from this same owner as `ReconcileReason::SystemResume`.

Lifecycle notification ownership is selected by `scheduled_occurrence_id`. Events without it use the ordinary conversation notification policy, including Direct prompt-queue batch suppression and one terminal batch notification. Events with it are excluded from ordinary notifications and use the structured scheduled-notification event only. Native-notification navigation is queued as an explicit `conversation` or `scheduled` target, deduplicated by key, and consumed after the main window is restored; scheduled copy remains frontend i18n data rather than backend customer-facing text.

Both branches previously used settings schema version 3 for independent additions. The merged schema is version 4. Migration from either v3 shape applies both scheduler defaults and managed-agent capability fields while preserving explicit values from the source branch.

### Phase 10.4 next_run_at ownership, single next_at source, and running-occurrence reconciliation (2026-08-12)

Three coupled defects around a scheduled task that stuck in `running` while `next_run_at` appeared to regress were fixed together.

**1. `next_run_at` has a single maintainer.** Editing or toggling a job no longer overwrites `next_run_at` unconditionally. `ScheduledTaskService::update` compares the incoming schedule against the prior one: when the schedule is unchanged it persists the existing `next_run_at` (so editing an instruction, attachment, or non-schedule field does not disturb the scheduler's already-advanced deadline); only a real schedule change (or enable/disable) recomputes the next run via `derived_next_run_at`. `derived_next_run_at` keeps its `last_trigger_at`/`created_at` baseline so one-shot `At` schedules and legacy imports retain their trigger point, while periodic schedules always advance forward — regression to a past point is now impossible because the covering edit path preserves the materialized value instead of recomputing from a stale `last_trigger_at`.

**2. Single "next run" data source.** The list VM previously rendered `next_at` from `schedule.next_occurrence_after(now)` (real-time recompute), while the diagnostics VM used `record.next_run_at` (persisted). They now share one source: both come from the persisted `next_run_at` on `ScheduledJobRecord`. `ScheduledTaskVm::from_definition[_in_workspace]` takes a `next_run_at` parameter; `service.list` returns records; the `scheduled-task-updated` event payload carries the record's `next_run_at`. The management list and the detail page therefore can no longer disagree on the next-run time — the previous "list shows 16:34, detail shows 16:31" split is gone.

**3. Active state reconciliation for stuck `running` occurrences (root-cause fix, not a max-duration cap).** An occurrence leaves `running` only on a terminal lifecycle event; the lease heartbeat proves the scheduler process is alive, not that the underlying Task/Run is still executing, so a lost terminal event left the occurrence stuck forever (only a process restart recovered it). `handle_registered_deadline` now runs an explicit reconciliation right after `recover_expired`: for each `running` occurrence of the job, `reconcile_running_occurrence_outcome` checks the real Task/Run state via `task_has_active_execution`/`run_status` — if the Task/Run is still active the occurrence is preserved (long-running tasks are never killed), if the Task/Run is completed/missing the occurrence is finalized to `succeeded`/`failed` and a warning is logged. This converges control-plane state to actual state on every trigger point, using the existing recovery primitive with corrected "still-active ⇒ keep" semantics. Supporting hardening: `launch_prepared_run_background` wraps `drive_from_node` in `catch_unwind` so a panic routes through `terminalize_background_drive_error` instead of silently dropping the terminal event, and `handle_lifecycle_event` logs the three previously-silent bail-outs (missing `scheduled_occurrence_id`, no registered active occurrence).

Acceptance tests were added for each point: `derived_next_run_at_for_every_schedule_always_yields_future_point`, `update_preserves_next_run_at_when_schedule_is_unchanged`, `scheduled_task_vm_next_at_uses_persisted_next_run_at_not_realtime_recompute`, `list_running_occurrences_for_job_returns_only_running`, and the three `reconcile_running_occurrence_*` cases (underlying run completed ⇒ succeeded, missing ⇒ failed, still active ⇒ preserved). The scheduler suite is green; two unrelated `view_models.rs`/`commands.rs` tests (`round_graph_connects_ai_dynamic_exit_to_next_workflow_node`, `accepted_stop_persists_control_state_without_reading_timeline`) fail identically on the pre-change tree and are residual Windows-temp-directory issues, not regressions from this change.

### Phase 10.5 re-enabling a paused job no longer back-fills missed occurrences or fires missed notifications (2026-08-12)

Re-enabling a paused Repeat/Cron job used to compute `next_run_at` via `derived_next_run_at`, whose baseline is `last_trigger_at` — a value frozen at the moment the job was paused. For Repeat/Cron this yielded a trigger point that fell inside the disabled window (in the past), so the coordinator immediately treated it as missed: `reconcile_missed_deadlines` materialized one `missed` occurrence per elapsed period (only bounded by `MISSED_RECONCILE_BATCH_SIZE = 50` and `LATE_FIRE_GRACE = 60s`) and `notify_missed` raised a "missed N times" notification. The `Every` kind happened to avoid this only because the enable branch reset its `anchor_at` to now. This affected any run mode whose users commonly pick Repeat/Cron schedules — including Workflow and AUTO.

`set_enabled` now schedules the next run from `now` for **every** schedule kind on the disabled→enabled transition (`definition.schedule.next_occurrence_after(now)`), instead of recomputing from the stale `last_trigger_at`. The disabled window's elapsed periods are deliberately skipped, consistent with the existing "missed points outside grace are not back-filled" contract, so re-enabling produces neither historical `missed` occurrences nor a missed notification. The special-case `Every` anchor reset was removed — `next_occurrence_after(now)` covers it. The disable path is unchanged (`derived_next_run_at` returns `None` for a disabled job).

Acceptance: `reenabling_repeat_job_schedules_next_run_from_now_not_from_stale_last_trigger` sets `last_trigger_at` 30 days in the past, disables, re-enables, and asserts `next_run_at >= now`. The scheduler suite stays green (108 bin tests).

### Phase 10.6 scheduler deadline precision and monotonic persistence (2026-08-13)

Scheduler timestamps use SQLite's millisecond precision as their persistence boundary. `Every` schedule arithmetic normalizes both `anchor_at` and the comparison deadline to milliseconds and must return a value whose persisted millisecond is strictly greater than the persisted comparison value. This prevents a nanosecond anchor from producing the same millisecond deadline after a database round trip.

The previous implementation divided the elapsed duration with `num_seconds()`. When an anchor such as `07:16:49.249706300Z` was paired with a persisted deadline `08:22:49.249Z`, the sub-millisecond difference was truncated and the algorithm returned the same logical occurrence. `materialize_due_occurrence` then incremented `revision` while persisting the unchanged deadline, and the coordinator immediately rearmed the past deadline. The resulting zero-delay loop generated about 65 writes per second, kept `next_run_at` stale, and caused unrelated CAS operations to lose repeatedly.

The root correction uses checked O(1) millisecond interval arithmetic and returns a millisecond-normalized timestamp. Two defenses remain at adjacent boundaries: identical `save_job_definition` calls are revision-idempotent, and a stale past coordinator registration with no runnable recovery work is rearmed no earlier than `DEADLINE_FAILURE_RETRY_DELAY`. A runnable occurrence left by a failed process attempt is exempt from that stale-business-deadline backoff and resumes at its existing retry wake-up, so the protection does not double the failure retry delay. Interface regressions cover the exact production precision mismatch, assert one materialization advances `08:22:49.249Z` to `08:25:49.249Z`, and assert a second call at the same `now` returns `NotDue` without another revision increment. The change adds no scans, polling, cache, or lock expansion; it removes the unbounded database-write and timer-rearm hot loop.

### Phase 10.7 Direct continuous terminal convergence (2026-08-13)

A Direct/continuous scheduled occurrence owns exactly the ACP turn that the scheduler submitted. The scheduler configures that App through the same `configure_conversation_runtime_callbacks` boundary as an ordinary conversation, rather than installing a partial list of live/session callbacks. Direct success intentionally defers `AcpTurnFinished` until prompt-queue settlement; the queue-drain callback therefore carries the originating App clone, including `scheduled_occurrence_id`, `ScheduledTaskContextInfo`, lifecycle bus, and prompt-turn callback. It must not reconstruct an App from `DesktopState`, because that would discard the execution origin before the terminal event is emitted.

When a queued user-authored turn is automatically dispatched after the scheduled turn, `without_scheduled_turn_context` removes only the scheduled occurrence and prompt context. The ordinary conversation callbacks are retained/reconfigured, so further queue draining continues while later turns cannot finish or notify for the scheduler-owned occurrence. The scheduled turn's `AcpTurnFinished` reaches the inline scheduler subscriber with its stable occurrence identity; the subscriber stops the per-occurrence guard before durably transitioning the occurrence to `succeeded` or `failed`.

Deadline reconciliation is a recovery channel, not the primary completion path. `CoordinatorRuntimeDriver for ScheduledRuntime` explicitly delegates `reconcile_running_occurrences` to the concrete runtime implementation; the trait default remains a no-op only for isolated coordinator test drivers. If a terminal event is lost, reconciliation compares each running occurrence with canonical Task/Run/ACP activity. A terminal reconciliation first removes and awaits the matching heartbeat guard, then performs the owner-scoped occurrence transition. This prevents a repaired history row from retaining an in-memory heartbeat worker.

Regression coverage fixes the three boundaries: the Direct drain App preserves its scheduled occurrence identity, an automatically dispatched user turn clears only scheduled origin while retaining the prompt lifecycle callback, and reconciliation removes/stops the active guard before the durable terminal state is observed. Runtime cost is constant per callback. Recovery queries remain scoped to one job's running occurrences at an existing deadline wake-up; no polling, full-table scan, N+1 workspace lookup, unbounded queue, or lock-range expansion is introduced.

### Phase 10.8 Review closure: exact reconciliation and occurrence pagination (2026-08-13)

The canonical locator for a running occurrence is `project_id + scheduled_task_id + occurrence_id + task_id + run_id`. Recovery may inspect only the Run and Attempt owned by that occurrence; another active Run under the same reused Task cannot keep an older occurrence running. Filesystem probes stay scoped to the target Run so coordinator deadlines do not grow with the Task's complete Run history.

Lifecycle completion and recovery reconciliation share one terminal convergence pipeline: stop and remove the heartbeat guard, owner-CAS the occurrence terminal state, update the definition projection, emit task and occurrence events, apply notification policy, and request bounded retention cleanup. `scheduled-task-updated.task.nextAt` always comes from persisted `ScheduledJobRecord.next_run_at`; non-delete CRUD events cannot synthesize a null deadline.

Occurrence history uses keyset pagination ordered by `scheduled_at DESC, created_at DESC, id DESC`. The opaque cursor encodes the final row's three sort fields. The API returns `{ items, nextCursor }`, fixes the page size at 20, and reads at most 21 rows to determine whether another page exists. New occurrences inserted while browsing cannot duplicate or skip rows in subsequent pages.

Status filtering is part of the history query contract and runs before pagination. Unfiltered and filtered queries use `(job_id, scheduled_at, created_at, id)` and `(job_id, status, scheduled_at, created_at, id)` indexes respectively; the implementation must not use an optional `OR` predicate that prevents the filtered index prefix from being selected. Diagnostics compute the exact run total with `COUNT(run_id IS NOT NULL)` and expose only the latest 20 occurrences, rather than loading a fixed 200-row sample and treating it as the total.

The scheduled runtime settings cache treats a save response as an authoritative mutation. Every cache write advances a generation and notifies subscribers; an earlier GET may commit only when that generation has not changed. Multiple saves are serialized and merge each patch into the latest server snapshot, while the retention field separately tracks server value, draft, and dirty state.

The settings cache owns a mutation generation. A background GET commits only when no later mutation has advanced the generation; a successful save response is authoritative. Retention input keeps separate server value, draft, and dirty state so background refresh never mixes fields from two server versions. These changes add no polling, full scans, unbounded cache, cross-workspace N+1, or expanded lock scope.

Acceptance completed on 2026-08-14: the full Web suite of 1,089 tests across 168 files passed, together with Rust formatting/check, TypeScript checking, the production Web build, and the scheduled runtime/command regressions. The occurrence query reads at most 21 rows per page, the filtered query is covered by an `EXPLAIN QUERY PLAN` assertion for `idx_scheduled_occurrences_status_history`, and diagnostics use an exact SQL count while loading only the latest 20 rows.

All resumable user interactions share one scheduler boundary. Before either `respond_elicitation` exposes an AskUserQuestion response to ACP or `submit_manual_check` advances a Workflow manual decision, the command resolves the exact Task/Run/Round/Attempt locator. The scheduler atomically changes that occurrence back to `running`, installs its heartbeat guard, advances the definition runtime projection through revision/CAS, emits the existing occurrence update contract, and returns the reclaimed occurrence ID. ManualCheck injects that identity into the App passed to its background continuation so later lifecycle events finish the same record. Both success and failure manual decisions follow this path.

ManualCheck validates that the Run is still paused on the requested current node and attempt before entering the scheduler boundary, then repeats the same domain validation in the background continuation. A stale or invalid request is therefore rejected before it can move the occurrence to `running`; the repeated validation also prevents a later concurrent state change from mutating the wrong Run attempt.

ManualCheck uses the same one-shot launch handshake and Run-scoped single-flight lease as RuntimeContinue. The command acquires the lease before asking scheduler to reclaim the occurrence, so two concurrent or opposite decisions cannot let an unbound second request win the background launch race. It does not acknowledge success merely because the background thread was spawned: it waits until the manual decision and the resulting control transition have both been durably persisted. This places the acknowledgement before any long-running provider execution but after the returned Run has reached its authoritative `Running`, `Completed`, or newly paused state. A validation, workflow-load, persistence, thread-start, or panic failure before that acknowledgement first converges the Run to `Paused + RuntimeAbnormal` and emits the lifecycle event with the reclaimed occurrence ID, then returns structured `runtime.continue-launch-failed`. After acknowledgement, failure convergence carries the exact Run/Round/Node/Attempt execution ID and updates state only while that execution still owns the active attempt; a later provider callback, user action, or lifecycle transition cannot be overwritten by the stale failure.

Pre-acknowledgement failure convergence is also identity-guarded. Under the attempt lock it may change canonical state only while the original locator still identifies the paused, `manual_check_pending` attempt. A stop, retry, recovery, or other control transition that has replaced that attempt makes the failure `Superseded`: the replacement state is left untouched, while the scheduler-owned occurrence still receives a failure lifecycle event for the original locator so its heartbeat cannot remain `running`.

Attempt failure convergence distinguishes `Converged`, `Superseded`, and persistence failure. Only a proven execution/locator mismatch suppresses a stale post-acknowledgement event. An active legacy attempt without an execution ID is accepted only when the exact locator is still running and the durable Node also has no execution ID; it never falls back to an unconditional write. A lock, read, validation, or write failure is not treated as supersession; the runtime re-reads the exact locator, and if the original execution still owns it, the ownership is indeterminate, or the Run was already partially written as `Paused + RuntimeAbnormal`, it emits the terminal lifecycle event with the persistence error attached. Re-entering convergence for that exact partially paused Run also repairs the missing event. This preserves occurrence convergence when Run persistence succeeds but Round or Node persistence fails, without allowing an old execution to pause a newer one.

A missing scheduler database remains a no-I/O fast path for ordinary non-scheduled interactions. Once an `attention_required` record has been identified, storage, coordinator, workspace registration, reclaim, or race failures return structured scheduler codes and do not write the response signal or submit the manual decision. The resumed Run later converges the original occurrence to `succeeded` or `failed`. Each scheduled interaction adds one exact `LIMIT 1` lookup, one owner-scoped claim, one revision/CAS projection update, and one incremental event; it adds no polling, history scan, N+1 request, unbounded state, or wider lock scope.

The extra locator re-read occurs only when failure convergence itself fails. Normal execution and normal failure convergence keep the existing constant number of exact file operations and the same attempt-scoped lock; provider work remains outside the lock. Final backend verification covers the ManualCheck and background-drive races, 95 scheduler core tests, all 389 desktop tests, desktop compilation, formatting, and diff integrity. The broader core library completed 742 of 757 tests; its 15 residual failures remain confined to the pre-existing shared profile directory, Windows temp-path, Git subprocess/worktree isolation, and parallel JSONL groups.

### Phase 10.9 schema v3 与 deadline 转换收敛（2026-08-17）

Scheduler repository schema 升级到 v3。新数据库直接创建带 `revision`、`next_run_at`、migration marker 和最终 history/status-history 复合索引的 v3 结构；已有 v1 在同一事务内依次完成 v1→v2→v3，已有 v2 只在 v2→v3 migration 中重建两条历史索引。打开当前 v3 数据库只读取 `scheduler_schema.version` 并立即返回，不开启 immediate 写事务，也不执行 `DROP/CREATE INDEX`。因此正常列表、详情和操作路径的数据库 open 不再随 occurrence 历史量产生 O(N) 索引重建、SQLite 写锁或 busy timeout 风险；版本高于当前 binary 时仍在任何 mutation 前拒绝。

`next_run_at` 的 authoring 转换使用以下唯一规则：非 schedule 字段编辑保留 SQLite 已推进的 deadline；启用任务真实修改 schedule 时使用 `schedule.next_occurrence_after(now)`，停用任务修改 schedule 时保持 `None`；disabled→enabled 同样从 `now` 计算，enabled→disabled 写入 `None`；enabled→enabled 与 disabled→disabled 是幂等读取，直接返回当前 record，不增加 revision，也不发送 coordinator command。`derived_next_run_at` 继续只服务创建、legacy backfill 等需要按历史游标恢复连续性的路径，不再用于用户修改 schedule 或同状态启停。

这一设计没有新增 scheduler 状态、缓存或后台队列。schema migration 复用既有版本表和事务；deadline 复用 `ScheduleSpec::next_occurrence_after` 与现有 CAS record。回归固定 v1 数据保留、v2 索引一次性升级、v3 重开 `PRAGMA schema_version` 不变化、Repeat/Cron/Every 在 stale `last_trigger_at` 下仍得到未来 deadline，以及同状态启停不改 record/coordinator command。

### Phase 10.10 Scheduler 并入 core.db 与 canonical project identity（2026-08-19）

Scheduler 存储由“每 workspace 一个数据库”破坏式收敛为用户级 `core.db`。物理隔离不再承担 workspace identity 职责；所有 repository 和 runtime 调用必须显式传递 canonical `project_id`，SQL 主键、外键、唯一约束和查询索引统一以它作为第一列。definition JSON 的 `projectId` 与 SQL 行作用域不一致时拒绝读取，防止损坏数据绕过边界。两个项目允许复用相同 job ID 或 occurrence ID，读取、claim、lease、恢复、通知、保留清理和删除仍严格隔离。

旧项目级/用户级 `scheduled-tasks.db` 和 JSON store 不迁移、不导入、不读取、不 fallback；相应 path helper、migration marker、schema upgrade 和 startup import 代码已删除。新 scheduler schema 直接登记在共享 `core_schema` 的独立 `scheduler` component 下，因此可以与 Runtime recovery 表共存而不耦合两者版本。初始化策略与 `core.db` 对齐为 WAL、foreign keys、`synchronous = FULL` 和 3 秒 busy timeout。

性能上，enabled deadline、active occurrence、history 和 status history 索引都以 `project_id` 开头；正常协调、恢复、分页和 retention 查询只访问单项目范围，不因共享物理库引入跨项目全表扫描、N+1、额外缓存或扩大锁范围。方案复用既有 `core.db`、`core_schema`、事务和 coordinator，没有新增聚合、状态机、队列或迁移抽象，复杂度与当前开发阶段的数据规模和风险匹配。

定向验收覆盖 repository、core-state 共库、storage path、runtime/lease/lifecycle、service CRUD 和 attention lookup：45 + 6 + 1 + 87 + 26 + 2 项测试全部通过，core library 与 desktop 均编译通过，diff integrity 通过。按开发约定未执行全量回归、前端构建或 UI/EXE 启动验证。
