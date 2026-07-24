# 会话式运行时

## 信息架构

会话运行时窗口是用户与 agent 交互的核心区域。左侧选中最小单位是 run，右侧主区域永远展示当前选中 session 的具体对话。

运行态身份以 `projectId + taskId + runId + session locator` 为后端操作定位；前端 ACP 消息窗口、乐观事件和事件分页缓存必须额外使用 task 生命周期 namespace（优先 `TaskState.uuid`）隔离。`taskId/runId/roundId/nodeId/attemptId` 是目录内可复用编号，用户删除最高编号 task 后重新创建会再次出现同一组编号，因此不能单独作为 UI 内存缓存身份。会话模式中查看、继续、停止、权限响应、模型/权限配置、raw frames、产物/附件读取都必须作用在该 `projectId` 对应 workspace；查看历史 run 不提升最后活跃 workspace。只有成功创建或重跑产生新 run 后，该 `projectId` 才成为最后活跃 workspace，并在从会话模式切回工作台时同步为旧 UI 当前 workspace。

## 运行时数据与内存边界

- `acp.timeline.jsonl` 是会话展示事件的完整事实源；活动 runtime 内存只保存当前 text/thought/plan 累计流、未终态 tool call、未决 permission/elicitation、session metadata、usage 与 timing aggregate。完成并已持久化的历史事件必须立即从热状态释放，不能按完整会话历史常驻 `HashMap`。
- timeline 使用既有 item/patch 格式持续追加，旧文件继续可读；运行期不再依赖从全量内存历史重写 timeline。`AcpTimingState` 按交互 ID 增量观察 permission/elicitation 终态，不能为一次交互复制整份 timeline。
- 会话树刷新只读取 attempt 的 session metadata 与 lifecycle 摘要；仅当前选中会话读取完整 timeline 事件页。非选中会话的超大或不可读 timeline 不得阻断 run/session tree 刷新；选中会话仍保持既有事件窗口配置、cursor 和终态覆盖语义。
- 每个 ACP session route 是无损 FIFO 有界队列：最多 4 MiB 且最多 256 帧；队列为空时允许一个超过 4 MiB 的合法单帧进入。达到任一上限后 producer 阻塞等待消费，不合并、不丢弃、不重排；receiver drop、route 注销或连接关闭必须唤醒等待者。
- 桌面进程共享单一 `RuntimeLifecycleBus`，metrics、notifications、conversation-run-state 使用固定具名幂等订阅并只在 setup 注册一次；创建、重跑、继续与 prompt 路径不得重复挂载订阅。
- AI-DYNAMIC 生命周期状态按领域分层持久化：每个 `DynamicNodeState` 自己持有结构化 `pauseReason` 与 `runtimeError`，`DynamicRunState.pauseReason` 只表示 graph 聚合结果。事件日志仅用于审计与诊断，不能作为 leaf 生命周期恢复的唯一事实源。
- 并行 leaf 中一个节点异常、其他 sibling 仍运行时，graph 保持 `running`，异常 leaf 立即持久化自己的暂停原因和完整错误链；最后一个 active sibling 结束后，graph 按暂停 leaf 原因优先级聚合，不能统一降级为 `process-interrupted`。优先级为 `error-blocked > runtime-abnormal > permission-requested > waiting-for-user-input > process-interrupted`。
- AI-DYNAMIC leaf 恢复时只清除目标 leaf 的 `pauseReason/runtimeError`，其他 paused sibling 的原因与错误必须保留。Conversation lifecycle、Session Switcher 与 workflow graph 优先读取 leaf 自身字段；仅对旧数据回退到 graph/run pause reason 或 ACP cancelled 快照。
- 同一 `provider + workspace` 复用 ACP adapter 进程时，session 的模型与权限设置组成 connection-scoped 原子配置事务。多个并行 session 不得交错写 adapter 的进程级配置文件；消息 prompt 仍可并行，锁的范围只覆盖 `model + permission` 配置序列。
- 未路由 ACP frame 的 runtime 日志只记录 connection/provider、sessionId、JSON-RPC method、sessionUpdate 类型、原始字节数与限频摘要，不记录 prompt、技能列表、工具输出或完整 JSON。`runtime.log` 活跃文件上限 8 MiB，保留 4 个轮转文件并继续遵守 30 天清理；`acp.raw.jsonl` 保持完整原始排障来源及既有轮转配置。
- 以上均为隐性稳定性约束：不得改变消息内容、事件顺序、工具详情、流式节奏、权限语义、页面交互、ViewModel/API JSON、工作流并行度或既有 `acpChatEventPageSize` 配置值。本边界不包含 WebView 自动恢复、自动重载或内存压力下降低并行度。

## 顶部信息栏

- 标题显示：可 inline edit，修改后同步到 task 和所有 run
- 标题修改后不再被自动覆盖
- 顶部运行标题栏采用紧凑单行高度，优先把垂直空间留给消息流
- run 标题字号低于文档页级标题，`runId` 作为弱化辅助信息跟随主标题同行展示，避免顶部两行标题过于突兀
- 顶部区域采用“单块双行”而不是两个分裂 header：第一行承载 run 标题与主操作，第二行承载 session 元信息；两行共用同一块 surface，仅在整个区块底部保留总边线
- 整体高度收敛优先通过两行共同压缩上下留白实现，不通过单独挤压第二行的行盒来制造紧凑感
- 继续收窄时优先轻压第一行的上下留白，并缩短两行之间的垂直缝；第二行文字本身保持稳定，避免 metadata 层被压得过碎

## 顶部操作栏

### 重跑按钮
- 常显，icon 为新建
- 当前 run 运行中：弹窗二次确认，确认后停止当前 run → 创建新 run
- 当前 run 已结束：直接创建新 run
- run 历史始终保留

### 编辑工作流
- WORKFLOW 模式下显示查看按钮（Eye 图标）和编辑按钮（Workflow 图标）
- AUTO / WORKFLOW 中选中 AI-DYNAMIC 内部 session 时也显示查看按钮（Eye 图标），查看该 AI-DYNAMIC attempt 生成的运行态工作流；暂不提供编辑入口
- 查看工作流：打开 Sheet，复用旧 UI 的运行态工作图组件与数据链路，展示当前选中 session 所在 round 的实际路径图
- 查看工作流中的节点状态、暂停/成功图标、产物数、附件数、agent 标识等信息应与旧 UI 保持一致
- AI-DYNAMIC 内部 session 的查看工作流图必须绑定 `outerNodeId/outerAttemptId`，run 结束后的终态刷新也不能退回外层 AI-DYNAMIC 容器图
- 查看工作流中的 AI-DYNAMIC 内部节点点击后应切换到对应内部 session；匹配顺序为 `outerNodeId/outerAttemptId + nodeId/attemptId`，普通工作流节点仍按顶层 `nodeId/attemptId` 匹配
- 查看工作流 Sheet 展示的是 `ConversationRunVm.workflowGraph`，它必须跟 session tree 使用同一份后端 lifecycle 事实；`submit_conversation_prompt` / `stop_active_session` 或 ACP session update 返回的 lifecycle 即使没有携带新的 session payload，也必须立即 patch 对应 graph node/attempt 的 `status / runtimeDisplay / current`，避免 composer 已继续运行但抽屉图仍显示暂停。
- 当当前选中 AI-DYNAMIC 内部 session 时，外层 AI-DYNAMIC 容器的 terminal/live refresh 只触发 run VM 刷新，不得覆盖当前 `selectedSessionKey`；刷新请求必须继续携带内部 session key
- AI-DYNAMIC 内部 leaf 完成、暂停或被聚合暂停后，后端必须发出该 leaf 的 session/lifecycle update，前端收到 terminal/interactive 状态后刷新完整 run VM；不能让选中 leaf 长时间停留在旧的 `launching-next-node` runtime-active 派生状态。
- AI-DYNAMIC dynamic graph 中任意 leaf 从不可见/待依赖状态变为 `Ready | Running`，或 graph 内部创建新的后继 leaf 后，都必须在 graph 持久化后发出该 leaf 的 lifecycle update；这条规则按通用 dynamic leaf 可见性处理，不按 merge / acceptance 等具体节点类型写特殊分支。
- AI-DYNAMIC worker 在用户停止与最终输出几乎同时发生时，ACP snapshot 可以保留 `cancelled` 事实；但如果后端已经拿到完整且通过 DSL/schema 校验的 `dynamic-node-completion`，业务层按 `Completed + Success` 接受并继续推进 graph。半截 JSON、非法 schema 或 rejected proposal 仍保持 `Paused + ProcessInterrupted`，等待用户继续。
- Windows 桌面端运行 workflow / ACP / AI-DYNAMIC 时，后台 Git worktree 命令、MCP stdio 健康检查、MCP stdio `tools/list` 和 shell fallback 都属于非交互子进程，必须通过统一进程工具以隐藏控制台窗口；运行态不向用户暴露 `git.exe` / `cmd.exe` / MCP helper 弹窗，避免桌面产品退回 terminal 心智。
- 左侧会话侧边栏的 run 终态刷新不能只依赖 ACP session terminal update。ACP update 继续负责 session/graph 实时状态；run 真正完成并持久化后，后端通过 runtime `RunCompleted` lifecycle 推送 run 级事件，前端统一刷新当前 run 和 sidebar，让 run 行从 running 空心点切到既有终态展示。
- workflow control 限制触发的终局失败同样属于 run 完成事实。`max_attempts`、`max_rounds` 等路径在写入 `workflow_control_limit_exceeded`、`run-progress.json` 与 terminal run state 后，必须复用 `RunCompleted(Failure)` lifecycle 推送，确保会话页从 `launching-next-node` 实时刷新为终态横幅，而不是依赖用户切换会话后重新加载。
- 编辑工作流：打开 Sheet，内嵌 WorkflowEditor 完整编辑器
- 修改只影响未来 run，不影响当前 run snapshot

## Session Switcher

