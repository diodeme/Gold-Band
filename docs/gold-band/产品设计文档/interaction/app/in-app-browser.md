# 右侧工作区内置浏览器

## 1. 一句话定义

内置浏览器是会话右侧工作区里的一份应用级浏览能力：用一块零权限子 WebView 打开 `http(s)` 与本地 HTML，内部自己管理页签。它不是系统浏览器替代品，也不是每个 URL 一个工作区 Tab。

## 2. 范围

第一版包含：

- 右侧工作区一个 `kind: 'browser'` 资源，内部多页。
- 前进、后退、刷新、地址栏。
- 当前页可在电脑版与移动版之间切换；默认电脑版。
- 加载中刷新按钮切换为“停止”；无完成事件时 15 秒解除遮罩，避免网页永久遮挡交互。
- 地址栏区分网址与搜索词；搜索引擎可选百度、Google、Bing。
- 会话、Markdown、文件预览中的 `http(s)` 与本地 `.html/.htm` 点击后在此打开。
- 页内按用户手势分流：普通左键 `http(s)` 链接始终当前页跳转；Ctrl+点击、右键「新窗口」、以及仍到达引擎的 `window.open` 拦截为内部新页，不弹系统窗口。
- 进程内一份浏览会话与一份 Cookie profile；登录态跨应用重启保留。
- 空白页与无内部页统一为门户页；工具栏书签图标加入/移除当前站点。
- 下载走系统「另存为」，无应用内下载面板。
- 重建 WebView 时用 `BrandLoadingState` 呼吸 logo。
- Windows、macOS、Linux 同一套实现。Linux 个别 Wayland 错位不特判。

第一版不做：

- 从本机 Chrome / Edge / Firefox 导入 Cookie。
- 下载队列、进度、暂停、下载历史。
- 独立弹窗、扩展、多 profile、书签栏、开发者工具作为默认产品入口。
- 应用重启后恢复打开的 URL 列表。
- 历史管理页、从系统浏览器导入历史/图标、第三方图标 CDN。
- 以内存字节数（如 1GB）作为主淘汰阈值。

`mailto:` / `tel:` 仍走系统 opener，不进内置浏览器。

## 3. 根因与设计方向

原始子 WebView 分层设计成立。本轮现场故障属于正确设计但实现不完整：Windows 上同步 IPC 命令在主 WebView2 的 `WebResourceRequested` 回调内执行 `Window::add_child`，wry 用嵌套消息循环等待新 controller 完成，COM 不可重入导致命令永不返回。前端因此一直停在 `live=false && loading=true`；半成品 HWND 已挂在原矩形上，切到设置页后仍截获点击。另外，占位组件曾把整个 `page` 对象当作 effect 依赖，title / load 事件会反复 `hideAll`。修复方向是让创建/关闭/显隐命令保持 async、以 `pageId` 作为原生图层生命周期身份，并在打开远程页和 create 失败时收敛 loading，而不是增加第二套页状态或针对百度打补丁。

不能把网页画在主应用 WebView 的 iframe 里：多数站点会拒绝嵌入，且会和 Gold Band IPC 同进程。子 WebView 是原生图层，不是 CSS div，必须按占位盒同步坐标，并在右栏折叠、Sheet、Dialog、切 Tab 时显隐。

工作区每个 URL 一 Tab 会造成 Tab 爆炸（页内也会开新页）。浏览器是一个工作对象，页是它内部状态。浏览会话全应用一份，因为 Cookie profile 也是一份；按工作空间拆开会变成多套页签共用登录态。

## 4. 数据归属

先定数据，再定接口。

