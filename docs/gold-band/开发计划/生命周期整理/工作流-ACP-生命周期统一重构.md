# 工作流 / ACP 生命周期统一重构方案

## 背景与问题

当前问题不是单点 UI bug，而是 runtime、ACP 会话、AI-DYNAMIC 内部虚拟节点之间生命周期边界不一致导致的系统性问题。

1. 普通节点停止后继续发送可用，是因为最终会走 `run_continue -> drive_from_node_with_initial_session`，runtime 会重新接管节点推进。
2. AI-DYNAMIC 内部节点停止后发送卡在“发送中...”，是因为 dynamic inner `send_acp_prompt` 直接调用 `client::run_prompt`，只续写 ACP 会话，不会重启 `drive_dynamic_graph`，因此动态图仍然是 Paused，回复也不会进入 `DynamicNodeCompletion` 解析、proposal 校验和后续节点物化流程。
3. ACP 输出结束后输入框状态空白、session tree 仍是蓝点，是因为后端 lifecycle 仍认为 runtime active，但前端 composer 根据 ACP completed 本地 suppress runtime active，导致同一个事实在树和 composer 中被两套规则解释。
4. `observability_bus` 和 `intervention_notifier` 也是同类问题：一个是 workflow event 总线，一个是专用干预通知回调，副作用触发通道分裂。

目标是做破坏式但正确的重构：生命周期规则必须收敛为后端单一规则源，后端统一决定“当前 attempt 是否可发、发到哪里、显示什么运行态”；前端只渲染后端 lifecycle/composer 和极少量 optimistic in-flight 覆盖。现有前后端双轨推导、dynamic/regular 分叉重复、已经被新入口替代的多余代码要同步废弃删除，不保留兼容层。

## 核心原则

- 后端 lifecycle/composer 是唯一业务规则源。
- 前端不再根据 raw ACP status、runtime status、dynamic/regular 类型自行推导业务状态。
- AI-DYNAMIC 内部节点和普通节点必须通过统一 runtime 编排恢复，不允许 dynamic inner send 绕过 runtime。
- hook bus 只做副作用分发，不承载状态判断，不改变 runtime 控制流。
- 当前项目处于开发阶段，优先删除旧入口、旧字段、旧分支，不做兼容层和灰度逻辑。

## 本轮实施结果

本轮已按破坏式收敛方向完成第一阶段落地：

- `ConversationAttemptLifecycleVm` 已新增 `runtime.phase` 与 `composer` 决策层，`launching-next-node`、停止中、暂停输入继续、暂停按钮继续等状态都由后端派生。
- 会话态输入统一走 `submit_conversation_prompt`，前端不再在 `sendAcpPrompt` 与 `continueRun` 之间自行分叉；`process-interrupted/error-blocked` 的文本输入继续与 `waiting-for-user-input` 的按钮继续都指向 `runtime-continue`。
- AI-DYNAMIC 内部节点继续发送已改为 runtime 恢复：后端根据 outer locator + inner locator 校验 paused dynamic graph，只 re-arm 目标 dynamic node，并让它回到 `drive_dynamic_graph` 的 completion 解析、proposal 校验、materialize 和外层 workflow 后续推进链路。
- `ObservabilityBus` 已升级为 `RuntimeLifecycleBus`，metrics 与干预通知都改为 subscriber；`App.intervention_notifier` 专用回调已删除。
- 前端 composer 状态映射已改为消费后端 `lifecycle.composer`，只保留发送中、停止命令待确认、乐观消息等短暂本地 overlay；会话态的旧 `onContinue` 分支已删除。
- 后端 command 层已抽出 `AttemptLocator`，统一表达顶层 attempt 与 AI-DYNAMIC 内部 attempt，并集中处理 attempt dir、runtime current 匹配、dynamic outer/inner locator 等路径判断。
- 新旧 UI 的工作流 attempt composer / Round 详情继续发送已统一收敛到 `submit_conversation_prompt`；`send_acp_prompt` 保留为非 runtime 生命周期的窄入口，并在 paused/resumable/current workflow attempt 命中时拒绝直接 ACP prompt，防止绕过 runtime。
- `stop_active_session` 返回体与 ACP session update event 已附带最新 `lifecycle`，前端收到停止响应或 session update 时可立即更新 composer 和 session tree，不再只等待下一轮完整 run snapshot 收敛。
- 会话页的 lifecycle-only patch 已同步覆盖 `workflowGraph`：继续/停止命令即使只返回 lifecycle、不返回新 session payload，也会立即更新工作流查看抽屉中的 graph node/attempt 状态，避免 composer 已恢复运行但抽屉节点仍显示暂停。
- compact 用量栏已统一按 composer lifecycle active 展示运行态：`launching-next-node` 这类 ACP 已 terminal、runtime 仍 active 的阶段也显示旋转状态、当前用时、会话累计与 token 信息。

