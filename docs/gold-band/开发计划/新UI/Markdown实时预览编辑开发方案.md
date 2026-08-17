# Markdown 实时预览编辑开发方案

## 1. 元信息

- 状态：已实施（2026-08-03 二轮收口）
- 适用范围：右侧工作区中的 `.md`、`.markdown` 文件
- 上层方案：[右侧工作区文件系统与实时编辑开发方案](./右侧工作区文件系统与实时编辑开发方案.md)
- 产品约束：[右侧工作区文件浏览与编辑](../../产品设计文档/interaction/app/workspace-files.md)
- 核心依赖：`@atomic-editor/editor` `0.6.2`（精确锁定）

## 2. 目标

在现有文件工作区内增加 Markdown 实时预览编辑模式，同时保留完整源码编辑能力：

1. Markdown 默认以实时预览编辑模式打开。
2. 用户直接在渲染后的标题、列表、强调、链接、代码块和表格等内容中编辑；当前编辑位置按组件成熟交互显露必要 Markdown 标记。
3. 编辑器右上角提供“一键复制源码”和“切换源码/实时预览”两个悬浮按钮。
4. 两种模式共享同一份 Markdown 源码、自动保存队列、文件 revision、选区、原生撤销历史和同一个永久 `EditorView`；切换通过稳定 Compartment 原地重配置并恢复语义视口，不重建 View，也不维护双编辑器。
5. Markdown 内图片正常展示，但本地图片只能通过现有 `gold-band-preview://token` 安全协议加载，不能把本地路径直接交给 WebView。
6. 工作区内文件和用户显式打开的工作区外 Markdown 都允许编辑和近实时自动保存。

## 3. 非目标

本期不实现：

- 将 Markdown 转换为 HTML、富文本或 AST 后再反向序列化保存。
- 多人协作编辑、评论、修订模式。
- Mermaid、数学公式和任意 HTML 的完整执行环境。
- 网络图片的无提示自动加载。
- Markdown 图片上传、裁剪和资源重命名。
- 为标题标记设计特例，例如强制“标题文字删空后必须立即显示 `##`”。标记显隐采用 Atomic Editor 的成熟规则，验收关注语义可编辑性、源码一致性和撤销连续性。

## 4. 根因与设计判断

当前文件编辑器已经具备正确的基础设计：

- `FileContentStore` 是文件内容、保存状态、revision、外部授权和编辑器状态的统一生命周期所有者。
- `WorkspaceFileEditor` 使用 CodeMirror 6，并保存 `historyField`，Tab 切换不会丢失撤销历史。
- 编辑通过 300ms 合并、单文件串行写入、expected revision 和 watcher operation id 防止乱序覆盖与自身写回环。
- 图片已经通过 Rust 校验后签发短期 preview grant，WebView 不接收可任意读取本地文件的路径。

因此问题不是文件领域或保存模型的根本缺陷，而是通用源码编辑器缺少 Markdown 的视图层能力。Markdown 原文必须继续只有一份，并由 `FileContentStore` 统一保存；预览只能是 CodeMirror decoration/widget，不创建富文本状态，也不引入 DOM→Markdown 反序列化链路。

