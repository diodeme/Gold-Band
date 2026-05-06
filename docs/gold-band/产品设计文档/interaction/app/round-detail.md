# 任务编排：Round 详情页

## 1. 一句话定义
Round 详情页用于查看某个 run 中某一轮 round 的实际执行图、全局信息流，以及日志、会话、artifact、attachment 的详细内容。

---

## 2. 页面入口
进入方式：
- 在任务工作流页点击某个 round 行
- 从失败、可恢复、正在运行状态直接进入对应 round

页面面包屑：

```text
任务列表 > 任务01 > 工作流列表 > run01 > round01
```

---

`run01` 是执行上下文段，不打开独立 run 详情页；点击“工作流列表”返回任务工作流页。

## 3. 页面结构

Round 详情页采用两块主工作区 + 按需详情抽屉：

```text
┌──────────────────────────────────────────────┐
│ 上：实际工作图                                │
│ Actual Round Graph                           │
├──────────────────────────────────────────────┤
│ 下：全局信息流                                │
│ Global Information Stream                    │
└──────────────────────────────────────────────┘

点击节点 / 信息流条目 / 打开详情 -> 右侧 Detail Sheet 按需滑出
```

推荐比例：
- 实际工作图约占主体高度 45%-55%。
- 全局信息流约占主体高度 45%-55%。
- 详情信息默认不占据固定列宽，需要时以右侧 Sheet 抽屉展示。

两个主工作区均可滚动；上下分栏未来可调整大小。详情抽屉覆盖在右侧，不挤压主工作区。

---

## 4. 左上：实际工作图

### 4.1 定义
实际工作图展示当前 round 中真实发生的节点执行路径。

它不同于任务工作流页顶部的原始 workflow 图：
- 原始 workflow 图表达设计全貌。
- 实际工作图表达本 round 实际执行过、正在执行或等待执行的路径。

### 4.2 节点展示
节点卡片展示：
- node id
- node type
- status
- outcome
- latest attempt
- artifact 数量
- attachment 数量
- 当前是否运行中

### 4.3 节点高亮规则
以下节点需要视觉强调：
- 当前运行节点
- 失败节点
- paused / blocked 节点
- 有 artifacts 的节点
- 有 attachments 的节点
- 当前选中节点

其中 artifacts / attachments 可使用独立徽标，例如：

```text
A3  表示 3 个 artifacts
P2  表示 2 个 attachments
```

### 4.4 节点交互
- 单击节点：选中节点，左下信息流追加节点相关 artifacts / attachments；如果详情抽屉已打开或已固定，抽屉内容随 selection 更新。
- 双击节点：右侧详情抽屉打开节点摘要。
- 右键节点：打开上下文菜单。

节点右键菜单建议：
- 查看节点详情
- 查看会话
- 复制 node id
- 从该节点重试

---

## 5. 左下：全局信息流

### 5.1 选中 round 时
如果当前选中对象是 round，左下采用分 tab 信息架构：
- `上下文`：当前 task 的 requirement 摘要、当前 round 的可读状态摘要，避免直接把完整 `round.json` 当主内容展示。
- `运行记录`：run / round 事件、progress events、runtime log 摘要，只保留与时间线排障相关的内容。
- `产物` / `附件`：仅在选中 node 且存在对应内容时展示。

内容顺序建议：

```text
Context: Requirement -> Round Summary
Activity: Events -> Progress Events -> Runtime Log
```

### 5.2 选中 node 时
如果当前选中对象是 node，左下在 round 信息基础上追加：
- node 可读状态摘要
- node attempts
- node artifacts
- node attachments
- node 相关进度事件过滤结果

内容顺序建议：

```text
Context: Requirement -> Round Summary -> Selected Node
Activity: Events filtered by node -> Progress Events
Artifacts
Attachments
```

### 5.3 信息流交互
- 点击 requirement 或日志项：右侧详情抽屉打开对应详情；如果当前处于 node 上下文，selection 需要保留 node id，不能因为打开全局详情而退回 round 上下文。
- 点击 event：右侧详情抽屉打开 event JSON / 格式化说明；如果 event 没有自己的 node id，则沿用当前 node 上下文。
- 点击 artifact：右侧详情抽屉打开 artifact 内容。
- 点击 attachment：右侧详情抽屉打开 attachment 内容。
- 点击 artifact / attachment / worker-ref 后，左下仍保留其所属 node 的信息流，避免用户丢失节点上下文。

---

## 6. 右侧：详情抽屉

### 6.1 定义
右侧详情抽屉是按需查看区，不承担主导航，也不默认占据工作台固定宽度。

它用于展示用户从实际工作图或全局信息流中选择的具体对象。

可展示对象包括：
- 日志详情
- event 详情
- node 摘要
- provider 会话引用
- artifact 内容
- attachment 内容
- validation 详情

### 6.2 默认状态
进入 round 详情页时，详情抽屉默认关闭；用户点击“打开详情”时展示当前 selection 的详情。当前 selection 为 round 时展示 round summary：
- round id
- run id
- status
- outcome
- trigger
- repairLoopsUsed
- startedAt
- 当前节点
- 最近错误摘要

抽屉规则：
- 使用 shadcn/ui Sheet 右侧滑出。
- 非模态、无遮罩，不阻塞用户继续操作工作图和信息流。
- 支持固定详情：固定后点击图节点或信息流条目不会关闭抽屉，只切换内容；同时抽屉从覆盖式 Sheet 切换为右侧占位面板，主工作区自动收窄让位。
- 固定态面板不继续复用 Sheet Portal / Dialog Title 结构，避免非模态 Sheet 卸载过程残留 portal、focus guard 或全屏遮罩状态导致主界面变黑；固定后主工作区应自适应收窄，不出现中缝滚动条或横向滚动条。
- 关闭按钮、Escape 或未固定时点击非交互空白可收回抽屉。

