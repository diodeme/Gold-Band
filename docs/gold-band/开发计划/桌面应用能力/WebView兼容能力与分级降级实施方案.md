# WebView 兼容能力与分级降级实施方案

## 1. 文档状态

- 日期：2026-08-28
- 状态：自动化与 Windows 模拟验收完成，Monterey/WebKit 613 真机待验收
- 范围：Tauri 2 桌面端前端启动边界、macOS WKWebView 兼容、Markdown 渲染、主题与响应式降级
- 目标环境：包含 macOS Monterey 12.7.6、系统公开 WebKit `613.3.9.1.16` 在内的 Intel Mac

## 2. 问题与根因判断

### 2.1 用户现象

旧 Intel Mac 上启动 Gold Band 后出现白屏。生产 DevTools 显示前端入口在 React 挂载和应用自有错误诊断安装前，已经因当前 WKWebView 不支持 RegExp lookbehind 而解析失败。

已知入口链上的风险来源包括：

- `streamdown@2.5.0 -> remend@1.3.0`
- `mdast-util-gfm-autolink-literal@2.0.1`
- `web/src/components/prompt-kit/markdown.tsx`

同时，当前构建与样式系统存在以下兼容缺口：

- Vite 未显式声明生产 `build.target`，TypeScript 仅声明 `ES2022` 类型检查目标。
- `index.html -> main.tsx -> App.tsx` 静态导入全部页面、Markdown 和高亮依赖，不兼容代码可以在诊断链路启动前使整个应用失效。
- Tailwind CSS 4 的完整官方基线为 Safari 16.4，而 Monterey 最终可用的系统 WKWebView 能力可能仅相当于 Safari 15.6.1。
- 当前没有一个统一的 WebView 能力事实源，也没有在业务应用加载前进行能力门禁。

### 2.2 根因分类

该问题属于根本性的 WebView 兼容边界设计缺失，不是单个正则的局部实现错误。

原始设计使用 Tauri 系统 WebView、Tailwind/shadcn 和 prompt-kit/Streamdown 的方向仍然正确；缺陷在于没有将“对外宣称的系统支持范围”与“实际系统 WebView 能力”建立明确契约，并且将整个业务应用放在了无保护的静态启动链上。

因此实施时必须同时修复：

1. 启动边界。
2. 能力事实源和策略派生。
3. Markdown 通用语义中的不兼容正则。
4. 现代 CSS 和容器响应式的集中降级路径。
5. 可审计的 WebView 诊断日志。

### 2.3 首次真机结果与探测纠偏（2026-08-31）

Intel Mac DevTools DMG 已成功进入预检页，但现场能力快照同时报告 `:has()`、OKLCH、ResizeObserver、structuredClone 和 WebAssembly 可用，仅 `cssCustomProperties=false`。决策链确认是 `CSS.supports('--gold-band-capability-probe', '1')` 在该 WKWebView 上假阴性，继而把本应进入 compatible 档的环境错误判为 unsupported；冻结的 `AppleWebKit/605.1.15` user agent 不能作为真实系统 WebKit 版本依据。

能力分级和启动门禁的设计保持不变，缺陷属于正确设计下探测实现不完整。CSS 自定义属性改为语义探测：启动时挂载一个隐藏临时容器，让子节点通过继承变量解析颜色，再与直接设置同值的控制节点比较计算样式，最后在 `finally` 中移除。其他适合 `CSS.supports()` 的现代 CSS 能力继续走原探测，不增加平台、版本或用户机器特判。

最小测试先复现语义探测为真但 `CSS.supports()` 假阴性时仍被归为 unsupported，修复后同一测试转绿。相关 WebView 5 个测试文件/16 项、TypeScript 和生产构建通过；内置浏览器确认业务 App 进入 `/chat`、tier 为 full、临时探测节点无残留且控制台无 warning/error。修正版 Intel DevTools DMG 的同机验证仍待用户完成。

## 3. 开源组件与行业实践评估

### 3.1 继续复用的现有能力