实际接入确认问题来自 React 层以整套 extensions props 和 `key` 管理模式生命周期，而不是 Atomic 必须绑定独立 View。真实 DOM 契约证明 Atomic 0.6.x 的 table、inline preview 和图片扩展可通过显式 `Compartment.reconfigure()` 在同一 View 内完整往返。进一步用真实 `功能点todo列表.md` 复现出：若把 Markdown/GFM parser 也塞进模式 Compartment，源码切回预览时 parser 会被重新挂载，Atomic table `StateField` 在长文档增量语法树尚未恢复时可能得到空 decoration，最终停留为原始 Markdown。最终方案固定 CodeMirror 基础扩展拓扑，并按领域生命周期拆成语言、模式、编辑策略三个长期存在的 Compartment：parser 随文档稳定，只有 table、inline preview、图片 decoration 与源码 UI 随模式切换。普通文本使用源码位置与块内像素偏移；table/图片 range widget 使用“源码范围 + widget 内进度 + 语义位置”，先由原生 scroll effect 揭示目标范围，再由固定 `scrollHandler` 在 CodeMirror 新布局测量完成、准备消费滚动目标时按真实 widget 几何恢复组件内部位置；巨型 widget 达到单次 viewport 稳定上限时，下一布局帧只补发一次官方 measure 调度，不做轮询。反向切换在用户未滚动时继续使用同一锚点，源码视口顶部向内 1px 采样以消除浮点边界歧义。图片授权状态通过 `StateField + StateEffect` 更新，源码切预览前预解码当前视口附近的安全图片 URL。该边界复用 CodeMirror/Atomic 的扩展生命周期、增量语法树、官方滚动扩展点和虚拟滚动能力，不 fork 上游、不复制表格实现，也不支付重建或双编辑器成本。

## 5. 开源组件结论

采用 `@atomic-editor/editor`：

- 基于 CodeMirror 6，能以 decoration/widget 方式实现 Obsidian 风格实时预览。
- Markdown 原文始终保存在 `EditorState.doc`，预览只改变显示，不改变保存格式。
- 可独立使用 `inlinePreview`、`tables`、`highlightMarkdown` 等公开扩展，无需替换现有 React CodeMirror 容器。
- React 18/19 与现有 CodeMirror peer 范围兼容。

接入约束：

1. 锁定精确版本 `0.6.2`，升级必须先跑 Gold Band 回归测试。
2. 不直接使用 Atomic Editor 的 React 包装器；Gold Band 统一控制内容、只读、保存和文件 revision。源码 / 预览是同一编辑器的两个稳定 extension profile，不拥有两份 View 或独立文件状态。
3. 不启用其默认 `imageBlocks()`；该扩展会把 Markdown 原始地址直接赋给 `<img src>`，不满足本地文件安全边界。
4. 不直接采用完整 Atomic 主题。只使用行为扩展，并通过 Gold Band CSS token 适配视觉。
5. Markdown/Atomic 依赖保留在现有文件工作区动态 chunk，不能增加会话首屏主 chunk。

## 6. 用户交互

### 6.1 模式

```ts
type MarkdownEditorMode = 'live-preview' | 'source'
```

- `.md`、`.markdown` 默认 `live-preview`。
- `live-preview`：隐藏行号和折叠 gutter，使用文档阅读宽度；光标进入结构时显露编辑所需标记。
- `source`：沿用当前 CodeMirror 源码编辑体验、行号、语法高亮和查找。
- 模式按打开文件的 resource key 保存在 `FileContentStore` 运行期状态，不写入 Tab，不持久化到磁盘。
- 超过实时预览字符阈值时强制源码模式，并展示可理解的降级提示。

### 6.2 悬浮操作

内容区域右上角固定两个轻量图标按钮：

1. 复制源码：复制当前 `EditorView.state.doc.toString()`，即包括尚在自动保存等待期内的最新内容，不能重新读取磁盘 snapshot。
2. 切换模式：在 `live-preview` 与 `source` 间切换。

按钮使用 shadcn/ui `Button`、现有 Tooltip 和 Tailwind：

- 必须有 `aria-label` 与 tooltip。
- 不覆盖文件头中的保存状态。
- 键盘可聚焦，焦点样式清晰。
- 复制成功或失败采用轻量反馈，不增加常驻卡片。

### 6.3 编辑与撤销

