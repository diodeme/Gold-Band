# Agent prompt 检查

## 当前实现口径

- profile id 会在运行时解析为完整 profile 内容，并追加到当前节点 `systemPrompt`。
- AI 输出验证的 `output` DSL 会追加到当前节点 `systemPrompt`，要求 agent 在最后一步按 schema 输出结果。
- 没有 `output` DSL 时，不再根据 `exec-plan`、`verify-result` 等 artifact 名称硬编码输出约束。
- 当前节点 `systemPrompt` 会包含 project/task/run/round/node/attempt 基础信息、前序运行链、前序分支执行原因和 Gold Band 文件规则。
- `userPrompt` 保留 requirement、feedback、taskInstruction 与冷数据索引。

## 后续检查项

- AI 输出验证节点的 JSON 合法性仍由 runtime 根据 `output` DSL 和 `success_condition` 判定。
- 如果 agent 没有返回合法 JSON，后续应补充同节点继续对话/修正机制。
- prompt 文案仍需继续做 i18n 梳理。
