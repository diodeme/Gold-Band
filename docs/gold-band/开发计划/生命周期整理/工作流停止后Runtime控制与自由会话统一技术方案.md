# 工作流停止后 Runtime 控制与自由会话统一技术方案

## 1. 背景

Gold Band 当前已经具备 Direct、固定工作流、AI-DYNAMIC、人工 check、节点结束后追问、ACP stop / continue 等多种会话入口，但“Agent 可以继续对话”和“Runtime 应继续推进工作流”仍然存在语义耦合。

典型问题是：用户停止正在执行的工作流后，如果继续在同一个 ACP session 中发送消息，现有提交路径可能把这条普通消息理解为 runtime continue。Agent 一次回复结束后，Runtime 随即尝试读取 artifact、判断节点 outcome 或推进后继节点。用户因此无法在保持工作流停止的前提下，与 Agent 进行任意多轮澄清。

artifact 发射模式后置后，这个问题需要同时覆盖两类控制节点：

- `InlineControl`：当前 turn 的职责就是生成 runtime 控制 artifact，AI-DYNAMIC bootstrap dispatcher 属于该模式。
- `PostTurnProjection`：可见业务 turn 不要求 artifact；业务完成后由同一 session 的隐藏 `RuntimeFinalize` turn 生成控制 artifact。

停止可能发生在业务 turn、InlineControl turn 或 PostTurn finalize turn。不同阶段对 Agent 历史指令的影响不同，但 Runtime 是否继续推进不应依赖 Agent 输出内容，也不应依赖用户消息文本是否像“继续”。

本方案采用 control plane / conversation plane 分层的通用做法：将每次 Agent turn 明确归类为 Runtime 控制或非 Runtime 控制。该分类属于 invocation 级执行语义，不新增一套 workflow 持久化状态机。

## 2. 目标

1. Direct、工作流正常执行、工作流停止后聊天、人工 check 追问、节点或工作流结束后追问统一使用同一套 turn 控制语义。
2. 用户停止工作流后，Run / Round / Node 继续保持现有 `Paused + ProcessInterrupted`，不新增 `PausedConversation` 等重复状态。
3. 停止后的普通用户消息只驱动一次 ACP 对话 turn，不自动恢复 Runtime，可连续发送任意多轮。
4. 只有用户显式点击“继续工作流”，Runtime 才恢复节点执行、artifact 处理和 workflow edge 推进。
5. 原始 prompt 和原始 `output_contract` 保持不变；非 Runtime 控制 turn 仅暂停 Runtime 对 artifact 的消费、校验和流转。
6. 仅在控制模式从 `RuntimeControlled` 切换为 `NonRuntimeControlled` 后，第一条用户消息追加一次脱离 Runtime 的运行上下文；该上下文沿用现有 `<hidden>` 收缩模块展示，不把正文完全隐藏。
7. 仅在控制模式从 `NonRuntimeControlled` 切换为 `RuntimeControlled` 时，由 Runtime 自动发送继续提示；停止一个已经处于 NonRuntime 的普通对话 turn 不形成新边界，也不重复注入提示。
8. 停止发生在 PostTurn finalize 阶段时，继续复用 `artifact-emission.json(finalizing)` 作为“业务 turn 已完成、artifact 尚待可信完成”的事实；恢复时不信任中断回复，而是重新请求一次完整 artifact，不重新执行业务任务。

## 3. 非目标

1. 本方案不新增 Run、Round、Node 或 DynamicNode 的持久化状态枚举。
2. 本方案不复用 `manual_check_pending` 表达用户停止。人工 check 表示节点已经执行完、等待用户判定；用户停止表示节点尚未完成、Runtime 暂停推进。
3. 本方案不根据用户输入“继续”“可以了”等自然语言推测是否恢复工作流。
4. 本方案暂不解决历史 system prompt 与后续 user runtime context 的指令优先级问题。当前阶段复用既有用户消息 `<hidden>` 运行上下文能力；这里的 `hidden` 是消息气泡中默认收缩、可展开的模块，不代表内容从 timeline 完全不可见。
5. 本方案不删除、不改写节点配置或原始 `output_contract`，也不把 contract 迁移到另一套数据结构。

## 4. 核心抽象

### 4.1 Turn 控制模式

每次 provider invocation 都派生一个 turn 控制模式：

```rust
enum TurnControlMode {
    RuntimeControlled,
    NonRuntimeControlled,
}
```

该枚举属于调用级策略，不写入 `run.json`、`node.json` 或新的状态文件。它由提交入口和当前持久化生命周期事实共同决定。

#### RuntimeControlled

当前 turn 属于 Runtime 执行链。Runtime 可以：

- 按 `InlineControl / PostTurnProjection` 决定 artifact 发射方式；
- 消费执行链最终产生的控制输出；
- 校验 artifact 与 schema；
- 计算 `NodeOutcome`；
- 进入隐藏 repair、人工 check 或 workflow edge；
- 完成节点并调度后继节点。

`PostTurnProjection` 的可见业务回复不是 artifact，但它仍属于 Runtime 控制执行链；业务 turn 正常结束后，Runtime 会自动进入隐藏 finalize，最终消费 finalize 回复。

#### NonRuntimeControlled

当前 turn 只是会话。Runtime 必须：

- 把 Agent 回复写入 ACP timeline / session snapshot；
- 保留同 session 的 continue ref、附件、模型和权限配置能力；
- 不提取或校验 artifact；
- 不把 Agent 回复转换为 `NodeOutcome`；
- 不进入 PostTurn finalize 或 invalid output repair；
- 不改变 Run / Round / Node / DynamicNode 的业务状态；
- 不调度 workflow edge 或 AI-DYNAMIC 后继节点。

即使 Agent 在该 turn 中主动输出了符合原 contract 的 JSON，Runtime 也只能把它视为普通会话内容，不能意外完成节点。

