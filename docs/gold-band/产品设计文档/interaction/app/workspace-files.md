# 右侧工作区文件浏览与编辑

## 1. 产品定位

文件能力是会话右侧工作区的一类通用资源，不是 IDE、终端或独立一级页面。用户可以在快速对话和会话详情中浏览当前工作空间，查看文本、代码与图片，并通过会话中的本地文件链接直接打开对应文件。

## 2. 核心交互

- 右侧工作区没有 Tab 时展示“工作空间”入口；进入后以当前会话的工作位置为目录根（见 2.2）：会话在 linked worktree 中工作时是该 worktree，否则是 `projectId` 对应的项目工作空间。对于已发起且已选择运行节点的会话，入口页额外展示“运行目录”；快速对话 draft 和尚未产生运行节点的会话不展示该项。空态入口按工作区可用宽度整行展开，整行都是可点击区域，不以内容宽度收缩成胶囊卡片。存在 Tab 时，Tab 栏最左侧固定展示“新建 Tab”图标，使用 shadcn/Radix 菜单展示与空态完全相同的资源入口；选择入口后按资源稳定 key 打开或聚焦 Tab，禁止为同一资源创建重复 Tab。图标不随标签横向滚动，标签溢出菜单仍位于最右侧。溢出菜单是紧凑的单行选项列表：每项展示资源图标、截断标题和当前项勾选态，32px 行高，不使用说明文字或卡片化大间距。
- 右侧工作区整体最小宽度为 288px。可用宽度不少于 500px 时，左侧显示文件详情，右侧显示目录树；详情最小 280px、目录最小 200px。阈值分别由 `configs/app-config.toml` 的 `workspaceLayout.rightWorkspace.minWidth` 与 `workspaceLayout.rightWorkspace.file.splitMinWidth` 配置。工作空间和运行目录打开时沿用全局 `rightWorkspace.width`，不得主动扩宽或覆盖用户记忆；用户直接拖动可扩展到配置化的 1440px 上限，同时始终保留中间会话区的最小宽度。空间不足时切换为“文件 / 目录”单栏，不横向压缩两个不可用面板。
- 窗口连续缩放属于 DOM 布局热路径：外层只允许一个 `ResizeObserver` 合并到动画帧，并且只计算左/中/右可见性等离散断点；左栏与右栏跨过各自断点时立即展示或隐藏，使布局反馈始终对应当前窗口宽度。右栏可见且尚未达到用户首选宽度时，中栏使用 `preserve-pixel-size`、右栏使用 `preserve-relative-size`，由 `react-resizable-panels` 把窗口增减空间直接分配给右栏；到达首选宽度后交换所有权，中栏吸收后续窗口增量而右栏保持像素宽度。`ResizablePanel.onResize` 只把实际宽度写入 ref，并仅在“是否达到首选宽度”变化时发布一次离散 React state。因此文件区变宽和变窄都在实际宽度跨过配置化的 500px 阈值的当帧切换单双栏，不等待松手；固定拓扑中的左栏原生展开挤压右栏时也会立即切回右栏增长状态，禁止以方向锁掩盖低于最小分栏宽度的布局。用户直接拖动右侧 separator 时临时把右栏上限解锁到配置最大宽度，结束后以新偏好重新设限。热路径禁止调用 `panel.getSize()` 或 `panel.resize()`，避免面板库布局后再被应用代码二次改写。文件面板的逐像素宽度只写 ref；React state 只发布文件区单栏/双栏等离散状态，处于同一阈值区间时不得重渲染文件详情、Markdown 编辑器或目录树。内层 ResizeObserver 只能登记后续 rAF，并在该帧重新读取最终 `clientWidth`，不能立即消费面板库重组过程中的中间宽度。文件区状态机同时接收窗口宽度方向：变宽时禁止双栏退回单栏，变窄时禁止单栏重新升级双栏；一次 shell 尺寸变化引发的后续内层回调在 120ms settle 窗口内沿用最后方向，尺寸稳定后才回到 stationary，使右侧 separator 直接拖动仍可双向切换。目录分栏宽度只在用户直接拖动内部 separator 完成后保存，窗口缩放产生的布局变化不得覆盖用户偏好。
- 左侧导航、右侧工作区 Panel 及其相邻 separator 始终保留在同一 `ResizablePanelGroup` 拓扑中，自动隐藏使用组件原生 `collapsible + collapsedSize=0`，跨断点时只执行一次 `collapse/expand`；禁止条件卸载后重新挂载，否则面板库会恢复旧的双栏或三栏布局快照并瞬间挤压已经展开的文件双栏。折叠时 separator 只切换为 disabled、透明且不可命中，Panel 内容可以随展示态卸载；紧凑右栏内容只在 Sheet 中挂载一份。
- 目录按层懒加载，使用虚拟化树与标准键盘语义；搜索框右侧提供紧凑目录 / 树形目录切换，默认采用紧凑目录。紧凑目录只把“当前目录仅有一个目录子项且没有文件”的连续链投影成点分隔单行，真实目录节点、路径和 watcher 生命周期保持不变；树形目录逐级展示真实目录。两种模式下，用户展开目录后都连续加载并展开单目录链，直到遇到多个子目录、任意非目录项、空目录或安全深度上限。展开目录保持当前滚动锚点，只有新选择/新定位的文件可以触发一次主动滚动。根目录发生创建、删除或重命名时，刷新必须保留现有树实例、展开状态和滚动锚点，不能切入 loading 后重挂载并跳回顶部；局部结构刷新后必须按层重新装载仍处于展开态的后代，使新增文件只拆开受影响的紧凑链，其他链继续紧凑展示。只有首次加载或无可用快照时才展示 loading。虚拟树 overscan 按实际 content-box 高度动态计算，至少保留 24 行并覆盖约两个视口，快速滚动时不得暴露尚未挂载的空窗；滚动本身不发起目录加载。搜索在 Rust 后台执行并遵守 `.gitignore`，包含 `.github` 等隐藏文件但跳过 `.git` 仓库元数据，只为最终返回的有限结果读取文件大小与修改时间，前端只接收有限结果。匹配使用 `nucleo-matcher` 的 Unicode-aware 模糊评分，支持 `wft` 命中 `WorkspaceFileTree.tsx`；结果按“文件名完全匹配、文件名前缀、文件名模糊匹配、仅相对路径模糊匹配”分层，同层再按 Nucleo 分数、较浅路径和自然路径稳定排序。
- 模式切换按钮的图标表达当前展示状态：紧凑目录显示列表收合图标，树形目录显示列表树图标；Tooltip 与无障碍标签表达点击后的切换目标，避免把状态指示与动作说明混为一体。
- 目录筛选属于工作区工具栏控件，不属于 CodeMirror。它复用 shadcn/ui `Input` 的 `toolbar` 外观：静止时使用低对比度表面与边界，键盘聚焦时保留 1px 语义色焦点环和边界变化；表单输入继续使用标准的高可见焦点态，不能为了视觉弱化而全局移除无障碍焦点提示。
- 文件和目录共用紧凑右键菜单，提供绝对路径和 `/` 分隔的工作空间相对路径复制；紧凑目录行的选择、右键和“在文件管理器中打开”统一作用于合并链最后一级真实目录，需要操作中间目录时切换到树形目录。菜单按 LTR 优先从点击点向右展开，空间不足时允许组件进行碰撞调整。复制路径不得激活文件、切换详情或改变树布局；成功不提示，失败使用不占据文档流的浮层提示。Windows 绝对路径对客与剪贴板统一移除 `\\?\` extended-length 前缀，UNC 路径恢复为标准 `\\server\share` 格式。
- 项目工作空间目录树（非只读）支持条目增删改：文件右键提供“新建文件 / 新建文件夹”（创建在同级目录）、“重命名”“删除”；文件夹右键的新建目标为该文件夹本身；树空白处右键提供在根目录新建文件 / 文件夹。新建与重命名均为行内编辑：新建草稿行出现在目标目录中（文件夹草稿排在最前，文件草稿排在文件夹之后），目标目录自动展开，紧凑目录链不得把草稿折叠进合并链；重命名默认只选中扩展名前的主名。Enter / 失焦提交，Escape 取消，名称未变视为无操作。删除使用确认对话框，文件夹说明将连同内容移入系统回收站；有未保存编辑时额外警示。删除一律移入操作系统回收站 / 废纸篓，不做永久删除。
- 名称校验由 Rust 统一执行并返回 `workspace-file.name-invalid` + `reason`（empty、surrounding-whitespace、reserved、too-long、invalid-character、trailing-dot）；Windows 额外拒绝 `<>:"|?*`、控制字符、结尾点和 CON/PRN/AUX/NUL/COM1-9/LPT1-9 保留名。同名冲突返回 `workspace-file.already-exists`，仅大小写不同的重命名（同一文件）允许执行。所有写操作只接收 `projectId + workspacePath + relativePath/name`，由 Rust 重新解析并校验文件根后拒绝越界，不跟随末级符号链接。失败以浮层提示错误码对应的 i18n 文案，不进入文档流。
- 同一项目同一时间只允许一个条目写操作，进行中的行显示忙碌态，其他写操作与撤销被忽略。重命名或删除前先冲刷目标范围内文件的待保存编辑，冲刷失败则拒绝操作（`workspace-file.unsaved-changes`）；成功后已展开目录与当前打开文件跟随新路径，被删除范围内的文件关闭选择。
- 目录树获得焦点时 `Ctrl/Cmd+Z` 撤销最近一次条目操作：新建 → 移入回收站，重命名 → 改回原名，删除 → 从回收站还原到原路径。撤销历史只保存在当前应用会话内，每个项目最多 20 步；逆操作失败时丢弃该步并提示，不反复重试。回收站还原通过后端签发的不透明 receipt（每进程最多 64 个）完成，前端不接触回收站内部路径；原路径已被占用返回 `already-exists`，回收站条目已不存在或原父目录缺失返回 `workspace-file.restore-unavailable`。
- 会话详情的右侧工作区入口页及其“新建 Tab”菜单提供独立的“运行目录”资源，并展示当前 attempt 的运行产物；会话标题栏不重复放置入口。“工作空间”资源表示当前会话工作位置的文件根。右侧工作区按“Tab 生命周期、上下文绑定域、同 locator 数据失效”分层：Tab 集合继续归属于 run；工作空间 Tab 身份绑定 project，文件根随当前 session 的工作位置投影；运行目录使用 `conversation-directory:<projectId>:<taskId>:<runId>` 作为 run 内唯一 Tab 身份，并绑定当前 selected attempt。切换 session/attempt 时，已打开运行目录必须在原 Tab 内原位替换完整 attempt locator，不新增、不激活、不关闭 Tab；未打开时只更新入口。系统提示、附件、Diff 等显式打开的历史资源继续固定其原 locator，不因 selected attempt 变化而重绑。两类资源均复用同一 `FileWorkspaceSplitLayout`，因此共享左侧文件详情、右侧目录树、窄宽度“文件 / 目录”切换和宽度响应逻辑；运行目录默认只读，避免直接修改 Agent 运行产物。两类目录树均以当前文件的 canonical path 驱动同一主题化选中态（完整 `accent` 背景、细侧标与 `accent-foreground` 图标色）；目录只展开或收起，不改变当前文件选中。react-arborist 的瞬时焦点只使用主题 ring，不得再绘制第二块 `accent` 背景；hover 使用较弱的 `accent` 层级，因此当前文件、键盘焦点和鼠标反馈在 Gold Band、科技中性的明暗方案中都保持可区分。运行目录的虚拟树通过容器 `ResizeObserver` 测量可用高度，和项目目录一样填满详情区域并在内部滚动，不使用固定像素高度。高度测量必须归属于实际挂载的目录树组件；单双栏切换替换目录树 DOM 时，该组件随布局分支重新挂载并重新绑定观察器，禁止由跨分支存活的父组件持有旧 DOM 的测量生命周期。attempt locator 变化属于真实数据身份切换，必须清理旧文件选择、预览和目录树后读取新根目录，并通过 generation 或取消标记阻止旧 attempt 的迟到响应覆盖新目录。目录、文件以及工作空间筛选结果文件共用同一右键菜单组件，按“复制绝对路径、复制相对路径、在文件管理器中打开”的顺序提供操作；运行目录动作只接收 task/run/round/node/attempt 与相对路径，由 Rust 重新计算 attempt 根目录并拒绝越界；工作空间动作只接收 `projectId + workspacePath + relativePath`，由 Rust 重新解析并校验文件根。前端不传递任意本机绝对路径。
- 运行目录和用户消息附件中的 `.md` 必须复用统一的只读 Markdown 文档适配器：默认实时预览，可切换源码并复制源码，始终保持 `editable=false`。节点在当前 turn 新增并从附件卡打开的 `attachments/` 文件属于交付物编辑资源，复用普通 `FileContent`：Markdown 默认实时预览、可切换源码且 `editable=true`，保存使用外部文件精确 grant、revision/CAS 与原子写入。两类入口不得仅按普通文本省略 Markdown mode；文档超过统一配置阈值时自动固定为源码模式。
- 运行目录的 attempt locator 与入口文案属于低频工作区投影，只在这些语义字段实际变化时更新；语义未变时不得重复同步已打开 Tab 或重新读取目录。Agent text、thought、tool 等流式事件不得仅因 session leaf 对象引用变化而刷新工作区 Context。react-arborist 的节点 renderer 必须是模块级稳定组件，不能在 render 中创建新的组件类型；同一节点 identity 未变化时，无关流式更新和父级重渲染不得卸载目录行或关闭已经打开的 Radix 右键菜单。真实删除或重命名目标节点时，菜单随节点生命周期关闭。
- 单栏状态的“文件 / 目录”视图由当前选中文件的稳定 identity 驱动；无论此前是否已有文件，目录树选择新文件后都必须自动切回“文件”。重命名文件或其祖先目录、撤销重命名导致当前文件路径跟随变化时，属于同一文件的位置更新而不是新选择，不得切换视图，用户留在目录树中继续操作。用户消息附件的只读 CodeMirror 与节点附件卡打开的可编辑 CodeMirror 都启用原生折行并约束内部最小宽度，长行只能在内容区内换行，不得撑宽右侧工作区。
- 项目工作空间始终只有一个稳定的 `file-browser:<projectId>` 文件 Tab；树点击、搜索结果和会话文件链接均在该 Tab 内更新当前选中文件。`projectId + canonicalPath` 仅作为 `FileContentStore` 的文档身份；再次点击同一文件的不同链接位置只更新 `target/targetRevision`，不创建任何文件级 Tab。

