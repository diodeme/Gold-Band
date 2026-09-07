# AI-DYNAMIC 节点 Prompt 优化方案

## 1. 背景与目标

当前普通 workflow 节点的 prompt 已经拆分为稳定 system prompt、每次 invocation 刷新的 user hidden context、以及可见 user prompt。这个拆分解决了 ACP `session/load` / continue 场景下 system prompt 不能可靠刷新动态事实的问题。

AI-DYNAMIC 内部节点目前复用了普通 workflow 的 prompt bundle，但仍通过 `extra_system_sections` 把大量动态运行事实注入 system prompt，包括 dynamic graph、预算、workspace、resumable sessions、group context 等。这会让 AI-DYNAMIC 的 prompt 分层和普通 workflow 不一致，也会在 `sessionMode=continue` 时放大歧义：模型复用了旧 session，但 user prompt 只收到通用 `# Goal`，当前动态节点任务不够明确。

本方案目标：

- 收敛 AI-DYNAMIC prompt 职责分层，让 system prompt 只承载稳定规则。
- 修正 `sessionMode=continue` 的当前任务表达，避免把 continue 来源节点误当成当前任务。
- 保留现有 runtime 架构，不拆分 worker/router。
- 让 acceptance 接入 `dynamic-node-completion`，由验收结果决定结束或继续修复。
- 清理 system、hidden context、task 中重复和冲突的动态上下文。
- 将 AI-DYNAMIC runtime context 从“字段堆叠”重构为“按当前节点位置投影出的最小上下文”。

## 2. 设计判断

### 2.1 worker + profile + output contract 保持现状

`profile` 和 `dynamic-node-completion` 不冲突：

- `profile` 负责指导 agent 如何完成当前业务任务，例如开发、测试、审查、验收。
- `dynamic-node-completion` 是 output contract，负责指导 agent 最后输出 runtime 可消费的控制结果。

因此本轮不拆 runtime，不新增独立 router/planner 节点。AI-DYNAMIC bootstrap 的 `InlineControl` 继续在当前 turn 执行控制协议；worker / acceptance 的 `PostTurnProjection` 则把业务处理与控制规划分成两个 turn：业务 turn 可以直接完成当前任务，或在判断任务应继续分发时立即停止并自然结束，但不得提前拆分任务、选择 Agent 或规划/执行后继节点；hidden finalize turn 获得完整 artifact 协议和路由上下文后，才规划并输出 `dynamic-node-completion`。

需要优化的是 prompt 明确度：

- 当前任务执行阶段必须优先遵守 profile；选择交回 runtime 时则立即停止业务执行。
- PostTurn 业务 turn 不得提前规划后继任务或猜测 artifact schema。
- hidden finalize 阶段必须严格遵守 output contract，并且只在该阶段规划路由。
- prompt 不应让模型在缺少 artifact 协议、运行预算和 Agent 选项时提前形成无效分发方案。

### 2.2 merge 不接控制协议，acceptance 接控制协议

merge 是执行型节点，职责是合并分支、解决冲突、写合并报告。merge 的成功或失败由 provider run status 和 merge 结果表达，不需要强制输出 `dynamic-node-completion`。

acceptance 是决策型节点，职责是判断当前 group 是否满足目标，并决定是否继续修复。因此 acceptance 应接入 `dynamic-node-completion`：

- 验收通过：输出 `next.type="end"`。
- 验收不通过且只需一个修复方向：输出 `next.type="single"` 创建修复 worker。
- 验收不通过且需要多个独立修复方向：输出 `next.type="fanout"` 创建修复分支，并提供后续 merge / acceptance spec。

runtime 需要解析 acceptance 的 `dynamic-node-completion` 并 materialize 后续节点，而不是只根据 provider success 直接关闭 group。

### 2.3 continue 表示复用会话，不表示复用任务

`sessionMode=continue` 的语义是复用指定来源节点的 ACP session 记忆和上下文，不是继续执行来源节点的旧任务。

例如 `goodbye-step` continue `hello-step` 时：

- 当前节点仍然是 `goodbye-step`。
- 当前任务仍然是创建 good bye 类。
- `continueFromNodeId=hello-step` 只表示复用 hello 节点的会话上下文。

因此 continue 的 user prompt 必须显式包含当前动态节点 `nodeId / title / kind / task`，并说明 continue 来源只是会话复用。

## 3. Prompt 分层方案

### 3.1 AI-DYNAMIC system prompt

`src/prompts/<lang>/runtime/ai-dynamic/system.md` 只保留稳定规则：

- AI-DYNAMIC 的基本身份和边界。
- 文件系统与 workspace 的稳定规则。
- fan-out / merge / acceptance 的稳定职责。
- `dynamic-node-completion` 是内部控制协议。
- 节点身份、workspace、预算等动态运行事实以本次 user hidden context 为准；业务范围和验收标准仍服从用户需求与批准输入。

从 system prompt 移除以下会随 invocation 变化的字段：

- outer attempt、dynamic run、internal node、group、chain、depth。
- dynamic root、node dir、attempt dir、attachments dir。
- workspace path、workspace capability、upstream refs。
- agent routing、available providers、available profiles。
- remaining budget、allowed workflow snapshots。
- graph summary、resumable sessions、dependsOn、kind-specific context。

### 3.2 AI-DYNAMIC hidden context

新增：