### 4.2 场景映射

| 场景 | TurnControlMode | Runtime 状态变化 |
|---|---|---|
| Direct 首轮与后续消息 | `NonRuntimeControlled` | 无 workflow 推进 |
| 固定工作流正常执行 | `RuntimeControlled` | 正常完成节点并沿 edge 推进 |
| AI-DYNAMIC bootstrap / worker / acceptance | `RuntimeControlled` | 按节点角色应用 Inline 或 PostTurn |
| PostTurn 隐藏 finalize / repair | `RuntimeControlled` | 生成、校验 canonical artifact |
| 用户停止工作流后发送消息 | `NonRuntimeControlled` | 保持 `Paused + ProcessInterrupted` |
| 人工 check 等待期间追问 | `NonRuntimeControlled` | 保持 `manual_check_pending`，只由判定按钮推进 |
| 节点或工作流结束后追问 | `NonRuntimeControlled` | 保持既有完成事实 |
| 用户点击“继续工作流” | `RuntimeControlled` | 恢复当前 attempt 并继续执行链 |

### 4.3 Prompt 渲染模式与控制模式分工

继续复用现有 `UserPromptRenderMode`：

```rust
enum UserPromptRenderMode {
    RequirementTask,
    WorkflowResume,
    UserMessage,
    RuntimeResume,
    RuntimeFinalize,
    RuntimeRepair,
}
```

- `UserPromptRenderMode` 只决定本轮 prompt 如何组织和展示。
- `TurnControlMode` 决定 Runtime 是否消费结果并推进状态机。
- `UserMessage` 不再隐含 runtime continue；停止后的用户消息、Direct 消息、人工 check 追问和完成后追问都可以是 `UserMessage + NonRuntimeControlled`。
- 点击“继续工作流”使用 `RuntimeResume + RuntimeControlled`；`WorkflowResume` 继续保留给工作流内部的普通 session 继承，不由 composer 消息隐式触发。

### 4.4 控制模式切换事实与会话游标

切换提示不能靠 `stop` 次数推断。最新切换直接记录为 ACP session metadata；被接受的 prompt event 同步携带该 metadata，形成可恢复的轻量会话游标：

```rust
enum TurnControlTransitionCause {
    RuntimeInterrupted,
    WorkflowContinued,
    RuntimeTerminal,
}

struct AcpRuntimeControlCursor {
    current_mode: TurnControlMode,
    transition_id: String,
    transition_cause: TurnControlTransitionCause,
    suspended_context_accepted_for: Option<String>,
    suspended_context_prompt_id: Option<String>,
    changed_at: String,
}
```

- `AcpRuntimeControlCursor` 自身记录最新切换，不再增加第二个 transition 数据结构；`current_mode + transition_cause` 足以表达切换方向。
- `AcpRuntimeControlCursor` 作为 `runtimeControl` metadata 复用既有 `acp.snapshot.json` / `acp.session.json` 保存，不新增独立状态文件；若旧快照缺少该 metadata，应用恢复时只回看一次既有 timeline 并回填快照，后续提交为 O(1) 读取。
- 只有 `RuntimeControlled -> NonRuntimeControlled + RuntimeInterrupted` 使下一条用户消息待附加 `runtimeControlSuspended`。
- `NonRuntimeControlled -> NonRuntimeControlled` 的 stop 只取消当前普通对话，不改变 transition id。
- `NonRuntimeControlled -> RuntimeControlled + WorkflowContinued` 只允许产生一个 resume / finalize 控制 prompt。
- `RuntimeTerminal` 可使后续追问进入 NonRuntime，但不使用“用户已停止工作流”的 suspended context。

## 5. 持久化事实复用

### 5.1 用户停止

继续复用现有状态：

```text
Run.status       = Paused
Run.pause_reason = ProcessInterrupted
Round.status     = Paused
Node.status      = Paused
Node.outcome     = None
```

AI-DYNAMIC 单 leaf 停止继续复用 `DynamicNodeStatus::Paused + ProcessInterrupted`；父 graph / run 是否暂停仍按现有 active leaf 聚合规则决定。

停止只改变业务执行状态并取消当前 ACP prompt，不关闭 ACP session，不删除 worker ref，也不新增“对话中”状态。

### 5.2 PostTurn finalize

继续以 `artifact-emission.json(finalizing)` 为唯一 durable phase：

- 文件不存在：当前 attempt 尚未进入 finalize，继续工作流时恢复业务任务。
- 文件为 `finalizing`：只证明当前 attempt 已完成业务 turn 并进入 artifact finalize，不证明被中断的 Agent 回复已经完整，也不证明 artifact 已经通过解析和校验。
- 非 Runtime 控制消息不能删除、覆盖或重新创建该文件。
- finalize 期间停止后聊天，不得因为用户消息 turn 正常结束而重跑业务任务或完成节点。
- 点击继续时忽略被中断 finalize turn 中识别到的任何候选或片段，重新发送一次完整的 `artifact_finalize` prompt；只有这个新 turn 成功结束并产出合法 canonical artifact 后，Runtime 才允许写 artifact、计算 outcome 和完成节点。
- 不新增 `completed` emission phase。artifact 是否已可信完成继续由既有的 artifact 落盘、provider terminal success 和 Node / DynamicNode 完成事实共同表达；`finalizing` 只承担可恢复阶段定位。

### 5.3 原始 output contract

节点和 invocation 中的 `PromptOutputContract` 始终保留。`TurnControlMode` 只决定本轮是否激活 Runtime 消费行为：

| TurnControlMode | contract 数据 | Agent 历史中的 contract | Runtime artifact policy |
|---|---|---|---|
| `RuntimeControlled` | 保留 | 保留 | 按 emission mode 执行 |
| `NonRuntimeControlled` | 保留 | 保留 | 不提取、不校验、不 finalize、不 repair |

