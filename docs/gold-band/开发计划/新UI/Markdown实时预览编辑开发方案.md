# Markdown 实时预览编辑开发方案

## 1. 元信息

- 状态：实施前方案
- 适用范围：右侧工作区中的 `.md`、`.markdown` 文件
- 上层方案：[右侧工作区文件系统与实时编辑开发方案](./右侧工作区文件系统与实时编辑开发方案.md)
- 产品约束：[右侧工作区文件浏览与编辑](../../产品设计文档/interaction/app/workspace-files.md)
- 核心依赖：`@atomic-editor/editor` `0.6.2`（精确锁定）

## 2. 目标

在现有文件工作区内增加 Markdown 实时预览编辑模式，同时保留完整源码编辑能力：

1. Markdown 默认以实时预览编辑模式打开。
2. 用户直接在渲染后的标题、列表、强调、链接、代码块和表格等内容中编辑；当前编辑位置按组件成熟交互显露必要 Markdown 标记。
3. 编辑器右上角提供“一键复制源码”和“切换源码/实时预览”两个悬浮按钮。
4. 两种模式共享同一份 Markdown 源码、自动保存队列和文件 revision；实时预览与源码视图分别保留原生 CodeMirror 撤销历史，切换不会销毁实时预览的表格、选区和滚动状态。
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
- 图片已经通过 Rust 校验后签发短期 preview token，WebView 不接收可任意读取本地文件的路径。

因此问题不是文件领域或保存模型的根本缺陷，而是通用源码编辑器缺少 Markdown 的视图层能力。Markdown 原文必须继续只有一份，并由 `FileContentStore` 统一保存；预览只能是 CodeMirror decoration/widget，不创建富文本状态，也不引入 DOM→Markdown 反序列化链路。

实际接入确认 Atomic 0.6.x 的 table `StateField` 在扩展动态移除后重新加入会保留失效装饰，导致源码/预览往返后表格退回原文。最终方案在源码模式期间保留并隐藏实时预览 CodeMirror，只临时显示源码 CodeMirror；两者受同一个受控 `value` 驱动，只有一个可见和可输入。该边界复用成熟组件能力，不复制上游约 1200 行表格实现。

## 5. 开源组件结论

采用 `@atomic-editor/editor`：

- 基于 CodeMirror 6，能以 decoration/widget 方式实现 Obsidian 风格实时预览。
- Markdown 原文始终保存在 `EditorState.doc`，预览只改变显示，不改变保存格式。
- 可独立使用 `inlinePreview`、`tables`、`highlightMarkdown` 等公开扩展，无需替换现有 React CodeMirror 容器。
- React 18/19 与现有 CodeMirror peer 范围兼容。

接入约束：

1. 锁定精确版本 `0.6.2`，升级必须先跑 Gold Band 回归测试。
2. 不直接使用 Atomic Editor 的 React 包装器；Gold Band 统一控制内容、只读、保存和文件 revision。源码模式的临时 CodeMirror 只是同一源码的另一种输入视图，不拥有独立文件状态。
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

- 实时预览实例在源码模式期间保持挂载但隐藏，避免 Atomic table StateField 因动态重配失效；源码 CodeMirror 只在源码模式挂载。
- 两个视图共享受控源码、文件 revision 和自动保存队列，只有当前可见视图响应输入。
- 两个视图都支持原生 `Ctrl/Cmd+Z` 与 redo；预览视图自身的选区、滚动和撤销历史跨模式保持，源码视图离开前序列化 history。跨视图不承诺把另一视图产生的编辑合并为同一 undo 栈。
- 输入继续调用现有 `onChange`，进入 `FileContentStore.updateText()` 与自动保存链路。
- `Ctrl/Cmd+S`、失焦、切 Tab 和关闭继续 flush 当前内容。
- 保存失败或磁盘冲突时保留内存内容和撤销历史，沿用现有恢复流程。

## 7. 数据结构与领域归属

### 7.1 前端运行期状态