- 任一时刻只存在一个身份稳定的 `EditorView`。切换不序列化或重建 `EditorState`；doc、selection、history 天然保持在原实例中。切换前捕获统一视口锚点：普通文本记录源码位置与块内像素偏移；Atomic table 记录 Table 源码范围、渲染行索引和行内进度，其他 range widget 才使用组件内部相对进度。表格行通过稳定 Markdown parser 与 Atomic `thead/tbody/tr` 结构双向映射，不能用整个 widget 的像素百分比乘以整个源码范围字符数。用户未滚动到其他源码行时，往返继续保留原 widget 行锚点；编辑文档或滚动到其他源码块后重新采样。
- 基础扩展拓扑只初始化一次，React CodeMirror 的 `extensions` 与 `basicSetup` props 在模式切换期间保持稳定。Markdown/GFM parser 单独进入语言 Compartment 并贯穿文档生命周期；源码/预览 profile 只包含展示层扩展并进入模式 Compartment。同一个模式 transaction 提交 `Compartment.reconfigure()` 与 `EditorView.scrollIntoView(position, { y: 'start', yMargin })`；range widget 由基础拓扑中的 `EditorView.scrollHandler` 在 decoration 与 viewport 测量稳定、原生滚动目标即将执行时读取真实 block 几何并一次恢复内部位置。巨型 widget 若使首轮测量达到 CodeMirror 稳定次数上限，下一布局帧只补发一次 `requestMeasure()` 以继续消费待处理目标；不得使用固定 rAF 次数、定时器或 ResizeObserver 重试。
- 两种模式共享同一原生 `Ctrl/Cmd+Z` / redo history；Atomic table 与图片 decoration 只由显式 Compartment transaction 切换，不通过 React props 动态重组，也不通过 `key` 销毁 View。
- 输入继续调用现有 `onChange`，进入 `FileContentStore.updateText()` 与自动保存链路。
- `Ctrl/Cmd+S`、失焦、切 Tab 和关闭继续 flush 当前内容。
- 保存失败或磁盘冲突时保留内存内容和撤销历史，沿用现有恢复流程。

## 7. 数据结构与领域归属

### 7.1 前端运行期状态

```ts
type MarkdownImageState =
  | { kind: 'loading'; rawSrc: string }
  | { kind: 'ready'; rawSrc: string; canonicalPath: string; previewGrant: { token: string; expiresAtMs: string }; width: number; height: number }
  | { kind: 'approval-required'; rawSrc: string; canonicalPath: string; reason: MarkdownImageApprovalReason }
  | { kind: 'blocked'; rawSrc: string; limitationCode: string }
  | { kind: 'error'; rawSrc: string; errorCode: string }

interface MarkdownRuntimeState {
  mode: MarkdownEditorMode
  images: Map<string, MarkdownImageState>
  approvedExternalTargets: Set<string>
}
```

归属规则：

- 可编辑项目文件的 `MarkdownRuntimeState` 与文件 Tab 的内容、授权和撤销历史同生命周期，归 `FileContentStore` 管理。
- 运行目录、会话附件等只读 Markdown 只保留以完整 `documentKey` 为身份的瞬时 mode，由统一只读文档适配器管理；文档身份变化时自然重建该会话，不进入全局 Context，也不复制内容或授权状态。
- React 组件只订阅并渲染状态，不持有权威 preview token 集合。
- 关闭 Tab、LRU 淘汰、重新加载文档或图片引用被删除时，store 释放不再使用的 preview token。
- preview grant 到期前由 store 按当前引用批量轮换；新 grant 原子替换状态后才释放旧 token。续签失败保留当前图像并按退避时间重试，图片 `error` 与页面重新可见可触发幂等补发。
- 外部 Markdown 本身的 exact-file grant 只授权该 Markdown 读写，不能隐式扩展为图片目录授权。

### 7.2 Rust 领域模型

```rust
pub struct ResolveMarkdownImageInput {
    pub project_id: String,
    pub markdown_canonical_path: String,
    pub markdown_external_access_token: Option<String>,
    pub raw_src: String,
    pub approved_external_targets: Vec<String>,
}

pub struct WorkspaceFilePreviewGrantVm {
    pub token: String,
    pub expires_at_ms: String,
}

pub enum MarkdownImagePreviewVm {
    Ready {
        canonical_path: String,
        preview_grant: WorkspaceFilePreviewGrantVm,
        mime_type: String,
        width: u32,
        height: u32,
        animated: bool,
    },
    ApprovalRequired {
        canonical_path: String,
        reason: String,
    },
    Unsupported {
        limitation_code: String,
    },
}
```