本方案接受一个阶段性限制：原始 system prompt 中的 artifact 要求可能仍存在于 Agent 历史中，后续用户消息的 `<hidden>` runtime context 不一定具备更高指令优先级。因此 Agent 仍可能输出 artifact。正确性由 Runtime 控制模式保证：NonRuntime turn 的任何输出都不会被当作控制产物。

## 6. 停止后的自由会话

### 6.1 提交入口

普通 composer 消息与 Runtime continue 必须拆成两个动作：

```text
submit conversation message
  -> ACP prompt only
  -> NonRuntimeControlled
  -> workflow remains paused/completed/manual-check

continue workflow button
  -> runtime continue
  -> Runtime resume prompt
  -> RuntimeControlled
```

停止后的消息不得再进入 `drive_from_node_with_initial_session`、`finalize_ai_attempt` 或 `finalize_dynamic_worker_result`。它只复用同会话 ACP prompt helper，保存 timeline 与 session 生命周期。

### 6.2 Runtime → NonRuntime 后首条消息的运行上下文

只有当一个 `RuntimeControlled` invocation 被用户停止、Runtime 提交 `Paused + ProcessInterrupted`，从而使控制模式切换为 `NonRuntimeControlled` 后，第一条被 ACP 接受的用户消息才追加脱离 Runtime 的 runtime context。建议语义：

```text
当前工作流已由用户停止。本轮开始进入普通对话：无需遵循先前的 Runtime artifact 输出要求，也不要自行恢复或推进工作流。请直接回应用户当前的问题。只有 Runtime 后续发出的恢复提示才表示工作流已经继续。
```

要求：

1. 复用现有用户消息 `<hidden data-gold-band-hidden="true">` runtime context 结构，不新增 system prompt 通道。
2. 用户原始输入仍作为消息正文；运行说明在同一消息气泡中显示为默认收缩、可展开的“运行上下文”模块，维持现有设计，不做真正不可见处理。
3. context reason / identity 使用稳定类型，例如 `runtimeControlSuspended`，不能伪装成 repair。
4. 同一次 `RuntimeControlled -> NonRuntimeControlled` 切换后的第二条及后续用户消息不重复追加。
5. 在 NonRuntime 普通对话中再次点击停止，只是取消当前 Agent 回复，控制模式仍是 `NonRuntimeControlled`，不产生新切换边界；下一条用户消息不得重复追加解除说明。
6. 工作流显式恢复为 RuntimeControlled 后，如果用户再次停止 Runtime 执行，才会形成新的 `RuntimeControlled -> NonRuntimeControlled` 边界；该边界后的第一条消息重新追加。
7. 如果第一条消息在 ACP 接受前失败或被取消，下一次提交仍视为第一条；如果已经写入用户 prompt event，即使 Agent 回复随后被停止，也视为该切换说明已经进入会话历史。

### 6.3 基于控制模式切换的一次性判定

“第一次”不能只依赖前端组件内存，也不能只统计 `ProcessInterrupted` 次数。否则应用重启、页面切换、并发提交，或用户在 NonRuntime 对话中再次停止回复时，都会重复或漏发。实现应复用现有事实：

- 产生停止事件的 invocation 自身的 `TurnControlMode`；
- 当前 attempt 最新一次真正的 `RuntimeControlled -> NonRuntimeControlled` 切换事件；
- ACP timeline 中该模式切换之后的 user prompt 顺序；
- runtime context 已有的 reason / prompt identity；
- ACP session prompt lock。

后端在同一个 prompt lock 下判断：最新控制模式切换边界之后是否已经存在带 `runtimeControlSuspended` 的已接受 user prompt。不存在时把收缩 section 附加到本次消息；存在时只发送用户原文。

该判定允许给既有 stop / prompt timeline event 增加 `turnControlMode`、稳定 transition identity 等 metadata，但不新增 Run / Node 状态字段和独立状态文件。NonRuntime prompt 被再次停止时记录的 stop event 必须保留 `NonRuntimeControlled` 来源，使其不会被误认成新边界。

### 6.4 不同 artifact 阶段

| 停止位置 | Runtime → NonRuntime 后首条消息 context | NonRuntime 行为 |
|---|---|---|
| PostTurn 业务 turn | 添加通用“工作流已停止”说明 | 不触发 finalize；继续时恢复业务任务 |
| InlineControl turn | 添加停止说明，并明确当前无需遵循 artifact 要求 | 不启用 artifact 校验和提取 |
| PostTurn finalize / repair | 在 Runtime → NonRuntime 后首条消息添加停止说明，并明确当前无需遵循 artifact 要求 | 保留 `finalizing`，不覆盖用户消息，不消费中断产物 |

虽然 PostTurn 业务 turn 本身没有暴露具体 artifact schema，本方案仍在停止后的第一条消息加入通用停止说明，避免 Agent 把用户澄清误解为“继续完成原工作”。其中 artifact 相关句子可以按当前 contract 暴露情况条件渲染。

## 7. 显式继续工作流

### 7.1 用户动作

工作流停止后，composer 继续负责普通会话；界面同时提供明确的“继续工作流”按钮。只有该按钮可以把当前 attempt 从可继续暂停态恢复为 Runtime 控制。

不解析普通用户消息，不识别“继续”“没问题了”等关键词，也不在 Agent 回复结束后自动恢复。

### 7.2 NonRuntime → Runtime 继续提示

点击按钮后，Runtime 自动构造默认继续提示。只有当前控制模式确实从 `NonRuntimeControlled` 切换为 `RuntimeControlled` 时才发送；Runtime 已经 active 时的重复点击不能再次发送。该 prompt 必须：

