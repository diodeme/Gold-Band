# ACP 停止语义与 Adapter 长连接开发方案

## 1. 背景与目标

当前 Gold Band 的 ACP runtime 停止链路经历过多轮修正，但仍存在一个根本问题：业务停止、ACP session 停止、adapter 进程清理三类动作没有被清晰分层。典型问题包括：

1. 用户点击停止后，Gold Band runtime 已经显示停止，但底层 Claude/Codex 会话仍可能继续输出。
2. `session/cancel` 是 notification，没有 response；如果发送后立刻 kill adapter，adapter 可能还没处理 cancel。
3. kill adapter 不能可靠 kill 底层 provider work，反而会切断 ACP 协议通道。
4. 当前一 attempt 一 adapter process，无法利用 Claude/Codex adapter 内部已有的 session registry，多并行会话时资源浪费明显。
5. 旧 UI Run 列表/详情里的“停止 Run”当前走 terminal `killRun`，语义与新 UI ACP Stop 不一致。

本方案目标：

- 统一 Gold Band 内部所有停止点语义。
- 用户 Stop 走 `session/cancel`，保持 runtime 立即 `Paused + ProcessInterrupted`。
- session release / app close / terminal kill run 走 bounded `session/close`。
- 诊断会话 cleanup 走 `session/delete` first，fallback `session/close`。
- adapter 生命周期从 attempt 级进程改为 `provider_id + workspace_root` 级长连接。
- `provider.pid` 降级为 adapter process metadata，只用于诊断和 orphan cleanup，不再参与业务状态判断。
- 不恢复 `acp.cancel-requested` 文件。

## 2. 协议语义

### 2.1 `session/cancel`

语义：取消当前 active session 上正在执行的 work / prompt。

特征：

- ACP notification，没有 request id，没有 response。
- 不能通过 response 确认 adapter 已处理。
- 适合用户点击“停止当前生成”。
- 正常情况下不释放 ACP session，也不关闭 adapter process。
- ACP session/snapshot 被记录为 `cancelled` 只表达协议层确实观察到用户停止；业务 runtime 仍必须根据当前 attempt/graph 的事实决定是 `Paused + ProcessInterrupted`、`Completed + Success` 还是其他终态。AI-DYNAMIC worker 若已经产出完整合法的 `dynamic-node-completion`，即使 ACP stop reason 是 cancelled，业务层也按完成优先接受。

Gold Band 使用场景：

- 新 UI ACP Stop。
- 旧 UI ACP Stop。
- 普通 Stop Run：对该 run 当前 active ACP sessions 发 cancel。

### 2.2 `session/close`

语义：关闭 active ACP session。规范要求 close 时必须像 cancel 一样取消 ongoing work，然后释放 session 资源。

特征：

- ACP request/response，有 request id。
- close response 可作为 adapter 已处理 session release 的确认点。
- close 后当前 live session 资源已释放，不能继续复用这条 live channel 发 prompt。
- close 不等于删除历史，也不等于 kill adapter process；Gold Band 持久化的 ACP `sessionId` 必须保留，后续 runtime continue 继续使用原 `sessionId` 让 adapter/agent 侧恢复同一会话。

Gold Band 使用场景：

- app close 前释放 active sessions。
- terminal run kill 前释放相关 active sessions。
- 配置保存导致 adapter restart 前释放旧 connection 上的 sessions。
- pool eviction / connection shutdown。
- cancel timeout 后由用户或后台恢复策略选择 close session。

### 2.3 `session/delete`

语义：删除持久化 session/history，比 close 更强。

Gold Band 使用场景：

- 诊断 / doctor 创建的一次性 session cleanup。
- delete 失败时 fallback 到 bounded close。

## 3. 当前参考代码

### 3.1 Gold Band 当前 ACP runtime

