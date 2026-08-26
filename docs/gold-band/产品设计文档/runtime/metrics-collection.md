# WB 会话指标采集与批量上报设计

> 状态：2026-08-26 客户端有限投递合同已完成；服务端待外部仓库实现
>
> 适用接口：`POST /api/client-report/metrics/batch`
>
> 服务端处理合同：[会话指标上报服务端处理技术方案](./metrics-server-processing.md)

## 1. 目标与边界

Gold Band 对 Direct、Workflow、AUTO 的 lifecycle 事件使用统一的 Task 事件流、Runtime locator 和统计快照合同：

1. `(projectId, executionId)` 唯一定位一个 Task 指标事件流；`eventRevision` 在 Task 全生命周期持久、严格递增。
2. `runId/roundId/nodeId/attemptId` 定位事件实际描述的 Run、Round 和 attempt；事件主体与 `executionKind` 不得矛盾。
3. attempt terminal 携带当前 attempt counters；task delivery terminal 携带 Task 累计 counters 和代码变更统计。
4. Task 创建来源与本次 execution 触发来源分别冻结并逐事件携带。
5. 已被 collector 接受的事实通过 SQLite transactional outbox 有限投递；单条事件总计最多发起 3 次 HTTP attempt，网络与数据库不阻塞 Runtime 热路径。

能力只在 `wb` 渠道、合法 endpoint 和 API Key 同时可用时启用。指标系统只观察 runtime，不参与业务控制；采集失败不得改变 Task、Run、Round、Attempt 或 ACP 状态。

本合同直接替换旧的 run/attempt 局部 revision、内存 reporter、delivery-only counters 和不上传 Run/Round 的协议，不双写、不 fallback。

## 2. 根因与设计选择

旧实现把 Task、Run 和 Attempt 的 identity、sequence 与 aggregate 混在 `ExecutionObservabilityState` 中：

- terminal 后释放内存 state，新 Run、follow-up 或进程恢复会重用 revision。
- Workflow/AUTO 中间态以 `run/outer-run` 为主体，同时夹带 node/unit 字段。
- task counters 只能得到当前 run 的快照，attempt counters 无法下钻。
- 发送队列和有限 HTTP 重试在进程退出、断网或 5xx 后会丢事件。
- 终态缺少 task 代码变更归因，只能错误地扫描 workspace 或 Git。

因此引入一个单消费者 `MetricsCollector`，复用项目已有 `RuntimeLifecycleBus`、Tokio 有界 MPSC、`rusqlite`、`reqwest`、`TurnFileStore` 和 `similar::TextDiff`：

```text
Runtime producer
  -> bounded try_send(PendingMetricsFact)
  -> MetricsCollector
  -> one SQLite transaction:
       load/update TaskMetricsState
       allocate eventRevision
       apply typed transition
       validate/build immutable wire event
       insert metrics_outbox
  -> BatchUploader claim/send/ack
  -> POST /api/client-report/metrics/batch
```

不引入 OpenTelemetry 业务事件总线、外部消息队列、并发 SQLite writer、无界缓存或服务端空壳。

## 3. 数据归属与事实源

| 数据 | 权威来源 | 生命周期 | Wire |
|---|---|---|---|
| `projectId` | `GoldBandPaths.project_id` | Project 稳定 | 每事件必填 |
| `executionId` | `TaskState.uuid` | Task 稳定 | 每事件必填 |
| `runId` | `RunState.id` | 当前 Run | 每事件必填 |
| `roundId` | `RoundState.id` | 当前 Round | 每事件必填 |
| `nodeId/attemptId` | durable node/unit state | 当前 attempt | attempt 事件必填 |
| `eventRevision` | SQLite `TaskMetricsState.last_revision` | 跨 Run/Round/attempt/重启 | 每事件必填 |
| Task 来源 | Conversation authoring metadata | Task 创建后冻结 | 每事件必填 |
| execution 触发来源 | scheduler occurrence 或用户动作快照 | 当前 Run/Turn | 每事件必填 |
| attempt counters | `TaskMetricsState.active_attempts` | attempt started 至 terminal | attempt terminal |
| task counters | `TaskMetricsState.task_counters` | Task 创建至当前 delivery terminal | delivery terminal |
| 代码变更 | Run 创建时冻结的工作区路径与 Git 基线、delivery terminal 的最终 Git 快照 | Run 创建至 delivery terminal | delivery terminal |