前端只能提交 `projectId + markdownCanonicalPath + rawSrc + 当前文档授权集合`。最终路径解析、canonicalize、symlink 边界、格式识别和授权判断全部由 Rust 权威执行。

## 8. Atomic Editor 接入

### 8.1 扩展组合

实时预览模式按需加载：

```text
CodeMirror markdown language（GFM）
  + Atomic highlightMarkdown
  + Atomic inlinePreview
  + Atomic tables
  + GoldBand markdownImagePreview
  + Gold Band theme/readOnly/save/target-line extensions
```

源码 profile 不挂载 Atomic decoration、table widget 和图片 widget；Markdown/GFM 语法不属于源码 profile，而由独立语言 Compartment 在两种模式间稳定共享。切换时只原地重配置模式 Compartment，不销毁 View、不重启 parser，也不保留后台预览实例。

合法 GFM 表格始终启用 `Atomic tables`。安全降级精确到“表格自身包含 Markdown 图片”，不能因为文档其他位置存在图片而关闭全部表格。含图片表格暂时显示 Markdown 原文，避免 Atomic 内部把未经授权的 `src` 交给原始 `<img>`。widget 表格使用详情容器 `width: 100% + table-layout: fixed`，长内容在单元格内部换行，不能使用 `max-content` 撑宽编辑器。

链接点击统一路由：

- HTTP/HTTPS/mailto 交给现有安全外链能力。
- 本地文件链接复用工作区文件链接解析与右侧 Tab 打开能力；实时预览中的相对链接以当前 Markdown canonical path 的父目录为基准，会话消息中的相对链接仍以项目根为基准。
- 网络图片 badge 严格执行外层 Markdown 目标，不按 badge 类型写特例；本地文件、同文档锚点和外部 URL 分域处理，外部 URL 使用 Tauri opener 而不是 WebView `window.open`。
- 不直接调用 `window.open` 打开本地路径。

### 8.2 样式适配

- 内容颜色、muted、border、primary、code background 全部映射 Gold Band CSS variables。
- 实时预览正文使用 UI 字体和约 `65–75ch` 阅读宽度；代码块和行内代码使用 mono 字体。
- Markdown 标题接近正文，通过字重、轻量间距和细分隔表达层级，不使用文档页大标题样式。
- 深色主题避免多层黑色卡片和强边框。
- CodeMirror 虚拟滚动要求通过 line padding 表达垂直节奏，不对 `.cm-line` 使用会破坏高度测量的 margin。
- 正文固定使用稳定的 Gold Band 阅读字体栈、14px 基准字号和约 78ch 阅读宽度；标题、表格和内容 padding 使用紧凑相对层级，不继承可能用于代码的展示型自定义字体。

### 8.3 README 安全 HTML 子集

- 支持单行 `<div|p align="left|center|right">`、对应闭合标签、`<br>` 和单行 `<img ...>` 的视图解释。
- 白名单标签只生成 CodeMirror decoration、line class 和安全图片 widget，不执行 HTML，不使用 `dangerouslySetInnerHTML`。
- 本地 HTML `<img src>` 与 Markdown 图片共用 `resolve_markdown_image`、preview grant 和外部精确授权；网络图片不调用该接口，也不主动发起追踪请求。
- 实时预览中白名单 HTML 源码隐藏；需要修改标签时切换源码模式。未列入白名单的 HTML 保持源码显示。
- Markdown `CommentBlock`（包括 README-I18N 边界标记）在实时预览中隐藏；fenced code 内的注释仍作为代码内容展示。网络普通图片显示以 alt 命名、指向图片 URL 的普通链接；链接图片 badge 显示以 alt 命名、指向外层目标的普通链接。

## 9. Markdown 图片安全协议

### 9.1 支持来源

