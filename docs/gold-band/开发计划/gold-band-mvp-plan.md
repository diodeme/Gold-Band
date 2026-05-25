# Gold Band Rust MVP 实现方案

## 目标

先实现一条最小可用闭环：

1. 读取 task + workflow
2. 跑 `worker`
3. 若产出 `节点输出产物`，跑 `worker`
4. 若有 `worker`，跑 `worker`
5. 按 control 规则做 `continue / retry / acceptance loop`
6. 通过 CLI 查看状态、artifact、open-session

原则：先跑通主链路，再补增强能力。

---

## MVP 功能边界

### 必做
- task / run 基础目录结构
- workflow snapshot
- DSL 解析与基本校验
- runtime state
  - `run.json`
  - `round.json`
  - `node.json`
  - `worker-ref.json`
- `worker` 调用 Claude Code
- `worker` 串行执行命令
- `worker` 调用 Claude Code
- canonical artifact 落盘
  - `节点输出产物`
  - `节点输出产物`
  - `验收输出产物`
- control engine
- CLI
  - `run start`
  - `run status`
  - `run continue`
  - `run retry`
  - `run kill`
  - `artifact show/list`
  - `run open-session`

### 暂不做
- 非 ACP provider 的长期独立可视化协议
- `progress.events` 精细事件模型（已被 ACP-first 会话可视化方向取代）
- raw stream 复杂映射（后续只作为 raw/debug viewer）
- VSCode 插件
- 复杂 doctor/test matrix
- 高级调度 / 多 run 并发 orchestration

