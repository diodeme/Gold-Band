# ACP Dialog / Chat UI 计划

## 0. 当前实现状态

- `RoundDetailPage` 的节点 session tab 已切换为 `ACPChatDialog`。
- 前端新增 ACP session / event / permission / diagnostics 类型，数据来自 Tauri `AcpSessionVm`。
- `ACPChatDialog` 展示压缩 session header、消息流、thought、tool call、plan、permission、raw frames 和 composer。
- 会话 UI 已采用 prompt-kit copy-in 组件承载基础交互：`ChatContainer` 负责消息滚动，`Message` 负责用户/agent 气泡，`PromptInput` 负责 composer，`Tool` 负责工具调用卡片，`ChainOfThought` 负责 thought 折叠展示；ACP 专属逻辑只负责事件映射、权限和诊断。
- 系统提示弹窗正文、原始帧摘要展开详情、子 Agent 结果等长文本区统一跟随应用设置字体；仅在明确需要展示代码或固定宽度标识时才允许局部使用等宽字体。系统提示正文直接复用 AtomEditor/CodeMirror 的 Markdown 查看器并固定 `editable=false`，复制源码与 Markdown/原文切换沿用该组件右上角的工具栏，不使用额外文字、Switch 或独立工具行。
- 系统提示弹窗已收口为 shadcn/Radix Dialog + 原生 flex 滚动容器的单滚动面：标题栏固定，正文使用 Gold Band 统一滚动条且常驻，profile/runtime prompt 不做长度截断；长路径和连续字符在正文容器内强制断行，禁止由 `<pre>` 再创建嵌套滚动或把 Dialog 撑出视口。由于 Dialog 使用自然高度加 `max-height`，正文不得改用依赖百分比 viewport 高度的 Radix ScrollArea。前端回归测试固化 `max-height + flex column + min-height zero + direct overflow-y-scroll child` 布局契约。
- ACP 会话流支持将 `Agent` 工具调用生命周期内的子 Agent transcript 聚合为可展开/收起分组，不再把主 Agent 与子 Agent 输出完全混排。
- ACP session 初始化与后续追问必须分别维护 Gold Band 显式覆盖和 Agent 当前配置：模型只继承 `modelOverride`，权限模式只继承 `permissionModeOverride`，其余 ACP select 配置继承 `configOptionOverrides[实际 optionId]`；不得从 Agent 返回的 `currentModelId / currentModeId / currentValue` 反推 override。发起会话前模型、权限和思考强度都可切回“不指定”；会话详情仅在对应 override 尚为空时提供“不指定”，模型、权限或思考强度一旦选择具体值后都只能在具体值之间切换。same-session prompt、runtime continue 与 AI-DYNAMIC inner continue 只继续使用显式覆盖。
- 发起会话前与已建立会话后的模型配置按 Agent 能力选择载体：同时存在模型与思考强度时使用复合二级菜单，选择任一子项后保留菜单以便继续配置，点击外部才关闭；只有模型时使用普通单项菜单，权限始终使用单项菜单，两者选择后立即关闭。追问 composer 内的 PromptInput 点击聚焦逻辑必须忽略按钮、选择器以及 `menuitem`、`menuitemcheckbox`、`menuitemradio` 等菜单项，配置选择只生效而不主动聚焦文本框。
- ACP session 配置归一化统一采用 `configOptions` 优先、旧 `models` / `modes` 回退：目录、当前值和显示名必须使用同一优先级，避免 Codex 等 adapter 同时返回纯模型 config option 与“模型 × 思考强度”旧目录时展示展开组合。缺少 `category=thought_level` 时继续退化为模型单下拉，不从模型 ID 或名称反向猜测思考强度；回归测试覆盖新旧字段冲突和 legacy-only adapter。
- Composer 配置栏中的模型单选、模型复合菜单与权限单选统一基于非模态 shadcn/Radix DropdownMenu；相邻菜单必须支持双向一次点击切换，不能混用会拦截外部点击的 Select 弹层。单项菜单沿用 Radix 的选择即关闭语义，只有复合菜单拦截子项默认关闭事件。
- 待发送队列与追问 composer 已收口为同一个底部输入 surface：队列存在时 composer 使用无上圆角贴合形态；无附件时不渲染附件 chips 空容器，避免空 `margin-bottom` 把队列和输入框分开。队列进一步对齐任务列表的紧凑 `Collapsible` 标题、箭头、边线、内容行和 `card` 背景，不再让相邻面板分别使用实色与半透明 surface；但保持队列默认展开。标题行负责整体收起/展开，展开后默认展示 FIFO 前 3 条，列表底部独立提供“查看更多 / 仅显示前 3 条”，且整体收起后保留当前展示范围。前端回归测试固定覆盖该布局、颜色与交互契约。
- 2026-08-15 Composer 密度与临时尺寸收敛：会话详情 textarea 默认约两行高，保留内容自增高并开放 320px 上限内的原生纵向拖拽；拖拽高度只保存在挂载中的 DOM ref，不写 React 页面状态或持久化。消息视口与底部区域移除分界线、半透明表面和 backdrop；“当前运行状态 / 会话累计 / 上下文窗口”改为 content-width 一体式 tab，绝对定位并与任务列表、待发送队列或 prompt-kit 输入框整组贴合。tab 取消独立阴影、全圆角和 border，与实际位于栈顶的面板统一使用 `card` surface；栈顶面板按 tab 是否真实可见取消左上圆角，同色填充消除接缝。tab 右侧增加反向圆角连接器，复用 `card` token 填充接缝；连接器与 tab 左上、右上共同使用主题 `md` 圆角 token，并在最大宽度中预留对应空间。tab、连接圆弧、任务/队列栈顶和输入框统一使用无边框 composer surface，队列与任务列表内部也不绘制 divider border；content rail 以 `--gb-material-shadow / --gb-material-edge-shadow` 对整组绘制结果施加一次外阴影，各子面板关闭自身投影，防止拼接处出现明暗断层。快速对话与会话详情删除 composer 容器级 `focus-within` border/ring/filter；文本输入聚焦后只显示原生光标，内部按钮继续保留 `focus-visible`。它不占整行高度，只有实际宽度及圆弧连接范围遮挡后方消息。长运行状态截断，计时和上下文圆环保持可见。任务列表移入同一 content rail 并复用队列 surface，多个折叠面板形成一组连续轮廓。`Agent 调起中` 等运行状态从 PromptInput 内移到 tab 首位。附件按钮移到模型/权限左侧并使用中英文 `acp.attachHint`；快速对话与会话详情删除独立 Enter/Shift+Enter 提示行和废弃 hint 投影。接口回归固定 i18n、DOM 顺序、状态位置、tab 的绝对定位、内容宽度、共享 `card` surface、统一主题圆角、整组主题阴影、无容器级选中态、无边框 surface、圆环安全间距、任务/队列 surface、附件位置、可拖拽 class、用户最小高度与自增高共同生效，以及两类 composer 均不再保留提示占位。
- 2026-08-17 Composer 无边框一致性修复：将会话详情角标、待发送队列、任务列表、人工检查/外部状态面板与 prompt-kit 输入框统一收敛到 `ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName`，移除各段外轮廓 border、角标连接边和队列/任务内部 divider；折叠、拖拽、队列 revision 与 composer 草稿状态不变。DOM 回归固定输入框、队列和任务 surface 均显式消费 `border-0`，队列展开内容不再生成分隔 border。
- 2026-08-18 Composer joined surface 外轮廓恢复：按主题边界视觉复核，为快速对话、会话详情输入框和图片缩略图恢复 1px 完整主题 `border`。会话详情角标与其下最上层 surface 仍作为一个 joined surface：角标取消底边并以 `card` 连接桥覆盖下方顶边，消除内部横向分割线；右侧反向圆角删除 box-shadow 填充与独立 CSS border 两套近似几何，改为单一内联 SVG 四分之一圆路径，同一路径同时投影 `card` 填充边界和主题 `border` 描边，并向角标内部覆盖 1px 旧右边框。连接器填充区另向 composer 内延伸 1px，只遮盖圆弧下方透出的内部顶边抗锯齿，圆弧切点右侧继续由 composer 原生顶边承接。队列与任务列表内部仍不恢复 divider，交互和状态生命周期不变。
- 2026-08-18 Composer 内容轨道左对齐：继续复用 prompt-kit `PromptInput` 与共享 `ComposerContextArea`，不调整附件或 textarea 的独立定位。快速对话 surface 水平 inset 从 `px-4` 收敛为 `px-2.5`，图片、文字和底部操作整体左移 6px；会话详情通过共享布局常量以 `px-0` 覆盖 prompt-kit 默认 `px-2`，正文、附件与命令标签统一使用 `px-2.5` 内层，使浏览器计算出的内容起点与会话信息角标首段文字一致。DOM 契约测试固定两类 composer 都消费布局常量，数据、状态与渲染范围保持不变。
- 2026-08-18 Composer 边框粗细单一配置：在 composer 布局层新增 `ACP_SESSION_COMPOSER_BORDER_WIDTH_PX`，并由它生成 rail CSS 变量、stack surface 边框、角标连接桥、SVG stroke、viewBox 与遮盖深度；后续只需修改该变量即可同步调整 joined surface 外轮廓，避免接缝参数再次漂移。
- 2026-08-15 底部干预层布局收敛：权限申请、用户问询和会话级错误统一复用消息 content rail 的最大宽度、水平 padding 与消息行缩进，不再相对整个 viewport 单独排版。干预层底部由 Tailwind `pb-10` 统一提供 composer 信息 tab 的上探安全区，使 prompt-kit 自动贴底终点包含该留白；权限卡、问询卡和错误卡均不得被 tab 遮挡。接口回归固定 content rail 与安全区契约。
- 会话运行区的产物/附件入口已改为与任务列表一致的 `Collapsible` 折叠面板：默认收起展示非零产物/附件计数，展开后点击文件项继续复用现有详情弹窗；当任务列表存在时，产物/附件面板固定在任务列表上方。
- 2026-08-19 附件物化快照大小收敛：删除 `MaterializeAttachmentFileInput.size` 这一选择时瞬时元数据，后端以 Base64 解码后的实际字节作为唯一权威大小，并据此执行空文件、单文件、总量校验及返回 `AttachmentFileVm.size`。活动日志等源文件在选择后继续变化时保存实际读取快照，不再因选择时大小与读取后大小不同而报“附件保存失败”；接口回归固定 canonical size 与重复文件名物化语义。
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
- 为定位“live event 已到达但打字机未激活”的间歇性问题，增加默认关闭的 ACP streaming 结构化诊断轨迹：覆盖 router、attempt/branch locator、animation readiness/replay 水位、streaming target、Markdown render 与 terminal settle。轨迹不保存消息正文，最多保留 2,000 条并可从 DevTools 导出 JSON；单元测试固化容量上限、快照隔离和 locator 差异报告。
- ACP Markdown 播放复用单个 Streamdown 文档的 renderer token，并由每条消息唯一的文档水位严格顺序释放；列表 marker、各列表项、后续标题和段落不得各自启动动画。播放器按 Streamdown block DOM 身份缓存 token 索引，累计快照更新只重建变化 block，稳定 block 不重复扫描或校准。播放积压只调整 token 释放速度，不改变 canonical 发布频率或 Markdown 解析边界；工具、用户、终态或会话切换必须立即 settle。
- ACP streaming 调试开关同时采样浏览器 Long Animation Frame、播放 tick、会话 ResizeObserver 和自动贴底；高频 Resize/贴底只按 500ms 汇总，Long Animation Frame 只保留 blocking/render/style-layout 时长与最多三个 script attribution，不保存正文或脚本 URL。
- ACP streaming 诊断额外覆盖播放层 init/reconcile/约 500ms sample/超过 50ms long-frame/settle；只记录 canonical 长度、token 总数与水位、积压、帧数、释放数、最长帧、reconcile 耗时和原因，禁止记录正文或每字符事件，确保诊断不会成为新的热路径。
- 普通 ACP session 查询必须返回指定 `branchId` 的语义块窗口，而不是完整会话文件。根分支读取 `acp.timeline.jsonl`，Agent 分支读取 `agents/<AgentExecutionId>/timeline.jsonl`；初始默认返回最近约 30 个语义块，前端保留有限多页缓冲。分页游标统一为 `beforeCursor / afterCursor`，折叠工具数、Activity 审计数和子 Agent 历史都不参与父 branch 的 `hasOlder`。首个 root session-ready 快照仍必须把 snapshot metadata 与首个 `goldBandPrompt` 一起暴露；Agent 分支以 synthetic Agent Prompt 作为只读会话起点。
- `available_commands_update`、`usage_update`、session/mode/config update 等状态帧不渲染为聊天消息；它们只更新 session 状态或留在 Raw frames 中排障。
- ACP runtime 文件位于 `~/.gold-band/projects/{project-id}/tasks/...`，不写入项目工作树；ACP 会话身份只以当前 user runtime attempt 的 `worker-ref.json` 为事实源：`continue_ref.acpSessionId` 决定 resume/load 的目标 session 与 UI header 的 provider session id；`acp.session.json` 不再作为 session id 来源，但会保存 status、capabilities、adapter 配置快照、stop reason，以及通过可选 `session/list` 轮询 best-effort 拉取得到的 `title` 缓存。该能力受项目级 `configs/app-config.json` 控制，默认关闭。title 仅用于后续 UI/检索储备；本期不作为会话头部展示的依赖字段，拉取不到时保持为空。
- `configs/app-config.json` 是版本内共享的项目级 app config 入口，不是用户本机偏好设置：适合开发期可选能力和共享 UI/runtime 参数的统一管理。CLI 与桌面端都读取同一份文件；未声明字段继续走代码默认值，不要求每个配置都显式写入。当前文件示例：`{ "acpSessionTitleRefreshEnabled": false, "acpChatEventPageSize": 360 }`。
- session-wide metadata、pending permission、usage 和 diagnostics 由后端流式扫描全量事件得出，不允许为了 UI 轮询保留或传输全量事件数组。
- `Agent` execution 是后端规范分支模型：事件进入 timeline 前已按内部 branch metadata 路由，前端只将父 branch 的 launch item 投影为 `AgentLinkRow`，不得重新按 seq 生命周期窗口框选子事件。
- 未识别事件应进入诊断区或系统提示，不应破坏会话流。
- runtime control 在 provider session 建立前完成 timeline 扫描时，只能写入 `availability=unavailable` 的控制占位，不能把文件存在投影为 established session。初始 `get_acp_session` 对“无 session id、无可展示 timeline event、无 Agent branch execution”的该类占位返回 `null`；前端同时以 `isAcpSessionReadyForInitialDisplay` 守住 hydrate 成功门槛。返回 `null` 或未物化快照时不立即显示 "ACP 会话失败"，而是以 120ms / 300ms / 700ms / 1200ms 递增间隔短暂重试，避免与后端 `session/new` 及首次 timeline 写入形成竞争；所有重试耗尽仍未获得可展示快照时才降级为失败态。
- 若 runtime 已在 session-ready、session id 或首批 timeline event 形成前进入失败状态，ACP 会话壳必须以外层 canonical run lifecycle 为准立即停止初始 loading，优先展示现有错误状态组件与具体原因；失败状态包括 `runtime-abnormal`、`error-blocked`、`failed/failure/error/killed` 以及明确的 `runtime-error` composer。Direct 模式同样必须把 runtime diagnostic 交给会话壳，不能因为没有建立 ACP session 而丢失真实错误。`process-interrupted` 继续进入中断状态，其他非错误 pause 不得误判为初始化失败；已经建立 session、已有 timeline event 或 runtime 仍 active 时继续走正常会话路径。
- MCP transport 兼容性在 ACP `initialize` 与 `session/new|load` 之间统一收敛：使用本次连接实时返回的 `agentCapabilities.mcpCapabilities`，不按 `codex-acp` 等 Agent ID 硬编码。`stdio` 始终保留；HTTP/SSE 仅在 Agent 明确声明不支持时从本次请求剔除；能力字段缺失时保持兼容行为。真正发送 `session/new|load` 且发生过滤时，在 attempt 的 `acp.diagnostics.jsonl` 写入 warning，稳定 code 为 `acp.mcp-transport-unsupported`，data 至少包含 `agentType` 及 `skippedServers[].name/transport/capability`；直接复用已附着 session 时不重复写 warning；`acp.raw.jsonl` 中的 `session/new|load` 只记录实际发送的 accepted server。
- ACP adapter 关闭时序已收口为 `open → draining → closed`：关闭连接前先将其移出复用池并拒绝新 prompt，再取消待处理 permission/elicitation、发送 session cancel，并按 ACP session id 有界等待活跃 prompt worker 收敛；排空后执行 `session/close`，最后关闭 transport。draining 期间保留在途 response 的写权限，避免 AskUserQuestion/permission worker 被唤醒后向已关闭 stdin 写入。若排空超时，后续 transport 错误统一映射为 `interrupted`，不写入 `ACP prompt failed` diagnostics。
- 关闭恢复的单元验收固定覆盖：draining 拒绝新普通请求但允许 shutdown request；AskUserQuestion 等待中关闭会先生成 decline 并完成 prompt drain；worker 不收敛时等待必须按 timeout 返回；draining/closed 结构化错误必须被 prompt runtime 识别为 transport interruption。

