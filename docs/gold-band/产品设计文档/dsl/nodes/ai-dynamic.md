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
  ]
}
```

## 3. 关键语义
- `provider` 是 fan-out agent 的 provider，用于 bootstrap internal worker；fan-out agent 的角色与目标由 runtime 内置 prompt 提供，不在 DSL 中配置。
- `control` 是 runtime validation 的硬限制，不只是 prompt 提示。
- `allowedWorkflows.workflowId` 引用 workflow DSL 内的 `workflow.id`，不是模板外层 `template.id`；run start 时冻结为 allowed workflow snapshots。
- `allowedWorkflows` 引用的模板必须满足模板库级唯一性约束：若某个模板的 `workflow.id` 与其他模板重复，则任何包含该模板引用的 AI-DYNAMIC 工作流都不能保存，用户需手动修改模板 JSON 中的 `workflow.id` 后再试。
- `maxParallel` 是 runtime 的真实调度上限，不是提示词建议。dynamic graph 采用补位式并行：主线程统一维护 graph 状态并按空闲槽位发射 ready node；任一 running node 完成后，主线程先回写 proposal / materialize，再立即继续补齐新的 ready node，直到达到 `maxParallel`。
- `maxGroupDepth` 限制 fanout group 的嵌套深度；底层状态通过 `parentGroupId` 记录父子 group，子 group closed 后把自己的 acceptance 节点挂入父 group terminal，父 group 必须等所有 root chain 都到达 terminal boundary 后才会 merge。
- 外层 `ai-dynamic` DSL 不再配置 `merge` 或 `acceptance`。当内部节点输出 `next.type=fanout` 时，proposal 中必须同时给出该 group 的 `merge` 与 `acceptance` 可执行 spec；runtime 直接使用 proposal 中的 provider/title/task 创建节点，角色仍由 `src/prompts/<lang>/runtime/ai-dynamic/merge.md` 与 `src/prompts/<lang>/runtime/ai-dynamic/acceptance.md` 提供。
- 内部 worker / workflow-invocation 只能提交 `dynamic-node-completion` proposal；子线程负责执行并产出 proposal，主线程负责校验、记录 accepted/rejected proposal，并作为 graph 的唯一写入者执行 materialize。
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

workflow invocation 节点完成 child run 后由 runtime 包装 `dynamic-node-completion`，避免固定 child workflow 混入 dynamic 控制语义。

## 5. V1 边界
- 不支持 nested `ai-dynamic`，除非后续显式打开 `allowNestedDynamic`。
- 不引入 direct mode、route-decision、triage-result 或 replan artifact。
- 内部状态保存在外层节点 attempt 的 `dynamic/` 目录下，不写入外层 round trace。
- invalid proposal、internal node failure、merge failure 会让外层 run 进入 error-blocked pause。
