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
- 会话、Markdown 中的 `http(s)` 与本地 `.html/.htm` 点击后在此打开；工作空间与运行目录树中的 `.html/.htm` 默认打开源码，由浮层按钮再打开内置浏览器。
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

地址栏建议原先虽然使用 CSS `absolute`，但为绕开网页子 WebView 高于主 React WebView 的原生层级，又通过 `coverTop` 改写网页子 WebView 顶边；结果列表展开时网页可视区域被压缩，属于浮层边界设计缺陷。修复方向是复用 Tauri child WebView 能力，为建议列表建立一块受信任、可复用的独立子 WebView；它只拥有自身矩形和受限事件权限，网页子 WebView 的 bounds 始终不变。显示/隐藏使用单调 revision 收敛异步竞态，不复制浏览页或访问记录状态。

建议浮层首次聚焦约 1.5 秒才出现属于首帧就绪协议缺陷：浮层创建后保持隐藏，却要求页面连续执行两次 `requestAnimationFrame` 上报“已绘制”才显示；WebView2 会暂停隐藏表面的动画帧，因此正常路径无法完成，只能等待 1.2 秒兜底。修复方向是由 React 在建议行和主题同步提交后的 layout effect 上报当前 revision 已 ready，Rust 校验它仍是当前可见投影后立即显示；异常兜底继续保留，但不再进入正常关键路径。

对话栏菜单打开时网页变白，属于把「任意菜单打开即 hide」当成充分条件，过粗。平台约束只要求 HTML 浮层不得画进网页子 WebView 矩形。修复方向是对话栏以自身容器为 collision boundary，菜单不进入网页矩形；受该约束的菜单不走 hide，因为 hide 是原生 IPC，打开当下的未收敛几何一旦被当成相交，就会先 hide 再 show 闪白。hide 只留给全屏 Dialog / 非自身 Sheet，以及未受对话栏约束且与网页占位盒相交的菜单。地址栏建议仍用独立 child WebView，因为那块浮层必须压在网页矩形上。

本地 HTML 打不开属于原有设计缺陷：§9 原先把「已授权目录 + `file://` 导航」当作可用方案，但 Windows WebView2 对子 WebView 不保证 `file://` 导航与相对资源加载，实际表现为白屏且没有任何 load 事件；同时前端在提交地址时先写权威 URL、再异步下发原生命令，失败既不回滚也不给出结构化错误，重复点击还会在同一个 page 上并发下发多条原生导航。修复方向是改用与图片预览同源的 Tauri 自定义协议承载本地 HTML（按当前页已授权目录读取、逐段拒绝越界路径），并把「提交地址 → 原生命令 → 权威 URL 收敛」变成单一在途队列：同目标重复提交合并为一次，失败回滚到上一次确认 URL 并写结构化错误码，而不是新增第二套页状态或对某个文件类型打补丁。

不能把网页画在主应用 WebView 的 iframe 里：多数站点会拒绝嵌入，且会和 Gold Band IPC 同进程。子 WebView 是原生图层，不是 CSS div，必须按占位盒同步坐标，并在右栏折叠、Sheet、Dialog、切 Tab 时显隐。

启用 Tauri `unstable` 后，主 WebView 以 `WindowChild` 创建，runtime 只会给 `WindowContent` 挂 `TAURI_DRAG_RESIZE_WINDOW`。Win10 关闭 native shadow 后没有 DWM 外侧缩放框，冷启动就会失去四边拖拽，不需要先打开浏览器页。这是正确的无边框缩放设计下的宿主挂钩缺口，不是 Win10 阴影策略错误，也不该改成 HTML 缩放手柄。修复是窗口就绪后重新断言 `resizable` 以挂上 Tauri overlay，并在任何 child WebView `HWND_TOP` 之后把该 overlay 再抬到最前；其客户区有孔洞，建议浮层仍可在孔内接收点击。

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