### 桌面端 MVP 增量
- 使用 Tauri 2.x + Vite + React + TypeScript 生成桌面端应用。
- `src-tauri/` 作为桌面后端，通过 path dependency 复用 Rust core 的 `App`、runtime、storage 与 config。
- `web/` 作为桌面前端，实现左侧一级功能导航 + 右侧递进式任务编排页面栈；点击“任务编排”一级入口会重置到任务列表根页面。
- 前端通过 Tauri commands 读取 task/run/round/node/artifact view model，所有终局状态仍来自 canonical state。
- MVP 实现任务列表、任务工作流、Round 详情、上下文管理和设置页；任务详情并入任务工作流页，run 详情并入工作流页 run 分组；模型管理仅作为一级导航占位。
- 工作流作者态支持对 worker 节点在 AI 输出验证与人工 check 间二选一；开启人工 check 后 ACP 节点结束时暂停等待用户在会话面板点击“成功”或“失败”，再复用既有 success / failure edge 继续执行。
- 2026-05-02：前端已按 `docs/gold-band/产品设计文档/interaction/app/原型` 对齐应用壳、任务列表 Task Preview、工作流 execution history、Round 三块工作台和设置页本地偏好控件。
- 2026-05-02：补充浏览器调试 mock view model fallback；非 Tauri 浏览器环境使用 mock 数据，Tauri 环境继续使用真实 commands，方便后续用 Vite/浏览器验证布局。
- 2026-05-03：桌面端新增 workspace 选择、最近 workspace 记忆与默认项目根解析；Tauri dev 即使从 `src-tauri/` 启动，也会向上识别包含 `.gold-band/` 的项目根。
- 2026-05-03：任务列表改为固定比例列宽，避免右侧 Task Preview 同屏时横向溢出；刷新改为保留数据的局部进度反馈，首次加载使用骨架屏；未实现动作以显式禁用按钮展示，避免含义不清的更多菜单。
- 2026-05-06：任务列表刷新反馈区分手动与后台来源：自动轮询只静默更新数据，不触发表格顶部品牌色进度条或刷新按钮高亮，避免首页运行态每秒刷新造成黄色闪烁。
- 2026-05-03：桌面端 UI 从自定义全局 CSS 一次性迁移到 Tailwind CSS v4 + `shadcn@latest`；基础控件优先使用 shadcn/ui 生成组件，Gold Band 暖金深色语义沉淀为 token，API/view model/runtime 行为保持不变。
- 2026-05-03：桌面端任务编排 IA 收敛为任务列表、任务工作流、Round 详情三页；任务详情并入工作流页 task context，run 详情并入 workflow run 分组。
- 2026-05-03：Round 详情节点选择修复为前端 camelCase 状态、Tauri command snake_case selection 入参的显式转换；运行态自动刷新改为只看结构化 run/round/node 状态，避免历史 events 文本触发持续轮询和错误条闪烁。
- 2026-05-04：工作流 execution history 的 run 分组表格改为固定比例列宽，确保多个 run 卡片之间以及 run/round 行之间列边界稳定对齐。
- 2026-05-05：修复测试问题清单中的桌面端工作流与 Round 详情问题：工作流页展示 `workflow.json.control`，任务列表和工作流历史支持分页/排序/统一横向滚动，Round 详情使用 `round.json.trace` 展示真实执行路径，并将左下区域改为 Requirement / Log / Artifact / Attachment 动态 Tabs。
- 2026-05-05：桌面端国际化改为前后端协同：前端使用 `i18next + react-i18next` 翻译可见 UI，Tauri 后端提供轻量 translator 处理后端生成的标题、summary card fallback 与缺失内容提示，同时 VM 保留稳定 key/status 供前端翻译。
- 2026-05-05：补充验收修正：工作流 control 信息移入蓝图画板，面包屑等导航标签接入 i18n，任务列表分页布局改为响应式，execution history Action 列保持可见，Round 详情小窗口改为滚动而非裁切；面包屑当前页改为短金色渐变底线，可点击上级项 hover/focus 改为文字提亮与 primary 底边线反馈，任务 ID 作为不可点击上下文标签不显示 hover 底线。
- 2026-05-06：任务编排首页视觉层级收敛，summary cards 改为中性表面 + 小面积状态强调；Task Preview 改为固定 header + 内部滚动正文，执行统计窄栏单列展示，修复底部统计贴边/超出卡片的问题。
- 2026-05-06：任务列表 Task Preview 从固定右栏改为 shadcn/ui Sheet 右侧抽屉，初始不打开；单击任务行滑出，单击其他任务行直接切换内容，单击非任务区域、Escape 或关闭按钮收回。
- 2026-05-06：Round 详情页右侧 Detail Viewer 从常驻固定列改为 shadcn/ui Sheet 详情抽屉，释放实际工作图和信息流宽度；双击节点、右键详情/会话、点击信息流条目打开抽屉，支持固定详情持续对照；固定时抽屉切换为右侧占位面板，主工作区自动收窄。
- 2026-05-06：浏览器调试模式支持轻量 deep link：`/tasks`、`/tasks/:taskId/workflow`、`/tasks/:taskId/runs/:runId/rounds/:roundId`、`/settings`，用于 agent-browser 直达页面验证。
- 2026-05-07：任务工作流页顶部 task 摘要移除“当前状态：某节点正在执行”句子；Run/Round 记录与 Round 详情的当前节点展示改为可读化格式，组合展示节点类型、workflow 节点说明和原始 node id；Round 详情实际工作图从 workflow snapshot 补齐节点说明。
- 2026-05-07：修复 Round 详情实际工作图默认视口偏下和底部裁切的问题；GraphView 改为受控 viewport，按节点 bounds 和容器尺寸计算初始平移/缩放，并移除实际工作图超过父内容区的固定最小高度，确保打开页面时执行路径图边框与节点卡片完整居中展示；浏览器 fallback 对 `/run-024/round-001` 复现两节点失败验收图用于验证。
- 2026-05-07：任务工作流页工作流默认折叠，仅保留展开入口；展开后仍显示 control 规则条与只读 GraphView，首屏优先给运行记录。
- 2026-05-08：任务工作流页将工作流入口从页面内折叠条升级为顶部“工作流”生命周期卡片，按未创建/有效/无效提供新建、查看、修复动作；完整蓝图和 control 规则条进入右侧非模态抽屉。
- 2026-05-07：桌面端品牌 Logo 从临时菱形字形替换为用户提供的红蓝金无限环 SVG；Web 品牌区和 favicon 共用 `web/public/logo.svg`，Tauri 平台图标由同一 Logo 生成。
- 2026-05-07：修复任务编排面包屑上级项 hover/focus 高亮在页面跳转后残留的问题；可点击上级项改为纯 CSS 的 hover/focus-visible 临时反馈，Round 详情只保留当前 round 的常驻高亮。
- 2026-05-07：工作流 execution history 的 run 分组保持一致黑色背景，不使用黄色背景或左侧金线表达展开态，避免被误解为选中态；2026-05-08 起初始态所有 run 默认收起，点击整行或左侧箭头即可展开/收起。
- 2026-05-07：任务工作流页删除无效 Tabs、继续运行、停止 Run 和禁用态查看需求按钮；Workflow 与 Task Preview 的需求展示统一为单行 / 100 字截断预览，且仅在确实截断时显示完整需求入口；任务列表在当前右侧 Sheet 内切换到完整需求视图并提供返回图标。
- 2026-05-07：统一压缩桌面端卡片 header 与内容之间的过大空白；Round 详情左下信息流、Workflow 运行记录、Workspace 最近列表、Settings 表单卡片和遗留 Task/Run 详情页均移除 Card 默认 gap、覆盖 border header 大底部 padding，并降低内容区内边距。
- 2026-05-07：Settings 页面移除标题副文案、范围提示块，以及外观/语言卡片的辅助说明文案，保留主题切换与语言选择两组本地偏好控件。
- 2026-05-07：Settings 主题选择器升级为 `Sync with OS` 开关 + 条件化主题摘要 + 抽屉式主题选择；`desktopTheme` 扩展为 `system`、`light`、`light-warm`、`dark`、`black`，默认浅色调整为白蓝配色，Gold Band 深色升级为石墨香槟方向，保留暖金浅色并新增终端黑主题；`system` 会保留用户最近选择的浅色/深色变体；新增 `desktopFont` 偏好，浏览器调试模式优先使用 `queryLocalFonts()`，桌面端通过 Tauri `get_system_fonts` 枚举系统字体；前端验证继续通过 `/settings` deep link 使用 agent-browser 完成。
- 2026-05-08：字体选择模型从三套 CJK 预设收敛为一个内置默认字体 `app-default`（MiSans）+ 一个本机字体下拉列表；前端通过 `web/public/fonts/misans/*.woff2` 内置 `Gold Band MiSans`，默认字体预览保留彩色 sample，本机字体继续走系统枚举与浏览器 fallback 检测。
- 2026-05-08：Round 详情页移除左下“上下文”Tab，requirement 摘要上移到 Header，round 级状态保留在顶部指标区，节点详情改由工作图双击、右键菜单或详情抽屉按需查看；round 初始态不再展示单独的“编排事件”面板，Header 中“打开详情”替换为“打开日志”，节点日志由工作图右键菜单“查看日志”打开；实际工作图节点统一卡片底色，完成/失败/运行中等状态用圆形状态标记表达，产物/附件改为“产物:1”“附件:1”徽标，底部信息区只按当前选中节点的产物/附件渲染以避免切换闪烁，右键非选中节点时自动切换 selection，非固定详情抽屉用快速收起过渡后再展示菜单，日志详情长文本在抽屉内换行不撑宽容器。
- 2026-05-08：任务工作流页顶部 `Latest Run` 改为统一读取最新 run，右侧 `结果` 改为复用任务列表状态 badge（如“已完成”），并移除顶部 `产物` 聚合卡片；任务列表同步移除 `资源` 列，不再在主表格展示 `Axx / Pxx`，确保首页和工作流页都只保留任务主状态与最新 run 这类高价值字段。运行记录中的 run/round 也收敛为单一状态 badge：优先显示 outcome，无 outcome 时回退到 status，不再并排展示两枚状态标签。
- 2026-05-08：任务工作流页运行记录改为固定行高摘要列表；Run/Round 主行统一使用单行截断的 current node / pauseReason 摘要，展开后直接进入 round 明细列表，不再插入重复的 run 摘要条，避免不同分页因长文本换行导致列表高度和分页器位置抖动。
- 2026-05-09：任务工作流页进一步收敛首屏主次关系：新建 Run 移入运行记录 Header，需求摘要改为无轮廓的弱强调同名单行，运行记录增加稳定列头并用中性增强表面、缩进时间线和独立 Round 行背景强化 run -> round 父子层级；随后只压缩运行记录区域自身的 Header 与行高，页面标题区保持与其他详情页统一的 Page Header 间距。
- 2026-05-09：任务列表 Task Preview 抽屉改为上方完整需求框 + 框内滚动 + 复制 icon，底部固定单一“工作流”按钮；移除抽屉执行统计、查看产物入口，并校准任务列表 Action 列表头与“进入”按钮右对齐。随后继续收敛抽屉视觉：任务列表中的完整需求区与 Workflow / Round 详情复用同一套白底单框抽屉样式，不再保留额外的彩色外框，底部工作流保持强调色主按钮；共享的完整需求抽屉组件同步补充复制 icon，并收口到标题右侧。
- 2026-05-20：任务列表默认排序从 task ID 升序调整为降序，首页优先展示最新编号任务；切回 ID 列时也保持默认降序，减少用户每次进页后手动反转排序的操作。
- 2026-05-10：任务编排三页统一为后台每 10 秒静默刷新；Workflow 与 Round 详情补充手动刷新按钮；Workflow 顶部四张 stats 卡片对齐；Round 运行状态从标题旁移动到顶部结果卡；Workflow / Round 工作图节点与 Round 顶部当前节点卡都支持“前部展示 + 尾部截断 + hover 全文”。
- 2026-05-10：Workflow 运行记录改为单展开 accordion，同一时间最多展开一个 run，降低多条 run 同时展开时的视觉噪音；工作空间选择页主视觉图标改为复用 Gold Band logo；Run 分组行操作列没有操作时不再显示横线占位。
- 2026-05-11：Workflow 运行中 Run 的操作列提供查看与停止；停止会终止当前 provider 进程树并把 run 终止为 killed；最新 Run 未终止时禁用新建 Run，避免同一任务并发启动多个 workflow。
- 2026-05-13：Workflow paused Run 仍视为可停止的非终态，运行记录操作列需要展示停止；存在当前 round 时保留查看入口，completed 等终态不展示停止。
- 2026-05-13：Round 详情工作图节点主视觉改为终态优先展示 outcome，避免 `completed + failure` 显示绿色完成；顶部指标在终态 round 中将“当前节点”改为“结束节点”。
- 2026-05-13：ACP 会话审批等待卡片收敛为信息行 + 按钮行，按钮较多时不挤压标题；等待用户权限决策时停止当前步骤计时，并将 composer 收为紧凑等待状态。
- 2026-05-11：Round 详情工作图交互破坏式升级：单击节点打开结构化详情抽屉，节点资源进入二级抽屉，会话按 `progress.events` / `raw.stream` 分离，日志从会话中独立为分页日志抽屉；默认只检索最近约 1000 条热日志，全量日志保留 30 天。
- 2026-05-12：ACP-first 重构决策：废弃新增自研 `progress.events.jsonl` 精细事件模型，后续通过 ACP 调用 agent/provider，直接使用 ACP 统一后的 session events 在 Round 节点会话详情中展示原始 agent 过程；legacy Claude Code direct / raw stream 仅作为 fallback/debug，不再驱动新的可视化协议。
- 2026-05-12：Round 详情工作图节点状态视觉收敛：未选中节点保持白底卡片，当前节点仅保留随主题联动的状态徽标，暂停态显示“已暂停”而不是运行中，只有用户明确选中节点才使用卡片级蓝色边框 / 浅蓝底 primary 强调，避免当前态被误解为选中态。
- 2026-05-12：修正任务工作流页和 Round 详情页窄客户端响应式：运行记录五列布局只在足够宽度启用，不足时改为纵向紧凑栅格；Round 工作图 Header 的选中节点说明限制在剩余宽度内截断，避免标题被挤成多行或内容向右溢出。
- 2026-05-07：任务编排首页移除页面级 summary cards 和 ModuleBar 状态 tabs，全部任务 / 运行中 / 已完成改为表格内快捷筛选，可恢复 / 失败 / 配置异常改为状态筛选，并新增任务 ID、标题、需求与最新 Run 的关键字搜索；首页定位从运行态数据看板收敛为任务工作台。
- 2026-05-07：桌面端 UI 框架层级收敛为少卡片工作台规则：AppCard 与 Metric 弱化边框和阴影，Settings 页由三张独立卡片改为单主面板 + section 分隔，主题摘要、字体选项和本地字体预览降级为低对比选项行；各主题共享同一布局层级，Tauri command、view model 和偏好保存契约不变。
- 2026-05-14：ACP 会话 agent 输出接入紧凑 Markdown 渲染，用户 prompt 保持纯文本；标题不使用文章页大字号层级，只用加粗和轻量标识表达层级；本次不引入 Pretext，后续仅在纯文本日志/Raw frame 虚拟化行高预估等测量场景再评估。
- 2026-05-15：Round 当前节点处于 `error_blocked` 时不再显示成普通已暂停，而是用错误阻塞状态和危险色展示；该状态仍暴露“继续运行”入口，ACP 最新 error diagnostic 或 Raw frame JSON-RPC error 显示为会话顶部横幅，错误后的正常 agent 输出会自动清除横幅；恢复 prompt `继续/Continue` 按独立用户气泡展示，不拼到上一条需求气泡；ACP stop 超过 15 秒未收敛时自动熔断为 `paused + process_interrupted`。
- 2026-05-17：创建任务流程升级为“创建任务 -> 导入 txt/md requirement -> 创建 workflow -> 保存任务”；任务列表移除独立导入入口，工作流编辑器基于 `@xyflow/react` 支持拖拽节点、连接边、选择 Agent、配置 JSON 输出验证和 `$new-round` 边目标，创建任务 Sheet 标题栏右侧承载“保存任务”提交入口。任务级 workflow 写入 `authoring/workflow.json`，run 启动时冻结 `workflow.snapshot.json`，Round 详情继续展示运行态快照。人工 check 仅保留 UI 占位，后端 `worker` 兼容保留但新建默认模板不再生成。
- 2026-05-18：侧边栏“知识库”升级为“上下文管理”，首版提供角色管理；用户级 profile 存储在 `~/.gold-band/context/profiles/<name>-<id>.md`，项目级 profile 存储在 `~/.gold-band/projects/{project-id}/context/profiles/<name>-<id>.md`。工作流节点通过分布式唯一 profile `id` 引用，编辑器使用可搜索选择器，创建/更新时间使用本地 `YYYY-MM-DD HH:MM:SS`，运行时把 profile Markdown 正文注入 prompt bundle。
- 2026-05-18：默认角色扩展为方案、开发、审查、测试、验收、清理六类；默认 workflow 初始化时先同步默认角色，再将可见 profile `id` 绑定到 `plan/dev/review/test/accept/cleanup` 节点。默认路径更新为 `plan -> dev -> review -> test -> accept -> cleanup -> $end`，cleanup 为普通 worker 节点且不启用 AI 输出验证；保存 workflow 时集中校验必填字段、角色绑定和角色可见性，错误弹窗关闭后在字段处红色标注。
- 2026-05-20：修复 ACP JSON-RPC 帧判定：adapter 发起的 `session/request_permission` 即使与当前 `session/prompt` request id 相同，也按 inbound request 处理，不再误判节点已完成并提前进入 artifact 归一化。
- 2026-05-20：收敛 provider system prompt：未声明 `output` 的节点会被明确告知无需产出 canonical artifact 或查找 artifact/output 约束；当前节点上下文由 prompt 给出，前序产出仅按 prompt 明确给出的路径读取，`run_dir` 只作为这些路径的父级上下文，避免节点为寻找未声明产物或确认约束主动扫描 run 目录。前序节点结果统一进入 system prompt 的执行链、artifact 路径和 preview，不再以 `Current Feedback` 注入 user prompt；跨 round 链路用 `-$new-round->` 说明新轮次来源。
- 2026-05-21：ACP session 累计处理耗时改为净耗时，扣除 `session/request_permission` pending 到用户选择之间的阻塞式用户等待；该规则同时覆盖普通工具授权和 `ExitPlanMode` / keep planning 等 plan 决策。
- 2026-05-21：ACP 会话详情新增“系统提示”入口，从 raw frame 中解析 `session/new._meta.systemPrompt.append` 并用弹窗只读展示实际追加的 system prompt。
- 2026-05-23：continue 恢复路径改为重新渲染当前节点 system prompt，并随 `session/load._meta.systemPrompt.append` 传给 Claude Agent ACP；ACP 内部 create session 表示用 SDK `resume` 创建 query 进程，不改变 Gold Band 的 continue 语义。系统提示入口应同时解析 `session/new` 与 `session/load` 的追加内容。
- 2026-05-23：Codex ACP 0.14.0 会忽略 ACP `_meta.systemPrompt`；Gold Band 对 `codex-acp` 在 `session/prompt` 前内联当前节点 system prompt，避免首次调用丢失节点约束。
- 2026-05-23：桌面 ACP 会话面板的手动追问入口改为复用当前节点 prompt bundle，`session/load` 恢复旧会话后也重新追加节点 system prompt，避免用户追问时模型忘记输出 DSL。
- 2026-05-24：`max_attempts` 收敛为 round 内修复/重试预算，只统计 `failure` 修复跳转；超限时写入结构化控制失败原因。Round 详情工作图按逻辑节点合并多 attempt，以 attempt 标记和 ACP conversation 聚合展示 continue/new 会话差异；`session=new` 始终独立成可切换 conversation，只有后续 `session=continue` 才挂回被继续的 conversation；运行中 synthetic/provider echo 的同文 user prompt 只展示一条。
- 2026-05-21：工作流编辑器的节点 id 输入改为本地草稿提交，避免中文输入法 composition 阶段被受控值和 sanitize 打断；作者态画布普通节点直接展示原始 id，不再把 `test` 等默认模板名称本地化显示。
- 2026-05-21：AI 输出验证的 JSON 输出约束输入改为本地草稿 + 延迟校验，停止输入约 2 秒或失焦后再写入 DSL；自动 beautify 改为输入框右上角手动美化按钮，避免编辑半截 JSON 时被重排。
- 2026-05-25：桌面端接入 Tauri updater，按 `default` / `wb` 构建渠道隔离更新配置和 public key。default 渠道指向 `https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json`，`release-please` 在创建 draft release 后会先确保对应 git tag 指向 release commit，再于同一 workflow 构建 default 桌面安装包、签名并上传 `latest.json`；macOS arm64 使用 `macos-15`，macOS x64 使用 `macos-15-intel`；publish 后客户端才通过 latest 地址看到更新。独立 `Release` workflow 仅作为手动输入 tag 的重建 fallback。wb 渠道使用内网占位地址，本地 `npm run build:wb` 打包后由人工上传内网包与 JSON。
- 2026-05-25：设置页改为 `通用 / 外观 / 高级` tabs，高级页支持保存用户级 `desktopUpdaterUrlOverride`、恢复内置地址、手动检查更新和展示后台检查状态；用户覆盖 URL 不改变渠道 public key，避免 default / wb 串包；`desktopUpdaterLastCheckedAt` 持久化最近一次检查时间，展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`。
- 启动：`npm run dev`；构建：`npm run build` / `npm run build:default`；wb 本地构建：`npm run build:wb`。

---

## Rust 模块拆分

建议先用一个 binary crate，内部按模块拆，不急着一开始就上多 crate workspace。

```text
src/
  main.rs
  cli/
  app/
  domain/
  dsl/
  runtime/
  provider/
  worker/
  storage/
  control/
  artifacts/
  inspect/
  util/
