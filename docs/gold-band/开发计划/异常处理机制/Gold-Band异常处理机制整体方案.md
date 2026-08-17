# Gold Band 异常处理机制整体方案

## 1. 背景

当前 runtime 已经把异常大体分为：

- `runtime-abnormal`：可恢复运行异常，保留当前 run / round / node / attempt，允许 runtime continue。
- `error-blocked`：错误阻塞，不提供普通 runtime continue 输入入口。

现有实现的问题是分类粒度过粗：`classify_runtime_error` 只识别少量 typed error、`std::io::ErrorKind` 和 transport 文本信号，其他异常默认进入 `error-blocked`。这会导致 provider 层临时异常，例如 `503 auth_unavailable`、余额不足、账号池暂不可用、rate limit、catalog 缺失、model 配置问题等，被过早视为不可继续的错误阻塞。

这不是单个错误码补丁问题，而是异常域、恢复策略和 runtime 生命周期之间缺少统一结构化模型。

## 2. 设计目标

1. 收窄 `error-blocked` 的含义，只用于真正不能通过等待、登录、充值、刷新诊断、切换模型、修配置后恢复的控制流或设计阻塞。
2. 保持现有外层 `PauseReason` 稳定，不新增 `recoverable-manual` pause reason，避免前后端状态机大改。
3. 在 `runtime-abnormal` 内部增加恢复策略：`auto` 与 `manual`。
4. ACP 继续作为协议边界屏蔽 adapter 差异；Gold Band 后端统一管理 ACP raw error 到 runtime error 的映射，不在 runtime 调度层写 Claude / Codex / 其他 adapter 特判。
5. 后端输出稳定错误码和参数，前端负责 i18n 文案；后端不返回对客文案。
6. 自动重试必须有次数、退避和幂等边界，不允许无限重试或重复消费已完成业务结果。

## 3. 非目标

- 不修改 ACP 协议或要求外部 adapter 按 Gold Band 私有错误码返回。
- 不把所有异常都改成自动重试。
- 不把 workflow / DSL / AI 输出结构错误伪装成可恢复 provider 异常。
- 不在前端组件里按错误字符串追加局部判断。

## 4. 核心模型

### 4.1 外层生命周期保持不变

继续使用现有 `PauseReason`：

| pause reason | 含义 | 是否允许 runtime continue |
| --- | --- | --- |
| `process-interrupted` | 用户主动停止或进程中断收敛 | 是 |
| `runtime-abnormal` | 可恢复异常暂停 | 是 |
| `error-blocked` | 控制流、DSL、业务结构等阻塞 | 否 |
| `waiting-for-user-input` | 人工 check 等业务等待 | 按既有人工 check 语义 |
| `permission-requested` | 权限审批等待 | 按权限审批语义 |

### 4.2 新增内部恢复策略

新增 runtime 内部恢复策略：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryMode {
    Auto,
    Manual,
    Blocked,
}
```

含义：

| recovery | 对应 pause reason | 含义 |
| --- | --- | --- |
| `auto` | 自动重试期间不暂停；重试耗尽后转 `runtime-abnormal` | runtime 可自行处理的短暂异常 |
| `manual` | `runtime-abnormal` | 用户处理外部条件后可 continue / retry |
| `blocked` | `error-blocked` | 不修 workflow / DSL / 输出结构 / runtime invariant，继续也没有意义 |

### 4.3 统一错误结构

runtime 调度层只消费统一结构，不直接消费 ACP JSON-RPC raw error 文本：

```rust
pub struct RuntimeErrorInfo {
    pub code: RuntimeErrorCode,
    pub domain: RuntimeErrorDomain,
    pub recovery: RecoveryMode,
    pub retry_policy: Option<RetryPolicy>,
    pub params: serde_json::Value,
    pub diagnostic: String,
    pub raw: Option<serde_json::Value>,
}

pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: Vec<u64>,
    pub jitter: bool,
}
```

`diagnostic` 与 `raw` 只用于日志、开发排查和错误详情，不作为前端对客文案。前端只根据 `code + params + recovery` 做文案映射和交互状态。

## 5. 错误域

建议先定义以下错误域：

| domain | 来源 |
| --- | --- |
| `runtime.io` | 本地文件、目录、JSONL、临时资源、系统资源 |
| `runtime.transport` | ACP transport、adapter stdout、JSON-RPC route、连接关闭 |
| `provider` | provider API、账号、配额、rate limit、模型、网关 |
| `config` | provider / profile / model / permission / catalog 配置与诊断 |
| `workspace` | repo、git、worktree、权限、路径 |
| `workflow` | DSL、edge、workflow snapshot、控制流前提 |
| `dynamic` | AI-DYNAMIC proposal、fanout、merge、acceptance、内部图约束 |
| `internal` | runtime invariant、未知内部错误 |

## 6. Gold Band 后端统一 ACP Error Normalization

ACP 返回的错误统一先进入 Gold Band 后端 normalization 层：

```text
ACP JSON-RPC raw error
  -> normalize_acp_error(...)
  -> RuntimeErrorInfo
  -> runtime recovery / pauseReason / UI
