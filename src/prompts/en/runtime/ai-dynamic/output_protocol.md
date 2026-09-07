Your final step must output only the JSON content for the `dynamic-node-completion` artifact. Do not output explanations, Markdown, code fences, or any extra text.

{% if agent_strategy_mode == "fixed" %}
This AI-DYNAMIC node uses the fixed-agent strategy: except for `workflow-invocation`, all internal worker, merge, and acceptance nodes will use the same fixed provider chosen by runtime. Do not output provider fields for any node.
{{ model_policy }}
{% else %}
This AI-DYNAMIC node uses the dynamic-agent strategy: choose and output a provider only for later workers based on the routing guidance and available providers in this prompt. Merge / acceptance always use the bootstrap Agent, so do not output provider for them. Do not output `model` or `permissionMode` for any node; runtime reads saved configuration.
{{ model_policy }}
{% endif %}

The JSON Schema below is the effective output protocol for this run. Runtime generated it from the Rust data structures and narrowed it with the current AI-DYNAMIC configuration. Your output must satisfy it; runtime uses the same schema for validation and repair diagnostics.

```json
{{ json_schema }}
```

Constraint reminders:
- A successor task may only decompose an established in-scope outcome or repair a qualified `BLOCKER`. Do not promote a `FOLLOW_UP` or predecessor suggestion into a new outcome. Scope drift may only schedule restoration of the minimum in-scope solution.
{% if agent_strategy_mode == "fixed" %}- Under the fixed-agent strategy, do not output any `provider` fields. Runtime injects the fixed agent automatically.
{% else %}- Under the dynamic-agent strategy, workers must output a valid provider that follows the routing guidance in this prompt; `merge / acceptance` must omit provider because runtime always uses the bootstrap Agent.
- Do not output `provider` for `workflow-invocation`.
{% endif %}- {{ model_policy }}
- When `next.type="end"`, do not include `node / groupId / nodes / merge / acceptance`.
{% if end_summary_is_outer_handoff %}- If you use `next.type="end"`, `summary` must be a complete business handoff for the successor outside AI-DYNAMIC: state what was completed, key conclusions, important outputs, and any remaining concerns. Do not merely describe routing or say “accepted.”
{% else %}- If you use `next.type="end"`, `summary` is an internal progress or branch report. Accurately state what this node completed for the Runtime report manifest and the enclosing group.
{% endif %}
- When `next.type="single"`, you must provide a complete `next.node`, and you must not provide `groupId / nodes / merge / acceptance`.
- Do not output `workspace`, a workspace mode, a path, or a branch for any node. Runtime exclusively owns workspace assignment.
- A `next.type="single"` successor automatically inherits the current node's actual workspace.
- When `next.type="fanout"`, you must provide `groupId / nodes / merge / acceptance` together, and `nodes` must contain at least two branches; use `next.type="single"` for one successor node.
- Every `next.type="fanout"` child automatically receives an isolated worktree; merge and acceptance automatically return to that group's parent workspace.
- `profile` is only allowed on worker nodes and is optional. If present, use an ID from the schema enum or the ID after `profileId=...` in this prompt, not the displayName.
- Do not output `profile` for `merge` / `acceptance`; runtime uses the built-in AI-DYNAMIC merge / acceptance prompts.
{% if agent_strategy_mode == "dynamic" %}- If `provider` is present, it must be one of the available providers listed in the schema enum or this prompt.
{% endif %}- If `sessionMode` is omitted, it is treated as `new`; use `continue` only when resuming a reusable session node in the current chain.
- When `sessionMode="continue"`, you must provide `continueFromNodeId`, and it must reference one of the resumable session nodes listed in this prompt.
- Do not use `sessionMode="continue"` for `workflow-invocation`.
- If `workflowId` is present, it must be one of the allowed workflow DSL IDs listed in the schema enum or this prompt.
- Fanout node count must satisfy the schema `minItems/maxItems`, `maxFanout`, and remaining-budget constraints shown in this prompt.
- Output only the final JSON. Do not output pseudocode, commentary, or wrapped examples.