- `src/prompts/zh-CN/runtime/ai-dynamic/hidden_context.md`
- `src/prompts/en/runtime/ai-dynamic/hidden_context.md`

该模板承载每次 invocation 必须刷新的动态事实，但不直接等同于完整 graph dump。AI-DYNAMIC 内部节点的当前任务仍只放在可见 `# 任务` / `# Task` 中；如果外层配置了 `globalGoal`，它作为每个内部节点都必须继承的用户提示约束，进入独立 `# 用户提示` / `# User Tips` 块，不再和当前节点 task 混排。hidden context 只负责说明“为什么轮到当前节点、当前处于什么图位置、哪些业务附件可消费”。

- Dynamic identity：outer node、outer attempt、dynamic run、internal node、kind、title、group、chain、depth。
- Continue context：session mode、continueFromNodeId、continue source summary。
- Filesystem：Dynamic root 只展示一次；node dir 相对 Dynamic root、attempt dir 相对 node dir、attachments dir 相对 attempt dir，coordination snapshot 相对 Dynamic root；branch workspace 另声明一次 Runtime 权威 worktree root。
- Workspace：mode、path、capability。
- Graph context projection：direct predecessors、active group、inherited group、siblings、resumable sessions；siblings 只给 group 内普通 worker / workflow invocation 分支展示，merge / acceptance 不展示。
- Agent context：agent strategy、available providers、available profiles、allowed workflow snapshots；只在启用 output contract 的 worker / acceptance 中展示。
- Runtime limits：remaining budget、fanout / workflow invocation / group depth 等剩余额度；只在启用 output contract 的 worker / acceptance 中展示。
- Attachment manifest：当前节点允许消费的前序或 group 出口 attachments。

普通 workflow hidden context 仍保留 round / attempt / predecessors / attachments 等通用信息。AI-DYNAMIC 内部节点使用专用 hidden context 投影；渲染时合并到同一个 Gold Band hidden context 块中，但不再渲染普通 workflow 的 `Latest predecessor chain / transition reasons`，避免普通 workflow 前序链逻辑和 AI-DYNAMIC 内部图语义冲突。

AI-DYNAMIC hidden context 不展示内部控制 artifact：

- 不展示 `dynamic-node-completion` artifact 路径。
- 不展示 proposal artifact 路径。
- 不展示 raw stream / diagnostics 等 runtime 文件。
- 不通过 artifact 摘要解释“为什么轮到当前节点”；当前节点 task 和 context projection 已经承担该语义。

AI-DYNAMIC 可以展示 attachments，因为 attachments 是节点主动写给后续节点阅读的业务证据，例如 `dev-report.md`、`verification.json`、`accept-report.md`。

merge 是执行型节点，不承担路由规划输出：hidden context 对 merge 只保留当前动态节点、运行位置、直接前序、当前 group、继承 group、workspace 和可用 attachments；不展示并行兄弟节点、会话复用、运行预算、Agent 与 profile 选项。system prompt 和 node task 也按本次 invocation 是否启用 output contract 条件化渲染，merge 不显示 `dynamic-node-completion` / `next.type` 控制协议指导。

### 3.3 AI-DYNAMIC visible user prompt

`RequirementTask` 模式：

- 保留 `# 需求` / `# Requirement`。
- 如果存在外层 `globalGoal`，渲染独立 `# 用户提示` / `# User Tips`。
- `# 任务` / `# Task` 中只包含当前动态节点业务任务。
- 不追加 runtime 元信息或控制协议提示；当前动态节点身份、continue 来源、output contract 规则分别由 hidden context、system prompt 和 provider output contract 承担。

`WorkflowResume` 模式：

- 不再只输出通用 `# 目标` / `# Goal`。
- 必须包含当前动态节点任务。
- 不在可见 `# 任务` / `# Task` 中重复 `nodeId / title / kind / continueFromNodeId` 或 continue 解释；这些信息只放 hidden context。

建议中文可见结构：

```md
# 目标
继续当前 AI-DYNAMIC 内部节点。

# 用户提示
外层 globalGoal（如有）

# 任务
当前节点业务任务
```

`RuntimeRepair` 模式保持现状：只发送 repair prompt，不注入 hidden context。

### 3.4 ACP continue prompt state 统一

普通 workflow worker 与 AI-DYNAMIC 内部 worker / acceptance / merge 必须复用同一套 ACP invocation prompt state 决策：

- 新 session：`RequirementTask`。
- continue session 且用户没有显式输入：`WorkflowResume`，发送 runtime 默认继续提示。
- continue session 且用户有显式输入：`UserMessage`，只发送用户输入原文，不注入 hidden context，不包装 `# 目标` / `# Goal`。
- runtime repair：`RuntimeRepair` 单独覆盖，不参与普通 continue 决策。

实现上由统一 resolver 生成 `sessionMode / continueRef / resumePrompt / promptId / visibility / renderMode / attachments / model / permissionMode`。普通 workflow worker、AI-DYNAMIC worker、AI-DYNAMIC acceptance、AI-DYNAMIC merge 只负责提供各自的 continue ref 来源和业务参数，不得各自复制 prompt mode 判断。收敛后必须删除原先分散在 `run_continue`、dynamic worker 和 merge agent stage 中的重复 continue prompt 逻辑，避免新旧两套路径并存。

## 4. Acceptance 控制协议方案

