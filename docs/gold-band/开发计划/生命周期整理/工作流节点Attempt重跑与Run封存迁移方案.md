# 工作流节点 Attempt 重跑与 Run 封存迁移方案

> 状态：待实施
>
> 方案日期：2026-08-10
>
> 适用范围：Conversation Workflow 模式；Direct、AUTO 不在范围内

## 1. 背景

Gold Band 当前已有两类容易被混淆的能力：

1. **普通停止 / 继续**：当前 workflow attempt 因用户停止或可恢复异常进入 `Paused + ProcessInterrupted/RuntimeAbnormal`，之后继续同一个 run、round、node、attempt 和 ACP session。
2. **整任务重跑**：停止当前正在运行的 run，并从任务工作流入口创建一个新 run；历史 run 保留。

本方案新增第三类能力：

> 用户可以在 Workflow 会话中选择一个已经正常完成的 attempt，从该节点位置创建新 run；新 run 使用任务最新工作流，继承目标 attempt 及之前的完整会话历史，并从目标节点创建全新 attempt 继续执行。

这不是普通 `retry`，也不是在原 run 中修改 `workflow.snapshot.json`。它是一次带历史前缀和 ACP session 所有权迁移的 **run replacement / fork handoff**。

用户侧核心语义：

- 仅 Workflow 模式支持，Direct 和 AUTO 不支持。
- 只能选择正常完成的 attempt；运行异常、用户停止、已被替代等非正常终态不允许作为普通节点重跑锚点。
- 新 run 使用点击时任务的最新 authoring workflow，后续 edge、Agent、模型、权限和节点配置都以新快照为准。
- 目标节点必须仍存在于最新工作流；不存在时拒绝，且不能先停止旧 run。
- 重跑不会回滚工作区已经发生的修改。
- 旧 run 的全部运行和 ACP 活动都会停止，交接完成后整个旧 run 只读。
- 新 run 继承目标 attempt 及之前的全部消息，包括已经发生的 same-session follow-up，并接管其中可继续 ACP session 的唯一写入权。
- 目标之后的旧路径不会进入新 run，只保留在只读旧 run 中。
- 无论目标位于哪个 round，新 run 都在相同 round 位置从目标节点重新执行。
- AI-DYNAMIC 外层作为一个 workflow 节点重跑；选择内部动态节点时，实际从所属外层 AI-DYNAMIC 的 bootstrap 重新开始。

## 2. 结论

采用以下唯一方案：

> **新建 successor run，冻结最新 workflow snapshot；先让 source run 全部会话静默，再以事务方式封存 source run、迁移历史前缀与 ACP continuation ownership，最后从目标节点创建新 attempt。**

不采用“同一 run 原地重试并替换工作流快照”，原因不是实现成本，而是它破坏 run 的根本不变量：

```text
一个 run = 一份不可变 workflow snapshot + 在该快照下产生的一段可审计执行历史
```

如果同一 run 中途换成最新工作流，则旧 attempt、后续 edge 和新 attempt 分属不同配置版本，而 `RunState.workflow_snapshot` 仍只有一个事实源。除非把 workflow revision 下沉到每个 trace step 和 attempt，否则无法准确回答“某一步为何被调度、当时使用了什么 edge 和配置”。这会把 run 从执行边界退化成任意会话容器，不符合当前领域设计。

## 3. 现有能力与选型

### 3.1 可复用的现有能力

- `reserve_next_run_dir`：同 task 下原子分配新 run id。
- `run_pause`、per-attempt provider control、ACP cancel/drain：停止 workflow runtime 和普通 follow-up。
- `workflow.snapshot.json`、`round.trace`、`node.json`：表达实际执行路径和节点配置。
- `worker-ref.json`：provider / ACP session identity 和 `continue_ref` 的事实源。
- `AttemptLocator`：统一表达普通 attempt 与 AI-DYNAMIC 内部 attempt。
- `ConversationSessionTreeVm`、`switch_conversation_session`：展示和切换 round/node/attempt。
- shadcn/ui `AlertDialog`、`Button`、`DropdownMenu`：实现确认和 attempt 操作入口。
- prompt-kit copy-in 会话组件：继续承载消息、工具卡片、Markdown 和 composer，不新增自研会话基础控件。

### 3.2 不引入通用工作流或事务框架

该问题需要的是 Gold Band 自身领域状态交接，不是通用 DAG 调度或分布式事务：

- 不引入 Temporal、Cadence、Airflow 等外部 workflow engine。
- 不引入 Kafka、NATS 或 event-sourcing 框架。
- 不使用通用 `copy_dir` 直接递归复制整个 run。
- 不用 SQLite 作为主事实源；SQLite 仍是可重建派生索引。

使用现有 Rust 文件状态、同卷 staging + atomic rename、任务级互斥锁和持久化 handoff journal 即可。必须复制的是经过领域筛选的稳定快照，不是任意目录树。

## 4. 核心领域不变量

### 4.1 Run 快照不变量