```rust
struct TaskMetricsKey {
    project_id: String,
    execution_id: String,
}

struct AttemptMetricsKey {
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
}
```

`workspace` 只用于诊断与展示，不得代替 `projectId` 参与 state、唯一约束或 JOIN。服务端不得按末级 `runId/roundId/nodeId/attemptId` 全局反查。

## 4. Producer 输入合同

producer 只提交不可变领域事实，不生成 `eventId/eventRevision/reportedAt`，也不维护 task counters：

```rust
struct PendingMetricsFact {
    key: TaskMetricsKey,
    event_type: LifecycleEventType,
    occurred_at: String,
    user_id: String,
    workspace: String,
    session_mode: MetricsSessionMode,
    subject: MetricsSubject,
    runtime_locator: MetricsRuntimeLocator,
    task_origin: MetricsTaskOrigin,
    execution_trigger: MetricsExecutionTrigger,
    transition: MetricsTransition,
    payload: MetricsPayload,
}
```

`MetricsSubject` 是 tagged enum：`DirectTurn`、`WorkflowRun`、`WorkflowNodeAttempt`、`AutoOuterRun`、`AutoUnitAttempt`。`executionKind` 只能由 subject 映射，producer 不接受裸 kind 与平行 node 字段。

`MetricsTransition` 只表达 collector 需要累加的受控动作：pause、带 `UserExecutionAction` 的 resume、permission/elicitation request、follow-up 或 none。重复 request ID、重复 action ID 和未产生状态转换的命令必须幂等。

## 5. Wire 合同

公共字段：

| 字段 | 规则 |
|---|---|
| `eventId` | collector 创建的 UUID；outbox 与服务端幂等键 |
| `eventRevision` | `(projectId, executionId)` 内严格递增，允许缺口，不重复、不回退 |
| `occurredAt` | 领域事实发生时间 |
| `reportedAt` | collector 接受并创建 immutable wire event 的时间；重试不变 |
| `projectId` | Project canonical identity |
| `executionId` | Task UUID；同一 Task 全生命周期不变 |
| `runId/roundId` | Runtime 本地 `run-NNN/round-NNN`；所有 lifecycle event 必填 |
| `workspace/userId/clientVersion` | 在事实创建时冻结 |
| `taskOrigin/executionTrigger` | 联合类型；逐事件携带 |

`occurredAt/reportedAt/timing.startedAt/timing.endedAt` 在 metrics wire 边界统一转换为无时区本地毫秒字符串 `YYYY-MM-DDTHH:mm:ss.SSS`。runtime 内部允许继续读取历史 `<unix-seconds>Z` 或 RFC 3339 值，但不得把这些格式直接透传到 wire；四个字段使用同一个 formatter。该合同按产品要求不携带 `Z` 或 offset，服务端不得把它们当作带时区 RFC 3339 字符串解析。可选的 `timing` 无法解析时省略整个 `timing` 并记录结构化诊断，不得因此丢弃 terminal 事件。

`eventRevision` 表示 collector 接受顺序，不表示 `occurredAt` 时间顺序。Task delivery terminal 不是事件流吸收态，同一 Task 后续 `run-002` 或 follow-up 继续使用更高 revision。

### 5.1 事件主体矩阵

| sessionMode | executionKind | 必填主体字段 |
|---|---|---|
| Direct | `turn` | `attemptId/attemptIndex` |
| Workflow delivery | `run` | 无 attempt 字段 |
| Workflow attempt | `node-attempt` | `nodeId/attemptId/attemptIndex/roundIndex/roleName` |
| AUTO delivery | `outer-run` | 无 attempt 字段 |
| AUTO attempt | `unit-attempt` | `nodeId/attemptId/attemptIndex/roundIndex/roleName/unitKind` |

Workflow/AUTO 的 paused/resumed/intervention 必须分别使用 `node-attempt/unit-attempt`。找不到 durable active attempt locator 时不构造事件，记录 `METRICS_ACTIVE_ATTEMPT_MISSING`，业务恢复继续。

