# 任务编排：任务工作流页

## 1. 一句话定义
任务工作流页用于展示单个 task 的原始 workflow 全貌，以及该 task 下按 run -> round 展开的执行历史。

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
│ 面包屑：任务列表 > 任务01 > 工作流                            │
│ 任务标题 / requirement 单行截断 + 完整需求详情入口                  │
├──────────────────────────────────────────────────────────────┤
│ 工作流                                           │
│ prepare -> plan -> execute -> validate -> finalize             │
├──────────────────────────────────────────────────────────────┤
│ run / round 执行列表                                           │
│ run-001                                                       │
│   round-001   success / artifacts / duration                   │
│   round-002   failure / validation failed                      │
│ run-002                                                       │
│   round-001   running / current node                           │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. 顶部任务摘要
展示当前 task 的稳定上下文：
- task id
- title
- requirement 单行截断内容，默认取完整 authoring requirement 的前 100 字以内
- 仅当预览确实发生截断时显示链接样式“查看完整需求”入口，点击从右侧打开完整需求抽屉
- workflow 校验状态
- 当前 active run
- 最近 outcome
- artifact 总数

操作：
- 新建 run

工作流页不再放置无实际切换作用的总览 / 运行记录 / 节点 / 产物 Tabs；也不在顶部展示继续运行、停止 run 或禁用态查看需求按钮。

---

## 5. 工作流

### 5.1 定义
顶部 workflow 图展示 task authoring 阶段解析出的原始 workflow。

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
- 单击节点：在页面内显示该节点的跨 run 摘要。
- 双击节点：过滤下方 run / round 列表，仅看涉及该节点的 round。
- 右键节点：显示节点级操作菜单，如复制 node id、查看历史 attempts。

---

## 6. Run / Round 执行列表

### 6.1 排列方式
下方列表按 run 分组，采用紧凑分组列表展示；Run 是一级扫描对象，Round 是展开后的明细：

```text
run-001   completed / success   当前 Round round-002
  round-001   completed / failure   当前节点 -       查看
  round-002   completed / success   当前节点 accept  查看
run-002   completed / success   当前 Round round-001
```

默认排序：
- 最新 run 在上
- run 内 round 按执行顺序展示
- 最新、运行中、暂停或可恢复 run 默认展开，但默认展开不表达选中态，不额外改变 run 背景色

### 6.2 Run 分组行
Run 分组行展示：
- run id
- status
- outcome
- 当前 round
- resumable 状态
- pauseReason（如存在）

Run 分组行操作：
- 展开 / 收起

Run 行只作为分组入口，不打开独立 run 详情页；恢复和停止 run 不在该列表内作为常驻按钮展示。

### 6.3 Round 明细行
Round 明细行展示：
- round id
- index
- status
- outcome
- 当前节点或失败节点

Round 行使用明确“查看 / Open”按钮进入 round 详情页；按钮必须稳定可见，不使用弱化箭头作为唯一入口。

页面层级变为：

```text
任务列表 > 任务01 > 工作流列表 > run01 > round01
```

---

## 7. 运行状态表达
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

## 8. Tauri 2.x MVP 对应实现

MVP 中任务工作流页由 Tauri command `get_workflow` 提供 view model，前端页面位于 `web/src/pages/WorkflowPage.tsx`。

当前实现规则：
- 原始 workflow 图读取 task authoring workflow，并以真实节点-边画布展示；节点为 UML 风格卡片，边以箭头和 label 表达 success/failure/invalid 等分支。
- 原始 workflow 图在任务工作流页保持只读，不提供右键操作或节点编辑能力；用户展开后只通过缩放和平移查看全貌。
- 页面布局对齐原型：顶部保留 Workflow 模块条与新建 Run 操作，不展示无效 Tabs；中部展示 task 稳定指标条，工作流默认折叠为按需查看入口，下方优先展示 run / round execution history。
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
- 2026-05-07 起任务工作流页顶部删除无实际作用的总览 / 运行记录 / 节点 / 产物 Tabs，删除继续运行、停止 run 和禁用态查看需求按钮；需求改为单行 / 100 字截断预览，仅在确实截断时通过链接样式入口打开右侧完整需求抽屉。
- 2026-05-07 起面包屑上级项的视觉反馈限定为瞬时 hover / focus-visible，不使用组件状态保存选中项，避免从工作流页进入 Round 详情后“工作流列表”仍被误高亮。
- 2026-05-05 起页面可见 UI 文案走桌面端 i18n，中文模式除 AI、Java、JSON、workflow.json、真实 id 和日志原文等技术词外均显示中文，英文模式均显示英文。

---

## 9. 一句话总结

> 任务工作流页上半区看“原始 workflow 设计”，下半区看“这个 workflow 在每次 run / round 中实际跑成了什么样”。
