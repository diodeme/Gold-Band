# ACP 会话长连接与历史同步技术方案

## 1. 文档状态

- 状态：已实施（2026-07-25）
- 适用范围：Direct、Workflow、AUTO、AI-DYNAMIC 内部 ACP session
- 关联方案：
  - `docs/gold-band/开发计划/生命周期整理/ACP停止语义与Adapter长连接开发方案.md`
  - `docs/gold-band/开发计划/生命周期整理/工作流-ACP-生命周期统一重构.md`
  - `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`

本文解决三个相互关联的问题：

1. Provider 在 `session/load` 时完整回放历史，导致 `acp.timeline.jsonl` 对相同稳定 ID 重复追加 patch，长期存在接近 O(N²) 的存储增长。
2. 当前每次 prompt 都创建新的 `AcpRuntime`，执行 `initialize -> session/load -> session/prompt`，随后注销 session route；连续追问没有复用已经打开的 ACP session。
3. 部分 Agent 可能允许 Gold Band 与外部客户端共享 Provider session；该能力不能按产品全局假定，而要由每个 Agent 显式声明后才同步外部新增历史。

本方案不是针对 Direct 的局部优化。Direct 后续对话、Workflow/AUTO 节点完成后的手动追问、runtime continue 必须统一使用同一套 ACP session runtime、历史同步和持久化规则。

## 2. 当前根因

### 2.1 Prompt 生命周期错误地拥有 Session 生命周期

当前 `client::run_prompt(...)` 每次调用都会：

```text
创建 AcpRuntime
  -> 获取或启动 AdapterConnection
  -> initialize
  -> session/new 或 session/load
  -> 注册 session route
  -> session/prompt
  -> 写 snapshot/session/worker-ref
  -> 注销 session route
  -> 销毁 AcpRuntime
```

虽然 `AdapterConnectionManager` 已按 `provider_id + workspace_root` 复用 adapter process，但 `AcpRuntime` 和 session route 仍属于单次 prompt。因此连续追问复用了进程，却没有复用已打开的 session runtime。

### 2.2 Timeline 只在读取时合并，没有在写入时去重

`append_timeline_patch(...)` 是 append-only。`load_timeline_items(...)` 才按 `itemId + revision` 合并为 canonical item。

因此当前事实是：

```text
磁盘文件：一个稳定 itemId 可以有多行 patch
加载投影：一个稳定 itemId 最终只显示一个 item
```

完整历史每次 reload 都重新进入持久化链路时，即使 UI 最终只显示一条，文件仍会重复增长。

### 2.3 ACP 没有通用的增量历史 cursor

当前内置 Claude/Codex adapter 的 `session/load` 会完整回放 Provider 历史。ACP 没有统一的 `afterRevision`、`afterItemId` 或增量 history cursor，因此一次真正的 reload 至少需要接收并比较完整历史。

能够优化的是：

- 不要每次 prompt 都 reload。
- reload 时相同历史只比较、不重复写盘。
- 使用 Provider freshness 信号判断是否需要 reload。

不能在现有协议下保证的是：频繁在多个客户端之间交替写入同一超长 session 时，Provider 完整 replay 的网络与解析成本始终为 O(1)。要彻底解决该成本，需要未来增加 Provider 增量同步能力。

## 3. 设计目标

### 3.1 必须满足

- 同一 attached session 的连续 prompt 直接调用 `session/prompt`，不执行 `session/load`。
- Direct、Workflow、AUTO、AI-DYNAMIC 使用统一 session runtime 管理。
- Provider history 使用稳定 ID 对账；相同 canonical 内容不追加 timeline patch。
- timeline 定期 compaction，文件大小与唯一 UI item 数保持同阶。
- adapter connection、session runtime 都是有界资源，不能随历史会话数无界增长。
- Agent 显式开启外部会话同步后，Gold Band 才在下一次用户 prompt 前通过 `session/list.updatedAt` 检测并 reload。
- MCP、cwd 等本地 session-defining 配置变化时，即使 Provider `updatedAt` 未变化，也必须使用最新配置 reload。
- raw frame 保留原始到达顺序和既有容量滚动，不因 timeline 去重或 compaction 被修改。
- active prompt、permission、elicitation、cancel/close 中的 session 不得被 TTL/LRU 驱逐。

### 3.2 非目标

- 本阶段不新增“同步 Agent 会话”按钮。
- 本阶段不默认认定 Claude/Codex 等 Agent 的 ACP 会话与其原生客户端会话属于同一线性分支。
- 本阶段不为不支持 `session/list.updatedAt` 的第三方 Provider 猜测外部变化。
- 本阶段不修改 ACP 协议或要求第三方 Provider 实现 Gold Band 私有增量历史协议。
- 本阶段不让完整历史常驻 `AcpRuntime` 热内存。
- 本阶段不改变现有 sessionId、attempt 目录、worker-ref、raw frame 和前端 timeline ViewModel 协议。

## 4. 领域模型

生命周期必须拆成四个领域，不再由单次 prompt 统一拥有。

