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

## 可观测性

统一使用 `gold_band::perf` tracing target 记录：

- Worker invocation 总构建耗时。
- MCP 配置解析耗时和服务数量。
- ACP adapter 解析/启动耗时及复用结果。
- ACP initialize 耗时及成功状态。
- `session/new` 耗时及 MCP 数量。
- 首次 ready session update 的端到端耗时。
- `create_conversation_run` 命令总耗时。

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
- 非当前会话切换仍返回 `loading`。
- 初始化错误和中断状态必须覆盖 `initializing`。
