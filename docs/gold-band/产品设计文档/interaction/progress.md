# Gold Band Progress 规范

## 1. 一句话定义
Gold Band Progress 用来定义工作流运行过程中的**观测层文件**。

首版固定三层：
- `raw.stream.jsonl`
- `progress.events.jsonl`
- `run-progress.json`

## 2. 设计原则

### 2.1 progress 属于观测层，不属于控制流层
这些文件用于：
- 长任务时的反馈
- CLI / 插件中的时间线展示
- 失败排查
- 运行态观察

它们不应直接决定：
- workflow 是否成功
- `exec / verify` 的状态流转
- artifact 是否有效

### 2.2 raw、events、snapshot 必须分层
- raw 保存 provider 原始事实
- events 保存 Gold Band 规范化事件流
- `run-progress.json` 保存当前 run 的快速状态快照

### 2.3 解析失败不能拖垮 runtime
即使 stream 结构变化或提炼失败，也不应影响：
- attempt 执行
- 最终 primary artifact 的规范化落盘
- `exec / verify` 流转

补充：
- 若 provider 不支持 raw stream，progress 应退化为 polling / 状态快照 / 最终快照模式
- 这属于观测能力降级，不应影响主执行正确性

## 3. 三层文件边界

### `raw.stream.jsonl`
- provider 原始流式输出留档
- provider-specific
- Gold Band 不要求理解其全部内部字段

### `progress.events.jsonl`
- Gold Band 规范化后的节点进度事件流
- provider-agnostic
- 作为 CLI / 插件展示详细过程的主来源

### `run-progress.json`
- 当前 run 的快速状态快照
- 用于快速读取“工作流现在走到哪里了”
- 不替代 `progress.events.jsonl`
- 不替代 `run.json` / `node.json`

## 4. `run-progress.json` 最小 schema

```json
{
  "version": "0.1",
  "status": "running",
  "currentRoundId": "round-001",
  "currentNodeId": "run-tests",
  "currentNodeType": "exec",
  "currentAttemptId": "attempt-002",
  "currentStage": "running_command",
  "summary": "正在执行 run-tests 节点的第 2 次尝试",
  "updatedAt": "2026-03-29T10:03:12Z"
}
```

最小必填字段：
- `version`
- `status`
- `currentRoundId`
- `currentNodeId`
- `currentNodeType`
- `currentAttemptId`
- `currentStage`
- `summary`
- `updatedAt`

`status` 当前最小枚举：
- `running`
- `paused`
- `completed`

`currentNodeType` 当前最小枚举：
- `worker`
- `exec`
- `verify`

`currentStage` 当前建议最小枚举：
- `starting`
- `calling_provider`
- `streaming`
- `normalizing_artifact`
- `running_command`
- `verifying`
- `paused`
- `blocked`
- `completed`

说明：
- `run-progress.json.status` 应与 `run.json.status` 对齐
- `run-progress.json` 只表达快速查阅视图，不单独承载 canonical outcome
- 失败、invalid、killed 等终态语义应通过 `run.json.outcome` / `node.json.outcome` 表达
- `summary` 面向 CLI / 插件快速展示，不参与控制流判断

## 5. `progress.events.jsonl` 最小包络

```json
{
  "version": "0.1",
  "type": "tool_started",
  "timestamp": "2026-03-22T10:01:00Z",
  "data": {}
}
```

最小必填字段：
- `version`
- `type`
- `timestamp`
- `data`

## 6. 当前刻意留白
当前阶段先不定：
- `progress.events.jsonl` 的完整事件枚举
- tool 事件内部字段全集
- 文本事件、推理事件、卡片事件细分模型
- `stream-json -> progress.events` 的具体映射策略
- `run-progress.json` 的扩展字段（如 repair/acceptance loop 统计）

## 7. 与其他文档的关系
- [Provider Adapter 接口](../provider/adapter.md)
- [Worker Ref 规范](../provider/worker-ref.md)
- [Runtime 概览](../runtime/overview.md)

## 8. 当前实现说明

当前代码已优先落地：
- run 级 `run-progress.json`
- run 级 `events.jsonl`
- attempt 级 `raw.stream.jsonl`
- `.gold-band/logs/runtime.log*` debug 日志

说明：
- `.gold-band/logs/` 仅用于 debug / 排障 / 运行分析，不是 canonical state
- `run-progress.json`、`events.jsonl`、`raw.stream.jsonl` 都属于 observability files
- 当前轮次尚未完整实现 `progress.events.jsonl`，但路径仍保留为后续扩展位
- 所有 observability 写入都应视为 best-effort，失败不能影响主流程

## 9. console / 插件读取优先级

当 console CLI 或插件需要展示运行详情时，建议按以下优先级读取：

### 9.1 run 摘要
- 首选 `run-progress.json`
- 必要时结合 `run.json` 做 canonical 状态确认

### 9.2 run 时间线
- 首选 run 级 `events.jsonl`
- 它是整个 run 的主时间线来源

### 9.3 attempt 级 provider 输出
- 若存在 `progress.events.jsonl`，优先展示它
- 若不存在，则回退展示 `raw.stream.jsonl`
- 若两者都不存在，应展示 empty state，而不是猜测 provider 内部状态

### 9.4 canonical 状态确认
- `run.json` / `round.json` / `node.json` 始终是最终状态来源
- UI 不应根据 `progress.events.jsonl` 或 `raw.stream.jsonl` 直接推断控制流结论

## 10. 一句话总结

> Progress 的三层结构是：`raw.stream.jsonl` 保存 provider 原始流，`progress.events.jsonl` 保存 Gold Band 规范化进度事件，`run-progress.json` 保存整个 workflow 当前运行到哪里；它们共同服务观测与调试，但不直接参与工作流控制流判断。
