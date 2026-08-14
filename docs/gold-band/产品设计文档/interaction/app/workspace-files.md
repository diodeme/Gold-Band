# 右侧工作区文件浏览与编辑

## 1. 产品定位

文件能力是会话右侧工作区的一类通用资源，不是 IDE、终端或独立一级页面。用户可以在快速对话和会话详情中浏览当前工作空间，查看文本、代码与图片，并通过会话中的本地文件链接直接打开对应文件。

## 2. 核心交互

- 右侧工作区没有 Tab 时展示“工作空间”入口；进入后始终以当前 `projectId` 对应的项目工作空间为目录根。对于已发起且已选择运行节点的会话，入口页额外展示“运行目录”；快速对话 draft 和尚未产生运行节点的会话不展示该项。空态入口按工作区可用宽度整行展开，整行都是可点击区域，不以内容宽度收缩成胶囊卡片。存在 Tab 时，Tab 栏最左侧固定展示“新建 Tab”图标，使用 shadcn/Radix 菜单展示与空态完全相同的资源入口；选择入口后按资源稳定 key 打开或聚焦 Tab，禁止为同一资源创建重复 Tab。图标不随标签横向滚动，标签溢出菜单仍位于最右侧。溢出菜单是紧凑的单行选项列表：每项展示资源图标、截断标题和当前项勾选态，32px 行高，不使用说明文字或卡片化大间距。
- 右侧工作区整体最小宽度为 288px。可用宽度不少于 500px 时，左侧显示文件详情，右侧显示目录树；详情最小 280px、目录最小 200px。阈值分别由 `configs/app-config.toml` 的 `workspaceLayout.rightWorkspace.minWidth` 与 `workspaceLayout.rightWorkspace.file.splitMinWidth` 配置。工作空间和运行目录打开时沿用全局 `rightWorkspace.width`，不得主动扩宽或覆盖用户记忆；用户直接拖动可扩展到配置化的 1440px 上限，同时始终保留中间会话区的最小宽度。空间不足时切换为“文件 / 目录”单栏，不横向压缩两个不可用面板。
- 窗口连续缩放属于 DOM 布局热路径：外层只允许一个 `ResizeObserver` 合并到动画帧，并且只计算左/中/右可见性等离散断点；左栏与右栏跨过各自断点时立即展示或隐藏，使布局反馈始终对应当前窗口宽度。右栏可见且尚未达到用户首选宽度时，中栏使用 `preserve-pixel-size`、右栏使用 `preserve-relative-size`，由 `react-resizable-panels` 把窗口增减空间直接分配给右栏；到达首选宽度后交换所有权，中栏吸收后续窗口增量而右栏保持像素宽度。`ResizablePanel.onResize` 只把实际宽度写入 ref，并仅在“是否达到首选宽度”变化时发布一次离散 React state。因此文件区变宽和变窄都在实际宽度跨过配置化的 500px 阈值的当帧切换单双栏，不等待松手；固定拓扑中的左栏原生展开挤压右栏时也会立即切回右栏增长状态，禁止以方向锁掩盖低于最小分栏宽度的布局。用户直接拖动右侧 separator 时临时把右栏上限解锁到配置最大宽度，结束后以新偏好重新设限。热路径禁止调用 `panel.getSize()` 或 `panel.resize()`，避免面板库布局后再被应用代码二次改写。文件面板的逐像素宽度只写 ref；React state 只发布文件区单栏/双栏等离散状态，处于同一阈值区间时不得重渲染文件详情、Markdown 编辑器或目录树。内层 ResizeObserver 只能登记后续 rAF，并在该帧重新读取最终 `clientWidth`，不能立即消费面板库重组过程中的中间宽度。文件区状态机同时接收窗口宽度方向：变宽时禁止双栏退回单栏，变窄时禁止单栏重新升级双栏；一次 shell 尺寸变化引发的后续内层回调在 120ms settle 窗口内沿用最后方向，尺寸稳定后才回到 stationary，使右侧 separator 直接拖动仍可双向切换。目录分栏宽度只在用户直接拖动内部 separator 完成后保存，窗口缩放产生的布局变化不得覆盖用户偏好。
- 左侧导航、右侧工作区 Panel 及其相邻 separator 始终保留在同一 `ResizablePanelGroup` 拓扑中，自动隐藏使用组件原生 `collapsible + collapsedSize=0`，跨断点时只执行一次 `collapse/expand`；禁止条件卸载后重新挂载，否则面板库会恢复旧的双栏或三栏布局快照并瞬间挤压已经展开的文件双栏。折叠时 separator 只切换为 disabled、透明且不可命中，Panel 内容可以随展示态卸载；紧凑右栏内容只在 Sheet 中挂载一份。
- 目录按层懒加载，使用虚拟化树与标准键盘语义；搜索框右侧提供紧凑目录 / 树形目录切换，默认采用紧凑目录。紧凑目录只把“当前目录仅有一个目录子项且没有文件”的连续链投影成点分隔单行，真实目录节点、路径和 watcher 生命周期保持不变；树形目录逐级展示真实目录。两种模式下，用户展开目录后都连续加载并展开单目录链，直到遇到多个子目录、任意非目录项、空目录或安全深度上限。展开目录保持当前滚动锚点，只有新选择/新定位的文件可以触发一次主动滚动。根目录发生创建、删除或重命名时，刷新必须保留现有树实例、展开状态和滚动锚点，不能切入 loading 后重挂载并跳回顶部；局部结构刷新后必须按层重新装载仍处于展开态的后代，使新增文件只拆开受影响的紧凑链，其他链继续紧凑展示。只有首次加载或无可用快照时才展示 loading。虚拟树 overscan 按实际 content-box 高度动态计算，至少保留 24 行并覆盖约两个视口，快速滚动时不得暴露尚未挂载的空窗；滚动本身不发起目录加载。搜索在 Rust 后台执行并遵守 `.gitignore`，前端只接收有限结果。匹配使用 `nucleo-matcher` 的 Unicode-aware 模糊评分，支持 `wft` 命中 `WorkspaceFileTree.tsx`；结果按“文件名完全匹配、文件名前缀、文件名模糊匹配、仅相对路径模糊匹配”分层，同层再按 Nucleo 分数、较浅路径和自然路径稳定排序。
- 模式切换按钮的图标表达当前展示状态：紧凑目录显示列表收合图标，树形目录显示列表树图标；Tooltip 与无障碍标签表达点击后的切换目标，避免把状态指示与动作说明混为一体。
- 目录筛选属于工作区工具栏控件，不属于 CodeMirror。它复用 shadcn/ui `Input` 的 `toolbar` 外观：静止时使用低对比度表面与边界，键盘聚焦时保留 1px 语义色焦点环和边界变化；表单输入继续使用标准的高可见焦点态，不能为了视觉弱化而全局移除无障碍焦点提示。
- 文件和目录共用紧凑右键菜单，提供绝对路径和 `/` 分隔的工作空间相对路径复制；紧凑目录行的选择、右键和“在文件管理器中打开”统一作用于合并链最后一级真实目录，需要操作中间目录时切换到树形目录。菜单按 LTR 优先从点击点向右展开，空间不足时允许组件进行碰撞调整。复制路径不得激活文件、切换详情或改变树布局；成功不提示，失败使用不占据文档流的浮层提示。Windows 绝对路径对客与剪贴板统一移除 `\\?\` extended-length 前缀，UNC 路径恢复为标准 `\\server\share` 格式。
- 会话详情的右侧工作区入口页及其“新建 Tab”菜单提供独立的“运行目录”资源，并展示当前 attempt 的运行产物；会话标题栏不重复放置入口。“工作空间”资源始终只表示项目工作空间。两类资源均复用同一 `FileWorkspaceSplitLayout`，因此共享左侧文件详情、右侧目录树、窄宽度“文件 / 目录”切换和宽度响应逻辑；运行目录默认只读，避免直接修改 Agent 运行产物。两类目录树均以当前文件的 canonical path 驱动同一主题化选中态（背景、细侧标与图标色）；目录只展开或收起，不改变当前文件选中。运行目录的虚拟树通过容器 `ResizeObserver` 测量可用高度，和项目目录一样填满详情区域并在内部滚动，不使用固定像素高度。高度测量必须归属于实际挂载的目录树组件；单双栏切换替换目录树 DOM 时，该组件随布局分支重新挂载并重新绑定观察器，禁止由跨分支存活的父组件持有旧 DOM 的测量生命周期。目录、文件以及工作空间筛选结果文件共用同一右键菜单组件，按“复制绝对路径、复制相对路径、在文件管理器中打开”的顺序提供操作；运行目录动作只接收 task/run/round/node/attempt 与相对路径，由 Rust 重新计算 attempt 根目录并拒绝越界；工作空间动作只接收 `projectId + relativePath`，由 Rust 重新解析注册的工作空间根目录。前端不传递任意本机绝对路径。
- 运行目录的 attempt locator 与入口文案属于低频工作区投影，只在这些语义字段实际变化时更新；Agent text、thought、tool 等流式事件不得仅因 session leaf 对象引用变化而刷新工作区 Context。react-arborist 的节点 renderer 必须是模块级稳定组件，不能在 render 中创建新的组件类型；同一节点 identity 未变化时，无关流式更新和父级重渲染不得卸载目录行或关闭已经打开的 Radix 右键菜单。真实删除或重命名目标节点时，菜单随节点生命周期关闭。
- 单栏状态的“文件 / 目录”视图由当前选中文件的稳定 identity 驱动；无论此前是否已有文件，目录树选择新文件后都必须自动切回“文件”。用户附件的只读 CodeMirror 启用原生折行并约束内部最小宽度，长行只能在内容区内换行，不得撑宽右侧工作区。
- 项目工作空间始终只有一个稳定的 `file-browser:<projectId>` 文件 Tab；树点击、搜索结果和会话文件链接均在该 Tab 内更新当前选中文件。`projectId + canonicalPath` 仅作为 `FileContentStore` 的文档身份；再次点击同一文件的不同链接位置只更新 `target/targetRevision`，不创建任何文件级 Tab。
- 关闭最后一个资源 Tab 时右侧工作区同步收起。接口级验收必须按稳定 file-browser key 查询资源，并从 `selectedFile` 读取当前文件与定位 revision；连续打开任意数量的项目文件后 Tab 数仍为 1，测试和消费者不得继续依赖旧的 file key 或“每文件一 Tab”结构。
- 会话中的本地文件链接使用轻量主题底色，文件类型图标单独使用语义强调色以保证对比度；不能只依赖下划线表达可点击。链接目标带 `:line[:column]` 或 `#Lline[-LendLine]` 时，可见名称必须连续显示为紧凑的 `文件名:位置`，位置与文件名完全继承同一字号、字体、字重和颜色；不得用间隙、独立颜色、独立 badge、小号等宽文本或第二层底色把位置拆成附属标签。Markdown label 已含等价位置时不得重复追加。路径解析得到的 `target/targetRevision` 属于绑定 `projectId + canonicalPath` 的一次定位意图：相同链接每次点击都产生新 revision。Adapter 以 `documentKey + contentRevision + 当前 EditorView ref` 判断文档实例，不得用全文字符串相等判断，因为 CodeMirror 会规范化 CRLF。`onCreateEditor` 只初始化 View 插件；外部定位必须等受控 `value` 同步后的 React effect，再在同一个 CodeMirror transaction 中提交 selection 与官方 `EditorView.scrollIntoView(range, { y: 'center' })` effect。滚动测量、虚拟高度换算与视口更新完全交给 CodeMirror，不得用 rAF/ResizeObserver 轮询、`coordsAtPos`、估算块高度或直接写 `scrollTop` 实现第二套滚动器。transaction 成功 dispatch 后按文档身份消费 revision，文件切换不能沿用其他文件的已消费 revision。
- 工作空间外文件仅能由用户显式点击会话本地文件链接打开。详情显示不可点击的绝对路径，右侧目录树仍属于当前工作空间；文本、代码、配置和 SVG 源码允许编辑，图片保持只读。