| 实体 | 领域 | 权威来源 | 生命周期 | 持久化 |
|---|---|---|---|---|
| `BrowserSession` | 应用级浏览 | 进程内单例 | 首次打开浏览器时创建，进程退出时销毁 | 不落盘 URL |
| `BrowserPage` | 浏览会话内部页 | `pageId`（稳定 UUID）+ url + title + `activePageId` + `viewMode` | 随会话增删 | 不落盘 |
| `BrowserVisit` | 应用级访问记录 | 规范化 `http(s)` URL | 地址栏补全；最多 200 条，按最近访问淘汰 | `{appData}/browser-profile/history.json` |
| Origin favicon | 浏览图标缓存 | `scheme+host+port` | 32px PNG，失败用 Globe | `{appData}/browser-profile/favicons/` |
| `BrowserBookmark` | 门户书签 | 稳定 `bookmarkId` + origin | 用户有序列表，上限 32 | `{appData}/browser-profile/bookmarks.json` |
| 活 WebView 实例 | 浏览宿主 | `pageId` → native webview label | 最多 5 个 LRU；当前页不可淘汰 | 不进入 React state |
| Cookie / localStorage | WebView profile | 应用数据目录 `browser-profile/` | 跨重启保留 | 由 WebView 引擎管理 |
| 工作区 `browser` 资源 | 当前会话 scope 的投影 | `kind: 'browser'` + 当前 `scopeKey` | 只表示「这个会话 Tab 条上有没有浏览器入口」 | 与其他工作区 Tab 一样仅进程内 |
| `BrowserPreferences` | 用户偏好 | Settings v1 嵌套结构 | 应用配置生命周期 | 搜索引擎、localhost 链接和普通网页链接的打开方式 |

不变量：

- 全应用只有一份 `BrowserSession` 和一份活 WebView LRU。不同工作空间打开「浏览器」看到同一组页、同一登录态。
- 网页列表不复制进会话资源 LRU（现有 24 个 scope）。会话 LRU 只保存「本 scope 是否挂了浏览器投影 Tab」。
- 资源描述符仍带当前 `scopeKey`，不把 A 会话的文件/Agent Tab 写入 B 会话。浏览器是明确的应用级例外：投影按 scope，内容按进程单例。
- Native WebView 句柄不进 React state、不进 Tab 描述符、不进 conversation preference。
- 访问记录与打开的内部页分开。关掉页签或工作区浏览器 Tab 不删除访问记录；重启也不恢复打开的 URL 列表。
- 门户书签与访问记录分开。图标按 origin 缓存在 favicons 目录；书签 JSON 不存 data URL。渠道内置目录只在该渠道首次生成书签文件时拷贝一次。
- `about:blank` / 无内部页都显示同一门户页，不创建白色子 WebView。

`BrowserPage` 只存摘要：`pageId`、url、title、是否当前页、电脑/移动浏览意图。不存 DOM、滚动像素、截图。

## 5. 工作区投影与内部页签

### 5.1 工作区 Tab

- 稳定 key：当前 scope 内唯一的 `browser`。
- 「新建 Tab」菜单与空态入口提供「浏览器」。
- 点 `https://` 或本地 HTML：若本 scope 还没有该投影，打开并聚焦；若已有，只聚焦并在全局会话里打开或复用内部页。
- × 掉工作区「浏览器」：去掉本 scope 的投影 Tab，**不**清空全局内部页；丢掉活 WebView。
- 再从任意会话打开浏览器：回到同一组内部页，停在上次当前页。
- 内部「关闭所有标签」：清空全局内部页；工作区浏览器 Tab **仍在**，内容区只留 `+`。

### 5.2 内部页签栏

位于浏览器面板工具栏下方，不是第二套工作区 Tab 外观。紧凑：标题截断，激活态用主题 `accent`。

- 右侧固定 `+`，不随页签滚动。点 `+` 新建空白内部页，内容是门户页。
- 页签可收到最小宽度；再多则横向滚动，使用 `gold-themed-scrollbar`。
- `scrollWidth` 实际超过可视宽度时，再显示完整页列表菜单；未溢出不占空间。
- **不叠放**页签。
- 页签左侧展示该页 origin 图标；没有缓存时用 Globe。溢出菜单用同一套图标。
- 关闭按钮对齐工作区 Tab：激活项 × 常显但弱化，未激活 hover 出现。
- 右键使用 shadcn/ui ContextMenu：关闭、关闭其他标签、关闭左侧所有标签、关闭右侧所有标签、关闭所有标签。关闭所有标签后仍显示 `+`，内容区回到门户页，不关闭工作区浏览器 Tab。