```

Normalization 层可以读取：

- JSON-RPC `code`
- `message`
- `data`
- `data.details`
- `data.errorKind`
- `data.codex_error_info`
- 其他未知字段

但这些字段只作为输入信号。runtime 调度层不出现 Claude / Codex / adapter 名称分支。

### 6.1 ACP 标准码粗分

| ACP 错误 | Gold Band 分类 |
| --- | --- |
| authentication required | `provider.auth_required`, `manual` |
| invalid params | `config.invalid` 或 `provider.config_invalid`, `manual` |
| resource not found | `config.resource_not_found`, `manual` |
| internal error | 继续分析 `data/message/details/raw` |

### 6.2 语义特征统一识别

不按 adapter 分支，只按语义特征统一映射：

| 信号 | Gold Band code | recovery |
| --- | --- | --- |
| `auth_unavailable`, `no auth available`, auth cooldown | `provider.auth_unavailable` | `manual` |
| `auth required`, `not logged in`, missing key | `provider.auth_required` | `manual` |
| `insufficient_quota`, `quota exceeded`, `balance`, `credits` | `provider.quota_insufficient` | `manual` |
| `rate_limit`, `429`, `cooldown`, retry-after | `provider.rate_limited` | `manual` |
| `502`, `503`, `504`, `overloaded`, `server_error`, gateway unavailable | `provider.server_unavailable` | 尚未形成 provider terminal verdict 时为 `auto`；明确 terminal failure 时为 `manual` |
| `invalid model`, `unsupported model`, model not found | `provider.model_invalid` | `manual` |
| config option invalid / unsupported | `provider.config_invalid` | `manual` |
| connection reset / broken pipe / timeout / channel closed | `runtime.transport_interrupted` | `auto` |
| Windows `os error 1450`, temporary local resource failure | `runtime.resource_unavailable` | `auto`，耗尽后 `manual` |

字符串匹配只允许集中在 normalization 层作为兜底，并必须有单元测试覆盖。业务代码、前端组件和调度循环不得散落 `message.contains(...)`。

## 7. 分类规则

### 7.1 可自动恢复 `auto`

自动恢复适用于 runtime 可以自己重试，且不需要用户先修配置的短暂异常：

- `std::io::ErrorKind::Interrupted`
- `WouldBlock`
- `TimedOut`
- `BrokenPipe`
- `ConnectionAborted`
- `ConnectionReset`
- ACP transport closed / channel closed / route disconnected
- adapter stdout 断开但可重新拉起 connection
- 尚未形成 provider terminal verdict 的 502 / 503 / 504 / overloaded / server unavailable
- git lock、临时目录占用、短暂文件系统资源不足

自动重试耗尽后转为：

- `pauseReason = runtime-abnormal`
- `recovery = manual`
- 保留最后一次 `RuntimeErrorInfo`

### 7.2 人工可恢复 `manual`

人工可恢复适用于用户处理外部条件后可以 retry / continue 的异常：

- 余额不足、quota insufficient、credits exhausted
- rate limit / cooldown
- auth required / auth unavailable / no auth available
- provider 缺失、agent 未配置
- model invalid / model 不存在 / unsupported model
- catalog missing / doctor 诊断缺失或过期
- permission mode 不支持
- 非 git workspace、无 HEAD、worktree 能力缺失
- MCP server 配置缺失或认证缺失

这些错误不应该进入 `error-blocked`。它们应进入：

- `pauseReason = runtime-abnormal`
- `recovery = manual`
- UI 允许用户处理后继续或 retry

### 7.3 阻塞 `blocked`

`blocked` 必须克制，只用于继续当前 runtime 必然无意义的情况：

- workflow / DSL 无效
- edge 缺失导致控制流无路可走
- workflow snapshot 与运行时状态不一致
- 输出修复所需的 session / continue identity 缺失，无法安全恢复当前 attempt
- dynamic 控制约束无法满足，例如 `maxFanout`、`maxDepth`、`maxDynamicNodes`、`maxWorkflowInvocations` 在 repair 后仍超限
- runtime invariant 破坏，无法确定安全恢复点

这些错误进入：

- `pauseReason = error-blocked`
- `recovery = blocked`
- UI 锁定普通输入，只展示错误详情和按错误类型派生的处理入口；处理入口不等于 runtime continue。只有后端验证存在安全恢复点并生成明确恢复计划时，才允许恢复，否则只能重新运行、从节点重新开始或进入诊断流程。

### 7.4 业务结果与异常边界

`success / failure` edge 只消费节点业务结果，不消费 runtime/provider 异常。

节点业务结果来自 `NodeOutcome`：

- `success`：节点产出 artifact，结构合法，且 success condition 判定通过；没有 success condition 时，合法产出默认成功。
- `failure`：节点产出 artifact，结构合法，但 success condition 明确判定不通过。
- `invalid`：节点声明了 artifact 但缺失、JSON 非法、schema 不合法，或 success condition 所需字段缺失。
- `killed`：显式终止后的终局结果。

异常不代表节点业务失败，不能直接走 `failure` edge：

- auth / quota / rate limit / provider unavailable / model invalid / catalog missing
- ACP transport closed / channel closed / adapter crash
- 本地 IO、临时资源、workspace 能力异常
- runtime 调度、状态持久化或诊断异常

这些异常必须先归一化为 `RuntimeErrorInfo`，再映射到 `runtime-abnormal` 或 `error-blocked`。即使 provider/adapter 返回了失败状态，Gold Band 后端也必须先判断它是“业务失败”还是“运行异常”；只有明确属于业务结果失败时，才允许写入 `NodeOutcome::Failure` 并进入 `failure` edge。

因此，`edge` 缺失导致的 blocked 只适用于已经有确定业务结果但 workflow 没有对应控制流的情况。例如节点 artifact 判定为 `failure`，但 workflow 只声明了 `success` edge。provider/auth/transport 等异常缺少 edge 不构成 workflow edge 缺失，也不能用 `failure` edge 承接。

## 8. Runtime 状态映射

| RuntimeErrorInfo.recovery | runtime 行为 | pause reason |
| --- | --- | --- |
| `auto` | 按 retry policy 自动重试；不立即暂停 | 无 |
| `auto` 耗尽 | 暂停，等待用户处理 | `runtime-abnormal` |
| `manual` | 暂停，保留 attempt 和 continue_ref | `runtime-abnormal` |
| `blocked` | 暂停，锁普通 continue 输入 | `error-blocked` |

普通 worker 与 AI-DYNAMIC 内部 leaf 必须使用同一分类器。同一种 ACP/provider 错误在普通 worker 和 AI-DYNAMIC bootstrap / worker / merge / acceptance 中不能表现出不同 pause reason。

## 9. 自动重试策略

默认策略：

```rust
RetryPolicy {
    max_attempts: 3,
    backoff_ms: vec![1000, 3000, 10000],
    jitter: true,
}
```

约束：

1. 仅对尚未形成 provider terminal verdict 的 `recovery=auto` 生效，例如 ACP transport interruption 或临时本地资源异常。`session/prompt` JSON-RPC error、`willRetry=false` 和 `threadStatus=systemError` 已明确终结当前 prompt，必须归一化为 `manual` 并清除 retry policy，避免自动重放可能已经产生部分副作用的业务工作。
2. retry attempt 必须写入 run progress / events，便于用户知道系统正在重试。
3. provider 已经产出完整合法业务 artifact 时，不重复发送 prompt，优先执行现有“接受已完成结果”逻辑。
4. 用户停止、权限请求、elicitation、manual check 等用户交互状态不自动重试。
5. 自动重试耗尽后降级为 `runtime-abnormal + manual`，不进入 `error-blocked`。
6. AI-DYNAMIC 并行 leaf 自动重试不能阻塞其他 active leaf；只有所有 leaf 都暂停且无 active leaf 时，父 graph/run 才收敛为暂停。
7. artifact/schema 非法的隐藏 repair 属于输出协议状态机，不消费 provider retry budget；同一 drive 最多三次 repair，耗尽后进入可显式继续的 `runtime-abnormal + manual`。

## 10. UI 行为

| 状态 | UI 行为 |
| --- | --- |
| 自动重试中 | composer 锁定，显示重试中状态和第 N 次重试 |
| `runtime-abnormal + manual` | 恢复普通输入框；用户可继续 NonRuntime 对话，显式“继续工作流”动作才恢复 Runtime；展示异常提示 |
| `error-blocked + blocked` | composer 进入 runtime-error；普通输入锁定；展示修复入口 |
| `process-interrupted` | 恢复普通输入框；用户可继续 NonRuntime 对话，显式“继续工作流”动作才恢复 Runtime |

UI 只消费后端派生结果，不在组件中按 ACP session `failed/cancelled` 或错误文本重新推断。若 ACP `failed/error` 先于 runtime pause reason 落盘到达，且当前 attempt 仍是 `paused + outcome=null`，Conversation VM 必须把它视为 runtime 正在收敛的中间态，保持 `runtime-active` 锁定，不短暂展示 `runtime-error`；待 runtime 写出 `runtime-abnormal` 后再恢复 runtime continue 输入。ACP diagnostics `lastError` 可以展示为顶部 banner 来解释 provider/ACP 失败原因，但不能绕过 lifecycle 驱动 composer 错误态；用户修复外部条件并继续成功后，banner 按后续正常响应自然消失。

横幅诊断精度以结构化控制面证据为准：JSON-RPC error 携带 provider error code/message 时可以展示精确原因；只有 `threadStatus=systemError` 时展示 provider-neutral 通用原因。随后到达的无 ID Agent 文本仍进入 canonical timeline，但不得通过匹配 `429`、quota 等文字反向改写错误分类或 lifecycle；若 adapter 需要统一精确原因，应在 ACP 边界补充结构化 error payload。

建议 VM 增加：

```ts
runtimeDisplay: {
  code: string
  tone: "neutral" | "running" | "warning" | "danger" | "success"
  blockingError: boolean
  resumable: boolean
  recovery?: "auto" | "manual" | "blocked"
  errorCode?: string
  errorParams?: Record<string, unknown>
}
```

## 11. 落盘与可观测性

每次异常分类都应可追踪：

- `run-progress.json` 写入标准化错误摘要。
- `events.jsonl` 写入 `runtime_error_classified`。
- ACP raw error 保留在 `acp.raw.jsonl`。
- 标准化结果写入 attempt 级诊断，建议文件名为 `runtime-error.json` 或并入现有诊断结构。
- 自动重试写入 `runtime_auto_retry_scheduled`、`runtime_auto_retry_started`、`runtime_auto_retry_exhausted`。

建议标准化错误落盘结构：

```json
{
  "version": "0.1",
  "code": "provider.auth_unavailable",
  "domain": "provider",
  "recovery": "manual",
  "params": {
    "provider": "claude-acp",
    "model": "deepseek-v4-pro"
  },
  "diagnostic": "ACP session/prompt failed with 503 auth_unavailable",
  "rawRef": "acp.raw.jsonl#9"
}
```

## 12. 实施步骤

### 阶段一：结构化分类闭环

1. 新增 `RuntimeErrorInfo`、`RuntimeErrorDomain`、`RecoveryMode`、`RetryPolicy`。
2. 新增集中 normalization 模块，例如 `src/app/runtime_error.rs` 或 `src/runtime/error.rs`。
3. 将现有 `classify_runtime_error` 改为返回 `RuntimeErrorInfo`。
4. ACP JSON-RPC error 在 provider bridge 边界转换为 `RuntimeErrorInfo`。
5. 把 provider/auth/quota/rate-limit/model/catalog/workspace 能力类错误从 `error-blocked` 调整为 `runtime-abnormal + manual`。
6. 保持 `error-blocked` 只处理 workflow / DSL / dynamic proposal repair exhausted / runtime invariant。

### 阶段二：自动重试

1. 对 `recovery=auto` 增加 retry policy。
2. 普通 worker provider invocation 增加有界退避重试。
3. AI-DYNAMIC leaf provider invocation 使用同一 retry helper。
4. 自动重试中写 run progress 和 lifecycle event。
5. 重试耗尽后降级为 `runtime-abnormal + manual`。

### 阶段三：前端展示

1. Conversation VM / Run VM 增加 `recovery` 与 `errorCode`。
2. composer state 矩阵增加：
   - auto retrying
   - runtime-abnormal manual
   - error-blocked blocked
3. i18n 根据 `errorCode + params` 映射文案。
4. `error-blocked` 不再承载 provider/model/catalog/workspace 可人工恢复错误。

### 阶段四：文档同步

同步维护：

- `docs/gold-band/产品设计文档/runtime/control.md`
- `docs/gold-band/产品设计文档/runtime/state/run.json.md`
- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/开发计划/AI动态路由/AI-DYNAMIC节点方案.md`
- `docs/gold-band/开发计划/生命周期整理/并行节点通用停止继续语义与AI-DYNAMIC落地方案.md`