1. 每个 run 只有一份不可变 `workflow.snapshot.json`。
2. source run 继续使用原快照，历史展示永不被最新 authoring workflow 回写。
3. successor run 从最新 `authoring/workflow.json` 规范化、严格校验后冻结自己的快照。
4. successor run 的新 attempt 和后续调度只读取 successor snapshot。
5. 继承的历史 trace 是来源事实，不要求与最新工作流的旧前驱 edge 一致；fork boundary 解释了为何 successor 可以从非 entry 节点开始。

### 4.2 单写入所有权不变量

1. 同一个 ACP provider session 在 Gold Band 中任何时刻最多只有一个可写 owner locator。
2. source run 一旦封存，所有 runtime continue、same-session follow-up、权限响应、elicitation 响应和配置修改入口均不可写。
3. 被继承的可继续 session 只能由 successor run 持有 continuation lease。
4. source run 继续保留历史 `worker-ref` 和 session id 用于审计展示，但它们不再代表写权限。
5. 没有进入继承前缀的下游 session 被永久 revoke，不转移给 successor。

### 4.3 历史不变量

1. source run 不删除、不改写既有正常完成 attempt 的 outcome、消息和产物。
2. 继承 attempt 在 successor 中是封存时刻的不可变快照，不随 source run 变化。
3. successor 中的 inherited attempt 和 fresh attempt 必须有不同的本地 UUID。
4. inherited attempt 保留来源 locator、来源 UUID、原始时间、原 outcome 和原 `resolvedConfig`。
5. fresh attempt 使用最新工作流节点定义重新生成 `resolvedConfig`，不能复制目标旧 attempt 的运行配置。

### 4.4 运行推进不变量

1. successor 发布并启动前，source 已不可写。
2. source 迟到的 provider 成功结果不能推进 source edge，也不能写入 successor。
3. fresh attempt 成为 successor `currentAttempt` 后才能启动 provider。
4. 新 attempt 的 outcome 决定后续 edge；旧 target attempt 的 outcome 只属于历史。
5. 普通 Stop 仍保持 `Paused + ProcessInterrupted` 可继续语义；只有 run replacement handoff 才产生不可继续封存语义。

## 5. 术语与数据结构

### 5.1 定位对象

```rust
struct WorkflowAttemptRerunLocator {
    task_id: String,
    source_run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
}
```

- 普通 Worker：`outer_* = None`。
- AI-DYNAMIC 外层：使用普通顶层 locator。
- AI-DYNAMIC 内部 leaf：携带 outer locator；预检时转换为外层 AI-DYNAMIC effective target。

### 5.2 Run 来源

successor run 增加结构化来源，不使用散落 string：

```rust
enum RunOriginKind {
    Initial,
    TaskRerun,
    AttemptRerun,
}

struct AttemptRerunOrigin {
    source_run_id: String,
    requested_locator: AttemptLocator,
    effective_target: AttemptLocator,
    inherited_through_sequence: u32,
    source_workflow_hash: String,
    successor_workflow_hash: String,
    handoff_id: String,
}
```

`RunState` 持有 `origin` 或引用同 run 目录下的 `run-origin.json`。控制流、UI 和审计都读取该结构，不从事件文本猜测。

### 5.3 Source run 封存关系

普通 Stop 与永久封存必须分离。建议新增：

```rust
enum RunSealReason {
    ReplacedByAttemptRerun,
}

struct RunReplacementState {
    reason: RunSealReason,
    successor_run_id: String,
    handoff_id: String,
    sealed_at: String,
}
```

`RunState.replacement: Option<RunReplacementState>` 是“该 run 是否仍可写”的权威事实。所有写接口先经过统一 `ensure_run_writable(...)`，不能由前端是否展示 composer 决定。

source run 原本已经 `Completed + Success/Failure` 时，保留原 outcome，只增加 replacement/seal 事实；不能为了表示封存覆盖历史结果。

source run 原本仍为 `Running/Paused` 时，需要新增明确的终局 `RunOutcome::Superseded`，当前 round 同样以 `Superseded` 结束，当前未终态 node 使用 `NodeOutcome::Superseded`。不能复用：

- `Killed`：现有生命周期方案已经废弃新的 killed 写入，而且它无法表达 successor 关系。
- `Failure`：用户主动替代不等价于业务执行失败。
- `Paused + ProcessInterrupted`：paused 仍表示可 runtime continue，与永久封存冲突。

历史 killed 继续只读兼容；新 replacement 只写 `superseded`。

### 5.4 Attempt 来源

inherited attempt 增加：

```rust
enum AttemptOriginKind {
    Executed,
    Inherited,
}

struct InheritedAttemptOrigin {
    source_locator: AttemptLocator,
    source_attempt_uuid: Option<String>,
    inherited_at: String,
    handoff_id: String,
}
```

建议单独写 `attempt-origin.json`，避免把 session 迁移信息塞进 `NodeState.resolved_config`。

### 5.5 ACP continuation ownership

`worker-ref.json` 继续负责 session identity，但不再单独承担跨 run 写权限。新增 task-scoped ownership registry：

```rust
struct AcpContinuationLease {
    lease_id: String,
    provider_id: String,
    session_id_hash: String,
    owner: AttemptLocator,
    generation: u64,
    state: AcpContinuationLeaseState,
    transferred_from: Option<AttemptLocator>,
    updated_at: String,
}

enum AcpContinuationLeaseState {
    Owned,
    Transferring,
    Revoked,
}
```

