# 任务编排：任务工作流页

## 1. 一句话定义
任务工作流页用于展示单个 task 的工作流生命周期入口，以及该 task 下按 run -> round 展开的执行历史。

---

## 2. 页面入口
进入方式：
- 从任务列表双击某个任务或点击“进入任务”
- 从 round 详情面包屑返回“工作流列表”

页面面包屑：

```text
任务列表 > 任务01 > 工作流
```

---

## 3. 页面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ 统一 Page Header：面包屑 / 任务标题 / 低强调 requirement / stats │
│ stats: Task ID / 工作流(状态 + 查看/新建/修改/修复) / 最新 Run / 结果 │
├──────────────────────────────────────────────────────────────┤
│ run / round 执行列表：筛选 / 排序 / 新建 Run                     │
│ run-001                                                       │
│   round-001   success / artifacts / duration                   │
│   round-002   failure / validation failed                      │
│ run-002                                                       │
│   round-001   running / current node                           │
└──────────────────────────────────────────────────────────────┘

右侧工作流抽屉：
作者态画布编辑器：plan -> dev -> review -> test -> accept -> cleanup
```

---

## 4. 顶部任务摘要
顶部区域使用与任务列表页、Round 详情页一致的统一 Page Header：面包屑在 Header 内第一行，主标题直接展示任务标题，不额外展示蓝色 task id eyebrow；requirement 仅作为低强调单行上下文展示，低对比 stats 位于同一 Header 的下一行。Header 右侧保留手动刷新按钮，后台每 10 秒静默刷新一次当前页面数据；新建 Run 不放在全局 Header，而放入运行记录卡片 Header，表达它是对当前 run 列表的主操作。

展示当前 task 的稳定上下文：
- task id
- title
- requirement 单行截断内容，默认取完整 authoring requirement 的前 100 字以内，只作为标题下方行内文本展示，不使用独立边框、底色或小卡片轮廓，避免长需求抢占首屏主注意力
- 仅当预览确实发生截断时在同一行显示链接样式“查看完整需求”入口，点击从右侧打开完整需求抽屉；抽屉标题右侧提供复制 icon，一键复制完整需求
- 工作流生命周期状态与对应入口动作：未创建 -> 新建工作流，有效 -> 查看，无效或校验失败 -> 修复
- 最新 run
- 与任务列表一致的任务状态标签（已完成 / 可恢复 / 失败等）

视觉规则：
- 顶部状态只作为当前 task 的上下文 stats，不作为页面级 KPI 看板。
- 顶部四张 stats 卡片保持等高，label 与 value 使用统一垂直节奏；“工作流”卡片中的状态标签和动作按钮需与其他 stats 卡片对齐。
- stats item 使用低对比背景与弱边框，不使用重卡片、重阴影或大面积色块。

操作：
- 新建 run：放在运行记录卡片 Header 右侧，不放在全局 Page Header

工作流页不再放置无实际切换作用的总览 / 运行记录 / 节点 / 产物 Tabs；也不在顶部展示继续运行、停止 run 或禁用态查看需求按钮。

---

## 5. 工作流

### 5.1 定义
工作流卡片是 task authoring workflow 的生命周期入口，主页面只展示状态与动作；完整 workflow 图进入右侧抽屉查看。

它表达的是：
- workflow 的设计结构
- 节点顺序
- 条件路径
- success / failure / invalid 分支

它不表达某一次 round 的实际执行细节。

### 5.2 节点展示
每个节点展示：
- node id
- node type
- 简短 label
- 是否有历史 artifacts
- 最近执行 outcome 摘要

### 5.3 交互
- 工作流状态卡片统一命名为“工作流”，承载查看、新建、修复等生命周期动作。
- 状态标签与动作按钮同一行展示，状态靠左，动作按钮靠右。
- 点击工作流动作从右侧打开非模态抽屉；查看模式展示 control 规则条、只读 workflow 图与 workflow JSON 预览。
- 有效状态显示查看；未创建状态显示新建工作流；无效或校验失败状态显示修复。
- 新建 / 修改 / 修复模式进入作者态画布编辑器，基于 `@xyflow/react` 支持新增节点、连接边、选择节点/边并在右侧 Inspector 配置；节点坐标不写入 workflow DSL，由系统根据节点和边自动排布为规整的从左到右结构。
- 新增节点后画布自动聚焦到该节点，用户只维护节点、边和属性逻辑，不需要手动整理画布位置。
- 节点配置包含 node id、goal、provider agent、profile（中文界面显示为“角色”，英文界面显示为“Profile”）、节点结果判定方式；agent 来源于 Agent 管理页已配置 agent 卡片。
- profile 配置使用可搜索选择器，默认加载用户级 `~/.gold-band/context/profiles/` 与当前项目级 `~/.gold-band/projects/{project-id}/context/profiles/` 下的所有 profile；workflow DSL 保存 profile `id`，选项展示名称、ID、摘要、创建时间和更新时间，并提供入口跳转到“上下文管理 / 角色管理”。
- 所有 worker/verify 节点保存前必须绑定可见角色；如果模板中的角色 ID 已删除或因项目可见性变化不可访问，选择器打开时可显示为空，点击保存时一次性弹窗报告问题，关闭弹窗后清空该节点角色并在字段处红色高亮标注原因。
- worker 节点结果判定方式支持 AI 输出验证与人工 check 二选一；开启其中一种会自动关闭另一种，避免同一节点同时存在机器判定和人工判定。
- worker 节点配置支持开启人工 check；开启后，ACP 会话自然结束时不直接进入后续 edge，而是将当前 node / run / round 暂停为 `WaitingForUserInput`。
- 人工 check 节点的会话面板提供“成功”“失败”两个按钮；用户点击后把该节点结果强制写为 `success` 或 `failure`，并继续走现有 success / failure 分支。
- 默认模板来自后端持久化的内置 workflow JSON，前端“默认模板”按钮只应用该模板，不维护独立业务默认 schema/expression；默认模板生成顺序为先同步默认角色，再把生成出的角色 ID 写入默认节点 profile。
- 默认模板为 `plan -> dev -> review -> test -> accept -> cleanup -> $end`，不再默认生成 `exec` 节点或 `exec-plan` 产物；review/test/accept 使用 worker JSON 输出验证决定 success/failure 分支，cleanup 是普通 worker 节点，不启用 AI 输出验证。
- 默认 review/test/accept 的 JSON 输出约束使用简化 AI 面向结构：`{"reason":"String","result":"boolean"}`；旧完整 JSON Schema 不再兼容。
- AI 输出验证由输出产物 key、简化 JSON 输出约束和成功表达式组成；新建节点不会自动填写 schema/expression，输入项旁提供问号说明指导用户填写。
- 成功表达式采用受限 JSONPath-like 形式，例如 `$.result == true`、`$.result=="true"`，支持多级路径和数组下标（如 `$.xx.yy[0].zz`）；保存时校验表达式路径必须存在于 JSON 输出约束中。
- 作者态画布中的 failure/invalid 回退边自动分配独立 lane 路由，避免 review/test 回 dev 的边与主成功路径或彼此重合。
- 工作流图节点长文本默认优先展示前部内容，尾部截断；鼠标悬浮节点标题或元信息时展示完整全文。

---

### 5.4 作者态与运行态边界
- 任务级工作流保存为 `tasks/<task>/authoring/workflow.json`，可在任务工作流页后续修改。
- 新建 run 时 runtime 会把当时的 authoring workflow 写入 `runs/<run>/workflow.snapshot.json`。
- 已存在 run / round 的展示和继续执行只读取运行时快照，不被后续 authoring workflow 修改回写。

## 6. Run / Round 执行列表

### 6.1 排列方式
下方列表按 run 分组，采用紧凑分组列表展示；Run 是一级扫描对象，Round 是展开后的明细。列表使用稳定列结构：Run/Round、状态、当前进度、上下文、操作，避免字段像散落文本一样横向漂移：

```text
run-001   success   当前 Round round-002
  round-001   failure   当前节点 -       查看
  round-002   success   当前节点 accept  查看