## 13. 验收测试

### 13.1 Rust runtime 测试

必须补充接口层单元测试：

| 场景 | 期望 |
| --- | --- |
| ACP `503 auth_unavailable` | `runtime-abnormal + manual` |
| ACP `server_error` 且 message 包含 503 | 先 `auto`，耗尽后 `runtime-abnormal + manual` |
| `insufficient_quota` / balance insufficient | `runtime-abnormal + manual` |
| `429 rate_limit` / cooldown | `runtime-abnormal + manual` |
| auth required / missing API key | `runtime-abnormal + manual` |
| model invalid / unsupported model | `runtime-abnormal + manual` |
| catalog missing / doctor cache missing | `runtime-abnormal + manual` |
| connection reset / channel closed | `auto`，耗尽后 `runtime-abnormal + manual` |
| artifact success condition 为 false 且存在 failure edge | 写入 `NodeOutcome::Failure`，按 failure edge 继续 |
| artifact success condition 为 false 但缺少 failure edge | `error-blocked + blocked` |
| provider/auth/transport 异常返回失败状态 | 不写入 `NodeOutcome::Failure`；归一化为 `RuntimeErrorInfo` |
| workflow edge missing | 仅业务结果已有确定 outcome 时进入 `error-blocked + blocked` |
| AI-DYNAMIC proposal repair exhausted | `error-blocked + blocked` |
| non-git workspace requested worktree | `runtime-abnormal + manual` |

