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
- 2026-07-25：用户消息中的隐藏 runtime context 改为由当前可见内容统一驱动气泡宽度。隐藏根节点、Trigger、Content 使用无百分比宽度的嵌套 grid stretch；`82cqi` 只保留为消息列最大测量宽度。组件在该上限内以不可见副本进行真实排版，通过 `Range.getClientRects()` 获取各文本行宽度，折叠态取标签/可见正文最大值，展开态再纳入隐藏正文；ResizeObserver、展开状态和字体加载触发重测。由此删除固定 `rem` 与线性 `65cqi` 最终宽度，避免客户端越宽、气泡尾部空白越大的问题。
- 2026-07-22：默认工作流“需求采访”开关收敛为 workspace 级偏好，仅在内置 `default` 模板显示；自定义模板拓扑不受影响。elicitation 回答后不再生成独立用户消息气泡，保留 `AskUserQuestion` 工具卡片；response signal 改由 runtime 完成 JSON-RPC 回包后清理，修复 completed run follow-up 提交后卡在“发送中”。
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
- 2026-07-07：会话继续/追问语义补齐：`codex-acp` 仅在同一 ACP session 首轮将 stable system prompt 作为 hidden user block 内联并持久化审计，后续停止后继续、恢复继续和完成后追问不再重复发送或记录该 system prompt；普通 worker 与 AI-DYNAMIC internal worker 的继续输入统一支持本次新附件，且不重带任务输入附件或历史附件；会话消息流中的附件预览按 timeline `raw.attachments[].path` 分流，`task-inputs/` 继续读取 task 级 `authoring/inputs`，`user-inputs/` 按 attempt locator 读取本轮新附件。
- 2026-07-07：`$new-round` 控制边新增必填 `new_round_entry`。作者态在指向 `$new-round` 的 failure 边上展示“新 Round 起点”下拉，可选 `$entry` 或真实节点；`$entry` 表示当前 workflow entry。runtime 打开新 round 后按该字段选择首个节点，不再固定从 workflow entry 重入；保存规范化只在 `$new-round` 边输出该字段。
- 2026-07-09：历史 task / run 兼容旧 `$new-round` 边：运行启动、重跑冻结 snapshot、以及运行态读取 frozen snapshot 时，若 `$new-round` 边缺失 `new_round_entry`，snapshot 专用规范化会补为 `$entry` 后再走严格校验，并只把补齐结果写入本次 `workflow.snapshot.json`；`authoring/workflow.json` 不回写，作者态新建/保存 workflow 仍保持必填校验。
- 2026-07-08：默认工作流的 `accept.failure -> $new-round` 起点从 `$entry` 调整为 `dev`，避免验收失败后重复执行方案节点；默认 workflow 节点 goal 改为按桌面语言生成中英文文案，不再硬编码英文。
- 2026-07-08：工作流控制默认分支语义调整：节点产生 `success` 或 `failure` 后若没有匹配同类型 edge，runtime 不再进入 `error-blocked`，而是等价于隐式指向 `$end`，按当前 outcome 完成 run；显式 edge 仍优先。
- 2026-07-07：作者态工作流入口改为由画布拓扑自动派生：没有普通入边的真实节点显示“入口”标识；唯一入口候选自动写入 `workflow.entry`，多个或零个入口候选会阻止保存。`new_round_entry` 仅表示下一轮 Round 起点，不参与初始入口推导，也不会在拖线到 `$new-round` 时自动补默认值。
- 2026-07-09：作者态入口推导验收修正：failure 回退边指回 success 主链前序节点时不再计入初始入口入边，避免“开发 -> 测试 -> 验收，测试失败回开发”这类合法工作流被误判为没有入口；非回退的前向 failure 分支仍计入入边，防止分支节点被误识别为第二入口。
- 2026-07-07：作者态工作流画布自动整形改为使用 success 主链拓扑顺序，不再用 `workflow.nodes` 数组追加顺序判断边是否回退；后追加的前置入口节点连到原入口后会排到主链前方，failure 边继续作为分支/回退线。
- 2026-07-08：多 round 会话上下文收敛：会话树每个 round 的节点顺序优先使用 `round.json.trace.sequence`，不再受 `workflow.nodes` 数组顺序影响；普通 worker hidden runtime context 的 predecessor 默认只包含当前 round 已执行节点，只有 `$new-round` 入口节点额外包含本轮起点之前的稳定前缀节点；附件 locator 带 `round/node/attempt`，中文 hidden context 标题完成本地化。
- 2026-07-08：`$new-round` 首节点的 hidden context 新增进入本轮的触发原因：上一 round 最后触发 `$new-round` 的节点不进入 predecessor chain，但其 output artifact、预览和 attachments 会出现在入口节点的“最新前序流转原因”，用于让本轮入口节点理解为什么需要重做；本轮后续节点不继续携带该触发原因或历史稳定前缀。
- 2026-07-08：普通 worker runtime prompt 强化自由输出目录边界：attempt 根目录归 Gold Band runtime / ACP 管理，角色、任务或用户要求输出报告、脚本、过程记录等文件且未给绝对路径时，默认写入 hidden context 的 attachments 目录；hidden context 中的“附件目录”同时标注为本节点自由输出默认落点。
- 2026-07-08：默认审查 profile 明确只 review 当前开发节点 / 本轮迭代改动，优先使用 `dev-report.md` 中的文件与行号限定范围，缺失时退回当前 git diff；历史遗留问题只有被当前改动引入、放大或直接影响当前改动时才阻塞裁决。
- 2026-07-08：会话消息流新增 runtime control JSON 展示优化。普通 worker output contract 与 AI-DYNAMIC `dynamic-node-completion` 继续复用 Rust 端既有 JSON artifact 提取和校验链路；runtime 在实际消费控制输出或将非法 JSON 控制候选送入 repair 时为对应 ACP `textDelta` 写入 `raw.runtimeControlOutputDisplay`，前端只基于该标记把自然语言和控制 JSON 拆分展示，控制 JSON 收起态只显示单行 Gold Band 工作流控制条，非法候选使用告警色和告警图标，未标记 JSON 保持普通 Markdown 消息。
- 2026-08-01：会话 thought / reasoning 从 Streamdown Markdown 渲染切回 prompt-kit `ChainOfThoughtText` 纯文本展示，Markdown 标记按字面显示；展示层裁掉整段首尾空白并保留内部换行。assistant 正文继续使用现有流式 Markdown presentation。后端在独立完整 thought chunk 之间只补一个换行，避免多个思考粘连且不产生额外空白行，token 级 chunk 继续无缝累计。接口回归必须验证 `**...**`、列表符号和代码围栏不会生成 Markdown DOM，首尾换行不会撑高内容区，同时 active thought 收起时仍不占布局。
- 2026-05-07：桌面端品牌 Logo 从临时菱形字形替换为用户提供的红蓝金无限环 SVG；Web 品牌区和 favicon 共用 `web/public/logo.svg`，Tauri 平台图标由同一 Logo 生成。
- 2026-05-07：修复任务编排面包屑上级项 hover/focus 高亮在页面跳转后残留的问题；可点击上级项改为纯 CSS 的 hover/focus-visible 临时反馈，Round 详情只保留当前 round 的常驻高亮。
- 2026-05-07：工作流 execution history 的 run 分组保持一致黑色背景，不使用黄色背景或左侧金线表达展开态，避免被误解为选中态；2026-05-08 起初始态所有 run 默认收起，点击整行或左侧箭头即可展开/收起。
- 2026-05-07：任务工作流页删除无效 Tabs、继续运行、停止 Run 和禁用态查看需求按钮；Workflow 与 Task Preview 的需求展示统一为单行 / 100 字截断预览，且仅在确实截断时显示完整需求入口；任务列表在当前右侧 Sheet 内切换到完整需求视图并提供返回图标。
- 2026-06-12：新会话 UI 侧边栏会话行新增删除能力；hover 操作区补齐删除按钮，删除前弹出不可撤销确认，确认后删除 `~/.gold-band` 下对应 task 目录，并在系统支持时优先移入回收站；若存在运行中的 run，则拒绝删除并提示先停止。
- 2026-05-07：统一压缩桌面端卡片 header 与内容之间的过大空白；Round 详情左下信息流、Workflow 运行记录、Workspace 最近列表、Settings 表单卡片和遗留 Task/Run 详情页均移除 Card 默认 gap、覆盖 border header 大底部 padding，并降低内容区内边距。
- 2026-05-07：Settings 页面移除标题副文案、范围提示块，以及外观/语言卡片的辅助说明文案，保留主题切换与语言选择两组本地偏好控件。
- 2026-05-07：Settings 主题选择器升级为 `Sync with OS` 开关 + 条件化主题摘要 + 抽屉式主题选择；`desktopTheme` 当前支持 `system`、`light`、`light-gray`、`dark`、`black`，浅色分为瓷白与科技灰，深色分为石墨深色与终端黑；`system` 会保留用户最近选择的浅色/深色变体；新增 `desktopFont` 偏好，浏览器调试模式优先使用 `queryLocalFonts()`，桌面端通过 Tauri `get_system_fonts` 枚举系统字体；前端验证继续通过 `/settings` deep link 使用 agent-browser 完成。
- 2026-05-08：字体选择模型从三套 CJK 预设收敛为一个内置默认字体 `app-default`（MiSans）+ 一个本机字体下拉列表；前端通过 `web/public/fonts/misans/*.woff2` 内置 `Gold Band MiSans`，默认字体预览保留彩色 sample，本机字体继续走系统枚举与浏览器 fallback 检测。
- 2026-05-08：Round 详情页移除左下“上下文”Tab，requirement 摘要上移到 Header，round 级状态保留在顶部指标区，节点详情改由工作图双击、右键菜单或详情抽屉按需查看；round 初始态不再展示单独的“编排事件”面板，Header 中“打开详情”替换为“打开日志”，节点日志由工作图右键菜单“查看日志”打开；实际工作图节点统一卡片底色，完成/失败/运行中等状态用圆形状态标记表达，产物/附件改为“产物:1”“附件:1”徽标，底部信息区只按当前选中节点的产物/附件渲染以避免切换闪烁，右键非选中节点时自动切换 selection，非固定详情抽屉用快速收起过渡后再展示菜单，日志详情长文本在抽屉内换行不撑宽容器。
- 2026-06-11：修复节点完成后 workflow 不推进的问题：ACP timeline / token 读取从 orchestrator 主控制流剥离，指标开关关闭时不读取 token 文件，开启时也只在旁路任务中读取并捕获 panic；同时修复 UTF-8 字符截断 panic、JSONL/raw log 轮转的字节切片 panic、新 UI 首个 attempt 创建前的 `Agent 调起中` 空态、attempt 创建后首个可见消息前的 `处理中...` 占位，以及 ACP 状态旋转标识的 CSS 圆环动画。
- 2026-07-08：会话式运行页将 `runtimeActive` 但 ACP session 详情暂为空的状态统一显示为 `加载中...`，避免下一节点会话 hydrate 前暴露 `conversation.runtime.runtimeActive` 内部 key 或误导为上一会话的“拉起下一节点中”；补充前端单元测试覆盖 shell 状态解析。
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
- 2026-05-18：侧边栏“知识库”升级为“上下文管理”，首版提供角色管理；用户级 profile 存储在 `~/.gold-band/context/profiles/<name>-<id>.md`。工作流节点通过分布式唯一 profile `id` 引用，编辑器使用可搜索选择器，创建/更新时间使用本地 `YYYY-MM-DD HH:MM:SS`，运行时把 profile Markdown 正文注入 prompt bundle。
- 2026-07-09：工作流已支持多 workspace，profile 自定义角色收敛为用户级唯一来源；运行时不再读取项目级 profile，前端上下文管理和工作流 profile 下拉不再展示项目级概念，不提供迁移或双读兼容。
- 2026-05-18：默认角色扩展为方案、开发、审查、测试、验收、清理六类；默认 workflow 初始化时先同步默认角色，再将可用 profile `id` 绑定到 `plan/dev/review/test/accept/cleanup` 节点。默认路径更新为 `plan -> dev -> review -> test -> accept -> cleanup -> $end`，accept failure 新建 Round 后从 `dev` 重开，cleanup 为普通 worker 节点且不启用 AI 输出验证；保存 workflow 时集中校验必填字段、角色绑定和角色存在性，错误弹窗关闭后在字段处红色标注。
- 2026-05-20：修复 ACP JSON-RPC 帧判定：adapter 发起的 `session/request_permission` 即使与当前 `session/prompt` request id 相同，也按 inbound request 处理，不再误判节点已完成并提前进入 artifact 归一化。
- 2026-05-20：收敛 provider system prompt：未声明 `output` 的节点会被明确告知无需产出 canonical artifact 或查找 artifact/output 约束；当前节点上下文由 prompt 给出，前序产出仅按 prompt 明确给出的路径读取，`run_dir` 只作为这些路径的父级上下文，避免节点为寻找未声明产物或确认约束主动扫描 run 目录。前序节点结果统一进入 system prompt 的执行链、artifact 路径和 preview，不再以 `Current Feedback` 注入 user prompt；跨 round 链路用 `-$new-round->` 说明新轮次来源。
- 2026-05-21：ACP session 累计处理耗时改为净耗时，扣除 `session/request_permission` pending 到用户选择之间的阻塞式用户等待；该规则同时覆盖普通工具授权和 `ExitPlanMode` / keep planning 等 plan 决策。
- 2026-05-21：ACP 会话详情新增“系统提示”入口，从 raw frame 中解析 `session/new._meta.systemPrompt.append` 并用弹窗只读展示实际追加的 system prompt。
- 2026-07-28：ACP 系统提示弹窗默认使用现有 prompt-kit Markdown 渲染器展示，保留“渲染 Markdown / 原文”切换并通过独立本地偏好记忆，不与产物预览模式或 ACP session 数据耦合；多 attempt 切换继续沿用当前查看模式。
- 2026-07-28：ACP 系统提示弹窗在桌面断点显式覆盖 shadcn Dialog 默认 `sm:max-w-lg` 为 `sm:max-w-5xl`，解决调用侧普通 `max-w-*` 无法覆盖响应式默认值导致的窄弹窗问题；小窗口继续保留组件默认视口安全边距。
- 2026-05-23：continue 恢复路径改为重新渲染当前节点 system prompt，并随 `session/load._meta.systemPrompt.append` 传给 Claude Agent ACP；ACP 内部 create session 表示用 SDK `resume` 创建 query 进程，不改变 Gold Band 的 continue 语义。系统提示入口应同时解析 `session/new` 与 `session/load` 的追加内容。
- 2026-05-23：Codex ACP 0.14.0 会忽略 ACP `_meta.systemPrompt`；Gold Band 对 `codex-acp` 在 `session/prompt` 前内联当前节点 system prompt，避免首次调用丢失节点约束。
- 2026-05-23：桌面 ACP 会话面板的手动追问入口改为复用当前节点 prompt bundle，`session/load` 恢复旧会话后也重新追加节点 system prompt，避免用户追问时模型忘记输出 DSL。
- 2026-05-24：`max_attempts` 收敛为 round 内修复/重试预算，只统计 `failure` 修复跳转；超限时写入结构化控制失败原因。Round 详情工作图按逻辑节点合并多 attempt，以 attempt 标记和 ACP conversation 聚合展示 continue/new 会话差异；`session=new` 始终独立成可切换 conversation，只有后续 `session=continue` 才挂回被继续的 conversation；运行中 synthetic/provider echo 的同文 user prompt 只展示一条。
- 2026-06-04：AI-DYNAMIC 节点补齐与普通 worker 一致的权限模式配置，并将该权限作为 bootstrap / 派生 worker / merge / acceptance 的统一默认权限继承；权限字段最终以 provider doctor 返回的真实 ACP mode id 为准，产品侧虚拟权限名只作为可解析输入之一。Round 详情主图不再把 AI-DYNAMIC 仅视为一个复合占位节点，而是直接内联其实际执行的动态节点，并通过 outer locator 复用普通节点的详情、会话、Raw frame、artifact 与 attachment 查看链路。
- 2026-05-21：工作流编辑器的节点 id 输入改为本地草稿提交，避免中文输入法 composition 阶段被受控值和 sanitize 打断；作者态画布普通节点直接展示原始 id，不再把 `test` 等默认模板名称本地化显示。
- 2026-05-21：AI 输出验证的 JSON 输出约束输入改为本地草稿 + 延迟校验，停止输入约 2 秒或失焦后再写入 DSL；自动 beautify 改为输入框右上角手动美化按钮，避免编辑半截 JSON 时被重排。
- 2026-05-25：桌面端接入 Tauri updater，按 `default` / `wb` 构建渠道隔离更新配置和 public key。default 渠道指向 `https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json`，`release-please` 在创建 draft release 后会先确保对应 git tag 指向 release commit，再于同一 workflow 构建 default 桌面安装包、签名并上传 `latest.json`；该 workflow 支持 `main` push 自动触发和 GitHub Actions 页面手动触发，手动触发用于补跑 release-please 主链路；updater manifest 生成时显式使用 release tag，避免 workflow_dispatch 分支名进入 `version` 或下载 URL；Windows 平台优先选择签名的 setup exe 作为更新安装包；macOS arm64 使用 `macos-15`，macOS x64 使用 `macos-15-intel`；publish 后客户端才通过 latest 地址看到更新。独立 `Release` workflow 仅作为手动输入 tag 的重建 fallback，重建时应用源码来自 release tag，发布脚本和 manifest 生成逻辑来自所选 workflow 分支。wb 渠道使用内网占位地址，本地 `npm run build:wb` 打包后由人工上传内网包与 JSON；本地生成 `latest.json` 时必须优先匹配本次构建 version 对应的签名安装包，避免目录残留旧包时 URL 指回历史 exe。
- 2026-05-25：设置页改为 `通用 / 外观 / 高级` tabs，高级页支持保存用户级 `desktopUpdaterUrlOverride`、恢复内置地址、手动检查更新和展示后台检查状态；用户覆盖 URL 不改变渠道 public key，避免 default / wb 串包；`desktopUpdaterLastCheckedAt` 持久化最近一次检查时间，展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`。
- 2026-06-12：高级设置中“记录详细日志”“开启指标上报”的常驻说明文案改为与“使用本地 Claude”一致的 tips icon tooltip 形式，减少长说明占位；“开启指标上报”标题颜色与相邻设置项统一为 muted heading 样式；这两项开关位置也改为与“使用本地 Claude”一致，放到标题行内而不是远端右对齐。
- 2026-05-27：更新提示新增分层红点：后台发现当前可更新版本时，左侧 Settings、设置页 Advanced tab 和 Updates 分组标题同时提醒；Settings 和 Advanced 的已读状态按版本号持久化，用户逐层进入时只清当前层，Updates 红点仅在当前无可更新版本时消失。
- 2026-05-27：右侧主内容区顶部新增一次性更新公告区；首次发现某个新版本时展示公告，点击后弹窗引导用户前往 设置 → 高级 → 更新；公告关闭状态与可用更新快照一并持久化，重启应用后若版本仍可更新则公告继续可见，直到用户关闭或后续检查确认无更新。
- 2026-05-27：修正更新状态区的缓存展示语义；当重启后仅命中持久化的可用更新快照、实时 `updateStatus` 仍是 `idle` 时，UI 仍按“可更新”态展示状态文案、版本号和安装入口，避免出现“尚未检查”与可更新版本并存。
- 2026-05-26：Windows release 桌面包使用 GUI subsystem，安装后双击启动不再附带 cmd 窗口；debug/dev 构建仍保留控制台输出。后台子进程统一通过 process helper 设置隐藏窗口，Windows 进程树清理丢弃 `taskkill` stdout/stderr，ACP provider 的 npx/codex 等子进程同样不弹控制台窗口。
- 2026-06-04：桌面端左右侧 Sheet 抽屉统一支持边缘拖拽调宽与本地宽度记忆；`SheetContent` 负责默认调宽能力、视口边界约束和 localStorage 持久化，各页面只补稳定 `resizeStorageKey` 与宽度上下限；修正首次打开任务预览时拖拽手柄抢占焦点导致的蓝色高亮，要求手柄默认隐藏、悬停弱提示、拖拽中再高亮。
- 2026-06-11：会话式运行页 compact composer 用量栏恢复具体处理状态标签，运行中必须展示“思考中...”/“工具调用中...”等当前步骤文案；后端工作流在节点完成后立即持久化下一节点或新 round 的 `run.current* / round.trace / node.json`，并隔离 metrics 回调 panic，避免出现当前节点已 completed 但工作流长期停在 running 旧节点的状态裂缝。
- 2026-06-11：修复新 UI ACP 会话的跨节点自动跳转策略；前端把“是否允许自动跟随 running session”提升为显式状态，只有当前消息窗口贴底且用户仍在跟随当前运行会话时，新的 ACP live event 才会把选中会话切到下一运行节点。用户手动切到其他 session 或滚离底部后，后台节点继续运行，但不会再抢占当前会话视图；run VM 刷新若未命中自动跟随条件，必须保留既有 `selectedSessionKey`，并且手动切换与已排队的 live refresh 冲突时，手动选择优先。
- 2026-06-12：会话页手动切换后的 auto-follow 判定改为基于 `run.activeSessions` 是否包含当前选中 session，而不是依赖叶子节点自身的 `runtimeDisplay.tone`；这样已完成节点在树状态短暂滞后时，也不会被误判为仍应跟随并再次跳回后台运行节点。
- 2026-06-12：修复新 UI 默认选错 session 的问题。run VM 无显式 `selectedSessionKey` 时默认按 attempt 开始时间选择最新 session，避免 task-040 这类最新 `开发/attempt-002` 被 workflow 顺序最后的 `测试/attempt-001` 抢占；`process-interrupted` 可继续态仍保留 composer 输入触发 workflow runtime continue 的既有设计。
- 2026-06-12：补齐会话页运行中停止链路并收敛为统一入口。新 UI composer 不再在前端区分普通 ACP prompt 与 workflow runtime continue，而是统一调用桌面 `stop_active_session`；后端内部判定 run running 时复用既有 `App::run_pause` 完成 run 暂停、当前 attempt cancel、provider pid 清理和 dynamic descendants 暂停，run 已非 running 但 ACP 追问仍活跃时复用 `cancel_acp_session` 停止该 ACP session，避免前端和 Tauri command 层复制第二套停止逻辑。
- 2026-06-25：runtime 增加 `runtime-abnormal` 可继续异常暂停，用于本地 IO/资源、ACP transport 或 driver 异常，区别于 provider/model/workflow 前提错误导致的 `error-blocked`；JSONL append/roll/timeline overwrite 按同一路径串行化，避免并发写坏一行 JSONL；AI-DYNAMIC continue 前会先接受已完整落盘的 `dynamic-node-completion`，避免 session 已完成但 driver 异常暂停后重复发送；doctor ACP 目录改为临时/有界诊断产物，成功后删除、失败时只保留最近一次 bounded bundle。
- 2026-06-28：修复关闭应用/启动恢复后权限申请重复弹窗的问题。停止流程中的 attempt cancel 现在会同步把未决 ACP permission request 写成 `cancelled` response，并 upsert `acp.timeline.jsonl` / legacy `acp.events.jsonl` 的 `permissionRequest(status=cancelled)`；ACP prompt 的 cancelled/interrupted/error 收尾路径也会执行同一 pending interaction 收敛。`AcpSessionVm.events` 即使做分页裁剪也会附带每个 permission request 的最新终态，用来覆盖前端缓存中的旧 pending。重进页面只回放取消/已选择事实，不再恢复权限弹窗；迟到的旧弹窗授权不能把已取消权限反写为 `selected`。已选择的 `selected` 权限事件不会被停止流程覆盖。前端 ACP event 合并改为按 canonical permission request id 替换 permission 事件，不再把 `sessionId` 混入权限请求身份；后端 cancelled permission event 继承原 pending event 的 session/tool/raw 上下文，避免同一权限裂变为旧 pending 与新 cancelled 两条 UI 事实。
- 2026-06-29：ACP permission / elicitation 的 request-response JSON 文件收敛为临时信号文件，长期事实源统一为 timeline/events。runtime 消费响应并写入终态事件后会清理对应 request/response 文件；非 active session 的 command-side durable replay 写完终态事件后也会清理信号文件。停止流程写出的 cancelled response 保留到 live waiter 消费，避免提前删除导致阻塞线程无法解除。
- 2026-06-29：AI-DYNAMIC driver 热循环持久化改为按 `DynamicGraphState` 内容指纹去重；graph 未变化的 200ms worker 等待轮次不再重复重写 graph/run/node/group/proposal JSON，ready/launch scheduler 诊断事件也只在实际 promoted ready 或实际 launch 时写入，避免无意义磁盘 I/O 和 JSONL 心跳膨胀。
- 2026-06-29：ACP elicitation 卡片视觉密度收敛：已确认回答、多步骤进度、题干、选项行、自定义输入与底部操作区统一压缩上下留白和控制高度，保持会话流内联提问的轻量表单形态，不改变 request/response 协议与答案提交语义。
- 2026-06-29：前端构建类型检查拆分为生产源码配置 `web/tsconfig.build.json` 与 Vitest 测试运行配置；`npm run web:build` 不再把 Node 环境测试文件纳入浏览器源码编译，测试验收继续通过 `npm run web:test` 固化。
- 2026-06-29：wb 构建链路补齐 MCP stdio 握手实现对 `std::process::Command` 的显式依赖，保持新增 stdio MCP health/tools 探测逻辑可被 Rust 编译器稳定解析。
- 启动：`npm run dev`；构建：`npm run build` / `npm run build:default`；wb 本地构建：`npm run build:wb`。
- 仓库级依赖安装与锁文件统一使用 `npm` / `package-lock.json`；除非单独立项迁移包管理器，否则不新增 `pnpm-lock.yaml`、`yarn.lock` 等并行 lockfile。

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
- `pause_run()`
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

run 创建编号规则统一由 runtime 负责：普通启动和会话页重跑都扫描当前 task 的 `runs/` 目录最大 `run-NNN` 后递增，并先原子创建目标 run 目录占位，再写入 `run.json`、`workflow.snapshot.json`、`round.json` 和首个 `node.json`。前端不得根据当前选中的 run 推导新 run id；并发重跑时目录占位失败的一方必须重新扫描最大编号再分配。

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

### 2026-06-06：需求标题归一化实验工具
- 新增独立可运行的 Rust bin：`src/bin/requirement_title.rs`
- 目标：接收 requirement 文本文件路径，输出一个约 10 字左右的中文短标题，优先服务 txt / md / 纯文本导入场景
- 当前实现策略：采用结构优先 + 自然语言回退 + 轻量统计压缩的三层管线，不依赖大模型
- 具体顺序：先尝试抽取 H1/主标题等强结构信号；若输入缺少结构，则回退到前导主题句；若仍过长，再依据重复度、位置和技术实体显著性压缩标题
- 当前范围：先只支持中文，作为后续多语言 `language profile` 架构的最小切片
- 当前仓库主 lib 另有独立编译问题时，可单独用 `rustc --edition=2024 src/bin/requirement_title.rs -o .claude/requirement_title_standalone.exe` 验证该文件逻辑
- 常规验证方式：`cargo run --bin requirement_title -- <文件路径>`

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

## 2026-07-21：工作流长运行内存稳定性隐性优化

- 生命周期：桌面进程共享一个 `RuntimeLifecycleBus`，metrics、notifications、conversation-run-state 在 setup 以固定键幂等订阅一次；保留匿名订阅供测试和内部场景使用。
- ACP 传输：每 session route 使用 4 MiB / 256 帧无损 FIFO 背压，允许空队列单个超大帧；不影响 RPC pending response，不丢弃、不合并、不重排。
- Timeline：磁盘 `acp.timeline.jsonl` 是完整事实源，内存只保留当前 text/thought/plan 流、未终态 tool、未决 permission/elicitation 及 metadata/usage/timing；会话树只加载 metadata/lifecycle，选中会话才加载完整事件页。
- 日志：未路由 frame 仅记录摘要并按连接/事件类型限频；`runtime.log` 8 MiB 轮转、保留 4 份并继续执行 30 天清理，`acp.raw.jsonl` 保持现状。
- 兼容边界：Tauri command、Runtime API、ViewModel JSON、前端类型、既有事件窗口配置、75ms/125ms 流式刷新、消息/工具/权限/分页/自动跟随与 workflow 并行度全部不变；不包含 WebView 恢复和高内存降并行。
- 回归固化：覆盖具名订阅幂等、10,000 帧 FIFO、字节/帧背压、超大帧与关闭唤醒、热状态释放后 timeline 可读、tool input/output 合并、permission/elicitation timing、非选中不可读 timeline、日志限频和 size rotation。合入前必须通过 Rust workspace、Web test/build 与桌面 deep-link 验证。
- 本次结果：Rust workspace 全量通过；Release ACP route 10 项通过；Web 54 个测试文件、362 项通过且生产构建成功；桌面端现有 run/session deep-link 冒烟通过，测试实例与 dev server 已清理。字体测试仅修正元素定位，未改变 UI。

---

## 2026-07-22：ACP 流式 Markdown 顺滑呈现

- 根因修复：不再把 75ms/125ms 的 canonical snapshot 合并节奏直接当成视觉输出帧率，也不再把完整 snapshot 提前放入 DOM 后用透明字符模拟逐字。唯一活跃 text/thought item 使用局部 presentation controller 稳定推进可见 offset，消息框只按真实可见前缀增长，消除底部大块预留空白和跨 block 零散字符。
- Markdown：prompt-kit Markdown copy-in 使用模块化 `streamdown` 核心，流式阶段对当前可见前缀做不完整语法修复；完成后停止 presentation/incomplete repair，但已流式组件保持同一 block renderer DOM，重新加载的历史消息才直接 static。syntax guard 吞并纯 Markdown 控制符和未完成链接地址。Streamdown 不再启用全字符 opacity/stagger，避免 block 更新重播历史字符。思考过程与普通 assistant 消息统一实时 Markdown。
- Thought canonical：Claude Code 独立 thought chunk 原始数据不带换行，后端 accumulator 对完整 strong block chunk 写入段落分隔，token 级 chunk 保持连续；前端只展示 timeline canonical，不对旧会话增加内容修复或兼容重写。
- Thought 折叠生命周期：active streaming thought 收起时通过 Radix `forceMount + hidden` 保留 Markdown presentation 实例与 visible offset，再展开不重放；完成后的历史 thought 恢复普通按需挂载，控制常驻开销。
- 活跃生命周期：按最大 `endedSeq/seq` 的最新非 timing、非 optimistic 事件选择唯一 streaming item；tool、plan、permission 或其他生命周期边界到达后，旧 text/thought 不再继续动画。保留 timeline 稳定 id、工具/权限即时路径、分页和自动跟随。
- 性能边界：只有当前活跃尾部运行约 32ms 的呈现帧，历史消息无 timer；速率根据 backlog 在统一变量范围内自适应，单帧 elapsed 有上限，避免标签页恢复或大批次导致瞬间跳跃。不启用 Shiki、Mermaid、KaTeX 插件。
- 回归要求：必须通过 presentation/Markdown/活跃流接口单测、全量 Web test、生产构建，并在前端 deep link 中验证 thought 与普通消息的实时粗体、列表、代码围栏、容器无预布局空白、批次积压平滑追赶及 terminal 最终收敛。

---

## 结论

建议主实现语言使用 Rust，先围绕 CLI + runtime + Claude Code provider 跑通 MVP 闭环，再逐步补 provider 扩展、progress 观测与插件层。

---

## 2026-07-23：Direct 持续 Agent 会话

- 新增 Direct 运行模式：外观是单一持续 Agent 对话，底层复用单 Worker execution shell 和现有 ACP/session/storage 管道。
- Direct 使用 RawAgent prompt envelope，首轮与追问的 system prompt 均为空；不注入 Gold Band runtime/profile/hidden/output/repair 内容。
- 修复 completed run follow-up 生命周期：per-attempt live activity 区分真实 Starting/Running/CancelRequested 与 stale disk snapshot，页面重挂载后 composer、停止、耗时和 token 不丢失。
- 快速会话、runtime header、侧边栏和搜索完成 Direct 交互；Agent/model/permission 只在快速会话配置并按 workspace + Agent 记忆，运行模式管理仅保留工作流与 AUTO，Direct 历史使用 Agent icon 与 `lastActivityAt`。
- Direct 内部 `raw-agent` worker 不参与 profile 解析且禁止绑定 profile，避免角色解析阻断创建或向空 system prompt 注入 Gold Band 上下文。
- 回归范围包含 prompt、lifecycle、创建/config、前端 composer 状态、tab 顺序和 sidebar identity；合入前要求 Rust workspace、Web tests/build 与 `/chat`、Direct run deep link 实际验证通过。

## 2026-07-31：Direct 侧边栏活跃 turn 指示恢复

- 根因修复：Direct 用 Agent icon 替换 run 状态点且隐藏 run 子行后，侧边栏失去运行态入口；同时 completed run 上的后续追问不会把 `latestRun.status` 改回 running，因此不能在前端补一个基于 run status 的特例。
- 后端 `ConversationTaskRowVm.activity` 统一聚合 task 下 per-attempt live prompt activity 与首轮 runtime running 状态，覆盖 starting、accepted、running、cancel-requested 和 runtime-active。
- 前端在 Direct Agent icon 外使用轻量 CSS 旋转环；提交/停止返回的 canonical lifecycle snapshot 与 live lifecycle 事件同步更新 workspace、置顶两份 task 行，终态后恢复静态 Agent icon。
- 回归要求：Rust 单测固化 task root prompt activity 与 runtime fallback，Web 单测固化 lifecycle-to-sidebar 映射和 Direct-only 显示条件；通过 Web build/test、Rust 定向测试并 deep link 启动前端验证侧栏视觉。

---

## 2026-07-24：新会话搜索索引生命周期收敛

- 根因修复：侧栏继续以文件系统为权威事实源，SQLite 仍为派生搜索索引；task 创建和元数据更新统一由 `App` 核心生命周期刷新索引，不再由任务工作台或会话 UI 各自补调用。
- 跨 workspace 身份：task ID 只在项目内递增，SQLite schema v2 改用 `task_path` 作为主键；迁移保留现有索引行但不扫描旧任务，避免不同项目的 `task-001` 相互覆盖，删除也只清理目标路径。
- workspace 路由：项目 ID 统一复用 `GoldBandPaths::project_id`，Windows 对历史 drive letter 大小写差异兼容匹配；搜索命中后使用状态中已有的规范 project ID 组装路由，避免索引有结果但 workspace 解析失败后被过滤。
- 搜索 workspace 范围：会话搜索只覆盖 `conversationWorkspaces` 中显式存在的侧边栏工作空间，不再额外注入 `DesktopContext.repo_root`；不包含已移除或未注册的历史 workspace。允许的 task 目录在 SQLite FTS 排序与 `LIMIT` 之前过滤，避免范围外命中挤占可见结果。
- 中英文子串搜索：SQLite task FTS 升级为内置 trigram tokenizer；3 字符以上关键词支持标题、描述、需求正文任意位置匹配，1～2 字符关键词在 sidebar workspace 范围内使用字面包含匹配，修复“你好可命中但随便无法命中随便用askUserQuestion”的分词缺陷。用户输入统一按普通文本转义，多关键词使用 AND 语义，标题命中优先排序。
- 命中上下文展示：搜索接口新增 `matchPreview`，从真正命中的标题、描述或完整需求正文中截取上下文；短内容完整展示，只有长文本才在关键词前最多保留 10 个字符，避免短内容被误截断并保证关键词在单行内可见。关键词使用无底色、高对比 `foreground` 文字和轻量下划线高亮，兼容亮色与深色主题。
- 新数据范围：本次不扫描、不重建既有 `tasks` 索引缺口；修复发布后新建的会话，以及之后更新标题/描述的 task，可按标题、描述和 requirement 搜索。
- 可导航结果：会话搜索根据索引中的 `task_path` 解析 workspace，并从文件事实源补齐最新 Run；只返回能够形成 `projectId/taskId/runId` 路由的结果，点击后直接打开最近 Run。
- 错误语义：搜索索引不可用或查询失败返回结构化错误码，前端展示搜索失败，不再伪装成“没有匹配结果”。
- 回归固化：Rust 测试覆盖“创建 task 即可搜索、元数据更新刷新索引”、“搜索结果包含最新 Run”、“侧边栏 workspace 范围在 `LIMIT` 之前生效”和“随便/askUser/你好等中英文子串、短查询及命中摘要”；Web 测试覆盖 Tauri 搜索接口参数、搜索结果路由映射与关键词字面高亮，并要求桌面端完成“新建会话 → 搜索 → 查看命中上下文 → 打开”验证。

---

## 2026-07-24：会话页头身份信息收敛

- Direct 运行标题栏移除重复的 Agent、model、permission mode，仅保留目录按钮；Agent 身份统一由共享 ACP 会话信息栏承担。
- `AcpSessionVm` 增加由后端 provider 注册信息派生的 `adapterIconKey`，前端不通过展示名称猜测图标；未知 provider 使用通用 Agent 图标。
- 共享会话信息栏展示 Agent icon + 名称，移除会在会话中途变化的权限模式；session ID 支持点击复制，并通过自动消失的 Tooltip 提示复制成功。
- 回归要求覆盖 Direct 页头不再渲染旧配置元数据、共享 ACP 页头图标/权限隐藏/复制入口，以及 Web build 与 Direct deep-link 实际交互。
- Direct 在 session 就绪后不再渲染独立运行标题栏，而由 ACP 会话头组合标题、Agent/session 身份、原始帧与目录操作为单行；左侧身份组按自然宽度紧邻排列，Direct 标题不为透明编辑图标预留宽度，右侧操作组独立贴右，session 启动阶段仍保留运行标题占位，避免页头闪失。
- 会话标题编辑提示从 HTML `title` 切换到 shadcn Tooltip，统一 Direct、Workflow、AUTO 的主题样式与键盘可访问行为，不再出现 Windows 原生提示框。

---

## 2026-07-24：ACP 追问模型“不指定”语义修复

- Direct 发起会话的 Gold Band 合成模型选项由“默认模型”改名为“不指定”，英文为 `Unspecified`；提交仍使用空模型配置，不向 ACP 发送 Agent 模型 ID。
- attempt ACP session metadata 新增 `modelOverride`，与 Agent 返回的 `models.currentModelId / configOptions.currentValue` 分离。首次未指定模型时 override 为空，即使 Agent 报告 `currentModelId = default`，后续追问也不得把该值显式回传。
- 会话详情在 override 为空时展示“不指定”和 Agent 返回的完整模型目录；选择任意 Agent 模型后写入 override，并从该 session 的下拉列表中移除“不指定”。Agent 的 `default` 作为普通不透明模型 ID 原样保留。
- runtime continue、AI-DYNAMIC inner continue 和 ACP same-session prompt 统一只读取 `modelOverride`；具体模型继续通过 `session/set_config_option(model)` 应用，未指定则不设置模型并继承 Agent 环境配置。
- 回归覆盖 Agent `currentModelId = default` 但 Gold Band 未指定时续聊得到 `None`、用户明确选择 Agent `default` 时续聊得到 `Some("default")`、前端配置视图保持“不指定”和 Agent current model 分离，以及 Web build。

---

## 2026-07-27：ACP 权限模式“不指定”语义统一

- Direct、AUTO 与工作流编辑器中的可空权限模式统一将“默认 / 不设置”改名为“不指定”，英文统一为 `Unspecified`；会话创建前仍允许清回空配置。
- attempt ACP session metadata 新增 `permissionModeOverride`，与 Agent 返回的 `modes.currentModeId / configOptions.currentValue` 分离。首次未指定权限模式时 override 为空，即使 Agent 报告当前 mode，后续追问也不得把该值反推成 Gold Band 显式选择。
- 会话详情在权限 override 为空时展示“不指定”和 Agent 返回的完整权限模式目录；选择任意 Agent mode 后写入显式 override，并从该 session 的下拉列表中移除“不指定”，但仍允许在具体 mode 之间切换。
- runtime continue、AI-DYNAMIC inner continue 和 ACP same-session prompt 统一只读取 `permissionModeOverride`；未指定则不调用权限配置 API，继续继承 Agent 环境配置。模型与权限的 override/current 数据结构、显示和追问语义保持一致。
- 回归覆盖 Agent `currentModeId = default` 但 Gold Band 未指定时续聊得到 `None`、用户明确选择 Agent `default` 时续聊得到 `Some("default")`、前端配置视图保持“不指定”和 Agent current mode 分离，以及 Rust/Web 测试、Web build 和 Direct deep-link 实际验证。

---

## 2026-07-28：原始帧默认倒序与排序切换

- `AcpRawFrameQueryInput` 增加类型化 `asc / desc` 排序参数，后端默认 `desc`，以 append-only JSONL 行号作为稳定记录时序完成跨页排序；同一时间戳下不依赖不稳定的文本比较。
- Raw frames 筛选区复用 shadcn/ui `Select` 增加“最新优先 / 最早优先”，切换顺序、搜索或过滤后回到第 0 页；第一页、上一页和下一页文案按当前顺序表达实际时间方向。
- 破坏式替换旧的“最新页内升序”行为，不保留前端当前页反转或旧 `latest` 字符串兼容路径。
- Rust 接口层回归覆盖默认倒序、升序第二页、分页边界与返回排序枚举；Web build 和桌面端原始帧 deep link 验证控件默认值、切换结果及分页文案。

---

## 2026-07-24：会话工作空间状态与安全移除修复

- 根因修复：会话工作空间身份此前同时存在持久化 `conversationWorkspaces`、大小写不一致的 `projectId` key 和隐式 `DesktopContext.repo_root` 三条来源，导致 Direct 首轮可运行但追问按精确 key 报 `workspace.not-found`，移除时也可能删不中并重排相邻项。本次收敛为 `conversationWorkspaces` 单一列表来源，保留 workspace-scoped `App.paths.repo_root` 作为执行上下文，不再把桌面启动 workspace 当作会话成员。
- 状态迁移：新增 `stateSchemaVersion=1`，启动时一次性重新生成规范 `projectId`、按规范化路径去重，并迁移最后活跃工作空间、运行模式和置顶。规范 key 的运行模式覆盖历史大小写 key，确保用户已选择的 Direct Agent/model/permission 不被旧 Workflow 配置覆盖；迁移写入继续使用原子文件替换。版本命中后直接返回，二次调用也不改变 JSON。
- 统一解析：首轮创建、Direct completed-run follow-up、重跑、历史查看、权限/停止命令、附件、运行模式和置顶统一使用共享 resolver；Windows 历史 drive-letter 大小写可解析到状态中规范 ID，VM 与事件也继续使用规范 ID。
- 删除语义：后端先解析目标工作空间，再关闭其 ACP 连接并删除持久化列表项，同时清理关联 pins/run modes/last；未知目标返回结构化错误，任何 task/run/session 和工作空间文件都不删除。
- 删除交互：侧栏移除按钮先打开 shadcn/ui 确认框，展示工作空间名称并明确磁盘文件、历史会话保留；请求 pending 时禁止重复提交、取消和关闭，成功返回前不更新列表。删除当前会话所属工作空间后返回会话主页并选择后端 fallback。
- 回归固化：Rust 覆盖用户原始大小写冲突状态、规范 Direct 配置优先、迁移仅一次、显式 sidebar/search 范围、大小写 resolver 和关联状态清理；Web 覆盖确认门控、pending 单次提交、当前页 fallback 与最终工作空间为空。生产构建和 `/chat` 视觉验证通过。
- 编译契约修正：迁移代码使用的 `stateSchemaVersion` 固化到共享 `StateConfig`，补充非零版本 camelCase roundtrip、历史状态缺字段默认为 `0`、零值省略写回的单元测试，避免桌面 crate 与核心 crate 的状态模型再次不同步。
- Round 编号清理：删除指标功能遗留但从未接入的 `next_round_id` 目录扫描 helper；新 round 继续唯一地由当前 `RoundState.index + 1` 生成 ID，避免文件系统扫描与 runtime 状态形成双事实源。
- 全量回归修正：ACP timeline 计时测试原先把所有 fixture 事件写成相同 `seq=1`，解析进入 HashMap 后顺序不稳定，导致预期 11 秒而随机得到 1/8 秒；测试数据改为按落盘顺序生成单调递增序号，固化真实 timeline 接口契约，不修改生产计时算法。

---

## 2026-07-28：会话侧边栏相对时间边界修正

- 根因修复：原侧边栏以“周数小于 4”切换月份、以“月数小于 12”切换年份，但月份按 30 天、年份按 365 天取整，导致 28–29 天显示 `0mo`，360–364 天显示 `0y`。
- 领域收敛：相对时间格式化从 React 组件下沉到共享 `datetime` 模块，任务行与 run 行使用同一接口；继续保持侧边栏既有 `m/h/d/w/mo/y` 紧凑展示，不引入改变文案形态的第三方格式化依赖。
- 连续区间：不足 1 分钟显示“刚刚”，1–59 分钟显示分钟，1–23 小时显示小时，1–6 天显示天，7–29 天显示周，30–364 天显示月，365 天起显示年。
- 回归固化：前端纯函数测试覆盖所有单位切换边界、Unix 秒时间戳、未来时间与非法输入；生产构建和侧边栏实际展示验证通过后完成验收。

---

## 2026-07-29：用户反馈入口按渠道收口

- 仅 `wb` 渠道在顶栏显示「帮助」按钮；复用启动信息中的 `appInfo.channel` 贯穿 Shell 到 AppTitleBar，其他渠道及启动信息未就绪时不渲染入口。
- Web 回归测试分别固化 `wb` 可见与 `default` 不可见，避免后续渠道配置与 UI 能力再次脱节。

---

## 2026-07-29：ACP Elicitation 多行题干与跨版本结构兼容

- 根因修复：ElicitationCard 不再把 `params.message` 按换行和步骤下标切题；单题的上下文与实际问题整体展示，多题使用字段 description，通用 provider message 可隐藏。
- 协议边界：Rust 使用官方 `agent-client-protocol-schema 1.6.0` 的 `CreateElicitationRequest` 反序列化并持久化完整请求，timeline 保留 mode、scope、session/tool identity、schema 与 `_meta`。
- 版本兼容：按 schema shape 支持 Claude Agent ACP 0.44 全局 `customAnswer`、0.45.1 `question_n_custom` 和当前 `_askUserQuestionCustomAnswer` 元数据，不要求用户机器上的旧 Agent 同步升级。
- 展示能力：选项 description 与 Claude preview 元数据保持结构化渲染；普通文本字段不再被猜测为首题自定义答案。
- 回归固化：Rust 覆盖生产 0.44 fixture、pending roundtrip 和完整 timeline request；Web 覆盖多行题干、三类自定义答案、选项元数据及刷新恢复，并要求生产构建和 ACP 会话实际验证通过。

---

## 2026-07-30：PR #81 合并修复与反馈安全边界收敛

- 合并策略：保留单一 PR 与原分支，合并最新 main 后同时保留反馈渠道和 main 的 avatar、ACP elicitation、terminal failure 等能力，不拆分提交组。
- 反馈信任边界：破坏式删除 `sessionWorkspace` / `screenshotPaths` command 契约，改为 `projectId + taskId` 后端解析和截图 File bytes；task id、canonical root、逐文件路径与 symlink 规则统一校验。task id 的路径分隔符校验显式覆盖 `/` 与 `\\`，不依赖 Windows/Linux 的 `Path` 解析差异。
- 工作空间状态清理：移除 workspace 时以请求 ID、持久化 ID、路径重算 ID 组成身份别名集合，统一删除 run mode、pin 与 last workspace 引用，固化跨平台大小写差异下的回归测试。
- 资源生命周期：使用 image/walkdir/tempfile/zip/ReaderStream；截图验证后统一重编码 PNG，任务 ZIP 写临时文件并流式上传；统一限制描述、截图、归档未压缩/压缩/文件数、日志和总请求大小。
- 渠道能力：`feedbackEnabled` 从 channel JSON 编译到 `AppInfoVm`，前端只透传 boolean，后端二次门控；不再硬编码 `channel === wb`。
- 错误协议：补齐 disabled、session-not-found、attachment-invalid、payload-too-large 等结构化错误码；网络原始错误只写 metrics.log。
- MCP 范围收口：transport、Streamable HTTP 和 per-Agent 兼容性由独立 MCP 方案统一维护；本次删除 provider 层按 provider ID 硬编码 transport、预过滤 server 和 attempt warning 的重复实现，避免与 MCP 管理域形成双重事实源。
- 配置规范化：stale Agent config option 使用纯函数清理，validate 不再 mutation 输入；Direct/AUTO 提交和能力刷新使用规范化结果。
- 回归要求：Rust workspace、桌面 crate、Web 全量测试、生产构建、default/wb 渠道编译与 wb UI 实际验证全部通过后才允许推送原 PR 分支。

---

## 2026-07-30：Streamable HTTP MCP 协议与 session 生命周期修复

- 根因修复：废弃“读取完整 HTTP body 后取第一条 `data:`”的错误模型；Streamable HTTP SSE 改为按 event 增量解析，并按 JSON-RPC request id 等待对应 response，允许服务端在此前发送 request、notification、keepalive 或其他 response。
- SSE framing：多条 `data:` 按标准使用换行拼接，comment 不产生消息；目标 response 到达后立即返回，不依赖服务端关闭 SSE 连接。
- session 状态：`Mcp-Session-Id` 与协商后的 `protocolVersion` 统一由客户端管理；后续 notification、tools/list 与 DELETE 均携带协商版本和 session header。
- session 恢复：携带 session 的请求收到 `404` 后，不单独重放失败请求，而是清除旧状态并完整重走 initialize → notifications/initialized → tools/list；连续失效则停止重试并返回错误。
- 资源释放：健康检查与工具发现属于短生命周期操作，完成或失败后均 best-effort 发送 HTTP DELETE；`404/405` 视为已释放或服务端不支持主动释放。
- HTTP 方法安全：禁用自动重定向，避免 301/302 将 MCP POST 降级成 GET；要求配置最终 endpoint URL。
- UI 修复：Agent 兼容性状态的 Tooltip 使用非 disabled 包装触发器，支持/不支持状态仍可 hover 查看说明。
- 回归固化：Rust 单元测试覆盖多行 SSE、前置 notification/错误 id、目标 response 到达但连接仍保持、session 404 后重新握手、协商版本透传和最终 DELETE。
