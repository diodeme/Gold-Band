# Gold Band Progress 规范

## 1. 一句话定义
Gold Band Progress 用来定义 AI worker 节点执行过程中的**观测层文件**。

首版固定三层：
- `raw.stream.jsonl`
- `progress.events.jsonl`
- `progress.json`

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

### 2.2 raw 与 normalized 必须分层
- raw 保存 provider 原始事实
- events 保存 Gold Band 规范化事件流
- progress.json 保存当前状态快照

### 2.3 解析失败不能拖垮 runtime
即使 stream 结构变化或提炼失败，也不应影响：
- attempt 执行
- 最终 primary artifact 的规范化落盘
- `exec / verify` 流转

## 3. 三层文件边界

### `raw.stream.jsonl`
- provider 原始流式输出留档
- provider-specific
- Gold Band 不要求理解其全部内部字段

### `progress.events.jsonl`
- Gold Band 规范化后的节点进度事件流
- provider-agnostic
- 作为 CLI / 插件展示详细过程的主来源

### `progress.json`
- 当前节点状态快照
- 用于快速读取“现在是什么状态”
- 不替代 `progress.events.jsonl`

## 4. `progress.json` 最小 schema

```json
{
  "version": "0.1",
  "status": "running",
  "provider": "claude-code",
  "startedAt": "2026-03-22T10:00:00Z",
  "lastEventAt": "2026-03-22T10:03:12Z",
  "eventCount": 12,
  "currentActivity": {
    "kind": "tool",
    "label": "Glob"
  }
}
```

最小必填字段：
- `version`
- `status`
- `provider`
- `startedAt`
- `lastEventAt`
- `eventCount`
- `currentActivity`

`status` 当前最小枚举：
- `running`
- `paused`
- `completed`

说明：
- `progress.json.status` 应与 attempt 级 `node.json.status` 对齐
- Progress 只表达运行态快照，不单独承载 `outcome`
- 失败、invalid、killed 等终态语义应通过 `node.json.outcome` / `run.json.outcome` 表达

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

## 7. 与其他文档的关系
- [Provider Adapter 接口](../provider/adapter.md)
- [Worker Ref 规范](../provider/worker-ref.md)
- [Runtime 概览](../runtime/overview.md)

## 8. 一句话总结

> Progress 的三层结构是：`raw.stream.jsonl` 保存 provider 原始流，`progress.events.jsonl` 保存 Gold Band 规范化进度事件，`progress.json` 保存当前状态快照；它们共同服务观测与调试，但不直接参与工作流控制流判断。
