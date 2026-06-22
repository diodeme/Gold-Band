# Runtime Control 规范

## 1. 定义
Runtime Control 是运行时状态机：它读取当前 worker 节点的 `NodeOutcome`，按 workflow edge 决定下一步，并负责 run / round / node 状态落盘。

## 2. 节点模型
当前 runtime 只执行 `worker` 节点。节点 outcome 来自三种路径：

1. provider 成功且无需产物校验：`success`。
2. AI 输出验证：读取 `output.artifact`，按 `success_condition` 得到 `success / failure`；声明了 `output.schema` 且输出不合法时进入内部 `invalid` 修复流程。
3. 人工 check：会话结束后暂停，用户提交成功或失败。

## 3. 控制决策

| 当前 outcome | 决策 |
| --- | --- |
| `success` | 查找 `on=success` edge；无 edge 则错误阻塞 |
| `failure` | 查找 `on=failure` edge；无 edge 则错误阻塞 |
| `invalid` | 不查找 edge；若来自 `output.schema` 不合法则同 attempt 隐藏追问修复，最多 3 次；修复耗尽后 run failure |
| `killed` | run 完成 killed |
| `None` | run 暂停，保留当前节点与 attempt |

edge target 规则：

- 指向 worker：创建目标节点的新 attempt 并继续执行。
- 指向 `$end`：根据 edge outcome 完成 run。
- 指向 `$new-round`：关闭当前 round，创建新 round，并从 workflow entry 重新开始；`success -> $new-round` 在 DSL 校验阶段被拒绝。

## 4. session 继承
- `session=new`：目标 worker 新开会话。
- `session=continue`：仅当目标 provider 支持 continue session 时可用。
- continue ref 来自目标 worker 节点当前最新 attempt 的 worker ref；找不到时降级为普通新会话上下文。
- 上一节点的 primary/output artifact 可作为 feedback summary 进入下一次 worker 调用。

## 5. attempt 限制
节点跳转不再使用 repair loop 概念，而由显式 edge 创建目标节点的新 attempt。例如：

```json
{ "from": "test", "to": "dev", "on": "failure", "session": "continue" }
```

`control.max_attempts` 表示当前 round 内的修复/重试预算，只统计由 `failure` 触发、且 edge 指向真实 worker 节点的修复跳转。正常 `success` 前进不消耗该预算；`output.schema` 不合法触发的隐藏追问不新增 attempt，也不消耗该预算。例如 `max_attempts = 1` 时，`test failure -> dev` 可修复一次，修复后的 `dev success -> test` 仍应继续执行。超过预算时 runtime 不再创建新的 attempt，当前 run / round 以 failure 结束，并写入结构化 `workflow_control_limit_exceeded` 事件用于 UI 展示停止原因。没有声明 `max_attempts` 时不限制。

## 6. 新 round
`$new-round` 用于表达验收类 worker 未通过后的下一轮执行：

```json
{ "from": "accept", "to": "$new-round", "on": "failure" }
```

新 round 使用同一 workflow snapshot，从 `entry` 重新开始，并把上一轮失败节点的输出摘要纳入反馈上下文。若 workflow 声明了 `control.max_rounds`，该值限制 `$new-round` 可打开的新 round 数，初始 round 不计入；超过限制时当前 run / round 以 failure 结束。

## 7. 人工 check 暂停
启用 `manual_check=true` 的 worker 在 provider 会话自然结束后进入：

- run: `paused`
- round: `paused`
- node: `paused`
- pause reason: `waiting-for-user-input`

人工 check 暂停不是 runtime continue：当前 ACP 会话的输入区保持可用，用户可以继续发送普通 ACP prompt 追问或补充上下文，这些消息不会触发 workflow edge。会话面板额外展示“成功 / 失败”判定按钮；只有用户点击其中一个按钮后，runtime 才写回 `NodeOutcome` 并继续按 edge 流转。

`manual_check_pending` 必须持久化在当前 attempt 的 `node.json` 中。应用关闭后再次打开，只要 run / round / node 仍处于上述暂停态且 `manual_check_pending=true`，会话面板仍应恢复判定按钮和可用输入区，点击成功或失败后继续推进 runtime。

## 8. 错误阻塞
以下情况进入 `paused + error_blocked`：

- edge 缺失导致无法决定下一步。
- provider 调用失败且无法恢复。
- AI 输出验证声明了产物但产物缺失。
- 输出结构或成功条件路径不满足 DSL 声明。

## 9. 状态一致性
每次节点进入、完成、暂停、跳转或打开新 round 时，runtime 必须同步更新：

- `run.json`
- `round.json`
- `node.json`
- round trace
- progress snapshot / run events
