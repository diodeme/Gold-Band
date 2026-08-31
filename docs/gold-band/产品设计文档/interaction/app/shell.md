# 桌面客户端应用壳与一级导航

## 1. 一句话定义
应用壳定义 Gold Band 桌面端的全局框架：左侧负责导航，中间承载当前一级任务，右侧按需承载跨页面辅助资源。

---

## 2. 页面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ 共享顶栏：品牌 icon+标题 / 左栏开关 / 右栏开关 / 可拖拽空白区 / 窗口控制    │
├───────────────┬──────────────────────────────────────────────┤
│ Logo          │ 当前一级功能区                                │
│               │                                              │
│ 一级菜单       │ 页面标题 / 面包屑 / 页面操作                    │
│ - 任务编排     │                                              │
│ - 上下文管理   │ 页面主体                                      │
│               │                                              │
│ Settings      │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

---

## 3. 左侧一级功能区

### 3.1 Logo 区
位于左上角。

内容：
- Gold Band logo
- 可选：当前 workspace 名称

行为：
- 点击 logo 返回当前 workspace 的默认入口。
- 当前 MVP 中默认返回“任务编排 / 任务列表”，并重置任务编排内部的深层页面状态。

### 3.2 Workspace 选择与记忆
左侧 Logo 下方显示当前 workspace 路径，并作为”切换工作空间”入口。

规则：
- 桌面端启动上下文只从当前进程目录向上查找包含 `.git/` 或 `.gold-band/` 的项目根目录，避免 Tauri dev 从 `src-tauri/` 启动时误读子目录；该上下文只作为内部配置、诊断与旧 Workbench 的进程内 seed，不从用户设置恢复单一“当前 workspace”。
- 用户可通过原生目录选择器打开新的 workspace；选择后立即刷新任务编排页面栈。
- 桌面端原生目录选择器在主线程必须使用非阻塞调用；禁止在 workspace 选择链路使用 blocking dialog API，避免 macOS 上触发 event loop 卡死。
- 最近使用 workspace 写入用户级本地偏好，不属于 task / run / round canonical state。
- **新旧 UI 多工作空间职责分离**：旧 UI（工作台模式）只在当前进程内维护单一全局 workspace（`DesktopContext.repo_root`），所有 task/run 操作均在该 workspace 下执行；新 UI（会话模式）以 `conversation_workspaces` 作为唯一工作空间列表，并以 `last_conversation_workspace` 记录最后活跃项。会话侧边栏和命令不得无条件使用 `DesktopContext.repo_root`；每次创建、追问、查看历史、权限处理、停止或附件读取都先从持久化工作空间解析路径，再构造 workspace-scoped `App.paths.repo_root`。Runtime 启动恢复不扫描该列表，只消费用户级 `core.db` 中跨工作空间的有界 recovery candidates，并对每条候选构造 scoped App。
- **Workspace 唯一身份**：`projectId` 是 workspace 唯一业务身份，目录、路由、事件、缓存、持久引用和 Runtime recovery 均以它作为作用域；不再保留 `workspaceKey` 平行身份。`workspacePath` 是可重新校验的磁盘 locator，`name` 是用户可编辑展示字段，二者都不能反向成为关联主键。`projectId` 由规范化 workspace 路径确定性生成，格式为 `{最多 70 位可读 slug}--{8 位 BLAKE3 十六进制哈希}`，完整长度不超过 80；源参数统一来自 `configs/app-config.toml [projectIdentity]`，slug 上限由总长、分隔符和哈希长度计算，不重复配置。
- **Manifest 失败边界**：桌面上下文初始化、workspace 注册、同步、切换、Runtime recovery 与 Scheduler 注册在访问项目运行目录前都必须完成 manifest 读取、归属校验或首次创建。缺失、损坏、归属冲突以及读取、创建、原子提交发生的 I/O 失败均阻断该 workspace 初始化或写入；不得因操作属于启动读取边界而吞掉 I/O 错误后继续运行。
- **身份迁移恢复边界**：旧项目目录改名后，启动迁移必须同步改写 `run.json`、`worker-ref.json`、ACP session/snapshot、文件变更记录，以及 AI-DYNAMIC `graph.json.workspaces[*].path` 中明确参与执行或恢复的旧 runtime locator。`graph.json` 是 AI-DYNAMIC workspace catalog 的 canonical state，`dynamic/workspaces/*.json` 必须从 graph 同步投影；格式损坏的单条 graph 只隔离对应历史运行，不阻断桌面启动，读写 I/O 失败同样不阻断本次启动但保留未完成门闩供下次重试。迁移还需对 `run.json` 引用且位于受管 `runtime/worktrees/` 下的 linked worktree 条件式执行 Git 原生 `worktree repair`，repair 失败不得阻断全局迁移，后续使用或删除前按 Git 登记事实重试。除可按使用边界补偿的 Git repair 外，持久化步骤完成后提交 `core_schema.workspace_identity=3`；版本低于 3 时补跑，完成后启动直接跳过。raw/timeline/diagnostics 历史记录保持原样，不作为恢复事实源，也不建立旧 `projectId` fallback。
- **工作台观察期边界**：2026-07-22 起产品内隐藏 Workbench / Conversation 形态切换入口，桌面根路径默认进入 `/chat` 会话主页；旧工作台页面、路由与单 workspace 状态暂时保留，只允许通过显式 `/tasks`、`/agents`、`/contexts`、`/settings` 等 deep link 访问，不再读取历史 UI 模式偏好覆盖默认入口。
- **持久化边界**：`recent_desktop_workspaces` 仅由旧 UI 管理（`choose_workspace` / `select_recent_workspace` / `remove_recent_workspace`）；`conversation_workspaces` 和 `last_conversation_workspace` 仅由新 UI 管理（`add_conversation_workspace` / 成功创建/重跑后的 `save_last_conversation_workspace` / `remove_conversation_workspace`）。新 UI 添加、查看或草稿选择 workspace 不污染旧 UI 最近列表。
- **废弃字段移除**：`SettingsConfig.desktop_workspace` 已删除，settings schema v6 在读取旧配置时移除磁盘上的 `desktopWorkspace`。旧 Workbench 仅保留 `recent_desktop_workspaces` 最近列表，不再持久化或恢复单一当前 workspace；会话 UI 的 workspace canonical state 只允许来自 `conversation_workspaces`，`last_conversation_workspace` 只决定最近活跃项。Runtime 恢复范围由运行前登记的 recovery candidates 决定，不由最近或最后活跃列表扩张或缩小。
- **旧状态迁移**：workspace identity 升级在桌面上下文初始化、Runtime recovery 与 Scheduler 启动之前执行一次。迁移按规范化路径确定性生成新 `projectId`，移动旧项目目录，重写 manifest、会话 workspace/run mode/pin/最后活跃引用，以及明确位于旧 runtime root 下的 executable locator；AI-DYNAMIC 只迁移外层运行在普通会话 worktree 时保存的 workspace path，外层主工作区和仓库内 `.gold-band/worktrees/` 子 worktree 不命中。迁移扫描明确跳过 `runtime/worktrees/` checkout，不读取或改写用户源码。普通会话 linked worktree 以 Git common-dir 和主仓库 worktree catalog 为权威去重并 repair，同步重建搜索投影后写入 `core.db` 完成版本。带合法 manifest 的历史项目目录即使原 workspace 已删除或不在最近列表中，也以 manifest 所在实际目录作为迁移来源；无 manifest 且不被 StateConfig 识别的异常目录保持原样并跳过。版本低于 3 时即使目录已改名也补跑，达到 3 后不再枚举或写入 workspace 数据。迁移完成后的查询、移除和路由只接受持久化 canonical `projectId` 精确匹配，不维护旧 ID、大小写或路径重算 alias；raw/timeline/diagnostics 历史内容保持不变。
- **最近列表管理**：旧 UI workspace 选择页的最近列表每行提供打开与移除操作；移除只删除用户级 `recent_desktop_workspaces` 记录，不切换当前 workspace，不删除磁盘目录，也不影响新 UI 的 `conversation_workspaces`。当前正在使用的 workspace 不允许从最近列表移除；有效最近列表只剩一个 workspace 时也禁用移除，避免把工作台置入无当前 workspace 的状态。