## 最终架构

### 1. 后端 lifecycle VM 增加 composer 决策层

在 `ConversationAttemptLifecycleVm` 基础上新增后端派生的 composer 字段，复用已有 `runtime`、`acp`、`runtime_display`、`continue_kind`。

建议结构：

- `runtime.phase`: `idle | launching-session | provider-running | finalizing-attempt | launching-next-node | paused | terminal`
- `composer.mode`: `normal | runtime-active | stopping | interrupted-input | paused-action | invalid-workflow | runtime-error | permission-blocked | submitting`
- `composer.submitTarget`: `acp-prompt | runtime-continue | permission-response | none`
- `composer.processingKind`: 现有 processing kind 加 `launching-next-node`
- `composer.statusKey` 或 `statusCode`: 例如 `conversation.runtime.launchingNextNode`
- `composer.canStop`、`composer.lockInput`、`composer.showContinueAction`

后端派生规则：

- runtime active 优先于已 completed 的 ACP 会话；如果 runtime 仍 active 且 ACP 已 terminal，则 composer 显示 `launching-next-node`。
- runtime terminal 时，抑制 stale ACP active。
- `paused + process-interrupted/error-blocked + resumable` 表示允许文本输入，但提交目标是 `runtime-continue`。
- `paused + waiting-for-user-input + resumable` 表示继续按钮，不是自由输入。
- ACP `cancelling/cancel-requested`、cancel marker 或 provider pid 未清理时进入 `stopping`。
- workflow invalid / runtime error 由后端给出 mode，前端不再自行猜测。

### 2. 生命周期 Hook Bus

将现有 `ObservabilityBus` 破坏式升级为 `RuntimeLifecycleBus` 或 `WorkflowLifecycleHookBus`，事件类型从 `WorkflowEvent` 扩展为 `RuntimeLifecycleEvent`。

调整：

- 删除 `App.intervention_notifier`、`with_intervention_notifier`、`notify_intervention` 和 orchestrator 里直接读取 `app.intervention_notifier` 的逻辑。
- metrics 改成 lifecycle bus 的 subscriber，继续消费 `NodeStarted/NodeCompleted`。
- intervention notification 改成 lifecycle bus 的 subscriber，匹配 `RunPaused/NodePaused` 且 reason 为 `WaitingForUserInput | ErrorBlocked | ProcessInterrupted` 时触发 `InterventionNotification`。
- lifecycle/session UI refresh 可以作为 subscriber 或统一 emit 触发点，但只能通知前端重新取/合并后端 lifecycle，不能在 subscriber 里重新定义业务状态。

边界：

- Hook bus 只做副作用分发：metrics、OS 通知、前端刷新通知、日志等。
- Hook subscriber 不允许改变 runtime 控制流，不允许决定 composer mode/submitTarget，不允许修正状态文件。
- 生命周期事实仍以 `RunState/NodeState/DynamicGraphState + derive_conversation_attempt_lifecycle` 为唯一权威。
- 事件 payload 必须携带统一 Attempt Locator、status/outcome/pause_reason、node label、attempt dir 等通用字段，避免 subscriber 回头散落读取不同路径。
- subscriber 中的重活应异步派发或保证失败不影响 runtime；任何 subscriber panic/失败都不能影响编排。

#### 2.1 Hook Bus 设计模式

采用 Domain Event / Observer 模式，而不是一组散落 callback。

runtime 只发布已经发生的生命周期事实，例如：

- `RunPaused`
- `NodeStarted`
- `NodeCompleted`
- `AttemptStopped`
- `DynamicGraphPaused`
- `DynamicNodeResumed`
- `AcpSessionUpdated`
- `RuntimeAdvancingNextNode`

subscriber 只消费事实并执行副作用：

