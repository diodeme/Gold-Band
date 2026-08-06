# 运行模式管理

## 信息架构

运行模式管理是会话式 UI 新增的独立设置页面，管理新会话创建的默认配置。

## 页面定位

- 仅管理 AUTO模式和工作流模式
- 页面主体顶部提供项目选择器，数据源复用新 UI 的 conversation workspace 列表；进入页面时默认选中当前会话首页/快速对话正在打开的项目，新增或删除项目后跟随该列表动态更新。
- 运行模式配置属于 workspace/project 级记忆；切换项目后，工作流模式 tab 和 AUTO模式 tab 必须加载该项目自己的运行模式选择。目标项目尚无保存记录时，页面明确回到默认 AUTO，而不是沿用上一个项目的状态。
- 顶部分段入口仅按“工作流模式 / AUTO模式”展示。会话主页额外提供的 Direct 属于快速会话本地配置，不进入本页。
- 页面不提供独立的页面级“保存”按钮；切换 `工作流模式 / AUTO模式` tab、切换工作流模板、切换 AUTO 模板时立即应用为当前默认运行模式。运行模式持久化必须保留另一侧配置：切到 WORKFLOW 不能清空 `autoConfig`，切回 AUTO 也不能清空已选工作流模板。
- 不吞并 Agent 管理和上下文管理（各自独立菜单）
- 页面壳与 Agent 管理、上下文管理、设置页保持一致：使用全宽 `PageHeader` 仅承载标题；同级页面通过侧栏导航，不提供重复的页内返回操作。主体区域从左侧开始铺满可用宽度，不使用居中窄容器。
- 工作流模板编辑器作为主体工作区直接铺开；外层不再额外套卡片边框，避免在深色主题下形成嵌套面板。
- 页面级原生滚动容器、弹层内部滚动区和 shadcn `ScrollArea` 均继承 Gold Band 主题滚动条 token；滚动条统一为 10px 交互轨道、约 4px 可见 thumb，不再使用系统默认灰色滚动条、局部灰色 thumb 或胖瘦不一致的 Radix 自绘滚动条。

## AUTO模式

AUTO 模式本质上是一个只有 AI-DYNAMIC 节点的工作流。

### 配置项
- **节点 ID**：固定为 `ai-dynamic`
- **Agent 策略**：固定 Agent 或动态 Agent
- **固定 Agent**：固定策略下从 Agent 管理枚举已配置 agent，并共同配置模型、思考强度与该 Agent doctor 返回的原生权限模式。内部 proposal 不输出 provider，runtime 注入固定 Agent 的初始化配置。
- **动态 Agent**：动态策略下依次配置初始分发 Agent、分发模型、验收模型、控制面共享权限，以及每个可选动态 worker Agent 的模型/原生权限/思考强度。分发模型与验收模型都只读取初始分发 Agent 的模型目录；Agent 决策指南只描述 worker 任务到 Agent 的选择规则。
- **允许调用的工作流**：引用工作流 DSL 内的 `workflow.id`
- **可用角色列表**：引用上下文管理中的 profile id
- **动态控制**：`maxDynamicNodes`、`maxFanout`、`maxDepth`、`maxParallel`、`maxGroupDepth`、`maxWorkflowInvocations`

### 会话级配置
- 固定 Agent 策略下，composer 展示 agent、模型和该 Agent 原生权限模式，可作为本次会话的初始化 override。
- 动态 Agent 策略下，composer 只展示 Dynamic Agent 标识；各 Agent 的初始化模型/权限在 AUTO 配置中预设，不再提供共享权限入口。
- ACP session 建立后，用户仍可通过会话 composer 的权威 session config options 实时切换当前会话模型与权限；该 override 不回写 AUTO 候选配置。
- **全局 Goal** 在 composer 中输入，非必填；运行时追加到每个 AI-DYNAMIC 内部节点目标
- composer 提供跳转 AUTO模式 tab 的快速入口，用户需要改模板级配置时直接进入运行模式管理

