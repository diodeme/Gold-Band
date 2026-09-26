# ACP 回合终态重开与自动重试准入根因修复方案（P0）

## 状态

- 类型：根因修复方案（P0）
- 状态：待实施
- 关联问题：用户新开 Claude 会话稳定报错 `ACP 会话失败 acp.prompt-execution-already-settled`
- 关联文档：
  - `docs/gold-band/开发计划/生命周期整理/工作流Runtime执行阶段与ACP生命周期解耦技术方案.md`
  - `docs/gold-band/开发计划/生命周期整理/ACP停止语义与Adapter长连接开发方案.md`
  - `docs/gold-band/rules/state-lifecycle-and-data-integrity.md`
  - `docs/gold-band/rules/bug-fix-verification.md`
- 不包含：`initialize` 的 60s 硬编码超时的根因修复（属于 P1，另立方案）；初始化投影与超时定位补充见第十二节。

## 一、问题现象

用户在客户端新开 Claude 会话，稳定报错：

```
ACP 会话失败 acp.prompt-execution-already-settled
```

命令行单独驱动同一个 adapter（`npx -y @agentclientprotocol/claude-agent-acp@0.72.0`）执行 `initialize -> session/new -> session/prompt` 全部成功，说明 adapter 本身可用，问题落在客户端的回合生命周期内。

## 二、复现与证据

- `D:\Test\feedback-19-session`：新会话，prompt 为 "hello"。`acp.snapshot.json` 中两次 `initialize` 均为出站、零入站，`elapsedMs=60037 status=timeout`，随后第二次 `elapsedMs=60055 status=timeout`。
- 同目录 `acp.snapshot.json` 的 `turnError` 正确记录了真实原因：

```
{"code":"runtime.transport-interrupted","recovery":"auto","diagnostic":"ACP `initialize` timed out after 60 seconds"}
```

- 同目录 `events.jsonl`：`runtime_auto_retry`（1/3，delayMs=1000）之后紧接着 `run_paused ... acp.prompt-execution-already-settled`。
- 结论：真实失败原因是 initialize 超时（属于 P1 的环境与超时问题），但用户看到的错误被准入拒绝覆盖了。

## 三、根因分析

### 3.1 分类

原有生命周期设计缺陷。既不是业务逻辑写错，也不是单纯的实现遗漏。

设计意图：一个用户可见回合（logical turn）在其 runtime 被重建多次时保持同一个 `turnId`，由 orchestrator 在调度边界分配（`logical_prompt_id`），从而保证同一次用户输入在时间线、用量与停止控制上始终是同一轮。

### 3.2 缺陷形成路径

1. 第 1 次尝试：provider 准入写入 `starting` 并抢占（`admit_session_turn_for_execution` -> `begin_session_turn`）。
2. `client::run_prompt` 用 `AcpLifecycleTerminalGuard` 包住整段执行（`src/acp/client.rs:2785`）。
3. initialize 超时返回 Err，守卫执行 `persist_session_turn_failure_owned`，把这一轮写成终态：`liveTurnActivity=idle`、`latestTurnStatus=failed`、`turnError.recovery=auto`。
4. orchestrator 看到 `recovery=auto`，`auto_retry_delay_ms` 返回退避时间，写 `runtime_auto_retry` 事件，`continue` 进入第 2 次尝试。
5. 第 2 次尝试再次准入。`turnId` 与上次相同（刻意复用），`classify_existing_turn` 命中 `lifecycle_is_terminal -> ExistingTerminal` -> `AcpTurnExecutionClaim::AlreadySettled` -> 在 `src/provider/mod.rs:1685` 执行 `bail!("acp.prompt-execution-already-settled")`。

也就是说：结算与准入各自判断这一轮是否还能继续，两套判断源不一致。结算写死了 `failed` 终态，准入把任何终态一律视为不可再用，于是自动重试必然被自己上一次的结算挡回。

### 3.3 为什么不能简单删掉准入检查

准入检查承担两个必要职责：

- 防止同一轮的重复执行者并发提交（幂等）。
- 防止用户已停止（`cancelRequested`）的一轮被错误复活。

因此修复方向不是放宽或删除准入，而是补一个显式的终态重开转换，并让是否可重开只由持久化事实（`turnError.recovery`）判定。

## 四、现状模型

关键数据结构（`src/acp/events.rs`）：

- `AcpLifecycleHeader`：`revision`、`turn_id`、`prompt_event_id`、`availability`、`live_turn_activity`、`latest_turn_status`、`stop_reason`、`turn_error`、`operation_id`。
- `AcpLiveTurnActivity`：`Idle | Starting | Accepted | Running | CancelRequested`。
- `AcpLatestTurnStatus`：`None | Completed | Cancelled | Failed`。
- `AcpTurnAdmission`：`Started | ExistingActive | ExistingTerminal`。
- `AcpTurnExecutionClaim`：`Claimed | AlreadySettled | Stale`。
- `AcpLifecycleOwner { turn_id, operation_id, revision }`：准入与结算共用的 CAS 元组。