要求：

- 文件名只使用 session id 的稳定 hash，不直接暴露原 session id。
- 更新在 task handoff lock 下使用临时文件原子替换。
- generation 每次迁移递增，防止迟到请求持有旧 owner。
- `submit_conversation_prompt`、runtime continue、permission、elicitation、session config 等所有写入口统一校验 lease owner 和 run seal。
- 非 continue-capable session 只迁移消息快照，不创建 owned lease。

## 6. 重跑资格与预检

### 6.1 模式

只允许 conversation metadata `runMode = workflow`。

- Direct：不展示入口，后端返回 `workflow-attempt-rerun.unsupported-mode`。
- AUTO：不展示入口，后端返回同一结构化错误，并在 params 中返回实际 mode。

### 6.2 普通 attempt

普通 attempt 必须满足：

- locator 属于 source run。
- `NodeState.status = Completed`。
- outcome 是正常 workflow completion：`Success | Failure | Invalid`。
- 不允许 `Killed | Superseded`。
- attempt 必须出现在所属 round 的 trace 中，而且 node/attempt/sequence 唯一一致。

节点完成后的 ACP follow-up 是否 active 不影响重跑资格；它会在 handoff quiesce 阶段被停止并固化到消息快照。

source run 自身可以是 running、paused 或 completed；资格只约束所选 attempt。source run 已经 sealed 时不允许再次创建不同 successor。

### 6.3 AI-DYNAMIC

AI-DYNAMIC 始终以外层 workflow node 为重跑单元。

#### 选择外层 attempt

- 外层 attempt 满足普通 completed attempt 规则。
- successor 创建新的外层 attempt。
- 不复用旧 `dynamic-graph.json` 作为新执行状态。
- 新 attempt 从 bootstrap 开始，重新冻结 allowed workflow snapshots 并生成动态图。

#### 选择内部 leaf

- requested inner attempt 必须正常 completed。
- 预检解析并校验 outer node/attempt 归属。
- effective target 转换为 outer AI-DYNAMIC attempt。
- UI 必须显示专用确认说明：无法只重跑动态内部节点，继续后将从所属 AI-DYNAMIC bootstrap 重新生成完整内部工作流。
- 旧 outer attempt 及其所有内部 leaf 在 source 中完整保留并封存；successor 把该 outer attempt 作为历史快照，然后创建新的 outer attempt。
- 如果旧 outer attempt 当时仍未终态，继承副本以 `Superseded` 表达它在 handoff 时被整体替代；requested inner leaf 继续保留原 completed outcome。

### 6.4 最新工作流预检

停止任何 source 活动之前必须完成：

1. 读取 task 当前 `authoring/workflow.json`。
2. 执行 legacy snapshot normalization。
3. 使用最新 Agent diagnostics 规范化模型字段。
4. 严格校验 DSL、edge、entry、control 和 AI-DYNAMIC allowed workflows。
5. 校验引用 Agent 和配置可用性。
6. 确认 effective target node id 仍存在。

允许 target 节点的类型、Agent、模型、权限、profile、output contract 和后续 edge 与 source snapshot 不同。fresh attempt 完全使用最新定义。

如果 target 不存在，返回 `workflow-attempt-rerun.target-missing-in-latest-workflow`。此时 source run、ACP prompt、follow-up 和 workspace 都不得发生变化。

## 7. 历史前缀规则

### 7.1 普通节点

设 target trace step 为 `(roundIndex, sequence)`：

- target round 之前的 round：完整继承。
- target round：只继承 trace sequence 小于等于 target 的步骤及其 attempt。
- target attempt：作为 inherited completed attempt 保留。
- target 之后的 trace、attempt、round：不进入 successor。
- target round 在 successor 中重新打开为 `Running + outcome=None`。
- 在 target node 下创建新的 fresh attempt，并追加 `edgeOutcome=attempt-rerun` trace step。

示例：

```text
source run-007 / round-002

A/attempt-001 completed
B/attempt-001 completed   <- target
C/attempt-001 completed
D/attempt-001 running

successor run-008 / round-002

A/attempt-001 inherited completed
B/attempt-001 inherited completed
B/attempt-002 fresh running
```

source 中的 C、D 继续可见但只读；它们的 session 不迁移。

### 7.2 AI-DYNAMIC

AI-DYNAMIC 外层 attempt 是不可拆分的 workflow history unit：

- 选择外层或任一内部 leaf，都以 outer attempt 所在 trace step 作为 fork boundary。
- outer attempt 的历史动态图、内部消息、产物和 follow-up 作为一个 inherited 历史容器迁移。
- fresh outer attempt 不读取 inherited dynamic graph 的 ready/running/currentNodeIds，不复用 worktree 和 child run。
- 新 bootstrap 根据最新外层配置重新开始；之后是否并行由新 dynamic control 的 `maxParallel` 决定。

### 7.3 Round 位置与 control budget

“仍在原 round 位置”与“沿用旧工作流控制预算”必须分开：

