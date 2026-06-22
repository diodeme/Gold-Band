# 会话式运行时

## 信息架构

会话运行时窗口是用户与 agent 交互的核心区域。左侧选中最小单位是 run，右侧主区域永远展示当前选中 session 的具体对话。

运行态身份以 `projectId + taskId + runId + session locator` 为准。会话模式中查看、继续、停止、权限响应、模型/权限配置、raw frames、产物/附件读取都必须作用在该 `projectId` 对应 workspace；查看历史 run 不提升最后活跃 workspace。只有成功创建或重跑产生新 run 后，该 `projectId` 才成为最后活跃 workspace，并在从会话模式切回工作台时同步为旧 UI 当前 workspace。

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
- AI-DYNAMIC 内部节点的 session 状态来源于 dynamic graph 中的节点状态（`dynamic/nodes/<node-id>/node.json` 或 `graph.json.nodes`），ACP attempt 目录只代表聊天会话记录，不作为工作流节点成败状态来源
- 当 runtime attempt 已因 `process-interrupted / waiting-for-user-input / error-blocked` 进入可继续暂停时，session tree 与 composer 的用户态状态必须继续展示为 `paused`；此时 ACP snapshot/session 被写成 `cancelled` 只代表底层会话传输已结束，不能覆盖 runtime 的“可继续”事实
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
- **前端兜底 hydration**：若第一条 live event 到达时 base session 仍缺少系统提示词、配置枚举或首个 Gold Band 用户消息，前端触发短重试 `getAcpSession` 从磁盘补充元数据；同一 session 只有拿到可展示 metadata 后才视为 hydrated。
- **event-only shell**：`createLiveAcpSessionShell` 只在没有任何 base session 且 runtime 确认为运行中时创建临时渲染壳，不作为稳定元数据来源；壳中不含 system prompt 与 model/config 字段。
- **session 等价判断**：`sessionsEquivalent` 必须比较 session config 与 adapter 元数据签名，使后端在启动阶段发出的元数据-only session 快照（事件数可能没有变化）能刷新 UI。模型/权限栏只要存在可选项就应展示，不以 `currentModelId/currentModeId` 是否已归一化作为隐藏条件。

### 自动切换规则
- 上一个 session 完成 + 消息窗口在底部 → 自动切换并折叠历史
- 用户不在底部（正在看历史）→ 不自动切换、不折叠
- 用户通过 session tree 或工作流图入口手动切到任意 session 后，自动跟随立即解除；后续新 running session 只在后台推进，不抢占当前查看中的会话
- 当前选中 session 因 runtime 自然完成而从 active 变为 terminal 时，如果用户仍在底部且未手动切换，session auto-follow 进入 pending 状态；后续同一 run 的新 active child session 首次 live event 到达时可以切换过去。
- 自动跟随分为两层：消息列表的贴底 pin 控制当前 session 内流式内容是否滚到最新；session auto-follow 控制是否随 workflow 切到新的 active session。用户滚回当前活跃 session 底部时，恢复贴底 pin 并恢复 session auto-follow；用户滚回历史/非活跃 session 底部时，只恢复当前消息贴底，不切换 session。
- 顶部运行中节点 chip 是显式“跟随当前活跃 session”入口：点击 active chip 且消息窗口位于底部时，重新进入自动跟随；live event 到达或完整 run VM 刷新不能单独恢复自动跟随
- 刷新 run VM 时若未满足自动跟随条件，前端必须继续保留当前 `selectedSessionKey` 与当前 session payload，不能因为其他 session 的 live event 或后端默认 selected key 回退到最新 running attempt；若手动切换与已排队的 live refresh 同时发生，仍以最新手动选择为准
- 会话页内“进入 run 时重置自动跟随”的前端 effect 只能绑定 `runId` 等稳定 run 身份，不能依赖父组件每次重建的回调引用；否则 live refresh 触发父组件重渲染后会误把手动关闭的自动跟随重新打开
- 手动切换后是否恢复自动跟随，必须以 `run.activeSessions` 中是否仍包含当前选中 session 为准，不能仅依赖该 leaf 自身的 `runtimeDisplay.tone`，避免树状态短暂不一致时把已完成 session 误判成仍可跟随
- 前端所有完整 `ConversationRunVm` 快照进入 React state 时必须走统一合并入口，不允许调用点直接覆盖；合并入口负责保留当前 selected key、阻止 ACP `unknown` 空快照降级 runtime active 状态，并在 run 仍运行但 activeSessions 暂空时从 selected leaf 补出临时 active session。合并后 `selectedSessionKey` 与 `selectedSession / artifacts / attachments` 必须属于同一个 leaf；若 live refresh 或旧的手动切换请求返回了其他 session 的 payload，前端必须丢弃该 payload，而不是把它套到当前选中 key 上。用户通过 session tree 切换到目标 session 后，目标 `selectedSession` payload 回填前属于详情加载中状态，右侧主区域显示中性加载，不得短暂展示 ACP 会话失败横幅；只有目标 session 详情请求完成后仍确认没有 session/live shell，才展示缺失 ACP session 错误。
- 只有一个 session 运行中 → 自动展开该 session
- 多个 session 运行中 → 显示折叠行（session 名 + 实时状态），用户点击进入

