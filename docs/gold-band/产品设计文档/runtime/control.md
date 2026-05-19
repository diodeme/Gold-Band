# Gold Band Runtime Control 规范

## 1. 一句话定义
Runtime Control 用来定义 Gold Band 如何根据节点执行结果，驱动 workflow 的状态流转。

它回答的核心问题是：

> **当前节点执行完以后，下一步到底去哪、是否重试、是否开新一轮、是否暂停、是否结束。**

---

## 2. 控制层职责
控制层负责：

- 判断当前节点执行结果的控制语义
- 根据 edge / defaults / runtime policy 计算下一步
- 决定是：
  - 新建 attempt
  - 进入下一个 node
  - 新建 round
  - pause run
  - complete run

控制层不负责：

- 直接解析 provider raw stream
- 直接理解 provider-specific 中间事件
- 直接决定 artifact 内容是否“语义正确”

换句话说：

> **控制层只基于 canonical result 和 runtime policy 做流转，不基于 raw stream 做流转。**

---

## 3. 控制层的输入
控制层最少依赖以下输入：

### 3.1 当前节点定义
- 当前 node 是什么
- 当前 node 的类型是什么（`worker / exec / verify`）
- 当前 node 的配置是什么

### 3.2 当前执行结果
- 当前 attempt 的 canonical result
- 该 result 是否有效
- runtime 对该结果归纳出的 outcome

### 3.3 transition policy
- edge 定义
- `onAcceptanceFailure`
- retry / session policy
- node / workflow 默认策略

### 3.4 当前运行态
- 当前 run / round / attempt 编号
- 当前 run 是否已 pause
- 当前 run 是否处于错误阻塞态

### 3.5 prompt 前序链输入
`round.trace` 是当前节点 system prompt 中“前序运行节点链”的权威来源。

runtime 在调用 provider 前应：
- 读取当前 run 已完成的历史 rounds，并按 `round.index` 排序
- 使用内存中的当前 round trace 覆盖磁盘上的当前 round
- 按 `trace.sequence` 展开执行链
- 截止到当前 `node / attempt` 之前
- 用下一条 trace 的 `edge_outcome` 表达分支方向
- 用节点 DSL 判断分支原因是普通节点、人工 check，还是节点输出检查

该信息只用于帮助 agent 理解“为什么走到当前节点”，不替代控制层的 edge 计算。

---

## 4. 节点终局 outcome 与 `paused` 状态
控制层不直接吃原始文件细节，而吃 runtime 归纳后的控制语义。

MVP 建议将两类概念分开：

- `status`：生命周期状态，使用 `running | paused | completed`
- `outcome`：终局结果，使用 `success | failure | invalid`

其中：
- `paused` **不是 outcome**，而是可恢复的生命周期状态
- 显式 `run kill` 产生的 `killed` 属于 run / round / node 的终局状态值，不属于节点 canonical result 的归纳 outcome

### 4.1 `success`
表示当前节点执行完成，且产出满足本节点的最小 contract。

### 4.2 `failure`
表示当前 attempt 已结束，但节点目标未达成，或 provider 执行失败 / 异常结束。

说明：
- 对 `exec / verify`，它通常表示节点目标未通过
- 对 `worker`，它通常表示 provider 调用异常结束、执行失败，或节点目标没有完成
- `failure` 不应用来表达“返回包结构不合法”这类 contract 问题

### 4.3 `invalid`
表示当前 attempt 已结束，但其结果不满足最小 contract，例如结果缺失、schema 不合法、关键信息不匹配或 provider 最小返回包不合法。

### 4.4 `paused`
表示当前 attempt 尚未结束，但 runtime 观测到它进入可恢复的系统挂起态。

MVP 中：
- 不提供用户显式 `pause` 命令
- `paused` 只表示系统挂起，不表示终局结果
- `paused` 的典型来源只有：
  - `process_interrupted`
  - `waiting_for_user_input`
  - `error_blocked`

`continue` 只暴露给可恢复的中途暂停态：run / round / 当前 node 必须仍处于 `paused`，且 run 没有终局 outcome，暂停原因必须是 `process_interrupted`、`waiting_for_user_input` 或 `error_blocked`。桌面端应将 `error_blocked` 展示为错误阻塞而不是普通已暂停，但仍保留用户显式继续入口；已经 `completed` 且 outcome 为 `success / failure / killed` 的 round 不展示继续入口。