- round id/index 保持 target 所在位置，用于展示、前驱上下文和新 round 编号。
- `newRoundsOpened` / `acceptanceLoopsUsed` 只统计 successor 在 fork 之后实际新开的 round，初始值为 0。
- `control.max_rounds` 使用最新 workflow，并只限制 successor fork 后新开的 round。
- `control.max_attempts` 只统计 fork boundary 之后由最新 workflow failure edge 产生的修复跳转。
- inherited trace 不消耗 successor 最新 workflow 的 repair/new-round budget。

需要在 run origin 或 trace 中保存 fork sequence，控制计数不得再把 inherited step 与 successor executed step 混在一起统计。

## 8. 稳定会话快照

### 8.1 为什么不能直接复制目录

completed NodeState 不代表对应 ACP attempt 目录不再变化。用户可能在节点完成后继续 same-session follow-up，timeline、snapshot、raw、permission、elicitation 和 turn-files 都可能仍在写。

因此 handoff 必须先让 source run 下全部 active session 静默，再生成稳定快照。不能边复制边允许旧 follow-up 继续。

### 8.2 Quiesce 范围

必须枚举 source run 的全部活动，而不是只处理 `RunState.currentAttempt`：

- 当前普通 workflow attempt。
- 当前 AI-DYNAMIC 外层、内部 leaf 和 child workflow run。
- 所有 completed attempt 上仍在运行的 same-session follow-up。
- pending permission / elicitation waiter。
- 处于自动重试 backoff、finalizing 或 cancel drain 的 prompt。

处理规则：

1. 先设置 handoff write gate，拒绝新的 prompt/continue/permission/elicitation 请求。
2. workflow runtime 走现有 pause/interrupted 中断路径，阻止旧 executor 推进。
3. 每个 active prompt 发送一次 `session/cancel` 并 drain 到 cancelled/interrupted 或 deadline。
4. pending permission / elicitation 固化为 cancelled/declined 事实。
5. 持久化最终 ACP snapshot、timeline、turn-files change set 和 session metadata。
6. 从 adapter route map 解除 source attempt route；禁止 `session/delete`，保留可 `session/load` 的 session identity。
7. 记录每个 attempt 的稳定 watermark。

watermark 至少包括：

```rust
struct AttemptSnapshotWatermark {
    locator: AttemptLocator,
    newest_seq: Option<u64>,
    snapshot_revision: Option<u64>,
    worker_ref_hash: Option<String>,
    captured_at: String,
}
```

### 8.3 Quiesce 失败

- 在 source seal 提交前无法收敛某个 session：handoff 不提交，successor 不发布，返回结构化错误。
- 已经停止的 source prompt 保持普通 `Paused + ProcessInterrupted`，用户可以重试 handoff或继续旧 run。
- 不 kill adapter 伪装为成功，不复制仍在变化的文件。
- 一旦 source seal 已提交，不能回滚为可写；启动恢复必须继续完成 handoff。

## 9. 语义化物化规则

successor 不递归复制整个 source run，只物化 manifest 指定的 inherited attempts。

### 9.1 需要复制

- 重写后的 `node.json`。
- `attempt-origin.json`。
- `worker-ref.json` 和 continue ref。
- 最终 `acp.snapshot.json`、`acp.timeline.jsonl`。
- 仍有审计价值的 ACP raw / diagnostics / events 文件。
- 已终态 permission / elicitation 请求与响应事实。
- artifacts、attachments、turn-files manifest/CAS 引用。
- AI-DYNAMIC inherited outer attempt 下的完整历史 graph、内部 attempt、child run history 和 proposal 审计文件。

### 9.2 必须重建或重写

- run id、run UUID。
- round 的 run id、round UUID 和 origin。
- attempt 的 run id、attempt UUID 和 origin。
- `current`、active、pause、composer 等派生状态。
- session ownership lease owner/generation。
- successor run events、progress 和 lifecycle 事件。

inherited attempt 的 `resolvedConfig` 保持 source 原值；fresh target attempt 使用 latest workflow 新值。

### 9.3 禁止复制

- `provider.pid`。
- 内存锁、临时文件、staging 文件。
- 未完成 permission / elicitation signal。
- adapter connection handle/route。
- source run 当前 active 标记。
- source target 之后的 attempt、round 和 session lease。
- fresh AI-DYNAMIC 的旧 worktree、ready/running 调度状态和 allowed workflow snapshots。

### 9.4 大文件与 CAS

- 已由 task/attempt CAS 管理的内容迁移引用和 ownership，不重复复制 blob。
- 普通文件先复制到 staging，再校验 manifest 中的 size/hash。
- 不使用 hard link 连接仍可能被修改的 source 文件。
- successor 发布后 inherited 文件不可原地修改；后续 follow-up 写入 successor 自己的 overlay/timeline，不回写 source。

## 10. Handoff 事务

### 10.1 锁与幂等

使用 task-scoped handoff lock；同一 task 同时最多一个 run replacement transaction。

命令携带 `requestId`：

- 同 requestId 重试返回同一 transaction/successor。
- source 已有已提交 replacement 时返回既有 `successorRunId`，不能再创建第二个 successor。
- 不同请求竞争同一 source 时返回 `workflow-attempt-rerun.handoff-in-progress` 或 `source-run-sealed`。

### 10.2 持久化 journal

