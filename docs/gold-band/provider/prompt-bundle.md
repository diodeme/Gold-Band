# Prompt Bundle 规范

## 1. 一句话定义
`prompt bundle` 是 runtime 在调用 provider 前组装出的标准输入包。

它不是原始目录的直接暴露，而是 runtime 经过选择、归纳、分层后的调用材料。

它至少由两部分组成：

- `systemPrompt`
- `userPrompt`

其中上下文还应遵循：
- 热数据直接内联
- 冷数据只暴露文件索引

---

## 2. 设计目标
`prompt bundle` 的设计目标是：

1. 让模型一开始就拿到必须知道的任务目标与约束
2. 避免把大体积上下文直接塞进窗口
3. 保证 provider 不需要自行猜测目录语义
4. 保证 runtime 对可见上下文有明确控制权

---

## 3. 热数据与冷数据

### 3.1 热数据
热数据指本次调用中**必须直接告诉模型**的信息。

典型包括：
- 原始 `requirement`
- 当前节点 `profile`
- 当前 `invocationKind`
- 当前 feedback 摘要
- 当前输出契约
- 最小 runtime 上下文摘要

这些内容应直接进入 `systemPrompt` 或 `userPrompt` 正文。

### 3.2 冷数据
冷数据指本次调用中**可以暴露给模型，但不应直接占用上下文窗口**的信息。

典型包括：
- 上游节点的 `artifacts`
- `attachments`
- 较长的执行结果
- 报告、日志、补充说明等材料

这些内容不直接展开正文，而是以**文件索引**的方式暴露给模型，供其按需读取。

补充：
- 冷数据被暴露，不等于必须被读取
- 模型只有在需要时才应主动读取冷数据
- 未读取的冷数据，不应被假设其内容

---

## 4. system prompt 与 user prompt 的职责边界

### 4.1 `systemPrompt`
`systemPrompt` 负责表达**不可协商的运行约束**，回答的是：

> 你是谁、你当前在什么位置、你必须遵守什么规则。

它应承载：
- Gold Band runtime 的最小运行语义
- 当前节点角色与职责边界
- 当前节点 `profile`
- 当前输出契约
- 冷数据访问规则

### 4.2 `userPrompt`
`userPrompt` 负责表达**本次任务内容**，回答的是：

> 这次要做什么、当前反馈是什么、你可参考什么材料。

它应承载：
- 原始 `requirement`
- 当前 feedback 摘要
- 本次任务指令
- 冷数据索引

---

## 5. Prompt Placement Rule
- stable runtime rules and output contracts belong to `systemPrompt`
- task-specific goal and feedback belong to `userPrompt`
- hot data may appear in either prompt depending on whether it is a runtime rule or a task input
- cold data must not be expanded by default; it should be exposed as file index only

---

## 6. System Prompt 模板
首版建议 runtime 使用模板方式动态组装 `systemPrompt`。

占位符建议兼容 Rust 常见模板风格：
- `{{field_name}}`
- `{{#if field}} ... {{/if}}`
- `{{#each items}} ... {{/each}}`

建议模板如下：

```md
You are running inside Gold Band runtime.

Gold Band runtime model:
- A run is the full execution of one task.
- A round is one closed-loop iteration inside the run.
- A node is one workflow step inside a round, such as worker / exec / verify.
- An attempt is one concrete execution of a node.

Current location:
- Run: {{run_id}}
- Round: {{round_id}}
- Node: {{node_id}}
- Node type: {{node_type}}
- Attempt: {{attempt_id}}
- Invocation kind: {{invocation_kind}}

Current path context:
- Attempt directory: {{attempt_dir}}
- This directory identifies the current node attempt only.
- Do not assume you should scan the whole run history unless specific files are exposed below.

Profile:
{{profile_prompt}}

Role contract:
{{role_contract_prompt}}

Output contract:
{{#if primary_artifact_name}}
- Return exactly one primary artifact: {{primary_artifact_name}}
- The primary artifact must follow this DSL/schema:
{{primary_artifact_schema_prompt}}
- The primary artifact content must be a string.
{{else}}
- No primary artifact is required unless the user prompt states otherwise.
{{/if}}
- Do not invent undeclared artifacts.

Cold context access rule:
- Supporting artifacts and attachments may be exposed as cold context.
- Cold context is optional: read it only when needed.
- Do not assume the contents of a cold file unless you have read it.
- Only use files explicitly exposed in this invocation.
```

