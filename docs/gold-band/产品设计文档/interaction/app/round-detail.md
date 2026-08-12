# 任务编排：Round 详情页

## 1. 一句话定义
Round 详情页用于查看某个 run 中某一轮 round 的实际执行图、全局信息流，以及日志、会话、artifact、attachment 的详细内容。

---

## 2. 页面入口
进入方式：
- 在任务工作流页点击某个 round 行
- 从失败、可恢复、正在运行状态直接进入对应 round

页面面包屑：

```text
任务列表 > 任务01 > 工作流列表 > run01 > round01
```

---

`run01` 是执行上下文段，不打开独立 run 详情页；点击“工作流列表”返回任务工作流页。面包屑只有当前页使用常驻金色渐变底线，上级可点击项仅在 hover 或键盘 focus-visible 时临时提亮，不保留点击后的选中态。

## 3. 页面结构

Round 详情页采用统一 Page Header + 两块主工作区 + 按需详情抽屉：

```text
┌──────────────────────────────────────────────┐
│ 统一 Page Header：面包屑 / round 标题 / requirement / 状态 / 操作 / stats │
├──────────────────────────────────────────────┤
│ 上：实际工作图                                │
│ Actual Round Graph                           │
├──────────────────────────────────────────────┤
│ 下：节点相关信息流（仅选中节点后出现）             │
│ Node Progress / Artifact / Attachment         │
└──────────────────────────────────────────────┘

点击节点 / 信息流条目 / 打开日志 -> 右侧 Detail Sheet 按需滑出
```

顶部规则：
- 面包屑、round id、requirement 单行摘要、主要操作与 trigger/maxAttempts/maxRounds/currentNode/result stats 同属统一 Page Header。
- result 状态改为放入顶部信息卡，不再跟随在 round 标题后；requirement 摘要位于标题下方，并与任务工作流页使用同一套无轮廓的低强调行内样式；Header 右侧保留手动刷新按钮，后台每 10 秒静默刷新一次当前页面数据。

推荐比例：
- 实际工作图约占主体高度 45%-55%。
- 未选中节点时，实际工作图占满主体区域。
- 选中节点后，节点相关信息流约占主体高度 45%-55%。
- 详情信息默认不占据固定列宽，需要时以右侧 Sheet 抽屉展示。

两个主工作区均可滚动；上下分栏未来可调整大小。详情抽屉覆盖在右侧，不挤压主工作区。
- 页面应保持高信息密度：Header、图区、Tabs header 与信息流项使用紧凑内边距，优先把垂直空间留给真实工作图和上下文内容，而不是留白。
- 客户端宽度较窄时，工作图卡片 Header 中的“工作图”标题保持单行，选中节点长文本必须在剩余宽度内单行截断；不得挤压标题换行或撑出页面右侧。

---

## 4. 左上：实际工作图

### 4.1 定义
实际工作图展示当前 round 中真实发生的节点执行路径。

对于 AI-DYNAMIC 这类复合节点，Round 详情不再只显示一个外层占位节点；运行态会把内部实际执行过的 bootstrap / worker / workflow-invocation / merge / acceptance 节点直接内联到主图中，并复用普通节点的点击、详情、会话、日志与产物查看逻辑。

当 AI-DYNAMIC 后续连接普通 workflow 节点时，主图中的可视化边从内部 dynamic graph 的出口节点连接到后续普通节点；出口节点是内部图最后一层无下游节点，判定同时包含显式 `dependsOn`、继续会话引用以及 runtime 基于 `chainId/depth` 派生的隐式成功边。当前通常只有一个出口，数据结构保留多个出口同时连接后续节点的能力。

动态内联节点的布局顺序必须落在其外层 AI-DYNAMIC trace 步骤与后续普通 workflow 节点之间，确保出口到后续节点的边作为正向主路径参与布局，而不是被误判为回退边。

动态内联节点在“查看会话”后的继续输入、权限响应、停止会话也必须沿用同一套节点详情交互；前端只传当前选中节点 id / attempt 与其 outer AI-DYNAMIC 定位，后端负责解析到真实 dynamic attempt 目录，不能为 dynamic 节点单独暴露第二套会话 UI 或第二套操作入口。

作者态 AI-DYNAMIC Inspector 的默认权限模式需要在重新打开页面时稳定回显；对于历史工作流 JSON 中遗留的 `permissionMode` 键名，前端读取时应映射回当前统一的 `permission_mode` 字段，后端保存工作流时也应统一输出 `permission_mode`，避免自定义工作流的节点权限配置在 UI 中看起来像未保存。

它不同于任务工作流页的作者态 workflow：
- 任务工作流页表达当前可编辑的 task authoring workflow。
- 实际工作图表达本 run 创建时冻结下来的 `workflow.snapshot.json` 以及本 round 实际执行过、正在执行或等待执行的路径。
- 任务级 workflow 后续修改不会回写已存在 run / round 的实际工作图。

