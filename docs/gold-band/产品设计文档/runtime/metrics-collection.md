# WB 会话指标采集与批量上报设计

> 状态：2026-08-21 客户端破坏式合同升级已完成；服务端待外部仓库实现
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
5. 已被 collector 接受的事实通过 SQLite transactional outbox 可靠补报；网络与数据库不阻塞 Runtime 热路径。

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
| 代码变更 | 已 finalized 的 turn file change set | Task 创建至当前 delivery terminal | delivery terminal |

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

`reportedAt` 使用 collector 生成的 Asia/Shanghai 本地毫秒时间。`occurredAt/scheduledAt` 沿用当前 runtime canonical 时间值，可能是 RFC 3339、Asia/Shanghai 本地毫秒时间或 `<unix-seconds>Z`；服务端按处理合同解析后统一存 UTC。该多格式边界属于现有 runtime 时间模型，本次不新增第四种格式。

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

### 5.2 来源联合类型

```json
{
  "taskOrigin": {"kind": "scheduled-task", "scheduledTaskId": "scheduled-task-001"},
  "executionTrigger": {
    "kind": "scheduled-occurrence",
    "scheduledTaskId": "scheduled-task-001",
    "scheduledOccurrenceId": "occurrence-001",
    "triggerKind": "scheduled",
    "scheduledAt": "2026-08-20T10:00:00Z",
    "sessionPolicy": "new"
  }
}
```

- Task 创建来源为 `user/scheduled-task`，创建后不变。
- execution 触发来源为 `user/scheduled-occurrence`，描述本次 Run/Turn。
- “立即执行”仍是 scheduled occurrence，`triggerKind=manual`。
- 定时 Task 后续用户 follow-up 保持 scheduled task origin，但 trigger 改为 user。
- 当两个对象都是 scheduled 类型时 `scheduledTaskId` 必须相等。
- 不上传 instruction、标题、cron 文案等可变或敏感来源字段。

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
- FollowUp：Task 已完成一次交付后接受新目标，只 `task.followUp + 1`。
- node/unit attempt 的 `followUpCount` 必须为 0。
- attempt terminal 从 active map 取快照后移除；task counters 不从 attempt terminal 求和。

## 7. Task 代码变更

仅 `turn/run/outer-run execution.completed` 携带：

```json
{
  "codeChanges": {
    "addedLines": 128,
    "deletedLines": 37,
    "changedFiles": 9,
    "completeness": "complete",
    "limitationCodes": []
  }
}
```

