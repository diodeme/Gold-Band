# 右侧工作区文件浏览与编辑

## 1. 产品定位

文件能力是会话右侧工作区的一类通用资源，不是 IDE、终端或独立一级页面。用户可以在快速对话和会话详情中浏览当前工作空间，查看文本、代码与图片，并通过会话中的本地文件链接直接打开对应文件。

## 2. 核心交互

- 右侧工作区没有 Tab 时展示“文件”入口；进入后始终以当前 `projectId` 对应的工作空间为目录根。
- 可用宽度不少于 620px 时，左侧显示文件详情，右侧显示目录树；打开文件能力时优先请求 760px 右栏宽度，右栏最大允许扩展到 960px。空间不足时切换为“文件 / 目录”单栏，不横向压缩两个不可用面板。
- 目录按层懒加载，使用虚拟化树与标准键盘语义；搜索在 Rust 后台执行并遵守 `.gitignore`，前端只接收有限结果。
- 文件右键菜单提供绝对路径和 `/` 分隔的工作空间相对路径复制。路径复制失败时保留当前树状态并显示可恢复提示。
- 文件 Tab 以 `projectId + canonicalPath` 去重。再次点击同一文件的不同链接位置只更新 `target/targetRevision`，不创建重复 Tab。
- 工作空间外文件仅能由用户显式点击会话本地文件链接打开。详情显示不可点击的绝对路径，右侧目录树仍属于当前工作空间；文本、代码、配置和 SVG 源码允许编辑，图片保持只读。

## 3. 查看格式

| 类别 | 内置能力 |
|---|---|
| 文本、日志 | CodeMirror 查看、查找、行号、换行和编辑 |
| Markdown | 默认实时预览编辑，可切换源码模式并一键复制当前内存源码；两种模式共享源码、撤销历史和自动保存队列 |
| 常见代码与配置 | CodeMirror 按需语言高亮；无语言包时回退纯文本 |
| PNG、JPEG、WebP、GIF、BMP、ICO | 安全图片预览、缩放、适应窗口、原始大小和拖拽平移；GIF 支持播放/暂停，并在 reduced motion 下默认显示静态首帧 |
| SVG | Rust 安全栅格化预览，可切换源码编辑 |
| PDF、Office、音视频、压缩包、字体、数据库及其他二进制 | 显示明确的不支持状态并提供系统应用打开 |

文件识别以签名、BOM 和内容探测为权威事实，扩展名只辅助选择图标与语言能力；PDF、压缩包等二进制即使碰巧可按 UTF-8 解码也不得进入文本编辑器。文本编码保证 UTF-8、UTF-8 BOM、带 BOM 的 UTF-16 LE/BE，保存时保留 BOM 与 CRLF/LF 语义；无法可靠解码的内容不做有损猜测，也不自动写回。大文件读取和 revision 计算使用流式处理，不为识别或哈希重复完整载入文件。

CodeMirror 不启用上游固定浅色主题。编辑器背景、正文、行号、选区、活动行和语法高亮统一引用 Gold Band 语义色 token，应用主题切换后立即继承当前浅色或深色外观，不维护独立 IDE 主题状态。

### 3.1 Markdown 实时预览编辑

- Markdown 使用 `@atomic-editor/editor` 的 CodeMirror 6 公开扩展实现实时预览，不创建 HTML/富文本副本，也不执行 DOM 到 Markdown 的反向序列化。
- 用户可直接编辑标题、强调、列表、任务项、链接、代码块和表格；当前结构的 Markdown 标记按成熟组件规则显露。
- 内容区域右上角提供“复制 Markdown 源码”和“源码 / 实时预览”两个悬浮按钮。复制内容取自当前 `EditorState.doc`，包含尚处于自动保存等待期的最新输入。
- 模式切换只重配 CodeMirror extension，不卸载编辑器；选区、滚动位置、撤销/重做和目标行定位保持连续。
- 超过配置阈值的 Markdown 自动降级源码模式，避免长文档 decoration、表格和图片 widget 影响输入性能。
- 详细数据、接口、安全和验收约束见[Markdown 实时预览编辑开发方案](../../../开发计划/新UI/Markdown实时预览编辑开发方案.md)。