- 位于会话窗口顶部信息展示区
- 显示路径如 `round-001/dev/attempt-002`
- 当前选中 session 的顶部 trigger 也显示同一枚状态标记，与下拉树中的 attempt 行保持一致
- 点击展开 round → node → attempt 层级树
- 用户可切换具体 session
- 每个 attempt 前仅显示轻量状态圆点，颜色只来自后端 `runtimeDisplay.tone`：绿色成功、红色失败/错误阻塞、黄色暂停、灰色待处理/未知；运行中使用主色圆点配外圈脉冲 halo
- 已选中的 session 行仍保留同一枚状态标记，不能因为选中高亮而丢失运行态/结果态识别
- `status / outcome / pauseReason` 只作为运行事实字段保留；Session Switcher、顶部选中栏、工作流查看 Sheet 不在前端自行推断成功/失败/暂停，而是统一消费后端派生的 `runtimeDisplay.code / tone / icon / terminal / resumable / reasonCode`
- `completed + outcome=null` 不展示为成功；成功必须来自 `outcome=success` 派生出的 `runtimeDisplay.tone=success`
- AI-DYNAMIC 内部节点的 session 状态来源于 dynamic graph 中的节点状态（`dynamic/nodes/<node-id>/node.json` 或 `graph.json.nodes`），ACP attempt 目录只代表聊天会话记录，不作为工作流节点成败状态来源；live running graph 中不得用旧的 `acp.session.json/acp.snapshot.json=cancelled` 反向覆盖 running sibling 的工作流状态，只有父级/dynamic graph 已暂停的历史坏状态才允许做 legacy recovery
- 当 runtime attempt 已因 `process-interrupted / runtime-abnormal / waiting-for-user-input` 进入可继续暂停时，session tree 与 composer 的用户态状态必须继续展示为可继续暂停；其中 `runtime-abnormal` 使用危险色/异常图标提醒，但 `blockingError=false` 且可输入继续。当 runtime attempt 因 `error-blocked` 暂停时，session tree 必须保留错误阻塞状态并由 composer 展示 `runtime-error`；此时 ACP snapshot/session 被写成 `failed` 或 `cancelled` 只代表底层会话传输已结束，不能覆盖 runtime 的暂停或错误事实
- 若 provider/ACP `failed/error` 先于 runtime 异常归一化结果到达，而当前 attempt 仍是 `paused + outcome=null + current` 且没有明确 `pauseReason`，Conversation VM 必须把该状态派生为 runtime 仍在收敛中的锁定态；不得短暂展示“当前会话运行失败”。待后端写出 `runtime-abnormal` 后，composer 再切换为可继续输入。
- ACP diagnostics 中的 `lastError` 可以作为顶部 banner、日志、详情和消息流诊断来源，用来告诉用户 provider/ACP 为什么失败；但它不能驱动 composer 进入 `runtime-error`，也不能覆盖 workflow runtime lifecycle。用户修复外部条件并继续后，如果同一会话产生了后续正常响应，banner 应按诊断可见性规则自然消失。
- 桌面客户端退出、workspace/provider 重载等主动关闭路径必须使用 ACP 两阶段有界关闭：连接先从 `open` 进入 `draining` 并立即停止接收新的普通请求；随后取消目标 session 的 permission、elicitation 和 prompt，允许已经在途的 JSON-RPC response 在 draining 阶段完成，并按 session 等待 active prompt 有界归零；最后才发送 `session/close` 并关闭 adapter transport。等待超时后可以强制关闭，但 draining/closed 导致的 transport 不可用必须归一化为 `interrupted`，不得落盘为 `ACP prompt failed` 或触发会话失败 banner。
- AI-DYNAMIC leaf 已持久化 `runtimeError` 时，当前选中 leaf 的错误展示优先使用该结构化错误的完整 diagnostic；不得只展示 outer provider context，也不得被 graph/run 的泛化暂停原因覆盖。
- 每个 attempt leaf 必须暴露真实 `artifactCount / attachmentCount`，计数来源与当前选中 session 底部资源条使用同一套后端资源列表规则；计数不能写死或由前端推断，避免 session tree 与资源条对同一 attempt 的文件事实不一致。

### 默认 session 选择
- 用户已有选中 session 且仍有效时保持
- 多个 session 默认最近 session；最近 session 必须按 session/attempt 的实际开始时间选择，不能按 workflow DSL 节点顺序选择最后一个节点
- run 结束时显示到达 end 状态的 session
- run 启动时必须先同步创建首个 `round/node/attempt` 的最小运行锚点并写入 `node.json`，再后台启动 agent/provider；`selectedSessionKey` 应能在首次 `getConversationRun` 中从当前 attempt 推导出来，不能依赖首个 ACP frame 到达后才出现。
- 没有显式 `selectedSessionKey` 时，默认 session 选择顺序为：当前 runtime attempt → active/running attempt → 最近 session；只有不存在运行中锚点时才回退到最新历史 session。前端可用 `activeSessions` 做短暂兜底，但该兜底只用于极短竞态，不作为主事实源。
- 新会话从会话式主页发起后，run 创建命令只负责落盘 task/run 初始状态并后台启动执行；前端收到该 run 的第一个 ACP live event 后必须立即刷新 session tree，插入对应 attempt，选中该 session，并把右侧详情切到该 session。后续同一 attempt 的普通流式消息由 ACP 会话详情订阅直接合并，不依赖整页轮询；后端应具备向前端推送完整 session snapshot 的基础通道，但当前自动 workflow 只在 run completed 完成态落盘后额外推送 terminal session snapshot，当前已选中 session 的 terminal session snapshot 仍必须触发 run VM 刷新，避免最后节点没有下一跳事件时父级 lifecycle 停留在 active。
- run 已进入 `running` 但首个 attempt 尚未出现在 session tree 前，右侧主区域显示 `Agent 调起中` 状态，不回退为“暂无活跃会话”。attempt 已出现在 session tree 但尚无可见 thought/text/tool timeline item 时，消息主区域显示 `处理中...`；收到首个 thought 后自然切换为 `思考中...`，避免创建 session 后到首 token 前出现空白。会话式运行页必须把当前 attempt 的外层 runtime status 传入 ACPChatDialog，不能只依赖 ACP snapshot/session status；当前选中 attempt 运行中时必须展示阶段状态、禁用输入并显示停止按钮，当前选中 attempt 已结束时必须恢复正常追问输入且不显示停止按钮。

### 会话元数据展示

会话窗口 header 中的模型名称/选择器、权限模式标签和系统提示词按钮依赖于完整的 `AcpSessionVm` 元数据（`config.currentModelId`、`config.currentModeId`、`systemPromptAppend`）。为保证这些信息在实时流式开始后即可见：

- **后端 session-ready 快照**：provider 在 ACP `session/new` 或 `session/load` 完成后，必须先把 Gold Band synthetic user prompt 写入 timeline，再写 `acp.snapshot.json` 并通过 `acp_session_update_emitter` 发送完整 `AcpSessionVm`，最后才开始真实 `session/prompt` 流式输出。首个可见 snapshot 必须同时具备 `systemPromptAppend`、模型/权限配置和首个用户消息，避免首屏先渲染 agent thinking。
- **系统提示词来源**：新 session 的 `systemPromptAppend` 属于 snapshot metadata，`acp.raw.jsonl` 只作为旧历史 session 的 fallback 和协议排障事实源；前端不直接解析 raw 来展示系统提示词。
- **前端初始化 readiness fetch**：前端订阅会话更新后只在初始化阶段调用 `getAcpSession` 等待 session-ready 快照；等待窗口必须覆盖 ACP provider 慢启动场景（例如 `initialize + session/new` 超过 10 秒），不能用 2 秒级短重试提前放弃。若 snapshot 已具备系统提示词、配置枚举和首个 Gold Band 用户消息，立即进入实时渲染。live event 到达时不得反复触发 metadata hydration；实时阶段缺 metadata 属于后端未按 session-ready 契约发送完整 `AcpSessionVm` 或初始化 readiness 等待窗口不足，应修复事实来源链路，而不是由前端事件流补拉掩盖。
- **event-only shell**：`createLiveAcpSessionShell` 只在调用方显式允许 event-only fallback 时创建临时渲染壳，不作为稳定元数据来源；壳中不含 system prompt 与 model/config 字段。会话式运行页必须关闭该 fallback：若继续会话启动早期只扫描到 timeline/live event、尚未拿到包含 `systemPromptAppend`、配置枚举和 Gold Band 用户消息的 ready session，主区域继续显示初始化 loading，而不是把 partial session 渲染成“无 session id / 无系统提示 / 无权限模型”的运行会话。
- **运行中重进显示**：若重新切回一个 running session 时已经存在 base `AcpSessionVm`，且当前 session event window 已包含可展示的 thought/text/tool/plan/permission/elicitation 等 timeline 事件，即使完整 metadata 或最早的 Gold Band synthetic user prompt 已不在当前分页窗口，也应退出 loading 并渲染当前消息流，同时停止 readiness 轮询。纯 metadata/config update 事件不得解除 loading gate，避免把尚未形成可见对话的 early session 渲染成空会话。
- **session leaf 归属**：`AcpSessionVm` 必须携带 `roundId/nodeId/attemptId` 与可选 `outerNodeId/outerAttemptId`，前端优先用这些稳定身份判断选中 session 是否属于当前 leaf；`cwd` 表示 Gold Band session attempt 存储目录，只作为旧数据 fallback。provider 真实工作目录单独放在 `providerCwd`，不能覆盖 `cwd`，否则 dynamic continue session 会被误判为不属于当前 leaf 并进入初始 loading。
- **authoritative session ref**：前端用于异步回调的最新 session ref 只能由明确数据入口维护（外部 session prop、identity 初始化、subscription session、initial fetch、permission/stop/model response、live-only timing patch）。`effective` / visible session / optimistic session 等 UI 派生态不得反写 ref，否则后端已推送的 session-ready snapshot 可能被旧 render 中的 event-only shell 覆盖。session payload reducer 不能仅因 ref 等价就跳过 React state 同步；ref 是异步事实缓存，`currentSession` 展示态必须用自身当前值再做一次等价判断，确保 UI 最终追上 session-ready payload。同一 ACP session 内，`systemPromptAppend`、模型/权限配置和 Gold Band synthetic user prompt 是 session-scoped metadata；后续 live timing、分页响应、空外部 prop 或 event-only shell 只能作为 patch 合入，不能把已就绪 metadata 降级为空。
- **可见事件合并**：base session 一旦存在，消息流必须按 `AcpSessionVm.events + loadedEvents` 合成可见窗口；`loadedEvents` 只是实时/分页事件窗口，不能直接替换 session snapshot 中的事件。Gold Band synthetic user prompt 可以只通过 session-ready snapshot 到达，因此前端必须保留 snapshot prompt 并继续合并后续 live event。
- **session 等价判断**：`sessionsEquivalent` 必须比较 session config 与 adapter 元数据签名，使后端在启动阶段发出的元数据-only session 快照（事件数可能没有变化）能刷新 UI。模型/权限栏只要存在可选项就应展示，不以 `currentModelId/currentModeId` 是否已归一化作为隐藏条件。

### 自动切换规则
- 上一个 session 完成 + 消息窗口在底部 → 自动切换并折叠历史
- 用户不在底部（正在看历史）→ 不自动切换、不折叠
- 用户通过 session tree 或工作流图入口手动切到任意 session 后，自动跟随立即解除；后续新 running session 只在后台推进，不抢占当前查看中的会话
- 当前选中 session 因 runtime 自然完成而从 active 变为 terminal 时，如果用户仍在底部且未手动切换，session auto-follow 进入 pending 状态；后续同一 run 的新 active child session 首次 live event 或 lifecycle-only active update 到达时可以切换过去。
- 自动跟随分为两层：消息列表的贴底 pin 控制当前 session 内流式内容是否滚到最新；session auto-follow 控制是否随 workflow 切到新的 active session。用户滚回当前活跃 session 底部时，恢复贴底 pin 并恢复 session auto-follow；用户滚回历史/非活跃 session 底部时，只恢复当前消息贴底，不切换 session。
- 用户手动查看历史 session 后，只有再次明确选中最新 active/current runtime leaf 并回到底部，才恢复 session auto-follow；仅把历史 session 滚到底部不能恢复 auto，也不能让后续 background active session 抢焦点。
- 顶部运行中节点 chip 是显式“跟随当前活跃 session”入口：点击 active chip 且消息窗口位于底部时，重新进入自动跟随；live event 到达或完整 run VM 刷新不能单独恢复自动跟随
- 刷新 run VM 时若未满足自动跟随条件，前端必须继续保留当前 `selectedSessionKey` 与当前 session payload，不能因为其他 session 的 live event 或后端默认 selected key 回退到最新 running attempt；若手动切换与已排队的 live refresh 同时发生，仍以最新手动选择为准
- 会话页内“进入 run 时重置自动跟随”的前端 effect 只能绑定 `runId` 等稳定 run 身份，不能依赖父组件每次重建的回调引用；否则 live refresh 触发父组件重渲染后会误把手动关闭的自动跟随重新打开
- 手动切换后是否恢复自动跟随，必须以 `run.activeSessions` 中是否仍包含当前选中 session 为准，不能仅依赖该 leaf 自身的 `runtimeDisplay.tone`，避免树状态短暂不一致时把已完成 session 误判成仍可跟随
- 前端所有完整 `ConversationRunVm` 快照进入 React state 时必须走统一合并入口，不允许调用点直接覆盖；合并入口负责保留当前 selected key、阻止 ACP `unknown` 空快照降级 runtime active 状态，并在 run 仍运行但 activeSessions 暂空时从 selected leaf 补出临时 active session。合并后 `selectedSessionKey` 与 `selectedSession / artifacts / attachments` 必须属于同一个 leaf；若 live refresh 或旧的手动切换请求返回了其他 session 的 payload，前端必须丢弃该 payload，而不是把它套到当前选中 key 上。用户通过 session tree 切换到目标 session 后，目标 `selectedSession` payload 回填前属于详情加载中状态，右侧主区域显示中性加载，不得短暂展示 ACP 会话失败横幅；若目标 leaf 的 runtime 仍 active 但 `selectedSession/effective session` 暂为空，也继续显示同一中性加载态，不展示内部 runtime 状态 key 或“拉起下一节点中”。只有目标 session 详情请求完成后仍确认没有 session/live shell 且 runtime 不再 active，才展示缺失 ACP session 错误。
- 只有一个 session 运行中 → 自动展开该 session
- 多个 session 运行中 → 显示折叠行（session 名 + 实时状态），用户点击进入