工具栏：后退、前进、刷新/停止、地址栏。页内前进后退用 WebView 原生会话历史。电脑/移动切换左侧是书签图标：当前 `http(s)` 页可点，已加入用 `accent-foreground` 实心并带 `accent` 浅底，再点删除。门户页上该书签按钮不可用。地址栏聚焦且尚未改字时，在输入框下方浮层展开最近访问（最多 8 条，标题 + 精简 URL + origin 图标）；开始输入后过滤同一列表，普通词额外提供「搜索网页」。悬停或键盘选中访问项时显示 ×，删除该条访问记录，不关浮层、不跳转。浮层只在地址栏编辑中打开：回车提交当前输入（需方向键才选中建议项），提交后或页 URL 同步后关闭，不再自动弹出。浮层不挤占工具栏和页签；落到网页上的部分只把子 WebView 顶边下移，页面仍显示在列表下方。内部页签和溢出菜单复用同一 origin 图标，没有图标时用 Globe。地址栏提交时，显式 scheme、域名、IP、localhost 与本地 HTML 作为地址；其他裸词和含空格文本按当前搜索引擎生成查询 URL。空页首次提交必须创建原生页，已有活页才 navigate。提供带 shadcn Tooltip 的「用系统浏览器打开」次要动作；对尚未提交的搜索词也使用当前搜索引擎解析。

电脑版 / 移动版是当前内部页的浏览意图，不是设置项，也不落盘。默认电脑版，工具栏在「用系统浏览器打开」左侧显示手机图标，表示下一步切到移动版；移动版时显示电脑图标。点击后在**同一活 WebView** 上更换 User-Agent 并 reload 当前文档：电脑版使用引擎默认 UA（切回时恢复创建时记下的引擎 UA），移动版使用固定 Android Chrome UA。reload 不加历史条目，前进后退继续使用该实例的原生会话历史。栏宽仍跟右侧工作区走，不模拟手机外框。纯 CSS 按宽度响应的站点可能看起来变化不大；按 UA 分流的站点（如 Google、百度）会切版。新标签默认电脑版，互不影响。

### 5.3 门户页

没有内部页、以及当前内部页是 `about:blank` 时，内容区都是门户页。固定首卡「打开空白页」，与工具栏 `+` 同一命令，不可拖、不可删。其余是书签卡：站点图标、截断名称、截断 URL，Tooltip 出全文；按加入顺序排列，可拖拽排序；悬停 × 删除。点书签：若当前是空白内部页则在该页打开，否则 `openUrl`。

书签身份是 origin。渠道内置列表在 `configs/browser-bookmarks.json` 按 `default` / `wb` 维护；当前渠道在首次打开浏览器时写入用户列表。之后只认用户列表。`wb` 内置内网站点（码灵官网、CI 平台、DPMS、AOMP、WeAPM、Pace+、CMDB），图标在内网打开门户时再拉取，不打进安装包。图标复用 origin favicon 缓存：访问过的站直接用；未访问的内置站只拉取图标，不写访问记录。

## 6. 链接打开

统一入口：`openWebTarget(href, source)`。禁止各处直接 `openExternalUrl` 打开 `http(s)` 或本地 HTML。

| 输入 | 行为 |
|---|---|
| `http://` / `https://` | 按“设置 → 通用 → 浏览器”路由；localhost 默认内部打开，普通网页默认系统浏览器，可分别切换 |
| 地址栏裸词 / 含空格文本 | 使用当前搜索引擎搜索；支持百度、Google、Bing |
| 本地 `.html` / `.htm`（相对路径、绝对路径、`file://`） | 经现有文件链接 resolver 得到 canonical locator 后，用 preview grant 在内部页加载；不进 CodeMirror |
| 其他本地文件 | 现有文件工作区 |
| `mailto:` / `tel:` | 系统 opener |
| 页内普通左键 `http(s)` 链接 | 当前内部页跳转；站点 `preventDefault` / `target=_blank` 不得把点击吞掉 |
| 页内 Ctrl+点击、右键「新窗口」 | 内部新页，不创建系统窗口 |

