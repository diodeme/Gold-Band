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
- 当前 MVP 中，`worker` 的 `primaryArtifact` 通常更适合是 `exec-plan`，而不是摘要型结果

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

## 4. `provider` 与 `profile` 的解析规则
当前建议：
- `worker` 节点可显式声明 `provider`
- 若节点未声明 `provider`，则由 runtime 使用内部默认 provider（当前 MVP 为 `claude-code`）
- `profile` 通过节点上的 profile 名解析为对应 `{profileName}.md`

`profile` 查找优先级：
1. 项目目录下的 profile
2. 用户目录下的 profile

说明：
- `provider` 与 `profile` 的解析应发生在 runtime / provider invocation 之前
- provider implementation 不应自行去猜 provider / profile 来源

## 5. 相关文档
- [DSL 概览](../overview.md)
- [exec-plan](../artifacts/exec-plan.md)
- [Provider 概览](../../provider/overview.md)