## Composer 附件

继续对话时可上传附件作为本轮输入内容：

- **入口**：纸夹按钮、拖拽、粘贴（统一走 same-session 附件模型）；桌面端必须在基础 Tauri 配置和 channel overlay 中关闭原生 WebView file-drop，让文件拖拽进入前端 HTML5 drop zone，拖入 composer 时稳定显示可投放状态
- **预览**：图片文件在 composer 内显示缩略图，点击可打开沉浸式大图预览；预览使用单层深色遮罩按合适尺寸展示原图，不支持缩放或拖拽，点击空白遮罩关闭
- **消息展示**：用户消息下方的图片附件显示为固定尺寸小缩略图，点击进入独立全屏原图预览，不进入附件详情弹窗；文本/代码附件继续显示为紧凑文件 chip 并走附件详情。base64/data URL 只作为内部图片数据承载，不直接作为可见文本展示。
- **传输**：新会话初始输入附件只进入 task 级 `authoring/inputs/`；发送前若附件来自粘贴、拖拽或浏览器 File 对象，前端先通过桌面命令 materialize 到 Gold Band 临时输入附件区，拿到本地路径后继续走现有 `attachmentPaths -> authoring/inputs -> provider task-inputs` 链路。输入附件作为 ACP content block 发送给 agent，不混入 agent 输出产物目录。
- **AI-DYNAMIC**：AUTO / WORKFLOW 中的 AI-DYNAMIC 内部 worker、merge、acceptance 节点必须与普通 worker 复用同一 task input attachment 数据源；动态节点不得把 `input_attachment_paths` 清空，也不得要求 agent 主动扫描 run 目录寻找图片。

## Composer 状态

运行中的状态提示必须放在 composer 内，compact 模式下也不能只展示耗时或 token。当前步骤状态应展示具体文案：发送中、处理中、思考中、工具调用中、响应中、停止中、拉起下一节点中；会话式运行页的 compact 用量栏需在计时前展示带轻量旋转图标的状态标签，例如“思考中...”“工具调用中...”或“拉起下一节点中...”。这类状态是否展示运行态视觉取决于后端 composer active/lifecycle，而不只取决于 ACP session 是否仍 active；当 ACP 已 completed 但 runtime 仍处于 `launching-next-node` 时，compact 栏仍必须展示旋转状态、当前用时、会话累计与 token 用量。旋转标识应避免 SVG stroke 在高频刷新下掉帧，优先使用 CSS 边框圆环。Round 详情等非 compact 面板继续使用 composer 内状态行，不作为消息流卡片。

## 系统通知

系统通知只用于用户可能没有看到当前会话页时的关键提醒，不替代会话内状态展示。

会触发系统通知的事件范围固定为：任务完成、权限审批请求、节点结束后请求人工判断是否成功、异常中断或错误阻塞。用户主动停止、会话内普通运行中、拉起下一节点中、普通 ACP 追问完成不触发系统通知。

通知发送前必须判断桌面注意力状态：窗口未聚焦、窗口最小化、窗口不可见，或当前前端页面不是该事件对应的 run/session 时才发送；如果用户正聚焦在 Gold Band 并查看对应 `taskId/runId/roundId/nodeId/attemptId`，则只更新页面内 composer、session tree 和工作流图，不弹 OS 通知。