工具栏：后退、前进、刷新/停止、地址栏。页内前进后退用 WebView 原生会话历史。电脑/移动切换左侧是书签图标：当前 `http(s)` 页可点，已加入用 `accent-foreground` 实心并带 `accent` 浅底，再点删除。门户页上该书签按钮不可用。地址栏聚焦且尚未改字时，在输入框下方浮层展开最近访问（最多 8 条，标题 + 精简 URL + origin 图标）；开始输入后过滤同一列表，普通词额外提供「搜索网页」。悬停或键盘选中访问项时显示 ×，删除该条访问记录，不关浮层、不跳转。浮层只在地址栏编辑中打开：回车提交当前输入（需方向键才选中建议项），提交后或页 URL 同步后关闭，不再自动弹出。浮层不挤占工具栏、页签或网页；网页子 WebView 的位置和尺寸在展开前后保持不变。内部页签和溢出菜单复用同一 origin 图标，没有图标时用 Globe。地址栏提交时，显式 scheme、域名、IP、localhost 与本地 HTML 作为地址；其他裸词和含空格文本按当前搜索引擎生成查询 URL。空页首次提交必须创建原生页，已有活页才 navigate。提供带 shadcn Tooltip 的「用系统浏览器打开」次要动作；对尚未提交的搜索词也使用当前搜索引擎解析。

电脑版 / 移动版是当前内部页的浏览意图，不是设置项，也不落盘。默认电脑版，工具栏在「用系统浏览器打开」左侧显示手机图标，表示下一步切到移动版；移动版时显示电脑图标。点击后在**同一活 WebView** 上更换 User-Agent 并 reload 当前文档：电脑版使用引擎默认 UA（切回时恢复创建时记下的引擎 UA），移动版使用固定 Android Chrome UA。reload 不加历史条目，前进后退继续使用该实例的原生会话历史。栏宽仍跟右侧工作区走，不模拟手机外框。纯 CSS 按宽度响应的站点可能看起来变化不大；按 UA 分流的站点（如 Google、百度）会切版。新标签默认电脑版，互不影响。

### 5.3 门户页

没有内部页、以及当前内部页是 `about:blank` 时，内容区都是门户页。固定首卡「打开空白页」，与工具栏 `+` 同一命令，不可拖、不可删。其余是书签卡：站点图标、截断名称、截断 URL，Tooltip 出全文；按加入顺序排列，可拖拽排序；悬停 × 删除。点书签：若当前是空白内部页则在该页打开，否则 `openUrl`。

书签身份是 origin。渠道内置列表在 `configs/browser-bookmarks.json` 按 `default` / `wb` 维护；当前渠道在首次打开浏览器时写入用户列表。之后只认用户列表。`wb` 内置内网站点（码灵官网、CI 平台、DPMS、AOMP、WeAPM、Pace+、CMDB），图标在内网打开门户时再拉取，不打进安装包。图标复用 origin favicon 缓存：访问过的站直接用；未访问的内置站只拉取图标，不写访问记录。

## 6. 链接打开

统一入口：`openWebTarget(href, source)`。禁止各处直接 `openExternalUrl` 打开 `http(s)` 或本地 HTML。

| 输入 | 行为 |
|---|---|
| `http://` / `https://` | 按“设置 → 通用 → 浏览器”路由；localhost 与普通网页默认均在内置浏览器打开，可分别切换 |
| 地址栏裸词 / 含空格文本 | 使用当前搜索引擎搜索；支持百度、Google、Bing |
| 会话 / Markdown 中的本地 `.html` / `.htm`（相对路径、绝对路径、`file://`） | 经现有文件链接 resolver 得到 canonical locator 后，用 preview grant 在内部页加载 |
| 工作空间 / 运行目录树中的 `.html` / `.htm` | 默认打开文件工作区 CodeMirror 源码；源码右上角浮层按钮再走内置浏览器 |
| 其他本地文件 | 现有文件工作区 |
| `mailto:` / `tel:` | 系统 opener |
| 页内普通左键 `http(s)` 链接 | 当前内部页跳转；站点 `preventDefault` / `target=_blank` 不得把点击吞掉 |
| 页内 Ctrl+点击、右键「新窗口」 | 内部新页，不创建系统窗口 |
| Agent 管理诊断帮助中的 ACP Registry | 始终走 `openWebTarget` 打开内置浏览器；不受“打开网站”开关影响，也不走系统 opener |

Markdown 会话链接、文件 Markdown 预览、源码管理 Markdown 外链，以及 Agent 管理诊断帮助中的 ACP Registry 共用该入口。Agent 管理页与快速对话、创建定时任务一样使用当前工作空间的 draft 右侧工作区投影，未打开工作区时右栏保持收起。