- `MetricsSubscriber`：把 `NodeStarted/NodeCompleted` 转成节点指标。
- `InterventionNotificationSubscriber`：把 `RunPaused/NodePaused + pauseReason` 转成系统通知。
- `UiLifecycleRefreshSubscriber`：通知前端重新拉取或合并后端 lifecycle。
- `AuditLogSubscriber`：记录审计或调试日志。

后续迭代的规则：

- 如果新增的是同一种语义事件的新触发点，只需要在新位置 emit 同类事件，subscriber 不需要改逻辑。
- 如果新增的是全新业务语义，才新增 event kind 或 subscriber policy。
- 不允许把不同语义硬塞进同一个事件，只为了复用 subscriber。

推荐事件 envelope：

```text
RuntimeLifecycleEvent {
  eventId
  eventKind
  occurredAt
  locator
  runtimeStatus
  outcome
  pauseReason
  phase
  nodeLabel
  attemptDir
  metadata
}
```

事件 payload 应直接携带 subscriber 判断所需的核心状态快照；subscriber 可以补读 token 等重数据，但不应该为了判断业务语义再去散落读取 runtime/dynamic/acp 文件。

#### 2.2 异步策略

Hook bus 默认异步。`emit(event)` 对 runtime 主流程必须足够轻量，不能在编排线程中执行 HTTP 上报、OS toast、磁盘重活等副作用。

推荐语义：

- best-effort delivery
- at-least-once 倾向，subscriber 自己通过 `eventId` / `dedupKey` / locator 做幂等
- subscriber panic/失败不影响 runtime，也不影响其他 subscriber
- 进程崩溃时允许丢失未消费事件，不做持久化消息队列

metrics 和 notification 当前都属于异步 subscriber：

- metrics 需要配置读取、token 读取和 HTTP 请求，不能阻塞工作流推进。
- notification 需要 OS toast 调用，也不能阻塞 runtime，失败只记录 warn。

不建议同时设计完整的同步 bus 和异步 bus，避免重新形成双轨。可以在一个 bus 内保留 subscriber 执行策略字段：

```text
SubscriberMode::Async
SubscriberMode::Inline
```

默认全部使用 `Async`。`Inline` 只允许用于纯内存、极快、无 IO、不改变状态、确实需要严格顺序的内部观察逻辑；当前 metrics 和 notification 都不应使用 `Inline`。

#### 2.3 生态工具选型

不引入 Kafka / NATS / RabbitMQ / event-sourcing 框架，也不引入维护不确定的第三方 Rust event bus crate。当前场景是桌面端进程内 lifecycle hook，使用已有成熟基础设施即可。

推荐组合：

- 事件分发：`tokio::sync::broadcast` 或 `tokio::sync::mpsc`
- 异步执行：`tokio::spawn`
- 日志：现有 `tracing`
- metrics HTTP：现有 `reqwest`
- 系统通知：现有 Windows `tauri-winrt-notification` 与 macOS/Linux `notify-rust`

优先方案：

```text
RuntimeLifecycleBus
  -> tokio::sync::broadcast::Sender<RuntimeLifecycleEvent>
  -> 每个 subscriber 持有独立 receiver
  -> subscriber 内部 tokio::spawn 异步消费
```

`broadcast` 的优势是多个 subscriber 都能收到同一事件，并且慢 subscriber 的 lag 可被检测。队列语义是内存型、best-effort；队列满或 lag 时记录 warn，subscriber 通过幂等键处理重复或重放风险。

只有当后续发现某类 subscriber 需要独立背压或不同丢弃策略时，再在 subscriber 内部接一层 bounded `mpsc`。

### 3. 统一 Attempt Locator

已新增 Rust 侧 `AttemptLocator`，供 command 层的 prompt submit、stop、session update emit 与 lifecycle 查询复用。

两类 attempt：

- 顶层 attempt：`taskId/runId/roundId/nodeId/attemptId`
- AI-DYNAMIC 内部 attempt：同上 + `outerNodeId/outerAttemptId`

它负责集中处理：

- attempt 目录定位
- runtime current attempt 匹配
- 顶层 attempt 与 AI-DYNAMIC inner attempt 的 runtime node/attempt 映射
- stop / prompt submit / session update emit 的 locator 参数传递
- lifecycle 查询所需的 outer/inner 定位

