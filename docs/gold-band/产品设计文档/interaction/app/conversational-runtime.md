# 会话式运行时

## 信息架构

会话运行时窗口是用户与 agent 交互的核心区域。左侧选中最小单位是 run，右侧主区域永远展示当前选中 session 的具体对话。

## 上下文压缩状态

上下文压缩属于 ACP 运行阶段，不是 assistant 普通消息：

- 消息流使用无头像的轻量结构化行，保留 assistant 结构行的横向位置；不使用大卡片、嵌套面板或粗边框。
- running 状态展示“正在压缩上下文”、压缩前占用、已耗时和不定进度动画；动画必须遵守 `prefers-reduced-motion`。
- 运行超过 120 秒后展示“耗时较长，仍在等待 Agent”，但仍不得伪造失败或百分比。
- completed 状态原位更新为“上下文压缩完成”和总耗时。压缩条目可以继续只展示压缩前占用与窗口上限；会话底部的“上下文窗口”在 runtime 观察到 reset 后首个有效正数时切换为 compact 后 ACP 当前上下文占用。reset 过程中的 `used=0` 不进入 UI，尚未获得有效值时保持上一次确认值或展示 `--`。
- interrupted 状态展示“上下文压缩已中断”，并由既有 ACP/runtime terminal 生命周期解除 composer 锁定。
- composer 在 active compaction 期间将泛化 processing kind 切换为 `compacting`，文案为“正在压缩上下文”，停止按钮继续复用现有会话停止语义。
- 状态变化使用 polite live region 提供无障碍播报。

运行态身份以 `projectId + taskId + runId + session locator` 为后端操作定位；前端 ACP 消息窗口、乐观事件和事件分页缓存必须额外使用 task 生命周期 namespace（优先 `TaskState.uuid`）隔离。`taskId/runId/roundId/nodeId/attemptId` 是目录内可复用编号，用户删除最高编号 task 后重新创建会再次出现同一组编号，因此不能单独作为 UI 内存缓存身份。会话模式中查看、继续、停止、权限响应、模型/权限配置、raw frames、产物/附件读取都必须作用在该 `projectId` 对应 workspace；查看历史 run 不提升最后活跃 workspace。只有成功创建或重跑产生新 run 后，该 `projectId` 才成为最后活跃 workspace，并在从会话模式切回工作台时同步为旧 UI 当前 workspace。

## 运行时数据与内存边界

- `acp.timeline.jsonl` 是会话展示事件的规范索引，采用“canonical base + append patch journal”模型；活动 runtime 内存只保存当前 text/thought/plan 累计流、未终态 tool call、未决 permission/elicitation、session metadata、usage 与 timing aggregate。完成并已持久化的历史事件必须立即从热状态释放，不能按完整会话历史常驻 `HashMap`。
- 根会话与每个 Agent execution 是统一的 `ConversationBranch`。根分支写入 `acp.timeline.jsonl`，Agent 分支写入 `agents/<稳定 AgentExecutionId>/timeline.jsonl`；`acp.agents.jsonl` 与分支 snapshot 保存父子关系、生命周期、统计、attention 和最新 cursor。每个规范事件只属于一个分支，Agent 正式文本、TODO、工具与权限不得复制回父分支。每个分支只投影 `parentAgentExecutionId` 指向自身的直属 Agent；根分支只投影 `parentAgentExecutionId = null` 的顶层 Agent，后代必须在父 Agent 会话中逐级出现。
- Claude `_meta.claudeCode` 只允许在 ACP 适配边界转换为内部关系；前端与持久化查询只消费 `_meta.goldBandConversation` 和稳定 branch ID。旧 `acp.events.jsonl` 只允许一次性迁移并保留为审计文件，迁移后运行时、权限、elicitation 和查询不得继续双读或双写。
- usage aggregate 必须区分上下文窗口 gauge 与 Token 消耗 counter：`used/size` 归 Provider ACP session，表示 Agent ACP 最新确认的当前上下文；同一 session 被新 Gold Band attempt continue/resume 时，必须先从 `continue_ref.snapshotFile` 继承上一 attempt 的最后有效 gauge，再由新 Provider 观测直接替换，不能把多个 attempt 的 `used` 做算术相加。compact 后直接切换为 compact 后值；`input/output/cache/total` 归当前 attempt，表示 Provider 返回的累计消耗。瞬时 `used=0` 只属于 raw 协议采样或 compact reset 边界，不能覆盖 canonical timeline、snapshot 或 UI。
- timeline 使用既有 item/patch 格式持续追加，旧文件继续可读；压缩阈值只统计上次 canonical rewrite 后新增的 patch 字节量和 patch/item 比例，不能因 canonical base 自身超过 8 MiB 而在每次 upsert 后重复压缩。压缩完成后 patch 计数和 patch 字节预算同时归零。`AcpTimingState` 按交互 ID 增量观察 permission/elicitation 终态，不能为一次交互复制整份 timeline。
- `AcpUiEvent.raw` 内单个超过 64 KiB 的 terminal output、diff old/new text 或其他大字符串必须写入 attempt 级 `acp.file-blobs`，timeline 只保存带内容 hash 的 `$goldBandBlob` 引用。会话分页只在最终返回的事件窗口中还原 Blob；工具详情接口按需还原完整内容，会话树、lifecycle、统计扫描和未选中会话不得读取 Blob 正文。
- 会话树刷新只读取 attempt 的 session metadata 与 lifecycle 摘要；没有显式 `selectedSessionKey` 的 `get_conversation_run` 只返回默认 key、会话树和 run 摘要，`selectedSession` 保持空，由会话详情分页接口独立加载。每个 leaf 必须同时返回轻量 `sessionEstablished` 事实和可用的真实 ACP `sessionId`：worker ref 已持有 ACP session id，或非空 snapshot/session/timeline 已落盘时为 established；只有 outbound `session/new` 的 raw 帧不表示建连成功。前端不得把 summary-first 的 `selectedSession=null` 推断为初始化中断。非选中会话的超大或不可读 timeline 不得阻断 run/session tree 刷新；选中会话仍保持既有事件窗口配置、cursor 和终态覆盖语义。所有磁盘投影构建运行在 blocking worker pool，不占用 Tauri async/UI 调度线程。
- 共享 ACP stdout reader 属于传输控制面，绝不能等待某个 session consumer。reader 解析一帧后先公平分发 JSON-RPC response/cancel control，再以非阻塞方式投递 session route；每个 session event pump 仍按 4 MiB / 256 帧窗口消费。session ingress 具有独立 64 MiB / 16,384 帧硬上限，单 session 超限只关闭该 route 并报告过载，不阻塞共享 reader、其他 session 或 RPC pending response；同一 session 内仍保持 FIFO，不合并、不重排。
- ACP prompt 终态必须以 session route 的有界收敛为边界，不能把 JSON-RPC response 已到达等同于相关 session update 已被 runtime 观察。每个 session route generation 为入队帧分配单调 `routeSeq`；stdout reader 分发 `session/prompt` response 时，附带该 session 当时的 route watermark。runtime 收到成功 response 后先消费同一 generation 至该 watermark，再执行 200 ms quiet drain，以捕获 response 后紧随到达的 ACP 标准 chunk；quiet drain 与水位等待共同受配置化 `acpPromptTerminalRouteTimeoutMs` 约束，持续事件、route 关闭或 generation 不一致必须返回结构化传输收敛异常，绝不能无限等待或静默认定成功。`routeSeq` 只存在于 connection → event pump → runtime 的内存控制面；timeline `seq / endedSeq` 与前端 `headSeq / afterSeq` 继续表示 canonical UI event 的展示 revision，二者生命周期不同、不得互相替代。
- ACP 的 `messageId` 只表示 provider 是否为 `agent_message_chunk` 提供稳定消息身份，不天然等价于正文或错误语义；Gold Band 不读取 Codex、Claude 等 provider 私有 `_meta` 来猜测通用终态，也不按 adapter ID 或文本内容分支。所有 agent 文本（含无 `messageId` 的 chunk）都必须写入 canonical timeline 并展示在 Agent 消息气泡。无 output contract 的 turn 与 `PostTurnProjection` 的可见业务 turn 使用 `Conversation` 策略：无 ID 文本是正常可见回复，不显示异常横幅、不触发 Gold Band 自动重试。`InlineControl` turn 与后置投影的 finalize turn 才使用 `ArtifactContract` 策略：artifact/schema 候选只读取有非空 `messageId` 的输出；若该控制 turn 只有无 ID 文本，则文本仍进入 canonical timeline 和既有消息展示，同时以 `provider.acp-unidentified-agent-output + Manual recovery` 进入 `RuntimeAbnormal`，不进入自动重试或 invalid-output repair；若已有合法有 ID artifact，无 ID 文本仅作为补充，不污染 artifact 且不阻断成功。JSON-RPC error、transport error 等 ACP 明确结构化错误继续沿用既有归一化与自动重试策略。未来 ACP 提供标准 notice/error 语义时，只替换统一输出分类接口，不改变 timeline 展示和 output contract 边界。
- `PostTurnProjection` 的两个 turn 复用同一 ACP session：第一个 turn 是用户可见的正常业务回复；第二个 `RuntimeFinalize` prompt 对用户隐藏，timeline 原始 prompt 事件使用 `reason=artifactFinalize`；agent 返回的 canonical artifact 继续按既有 runtime control 折叠条展示。若 artifact 校验失败，后续隐藏 repair prompt 使用 `reason=invalidOutputRepair`，只修复控制结果，不得让 agent 重复业务工作。业务 turn 与 finalize 之间的 `artifact-emission.json(finalizing)` 是 UI 重进、停止后继续及进程恢复时的阶段事实；存在该状态时 composer/runtime 恢复的是控制归一化阶段，而不是再次发送任务 prompt。
- ACP prompt 输出投影与 canonical timeline stream 的字符预算必须由生命周期状态同时持有累计字符数；每个新 chunk 只扫描自身一次并按 UTF-8 字符边界追加，禁止为检查上限反复遍历已经累计的完整字符串。该优化将“上限检查 + 有界字符串追加”步骤由最坏 O(n²) 收敛为整体 O(n)，不改变既有累计快照、字符上限、截断表现、artifact 候选段或 timeline 展示语义；runtime 从持久化 timeline 恢复开放 stream 时允许对已有内容执行一次字符计数，后续继续增量维护。
- 桌面进程共享单一 `RuntimeLifecycleBus`，metrics、notifications、conversation-run-state 使用固定具名幂等订阅并只在 setup 注册一次；创建、重跑、继续与 prompt 路径不得重复挂载订阅。
- 会话侧栏仅将活跃态动画绑定到既有身份/状态载体：进行中的 Workflow/AUTO run 使用 `gold-running` 蓝色圆点低强度呼吸；进行中的 Direct 会话让既有 Agent icon 低强度呼吸。动画必须遵守系统 reduced-motion 设置，禁止使用旋转外圈或标题文字动画；暂停保持既有黄色静态点，成功保持既有绿色静态点，失败保持既有红色静态点。
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

### 流式重渲染下的组件稳定性

- 会话消息的实时 flush 可以触发父级连续重渲染；顶部标题、Tooltip、菜单和按钮等静态交互组件必须在父级高频更新时保持 ref identity 与内部状态收敛，不能由 ref detach/attach 反向驱动无限状态更新。
- shadcn copy-in 组件统一从项目选定的 `radix-ui` 聚合包消费 primitive；不得同时保留没有源码消费者的 `@radix-ui/react-*` 直依赖，也不得让同一种 Slot/Tooltip primitive 在 lockfile 中由多套直依赖版本共同拥有。
- Radix 升级必须保留 shadcn copy-in 层的可定制代码、键盘交互、ARIA 和 focus 管理，不通过删除 Tooltip、移除 `asChild` 或增加特定流式条件分支规避组件生命周期问题。
- 回归验收必须使用真实 DOM 挂载 Tooltip trigger，在 trigger 打开后连续更新父级输入，确认触发器仍挂载且不会产生 `Maximum update depth exceeded`。

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
- 会话编辑或修复工作流保存时，必须一次提交 Task 作者态 `WorkflowDsl` 与整份 `WorkflowModelBindings`；Agent、模型、权限和 config options 的修改不得因会话页回调链路丢失
- 修改只影响未来 run，不影响当前 run snapshot

## Session Switcher

- 位于会话窗口顶部信息展示区
- 显示路径如 `round-001/dev/attempt-002`
- 当前选中 session 的顶部 trigger 也显示同一枚状态标记，与下拉树中的 attempt 行保持一致
- 点击展开 round → node → attempt 层级树
- 用户可切换具体 session
- 每个 attempt 前仅显示轻量状态圆点，颜色只来自后端 `runtimeDisplay.tone`：绿色成功、红色失败/错误阻塞、黄色暂停、灰色待处理/未知；运行中统一使用 `gold-running` 蓝色圆点的低强度呼吸动画，并遵守 reduced-motion，不叠加另一套外圈 ping halo。
- 已选中的 session 行仍保留同一枚状态标记，不能因为选中高亮而丢失运行态/结果态识别
- `status / outcome / pauseReason` 只作为运行事实字段保留；Session Switcher、顶部选中栏、工作流查看 Sheet 不在前端自行推断成功/失败/暂停，而是统一消费后端派生的 `runtimeDisplay.code / tone / icon / terminal / resumable / reasonCode`
- `completed + outcome=null` 不展示为成功；成功必须来自 `outcome=success` 派生出的 `runtimeDisplay.tone=success`
- AI-DYNAMIC 内部节点的 session 状态来源于 dynamic graph 中的节点状态（`dynamic/nodes/<node-id>/node.json` 或 `graph.json.nodes`），ACP attempt 目录只代表聊天会话记录，不作为工作流节点成败状态来源；live running graph 中不得用旧的 `acp.session.json/acp.snapshot.json=cancelled` 反向覆盖 running sibling 的工作流状态，只有父级/dynamic graph 已暂停的历史坏状态才允许做 legacy recovery
- 当 runtime attempt 已因 `process-interrupted / runtime-abnormal / waiting-for-user-input` 进入可继续暂停时，session tree 与 composer 的用户态状态必须继续展示为可继续暂停；其中 `runtime-abnormal` 使用危险色/异常图标提醒，但 `blockingError=false` 且可输入继续。当 runtime attempt 因 `error-blocked` 暂停时，session tree 必须保留错误阻塞状态并由 composer 展示 `runtime-error`；此时 ACP snapshot/session 被写成 `failed` 或 `cancelled` 只代表底层会话传输已结束，不能覆盖 runtime 的暂停或错误事实
- 对 Workflow/AUTO，`process-interrupted` 的可继续会话语义只适用于 ACP session 已建立、或已经存在可展示 timeline/metadata 的 attempt。前端必须使用会话树 leaf 的 `sessionEstablished` 与详情响应判断，不能因为摘要接口有默认 `selectedSessionKey` 但没有内嵌 `selectedSession` 就提前判定中断；established leaf 必须先走分页详情加载并恢复历史。若编排 runtime 已停止且 ACP 初始化从未建立 session（无 `sessionId`、无可展示 metadata、无 timeline item），会话区域才展示无图标的单行状态“会话发起中断，请重跑该任务”，隐藏 composer，不提供继续或重试入口，也不得继续执行首次会话 readiness 重试。只有 outbound `session/new`、最终以 disconnected 结束的 attempt 属于未建立。Direct 是例外：它没有 Runtime continue，停止落盘的 cancelled shell 始终保留普通 composer，下一条消息可重新建立 ACP session。原始帧与 diagnostics 仍保留在高级诊断入口中。
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
- 历史 AUTO dynamic graph 的 workspace catalog 迁移在存储边界执行；读取 Git HEAD 失败时仅使用稳定的 legacy 占位提交身份继续生成只读历史投影，且该失败分支必须遵守 Git 服务的 `Result` 错误契约，不改变 graph/session 身份或迁移幂等性。
- 新会话从会话式主页发起后，run 创建命令只负责落盘 task/run 初始状态并后台启动执行；前端收到该 run 的第一个 ACP live event 后必须立即刷新 session tree，插入对应 attempt，选中该 session，并把右侧详情切到该 session。后续同一 attempt 的普通流式消息由 ACP 会话详情订阅直接合并，不依赖整页轮询；后端应具备向前端推送完整 session snapshot 的基础通道，但当前自动 workflow 只在 run completed 完成态落盘后额外推送 terminal session snapshot，当前已选中 session 的 terminal session snapshot 仍必须触发 run VM 刷新，避免最后节点没有下一跳事件时父级 lifecycle 停留在 active。
- run 已进入 `running` 但首个 attempt 尚未出现在 session tree 前，右侧主区域显示 `Agent 调起中` 状态，不回退为“暂无活跃会话”。attempt 已出现在 session tree 但尚无可见 thought/text/tool timeline item 时，消息主区域显示 `处理中...`；收到首个 thought 后自然切换为 `思考中...`，避免创建 session 后到首 token 前出现空白。会话式运行页必须把当前 attempt 的外层 runtime status 传入 ACPChatDialog，不能只依赖 ACP snapshot/session status；当前选中 attempt 运行中时必须展示阶段状态和停止按钮。Workflow/AUTO 运行中继续禁用输入；Direct 从首次 prompt 的本地“发送中”开始保持输入可用，后续提交进入 attempt 持久化队列。当前选中 attempt 已结束时恢复正常追问输入且不显示停止按钮。

### 会话元数据展示

会话窗口 header 中的模型名称/选择器、权限模式标签和系统提示词按钮依赖于完整的 `AcpSessionVm` 元数据（`config.currentModelId`、`config.currentModeId`、`systemPromptAppend`）。为保证这些信息在实时流式开始后即可见：

