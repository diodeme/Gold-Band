# ACP Dialog / Chat UI 计划

## 0. 当前实现状态

- `RoundDetailPage` 的节点 session tab 已切换为 `ACPChatDialog`。
- 前端新增 ACP session / event / permission / diagnostics 类型，数据来自 Tauri `AcpSessionVm`。
- `ACPChatDialog` 展示压缩 session header、消息流、thought、tool call、plan、permission、raw frames 和 composer。
- 会话 UI 已采用 prompt-kit copy-in 组件承载基础交互：`ChatContainer` 负责消息滚动，`Message` 负责用户/agent 气泡，`PromptInput` 负责 composer，`Tool` 负责工具调用卡片，`ChainOfThought` 负责 thought 折叠展示；ACP 专属逻辑只负责事件映射、权限和诊断。
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
- `available_commands_update`、`usage_update`、session/mode/config update 等状态帧不渲染为聊天消息；它们只更新 session 状态或留在 Raw frames 中排障。
- 未识别事件应进入诊断区或系统提示，不应破坏会话流。

## 4. Gold Band 会话信息架构

节点详情中的 ACP Dialog / Chat UI 建议分为：

1. **Session Header**：provider、adapter、session id、cwd、连接状态、恢复状态。
2. **Message List**：用户消息、agent 文本消息、系统提示。
3. **Reasoning / Thought**：思考内容，默认折叠或弱化。
4. **Tool Calls**：工具调用卡片，作为会话流中的结构化消息块。
5. **Plan**：agent 计划与状态，作为独立 plan block。
6. **Permission**：权限请求与用户响应，用于 ACP `session/request_permission`。
7. **Composer**：用户输入区，用于继续会话、回答 agent 自由文本问题、提交下一次 `session/prompt`。
8. **Terminal / File Details**：命令、cwd、输出、退出码、文件读写路径，作为 tool call 的详情，不作为主输出形态。
9. **Errors**：ACP error、adapter crash、auth required、timeout。
10. **Raw / Diagnostics**：原始 ACP frame / transcript 查看，仅用于排障。

## 5. 推荐组件拆分

基础 AI chat 交互优先使用 prompt-kit 生成到项目内的源码组件，避免自研消息容器、输入框和工具调用基础控件：

- `ChatContainerRoot / ChatContainerContent / ChatContainerScrollAnchor`：承载消息滚动和自动贴底行为。
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
- `ThoughtBlock`：把合并后的 `ThoughtDelta` 映射为 prompt-kit `ChainOfThought`，标题展示思考耗时而非字符数。
- `PlanBlock`：展示计划条目和状态变化。
- `PermissionRequestDialog`：展示权限请求、选项和用户决策。
- `SessionStatusBar`：展示连接、恢复、错误和队列状态。
- `RawFrameViewer`：按 event kind 查看和复制 ACP raw frame。

## 6. UI 展示规则

### 6.1 用户输入

- 用户通过 prompt-kit `PromptInput` 输入 prompt 或回答 agent 的自由文本问题。
- 发送后立即清空 composer 并乐观生成右侧用户消息，同时调用下一次 ACP `session/prompt`。
- ACP client 在发送 `session/prompt` 前持久化 synthetic `userTextDelta`，确保初始 prompt 和后续继续输入都作为右侧用户消息出现在会话流中；只展示用户 prompt，不展示 system prompt。
- 当 node 处于 `waiting_for_user_input`、permission pending、adapter disconnected 等状态时，composer 应显示明确状态。
- 用户输入不走 terminal stdin，不依赖 legacy CLI 会话。

### 6.2 文本流

- 合并连续 text delta，避免一 token 一行。
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

Tool call update 应按 `toolCallId` 更新同一张卡片，而不是生成重复卡片。terminal / file 细节挂载到对应 tool call，不应成为主会话输出。工具卡片使用 prompt-kit `Tool` 承载折叠和状态展示，默认只展示工具名、状态和轻量图标；展开后展示路径、查询等关键参数块与输出摘要；不展示 tool call id、kind、input 或 raw details。工具卡展开/收起属于阅读动作，必须保留当前滚动位置，不能触发会话容器自动滑到底部；长路径、JSON 输出和连续字符必须在工具卡宽度内换行或内层滚动，不能撑宽抽屉。

