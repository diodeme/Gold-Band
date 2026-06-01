# AI-DYNAMIC 节点

## 1. 一句话定义
`ai-dynamic` 是普通 workflow 中的复合节点：外层 runtime 仍按固定 DSL 前进，进入该节点后由内部 dynamic graph 根据 `dynamic-node-completion` artifact 派生后续内部节点、fanout group、merge 和 acceptance。

## 2. DSL 结构

```json
{
  "id": "router",
  "type": "ai-dynamic",
  "provider": "claude-acp",
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
  ],
  "merge": {
    "provider": "claude-acp"
  },
  "acceptance": {
    "provider": "claude-acp"
  }
}
```

## 3. 关键语义
- `provider` 是 fan-out agent 的 provider，用于 bootstrap internal worker；fan-out agent 的角色与目标由 runtime 内置 prompt 提供，不在 DSL 中配置。
- `control` 是 runtime validation 的硬限制，不只是 prompt 提示。
- `allowedWorkflows.workflowId` 引用 workflow DSL 内的 `workflow.id`，不是模板外层 `template.id`；run start 时冻结为 allowed workflow snapshots。
- `allowedWorkflows` 引用的模板必须满足模板库级唯一性约束：若某个模板的 `workflow.id` 与其他模板重复，则任何包含该模板引用的 AI-DYNAMIC 工作流都不能保存，用户需手动修改模板 JSON 中的 `workflow.id` 后再试。
- `maxGroupDepth` 限制 fanout group 的嵌套深度；底层状态通过 `parentGroupId` 记录父子 group，子 group closed 后把自己的 acceptance 节点挂入父 group terminal，父 group 必须等所有 root chain 都到达 terminal boundary 后才会 merge。
- `merge` 和 `acceptance` 只配置 provider；角色由 `src/prompts/<lang>/runtime/ai-dynamic/merge.md`、`src/prompts/<lang>/runtime/ai-dynamic/acceptance.md` 提供，runtime 创建节点并通过 minijinja 渲染把 group/worktree/terminal/child-run 等上下文与当前剩余预算注入 system prompt，把当前 goal 注入 user prompt。
- 外层 edge 仍然只消费 `ai-dynamic` 的最终 `success / failure / killed` outcome。

## 4. 内部控制 artifact
内部 worker 必须输出 canonical artifact：

```text
dynamic-node-completion
```

V1 支持：
- `next.type=end`
- `next.type=single`
- `next.type=fanout`

workflow invocation 节点完成 child run 后由 runtime 包装 `dynamic-node-completion`，避免固定 child workflow 混入 dynamic 控制语义。

## 5. V1 边界
- 不支持 nested `ai-dynamic`，除非后续显式打开 `allowNestedDynamic`。
- 不引入 direct mode、route-decision、triage-result 或 replan artifact。
- 内部状态保存在外层节点 attempt 的 `dynamic/` 目录下，不写入外层 round trace。
- invalid proposal、internal node failure、merge failure 会让外层 run 进入 error-blocked pause。