- **后端 session-ready 快照**：provider 在 ACP `session/new`、`session/resume` 或 `session/load` 完成后，必须先把 Gold Band synthetic user prompt 写入 timeline，再写 `acp.snapshot.json` 并通过 `acp_session_update_emitter` 发送完整 `AcpSessionVm`，最后才开始真实 `session/prompt` 流式输出。首个可见 snapshot 必须同时具备 `systemPromptAppend`、模型/权限配置和首个用户消息，避免首屏先渲染 agent thinking。
- **ACP 恢复策略**：runtime 必须从 `initialize.agentCapabilities` 读取 `sessionCapabilities.resume` 与顶层 `loadSession`，不得按 provider id 猜测。已附着且配置指纹有效的 runtime 直接发送 `session/prompt`；脱离进程后的普通续接优先 `session/resume`，resume 未声明而 load 已声明时才以 `session/load` 降级；启用外部会话同步时必须使用能够回放完整历史的 `session/load`。严格 continue 且两项能力均未声明时返回 `acp.session-restore-unsupported`；需要外部历史同步但仅支持 resume 时返回 `acp.history-sync-unsupported`；非严格恢复且两项均不支持时才允许创建新 session。
- **恢复期会话壳单调性**：attempt 一旦已持久化 ACP session 身份，`sessionEstablished / sessionId` 在同 attempt 快照合并中只能保持或补全，不能被 resume 期间的临时空 payload 降级。`session/resume` 、本轮 prompt 提交或 run 快照刷新期间，即使详细 `AcpSessionVm` 短暂缺失，前端也必须由已建立会话引用构造可展示壳，保留 composer、队列和已缓存 timeline；只有明确的恢复/初始化失败才进入 ACP 错误页。
- **lifecycle-only update 与 session snapshot 分离**：ACP event 中 `lifecycle` 存在而 `session` 缺失表示只更新 runtime/composer/队列，不表示 session 被清空。前端只能在收到非空的 authoritative session snapshot 时替换 `currentSession`；这条规则对 Direct 队列的 lifecycle 回执、停止、permission 与普通 ACP lifecycle 更新统一生效。
- **turn-finished callback 统一装配**：创建首轮、重跑、runtime continue 和 same-session prompt 必须经由同一 runtime callback assembler 装配 ACP live event、session snapshot 和 `prompt_turn_finished` callback。turn 成功、失败或取消只能从此回调产生一次终态后继处理；Direct 调度器在其内部判断模式，Workflow/AUTO 保持 no-op。
- **恢复状态机**：`session/resume` 使用 `RestoringWithoutReplay -> AwaitingTurnStart -> Live`，不得启动 Provider history importer 或 replay quiet-drain；恢复响应前若 Agent 违规发送历史内容，只保留 raw frame 并以结构化诊断抑制。`session/load` 使用 `ReplayingHistory -> AwaitingTurnStart -> Live`，保留 history importer、load response 后 quiet-drain 和 prompt 前二次 drain，以防异步 adapter 的迟到 replay。两条路径都必须先捕获响应中的 `models/modes/configOptions`，再依次应用模型 override、权限模式 override 与其他 config option，最后才发送 prompt。
- **系统提示词来源**：新 session 的 `systemPromptAppend` 属于 snapshot metadata，`acp.raw.jsonl` 只作为旧历史 session 的 fallback 和协议排障事实源；前端不直接解析 raw 来展示系统提示词。
- **系统提示词长内容展示**：系统提示词弹窗必须支持完整展示任意长度的 profile/runtime prompt，不截断角色正文。弹窗采用固定标题栏与单一正文滚动区域：shadcn/Radix Dialog 负责视口高度上限和裁切，直接 flex 子级滚动容器负责全部纵向滚动并使用 Gold Band 统一滚动条样式；不得在仅有 `max-height` 的 Dialog 内使用依赖百分比高度的 Radix ScrollArea viewport，否则正文会随内容增长后被外层裁切。正文 `<pre>` 只负责保留换行与对长路径、连续字符进行容器内断行，不得再创建嵌套滚动层。
- **前端初始化 readiness fetch**：前端订阅会话更新后只在初始化阶段调用 `getAcpSession` 等待 session-ready 快照；等待窗口必须覆盖 ACP provider 慢启动场景（例如 `initialize + session/new` 超过 10 秒），不能用 2 秒级短重试提前放弃。若 snapshot 已具备系统提示词、配置枚举和首个 Gold Band 用户消息，立即进入实时渲染。live event 到达时不得反复触发 metadata hydration；实时阶段缺 metadata 属于后端未按 session-ready 契约发送完整 `AcpSessionVm` 或初始化 readiness 等待窗口不足，应修复事实来源链路，而不是由前端事件流补拉掩盖。
- **event-only shell**：`createLiveAcpSessionShell` 只在调用方显式允许 event-only fallback 时创建临时渲染壳，不作为稳定元数据来源；壳中不含 system prompt 与 model/config 字段。会话式运行页必须关闭该 fallback：若继续会话启动早期只扫描到 timeline/live event、尚未拿到包含 `systemPromptAppend`、配置枚举和 Gold Band 用户消息的 ready session，主区域继续显示初始化 loading，而不是把 partial session 渲染成“无 session id / 无系统提示 / 无权限模型”的运行会话。
- **运行中重进显示**：若重新切回一个 running session 时已经存在 base `AcpSessionVm`，且当前 session event window 已包含可展示的 thought/text/tool/plan/permission/elicitation 等 timeline 事件，即使完整 metadata 或最早的 Gold Band synthetic user prompt 已不在当前分页窗口，也应退出 loading 并渲染当前消息流，同时停止 readiness 轮询。纯 metadata/config update 事件不得解除 loading gate，避免把尚未形成可见对话的 early session 渲染成空会话。
- **session leaf 归属**：`AcpSessionVm` 必须携带 `roundId/nodeId/attemptId` 与可选 `outerNodeId/outerAttemptId`，前端优先用这些稳定身份判断选中 session 是否属于当前 leaf；`cwd` 表示 Gold Band session attempt 存储目录，只作为旧数据 fallback。provider 真实工作目录单独放在 `providerCwd`，不能覆盖 `cwd`，否则 dynamic continue session 会被误判为不属于当前 leaf 并进入初始 loading。
- **authoritative session ref**：前端用于异步回调的最新 session ref 只能由明确数据入口维护（外部 session prop、identity 初始化、subscription session、initial fetch、permission/stop/model response、live-only timing patch）。`effective` / visible session / optimistic session 等 UI 派生态不得反写 ref，否则后端已推送的 session-ready snapshot 可能被旧 render 中的 event-only shell 覆盖。session payload reducer 不能仅因 ref 等价就跳过 React state 同步；ref 是异步事实缓存，`currentSession` 展示态必须用自身当前值再做一次等价判断，确保 UI 最终追上 session-ready payload。同一 ACP session 内，`systemPromptAppend`、模型/权限配置和 Gold Band synthetic user prompt 是 session-scoped metadata；后续 live timing、分页响应、空外部 prop 或 event-only shell 只能作为 patch 合入，不能把已就绪 metadata 降级为空。
- **可见事件合并**：base session 一旦存在，消息流必须按 `AcpSessionVm.events + loadedEvents` 合成可见窗口；`loadedEvents` 只是实时/分页事件窗口，不能直接替换 session snapshot 中的事件。Gold Band synthetic user prompt 可以只通过 session-ready snapshot 到达，因此前端必须保留 snapshot prompt 并继续合并后续 live event。
- **逻辑 prompt 的双重身份**：同一个逻辑 prompt 的可见 timeline 事实与 provider 调用记账必须分域。`acp.snapshot.json.promptRetry` 持久化 canonical user event 的 `id / seq / timestamp / hiddenFromChat`；普通 worker、Direct 与 AI-DYNAMIC 在 runtime 自动重建时按 `promptId + visibility` upsert 同一个事件，不得为每次 retry 新增用户事件。visible prompt 与 hidden repair 即使共享 `promptId` 也属于不同 lifecycle，hidden 事件不能覆盖可见气泡。每一次真实 `session/prompt` RPC 另生成独立 usage transaction ID，provider 返回有效 usage 时分别累计，不能因 timeline event 去重而漏算失败尝试的真实消耗。
- **prompt lifecycle 单调合并**：retry 被调度后 canonical 用户事件进入 `processing + retry`，新 runtime 首帧必须继续该状态，不能短暂重置为 `completed`；成功终态清除 retry footer，失败与用户停止分别固定为 `failed / cancelled`。durable prompt cancellation 的所有权属于 attempt 级 `cancel_attempt_prompt()`，该入口必须在 timeline 文件锁内定位最新 `processing + retry` 的 Gold Band 用户事件，分配单调 revision，并以同一 canonical event ID 原子 upsert `cancelled`；因此停止发生在 retry backoff、runtime initialize/session setup 或 active prompt RPC 任一阶段，都不依赖某个 runtime 是否持有 `activePromptTurn`。runtime 重建仍需从已加载 timeline 按持久化 `promptEventId` 恢复尚处于 processing 的 retry 事件，用于运行时内存一致性；普通新 prompt 启动前的历史 completed/cancelled 事件不属于 pending retry，不得被停止入口或恢复逻辑改写，重复停止不得产生新 patch。live event、stop response 与完整 session snapshot 合并时以 `endedSeq -> startedSeq -> originalSeq` 作为 lifecycle revision，旧 revision 或同 revision 的非终态不得覆盖已落定终态。前端按 `promptId` 折叠多个物理事件仅用于旧 timeline 兼容，新数据的正确性不得依赖该折叠。
- **session 等价判断**：`sessionsEquivalent` 必须比较 session config 与 adapter 元数据签名，使后端在启动阶段发出的元数据-only session 快照（事件数可能没有变化）能刷新 UI。模型/权限栏只要存在可选项就应展示，不以 `currentModelId/currentModeId` 是否已归一化作为隐藏条件。

### 自动切换规则
- 上一个 session 完成 + 消息窗口在底部 → 自动切换并折叠历史
- 用户不在底部（正在看历史）→ 不自动切换、不折叠
- 用户通过 session tree 或工作流图入口手动切到任意 session 后，自动跟随立即解除；后续新 running session 只在后台推进，不抢占当前查看中的会话
- 当前选中 session 因 runtime 自然完成而从 active 变为 terminal 时，如果用户仍在底部且未手动切换，session auto-follow 进入 pending 状态；后续同一 run 的新 active child session 首次 live event 或 lifecycle-only active update 到达时可以切换过去。
- session auto-follow 必须以当前选中 session 与目标 active session 的 canonical `lifecycle.control.mode = runtime-controlled` 为前置条件。用户在已完成节点中发起普通追问后，该 turn 属于 `non-runtime-controlled`；即使追问结束且消息窗口仍在底部，也只能继续当前 session 内的消息贴底，不得切换到其他 runtime active session。没有任何当前选中 session 时的初始 active session 选择不属于 auto-follow，不受此门禁影响。
- 自动跟随分为两层：消息列表的贴底 pin 控制当前 session 内流式内容是否滚到最新；session auto-follow 控制是否随 workflow 切到新的 active session。用户滚回当前活跃 session 底部时，恢复贴底 pin 并恢复 session auto-follow；用户滚回历史/非活跃 session 底部时，只恢复当前消息贴底，不切换 session。
- 消息列表贴底 pin 的事实来源必须是“用户滚动意图 + 滚动 viewport + 实际内容尺寸”组成的统一状态机。ACP 主消息区复用 prompt-kit `ChatContainer` / `use-stick-to-bottom`，由内容根节点 `ResizeObserver` 覆盖 timeline 更新、流式 Markdown presentation 帧、图片或折叠内容等真实布局增长；不得再把 `timeline` 变化当成唯一贴底触发器，也不得在业务组件中另存 `pinToBottomRef` 后重复写 `scrollTop = scrollHeight`。任意向上滚动输入都必须立即解除贴底锁，即使视口仍位于第三方组件定义的 bottom-near 阈值内；只有视口真正回到内容底部时才恢复锁，并通过同一 `onAtBottomChange` 信号驱动 session auto-follow。
- `ChatContainer` 首次挂载且 follow 意图为 true 时，必须在包装层 `useLayoutEffect` 内使用真实 `scrollHeight - clientHeight` 同步完成首屏底部对齐，不得依赖第三方 `instant` 动画在首次 paint 后再收敛。缓存历史阅读位置的 DOM anchor/scrollTop 恢复继续在父层布局阶段覆盖默认贴底；业务页不得自行重复写底部位置。
- 贴底 pin 的跟随意图由 Gold Band 的 `ChatContainer` 包装层持有，第三方组件的几何 `isAtBottom` 仅作为滚动执行状态，不能直接覆盖用户意图。没有 wheel、键盘、pointer/滚动条拖动或业务分页/分支恢复 `stopScroll()` 等明确退出信号时，浏览器布局引起的 scroll 与 ResizeObserver 回调竞态不得解除跟随；包装层必须在流式 Markdown 终态重排、turn 文件变更卡插入及其异步详情增高后恢复真实底部。外部 `stopScroll()` 必须统一进入 manual，外部 `scrollToBottom()` 则显式恢复 follow，避免分页锚点与自动贴底争夺滚动位置。
- 用户在贴底状态主动展开 Activity、隐藏运行上下文等会显著增高的会话内披露内容时，滚动容器建立临时 disclosure 会话并暂停 follow，保持触发标题的 viewport 位置，让详情按文档流向下展开。多个同时打开的披露内容共享同一会话；关闭任意一个后只要实际 viewport 已到达底部就立即恢复贴底，无需等待其他内容收起；若始终未到底部，则最后一个收起时恢复。若展开前已经脱底，或展开期间发生 wheel、键盘、滚动条拖动及业务 `stopScroll()` 等明确滚动意图，则披露会话不得在收起时强拉到底部，但用户自己回到真实底部仍正常恢复。临时暂停和恢复必须由 `ChatContainer` 的共享 content-expansion context/hook 统一管理，各折叠组件只持有生命周期 token；不得为 Activity 等具体组件逐层透传专用回调，也不得直接读写 `scrollTop`。
- 用户 prompt 内多个隐藏段应作为同一紧凑 disclosure 组投影到可见正文之前；隐藏段之间以及最后一个隐藏段与正文之间只保留组件 spacing token。解析器保留原始 prompt，展示层合并可见片段并清除隐藏段边界产生的前导空行，不能把模板换行渲染为额外 spacer，也不能改变发送给 provider 的原文。
- 历史会话初始化可以瞬时定位到底部，但该定位必须允许用户逃逸；不得使用 `ignoreEscapes` 等不可中断选项跨越 Markdown、折叠节点或图片的异步布局阶段。发送新消息等由用户明确触发的“查看最新内容”动作可以主动贴底，但后续向上滚动仍拥有最高优先级。
- 用户手动查看历史 session 后，只有再次明确选中最新 active/current runtime leaf 并回到底部，才恢复 session auto-follow；仅把历史 session 滚到底部不能恢复 auto，也不能让后续 background active session 抢焦点。
- 顶部运行中节点 chip 是显式“跟随当前活跃 session”入口：点击 active chip 且消息窗口位于底部时，重新进入自动跟随；live event 到达或完整 run VM 刷新不能单独恢复自动跟随
- 刷新 run VM 时若未满足自动跟随条件，前端必须继续保留当前 `selectedSessionKey` 与当前 session payload，不能因为其他 session 的 live event 或后端默认 selected key 回退到最新 running attempt；若手动切换与已排队的 live refresh 同时发生，仍以最新手动选择为准
- 会话页内“进入 run 时重置自动跟随”的前端 effect 只能绑定 `runId` 等稳定 run 身份，不能依赖父组件每次重建的回调引用；否则 live refresh 触发父组件重渲染后会误把手动关闭的自动跟随重新打开
- 手动切换后是否恢复自动跟随，必须以 `run.activeSessions` 中是否仍包含当前选中 session 为准，不能仅依赖该 leaf 自身的 `runtimeDisplay.tone`，避免树状态短暂不一致时把已完成 session 误判成仍可跟随
- 前端所有完整 `ConversationRunVm` 快照进入 React state 时必须走统一合并入口，不允许调用点直接覆盖；合并入口负责保留当前 selected key、阻止 ACP `unknown` 空快照降级 runtime active 状态，并在 run 仍运行但 activeSessions 暂空时从 selected leaf 补出临时 active session。合并后 `selectedSessionKey` 与 `selectedSession / artifacts / attachments` 必须属于同一个 leaf；若 live refresh 或旧的手动切换请求返回了其他 session 的 payload，前端必须丢弃该 payload，而不是把它套到当前选中 key 上。用户通过 session tree 切换到目标 session 后，目标 `selectedSession` payload 回填前属于详情加载中状态，右侧主区域显示中性加载，不得短暂展示 ACP 会话失败横幅；若目标 leaf 的 runtime 仍 active 但 `selectedSession/effective session` 暂为空，也继续显示同一中性加载态，不展示内部 runtime 状态 key 或“拉起下一节点中”。只有目标 session 详情请求完成后仍确认没有 session/live shell 且 runtime 不再 active，才展示缺失 ACP session 错误。
- 只有一个 session 运行中 → 自动展开该 session
- 多个 session 运行中 → 显示折叠行（session 名 + 实时状态），用户点击进入

## Composer 上下文功能区与引用

快速对话与会话详情的 composer 共用一个位于输入框内部、无独立边框或分隔面的上下文功能区。功能区只承载本次发送携带的引用与附件；没有内容时不渲染，出现第一项后 composer 自然增高，宽度不足时自动换行，最多占两行高度，超出后区域内部滚动，不继续挤压消息视口。

- **引用边界**：只有已经完成的 agent `textDelta` 正式消息正文可引用；用户消息、流式中的 agent 文本、thought、activity/tool、权限申请、elicitation、附件、头像和时间均不可引用。判定以选区首尾实际非空文本节点为准，兼容浏览器整段全选时把 Range 端点提升到消息外层容器的表示；首尾实际文本必须仍位于同一个正式消息正文 DOM 边界内，跨消息、跨折叠区或包含头像/时间等非正文文本时不显示引用入口。
- **多引用草稿**：用户可以按选择顺序添加多个引用；引用与正文、附件归属于同一个 keyed session 未发送草稿，切换 session/branch 后严格隔离并可恢复。相同来源消息中的相同选区不重复添加；单条用户消息最多 64 条引用，所有引用正文合计最多 12,000 字符，超限不改变已有草稿并在 composer 内显示本地化提示。
- **发送与展示数据**：引用不是正文语法，也不得通过 Markdown `>` 反推。发送 DTO 只提交用户亲自输入的 `displayText` 与带稳定 `id + sourceMessageKey + text` 的 `quotes`；引用文字与来源元数据都属于用户可控输入，不构成权限或数据完整性信任边界。后端只校验引用条数、唯一且有界的 ID、有界的来源键、非空正文和 12,000 字符总上限，不加载 timeline、不验证来源是否存在，也不限定未来只能引用消息；随后统一构造 Agent 消费的完整 prompt。前端创建消息引用时仍只允许选区完全位于单个已完成 Agent 正文 DOM 边界，跨消息、Activity、Thought、Tool、Permission 或 Elicitation 不出现引用动作。canonical `userTextDelta.raw.quotes` 与 optimistic event 保存同一结构化元数据，气泡正文只使用 `displayText`。用户自行输入 `> 文本` 仍是普通正文，不产生引用入口；开发阶段旧事件没有元数据时保持原展示，不增加文本推断兼容层。发送时原子分离整份 keyed 草稿；提交失败只恢复到原 session/branch 的空草稿，不得覆盖其他 session 或用户其间的新输入。
- **Agent 消息排版**：透明背景的 Agent 正文不沿用用户气泡的对称纵向内边距；36px 头像与 24px 正文首行使用 8px 顶部、0 底部的非对称内边距对齐中心线，不能依赖气泡整体留白偶然对齐。完成态复制操作区使用固定紧凑高度且始终占位，仅通过 hover/focus 透明度切换显隐。主会话 timeline 与只读 Agent 会话统一使用 4px 语义块间距，正文与操作区使用 2px 组内间距；显隐复制按钮不得改变相邻正文、Activity 或下一条消息的位置。
- **消息引用入口**：带结构化 `quotes` 的用户消息在气泡上方显示“n 条引用”紧凑入口，使用 shadcn Popover 按选择顺序查看详情；正文中不重复展示引用原文。详情只有点击后挂载，弹层最大高度为 `min(24rem, 视口高度 - 4rem)`，标题固定，只有引用列表内部滚动；超长单条文本在同一滚动区换行。Direct 待发送队列展示 `quoteCount`，编辑正文保留既有结构化引用。
- **标签交互**：引用标签显示顺序编号，hover/focus 通过 shadcn Tooltip 查看完整内容，支持逐项删除。整个功能区与输入区共享 prompt-kit `PromptInput` 的背景、圆角、边框与 focus ring，不形成嵌套卡片。
- **Agent 正文复制**：已经退出流式态、非失败且正文非空的 Agent `textDelta` 在正文下方提供复制操作；桌面指针 hover 或键盘 focus 时显示，无法 hover 的触摸环境保持可见，操作区预留固定高度以避免消息布局跳动。复制内容直接使用该消息用于渲染的 canonical Markdown 原文；带 runtime control 的消息只复制剥离隐藏控制协议后的可见正文。Thought、Activity/Tool、Permission、Elicitation、用户消息及仍在流式输出的正文不提供该入口。复制反馈只属于单条消息的局部状态，不提升到会话 timeline 状态。
- **行内语义标签**：Streamdown 渲染的反引号行内内容与所在正文使用相同的 UI 字体、字号、字重和行高，只通过主题 `surfaceHigh` 语义底色、圆角与水平内边距表达标签边界；可点击本地文件标签复用同一层级的底色。标签底色必须直接消费主题的高层 surface，不得再叠加 `muted` 透明度而与会话背景二次混合。不得因行内代码切换到更小的等宽字体，也不得靠额外强边框补偿对比度。
- **围栏代码块**：共享 prompt-kit Markdown 渲染器使用 Streamdown 官方代码块与 `@streamdown/code` Shiki 插件。带语言标记的 fenced code block 在顶部展示声明语言并按该语言高亮，右上角始终提供该代码块自己的复制按钮；复制内容只包含代码正文，不包含围栏或语言标记。一条消息的多个代码块相互独立；未声明语言时保持纯文本，不进行自动语言探测。代码正文保留源码换行与缩进，单行超过消息宽度时在代码块内部自动折行，不产生横向撑宽或横向滚动。代码块复制与消息级 Markdown 原文复制并存，分别满足局部代码和整条回复的复制需求。

### Composer 附件

继续对话时可上传附件作为本轮输入内容：

- **入口**：纸夹按钮、拖拽、粘贴（统一走 same-session 附件模型）；桌面端必须在基础 Tauri 配置和 channel overlay 中关闭原生 WebView file-drop，让文件拖拽进入前端 HTML5 drop zone，拖入 composer 时稳定显示可投放状态
- **预览与布局**：附件从 composer 外部独立区域迁入统一上下文功能区。图片只显示固定方形缩略图，不显示文件名；hover/focus Tooltip 展示文件名和大小，点击沿用右侧工作区图片预览。文本/普通文件显示图标与截断文件名并沿用文本预览。单项删除按钮在 hover/focus 可见，触摸设备保持可见。
- **消息展示**：用户消息下方的图片附件显示为固定尺寸小缩略图，点击进入独立全屏原图预览，不进入附件详情弹窗；文本/代码附件继续显示为紧凑文件 chip 并走附件详情。base64/data URL 只作为内部图片数据承载，不直接作为可见文本展示。消息流附件预览必须按 timeline `raw.attachments[].path` 区分来源：`task-inputs/<name>` 属于新会话首轮 task 输入附件，继续读取 task 级 `authoring/inputs`；`user-inputs/<name>` 属于继续/追问本轮新附件，按当前 session locator 读取该 attempt 下的相对文件。两类附件不得混用读取入口，否则首轮需求附件或完成后追问附件会在 UI 中丢失内容。
- **消息附件布局**：同一条消息中的图片与普通文件必须按媒体类型分为两个独立附件行，图片行在上、文件行在下，禁止混排；同类多项只在各自行内换行。图片保持固定缩略图尺寸，普通文件使用内容宽度的紧凑 pill，不能被消息容器拉伸成大卡片。
- **传输**：新会话初始输入附件只进入 task 级 `authoring/inputs/`，并且只在 `SessionMode::New` 的首次 ACP session 初始化时作为 provider `task-inputs` content block 发送；同一个 ACP session 内的 `continue` / resume 不自动重发 task-level input attachments，避免历史输入在每轮用户消息下重复出现。发送前若附件来自粘贴、拖拽或浏览器 File 对象，前端先通过桌面命令 materialize 到 Gold Band 临时输入附件区，拿到本地路径后再进入对应输入链路。本轮 composer 显式选择的附件属于 resume prompt attachments，只随本轮 same-session prompt 发送。输入附件作为 ACP content block 发送给 agent，不混入 agent 输出产物目录。
- **格式契约**：后端使用同一份附件格式注册表派生可选择扩展名、内容类别（image/text）与 MIME；前端查询到“支持”的扩展名必须都能被 provider resolver 转换为 ACP content block，并同步生成 timeline `raw.attachments` 元数据，不允许维护彼此独立的白名单、MIME 映射和文本类型判断。`.jsonl` 按 `application/json` 文本资源处理。
- **历史投影修复**：对旧版本已经保存于 `authoring/inputs/`、但因旧格式分类遗漏而没有进入首条 timeline 用户消息的 task 输入附件，session ViewModel 在读取 `SessionMode::New` 根分支时按 `task-inputs/<name>` 补齐 `raw.attachments`，按 path 去重且不改写原始 timeline；后续带 `promptId` 的追问消息不得被补入初始附件。
- **AI-DYNAMIC**：AUTO / WORKFLOW 中的 AI-DYNAMIC 内部 worker、merge、acceptance 节点必须与普通 worker 复用同一 task input attachment 数据源；动态节点不得把 `input_attachment_paths` 清空，也不得要求 agent 主动扫描 run 目录寻找图片。