- Tauri 2 继续使用操作系统提供的 WebView，不在 macOS 单独嵌入第二套浏览器内核。
- Markdown 继续复用 prompt-kit/Streamdown，不自研 Markdown parser、未闭合语法修复或第二套分块规则。
- 代码高亮复用 Shiki 官方 WASM Oniguruma 引擎，避免在旧 JavaScript RegExp 引擎上翻译 TextMate grammar。
- 布局继续复用 CSS container query；只在引擎不支持时，通过一个共享 ResizeObserver 适配器提供离散尺寸档位。
- Tailwind CSS 4 对常见透明度 utility 已输出不支持 `color-mix()` 时的基础颜色声明，方案仅需覆盖项目自定义主题材质和装饰效果。

### 3.2 不采用的方案

- 不按 `macOS && version < x` 在业务组件中散落特判。
- 不建设旧 macOS 专用业务页面或第二套 React App。
- 不通过关闭 GFM、链接、流式 Markdown 或代码块回避兼容问题。
- 不尝试让 Tauri WKWebView 改用 Safari Staged WebKit；WKWebView 只能使用操作系统公开 framework。
- 不引入持久化能力结果、轮询、事件总线或平台版本矩阵。

## 4. 支持策略

### 4.1 两个阈值、三种状态

| 状态 | 含义 | 用户体验 |
| --- | --- | --- |
| `unsupported` | 缺失应用核心运行所必需的 Web 能力 | 不加载业务 App，显示可恢复的升级指引和诊断信息 |
| `compatible` | 核心业务可用，但缺失部分现代 CSS/引擎能力 | 保留全部核心功能和内容语义，降级材质、动效和部分布局实现 |
| `full` | 满足完整现代能力集 | 使用当前全部现代 CSS、材质和响应式实现 |

### 4.2 版本参考

版本只用于诊断和升级指引，运行策略必须由能力探测决定。

- 建议最低参考：WebKit `613.1.17`，约等于 Safari 15.4 能力，macOS 12.3。
- 已知兼容目标：WebKit `613.3.9.1.16`，约等于 Safari 15.6.1 能力，macOS Monterey 12.7.6。
- 建议完整能力参考：WebKit `615.1.26`，约等于 Safari 16.4 能力，macOS 13.3 及以上。

### 4.3 体验一致性契约

`compatible` 与 `full` 必须保持：

- 相同的核心业务流程、数据结果和内容语义。
- 相同的 Markdown/GFM 语义、本地文件链接、流式内容和代码块基础交互。
- 相同的对话、任务、工作流、文件和源码管理能力。

允许存在的差异：

- 背景模糊、透明材质和精细颜色混合改为实色。
- 依赖 `@property` 的装饰动画关闭。
- 缺失 container query 时由离散容器尺寸档位代替，不要求像素级一致。
- WASM 高亮引擎初始化失败时，代码块保留原文、复制和行号，仅丢失语法着色。

## 5. 数据结构与事实源

### 5.1 原始能力

```ts
interface WebviewCapabilities {
  regexpLookbehind: boolean;
  cssColorMix: boolean;
  cssContainerQueries: boolean;
  cssHasSelector: boolean;
  cssBackdropFilter: boolean;
  resizeObserver: boolean;
  structuredClone: boolean;
  webAssembly: boolean;
}
```

能力对象在每次页面启动时同步计算一次，之后不可变。它属于当前 WebView 进程的瞬时运行事实，不持久化、不轮询，不反向成为业务 canonical state。

### 5.2 派生策略

```ts
interface WebviewFeaturePolicy {
  tier: 'unsupported' | 'compatible' | 'full';
  themeRendering: 'fallback-tokens' | 'modern-css';
  responsiveLayout: 'measured' | 'container-query';
  codeHighlighting: 'plain' | 'wasm';
  visualMaterial: 'solid' | 'native';
}
```

策略必须由纯函数从 `WebviewCapabilities` 派生。平台名称、macOS 版本和 WebKit 版本不允许参与策略分支。

### 5.3 运行诊断事实