### 行为
- 每个 workspace 记忆上次选择的运行模式
- 运行模式管理页切换项目只改变当前编辑/保存的 project scope，不创建模板；保存修改、切换 tab、切换工作流模板和切换 AUTO 模板均写入当前选中的项目。
- AUTO 模式下创建 task 前生成标准 WorkflowDsl
- 生成的 workflow 走现有 validation、snapshot、runtime
- 快速会话记忆上一次会话级 AUTO 选择；AUTO模式 tab 的当前配置可保存为模板，并可切换生效模板
- AUTO 模板保存 AI-DYNAMIC 模板级的 fixed/bootstrap/候选 Agent 模型与原生权限配置，不保存全局 Goal
- AUTO 模板存储在用户目录 `~/.gold-band/context/auto-templates.json`，属于用户级跨 workspace 模板；创建时由后端生成 `auto-template-<uuid-v4-without-hyphens>` 分布式 ID，模板名称只用于展示与重名校验，不参与身份生成。首次读取时若后端模板为空，会把旧版 `localStorage.gold-band-auto-mode-templates` 导入到该文件并清理旧 key；既有模板 ID 保持不变。
- AUTO 模板下拉依次分为“新增模板”“不使用模板”和已保存模板列表三个区域；已保存模板列表与工作流模板列表均限制最大高度并在超出后内部滚动。“新增模板”创建独立的空白 AUTO 草稿并显示“新增模板（未保存）”，不得复用“不使用模板”或当前模板的配置。删除当前模板只解除模板绑定并清空模板名，不清空用户正在编辑的 AUTO 配置字段
- AUTO 模板选择器的高亮项必须与当前编辑身份一致：未保存草稿高亮“新增模板”；只有未绑定任何已保存模板且不存在草稿时，才高亮“不使用模板”。
- AUTO 与工作流模板管理复用同一个模板操作行组件；操作栏顺序统一为模板选择、保存修改、新模板名称、另存模板，“保存修改”和“另存”使用主题色按钮。
- AUTO 的“保存修改”保存当前 AUTO 配置：选中模板时更新该模板并设为当前默认 AUTO 配置；选择“不使用模板”时不创建模板，直接把当前参数持久化为默认 AUTO 配置。
- AUTO 的“另存模板”只负责创建新模板；模板名称为空或重复时仅在 AUTO模式 tab 内展示错误，不污染工作流模式区域。
- AUTO 保存修改和另存提交期间按钮展示“保存中…”并禁用重复提交；反馈横幅位于模板操作行下方，成功反馈短暂展示后自动消失，错误反馈保留在 AUTO模式 tab 内等待用户修正。
- AUTO 模板保存和另存必须给出明确反馈；模板名重复、Agent 不可用、动态策略缺少可用 Agent、原生权限不属于对应 Agent doctor 目录、动态控制参数非法时不允许静默保存
- 动态 Agent 策略中，bootstrap、merge、acceptance 固定使用初始分发 Agent；bootstrap 与验收模型分别配置，但三类控制面节点共用同一个原生权限模式，不复用候选 worker Agent 的配置
- 动态策略的思考强度按运行角色独立持久化：`bootstrapConfigOptions` 用于初始分发，`acceptanceConfigOptions` 用于 merge / acceptance，每个 `availableAgents[]` 自带 `configOptions` 用于该 provider 的普通动态 worker。runtime 必须按实际节点 kind/provider 选择对应 map，不能继续读取 AI-DYNAMIC 节点级 `configOptions` 作为动态策略的全局覆盖。
- 可选动态 Agent 的模型与权限均支持清空并使用 provider 默认值。无论 Agent 决策指南是否填写，内部 proposal DSL 都只输出 provider；runtime 查找候选行并注入预设模型、原生权限与 config options。
- AUTO 的可用角色列表只作为内部 worker proposal 的可选 profile ID 白名单；worker 不填 profile 时不注入角色内容。merge / acceptance 不接受 proposal profile，始终使用 runtime 内置 merge / acceptance prompt。
- Agent 列表展示所有已配置 Agent；未通过诊断或不支持的 Agent 置灰，不可选，并展示不可选原因
- 允许调用的工作流按 DSL `workflow.id` 去重判断；重复或空 ID 的工作流直接展示在允许调用工作流列表下方，标签保留名称，感叹号 icon tooltip 展示原因
- AUTO 配置加载后若“允许调用的工作流”包含已无法解析的 `workflow.id`，或“可用角色列表”包含已无法解析的 profile id，页面自动从当前 project 配置中剔除该引用并持久化；若当前已选 AUTO 模板也有同类失效引用，则在该模板被选中并加载时同步清理并回写其在用户级 `auto-templates.json` 中的记录。未选中的 AUTO 模板不扫描、不改写。模板仍在但 ID 重复、为空或包含不允许嵌套的 AI-DYNAMIC 时继续按常规校验处理，不自动删除。页面用黄色警告横幅分别告知已移除的工作流和角色数量；其消失规则由消息类型统一管理，warning 自动展示约 5 秒且不可被调用方改为常驻。切换 AUTO 模板时立即清除该横幅，后续保存和另存不再被该历史失效引用阻断。

