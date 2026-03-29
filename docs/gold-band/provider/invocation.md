  # Worker Invocation Contract

## 1. 一句话定义
Worker Invocation Contract 用来定义：

> **Gold Band 每次调用一个 `worker` 节点时，到底给 agent 什么，以及要求它返回什么。**

其中也包括执行层复用同一调用通道的 `verify` 节点。

这是 `worker` 节点（以及执行上按特殊 `worker` 处理的 `verify` 节点）与 provider adapter 之间最核心的契约层。

---

## 2. 设计原则

### 2.1 不把整个 attempt 目录无脑交给 agent
agent 不是默认“自己去扫整个 attempt 目录”。

正确做法是：
- runtime 组装一份标准输入包
- agent 通过这份输入包理解当前任务
- requirement 作为稳定目标输入
- 若存在上一轮验收失败，则直接把最新 `verify-result` 纳入本轮输入

### 2.2 A() 负责组装输入，不暴露内部 sidecar
每次调用 `worker` 时，真正对 provider 暴露的应是 A() 的最小调用请求。

其中：
- requirement / profile / runtime 环境摘要由 A() 统一整理
- 如有内部 sidecar 文件，也只属于 runtime 内部约定
- provider public contract 不应依赖这些 sidecar 文件存在

### 2.3 canonical artifacts 必须结构化返回
凡是会被 runtime 或下游节点程序化消费的内容，必须通过结构化结果返回，再由 runtime 落盘到 `artifacts/`。

### 2.4 free-form 附件可以自由返回，但不能成为强契约
AI agent 可以返回自由格式的附件内容，但这些内容：
- 应落到 `attachments/`
- 不应成为 runtime 控制流判断依据
- 只能作为后续节点的可选参考材料

### 2.5 prompt bundle 必须冷热分离
每次 provider 调用前，runtime 应把上下文分成两类：

- **热数据**：本次调用中必须直接告诉模型的内容，应直接进入 prompt 正文
- **冷数据**：本次调用中可暴露给模型、但不应默认占用上下文窗口的内容，应只以文件索引方式暴露，供模型按需读取

说明：
- 热数据强调“必须知道”
- 冷数据强调“可选知道”
- 冷数据被暴露，不等于保证已被消费

### 2.6 system/user prompt 必须分工明确
`prompt bundle` 不只是字符串拼接，还要明确哪些信息以什么身份给到模型。

原则上：
- `systemPrompt` 负责不可协商的运行约束
- `userPrompt` 负责本次任务内容与当前反馈

---

## 3. 两层接口模型

当前应明确区分两层：

### 3.1 A()：provider 对外统一接口
这是 Gold Band runtime 直接依赖的稳定接口。

它接收的就是 4.1 中定义的外层调用请求。

特点：
- 可以包含路径字段
- 负责表达本次 attempt 的上下文边界
- 负责选择热数据与冷数据
- 负责生成最终 `prompt bundle`
- 是 runtime 唯一应直接依赖的 provider 入口

### 3.2 B()：provider implementation 内部执行接口
这是每个 provider 实现类真正需要实现的内部接口。

它接收的应是 `prompt bundle`，而不是路径型输入。

特点：
- 不应再携带 `inputPath`、`attemptDir`、`workspaceDir` 这类路径字段
- 应只包含已经准备好的 prompt 正文与冷数据索引
- 用于最终映射到 provider 的 system/user prompt 或命令参数
- 是 provider-specific 的真实执行点

---

## 4. A() 外层调用请求

## 4.1 最小调用输入
每次调用 `worker`，provider adapter / runtime 最少要给 A()：

- `invocationKind`
- `profile`
- `requirementPath` 或 `requirementText`
- `workspaceDir`
- `attemptDir`
- `primaryArtifact`（仅当当前节点声明了它时）
- `taskInstruction`（可选）
- `sessionMode`
- `continueRefPath`（仅当 `sessionMode = continue` 时）
- `streamMode`
- `feedbackSummary`（可选）
- `verifyResultPath` 或 `verifyResultText`（仅当存在上一轮验收失败反馈时）
- `coldArtifacts[]`（可选）
- `coldAttachments[]`（可选）

建议最小输入示意：