## 4. Gold Band 会话信息架构

节点详情中的 ACP Dialog / Chat UI 建议分为：

1. **Session Header**：provider、adapter、session id、cwd、连接状态、恢复状态。
   - Agent 名称与 session id 作为同一身份文本组按 baseline 对齐，并统一使用紧凑行高；图标与右侧操作继续按控件中心对齐，不增加针对单一字体的位移补丁。
   - Direct 标题与 Agent 身份使用独立组间距；长 session id 统一格式化为前 8 位与后 4 位的紧凑文本，Tooltip 和剪贴板仍消费完整值，避免技术标识挤压主标题。
   - session id 复制反馈与 Tooltip 由同一状态机管理；复制反馈结束或桌面窗口失焦后进入重开锁定，忽略切回应用时 WebView 残留 focus/pointer 产生的 `open=true`，直到鼠标移出或焦点在应用内移走后才解锁。回归测试必须覆盖“复制 → 反馈关闭”和“复制 → 切换应用 → 返回”两条路径。
2. **Message List**：用户消息、agent 文本消息、系统提示。
3. **Activity Summary**：连续 thought / tool / error 形成一个稳定语义块，默认折叠；展开后按局部 cursor 加载审计详情。
4. **Agent Link**：父 branch 只显示 `AgentLinkRow`；点击后在右侧工作区打开只读 Agent 会话，不内嵌 transcript。
5. **Plan / Todo**：只显示当前 branch 的最新计划快照，位于 composer 或只读会话底部状态区上方，不重复写入消息流。
6. **Permission / Elicitation**：只展示当前 branch 的待决 intervention；Agent 阻塞时仍可在只读 Tab 内决策。
7. **Composer**：仅根会话提供用户输入、停止与 ACP 配置；Agent branch 不挂载 composer。根输入用于继续会话、回答 agent 自由文本问题、提交下一次 `session/prompt`；输入区下方展示 ACP 配置。新建对话与详情页复用同一模型选择器：只有模型时渲染普通模型下拉；同时存在 `category=thought_level` 时，模型栏变为单个复合下拉，第一层提供模型和思考强度两个子入口，第二层展示对应选项，子菜单由 Radix 原生指针、点击和键盘状态管理，不额外绑定点击翻转。权限模式继续独立展示。模型和思考均为空时复合触发器显示“不指定”，只选择思考强度时显示 `不指定 · 思考强度`；UI 保留 Agent 返回的实际 config option ID，不对 `reasoning_effort/effort` 做分支。追问区的复合菜单嵌套在 PromptInput 内，PromptInput 只在点击空白或非交互内容时聚焦文本框，不得从按钮、选择器或菜单项抢焦点。
   - 发起会话与追问会话的模型、权限触发器统一显示弱化配置名和当前主值，并复用同一套 composer 配置触发器样式：最终高度 36px，统一宽度、间距、无阴影表面、边框、深色背景、箭头和焦点态；模型复合值仍按 `模型 · 思考强度` 组合，不把思考强度拆成独立触发器。Composer 配置菜单统一使用非模态 `DropdownMenuTrigger`；选择模型或思考强度时只更新值并保持菜单打开，点击外部才关闭，回归必须覆盖追问区主菜单、二级菜单，以及“模型 → 权限”“权限 → 模型”的双向一次点击切换。