```ts
interface WebviewRuntimeFacts {
  platform: string;
  architecture: string;
  osVersion: string | null;
  webkitBundleVersion: string | null;
  userAgent: string;
  capabilities: WebviewCapabilities;
  policy: WebviewFeaturePolicy;
}
```

运行诊断事实只用于：

- `runtime.log` 中的一次性环境记录。
- `unsupported` 页面的升级指引和诊断复制。
- DevTools 诊断包的现场支持。

它不得决定业务功能、不得写入用户偏好、不得覆盖 Runtime 状态。

## 6. 整体架构

```text
Tauri/Rust 运行诊断事实
              │
              ▼
启动时一次 JS/CSS 能力探测
              │
              ▼
    immutable WebviewCapabilities
              │
              ▼
      pure WebviewFeaturePolicy
         │              │
         │              └─ unsupported 轻量页
         ▼
   动态加载共用 React App
         │
         ├─ Theme Runtime 适配
         ├─ Markdown/WASM 高亮适配
         └─ Responsive Layout 适配
```

全部业务页面继续使用同一份 React 实现。能力差异只在启动、主题、Markdown 和响应式边界内消化。

## 7. 分阶段实施计划

### 阶段 A：建立最小失败证据

1. 新增 WebKit 613/Monterey 模拟能力 fixture。
2. 新增能力检测与策略分类测试，固定 `unsupported/compatible/full` 边界。
3. 新增启动入口契约测试，证明当前入口会在预检前静态导入 App/Markdown。
4. 新增 Markdown 兼容测试，覆盖：
   - 本地文件链接。
   - 图片不得被当成文件链接。
   - Web 链接和邮箱自动链接。
   - GFM 标点边界。
   - 流式未闭合 Markdown。
5. 保留已有用户日志、DevTools 错误和静态依赖位置作为无法在 Windows 真实运行 WKWebView 613 的替代现场证据。

验收：修复前的失败测试必须稳定失败，且失败原因与入口解析和 lookbehind 根因一致。

### 阶段 B：重构启动边界

1. 将 `web/index.html` 的直接 `main.tsx` 入口替换为极小的兼容预检入口。
2. 预检入口只静态导入：
   - 能力探测器。
   - 纯策略解析器。
   - 不依赖 React/Tailwind 的启动状态样式。
3. `compatible/full` 策略通过后，再使用动态 `import()` 加载现有 React App。
4. App chunk 解析或加载失败时，由预检入口捕获并显示结构化启动错误，不再进入无诊断白屏。
5. `unsupported` 不导入业务 App，直接显示升级 macOS 指引、能力缺口和诊断复制按钮。
6. 启动页的原生 DOM 是启动安全边界的必要例外，不得引用 shadcn/prompt-kit；正常 App 仍遵守前端组件策略。

验收：在测试中注入 `unsupported` 能力时，App 加载器不得被调用；注入 `compatible/full` 时只调用一次。

### 阶段 C：集中能力与诊断

1. 实现 `detectWebviewCapabilities(environment)`，所有探测使用安全的运行 API，能力不存在时返回 `false` 而不得抛出。
2. 实现 `resolveWebviewFeaturePolicy(capabilities)` 纯函数。
3. 在 `<html>` 上设置统一数据属性，供 CSS 和调试使用；业务组件不得读取平台版本。
4. macOS 后端使用系统 framework 信息获取公开 WebKit bundle 版本，不执行 `sw_vers`/shell 外部命令。
5. 诊断采集异步且 best-effort，不阻塞 React App 加载；成功后向 `runtime.log` 写入一条结构化能力摘要。
6. 日志不采集聊天内容、prompt、附件路径或页面数据。

验收：同一页面生命周期只计算和上报一次；诊断失败不改变 tier，不会阻止 App 启动。

### 阶段 D：根治 Markdown lookbehind

1. 升级 Streamdown/remend 到已修复对应 lookbehind 的上游版本，并锁定通过回归的精确依赖树。
2. 将本地 `proxyLocalFileLinks` 改为不使用 lookbehind 的等价语义实现。
3. 优先检查 `mdast-util-gfm-autolink-literal` 最新上游版本；上游仍未修复时，对该单一依赖建立最小、可审计、带来源链接的本地补丁。
4. 补丁只替换边界检测实现，不改变 GFM AST 结果；上游发布正式修复后必须可以直接删除。
5. 生产 Markdown 依赖链增加兼容审计测试，防止依赖升级重新引入会在 WebKit 613 解析阶段失败的 RegExp。