Markdown 会话链接、文件 Markdown 预览、源码管理 Markdown 外链共用该入口。

页内点击按手势分流，不按 HTML `target` 分流。普通左键在子 WebView 文档捕获阶段把 `http(s)` 链接改成当前页 `location.assign`，这样站点 `preventDefault` / `target=_blank` 不能把点击吞掉。Ctrl+点击、右键「新窗口」、以及仍然到达引擎的 `window.open` 才走 `on_new_window`，拦截为内部新页。Windows 上 External 子 WebView 对 `_blank` 锚点经常根本不触发该回调；Tauri `NewWindowResponse::Deny` 等于 Handled，也不会回退成当前页导航。因此左键契约必须在点击发生时落实，不能指望 Deny 之后再补一次 navigate。

### 6.1 浏览器设置

统一位于“设置 → 通用 → 浏览器”，复用现有设置持久化与 shadcn `Select` / `Switch`：

- 搜索引擎：百度（默认）、Google、Bing。
- 在 Gold Band 中打开 localhost 链接：默认开启。
- 在 Gold Band 中打开普通 HTTP/HTTPS 链接：默认关闭。

这些开关只决定应用内容中的链接路由；用户已经进入内置浏览器后，在地址栏输入的目标始终在当前内置页打开。

## 7. 宿主与布局同步

主界面仍是一块 WebView。浏览内容是同窗口上的子 WebView，盖在 React 占位 div 上。

范式：

```text
BrowserPanel
├─ React 工具栏 / 内部页签栏
└─ NativeBrowserViewport
   └─ 空 div（占位）
        → ResizeObserver + 布局 commit
        → rAF 合并，每帧最多一次
        → setPosition / setSize
        → Native WebView
```

约束：

- 使用 Tauri 官方 `Webview` 的 `setPosition` / `setSize` / `show` / `hide` / `close`。创建子 WebView 所需 capability 与 `unstable` 特征在开发方案中记录。Windows 上所有会创建、关闭或操作子 WebView 的命令必须是 async，禁止从同步 IPC 命令调用 `add_child`。
- 同步必须同时覆盖 **尺寸变化和位移**。仅 `ResizeObserver` 不够：左栏收起而右栏宽度不变时，`x` 会变、`width` 不变。必须接到现有 Shell 布局帧（左栏折叠、右栏 collapse/expand、窗口 `scaleFactor`、Sheet 开关、Tab 激活）。
- 坐标使用逻辑像素，与 `getBoundingClientRect()` 对齐，并处理 DPI。
- 同步热路径禁止每次回调两次无合并 IPC；禁止把逐像素写入 React 根 state。rAF 合并必须读取最新 bounds，不得把同一帧内的后续尺寸丢掉。
- 原生 create 未完成时占位盒仍可能继续布局；create 成功后必须把当时最新的占位矩形应用到 WebView，不能沿用发起 create 时的第一帧尺寸。实例变为 live 后要再同步一次，且不得因此 hide。
- 占位组件的原生显隐生命周期只绑定 `pageId` 与可见性，不得因 title、loading 或 url 投影更新而卸载观察器或 `hideAll`。
- 改 URL 使用 navigate，不重建 WebView。只有新内部页才 create，淘汰或关闭页才 close。
- 门户 URL 仍测量占位盒并记住 bounds，但不 create / show；从门户提交地址或打开书签时用该尺寸创建原生页。
- 标签必须唯一且符合 Tauri webview label 规则。
- 地址栏建议是工具栏内的绝对定位浮层，不改变工具栏和页签高度。子 WebView HWND 在 React 之上，浮层不能画在网页上；底边落到网页占位内时，只把 native host 的 `top` 设为重叠高度，页面仍显示在列表下方。不得为此 `hideAll`。

## 8. 生命周期与显隐

未打开过浏览器：不创建子 WebView、不创建 WebView 环境、不挂观察器、不跑同步 rAF、不预热 profile。面板模块懒加载。