### 3.3 一级菜单
当前菜单：

```text
任务编排
Agent 管理
上下文管理（占位）
模型管理（占位）
```

规则：
- 一级菜单只控制中间主工作区的根模块。
- 点击“任务编排”应回到任务列表根页面，而不是保留任务工作流或 Round 详情等深层页面。
- 当前实现任务编排、Agent 管理、上下文管理和设置。
- 上下文管理当前提供角色管理（Profile Management），用于维护工作流节点引用的 profile；列表拆成“自定义角色 / 内置角色”双 tab，自定义角色维护独立分页与过滤，内置角色单独浏览。内置角色与自定义角色卡片使用同一套紧凑密度基线，桌面常见宽度下优先维持一行 3 张卡片，再按宽度退化。浏览器预览 mock 与桌面端 Tauri 数据通路必须分层隔离，不能在正式路径复用 mock 状态；该分层由前端 Vitest 回归测试持续覆盖 runtime 选择、facade 透传和 desktop/browser 双实现语义。
- 上下文管理的 MCP 管理页按“自定义 MCP / 内置 MCP”分段展示。自定义 MCP 允许用户新增、编辑、删除和启停；内置 MCP 由渠道配置注入，只允许用户查看、诊断、查看工具列表和启停，不能编辑或删除。内置 MCP 仅在对应渠道声明 `builtinMcpServers` 时注入，首次注入使用渠道配置的 `enabled` 默认值；后续启动同步只刷新名称、连接方式和帮助信息，必须保留用户在本机选择的启停状态。MCP 工具列表通过后端 `tools/list` 获取，stdio transport 必须在同一个子进程会话中先完成 `initialize`，再等待 `tools/list` 的 JSON-RPC 响应，不能把 initialize 响应误当作工具列表结果。Gold Band 设置页和存储层继续使用内部 transport + map 结构；发送 ACP `session/new|load` 时必须转换为 ACP `mcpServers` wire format：stdio 不带 `type`，HTTP/SSE 使用 `type: "http"|"sse"`，`env` 与 `headers` 均为 `{ name, value }` 数组，不能透传内部 `id`、`transport` 或对象 map。
- 模型管理保持占位，可显示 disabled 或 coming soon 状态。
- 不在一级菜单中放 run、round、node 等任务内部对象。

### 3.4 Settings 入口
位于左下角。

行为：
- 点击进入设置页，或打开设置浮层。
- 当前包含通用、外观、高级三类设置：通用承载语言，外观承载主题与字体，高级承载更新地址覆盖和手动检查更新。
- 当后台发现当前可更新版本且用户尚未读到该版本时，Settings 入口右侧显示红点；用户进入设置页后，只清除这一层红点，不影响高级页和更新分组内更深层的提醒。

---

## 4. 中间主工作区与右侧辅助工作区

应用壳固定为左侧导航、中间主工作区、右侧会话工作区三个领域。一级页面及其页面栈只占用中间主工作区；右侧只在快速对话和具体会话详情中可用，用于承载当前会话的辅助资源，不再作为跨一级页面保留内容的全局杂物栏。

