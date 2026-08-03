# 右侧工作区文件浏览与编辑

## 1. 产品定位

文件能力是会话右侧工作区的一类通用资源，不是 IDE、终端或独立一级页面。用户可以在快速对话和会话详情中浏览当前工作空间，查看文本、代码与图片，并通过会话中的本地文件链接直接打开对应文件。

## 2. 核心交互

- 右侧工作区没有 Tab 时展示“文件”入口；进入后始终以当前 `projectId` 对应的工作空间为目录根。
- 可用宽度不少于 540px 时，左侧显示文件详情，右侧显示目录树；详情最小 280px、目录最小 200px。打开文件能力时优先请求 760px 右栏宽度，右栏最大允许扩展到 960px。空间不足时切换为“文件 / 目录”单栏，不横向压缩两个不可用面板。
- 目录按层懒加载，使用虚拟化树与标准键盘语义；展开目录保持当前滚动锚点，只有新选择/新定位的文件可以触发一次主动滚动。虚拟树 overscan 按实际 content-box 高度动态计算，至少保留 24 行并覆盖约两个视口，快速滚动时不得暴露尚未挂载的空窗；滚动本身不发起目录加载。搜索在 Rust 后台执行并遵守 `.gitignore`，前端只接收有限结果。匹配使用 `nucleo-matcher` 的 Unicode-aware 模糊评分，支持 `wft` 命中 `WorkspaceFileTree.tsx`；结果按“文件名完全匹配、文件名前缀、文件名模糊匹配、仅相对路径模糊匹配”分层，同层再按 Nucleo 分数、较浅路径和自然路径稳定排序。
- 目录筛选属于工作区工具栏控件，不属于 CodeMirror。它复用 shadcn/ui `Input` 的 `toolbar` 外观：静止时使用低对比度表面与边界，键盘聚焦时保留 1px 语义色焦点环和边界变化；表单输入继续使用标准的高可见焦点态，不能为了视觉弱化而全局移除无障碍焦点提示。
- 文件和目录共用紧凑右键菜单，提供绝对路径和 `/` 分隔的工作空间相对路径复制；菜单按 LTR 优先从点击点向右展开，空间不足时允许组件进行碰撞调整。复制路径不得激活文件、切换详情或改变树布局；成功不提示，失败使用不占据文档流的浮层提示。Windows 绝对路径对客与剪贴板统一移除 `\\?\` extended-length 前缀，UNC 路径恢复为标准 `\\server\share` 格式。
- 文件 Tab 以 `projectId + canonicalPath` 去重。再次点击同一文件的不同链接位置只更新 `target/targetRevision`，不创建重复 Tab。
- 会话中的本地文件链接使用轻量主题底色，文件类型图标单独使用语义强调色以保证对比度；不能只依赖下划线表达可点击。路径解析得到的 `target/targetRevision` 属于绑定 `projectId + canonicalPath` 的一次定位意图：相同链接每次点击都产生新 revision。Adapter 以 `documentKey + contentRevision + 当前 EditorView ref` 判断文档实例，不得用全文字符串相等判断，因为 CodeMirror 会规范化 CRLF。`onCreateEditor` 只初始化 View 插件；外部定位必须等受控 `value` 同步后的 React effect，再在同一个 CodeMirror transaction 中提交 selection 与官方 `EditorView.scrollIntoView(range, { y: 'center' })` effect。滚动测量、虚拟高度换算与视口更新完全交给 CodeMirror，不得用 rAF/ResizeObserver 轮询、`coordsAtPos`、估算块高度或直接写 `scrollTop` 实现第二套滚动器。transaction 成功 dispatch 后按文档身份消费 revision，文件切换不能沿用其他文件的已消费 revision。
- 工作空间外文件仅能由用户显式点击会话本地文件链接打开。详情显示不可点击的绝对路径，右侧目录树仍属于当前工作空间；文本、代码、配置和 SVG 源码允许编辑，图片保持只读。

## 3. 查看格式

| 类别 | 内置能力 |
|---|---|
| 文本、日志 | CodeMirror 查看、查找、行号、换行和编辑 |
| Markdown | 默认实时预览编辑，可切换源码模式并一键复制当前内存源码；两种模式共享源码、选区、撤销历史和自动保存队列 |
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
- Markdown 始终只挂载一个、身份稳定的 CodeMirror `EditorView`。源码 / 预览是同一编辑器的配置状态，不参与 React `key`，也不通过 props 替换整套扩展。基础扩展拓扑固定，模式能力与编辑策略分别由稳定 `Compartment` 管理；切换前把视口顶部映射成“顶部逻辑源码块位置 + 相对视口的非负像素边距”，随后在同一个 transaction 中提交 `Compartment.reconfigure()` 与官方 `EditorView.scrollIntoView(position, { y: 'start', yMargin })` effect。CodeMirror 负责 decoration 更新、懒测量、虚拟高度修正与最终滚动；不能销毁 View、读取估算块高度自行换算、保存裸 `scrollTop`，也不能同时常驻两个编辑器。
- Atomic table、Markdown 图片与 README decoration 只在模式 Compartment 内显式重配置，不随 React extensions props 反复重组。图片授权状态继续由稳定 `StateField + StateEffect` 更新；源码切到预览前，对当前源码视口及有限 overscan 内已有 preview grant 的图片执行 `HTMLImageElement.decode()`，解码完成或明确失败后再原子提交模式 transaction。图片 URL 与 token 不因模式切换释放，禁止用截图、遮罩、淡入或固定延迟掩盖重挂载闪烁。
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

## 6. 领域与性能边界

- `RightWorkspaceState` 只保存轻量资源 locator、Tab 与激活态。
- `FileExplorerStore` 管理树、展开状态、目录缓存、搜索请求序号和 watcher 失效刷新。
- `FileExplorerStore` 同时区分用户滚动位置与一次性选中 reveal：侧栏收起/展开重挂载时恢复原滚动位置，同一选中路径不会再次自动居中；程序化恢复滚动不反写用户滚动快照。虚拟树高度必须使用扣除容器 padding 后的 content box，且目录树禁止横向滚动；再配合稳定 scrollbar gutter、关闭 scroll anchoring 和限制 overscroll 传播，避免底部存在被裁切的伪滚动区及随后的回弹震颤。
- `FileContentStore` 管理内容快照、CodeMirror 历史、preview/access grant、自动保存串行队列、冲突与有限 LRU。
- Markdown 模式、内嵌图片解析状态、文档级精确授权和派生 preview grant 与文件 Tab 同生命周期，也由 `FileContentStore` 统一管理。
- `WorkspaceFileService` 管理路径授权、目录/搜索、类型识别、读取、revision、原子写入、图片安全输出和 watcher。
- 文件面板、CodeMirror、语言支持与虚拟化文件树使用独立动态 chunk；未打开文件功能时不进入会话首屏。
- `configs/app-config.toml` 是工作区布局阈值的权威来源。桌面 `get_app_bootstrap.appConfig.workspaceLayout` 必须完整投影 `shellMinWidth/shellMinHeight`、`rightWorkspace` 及各页面 profile；`rightWorkspace.file` 与右栏宽度属于同一生命周期契约，不能只存在于前端类型或 browser mock。桌面 bootstrap 完成后前端直接消费真实契约，不增加缺字段 fallback。

## 7. 实现状态

2026-08-03 已完成 MVP 实现，并通过 Rust 文件服务专项测试、前端全量回归、生产构建及本地真实页面的浅色、深色、双栏和窄屏验证。同日补齐桌面 bootstrap 的 `rightWorkspace/file` 配置投影与序列化契约测试，确保隐藏启动窗口不会因真实 IPC 数据缺字段导致首屏渲染中断。后续修正目录滚动锚点、内容事件导致整树刷新、右栏宽窄往返丢失偏好宽度、540px 双栏阈值、GFM 表格、紧凑排版、目录筛选工具栏焦点态、Nucleo 模糊搜索相关度及 README 安全 HTML 子集；搜索结果上限现在通过全局 Top-K 应用于全部有效候选，不再由文件系统遍历顺序决定候选集合。窗口回拉时，右栏按扣除当前可见左栏与中栏最小宽度后的真实剩余空间渐进恢复，空间足够后才恢复完整偏好宽度。若右侧文件区已经进入双栏，左侧导航必须等到恢复后仍能保住 540px 文件双栏时再出现，保证增宽过程的布局单调变化，禁止双栏闪烁或松手反跳。文件体验的第二轮收口进一步统一了低强调主题化会话文件入口、可确认的首次行号定位、32px 整行树命中区、无边框选中态和主题文件夹图标；树重挂载不再重复 reveal，底部滚动使用稳定 gutter。Markdown 改为单 View 稳定扩展模型，真实长文档表格、模式切换语义视口、README 注释和网络图片超链接由自动化契约覆盖；本地图片 preview grant 支持续期、失败保留和重新签发，正文基准字号为 14px。第三轮收口把定位意图按文档身份隔离；Markdown 相对链接统一以当前文档目录解析，响应式表格不再撑宽详情；虚拟树按 content-box 高度布局，从根因移除底部伪滚动区。第四轮根据真实反馈将模式切换锚点改为顶部逻辑块与像素偏移；行号定位删除了与 CodeMirror 竞争的外层坐标验收和手动 `scrollTop` 校正，统一由原生 `EditorView.scrollIntoView` effect 调度；README badge 严格按外层目标分流，外链接入 Tauri opener；Markdown 与源码高亮改用可读语义色，目录树选中态及 overscan 随主题和视口动态变化，并新增首次、已打开文件及重复点击同一 `#L47`、实时预览定位和四类 badge 目标的组件契约。第五轮将模式切换恢复也收敛到 CodeMirror 原生滚动 effect，移除对新 View 懒测量高度的读取、固定帧重试和手动 `scrollTop` 写入，避免渲染 → 源码 → 渲染往返时语义块向下漂移。第六轮从生命周期根因移除模式 `key` 与 EditorView 重建，改为固定扩展拓扑、显式 Compartment 原地重配置及视口图片预解码；同一 View 身份、表格往返、图片 decode 和语义锚点均由接口测试固化。
