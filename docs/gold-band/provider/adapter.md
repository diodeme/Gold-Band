# Provider Adapter 接口

## 1. 定位
provider adapter 是 provider-specific 差异的隔离层。

它内部应至少包含两层：

- **A() 对外统一接口**：Gold Band runtime 直接调用的稳定入口
- **B() 内部执行接口**：每个 provider 实现类真正需要实现的执行入口

它整体负责：
- 调起 provider
- 接收 runtime 传来的外层调用请求
- 在 A() 内部选择热数据与冷数据
- 在 A() 内部把调用请求整理成 prompt bundle
- 把 prompt bundle 交给 B() 执行
- 由 B() 把 prompt bundle 映射成 provider 特定命令或参数
- 接收输出
- 提供 worker reference
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

`runWorker()` 应被理解为 **A() 对外统一接口**。

其正式调用契约见 [Worker Invocation Contract](invocation.md)。

最小输入语义：
- `profile`
- `requirementPath` 或 `requirementText`
- `workspaceDir`
- `attemptDir`
- `primaryArtifact`
- `sessionMode`
- `continueRefPath`
- `streamMode`
- `verifyResultPath` 或 `verifyResultText`

说明：
- `sessionMode` / `continueRefPath` 只影响 provider 如何启动本次 attempt
- CLI 级 `continue` / `retry` 是 runtime 对 attempt 的控制动作，不等同于 provider 输入里的 `sessionMode`

最小输出语义：
- `status`
- `exitCode`
- `resultPayload`
- `workerRefSeed`
- `stream`

说明：
- `resultPayload` 不要求顶层携带 `version`
- 若当前节点声明了 `primaryArtifact`，则 `resultPayload.primaryArtifact` 必须存在
- `primaryArtifact.content` 固定为字符串
- 若当前节点未声明 `primaryArtifact`，则 `resultPayload` 可以为空或缺省；runtime 不因此报错

### `openSession(ref)`
根据 `worker-ref` 打开某个 provider 的原始会话。

### `buildContinueCommand(ref)`
用于构建 provider-specific 的继续/打开命令模板。

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

### Level 3：增强观测能力
- 原始流式输出
- 更稳定的中间进度来源
- 更丰富的 provider capability 暴露

## 4. 与其他文档的关系
- [CLI 规范](../interaction/cli.md)
- [Progress 规范](../interaction/progress.md)
- [Worker Invocation Contract](invocation.md)
- [Prompt Bundle 规范](prompt-bundle.md)
- [Worker Ref 规范](worker-ref.md)
- [Claude Code Provider 实现](implementations/claude-code.md)

## 5. 一句话总结

> provider adapter 的最小职责，是让 Gold Band 能描述 provider、诊断 provider、运行 worker、拿到最终结果、获取 worker reference，并在需要时继续或打开原始会话。