- 使用同一个 ACP session 和 continue ref；
- `PromptVisibility::Hidden`；
- 不产生可见用户消息气泡；
- 使用稳定 runtime reason，例如 `runtimeControlResume`；
- 与 `RuntimeRepair` 一样由 Runtime 创建和追踪，而不是伪装成用户输入；
- 被 ACP 接受后，Runtime 才进入受控执行链。

建议语义：

```text
用户已选择继续工作流。此前的普通对话阶段已经结束，请继续完成原任务；当前 Runtime 控制要求与输出契约重新生效。
```

中英文模板必须统一放置在：

- `src/prompts/zh-CN/runtime/runtime_control_resume.md`
- `src/prompts/en/runtime/runtime_control_resume.md`

控制模式切换模板放置在：

- `src/prompts/zh-CN/runtime/runtime_control_suspended.md`
- `src/prompts/en/runtime/runtime_control_suspended.md`

模板可以根据是否存在当前 artifact contract、是否处于 finalizing 渲染必要变量，但不得在实现代码中硬编码长 prompt。

### 7.3 恢复目标

| durable 事实 | 点击继续后的行为 |
|---|---|
| 无 `artifact-emission.json`，普通 workflow worker | 发送 resume prompt，恢复业务执行；成功后再进入 PostTurn finalize |
| InlineControl | 发送 resume prompt，恢复原 contract 的 Runtime 消费与校验 |
| `artifact-emission.json(finalizing)` | 不重跑业务任务；不信任中断回复，重新发送完整 artifact finalize prompt；该 prompt 自身完成 NonRuntime → Runtime 切换，`runtimeControlResume` 不再额外制造重复 turn |
| RuntimeRepair | 继续复用 repair prompt，只修复 artifact |
| AI-DYNAMIC paused leaf | 只恢复目标 leaf；兄弟 leaf 与 graph 聚合语义保持现状 |

finalizing 场景中，现有 `artifact_finalize.md` 已经同时表达“继续”和“重新完整输出 canonical artifact”，因此它就是恢复后的首个 RuntimeControlled prompt，不需要先发送一条通用 resume 再发送 finalize。此前被停止的 finalize 回复只作为会话历史保留，不作为候选 artifact 继续拼接、补齐或验收。

## 8. Provider 与停止保护

### 8.1 Provider 执行分流

Provider 必须先判断 `TurnControlMode`，再判断 `OutputEmissionMode`：

```text
NonRuntimeControlled
  -> 只运行一次 conversation prompt
  -> 不进入 post-turn projection
  -> 不提取 result payload

RuntimeControlled
  -> InlineControl: 当前 turn 读取 artifact
  -> PostTurnProjection: 业务 turn + hidden finalize
```

这样可以保留完整 `output_contract`，同时避免 finalizing 状态把停止后的用户消息替换成 artifact finalize prompt。

### 8.2 Paused 状态下允许普通 ACP prompt

现有 ACP 执行保护会观察 attempt 是否仍在运行；工作流 turn 一旦发现 `Paused` 就应取消。这条规则需要继续保留，但不能用于停止后的自由对话：NonRuntime turn 启动时，Run 本来就应该保持 `Paused`。

调整后的规则：

- RuntimeControlled prompt：继续要求对应 attempt 为当前 `Running`。
- 用户停止后的 NonRuntime prompt：允许对应 attempt 为当前 `Paused + ProcessInterrupted`，但仍必须属于同一 current attempt / dynamic leaf。
- 人工 check 与 completed follow-up：继续复用各自现有非 Runtime ACP prompt 许可。
- 用户再次点击停止：通过现有 active prompt cancel 取消当前 Agent 回复；业务状态继续保持 Paused。
- 应用关闭、session 删除、attempt 切换或显式 cancel 仍然可以终止该 prompt，不能因为允许 paused conversation 而失去取消能力。

这不是新增业务状态，而是让 prompt 执行保护识别“工作流执行”和“暂停后的普通聊天”属于不同调用目的。

## 9. 后端接口与路由

### 9.1 普通消息

统一 conversation submit 根据当前 surface 和 lifecycle 路由：

| 当前事实 | 提交目标 |
|---|---|
| Direct | 非 Runtime ACP prompt |
| Workflow `Running` | composer 锁定，不接受普通消息 |
| Workflow `Paused + ProcessInterrupted` | 非 Runtime ACP prompt，不恢复 workflow |
| `manual_check_pending` | 非 Runtime ACP prompt，等待成功/失败按钮 |
| Node / Run completed | 非 Runtime ACP follow-up |
| permission / elicitation pending | 进入对应结构化响应，不作为普通 prompt |

普通消息接口不得携带“恢复 Runtime”的隐式副作用。

### 9.2 Runtime continue

Runtime continue 成为纯动作接口：

- 由“继续工作流”按钮调用；
- 不接收可见用户 prompt 作为恢复判据；
- 后端自己渲染默认继续提示，并沿用现有 Runtime prompt 的展示与诊断策略；
- 复用现有 run / round / node / dynamic leaf re-arm、continue ref 和 scheduler；
- 接受后立即由后端 lifecycle 锁定 composer，避免旧 paused snapshot 短暂恢复输入。

通用 continue 的领域资格固定为：

- 允许 `ProcessInterrupted`、`RuntimeAbnormal`；
- 拒绝 `WaitingForUserInput`、`PermissionRequested`、`ErrorBlocked`；
- `manual_check_pending` 等待阶段属于 NonRuntime，自由对话不改变判定事实，只由成功/失败按钮调用独立 manual-check 接口推进；
- fixed 与 AI-DYNAMIC leaf 共同复用 `PauseReason::allows_explicit_runtime_continue`，不维护两套条件。

