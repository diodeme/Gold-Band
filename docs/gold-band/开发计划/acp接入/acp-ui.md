# ACP Dialog / Chat UI 计划

## 0. 当前实现状态

- `RoundDetailPage` 的节点 session tab 已切换为 `ACPChatDialog`。
- 前端新增 ACP session / event / permission / diagnostics 类型，数据来自 Tauri `AcpSessionVm`。
- `ACPChatDialog` 展示压缩 session header、消息流、thought、tool call、plan、permission、raw frames 和 composer。
- 会话 UI 已采用 prompt-kit copy-in 组件承载基础交互：`ChatContainer` 负责消息滚动，`Message` 负责用户/agent 气泡，`PromptInput` 负责 composer，`Tool` 负责工具调用卡片，`ChainOfThought` 负责 thought 折叠展示；ACP 专属逻辑只负责事件映射、权限和诊断。
- 系统提示弹窗正文、原始帧摘要展开详情、子 Agent 结果等长文本区统一跟随应用设置字体；仅在明确需要展示代码或固定宽度标识时才允许局部使用等宽字体。
- ACP 会话流支持将 `Agent` 工具调用生命周期内的子 Agent transcript 聚合为可展开/收起分组，不再把主 Agent 与子 Agent 输出完全混排。
- ACP session 初始化与后续追问必须分别维护 Gold Band 显式模型覆盖和 Agent 当前模型：模型只继承 `modelOverride`，权限模式继续继承 `currentModeId`。Gold Band 发起会话的“不指定”写为 `modelOverride = null`，不得从 Agent 返回的 `currentModelId` 反推 override；用户在会话窗口选择任意 Agent 模型后写入具体 override，下一次 `session/prompt` 继续使用该值。
- 会话运行区的产物/附件入口已改为与任务列表一致的 `Collapsible` 折叠面板：默认收起展示非零产物/附件计数，展开后点击文件项继续复用现有详情弹窗；当任务列表存在时，产物/附件面板固定在任务列表上方。
- 节点详情抽屉中的 artifact / attachment 内容以二级详情层打开，返回或关闭产物详情时恢复原节点详情抽屉。
- 节点详情抽屉顶部只保留紧凑“查看详情 / 查看会话”切换，不重复展示长节点说明。
- legacy `progress.events` / `raw.stream` 不再作为节点会话主视图，仅保留系统日志/诊断入口。

## 1. 核心方向

Gold Band 后续 ACP 输入输出不再以 terminal/log 面板或自研 `progress.events.jsonl` timeline 呈现，而是以对话框 / Chat UI 呈现。

```text
ACP SessionUpdate / ToolCall / Plan / Permission / Error
  -> Gold Band 会话详情 ViewModel
  -> ACP Dialog / Chat UI
  -> Round 节点详情 / 会话抽屉
```

UI 目标是让用户在 Gold Band 内直接用“对话”的方式理解和继续 agent 会话：用户通过 chat composer 输入，agent 输出以消息气泡、结构化卡片和状态块展示；工具调用、权限请求、计划变更、模式变更不混入普通文本日志。

## 2. 借鉴 Jockey 的 UI 思路

Jockey 的可借鉴点：

- 文本 delta 进入正在流式生成的消息。
- thought/reasoning 单独存储，可折叠展示。
- tool call 以卡片形式展示，并支持 update 原地刷新。
- terminal metadata 聚合到对应 tool call。
- plan entries 作为独立结构化块展示。
- permission request 进入会话流，等待用户决策。
- stream event 带 seq，前端可发现丢帧或乱序。
- connection lost / prewarm / runtime state 作为会话级事件提示。
- ACP 原始事件先归一化为 UI event model，再由前端组件渲染。

参考目录：

```text
.external/jockey/src/lib/acpEventBridge.ts
.external/jockey/src/lib/acpEventBus.ts
.external/jockey/src/hooks/useAcpEventListeners.ts
.external/jockey/src-tauri/src/acp/client.rs
.external/jockey/src-tauri/src/acp/worker/types.rs
```

参考文档：

```text
docs/gold-band/开发计划/acp接入/jockey-claude-agent-sdk-bridge.md
```

Gold Band 需要吸收的是 Jockey 的 ACP 事件归一化和 Chat/Session UI 思路，而不是恢复 Claude Code legacy CLI 的 terminal 心智。

## 3. ACP UI event model

前端不直接散落解析 ACP 原始 JSON。ACP client / ViewModel 应先把 ACP session events 归一化为 UI 可消费的事件模型：

- `TextDelta`：agent 文本增量。
- `ThoughtDelta`：reasoning / thought 增量。
- `ToolCall`：工具调用创建。
- `ToolCallUpdate`：工具调用状态、输出、metadata 更新。
- `Plan`：计划块与步骤状态。
- `PermissionRequest`：权限请求与可选操作。
- `ModeUpdate`：agent mode 变化。
- `ConfigUpdate`：模型、权限、工具或运行配置变化。
- `SessionInfo`：session id、adapter、cwd、capabilities、恢复状态。
- `AvailableCommands`：可用命令或快捷动作，进入 session 状态，不进入主消息流。
- `UsageUpdate`：上下文窗口、已用 token、费用等用量状态，进入 session 状态或 Raw frames，不进入主消息流。
- `SessionError`：ACP error、adapter crash、auth required、timeout。

