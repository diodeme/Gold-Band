# 会话式运行时

## 信息架构

会话运行时窗口是用户与 agent 交互的核心区域。左侧选中最小单位是 run，右侧主区域永远展示当前选中 session 的具体对话。

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
- 查看工作流：打开 Sheet，复用旧 UI 的运行态工作图组件与数据链路，展示当前选中 session 所在 round 的实际路径图
- 查看工作流中的节点状态、暂停/成功图标、产物数、附件数、agent 标识等信息应与旧 UI 保持一致
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

### 默认 session 选择
- 用户已有选中 session 且仍有效时保持
- 多个 session 默认最近 session
- run 结束时显示到达 end 状态的 session
- 新会话从会话式主页发起后，run 创建命令只负责落盘 task/run 初始状态并后台启动执行；前端收到该 run 的第一个 ACP live event 后必须立即刷新 session tree，插入对应 attempt，选中该 session，并把右侧详情切到该 session。后续同一 attempt 的流式消息由 ACP 会话详情订阅直接合并，不依赖整页轮询。
- run 已进入 `running` 但首个 attempt 尚未出现在 session tree 前，右侧主区域显示 `Agent 调起中` 状态，不回退为“暂无活跃会话”。attempt 已出现在 session tree 但尚无可见 thought/text/tool timeline item 时，消息主区域显示 `处理中...`；收到首个 thought 后自然切换为 `思考中...`，避免创建 session 后到首 token 前出现空白。

### 自动切换规则
- 上一个 session 完成 + 消息窗口在底部 → 自动切换并折叠历史
- 用户不在底部（正在看历史）→ 不自动切换、不折叠
- 只有一个 session 运行中 → 自动展开该 session
- 多个 session 运行中 → 显示折叠行（session 名 + 实时状态），用户点击进入

## Composer 附件

继续对话时可上传附件作为本轮输入内容：

- **入口**：纸夹按钮、拖拽、粘贴（统一走 same-session 附件模型）；桌面端文件进入 WebView 后即声明可拖拽，拖入 composer 时应稳定显示可投放状态
- **预览**：图片文件在 composer 内显示缩略图，点击可打开沉浸式大图预览；预览使用单层深色遮罩按合适尺寸展示原图，不支持缩放或拖拽，点击空白遮罩关闭
- **传输**：输入附件作为 ACP content block 发送给 agent，不混入 agent 输出产物目录

## Composer 状态

运行中的状态提示必须放在 composer 内，compact 模式下也不能只展示耗时或 token。当前步骤状态应展示具体文案：发送中、处理中、思考中、工具调用中、响应中、停止中；会话式运行页的 compact 用量栏需在计时前展示带轻量旋转图标的状态标签，例如“思考中...”或“工具调用中...”。旋转标识应避免 SVG stroke 在高频刷新下掉帧，优先使用 CSS 边框圆环。Round 详情等非 compact 面板继续使用 composer 内状态行，不作为消息流卡片。

### 互斥状态
1. **正常输入**：当前 session 已正常结束时，用户可继续输入消息（含附件）
2. **运行中锁定**：当前 session 正在运行时不允许输入消息
3. **运行错误提示/操作**：当前 session 派生为 `runtimeDisplay.code=error-blocked` 或终局失败时，不允许输入，显示错误原因和修复入口；`error-blocked` 不归入“暂停可继续”，`killed / failure / invalid` 也必须使用终止或失败文案，错误态不得复用“当前会话已暂停，可继续运行”这类暂停提示
4. **工作流无效修复按钮**：需要通过 runtime 继续暂停 run 且 workflow 无效时，不允许输入，显示修改按钮；该状态优先于普通暂停继续
5. **继续按钮**：当前 session 普通暂停且可继续时不允许输入，显示继续按钮

### 修复入口

- 会话运行时的“修复”按钮与旧任务工作流页的 repair drawer 心智一致：打开当前任务工作流编辑 Sheet，让用户修复 workflow 配置。
- 修复 Sheet 标题使用“修复工作流”，而不是普通“编辑工作流”；Header 中展示无效状态、查看错误原因入口和错误原因摘要，帮助用户理解为什么需要修复。
- 在会话页保存修复后的 workflow 后，必须重新拉取当前 conversation run VM，使 workflow 有效性、session tree、工作流图与 composer 状态立即刷新。
- 修复入口不直接调用 `continueRun`；用户完成修复后再按运行态规则继续。

### 继续输入
- 当前 session 正常结束后，在会话窗口追问属于 ACP same-session prompt，不要求 authoring workflow 合法
- 追问发送时，当前会话对应行进入旋转运行态；结束后只影响该 ACP session 的消息流，不触发工作流 runtime 继续执行
- 当前 run 暂停后通过 runtime 继续仍然要求 workflow 合法；如果 workflow 无效，composer 只显示修改按钮

### 停止
- 停止并重跑在顶部操作区
- composer 内也有 stop 按钮（ACP 会话停止）
- composer 内的 ACP 停止表示“中断当前响应”，不是 workflow 配置错误；停止后的 attempt 应显示为可继续暂停
- ACP 停止先尝试优雅取消，超时后可 kill provider 进程；由停止触发的 adapter closed / cancelled 结果仍按 `process-interrupted` 派生，composer 显示继续按钮，不显示修复工作流入口

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
- 输入附件来源于 task 级 authoring/attachments/，创建会话时设定，重跑自动复用
- 输入附件使用 Upload 图标 + 蓝色标记，与输出产物/附件区分
- 当前选中 session 的产物 / 输出附件统一通过底部文件项进入弹窗查看，点击文件项直接打开该文件详情，不再经过单独列表页，也不再保留顶部重复入口
- 点击查看详情，图片类附件以 base64 预览展示

## 附件生命周期

- 新会话附件绑定 task，作为初始输入的一部分，持久化到 authoring/attachments/
- 重跑复用 task-level 附件（同一 task 的 authoring/attachments/ 在多次 run 间共享）
- 继续对话新附件进入当前 ACP session，发送后以文本形式告知 agent 文件名
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

## 工具调用参数展示

- 工具调用卡片展开后以有序列表展示工具输入参数
- 参数按来源优先级提取：rawInput > 结构化 fields > title/locations 解析
- 同标签参数保留多个不同值（如多个路径、多个查询条件）
- 语义化参数缺失时回退展示原始输入 JSON