ACP provider 的 continue 必须恢复既有 session：runtime 使用当前 attempt 的 `worker-ref.json.continue_ref` 执行 `session/load`，加载失败即阻塞，不允许新建 session 后发送短 prompt。恢复成功后，runtime 发送本地化用户 prompt：中文为 `继续`，英文为 `Continue`。每次继续都必须携带新的 prompt identity（如 `promptId`）并写入 synthetic `goldBandPrompt` 事件元数据，供桌面端把“新的继续回合”与历史同文本继续回合区分开，避免错误复用旧回合计时或把多次继续合并成一条消息。

桌面客户端关闭时，应用壳需要 best-effort 停止所有仍处于 `running` 的 run：先写入 ACP cancel 标记并取消 pending permission；运行中的 ACP runtime 发现该标记后，必须发送不带 `id` 的 JSON-RPC notification `session/cancel`，不能把它作为 request 等待 adapter 返回；随后再清理 provider 进程树，最后将 run / round / 当前 node 收束为 `completed + killed`。已 `paused` 或 `completed` 的 run 不在关闭时自动改写。

---

## 5. 三类节点如何归纳 outcome

### 5.1 `worker`
`worker` 节点执行后，runtime 至少检查：

- 若当前节点声明了 `primaryArtifact`：
  - 是否返回了 `primaryArtifact`
  - 返回的 `primaryArtifact.name` 是否匹配当前节点声明的 `primaryArtifact`
  - 该 artifact 是否满足最小 schema
- 若当前节点未声明 `primaryArtifact`：
  - runtime 不要求产出 canonical artifact
  - runtime 只依据 provider invocation 的完成状态归纳 `success / failure / paused`
  - 只有 provider adapter 返回包本身不合法时，才归为 `invalid`

归纳规则建议：

- 已声明 `primaryArtifact`，且产物存在且合法 -> `success`
- 未声明 `primaryArtifact`，且 provider 调用成功完成 -> `success`
- provider 执行被中断且当前 attempt 可恢复 -> 当前 attempt 进入 `paused`
- provider 执行异常结束，或调用失败 -> `failure`
- 已声明 `primaryArtifact`，但产物缺失 / 名称不匹配 / schema 不合法 -> `invalid`
- 未声明 `primaryArtifact`，但 provider adapter 返回包本身不合法 -> `invalid`

补充语义：
- `worker.success`：自动进入普通 downstream edge
- `worker.paused`：不自动流转，等待用户执行 `continue`
- `worker.failure`：建议将 run / round / node 置为 `paused + error_blocked`，等待用户决定是否 `retry`
- `worker.invalid`：建议将 run / round / node 置为 `paused + error_blocked`，等待用户决定是修正当前产物后 `continue`，还是直接 `retry`

### 5.2 `exec`
`exec` 节点执行后，runtime 依据 `exec-result.json` 归纳：

- `exec-result.status = success` -> `success`
- `exec-result.status = failure` -> `failure`
- `exec-result.json` 缺失或不合法 -> `invalid`

说明：
- `exec` 的整体成败直接以 canonical `exec-result.status` 为准
- `failure` 表示命令执行目标未通过
- `invalid` 表示 `exec-result` 自身不满足最小 contract

### 5.3 `verify`
`verify` 节点执行后，runtime 依据 `verify-result.json` 归纳：

- 验收通过 -> `success`
- 验收未通过 -> `failure`
- `verify-result.json` 缺失或不合法 -> `invalid`

说明：
- `verify.failure` 只表示“验收不通过”
- 是否自动进入大循环，由 `onAcceptanceFailure` 决定
- `verify.invalid` 不应被当作普通验收失败处理；它表示验收节点自身没有产出合法 contract

---

## 6. 最小状态机动作
首版控制层建议只做 6 类动作：

### 6.1 `startNode`
启动某个 node 的一次新 attempt。

### 6.2 `retryNode`
在当前 round 内，为某个 node 新建一次 attempt。

### 6.3 `transitionToNode`
流转到另一个 node。

### 6.4 `openNewRound`
新建一个 round，并将控制流指向大循环入口 node。

### 6.5 `pauseRun`
将 run 置为暂停，等待外部动作。

### 6.6 `completeRun`
将 run 置为完成状态。

