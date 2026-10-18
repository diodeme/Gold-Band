# ACP 回合终态重开与自动重试准入根因修复方案（P0）

## 状态

- 类型：根因修复方案（P0）
- 状态：已实施
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

命令行单独驱动同一个 adapter 执行 `initialize -> session/new -> session/prompt` 全部成功，说明 adapter 本身可用，问题落在客户端的回合生命周期内。

## 二、复现与证据

- 新会话，prompt 为 "hello"。`acp.diagnostics.jsonl` 中 `initialize` 为出站、零入站，`elapsedMs=60037 status=timeout`。
- `acp.snapshot.json` 的 `turnError` 记录了真实原因：

```
{"code":"runtime.transport-interrupted","recovery":"auto","diagnostic":"ACP `initialize` timed out after 60 seconds"}
```

- `events.jsonl`：`runtime_auto_retry`（1/3，delayMs=1000）之后紧接着 `run_paused ... acp.prompt-execution-already-settled`。
- 结论：真实失败原因是 initialize 超时（P1 的环境与超时问题），但用户看到的错误被准入拒绝覆盖。

## 三、根因分析

### 3.1 分类

原有生命周期设计缺陷。设计意图：一个用户可见回合（logical turn）在 runtime 被重建多次时保持同一个 `turnId`，由 orchestrator 在调度边界分配（`logical_prompt_id`）。

### 3.2 缺陷形成路径

1. 第 1 次尝试：provider 准入写入 `starting` 并抢占（`admit_session_turn_for_execution`）。
2. `client::run_prompt` 用 `AcpLifecycleTerminalGuard` 包住整段执行。
3. initialize 超时返回 Err，守卫执行 `persist_session_turn_failure_owned`，把该 attempt 写成 `idle + failed + turnError.recovery=auto`。
4. orchestrator 看到 `recovery=auto`，写 `runtime_auto_retry` 事件并重试。
5. 第 2 次尝试用相同 `turnId` 准入，`classify_existing_turn` 命中终态 -> `AlreadySettled` -> `acp.prompt-execution-already-settled`。

结算把 attempt 写成终态，准入把任何终态都视为不可再用，自动重试必然被自己上一次的结算挡回。

### 3.3 为什么不能只加一个「我是重试」标志

准入检查承担三个职责：防止重复执行者并发提交（幂等）、防止已停止的轮次被复活、防止重放已派发给 Agent 的业务 prompt（`classify_acp_prompt_run` 的既有原则：重放可能重复部分副作用）。

一个 bool 重试标志无法满足后两条：它不知道自己要替换的是哪一个 attempt，也不知道失败发生在 prompt 派发前还是后。修复必须让重开同时满足：

- **身份 CAS**：重试必须点名它替换的那个失败 attempt，准入校验它等于持久化 lifecycle owner。
- **未派发证明**：只有 prompt 从未交给 Agent 的失败（`initialize`、`session/new`、`session/load`、启动阶段 transport）才能重开。

## 四、数据与事实源

- `AcpLifecycleOwner { turn_id, operation_id, revision }`：准入与结算共用的 CAS 元组。
- `AcpFailedPromptAttempt { turn_id, operation_id }`（新增，非持久化）：provider 在 `run_prompt` 失败时把本次 attempt 的 owner 身份作为 anyhow context 附在错误上。orchestrator 看不到 provider 内部生成的 `operationId`，只能从错误链取回它，再原样交给下一次调用。
- `promptRetry.promptId`（既有持久化事实）：runtime 在 `record_user_prompt_event` 中生成，随 `running` snapshot 写盘，写盘发生在发送 `session/prompt` 之前，且跨终态保留。它尚未指向本 turn，即证明 Agent 没有收到本 turn 的 prompt。不新增持久字段。

## 五、修复方案

### 5.1 重开判定（`failed_attempt_is_reopenable`）

在 `session_metadata_lock` 内、基于 `admission_metadata_base`（兼容只有 `acp.session.json` 的旧 attempt）读取的同一份值，同时满足：

1. `retry_of.turn_id == header.turn_id` 且 `retry_of.operation_id == header.operation_id`；
2. `lifecycle_is_terminal` 且 `latest_turn_status == Failed` 且 `turn_error.recovery == Auto`；
3. `promptRetry.promptId != turn_id`（prompt 未派发）。

