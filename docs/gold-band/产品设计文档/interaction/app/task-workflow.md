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
│ 任务标题 / requirement 摘要 / 当前状态                         │
├──────────────────────────────────────────────────────────────┤
│ 原始 workflow 全貌图                                           │
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
- requirement 摘要
- workflow 校验状态
- 当前 active run
- 最近 outcome
- artifact 总数

操作：
- 新建 run
- 继续运行
- 停止当前 run
- 查看 requirement

危险操作如停止当前 run 必须使用明确的危险色和确认提示。

---

## 5. 原始 workflow 全貌图

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
- 最新、运行中、暂停或可恢复 run 默认展开

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
- Resume
- Stop

Run 行只作为分组与操作入口，不打开独立 run 详情页。

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
- 原始 workflow 图在任务工作流页保持只读，不提供右键操作或节点编辑能力；用户只通过缩放和平移查看全貌。
- 页面布局对齐原型：顶部保留 Workflow 模块条与页面操作，中部展示 task 稳定指标条，上半区展示原始 workflow 全貌图，下半区展示 run / round execution history。
- run / round 历史按 run 分组，最新 run 优先。
- run 行只作为分组与操作行，点击 round 进入 round 详情页。
- 停止 run 通过明确确认框触发 `kill_run`，并使用危险色按钮。
- workflow 设计状态与 run/round 执行状态分离显示，执行终局不从日志推断。
- 2026-05-03 起页面使用 Tailwind CSS v4 + shadcn/ui Tabs、Card、Table、Button、Badge、Scroll Area 等现成组件重构；Workflow 模块条、task 指标条、图视图和 run/round 分组历史行为不变。
- 2026-05-04 起 run / round execution history 的每个 run 分组表格使用同一套固定比例列宽，避免不同 run 卡片因内容长度不同导致 ID、Status、Outcome、Trigger、Loops、Current Node、Artifacts、Action 列错位。
- 2026-05-05 起工作流页必须展示 `workflow.json.control` 的全局控制信息，包括 `max_repair_loops`、`max_acceptance_loops`、`on_acceptance_failure`，并在 UI 中分别显示为最大修复循环、最大验收循环、验收失败策略。
- 2026-05-05 验收修正：`workflow.json.control` 不再使用独立卡片展示，而是放入“原始 workflow 全貌图 / 工作流蓝图”卡片内的紧凑规则条；规则条位于画布上方，不覆盖节点与边。画布不应因节点较少而自动放大到占满整屏，需要限制 fitView 最大缩放，并保持中等高度、节点间距与阅读留白。
- 2026-05-06 起 run / round execution history 从混合表格改为紧凑分组列表，支持状态筛选、run 分组分页和 run id 排序；Run 行只保留当前 Round 和必要操作，Round 明细只保留状态、结果、当前节点与明确“查看 / Open”入口。
- 2026-05-06 验收修正：运行记录不展示 Round 数、资源、触发、循环等低价值字段，避免列表重新变成数据库记录表。
- 2026-05-05 起页面可见 UI 文案走桌面端 i18n，中文模式除 AI、Java、JSON、workflow.json、真实 id 和日志原文等技术词外均显示中文，英文模式均显示英文。

---

## 9. 一句话总结

> 任务工作流页上半区看“原始 workflow 设计”，下半区看“这个 workflow 在每次 run / round 中实际跑成了什么样”。
