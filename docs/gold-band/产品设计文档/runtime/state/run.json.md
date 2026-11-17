# `run.json` 规范

## 1. 一句话定义
`run.json` 保存某次执行的全局状态。

它用于表达：
- 这次 run 属于哪个 task
- 当前 run 正在运行、暂停还是已完成
- 当前 round / node / attempt 到哪一步
- 最近一次已完成节点的上报快照
- 最终是成功、失败还是停止

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "id": "run-001",
  "taskId": "task-20260320-001-login-error",
  "status": "running",
  "outcome": null,
  "startedAt": "2026-03-20T10:30:00Z",
  "updatedAt": "2026-03-20T10:32:00Z",
  "workflowSnapshot": "workflow.snapshot.json",
  "currentRound": "round-001",
  "currentNode": "dev",
  "currentAttempt": "attempt-002",
  "acceptanceLoopsUsed": 0,
  "pauseReason": null,
  "execution": {
    "revision": 3,
    "phase": "running-node",
    "locator": {
      "roundId": "round-001",
      "nodeId": "dev",
      "attemptId": "attempt-002"
    },
    "recoveryCandidateToken": "0e531c98-0a36-43a9-b0bf-eec272aa7a41",
    "updatedAt": "2026-03-20T10:32:00Z"
  }
}
```

---

## 3. 必填字段
- `version`
- `id`
- `taskId`
- `status`
- `outcome`
- `startedAt`
- `updatedAt`
- `workflowSnapshot`
- `currentRound`
- `currentNode`
- `currentAttempt`
- `acceptanceLoopsUsed`
- `pauseReason`
- `execution`

---

## 4. 字段说明

### `id`
- 类型：string
- 格式：`run-NNN`

说明：
- `id` 在同一 task 的 `runs/` 目录下唯一。
- 创建新 run 时从该 task 下已持久化的最大 `run-*` 数字后缀递增，并在写入 `run.json` 前先原子创建对应 run 目录占位。
- 会话页“重跑该任务”与普通 `run start` 共享同一分配规则，不允许基于当前选中的 run id 直接 `+1`。

### `status`
- 类型：string
- 枚举：`running | paused | completed`

`completed` 是 Run 的不可逆终态。无论 outcome 为 `success | failure | killed`，同一 Run 都不得再回退为 `running | paused`；终态后的重跑必须创建 successor/new run，不能原地清空 outcome。

### 失败节点重跑与历史继承

Workflow 的普通 Worker 在 `Completed + Failure/Invalid` 后，可从精确的历史 attempt 创建 successor Run。来源 Run 为 `ErrorBlocked` 时拒绝。当前 `Paused + RuntimeAbnormal` 节点继续使用原 Run 的新 attempt。

可选 `rerun` 元数据包含 `origin` 和 `sealedBy`（Rust 字段名为 `sealed_by`）：前者记录 source Run UUID、完整锚点、操作 ID、继承的 round index/trace sequence 与 authoring revision；后者记录当前活动交接操作和 successor 身份。它们不增加生命周期状态。已封存 Run 保留原结果，拒绝继续执行、当前计划写入和 ACP 追问。当前 successor 未 Completed 时，来源拒绝新的 handoff；当前 successor Completed 后，可从其他历史失败 attempt 串行分叉，并更新 `sealedBy` 为新的活动交接。任意时刻最多只有一个 successor 持有来源工作区写权限。

`Completed` 不代表 ACP 追问已经停止。下一次交接必须同时封存前一个已完成的工作区写入者，使用同一 `sealedBy` 拒绝新准入并阻止迟到的 queued prompt；取消并等待它及其实际执行的 child Run 的 live ACP 全部退出后，才可发布新的 Run。旧 Run 的 `status/outcome` 与历史产物保持原值。若 successor 已把写权限交给另一个 Run，沿 `sealedBy` 链检查实际末端；活动末端未 Completed 时拒绝交接，不得仅看中间 Run 的终态。封存链最多 256 个 Run，元数据累计最多 8 MiB，循环、越界或超限明确拒绝。

既有重跑操作记录保存可选的 `previousSuccessors[]`（元素包含 `runId`、`runUuid`），在覆盖来源 seal 前持久化，用于中断后恢复旧写入者的封存与收敛；它是恢复定位，不是新的生命周期或写权限事实源。首次交接为空。来源 seal 已提交而旧写入者 seal 尚未写入时，恢复必须校验旧 Run 的 ID/UUID、补齐同一操作的末端 seal、完成 ACP 收敛，再物化并暂停目标 Run，不调用 provider。

successor 使用任务最新可执行定义，保留到锚点为止的必要 node/产物/附件快照；目标追加新 attempt、新会话。继承 trace 水位以前的节点是历史输入，不消耗新执行的控制预算、不产生执行事件。聊天、Raw、worker-ref 和旧下游不复制。继承节点的产物按照其持久化 resolved config 解释。目标 run.json 最后发布；发布前操作由持久化重跑记录与现有 runtime recovery candidate 恢复。

重跑沿用来源实际工作区，不回滚文件、Git 提交或外部副作用。前缀数量和资源大小有上限；预检后的资源路径若被替换为符号链接、junction 或越界目标，复制阶段必须重新按来源 Run 边界打开并拒绝。超限、工作区丢失、目标删除或定义无效均明确拒绝，不静默截断或退回主工作区。详见开发计划中的《工作流失败节点重跑技术方案》。

### `outcome`
- 类型：string | null
- 枚举：`success | failure | killed | null`

说明：
- `running` 或 `paused` 时必须为 `null`
- 当 `status = completed` 时，必须给出 `outcome`
- `killed` 只表示显式 `run kill` 造成的终局结果

### `workflowSnapshot`
- 类型：string
- 含义：本次 run 实际执行的 workflow snapshot 路径
- 路径基准：run 目录

### `currentRound`
- 类型：string | null
- 含义：当前所在 round id

说明：
- 字段必须存在
- run 创建后但首个 attempt 尚未真正启动前，可为 `null`
- 一旦进入某个 round，通常应保留最后一次已定位的 round id，即使 run 后续完成

### `currentNode`
- 类型：string | null
- 含义：当前所在 node id

说明：
- 字段必须存在
- run 创建后但首个 attempt 尚未真正启动前，可为 `null`
- run 完成后建议保留最后一次已定位的 node id，便于 inspect 与恢复分析

### `currentAttempt`
- 类型：string | null
- 含义：当前所在 attempt id

说明：
- 字段必须存在
- run 创建后但首个 attempt 尚未真正启动前，可为 `null`
- run 完成后建议保留最后一次已定位的 attempt id，便于 inspect 与恢复分析

`currentRound/currentNode/currentAttempt` 三元组是跨 `node.json -> round.json -> run.json` 当前位置转移和新 trace step 的 durable commit marker，不是同 locator 内部状态更新的通用事务 revision。`round.trace`、attempt 目录和事件日志都不能反向覆盖该三元组。读取当前 round 的 trace 时，Graph、执行上下文、恢复和历史投影只能消费截至该 locator 的 canonical 前缀。目标 round 的 `index` 严格小于当前 round 时，必须为 `completed` 并消费完整已校验 trace，否则是损坏状态；`index` 更大的 round 是未提交/孤立状态并忽略；相同 `index` 却不是 current round 则是 identity 冲突。若当前 round 的 trace 不包含唯一匹配的 current locator，活动 Run 必须判定为状态损坏并拒绝继续写入，不能猜测 trace 尾部。

会话树中每个节点及其 attempts 同样以该 commit marker 限定的 trace 为成员事实源，目录存在不代表已提交。后台 driver 推进后，异常收敛使用它当前持有的完整 locator 和 execution owner，不能把初始节点快照当作整个 Run 生命周期内固定不变的 owner；迟到失败继续由现有 execution identity 校验拦截，不增加持久字段。

### `acceptanceLoopsUsed`
- 类型：number
- 含义：当前 run 已实际消耗的 acceptance loop 次数

说明：
- 统计口径应与 Runtime Control 中的 acceptance loop 定义一致
- `round-001` 不计入
- 只有真正新建 acceptance round 时才加 1
- `worker.failure + stop` 不计入
- `worker.invalid` 不计入

### `pauseReason`
- 类型：string | null
- 枚举：`process-interrupted | runtime-abnormal | error-blocked | waiting-for-user-input | permission-requested | null`

说明：
- 仅当 `status = paused` 时允许为非 null
- `process-interrupted` 表示用户停止、关闭或启动恢复等主动中断，可通过 runtime continue 恢复当前 attempt
- `runtime-abnormal` 表示可恢复异常暂停，包括本地 IO/资源、ACP transport、driver 异常、`session/prompt` JSON-RPC error、adapter 结构化 terminal failure、artifact 控制 turn 出现“已有稳定消息但最终消息无稳定 ID”的不可信终态，以及 auth/quota/rate-limit/provider/model/catalog/workspace 等用户处理外部条件后可继续的异常；它需要以异常视觉提醒用户，但仍可通过 runtime continue 恢复。结构化 terminal failure 固定为 `recovery=manual`，不得自动重放可能已产生部分副作用的业务 prompt
- `error-blocked` 表示 workflow/DSL/control edge、输出修复所需的 session / continue identity 缺失、runtime invariant 等当前路径不可继续的阻塞，不提供当前 session 的直接 continue 入口；有可用 continue identity 的输出 repair 耗尽属于可恢复的 `runtime-abnormal`
- `waiting-for-user-input` 与 `permission-requested` 表示 runtime 等待用户明确决策

`pauseReason` 是外层生命周期字段。更细的异常语义由 run progress / run events 中的 `RuntimeErrorInfo` 表达：`recovery=auto` 表示 runtime 正在或已经进行 bounded retry，耗尽后降级为 `runtime-abnormal`；`recovery=manual` 表示用户处理外部条件后可继续；`recovery=blocked` 表示不能普通 continue。旧 run 没有 `RuntimeErrorInfo` 时，`runtime-abnormal` 默认视为 manual，`error-blocked` 默认视为 blocked。

未识别异常保留原始 `diagnostic`，默认 `internal.unknown + recovery=manual`，暂停为 `runtime-abnormal`，允许用户处理后显式继续，不自动重放 prompt。只有已明确判定的控制流、DSL 或不变量错误使用 `recovery=blocked`；不能仅因错误未列入映射表就禁止继续。未知错误码的 UI 直接展示原始原因；已映射错误显示本地化摘要并保留原始详情。

### `execution.recoveryCandidateToken`

- 类型：string | null
- 含义：当前 run execution generation 在用户级 `core.db.runtime_recovery_candidates` 中的 fencing token

说明：
- token 必须在 run 首次或再次持久化为 `Running` 前登记并写入。
- token 只保护候选删除的 generation 一致性，不决定 `status / outcome / phase`，也不能代替 `run.json` canonical lifecycle。
- run 持久化为非 Running 后，候选使用该 token 条件删除；旧 execution 的迟到清理不得删除新 token。
- 历史 run 缺少此字段时按 `null` 读取。启动恢复仍以候选行定位并以 canonical status 校验，不从 token 推断生命周期。

### `lastExecutedNode`
- 类型：object | null
- 含义：最近一次完成的节点快照，用于节点指标上报、下一节点启动时的 predecessor 语义，以及运行中断后的恢复分析；它是观察性快照，不参与控制流判定。

说明：
- 节点完成后的控制流推进必须优先于指标采集和 token 快照读取；但用于推进的完成节点快照必须在应用控制决策前进入同一个内存 `RunState`，并与该决策造成的 `run.json` 状态变化同次持久化，不能先写终态、再只更新内存或依赖旁路二次写。
- 无论控制决策进入后继节点、新 Round，还是以 success / failure 终结，durable `lastExecutedNode` 都必须指向实际触发本次决策的完成节点；终态写入尤其不得保留前一个节点的旧快照。
- 该字段不得成为推进控制流的唯一事实源；控制流仍以当前完成节点的 `node.json.outcome` 和 workflow edge 为准。
- 指标开关关闭时不得读取 ACP timeline / token 文件；指标开关开启时，token 读取、上报失败或 panic 都不得阻断 `run.json / round.json / node.json` 的推进落盘。

### 推进落盘顺序

节点完成后的状态更新必须避免长期暴露“当前 node 已 completed，但 run/round 仍停留在旧节点 running”的中间态：

1. 先写当前 attempt 的 `node.json = completed + outcome`。
2. 派生当前完成节点的 `lastExecutedNode` 快照，并根据 workflow edge 计算下一步控制决策。
3. 在同一个 `RunState` 上同时应用完成节点快照和控制决策：进入下一节点或新 Round 时更新 `run.current*`、`round.trace` 和新节点 `node.json = running`；终结时更新 `run.status / outcome`。
4. 使用本次控制决策的 canonical 持久化入口一次写入包含最新 `lastExecutedNode` 的 `run.json`，终态分支不得在写入后再旁路修改该字段。
5. 只有在上述状态落盘后，后续 provider 调用失败才允许表现为新节点暂停或错误阻塞；metrics 只能作为旁路观察逻辑，不能改变 runtime 主状态。

---

## 5. runtime 校验规则
以下情况应视为 `invalid`：

- 缺少任一必填字段
- `status` 不在合法枚举内
- `outcome` 不在合法枚举内且不为 null
- `status = running` 但 `outcome != null`
- `status = paused` 但 `outcome != null`
- `status = completed` 但 `outcome = null`
- `acceptanceLoopsUsed` 不是非负整数
- `status != paused` 但 `pauseReason != null`
- `pauseReason` 不属于合法枚举且不为 null
- `execution.recoveryCandidateToken` 非 null 且不是非空 string
- `currentRound | currentNode | currentAttempt` 任一字段缺失
- `currentAttempt != null` 但 `currentNode = null`
- `currentNode != null` 但 `currentRound = null`

---

## 6. 相关文档
- [Runtime 概览](../overview.md)
- [用户级核心状态与 Runtime 恢复](../core-state-and-recovery.md)
- [控制层](../control.md)
- [round.json](round.json.md)
- [node.json](node.json.md)

---

## 7. 一句话总结

> `run.json` 是 run 级状态快照：它告诉 Gold Band 这次执行目前跑到哪、是否暂停，以及最终是怎样结束的。