```json
{
  "invocationKind": "worker_generic",
  "profile": "developer",
  "requirementPath": "authoring/requirement.md",
  "workspaceDir": "/repo",
  "attemptDir": "/repo/.gold-band/tasks/.../attempt-001",
  "primaryArtifact": null,
  "taskInstruction": null,
  "sessionMode": "new",
  "continueRefPath": null,
  "streamMode": "raw",
  "feedbackSummary": null,
  "verifyResultPath": null,
  "coldArtifacts": [],
  "coldAttachments": []
}
```

### 字段语义
- `invocationKind`：本次 provider 调用在 runtime 控制语义上的类别
- `profile`：角色预设
- `requirementPath` / `requirementText`：本次调用的稳定需求输入，至少二选一
- `workspaceDir`：代码工作区根目录
- `attemptDir`：本次 attempt 的私有落盘目录
- `primaryArtifact`：当节点声明它时，表示本次调用要求返回的唯一标准输出逻辑名；若节点未声明，则该字段可省略或为 `null`
- `taskInstruction`：runtime 为本次调用附加的任务说明；首版建议可选
- `sessionMode`：`new | continue`
- `continueRefPath`：当需要复用历史会话时，指向上一次 `worker-ref.json` 的路径
- `streamMode`：是否请求 provider 返回原始流式输出
- `feedbackSummary`：runtime 对当前失败反馈或修复背景的摘要，属于热数据候选
- `verifyResultPath` / `verifyResultText`：上一轮验收失败反馈；若存在，应作为本轮修复输入的一部分
- `coldArtifacts[]`：runtime 显式暴露给本次调用的冷 artifact 文件索引
- `coldAttachments[]`：runtime 显式暴露给本次调用的冷 attachment 文件索引

说明：
- 这仍然是 **A() 外层统一接口**
- 因此允许包含路径字段
- `provider` 不必重复出现在这里，provider 选择应先于 A() 完成
- 这些路径字段不应直接进入 B() 的 `prompt bundle` 层
- 这里的 `sessionMode = new | continue` 是 provider 启动模式，不等同于 CLI 层的 `continue` / `retry`

### 4.1.1 `invocationKind`
首版建议最少定义以下几类：

- `worker_generic`
- `worker_repair_exec`
- `worker_repair_verify`
- `verify_acceptance`

说明：
- `worker_generic` 表示普通 `worker` 调用
- 它不预设本次调用一定用于 planning
- 它也不预设一定存在 `primaryArtifact`
- 特殊 repair / verify 场景可在此基础上收敛额外任务指令

### 4.1.2 `verify` 节点的输入特例
`verify` 在 DSL 上是独立节点类型，但在执行层建议复用与 `worker` 相同的 provider invocation 主通道。

因此首版建议：
- `verify` 也走 `runWorker()`
- `invocationKind = verify_acceptance`
- `primaryArtifact` 固定为 `verify-result`
- 其输入组装规则比通用 `worker` 更收敛

对 `verify` 而言：
- `requirementPath` / `requirementText` 仍表示原始 requirement
- `verifyResultPath` / `verifyResultText` 通常不作为 `verify` 自身输入
- runtime 应额外把“当前 round 的关键验收证据”整理进冷数据索引，必要摘要进入热数据

也就是说：
- 普通 `worker` 的 user prompt 重点是“完成需求 / 修复问题 / 执行当前任务”
- `verify` 的 user prompt 重点是“基于 requirement 和显式证据做验收判断”

---

## 4.2 runtime sidecar 文件的定位
runtime 内部如果需要保留额外 sidecar 文件，应把它们视为项目内部约定，而不是 provider 对外接口的一部分。

说明：
- provider public contract 不应重复承载运行时摘要字段
- provider implementation 不应依赖某个 sidecar 文件是否存在
- sidecar 文件仅用于 runtime 自身的审计、调试与回放

---

## 4.3 关于 `attachments/` 的明确规则
`attachments/` 是**上下文池**，不是默认输入全集。

也就是说：
- 不能把整个 `attachments/` 目录无脑丢给 agent 自己扫
- runtime 应在调用前显式决定哪些附件被纳入冷数据索引
- agent 只应默认依赖这些被显式暴露的附件

---

## 5. Prompt Bundle 的定位

`prompt bundle` 属于 A() 到 B() 之间的内部模型输入层，不属于 runtime 对 provider 的最外层调用请求。

它负责：
- 把外层调用请求进一步整理成最终给模型的 `systemPrompt` / `userPrompt`
- 明确热数据直接内联、冷数据只暴露文件索引
- 明确 `systemPrompt` 与 `userPrompt` 的职责边界
- 把 artifact DSL / schema 摘要并入最终输出契约