页内点击按手势分流，不按 HTML `target` 分流。普通左键在子 WebView 文档捕获阶段把 `http(s)` 链接改成当前页 `location.assign`，这样站点 `preventDefault` / `target=_blank` 不能把点击吞掉。Ctrl+点击、右键「新窗口」、以及仍然到达引擎的 `window.open` 才走 `on_new_window`，拦截为内部新页。Windows 上 External 子 WebView 对 `_blank` 锚点经常根本不触发该回调；Tauri `NewWindowResponse::Deny` 等于 Handled，也不会回退成当前页导航。因此左键契约必须在点击发生时落实，不能指望 Deny 之后再补一次 navigate。

### 6.1 浏览器设置

统一位于“设置 → 通用 → 浏览器”，复用现有设置持久化与 shadcn `Select` / `Switch`：

- 搜索引擎：百度（默认）、Google、Bing。
- 在 Gold Band 中打开 localhost 链接：默认开启。
- 在 Gold Band 中打开普通 HTTP/HTTPS 链接：默认开启。

这些开关只决定应用内容中的 `http(s)` 链接路由，按 localhost 与普通网站分开。会话和 Markdown 中的本地 `.html` 仍直接进内置浏览器，不由这两个开关控制；工作空间与运行目录树中的 `.html` 默认打开源码，由源码浮层按钮再打开内置浏览器。用户已经进入内置浏览器后，在地址栏输入的目标始终在当前内置页打开。对客标题写 “打开 localhost / 打开网站”，说明写“对话和文档里的本机服务地址 / 普通网站，关闭后改用系统浏览器”，不使用“本地链接”以免被理解成本地 HTML，也不列举 `127.0.0.1`、`::1` 或协议名。

## 7. 宿主与布局同步

主界面仍是一块 WebView。浏览内容是同窗口上的子 WebView，盖在 React 占位 div 上。

范式：

```text
BrowserPanel
├─ React 工具栏 / 内部页签栏
├─ AddressSuggestionWebView（按需 show/hide 的浮层）
└─ NativeBrowserViewport
   └─ 空 div（占位）
        → ResizeObserver + 布局 commit
        → rAF 合并，每帧最多一次
        → setPosition / setSize
        → Native WebView
```

约束：

- 使用 Tauri 官方 `Webview` 的 `setPosition` / `setSize` / `show` / `hide` / `close`。创建子 WebView 所需 capability 与 `unstable` 特征在开发方案中记录。Windows 上所有会创建、关闭或操作子 WebView 的命令必须是 async，禁止从同步 IPC 命令调用 `add_child`。
- 同步必须同时覆盖 **尺寸变化和位移**。仅 `ResizeObserver` 不够：左栏收起而右栏宽度不变时，`x` 会变、`width` 不变。Shell 在 `setLayout` 和面板 `onLayoutChanged` 上发出布局帧，占位组件收到后重新测量；`visualViewport` 的 resize 覆盖缩放。Sheet 开关与 Tab 激活仍走显隐恢复和 `pageId` 变化。这些后续测量继续走 rAF 合并，不写 React state。
- 坐标使用逻辑像素，与 `getBoundingClientRect()` 对齐，并处理 DPI。
- 同步热路径禁止每次回调两次无合并 IPC；禁止把逐像素写入 React 根 state。rAF 合并必须读取最新 bounds，不得把同一帧内的后续尺寸丢掉。
- 原生 create 未完成时占位盒仍可能继续布局；create 成功后必须把当时最新的占位矩形应用到 WebView，不能沿用发起 create 时的第一帧尺寸。实例变为 live 后要再同步一次，且不得因此 hide。create 失败时，若该页仍有已保留的原生窗口，仍把最新占位矩形写到窗口上，不能因为这次创建失败就停在旧位置。
- 占位组件的原生显隐生命周期只绑定 `pageId` 与可见性，不得因 title、loading 或 url 投影更新而卸载观察器或 `hideAll`。
- 改 URL 使用 navigate，不重建 WebView。只有新内部页才 create，淘汰或关闭页才 close。
- 门户 URL 仍测量占位盒并记住 bounds，但不 create / show；从门户提交地址或打开书签时用该尺寸创建原生页。
- 标签必须唯一且符合 Tauri webview label 规则。
- 活 WebView registry 与访问顺序只由 Rust `BrowserHostInner` 管理。达到上限并成功关闭淘汰页后，Rust 立即发送 `discarded { pageId }` 事件，前端只据此把对应页投影为 `live=false`，不得另建一套淘汰顺序或提前猜测候选页；即使随后新 WebView 创建失败，淘汰事实也必须收敛。
- 紧凑右侧工作区 Sheet 的 overlay 必须带稳定 owner 标记，使浏览器忽略承载自身的 overlay 并按 Sheet 占位盒展示。全屏 Dialog / AlertDialog / 非自身 Sheet overlay，以及未受对话栏 collision 约束且与网页占位盒相交的菜单，仍必须 hide 原生页。对话栏内 Select / Dropdown / Popover / Context Menu 以对话栏容器为 collision boundary，宽度不超过 Radix available-width，不得进入网页矩形；这类受约束菜单打开时保持网页可见，不得因打开瞬间的未收敛矩形触发 hide→show。
- 地址栏建议在普通 Web 预览中保留工具栏内的绝对定位回退；桌面端使用标签固定为 `gb-browser-address-suggestions` 的独立受信任 child WebView。该浮层只同步地址栏锚点、最多 9 条有界展示数据、选中索引和必要主题 token；位置优先在地址栏下方，空间不足时翻转到上方，并约束在应用视口内。不得改写网页 native host 的 `top`、不得 resize/hide 网页，也不得为建议列表复制访问记录 canonical state。

