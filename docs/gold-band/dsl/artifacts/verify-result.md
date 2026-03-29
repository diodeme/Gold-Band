# `verify-result` 规范

## 1. 一句话定义
`verify-result.json` 表达 `verify` 节点的验收结论。

它回答的是：
- 任务是否通过验收
- 哪些需求仍未满足
- 当前验证证据是否不足
- 若进入下一轮，应该把哪些反馈带回给 `worker`

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "status": "fail",
  "summary": "需求尚未完全满足",
  "unmetRequirements": [
    "缺少错误场景处理"
  ],
  "validationGaps": [
    "仅运行了增量单测，未验证集成路径"
  ]
}
```

---

## 3. 必填字段
- `version`
- `status`
- `summary`
- `unmetRequirements`
- `validationGaps`

---

## 4. 字段说明

### `status`
- 类型：string
- 枚举：`pass | fail`

说明：
- `pass` 表示本次验收通过
- `fail` 表示本次验收未通过
- `fail` 不等于 `invalid`
- `invalid` 是 runtime 对缺失 / 不合法 `verify-result` 的归纳 outcome，不是 `verify-result.status` 的取值

### `summary`
- 类型：string
- 含义：本次验收结论摘要

建议：
- 应优先总结“为什么通过 / 为什么不通过”
- 不应只重复 requirement 原文
- 当 `status = fail` 时，`summary` 应能概括主要失败原因

### `unmetRequirements`
- 类型：string[]
- 含义：尚未满足的需求点
- 允许为空数组，但字段必须存在

说明：
- 用于表达“需求本身还没完成或行为不符合 requirement”
- 偏向实现缺失、实现不正确、覆盖不完整这类问题

### `validationGaps`
- 类型：string[]
- 含义：验证证据不足之处
- 允许为空数组，但字段必须存在

说明：
- 用于表达“证据不足，当前不能放行”
- 偏向测试不足、证据链不完整、缺少关键验证材料这类问题

---

## 5. runtime 校验规则
以下任一情况都应视为 `invalid`：

- 缺少任一必填字段
- `status` 不在合法枚举内
- `summary` 为空或不是字符串
- `unmetRequirements` 不是数组
- `validationGaps` 不是数组
- `status = pass` 但 `unmetRequirements` 非空
- `status = pass` 但 `validationGaps` 非空
- `status = fail` 且 `unmetRequirements` 与 `validationGaps` 同时为空

补充说明：
- `verify.invalid` 表示 runtime 无法接受当前 `verify-result` 为合法 contract
- `verify.failure` 表示 contract 合法，但验收结论是不通过
- 因此 `invalid` 与 `fail` 是两层不同语义，不应混用

---

## 6. 与控制层的关系
- 合法且 `status = pass` 的 `verify-result` -> runtime 归纳为 `verify.success`
- 合法且 `status = fail` 的 `verify-result` -> runtime 归纳为 `verify.failure`
- 缺失、字段不合法或违反本规范约束的 `verify-result` -> runtime 归纳为 `verify.invalid`

说明：
- `verify.failure` 可进入 `onAcceptanceFailure` 定义的控制分支
- `verify.invalid` 不应进入 acceptance loop，而应按 runtime 的错误阻塞策略处理

---

## 7. 相关文档
- [verify 节点](../nodes/verify.md)
- [Runtime Control](../../runtime/control.md)

---

## 8. 一句话总结

> `verify-result.json` 是 `verify` 节点的 canonical result：它必须把“验收未通过”与“结果结构不合法”区分开来，前者进入验收控制分支，后者则进入 runtime 的错误阻塞路径。