### 4.1 AdapterConnectionPool

职责：管理 adapter process 和 JSON-RPC transport。

```text
AdapterConnectionKey
  - providerId
  - workspaceRoot
```

建议结构：

```text
AdapterConnectionEntry
  - key
  - connection
  - generation
  - configSignature
  - state
  - createdAt
  - lastActivityAt
  - attachedSessionCount
  - activePromptCount
  - leaseCount
```

`generation` 每次 transport 重建递增。Session runtime 保存自己 attached 时的 generation；两者不一致时必须重新 attach/load。

状态：

```text
Starting -> Ready -> Draining -> Closed
                  -> Failed
```

### 4.2 AcpSessionRuntimeRegistry

职责：管理已经 attach 到某条 AdapterConnection 的 ACP session。

```text
AcpSessionRuntimeKey
  - attempt locator
  - acpSessionId
```

attempt locator 使用现有稳定运行定位：

```text
projectId + taskId + runId + roundId + nodeId + attemptId
```

AI-DYNAMIC 内部 session 继续附加 `outerNodeId + outerAttemptId`。

建议结构：

```text
AcpSessionRuntimeEntry
  - key
  - sessionId
  - attemptDir
  - connectionKey
  - connectionGeneration
  - state
  - routeReceiver / eventPump
  - lastActivityAt
  - foregroundLeaseUntil
  - providerFreshness
  - attachedConfigFingerprint
  - externalSessionSyncEnabledAtAttach
  - syncRequired
  - activePrompt
  - pendingPermissionCount
  - pendingElicitationCount
  - timelineWriter
```

状态：

```text
Detached
  -> Attaching
  -> IdleAttached
  -> Prompting
  -> IdleAttached
  -> Detaching
  -> Detached

任意状态 -> Failed
```

Session runtime 必须拥有持续 event pump。只注册 route 而不持续消费会导致有界 route 队列填满，最终阻塞共享 connection 的 stdout 分发，因此不能保留没有消费者的“空 route”。

### 4.3 PromptRun

职责：表示单次 `session/prompt`，生命周期只覆盖一次 turn。

```text
PromptRun
  - turnId
  - promptRequestId
  - startedAt
  - stopProbe
  - cancelRequestedAt
  - stopReason
  - finalOutput
  - usage
```

PromptRun 结束后释放，但 AcpSessionRuntime 可以继续保持 `IdleAttached`。

### 4.4 TimelineStore

职责：统一处理 timeline canonical merge、fingerprint、patch 写入和 compaction。

所有 timeline 写入必须通过统一接口：

```text
TimelineStore::upsert(item) -> Unchanged | Appended
TimelineStore::compact_if_needed()
```

permission、elicitation、provider history、streaming message、tool update、placement patch 都不得绕过 TimelineStore 直接追加文件。

## 5. Prompt 分发统一入口

新增统一的 session prompt dispatcher，替代上层直接调用 `client::run_prompt(...)`。

```text
SessionPromptDispatcher::submit(request)
```

请求至少包含：

```text
SessionPromptRequest
  - providerId
  - adapterWorkspaceRoot
  - providerCwd
  - attemptLocator
  - attemptDir
  - continueRef / sessionId
  - promptBundle
  - desiredSessionConfig
  - promptOrigin
  - lifecycle callbacks
```

`promptOrigin` 用于区分同步策略，不用于分叉 session 实现：

```text
UserDirectPrompt
UserWorkflowFollowUp
UserRuntimeContinue
RuntimeAutomaticContinue
RuntimeRepair
```

规则：

- 所有 origin 使用同一 AcpSessionRuntimeRegistry。
- 只有 Agent 的 `externalSessionSyncEnabled=true` 时，用户触发的 prompt 才在 session idle 时执行 freshness probe。
- runtime 内部连续 repair/automatic continue 在同一 attached runtime 上直接 prompt，不额外执行外部 freshness probe。
- session 不存在、已驱逐或 transport generation 变化时，所有 origin 都必须 attach/load。

### 5.1 Agent 级能力开关

外部会话同步属于 `ManagedAgentConfig`，不属于 `configs/app-config.toml` 的全局运行参数：

```text
ManagedAgentConfig
  - adapter
  - primaryAgentDir
  - compatibleAgentDirs[]
  - externalSessionSyncEnabled = false
```

默认关闭，并以 Beta 能力展示。只有 Agent 明确保证不同客户端连接的是同一条线性上下文，或未来 Gold Band 能显式选择 Provider branch/leaf 时才允许开启。Agent 管理页的修改抽屉必须同时编辑 `adapter`、主 Agent 目录、兼容 Agent 目录和 `externalSessionSyncEnabled`；主目录不能为空，兼容目录规范化后只参与读取。同步开关标题右侧展示紧凑 Beta Badge 和可聚焦问号 Tooltip，Tooltip 解释“同步同一个 Session 在其他客户端中发生过的对话”，说明文案明确警告：仅在确认 Agent 支持跨客户端共享同一会话上下文时开启，否则可能造成历史顺序或上下文理解错误。列表主卡片仅保留命令、参数、环境变量和最近检测四项运行摘要，不展示高级配置。