补充：
- 当控制层命中 edge `to = "$end"` 时，runtime 应直接执行 `completeRun`
- MVP 中只应允许 `success -> "$end"` 或 `failure -> "$end"`
- `invalid` 不应作为 DSL 可感知的终止分支；它属于必须先被修复的中间阻塞态，不应用 `"$end"` 直接收束
- 若该 edge 的 `on = success`，则 run 以成功语义完成；若 `on = failure`，则 run 以失败语义完成

---

## 7. 小循环与大循环

## 7.1 小循环（repair loop）
小循环的典型触发条件：

- `exec.failure`
- `exec.invalid`

典型路径：

```text
worker -> exec -> worker
```

语义：
- 不新建 round
- 新建 attempt
- 按 edge 上的 session policy 决定新 attempt 是复用历史会话上下文，还是全新开始

### repair loop 的计数口径
repair loop 统计的不是 attempt 数，也不是完整闭环数；它统计的是：

> **在同一个 round 内，控制层因 `exec.failure` 或 `exec.invalid`，实际决定回到某个 `worker` 的次数。**

也就是说，每次真正发生：
- `exec` 产出 `failure` / `invalid`
- 控制层决定沿 repair 路径返回 `worker`
- runtime 为该 `worker` 新建下一次 attempt

此时 repair loop 计数 +1。

### 小循环控制规则
- `exec.failure` -> 回到指定 `worker`
- `exec.invalid` -> 若显式存在对应 edge，则按该 edge 执行
- `exec.invalid` -> 若未显式声明 edge，默认回到 `planFrom` 指向的 `worker`；默认优先尝试 `session = continue`，若目标 provider 不支持 continue，则自动降级为 `new`
- `worker.failure` / `worker.invalid` / `worker.paused` 不进入 repair loop，也不直接进下游
- `verify.invalid` 不属于小循环；它不应回到 `worker` 进入 repair loop

MVP 建议：
- `worker.success` 是 `worker` 唯一自动进入普通 downstream edge 的 outcome
- `worker.paused` 由用户执行 `continue`
- `worker.failure` 由用户决定是否 `retry`
- `worker.invalid` 由用户决定是修正当前 attempt 产物后 `continue`，还是直接 `retry`

首版建议：
- `exec.failure -> worker`：计入 repair loop
- `exec.invalid -> worker`：计入 repair loop
- `worker.failure`：不计入 repair loop
- `worker.invalid`：不计入 repair loop
- `verify.failure`：不计入 repair loop
- `verify.invalid`：不计入 repair loop

## 7.2 大循环（acceptance loop）
大循环的典型触发条件：

- `verify.failure`

典型路径：

```text
worker -> exec -> verify -> (new round) -> entry
```

语义：
- 新建 round
- 回到 `workflow.entry`
- 原始 requirement 保持不变
- 最新 `verify-result` 作为下一轮 `worker` 的直接反馈输入
- 这表示开启了新一轮“实现 -> 执行 -> 验收”

### acceptance loop 的计数口径
acceptance loop 统计的不是 round 总数；它统计的是：

> **在同一个 run 内，控制层因 `verify.failure`，实际创建新的 acceptance round 的次数。**

也就是说，每次真正发生：
- `verify` 产出 `failure`
- 控制层决定继续下一轮
- runtime 新建一个新的 round 并回到 `workflow.entry`

此时 acceptance loop 计数 +1。

首版建议：
- `round-001` 是初始执行，不计入 acceptance loop
- `verify.failure + auto_loop`：新 round 创建时计入 acceptance loop
- `verify.failure + stop`：不计入 acceptance loop
- `verify.invalid`：不计入 acceptance loop

---

## 8. `onAcceptanceFailure` 控制策略
首版建议支持：

- `auto_loop`
- `stop`

### 8.1 `auto_loop`
- `verify.failure` 后自动新建 round
- 再流转到 `workflow.entry`
- 下一轮保持原始 requirement 不变，并直接携带最新 `verify-result`
- 新 round 真正创建时，acceptance loop 计数 +1

### 8.2 `stop`
- `verify.failure` 后直接结束 run
- run 以“验收失败”语义完成
- 不消耗 acceptance loop 配额

### 8.3 `verify.invalid` 的默认处理
`verify.invalid` 与 `verify.failure` 语义不同。

