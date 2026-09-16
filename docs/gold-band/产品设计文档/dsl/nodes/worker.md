# `worker` 节点规范

## 1. 当前定位
`worker` 节点是 DSL 中的通用 AI worker 节点。

它的行为应由以下几部分共同决定：
- `provider`
- `profile`
- `goal`
- `output`

也就是说：
- `worker` 是节点类型
- `provider` 是底层实现层
- `profile` 是角色预设层

## 2. 当前已知结论
- `worker` 节点是通用 AI worker 节点
- 不是所有 `worker` 节点都必须产出 canonical artifact
- 一个 `worker` 节点一次只应有一个 `output.artifact`
- 只有声明 `output` 时，runtime 才要求生成并校验对应 canonical artifact
- 若未声明 `output`，runtime 不要求 canonical artifact，而只依据 provider invocation 的完成状态归纳 `success / failure / paused`
- 若未声明 `output.schema`，runtime 不触发结构化输出自修复
- 若声明了 `output.schema`，JSON 非法或 schema 不合法仍走同 attempt 的隐藏修复；完全没有 artifact 则走独立、有界的 finalize 提醒，不占用 repair 次数
- provider 执行失败或异常结束应归为 `failure`
- 新建工作流中，`worker` 不再默认产出 `节点输出产物`；review/test/accept 等验证型 worker 可产出 `*-result` JSON artifact
- 当声明 `output.kind=json` 与 `successCondition` 时，runtime 按 JSON 字段值把节点归纳为 `success / failure`；schema 输出不合法属于内部 `invalid` 状态，不作为 edge outcome
- AI 输出验证与 `manual_check=true` 是互斥的结果判定方式，同一 worker 不应同时声明两者

## 3. 当前关注点
- 如何绑定 `provider`
- 如何绑定 `profile`
- 如何表达 `output.artifact`
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
  "output": { "kind": "json", "artifact": "review-result" },
  "success_condition": { "path": "passed", "equals": true }
}
```

规则：
- JSON 输出验证与人工 check 二选一；声明 `output` / `success_condition` 时不应同时声明 `manual_check=true`。
- `output.artifact` 是当前节点 canonical artifact 的唯一逻辑名来源。
- `output` DSL 使用 PostTurnProjection：业务首轮不注入输出协议，首次正常结束后通过 hidden finalize 提供协议。
- 没有 `output` DSL 时，runtime 不因为 artifact 名称自动向 `systemPrompt` 注入结构化输出格式。
- 没有 `output` 时，runtime 会在 `systemPrompt` 明确告知 agent 不需要产出 canonical artifact，也不需要查找、推断或读取 artifact/output 约束。
- `success_condition.path` 当前是简单 dot path，例如 `passed` 或 `result.passed`。
- 字段值等于 `equals` 时节点 outcome 为 `success`；不等于时为 `failure`；声明了 `output.schema` 且 JSON 非法或字段不合法时触发既有隐藏修复，修复耗尽后 workflow failure。没有 artifact 不等于业务失败。

### Artifact 提交与停止恢复

- 首次正常业务 `end_turn` 提供 artifact 协议；Agent 可以继续当前任务，不必因为收到协议而提前结束。
- 此后正常结束时先提取本轮候选：有候选交给既有解析、校验和结果判定；错误候选进入原有 repair；没有候选则最多追加 5 次 hidden finalize 提醒。首次提供协议不计入这 5 次。
- 第 5 次提醒后的回复仍没有 artifact 时，通过 `provider.artifact-finalize-reminders-exhausted` 进入可人工继续的运行异常暂停，不自动无限重试、不判定业务失败。显式继续开启新的 5 次提醒额度。
- 用户停止与 artifact repair 是独立操作。停止、中断、等待输入、权限请求和 provider 错误不会触发无 artifact 催交；继续使用已有 resume / 用户消息 prompt，不因 checkpoint 为 `finalizing` 而替换成 finalize 或 repair。
- `artifact-emission.json` 继续记录协议阶段及 prompt generation，不增加第二套节点生命周期。已进入 finalizing 的节点恢复后，正常结束仍可收集 artifact；自动提醒使用新 prompt identity，且不重复携带用户恢复控制意图。
- 只提取当前 provider 回复的候选，不把 attempt 目录中的旧 artifact 当作本轮新提交；没有消息定位信息的错误 JSON 也保留候选交给校验，而不是误判为缺失。
- 无 output 节点和人工 check 节点不进入该循环；人工 check 与输出验证互斥，原有人工判定流程保持不变。合法 artifact 的业务结果仍按 success condition 决定，不把业务失败当作格式修复。

## 4. `provider` 与 `profile` 的解析规则
当前建议：
- `worker` 节点必须显式声明 `provider`
- 桌面作者态 UI 从 Agent 管理页已配置且支持的 agent card 中选择 provider
- `runtime-managed` worker 保存/运行前必须显式声明 `profile`，字段值为 profile `id`，不是角色名称
- `raw-agent` worker 不参与 profile 解析，且禁止声明非空 `profile`；这保证 Direct 不会注入 Gold Band 角色或 system prompt
- 默认 workflow 初始化时先同步默认角色，再把生成出的 profile `id` 写入默认节点；默认 cleanup 节点是普通 worker，不声明输出验证

`profile` 查找优先级：
1. 客户端内建 profile id
2. 用户级 profile id

说明：
- `provider` 与 `profile` 的解析应发生在 runtime / provider invocation 之前
- provider implementation 不应自行去猜 provider / profile 来源
- 如果 `runtime-managed` worker 的 profile id 不存在，workflow 保存/运行应失败并提示用户重新选择角色
- 如果 `raw-agent` worker 绑定了 profile，workflow 保存/运行必须直接失败，不能静默忽略

## 5. 相关文档
- [DSL 概览](../overview.md)
- [节点输出产物](../artifacts/节点输出产物.md)
- [Provider 概览](../../provider/overview.md)

## 6. `prompt_envelope`

- `worker.prompt_envelope` 是冻结执行字段，枚举为 `runtime-managed | raw-agent`，缺省值为 `runtime-managed`。
- 普通用户可编辑工作流节点始终保存为 `runtime-managed`；WorkflowEditor 不暴露该字段。
- `raw-agent` 仅用于 Gold Band 创建的 Direct 内部单 Worker workflow，不代表新增用户可见节点类型。
- profile resolver 只解析 `runtime-managed` worker；`raw-agent` 必须保持 `profile = null`。
- provider invocation 必须从 workflow snapshot 读取该字段，确保首轮、runtime continue 和 completed-run follow-up 使用同一 prompt 语义。