Agent 配置是 Provider 级全局配置，不属于当前 workspace。新增、修改或删除 Agent 前，必须跨所有 workspace 检查该 Provider 是否存在 active prompt；保存前统一 detach 该 Provider 的 idle session runtime，并关闭所有 `provider + workspace` connection，使下一次 prompt 使用新配置，不能只失效当前 `App.paths.repo_root`。

关闭时仍保留长连接与必要恢复：attached session 直接 `session/prompt`；detached/被驱逐/session generation 变化时仍执行 `session/load`，但 load 的 Provider history replay 整体只保留在 raw 审计，不导入 timeline。不能只丢弃 `user_message_chunk`，否则会留下没有用户上下文的 assistant/tool 历史。

## 6. Reload 判定

### 6.1 ProviderFreshness

内置 Claude/Codex 的 `session/list` 返回：

```text
sessionId
cwd
title
updatedAt
```

定义：

```text
ProviderFreshness
  - capability: Supported | Unsupported | TemporarilyUnavailable
  - revision: Option<String>
  - observedAt
```

`updatedAt` 在 Gold Band 内作为 opaque revision token 使用，不解析为业务时钟，不比较时间大小，只判断字符串是否相同。

当前 title refresh 只读取 `title`，实施时应抽出独立接口：

```text
probe_session_freshness(sessionId, cwd)
  -> Found { revision, title? }
  -> NotFound
  -> Unsupported
  -> TemporarilyUnavailable
```

实现必须处理 `session/list` pagination/cursor，并设置有界页数和超时；不能假设目标 session 永远位于第一页。

### 6.2 SessionConfigFingerprint

Provider `updatedAt` 只能反映 Provider 侧 session 变化，不能检测 Gold Band 本地刚修改的 MCP/session 配置。

定义规范化配置：

```text
DesiredSessionConfig
  - provider identity / adapter config generation
  - providerCwd
  - normalizedMcpServers
  - sessionSystemPromptVersion
```

MCP server 必须按稳定字段规范化并排序，避免仅顺序变化触发 reload。fingerprint 不包含 model、permission mode 等可通过 session config API动态修改的字段。

```text
SessionConfigFingerprint = hash(canonical_json(DesiredSessionConfig))
```

Claude adapter 已使用 `cwd + sorted mcpServers` fingerprint 判断是否需要 teardown/recreate；Codex adapter 会在 load/resume 时用最新 MCP 构建新的 session config。因此内置 adapter 可以通过 reload 获取新 MCP。

### 6.3 最终判断矩阵

```text
needReload =
    session runtime 不存在
    OR session state == Detached/Failed
    OR connection generation 变化
    OR syncRequired
    OR (externalSessionSyncEnabled AND provider revision 变化)
    OR desiredSessionConfigFingerprint != attachedSessionConfigFingerprint
```

详细行为：

| 条件 | 行为 |
|---|---|
| 外部同步关闭，runtime attached，config fingerprint 相同 | 不调用 `session/list`，直接 `session/prompt` |
| 外部同步关闭，runtime detached | 正常 load/resume，但不把整段 Provider history replay 写入 timeline |
| attached runtime 创建时同步关闭，当前配置首次开启 | 立即设置 `syncRequired`；下一次 prompt 前直接 load，不先调用 `session/list` |
| runtime attached，revision 相同，config fingerprint 相同 | 直接 `session/prompt` |
| runtime attached，revision 变化 | session idle 后 reload，导入历史，再 prompt |
| runtime attached，本地 MCP/cwd/system context fingerprint 变化 | 携带最新配置 reload，再 prompt |
| session runtime 被 TTL/LRU 驱逐 | load/resume 一次，再 prompt |
| connection generation 变化 | 在新 transport 上 initialize + load/resume，再 prompt |
| Provider 不支持 `updatedAt`，runtime attached | 不猜测外部变化，直接 prompt |
| Provider 不支持 `updatedAt`，runtime detached | 正常 load/resume |
| freshness probe 临时失败，runtime attached | 不因探测失败反复 reload；记录诊断并直接 prompt |
| freshness probe 从未知恢复为可用，但没有可信 baseline | reload 一次建立新 baseline |
| session/list 明确 NotFound | 按 session-not-found 现有错误/重建策略处理，不静默创建另一会话 |

### 6.4 Baseline 更新

为了区分 Gold Band 自己刚写入的变化和外部客户端后续变化：

以下流程仅在当前 Agent 开启 `externalSessionSyncEnabled` 时执行：