### 13.2 AI-DYNAMIC 回归

- bootstrap provider 抛 `auth_unavailable` 时，外层 run 与 dynamic graph 都应落 `runtime-abnormal`，不落 `error-blocked`。
- 并行 leaf 中一个 leaf `runtime-abnormal + manual`，另一个 leaf 仍 running 时，父 graph 继续 running。
- 所有 leaf 都进入 manual 可恢复暂停时，父 graph/run 收敛为 `runtime-abnormal`。
- 已产出合法 `dynamic-node-completion` 后遇到 transport 异常，优先接受结果，不重复 prompt。

### 13.3 前端状态测试

- `runtime-abnormal + manual` composer 恢复普通输入，submit target 为 `acp-prompt`；普通消息保持 NonRuntime，对应的显式 continue action 才恢复 Runtime。
- `error-blocked + blocked` composer 锁定，submit target 为 `none`。
- 自动重试中 composer 锁定并展示 retrying 状态。
- ACP snapshot/session 为 `failed` 或 `cancelled` 时，不覆盖后端 runtime lifecycle。

## 14. 兼容性

旧 run 没有 `RuntimeErrorInfo` 时：

- 继续按现有 `pauseReason` 派生 UI。
- `runtime-abnormal` 默认视为 `manual`。
- `error-blocked` 默认视为 `blocked`。