- 数据来自已 finalized 的 `TurnFileChangeSet`，复用其 summary、logical path 和 limitation code。
- 行数表达 Task edit churn，不声称是 workspace 最终净 diff。
- 同一路径跨 turn 多次编辑时行数累加，`changedFiles` 去重。
- 完整且无变更为 complete + `0/0/0`；存在 capture limit、non-linear mutation、二进制或缺失标准 diff 为 partial；完全无可信来源为 unavailable + null。
- collector 只保存数值、去重 path 和 limitation code；wire、日志和服务端均不包含 path、diff 或源码。
- task terminal 不调用 Git、不扫描 workspace、不读取全部源码。

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
    last_revision: u64,
    task_counters: MetricsCounters,
    active_attempts: HashMap<AttemptMetricsKey, MetricsCounters>,
    code_changes: TaskCodeChangeAccumulator,
}
```

SQLite 至少包含：

- `metrics_task_state(project_id, execution_id, last_revision, task_counters_json, code_changes_json, updated_at)`；复合主键为 Project/Task。
- `metrics_attempt_state(project_id, execution_id, run_id, round_id, node_id, attempt_id, counters_json, request_ids_json, updated_at)`。
- `metrics_outbox(event_id, project_id, execution_id, event_revision, reported_at, payload_json, status, attempt_count, next_attempt_at, lease_owner, lease_until, acked_at, error_code)`。

collector 使用单 SQLite writer 和短事务。HTTP 永不位于事务或锁内。pending/过期 in-flight 在事务内 claim；accepted/duplicate ack，逐项 rejected 只拒绝对应行；timeout/429/5xx 回到 pending 并有界退避。401/403 先释放当前 lease 回 pending，再停止当前 uploader，避免无效鉴权持续请求；应用重启或指标子系统按新配置重新初始化后恢复。lease 过期可恢复。

Task delivery terminal 不删除 state。只有 Task 明确删除/归档且没有未确认 outbox 时，才按统一保留策略清理。

## 10. 生命周期

### Direct

每个真实 ACP prompt 发布 turn started/terminal；内部 Worker shell 不产生 Workflow 指标。首轮后新目标由 typed FollowUp transition 计数。`executionId` 为 Task UUID，`runId/roundId` 使用实际 Runtime locator。

### Workflow

run started/terminal 使用 `run` subject；node started/terminal 及 paused/resumed/intervention 使用真实 `node-attempt`。新 Round 更新 round locator，新 Run 更新 run locator，task revision 不重置。恢复只发 resumed，不重复 started。

### AUTO

outer delivery 使用 `outer-run`；dynamic worker/workflow invocation/merge/acceptance 使用 `unit-attempt`。中间态绑定触发它的 durable dynamic leaf；AUTO wrapper 不上报。workflow invocation 用 `childRunId` 关联 child Workflow。

## 11. 客户端校验与结构化错误

collector 构造 wire event 前校验：

1. Project/Task/Run/Round identity 完整且格式合法。
2. subject 与 session mode、event type、attempt fields 符合矩阵。
3. counters/codeChanges 只出现在规定 terminal。
4. node/unit followUp 为 0。
5. scheduled 联合类型字段成组出现且 ID 一致；Workflow/AUTO 不允许 continuous。
6. codeChanges complete 时数值完整，unavailable 时数值为 null。

内部错误使用稳定 code 与结构化 params，不含对客文案：`METRICS_ACTIVE_ATTEMPT_MISSING`、`METRICS_FACT_INVALID`、`METRICS_COLLECTOR_STORAGE_FAILED`、`METRICS_OUTBOX_CLAIM_FAILED`。

单条坏事实只丢弃该事实并记录有界诊断，不影响同批其他事实和业务 lifecycle。

## 12. 性能、背压与隐私

- producer 热路径为 O(1) 有界 `try_send`；队列容量沿用 2048，单批最多 100。
- SQLite 工作在专用 blocking worker；同一时刻一个 writer，不持锁 await，不把 HTTP 放进事务。
- active attempt terminal 后移除；changed-file set 沿用 turn capture entry 上限；outbox 有状态、lease 和保留策略。
- 不记录 prompt、回复、工具输入输出、附件正文、源码、diff 或 logical path。
- 日志只记录 event/task identity、稳定错误码、队列/批次/状态和大小，不记录 payload 原文。

实现已通过有界队列、单 writer、短事务、lease 恢复和真实 HTTP 部分响应测试完成结构性性能验收。当前没有采样证据表明 CPU 或内存存在热点，因此不增加并发 writer、缓存或微优化；release 压测应覆盖连续事件、混合 100 条 batch、SQLite 慢和网络重试，并记录 lifecycle 延迟、队列丢弃与 batch 吞吐基线。

## 13. 接口级验收

- 同一 Task 跨 `run-001/round-001 -> round-002 -> run-002/round-001` 以及应用重启，revision 严格递增。
- state cursor、counters/codeChanges 和 outbox insert 任一步失败都不产生半提交。
- Workflow/AUTO 中间态使用 attempt subject；缺失 locator 不伪造事件且业务继续。
- attempt/task counters 在 permission、elicitation、manual continue、follow-up、retry 与重复 request 下符合精确定义。
- scheduled new/continuous/manual/follow-up 的 origin/trigger 组合正确。
- code changes 覆盖 complete/partial/unavailable、同路径多 turn 去重与隐私边界。
- batch 100 条混合多个 Project/Task/Run/Round 不串 state；部分 rejected 不影响 accepted。
- producer queue 满、SQLite 慢和网络重试不阻塞 Runtime 事件线程。

## 14. 实施状态

- [x] 正式协议、数据归属、服务端文档边界完成评审。
- [x] typed `PendingMetricsFact` 与 subject/origin/trigger/action 完成。
- [x] SQLite collector、TaskMetricsState、outbox/uploader 完成。
- [x] Direct、Workflow、AUTO、scheduler 与 codeChanges producer 完成。
- [x] 客户端接口测试与真实 batch 部分响应验收完成。
- [ ] release 性能基线与服务端实现由发布/服务端仓库按 `metrics-server-processing.md` 完成。