### 5.2 来源与定时触发快照

```json
{
  "taskOrigin": "scheduled",
  "executionTrigger": {
    "type": "cron",
    "scheduledTaskId": "scheduled-task-001",
    "scheduledOccurrenceId": "occurrence-001",
    "scheduledAt": "2026-08-20T10:00:00.000",
    "expression": "0 0 10 * * MON-FRI",
    "timezone": "Asia/Shanghai"
  }
}
```

- `taskOrigin` 只允许字符串 `user/scheduled`；用户来源不输出 `executionTrigger`。
- scheduled 的 `executionTrigger.type` 只允许 `once/repeat/cron`，并始终携带 `scheduledTaskId/scheduledOccurrenceId/scheduledAt/timezone`。
- `ScheduleKind::At` 映射 `once`；`Every/Repeat` 映射 `repeat`，通过 `repeatKind` 携带 interval/hourly/daily/weekdays/weekly 及对应 value/unit/anchorAt/hour/minute/weekdays；`Cron` 映射 `cron` 并携带 expression。
- trigger 必须在 occurrence 创建时从 `ScheduledTaskDefinition.schedule` 冻结，后续编辑定时任务不得改写已经开始的 Run/Turn。
- 删除 metrics wire 的 `triggerKind/sessionPolicy`，不上传 instruction、标题或其他任务内容。

## 6. Counters

| 事件 | counters scope | 规则 |
|---|---|---|
| Workflow node `execution.completed` | 当前 attempt | 必填 |
| AUTO unit `execution.completed` | 当前 attempt | 必填 |
| Direct turn `execution.completed` | 当前 Task 累计 | 必填 |
| Workflow run `execution.completed` | 当前 Task 累计 | 必填 |
| AUTO outer run `execution.completed` | 当前 Task 累计 | 必填 |
| started/paused/resumed/intervention/acceptance | 无 | 禁止 |

六项 counters：`pauseCount/resumeCount/permissionRequestCount/elicitationCount/manualContinueCount/followUpCount`，均为非负整数。

```rust
enum UserExecutionAction {
    ManualContinue,
    PermissionResponse,
    ElicitationResponse,
    FollowUp,
    AutomaticRecovery,
}
```

- ManualContinue：真实 paused -> running 时 `resume + 1`、`manualContinue + 1`。
- Permission/Elicitation response：真实恢复时只 `resume + 1`。
- FollowUp：Workflow/AUTO 在 paused attempt 接受一次非空用户输入时，当前 attempt 与 Task 各 `followUp + 1`；Direct 后续用户 prompt 沿用相同 actionId 幂等口径。
- 同一次“带内容人工继续”同时产生 `resume + 1`、`manualContinue + 1`、`followUp + 1`；纯继续、权限/elicitation 回答和自动恢复不增加 follow-up。
- attempt terminal 从 active map 取快照后移除；task counters 不从 attempt terminal 求和。

## 7. Task 代码变更

仅 `turn/run/outer-run execution.completed` 携带：

```json
{
  "codeChanges": {
    "addedLines": 128,
    "deletedLines": 37,
    "changedFiles": 9
  }
}
```

- 数据只表示任务结束时工作区相对 Run 创建时 Git 基线的最终净差异，不累计 turn churn。
- 指标开启时，Run 创建后、首个节点执行前原子写入独立的 `observability/code-change-baseline.json`，冻结 `workspacePath + baselineCommit`：主工作区 Run 使用 `repoRoot + startup HEAD`，worktree Run 使用 `worktree.path + forkCommit`。
- 基线快照独立于 `RunState`，不得为指标向 Workflow 状态增加字段或改变创建、继续、停止、恢复和 terminal 流程；已存在的基线不得被后续重启或工作区 HEAD 覆盖。
- run terminal 调用现有 Git service 一次，按冻结基线覆盖 baseline 后的 commit、staged、unstaged 与未跟踪文件；编辑后恢复原状不计数。
- rename 按一个最终文件计数；二进制文件计入 `changedFiles`，不虚构增删行；无变更固定为 `0/0/0`。
- 结果先原子写入 run observability 的 `code-changes.json`，terminal fact 读取或复用该快照；重放不得重新扫描已经结束或清理的工作区。
- Git 基线或工作区不可用时省略整个 `codeChanges` 并记录结构化诊断；不得用 terminal 时的 HEAD 代替缺失基线，采集失败不得改变 runtime terminal。
- 删除 `completeness/limitationCodes`、turn-files delta 以及 collector 的跨 attempt code change accumulator。

