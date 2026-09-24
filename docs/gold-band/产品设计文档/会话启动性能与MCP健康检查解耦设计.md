# 会话启动性能与 MCP 健康检查解耦设计

## 问题定义

新建会话的 Worker 构建阶段曾同步检查每个启用的 MCP 服务。`McpManager` 每次新建实例时健康缓存为空，因此远程 HTTP/SSE 握手和本地 stdio 启动会重复发生在会话关键路径，导致进入会话前额外等待约 4～5 秒。

该问题属于生命周期边界设计缺陷：MCP 配置属于会话输入，MCP 健康状态属于可变诊断数据，两者不应共享同一个同步读取接口。

## 领域与数据结构

### 配置领域

- 数据源：`SettingsConfig.context_servers`。
- 生命周期：由用户添加、编辑、启用或停用时变更。
- 会话接口：只读取已启用项并转换为 ACP `mcpServers`。
- 约束：读取配置不得发起网络请求、不得启动子进程、不得根据瞬时健康状态过滤配置。
- 内置服务：编译期 definition 在应用启动时幂等 reconcile 到 `settings.json`；保留用户 `enabled` 选择，定义未变化时不写盘。内置项复用标准 managed stdio 卡片，不建立第二种“会话托管”类型。

### 诊断领域

- 数据：`McpServerDiagnosticState`、`McpServerHealthResult`、工具发现结果。
- 生命周期：用户保存/启用配置后的显式检查或手动诊断时刷新；应用启动和普通列表刷新不批量探活。
- 接口：`check_health`、`list_tools` 等显式诊断入口。
- 语义：卡片展示“尚未检测 / 正在检测 / 最近检测通过 / 最近检测失败 / 需要授权”，不展示“正在运行”；诊断进程结束后结果仍可显示。
- 约束：诊断失败不能阻塞会话壳展示；具体 MCP 是否可用由 ACP/Agent 在建立会话时按协议处理并产生结构化诊断。stdio 诊断必须执行 `initialize → notifications/initialized → tools/list → 关闭进程树`。

### 可执行会话快照领域

- 数据源：本次 invocation 的已启用配置，以及当前 canonical project/task/runtime context。
- 生命周期：每次新建、加载、恢复或人工续聊前重新生成，只存在于本次会话准备链路，不回写 settings。
- 内置共享记忆：持久 definition 只有 `current_exe + --gold-band-memory-mcp`；会话快照追加 repo root、data root、project id、task id 与语言绑定。无绑定实例仍可协议探活，业务工具调用返回 `memory.context-required`。
- 所有权：Gold Band Client 把 `mcpServers` 交给 ACP Agent，由 Agent 在 `session/new/load/resume` 时连接 stdio；Gold Band 不再常驻同一 MCP 的第二份连接。

## 接口设计

`McpManager::configured_acp_mcp_servers()` 是纯配置解析边界：

1. 读取 settings。
2. 排除 `enabled = false` 的配置。
3. 将启用配置序列化为 ACP schema。
4. 不读取健康缓存，不执行探活。

随后统一 prompt 准备边界把内置记忆基础定义解析为会话快照，并仅在该定义启用时注入记忆 system rules 与本轮数据投影。Direct、Workflow、AI-DYNAMIC 和人工续聊必须走同一准备边界，不能在 Tauri command 层重新读取未绑定配置覆盖快照。

原先同时负责“配置解析 + 健康检查 + 健康过滤”的启动接口被删除，不保留兼容入口。

## 会话初始化状态

会话展示状态拆分为：

- `initializing`：当前运行节点正在创建首个 ACP 会话，但 sessionId/元数据尚未到达。展示完整聊天壳，状态显示在 composer 内，发送和依赖元数据的控制项保持不可用。
- `loading`：切换或加载一个既有目标会话，目标数据尚未返回。允许使用全屏加载状态。
- `available`：已有基础会话或事件壳。
- `interrupted` / `error`：初始化在建立会话前终止或失败，优先于初始化展示。
- `missing`：加载结束且运行时也不再负责创建会话。

只有当前运行节点拥有 `initializing` 展示权；历史会话切换继续使用 `loading`，避免把不同生命周期合并成一个模糊状态。

### 新会话首屏投影契约

新会话的页面壳、timeline 和 composer 必须由同一初始化归属共同投影，不能分别根据瞬时 sessionId、runtime active 或事件数量决定是否就绪：

