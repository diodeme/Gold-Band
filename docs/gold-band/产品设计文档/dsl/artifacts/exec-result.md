# `exec-result` 规范

## 1. 一句话定义
`exec-result.json` 表达 `exec` 节点执行完命令后的标准化结果。

它是 runtime 做确定性判断的核心依据。

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "status": "failure",
  "commands": [
    {
      "id": "build",
      "exitCode": 0,
      "status": "success",
      "startTime": "2026-03-29T10:00:00Z",
      "endTime": "2026-03-29T10:00:03Z",
      "durationMs": 3000,
      "timedOut": false,
      "stdoutPath": "commands/01-build/stdout.log",
      "stderrPath": "commands/01-build/stderr.log"
    },
    {
      "id": "test-targeted",
      "exitCode": 1,
      "status": "failure",
      "startTime": "2026-03-29T10:00:03Z",
      "endTime": "2026-03-29T10:00:08Z",
      "durationMs": 5000,
      "timedOut": false,
      "stdoutPath": "commands/02-test/stdout.log",
      "stderrPath": "commands/02-test/stderr.log"
    }
  ]
}
```

---

## 3. 必填字段
- `version`
- `status`
- `commands`

每条已执行命令必填：
- `id`
- `exitCode`
- `status`
- `startTime`
- `endTime`
- `durationMs`
- `timedOut`
- `stdoutPath`
- `stderrPath`

每条 `skipped` 命令必填：
- `id`
- `status`

---

## 4. 字段说明

### `status`
- 类型：string
- 枚举：`success | failure`
- 含义：本次 `exec` 节点的整体结果

### command `status`
- 类型：string
- 枚举：`success | failure | skipped`

补充：
- `skipped` 只表示该命令按当前串行执行策略未被执行
- `skipped` 命令通常也不要求生成 `stdout.log` / `stderr.log`

### `startTime` / `endTime` / `durationMs` / `timedOut`
- 这些字段直接记录在 `exec-result.json.commands[]` 中，而不是拆成额外 sidecar
- `timedOut` 为 boolean；若为 `true`，通常应与 `status = failure` 一起出现，而不是额外引入顶层 `timeout` 状态
- `skipped` 只用于表示串行执行中后续命令未被执行；此时不要求这些执行期字段
- `timeoutSec` 若触发超时，应在对应 command entry 与整体聚合结果中体现

---

## 5. runtime 校验规则
以下任一情况都应视为 `invalid`：

- 缺少任一必填字段
- `status` 不在合法枚举内
- `commands` 不是数组
- 任意命令缺少必填字段
- command `status` 不在合法枚举内
- 顶层 `status` 与各 command 聚合结果不一致
- `skipped` 的 command 仍携带与未执行语义冲突的结果字段值
- 已执行命令的 `stdoutPath | stderrPath` 为空字符串

---

## 6. 聚合规则
首版建议：
- 若所有已执行命令均为 `success`，且不存在 `failure`，则顶层 `status = success`
- 只要任一已执行命令为 `failure`，则顶层 `status = failure`
- `skipped` 不会单独让顶层 `status = success`
- 在 fail-fast 串行执行下，失败命令之后的后续命令通常应为 `skipped`
- `exec-result.json.commands[]` 就是每条命令的唯一 canonical 执行结果，不再额外维护 `commands/*/result.json`

---

## 7. 一句话总结

> `exec-result.json` 是 `exec` 节点的 canonical result：它稳定记录串行命令执行结果，并为 runtime 提供确定性 success/failure 判断依据。