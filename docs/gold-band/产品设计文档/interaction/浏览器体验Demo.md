# 浏览器体验 Demo

## 定位与范围

Demo 是复用客户端界面的产品体验入口，独立构建、独立部署。官网不属于本次范围。

- 两条固定的已完成会话，可切换、展开工具活动、查看会话信息。
- 工作流图和节点配置可查看；角色、Skill 的预设内容可查看。
- 会话中的 Markdown 链接打开现有右侧文件工作区，文件只读。
- 可调整主题、深浅色、语言、字体、字号以及侧栏布局。
- 不开放 Git 执行、Agent 执行、消息发送、业务保存、系统文件操作、安装、反馈提交或远程连接；源码管理可浏览预设假数据。
- 不实现业务 CRUD、浏览器文件系统或业务数据持久化。

## 模拟客户端体验（2026-09-09）

- 外层使用现有 shadcn/resizable 与 react-resizable-panels 组成窗口，可拖动左右边缘；内部三栏、双栏、单栏按实际容器宽度响应，不用浏览器宽度代替客户端宽度。手机保留窄窗口与抽屉导航。
- 标题栏浏览器展示不预留 macOS 原生窗口按钮的 72px 空间；桌面默认窗口策略不变。右上角语言选择与设置共用 saveDesktopPreferences、i18n 和浏览器偏好。
- 快速对话保留 Direct、Workflow、AUTO 和临时配置选择。快速对话及已有会话均保留 prompt-kit 输入框，显示中英文 Demo 限制提示，禁用输入、附件、发送和执行。
- Agent 管理展示样例目录全部配置卡片，可打开编辑表单并临时调整字段；禁用保存、新增、删除、诊断，关闭后丢弃临时值。
- 会话标题保留悬浮操作图标，修改类操作禁用；消息时间、复制按钮沿用客户端悬浮规则，复制已展示文字仍可用。
- 运行模式直接复用客户端 RunModeManagementPage 与 WorkflowEditor，保留工作流/AUTO、模板选择、画布/JSON、节点和边的配置、模型绑定、控制参数等完整界面。可临时调整，禁用保存、另存及持久删除。删除早期独立的 DemoWorkflow 简化图页面。
- 上下文默认显示九个内置角色。名称和摘要随语言切换，角色正文按打开动作加载 src/prompts 对应语言文件，不复制提示词正文到 Demo 代码。
- Skill 使用规范来源 .gold-band 匹配应用图标，展示多 Agent 已关联图标和溢出菜单；图标只读，不实际创建或删除软链。
- MCP 展示 HTTP、SSE 两个预设卡片，可查看预设工具。兼容 Agent 为 Claude、Codex；HTTP 均支持，SSE 的 Codex 显示不支持红点与提示。禁用服务器修改、开关和真实诊断，不请求样例 URL。

新增状态仅包含窗口几何、当前模板、快速对话选项与打开的临时表单；没有新增依赖、持久业务状态或桌面 API。数据量固定为两条会话、两套工作流、九个内置角色、一个 Skill、两个 MCP 和样例 Agent 目录。角色正文按需动态导入；拖动复用现有 ResizeObserver/动画帧边界，不在每个像素变化时刷新 Demo 根状态。

Windows 构建中适配器插件必须通过 Vite normalizePath 返回规范模块 ID，防止直接导入与插件导入形成两个 runtime 实例，导致设置与内容读取各自持有一份偏好状态。

## Deep Link 与会话样例

使用 hash 路由，直接打开和刷新无需静态服务器重写。支持 #conversation-home、#agents、#contexts、#settings、#run-mode-management；#contexts?tab=mcp 和 #contexts?tab=skills 定位上下文标签，#run-mode-management?mode=auto 定位 AUTO，template=default-lightweight 选择轻量模板。

#mock-task 为 Direct 会话；#demo-review 为默认轻量工作流会话，包含 run-052、run-051 两次运行，每次有 round-001、round-002 两轮，每轮包含 grill、dev-test、accept。首轮验收提出补充项，第二轮修正后通过。两次运行分别演示配置检查与模块职责整理，Run/Round/节点拥有独立会话身份与正文；所有开发节点保留附件和变更卡片。

侧栏展开工作流标题查看 Run 列表，原会话选择器切换 Round/节点。默认打开最新 Run 的末轮验收；#demo-review?node=dev-test 定位末轮开发节点，#demo-review?run=run-051&round=round-001&node=accept 定位历史首轮验收。导航保存完整 Run/Round/节点位置，支持前进、后退、刷新。未知参数回到有效样例，不解析任意文件路径或调用桌面 deep link。样例总规模为 2 个 Run、4 个 Round、12 个节点会话，列表仅存摘要，正文按所选会话读取，不增加依赖或持久状态。