| 动作 | 原生图层 | 活 WebView（≤5） | 内部页摘要 | 工作区投影 Tab |
|---|---|---|---|---|
| 从未打开 | 无 | 无 | 无 | 无 |
| 右栏可见且当前是浏览器 | 对齐占位盒 | 当前页必活，其余 LRU | 保留 | 在 |
| 切到文件 / Agent | 立刻 hide，停同步 | 可留在 LRU | 保留 | 在 |
| 切会话 scope（详情 ↔ 快速会话）且两边浏览器仍是激活投影 | 不 hide、不 suppress；面板可能被 React 复用 | 留着 | 保留 | 随新 scope 的投影 |
| 会话内设置（Shell 仍在，右栏 scope 为空） | 立刻 hide，随后 discard | **全部丢弃** | 保留 | 随 scope 保存 |
| 手动关右栏 | 立刻 hide，停同步 | **全部丢弃** | 保留 | 在（只是看不见）
| 窗口变窄自动收起，Sheet 未开 | 立刻 hide，停同步 | **先留着** | 保留 | 在 |
| 窄屏 Sheet 打开 | 对齐抽屉矩形 | 留着 | 保留 | 在 Sheet 中 |
| 任意 Dialog / 权限弹窗 / 菜单打开 | hide | 留着 | 保留 | 不变 |
| overlay 关闭且浏览器仍是可见激活资源 | 按占位盒 show | 当前页若已丢弃则重建 | 保留 | 不变 |
| × 工作区「浏览器」 | hide | 全部丢弃 | **保留** | 去掉 |
| 内部关闭所有标签 | hide | 全部关闭 | 清空 | 仍在，空态 + |
| 离开会话 Shell（例如进入独立工作台设置页） | 立即 close/discard，停同步 | **全部丢弃** | 保留 | 随 scope 保存 |
| 应用退出 | 全部 close | 无 | 丢弃 | 丢弃；Cookie 保留 |

丢弃后的再打开：只重建当前页；其余内部页只显示标题，点到再创建 WebView。页内 JS 状态和滚动位置不恢复；Cookie 使登录通常仍有效。

`suppress` 只覆盖面板真正卸载的窗口，防止半成品 HWND 挡点击。`resume` 必须通知占位组件按当前占位盒重新 `show`；只清标志位而不同步，会留下工具栏还在、网页全白的状态。scope-change / deactivate 的 close resolver 不得对全局浏览会话 `suppress` 或 hide：显隐由浏览器面板挂载/卸载负责。`available=false` 的 discard 必须带 generation fence，避免切走再立即回来时迟到 discard 杀掉刚恢复的实例。

`WorkspaceShell` 是同窗口 child WebView 的最终 owner。占位组件卸载时的 `hideAll` 只负责局部可见性收敛，不能替代 owner 卸载时的 `discardAll`。owner cleanup 使用本地 generation fence：React StrictMode 的模拟卸载若紧接着重新挂载，旧 cleanup 必须失效；真正离开 Shell 时则关闭全部实例。关闭顺序固定为先 best-effort hide，再执行 native close，close 成功后才从 registry 删除；close 失败必须返回结构化错误并保留 registry 项，允许重试，同时优先让图层退出命中区域。

活 WebView 上限 **5**。超出淘汰最久未访问且非当前页的实例。不以 1GB 作为第一版主阈值。

## 9. 安全

- 浏览用子 WebView **零 Tauri IPC**。capability 不得授予任何 Gold Band command。
- 与主界面 WebView 使用不同的用户数据目录：`{appData}/browser-profile/`。退出时不 `clear_all_browsing_data`。产品可后续提供「清除浏览数据」，第一版不做入口也可以。
- 本地 HTML 经现有文件链接 resolver 得到 canonical path 后，以 `file://` 加载，并按该文件父目录做导航白名单。相对资源只覆盖已授权目录。禁止把任意 `file://`、`tauri://`、`ipc.localhost` 暴露给不可信页。
- `gold-band-preview` 仍用于图片预览；其 CSP 不能承载可执行 HTML，故不作为内置浏览器文档协议。
- 不从其他浏览器读 Cookie 文件。

## 10. 下载

不做下载管理器。仍要有明确策略，避免 Windows 默默写入「下载」文件夹、macOS/Linux 点击无反应。