## 4. 编辑与保存

- 文本修改先进入 CodeMirror 本地状态，300ms 合并后自动保存；`Ctrl/Cmd+S`、切换资源、收起工作区和关闭 Tab 会立即冲刷保存队列。
- 撤销、重做使用 CodeMirror 原生历史；撤销或重做得到的内容与普通输入走同一自动保存协议。编辑器历史在运行期随 `FileContentStore` 保留，关闭文件后释放。
- 正常编辑不展示长期 dirty 圆点，也不弹出未保存确认。只有保存失败或磁盘版本冲突时阻止关闭，并在文件头部提供重试、重新载入、重新授权或显式覆盖操作。
- 后端写入必须携带读取时的 `FileRevisionVm`。revision 不一致时不修改磁盘，前端进入 conflict；所有写入使用原子替换并保留原文件权限。
- Rust `notify` 事件区分自身 `operationId/revision` 与外部写入。干净文件自动重新载入；存在本地修改时暂停保存并进入冲突状态。
- 保存失败或进入冲突后，后续输入只更新内存中的最新内容，不得隐式重试写盘或绕过冲突；只有用户明确选择重试、重新授权或覆盖后才恢复保存。
- 应用正常关闭、项目切换和项目删除前统一冲刷相关文件的保存队列；冲刷失败时保留运行期内容并阻止破坏性切换。

## 5. 权限与安全

- 前端只提交 `projectId + canonicalPath`；Rust 权威解析项目根、规范化路径并判断工作空间内外。
- 工作空间外文件使用绑定单个 canonical path、项目、读写权限和 TTL 的 access grant。token 不进入 Tab、持久化、日志或 URL，关闭 Tab 时主动释放。
- 工作空间外 watcher 在每批事件发出前重新校验并读取轮换后的 grant；授权过期或释放后立即停止向前端发送该文件事件。
- 图片不使用 `file://`。Rust 完成文件签名、字节数、像素数和 revision 校验后签发短期 preview token；SVG 禁止原始 DOM 注入和外部资源加载。
- Markdown 图片不使用组件默认的原始 `<img src>`：工作空间内图片以及工作空间外 Markdown 同目录/子目录相对图片自动签发 preview token；文档目录外引用按当前文档统一确认一次，且只授权文档实际引用的精确文件。UNC、网络图片和危险 scheme 不静默加载。
- 所有文件错误继续使用 `CommandErrorVm { code, params }`；Rust 不产生对客文案，前端同步维护中英文恢复提示。

## 6. 领域与性能边界

- `RightWorkspaceState` 只保存轻量资源 locator、Tab 与激活态。
- `FileExplorerStore` 管理树、展开状态、目录缓存、搜索请求序号和 watcher 失效刷新。
- `FileContentStore` 管理内容快照、CodeMirror 历史、preview/access token、自动保存串行队列、冲突与有限 LRU。
- Markdown 模式、内嵌图片解析状态、文档级精确授权和派生 preview token 与文件 Tab 同生命周期，也由 `FileContentStore` 统一管理。
- `WorkspaceFileService` 管理路径授权、目录/搜索、类型识别、读取、revision、原子写入、图片安全输出和 watcher。
- 文件面板、CodeMirror、语言支持与虚拟化文件树使用独立动态 chunk；未打开文件功能时不进入会话首屏。
- `configs/app-config.toml` 是工作区布局阈值的权威来源。桌面 `get_app_bootstrap.appConfig.workspaceLayout` 必须完整投影 `shellMinWidth/shellMinHeight`、`rightWorkspace` 及各页面 profile；`rightWorkspace.file` 与右栏宽度属于同一生命周期契约，不能只存在于前端类型或 browser mock。桌面 bootstrap 完成后前端直接消费真实契约，不增加缺字段 fallback。

## 7. 实现状态

2026-08-03 已完成 MVP 实现，并通过 Rust 文件服务专项测试、前端全量回归、生产构建及本地真实页面的浅色、深色、双栏和窄屏验证。同日补齐桌面 bootstrap 的 `rightWorkspace/file` 配置投影与序列化契约测试，确保隐藏启动窗口不会因真实 IPC 数据缺字段导致首屏渲染中断。
