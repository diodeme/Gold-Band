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

## 验收标准

- 实时流式会话中，文本气泡和 thought 内容持续补齐。
- 停止会话前后，同一消息内容一致。
- 强刷后从磁盘恢复的会话内容与实时可见内容一致。
