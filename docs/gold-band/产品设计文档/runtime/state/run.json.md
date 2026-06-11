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
  "pauseReason": null
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

---

## 4. 字段说明

### `status`
- 类型：string
- 枚举：`running | paused | completed`

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
- 枚举建议：`process_interrupted | error_blocked | null`

说明：
- 仅当 `status = paused` 时允许为非 null
- MVP 中不支持用户主动 `pause`，因此 `pauseReason` 只记录系统观测到的挂起原因

### `lastExecutedNode`
- 类型：object | null
- 含义：最近一次完成的节点快照，用于节点指标上报、下一节点启动时的 predecessor 语义，以及运行中断后的恢复分析；它是观察性快照，不参与控制流判定。

说明：
- 节点完成后的控制流推进必须优先于指标采集和 token 快照读取；`lastExecutedNode` 可以在推进完成后作为内存 predecessor 快照更新。
- 该字段不得成为推进控制流的唯一事实源；控制流仍以当前完成节点的 `node.json.outcome` 和 workflow edge 为准。
- 指标开关关闭时不得读取 ACP timeline / token 文件；指标开关开启时，token 读取、上报失败或 panic 都不得阻断 `run.json / round.json / node.json` 的推进落盘。

### 推进落盘顺序

节点完成后的状态更新必须避免长期暴露“当前 node 已 completed，但 run/round 仍停留在旧节点 running”的中间态：

1. 先写当前 attempt 的 `node.json = completed + outcome`。
2. 根据 workflow edge 计算下一步控制决策。
3. 如果进入下一节点或新 round，必须立即写入新的 `run.current*`、`round.trace` 和新节点 `node.json = running`。
4. 只有在上述状态落盘后，后续 provider 调用失败才允许表现为新节点暂停或错误阻塞；metrics 只能作为旁路观察逻辑，不能改变 runtime 主状态。

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
- `currentRound | currentNode | currentAttempt` 任一字段缺失
- `currentAttempt != null` 但 `currentNode = null`
- `currentNode != null` 但 `currentRound = null`

---

## 6. 相关文档
- [Runtime 概览](../overview.md)
- [控制层](../control.md)
- [round.json](round.json.md)
- [node.json](node.json.md)

---

## 7. 一句话总结

> `run.json` 是 run 级状态快照：它告诉 Gold Band 这次执行目前跑到哪、是否暂停，以及最终是怎样结束的。
