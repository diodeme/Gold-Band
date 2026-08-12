# 工作流 Runtime 执行阶段与 ACP 生命周期解耦技术方案

## 1. 背景

Gold Band 当前已经将“本轮 Agent 回复是否由 Runtime 消费”抽象为 `RuntimeControlled / NonRuntimeControlled`，并允许用户停止工作流后继续复用同一 ACP session 进行自由会话。该方向是正确的，但 Conversation lifecycle 的阶段投影仍存在一个根本性设计缺陷：部分代码使用 ACP session / turn 的终态推断工作流节点是否已经结束以及 Runtime 是否正在进入下一节点。

当前典型推断近似为：

```text
Runtime active + ACP terminal
  -> 当前节点的 Agent 已结束
  -> Runtime 正在拉起下一节点
```

该推断不成立。一次 ACP turn 结束可能来自：

- RuntimeControlled 业务 turn；
- RuntimeFinalize；
- RuntimeRepair；
- 用户停止工作流后的 NonRuntime 普通追问；
- manual check 等待期间的普通追问；
- 节点或 Run 结束后的 follow-up；
- Direct 会话中的任意一轮回复。

这些 turn 都可以把最新回复写成 `completed / cancelled / failed`，但只有 Runtime 在完成 artifact 消费、校验、人工判定和 outcome 提交后，工作流节点才真正完成。ACP turn 终态不能证明节点完成，更不能证明 Runtime 已进入 edge transition。

实际问题表现为：用户停止工作流并发送一条 NonRuntime 普通追问，Agent 回复结束后，持久化 ACP 状态出现 `completed`；随后用户点击继续工作流，在 Runtime resume prompt 尚未登记为 live activity 的短暂窗口内，节点已经恢复为 Running，但投影层仍读到上一条 NonRuntime turn 的 `completed`，于是错误展示“拉起下一节点中”。

本方案不针对该窗口增加一个排除条件，而是从领域模型上拆开：

1. Workflow Runtime 执行阶段；
2. Runtime 对当前 turn 的控制模式；
3. ACP live turn 活动；
4. ACP session 可用性与历史 turn 结果。

四类状态只能单向组合为 UI，不得相互替代或反向推导。

## 2. 设计判断

### 2.1 根因性质

该问题来源于根本性设计缺陷，而不是正确设计下的局部实现遗漏：

- 工作流节点属于 Runtime 领域；
- ACP turn 属于 Agent 会话领域；
- session 可复用性属于 Provider / ACP 连接领域；
- 是否消费本轮结果属于 Runtime Control 领域。

当前实现把不同领域中都叫作 `running / completed / paused` 的字段当成同一生命周期使用，造成跨领域反推。继续增加 `if NonRuntime`、`if manual check`、`if finalizing` 等特判，只会在新的阶段边界再次出现同类问题。

### 2.2 行业实践与组件选择

本方案采用状态机、聚合根和 CQRS/read model 中的通用原则：

- 一个业务事实只有一个权威写入者；
- 下游投影只能由权威事实生成；
- 观测数据不得反向控制业务状态；
- 跨文件落盘通过固定提交顺序、revision 和启动恢复保证 crash consistency；
- UI 不从低层 transport 状态猜测高层业务阶段。

Rust 状态机库可以约束内存中的 transition，但无法处理本项目的 JSON 原子写入、ACP cursor、artifact checkpoint、AI-DYNAMIC graph 和启动恢复边界。当前 Rust enum、领域 transition service、现有原子 JSON helper 与生命周期锁已经足够，不新增第三方依赖。

## 3. 目标

1. Workflow 节点是否完成只由 Runtime 权威状态决定。
2. `launching-next-node` 只由 Runtime 明确提交的执行阶段产生。
3. ACP turn 的 `completed / cancelled / failed` 不再影响 Workflow phase。
4. 停止后的 NonRuntime 普通追问无论如何结束，都保持 `Paused + ProcessInterrupted` 并等待用户显式继续。
5. Runtime resume 启动窗口不读取上一条 NonRuntime turn 的终态。
6. manual check、PostTurn finalize、repair、AI-DYNAMIC workspace transition 均使用各自的权威 Runtime phase。
7. 客户端启动时继续把遗留 Running run 转成 `Paused + ProcessInterrupted`，并同步修正权威 execution state。
8. `run-progress.json` 降级为纯观测投影，不再参与任何业务判断。
9. Conversation VM 和前端 composer 只消费明确的领域 facet，不再交叉猜测。
10. 不新增轮询、timeline 热路径扫描或按 token 写入状态的性能负担。