### 4.1 invocation 构建

当前 `DynamicNodeKind::Worker | DynamicNodeKind::WorkflowInvocation` 会启用 `dynamic_output_contract`，`Merge | Acceptance` 不启用。

调整为：

- `Worker`：继续启用 `dynamic_output_contract`。
- `WorkflowInvocation`：保持现有包装语义。
- `Merge`：不启用 `dynamic_output_contract`。
- `Acceptance`：启用 `dynamic_output_contract`。

### 4.2 execution 流程

acceptance 不再完全复用普通 `execute_dynamic_agent_stage` 的终止逻辑。建议新增或拆分 acceptance 执行路径：

1. 准备 main workspace、attempt 目录和 prompt invocation。
2. 调 provider。
3. provider success 后读取 `dynamic-node-completion` artifact。
4. 走与 worker 相同的 JSON schema 校验和 proposal 语义校验。
5. 校验失败进入现有 repair 流程。
6. 校验通过后：
   - `next.end`：标记 acceptance success，并关闭当前 group。
   - `next.single` / `next.fanout`：materialize 修复节点，当前 group 不关闭，等待修复链路完成后重新进入 merge / acceptance 或后续 proposal 指定的路径。

### 4.3 group 状态

当前 group closure 逻辑在 acceptance 节点成功后直接关闭 group。调整为：

- acceptance 输出 `next.end` 才关闭 group。
- acceptance 输出后续节点时，不关闭 group。
- 后续修复节点完成后，按现有 dynamic graph 驱动继续推进。

需要避免重复创建同名 acceptance 节点。实现时可以：

- 允许新的修复链路 proposal 创建新的 acceptance 节点。
- 或在当前 acceptance 后续链路结束时，由 proposal 自己决定是否 `single` 到修复 worker，再由修复 worker 创建新的 acceptance。

本轮优先采用 proposal 驱动，不额外引入自动 retry acceptance 机制。

## 5. 上下文去重方案

### 5.1 权威来源

同一类信息只保留一个权威注入位置：

- 稳定规则：system prompt。
- 本次运行事实：AI-DYNAMIC hidden context。
- 用户需求和当前任务：visible user prompt。
- output JSON schema：output contract。

如果 system、hidden、task 中出现同类运行事实，模型应以本次 hidden context 为准；hidden context 不得覆盖业务范围、明确排除项或批准验收标准。

### 5.2 需要去重的信息

- branch workspace、terminal nodes、merge base、branch head、dirty status。
- group id、root nodes、terminal nodes、merge node、acceptance node。
- current node id/title/task/kind。
- dynamic root、attempt dir、attachments dir。
- graph summary、remaining budget、resumable sessions。

merge / acceptance 的 node task 中不再拼接大段 runtime 派生上下文，只保留用户可读任务目标；详细 group 和 branch 信息进入 hidden context。

## 6. AI-DYNAMIC Runtime Context Projection 重构方案

### 6.1 问题判断

当前 AI-DYNAMIC runtime context 的根本问题不是缺字段，而是字段组织方式不对。现状更接近“把所有动态事实都塞进 prompt”：

- `graph_summary` 给出全局节点数量和完成节点列表。
- `upstream_refs` 混合了 dependency、control artifact 路径和 attachments 目录。
- `kind_specific_context` 只对 merge / acceptance 特化，普通 worker、group 后 single、nested fanout 缺少统一规则。
- 通用 predecessor context 可能把 `dynamic-node-completion` 当成前序 artifact 暴露给模型。

AI-DYNAMIC 内部节点和普通 workflow 节点不同：动态节点被 materialize 时已经带有当前 `task`，它不需要通过上游控制 artifact 理解“为什么轮到我”。因此 AI-DYNAMIC runtime context 应该从“历史说明书”变成“当前节点上下文投影”。

### 6.2 设计目标

新增一个统一的上下文投影概念：

```rust
DynamicContextProjection::build(graph, current_node)
```

它只回答三个问题：

1. 为什么轮到当前节点。
2. 当前节点处在 dynamic graph 的什么位置。
3. 哪些 attachments 可以消费。

它明确不回答：

1. 不展示 `dynamic-node-completion` artifact。
2. 不展示控制 artifact 路径。
3. 不把 sibling 分支产物伪装成前序输入。
4. 不展开全量历史。

### 6.3 投影视图

建议输出以下固定视图：

```rust
struct DynamicContextProjection {
    current: CurrentNodeView,
    direct_predecessors: Vec<NodeRefView>,
    active_group: Option<GroupDetailView>,
    inherited_groups: Vec<GroupExitSummaryView>,
    siblings: Vec<SiblingView>,
    attachments: AttachmentManifest,
    runtime_limits: RuntimeLimitsView,
    session_reuse: SessionReuseView,
}
```

各视图职责如下：