```rust
enum AttemptRerunHandoffPhase {
    Prepared,
    QuiescingSource,
    SourceQuiesced,
    PrefixMaterialized,
    OwnershipTransferring,
    SourceSealed,
    SuccessorPublished,
    SuccessorStarted,
    Completed,
}
```

journal 记录 source/successor、requested/effective locator、工作流 hash、manifest、watermark、lease generation、阶段和错误码。文件采用同目录临时文件原子替换。

### 10.3 执行顺序

```text
Acquire task handoff lock
  -> preflight latest workflow + target + agents
  -> reserve successor run id
  -> write handoff journal / staging successor
  -> enable source handoff write gate
  -> quiesce every source active session
  -> capture stable attempt watermarks
  -> materialize inherited prefix in staging
  -> prepare fresh target node from latest workflow
  -> transfer/revoke continuation leases
  -> terminalize nonterminal source runtime as Superseded
  -> persist source replacement seal
  -> atomically publish successor run
  -> emit handoff committed lifecycle event
  -> start fresh target attempt in background
  -> mark handoff completed
```

最新工作流校验、Agent 校验和 staging 准备必须先于 source stop。真正 session ownership transfer 和 source seal 必须在同一个不可并发写区间完成。

### 10.4 崩溃恢复

- `Prepared/QuiescingSource` 且 source 未 sealed：丢弃或复用 staging；source 仍可写，必要时保持普通 interrupted。
- `SourceQuiesced/PrefixMaterialized` 且未 sealed：可以继续 transaction，也可以安全失败并让用户恢复 source。
- `OwnershipTransferring`：根据 lease generation 和 journal 幂等补齐，不能同时把 source/successor判为 owner。
- `SourceSealed` 之后：禁止恢复 source 写权限；启动时必须继续发布/启动 successor。
- `SuccessorPublished` 但未 started：根据 fresh current attempt 和 event id 幂等启动一次。
- recovery 不重复发送用户 prompt，不重复创建 attempt，不重复发系统通知。

## 11. 状态迁移

### 11.1 Source 已完成

如果 source run 原本已经 completed：

- 保留 `RunStatus::Completed` 和原 `RunOutcome::Success/Failure`。
- 增加 replacement seal。
- 停止并终态化所有 completed attempt follow-up。
- 转移继承前缀 session lease，撤销下游 session lease。
- lifecycle/composer 派生为 sealed/read-only。

### 11.2 Source 仍在运行或暂停

- source current runtime 先按 interruption 机制停止，确保 provider 不再写。
- handoff commit 时 run/当前 round 转为 `Completed + Superseded`。
- 当前未终态 node 转为 `Completed + Superseded`。
- 已完成历史 node 保持原 outcome。
- `pauseReason = None`，因为 sealed source 不再是可继续 paused 状态。
- `currentRound/currentNode/currentAttempt` 保留最后 locator 供审计。

### 11.3 Successor

- `RunStatus::Running`、`outcome=None`、`pauseReason=None`。
- `workflowSnapshot` 指向最新冻结文件。
- `currentRound` 指向 target round。
- `currentNode` 指向 latest workflow 中同 id 节点。
- `currentAttempt` 指向 fresh attempt。
- target 之前的 round 保持 inherited completed 状态。
- target round 为 running，outcome=None。
- inherited attempts 保持 source 历史状态。
- fresh target attempt 为 running，outcome=None，finishedAt=None。
- `lastExecutedNode` 指向 inherited target 历史 attempt，但只作为 predecessor/观察快照，不参与 edge 决策。

## 12. 调度与上下文

### 12.1 Fresh target

fresh target 使用：

- latest workflow node type。
- latest Agent/profile/model/permission/config options。
- latest output contract。
- `SessionMode::New`，不继续 target 历史 session 作为 workflow execution。

目标历史 session 的 continuation lease 仍可以在 successor 会话树中用于用户主动 follow-up；它与 fresh workflow attempt 是两个明确 leaf，不能把 follow-up结果当成 fresh attempt 的 workflow outcome。

### 12.2 前驱上下文

现有前驱 context builder 需要识别 inherited trace：

- 读取 successor 本地物化的历史 artifact、attachment 和 normalized output。
- 使用 fork boundary 之前的实际 trace 作为历史前驱。
- fresh target 自己的历史旧 attempt 不能作为自己的普通前驱重复注入；它只用于会话历史与 rerun provenance。
- target 完成后的下一节点只消费 successor fork 后的新 target attempt，不消费 source 中已丢弃的下游路径。

AI-DYNAMIC fresh bootstrap 不继承旧动态图控制状态，但可以按普通前驱规则读取外层节点之前的稳定历史上下文。

### 12.3 迟到结果门禁

orchestrator、node executor、AI-DYNAMIC scheduler 和 ACP callback 在持久化完成或推进 edge 前必须校验：

- run 未 sealed。
- attempt 仍是对应 runtime owner/current。
- continuation lease generation 未过期。
- handoff write gate 未关闭该 source。

任一不满足时只做 best-effort drain/diagnostic，不写业务状态、不发后继节点、不覆盖 successor 文件。

## 13. 后端接口

### 13.1 预检

```rust
validate_workflow_attempt_rerun(input) -> WorkflowAttemptRerunValidationVm
```

返回：