1. attached runtime 必须保存创建时的同步策略。检测到 `false -> true` 时设置 `syncRequired`，该状态优先于 freshness probe。
2. `syncRequired` 完成前必须成功执行 `session/load` 并完成 replay 收敛；load 失败时终止本轮 prompt，不得回退 `session/new`。
3. session new/load 完成后记录一次 Provider revision。
4. 本地 prompt 完成后 best-effort 再探测一次并保存 revision baseline；`syncRequired` 清除前禁止执行该更新，避免本地新 prompt revision 覆盖旧 baseline。
5. 下一次用户 prompt 前探测；与 baseline 不同才 reload。
6. post-prompt baseline 探测失败时标记为 `Unknown`，探测恢复后 reload 一次，而不是永久信任未知 baseline。

`session/list` freshness probe 保持单页 5 秒、最多 8 页的有界 best-effort 行为。超时不通过扩大阈值掩盖：普通 attached session 可记录诊断后继续 prompt；首次开启同步等 `syncRequired` 场景根本不进入 probe，而是直接 load。

会话标题刷新可以继续读取 `session/list.title`，但不得因此写入或改变 Provider revision baseline；标题能力与外部历史同步策略是两个独立领域。

并发在两个客户端同时对同一个 Provider session 写 prompt 不属于本阶段保证范围。Gold Band 保证的是串行切换场景下，在下一次用户 prompt 前进行最终一致性同步。

## 7. Timeline 写入去重

### 7.1 Canonical merge

TimelineStore 维护当前 attached session 的轻量写入索引：

```text
TimelineWriteIndex[itemId]
  - latestRevision
  - semanticFingerprint
  - firstSeq
  - firstTimestamp
```

索引只保存 ID、hash 和少量位置元数据，不保存完整会话消息正文。Session runtime detach 后释放该索引。

收到 item 时：

```text
existing canonical item
  + incoming item
  -> merge_timeline_item_revision
  -> merged item
  -> semantic fingerprint
```

若 fingerprint 未变化，不追加 patch。

### 7.2 Semantic fingerprint

fingerprint 包含影响最终 UI/业务事实的字段，例如：

- kind
- role/content/update
- title/status
- toolCallId/tool detail
- permission/elicitation 状态与结果
- timing terminal aggregate
- historyPlacement
- provider history stable identity
- runtime control output display

fingerprint 不包含：

- 本次 replay 新生成的 seq
- replay 到达 timestamp
- timeline revision
- 仅用于原始帧审计的 transport metadata

首次 seq/timestamp/startedSeq 继续由 merge 逻辑保留。`historyPlacement` 只有实际发生变化时才写一个 placement patch。

### 7.3 Streaming update

现有 streaming patch throttle 保留，但写入前增加 semantic fingerprint 判断：

- 内容延长：写入或进入 pending throttle。
- 完全相同的重复 chunk/update：跳过。
- stream terminal 状态变化：必须写入。

### 7.4 Provider replay

一次完整 Provider replay 的处理结果应为：

```text
相同历史 item：只比较，不写盘
外部新增 item：写入
已有 item 内容/状态变化：写入一个 revision
位置锚点变化：写入一个 placement revision
```

因此 replay 仍可能是 O(N) 解析，但磁盘写入与实际变化量同阶。

## 8. Timeline Compaction

仅写入去重不能消除正常 streaming 和历史位置修订产生的旧 patch，需要增加 compaction。

### 8.1 触发条件

触发参数必须配置化，建议同时支持：

```text
timeline file bytes > max bytes
OR patch count / unique item count > ratio
```

compaction 不按 raw 的“直接裁掉最旧行”方式实现，因为裁剪可能删除某个 item 的最新 revision。

### 8.2 Compaction 流程

```text
获取 timeline 文件锁
  -> load canonical items
  -> 按最终投影顺序生成一个 snapshot item/每个稳定 ID
  -> 写临时文件
  -> flush/fsync
  -> 原子 replace
  -> 重建 TimelineWriteIndex
```

compaction 后必须保持：

- `load_timeline_items()` 投影完全一致。
- item 稳定 ID 不变。
- first seq/timestamp/startedSeq 不变。
- historyPlacement 不变。
- raw 文件完全不变。
- pending permission/elicitation 不丢失。

### 8.3 崩溃安全

- 不原地截断后重写。
- 临时文件未完成时继续使用旧 timeline。
- replace 完成后临时文件可清理。
- Windows 文件锁与 replace 失败必须返回结构化诊断，不能留下半份 timeline。

## 9. 有界长连接规则

### 9.1 Foreground lease

当前前台选中的 session 由桌面端续租。初始配置：

```toml
acpSessionForegroundLeaseTtlSecs = 90
acpSessionForegroundLeaseRenewIntervalSecs = 30
```

lease 只保护 idle session 不被清理，不创建新的 session，也不触发 reload。

页面隐藏、切换 session 或窗口销毁后停止续租；lease 到期后 session 进入普通 idle LRU 候选。

### 9.2 Session runtime TTL/LRU

初始配置：

```toml
acpSessionIdleTtlSecs = 600
acpMaxIdleSessionRuntimes = 8
```

驱逐规则：