- `CurrentNodeView`：当前 nodeId / title / kind / workspace / sessionMode / continueFromNodeId；不重复渲染 task。
- `DirectPredecessorView`：只展示 accepted proposal materialization 的直接 `source` 与显式 `dependsOn` 直接节点；两者取并集并按 nodeId 去重，只展示节点状态和结果，不展示 artifact。更早 source 仅进入下方有界附件链路。
- `ActiveGroupView`：当前节点在 group 内时展示当前 group 的详细状态，例如 root nodes、siblings、merge、acceptance、branch workspace。
- `InheritedGroupView`：当前节点位于 group 后续 single 链路时，展示 group 出口摘要，例如 acceptance 失败后创建修复节点。
- `SiblingView`：仅当当前普通 worker / workflow invocation 的 chain 能映射到 active group 的某个 root branch 时，展示同一 fanout cohort 的其他 roots，并且只说明存在、状态和边界；repair / reaccept 链不得因 group 回到 `open` 而显示旧 roots，也不能消费其 attachments。
- `AttachmentManifest`：列出当前节点允许消费的 attachments。
- `RuntimeLimitsView`：预算、fanout、workflow invocation、group depth、parallel slot。
- `SessionReuseView`：resumable sessions 和 continue 来源说明。

### 6.4 group 内外展示规则

#### group 内部节点

group 内部节点看当前 group 详细信息：

```text
Current node
+ Direct predecessors
+ Active group detail
+ Sibling existence and boundaries
+ Available attachments allowed for this node
+ Parent group summary, if nested
```

典型规则：

- fanout worker：仅在自己的 chain 映射到当前 group root branch 时知道同批 sibling 存在，但不能消费未显式依赖的 sibling attachments。
- merge：通过显式 `dependsOn` 消费直接 terminal branch 附件，并在同一 group 证据分类中保留当前 group 的 terminal/root 原始输入。
- acceptance：通过 materialization source 与显式 `dependsOn` 消费 merge 附件，并在同一 group 证据分类中取得当前 group 的 terminal/root branch 输入及相关 group 最近一轮 merge / acceptance 附件。
- acceptance 创建 repair / reaccept 后继续沿用既有 group `open` 生命周期；该状态仅表示 group 尚未最终闭合，不把 repair / reaccept 重新归入旧 fanout cohort。

#### group 后续 single 节点

group 后面的 single 不属于原 fanout 并行分支，它看到的是 group 出口上下文：

```text
Current node
+ Direct predecessor
+ Inherited group exit summary
+ Direct predecessor attachments
+ Group exit attachments
```

例如：

```text
fanout group G
  -> merge
  -> acceptance failed
  -> B(single)
```

`B` 应看到：

- `acceptance` 是 accepted proposal materialization source 链上的直接来源。
- `G` 的 branches / merge / acceptance 已完成或进入 repair-chain-active。
- `acceptance` 写出的验收失败证据附件。
- 显式 `dependsOn` 节点和该 group 最近一次 merge 的附件。

不应展示：

- `dev-good-morning/artifacts/dynamic-node-completion.json`
- `dev-good-night/artifacts/dynamic-node-completion.json`
- bootstrap proposal artifact

#### single 后再 single

例如：

```text
fanout group G -> B(single) -> E(single)
```

`E` 应优先看到：

- materialization source 接力链中的 `B` 及更早来源节点，整条链最多回溯 5 个节点。
- 继承的 `G` 出口摘要。
- `G` 历史中最近一轮 merge / acceptance 的关键证据附件。

越往后走只沿 accepted source 接力链有界回溯，group 背景降级为摘要，避免把全部历史反复注入 prompt。

#### nested fanout

例如：

```text
fanout group G1 -> B(single) -> E(single) -> fanout group G2
```

`G2` 内部 worker 应看到：

- 当前 active group 是 `G2`，展示 `G2` 详细信息。
- `E` 是直接来源。
- `G1` 是 parent / inherited group，只展示摘要和关键出口证据。
- 只有 chain 能映射到 `G2` root branch 的 worker 才展示同批 sibling 的存在和边界，且不能消费未显式依赖的 sibling attachments。

`G2` merge / acceptance 都通过同一 group 证据分类取得当前 group 的 terminal/root branch 原始输入；merge 的 terminal 节点通常也会命中直接 `dependsOn`，跨分类按 nodeId 去重。二者均按三类来源保留 `E` 和 `G1` 最近一轮 merge / acceptance 的关键附件作为背景证据。

### 6.5 AttachmentManifest 规则

AI-DYNAMIC 的可消费材料以 attachments 为准，来源严格限定为三类：

1. accepted proposal materialization `source` 接力链：从当前节点的 source 逐跳向前，最多回溯 5 个节点。
2. 显式 `dependsOn`：只包含当前节点直接声明的依赖节点，不递归展开依赖链。
3. Group 证据：当前节点为 merge 或 acceptance 时，先加入当前 group 的 terminal 与 root branch 输入；随后对当前节点 active / inherited 的相关 group，以最新 acceptance 为轮次锚点，并选择它通过显式 `dependsOn` 对应的 merge，尚无 acceptance 时才选择最新 merge。历史附件不按节点 `status/outcome` 过滤，失败、中断或执行中的节点已有附件同样可见。若最近控制节点没有附件，不回退到更旧一轮的附件。

三类来源按 nodeId 跨类去重。同一节点无论同时命中 source、`dependsOn` 或 group 证据，只在 manifest 中出现一次。group 进入 repair / reaccept 后仍按既有状态机回到 `open`；当前 `mergeNodeId / acceptanceNodeId` 可以为下一轮重置，因此只能表达当前阶段，历史证据必须从 canonical node 与 accepted proposal 历史解析，不能依赖这两个可变槽。