```ts
type MarkdownImageState =
  | { kind: 'loading'; rawSrc: string }
  | { kind: 'ready'; rawSrc: string; canonicalPath: string; previewToken: string; width: number; height: number }
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

- `MarkdownRuntimeState` 与文件 Tab 的内容、授权和撤销历史同生命周期，归 `FileContentStore` 管理。
- React 组件只订阅并渲染状态，不持有权威 preview token 集合。
- 关闭 Tab、LRU 淘汰、重新加载文档或图片引用被删除时，store 释放不再使用的 preview token。
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

pub enum MarkdownImagePreviewVm {
    Ready {
        canonical_path: String,
        preview_token: String,
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

源码模式的可见 CodeMirror 不挂载 Atomic decoration、table widget 和图片 widget，仅保留 Markdown 语法、通用主题及文件编辑扩展；后台保留的预览实例不可见、不可聚焦，也不会建立第二份保存状态。

合法 GFM 表格始终启用 `Atomic tables`。安全降级精确到“表格自身包含 Markdown 图片”，不能因为文档其他位置存在图片而关闭全部表格。含图片表格暂时显示 Markdown 原文，避免 Atomic 内部把未经授权的 `src` 交给原始 `<img>`。

链接点击统一路由：

- HTTP/HTTPS/mailto 交给现有安全外链能力。
- 本地文件链接复用工作区文件链接解析与右侧 Tab 打开能力。
- 不直接调用 `window.open` 打开本地路径。

### 8.2 样式适配

- 内容颜色、muted、border、primary、code background 全部映射 Gold Band CSS variables。
- 实时预览正文使用 UI 字体和约 `65–75ch` 阅读宽度；代码块和行内代码使用 mono 字体。
- Markdown 标题接近正文，通过字重、轻量间距和细分隔表达层级，不使用文档页大标题样式。
- 深色主题避免多层黑色卡片和强边框。
- CodeMirror 虚拟滚动要求通过 line padding 表达垂直节奏，不对 `.cm-line` 使用会破坏高度测量的 margin。
- 正文固定使用稳定的 Gold Band 阅读字体栈、15px 基准字号和约 78ch 阅读宽度；标题、表格和内容 padding 使用紧凑层级，不继承可能用于代码的展示型自定义字体。

### 8.3 README 安全 HTML 子集

- 支持单行 `<div|p align="left|center|right">`、对应闭合标签、`<br>` 和单行 `<img ...>` 的视图解释。
- 白名单标签只生成 CodeMirror decoration、line class 和安全图片 widget，不执行 HTML，不使用 `dangerouslySetInnerHTML`。
- HTML `<img src>` 与 Markdown 图片共用 `resolve_markdown_image`、preview token 和外部精确授权；网络 badge 默认显示紧凑占位，不主动发起追踪请求。
- 实时预览中白名单 HTML 源码隐藏；需要修改标签时切换源码模式。未列入白名单的 HTML 保持源码显示。

## 9. Markdown 图片安全协议

### 9.1 支持来源

| 图片来源 | 默认行为 |
|---|---|
| 当前工作区内的相对或绝对本地图片 | 自动解析并安全展示 |
| 工作区外 Markdown 同目录及子目录的相对图片 | 自动解析并安全展示 |
| 外部 Markdown 的 `../`、其他盘符或目录外绝对图片 | 当前文档统一确认一次，只授权实际引用的精确文件 |
| UNC/网络共享路径 | 默认阻止自动加载 |
| `http://`、`https://` 网络图片 | 默认占位，确认后才加载，避免追踪请求 |
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
  -> 签发绑定 path + revision 的短期 preview token
  -> <img src="gold-band-preview://token">