### 4.2 节点展示
节点卡片展示：
- node id
- node type
- status
- outcome
- latest attempt
- artifact 数量
- attachment 数量
- 当前是否运行中

长文本规则：
- 节点标题与 node id / node type 元信息默认优先展示前部内容，尾部截断。
- 鼠标悬浮节点标题或元信息时展示完整全文，不影响单击、双击和右键节点操作。

### 4.3 节点高亮规则
以下节点需要视觉强调：
- 当前运行节点
- 失败节点
- paused / blocked 节点
- 有 artifacts 的节点
- 有 attachments 的节点
- 当前选中节点
- AI-DYNAMIC 内部节点

视觉边界：
- 未选中节点保持白底卡片，不因“当前节点”身份使用蓝色浅底。
- AI-DYNAMIC 内部节点在主图中使用轻量标签或低对比强调色区分其来源，但不额外引入独立交互模型；点击行为与普通节点保持一致。
- 当前运行节点仅用随主题联动的“当前/运行中”徽标表达，徽标可使用 primary/accent 语义色增强识别，但节点卡片仍保持白底，避免与选中态混淆。
- 已结束节点的主视觉优先使用 outcome；例如 `status=completed` 且 `outcome=failure` 时必须显示失败图标/色彩，而不是完成态对勾。
- 只有用户明确选中的节点使用蓝色边框 / 浅蓝底等 primary 卡片级强调。
- 节点卡片的颜色、图标、当前徽标和错误阻塞展示统一来自后端 `runtimeDisplay` 派生结果；前端 GraphView 不再接收单独的 `activeStatus` 覆盖，也不根据 `status/outcome/pauseReason` 重新判断 tone。
- `runtimeDisplay` 派生优先级为：终局失败/kill/invalid 优先于生命周期状态；当前暂停且 `pauseReason=error-blocked` 展示为错误阻塞；运行中展示为 running；普通 paused 展示为 warning；`outcome=success` 展示为 success；`completed + outcome=null` 仅表示生命周期完成，不等同成功。
- AI-DYNAMIC 内联节点与会话式运行时的 session tree 使用同一动态节点事实状态，不能从 ACP attempt 会话快照推断工作流节点状态。

其中 artifacts / attachments 使用可读徽标，例如：

```text
产物:3  表示 3 个 artifacts
附件:2  表示 2 个 attachments
```

### 4.4 节点交互
- 单击节点：选中节点，左下信息流追加节点相关 artifacts / attachments；如果详情抽屉已打开或已固定，抽屉内容随 selection 更新。
- 双击节点：右侧详情抽屉打开节点摘要。
- 右键节点：先切换为当前选中节点；如果非固定详情抽屉正在打开，先用快速收起动画关闭抽屉，再打开该节点上下文菜单，避免抽屉瞬间消失造成跳变。

节点右键菜单建议：
- 查看节点详情
- 查看日志
- 查看会话
- 复制 node id
- 从该节点重试

---

## 5. 左下：全局信息流

### 5.1 默认信息架构
左下不再保留抽象的 `上下文` Tab，也不在 round 初始态展示单独的 `编排事件` 面板；round 级上下文放到顶部 Header/指标区附近：
- requirement 摘要与“查看完整需求”入口放在标题下方；完整需求抽屉标题右侧提供复制 icon，一键复制完整需求。
- round result 状态放入顶部 stats，trigger、maxAttempts、maxRounds、currentNode 与其并列展示；currentNode 卡片默认单行截断，鼠标悬浮显示完整全文。
- Header 操作区使用“打开日志”按需打开事件/日志详情抽屉，不用一整块信息流承载少量运行事件。

### 5.2 选中 node 时
如果当前选中对象是 node，左下仅按需出现 node 相关信息：
- `产物` / `附件`：仅在选中 node 且存在对应内容时展示。

节点可读状态摘要、attempt、日志、会话引用等不再放入左下 `上下文` 或 `节点进度` Tab，而是通过工作图双击节点、右键菜单或详情抽屉查看。

### 5.3 信息流交互
- 点击信息流任意条目：右侧详情抽屉打开对应详情；selection 使用通用 `contextNodeId` 保留当前 node 上下文，不能因为打开 round、requirement、event 或 log 等全局详情而退回 round 上下文。
- 点击 event：右侧详情抽屉打开 event JSON / 格式化说明；event 自身可携带 node id，否则沿用 `contextNodeId`。
- 点击 artifact：右侧详情抽屉打开 artifact 内容。
- 点击 attachment：右侧详情抽屉打开 attachment 内容。
- 点击 artifact / attachment / worker-ref 后，左下仍保留其所属 node 的信息流，避免用户丢失节点上下文。

---

## 6. 右侧：详情抽屉

### 6.1 定义
右侧详情抽屉是按需查看区，不承担主导航，也不默认占据工作台固定宽度。

