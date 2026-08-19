# Gold Band 桌面客户端交互概览

## 1. 一句话定义
Gold Band 桌面客户端是面向本地项目的 AI workflow 编排与观测工具。

它不是：
- CLI / TUI 的图形皮肤
- 聊天应用
- 终端模拟器
- 单页运行态大仪表盘

它是：
- 原生桌面应用壳
- 一级功能模块导航
- 任务编排的递进式工作区
- runtime 状态、工作流、产物、日志的可视化浏览器

---

## 2. 核心信息架构
桌面端采用固定应用壳：

```text
┌──────────────────────────────────────────────────────────────┐
│ Gold Band 桌面窗口                                            │
├───────────────┬──────────────────────────────────────────────┤
│ 左侧一级功能区 │ 右侧当前功能区                                │
│               │                                              │
│ Logo          │ 任务编排 / Agent 管理 / 知识库 / 模型管理 / 设置 │
│ 一级菜单       │                                              │
│ Settings      │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

左侧只负责全局一级功能切换；右侧承载当前功能的全部页面、导航和操作。

当前 MVP 只实现：
- 任务编排
- Agent 管理
- 设置中的主题切换
- 设置中的字体选择
- 设置中的语言选择
- 设置高级页中的更新地址覆盖、后台检查与手动检查更新
- 工作空间选择、切换与最近 workspace 记忆

以下一级功能仅占位：
- 知识库
- 模型管理

---

## 3. 任务编排的页面层级
任务编排不是单页，而是递进式页面栈：

```text
任务列表
  -> 任务工作流
    -> Round 详情