- `src/acp/client.rs`：当前 `AcpRuntime` 持有 adapter child、stdin/stdout/stderr、request loop、session/prompt orchestration。
- `src/acp/adapter.rs`：`spawn_adapter(...)` 负责解析并启动 adapter process。
- `src/provider/mod.rs`：`AcpProvider::run_worker_with_callbacks(...)` 调用 `client::run_prompt(...)`。
- `src-tauri/src/commands.rs`：`stop_active_session(...)` / `stop_acp_session(...)` 是新 UI ACP Stop 当前入口。
- `src/app/mod.rs`：`run_pause(...)`、`run_kill(...)`、`stop_all_running_sessions(...)`、`recover_interrupted_running_sessions(...)` 写 runtime lifecycle。
- `src/app/orchestrator.rs`：当前存在基于 `provider.pid` 的 continue guard，需要替换为 prompt/session lifecycle guard。

### 3.2 外部 adapter 参考

Claude adapter：

- `.external/claude-agent-acp/src/acp-agent.ts`：`closeSession(...)` 调 `teardownSession(...)`。
- `.external/claude-agent-acp/src/acp-agent.ts`：`teardownSession(...)` 会先 cancel，再 close query stream，最后从 session map 删除。
- `.external/claude-agent-acp/src/tests/acp-agent.test.ts`：测试覆盖 close 后 session 删除、abort controller 被 abort、wedged prompt 以 cancelled 收尾。

Codex adapter：

- `.external/codex-acp/src/codex_agent.rs`：`close_session(...)` 调 thread shutdown，移除 thread/session root，并返回 `CloseSessionResponse`。
- `.external/codex-acp/src/codex_agent.rs`：`cancel(...)` 处理 `CancelNotification`。
- `.external/codex-acp/src/thread.rs`：shutdown complete 时 pending prompt 返回 `StopReason::Cancelled`。

Zed 参考：

- `D:/Projects/code/ai/zed/crates/agent_servers/src/acp.rs`：用户停止走 `CancelNotification`，close session 走 `CloseSessionRequest` 并 await response。
- `D:/Projects/code/ai/zed/crates/acp_thread/src/acp_thread.rs`：thread cancel 后等待原 prompt task 收尾，并把 cancelled stop reason 映射为停止状态。

## 4. 统一停止点矩阵

| 入口 | Runtime 状态 | ACP 操作 | Adapter process 操作 |
|---|---|---|---|
| 新 UI ACP Stop | 停止当前 leaf/session；普通 attempt 立即写 `Paused + ProcessInterrupted`，AI-DYNAMIC 内部 leaf 只暂停目标 dynamic node，父 run 由 active leaf 聚合决定 | 对目标 ACP session 发 `session/cancel` | 不 kill；cancel timeout 暴露 stop failed |
| 旧 UI ACP Stop | 与新 UI leaf stop 一致 | `session/cancel` | 不 kill |
| 旧 UI / 新 UI 侧边栏 Run Stop | 整个 run 写 `Paused + ProcessInterrupted`，所有 active leaf 一起暂停，已 completed leaf 不被覆盖 | 对当前 attempt 及 AI-DYNAMIC active descendants 的 live sessions 发 `session/cancel` | 不 kill |
| terminal run kill | 写 `Completed + Killed` | bounded `session/close` | close 失败返回/记录错误，不用 kill 伪装成功 |
| app close | running runs 写 `Paused + ProcessInterrupted` | 对所有 live connections 的 sessions 做 bounded `session/close` | 正常退出 adapter 长连接；close 失败记录/返回，不静默吞掉 |
| crash recovery | running runs 写 `Paused + ProcessInterrupted` | 无 live connection，不补发协议 | `provider.pid` 仅用于 orphan cleanup |
| diagnostic cleanup | 不影响业务 runtime | `session/delete` first，fallback close | 释放诊断 session/connection |
| 正常 attempt 完成 | success/failure 按现有 workflow | session idle/reusable 或按策略 close | adapter 留在长连接中复用 |

## 5. 改动点总览

### 5.1 后端 ACP client

目标文件：