## 8. 生命周期与显隐

未打开过浏览器：不创建子 WebView、不创建 WebView 环境、不挂观察器、不跑同步 rAF、不预热 profile。面板模块懒加载。

| 动作 | 原生图层 | 活 WebView（≤5） | 内部页摘要 | 工作区投影 Tab |
|---|---|---|---|---|
| 从未打开 | 无 | 无 | 无 | 无 |
| 右栏可见且当前是浏览器 | 对齐占位盒 | 当前页必活，其余 LRU | 保留 | 在 |
| 切到文件 / Agent（deactivate） | 立刻 hide + suppress，停同步 | 保留（不丢弃） | 保留 | 在 |
| 切会话 scope（详情 ↔ 快速会话） | 页面不变，只按新占位盒重新对齐；不得 hide / show | 保留（不丢弃） | 保留 | 随新 scope 的投影 |
| 会话内设置 / 上下文 / 搜索等（Shell 仍在，右栏 scope 为空；Agent 管理除外，它复用 draft 投影） | 立刻 hide + suppress，停同步 | 保留（不丢弃） | 保留 | 随 scope 保存 |
| 手动收起右栏（workspace-close） | 立刻 hide + suppress，停同步 | 保留（不丢弃） | 保留 | 在（只是看不见）
| 窗口变窄自动收起，Sheet 未开 | 立刻 hide + suppress，停同步 | 保留 | 保留 | 在 |
| 窄屏 Sheet 打开 | 对齐抽屉矩形 | 留着 | 保留 | 在 Sheet 中 |
| 全屏 Dialog / 权限弹窗 / 未受对话栏约束且与网页占位盒相交的菜单打开 | hide | 留着 | 保留 | 不变 |
| 对话栏受 collision 约束的菜单打开 | 保持 show | 留着 | 保留 | 不变 |
| overlay 关闭且浏览器仍是可见激活资源 | resume 后按占位盒 show | 当前页若已丢弃则重建 | 保留 | 不变 |
| × 工作区「浏览器」（close） | hide | 全部丢弃 | **保留** | 去掉 |
| 内部关闭所有标签 | hide | 全部关闭 | 清空 | 仍在，空态 + |
| 离开会话 Shell（例如进入独立工作台设置页） | 立即 hide + suppress，停同步 | 保留（不丢弃） | 保留 | 随 scope 保存 |
| 应用退出 | 全部 close | 无 | 丢弃 | 丢弃；Cookie 保留 |

丢弃后的再打开：只重建当前页；其余内部页只显示标题，点到再创建 WebView。页内 JS 状态和滚动位置不恢复；Cookie 使登录通常仍有效。

展示状态与资源终止必须分离：右栏收起、切 Tab、切 scope、离开 Shell、进入设置页只做 `hide + suppress`，**不销毁**实例；只有用户明确关闭浏览器工作对象（`reason === 'close'`）、关闭内部页、LRU 淘汰和应用退出才 `discard`。因此 `BrowserNativeLifecycle` 必须同时评估 `available`、`requestedOpen`、`presented`、`autoCollapsedHidden`、`activeIsBrowser` 与 `scopeKey`：任一展示条件不成立才 suppress。`scopeKey` 变化只触发重新求值：浏览器会话是进程单例，切换会话时可见页并未改变，展示条件仍成立就不得对同一页面 hide / show，强制 hide→show 只会造成闪屏。