接口返回 `runtime-continue-started` 前必须完成一次启动握手：后台 driver 第一次把目标 Runtime 状态持久化为 Running 后发送一次性 started 信号，command 才返回接受。若 workflow 校验、continue ref、driver 初始化或线程创建在该事实前失败，command 同步返回 `runtime.continue-launch-failed`；前端不写 optimistic Running。握手后发生的非正常退出只对“仍是原 current active attempt”的状态做 CAS 收敛并刷新权威 session/lifecycle；用户已经 stop、节点已完成或 current attempt 已变化时不覆盖。AI-DYNAMIC 的失败收敛同时清除 starting/pending resume registry，并把 re-arm 后仍为 `Ready | Running` 的目标 leaf 收敛为 `RuntimeAbnormal`。

开发阶段明确替换旧语义，不保留“发送普通消息即 runtime continue”的兼容分支。

## 10. 前端交互

1. 用户停止完成后，composer 恢复可输入状态，普通发送按钮只发送非 Runtime 消息。
2. 停止态额外显示“继续工作流”主操作，不要求用户输入“继续”。
3. 点击继续后不插入可见用户消息；composer 立即进入 Runtime active / starting 锁定状态。
4. Agent 的 Runtime resume、finalize 和 repair prompt 不伪装成用户手写文本；`<hidden>` runtime context 沿用消息气泡中的默认收缩模块，独立 Runtime prompt 则沿用现有 visibility / reason 展示策略。
5. 工作流仍处于停止状态时，任意数量的 Agent 回复完成只更新 session，不刷新为 run completed，也不自动切换后继节点。
6. completed follow-up 与 Direct 继续沿用普通聊天体验，不显示“继续工作流”。
7. AI-DYNAMIC 只在选中的 paused leaf 上显示继续动作；点击后只恢复该 leaf。

## 11. 并发、幂等与恢复

1. 同一 session 的普通消息、hidden resume、finalize 和 repair 继续共享 ACP prompt lock，不能并行修改同一个会话。
2. “继续工作流”重复点击必须幂等：第一个请求接受并 re-arm 后，后续请求读取到 `Running` 或已进入 active prompt，返回 `runtime.continue-not-available` / `runtime.continue-already-active`，不重复发送 hidden resume。
3. 停止后两条用户消息并发提交时，prompt lock 保证只有第一条附带 `runtimeControlSuspended`。
4. 应用在停止后的自由对话期间关闭，Run 仍然是 `Paused + ProcessInterrupted`；启动恢复不创建新的 Runtime 状态。
5. 应用在 hidden resume 已接受但尚未完成时退出，按原 Runtime 恢复规则处理；若 emission state 已为 finalizing，只恢复 finalize。
6. 停止、resume 与 provider terminal 的竞态继续遵守“已落盘完整合法 artifact 才能完成”的现有规则；NonRuntime 输出永远不参与该完成收敛。
7. continue 接口只在 Running durable fact 建立后返回 started；启动前失败同步返回错误，启动后失败发布 authoritative lifecycle。
8. 迟到 continue 失败只能暂停原 active attempt，不覆盖用户 stop、完成事实或后续 attempt；dynamic re-arm 失败不得遗留 `Ready | Running` 无 owner 状态。

### 11.1 AI-DYNAMIC workspace 临界区与停止兑现

worktree 创建本身不拆成新的业务状态，也不禁止用户点击停止。AI-DYNAMIC 在 Graph 与 Git 必须同步变化的操作期间使用 `DynamicRunPhase::PreparingWorkspace`：

```text
Executing
  -> PreparingWorkspace
  -> Executing / Paused
```

覆盖范围包括 fanout checkpoint/fork、`DynamicNext::End` checkpoint、group merge 前 child checkpoint、group close 的 child release，以及整图完成时的 runtime workspace release。`Single` 只修改 Graph，不进入该阶段。

实现约束：

1. `PreparingWorkspace` 是 dynamic run 内部阶段，不新增 Run / Round / Node 暂停枚举。
2. 阶段开始与结束均持有既有 project-scoped dynamic state lock；Git 操作不得挪到锁外，不做补偿删除或两阶段 worktree 事务。
3. 阶段开始先持久化 `dynamic-run.json + graph.json` 并发布 session update；成功或失败后都恢复 `Executing`，再持久化完整 Graph catalog。
4. 用户点击停止后，前端沿用 `stopCommandPending / cancelling`，在等待期间持续显示“正在停止…”。workspace 交接窗口锚定 completed leaf 时，精确 session stop 升级为外层 run stop，先落盘 `Paused + ProcessInterrupted` 再等待 dynamic state lock；选中仍 active 的并行 leaf 时保留单 leaf stop，只把兑现时点延后到临界区结束。
5. 临界区释放后，停止逻辑暂停 descendants；scheduler 下一轮观察外层已停止，不再启动后继 Agent。
6. 停止不删除已经创建的 worktree；显式 continue 复用已有 workspace catalog/tree。
7. 旧 dynamic execution 的迟到成功结果，包括完整合法 completion，也不能跨越用户 stop boundary 恢复 Runtime。

Conversation VM 在外层仍 Running 且 phase 为 `PreparingWorkspace` 时，把当前 dynamic session 投影为 `runtime.phase / composer.processingKind = preparing-workspace`、锁定输入并保留 stop 能力；前端显示“正在准备开发环境…”。本地 stop pending 的 `stopping` 优先级高于该后端 phase。

### 11.2 性能约束与审视

该方案不会给正常 Runtime 执行增加额外业务 turn；分支判断只是 invocation 级枚举判断。停止后的自由对话还会跳过 artifact 提取、schema 校验、PostTurn finalize、repair 和 edge 调度，因此单次 NonRuntime turn 的 Runtime 侧工作量低于当前受控 turn。

需要在实现中固定以下约束：