```

任务详情不再作为独立页面出现，它的 requirement 摘要、当前状态与运行入口合并到任务工作流页顶部。run 也不再作为独立详情页出现，而是任务工作流页中的分组行；round 是唯一的执行详情下钻页。

页面顶部显示面包屑导航：

```text
任务列表 > 任务01 > 工作流
任务列表 > 任务01 > 工作流列表 > run01 > round01
```

用户点击面包屑中的任意层级，可返回对应上级页面。

---

## 4. 页面文档
- [应用壳与一级导航](shell.md)
- [任务列表页](task-list.md)
- [任务详情页（已并入任务工作流页）](task-detail.md)
- [任务工作流页](task-workflow.md)
- [Round 详情页](round-detail.md)
- [Agent 管理页](agent-management.md)
- [上下文管理与角色批量导入](context-management.md)
- [设置页](settings.md)

---

## 5. 交互原则

### 5.1 一级功能与业务页面分离
- 左侧一级菜单只切换功能模块。
- 任务列表、任务工作流、round 详情都属于右侧任务编排功能区内部页面。
- 不应把 workflow DAG 直接放在应用首页。

### 5.2 桌面端使用直接操作
核心操作应通过：
- 按钮
- 菜单
- 右键菜单
- 面包屑
- 可点击节点
- 可点击 artifact / attachment
- 设置弹窗或设置页

不使用：
- slash command
- terminal 输入区
- chat input
- 自然语言命令解析

### 5.3 Canonical state 与观测信息分层
桌面端展示运行过程时必须区分：
- canonical state：task / run / round / node 的最终事实
- observability：events / logs / raw stream
- artifacts：runtime 规范化产物
- attachments：provider 或节点产生的附件

UI 不应根据日志直接推断 workflow 终局，终局状态以 canonical state 为准。

### 5.4 产物优先
任务编排不是看 Agent 说了什么，而是看：
- requirement 是否被满足
- workflow 执行到哪里
- 哪些节点产生了 artifacts / attachments
- validation 是否通过
- 失败时可从哪里恢复

### 5.5 工作台优先于数据看板
任务编排首页是任务工作台入口，不是运行态 KPI dashboard。

状态聚合能力应进入：
- 任务表格内的快捷筛选
- 状态筛选和关键字搜索
- 具体任务、run、round 的上下文信息

不在首页首屏展示页面级任务状态统计气泡或大数字 summary cards。

---

## 6. Tauri 2.x MVP 实现说明

桌面端 MVP 使用 Tauri 2.x + Vite + React + TypeScript 实现：
- Tauri 后端位于 `src-tauri/`，通过 path dependency 复用 Rust core 的 `App`、runtime、storage 与 config。
- 前端位于 `web/`，只负责桌面应用壳、页面栈、图形展示与直接操作。
- 前后端通过 Tauri commands 交换 view model，终局状态仍以 canonical state 为准。
- 桌面端 workspace 不依赖 Tauri 进程启动目录：启动时恢复用户记忆，或向上查找 `.gold-band/` 作为项目根；用户可通过原生目录选择器切换 workspace。
- 开发热加载启动命令为 `npm run dev`；需要固定当前源码快照、且不随前端或 Rust 文件修改热加载/重启时，使用默认渠道静态开发启动命令 `npm run dev:static`。该命令先将一次性 `web:build` 直接输出到本次进程独占的 `src-tauri/target/static-dev/<channel>/<snapshot>/frontend`，再让 Tauri `frontendDist` 只服务该不可变目录；退出后清理本次前端快照。后续其他进程改写 `web/dist` 不会触发当前客户端刷新，深层会话路由也不会在并行构建的清空窗口内落入临时 404。该模式同时关闭 Tauri source watcher，使用独立 Cargo target，并关闭 static dev 专用 Cargo dev profile 的 Rust debug symbols，避免与普通构建争用 Windows PDB 或触发容量限制；普通 `npm run dev` 的源码级调试能力不受影响。默认渠道构建命令为 `npm run build` / `npm run build:default`，wb 内网渠道本地临时构建命令为 `npm run build:wb`。
- Tauri updater 按构建渠道内置更新配置：default 指向 GitHub Release `latest.json`，wb 指向内网占位地址；两个渠道内置不同 public key，避免跨渠道更新包互相验证通过。default 渠道由 `release-please` 创建 draft release 后在同一 GitHub Actions workflow 确保 git tag 存在，并附加桌面安装包、签名和 `latest.json`；该 workflow 支持 `main` push 自动触发和 GitHub Actions 页面手动触发，便于 release-please 主链路补跑；项目处于 `0.x` 阶段时，breaking change 按 minor 版本发布，例如 `0.12.x` 的 breaking change 发布为 `0.13.0`，避免在产品尚未进入稳定版时自动提升到 `1.0.0`；manifest 始终使用 release tag 生成版本号和下载 URL，Windows 平台优先指向签名的 setup exe 安装包；手动 fallback 重建时应用源码来自 release tag，发布脚本来自所选 workflow 分支；macOS arm64 使用 `macos-15`，macOS x64 使用 `macos-15-intel`，release publish 后客户端才会从 latest 地址看到更新。PR checks 对完整合并树执行 `cargo fmt --all -- --check`，上游格式漂移必须先通过同一 formatter 收敛，不能因本次业务改动未触及对应 Rust 文件而绕过；跨平台文本契约按逻辑行断言，不绑定工作区的 LF/CRLF，依赖 Agent 可用性的运行夹具必须显式提供诊断事实。Runtime continue 的传输 prompt 保留隐藏控制段且对客投影只取 `display_text`；AI-DYNAMIC Merge 不拥有 completion contract，其 continue 隐藏段不得伪造 artifact 输出或 post-turn 归一化约束。
- Windows release 包按 GUI 桌面应用启动，不附带 cmd 控制台窗口；仅 debug/dev 构建保留控制台输出以便开发调试；后台子进程通过统一 process helper 启动，ACP provider、诊断清理、Toast AUMID 注册等 npx/codex/taskkill/reg/PowerShell 调用不弹控制台窗口。
- macOS release 不维护 signed/unsigned 两套产品逻辑。`tauri.conf.json` 默认使用 ad-hoc signing identity `-`；两种架构继续走同一 release job 和原文件名。CI 中 Apple 凭证全部缺失时只设置 `APPLE_SIGNING_IDENTITY=-`，部分缺失直接失败，完整时把全部凭证交给 Tauri bundler 签名、公证；构建后只做严格 codesign 完整性验证，不对未公证包执行必然失败的 Gatekeeper assess。
- Apple Developer Program 凭证尚未就绪期间，仓库提供独立 macOS 安装脚本作为临时分发路径，不在桌面产品内增加 unsigned 分支。脚本仅依赖 macOS 原生命令：latest tag 由 GitHub Release API 提供，DMG 与同名 `.sha256` sidecar 必须匹配，并在挂载后固定校验 `Gold Band.app`、bundle identifier 与 ad-hoc code signature；安装使用同文件系统暂存、旧版本备份和失败恢复，只有全部校验通过后才移除暂存 App 的 quarantine。历史无 checksum Release 不兼容。两条 release workflow 的汇总阶段上传与 release commit 同源的 `install-gold-band-macos.sh`，并为 DMG、macOS updater archive、Windows installer 和 Linux packages 流式生成 sidecar，内存占用不随资产大小增长；sidecar 与资产同源，只承担下载完整性和错配检查，不替代 Developer ID 与 Apple 公证。临时安装说明统一位于 `docs/guide/macos-install.md` 与 `docs/guide/macos-install.zh-CN.md`；英文和中文 README 只链接同语言指南，两份指南互设语言入口且共享相同命令块。用户通过 GitHub Release 的 latest 资产地址把脚本下载到 macOS 用户临时目录，下载成功后直接执行，不需要 clone 仓库；脚本默认解析 latest 版本，下载失败不得继续执行旧文件或空脚本，普通执行仍保留终端 stdin 供覆盖确认使用。
- ACP、MCP stdio、Agent doctor 和登录 Shell PATH 探测统一由 `ManagedProcessGroup` 管理。Windows 使用 Job Object 并继承隐藏控制台标志，macOS/Linux 使用独立进程组和 TERM→宽限期→KILL；正常生命周期不再按单个 PID 清理，持久化 PID/PGID 适配器只用于崩溃后的下次启动恢复。

MVP 范围：
- 实现任务列表、任务工作流、Round 详情、Agent 管理和设置页；任务详情并入任务工作流页，run 详情并入工作流页 run 分组。
- Agent 管理负责维护已配置 agent type、执行命令、环境变量与诊断状态。
- Worker / Verify 节点必须显式声明 `provider`，当前语义为 managed agent type；运行时不提供默认 Claude 兜底。
- 知识库、模型管理保持一级导航占位。
- 不提供 command bar、slash command、terminal input 或 chat input。

---

## 7. 2026-05-02 原型对齐记录

本轮前端实现按 `interaction/app/原型` 对齐桌面客户端：
- 应用壳保持左侧一级功能导航，右侧承载所有任务编排页面栈。
- 任务列表恢复原型中的“表格 + Task Preview”行为，单击预览、双击或按钮直接进入任务工作流。
- 工作流页恢复顶部模块条、task 指标条、原始 workflow 图与 execution history 两段式布局。
- Round 详情页恢复实际工作图、全局信息流和详情查看工作台。
- 设置页恢复 segmented theme 与语言选择，并保持用户级本地偏好语义。
- 浏览器调试环境下启用仅前端可见的 mock view model fallback，便于用 Vite/浏览器检查原型布局；Tauri 环境仍通过 commands 读取真实 canonical state。
- 默认桌面偏好改为 dark，避免 `system` 在浅色系统上破坏暗色原型的一致性；用户仍可在设置页显式选择 Light/System。

---

## 8. 2026-05-03 Tailwind/shadcn 重构记录

本轮桌面端前端从自定义全局 CSS 一次性迁移到 Tailwind CSS v4 + `shadcn@latest`：
- 基础控件优先采用 shadcn/ui 生成组件，包括 Button、Badge、Card、Table、Tabs、Select、Alert、Tooltip、Dropdown Menu、Scroll Area、Skeleton 等。
- Gold Band 暖金深色视觉语义沉淀为 Tailwind/shadcn token，保留 Light / Dark / System 主题偏好。
- 一级功能侧边栏 + 右侧递进式任务编排页面栈保持不变，未引入 command bar、terminal input 或 chat input。
- API/view model/runtime 操作合约保持不变，重构只替换视觉实现和组件组合方式。
- 状态色从全局 `.tone-*` class 改为显式语义映射，避免 Tailwind 动态 class 漏编译。
- 任务列表页继续使用 shadcn/ui 表格和按钮，但改为固定比例列宽、局部刷新进度反馈，并移除含义不清的更多菜单入口。

---

## 9. 2026-05-06 任务编排首页视觉修正记录

本轮基于桌面端截图反馈收敛任务编排首页视觉层级：
- 保持左侧一级功能导航 + 右侧递进式任务编排页面栈不变，未引入 command bar、terminal input 或 chat input。
- 首页 summary cards 从整卡状态色改为中性卡片表面 + 小面积状态强调，降低暖金色块和描边密度。
- 任务列表主区域缩小间距，表格继续使用 shadcn/ui Table、固定列宽和内部横向滚动，避免页面级横向溢出。
- Task Preview 改为固定 header + 内部 ScrollArea 的安全布局；执行统计在窄栏内单列展示，长 run id、中文/英文标签和按钮文案必须在卡片内换行或截断。
- 顶部 ModuleBar 与 action group 增加换行和最小宽度保护，避免按钮组在窄宽度下撑破内容区。

---

## 10. 2026-05-06 Task Preview Sheet 交互记录

本轮将任务列表预览从固定右栏改为 shadcn/ui Sheet 右侧抽屉：
- 首页主区域回到高密度任务列表，Task Preview 不再占用固定右栏宽度。
- 单击任务行打开右侧 Task Preview Sheet；抽屉已打开时单击另一任务行直接切换内容。
- Task Preview Sheet 使用非模态交互，不用遮罩阻塞列表；单击非任务区域、Escape 或关闭按钮收回。
- 抽屉内部继续保持固定 header + 内部滚动正文，执行统计、长 run id 和操作按钮必须在抽屉内安全换行或截断。

---

## 11. 2026-05-06 Round 详情抽屉化记录

本轮将 Round 详情页右侧常驻 Detail Viewer 改为 shadcn/ui Sheet 详情抽屉：
- 实际工作图和全局信息流默认占满主工作区宽度，详情不再长期挤压画布。
- 单击节点仍负责选择和更新下方上下文；双击节点、右键查看节点详情/会话、点击信息流条目会打开详情抽屉。
- 详情抽屉使用非模态、无遮罩交互；未固定时作为覆盖式 Sheet，固定后切换为右侧占位面板，让工作图和信息流自动收窄以便持续对照图和 JSON。
- 详情内容复用现有 DetailViewer 内容区和 CodeBlock，不自研基础抽屉控件。

---

## 12. 2026-05-06 浏览器调试 Deep Link 记录

本轮为桌面端 Web 调试模式补充轻量 deep link，不引入 React Router：
- `/tasks` 直达任务列表。
- `/tasks/:taskId/workflow` 直达指定任务工作流页。
- `/tasks/:taskId/runs/:runId/rounds/:roundId` 直达指定 Round 详情页。
- `/settings` 直达设置页。
- App 内部导航会同步 `history.pushState`，浏览器前进/后退通过 `popstate` 恢复页面状态。
- deep link 主要服务 Vite 浏览器调试和 agent-browser 验证；Tauri command、view model 与 canonical state 契约不变。

---

## 13. 2026-05-07 运行节点可读化记录

本轮修正任务工作流页和 Round 详情页中当前节点只显示内部 id 的问题：
- 当前状态、Run 分组行、Round 明细行和 Round header 均展示“节点类型 + 节点说明 + 原始 node id”。
- `run-tests` 等内部 id 继续保留用于定位 canonical state，但不再单独作为用户理解当前阶段的主文案。
- Round 详情实际工作图优先从 run 的 workflow snapshot 读取节点说明，避免真实执行图退化为纯 id 列表。

---

## 14. 2026-05-07 工作流蓝图默认折叠记录

本轮将任务工作流页的工作流改为默认折叠：
- 首屏优先展示 task 摘要、关键指标和运行记录，蓝图不再默认占据大块高度。
- 折叠态保留“工作流”标题与展开按钮，用户需要检查 authoring workflow 时再展开。
- 展开后仍显示 control 规则条与只读节点-边画布，不改变 Tauri command、view model 或 canonical state 契约。

---

## 15. 2026-05-07 品牌 Logo 替换记录

本轮将桌面端品牌标识从临时菱形字形替换为用户提供的红蓝金无限环 Logo：
- 左侧应用壳品牌区使用 `web/public/logo.svg`，保持 Gold Band 产品名和 AI Orchestrator 副标题不变。
- 浏览器调试 favicon 与 Web 侧品牌图共用同一 SVG，减少多份前端 Logo 资源漂移。
- Tauri 图标资源由同一 Logo 生成正方形源图与平台图标，Windows `.ico`、macOS `.icns` 和 PNG 图标使用一致品牌来源。

---

## 16. 2026-05-07 任务列表工作台化记录

本轮将任务编排首页从状态 summary cards 收敛为表格工作台：
- 移除页面级任务状态统计气泡，避免首页变成数据看板。
- `全部任务 / 运行中 / 已完成` 从 ModuleBar 移入任务表格工具条。
- 可恢复、失败、配置异常作为状态筛选出现，关键字搜索支持 ID、标题、需求和最新 Run。
- Workflow 和 Round 页面保留必要上下文摘要，但不把首页设计成 KPI dashboard。

---

## 17. 2026-05-07 UI 框架层级收敛记录

本轮将桌面端 UI 从多卡片、多色块拼贴收敛为更克制的工作台层级：
- 页面主体优先采用一个主工作面，内部用 section、低对比分隔线和留白组织内容。
- 卡片只用于真正独立的对象；设置项、字体选项、主题摘要和指标项不默认做成完整卡片。
- 所有主题共享同一套布局层级，主题 token 只负责换色，不改变页面结构。
- AppCard 与 Metric 默认弱化边框和阴影，减少浅黑色方块堆叠。

---

## 18. 2026-05-07 设置页主题选择器记录

本轮将设置页主题选择从 segmented Light / Dark / System 升级为 `Sync with OS` 开关 + 条件化主题摘要 + 抽屉式主题选择：
- `Sync with OS` 开启时保存 `desktopTheme = system`，并随操作系统浅色/深色变化自动解析到用户最近选择的对应模式主题。
- Light 分组提供瓷白和科技灰；瓷白作为浅色默认，科技灰提供更接近成熟桌面 AI 工具的冷中性灰工作区。
- Dark 分组提供石墨香槟 Gold Band 深色和新增终端黑主题。
- 主题和字体 token 继续沿用 Tailwind CSS v4 + shadcn/ui 的 semantic CSS variables；字体模型收敛为一个内置默认字体 `app-default`（MiSans）加一个本机字体下拉列表，不引入 command bar、terminal input 或聊天入口。

---

## 19. 2026-05-08 工作流入口抽屉化记录

本轮将任务工作流页的页面内“工作流”折叠条升级为顶部指标区的“工作流”生命周期卡片：
- 主页面只保留工作流状态与动作入口，状态包括未创建、有效、无效/校验失败等。
- 有效状态提供查看 / 修改，未创建状态提供新建工作流，无效或校验失败状态提供修复 / 修改。
- 点击动作打开右侧非模态工作流抽屉，抽屉内展示 workflow control 规则条与只读 workflow 图。
- 运行记录直接跟随 Header 下方展示，不再被工作流蓝图折叠条打断。

---

## 20. 2026-06-04 抽屉统一调宽记录

本轮为桌面端所有左右侧 Sheet 抽屉统一补齐拖拽调宽与本地宽度记忆：
- `SheetContent` 成为统一抽屉基座，右侧/左侧抽屉默认支持边缘拖拽调宽，不再要求每个页面单独实现。
- 各抽屉通过稳定的 `resizeStorageKey` 记忆最近一次宽度；同类抽屉下次打开时恢复用户上次使用的宽度。
- 首次打开时拖拽手柄不应抢占正文焦点；默认仅在边缘悬停时弱提示，拖拽过程中才高亮。
- 小窗口下宽度会继续受视口约束，避免抽屉被记忆宽度挤出屏幕。

---

## 9. 2026-05-04 工作流图视图记录

本轮桌面端工作流展示从卡片列表升级为真实节点-边图：
- 任务工作流页的原始 workflow 图使用只读画布，展示 authoring workflow 的节点、边、分支标签与 UML 风格节点卡片。
- Round 详情页的实际工作图使用可交互画布，支持缩放、平移、节点选中、双击详情和右键节点菜单。
- 图布局使用 `dagre` 基于有向边自动排布，节点渲染使用 React/Tailwind/shadcn 组合，状态色仍来自 canonical state 的 status/outcome。
- 当前实现只改变图形表达方式，不改变 Tauri command、view model 或 runtime state 契约。

---

## 10. 2026-05-03 三页 IA 收敛记录

本轮桌面端任务编排主导航收敛为三页：
- 任务列表：展示 requirement 摘要、当前状态和 Task Preview，双击或按钮进入任务工作流。
- 任务工作流：承载 task context、工作流，以及按 run -> round 展开的执行历史；run 只作为分组行，不再打开独立详情页。
- Round 详情：保持左上实际工作图、左下全局信息流、右侧 Detail Viewer；日志、会话、artifact、attachment 都在右侧查看。

任务详情页面合并到任务工作流页顶部上下文，run 详情页面合并到工作流页的 run 分组与 Round 详情上下文。

---

## 11. 2026-06-18 系统通知干预弹窗

编排器在人工确认 / 权限请求 / 执行错误 / 进程中断四类暂停场景下，通过 OS 系统通知单一表面主动提醒用户：Windows 保留现有 Toast、按钮、AUMID 与图标实现，macOS/Linux 使用 `notify-rust` 的 typed `NotificationResponse`。Windows Toast 首次发送时会幂等注册 AUMID 与开始菜单快捷方式；该注册属于后台非交互流程，所有 `reg` / PowerShell helper 必须通过统一 process helper 隐藏控制台窗口。

交互约束：
- 系统通知展示时长由 `configs/app-config.toml` 的 `notificationAutoDismissTargetSecs` 统一管理，当前产品目标为 20 秒。Windows 原生 Toast 只提供 Short（约 7 秒）和 Long（约 25 秒）两档，因此按最近档位解析为非持久的 Long，实际约 25 秒后自动收起；Windows 仍可能按用户的辅助功能通知时长设置调整实际展示时间。未点击的通知保留在通知中心，避免用户错过关键提醒。
- 通知无解决闭环。点击「忽略」或「查看详情」时由后端清 dedup key，允许同节点再次弹出；横幅自然超时不代表业务问题已处理，不清理 dedup key。
- Windows 通知正文点击、Windows「查看详情」、macOS/Linux 默认点击与 `view` action 统一生成 `ViewActionPayload`。后端先将 payload 放入待导航队列，再恢复或重建 `main` 窗口并发送“导航可用”信号；前端先注册监听，再通过 `take_pending_intervention_navigations()` 原子排空。该数据/信号分层保证窗口销毁后重建时不会因事件早于监听器而丢失或重复导航。
- 「查看详情」按当前 uiMode deep link 到对应节点：工作台模式定位到 Round 详情并选中节点；会话模式定位到会话 run 并在 sessionTree 内匹配节点 session。
- 弹窗只承载「提醒 + 跳转」，不承载决策本身——权限/人工确认的 Allow/Reject 仍走主干卡片与命令，与弹窗点掉是两个独立动作。

> 2026-06-19 更新：移除程序内部右上角弹窗，仅保留 OS 系统级 Toast。

数据契约与实现记录见 `docs/gold-band/开发计划/新增流程/系统通知干预弹窗.md`。

---

## 13. 2026-08-16 悬浮提示组件统一

- 桌面端产品提示统一消费项目 shadcn/Radix Tooltip 与全局 Provider，浏览器原生 `title` 不再承担视觉提示；`aria-label` 继续独立提供图标按钮的无障碍名称。详细交互约束以 `docs/gold-band/rules/ui-interaction.md` 为唯一真源。
- 统一范围覆盖窗口标题栏、工作流节点快捷操作与画布控制、会话/ACP 消息及 composer、轮次文件变更、附件、文件与源码管理、运行模式和定时任务页面，以及 Markdown 代码复制与图片下载控制。
- React Flow 与 Streamdown 等依赖会间接创建原生 `title` 的控制项必须通过官方扩展接口组合现有 Tooltip，保留原有缩放、复制、图片加载/下载、键盘 focus 和流式渲染能力，不修改依赖源码或在运行时扫描、删除 DOM 属性。
- React Flow 画布控制继续使用统一的 Lucide 线性图标；组合 `ControlButton` 时必须在工作流画布样式边界显式恢复 `fill: none / stroke: currentColor`，不得让依赖针对自带填充图标的通用 SVG 样式吞掉放大、缩小图标中的 `+ / −` 语义。
- Tooltip 只投影已有标签或路径，不新增业务状态；内容过长时允许换行或安全断词，提示层不得改变原控件布局与点击目标。

---

## 12. 一句话总结

> 桌面端的基础模型是“左侧一级功能导航 + 右侧递进式任务编排页面栈”，任务从列表进入工作流，再进入 Round 详情查看节点、日志、会话、artifact 与 attachment。