ACP 权限请求也必须走统一 runtime lifecycle 事件，不允许从 live event 旁路直接弹通知；这样权限请求、人工判断、异常中断和任务完成共享同一套去重、点击跳转和前台抑制规则。

## 流式渲染性能

- ACP 会话继续保持 `raw + timeline` 双层设计：`acp.raw.jsonl` 只作为协议排障事实源，主消息流只消费后端聚合后的 timeline item。
- 活跃会话 live update 不应按 token 级别驱动完整 React 渲染；文本、thought、plan 等高频更新需要在前端或后端合并为短时间窗口内的最新 item，tool、permission、error、terminal 状态仍需即时反馈。
- 后端 `acp.timeline.jsonl` 对 streaming timeline item 的 patch 写入也应短窗口合并，非 streaming item、session 写入、shutdown 和 runtime drop 前必须 flush pending patch，避免长输出时把每个 chunk 都落为一条 patch。
- 后端对 completed ACP timeline/events 的读取缓存必须绑定文件签名（至少文件长度与修改时间）。会话 snapshot 进入 terminal/completed 后仍可能存在最后一批 timeline flush 或 compact 写入，缓存不得仅以路径命中，否则会把缺尾部消息的中间状态长期返回给前端。
- 系统提示、产物预览、工作流编辑等覆盖式交互打开时，ACP 主消息流应暂停非关键 streaming UI flush，仅在内存中保留同一 text/thought/plan item 与同一 `toolCallId` 非终态工具事件的最新合并帧；权限、错误、工具终态和 session 终态仍即时处理，覆盖式交互关闭后再低优先级补 flush 最新帧。
- 前端必须把 text/thought/plan 与非终态 toolCall/toolCallUpdate streaming flush 视为低优先级、可合并的后台 UI 任务，而不是固定定时器任务。覆盖式交互打开、消息列表用户滚动、wheel 等滚动输入期间都应进入同一套 interaction quiet window：取消已排队但尚未执行的 streaming flush timer，只缓存最新帧；交互安静后再 trailing flush。不得为每种交互单独散落补丁式暂停逻辑，也不得在消息容器上用 pointer/touch 起手事件拦截所有按钮点击。
- Conversation run 级 live update 必须与当前 ACP 消息热路径分层调度：当前 selected session 的普通 timeline event 只进入 ACPChatDialog 局部合并；已存在于 session tree 里的后台 session 普通 live event 不得触发 `getConversationRun` 和整页 React state 更新；只有新 session 锚点缺失、terminal snapshot、权限/暂停/等待输入等交互态才允许排队完整 run refresh。后台非终态 session snapshot 只允许做轻量运行态 patch，且不能替换当前 selected session payload。
- ACP 消息滚动容器的 `scroll` 事件不得同步读取 `scrollHeight/clientHeight/getBoundingClientRect`。滚动期间只允许记录交互和排一个 `requestAnimationFrame`，在 rAF 中合并完成贴底状态、历史分页触发和 `isAtBottom` 更新；timeline 更新后的自动贴底也必须尊重 interaction quiet window，用户正在滚动时不得抢写 `scrollTop`。
- 关闭状态的系统提示弹窗、产物弹窗和工作流 sheet 不应解析大文本或 workflow JSON；打开时再计算内容，并尽量使用 memo 化结果，避免被 live stream render 带着重复执行。
- 正在流式增长的 assistant 文本以轻量纯文本草稿形态展示，避免每个 chunk 都重新执行完整 Markdown 解析；消息稳定后再切换为 Markdown 渲染。
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