```ts
interface WorkflowAttemptRerunValidationVm {
  valid: boolean;
  code?: string | null;
  requestedTarget: AttemptLocatorVm;
  effectiveTarget?: AttemptLocatorVm | null;
  effectiveNodeLabel?: string | null;
  dynamicBootstrapRestart: boolean;
  activeSessionCount: number;
  inheritedSessionCount: number;
  discardedDownstreamSessionCount: number;
  latestWorkflowHash?: string | null;
  validationToken?: string | null;
}
```

validation token 绑定 source revision、target、latest workflow hash 和过期时间。执行接口仍需重新校验，token 只用于发现弹窗期间的状态变化，不代替后端授权。

### 13.2 执行

```rust
rerun_workflow_attempt(input) -> WorkflowAttemptRerunStartedVm
```

输入至少包含 projectId、taskId、source locator、requestId 和 validationToken。成功返回 successor `ConversationRunVm` 或能够立即 deep link 的 project/task/run/current locator。

命令只在 handoff committed、successor 已发布后返回成功；fresh provider 在后台启动，UI 通过现有 ACP live update 和 run state update 收敛。

### 13.3 写入门禁

以下所有入口调用统一 guard：

- `submit_conversation_prompt`
- runtime continue
- direct ACP prompt narrow entry
- stop/permission/elicitation response
- model/permission/config option mutation
- manual check submission
- dynamic inner continue

source sealed 时返回同一结构化错误并附 successor run id，不能让各入口各自猜测。

## 14. 错误码

后端只返回 code 和 params，用户文案由前端中英文 i18n 处理。

| code | params | 含义 |
|---|---|---|
| `workflow-attempt-rerun.unsupported-mode` | `mode` | Direct/AUTO 不支持 |
| `workflow-attempt-rerun.attempt-not-found` | locator | attempt 不存在或归属错误 |
| `workflow-attempt-rerun.attempt-not-completed` | status/outcome | 目标不是正常 completed attempt |
| `workflow-attempt-rerun.dynamic-owner-not-found` | outer locator | 内部 leaf 无合法外层 AI-DYNAMIC |
| `workflow-attempt-rerun.latest-workflow-invalid` | validation codes | 最新工作流无效 |
| `workflow-attempt-rerun.target-missing-in-latest-workflow` | nodeId | 最新工作流已删除目标节点 |
| `workflow-attempt-rerun.agent-unavailable` | agentId/nodeId | 最新节点 Agent 不可用 |
| `workflow-attempt-rerun.source-run-sealed` | successorRunId | source 已被替代 |
| `workflow-attempt-rerun.handoff-in-progress` | handoffId | 同 task 已有交接事务 |
| `workflow-attempt-rerun.source-changed` | expected/actual revision | 弹窗期间 source 变化 |
| `workflow-attempt-rerun.session-quiesce-failed` | failed locators | 无法稳定停止全部 session |
| `workflow-attempt-rerun.session-not-transferable` | locator/provider | 应可继续但 identity/lease 不完整 |
| `workflow-attempt-rerun.snapshot-materialize-failed` | handoffId | 历史快照物化失败 |
| `workflow-attempt-rerun.recovery-required` | handoffId | seal 后事务需要恢复完成 |
| `conversation.session-sealed` | successorRunId | 旧 run 会话只读 |
| `conversation.session-ownership-mismatch` | owner locator/generation | session 写入 owner 不匹配 |

错误 params 不包含面向用户的自然语言，也不泄露敏感 continue ref/session id。

## 15. 前端交互

### 15.1 入口

- Workflow 会话 session switcher 的 completed attempt 提供“从此节点重跑”。
- Round 实际工作图的 attempt/node上下文菜单复用同一动作。
- Direct/AUTO 不渲染入口。
- AI-DYNAMIC 内部 leaf 可以触发，但 UI 使用预检返回的 effective outer target。

入口使用现有 shadcn/ui copy-in 组件，不自研菜单和弹窗。

### 15.2 普通确认

必须包含用户指定文案：

> 当前重跑不会回滚工作区中已修改的代码，确定要重跑吗？

同时展示对决策必要的影响：

> 旧 Run 中正在进行的会话将停止，旧 Run 将变为只读。目标节点之后的会话不会迁移到新 Run。

确认动作：“停止旧 Run 并重跑”。

### 15.3 AI-DYNAMIC 内部确认

额外展示：

> AI-DYNAMIC 内部节点由运行时动态生成，无法单独重跑。继续后将从所属的 AI-DYNAMIC 节点重新执行 bootstrap，并重新生成内部工作流。

### 15.4 节点已删除

预检发现节点不在最新工作流时，不展示危险确认按钮，直接使用 AlertDialog 告知：

> 该节点已不在最新工作流中，无法从此处重跑。

source run 不停止。

### 15.5 交接中与完成

- 用户确认后锁定重复提交，显示单一 handoff 处理中状态。
- 不在消息流中插入“处理中”卡片；状态放在操作按钮/弹窗和 composer 区域。
- 成功后 deep link 到 successor run，并自动选中 fresh target attempt。
- source run 页面如果仍打开，收到 seal event 后立即变为只读，并提供“打开新 Run”。
- 不乐观伪造 successor session tree；以 committed command response 和后端 live update 为准。

## 16. 生命周期事件与通知