## 工作流模式

复用现有工作流模板管理能力，从 TaskListPage 创建抽屉中抽取。

### 功能
- 查看已保存的工作流模板列表
- 内置默认模板使用稳定 ID `default` 作为存储与引用身份；所有展示入口（运行模式管理、会话 composer、旧任务创建页和工作流编辑器）根据当前界面语言显示“默认工作流 / Default workflow”，不得直接展示持久化历史名称。用户创建的模板名称保持原样。
- 新建、编辑、删除模板
- 最后使用的模板记忆（workspace 级）
- 会话 composer 选择 WORKFLOW 模式时展示工作流模板下拉，并提供跳转工作流模式 tab 的快速入口
- WORKFLOW 模式发起会话等价于旧 UI 使用指定工作流创建 task
- 运行模式管理的工作流模板编辑区、旧 UI 创建任务抽屉、任务工作流页必须复用同一个 `WorkflowEditor` 组件；各入口只允许保留不同的外层模板选择/保存编排
- 工作流模板“新增模板”必须创建可编辑的空 `WorkflowDsl` 草稿并立即进入 `WorkflowEditor`；选择器显示“新增模板（未保存）”。空态只用于模板 store 不可用或没有可编辑草稿，不用于新增模板流程。
- 工作流模板编辑器必须区分完整 `WorkflowDsl` 草稿和画布投影：右侧 Inspector 的目标、模型、角色、权限、校验 schema、表达式、动态路由 prompt、动态控制等配置字段输入只更新编辑草稿，不应触发画布节点/边投影重算；只有节点增删、节点 id/type/provider、边 from/to/on、选中态、校验高亮和终点显隐等会改变画布呈现的字段才刷新 ReactFlow。
- 普通 Worker 节点和 AI-DYNAMIC 的固定、动态 Agent 策略必须复用 Direct 的 ACP 模型复合选择器。固定策略的模型分别写入既有 `model` / `agentStrategy.model`，思考强度按 Agent 返回的真实 option id 写入节点 `config_options` / `configOptions`；动态策略写入初始分发、验收和各候选 Agent 自己的 role-scoped config options。不得硬编码 `reasoning_effort` 等 provider 专用字段；切换 Agent 或策略时同时清空对应模型与 option overrides，避免跨 Agent 污染。
- “保存为新的工作流”不会继承来源 `workflow.id` 作为新模板 DSL ID；后端保存时生成 `workflow-{uuid}`，如与现有模板冲突最多重试 3 次
- 工作流模板存储在用户目录 `~/.gold-band/context/workflows.json`，属于用户级跨 workspace 模板；若新路径不存在且当前 workspace 仍存在旧版 `authoring/workflows.json`，首次读取时会复制迁移到用户级 context
- 保存/删除后必须立即刷新当前页面和会话主页持有的 workflow template store；另存成功后以保存结果中的模板身份更新当前运行模式和编辑选择，新模板应立刻出现在模板选择器中并保持显示保存后的模板名，不能回退到默认工作流

