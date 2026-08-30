你必须在最后一步只输出 `dynamic-node-completion` artifact 对应的 JSON 内容，不要输出解释、Markdown、代码围栏或额外文字。

{% if agent_strategy_mode == "fixed" %}
当前 AI-DYNAMIC 使用固定 agent 策略：除 `workflow-invocation` 外，所有 internal worker、merge、acceptance 节点都会由 runtime 自动使用同一个固定 provider。你不需要为任何节点输出 provider，输出中也不要包含 provider 字段。
{{ model_policy }}
{% else %}
当前 AI-DYNAMIC 使用动态 agent 策略：你只需要根据当前 prompt 里的“节点 agent 选择说明”和“可用 providers”，为后续 worker 明确输出 provider。merge / acceptance 固定由初始分发 Agent 执行，不要为它们输出 provider。任何节点都不要输出 `model` 或 `permissionMode`；runtime 会读取预配置。
{{ model_policy }}
{% endif %}

下面的 JSON Schema 是本次运行的有效输出协议，由 runtime 从 Rust 数据结构生成并按当前 AI-DYNAMIC 配置动态收窄。你的输出必须满足它；runtime 也会使用同一份 schema 做校验和 repair 诊断。

```json
{{ json_schema }}
```

约束提醒：
{% if agent_strategy_mode == "fixed" %}- 固定 agent 策略下，不要输出任何 `provider` 字段；runtime 会自动填充固定 agent。
{% else %}- 动态 agent 策略下，worker 必须输出合法 provider，且必须符合当前 prompt 给出的节点 agent 选择说明；`merge / acceptance` 不要输出 provider，runtime 会固定使用初始分发 Agent。
- `workflow-invocation` 不要输出 `provider`。
{% endif %}- {{ model_policy }}
- `next.type="end"` 时，`next` 中不要再放 `node / groupId / nodes / merge / acceptance`。
{% if end_summary_is_outer_handoff %}- 如果本次使用 `next.type="end"`，`summary` 必须是交给 AI-DYNAMIC 外层后继节点的完整业务交接摘要：说明已完成内容、关键结论、重要产物及仍需关注事项；不要只写路由动作或“验收通过”。
{% else %}- 如果本次使用 `next.type="end"`，`summary` 是内部进度/分支报告，准确说明本节点完成内容，供 Runtime 报告清单和上层 group 使用。
{% endif %}
- `next.type="single"` 时，必须提供完整的 `next.node`，不要提供 `groupId / nodes / merge / acceptance`。
- 不要为任何节点输出 `workspace`、workspace mode、路径或分支；runtime 独占工作空间分配权。
- `next.type="single"` 会自动继承当前节点的实际 workspace。
- `next.type="fanout"` 时，必须同时提供 `groupId / nodes / merge / acceptance`，且 `nodes` 至少包含两个分支；只有一个后继节点时使用 `next.type="single"`。
- `next.type="fanout"` 的每个 child 会自动获得隔离 worktree；merge 与 acceptance 自动回到该 group 的父 workspace。
- `profile` 只允许在 worker 节点中使用，选填；如果填写，必须使用 schema enum 或当前 prompt 中 `profileId=...` 后面的 ID，不要填写 displayName。
- `merge` / `acceptance` 不要输出 `profile`；它们统一使用 runtime 内置的 AI-DYNAMIC merge / acceptance prompt。
{% if agent_strategy_mode == "dynamic" %}- `provider` 如果填写，必须是 schema enum 或当前 prompt 中列出的可用 provider 之一。
{% endif %}- `sessionMode` 不填时按 `new` 处理；只有要继续当前链路内可复用会话节点时才填 `continue`。
- `sessionMode="continue"` 时必须填写 `continueFromNodeId`，且只能引用当前 prompt 列出的可复用会话节点。
- `workflow-invocation` 不要使用 `sessionMode="continue"`。
- `workflowId` 如果填写，必须是 schema enum 或当前 prompt 中列出的 allowed workflow DSL ID 之一。
- fanout 的节点数量必须满足 schema `minItems/maxItems`、当前 prompt 给出的 `maxFanout` 和剩余预算约束。
- 不要输出伪代码、说明文字或示例包裹语；只输出最终 JSON。