1. 仅选择 `IdleAttached`。
2. 排除 foreground lease 未过期的 session。
3. 排除 active prompt、permission、elicitation、cancel/close 中的 session。
4. 先清理超过 TTL 的 session。
5. 仍超过 idle 容量时按 LRU 清理。
6. active session 数量可以临时超过容量；终态后立即重新收敛。

驱逐 session 时：

```text
flush timeline pending patch
  -> settle pending interaction
  -> bounded session/close
  -> unregister route
  -> stop event pump
  -> drop TimelineWriteIndex/hot state
  -> registry state = Detached
```

sessionId、worker-ref、timeline 和 snapshot 保留，后续可 reload。

### 9.3 Adapter connection TTL/LRU

初始配置：

```toml
acpAdapterConnectionIdleTtlSecs = 600
acpMaxIdleAdapterConnections = 4
```

Connection 只有在以下条件同时满足时才可驱逐：

- active prompt count 为 0。
- attached session count 为 0。
- 不在 Starting/Draining。
- 无 connection lease/config transaction。

先按 idle TTL 清理，再按 LRU 收敛到容量。关闭必须复用现有两阶段 bounded close/drain 语义。

### 9.4 连接与 Session 数量关系

不需要为了避免 reload 保持所有历史 session：

```text
一个 workspace/provider adapter connection
  -> 少量 active/recent attached sessions
  -> 大量 detached historical sessions，仅保留磁盘事实与 sessionId
```

查看历史 timeline 不要求 attach session。只有 freshness 同步、发送 prompt、修改 session config、permission/elicitation 恢复等操作才需要 session runtime。

## 10. MCP 与 Session 配置更新

### 10.1 MCP 增删改

用户修改 MCP 后，Provider `updatedAt` 不会自动变化，因此依赖 SessionConfigFingerprint。

下一次 prompt：

```text
desired fingerprint != attached fingerprint
  -> session/load 携带最新 mcpServers
  -> adapter 用相同 sessionId 重建/恢复 session
  -> replay 经 TimelineStore 去重
  -> session/prompt
```

### 10.2 不同配置的处理边界

| 配置变化 | 行为 |
|---|---|
| MCP server 列表/transport/env | reload session；必要时重建 session 内 Provider runtime |
| provider cwd/worktree | reload session |
| session system context/version | reload session，前提是 Provider 支持对应 metadata |
| model | 优先 session config API，不 reload |
| permission mode | 优先 session config API，不 reload |
| adapter command/args/env/useLocalClaude | 重建 AdapterConnection，generation 变化后 reload session |
| provider/agent 类型变化 | 新 connection + 新 session 身份，不复用旧 attached runtime |

## 11. Direct、Workflow、AUTO 接入

### 11.1 Direct

- 首轮：创建或获取 session runtime，`session/new -> session/prompt`。
- 后续追问：attached 且 freshness/config 未变化时直接 `session/prompt`。
- session 被驱逐、应用重启或 Provider revision 变化时 load 一次。

### 11.2 Workflow/AUTO 手动追问

当前已统一通过非 runtime-continue ACP turn 事件表达，但底层必须改走 SessionPromptDispatcher。

- 节点完成后的手动追问复用该 attempt 的 session runtime。
- 不重新运行完整 workflow validation。
- 不因 run 已 completed 就强制关闭 session；是否保留由 session TTL/LRU 决定。
- 手动追问仍产生独立稳定 turnId 和 `AcpTurnFinished`。

### 11.3 Runtime continue

- `process-interrupted` 后用户输入继续：使用原 session runtime；用户触发路径执行 freshness probe。
- workflow 自身无用户输入的自动继续：attached 时直接 prompt，不做外部 freshness probe。
- transport/session 已失效：load/resume 后继续现有 runtime orchestration。

### 11.4 Runtime repair

repair 属于同一业务 turn/运行控制流，不应在每个 repair prompt 前 reload。只有 connection generation 或 session state 已失效时才重新 attach。

### 11.5 AI-DYNAMIC

- 每个内部 leaf session 使用自己的 AcpSessionRuntimeKey。
- 并行 active leaf 均为 pinned，不受 idle 容量限制。
- leaf 完成后进入 idle LRU；用户后续选中并追问时复用或 reload。
- session runtime 管理不得改变 dynamic graph 的完成、暂停、proposal 和 artifact 生命周期事实。

## 12. 配置设计

新增项目级配置统一放入：

```text
configs/app-config.toml
```

建议字段：

```toml
acpSessionForegroundLeaseTtlSecs = 90
acpSessionForegroundLeaseRenewIntervalSecs = 30
acpSessionIdleTtlSecs = 600
acpAdapterConnectionIdleTtlSecs = 600
acpMaxIdleSessionRuntimes = 8
acpMaxIdleAdapterConnections = 4

# Timeline compaction 参数在实现前结合 task-061、长流式回复和大工具调用样本确定。
acpTimelineCompactMaxSizeBytes = 8388608
acpTimelineCompactPatchRatio = 4
```

规则：