它用于展示用户从实际工作图或全局信息流中选择的具体对象。

可展示对象包括：
- 日志详情
- event 详情
- node 摘要
- ACP Dialog / Chat 会话
- provider 会话引用
- artifact 内容
- attachment 内容
- validation 详情

### 6.2 ACP 会话 Tab

节点详情抽屉的“查看会话”展示 ACP Dialog / Chat UI，而不是 `progress.events.jsonl` / `raw.stream.jsonl` 两个 legacy 日志 Tab。

会话区包含：
- Session header：压缩为两行，只展示 provider/adapter 显示名、国际化状态、系统提示按钮、原始帧小按钮和 provider session id；provider session id 从当前 attempt 的 `worker-ref.json` 派生，`acp.session.json` 只作为运行态快照；不展示 cwd、恢复标记、事件数或错误数。系统提示按钮优先读取本 attempt ACP raw frame 中 `session/new._meta.systemPrompt.append`、`session/resume._meta.systemPrompt.append` 或 `session/load._meta.systemPrompt.append`，并用弹窗展示当前 attempt 实际追加的 system prompt；若会话已进入运行态但 conversation 聚合快照尚未补齐，前端仍应优先使用当前 attempt 详情返回的 `systemPromptAppend` 保持按钮可点击，不等待节点结束后才恢复。只有 raw frame 与当前 session 详情都没有追加内容时按钮才禁用。系统提示正文复用 AtomEditor/CodeMirror 的 Markdown 查看器，以只读策略固定其编辑能力；复制源码与源码/实时预览切换沿用同一个查看器右上角工具栏，切换 attempt 时沿用同一查看模式，不把该偏好写入 ACP session 数据。桌面断点下弹窗宽度显式覆盖 shadcn Dialog 默认的 `sm:max-w-lg`，使用 `sm:max-w-5xl` 提升长 prompt 的横向阅读空间；窄窗口仍遵循 Dialog 的视口安全边距。渲染与原文模式都必须跟随应用设置字体，原文模式不使用浏览器默认等宽字体。
- Message list：基于 prompt-kit `ChatContainer` / `Message` 展示 agent/user 文本气泡；agent 文本气泡支持 Markdown（GFM）渲染，用户 prompt 始终保持纯文本原样展示，不解析 Markdown；Markdown 标题不使用文章页大字号层级，一级标题只做加粗和轻量标识，二/三级标题保持接近正文的紧凑字重，避免会话流高度失控；代码块、表格、长链接和长中英文内容必须限制在气泡宽度内滚动或主动换行；初始 prompt 和后续继续输入都作为右侧用户消息出现；窗口内以聚合后的 timeline item 作为最小视觉单元，一条 agent/user 消息不能拆成多个可见消息块；新建 ACP session 时 system prompt 通过 `_meta.systemPrompt.append` 注入，继续输入发送到 ACP 时只包含用户 prompt，不追加内部续聊说明，也不展示 system prompt。
- Thought：基于 prompt-kit `ChainOfThought` 展示合并后的 thought 折叠块，标题展示思考耗时（`xx 秒` / `xxs`），不展示字符数；summary 与详情必须保持在同一个 thought 卡片内，展开/收起不拆成两条消息。
- Tool call：基于 prompt-kit `Tool` 展示按 `toolCallId` 原地更新的工具卡片，默认紧凑显示工具名、状态与图标，展开后展示路径、查询等参数和输出摘要；summary 与详情必须保持在同一个 tool 卡片内，展开/收起不拆成两条消息；live push 期间工具卡默认保持收起，只更新标题、状态和关键参数，用户可手动展开查看实时详情。
- Agent / 子 Agent：Gold Band 在 ACP 接入边界把标准或 provider metadata 归一化为内部 `AgentTranscriptRelation`，并由 `sessionId + launchToolCallId` 生成稳定 `AgentExecutionId`。当前 Claude `_meta.claudeCode.subagent/toolName/parentToolUseId` 只在该边界转换；原始字段仅保留于 `acp.raw.jsonl`，前端、语义分页和分支持久化只消费 `_meta.goldBandConversation`。根 timeline 只保留顶层 Agent launch 链接，每个 Agent transcript 写入 `agents/<AgentExecutionId>/timeline.jsonl`，父子关系写入扁平 `acp.agents.jsonl`，不按当前窗口或 seq 邻近关系重建递归树。父分支使用轻量 `AgentLinkRow`，点击后在通用右侧工作区打开只读 Agent Tab；嵌套 Agent 在父 Agent 会话中继续打开新 Tab，父分支不挂载、复制或折叠子 transcript。
- Plan / Todo：plan 是所属 conversation branch 的最新生命周期快照，不作为重复消息卡片渲染。规范化和分盘写入阶段已经确定 branch ID，根会话与每个 Agent 查询都只投影本 branch 最新 plan 到 composer 上方的紧凑 Todo 列表；不再通过内容血缘、事件邻近或前端文本去重猜归属。`available_commands_update`、`usage_update`、session/mode/config update 等状态帧不作为聊天消息展示。
- 活动批次：ACP 消息流按 `ActivityBatch → TextMessage → ActivityBatch` 分段。正式文字始终直接流式展示；文字出现前的 thought、普通 tool call、失败 tool call 与 permission request 都属于同一 Activity，只有正式文字、Agent 层级、attempt 边界或上下文压缩等独立生命周期事件才能结束当前活动段。展开/收起只控制详情可见性，不能成为数据分段条件；流式追加必须继续进入同一稳定 Activity key，第一段文字到达时才冻结此前批次，文字后再次出现活动则创建新批次。摘要只能投影 ACP 明确返回的 `toolName/title/status/rawInput/locations/parentToolCallId`、ID、时间和客观计数，不解释命令意图：例如展示“正在调用 PowerShell · npm run web:test”，禁止猜成“正在运行测试”。Grep/Glob 只计搜索操作，不能计为读文件；Shell 命令文本不参与文件读写推断。成功 Read 按唯一规范化路径统计读取文件，成功 Write/Edit/ApplyPatch 按结构化路径统计写入文件。
- 活动审计与性能：活动摘要运行中使用 1.8 秒低强度浅→深→浅呼吸效果，归档后立即静止，并遵守系统 reduced-motion 设置。Activity 折叠时不读取、不构造、不解析普通工具、失败工具和 thought 详情；首次展开只查询最近 40 条压缩审计项，存在更早记录时在 Activity 内部继续“显示更早活动”。工具 raw input/output 只在具体工具再次展开后通过单条详情接口读取。已决 permission 不进入审计详情；pending permission 由独立 intervention 实时展示。根与 Agent 继续使用原生滚动、有限语义块窗口、真实 DOM 高度和锚点补偿，不引入动态高度虚拟列表；非激活 Agent Tab 不挂载 `ConversationViewport`。
- Agent 生命周期：Agent launch 与 Agent execution 是两个领域。后台 launch tool 的 `completed` 或结构化启动回执只表示 accepted，分支只有 synthetic Prompt 时保持 queued；出现所属工具、思考或正式文字后转 running，存在 pending permission/elicitation 时转 waiting_permission。只有前台 Agent 的正式工具结果、分支规范结果事件或根 session 的正常终态可形成 completed 证据；不得按回执文字猜测结果。根 stop/cancel/failure 只把仍为 queued/running/waiting 的 Agent 收敛为 interrupted，已有正式完成证据的 Agent 保持 completed。继续运行创建新的 attempt/execution，不把新事件写回旧 branch。
- Permission：permission request 在规范化阶段绑定 owning branch ID。根会话只显示对应 Agent link 的 attention，不把 Agent 权限卡平铺进根消息流；打开 owning Agent Tab 后由同一个 `InterventionLayer` 展示轻量可操作卡，并使用根 ACP session locator 与规范 request ID 提交决策。待决权限向所有祖先 Agent link/Tab 投影 attention。决策成功后当前 Agent Tab重新补拉同一 branch，不接受命令返回的根 session VM 覆盖当前视口；已决权限从 intervention 与 Activity 审计中移除。事件自身携带的版本化 timing 可立即驱动等待态，历史无版本 timing 不能覆盖 live 状态。
- Composer：基于 prompt-kit `PromptInput` 用于继续 ACP 会话；点击发送后立即清空并乐观展示右侧用户气泡；会话处于 pending/running/cancelling 等 active 状态时显示“停止”按钮并禁用普通发送。待发送队列属于同一个底部输入 surface：队列存在时必须与 composer 上边缘紧贴，composer 去除上圆角并与队列共享连续边界；无附件时不得渲染空附件占位或额外 margin，避免队列和输入框被分隔成两个独立面板。停止命令先把 attempt / run / round 收敛为 `paused + process_interrupted`，并返回持久化 session snapshot；若该 snapshot 已是 `cancelled` 等终态，前端必须立即解除本地 cancelling、遮罩和输入锁，即使并发到达的 lifecycle 仍短暂报告 `cancelling`。停止后的 session 补拉只做后台校准，不得阻塞停止 UI 收敛。运行中的 ACP runtime 仍通过无 `id` 的 `session/cancel` notification 请求 provider 优雅退出，超时后走强制终止兜底；`ExitPlanMode` 等 plan intervention 权限例外允许输入自然语言反馈并排队发送；permission pending 时 composer 只显示等待决策状态。
- Raw frames：作为会话画布的切换视图，普通会话刷新只统计 `acp.raw.jsonl` 行数；Raw frames 按 JSONL 一行一个 frame 由后端分页读取，排序属于后端查询契约，默认使用 `desc`（最新优先），支持切换 `asc`（最早优先），切换排序、关键词、direction（inbound/outbound）或 kind/method 过滤后统一回到该顺序的第 0 页，禁止仅在前端反转当前页。append-only JSONL 的行号代表稳定记录时序，同一展示时间戳下以行号决定先后；`desc` 时页内行号递减、后续页更早，`asc` 时页内行号递增、后续页更新，第一页与前后页按钮文案必须随排序方向联动；原始帧不追加到聊天消息流末尾；摘要行必须单行截断，时间统一显示为本地系统时区 `YYYY-MM-DD HH:MM:SS`，展开详情使用克制暗色代码面板和柔和选中态，且详情正文必须跟随应用设置字体；短 frame 自然展开不显示内层滚动条，只有超长 frame 才限制高度并显示细滚动条；内容必须在抽屉宽度内主动换行，不能撑出窗口。
- Conversation branch page：`get_acp_session` 通过 `branchId` 对根或 Agent 返回同一个 `AcpSessionVm/eventPage`。分页单位是用户正式消息、Assistant 正式消息、Agent link、连续工具/思考形成的 Activity、待决交互和明确生命周期边界等稳定语义块；不按 raw frame、tool/thought event 数量、DOM 高度或折叠状态计数。一个 Activity 和一个 Agent link 在父分支始终各算一个块，Agent 内数百条事件不会改变父分支 `hasOlder`。`oldestCursor/newestCursor` 只描述所选 branch 的语义窗口；不存在 Agent 专用 page 字段，也不再向父窗口注入 launch anchors。Activity 详情使用独立 `earlierCursor`，展开和“显示更早活动”都不改变 branch cursor。
- Scroll list：根与 Agent 消息列表复用 prompt-kit 原生滚动容器和同一个有限滑动窗口，默认保留至少三个语义页。向上/向下换页前捕获真实 `[data-acp-item-key]` DOM 锚点，合并与裁剪后按同一 item 的 top 差补偿 `scrollTop`；不使用动态高度虚拟列表。每个 branch 的事件窗口、anchor、scrollTop、贴底和 `hasOlder/hasNewer` 保存在有限 LRU；切换右侧 Tab、Dock 与 Sheet 时按 branch key 恢复，非激活 Tab 不长期隐藏挂载 DOM。
- Scroll：只有当前 branch 处于真实底部且 `hasNewer=false` 时，正式文字、pending intervention 或 Activity 高度变化才允许维持贴底。用户阅读历史、Activity 展开/收起、工具详情延迟返回和分页 prepend 都必须保持当前真实 DOM 锚点；授权卡挂载不得破坏贴底锁。向上进入预取区时只按 branch `oldestCursor` 加载更早语义页，向下浏览旧窗口时按 `newestCursor` 补拉新页；两方向共用单一分页状态，不并发争抢窗口。超长用户 prompt 和 Assistant Markdown 保持为单个语义项自然滚动，不按高度拆页。
- Pending interaction：permission / elicitation 的交互卡属于当前 session 生命周期状态，不能仅凭有限历史窗口中的 `pending` 事件推断。完整 session 返回的 `pendingPermissions` 等权威状态优先；仅当 session 仍为 active、当前窗口已经包含最新事件端（`hasNewer=false`），且 session timing 的 `waitReason` 与交互类型一致时，才允许从事件窗口恢复 pending 卡。已完成/失败/取消会话或仍有较新分页的历史窗口中，旧 `elicitationRequest(pending)` 只能作为 AskUserQuestion 工具历史的一部分展示，不得重新出现可操作提问卡。
- Processing：pending/running 且尚无可展示事件时在 composer 内显示“Claude 调起中”；用户点击发送后，调起 ACP 到真实 `userTextDelta` 写入前显示“发送中”动效且不计时；真实用户消息写入后到首个非用户帧之间切换为“处理中”动效并开始当前步骤计时，同时移除右侧乐观用户气泡；首帧后按最新事件类型展示“思考中 / 工具调用中 / 回复生成中”；composer 只保留两类计时：当前步骤/操作计时，以及 session 累计耗时；session 累计耗时按同一 ACP 会话内各个 prompt turn 的实际运行时段累加，继续会话时只重置当前步骤计时，不重置历史累计值，也不把两轮之间的用户空闲时间计入总时长；关闭抽屉不会中断后端 ACP prompt，重新打开同一节点会话时若持久化 session 仍为 active，前端必须立即订阅 live push 并继续渲染新增事件；等待 `session/request_permission` 用户决策时停止当前步骤计时并隐藏处理中状态，session 累计耗时也必须扣除 `permissionRequest(pending)` 到用户选择的等待区间；该规则覆盖普通工具授权以及 `ExitPlanMode` / keep planning 等 plan 决策，pending 期间刷新不得让累计耗时继续增长；普通继续优先使用不回放历史的 `session/resume`，只有跨端完整历史同步或 load-only Agent 才使用 `session/load`；load 回放的历史消息不重复追加到 UI 事件流，已有聊天历史仍按原顺序显示；消息流不插入独立处理中卡片。
- ACP session 尚未建立时，会话壳的 loading 只表示外层 runtime 仍 active 且 session 仍可能创建。只要 canonical run lifecycle 已进入 `runtime-abnormal`、`error-blocked` 或失败终态，会话壳必须立即结束 loading 并展示 runtime diagnostic。Workflow/AUTO 的 `process-interrupted` 可展示初始化中断；Direct 停止没有 Runtime continue，必须保留普通 composer，不能因停止时机不同要求重跑。非错误 pause 不得伪装成初始化错误。ACP session 文件、session id 和 timeline 都不是决定 run 是否仍运行的事实来源。
- Avatar：agent 文本使用左侧机器人头像；thought、tool call、plan 与处理中状态不展示头像，但保留与工具卡一致的横向位置，用户 prompt 仍使用右侧用户头像。
- Tool header：工具调用标题行左对齐，显著展示工具操作名，次一级展示路径、pattern 或 query，例如 `Glob .claude/**/*`、`Read xxx.js`；状态徽标和展开按钮保留在右侧。