runtime 已 terminal/completed 且不可继续时，底层 ACP snapshot 中残留的 `running / sending / responding` 只能作为 stale 事实处理，不能让 leaf 或 composer 继续保持 active。反过来，当前 ACP session 已自然 `completed` 但 runtime 仍处于 active 时，后端必须用 `runtime.phase=launching-next-node` 与 `composer.processingKind=launching-next-node` 表达“拉起下一节点中”，前端不得自行 suppress runtime active 或把 composer 清空。只有后端 lifecycle/ACP facet 明确处于 stopping，或本地 stop 命令尚未返回时，才可以继续优先锁定 composer，但同一 attempt 已收到 `completed / cancelled / failed / killed / error` 等 ACP terminal snapshot 后，必须立即结束 ACP active/stopping 与本地 stopping 锁定。会话式运行页收到当前选中 session 的完整 session snapshot 时，必须先在 App 层更新 `ConversationRunVm.selectedSession`，再刷新 run tree/lifecycle；若 run refresh 返回的 `selectedSession` payload 临时为空，前端必须保留同 key 的现有 session payload；同 key 的完整 session snapshot 则作为 payload 权威更新替换旧值；selected session identity 变化时不得沿用旧 payload；会话组件也不得仅因本地已有 timeline events 就把缺失 payload 重建为 `running`，只有 runtime lifecycle 明确 active 时才允许创建临时 running shell 承载早期流式事件。

composer 只消费后端 lifecycle/composer + ACP session live status + 少量本地 optimistic 状态；placeholder、输入禁用、停止按钮、状态文案和发送目标都来自同一个 semantic composer state。

### 互斥状态
1. **正常输入**：当前 session 已正常结束时，用户可继续输入消息（含附件），发送目标为 ACP same-session prompt
2. **运行中锁定**：当前 lifecycle 表示 runtime active 时不允许输入消息
3. **停止中锁定**：本地 stop 命令未返回、ACP session 为 `cancelling/cancel_requested`、或 lifecycle 的 ACP facet 为 `stopping` 时，composer 显示“正在停止当前会话…”并锁定输入；但同一 session 的 ACP terminal snapshot 已到达时，本地 stop/cancelling 与 stale `acp.stopping` 必须让位
4. **运行错误提示/操作**：当前 session 派生为 `runtimeDisplay.blockingError=true` 且后端 composer 给出 `runtime-error` 时，不允许输入，显示错误原因；测试/验收节点正常完成后的 `failure / invalid` 只表示 workflow outcome，不触发 runtime-error 锁定态。`error-blocked` 若 run 仍 resumable，则后端把它归入 `interrupted-input + runtime-continue`，用户可通过输入补充内容恢复 runtime；真正的 killed/session failed 仍使用终止或失败文案。
5. **工作流无效修复按钮**：只有 submit target 为 runtime continue 且 workflow 无效时才不允许输入并显示修改按钮；当前 session 已正常结束后的 ACP same-session 追问不受 workflow invalid 阻塞
6. **继续按钮**：当前 session 因 `waiting-for-user-input` 暂停且可继续时不允许输入，显示继续按钮；点击后仍走 runtime `continue`，只是继续文案保持默认
7. **停止后用户介入**：当前 session 因用户停止而派生为 `process-interrupted` 且可继续时，不显示继续按钮，恢复输入框；用户发送的文本仍走同一条 runtime `continue` 链路，只是把默认“继续”替换成用户发送内容，因此用户感知上是在会话中发出一条消息

### 修复入口

- 会话运行时的“修复”按钮与旧任务工作流页的 repair drawer 心智一致：打开当前任务工作流编辑 Sheet，让用户修复 workflow 配置。
- 修复 Sheet 标题使用“修复工作流”，而不是普通“编辑工作流”；Header 中展示无效状态、查看错误原因入口和错误原因摘要，帮助用户理解为什么需要修复。
- 在会话页保存修复后的 workflow 后，必须重新拉取当前 conversation run VM，使 workflow 有效性、session tree、工作流图与 composer 状态立即刷新。
- 修复入口不直接调用 `continueRun`；用户完成修复后再按运行态规则继续。