- `src/acp/client.rs`
- `src/acp/adapter.rs`
- `src/acp/events.rs`

主要改动：

1. 将当前 `ProviderControlState::ForceStopping` 语义改为 `CancelRequested`。
2. 将 `request_force_stop(...)` 改名或包装为 `request_prompt_cancel(...)`。
3. `session/cancel` 发送后，不立刻退出并 kill adapter。
4. request loop 继续 drain 当前 `session/prompt`，直到：
   - prompt response 返回 cancelled/interrupted；或
   - bounded cancel deadline 到期；或
   - adapter transport failed。
5. cancel deadline 到期时，不 kill adapter 作为成功兜底，而是写明确 stop failed / cancel timeout 状态。
6. 抽出一等公民 helper：
   - `send_session_cancel_notification(session_id)`
   - `close_session_bounded(session_id, timeout)`
   - `delete_session_bounded(session_id, timeout)`
   - `cleanup_diagnostic_session_bounded()`
7. 将 doctor 当前 `cleanup_diagnostic_session(...)` 保留为 delete-first 行为，但改用新的 bounded helper。

### 5.2 Adapter Connection Manager

新增或拆分文件建议：

- `src/acp/connection.rs`
- `src/acp/pool.rs` 或 `src/acp/manager.rs`
- `src-tauri/src/state.rs`

核心模型：

```text
AdapterConnectionKey = provider_id + workspace_root
```

`workspace_root` 表示用户打开的逻辑项目根目录，不是每个 ACP session 的执行 cwd。AI-DYNAMIC worktree 属于该逻辑项目的派生执行目录：adapter process 继续按原始 workspace_root 复用，`session/new.cwd` / `session/load.cwd` 指向具体 worktree，确保并行 worktree session 不再各自启动 adapter。

建议结构：

```text
AdapterConnectionHandle
- key
- pid
- child/stdin/stdout/stderr ownership
- initialized capabilities
- request id allocator
- pending request map
- session registry
- health state
- config version

AcpSessionHandle
- session_id
- attempt locator
- attempt_dir
- lifecycle state
- active prompt handles
- reusable / closing / closed state

PromptRunHandle
- attempt locator
- prompt request id
- cancel requested timestamp
- cancel sent flag
- cancel deadline
- observed stop reason
- runtime stop probe
```

状态建议：

```text
ConnectionState: Starting | Ready | Closing | Failed | Exited
SessionState: Active | IdleReusable | CancelRequested | Closing | Closed | Failed
PromptState: Running | CancelRequested | CancelObserved | Settled | TimedOut
```

第一阶段可先抽出 transport，但仍保持一 attempt 一 adapter；第二阶段再启用 provider + workspace 长连接。

### 5.3 Runtime / app lifecycle

目标文件：

- `src/app/mod.rs`
- `src/app/orchestrator.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/state.rs`

主要改动：

1. `stop_active_session(...)`：
   - 保持 API 不变。
   - 立即写 `Paused + ProcessInterrupted`。
   - 对 active ACP session 发 `session/cancel`。
   - 不 kill adapter。

2. `pause_run` / 旧 UI Stop Run / 新 UI 侧边栏 Run Stop：
   - 普通 run 停止应走 `Paused + ProcessInterrupted`。
   - 对 run 当前 active session 及 AI-DYNAMIC active descendants 的 live ACP sessions 发 `session/cancel`，不改写已 completed leaf 的 terminal ACP snapshot。
   - 与 `stop_active_session` 共享先 runtime control、再 live session registry 兜底的 cancel 逻辑；命中 runtime control 时仍要同时尝试 live registry 直发 `session/cancel`，避免 run 级停止只写本地状态但真实 session 继续输出。
   - 不走 terminal `run_kill()`。