### Composer 尺寸与底部布局

- 会话详情追问输入框默认约两行正文高度，继续复用 prompt-kit 的内容自增高与 320px 有界上限；同时开放浏览器原生纵向拖拽。拖拽结果只作为当前已挂载 textarea 的临时最小高度，后续正文增长仍可继续撑高，切换页面或重新挂载后恢复默认值，不进入 React 页面状态、设置或本地存储。
- 会话详情底部与消息视口使用同一 `background` surface，不设置分界线、半透明遮罩或独立 backdrop。Composer 上方的“当前运行状态 / 会话累计 / 上下文窗口圆环”使用内容宽度驱动的一体式 tab，贴合任务列表、待发送队列或输入框整组的左上边线，不进入正常文档流；tab 取消独立阴影、全圆角和底边，与当前最上方面板统一使用不透明 `card` surface，并由该面板取消左上圆角形成真实连接口。tab 与主 surface 顶边重叠 1px，同色遮罩只负责消除抗锯齿接缝，左边线必须连续延伸到主 surface，不能仅靠浮层覆盖伪造连接；tab 右边线仅在交接点通过凹圆弧转入主 surface 顶边，不形成额外的长曲线肩部。左上角、右上角和交界凹角必须共同消费主题 `md` 圆角 token，保持同一轮廓半径；上下两段圆弧的直径和必须小于 tab 实际高度，在右侧保留可辨识的垂直边线，不能让相邻圆弧重叠成倾斜的 S 形边界。圆弧填充必须继续使用同一个 `card` token，并限制向内容区侵入的范围，为上下文圆环保留独立安全间距，不得退回直角接缝、遮挡圆环或引入第二种近似背景色。tab、连接圆弧和当前栈顶面板统一消费 `composer` recipe 的背景与完整 `border` token；外阴影只由 content rail 按整组绘制结果消费 `--gb-material-shadow / --gb-material-edge-shadow`，tab、队列、任务面板和输入框自身不得重复投影，避免内部接缝出现双重阴影。tab 只遮挡自身宽度范围内的消息，同一水平位置的其余区域继续显示消息内容。tab 最大宽度受 content rail 约束并为右侧圆弧保留对应主题半径的空间，运行状态过长时优先截断，计时与上下文圆环保持可见；不得重新扩展为占满整行的状态栏或独立悬浮胶囊。任务列表与队列使用同一紧凑折叠 surface，多个面板相邻时只保留整组顶部圆角，最下方面板与输入框贴合。
- 附件入口与模型、思考强度、权限配置归入同一底部 command bar，并固定排在配置项左侧。快速对话与会话详情均不渲染“Enter 发送，Shift+Enter 换行”说明行；键盘发送行为保持不变，但不再为提示文案预留垂直空间。附件 action 的可见提示和无障碍名称统一使用 `acp.attachHint` 中英文资源。

## Composer 状态

运行中的状态提示必须放在 composer 上方的紧凑信息栏，不得进入 `PromptInput` 内挤占正文高度，也不能作为消息流卡片。当前步骤状态应展示具体文案：发送中、处理中、思考中、工具调用中、响应中、停止中、Agent 调起中、拉起下一节点中，并固定排在会话累计与上下文窗口之前。工作流阶段只消费 `run.json.execution.phase`：仅 Runtime 已明确提交 `LaunchingNextNode` 时展示“拉起下一节点中”，ACP turn 的 `completed/cancelled/failed` 永远不能推导该阶段。Direct 不消费工作流 execution phase，也不会展示工作流后继节点状态。ACP completed 只表示上一轮 turn 已结束，不表示该会话不能继续追问；用户发起新的 same-session ACP prompt 后，发送中、处理中属于当前进程 live turn 或命令 accepted 前的本地 overlay，不得被旧 terminal snapshot 压掉。旋转标识统一复用 reduced-motion-safe 的 CSS 边框圆环，并由 transform 动画交给浏览器合成；活动摘要与会话信息栏不得各自退回 SVG stroke spinner。

ACP 历史分页能力必须以客户端当前合并窗口为权威，并仅在前端事件缓冲发生真实截断时由缓冲状态补充；分支视口缓存只负责恢复 scrollTop、锚点与是否贴底，不得把旧的 `hasOlder` 重新解释为当前会话仍有历史。完整 session 快照可以替换分页边界，返回 `hasOlder=false` 时必须清除旧分页状态；`afterSeq/afterCursor` 增量响应中的 `hasOlder` 只表示“本次增量之前存在事件”，这些事件可能已在客户端窗口内，因此增量合并只能扩展 newest 边界并继承当前窗口的 oldest/hasOlder，不能凭增量响应重新制造历史缺口。该规则避免流式消息、断线补帧、卡片折叠或布局重算期间闪现“上滑查看历史信息”。

会话累计的口径是当前 ACP attempt 内所有 Gold Band prompt turn 的 agent 净处理耗时之和：每轮从 Gold Band synthetic/user prompt 写入 timeline 开始，到该轮最后一个可观察的处理事件结束；多次继续、恢复、余额错误重试或用户空闲造成的两轮 prompt 之间墙钟间隔不得计入。`available_commands_update`、`current_mode_update`、`session_info_update` 等会话元数据更新不推进处理耗时，`acp.snapshot.json.createdAt -> updatedAt` 只描述底层 ACP session 生命周期跨度，不能作为会话累计的 fallback。

## 系统通知

系统通知只用于用户可能没有看到当前会话页时的关键提醒，不替代会话内状态展示。

会触发系统通知的事件范围固定为：任务完成、Agent 单轮回复完成/失败、权限审批请求、ACP elicitation 提问、节点结束后请求人工判断是否成功、异常中断或错误阻塞。用户主动停止、会话内普通运行中、拉起下一节点中不触发系统通知；当前目标 session 在前台可见时继续抑制 OS 通知。

通知发送前必须判断桌面注意力状态：窗口未聚焦、窗口最小化、窗口不可见，或当前前端页面不是该事件对应的 run/session 时才发送；如果用户正聚焦在 Gold Band 并查看对应 `projectId/taskId/runId/roundId/nodeId/attemptId`，则只更新页面内 composer、session tree 和工作流图，不弹 OS 通知。项目内的 task/run 等编号允许从 `001` 重新计数，因此任何跨 workspace 的通知定位和去重都不得省略 `projectId`。

通知点击导航使用“Rust 持久到取出的待导航队列 + 可丢失唤醒信号”，不把 Tauri event 本身当作业务事实。Windows 正文点击/查看按钮和 macOS/Linux `NotificationResponse::Default / Action("view")` 都先写入包含必填 `projectId` 的 `ViewActionPayload` 队列，清理对应 dedup，再调用桌面生命周期的 `ensure_main_window()`；窗口存在时显示、取消最小化并聚焦，窗口销毁时从 Tauri 配置重建。前端必须先注册 `gold-band://intervention-navigate`，再调用 `take_pending_intervention_navigations()` 原子 drain；同一 dedup payload 在入队和 drain 过程中不得重复。前端直接按 payload 的 `projectId` 加载 workspace，不允许遍历侧栏按 `taskId` 猜测项目。关闭、忽略和过期只清 dedup，不产生导航。

macOS/Linux 原生通知响应适配必须遵守 `notify-rust 4.18` 的借用型 `ResponseHandler` 契约，即只在回调期间读取 `&NotificationResponse`，再映射为 Gold Band 内部的 `Navigate / ClearDedup` 处置，不把第三方枚举的所有权和平台差异扩散到导航队列。响应分类和 `ResponseHandler` trait 约束由跨平台单元测试固化；release 构建前必须在 Linux Runner 执行 `cargo check --workspace --all-targets`，确认非 Windows 路径可编译后才能启动多平台打包。发布编译门禁不得执行全量业务测试，避免与产物可构建性无关的平台断言阻断发版。

ACP 权限请求与 elicitation 提问都必须收敛到统一 intervention notification 机制：runtime 控制下的暂停由 lifecycle 事件触发通知，ACP live event 同时做旁路桥接补齐实时提醒。这样权限请求、elicitation、人工判断、异常中断和任务完成共享同一套去重、点击跳转和前台抑制规则。permission/elicitation 的 canonical event id 必须包含 ACP request id：同一请求的重复 live update 只通知一次，同一 attempt 或 Direct 长会话中的后续请求必须独立通知；elicitation 还必须与 `waiting-for-user-input` 的人工确认通知使用不同 kind suffix。通知展示身份不得暴露 `direct-agent` 等内部 node id：Direct 使用 conversation metadata 中的 Agent 名称，普通节点优先使用实际 provider/Agent 展示名，只有历史数据缺失时才回退 node id。

ACP elicitation 也复用同一条 session event / timeline 管道：`elicitationRequest` 与 `elicitationResponse` 虽然不直接作为普通消息卡片渲染，但必须保留在完整 session timeline 中。后端 `AcpSessionVm.pendingElicitations` 是当前待回答请求的权威投影，与 `pendingPermissions` 同层；它必须从完整 timeline 而不是当前分页窗口生成，并在 session terminal、对应 response 或 stop decline 后清空。前端收到 live request/response 时同步更新同一个 session 字段，composer 直接消费该字段，不得再用 `timing.waitReason`、有限事件窗口或本地 Map 判断一个真实提问是否存在；timing 只负责暂停计时和状态文案。刷新或重进页面后由后端权威投影恢复。回答提交后交互卡片立即消失，不额外合成用户消息气泡；Agent 原生 `AskUserQuestion` 的 `toolCall/toolCallUpdate` 仍按普通工具卡片展示，并保留 completed 状态、关键参数和工具输出。

`elicitation/create` 的协议边界必须使用官方 `agent-client-protocol-schema` 类型反序列化，不能在 runtime 中手工摘取 `message / requestedSchema`。pending signal 与 `elicitationRequest.raw` 保存完整请求，包含 `mode`、scope、`sessionId`、`toolCallId`、`requestedSchema` 和 `_meta`；event 顶层 session/tool identity 从类型化 scope 派生。前端刷新恢复同时识别完整请求形态与历史上仅保存 schema 的 timeline 形态，但新写入事实统一使用完整请求。

ElicitationCard 不从自然语言猜测题目边界。表单级 `message` 必须整体展示并保留换行；字段级 `title` 作为短标签，`description` 作为该字段题干或帮助文本；多题时 provider 的通用 message 可以隐藏。单题没有字段 description 时，完整 message 就是题干，禁止按 `split("\n")` 或步骤 index 截取其中一行。

AskUserQuestion 自定义答案按请求结构关联，不按 Agent 版本号分支：优先读取字段 `_meta._askUserQuestionCustomAnswer.questionId`，其次识别 `question_n_custom -> question_n`；旧版全局 `customAnswer` 与普通文本字段保持独立步骤并原样提交自己的 key，绝不能把任意未匹配文本字段猜成首个选项题的伴随输入。枚举选项的 `description` 与 `_meta._claude/askUserQuestionOption.preview` 保持结构化展示。该 shape-based 规则覆盖 Claude Agent ACP 0.44、0.45.1 及当前版本，升级 Gold Band 后无需同步升级用户机器上的 Agent 才能正确显示旧请求。

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
- 系统提示、产物预览、工作流编辑等覆盖式交互打开时，ACP 主消息流应暂停非关键 streaming UI flush，仅在内存中保留同一 text/thought/plan item 与同一 `toolCallId` 非终态工具事件的最新合并帧；权限、错误、工具终态和 session 终态仍即时处理，覆盖式交互关闭后再 trailing flush 最新帧。
- 前端必须把 text/thought/plan 与非终态 toolCall/toolCallUpdate streaming flush 视为 latest-wins、单飞的后台 UI 任务。覆盖式交互打开、消息列表用户滚动、wheel 等滚动输入期间都应进入同一套 interaction quiet window：最多保留一个待执行 timer，并以稳定 stream/tool identity 为 key 替换累计快照；交互安静后 drain 一次并同步发布。不得对每批累计字符串创建 `startTransition`，否则被交互延后的 transition 会同时保留多份已经过期的长字符串；不得为每种交互单独散落补丁式暂停逻辑，也不得在消息容器上用 pointer/touch 起手事件拦截所有按钮点击。
- text/thought live item 的前端合并必须保持单调：同一 stream 的旧短帧、空帧或乱序 hydrate 帧不得覆盖已显示的完整 content；interaction quiet window 只能延迟普通 trailing flush，不能让 tool、permission、lifecycle 或 session 边界事件越过 pending text/thought，显式 `sync` flush 必须绕过交互 defer 但继续尊重真实 live pause。
- Conversation run 级 live update 必须与当前 ACP 消息热路径分层调度：当前 selected session 的普通 timeline event 只进入 ACPChatDialog 局部合并；已存在于 session tree 里的后台 session 普通 live event 不得触发 `getConversationRun` 和整页 React state 更新；只有新 session 锚点缺失、terminal snapshot、权限/暂停/等待输入等交互态才允许排队完整 run refresh。后台非终态 session snapshot 只允许做轻量运行态 patch，且不能替换当前 selected session payload。
- ACP 消息滚动容器的 `scroll` 事件不得同步读取 `scrollHeight/clientHeight/getBoundingClientRect`。滚动期间只允许记录交互和排一个 `requestAnimationFrame`，在 rAF 中处理历史分页与 interaction quiet window；贴底状态、用户逃逸和内容 resize 引发的程序滚动统一由 prompt-kit `ChatContainer` 管理。interaction quiet window 只延迟普通 live event flush，不再持有第二套 timeline 自动滚动门控；流式 Markdown presentation 即使在两次 timeline snapshot 之间继续改变 DOM 高度，也必须由内容尺寸观察持续贴底。Activity 展开时，pending 权限卡审批后缩为审计行、后续工具增长和下一张 pending 权限卡插入属于同一内容 resize 生命周期：原本已贴底时必须持续跟随，用户主动上滚后不得抢回底部。加载更早历史前显式解除贴底锁，合并完成后继续使用可见 item DOM 锚点补偿阅读位置。
- 关闭状态的系统提示弹窗、产物弹窗和工作流 sheet 不应解析大文本或 workflow JSON；打开时再计算内容，并尽量使用 memo 化结果，避免被 live stream render 带着重复执行。
- ACP 事实状态刷新节奏与用户可见呈现节奏必须分层：timeline/live flush 继续负责把同一稳定 item 合并成最新累计快照；当前活跃 assistant text item 由独立 presentation controller 维护“canonical 目标 + 可见 offset + 速率余量”，只把已经到达可见 offset 的 Markdown 前缀放入 DOM。不得把完整 snapshot 先布局后仅用 opacity/stagger 隐藏未显示字符，否则消息容器会按最终高度提前撑开，并让多个 Markdown block 在不同位置提前出现；presentation controller 也不得反向修改 timeline canonical 或让每个字符进入会话级 state。默认以约 32ms 的有界呈现帧推进，并根据积压量在统一速率范围内追赶，从而把 75ms/125ms 的不稳定快照批次平滑成稳定视觉节奏。thought 不进入该 Markdown presentation controller，直接展示 timeline 已合并的纯文本快照。Activity 展开详情可能很长，内容末尾必须提供右对齐的轻量“收起”按钮，复用 shadcn Button；收起后将同一 Activity 标题恢复到最近可见位置，不要求用户回到顶部寻找触发器。
- 正在流式增长的 assistant 消息继续使用 prompt-kit `Markdown` copy-in；Streamdown streaming mode 只解析当前可见前缀，语法门控在推进 offset 时吞并纯 Markdown 控制符、未完成链接地址和代码围栏后缀。不得同时启用 Streamdown 全字符 opacity/stagger。thought 使用 prompt-kit `ChainOfThoughtText` 纯文本路径，以 `white-space: pre-wrap` 保留内部换行与连续空白，不解析 Markdown，`**`、反引号、列表符号与代码围栏均作为字面内容展示；展示层只裁掉整段首尾空白，避免 provider 边界换行形成空白首尾行。thought chunk 的语义分段继续由后端 timeline accumulator 写入 canonical：缺少换行的独立完整 strong block 之间只写入一个换行，token 级 thought chunk 继续无缝拼接；这是 chunk 语义归一化，不依赖前端是否启用 Markdown，前端也不得重写内部 canonical。最新活跃 stream 必须按最大 `endedSeq/seq` 的事件种类判定，tool、plan、permission 等生命周期事件到达后不得让旧 text/thought 继续处于 active streaming 生命周期。默认不因聊天主路径引入 Mermaid、完整 Shiki 语言包或 KaTeX 等未启用插件。
- 正在输出的 thought disclosure 收起时，prompt-kit `ChainOfThoughtContent` 对 active streaming thought 使用 Radix `forceMount` 保留当前纯文本节点，并在 closed 状态通过 `display: none` 脱离布局；thought 结束后恢复普通 Collapsible unmount 生命周期，避免所有历史思考内容长期常驻。
- timeline item 必须保持稳定 id；未变化的历史 item 应复用对象引用，让正式消息、Activity 摘要、工具审计行和 Agent link 的 memo 化渲染有效。子 Agent transcript 属于独立 branch，不进入父时间线对象图。
- Activity 摘要的 `live` 是同一 `activityStartSeq` 展示实体的单调生命周期：可以从 live 收敛为 archived，但已 archived 的 identity 不得被迟到的 active session snapshot 重新打开。停止响应、live event 与完整 snapshot 乱序到达时，同一摘要只完成一次“正在操作 → 已记录”转换；新一轮操作必须以新的 `activityStartSeq` 创建新 identity，不能复用旧摘要。
- 根会话 composer 草稿属于高频局部编辑状态，不得改变历史消息树消费的 branch locator、workspace command 或 timeline item 引用。用户逐键输入时，已完成 Markdown、Activity、Thought 和 Tool 只能由自身数据变化触发重渲染，不能因为 Provider value 临时创建新对象而绕过 memo 边界。
- 已完成消息的 prompt-kit `Markdown` 是静态渲染边界：当 Markdown 文本、className 和 streaming 标记未变时必须保持组件引用与解析结果稳定。右侧工作区打开文件、切换 Tab、拖动宽度不得使历史 Streamdown 重新解析；消息中的文件、artifact、Agent 与 turn file 操作只订阅右侧工作区稳定命令接口，不订阅 tabs、activeTab、requestedOpen 或 width。
- 会话分页按用户消息、Assistant 正式消息、Agent link、活动摘要、待决交互和 attempt/压缩边界等语义块计算。活动内工具数量、折叠状态和 Agent 子分支事件数不改变父分支 cursor 或 `hasOlder`；根分支和 Agent 分支共用同一 `eventPage`、有限事件 buffer、真实 DOM 锚点补偿和原生滚动容器，不引入动态高度虚拟列表。
- 活动摘要折叠时不请求审计详情。首次展开携带 `branchId + activityStartSeq + activityEndSeq`，后端顺序扫描轻量 header，只保留最近有限候选并仅反序列化当前页选中的事件；“显示更早活动”使用独立 cursor，不修改会话分页。单条工具 raw output 再延迟到该工具展开时按 event/tool ID 查询。已决 permission 不进入活动审计，待决 permission 只进入 owning branch 的 intervention。
- Activity 摘要统计与本地审计详情必须分别表达“总量”和“已加载范围”。`events.length > 0` 只代表存在局部实时尾部，不能代表详情完整；本地可见审计数少于摘要 `totalEventCount` 时，首次展开仍必须读取权威详情并按稳定事件身份合并。重新进入会话后的 summary-only 路径与活跃会话中“summary + partial live tail”竞态必须得到相同的两段思考/工具展示结果。
- Agent 分支只读面板与根会话复用 `ConversationViewport`、消息、Markdown、Activity、Tool 和 `InterventionLayer`，但不挂载 composer、模型/权限配置、停止、继续或重试 DOM。pending permission/elicitation 仍可在 owning Agent 分支中响应；停止和继续只由根 runtime lifecycle 控制。
- Raw frames 面板默认只展示行摘要；展开单条 frame 时才做 JSON pretty print 和长段落换行，不允许折叠态批量解析完整 raw 内容。
- 会话式运行页的工作流 Sheet 与 `GraphView` 必须把拓扑布局和运行态映射分开：布局只依赖节点 id/order 与边 from/to/label，ACP live payload、selected session、node status/current 等运行态刷新只能映射到既有坐标，不得重复执行布局。
- 会话 follow、ACP composer 与 GraphView 运行态不得在普通运行中输出持续性 console 日志；排障日志必须面向具体错误，且不能挂在 token/live event 热路径上。排查 `Maximum update depth exceeded` 时，只保留全局 `[gb-ui-error]` 诊断：命中该错误后输出当前 active element、最近 pointer 目标和截断 stack，用于定位 Radix/prompt-kit composed refs 触发源。
- shadcn/Radix `asChild` 触发器内使用的基础交互组件必须稳定转发 DOM ref。`Button` 作为 Tooltip、Collapsible、AlertDialog、Dropdown 等触发器的通用承载组件时必须保持 `forwardRef` 形态；项目封装的 TooltipTrigger、CollapsibleTrigger、PopoverTrigger、DialogTrigger、SheetTrigger、DropdownMenuTrigger、AlertDialogTrigger、SelectTrigger 等 Radix trigger wrapper 也必须保持 `forwardRef`，避免 Radix composed refs 在流式渲染与全局重绘期间反复 detach/attach 并触发最大更新深度错误。
- ACP composer 输入框工具栏属于 live streaming 热路径，`PromptInputAction` 不得使用会把 trigger ref 写入状态的 Radix TooltipTrigger；该区域图标按钮使用无状态原生 title 提示，避免输入框 value/status 高频刷新时 Tooltip trigger ref 参与 React 更新循环。
- prompt-kit `PromptInputTextarea` 的自动高度在一次受控值提交后只允许执行一次 `height=auto → scrollHeight → 最终高度` 测量写入。ref callback 必须稳定，`onChange` 不重复测量；否则 composer resize、消息区 ResizeObserver 与贴底校正会在同一按键帧内反复布局，长会话中表现为偶发整页闪烁。
- ACP composer 的模型、权限等低频配置控件属于冷路径。配置控件不得直接订阅完整 `AcpSessionVm` 或 timeline events；必须先统一归一化为 ACP session config view model，并以 `currentModelId/currentModeId/options` 生成配置签名。普通 text/thought/plan live event 只允许更新消息热路径；配置签名、会话 scope 或稳定 handler 变化时，配置栏才允许重渲染。
- 工作流图边必须保留 success / failure 等 label 标识，并使用 CSS stroke-dashoffset 表达轻量流动感；running 边可以使用更快的流动节奏和轻量 glow，但不得通过 React state、JS timer 或重新布局驱动画布动画。running node 的高亮优先使用 opacity / transform 类合成属性，不使用持续变化的 box-shadow、layout 或大面积 paint 动画。

