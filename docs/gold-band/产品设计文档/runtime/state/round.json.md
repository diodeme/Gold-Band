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
  "repairLoopsUsed": 0,
  "startedAt": "2026-03-20T10:30:10Z"
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
- `repairLoopsUsed`
- `startedAt`

---

## 4. 字段说明

### `index`
- 类型：number
- 含义：第几轮 round
- `round-001` 对应 `index = 1`

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
- `acceptance_loop`：由 `verify.failure` 触发的新 round

### `repairLoopsUsed`
- 类型：number
- 含义：当前 round 已实际消耗的 repair loop 次数

说明：
- 统计口径应与 Runtime Control 中的 repair loop 定义一致
- 只有 `exec.failure` 或 `exec.invalid` 真正触发回到某个 `worker` 时才加 1
- `worker.failure` / `worker.invalid` 不计入
- `verify.failure` / `verify.invalid` 不计入

---

## 5. runtime 校验规则
以下情况应视为 `invalid`：

- 缺少任一必填字段
- `index` 不是正整数
- `status` 不在合法枚举内
- `outcome` 不在合法枚举内且不为 null
- `trigger` 不在合法枚举内
- `repairLoopsUsed` 不是非负整数
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
