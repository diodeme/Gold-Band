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
    "provider": "claude-acp"
  },
  "permissionMode": "default",
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
- `agentStrategy` 中 agent 对应的 `model` 是可选字段。固定 Agent 策略下 proposal 不输出 `provider`，runtime 会为 worker / merge / acceptance 注入固定 provider；若固定 provider 未配置模型且 provider 暴露可选模型列表，runtime 会在 prompt 中要求 proposal 为 worker / merge / acceptance 输出 `model`，否则不输出 `model`。动态 Agent 策略下 proposal 必须为 worker / merge / acceptance 输出 provider；若 agent / 模型决策指南非空，普通 worker 仍必须输出 `model`，但已配置模型的 Agent 仍由 runtime 固定使用配置模型。
- 动态 Agent 策略新增可选字段 `acceptanceModel`。配置后，它只作用于 fanout 配套的 `merge` / `acceptance`：runtime 会固定使用该模型，这两个 spec 的 schema 和提示词都不再要求也不再暴露 `model` 字段；普通 worker 的 `model` 规则保持不变。
- `permissionMode` 复用普通 worker 节点的权限模式选择；作者态 DSL 中该字段仍表达统一的规范权限级别，runtime 会在 materialize bootstrap、派生 worker、merge 和 acceptance 等内部节点时按各自 provider 解析成真实 mode id 后再落盘，并在 provider 能力已知时提前校验兼容性。
- `control` 是 runtime validation 的硬限制，不只是 prompt 提示。
- `allowedWorkflows.workflowId` 引用 workflow DSL 内的 `workflow.id`，不是模板外层 `template.id`；run start 时冻结为 allowed workflow snapshots。
- `allowedWorkflows` 引用的模板必须满足模板库级唯一性约束：若某个模板的 `workflow.id` 与其他模板重复，则任何包含该模板引用的 AI-DYNAMIC 工作流都不能保存，用户需手动修改模板 JSON 中的 `workflow.id` 后再试。
- `maxParallel` 是 runtime 的真实调度上限，不是提示词建议。dynamic graph 采用补位式并行：主线程统一维护 graph 状态并按空闲槽位发射 ready node；任一 running node 完成后，主线程先回写 proposal / materialize，再立即继续补齐新的 ready node，直到达到 `maxParallel`。
- `maxGroupDepth` 限制 fanout group 的嵌套深度；底层状态通过 `parentGroupId` 记录父子 group，子 group closed 后把自己的 acceptance 节点挂入父 group terminal，父 group 必须等所有 root chain 都到达 terminal boundary 后才会 merge。
- 外层 `ai-dynamic` DSL 不再配置 `merge` 或 `acceptance`。当内部节点输出 `next.type=fanout` 时，proposal 中必须同时给出该 group 的 `merge` 与 `acceptance` 可执行 spec；fixed 策略下 provider 由 runtime 注入，dynamic 策略下 provider 由 proposal 明确给出。merge / acceptance 不接受 proposal profile，角色提示词统一由 `src/prompts/<lang>/runtime/ai-dynamic/merge.md` 与 `src/prompts/<lang>/runtime/ai-dynamic/acceptance.md` 提供；若动态策略已配置 `acceptanceModel`，merge / acceptance 也不再输出 `model`，直接由 runtime 固定注入该模型。
- 内部 worker / workflow-invocation 只能提交 `dynamic-node-completion` proposal；子线程负责执行并产出 proposal，主线程负责校验、记录 accepted/rejected proposal，并作为 graph 的唯一写入者执行 materialize。
- runtime 通过通用 output contract 机制把 artifact 名称、类型以及完整的 AI-DYNAMIC 输出协议文本注入 prompt；`dynamic-node-completion` 基础 schema 由 Rust 数据结构通过 `schemars` 生成，runtime 再按当前 Agent 策略、provider/model 需求、可用 worker profile、allowed workflow snapshot 与 `maxFanout` 收窄为本次运行的有效 JSON Schema。这份有效 schema 同时进入 provider output contract、`src/prompts/<lang>/runtime/ai-dynamic/output_protocol.md` 和 runtime validator，避免 prompt、解析与修复规则分叉。
- internal worker 在 prompt 中还会额外拿到一段“当前链路可复用会话节点”列表，只包含当前 dynamic graph、当前 chain、且位于最近 fan-out 边界之内的可继续节点；列表字段最小化为 `nodeId / title / goal`。若 proposal 中某个后继节点声明 `sessionMode=continue`，则必须同时提供 `continueFromNodeId`，并且只能引用这份列表中的 worker 节点；`workflow-invocation` 不允许继续会话。
- proposal 校验失败与非法 JSON 解析失败统一进入同一个 repair 回路：runtime 会把本轮发现的全部问题一次性回传给当前 internal worker 做隐藏修复，最多重试 3 次；耗尽后外层 AI-DYNAMIC 进入 `paused/error-blocked`。结构性错误先由有效 JSON Schema 诊断，业务图错误继续由 Rust 语义校验聚合；repair prompt 渲染结构化诊断，包含 code、path、actual、expected、allowed values、suggested repair，并附带当前合法 provider/model、worker profile ID 与 allowed workflow ID 参考。
- proposal 的业务校验会尽可能聚合错误，而不是命中第一条就返回。典型错误包括 profile 不存在、provider 不可用、fanout 超出 `maxFanout`、group depth 超出 `maxGroupDepth`、workflowId 不在 allowed snapshot、merge/acceptance spec 不完整等。
- rejected proposal 不再只保存字符串错误，而是保存结构化错误对象：至少包含 `code`、`message`、`params`，并可携带 `path / actual / expected / allowedValues / suggestion`。其中 `code` 用于稳定识别错误类型，`path` 指向 proposal JSON 路径，`params` 提供 nodeId / field / profile / provider / limit / actual 等上下文字段，便于后续 UI、日志和 prompt 复用。
- 外层 edge 仍然只消费 `ai-dynamic` 的最终 `success / failure / killed` outcome；若内部 dynamic worker、merge/acceptance 节点或 `workflow-invocation` child run 进入暂停，外层 `ai-dynamic` node 也以复合节点形式暂停，并在继续时由 runtime 委托内部 paused node 或 `childRunId` 从自身断点恢复。
- 外层 run stop 时需要递归停止 AI-DYNAMIC 内部并行节点与 child workflow run，并把可达 dynamic 状态一并收敛到 killed；应用关闭则递归把这些活跃资源收敛到 `ProcessInterrupted` paused，供后续 continue 恢复。