新增语义事件：

```text
RunReplacementRequested
RunSourceQuiesced
RunSealed
ContinuationOwnershipTransferred
RunSuccessorPublished
WorkflowAttemptRerunStarted
RunReplacementFailed
```

- lifecycle bus 只分发已发生事实，不负责修改 handoff 状态。
- metrics、UI refresh、audit log 可订阅。
- 不把每个被取消的 follow-up 转成多条系统通知。
- handoff 成功只需一次 UI refresh；是否发送系统通知沿用桌面注意力规则。
- eventId/handoffId 用于幂等和去重。

## 17. 安全与一致性

- 所有 locator 重新通过 `AppPaths` 解析，拒绝调用方提供物理路径。
- materialize 前 canonicalize source/destination，确保都位于同 task runtime store。
- 不复制 symlink 指向的外部内容；已有受控 asset/CAS locator 按其 ownership 规则处理。
- continue ref/session id 不进入日志、错误 params 或前端 validation VM。
- source seal 和 session lease 都由后端校验；前端只读状态不是安全边界。
- workspace 不做回滚、stash、reset 或 checkout；弹窗明确告知。
- 重跑不启动可见终端；后续外部命令继续统一通过 `process::background_command()`。

## 18. 实现范围

### 18.1 Runtime / domain

- `RunOutcome::Superseded`、`NodeOutcome::Superseded` 或等价 typed 终态。
- `RunReplacementState`、`AttemptRerunOrigin`、`InheritedAttemptOrigin`。
- source writable guard。
- fork-aware control counters 和 trace origin。
- fresh target prepare/drive 入口。

### 18.2 Storage

- task handoff journal 和 lock。
- semantic prefix manifest/materializer。
- attempt snapshot watermark。
- continuation lease registry。
- staging/publish/recovery。

### 18.3 ACP

- run-scoped 全 active session enumeration。
- cancel/drain/final snapshot。
- adapter route detach。
- lease owner/generation guard。
- inherited session 在 successor 下 `session/load` 和新 follow-up overlay。

### 18.4 AI-DYNAMIC

- inner locator 到 outer effective target。
- outer graph 全量静默与历史物化。
- child run/worktree 停止。
- fresh bootstrap，不复用旧 graph 状态。

### 18.5 Tauri / VM / API

- preflight + execute command。
- sealed source 和 successor link VM。
- inherited attempt origin/session ownership VM。
- 结构化错误映射。

### 18.6 Web

- attempt 菜单和 graph 入口。
- 两类 AlertDialog。
- handoff pending 防重。
- source 只读态和 successor 导航。
- 中文、英文 i18n。

### 18.7 旧入口清理

现有未完整实现、不能安全取消当前 attempt 或切换 identity 的 `run_retry/retry_run` 不作为本功能基础入口。实施时应删除或重构为内部 typed service，不能继续保留一个可绕过 seal/lease/handoff 的公开命令。

现有整任务 `rerun_conversation_task` 与节点 attempt rerun 语义不同：整任务重跑不继承 session，因此不自动获得 continuation ownership。两者可以复用 run id 分配、最新 workflow validation 和启动器，但不能共用一个含糊的“retry”接口。

## 19. 实施阶段

### Phase A：领域状态与写门禁

1. 增加 typed replacement/origin/superseded 状态。
2. 增加统一 `ensure_run_writable` 和 continuation lease guard。
3. 所有会话写入口接入 guard。
4. sealed source lifecycle/composer 派生为只读。

### Phase B：Quiesce 与稳定快照

1. 枚举 source run 全部 workflow/follow-up/dynamic 活动。
2. 接入 cancel/drain、pending intervention 终态化和 watermark。
3. 增加失败时保留普通 interrupted 可恢复语义。
4. 验证迟到 provider 结果不能推进。

### Phase C：Handoff journal 与历史物化

1. task-scoped lock、requestId 幂等、run id reservation。
2. staging、prefix manifest、hash 校验、atomic publish。
3. continuation ownership transfer/revoke。
4. crash recovery 覆盖全部 phase。

### Phase D：Successor 编排

1. latest workflow validation 和 target mapping。
2. fork-aware round/trace/control budget。
3. fresh target state、最新配置和后台 drive。
4. predecessor context 与 AI-DYNAMIC bootstrap。

### Phase E：接口和 UI

1. preflight/execute Tauri commands 和 browser mock。
2. session switcher/graph attempt 操作。
3. shadcn AlertDialog、中文/英文错误文案。
4. source 只读和 successor deep link。

### Phase F：删除旧链路、索引与文档

1. 删除不安全 `retry_run` 公开链路和失效前端入口。
2. 重建 successor attempt/session 的 SQLite 派生索引，避免重复把 source 统计为新执行。
3. 同步产品设计文档和 MVP 开发计划。
4. 完成 EXE/UI deep link 验证和测试资源清理。

## 20. 测试与验收

### 20.1 Domain 单元测试

- completed success/failure/invalid 可选，killed/superseded/paused/running 不可选。
- Direct/AUTO 拒绝。
- source completed 保留原 outcome 并 seal。
- source running/paused 终态为 superseded。
- sealed run 的所有写入口统一拒绝。
- lease generation/owner 校验和幂等 transfer/revoke。
- inherited/fresh UUID、origin 和 resolvedConfig 正确。