### 6.3 默认状态
进入 round 详情页时，详情抽屉默认关闭；用户点击“打开详情”时展示当前 selection 的详情。当前 selection 为 round 时展示 round summary：
- round id
- run id
- status
- outcome
- trigger
- maxAttempts / maxRounds
- startedAt（本地系统时区 `YYYY-MM-DD HH:MM:SS`）
- finishedAt（如有，使用本地系统时区 `YYYY-MM-DD HH:MM:SS`）
- 当前节点
- 最近错误摘要

抽屉规则：
- 使用 shadcn/ui Sheet 右侧滑出。
- 非模态、无遮罩，不阻塞用户继续操作工作图和信息流。
- 支持固定详情：固定后点击图节点或信息流条目不会关闭抽屉，只切换内容；同时抽屉从覆盖式 Sheet 切换为右侧占位面板，主工作区自动收窄让位。
- 固定态面板不继续复用 Sheet Portal / Dialog Title 结构，避免非模态 Sheet 卸载过程残留 portal、focus guard 或全屏遮罩状态导致主界面变黑；固定后主工作区应自适应收窄，不出现中缝滚动条或横向滚动条。
- 关闭按钮、Escape 或未固定时点击非交互空白可收回抽屉。

### 6.3 查看日志
点击左下日志项后，详情抽屉展示：
- 日志时间
- 来源
- 级别
- 内容
- 关联 run / round / node / attempt