1. 不得在每次普通消息提交时从头解析完整 ACP timeline 来判断是否需要 `runtimeControlSuspended`。控制模式切换事件写入稳定 transition metadata；stop accepted 控制面只读取 snapshot/session 小型 metadata，绝不触发 timeline 重建。旧 attempt 在后续普通恢复查询中首次缺少 cursor 时允许回扫一次 timeline并回填 snapshot/session，若没有 transition 则写入 `runtimeControlTimelineScanComplete` negative cache。之后只读取小型 metadata，在同一 session prompt lock 下完成候选判断与 accepted commit。
2. 不新增轮询器。RuntimeControlled 继续复用现有 stop probe；NonRuntime prompt 复用 ACP active prompt cancel / session disposal 能力，不因为 Run 保持 Paused 而高频轮询 `run.json`。
3. `<hidden>` 脱离说明只在真正的 Runtime → NonRuntime 切换后附加一次，避免每次 stop 或每条消息重复扩大会话上下文。NonRuntime → NonRuntime 的再次停止不增加 prompt token。
4. 普通 resume 只增加恢复工作所必需的一个短 Runtime turn；`finalizing` 恢复直接复用重新发起的 artifact finalize turn，不先发送通用 resume，因此不会产生两个连续控制 turn。
5. finalizing 被中断后重新生成完整 artifact 会增加一次短模型调用，但不会重跑通常更昂贵的业务 turn。这是用有限成本换取 artifact 完整性，不能用拼接中断文本或接受 partial candidate 的方式优化。
6. `TurnControlMode`、emission mode 与 transition cursor 必须在进入 provider 前一次派生并随 invocation 传递，不能在流式 token、timeline event 或 artifact chunk 处理中重复读取生命周期文件。
7. suspended context 的认领和同 session prompt 串行继续复用现有 ACP prompt lock；stop 与 accepted commit 对 cursor 小文件的并发写入使用固定 64 路 attempt path 哈希短锁，避免新增会随 attempt 数增长并在热路径全表清理的锁注册表。该短锁不覆盖 provider 调用；固定 workflow 的 per-run starting lease 也只覆盖启动窗口，不持有全局 lifecycle 锁等待整个 Agent turn。不同 run、session 与 AI-DYNAMIC leaf 仍可并行执行。
8. continue 启动握手使用单次进程内 channel 通知，不轮询磁盘；Running 落盘后立即释放 fixed per-run starting lease。失败 CAS 使用固定 64 路 attempt 状态短锁或既有 dynamic graph lock，只覆盖少量 JSON 状态收敛，不覆盖 provider turn，也不创建随历史 attempt 增长的锁对象。
9. `PreparingWorkspace` 不增加轮询、后台任务或 Agent turn。每次 transition 只新增开始阶段的两次权威 JSON 原子写入与两次 session refresh；结束阶段复用本来就需要的完整 Graph 持久化。worktree Git 操作仍受既有全局 Git 锁串行化，不降低不同 Agent session 的并行度；同一 graph 的 stop/continue 等待临界区是有意的一致性约束。

按以上约束，主要新增成本只有模式切换时的一条小型 timeline metadata 和恢复后必要的控制 prompt；不存在随消息数线性增长的热路径扫描，也不降低不同 session 的并行度。

## 12. 结构化错误

新增或调整的接口错误必须继续使用结构化 Runtime 错误，不返回对客字符串。建议错误码：

- `runtime.conversation-attempt-not-current`
- `runtime.conversation-not-available`
- `runtime.continue-not-available`
- `runtime.continue-already-active`
- `runtime.continue-launch-failed`
- `runtime.continue-launch-channel-closed`
- `runtime.control-boundary-invalid`

后端只返回 `code / params / raw diagnostic`；前端根据 i18n 映射展示用户动作。

## 13. 预计修改范围

### Runtime / Provider

- `src/provider/mod.rs`
  - 引入 invocation 级 `TurnControlMode`；
  - NonRuntime turn 绕过 artifact output policy、post-turn projection 与 result payload 提取；
  - resume / finalize / repair 的 Runtime visibility 与 reason 收敛。
- `src/app/orchestrator.rs`
  - 普通消息与 runtime continue 路由拆分；
  - Runtime → NonRuntime 切换后首次收缩 context 判定；
  - 固定工作流与 AI-DYNAMIC leaf 复用相同控制模式。
- `src/app/node_executor.rs`
  - RuntimeControlled 执行继续使用现有完成归一化；NonRuntime 不进入 attempt finalize。
- `src/acp/client.rs`
  - paused conversation 执行许可；
  - 保留显式 cancel、app close 和 attempt 切换保护。

### Prompts

- `src/prompts/zh-CN/runtime/runtime_control_suspended.md`
- `src/prompts/en/runtime/runtime_control_suspended.md`
- `src/prompts/zh-CN/runtime/runtime_control_resume.md`
- `src/prompts/en/runtime/runtime_control_resume.md`
- `src/prompts.rs`

### Desktop / Web

- `src-tauri/src/commands.rs`
  - 普通会话消息与 runtime continue 使用独立 command 语义。
- `src-tauri/src/view_models_conversation.rs`
  - paused composer 可输入并提供显式 continue action。
- `web/src/components/conversation/*`
  - 停止态普通发送与“继续工作流”按钮分离；
  - `<hidden>` runtime context 继续使用消息气泡中的默认收缩模块；Runtime resume 不伪装成用户手写消息。

## 14. 测试计划

### 14.1 Rust 单元测试