后续若继续下沉到 app runtime 层，worker ref 与 ACP prompt bundle lookup 也应复用同一 locator 语义，不能再新增 parallel locator 结构。

### 4. 统一 prompt submit

已新增后端 command `submit_conversation_prompt`，作为会话态 composer 的唯一文本/按钮提交入口。

输入：

- project id
- unified attempt locator
- prompt
- prompt id
- attachment paths

输出：

- `kind`: `acp-session | runtime-continue-started | rejected`
- 可选 `session`
- 可选 `run`
- 可选 `lifecycle`

执行规则：

1. 后端根据当前 run 是否 paused/resumable 以及选中 locator 是否匹配当前 runtime attempt 判定 `runtime-continue`。
2. 顶层 attempt 的 `runtime-continue` 调用 `run_continue_background`。
3. AI-DYNAMIC 内部 attempt 的 `runtime-continue` 调用 `run_continue_dynamic_inner_background`，携带 outer node/attempt 与 inner node/attempt。
4. 其余场景降级为同会话 ACP prompt helper，只处理 runtime 未接管的普通追问。
5. 新 UI 会话页与旧 UI Round 详情的工作流 attempt 提交都只调用 `submit_conversation_prompt`，不再自行选择 `sendAcpPrompt` 或 `continueRun`。

`send_acp_prompt` 仅保留为非 runtime 生命周期会话的窄入口；若请求命中 paused/resumable/current workflow attempt，后端返回 `acp.runtime-submit-required`，要求调用 `submit_conversation_prompt`，避免再次绕过 runtime。

### 5. 同会话 ACP prompt helper

从 `send_acp_prompt` 中抽出 app 层 helper，统一普通节点和 dynamic inner 节点的直接 ACP prompt 场景。

复用现有函数：

- `App::acp_prompt_bundle_for_attempt`
- `App::dynamic_acp_prompt_bundle_for_attempt`
- `App::acp_live_update_for`
- `App::acp_session_update_for`
- `gold_band::provider::resolve_attachments`
- `gold_band::acp::client::run_prompt`
- `acp_session_vm`
- `dynamic_acp_session_vm`

这个 helper 只处理“runtime 未接管、允许普通同会话聊天”的场景；如果 attempt 是 paused/resumable，就必须走 runtime continue。

### 6. AI-DYNAMIC 内部节点精确 resume

现有 `run_continue` 只能从 outer current attempt 继续，且 `execute_ai_dynamic_node` 里对 paused dynamic graph 的恢复会把所有 paused dynamic node 都设为 Ready。这对内部节点继续发送不够精确。

引入内部 resume plan，例如：

- `RuntimeResumePlan::TopLevelWorker { continueRef, prompt, promptId, attachmentPaths }`
- `RuntimeResumePlan::DynamicInner { targetNodeId, targetAttemptId, prompt, promptId, attachmentPaths }`

动态内部 resume 行为：

1. 校验 outer run 当前 paused，当前 outer node 是对应 AI-DYNAMIC node。
2. 加载 dynamic graph。
3. 校验 target dynamic node 存在、attempt id 匹配、状态 paused/continuable。
4. 将 `graph.run.status` 从 Paused 设为 Running，清理 pause reason/outcome。
5. 只 re-arm 目标 dynamic node；不要无差别恢复所有 paused node。
6. 使用用户输入作为 visible resume prompt，继续该 target worker。
7. 回到现有动态图主流程：`execute_dynamic_worker -> finalize_dynamic_worker_result -> build_dynamic_completion_from_artifact -> proposal validation -> materialize_dynamic_next -> refresh_dynamic_ready_nodes -> drive_dynamic_graph`。

本轮已实现 `DynamicResumeOverride`：`execute_ai_dynamic_node` 在 graph paused 时只恢复指定 inner node，`execute_dynamic_worker` 对匹配 override 的节点强制使用 `SessionMode::Continue`、复用保存的 ACP continue ref，并将用户输入、prompt id 与附件作为本轮 visible resume prompt 传入 provider。dynamic inner send 不再直接调用 `client::run_prompt` 后返回 session VM，而是回到 `drive_dynamic_graph`；若内部图完成，外层 `drive_from_node_with_initial_session` 继续执行原有控制决策并推进后续 workflow 节点。

### 7. 停止后同步 lifecycle

