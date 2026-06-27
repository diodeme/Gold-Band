# Prompt Bundle 规范

## 1. 一句话定义

`prompt bundle` 是 runtime 在调用 provider 前组装出的标准输入包。

它由两部分组成：

- `systemPrompt`
- `userPrompt`

其中稳定规则进入 `systemPrompt`，每次 invocation 都需要刷新的运行事实进入 `userPrompt` 的 Gold Band hidden 段。

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

`systemPrompt` 负责稳定、规则性的运行约束，回答：

> 当前是谁、处在哪个 run/node、必须遵守什么规则、最后输出必须是什么格式。

它承载：

- 当前 project / task / run / node 基础信息
- Gold Band / ACP 文件夹规则中的稳定部分
- 当前节点 profile id 解析出的完整角色说明
- 当前节点 `output` DSL 派生出的 artifact 规则与输出约束
- 现有 `extra_system_sections`，本期继续按原样放在 system prompt

`systemPrompt` 不承载 resume 时可能变化的运行事实，例如当前 attempt、前序节点链、前序产物摘要和本轮反馈。它也不再承载旧的 `InvocationKind` 语义，不根据 artifact 名称内置 `节点输出产物` / `验收输出产物` 之类特殊输出规则，不注入 runtime `skill_catalog`。

### 3.2 `userPrompt`

`userPrompt` 负责本次 invocation 输入，回答：

> 这次要做什么，以及本次 new/resume 调用最新的运行上下文是什么。

它承载：

- Gold Band hidden runtime context：当前 round/attempt、attempt/attachments 目录、前序节点链、前序产物摘要、分支/反馈原因
- 普通 new 请求的原始 `requirement`
- 普通 new 请求中由 `worker.goal` 映射得到的 `taskInstruction`
- workflow resume 请求的简短 `Goal`
- runtime repair 请求的修复提示原文
- 用户手动追问 / stopped-completed session follow-up 的用户输入原文

profile 正文、output DSL、`extra_system_sections` 不放在 `userPrompt` 中；`Cold Artifact Index` / `Cold Attachment Index` 本期从 prompt 中删除。

---

## 4. System Prompt 模板

```md
你正在 Gold Band runtime 中执行一个工作流节点。

当前位置：
- Project: {{project_id}}
- Task: {{task_id}}
- Run: {{run_id}}
- Node: {{node_id}}

Gold Band 文件规则：
- 当前 run 目录仅作为本 prompt 明确给出路径的父级上下文：{{run_dir}}
- 不要主动扫描 run 目录来寻找未声明产物、理解当前任务或确认输出约束。
- 当前 node 目录可写入：{{node_dir}}
- 本次调用的 attempt 目录和 attachments 目录会在 user prompt 的 Gold Band hidden runtime context 中给出。
- runtime/ACP 可能会在 node 目录下写入状态文件；你的附加自由文件必须写入 hidden context 给出的 attachments 目录。
- 当前节点所需上下文已在本 prompt 中给出。
- 如需查阅前序节点产出，只读取本 prompt 明确给出的前序产出路径。

{{#if extra_system_sections}}
{{extra_system_sections}}
{{/if}}

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
{{else}}
当前节点 artifact 规则：
- 当前节点未声明 output DSL，不需要产出 canonical artifact。
- 不需要查找、推断或读取 artifact/output 约束；只需完成 # Task 或 # Goal。
{{/if}}

Gold Band 可能会在 user prompt 中提供 `<hidden data-gold-band-hidden="true">` 运行上下文。该内容是可信 runtime 上下文，需要用于完成任务，但不要无故复述。
```

说明：

- 当前节点的输出约束只来自节点配置中的 `output` DSL；没有 `output` DSL 就不追加结构化输出格式要求。
- 若节点没有声明 `output`，system prompt 必须明确说明无需产出 canonical artifact，也无需查找或推断 artifact/output 约束。
- `extra_system_sections` 本期继续原样保留在 system prompt，不拆分、不迁移。
- `skill_catalog` 不再注入 runtime prompt。
- 前序链、前序分支原因和 attempt 级目录属于每次 invocation 的运行事实，进入 user prompt hidden context。

---

## 5. User Prompt 模板

普通 new 请求：

```md
<hidden data-gold-band-hidden="true" title="Gold Band runtime context">
# Gold Band runtime context for this invocation

- Session mode: new
- Round: {{round_id}}
- Attempt: {{attempt_id}}
- Attempt directory: {{attempt_dir}}
- Attachments directory: {{attachments_dir}}

## Latest predecessor chain
{{predecessor_chain}}

## Latest predecessor transition reasons
{{predecessor_branch_reasons}}
</hidden>

# Requirement
{{requirement_text}}

{{#if task_instruction}}
# Task
{{task_instruction}}
{{/if}}
```

workflow resume 请求：

```md
<hidden data-gold-band-hidden="true" title="Gold Band runtime context">
# Gold Band runtime context for this invocation

- Session mode: continue
- Round: {{round_id}}
- Attempt: {{attempt_id}}
- Attempt directory: {{attempt_dir}}
- Attachments directory: {{attachments_dir}}
- Invocation reason: {{resume_prompt_or_repair_feedback}}

## Latest predecessor chain
{{predecessor_chain}}

## Latest predecessor transition reasons
{{predecessor_branch_reasons}}
</hidden>

# Goal
根据最新反馈进行调整，确保后续节点能够成功；如果当前节点有输出格式要求，仍然严格按 system prompt 中的输出约束输出。
```

