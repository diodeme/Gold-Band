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
- 面包屑、round id、requirement 单行摘要、主要操作与 trigger/repairLoopsUsed/currentNode/result stats 同属统一 Page Header。
- result 状态改为放入顶部信息卡，不再跟随在 round 标题后；requirement 摘要位于标题下方，并与任务工作流页使用同一套无轮廓的低强调行内样式；Header 右侧保留手动刷新按钮，后台每 10 秒静默刷新一次当前页面数据。

推荐比例：
- 实际工作图约占主体高度 45%-55%。
- 未选中节点时，实际工作图占满主体区域。
- 选中节点后，节点相关信息流约占主体高度 45%-55%。
- 详情信息默认不占据固定列宽，需要时以右侧 Sheet 抽屉展示。

两个主工作区均可滚动；上下分栏未来可调整大小。详情抽屉覆盖在右侧，不挤压主工作区。
- 页面应保持高信息密度：Header、图区、Tabs header 与信息流项使用紧凑内边距，优先把垂直空间留给真实工作图和上下文内容，而不是留白。

---

## 4. 左上：实际工作图

### 4.1 定义
实际工作图展示当前 round 中真实发生的节点执行路径。

它不同于任务工作流页顶部的原始 workflow 图：
- 原始 workflow 图表达设计全貌。
- 实际工作图表达本 round 实际执行过、正在执行或等待执行的路径。

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

视觉边界：
- 未选中节点保持白底卡片，不因“当前节点”身份使用蓝色浅底。
- 当前节点仅用随主题联动的“当前”徽标表达，徽标可使用 primary/accent 语义色增强识别，但节点卡片仍保持白底，避免与选中态混淆。
- 只有用户明确选中的节点使用蓝色边框 / 浅蓝底等 primary 卡片级强调。

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
- round result 状态放入顶部 stats，trigger、repairLoopsUsed、currentNode 与其并列展示；currentNode 卡片默认单行截断，鼠标悬浮显示完整全文。
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
- Session header：压缩为两行，只展示 provider/adapter 显示名、国际化状态、原始帧小按钮和 provider session id；不展示 cwd、恢复标记、事件数或错误数。
- Message list：基于 prompt-kit `ChatContainer` / `Message` 展示 agent/user 文本气泡；初始 prompt 和后续继续输入都作为右侧用户消息出现，只展示用户 prompt，不展示 system prompt。
- Thought：基于 prompt-kit `ChainOfThought` 展示合并后的 thought 折叠块，标题展示思考耗时（`xx 秒` / `xxs`），不展示字符数。
- Tool call：基于 prompt-kit `Tool` 展示按 `toolCallId` 原地更新的工具卡片，默认紧凑显示工具名、状态与图标，展开后展示路径、查询等参数和输出摘要。
- Plan：使用独立 plan block 展示计划条目；`available_commands_update`、`usage_update`、session/mode/config update 等状态帧不作为聊天消息展示。
- Permission：permission request 使用独立 approval card；用户点击允许或拒绝后立即退出 pending UI，失败时恢复并提示。
- Composer：基于 prompt-kit `PromptInput` 用于继续 ACP 会话；点击发送后立即清空并乐观展示右侧用户气泡，permission pending 时禁用普通输入。
- Raw frames：作为会话画布的切换视图，按需读取受限大小的 `acp.raw.jsonl`，普通会话刷新只统计行数；Raw frames 按 jsonl 一行一个 frame 展示状态帧和原始 JSON，不追加到聊天消息流末尾；摘要行必须单行截断，展开详情使用克制暗色代码面板和柔和选中态，短 frame 自然展开不显示内层滚动条，只有超长 frame 才限制高度并显示细滚动条；内容必须在抽屉宽度内主动换行，不能撑出窗口。
- Scroll：会话只在新增 ACP event 或发送用户 prompt 时按“用户本来接近底部”规则显式贴底；切换 Raw frames、展开 raw frame 或展开 tool call 只保留当前阅读位置，不触发自动滑到底部。
- Processing：pending/running 且尚无可展示事件时显示“Claude 调起中”；发送 prompt 到首帧之间显示处理中动效，首帧后按最新事件类型展示“思考中 / 工具调用中 / 回复生成中”，同时展示当前步骤耗时和总耗时。
- Avatar：agent 文本、thought、tool call、plan 与处理中状态统一使用左侧机器人头像，用户 prompt 仍使用右侧用户头像。

### 6.3 默认状态
进入 round 详情页时，详情抽屉默认关闭；用户点击“打开详情”时展示当前 selection 的详情。当前 selection 为 round 时展示 round summary：
- round id
- run id
- status
- outcome
- trigger
- repairLoopsUsed
- startedAt
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
- 页面布局对齐桌面工作台：顶部使用统一 Page Header 承载面包屑、round id、requirement 摘要、status/outcome、trigger、repairLoopsUsed、currentNode 与直接操作；主体默认展示实际工作图，详情以右侧 Sheet 抽屉按需展示。
- 左下信息流不再展示“上下文”Tab，也不在 round 初始态展示 run events；选中 node 后仅按需展示 artifact 和 attachment Tab，节点日志通过工作图右键菜单进入详情抽屉。
- 右侧详情抽屉展示当前选择对象，默认关闭；点击“打开详情”、双击节点、右键查看节点详情/会话或点击信息流条目时打开。
- requirement、round summary、event、log、artifact、attachment、worker-ref 都可进入详情抽屉查看完整内容；artifact 在 UI selection 中使用逻辑名（如 `verify-result`），落盘文件仍为 `verify-result.json`，后端读取兼容两种形式。
- 选择 artifact / attachment / worker-ref 时通过独立 Tauri command 或 round selection 加载内容。
- 前端页面状态保持 camelCase，调用 `get_round_detail` 时将嵌套 `selection` 字段转换为 Rust `RoundSelectionInput` 所需的 snake_case，避免节点、artifact、attachment、worker-ref 选择反序列化失败。
- status/outcome 只来自 canonical state；ACP session events、日志和 raw frame 仅作为会话观测内容；运行态轮询只依据结构化 run/round/node 状态，不扫描会话详情文本或历史 events。
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
- 2026-05-12 验收修正：节点详情抽屉头部不重复展示长节点说明，只保留紧凑“查看详情 / 查看会话”切换；ACP 会话头部压缩为名称、原始状态、Raw frames 小按钮和 provider session id 两行；Raw frames 摘要和展开内容必须受抽屉宽度约束，长 JSON 不允许横向撑出窗口。
- 2026-05-12 验收修正：ACP 会话抽屉禁止因 Raw frames 切换、raw frame 展开或 tool call 展开自动滑到底部；pending/running 空事件态展示“Claude 调起中”，运行过程中持续展示处理中动效、当前步骤计时和总耗时；thought/tool/plan/处理中状态与 agent 文本统一展示机器人头像。

---

## 9. 一句话总结

> Round 详情页上方看“这一轮实际怎么跑”，右侧会话详情看“原始 agent 过程中发生了什么”，并保留跳转外部 CLI 的 handoff。