任一不满足即保持 `ExistingTerminal`，返回 `AlreadySettled`。

### 5.2 重开即普通准入

可重开时 `classify_existing_turn` 返回 `Admissible`，准入继续走既有 `PromptAdmitted` 转换：保留 `turnId`、写入新 `operationId`、`revision` 加一、清空 `promptEventId / stopReason / turnError`，再注册新的 active owner。不新增状态转换；PR 初稿中的 `PromptReopened` 与 `PromptAdmitted` 完全相同，已删除。锁内只读一次，不再二次 `read_json`。

### 5.3 接口

- `begin_session_turn(path, submission)`：签名不变，从不重开（Direct 对话等入口不涉及自动重试）。
- `admit_session_turn_for_execution(path, submission, retry_of: Option<&AcpFailedPromptAttempt>)`：provider 执行准入入口。
- `WorkerInvocation.automatic_retry_of: Option<AcpFailedPromptAttempt>`：只由 orchestrator 在自动重试时设置。
- 该身份不进入 `AcpPromptSubmission`：它是调用方意图，不是轮次内容。

### 5.4 orchestrator 意图生命周期

worker 路径（`drive_from_node_with_initial_session -> execute_ai_node`）与 AI-DYNAMIC 路径（`execute_dynamic_worker`）都在 Err 分支决定自动重试时执行 `automatic_retry_of = AcpFailedPromptAttempt::from_error(&err)`，在构建下一次 invocation 时 `take()`。因此身份只对紧随其后的那一次调用有效；Ok 分支的 auto 重试、repair、confirm 等新 turn 都不会继承。

### 5.5 旧执行者隔离

重开后 `operationId` 变化且 `revision` 前进，第 1 次尝试遗留的 owner 无法再通过 CAS，其迟到结算为 no-op；同一个失败身份被消费后再次出现也会因 owner 不匹配被拒绝。

## 六、拒绝矩阵

| 现有终态 | 重试意图 | 结果 |
| --- | --- | --- |
| `failed + auto`，prompt 未派发，身份匹配 | 有 | 重开，`Claimed`（新 operation id） |
| `failed + auto`，prompt 已派发 | 有 | `AlreadySettled` |
| `failed + auto`，身份不匹配或已被消费 | 有 | `AlreadySettled` |
| `failed + manual / blocked` | 有 | `AlreadySettled` |
| `failed`（任意） | 无 | `AlreadySettled` |
| `completed` / `cancelled` | 有或无 | `AlreadySettled`，取消保持既有语义 |
| `cancelRequested` | 有或无 | 不重开，按 `acp_turn_was_cancelled_before_execution` 处理 |
| 同一轮 `accepted / running` | 有或无 | 既有幂等语义（`ExistingActive`） |

不变量：

1. 一次输入内 `turnId` 不变，`operationId` 每次尝试唯一。
2. 已派发给 Agent 的 prompt 永不自动重放。
3. 判定与写入在同一把元数据锁内基于同一份值完成。
4. 结算与重开共用同一 owner 身份，不引入第二套身份。

## 七、变更清单

| 文件 | 变更 |
| --- | --- |
| `src/acp/events.rs` | 新增 `AcpFailedPromptAttempt`、`turn_prompt_was_dispatched`、`failed_attempt_is_reopenable`；`classify_existing_turn` 返回 `Admissible / Existing`；内部 `admit_session_turn` 承载重开；`admit_session_turn_for_execution` 接收 `retry_of` |
| `src/provider/mod.rs` | `WorkerInvocation.automatic_retry_of`；准入传入；`run_prompt` 错误附加失败 attempt 身份 |
| `src/app/node_executor.rs`、`src/app/orchestrator.rs` | 从错误取回失败身份并只交给下一次调用 |
| `src/acp/client.rs`、`src/acp/connection.rs` | 第十二节超时定位 |
| `src/config/mod.rs` | 删除死字段 `log_provider_command` |

## 八、回归测试

先在未修复语义上确认失败，再转绿：