隐藏后重新显示必然是原生图层的一次 hide → show，中间会露出承载面板的 HTML 空白，因此首次测量与 `show` 必须在 layout 阶段（首帧前）发出，不得排进 `requestAnimationFrame`；只有 resize / observer 驱动的后续同步才走 rAF 合并。只有 `live` 翻转时才额外补一次同步，避免挂载时重复同步。

离开可见状态同样在 layout 阶段发出 `suppress`，早于下一页绘制，也早于会话页卸载时的被动 effect。子 WebView 不在 HTML 树里，占位盒卸掉不会让它消失；若把 `hide` 留到绘制之后，设置页、上下文页已经显示时网页仍会盖在原来的矩形上。浏览器一旦打开过，宿主模块已在内存中，这次调用直接 `suppress`，不再等待动态 `import()`。会话壳卸载仍用 generation 微任务避开 StrictMode 的模拟卸载，但该微任务由 layout cleanup 登记，仍在绘制前执行。

占位盒尺寸是 HWND bounds 的权威投影，与导航一样按 `pageId` 只有一个在途事务：同一页同时最多一条未完成的 `setBounds`，较新测量只更新排队目标；在途 IPC 完成后必须继续应用到最新 `lastBounds`，过期尺寸不得成为最终 HWND。`suppress` 期间不得把收起、展开中间态或未达 2px 的测量写入 native：小于 2px 的占位盒不得 `hide` 已经隐藏的实例，不论当时 `suppress` 标志是否已经由 `resume` 清掉，也不得改写 `lastBounds`；有效但不该展示的测量只更新 `lastBounds`，`resume` 后再按当前占位盒提交。`lastBounds` 只表示最近一次有效占位测量，不是原生窗口已经到达的矩形。是否调用 `setBounds` 要和上一次成功发给原生窗口的矩形，或当前在途目标比较。隐藏期间记下的新尺寸，即使 `resume` 后占位盒没有再变化，也必须提交。否则在别的会话改过侧边栏或右栏宽度后，回来时网页会停在离开前的矩形上，和已经更新的面板错位。另一条路径仍然保留：不得把隐藏期间的窄测量写进原生窗口，否则切回会话时会先按上次正确尺寸显示，再被 layout 前的窄测量改小，网页媒体查询闪到紧凑布局后又拉回。

导航同样只有一个在途事务，与页面身份绑定：同一个 `pageId` 同时最多有一条未完成的原生 create/navigate，同目标重复提交直接复用在途 Promise，不同目标只保留最后一个排队目标。权威 URL（`page.url`）只在原生命令成功后写回；失败时结束 loading、让页面保持在上一次确认的 URL，并把结构化错误码投到该页 notice。地址栏作为用户输入保留待修正内容，不回写失败地址为权威值。禁用「先写 URL 再发命令」和「同一页并发导航」，是因为前者会留下白屏但地址栏显示成功的假象，后者会随点击次数线性堆积原生调用，最终拖垮 WebView 消息循环并让整个应用无响应。

导航 notice 只表示这次还没成功。该页随后创建成功，或失败之后才开始的文档加载完成时，清掉这条导航错误。更早一次 load 的迟到 finish，以及另一页创建成功，都不清。下载取消或无法另存为的 notice 不随创建或加载完成清除。notice 仍是会话上的一个错误码，不按页复制一份列表。

`suppress` 语义固定为幂等的「隐藏并阻止迟到 show」，`resume` 为幂等的「解除抑制并通知占位组件按当前 bounds 重新 show」；重复调用不得产生额外 IPC 或重复 show。`resume` 只清标志位而不同步，会留下工具栏还在、网页全白的状态。`scope-change` 的 close resolver 保持 no-op：scope 切换由 `BrowserNativeLifecycle` 统一驱动，该组件位于 `RightWorkspaceProvider` 内部，子级 effect 先于父级执行；若父级 resolver 再 suppress，会把刚恢复的新 scope 页面重新隐藏。`deactivate` 与 `workspace-close` 走 suppress，只有 `close` 走 `discardAll`。