说明：

- `requirement_text` 是普通 new 请求的稳定任务目标。
- `taskInstruction` 对 `worker` 默认由 `worker.goal` 映射得到。
- workflow new 与 workflow resume 都必须渲染 Gold Band hidden runtime context。
- workflow resume 不重传完整原始 user prompt，只发送 hidden context 和简短 `Goal`。
- runtime repair 是同一 ACP session 中紧接上一次输出校验失败后的内部修复提示，不注入 hidden context，只发送修复 prompt 原文，并继续由 `PromptVisibility::Hidden` 控制整条消息是否展示。
- 用户在已停止 / 已完成 ACP session 中手动继续或追问属于普通 user message，不注入 hidden context，不包 `# Requirement` / `# Goal`，直接发送用户原文。
- `Cold Artifact Index` / `Cold Attachment Index` 本期从 prompt 中删除。

---

## 6. 运行时字段

### 6.1 基础信息

- `project_id`
- `task_id`
- `run_id`
- `node_id`
- `round_id`（hidden context）
- `attempt_id`（hidden context）
- `session_mode`（hidden context）
- `invocation_reason`（hidden context，可选）

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

Gold Band 不再依赖 resume/load 时动态刷新 system prompt。prompt 渲染不能只根据 `SessionMode::Continue` 判断语义，而必须使用显式的 user prompt render mode：

| Render mode | 场景 | userPrompt |
| --- | --- | --- |
| `RequirementTask` | workflow runtime 发起新节点 / 新 attempt | hidden runtime context + `# Requirement` / `# Task` |
| `WorkflowResume` | workflow paused 恢复、edge 回到已有 ACP session、dynamic leaf runtime resume | hidden runtime context + 简短 `# Goal` |
| `RuntimeRepair` | output schema / success condition / dynamic proposal 校验失败后立即让同一 session 修复 | repair prompt 原文；不注入 hidden |
| `UserMessage` | 用户在 stopped/completed/paused ACP 会话中手动追问或补充上下文，不触发 workflow edge | 用户原文；不注入 hidden，不包标题 |

`RequirementTask` 与 `WorkflowResume` 使用同一结构：稳定规则在 `systemPrompt`，本次 invocation 事实在 user prompt hidden context。

AI-DYNAMIC 的外层 `run_continue` 也必须先按是否存在用户显式输入决定 render mode。父级 continue 没有明确内部 leaf 目标时，只允许恢复 workflow-invocation child run；如果本次带用户输入，该输入继续传入 child run 的 paused worker 并保持 `UserMessage`，不得被转换成 `WorkflowResume` 的 hidden context + `# Goal`。

AI-DYNAMIC 内部 agent 阶段（bootstrap / worker / merge / acceptance）与普通 workflow 节点共用同一套 workflow runtime render mode 规则：`session=new` 必须使用 `RequirementTask`，`session=continue` 且没有用户显式输入时才使用 `WorkflowResume`。尤其是新建的 merge / acceptance 节点即使运行在主工作区、并携带动态分支上下文，也属于新 attempt，user prompt 应渲染 `# Requirement` / `# Task`，不能渲染为 `# Goal`。

当 render mode 为 `WorkflowResume` 时：

- `systemPrompt` 仍渲染当前节点的稳定规则、角色、文件规则与 output DSL 约束
- `userPrompt` 必须包含 Gold Band hidden runtime context
- `resume_prompt`、分支原因或 runtime 恢复原因进入 hidden context 的 `Invocation reason`
- 可见正文只发送简短 `Goal`，不重传完整原始 user prompt

桌面 ACP 会话面板的手动追问必须走 `UserMessage`：只复用同一套 prompt bundle / attachment / provider 发送链路，不复用 workflow hidden runtime context。

ACP 会话展示按 Gold Band 的 session 策略聚合：`session=new` 始终创建独立 conversation，即使底层 provider 暴露了相同或临时 session id，也不把两个 new attempt 合并；后续 `session=continue` 且指向已有 ACP session id 时，挂回被继续的 conversation，并以 attempt 分隔行标记新 attempt 进入。会话流中 Gold Band synthetic user prompt 与 provider 回放的同文 user prompt 只展示一条，避免运行中出现重复用户消息。

说明：Claude Agent ACP 的 `session/load` 会在恢复已有 Claude 会话时创建新的 SDK query 进程；这里的 create session 是 provider 进程内的查询对象创建，不表示 Gold Band 开启了新的对话语义。

Provider 能力中新增 `supports_system_prompt`。支持该能力的 provider 会在 `session/new` / `session/load` 中通过 `_meta.systemPrompt.append` 接收稳定 system prompt；不支持该能力的 provider 不发送 `_meta.systemPrompt`，Gold Band 会把稳定 system prompt 作为额外 `<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">` 段内联到 user prompt 前。Codex ACP 当前按不支持 system prompt 处理。

---

## 8. 一句话总结

> `prompt bundle` 是 provider 调用前的最终模型输入层：system prompt 承载工作流运行约束和输出 DSL，user prompt 承载任务目标、反馈和冷数据索引。