3. 并行 leaf 聚合生命周期：
   - `stop_active_session` 的作用域是当前 leaf attempt/session，不是整个 graph；AI-DYNAMIC 内部 leaf stop 只暂停目标 dynamic node。
   - graph scheduler 继续处理其他 `Ready | Running` leaf；只有没有 active leaf 后，graph / outer node / run 才自动收敛为 `Paused + ProcessInterrupted`。
   - 该规则按 leaf attempt/session、graph scheduler、run aggregate 三层设计，后续普通固定工作流支持并行节点时复用同一模型。
   - 停止/完成竞态按业务 artifact 收敛：provider 返回 `Interrupted/cancelled` 但 AI-DYNAMIC worker 已落盘完整合法 `dynamic-node-completion` 时，dynamic node 进入 `Completed + Success` 并继续 graph；无 artifact、半截 JSON、schema/DSL invalid 或 proposal rejected 时保持 paused，不进入 repair prompt。
   - 详细落地见 `docs/gold-band/开发计划/生命周期整理/并行节点通用停止继续语义与AI-DYNAMIC落地方案.md`。
   - 对历史已落盘的分裂状态，继续入口必须在 re-arm 目标 leaf 前扫描同一 dynamic graph：凡是 `Ready | Running`、`outcome=null` 且 ACP snapshot/session 已 `cancelled` 的 stale active leaf，都先与 per-node 文件一起收敛为 `Paused + ProcessInterrupted` 并移出 `currentNodeIds`，再只恢复本次目标 leaf，避免 sibling 长时间停留在 `running + ACP cancelled` / “拉起下一节点中”。
   - 没有明确 inner leaf override 的父 run continue 不得批量恢复普通 paused worker leaf；只允许恢复代表暂停 child run 的 `workflow-invocation` leaf，让其继续 child run。
   - AI-DYNAMIC 内部 leaf 完成、暂停或被聚合暂停后，后端必须发出该 leaf 的 session/lifecycle update，前端据此刷新完整 run VM；前端不能靠 ACP terminal snapshot 自行推断 workflow runtime 是否完成或暂停。
   - dynamic graph 中任何 leaf 变为 `Ready | Running` 或创建新的 graph 后继 leaf 后，也必须在持久化后发出 lifecycle update；该更新可以没有完整 ACP session payload，前端仍应把它当成 runtime snapshot 处理。
   - 刷新完整 run VM 不等于自动抢焦点：只有 session auto-follow 仍处于 auto/pending 且用户没有手动切到历史 session 时，新 active child session 才能成为选中 session；用户查看历史后必须重新选择最新 active/current leaf 并回到底部才恢复 auto-follow。
   - run 终态刷新由 runtime lifecycle 驱动：`RuntimeLifecycleEvent::RunCompleted` 在 `RunState/RoundState/NodeState` 已持久化后桥接为前端事件，进入会话页统一 `getConversationRun + getConversationSidebar` 刷新入口；ACP terminal update 继续处理 session/graph 实时状态，但不再作为 run completed 的唯一依据。

4. `run_kill(...)`：
   - 保持唯一 terminal killed 路径。
   - 对相关 active ACP sessions 发 bounded `session/close`。
   - close 失败返回/记录错误，不用 kill adapter 伪装成功。

4. app close：
   - 先让所有 running runs 递归写 `Paused + ProcessInterrupted`，包含当前 node、AI-DYNAMIC graph/node 与 child run。
   - 再对 live adapter manager 中所有 provider/workspace connections 的 registered sessions 发 bounded `session/close`，不能只按当前 repo/workspace 过滤。
   - 单个 session close 失败不能短路其他 session 的 close；完成遍历后清理 Gold Band 持有的 adapter connection，并把 close 错误记录/返回为诊断事实。

5. startup recovery：
   - 持久化 `run.status=Running` 的 run 收敛为 `Paused + ProcessInterrupted`。
   - 不从 pid 推断业务状态。
   - `provider.pid` 只做确定属于 Gold Band 的 orphan adapter cleanup。

6. `orchestrator` continue guard：
   - 删除“`provider.pid` exists => wait for provider shutdown”的业务判断。
   - 改为查询 persisted prompt/session lifecycle 或 manager 中 live prompt state。