关键判定与转换：

- `lifecycle_is_terminal` 等于 `live_turn_activity == Idle && latest_turn_status != None`。
- `classify_existing_turn`（events.rs:2255）：`turn_id` 相同且为终态或停止态时返回 `ExistingTerminal`。
- `begin_session_turn`（events.rs:2652）：`classify_existing_turn` 命中即直接返回，不再写盘。
- `reduce_lifecycle_header`（events.rs:2068）与 `AcpLifecycleTransition`（events.rs:2056）：目前只有 `PromptAdmitted`、`ExecutionClaimed`、`StopRequested`、`TurnSettled`。
- `normalize_lifecycle_header`（events.rs:2039）：非 `Idle + Failed` 时清空 `turn_error`；非 `Idle` 时把 `latest_turn_status` 置 `None`、清空 `stop_reason`。

## 五、修复方案

### 5.1 单一判定源

这一轮能否重开，只允许由一个持久化事实证明：`latest_turn_status == Failed && turn_error.recovery == Auto`。调用方只能声明我这次是自动重试，不能自行决定可重开；真正的重开由准入在元数据锁内完成 CAS 写入。

### 5.2 新增状态转换 `PromptReopened`

在 `AcpLifecycleTransition`（events.rs:2056）新增：

```
PromptReopened { operation_id: &'a str }
```

在 `reduce_lifecycle_header` 中实现。语义与 `PromptAdmitted` 相同的重新开始，但保留 `turn_id`：

- `turn_id`：保持不变（同一逻辑轮）。
- `operation_id`：改为新的 operation id。
- `prompt_event_id`：置 `None`。
- `live_turn_activity`：置 `Starting`。
- `latest_turn_status`：置 `None`。
- `stop_reason`：置 `None`。
- `revision`：由 `reduce_lifecycle_header` 统一加一。
- `availability`：沿用当前值，不伪造；既有 `normalize_lifecycle_header` 会因 `live_turn_activity != Idle` 清空 `turn_error`。

`turn_error` 被清空是可接受的：上一轮的失败原因已由 `runtime_auto_retry` 事件与 attempt 诊断留存，重开后的活动轮不应再携带终态错误。

### 5.3 准入新增内部可重开分类

公开 `AcpTurnAdmission` 保持 `Started / ExistingActive / ExistingTerminal` 三种结果；新增内部 `ExistingTurnClassification::{New, Active, Terminal, AutoRetryable}`，仅由 `classify_existing_turn` 与 `begin_session_turn` 消费。

`classify_existing_turn`（events.rs）调整判定顺序：

1. `turn_id` 不同：返回 `None`（不变）。
2. submission 内容冲突：`bail!("acp.prompt-submission-conflict")`（不变）。
3. 新增：`automatic_retry == true` 且 `lifecycle_is_terminal(&current)` 且 `current.latest_turn_status == Failed` 且 `current.turn_error.recovery == Auto` 时返回 `AutoRetryable(current)`。
4. `lifecycle_is_terminal` 或 `lifecycle_is_stopping`：返回 `Terminal`（不变）。
5. 其余：返回 `Active`（不变）。

条件 3 要求 `lifecycle_is_terminal`，因此 `CancelRequested`（停止态）不会被重开；`recovery` 为 `Manual` 或 `Blocked` 的失败同样不会被重开。

### 5.4 `begin_session_turn` 执行重开写入

`begin_session_turn(path, submission, automatic_retry)`：

- 当 `classify_existing_turn` 返回 `AutoRetryable` 时，不直接返回，而是在同一把 `session_metadata_lock` 内：
  1. 重新读取并再次确认 `turn_id` 与条件 3 完全一致，防止锁外漂移。
  2. 以 `submission.operation_id` 作为新的 operation id 应用 `PromptReopened`。
  3. 覆盖 `promptSubmission`、更新 `updatedAt`、写盘。
  4. 调用 `mark_session_turn_active(path, turn_id)`。
5. 返回 `AcpTurnAdmission::Started(starting)`，因此公开枚举不暴露 `AutoRetryable`。
- 其余分支行为不变。

因此 `admit_session_turn_for_execution`（events.rs:2358）会走 `Started` 加 `Starting` 分支，成功 `claim_session_turn_for_execution`，不再返回 `AlreadySettled`。

### 5.5 调用方声明重试意图