### canonical lifecycle

会话页不得再让 runtime、attempt、ACP session 与 composer 各自重复解释同一个 `status` 字符串。后端 conversation VM 必须为每个 leaf 派生 `lifecycle`：

| 层级 | 字段 | 职责 |
|---|---|---|
| runtime facet | `status / outcome / pauseReason / resumable / current / active / continuable / phase / revision` | 表达 workflow runtime 是否运行、能否继续，以及 `run.json.execution` 的权威执行阶段与版本 |
| control facet | `mode=runtime-controlled/non-runtime-controlled` | 只表达当前 turn 的结果是否由 Runtime 消费，不代表 session 或节点状态 |
| ACP facet | `sessionAvailability / liveTurnActivity / latestTurnStatus / stopping` | 分别表达 session 可复用性、当前进程 live turn、最近历史 turn 与当前停止活动；历史 status 不得重建 live activity |

`acp.snapshot.json / acp.session.json` 的持久化 schema 同样使用 `availability + latestTurnStatus`，不再写入混合语义的通用 `status`。旧 metadata 首次读取时执行一次 O(1) 迁移并立即原子回写；迁移只读取当前 metadata 与已有 session identity，不扫描 timeline，也绝不能据此恢复进程内 live turn。
| lifecycle 顶层 | `displayStatus / runtimeDisplay / continueKind` | 作为 session tree、activeSessions 与 composer 的基础派生事实源 |
| composer facet | `mode / submitTarget / processingKind / statusKey / canStop / lockInput` | 作为 composer 输入、停止、状态文案和提交目标的唯一业务规则源 |

`status` 与 `runtimeDisplay` 仍可作为兼容字段暴露，但必须由 lifecycle 同一个派生函数产出，不能在前端或其他 VM 中重新拼优先级。

`runtimeDisplay` 必须同时表达视觉结果和错误语义：`tone=danger` 可以表示测试/验收节点正常完成后的 workflow outcome failure，但只有 `blockingError=true` 才能驱动 composer 的 runtime/session error 面板。前端不得再用红色或终局状态反推运行时错误。

runtime 已 terminal/completed 且不可继续时，底层 ACP snapshot 中残留的 `running / sending / responding` 只能作为历史或 stale 事实处理，不能让 leaf 或 composer 继续保持 active。反过来，Runtime active 时也不能根据 ACP terminal 猜测具体执行阶段：`StartingNode / RunningNode / FinalizingArtifact / RepairingArtifact / AwaitingManualCheck / Transitioning / LaunchingNextNode / PreparingWorkspace / Paused / Terminal` 全部来自 `run.json.execution`。停止后继续当前 paused leaf 首先进入 checkpoint 对应的权威阶段，绝不因旧 ACP snapshot 为 terminal 而复用 `launching-next-node`。只有当前进程的 prompt registry 为 `CancelRequested`，或本地 stop 命令尚未返回时，composer 才进入 stopping；磁盘上的旧 `cancelling/cancel-requested/running` session metadata 不得在重启后重建 live turn 或 stopping。会话式运行页收到当前选中 session 的完整 session snapshot 时，必须先在 App 层更新 `ConversationRunVm.selectedSession`，再刷新 run tree/lifecycle；若 run refresh 返回的 `selectedSession` payload 临时为空，前端必须保留同 key 的现有 session payload；同 key 的完整 session snapshot则作为 payload 权威更新替换旧值；selected session identity 变化时不得沿用旧 payload；会话组件也不得仅因本地已有 timeline events 就把缺失 payload 重建为 `running`，只有权威 Runtime phase 或当前进程 live turn 明确 active 时才允许创建临时 running shell 承载早期流式事件。

composer 只消费后端 lifecycle/composer + ACP session live status + run mode capability + 少量本地 optimistic 状态；placeholder、输入禁用、停止按钮、状态文案和发送目标都来自同一个 semantic composer state。若 ACP facet 已进入 terminal，历史未匹配且不属于当前提交的 optimistic sending 只能作为过期本地提交看待，不得继续触发“发送中”或输入锁定；但用户刚发起的新 same-session ACP prompt 必须作为当前本地 turn 展示发送中/处理中，直到后端接受 prompt、返回拒绝、或返回未包含该 prompt 的 terminal/空 session 后显式收敛。Direct 的本地 sending overlay 不锁输入，第二条提交立即进入持久化队列；Workflow/AUTO 继续执行 active 锁定。`runtime-continue-started` 表示后端已经接受本次继续命令，即使命令返回不携带新的 ACP session payload，前端也必须立即结束本地 sending/awaiting optimistic 锁，把本次用户气泡视为已接受；后续输入策略由共享 lifecycle 与 run mode capability 共同投影。

### 互斥状态
1. **正常输入**：当前 session 已正常结束时，用户可继续输入消息（含附件），发送目标为 ACP same-session prompt
2. **运行中输入策略**：Workflow / AUTO 在权威 Runtime execution 表示 active 时继续锁定输入；Direct 保持输入可用，提交目标由后端 lifecycle 明确给出 `queue-prompt`，内容进入当前 attempt 的持久化待发送队列，不直接并发进入 Provider
3. **停止中锁定**：本地 stop 命令未返回，或当前进程 lifecycle 的 `liveTurnActivity=cancel-requested/stopping=true` 时，composer 显示“正在停止当前会话…”并锁定输入；持久化 session metadata 只描述历史，不能在进程恢复后制造停止中状态
   - stop 命令返回 `accepted` 只表示取消已可靠受理，不得提前把 ACP session 写成 `cancelled` 或展示“已停止”。Runtime control cursor 与 ACP turn 终态分属不同领域：写入 cursor 时必须保留既有 `latestTurnStatus`，缺少 session metadata 时使用非终态 `none`，不能顺带制造 `cancelled`；仅有 cursor、没有 `sessionId` 的 metadata 也不能被恢复清理误判为 active ACP session。runtime 发送无响应的 `session/cancel` 后继续等待原 `session/prompt` RPC 收敛；取消期间新到达或正在等待的 permission / elicitation 必须由同一 turn 的 `CancelRequested` 闸门自动回复 cancelled / decline，不再展示新的用户决策卡片，也不得阻塞既有 30 秒 cancel drain。原 prompt 返回 cancelled/interrupted、transport 中断或 cancel deadline 到期后，统一收尾路径才写入终态；deadline 到期的 session 标记为不可直接复用，不能误报为 provider 已确认。
4. **运行错误提示/操作**：当前 session 派生为 `runtimeDisplay.blockingError=true` 且后端 composer 给出 `runtime-error` 时，不允许输入，显示错误原因；`error-blocked` 必须优先展示 canonical control failure 或结构化 `RuntimeErrorInfo` 的 `code + params` 映射文案，`run-progress.json` 只负责观测且不得成为错误语义来源；没有具体原因时才使用泛化文案。测试/验收节点正常完成后的 `failure / invalid` 只表示 workflow outcome，不触发 runtime-error 锁定态。`error-blocked` 表示不可重试的 runtime 阻塞，不提供 `runtime-continue` 输入入口；历史 killed/session failed 仍使用终止或失败文案。provider/auth/quota/rate-limit/model/catalog/workspace/transport 等异常应表现为 `runtime-abnormal`，不能因为 ACP session failed 或 provider stopReason=error 而进入 failure edge 或 runtime-error 锁定态。
   - workflow 控制流限制导致的终局失败（例如 `workflow_control_limit_exceeded`、`max_rounds_exceeded`）属于 run-level runtime 业务异常，不属于 ACP session failure，也不应把 composer 强制派生为 `runtime-error` 锁定态。后端会话 VM 必须复用 canonical control failure 解析结果，把标题与原因归一化到 `runtimeErrorMessage`；前端 ACP 会话页在顶部错误横幅中展示该消息，同时保持已终止会话的普通输入/追问能力。
   - 该类终局失败的实时刷新由后端 `RunCompleted(Failure)` lifecycle 负责，不能只依赖 ACP session terminal update 或重新进入页面时的冷加载。
5. **工作流无效修复按钮**：只有 submit target 为 runtime continue 且 workflow 无效时才不允许输入并显示修改按钮；当前 session 已正常结束后的 ACP same-session 追问不受 workflow invalid 阻塞
6. **人工 check 判定门**：当前 session 因 `waiting-for-user-input + manual_check_pending` 暂停时不显示继续按钮，输入框保持可用；普通文本只走 ACP same-session prompt，不推进 runtime edge，只有成功 / 失败判定按钮触发 `submit_manual_check`。
7. **停止后用户介入 / 运行异常继续**：当前 session 因用户停止派生为 `process-interrupted`，或因可恢复异常派生为 `runtime-abnormal` 时，恢复普通输入框并展示独立“继续工作流”动作。用户文本固定走 `UserMessage + NonRuntimeControlled` same-session 对话，不恢复 workflow；只有点击继续动作才发送隐藏 `RuntimeResume + RuntimeControlled`。`runtime-abnormal` 保留异常视觉与提示文案，但不进入 `runtime-error` 锁定态。若异常处于 `recovery=auto` 的 bounded retry 中，composer 锁定并展示重试中；重试耗尽后降级为 `runtime-abnormal + manual`，同时恢复普通输入与继续动作。

### Direct 待发送队列

Direct 在运行中的输入不是第二条并发 prompt，而是 attempt 级待发送意图：

- 数据持久化到 attempt 目录的 `acp.prompt-queue.json`，采用 FIFO，最多 10 条；每条保存稳定 item id、稳定 prompt id、正文、附件路径、创建时间和 `queued / dispatching` 状态。关闭应用、停止会话或切换页面都不得删除队列。
- 队列与 ACP/runtime lifecycle 归属同一 attempt。Workflow / AUTO 不投影队列，且不改变其运行中 composer 锁定策略；Direct 无队列时的普通发送、runtime continue、repair 也不进入用户优先等待窗口。
- 队列列表紧贴 composer 上边缘，位于 usage/token 行下方。默认展示 FIFO 前 3 条，点击“查看更多”展示全部（最多 10 条）。每条使用 icon action 提供编辑、使用、删除，并包含 tooltip 与 aria-label；编辑保存后保持原顺序。
- 运行中允许编辑和删除 queued 项；“使用”仅在会话停止/空闲时可用，用于指定某一条继续。被指定项以原稳定 prompt id 进入统一发送链路，不建立第二套消息展示。
- 用户停止或应用关闭后不自动弹出/发送队列。点击停止时必须先在进程内同步设置发送门禁，并持久化 `autoDispatchSuspended=true`、递增 revision；这既取消已等待的候选，也阻止停止之后才到达的 success 终态重新注册候选。只有用户之后主动提交新消息或点击某条“使用”才解除门禁。失败、取消和停止不继续出队，并按 durable timeline 结算当前 dispatch：已存在稳定 prompt id 的项目已经发送，必须删除；只有尚未写入用户 timeline 的未接受项才恢复到队列原位置。
- 自动候选先进入常量化 600 ms 用户优先窗口。窗口内真实用户提交会递增 queue revision，使低优先级自动 claim 失效，用户消息先进入已有 attempt 级 ACP prompt lock；该用户 turn 成功后再继续队列。仲裁位于后端，前端不使用 `setTimeout` 猜顺序。
- attempt 级 ACP prompt lock 是所有 prompt 的统一发送串行器；revision 只决定窗口内优先级，不能替代串行器。任何时刻最多一个 prompt 进入 Provider。
- 自动/手动出队先把项目标记为 `dispatching`。稳定 prompt id 写入 canonical user timeline 即表示消息已经持久化并离开队列；统一 prompt lifecycle 在落盘后发送 `Accepted { promptId }`，按进程内 active dispatch 精确删除对应项，不通过 session refresh 扫描 timeline。普通 Direct、Workflow、AUTO prompt 未命中 active queue dispatch 时不得读取队列或 timeline。`Finished { successful }` 只决定是否继续调度下一条；失败或取消只能恢复尚未接受的 dispatch。
- ACP prompt 发送采用“标准入口 + 已配置内部入口”两层接口：普通发送和 Direct 自动出队必须走标准入口，由它统一安装 live update、session update、`Accepted` 与 `Finished` callback；内部入口只接受已完成配置的受控 App 类型，禁止裸 `App` 绕过生命周期装配。scheduled continuous 因需保留 occurrence 上下文可以使用内部入口，但必须经过同一完整配置函数。每条自动出队消息完成后都必须再次进入 `Finished` 调度，直至队列为空、暂停、失败或被用户优先操作抢占。
- 队列持久化版本负责恢复边界：当前进程的 active dispatch 由 prompt id 索引，在线 accepted 结算为 `O(1)` active lookup + `O(queue)` 精确删除；timeline 只允许在旧版本队列迁移或应用重启后发现孤儿 `dispatching` 时扫描一次。恢复时 timeline 已接受则删除，尚未接受才恢复为 queued；旧版本错误恢复成 queued、但 prompt id 已存在于 timeline 的历史重复项也在一次性迁移中清理。已接受项不得在停止后回队，也不得保留原 prompt id 编辑成另一条逻辑消息进入 retry 链路。
- 出队消息继续写标准 `goldBandPrompt`/用户 timeline 事件并使用普通用户消息气泡；仅内部 prompt id 表明来源于 runtime 队列，不增加特殊消息样式。

### 修复入口

- 会话运行时的“修复”按钮与旧任务工作流页的 repair drawer 心智一致：打开当前任务的右侧工作流编辑资源，让用户修复 workflow 配置。
- 修复资源标题使用“修复工作流”，而不是普通“编辑工作流”；Header 中展示无效状态、查看错误原因入口和错误原因摘要，帮助用户理解为什么需要修复。
- 右侧编辑资源激活后按完整 `projectId/taskId` 单独读取 Task authoring 聚合，以其中的 `WorkflowDsl` 和 `WorkflowModelBindings` 初始化编辑器；不得从当前 Run 的 executable snapshot 反推作者态绑定。保存后以 `saveTaskWorkflow` 返回的最新 `WorkflowVm` 收敛编辑基线，再重新拉取当前 conversation run VM，使 workflow 有效性、session tree、运行图与 composer 状态立即刷新。
- 修复入口不直接调用 `continueRun`；用户完成修复后再按运行态规则继续。对于 `error-blocked`，修复入口只表示查看错误、修改 workflow 或进入诊断；只有后端确认存在安全恢复点并生成恢复计划时，才允许恢复，否则只能重新运行或从节点重新开始。