建议默认规则：
- `verify.failure`：表示验收结论为“不通过”，因此可进入 `auto_loop | stop` 分支
- `verify.invalid`：表示验收节点未产出合法 `verify-result`，属于 contract / runtime 问题

因此首版建议：
- `verify.invalid` 不进入 acceptance loop
- `verify.invalid` 不受 `onAcceptanceFailure` 控制
- `verify.invalid` 应直接使 run 进入阻塞态或失败完成

MVP 推荐行为：
- 将 run / round / 当前 verify attempt 统一置为 `paused + error_blocked`
- 不进入 acceptance loop
- 等待外部修复后再 `continue` 或 `stop`

---

## 9. pause / continue / retry 模型

## 9.1 run 状态
首版 run 至少应有：

- `running`
- `paused`
- `completed`

统一约束建议：
- `status != completed` 时，`outcome = null`
- `status = completed` 时，`outcome` 必须为终局值

## 9.2 pause 原因
建议最少记录：

- `process_interrupted`
- `error_blocked`

说明：
- `process_interrupted`：底层执行进程被 Ctrl+C、CLI 退出或宿主进程中断，但当前 attempt 仍保留为可继续状态
- `error_blocked`：运行遇到明确阻塞错误，等待外部修复后再继续或重试

MVP 约束：
- 不提供用户显式 `pause` 命令
- `paused` 只表示 runtime 观测到的系统挂起态

## 9.3 continue / retry 的区别
### `continue`
对当前 attempt 继续执行或重新结算，但不新建 attempt。

它在 MVP 中分成两类：

1. **resume current provider session**
   - 典型场景：`worker.paused`
   - runtime 尝试从当前 provider 会话断点恢复当前 attempt

2. **re-evaluate current attempt**
   - 典型场景：`worker.invalid`
   - 用户手动修正当前 attempt 产物后，runtime 重新校验当前 attempt，并决定是否进入下游
   - 此模式下不重新调用 provider

补充规则：
- `continue` 的对象是“当前 attempt”，不是任意指定位置
- `continue` 不创建新的 attempt，也不创建新的 round
- `continue` 是 runtime 控制动作，不等同于 provider 的 `sessionMode = continue`
- 若 `resume current provider session` 所需的 continue 能力不可用、`continueRef` 缺失、或 provider resume 失败，runtime 应将当前 run 保持在 `paused + error_blocked`，并明确报错，而不应偷偷降级为 `retry`
- 对已满足成功条件的 attempt，runtime 应自动沿 edge 流转，而不是等待用户 `continue`
- 若当前 attempt 重新结算后仍不满足下游条件，则 run 继续停在当前 node
- 若当前 attempt 既不是可恢复的 `paused`，也不是可重新结算的 `completed + invalid`，则 `continue` 应直接拒绝执行

### `retry`
对当前 node 重新发起一次新的 attempt。

它的典型场景包括：
- `worker.failure`：外部问题修复后，重新发起当前 node
- `worker.invalid`：放弃当前 attempt，让 provider 重新生成一版输出

补充规则：
- `retry` 一定创建新的 attempt
- `retry` 保持当前 round 不变
- `retry` 使用当前 node 的稳定输入重新调用 provider
- 手动 `retry` 默认使用 `sessionMode = new`
- 只有 workflow edge 明确声明 `session = continue` 时，runtime 才应请求历史 provider 会话复用

---

## 10. run 完成条件
run 在以下情况下进入完成态：

### 10.1 成功完成
- 终点 node 达成成功条件
- 或 `verify.success`
- 或 workflow 通过 edge 明确进入终止目标 `"$end"`

### 10.2 失败完成
- `verify.failure + onAcceptanceFailure = stop`
- 达到 retry / repair / acceptance 上限且不再继续
- 发生无法恢复的 contract / runtime 错误

### 10.3 显式停止完成
- 用户执行 `run kill`
- run 进入 `completed`
- run 的终局 `outcome = killed`
- 当前 round / node 若需要同步落盘，也应以 `killed` 作为终局值

---

## 11. MVP transition table

下表只描述首版默认行为，作为实现时的最小确定性规则。

