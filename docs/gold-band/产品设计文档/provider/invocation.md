# Worker Invocation Contract

## 1. 一句话定义

Worker Invocation Contract 定义 Gold Band runtime 每次调用一个 AI 节点时，给 provider 什么，以及要求 provider 返回什么。

当前契约不再依赖专门的 `InvocationKind` prompt 语义。节点语义来自工作流配置、运行位置、前序链、profile 和 output DSL。

---

## 2. 设计原则

### 2.1 runtime 负责组装输入

agent 不是默认“自己去扫整个 attempt 目录”。

runtime 应在调用前组装标准输入包：

- requirement 作为稳定目标输入
- profile id 解析成完整角色内容
- workflow/runtime 上下文进入 system prompt
- 冷 artifacts / attachments 只以索引方式暴露

### 2.2 system/user prompt 分工明确

- `systemPrompt`：当前节点位置、前序运行链、分支原因、文件规则、角色说明、output DSL 约束
- `userPrompt`：requirement、当前反馈、节点 taskInstruction、冷数据索引

### 2.3 canonical artifacts 必须结构化返回

凡是会被 runtime 或下游节点程序化消费的内容，必须通过 `primaryArtifact` 归一后落盘到 `artifacts/`。

### 2.4 attachments 是自由副作用

AI agent 可以写自由格式附件，但必须写入当前 attempt 的 `attachments/` 目录。attachments 不作为 runtime 控制流判断依据。

---

## 3. A() 外层调用请求

每次调用 AI 节点，runtime 给 provider 层的最小输入包括：

- `profile`
- `profileContent`
- `requirementPath` 或 `requirementText`
- `workspaceDir`
- `attemptDir`
- `primaryArtifact`（仅当当前节点声明）
- `outputContract`（仅当当前节点声明 `output` DSL）
- `runtimeContext`
- `predecessors[]`
- `taskInstruction`（对 worker 由 `worker.goal` 映射得到）
- `sessionMode`
- `continueRef`
- `streamMode`
- `feedbackSummary`
- `coldArtifacts[]`
- `coldAttachments[]`

示意：

```json
{
  "profile": "developer",
  "profileContent": "...",
  "requirementPath": "authoring/requirement.md",
  "workspaceDir": "/repo",
  "attemptDir": "~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001",
  "primaryArtifact": "dev-result",
  "outputContract": {
    "artifact": "dev-result",
    "kind": "json",
    "schema": { "result": "boolean", "reason": "string" },
    "successCondition": "JSON field `$.result` equals `true`"
  },
  "runtimeContext": {
    "projectId": "{project-id}",
    "taskId": "task-001",
    "runId": "run-001",
    "roundId": "round-001",
    "nodeId": "dev",
    "attemptId": "attempt-001"
  },
  "predecessors": [],
  "taskInstruction": "Implement the requested change",
  "sessionMode": "new",
  "continueRef": null,
  "streamMode": "stream-json",
  "feedbackSummary": null,
  "coldArtifacts": [],
  "coldAttachments": []
}
```

---

## 4. 字段语义

- `profile`：workflow 节点保存的 profile id。
- `profileContent`：runtime 根据 profile id 解析出的完整角色说明，进入 `systemPrompt`。
- `requirementPath` / `requirementText`：稳定需求输入。
- `workspaceDir`：代码工作区根目录。
- `attemptDir`：当前节点 attempt 私有目录。
- `primaryArtifact`：当前节点声明的 canonical artifact 逻辑名；仅用于 provider 输出提取和 runtime 落盘。
- `outputContract`：当前节点 `output` DSL 派生出的输出格式与成功条件；进入 `systemPrompt`。
- `runtimeContext`：project/task/run/round/node/attempt 以及 run/round/node/attempt/attachments 路径。
- `predecessors[]`：从 round trace 和历史 round 重建出的前序运行链。
- `taskInstruction`：本次调用的任务说明；对 worker 节点由 `worker.goal` 映射得到，并进入 `userPrompt`。
- `sessionMode`：`new | continue`。
- `continueRef`：复用历史 ACP session 时的引用。
- `feedbackSummary`：当前失败反馈或修复背景摘要。
- `coldArtifacts[]` / `coldAttachments[]`：runtime 显式暴露给本次调用的冷文件索引。

`InvocationKind` 若仍存在于内部代码，只能作为兼容/日志字段，不能决定 prompt 内容。

---

## 5. Prompt Bundle 的定位

`prompt bundle` 属于外层 invocation 到 provider 执行接口之间的最终模型输入层。

它负责：

- 把 runtime context、predecessor chain、profile content、output DSL 渲染进 `systemPrompt`
- 把 requirement、feedback、taskInstruction、冷数据索引渲染进 `userPrompt`
- 保持热数据内联、冷数据索引暴露

完整规范见 [Prompt Bundle 规范](prompt-bundle.md)。

---

## 6. 返回结果

当当前节点声明了 `primaryArtifact` 时，provider 返回结果至少应能被归一为：

```json
{
  "primaryArtifact": {
    "name": "dev-result",
    "content": "{ ... }"
  }
}
```

规则：

- `name` 使用 artifact 逻辑名，不是文件名。
- `content` 是模型最终输出的 raw string。
- runtime 根据当前节点配置解析、校验并落盘到 `artifacts/`。
- 若当前节点没有声明 `primaryArtifact`，provider 可不返回结构化 artifact。
- attachments 属于执行过程中的自由文件副作用，不进入 canonical artifact contract。

---

## 7. 一句话总结

> 每次调用 AI 节点时，Gold Band 显式传入 runtime context、前序链、profile 和 output DSL；prompt bundle 再将这些内容稳定映射到 system/user prompt，provider 不再通过旧 invocation kind 或 artifact 名称猜测节点语义。