### 6.3 查看日志
点击左下日志项后，详情抽屉展示：
- 日志时间
- 来源
- 级别
- 内容
- 关联 run / round / node / attempt

### 6.4 查看会话
右键节点选择“查看会话”后，详情抽屉展示：
- provider
- worker ref
- attempt id
- 会话状态
- 可打开原始 provider 会话的操作

Gold Band 默认只查看和 handoff，不在详情抽屉直接做聊天式接管。

### 6.5 查看 artifact / attachment
点击 artifact 或 attachment 后，详情抽屉展示：
- 名称
- 类型
- 来源 node
- 来源 attempt
- 更新时间
- validation 状态
- 内容预览

内容预览规则：
- JSON：格式化树或 pretty print
- Markdown：阅读视图
- 文本：plain text
- 图片：图片预览
- 不支持的二进制：展示 metadata 与打开文件位置

---

## 7. 返回与选择规则
- 点击面包屑返回上级页面。
- Esc 优先关闭右键菜单或未固定的详情抽屉。
- 详情抽屉固定时，Esc 不应破坏固定状态；用户可通过关闭按钮显式收回。
- 没有浮层时，Esc 可从具体对象详情返回 round summary。
- 再次 Esc 可清空节点选择，回到 round 选中状态。
- 不通过命令输入返回。

---

## 8. Tauri 2.x MVP 对应实现

MVP 中 Round 详情页由 Tauri command `get_round_detail` 提供 view model，前端页面位于 `web/src/pages/RoundDetailPage.tsx`。

当前实现规则：
- 左上实际工作图来自当前 round 中真实落盘的 node/attempt canonical state，并以真实节点-边画布展示；节点为 UML 风格卡片，边以箭头和 label 表达本轮路径关系。
- 实际工作图支持缩放、平移、节点选中、双击打开节点摘要，以及右键节点菜单；右键菜单保留查看节点详情、查看会话、复制 node id、从该节点重试等入口。
- 页面布局对齐桌面工作台：顶部全局面包屑由应用壳统一提供，页面 header 展示 round id、trigger、repairLoopsUsed、currentNode 与直接操作；主体为上方实际工作图、下方全局信息流，详情以右侧 Sheet 抽屉按需展示。
- 左下信息流默认展示 requirement、round summary、run events；选中 node 后追加 node、artifact、attachment 和 progress events。
- 右侧详情抽屉展示当前选择对象，默认关闭；点击“打开详情”、双击节点、右键查看节点详情/会话或点击信息流条目时打开。
- requirement、round summary、event、log、artifact、attachment、worker-ref 都可进入详情抽屉查看完整内容；artifact 在 UI selection 中使用逻辑名（如 `verify-result`），落盘文件仍为 `verify-result.json`，后端读取兼容两种形式。
- 选择 artifact / attachment / worker-ref 时通过独立 Tauri command 或 round selection 加载内容。
- 前端页面状态保持 camelCase，调用 `get_round_detail` 时将嵌套 `selection` 字段转换为 Rust `RoundSelectionInput` 所需的 snake_case，避免节点、artifact、attachment、worker-ref 选择反序列化失败。
- status/outcome 只来自 canonical state，日志和 raw stream 仅作为观测内容；运行态轮询只依据结构化 run/round/node 状态，不扫描详情文本或历史 events。
- 2026-05-03 起页面使用 Tailwind CSS v4 + shadcn/ui Card、Tabs、Button、Badge、Dropdown Menu、Scroll Area 等现成组件重构；左上实际工作图、左下信息流、右侧 Detail Viewer 三栏工作台和 selection 映射保持不变。
- 2026-05-06 起右侧 Detail Viewer 从常驻固定列改为 shadcn/ui Sheet 详情抽屉；主工作区默认由实际工作图和信息流占满，抽屉支持非模态查看、固定、关闭和随 selection 切换内容。
- 2026-05-05 起左上实际工作图优先来自 `round.json.trace`，只展示该 round 真实进入过的 node/attempt 序列；旧数据没有 trace 时按 node state 的 startedAt/attemptId 推断 fallback 路径，不再把 workflow 全景边按出现节点集合直接过滤后展示。
- 2026-05-05 起实际工作图与任务工作流页 GraphView 使用一致的节点卡片、边、背景和缩放控件样式；当前节点、有 artifacts 的节点、有 attachments 的节点和选中节点必须有独立高亮。实际工作图位于 Round 工作台左上区域时应限制 fitView 最大缩放，避免少量节点被放大成主视觉，图卡高度应与下方信息流形成均衡比例。
- 2026-05-05 起左下区域改为并排 Tabs：Requirement 与 Log 永远存在；选中有 artifacts/attachments 的节点后动态增加 Artifact / Attachment Tabs。点击日志、节点会话、artifact、attachment 只更新右侧详情抽屉，左下保持当前 round/node 上下文，不再采用“round 替换成 node”的模式。
- 2026-05-05 验收修正：Round 详情工作台在小窗口下必须允许主体滚动；实际工作图和左下信息流各自保留最小可读高度，不能被父级 `overflow-hidden` 裁切成只显示一部分。顶部 header 的 run/trigger 文案必须保持单行截断，不允许被指标区挤成竖排。
- 2026-05-05 验收修正：左下信息流的 Tabs 与上下文说明必须使用紧凑单行布局，日志项使用低内边距高信息密度卡片，优先把垂直空间留给真实日志内容。

---

## 9. 一句话总结

> Round 详情页上方看“这一轮实际怎么跑”，下方看“这一轮发生了什么”，右侧抽屉按需看“我点中的对象具体是什么”。