说明：
- Gold Band runtime model 这几行用于解释 `run / round / node / attempt` 的最小语义
- 这里不要求模型自己遍历整个目录层级，只提供必要定位信息
- `primary_artifact_schema_prompt` 不应只写 artifact 名字，必须携带其 DSL / schema 摘要
- 若当前节点未声明 `primaryArtifact`，则输出契约必须退化为“无强制主产物”

---

## 7. User Prompt 模板
首版建议 runtime 使用模板方式动态组装 `userPrompt`。

建议模板如下：

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
The following artifacts are available for optional inspection:

{{#each cold_artifacts}}
- {{name}}: {{path}}
{{/each}}
{{/if}}

{{#if cold_attachments.length}}
# Cold Attachment Index
The following attachments are available for optional inspection:

{{#each cold_attachments}}
- {{path}}
{{/each}}
{{/if}}
```

说明：
- `requirement_text` 属于稳定任务目标，应直接进入 `userPrompt`
- `feedback_summary` 用于表达当前修复背景或上一轮验收/执行失败摘要
- `task_instruction` 对普通 `worker` 可选，对特殊场景建议显式提供
- 冷数据索引只给文件路径清单，不默认展开正文

---

## 8. 运行时占位符建议
首版建议至少支持以下模板字段。

### 8.1 基础标识
- `{{run_id}}`
- `{{round_id}}`
- `{{node_id}}`
- `{{node_type}}`
- `{{attempt_id}}`
- `{{attempt_dir}}`
- `{{invocation_kind}}`

### 8.2 配置与约束
- `{{profile_prompt}}`
- `{{role_contract_prompt}}`
- `{{primary_artifact_name}}`
- `{{primary_artifact_schema_prompt}}`

### 8.3 任务内容
- `{{requirement_text}}`
- `{{feedback_summary}}`
- `{{task_instruction}}`

### 8.4 冷数据集合
- `{{#each cold_artifacts}}`
  - `{{name}}`
  - `{{path}}`
- `{{#each cold_attachments}}`
  - `{{path}}`

---

## 9. 场景化 task instruction 规则
普通 `worker` 不应默认被视为 planning worker。

因此：
- `worker_generic` 不要求固定 `taskInstruction` 模板
- 若 runtime 已有明确任务说明，则可注入 `taskInstruction`
- 若 runtime 没有额外任务说明，则不应为了形式完整而硬造一句“Produce a new ...”

### 9.1 `worker_generic`
表示普通 worker 调用。

它不预设：
- 一定存在 `primaryArtifact`
- 一定在做 planning
- 一定要返回结构化主产物

其本次具体任务由以下信息共同决定：
- 原始 `requirement`
- 当前节点 `profile`
- 可选的 `taskInstruction`
- 可选的 `primaryArtifact` 约束
- runtime 显式暴露的冷数据索引

### 9.2 `worker_repair_exec`
建议注入明确任务指令，例如：

```md
Update the worker output based on the original requirement and the latest execution failure.
{{#if primary_artifact_name}}
Produce a new {{primary_artifact_name}}.
{{/if}}
```

### 9.3 `worker_repair_verify`
建议注入明确任务指令，例如：

```md
Revise the worker output based on the original requirement and the latest acceptance feedback.
{{#if primary_artifact_name}}
Produce a new {{primary_artifact_name}}.
{{/if}}
```

### 9.4 `verify_acceptance`
建议注入明确任务指令，例如：

```md
Evaluate whether the requirement is satisfied based only on the provided evidence.
Produce a {{primary_artifact_name}}.
```

---

## 10. 最小 prompt bundle 结构示意
在 provider implementation 内部，A() 应把外层调用请求进一步整理成最小 `prompt bundle`，再交给 B() 执行。

建议最小结构示意：

```json
{
  "systemPrompt": "<rendered system prompt>",
  "userPrompt": "<rendered user prompt>",
  "metadata": {
    "invocationKind": "worker_generic",
    "profile": "developer",
    "nodeType": "worker",
    "primaryArtifact": null
  }
}
```

说明：
- `metadata` 只作为辅助信息
- prompt bundle 层不应继续依赖路径解引用
- requirement 文本、runtime 上下文摘要、artifact DSL 摘要、冷数据索引组装，应由 A() 在更上层完成
- B() 只负责消费已经准备好的 `prompt bundle`，并映射到 provider-specific 调用

---

## 11. 一句话总结

> `prompt bundle` 是 provider 调用前的最终模型输入层：它通过 system/user 分工，以及热数据内联、冷数据索引暴露的方式，把 runtime 已选上下文稳定地交给模型。