1. 用户提交后，当前节点立即进入完整会话页；`initializing` 不得复用历史内容的全屏 `loading` surface。
2. 信息栏直接展示 canonical composer phase：worktree/workspace 准备、Agent 调起或处理中。
3. 首条可展示 timeline item 到达前，消息区展示通用品牌加载组件；首条用户消息落盘并进入 timeline 后立即切换为正常消息列表。
4. 初始化归属尚未交出且 timeline 为空时，composer 保持运行态占位并禁止提交，不能因 runtime 已快速终止而提前恢复。
5. sessionId 尚未建立属于正常初始化事实，标题栏不展示“无 session id”占位。
6. “暂无 ACP 事件”仅用于非初始化归属、初始查询已完成、runtime 不活跃且确认为空的既有会话。
7. 品牌加载组件区分页面背景 surface 与消息区透明 surface；新会话首条消息前的内嵌 Logo 不得绘制独立背景块，浅色与深色主题保持相同层级关系。
8. 新会话初始查询重试耗尽时，若 runtime 仍 active 且没有实际查询错误，说明 provider 只完成了元数据落盘、timeline 正文尚未就绪，必须继续留在 `initializing` 并等待 live event 或后续刷新收敛；不得把“尚未就绪”投影为 `error`。只有实际请求失败，或 runtime 已不再负责创建会话，才允许进入 `error` / `missing`。该判定必须读取收尾时刻的最新 runtime active 事实，不能使用 effect 启动时捕获的旧闭包值。

该契约只收口既有 lifecycle、session query 和 timeline 的消费边界，不新增延时、轮询、缓存或平行状态机。

## 生命周期与正文投影边界

ACP 停止完成后，控制面只发送带单调 `revision` 与 `turnId` 的轻量 lifecycle patch；该 patch 不携带或触发完整 timeline 正文刷新。前端活动摘要使用同一 lifecycle 投影有效会话状态：`idle + latestTurnStatus != none + !stopping` 是权威终态，必须立即将当前 Activity 归档，即使已加载正文快照仍暂时为 `running`。`stopping` 与 `starting / accepted / running` 继续投影为活动态；没有 lifecycle 时才回退正文 snapshot。

本规则只改变展示投影，不回写 session canonical state、不触发 timeline 查询。前端以 lifecycle revision 合并迟到事件；本地下一轮 prompt 尚在接纳时保持运行投影，待同一 turn 的较新 lifecycle 到达后再收敛，避免上一轮终态遮蔽新一轮。

## 可观测性

统一使用 `gold_band::perf` tracing target 记录：

- Worker invocation 总构建耗时。
- MCP 配置解析耗时和服务数量。
- ACP adapter 解析/启动耗时及复用结果。
- ACP initialize 耗时及成功状态。
- `session/new` 耗时及 MCP 数量。
- 首次 ready session update 的端到端耗时。
- `create_conversation_run` 命令总耗时。
- 会话 worktree 准备总耗时，并以 `task_id`、`run_id` 和 worktree 路径关联运行上下文。
- Git worktree 创建的仓库锁等待、`git worktree add` 子进程和创建后校验耗时；子进程计时不得混入锁等待，已有 worktree 的幂等校验使用 `mode=existing` 单独标识。

日志不得包含 MCP 密钥、header 值、完整 prompt 或对客错误文案。

## 性能目标

- 新建后会话壳可见：目标小于 300ms，不等待 MCP 探活或 ACP 元数据。
- 已复用 adapter 的 session ready：通常约 1～2 秒，实际由 Agent 和网络决定。
- 会话启动阶段重复 MCP preflight：0 次。
- 应用启动阶段 MCP 握手：0 次；内置 definition 未变化时 settings 写入：0 次。
- 冷 adapter initialize 可单独观测，不再与 MCP 配置解析混为一段不可解释等待。

## 验收与回归

- 配置一个启用但命令不存在的 stdio MCP，配置序列化仍应成功并包含该服务。
- 停用的 MCP 不得传给 ACP。
- 停用共享记忆 MCP 时，不得注入记忆 rules 或 data block。
- 无绑定共享记忆 stdio 必须完成 initialize/initialized/tools-list；调用业务工具返回 `memory.context-required` 且不访问文件。
- 会话快照必须包含当前 task 绑定，settings 中的基础定义不得被该绑定污染。
- 页面刷新只读取配置与最近诊断结果，不触发 N 个 MCP 的批量握手。
- 当前运行的新会话在初始 fetch 进行中仍返回 `initializing`。
- 当前运行的新会话在初始 fetch 重试耗尽且无实际错误时仍返回 `initializing`；同一场景若存在实际查询错误，或 runtime 已停止，则分别返回 `error` / `missing`。
- 当前新会话即使 runtime 先于 timeline 查询收敛而终止，也保持完整聊天壳、品牌等待态和锁定 composer，直到首条 timeline item 到达。
- sessionId 未建立时标题栏不展示缺失占位；首条 timeline item 到达后优先展示消息而非等待态。
- 非当前会话切换仍返回 `loading`。
- 初始化错误和中断状态必须覆盖 `initializing`。