归一化边界：

- UI 组件只依赖 Gold Band 会话详情 ViewModel，不直接绑定 ACP crate / adapter 原始结构。
- Raw ACP frame 只在诊断入口展示，不作为普通用户主视图。
- 普通 ACP session 查询必须返回事件窗口而不是完整会话文件；V2 优先读取 `acp.timeline.jsonl + acp.snapshot.json`，初始默认返回最近约 30 条聚合 timeline item，向上加载历史时单次加载条数由项目级 `configs/app-config.json` 的 `acpChatEventPageSize` 控制；前端额外保留有限多页缓冲保证滚动连续；分页主游标改为 `beforeCursor / afterCursor`，兼容期继续接受 `beforeSeq / afterSeq`。首个 session-ready 快照必须把 snapshot metadata 与首个 `goldBandPrompt` timeline item 一起暴露，不能让首屏只看到 agent thinking。
- `available_commands_update`、`usage_update`、session/mode/config update 等状态帧不渲染为聊天消息；它们只更新 session 状态或留在 Raw frames 中排障。
- ACP runtime 文件位于 `~/.gold-band/projects/{project-id}/tasks/...`，不写入项目工作树；ACP 会话身份只以当前 user runtime attempt 的 `worker-ref.json` 为事实源：`continue_ref.acpSessionId` 决定 `session/load` 和 UI header 的 provider session id；`acp.session.json` 不再作为 session id 来源，但会保存 status、capabilities、adapter 配置快照、stop reason，以及通过可选 `session/list` 轮询 best-effort 拉取得到的 `title` 缓存。该能力受项目级 `configs/app-config.json` 控制，默认关闭。title 仅用于后续 UI/检索储备；本期不作为会话头部展示的依赖字段，拉取不到时保持为空。
- `configs/app-config.json` 是版本内共享的项目级 app config 入口，不是用户本机偏好设置：适合开发期可选能力和共享 UI/runtime 参数的统一管理。CLI 与桌面端都读取同一份文件；未声明字段继续走代码默认值，不要求每个配置都显式写入。当前文件示例：`{ "acpSessionTitleRefreshEnabled": false, "acpChatEventPageSize": 360 }`。
- session-wide metadata、pending permission、usage 和 diagnostics 由后端流式扫描全量事件得出，不允许为了 UI 轮询保留或传输全量事件数组。
- `Agent` 工具调用的子 Agent 分组是前端 timeline projection：前端根据 `Agent` tool call 的 start seq 与 terminal update seq 计算生命周期窗口，将窗口内子事件框定展示，不新增后端 ACP UI event kind。
- 未识别事件应进入诊断区或系统提示，不应破坏会话流。
- 初始 `get_acp_session` 返回 `null`（attempt 目录尚未写入 ACP 文件）时前端不立即显示 "ACP 会话失败"，而是以 120ms / 300ms / 700ms / 1200ms 递增间隔短暂重试，避免与后端首次 snapshot 写入形成竞争；所有重试耗尽仍为空时才降级为失败态。
- ACP adapter 关闭时序已收口为 `open → draining → closed`：关闭连接前先将其移出复用池并拒绝新 prompt，再取消待处理 permission/elicitation、发送 session cancel，并按 ACP session id 有界等待活跃 prompt worker 收敛；排空后执行 `session/close`，最后关闭 transport。draining 期间保留在途 response 的写权限，避免 AskUserQuestion/permission worker 被唤醒后向已关闭 stdin 写入。若排空超时，后续 transport 错误统一映射为 `interrupted`，不写入 `ACP prompt failed` diagnostics。
- 关闭恢复的单元验收固定覆盖：draining 拒绝新普通请求但允许 shutdown request；AskUserQuestion 等待中关闭会先生成 decline 并完成 prompt drain；worker 不收敛时等待必须按 timeout 返回；draining/closed 结构化错误必须被 prompt runtime 识别为 transport interruption。

## 4. Gold Band 会话信息架构

节点详情中的 ACP Dialog / Chat UI 建议分为：

1. **Session Header**：provider、adapter、session id、cwd、连接状态、恢复状态。
   - Agent 名称与 session id 作为同一身份文本组按 baseline 对齐，并统一使用紧凑行高；图标与右侧操作继续按控件中心对齐，不增加针对单一字体的位移补丁。
   - Direct 标题与 Agent 身份使用独立组间距；长 session id 统一格式化为前 8 位与后 4 位的紧凑文本，Tooltip 和剪贴板仍消费完整值，避免技术标识挤压主标题。
   - session id 复制反馈与 Tooltip 由同一状态机管理；复制反馈结束或桌面窗口失焦后进入重开锁定，忽略切回应用时 WebView 残留 focus/pointer 产生的 `open=true`，直到鼠标移出或焦点在应用内移走后才解锁。回归测试必须覆盖“复制 → 反馈关闭”和“复制 → 切换应用 → 返回”两条路径。