## 3. 查看格式

| 类别 | 内置能力 |
|---|---|
| 文本、日志 | CodeMirror 查看、查找、行号、换行和编辑 |
| Markdown | 项目文件默认实时预览编辑；系统提示、用户附件、运行产物与完全新增的文件版本均复用同一 AtomEditor 查看器并固定只读。查看器可切换源码模式并一键复制当前源码；两种模式共享源码与视口状态，项目文件额外共享撤销历史和自动保存队列 |
| 常见代码与配置 | CodeMirror 按需语言高亮；无语言包时回退纯文本 |
| PNG、JPEG、WebP、GIF、BMP、ICO | 安全图片预览、缩放、适应窗口、原始大小和拖拽平移；GIF 支持播放/暂停，并在 reduced motion 下默认显示静态首帧 |
| SVG | Rust 安全栅格化预览，可切换源码编辑 |
| PDF、Office、音视频、压缩包、字体、数据库及其他二进制 | 显示明确的不支持状态并提供系统应用打开 |

文件识别以签名、BOM 和内容探测为权威事实，扩展名只辅助选择图标与语言能力；PDF、压缩包等二进制即使碰巧可按 UTF-8 解码也不得进入文本编辑器。文本编码保证 UTF-8、UTF-8 BOM、带 BOM 的 UTF-16 LE/BE，保存时保留 BOM 与 CRLF/LF 语义；无法可靠解码的内容不做有损猜测，也不自动写回。大文件读取和 revision 计算使用流式处理，不为识别或哈希重复完整载入文件。