递归扫描每个来源节点时最多检查 10 个末端项：文件和空目录计数，含内容的目录只继续递归且不单独计数。遇到第 11 个末端项、符号链接或读取异常时停止展开，显式展示该节点完整 `attachments` 目录并提示其余文件到目录查看；manifest 只列找到的文件路径，不读取或内联附件正文。parallel sibling 不构成第四类附件来源；只有显式命中 `dependsOn`、source 或上述 group 证据规则时才可见。普通 repair / reaccept worker 不会取得旧 group branches。

hidden context 省略空分类，并用带解释的标题区分三类路径：“前序链路（创建当前节点的任务接力链，最多回溯 5 个节点）”“显式依赖（当前节点通过 dependsOn 明确指定的输入节点）”“Group 证据（当前 merge / acceptance 输入或相关 group 最近一轮合并与验收）”。

Attachment manifest 只列业务附件：

```text
Available attachments:
- B/attempt-001/attachments/fix-report.md
- G1-accept/attempt-001/attachments/verification.json
```

禁止列：

```text
artifacts/dynamic-node-completion.json
proposals/*.json
acp.raw.jsonl
acp.diagnostics.jsonl
```

### 6.6 Prompt 渲染结构

新的 `runtime/ai-dynamic/hidden_context.md` 建议结构按语言分别维护。中文 prompt 中 section 标题必须使用中文，英文 prompt 中 section 标题使用英文。

中文 `zh-CN` 示例：

```md
## 当前动态节点
...

## 运行位置
...

## 直接前序节点
...

## 当前 group
...

## 继承的 group 上下文
...

## 可用附件
...

## 会话复用
...

## 运行预算
...

## Agent 与 profile 选项
...
```

英文 `en` 示例：

```md
## Current dynamic node
...

## Runtime location
...

## Direct predecessors
...

## Active group
...

## Inherited group context
...

## Available attachments
...

## Session reuse
...

## Runtime limits
...

## Agent and profile options
...
```

空 section 不渲染，避免出现大量“无”。当前 task 不在 hidden context 中重复，始终由 visible `# 任务` / `# Task` 表达。

### 6.7 路径投影压缩

#### 设计判断与边界

长程运行中 hidden context 膨胀的原因不是 canonical locator 或文件布局错误，而是文本渲染对每个附件、节点目录与 branch workspace 重复输出相同绝对路径前缀。修复应停留在 `DynamicContextProjection` 的显示层：graph、attachment locator、workspace catalog、磁盘目录及后续工具读取仍使用原始完整路径，不新增缩写路径 schema、持久字段或第二事实源。

#### 数据与算法

- 当前 invocation 先声明一次 Dynamic root；node dir 相对 Dynamic root，attempt dir 相对 node dir，attachments dir 相对 attempt dir，`coordination-snapshot.json` 相对 Dynamic root。
- 可用附件以 Dynamic root 为 `pathRoot`；branch workspace 以 Runtime 已有的 dynamic worktree base dir 为 `pathRoot`，叶子继续携带 workspaceId、branch、commit、head 和 status 等既有 metadata，并遵守与附件相同的相对路径与跨根回退契约。
- 对每一组已经枚举、已经过可见性过滤的路径，按路径组件而不是字符串字符或固定目录层级写入 trie。任意层级都可以形成分叉；渲染时按组件稳定排序，并把“不是终点且只有一个 child”的连续节点压缩为一条 `a/b/c` 路径。
- 条目既可以是另一个条目的祖先，也可以在不同父目录下具有相同文件名；terminal 标记和父子结构必须同时保留，不能按文件名去重或丢失后代。
- 无法相对该组 `pathRoot` 的跨盘符、跨根或其他越界条目不参与相对 trie，按稳定顺序显式渲染为顶层 `absolutePath=<完整路径>`。`absolutePath` 本身是 Agent 可直接使用的 locator，不得与 `pathRoot` 再次拼接；该规则同样适用于 branch workspace，确保显示压缩不改变 locator 含义。

算法不能假设差异固定发生在 node、attempt 或 attachments 层。例如根为 `A` 时：

```text
A/B/C/D
A/C/D
A/B/C/E
A/B/D/E
```

应投影为：

```text
pathRoot=A
- B/
  - C/
    - D
    - E
  - D/E
- C/D
```

该投影只遍历本次 prompt 原本已经枚举的路径，时间和内存复杂度均为 `O(total path components)`；不新增文件系统扫描、I/O、依赖、缓存、队列、锁或并发机制。相比重复绝对路径，输出长度与 provider token 占用随公共前缀长度显著下降。

## 7. 实施步骤

### 任务 1：新增 AI-DYNAMIC hidden context prompt

涉及文件：

- 新增 `src/prompts/zh-CN/runtime/ai-dynamic/hidden_context.md`
- 新增 `src/prompts/en/runtime/ai-dynamic/hidden_context.md`
- 修改 `src/prompts.rs`

实现要点：

- 中英文目录结构保持一致。
- 新增 include 常量，例如 `AI_DYNAMIC_HIDDEN_CONTEXT_ZH_CN / EN`。
- hidden context 模板覆盖当前 node、continue、workspace、context projection、budget、agent。
- section 标题、说明文字和固定标签必须按语言分别维护；中文 prompt 不使用英文标题占位。
- 当前 task 不在 hidden context 中重复渲染。
- 内部控制 artifact 不进入 hidden context。

完成标准：