验收：同一组 Markdown 契约测试在修复后转绿，链接、图片、邮箱、GFM 和流式修复语义不变。

### 阶段 E：统一 WASM 代码高亮

1. 移除当前 `@streamdown/code` 对 Shiki JavaScript RegExp 引擎的强制选择。
2. 使用 Shiki 官方 `createOnigurumaEngine(import('shiki/wasm'))` 创建 Streamdown code plugin 适配器。
3. 高亮引擎和语言 grammar 只在首次出现代码块时加载。
4. 全局只保留一个初始化 Promise/实例，多条消息不得重复加载 WASM。
5. 高亮失败只记录一次诊断，同一代码块回退为纯文本，不阻止 Markdown 正文显示。

验收：测试固定惰性加载、单例初始化、多代码块复用、失败降级和复制/行号保留。

### 阶段 F：主题与 CSS 降级

1. 新增一份由 `data-webview-tier` 限定范围的兼容样式。
2. 主题生成器对自定义 `color-mix()` 声明输出基础 token 回退和 `@supports` 现代覆盖，不在各个组件手写颜色特判。
3. `compatible` 模式将以下材质集中收敛为实色：
   - composer。
   - dialog/sheet/popover。
   - sidebar/panel。
   - wallpaper overlay。
4. 关闭依赖 `@property`、模糊或复杂 filter 的装饰动画，保留 focus、hover、loading 和状态反馈。
5. 审计项目手写 `color-mix()`，将业务组件中的直接声明收敛为语义 token 或中央样式。

验收：模拟不支持 `color-mix/backdrop-filter/@property` 时，浅色和深色主题的文字、边界、按钮、输入框和弹层仍具有可用对比度。

### 阶段 G：容器响应式降级

1. 实现一个共享的 compatibility container adapter，只在 `responsiveLayout='measured'` 时启用。
2. 使用现有 `ResizeObserver`，每个可见容器最多每动画帧计算一次。
3. 只发布 `compact/medium/wide` 等离散档位，同一档位内的连续像素变化不提交 React state。
4. 对现有命名 container 进行清单化适配，优先保证：
   - 会话 composer 与信息栏。
   - 右侧工作区。
   - 设置页和主题抽屉。
   - 角色/上下文管理页。
   - 工作流编辑器。
   - Raw frame 和代码/文件阅读区。
5. 业务组件只声明容器身份和布局档位，不读取 macOS/WebKit 版本。

验收：正常宽度、窄窗口和重新拉宽时能够在预期档位间单调切换；不引起 Markdown、历史消息或应用根组件重渲染。

### 阶段 H：构建目标与 bundle 契约

1. Vite 明确设置 Safari 15.4 JavaScript 构建目标。
2. 保持预检 chunk 独立且最小，不得包含：
   - React/ReactDOM。
   - App 和业务页面。
   - Streamdown/Markdown。
   - Shiki/WASM。
   - browser mock API。
3. 对生产预检 chunk 和 App chunk 进行语法兼容审计。
4. 若 browser mock API 确实把不兼容代码带入桌面生产 chunk，才在构建边界分离 desktop/browser API；不借此重构整个 API 层。

验收：生产构建后记录预检、主应用和 WASM 资源大小；预检不得因为业务依赖变大。

### 阶段 I：文档与发布边界

1. 在产品设计文档新增 WebView 兼容能力设计，记录事实源、分级契约、错误码和用户体验边界。
2. 更新 App overview 和 MVP 开发计划中的 macOS/WebView 支持范围。
3. 在 Intel DevTools DMG 验收说明中增加能力诊断采集步骤。
4. 不把兼容诊断包加入正式 updater 产物链路。

## 8. 计划修改的代码边界

实际文件名允许在实施时按既有目录语义微调，但职责边界不得扩散。