### 20.2 Prefix / round 测试

- target 位于 round-001、中间 round、最后 round。
- 同节点多次 attempt 时按 trace sequence 精确截断。
- target 前 round 完整、target round 截断、后续 round 不复制。
- successor 仍位于原 round index。
- inherited trace 不消耗 successor `max_attempts/max_rounds`。
- target fresh outcome 根据最新 edge 进入新后继。
- 最新 workflow 改 Agent/config/edge 后全部生效。
- 最新 workflow 删除 target 时在停止 source 前拒绝。

### 20.3 ACP 测试

- completed attempt 有 active follow-up：取消、drain、消息完整进入 successor。
- source 旧入口 follow-up/continue/permission/elicitation 全部返回 sealed。
- successor inherited session 可以 `session/load` 并继续。
- provider session 只有 successor 一个 owner。
- target 之后 session 被 revoke，不进入 successor。
- cancel deadline 失败时不发布 successor。
- 迟到 success/text/tool update 不改 source/successor业务状态。
- pending permission/elicitation 在快照中有明确 cancelled/declined 终态。

### 20.4 AI-DYNAMIC 测试

- 外层 completed attempt 从 bootstrap 重跑。
- 内部 completed leaf 映射 outer，并返回专用确认标记。
- outer 仍运行且有并行 sibling 时，全部静默并封存旧 graph。
- inherited graph 可只读查看。
- fresh outer 不复用旧 currentNodeIds、worktree、proposal、allowed snapshots。
- 新图按最新 `maxParallel` 和 Agent 配置执行。

### 20.5 Transaction / crash 测试

对每个 handoff phase 注入失败或进程重启：

- seal 前不出现两个 owner。
- seal 后不恢复 source 写权限。
- successor 最多发布一次、fresh attempt 最多创建/启动一次。
- 相同 requestId 返回同一 successor。
- 并发不同请求只有一个提交。
- staging/manifest/hash 损坏可诊断，不读取半成品 run。

### 20.6 接口层验收

必须通过 Tauri command/ViewModel 接口固化：

- preflight validation VM 的 effective target、session counts 和错误码。
- execute response deep link 到 fresh target。
- source `ConversationRunVm` sealed/read-only/successor link。
- successor tree 包含 inherited prefix 和 fresh attempt。
- inherited session switch 后消息、产物、附件、session id 正确。
- old run 写入返回 `conversation.session-sealed` 和 successor id。

### 20.7 前端测试与真实验证

- 普通确认文案包含“不回滚工作区”。
- AI-DYNAMIC 内部节点出现 bootstrap 专用提醒。
- target 已删除只显示拒绝原因，不执行停止。
- pending 状态阻止重复点击。
- source seal 后 composer 消失并出现“打开新 Run”。
- success 后自动导航并选中 fresh attempt。
- Direct/AUTO 无入口。

涉及 UI、交互和路由时，必须启动前端，优先使用 Codex 内置浏览器 deep link 到目标会话页面验证；只有必须验证 EXE 原生窗口或客户端级会话行为时才使用 computer-use。验证后关闭本次页面、进程和临时会话。

## 21. 文档同步清单

代码实施时必须同步维护：

### 产品设计文档

- `docs/gold-band/产品设计文档/runtime/state/run.json.md`
- `docs/gold-band/产品设计文档/runtime/state/round.json.md`
- `docs/gold-band/产品设计文档/runtime/state/node.json.md`
- `docs/gold-band/产品设计文档/runtime/layout.md`
- `docs/gold-band/产品设计文档/runtime/control.md`
- `docs/gold-band/产品设计文档/provider/worker-ref.md`
- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/产品设计文档/interaction/app/round-detail.md`
- AI-DYNAMIC 相关产品设计文档。

### 开发计划

- 本文实施状态。
- `docs/gold-band/开发计划/新UI/会话式主页实施进度.md`。
- 与 ACP stop、长连接和生命周期统一方案发生交叉的已实施结论。

新增或修改 UI 文案必须同步维护 zh-CN/en；本功能不新增 prompt。若未来需要 AI 判断迁移或修复，相关双语 prompt 必须放在 `src/prompts/zh-CN/...` 与 `src/prompts/en/...` 对称目录，不能硬编码在实现中。

## 22. 完成定义

只有同时满足以下条件，节点 attempt 重跑才算完成：

1. source run 全部 runtime/ACP 活动已稳定停止并永久只读。
2. 任一旧入口都无法继续写 source session。
3. 可继续 session 在 successor 中拥有唯一 continuation ownership，并包含封存前全部消息。
4. successor 使用最新 workflow snapshot，从相同 round 的目标节点创建 fresh attempt。
5. 最新 edge、Agent 和节点配置实际生效。
6. source 迟到结果不能推进或覆盖 successor。
7. AI-DYNAMIC 外层从 bootstrap 重建，内部选择有明确提醒。
8. handoff 在并发、失败和进程重启下保持单 successor、单 owner、可恢复。
9. 工作区不被回滚，用户在确认前得到明确提示。
10. 单元测试、接口层回归、前端 deep-link 验证和中英文文案全部通过。