### 2.1 工作空间文件引用到 Composer

工作空间文件引用是 Composer 的一等结构化上下文，不是普通上传附件，也不是插入正文的一段路径文本。用户在项目工作空间文件树或搜索结果中右键 `kind=file` 的条目并选择“引用到对话”后，当前可用 Composer 的上下文区出现一个文件 chip；该 chip 只表示“本次 prompt 携带这个文件的引用”，不把文件正文、绝对路径或 Markdown 链接写入 textarea。

- 引用 chip 与现有 quote / attachment chip 使用同一轻量上下文区、同一删除交互和同一数量上限；文件 chip 展示文件名，Tooltip 展示相对路径与大小。点击未发送 chip 先通过工作空间 resolver 取得 canonical locator，再复用 `file-browser:<projectId>` 资源并选中当前文件，不创建文件级 Tab；删除 chip 只移除本次 prompt 上下文，不关闭用户已打开的工作空间文件 Tab。
- 前端草稿保存 `projectId + relativePath` 与展示摘要；`canonicalPath` 只能作为当前工作区投影用于打开文件，不作为提交身份。提交、排队和执行前的引用身份固定为权威 `projectId + relativePath`，重复引用同一文件去重，不新增文件 ID、revision 或第二事实源。文件树与当前 Composer 不属于同一个工作空间时仍可引用；该引用按自己的 `projectId` 解析，不因为切换 Composer 的 workspace 被清除。快速对话的 workspace 选择必须经由 App draft boundary 统一派发；composer selector、sidebar、workspace 移除后的自动切换和设置页入口都保留正文、普通附件和已添加的文件引用。
- 文件树和搜索结果的引用动作只对真实文件可见；目录、symlink、other、只读演示页以及当前 scope 没有 Composer 时隐藏。引用命令返回 added / duplicate / limit-exceeded / unavailable 结构化结果：added 在右侧工作区为 dock 时聚焦 Composer textarea，duplicate 作为幂等 no-op，limit-exceeded 在 Composer 上下文区显示中英文错误；窄屏 Sheet 展示时不抢走 Sheet 焦点。
- 发送前的引用校验和发送后的再次解析都由 Rust 在 blocking pool 完成：每条引用按自己的 `projectId` 在已注册工作空间中取根目录；当前会话项目的引用按该会话的 canonical 工作目录解析（运行 worktree、dynamic 节点 worktree 或 repo root），与 `@` 文件菜单列出的根一致。新会话尚无工作位置，按项目根解析。relative path 先做跨平台 lexical 校验，拒绝 `..`、根路径、Windows 盘符前缀和 UNC 路径，再 canonicalize 并确认仍在该 workspace 且为普通文件。prompt 文件列表使用解析后的 canonical 绝对路径。重复项先按 authoring identity、再按 canonical path 去重，最终唯一引用数加附件数不得超过上限。未注册的 project、文件缺失、目录、权限失败和 symlink escape 均返回既有 `CommandErrorVm { code, params }` 结构，前端负责中英文恢复文案。
- 定时任务 create / update 属于 authoring 入口，完整 service 写入在 blocking pool 执行，持久化前必须校验生效后的 workspace 引用集合；`workspaceFiles == null` 表示沿用当前集合，空数组表示清空，非空数组表示替换。执行前仍按当前磁盘状态再次解析，处理创建后文件被删除、替换或越界的变化。
- 该链路不读取文件正文、不计算 BLAKE3 revision、不签发 preview grant、不复制到 `user-inputs`，因此不得复用会读取全文的附件 stat/preview 命令。文件大小随路径进入 prompt 文件列表，MIME 只用于 timeline 展示；未知扩展回退 `application/octet-stream`。
- 发送后用户消息渲染轻量文件引用 chip。点击该 chip 打开当前工作空间文件；如果排队后文件被删除、改名或越界，执行返回结构化错误并按既有 turn 生命周期结算，不伪造成功，也不影响其他 prompt。队列摘要必须暴露文件引用数量，纯文件引用的排队项不得显示为空消息。
- 关闭最后一个资源 Tab 时右侧工作区同步收起。接口级验收必须按稳定 file-browser key 查询资源，并从 `selectedFile` 读取当前文件与定位 revision；连续打开任意数量的项目文件后 Tab 数仍为 1，测试和消费者不得继续依赖旧的 file key 或“每文件一 Tab”结构。
- 会话中的本地文件链接使用“文件图标 + 语义链接文字”的轻量文本按钮形态，不使用背景、边框或阴影；图标与文字统一消费主题包的 `link` 语义色，不得复用“运行中”状态色，保留主题 `font-medium` 层级，默认不显示下划线，hover 时显示下划线，键盘 focus 使用同一 `link` 语义色保留清晰 focus ring，不可用时统一切换为 `muted-foreground`。链接目标带 `:line[:column]` 或 `#Lline[-LendLine]` 时，可见名称必须连续显示为紧凑的 `文件名:位置`，位置与文件名完全继承同一字号、字体、字重和颜色；不得用间隙、独立颜色、独立 badge、小号等宽文本或第二层底色把位置拆成附属标签。Markdown label 已含等价位置时不得重复追加。路径解析得到的 `target/targetRevision` 属于绑定 `projectId + canonicalPath` 的一次定位意图：相同链接每次点击都产生新 revision。Adapter 以 `documentKey + contentRevision + 当前 EditorView ref` 判断文档实例，不得用全文字符串相等判断，因为 CodeMirror 会规范化 CRLF。`onCreateEditor` 只初始化 View 插件；外部定位必须等受控 `value` 同步后的 React effect，再在同一个 CodeMirror transaction 中提交 selection 与官方 `EditorView.scrollIntoView(range, { y: 'center' })` effect。滚动测量、虚拟高度换算与视口更新完全交给 CodeMirror，不得用 rAF/ResizeObserver 轮询、`coordsAtPos`、估算块高度或直接写 `scrollTop` 实现第二套滚动器。transaction 成功 dispatch 后按文档身份消费 revision，文件切换不能沿用其他文件的已消费 revision。
- 会话本地文件链接必须先经过统一 Rust resolver，再创建或更新文件资源。resolver 接受工作空间相对路径、平台绝对路径与 `file://` URL；WebView 在 Windows 会把盘符绝对路径投影成 `/C:/...` URL pathname，Rust 仅在 Windows 对满足 `/<盘符>:/` 的输入移除一个前导 `/`，Linux/macOS 必须把 `/...`（包括 `/E:/...`）保留为 Unix 绝对路径。只有 resolver 成功返回的 canonical locator 才能进入 Tab、读取、外部文件授权或系统打开链路；失败时在被点击链接旁展示结构化错误，不创建错误资源或伪造 canonical path。用户再次点击同一链接即从 resolver 重新尝试，不维护独立重试状态。同一链接请求按 `workspace handler + href + revision` 隔离；工作空间或 handler 改变后，旧请求不得阻塞新点击或把迟到错误投影到新作用域。
- 工作空间外文件仅能由用户显式点击会话本地文件链接打开。详情显示不可点击的绝对路径，右侧目录树仍属于当前工作空间；文本、代码、配置和 SVG 源码允许编辑，图片保持只读。