| 当前 node | 当前状态/结果 | 用户动作 | 默认控制动作 | 新建 attempt | 新建 round | run 状态变化 | 备注 |
|---|---|---|---|---|---|---|---|
| worker | `completed + success` | 无 | `transitionToNode` | 否 | 否 | 保持 `running` | 自动进入 downstream edge；未声明 `primaryArtifact` 时可仅由调用成功归纳 |
| worker | `paused + null` | `continue` | 恢复当前 provider 会话 | 否 | 否 | `paused -> running` | 仅适用于 `process_interrupted` |
| worker | `completed + failure` | 无 | `pauseRun(error_blocked)` | 否 | 否 | `running -> paused` | 等待用户 `retry` |
| worker | `completed + failure` | `retry` | `retryNode` | 是 | 否 | `paused -> running` | 手动 retry 默认 `session = new` |
| worker | `completed + invalid` | 无 | `pauseRun(error_blocked)` | 否 | 否 | `running -> paused` | 等待人工修正或重试 |
| worker | `completed + invalid` | `continue` | 重新校验当前 attempt 产物 | 否 | 否 | 维持 `paused` 或回到 `running` | 不重新调用 provider；典型为人工修正当前产物后继续 |
| worker | `completed + invalid` | `retry` | `retryNode` | 是 | 否 | `paused -> running` | 放弃当前 attempt |
| exec | `completed + success` | 无 | `transitionToNode` | 否 | 否 | 保持 `running` | 自动进入 downstream edge |
| exec | `completed + failure` | 无 | repair 路径回到 worker | 是 | 否 | 保持 `running` | repair loop +1；若无 repair 路径或超预算则失败完成 |
| exec | `completed + invalid` | 无 | 优先按显式 repair edge，否则默认回到 `planFrom` 对应 worker | 是 | 否 | 保持 `running` | 默认优先尝试 `continue`，不支持则降级为 `new` |
| verify | `completed + success` | 无 | `completeRun(success)` | 否 | 否 | `running -> completed` | run 成功完成 |
| verify | `completed + failure` | 无 | `openNewRound` 或 `completeRun(failure)` | 否 | 取决于 `onAcceptanceFailure` | 保持 `running` 或进入 `completed` | `auto_loop` 时 acceptance loop +1 |
| verify | `completed + invalid` | 无 | `pauseRun(error_blocked)` | 否 | 否 | `running -> paused` | 不进入 acceptance loop |
| 任意当前 node | 任意非完成态 | `kill` | `completeRun(killed)` | 否 | 否 | `running/paused -> completed` | 显式终止统一使用 `killed` |

## 12. 一个最小控制示例

```text
entry -> worker(develop) -> exec(run) -> verify(accept)
```

### 正常路径
- `worker.success` -> `exec`
- `exec.success` -> `verify`
- `verify.success` -> `completeRun(success)`

### 小循环
- `exec.failure` -> `retryNode(worker)`
- `exec.invalid` -> 显式 repair edge 或默认回到 `planFrom` 对应 worker
- 不新建 round
- 每次实际从 `exec` 回到 `worker`，repair loop 计数 +1

### 大循环
- `verify.failure + onAcceptanceFailure=auto_loop`
- `openNewRound`
- `transitionToNode(worker)`
- 每次实际创建新的 round，acceptance loop 计数 +1

### 直接停止
- `verify.failure + onAcceptanceFailure=stop`
- `completeRun(failure)`

### 显式终止边
- 任一节点命中 `to = "$end"` 的合法 edge
- runtime 直接 `completeRun`
- 终局语义由触发该 edge 的 outcome 决定；MVP 中最常见的是 `success -> "$end"`

---

## 13. 与其他文档的关系
- [Runtime 概览](overview.md)
- [目录布局](layout.md)
- [run.json](state/run.json.md)
- [round.json](state/round.json.md)
- [node.json](state/node.json.md)
- [Control DSL](../dsl/control.md)
- [Worker Invocation Contract](../provider/invocation.md)
- [exec-result](../dsl/artifacts/exec-result.md)
- [verify-result](../dsl/artifacts/verify-result.md)

---

## 14. 一句话总结

> **Gold Band 的控制层本质上是一个基于 canonical result 和 runtime policy 的小状态机：它不看 raw stream，只看当前 node、当前 outcome、当前 policy，然后决定是重试、流转、开新一轮、暂停，还是结束。这里 `failure` 表示目标未达成或执行失败，`invalid` 表示结果不满足最小 contract。**