不需要迁移旧运行目录。

## 15. 当前结论

Gold Band 异常处理的目标状态是：

- ACP adapter 差异停留在 ACP raw error 与诊断层。
- Gold Band 后端统一把 raw error 归一为稳定 runtime error。
- runtime 状态机只消费 `RuntimeErrorInfo`。
- `runtime-abnormal` 承载所有可恢复异常，并通过 `recovery=auto/manual` 区分自动重试和人工恢复。
- `error-blocked` 只用于真正不可通过重试/继续恢复的控制流或设计阻塞。

## 16. 2026-07-01 实施记录

本轮已落地：

1. 新增统一错误模型与归一化入口：
   - `RecoveryMode = auto | manual | blocked`
   - `RuntimeErrorInfo`
   - `RetryPolicy`
   - typed `RuntimeError`
2. provider result 增加 `runtime_error`，用于表达 provider/ACP 运行异常。
3. 普通 worker 和 AI-DYNAMIC leaf 在写业务 outcome 前先检查 `runtime_error`；如果存在，则抛给 orchestrator 归一化暂停，不写 `NodeOutcome::Failure`。
4. `failure edge` 与异常边界已固化：provider/auth/transport 等异常不能直接进入 `failure` edge。
5. provider/model/catalog/workspace 能力问题调整为 `runtime-abnormal + manual` 方向；AI-DYNAMIC catalog missing 和 worktree capability missing 不再落 `error-blocked`。
6. 顶层 worker execution 增加 bounded auto retry：仅 `recovery=auto` 生效，按 `RetryPolicy` 写 `runtime_auto_retry` 事件和 run progress；重试耗尽后按 `runtime-abnormal` 暂停。
7. run event 的 `control_failure` 中记录结构化 `runtimeError`，供 UI/诊断读取。
8. Conversation VM 将 `runtime-abnormal` 纳入 runtime continue 输入态，并让后端 runtime pause reason 优先于 ACP snapshot/session 的 `failed` / `cancelled` 历史终态。
9. 前端 composer 增加 lifecycle 闸门：stale diagnostics / runtimeErrorMessage 不能在 runtime active、收敛中或 `runtime-abnormal` 可继续态下制造 `runtime-error` 红框；顶部 ACP failure banner 仅作为原因提示保留。

