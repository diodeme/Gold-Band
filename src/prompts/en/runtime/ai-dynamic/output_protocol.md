Your final step must output only the JSON content for the `dynamic-node-completion` artifact. Do not output explanations, Markdown, code fences, or any extra text.

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
      "provider": "string, provider ID, required when kind=worker; omit when kind=workflow-invocation",
      "profile": "string, registered profile ID, optional; if present it must exist",
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
        "provider": "string, provider ID, required when kind=worker; omit when kind=workflow-invocation",
        "profile": "string, registered profile ID, optional; if present it must exist",
        "workspace": {
          "mode": "string, workspace mode, optional, usually readonly / worktree / main"
        },
        "dependsOn": ["string, dependency node IDs that already exist in dynamic graph, optional"],
        "workflowId": "string, allowed workflow DSL ID, required when kind=workflow-invocation; it must match a workflowId listed in the allowed workflow snapshots in this prompt"
      }
    ],
    "merge": {
      "title": "string, merge node title, required",
      "provider": "string, merge provider, required, and must be available",
      "profile": "string, registered profile ID, optional; if present it must exist",
      "task": "string, merge task instruction, required"
    },
    "acceptance": {
      "title": "string, acceptance node title, required",
      "provider": "string, acceptance provider, required, and must be available",
      "profile": "string, registered profile ID, optional; if present it must exist",
      "task": "string, acceptance task instruction, required"
    }
  }
}
```

Constraint reminders:
- When `next.type="end"`, do not include `node / groupId / nodes / merge / acceptance`.
- When `next.type="single"`, you must provide a complete `next.node`, and you must not provide `groupId / nodes / merge / acceptance`.
- When `next.type="fanout"`, you must provide `groupId / nodes / merge / acceptance` together.
- If `profile` is present, it must be one of the available profiles listed in this prompt.
- If `provider` is present, it must be one of the available providers listed in this prompt.
- If `workflowId` is present, it must be one of the allowed workflow DSL IDs listed in this prompt.
- Fanout node count must stay within the `maxFanout` and remaining-budget constraints shown in this prompt.
- Output only the final JSON. Do not output pseudocode, commentary, or wrapped examples.