### 2.2 会话工作位置与文件根

- 会话 VM（`AcpSessionVm`、`ConversationSessionLeafVm`）只暴露一个三态 `workLocation`：`main`（项目根）、`worktree { path, branch }`、`unavailable { reason: released | unresolved, path? }`。`released` 表示 worktree 已被收回，`unresolved` 表示 dynamic 节点或其 workspace 无法解析；不可用状态不得折叠成 `main`，避免把项目根文件当作会话文件展示。
- 右侧文件与源码管理 Tab 的 `root` 是当前会话工作位置在读取时的投影，不写回独立事实源。会话切到另一个根时，选中文件若属于旧根则取消选中；根变为不可用时保留选中文件，恢复后仍可回到原文件。
- 根不可用时，文件与源码管理 Tab 分别展示“会话工作树已收回 / 不可用”、原路径和“浏览主工作区”按钮。按钮只作用于当前 Tab（`browseMain`），会话重新可用后该选择失效；不自动回落项目根。
- 后端所有文件命令接收可选 `workspacePath`（`None` 为项目根）。非项目根必须是与项目共享 Git common dir 的 linked worktree 顶层目录，子目录、其他仓库和不存在的路径分别返回 `workspace-file.workspace-unavailable` / `workspace-file.workspace-outside-project`（参数 `workspacePath`）。Git 校验（两次 rev-parse）只在首次访问该根时执行，结果进入 32 项有界 LRU，按 project + 规范路径为键并在每次命中时复核目录存在与 `.git` 指针内容，worktree 被收回或替换后立即失效重验。
- 文件 watcher、前端目录树、正文代际和 watch 引用计数都以 `projectId + workspacePath` 为身份。watch 事件携带工作位置身份（linked worktree 路径，项目根为 `None`），前端按身份精确路由，不用路径前缀猜测，因此嵌套在项目目录内的 dynamic worktree 不会被主根和 worktree 根重复刷新。被监听的根目录消失时发送 `invalidated`。停止文件监听与 Git 状态监听不做 Git 校验，已收回的 worktree 仍能释放监听。