### 继续输入
- 当前 session 正常结束后，在会话窗口追问属于 ACP same-session prompt，不要求 authoring workflow 合法
- 追问发送时，当前会话对应行进入旋转运行态；结束后只影响该 ACP session 的消息流，不触发工作流 runtime 继续执行
- 当前 run 暂停后通过 runtime 继续仍然要求 workflow 合法；如果 workflow 无效，composer 只显示修改按钮
- 当前 run 因 `process-interrupted` 或 `error-blocked` 暂停且可继续时，composer 允许输入用户补充内容并触发 workflow runtime continue；这与当前 session 已正常结束后的 ACP same-session 追问不同，不能退化为普通 ACP prompt。旧 ACP snapshot/session 的 `cancelled` 只代表上一段响应的历史终态，不能取消本次继续、阻断 agent 拉起，或驱动 composer 的“会话已终止”错误态。AI-DYNAMIC 内部 leaf 继续必须由后端根据 locator 生成精确 leaf override：继续前先把同一 dynamic graph 中 `running/ready + outcome=null + ACP cancelled` 的 stale sibling 收敛为 paused 并移出 `currentNodeIds`，再只恢复本次目标 leaf 的同一 ACP session；没有明确 leaf 目标的父 run continue 不能批量恢复普通 paused worker，只能恢复代表 child run 的 workflow-invocation leaf。
- 会话态与旧 Round 详情中的工作流 attempt 文本发送、暂停按钮继续和继续发送都必须调用 `submit_conversation_prompt`。前端不得再按普通节点 / AI-DYNAMIC 内部节点、ACP prompt / runtime continue 自行分叉；后端根据 lifecycle/composer 与 `AttemptLocator` 决定走 `acp-prompt`、顶层 `runtime-continue` 或 AI-DYNAMIC inner exact resume。`send_acp_prompt` 只保留给不参与 workflow runtime 生命周期的 raw ACP 会话；如果命中 paused/resumable/current workflow attempt，后端必须拒绝并要求使用 `submit_conversation_prompt`。
- 停止按钮只调用桌面 `stop_active_session` 统一语义入口，不在前端按“ACP / runtime”维护两套停止链路。用户语义始终是“停止当前进行中的 leaf/session”；后端根据当前 run 与选中 session `AttemptLocator` 做分层收敛：普通单节点 attempt 停止会把当前 runtime attempt 写入 `Paused + ProcessInterrupted`；AI-DYNAMIC 内部 leaf 停止只暂停目标 dynamic node 与目标 ACP session，兄弟 leaf 仍为 `Ready | Running` 时父 graph/run 继续运行；当没有任何 active leaf，且剩余未完成 leaf 都是用户暂停的可继续节点时，父 dynamic graph、外层 AI-DYNAMIC attempt 与 run 自动收敛为 `Paused + ProcessInterrupted`，不能显示为错误阻塞。活跃 ACP runtime 发送一次 `session/cancel` notification 后继续 drain 当前 `session/prompt`，直到 adapter 返回 cancelled/interrupted 或 cancel deadline 到期；停止不是 terminal kill，也不把 adapter kill 当作 cancel 成功兜底。
- `stop_active_session` 返回成功代表后端已完成本次停止请求的业务落盘收敛：目标 leaf attempt 已进入可继续暂停态，目标 ACP session/snapshot 已写为 `cancelled`；父 run 是否 paused 由 graph 聚合状态决定。`session/cancel` 是无 response 的 notification，因此前端不能把命令返回理解成 provider 已确认取消；命令 pending 期间显示“正在停止”遮罩，返回后按后端 lifecycle 恢复目标 leaf 的可继续态，后续 ACP terminal snapshot 继续刷新消息流。
- 停止过程中可能同时出现 `run paused/process-interrupted` 与 ACP channel 仍在 drain 的事实；composer 展示优先级必须以停止流程为准：本地 stop 命令未返回、lifecycle 的 ACP facet 为 `stopping`、或 session metadata 为 `cancelling/cancel-requested` 时显示“正在停止当前会话…”并保持输入锁定。`provider.pid` 只作为 adapter process metadata，不参与停止完成、active/stopping 或 composer 状态推导。
- ACP adapter 生命周期按 `provider_id + workspace_root` 复用长连接；这里的 `workspace_root` 是用户打开的逻辑项目根目录，同一 workspace 下同一 provider 的多个 ACP session 共享一个 adapter process，不同 workspace 的 connection 可以在新 UI 中并存。AI-DYNAMIC worktree 只是 session 执行目录，不作为新的 adapter workspace key；adapter process 仍归属原始逻辑 workspace，`session/new.cwd` 才指向具体 worktree。后端 connection manager 按 JSON-RPC request id 与 `sessionId` 路由 response、timeline update 和 permission request。用户不感知 adapter pool，也不在前端暴露 cancel/close/delete 协议概念。
- 普通 Stop 只中断当前 prompt；停止后 Gold Band 持久化保留原 ACP `sessionId`，runtime continue 必须继续用原 `sessionId` 恢复同一业务会话。session release、terminal run kill、关闭应用以及 agent/MCP 配置保存导致的 restart boundary 使用 bounded `session/close` 释放 live sessions；关闭应用时先把所有 running run 递归收敛为 `Paused + ProcessInterrupted`，再对 manager 中所有 live provider/workspace connections 发起 bounded close，不能只按当前 workspace 过滤。普通 workspace 切换只是切换当前工作区视图，不关闭旧 workspace connection。新 UI 侧边栏删除 workspace 属于显式 remove boundary，移除前必须 bounded close 该 workspace 的 ACP connections，close 失败则保留 workspace 并展示错误。配置保存遇到 active prompt 时直接阻断并提示用户先停止会话，停止后再保存才关闭 idle connection 并使用新配置。adapter crash、stdout 断开或 transport closed 按可恢复中断处理，active runtime 收敛为 `Paused + ProcessInterrupted`；close 失败必须作为明确错误处理并记录诊断，不能静默吞掉，也不能把 kill adapter 伪装成成功。启动 crash recovery 没有 live connection 时只依据持久化 runtime lifecycle 收敛状态，`provider.pid` 仅作为 orphan cleanup 线索。
- composer semantic state 的优先级固定为：permission blocked → stopping → submitting → runtime active lock（含 `launching-next-node`）→ invalid workflow（仅 runtime continue 路径）→ runtime error → `process-interrupted/error-blocked` 输入继续 → `waiting-for-user-input` 按钮继续 → normal ACP prompt。后续新增状态必须先进入该派生表和矩阵测试，不能在组件里局部追加布尔判断。
- 排查停止状态不得恢复持续性 ACP composer console 日志；如需再次定位停止链路，应优先补充状态矩阵测试或临时一次性断点式诊断，完成排查后必须移除。