- Rust 编译可找到新增 prompt 常量。
- 中英文 prompt 文件字段一致。

### 任务 2：收窄 AI-DYNAMIC system prompt

涉及文件：

- 修改 `src/prompts/zh-CN/runtime/ai-dynamic/system.md`
- 修改 `src/prompts/en/runtime/ai-dynamic/system.md`

实现要点：

- 移除动态事实字段。
- 保留稳定规则与控制协议说明。
- 明确“本次运行事实以 Gold Band hidden runtime context 为准”。
- 直接按既有 `OutputEmissionMode` 渲染三种语义：InlineControl 当前轮次控制、PostTurnProjection 后置规划、无 emission mode 的纯执行节点。

完成标准：

- system prompt 不再包含 attempt dir、graph summary、remaining budget、resumable sessions 等动态字段。

### 任务 3：调整 runtime prompt 渲染

涉及文件：

- 修改 `src/app/orchestrator.rs`
- 视实现需要修改 `src/provider/mod.rs`

实现要点：

- 将 `dynamic_system_sections` 拆分为 stable system section 和 dynamic hidden context 数据源。
- 在 AI-DYNAMIC invocation 的 user prompt 中注入 AI-DYNAMIC hidden context。
- AI-DYNAMIC invocation 使用专用 hidden context 投影，不再渲染普通 workflow predecessor hidden context。
- `globalGoal` 作为每个内部节点的用户提示约束，进入独立 `# 用户提示` / `# User Tips`，不拼进当前节点 task。
- `WorkflowResume` 模式下为 AI-DYNAMIC 渲染当前 node task，而不是只渲染 generic `# 目标` / `# Goal`。
- `RuntimeRepair` 继续只发送 repair prompt。

完成标准：

- 普通 workflow prompt 行为不变。
- AI-DYNAMIC continue user prompt 包含当前 node task 与 `continueFromNodeId`。

### 任务 4：调整 node task 与 acceptance prompt

涉及文件：

- 修改 `src/prompts/zh-CN/runtime/ai-dynamic/node_task.md`
- 修改 `src/prompts/en/runtime/ai-dynamic/node_task.md`
- 修改 `src/prompts/zh-CN/runtime/ai-dynamic/acceptance.md`
- 修改 `src/prompts/en/runtime/ai-dynamic/acceptance.md`

实现要点：

- `node_task.md` 不追加固定尾巴；当前任务与 continue 来源的区别由 hidden context 和 system prompt 表达。
- `acceptance.md` 明确最终必须输出 `dynamic-node-completion`。
- 通用 `artifact_finalize.md` 在输出控制 artifact 前允许一次有界收尾：仅当当前任务要求的报告或其他附件尚未落盘时写入当前 attempt 的 attachments；无需或已经完成时跳过，不得继续业务任务或修改 workspace。
- acceptance pass/fail 映射：
  - pass：`next.type="end"`。
  - fail：`next.type="single"` 或 `fanout` 创建修复节点。

完成标准：

- bootstrap prompt 保持当前 turn 内联控制；worker / acceptance prompt 明确业务 turn 只执行或立即交回，hidden finalize turn 才规划并输出控制 JSON。

### 任务 5：让 acceptance 接入 dynamic proposal 流程

涉及文件：

- 修改 `src/app/orchestrator.rs`
- 按需补充 `src/dynamic.rs` schema/校验测试

实现要点：

- `DynamicNodeKind::Acceptance` 构建 invocation 时启用 `dynamic_output_contract`。
- acceptance provider success 后读取并校验 `dynamic-node-completion`。
- acceptance 输出 `next.end` 时关闭 group。
- acceptance 输出 `single/fanout` 时 materialize 后续节点，并保持 group 未关闭。
- merge 保持无 output contract。

完成标准：

- acceptance 可以通过 `dynamic-node-completion` 决定结束或继续修复。
- 非法 acceptance proposal 进入 repair 流程。

### 任务 6：新增 DynamicContextProjection 构建层

涉及文件：

- 修改 `src/app/orchestrator.rs`
- 按需新增 projection 辅助结构或内部 DTO

实现要点：

- 以当前 `DynamicGraphState` 和 `DynamicNodeState` 构建 `DynamicContextProjection`。
- 替换现有分散的 `dynamic_graph_summary`、`dynamic_upstream_refs_summary`、`dynamic_kind_specific_summary` 字符串拼装。
- 投影视图区分 direct predecessors、active group、inherited group、siblings、available attachments。
- 对 AI-DYNAMIC predecessor context 不再设置 `output_artifact`。

完成标准：

- AI-DYNAMIC prompt 中不再出现 `dynamic-node-completion` artifact 路径。
- group 内 / group 后 single / nested fanout 均由统一 projection 渲染。

### 任务 7：实现 AI-DYNAMIC attachments manifest

涉及文件：

- 修改 `src/app/orchestrator.rs`
- 按需修改 prompt 模板

实现要点：