| 图片来源 | 默认行为 |
|---|---|
| 当前工作区内的相对或绝对本地图片 | 自动解析并安全展示 |
| 工作区外 Markdown 同目录及子目录的相对图片 | 自动解析并安全展示 |
| 外部 Markdown 的 `../`、其他盘符或目录外绝对图片 | 当前文档统一确认一次，只授权实际引用的精确文件 |
| UNC/网络共享路径 | 默认阻止自动加载 |
| `http://`、`https://` 网络图片 | 不渲染、不下载，保留为普通超链接 |
| `data:`、`javascript:`、任意 HTML 注入 | 拒绝 |

“文档统一确认”不是开放目录：前端先解析当前文档内待确认引用，用户确认一次后，只把该批 canonical path 放入当前 Markdown runtime 的授权集合。新增的目录外引用需要再次确认。

### 9.2 本地图片流程

```text
Markdown Image src
  -> projectId + markdown locator + raw src
  -> Rust 解析并 canonicalize
  -> 校验工作区/文档目录/精确授权
  -> 文件签名、字节数、像素数校验
  -> SVG 由 Rust 栅格化，禁用外部资源
  -> 签发绑定 path + revision 且携带 expiresAtMs 的短期 preview grant
  -> <img src="gold-band-preview://token">
```

必须保证：

- WebView 不接收 `file://`，图片 DOM 不保存本地绝对路径。
- 不信任扩展名和传入 MIME，以 Rust 文件签名探测为准。
- SVG 不进入 DOM，不执行脚本，不加载字体、图片或网络资源。
- token 到期、文件 revision 变化、引用删除或 Tab 关闭后失效；前端必须在到期前轮换，不能让 widget 永久持有首次签发的 token。
- 同一文档相同 canonical path 复用 grant；并发解析受配置限制。

### 9.3 编辑过程

- 图片 widget 出现在 Markdown 图片源码所在行下方。
- 光标进入图片源码行时显露 `![alt](src)` 以便修改。
- 修改 `src` 后撤销旧解析请求；过期异步响应通过 request revision 忽略。
- 图片加载失败显示小型占位和结构化原因，不能让编辑器崩溃。

## 10. 接口与错误模型

新增 API：

```ts
resolveMarkdownImage(input: ResolveMarkdownImageInput): Promise<MarkdownImagePreviewVm>
```

继续复用：

- `releaseWorkspaceFilePreview(token)`：释放图片 token。
- `workspaceFilePreviewUrl(token)`：生成受限协议 URL。
- 现有 `CommandErrorVm { code, params }`；Rust 不返回对客文案。

新增错误/限制码建议：

| code | 含义 |
|---|---|
| `workspace-file.markdown-image-src-invalid` | 图片 src 不是允许的地址形式 |
| `workspace-file.markdown-image-outside-document-directory` | 需要当前文档级统一确认 |
| `workspace-file.markdown-image-network-blocked` | 网络图片意外进入本地解析接口时由后端防御性拒绝 |
| `workspace-file.markdown-image-unc-blocked` | UNC/网络共享路径不自动加载 |
| `workspace-file.markdown-image-reference-limit` | 单文档图片引用数量超限 |

文件不存在、权限不足、格式不支持、图片超大、像素超限、解码失败和 preview token 失效继续复用现有错误码。

## 11. 配置

所有阈值由 `configs/app-config.toml` 下发：

```toml
[workspaceFiles]
markdownLivePreviewMaxChars = 200000
markdownEmbeddedImageLimit = 100
markdownEmbeddedImageMaxConcurrent = 4
```

范围：

- `markdownLivePreviewMaxChars`：超过后只启用源码模式。
- `markdownEmbeddedImageLimit`：单文档最多解析的图片引用数。
- `markdownEmbeddedImageMaxConcurrent`：单文档并发解析/解码请求数。

## 12. 性能策略

