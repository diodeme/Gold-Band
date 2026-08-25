# AI-DYNAMIC 节点

## 1. 一句话定义
`ai-dynamic` 是普通 workflow 中的复合节点：外层 runtime 仍按固定 DSL 前进，进入该节点后由内部 dynamic graph 根据 `dynamic-node-completion` artifact 派生后续内部节点、fanout group、merge 和 acceptance。

## 2. DSL 结构

```json
{
  "id": "router",
  "type": "ai-dynamic",
  "agentStrategy": {
    "mode": "fixed",
    "provider": "claude-acp",
    "model": "sonnet",
    "permissionMode": "acceptEdits"
  },
  "control": {
    "maxDynamicNodes": 20,
    "maxFanout": 5,
    "maxDepth": 6,
    "maxParallel": 3,
    "maxGroupDepth": 1,
    "maxWorkflowInvocations": 10,
    "allowNestedDynamic": false
  },
  "allowedWorkflows": [
    { "workflowId": "dev-review-test-accept" }
  ]
}
```

## 3. 关键语义
- `provider` 是 fan-out agent 的 provider，用于 bootstrap internal worker；fan-out agent 的角色与目标由 runtime 内置 prompt 提供，不在 DSL 中配置。
- `agentStrategy` 的权限均保存 Agent doctor 返回的原生 ACP id：fixed 策略使用 fixed Agent 的 `permissionMode`；dynamic 策略使用控制面 `permissionMode` 供 bootstrap、merge、acceptance 共用，候选 worker 使用 `availableAgents[].permissionMode`。不指定时使用对应 Agent 默认权限。
- 动态 Agent 策略下，proposal 只为普通 worker 选择 `provider`，不得输出 `model` 或 `permissionMode`；merge / acceptance 也不得输出 provider。runtime 根据 worker provider 查找 `availableAgents[]` 并注入预设模型、原生权限与 config options；bootstrap、merge、acceptance 固定使用 `bootstrapProvider`，分别使用 `bootstrapModel` 与 `acceptanceModel`，并共享控制面 `permissionMode`。固定策略仍由 runtime 注入 provider，并沿用固定 Agent 的模型与权限配置。
- 模型目录属于快速变化的 provider 能力事实。加载或保存工作流模板、创建或读取 task authoring workflow、以及创建 run 冻结快照时，runtime 使用最新 agent diagnostics 统一规范化 fixed model、`bootstrapModel`、`acceptanceModel`、普通 worker model 与 `availableAgents[].model`：只有在当前目录明确存在且已不包含配置值时才把字段清为“不指定”，并同步持久化作者态 JSON 与运行快照，保证编辑器、原始配置和实际调用一致；目录缺失时保留原值。ACP `session/new/load` 返回权威配置目录后会再次校验，若模型已过期则跳过模型设置并使用 provider 默认值，同时记录 `model_config_normalized` / `acp_model_config_normalized` 诊断事件，不把模型迭代转成用户必须手工修复的运行异常。
- 动态 Agent 策略的可选字段 `acceptanceModel` 只从 `bootstrapProvider` 的模型目录选择，并作用于 fanout 配套的 `merge` / `acceptance`；未指定时使用初始分发 Agent 的默认模型。
- 产品不再维护“只读 / 询问 / 完全访问”等统一权限枚举或 provider 中央映射。编辑器直接展示当前 Agent doctor 返回的 mode id/name，保存原生 id，并在 provider 能力已知时按同一权威目录校验；新增 ACP Agent 不需要先补 Gold Band 权限映射。
- `control` 是 runtime validation 的硬限制，不只是 prompt 提示。
- `allowedWorkflows.workflowId` 引用 workflow DSL 内的 `workflow.id`，不是模板外层 `template.id`；run start 时冻结为 allowed workflow snapshots。
- `allowedWorkflows` 引用的模板必须满足模板库级唯一性约束：若某个模板的 `workflow.id` 与其他模板重复，则任何包含该模板引用的 AI-DYNAMIC 工作流都不能保存，用户需手动修改模板 JSON 中的 `workflow.id` 后再试。
- `maxParallel` 是 runtime 的真实调度上限，不是提示词建议。dynamic graph 采用补位式并行：主线程统一维护 graph 状态并按空闲槽位发射 ready node；任一 running node 完成后，主线程先回写 proposal / materialize，再立即继续补齐新的 ready node，直到达到 `maxParallel`。
- `maxGroupDepth` 限制 fanout group 的嵌套深度；底层状态通过 `parentGroupId` 记录父子 group，子 group closed 后把自己的 acceptance 节点挂入父 group terminal，父 group 必须等所有 root chain 都到达 terminal boundary 后才会 merge。
- 外层 `ai-dynamic` DSL 不再配置 `merge` 或 `acceptance`。当内部节点输出 `next.type=fanout` 时，proposal 中必须同时给出该 group 的 `merge` 与 `acceptance` 可执行 spec，但两者都不输出 provider/profile；runtime 固定注入控制面 Agent、共享权限及 `acceptanceModel`。merge / acceptance 的角色提示词统一由 `src/prompts/<lang>/runtime/ai-dynamic/merge.md` 与 `src/prompts/<lang>/runtime/ai-dynamic/acceptance.md` 提供。merge 是执行型节点，不接入 `dynamic-node-completion` output contract；acceptance 接入同一控制协议，验收通过输出 `next.type=end`，验收不通过输出 `single` 或 `fanout` 创建修复节点。
- 所有会调用 ACP provider 的内部节点都必须在 orchestration 边界获得非空、稳定的 logical turn ID，包括不接入 output contract 的 merge。turn identity 与 `sessionMode`、output contract 相互独立：同一业务 turn 的自动重试必须复用原 ID，新的 worker / merge / acceptance 业务 turn 必须使用新 ID。动态 invocation 构建接口必须把该 ID 表达为必填输入，不能让节点类型各自决定是否生成，也不能由 provider 临时补造。
- fanout 必须创建至少两个 child 节点；只有一个后继任务时必须使用 `next.type=single`。workspace 属于 Runtime 领域，proposal 不允许输出 `workspace`、mode、路径或 branch：`single` 自动继承当前实际 workspace；`fanout` 的每个 child 自动获得隔离 Git worktree 与稳定 branch；merge / acceptance 回到该 group 的父 workspace。worktree 目录放在目标 repo 的 `.gold-band/worktrees/<task>/<run>/<short-id>` 下，`short-id` 由 round、外层节点、attempt 和内部节点稳定生成，避免 Windows 长路径 checkout 失败并保证同一 run 内不冲突。merge 前 Runtime 先 checkpoint 各 child workspace，再把 workspace path、branch、head、forkCommit、checkpointCommit 与 status 注入 prompt；merge agent 基于这些权威字段解决冲突并验证结果。
- `next.type=single` 不创建隔离 worktree，也不创建配套 merge / acceptance；需要并行隔离时必须使用 `next.type=fanout` 并提供 merge / acceptance。任何 proposal 显式输出 `workspace` 都由有效 schema 以 `dynamic.schema.additional-property` 拒绝并进入统一 repair 回路，避免 Agent 与 Runtime 同时决定 workspace 生命周期。
- AI-DYNAMIC run 创建前要求项目根目录是具有 HEAD 的 Git repository；不满足时直接返回结构化错误 `run.git-repository-required`，不启动 provider。Git/worktree 探测、fanout checkpoint/fork、merge 前 checkpoint 和 release 都由 Runtime 的 workspace catalog 统一管理，proposal 层不再暴露 `supportsWorktree` 或 workspace mode 选择。
- AI-DYNAMIC `graph.json.workspaces` 是 workspace catalog 的 canonical state，`dynamic/workspaces/*.json` 只是投影。workspace identity 迁移仅在 `WorkspaceState.path` 命中旧用户 runtime root 时改写并重建投影，即外层 AI-DYNAMIC 运行于普通会话 worktree 的场景；外层主工作区路径与 AI-DYNAMIC 在仓库 `.gold-band/worktrees/` 下创建的隔离 worktree 不受用户 runtime 目录改名影响。单条损坏 graph 必须局部隔离，不得阻断桌面启动。
- 内部 worker / acceptance 只能提交 `dynamic-node-completion` proposal；子线程负责执行并产出 proposal，主线程负责校验、记录 accepted/rejected proposal，并作为 graph 的唯一写入者执行 materialize。merge 只负责合并和报告，不提交控制 proposal。acceptance 的 accepted proposal 为 `next.end` 时 group 才 closed；如果输出 `single` 或 `fanout`，runtime 会 materialize 修复节点并让 group 回到未闭合状态等待修复链路完成。同一 dynamic run 的 run/graph/node/group/proposal 状态快照读写必须通过 run 级状态锁串行化，JSON 状态文件采用同目录临时文件原子替换写入，避免调度线程在 graph 更新中途读到半写入或新旧内容混合导致 `trailing characters` 一类解析错误。driver 热循环的快照持久化必须以 `DynamicGraphState` 内容指纹为门禁：首次或 graph 实际变化时写出整组派生文件，等待 worker 消息的 scheduler 心跳不重复重写磁盘。
- runtime 通过通用 output contract 机制把 artifact 名称、类型以及完整的 AI-DYNAMIC 输出协议文本注入 prompt；`dynamic-node-completion` 基础 schema 由 Rust 数据结构通过 `schemars` 生成，runtime 再按当前 Agent 策略、可用 provider、worker profile、allowed workflow snapshot 与 `maxFanout` 收窄。dynamic 策略的有效 schema 要求 worker 只输出 provider，merge / acceptance 禁止输出 provider，三者都禁止 `model / permissionMode`；同一 schema 同时进入 provider output contract、双语 prompt 和 runtime validator。
- AI-DYNAMIC prompt 分层为稳定 system prompt、用户提示、当前 user task 和 runtime hidden context。system prompt 只保留 AI-DYNAMIC 角色、文件边界、workspace 语义、两阶段执行原则与按本次 invocation 能力启用的 output contract 原则；merge 这类执行型节点不显示 `dynamic-node-completion` / `next.type` 控制协议指导。外层 `globalGoal` 是每个内部节点都必须继承的用户提示约束，进入可见 `# 用户提示` / `# User Tips` 块，不与当前内部节点 task 合并；可见 `# 任务` / `# Task` 只放当前节点业务任务，不追加 nodeId/title/kind/continueFromNodeId 等运行元信息，也不追加固定控制协议尾巴，hidden context 不重复整段 task。每次 invocation 变化的信息进入 `src/prompts/<lang>/runtime/ai-dynamic/hidden_context.md`，并合并到同一个 Gold Band hidden context 块中展示；AI-DYNAMIC 专用 hidden context 会替代普通 workflow 的 predecessor hidden context，避免普通 workflow 逻辑错误显示“无前序”。AI-DYNAMIC hidden context 由 runtime 根据当前节点位置投影生成：包含直接前序、当前 group、继承的父 group、并行兄弟边界、可复用会话、运行预算、workspace 和可用 agent/profile；其中并行兄弟边界只给 group 内普通 worker / workflow invocation 分支查看，merge / acceptance 通过当前 group 与直接前序理解分支状态，不额外展示 siblings；会话复用、运行预算、agent/profile 选项只在启用 output contract 的 worker / acceptance 中展示，merge 不展示这些路由决策上下文。不展示内部控制 artifact 路径，不把 `dynamic-node-completion` 作为前序材料暴露给模型。
- AI-DYNAMIC 内部节点之间传递业务证据时使用 attachments，不使用控制 artifact。runtime 的 attachment manifest 只列当前节点允许消费的附件：直接前序 attachments 默认可见；merge / acceptance 可见当前 group 分支、merge 和验收相关附件；嵌套 fanout 只继承父 group 的出口附件摘要；普通并行 worker 只能看到 sibling 的存在和边界，不能消费 sibling attachments，除非显式成为依赖或进入 merge / acceptance。
- internal worker 在 hidden context 中会额外拿到一段“当前链路可复用会话节点”列表，只包含当前 dynamic graph、当前 chain、且位于最近 fan-out 边界之内的可继续节点；列表字段最小化为 `nodeId / title / goal`。若 proposal 中某个后继节点声明 `sessionMode=continue`，则必须同时提供 `continueFromNodeId`，并且只能引用这份列表中的 worker 节点；`workflow-invocation` 不允许继续会话。执行 `sessionMode=continue` 的节点时，continue 只表示复用 `continueFromNodeId` 的 ACP session 记忆，不表示继续执行来源节点任务；user prompt 的 `# 任务` / `# Task` 必须只保留当前节点业务任务，当前节点的 `nodeId/title/kind/continueFromNodeId` 等运行事实放 hidden context。
- proposal 校验失败与非法 JSON 解析失败统一进入同一个 repair 回路：runtime 会把本轮发现的全部问题一次性回传给当前 internal worker 做隐藏修复，最多重试 3 次；耗尽后外层 AI-DYNAMIC 进入 `paused/error-blocked`。结构性错误先由有效 JSON Schema 诊断，业务图错误继续由 Rust 语义校验聚合；repair prompt 渲染结构化诊断，包含 code、path、actual、expected、allowed values、suggested repair，并附带当前合法 provider/model、worker profile ID 与 allowed workflow ID 参考。
- 每次 internal worker / acceptance 输出被 runtime 提取为 `dynamic-node-completion` 候选后，runtime 会在对应 ACP `textDelta` 写入 `raw.runtimeControlOutputDisplay` 展示标记。accepted、rejected、非法 JSON parse failure 与 repair 重试的 A/B/C 输出都按各自 provider 返回独立标注；该标记只用于会话 UI 将控制 JSON 渲染为 Gold Band 工作流控制折叠条，收起态不展示 JSON 内容，不参与 proposal 校验或 graph materialize。
- dynamic leaf 的 ACP session update 是 canonical graph/timeline 持久化后的控制面失效通知，不携带第二份 timeline。仅当前选中的 AI-DYNAMIC root session 在收到 `runtime.active=false + runtime.phase=terminal` 的 lifecycle-only 通知后，执行一次合并去重、受既有页大小约束的 session 查询，使 `runtimeControlOutputDisplay` 后置标注无需切换会话或重启即可替换流式原文；切换 locator 后旧响应不得覆盖新会话。Direct、普通 Workflow 与 Agent branch 不进入这条额外查询路径。最后一个 leaf 完成到 outer AI-DYNAMIC 收尾之间允许 graph 临时持有该 leaf 的运行投影；graph 从 `running` 写入 `completed` 或 `paused` 时必须推进被释放 leaf 的既有 `runtimeLifecycleRevision`，在 graph durable 后只向受影响 leaf 发布定向 session update，释放 composer“处理中”。
- dynamic graph `0.4` 将 leaf 的 phase-only execution revision 替换为单一 `runtimeLifecycleRevision / runtimeLifecycleUpdatedAt`。leaf prompt、finalize/repair、暂停/终态，以及 graph workspace checkpoint/fork/merge/release 对 causal leaf 的接管/释放，都推进这个 leaf-owned 水位；outer `RunState.execution.revision` 与 ACP `acpRevision` 保持独立，前端不得组合或跨域比较。workspace transition 直接暂时写入 causal leaf 的 `runtimeExecutionPhase=PreparingWorkspace`，结束时恢复原 phase；`currentNodeIds` 仍只表示活跃 leaf 聚合，不能作为 transition owner，避免一个分支的 Git 操作污染并行或历史会话。
- proposal 的业务校验会尽可能聚合错误，而不是命中第一条就返回。典型错误包括 profile 不存在、provider 不可用、fanout 超出 `maxFanout`、group depth 超出 `maxGroupDepth`、workflowId 不在 allowed snapshot、merge/acceptance spec 不完整等。
- rejected proposal 不再只保存字符串错误，而是保存结构化错误对象：至少包含 `code`、`message`、`params`，并可携带 `path / actual / expected / allowedValues / suggestion`。其中 `code` 用于稳定识别错误类型，`path` 指向 proposal JSON 路径，`params` 提供 nodeId / field / profile / provider / limit / actual 等上下文字段，便于后续 UI、日志和 prompt 复用。
- 外层 edge 仍然只消费 `ai-dynamic` 的最终 `success / failure / killed` outcome；若内部 dynamic worker、merge/acceptance 节点或 `workflow-invocation` child run 进入暂停，外层 `ai-dynamic` node 也以复合节点形式暂停，并在继续时由 runtime 委托内部 paused node 或 `childRunId` 从自身断点恢复。会话态和 Round 详情对内部节点的继续发送必须走 `submit_conversation_prompt -> run_continue_dynamic_inner_background`，由 runtime 校验 outer locator 与 inner locator，只 re-arm 目标 internal node 并回到 `drive_dynamic_graph`；不得直接对 dynamic inner ACP session 调 `send_acp_prompt` 绕过 completion 解析和 graph materialize。`send_acp_prompt` 若命中 paused/resumable/current dynamic inner attempt，必须拒绝并要求统一 submit 入口。
- Round 详情运行态主图内联展示 AI-DYNAMIC 内部节点时，外层 workflow 的后续边仍按 `ai-dynamic` 最终 outcome 前进，但可视化连接端点必须落到内部 dynamic graph 的出口节点，而不是复合节点占位。出口节点按内部图真实边语义计算：显式 `dependsOn`、`sessionMode=continue` 和 runtime 由 `chainId/depth` 派生的隐式成功边都会让上游节点不再视为出口；当前 V1 常见为单出口，后续允许多个无下游出口同时连接到外层后继节点。
- 外层 run stop 时需要递归停止 AI-DYNAMIC 内部并行节点与 child workflow run，并把可达 dynamic 状态一并收敛到 `ProcessInterrupted` paused；应用关闭或启动恢复同样递归收敛为可继续暂停。可恢复本地 IO/资源、ACP transport 或 driver 异常收敛为 `RuntimeAbnormal` paused，供后续 continue 恢复；新停止链路不再把普通停止写成 killed。