- 读取 dynamic node `attachments/` 目录，生成可消费附件清单。
- 沿 accepted proposal materialization source 接力链最多回溯 5 个节点；显式 `dependsOn` 只取直接节点；group 证据仅为 merge / acceptance 加入当前 group terminal/root branch 输入，并为相关 active / inherited group 加入历史最新 acceptance 及其 `dependsOn` merge，尚无 acceptance 时取最新 merge，不按节点状态或结果过滤历史附件。
- 三类来源按 nodeId 去重；每个来源节点最多检查 10 个文件或空目录末端项，含内容的目录不单独计数；超出时展示完整 `attachments` 目录提示，不读取附件正文。
- group 历史证据从 canonical node / accepted proposal 历史解析，不依赖 repair 时会重置的当前 `mergeNodeId / acceptanceNodeId`。
- repair / reaccept 不展示旧 fanout siblings；只有当前 chain 映射到 group root branch 时才展示同批其他 roots。

完成标准：

- attachments 可以在 AI-DYNAMIC 内部节点之间传递业务证据。
- artifacts 不作为 prompt 上下文展示。

### 任务 8：清理重复上下文

涉及文件：

- 修改 `src/app/orchestrator.rs`
- 按需修改 prompt 模板

实现要点：

- merge / acceptance node task 不再拼接大段 branch workspace 详情。
- group / terminal / branch workspace 详情统一进入 AI-DYNAMIC context projection。
- 删除 `upstream refs` 中的 control artifact 路径。
- 删除 hidden context 中重复 current task 的字段。

完成标准：

- 同类 dynamic context 不再同时出现在 system、hidden、task 三处。

### 任务 9：同步产品设计与开发计划文档

涉及文件：

- 修改 `docs/gold-band/产品设计文档/dsl/nodes/ai-dynamic.md`
- 修改 `docs/gold-band/开发计划/AI动态路由/AI-DYNAMIC节点方案.md`
- 保留本文档作为专项实施方案

实现要点：

- 写清 prompt 分层。
- 写清 continue 语义。
- 写清 acceptance 通过 `dynamic-node-completion` 决定 end / repair。

完成标准：

- 设计文档、开发计划、prompt 模板和 runtime 行为一致。

## 8. 测试计划

### 7.1 单元测试

必须覆盖：

- AI-DYNAMIC system prompt 不包含动态 invocation 字段：
  - attempt dir
  - graph summary
  - remaining budget
  - resumable sessions
- AI-DYNAMIC system prompt 按 emission mode 区分：
  - bootstrap 的 `InlineControl` 展示当前 turn 控制协议
  - worker / acceptance 的 `PostTurnProjection` 展示后置控制规则，不出现 `dynamic-node-completion` 或 `next.type`
  - merge 的无 emission mode 分支只展示纯执行规则
- PostTurn 业务 turn 明确禁止提前拆分任务、选择 Agent 或规划/执行后继节点，中英文语义一致。
- PostTurn hidden finalize 中英文模板允许且只允许补齐尚缺的当前 attempt attachments，并继续禁止业务 workspace 修改；AI-DYNAMIC 另可只读明确声明的协调快照。
- AI-DYNAMIC hidden context 包含：
  - 当前 node id/title/kind/task
  - workspace mode/path/capability
  - remaining budget
  - graph summary
  - resumable sessions
  - continueFromNodeId
- `sessionMode=continue` 时 user prompt 包含当前节点 task，不只包含 generic goal。
- merge invocation 不启用 `dynamic_output_contract`。
- acceptance invocation 启用 `dynamic_output_contract`。
- acceptance 输出 `next.end` 后 group closed。
- acceptance 输出 `next.single` 后 materialize 修复节点，group 不 closed。
- acceptance 输出非法 JSON / 非法 proposal 后进入 repair 流程。
- AI-DYNAMIC prompt 不包含内部控制 artifact 路径：
  - `dynamic-node-completion`
  - `proposals/*.json`
  - `acp.raw.jsonl`
- AI-DYNAMIC hidden context 包含 attachment manifest。
- accepted source 接力链最多回溯 5 个节点，显式 `dependsOn` 只取直接节点；group 证据仅为 merge / acceptance 加入当前 group terminal/root branch 输入，并为相关 group 取历史最新 acceptance 及其 `dependsOn` merge，尚无 acceptance 时取最新 merge，不按节点状态或结果过滤历史附件。
- 三类来源按 nodeId 去重；每来源节点最多检查 10 个文件或空目录末端项，含内容的目录不单独计数；超出时明确展示完整 `attachments` 目录提示，附件正文不进入 prompt。
- parallel worker 不展示未命中三类来源的 sibling attachments；repair / reaccept chain 不显示旧 fanout roots。
- merge / acceptance 都通过 group 证据看到当前 group terminal/root branch 原始附件；merge 的 terminal 输入与显式 `dependsOn` 跨分类去重，acceptance 另通过显式依赖看到 merge 附件。
- group 后 single 与 single 后 single 都沿 source 链继承，并保留 inherited group 最近一轮 merge / acceptance 证据。
- nested fanout 展示 active group 详细信息和 parent group 摘要。
- Dynamic root 在同一 hidden context 中只声明一次；node 相对 Dynamic root、attempt 相对 node、attachments 相对 attempt，coordination snapshot 相对 Dynamic root。
- fanout 集成回归同时验证普通 worker 的业务 hidden context 与 hidden finalize context 使用同一 `Dynamic root + coordination-snapshot.json` locator 契约，且 merge 业务 turn 不注入协调快照。
- 附件与 branch workspace 路径树覆盖任意深度分叉、单子链压缩、祖先条目、不同父目录同名叶子和稳定排序。
- 附件或 branch workspace 无法相对 `pathRoot` 时渲染 `absolutePath=<完整路径>`；该值可直接使用且不得再拼接 root，不因压缩被遗漏或错误归组。