Direct 会话摘要提供与详情一致的 agentIdentity，沿用客户端列表的 Claude 图标。开发节点提供一份 Markdown 报告附件与两项文件变更；复用 TurnFileChangesCard、附件工作区和只读 Diff 查看器。附件正文、变更清单、对比内容由 Demo 适配器按完整会话 locator 校验后按需返回，不连接 Git、不写入真实文件。首次打开和后续 hash 跳转共用运行模式参数解析。

手机宽度移除外框两侧留白，覆盖 resizable panel 的内联 display 样式，让模拟客户端占满视口。hover 继续按浏览器输入设备能力判断：普通鼠标设备保留悬浮效果，hover:none 的触摸/测试环境沿用客户端触摸规则，不按 Chrome 品牌禁用悬浮。

## 复用与隔离

“更多”与客户端保持一致，包含需求管理和定时任务。需求管理复用原看板，固定四条待办、进行中、完成、失败样例；已完成记录可回看会话，待办准备只读取正文并预填快速对话，不领取任务。工作空间选择仅为内存展示，添加、移除、取消、账号切换与断开禁用。

定时任务固定两条 Direct/Workflow 样例，每条两次执行记录。支持列表筛选、详情、历史会话跳转和配置查看；启停、立即执行、删除与保存禁用。快速对话复用发送组合按钮、定时配置工作区；配置“完成”只更新原 composer 内存草稿，发送与创建任务始终禁用。配置与需求准备共享原草稿边界，不引入第二份草稿。

右侧保留文件、源码管理和运行目录入口。源码管理使用已有 browser preview 的变更、分支、工作树、提交历史与 Diff，所有 Git 写操作禁用，GitHub 不连接远程仓库。运行目录固定 reports/review.md，按完整会话 locator 和相对路径校验后读取，文件只读，不允许打开系统文件管理器。

新增 hash 深链接：#multica-tasks、#scheduled-tasks、#scheduled-task-create、#scheduled-task-detail?id=demo-daily（或 demo-weekly）。管理页面懒加载，需求正文、执行历史与目录正文按需读取，样例无增长、不轮询、不持久化业务修改。

`marketing/demo` 装配现有 WorkspaceShell、ConversationRunPage、ContextManagementPage、SettingsPage、GraphView 和文件查看组件。Demo 的 Vite 配置仅在本次构建中把 RuntimeApi 的适配器导入解析到 Demo runtime；桌面 API 契约、client.ts、desktop.ts 和 Rust 后端不变。

共享组件通过默认关闭的 ReadOnlyExperience 上下文控制只读体验。该上下文只有稳定布尔值，不承载业务数据或高频状态。桌面入口不提供它，维持原有行为。Demo 适配器另行使用显式方法开放清单，未开放的业务和系统调用返回结构化错误，不能只依赖 UI 隐藏。

事实源是固定样例；每次读取返回独立快照。业务内容刷新恢复样例，访客不能修改样例。外观、字体、语言以及侧栏宽度只写入当前站点的版本化 localStorage，并用 Zod 校验。布局仅开放现有 sidebar.width、rightWorkspace.width、pinned.collapsed 三个偏好，不接受任意配置键。不会写入桌面配置。浏览器存储不可用时使用内存偏好。

## 响应式与维护

三栏、双栏、单栏沿用客户端布局规则；Demo 在窄屏左栏收起后通过现有 Sheet 与 ConversationSidebar 提供导航。右侧文件在窄屏沿用客户端 Sheet 展示。不另建移动客户端 UI。

共享组件变化后重新构建部署即可同步。新增后端功能不会自动开放；需要显式增加 Demo 方法与测试。

## 自评与验收

这是现有前端架构的装配补全，不替换桌面 API。复用 React、shadcn/ui、CodeMirror、现有 Markdown 和 React Flow，不增加依赖、服务器或数据库。

样例固定两条会话、一个 Skill、一个自定义角色和现有内置角色/工作流；正文按页面或文件打开时消费，设置和上下文页面懒加载。偏好只在用户操作时写入，拖拽仍沿用现有离散布局状态与完成后保存机制，不引入无界缓存、轮询或队列。

验收覆盖接口拒绝写入、只读文件、会话身份、偏好隔离和刷新恢复，以及桌面/窄屏浏览、主题与语言切换。验证记录见开发计划。