| 领域 | 计划修改 |
| --- | --- |
| 启动 | `web/index.html`、新的 compatibility bootstrap 入口、现有 `web/src/main.tsx` |
| 能力 | 新的 `web/src/lib/webview-capabilities.ts`、`webview-feature-policy.ts` |
| 诊断 | `web/src/api/*`、`src-tauri/src/commands.rs`、`src-tauri/src/main.rs`、必要的 macOS 诊断 adapter |
| Markdown | `web/src/components/prompt-kit/markdown.tsx`、Streamdown/remend/mdast 依赖边界、新的 WASM code plugin adapter |
| 主题 | `web/src/theme.ts`、`web/src/styles.css`、主题生成器和生成 CSS |
| 响应式 | 新的共享 compatibility container adapter，以及现有命名 container 的最小接入 |
| 构建 | `web/vite.config.ts`、必要的 bundle 契约测试 |
| 测试 | `web/tests` 下的 capability、bootstrap、Markdown、高亮、主题和响应式回归 |
| 文档 | 产品设计文档、本实施计划、App overview 和 MVP 计划 |

## 9. 错误契约

启动错误使用结构化类型，不使用一个临时 string 决定 UI。建议稳定错误码：

| code | 含义 |
| --- | --- |
| `webview.capability.unsupported` | 缺失核心 WebView 能力，业务 App 不应加载 |
| `webview.app_chunk.load_failed` | 能力预检通过，但业务 chunk 解析或加载失败 |
| `webview.runtime_facts.unavailable` | 版本诊断获取失败，不影响能力策略 |
| `webview.code_highlighter.unavailable` | WASM 高亮初始化失败，代码块回退纯文本 |

后端诊断接口只返回稳定 code 和结构化参数。正常 React App 内的对客文案由现有 i18n 映射；在 React/i18n 之前展示的启动安全页仅保留极小中英文映射。

## 10. 测试与验收矩阵

### 10.1 自动化测试

- 能力探测器：每个 API 存在、缺失和探测抛错场景。
- 策略解析器：最低、Monterey-compatible 和 full 能力 fixture。
- 启动入口：不支持时不导入 App，支持时只导入一次，加载失败可见。
- Markdown：链接、图片、邮箱、GFM、流式修复、代码块。
- WASM 高亮：惰性、单例、复用、失败回退。
- 主题：兼容 token、实色材质、浅色/深色、显式 component variant 覆盖。
- 响应式：档位边界、同档不重渲染、卸载断开 observer。
- Rust 接口：macOS 版本解析、非 macOS 结果、日志限长和敏感字段边界。

### 10.2 构建验收

- TypeScript 检查。
- Web 定向测试。
- Web 完整回归。
- 生产 Web 构建。
- Rust 定向单元/接口测试。
- Rust 格式检查。
- 预检 chunk 静态依赖和大小审计。
- 生产 bundle 的不兼容 RegExp 审计。

### 10.3 UI 验收

使用开发期能力 fixture 分别 deep link 到：

1. `unsupported` 启动页。
2. Monterey-compatible 正常 App。
3. full 正常 App。

每种支持状态至少检查：

- 浅色和深色。
- 正常宽度、窄窗口和重新拉宽。
- 对话、设置、管理页、右侧工作区和工作流编辑器。
- Markdown 标题、列表、表格、链接、图片和代码块。
- DevTools console 无新的 warning/error。

验证结束后关闭页面并清理本次启动的测试进程。

### 10.4 真机验收

Windows 可以完成能力注入、接口回归、生产 bundle 审计和 UI 模拟，但不能代替 WKWebView 613 真机结论。

最终需在已知用户环境或等价设备上使用 Intel DevTools DMG 完成：

1. 首次启动不白屏。
2. `runtime.log` 中存在一次性 WebView 诊断记录。
3. 对话、任务、设置和右侧工作区主路径可用。
4. 流式 Markdown 正常收敛，本地链接和 GFM 语义正确。
5. WASM 代码高亮可用；若失败，纯文本回退不影响正文。
6. 窗口缩放与主要弹层、工作区布局不溢出。