2. **Message List**：用户消息、agent 文本消息、系统提示。
3. **Reasoning / Thought**：思考内容，默认折叠或弱化。
4. **Tool Calls**：工具调用卡片，作为会话流中的结构化消息块。
5. **Agent / Sub-agent Group**：`Agent` 工具调用触发的子 Agent transcript 分组，默认完成后收起、运行中展开。
6. **Plan**：agent 计划与状态，作为独立 plan block。
7. **Permission**：权限请求与用户响应，用于 ACP `session/request_permission`。
8. **Composer**：用户输入区，用于继续会话、回答 agent 自由文本问题、提交下一次 `session/prompt`；输入区下方展示 adapter 当前生效的模型与权限模式。只要 `models/modes/configOptions` 中存在可选项就展示配置栏，当前值尚未归一化时显示选择占位，不隐藏整条配置。
9. **Terminal / File Details**：命令、cwd、输出、退出码、文件读写路径，作为 tool call 的详情，不作为主输出形态。
10. **Errors**：ACP error、adapter crash、auth required、timeout。
11. **Raw / Diagnostics**：原始 ACP frame / transcript 查看，仅用于排障。

## 5. 推荐组件拆分

基础 AI chat 交互优先使用 prompt-kit 生成到项目内的源码组件，避免自研消息容器、输入框和工具调用基础控件：

- 普通 `overflow-y-auto` message list：承载 ACP 历史浏览和向上分页；对 prepend 历史使用 scrollHeight 差值补偿 scrollTop，避免虚拟列表重新测量高度时闪回；对流式消息内容增高使用内容尺寸监听来维持底部贴合，避免只在事件数量变化时滚动。
- `ChatContainerRoot / ChatContainerContent / ChatContainerScrollAnchor`：仅用于不需要历史分页的普通聊天容器场景。
- `Message / MessageContent`：承载用户与 agent 气泡。
- `PromptInput / PromptInputTextarea / PromptInputActions / PromptInputAction`：承载 composer、快捷键、loading 和 action 区域。
- `Tool`：承载工具调用卡片的折叠、状态、输入输出展示。
- `ChainOfThought / ChainOfThoughtStep / ChainOfThoughtTrigger / ChainOfThoughtContent`：承载 thought / reasoning 折叠展示。

ACP 专属组件只做协议事件映射和业务状态组合：

- `ACPChatDialog`：承载会话对话框或会话抽屉。
- `ACPSessionHeader`：展示 session/provider/adapter/cwd/连接状态。
- `ACPMessageList`：按时间顺序展示消息和结构化事件块。
- `ACPEventRenderer`：根据归一化事件类型选择渲染组件。
- `ToolCallCard`：把 ACP `ToolCall` / `ToolCallUpdate` 映射为 prompt-kit `Tool` props。
- `ChildAgentGroupCard`：把 `Agent` 工具调用与其生命周期内的子 Agent transcript 聚合为可展开/收起分组。
- `ThoughtBlock`：把合并后的 `ThoughtDelta` 映射为 prompt-kit `ChainOfThought`，标题展示思考耗时而非字符数。
- `PlanBlock`：展示计划条目和状态变化。
- `PermissionRequestDialog`：展示权限请求、选项和用户决策。
- `SessionStatusBar`：展示连接、恢复、错误和队列状态。
- `RawFrameViewer`：按 event kind 查看和复制 ACP raw frame。

## 6. UI 展示规则

### 6.1 用户输入