- 字段加入 `ProjectAppConfig`，使用 camelCase TOML 名称。
- 默认值在 `RuntimeConfig::default()` 中定义。
- `configs/app-config.toml` 只覆盖声明值。
- 非法的 0 值或不合理组合必须在 apply 阶段拒绝或回退默认值。
- lease renew interval 必须小于 lease TTL。
- timeline compaction target 不能用 raw 的直接裁剪 target 语义。

初值只用于建立有界行为，后续根据 metrics 调整，不把数值写死在 pool/registry 实现中。

## 13. 失败与降级

### 13.1 session/list 不支持

- 每个 adapter connection generation 只探测一次 Unsupported，避免每次 prompt 重复报错。
- attached session 直接 prompt。
- detached session 正常 load/resume。
- 不承诺实时导入外部客户端历史。

### 13.2 session/list 临时失败

- 不把网络/Provider 临时失败转换成无条件 reload。
- 保持 attached runtime 可用并发送 prompt。
- 记录限频结构化诊断。
- baseline 标记 Unknown；探测恢复后 reload 一次。

### 13.3 reload 失败

- SessionNotFound：沿用明确的 session missing 错误语义，不伪装为新 session 成功。
- transport interrupted：runtime 收敛为可恢复 interruption。
- history replay 中断：不提交半完成的 placement 批次；已经按稳定 ID落盘的 canonical item仍可读取。
- MCP 重建失败：保留旧 timeline 和 sessionId，返回结构化配置错误，不发送 prompt。

### 13.4 eviction close 失败

- 不从 registry 静默删除仍可能 active 的 session。
- 标记 `DetachingFailed` 或 Failed，并记录诊断。
- capacity scanner 继续处理其他候选，单个失败不阻断全局清理。

## 14. 可观测性

现有 raw、diagnostics 和 runtime log 足以承载本方案，不新增包含 prompt 内容的生产日志。

新增低频结构化事件建议：

```text
acp_session_runtime_attached
acp_session_runtime_reused
acp_session_runtime_evicted
acp_connection_evicted
acp_freshness_probe
acp_reload_decision
acp_provider_history_replay_summary
acp_timeline_compacted
```

字段仅包含：

- provider/connection key 的非敏感标识
- session/attempt locator
- reload reason code
- connection generation
- replay observed/imported/unchanged 数量
- timeline bytes before/after
- compaction duration
- pool/session active/idle 数量

不得记录 prompt、tool input/output、MCP secret/env value、完整 raw JSON。

建议 metrics：

```text
attached_session_count
idle_session_count
adapter_connection_count
session_runtime_reuse_count
session_reload_count{reason}
freshness_probe_count{result}
provider_replay_items{decision}
timeline_patch_append_count
timeline_patch_skip_count
timeline_compaction_count
timeline_bytes_before_after
```

## 15. 实施阶段

### 阶段一：TimelineStore

目标：先消除 O(N²) 存储增长，保持现有 prompt/runtime 生命周期不变。

- 抽出统一 TimelineStore。
- 增加 semantic fingerprint 和 unchanged skip。
- Provider replay、placement patch、streaming patch 接入统一 upsert。
- 增加 compaction。
- 修复 task-061 一类现有文件：达到阈值后在下一次 load/close 时自动 compact，raw 不修改。

### 阶段二：Session runtime 拆分

目标：从单次 `AcpRuntime` 中拆出长生命周期 session runtime。

- 保留 AdapterConnectionManager。
- 新增 AcpSessionRuntimeRegistry。
- route/event pump 迁移到 session runtime。
- PromptRun 仅拥有单次 prompt 状态。
- 连续 prompt 不再 shutdown runtime。

### 阶段三：有界资源管理

- 配置接入 `configs/app-config.toml`。
- foreground lease。
- session TTL/LRU。
- connection TTL/LRU。
- bounded close 和 shutdown 集成。

### 阶段四：Freshness 与配置指纹

- 抽出 session/list freshness probe。
- 持久化/维护 last known Provider revision。
- 实现 SessionConfigFingerprint。
- reload decision matrix。
- MCP 变化下一次 prompt 生效。

### 阶段五：统一三种运行模式

- Direct 后续追问改走 SessionPromptDispatcher。
- Workflow/AUTO 手动追问改走同一 dispatcher。
- runtime continue/repair 接入同一 registry，但按 origin 控制 freshness probe。
- 删除旧的“每次 prompt 创建并 shutdown AcpRuntime”消费路径，不保留并行 fallback。

## 16. 单元测试与接口验收

### 16.1 TimelineStore

- 同一稳定 ID、canonical 内容不变：JSONL 行数不增加。
- replay 仅 seq/timestamp 变化：不追加 patch。
- assistant streaming 内容增长：按 throttle 写入，终态必写。
- historyPlacement 不变：不追加；变化：只追加一个 revision。
- permission/elicitation 状态变化正确写入。
- compaction 前后 `load_timeline_items()` 完全等价。
- compaction 中断继续读取旧文件。
- task-061 重复 ID 文件 compact 后只保留 canonical item，展示顺序不变。