## Composer 附件

继续对话时可上传附件作为本轮输入内容：

- **入口**：纸夹按钮、拖拽、粘贴（统一走 same-session 附件模型）；桌面端必须在基础 Tauri 配置和 channel overlay 中关闭原生 WebView file-drop，让文件拖拽进入前端 HTML5 drop zone，拖入 composer 时稳定显示可投放状态
- **预览**：图片文件在 composer 内显示缩略图，点击可打开沉浸式大图预览；预览使用单层深色遮罩按合适尺寸展示原图，不支持缩放或拖拽，点击空白遮罩关闭
- **消息展示**：用户消息下方的图片附件显示为固定尺寸小缩略图，点击进入独立全屏原图预览，不进入附件详情弹窗；文本/代码附件继续显示为紧凑文件 chip 并走附件详情。base64/data URL 只作为内部图片数据承载，不直接作为可见文本展示。消息流附件预览必须按 timeline `raw.attachments[].path` 区分来源：`task-inputs/<name>` 属于新会话首轮 task 输入附件，继续读取 task 级 `authoring/inputs`；`user-inputs/<name>` 属于继续/追问本轮新附件，按当前 session locator 读取该 attempt 下的相对文件。两类附件不得混用读取入口，否则首轮需求附件或完成后追问附件会在 UI 中丢失内容。
- **传输**：新会话初始输入附件只进入 task 级 `authoring/inputs/`，并且只在 `SessionMode::New` 的首次 ACP session 初始化时作为 provider `task-inputs` content block 发送；同一个 ACP session 内的 `continue` / resume 不自动重发 task-level input attachments，避免历史输入在每轮用户消息下重复出现。发送前若附件来自粘贴、拖拽或浏览器 File 对象，前端先通过桌面命令 materialize 到 Gold Band 临时输入附件区，拿到本地路径后再进入对应输入链路。本轮 composer 显式选择的附件属于 resume prompt attachments，只随本轮 same-session prompt 发送。输入附件作为 ACP content block 发送给 agent，不混入 agent 输出产物目录。
- **AI-DYNAMIC**：AUTO / WORKFLOW 中的 AI-DYNAMIC 内部 worker、merge、acceptance 节点必须与普通 worker 复用同一 task input attachment 数据源；动态节点不得把 `input_attachment_paths` 清空，也不得要求 agent 主动扫描 run 目录寻找图片。

## Composer 状态

运行中的状态提示必须放在 composer 内，compact 模式下也不能只展示耗时或 token。当前步骤状态应展示具体文案：发送中、处理中、思考中、工具调用中、响应中、停止中、拉起下一节点中；会话式运行页的 compact 用量栏需在会话累计前展示带轻量旋转图标的状态标签，例如“思考中...”“工具调用中...”或“拉起下一节点中...”。这类状态是否展示运行态视觉取决于后端 composer active/lifecycle，而不只取决于 ACP session 是否仍 active；当 ACP 已 completed 但 runtime 仍处于 `launching-next-node` 时，compact 栏仍必须展示旋转状态、会话累计与 token 用量。ACP completed 只表示上一轮 turn 已结束，不表示该会话不能继续追问；用户发起新的 same-session ACP prompt 后，发送中、处理中属于前端本地 turn overlay，不得被旧 terminal snapshot 压掉。旋转标识应避免 SVG stroke 在高频刷新下掉帧，优先使用 CSS 边框圆环。Round 详情等非 compact 面板继续使用 composer 内状态行，不作为消息流卡片。

会话累计的口径是当前 ACP attempt 内所有 Gold Band prompt turn 的 agent 净处理耗时之和：每轮从 Gold Band synthetic/user prompt 写入 timeline 开始，到该轮最后一个可观察的处理事件结束；多次继续、恢复、余额错误重试或用户空闲造成的两轮 prompt 之间墙钟间隔不得计入。`available_commands_update`、`current_mode_update`、`session_info_update` 等会话元数据更新不推进处理耗时，`acp.snapshot.json.createdAt -> updatedAt` 只描述底层 ACP session 生命周期跨度，不能作为会话累计的 fallback。

## 系统通知

系统通知只用于用户可能没有看到当前会话页时的关键提醒，不替代会话内状态展示。

会触发系统通知的事件范围固定为：任务完成、Agent 单轮回复完成/失败、权限审批请求、ACP elicitation 提问、节点结束后请求人工判断是否成功、异常中断或错误阻塞。用户主动停止、会话内普通运行中、拉起下一节点中不触发系统通知；当前目标 session 在前台可见时继续抑制 OS 通知。

通知发送前必须判断桌面注意力状态：窗口未聚焦、窗口最小化、窗口不可见，或当前前端页面不是该事件对应的 run/session 时才发送；如果用户正聚焦在 Gold Band 并查看对应 `taskId/runId/roundId/nodeId/attemptId`，则只更新页面内 composer、session tree 和工作流图，不弹 OS 通知。

ACP 权限请求与 elicitation 提问都必须收敛到统一 intervention notification 机制：runtime 控制下的暂停由 lifecycle 事件触发通知，ACP live event 同时做旁路桥接补齐实时提醒。这样权限请求、elicitation、人工判断、异常中断和任务完成共享同一套去重、点击跳转和前台抑制规则。permission/elicitation 的 canonical event id 必须包含 ACP request id：同一请求的重复 live update 只通知一次，同一 attempt 或 Direct 长会话中的后续请求必须独立通知；elicitation 还必须与 `waiting-for-user-input` 的人工确认通知使用不同 kind suffix。通知展示身份不得暴露 `direct-agent` 等内部 node id：Direct 使用 conversation metadata 中的 Agent 名称，普通节点优先使用实际 provider/Agent 展示名，只有历史数据缺失时才回退 node id。

ACP elicitation 也复用同一条 session event / timeline 管道：`elicitationRequest` 与 `elicitationResponse` 虽然不直接作为普通消息卡片渲染，但必须保留在当前 session events 中，供 composer 底部的提问卡片推导 pending/answered 状态。已回答状态不能只依赖前端内存 Map，刷新或重进页面后必须能从 `elicitationResponse` 回放恢复。回答提交后交互卡片立即消失，不额外合成用户消息气泡；Agent 原生 `AskUserQuestion` 的 `toolCall/toolCallUpdate` 仍按普通工具卡片展示，并保留 completed 状态、关键参数和工具输出。

多问题 ElicitationCard 的已确认答案必须作为卡片内唯一事实源，当前步骤的单选、多选和自定义输入只属于可丢弃草稿。步骤前进时把当前题答案按字段原子替换进答案集；返回历史步骤时从答案集恢复选中状态；用户对历史步骤执行“跳过”时，必须同时删除该题主字段与自定义伴随字段，最终 `content` 只从清理后的答案集构建。不得分别维护“界面选中值”和“提交答案”而缺少双向同步，也不得让已经跳过或已经被预设选项替换的旧自定义值进入提交载荷。

ElicitationCard 属于高频表单卡片，不使用宽松的营销式留白。卡片本体、步骤指示器、题干、选项行和底部操作区应保持紧凑节奏：优先压缩上下 padding、控制项高度和区块间距，同时保留稳定点击面积与清晰层级，避免在会话消息流中形成过厚的大白块。

ElicitationCard 的单选、多选必须共享同一套选中语义：使用 `accent` 高层 surface、`accent-foreground` 边界与实心勾选标记，并增加轻量内描边强化整行状态；实心标记内部的勾线固定使用 `background` 反色，不得使用可能带透明度的 `accent` surface 色，确保四套主题均可辨识。不得用中性深色 `primary` 同时承担选中背景、边框和图标，否则在石墨深色与终端黑中会失去可辨识度。选中按钮同时暴露 `aria-pressed`，视觉状态与可访问状态保持一致。

多步骤提问的步骤指示器、题干 Markdown、选项行、可选跳过与提交按钮共用同一套 13px 级正文节奏，底部主操作按钮保持固定高度，避免消息流在等待用户决策时出现明显高度跳变。提交完成后不保留额外的“已确认回答”信息行或用户气泡，历史语义由 `AskUserQuestion` 工具卡片与结构化 timeline 事实承担。

## 流式渲染性能