- 中间主工作区承载会话、任务详情、工作流画布、上下文卡片和设置等一级任务内容。
- 左侧导航与中间主工作区之间只由中间区域自身的圆角边界绘制可见分隔；可拖拽 resize handle 仅提供命中区域，不再额外绘制贯穿全高的直线，避免直线与左上圆角在顶部形成断裂接缝。
- 右侧辅助工作区使用通用资源 Tab 描述符。当前正式资源包括 Agent 分支会话、工作空间文件、运行工作流查看、运行工作流编辑/修复、ACP 系统提示和 ACP 原始帧；后续 Diff、产物和日志复用同一容器。文件资源的格式、保存、授权和性能边界见 [右侧工作区文件浏览与编辑](workspace-files.md)。
- 资源 Tab 只保存稳定 locator：工作流资源绑定 `projectId/taskId/runId`，系统提示与原始帧绑定完整 attempt locator（含 outer attempt 与 `branchId`）。工作流图、编辑草稿、system prompt 正文和 raw frame page 都不进入轻量会话 LRU；只有激活 Tab 才解析或查询对应大内容。
- 会话详情页的“查看工作流 / 编辑工作流 / 修复工作流”统一打开右侧资源，不再打开独立 Sheet；ACP 标题栏的“系统提示 / 原始帧”也统一打开右侧资源，不再替换主会话画布。嵌套 Agent 使用自身 attempt/branch locator 打开对应资源，切换资源不得卸载或重置原 Agent Tab 的缓存内容。
- 原始帧工具栏按右侧资源容器宽度布局：搜索框固定独占第一行；事件类型、方向、排序位于下一行横向排列并允许自然换行。禁止使用一个大断点在“整组竖排 / 整组横排”之间切换，避免宽面板仍出现三个 Select 纵向堆叠。
- 会话标题栏中的“系统提示 / 原始帧”是打开或聚焦右侧资源的动作入口，不承担资源选中态；当前激活状态只由右侧 Tab 表达。只有不存在会话工作区、按钮确实在当前页面切换 Raw 画布的旧页面，才允许“原始帧”按钮显示选中态。
- 工作流编辑草稿独立于 Tab 描述符按资源 key 做有限运行期缓存。收起右栏、切换 Tab 或暂时切走会话不能丢草稿；主动关闭存在未保存定义或模型绑定修改的编辑 Tab 必须确认。编辑资源只在激活时按完整 workspace locator 读取 Task authoring 聚合，运行图继续读取当前 run snapshot；保存复用 canonical `saveTaskWorkflow` 协议，以返回的最新 `WorkflowVm` 更新草稿基线，并刷新当前会话 run snapshot。
- 资源以稳定 `resourceKey` 去重；关闭当前 Tab 后激活相邻 Tab，关闭最后一个 Tab 后同步收起右侧工作区并清空激活态；需要继续使用空白入口时，可通过顶栏右栏开关重新打开。不把资源集合是否为空当成自动恢复展开的理由。Tab 条允许原生横向滚动；只有 `scrollWidth` 实际超过 `clientWidth` 时才显示紧凑的完整 Tab 菜单，未溢出时不长期占用标题栏空间。Tab 条与会话正文、设置页和资源树共用 `gold-themed-scrollbar` 的平台能力分支，不得为了独立压缩高度而切换到另一套浏览器滚动条渲染路径。Tab 采用有间距的轻量标签布局：激活项使用圆角弱底色和正常前景色，关闭按钮常显但降低透明度；未激活项透明，仅在 hover 时出现弱底色和关闭按钮。不使用整格矩形填充、竖分隔线或底部选中横线。
- 只挂载激活 Tab 的内容 DOM。非激活资源仅保留轻量定位、状态、attention、有限分页窗口与滚动恢复状态，不长期隐藏挂载多个消息视口。
- Agent 分支的可展示条件由分支领域数据决定：非根 `branchId` 已返回 canonical `branchExecution` 时即为有效会话，不得继续等待只属于根会话的 system prompt、配置选项或 Gold Band user prompt。`interrupted` 等历史分支也必须在首次有效查询后停止初始化重试。
- 已打开 Agent 的完整但有限语义窗口进入由 `acpChatResourceCacheSessionCount` 控制的内存 LRU，默认最多 8 个 branch key；切换 Tab 时先同步恢复会话、事件窗口和滚动锚点，再在后台刷新 canonical 数据。缓存未命中才展示加载壳，刷新不得把已经可审计的内容退回“加载中”。
- 右侧 Dock 与紧凑宽度 Sheet 共用同一 Tab state 和内容组件。窗口自动收窄只隐藏 Dock，不自动用 Sheet 覆盖中间内容；用户在紧凑模式显式点击资源链接时才打开 Sheet。
- 用户手动关闭工作区只改变会话 Shell 级 `requestedOpen`，不删除 Tab；自动折叠、进入没有右栏能力的运行模式/管理页面以及资源 scope 切换都只改变有效呈现，不得覆盖手动开关意图。
- `requestedOpen` 与 `tabs` 独立建模：共享顶栏右栏开关可以在 `tabs=[]` 时展开空白入口页。`requestedOpen`、打开动作 revision 与当前运行期宽度投影归属会话 Shell，由快速对话和具体会话详情共享；快速对话使用 `draft:<projectId>` scope，具体会话使用 `conversation:<projectId>:<taskId>:<runId>` scope，只有 Tab 与激活态在当前 scope 内读写。资源描述符必须携带相同 `scopeKey`，不允许旧会话资源写入新会话。
- 会话资源轻量状态进入 24 项运行期 LRU，只在用户进入 scope、打开或操作资源时更新访问顺序，后台流式事件不 touch。第 25 个有状态 scope 淘汰最久未访问项；被淘汰会话再次进入时 Tab 与激活态为空，但右栏是否展开仍服从 Shell 级用户意图。快速对话创建新会话时删除 draft 资源，不迁移可能带会话归属的 Tab；Shell 级展开意图无需迁移。
- ACP Session VM、有限事件窗口、正文 hydrate 标记和滚动/分页锚点按同一 resource key 进入统一重资源 LRU；容量由 `acpChatResourceCacheSessionCount` 控制，默认 8。淘汰必须原子释放同一 resource 的全部可重建投影，禁止独立顺序造成部分大对象继续驻留。live branch snapshot 继续使用自身 64 项轻量上限，并保护仍有订阅者的条目。
- Tab 与激活态只保存在当前应用进程内，重启后清空；右侧工作区像素宽度不属于会话内容，继续写入用户级 conversation preference 并跨重启全局恢复，切换会话不得造成宽度跳变。
- 会话模式的 `WorkspaceShell` 必须在中间主工作区、右侧 Dock 和紧凑 Sheet 的共同稳定边界提供一次 shadcn `TooltipProvider`。资源面板可以直接复用主会话中含 Tooltip 的标题、工具和操作组件；不得要求每种右侧资源自行补 Provider，否则 Agent 内容异步加载后会使整个工作区渲染树异常退出。
- 资源 renderer 与关闭 resolver 按 `RightWorkspaceResource.kind` 注册，文件、工作流和诊断资源不能使用会相互覆盖的单例回调。文件资源切换、收起和关闭前由异步 resolver 冲刷自动保存；保存失败或 revision 冲突时保留 Tab。
- 应用整窗关闭由根壳唯一的关闭事务接管，旧 Workbench deep link 与会话 UI 不得各自注册关闭生命周期。原生标题栏按钮、任务栏“关闭窗口”和系统快捷键触发 `onCloseRequested` 后立即阻止默认关闭；并发请求复用当前事务，不重复保存、停止会话或销毁窗口。事务先冲刷全部文件保存队列：成功则继续；失败或异常则使用 shadcn `AlertDialog` 提供“重试保存 / 取消关闭 / 放弃修改并退出”。重试只重新执行保存，取消必须保持窗口和所有运行会话不变，放弃只丢弃本进程尚未落盘的文件修改。
- 文件保存决策通过后，前端调用唯一的 Rust `prepare_app_exit` 接口，按顺序暂停运行中 Run、取消活跃 ACP attempt、关闭 ACP 连接、清理 Agent 诊断进程并处理待安装更新。各清理步骤保持 best-effort，并只返回结构化 warning code；禁止在 Rust `WindowEvent::CloseRequested` 中提前执行这些副作用，否则保存失败后取消关闭仍会停止会话。退出准备结束后前端显式调用一次 `destroy()`，关闭回调内禁止递归调用 `close()`。主窗口 capability 必须同时声明 `core:window:allow-close` 与 `core:window:allow-destroy`。
- 右栏像素宽度仍是用户级偏好；文件资源首次挂载时可请求现有最大宽度以优先形成“左详情、右目录”双栏，用户之后的手动 resize 不得被持续回弹。可用宽度低于文件 split 阈值时使用文件/目录单栏切换。

---

## 5. 面包屑规则
面包屑只出现在中间主工作区，不属于左侧一级菜单或右侧资源 Tab。

示例：

```text
任务列表 > 任务01 > 工作流 > run01 > round01
```

行为：
- 点击“任务列表”返回任务列表页。
- “任务01”仅作为当前任务上下文标签，不可点击，不显示 hover 底线。
- 点击“工作流”返回任务工作流页；当“工作流”为当前页时只表示当前位置。
- 当前页最后一项不可点击或仅表示当前位置。
- 可点击上级项默认无下划线，鼠标悬浮或键盘 focus 时使用文字提亮与 primary 底边线作为可选中反馈；当前页使用更短的金色渐变底线表示当前位置；不可点击项不显示底线，只显示文本状态。

---

## 6. 桌面端交互原则

### 6.1 保持稳定区域
- 左侧一级功能区不随页面下钻变化。
- 中间主工作区随页面层级变化；右侧会话工作区只在快速对话与会话详情中恢复对应 scope。进入 Agent 管理、上下文管理、运行模式管理或设置时隐藏顶栏入口并卸载 Dock，但不改写会话 Shell 的打开意图、运行期宽度，也不主动清除仍在 LRU 内的资源状态。
- 在中心最小宽度不同的一级功能页与会话页之间往返时，左栏用户宽度、右栏用户宽度和当前页面布局 profile 共同生成唯一 canonical 三栏布局。若 `react-resizable-panels` 在约束更新首帧把目标布局夹到旧页面的中心最小宽度，Shell 只在实际 applied layout 与 canonical 目标相差超过 1px 时于下一帧有界重放一次；不得把该瞬时投影写回用户偏好，也不得通过持续 Observer、轮询或第二套宽度状态修补。
- 用户始终知道自己处于哪个一级模块。

### 6.2 避免终端心智
应用壳不提供：
- command bar
- slash command
- terminal input
- chat input

全局操作应放入：
- 菜单
- 按钮
- 右键菜单
- 设置页
- 快捷键

