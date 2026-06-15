Your final step must output only the JSON content for the `dynamic-node-completion` artifact. Do not output explanations, Markdown, code fences, or any extra text.

{% if agent_strategy_mode == "fixed" %}
This AI-DYNAMIC node uses the fixed-agent strategy: except for `workflow-invocation`, all internal worker, merge, and acceptance nodes will use the same fixed provider chosen by runtime. Do not output provider fields for any node.
{{ model_policy }}
{% else %}
This AI-DYNAMIC node uses the dynamic-agent strategy: the bootstrap agent is already fixed by runtime, but for later worker / merge / acceptance nodes you must choose and output the provider for each node based on the routing guidance and available providers in this prompt.
{{ model_policy }}
{% endif %}

The JSON Schema below is the effective output protocol for this run. Runtime generated it from the Rust data structures and narrowed it with the current AI-DYNAMIC configuration. Your output must satisfy it; runtime uses the same schema for validation and repair diagnostics.

```json
{{ json_schema }}
```

Constraint reminders:
{% if agent_strategy_mode == "fixed" %}- Under the fixed-agent strategy, do not output any `provider` fields. Runtime injects the fixed agent automatically.
{% else %}- Under the dynamic-agent strategy, every `worker / merge / acceptance` must output a valid provider and it must follow the routing guidance in this prompt.
- Do not output `provider` for `workflow-invocation`.
{% endif %}- {{ model_policy }}
- When `next.type="end"`, do not include `node / groupId / nodes / merge / acceptance`.
- When `next.type="single"`, you must provide a complete `next.node`, and you must not provide `groupId / nodes / merge / acceptance`.
- When `next.type="fanout"`, you must provide `groupId / nodes / merge / acceptance` together.
- `profile` is only allowed on worker nodes and is optional. If present, use an ID from the schema enum or the ID after `profileId=...` in this prompt, not the displayName.
- Do not output `profile` for `merge` / `acceptance`; runtime uses the built-in AI-DYNAMIC merge / acceptance prompts.
{% if agent_strategy_mode == "dynamic" %}- If `provider` is present, it must be one of the available providers listed in the schema enum or this prompt.
{% endif %}- If `sessionMode` is omitted, it is treated as `new`; use `continue` only when resuming a reusable session node in the current chain.
- When `sessionMode="continue"`, you must provide `continueFromNodeId`, and it must reference one of the resumable session nodes listed in this prompt.
- Do not use `sessionMode="continue"` for `workflow-invocation`.
- If `workflowId` is present, it must be one of the allowed workflow DSL IDs listed in the schema enum or this prompt.
- Fanout node count must stay within the schema `maxItems`, `maxFanout`, and remaining-budget constraints shown in this prompt.
- Output only the final JSON. Do not output pseudocode, commentary, or wrapped examples.