临时离开统一 suppress 而不是 discard，是为了保留页内 JS 状态、滚动位置、SPA 状态和原生导航历史，代价是隐藏实例仍占用内存并可能继续跑定时器、网络或音频，由 5 个活实例上限兜底。语义按 Tauri 子 WebView 定义，不绑定具体内核：`show/hide`、bounds、`close`、registry 与 LRU 对所有平台一致，Windows WebView2 / macOS WKWebView / Linux WebKitGTK 只作为平台实现细节。第一版不引入平台专属 suspend/冻结状态机；只有实测隐藏实例消耗不可接受时，才单独设计跨平台降级能力。

`WorkspaceShell` 仍是同窗口 child WebView 的最终 owner。原生网页的显隐只由 `BrowserNativeLifecycle` 写入；浏览器面板和占位组件卸载不得自行调用 `resume`、`suppress` 或 `hideAll`，避免同一 scope 切换产生重复 hide/show。占位组件只负责在 layout 阶段测量并把当前 `pageId + bounds + visible` 交给 host，后续 ResizeObserver/窗口变化通过 rAF 合并到 `ensurePage`；同一次测量不得再旁路发送 bounds IPC。owner cleanup 使用本地 generation fence：React StrictMode 的模拟卸载若紧接着重新挂载，旧 cleanup 必须失效。这条失效检查放在 layout cleanup 登记的微任务里，真实卸载仍在绘制前 `suppress`。无论是面板卸载、Shell 卸载还是 `available=false`，都只 suppress，不销毁实例，避免切走再立即回来时迟到 discard 杀掉刚恢复的实例。`available=false` 且宿主模块已加载时，在 layout effect 本体里同步 `suppress`，不等这个微任务。关闭顺序固定为先 best-effort hide，再执行 native close，close 成功后才从 registry 删除；close 失败必须返回结构化错误并保留 registry 项，允许重试，同时优先让图层退出命中区域。

地址建议浮层不进入活网页 LRU。聚焦/输入/键盘选中/主题或窗口尺寸变化时更新同一个实例；提交、Escape、主界面点到地址栏以外、浏览页子 WebView 获得焦点、浏览器整体隐藏或 owner 丢弃时 hide/close。地址栏因点击浮层而失焦不得 hide。show/hide revision 由模块级分配器跨组件挂载单调递增；整体隐藏还必须在 Rust 侧失效化在途 show，迟到请求不得覆盖较新的 hide。鼠标选择与删除通过带 revision 和稳定 item key 的事件回到主 WebView；主界面只接受当前可见 revision 且仍存在于当前建议投影中的项目。dismiss 只关闭浮层，不匹配建议项。

地址建议浮层按投影收敛，不按调用次数收敛：前端只有投影（bounds、条目、选中项、主题）真正变化时才发送一次请求，相同的重复同步事件不得再次发送；Rust 侧对浮层的创建/显隐串行化，并在持锁后读取**最新投影**决定最终状态。因此并发或交错的请求必须收敛到最新投影：较新的 show 让所有在途请求最终显示最新内容，较新的 hide 让所有在途请求最终隐藏；任何迟到请求都不得隐藏较新 show 已经显示的浮层。原生命令失败不得被静默吞掉，必须上报结构化错误码，并在下一次投影变化时允许重试。

地址建议浮层必须与浏览页子 WebView 走同一条原生创建路径：使用**独立 WebView2 数据目录**（`{appData}/browser-profile/address-suggestions`）与**绝对 URL**（由主 WebView origin 推导的 `…/index.html?surface=browser-address-suggestions`），不得与主 WebView 共享环境，也不得依赖应用相对路径。浮层必须记录自己的文档生命周期（create 完成时的文档 URL、`load-start`/`load-finish`）。判定浮层是否真的可用不能只看窗口是否可见：只有 WebView2 内容窗口存在、文档 URL 是建议面、且页面加载事件出现，才算渲染成功；仅有容器窗口（无内容子窗口）时是“透明可点击空窗”，会挡住网页点击却不显示任何内容，属于必须立即修复的故障。

浮层首帧必须无黑底闪烁：子 WebView 创建时使用透明默认背景并保持隐藏，只有建议文档已经执行、React 已同步提交建议行和主题、并通过当前 revision 的 ready 命令确认后才 show；同时保留一个 1.2 秒有界兜底，避免页面脚本异常时列表永久不可见。窗口位置/尺寸同步与显隐是两件事：bounds 可以在创建后立即设置，但首帧可见性必须等待内容提交，不得依赖“先显示、后加载”。