## 校验规则

创建新会话时校验：
- workspace 已选择
- AUTO 模式：固定策略要求 agent；动态策略要求 bootstrap agent 和至少一个可用 agent；决策指南为空时，每个可用 agent 必须配置模型
- WORKFLOW 模式：workflow 模板存在且通过与运行模式管理保存相同的 DSL/Agent/Profile 校验；例如节点权限模式不属于当前 Agent 时必须阻断启动
- 校验失败时在 composer 下方持续展示错误和修复入口，直到用户重新发送或页面重新加载；不使用短暂消失的顶部 toast 承载阻断错误
- 工作流模板保存/另存被 DSL 校验或后端校验拦截时，必须在模板编辑区域展示错误原因，不允许表现为按钮无反应

## ACP 模型配置

- Direct/AUTO 发起会话、工作流节点 Inspector、AUTO 固定 Agent 模板配置与 ACP 已建立后的追问 composer 共用同一个模型复合选择器：单一“模型”触发器的第一层提供“模型”和“思考强度”，第二层展示对应选项；权限模式保持独立。追问区虽然嵌套在 PromptInput 内，但点击配置按钮、菜单项等交互元素时不得触发输入框聚焦，弹层位置必须跟随触发器，不允许落到抽屉或页面左上角。
- 思考强度属于通用 ACP config option override：能力发现只识别 `category=thought_level`，持久化使用 Agent 返回的 option id。普通 Worker 使用 DSL `config_options`，AI-DYNAMIC 与会话 AUTO 使用 camelCase `configOptions`，运行时继续通过既有 `BTreeMap<String, String>` 管道传给 provider。
- 复合选择器的子菜单开合由 Radix DropdownMenu 原生的指针、点击与键盘状态统一管理，业务组件不得重复绑定点击切换，避免一次点击发生两次状态翻转。
- Composer 内相邻的模型与权限配置菜单统一使用非模态 DropdownMenu 交互；无论当前展开哪一个，单击另一个都必须在同一次点击中完成关闭旧菜单并打开新菜单，不允许使用会消费第一次外部点击的模态 Select 弹层。
- 发起会话前允许模型、权限和思考强度回到“不指定”；进入追问 session 后，“不指定”只在对应显式 override 尚未建立时提供，任一配置选择具体值后便不能再清空，只能切换到其他具体值。
- 用户切换模型后，当前 session snapshot/configOptions 的 current model 要作为下一次 ACP prompt 的模型 override 传给 provider；回复完成后 UI 不应回退到切换前模型

## 与 Agent/Context 管理的边界

- Agent 管理：独立页面，管理 agent 的配置和诊断
- 上下文管理：独立页面，管理角色（profile）
- 运行模式管理：仅管理 AUTO 与工作流模式，通过引用 Agent、角色和工作流模板完成组合，不复制其源配置

## Direct 模式

- Direct 不在运行模式管理页提供 tab、配置区或保存入口。
- Direct 的 Agent、模型和权限全部在快速会话 composer 内完成选择；切换 Agent 时按 workspace 恢复该 Agent 上一次的模型与权限偏好。
- Direct 校验错误在 composer 内联展示，不提供跳转运行模式管理的“去配置 / 修复”按钮。
- Agent 的 adapter 命令、环境和诊断仍由独立的 Agent 管理页维护；快速会话只引用 Agent，不复制其源配置。