run-002   success   当前 Round round-001
```

默认排序：
- 最新 run 在上
- run 内 round 按最新在上展示
- 初始态所有 run 默认收起；运行记录采用单展开 accordion 行为，同一时间最多展开一个 run，展开新 run 时自动收起此前展开的 run，切换正序/倒序时不应把所有 run 一次性展开
- 运行记录主列表按“固定行高摘要行”阅读：collapsed run 行与 round 行不因长文本自动增高，只有用户主动展开 run 时才增加内容高度
- 运行记录卡片主体需要保留稳定最小高度，使分页器在不同分页、少量结果与空状态下都保持接近固定的垂直位置
- 客户端宽度不足以容纳完整五列表头时，运行记录不强行保持表格列宽；Run / Round 行改为纵向紧凑栅格，操作按钮仍在可见区域内，禁止产生页面级横向滚动或右侧裁切。

### 6.2 Run 分组行
Run 分组行展示：
- run id
- 单一状态标签（优先显示 outcome；无 outcome 时回退到 status，如成功 / 失败 / 已暂停 / 已停止）
- 当前 round
- 当前 node
- pauseReason（如存在）

Run 分组行规则：
- collapsed 状态下按固定高度摘要行展示
- Run/Round、状态、当前进度、上下文、操作列与 Round 明细行保持同一列节奏；Run 分组行没有直接操作时，操作列保持空白，不显示横线或其他占位符
- 当前 node、pauseReason 等长文本在主行内只显示单行截断，不换行撑高
- 展开后直接进入 round 明细列表，不额外插入重复的 run 级摘要条
- 展开态允许使用更明确的中性表面、左侧弱边界和子列表底色区分父子层级，但不得使用大面积品牌色背景造成“选中态”误解

Run 分组行操作：
- 点击整行或左侧箭头展开 / 收起
- running / paused Run 的操作列展示“停止”；存在当前 round 时同时展示“查看”，查看进入当前 round 详情，停止需要终止 provider 进程并将 run / round / 当前 node 置为 killed
- completed 等终态 Run 没有直接操作时，操作列保持空白

Run 行只作为分组入口，不打开独立 run 详情页；恢复 run 不在该列表内作为常驻按钮展示。

### 6.3 Round 明细行
Round 明细行展示：
- round id
- index
- 单一状态标签（优先显示 outcome；无 outcome 时回退到 status）
- 当前节点或失败节点

Round 行规则：
- 使用与 run 摘要行一致的紧凑固定行高节奏和列宽
- 展开区域通过缩进、左侧时间线和独立浅表面表达 Round 从属于当前 Run
- 当前节点只在行内展示单行截断摘要；需要完整上下文时进入 round 详情页

Round 行使用明确“查看 / Open”按钮进入 round 详情页；按钮必须稳定可见，不使用弱化箭头作为唯一入口。

页面层级变为：

```text
任务列表 > 任务01 > 工作流列表 > run01 > round01
```

---

## 7. Round 工作图详情、会话与日志

### 7.1 节点详情抽屉

Round 详情页的实际工作图是运行排障的主入口。用户单击节点时，右侧滑出节点抽屉，默认进入“查看详情”。项目仍处于开发阶段，本页采用破坏式更新：节点详情不再展示原始 `node.json`，下方产物/附件信息流不再作为主入口，旧 JSON 查看器路径不做灰度兼容。

节点详情抽屉结构：
- 左侧外置垂直 tab：查看详情、查看会话。
- 查看详情默认展示结构化节点信息：node id、节点说明、节点类型、sequence、status、outcome、current 标记、attempt id、startedAt、finishedAt。
- artifact 与 attachment 作为资源列表展示，不预加载完整正文。
- 点击 artifact 或 attachment 后打开二级抽屉展示完整内容；二级抽屉左上提供返回按钮，返回上一级节点详情。

右键菜单只作为低频快捷入口，保留查看详情、查看会话、查看日志、复制 node id、从该节点重跑；核心浏览路径必须通过单击节点完成。

### 7.2 会话页

“查看会话”用于查看 runtime 和 provider 的会话记录，不再混入系统排障日志。会话页内部使用横向 tab：
- `progress.events`：展示 attempt 的 runtime/provider 进度事件。
- `raw.stream`：展示 provider 原始 stdout/stderr stream envelope。

会话条目按一行一条分页展示，保留时间、类型、节点、阶段、摘要等字段；内容过长时单行截断，必要时在详情或 tooltip 中查看完整原文。

### 7.3 日志抽屉与冷热数据

顶部 Header 只保留“打开日志”，删除外层“导出日志”。打开日志后从右侧滑出独立日志抽屉，抽屉内提供导出能力。

日志页展示系统关键排障日志：
- 一条日志一行。
- 列包含时间、类型、节点、阶段、摘要。
- 支持分页。
- 默认查询当前热日志，限制最近约 1000 条，保证打开速度。
- 全量日志保留 30 天，用于导出、深度排障或扩大检索范围。

首版不引入 SQLite。现有 `events.jsonl`、`progress.events.jsonl`、`raw.stream.jsonl` 已经是一行一条，先基于 JSONL tail、结构化解析与分页实现；只有当出现跨任务全文检索、复杂筛选或大规模索引需求时，再引入 SQLite。

---

## 8. 运行状态表达
工作流页需要同时展示两层状态：

### 7.1 Workflow 设计状态
来自原始 workflow 解析结果：
- valid
- invalid
- missing

### 7.2 Run / Round 执行状态
来自 canonical state：
- running
- paused
- completed
- success
- failure
- killed

不应根据 raw stream 或日志直接推断终局状态。

---

## 9. Tauri 2.x MVP 对应实现

MVP 中任务工作流页由 Tauri command `get_workflow` 提供 view model，前端页面位于 `web/src/pages/WorkflowPage.tsx`。

当前实现规则：
- 原始 workflow 图读取 task authoring workflow，并以真实节点-边画布展示；节点为 UML 风格卡片，边以箭头和 label 表达 success/failure/invalid 等分支。
- 原始 workflow 图在任务工作流页保持只读，不提供右键操作或节点编辑能力；用户展开后只通过缩放和平移查看全貌。
- 页面布局对齐原型：顶部使用统一 Page Header 承载面包屑、任务标题、低强调 requirement 摘要和 task 稳定指标，不展示无效 Tabs；新建 Run 操作归入运行记录 Header；工作流由指标条中的“工作流”卡片承载状态与生命周期动作，下方优先展示 run / round execution history。
- 顶部 requirement 默认展示完整 authoring 内容的单行截断预览，仅当内容超过 100 字时通过链接样式入口打开右侧完整需求抽屉。
- run / round 历史按 run 分组，最新 run 优先。
- run 行只作为分组行，点击 round 进入 round 详情页。
- workflow 设计状态与 run/round 执行状态分离显示，执行终局不从日志推断。
- 2026-05-03 起页面使用 Tailwind CSS v4 + shadcn/ui Tabs、Card、Table、Button、Badge、Scroll Area 等现成组件重构；Workflow 模块条、task 指标条、图视图和 run/round 分组历史行为不变。
- 2026-05-04 起 run / round execution history 的每个 run 分组表格使用同一套固定比例列宽，避免不同 run 卡片因内容长度不同导致 ID、Status、Outcome、Trigger、Loops、Current Node、Artifacts、Action 列错位。
- 2026-05-05 起工作流页必须展示 `workflow.json.control` 的全局控制信息，包括 `max_repair_loops`、`max_acceptance_loops`、`on_acceptance_failure`，并在 UI 中分别显示为最大修复循环、最大验收循环、验收失败策略。
- 2026-05-05 验收修正：`workflow.json.control` 不再使用独立卡片展示，而是放入“工作流 / 工作流蓝图”卡片内的紧凑规则条；规则条位于画布上方，不覆盖节点与边。画布不应因节点较少而自动放大到占满整屏，需要限制 fitView 最大缩放，并保持中等高度、节点间距与阅读留白。
- 2026-05-06 起 run / round execution history 从混合表格改为紧凑分组列表，支持状态筛选、run 分组分页和 run id 排序；Run 行只保留当前 Round 和必要操作，Round 明细只保留状态、结果、当前节点与明确“查看 / Open”入口；默认展开的 run 不使用高亮背景，避免被误解为选中态。
- 2026-05-06 验收修正：运行记录不展示 Round 数、资源、触发、循环等低价值字段，避免列表重新变成数据库记录表。
- 2026-05-07 起顶部 task 摘要不再拼接“当前状态：某节点正在执行”句子；当前节点只在 Run 分组行、Round 明细行和 Round 详情中以结构化字段展示，并使用“节点类型 + 节点说明 + 原始 node id”的可读格式。
- 2026-05-07 起工作流默认折叠，仅保留标题与“展开蓝图”按钮；展开后再显示 control 规则条和只读 GraphView，避免运行记录被蓝图挤到首屏下方。
- 2026-05-08 起工作流从页面内折叠条升级为顶部“工作流”状态卡片，卡片内根据生命周期提供新建、查看、修复入口；状态标签靠左、动作按钮靠右，完整蓝图与 control 规则条改由右侧非模态抽屉承载。
- 2026-05-07 起顶部 task 指标条降级为低对比上下文 stats，避免工作流页首屏形成 KPI 卡片墙；信息结构不变。
- 2026-05-07 起任务工作流页顶部删除无实际作用的总览 / 运行记录 / 节点 / 产物 Tabs，删除继续运行、停止 run 和禁用态查看需求按钮；需求改为单行 / 100 字截断预览，仅在确实截断时通过链接样式入口打开右侧完整需求抽屉。
- 2026-05-07 起面包屑上级项的视觉反馈限定为瞬时 hover / focus-visible，不使用组件状态保存选中项，避免从工作流页进入 Round 详情后“工作流列表”仍被误高亮。
- 2026-05-08 起任务工作流页使用统一 Page Header：面包屑、任务标题、requirement 摘要和上下文 stats 同属顶部表面，蓝图与运行记录从 Header 下方开始；2026-05-09 起新建 Run 移入运行记录 Header，避免全局 Header 按钮与列表主操作脱节。
- 2026-05-08 验收修正：工作流页进一步收紧 Header、指标条和运行记录分组的纵向留白；run 内 round 改为最新在上，正序/倒序切换只改变排序，不批量重置 run 展开状态。
- 2026-05-08 验收修正：顶部 task stats 的 `Latest Run` 统一锚定最新 run，右侧结果位改为复用任务列表状态标签（已完成 / 可恢复 / 失败等），并删除独立 `产物` 卡片，避免历史 paused run 和低价值聚合统计覆盖首页已展示的任务主状态。
- 2026-05-08 验收修正：运行记录进一步收敛为固定行高摘要列表；Run 与 Round 主行不再因 `currentNode`、`pauseReason` 等长文本自动增高，长内容在主行单行截断；展开后直接进入 round 明细列表，不再插入重复的 run 摘要条。运行记录主体增加稳定最小高度，使不同分页和空状态下分页器位置保持稳定；初始态所有 run 默认收起，点击整行或左侧箭头即可展开/收起。
- 2026-05-10 验收修正：运行记录改为单展开 accordion，同一时间最多展开一个 run，避免多条 run 的 round 明细同时铺开造成页面拥挤。
- 2026-05-10 验收修正：Run 分组行操作列无可用操作时保持空白，不显示横线占位，减少无意义视觉噪音。
- 2026-05-10 行为修正：桌面端点击“新建 Run”后，Tauri command 只同步创建 run / round 初始状态并立即返回，后续 workflow 驱动在后台线程继续执行；前端刷新应能马上看到新增 run，不能因等待长时间执行导致整个应用卡死。
- 2026-05-11 行为修正：最新 Run 未进入终止态时禁止新建 Run；运行中 Run 的操作列提供“查看”和“停止”，停止会杀掉当前 provider 进程并将 workflow 执行终止为 killed。
- 2026-05-11 起 Round 详情采用破坏式更新：单击实际工作图节点直接打开右侧节点抽屉，默认展示结构化详情；会话以 `progress.events` / `raw.stream` 横向 tab 分离；顶部只保留打开日志，日志抽屉内部承载导出、分页和热日志说明；默认检索最近约 1000 条热日志，全量日志保留 30 天。
- 2026-05-09 验收修正：运行记录 Header 承载新建 Run、筛选和排序；Run/Round 列表增加 Run/Round、状态、当前进度、上下文、操作的稳定列头。展开态使用中性增强表面、缩进时间线和独立 Round 行背景加强父子层级，避免大面积白底导致页面过轻。
- 2026-05-05 起页面可见 UI 文案走桌面端 i18n，中文模式除 AI、Java、JSON、workflow.json、真实 id 和日志原文等技术词外均显示中文，英文模式均显示英文。
- 2026-05-18 起工作流编辑器的 profile 字段从自由文本改为角色选择器，按名称、ID、摘要和正文检索；选中后仅把 profile `id` 写入 workflow DSL，运行时解析项目级 / 用户级 Markdown profile 并把正文注入 provider prompt。

---

## 10. 一句话总结

> 任务工作流页顶部通过“工作流”卡片管理原始 workflow 生命周期，主区域聚焦这个 workflow 在每次 run / round 中实际跑成了什么样。