CodeMirror 不启用上游固定浅色主题。编辑器背景、正文、行号、选区、活动行和语法高亮统一引用 Gold Band 语义色 token，应用主题切换后立即继承当前浅色或深色外观，不维护独立 IDE 主题状态。`primary` 在深色主题中属于表面色，不能作为链接或代码前景色；源码与 Markdown 的链接/重点语法使用 `gold-running`，代码背景使用 `gold-surface-high + background` 混合色。

### 3.1 Markdown 实时预览编辑

- Markdown 使用 `@atomic-editor/editor` 的 CodeMirror 6 公开扩展实现实时预览，不创建 HTML/富文本副本，也不执行 DOM 到 Markdown 的反向序列化。
- 用户可直接编辑标题、强调、列表、任务项、链接、代码块和表格；当前结构的 Markdown 标记按成熟组件规则显露。
- 实时预览正文基准字号为 14px，标题、表格、代码块继续使用相对层级，不放大成文档页式展示。
- 内容区域右上角提供“复制 Markdown 源码”和“源码 / 实时预览”两个悬浮按钮。复制内容取自当前 `EditorState.doc`，包含尚处于自动保存等待期的最新输入。
- Markdown 始终只挂载一个、身份稳定的 CodeMirror `EditorView`。源码 / 预览是同一编辑器的展示状态，不参与 React `key`，也不通过 props 替换整套扩展。基础扩展拓扑固定，并按生命周期拆成语言、模式、编辑策略三个稳定 `Compartment`：Markdown/GFM language parser 在文档生命周期内持续挂载，源码/预览切换只能重配置展示模式，不能卸载或替换 parser；每个 Compartment 以及独立通过 StateEffect 更新的图片配置都记录已应用 profile，首次 View 创建后不得为相同 profile 再 dispatch，只有对应领域状态真实变化时才允许 reconfigure/update，避免大型 widget 挂载后发生冗余同步布局。普通文本块的视口锚点为源码位置与块内像素偏移；Atomic table 使用“源码 Table 范围 + 渲染行索引 + 行内进度”，通过 CodeMirror 公共 `domAtPos()` 关联 widget DOM，并利用稳定 Markdown parser 在源码行与 `thead/tbody/tr` 之间双向映射，禁止把整个表格像素百分比直接换算成整个 Table 的字符百分比；其他 range widget 保存源码范围、组件内相对进度及往返语义位置。模式 transaction 先用官方 `EditorView.scrollIntoView` 把目标范围带入视口；固定 `scrollHandler` 在 CodeMirror 完成新 decoration 测量、准备执行该滚动目标时，使用真实 widget 几何恢复组件内部位置。巨型 widget 触发 CodeMirror 单次 viewport 稳定上限时，只允许在下一布局帧补发一次官方 measure 调度以消费仍待处理的滚动目标，不做固定帧轮询或 ResizeObserver 重试。视口顶部采样向内容区内缩 1px，避免浮点边界把已恢复的源码行误判成上一行；源码态顶部仍位于同一表格行时，反向切换复用该行原锚点。禁止读取 Atomic 私有 model、销毁 View、估算 widget 高度、保存全局裸 `scrollTop` 或同时常驻两个编辑器。
- Atomic table、Markdown 图片与 README decoration 只在模式 Compartment 内显式重配置，不随 React extensions props 反复重组；Markdown/GFM parser 只位于语言 Compartment。这样 table `StateField` 每次进入预览时都消费同一棵持续增长的语法树，长文档从源码返回预览不会因重新解析尚未完成而退化成原始 Markdown。图片授权状态继续由稳定 `StateField + StateEffect` 更新；源码切到预览前，对当前源码视口及有限 overscan 内已有 preview grant 的图片执行 `HTMLImageElement.decode()`，解码完成或明确失败后再原子提交模式 transaction。图片 URL 与 token 不因模式切换释放，禁止用截图、遮罩、淡入或固定延迟掩盖重挂载闪烁。
- 合法 GFM 表格使用 Atomic table widget；只有表格单元格本身含图片时才关闭该 widget，防止上游把原始地址直接交给 `<img>`。文档其他位置含图片不影响表格渲染。表格采用详情容器宽度和 fixed layout，长文本在单元格内部换行，不得把 CodeMirror 或文件详情撑出横向滚动。
- README 常见的单行 `<div|p align="left|center|right">`、闭合标签、`<br>` 和单行 `<img>` 进入安全白名单视图；Markdown HTML 注释属于非展示元数据，在实时预览中隐藏，在 fenced code 中作为示例出现的注释仍正常显示。只解释布局语义，图片仍通过 preview token。其他原始 HTML 显示源码，不使用 `dangerouslySetInnerHTML`。
- 单独占行的本地 Markdown 图片交给安全图片 widget。网络图片永不进入 `<img>`：普通网络图片显示以 alt 为名称、指向图片 URL 的普通超链接；“图片包在链接中”的 badge 显示以 alt 为名称、严格执行外层目标的普通超链接。目标统一分为本地文件、同文档 `#` 锚点和 HTTP/HTTPS/mailto/tel 外链：本地相对路径复用工作区导航并以当前 Markdown 文件目录为基准，同文档 `#` 由当前编辑器处理，外链通过 Tauri opener 交给系统默认应用，不使用 WebView `window.open`。
- 超过配置阈值的 Markdown 自动降级源码模式，避免长文档 decoration、表格和图片 widget 影响输入性能。
- 详细数据、接口、安全和验收约束见[Markdown 实时预览编辑开发方案](../../../开发计划/新UI/Markdown实时预览编辑开发方案.md)。