8. **Terminal / File Details**：命令、cwd、输出、退出码、文件读写路径，作为 tool call 的详情，不作为主输出形态。
9. **Errors**：ACP error、adapter crash、auth required、timeout；会话内错误归入 Activity 审计，阻断错误保留独立反馈。
10. **Raw / Diagnostics**：原始 ACP frame / transcript 查看，仅用于根会话排障，不在只读 Agent Tab 展示入口。

## 5. 推荐组件拆分

基础 AI chat 交互优先使用 prompt-kit 生成到项目内的源码组件，避免自研消息容器、输入框和工具调用基础控件：

- 普通 `overflow-y-auto` message list：承载 ACP 历史浏览和向上分页；对 prepend 历史使用 scrollHeight 差值补偿 scrollTop，避免虚拟列表重新测量高度时闪回；对流式消息内容增高使用内容尺寸监听来维持底部贴合，避免只在事件数量变化时滚动。
- `ChatContainerRoot / ChatContainerContent / ChatContainerScrollAnchor`：仅用于不需要历史分页的普通聊天容器场景。
- `Message / MessageContent`：承载用户与 agent 气泡。
- `PromptInput / PromptInputTextarea / PromptInputActions / PromptInputAction`：承载 composer、快捷键、loading 和 action 区域。受控输入的 autosize ref 必须稳定，每次 value 更新只在 layout effect 中测量一次，不能在 `onChange`、ref 重挂载和 layout effect 三条路径重复读写高度。
- `ComposerContextArea`：快速对话与会话详情复用的输入上下文功能区，位于 `PromptInput` 内部并统一呈现多引用、图片缩略图和普通文件标签；按容器宽度换行，最多两行后内部滚动，不使用独立卡片或第二套附件面板。
- `Tool`：承载工具调用卡片的折叠、状态、输入输出展示。
- `ChainOfThought / ChainOfThoughtStep / ChainOfThoughtTrigger / ChainOfThoughtContent`：承载 thought / reasoning 折叠展示。