### 继续输入
- 当前 session 正常结束后，在会话窗口追问属于 ACP same-session prompt，不要求 authoring workflow 合法
- 追问发送时，composer 进入本地 turn 的发送中 / 处理中 / 计时状态；结束后只影响该 ACP session 的消息流，不触发工作流 runtime 继续执行
- Gold Band 本地发起的用户消息仍以调用 provider 前持久化的 synthetic `goldBandPrompt` 为 canonical 事实，并以 `raw.promptId` 标识本轮输入。仅当当前 Agent 显式开启 `externalSessionSyncEnabled` 时，Provider 在 `session/load` 中回放的历史才进入对账导入：将历史 `goldBandPrompt` 作为有序锚点与 replay user turn 对账；Provider 允许漏回放部分本地 turn，后续 replay 消息只要匹配剩余锚点就仍属于本地回显，并跳过已缺失的本地锚点；完全匹配不到剩余锚点的消息才属于外部客户端。外部 user turn 及其后的 assistant/thought/tool/plan 更新先作为一个完整 turn 暂存，匹配到下一个本地 prompt 后再整组写入，并记录插入该 prompt 之前的逻辑位置；load 结束仍未遇到右锚点的尾部历史只记录已匹配的左锚点。不得因 Provider 漏掉一个重复文本 turn，导致后续本地工具调用被错误导入并与原卡片 identity 冲突。开关关闭时 replay 整体不进入 timeline。
- Provider history identity 优先使用 `messageId/toolCallId`；Agent 未提供稳定 ID 时，使用 `session + afterPromptId + gapTurnIndex + kind/itemIndex` 生成稳定身份，使尾部历史在下一次 load 获得 `beforePromptId` 后仍只更新同一 item。ACP replay 不提供原始消息时间时，timeline 只能保存本次同步到达时间和原始 replay 顺序，不伪造历史时间。
- Claude ACP 当前把 `[Request interrupted by user]` 与 `[Request interrupted by user for tool use]` 作为普通 `user_message_chunk` 返回，且没有结构化 control 标志。现阶段只在 Claude Provider 归一化层对这两个完整字符串做精确隐藏，原始帧继续保留；不得使用 contains/前缀等模糊规则。该规则是上游协议补充结构化 interruption 事件前的临时适配，后续应改为消费结构化字段。
- prompt lifecycle 明确拆为 `Starting -> Accepted -> Running -> CancelRequested -> terminal`：`Accepted` 表示 synthetic 用户消息已经持久化并可在重启后恢复，`Running` 表示 `session/prompt` 已真正发出。前端在匹配到持久化 `promptId` 后从“发送中”切到“处理中”，不得等待 provider echo。
- 已完成 run 上继续发起 same-session follow-up 时，当前内存 `PromptActivity` 是本轮 ACP lifecycle 的权威事实，优先于上一轮持久化 snapshot 的 `completed/cancelled`。会话 VM 必须用同一有效状态驱动 session status、pending permission、Agent execution projection 与 composer；stale-session completion fuse 在存在活跃 prompt 时禁止把 snapshot 改回终态。每个流式 ACP update 携带的 lifecycle 必须立即进入当前会话局部状态，前端本地 submit command 未结算前也不得被上一轮 terminal snapshot 清除；因此只要本轮 prompt 仍为 Starting/Accepted/Running，嵌套 Agent 不得误显示“已中断”，composer 必须保留停止入口。
- 恢复已有 ACP session 时，`session/load` response 只表示 RPC 已返回，不代表 Provider history replay 已发送完成。runtime 必须继续保持 `Replaying`，通过 session event pump 等待 inbound replay 达到有界静默，再结算暂存的 Provider history turn；在 session-ready 快照与真实 `session/prompt` 之间再次执行同一静默屏障，确认回放收敛后才进入 `AwaitingTurnStart` 并发送 prompt。静默屏障超时必须终止本轮同步，不能让历史与当前 turn 混流。外部会话同步关闭时，屏障内所有 replay content 只保留 raw 审计，不进入 timeline；开启时才交给 Provider history importer。已知历史 identity 在后续阶段仍需抑制，避免极迟到的旧消息被重复追加或移动到会话末尾。
- 停止命令返回 `accepted` 后，前端保留当前 `selectedSession`，先合并 response 中的小型 lifecycle/run 摘要，再只移除仍为 `optimistic + sending` 的未接受消息；不能用空 session payload 清空消息区。已经出现 durable `goldBandPrompt` 的用户消息必须保留；若停止发生在持久化前，未接受 optimistic 消息应立即消失且不得在重启后复现。最终 ACP terminal snapshot 与后台分页校准只负责结算状态，不参与 stop command 的响应延迟。
- 历史版本中未分类、直接写入 timeline 的 provider user echo 继续在读取投影中清理；新版本只有经过 replay 对账确认的外部消息才携带 `source=providerHistory` 并进入聊天时间线。每个外部历史 item 在 `raw.historyPlacement` 中记录 `version/afterPromptId/beforePromptId/gapTurnIndex`，顶层继续记录 `historyItemIndex`；`seq/timestamp` 永远表示审计到达顺序，不因逻辑插入位置改变。后端 session projection 与前端窗口 merge 必须按本地 prompt 锚点构建展示顺序，同一 gap 内按 `historyItemIndex`、审计 seq 排列；分页筛选与 cursor 仍按审计 seq 工作，并以当前窗口审计 seq 的最小/最大值计算边界。读取投影只对缺少 `historyPlacement` 的旧 Provider-history turn 使用文本锚点修复，结构化新历史不得再按文本猜测删除；若 replay patch 与既有非 Provider-history item 使用同一稳定 identity，原始本地 item 优先。raw/timeline 审计记录不重写，placement-only patch 合并时保留首次 `seq/timestamp/start/end/timing`，同时以真实最大 patch revision 继续分配后续序号。
- Gold Band 在发起 Direct 会话时提供的合成模型与权限模式空选项统一命名为“不指定”（英文 `Unspecified`）。这些值只存在于 Gold Band UI，提交时分别固化为 `modelOverride = null`、`permissionModeOverride = null`，不得与 Agent 通过 ACP 返回的 `default`、`auto` 等不透明配置 ID 混用。
- ACP session 同时保留 Agent 报告的 `currentModelId / currentModeId` 和 Gold Band 管理的 `modelOverride / permissionModeOverride`：current 字段只用于呈现 Agent 当前配置，override 字段分别是后续追问是否显式调用 `session/set_config_option(model|mode)` 或对应 mode API 的唯一事实源。override 为空时追问不设置对应配置，继续继承 Agent 环境配置；不得从 Agent current 字段反推 Gold Band override。
- 会话详情在对应 override 为空时显示“不指定”，并同时保留 Agent 返回的完整模型或权限模式目录；用户选择任意 Agent 配置（包括 Agent 自己返回的 `default`）后写入显式 override，同一 session 内不再提供该项的“不指定”选项，但仍允许在 Agent 返回的具体配置之间切换。
- 当前 run 暂停后的 NonRuntime 普通对话不受 workflow 是否有效影响；显式“继续工作流”仍要求 workflow 合法，后端校验失败时返回结构化错误，不能把普通消息升级成恢复信号。
- 对不支持原生 `systemPrompt` 的 ACP provider，Gold Band 只在同一 ACP session 的首轮 `session/prompt` 中把 stable system prompt 作为 hidden user block 内联发送并持久化审计；同一 session 的停止后继续、恢复继续和完成后追问不得重复内联或重复 timeline 记录 stable system prompt。后续输入只包含本次用户文本与本次新上传附件；不得重带原始任务附件、历史附件或上一轮 runtime hidden context。
- 当前 run 因 `process-interrupted` 或 `runtime-abnormal` 暂停且可继续时，composer 同时提供两条独立路径：文本输入固定为 `UserMessage + NonRuntimeControlled` 的 same-session 普通追问，保持 run/node paused；“继续工作流”按钮调用纯动作接口并以隐藏 `RuntimeResume + RuntimeControlled` 恢复。普通消息不得解析成 continue，也不得因 Agent 回复结束而读取 artifact 或推进节点。旧 ACP snapshot/session 的 `failed` 或 `cancelled` 只代表上一段响应的历史终态，不能取消显式继续、阻断 Agent 拉起或驱动错误态。
- AI-DYNAMIC 内部 leaf 的显式继续由后端根据 locator 生成精确 leaf override：继续前先检查目标 leaf 是否已有完整合法 `dynamic-node-completion`，若已完成则先收敛并接受 proposal；父 run/round/外层 attempt 因同一中断 paused 时先恢复外层 running，再收敛 stale sibling，最后只恢复目标 leaf 的同一 ACP session。没有明确 leaf 目标的父 run continue 不能批量恢复普通 paused worker；并发 leaf continue 继续复用单 scheduler 与 pending resume 机制。`error-blocked` 不提供恢复动作。
- 会话态与旧 Round 详情中的普通文本都调用 `submit_conversation_prompt`，该 command 只路由 NonRuntime ACP prompt；显式继续统一调用 `continue_conversation_runtime`，不接收可见 prompt，不创建 optimistic 用户消息。`send_acp_prompt` 仍是底层同 session ACP helper，command 层必须先用 control mode / current attempt 许可隔离 Runtime 与 NonRuntime。
- 通用继续资格只覆盖 `process-interrupted` 与 `runtime-abnormal`。`waiting-for-user-input + manual_check_pending` 是 NonRuntime 会话加独立成功/失败判定按钮，不展示也不接受通用继续；permission、elicitation 与 `error-blocked` 同样只能走对应结构化入口。固定 workflow 与 AI-DYNAMIC leaf 必须使用同一后端领域判定。
- `runtime-continue-started` 是 durable acceptance：后端必须等到目标 Running 状态落盘后才能返回。启动前失败直接返回结构化错误，前端不建立 Running override；启动后意外失败由后端对原 active attempt 做 compare-and-set 收敛为 `runtime-abnormal` 并发送权威 lifecycle。用户 stop、节点完成或 attempt 切换形成的新事实优先，迟到失败不能覆盖。AI-DYNAMIC 启动失败还要回收 re-arm leaf 和 starting resume registry。
- Workflow/AUTO 的中英文基础 runtime system prompt 必须预先说明：用户主动打断当前工作并转而讨论其他内容时，Agent 在 Runtime 明确恢复工作流前无需遵守当前 artifact 输出语义，应自然回应当前问题；中断期间用户针对当前任务给出的最新明确指引在恢复后继续有效，可以调整任务内容、交付结果或角色预设流程，但不能覆盖 artifact contract、Gold Band 文件规则、安全与能力边界。AI-DYNAMIC 通过既有基础 system + stable section 组合自然继承该规则，不在专属 section 重复；Direct system prompt 继续为空。停止后的普通消息只发送用户原文，不追加一次性 suspended hidden context，也不创建相应 accepted cursor 字段。Runtime 侧仍以 `NonRuntimeControlled` 独立保证不提取、不校验 artifact 且不推进节点，不能把正确性只交给 Agent 遵循提示。显式继续 prompt 保持 `PromptVisibility::Hidden + reason=runtimeControlResume`，只重新建立 Runtime 结果消费和 artifact 契约，不能被解释为回滚到中断前的角色流程。
- 普通消息、hidden resume、finalize 与 repair 继续共用 ACP session prompt lock。固定 workflow 的 continue 还必须在状态读取前认领 per-run starting lease，双击或并发请求只接受一个；lease 只保护后台启动窗口，不持锁等待 Agent，不同 run、不同 session 和 AI-DYNAMIC leaf 保持并行。
- `WorkflowContinued` 采用 source transition CAS：prompt 在 accepted event 落盘前只携带候选 transition，不提前把 session 持久化为 RuntimeControlled；初始化或接受失败时仍可重试，新的 stop 边界也不会被旧 resume 回写覆盖。Direct / `RawAgent` 首轮直接使用 NonRuntime，不创建伪 Runtime 边界。
- legacy attempt 缺少 `runtimeControl` metadata 时允许回看一次 timeline；无 transition 的结果通过 snapshot/session 的 `runtimeControlTimelineScanComplete` 形成 negative cache。后续消息只读取小型 metadata，不得随 timeline 长度增加重复扫描成本。
- 若停止发生在 PostTurn artifact finalize，`artifact-emission.json(finalizing)` 只证明业务 turn 已结束，不证明中断输出完整。暂停期间的普通消息不得触发 finalize；点击继续后跳过业务 turn并重新发送完整 artifact finalize prompt，中断候选永不参与 artifact 验收。
- 停止按钮只调用桌面 `stop_active_session` 统一语义入口，不在前端按“ACP / runtime”维护两套停止链路。用户语义始终是“停止当前进行中的 leaf/session”；后端根据当前 run 与选中 session `AttemptLocator` 做分层收敛：普通单节点 attempt 停止会把当前 runtime attempt 写入 `Paused + ProcessInterrupted`；AI-DYNAMIC 内部 leaf 停止只暂停目标 dynamic node 与目标 ACP session，兄弟 leaf 仍为 `Ready | Running` 时父 graph/run 继续运行；当没有任何 active leaf，且剩余未完成 leaf 都是用户暂停的可继续节点时，父 dynamic graph、外层 AI-DYNAMIC attempt 与 run 自动收敛为 `Paused + ProcessInterrupted`，不能显示为错误阻塞。活跃 ACP runtime 发送一次 `session/cancel` notification 后继续 drain 当前 `session/prompt`，直到 adapter 返回 cancelled/interrupted 或 cancel deadline 到期；停止不会写入 `Killed`，也不把 adapter kill 当作 cancel 成功兜底。
- `stop_active_session`、attempt teardown 和 run 级 best-effort cancel 在处理 ACP 停止时，除了 permission 外也必须取消 attempt 目录下尚未完成的 elicitation request，确保 runtime 中阻塞等待 `elicitation/create` 的分支解除，不留下孤儿轮询。stop command 的 accepted 边界只要求暂停状态和 cancelled session snapshot 已持久化；permission/elicitation timeline 终态归并属于后台 cleanup，不得重新阻塞控制面响应。
- `stop_active_session`、应用关闭和启动 crash recovery 共享的 attempt cancel 流程必须把未决 ACP permission request 一次性收敛为终态：写入 `acp.permission-response.*.json` 的 `cancelled=true`，并同步 upsert `acp.timeline.jsonl` / legacy `acp.events.jsonl` 中对应 `permissionRequest` 为 `status=cancelled`。前端重进页面只能回放“该 tool call 已中断/取消”的历史事实，不得再次弹出权限决策；已经由用户选择成 `selected` 的权限事件不能被停止流程覆盖成 cancelled，已被停止流程写成 `cancelled` 的权限也不能被迟到的旧弹窗点击覆盖回 `selected`。
- `AcpSessionVm.events` 的分页窗口可以裁剪普通消息，但不能裁剪掉权限请求的最新事实。后端返回 session VM 时必须把每个 `permissionRequest` 的最新状态事件附加到当前窗口中，用同一 request id 覆盖前端缓存中的旧 `pending`；否则用户授权或停止后，历史缓存可能继续把已完成权限恢复为弹窗。
- ACP permission request 的业务身份是 canonical request id（去除重复 `permission-` 展示前缀后的 `raw.requestId` / `id`），不是 `sessionId`、attempt id 或 timeline display id。前端 live/cache/session merge 必须按 canonical request id 替换旧事件；后端补写 `cancelled` 终态时应继承原 pending event 的 `sessionId`、`toolCallId`、title 与 raw options，避免同一权限请求在 UI 中裂变成“旧 pending + 新 cancelled”两条事实。
- ACP permission 的状态机由后端 permission lifecycle 统一拥有：收到 request 时写入 `pending`，用户 Allow / Reject 写入 response signal 时必须同步 upsert `permissionRequest(status=selected)` 到 `acp.timeline.jsonl` / legacy `acp.events.jsonl`，并继承原 pending event 的 `sessionId`、`toolCallId`、title 与 raw options；runtime waiter 消费 response 后也要再次确认终态已落盘。正常决策、停止取消和非活跃 fallback 不能分散维护 timeline 终态，否则重进会话会把历史 pending 重新识别为弹窗。
- 活跃 ACP runtime 的内存 timeline map 与磁盘 timeline 同属权限状态事实源。权限 response 被 live waiter 消费时，终态事件必须通过 runtime 自身的 `persist_event` 合并进内存 timeline，再写 patch / final timeline；不能只由 Tauri command 或 helper 直接改磁盘，否则 runtime shutdown 时旧的 pending 内存快照会覆盖刚写入的 selected/cancelled。
- ACP elicitation 的状态机由后端 elicitation lifecycle 统一拥有：收到 `elicitation/create` 时写入 `elicitationRequest(pending)`；Tauri command 提交用户决策时写入 durable response signal 并 upsert `elicitationResponse(completed)`，前端据此立即关闭卡片。runtime waiter 读取同一 signal、持久化规范终态、发送 ACP JSON-RPC response 后，才清理 request/response signal。不得根据 `acp.snapshot.json` / `acp.session.json` 的 completed 等元数据提前删除 signal，因为已完成 run 的 follow-up prompt 仍可能存在活跃阻塞 waiter。
- `acp.permission-request/response.*.json` 与 `acp.elicitation-request/response.*.json` 只作为 runtime 阻塞等待与 Tauri command 之间的临时信号文件，不作为长期事实源。permission/elicitation 的历史事实统一落在 `acp.timeline.jsonl` / legacy `acp.events.jsonl`；elicitation response 的所有权固定为“command 生产、runtime 消费并在成功回包后清理”。显式 stop/close/timeout 负责未决交互的取消和陈旧文件收敛，不能由展示层会话状态推断 waiter 是否存在。
- `stop_active_session` 是控制面接口，先在 blocking worker 中持久化目标 leaf/run 的 `Paused + ProcessInterrupted` 和 `acp.snapshot.json/acp.session.json = cancelled`，登记 prompt cancel-requested，再返回 `{ operationId, status: accepted, kind: stop-accepted, run, lifecycle }`；响应不得构建 `AcpSessionVm`、扫描 timeline、还原 Blob 或等待 provider。permission/elicitation 终态补写、`session/cancel` notification、搜索索引和详情刷新在后台执行。父 run 是否 paused 由 graph 聚合状态决定；`accepted` 只表示 durable stop intent 已建立，不表示 provider 已确认取消。前端按 lifecycle 保持“正在停止”或恢复可继续态，后续 ACP terminal snapshot 继续刷新消息流。
- 停止过程中可能同时出现 `run paused/process-interrupted` 与 ACP channel 仍在 drain 的事实；composer 展示优先级必须以当前进程停止活动为准：本地 stop 命令未返回，或 prompt registry/lifecycle 明确为 `CancelRequested/stopping` 时显示“正在停止当前会话…”并保持输入锁定。session metadata 与 `provider.pid` 都不参与 live active/stopping 推导。
- ACP adapter 生命周期按 `provider_id + workspace_root` 复用长连接；这里的 `workspace_root` 是用户打开的逻辑项目根目录，同一 workspace 下同一 provider 的多个 ACP session 共享一个 adapter process，不同 workspace 的 connection 可以在新 UI 中并存。AI-DYNAMIC worktree 只是 session 执行目录，不作为新的 adapter workspace key；adapter process 仍归属原始逻辑 workspace，`session/new.cwd` 才指向具体 worktree。后端 connection manager 按 JSON-RPC request id 与 `sessionId` 路由 response、timeline update 和 permission request。用户不感知 adapter pool，也不在前端暴露 cancel/close/delete 协议概念。
- AI-DYNAMIC 节点只持有 `workspaceId`，实际路径与生命周期统一由 dynamic workspace catalog 管理。fanout 成功收敛并通过 acceptance 后，runtime 删除对应 child worktree，将 catalog 状态更新为 `released`，同时保留 workspace 记录供恢复诊断和历史验收；测试必须通过 catalog 解析路径与状态，不能在节点上重复维护 `workspacePath`。
- dynamic graph 使用独立于领域对象的 schema version：当前 graph 为 `0.2`，内部 run/node/group/workspace 仍使用各自既有版本。所有 graph 消费方必须通过统一存储读取边界：读取历史 `0.1` 时，将节点 `workspace/workspacePath` 确定性转换为 graph-owned workspace catalog，并补齐 group 的 `targetWorkspaceId/childWorkspaceIds`；完整校验通过后使用原子替换一次性写回，之后重复读取不得再次改盘。迁移保留 dynamic run、node 与 attempt/session locator 身份；无法从已注册 Git worktree 证明仍安全可用的旧 workspace 记录为 `released`，只支持历史展示，不虚构可恢复能力。迁移按具体 graph 延迟触发，不在启动时扫描全部历史运行。
- 普通 Stop 只中断当前 prompt；停止后 Gold Band 持久化保留原 ACP `sessionId`，runtime continue 必须继续用原 `sessionId` 恢复同一业务会话。session release、关闭应用以及 agent/MCP 配置保存导致的 restart boundary 使用 bounded `session/close` 释放 live sessions；关闭应用时先把所有 running run 递归收敛为 `Paused + ProcessInterrupted`，再对 manager 中所有 live provider/workspace connections 发起 bounded close，不能只按当前 workspace 过滤。普通 workspace 切换只是切换当前工作区视图，不关闭旧 workspace connection。新 UI 侧边栏删除 workspace 属于显式 remove boundary，移除前必须 bounded close 该 workspace 的 ACP connections，close 失败则保留 workspace 并展示错误。重跑前停止旧 run 同样走 `Paused + ProcessInterrupted`，不产生新的 `killed` 会话；历史 killed 仅作为只读兼容状态展示。配置保存遇到 active prompt 时直接阻断并提示用户先停止会话，停止后再保存才关闭 idle connection 并使用新配置。adapter crash、stdout 断开或 transport closed 按可恢复中断处理，active runtime 收敛为 `Paused + ProcessInterrupted`；close 失败必须作为明确错误处理并记录诊断，不能静默吞掉，也不能把 kill adapter 伪装成成功。启动 crash recovery 没有 live connection 时只依据持久化 runtime lifecycle 收敛状态，`provider.pid` 仅作为 orphan cleanup 线索。
- `session/close` 是 ACP session 的关闭边界，不是可恢复暂停边界。后端在发送 close 前必须先结算该 attempt 下所有 pending permission / elicitation：写入 cancelled response、把 timeline 中对应 request upsert 为 terminal 状态，并清理 pending signal 文件；close 成功后必须把 `acp.snapshot.json` / `acp.session.json` 写为 `cancelled + stopReason=cancelled`。视图层读取历史数据时，如果 snapshot 仍是 active 且 `acp.raw.jsonl` 中最后一个 ACP 生命周期边界是同一 JSON-RPC id 的 `session/close` request 与成功 result，必须将该 session 熔断为 `cancelled` 并隐藏 pending permission；如果该 close 之后又出现新的 `session/load` / `session/prompt`，说明会话已经重新进入运行态，不能再用旧 close 熔断当前 snapshot，避免恢复运行中的权限响应被误判为 terminal 后删除。
- composer semantic state 的优先级固定为：permission blocked → 当前进程 stopping → submitting → authoritative runtime active lock（具体 phase 来自 `run.json.execution`）→ invalid workflow（仅 runtime continue 路径）→ runtime error（含 `error-blocked`）→ `process-interrupted` / `runtime-abnormal` 输入继续 → `waiting-for-user-input + manual_check_pending` 普通 ACP prompt + 判定按钮 → normal ACP prompt。后续新增状态必须先进入该派生表和矩阵测试，不能在组件里局部追加布尔判断。
- `permission blocked` 属于 runtime 运行态阻塞，不是独立的 composer 替代视图。前端必须继续渲染同一个 prompt-kit `PromptInput`，用禁用 textarea、运行态 placeholder、权限等待 hint 和停止入口表达“当前会话由 runtime 运行中，暂不可输入”；权限决策卡片可以作为会话交互卡展示，但不得覆盖或替换原输入框。刷新或重进页面后，只要持久化 timeline/session 仍存在 pending permission，就必须恢复同样的锁定 composer 状态。
- 权限决策卡片采用轻量审批 surface，不把所有 allow 选项渲染成高强调实心主按钮。只有 `pending` 权限展示完整可操作卡片，并允许穿透收起的 Activity / Agent 分支常驻；权限变为 `selected`、`rejected`、`cancelled` 等终态后，不再在 Activity 折叠详情中展示权限申请记录，工具请求及其终态由相邻工具行唯一呈现。标题与等待状态组成单一信息行，操作区使用两列紧凑按钮；allow 使用低透明度 accent surface，reject 使用中性描边，避免多个同级操作同时争夺视觉焦点。选项名称保持单行截断，但整个按钮必须复用 shadcn Tooltip 作为触发器，在鼠标悬浮与键盘聚焦时展示完整文本；按钮同时保留完整 `aria-label`，不得使用浏览器原生 `title` 或按字符数猜测是否溢出。
- 排查停止状态不得恢复持续性 ACP composer console 日志；如需再次定位停止链路，应优先补充状态矩阵测试或临时一次性断点式诊断，完成排查后必须移除。