```

必须保证：

- WebView 不接收 `file://`，图片 DOM 不保存本地绝对路径。
- 不信任扩展名和传入 MIME，以 Rust 文件签名探测为准。
- SVG 不进入 DOM，不执行脚本，不加载字体、图片或网络资源。
- token 到期、文件 revision 变化、引用删除或 Tab 关闭后失效。
- 同一文档相同 canonical path 复用 token；并发解析受配置限制。

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
| `workspace-file.markdown-image-network-blocked` | 网络图片尚未获确认 |
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
markdownExternalImagePolicy = "confirm"
```

范围：

- `markdownLivePreviewMaxChars`：超过后只启用源码模式。
- `markdownEmbeddedImageLimit`：单文档最多解析的图片引用数。
- `markdownEmbeddedImageMaxConcurrent`：单文档并发解析/解码请求数。
- `markdownExternalImagePolicy`：初始只允许 `confirm`，配置层预留未来受管环境策略；不能在组件中硬编码策略分支。

## 12. 性能策略

1. Atomic 和 Markdown parser 只在打开 Markdown 时动态加载。
2. 源码/预览共享同一源码和保存状态；预览实例跨模式保留，避免 Atomic table StateField 动态重配失效。
3. 图片只解析语法树识别出的 `Image` 节点，不用全量 DOM 扫描。
4. 文档修改仅重算受影响图片引用；普通文字输入不重复解析全部图片。
5. 相同 canonical path 去重，图片请求按配置限制并发。
6. 每个请求携带内容/request revision，忽略旧响应。
7. 超过 `markdownLivePreviewMaxChars` 时不挂载高成本 decoration/table/widget。
8. 不启用 Atomic 默认图片扩展，避免未授权网络或本地读取。

## 13. 测试矩阵

### 13.1 前端接口与 store 测试

- `.md` 默认实时预览，其他文本不受影响。
- 模式按 resource key 保存，关闭资源后释放。
- 切换模式不改变源码；预览实例不卸载，源码实例保留自己的原生 undo history。
- 复制的是当前内存源码，不是落盘旧内容。
- 输入、撤销、redo 继续进入原自动保存队列。
- 超阈值 Markdown 自动降级源码模式。
- 图片引用删除、变更、reload、LRU 和 Tab 关闭均释放 token。
- 过期图片解析响应不能覆盖新引用状态。

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
- README 白名单对齐标签不显示为正文，本地 HTML 图片走 preview token，远程 badge 不静默联网。
- 删除和新增标题文本后，最终 Markdown 源码语义正确；标记显隐按 Atomic 成熟交互执行。
- 两种模式内分别支持 `Ctrl/Cmd+Z` 与 redo；切换不销毁预览视图历史。
- 图片通过 `gold-band-preview://token` 展示，DOM 中不存在本地路径。
- 外部目录图片只出现一次文档级确认，不逐图片打断。
- 深色/亮色、宽屏/窄屏、长文档和多图片滚动正常。
- 目标行链接打开 Markdown 后仍定位到正确源码位置并滚动到视口中央。

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

### 阶段 4：真实 UI 验收

- 启动临时前端，deep link 到文件工作区。
- 验证编辑、撤销、自动保存、切换、复制、内外部图片、深浅主题与窄屏。
- 清理自己启动的服务和测试资源。
- 同步产品设计文档与上层开发计划完成状态。

## 15. 完成标准

- [x] Markdown 默认实时预览编辑，源码模式可随时切换。
- [x] 两种模式共享源码、revision 和自动保存链路；预览实例跨切换保留，两个可见编辑视图分别具备原生撤销历史。
- [x] 右上角复制源码与模式切换按钮完成无障碍和轻量反馈。
- [x] 标题、列表、强调、链接、代码块、任务项和合法 GFM 表格可直接编辑。
- [ ] Markdown 本地图片只通过安全 preview token 展示。
- [ ] 工作区外 Markdown 同目录图片自动展示，目录外引用按文档统一确认且只授权精确引用。
- [ ] SVG 安全栅格化；网络、UNC 和危险 scheme 不会静默加载。
- [ ] 大文档、多图片、过期异步响应和 token 生命周期符合性能与安全约束。
- [ ] 前端、Rust 接口和组件测试覆盖关键验收。
- [ ] 已完成真实 UI 的深浅主题、宽窄布局和编辑流程验证。
- [ ] 产品设计文档和上层开发计划同步更新。