本轮未改变：

- `run.json` 暂不新增持久字段；旧 run 仍只依赖 `pauseReason`，结构化错误通过 run progress / run events 观察。
- `error-blocked` 后的通用 SafeResumePlan 尚未实现；blocked 仍不走普通 `runtime continue`。
- AI-DYNAMIC 并行 leaf 的自动重试仍以统一分类和暂停收敛为主，未引入复杂的并行 leaf 内部重试调度。

验证：

- `cargo test -j 1 runtime_error --lib` 通过，覆盖 auth_unavailable、503 server、quota、rate limit、model/catalog/workspace、transport/io、unknown blocked，以及 provider runtime error 不写业务 failure。
- `cargo test -j 1 -p gold-band-desktop runtime_abnormal_pause_is_input_continue_even_when_acp_failed` 通过，覆盖 ACP `failed` 不覆盖 `runtime-abnormal` 输入继续态。
- `cargo test -j 1 -p gold-band-desktop paused_parent_runtime_abnormal_dynamic_leaf_is_runtime_continue` 通过，覆盖 AI-DYNAMIC leaf 在父 run `runtime-abnormal` 时仍恢复 runtime continue。
- `npm run web:test -- acp-runtime-composer-state` 通过，覆盖 stale runtime error message / ACP diagnostics 在 runtime 管理的可恢复状态下不展示 composer 错误红框。
- `npm run web:build` 通过。
- `cargo check -j 1` 通过。
- `cargo test -j 1 --test ai_dynamic_node` 中 20/22 通过；2 个失败为 prompt 文本断言固定匹配 `# Requirement\n...`，而当前 Windows 工作树中的 runtime prompt 模板为 CRLF，实际为 `# Requirement\r\n...`，不属于本次异常处理行为失败。
- 全量 `cargo test` 曾触发 Windows 页面文件不足 / rustc OOM（`os error 1455`、metadata mmap failure），未跑完；这不是测试断言失败。

## 17. ACP prompt 终态、messageId 与 artifact repair 统一规则（现行）

本节是 ACP RuntimeControlled 输出异常机制的集中决策真源。前文定义错误域、恢复策略和外层生命周期；本节统一回答一次 prompt 怎样算失败、什么时候允许 artifact 校验或 repair、`messageId` 不同组合对应什么结果。产品设计文档可以解释各自领域，但不得建立与本节冲突的第二套决策表。

### 17.1 两套独立状态机

必须区分两套状态机，不能互相借用次数或把一种失败伪装成另一种：

1. **Provider 恢复状态机**：处理 transport、JSON-RPC、adapter 和 provider terminal failure。只有尚未形成 provider terminal verdict 的 `RecoveryMode::Auto` 异常可以按 `RetryPolicy` 自动重试；明确 terminal failure 一律 `Manual`。
2. **Artifact 输出状态机**：业务 turn 成功后，按 `business-turn → finalizing → repair` 推进 JSON 提取与 schema 校验。同一 drive 最多自动发送三次隐藏 repair；repair 不重做业务工作，也不消费 provider retry budget。

RuntimeControlled PostTurnProjection 包含三个 phase：

| phase | 职责 | 是否允许执行业务工作 |
| --- | --- | --- |
| `business-turn` | 完成用户任务并自然回复 | 是 |
| `finalizing` | 同一 session 的隐藏 turn，只生成 canonical artifact | 否 |
| `repair` | 同一 session 的隐藏 turn，只修复上一份非法 artifact | 否 |

只有 `business-turn` 已经 terminal success，Runtime 才能原子写入 `artifact-emission.json(finalizing)` 并发送隐藏 finalize。业务 turn 失败时不能创建该 checkpoint；finalize / repair 失败时保留已有 checkpoint，供用户显式继续。

### 17.2 Prompt 终态判定优先级

每个 prompt 开始时创建并重置 transient `AcpPromptTerminalState`，统一观察当前 turn 的结构化 terminal evidence。结算顺序固定如下，前项覆盖后项：