ACP 专属组件只做协议事件映射和业务状态组合：

- `ACPChatDialog`：组合共享会话视口、intervention 与根会话 composer，并负责 branch 查询和实时事件合并。branch locator 使用稳定对象引用下发；composer 草稿逐键更新不得使历史 Markdown/Activity/Tool 消费者失去 memo 命中。
- `ConversationViewport` / `ACPMessageList`：根会话与 Agent 分支共用的原生滚动消息视口，按语义块展示正式文字、活动摘要、TODO 和 Agent 链接。
- `ACPSessionHeader`：展示 session/provider/adapter/cwd/连接状态；Agent 只读容器按只读边界隐藏不适用入口。
- `ACPEventRenderer`：根据 Gold Band 规范事件类型选择渲染组件，不读取 provider 私有 metadata。
- `ToolCallCard`：把 ACP `ToolCall` / `ToolCallUpdate` 映射为 prompt-kit `Tool` props。
- `AgentLinkRow`：在所属父分支中展示轻量 Agent execution 链接，不挂载子 transcript。
- `RightWorkspaceDock` / `AgentConversationPanel`：用通用多 Tab 右侧工作区承载只读 Agent 分支会话；仅挂载激活 Tab 的完整视口。
- `AcpActivityBatchRow`：展示一个稳定活动语义块，并在首次展开后按局部 cursor 延迟读取审计详情。摘要总数与本地详情完整性分开建模；即使已混入少量 live 尾部，只要本地审计数小于摘要总数，首次展开仍读取权威详情。
- `ThoughtBlock`：把合并后的 `ThoughtDelta` 映射为 prompt-kit `ChainOfThought`，标题展示思考耗时而非字符数。
- `PlanBlock`：展示计划条目和状态变化。
- `InterventionLayer` / `PermissionRequestCard`：只展示当前 branch 的待决权限或提问，并使用规范 request ID 提交决策。
- `SessionStatusBar`：展示连接、恢复、错误和队列状态。
- `RawFrameViewer`：按 event kind 查看和复制 ACP raw frame。

## 6. UI 展示规则

### 6.1 用户输入