### 5.4 配置保存边界

目标区域：provider / MCP / adapter 配置保存命令和状态管理代码。实现时先搜索配置保存入口，再接入 manager API。

规则：

1. `provider_id + workspace_root` 是 adapter connection key；AI-DYNAMIC worktree 不生成新 key，而是沿用原始逻辑 workspace root。
2. provider、adapter、MCP、auth/account、默认模型/权限等影响 adapter 行为的配置保存时，必须重启该 key 下 connection。
3. 同 key 不允许新旧配置 connection 并存。
4. 无 active prompt：
   - close idle sessions。
   - 关闭旧 connection。
   - 保存配置。
   - 下次请求启动新 connection。
5. 有 active prompt：
   - 第一版直接阻断配置保存，命令层返回结构化错误 `acp.active-prompt-blocks-config-save`。
   - 前端提示用户先停止当前会话，再重新保存配置。
   - 配置保存流程不自动停止所有 sessions，也不在用户未停止前 close active session。
   - 用户停止后，runtime 保持 `Paused + ProcessInterrupted`，并保留原 ACP `sessionId` 供后续 continue 使用。
6. close 失败：配置保存失败或进入明确错误态，不启动新 adapter。

### 5.5 `provider.pid` 降权

允许用途：

- 记录当前 adapter connection pid。
- 诊断展示。
- app close / config restart 后的进程资源清理记录。
- crash 后 live handle 丢失时 orphan cleanup。

禁止用途：

- 判断 ACP session 是否 running。
- 判断 attempt 是否 stopping。
- 判断 stop 是否成功。
- 判断 run 是否能 continue。
- 推导前端 UI 状态。

实现要求：

- 当前进程内优先使用 `AdapterConnectionHandle` 管理 child process。
- 只有 crash/restart 后没有 live handle 时，才读取 `provider.pid` 做 orphan cleanup。

### 5.6 前端与旧 UI 入口

目标文件：

- `web/src/components/acp/ACPChatDialog.tsx`
- `web/src/pages/WorkflowPage.tsx`
- `web/src/pages/RunDetailPage.tsx`
- `web/src/api.ts`
- `web/src/api/client.ts`
- `web/src/api/desktop.ts`
- `web/src/api/browser.ts`
- `web/src/lib/acp-runtime-composer-state.ts`
- `src-tauri/src/view_models.rs`
- `src-tauri/src/view_models_conversation.rs`

要求：

1. ACPChatDialog 保持单“停止”按钮，调用 `stopActiveSession`。
2. 前端不暴露 cancel/close/delete 协议概念。
3. `stopCommandPending` 只表示 Tauri command pending，不表示 cancel 成功。
4. stopping / active / continue 只由 backend lifecycle、ACP facet、local transient state 推导。
5. 旧 UI `WorkflowPage` / `RunDetailPage` 普通“停止 Run”不再调用 terminal `killRun`。
6. 如果保留 terminal kill 入口，必须改名为“终止/强制终止”等明确文案。

## 6. 推荐实现阶段

### Phase A：稳定当前停止语义

目标：先解决 stop 不应 kill adapter、不应吞掉 cancel 失败的问题。

本轮已落地：

1. `ProviderControlState::ForceStopping` 已收敛为 `CancelRequested`，对外入口改为 `request_prompt_cancel(...)`。
2. stop command 继续立即写 `Paused + ProcessInterrupted`，并取消 pending permission、持久化 cancelled ACP snapshot/session。
3. active runtime 收到 cancel request 后发送一次 `session/cancel`，并继续 drain 当前 `session/prompt`；只有收到 cancelled/interrupted 或 cancel deadline 才结束该 prompt。
4. cancel timeout 返回明确错误，不再 kill adapter 作为成功兜底。
5. doctor cleanup 已抽出 bounded `session/delete` / `session/close` helper，delete first，close fallback。
6. `run_pause` / 旧 UI Run Stop 已改为 pause/interrupted 语义，普通停止不再按 `provider.pid` kill adapter。
7. `session/prompt` 正常返回 `stopReason=cancelled/canceled/interrupted` 时，ACP session metadata 保持 `cancelled`，不会被写成 `completed`。