### 16.2 Session runtime

- 同一 session 连续两次 user prompt：一次 session load/new，两次 session/prompt。
- Workflow/AUTO 手动追问与 Direct 使用相同 registry。
- runtime repair 不重复 load。
- route 在 idle attached 期间持续消费，不发生队列阻塞。
- prompt active 时 TTL/LRU 不驱逐。
- permission/elicitation pending 时不驱逐。
- idle session 超时后 bounded close，下一次 prompt load 一次。

### 16.3 Freshness

- revision 相同：不 reload。
- revision 变化：先 reload/import，再 prompt。
- post-prompt baseline 更新成功：下一轮不会把自己的写入误判为外部变化。
- baseline Unknown 后探测恢复：reload 一次。
- Provider Unsupported：attached 直接 prompt，detached load。
- session/list pagination 能找到非第一页 session。
- freshness probe 超时不会导致无限 reload。

### 16.4 配置变化

- MCP 顺序变化但内容一致：fingerprint 相同。
- 新增/删除/修改 MCP：fingerprint 变化并 reload。
- reload 请求携带最新 mcpServers。
- model/permission 更新不触发 session reload。
- adapter config signature 变化导致 connection generation 更新和 session reattach。

### 16.5 容量

- idle session 数超过上限时按 LRU 驱逐。
- active session 可临时超过上限，完成后收敛。
- idle connection 超过上限时仅驱逐无 attached session 的 connection。
- app close 对所有 connection/session 执行现有 bounded drain/close。

## 17. 验收标准

使用 task-061 或等价长会话进行验收：

1. 连续发送 10 次 prompt，第一轮之后不再出现 `session/load`。
2. 每轮 prompt 前可观察到 freshness probe；revision 未变化时直接 prompt。
3. 在外部 Claude Code 中追加对话后，下一次 Gold Band prompt 前检测到 revision 变化并 reload。
4. 外部新增历史正确插入 timeline，已有稳定 ID 不重复增加 patch。
5. 新增 MCP 后下一次 prompt 发生配置指纹 reload，并能调用新 MCP。
6. `acp.timeline.jsonl` 的增长与新增/变化 item 数同阶，不随完整历史 replay 重复增长。
7. session idle 超过 TTL 或容量后被释放；重新发送时只付出一次 reload。
8. Direct、Workflow、AUTO 手动追问行为一致。
9. raw 文件仍保留完整审计帧并按既有容量滚动。

## 18. 最终设计结论

本方案的核心不是“永远不 reload”，而是将 reload 从每次 prompt 的固定步骤改成有明确依据的 session reattach/sync 操作：

```text
连续对话：长连接直接 prompt
Provider revision 变化：reload
本地 session 配置变化：reload
session/connection 被有界驱逐或失效：reload
无 updatedAt 的 Provider：attached 直接 prompt，detached 才 reload
```

同时，Provider 完整 replay 只允许产生实际新增或变化的 timeline 写入，并通过 compaction 把 timeline 文件长期维持在 O(唯一消息数) 的规模。

## 19. 实施记录（2026-07-25）

本轮已完成方案的核心实现：

