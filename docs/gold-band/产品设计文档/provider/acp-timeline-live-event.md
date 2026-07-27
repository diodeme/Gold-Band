# ACP Timeline 与实时事件契约

## 背景

Gold Band 的 ACP 会话同时服务两类读取路径：

- 实时路径：运行中的 adapter 事件通过 Tauri event 推送到前端。
- 恢复路径：强刷或重新打开页面时，从 `acp.timeline.jsonl` 读取 timeline item。

两条路径必须渲染出相同会话内容。实时路径允许节流与批量更新，但不允许出现停止会话或强刷后才补齐文本的差异。

## 事件契约

`textDelta`、`thoughtDelta`、`plan` 在 Gold Band UI 层不是原始 token delta，而是按稳定 timeline item 聚合后的累计快照：

- 同一段 assistant 文本使用稳定 `id`，例如 `assistant-message-{messageId}`。
- 同一段 thought 使用稳定 `id`，例如 `assistant-thought-{messageId}`。
- `content` 表示该稳定 item 到当前 `endedSeq` 为止的完整内容。
- `seq` / `startedSeq` 定义 timeline item 的规范顺序，必须随事件推进单调递增；恢复解析、计时重建和测试夹具都不得依赖 HashMap 遍历顺序或相同序号下的偶然排序。
- 原始 token delta 只保留在 `acp.raw.jsonl`，不得由前端 chat 渲染重新解释。

## 后端实时发送规则

后端可以对流式 timeline item 做节流，但节流边界必须按稳定 item 管理：

- 同一个稳定 item 的连续快照可以合并到 pending live update。
- 当下一个流式快照属于不同稳定 item 时，必须先发送旧 pending 快照，再处理新 item。
- `toolCall`、`usageUpdate`、session terminal 等非流式事件发送前，也必须先发送 pending stream 快照。
- 单槽 pending 只能缓存“同一个稳定 item 的最新快照”，不能覆盖不同 text/thought/plan item。

## 前端状态规则

前端合并 ACP 事件时必须以稳定 key 为准，key 至少包含 attempt/session、kind 与 stable id。合并同一 key 的 stream 快照时：

- `endedSeq` 或时间更旧的快照不得覆盖较新的内容。
- 空 `content` 只能作为初始化态，不得清空已有非空内容。
- 非流式事件到达前，必须先 flush pending stream buffer，保证事件顺序和可见内容一致。
- session 等价判断必须覆盖整个事件窗口，不能只比较最后一条事件。
- 完整 `AcpSessionVm` 快照到达后，`systemPromptAppend`、`config/models/modes/configOptions` 和 snapshot 中的 timeline events 是当前会话展示的事实来源；实时 `loadedEvents` 只能与 snapshot events 合并，不能覆盖或清空 snapshot events。`createLiveAcpSessionShell` 只允许作为没有 base session 时的临时壳。
- Gold Band synthetic user prompt 允许由 session-ready snapshot 送达而不单独发送 live event。前端可见事件窗口必须合并 snapshot prompt 与后续 live event，避免实时阶段缺用户消息、停止/刷新后才补齐。

## 验收标准

- 实时流式会话中，文本气泡和 thought 内容持续补齐。
- 实时流式会话中，首个 Gold Band 用户消息、系统提示词入口和模型/权限配置在 session-ready 快照到达后立即可见。
- 停止会话前后，同一消息内容一致。
- 强刷后从磁盘恢复的会话内容与实时可见内容一致。

## 上下文压缩生命周期事件

Claude-compatible ACP adapter 通过独立的 `agent_message_chunk` 控制文本暴露上下文压缩：

- 独立文本 `Compacting...` 归一化为 `contextCompaction + running`。
- 独立文本 `Compacting completed.` 归一化为 `contextCompaction + completed`。
- 仅精确匹配独立控制文本；普通回复中提到 `Compacting...` 时仍保留为 `textDelta`。

`contextCompaction` 是 Gold Band 的结构化 timeline item，不进入 assistant 最终文本。开始与完成使用同一稳定 item id，通过 timeline patch 原位更新。其 `raw.contextCompaction` 字段定义为：

```json
{
  "phase": "started | completed | interrupted",
  "detectionSource": "providerControlMessage",
  "contextUsedBefore": 169052,
  "contextSize": 200000,
  "contextUsedAfter": 23825,
  "reason": "prompt_finished"
}
```

- 开始时记录最近一次有效 `usage_update` 的 `used/size`。
- 完成信号到达时记录结束时间。若 provider 随后上报 `used=0`，以该事件作为 compaction usage reset 边界，reset 后首个正数 usage 可继续补写 `contextUsedAfter` 作为诊断数据；`used=0` 本身不作为结果。为兼容不发送 reset 的 adapter，低于压缩前占用的首个正数仍可作为降级采样。Claude ACP adapter 当前会在 `usage_update.used` 中混用完整 `getContextUsage` 与普通 turn 的 message-token proxy，因此 `contextUsedAfter` 暂不进入对客 UI，也不作为精确压缩收益验收依据；待上游提供统一、可判别的数据口径后再恢复展示。
- 如果 prompt 在 active compaction 尚未收到完成信号时结束、取消或失败，item 转为 `interrupted`，不能永久保留 running。
- runtime 重新附着时，从 timeline 恢复仍在 running、或 completed 但尚待压缩后 usage 的状态；已有 `contextUsedAfter` 或 interrupted 的条目不再进入热状态。
- ACP 未提供百分比或子阶段，任何前端都不得从耗时推导虚假百分比。

## 上下文用量状态契约

ACP `usage_update.used` 是上下文窗口的状态量，不是累计 Token 计数。Provider 原始采样与 UI canonical 状态必须分层：

- `acp.raw.jsonl` 原样保留每次 `usage_update`，包括 `used=0`，只用于协议审计与诊断。
- runtime 维护最后一次确认的正数上下文占用 `confirmed_used`；普通请求、取消、恢复和 compact 过渡期间出现的 `used=0` 不得覆盖该值。
- compact running 期间冻结压缩前确认值；completed 后以 `used=0` 作为 reset 边界，reset 后首个正数作为新的当前上下文占用。为兼容不发送 reset 的 adapter，低于压缩前占用的首个正数可以作为降级确认。
- canonical `usageUpdate` timeline item 使用 session 级稳定 ID，只写入确认后的 `used/size/cost`；`acp.snapshot.json.usedTokens` 与 `AcpUsageVm.used` 使用相同确认口径。
- 从未获得正数确认值时，`used` 为缺省值，UI 展示 `--`，不得把瞬时 0 表达为真实空上下文。
- `inputTokens / outputTokens / cachedReadTokens / cachedWriteTokens / totalTokens` 是 Provider 返回的消耗计数，与上下文窗口 gauge 独立；不得通过 `used` 的正向差值推导累计消耗。
