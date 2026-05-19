# Prompt Bundle 规范

## 1. 一句话定义

`prompt bundle` 是 runtime 在调用 provider 前组装出的标准输入包。

它由两部分组成：

- `systemPrompt`
- `userPrompt`

其中热数据直接进入 prompt 正文，冷数据只暴露文件索引。

---

## 2. 设计目标

1. 让模型明确当前工作流节点的位置、角色和边界。
2. 让模型遵守 Gold Band / ACP 的文件读写规则。
3. 让节点输出约束来自工作流 DSL，而不是 provider 自行猜测。
4. 避免把大体积上下文直接塞进窗口。
5. 保证 provider 只消费已经渲染好的 prompt bundle。

---

## 3. system prompt 与 user prompt 的职责边界

### 3.1 `systemPrompt`

`systemPrompt` 负责不可协商的运行约束，回答：

> 当前是谁、处在哪个节点、前面怎么走到这里、必须遵守什么规则、最后输出必须是什么格式。

它承载：

- 当前 project / task / run / round / node / attempt 基础信息
- 当前节点的前序运行链
- 前序节点分支执行原因
- Gold Band / ACP 文件夹规则
- 当前节点 profile id 解析出的完整角色说明
- 当前节点 `output` DSL 派生出的输出约束

`systemPrompt` 不再承载旧的 `InvocationKind` 语义，也不根据 artifact 名称内置 `exec-plan` / `verify-result` 之类特殊输出规则。

### 3.2 `userPrompt`

`userPrompt` 负责本次任务输入，回答：

> 这次要做什么、当前反馈是什么、可以参考哪些冷数据。

它承载：

- 原始 `requirement`
- 当前 feedback 摘要
- 由 `worker.goal` 映射得到的 `taskInstruction`
- 冷 artifact 索引
- 冷 attachment 索引

profile 正文和 output DSL 不放在 `userPrompt` 中。

---

## 4. System Prompt 模板

```md
你正在 Gold Band runtime 中执行一个工作流节点。

当前是：
- Project: {{project_id}}
- Task: {{task_id}}
- Run: {{run_id}}
- Round: {{round_id}}
- Node: {{node_id}}
- Attempt: {{attempt_id}}

当前节点的前序运行节点：
{{predecessor_chain}}

当前节点前序节点的分支执行原因：
{{predecessor_branch_reasons}}

Gold Band 文件规则：
- 所有节点运行产物都位于：{{attempt_dir}}
- 本次节点运行中，你创建的自由文件必须写入：{{attachments_dir}}
- 不要把自由文件写到 attachments 之外。
- 当前 run 目录可读取：{{run_dir}}
- 当前 node 目录可写入：{{node_dir}}
- runtime/ACP 可能会在 node 目录下写入状态文件；你的附加文件仍只能写入 attachments。

当前节点角色：
- Profile ID: {{profile_id}}

{{profile_content}}

{{#if output_contract}}
当前节点输出约束：
- 输出 artifact: {{output_contract.artifact}}
- 输出类型: {{output_contract.kind}}

你必须在最后一步按照以下格式输出你的结果：
{{output_contract.schema}}

{{#if output_contract.success_condition}}
runtime 将使用以下条件判断节点结果：
{{output_contract.success_condition}}
{{/if}}
{{/if}}
```

说明：

- `predecessor_chain` 以执行路径形式展示，例如 `A/attempt-001 -success-> B/attempt-001 -failure-> 当前节点`。
- `predecessor_branch_reasons` 对普通节点可省略详细原因；人工 check 展示人工检查结果；节点输出检查展示 output DSL、节点输出和分支方向。
- 当前节点的输出约束只来自节点配置中的 `output` DSL；没有 `output` DSL 就不追加结构化输出格式要求。
- 冷数据正文不默认展开，只提供索引。

---

## 5. User Prompt 模板

```md
# Requirement
{{requirement_text}}

{{#if feedback_summary}}
# Current Feedback
{{feedback_summary}}
{{/if}}

{{#if task_instruction}}
# Task
{{task_instruction}}
{{/if}}

{{#if cold_artifacts.length}}
# Cold Artifact Index
{{#each cold_artifacts}}
- {{name}}: {{path}}
{{/each}}
{{/if}}

{{#if cold_attachments.length}}
# Cold Attachment Index
{{#each cold_attachments}}
- {{path}}
{{/each}}
{{/if}}
```

说明：

- `requirement_text` 是稳定任务目标。
- `taskInstruction` 对 `worker` 默认由 `worker.goal` 映射得到。
- `feedback_summary` 表达当前修复背景或上一节点失败摘要。
- 冷数据索引只给路径清单，不默认展开正文。

---

## 6. 运行时字段

### 6.1 基础信息

- `project_id`
- `task_id`
- `run_id`
- `round_id`
- `node_id`
- `attempt_id`

### 6.2 文件规则

- `run_dir`
- `round_dir`
- `node_dir`
- `attempt_dir`
- `attachments_dir`

### 6.3 前序链

- `predecessors[].round_id`
- `predecessors[].node_id`
- `predecessors[].attempt_id`
- `predecessors[].node_type`
- `predecessors[].branch_kind`
- `predecessors[].outcome`
- `predecessors[].branch_direction`
- `predecessors[].output_artifact`
- `predecessors[].branch_reason`

### 6.4 节点配置

- `profile`
- `profile_content`
- `output_contract.artifact`
- `output_contract.kind`
- `output_contract.schema`
- `output_contract.success_condition`

---

## 7. Continue session 规则

ACP 的 `systemPrompt` 只在 `session/new` 时通过 `_meta.systemPrompt.append` 注入。

当 `sessionMode = continue` 且存在 resume prompt 时：

- `systemPrompt` 为空
- `userPrompt` 为 `Continue` / `继续`
- 复用已有 ACP session 的上下文

如果跨节点需要新的角色、文件规则或输出 DSL 约束，应使用新 session。

---

## 8. 一句话总结

> `prompt bundle` 是 provider 调用前的最终模型输入层：system prompt 承载工作流运行约束和输出 DSL，user prompt 承载任务目标、反馈和冷数据索引。
