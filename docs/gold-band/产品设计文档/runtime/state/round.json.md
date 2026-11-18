# `round.json` 规范

## 1. 一句话定义
`round.json` 表示一次 acceptance round，也就是一次大循环。

它用于表达：
- 这是第几轮
- 这轮为什么开始
- 当前这轮是否还在运行
- 这轮最终是否成功或失败

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "id": "round-001",
  "runId": "run-001",
  "index": 1,
  "status": "running",
  "outcome": null,
  "trigger": "initial",
  "startedAt": "2026-03-20T10:30:10Z",
  "trace": [
    {
      "sequence": 1,
      "nodeId": "plan",
      "attemptId": "attempt-001",
      "fromNodeId": null,
      "edgeOutcome": null,
      "enteredAt": "2026-03-20T10:30:10Z"
    }
  ]
}
```

---

## 3. 必填字段
- `version`
- `id`
- `runId`
- `index`
- `status`
- `outcome`
- `trigger`
- `startedAt`
- `trace`

---

## 4. 字段说明

### `index`
- 类型：number
- 含义：第几轮 round
- `round-001` 对应 `index = 1`
- 新 round 的 `id` 必须直接由当前 `RoundState.index + 1` 格式化生成；runtime 状态是编号事实源，不扫描 rounds 目录推导另一套编号。

### `status`
- 类型：string
- 枚举：`running | paused | completed`

说明：
- `paused` 表示当前 round 中存在被 runtime 挂起、等待外部动作的当前 attempt

### `outcome`
- 类型：string | null
- 枚举建议：`success | failure | killed | null`

说明：
- `running` 或 `paused` 时必须 `outcome = null`
- `completed` 时必须给出终局值

### `trigger`
- 类型：string
- 枚举建议：`initial | acceptance_loop`

说明：
- `initial`：首轮执行
- `acceptance_loop`：由 `worker.failure` 触发的新 round

### `trace`
- 类型：array
- 含义：当前 round 按提交顺序进入过的 node/attempt 序列，用于控制上下文与 Round 详情页展示实际执行路径；当前位置转移仍以 `run.json.currentRound/currentNode/currentAttempt` 为唯一 commit marker，同 locator 内部状态更新使用各自的 lifecycle/revision 契约。

每个 trace step 包含：
- `sequence`：本 round 内的递增序号，从 1 开始。
- `nodeId`：进入的真实 workflow node id。
- `attemptId`：该次进入对应的 attempt id。
- `fromNodeId`：从哪个 node 转入；entry 节点为 null。
- `edgeOutcome`：触发转移的 outcome，如 `success | failure | retry | null`；schema 输出不合法的隐藏修复不新增 trace step。
- `enteredAt`：进入该 node/attempt 的时间。

canonical 读取规则：

- 共享 helper 必须同时接收 run、当前 round 和目标 round：目标为 `run.currentRound` 时，执行、恢复、Graph VM、历史投影、attempt 计数和 retry 幂等判断只消费截断到 `run.current*` 唯一匹配 step 的 trace（包含该 step）。
- 目标 round 的 `index` 严格小于当前 round 时，必须为 `completed` 并消费完整已校验 trace，否则是损坏状态；目标 round 的 `index` 更大时，将其视为未提交/孤立状态并忽略；相同 `index` 却不是 `run.currentRound` 时，判定为 identity 冲突。
- `node -> round -> run` 多文件提交在 round 已写、run 未写时可能留下未提交尾部；该尾部不得驱动执行、显示为 running 或进入 predecessor 上下文，下一次同 round 写入必须先丢弃该尾部。
- current step 缺失、重复，或 sequence 非严格递增时属于损坏状态；不得使用目录最大 attempt、最后一个 trace step 或显示顺序猜测 current。
- 当前节点 retry 在 canonical 前缀后追加同 node 的新 step，`edgeOutcome=retry`；相邻的 expected step 与 retry step 可用于识别同一请求的幂等重投。
- 会话树的节点集合、各节点最新状态、attempt 列表及 session 续接关系必须由同一 canonical trace 投影。按 trace locator 精确读取并校验 `node.json`，保留 trace 顺序；预留空目录、孤立 node 文件和未提交 trace 尾部都不进入会话树或 active sessions。已提交 locator 对应文件缺失或身份不匹配时返回错误，不回退到物理目录猜测。
- 查询单个历史 round 的会话续接关系时，必须同时提供 `run.currentRound` 的上下文；只消费目标 round 的 canonical trace。读取目标与当前两个 round 即可，不为一次历史会话查询扫描其他 round 的节点或会话正文。

缺失 `trace` 的旧 `round.json` 仍可反序列化为空数组，但不能据此绕过 current locator 校验，也不能通过目录扫描重建 Workflow/AUTO 会话树。Direct 不使用 Workflow trace 授权，继续使用其既有会话读取契约。

---

## 5. runtime 校验规则
以下情况应视为 `invalid`：

- 缺少任一必填字段
- `index` 不是正整数
- `status` 不在合法枚举内
- `outcome` 不在合法枚举内且不为 null
- `trigger` 不在合法枚举内
- `trace` 中任一 step 的 `sequence` 不是正整数、sequence 非严格递增、locator 重复，或 `nodeId` / `attemptId` 为空
- `round.id = run.currentRound` 时，trace 不包含唯一匹配的 `run.current*` step
- 早于当前 round 的历史 round 状态不是 `completed`
- 非当前 round 与当前 round 的 `index` 相同
- `status = running` 但 `outcome != null`
- `status = paused` 但 `outcome != null`
- `status = completed` 但 `outcome = null`

---

## 6. 相关文档
- [Runtime 概览](../overview.md)
- [控制层](../control.md)
- [run.json](run.json.md)

---

## 7. 一句话总结

> `round.json` 是大循环级状态快照：它告诉 runtime 当前是第几轮，这一轮是怎么开始的，以及最后是否完成。
