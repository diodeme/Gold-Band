你必须在最后一步只输出 `dynamic-node-completion` artifact 对应的 JSON 内容，不要输出解释、Markdown、代码围栏或额外文字。

下面按场景给出输出示例。注意：这些示例中的字符串已经同时包含“类型 / 含义 / 必填性”说明，用于帮助你理解输出协议；真正的合法性仍以 runtime 校验为准。

## 场景 1：当前链路结束，用 `next.type="end"`

```json
{
  "version": "string，版本号，必填，固定填 0.1",
  "kind": "string，artifact 类型，必填，固定填 dynamic-node-completion",
  "status": "string，执行状态，必填，固定填 success",
  "summary": "string，本节点本次完成情况摘要，必填",
  "next": {
    "type": "string，后续动作类型，必填，当前场景固定填 end"
  }
}
```

## 场景 2：只创建一个后继节点，用 `next.type="single"`

```json
{
  "version": "string，版本号，必填，固定填 0.1",
  "kind": "string，artifact 类型，必填，固定填 dynamic-node-completion",
  "status": "string，执行状态，必填，固定填 success",
  "summary": "string，本节点本次完成情况摘要，必填",
  "next": {
    "type": "string，后续动作类型，必填，当前场景固定填 single",
    "node": {
      "id": "string，节点 ID，必填，必须唯一",
      "kind": "string，节点类型，必填，只能是 worker / workflow-invocation",
      "title": "string，节点标题，必填",
      "task": "string，节点任务说明，必填",
      "provider": "string，provider 标识，kind=worker 时必填；kind=workflow-invocation 时不要填",
      "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
      "workspace": {
        "mode": "string，工作区模式，选填，通常填 readonly / worktree / main"
      },
      "dependsOn": ["string，依赖的已有 dynamic 节点 ID，选填"],
      "workflowId": "string，允许调用的 workflow DSL ID，kind=workflow-invocation 时必填；必须匹配当前 prompt 中 allowed workflow snapshots 列表里的 workflowId"
    }
  }
}
```

## 场景 3：创建 fan-out 分组，用 `next.type="fanout"`

```json
{
  "version": "string，版本号，必填，固定填 0.1",
  "kind": "string，artifact 类型，必填，固定填 dynamic-node-completion",
  "status": "string，执行状态，必填，固定填 success",
  "summary": "string，本节点本次完成情况摘要，必填",
  "next": {
    "type": "string，后续动作类型，必填，当前场景固定填 fanout",
    "groupId": "string，fanout 分组 ID，必填，必须唯一",
    "nodes": [
      {
        "id": "string，fanout 子节点 ID，必填，必须唯一",
        "kind": "string，节点类型，必填，只能是 worker / workflow-invocation",
        "title": "string，节点标题，必填",
        "task": "string，节点任务说明，必填",
        "provider": "string，provider 标识，kind=worker 时必填；kind=workflow-invocation 时不要填",
        "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
        "workspace": {
          "mode": "string，工作区模式，选填，通常填 readonly / worktree / main"
        },
        "dependsOn": ["string，依赖的已有 dynamic 节点 ID，选填"],
        "workflowId": "string，允许调用的 workflow DSL ID，kind=workflow-invocation 时必填；必须匹配当前 prompt 中 allowed workflow snapshots 列表里的 workflowId"
      }
    ],
    "merge": {
      "title": "string，merge 节点标题，必填",
      "provider": "string，merge provider，必填，且必须可用",
      "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
      "task": "string，merge 任务说明，必填"
    },
    "acceptance": {
      "title": "string，acceptance 节点标题，必填",
      "provider": "string，acceptance provider，必填，且必须可用",
      "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
      "task": "string，acceptance 任务说明，必填"
    }
  }
}
```

约束提醒：
- `next.type="end"` 时，`next` 中不要再放 `node / groupId / nodes / merge / acceptance`。
- `next.type="single"` 时，必须提供完整的 `next.node`，不要提供 `groupId / nodes / merge / acceptance`。
- `next.type="fanout"` 时，必须同时提供 `groupId / nodes / merge / acceptance`。
- `profile` 如果填写，必须是当前 prompt 中列出的可用 profile 之一。
- `provider` 如果填写，必须是当前 prompt 中列出的可用 provider 之一。
- `workflowId` 如果填写，必须是当前 prompt 中列出的 allowed workflow DSL ID 之一。
- fanout 的节点数量不能超过当前 prompt 给出的 `maxFanout` / 剩余预算约束。
- 不要输出伪代码、说明文字或示例包裹语；只输出最终 JSON。