- 用户通过 prompt-kit `PromptInput` 输入 prompt 或回答 agent 的自由文本问题。
- 发送后立即清空 composer 并乐观生成右侧用户消息，同时调用下一次 ACP `session/prompt`。
- 每次 Gold Band 用户输入（包括 round 顶部“继续运行”触发的本地化 `继续/Continue`）都要生成新的 prompt identity，并同时写入乐观用户气泡与后端 synthetic `goldBandPrompt` 事件元数据；同文本历史 prompt 不得复用同一 identity。
- ACP client 在发送 `session/prompt` 前持久化 synthetic `userTextDelta`，确保初始 prompt 和后续继续输入都作为右侧用户消息出现在会话流中；稳定 system prompt 只在不支持 system prompt 的 provider 降级场景中作为 Gold Band hidden 段进入 user prompt。
- synthetic `goldBandPrompt` 是 Gold Band 本地输入的唯一事实源，但 ACP session 是可由外部 Agent 客户端继续的共享会话。`session/load` replay 中的 `user_message_chunk` 必须与既有 Gold Band prompt 有序锚点对账：Provider 漏回放部分本地 turn 时允许跳过缺失锚点并继续匹配后续本地 prompt；本地 prompt 回显不写 timeline，完全匹配不到剩余锚点的外部 user turn 及其 assistant/thought/tool/plan 更新先按完整 turn 暂存，遇到下一个本地 prompt 后整组写入并标记 `beforePromptId`，尾部历史在 load finish 时只标记 `afterPromptId`。Provider 提供 message/tool identity 时直接复用；缺失 ID 的 replay 使用 `session + afterPromptId + gapTurnIndex + kind/itemIndex` 生成稳定身份。必须回归覆盖“本地 `hi, hi, ask` / replay `hi, external, ask`”，确保 `external` 位于第二个 `hi` 之后、`ask` 之前，而 `ask` 及其工具调用仍按本地回放抑制。
- Claude ACP 暂未为 interruption control 提供结构化字段。归一化层临时只精确隐藏 `[Request interrupted by user]` 与 `[Request interrupted by user for tool use]` 两个完整值，不做模糊文案匹配，并保留 raw frame；上游支持 typed interruption 后删除该文本适配。
- prompt 内存活动状态拆为 `Starting / Accepted / Running / CancelRequested`。持久化 synthetic prompt 后进入 `Accepted` 并立即发 live event；实际发起 `session/prompt` 前再进入 `Running`。前端匹配到 durable `promptId` 即结束“发送中”并进入“处理中”，不以 provider echo 作为接受边界。
- Task-level input attachments 是新 ACP session 的初始化上下文，只随 `SessionMode::New` 首次发送；同一个 session 内 resume/continue 不自动重发这些 task inputs，也不在后续用户气泡下重复展示。用户在 composer 中本轮显式选择的附件仍作为本轮 same-session prompt attachments 单独发送和展示。
- Gold Band 生成的 `<hidden data-gold-band-hidden="true">` 段在用户气泡内默认折叠，hidden 段和可见 requirement/goal 保持在同一个 user bubble 中，不拆成独立消息；展开后展示原文，再次点击收起。用户消息内容容器使用 intrinsic sizing，隐藏标题始终 `w-full` 铺满当前气泡；折叠时对整个隐藏区启用 inline-size containment，使其不参与父气泡宽度计算，气泡宽度只由可见正文决定；展开时解除 containment，隐藏正文使用 `w-max + max-w-full` 扩展整个气泡并继续受消息最大宽度约束。hidden 后面的可见片段只在展示层去掉开头换行，避免折叠块和正文间距被 prompt 模板空行放大；真实 prompt 内容和事件记录不变。普通用户手写的 `<hidden>` 文本不折叠。
- 用户气泡由 prompt-kit `MessageContent` 的 user 变体统一呈现，并消费主题级 `message-user` / `message-user-foreground` token。浅色主题使用接近参考图的轻浅灰背景、深色正文、无可感知边框和无投影；hidden 折叠区只在同一色调内轻微压暗，禁止恢复大面积 `primary` 混色或灰块套白块。
- assistant 文本消息由同一 prompt-kit `MessageContent` 的 assistant 变体呈现，但采用透明背景、实色正文、无边框和无投影；会话标题栏消费独立 `content-header`，四套主题当前都将它映射为 `var(--sidebar)`，并通过轻量底边界与消息区分层。标题栏不增加卡片或投影，主阅读区只允许用户气泡和结构化工具/思考对象使用必要的灰色 surface，避免灰块堆叠成灰雾。
- 用户在已停止 / 已完成 ACP session 中手动追问时属于普通 user message，后端直接发送用户原文，不注入 Gold Band hidden runtime context，也不包装成 `# Goal`。
- 当前 run 因 `process-interrupted` 暂停且用户在 composer 中输入补充内容时，提交仍走 runtime `continue` 主链路而不是普通 ACP prompt；但 `run_continue()` 必须把这类“停止后用户继续”判定为 `UserMessage` 渲染语义，只把用户原文发送给 provider。只有 workflow 自身恢复执行、没有用户显式追问时，才判定为 `WorkflowResume` 并发送 hidden runtime context + `# Goal`。
- 当 node 处于 `waiting_for_user_input`、permission pending、adapter disconnected 等状态时，composer 应显示明确状态。
- 当 ACP session 处于 pending/running/cancelling 等 active 状态时，composer 展示 Stop action，普通 Send 禁用；Stop 请求只取消当前 ACP adapter prompt。AI-DYNAMIC 内部 leaf 的运行事实以 dynamic leaf workflow 状态为准，ACP `cancelled` 只代表聊天会话传输事实，不得让前端或命令层把 live running sibling 推成 paused。
- 若 Stop 发生在本地 optimistic prompt 已创建、但后端真实 `userTextDelta` 尚未写入之前，前端必须立即移除该 optimistic prompt 并释放其发送锁；未被后端接受的取消 prompt 不得永久停留在“发送中”。停止命令成功后必须先读取并合并最终 session，再只清理 `optimistic=true && status=sending` 的消息；已由 durable `goldBandPrompt` 接受的用户消息继续保留，避免停止后当前 UI 消失、应用重启又从磁盘恢复。`runtime-continue-started` 与普通发送不同：该返回值即表示后端已接受继续命令，即使 `session` 为 `null`，后端响应也必须携带 `runtime-active / provider-running` lifecycle，前端要把本次 optimistic prompt 从 `sending` 转为 accepted/completed 并清理 `awaitingResponse`，不能继续等待 ACP echo 才解除发送锁。后台线程真正写入 run/node running 文件前，旧的 paused/interrupted-input lifecycle snapshot 不得把 composer 短暂降级为可输入；只有父级 lifecycle 追到 active、stopping 或 runtime-error 后，前端才释放本地 continue-started lifecycle override。
- Plan intervention permission 是 active-session 发送锁的唯一例外：composer 仍可输入反馈，但只在权限决策完成且当前 turn 结束后继续发送 queued prompt。
- 用户输入不走 terminal stdin，不依赖 legacy CLI 会话。