```

---

## 模块职责

### 1. `cli/`
负责命令行入口和参数解析。

建议使用：
- `clap`

子命令先做：
- `task show`
- `run start <task-id>`
- `run status <run-id>`
- `run continue <run-id>`
- `run retry <run-id>`
- `run kill <run-id>`
- `run open-session ...`
- `artifact list/show`

CLI 只做参数解析和调用 app service，不直接碰底层细节。

### 2. `domain/`
放最核心的 typed model。

例如：
- `RunStatus = Running | Paused | Completed`
- `RunOutcome = Success | Failure | Killed`
- `NodeType = Worker | Exec | Verify`
- `NodeOutcome = Success | Failure | Invalid | Killed`
- `SessionMode = New | Continue`
- `ExecCommandStatus = Success | Failure | Skipped`
- `AcceptanceFailurePolicy = AutoLoop | Stop`

这一层尽量不依赖 IO，是整个项目的建模核心。

### 3. `dsl/`
负责 workflow DSL 的解析和校验。

包括：
- workflow 文件读入
- `nodes[] / edges[] / control`
- 合法性校验
- `$end`
- `goal -> taskInstruction` 的规则落地到 resolved config 前的准备

建议输出两层：
- `WorkflowDsl`：原始输入
- `ValidatedWorkflow`：校验后的可执行模型

### 4. `runtime/`
负责 run / round / node / attempt 的生命周期管理。

包括：
- 创建 run 目录
- 创建 round / attempt
- 写 `run.json`
- 写 `round.json`
- 写 `node.json`
- 写 workflow snapshot
- 更新 `currentRound/currentNode/currentAttempt`

### 5. `storage/`
负责文件系统读写和路径约定。

例如：
- `RunPaths`
- `AttemptPaths`
- artifact path resolver
- JSON read/write helpers
- atomic write

建议 runtime 不自己拼大量路径，统一走 storage/path builder。

### 6. `artifacts/`
负责 canonical artifact 的规范化、校验、落盘。

先做三类：
- `节点输出产物`
- `节点输出产物`
- `验收输出产物`

职责：
- schema struct
- parse / validate
- write canonical json
- 从 provider result 提取并校验 output artifact

### 7. `provider/`
负责 provider adapter 抽象和 Claude Code 实现。

建议先定义 trait：

```rust
trait ProviderAdapter {
    fn describe_provider(&self) -> ProviderInfo;
    fn doctor(&self) -> DoctorResult;
    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult>;
    fn open_session(&self, worker_ref: &WorkerRef) -> Result<()>;
}
```

内部再分：

#### `provider::invocation`
- A() 输入模型
- prompt bundle
- execution context

#### `provider::claude_code`
- Claude Code adapter
- prompt bundle -> Claude Code 命令映射
- session continue/new
- worker-ref seed 提取

MVP 只实现 `claude-code`。

### 8. `worker/`
负责执行 `节点输出产物`。

包括：
- 读取当前 round 最新 `节点输出产物`
- 串行执行 commands
- fail-fast
- 生成 `节点输出产物.json`
- 写 `stdout.log` / `stderr.log`

这一层不混 control 逻辑，只返回 worker 结果。

### 9. `control/`
MVP 核心。

负责：
- 根据 node result 归纳 outcome
- 查 edge
- 判断 `$end`
- 判断 `failure 边`
- 判断 repair loop / acceptance loop
- 计算下一步动作

建议做成纯逻辑模块：

输入：
- validated workflow
- current node
- node outcome
- runtime state
- capability info

输出：

```rust
enum ControlDecision {
    TransitionToNode { node_id: String, session: SessionMode },
    OpenNewRound,
    CompleteRunSuccess,
    CompleteRunFailure,
    PauseErrorBlocked,
    PauseInterrupted,
}
```

### 10. `app/`
应用服务层，串起 CLI、runtime、provider、worker、control。

例如：
- `start_run()`
- `continue_run()`
- `retry_run()`
- `kill_run()`
- `open_session()`

这层是 orchestration，不放太多 schema 细节。

---

## 核心执行主链路

### `run start`
MVP 主流程：

1. 读取 task
2. 解析 workflow
3. DSL 校验
4. 创建 run + `round-001`
5. 从 `entry` 开始执行 node

桌面端 `start_run` command 需要在第 4 步完成后立即返回初始 run summary，并把第 5 步交给后台线程执行，避免 UI 等待完整 workflow 跑完后才恢复响应。若最新 Run 尚未进入终止态，桌面端不允许继续新建 Run。

### `run kill`
MVP 行为：

1. 读取当前 run / round / node / attempt
2. 若当前 attempt 存在 provider 进程记录，则终止 provider 进程树
3. 将 run、当前 round、当前 node 写为 `completed + killed`
4. 后台 workflow 驱动在发现 run 已 killed 后停止推进，不再把 run 覆写回 paused 或 running

### 如果 node 是 `worker`
- resolve provider/profile
- 生成 invocation
- `goal -> taskInstruction`
- 调 provider
- 生成 artifact / worker-ref / node.json
- control 决定下一步

### 如果 node 是 `worker`
- 读取当前 round 最新 `节点输出产物`
- 执行 commands
- 写 `节点输出产物`
- control 决定下一步

### 如果 node 是 `worker`
- 组装默认 evidence package
- 调 provider
- 写 `验收输出产物`
- control 决定下一步

循环直到：
- complete
- paused

---

## MVP 状态机建议

### `worker`
- `success`
- `failure`
- `invalid`
- `paused`

### `worker`
- `success`
- `failure`
- `invalid`

### `worker`
- `success`
- `failure`
- `invalid`

### continue / retry
- `continue`
  - resume current provider session
  - 或 re-evaluate current invalid attempt
- `retry`
  - always new attempt
  - manual retry default `session = new`

### schema 输出修复规则
- 声明 `output.schema` 的 worker 输出不合法时，不走 edge。
- runtime 在同一 attempt / provider session 中隐藏追问 agent 修复输出。
- 隐藏追问最多 3 次；仍不合法则 workflow failure。

---

## MVP 文件落盘

### worker attempt
```text
attempt-001/
  node.json
  worker-ref.json
  artifacts/
    节点输出产物.json   # 如果有
  attachments/