### 6.3 适配桌面窗口
功能区应支持：
- 主窗口首次创建和 macOS Dock 重建统一读取 Tauri 权威窗口配置；默认逻辑尺寸为 1280×720，并由宿主按当前显示器工作区居中。默认位置不使用固定物理坐标，也不持久化临时启动位置，避免系统缩放或显示器变化后窗口落到可见区域之外。
- 面板宽度调整
- 列表滚动
- 详情页滚动
- 大屏展示更多列
- 小窗口下优先保证中间页面核心内容；左侧导航和右侧辅助工作区按统一状态机自动收起，并保留显式恢复入口
- 应用整窗统一使用共享顶栏，不保留额外原生 header；macOS 仅保留系统左上角 traffic lights，Windows/Linux 使用顶栏右侧自定义窗口按钮
- 顶栏颜色跟随当前主题切换，浅色与深色主题都维持同一套结构
- 共享顶栏统一使用 36px 紧凑高度；品牌图标容器为 28×36px，应用品牌标题使用独立的 16px/700 字重，不继承全局界面强调所用的 520 `font-bold` 映射。Logo 以此共享规格填充容器，保证在紧凑顶栏中具备与标题相称的视觉权重。左右栏开关继续使用 28px 点击区和 14px 对称 `PanelLeft/PanelRight` 图标。顶栏左侧按“品牌 icon+标题、左侧导航开关”排列，右侧工作区开关放在顶栏尾部操作区。Windows/Linux 中它位于自定义窗口控制组之前，自定义窗口按钮填满顶栏高度但保留既有横向点击宽度；macOS 的原生 traffic lights 仍由品牌前方安全区占位，右栏开关通过正常 flex 流停在顶栏右端，不使用绝对坐标。打开态仅使用低对比 titlebar hover surface，不增加强边框。右栏按钮只在快速对话与会话详情页展示，观察期内不展示 Workbench / Conversation 形态切换控件。
- 共享顶栏除按钮、输入等交互控件外都属于窗口拖拽命中区；鼠标拖拽与双击最大化统一由 Tauri `data-tauri-drag-region` 注入脚本管理，WebView2 `app-region: drag` 仅作为 Windows 触摸/触控笔补充。禁止 React `mousedown` 再手动调用 `startDragging()`，避免同一手势重复进入 TAO 原生拖拽生命周期；交互控件必须显式退出拖拽区
- 侧边栏折叠/展开使用平滑宽度过渡，不做瞬时消失；内容透明度可略早于宽度收起，以减少视觉突兀
- 顶栏与侧边栏默认共用同一 surface 底色，并去掉强横向分割线；右侧主区使用左上圆角和主题化主 surface 阴影表达统一的前后层级，主区圆角后方露出的底色继续复用 sidebar surface，而不是把侧边栏自身裁成圆角，避免角后方出现异色小方块。主区统一消费 `--workspace-main-surface-shadow`，但大面积工作区不得直接叠加以底部层级为主的 composer material shadow，否则其侧向扩散会与环境层重合而形成深色灰带。最终投影按方向分工：左侧与四周环境层使用零偏移、零 spread、16px blur 和主题 `--gold-window-edge-shadow` 的 45%；顶部层使用 `0 -8px 16px -8px` 和同一主题色的 85%，利用负 spread 将增强限制在顶部，避免加深长边。中心区与展开后的右侧工作区必须由各自不会裁切的 surface host 消费同一 token，使顶部层级连续；中心区左侧不得再叠加实体 `border-left`，避免系统缩放后形成粗硬边，中心与右侧之间仅由 resize handle 表达可调整边界。承载左中右面板的工作区容器必须纵向放行投影越过顶部边界，同时在横向使用 `overflow: clip` 阻止贴近窗口边缘的投影扩大页面滚动宽度，内层 `<main>` 只负责背景、顶部边界与内容裁切。禁止按页面写死阴影值、正 spread 或多层同向投影；顶部专用负 spread 不得在左侧长边形成可辨识加深或圆角灰块
- 会话正文继续复用统一的 `gold-themed-scrollbar` 交互区域，但使用独立的低对比语义色：轨道始终透明，静止 thumb 为当前主题前景色的 11%，hover token 为 22%。该变体只挂载到会话主滚动 viewport，不降低设置页、资源树或右侧工作区等高密度滚动区的可发现性；静止态弱化后仍必须保留平台 hover 反馈和键盘滚动能力。所有浏览器原生滚动容器必须互斥选择绘制路径：支持标准属性时统一使用 `scrollbar-width: thin` 与 `scrollbar-color`；仅在不支持标准属性的旧 WKWebView / WebKitGTK 中通过 `@supports not (scrollbar-width: thin)` 启用 `::-webkit-scrollbar` fallback。不得通过局部 `scrollbar-width: auto`、`scrollbar-color: auto` 或面板状态切换绘制所有权；同一平台中打开/收起右侧工作区、窗口 resize 与 viewport reflow 都不得改变滚动条渲染器和可见宽度。旧 WebKit fallback 中会话容器 hover 不显示整条轨道底色，避免轨道与半透明 thumb 叠色。
- 桌面外轮廓、阴影与圆角优先由宿主窗口 compositor 管理；Windows 11 及更高版本不得再叠加整窗 border、CSS 圆角或高层级伪元素，避免与 DWM 圆角形成双层轮廓。Windows 10 不具备 DWM 原生圆角能力，由宿主 bootstrap 下发统一的 frame/shadow policy：关闭 TAO undecorated native shadow，应用根壳仅在 `app-outline` 策略下绘制不占布局的主题化 1px 内侧边界，并叠加四边一致的低透明度柔和内阴影，增强独立窗口层次；边界和阴影按主题 token 管理，不得使用可能被 WebView viewport 裁切的 CSS `outline`。Win10 保持系统方角，不伪造与真实窗口区域不一致的 CSS 圆角。
- Windows/Linux 顶栏右侧承载窗口最小化、最大化/还原、关闭操作；macOS 使用系统原生左上角 traffic lights，顶栏不重复渲染自定义窗口按钮
- Windows/Linux 自定义窗口控制组使用 `w-max + flex-none` 保持 intrinsic width，组内最小化、最大化/还原、关闭三个按钮也必须分别使用 `flex-none`，禁止嵌套 Flex 在窄窗口下压缩按钮宽度；关闭按钮按 Windows 原生标题栏习惯贴紧右侧窗口边缘，不额外设置右侧 gutter。
- 顶栏窗口控制按钮组左侧可承载「帮助」入口（DropdownMenu）。是否展示由渠道配置的 `feedbackEnabled` 能力布尔值决定，经 `AppInfoVm` 贯穿应用壳；当前 `wb=true`、默认渠道为 false。前端不根据渠道名称猜能力，后端 command 也必须再次校验。当前菜单仅含「用户反馈」，详见 interaction/app/feedback.md。
- 桌面窗口最小尺寸由 `configs/app-config.toml` 的 `workspaceLayout` 页面 profile 统一管理，前端在页面切换时通过 Tauri Window API 更新原生最小尺寸；`tauri.conf.json` 与 `html/body/#root` 不得再维护固定 `min-width/min-height`。WebView 必须始终服从真实 viewport 尺寸并继续触发响应式布局，禁止在达到 CSS 最小宽度后保持旧布局、由原生窗口直接裁切右侧内容。
- 桌面壳内的二级布局必须基于实际内容容器宽度决定分栏，而不是直接复用整窗 `md/lg/xl` breakpoint。侧边栏、section 标题列、抽屉和详情 inspector 都会减少真实可用宽度；嵌套区域优先使用 Tailwind container query，固定画布/表格则必须提供明确的换行、堆叠或横向滚动降级策略。
- 三段式工作区使用 shadcn `Resizable` copy-in（`react-resizable-panels`）统一管理面板，不维护手写全局 mousemove 拖拽。页面通过 `configs/app-config.toml` 的集中式 profile 分别声明中间区域硬下限 `centerMinWidth`、无右栏时的左栏自动收起舒适宽度 `centerAutoCollapseWidth` 和当前页面原生窗口下限 `windowMinWidth`。会话为 360/420/480px，上下文卡片为 520/520/520px、工作流画布为 640/640/640px、设置为 480/480/480px。中间硬下限与右侧 288–1440px 范围直接声明为 Resizable Panel 约束；组件库在同一 flex layout 中联合求解，不得把整窗每像素宽度重新写入 React state 或动态生成右栏 `maxSize`。
- 横向缩小时先把右侧压到最小宽度，再自动隐藏左栏，再隐藏右侧；放大时先恢复右侧、再恢复左侧。窗口和面板的像素尺寸必须在系统拖拽期间连续跟随，不做 debounce 或“释放后才改变”的延迟布局。折叠阈值使用当前持久化左栏宽度和 48px 迟滞，手动折叠和自动折叠分别建模；ResizeObserver 只在 animation frame 更新 ref 中的 `previousWidth`，仅当 `{left,right,rightOwnsWindowResize}` 离散呈现状态改变时提交 React state。左右宽度统一在 `ResizablePanelGroup.onLayoutChanged` 确认用户完成分隔线拖动后换算为像素并写入会话 UI preference；`meta.isUserInteraction` 是用户完成动作的权威事实，具体分隔条归属优先使用面板库公开 WAI-ARIA Separator 的焦点身份，焦点不可用时才比较“上一次完成布局 → 本次完成布局”的外侧 Panel 主变化量。中区触底后相邻外侧 Panel 可以同时变化，不得因此丢弃用户偏好；折叠到 0 只改变呈现，不写入像素偏好。异步 sidebar VM 到达后必须 hydrate Provider 初始宽度，不能让首次渲染的 440px fallback 覆盖已持久化值。
- 左侧导航可由用户拖到 176px 的紧凑可读宽度；导航项与工作空间标题保持单行、超长名称在内容区截断，操作图标和点击区域不缩小，继续可通过独立按钮完全折叠。
- 普通窗口缩放的 resize owner 只能由 canonical 输入决定，不能读取右栏当前实际宽度形成反馈环。左栏始终保持像素宽度；右栏可见且当前总宽不足以同时容纳“左栏偏好 + 中区下限 + 右栏偏好”时，中区保持像素下限、右栏使用相对行为吸收窗口增减，直到重新达到右栏偏好；容纳完整偏好后切回“右栏像素、中区相对”。owner 只在上述阈值跨越时改变，不随每个像素提交 React state。页面 profile、左右栏有效显隐或偏好变化后，必须在面板库完成本轮约束注册后调用官方 Group `setLayout()`，以一个 canonical 目标同时收敛三栏；禁止在目标求解前分别向左右 Panel 发送 `collapse/expand/resize`，否则相邻 pivot 会在旧页面约束下牺牲另一侧。若 `setLayout()` 返回的 applied layout 仍把目标可见外栏保留为 0，说明 Panel 折叠身份尚未释放；同一离散事务允许对该 Panel 调用官方 `expand()`，并对原目标有界重试一次，不得循环或把重试结果写成新偏好。右侧分隔条始终可在配置的 288–1440px 范围内直接调整，完成后才写入偏好。
- 真实客户端布局问题使用会话 Shell 内的有界诊断时间线排查。诊断默认关闭，沿用现有 DevTools 本地开关约定：执行 `localStorage.setItem('goldBand.debug.workspaceLayout', '1'); location.reload();` 后启用，排查结束执行 `localStorage.removeItem('goldBand.debug.workspaceLayout'); location.reload();` 关闭。关闭时不注册 Group 连续 layout 回调、诊断快捷键，不读取 Panel size 或调度诊断帧；启用时最多保留最近 2,000 条结构化记录，覆盖页面、viewport、布局 profile、用户开关意图、自动折叠输入/输出、Panel 实际像素宽度、Panel 同步动作、Group 连续 layout、完成事件及其前一布局、焦点 separator、用户 resize 归属、首次 applied layout、显式展开项与最终 applied layout。记录只驻留内存，不包含会话正文、资源标题或文件路径，不逐条写控制台或磁盘；用户复现后按 `Ctrl+Alt+Shift+L` 一次导出 JSON 到剪贴板，同时输出单条可复制控制台摘要。诊断读取实际 Panel size 只允许发生在页面/展示离散提交和同步动作中，ResizeObserver 与 Group 连续变化热路径只能记录已有数值，不得额外强制布局或触发 React state。
- Tauri 原生最小宽度随当前页面 profile 动态切换：会话与设置为 480px、上下文卡片为 520px、工作流画布为 640px。窗口约束同步必须按数值去重，相同宽高不得重复调用宿主 mutation；窗口最大化期间只记录最新待应用约束，不调用 `setMinSize/setSize`，避免 Windows TAO 在重检尺寸边界时退出最大化。用户恢复普通窗口后再应用最新约束，若恢复尺寸低于新页面下限才扩到下限。进入更窄页面只放宽约束，不主动缩小用户窗口；`shellMinWidth/shellMinHeight` 保护应用 chrome 的绝对下限；初始隐藏窗口必须完成当前页面约束和主题背景同步后再显示，页面快速切换与恢复后的 pending 应用必须进入同一串行生命周期，最终页面配置最后生效。
- 紧凑 Sheet 复用 shadcn/Radix 的焦点陷阱与键盘可访问性，但打开后的初始焦点落在对话区容器，不落到唯一的关闭按钮。关闭按钮只使用 `focus-visible` 绘制键盘焦点环，不得使用 `data-state=open` 背景或普通 `focus` 环把鼠标打开后的自动焦点误呈现为选中态；键盘 Tab 进入关闭按钮时仍必须有可见焦点。
- Windows 无边框窗口使用 WebView2 composition 路径承载连续边缘 resize：Tauri Window 在 Windows 平台启用透明控制器，但 `html/body/#root` 与应用壳始终绘制不透明主题 surface，不向用户呈现实际透明效果。宿主 Window 背景随主题同步，WebView 层保持 composition 模式，禁止为了遮盖黑带重新设为不透明控制器。
- 主窗口初始隐藏；bootstrap、主题和宿主背景准备完成后由前端显式显示，避免 transparent composition 窗口在首帧 CSS 尚未就绪时闪现。窗口 decorations、title bar style 与 native shadow 的生命周期只由 Rust/Tauri 宿主配置管理，Web 层只同步主题背景并显示窗口，不具备运行时修改 decorations 的权限。Windows 自定义无边框模式按系统能力控制 native shadow：Win11 及更高版本开启，由 DWM 提供系统圆角与边界；Win10 关闭，避免 TAO `top=0 / left-right-bottom=frame_thickness` 的非对称 non-client frame，仅由应用内嵌边界补足可感知轮廓。关闭 Win10 shadow 不移除 `RESIZABLE/WS_SIZEBOX`，TAO 继续通过完整客户区边缘 hit-test 提供四边缩放；macOS 恢复 native decorations、shadow 与 traffic lights。
- 上下文管理的角色卡片网格按列表容器实际宽度决定列数，而不是只依赖整窗 viewport：窄容器单列、中等容器双列、足够宽时三列。卡片底部操作区允许整组换行，系统显示缩放、字体增大或翻译文案变长时不得越过卡片边界或覆盖相邻卡片。