- `WorkerInvocation`（`src/provider/mod.rs:333`）新增 `#[serde(default)] pub automatic_prompt_retry: bool`。
- 赋值来源：orchestrator 的 `auto_retry_attempts > 0`（worker 路径 `execute_ai_node` 与 dynamic 路径 `build_dynamic_worker_invocation` 返回后显式赋值），再传入 provider。
- provider 在调用准入时把该标志传给 `begin_session_turn` 或 `admit_session_turn_for_execution`。
- 该标志不得进入 `AcpPromptSubmission`。该结构会被持久化并用于冲突比较，把标志放进去会触发 `acp.prompt-submission-conflict`；它是调用方意图，不是轮次内容。

### 5.6 旧执行者与新执行者的隔离

重开后 `operation_id` 变化且 `revision` 加一，因此第 1 次尝试遗留的 `AcpLifecycleOwner`（旧 operation id）无法再通过 CAS：其迟到写盘会被 `persist_session_turn_terminal_with_error_owned` 的 `operation_id` 校验判为 no-op。无需新增额外机制。

## 六、不变量与拒绝矩阵

| 现有终态 | 调用方声明自动重试 | 结果 |
| --- | --- | --- |
| `failed` 且 `recovery=auto` | 是 | 重开，返回 `Started`（新 operation id，revision 加一） |
| `failed` 且 `recovery=manual` | 是 | `AlreadySettled`，即 `acp.prompt-execution-already-settled` |
| `failed` 且 `recovery=blocked` | 是 | `AlreadySettled` |
| `failed`（任意 recovery） | 否 | `AlreadySettled` |
| `completed` | 是或否 | `AlreadySettled` |
| `cancelled` | 是或否 | 保持停止优先，按既有取消语义处理 |
| `cancelRequested`（停止态） | 是或否 | 不重开，按既有 `acp_turn_was_cancelled_before_execution` 处理 |
| 同一轮 `accepted/running`（重复提交） | 是或否 | 保持既有幂等语义（`ExistingActive`） |

不变量：

1. 一次输入内 `turn_id` 不变，`operation_id` 每次尝试必须唯一。
2. 从终态回到活动态的唯一合法路径是 `PromptReopened`，且必须满足条件 3。
3. 重开必须与判定在同一把元数据锁内原子完成。
4. 结算与重开共用同一 owner 与 CAS 元组语义，不得引入第二套身份。

## 七、变更清单

| 文件 | 变更 |
| --- | --- |
| `src/acp/events.rs` | 新增 `AcpLifecycleTransition::PromptReopened` 与 reduce 分支；新增内部 `ExistingTurnClassification`；`classify_existing_turn` 增加可重开判定；`begin_session_turn` 增加 `automatic_retry` 参数与重开写入；`admit_session_turn_for_execution` 传递该参数 |
| `src/provider/mod.rs` | `WorkerInvocation` 新增 `automatic_prompt_retry`；prompt 执行处把该标志传入准入 |
| `src/app/node_executor.rs` | `execute_ai_node` 接收自动重试标志并在 build 后赋值 |
| `src/app/orchestrator.rs` | worker 路径与 dynamic 路径把重试计数传为标志 |
| `src/acp/control.rs`、`src/storage/sqlite.rs` | 既有 `begin_session_turn` 调用点补 `automatic_retry = false` |
| `src/acp/events.rs` 测试模块 | 新增最小失败测试与对照用例 |

P1 初始化/超时定位补充涉及 `src/acp/client.rs`（超时错误上下文）、`src/acp/connection.rs`（非 noise stderr 有界缓冲与尾部）、`web/src/i18n.ts` 与 `web/src/components/acp/ACPChatDialog.tsx`（composer 初始化文案）。

如果采用直接改签名策略，需同步更新 `begin_session_turn` 的全部调用点与既有单测。按项目开发阶段破坏式更新规则，不保留兼容包装。

## 八、最小失败测试

先写测试并确认失败，再改实现（遵循 `docs/gold-band/rules/bug-fix-verification.md`）。测试位置：`src/acp/events.rs` 测试模块。

1. `automatic_retry_reopens_a_recoverable_failed_turn`
   - 准备：写入终态 `failed + recovery=Auto` 的 header。可用 `begin_session_turn` 起一轮后 `drop(AcpLifecycleTerminalGuard)`，或直接调 `persist_session_turn_failure_owned`。
   - 动作：同一 `turn_id`，`automatic_retry = true` 再次准入。
   - 断言：得到 `Started`；`operation_id` 与上次不同；`revision` 为上次加一；`live_turn_activity == Starting`；`latest_turn_status == None`；`turn_error == None`。
   - 预期在修复前失败（当前返回 `ExistingTerminal` 或 `AlreadySettled`）。
2. `automatic_retry_rejects_non_recoverable_terminal_turns`（对照）
   - `recovery=Manual`、`recovery=Blocked`、`latest_turn_status=Completed`、`automatic_retry=false` 四种输入均须保持 `ExistingTerminal`。
