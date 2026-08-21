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

作者态画布编辑器：interview -> plan -> dev -> review -> test -> accept -> cleanup
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
- success / failure 分支

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
- control 规则条展示 `max_attempts` 与 `max_rounds`；未配置时显示“不限制”。
- 有效状态显示查看；未创建状态显示新建工作流；无效或校验失败状态显示修复。
- 工作流校验错误在桌面 VM 中以 `code + params` 结构返回，前端按 i18n 渲染可见文案；后端不返回直接展示给用户的本地化句子，也不要求前端解析后端英文错误字符串。
- 新建 / 修改 / 修复模式进入作者态画布编辑器，基于 `@xyflow/react` 支持新增节点、连接边、选择节点/边并在右侧 Inspector 配置；节点坐标不写入 workflow DSL，由系统根据节点和边自动排布为规整的从左到右结构。拓扑布局只依赖入口、节点身份和边关系，Inspector 中的 Agent、goal、模型、角色、权限与工作流控制变化不得重新执行整图布局；Agent icon 等画布展示信息只刷新轻量展示投影。
- 画布左上角提供浮动结构工具条：单个 `+` icon 使用 shadcn/ui `DropdownMenu` 展开 Agent 节点、可用时的 AI-DYNAMIC 节点、结束节点和 New Round 节点；统一删除操作支持当前选中的真实节点、终点节点或边，并按选择对象显示“删除节点”或“删除边”，边 Inspector 不再提供重复删除入口。所有真实节点提供 success 语义化出线把手，单出口在节点右侧垂直居中；开启 AI 输出验证或人工 check 的 worker 节点额外提供 failure 把手，此时 success / failure 在节点右侧上下对称。扩大连线命中半径并支持点击后再点目标完成连接。选中真实节点时使用 React Flow `NodeToolbar` 提供与节点能力一致的快速新增后继与删除操作，终点节点不显示快捷操作栏。
- 工作流编辑器通过 `allowAiDynamic` 能力开关控制是否允许新增 `AI-DYNAMIC` 节点；开启后 `+` 下拉菜单显示本地化的 AI 动态节点选项，关闭时不显示该选项。
- `AI-DYNAMIC` 节点 Inspector 默认展示节点 ID，并提供两个默认收起的编辑块：基础信息、Fan-out Agent。基础信息包含 allowed workflows 与动态控制限制；agent 块只配置 provider，角色和目标由 runtime 内置 prompt 提供。
- allowed workflows 使用可搜索多选下拉栏，分为“可选择的工作流”和“不可选择的工作流”。不可选择项禁用并展示原因，例如 `workflow.id` 重复、`workflow.id` 为空、包含 AI-DYNAMIC 但未允许嵌套；默认工作流不做重复 ID 豁免。触发器内以标签展示已选 workflow 名称与 DSL `workflow.id`，标签可直接删除。`allowedWorkflows.workflowId` 存储 workflow 定义内的 `id`，不使用模板外层 `template.id`。
- 保存 workflow 时，前端校验 AI-DYNAMIC 的控制限制必须为正整数、allowed workflow 必须存在且不重复、`allowNestedDynamic=false` 时不得选择包含 AI-DYNAMIC 的 workflow；后端保存和 run start 时会再次校验并冻结 snapshot。
- 作者态画布的 `+` 菜单直接提供“结束节点”和“New Round 节点”；添加后会在画布中出现对应虚拟终点。终点支持点击选中，并通过画布顶部统一删除操作移除；删除终点时同步删除所有指向该终点的边并纳入 Undo / Redo。画布右键菜单保留相同能力作为次要快捷方式，不作为唯一入口。
- 初始入口不提供独立选择器，由画布拓扑自动派生：真实节点中没有初始拓扑入边的节点显示“入口”标识；初始拓扑入边包含 success 主链入边和非回退的前向分支入边，不包含指回 success 主链前序节点的 failure 回退边，也不包含 `$new-round.new_round_entry`。唯一入口候选会自动写入 `workflow.entry`，多个或零个入口候选均不能保存。`$new-round.new_round_entry` 仍必须在边 Inspector 中显式选择下一轮 Round 起点。
- 作者态画布自动整形使用 success 主链拓扑顺序，不使用 `workflow.nodes` 数组追加顺序判断前后；当用户新增前置节点并连出 success 边时，该节点应自动排到原入口前方。success 主链保持紧凑直连，failure、回退和其他跨节点分支边使用障碍感知的正交路由，节点矩形及其安全间距必须作为不可穿越区域。边标签同样属于布局障碍物：分支边必须避开主链标签，分支标签优先放在能够完整容纳标签且不与节点、其他标签重叠的清晰正交线段上；作者态与运行态使用同一规则。
- Inspector 以“当前选择”组织：选中节点或边的配置始终优先展示，节点配置与边配置自身均可点击标题折叠。未选中真实节点时，直接展示“工作流控制”卡片，不再叠加“工作流设置”折叠外壳与标题栏；该卡片提供 `max_attempts` 与 `max_rounds` 两个可选正整数，留空表示不限制。选中真实节点时，同一工作流控制卡片作为节点 Inspector 内的同级业务分区展示。进入编辑态后，抽屉外层不再重复展示 Attempt / Round 摘要。
- 新增节点后画布自动聚焦到该节点，用户只维护节点、边和属性逻辑，不需要手动整理画布位置。首次打开需等待容器尺寸稳定后执行 fit，最小缩放允许到 `0.3`；用户 pan / zoom 在画布与 JSON、窄屏面板和宽屏分栏切换时保持不变。
- 编辑器宽容器使用 shadcn/ui `ResizablePanelGroup` 展示可调宽画布 / Inspector；宽屏只保留两者之间一条可拖拽分隔线，画布与 Inspector surface 不再分别绘制完整外框。容器不足时切换为“画布 / 配置面板”单面板标签，不允许把 Inspector 纵向堆到画布下方形成超长滚动页。工作流模板管理中的编辑工作区占满页面标题、模式栏和模板操作栏以下的视口剩余高度，取消工作流模式的页面底部留白，使画布与 Inspector 直接延伸到页面底部；页面本身不产生第二层纵向滚动，Inspector 标题固定，配置内容在面板内部独立滚动。`WorkflowEditor` 的高度由使用方容器控制，其他 Sheet / 工作区可以继续使用自身的高度策略。ResizeObserver 只发布单面板 / 双面板离散状态，不逐像素提交根 React state。真实节点与终点节点共用同一选中颜色、外发光和渐变语义，仅按节点形状保持不同圆角。
- 作者态支持节点 / 边键盘选择、Delete / Backspace 删除、Ctrl/Cmd+Z 撤销和 Ctrl/Cmd+Y 或 Ctrl/Cmd+Shift+Z 重做；历史只保留最近 50 个 canonical workflow 草稿，不持久化为第二套业务图模型。打开或替换 workflow、撤销、重做以及删除当前节点后，画布统一清空瞬时选择和编辑器内焦点，不得自动选择第一个节点；只有用户显式点击、创建新节点或定位校验问题时才主动选择对象。画布不显示 MiniMap；工作流导航统一使用画布平移、缩放与 fit view controls，不维护额外的图边界、显隐状态或尺寸观察器。
- 编辑器在用户停止输入后持续执行保存前校验，并在 Inspector 顶部保留可点击的问题摘要；点击问题应选择并聚焦对应节点或边，同时标记相关字段。保存时的阻断弹窗继续作为最终防线，不能成为发现问题的唯一入口。
- Inspector 高频文本与数字输入先保存在局部草稿，短暂空闲后合并写回 canonical `WorkflowDsl`；父级草稿通知与 `onChange` 同样合并发布。节点 ID 提交若与尚未发布的字段草稿重叠，必须把字段 patch 与重命名 patch 合并为一次原子更新，不得让后一次重命名覆盖 goal、model、output 等待提交字段。JSON 文本只在进入 JSON 模式时从 canonical workflow 生成，画布输入不得反复序列化完整 DSL。
- 作者态画布自动整形使用 success 主链拓扑顺序，不使用 `workflow.nodes` 数组追加顺序判断前后；当用户新增前置节点并连出 success 边时，该节点应自动排到原入口前方，failure 边仍按主链顺序作为分支/回退线处理。
- Inspector 顶部提供工作流级控制项：`max_attempts` 与 `max_rounds`，均为可选正整数；留空表示不限制。
- 新增节点后画布自动聚焦到该节点，用户只维护节点、边和属性逻辑，不需要手动整理画布位置。
- 所有内置与自定义工作流模板都必须把“工作流定义”和“本机模型绑定”作为两个权威数据域管理，并继续复用同一个 `WorkflowDsl` 类型。工作流定义保存 node id、goal、profile（中文界面显示为“角色”，英文界面显示为“Profile”）、拓扑、输出协议、结果判定与控制参数；普通 Worker 的作者态执行字段保持为空，本机模型绑定独立保存 Agent、模型、权限模式以及 Agent capability 声明的全部 session config options。内置工作流升级只能替换工作流定义，不得覆盖用户已经保存的本机模型绑定。
- `WorkflowModelBindings` 的跨端接口必须始终返回完整结构；没有任何绑定时 `bindings` 仍序列化为显式空数组 `[]`，不得因省略空集合破坏前端必填字段契约。编辑器入口同时把缺失集合的历史或异常 payload 规范化为空数组，局部坏数据不得导致工作流页白屏。
- 普通 Worker 节点 Inspector 拆为“模型配置”和“节点配置”两个业务分区。“模型配置”包含 Agent、模型、思考强度等 Agent config options、权限模式和“同步至其他节点”；“节点配置”包含 node id、goal、profile、输出协议与节点结果判定。界面使用三个同级带边界分区，顺序固定为“模型配置 → 工作流控制 → 节点配置”，不显示 `worker` 等实现类型标识。两类字段仍在同一个编辑草稿生命周期内管理，但必须分别计算变更，不能用单个 `dirty` 布尔值混合判断保存权限。
- 本机模型绑定只覆盖普通 Worker；AI-DYNAMIC 及其内部动态节点继续在运行时统一配置，不增加模型绑定槽位，不参与完成数量统计，也不进入“同步至其他节点”的范围。
- 每个普通 Worker 持有内部、不可编辑的稳定 `executionSlotId`，模型绑定按该 ID 关联而不按可编辑 node id 或名称反查。修改 node id 保留槽位和绑定；内置 Worker 使用跨版本固定常量；自定义 Worker 新建时生成 UUID；画布顶部新增与节点快捷新增后继必须复用同一 Worker 创建契约，在节点进入 Inspector 前完成槽位生成，确保首次选择 Agent 可立即建立本机模型绑定；复制 Worker 时生成新槽位并复制完整模型配置；另存整个工作流时保留槽位和配置，由新的 template ID 隔离绑定作用域。JSON 编辑中缺少槽位的既有 Worker 按相同 node id 复用当前草稿槽位，真正新增的 Worker 生成一次 UUID；用户显式写入的重复槽位不得被静默修复。Worker 槽位重复和绑定数组中的槽位重复分别返回结构化错误，迁移、保存与运行注入都必须在建立 Map 前阻断。
- Agent 来源于 Agent 管理页已配置且最近一次 doctor 成功的 Agent 卡片，前端不提供默认 Agent。新增普通 Worker 与尚未绑定本机模型配置的普通 Worker 必须由用户显式选择 Agent；模型、权限模式与 config option 允许“不指定”，表示使用 Agent 默认值，不能把“不指定”误判为节点未配置。
- 工作流创建、修改、模型绑定保存和运行前解析复用现有权威 doctor 缓存，不为每次 Run 重复诊断。绑定必须引用具体 `ManagedAgentId`；显式模型、权限和 config option ID/值必须属于最新能力目录。doctor 成功但没有返回模型或权限目录时，“不指定”仍有效。未诊断、诊断失败或缓存失效的 Agent 不出现在可选列表中；既有绑定中的失效 ID 必须原样保留并展示具体错误，不得自动清空、替换或传给 Agent 试错。
- 节点 id 输入框使用本地草稿编辑，中文输入法组合输入期间不更新 workflow DSL；失焦、Enter 或组合输入结束后再提交到节点与关联边。作者态画布普通节点直接展示原始节点 id，不把 `test` 等默认模板节点名翻译成本地化文案。
- profile 配置使用可搜索选择器，默认加载客户端内建角色与用户级 `~/.gold-band/context/profiles/` 下的所有 profile；workflow DSL 保存 profile `id`，选项展示名称、ID、scope、摘要、创建时间和更新时间，并提供入口跳转到“上下文管理 / 角色管理”。内置角色的名称、摘要和正文均按当前桌面语言返回，英文界面展示英文元数据；workflow、任务和运行快照始终只保存稳定的 `pf-builtin-*` profile ID，不保存本地化名称或摘要，因此切换语言不会破坏已有角色引用。
- ACP 权限模式下拉来自 Agent 管理页最近一次诊断缓存的 `supportedModes`；切换节点 Agent 时清空旧模型、权限模式与依赖 Agent 的 config options，本机模型绑定只允许保存当前 Agent 支持的显式权限模式，空选项统一显示为“不指定”，表示沿用 Agent / adapter 当前默认模式。
- 所有 worker 节点保存前必须绑定可见角色；如果模板中的角色 ID 已删除或不存在，选择器打开时可显示为空，点击保存时一次性弹窗报告问题，关闭弹窗后清空该节点角色并在字段处红色高亮标注原因。
- 内置角色可打开并编辑草稿，但不能直接保存覆盖，只能另存为新的普通角色；删除角色时若仍被 workflow 模板、任务 workflow 或可继续运行快照引用，首次点击删除时在确认弹窗内提示影响范围与后续修复成本，二次确认后仍允许删除；删除后被引用的 workflow 会进入需要重新选择角色的修复态，可继续运行快照也可能无法继续，需人工修复。
- worker 节点结果判定方式支持 AI 输出验证与人工 check 二选一；开启其中一种会自动关闭另一种，避免同一节点同时存在机器判定和人工判定。
- worker 节点配置支持开启人工 check；开启后，ACP 会话自然结束时不直接进入后续 edge，而是将当前 node / run / round 暂停为 `WaitingForUserInput`。
- 人工 check 节点的会话面板提供“成功”“失败”两个按钮；用户点击后把该节点结果强制写为 `success` 或 `failure`。作者态为 AI 输出验证和人工 check 节点都开放 success / failure 出口与可配置分支；未配置当前 outcome 的边时继续遵守统一控制契约，等价于隐式指向 `$end`，并按当前 outcome 完成工作流。
- 默认模板来自后端持久化的内置 workflow JSON，前端“默认模板”按钮只应用该模板，不维护独立业务默认 schema/expression，也不会在模板缺失时本地合成默认 workflow；默认模板生成顺序为先同步默认角色，再把生成出的角色 ID 写入默认节点 profile。
- 内置模板由后端统一生成并在模板 store 中按稳定 ID upsert，前端不维护本地默认 workflow 生成器。稳定 ID `default` 的展示名调整为“默认完整工作流 / Default full workflow”，拓扑保持 `interview -> plan -> dev -> review -> test -> accept -> cleanup -> $end`：interview 使用人工 check 判定结束；review/test/accept 使用 worker JSON 输出验证；accept failure 开启新 Round 并从 `dev` 开始；cleanup 不启用 AI 输出验证。默认控制配置为 `max_attempts=10`、`max_rounds=3`。
- 新增稳定 ID `default-lightweight` 的“默认轻量工作流 / Default lightweight workflow”，拓扑为 `grill -> dev-test -> accept -> $end`。`grill` 使用内置拷问角色并通过人工 check 判定结束，`dev-test` 使用内置开发测试角色在同一节点完成实现与测试，`accept` 复用现有验收角色和 JSON 输出验证。`accept.failure -> $new-round(new_round_entry=dev-test)`；默认控制配置同样为 `max_attempts=10`、`max_rounds=3`。`max_rounds` 只统计额外打开的新 Round，因此最多执行初始 Round 加 3 个新 Round；超限时使用现有结构化控制错误结束。
- 内置访谈角色（profile id `pf-builtin-interview`）用于 plan 节点前的需求澄清：通过苏格拉底式深度访谈把模糊需求转化为清晰规格，产物为 `interview-spec.md`（无 output contract、无结构化 AI 判定）；Agent 会话自然结束后进入人工 check，只有用户判定成功才沿 success edge 进入 plan，判定失败且未配置 failure edge 时工作流失败。`interview-spec.md` 的目标、约束、非目标、验收标准和技术上下文作为 plan 节点的输入依据，plan 角色 prompt 在存在前序访谈节点时优先读取该产物。访谈使用 ACP elicitation 一次只问一个问题，向用户询问代码库相关问题前必须先用节点自身文件搜索能力收集事实。
- 内置拷问角色（profile id `pf-builtin-grill`）围绕计划、决策或想法进行深入访谈，直到达成共同理解；产出 `grill-consensus.md` 共识文档。该角色既可由用户手动选择，也是默认轻量工作流的可选入口节点；在内置轻量模板中，会话自然结束后同样通过人工 check 决定是否进入 `dev-test`。
- 内置开发测试角色（profile id `pf-builtin-dev-test`）用于低风险、小范围需求，在同一次节点执行中完成根因分析、实现、单元/接口测试和必要回归，并把实现与验证证据交给验收节点。完整工作流继续使用独立开发与测试角色，不因新增组合角色改变职责边界。
- `WorkflowTemplate.isBuiltIn` 只声明模板是否由系统管理，不区分完整或轻量类型；`optionalEntryStage` 独立声明可选入口节点及其 i18n 标签键。前端不得再按 `default` ID 特判采访开关。选择默认完整工作流时显示“需求采访”，选择默认轻量工作流时显示“需求拷问”，默认均开启并按模板 ID 记忆 workspace 偏好；选择自定义模板时隐藏开关但不清空偏好。关闭后只在本次创建的 workflow 副本中移除对应入口节点并从其唯一 success 后继开始，模板本体和运行模式管理页的完整画布不变。另存出的自定义模板写为 `isBuiltIn=false` 并剥离可选入口元数据，其拓扑不再受该偏好影响。
- 创建任务 Sheet 与运行模式管理页负责模板维护：模板下拉顶部提供“新增模板”按钮进入空白画布，行内提供删除按钮，所有内置模板不可删除；内置与自定义模板的主保存按钮统一命名为“保存修改”。自定义模板通过该按钮原子保存定义与整份模型绑定，允许保存模型绑定不完整或失效的不可运行状态；内置模板仅在定义未变且全部普通 Worker 绑定有效时，才允许通过该按钮保存整份模型绑定。空白画布通过“另存为新模板”沉淀。
- 内置模板的保存权限必须通过两个规范化投影判断：定义投影排除全部模型绑定字段，绑定投影只包含各稳定执行槽位的 Agent、模型、权限和 config options。只修改绑定时，全部普通 Worker 有效才启用“保存修改”；存在任何定义改动时禁用该按钮，并同时提供“另存为新工作流”与“还原其他修改”。前者原子保存当前定义与绑定为普通自定义模板，后者恢复系统定义并保留仍属于系统 Worker 的模型配置草稿。界面不得静默丢弃任一类改动，也不得依赖字段路径黑名单或整个草稿的 `JSON.stringify` 判断保存权限。
- 内置模板未配完时，草稿只保留在当前编辑会话；切换模板或离开页面提示继续配置或放弃修改，应用重启不恢复。产品不增加首次启动向导、步骤进度或保存后自动继续运行。
- Agent 切换会联动清空模型、思考级别与权限模式，可选配置恢复“不指定”时必须回到省略字段的规范结构。用户把两类改动分别还原到各自基线后，对应脏状态应立即消失；创建任务本身由 Sheet 标题栏右侧“保存任务”提交，避免与模板保存混淆；模板保存成功提示短暂展示后自动消失，错误提示持续展示直到用户修正或手动关闭。
- “同步至其他节点”只在当前工作流内复制当前普通 Worker 的完整 Agent、模型、权限和 config options。默认只填充未配置的普通 Worker，也可显式选择覆盖已有配置；确认界面必须明确提示作用域仅为当前工作流，并显示实际配置值以及填充、覆盖、跳过数量，最终作为一份绑定快照原子保存。
- 工作流普通导出只包含可移植定义，不包含本机模型绑定和 `executionSlotId`。导入时为普通 Worker 生成新的槽位并保持全部未配置；导入旧格式时忽略旧 provider、模型、权限和 config options，不把来源机器的配置带入本机。
- 默认 review/test/accept 的 JSON 输出约束使用简化 AI 面向结构：`{"reason":"String","result":"boolean"}`；旧完整 JSON Schema 不再兼容。
- AI 输出验证由输出产物 key、简化 JSON 输出约束和成功表达式组成；新建节点不会自动填写 schema/expression，输入项旁的问号说明统一使用圆形问号 icon + 随主题变化的浅色 shadcn/ui `Tooltip` 指导用户填写，悬浮或聚焦即可出现；profile 标签旁帮助也使用同一问号入口。原本带明确语义的其他说明 icon（如 profile summary）保持原语义；schema 输出不合法时 runtime 会同 attempt 隐藏追问修复，隐藏追问最多 3 次。声明 JSON 输出的 worker 只有在最近 assistant 输出中提取到可解析 JSON 时才允许落盘 canonical artifact，不得把普通自然语言回复 fallback 成 `.json` 产物；进入隐藏修复前必须清理本轮非法 output artifact，避免修复中断后产物列表展示旧的无效产物。
- JSON 输出约束输入框不在输入过程中自动格式化；输入停止约 2 秒或失焦后再做 JSON 格式判断并写入 DSL。输入框右上角提供悬浮美化按钮，用户主动点击时才格式化当前 JSON 文本。
- 成功表达式采用受限 JSONPath-like 形式，例如 `$.result == true`、`$.result=="true"`，支持多级路径和数组下标（如 `$.xx.yy[0].zz`）；保存时校验表达式路径必须存在于 JSON 输出约束中。
- 作者态画布允许编辑过程中临时存在多条同类型出边，便于先拖拽连线再修改边类型；持久化 workflow 时校验同一来源节点的同一结果类型只能有一条出边，并校验 failure 边的来源节点必须已开启 AI 输出验证或人工 check。仅当节点同时关闭这两种结果判定方式时，才同步移除该节点既有的 failure 边；在两种判定方式之间切换时保留同一 outcome 控制语义。创建任务 Sheet 标题栏的保存任务按钮与模板保存/另存按钮都会触发创建态校验，任务详情编辑抽屉的保存工作流按钮触发任务 workflow 校验。作者态与运行态复用 `@tisoap/react-flow-smart-edge` 的障碍感知路径搜索；路由失败时回退 React Flow 原生 smooth-step，不能让边消失。
- 工作流图节点长文本默认优先展示前部内容，尾部截断；鼠标悬浮节点标题或元信息时展示完整全文。
- 工作流图边必须直接展示 success / failure 等分支 label，label 随当前语言本地化；作者态所有边统一使用同一个 routed edge renderer，作者态与运行态 label 都使用不透明主题背景并固定在连线前景层，标签跟随实际绕行路径中心，任何路径方向都不得让线条覆盖标签。边持续保持虚线流动效果，运行中边可使用更高亮的流动节奏，但动画只能由 CSS 完成，不得触发路径搜索或画布重新布局。
- 工作流图节点中的 agent icon 使用固定浅色底座，并按 SVG 内部留白做视觉缩放，保证 Claude、Codex、Cursor、Gemini、OpenCode 的图标面积接近。