1. **用户停止**：当前 attempt 收敛为 `Paused + ProcessInterrupted`。晚到 terminal failure、transport error、文本或 artifact 不得覆盖停止事实，也不得重新触发自动重试。
2. **明确的结构化 terminal failure**：`session/prompt` JSON-RPC error、`_meta.codex.error.willRetry=false` 或 `_meta.codex.threadStatus.type=systemError` 均表示当前 prompt 已失败。它们优先于 `stopReason=end_turn`、Agent 文本、messageId 和 artifact 候选，固定映射为 `RecoveryMode::Manual`、清除 `retryPolicy`，并收敛为 `Paused + RuntimeAbnormal`；不得 finalize、repair 或自动重放业务 prompt。
3. **尚无 terminal verdict 的 transport / 临时资源异常**：按统一 normalization 得到 `RecoveryMode::Auto` 后，使用共享 `RetryPolicy` 最多自动重试三次；耗尽后转为 `Paused + RuntimeAbnormal + Manual`。
4. **ACP stop reason**：无上述失败时，明确 `end_turn` 才进入成功后的输出处理；`cancelled / interrupted / max_tokens / max_turn_requests` 作为未完整结束处理；未知或缺失 stop reason 是协议失败，必须先进入 RuntimeError normalization，不得进入 artifact repair。
5. **Artifact 提取与 schema 校验**：只有前四步确认当前 RuntimeControlled turn 可以消费输出后才执行。Provider/runtime 错误永远先于 JSON、schema 和 success condition。

`willRetry=true` 只表示 adapter 自己仍在恢复，不单独构成 terminal failure；如果随后正常 `end_turn` 且没有更晚的失败证据，按正常成功处理。如果随后出现 `willRetry=false` 或 `systemError`，则提升为 terminal failure。`session/prompt` response 到达后仍必须完成 route watermark 与有界 quiet drain，确保紧随其后的 terminal update 能参与同一次结算。

### 17.3 messageId 与 JSON 候选矩阵

`messageId` 只证明 `agent_message_chunk` 是否具有 provider 提供的稳定消息身份，不证明文本一定是业务结果，也不直接证明它是错误。所有 Agent 文本，无论有无 ID，都写入 canonical timeline 并正常展示；以下限制只作用于声明 output contract 的 RuntimeControlled finalize / repair turn，Direct 和普通 NonRuntime 对话不应用 artifact 身份规则。

Runtime 为当前 turn 维护最近最多三条 Agent message。JSON artifact 的候选规则固定为：

| 当前 turn 的 Agent message 形态 | JSON 候选 | 结果 |
| --- | --- | --- |
| 最终 message 有稳定 ID | 从最终 message 开始向前检查最近最多三条 message，取第一段可解析 JSON；窗口内更早的 message 可以无 ID | 找到后进入 schema 校验；都找不到则进入 invalid-output repair |
| 整个 turn 从未出现稳定 ID | 只取最终一条无 ID message，不回扫更早 message | 合法则进入 schema 校验；非法则进入 invalid-output repair |
| turn 内出现过稳定 ID，但最终 message 无 ID | 不提取 JSON、不回扫、不尝试 repair | 返回 `provider.acp-terminal-message-unidentified + Manual`，收敛为 `Paused + RuntimeAbnormal` |
| 没有任何 Agent message | 无候选 | 作为 invalid output 进入既有 repair；缺少安全 continue identity 时转 `ErrorBlocked` |
| 非 JSON artifact contract | 只消费最终 message 的非空文本 | 再按对应 artifact validator 处理，不应用 JSON 回扫 |
| 无 output contract 的 Direct / 普通对话 | 不选 artifact 候选 | 展示全部文本，按普通会话终态处理 |

“出现过稳定 ID、最终无 ID”必须直接暂停，因为该结尾既可能是 provider 把错误文本化，也可能是未完成输出。此时回扫旧 JSON 可能选中非 artifact JSON；继续自动 repair 又可能在业务工作未完成时伪造一份合法 artifact。因此这一分支既不能回扫，也不能 repair。

### 17.4 Schema、success condition 与 repair

候选提取后按固定顺序处理：

1. JSON 解析。
2. artifact schema 校验。
3. success condition 判定。
4. 根据确定的业务结果进入 `success` 或 `failure` edge。

结果矩阵：