## 3. 查看格式

| 类别 | 内置能力 |
|---|---|
| 文本、日志 | CodeMirror 查看、查找、行号、换行和编辑 |
| Markdown | 项目文件与节点附件卡打开的本轮新增附件默认实时预览编辑，可切换源码并保存；系统提示、用户消息附件、运行目录文件与完全新增的历史文件版本固定只读。两类资源复用同一 AtomEditor/WorkspaceFileEditor 契约并共享模式与视口语义；可编辑资源额外共享撤销历史、revision 与自动保存队列。源码/预览切换按钮只在父级传入 `onMarkdownModeChange` 时出现，禁止画出无法切换的空按钮 |
| 常见代码与配置 | CodeMirror 按需语言高亮；无语言包时回退纯文本 |
| PNG、JPEG、WebP、GIF、BMP、ICO | 安全图片预览、缩放、适应窗口、原始大小和拖拽平移；GIF 支持播放/暂停，打开时默认播放，不因系统 reduced-motion 停在首帧 |
| SVG | `<img>` 原生矢量预览（文字、字体、滤镜与浏览器一致，缩放清晰），可切换源码编辑；工具栏“在浏览器中打开”用内置浏览器完整渲染（含脚本与交互） |
| HTML（`.html` / `.htm`） | 工作空间与运行目录树默认打开 CodeMirror 源码，内容区右上角浮层按钮再打开内置浏览器；会话和 Markdown 中的本地 HTML 引用仍直接进内置浏览器。边界见 [内置浏览器](in-app-browser.md) |
| PDF、Office、音视频、压缩包、字体、数据库及其他二进制 | 显示明确的不支持状态并提供系统应用打开 |

「使用系统应用打开」由 `open_file_with_system_app { projectId, workspacePath, canonicalPath, externalAccessToken }` 在 Rust 端按读取同一套规则授权：工作区内文件直接放行，工作区外文件必须持有匹配的外部访问授权，之后才交给系统 opener；前端不直接调用 opener `openPath`。图标按钮带 Tooltip，失败时在按钮旁按错误码提示，不静默。

文件识别以签名、BOM 和内容探测为权威事实，扩展名只辅助选择图标与语言能力；PDF、压缩包等二进制即使碰巧可按 UTF-8 解码也不得进入文本编辑器。文本编码保证 UTF-8、UTF-8 BOM、带 BOM 的 UTF-16 LE/BE，保存时保留 BOM 与 CRLF/LF 语义；无法可靠解码的内容不做有损猜测，也不自动写回。大文件读取和 revision 计算使用流式处理，不为识别或哈希重复完整载入文件。

CodeMirror 不启用上游固定浅色主题。编辑器背景、正文、行号、选区、活动行和语法高亮统一引用 Gold Band 语义色 token，应用主题切换后立即继承当前浅色或深色外观，不维护独立 IDE 主题状态。`primary` 在深色主题中属于表面色，不能作为链接或代码前景色；源码与 Markdown 的链接使用主题包 `link`，其他状态/重点语法按用途使用 `gold-running`，代码背景使用 `gold-surface-high + background` 混合色。

### 3.1 Markdown 实时预览编辑