保持 `stop_active_session` 是统一停止入口，继续复用：

- `pause_attempt_runtime_state`
- `pause_dynamic_attempt_runtime_state`
- `spawn_acp_cancel_shutdown`

停止成功后 `stop_active_session` 返回体与 `AcpSessionUpdatedEventVm` 都携带最新 `lifecycle/composer`。前端接收停止响应时直接覆盖当前 composer lifecycle，接收 session update 时同步 patch selected/background leaf 与 activeSessions；完整 run snapshot 仍用于最终校准，但不再是 stop 后状态收敛的唯一通道。

### 8. 前端 composer 只渲染后端 lifecycle

要求：

- 移除或大幅削弱 `suppressStaleRuntimeActive`；这种业务判断移到后端。
- `deriveAcpRuntimeComposerState` 不再保留独立业务规则，只做后端 `lifecycle.composer` 到 UI props 的映射。
- local `sending/waitingForOptimisticPrompt/stopCommandPending` 只作为命令进行中的 optimistic overlay。
- 删除前端自有 runtime/acp 生命周期判断分支，例如 stale runtime suppress、dynamic/regular send 选择、根据 raw ACP status 推断可继续性等。
- `ACPChatDialog` 不再根据 `submitTarget` 在 `sendAcpPrompt` 和 `continueRun` 间自行分叉，统一调用 `submit_conversation_prompt`。
- 增加 i18n：`conversation.runtime.launchingNextNode = 拉起下一节点中...`。
- session tree 的 active dot 和 composer 使用同一个后端 lifecycle，不再出现树是 active、composer 空白的状态。

## 关键修改文件

- `src/app/observability.rs`
- `src/app/mod.rs`
- `src/app/orchestrator.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/commands_conversation.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/metrics.rs`
- `src-tauri/src/notifications.rs`
- `src-tauri/src/view_models_conversation.rs`
- `src-tauri/src/main.rs`
- `web/src/types.ts`
- `web/src/api/client.ts`
- `web/src/api/desktop.ts`
- `web/src/api/browser.ts`
- `web/src/components/acp/ACPChatDialog.tsx`
- `web/src/lib/acp-runtime-composer-state.ts`
- `web/src/lib/conversation-run-snapshot.ts`
- `web/src/pages/ConversationRunPage.tsx`
- `web/src/i18n.ts`

## 废弃清单

- 前端自推导 runtime/acp 生命周期业务规则。
- 前端 dynamic/regular send 分支。
- dynamic inner `send_acp_prompt -> client::run_prompt` 绕过 runtime 的路径。
- 后端 command 层重复的 dynamic/regular prompt 分支。
- `App.intervention_notifier` 专用回调、`with_intervention_notifier`、`notify_intervention`。
- 被后端 lifecycle/composer 取代的兼容字段和临时状态补丁。

## 测试计划

后端测试：

- lifecycle hook bus 能同时分发给 metrics subscriber 和 intervention subscriber。
- `emit(event)` 不执行重 IO，metrics / notification 通过异步 subscriber 消费。
- subscriber panic/失败不影响 runtime emit，也不影响其他 subscriber。
- broadcast lag / 队列满时记录 warn，不阻塞 runtime 主流程。
- subscriber 可使用 `eventId` / `dedupKey` / locator 做幂等，通知重复触发仍被 dedup。
- `RunPaused/NodePaused` 类事件能触发 intervention notification，非干预 pause reason 不触发。
- `clone_for_background` 传播同一个 lifecycle hook bus，不再传播独立 notifier。
- runtime running + ACP completed => composer active + `launching-next-node`。
- dynamic paused + ACP cancelled + `process-interrupted` + resumable => `submitTarget = runtime-continue`。
- runtime terminal => 抑制 stale ACP active。
- ACP cancelling/cancel marker/provider pid => `stopping`。
- paused dynamic graph 只恢复指定 dynamic node。
- dynamic resume 使用用户 prompt 作为 visible resume prompt。
- dynamic resume 会进入 proposal 解析/物化，而不是只写 ACP session metadata。
- 非目标 paused dynamic node 不被误恢复。

前端测试：

- `launching-next-node` 会显示状态。
- interrupted input 使用 `runtime-continue`。
- stopping 期间锁输入直到后端 lifecycle/session 解除。
- terminal completed session 不保留 runtime-active 假状态。
- composer 状态映射只消费后端 lifecycle/composer，不保留独立业务推导规则。