## 4. 编辑与保存

- 文本修改先进入 CodeMirror 本地状态，300ms 合并后自动保存；`Ctrl/Cmd+S`、切换资源、收起工作区和关闭 Tab 会立即冲刷保存队列。
- 撤销、重做使用 CodeMirror 原生历史；撤销或重做得到的内容与普通输入走同一自动保存协议。编辑器历史在运行期随 `FileContentStore` 保留，关闭文件后释放。
- 正常编辑不展示长期 dirty 圆点，也不弹出未保存确认。只有保存失败或磁盘版本冲突时阻止关闭，并在文件头部提供重试、重新载入、重新授权或显式覆盖操作。
- 后端写入必须携带读取时的 `FileRevisionVm`。revision 不一致时不修改磁盘，前端进入 conflict；所有写入使用原子替换并保留原文件权限。
- Rust `notify` 事件区分自身 `operationId/revision` 与外部写入。干净文件自动重新载入；存在本地修改时暂停保存并进入冲突状态。
- watcher 事件必须结合 `operationId`、revision 是否存在以及当前树中是否已有该路径判断领域，不能只按 `kind` 分类。`modified`、自身写入，以及“已知且仍存在的路径被原子替换”都属于文件内容领域；原子写入产生的未知临时路径在重命名后已经不存在，也不属于目录结构。只有节点身份确实新增、消失或改名时才按受影响父目录刷新。目录树不展示 revision 元数据，因此普通自动保存不产生目录请求或树快照更新。
- 保存失败或进入冲突后，后续输入只更新内存中的最新内容，不得隐式重试写盘或绕过冲突；只有用户明确选择重试、重新授权或覆盖后才恢复保存。
- 应用正常关闭、项目切换和项目删除前统一冲刷相关文件的保存队列；冲刷失败时保留运行期内容并阻止破坏性切换。