第二轮已继续落地 Phase B-D 的主链路：JSON-RPC transport 已拆为 `src/acp/connection.rs`，普通 ACP worker 改为通过 `provider_id + workspace_root` 级 connection manager 复用 adapter process；`session/update` 与 permission request 通过 `sessionId` 路由回对应 attempt，request/response 通过 pending request id 路由；普通 runtime release 不再无条件 kill adapter。`run_kill` 与 app close 已接入 bounded `session/close`，agent/provider/MCP 配置保存作为 connection restart boundary；普通 workspace 切换不关闭旧 workspace connection，以支持新 UI 多 workspace 并存。配置保存遇到 active prompt 会先阻断并提示用户停止会话；停止后仍保留原 ACP `sessionId`，continue 必须用原 `sessionId` 恢复同一业务会话。`orchestrator` 的 `provider.pid exists => wait` continue guard 已删除，`provider.pid` 只保留为 adapter process metadata / orphan cleanup 线索。

### Phase B：抽出 JSON-RPC transport

目标：把 process/transport 与 session/prompt runner 解耦。

已落地：`AdapterConnection` 统一持有 child/stdin/stdout/stderr、request id allocator、pending request map、session route map 与 initialized capabilities cache；`AcpRuntime` 只保留 attempt-local timeline/session/prompt 状态，普通 request 通过 pending id 等待 response，`session/update` / `session/request_permission` 通过 `sessionId` 路由到对应 attempt receiver。

改动：

1. stdout/stderr reader、request id、pending request map 抽成 connection transport。
2. request routing 统一按 request id 返回。
3. session/update routing 增加 sessionId -> attempt/session handle 映射。
4. permission request routing 绑定到正确 attempt_dir。
5. `AcpRuntime` 不再无条件 owning/killing child。

### Phase C：启用 provider + workspace 长连接

目标：一个 provider/workspace adapter process 承载多个 sessions。

已落地：`AdapterConnectionManager` 以 `provider_id + workspace_root` 为 key 缓存 connection；`client::run_prompt(...)` 获取同 key connection 后创建 / load ACP session，同一 adapter process 可承载多个 session route 与 active prompt，普通 prompt 完成只 release attempt-local runtime，不关闭 adapter process。stdout 断开、写入失败、request route 断开或 adapter process exit 都会被映射为 recoverable interruption，runtime 收敛为 `Paused + ProcessInterrupted`，后续 continue 重新拉起 connection 并使用原 ACP `sessionId`。

改动：

1. Tauri state 中持有 `AdapterConnectionManager`。
2. `AcpProvider` 发起 run_prompt 时从 manager 获取 connection。
3. 同一 connection 可创建/load 多个 ACP sessions。
4. 多个 active sessions 并发 prompt。
5. connection-level failure 按可恢复中断处理，active sessions/prompt 通过等待路径收敛为 `Paused + ProcessInterrupted`。

### Phase D：配置保存、app close、recovery 完整接入

目标：完成长连接生命周期闭环。

已落地：`run_kill` 与 dynamic terminal kill 对 live attempt session 发 bounded `session/close`；app close close 当前 workspace 的 live connections，并把 running runs 收敛为 `Paused + ProcessInterrupted`；agent/provider 配置、MCP 配置在保存前检查 active prompt，无 active prompt 时 close 旧 connection，有 active prompt 时返回结构化错误并提示先停止会话；普通 workspace 切换只切换当前视图/上下文，不关闭旧 workspace connection；startup recovery 仍只依据 persisted runtime/session lifecycle 收敛业务状态，不补发协议；`provider.pid` 不再阻塞 continue。

改动：