## 8. Usage、模型与质量字段

Usage 的权威粒度仍是 Direct turn、Workflow node attempt 和 AUTO unit attempt。Workflow run/AUTO outer run 不聚合子 attempt Usage。

- started 不携带 model；completed 从 ACP session 与 prompt usage segment 解析真实 provider/model。
- 同 attempt 内按首次使用顺序合并 `modelUsages[]`；顶层 usage 是非 null segment 的求和。
- provider counter 回退、session 更换或字段缺失时保持 null，不产生负数、不猜测 0。
- AUTO workflow invocation 不承接 child Workflow token；没有独立 ACP 调用时 usage 为 null。
- `roundCount`、acceptance `passed/acceptanceAttempt/firstPass`、`failedAttemptId` 和稳定 terminal reason 继续按既有领域终态生成。

## 9. SQLite state 与 outbox

```rust
struct TaskMetricsState {
    counters: MetricsCounters,
    acceptance_attempts: u32,
}
```

attempt 的 `started/paused/resumed/completed` 由 canonical runtime transition 产生稳定 factId。collector 只按该领域身份幂等，不使用 payload 内容指纹；重复投影不分配新 revision，不重复更新 counters。`started` 只允许来自 attempt 首次进入 Running，paused attempt 的继续只能产生 resumed。

Workflow continue 保持 Runtime 原有控制契约：在后台 drive 启动前，以 attempt locator 和 execution ID 校验当前 paused 事实，一次提交 `Run.status=Running`、清空 pause reason 并推进 `execution.revision`。metrics 不拥有 continue action，不得向 `RunState/RuntimeExecutionState` 增加 `pendingAction`、额外状态或消费分支，也不得改变 continue、stop、重启恢复和启动失败收敛的顺序。

显式 continue 在调用期构造非持久化 metrics context，只冻结提交前的 pause reason、提交成功后的 execution revision、非空输入标记和 prompt ID。drive 首次成功持久化后消费一次：resume factId/transitionId 使用 `run:<runUuid>:resume:<executionRevision>`，人工继续映射 `ManualContinue`；非空输入优先使用 prompt ID 作为 follow-up action ID，缺失 prompt ID 时使用该次 resume transition ID 派生稳定 ID。该 context 只负责指标投影，不参与可继续性判断、CAS、错误恢复或 UI 状态。启动前失败仍按原 execution ID 收敛为 `Paused + RuntimeAbnormal`，不会留下指标专用状态阻塞下一次 continue。

带人工确认的节点使用 `Running -> AwaitingManualCheck(Paused) -> Completed`。Provider 返回仅固化 artifact 并进入等待确认，不发布 terminal；用户确认后才写 outcome/finishedAt 并发布唯一 completed，确认动作本身不计 resume/manualContinue。

SQLite 至少包含：

- `metrics_task_state(project_id, execution_id, last_revision, state_json, updated_at)`；复合主键为 Project/Task，`state_json` 只保存 Task counters 与 acceptance 次数。
- `metrics_attempt_state(project_id, execution_id, run_id, round_id, node_id, attempt_id, counters_json, updated_at)`。
- `metrics_transition_dedup(project_id, execution_id, transition_kind, transition_id, created_at)` 与 `metrics_fact_dedup(project_id, execution_id, fact_id, payload_json, created_at)`。
- `metrics_outbox(event_id, project_id, execution_id, event_revision, reported_at, payload_json, status, attempt_count, next_attempt_at, lease_owner, lease_until, acked_at, error_code)`。

collector 使用单 SQLite writer 和短事务。HTTP 永不位于事务或锁内。pending/过期 in-flight 在事务内 claim；claim 时按事件原子执行 `attempt_count + 1`，首次请求计为第 1 次，只允许领取 `attempt_count < 3` 的行。accepted/duplicate ack，逐项 rejected 只拒绝对应行；网络错误、HTTP 408/429/5xx、无效或不完整响应属于可重试错误，未达到 3 次的事件回到 pending 并按各自 attempt 有界退避。普通 4xx（包含 401/403）和本地 payload 错误属于永久错误，第一次失败即物理删除。可重试错误在第 3 次失败后物理删除，不保留 dropped/dead-letter 状态，应用重启后不可恢复。