### 6.4 查看会话
右键节点选择“查看会话”后，详情抽屉展示 ACP 统一后的原始 agent 过程：
- provider / ACP adapter
- worker ref / ACP session id
- attempt id
- 会话状态与 stop reason
- agent message 文本流
- 右侧用户 prompt 气泡（包含初始 prompt 与后续继续输入）
- thought / reasoning 折叠区，标题展示思考耗时
- prompt-kit Tool 风格的 tool call / tool call update 卡片
- Agent 工具调用对应的子 Agent transcript 可展开/收起分组
- plan entries
- permission request
- terminal / file 操作与输出
- ACP raw frame / transcript 查看入口
- 可打开原始 provider CLI 会话的 handoff 操作

Gold Band 默认只查看和 handoff，不在详情抽屉直接做聊天式接管；会话详情基于 ACP session events，不再基于自研 `progress.events.jsonl`。

### 6.5 查看 artifact / attachment
点击 artifact 或 attachment 后，详情抽屉展示：
- 名称
- 类型
- 来源 node
- 来源 attempt
- 更新时间
- validation 状态
- 内容预览

artifact / attachment 从节点详情抽屉内进入时属于节点详情的二级查看层；点击“返回节点”或关闭当前产物详情时，应回到原节点详情抽屉和当前节点上下文，不能直接退回 Round 主页。