完整规范见：
- [Prompt Bundle 规范](prompt-bundle.md)

---

## 6. 要求 agent 返回什么

### 6.1 最小返回结构
当当前节点声明了 `primaryArtifact` 时，agent 的返回结果至少应能被 provider adapter 归一成：

```json
{
  "primaryArtifact": {
    "name": "exec-plan",
    "content": "{ ... }"
  }
}
```

说明：
- `primaryArtifact`：仅在节点声明它时，才构成本次调用要求返回的唯一标准输出
- 若节点未声明 `primaryArtifact`，则 provider 返回包可不含该字段，甚至可没有结构化结果
- `attachments/` 若存在，应视为执行过程中的文件副作用，由 runtime 在执行后发现与整理，而不是结构化返回值的一部分

---

### 6.2 `primaryArtifact`
当节点声明它时，`primaryArtifact` 表达本次调用要求返回的唯一标准输出。

最小建议结构：

```json
{
  "name": "exec-plan",
  "content": "{ ... }"
}
```

说明：
- `name` 使用逻辑名，而不是文件名
- `content` 固定为字符串；provider 不负责返回已 parse 的对象
- runtime 不信任 `content` 的内部结构，而是按当前 `primaryArtifact` 对应 schema 自己解析、校验并规范化落盘
- runtime 再根据 `name` 把它规范化落盘为 canonical artifact 文件

### 重要规则
- 若当前节点声明了 `primaryArtifact`，则一个 `worker` 节点一次只应返回一个 `primaryArtifact`
- `primaryArtifact.name` 必须匹配当前节点声明的 `primaryArtifact`
- `primaryArtifact.content` 固定必须是字符串
- 当节点声明了 `primaryArtifact` 时，缺失、类型不对、无法解析或 schema 不合法，会导致 runtime 判为 `invalid`

---

### 6.3 `attachments`
`attachments` 不是 canonical 返回契约，而是 worker 执行过程中的自由文件副作用。

说明：
- worker 可以在执行过程中自行写入 `attachments/` 目录
- runtime 可以在执行后发现并整理这些文件
- `attachments` 不进入 canonical artifact contract
- `attachments` 不能作为控制流判断依据

---

## 7. runtime 如何接收并处理返回结果

### 7.1 provider adapter 返回给 runtime 的最小输出
在 provider 层，`runWorker()` 最少应返回：

- `status`
- `exitCode`
- `resultPayload`
- `workerRefSeed`
- `stream`

其中：
- `resultPayload` 就是上面定义的结构化返回包原材料
- `workerRefSeed` 用于生成 `worker-ref.json`

### 7.2 runtime 的后处理职责
runtime 在收到 `resultPayload` 后应：

1. 若当前节点声明了 `primaryArtifact`，校验它是否存在
2. 若当前节点声明了 `primaryArtifact`，校验其 `name` 是否匹配当前节点声明值
3. 若存在合法 `primaryArtifact`，将其规范化落盘到 `artifacts/`
4. 发现或整理 worker 写出的 `attachments/` 文件
5. 更新 `manifest.json`
6. 不把 `attachments` 当作控制流判断依据

补充规则：
- 若当前节点未声明 `primaryArtifact`，runtime 不要求 canonical artifact
- 此时 provider 即使没有返回结构化结果，也不构成错误
- 此时该次调用的 `success / failure / paused` 只依据 provider invocation 的完成状态归纳
- 只有当 provider adapter 返回包本身连最小外层契约都不满足时，runtime 才将该次调用归为 `invalid`

---

## 8. 哪些必须固化格式，哪些允许自由产出

### 必须固化格式
所有会被 runtime 或下游节点程序化消费的内容。

当前包括：
- `exec-plan`
- `exec-result`
- `verify-result`

但对于单次 `worker` 调用，当前建议一次只产生其中一个 `primaryArtifact`。

### 可以自由产出
所有只供人类阅读或后续 agent 按需参考的附件内容。

当前建议统一落在：
- `attachments/`

---

## 9. 一句话总结

> **每次调用 `worker` 时，Gold Band 应显式给 A() 一份最小外层调用请求；A() 再把它整理成带有冷热分层、明确 system/user 分工的 `prompt bundle`。若节点声明了 `primaryArtifact`，则 runtime 要求它返回对应的唯一标准输出，而 `attachments/` 则属于执行过程中的自由文件副作用。**