## 5. 权限与安全

- 前端只提交 `projectId + canonicalPath`；Rust 权威解析项目根、规范化路径并判断工作空间内外。
- 工作空间外文件使用绑定单个 canonical path、项目、读写权限和 TTL 的 access grant。token 不进入 Tab、持久化、日志或 URL，关闭 Tab 时主动释放。
- 工作空间外 watcher 在每批事件发出前重新校验并读取轮换后的 grant；授权过期或释放后立即停止向前端发送该文件事件。
- 图片不使用 `file://`。Rust 完成文件签名、字节数、像素数和 revision 校验后签发 `WorkspaceFilePreviewGrantVm { token, expiresAtMs }`；SVG 禁止原始 DOM 注入和外部资源加载。
- Markdown 图片不使用组件默认的原始 `<img src>`：工作空间内图片以及工作空间外 Markdown 同目录/子目录相对图片自动签发 preview grant；文档目录外引用按当前文档统一确认一次，且只授权文档实际引用的精确文件。grant 到期前按当前引用批量原子轮换，新 token 生效后再释放旧 token；页面重新可见、图片加载失败或开发后端重启导致内存 grant 丢失时幂等补发。UNC 和危险 scheme 不加载，网络图片只保留超链接。
- 所有文件错误继续使用 `CommandErrorVm { code, params }`；Rust 不产生对客文案，前端同步维护中英文恢复提示。
- “在文件管理器中显示”必须先由 Rust 按当前 project/attempt 根目录解析相对路径、执行 canonicalize 并完成越界校验，再把已授权 canonical path 交给官方 `tauri-plugin-opener` 的 `reveal_item_in_dir`。工作空间与运行/会话目录复用同一平台抽象，不在业务代码中拼接 Explorer 参数，也不维护 `xdg-open`/Finder 分支。