- 用户通过 prompt-kit `PromptInput` 输入 prompt 或回答 agent 的自由文本问题。
- 发送后立即清空 composer 并乐观生成右侧用户消息，同时调用下一次 ACP `session/prompt`。
- 每次 Gold Band 用户输入（包括 round 顶部“继续运行”触发的本地化 `继续/Continue`）都要生成新的 prompt identity，并同时写入乐观用户气泡与后端 synthetic `goldBandPrompt` 事件元数据；同文本历史 prompt 不得复用同一 identity。
- 普通 prompt 的 optimistic 用户气泡同时保存提交瞬间最后一个 canonical `endedSeq/seq`。并行会话切换或 replay/snapshot 分批到达时，临时气泡按该序号锚点插入，不得追加到已经到达的本轮 Agent 响应之后；匹配 `promptId` 的 durable `goldBandPrompt` 到达后由 canonical 序号接管，不能按正文或时间戳重排。
- ACP client 在发送 `session/prompt` 前持久化 synthetic `userTextDelta`，确保初始 prompt 和后续继续输入都作为右侧用户消息出现在会话流中；稳定 system prompt 只在不支持 system prompt 的 provider 降级场景中作为 Gold Band hidden 段进入 user prompt。
- synthetic `goldBandPrompt` 是 Gold Band 本地输入的唯一事实源，但 ACP session 是可由外部 Agent 客户端继续的共享会话。`session/load` replay 中的 `user_message_chunk` 必须与既有 Gold Band prompt 有序锚点对账：Provider 漏回放部分本地 turn 时允许跳过缺失锚点并继续匹配后续本地 prompt；本地 prompt 回显不写 timeline，完全匹配不到剩余锚点的外部 user turn 及其 assistant/thought/tool/plan 更新先按完整 turn 暂存，遇到下一个本地 prompt 后整组写入并标记 `beforePromptId`，尾部历史在 load finish 时只标记 `afterPromptId`。Provider 提供 message/tool identity 时直接复用；缺失 ID 的 replay 使用 `session + afterPromptId + gapTurnIndex + kind/itemIndex` 生成稳定身份。必须回归覆盖“本地 `hi, hi, ask` / replay `hi, external, ask`”，确保 `external` 位于第二个 `hi` 之后、`ask` 之前，而 `ask` 及其工具调用仍按本地回放抑制。
- Claude ACP 暂未为 interruption control 提供结构化字段。归一化层临时只精确隐藏 `[Request interrupted by user]` 与 `[Request interrupted by user for tool use]` 两个完整值，不做模糊文案匹配，并保留 raw frame；上游支持 typed interruption 后删除该文本适配。
- prompt 内存活动状态拆为 `Starting / Accepted / Running / CancelRequested`。持久化 synthetic prompt 后进入 `Accepted` 并立即发 live event；实际发起 `session/prompt` 前再进入 `Running`。前端匹配到 durable `promptId` 即结束“发送中”并进入“处理中”，不以 provider echo 作为接受边界。
- Task-level input attachments 是新 ACP session 的初始化上下文，只随 `SessionMode::New` 首次发送；同一个 session 内 resume/continue 不自动重发这些 task inputs，也不在后续用户气泡下重复展示。用户在 composer 中本轮显式选择的附件仍作为本轮 same-session prompt attachments 单独发送和展示。
- Agent 正式消息选择引用只对完成态 `textDelta` 正文开放。选区边界按 Range 内首尾实际非空文本节点归一化，兼容浏览器整段全选时端点落在消息外层容器的情况；归一化后必须完全位于同一消息 DOM 边界，跨 Activity/Thought/Tool/Permission/Elicitation、头像/时间或跨消息时不产生引用动作。引用进入按完整 session locator 隔离的 composer draft，可多项添加、逐项预览删除；相同来源和文本去重，最多 64 条且总字符上限 12,000。发送 DTO 只携带 `displayText + quotes`；后端把引用作为普通用户输入，只校验条数、总字符数、唯一 ID 与元数据长度，不读取 timeline 或验证来源存在，再按顺序构造 Markdown blockquote prompt。用户消息将引用保存在 `raw.quotes`，以“n 条引用”打开 shadcn Popover，标题固定且列表在最大 24rem/视口余量内滚动。待发送队列暴露引用计数并在编辑正文时保留引用；提交草稿按完整 session locator 分离，失败只恢复到原 session/branch 的空草稿。
- Agent 正式消息复制复用 prompt-kit `MessageActions / MessageAction`，仅在 `textDelta` 正文退出流式态、非失败且非空时挂载。按钮在桌面 hover/focus 时显示、触摸环境常显，复制用于 Markdown 渲染的 canonical 原文并提供局部对勾反馈；透明 Agent 正文使用 `pt-2 + pb-0`，显式对齐 36px 头像与 24px 首行中心线而不恢复底部气泡留白，正文与 24px 操作按钮间距为 2px，操作区固定为紧凑占位高度，主会话与只读 Agent 会话的 timeline 语义块间距统一为 4px，显隐操作不得引发布局跳动；不得读取 DOM、重新序列化渲染结果或把复制状态提升到会话级，避免历史消息树无关重渲染。
- prompt-kit Markdown 围栏代码块启用 Streamdown 官方 controls 与 `@streamdown/code` Shiki 高亮插件：顶部显示 Markdown 明确声明的语言，逐块复制纯代码正文，关闭下载和默认行号；代码正文使用 `pre-wrap + overflow-wrap:anywhere` 保留源码格式并在消息宽度内自动折行。禁止浏览器端自动猜测语言；高亮器按已出现语言异步加载并缓存，不能阻塞正文首屏渲染，代码块复制反馈由 Streamdown 组件局部管理。Tooltip 只包裹不接管复制事件的原生触发节点，不能用 `asChild` 注入覆盖第三方按钮自身的 `onClick`。
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
- 实时 text/thought 累计快照继续使用 stable identity 的 latest-wins 单项缓冲和 125ms 合并窗口；普通 DOM `scroll` 不再视作用户交互，只有 wheel、滚动键或滚动条 pointer 输入开启 180ms quiet window。每批 pending 更新设 250ms 绝对发布 deadline，避免自动贴底或持续滚动把正文饿死到 `usageUpdate/session terminal` 才一次显示。
- 消息视口的 scroll handler 每动画帧至多执行一次，只保存 O(1) 的滚动位置、贴底和分页状态；不得在滚动热路径遍历 `[data-acp-item-key]` 或逐项读取 `getBoundingClientRect()`。精确消息 anchor 在卸载/会话切换时捕获，prepend 历史继续使用独立 pagination anchor 补偿。
- 前端回归覆盖：自动/布局 scroll 不触发 interaction quiet；wheel、键盘和滚动条输入会触发；持续交互仍受 pending deadline 限制；scroll 热路径不扫描消息 DOM；终态事件保持正文先 flush、生命周期后收敛。
- 流式 Markdown 移除 Gold Band 自制的 32ms visible-prefix RAF，改用 Streamdown 2.5 单文档实例的 `animated + isAnimating` renderer token，由 Gold Band 的唯一轻量 RAF 只沿 block cursor 释放严格连续的文档前缀；Gold Band 只管理 canonical snapshot、live target、会话 identity 和 token 播放索引，不判断未闭合 Markdown。首次以 streaming 挂载的新回复从零播放；static 历史在重新进入 active streaming 时先把当前 renderer token 全部结算为基线，只播放后续 append，禁止沿用 static block 计数。回归固定：每条流式消息最多一个 RAF、streaming canonical 全文在 DOM、稳定 block 索引复用、settle 后旧消息静态完整、新消息独立动画、static→streaming 不重播、跨块文档语义保持，以及不同会话复用 provider event id 时 render key 隔离。
- 2026-08-15：补齐 Agent Markdown 本地文件链接的位置展示。继续复用 prompt-kit/Streamdown link override 与既有 Workspace 解析、定位链路，不修改后端 DTO 或 canonical target；展示层从 href 的 `:line[:column]`、`#Lline[-LendLine]` 后缀派生紧凑位置，并与文件名组成同字号、同字体、同字重、同颜色的连续 `文件名:位置` 文本，label 已含等价位置时不重复。接口回归覆盖 Windows/相对/file URI、行列、范围、无位置、点击参数不变、视觉分组和静态 Markdown/右侧工作区 render 隔离。

