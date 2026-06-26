# Gold Band Runtime 概览

## 1. 核心对象
Runtime 当前围绕以下对象组织：
- task
- run
- round
- attempt
- node
- artifact
- worker reference
- dynamic run / dynamic node / dynamic group / dynamic proposal

## 2. 目录层级模型
当前推荐 4 层：

```text
preset -> task -> run -> round/attempt
```

## 3. 当前关键结论
- 顶层对象不是 conversation，而是 task
- session 复用不等于产物目录复用
- runtime 只信规范化产物，不信模型自己起的文件名
- 节点之间通过 runtime registry 传递产物，而不是直接猜路径
- `status` 与 `outcome` 必须分离：`status` 表示生命周期，`outcome` 表示终局结果
- `paused` 只属于 `status`，不属于 `outcome`
- `ai-dynamic` 内部状态归属外层节点 attempt，外层 round graph 只保留一个复合节点
- AI-DYNAMIC prompt 分层遵循：runtime 决定的身份、历史、路径、限制、可用资源和输出协议进入 system prompt，并通过 minijinja 模板渲染；requirement 与当前 goal 进入 user prompt
- 桌面端维护全局 `agent_diagnostics` 缓存并由后台 doctor 定期刷新；workflow 启动命令会要求普通 worker provider、AI-DYNAMIC bootstrap provider 与 dynamic strategy 的 available agents 均有可用 doctor 结果；普通 worker 启动前用当前缓存校验已配置的 model / permissionMode，AI-DYNAMIC schema、prompt 和 permission 校验也读取当前缓存，不在执行中同步 doctor provider capabilities。AI-DYNAMIC prompt 和 schema 中的模型枚举必须使用 ACP `configOptions[].options[].value`，展示名只作为辅助标签；当动态 proposal 需要输出模型但最新缓存没有该 provider 的模型目录时，runtime 在启动 provider session 前进入 `error-blocked`，不允许让 agent 猜模型值。
- runtime 自身的修复提示也统一放在 `src/prompts/<lang>/runtime/`，例如节点输出不满足 output DSL 时使用 `src/prompts/<lang>/runtime/invalid_output_repair.md` 生成隐藏 repair prompt

## 4. 子文档结构
- [控制层](control.md)
- [目录布局](layout.md)
- 状态文件规范
  - [task.json](state/task.json.md)
  - [run.json](state/run.json.md)
  - [round.json](state/round.json.md)
  - [node.json](state/node.json.md)

实现时建议先看：
1. [控制层](control.md) —— 状态机、continue/retry/kill、transition table
2. [run.json](state/run.json.md) —— run 级生命周期与终局状态
3. [round.json](state/round.json.md) —— round 级循环与挂起状态
4. [node.json](state/node.json.md) —— attempt 级状态与 outcome

## 5. 解析优先级

### workflow 解析优先级
建议统一为：
1. CLI 覆盖参数 `--workflow`
2. task 目录下的默认 workflow
3. 项目目录下的预设 workflow
4. 用户目录下的预设 workflow

### provider 解析优先级
建议统一为：
1. 当前节点显式声明的 `provider`
2. runtime 内部默认 provider（当前 MVP 为 `claude-code`）

### profile 解析优先级
建议统一为：
1. 项目目录下的 profile
2. 用户目录下的 profile

约束：
- 若 `worker` / `worker` 节点声明了 `profile`，runtime 在 `run start` 时必须解析成功，否则直接失败
- `validate_workflow()` 只负责结构校验；profile 是否存在属于 runtime resolution

## 6. 状态语义总说明
MVP 中建议统一遵循：

- `status`：生命周期状态，使用 `running | paused | completed`
- `outcome`：终局结果
  - `node`：`success | failure | invalid | killed | null`
  - `run / round`：`success | failure | killed | null`

统一约束：
- `status != completed` 时，`outcome = null`
- `status = completed` 时，`outcome` 必须为终局值
- `paused` 只表示 runtime 观测到的系统挂起态，不表示终局结果
- `failure` 表示目标未达成或执行失败
- `invalid` 表示结果不满足最小 contract