- `acp::events::tests::automatic_retry_reopens_the_named_startup_failure`：修复前（任何终态都不重开）失败。
- `acp::events::tests::automatic_retry_does_not_reopen_after_the_prompt_was_dispatched`：bool 标志语义下失败。
- `acp::events::tests::automatic_retry_naming_a_stale_attempt_does_not_reopen`：bool 标志语义下失败。
- `acp::events::tests::automatic_retry_reopens_from_a_legacy_session_only_lifecycle`、`automatic_retry_rejects_non_recoverable_or_unnamed_terminal_turns`、`late_settlement_from_replaced_operation_is_a_no_op`：对照。
- `acp::client::tests::automatic_retry_reopens_only_attempts_whose_prompt_was_never_dispatched`：真实 fixture adapter 分别在 `session/new` 与 `session/prompt` 失败，证明 runtime 在派发前持久化 `promptRetry`。
- `provider::tests::provider_passes_the_named_failed_attempt_to_admission`：provider 错误携带失败身份，点名重试真正重新执行，未点名或过期身份被拒。
- `tests/worker_bootstrap.rs::automatic_retry_names_only_the_failed_attempt_it_replaces`、`tests/ai_dynamic_node.rs::ai_dynamic_repair_turn_does_not_inherit_the_startup_retry_intent`：orchestrator 只把身份交给紧随其后的一次调用。
- `acp::connection::tests::stderr_diagnostic_tail_keeps_the_latest_lines_within_its_bound`：stderr 缓冲保留尾部。

## 九、影响面与风险

- 影响：仅 ACP provider 执行准入路径。Direct 对话准入（`begin_session_turn`）不变。
- 已知遗留：prompt 已派发后的 auto 失败（如 `session/prompt` 超时）仍会被 orchestrator 自动重试一次并以 `acp.prompt-execution-already-settled` 结束，原始错误保留在 lifecycle `turnError` 与 `runtime_auto_retry` 事件中。是否在结算时把派发后失败降为 manual，需单独评审 main 既有的 auto 分类（如 `prompt_terminal_route_convergence_errors_are_auto_recoverable`）。
- `initial-direct-turn` 是全局常量，多轮复用同一 id；本方案不改变轮次身份生成契约。

## 十、方案自评审

过度设计评审：只新增一个非持久化身份结构和一个可选参数；「未派发」复用既有持久化事实 `promptRetry`，重开复用既有 `PromptAdmitted` 转换，未新增状态机、持久字段或缓存。

性能影响评审：重开判定在既有锁内对已读入的 JSON 做 O(1) 字段比较；错误路径多一层 context。stderr 缓冲由每行全量重算改为按行 O(1) 摊销的有界队列。无全量扫描、无长持锁。

## 十一、实施记录

1. 合并 main 后，按本方案在 `pr136-integration` 分支实现并补齐回归测试。
2. 需人工复核：真实 Agent 启动失败（如 `initialize` 超时）后自动重试能重新执行；prompt 已派发后失败不被重放。

## 十二、初始化超时定位与投影补充（P1）

1. **初始化进度信号**：ACP 初始化开始后、`sessionId` 尚未到达前，composer 与运行页统一展示 `acp.preparingAgent`（「正在准备 Agent」）；删除同一状态的旧 key `acp.launchingClaude`。复用既有 `initializing` 投影，不新增状态机。
2. **超时错误带定位上下文**：请求超时返回结构化 `runtime.transport-interrupted`（`recovery=auto`，与文本归一化结果一致）。`params` 只含 `method / timeoutSeconds / adapterId / adapterCommand`（命令 basename），adapter `args`、`PATH`、`node` 版本不进入错误（可能含 token 或私有路径；同步执行 `node --version` 也会阻塞且结果不代表 adapter 实际运行时）。adapter stderr 尾部放入 `raw.stderrTail`。
3. **adapter stderr**：非 noise 的 `Diagnostic` 行进入连接级有界尾部缓冲（按字符上限保留最新行），只在该连接的超时错误中引用。连接按 adapter 与 workspace 复用，stderr 无法归属到单个 session，因此不写入 per-attempt 诊断。
4. **死字段**：删除从未被消费的 `log_provider_command`（`RuntimeConfig`、`SettingsConfig` 与设计文档）。
