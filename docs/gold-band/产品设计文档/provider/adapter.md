# Provider Adapter 接口

## 1. 定位
provider adapter 是 provider-specific 差异的隔离层。

它内部应至少包含两层接口，但这两层首先是 **ownership boundary**，其次才是代码分层：

- **A()：runtime 拥有并直接依赖的稳定接口**
- **B()：provider implementation 拥有并实现的内部执行接口**

说明：
- A() 与 B() 可以物理上同处 provider 模块中
- 但 A() 的契约归 runtime 所有，B() 的契约归 provider implementation 所有
- Gold Band runtime 只应直接依赖 A()，不应直接依赖某个 provider 的 B()

它整体负责：
- 调起 provider，优先启动 ACP-compatible adapter
- 接收 runtime 传来的外层调用请求
- 在 A() 内部选择热数据与冷数据
- 在 A() 内部把调用请求整理成 prompt bundle
- 把 prompt bundle 映射为 ACP 调用：新建 session 时通过 `_meta.systemPrompt.append` 注入 system prompt，继续会话的 `session/prompt` 只发送用户 prompt 内容
- 接收 ACP `session/update`、permission request 与 prompt response
- 保存 ACP 会话观测材料、adapter 返回的 session config 快照（`models` / `modes` / `configOptions`）与 raw frame
- 提供 worker reference 与外部 CLI handoff
- 暴露能力信息

## 2. 最小接口

### `describeProvider()`
返回 provider 基本信息与能力摘要。

最少表达：
- `providerId`
- `displayName`
- `capabilities`
- `isDefault`

### `doctor()`
检查 provider 当前是否可用。

最少回答：
- provider 是否已安装
- 可执行入口是否存在
- 当前环境是否满足最小运行条件
- 失败时给出明确原因

### `runWorker()`
运行一次 AI worker attempt。

`runWorker()` 应被理解为 **A()：runtime-facing 稳定接口**。

其正式调用契约见 [Worker Invocation Contract](invocation.md)。

最小输入语义：
- `profile` / `profileContent`
- `requirementPath` 或 `requirementText`
- `workspaceDir`
- `attemptDir`
- `primaryArtifact`
- `outputContract`（来自当前节点 `output` DSL）
- `runtimeContext`
- `predecessors[]`
- `taskInstruction`
- `sessionMode`（可选，缺省为 `new`）
- `continueRefPath`
- `streamMode`

说明：
- `sessionMode` / `continueRefPath` 只影响 provider 如何启动本次 attempt
- 未显式提供 `sessionMode` 时，默认使用 `new`
- CLI 级 `continue` / `retry` 是 runtime 对 attempt 的控制动作，不等同于 provider 输入里的 `sessionMode`

最小输出语义：
- `status`
- `exitCode`
- `resultPayload`
- `workerRefSeed`
- `sessionEvents`（ACP normalized UI events，落盘到 `acp.events.jsonl`）
- `rawSession`（ACP raw frame，落盘到 `acp.raw.jsonl`）

说明：
- `resultPayload` 不要求顶层携带 `version`
- 若当前节点声明了 `primaryArtifact`，则 `resultPayload.primaryArtifact` 必须存在
- `primaryArtifact.content` 固定为字符串，表示模型按 output structure 返回的原始内容
- provider 不负责把 `primaryArtifact.content` parse 成语义对象
- `sessionEvents` 保持 ACP session event 语义，用于会话详情可视化，不再转换为 Gold Band 自研 `progress.events.jsonl`
- `rawSession` 只用于 raw viewer / 排障，不作为 UI 主协议
- 若当前节点未声明 `primaryArtifact`，则 `resultPayload` 可以为空或缺省；runtime 不因此报错

### `openSession(ref)`
根据 `worker-ref` 打开某个 provider 的原始会话。

说明：
- 这是 provider handoff 能力，不是 Gold Band runtime 内部的 `continue` 控制动作
- 调用它意味着 Gold Band 把交互控制权交还给 provider

### `buildContinueCommand(ref)`
用于构建 provider-specific 的继续/打开命令模板。

说明：
- 该能力既可用于 `open-session` 的 provider handoff，也可供 runtime 在内部恢复 provider 会话时使用
- 但具体使用它并不改变 `run continue` 仍属于 Gold Band runtime 控制动作这一事实

### B()：内部执行接口（实现类提供）
这是每个 provider implementation 真正需要实现的内部执行点。

其输入应是 prompt bundle，而不是路径型输入；其职责是：
- 接收 A() 组装好的 prompt bundle
- 消费已经分好层的 `systemPrompt` / `userPrompt`
- 在需要时配合模型按需访问 runtime 已暴露的冷数据文件索引
- 映射到 provider-specific 的 system/user prompt 或命令参数
- 发起真实调用
- 返回原始结果给 A() 做统一收尾

## 3. 最小能力分级

### Level 1：基础执行能力
- `describeProvider`
- `doctor`
- `runWorker`
- 最终结果返回
- 基础 `worker-ref`

### Level 2：会话可继续能力
- `openSession`
- `buildContinueCommand`
- 可继续或可打开的原始会话引用

运行时规则：
- 若 workflow edge 显式请求 `session = continue`，但 provider 不支持 continue，则应在 DSL / runtime 校验阶段直接报错
- 若 provider 不支持 `openSession`，CLI `open-session` 应明确报错，而不是静默降级为其他动作

### Level 3：ACP 会话可视化能力
- ACP session events
- ACP raw frame / raw transcript
- tool call / plan / thought / permission / terminal 等原始 agent 过程展示
- 更丰富的 provider capability 暴露
- 外部 CLI handoff

## 4. 与其他文档的关系
- [CLI 规范](../interaction/cli.md)
- [Progress 规范](../interaction/progress.md)
- [Worker Invocation Contract](invocation.md)
- [Prompt Bundle 规范](prompt-bundle.md)
- [Worker Ref 规范](worker-ref.md)
- [Claude Code Provider 实现](implementations/claude-code.md)

## 5. 一句话总结

> provider adapter 的最小职责，是让 Gold Band 能描述 provider、诊断 provider、运行 worker、拿到最终结果、获取 worker reference，并在需要时继续或打开原始会话；其中 A() 归 runtime 所有，B() 归 provider implementation 所有。