浮层的点击回传必须走**可校验、可审计的命令通路**：`browser_address_suggestion_action { revision, kind, key }`，由 Rust 校验（revision 非零、kind 仅 choose/remove/dismiss、key 非空且有界）并记录日志后转发给主 WebView；不得只依赖子 WebView 之间的 `emitTo`，因为浮层没有自己的诊断通道，跨 WebView 事件一旦被拒绝或丢失就完全不可观测。主 WebView 仍按 revision 校验；choose/remove 还要用稳定 item key 匹配当前建议投影。

点击浮层必然让地址栏先失焦（焦点转到原生浮层 WebView），因此不得把地址栏 `blur` 当成建议会话结束：那会先 hide 再在 `remove` 后 show，记录面板会闪一下。判定一次建议点击是否有效，必须使用**浮层当时正在显示的 revision**。`remove` 只更新剩余建议投影，不关浮层、不跳转。关闭只发生在选中跳转、提交、Escape、主界面点到地址栏以外，以及浏览页子 WebView 获得焦点（`dismiss`）。网页获得焦点由页面 WebView 的 focus 信号发出 dismiss，不能再靠地址栏失焦猜测。

地址栏草稿属于用户：地址栏里已有未提交改动时，网页带来的 `url` 事件不得覆盖已输入内容，也不得把“已输入”状态重置回“最近访问”。仅聚焦、尚未改字不构成草稿。提交后清掉这层保护。只有切换内部页（`pageId` 变化）可以在仍有未提交改动时用权威 URL 重写地址栏。否则同一个输入在“空白页”与“已打开页面”上会落到不同的建议模式：空白页稳定走过滤结果，已打开页面会被页面事件打回最近访问，表现为两处检索效果不一致。

同文档历史跳转不产生新文档，整页加载事件看不到它。`history.pushState`、`replaceState`、同文档后退/前进和 hash 变化仍要写回地址栏。Windows 上 `Source` 对一部分 `history.pushState` 保持为上一次整页加载的地址，`SourceChanged` 因此带不出新路径；`HistoryChanged` 之后由宿主读取文档的 `location.href`（WebView2 `ExecuteScript` 的 JSON 字符串，不是页面主动上报）。读取带递增 probe，文档加载会作废更早的读取，迟到结果不能盖住更新的地址。`SourceChanged` 在 `IsNewDocument == false` 时仍立即发布引擎 Source。macOS 观察 WKWebView 的 `URL`，Linux 观察 WebKit `uri`；这两处在整页加载进行中（`isLoading` / `is_loading`）不发布，避免和文档加载事件重复。不向浏览页开放 IPC。地址与上次已投影的位置相同则不重复发送。只有去掉 fragment 后的 `http(s)` 访问地址变了，才写入访问记录，避免页面滚动改 hash 时反复写历史文件。地址栏里已有未提交改动时，这条 `url` 事件仍然不覆盖草稿。

地址建议浮层必须位于所有浏览页子 WebView 之上：子 WebView 创建时会插到窗口 z-order 顶部，因此**在浮层之后创建的网页子 WebView 会盖住浮层**，只留下网页视口上沿之上的一条建议可见。每次显示浮层都必须显式把它抬到最前（`SetWindowPos(HWND_TOP, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)`），不得依赖创建顺序；判定标准是窗口子级 z-order 中浮层排在网页子 WebView 之前，且列表在网页打开时仍完整可见。Windows 无边框缩放 overlay（`TAURI_DRAG_RESIZE_WINDOW`）必须压在浏览页之上，否则贴边的网页 HWND 会吃掉窗口右/下边缘命中；显示浮层后必须把该 overlay 再抬到最前。overlay 客户区有孔洞，建议列表仍在孔内接收点击。

浮层的首帧主题必须在**文档开始**就注入：浮层入口是同一套应用资源，未注入主题时会先按样式表的默认深色主题绘制，再由 React 应用真实主题，肉眼表现为“第一次打开闪一下黑屏”。因此主题（`dark`、`themeId`、`colorScheme`、`visualQuality`、`materialModel` 与语义变量）必须随初始化脚本一起在页面脚本之前写入根元素，保证第一次绘制就是正确主题。

浮层的显示时机必须由“内容已同步提交”驱动，不能只用 `load-finish`，也不能等待隐藏表面的 `requestAnimationFrame`：前者不能证明建议行已经挂载，后者在 WebView2 隐藏时可能不被调度，形成只能依赖 1.2 秒兜底的就绪死锁。建议面在 React layout effect 中应用主题并通过 `browser_address_suggestions_ready { revision }` 上报；Rust 仅在“当前投影可见且 revision 匹配”时显示浮层。ready 上报缺失时才允许有界兜底显示。

