# Gold Band Control DSL 规范

## 1. 一句话定义
Control DSL 定义 workflow 的控制面：节点之间如何流转、节点 outcome 如何映射到下一步、何时结束 run、何时打开新 round，以及 session 是否复用。

## 2. 控制原则
- 节点统一为 `worker`，控制层不根据节点 id 或历史名称赋予特殊语义。
- 所有跳转优先由显式 edge 表达。
- edge 的 `on` 只接受 `success / failure / invalid`。
- edge 的 `to` 可指向真实 worker 节点、`$end` 或 `$new-round`。
- edge 的 `session` 可选；省略时为 `new`，声明 `continue` 时目标 provider 必须支持 continue session。
- `$end` 与 `$new-round` 是控制目标，不是节点 id。

## 3. 全局控制字段

```json
{
  "control": {
    "max_attempts": 3,
    "max_rounds": 2
  }
}
```

两个字段均可省略，省略表示不限制。

- `max_attempts`：当前 round 内，同一条 `来源节点 -> 目标节点` transition 可创建的最大 attempt 次数。比如值为 3 时，`A -> B` 在同一个 round 内最多执行 3 次，`C -> B` 也可独立执行 3 次。
- `max_rounds`：`$new-round` 可打开的新 round 最大次数，初始 round 不计入。

超过任一限制时，runtime 不再创建新的 attempt / round，当前 workflow 以 failure 结束。

## 4. edge 语义

```json
{
  "from": "test",
  "to": "dev",
  "on": "failure",
  "session": "continue"
}
```

- `from`：真实 worker 节点 id。
- `to`：真实 worker 节点 id、`$end` 或 `$new-round`。
- `on`：当前节点归纳出的 outcome。
- `session`：可选，`new` 或 `continue`。

## 5. outcome 到控制决策

| outcome | 有匹配 edge | 无匹配 edge |
| --- | --- | --- |
| `success` | 按 edge 跳转；`$end` 完成成功；`$new-round` 打开新 round | 暂停为错误阻塞 |
| `failure` | 按 edge 跳转；`$end` 完成失败；`$new-round` 打开新 round | 暂停为错误阻塞 |
| `invalid` | 按 edge 跳转；不能指向 `$end` | 暂停为错误阻塞 |
| `killed` | 不看 edge | run 完成 killed |
| `none` | 不看 edge | 暂停，等待外部继续或人工处理 |

## 6. 人工 check 与 AI 输出验证
- `manual_check=true`：worker 会话自然结束后暂停到 `WaitingForUserInput`，用户提交成功/失败后再按对应 edge 继续。
- `output + success_condition`：runtime 保存 AI 输出产物，并按成功条件归纳 outcome。
- 二者互斥；一个节点不能同时启用人工 check 和 AI 输出验证。

## 7. 新 round
`$new-round` 表示开启下一轮执行，entry 仍使用 workflow 的 `entry`。下一轮保留原始 requirement，并把上一轮失败节点的输出摘要作为反馈上下文提供给新的 worker 调用。

## 8. 校验要求
- `entry` 必须存在。
- 所有 edge source 必须是真实 worker 节点。
- edge target 必须是真实 worker 节点、`$end` 或 `$new-round`。
- `invalid -> $end` 非法。
- `session=continue` 不能指向 `$end` / `$new-round`。
- `session=continue` 的目标 provider 必须支持 continue session。
- `control.max_attempts` 与 `control.max_rounds` 可省略；声明时必须为正整数。
- 启用 `success_condition` 时必须声明 JSON `output`。
- `output.artifact` 必须与 `primary_artifact` 一致。
