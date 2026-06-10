你必须在最后一步只输出 `dynamic-node-completion` artifact 对应的 JSON 内容，不要输出解释、Markdown、代码围栏或额外文字。

{% if agent_strategy_mode == "fixed" %}
当前 AI-DYNAMIC 使用固定 agent 策略：除 `workflow-invocation` 外，所有 internal worker、merge、acceptance 节点都会由 runtime 自动使用同一个固定 agent。你不需要为任何节点输出 provider，输出中也不要包含 provider 字段。
{% else %}
当前 AI-DYNAMIC 使用动态 agent 策略：初始分发节点 agent 已由 runtime 固定；你需要根据当前 prompt 里的“节点 agent 选择说明”和“可用 providers”，为后续每个 worker / merge / acceptance 节点明确输出对应的 provider。
{% if dynamic_requires_model_in_proposal %}当前配置提供了节点 agent 选择说明，因此每个 worker / merge / acceptance 节点都必须输出 `model`。如果某个 provider 在配置中已经锁定模型，runtime 会优先使用配置模型，即使你输出了其他 model。{% else %}当前配置没有节点 agent 选择说明，因此每个可选 provider 的模型都由配置锁定；不要在 worker / merge / acceptance 节点中输出 `model`。{% endif %}
{% endif %}

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
      {% if agent_strategy_mode == "dynamic" %}
      "provider": "string，provider 标识，kind=worker 时必填；kind=workflow-invocation 时不要填",
      "model": "string，模型名称；有节点 agent 选择说明时 kind=worker 必填；没有节点 agent 选择说明时不要填；kind=workflow-invocation 时不要填",
      {% endif %}
      "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
      "sessionMode": "string，会话模式，选填，默认填 new；只有继续当前链路内可复用会话节点时才填 continue",
      "continueFromNodeId": "string，可复用会话来源节点 ID，sessionMode=continue 时必填；必须来自当前 prompt 列出的可复用会话节点",
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
        {% if agent_strategy_mode == "dynamic" %}
        "provider": "string，provider 标识，kind=worker 时必填；kind=workflow-invocation 时不要填",
        "model": "string，模型名称；有节点 agent 选择说明时 kind=worker 必填；没有节点 agent 选择说明时不要填；kind=workflow-invocation 时不要填",
        {% endif %}
        "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
        "sessionMode": "string，会话模式，选填，默认填 new；只有继续当前链路内可复用会话节点时才填 continue",
        "continueFromNodeId": "string，可复用会话来源节点 ID，sessionMode=continue 时必填；必须来自当前 prompt 列出的可复用会话节点",
        "workspace": {
          "mode": "string，工作区模式，选填，通常填 readonly / worktree / main"
        },
        "dependsOn": ["string，依赖的已有 dynamic 节点 ID，选填"],
        "workflowId": "string，允许调用的 workflow DSL ID，kind=workflow-invocation 时必填；必须匹配当前 prompt 中 allowed workflow snapshots 列表里的 workflowId"
      }
    ],
    "merge": {
      "title": "string，merge 节点标题，必填",
      {% if agent_strategy_mode == "dynamic" %}
      "provider": "string，merge provider，必填，且必须可用",
      "model": "string，merge 模型名称；有节点 agent 选择说明时必填；没有节点 agent 选择说明时不要填",
      {% endif %}
      "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
      "task": "string，merge 任务说明，必填"
    },
    "acceptance": {
      "title": "string，acceptance 节点标题，必填",
      {% if agent_strategy_mode == "dynamic" %}
      "provider": "string，acceptance provider，必填，且必须可用",
      "model": "string，acceptance 模型名称；有节点 agent 选择说明时必填；没有节点 agent 选择说明时不要填",
      {% endif %}
      "profile": "string，已注册 profile ID，选填；如果填写，必须存在",
      "task": "string，acceptance 任务说明，必填"
    }
  }
}
```

约束提醒：
{% if agent_strategy_mode == "fixed" %}- 固定 agent 策略下，不要输出任何 `provider` 字段；runtime 会自动填充固定 agent。
{% else %}- 动态 agent 策略下，所有 `worker / merge / acceptance` 都必须输出合法 provider，且必须符合当前 prompt 给出的节点 agent 选择说明。
- 有节点 agent 选择说明时，对应 `worker / merge / acceptance` 必须输出 `model`；如果该 provider 配置中已经锁定模型，runtime 会优先使用配置模型。
- 没有节点 agent 选择说明时，不要输出 `model`；runtime 会使用每个 provider 配置中锁定的模型。
- `workflow-invocation` 不要输出 `provider`。
{% endif %}- `next.type="end"` 时，`next` 中不要再放 `node / groupId / nodes / merge / acceptance`。
- `next.type="single"` 时，必须提供完整的 `next.node`，不要提供 `groupId / nodes / merge / acceptance`。
- `next.type="fanout"` 时，必须同时提供 `groupId / nodes / merge / acceptance`。
- `profile` 如果填写，必须是当前 prompt 中列出的可用 profile 之一。
{% if agent_strategy_mode == "dynamic" %}- `provider` 如果填写，必须是当前 prompt 中列出的可用 provider 之一。
{% endif %}- `sessionMode` 不填时按 `new` 处理；只有要继续当前链路内可复用会话节点时才填 `continue`。
- `sessionMode="continue"` 时必须填写 `continueFromNodeId`，且只能引用当前 prompt 列出的可复用会话节点。
- `workflow-invocation` 不要使用 `sessionMode="continue"`。
- `workflowId` 如果填写，必须是当前 prompt 中列出的 allowed workflow DSL ID 之一。
- fanout 的节点数量不能超过当前 prompt 给出的 `maxFanout` / 剩余预算约束。
- 不要输出伪代码、说明文字或示例包裹语；只输出最终 JSON。