失败结算必须在单个 SQLite 事务内逐事件执行，并校验 `event_id + in_flight + lease_owner + attempt_count`；同一 HTTP batch 中不同 attempt 的事件可以分别进入“继续等待”和“立即删除”。应用升级后，claim 事务先恢复过期 lease，再物理删除历史 `pending && attempt_count >= 3` 的记录，避免旧的无限重试数据再次发送。claim 前计数保证任何事件不超过 3 次请求；进程在 claim 后、HTTP 请求前崩溃时，该次额度仍被消耗，因此实际请求可能少于 3 次，这是有限投递策略接受的数据丢失窗口。

Task delivery terminal 不删除 state。只有 Task 明确删除/归档且没有未确认 outbox 时，才按统一保留策略清理。

## 10. 生命周期

### Direct

每个真实 ACP prompt 发布 turn started/terminal；内部 Worker shell 不产生 Workflow 指标。首轮后新目标由 typed FollowUp transition 计数。`executionId` 为 Task UUID，`runId/roundId` 使用实际 Runtime locator。

ACP prompt dispatcher 是 Direct turn 的唯一源头：prompt 被 provider 接受时发布 typed `DirectTurnLifecycle::Started`，命令完成、provider 错误、用户取消或 join failure 时发布对应 `Finished(outcome)`。首轮使用固定内部 turn ID，后续轮使用持久化 prompt ID；started/terminal 必须使用同一 ID。

Direct、Workflow 和 AUTO 的源头事实统一进入 `core.metrics-producer` FIFO worker，再产生 `PendingMetricsFact`。不为 Direct 保留独立 sender/worker/队列，避免队列未初始化、容量不一致或 `try_send` 失败导致只有 Direct 丢失日志。active turn 按 Task UUID 唯一管理并记录 turn ID：重复 started、重叠 turn、不匹配 terminal 和缺失 started 都丢弃异常事实并记录结构化诊断。terminal fact 构造完成后先清理 active turn 与运行期 observability state，再发布下游事实，保证 terminal 对观察者可见时旧状态已不可见。

### Workflow

run started/terminal 使用 `run` subject；node started/terminal 及 paused/resumed/intervention 使用真实 `node-attempt`。新 Round 更新 round locator，新 Run 更新 run locator，task revision 不重置。恢复只发 resumed，不重复 started。

### AUTO

outer delivery 使用 `outer-run`；dynamic worker/workflow invocation/merge/acceptance 使用 `unit-attempt`。中间态绑定触发它的 durable dynamic leaf；AUTO wrapper 不上报。workflow invocation 用 `childRunId` 关联 child Workflow。

## 11. 客户端校验与结构化错误

collector 构造 wire event 前校验：

1. Project/Task/Run/Round identity 完整且格式合法。
2. subject 与 session mode、event type、attempt fields 符合矩阵。
3. counters/codeChanges 只出现在规定 terminal。
4. scheduled 来源必须携带合法 `once/repeat/cron` 快照，user 来源必须省略 trigger。
5. `occurredAt/reportedAt/timing.startedAt/timing.endedAt` 必须符合无时区本地毫秒格式。
6. codeChanges 出现时三个非负整数必须同时存在。

内部错误使用稳定 code 与结构化 params，不含对客文案：`METRICS_ACTIVE_ATTEMPT_MISSING`、`METRICS_FACT_INVALID`、`METRICS_COLLECTOR_STORAGE_FAILED`、`METRICS_OUTBOX_CLAIM_FAILED`。

单条坏事实只丢弃该事实并记录有界诊断，不影响同批其他事实和业务 lifecycle。

## 12. 性能、背压与隐私