## 6. 领域与性能边界

- `RightWorkspaceState` 只保存轻量资源 locator、Tab 与激活态。
- 右侧工作区按生命周期拆分两个 React context：`RightWorkspaceState` 暴露 tabs、activeTab、requestedOpen、width 等可变展示状态，只供 Dock、Panel 和布局消费；`RightWorkspaceCommands` 暴露会话 scope 内引用稳定的 `openResource/getResource`。消息、Turn 文件卡片和 Markdown 文件链接只消费 commands；需要判断已打开资源时通过 `getResource(key)` 调用时读取 Store，不订阅完整 tabs 快照。
- `FileExplorerStore` 管理树、展开状态、目录缓存、搜索请求序号和 watcher 失效刷新。
- `FileExplorerStore` 同时管理项目级目录展示模式，并区分用户滚动位置与一次性选中 reveal：侧栏收起/展开或展示模式切换重挂载时恢复原滚动位置，同一选中路径不会再次自动居中；程序化恢复滚动不反写用户滚动快照。虚拟树高度必须使用扣除容器 padding 后的 content box。树形目录按层级计算稳定最小行宽；紧凑目录同时保留层级最小宽度与合并名称的完整固有宽度。两种模式都只在真实内容宽度超过侧栏时出现横向滚动，未溢出时不得保留空滚动范围，并保留稳定 scrollbar gutter、关闭 scroll anchoring、限制 overscroll 传播，避免底部存在被裁切的伪滚动区及随后的回弹震颤。
- 连续缩放的可变像素值属于瞬时布局数据，不进入 `RightWorkspaceContext`、`FileExplorerStore` 的可观察快照或组件 state。标签溢出检测只观察标签条容器并按动画帧合并测量，不逐个观察所有标签子节点；只有溢出布尔值变化时才发布 React 更新。
- `FileContentStore` 管理内容快照、CodeMirror 历史、preview/access grant、自动保存串行队列、冲突与有限 LRU。
- Markdown 模式、内嵌图片解析状态、文档级精确授权和派生 preview grant 与文件 Tab 同生命周期，也由 `FileContentStore` 统一管理。
- `WorkspaceFileService` 管理路径授权、目录/搜索、类型识别、读取、revision、原子写入、图片安全输出和 watcher。
- 文件面板、CodeMirror、语言支持与虚拟化文件树使用独立动态 chunk；未打开文件功能时不进入会话首屏。
- `configs/app-config.toml` 是工作区布局阈值的权威来源。桌面 `get_app_bootstrap.appConfig.workspaceLayout` 必须完整投影 `shellMinWidth/shellMinHeight`、`rightWorkspace` 及各页面 profile；`rightWorkspace.file` 与右栏宽度属于同一生命周期契约，不能只存在于前端类型或 browser mock。桌面 bootstrap 完成后前端直接消费真实契约，不增加缺字段 fallback。

## 7. 实现状态

2026-08-09 文件 reveal 已迁移到官方 `tauri-plugin-opener` Rust API。项目工作空间和会话运行目录仍分别使用原有受控 locator 解析路径，只有验证后的 canonical path 会交给 opener；删除 Explorer `/select` 参数拼接和 `xdg-open` 平台分支，使 Finder reveal 成为同一接口的 macOS 实现。