### 6.2 文本流

- 合并连续 text delta，避免一 token 一行。
- 实时轮询收到后端已归一化的 delta 快照时，按 attempt-scoped session、kind、event id 稳定身份替换同一流的旧快照，不能因为 `seq` 随最新 raw frame 前进就追加成多条消息。
- 前端只合并同一 stable delta stream；不同 event id 的相邻 text / thought delta 不做跨流拼接，避免实时轮询把消息边界压成一个气泡。
- 保留原始时间顺序。
- 与 tool call / plan block 同处一个会话流。
- 文本输出以 agent message bubble 呈现，不以 stdout/stderr 日志呈现。

### 6.3 Thought / Reasoning

- 默认折叠。
- 标识为 agent 内部过程，不作为 runtime 判定依据。
- 若 provider 不返回 thought，则隐藏该区域。
- Thought delta 与 text delta 分流，不混入最终回答正文。
- 连续 thought delta 应合并为一个思考过程块；如果中间只穿插 usage / available commands 等隐藏状态帧，仍按同一个 thought block 展示。
- Thought 标题展示从 ACP event timestamp 派生的思考耗时（如 `12 秒` / `12s`），不展示字符数。

### 6.4 Tool Call

Tool call 卡片展示：

- 工具名 / title
- status
- input 摘要
- output 摘要
- 文件位置 / locations 仅在包含具体文件、行号或 range 时展示；Glob 这类仅重复搜索根目录的 locations 默认隐藏
- terminal metadata
- raw input / raw output 展开入口

Tool call update 应按 attempt-scoped `toolCallId` 更新同一张卡片，而不是生成重复卡片。多 attempt 会话和实时轮询必须共用同一套事件归一化 helper，同时作用到 `event.id`、`toolCallId` 和子 Agent `_meta.claudeCode.parentToolUseId`；实时轮询返回的 attempt-local `seq` 需要映射为会话内 display `seq`，merge key 不得依赖会变化的 `seq`。terminal / file 细节挂载到对应 tool call，不应成为主会话输出。工具卡片使用 prompt-kit `Tool` 承载折叠和状态展示，标题行左对齐显示“操作名 + 次级参数”，例如 `Glob .claude/**/*`、`Read xxx.js`；展开后展示路径、查询等关键参数块与输出摘要；不展示 tool call id、kind、input 或 raw details。工具卡展开/收起属于阅读动作，必须保留当前滚动位置，不能触发会话容器自动滑到底部；长路径、JSON 输出和连续字符必须在工具卡宽度内换行或内层滚动，不能撑宽抽屉。

`Agent` 工具调用不按普通工具卡扁平展示子过程，而是由 `ChildAgentGroupCard` 聚合其生命周期窗口内的子 Agent transcript：普通工具仍使用 prompt-kit `Tool`；`Agent` 工具 header 显示子 Agent 类型、任务说明、状态和子事件数量；展开后内部继续复用 `ACPEventRenderer` 渲染文本、thought、tool call 和 plan；并发发起的多个 `Agent` 工具保持同层并列，不互相嵌套；子 Agent 内部工具优先按 `_meta.claudeCode.parentToolUseId` 归属到对应 Agent，只有缺少该元数据时才回退到 seq 生命周期窗口；如果当前历史窗口缺少 Agent opener，则暂时保持扁平展示，避免误把半截历史归入错误分组。

### 6.5 Permission Request

权限类提问使用 ACP `session/request_permission`：UI 展示 agent 请求、tool call 摘要和可选项，用户选择后返回 `RequestPermissionResponse`。

权限请求可以展示为：

- 阻塞式 dialog：用于必须先决策才能继续的请求。
- inline approval bar：用于嵌入会话流并保留上下文的请求，视觉上参考 prompt-kit `system-message` 的轻量提示，而不是大块表单卡片。

权限请求必须保留用户选择、时间和相关 tool call id，便于后续排障。用户点击允许或拒绝后，UI 立即乐观关闭 pending 卡片；若响应失败，再恢复卡片并提示重试。pending / selected 的确认协议键必须使用 ACP JSON-RPC `session/request_permission` 原始 request id；timeline item id 可以是 `permission-<id>` 展示键，但前端提交、后端 `AcpPermissionRequestVm.requestId`、`acp.permission-request.<id>.json` 和 `acp.permission-response.<id>.json` 必须统一回到原始 id。新旧 UI 同时打开同一 ACP session 时，只能消费同一个 canonical requestId，不允许各自用本地展示 id 写响应文件。pending / waiting 状态使用低强调 primary 语义色，不使用 warning 橙色；审批卡片固定为信息行 + 按钮行两层，宽度不强制撑满会话列，按钮较多时使用居中的两列按钮组，不得挤压标题和 pending 状态；按钮使用紧凑胶囊形态，长选项文本单行截断。普通工具权限等待用户决策时，composer 只显示紧凑等待状态，不保留大号禁用输入框；`ExitPlanMode` 这类包含“keep planning / 继续规划”选项的 plan 决策权限例外，composer 必须保持可输入，但等待决策期间不展示“处理中”计时，且该 pending 到 selected 的等待区间不计入 session 累计净处理耗时；用户输入自然语言反馈时等价于选择继续规划并排队发送该反馈，输入框 placeholder 显示“输入修改意见继续规划”。