### 6.5 Permission Request

权限类提问使用 ACP `session/request_permission`：UI 展示 agent 请求、tool call 摘要和可选项，用户选择后返回 `RequestPermissionResponse`。

权限请求可以展示为：

- 阻塞式 dialog：用于必须先决策才能继续的请求。
- inline approval card：用于嵌入会话流并保留上下文的请求。

权限请求必须保留用户选择、时间和相关 tool call id，便于后续排障。用户点击允许或拒绝后，UI 立即乐观关闭 pending 卡片；若响应失败，再恢复卡片并提示重试。pending / waiting 状态使用低强调 primary 语义色，不使用 warning 橙色。

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

### 6.8 Raw / Diagnostics

Raw 视图用于排障：

- 展示 ACP 原始事件 / frame。
- Raw frames 是会话画布的切换视图，不追加到聊天消息流后方。
- Raw frames 按需加载，普通 `get_acp_session` 只统计 raw frame 行数，不解析完整 raw JSONL；Raw frames 详情读取也应有体积上限，避免大文件拖慢会话主 UI。
- Raw frames 按 jsonl 一行一个 frame 的形式展示，默认展示前 300 字符并单行截断；点击该行后以 pretty JSON 或纯文本多行展开，使用克制的暗色代码面板和柔和选中态；短 frame 自然展开不显示内层滚动条，只有超长 frame 才限制高度并显示细滚动条；超长连续字符主动切分换行，内容必须在容器内显示，不能撑出窗口。
- 支持复制。
- 支持按 event kind 过滤。
- 支持关联到会话流中的消息、tool call 或 permission request。

Raw 视图不承担主交互，不把 ACP 原始 JSON 暴露为普通用户默认体验。切换 Raw 视图或展开单个 frame 时必须保留用户当前阅读位置；Raw 详情内容必须主动换行，禁止横向撑出会话抽屉。

### 6.9 处理中反馈与计时

- 会话处于 pending / running 且尚无可渲染事件时，Message List 显示“Claude 调起中”，不显示“暂无 ACP 事件”。
- 用户发送 prompt 到首个非用户帧返回前，底部显示处理中动效、当前步骤耗时和总耗时，避免长时间无变化被误判为卡死。
- 首帧返回后，根据最新可见事件类型切换为“思考中 / 工具调用中 / 回复生成中”；会话进入 completed / failed / cancelled 后停止处理中计时。
- Agent 文本、thought、tool call、plan 和处理中状态都属于 assistant 时间轴行，统一展示左侧机器人头像；用户 prompt 保持右侧用户头像。

## 7. 与 Gold Band runtime 的关系

ACP Dialog / Chat UI 只解释 ACP 会话过程，不替代：

```text
run.json
round.json
node.json
artifact validation
workflow control
```

UI 上应避免把 ACP `stopReason` 或 tool call status 直接展示成 Gold Band node outcome。Gold Band runtime canonical state 仍由 task / run / round / node / attempt / artifact 维护。

## 8. UI 功能模块清单

ACP UI 不按“第一阶段 / 第二阶段”组织，而按可独立实现的功能模块拆分：

1. `ACPChatDialog` 容器与布局。
2. `ACPSessionHeader` 会话状态展示。
3. `ACPMessageList` 会话流渲染。
4. `ACPComposer` 用户输入与等待态。
5. `TextDelta` 流式消息合并。
6. `ThoughtBlock` 折叠思考内容。
7. `ToolCallCard` 工具调用卡片。
8. `PermissionRequestDialog` / inline approval card。
9. `PlanBlock` 计划块。
10. `ModeUpdate` / `ConfigUpdate` / `SessionInfo` 状态提示。
11. `RawFrameViewer` 诊断视图。
12. 错误、断线、恢复、seq gap 提示。

详细执行 todo 见：

```text
docs/gold-band/开发计划/acp接入/acp功能模块todo列表.md
```

## 9. 一句话总结

> Gold Band ACP UI 应是一个 Dialog / Chat UI：用户通过 composer 输入，agent 输出以消息、thought block、tool card、plan block、permission dialog 和诊断视图呈现；UI 的唯一数据源是 ACP 统一事件，而不是 terminal/log 或 Claude Code legacy CLI 输出。