### 6.3 Thought / Reasoning

- 默认折叠。
- 标识为 agent 内部过程，不作为 runtime 判定依据。
- 若 provider 不返回 thought，则隐藏该区域。
- Thought delta 与 text delta 分流，不混入最终回答正文。
- 连续 thought delta 应合并为一个思考过程块；如果中间只穿插 usage / available commands 等隐藏状态帧，仍按同一个 thought block 展示。
- Thought 标题展示从 ACP event timestamp 派生的思考耗时（如 `12 秒` / `12s`），不展示字符数。
- 2026-08-13 会话运行态视觉收敛：Thought 普通态去除嵌套卡片阴影，展开正文限制为 `max-h-72` 并提供可聚焦的内部纵向滚动区；长思考不会无限拉长消息流，滚动状态保持在 Thought 自身，不进入会话根状态或 Context。

### 6.4 Tool Call

Tool call 卡片展示：

- 工具名 / title
- status
- input 摘要
- output 摘要
- 文件位置 / locations 仅在包含具体文件、行号或 range 时展示；Glob 这类仅重复搜索根目录的 locations 默认隐藏
- terminal metadata
- raw input / raw output 展开入口

Tool call update 应按 attempt-scoped `toolCallId` 更新同一条审计项，而不是生成重复卡片。事件归一化、磁盘分支路由和 live push 共用稳定 `event.id / toolCallId / branchId`；merge key 不得依赖持续变化的 `seq`。terminal / file 细节挂载到对应 tool call，不应成为正式文字。工具卡使用 prompt-kit `Tool`，标题行左对齐显示“操作名 + 次级参数”，例如 `Glob .claude/**/*`、`Read xxx.js`；Activity 折叠时不构造工具详情，Activity 展开后只获得压缩审计项，单条工具再次展开时才通过 `get_acp_tool_detail` 读取 raw input/output。长路径、JSON 输出和连续字符必须在工具卡宽度内换行或内层滚动，不能撑宽会话视口。

2026-08-13 工具调用与运行态样式继续复用 prompt-kit `Tool`、shadcn `Collapsible/Button` 和 Lucide：普通工具卡收敛为低边界透明紧凑行，关键参数使用单个等宽 chip，详情以左侧细竖线展开；Activity 的详情懒加载和单工具按需读取接口保持不变。Todo 展开区改为状态标记、正文和弱状态文案组成的任务行：完成使用勾选、进行中使用运行环、待处理使用小号中性空心点，不再显示重复顺序的编号圆环；composer 的 canonical session 耗时跟随状态栏正文排版，只用 `tabular-nums` 稳定数字宽度并复用既有间距，不增加装饰分隔点、新依赖、额外缓存、100ms timer 或 shimmer。目标 Web 回归覆盖长 Thought 滚动、工具详情按需读取、Todo 状态映射及计时排版；性能评审结论为渲染范围和数据加载量不变，无需 benchmark。

`Agent` 工具调用在所属父 branch 中只渲染 `AgentLinkRow`，展示 Agent 名称、说明、统一 execution 状态以及 ACP 可客观确认的工具数和文件读写数。点击后通过稳定资源 key 在通用 `RightWorkspaceDock` 中打开或激活只读 `AgentConversationPanel`；紧凑窗口使用同源 Sheet。嵌套 Agent 在父 Agent 会话中继续显示相同链接并打开新 Tab，不递归挂载 transcript，不使用 Collapsible，也不按 seq 邻近关系猜分组。Claude `_meta.claudeCode.subagent/toolName/parentToolUseId` 只允许在 Rust ACP 适配边界转换为 `_meta.goldBandConversation` 与稳定 `AgentExecutionId`；前端、分页和持久化不得读取 Claude 字段。

2026-08-02 破坏式替换旧 `ChildAgentGroupCard`：根 timeline、每个 Agent timeline 和 Agent lifecycle index 分离持久化；根/Agent 共用 branch 查询、消息渲染、语义分页和 intervention。后台 Agent launch 的 `completed` 只表示已接受，不能生成正式结果或把 execution 提前置为 completed；根停止只中断仍活动的 Agent，已有正式完成证据的 Agent 保持 completed。

### 6.5 Permission Request

权限类提问使用 ACP `session/request_permission`：UI 展示 agent 请求、tool call 摘要和可选项，用户选择后返回 `RequestPermissionResponse`。

权限请求可以展示为：

- 阻塞式 dialog：用于必须先决策才能继续的请求。
- inline approval bar：用于嵌入会话流并保留上下文的请求，视觉上参考 prompt-kit `system-message` 的轻量提示，而不是大块表单卡片。