### 6.6 Plan / Mode / Config / SessionInfo

- Plan block 展示 agent 当前计划、step title、status、nested entries。
- Mode / Config update 以轻量系统消息或 session status 展示。
- SessionInfo 展示 provider、adapter、session id、capabilities、cwd、恢复状态。
- Plan 是可视化辅助，不直接决定 Gold Band workflow edge。

### 6.7 Agent 提问 / 用户回答

自由文本澄清类提问按普通会话轮次处理：agent 在消息中提出问题并结束 turn，Gold Band 将节点标记为等待用户输入；用户在 `ACPComposer` 中输入回答后，由 `run continue` 发送下一次 ACP `session/prompt`。

```text
agent message(question)
  -> node waiting_for_user_input
  -> user answer in ACPComposer
  -> next session/prompt(answer)
```

Round 详情页顶部的“继续运行”属于 canonical workflow runtime 动作，不复用 composer 的任意用户输入。它只在当前 run / round / node 为可恢复 `paused` 时出现；`error_blocked` 在 UI 上显示为错误阻塞，但仍属于用户可显式继续的暂停态。点击后自动恢复当前 attempt 的 ACP session，并发送本地化短 prompt：中文 `继续`，英文 `Continue`。如果 `session/load` 失败，不允许 fallback 到新 ACP session。连续的用户 prompt 必须按事件边界独立成气泡展示，不能把恢复 prompt 拼接到上一条需求 prompt 末尾。

### 6.8 Raw / Diagnostics

Raw 视图用于排障：

- 展示 ACP 原始事件 / frame。
- Raw frames 是会话画布的切换视图，不追加到聊天消息流后方。
- Raw frames 按需加载，普通 `get_acp_session` 只统计 raw frame 行数，不解析完整 raw JSONL；Raw frames 详情读取也应有体积上限，避免大文件拖慢会话主 UI。
- 普通 session 返回的 UI event raw 只能保留渲染 tool、plan、permission 所需的摘要字段，超长字符串和超大 raw payload 必须截断；完整原始内容只通过 Raw frames 分页查看。
- 最新 ACP error diagnostic 或 Raw frame 中的 JSON-RPC `frame.error.message` 必须显示为会话顶部错误横幅，不再重复插入消息流；若该错误时间之后出现新的正常 agent 输出，横幅自动消失。
- ACP stop 点击后必须先同步把当前 attempt / run / round 收敛到 `paused + process_interrupted`，让 ACP 抽屉和 Round 详情立即退出 active / stopping 态；随后运行中的 ACP runtime 观察到取消标记后，发送不带 `id` 的 JSON-RPC notification `session/cancel`，不能把它当 request 等待响应；若短暂宽限后 provider 仍未结束，再清理 provider pid 并强制 kill，对应 session 最迟在 15 秒 fuse 后兜底写为 `cancelled`，避免 composer 永久显示“停止中”。
- Raw frames 按 JSONL 一行一个 frame 的形式由后端分页展示，默认加载最新页（page 0），页内按行号升序展示；摘要默认单行截断，时间统一显示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；点击该行后以 pretty JSON 或纯文本多行展开，使用克制的暗色代码面板和柔和选中态；短 frame 自然展开不显示内层滚动条，只有超长 frame 才限制高度并显示细滚动条；超长连续字符主动切分换行，内容必须在容器内显示，不能撑出窗口，且展开正文跟随应用设置字体。
- 支持服务端关键词检索，不把全量 `acp.raw.jsonl` 传给前端。
- 支持按 direction（inbound/outbound）和 kind/method 过滤。
- 支持关联到会话流中的消息、tool call 或 permission request。

Raw 视图不承担主交互，不把 ACP 原始 JSON 暴露为普通用户默认体验。切换 Raw 视图或展开单个 frame 时必须保留用户当前阅读位置；用户主动检索、筛选或翻页时只替换当前页结果；Raw 详情内容必须主动换行，禁止横向撑出会话抽屉。

新增用户 prompt、轮询获得新 ACP event 或 agent 回复追加内容且用户仍在底部时，会话列表必须贴底；同一条流式 agent 消息内容变高但事件数量不变时，也必须通过内容尺寸变化监听继续贴底；抽屉关闭不会停止后端 ACP prompt，重新打开同一节点会话时只要持久化 session status 仍是 pending/running/cancelling 等 active 状态，`ACPChatDialog` 必须立即恢复约 1.5 秒一次的 session 轮询并继续合并渲染新增事件；用户上滑加载历史期间必须冻结自动贴底并忽略虚拟列表加载后的临时 at-bottom 误报；历史加载应在用户不在底部且距离顶部约 240px 内预触发，并在顶部显示“— 上滑查看历史信息 —”提示，不要求用户贴到绝对顶部；加载成功后只保持当前阅读锚点，prepend 前后用 scrollHeight 差值补偿 scrollTop，避免滚动条长度变化导致阅读位置按比例回退；不自动下拉补较新页，避免快速上下滚动时两个方向的分页互相抢占滚动位置；处理中提示结束时只移除 composer/乐观气泡状态，不允许 session 刷新导致消息区先跳顶部再回底部。

