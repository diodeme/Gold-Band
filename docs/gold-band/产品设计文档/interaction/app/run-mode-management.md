# 运行模式管理

## 信息架构

运行模式管理是会话式 UI 新增的独立设置页面，管理新会话创建的默认配置。

## 页面定位

- 仅管理 AUTO 设置和工作流模板管理
- 页面主体顶部提供项目选择器，数据源复用新 UI 的 conversation workspace 列表；进入页面时默认选中当前会话首页/快速对话正在打开的项目，新增或删除项目后跟随该列表动态更新。
- 运行模式配置属于 workspace/project 级记忆；切换项目后，工作流模板 tab 和 AUTO 设置 tab 必须加载该项目自己的运行模式选择。目标项目尚无保存记录时，页面明确回到默认 AUTO，而不是沿用上一个项目的状态。
- 顶部分段入口按“工作流模板 / AUTO 设置”展示，和会话主页处理模式顺序一致。
- 页面不提供独立的页面级“保存”按钮；切换 `工作流模板 / AUTO 设置` tab、切换工作流模板、切换 AUTO 模板时立即应用为当前默认运行模式。运行模式持久化必须保留另一侧配置：切到 WORKFLOW 不能清空 `autoConfig`，切回 AUTO 也不能清空已选工作流模板。
- 不吞并 Agent 管理和上下文管理（各自独立菜单）
- 页面壳与 Agent 管理、上下文管理、设置页保持一致：使用全宽 `PageHeader` 承载标题/返回操作，主体区域从左侧开始铺满可用宽度，不使用居中窄容器。
- 工作流模板编辑器作为主体工作区直接铺开；外层不再额外套卡片边框，避免在深色主题下形成嵌套面板。
- 页面级原生滚动容器、弹层内部滚动区和 shadcn `ScrollArea` 均继承 Gold Band 主题滚动条 token；滚动条统一为 10px 交互轨道、约 4px 可见 thumb，不再使用系统默认灰色滚动条、局部灰色 thumb 或胖瘦不一致的 Radix 自绘滚动条。

## AUTO 设置

AUTO 模式本质上是一个只有 AI-DYNAMIC 节点的工作流。

### 配置项
- **节点 ID**：固定为 `ai-dynamic`
- **Agent 策略**：固定 Agent 或动态 Agent
- **固定 Agent**：固定策略下从 Agent 管理枚举已配置 agent，可选择该 agent 的模型；模型为空表示由 provider 默认模型或运行时 prompt 引导决定。内部 proposal 不输出 provider，runtime 会把固定 provider 注入到 worker / merge / acceptance。
- **动态 Agent**：动态策略下配置初始分发节点 Agent、初始分发节点模型、可选动态 Agent 列表、每个可选 Agent 的可选模型，以及 agent / 模型决策指南
- **允许调用的工作流**：引用工作流 DSL 内的 `workflow.id`
- **可用角色列表**：引用上下文管理中的 profile id
- **动态控制**：`maxDynamicNodes`、`maxFanout`、`maxDepth`、`maxParallel`、`maxGroupDepth`、`maxWorkflowInvocations`

### 会话级配置
- **权限模式** 不在 AUTO tab 中最终决定；会话 composer 选择 AUTO 后展示权限下拉，并作为本次会话发起 AI-DYNAMIC 工作流的最终值
- 固定 Agent 策略下，composer 展示 agent 下拉、模型下拉和该 agent 支持的权限模式；composer 中选择的 agent / 模型可以覆盖 AUTO tab 当前配置，用于快速会话
- 动态 Agent 策略下，composer 展示 Dynamic Agent 标识和通用权限模式下拉
- **全局 Goal** 在 composer 中输入，非必填；运行时追加到每个 AI-DYNAMIC 内部节点目标
- composer 提供跳转 AUTO tab 的快速入口，用户需要改模板级配置时直接进入运行模式管理

