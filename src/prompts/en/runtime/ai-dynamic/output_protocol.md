Your final step must output only the JSON content for the `dynamic-node-completion` artifact. Do not output explanations, Markdown, code fences, or any extra text.

{% if agent_strategy_mode == "fixed" %}
This AI-DYNAMIC node uses the fixed-agent strategy: except for `workflow-invocation`, all internal worker, merge, and acceptance nodes will use the same fixed agent chosen by runtime. Do not output provider fields for any node.
{% else %}
This AI-DYNAMIC node uses the dynamic-agent strategy: the bootstrap agent is already fixed by runtime, but for later worker / merge / acceptance nodes you must choose and output the provider for each node based on the routing guidance and available providers in this prompt.
{% if dynamic_requires_model_in_proposal %}Because routing guidance is configured, every worker / merge / acceptance node must output `model`. If a provider already has a configured model, runtime will use the configured model even if you output a different model.{% else %}Because routing guidance is empty, every available provider has a configured model. Do not output `model` for worker / merge / acceptance nodes.{% endif %}
{% endif %}

Below are scenario-based examples. The strings inside the JSON include type / meaning / requiredness hints for the model. Runtime validation remains the final source of truth.

## Scenario 1: end the current chain with `next.type="end"`

```json
{
  "version": "string, version, required, must be 0.1",
  "kind": "string, artifact kind, required, must be dynamic-node-completion",
  "status": "string, execution status, required, must be success",
  "summary": "string, summary of what this node completed, required",
  "next": {
    "type": "string, next action type, required, fixed to end in this scenario"
  }
}
```

## Scenario 2: create exactly one successor with `next.type="single"`

```json
{
  "version": "string, version, required, must be 0.1",
  "kind": "string, artifact kind, required, must be dynamic-node-completion",
  "status": "string, execution status, required, must be success",
  "summary": "string, summary of what this node completed, required",
  "next": {
    "type": "string, next action type, required, fixed to single in this scenario",
    "node": {
      "id": "string, node ID, required, must be unique",
      "kind": "string, node kind, required, must be worker / workflow-invocation",
      "title": "string, node title, required",
      "task": "string, node task instruction, required",
      {% if agent_strategy_mode == "dynamic" %}
      "provider": "string, provider ID, required when kind=worker; omit when kind=workflow-invocation",
      "model": "string, model name; required for kind=worker when routing guidance is configured; omit when routing guidance is empty; omit when kind=workflow-invocation",
      {% endif %}      "profile": "string, registered profile ID, optional; if present it must exist",
      "sessionMode": "string, session mode, optional, defaults to new; use continue only when resuming a reusable session node listed in this prompt for the current chain",
      "continueFromNodeId": "string, reusable session source node ID, required when sessionMode=continue; it must come from the resumable session node list in this prompt",
      "workspace": {
        "mode": "string, workspace mode, optional, usually readonly / worktree / main"
      },
      "dependsOn": ["string, dependency node IDs that already exist in dynamic graph, optional"],
      "workflowId": "string, allowed workflow DSL ID, required when kind=workflow-invocation; it must match a workflowId listed in the allowed workflow snapshots in this prompt"
    }
  }
}
```

## Scenario 3: create a fan-out group with `next.type="fanout"`

```json
{
  "version": "string, version, required, must be 0.1",
  "kind": "string, artifact kind, required, must be dynamic-node-completion",
  "status": "string, execution status, required, must be success",
  "summary": "string, summary of what this node completed, required",
  "next": {
    "type": "string, next action type, required, fixed to fanout in this scenario",
    "groupId": "string, fanout group ID, required, must be unique",
    "nodes": [
      {
        "id": "string, fanout child node ID, required, must be unique",
        "kind": "string, node kind, required, must be worker / workflow-invocation",
        "title": "string, node title, required",
        "task": "string, node task instruction, required",
        {% if agent_strategy_mode == "dynamic" %}
        "provider": "string, provider ID, required when kind=worker; omit when kind=workflow-invocation",
        "model": "string, model name; required for kind=worker when routing guidance is configured; omit when routing guidance is empty; omit when kind=workflow-invocation",
        {% endif %}
        "profile": "string, registered profile ID, optional; if present it must exist",
        "sessionMode": "string, session mode, optional, defaults to new; use continue only when resuming a reusable session node listed in this prompt for the current chain",
        "continueFromNodeId": "string, reusable session source node ID, required when sessionMode=continue; it must come from the resumable session node list in this prompt",
        "workspace": {
          "mode": "string, workspace mode, optional, usually readonly / worktree / main"
        },
        "dependsOn": ["string, dependency node IDs that already exist in dynamic graph, optional"],
        "workflowId": "string, allowed workflow DSL ID, required when kind=workflow-invocation; it must match a workflowId listed in the allowed workflow snapshots in this prompt"
      }
    ],
    "merge": {
      "title": "string, merge node title, required",
      {% if agent_strategy_mode == "dynamic" %}
      "provider": "string, merge provider, required, and must be available",
      "model": "string, merge model name; required when routing guidance is configured; omit when routing guidance is empty",
      {% endif %}
      "profile": "string, registered profile ID, optional; if present it must exist",
      "task": "string, merge task instruction, required"
    },
    "acceptance": {
      "title": "string, acceptance node title, required",
      {% if agent_strategy_mode == "dynamic" %}
      "provider": "string, acceptance provider, required, and must be available",
      "model": "string, acceptance model name; required when routing guidance is configured; omit when routing guidance is empty",
      {% endif %}
      "profile": "string, registered profile ID, optional; if present it must exist",
      "task": "string, acceptance task instruction, required"
    }
  }
}
```

Constraint reminders:
{% if agent_strategy_mode == "fixed" %}- Under the fixed-agent strategy, do not output any `provider` fields. Runtime injects the fixed agent automatically.
{% else %}- Under the dynamic-agent strategy, every `worker / merge / acceptance` must output a valid provider and it must follow the routing guidance in this prompt.
- When routing guidance is configured, every matching `worker / merge / acceptance` must output `model`; if that provider has a configured model, runtime still prefers the configured model.
- When routing guidance is empty, do not output `model`; runtime uses the configured model for each provider.
- Do not output `provider` for `workflow-invocation`.
{% endif %}- When `next.type="end"`, do not include `node / groupId / nodes / merge / acceptance`.
- When `next.type="single"`, you must provide a complete `next.node`, and you must not provide `groupId / nodes / merge / acceptance`.
- When `next.type="fanout"`, you must provide `groupId / nodes / merge / acceptance` together.
- If `profile` is present, it must be one of the available profiles listed in this prompt.
{% if agent_strategy_mode == "dynamic" %}- If `provider` is present, it must be one of the available providers listed in this prompt.
{% endif %}- If `sessionMode` is omitted, it is treated as `new`; use `continue` only when resuming a reusable session node in the current chain.
- When `sessionMode="continue"`, you must provide `continueFromNodeId`, and it must reference one of the resumable session nodes listed in this prompt.
- Do not use `sessionMode="continue"` for `workflow-invocation`.
- If `workflowId` is present, it must be one of the allowed workflow DSL IDs listed in this prompt.
- Fanout node count must stay within the `maxFanout` and remaining-budget constraints shown in this prompt.
- Output only the final JSON. Do not output pseudocode, commentary, or wrapped examples.