### 7.2 回归场景

- continue 场景：`goodbye-step` 复用 `hello-step` session，但当前任务仍是 goodbye。
- fanout / merge / accept 场景：merge 只合并，acceptance 决定 end 或继续修复。
- 普通 workflow prompt 分层保持不变。
- runtime repair 不注入 hidden context。

### 7.3 验证命令

后端：

```bash
cargo test
```

前端如涉及 hidden prompt 展示：

```bash
npm run web:test
npm run web:build
```

涉及 UI / 会话展示时，还必须启动前端并检查 hidden prompt 展示不重复、不混乱。

## 9. 验收标准

- [x] AI-DYNAMIC stable system prompt 不再承载动态运行事实。
- [x] AI-DYNAMIC user hidden context 承载完整本次 invocation 动态事实。
- [x] AI-DYNAMIC 内部节点不渲染普通 workflow predecessor hidden context，前序语义由 `DynamicContextProjection` 统一表达。
- [x] `sessionMode=continue` 明确执行当前节点 task，并说明 continue 来源只用于会话复用。
- [x] 外层 `globalGoal` 作为每个内部节点的用户提示约束进入独立 `# 用户提示` / `# User Tips`。
- [x] worker 继续保持“按 profile 执行任务 + 最终输出 `dynamic-node-completion`”模式。
- [x] merge 不强制输出控制协议。
- [x] acceptance 强制输出 `dynamic-node-completion`，并可决定 end 或继续修复。
- [x] hidden finalize 可有界补齐尚缺的报告或其他 attachments，不把 finalize 扩展为第二个业务 turn。
- [x] dynamic context 重复注入显著减少，branch/group/workspace 信息有唯一权威来源。
- [x] AI-DYNAMIC runtime context 由 `DynamicContextProjection` 或等价投影层统一生成。
- [x] AI-DYNAMIC prompt 不展示内部控制 artifact，只展示可消费 attachments。
- [x] group 内、group 后 single、single 后 single、nested fanout 的上下文投影规则已按三类来源实现并写入回归测试：accepted source 接力链最多 5 节点、显式 `dependsOn` 直接节点、merge / acceptance 当前 group terminal/root 输入、相关 group 最新 acceptance 与其 merge 配对且历史附件不按状态过滤；同时覆盖跨类 nodeId 去重、每来源节点 10 个文件或空目录末端项上限、非空目录递归、完整目录提示，以及 repair / reaccept 不显示旧 fanout roots。
- [x] hidden context 只重复声明一次 Dynamic root，并按 node→attempt→attachments 的逐级相对关系展示运行目录；附件与 branch workspace 按路径组件做任意深度树形分组和单子链压缩，跨根路径安全回退为可直接使用的 `absolutePath=` locator。
- [x] 中英文 prompt 同步维护。
- [x] 产品设计文档与开发计划同步更新。
- [x] 后端单元测试覆盖 prompt 分层、continue、acceptance end/repair、repair 流程。

## 10. 非目标

- 不优化 fanout 高质量编排策略。
- 不拆分 runtime，不新增独立 router/planner 节点。
- 不改变普通 workflow prompt 分层。
- 不改变 `dynamic-node-completion` 作为 AI-DYNAMIC 内部控制协议唯一入口的定位。
- 不把 merge 改成控制决策节点。

## 11. 2026-09-04 附件链路投影实现记录

- 附件来源收敛为三类：accepted proposal materialization source 接力链最多回溯 5 节点；当前节点显式 `dependsOn` 的直接节点；group 证据仅为 merge / acceptance 加入当前 group terminal/root branch 输入，并为 active / inherited 相关 group 以最新 acceptance 及其 `dependsOn` merge 表达最近一轮，尚无 acceptance 时取最新 merge，历史附件不按节点状态或结果过滤，最近节点无附件时不回退到旧一轮证据。
- 三类来源按 nodeId 跨类去重；每个来源节点最多检查 10 个文件或空目录末端项，含内容的目录继续递归且不单独计数；超出时显式给出完整 `attachments` 目录并提示到目录查看；只投影文件路径，不读取附件正文。
- hidden context 的基础运行字段与附件投影字段分片构造后合并，保持同一模板变量接口，同时避免字段增长触发单个 `serde_json::json!` 宏的递归深度上限。
- acceptance repair 后 group 继续使用既有 `open` 状态，不新增 phase。历史 merge / acceptance 证据从 canonical node 与 accepted proposal 历史解析，不依赖为下一轮重置的当前 `mergeNodeId / acceptanceNodeId`。
- sibling 投影只面向当前 active fanout cohort：只有当前 chain 映射到 group root branch 时才显示同批其他 roots；repair / reaccept chain 即使处于 `open` group 也不显示旧 fanout roots。
- 修复前建立最小失败测试，覆盖 source 两跳中的 `accept-report.md` 及 5 节点边界、显式 `dependsOn` 与 source 并存、merge / acceptance 在 root 与 terminal 不同时仍可读两端原始证据、group 历史证据不受可变槽重置影响、nodeId 去重、每来源节点第 10/11 个附件目录提示，以及 repair / reaccept 不读取旧 branch 附件且不显示旧 siblings；不得用针对具体 node 名称的条件分支修补。