1. Direct turn 派生为 NonRuntime，不解析 artifact。
2. 正常 workflow worker 派生为 RuntimeControlled，并保持 PostTurn 两阶段流程。
3. workflow `Paused + ProcessInterrupted` 的普通消息保持 paused，Agent 回复成功后不写 artifact、不完成节点、不进入 edge。
4. Runtime → NonRuntime 后第一条已接受消息包含 `runtimeControlSuspended` 收缩 context，第二条不包含。
5. NonRuntime 对话回复被再次停止后，下一条消息不重复包含 `runtimeControlSuspended`。
6. 工作流恢复为 Runtime 后再次停止，新 Runtime → NonRuntime 边界后的第一条消息重新包含 context。
7. 第一条消息在 ACP 接受前失败时，下一条仍包含 context；接受后回复被取消时不重复包含。
8. InlineControl 停止后，原 contract 仍存在，但 NonRuntime turn 不启用 artifact policy；普通文本不会产生 unidentified-output failure。
9. PostTurn business 停止后，NonRuntime 回复不会触发 `artifact-emission.json` 或 finalize。
10. PostTurn finalizing 停止后，用户消息不会被 finalize prompt 覆盖，`artifact-emission.json(finalizing)` 保持不变。
11. finalizing 状态点击继续时跳过业务 turn，丢弃中断候选，只重新发送一次完整 finalize；合法新输出才可完成节点。
12. 普通 paused workflow 点击继续时只发送一次 `runtimeControlResume`，不伪装成用户手写 prompt。
13. AI-DYNAMIC paused leaf 普通聊天不恢复 graph；点击继续只恢复目标 leaf。
14. NonRuntime turn 即使输出合法 artifact JSON，也不生成 `ProviderResultPayload`，不改变 outcome。
15. Runtime continue 重复提交只发送一次 resume。
16. paused conversation 中再次 stop 可以取消当前 prompt，workflow 仍保持 paused，control transition identity 不变化。
17. 长 timeline 下连续提交 NonRuntime 消息不重复全量扫描事件；control cursor 在恢复时构建一次，提交热路径保持常量级判定。
18. fixed / AI-DYNAMIC 通用 continue 只允许 `ProcessInterrupted | RuntimeAbnormal`；manual check、permission、elicitation/waiting 与 ErrorBlocked 均拒绝。
19. continue 启动前失败同步返回结构化错误且保持原 paused 事实，不返回 started。
20. 已握手后的 fixed / AI-DYNAMIC 意外失败收敛为 RuntimeAbnormal；dynamic re-arm 不遗留 Ready/Running。
21. 用户 stop 先完成时，迟到失败不覆盖 ProcessInterrupted；目标完成或 attempt 切换时同样不回写旧状态。

### 14.2 前端单元测试

1. `Paused + ProcessInterrupted` composer 可输入，同时展示“继续工作流”。
2. 停止态发送普通文本调用 conversation prompt command，不调用 runtime continue。
3. 点击“继续工作流”调用 continue command，不携带可见用户 prompt。
4. `<hidden>` runtime context 保持默认收缩且可展开；resume / finalize / repair 沿用既有 Runtime prompt 展示策略，不渲染成用户手写文本。
5. Agent 普通回复结束后 run 仍 paused，“继续工作流”按钮仍存在。
6. Direct、completed follow-up 和 manual check 普通消息行为不回归。
7. AI-DYNAMIC 选中 paused leaf 时 continue action 只携带目标 leaf locator。

### 14.3 页面验证

1. 普通 workflow worker 输出中点击停止；停止后连续追问两轮，确认两轮均可正常回复且 workflow 不推进；点击继续后恢复原节点并最终进入后继节点。
2. AI-DYNAMIC bootstrap 输出中停止；发送普通问题，确认 Agent 收到停止 runtime context 且普通回复不被判 artifact invalid；点击继续后重新输出控制 artifact。
3. PostTurn worker 业务回复完成、finalize 期间停止；发送普通问题，确认问题没有被 finalize prompt 覆盖；点击继续后重新请求完整 artifact、只接受新 finalize 输出，且不重跑业务任务。
4. Runtime → NonRuntime 后第一条消息检查 raw prompt，确认包含一次 `runtimeControlSuspended` 且 UI 显示为默认收缩的运行上下文；第二条消息不再包含。随后停止该 NonRuntime 回复，确认下一条仍不重复注入。
5. 点击继续检查 timeline，确认只产生一次 NonRuntime → Runtime 提示，并沿用既有 Runtime prompt 展示策略。
6. 工作流结束后继续追问，确认回复只作为会话内容，不重复写 artifact 或再次完成 run。

## 15. 文档同步

实施时同步维护：