2026-08-07 文件树已增加紧凑目录 / 树形目录切换：紧凑投影保持链首节点身份并把上下文操作绑定到链尾真实路径；两种模式的展开动作都会连续装载单目录链。watcher 局部结构刷新会重新装载已展开后代，保证新增文件动态拆分紧凑链。树形目录按层级最小行宽计算横向溢出，紧凑目录按层级宽度与完整合并名称的较大值计算，两种模式都只在真实溢出时出现横向滚动；模式按钮使用清晰的列表树 / 列表收合图标。右侧工作区整体最小宽度同步由 320px 收至 288px。上述数据、交互和溢出契约由 FileExplorerStore 与 WorkspaceFileTree 单元测试固化，并已在本地真实页面用深层 Java 路径完成模式切换、连续展开及有/无横向溢出的交互验证。

2026-08-04 修复流式会话同时浏览文件时的消息树失效：工作区 state/commands context 已分离，Markdown 文件链接 handler 不再依赖 tabs，历史 prompt-kit Markdown 增加静态 memo 边界。接口回归固定“打开并切换 15 个文件、改变右栏宽度时，命令消费者与历史 Markdown 不重渲染”，避免文件操作重新解析完整会话历史或重复创建 CodeMirror/Streamdown 子树。右侧 Tab 条的 `ResizeObserver` effect 只在执行测量时读取当前 ref，不允许闭包强持有已经 detach 的旧 tab strip DOM；cleanup 继续统一断开 observer 并取消待执行 rAF。

2026-08-04 进一步收敛窗口连续缩放：移除 shell resize 热路径中的逐帧右栏 `getSize/resize` 和 `onResize` 像素跟踪，连续几何只由面板库计算；右栏可见时由其独占窗口尺寸增量，并受用户首选宽度上限约束。文件区因此在扩展和收缩时都随实际宽度即时跨过单双栏阈值，不再等待窗口拖拽结束，也不会与应用主动恢复形成双布局反馈。

2026-08-03 已完成 MVP 实现，并通过 Rust 文件服务专项测试、前端全量回归、生产构建及本地真实页面的浅色、深色、双栏和窄屏验证。同日补齐桌面 bootstrap 的 `rightWorkspace/file` 配置投影与序列化契约测试，确保隐藏启动窗口不会因真实 IPC 数据缺字段导致首屏渲染中断。后续修正目录滚动锚点、内容事件导致整树刷新、右栏宽窄往返丢失偏好宽度、540px 双栏阈值、GFM 表格、紧凑排版、目录筛选工具栏焦点态、Nucleo 模糊搜索相关度及 README 安全 HTML 子集；搜索结果上限现在通过全局 Top-K 应用于全部有效候选，不再由文件系统遍历顺序决定候选集合。窗口回拉时，右栏按扣除当前可见左栏与中栏最小宽度后的真实剩余空间渐进恢复，空间足够后才恢复完整偏好宽度。若右侧文件区已经进入双栏，左侧导航必须等到恢复后仍能保住 540px 文件双栏时再出现，保证增宽过程的布局单调变化，禁止双栏闪烁或松手反跳。文件体验的第二轮收口进一步统一了低强调主题化会话文件入口、可确认的首次行号定位、32px 整行树命中区、无边框选中态和主题文件夹图标；树重挂载不再重复 reveal，底部滚动使用稳定 gutter。Markdown 改为单 View 稳定扩展模型，真实长文档表格、模式切换语义视口、README 注释和网络图片超链接由自动化契约覆盖；本地图片 preview grant 支持续期、失败保留和重新签发，正文基准字号为 14px。第三轮收口把定位意图按文档身份隔离；Markdown 相对链接统一以当前文档目录解析，响应式表格不再撑宽详情；虚拟树按 content-box 高度布局，从根因移除底部伪滚动区。第四轮根据真实反馈将模式切换锚点改为顶部逻辑块与像素偏移；行号定位删除了与 CodeMirror 竞争的外层坐标验收和手动 `scrollTop` 校正，统一由原生 `EditorView.scrollIntoView` effect 调度；README badge 严格按外层目标分流，外链接入 Tauri opener；Markdown 与源码高亮改用可读语义色，目录树选中态及 overscan 随主题和视口动态变化，并新增首次、已打开文件及重复点击同一 `#L47`、实时预览定位和四类 badge 目标的组件契约。第五轮将模式切换恢复也收敛到 CodeMirror 原生滚动 effect，移除对新 View 懒测量高度的读取、固定帧重试和手动 `scrollTop` 写入，避免渲染 → 源码 → 渲染往返时语义块向下漂移。第六轮从生命周期根因移除模式 `key` 与 EditorView 重建，改为固定扩展拓扑、显式 Compartment 原地重配置及视口图片预解码。第七轮进一步按领域生命周期拆分语言与展示 Compartment，Markdown/GFM parser 在源码/预览往返期间保持稳定，Atomic table 重新挂载时直接消费已持续解析的语法树。第八轮补齐 range widget 的双向视口映射：表格/图片内部进度映射为源码语义位置，反向切换由稳定 `scrollHandler` 在 CodeMirror 新布局测量完成后恢复块内位置，并以 1px 内缩采样消除源码行浮点边界歧义；同一 View 身份、真实 Todo 长表格往返、widget 内部进度、图片 decode 和普通语义锚点均由接口测试固化。

