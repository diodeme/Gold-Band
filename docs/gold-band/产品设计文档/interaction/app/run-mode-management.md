# 运行模式管理

## 信息架构

运行模式管理是会话式 UI 新增的独立设置页面，管理新会话创建的默认配置。

## 页面定位

- 仅管理 AUTO 设置和工作流模板管理
- 不吞并 Agent 管理和上下文管理（各自独立菜单）
- 页面壳与 Agent 管理、上下文管理、设置页保持一致：使用全宽 `PageHeader` 承载标题/返回操作，主体区域从左侧开始铺满可用宽度，不使用居中窄容器。
- 工作流模板编辑器作为主体工作区直接铺开；外层不再额外套卡片边框，避免在深色主题下形成嵌套面板。

## AUTO 设置

AUTO 模式本质上是一个只有 AI-DYNAMIC 节点的工作流。

### 配置项
- **节点 ID**：固定为 `ai-dynamic`
- **Agent 策略**：固定 Agent 或动态 Agent
- **固定 Agent**：固定策略下从 Agent 管理枚举已配置 agent，可选择该 agent 的模型；模型为空表示由 provider 默认模型或运行时 prompt 引导决定
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
- AUTO 模式下创建 task 前生成标准 WorkflowDsl
- 生成的 workflow 走现有 validation、snapshot、runtime
- 快速会话记忆上一次会话级 AUTO 选择；AUTO tab 的当前配置可保存为模板，并可切换生效模板
- AUTO 模板只保存 AI-DYNAMIC 模板级配置，不保存会话级权限模式和全局 Goal
- AUTO 模板存储在用户目录 `~/.gold-band/context/auto-templates.json`，属于用户级跨 workspace 模板；首次读取时若后端模板为空，会把旧版 `localStorage.gold-band-auto-mode-templates` 导入到该文件并清理旧 key
- AUTO 模板下拉支持选择和删除；删除当前模板只解除模板绑定并清空模板名，不清空用户正在编辑的 AUTO 配置字段
- AUTO 模板保存和另存必须给出明确反馈；模板名重复、Agent 不可用、动态策略缺少可用 Agent、无决策指南且可选动态 Agent 未选择模型、动态控制参数非法时不允许静默保存
- 动态 Agent 策略中，初始分发节点 Agent 可以独立选择模型；后续调起 bootstrap 节点时使用该模型，不复用可选动态 Agent 的模型配置
- 可选动态 Agent 的模型下拉支持清空。若 agent / 模型决策指南为空，则每个可选动态 Agent 必须选择模型，AI-DYNAMIC 内部 proposal DSL 不需要输出 `model`；若决策指南非空，则内部 proposal DSL 必须输出 `model`，但已在配置里选择模型的 Agent 仍固定使用配置模型，忽略 proposal 中对该 Agent 给出的其他模型
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
- “保存为新的工作流”不会继承来源 `workflow.id` 作为新模板 DSL ID；后端保存时生成 `workflow-{uuid}`，如与现有模板冲突最多重试 3 次
- 工作流模板存储在用户目录 `~/.gold-band/context/workflows.json`，属于用户级跨 workspace 模板；若新路径不存在且当前 workspace 仍存在旧版 `authoring/workflows.json`，首次读取时会复制迁移到用户级 context
- 保存/删除后必须立即刷新当前页面和会话主页持有的 workflow template store，新模板应立刻出现在模板选择器中，并显示保存后的模板名

## 校验规则

创建新会话时校验：
- workspace 已选择
- AUTO 模式：固定策略要求 agent；动态策略要求 bootstrap agent 和至少一个可用 agent；决策指南为空时，每个可用 agent 必须配置模型
- WORKFLOW 模式：workflow 模板有效
- 校验失败时在 composer 下方持续展示错误和修复入口，直到用户重新发送或页面重新加载；不使用短暂消失的顶部 toast 承载阻断错误
- 工作流模板保存/另存被 DSL 校验或后端校验拦截时，必须在模板编辑区域展示错误原因，不允许表现为按钮无反应

## ACP 会话配置

- ACP 会话底部模型/权限切换使用 shadcn Select popper 弹层，弹层位置必须跟随触发器，不允许落到抽屉或页面左上角
- 用户切换模型后，当前 session snapshot/configOptions 的 current model 要作为下一次 ACP prompt 的模型 override 传给 provider；回复完成后 UI 不应回退到切换前模型

## 与 Agent/Context 管理的边界

- Agent 管理：独立页面，管理 agent 的配置和诊断
- 上下文管理：独立页面，管理角色（profile）
- 运行模式管理：仅管理 AUTO 设置 + 工作流模板，通过下拉引用上述两项