## 4. 内部控制 artifact
内部 worker 与 acceptance 必须输出 canonical artifact：

```text
dynamic-node-completion
```

V1 支持：
- `next.type=end`
- `next.type=single`
- `next.type=fanout`

内部 worker 的 `profile` 为选填；不填时 runtime 不注入 worker profile 内容。`profile` 只允许出现在 worker proposal 中，必须使用可用 profile 的 id，不能使用 displayName；merge / acceptance 不输出 provider/profile/model/permissionMode，统一使用 runtime 内置 prompt 和控制面配置。dynamic Agent 策略只有 worker proposal 输出 provider。`workflow-invocation` 节点不输出 `provider`、`model` 或 `permissionMode`。

workflow invocation 节点完成 child run 后由 runtime 包装 `dynamic-node-completion`，避免固定 child workflow 混入 dynamic 控制语义。

## 5. V1 边界
- 不支持 nested `ai-dynamic`，除非后续显式打开 `allowNestedDynamic`。
- 不引入 direct mode、route-decision、triage-result 或 replan artifact。
- 内部状态保存在外层节点 attempt 的 `dynamic/` 目录下，不写入外层 round trace。
- invalid proposal、provider/model/catalog/workspace/workflow/DSL 前提错误、不可恢复 internal node failure 或 merge failure 会让外层 run 进入 `error-blocked` pause；本地 IO/资源、ACP transport、driver interruption 等可恢复运行异常进入 `runtime-abnormal` pause，保留 runtime continue 入口。