- `docs/gold-band/产品设计文档/runtime/control.md`
- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/开发计划/gold-band-mvp-plan.md`
- `docs/gold-band/开发计划/生命周期整理/工作流-ACP-生命周期统一重构.md`

重点删除旧的“停止后在 composer 输入消息即 runtime continue”描述，统一改为“普通消息保持 NonRuntime；显式按钮恢复 Runtime”。

## 16. 实施顺序

1. 在 provider invocation 中引入 `TurnControlMode`，先用单元测试固定 Runtime / NonRuntime 的 artifact 与状态推进差异。
2. 抽取同 session NonRuntime ACP prompt helper，让 stopped workflow、manual check、completed follow-up 与 Direct 共享。
3. 拆分普通消息提交和 runtime continue；继续动作使用 Runtime 默认提示，不伪装成用户手写文本。
4. 接入控制模式 transition identity 与 Runtime → NonRuntime 后首次收缩 runtime context，并固化跨页面、重启、并发和 NonRuntime 再次停止的幂等测试。
5. 接入 InlineControl 与 PostTurn finalizing 的 NonRuntime 分流。
6. 更新 desktop lifecycle VM 和前端 composer / continue action。
7. 更新产品设计文档、MVP 计划与旧生命周期方案中的过时描述。
8. 运行 Rust、Web 单元测试，并按 deep link 完成普通 workflow、AI-DYNAMIC InlineControl 和 PostTurn finalizing 三条页面验证。

## 16.1 实施结果

截至 2026-08-10，本方案已落地以下主链路：

1. provider invocation 新增 `TurnControlMode`；NonRuntime turn 保留原始 `output_contract`，但绕过 PostTurn projection、artifact result payload、校验、repair 与状态推进。
2. stop 在 RuntimeControlled → NonRuntimeControlled 时写入既有 ACP snapshot/session 的 `runtimeControl` metadata；NonRuntime 对话再次 stop 不创建新 transition。cursor 热路径只在当前 attempt 确实处于 `Paused + ProcessInterrupted` 时启用，Direct、completed follow-up 与 manual check 不读取或回扫 timeline。
3. 停止后的第一条已接受用户消息附加一次默认收缩的 `<hidden>` runtime context；transition id 与 accepted prompt id 一起持久化，后续消息不重复附加。
4. 普通 `submit_conversation_prompt` 不再隐式恢复 workflow；新增 `continue_conversation_runtime` 纯动作 command，固定工作流与 AI-DYNAMIC leaf 均由 locator 精确恢复。
5. lifecycle VM 将可恢复暂停投影为 `continueKind=action + composer.mode=normal + submitTarget=acp-prompt`；Web composer 保持可聊天，并独立显示“继续工作流”按钮。
6. 默认继续使用隐藏 `RuntimeResume` prompt 与 `runtimeControlResume` reason，不生成 optimistic 用户气泡；中英文 prompt 已统一进入 `src/prompts/<language>/runtime/`。
7. PostTurn `finalizing` 被中断后不接受任何 interrupted candidate；继续时由既有 emission phase 跳过业务 turn并重新请求完整 finalize。InlineControl 同样不能让停止落盘后的迟到 completion 跨越 stop boundary；只有停止前已校验并提交的 artifact 保持既有完成事实。
8. suspended context 的待发送判断移动到 ACP session prompt lock 内；同一 session 并发提交只有第一条能够认领 transition。cursor commit 失败不再被 callback 吞掉，accepted event 和一次性 context 状态保持同一持久化边界。
9. `WorkflowContinued` 改为 prepare / commit 两阶段：provider 接受前 cursor 仍是 NonRuntime，accepted user prompt event 落盘后才使用 source transition id 做 CAS。新的 stop 边界不会被迟到 resume 覆盖，初始化或接受失败也不会造成提前恢复。
10. 固定 workflow continue 在任何 run 状态读取和后台 spawn 前获取 per-run starting lease；同一 run 的双击只接受一个请求，lease 随后台线程结束或 unwind 自动释放，不同 run 不互相阻塞。AI-DYNAMIC 继续复用既有精确 leaf scheduler / starting window。
11. legacy cursor 缺失时最多扫描 timeline 一次，无结果时在 snapshot/session 持久化 `runtimeControlTimelineScanComplete` negative cache；cursor 写入使用固定 64 路路径哈希短锁，不增加随历史 attempt 数量增长的热路径全表扫描。
12. Direct / `RawAgent` 首轮直接派生为 `NonRuntimeControlled`；PostTurnProjection 业务 turn 的动态 Profile 变量不会提前启用路由协议，原始 contract 仅由隐藏 finalize 消费。
13. Rust 与前端回归覆盖控制游标、NonRuntime 重复 stop、accepted 后 resume commit、stale resume CAS、legacy negative cache、并发首次 context、固定 continue lease、Direct 首轮、NonRuntime stop probe、暂停态 composer、隐藏 resume、原 contract 保留但 Runtime 消费停用，以及 PostTurn 中断输出不可信；Provider 接口测试同步删除 artifact 前置旧断言。
14. AI-DYNAMIC workspace transition 统一进入持久化 `PreparingWorkspace` 临界区；正常时 composer 显示“正在准备开发环境…”，停止 pending 显示“正在停止…”，并在 checkpoint/fork/release 完成后兑现暂停。停止不回滚或删除已创建 worktree，continue 复用 workspace catalog/tree。
15. AI-DYNAMIC 集成夹具已按当前控制协议固化：proposal 不再输出 Runtime-owned `workspace` 字段；普通业务 invocation、`RuntimeFinalize`、`RuntimeRepair` 与显式 `UserMessage` 按 render mode 区分；finalize 不重复携带业务附件；merge 读取 checkpoint 后的 `forkCommit / checkpointCommit / clean status`。
16. Conversation VM 以 dynamic run 是否仍拥有执行权判断 Runtime active，而不是只看 selected leaf/ACP 是否 terminal；completed leaf 到下一节点之间投影 `launching-next-node`，进入 workspace 临界区后投影 `preparing-workspace`，两者都保持 composer 锁定与停止入口。

## 17. 不采用的方案

1. 不新增 `PausedConversation` 持久化状态：现有 `Paused + ProcessInterrupted` 已经表达业务停止，新增状态会与 Run / Node 生命周期重复。
2. 不把 `manual_check_pending` 用作停止后聊天：两者完成阶段不同。
3. 不让普通用户消息继续调用 runtime continue：这会继续把 Agent 回复结束等同于节点结束。
4. 不根据用户消息内容自动恢复：自然语言不能作为 workflow 控制信号。
5. 不删除原始 `output_contract`：contract 是节点 Runtime 控制契约，恢复时必须原样复用。
6. 不仅依赖 Agent 是否遵从收缩 runtime context：NonRuntime 模式必须从 Runtime 侧禁止 artifact 校验与状态推进。
7. 不把每个 stop event 都当作新边界：只在控制模式实际发生 Runtime → NonRuntime 时注入解除说明，NonRuntime → NonRuntime 不重复注入。
8. 不信任被中断的 finalize 输出，也不尝试拼接或检查其“看起来是否完整”：恢复时重新请求完整 artifact，以新 turn 的 terminal success、解析和 schema 校验作为唯一验收入口。