内容预览规则：
- JSON：格式化树或 pretty print
- Markdown：阅读视图
- 文本：plain text
- 图片：图片预览
- 不支持的二进制：展示 metadata 与打开文件位置

---

## 7. 返回与选择规则
- 点击面包屑返回上级页面。
- Esc 优先关闭右键菜单或未固定的详情抽屉。
- 详情抽屉固定时，Esc 不应破坏固定状态；用户可通过关闭按钮显式收回。
- 没有浮层时，Esc 可从具体对象详情返回 round summary。
- 再次 Esc 可清空节点选择，回到 round 选中状态。
- 不通过命令输入返回。

---

## 8. Tauri 2.x MVP 对应实现

MVP 中 Round 详情页由 Tauri command `get_round_detail` 提供 view model，前端页面位于 `web/src/pages/RoundDetailPage.tsx`。

当前实现规则：
- 左上实际工作图来自当前 round 中真实落盘的 node/attempt canonical state，并以真实节点-边画布展示；节点为 UML 风格卡片，边以箭头和 label 表达本轮路径关系。
- 实际工作图支持缩放、平移、节点选中、双击打开节点摘要，以及右键节点菜单；右键菜单保留查看节点详情、查看会话、复制 node id、从该节点重试等入口。
- 页面布局对齐桌面工作台：顶部使用统一 Page Header 承载面包屑、round id、requirement 摘要、status/outcome、trigger、maxAttempts / maxRounds、当前节点/结束节点与直接操作；终态 round 使用“结束节点”文案，避免暗示节点仍在运行；主体默认展示实际工作图，详情以右侧 Sheet 抽屉按需展示。
- 左下信息流不再展示“上下文”Tab，也不在 round 初始态展示 run events；选中 node 后仅按需展示 artifact 和 attachment Tab，节点日志通过工作图右键菜单进入详情抽屉。
- 右侧详情抽屉展示当前选择对象，默认关闭；点击“打开详情”、双击节点、右键查看节点详情/会话或点击信息流条目时打开。
- requirement、round summary、event、log、artifact、attachment、worker-ref 都可进入详情抽屉查看完整内容；artifact 在 UI selection 中使用逻辑名（如 `验收输出产物`），落盘文件仍为 `验收输出产物.json`，后端读取兼容两种形式。
- 选择 artifact / attachment / worker-ref 时通过独立 Tauri command 或 round selection 加载内容。
- 前端页面状态保持 camelCase，调用 `get_round_detail` 时将嵌套 `selection` 字段转换为 Rust `RoundSelectionInput` 所需的 snake_case，避免节点、artifact、attachment、worker-ref 选择反序列化失败。
- status/outcome 只来自 canonical state；ACP session events、日志和 raw frame 仅作为会话观测内容；ACP session status 不作为节点主状态展示，运行态轮询只依据结构化 run/round/node 状态。工作图节点主视觉在运行/待处理/暂停等过程态使用 status，在终态优先使用 outcome。artifact 归档只从最近有限个 assistant 文本输出段中查找可解析 JSON，不无限扫描历史会话详情。
- 2026-05-03 起页面使用 Tailwind CSS v4 + shadcn/ui Card、Tabs、Button、Badge、Dropdown Menu、Scroll Area 等现成组件重构；左上实际工作图、左下信息流、右侧 Detail Viewer 三栏工作台和 selection 映射保持不变。
- 2026-05-06 起右侧 Detail Viewer 从常驻固定列改为 shadcn/ui Sheet 详情抽屉；主工作区默认由实际工作图和信息流占满，抽屉支持非模态查看、固定、关闭和随 selection 切换内容。
- 2026-05-05 起左上实际工作图优先来自 `round.json.trace`，只展示该 round 真实进入过的 node/attempt 序列；旧数据没有 trace 时按 node state 的 startedAt/attemptId 推断 fallback 路径，不再把 workflow 全景边按出现节点集合直接过滤后展示。
- 2026-05-05 起实际工作图与任务工作流页 GraphView 使用一致的节点卡片、边、背景和缩放控件样式；当前节点、有 artifacts 的节点、有 attachments 的节点和选中节点必须有独立高亮。实际工作图位于 Round 工作台左上区域时应限制 fitView 最大缩放，避免少量节点被放大成主视觉，图卡高度应与下方信息流形成均衡比例。
- 2026-05-05 起左下区域按当前选中节点动态展示 Artifact / Attachment Tabs；未选中节点或当前节点无产物/附件时不展示底部信息区。点击日志、节点会话、artifact、attachment 只更新右侧详情抽屉，左下保持当前 node 上下文，不再采用“round 替换成 node”的模式。
- 2026-05-05 验收修正：Round 详情工作台在小窗口下必须允许主体滚动；实际工作图和左下信息流各自保留最小可读高度，并按客户端高度收缩，不能被父级 `overflow-hidden` 裁切成只显示一部分；未展示底部信息区时，工作图卡片应填满 Header 下方剩余工作区，由外层统一 padding 保持工作图到上方内容和客户端底部的距离一致。顶部 header 的 run/trigger 文案必须保持单行截断，不允许被指标区挤成竖排。
- 2026-05-05 验收修正：左下信息流的 Tabs 与上下文说明必须使用紧凑单行布局，日志项使用低内边距高信息密度卡片，优先把垂直空间留给真实日志内容。
- 2026-05-07 验收修正：左下信息流、任务工作流运行记录、Workspace 最近列表、Settings 表单卡片以及遗留 Task/Run 详情卡片必须移除 shadcn/ui Card 默认 `gap-6` 与 border header 默认大底部 padding 的叠加影响；Tabs header 下方不得保留空的 TabsContent 占位，内容卡片应紧贴 header 后以小内边距开始。
- 2026-05-07 起 Round header、选中节点提示与实际工作图节点说明都必须优先使用 workflow snapshot 中的节点说明，并同时保留原始 node id；当节点说明缺失时也要展示节点类型，避免 `run-tests` 等内部 id 单独出现导致用户无法理解当前阶段。
- 2026-05-07 起实际工作图打开后必须在画布可视区域内默认完整展示；GraphView 使用受控 viewport 按节点 bounds 和容器尺寸计算初始平移/缩放，实际工作图在大画布中采用居中视觉锚点，让节点组靠近页面视觉中心；实际工作图容器不得设置超过父内容区的固定最小高度，避免执行路径图底部圆角和节点卡片被父级 `overflow-hidden` 裁切。
- 2026-05-08 起 Round 详情页使用统一 Page Header：面包屑、round 标题、状态 badge、直接操作和低对比 stats 使用与任务列表/工作流页一致的顶部表面；stats 位于下一行，避免挤压标题与 run/trigger 文案。
- 2026-05-08 验收修正：Round 详情页继续收紧 Header、图区容器、Tabs header 和信息流列表的纵向间距；默认工作台高度与上下分区最小值同步下调，避免少量节点或少量上下文时首屏出现大块空白。
- 2026-05-08 起移除左下“上下文”Tab：requirement 摘要上移到 Header，round 状态、触发、修复循环和当前节点保留在顶部指标区；节点详情改由工作图双击、右键菜单或详情抽屉按需查看。
- 2026-05-08 起 round 初始态不再展示单独的“编排事件”面板，Header 中“打开详情”替换为“打开日志”，按需打开事件/日志详情抽屉；底部只在选中节点后展示产物、附件，节点日志由右键菜单“查看日志”打开。
- 2026-05-08 起实际工作图节点不再用整卡背景/边框表达状态，普通节点统一卡片底色，完成/失败/运行中等状态优先用节点左侧圆形状态标记表达，不再重复展示“已完成”等文字状态标签；产物/附件使用“产物:1”“附件:1”可读徽标。工作图 header 不再保留颜色图例；当前节点使用“当前”pill，用户选中节点使用独立的浅金底、暖金细描边与轻微 glow，避免与状态色混淆；右键非选中节点时自动切换 selection，非固定详情抽屉用约 150ms 快速收起过渡后再展示菜单；日志详情中的长 JSON、路径和 prompt 文本必须在抽屉宽度内换行，不允许撑宽详情容器。
- 2026-05-12 起 Round 节点会话详情切换为 ACP-first 方向：会话 Tab 展示 ACP session events、tool calls、thought、plan、permission、terminal/file 与 raw frame，不再以 `progress.events.jsonl` / `raw.stream.jsonl` 二选一作为主信息架构；保留打开原始 provider CLI 的 handoff。
- 2026-05-12 验收修正：从节点详情抽屉打开 artifact / attachment 内容后，“返回节点”和关闭当前产物详情必须恢复原节点详情抽屉，保留当前 node selection，不允许直接关闭到 Round 主页。
- 2026-05-12 验收修正：节点详情抽屉头部不重复展示长节点说明，只保留紧凑“查看详情 / 查看会话”切换；ACP 会话头部压缩为名称、Raw frames 小按钮和 provider session id 两行，不展示 ACP session status 以免与节点 canonical status 混淆；Raw frames 摘要和展开内容必须受抽屉宽度约束，长 JSON 不允许横向撑出窗口。
- 2026-05-12 验收修正：ACP 会话抽屉禁止因 Raw frames 切换、raw frame 展开或 tool call 展开自动滑到底部；点击发送到 `session/prompt` 请求完成前显示“发送中”，消息发出后等待 ACP 响应时切换为“处理中”，右侧乐观用户气泡同步切换状态；pending/running 空事件态与运行过程的处理中动效、当前步骤计时统一放在 composer 内，总耗时按每轮请求-响应耗时累加并常驻展示，不作为消息流卡片；permission request 使用轻量 inline action bar；thought/tool/plan 状态不展示头像但保留工具卡横向位置，工具卡高度更紧凑；工具标题左对齐显示“操作名 + 次级参数”。
- 2026-08-02 起会话中的 Agent 不再以内嵌折叠 transcript 展示，而是作为轻量链接在通用右侧工作区打开只读分支会话。父分支只展示直属 Agent；嵌套 Agent 在直接父 Agent 会话中逐级出现。Plan 必须携带内部 `branch | unscoped` 归属，存在 Agent execution 时根会话不展示无法确认归属的 session-wide Todo，也不根据文字猜测归属。

---

## 9. 一句话总结

> Round 详情页上方看“这一轮实际怎么跑”，右侧会话详情看“原始 agent 过程中发生了什么”，并保留跳转外部 CLI 的 handoff。
