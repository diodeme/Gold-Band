# `exec-result` 规范

## 1. 一句话定义
`exec-result.json` 表达 `exec` 节点执行完命令后的标准化结果。

它是 runtime 做确定性判断的核心依据。

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "status": "fail",
  "groups": [
    {
      "id": "build",
      "status": "pass",
      "commands": [
        {
          "id": "build",
          "exitCode": 0,
          "status": "pass",
          "stdoutPath": "commands/01-build/stdout.log",
          "stderrPath": "commands/01-build/stderr.log",
          "resultPath": "commands/01-build/result.json"
        }
      ]
    },
    {
      "id": "test",
      "status": "fail",
      "commands": [
        {
          "id": "test-targeted",
          "exitCode": 1,
          "status": "fail",
          "stdoutPath": "commands/02-test/stdout.log",
          "stderrPath": "commands/02-test/stderr.log",
          "resultPath": "commands/02-test/result.json"
        }
      ]
    }
  ]
}
```

---

## 3. 必填字段
- `version`
- `status`
- `groups`

每个 group 必填：
- `id`
- `status`
- `commands`

每条命令必填：
- `id`
- `exitCode`
- `status`
- `stdoutPath`
- `stderrPath`
- `resultPath`

---

## 4. 字段说明

### `status`
- 类型：string
- 枚举：`pass | fail`
- 含义：本次 `exec` 节点的整体结果

### group `status`
- 类型：string
- 枚举：`pass | fail | skipped`

### command `status`
- 类型：string
- 枚举：`pass | fail | skipped`

---

## 5. runtime 校验规则
以下任一情况都应视为 `invalid`：

- 缺少任一必填字段
- `status` 不在合法枚举内
- `groups` 不是数组
- 任意 group 缺少必填字段
- group `status` 不在合法枚举内
- 任意命令缺少必填字段
- command `status` 不在合法枚举内
- 顶层 `status` 与各 group 聚合结果不一致
- 任意 group `status` 与其 `commands` 聚合结果不一致
- `skipped` 的 command 若存在执行结果字段，其值不满足当前 prompt / runtime 约定
- `stdoutPath | stderrPath | resultPath` 为空字符串
---

## 6. 相关文档
- [exec 节点](../nodes/exec.md)
- [Runtime Control](../../runtime/control.md)

---

## 7. 一句话总结

> `exec-result.json` 是 `exec` 节点的 canonical result：它稳定记录每条命令的执行结果，并为 runtime 提供确定性 pass/fail 判断依据。
