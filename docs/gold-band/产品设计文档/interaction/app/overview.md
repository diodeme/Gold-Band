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
- 启动命令为 `npm run dev`，默认渠道构建命令为 `npm run build` / `npm run build:default`，wb 内网渠道本地临时构建命令为 `npm run build:wb`。
- Tauri updater 按构建渠道内置更新配置：default 指向 GitHub Release `latest.json`，wb 指向内网占位地址；两个渠道内置不同 public key，避免跨渠道更新包互相验证通过。default 渠道由 `release-please` 创建 draft release 后在同一 GitHub Actions workflow 确保 git tag 存在，并附加桌面安装包、签名和 `latest.json`；macOS arm64 使用 `macos-15`，macOS x64 使用 `macos-15-intel`，release publish 后客户端才会从 latest 地址看到更新。

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
- Light 分组提供白蓝默认浅色和暖色浅色；白蓝配色成为新的浅色默认。
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

## 11. 一句话总结

> 桌面端的基础模型是“左侧一级功能导航 + 右侧递进式任务编排页面栈”，任务从列表进入工作流，再进入 Round 详情查看节点、日志、会话、artifact 与 attachment。
