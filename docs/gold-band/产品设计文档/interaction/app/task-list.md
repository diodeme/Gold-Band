# 任务编排：任务列表页

## 1. 一句话定义
任务列表页是“任务编排”一级功能的根页面，用于浏览所有 task、查看 requirement 摘要和当前运行状态，并进入单个任务。

---

## 2. 页面入口
进入方式：
- 启动桌面客户端后的默认页面
- 点击左侧一级菜单“任务编排”
- 在任务编排内部面包屑点击“任务列表”

---

## 3. 页面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ 面包屑：任务列表                                               │
│ 标题：任务编排                                                 │
│ 操作：新建任务 / 导入 requirements / 刷新                       │
├──────────────────────────────────────────────────────────────┤
│ 状态摘要卡：全部 / 运行中 / 可恢复 / 校验失败 / 最近完成          │
├──────────────────────────────────────────────────────────────┤
│ 任务列表                                                       │
│ task id | requirement 摘要 | 状态 | 最近 run | artifacts | 操作 │
│                                                              │
│ 单击任务行 -> 右侧 Task Preview Sheet 按需滑出                 │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. 顶部区域

### 4.1 面包屑
根页面只显示：

```text
任务列表
```

### 4.2 页面标题
标题：

```text
任务编排
```

副标题：

```text
管理 requirements、运行状态与 workflow 执行历史
```

### 4.3 页面操作
当前建议：
- 刷新
- 新建任务
- 导入 requirements

MVP 如果暂不实现新建和导入，可以保留为 disabled 或占位按钮；未实现动作应直接展示为带文案的禁用按钮，不放入含义不清的“更多”菜单。

---

## 5. 状态摘要卡
显示当前 workspace 下 task 总览。

建议卡片：
- 全部任务
- 运行中
- 可恢复
- 校验失败
- 最近完成

每张卡片包含：
- 数量
- 小面积状态强调，例如左侧色条或状态点
- 简短说明

视觉规则：
- 摘要卡使用中性卡片表面，不使用整卡大面积状态色填充。
- 数字是辅助扫描信息，不应压过任务列表主视觉。
- 多张卡片在中小桌面宽度下必须响应式换行。

点击卡片可作为列表过滤器。

---

## 6. 任务列表

### 6.1 列表字段
每行 task 展示：
- task id
- task title
- requirement 摘要
- 当前状态
- workflow 校验状态
- 最近 run
- 更新时间
- artifact 数量
- 操作按钮

示例：

```text
task-001  Tauri 桌面端重写  摘要...  Running   run-003  12:31  8 artifacts  进入任务
task-002  修复 provider 输出  摘要...  Resumable run-002  昨天   3 artifacts  继续
task-003  优化文档结构        摘要...  Failed    run-001  周一   1 artifact   查看失败
```

### 6.2 状态展示
状态应区分：
- Running
- Resumable
- Completed
- Failed
- Invalid
- Missing Workflow

Invalid / Missing 不应只显示红色标签，还需要展示原因摘要。

### 6.3 requirement 摘要
requirement 摘要来自 task authoring 内容。

展示规则：
- 列表中展示 1 行摘要。
- 过长内容截断。
- 单击任务行打开 Task Preview Sheet，抽屉内展示前 100 字以内 / 单行截断的需求预览。
- 抽屉内使用链接样式“查看完整需求”入口打开当前右侧抽屉内的完整需求视图；完整需求视图可用返回图标回到 Task Preview。

### 6.4 宽度与刷新规则
任务列表是首页主视觉，Task Preview 以按需右侧抽屉承载：
- 表格列使用固定比例布局，不用内容宽度撑破主内容区。
- 低优先级字段可缩写，例如 `Workflow Valid` 缩写为 `Workflow`、`Artifacts` 缩写为 `Assets`。
- 单元格内容超长时在本列内省略，不触发页面横向滚动。
- Task Preview Sheet 覆盖在内容区右侧，不挤压任务表格；任务表格可在自身容器内横向滚动，页面级不出现横向滚动。
- 刷新时保留当前列表和抽屉状态；用户手动点击刷新时只在刷新按钮和表格卡片顶部使用低对比度局部反馈，不淡化或重绘整页；后台自动刷新必须静默更新，不触发品牌色进度条、按钮高亮或选中行闪烁；只有首次加载才显示整页骨架屏。

---

## 7. 行为

### 7.1 单击任务
单击任务行：
- 打开右侧 Task Preview Sheet。
- 如果抽屉已打开，单击另一任务行时不关闭抽屉，直接切换抽屉内容。
- 单击抽屉外的非任务行区域、按 Escape 或点击关闭按钮时收回抽屉。
- 不立即进入深层页面，避免误触。

### 7.2 双击任务 / 点击进入任务
进入任务工作流页。

页面层级变为：

```text
任务列表 > 任务01 > 工作流
```

### 7.3 点击继续运行
如果任务存在 resumable run：
- 进入任务工作流页或最近 run 的恢复入口。
- 明确显示将恢复哪个 run / round。