本轮已执行：

- `cargo test -p gold-band --lib dynamic_inner_resume_only_rearms_target_node`
- `cargo test -p gold-band --lib dynamic`
- `cargo test -p gold-band --lib observability`
- `cargo test -p gold-band-desktop --bin gold-band-desktop submit_conversation_prompt`
- `cargo test -p gold-band-desktop --bin gold-band-desktop waiting_for_user_input_pause_is_action_continue`
- `npm run web:test -- acp-runtime-composer-state conversation-runtime-workflow`
- `npm run web:build`

说明：完整 `cargo test -p gold-band` 仍受既有 `tests/entity_uuid_test.rs` 中 `LastExecutedNode` 字段不匹配阻塞，该问题不是本轮 lifecycle 重构引入。

## 人工页面验证清单

实施完成后，在页面上按以下路径人工验证：

1. 普通工作流停止/继续：进入会话态任务详情，启动一个常规 Worker 节点；在 ACP 输出中途点击 composer 停止；停止完成后在输入框输入新消息并发送；确认输入框不会长期停在“发送中...”，节点继续输出，后续工作流节点正常推进。
2. AI-DYNAMIC 内部节点停止/继续：进入包含 AI-DYNAMIC 的任务，展开 session tree 选择内部节点（例如 `bootstrap`）；在内部节点输出中途点击停止；停止完成后直接在该内部节点会话输入新消息；确认 dynamic graph 被恢复，内部节点继续输出并在结束后继续生成/推进后续 dynamic 节点。
3. 下一节点拉起状态：在一个有连续节点的工作流中等待当前 ACP 输出结束；观察下一节点尚未出现输出的短暂窗口；确认 composer 显示“拉起下一节点中...”，session tree 蓝点与 composer 状态一致。
4. 终态清理：工作流全部完成后，确认 composer 不再显示运行中/拉起中，session tree 蓝点消失或转为终态，不因旧 ACP completed/running 文件残留显示 active。
5. 干预通知 hook：触发需要人工介入的暂停（权限请求、错误阻塞或手动停止后的可继续态）；确认系统通知仍出现且点击“查看详情”能跳转到对应任务/节点；重复触发同一 dedup key 不重复弹。
6. 指标 hook：如果本地启用了节点指标上报配置，跑一个包含普通节点和 AI-DYNAMIC 的流程；确认节点开始/完成指标仍能产生，AI-DYNAMIC 内部 worker 不重复生成外层开始/结束哨兵。
7. 前端规则收敛：在普通节点、AI-DYNAMIC 内部节点、暂停态、停止中、终态之间切换选中会话；确认 composer 行为只跟随后端状态变化，不出现同一节点左侧树 active 但输入框无状态的分裂表现。

## 实施顺序

1. 后端新增/扩展 lifecycle composer VM，并加 VM 单元测试。
2. 将 `ObservabilityBus` 升级为 lifecycle hook bus，把 metrics 和 intervention notification 都改成 subscriber，并删除 `intervention_notifier` 专用回调。
3. 引入 Attempt Locator，先用于 lifecycle/stop/session lookup，降低后续 command 分支复杂度。
4. 抽出同会话 ACP prompt helper。
5. 新增 `submit_conversation_prompt` command，并让普通 acp prompt 走新入口。
6. 实现 dynamic inner exact resume，让 paused dynamic node send 走 runtime。
7. 更新前端 API 和 `ACPChatDialog`，移除业务分叉和 stale suppress。
8. 删除被后端 lifecycle/composer 取代的旧前端推导代码、旧 dynamic/regular 提交分支和后端重复 command 分支。
9. 更新 i18n、session update 合并逻辑和前端测试。
10. 同步产品设计文档与开发计划。
11. 跑自动化测试并保证通过。

## 不采用的修复方式

- 不在前端为 dynamic 节点继续追加 if/else 或超时重置 `sending`。
- 不继续让 dynamic inner `send_acp_prompt` 直接调用 `client::run_prompt`。
- 不通过简单删除 `suppressStaleRuntimeActive` 解决空白状态；真正的状态归属要移到后端。
- 不保留 lifecycle 双轨、prompt submit 双轨或 intervention 通知专用回调。