- ACP 会话继续保持 `raw + timeline` 双层设计：`acp.raw.jsonl` 只作为协议排障事实源，主消息流只消费后端聚合后的 timeline item。
- ACP 会话累计属于会话级实时指标，不由前端从当前分页窗口临时重算，也不由前端基于本地时钟补算。后端 ACP runtime 维护 `AcpTimingState` 作为 timeline timing 的内存缓存，timeline / snapshot 才是重放事实；普通 `AcpUiEvent` 上的 `timing` patch 只表示该历史事件发生时的耗时锚点，不驱动 live-only timing 更新。当会话运行时，后端发送 live-only `timingUpdate` tick 作为实时显示通道，仅携带 timing patch，不进入 timeline。tick 调度必须按 prompt loop due time 检查，不能只依赖 `recv_timeout` 空闲分支；adapter 高频推送 text/thought/tool/usage 时仍需持续发送 timing tick。`AcpSessionVm.timing` / `acp.snapshot.json.timing` 保存刷新恢复锚点；活跃会话的 `getAcpSession` 必须优先返回 timeline 扫描出的当前净累计秒数，避免 stale snapshot 在切换会话、权限响应或刷新时短暂覆盖实时值；终态会话继续以 snapshot timing 作为持久化事实，但写入 terminal snapshot 时必须按 prompt 结束 / cancel 当前时刻结算当前 turn，不能因当前 turn 只有 live-only tick 而回退到上一条 timeline event。后端重放 timeline 时必须识别 compacted permission / elicitation 事件上的 `startedAt -> endedAt(timestamp)` 已闭合等待区间，即使 pending 事件已被压缩出当前窗口，也必须扣除用户等待时间；runtime 写入 permission / elicitation 边界事件或发送用户等待期间 live-only tick 前，也必须从当前 timeline item 集合重建 timing cache，避免阻塞等待和 upsert replacement 产生第二套实时计时。所有 timing 都携带 `revision` / `observedAt`：普通 timeline event 使用事件 seq，live-only tick 与 terminal snapshot 使用 runtime synthetic revision。前端 compact 用量栏优先显示后端 session snapshot 或 live-only `timingUpdate` 的 `timing.sessionElapsedSeconds`，缺少 timing 的旧会话才回退 `sessionElapsedSeconds`；同一 ACP session 的外部 session prop、identity 初始化、stop response、subscription session、final `getAcpSession` 与 live-only `timingUpdate` 等 payload 乱序到达时，前端按 timing revision 接受或拒绝 timing，同时继续接受较新 payload 的 status/events/metadata。live-only `timingUpdate` 必须脱离 text/thought/tool 消息流的 `startTransition` 与 interaction quiet window，先同步更新 compact 栏，再低优先级渲染消息流；metadata 不属于 live event 热路径，必须由 session-ready snapshot 提供。前端不再展示当前步骤耗时。权限等待、elicitation 等用户交互等待、用户空闲和 `available_commands_update` / `current_mode_update` / `session_info_update` 等 metadata update 不计入净处理耗时。
- 活跃会话 live update 不应按 token 级别驱动完整 React 渲染；文本、thought、plan 等高频更新需要在前端或后端合并为短时间窗口内的最新 item，tool、permission、error、terminal 状态仍需即时反馈。
- 后端 `acp.timeline.jsonl` 对 streaming timeline item 的 patch 写入也应短窗口合并，非 streaming item、session 写入、shutdown 和 runtime drop 前必须 flush pending patch，避免长输出时把每个 chunk 都落为一条 patch。
- 后端对 completed ACP timeline/events 的读取缓存必须绑定文件签名（至少文件长度与修改时间）。会话 snapshot 进入 terminal/completed 后仍可能存在最后一批 timeline flush 或 compact 写入，缓存不得仅以路径命中，否则会把缺尾部消息的中间状态长期返回给前端。
- 系统提示、产物预览、工作流编辑等覆盖式交互打开时，ACP 主消息流应暂停非关键 streaming UI flush，仅在内存中保留同一 text/thought/plan item 与同一 `toolCallId` 非终态工具事件的最新合并帧；权限、错误、工具终态和 session 终态仍即时处理，覆盖式交互关闭后再低优先级补 flush 最新帧。
- 前端必须把 text/thought/plan 与非终态 toolCall/toolCallUpdate streaming flush 视为低优先级、可合并的后台 UI 任务，而不是固定定时器任务。覆盖式交互打开、消息列表用户滚动、wheel 等滚动输入期间都应进入同一套 interaction quiet window：取消已排队但尚未执行的 streaming flush timer，只缓存最新帧；交互安静后再 trailing flush。不得为每种交互单独散落补丁式暂停逻辑，也不得在消息容器上用 pointer/touch 起手事件拦截所有按钮点击。
- text/thought live item 的前端合并必须保持单调：同一 stream 的旧短帧、空帧或乱序 hydrate 帧不得覆盖已显示的完整 content；interaction quiet window 只能延迟普通 trailing flush，不能让 tool、permission、lifecycle 或 session 边界事件越过 pending text/thought，显式 `sync` flush 必须绕过交互 defer 但继续尊重真实 live pause。
- Conversation run 级 live update 必须与当前 ACP 消息热路径分层调度：当前 selected session 的普通 timeline event 只进入 ACPChatDialog 局部合并；已存在于 session tree 里的后台 session 普通 live event 不得触发 `getConversationRun` 和整页 React state 更新；只有新 session 锚点缺失、terminal snapshot、权限/暂停/等待输入等交互态才允许排队完整 run refresh。后台非终态 session snapshot 只允许做轻量运行态 patch，且不能替换当前 selected session payload。
- ACP 消息滚动容器的 `scroll` 事件不得同步读取 `scrollHeight/clientHeight/getBoundingClientRect`。滚动期间只允许记录交互和排一个 `requestAnimationFrame`，在 rAF 中合并完成贴底状态、历史分页触发和 `isAtBottom` 更新；timeline 更新后的自动贴底也必须尊重 interaction quiet window，用户正在滚动时不得抢写 `scrollTop`。
- 关闭状态的系统提示弹窗、产物弹窗和工作流 sheet 不应解析大文本或 workflow JSON；打开时再计算内容，并尽量使用 memo 化结果，避免被 live stream render 带着重复执行。
- ACP 事实状态刷新节奏与用户可见呈现节奏必须分层：timeline/live flush 继续负责把同一稳定 item 合并成最新累计快照；当前活跃 text/thought item 由独立 presentation controller 维护“canonical 目标 + 可见 offset + 速率余量”，只把已经到达可见 offset 的 Markdown 前缀放入 DOM。不得把完整 snapshot 先布局后仅用 opacity/stagger 隐藏未显示字符，否则消息容器会按最终高度提前撑开，并让多个 Markdown block 在不同位置提前出现；presentation controller 也不得反向修改 timeline canonical 或让每个字符进入会话级 state。默认以约 32ms 的有界呈现帧推进，并根据积压量在统一速率范围内追赶，从而把 75ms/125ms 的不稳定快照批次平滑成稳定视觉节奏。
- 正在流式增长的 assistant 消息与思考过程必须共用 prompt-kit `Markdown` copy-in，不得让 thought 继续走 `whitespace-pre-wrap` 纯文本旁路。Streamdown streaming mode 只解析当前可见前缀，语法门控在推进 offset 时吞并纯 Markdown 控制符、未完成链接地址和代码围栏后缀，避免 `**`、反引号或 `[blocked]` 占用独立显示帧。不得同时启用 Streamdown 全字符 opacity/stagger：可见 offset 已是唯一呈现状态，第二套字符动画会在 block 重建时把历史字符重新判为新增字符，并让透明字符继续参与布局。thought chunk 的语义分段必须由后端 timeline accumulator 写入 canonical：完整的独立 strong block 之间写入段落分隔，token 级 thought chunk 继续无缝拼接；前端不得为了兼容旧会话重写 canonical。消息离开最新活跃 stream 后允许呈现队列短暂收敛，收敛后关闭 incomplete repair，但同一已流式组件必须继续保持 block renderer 路径，不能在完成瞬间切换成另一棵 static renderer DOM；重新加载的历史静态消息可以直接使用 static mode。最新活跃 stream 必须按最大 `endedSeq/seq` 的事件种类判定，tool、plan、permission 等生命周期事件到达后不得让旧 text/thought 继续处于 streaming。默认不因聊天主路径引入 Mermaid、完整 Shiki 语言包或 KaTeX 等未启用插件。
- 正在输出的 thought disclosure 收起时不得卸载 Markdown presentation。prompt-kit `ChainOfThoughtContent` 对 active streaming thought 使用 Radix `forceMount` 保留组件实例，并在 closed 状态通过 `display: none` 脱离布局；重新展开必须复用原 visible offset 与 DOM，而不是从 offset 0 重放。thought 结束后恢复普通 Collapsible unmount 生命周期，避免所有历史思考内容长期常驻。
- timeline item 必须保持稳定 id；未变化的历史 item 应尽量复用对象引用，让消息、工具卡、thought 和子 Agent 分组的 memo 化渲染有效。
- Raw frames 面板默认只展示行摘要；展开单条 frame 时才做 JSON pretty print 和长段落换行，不允许折叠态批量解析完整 raw 内容。
- 会话式运行页的工作流 Sheet 与 `GraphView` 必须把拓扑布局和运行态映射分开：布局只依赖节点 id/order 与边 from/to/label，ACP live payload、selected session、node status/current 等运行态刷新只能映射到既有坐标，不得重复执行布局。
- 会话 follow、ACP composer 与 GraphView 运行态不得在普通运行中输出持续性 console 日志；排障日志必须面向具体错误，且不能挂在 token/live event 热路径上。排查 `Maximum update depth exceeded` 时，只保留全局 `[gb-ui-error]` 诊断：命中该错误后输出当前 active element、最近 pointer 目标和截断 stack，用于定位 Radix/prompt-kit composed refs 触发源。
- shadcn/Radix `asChild` 触发器内使用的基础交互组件必须稳定转发 DOM ref。`Button` 作为 Tooltip、Collapsible、AlertDialog、Dropdown 等触发器的通用承载组件时必须保持 `forwardRef` 形态；项目封装的 TooltipTrigger、CollapsibleTrigger、PopoverTrigger、DialogTrigger、SheetTrigger、DropdownMenuTrigger、AlertDialogTrigger、SelectTrigger 等 Radix trigger wrapper 也必须保持 `forwardRef`，避免 Radix composed refs 在流式渲染与全局重绘期间反复 detach/attach 并触发最大更新深度错误。
- ACP composer 输入框工具栏属于 live streaming 热路径，`PromptInputAction` 不得使用会把 trigger ref 写入状态的 Radix TooltipTrigger；该区域图标按钮使用无状态原生 title 提示，避免输入框 value/status 高频刷新时 Tooltip trigger ref 参与 React 更新循环。
- ACP composer 的模型、权限等低频配置控件属于冷路径。配置控件不得直接订阅完整 `AcpSessionVm` 或 timeline events；必须先统一归一化为 ACP session config view model，并以 `currentModelId/currentModeId/options` 生成配置签名。普通 text/thought/plan live event 只允许更新消息热路径；配置签名、会话 scope 或稳定 handler 变化时，配置栏才允许重渲染。
- 工作流图边必须保留 success / failure 等 label 标识，并使用 CSS stroke-dashoffset 表达轻量流动感；running 边可以使用更快的流动节奏和轻量 glow，但不得通过 React state、JS timer 或重新布局驱动画布动画。running node 的高亮优先使用 opacity / transform 类合成属性，不使用持续变化的 box-shadow、layout 或大面积 paint 动画。

### canonical lifecycle

会话页不得再让 runtime、attempt、ACP session 与 composer 各自重复解释同一个 `status` 字符串。后端 conversation VM 必须为每个 leaf 派生 `lifecycle`：

| 层级 | 字段 | 职责 |
|---|---|---|
| runtime facet | `status / outcome / pauseReason / resumable / current / active / continuable / phase` | 表达 workflow runtime 与 attempt 是否仍由运行时控制、是否可继续，以及当前运行阶段 |
| ACP facet | `status / active / stopping / terminal` | 表达底层 ACP provider/session 是否还在响应或停止流程中 |
| lifecycle 顶层 | `displayStatus / runtimeDisplay / continueKind` | 作为 session tree、activeSessions 与 composer 的基础派生事实源 |
| composer facet | `mode / submitTarget / processingKind / statusKey / canStop / lockInput` | 作为 composer 输入、停止、状态文案和提交目标的唯一业务规则源 |

`status` 与 `runtimeDisplay` 仍可作为兼容字段暴露，但必须由 lifecycle 同一个派生函数产出，不能在前端或其他 VM 中重新拼优先级。

`runtimeDisplay` 必须同时表达视觉结果和错误语义：`tone=danger` 可以表示测试/验收节点正常完成后的 workflow outcome failure，但只有 `blockingError=true` 才能驱动 composer 的 runtime/session error 面板。前端不得再用红色或终局状态反推运行时错误。

runtime 已 terminal/completed 且不可继续时，底层 ACP snapshot 中残留的 `running / sending / responding` 只能作为 stale 事实处理，不能让 leaf 或 composer 继续保持 active。反过来，当前 ACP session 已自然 `completed` 但 runtime 仍处于 active 时，后端必须用 `runtime.phase=launching-next-node` 与 `composer.processingKind=launching-next-node` 表达“拉起下一节点中”，前端不得自行 suppress runtime active 或把 composer 清空。停止后继续当前 paused leaf 不属于拉起后继节点；如果旧 ACP snapshot 是 `cancelled / failed / killed / error` 等 terminal 状态但 runtime 已接受继续，后端 lifecycle 应输出普通 `provider-running / processing`，不得复用 `launching-next-node`。只有后端 lifecycle/ACP facet 明确处于 stopping，或本地 stop 命令尚未返回时，才可以继续优先锁定 composer，但同一 attempt 已收到 `completed / cancelled / failed / killed / error` 等 ACP terminal snapshot 后，必须立即结束 ACP active/stopping 与本地 stopping 锁定。会话式运行页收到当前选中 session 的完整 session snapshot 时，必须先在 App 层更新 `ConversationRunVm.selectedSession`，再刷新 run tree/lifecycle；若 run refresh 返回的 `selectedSession` payload 临时为空，前端必须保留同 key 的现有 session payload；同 key 的完整 session snapshot 则作为 payload 权威更新替换旧值；selected session identity 变化时不得沿用旧 payload；会话组件也不得仅因本地已有 timeline events 就把缺失 payload 重建为 `running`，只有 runtime lifecycle 明确 active 时才允许创建临时 running shell 承载早期流式事件。