### 侧栏加载与桌面 IPC 响应性

- workspace 元数据与完整会话侧栏是两个不同接口。`get_conversation_workspaces` 只读取 `StateConfig.conversationWorkspaces` 并返回 `projectId / workspacePath / name`，供上下文管理、运行模式选择等轻量选择器使用；这些页面不得为了 workspace 列表触发 task/run 扫描。
- `get_conversation_sidebar` 需要遍历多个 workspace 的 task 与 run 历史，属于阻塞文件 I/O。该命令以及 pin、unpin、reorder、workspace 增删同步、task 删除后返回侧栏的命令必须使用 async Tauri command，并把完整操作放入 `spawn_blocking`；禁止在 Tauri IPC 事件处理线程直接执行侧栏扫描，否则会阻塞自定义标题栏的 `startDragging`、窗口按钮和其他 invoke。
- 上下文管理的 Profile、Agent registry、MCP 列表、全局 SKILL、项目 SKILL 同样属于文件系统读取入口，统一复用 `spawn_blocking_command`。页面级延迟加载只减少不必要的工作量，blocking pool 边界负责保证确实发生读取时窗口事件仍可被处理，两者缺一不可。
- 侧栏数据只消费任务身份、会话元数据和 run 摘要，不消费 workflow 合法性、Profile 解析或任务详情。因此构建侧栏时每个 workspace 只读取一次 task 列表，每个 task 只读取一次 run 列表；不能复用会进行 workflow/Profile 校验并重复扫描 run 的通用 `task_summaries()`。
- 单个 task 的 run 历史损坏时，侧栏仍保留该 task 并把 run 列表降级为空；单个历史文件问题不能导致整个 workspace 从侧栏消失。完整错误仍由进入任务/运行详情后的专用接口返回。

### 停止
- 停止并重跑在顶部操作区
- composer 内也有 stop 按钮（ACP 会话停止）
- composer 内的 ACP 停止表示“中断当前响应”，不是 workflow 配置错误；停止后的 attempt 应显示为可继续暂停
- 会话内停止使用 `stop_active_session` 单一路径；旧 UI Run 停止与新 UI 侧边栏 run 右键“停止”使用 `pause_run`。新 UI 侧边栏停止菜单只挂在具体 run 行，不挂在任务/需求标题行；菜单打开和菜单内容二次右键都必须阻止 WebView 原生右键菜单。二者共享普通中断语义但作用域不同：`stop_active_session` 只停止当前 leaf/session，AI-DYNAMIC fan-out 中不会拖停兄弟 leaf；`pause_run` 停止整个 run，会把该 run 下所有 active leaf 一起写成 `paused + process-interrupted` 并分别发送 `session/cancel`。若运行线程控制句柄不可用，则通过 live ACP connection registry 对目标 attempt 的真实 ACP session 发 best-effort `session/cancel`。活跃 ACP runtime 不因 cancel notification 已发出就立刻退出，而是继续 drain 当前 `session/prompt`；cancel timeout 必须暴露为明确错误，不能 kill adapter 伪装成功。停止不是 kill run，不能把 run/round/node/dynamic node 写成 `killed`。
- 新 UI 侧边栏的 Workflow/AUTO task 只要存在 run，就必须展示 run 子列表；只有一个 run 时也展示 `run-001` 行，确保右键停止菜单始终挂在具体 run 行上，而不是回退到 task 行。Direct task 不展示 run 子列表，停止当前回复继续使用会话 composer 内的统一停止入口。
- 新 UI 侧边栏的置顶区与普通工作区是两个独立的列表区域；同一会话同时出现在两处时，run 子列表开合状态必须按区域隔离。置顶区内部一次只展开一个会话实例，普通工作区内部一次只展开一个会话实例，点击其中一区不得联动展开另一区的同一 task。
- 新 UI 侧边栏的选中高亮同样按区域隔离；同一会话同时出现在置顶区与普通工作区时，只高亮用户最后交互的那个区域实例，另一处保持普通展示，避免用户误判两处列表被同步选中。
- 停止反馈只保留在 composer 内：停止按钮进入 pending/禁用态，状态行显示“停止中”，输入按 canonical composer lifecycle 锁定；不得用全局或页面级遮罩覆盖会话消息区。一般执行阶段的 `stop_active_session` / `pause_run` 仍快速收敛；若 AI-DYNAMIC 正处于 workspace 一致性临界区，停止命令保持 pending，直到临界区完成后才返回。页面继续展示原有消息，不能增加前端 timeout 或把仍在等待的停止伪装成失败。侧边栏 run 级“停止”仍须立即关闭菜单并发起一次请求，重复提交由命令 pending 与请求版本隔离处理。
- 关闭客户端和启动时崩溃恢复与用户停止共享同一 interruption 语义：所有仍为 running 的 run、当前 node 和 AI-DYNAMIC descendants 都收敛为 `paused + process-interrupted`。`provider.pid` 不参与业务状态判断，只能作为 adapter process metadata 用于诊断和 orphan cleanup。
- 停止请求一旦落盘，任何迟到的 ACP success response（包括完整合法的 AI-DYNAMIC `dynamic-node-completion`）都不能写 success artifact、恢复外层 Runtime 或驱动 workflow / dynamic graph 跳到下一节点；runtime 必须在 provider 返回后同时确认当前 attempt 与 execution generation 仍是 running/current，确认已暂停则直接停止推进。只有用户显式点击“继续工作流”产生新的 execution，才允许重新进入 Runtime 控制。
- AI-DYNAMIC 的 checkpoint、fanout worktree 创建、merge 前 checkpoint、child worktree release 与整图结束 release 使用持久化 `PreparingWorkspace` 内部阶段。该阶段是 Graph + Git 的不可中断一致性临界区，不是新的用户暂停状态：正常情况下 composer 显示“正在准备开发环境…”，允许点击停止；点击后本地停止 pending 优先显示“正在停止…”。若 composer 锚定的是已经完成、正在等待 workspace 交接的 leaf，精确 session stop 必须升级为外层 run stop，先落盘 `Paused + ProcessInterrupted`，再等待当前 transition 完成并暂停 descendants；仍为 active 的并行 leaf 则继续保留单 leaf stop 语义，在同一临界区结束后兑现。已经创建的 worktree 不因停止被删除，继续工作流时复用现有 workspace catalog/tree。
- 当 selected AI-DYNAMIC leaf 的 ACP 与节点状态已经 terminal、但外层 dynamic run 仍在驱动下一节点或执行 `PreparingWorkspace` 时，Conversation VM 的 Runtime facet 继续投影为 active/running；普通交接阶段显示“正在启动下一节点”，workspace 临界区显示“正在准备开发环境…”。不得因 leaf terminal 把 composer 提前降为自由会话，也不得丢失停止按钮。
- runtime 异常、agent/provider 异常与 workflow DSL 无效必须分开提示：只有 `workflowValid=false` 或明确的 workflow validation error 才展示“修改/修复工作流”入口；`runtime-abnormal` 表示异常但可继续，恢复输入框并保留异常提示；provider/model/catalog/workspace 等 manual 可恢复异常也归入 `runtime-abnormal`；`error-blocked`、session failure、session killed 等不可继续运行期异常只提示查看错误原因，不默认引导用户修改工作流。
- 当前选中 session 已有 `diagnostics.lastError` 时，错误面板文案应直接拼接具体错误原因，避免用户再额外寻找日志入口。
- 新 UI 中，`process-interrupted` 都恢复普通输入框，但只有 `runMode=workflow/auto` 且后端 lifecycle 返回 `continueKind=action` 时展示“继续工作流”。该动作位于 composer 发送按钮旁；前端不得在 stop 响应后自行合成继续资格。发送文本只产生 `UserMessage + NonRuntimeControlled`，不调用 `run_continue()`；点击按钮才产生隐藏 `RuntimeResume + RuntimeControlled`，且不携带用户可见文本。continue command 返回的已持久化 active lifecycle 必须立即用于当前 leaf、composer 以及左侧 sidebar 中同一 task/run 的 `latestRun` 和 `runs[]` 摘要，使按钮从“正在继续”直接切换为“停止”、两级侧栏圆点同步变为 Running；不能等到下一节点启动后才校准，也不能把仅 ACP active 的 NonRuntime 普通追问误投影为 workflow run Running。本地 pending 只在权威 lifecycle 离开 continuable 后释放，不能因父级刷新稍晚而短暂回退成“继续工作流”。Direct 停止后只保留普通发送，即使首个 session 尚未完整建立也不进入工作流重跑提示。AI-DYNAMIC 的继续动作必须携带精确 leaf locator，不能通过外层 parent continue 批量恢复 paused worker。

## 会话信息栏（ACPSessionHeader）

- 单行布局：Agent icon + Agent 名称 + 可复制 sessionId + 操作按钮；权限模式属于可变运行配置，不在会话身份栏中展示
- Agent 名称与 sessionId 必须放在共享的文本基线容器中，并使用一致的行高节奏；外层图标与操作区仍按控件中心对齐，禁止通过单独的 top/margin 像素偏移补偿字号差异
- Direct 组合页头通过留白区分“会话标题”和“Agent 身份”两组信息：标题末尾保留约 12px 组间距，Agent 名称与 sessionId 保留约 6px 组内距；长 sessionId 默认显示前 8 位与后 4 位，中间使用省略号，Tooltip 与复制操作继续使用完整值
- 会话信息栏与运行标题栏保持同一套紧凑节奏：缩小上下 padding、降低主标题字号、压低按钮高度，减少双层头部对内容区的挤压
- 可编辑会话标题的悬浮提示统一使用项目内置 shadcn Tooltip，禁止使用 HTML `title` 触发 Windows/WebView 原生 tooltip；鼠标悬浮与键盘聚焦共享主题化提示样式
- Workflow/AUTO 的第二行作为元信息层，视觉权重需低于第一行：更小字号、更轻字重、更弱对比度，不与任务标题竞争主次；Direct 使用下述单层组合页头
- 运行标题栏与 ACP session header 统一消费独立的 `content-header` token 并只保留轻量底部分隔线；四套主题当前都将该 token 映射为 `var(--sidebar)`，使标题栏与侧边栏组成连续应用框架，并与消息阅读区明确分层。保留独立 token 是为了未来可只改色板映射，不改变组件接口；标题栏不得增加独立卡片、投影或嵌套灰块。
- 用户消息气泡使用独立的 `message-user` / `message-user-foreground` 语义 token，不复用 `primary` 混色。科技灰下采用 `#f3f3f3` 浅灰底与 `#202020` 深色正文，不显示可感知边框和投影；长消息仍应保持轻盈，不能形成大面积中灰实体面板。深色主题使用同源的中性高层 surface 与高对比文字。
- assistant 自然语言正文直接显示在页面背景上，使用实色 `foreground`，不再包裹白色卡片、灰色边框或投影。工具、思考、代码块和控制输出仍可使用必要的结构化 surface，从而让主阅读路径保持高白度与高黑度。
- 两个内置主题共享的默认 UI 字体家族保持 `Gold Band MiSans`，并由同版本 MiSans variable font 提供连续真实字重轴。无显式字重的排版基线固定为 300，位于该字体的 Light 250 与 Regular 330 之间；`font-medium / font-semibold / font-bold` 依次映射为 MiSans Medium 380、Demibold 450、Semibold 520，字体注册范围也封顶 520，不提供 Bold 630。正文、行内代码与使用 `font-normal` 的标签保持同一 300 基线，标题和 `strong` 继续通过 380–520 建立层级；不得用 opacity、阴影、伪粗体或静态 Light 冒充 300。
- Thought 使用低边界紧凑结构；展开正文最大高度为 `18rem`，超过后仅正文区域纵向滚动，保留主题滚动条、键盘焦点和 overscroll 边界。内部滚动不改变会话主视口的 canonical 贴底状态，也不自动追随流式内容抢走用户当前阅读位置。
- ACP 会话主消息流、raw frames 面板和 prompt-kit 聊天滚动容器使用 Gold Band 主题化滚动条；滚动条颜色必须来自主题 token（主色、muted、surface），科技灰主题使用无彩石墨与中性 surface 混合，不回退为系统默认灰色，也不引入蓝灰色偏。
- 主题源码契约测试必须在解析 CSS 前统一换行符，Windows CRLF 与 Linux LF 必须得到相同 token 验收结果；不得把工作区 checkout 的行尾格式误判为视觉回归。
- Gold Band runtime prompt 中的 `<hidden data-gold-band-hidden="true">` 段在用户消息气泡内默认折叠展示，折叠块与可见 requirement/goal 同属一个 bubble；展开后展示隐藏原文，再次点击收起。折叠块使用当前文字色的极低透明 surface，不使用白色 `background` 在浅灰气泡内再造一层亮卡片。用户消息行建立 inline-size container，结构 token `--conversation-message-max-inline-size: 82cqi` 只定义消息气泡允许使用的最大测量宽度，不直接作为最终正文宽度。组件在这个上限内创建不可见的同字体测量副本，通过 `Range.getClientRects()` 读取每个实际排版行的宽度；折叠态最终宽度取隐藏标签完整宽度与所有可见正文行宽度的最大值，展开态再纳入已展开隐藏正文行宽度，并向上取整为稳定像素值。消息行 ResizeObserver、展开状态和字体加载完成都会触发重新测量，因此窗口变宽只会改变文本的真实换行结果，不会让气泡随容器比例无条件线性增长。隐藏区使用嵌套 grid stretch：根节点、Trigger 和 Content 均不声明 `w-full` 等百分比宽度；外层先应用测量后的最终宽度，内层单列 grid 与 Trigger 的 `minmax(0,1fr) auto` 两列布局再自动铺满。超长内容达到消息列上限后换行，不使用固定 `rem`/像素宽度猜测，也不使用 inline-size containment 排除可见区域的宽度贡献。该规则适用于 workflow new 和 workflow resume，并覆盖会话态与旧工作台复用的 ACPChatDialog；用户手动追问和 runtime repair 不注入 hidden runtime context。hidden 后面的可见片段只在展示层去掉开头换行，真实 prompt 事件内容不变。
- 产物来源固定为当前选中 session（含 AI-DYNAMIC 内部节点）的 artifacts / attachments，不使用 run 级聚合占位数据
- 产物弹窗遮罩使用轻量弱化遮罩（低透明深色 + blur），主体面板保持半透明而不过度强调，不做厚重黑色卡片
- sessionId 与 Agent 身份同行，不再单独占行；长值采用“前 8 位…后 4 位”的紧凑投影，点击仍复制完整值，悬浮显示完整值，并在复制后显示会自动消失的轻量“已复制”提示
- sessionId Tooltip 的复制反馈采用 `idle -> copied -> closing -> idle` 单一状态生命周期：反馈到期时先保持“已复制”内容关闭 Tooltip，关闭过渡完成后才恢复完整 ID 内容；`closing` 阶段忽略悬浮重开，禁止在关闭动画中闪现完整 ID
- sessionId Tooltip 的窗口生命周期必须与复制反馈统一管理：应用失焦时立即取消反馈计时并关闭 Tooltip；恢复窗口后不接受 WebView 残留 focus/pointer 导致的自动重开，只有触发器真正离开后的新一次悬浮或键盘聚焦才可再次展示完整 ID

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
- task 输入附件的生命周期数据由 `authoring/inputs/`、provider `PromptBundle.attachment_metas` 和首条用户消息 `raw.attachments` 共同投影；格式注册表是三者共享的分类事实源，不能出现“文件已持久化但消息/Agent 输入缺失”的半状态

## Todo/Plan 任务面板

- 位于 composer 上方、AcpUsagePanel 下方
- 默认收起，显示任务进度摘要（如 "2/4 · 当前任务名称"）
- 展开后使用紧凑任务行展示完整条目列表；完成、进行中和待处理分别使用勾选、运行环和小号中性空心状态点，并辅以状态文案，禁止只依赖颜色或堆叠大号 Badge。面板摘要已经提供完成数，待处理行不得再显示序号圆环重复表达顺序。
- 仅显示当前 branch 的 todo；Agent 内部 plan 只在对应右侧 Agent 会话中展示，不回落到父分支
- 规范化边界为 plan 写入 `planOwnership = branch | unscoped`。只有 provider relation 或既有内部 branch 定位能够证明归属时才标记为 `branch`；缺少范围信息的 session-wide plan 标记为 `unscoped`，不得根据条目文本、事件邻近或 Agent 名称猜测归属
- 普通且没有 Agent execution 的根会话可以展示 `unscoped` plan；一旦当前 session 存在 Agent execution，根会话必须 fail-closed 隐藏 `unscoped` plan，防止 provider 聚合的子 Agent Todo 平铺到主会话
- 每次 plan 更新时面板实时刷新，不再在消息流中追加重复 plan 卡片

## Composer 配置栏

### 追问草稿生命周期

- 会话详情 composer 的未发送正文与待发送附件属于同一运行期草稿，由 ACP composer draft store 统一管理，不散落在 `ACPChatDialog` 的组件本地状态中。
- 草稿使用完整 `projectId + taskId + runId + roundId + nodeId + attemptId + outer attempt + branch` locator 隔离；切换 task、run、节点、attempt 或 Agent 分支后，再返回原会话必须恢复原正文、图片与其他附件，不能按显示名、当前选中项或末级 ID 反查。
- 草稿只在应用进程内保留，不写入 SQLite、localStorage、sessionStorage 或文件系统；退出应用后允许丢失。发送接受后、用户明确清空或删除附件时立即清除对应内容，普通会话切换和组件卸载不得清除。
- 浏览器 `File`、图片 object URL 与草稿同生命周期。运行期 store 必须同时限制草稿条目数和附件总字节数；淘汰、明确清空与应用退出时释放预览 URL，避免长时间会话切换形成无界内存增长。
- 当前 composer 只订阅当前 locator 的草稿；键入正文不得更新应用壳 Context、会话侧栏、历史消息或 Markdown。该能力继续复用 prompt-kit composer 与既有附件选择器，不新增第二套输入或附件组件。

- 新建对话 composer 与会话详情 composer 共享同一套 ACP 配置选择器。Agent 只提供模型时展示普通模型下拉；同时提供 `configOptions[category=thought_level]` 时，模型栏切换为单槽位复合下拉，权限仍是相邻的独立下拉，不按 Codex `reasoning_effort`、Claude `effort` 等具体 ID 写死。
- ACP select 配置以 `configOptions` 为当前协议事实源：模型目录、当前模型和值展示优先读取 `configOptions[category=model]`，权限模式同理优先读取 `configOptions[category=mode]`；旧 `models` / `modes` 只在对应 config option 缺失时作为兼容回退。两套字段同时存在但内容冲突时不得让旧字段覆盖 config option，也不得解析模型名称或 ID 中的 `(low)`、`[max]` 等 adapter 私有组合格式来推断思考强度。
- 复合下拉的第一层只展示“模型”和“思考强度”两个入口及其已选值，点击后进入各自选项。主下拉面板默认以触发器左边缘为锚点对齐，面板更宽时向右展开，避免向左悬出并打断相邻控件的阅读顺序。composer 配置菜单使用非模态 DropdownMenu，打开模型菜单后直接点击相邻权限触发器时，必须在同一次点击中关闭模型并打开权限，不得要求第二次点击。两个子栏使用受控 click-to-open 交互，同一时刻只允许一个展开，打开其中一个必须自动关闭另一个；同一使用位置的两个子选项面板固定向同一侧展开，避免因选项宽度不同左右跳变。只有同时存在模型与思考强度子入口的复合菜单在选择具体项后保持打开、等待点击外部关闭；纯模型菜单与权限菜单属于单项菜单，选择后立即关闭。会话详情内列表默认向上弹出，新建对话按可用空间弹出，超出高度时内部滚动。
- Gold Band 的模型与思考强度初始均为空，复合触发器统一显示“不指定”，表示不覆盖 Agent 自己的 `currentValue`；只选择模型时显示模型名，只选择思考强度时显示 `不指定 · 思考强度`，两者都选择后显示 `模型 · 思考强度`。发起会话前可清除任一选择；进入会话详情后，每个配置仅在自身 override 尚为空时提供“不指定”，模型、权限和思考强度一旦选择具体值便只能在具体值之间切换。
- 模型、思考强度和权限都是当前 ACP session 的可切换配置；选中列表项后立即更新会话快照，并在下一次 prompt 前通过 ACP `session/set_config_option` 或 provider 能力等价路径同步到底层会话。配置项点击只负责更新配置与菜单生命周期，不得主动聚焦 composer 输入框；PromptInput 的空白点击聚焦判定必须把普通、复选和单选菜单项都视为交互元素。
- 后续同一 ACP session 的每次追问和 runtime continue 只复用 Gold Band 的显式覆盖：`modelOverride`、`permissionModeOverride` 与 `configOptionOverrides`。不得从 Agent 返回的 `currentModelId/currentModeId/currentValue` 反推用户覆盖；未指定时继续交由 Agent 决定默认值。
- 运行时应用顺序固定为模型、权限模式、其余通用 config option。模型切换后以 Agent 返回的新 `configOptions` 作为后续配置事实源，通用选项必须按实际 option ID 和可选值校验。
- 复合下拉第一层未选择的子栏不显示占位值，触发器和已选态只展示名称；长描述只在具体选项中换行展示，不允许撑破触发器或越出窗口边界。协议解析统一收敛在 ACP session config 工具中，展示组件只消费归一化后的 id/name/description。
- 新建对话与会话详情中的模型、权限触发器统一使用“弱化配置名 + 主值”结构，例如 `模型  GPT-5.6-Sol · High`、`权限  不指定`；两类触发器共享同一套 36px composer 配置触发器视觉规范，统一宽度策略、间距、无阴影表面、边框、深色背景、箭头尺寸与焦点态。Composer 配置单选与复合选择统一使用非模态 Radix DropdownMenu，保证相邻菜单双向一次点击切换。