### 6.9 处理中反馈与计时

- 会话处于 pending / running 且尚无可渲染事件时，composer 内显示“Claude 调起中”，Message List 不显示“暂无 ACP 事件”；如果 ACP session status 尚未写入但当前 runtime node 已是 pending / running / in_progress，也按同一启动状态处理，避免新 run 初始化窗口出现空事件误导。
- 用户点击发送后立即清空 composer 并乐观生成右侧用户气泡；调起 ACP 到真实 `userTextDelta` 写入会话前显示“发送中...”，该提交阶段不参与计时。乐观用户气泡按 task / run / round / node / attempt 维度保留在前端运行态中，关闭并重新打开同一会话抽屉时必须恢复显示并继续锁定 composer，直到后端写入真实用户消息或发送失败。真实用户消息写入后移除乐观气泡，并从该消息时间点进入“处理中...”到首个非用户帧返回；首帧后按最新帧类型切换为“思考中 / 工具调用中 / 回复生成中”。composer action 行与发送按钮保留足够间距，避免按钮贴近输入框。
- 同一会话中连续多次 `继续/Continue` 必须各自保留独立消息行；去重只能基于同一 prompt identity 的重复快照，不能只按文本内容去重。允许出现“历史继续 + 新继续”的两条独立气泡，但禁止把它们拼接成 `继续继续` 或把新回合错误合并进旧回合。新写入的 synthetic `goldBandPrompt` 必须携带 `raw.promptId`；历史数据缺失 promptId 时，前端渲染层只能按事件身份兜底保留多条 Gold Band prompt，不得按 `attemptId + text` 折叠真实多轮继续。
- Composer 只保留两类计时：当前步骤/操作计时，以及 session 累计耗时。当前步骤计时从真实用户消息写入后的首个处理中阶段开始，并随“思考中 / 工具调用中 / 回复生成中”等状态切换；会话进入 completed / failed / cancelled 或等待用户权限决策时停止当前步骤计时。session 累计耗时不按墙钟跨度计算，而是由后端按同一 ACP 会话内每个用户 prompt turn 的实际运行时段累加得到的净处理耗时：每轮从真实用户消息写入开始，到该轮最后一个响应/思考/工具/计划事件结束为止，并扣除 `session/request_permission` 的 `permissionRequest(pending)` 到用户选择的等待区间；该扣除覆盖普通工具授权以及 `ExitPlanMode` / keep planning 等 plan 决策。继续会话时在历史累计值上继续增加，不把两轮之间的用户空闲时间计入总时长。
- 继续 ACP session 时，runtime 的 replay phase 先按 user turn 判断本地回显与外部新增历史：本地 turn 不重复写入，外部 turn 先暂存；`session/load` response 后 drain inbound queue 并 finish 暂存历史，prompt 前仍处于 replay 时再次 finish 兜底，随后进入 `AwaitingTurnStart`。Provider history 在 `raw.historyPlacement` 保存 `version/afterPromptId/beforePromptId/gapTurnIndex`，`historyItemIndex` 保存组内位置；timeline 的 `seq/timestamp` 只表示审计到达顺序。后端投影与前端 merge 按“本地 prompt + Provider history 锚点”重建展示顺序，分页仍按审计 seq，并以窗口 min/max 审计 seq 生成 cursor。placement-only patch 保留首次 `seq/timestamp/start/end/timing`；旧版文本锚点清理只作用于缺少 placement 的历史，Provider-history patch 与既有本地 item identity 冲突时仍保留原始本地事件。raw/timeline 审计文件不重写。
- Agent 文本展示左侧机器人头像；thought、tool call、plan 同属 assistant 结构化时间轴行，同样展示左侧机器人头像；所有展示头像的消息（用户消息、agent 文本、tool call、thought、plan）均在头像下方展示当前消息时间（`HH:mm` 格式）；处理中状态放 composer 内，不展示头像与时间；用户 prompt 保持右侧用户头像。

## 7. 与 Gold Band runtime 的关系

ACP Dialog / Chat UI 只解释 ACP 会话过程，不替代：

```text
run.json
round.json
node.json
artifact validation
workflow control
```

UI 上应避免把 ACP `stopReason`、session status 或 tool call status 直接展示成 Gold Band node status/outcome；ACP 会话头部不展示 session status，处理中状态由 composer 表达。返回 JSON artifact 时，runtime 只在最近有限个 assistant 文本输出段中查找可解析 JSON，支持最后一段为“说明文字 + JSON”或 JSON 出现在倒数几段内，但不无限扫描历史会话；未提取到合法 JSON 时不得把普通 assistant 文本 fallback 成 artifact。普通 worker 进入 invalid-output hidden repair 前必须删除本次非法 output artifact，repair 被停止或中断时 UI 不应继续展示旧的无效产物。Gold Band runtime canonical state 仍由 task / run / round / node / attempt / artifact 维护。