composer 只消费后端 lifecycle/composer + ACP session live status + 少量本地 optimistic 状态；placeholder、输入禁用、停止按钮、状态文案和发送目标都来自同一个 semantic composer state。若 ACP facet 已进入 terminal，历史未匹配且不属于当前提交的 optimistic sending 只能作为过期本地提交看待，不得继续触发“发送中”或输入锁定；但用户刚发起的新 same-session ACP prompt 必须作为当前本地 turn 展示发送中/处理中，直到后端接受 prompt、返回拒绝、或返回未包含该 prompt 的 terminal/空 session 后显式收敛。`runtime-continue-started` 表示后端已经接受本次继续命令，即使命令返回不携带新的 ACP session payload，前端也必须立即结束本地 sending/awaiting optimistic 锁，把本次用户气泡视为已接受；后续是否锁输入只由返回的 lifecycle 与后续 session/lifecycle update 决定。

### 互斥状态
1. **正常输入**：当前 session 已正常结束时，用户可继续输入消息（含附件），发送目标为 ACP same-session prompt
2. **运行中锁定**：当前 lifecycle 表示 runtime active 时不允许输入消息
3. **停止中锁定**：本地 stop 命令未返回、ACP session 为 `cancelling/cancel_requested`、或 lifecycle 的 ACP facet 为 `stopping` 时，composer 显示“正在停止当前会话…”并锁定输入；但同一 session 的 ACP terminal snapshot 已到达时，本地 stop/cancelling 与 stale `acp.stopping` 必须让位
4. **运行错误提示/操作**：当前 session 派生为 `runtimeDisplay.blockingError=true` 且后端 composer 给出 `runtime-error` 时，不允许输入，显示错误原因；`error-blocked` 必须优先展示后端 `runtimeErrorMessage`（来自 run-progress 阻塞摘要或等价 runtime 错误摘要，遇到结构化 `RuntimeErrorInfo` 时优先使用 `code + params` 映射文案），没有具体原因时才使用泛化文案；测试/验收节点正常完成后的 `failure / invalid` 只表示 workflow outcome，不触发 runtime-error 锁定态。`error-blocked` 表示不可重试的 runtime 阻塞，不提供 `runtime-continue` 输入入口；历史 killed/session failed 仍使用终止或失败文案。provider/auth/quota/rate-limit/model/catalog/workspace/transport 等异常应表现为 `runtime-abnormal`，不能因为 ACP session failed 或 provider stopReason=error 而进入 failure edge 或 runtime-error 锁定态。
   - workflow 控制流限制导致的终局失败（例如 `workflow_control_limit_exceeded`、`max_rounds_exceeded`）属于 run-level runtime 业务异常，不属于 ACP session failure，也不应把 composer 强制派生为 `runtime-error` 锁定态。后端会话 VM 必须复用 canonical control failure 解析结果，把标题与原因归一化到 `runtimeErrorMessage`；前端 ACP 会话页在顶部错误横幅中展示该消息，同时保持已终止会话的普通输入/追问能力。
   - 该类终局失败的实时刷新由后端 `RunCompleted(Failure)` lifecycle 负责，不能只依赖 ACP session terminal update 或重新进入页面时的冷加载。
5. **工作流无效修复按钮**：只有 submit target 为 runtime continue 且 workflow 无效时才不允许输入并显示修改按钮；当前 session 已正常结束后的 ACP same-session 追问不受 workflow invalid 阻塞
6. **人工 check 判定门**：当前 session 因 `waiting-for-user-input + manual_check_pending` 暂停时不显示继续按钮，输入框保持可用；普通文本只走 ACP same-session prompt，不推进 runtime edge，只有成功 / 失败判定按钮触发 `submit_manual_check`。
7. **停止后用户介入 / 运行异常继续**：当前 session 因用户停止派生为 `process-interrupted`，或因可恢复异常派生为 `runtime-abnormal`，且可继续时，不显示继续按钮，恢复输入框；用户发送的文本仍走同一条 runtime `continue` 链路，只是把默认“继续”替换成用户发送内容，因此用户感知上是在会话中发出一条消息。`runtime-abnormal` 需要保留异常视觉与提示文案，但不进入 `runtime-error` 锁定态。若异常处于 `recovery=auto` 的 bounded retry 中，composer 锁定并展示重试中；重试耗尽后降级为 `runtime-abnormal + manual` 并恢复输入。

### 修复入口

- 会话运行时的“修复”按钮与旧任务工作流页的 repair drawer 心智一致：打开当前任务工作流编辑 Sheet，让用户修复 workflow 配置。
- 修复 Sheet 标题使用“修复工作流”，而不是普通“编辑工作流”；Header 中展示无效状态、查看错误原因入口和错误原因摘要，帮助用户理解为什么需要修复。
- 在会话页保存修复后的 workflow 后，必须重新拉取当前 conversation run VM，使 workflow 有效性、session tree、工作流图与 composer 状态立即刷新。
- 修复入口不直接调用 `continueRun`；用户完成修复后再按运行态规则继续。对于 `error-blocked`，修复入口只表示查看错误、修改 workflow 或进入诊断；只有后端确认存在安全恢复点并生成恢复计划时，才允许恢复，否则只能重新运行或从节点重新开始。