### 停止
- 停止并重跑在顶部操作区
- composer 内也有 stop 按钮（ACP 会话停止）
- composer 内的 ACP 停止表示“中断当前响应”，不是 workflow 配置错误；停止后的 attempt 应显示为可继续暂停
- 会话内停止使用 `stop_active_session` 单一路径；旧 UI Run 停止与新 UI 侧边栏 run 右键“停止”使用 `pause_run`。新 UI 侧边栏停止菜单只挂在具体 run 行，不挂在任务/需求标题行；菜单打开和菜单内容二次右键都必须阻止 WebView 原生右键菜单。二者共享普通中断语义但作用域不同：`stop_active_session` 只停止当前 leaf/session，AI-DYNAMIC fan-out 中不会拖停兄弟 leaf；`pause_run` 停止整个 run，会把该 run 下所有 active leaf 一起写成 `paused + process-interrupted` 并分别发送 `session/cancel`。若运行线程控制句柄不可用，则通过 live ACP connection registry 对目标 attempt 的真实 ACP session 发 best-effort `session/cancel`。活跃 ACP runtime 不因 cancel notification 已发出就立刻退出，而是继续 drain 当前 `session/prompt`；cancel timeout 必须暴露为明确错误，不能 kill adapter 伪装成功。停止不是 kill run，不能把 run/round/node/dynamic node 写成 `killed`。
- 停止期间会话窗口显示全局“正在停止”遮罩，停止正常交互与流式观感；后端只合并已经进入 ACP runtime channel 的事件，不再等待额外文件信号。命令返回后前端按后端 lifecycle 和最终 snapshot 对齐已确认消息。侧边栏 run 级“停止”点击后也必须立即关闭菜单并展示页面级“正在停止当前运行”遮罩；遮罩不只跟随 `pause_run` 命令返回，而是等当前 run VM 刷新确认 run 非 running、active sessions 清空且选中 ACP session 已 terminal 后再消失，避免用户误以为操作没有生效。
- 关闭客户端和启动时崩溃恢复与用户停止共享同一 interruption 语义：所有仍为 running 的 run、当前 node 和 AI-DYNAMIC descendants 都收敛为 `paused + process-interrupted`。`provider.pid` 不参与业务状态判断，只能作为 adapter process metadata 用于诊断和 orphan cleanup。
- 停止请求一旦落盘，迟到的 ACP success response 不能写 success artifact，也不能驱动 workflow 跳到下一节点；runtime 必须在 provider 返回后重新确认当前 attempt 仍是 running/current，确认已暂停则直接停止推进。
- runtime 异常、agent/provider 异常与 workflow DSL 无效必须分开提示：只有 `workflowValid=false` 或明确的 workflow validation error 才展示“修改/修复工作流”入口；`error-blocked`、session failure、session killed 等运行期异常只提示查看错误原因，不默认引导用户修改工作流。
- 当前选中 session 已有 `diagnostics.lastError` 时，错误面板文案应直接拼接具体错误原因，避免用户再额外寻找日志入口。
- 新 UI 中，`process-interrupted` 不再展示单独“继续”按钮，而是恢复输入框；用户点击发送后仍走 runtime `continue`，只是把默认“继续”替换成用户本次输入内容，因此用户感知上是继续在当前会话里发消息