## 4. 非目标

1. 不取消同一 ACP session 的多轮复用。
2. 不把“最新 ACP turn 完成”改成“ACP session 不可继续”。
3. 不改变现有 Runtime / NonRuntime turn 控制语义。
4. 不改变用户停止后仍可自由对话、只有按钮才能恢复工作流的产品交互。
5. 不改变启动时自动暂停遗留 Running run 的既有策略。
6. 不使用用户输入文本推测是否恢复工作流。
7. 不让 `run-progress.json` 成为新的权威状态文件。
8. 不通过前端本地 timer 模拟 Runtime phase。

## 5. 领域模型

### 5.1 Workflow Runtime 执行状态

Workflow Runtime 是节点执行、artifact 消费、outcome 提交和 edge transition 的唯一权威写入者。

在 `run.json` 中新增：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExecutionState {
    pub revision: u64,
    pub phase: RuntimeExecutionPhase,
    pub locator: Option<AttemptLocator>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeExecutionPhase {
    StartingNode,
    RunningNode,
    FinalizingArtifact,
    RepairingArtifact,
    AwaitingManualCheck,
    Transitioning,
    LaunchingNextNode,
    PreparingWorkspace,
    Paused,
    Terminal,
}
```

`RunState` 增加：

```rust
pub execution: RuntimeExecutionState
```

字段所有权：

| 字段 | 权威含义 |
|---|---|
| `RunState.status` | Run 的顶层 Running / Paused / Completed 状态 |
| `RunState.pause_reason` | Run 暂停原因 |
| `RunState.current_*` | 当前外层 attempt locator |
| `execution.phase` | Runtime 当前执行阶段 |
| `execution.locator` | 该阶段实际作用的精确 attempt / dynamic leaf |
| `execution.revision` | 每次权威 Runtime transition 单调递增的版本 |

`execution.locator` 不能由页面当前选择的 session 反推。固定 workflow 指向普通 attempt；AI-DYNAMIC 指向精确 outer + inner locator。

### 5.2 Runtime Control 状态

继续复用现有：

```rust
pub enum TurnControlMode {
    RuntimeControlled,
    NonRuntimeControlled,
}
```

该状态只回答：

> 当前这个 Agent turn 的输出是否交给 Runtime 消费、提取 artifact 并决定后续流转？

它不回答：

- ACP session 是否存在；
- 最新 turn 是否结束；
- 节点是否完成；
- 是否正在启动下一节点。

现有 runtime control cursor、accepted boundary 与 CAS 继续保留。控制模式属于 attempt/session turn 边界，不复制成另一份 workflow phase。

### 5.3 ACP live turn 活动

继续复用并明确命名现有进程内 `PromptActivity`：

```rust
pub enum AcpTurnActivity {
    Idle,
    Starting,
    Accepted,
    Running,
    CancelRequested,
}
```

实现上可继续使用当前 registry；DTO 层将 `PromptActivity` 投影为 `liveTurnActivity`。客户端重启后 registry 为空，因此 live turn 固定为 `Idle`，不能从历史 session status 恢复出一个伪 live turn。

### 5.4 ACP session 与历史 turn

当前 `acp.session.json.status` 容易被误读成“整个 session 已完成”。新结构必须拆分：

```rust
pub enum AcpSessionAvailability {
    Established,
    Restorable,
    Unavailable,
    Closing,
}

pub enum AcpLatestTurnStatus {
    None,
    Completed,
    Cancelled,
    Failed,
}
```

含义：

- `sessionAvailability` 决定能否继续使用同一 session / continue ref；
- `latestTurnStatus` 只是历史信息；
- `liveTurnActivity` 只描述当前进程内正在执行的 prompt；
- `latestTurnStatus=completed` 不表示 session 不可复用，也不表示节点完成。

开发阶段执行破坏式更新：新 DTO 删除通用 `acp.status` 的业务消费路径，不长期保留“新旧字段二选一”的 fallback。持久化旧文件通过一次性 schema migration 转换为 `availability + latestTurn`，转换结果重新写入，后续只读新结构。

### 5.5 Direct 模式

Direct 不属于 Workflow Runtime：

- `RunState.execution` 不参与 Direct Conversation VM；
- Direct 只消费 ACP session、live turn、latest turn 和 prompt queue；
- Direct 不可能产生 `Transitioning / LaunchingNextNode / PreparingWorkspace`；
- Direct turn 始终是 NonRuntimeControlled。

## 6. 核心不变量

实现必须在领域接口与测试中固化以下不变量：

1. ACP 状态不能写 Workflow Runtime phase。
2. `latestTurnStatus` 不能改变 Run / Round / Node 状态。
3. NonRuntime turn 不能提交 node outcome。
4. NonRuntime turn 不能进入 finalize、repair、edge transition。
5. `LaunchingNextNode` 只能在当前节点 outcome 已 durable 提交后出现。
6. `AwaitingManualCheck` 只能由 Runtime 在业务 turn 与必要 artifact 阶段完成后写入。
7. `Paused + ProcessInterrupted` 下的普通 turn 结束后仍是 `Paused`。
8. `run-progress.json` 不能影响 Runtime、continue 资格、composer 或 sidebar 状态。
9. 前端不得从 session / turn 状态推断 workflow phase。
10. 启动恢复必须在首个 Conversation VM 暴露给前端前完成。

明确删除以下推断：

```text
runtime_active && acp_terminal
  -> launching-next-node
```

以及所有等价实现。

## 7. 权威状态迁移

### 7.1 启动节点

```text
Run.status = Running
execution.phase = StartingNode
execution.locator = target attempt
execution.revision += 1

provider prompt accepted
  -> execution.phase = RunningNode
```

`StartingNode` 表示 Runtime 已决定执行该 locator，但当前 RuntimeControlled ACP turn 尚未 active。UI 显示“正在启动”或“正在继续”，不能读取上一条 turn 的终态。

### 7.2 正常 RuntimeControlled turn

```text
StartingNode
  -> RunningNode
  -> FinalizingArtifact（PostTurnProjection）
  -> RepairingArtifact（输出不合法时）
  -> AwaitingManualCheck（启用人工 check 时）
  -> outcome commit
```

InlineControl 的 artifact 在 `RunningNode` turn 内消费；PostTurnProjection 的 finalize / repair 使用独立权威 phase。

### 7.3 停止工作流

```text
Run.status = Paused
Run.pauseReason = ProcessInterrupted
execution.phase = Paused
execution.locator = stopped attempt
execution.revision += 1
TurnControlMode = NonRuntimeControlled
```

停止后的普通追问：

```text
Run.status              = Paused
execution.phase         = Paused
TurnControlMode         = NonRuntimeControlled
liveTurnActivity        = Running
latestTurnStatus        = previous history
```

Agent 回复结束：

```text
Run.status              = Paused
execution.phase         = Paused
TurnControlMode         = NonRuntimeControlled
liveTurnActivity        = Idle
latestTurnStatus        = Completed
```

这里的 `Completed` 只属于 ACP latest turn。页面继续展示普通输入与“继续工作流”。

### 7.4 显式继续工作流

点击“继续工作流”后，Runtime 根据 durable checkpoint 选择恢复阶段：

| durable 事实 | 恢复 phase |
|---|---|
| 普通 workflow business turn 尚未完成 | `StartingNode` |
| `artifact-emission.json(finalizing)` | `FinalizingArtifact` |
| artifact repair 待继续 | `RepairingArtifact` |
| AI-DYNAMIC workspace 临界区 | `PreparingWorkspace` |
| AI-DYNAMIC 普通 leaf | `StartingNode` + 精确 inner locator |

恢复顺序：

```text
1. 校验 continue 资格与 source transition
2. 在生命周期锁内提交 Run Running + execution phase/revision
3. 准备 RuntimeResume / finalize / repair invocation
4. Provider control 注册 live turn
5. accepted event 后提交 RuntimeControlled cursor CAS
```

第 2 到第 4 步的窗口由权威 `execution.phase` 表达，绝不读取上一条 NonRuntime latest turn 的 `Completed`。

### 7.5 人工 check

```text
业务 turn / artifact 阶段完成
  -> execution.phase = AwaitingManualCheck
  -> Run.status = Paused 或现有 waiting 状态投影
  -> TurnControlMode = NonRuntimeControlled
```

人工 check 期间普通追问只改变 ACP live/latest turn，不改变 `AwaitingManualCheck`。

用户点击成功 / 失败后：

```text
提交 NodeOutcome
  -> execution.phase = Transitioning
```

通用“继续工作流”按钮不能绕过 manual check。

### 7.6 节点完成与跳转

固定提交顺序：

```text
1. 原子写入 node.json：Completed + outcome
2. 原子写入 run.json：execution.phase = Transitioning，revision + 1
3. Runtime 计算 edge target
4. 原子写入 run.json：
   - 有 target：current locator = target，phase = LaunchingNextNode，revision + 1
   - 无 target：phase = Terminal，Run Completed，revision + 1
5. 创建 / 启动 target attempt：phase = StartingNode
```

只有第 4 步可以产生 `LaunchingNextNode`。ACP turn 结束、session idle 或 timeline terminal 都不能产生该阶段。

### 7.7 AI-DYNAMIC

AI-DYNAMIC graph 继续拥有内部拓扑、leaf outcome、group、workspace catalog 等细节；顶层 `RunState.execution` 是 Conversation / UI 的权威执行阶段。

映射规则：

| Dynamic 权威事实 | 顶层 execution phase |
|---|---|
| selected leaf prompt starting / running | `StartingNode / RunningNode` |
| selected leaf finalize / repair | `FinalizingArtifact / RepairingArtifact` |
| graph 正在聚合或选择后继 | `Transitioning` |
| workspace checkpoint / fork / release | `PreparingWorkspace` |
| 后继 leaf 已确定、尚未 active | `LaunchingNextNode` |
| outer / selected leaf 被停止 | `Paused` |

顶层 phase 与 dynamic graph 的更新必须位于现有 project-scoped dynamic state lock 和 run lifecycle transition 边界内。不得由 selected leaf 的 ACP terminal 状态反推 outer graph phase。

## 8. 持久化与 crash consistency

### 8.1 权威文件

| 文件 | 定位 | 是否参与业务决策 |
|---|---|---|
| `run.json` | Run 与 Runtime execution aggregate | 是 |
| `node.json` | 单节点状态、outcome、manual check | 是 |
| `artifact-emission.json` | PostTurn durable checkpoint | 是 |
| dynamic graph / run / node JSON | AI-DYNAMIC 内部权威状态 | 是 |
| runtime control cursor | 当前 turn 控制边界 | 是，仅控制结果消费 |
| ACP session / snapshot | session 与 turn 历史 | 仅 ACP 领域 |
| `run-progress.json` | 观测投影 | 否 |
| `events.jsonl` | 审计与诊断事件 | 否，不作为恢复 WAL |

### 8.2 revision

`run-progress.json` 增加：

```json
{
  "runtimeRevision": 42,
  "status": "running",
  "currentStage": "running-node"
}
```

任何仍展示 progress 的旧详情 / console 入口必须同时读取 `run.json.execution.revision`：

```text
progress.runtimeRevision != run.execution.revision
  -> progress stale
  -> 忽略阶段和状态，只允许展示已明确标注为历史诊断的数据
```

Conversation VM、continue 资格、sidebar 和 composer 完全不读取 `run-progress.json`。

### 8.3 写入服务

新增统一领域服务，禁止业务代码散落修改 phase：

```rust
pub struct RuntimeLifecycleStore { ... }

impl RuntimeLifecycleStore {
    pub fn start_node(...);
    pub fn mark_provider_running(...);
    pub fn begin_finalize(...);
    pub fn begin_repair(...);
    pub fn await_manual_check(...);
    pub fn pause(...);
    pub fn begin_transition(...);
    pub fn launch_next_node(...);
    pub fn prepare_workspace(...);
    pub fn complete(...);
}
```

每个接口负责：

1. 校验允许的 source phase；
2. 校验 locator / runtime execution id；
3. 在现有 run-scoped lifecycle lock 内更新状态；
4. revision 单调递增；
5. 使用现有原子 JSON helper 落盘；
6. 返回已提交的权威 snapshot；
7. 锁外生成 progress projection 与 lifecycle event。

错误继续使用结构化 `RuntimeErrorInfo`，不得返回面向用户的字符串。

### 8.4 跨文件提交顺序

当前持久化为多个原子 JSON 文件，不具备跨文件事务。方案不引入数据库或 WAL，而采用固定写入顺序与启动 reconciliation：

- 节点完成时先写 `node.json`，再推进 `run.json`；
- run 绝不先指向后继节点、再补写前驱 outcome；
- 崩溃在 node commit 后：启动 reconciliation 可根据 completed node 继续收敛 run；
- 崩溃在 run transition 后：node outcome 已存在，不会出现无完成证据的后继；
- 每次收敛都产生新的 revision，旧 progress 自动失效。

如未来需要强事务，再将该 aggregate 迁移到 SQLite；本次不为单个 phase 投影问题引入第二套持久化基础设施。

## 9. 启动恢复

现有行为保持：客户端启动时，所有遗留 `RunStatus::Running` 统一转为：

```text
Run.status = Paused
Run.pauseReason = ProcessInterrupted
```

同时补齐：

```text
execution.phase = Paused
execution.locator = current locator
execution.revision += 1
```

启动顺序必须保持：

```text
1. 注册 Runtime lifecycle subscribers
2. recover_interrupted_running_sessions()
3. 同步修正 current node / dynamic descendants / execution phase
4. 使旧 progress revision 失效
5. 才允许首个 Conversation VM 暴露给前端
```

重启后的 ACP 状态：

- live turn registry 为空，`liveTurnActivity=Idle`；
- latest turn 仍保留历史 `Completed / Cancelled / Failed`；
- session continue ref 仍可恢复；
- 历史 latest turn 不能覆盖已恢复的 `Paused` phase。

### 9.1 启动 reconciliation

对跨文件崩溃窗口执行确定性收敛：

1. `run=Running`：按现有策略暂停，不自动继续。
2. `run=Paused`：强制 `execution.phase=Paused`，保留 pause reason。
3. `run=Terminal`：强制 `execution.phase=Terminal`。
4. 当前 node 已 completed、run 仍指向该节点且进程为异常退出：仍先暂停，由用户显式继续后从 `Transitioning` checkpoint 收敛，不在启动时静默推进业务。
5. dynamic graph 的 Running leaf 同步转为 Paused，保留已有 continue locator。
6. progress revision 不匹配时不重写也可失效；后续权威 transition 会生成新 projection。

## 10. 后端接口与 Conversation VM

### 10.1 Lifecycle DTO

破坏式调整为明确 facet：

```ts
type ConversationAttemptLifecycle = {
  runtime: {
    status: RuntimeStatus;
    phase: RuntimeExecutionPhase | null;
    revision: number | null;
    pauseReason: PauseReason | null;
    active: boolean;
    continuable: boolean;
    current: boolean;
  };
  control: {
    mode: 'runtime-controlled' | 'non-runtime-controlled';
  };
  acp: {
    sessionAvailability: AcpSessionAvailability;
    liveTurnActivity: AcpTurnActivity;
    latestTurnStatus: AcpLatestTurnStatus;
    stopping: boolean;
  };
  composer: ConversationComposer;
};
```

删除 / 停止消费：

- 含义模糊的通用 `acp.status`；
- `acp.terminal` 驱动 Runtime phase；
- `runtime_active && acp_terminal` 推导；
- selected session status 覆盖 Run phase；
- 前端从 timeline 尾事件猜测 workflow transition。

### 10.2 Runtime active

`runtime.active` 只由权威 Runtime 状态派生：

```text
Run.status == Running
&& execution.phase not in {Paused, Terminal}
```

ACP live turn 可以让 composer 暂时锁定或显示停止按钮，但不能把 paused workflow 改成 Runtime active。

### 10.3 Composer 投影

| Runtime phase | Control mode | ACP live turn | Composer |
|---|---|---|---|
| `Paused` | NonRuntime | Idle | 普通输入 + 继续工作流 |
| `Paused` | NonRuntime | Running | 普通回复思考中，可停止该 turn |
| `StartingNode` | Runtime | Idle / Starting | 正在继续 / 正在启动 |
| `RunningNode` | Runtime | Running | 思考中 / 处理中 |
| `FinalizingArtifact` | Runtime | Running | 正在整理结果 |
| `RepairingArtifact` | Runtime | Running | 正在修复输出 |
| `AwaitingManualCheck` | NonRuntime | Idle | 普通输入 + 成功 / 失败按钮 |
| `Transitioning` | Runtime | Idle | 正在处理节点结果 |
| `LaunchingNextNode` | Runtime | Idle / Starting | 拉起下一节点中 |
| `PreparingWorkspace` | Runtime | 任意 | 正在准备开发环境 |
| `Terminal` | NonRuntime | Idle | 普通 follow-up 输入 |

优先级：

```text
stop pending
  > permission / elicitation
  > authoritative Runtime phase
  > ACP live turn activity
  > latest turn history（只用于历史标记）
```

`latestTurnStatus` 不参与 composer phase 文案。

## 11. run-progress 与旧入口收敛

`run-progress.json` 保留用于：

- 诊断页；
- 旧 Run 详情的阶段说明；
- console 摘要；
- 性能与审计记录。

但需要完成以下收敛：

1. 所有写入携带 `runtimeRevision`。
2. 状态和 stage 由已提交 `RuntimeExecutionState` 映射，不再由调用点自由传字符串。
3. Conversation VM 完全移除读取。
4. 旧详情 / console 遇到 stale revision 时显示权威 Run 状态，不显示 stale stage。
5. `latest_control_failure` 的 progress fallback 只允许读取 diagnostic payload，不允许覆盖 Run 状态；后续应由结构化 Runtime failure 事件替代。
6. `find_active_or_resumable_run_id` 只按权威 Run 状态选择，progress 文件是否存在不得改变选择优先级。

## 12. 前端改造

前端继续使用现有 prompt-kit composer 与 shadcn/ui，不新增基础聊天组件。

### 12.1 状态消费

- `acp-runtime-composer-state.ts` 只组合后端 facet；
- 移除 `backendProcessingKind === launching-next-node` 对 timeline / session 状态的补推；
- timeline 只决定当前 Agent 活动的细粒度文案，如 thinking / tool / responding；
- Runtime phase 决定工作流阶段文案；
- sidebar 状态点继续消费后端权威 Runtime display，不读取 session terminal 推断节点状态。

### 12.2 本地 pending

“继续工作流”点击后的本地 pending 只用于覆盖请求往返窗口：

- 后端接受后立即返回带 revision 的权威 lifecycle；
- 本地 pending 在收到 revision 不小于请求返回值的 snapshot 后释放；
- 不允许从旧 paused snapshot 或旧 latest turn 产生按钮闪烁；
- 不使用定时器推导 `LaunchingNextNode`。

### 12.3 UI 验证

实现后必须启动前端并通过 deep link 验证：

1. 停止 → 普通追问 → 回复完成；
2. 停止 → 普通追问 → 再次停止该回复；
3. 普通追问结束后点击继续；
4. manual check 等待期间追问；
5. PostTurn finalize 中断后继续；
6. AI-DYNAMIC leaf 与 workspace transition；
7. 客户端退出重启后的 paused 恢复。

验证完成后清理测试 run、临时页面和测试进程。

## 13. 迁移策略

项目处于开发阶段，采用一次性破坏式迁移，不长期维护双读兼容层。

### 13.1 RunState migration

旧 `run.json` 缺少 `execution` 时，在存储层执行一次性迁移并立即回写：

| 旧 Run 状态 | 初始 execution phase |
|---|---|
| Running（正常读取） | `RunningNode`；桌面启动恢复随后转 `Paused` |
| Paused | `Paused` |
| Completed / Failed / Cancelled | `Terminal` |

旧数据迁移不得读取 ACP session terminal 推断 phase。迁移完成后删除无 execution 的 fallback。

### 13.2 ACP migration

旧 `acp.session.json.status` 一次性映射：

- session id / continue ref 存在：`Established` 或 `Restorable`；
- 旧 `completed / cancelled / failed`：只写入 `latestTurnStatus`；
- 不从旧 status 恢复 live activity；
- 写回新 schema 后删除通用 status 消费。

### 13.3 Web DTO migration

Rust 与 Web 在同一变更中切换到新 DTO：

- 更新 `src-tauri` VM；
- 更新 `web/src/types.ts`；
- 删除旧字段引用；
- 不增加 `acp.status ?? latestTurnStatus` fallback。

## 14. 实现步骤

### 阶段一：固化现有错误

1. 增加接口测试：Paused + NonRuntime + latest turn completed 仍 continuable。
2. 增加 resume 启动窗口测试：Runtime StartingNode + stale latest completed 不产生 LaunchingNextNode。
3. 增加 manual check、finalize、repair、AI-DYNAMIC 的反例测试。

### 阶段二：建立权威数据

1. 新增 `RuntimeExecutionState / RuntimeExecutionPhase`。
2. 扩展 `RunState` schema 与一次性迁移。
3. 新增 `RuntimeLifecycleStore`，集中 phase transition。
4. 所有 source phase、locator、execution id 使用领域校验。

### 阶段三：接入 Runtime 写路径

1. node start / provider accepted；
2. business turn / finalize / repair；
3. manual check；
4. outcome commit / edge transition；
5. fixed workflow continue；
6. AI-DYNAMIC leaf / graph / workspace；
7. stop 与启动恢复。

### 阶段四：拆分 ACP DTO

1. session availability；
2. live turn activity；
3. latest turn status；
4. 持久化一次性 migration；
5. 删除通用 ACP terminal 对 Runtime 的影响。

### 阶段五：Conversation VM 与 Web

1. 后端输出 runtime / control / acp 三类 facet；
2. composer 只消费权威 Runtime phase；
3. timeline 只投影 live Agent activity；
4. sidebar / session tree 统一消费后端 display；
5. 删除旧 inference 与 fallback。

### 阶段六：progress 降级

1. 增加 runtime revision；
2. 所有 writer 由 committed execution state 生成；
3. 清除 Conversation / active run selection 对 progress 的依赖；
4. 旧详情与 console 增加 stale revision 处理。

### 阶段七：删除与审查

全局搜索并删除：

- `runtime_active && acp_terminal` 类判断；
- session `completed` 推导 workflow transition；
- progress 文件存在影响 active run 选择；
- 前端 timeline terminal 推导 Runtime phase；
- 新旧 ACP DTO 双读 fallback。

## 15. 测试与验收矩阵

### 15.1 Runtime / 存储单元测试

1. 所有合法 phase transition。
2. 非法 source phase 返回结构化错误。
3. revision 严格单调递增。
4. locator / execution id 不匹配时拒绝迟到写入。
5. node outcome 先于 transition durable 提交。
6. stale progress revision 被忽略。
7. 旧 Run / ACP schema 只迁移一次。
8. 启动恢复把 Running + 任意 phase 收敛为 Paused。

### 15.2 Conversation VM 接口测试

1. `Paused + NonRuntime + live Idle + latest Completed`：普通输入 + continue action。
2. `Paused + NonRuntime + live Running`：仍 Paused，只停止当前回复。
3. `StartingNode + stale latest Completed`：显示正在继续，不显示下一节点。
4. `RunningNode + live Running`：显示思考 / 处理中。
5. outcome 未提交：任何 ACP terminal 都不能产生 LaunchingNextNode。
6. outcome 已提交且 phase 为 Transitioning：显示正在处理节点结果。
7. phase 明确为 LaunchingNextNode：才显示拉起下一节点。
8. manual check 追问完成后仍 AwaitingManualCheck。
9. Direct 永不产生 workflow phase。

### 15.3 Runtime 集成测试

1. 停止 → 普通追问 → 回复完成 → 继续资格仍在。
2. 停止 → 普通追问 → 停止回复 → 继续资格仍在。
3. 普通追问完成后点击继续，不读取旧 turn terminal。
4. PostTurn business / finalize / repair 分别中断与恢复。
5. InlineControl 中断与恢复。
6. manual check 成功 / 失败是唯一推进入口。
7. fixed workflow edge transition 的 phase 顺序正确。
8. AI-DYNAMIC 精确 leaf、merge、acceptance、workspace phase 正确。
9. 客户端重启后 Running 自动变 Paused，旧 progress 失效。
10. 模拟各原子写入边界崩溃，启动 reconciliation 可确定性收敛。

### 15.4 Web 单元与交互测试

1. ACP latest turn completed 不改变 Runtime phase。
2. continue pending 不闪回继续按钮。
3. composer status 文案按权威 phase 映射。
4. timeline thinking / tool 只覆盖 Agent 活动文案，不覆盖 workflow transition。
5. session tree 与 sidebar 使用同一后端状态点。
6. deep link 启动恢复页面无 stale progress 闪烁。

## 16. 性能评估

### 16.1 正常热路径

本方案不增加：

- 文件轮询；
- timeline 扫描；
- per-token JSON 写入；
- provider turn；
- 全局长锁；
- 跨 session 串行化。

新增成本仅为 Runtime phase transition 时在本来就要写入的 `run.json` 中更新一个小型 execution object 和 revision。phase 变化发生在节点 / turn 边界，不发生在流式 token 热路径。

### 16.2 读取成本

Conversation VM 本来就读取 `run.json`，直接从同一对象获得 execution phase，不增加额外文件读取。相反，移除跨 timeline / session / progress 的推断后，状态派生成本更低且稳定为 O(1)。

### 16.3 写入与并发

- 继续复用现有 run-scoped lifecycle lock；
- AI-DYNAMIC 继续复用 project-scoped dynamic state lock；
- phase 写入不覆盖 provider 调用时长；
- 不同 run / session / leaf 保持并行；
- progress 和 lifecycle event 在权威状态提交后锁外生成。

### 16.4 磁盘与迁移

`execution` 与 revision 仅增加少量 JSON 字节。一次性旧 schema migration 只在首次读取旧文件时执行并回写，不在后续热路径重复判断。

## 17. 风险与约束

1. `run.json / node.json / dynamic graph` 仍是多文件持久化，必须严格执行提交顺序与启动 reconciliation。
2. phase 不能成为另一套与 Run / Node 状态独立演进的状态机；所有 mutation 必须集中在 `RuntimeLifecycleStore`。
3. lifecycle event 与 progress 是下游投影，即使写入失败也不能回滚已提交的 Runtime 状态。
4. Web 本地 pending 不能长期覆盖后端 revision；超时应重新读取权威 snapshot，而不是自行猜 phase。
5. AI-DYNAMIC 不能让 inner ACP terminal 覆盖 outer graph phase。
6. 迁移完成后必须删除旧字段和 fallback，否则领域混淆会通过兼容代码重新出现。

## 18. 完成标准

满足以下条件才算完成：

1. Runtime execution phase 已持久化且有单调 revision。
2. 所有 Runtime transition 通过统一领域服务写入。
3. ACP session、live turn、latest turn DTO 已拆分。
4. `runtime_active && acp_terminal => launching-next-node` 及等价推断全部删除。
5. `run-progress.json` 不再参与业务决策。
6. 停止后普通追问结束仍保持 Paused + continuable。
7. `LaunchingNextNode` 只在 node outcome durable 提交后出现。
8. 启动恢复同步收敛 execution phase / revision，且保留现有 ProcessInterrupted 语义。
9. fixed workflow、manual check、PostTurn、InlineControl、AI-DYNAMIC、Direct 全部通过接口回归。
10. 前端 deep link 与客户端重启场景验证无错误阶段闪烁。

## 18.1 实施状态（2026-08-12）

已完成：

- `RunState.execution`、显式 phase、精确 locator 与单调 revision；所有字段写入统一经过 `RuntimeLifecycleStore` 状态转换校验。
- fixed workflow、PostTurn finalize/repair、manual check、节点 transition、AI-DYNAMIC leaf/workspace 与 continue 启动窗口的权威 phase 接入。
- Conversation DTO 拆为 runtime/control/ACP 三组 facet，删除通用 `acp.status/active/terminal` 消费；当前 turn 只认进程内 prompt registry，历史 session status 不再重建 live activity。
- 前端 composer、session tree、sidebar 和 active snapshot 只消费明确 facet；`LaunchingNextNode` 仅来自后端 execution phase。
- `run-progress.json` 降级为 revision 对齐后的详情观测，active run 选择、错误语义与 Conversation VM 不再依赖 progress。
- ACP metadata 已完成破坏式 schema 收口：新写入只包含 `availability + latestTurnStatus`，旧通用 `status` 由统一 loader 首次读取后一次性回写；迁移不扫描 timeline，也不恢复 live turn。
- 启动恢复、停止 revision、stale progress、manual check、Direct、AI-DYNAMIC、继续窗口和旧 snapshot 竞态均已加入 Rust/Web 回归测试。

验证与实际 UI deep-link 结果记录在本次实现验收中；若产品运行环境没有可复用测试会话，则以 production build、接口测试和浏览器可达页面验证为最低交付门槛。

## 19. 最终原则

```text
Workflow Runtime 决定工作流执行阶段。
Runtime Control 决定是否消费当前 turn。
ACP live turn 决定 Agent 当前是否正在回复。
ACP session 决定会话是否可复用。
latest turn 只描述历史。
progress 只负责观测。
```

不同领域可以在 Conversation VM 中组合展示，但任何低层状态都不得反向决定高层业务事实。