## 工具调用参数展示

- 工具调用默认使用低边界、无阴影的紧凑行，首行只承载操作名、单个关键参数 chip、状态和常显展开箭头；参数使用等宽字体与单行截断，完整输入输出仍在展开详情中查看。
- 展开详情使用左侧细竖线表达从属关系，不再嵌套新的高对比卡片 surface；长路径、JSON 和命令继续在自身内容区换行或滚动。
- 工具调用卡片展开后以有序列表展示工具输入参数
- 参数按来源优先级提取：rawInput > 结构化 fields > title/locations 解析
- 同标签参数保留多个不同值（如多个路径、多个查询条件）
- 语义化参数缺失时回退展示原始输入 JSON

## Agent 分支会话展示

- `Agent` 工具调用在所属父分支只投影为轻量 `AgentLinkRow`，展示 ACP 已确认的名称、说明、结构化状态、工具数和文件读写数；不提供 Collapsible，不在父消息流挂载 transcript，也不猜测“正在测试/搜索”等意图。
- 点击 Agent link 使用稳定资源 key 在右侧工作区打开或激活只读 Agent Tab。嵌套 Agent 在父 Agent 分支中继续显示相同链接，并打开另一个 Tab；同一 execution 原位更新，不因 streaming update 生成新行。
- Agent Prompt 作为目标分支的 synthetic 用户消息持久化；正式回答只在目标分支展示。launch tool 的 `completed` 仅表示分发完成，execution 状态由统一索引按 queued/running/waiting_permission/completed/failed/interrupted 管理。
- TODO 按显式 branch ownership 投影；根分支与任一 Agent 分支都不得依据文字猜归属。父分支只列直属 Agent，嵌套 execution 只在其直接父 Agent 会话中出现。待决权限只在 owning branch 展示可操作 intervention，同时向所有祖先 Agent link/Tab 投影 attention；决策完成后退出 intervention，不保留权限申请审计行。
- 只挂载激活 Agent Tab 的 `ConversationViewport`。分支事件窗口、滚动锚点、贴底和 `hasOlder/hasNewer` 保存在有限 LRU 中；切回 dirty Tab 时重新拉取该分支最新语义页。普通后台 streaming 不驱动所有 Tab React render。

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
- Composer 运行态继续使用既有秒级 canonical 计时，当前动作与 session 累计耗时以紧凑同行展示；计时跟随状态栏正文的字体和字号，仅使用 `tabular-nums` 稳定数字宽度，各信息组通过既有间距区分，不增加装饰分隔点、高频前端 timer、文字 shimmer 或第二套 loading 生命周期。
- completed run 上的 follow-up 仍可能存在实时 ACP prompt。后端必须读取 per-attempt `Starting / Running / CancelRequested` 活动状态；只有没有实时活动时，terminal runtime 才能压制磁盘残留的 stale `running` session snapshot。
- 前端的 `sending / awaitingResponse / cancelling` 只覆盖命令往返窗口。页面切换或组件重挂载后，停止按钮、计时和 token 展示必须完全由后端 lifecycle/session snapshot 恢复；输入策略由同一 lifecycle 加 run mode capability 恢复，Direct active 可输入入队，Workflow/AUTO active 锁定。
- prompt 写入 terminal session snapshot 前必须先将实时活动标记为 finished，避免终态事件到达后 UI 仍被旧活动状态锁定。

## ACP 会话 attachment、历史同步与有界资源

- ACP adapter process、Provider session attachment 和单次 prompt 是三个不同生命周期。AdapterConnection 以 `provider + adapter workspace` 复用；session runtime 以 attempt locator + ACP sessionId 复用；PromptRun 只覆盖单轮 `session/prompt`。
- 同一 attached session 的连续追问不得固定执行 `session/load`。发送前先检查 connection generation、本地 session config fingerprint 和 Provider freshness；三者均未变化时直接 `session/prompt`。
- 外部历史同步是 Agent 级 Beta 可选能力，配置字段为 `ManagedAgentConfig.externalSessionSyncEnabled`，默认关闭，不设置全局开关。Agent 管理 UI 的可编辑项必须与 `ManagedAgentConfig` 对齐：执行命令、参数、环境变量、`primaryAgentDir`、`compatibleAgentDirs` 和外部会话同步开关都要支持保存与回填；主卡片只展示命令、参数、环境变量和最近检测，Agent 目录与外部会话同步等高级属性只在修改抽屉展示。同步开关标题必须展示紧凑 Beta Badge 和可聚焦问号 Tooltip；Tooltip 解释“同步同一个 Session 在其他客户端中发生过的对话”，常驻说明明确提示“仅在确认该 Agent 支持跨客户端共享同一会话上下文时开启，否则可能造成历史顺序或上下文理解错误”。只有 Agent 能保证跨客户端共享同一线性上下文，或提供可选择的 branch/leaf 时才允许开启。
- attached runtime 保存创建时的外部同步策略。当前 Agent 配置从关闭变为开启时必须设置 `syncRequired`，下一次 prompt 前直接强制 `session/load`，不得先依赖 `session/list`；required sync 成功并完成 replay 收敛前不得发送本地 prompt，也不得用本地新 revision 覆盖 freshness baseline，load 失败不能回退到 `session/new`。
- 开关开启时，Provider freshness 优先使用 `session/list.updatedAt`，该字段只作为 opaque revision token 比较，不解析时间先后。revision 变化时先 load/import 外部历史，再发送本轮 prompt；Provider 不返回 `updatedAt` 时，attached session 直接 prompt，detached session 才 load。开关关闭时 attached session 不执行 revision probe，直接 prompt；detached session 仍正常 load/resume，但 load 的整段 Provider history replay 只进入 raw 审计，不进入 timeline，不能只过滤 user chunk 后留下孤立 assistant/tool 历史。
- `session/list` 是有界 best-effort freshness probe，单页超时 5 秒、最多 8 页；临时超时只影响普通 revision 探测，不得决定首次启用同步是否 load。Agent 配置属于 Provider 全局边界，保存前跨所有 workspace 检查 active prompt，并统一 detach/关闭该 Provider 的全部 idle session runtime 与 adapter connection。
- session title refresh 与 freshness baseline 独立：允许读取 `session/list.title`，但在外部同步关闭时不得更新 revision baseline 或触发 reload。
- 本地 MCP/cwd 变化不依赖 Provider revision。MCP server 先规范化对象字段并按稳定 JSON 排序，再参与 session config fingerprint；仅顺序变化不 reload，真实增删改在下一次 prompt 前 reload，并把最新 `mcpServers` 传给 Provider。
- session route 在 idle attached 期间仍由独立 event pump 持续消费，不能把有界 connection route 留给无人读取的 receiver。事件泵自身保持有界背压；prompt runtime 重新进入时继续从同一 pump 消费。
- session runtime 使用 foreground lease、idle TTL 与 LRU 有界保留；active prompt、permission/elicitation 处理中和前台 lease 内 session 不参与驱逐。会话详情页按后端返回的 renew interval 续租，页面关闭后自然停止续租。
- `acp.timeline.jsonl` 是 UI canonical timeline，不是原始传输审计。所有 upsert 先合并既有 canonical item，再计算语义指纹；仅 replay 的 `seq/timestamp` 变化不落盘，内容、状态、工具结果或 history placement 变化才追加 revision。
- timeline 达到配置化大小、patch/unique ratio 阈值，或读取旧文件时发现同一稳定 ID 存在语义完全相同的重复 revision 后，必须在文件锁内加载 canonical projection，并通过原子文件替换压缩为每个稳定 ID 一条 item。首次 `seq/timestamp/startedSeq` 和 history placement 保持不变；既有重复 replay 在下次打开 timeline 时自动收敛，`acp.raw.jsonl` 继续保存原始到达顺序并沿用独立滚动策略。
- 自动重试属于同一个逻辑 prompt turn：orchestrator 在进入 provider 调用前分配稳定 `promptId`，并在同一自动重试循环的每次 runtime 重建中作为 invocation 输入重复传递；普通 worker 与 AI-DYNAMIC leaf 共同消费同一 retry policy/backoff/max attempts，AI-DYNAMIC 只重试失败 leaf，不阻塞 sibling。`PromptBundle` 不得按 `session=new/continue` 丢弃该身份。`acp.snapshot.json.promptRetry` 是 ID 与 retry counter 的唯一持久化事实源，即使 session 已进入 terminal 状态也不清除；runtime 重建只读取该小字段，不扫描 timeline。每个用户 timeline 事件同步保存自己的 `raw.retry`，因此新 turn 覆盖 snapshot 后，历史失败消息仍可显示其累计重试次数。前端按 prompt ID 合并为一个用户气泡，绝不按文本内容猜测重试关系。每次 provider 调用持有其本次逻辑 prompt 的完整用户事件直到终态结算；已完成用户事件不属于 runtime hot cache，`failed` patch 必须通过该生命周期对象写回同一 timeline ID，不能依赖热缓存查找；`cancelled` 还必须由 attempt 级停止入口直接结算 durable timeline，覆盖 backoff 等无 runtime 所有者的空窗。编排层尚未耗尽同一结构化 retry policy 时，ACP runtime 必须先把该事件更新为 `processing + raw.retry(nextAttempt)`，不得短暂写入 `failed`；耗尽后才结算失败。用户消息气泡不是错误容器，不使用红色失败样式或“发送失败”文案，错误由统一会话错误面展示。重试中在气泡下显示进度，并以低强度浅→深→浅呼吸动画表达仍在进行；系统启用 reduced motion 时保持静态。成功后移除 footer，最终失败固定显示累计重试次数且保持静态；用户停止则该 prompt 自身结算为 `cancelled`，固定显示静态“重试 x 次后已停止”，后续继续的新 prompt 不得改写它。ACP JSON-RPC error、transport error 与未来协议标准化的 terminal error 必须归一化为同一结构化 runtime error，同时写入诊断、会话错误展示与终态 runtime snapshot；provider 私有 `session/update` 字段和普通文本 chunk 不作为 Gold Band 自动重试依据。确认 terminal 后不得在侧栏或 composer 保留 running 状态。
- 会话详情恢复必须使用 snapshot/live 水位交接，不能把一次 ready snapshot 当作“已追到最新”。全局 ACP 事件路由在页面卸载期间按 attempt + branch 保留有界 latest-wins 交接缓冲；同一稳定事件只保留最新引用，并同时受 branch 数、单 branch 事件数、单事件估算字节、单 branch 字节和全局字节预算约束。branch LRU 必须严格执行，活跃 listener 只属于组件生命周期，不能绕过 replay 缓冲上限或持有 timeline payload。超限时只保留 `headSeq + requiresCatchUp`，不得缓存完整 timeline 或触发非当前页面 React 渲染。页面重挂载先确认全局订阅就绪，再合并快照、交接事件；普通完整 replay 可立即完成展示交接，但缓冲仍需等待权威快照覆盖稳定 generation 后才能回收。存在 payload 缺口时，页面先展示已有静态内容，后台使用可取消、间隔封顶的指数退避持续执行 `afterSeq` 追平；缺口水位覆盖前不得进入 live 动画，不能用固定次数或固定时间窗口猜测持久化完成。
- timeline 增量游标表示“客户端已观察到的最新 revision 位置”。`afterSeq` 必须返回 `newestSeq > cursor` 的语义块，而不是只返回 `startedSeq > cursor` 的新块；否则从 seq 2 开始、更新到 seq 20 的累计 text/tool block 会被错误遗漏。增量候选必须先按 `(newestSeq, oldestSeq)` 的 revision/语义范围顺序分页，再按语义顺序展示；相同 `newestSeq` 的块作为原子 revision 组同页返回，不能因 page limit 切开后被下一游标跳过。快照、回放和 live 合并必须按稳定事件身份保持单调，旧快照不得覆盖更高 `endedSeq` 的正文或状态。
- optimistic 用户消息是 canonical timeline 的临时投影。提交时必须同时记录稳定 `promptId` 和当时最后一个 canonical `endedSeq/seq` 作为插入锚点；canonical 用户事件尚未到达时，optimistic 消息固定显示在该锚点之后、所有更高序号响应之前，不能无条件追加到当前事件数组末尾。canonical `promptId` 到达后直接移除临时投影并由权威序号接管位置；不得使用正文、秒级时间戳或当前数组下标猜测 turn 顺序。
- Markdown 流式展示由事件来源而非整个 session active 状态决定。快照、缓存恢复和水位补偿回放全部静态完整展示；只有本次页面完成水位交接后收到、且位于当前用户 prompt 之后的 live `textDelta/thoughtDelta` 才拥有唯一 streaming target。新的 user/tool/plan/permission/terminal 边界到达时必须立即结算前置 Markdown，不得出现工具已经展示而前置文本仍停留在打字机前缀，也不得在 completed run follow-up 重挂载后重播上一轮回复。
- live 正文按 stable stream identity 进入有界 latest-wins buffer，并以 125ms 短窗口合并发布。交互 quiet 只能由明确的 wheel、键盘滚动或滚动条 pointer 输入触发；普通 `scroll` 事件可能来自自动贴底、内容 resize 或布局恢复，不得据此延期正文发布。每批 pending 正文从首次入缓冲开始最多延期 250ms，持续交互也必须在 deadline 内发布最新快照；非 coalescable 生命周期事件继续先 flush 正文再应用终态。
- 会话滚动热路径只读取 `scrollTop/scrollHeight/clientHeight` 并按动画帧保存 O(1) 位置状态，不扫描消息节点或读取每条消息几何。精确 DOM anchor 仅在会话卸载/切换等低频边界捕获；历史 prepend 的分页 anchor 补偿继续使用独立的一次性测量。开发构建可放大布局成本，但上述约束对开发与生产构建同时生效。
- ACP 流式播放诊断使用独立、默认关闭的 `goldBand.debug.acpStreaming` 开关。开启后记录 router 接收、locator 匹配、水位交接、streaming target 决策、Markdown render，以及播放层初始化、DOM reconcile、约 500ms 汇总、超过 50ms 的 RAF 长帧、浏览器 Long Animation Frame、会话内容 ResizeObserver、自动贴底和 settle 原因；记录 ID/seq/状态、正文长度、renderer token 数、水位、积压、释放速率、block 索引复用/重建/扫描量、播放 tick 耗时、blocking/render/style-layout 时长、500ms 内尺寸变化与滚动写入次数，但不记录正文或脚本 URL。Long Animation Frame 只保留最重的三类 script attribution；Resize/贴底高频事件只汇总不逐次记录。积压归零时重置帧时钟和采样窗口，避免把无播放工作的空闲期计为长帧。诊断只保留最新 2,000 条有界内存记录，不在 token 热路径逐字符跨 IPC 落盘；通过 `window.__goldBandAcpStreamingDiagnostics.exportJson()` 导出，排查完成后关闭开关。
- Markdown 逐字视觉复用 Streamdown 2.5 正式动画扩展点生成 renderer token：canonical snapshot 始终完整进入单个 Streamdown 文档上下文，incomplete repair、block/AST 和未闭合结构只由 Streamdown/remend 管理；Gold Band 不运行截取 Markdown 前缀的 32ms RAF，不自行判断闭合或拆分 Streamdown 文档，只以至多一个轻量 RAF 按文档顺序释放已有 DOM token，并按积压动态追赶。播放器按 Streamdown block 身份缓存 token 索引，DOM 更新只重建变化 block，稳定 block 不重复扫描或校准。列表、标题和段落不得跨 block 并行播放。Markdown 首次挂载即为 streaming 时从零播放；已经以 static 完整展示的组件再次进入 streaming 时，必须在 Streamdown 生成 streaming token 后把当前全部 token 结算为可见基线，只播放其后追加内容，禁止把 static block 水位解释为 streaming character 水位。`streaming=false`、工具/用户/终态边界与会话切换必须取消积压并立即保留完整 canonical；消息 render identity 同时包含 event-window 与 provider event identity，禁止跨会话复用半完成展示状态。
- Direct、Workflow、AUTO、runtime continue/repair 与 AI-DYNAMIC leaf 共享同一 dispatcher/registry 语义，不允许重新引入 Direct 专用的长连接旁路。

## ACP Attempt Token 累计契约

- 会话详情的最小逻辑与统计单位是 attempt。底部“Token 用量”表示当前 ACP attempt 内所有 Gold Band prompt turn 的累计消耗，不是最近一次 Provider 调用的快照。Direct 多轮、停止后继续、runtime 重建、节点完成后在原 attempt 追问都继续累计；resume 到相同 Provider session 但创建新 attempt 时，从新的 attempt 重新计数。
- Provider `PromptResponse.usage` 是单轮 prompt 增量。runtime 必须先解析为 `AcpPromptTokenUsage`，再通过唯一的 `AcpAttemptTokenTotals` 状态累加；禁止把单轮值直接覆盖 attempt 累计值，也禁止使用字段 `max()`、上下文窗口 `used` 差值或前端消息窗口估算累计消耗。
- `acp.snapshot.json` 同时保存两个不同领域的数据：`inputTokens / outputTokens / cachedReadTokens / cachedWriteTokens / totalTokens` 保留最后一轮 prompt 快照，供后续单独讨论的节点指标链路使用；`attemptInputTokens / attemptOutputTokens / attemptCachedReadTokens / attemptCachedWriteTokens / attemptTotalTokens` 保存会话 UI 当前 attempt 用量的物化快照。两组字段不得在正常 UI 投影中互相 fallback 或混用。
- attempt 目录必须维护独立的 `acp.prompt-usage.jsonl` 写前日志。首次启用时先写一条 `attemptBaseline` 固化已有 snapshot 累计；每轮在调用 Provider 前持久化 `promptStarted(turnId, turnSeq)`，收到成功 `PromptResponse.usage` 后立即持久化 `promptCompleted(turnId, usage)`，随后才能把结果投影到内存状态和 snapshot。日志追加必须持有路径级锁，完成 `flush + sync_data`，并在下一次追加前截断不可解析的半行；完整但缺少换行的尾记录必须保留。
- `acp.prompt-usage.jsonl` 是 attempt 累计恢复的权威事务记录，`acp.snapshot.json` 只是可重建的物化缓存。runtime 重建或会话读取时按稳定 `turnId` 去重并重放 baseline 与 completed；若崩溃发生在 `acp.raw.jsonl` 已记录 Provider 响应、但 `promptCompleted` 尚未落盘的窗口，则把未完成的 `promptStarted` 与 raw 中的 `session/prompt` 成功响应配对，补写 recovered completion。修复必须幂等，同一 turn 不得重复累计。
- `acp.raw.jsonl` 可独立滚动，因此 raw 修复只处理 baseline 时间之后的结果；不得用不完整的滚动窗口覆盖或降低已有 baseline。升级前的旧 attempt 若没有 `attempt*` 字段，以最后一轮字段建立不下降的迁移 baseline；这只是一次性持久化迁移，不改变 UI 禁止读取最后一轮字段的契约，也不承诺从已经滚掉的 raw 中还原不可得的历史轮次。
- runtime 在同一 attempt 内重新创建、停止后继续或节点完成后追问时，从当前 attempt journal 恢复 `AcpAttemptTokenTotals` 后继续累加；保留中的 attached runtime 则在同一内存状态中继续累加。新 attempt 不继承旧 attempt totals，即使两者恢复的是同一个 Provider session。单轮响应缺少 `totalTokens` 但存在分项时，使用输入、输出、缓存读、缓存写的饱和加法计算该轮 total；缺失分项不清空既有累计值。
- `usedTokens / contextWindowSize` 是上述 attempt counter 的例外：它们是 Provider session 的 latest-value gauge。同一 `sessionId` 创建新 attempt 时，runtime 只从 `continue_ref.snapshotFile` 继承这两个字段，并在当前 attempt 首次 running snapshot 中物化，保证新会话 UI 直接读取当前 leaf 的 `selectedSession` 即可恢复上下文占用；不得继承旧 attempt 的 `attempt*Tokens` 或 `timing`，也不得在前端扫描或聚合多个 attempt。`snapshotFile` 的 sessionId 必须与 continue 的 sessionId 一致，避免跨会话串值。
- `AcpUsageVm.inputTokens / outputTokens / cachedReadTokens / cachedWriteTokens / totalTokens` 对聊天 UI 投影累计字段。timeline 中的 `usage_update` 只负责 `used / size / cost` 等上下文状态，不得用最近一轮 usage breakdown 覆盖累计字段。
- 会话 composer 上方的信息栏按“当前运行状态、会话累计、上下文窗口”排列；运行状态只在活动时显示，不进入 `PromptInput`，也不再横向展示 `used / size` 数字或 Token 明细。圆环采用 24px 紧凑尺寸，中央只显示整数占用百分比的数值部分，不显示 `%`，避免短标签在内圈拥挤；触发器的无障碍标签仍读出完整百分比。进度弧按显示百分比分段并复用主题状态色：`<60%` 使用 `gold-success`，`60%–<75%` 使用 `gold-running` 作为信息提示，`75%–<90%` 使用 `gold-warning`，`≥90%` 使用 `gold-danger`，未知值使用 `muted-foreground`。该颜色只是基于现有 gauge 的辅助视觉投影，不代表 Provider 的真实 compaction 阈值；精确数字与无障碍标签仍是主信息。鼠标悬浮或键盘聚焦后通过项目 Tooltip 展示“占用 used / size”，不重复百分比，并逐行展示 Provider 已上报的输入、输出、缓存读、缓存写和总量。圆环直接消费当前 `AcpUsageVm` 投影，不新增轮询、定时器、缓存或第二份 usage 状态；普通 text/thought/tool 流式更新不得触发该信息栏重渲染。