- 新增 `TimelineStore`，统一执行 canonical merge、语义指纹去重、外部文件变更检测与原子 compaction。相同稳定 ID 仅发生 replay `seq/timestamp` 变化时不再追加 patch；内容、状态和 `historyPlacement` 的真实变化仍追加 revision。
- `acp.timeline.jsonl` 在文件大小超过 `acpTimelineCompactMaxSizeBytes`、patch 数超过唯一 item 数的 `acpTimelineCompactPatchRatio` 倍，或打开旧文件时检测到语义完全相同的重复 revision 时，原子改写为每个稳定 ID 一条 canonical item；因此 task-061 一类既有重复 replay 会在下次读取时自动收敛，`acp.raw.jsonl` 不参与压缩。
- 新增有界 ACP session runtime registry。第一次 `session/new/load` 后保留 session route，并由独立 event pump 持续消费 connection route；下一次同 attempt prompt 可以直接复用 attachment，不再固定执行 `session/load`。
- session runtime 以 attempt 目录为稳定身份，并使用 per-attempt prompt lock 串行化同一 session 的 prompt；Direct、Workflow/AUTO 手动追问、runtime continue 与 AI-DYNAMIC leaf 因为共用 `client::run_prompt`，统一进入该 registry。
- AdapterConnection 增加 connection generation 与最后活动时间；adapter 配置变化或 transport 重建后 generation 变化，旧 session attachment 不会跨连接复用。
- 用户 prompt 前使用带超时、最多 8 页的 `session/list` freshness probe。`updatedAt` 作为 opaque revision：相同直接 prompt，变化先 reload；无 `updatedAt` 的 Provider 在 attached 状态降级为直接 prompt，detached 时正常 load；临时探测失败把 baseline 标记为 Unknown，恢复后 reload 一次。
- MCP/cwd 使用规范化 session config fingerprint；MCP 数组顺序和对象字段顺序不影响 fingerprint，增删改 MCP 会在下一次 prompt 前触发携带最新 `mcpServers` 的 reload。model/permission mode 不进入 fingerprint，继续使用 session config API。
- session runtime 和无 attachment 的 adapter connection 均按 TTL + LRU 有界回收；active prompt 与前台 lease 内 session 不参与驱逐。会话详情打开时通过配置化的 lease renew interval 续租。
- 所有策略值已进入 `configs/app-config.toml`，0 值回退默认值，renew interval 大于等于 lease TTL 时自动收敛到 TTL 的三分之一。
- 外部会话同步改为 Agent 级 `externalSessionSyncEnabled`，默认关闭，不进入 `configs/app-config.toml`。Agent 管理页与 `ManagedAgentConfig` 对齐，可编辑 adapter、`primaryAgentDir`、`compatibleAgentDirs` 与同步开关；关闭时不执行 revision freshness probe，detached load 的 Provider history replay 不写 timeline，当前实时回复、长连接和 TTL/LRU 不受影响。
- attached runtime 记录创建时的外部同步策略；首次 `false -> true` 设置 `syncRequired`，下一次 prompt 前直接强制 `session/load`，不依赖可能超时的 `session/list`。required sync 成功前禁止 post-prompt revision baseline 更新，load 失败也不回退新建 session。
- 修复 detached `session/load` 的回放结束竞态：RPC response 与 session event pump 是两条独立到达路径，response 返回时的一次 `try_recv` 不能证明历史已经排空。runtime 现在在 load 后和真实 prompt 前分别等待 replay 队列达到有界静默，静默前保持 `Replaying`；外部同步关闭时所有 replay content 只写 raw，开启时才导入 timeline。等待超过上限时本轮同步失败且不发送 prompt，避免未知历史 `agent_message_chunk` 被误判为实时输出。
- Agent 配置保存边界升级为 Provider 全局失效：跨 workspace 阻断 active prompt，detach 所有该 Provider 的 idle attachment，并关闭所有 Provider connection。

接口回归已覆盖：timeline replay 去重、内容变化 revision、compaction 前后投影等价、旧重复 revision 打开时自动压缩且投影不变、idle event pump、load response 后延迟到达的 replay agent chunk 仍保持抑制、MCP fingerprint 归一化与变更、Provider revision 判定矩阵、首次开启同步时即使 `session/list` 会超时仍优先 load、Provider 跨 workspace 连接筛选、配置边界，以及既有 ACP 全量 Rust 单元测试。

## 20. 能力驱动的 resume/load 恢复策略（2026-08-07）

旧实现把 detached session 的“恢复上下文”与“回放完整历史”统一交给 `session/load`，导致普通 continue 承担了不必要的 replay、history importer 与 quiet-drain。ACP v1 已将 `session/resume` 稳定为不回放历史的上下文恢复接口，因此恢复方法改由两类数据共同决定：恢复意图 `ContinueOnly | SyncHistory`，以及 `initialize.agentCapabilities` 中的 `sessionCapabilities.resume` / 顶层 `loadSession`。

决策矩阵：

| 恢复条件 | 方法 |
| --- | --- |
| attached runtime 有效且配置指纹未变化 | 直接 `session/prompt` |
| 普通续接，支持 resume | `session/resume` |
| 普通续接，不支持 resume 但支持 load | `session/load` fallback |
| 需要跨端完整历史同步且支持 load | `session/load` |
| 需要历史同步但仅支持 resume | `acp.history-sync-unsupported` |
| strict continue 且两者均不支持 | `acp.session-restore-unsupported` |
| non-strict 且两者均不支持 | `session/new` |

生命周期拆分为 `RestoringWithoutReplay`、`ReplayingHistory`、`AwaitingTurnStart` 与 `Live`。resume 路径不启动 `ProviderHistoryReplay`、不等待 replay quiet barrier；恢复期间意外收到的 content update 只保留 raw 并写 `acp.unexpected-resume-replay` 结构化诊断。load 路径保留现有 importer、response 后 drain 与 prompt 前二次 drain；虽然 ACP 规范要求 load 在响应前完成回放，这层 quiet barrier 仍用于保护已经观测到的异步/不合规 adapter。

resume/load 请求都携带 `sessionId`、`cwd`、过滤后的 `mcpServers` 与可选 `_meta.systemPrompt.append`。成功响应先更新 `models/modes/configOptions`，再在同一个 session config transaction 内依次应用模型、权限模式和其他 config option，最后才发送 `session/prompt`。Tauri command 层识别 `RuntimeError` 并只向前端返回结构化 `code + params`；Raw Frame 过滤器分别显示 Resume 与 Load，系统提示解析和 close fuse 生命周期边界同时识别 `session/resume`。

接口回归覆盖 capability 解析、六类恢复决策、resume/load 参数、resume 无 replay 状态、load 延迟 replay 抑制、structured command error、resume 系统提示解析、close fuse 清除以及 Raw Frame i18n 映射。