1. Atomic 和 Markdown parser 只在打开 Markdown 时动态加载。
2. 源码/预览共享同一源码、保存状态与永久 View；模式切换由显式 Compartment transaction 原地完成，React 不替换扩展 props。进入预览前只预解码当前视口及有限 overscan 的本地图片，不扫描或等待整篇文档。
3. 图片只解析语法树识别出的 `Image` 节点，不用全量 DOM 扫描。
4. 文档修改仅重算受影响图片引用；普通文字输入不重复解析全部图片。
5. 相同 canonical path 去重，图片请求按配置限制并发。
6. 每个请求携带内容/request revision，忽略旧响应。
7. 超过 `markdownLivePreviewMaxChars` 时不挂载高成本 decoration/table/widget。
8. 不启用 Atomic 默认图片扩展，避免未授权网络或本地读取。
9. 语言、模式、编辑策略 Compartment 以及图片 StateField 配置均以已应用 profile 去重；首次 View 创建已包含的 profile 不得在挂载 effect 中重复 reconfigure/update，避免长表格 DOM 挂载后因无状态变化的 transaction 触发同步 selection/layout 测量。

## 13. 测试矩阵

### 13.1 前端接口与 store 测试

- `.md` 默认实时预览，其他文本不受影响。
- 运行目录与会话附件的 `.md` 复用统一只读适配器，保持 `editable=false`，支持实时预览/源码切换，并在超阈值时固定源码模式。
- 模式按 resource key 保存，关闭资源后释放。
- 切换模式不改变源码；同一 View 原地重配置后保持 selection、undo history 与语义视口锚点。
- 复制的是当前内存源码，不是落盘旧内容。
- 输入、撤销、redo 继续进入原自动保存队列。
- 超阈值 Markdown 自动降级源码模式。
- 图片引用删除、变更、reload、LRU 和 Tab 关闭均释放 token。
- 过期图片解析响应不能覆盖新引用状态。
- preview grant 在到期前轮换；续签失败保留旧状态并退避重试，图片加载错误只对仍为当前 token 的 widget 触发补发。
- 网络图片和 badge 不进入本地解析接口，只生成目标正确的普通超链接。
- badge 外层为 `LICENSE`、`../LICENSE` 等本地目标时，点击由工作区 handler 接管，WebView 不得解析成应用的 localhost 页面。

### 13.2 Rust 接口测试

- 工作区内相对路径、绝对路径和 URL 编码路径可解析。
- `..`、symlink、其他盘符经过 canonicalize 后重新判定边界。
- 外部 Markdown 同目录/子目录相对图片自动允许。
- 外部 Markdown 的目录外图片未在精确授权集合时返回 `approvalRequired`，授权后仅该路径可读取。
- Markdown external grant 缺少、过期或绑定其他文件时拒绝解析。
- UNC、网络 URL、`data:`、`javascript:` 按策略阻止。
- 伪造图片扩展名、损坏图片、超大字节、超大像素安全降级。
- SVG 返回栅格化 preview，绝不返回 SVG 源码 DOM。

### 13.3 组件验收

- 右上角两个悬浮按钮可见、可键盘操作、tooltip 与 aria-label 正确。
- 标题、强调、列表、任务项、链接、代码块和表格可直接编辑。
- 合法 GFM 表格在首次打开以及“源码→预览”往返后都保持 table widget；文档其他位置含图片不影响表格。
- README 白名单对齐标签不显示为正文，本地 HTML 图片走 preview grant，远程 badge 不静默联网且保留外层目标链接。
- 删除和新增标题文本后，最终 Markdown 源码语义正确；标记显隐按 Atomic 成熟交互执行。
- 两种模式共享 `Ctrl/Cmd+Z` 与 redo history；普通文本切换后通过 CodeMirror 原生 scroll effect 恢复顶部语义源码块。渲染 → 源码 → 渲染往返后，原顶部语义块仍须位于视口顶部；当视口位于 table 内部时，源码态必须定位到对应 Markdown 表格行，反向切换恢复同一 `<tr>` 及行内进度，不能退化为整表字符百分比或 widget 起点；图片等其他 range widget 继续恢复同一源码语义进度。真实几何只允许在 CodeMirror `scrollHandler` 的稳定滚动阶段读取并一次提交；源码顶部边界采样不得因亚像素误差误判用户已滚动，不允许固定帧复测。
- 图片通过 `gold-band-preview://token` 展示，DOM 中不存在本地路径。
- 外部目录图片只出现一次文档级确认，不逐图片打断。
- 深色/亮色、宽屏/窄屏、长文档和多图片滚动正常。
- 目标行链接打开 Markdown 后仍定位到正确源码位置并滚动到视口中央；同一链接重复点击必须建立新定位 revision。文档实例使用 `documentKey + contentRevision + 当前 EditorView ref` 判定，不得比较 CodeMirror 已规范化换行的全文与原始 CRLF 文本。`onCreateEditor` 不消费定位；受控 value 同步后的 React effect 通过同一个 transaction 提交 selection 和 `EditorView.scrollIntoView(range, { y: 'center' })`。不得在 Adapter 中增加 rAF/ResizeObserver 等待，也不得用 `coordsAtPos`、估算块高度或直接写 `scrollTop` 重复实现 CodeMirror 的虚拟滚动。target revision 不得触发 Markdown 扩展重载或 EditorView 重建；已消费 revision 按 document key 隔离。