活 WebView 上限 **5**。超出淘汰最久未访问且非当前页的实例。不以 1GB 作为第一版主阈值。

## 9. 安全

- 浏览用子 WebView **零 Tauri IPC**。capability 不得授予任何 Gold Band command。
- 地址建议 child WebView 只加载应用内受信任入口，capability 仅授予 `core:event:default`，用于把 choose/remove 事件发给主 WebView；不得继承浏览页权限，也不得直接读取历史文件或调用浏览器命令。
- 与主界面 WebView 使用不同的用户数据目录：`{appData}/browser-profile/`。退出时不 `clear_all_browsing_data`。产品可后续提供「清除浏览数据」，第一版不做入口也可以。
- 本地 HTML 不通过 `file://` 导航，而是经 `gold-band-browser-file://` 自定义协议加载：Rust 只在「当前页已授权目录」内按路径段解析请求，拒绝越界、编码分隔符和非 GET/HEAD，响应带 `no-store`、`nosniff` 和按扩展名推断的 `Content-Type`。返回给前端与地址栏的展示 URL 仍是 canonical `file://` 路径，协议 URL 只存在于原生图层内部。
- 授权目录只由「打开本地 HTML 时解析出的文件父目录」产生，不随导航扩散；页面一旦跳到 `http(s)`，该页的授权目录立即清空，协议请求随即 404，避免本地目录在浏览过程中被长期占有。授权目录的解析是浏览器专用轻量解析，不签发外部文件访问令牌、不启动文件监听，避免连续打开链接时堆积授权与 watcher。
- Windows 下 WebView2 只保证自定义协议以 `http://<scheme>.localhost/<path>` 形态参与导航（`wry` 仅改写初始 URL，`navigate` 不会改写），因此导航 URL 需要平台化生成；协议处理器同时接受 `http(s)://<scheme>.localhost` 与 `<scheme>://localhost` 两种形态。
- 禁止把任意 `file://`、`tauri://`、`ipc.localhost` 暴露给不可信页；`file://` 不再进入导航白名单，只保留 canonical 展示语义。
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

创建前的品牌呼吸不因系统 reduced-motion 停止。已存活页之间切换不走全屏呼吸。后续同页导航的细进度条不做第一版。

## 12. 平台

Windows / macOS / Linux 共用占位同步、show/hide、内部页、profile、另存为。Linux 已知 Wayland 下子 WebView 可能错位，第一版不增加发行版特判；功能在可对齐的环境交付，错位记为已知限制。

平台原生监听代码必须在对应系统的真实 Rust target 上通过编译门禁。macOS 的 `objc2::define_class!` 协议实现必须先把协议导入为标识符，再在宏内引用；`ivars` / `alloc` 所属 trait 必须显式进入作用域。Tauri `with_webview` 的 `Send` 调度边界只携带可发送的整数句柄，在目标 UI 回调内恢复 Objective-C retained pointer；调度失败必须立即释放所有权。跨平台日志接口使用 `Display + ?Sized` 统一接受字符串、具体错误和 trait object。PR Checks 使用 macOS runner 编译桌面 crate，避免 Linux / Windows 因条件编译跳过 WKWebView 集成错误。

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

本地 HTML 协议不引入本地 HTTP 服务、缓存、临时目录或第二份授权状态：协议处理器直接复用页面已持有的授权目录与 Tauri 异步自定义协议能力，读文件是单次有界读取（上限 64MB），请求之间无共享状态。导航收敛不新增页状态字段，只复用现有 `page.url` 与 `loading`，因此也没有引入额外的事实源。

性能：未打开路径零增量。打开后最多 5 块 WebView，可见时每动画帧最多一次 bounds IPC。关右栏或 × 投影 Tab 必须停同步并丢弃实例，避免看不见时占内存。内部页摘要有界（建议内部页上限 32，超出拒绝新建或淘汰最旧非当前页标题，实现时在开发方案固定）。坐标同步不得触发会话 Markdown 重渲染。本地 HTML 协议按需读取单个文件，不做目录扫描、不做递归遍历；同一目标连续提交不产生额外原生调用，因此性能风险与点击次数无关。