### AI-DYNAMIC 内部状态
`ai-dynamic` 节点进入执行后，会在当前外层 attempt 下创建 `dynamic/` 子目录：

```text
nodes/<outer-node>/attempt-001/dynamic/
  dynamic-run.json
  allowed-workflow-snapshots.json
  graph.json
  events.jsonl
  nodes/<internal-node>/attempt-001/
  groups/<group-id>.json
  proposals/<proposal-id>.json
```

内部状态由 runtime 派生：
- `DynamicRunState` 记录父 run / round / node / attempt、控制限制、allowed workflow snapshots 和当前 ready/running internal nodes。
- `DynamicNodeState` 记录 worker、workflow invocation、merge、acceptance 四类内部节点。
- `DynamicGroupState` 记录 fanout group 的父 group、root、terminal、merge、acceptance 节点；多层 fanout 使用 `parentGroupId` 串联父子关系，子 group closed 后以 acceptance 节点作为父 group 的 terminal boundary。
- `DynamicProposalState` 记录每个 `dynamic-node-completion` 的原文路径、解析结果、校验状态和 materialize 事件。

外层 `node.json` 只记录 `ai-dynamic` 节点的最终生命周期；内部 graph 不污染外层 round trace。

### AI-DYNAMIC / ACP 诊断事件

当 AI-DYNAMIC 内部节点启动缓慢时，runtime 会在 `dynamic/events.jsonl` 写入结构化诊断事件，用于拆分 Ready→Running 调度、线程启动、状态读取、worktree 创建、prompt 构建和 provider 调用耗时。关键事件包括 `dynamic_ready_refreshed`、`dynamic_launch_ready_begin/end`、`dynamic_launch_candidate`、`dynamic_node_marked_running`、`dynamic_thread_spawned`、`dynamic_job_state_loaded`、`dynamic_worker_workspace_begin/end`、`dynamic_worktree_git_lock_wait_begin/end`、`dynamic_worktree_add_begin/end`、`dynamic_worker_invocation_build_begin/end`、`dynamic_worker_invocation_build_step_begin/end` 和 `dynamic_worker_provider_begin/end`。所有 runtime/ACP JSONL append、roll 和同路径 timeline overwrite 都通过 storage 层按 normalized path 串行化，避免并行 fanout 节点或 ACP patch/compact 写入同一 JSONL 时发生行内容交错；该锁只覆盖同一路径文件 IO，不串行化 worker、worktree、prompt 构建或 provider 执行。

ACP attempt 会在 `acp.diagnostics.jsonl` 写入 adapter 复用/新建结果和 JSON-RPC timing，例如 `acp_adapter_resolved`、`acp_initialize_cached`、`acp_rpc_begin/end`。这些事件只用于诊断，不改变 canonical state；复跑排查时应先对齐 dynamic events 与 ACP diagnostics，再判断等待发生在调度、worktree、ACP RPC 还是 provider 首响应阶段。

## 7. runtime 配置
当前 runtime 相关配置统一由 `RuntimeConfig` 管理，至少包括：
- `default_provider`
- `log_level`
- `log_prompts`
- `log_provider_command`
- `log_retention_days`

边界：
- 配置由 CLI 或上层入口构造
- `App` 持有配置并向 observability / provider 执行链透传
- provider command 与完整 prompt 仅属于 debug observability，不属于 canonical state

## 8. 与 console / 插件的关系
- console CLI 是同一套 runtime 的交互壳，不引入新的 runtime 语义
- scriptable CLI、console CLI、VSCode 插件都应复用同一套 run / round / node / attempt / artifact 模型
- UI 可以提供更强的浏览、帮助与下钻体验，但不能改变 canonical state 的来源与控制流语义

## 9. 相关边界文件
- [Worker Ref 规范](../provider/worker-ref.md)
- [Progress 规范](../interaction/progress.md)