### 继续输入
- 当前 session 正常结束后，在会话窗口追问属于 ACP same-session prompt，不要求 authoring workflow 合法
- 追问发送时，composer 进入本地 turn 的发送中 / 处理中 / 计时状态；结束后只影响该 ACP session 的消息流，不触发工作流 runtime 继续执行
- Gold Band 在发起 Direct 会话时提供的合成模型选项统一命名为“不指定”（英文 `Unspecified`），其值只存在于 Gold Band UI，提交时固化为 `modelOverride = null`，不得与 Agent 通过 ACP 返回的 `default`、`auto` 等不透明模型 ID 混用。
- ACP session 同时保留 Agent 报告的 `currentModelId` 和 Gold Band 管理的 `modelOverride`：前者只用于呈现 Agent 当前配置，后者是后续追问是否显式调用 `session/set_config_option(model)` 的唯一事实源。`modelOverride = null` 时追问不设置模型，继续继承 Agent 环境配置。
- 会话详情在 `modelOverride = null` 时显示“不指定”，并同时展示 Agent 返回的完整模型目录；用户选择任意 Agent 模型（包括 Agent 自己返回的 `default`）后写入显式 override，同一 session 内不再提供“不指定”选项，但仍允许在 Agent 模型之间切换。
- 当前 run 暂停后通过 runtime 继续仍然要求 workflow 合法；如果 workflow 无效，composer 只显示修改按钮
- 对不支持原生 `systemPrompt` 的 ACP provider，Gold Band 只在同一 ACP session 的首轮 `session/prompt` 中把 stable system prompt 作为 hidden user block 内联发送并持久化审计；同一 session 的停止后继续、恢复继续和完成后追问不得重复内联或重复 timeline 记录 stable system prompt。后续输入只包含本次用户文本与本次新上传附件；不得重带原始任务附件、历史附件或上一轮 runtime hidden context。
- 当前 run 因 `process-interrupted` 或 `runtime-abnormal` 暂停且可继续时，composer 允许输入用户补充内容并触发 workflow runtime continue；这与当前 session 已正常结束后的 ACP same-session 追问不同，不能退化为普通 ACP prompt。旧 ACP snapshot/session 的 `failed` 或 `cancelled` 只代表上一段响应的历史终态，不能取消本次继续、阻断 agent 拉起，或驱动 composer 的“会话已终止/运行失败”错误态。AI-DYNAMIC 内部 leaf 继续必须由后端根据 locator 生成精确 leaf override：继续前先检查目标 leaf 是否已有完整合法 `dynamic-node-completion`，若已完成则先收敛并接受 proposal，避免重复发送；如果父 run/round/外层 AI-DYNAMIC attempt 因同一个 attempt 的可恢复中断而处于 paused，后端必须在返回 `runtime-continue-started` 和启动 scheduler 前先恢复外层 running，避免前端拿到 accepted 后仍显示旧 paused/sending 状态，也避免 scheduler 立即再次按 outer stopped 暂停 graph；再把同一 dynamic graph 中 `running/ready + outcome=null + ACP cancelled` 的 stale sibling 收敛为 paused 并移出 `currentNodeIds`，最后只恢复本次目标 leaf 的同一 ACP session；没有明确 leaf 目标的父 run continue 不能批量恢复普通 paused worker，只能恢复代表 child run 的 workflow-invocation leaf。父 run 继续如果带用户显式输入，且实际恢复目标是 workflow-invocation child run，则该输入必须继续传入 child run 的 paused worker，并按 `UserMessage` 渲染；只有父 run 纯继续且没有用户输入时，child run 才使用 `WorkflowResume`。同一 dynamic graph 的多个 leaf 几乎同时继续时，后端只允许一个 graph scheduler 启动；后到的继续请求在 scheduler 注册完成前暂存为 pending resume，注册后立即交给同一个 scheduler，并继续按 `maxParallel` 并行拉起 leaf，不把 leaf 执行串行化。`error-blocked` 不走 runtime continue，必须显示不可重试错误。
- 会话态与旧 Round 详情中的工作流 attempt 文本发送、暂停按钮继续和继续发送都必须调用 `submit_conversation_prompt`。前端不得再按普通节点 / AI-DYNAMIC 内部节点、ACP prompt / runtime continue 自行分叉；后端根据 lifecycle/composer 与 `AttemptLocator` 决定走 `acp-prompt`、顶层 `runtime-continue` 或 AI-DYNAMIC inner exact resume。`send_acp_prompt` 只保留给不参与 workflow runtime 生命周期的 raw ACP 会话；如果命中 paused/resumable/current workflow attempt，后端必须拒绝并要求使用 `submit_conversation_prompt`。
- 停止按钮只调用桌面 `stop_active_session` 统一语义入口，不在前端按“ACP / runtime”维护两套停止链路。用户语义始终是“停止当前进行中的 leaf/session”；后端根据当前 run 与选中 session `AttemptLocator` 做分层收敛：普通单节点 attempt 停止会把当前 runtime attempt 写入 `Paused + ProcessInterrupted`；AI-DYNAMIC 内部 leaf 停止只暂停目标 dynamic node 与目标 ACP session，兄弟 leaf 仍为 `Ready | Running` 时父 graph/run 继续运行；当没有任何 active leaf，且剩余未完成 leaf 都是用户暂停的可继续节点时，父 dynamic graph、外层 AI-DYNAMIC attempt 与 run 自动收敛为 `Paused + ProcessInterrupted`，不能显示为错误阻塞。活跃 ACP runtime 发送一次 `session/cancel` notification 后继续 drain 当前 `session/prompt`，直到 adapter 返回 cancelled/interrupted 或 cancel deadline 到期；停止不会写入 `Killed`，也不把 adapter kill 当作 cancel 成功兜底。
- `stop_active_session`、attempt teardown 和 run 级 best-effort cancel 在处理 ACP 停止时，除了 permission 外也必须同步取消 attempt 目录下尚未完成的 elicitation request，确保 runtime 中阻塞等待 `elicitation/create` 的分支能立刻解除，不留下孤儿轮询。
- `stop_active_session`、应用关闭和启动 crash recovery 共享的 attempt cancel 流程必须把未决 ACP permission request 一次性收敛为终态：写入 `acp.permission-response.*.json` 的 `cancelled=true`，并同步 upsert `acp.timeline.jsonl` / legacy `acp.events.jsonl` 中对应 `permissionRequest` 为 `status=cancelled`。前端重进页面只能回放“该 tool call 已中断/取消”的历史事实，不得再次弹出权限决策；已经由用户选择成 `selected` 的权限事件不能被停止流程覆盖成 cancelled，已被停止流程写成 `cancelled` 的权限也不能被迟到的旧弹窗点击覆盖回 `selected`。
- `AcpSessionVm.events` 的分页窗口可以裁剪普通消息，但不能裁剪掉权限请求的最新事实。后端返回 session VM 时必须把每个 `permissionRequest` 的最新状态事件附加到当前窗口中，用同一 request id 覆盖前端缓存中的旧 `pending`；否则用户授权或停止后，历史缓存可能继续把已完成权限恢复为弹窗。
- ACP permission request 的业务身份是 canonical request id（去除重复 `permission-` 展示前缀后的 `raw.requestId` / `id`），不是 `sessionId`、attempt id 或 timeline display id。前端 live/cache/session merge 必须按 canonical request id 替换旧事件；后端补写 `cancelled` 终态时应继承原 pending event 的 `sessionId`、`toolCallId`、title 与 raw options，避免同一权限请求在 UI 中裂变成“旧 pending + 新 cancelled”两条事实。
- ACP permission 的状态机由后端 permission lifecycle 统一拥有：收到 request 时写入 `pending`，用户 Allow / Reject 写入 response signal 时必须同步 upsert `permissionRequest(status=selected)` 到 `acp.timeline.jsonl` / legacy `acp.events.jsonl`，并继承原 pending event 的 `sessionId`、`toolCallId`、title 与 raw options；runtime waiter 消费 response 后也要再次确认终态已落盘。正常决策、停止取消和非活跃 fallback 不能分散维护 timeline 终态，否则重进会话会把历史 pending 重新识别为弹窗。
- 活跃 ACP runtime 的内存 timeline map 与磁盘 timeline 同属权限状态事实源。权限 response 被 live waiter 消费时，终态事件必须通过 runtime 自身的 `persist_event` 合并进内存 timeline，再写 patch / final timeline；不能只由 Tauri command 或 helper 直接改磁盘，否则 runtime shutdown 时旧的 pending 内存快照会覆盖刚写入的 selected/cancelled。
- ACP elicitation 的状态机由后端 elicitation lifecycle 统一拥有：收到 `elicitation/create` 时写入 `elicitationRequest(pending)`；Tauri command 提交用户决策时写入 durable response signal 并 upsert `elicitationResponse(completed)`，前端据此立即关闭卡片。runtime waiter 读取同一 signal、持久化规范终态、发送 ACP JSON-RPC response 后，才清理 request/response signal。不得根据 `acp.snapshot.json` / `acp.session.json` 的 completed 等元数据提前删除 signal，因为已完成 run 的 follow-up prompt 仍可能存在活跃阻塞 waiter。
- `acp.permission-request/response.*.json` 与 `acp.elicitation-request/response.*.json` 只作为 runtime 阻塞等待与 Tauri command 之间的临时信号文件，不作为长期事实源。permission/elicitation 的历史事实统一落在 `acp.timeline.jsonl` / legacy `acp.events.jsonl`；elicitation response 的所有权固定为“command 生产、runtime 消费并在成功回包后清理”。显式 stop/close/timeout 负责未决交互的取消和陈旧文件收敛，不能由展示层会话状态推断 waiter 是否存在。
- `stop_active_session` 返回成功代表后端已完成本次停止请求的业务落盘收敛：目标 leaf attempt 已进入可继续暂停态，目标 ACP session/snapshot 已写为 `cancelled`；父 run 是否 paused 由 graph 聚合状态决定。`session/cancel` 是无 response 的 notification，因此前端不能把命令返回理解成 provider 已确认取消；命令 pending 期间显示“正在停止”遮罩，返回后按后端 lifecycle 恢复目标 leaf 的可继续态，后续 ACP terminal snapshot 继续刷新消息流。
- 停止过程中可能同时出现 `run paused/process-interrupted` 与 ACP channel 仍在 drain 的事实；composer 展示优先级必须以停止流程为准：本地 stop 命令未返回、lifecycle 的 ACP facet 为 `stopping`、或 session metadata 为 `cancelling/cancel-requested` 时显示“正在停止当前会话…”并保持输入锁定。`provider.pid` 只作为 adapter process metadata，不参与停止完成、active/stopping 或 composer 状态推导。
- ACP adapter 生命周期按 `provider_id + workspace_root` 复用长连接；这里的 `workspace_root` 是用户打开的逻辑项目根目录，同一 workspace 下同一 provider 的多个 ACP session 共享一个 adapter process，不同 workspace 的 connection 可以在新 UI 中并存。AI-DYNAMIC worktree 只是 session 执行目录，不作为新的 adapter workspace key；adapter process 仍归属原始逻辑 workspace，`session/new.cwd` 才指向具体 worktree。后端 connection manager 按 JSON-RPC request id 与 `sessionId` 路由 response、timeline update 和 permission request。用户不感知 adapter pool，也不在前端暴露 cancel/close/delete 协议概念。
- 普通 Stop 只中断当前 prompt；停止后 Gold Band 持久化保留原 ACP `sessionId`，runtime continue 必须继续用原 `sessionId` 恢复同一业务会话。session release、关闭应用以及 agent/MCP 配置保存导致的 restart boundary 使用 bounded `session/close` 释放 live sessions；关闭应用时先把所有 running run 递归收敛为 `Paused + ProcessInterrupted`，再对 manager 中所有 live provider/workspace connections 发起 bounded close，不能只按当前 workspace 过滤。普通 workspace 切换只是切换当前工作区视图，不关闭旧 workspace connection。新 UI 侧边栏删除 workspace 属于显式 remove boundary，移除前必须 bounded close 该 workspace 的 ACP connections，close 失败则保留 workspace 并展示错误。重跑前停止旧 run 同样走 `Paused + ProcessInterrupted`，不产生新的 `killed` 会话；历史 killed 仅作为只读兼容状态展示。配置保存遇到 active prompt 时直接阻断并提示用户先停止会话，停止后再保存才关闭 idle connection 并使用新配置。adapter crash、stdout 断开或 transport closed 按可恢复中断处理，active runtime 收敛为 `Paused + ProcessInterrupted`；close 失败必须作为明确错误处理并记录诊断，不能静默吞掉，也不能把 kill adapter 伪装成成功。启动 crash recovery 没有 live connection 时只依据持久化 runtime lifecycle 收敛状态，`provider.pid` 仅作为 orphan cleanup 线索。
- `session/close` 是 ACP session 的关闭边界，不是可恢复暂停边界。后端在发送 close 前必须先结算该 attempt 下所有 pending permission / elicitation：写入 cancelled response、把 timeline 中对应 request upsert 为 terminal 状态，并清理 pending signal 文件；close 成功后必须把 `acp.snapshot.json` / `acp.session.json` 写为 `cancelled + stopReason=cancelled`。视图层读取历史数据时，如果 snapshot 仍是 active 且 `acp.raw.jsonl` 中最后一个 ACP 生命周期边界是同一 JSON-RPC id 的 `session/close` request 与成功 result，必须将该 session 熔断为 `cancelled` 并隐藏 pending permission；如果该 close 之后又出现新的 `session/load` / `session/prompt`，说明会话已经重新进入运行态，不能再用旧 close 熔断当前 snapshot，避免恢复运行中的权限响应被误判为 terminal 后删除。
- composer semantic state 的优先级固定为：permission blocked → stopping → submitting → runtime active lock（含 `launching-next-node`）→ invalid workflow（仅 runtime continue 路径）→ runtime error（含 `error-blocked`）→ `process-interrupted` / `runtime-abnormal` 输入继续 → `waiting-for-user-input + manual_check_pending` 普通 ACP prompt + 判定按钮 → normal ACP prompt。后续新增状态必须先进入该派生表和矩阵测试，不能在组件里局部追加布尔判断。
- `permission blocked` 属于 runtime 运行态阻塞，不是独立的 composer 替代视图。前端必须继续渲染同一个 prompt-kit `PromptInput`，用禁用 textarea、运行态 placeholder、权限等待 hint 和停止入口表达“当前会话由 runtime 运行中，暂不可输入”；权限决策卡片可以作为会话交互卡展示，但不得覆盖或替换原输入框。刷新或重进页面后，只要持久化 timeline/session 仍存在 pending permission，就必须恢复同样的锁定 composer 状态。
- 排查停止状态不得恢复持续性 ACP composer console 日志；如需再次定位停止链路，应优先补充状态矩阵测试或临时一次性断点式诊断，完成排查后必须移除。

### 停止
- 停止并重跑在顶部操作区
- composer 内也有 stop 按钮（ACP 会话停止）
- composer 内的 ACP 停止表示“中断当前响应”，不是 workflow 配置错误；停止后的 attempt 应显示为可继续暂停
- 会话内停止使用 `stop_active_session` 单一路径；旧 UI Run 停止与新 UI 侧边栏 run 右键“停止”使用 `pause_run`。新 UI 侧边栏停止菜单只挂在具体 run 行，不挂在任务/需求标题行；菜单打开和菜单内容二次右键都必须阻止 WebView 原生右键菜单。二者共享普通中断语义但作用域不同：`stop_active_session` 只停止当前 leaf/session，AI-DYNAMIC fan-out 中不会拖停兄弟 leaf；`pause_run` 停止整个 run，会把该 run 下所有 active leaf 一起写成 `paused + process-interrupted` 并分别发送 `session/cancel`。若运行线程控制句柄不可用，则通过 live ACP connection registry 对目标 attempt 的真实 ACP session 发 best-effort `session/cancel`。活跃 ACP runtime 不因 cancel notification 已发出就立刻退出，而是继续 drain 当前 `session/prompt`；cancel timeout 必须暴露为明确错误，不能 kill adapter 伪装成功。停止不是 kill run，不能把 run/round/node/dynamic node 写成 `killed`。
- 新 UI 侧边栏的 Workflow/AUTO task 只要存在 run，就必须展示 run 子列表；只有一个 run 时也展示 `run-001` 行，确保右键停止菜单始终挂在具体 run 行上，而不是回退到 task 行。Direct task 不展示 run 子列表，停止当前回复继续使用会话 composer 内的统一停止入口。
- 新 UI 侧边栏的置顶区与普通工作区是两个独立的列表区域；同一会话同时出现在两处时，run 子列表开合状态必须按区域隔离。置顶区内部一次只展开一个会话实例，普通工作区内部一次只展开一个会话实例，点击其中一区不得联动展开另一区的同一 task。
- 新 UI 侧边栏的选中高亮同样按区域隔离；同一会话同时出现在置顶区与普通工作区时，只高亮用户最后交互的那个区域实例，另一处保持普通展示，避免用户误判两处列表被同步选中。
- 停止期间会话窗口显示全局“正在停止”遮罩，停止正常交互与流式观感；后端只合并已经进入 ACP runtime channel 的事件，不再等待额外文件信号。命令返回后前端按后端 lifecycle 和最终 snapshot 对齐已确认消息。侧边栏 run 级“停止”点击后也必须立即关闭菜单并展示页面级“正在停止当前运行”遮罩；遮罩不只跟随 `pause_run` 命令返回，而是等当前 run VM 刷新确认 run 非 running、active sessions 清空且选中 ACP session 已 terminal 后再消失，避免用户误以为操作没有生效。
- 关闭客户端和启动时崩溃恢复与用户停止共享同一 interruption 语义：所有仍为 running 的 run、当前 node 和 AI-DYNAMIC descendants 都收敛为 `paused + process-interrupted`。`provider.pid` 不参与业务状态判断，只能作为 adapter process metadata 用于诊断和 orphan cleanup。
- 停止请求一旦落盘，迟到的普通 ACP success response 不能写 success artifact，也不能驱动 workflow 跳到下一节点；runtime 必须在 provider 返回后重新确认当前 attempt 仍是 running/current，确认已暂停则直接停止推进。唯一例外是 AI-DYNAMIC worker 已经返回完整且合法的 `dynamic-node-completion`：这属于业务结果已完成但 stop 先被 ACP 观察到的竞态，应按完成优先接受并继续 graph 推进；非法/不完整 completion 不进入该例外。
- runtime 异常、agent/provider 异常与 workflow DSL 无效必须分开提示：只有 `workflowValid=false` 或明确的 workflow validation error 才展示“修改/修复工作流”入口；`runtime-abnormal` 表示异常但可继续，恢复输入框并保留异常提示；provider/model/catalog/workspace 等 manual 可恢复异常也归入 `runtime-abnormal`；`error-blocked`、session failure、session killed 等不可继续运行期异常只提示查看错误原因，不默认引导用户修改工作流。
- 当前选中 session 已有 `diagnostics.lastError` 时，错误面板文案应直接拼接具体错误原因，避免用户再额外寻找日志入口。
- 新 UI 中，`process-interrupted` 不再展示单独“继续”按钮，而是恢复输入框；用户点击发送后仍走 runtime `continue`，只是把默认“继续”替换成用户本次输入内容，因此用户感知上是继续在当前会话里发消息。运行时在 `run_continue()` 主路径必须显式区分两类 continue：`pause_reason=process-interrupted` 且本次带用户显式输入时，渲染语义为 `UserMessage`（仅用户原文，不注入 hidden runtime context，不包装 `# Goal`）；只有 workflow 自身恢复执行、没有用户追问语义的 continue，才使用 `WorkflowResume`（hidden runtime context + `# Goal`）。AI-DYNAMIC 外层 attempt 的 parent continue 输入不作为内部 leaf override 存储；它只在恢复 workflow-invocation child run 时向下传递，避免把普通动态 worker 批量恢复为用户追问。