```

### worker attempt
```text
attempt-001/
  node.json
  节点输出产物.source.json
  artifacts/
    节点输出产物.json
  commands/
    01-build/
      command.json
      stdout.log
      stderr.log
```

### output validation attempt
```text
attempt-001/
  node.json
  worker-ref.json
  artifacts/
    验收输出产物.json
```

---

## 推荐 Rust 技术选型

### 必要库
- `clap`：CLI
- `serde` / `serde_json`：schema
- `anyhow`：应用层错误
- `thiserror`：领域错误
- `tokio`：异步进程 / IO
- `tracing`：日志
- `camino`：UTF-8 path
- `uuid` 或时间戳生成 run/attempt id
- `indexmap`：若需保留 DSL 顺序

### 可选
- `schemars`：后续做 JSON schema
- `toml` / `serde_yaml`：若以后支持其他配置格式

---

## MVP 实现顺序

### Phase 1：先把骨架跑通
1. domain enums / structs
2. DSL parser + validator
3. runtime/storage path layout
4. CLI `run start/status`

### Phase 2：接通 worker
5. provider trait
6. Claude Code provider MVP
7. worker invocation + prompt bundle
8. worker artifact normalize

### Phase 3：接通 worker / output validation
9. worker runner
10. 节点输出产物 writer
11. output validation invocation
12. 验收输出产物 writer

### Phase 4：控制流闭环
13. control engine
14. continue / retry / kill
15. acceptance loop
16. `$end`

### Phase 5：可用性命令
17. artifact list/show
18. open-session
19. inspect/status 细化

---

## MVP 验证标准

### 测试目标

将本节作为 MVP 的主测试计划入口，用于验证 `worker-only 工作流` 主链路、repair loop、acceptance loop 与异常恢复机制是否形成可重复执行的闭环。

### 测试范围

- task / workflow 读取与运行初始化。
- `worker` 节点执行与 artifact 落盘。
- `节点输出产物` 产出后的 `worker` 执行。
- `worker` 执行与 run 最终状态收敛。
- `continue` / `retry` / `open-session` 等恢复入口。
- run 状态、artifact、session 等 CLI 检查能力。

### 不在本次范围

- 不验证超出 MVP 边界的高级调度、并发编排或额外 provider 扩展。
- 不用单一 happy path 代替异常恢复验证。
- 不用只看日志输出代替 run 状态、artifact 和会话状态检查。

### 测试前置条件

- 准备可运行的最小 task / workflow 示例。
- provider、运行命令与必要环境变量已就绪。
- runtime layout 可正常创建 run、round、artifact 和状态文件。
- 测试执行者可使用 `run start`、`run status`、`continue`、`retry`、`open-session` 等入口。

### 核心测试场景

#### 场景 1：`worker-only 工作流 -> success`

- 前置条件：`worker` 能生成合法 `节点输出产物`，`worker` 与 `worker` 均可成功执行。
- 操作步骤：启动 run，等待 `worker`、`worker`、`worker` 依次完成。
- 预期结果：run 最终状态为 `completed + success`。
- 关键产物或状态：worker artifact、worker 结果、output validation 结果、最终 run 状态均已落盘且可查看。
- 失败判定：任一阶段未产出预期文件、状态未收敛或最终状态不是 `completed + success`。

#### 场景 2：`worker failure -> repair -> worker success -> output validation success`

- 前置条件：首次 `worker` 会失败，系统允许进入 repair loop。
- 操作步骤：启动 run，触发 `worker` 失败，执行修复后重新运行 `worker`，再进入 `worker`。
- 预期结果：repair loop 生效，后续 `worker` 与 `worker` 成功，run 最终成功结束。
- 关键产物或状态：失败原因、修复后的新输入、重试记录与最终成功结果均可追踪。
- 失败判定：`worker` 失败后无法进入修复流程，或修复后状态、产物、轮次记录不一致。

#### 场景 3：`output validation failure -> auto_loop -> new round -> success`

- 前置条件：首次 `worker` 返回失败，系统允许进入 acceptance loop。
- 操作步骤：启动 run，执行到 `worker` 失败，触发自动 loop，进入新 round 后再次完成主链路。
- 预期结果：acceptance loop 生效，新 round 可以继续推进，最终收敛为成功状态。
- 关键产物或状态：output validation 失败原因、新 round 状态迁移、后续 round 产物与最终结果均清晰可追踪。
- 失败判定：`worker` 失败后未生成新的可执行 round，或 loop 行为与文档定义不一致。

#### 场景 4：`worker invalid / interrupted`

- 前置条件：`worker` 返回非法结果，或执行过程中被中断。
- 操作步骤：启动 run，触发 `worker` 非法输出或中断，再执行 `run continue` / `run retry`。
- 预期结果：恢复入口行为符合文档，能够区分继续执行与重新尝试的边界。
- 关键产物或状态：中断前状态、恢复后的 run / round 状态、重试结果与会话入口均可检查。
- 失败判定：恢复命令语义不清、状态被覆盖、产物丢失，或无法继续排查原因。

### 验收通过标准

- 上述 4 个场景全部至少成功验证一次。
- 每个场景都能同时验证状态流转、artifact 落盘与 CLI 可观测性。
- 异常场景必须能定位失败阶段，并能通过文档定义的恢复入口继续处理。
- 不允许出现 run 最终状态与实际产物不一致的情况。

### 结果记录方式

- 记录每个场景的输入、执行步骤、最终状态与关键产物路径。
- 记录失败场景的触发方式、恢复动作与最终结论。
- 回归时至少重复执行上述 4 个核心场景。

---

## 最小实现切片

### Slice 1
- DSL parser
- runtime layout
- `run start`
- 单 worker 节点
- worker artifact 落盘
- `run status`

### Slice 2
- `worker`
- `节点输出产物`
- repair loop

### Slice 3
- `worker`
- acceptance loop
- `$end`

### Slice 4
- `continue / retry / open-session`

---

## 结论

建议主实现语言使用 Rust，先围绕 CLI + runtime + Claude Code provider 跑通 MVP 闭环，再逐步补 provider 扩展、progress 观测与插件层。