## 14. 分阶段实施

### 阶段 1：状态、配置与 Atomic 接入

- 精确锁定依赖。
- 增加 Markdown mode/runtime 状态和配置下发。
- 在现有 `WorkspaceFileEditor` 中动态挂载 Atomic 扩展。
- 固化模式切换、源码一致和撤销历史测试。

### 阶段 2：悬浮操作与视觉适配

- 增加复制源码、模式切换按钮。
- 增加中英文文案、tooltip、aria-label 和反馈。
- 使用 Gold Band token 适配实时预览样式。

### 阶段 3：安全图片

- 新增 Rust Markdown 图片解析接口与结构化响应。
- 实现 Gold Band CodeMirror image widget。
- 接入文档级精确授权、token 去重/释放和请求 revision。
- 补齐路径、安全、格式与生命周期测试。

### 阶段 4：验证与交付

- 自动化验证编辑、撤销、自动保存、模式 profile、首次目标行、真实长文档表格、网络图片链接和 preview grant 生命周期。
- 按本轮需求不执行浏览器验证；真实主题、宽窄布局与视觉验收由需求方完成。
- 同步产品设计文档与上层开发计划完成状态。

## 15. 完成标准

- [x] Markdown 默认实时预览编辑，源码模式可随时切换。
- [x] 两种模式共享源码、revision、selection、history 和自动保存链路；任一时刻只存在一个 EditorView，并恢复语义视口锚点。
- [x] 右上角复制源码与模式切换按钮完成无障碍和轻量反馈。
- [x] 标题、列表、强调、链接、代码块、任务项和合法 GFM 表格可直接编辑。
- [x] Markdown 本地图片只通过安全 preview grant 展示，并在到期前轮换。
- [x] 工作区外 Markdown 同目录图片自动展示，目录外引用按文档统一确认且只授权精确引用。
- [x] SVG 安全栅格化；网络图片只保留超链接，UNC 和危险 scheme 不会静默加载。
- [x] 大文档、多图片、过期异步响应和 grant 生命周期符合性能与安全约束。
- [x] 前端、Rust 接口和组件测试覆盖关键验收。
- [ ] 真实 UI 的深浅主题、宽窄布局和编辑流程由需求方验证。
- [x] 产品设计文档和上层开发计划同步更新。

## 16. 2026-08-17 运行目录只读 Markdown 补齐

- [x] 新增统一只读 Markdown 文档适配器，集中管理只读属性、文档级 mode 和长度阈值。
- [x] 运行目录按 canonical path 识别 Markdown，不再把 `.md` 作为缺少 mode 的普通文本查看器打开。
- [x] 会话附件复用同一适配器，删除重复的 mode 重置 effect。
- [x] 增加组件与运行目录 DOM 契约测试，覆盖默认实时预览、源码切换、文档身份重置、只读属性和超阈值降级。