1. 配置保存接入 adapter restart boundary。
2. app close 先 close sessions，再关闭 connection。
3. startup recovery 只依据 persisted runtime/session lifecycle 收敛业务状态。
4. `provider.pid` 只用于 orphan cleanup。
5. 删除 pid-based continue guard。

## 7. 单元测试与回归测试

### 7.1 Rust：Stop/cancel

用 mock ACP adapter / protocol harness 覆盖：

1. `stop_active_session` 立即写 `Paused + ProcessInterrupted`。
2. active runtime 收到 cancel request 后发送 `session/cancel`，不发送 `session/close`。
3. 重复 stop 只发送一次 cancel notification。
4. `session/prompt` 返回 `stopReason=cancelled` 后，provider result 映射为 interrupted。
5. cancel 后迟到 success response 不写 success artifact、不推进 workflow。
6. cancel timeout 不调用 kill adapter 作为成功兜底，而是写 stop failed / cancel timeout。

### 7.2 Rust：close/delete

1. `close_session_bounded` 发送 `session/close` request，并等待 response。
2. `close_session_bounded` timeout 后返回明确错误。
3. app close / run kill / session release 使用 close，不走 cancel。
4. diagnostic cleanup 先 `session/delete`，delete 失败 fallback `session/close`。
5. delete 和 close 都失败时，diagnostics 中记录错误。

### 7.3 Rust：runtime invariant

1. 普通 stop / pause / app close / crash recovery 都写 `Paused + ProcessInterrupted`。
2. `run_kill` 是唯一写 `Completed + Killed` 的路径。
3. dynamic inner attempt 停止时，outer node / dynamic graph / inner node 都保持 paused/interrupted，不写 killed。
4. stopped attempt 的 provider success 不能 transition 到下一节点。
5. run continue 不再因 `provider.pid` 存在而阻塞。

### 7.4 Rust：adapter 长连接

1. 同一 `provider_id + workspace_root` 复用同一个 adapter connection。
2. 不同 provider 或 workspace 创建不同 connection。
3. 同一 connection 内可注册多个 sessions。
4. 两个 sessions 同时 active prompt 时，response 按 request id 回到各自 prompt。
5. `session/update` 按 sessionId 写入正确 attempt timeline。
6. `session/cancel` 只影响目标 session。
7. `session/close` 只释放目标 session，不影响同 connection 其他 session。
8. connection crash / stdout disconnect / transport closed 后，active prompt 等待路径收到 `AcpTransportInterrupted`，runtime 进入 `Paused + ProcessInterrupted`，后续 continue 创建新 connection 并复用原 ACP `sessionId`。

### 7.5 Rust：配置保存 boundary

1. 无 active prompt 时，保存配置会 close idle sessions 并关闭旧 connection。
2. 有 active prompt 时，配置保存直接失败并返回 `acp.active-prompt-blocks-config-save`。
3. 前端把结构化错误渲染为“先停止会话再保存配置”的用户动作。
4. 用户停止后，保存配置可以关闭 idle connection，并且停止后的 ACP 会话仍可用原 `sessionId` continue。
5. close 失败时配置保存失败，不启动新 adapter。
6. 同 key 下不出现新旧配置 connection 并存。

### 7.6 前端单元测试

1. ACP Stop 仍调用 `stopActiveSession`。
2. command pending 显示 stopping/overlay。
3. paused/interrupted lifecycle 后 composer 进入 continue input。
4. terminal/cancelled ACP snapshot 不显示 killed/error 文案。
5. stopping 状态不依赖 pid。
6. `WorkflowPage` / `RunDetailPage` 普通“停止 Run”调用 pause/interrupted 语义 API，不再调用 terminal `killRun`。
7. terminal kill 如果保留，文案与普通停止区分。

## 8. 人工验证案例

实现阶段不要求页面集成验证；以下场景由人工验证：

1. 新 UI ACP 运行中点击停止：
   - Gold Band 立即显示 paused/interrupted。
   - 输入区可继续。
   - 底层 Claude/Codex 不再持续输出。

