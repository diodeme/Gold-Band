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

### 诊断领域

- 数据：`McpServerState`、`McpServerHealthResult`、工具发现结果。
- 生命周期：用户手动检查或后台诊断时刷新，可随网络、认证和服务进程变化。
- 接口：`check_health`、`list_tools` 等显式诊断入口。
- 约束：诊断失败不能阻塞会话壳展示；具体 MCP 是否可用由 ACP/Agent 在建立会话时按协议处理并产生结构化诊断。

## 接口设计

`McpManager::configured_acp_mcp_servers()` 是纯配置解析边界：

1. 读取 settings。
2. 排除 `enabled = false` 的配置。
3. 将启用配置序列化为 ACP schema。
4. 不读取健康缓存，不执行探活。

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

该契约只收口既有 lifecycle、session query 和 timeline 的消费边界，不新增延时、轮询、缓存或平行状态机。

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
- 冷 adapter initialize 可单独观测，不再与 MCP 配置解析混为一段不可解释等待。

## 验收与回归

- 配置一个启用但命令不存在的 stdio MCP，配置序列化仍应成功并包含该服务。
- 停用的 MCP 不得传给 ACP。
- 当前运行的新会话在初始 fetch 进行中仍返回 `initializing`。
- 当前新会话即使 runtime 先于 timeline 查询收敛而终止，也保持完整聊天壳、品牌等待态和锁定 composer，直到首条 timeline item 到达。
- sessionId 未建立时标题栏不展示缺失占位；首条 timeline item 到达后优先展示消息而非等待态。
- 非当前会话切换仍返回 `loading`。
- 初始化错误和中断状态必须覆盖 `initializing`。