## 8. UI 功能模块清单

ACP UI 不按“第一阶段 / 第二阶段”组织，而按可独立实现的功能模块拆分：

1. `ACPChatDialog` 容器与布局。
2. `ACPSessionHeader` 会话身份与 Raw frames 入口展示，不展示 ACP session status。
3. `ACPMessageList` 会话流渲染。
4. `ACPComposer` 用户输入与等待态。
5. `TextDelta` 流式消息合并。
6. `ThoughtBlock` 折叠思考内容。
7. `ToolCallCard` 工具调用卡片。
8. `ChildAgentGroupCard` 子 Agent transcript 分组。
9. `PermissionRequestDialog` / inline approval card。
10. `PlanBlock` 计划块。
11. `ModeUpdate` / `ConfigUpdate` / `SessionInfo` 状态提示。
12. `RawFrameViewer` 诊断视图。
13. 错误、断线、恢复、seq gap 提示。

详细执行 todo 见：

```text
docs/gold-band/开发计划/acp接入/acp功能模块todo列表.md
```

## 9. 一句话总结

> Gold Band ACP UI 应是一个 Dialog / Chat UI：用户通过 composer 输入，agent 输出以消息、thought block、tool card、plan block、permission dialog 和诊断视图呈现；UI 的唯一数据源是 ACP 统一事件，而不是 terminal/log 或 Claude Code legacy CLI 输出。

### 斜杠命令验收固化

- Rust：覆盖五类 Agent 的读写目录策略；Codex 同时读取 `.codex / .agents`，Claude 不读取 `.agents`；命令 payload 解析、ACP 优先去重、命名空间与 Unicode Skill 名、Skill 增删重扫和旧持久化目录迁移有单测；通知先于 route 注册时仍能按序送达。
- Frontend：`/`、`/ckm:design` 可匹配，空格、英文逗号、中文逗号会关闭；过滤、插入文本、完整命令标签解析和键盘选中项滚动计算有纯函数单测。标签解析先消费最长合法命令 token，再判断后续分隔符，必须覆盖 `ckm:design-system`、`review.fix` 等包含 `- / . / :` 的命令，禁止回溯成较短标签。标签必须只识别当前 Agent 目录中的完整命令，保留原始大小写和分隔符，并与 textarea 首行使用一致的 `rem` 排版节奏和顶部基线；标签使用主题语义色适配明暗主题，摘要统一使用 shadcn Tooltip，不使用浏览器原生 `title`。删除分隔符或破坏命令后取消标签，再次补充分隔符后恢复。命令行的鼠标与键盘选中态必须统一使用 cmdk `data-selected`，以背景、内描边和左侧强调条保证可辨识；浅色主题采用低透明度 `primary` 蓝色，深色主题采用 `foreground` 叠层。共享菜单还需验证紧凑字号与 hint 标签层级、点击外部关闭、删除后重开，以及切换 Agent 时不展示旧目录快照。
- Session Header：静态渲染测试必须固化 Agent 名称与 session id 的 `items-baseline` 组合、一致 `leading-5`、Direct 标题组间距和名称/ID 组内距，并禁止恢复 session id 的垂直 padding 对齐方式；纯函数测试覆盖长 ID 的“前 8 位…后 4 位”投影和短 ID 原样展示，浏览器验收完整值 Tooltip 与复制行为。
- Session ID 复制反馈状态测试必须覆盖 `idle -> copied -> closing -> idle`；`feedback-elapsed` 只关闭 Tooltip、不清除反馈内容，`closing` 阶段拒绝悬浮重开，防止关闭动画闪现完整 ID。
- 快速对话验收：命令列表以带圆角、不参与布局的覆盖层紧贴首行输入下方，打开时 composer 高度不变，左右边缘与主输入框对齐；关闭当前 `/query` 后切页返回保持关闭，输入变化后可重新触发，切换 Agent 后新 Agent 菜单重新打开。会话详情仍在 composer 上方显示浮层，浮层宽度使用 composer anchor 实际宽度并与其左右严格对齐。两处 composer 的命令标签显示一致，发送与持久化仍使用包含 `/${name}` 和分隔符的原始文本。标签宽度由共享 `ResizeObserver` hook 测量并只作用于 textarea 首行 `text-indent`，textarea 保持全宽，第二行及后续换行必须从输入区左边缘开始。
- 键盘滚动验收：`CommandList` 是菜单唯一滚动容器并由组件直接持有 ref；连续按 ArrowDown/ArrowUp 越过当前可见范围时，该容器的 `scrollTop` 必须变化，选中项的实际矩形始终位于容器可见矩形内。测试和实现不得动态发现父级滚动节点，也不得使用跨父节点的 `offsetTop`。
- 构建：`cargo check -p gold-band-desktop`、目标 Rust 单测、`npm run web:test -- --run web/tests/slash-command.test.ts`、`npm run web:build` 必须通过。