## 会话信息栏（ACPSessionHeader）

- 单行布局：模型名 + 权限模式 Badge + sessionId + 操作按钮
- 会话信息栏与运行标题栏保持同一套紧凑节奏：缩小上下 padding、降低主标题字号、压低按钮高度，减少双层头部对内容区的挤压
- 第二行作为元信息层，视觉权重需低于第一行：更小字号、更轻字重、更弱对比度，不与任务标题竞争主次
- 用户消息气泡避免使用高饱和整块主色填充；在深色主题下优先使用主色混入 card/background 的柔和底色，保证信息突出但不刺眼
- 产物来源固定为当前选中 session（含 AI-DYNAMIC 内部节点）的 artifacts / attachments，不使用 run 级聚合占位数据
- 产物弹窗遮罩使用轻量弱化遮罩（低透明深色 + blur），主体面板保持半透明而不过度强调，不做厚重黑色卡片
- sessionId 与模型名、权限模式同行，不再单独占行

## 产物/附件信息区

- 位于 composer 下方
- 三区展示：输入附件 / 产物 / 附件（输出）
- 整体采用紧凑单行 chip 区，优先压缩上下留白与按钮高度，避免资源条挤占对话输入区和消息区高度
- composer 底部状态栏与资源条之间不额外保留大块过渡留白，输入区、模型权限信息与资源条保持连续的紧凑垂直节奏
- 资源条不单独增加顶部边线，直接承接 composer 自身底边，避免连续双分隔线把输入区与文件区切得过碎
- 资源条首行内容尽量贴近 composer 底边，优先压缩资源条自身顶部内边距，而不是继续压缩文件 chip 点击热区
- 输入附件来源于 task 级 `authoring/inputs/`，创建会话时设定，重跑自动复用
- 输入附件使用 Upload 图标 + 蓝色标记，与输出产物/附件区分
- 当前选中 session 的产物 / 输出附件统一通过底部文件项进入弹窗查看，点击文件项直接打开该文件详情，不再经过单独列表页，也不再保留顶部重复入口
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
- 后续同一 ACP session 的每次追问都必须优先复用当前会话快照中的 `currentModelId / currentModeId`；如果用户中途切换了模型或权限模式，下一次 `session/prompt` 必须继续带上最新选择，而不是回退到节点初始配置。
- 模型选中态只在触发器展示模型名称，长描述只在下拉项中换行展示，不允许撑破触发器或越出窗口边界
- 配置栏解析逻辑统一收敛在前端 ACP session config 工具中：优先读取 provider 返回的 `models.availableModels / modes.availableModes`，缺失时回退 `configOptions[category=model|mode].options`。展示组件只消费归一化后的 id/name/description，不在 JSX 内重复解析协议 payload。

## 工具调用参数展示

- 工具调用卡片展开后以有序列表展示工具输入参数
- 参数按来源优先级提取：rawInput > 结构化 fields > title/locations 解析
- 同标签参数保留多个不同值（如多个路径、多个查询条件）
- 语义化参数缺失时回退展示原始输入 JSON