### 行为
- 每个 workspace 记忆上次选择的运行模式
- 运行模式管理页切换项目只改变当前编辑/保存的 project scope，不创建模板；保存修改、切换 tab、切换工作流模板和切换 AUTO 模板均写入当前选中的项目。
- AUTO 模式下创建 task 前生成标准 WorkflowDsl
- 生成的 workflow 走现有 validation、snapshot、runtime
- 快速会话记忆上一次会话级 AUTO 选择；AUTO tab 的当前配置可保存为模板，并可切换生效模板
- AUTO 模板只保存 AI-DYNAMIC 模板级配置，不保存会话级权限模式和全局 Goal
- AUTO 模板存储在用户目录 `~/.gold-band/context/auto-templates.json`，属于用户级跨 workspace 模板；首次读取时若后端模板为空，会把旧版 `localStorage.gold-band-auto-mode-templates` 导入到该文件并清理旧 key
- AUTO 模板下拉支持选择和删除；删除当前模板只解除模板绑定并清空模板名，不清空用户正在编辑的 AUTO 配置字段
- AUTO 与工作流模板管理复用同一个模板操作行组件；操作栏顺序统一为模板选择、保存修改、新模板名称、另存模板，“保存修改”和“另存”使用主题色按钮。
- AUTO 的“保存修改”保存当前 AUTO 配置：选中模板时更新该模板并设为当前默认 AUTO 配置；选择“不使用模板”时不创建模板，直接把当前参数持久化为默认 AUTO 配置。
- AUTO 的“另存模板”只负责创建新模板；模板名称为空或重复时仅在 AUTO tab 内展示错误，不污染工作流模板管理区域。
- AUTO 保存修改和另存提交期间按钮展示“保存中…”并禁用重复提交；反馈横幅位于模板操作行下方，成功反馈短暂展示后自动消失，错误反馈保留在 AUTO tab 内等待用户修正。
- AUTO 模板保存和另存必须给出明确反馈；模板名重复、Agent 不可用、动态策略缺少可用 Agent、无决策指南且可选动态 Agent 未选择模型、动态控制参数非法时不允许静默保存
- 动态 Agent 策略中，初始分发节点 Agent 可以独立选择模型；后续调起 bootstrap 节点时使用该模型，不复用可选动态 Agent 的模型配置
- 可选动态 Agent 的模型下拉支持清空。若 agent / 模型决策指南为空，则每个可选动态 Agent 必须选择模型，AI-DYNAMIC 内部 proposal DSL 不需要输出 `model`；若决策指南非空，则内部 proposal DSL 必须输出 `model`，但已在配置里选择模型的 Agent 仍固定使用配置模型，忽略 proposal 中对该 Agent 给出的其他模型
- AUTO 的可用角色列表只作为内部 worker proposal 的可选 profile ID 白名单；worker 不填 profile 时不注入角色内容。merge / acceptance 不接受 proposal profile，始终使用 runtime 内置 merge / acceptance prompt。
- Agent 列表展示所有已配置 Agent；未通过诊断或不支持的 Agent 置灰，不可选，并展示不可选原因
- 允许调用的工作流按 DSL `workflow.id` 去重判断；重复或空 ID 的工作流直接展示在允许调用工作流列表下方，标签保留名称，感叹号 icon tooltip 展示原因

## 工作流模板管理

复用现有工作流模板管理能力，从 TaskListPage 创建抽屉中抽取。

### 功能
- 查看已保存的工作流模板列表
- 新建、编辑、删除模板
- 最后使用的模板记忆（workspace 级）
- 会话 composer 选择 WORKFLOW 模式时展示工作流模板下拉，并提供跳转工作流 tab 的快速入口
- WORKFLOW 模式发起会话等价于旧 UI 使用指定工作流创建 task
- 运行模式管理的工作流模板编辑区、旧 UI 创建任务抽屉、任务工作流页必须复用同一个 `WorkflowEditor` 组件；各入口只允许保留不同的外层模板选择/保存编排
- 工作流模板“新增模板”必须创建可编辑的空 `WorkflowDsl` 草稿并立即进入 `WorkflowEditor`；选择器显示“新增模板（未保存）”。空态只用于模板 store 不可用或没有可编辑草稿，不用于新增模板流程。
- 工作流模板编辑器必须区分完整 `WorkflowDsl` 草稿和画布投影：右侧 Inspector 的目标、模型、角色、权限、校验 schema、表达式、动态路由 prompt、动态控制等配置字段输入只更新编辑草稿，不应触发画布节点/边投影重算；只有节点增删、节点 id/type/provider、边 from/to/on、选中态、校验高亮和终点显隐等会改变画布呈现的字段才刷新 ReactFlow。
- “保存为新的工作流”不会继承来源 `workflow.id` 作为新模板 DSL ID；后端保存时生成 `workflow-{uuid}`，如与现有模板冲突最多重试 3 次
- 工作流模板存储在用户目录 `~/.gold-band/context/workflows.json`，属于用户级跨 workspace 模板；若新路径不存在且当前 workspace 仍存在旧版 `authoring/workflows.json`，首次读取时会复制迁移到用户级 context
- 保存/删除后必须立即刷新当前页面和会话主页持有的 workflow template store，新模板应立刻出现在模板选择器中，并显示保存后的模板名

## 校验规则

创建新会话时校验：
- workspace 已选择
- AUTO 模式：固定策略要求 agent；动态策略要求 bootstrap agent 和至少一个可用 agent；决策指南为空时，每个可用 agent 必须配置模型
- WORKFLOW 模式：workflow 模板存在且通过与运行模式管理保存相同的 DSL/Agent/Profile 校验；例如节点权限模式不属于当前 Agent 时必须阻断启动
- 校验失败时在 composer 下方持续展示错误和修复入口，直到用户重新发送或页面重新加载；不使用短暂消失的顶部 toast 承载阻断错误
- 工作流模板保存/另存被 DSL 校验或后端校验拦截时，必须在模板编辑区域展示错误原因，不允许表现为按钮无反应

## ACP 会话配置

- ACP 会话底部模型/权限切换使用 shadcn Select popper 弹层，弹层位置必须跟随触发器，不允许落到抽屉或页面左上角
- 用户切换模型后，当前 session snapshot/configOptions 的 current model 要作为下一次 ACP prompt 的模型 override 传给 provider；回复完成后 UI 不应回退到切换前模型

## 与 Agent/Context 管理的边界

- Agent 管理：独立页面，管理 agent 的配置和诊断
- 上下文管理：独立页面，管理角色（profile）
- 运行模式管理：仅管理 AUTO 设置 + 工作流模板，通过下拉引用上述两项