### 6.4 运行态生命周期
- Round 详情页的“继续运行”只在当前 run / round / node 处于可恢复暂停态时出现；成功、失败或 killed 的终局 round 不展示该入口。
- 顶部“继续运行”是 workflow runtime 控制动作，不等同于 ACP 会话抽屉中的自由输入 composer；它会向当前 ACP session 自动发送本地化 `继续 / Continue`。
- 根 ACP 会话属于中间主工作区；Agent 分支会话属于右侧辅助工作区的只读资源。关闭 Agent Tab 或右侧工作区不取消根 runtime，重新打开同一 branch 时从有限 LRU 和 branch 查询恢复语义窗口、滚动位置与实时状态。
- 主会话和 Agent 会话中的子 Agent 链接使用与 Assistant 消息相同的 Agent 头像 preference；嵌套层级继承当前执行者身份，不使用独立硬编码 Bot 圆标。未配置自定义头像时继续由共享头像组件提供 Bot fallback，attention 仅叠加外圈状态，不替换头像。
- 根会话耗时使用根 ACP attempt 的墙钟耗时，不把并行或串行 Agent 分支耗时求和。Agent 会话耗时只使用该 Agent execution 自身的开始与最后更新时间。
- 根会话 Token 使用 ACP provider 对整个根 attempt 返回的累计 usage；Claude Agent ACP 的该累计值包含同一 turn 内嵌套 Agent 的模型调用。Gold Band 不从各分支 transcript 重复求和，Agent 分支在 provider 未提供独立 usage 时不伪造 Token 数字。
- Todo、直属子 Agent 与活动统计按当前 branch ID 投影；Agent 会话标题区和任务列表只展示该 Agent 自身的数据，不能平铺到根会话或兄弟 Agent。
- 主窗口关闭与应用退出是两个不同的生命周期动作。macOS 红色关闭只在前端冲刷编辑队列后销毁 `main` WebViewWindow，Rust runtime、ACP/MCP 连接和系统 Dock 应用继续存活；Windows/Linux 关闭主窗口则进入应用退出事务。Cmd+Q、系统菜单退出、updater 退出和无窗口退出统一由 Rust `DesktopLifecycleCoordinator` 协调，不能由前端直接销毁进程。
- 应用退出状态固定为 `Running / ClosingMainWindow / AwaitingFrontend / Cleaning / ReadyToExit`。存在主窗口时，宿主发送带 `requestId` 的退出请求，前端复用同一保存事务并通过 `resolve_app_exit` 返回 `Proceed / Cancel`；监听失败或 15 秒内未响应必须取消退出，不能静默丢弃未保存内容。后端清理全局上限为 15 秒，超时后强制终止受管进程组，最终只调用一次 `app.exit(0)`。
- macOS `RunEvent::Reopen` 和通知点击统一调用 `ensure_main_window()`：已有 `main` 时 show、unminimize、focus；窗口已销毁时使用 `WebviewWindowBuilder::from_config` 从权威 Tauri window config 重建，并继续复用 bootstrap 完成后的显示流程。此处 Dock 指 macOS 系统 Dock，与右侧 `RightWorkspaceDock` 无关。
- 会话侧栏 Direct 任务在活动态保持 Agent 图标的呼吸效果，不增加旋转环或终局状态点。
- Direct turn 到达终局且对应会话尚未成功呈现时，在 Agent 图标右上角叠加未查看结果点：完成使用 `gold-success`，停止/取消使用 `gold-warning`，失败/异常使用 `gold-danger`。该点表达“未查看的终局事件”，不是 run 的权威状态；不得因查看而修改 `run.status`、`outcome` 或 ACP lifecycle。
- 侧栏、置顶、搜索结果、系统通知和 deep link 必须汇入同一会话呈现事务。只有目标 project/task/run 已成功呈现，且待确认 `eventId` 仍是该任务最新终局事件时才持久化已查看；迟到的旧确认不得清除更新事件。仅导航开始、加载失败或打开了同任务的其他 run 时不得消失。