2. 旧 UI ACP 会话点击停止：
   - 行为与新 UI 一致。

3. 旧 UI Run 列表/详情点击普通停止：
   - run 进入 paused/interrupted。
   - 不显示 killed/terminated。
   - 可以继续。

4. “agent 调起中...”阶段点击停止：
   - runtime 立即 paused。
   - 后续 provider success 不推进 workflow。

5. 并行多个 ACP attempt：
   - 同一 provider + workspace 下 adapter process 数量不随 attempt 线性增长。
   - 多个 session 的消息不串流、不串 timeline。

6. 配置保存：
   - 有 active prompt 时保存被阻断，并提示先停止当前会话。
   - 用户停止后再保存，旧 idle connection 被 bounded close。
   - 保存后新请求使用新配置。
   - 停止后的 ACP 会话仍可用原 `sessionId` continue。
   - 不存在同 key 新旧配置 connection 并存。

7. 关闭应用：
   - 重启后 running runs 收敛为 paused/interrupted。
   - 不出现 pid 驱动的错误 stopping 状态。

8. 诊断能力检测：
   - 临时 session 被 delete/close。
   - 不污染业务会话列表和 runtime 状态。

9. 新 UI 移除 workspace：
   - 侧边栏 workspace 标题悬浮删除按钮是显式 workspace remove boundary。
   - 移除前按该 workspace path bounded close 对应 ACP connections。
   - close 失败时移除失败并保留 workspace，不静默丢弃错误。
   - 普通 workspace 切换仍不关闭旧 connection。

## 9. 不做事项

- 不把用户 Stop 改成默认 `session/close`。
- 不把 kill adapter 当成 cancel/close 失败后的成功兜底。
- 不用 `provider.pid` 推断业务状态或 UI 状态。
- 不恢复 `acp.cancel-requested` 文件。
- 不全局 kill `claude.exe` / `node.exe`。
- 不让前端理解 ACP 协议细节。
- 不把 adapter key 设计得过重；第一版就是 `provider_id + workspace_root`。

## 10. 后续增强项

1. `AdapterConnectionManager` 迁移到 Tauri/App state：
   - 当前实现仍通过全局 singleton 访问 manager，功能上可满足本轮停止、配置保存和 workspace 移除语义。
   - 后续建议由 `DesktopState` / `AppState` 持有 `Arc<AdapterConnectionManager>`，让 connection 生命周期跟随 app instance，便于 app close 统一释放、测试注入独立 manager，并避免多窗口/多 profile 场景下全局状态污染。
   - 迁移时不改变业务 key，仍保持 `provider_id + workspace_root`。

2. 完整 ACP 协议 harness：
   - 当前已有 key、`sessionId` 提取、continue 原 `sessionId` 和错误文案等定向测试，但还缺一个 fake ACP adapter / protocol harness 来覆盖真实并发路由。
   - harness 需要模拟同一 adapter process 内多个 sessions 并发 prompt，交错返回 JSON-RPC response、`session/update` 和 permission request，验证 response 按 request id、timeline/permission 按 `sessionId` 路由。
   - 还需要覆盖 cancel A 不影响 B、close A 不影响 B、transport disconnect 后 active runtime 收敛为 `Paused + ProcessInterrupted`、continue 重新 spawn connection 且继续使用原 ACP `sessionId`。

3. `provider.pid` orphan cleanup 安全模型：
   - 本轮已完成降权：`provider.pid` 不再推导 running/stopping/continue/stop 成功，不参与 UI 状态。
   - 后续如果要在 crash 后清理残留 adapter process，不能只凭 pid 文件 kill。必须校验 pid 仍存在、command line/cwd/env/启动 marker 能证明它是 Gold Band 为对应 workspace/provider 启动的 adapter，且 pid 未被系统复用。
   - 建议增加 per-process nonce/marker 文件或启动 env marker；清理失败只记录诊断，不影响业务 continue，也不全局 kill `claude.exe` / `node.exe`。