1. 拦截下载事件。
2. 弹出系统「另存为」。
3. 用户确认则保存该文件，取消则结束。
4. 无应用内列表、进度条、队列。

blob、必须 POST、或另存为拿不到完整字节时，返回结构化错误码，前端显示本地化文案。不为此做重试队列。

## 11. 加载反馈

子 WebView 没有 Gold Band 呼吸动画。重建或首次创建期间：

1. 原生实例尚未创建时，网页图层不存在，占位盒内使用现成 `BrandLoadingState`（与会话加载同一呼吸 logo）。
2. 原生实例创建成功后立即 show，让 WebView 渐进渲染页面；不得等待 `load-finish` 才显示。
3. 前端和 Rust `on_page_load` 都不得用 `load-start` / `load-finish` 改变页面可见性；这两个事件只驱动工具栏的停止/刷新状态与有界恢复计时。

加载开始会启动与页面绑定的 15 秒有界恢复定时器：新建远程页、提交地址、收到 `load-start` 时都会重置该定时器；收到完成、停止、关闭、丢弃或原生 create 失败时立即取消。超时只恢复工具栏状态，不伪造页面完成事件，也不轮询。即使站点持续报告 loading，已创建的网页仍保持可见和可交互。create 失败后必须清掉 loading，不得让呼吸 logo 永久挡住工作区。

遵守现有 `prefers-reduced-motion`。已存活页之间切换不走全屏呼吸。后续同页导航的细进度条不做第一版。

## 12. 平台

Windows / macOS / Linux 共用占位同步、show/hide、内部页、profile、另存为。Linux 已知 Wayland 下子 WebView 可能错位，第一版不增加发行版特判；功能在可对齐的环境交付，错位记为已知限制。

## 13. 错误

后端只返回 `CommandErrorVm { code, params }`，不写对客文案。至少：

| code | 含义 |
|---|---|
| `browser.webview.create_failed` | 子 WebView 创建失败 |
| `browser.webview.unavailable` | 当前平台/能力无法创建子 WebView |
| `browser.webview.operation_failed` | show / hide / close / navigate / bounds 等原生操作执行失败；`params.operation` 标识操作 |
| `browser.navigation.invalid` | URL 不合法或 scheme 不允许 |
| `browser.download.unsupported` | 该次下载无法另存为 |
| `browser.download.cancelled` | 用户取消另存为（若需与失败区分） |
| `browser.local_html.grant_failed` | 本地 HTML 路径无法授权或不是可读 HTML |
| `browser.page.limit_reached` | 内部页已达 32 个上限 |
| `browser.bookmark.limit_reached` | 门户书签已达 32 个上限 |
| `browser.bookmark.invalid` | URL 不是可加入的 `http(s)` 站点，或排序 ids 不匹配 |

前端按 code 显示中英文文案和可执行动作（重试、用系统浏览器打开）。

原生浏览器生命周期写入 `runtime.log` 的 `gold_band::browser` target。诊断至少包含 operation、`pageId`、WebView label、load event、registry 数量与成功/失败；远程 URL 只记录 scheme + host + port，本地文件只记录 `file://<local>`，不得记录路径、query、fragment 或页面内容。bounds 高频成功事件不逐帧记录，只记录创建时的初始矩形与失败。

## 14. 方案审视

过度设计：不把每个 URL 做成工作区资源；不按工作空间复制浏览会话；不用 1GB 内存做主阈值；不在启动预热 WebView；不导入外站 Cookie；不做下载队列。浏览会话单例比「每 scope 一份」更少状态。现有工作区 Tab、ContextMenu、BrandLoadingState、preview grant、文件链接 resolver 直接复用。

性能：未打开路径零增量。打开后最多 5 块 WebView，可见时每动画帧最多一次 bounds IPC。关右栏或 × 投影 Tab 必须停同步并丢弃实例，避免看不见时占内存。内部页摘要有界（建议内部页上限 32，超出拒绝新建或淘汰最旧非当前页标题，实现时在开发方案固定）。坐标同步不得触发会话 Markdown 重渲染。