## 11. 性能影响评审

### 11.1 启动

- JS/CSS 能力探测为固定数量的同步调用，时间和内存复杂度均为 O(1)。
- CSS 自定义属性探测固定创建一个宿主与两个子节点、读取两次计算样式并立即移除；只发生一次常量级 style calculation，不保留 DOM、observer、缓存或 React state。
- macOS/WebKit 版本诊断异步进行，不放入 React App 加载的前置阻塞链。
- 预检边界可以延后业务 chunk 请求，但仅增加一次当地资源动态导入；验收时应记录预检和 App 首屏时序。

### 11.2 Markdown 与 WASM

- 最终生产 `web/dist/assets` 为 261 个文件、合计 10.20 MiB 未压缩；其中 Shiki WASM 引擎对应 JS 资源约 622 KiB，按需语言/主题等非首屏资源约 6.33 MiB。原方案同样使用 Shiki，不能仅凭本次目录总量推断安装包增量，发布时应通过前后 CI artifact 对比确认。
- WASM 不进入首屏关键路径，只在首次出现代码块时加载。
- 高亮引擎使用单例，不按消息、代码块或流式快照重复初始化。
- 不改变现有 Streamdown 增量 block parser 和文档级单 RAF 播放水位契约。

### 11.3 响应式

- full 模式仍使用原生 CSS container query，不新增 observer 或 React state。
- compatible 模式只对当前可见的已登记容器建立 observer。
- Resize 事件每帧最多处理一次，只有跨越离散档位时才更新 React state。
- 不将尺寸能力放入应用根 Context，避免连续缩放导致 Markdown、历史消息和工作区树重渲染。

### 11.4 I/O、内存与并发

- 每次页面启动最多一次系统 WebKit 版本读取和一条诊断日志。
- 日志继续使用现有有界异步 `runtime.log` writer，不新建日志队列。
- 不引入无界缓存、轮询、重试队列、长时间锁或应用主线程同步 I/O。

## 12. 过度设计评审

方案的最小新增抽象为：

1. 一个能力探测器。
2. 一个纯策略解析器。
3. 一个启动预检入口。
4. 一份有作用域的兼容样式/token 路径。
5. 一个仅在缺少 container query 时生效的共享尺寸适配器。
6. 一个复用 Shiki 官方 WASM 引擎的 Streamdown code adapter。

不新增：

- 第二套 React App 或业务组件树。
- 按 macOS/WebKit 版本维护的功能矩阵。
- 持久化能力字段、新领域 aggregate 或状态机。
- 事件总线、轮询、后台 worker 或双写路径。
- 自研 Markdown parser、代码 grammar 引擎或浏览器内核。

现有能力尚不足以表达“App 能否安全加载”这一不变量，因此新增能力对象和策略解析器是必要的；其余业务 canonical state 无需复制或改造。

## 13. 完成定义

只有同时满足以下条件时，本计划才能标记为完成：

- 修复前的最小失败测试已记录，修复后使用同一测试转绿。
- 旧 WebView 不再在 App 加载前因 lookbehind 白屏。
- `unsupported/compatible/full` 三档策略由能力而不是平台版本派生。
- compatible 与 full 共用同一业务 App 和 Markdown 语义。
- Markdown 生产链不包含已知会在 WebKit 613 解析阶段失败的 RegExp。
- WASM 高亮按需加载，失败时代码正文仍可用。
- 兼容主题和响应式适配通过浅色、深色和窄窗口验收。
- `runtime.log` 中可以看到一次性结构化 WebView 诊断且不含业务敏感内容。
- TypeScript、相关 Web 回归、Rust 接口测试、生产构建和 UI 验证通过。
- 产品设计文档、App overview 和 MVP 开发计划已同步。
- Intel macOS Monterey/WebKit 613 真机冒烟结果已记录；若尚未获得真机结果，必须明确标记“自动化完成、真机待验收”，不得宣称已完全验证。
- 首次真机暴露的 CSS 自定义属性假阴性已由最小失败测试固定，修正版需在同一设备确认不再被错误拦截。