## 4. 内部控制 artifact
内部 worker 必须输出 canonical artifact：

```text
dynamic-node-completion
```

V1 支持：
- `next.type=end`
- `next.type=single`
- `next.type=fanout`

内部 worker 的 `profile` 为选填；不填时 runtime 不注入 worker profile 内容。`profile` 只允许出现在 worker proposal 中，必须使用可用 profile 的 id，不能使用 displayName；merge / acceptance 不输出 profile，统一使用 runtime 内置 prompt。内部 worker / merge / acceptance 的 proposal 只有在 prompt 标记 `model required in proposal` 时才输出 `model`；但若动态策略配置了 `acceptanceModel`，则该规则只继续约束普通 worker，merge / acceptance 不再输出 `model`。`workflow-invocation` 节点不输出 `provider` 或 `model`。

workflow invocation 节点完成 child run 后由 runtime 包装 `dynamic-node-completion`，避免固定 child workflow 混入 dynamic 控制语义。

## 5. V1 边界
- 不支持 nested `ai-dynamic`，除非后续显式打开 `allowNestedDynamic`。
- 不引入 direct mode、route-decision、triage-result 或 replan artifact。
- 内部状态保存在外层节点 attempt 的 `dynamic/` 目录下，不写入外层 round trace。
- invalid proposal、internal node failure、merge failure 会让外层 run 进入 error-blocked pause。