---

### 5.4 作者态与运行态边界
- 模板创建 Task 时复制当时的 `WorkflowDsl` 与整份模型绑定，之后模板与 Task 互不回写；Task 作者态继续使用同一个 `WorkflowDsl` 类型，并单独持有 Task 模型绑定。
- Task 作者态把 `{ workflow, modelBindings }` 作为一个聚合原子写入 `authoring/workflow.json`；定时任务创建或编辑时把同一聚合冻结到 content snapshot。两者都不维护独立绑定文件或双写路径。
- 读取旧版 Task `authoring/workflow.json` 时，必须先在内存中完成槽位补齐、旧 Worker 本机字段抽取和 revision 规范化，再将完整新聚合一次性写回；不能因已识别为旧格式而跳过迁移。迁移后的实体必须能直接通过同一运行解析入口，重复读取保持幂等。
- 聚合保存必须以已持久化的作者态实体为 revision 基线：规范化后模型绑定 payload 真正变化时，`bindingRevision` 只递增一次；相同内容重复保存以及仅修改工作流定义时保持不变。`definitionRevision` 始终由最终 `WorkflowDsl` 重算，另存模板必须先生成最终 workflow ID，再迁移、计算 revision 和原子写入，保证接口返回值与立即重读完全一致。客户端提交的 revision 不是权威序列。
- 会话 Run 中的工作流编辑或修复入口编辑的是 Task 作者态聚合，不是当前 Run 的 executable snapshot。入口激活时按 `projectId/taskId` 读取完整 `WorkflowVm`，同时初始化定义和本机模型绑定；保存成功后使用接口返回实体更新编辑基线。当前及历史 Run 的图和继续执行仍只读取各自冻结快照。
- Task 可保存不完整或失效的模型绑定，但不能创建新 Run；运行尝试必须阻断并 deep link 到第一个问题普通 Worker，用户保存修复后自行再次发起运行，不保留启动意图或自动续跑。
- 新建 Run 前，runtime 按 `executionSlotId` 严格校验并把 Task 绑定注入普通 Worker 的 provider、model、permission 与 config options，随后把完整可执行 `WorkflowDsl` 冻结为 `runs/<run>/workflow.snapshot.json`；校验失败不得产生部分 Run。
- 创建 Run 的结构化绑定错误携带 `workflowTemplateId`、`nodeId` 和 `executionSlotId`；修复入口打开对应模板并聚焦第一个失效普通 Worker，不自动重试原启动动作。
- workflow snapshot 中的 `AI-DYNAMIC` allowed workflows 会在进入节点时冻结为 `allowed-workflow-snapshots.json`；内部 workflow invocation 只引用本次冻结快照，不读取 live 模板。
- 已存在 Run / Round 的展示和继续执行只读取运行时快照，不被后续模板、Task、Agent 能力或应用升级回写。ACP session 建立后的模型与权限 override 只属于当前 session，不反向修改 Task 或模板绑定。
- 删除仍被引用的 Managed Agent 时允许二次确认删除，并展示受影响的模板、Task 与定时任务数量；模板按用户级 store 统计一次，Task 与定时任务必须聚合全部 `conversation_workspaces` 以及当前 Workbench workspace，并按稳定 `projectId` 去重。统计属于按需文件/数据库扫描，必须在 blocking pool 执行；单个 Task authoring、定时定义或 Workflow 快照损坏时继续统计其他实体，并分别返回“无法确认的 Task / 定时任务”数量，确认框必须明确提示这些 unknown 引用，用户仍可显式确认删除。只有任务目录或数据库整体无法枚举/查询时才返回命令级错误；统计整体失败或尚未完成时删除动作保持禁用，并在弹窗原位提供重新统计入口。绑定保留失效 Agent ID。已建立的活动 session 可继续，尚未启动的 Worker、停止后恢复和未来定时触发在无法解析 Agent 时进入结构化 `error-blocked`。运行快照不复制 Agent 环境变量等敏感启动配置。

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
- running / paused Run 的操作列展示“停止”；存在当前 round 时同时展示“查看”，查看进入当前 round 详情。手动停止需要递归终止该 run 当前执行树下的所有活跃资源：当前 provider / ACP 会话、AI-DYNAMIC 内部并行节点、以及 workflow-invocation 拉起的 child run；随后将 run / round / 当前 node 与已发现的 dynamic 子状态一并收敛为 killed。
- `workflow-invocation` 在 AI-DYNAMIC 内部按复合节点处理：child workflow run 若暂停，外层 dynamic node 也暂停；继续该外层 run 时，runtime 直接委托对应 `childRunId` 从自身断点恢复，外层只观察复合节点状态，不直接暴露内部 ACP 细节。
- 关闭桌面应用与手动“停止 run”语义不同：关闭应用时，当前所有 running run 会递归请求取消活跃 provider/ACP，并把 run、AI-DYNAMIC dynamic run、内部 paused node、以及 child workflow run 统一写成 `ProcessInterrupted` 可恢复暂停；再次打开应用后，用户可继续这些 run。
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
- AI-DYNAMIC 内部控制协议产物 `dynamic-node-completion` 也作为产物展示；runtime 必须只在 provider 已返回完整且合法的 artifact 内容后落盘该文件，不允许用 0 字节占位文件、停止前最后一句普通 assistant 文本或非法 JSON 触发产物数量和弹窗入口。
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
- 工作流校验分为 authoring definition 与 executable snapshot 两层：模板、Task 和编辑器图投影只校验节点、边、可达性、输出契约等定义结构，不要求普通 Worker 在 `WorkflowDsl` 内持有 provider/model/permission/config；创建 Run 前必须先按 `executionSlotId` 注入 Task 模型绑定，再对完整 executable workflow 严格校验 provider、model、权限与 session capability。不得在绑定注入前使用 executable 校验阻断合法 authoring 数据。
- 原始 workflow 图读取 task authoring workflow，并以真实节点-边画布展示；节点为 UML 风格卡片，边以箭头、流动虚线和 label 表达 success/failure 分支。
- 原始 workflow 图在任务工作流页保持只读，不提供右键操作或节点编辑能力；用户展开后只通过缩放和平移查看全貌。
- 页面布局对齐原型：顶部使用统一 Page Header 承载面包屑、任务标题、低强调 requirement 摘要和 task 稳定指标，不展示无效 Tabs；新建 Run 操作归入运行记录 Header；工作流由指标条中的“工作流”卡片承载状态与生命周期动作，下方优先展示 run / round execution history。
- 顶部 requirement 默认展示完整 authoring 内容的单行截断预览，仅当内容超过 100 字时通过链接样式入口打开右侧完整需求抽屉。
- run / round 历史按 run 分组，最新 run 优先。
- run 行只作为分组行，点击 round 进入 round 详情页。
- workflow 设计状态与 run/round 执行状态分离显示，执行终局不从日志推断。
- 2026-05-03 起页面使用 Tailwind CSS v4 + shadcn/ui Tabs、Card、Table、Button、Badge、Scroll Area 等现成组件重构；Workflow 模块条、task 指标条、图视图和 run/round 分组历史行为不变。
- 2026-05-04 起 run / round execution history 的每个 run 分组表格使用同一套固定比例列宽，避免不同 run 卡片因内容长度不同导致 ID、Status、Outcome、Trigger、Loops、Current Node、Artifacts、Action 列错位。
- 2026-05-05 起工作流页必须展示 `workflow.json.control` 的全局控制信息，包括 `max_attempts`、`max_rounds`，并在 UI 中分别显示为最大 Attempt、最大 Round。
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
- 2026-05-11 行为修正：最新 Run 未进入终止态时禁止新建 Run；运行中 Run 的操作列提供“查看”和“停止”，手动停止会递归终止当前 provider/ACP、AI-DYNAMIC 内部活跃节点与 child workflow run，并将执行终止为 killed。
- 2026-06-02 行为修正：关闭桌面应用不再等价于手动停止；应用关闭会把所有运行中的 run 递归写成 `ProcessInterrupted` 可恢复暂停，AI-DYNAMIC 内部 `workflow-invocation` child graph 作为复合节点一起暂停并在继续时委托 child run 自身恢复。
- 2026-05-11 起 Round 详情采用破坏式更新：单击实际工作图节点直接打开右侧节点抽屉，默认展示结构化详情；会话以 `progress.events` / `raw.stream` 横向 tab 分离；顶部只保留打开日志，日志抽屉内部承载导出、分页和热日志说明；默认检索最近约 1000 条热日志，全量日志保留 30 天。
- 2026-05-09 验收修正：运行记录 Header 承载新建 Run、筛选和排序；Run/Round 列表增加 Run/Round、状态、当前进度、上下文、操作的稳定列头。展开态使用中性增强表面、缩进时间线和独立 Round 行背景加强父子层级，避免大面积白底导致页面过轻。
- 2026-05-05 起页面可见 UI 文案走桌面端 i18n，中文模式除 AI、Java、JSON、workflow.json、真实 id 和日志原文等技术词外均显示中文，英文模式均显示英文。
- 2026-05-18 起工作流编辑器的 profile 字段从自由文本改为角色选择器，按名称、ID、摘要和正文检索；选中后仅把 profile `id` 写入 workflow DSL，运行时解析内建 / 用户级 Markdown profile 并把正文注入 provider prompt。
- 2026-07-22 新增内置拷问角色（profile id `pf-builtin-grill`），最初仅用于角色选择器中的独立深度拷问场景；2026-08-13 的默认轻量工作流方案进一步将其作为可关闭的入口节点。该角色最终产出 `grill-consensus.md` 共识文档。

---

## 10. 一句话总结

> 任务工作流页顶部通过“工作流”卡片管理原始 workflow 生命周期，主区域聚焦这个 workflow 在每次 run / round 中实际跑成了什么样。