### 7.4 点击查看产物
进入该任务最近 run 的 artifact 汇总视图。

---

## 8. Task Preview Sheet 布局约束

Task Preview Sheet 是任务列表的轻量详情抽屉，不承担完整任务详情页职责。

布局规则：
- Task Preview 使用 shadcn/ui Sheet 从右侧滑出，默认不自动打开。
- Sheet 使用非模态交互，不用遮罩阻塞任务列表；抽屉打开时仍可单击其他任务行切换内容。
- Sheet header 固定在滚动区外，requirement 截断预览、执行统计和操作按钮位于内部 ScrollArea。
- requirement 详情入口采用链接样式，不使用突兀的主按钮或禁用按钮；仅当预览确实发生截断时显示，点击后在当前右侧 Sheet 内切换到完整需求视图，保留返回入口回到 Task Preview。
- quote、requirement preview、run id、workflow 状态和按钮文案都必须在抽屉内换行或截断，不允许撑破容器。
- 执行统计在右侧抽屉内优先单列展示；如果改为多列，必须证明长 run id 和中英文标签不会溢出。
- 页面级不产生横向滚动；任务表格可在自身容器内横向滚动。

验收规则：
- 初始进入任务列表不显示 Task Preview Sheet。
- `document.documentElement.scrollWidth <= document.documentElement.clientWidth`。
- “执行统计”始终位于 Task Preview Sheet 内部，不贴边、不裁切、不覆盖下方操作按钮。
- 单击任务行打开抽屉，单击另一任务行直接切换内容，单击非任务区域收回抽屉。
- 列表双击进入、Tabs 筛选、排序、分页和刷新状态保持可用。

---

## 9. 空状态
当没有任务时，展示：
- 当前 workspace 尚无任务
- 新建任务
- 导入 requirements
- 查看示例任务

不显示 command 提示。

---

## 10. Tauri 2.x MVP 对应实现

MVP 中任务列表由 Tauri command `get_task_list` 提供 view model，前端页面位于 `web/src/pages/TaskListPage.tsx`。

当前实现规则：
- summary cards 来自 canonical task/run 状态派生统计。
- 列表行展示 task title/id、requirement preview、display status、latest run、artifact/attachment 数量。
- 单击任务行打开右侧 Task Preview Sheet；抽屉打开时单击另一任务行直接切换内容；双击任务行或点击“进入任务”进入任务工作流页。
- Task Preview Sheet 展示 task id、状态、requirement 截断预览、latest run、artifact/attachment 统计和进入/产物操作入口；完整 requirement 通过链接样式入口进入当前 Sheet 内的完整需求视图。
- 表格列使用固定比例并在单元格内截断，Task Preview Sheet 不挤压任务列表，避免页面级横向溢出。
- 刷新时保留当前数据，只在刷新按钮和表格卡片顶部叠加局部进度反馈；首次加载使用骨架屏。
- 新建任务和导入 requirements 暂不实现，后续再补入口；当前以显式禁用按钮展示，不放入“更多”菜单。
- 任务列表页禁用浏览器/WebView 默认右键菜单，避免用户误以为系统菜单是应用功能。
- 2026-05-03 起页面使用 Tailwind CSS v4 + shadcn/ui Card、Table、Tabs、Button、Skeleton 等现成组件重构；单击选择、双击进入、刷新保留数据和右侧 Task Preview 行为不变。
- 2026-05-05 起任务列表顶部 All / Running / Completed Tabs 是真实筛选控件；表头支持排序；表格底部提供页大小、上一页、下一页和当前范围。
- 2026-05-05 起任务表使用一个统一横向滚动容器和固定 `min-width`，按最大行宽展示各列，避免部分卡片或区域出现不一致滑动条。
- 2026-05-05 验收修正：分页开启后任务列表优先保证表格可读宽度；中小桌面宽度下右侧 Task Preview 下移为全宽卡片，summary cards 使用响应式列数，避免内容被固定右栏挤压截断。
- 2026-05-05 起 summary card 后端提供稳定 key，前端用 i18n 翻译标题；中文模式除技术词外均显示中文，英文模式均显示英文。
- 2026-05-06 起任务列表首页做视觉层级收敛：summary card 改为中性表面 + 小面积状态强调，主内容间距收紧，Task Preview 改为 header 固定、正文内部滚动的安全结构；执行统计在窄栏内单列展示，避免贴边或超出卡片。
- 2026-05-07 起 Task Preview 内 requirement 默认只展示单行 / 100 字截断预览，且仅在确实截断时显示完整需求入口；点击后当前右侧 Sheet 切换到完整需求视图，并在顶部提供返回图标回到 Task Preview。
- 2026-05-06 起 Task Preview 从固定右栏改为 shadcn/ui Sheet 右侧抽屉：初始不打开，单击任务行滑出，单击其他任务行直接切换内容，单击非任务区域、Escape 或关闭按钮收回。

---

## 11. 一句话总结

> 任务列表页负责让用户从 requirement 与运行状态出发选择 task，不展示完整 DAG；DAG 从任务工作流页开始出现。