3. `automatic_retry_does_not_reopen_a_cancelled_turn`（对照）
   - `cancelRequested` 状态不得被重开，仍按既有取消语义返回。
4. `late_settlement_from_replaced_operation_is_a_no_op`
   - 重开后用旧的 `AcpLifecycleOwner` 调 `persist_session_turn_failure_owned`，断言返回 `None` 且 header 未被改写。

验收方式：`cargo test` 相关用例全绿；四类对照用例证明修复没有放宽其它终态。本机运行 cargo 需先设置 `CARGO_BUILD_JOBS=1` 与 `RUSTFLAGS=-C debuginfo=0`。

## 九、影响面与风险

- 影响：仅 ACP 回合准入路径。Direct、Workflow、AI-DYNAMIC 共用同一 provider 边界，行为一致。
- 不改动：时间线投影、停止控制、用量统计、SQLite 索引。
- 风险 1：误重开不可恢复的失败。由 5.3 条件 3 与第八节对照用例约束。
- 风险 2：重开后 `turn_error` 被清空，导致上次失败原因在 lifecycle 上不可见。缓解：失败原因仍由 `runtime_auto_retry` 事件与 attempt 诊断留存；若需 UI 展示重试中，在 P1 可观测性方案处理，不在本方案新增字段。
- 风险 3：`initial-direct-turn` 是全局常量，多轮之间复用同一 id。本方案不改变轮次身份生成契约，仅在此记录；若需按轮隔离，另立方案。

## 十、方案自评审

过度设计评审：新增面为一个状态转换、一个准入变体、一个 bool 标志，不新增状态机、队列、缓存或持久字段；`operation_id` 与 `revision` 复用既有 CAS 语义。这与一个用户轮次可重建多次 runtime 的既有设计一致。被否决的更省事做法（复用 `ExistingActive`、按错误码打补丁）会引入语义含糊或回到补丁式修复。结论：与实际问题规模匹配，无过度设计。

性能影响评审：重开只在自动重试路径发生，多一次同锁内的 JSON 读写（O(1)，非热路径）；标志位为内存字段。无全量扫描、无 N+1、无无界缓存、无长持锁。结论：无明显性能风险。

## 十一、实施步骤

1. 编码前完整阅读 `docs/gold-band/rules/state-lifecycle-and-data-integrity.md` 与 `docs/gold-band/rules/bug-fix-verification.md`。
2. 先写第八节测试，确认失败原因与本文第三节一致。
3. 实现第五节的状态转换、准入变体与调用方标志。
4. 运行 `cargo test` 目标用例与 ACP 相关回归。
5. 同步维护 `docs/gold-band/产品设计文档`（生命周期状态机文档需补充 `PromptReopened` 转换与结算与重开共用判定源契约）与 `docs/gold-band/开发计划`。
6. 用 `D:\Test\feedback-19-session` 或等价路径核对修复后同一场景不再出现 `acp.prompt-execution-already-settled`。

## 十二、初始化超时定位与投影补充（P1）

1. **初始化进度信号**：ACP 初始化开始后、`sessionId` 尚未到达前，通过现有 `initializing` 投影把当前运行节点的初始化状态投到 composer，展示「正在准备 Agent」。复用 [会话启动性能与MCP健康检查解耦设计.md](D:/IdeaProjects/tmp/Gold-Band/docs/gold-band/产品设计文档/会话启动性能与MCP健康检查解耦设计.md) 的「会话初始化状态」规则；只补投影来源，不新增状态机。
2. **超时错误带定位上下文**：把解析后的 adapter 命令、`PATH` 头部、`node` 版本、adapter `stderr` 尾部写入该错误的结构化 `params`/`raw`。同时处理已接线但从未被消费的 `log_provider_command`（定义见 [provider/mod.rs:386](/D:/IdeaProjects/tmp/Gold-Band/src/provider/mod.rs:386)，赋值见 [node_executor.rs:648](/D:/IdeaProjects/tmp/Gold-Band/src/app/node_executor.rs:648)）：要么在日志路径真正使用，要么删除，不能留下误导排查的死字段。
3. **不再吞 adapter stderr**：`classify_adapter_stderr` 保留 noise 归类；非 noise 的 `Diagnostic` 不再只走 `debug!`，改为有界落盘到 per-attempt 结构化诊断；`tracing` 仍保留 debug 级别，避免刷屏。

## 十三、待确认

- 是否把结算与重开必须共用同一判定源沉淀为 `docs/gold-band/rules/state-lifecycle-and-data-integrity.md` 的一条规则。按团队共享协作规则，需明确同意后才写入。
- 本方案新增 P1 的初始化投影与超时定位补充；`SessionStart` hook 慢启动本身的性能根因仍不覆盖，另立方案。