- producer 热路径为 O(1) 有界 `try_send`；队列容量沿用 2048，单批最多 100。
- SQLite 工作在专用 blocking worker；同一时刻一个 writer，不持锁 await，不把 HTTP 放进事务。
- active attempt terminal 后移除；outbox 有状态、lease、最多 3 次 attempt 和保留策略，失败事件不会形成无界积压。
- 不记录 prompt、回复、工具输入输出、附件正文、源码、diff 或 logical path。
- 每次 HTTP batch attempt 在统一发送边界记录 `requestId + attempt + url + body`；`body` 必须复用实际发送的序列化字符串，允许包含 metrics wire payload，但不得记录认证头。wire payload 继续禁止 prompt、回复、工具输入输出、附件正文、源码、diff 或 logical path；日志仍受 20 MB 总量上限保护。

每次 claim 和失败结算仍为单批最多 100 条的 O(batch size) 点更新；历史超限清理复用 outbox claim 事务且只在 uploader 轮询时执行，不增加 Runtime 热路径 I/O、HTTP 次数、线程、缓存或锁范围。有限尝试会降低长期断网时的数据库行数、日志量和网络请求量。当前没有采样证据表明 CPU 或内存存在热点，因此不增加索引、并发 writer 或缓存；release 压测应覆盖连续事件、混合 100 条 batch、SQLite 慢和有限网络重试，并记录 lifecycle 延迟、队列丢弃与 batch 吞吐基线。

## 13. 接口级验收

- 同一 Task 跨 `run-001/round-001 -> round-002 -> run-002/round-001` 以及应用重启，revision 严格递增。
- state cursor、counters/codeChanges 和 outbox insert 任一步失败都不产生半提交。
- Workflow/AUTO 中间态使用 attempt subject；缺失 locator 不伪造事件且业务继续。
- attempt/task counters 在 permission、elicitation、manual continue、follow-up、retry 与重复 request 下符合精确定义。
- scheduled new/continuous/manual/follow-up 的 origin/trigger 组合正确。
- code changes 覆盖 commit/staged/unstaged/untracked、revert、rename、binary、无变更、快照重放与 Git 不可用。
- 关闭客户端后继续和停止后带文字继续均只产生 resumed，不重复 started；失败启动后再次 continue 不受任何 metrics 状态阻塞；run terminal counters 固定为对应 canonical action 的投影。
- 人工确认节点严格按 started -> paused/manual-decision -> completed，terminal 后不得再出现 paused。
- batch 100 条混合多个 Project/Task/Run/Round 不串 state；部分 rejected 不影响 accepted。
- producer queue 满、SQLite 慢和网络重试不阻塞 Runtime 事件线程。
- 单条事件首次请求计为第 1 次；可重试失败最多请求 3 次，第三次失败后物理删除且第 4 次无法 claim；永久错误第一次失败即删除。
- 混合 attempt batch 按事件独立结算；应用重启不重置 attempt；历史 `attempt_count >= 3` 的 pending 行在 claim 前删除且不发送。
- request 日志包含批内最大 outbox `attempt_count`、实际 endpoint 和与 HTTP body 字节一致的 JSON，且不包含 API key；失败结算日志包含 retryable、rescheduled、dropped 与最大次数。
- Direct prompt bridge 对 accepted/finished 发布同一 turn ID 的 typed 源头事实，completed/failed/cancelled 精确映射；重复、重叠和不匹配转换不产生 outbox 事件。

## 14. 实施状态

- [x] 正式协议、数据归属、服务端文档边界完成评审。
- [x] typed `PendingMetricsFact` 与 subject/origin/trigger/action 完成。
- [x] SQLite collector、TaskMetricsState、outbox/uploader 完成。
- [x] Direct、Workflow、AUTO、scheduler 与 codeChanges producer 完成。
- [x] 客户端接口测试与真实 batch 部分响应验收完成。
- [x] uploader request 日志恢复 `attempt + url + body`，并以接口测试固定批次 attempt 与实际 wire body 一致性。
- [x] uploader 改为单事件总计最多 3 次请求；第三次可重试失败、永久错误和历史超限 pending 均物理删除，并以逐事件、重启持久化接口测试固定合同。
- [x] Direct prompt lifecycle 已收敛到共享 metrics producer，删除容量 512 的独立队列与 worker，并补齐 turn identity/outcome/重复转换接口测试。
- [ ] release 性能基线与服务端实现由发布/服务端仓库按 `metrics-server-processing.md` 完成。