| 结果 | Runtime 行为 |
| --- | --- |
| JSON 与 schema 合法，success condition 通过或不存在 | 写 `NodeOutcome::Success`，进入 success edge |
| JSON 与 schema 合法，success condition 明确不通过 | 写 `NodeOutcome::Failure`，进入 failure edge；缺少对应 edge 才是 `ErrorBlocked` |
| JSON 缺失、不可解析或 schema 非法，repair 尚未耗尽 | 在同一 session 发送隐藏 `RuntimeRepair`，只修复控制产物 |
| 同一 drive 已自动 repair 三次仍非法 | 不写业务 failure，不进入 `ErrorBlocked`；收敛为 `Paused + RuntimeAbnormal + Manual`，保留 attempt、continue ref 和 finalizing checkpoint |
| repair 所需 session / continue identity 缺失，无法确定安全恢复点 | `Paused + ErrorBlocked` |
| finalize / repair turn 收到结构化 terminal failure | 立即 `Paused + RuntimeAbnormal + Manual`，当前 drive 不再发下一次 repair，保留 finalizing checkpoint |

repair 耗尽后的 composer 恢复普通输入。用户可以继续 NonRuntime 对话；只有显式“继续工作流”或“继续并发送”动作才重新进入 Runtime 控制。它不是 `ErrorBlocked`，也不是自动开始第四次 repair。

### 17.5 全量决策矩阵

| 输入事实 | 是否校验 artifact | 是否自动动作 | 最终结果 |
| --- | --- | --- | --- |
| 用户停止，之后又到达任意失败或文本 | 否 | 否 | `Paused + ProcessInterrupted` |
| prompt JSON-RPC error | 否 | 否 | `Paused + RuntimeAbnormal + Manual` |
| `willRetry=false` 或 `threadStatus=systemError`，即使同时有 `end_turn` | 否 | 否 | `Paused + RuntimeAbnormal + Manual` |
| 仅 `willRetry=true`，随后正常 `end_turn` | 按 contract | finalize / repair 规则 | 正常输出路径 |
| 无 terminal verdict 的 transport interruption | 否 | Provider auto retry，最多三次 | 成功后继续；耗尽为 `Paused + RuntimeAbnormal` |
| 业务 turn 正常结束，PostTurnProjection contract 存在 | 尚不校验业务文本 | 写 finalizing checkpoint，发送一次隐藏 finalize | 进入 `finalizing` |
| finalize / repair 最终 message 有稳定 ID | 最近三条倒序提取 | 非法时最多三次 repair | 合法则按 schema / success condition；耗尽为 `RuntimeAbnormal` |
| finalize / repair 全 turn 都无稳定 ID | 只校验最终 message | 非法时最多三次 repair | 合法则继续；耗尽为 `RuntimeAbnormal` |
| finalize / repair 先有稳定 ID、最终无 ID | 否 | 否 | `provider.acp-terminal-message-unidentified`，`Paused + RuntimeAbnormal` |
| repair 中出现 terminal failure | 否 | 不再 repair | `Paused + RuntimeAbnormal + Manual` |
| 合法 artifact 的 success condition 不通过 | 是 | 不 repair | 业务 `Failure`，按 failure edge |
| 无安全恢复 identity 或 runtime invariant 已破坏 | 否 | 否 | `Paused + ErrorBlocked` |

### 17.6 诊断与横幅

生命周期只依赖结构化控制面证据。JSON-RPC error 携带 provider code/message 时，横幅可以展示精确原因；只有 `systemError` 时展示 provider-neutral 通用原因。后续无 ID Agent 文本仍显示在消息流，但不得通过匹配 `429`、quota、error 等词反向改写 RuntimeErrorInfo 或 pause reason。若 adapter 希望提供一致的精确诊断，必须在 ACP 边界提供结构化 error payload。

### 17.7 回归验收

接口测试必须至少固定：

- `systemError + end_turn` 仍失败，且没有 finalize、repair 或 `runtime_auto_retry`。
- prompt JSON-RPC error 为 `Manual` 且 `retryPolicy=None`。
- quiet drain 中晚到的 `systemError` 能覆盖先到的成功 response。
- 用户停止覆盖晚到 terminal failure，并使 retry wait 退出。
- mixed terminal 只调用一次 Provider，不产生 `invalid_output_repair_requested`。
- 最终稳定 message 只能回扫最近三条；可命中窗口内更早的无 ID JSON，但不能命中第四条以外。
- 全 turn 无 ID 的合法 JSON 可成功，非法输出可进入最多三次 repair。
- 三次 repair 耗尽后是可输入、可显式继续的 `RuntimeAbnormal`，不是 `ErrorBlocked`。