### 2026-08-04 会话历史版本与 Diff 资源

- 右侧工作区增加 `file-version`、`file-diff` 与 `conversation-asset` 三类只读资源。历史版本 key 包含 change set/change identity，同一路径不同 turn 不复用错误内容；消息附件和 artifact key 包含完整 attempt/branch locator。
- `file-diff` 使用官方 `@codemirror/merge` 的 `unifiedMergeView`，固定只读、无 merge controls，开启 gutter、变化高亮和未修改区折叠。标题使用“本轮修改 Diff”，表明比较的是本 Prompt Turn 第一次 tool diff 的 oldText 与最后一次 tool diff 的 newText，而不是 live workspace；仅当官方 changed chunk 数量至少为 2 时展示上一处/下一处导航。viewer 必须跟随右侧工作区容器宽度并启用 `EditorView.lineWrapping`，长行在当前可视宽度内换行，不产生页面级横向滚动。普通文件与 diff 复用同一语言加载、主题和 syntax highlight extension；新增片段的主题选择器必须命中同一编辑器根节点 `&.cm-merge-b`，显式移除 merge 默认 background image，只保留实色语义背景。只读 diff/version viewer 不安装 CodeMirror 自绘 selection layer，使用应用级 `--text-selection` / `--text-selection-foreground` 原生选中态，避免 diff 标记背景遮挡深色模式选区；普通 CodeMirror 自绘选区也必须使用同一 selection token。
- 打开 `file-diff` / `file-version` 属于普通只读浏览，即使捕获或渲染存在限制也不得显示 Tab 黄点；限制仅在 viewer 内说明。变更列表的“修改”文件图标使用主题 `gold-running` 蓝色语义 token，不使用固定琥珀色；新增/删除仍使用各自的成功/破坏性语义色。
- ACP `oldText/newText` 必须是文件内容，不得包含 unified diff 的 `No newline at end of file` 元数据。若 provider 的后续 tool update 错把该标记混入标准文本字段，捕获层需要移除标记并恢复真实的文件末尾换行状态；已有 change set 通过 schema 迁移从 durable journal 重新生成，不把元数据伪装成普通删除/新增行。
- 变更卡收起时只展示配置数量的预览行；展开后全部文件进入同一个 ScrollArea，预览行不得固定在滚动区外。标题固定使用“本轮变更 N 个文件”，partial 不在标题后追加告警图标。
- 变更卡从 change set summary 的加载占位切换到文件清单时，首帧必须直接采用最终收起结构；初始关闭不触发 `CollapsibleContent` 的退出动画，避免完整清单先参与绘制再收起。展开/收起动画只在用户操作折叠入口后启用，异步数据到达本身不改变 disclosure 意图。
- `get_turn_file_change_set` 与 `get_file_comparison` 只接受受控 attempt locator、branch、changeSetId/changeId；后端校验标识符、branch ownership 和 CAS hash，不接受前端提交任意 blob 路径或 runtime 绝对路径。
- `configs/app-config.toml` 的 `turnFiles` 统一管理卡片预览数、捕获条目/字节上限与 diff 渲染上限；CAS 不启用额外内存 blob cache，blob 生命周期跟随 attempt。
- 未来 Git commit/tree/blob 比较继续返回同一 `FileChange/FileComparison` 前端模型并复用 unified viewer；外部 Git 命令必须通过后台进程 helper，本期不提供 Git UI。