权限请求必须保留用户选择、时间和相关 tool call id，便于后续排障。用户点击允许或拒绝后，UI 立即乐观关闭 pending 卡片；若响应失败，再恢复卡片并提示重试。pending / selected 的确认协议键必须使用 ACP JSON-RPC `session/request_permission` 原始 request id；timeline item id 可以是 `permission-<id>` 展示键，但前端提交、后端 `AcpPermissionRequestVm.requestId`、`acp.permission-request.<id>.json` 和 `acp.permission-response.<id>.json` 必须统一回到原始 id。新旧 UI 同时打开同一 ACP session 时，只能消费同一个 canonical requestId，不允许各自用本地展示 id 写响应文件。pending / waiting 状态使用低强调 primary 语义色，不使用 warning 橙色；审批卡片固定为信息行 + 按钮行两层，宽度不强制撑满会话列，按钮较多时使用居中的两列按钮组，不得挤压标题和 pending 状态；allow 选项使用浅色 accent surface，reject 使用中性描边，禁止把所有 allow 同时做成深色实心主按钮。按钮使用紧凑胶囊形态，长选项文本单行截断，并以整个按钮作为 shadcn Tooltip trigger，在 hover / keyboard focus 时展示可换行全文，同时提供完整 `aria-label`。所有 `session/request_permission` 都只通过权限卡片响应，不按 option id、名称或 kind 猜测产品语义，也不允许 composer 发送隐式代替用户选择。Direct 会话的 Agent turn 尚未完成时，composer 新消息继续进入现有 prompt queue，和 pending 权限卡片独立存在。

2026-07-28 权限卡片视觉与长文本验收已固化：复用现有 shadcn `Button` / `Tooltip` copy-in 组件，卡片收敛为低边界、低阴影的轻量审批条；前端静态渲染单测覆盖浅色 allow / 中性 reject 层级、截断标签、Tooltip trigger 与完整无障碍名称。UI 验收需在本地会话页同时检查浅色和深色主题，以及长命令选项的鼠标悬浮与键盘聚焦全文展示。

2026-08-21 权限卡片工具参数改为有界摘要预览：使用 CSS line clamp 最多展示 6 个完整文本行，超出部分在末行显示省略号，不裁出残缺行，也不在卡片内增加滚动；决策按钮固定排在摘要之后，确保高熵或超长参数不会把允许/拒绝入口推出可视区域。前端契约测试覆盖行数约束、截断样式及按钮顺序。

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

Round 详情页顶部的“继续运行”属于 canonical workflow runtime 动作，不复用 composer 的任意用户输入。它只在当前 run / round / node 为可恢复 `paused` 时出现；`error_blocked` 在 UI 上显示为错误阻塞，但仍属于用户可显式继续的暂停态。点击后自动恢复当前 attempt 的 ACP session，并发送本地化短 prompt：中文 `继续`，英文 `Continue`。严格 continue 按能力选择 `session/resume` 或 load-only fallback；恢复 RPC 失败或 Agent 未声明任何恢复能力时，不允许 fallback 到新 ACP session。连续的用户 prompt 必须按事件边界独立成气泡展示，不能把恢复 prompt 拼接到上一条需求 prompt 末尾。

### 6.8 Raw / Diagnostics

Raw 视图用于排障：

- 展示 ACP 原始事件 / frame。
- Raw frames 是会话画布的切换视图，不追加到聊天消息流后方。
- Raw frames 按需加载，普通 `get_acp_session` 只统计 raw frame 行数，不解析完整 raw JSONL；Raw frames 详情读取也应有体积上限，避免大文件拖慢会话主 UI。
- 普通 session 返回的 UI event raw 只能保留渲染 tool、plan、permission 所需的摘要字段，超长字符串和超大 raw payload 必须截断；完整原始内容只通过 Raw frames 分页查看。
- 最新 ACP error diagnostic 或 Raw frame 中的 JSON-RPC `frame.error.message` 必须显示为会话顶部错误横幅，不再重复插入消息流；若该错误时间之后出现新的正常 agent 输出，横幅自动消失。
- ACP stop 点击后同步把业务 runtime 收敛到 `paused + process_interrupted`，并把当前 turn 的进程内活动状态设为 `CancelRequested`；返回 `accepted` 不等于 ACP 已停止，composer 继续显示 stopping。Runtime control cursor 只记录控制权切换，不得把 `latestTurnStatus` 改成 `cancelled`；metadata 不存在时以 `none` 初始化。runtime 发送不带 `id` 的 JSON-RPC notification `session/cancel` 后等待原 `session/prompt` RPC 返回；取消期间 pending 或迟到 permission / elicitation 由同一取消闸门自动回复 cancelled / decline，不能阻塞 30 秒 bounded drain。prompt 返回 cancelled/interrupted、transport 中断或 deadline 到期后才由统一收尾路径写 terminal snapshot；deadline 到期隔离 session 直接复用，不通过 `provider.pid` 强杀共享 adapter，也不提前伪造“停止成功”。
- Raw frames 按 JSONL 一行一个 frame 的形式由后端分页展示，查询接口以 append-only 行号作为稳定记录时序，默认 `desc`（最新优先、page 0 为最新一页且页内递减），并支持切换 `asc`（最早优先、page 0 为最早一页且页内递增）；排序与过滤变更统一回到 page 0，前端不得只反转当前页，第一页/上一页/下一页文案随排序方向切换；摘要默认单行截断，时间统一显示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；点击该行后以 pretty JSON 或纯文本多行展开，使用克制的暗色代码面板和柔和选中态；短 frame 自然展开不显示内层滚动条，只有超长 frame 才限制高度并显示细滚动条；超长连续字符主动切分换行，内容必须在容器内显示，不能撑出窗口，且展开正文跟随应用设置字体。
- 支持服务端关键词检索，不把全量 `acp.raw.jsonl` 传给前端。
- 支持按 direction（inbound/outbound）和 kind/method 过滤。
- 支持关联到会话流中的消息、tool call 或 permission request。

Raw 视图不承担主交互，不把 ACP 原始 JSON 暴露为普通用户默认体验。切换 Raw 视图或展开单个 frame 时必须保留用户当前阅读位置；用户主动检索、筛选或翻页时只替换当前页结果；Raw 详情内容必须主动换行，禁止横向撑出会话抽屉。