## 会话信息栏（ACPSessionHeader）

- 单行布局：Agent icon + Agent 名称 + 可复制 sessionId + 操作按钮；权限模式属于可变运行配置，不在会话身份栏中展示
- Agent 名称与 sessionId 必须放在共享的文本基线容器中，并使用一致的行高节奏；外层图标与操作区仍按控件中心对齐，禁止通过单独的 top/margin 像素偏移补偿字号差异
- Direct 组合页头通过留白区分“会话标题”和“Agent 身份”两组信息：标题末尾保留约 12px 组间距，Agent 名称与 sessionId 保留约 6px 组内距；长 sessionId 默认显示前 8 位与后 4 位，中间使用省略号，Tooltip 与复制操作继续使用完整值
- 会话信息栏与运行标题栏保持同一套紧凑节奏：缩小上下 padding、降低主标题字号、压低按钮高度，减少双层头部对内容区的挤压
- 可编辑会话标题的悬浮提示统一使用项目内置 shadcn Tooltip，禁止使用 HTML `title` 触发 Windows/WebView 原生 tooltip；鼠标悬浮与键盘聚焦共享主题化提示样式
- Workflow/AUTO 的第二行作为元信息层，视觉权重需低于第一行：更小字号、更轻字重、更弱对比度，不与任务标题竞争主次；Direct 使用下述单层组合页头
- 浅色主题下运行标题栏与 ACP session header 使用纯白 `content-header`，只保留轻量分隔线；灰色仅属于窗口框架和侧栏，不得在主内容顶部形成连续灰带。深色主题由同一 token 映射到对应低层 surface。
- 用户消息气泡使用独立的 `message-user` / `message-user-foreground` 语义 token，不复用 `primary` 混色。科技灰下采用 `#f3f3f3` 浅灰底与 `#202020` 深色正文，不显示可感知边框和投影；长消息仍应保持轻盈，不能形成大面积中灰实体面板。深色主题使用同源的中性高层 surface 与高对比文字。
- assistant 自然语言正文直接显示在页面背景上，使用实色 `foreground`，不再包裹白色卡片、灰色边框或投影。工具、思考、代码块和控制输出仍可使用必要的结构化 surface，从而让主阅读路径保持高白度与高黑度。
- ACP 会话主消息流、raw frames 面板和 prompt-kit 聊天滚动容器使用 Gold Band 主题化滚动条；滚动条颜色必须来自主题 token（主色、muted、surface），科技灰主题使用无彩石墨与中性 surface 混合，不回退为系统默认灰色，也不引入蓝灰色偏。
- Gold Band runtime prompt 中的 `<hidden data-gold-band-hidden="true">` 段在用户消息气泡内默认折叠展示，折叠块与可见 requirement/goal 同属一个 bubble；展开后展示隐藏原文，再次点击收起。折叠块使用当前文字色的极低透明 surface，不使用白色 `background` 在浅灰气泡内再造一层亮卡片。用户消息行建立 inline-size container，结构 token `--conversation-message-max-inline-size: 82cqi` 只定义消息气泡允许使用的最大测量宽度，不直接作为最终正文宽度。组件在这个上限内创建不可见的同字体测量副本，通过 `Range.getClientRects()` 读取每个实际排版行的宽度；折叠态最终宽度取隐藏标签完整宽度与所有可见正文行宽度的最大值，展开态再纳入已展开隐藏正文行宽度，并向上取整为稳定像素值。消息行 ResizeObserver、展开状态和字体加载完成都会触发重新测量，因此窗口变宽只会改变文本的真实换行结果，不会让气泡随容器比例无条件线性增长。隐藏区使用嵌套 grid stretch：根节点、Trigger 和 Content 均不声明 `w-full` 等百分比宽度；外层先应用测量后的最终宽度，内层单列 grid 与 Trigger 的 `minmax(0,1fr) auto` 两列布局再自动铺满。超长内容达到消息列上限后换行，不使用固定 `rem`/像素宽度猜测，也不使用 inline-size containment 排除可见区域的宽度贡献。该规则适用于 workflow new 和 workflow resume，并覆盖会话态与旧工作台复用的 ACPChatDialog；用户手动追问和 runtime repair 不注入 hidden runtime context。hidden 后面的可见片段只在展示层去掉开头换行，真实 prompt 事件内容不变。
- 产物来源固定为当前选中 session（含 AI-DYNAMIC 内部节点）的 artifacts / attachments，不使用 run 级聚合占位数据
- 产物弹窗遮罩使用轻量弱化遮罩（低透明深色 + blur），主体面板保持半透明而不过度强调，不做厚重黑色卡片
- sessionId 与 Agent 身份同行，不再单独占行；长值采用“前 8 位…后 4 位”的紧凑投影，点击仍复制完整值，悬浮显示完整值，并在复制后显示会自动消失的轻量“已复制”提示
- sessionId Tooltip 的复制反馈采用 `idle -> copied -> closing -> idle` 单一状态生命周期：反馈到期时先保持“已复制”内容关闭 Tooltip，关闭过渡完成后才恢复完整 ID 内容；`closing` 阶段忽略悬浮重开，禁止在关闭动画中闪现完整 ID

## 产物/附件信息区

- 位于 composer 上方、任务列表折叠面板上方；当任务列表存在时，产物/附件折叠面板在上，任务列表折叠面板在下，二者使用同一套紧凑边线、触发器高度和展开动效。
- 三区展示：输入附件 / 产物 / 附件（输出）
- 当前选中 session 的产物 / 输出附件默认收起为“产物与附件”折叠面板，摘要只展示数量大于 0 的资源类型；展开后展示可点击文件项。
- 整体采用与任务列表一致的折叠信息区，优先压缩上下留白与按钮高度，避免资源区挤占对话输入区和消息区高度
- composer 底部状态栏与资源条之间不额外保留大块过渡留白，输入区、模型权限信息与资源条保持连续的紧凑垂直节奏
- 资源条不单独增加顶部边线，直接承接 composer 自身底边，避免连续双分隔线把输入区与文件区切得过碎
- 资源条首行内容尽量贴近 composer 底边，优先压缩资源条自身顶部内边距，而不是继续压缩文件 chip 点击热区
- 输入附件来源于 task 级 `authoring/inputs/`，创建会话时设定，重跑自动复用
- 输入附件使用 Upload 图标 + 蓝色标记，与输出产物/附件区分
- 当前选中 session 的产物 / 输出附件统一通过折叠面板内文件项进入弹窗查看，点击文件项直接打开该文件详情，不再经过单独列表页，也不再保留顶部重复入口
- 点击查看详情，图片类附件必须以图片元素渲染原图预览；base64/data URL 不直接展示为文本
- 当前选中 session 即使没有可展示 ACP 消息内容，只要 attempt 目录下存在 `artifacts/` 或 `attachments/` 文件，底部资源条也必须列出对应文件 chip；资源展示绑定 session locator（round/node/attempt，含 AI-DYNAMIC outer locator），不绑定聊天内容是否成功加载。

## 附件生命周期

- 新会话附件绑定 task，作为初始输入的一部分，持久化到 `authoring/inputs/`
- 重跑复用 task-level 附件（同一 task 的 `authoring/inputs/` 在多次 run 间共享）
- 继续对话新附件进入当前 ACP session 的 user-inputs 链路，不写入 task 初始输入附件目录
- 输入附件展示为独立层级，不与 agent 运行产物和输出附件混合

## Todo/Plan 任务面板

- 位于 composer 上方、AcpUsagePanel 下方
- 默认收起，显示任务进度摘要（如 "2/4 · 当前任务名称"）
- 展开后展示完整条目列表，每项包含状态 Badge 和内容
- 仅显示主会话顶层 todo；子 Agent 内部 plan 保留在各自分组中
- 每次 plan 更新时面板实时刷新，不再在消息流中追加重复 plan 卡片

## Composer 配置栏

- composer 底部模型与权限配置统一使用胶囊式控件外观，模型选择器需要明确表现出“可展开下拉”的交互心智，不能像纯文本标签
- 模型下拉列表默认向上弹出，并受当前窗口可用高度约束；超出时内部滚动，不允许选项直接溢出会话窗口外
- 模型和权限都是当前 ACP session 的可切换配置；选中列表项后需要立即更新会话快照，并通过 ACP `session/set_config_option` 或 provider 能力等价路径同步到底层会话。
- 后续同一 ACP session 的每次追问和 runtime continue 都必须优先复用当前会话快照中的 `currentModelId / currentModeId`；如果用户中途切换了模型或权限模式，下一次 `session/prompt` 或停止/异常后的继续恢复必须同时带上最新模型与权限选择，而不是回退到节点初始配置。该选择属于当前 attempt 的运行配置，只影响本次恢复的目标会话，不回写 workflow DSL，也不污染后继节点的模型/权限策略。
- 模型选中态只在触发器展示模型名称，长描述只在下拉项中换行展示，不允许撑破触发器或越出窗口边界
- 配置栏解析逻辑统一收敛在前端 ACP session config 工具中：优先读取 provider 返回的 `models.availableModels / modes.availableModes`，缺失时回退 `configOptions[category=model|mode].options`。展示组件只消费归一化后的 id/name/description，不在 JSX 内重复解析协议 payload。

## 工具调用参数展示

