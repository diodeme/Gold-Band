# 定时任务运行时实现补充

## 历史基线

统一运行时改造前已经具备结构化 schedule、内容指纹、Composer 创建入口、全局管理页和 Direct/Workflow/AUTO 的基础 Task/Run 物化链路。当时的 JSON definition、`lastTriggerAt` 游标和每秒全量轮询只作为迁移前基线，不能作为可靠性实现继续扩展；当前活跃路径已经由 SQLite 与 deadline coordinator 取代。

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

## 迁移

首次初始化 scheduler database 时扫描旧 JSON definition 和 trigger 文件并幂等导入。迁移成功后旧 JSON 不再被 runtime 读写；删除调度定义只删除调度数据和输入快照，不删除 Gold Band Task/Run/ACP 历史。

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

### Phase 5 workspace isolation and startup migration (2026-08-03)

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