---

## 7. Tauri 2.x MVP 对应实现

- 应用壳使用 Theme Contract v2 的 `shell`、`titlebar`、`sidebar`、`navigation-item` 等稳定 role；业务组件不得读取具体 `themeId` 或通过页面级 class 复制主题状态。
- 跨页面导航图标通过有限语义槽接入 `ThemeIcon`。主题资源可用时按 scheme descriptor 渲染，缺失或加载失败时保持原 Lucide 图标、可访问名称和按钮尺寸不变。
- 主壳标记稳定 `app` wallpaper surface；壁纸只为当前可见 surface 预加载，使用安全底色和 overlay 保证窗口 resize 与正文可读性。资源失败只回退当前 surface，不新增 Tauri IPC 或文件权限。
- 主题更新只投影根 CSS variables、resource locator 和低频图标 descriptor，不重新请求会话、卸载右侧资源或扩大重型 React 子树的订阅范围。

MVP 中应用壳由 `web/src/components/Shell.tsx` 实现：
- 左侧固定展示 Gold Band、workspace 路径/切换入口、任务编排、Agent 管理、上下文管理、模型管理占位、设置。
- 右侧由 React 状态维护当前一级模块内容；任务编排继续使用递进式页面栈，Agent 管理和上下文管理为独立管理页。
- 工作空间选择页由 `web/src/pages/WorkspaceSelectPage.tsx` 实现，展示原生选择按钮和最近 workspace 列表；主视觉入口使用与侧边栏一致的 canonical Gold Band logo。该页面只作为工作台模式覆盖层渲染，不进入会话模式页面栈。
- Tauri commands `choose_workspace` / `select_recent_workspace` 负责切换 workspace，并将最近列表写入用户级配置；`remove_recent_workspace` 只移除最近列表项并返回刷新后的 bootstrap。
- `choose_workspace` 与会话侧 `add_conversation_workspace` 必须统一复用非阻塞目录选择封装，避免同类原生弹窗行为分叉。
- 桌面端必须为 `choose_workspace` / `select_recent_workspace` 记录结构化系统日志，至少覆盖“打开目录选择器”“用户取消”“目录返回”“切换完成”四个阶段，便于排查 macOS 原生目录选择器卡死或切换后状态未刷新问题。
- Tauri window 默认逻辑尺寸为 1280×720，并使用宿主原生 `center` 定位；`src-tauri/tauri.conf.json` 只管理默认尺寸、初始位置与 chrome 属性，页面最小尺寸唯一来自 `configs/app-config.toml` 的 `workspaceLayout`。渠道构建 overlay 只能完整继承基础 window config 并覆盖渠道属性，不能独立硬编码宽高、位置或最小尺寸。
- 应用壳不提供命令输入、slash command、terminal input 或 chat input。
- 2026-05-03 起应用壳使用 Tailwind CSS v4 + shadcn/ui Button、Tooltip、Separator 等现成组件重构；侧边栏 IA、workspace 切换入口和右侧页面栈行为不变。
- 2026-06-08 起新旧 UI 共用 `web/src/components/AppTitleBar.tsx` 共享顶栏；Tauri 基础配置关闭 WebView file-drop，避免与 composer 附件拖拽上传争抢文件 drop。
- 2026-06-19 起桌面 bootstrap 暴露 `platform` 作为前端唯一平台事实源：macOS 启用原生 traffic lights + overlay 标题栏并隐藏系统标题文本；Windows/Linux 继续关闭整窗 decorations，由共享顶栏右侧自定义窗口控制接管最小化、最大化/还原和关闭。
- 2026-06-29 起共享顶栏拖拽命中升级为“整条顶栏默认可拖，交互控件显式 no-drag”；早期根壳 overlay 外轮廓方案已由 2026-07-23 的 native compositor 方案替换。
- 2026-07-22 起共享顶栏隐藏 Workbench / Conversation toggle，根路径统一进入会话主页；旧工作台仅保留显式 deep link，供观察期继续验证而不暴露产品入口。
- 2026-07-23 起共享顶栏取消 Windows/Linux 窗口控制组的末尾 gutter，并固定控制组及每个按钮的 intrinsic width；删除 Web 层 `1040x680` 最小尺寸镜像，让真实 viewport 持续驱动响应式布局。角色管理卡片网格改为容器查询升列，卡片操作区支持换行，以覆盖 Win10 1080p 显示缩放和最小窗口场景。
- 2026-07-23 Windows 边缘缩放修正：采用 Tauri/WebView2 transparent composition workaround，并复用 AionUi 的“窗口初始隐藏、首帧完成后显示”启动策略；保留 native shadow 以获得 Win11 DWM 原生圆角和边框，删除 Win11 根壳的整窗 border、CSS 圆角与高层级伪元素。
- 2026-07-28 Windows 10 窗口边界兼容：不回退 opaque WebView2 controller，也不恢复跨版本通用 CSS 圆角；桌面 bootstrap 基于 `RtlGetVersion` 的真实系统 build 下发 `native-compositor` / `app-outline` frame policy 与 `nativeShadow`。Win10 关闭 TAO undecorated shadow，使用对比度更明确的主题化 1px 内侧边界与四边一致的柔和内阴影，消除三侧 native frame 黑线的同时保留清晰窗口层次；Win11 继续使用 DWM 原生圆角与 shadow。
- 2026-07-28 Windows 10 最大化拖拽修正：删除共享顶栏重复的 React `startDragging()` 与手动双击切换，鼠标和双击只通过 Tauri `data-tauri-drag-region` 进入一次原生拖拽/最大化流程；保留 WebView2 app-region 作为触摸输入补充，避免最大化窗口拖拽还原时连续发送两次 caption drag 导致窗口移出工作区。
- 2026-08-02：会话模式应用壳升级为 `WorkspaceShell` 三段式布局。右侧 `RightWorkspaceDock` 使用通用多 Tab 资源模型和同源紧凑 Sheet；Agent 分支是首个只读资源。页面中间最小宽度只由布局 profile 约束，不复制为 Web 根最小宽度。
- 2026-08-02：`WorkspaceShell` 补齐统一 Tooltip 上下文边界，覆盖中间会话、右侧 Agent Dock 与紧凑 Sheet。回归测试必须从工作区资源模型实际打开 Agent Tab，并验证 Agent 内容中的 Tooltip 可直接挂载，不再出现“加载中”后因 Provider 缺失导致的白屏。
- 2026-08-02：Agent Tab 初始化改为 branch-scoped readiness 与有限 Session VM LRU。实测历史大分支的后端查询为几十至一百余毫秒；此前分钟级等待来自前端把已返回的 `interrupted` canonical 分支误判为未就绪后执行整段退避重试，并非文件体积。调试时可设置 `localStorage.setItem("goldBand.debug.acpTiming", "1")`，以前端同一 `traceId` 串联 effect、request 与 Rust command/view-model 分段日志；验证后删除该 key，常规运行不输出逐请求性能日志。
- 2026-08-02：右侧 Tab 条改为基于真实横向溢出按需显示紧凑 Tab 菜单；会话中间区最小宽度由 420px 校准为 360px，其余卡片、画布和设置 profile 不变。该阶段采用的 4px WebKit 专用横向滚动条已于 2026-08-16 被应用级互斥渲染策略替换。
- 2026-08-16：原生滚动条按浏览器能力统一选择标准属性或旧 WebKit fallback；同一应用运行环境内不再允许会话、右侧 Tab 或其他局部容器通过 `auto` 覆盖切换渲染器。
- 2026-08-02：共享顶栏品牌移至左侧安全区起点，其后排列左右工作区开关。右侧 `requestedOpen` 与 Tab 集合解耦，支持无资源空白入口页；Tab 仅运行期记忆。宽度持久化改用 resizable group 的用户完成事件，并支持异步 preference hydrate，修复重启后总是回到 440px 的问题。
- 2026-08-02：共享顶栏左右工作区开关统一收敛为 28px 按钮和 14px 图标；左栏开关留在品牌后，右栏开关移至尾部操作区并位于 Windows/Linux 窗口控制之前。macOS 继续在左侧保留 traffic lights 安全区，尾部入口使用 flex 流定位，不维护平台绝对坐标。
- 2026-08-02：右侧辅助区正式收敛为会话工作区。入口只在快速对话与会话详情展示；draft 与 conversation-run 使用独立 scope，轻量工作区状态进入 24 项 LRU，ACP Session/events/view state 合并为 12 项原子重资源 LRU，宽度继续作为全局 UI preference。
- 2026-08-02：会话辅助入口完成资源化迁移。查看/编辑/修复工作流、系统提示和原始帧改为 locator-only 右侧 Tab；主 ACP 画布不再因查看 raw frame 被替换。工作流编辑草稿与轻量 Tab 状态分离，收起或切换后可恢复，关闭脏 Tab 需确认。
- 2026-08-02：修复 Web reveal 流程无条件关闭 decorations、覆盖 macOS 原生标题栏的问题。窗口 chrome 状态收归 Rust/Tauri 单一所有者，WebView 移除 `allow-set-decorations` 权限；macOS 保持“traffic lights 安全区 → 品牌 → 左栏开关 → 弹性空白 → 右栏开关”的共享顶栏顺序，Windows/Linux 自绘窗口按钮不变。
- 2026-08-02：修复三段式响应式布局不可达。Tauri 原生最小宽度由旧的 1040px 收敛为布局 profile 最大值 640px；自动折叠阈值和右栏动态上限改用用户当前左栏宽度，窗口缩小时可以真实经历“右栏压缩 → 左栏隐藏 → 右栏隐藏”，紧凑模式继续复用同一 Tab state 和 `RightWorkspaceDock` Sheet。
- 2026-08-02：Dock/Sheet 模式切换时，Sheet 可保留 Radix 退出动画外壳，但 compact 状态结束后必须立即卸载内部 `RightWorkspaceDock`；禁止退出动画期间同时挂载两套 Agent 内容、重复建立实时订阅。
- 2026-08-02：修复渠道 Tauri overlay 覆盖基础窗口最小宽度的问题。`scripts/channel-config.mjs` 不再维护第二份 1040px 等窗口参数，而是完整继承 `src-tauri/tauri.conf.json` 的主窗口配置，仅替换渠道标题；渠道契约测试比较最终 overlay 与基础窗口配置，确保真实客户端和浏览器响应式验收使用同一约束。
- 2026-08-02：补齐无右栏时的左栏紧凑策略。布局 profile 将中间内容硬下限与自动折叠舒适宽度分开建模；会话分别为 360px/420px。右栏关闭时按舒适宽度收起左栏，避免折叠阈值 616px 低于原生 640px 最小宽度而永远不可达；右栏打开时仍按硬下限计算，保持“右栏先压到最小值，再收左栏”的顺序。
- 2026-08-02：窗口连续缩放热路径移除每像素 React 双提交。原生窗口与 Resizable flex 面板继续逐帧跟随指针；`previousWidth` 下沉到 ref，React 只接收跨越折叠临界点后的 `{left,right}`，右栏最大值改由 Panel 原生 min/max 与中间 min 联合约束。左栏逐帧 `onResize` 持久化定时器被删除，左右宽度只在用户释放分隔线后的 `onLayoutChanged` 中保存；会话导航和右侧 Dock 建立 memo 边界，避免无关壳层提交重建长列表或 Agent 视口。
- 2026-08-03：修复最大化窗口切换页面时被还原。页面约束同步增加 applied/pending 状态：同约束切换不触发宿主 API；不同约束在最大化期间延迟，恢复普通窗口后由 resize 生命周期应用最新值。禁止使用“先退出再重新最大化”的闪动补偿。
- 2026-08-04：会话文件入口统一资源化。用户消息附件、Agent artifact、prompt turn 历史原文与 diff 都打开右侧工作区 Tab；旧 composer 上方资产聚合栏和会话内预览 modal 已删除。右侧 Dock/Sheet 继续共享同一资源状态，新增历史资源一律只读，不改变 live workspace 文件的编辑与自动保存语义。
- 2026-08-04：Conversation 主页面与 session switch payload 删除仅服务旧聚合栏的 `artifacts/attachments` 数组；Round/节点排障入口及按名读取接口保留。会话首屏只携带 change set summary 指针，文件清单和正文分别在卡片/Tab 打开时懒加载。
- 2026-08-09：桌面生命周期收归 Rust `DesktopLifecycleCoordinator`。macOS 红色关闭只销毁主窗口，Dock 重开可显示或按配置重建；Windows/Linux 关闭、Cmd+Q、菜单退出和 updater 退出统一执行“前端保存握手 → 后端有界清理 → 单次退出”。ACP、MCP、Agent doctor 与登录 Shell 探测统一由 `command-group` 受管进程组拥有，正常退出不再散落调用 `taskkill`、单进程 `kill()` 或手写 Unix PID kill。
- 2026-08-09：macOS 发布采用单一可选凭证流水线。基础 bundle 配置使用 ad-hoc identity `-`；无 Apple 凭证时仍由 GitHub macOS runner 生成 arm64/x64 DMG，并对产出的 `.app` 执行 `codesign --verify --deep --strict`。凭证部分配置时立即失败，配置完整时由同一 `tauri-action` 接收证书、Developer ID、Apple ID、app-specific password 与 Team ID 完成签名和公证。产品下载页和应用内不增加未公证分支，产物名不增加 unsigned 后缀。
- 2026-08-17：Apple Developer Program 凭证未就绪期间，仓库提供外部 macOS 安装脚本；它不是第二套产品 bundle。两条 release workflow 把 release commit 中的安装脚本作为同名 Release 资产发布，并为各平台发布资产生成同名 `.sha256`；用户通过 latest 资产 URL 直接执行脚本，无需 clone 仓库。脚本默认解析 latest，只接受有 sidecar 的新 DMG，并依次验证摘要、磁盘映像、固定 bundle identity 与 codesign；在 `/Applications` 同卷暂存并支持旧 App 恢复，校验通过后才移除 quarantine。历史 Release 不增加兼容 fallback。
- 2026-08-15：左右栏宽度恢复统一以全局 `sidebar.width` / `rightWorkspace.width` 为唯一持久化事实源。`react-resizable-panels` 的 `defaultSize` 只负责 Panel 首次注册；异步 preference hydrate 到达且对应分隔条尚未被用户操作时，通过 Group canonical layout 事务应用有界像素宽度。用户操作后由本地偏好投影立即接管，并仅在对应分隔条的完成事件持久化一次。工作空间、运行目录及其他右侧资源只能按当前实际宽度响应，禁止请求推荐宽度或改写外层右栏偏好。
- 2026-08-16：真实客户端日志确认两个独立实现缺口：固定两侧像素行为会把受窗口约束后的临时右栏宽度当成后续基线，重新拉宽后不再追赶 `rightWorkspace.width`；从 640px 中区下限页面返回 360px 会话页时，独立右 Panel 展开又会在旧约束过渡期把左栏折叠。最终改为 canonical 阈值驱动的离散 resize owner，并用官方 Group `setLayout()` 在约束提交后原子收敛三栏。`requestedOpen`、打开 revision 与运行期宽度投影继续归属会话 Shell Store，使快速对话与会话详情共享右栏开关和宽度记忆；页面能力和资源 scope 只决定有效呈现与 Tab 集合，不冒充用户关闭动作。
- 2026-08-16：由于浏览器宽度模拟未复现 Windows Tauri 客户端反馈，增加三栏布局有界内存诊断。诊断统一记录 canonical 意图、自动折叠决策、Panel 物理状态与面板库 layout 时间线，并提供 `Ctrl+Alt+Shift+L` 剪贴板导出；仅在刷新前设置 `goldBand.debug.workspaceLayout=1` 时启用，默认关闭且不注册诊断热路径；不再根据截图继续增加未经真实序列验证的布局补丁。
- 2026-08-16：客户端诊断确认用户把右栏从受约束的 733px 拖至 288px 后，`onLayoutChanged` 已报告 `isUserInteraction=true`，但 Separator pointer intent 未透传，导致 `rightWorkspace.width` 仍停在 772px；返回会话页时原子布局按旧偏好恢复为 733px。宽度写回改为比较相邻两次完成布局中左右外侧 Panel 的差量来识别用户操作目标，移除 pointer/keyboard intent 旁路状态，并统一使用 Panel Group 的可分配宽度换算布局百分比，避免把包含 separator 的 Shell 宽度混入同一坐标系；诊断同时记录前一布局和最终归属。
- 2026-08-16：后续客户端日志证明“只有一个外侧 Panel 变化”的假设仍不成立：左 separator 从 176px 拖到 303px 时，中区先到 360px 下限，右栏继而从 674px 被挤到 615px，导致两侧差量同时变化且左偏好未写回；返回会话页时 `setLayout()` 的 canonical 目标为左176/中428/右674，但 applied layout 因左 Panel 保留 collapsed identity 变成左0/中640/右638。用户 resize 归属改用公开 Separator 焦点身份并以主变化量兜底；Group 同步根据 applied layout 识别目标可见但仍为 0 的 Panel，显式展开后对原目标有界重试一次。
- 2026-08-11：中间工作区顶边、左边与右侧工作区 separator 统一使用不透明语义色 `workspace-divider`。该 token 由当前主题的 `sidebar-border` 与 `gold-workspace` 预混合，禁止在不同底色上分别叠加半透明 `sidebar-border/70`，避免高 DPI 下横竖边线交点出现色阶断层。Dock 展示时，中间 Panel 与右侧 Panel 必须各自绘制同为 1 CSS px 的顶边，使边界连续横跨两个区域，separator 从顶边下方形成 T 形交点；separator 的 1px 布局宽度、4px 命中区和 hover 状态保持不变。
- 2026-08-14：应用壳主题材质从手写 Glass 专用选择器迁到 Theme SDK 编译的包级 recipe CSS。Shell、标题栏、侧栏、工作区、Composer 与共享控件仍只暴露稳定 `data-theme-role`；新增合规主题包通过 DTCG token、封闭 recipe 和构建 Catalog 接入，不修改壳层 DOM、导航状态或 React 生命周期。
- 2026-08-16：主题 recipe 由各主题明确声明 role 视觉，并统一作为 CSS `components` 层默认值；组件显式变体可以覆盖背景、前景、边框色、focus ring、阴影、动效和几何，不允许高优先级 recipe 抹掉 `border-0`、单边分隔、圆形、pill、joined-control 圆角或定向阴影。Gold Band 与技术中性主题中的 Shell、共享顶栏、侧栏、右侧工作区、编辑器根面和源码管理根面不拥有完整 perimeter，统一声明 `borderWidth:none + radius:none`；后续主题仍可选择其他 role 形状。工作区顶边、主区圆角、侧栏/右栏 separator 与 Sheet 靠内容侧边线继续由布局 owner 单独绘制。共享顶栏本身不显示下边框，也不得形成四边圆角卡片。
- 2026-08-16：Gold Band 浅色主题的共享顶栏与侧栏统一使用 `#fafafa` sidebar surface，消除顶栏白色条带与导航区之间的色阶断层；深色 Gold Band 和技术中性主题维持各自已有声明。该视觉由主题 token/recipe 投影，禁止在共享 `AppTitleBar` 中按主题特判。
- 2026-08-15：共享顶栏从 44px 收紧为 36px，品牌图标容器同步收紧为 24×36px，应用标题由 14px 提升为 16px，并使用独立 700 字重而不是全局映射为 520 的 `font-bold`；帮助入口为 28px 高，左右栏开关保持 28px，Windows/Linux 窗口控制保留既有横向点击宽度并填满顶栏高度。改动只调整共享 `AppTitleBar` 的静态布局 token，不改变拖拽区、平台控制策略和窗口生命周期。
- 2026-08-20：用户反馈紧凑顶栏中的品牌标识视觉权重偏低。共享 `AppTitleBar` 的品牌图标容器由 24×36px 提升为 28×36px，标题、操作按钮、拖拽区和平台窗口控制策略保持不变；尺寸作为单一 layout token，由所有页面共同消费。

---

## 8. 一句话总结

> 应用壳稳定组织左侧导航、中间主任务和右侧辅助资源；页面下钻不再与辅助资源工作区混为同一个“右侧区域”。