## Agent 单轮回复通知

- 系统通知区分“workflow run 完成”和“ACP prompt turn 完成”。普通 Workflow/AUTO 的自动运行完成继续使用“任务完成”；Direct 首轮、Direct 后续追问，以及 Workflow/AUTO 节点完成后的手动追问统一使用“{Agent} 回复完成 / 回复失败”。
- 手动追问通知只覆盖 `submit_conversation_prompt -> acp-prompt` 的非 runtime continue 路径。停止/异常后的 runtime continue 仍属于 workflow 生命周期，由既有 intervention/run completion 通知表达，不能同时再发一条 Agent turn 通知。
- 每个手动 prompt 在进入 ACP 前必须拥有稳定 `turnId`。前端未提供时由后端生成并写入 prompt event；通知去重键必须包含 `run / round / node / attempt / turnId`，同一 attempt 的连续追问不能互相去重。
- turn 终态统一为 `Completed / Failed / Cancelled`。Completed 和 Failed 产生通知；用户主动停止对应 Cancelled，不产生完成或失败通知。adapter transport interrupted 属于 Failed，不得伪装为用户停止。
- Direct 首轮仍由内部单 Worker run 驱动，但 `RunCompleted` 事件必须携带从 `authoring/conversation.json` 固化的 Agent 展示身份。成功的 Direct `RunCompleted` 只更新运行状态，通知必须延后交给同一 prompt queue 批次决策；失败仍立即通知。通知订阅器不得根据 `direct-agent` 等节点 ID 判断 Direct，也不得依赖当前 UI 工作区回读元数据。
- 当前窗口失焦、最小化、隐藏，或用户正在查看其他 task/run/session 时发送通知；当前目标 session 正在前台可见时抑制通知。permission 与 elicitation 继续沿用即时通知，不等待 turn 结束。
- 同一 Direct attempt 自动连续消费待发送队列时，从触发该批次的首条普通 prompt（包括 Direct 首轮）开始，所有成功 turn 都先进入统一的完成决策：`AcpPromptLifecycleEvent::Finished` 携带稳定 `promptId`，调度器等待 600ms 用户优先窗口并尝试领取后继。`AcpTurnFinished.batchProgress` 固定包含 `completedReplyCount + continues`；实际领取成功时仅累计计数并标记 `continues=true`，系统通知层不为该中间成功单独弹窗。队列清空、暂停、被用户优先输入抢占或无法继续领取时，终点事件携带整个连续批次的累计回复数并立即清理批次状态：计数为 1 使用“{Agent} 回复完成”，大于 1 使用“{Agent} 已连续回复 X 条”。批次计数属于当前进程内正在执行的 provider 生命周期，失败、取消或应用退出必须清理，不写入 durable prompt queue；应用重启后旧 provider turn 不可能继续回调。不得按 `turn-queued-*` 前缀区分首条与后续消息。Failed 始终立即通知，Cancelled 仍不通知；权限、elicitation、运行异常以及其他 project/session 的通知不参与合并。
- 通知正文不包含 Agent 回复原文、工具参数或附件内容，避免在操作系统通知中心泄露会话正文；点击“查看详情”仍定位到对应 task/run/attempt。

## ACP 斜杠命令目录与输入交互

- Gold Band 将 Agent 通过 ACP `available_commands_update` 公布的条目称为“ACP 原生命令”。ACP 没有标准 SKILL 发现接口，因此该列表不是最终命令目录；Doctor 还要从当前 Agent 的 Skill 读取目录扫描用户级与 workspace 级 `skills/*/SKILL.md`，只解析 `name / description` 元数据，不读取正文，也不进行 prompt injection。
- `ManagedAgentConfig` 直接保存公共/全局 `primaryAgentDir`、可选 `projectPrimaryAgentDir` 与 `compatibleAgentDirs`，不再由 Agent 类型分支推导目录。项目主目录为空缺字段时表示全局和项目共用公共主目录；字段存在时表示拆分作用域。`AgentSkillDirectoryPolicy { global, project }` 只根据实例配置生成，每个作用域的写列表为该作用域主目录，读列表为该作用域主目录加共用兼容目录并去重；所有消费方统一在 Agent 目录后追加 `skills`。Pi 默认使用全局 `.pi/agent`、项目 `.pi`、兼容 `[.agents]`，其他现有模板默认不拆分。
- 最终目录按 `ACP 原生命令 > 读取目录发现的 SKILL` 合并，并以命令名不区分大小写去重。ACP 条目保留 `description / inputHint`，SKILL 条目使用 frontmatter 的 `name / description`；扫描支持 Agent 根目录下的 `skills` 以及 `.codex/skills/.system` 这类一层容器目录和 Skill 符号链接。
- 命令菜单只是一份名称级展示索引，不决定 Agent 最终读取哪个 SKILL 实例。同名 SKILL 的展示元数据暂按“主目录优先于兼容目录；同一目录优先实体目录、再软链接”的确定性顺序选取，即 `主目录实体 > 主目录软链接 > 兼容目录实体 > 兼容目录软链接`；选择后仍只把命令文本发送给 Agent，由 Agent 按自身规则解析实际实例。
- 命令目录的数据模型为 `AcpCommandCatalog { agentType, workspaceKey, acpCommands?, commands, updatedAt }`，其中 `acpCommands` 保存未混入 SKILL 的原始 ACP 列表，`commands` 保存最终列表，命令项为 `AcpCommandItem { name, description, inputHint? }`。目录必须以 `{agentType, workspace}` 为联合身份，因为同一 Agent 在不同 workspace 可发现不同的项目级 SKILL；查询目录时从原始 ACP 列表重新扫描，保证 Skill 增删不会被旧合并结果残留。
- 桌面端把目录持久化到 `~/.gold-band/desktop/agent-command-catalogs.json`。自动 Agent doctor、手动 doctor、活跃 ACP 会话的 `available_commands_update` 都会更新原始 ACP 列表并重建最终目录；SKILL 创建、删除或同步目标变更成功后异步刷新当前 workspace 的已配置 Agent，不阻塞 SKILL 保存链路。旧目录文件没有 `acpCommands` 时兼容读取，并在下一次 Doctor 后迁移为可精确重扫的新结构。
- doctor 的 `session/new` 与命令通知存在并发窗口。连接层必须为尚未注册 route 的 session frame 提供有界、带 TTL 的早到缓冲，并在 route 注册时按序补投；不得依赖固定 sleep 掩盖消息丢失。doctor 在 session 建立后只追加一个有上限的命令发现等待窗口，随后立即清理诊断 session。
- 快速对话仅在 Direct 或固定 Agent 的 AUTO 模式中展示该 Agent 的目录；动态 AUTO 和尚未解析 Agent 的 Workflow 不展示 Agent 专属命令。会话详情页使用当前 ACP session 的 provider 与 provider cwd/workspace 查询目录。
- 输入内容仅匹配独立的 `/query` 时打开菜单，命令字符支持 Unicode 字母/数字以及 `.`、`_`、`:`、`-`，因此中文 Skill 名不会被目录或输入过滤丢弃。标签解析必须先读取最长合法命令 token，再检查其后的首字符是否为分隔符；不得因 `-`、`.`、`:` 同属 Unicode 标点而回溯成较短命令。输入空格、`,`、`，` 等分隔符后匹配立即失效并关闭菜单；若分隔符前是当前目录中的完整命令，则输入区把该命令前缀投影为标签。标签绝对定位在首行，通过共享 `ResizeObserver` hook 测量真实宽度并只设置 textarea 的首行 `text-indent`；textarea 自身始终保持完整宽度，因此显式换行和自动折行从输入区左边缘开始，不得形成贯穿所有行的标签列。标签与 textarea 首行共享基于 `rem` 的排版节奏和顶部基线，不依赖物理像素，随系统缩放、窗口 DPI 与根字号变化；颜色只使用 `secondary / secondary-foreground / border` 语义 token，摘要通过共享 shadcn Tooltip 展示。分隔符与后续正文继续由原生 textarea 编辑。删除分隔符、破坏命令名或切换到不含该命令的 Agent 后立即恢复普通文本，再次形成“完整命令 + 分隔符”时重新标签化。刚从菜单写入的标准 `/${name} ` 状态按一次 Backspace 时，输入状态机只移除标签成立所需的尾随分隔符，保留完整 `/${name}` 为普通可编辑文本，并把光标放到文本末尾；第二次 Backspace 再交给 textarea 删除普通字符。该转换不得依赖 blur、DOM 层标签节点或浏览器恰好保留的 selection。方向键移动时选中项必须跟随可见滚动区域；Esc 或点击菜单外关闭，但保留输入中的 `/`，用户删除并重新输入后可再次打开。选中后写入 `/${name} `，标签只属于前端显示投影，发送给 ACP 的值仍是完整普通文本。
- 菜单使用共享的 shadcn `Popover + Command` copy-in 组合，`CommandList` 是唯一滚动容器；命令名、描述、输入提示使用紧凑的小字号层级，`inputHint` 使用弱化标签而不是与描述拼成一段文本。键盘与鼠标选中态统一由 cmdk `data-selected` 驱动，并同时使用透明背景、内描边和左侧短强调条三层主题语义信号；浅色主题使用低透明度 `primary` 蓝色，深色主题使用 `foreground` 叠层，保证风格统一且均可辨识。切换 Agent/workspace 时以目录联合身份隔离快照，旧 Agent 的命令不得在新目录加载期间闪现。
- 快速对话的命令列表使用同一 `Command` 内容，在首行输入下方以带圆角的绝对定位覆盖层展开，不参与 composer 高度计算，因此打开菜单时主输入框整体尺寸保持不变；列表左右边缘与快速对话主输入框外边缘对齐。会话详情继续使用 composer 上方的 Popover，并以 Radix anchor 实际宽度作为菜单宽度，使两侧与 composer 严格对齐。
- 用户通过 Esc、点击外部等方式关闭当前 `/query` 后，关闭状态按稳定的 `{agentType, workspace}` 目录身份与当前输入值保留在前端运行期；切换页面再返回不会因为组件重新挂载而重开。输入值改变或删除后允许重新触发；切换 Agent 时清除新 Agent 上的关闭状态并展示其命令。
- 方向键改变选中项后，菜单通过直接持有的 `CommandList` ref 调整唯一滚动容器的 `scrollTop`，选中项位置只相对该容器计算并执行最小滚动；不得叠加第二层滚动组件、动态查找 DOM 父节点或使用跨父节点的 `offsetTop`。
- 鼠标或键盘选中命令后，命令 controller 必须在写入 `/${name} ` 并关闭菜单后通知 composer 恢复 textarea 焦点，使用户可以直接继续输入或再次按 Enter 发送。焦点恢复统一延迟到下一动画帧，等待受控值、标签投影和 Popover 关闭完成，并同步把 selection 放到 textarea 当前可见值末尾；快速对话原生 textarea 与会话详情 prompt-kit textarea 都通过显式 ref 接入，不允许使用 `querySelector` 猜测输入框。命令标签解除后复用同一焦点/selection 生命周期。若 textarea 在恢复执行前已进入 disabled 状态，则跳过聚焦。

## Prompt turn 文件变化

- 会话之间的导航必须按完整 `projectId + taskId + runId` 身份提交目标 locator。用户点击后立即切换侧边栏选中态、路由、主内容 scope 与右侧工作区 scope；旧会话正文不得继续占据主区域。目标 `ConversationRunVm` 尚未就绪时，主区域在最终内容位置展示品牌加载态；同一 run 内切换 session 时也先提交 `selectedSessionKey` 并清空不属于目标 locator 的旧 `selectedSession` 投影，再异步读取目标内容。
- 侧边栏 task 行、run 行、搜索结果和通知跳转不得直接写 `conversationPage`，必须统一进入会话导航接口，由该边界在路由提交前保存当前 run 并恢复目标 run 缓存。任何旁路都会让缓存命中失效并重放全屏 loading。
- 导航请求使用单调递增 request id；快速连续选择时只有最新请求且返回快照完整匹配目标身份才能提交，较慢的旧请求即使最后返回也必须丢弃。加载态是 locator 对应内容的 transient 投影，不持久化、不复制 canonical session 生命周期。目标会话的实时订阅在首轮目标快照提交后启动，避免旧响应覆盖新选择。
- ACP 首屏状态只由当前完整 session locator 的首次 `getAcpSession` 正文请求生命周期决定：请求未完成时始终显示品牌加载态，成功后根据返回内容直接进入 timeline 或空态，失败则进入错误态。run/session tree、switch 摘要、本地缓存、session established 或 live shell 都不是该请求的完成信号，不得通过 session 终态、事件数量或摘要字段推测加载是否结束。
- 已成功 hydrate 的 ACP 正文缓存是上条的明确例外：它必须使用完整 event-window identity 与仅由成功 `getAcpSession` 写入的运行期 `contentHydrated` 标记，不得由 summary/session prop/live event 伪造。命中时立即展示有界缓存内容并在后台 revalidate；刷新失败保留可用缓存，不回退到全屏 loading。
- Conversation run 轻量摘要与 ACP event window 分别使用最多 12 项的内存 LRU，共用现有 canonical locator，不持久化。切回已访问会话时，run 与正文均命中才允许直接恢复完整页面；任一层未命中则保持单一连续的页面级品牌加载态，不先显示全屏 Logo、再切换为会话框架内第二个 Logo。
- 会话、ACP 初始内容和运行中待首条消息统一复用品牌加载组件。组件只引用 `/logo.svg` 作为 Logo 真源，使用 `opacity + transform` 呼吸动画，并为 `prefers-reduced-motion` 停用动画；替换公共 Logo 资产后所有品牌加载态自动同步。
- 文件变更详情继续通过独立受控接口读取，不扩充 Conversation 主 DTO。页面目标快照提交后再后台预取当前 selected branch 时间线尾部最近 12 个 change set，并写入 96 项有界 LRU；预取不属于导航关键路径，失败也不影响会话内容，`TurnFileChangesCard` 回到稳定占位与原接口错误处理。

- 每个可见 prompt 使用稳定 `turnId/promptId`；hidden repair 继承最近可见 turn，不生成第二张用户可见文件卡。完成、失败和取消都会结算已经捕获的变化。
- 文件变化的唯一事实源是当前 prompt 生命周期内 ACP `toolCall/toolCallUpdate` 的标准 `content[type=diff]`。运行时不扫描目录、不读取 live 文件、不调用 Git，也不按 write/edit/shell 等工具名猜测。
- 同一个 `toolCallId + path` 的多次 update 是同一工具操作的流式修订，必须先取最后一个 event revision，不能误当成顺序 mutation；不同 tool call 才按 `eventSeq + contentIndex` 折叠。相邻 hash 不连续时保留证据并标记 partial，最终恢复原版本时不展示。
- 每份 old/new 正文先写入 attempt 级 BLAKE3 CAS，再追加 durable mutation journal；finalized change set 独立保存，timeline 只追加 summary 与 `changeSetId` 指针。历史查看只读取捕获版本，不受磁盘后续修改或删除影响。
- 变更卡是 prompt turn 末尾无头像的结构化行，持久化事件的 `startedSeq` 必须等于卡片自身终态 seq；历史读取发现旧指针仍绑定 prompt 起始 seq 时统一修正到终态位置，保证实时与重载顺序一致。修改打开右栏 unified diff，新增打开该轮 after 原文，删除行不提供点击或键盘焦点；无变化不渲染空卡。
- 用户点击停止、provider cancel 或 prompt 失败都属于 turn 终态，必须结算已经收到的标准 diff。直接杀进程只能依赖已落盘 mutation journal，不能承诺生成尚未来得及写入的终态卡片。
- shell/Bash 命令本身不是文件变化事实源。只有 provider 对该 tool call 返回标准 `content[type=diff]` 才能统计；若 `rm`、重定向或脚本写入只返回普通 stdout/完成状态，Gold Band 不解析命令文本、不读取磁盘补偿，也不会把该操作猜成文件变化。
- 用户消息附件和 canonical artifact 保持各自消息归属，点击后打开右侧会话资源，不进入文件变化卡。Conversation 主 DTO 不再聚合当前 session 的 artifacts/attachments，composer 上方也不再显示独立资产展开栏。
- 根会话和 Agent branch 按持久化 branch ownership 各自查询 change set。前端不根据路径或自然语言推断归属，也不把 sibling branch 的变化投影到当前会话。
## Composer 附件与图片工作区

- 快速对话与会话详情追问的未发送图片使用同一 `draft-attachment` 右侧工作区资源；点击附件 chip 不打开遮罩式图片 Dialog。图片被移除、清空或随 prompt 提交后，必须在同一事件链关闭对应预览 Tab，不能保留引用已释放 Object URL 的僵尸资源。
- Composer 上下文功能区必须与其下方正文使用同一水平内容 inset：快速对话的附件缩略图、命令标签与普通文字共用无额外缩进的左边缘；会话详情追问的附件/引用标签与 prompt-kit textarea 共用 `px-3` 左边缘。共享上下文组件不得内置一套固定水平 padding，否则不同 composer 外层 padding 会产生错位。
- 系统文件选择器得到的本地图片不得把真实文件路径交给 WebView；桌面后端在 blocking pool 中读取元数据并签发短期 preview grant，前端只消费协议 URL。粘贴、拖放和浏览器文件选择继续使用受草稿生命周期管理的 Object URL。
- 已发送消息图片、artifact 图片与未发送图片统一使用右侧图片看板。图片面板的内容承载层与看板必须形成连续的 `min-height: 0` 纵向 flex 链，让手势 viewport 占满标题栏以下的全部可用高度；图片在该真实视口内水平、垂直居中，背景只使用明暗主题语义纯色，不使用透明棋盘格。普通滚轮与触摸板双指滚动不得缩放或平移；只有 `Ctrl + 滚轮`、浏览器映射为 Ctrl-wheel 的原生触摸板 pinch、触摸屏 pinch 和工具栏按钮改变缩放，放大后可用鼠标拖拽平移；“适应窗口”回到由视口约束计算的完整图片尺寸，不等同于最小缩放。
- Ctrl-wheel 缩放必须经过独立输入适配层：Gold Band 自有的稳定 viewport DOM 持有原生非 passive `wheel` 监听，不把监听 ref 下放给可能覆盖外部 ref 的第三方 transform 容器；监听负责阻止 WebView 页面缩放，先按 `deltaMode` 将像素、行、页单位归一化，再限制单事件最大增量，并用乘法曲线把结果约束在 `0.1–8`。不得把平台原始 `deltaY` 直接交给缩放库的 step 算法，否则 Windows 高分辨率滚轮或触摸板会一步跳到上下限。缩放以指针位置为中心，同一动画帧内的连续输入合并为一次 transform；缩放百分比只更新看板内部 DOM，不得触发会话历史、Markdown 或 composer 重渲染。触摸屏 pinch 与鼠标拖拽继续由 `react-zoom-pan-pinch` 处理。
- Windows 精确式触摸板 pinch 的能力边界位于 WebView2，不是普通 DOM pointer：桌面窗口必须在 WebView 创建前开启 WRY `zoom_hotkeys_enabled`，使其对应的 WebView2 `IsPinchZoomEnabled` 允许 pinch 进入 Chromium 输入链。应用根节点用一个 capture、non-passive guard 阻止 Ctrl-wheel 与缩放快捷键改变整个桌面页面，但不得停止事件传播；图片 viewport 因此仍能消费同一 Ctrl-wheel 事件并执行局部缩放。图片区域外捏合、`Ctrl +/-/0` 均不得改变应用页面倍率。
- 触摸板 pinch 与 Ctrl-wheel 共用指数缩放灵敏度 `0.003`；相较上一版 `0.002` 提升 50%，相较初版 `0.0015` 提升 100%，减少手指需要移动的距离。单事件归一化增量仍限制为 `120px`，因此任意单次极端输入从 100% 最多变化到约 143% 或 70%，不得以提速为由取消限幅或回退到线性 step。连续手势的每帧变换在提交给缩放组件前必须原子写回权威 transform ref，后续输入不得仅依赖第三方组件可能滞后的回调状态。