- 工具调用卡片展开后以有序列表展示工具输入参数
- 参数按来源优先级提取：rawInput > 结构化 fields > title/locations 解析
- 同标签参数保留多个不同值（如多个路径、多个查询条件）
- 语义化参数缺失时回退展示原始输入 JSON

## Runtime Control JSON 展示

- Runtime output contract 或 AI-DYNAMIC completion 被后端实际消费为控制 JSON 后，后端会在对应 ACP `textDelta` timeline item 的 `raw.runtimeControlOutputDisplay` 中写入展示标记；前端只消费该标记，不按消息内容全局猜测 JSON。
- 带标记的 assistant 消息如果同时包含自然语言和控制 JSON，自然语言继续作为普通 assistant 消息气泡展示，控制 JSON 单独展示为 `GOLD BAND 工作流控制` 折叠控制条。
- 带标记的 assistant 消息如果仅包含控制 JSON，不展示普通消息气泡，只展示折叠控制条。
- runtime 已将输出作为控制候选处理但 JSON 不合法时，也会写入 `parseStatus=invalid` 展示标记；这类消息同样拆分展示为自然语言 + 控制条，展开后展示原始 JSON-like 内容。
- 未带标记的 assistant JSON 始终按普通 Markdown 消息展示，包括节点完成后的普通追问、用户要求 agent 输出的业务 JSON、调试说明和代码示例。
- 控制条视觉参考 tool call 的紧凑结构；`parseStatus=valid` 使用 Gold Band 主色和控制清单图标弱强调，`parseStatus=invalid` 使用告警色和告警图标弱强调。收起态只保留单行控制条，展示标题、路由语义和 artifact 名称，不展示 JSON 预览；展开后在控制条下方展示完整格式化 JSON。

## Direct 运行时呈现

- Direct 对用户呈现为一个持续 Agent 对话，不展示 workflow、round/node/attempt path、run outcome、session switcher、重跑或工作流查看/编辑入口。
- Direct 不保留独立运行标题栏；组合页头左侧按内容自然宽度紧邻展示可编辑会话标题、Agent icon/名称、可复制 sessionId，右侧通过独立操作区展示原始帧与目录按钮。紧凑态标题不预留隐藏编辑图标的布局宽度，标题只在过长时截断，不得用伸展布局把 Agent 身份推向中部；页头不重复展示 model 或 permission mode，`runId` 只保留在高级诊断数据中。
- Direct ACP header 不展示“系统提示”按钮；Direct 的 system prompt 本就为空，不保留无效或禁用态入口。原始帧与其他诊断能力继续保留。
- 消息、thought、plan、tool call、permission、elicitation、附件、raw frame、token、cost、context 和耗时继续复用现有 ACP/prompt-kit 管道。
- composer 内的发送中、思考中、工具执行中、回复中、停止中和计时仍由 canonical lifecycle 驱动，不新增 Direct 专用 chat 组件。
- completed run 上的 follow-up 仍可能存在实时 ACP prompt。后端必须读取 per-attempt `Starting / Running / CancelRequested` 活动状态；只有没有实时活动时，terminal runtime 才能压制磁盘残留的 stale `running` session snapshot。
- 前端的 `sending / awaitingResponse / cancelling` 只覆盖命令往返窗口。页面切换或组件重挂载后，输入锁定、停止按钮、计时和 token 展示必须完全由后端 lifecycle/session snapshot 恢复。
- prompt 写入 terminal session snapshot 前必须先将实时活动标记为 finished，避免终态事件到达后 UI 仍被旧活动状态锁定。

## Agent 单轮回复通知

- 系统通知区分“workflow run 完成”和“ACP prompt turn 完成”。普通 Workflow/AUTO 的自动运行完成继续使用“任务完成”；Direct 首轮、Direct 后续追问，以及 Workflow/AUTO 节点完成后的手动追问统一使用“{Agent} 回复完成 / 回复失败”。
- 手动追问通知只覆盖 `submit_conversation_prompt -> acp-prompt` 的非 runtime continue 路径。停止/异常后的 runtime continue 仍属于 workflow 生命周期，由既有 intervention/run completion 通知表达，不能同时再发一条 Agent turn 通知。
- 每个手动 prompt 在进入 ACP 前必须拥有稳定 `turnId`。前端未提供时由后端生成并写入 prompt event；通知去重键必须包含 `run / round / node / attempt / turnId`，同一 attempt 的连续追问不能互相去重。
- turn 终态统一为 `Completed / Failed / Cancelled`。Completed 和 Failed 产生通知；用户主动停止对应 Cancelled，不产生完成或失败通知。adapter transport interrupted 属于 Failed，不得伪装为用户停止。
- Direct 首轮仍由内部单 Worker run 驱动，但 `RunCompleted` 事件必须携带从 `authoring/conversation.json` 固化的 Agent 展示身份。通知订阅器不得根据 `direct-agent` 等节点 ID 判断 Direct，也不得依赖当前 UI 工作区回读元数据。
- 当前窗口失焦、最小化、隐藏，或用户正在查看其他 task/run/session 时发送通知；当前目标 session 正在前台可见时抑制通知。permission 与 elicitation 继续沿用即时通知，不等待 turn 结束。
- 通知正文不包含 Agent 回复原文、工具参数或附件内容，避免在操作系统通知中心泄露会话正文；点击“查看详情”仍定位到对应 task/run/attempt。

## ACP 斜杠命令目录与输入交互

- Gold Band 将 Agent 通过 ACP `available_commands_update` 公布的条目称为“ACP 原生命令”。ACP 没有标准 SKILL 发现接口，因此该列表不是最终命令目录；Doctor 还要从当前 Agent 的 Skill 读取目录扫描用户级与 workspace 级 `skills/*/SKILL.md`，只解析 `name / description` 元数据，不读取正文，也不进行 prompt injection。
- 每个 Agent 统一维护 `AgentSkillDirectoryPolicy { writeDirNames, readDirNames }`。写列表只定义 SKILL 管理同步目标；读列表定义 Agent 实际发现位置。默认策略为 Claude `.claude -> .claude`，Codex `.codex -> .codex + .agents`，Cursor `.cursor -> .cursor + .agents`，Gemini `.gemini -> .gemini + .agents`，OpenCode `.opencode -> .opencode + .agents`；`skillsDirOverride` 替换主读写目录，非 Claude Agent 仍追加 `.agents` 兼容读取。
- 最终目录按 `ACP 原生命令 > 读取目录发现的 SKILL` 合并，并以命令名不区分大小写去重。ACP 条目保留 `description / inputHint`，SKILL 条目使用 frontmatter 的 `name / description`；扫描支持 Agent 根目录下的 `skills` 以及 `.codex/skills/.system` 这类一层容器目录和 Skill 符号链接。
- 命令目录的数据模型为 `AcpCommandCatalog { agentType, workspaceKey, acpCommands?, commands, updatedAt }`，其中 `acpCommands` 保存未混入 SKILL 的原始 ACP 列表，`commands` 保存最终列表，命令项为 `AcpCommandItem { name, description, inputHint? }`。目录必须以 `{agentType, workspace}` 为联合身份，因为同一 Agent 在不同 workspace 可发现不同的项目级 SKILL；查询目录时从原始 ACP 列表重新扫描，保证 Skill 增删不会被旧合并结果残留。
- 桌面端把目录持久化到 `~/.gold-band/desktop/agent-command-catalogs.json`。自动 Agent doctor、手动 doctor、活跃 ACP 会话的 `available_commands_update` 都会更新原始 ACP 列表并重建最终目录；SKILL 创建、删除或同步目标变更成功后异步刷新当前 workspace 的已配置 Agent，不阻塞 SKILL 保存链路。旧目录文件没有 `acpCommands` 时兼容读取，并在下一次 Doctor 后迁移为可精确重扫的新结构。
- doctor 的 `session/new` 与命令通知存在并发窗口。连接层必须为尚未注册 route 的 session frame 提供有界、带 TTL 的早到缓冲，并在 route 注册时按序补投；不得依赖固定 sleep 掩盖消息丢失。doctor 在 session 建立后只追加一个有上限的命令发现等待窗口，随后立即清理诊断 session。
- 快速对话仅在 Direct 或固定 Agent 的 AUTO 模式中展示该 Agent 的目录；动态 AUTO 和尚未解析 Agent 的 Workflow 不展示 Agent 专属命令。会话详情页使用当前 ACP session 的 provider 与 provider cwd/workspace 查询目录。
- 输入内容仅匹配独立的 `/query` 时打开菜单，命令字符支持 Unicode 字母/数字以及 `.`、`_`、`:`、`-`，因此中文 Skill 名不会被目录或输入过滤丢弃。标签解析必须先读取最长合法命令 token，再检查其后的首字符是否为分隔符；不得因 `-`、`.`、`:` 同属 Unicode 标点而回溯成较短命令。输入空格、`,`、`，` 等分隔符后匹配立即失效并关闭菜单；若分隔符前是当前目录中的完整命令，则输入区把该命令前缀投影为标签。标签绝对定位在首行，通过共享 `ResizeObserver` hook 测量真实宽度并只设置 textarea 的首行 `text-indent`；textarea 自身始终保持完整宽度，因此显式换行和自动折行从输入区左边缘开始，不得形成贯穿所有行的标签列。标签与 textarea 首行共享基于 `rem` 的排版节奏和顶部基线，不依赖物理像素，随系统缩放、窗口 DPI 与根字号变化；颜色只使用 `secondary / secondary-foreground / border` 语义 token，摘要通过共享 shadcn Tooltip 展示。分隔符与后续正文继续由原生 textarea 编辑。删除分隔符、破坏命令名或切换到不含该命令的 Agent 后立即恢复普通文本，再次形成“完整命令 + 分隔符”时重新标签化。方向键移动时选中项必须跟随可见滚动区域；Esc 或点击菜单外关闭，但保留输入中的 `/`，用户删除并重新输入后可再次打开。选中后写入 `/${name} `，标签只属于前端显示投影，发送给 ACP 的值仍是完整普通文本。
- 菜单使用共享的 shadcn `Popover + Command` copy-in 组合，`CommandList` 是唯一滚动容器；命令名、描述、输入提示使用紧凑的小字号层级，`inputHint` 使用弱化标签而不是与描述拼成一段文本。键盘与鼠标选中态统一由 cmdk `data-selected` 驱动，并同时使用透明背景、内描边和左侧短强调条三层主题语义信号；浅色主题使用低透明度 `primary` 蓝色，深色主题使用 `foreground` 叠层，保证风格统一且均可辨识。切换 Agent/workspace 时以目录联合身份隔离快照，旧 Agent 的命令不得在新目录加载期间闪现。
- 快速对话的命令列表使用同一 `Command` 内容，在首行输入下方以带圆角的绝对定位覆盖层展开，不参与 composer 高度计算，因此打开菜单时主输入框整体尺寸保持不变；列表左右边缘与快速对话主输入框外边缘对齐。会话详情继续使用 composer 上方的 Popover，并以 Radix anchor 实际宽度作为菜单宽度，使两侧与 composer 严格对齐。
- 用户通过 Esc、点击外部等方式关闭当前 `/query` 后，关闭状态按稳定的 `{agentType, workspace}` 目录身份与当前输入值保留在前端运行期；切换页面再返回不会因为组件重新挂载而重开。输入值改变或删除后允许重新触发；切换 Agent 时清除新 Agent 上的关闭状态并展示其命令。
- 方向键改变选中项后，菜单通过直接持有的 `CommandList` ref 调整唯一滚动容器的 `scrollTop`，选中项位置只相对该容器计算并执行最小滚动；不得叠加第二层滚动组件、动态查找 DOM 父节点或使用跨父节点的 `offsetTop`。