新增用户 prompt、轮询获得新 ACP event 或 agent 回复追加内容且用户仍在底部时，会话列表必须贴底；同一条流式 agent 消息内容变高但事件数量不变时，也必须通过内容尺寸变化监听继续贴底；抽屉关闭不会停止后端 ACP prompt，重新打开同一节点会话时只要持久化 session status 仍是 pending/running/cancelling 等 active 状态，`ACPChatDialog` 必须立即恢复约 1.5 秒一次的 session 轮询并继续合并渲染新增事件；用户上滑加载历史期间必须冻结自动贴底并忽略虚拟列表加载后的临时 at-bottom 误报；历史加载应在用户不在底部且距离顶部约 240px 内预触发，并在顶部显示“— 上滑查看历史信息 —”提示，不要求用户贴到绝对顶部；加载成功后只保持当前阅读锚点，prepend 前后用 scrollHeight 差值补偿 scrollTop，避免滚动条长度变化导致阅读位置按比例回退；不自动下拉补较新页，避免快速上下滚动时两个方向的分页互相抢占滚动位置；处理中提示结束时只移除 composer/乐观气泡状态，不允许 session 刷新导致消息区先跳顶部再回底部。

### 6.9 处理中反馈与计时

- 会话处于 pending / running 且尚无可渲染事件时，composer 内显示“Claude 调起中”，Message List 不显示“暂无 ACP 事件”；如果 ACP session status 尚未写入但当前 runtime node 已是 pending / running / in_progress，也按同一启动状态处理，避免新 run 初始化窗口出现空事件误导。
- 用户点击发送后立即清空 composer 并乐观生成右侧用户气泡；调起 ACP 到真实 `userTextDelta` 写入会话前显示“发送中...”，该提交阶段不参与计时。乐观用户气泡按 task / run / round / node / attempt 维度保留在前端运行态中，关闭并重新打开同一会话抽屉时必须恢复显示并继续锁定 composer，直到后端写入真实用户消息或发送失败。真实用户消息写入后移除乐观气泡，并从该消息时间点进入“处理中...”到首个非用户帧返回；首帧后按最新帧类型切换为“思考中 / 工具调用中 / 回复生成中”。composer action 行与发送按钮保留足够间距，避免按钮贴近输入框。
- 同一会话中连续多次 `继续/Continue` 必须各自保留独立消息行；去重只能基于同一 prompt identity 的重复快照，不能只按文本内容去重。允许出现“历史继续 + 新继续”的两条独立气泡，但禁止把它们拼接成 `继续继续` 或把新回合错误合并进旧回合。新写入的 synthetic `goldBandPrompt` 必须携带 `raw.promptId`；历史数据缺失 promptId 时，前端渲染层只能按事件身份兜底保留多条 Gold Band prompt，不得按 `attemptId + text` 折叠真实多轮继续。
- Composer 只保留两类计时：当前步骤/操作计时，以及 session 累计耗时。当前步骤计时从真实用户消息写入后的首个处理中阶段开始，并随“思考中 / 工具调用中 / 回复生成中”等状态切换；会话进入 completed / failed / cancelled 或等待用户权限决策时停止当前步骤计时。session 累计耗时不按墙钟跨度计算，而是由后端按同一 ACP 会话内每个用户 prompt turn 的实际运行时段累加得到的净处理耗时：每轮从真实用户消息写入开始，到该轮最后一个响应/思考/工具/计划事件结束为止，并扣除所有 `session/request_permission` 从 `permissionRequest(pending)` 到用户选择的等待区间。继续会话时在历史累计值上继续增加，不把两轮之间的用户空闲时间计入总时长。
- 继续 ACP session 时，普通 `session/resume` 不建立 history importer，也不执行 replay quiet-drain；resume 响应后直接进入 `AwaitingTurnStart`，恢复阶段违规到达的历史 content 只保留 raw 并抑制。只有 `session/load` 的 replay phase 才先按 user turn 判断本地回显与外部新增历史：本地 turn 不重复写入，外部 turn 先暂存；load response 后 drain inbound queue 并 finish 暂存历史，prompt 前仍处于 replay 时再次 finish 兜底，随后进入 `AwaitingTurnStart`。ACP 规范要求 load 在响应前完成回放，quiet-drain 是针对已观测到的异步/不合规 adapter 的防御性保护。Provider history 在 `raw.historyPlacement` 保存 `version/afterPromptId/beforePromptId/gapTurnIndex`，`historyItemIndex` 保存组内位置；timeline 的 `seq/timestamp` 只表示审计到达顺序。后端投影与前端 merge 按“本地 prompt + Provider history 锚点”重建展示顺序，分页仍按审计 seq，并以窗口 min/max 审计 seq 生成 cursor。placement-only patch 保留首次 `seq/timestamp/start/end/timing`；旧版文本锚点清理只作用于缺少 placement 的历史，Provider-history patch 与既有本地 item identity 冲突时仍保留原始本地事件。raw/timeline 审计文件不重写。
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
8. `AgentLinkRow` + `RightWorkspaceDock` + `AgentConversationPanel` Agent 分支资源展示。
9. `AcpActivityBatchRow` 活动摘要与独立审计详情加载。
10. `InterventionLayer` / inline permission 与 elicitation card。
11. `PlanBlock` 当前 branch 计划块。
12. `ModeUpdate` / `ConfigUpdate` / `SessionInfo` 状态提示。
13. `RawFrameViewer` 诊断视图。
14. 错误、断线、恢复、seq gap 提示。
15. 快速对话与会话详情共用 `SlashCommandMenu` / `useSlashCommandController`：独立 `/query` 打开，分隔符关闭，选择后插入普通 `/${name} ` 文本；已存在于当前目录的完整命令在分隔符出现后以输入标签投影，底层发送值保持原始文本。
16. ACP 命令目录由 Rust Core 按 Agent + workspace 持久化；每个 Agent 维护独立 Skill 写列表与读列表，Doctor 将 `available_commands_update` 的原生命令和用户级/workspace 级读目录中的 `SKILL.md` 元数据合并，ACP 条目优先并按名称去重。自动/手动 doctor、live update 与 SKILL 同步后刷新；连接层以有界 TTL early-session buffer 解决 `session/new` 返回前命令通知丢失。

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