- Markdown 使用 `@atomic-editor/editor` 的 CodeMirror 6 公开扩展实现实时预览，不创建 HTML/富文本副本，也不执行 DOM 到 Markdown 的反向序列化。
- Markdown renderer 的构建契约要求 React、CodeMirror 与 Lezer 的身份敏感运行时包在 Vite 产物中保持单实例；生产构建与 Vitest 共用同一份显式 dedupe 清单。包管理器允许存在的多版本或重复物理副本不得改变 parser、Facet、StateField、Context 与 decoration 的模块身份，也不得使实时预览静默退化为源码展示。
- 用户可直接编辑标题、强调、列表、任务项、链接、代码块和表格；当前结构的 Markdown 标记按成熟组件规则显露。
- 实时预览正文基准字号为 14px，标题、表格、代码块继续使用相对层级，不放大成文档页式展示。
- 内容区域右上角提供“复制 Markdown 源码”和“源码 / 实时预览”两个悬浮按钮。Markdown 默认打开实时预览：编辑器等预览扩展就绪后直接以渲染模式创建，不先展示源码。扩展加载失败时才进入源码，眼睛按钮会重新加载并切回预览。按钮图标表示当前正文实际所在的模式。复制内容取自当前 `EditorState.doc`，包含尚处于自动保存等待期的最新输入。
- Markdown 始终只挂载一个、身份稳定的 CodeMirror `EditorView`。源码 / 预览是同一编辑器的展示状态，不参与 React `key`，也不通过 props 替换整套扩展。基础扩展拓扑固定，并按生命周期拆成语言、模式、编辑策略三个稳定 `Compartment`：Markdown/GFM language parser 在文档生命周期内持续挂载，源码/预览切换只能重配置展示模式，不能卸载或替换 parser；每个 Compartment 以及独立通过 StateEffect 更新的图片配置都记录已应用 profile，首次 View 创建后不得为相同 profile 再 dispatch，只有对应领域状态真实变化时才允许 reconfigure/update，避免大型 widget 挂载后发生冗余同步布局。普通文本块的视口锚点为源码位置与块内像素偏移；Atomic table 使用“源码 Table 范围 + 渲染行索引 + 行内进度”，通过 CodeMirror 公共 `domAtPos()` 关联 widget DOM，并利用稳定 Markdown parser 在源码行与 `thead/tbody/tr` 之间双向映射，禁止把整个表格像素百分比直接换算成整个 Table 的字符百分比；其他 range widget 保存源码范围、组件内相对进度及往返语义位置。模式 transaction 先用官方 `EditorView.scrollIntoView` 把目标范围带入视口；固定 `scrollHandler` 在 CodeMirror 完成新 decoration 测量、准备执行该滚动目标时，使用真实 widget 几何恢复组件内部位置。巨型 widget 触发 CodeMirror 单次 viewport 稳定上限时，只允许在下一布局帧补发一次官方 measure 调度以消费仍待处理的滚动目标，不做固定帧轮询或 ResizeObserver 重试。视口顶部采样向内容区内缩 1px，避免浮点边界把已恢复的源码行误判成上一行；源码态顶部仍位于同一表格行时，反向切换复用该行原锚点。禁止读取 Atomic 私有 model、销毁 View、估算 widget 高度、保存全局裸 `scrollTop` 或同时常驻两个编辑器。
- Atomic table、Markdown 图片与 README decoration 只在模式 Compartment 内显式重配置，不随 React extensions props 反复重组；Markdown/GFM parser 只位于语言 Compartment。这样 table `StateField` 每次进入预览时都消费同一棵持续增长的语法树，长文档从源码返回预览不会因重新解析尚未完成而退化成原始 Markdown。图片授权状态继续由稳定 `StateField + StateEffect` 更新；源码切到预览前，对当前源码视口及有限 overscan 内已有 preview grant 的图片执行 `HTMLImageElement.decode()`，解码完成或明确失败后再原子提交模式 transaction。图片 URL 与 token 不因模式切换释放，禁止用截图、遮罩、淡入或固定延迟掩盖重挂载闪烁。
- 合法 GFM 表格使用 Atomic table widget；只有表格单元格本身含图片时才关闭该 widget，防止上游把原始地址直接交给 `<img>`。文档其他位置含图片不影响表格渲染。表格采用详情容器宽度和 fixed layout，长文本在单元格内部换行，不得把 CodeMirror 或文件详情撑出横向滚动。
- README 常见的单行 `<div|p align="left|center|right">`、闭合标签、`<br>` 和单行 `<img>` 进入安全白名单视图；Markdown HTML 注释属于非展示元数据，在实时预览中隐藏，在 fenced code 中作为示例出现的注释仍正常显示。只解释布局语义，图片仍通过 preview token。其他原始 HTML 显示源码，不使用 `dangerouslySetInnerHTML`。
- 单独占行的本地 Markdown 图片交给安全图片 widget。网络图片按[受信任的图片域名](settings.md#14-受信任的图片域名)加载：host 未受信任时不发请求，普通网络图片显示以 alt 为名称、指向图片 URL 的普通超链接，“图片包在链接中”的 badge 显示以 alt 为名称、严格执行外层目标的普通超链接；文档顶部提示“此文档包含 N 张来自 X 的网络图片”，提示文字过长截断时悬停显示全文（与文件头部路径相同：只有实际截断才显示 Tooltip），点击“加载”一次信任这些 host 并持久化，之后任何文档中来自这些 host 的图片直接显示。已受信任的网络图片以 `referrerPolicy=no-referrer` 的 `<img>` 显示，点击普通图片打开图片地址，点击链接图片执行外层目标。图片、提示框和引用文本的 widget 由 Gold Band decoration 以 `Prec.high` 渲染，Atomic inline preview 只负责隐藏这些节点的源码，二者在同一区间时以 Gold Band widget 为准，避免图片行被隐藏成空白。
- GitHub 提示框 `> [!NOTE]`、`[!TIP]`、`[!IMPORTANT]`、`[!WARNING]`、`[!CAUTION]`（标记独占首行，大小写不敏感）渲染为带图标、本地化标题和类型主题色左边框的提示框，类型、图标与主题 token 与对话 Markdown 共用 `web/src/lib/markdown-alerts.ts`；其他 `[!…]` 保持普通引用。没有对应引用定义的 `[文字]`、`[文字][label]` 不是链接，按 CommonMark 显示为带方括号的普通文本，不显示链接色和外链图标。
- 链接目标统一分为本地文件、同文档 `#` 锚点和 HTTP/HTTPS/mailto/tel 外链：本地相对路径复用工作区导航并以当前 Markdown 文件目录为基准（`.html/.htm` 打开内置浏览器，其余文件打开文件工作区）；同文档 `#` 由当前编辑器处理；`http(s)` 打开内置浏览器；`mailto:` / `tel:` 通过 Tauri opener 交给系统默认应用。不使用主 WebView `window.open`。内置浏览器边界见 [内置浏览器](in-app-browser.md)。
- 超过配置阈值的 Markdown 自动降级源码模式，避免长文档 decoration、表格和图片 widget 影响输入性能。
- 详细数据、接口、安全和验收约束见[Markdown 实时预览编辑开发方案](../../../开发计划/新UI/Markdown实时预览编辑开发方案.md)。

## 4. 编辑与保存

- 文本修改先进入 CodeMirror 本地状态，300ms 合并后自动保存；`Ctrl/Cmd+S`、切换资源、收起工作区和关闭 Tab 会立即冲刷保存队列。
- 撤销、重做使用 CodeMirror 原生历史；撤销或重做得到的内容与普通输入走同一自动保存协议。编辑器历史在运行期随 `FileContentStore` 保留，关闭文件后释放。
- 文本和 Markdown 的阅读位置与已消费的行号定位 revision 跟编辑器历史放在同一份文件运行期会话里。滚动时记下当前编辑器 scroller 的偏移，并在几何有效时附带视口锚点。编辑器重新挂载后，等本次测量完成再写回该偏移；预览块尚未形成真实高度时不使用会落到文档开头的锚点滚动。滚动容器高度为 0，或非用户操作把偏移打回开头时，不覆盖已记下的位置。新的行号链接仍优先于旧阅读位置。磁盘内容变化形成新的编辑边界时，偏移、锚点和已消费定位与撤销历史一起清空。图片缩放和滚动继续使用现有 image view state。模式切换仍用视口锚点，不把裸 `scrollTop` 当成源码与预览之间的换算。
- 正常编辑不展示长期 dirty 圆点，也不弹出未保存确认。只有保存失败或磁盘版本冲突时阻止关闭，并在文件头部提供重试、重新载入、重新授权或显式覆盖操作。
- 后端写入必须携带读取时的 `FileRevisionVm`。revision 不一致时不修改磁盘，前端进入 conflict；所有写入使用原子替换并保留原文件权限。
- Rust `notify` 事件区分自身 `operationId/revision` 与外部写入。干净文件自动重新载入；存在本地修改时暂停保存并进入冲突状态。
- 目录结构是否变化由已加载的树判定，不由 watcher `kind` 判定：notify 的事件种类随平台与批次合并而变（Windows 新文件先报 create 再报 modify，checkout 写入的新文件在批次内可能只剩 `modified`），`kind` 只用于区分路径事后是否仍存在（`removed`、无 revision 的 `renamed` 表示已不存在）。仍存在的路径：已在树中属于文件内容领域（普通写入与原子替换），不在树中且其父目录已加载才是新增节点；已不存在的路径：只有树中已知节点才算结构变化，原子写入的临时路径因此不触发刷新。父目录未加载（折叠未展开）的路径对用户不可见，直接跳过，不占用待刷新父目录上限。带 `operationId` 的自身写入已由操作自身刷新父目录。目录树不展示 revision 元数据，因此普通自动保存不产生目录请求或树快照更新。
- 根目录列表同一时刻只有一个请求。列表进行中收到对账或结构失效时不另起并发请求，而是标记过期，当前列表返回后立即重读一次再进入 `ready`，避免早于 watcher 启动或早于变化的列表结果覆盖较新的磁盘状态（例如以新工作树发送时，树在 checkout 过程中首次加载）。
- 工作区文件面板激活时必须先建立前端事件订阅并确认后端 watcher 已启动，再对已有缓存执行一次权威磁盘对账：目录树静默重读根目录和仍处于展开态的分支，当前选中的 clean 文件静默重读内容。若搜索框仍有关键字，同一次对账重跑文件名搜索，并保留上一次结果直到新结果返回。对账保持 `ready`、展开状态和滚动位置，不退回首次 loading。离开会话会停止文件监听并递增正文代际；再进入时这次重读仍然保持已有正文和阅读位置，不能因为代际变化把已打开文件切成 loading。重读结果按目录 identity 合并进已加载子树：仍存在的目录保留已加载后代，列表未变化时不发布新快照，也不把子节点先清成未加载再逐层打开。这样面板未挂载期间由 Agent、IDE、脚本新增的空目录、文件或内容不会永久停留在旧缓存。文件面板的最后一次监听停止时递增该项目的正文代际；代际过期的其他已缓存文件不在激活时批量重读，下次打开才读磁盘。监听持续期间仍复用同一代际的正文，并由文件事件重读干净文件。watcher 启动失败不得吞错或保留虚假的引用计数，下次激活必须能够重试；启停、订阅和引用释放保持对称。
- 已有文件名搜索结果时，目录结构刷新的同一次去抖也重跑搜索，旧结果保持可见，不把列表换成 loading。只改文件内容不改变文件名搜索，不重跑搜索。用户改关键字后的迟到结果仍由搜索序号丢弃。
- watcher 仍是唯一实时失效来源，不增加轮询。Rust 事件通道和单批路径集合必须有固定上限，150ms quiet debounce 同时受 1 秒最大延迟约束；持续生成文件不能无限延后前端收敛。notify 错误、事件队列溢出或单批路径溢出统一发送一次 project/file scope `invalidated`，前端据此重读权威目录与 clean 内容。普通第三方文件事件不计算全文哈希；只有匹配应用最近写入、需要恢复 `operationId` 时才计算 revision。前端正常结构事件最多聚合 64 个受影响父目录并只重读最小分支集合，超限或作用域失效才重读根与展开目录。
- 保存失败或进入冲突后，后续输入只更新内存中的最新内容，不得隐式重试写盘或绕过冲突；只有用户明确选择重试、重新授权或覆盖后才恢复保存。
- 应用正常关闭、项目切换和项目删除前统一冲刷相关文件的保存队列；冲刷失败时保留运行期内容并阻止破坏性切换。

## 5. 权限与安全

- 链接解析阶段前端只提交 `projectId + workspacePath + rawHref + 可选 baseCanonicalPath`（`workspacePath` 取当前文件根，会话工作树不可用时直接返回 `workspace-file.workspace-unavailable`，不回落项目根），canonical locator 只能由 Rust resolver 在解析、规范化并判断工作空间内外后返回；后续文件命令前端只提交 `projectId + workspacePath + canonicalPath`，不得从 raw href 自行构造 canonical path。
- 工作空间外文件使用绑定单个 canonical path、项目、读写权限和 TTL 的 access grant。token 不进入 Tab、持久化、日志或 URL，关闭 Tab 时主动释放。同一 canonical 文件再次解析得到新 grant 时，必须先由 `FileContentStore` 按文档 identity 尝试接管并轮换运行期授权；文档尚未加载时才保存为 primed grant，不能以 file-browser 当前选中项代替真实文档生命周期。
- 工作空间外 watcher 在每批事件发出前重新校验并读取轮换后的 grant；授权过期或释放后立即停止向前端发送该文件事件。
- `turn-attachment` 不接受前端绝对路径。后端必须先以完整 attempt locator、branch、changeSetId 和 attachmentId 查询 finalized manifest，验证 branch ownership，再在 attempt `attachments/` 根内解析 manifest 相对路径；canonicalize 后越界、缺失、非普通文件或 symlink escape 一律拒绝。验证通过后才可复用工作空间外精确读写 grant 与 watcher。
- 图片不使用 `file://`。Rust 完成文件签名、字节数、像素数和 revision 校验后签发 `WorkspaceFilePreviewGrantVm { token, expiresAtMs }`；SVG 禁止原始 DOM 注入和外部资源加载。
- Markdown 图片不使用组件默认的原始 `<img src>`：工作空间内图片以及工作空间外 Markdown 同目录/子目录相对图片自动签发 preview grant；文档目录外引用按当前文档统一确认一次，且只授权文档实际引用的精确文件。grant 到期前按当前引用批量原子轮换，新 token 生效后再释放旧 token；页面重新可见、图片加载失败或开发后端重启导致内存 grant 丢失时幂等补发。UNC 和危险 scheme 不加载；网络图片不经过 preview grant，只有 host 已受信任时才直接加载。
- 所有文件错误继续使用 `CommandErrorVm { code, params }`；Rust 不产生对客文案，前端同步维护中英文恢复提示。
- “在文件管理器中显示”必须先由 Rust 按当前 project/attempt 根目录解析相对路径、执行 canonicalize 并完成越界校验，再把已授权 canonical path 交给官方 `tauri-plugin-opener` 的 `reveal_item_in_dir`。工作空间与运行/会话目录复用同一平台抽象，不在业务代码中拼接 Explorer 参数，也不维护 `xdg-open`/Finder 分支。

## 6. 领域与性能边界

- `RightWorkspaceState` 只保存轻量资源 locator、Tab 与激活态。
- 右侧工作区按生命周期拆分两个 React context：`RightWorkspaceState` 暴露 tabs、activeTab、requestedOpen、width 等可变展示状态，只供 Dock、Panel 和布局消费；`RightWorkspaceCommands` 暴露会话 scope 内引用稳定的 `openResource/getResource`。消息、Turn 文件卡片和 Markdown 文件链接只消费 commands；需要判断已打开资源时通过 `getResource(key)` 调用时读取 Store，不订阅完整 tabs 快照。
- `FileExplorerStore` 管理树、展开状态、目录缓存、搜索请求序号和 watcher 失效刷新。
- `FileExplorerStore` 同时管理项目级目录展示模式，并区分用户滚动位置与一次性选中 reveal：侧栏收起/展开或展示模式切换重挂载时恢复原滚动位置，同一选中路径不会再次自动居中；程序化恢复滚动不反写用户滚动快照。虚拟树高度必须使用扣除容器 padding 后的 content box。树形目录按层级计算稳定最小行宽；紧凑目录同时保留层级最小宽度与合并名称的完整固有宽度。两种模式都只在真实内容宽度超过侧栏时出现横向滚动，未溢出时不得保留空滚动范围，并保留稳定 scrollbar gutter、关闭 scroll anchoring、限制 overscroll 传播，避免底部存在被裁切的伪滚动区及随后的回弹震颤。
- 连续缩放的可变像素值属于瞬时布局数据，不进入 `RightWorkspaceContext`、`FileExplorerStore` 的可观察快照或组件 state。标签溢出检测只观察标签条容器并按动画帧合并测量，不逐个观察所有标签子节点；只有溢出布尔值变化时才发布 React 更新。
- `FileContentStore` 管理内容快照、CodeMirror 历史、preview/access grant、自动保存串行队列、冲突与有限 LRU。
- 可编辑项目文件和 `turn-attachment` 的 Markdown 模式、内嵌图片解析状态、文档级精确授权、派生 preview grant、revision 与保存队列均与文件 Tab 同生命周期，由 `FileContentStore` 统一管理。运行目录、用户消息附件等只读 Markdown 的 mode 归属于以完整 `documentKey` 标识的只读文档组件会话；切换文档身份时重置为默认模式，不进入全局 Context 或复制文件内容状态。
- `WorkspaceFileService` 管理路径授权、目录/搜索、类型识别、读取、revision、原子写入、图片安全输出和 watcher。
- 文件面板、CodeMirror、语言支持与虚拟化文件树使用独立动态 chunk；未打开文件功能时不进入会话首屏。
- `configs/app-config.toml` 是工作区布局阈值的权威来源。桌面 `get_app_bootstrap.appConfig.workspaceLayout` 必须完整投影 `shellMinWidth/shellMinHeight`、`rightWorkspace` 及各页面 profile；`rightWorkspace.file` 与右栏宽度属于同一生命周期契约，不能只存在于前端类型或 browser mock。桌面 bootstrap 完成后前端直接消费真实契约，不增加缺字段 fallback。

## 7. 实现状态

- 2026-09-29 文件根改为跟随会话工作位置（2.2）：会话 VM 以三态 `workLocation` 取代 `worktreePath/worktreeBranch`；文件命令、watch、目录树与正文代际按 `projectId + workspacePath` 隔离；worktree 收回后文件与源码管理 Tab 展示不可用状态和“浏览主工作区”；prompt 文件引用与 `@` 菜单按会话 canonical 工作目录解析。Rust 根校验与有界缓存、watch 身份、三态投影，以及前端 store 根隔离、事件路由、Tab 根投影与不可用状态均有单元测试固化，并已在本地页面验证可用 / 已收回 / 浏览主工作区三种状态。
- 2026-09-22 会话切回时的目录对账改为按目录 identity 合并已加载子树。列表未变化不发布快照，已展开后代不再先清空再逐层打开。
- 2026-09-20 工作空间文件引用到 Composer 已完成：文件树 / 搜索结果右键、首页与 ACP 草稿、结构化 prompt DTO、Rust admission/dispatch 前解析、queue / promptSubmission 持久化、ACP ResourceLink、Timeline 消息 chip 与浏览器 mock 已接入。引用链路保持轻量 metadata，不读取正文、不复制文件、不进入普通附件路径。
- 2026-08-16 起文件工作区接入 Theme Contract v2：外层使用稳定 `workspace` wallpaper surface，编辑器使用 `editor` role。主题只改变背景投影、字体变量、边界、形状和材质，不销毁 CodeMirror `EditorView`，也不改变文件加载、保存或 revision 状态。
- 壁纸仅在工作区 surface 可见时预加载；缺失、损坏或由 performance 档关闭时回退语义底色。编辑器正文继续使用独立 editor 字体栈和字号，locale 切换不重载文件内容。
- 2026-08-17 补齐运行目录 Markdown 能力：运行目录与会话附件接入统一只读 Markdown 适配器，固定只读、默认实时预览、支持源码切换，并统一遵守高亮与实时预览长度阈值；DOM 回归测试固定运行目录 `.md` 的读取 locator、只读属性和模式切换契约。
- 2026-08-28 新增 `turn-attachment` 交付物资源：同一 turn change set 中的附件/普通变化集合天然互斥，附件点击立即打开独立 Tab，再通过 manifest 身份签发精确外部读写 grant。面板直接复用可编辑 `FileContent`，因此 Markdown 渲染/源码切换、自动保存、CAS 冲突和关闭冲刷不复制第二套实现。
- 2026-09-17 工作空间与运行目录树中的 `.html/.htm` 默认打开 CodeMirror 源码；右上角浮层按钮 flush 后打开内置浏览器。会话和 Markdown 本地 HTML 引用仍直接进浏览器。不增加 HTML 实时预览模式。

2026-08-09 文件 reveal 已迁移到官方 `tauri-plugin-opener` Rust API。项目工作空间和会话运行目录仍分别使用原有受控 locator 解析路径，只有验证后的 canonical path 会交给 opener；删除 Explorer `/select` 参数拼接和 `xdg-open` 平台分支，使 Finder reveal 成为同一接口的 macOS 实现。

2026-08-07 文件树已增加紧凑目录 / 树形目录切换：紧凑投影保持链首节点身份并把上下文操作绑定到链尾真实路径；两种模式的展开动作都会连续装载单目录链。watcher 局部结构刷新会重新装载已展开后代，保证新增文件动态拆分紧凑链。树形目录按层级最小行宽计算横向溢出，紧凑目录按层级宽度与完整合并名称的较大值计算，两种模式都只在真实溢出时出现横向滚动；模式按钮使用清晰的列表树 / 列表收合图标。右侧工作区整体最小宽度同步由 320px 收至 288px。上述数据、交互和溢出契约由 FileExplorerStore 与 WorkspaceFileTree 单元测试固化，并已在本地真实页面用深层 Java 路径完成模式切换、连续展开及有/无横向溢出的交互验证。

2026-08-04 修复流式会话同时浏览文件时的消息树失效：工作区 state/commands context 已分离，Markdown 文件链接 handler 不再依赖 tabs，历史 prompt-kit Markdown 增加静态 memo 边界。接口回归固定“打开并切换 15 个文件、改变右栏宽度时，命令消费者与历史 Markdown 不重渲染”，避免文件操作重新解析完整会话历史或重复创建 CodeMirror/Streamdown 子树。右侧 Tab 条的 `ResizeObserver` effect 只在执行测量时读取当前 ref，不允许闭包强持有已经 detach 的旧 tab strip DOM；cleanup 继续统一断开 observer 并取消待执行 rAF。

2026-08-04 进一步收敛窗口连续缩放：移除 shell resize 热路径中的逐帧右栏 `getSize/resize` 和 `onResize` 像素跟踪，连续几何只由面板库计算；右栏可见时由其独占窗口尺寸增量，并受用户首选宽度上限约束。文件区因此在扩展和收缩时都随实际宽度即时跨过单双栏阈值，不再等待窗口拖拽结束，也不会与应用主动恢复形成双布局反馈。

2026-08-03 已完成 MVP 实现，并通过 Rust 文件服务专项测试、前端全量回归、生产构建及本地真实页面的浅色、深色、双栏和窄屏验证。同日补齐桌面 bootstrap 的 `rightWorkspace/file` 配置投影与序列化契约测试，确保隐藏启动窗口不会因真实 IPC 数据缺字段导致首屏渲染中断。后续修正目录滚动锚点、内容事件导致整树刷新、右栏宽窄往返丢失偏好宽度、540px 双栏阈值、GFM 表格、紧凑排版、目录筛选工具栏焦点态、Nucleo 模糊搜索相关度及 README 安全 HTML 子集；搜索结果上限现在通过全局 Top-K 应用于全部有效候选，不再由文件系统遍历顺序决定候选集合。窗口回拉时，右栏按扣除当前可见左栏与中栏最小宽度后的真实剩余空间渐进恢复，空间足够后才恢复完整偏好宽度。若右侧文件区已经进入双栏，左侧导航必须等到恢复后仍能保住 540px 文件双栏时再出现，保证增宽过程的布局单调变化，禁止双栏闪烁或松手反跳。文件体验的第二轮收口进一步统一了低强调主题化会话文件入口、可确认的首次行号定位、32px 整行树命中区、无边框选中态和主题文件夹图标；树重挂载不再重复 reveal，底部滚动使用稳定 gutter。Markdown 改为单 View 稳定扩展模型，真实长文档表格、模式切换语义视口、README 注释和网络图片超链接由自动化契约覆盖；本地图片 preview grant 支持续期、失败保留和重新签发，正文基准字号为 14px。第三轮收口把定位意图按文档身份隔离；Markdown 相对链接统一以当前文档目录解析，响应式表格不再撑宽详情；虚拟树按 content-box 高度布局，从根因移除底部伪滚动区。第四轮根据真实反馈将模式切换锚点改为顶部逻辑块与像素偏移；行号定位删除了与 CodeMirror 竞争的外层坐标验收和手动 `scrollTop` 校正，统一由原生 `EditorView.scrollIntoView` effect 调度；README badge 严格按外层目标分流，外链接入 Tauri opener；Markdown 与源码高亮改用可读语义色，目录树选中态及 overscan 随主题和视口动态变化，并新增首次、已打开文件及重复点击同一 `#L47`、实时预览定位和四类 badge 目标的组件契约。第五轮将模式切换恢复也收敛到 CodeMirror 原生滚动 effect，移除对新 View 懒测量高度的读取、固定帧重试和手动 `scrollTop` 写入，避免渲染 → 源码 → 渲染往返时语义块向下漂移。第六轮从生命周期根因移除模式 `key` 与 EditorView 重建，改为固定扩展拓扑、显式 Compartment 原地重配置及视口图片预解码。第七轮进一步按领域生命周期拆分语言与展示 Compartment，Markdown/GFM parser 在源码/预览往返期间保持稳定，Atomic table 重新挂载时直接消费已持续解析的语法树。第八轮补齐 range widget 的双向视口映射：表格/图片内部进度映射为源码语义位置，反向切换由稳定 `scrollHandler` 在 CodeMirror 新布局测量完成后恢复块内位置，并以 1px 内缩采样消除源码行浮点边界歧义；同一 View 身份、真实 Todo 长表格往返、widget 内部进度、图片 decode 和普通语义锚点均由接口测试固化。

### 2026-08-04 会话历史版本与 Diff 资源

#### 2026-09-28 已记录变更与后台计算

- 范围说明仅放在标题文字旁的低强调 Info 图标 Tooltip 中，鼠标悬浮和键盘聚焦均可查看；不再单独占据说明行。复用既有说明翻译与共享 Tooltip，不作为告警状态。

- 变更卡表示成功工具调用通过 ACP 记录的比较，不表示最终 Git 净变化。范围外路径同样显示；路径只作标识，比较正文只从本 attempt 的 CAS 读取，不读取目标路径补齐历史。
- Write 创建后，即使 Bash/Python 或外部操作改变磁盘内容，只要没有后续 ACP diff，仍保留当时的创建比较，不推测未采集改动，也不以当前磁盘内容否定已有证据。
- 同文件编辑仅在所有捕获范围的前后版本精确衔接时折叠为首尾比较；恢复原样则不显示。任意断链时保留各次独立比较，不模糊拼接局部片段。同一路径在卡片中只占一项，展开查看各次编辑，复用原有悬浮和右侧 CodeMirror viewer。
- 删除与清空必须区分：ACP 明确标记 `_meta.kind = delete` 且旧正文存在、新正文为空或缺省时，删除后版本为不存在；普通空正文仍表示文件存在。矛盾或缺失证据不生成有效比较。连续创建再删除首尾均不存在时不展示；创建空文件和清空保留存在性变化。
- 每个活跃 Prompt Turn 沿用一个有界后台 worker；事件先持久化再非阻塞提交，待处理同工具调用只保留最终 revision。后台存储正文并按文件预计算 similar Diff 行数，使用有界的端点 hash 统计缓存，不保留或复核磁盘见证快照。会话完成状态先发布，随后等待已接收编辑处理完成再发布变更卡；不向聊天流发布预计算更新。
- 沿用捕获条目、单文件和总字节上限；失败或超限标为 partial，单次 Diff 设置 300ms 算法预算。正文仍按用户打开预览或右侧资源时懒加载，不扫描全仓或历史。
- 文件计数按唯一逻辑路径统计；多次不可合并编辑显示编辑次数，不显示伪净行数。partial 隐藏轮次增删总计；没有可展示文件时显示记录不完整，不伪装成成功的零变更。
- 复用现有 mutation journal、CAS、comparison identity 和组件，不新增 schema、依赖或持久队列；本次不新增历史迁移，不重写已有记录。

- 右侧工作区增加 `file-version`、`file-diff` 与 `conversation-asset` 三类只读资源。历史版本 key 包含 change set/change identity，同一路径不同 turn 不复用错误内容；消息附件和 artifact key 包含完整 attempt/branch locator。
- `file-diff` 使用官方 `@codemirror/merge` 的 `unifiedMergeView`，固定只读、无 merge controls，开启 gutter、变化高亮和未修改区折叠。标题使用“本轮修改 Diff”，比较当前已记录编辑或精确衔接链的首尾端点，而不是 live workspace；不同范围的断链编辑各自拥有独立 comparison identity；仅当官方 changed chunk 数量至少为 2 时展示上一处/下一处导航。viewer 必须跟随右侧工作区容器宽度并启用 `EditorView.lineWrapping`，长行在当前可视宽度内换行，不产生页面级横向滚动。普通文件与 diff 复用同一语言加载、主题和 syntax highlight extension；新增片段的主题选择器必须命中同一编辑器根节点 `&.cm-merge-b`，显式移除 merge 默认 background image，只保留实色语义背景。只读 diff/version viewer 不安装 CodeMirror 自绘 selection layer，使用应用级 `--text-selection` / `--text-selection-foreground` 原生选中态，避免 diff 标记背景遮挡深色模式选区；普通 CodeMirror 自绘选区也必须使用同一 selection token。
- 所有复用统一 viewer 的 Turn、Git worktree、Commit 与 GitHub PR Diff 保留 CodeMirror 官方行级与字符级 Diff：新增/删除行使用 10% 语义背景，`.cm-deletedText` / `.cm-changedText` 字符变化层统一降为 12% 语义背景并移除默认 background image，避免两层叠加后过重。merge viewer 继续使用 CodeMirror 默认 active line 与 chunk range 导航；用户主动框选文本仍使用应用级 selection token。
- 打开 `file-diff` / `file-version` 属于普通只读浏览，即使捕获或渲染存在限制也不得显示 Tab 黄点；限制仅在 viewer 内说明。变更列表的“修改”文件图标使用主题 `gold-running` 蓝色语义 token，不使用固定琥珀色；新增/删除仍使用各自的成功/破坏性语义色。
- ACP `oldText/newText` 必须是文件内容，不得包含 unified diff 的 `No newline at end of file` 元数据。若 provider 的后续 tool update 错把该标记混入标准文本字段，捕获层需要移除标记并恢复真实的文件末尾换行状态；已有 change set 通过 schema 迁移从 durable journal 重新生成，不把元数据伪装成普通删除/新增行。
- 变更卡收起时只展示配置数量的预览文件；展开后全部文件进入同一个 ScrollArea，预览行不得固定在滚动区外。标题使用“已记录变更 · N 个文件”，空 partial 显示记录不完整，不在标题后追加告警图标。卡片作为完整的回合结果区块，在消息流中使用 Tailwind `mb-3` 保留底部呼吸空间，避免下一条消息紧贴卡片边界。
- 变更卡从 change set summary 的加载占位切换到文件清单时，首帧必须直接采用最终收起结构；初始关闭不触发 `CollapsibleContent` 的退出动画，避免完整清单先参与绘制再收起。展开/收起动画只在用户操作折叠入口后启用，异步数据到达本身不改变 disclosure 意图。
- 变更文件行使用 shadcn/ui Hover Card 提供只读 unified Diff 预览：鼠标稳定悬浮 350ms 后按 `attempt/branch/changeSet/change` 完整 locator 懒加载，离开后保留 150ms 宽限；键盘聚焦同一行时立即打开同一预览，鼠标按下导致的 focus 不得触发键盘预览路径。预览使用紧凑尺寸，默认最大 40rem × 24rem、高度随 viewport 约束为 44vh，标题固定为 36px，正文为唯一滚动区；同一时刻只挂载当前文件的一份 CodeMirror。修改/新增行的原点击仍打开右侧工作区，并在 pointer down 发生时先阻断同一指针交互带来的 focus open request，再在导航事务开始时收起当前 Hover Card；点击后的焦点转移、布局位移、合成 pointer 事件和迟到 open timer 均不得重新打开浮层。点击时记录指针的 `clientX/clientY`；只有用户再次移动鼠标，后续 `pointermove` 的实际客户区坐标与点击坐标不同时，才解除抑制并恢复悬浮预览；不使用 WebView 在布局重排时可能失真的 `movementX/movementY` 判定真实移动。删除行只允许悬浮/聚焦预览，不新增点击导航。
- 悬浮交互的临时诊断使用 `goldBand.debug.turnFileHover=1` 本地开关启用，以 `[GoldBand][Turn file hover]` 输出可直接复制的单行 JSON；只记录行实例、change ID、开合请求、Radix Content `data-state`、指针坐标和抑制状态，不记录文件路径、Diff 正文或 locator。开关未启用时不建立 MutationObserver，不输出日志。
- 悬浮预览与右侧 Turn Diff 必须复用同一个 `ReadonlyUnifiedDiff` 和 comparison loader。前端 comparison cache 以完整 locator 为 key，只保留最近 2 项并合并相同 in-flight 请求；被淘汰的迟到请求不得覆盖同 key 的新请求。正文仍只在用户打开预览或右侧资源时读取，不预取整张文件列表。
- `get_turn_file_change_set` 与 `get_file_comparison` 只接受受控 attempt locator、branch、changeSetId/changeId；后端校验标识符、branch ownership 和 CAS hash，不接受前端提交任意 blob 路径或 runtime 绝对路径。
- `configs/app-config.toml` 的 `turnFiles` 统一管理卡片预览数、捕获条目/字节上限与 diff 渲染上限；CAS 不启用额外内存 blob cache，blob 生命周期跟随 attempt。
- 未来 Git commit/tree/blob 比较继续返回同一 `FileChange/FileComparison` 前端模型并复用 unified viewer；外部 Git 命令必须通过后台进程 helper，本期不提供 Git UI。
