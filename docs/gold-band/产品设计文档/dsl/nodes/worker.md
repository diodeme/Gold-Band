# `worker` 节点规范

## 1. 当前定位
`worker` 节点是 DSL 中的通用 AI worker 节点。

它的行为应由以下几部分共同决定：
- `provider`
- `profile`
- `goal`
- `primaryArtifact`

也就是说：
- `worker` 是节点类型
- `provider` 是底层实现层
- `profile` 是角色预设层

## 2. 当前已知结论
- `worker` 节点是通用 AI worker 节点
- 不是所有 `worker` 节点都必须产出 `exec-plan`
- 一个 `worker` 节点一次只应有一个 `primaryArtifact`
- 只有声明 `primaryArtifact` 时，runtime 才要求生成并校验对应 canonical artifact
- 若未声明 `primaryArtifact`，runtime 不要求 canonical artifact，而只依据 provider invocation 的完成状态归纳 `success / failure / paused`
- 若未声明 `primaryArtifact`，只有 provider adapter 返回包本身不合法时，runtime 才归为 `invalid`
- 若声明了 `primaryArtifact` 但结果缺失、name 不匹配或 schema 不合法，应归为 `invalid`
- provider 执行失败或异常结束应归为 `failure`
- 新建工作流中，`worker` 不再默认产出 `exec-plan`；review/test/accept 等验证型 worker 可产出 `*-result` JSON artifact
- 当声明 `output.kind=json` 与 `successCondition` 时，runtime 按 JSON 字段值把节点归纳为 `success / failure / invalid`
- AI 输出验证与 `manual_check=true` 是互斥的结果判定方式，同一 worker 不应同时声明两者

## 3. 当前关注点
- 如何绑定 `provider`
- 如何绑定 `profile`
- 如何表达 `primaryArtifact`
- 节点输入契约如何自动组装

## 3.1 `goal` 的运行时语义
`goal` 不是纯描述性元数据。

首版规则直接固定为：
- `worker.goal` -> runtime `taskInstruction`
- `taskInstruction` -> `userPrompt` 的 `# Task`

也就是说：
- DSL 上的 `goal` 是该节点本次任务意图的 canonical 来源
- runtime 不应忽略它，也不应在没有 `goal` 的情况下自行硬造等价任务语义
- provider implementation 只消费已经映射好的 invocation / prompt，不负责反推 `goal`

## 3.2 JSON 输出验证
验证型 worker 可声明：

```json
{
  "primary_artifact": "review-result",
  "output": { "kind": "json", "artifact": "review-result" },
  "success_condition": { "path": "passed", "equals": true }
}
```

规则：
- JSON 输出验证与人工 check 二选一；声明 `output` / `success_condition` 时不应同时声明 `manual_check=true`。
- `output.artifact` 必须与 `primary_artifact` 一致。
- `output` DSL 会进入当前节点追加的 `systemPrompt`，提示 agent 最后一步按 schema 输出结果。
- 没有 `output` DSL 时，runtime 不因为 artifact 名称自动向 `systemPrompt` 注入结构化输出格式。
- `success_condition.path` 当前是简单 dot path，例如 `passed` 或 `result.passed`。
- 字段值等于 `equals` 时节点 outcome 为 `success`；不等于时为 `failure`；缺失、JSON 非法或 path 非法时为 `invalid`。

## 4. `provider` 与 `profile` 的解析规则
当前建议：
- `worker` 节点必须显式声明 `provider`
- 桌面作者态 UI 从 Agent 管理页已配置且支持的 agent card 中选择 provider
- `worker` 节点保存/运行前必须显式声明 `profile`，字段值为 profile `id`，不是角色名称
- 默认 workflow 初始化时先同步默认角色，再把生成出的 profile `id` 写入默认节点；默认 cleanup 节点是普通 worker，不声明输出验证

`profile` 查找优先级：
1. 当前项目级 profile id
2. 用户级 profile id

说明：
- `provider` 与 `profile` 的解析应发生在 runtime / provider invocation 之前
- provider implementation 不应自行去猜 provider / profile 来源
- 如果 profile id 不存在或当前项目不可见，workflow 保存/运行应失败并提示用户重新选择角色

## 5. 相关文档
- [DSL 概览](../overview.md)
- [exec-plan](../artifacts/exec-plan.md)
- [Provider 概览](../../provider/overview.md)