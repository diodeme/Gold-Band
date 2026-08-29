# ACP Timeline 与实时事件契约

## 背景

Gold Band 的 ACP 会话同时服务两类读取路径：

- 实时路径：运行中的 adapter 事件通过 Tauri event 推送到前端。
- 恢复路径：强刷或重新打开页面时，从 `acp.timeline.jsonl` 读取 timeline item。

两条路径必须渲染出相同会话内容。实时路径允许节流与批量更新，但不允许出现停止会话或强刷后才补齐文本的差异。

## 事件契约

`textDelta`、`thoughtDelta`、`plan` 以及非终态 `toolCall/toolCallUpdate` 在 Gold Band canonical/live 层不是必须逐帧提交的原始协议 delta，而是按稳定 timeline item 或 `toolCallId` 聚合后的最新快照：

- 同一段 assistant 文本使用稳定 `id`，例如 `assistant-message-{messageId}`。
- 同一段 thought 使用稳定 `id`，例如 `assistant-thought-{messageId}`。
- `content` 表示该稳定 item 到当前 `endedSeq` 为止的完整内容。
- `seq` / `startedSeq` 定义 timeline item 的规范顺序，必须随事件推进单调递增；恢复解析、计时重建和测试夹具都不得依赖 HashMap 遍历顺序或相同序号下的偶然排序。
- 原始 token delta 只保留在 `acp.raw.jsonl`，不得由前端 chat 渲染重新解释。
- tool terminal delta 仍按 FIFO 逐帧经过 runtime 并写入 Raw 审计；Timeline 与 live event 只保留同一工具身份在当前发布窗口内的最新投影。工具成功、失败、取消等终态不得合并延迟。

## 后端实时发送规则

后端可以对流式 timeline item 做节流，但节流边界必须按稳定 item 管理：

- pending timeline/live batch 以稳定 item id 为 key；同一 key 的新快照原位替换旧值，不同 text/thought/plan/tool identity 可以同时各保留一份。
- batch 每 75ms、session route 暂时清空或会话收尾时按 revision 顺序 flush；内存规模受当前活跃 identity 数约束，不受 Raw frame 数约束。
- 权限、elicitation、usage、错误、工具终态和 session terminal 等关键事件发送前，必须先按 revision flush 全部 pending batch，再立即处理关键事件。
- durable watermark 只在对应累计快照真实写入 Timeline 后附加；尚未 flush 的 live update 不得伪造持久化水位。
- Prompt 活跃期每次最多连续 drain 256 帧或 25ms，随后必须检查 JSON-RPC response、取消和诊断控制面；确认仍有 backlog 时使用零等待进入下一批，不能睡眠，也不能无限 drain 到队列清空后才观察控制面。
- 成功 response 已携带 session route watermark 时，runtime 必须直接消费至该 watermark，不能先执行一次无限 available drain；watermark 之后的持续流量只由后续 quiet drain 的既有边界处理。RPC error 没有 terminal 收敛要求，只做有界 best-effort drain。

## 管线性能诊断契约

- session route frame 必须携带 stdout reader 首次接收时的单调时钟，跨 early buffer、ingress queue 和 event pump 不得重置；runtime dequeue 后以该时间计算 queue wait。
- prompt 级生产摘要和 Debug/Trace 下的 5 秒窗口统一写入 attempt 的 `acp.diagnostics.jsonl`。摘要只保存固定枚举类别、固定延迟桶、计数、字节、阶段耗时及 ingress/pump 高水位，不保存 frame/prompt/tool payload。
- Raw roll 和 Timeline compaction 只在真实重写发生时计数；compaction duration 只覆盖 canonical timeline 的压缩调用，upsert duration 另行记录，不能把整个 prompt elapsed 误标为压缩耗时。
- live emit duration 在调用既有 live callback 的同步边界测量。该观测不得改变累计快照、latest-wins、durable watermark 或前端发布契约。
- 诊断写入为 best effort：失败只能进入内部 debug tracing，不得中断 Provider prompt；同时生产热路径禁止逐帧追加 diagnostics。
- queue wait 持续增长而 Raw roll/compaction 总耗时占比很低，表示 session consumer 吞吐不足；不得继续用调大 compaction 阈值掩盖逐帧 Timeline/IPC 写放大。

## Timeline 持久化提交与压缩契约

- 75ms 是从当前窗口第一条 pending update 开始计算的固定 deadline。一次慢写完成后必须清空旧 deadline，由下一条 update 开启新窗口；不得保存慢写开始时间并让后续 update 立即连续落盘。
- 同一窗口按稳定 item identity latest-wins，再按 branch 分组。每个 branch 的一批 distinct identity 只获取一次 Timeline 文件锁、打开一次 append 文件、执行一次缓冲写与 flush，并且最多 checkpoint/compact 一次。
- 批量 upsert 返回结果和 durable watermark 必须与输入 identity 对齐；批内重复 identity 是调用契约错误，不能产生顺序不确定的双写。
- 更新已有 item 时复用同一个 Timeline reader，通过 index locator seek 读取 canonical item；不得为同批每个 identity 重复打开文件。
- Timeline index V9 保证 locator 指向完整 canonical item。旧 index 首次打开时先用历史 replay 归一化迁移；V9 压缩直接读取最终 locator，不再扫描全部 patch，压缩复杂度由历史 revision 数降为最终 canonical item 数。
- ratio 压缩必须同时满足 patch 数超过 `uniqueItems × 4` 且至少达到 4,096；8 MiB 文件大小上限独立生效。这样避免小日志每 5 次更新就全量重写，同时仍保证文件增长有界。

## 前端状态规则

前端合并 ACP 事件时必须以稳定 key 为准，key 至少包含 attempt/session、kind 与 stable id。合并同一 key 的 stream 快照时：

- `endedSeq` 或时间更旧的快照不得覆盖较新的内容。
- 空 `content` 只能作为初始化态，不得清空已有非空内容。
- 非流式事件到达前，必须先 flush pending stream buffer，保证事件顺序和可见内容一致。
- session 等价判断必须覆盖整个事件窗口，不能只比较最后一条事件。
- 完整 `AcpSessionVm` 快照到达后，`systemPromptAppend`、`config/models/modes/configOptions` 和 snapshot 中的 timeline events 是当前会话展示的事实来源；实时 `loadedEvents` 只能与 snapshot events 合并，不能覆盖或清空 snapshot events。`createLiveAcpSessionShell` 只允许作为没有 base session 时的临时壳。
- Gold Band synthetic user prompt 允许由 session-ready snapshot 送达而不单独发送 live event。前端可见事件窗口必须合并 snapshot prompt 与后续 live event，避免实时阶段缺用户消息、停止/刷新后才补齐。

## Snapshot / live revision 交接

- timeline patch revision 是持久正文的 commit 水位，event `seq/endedSeq` 只是展示顺序，二者不得互相代替。
- durable live envelope 携带对应 branch 已持久化事件的 `timelineGeneration + timelineRevision`；尚未 flush 的累计流和不落盘的 `timingUpdate` 使用空水位，不能制造可查询的持久化承诺。generation 变化时旧 revision 不得与新 generation 直接比较。
- durable watermark 直接来自同一次 TimelineStore append/index mutation 的 locator；live 发布不得重新 externalize、序列化或哈希大 raw payload 来猜测持久化状态。延迟到达的旧 generation live event 必须丢弃，不能让前端水位回退。
- session page 返回 index `generation / coveredRevision` 和当前页 `newestRevision`。`afterRevision` 依据语义块 `lastRevision` 查询；同 revision 块同页返回。
- 有界 replay 只在 durable event 真正被淘汰时记录 `lossWatermarkRevision`。重进捕获一次固定 loss watermark，先合并 retained live，再用 revision delta 覆盖缺口；新到 live 不延长该目标。
- 新 generation 的 current-page snapshot 可以覆盖旧 generation 的 loss watermark；确认要求 snapshot generation 不早于缺口 generation，且 `coveredRevision` 已覆盖缺口 revision。generation 前进后旧缺口必须清除，避免每次重进重复刷新。
- 停止 accepted 与 terminal 只发布 lifecycle/control patch，不附带正文 session，也不触发终态补拉；尾部正文通过同一 live/revision delta 数据面自然收敛。
- 停止控制面不保留返回完整 `AcpSessionVm` 的旧取消 IPC；所有停止调用都必须经过轻量 accepted/lifecycle 入口。

## 验收标准

- 实时流式会话中，文本气泡和 thought 内容持续补齐。
- 实时流式会话中，首个 Gold Band 用户消息、系统提示词入口和模型/权限配置在 session-ready 快照到达后立即可见。
- 停止会话前后，同一消息内容一致。
- 强刷后从磁盘恢复的会话内容与实时可见内容一致。
- 2,000 个同一工具身份的非终态 terminal delta 突发不能产生 2,000 次 Timeline/IPC 提交；最终工具投影必须完整收敛，工具终态仍立即可见。
- 持续 session-update backlog 下，runtime 必须在每个有界 drain 批次之间观察 prompt response 与取消，不能因数据面繁忙触发伪 terminal-route timeout。
- response watermark 之后即使仍有 backlog，成功响应收敛也不会为了清空整个队列而无限延迟。
- Release 压测每个场景 1,000,000 条 update、每 256 frame 一批，共 3,907 次真实提交：单 identity 与 256 identities 均保持最终 latest-wins 内容和最大 sequence 正确；本机 7,814 次提交中实测最大单批落盘 320.73ms，叠加 75ms 窗口的保守可见上界约 396ms，不得出现秒级提交或分钟级累计积压。
- 同一份 10,000 update / 256 identities Release A/B 必须能证明修复覆盖真实写放大：V8 逐 identity 提交耗时 196.89s、单批最大 7.48s、P99 6.93s；V9 group commit 耗时 1.20s、单批最大 83.75ms、P99 76.58ms。总耗时和最大批延迟分别改善约 164 倍与 89 倍；不能只用修复后的新接口压测推断旧路径存在问题。

## 性能与过度设计评审

- streaming pending map、批量准备区和写缓冲只随当前 75ms 窗口内的 distinct identity/编码字节增长，不随会话历史 frame 数增长；Timeline 文件继续受 ratio 与 8 MiB 双边界约束。
- 普通批量提交时间复杂度为 O(batch identities)，V9 压缩为 O(canonical items)，不再对全部历史 patch 做全量扫描；文件锁只覆盖同一 Timeline 的索引校验、append 或原子压缩事务。
- 方案复用现有 JSONL、materialized index、文件锁、原子写和标准库 `BufWriter`，没有新增线程、数据库、持久队列、缓存层或依赖。现有 canonical identity/revision/generation 已足够表达不变量，因此没有复制状态模型。

## Agent 分支路由持久化契约

- 每个 timeline event 的 `_meta.goldBandConversation` 是分支路由的规范持久化表示，必须成组保存并恢复 `branchId`、`launchedAgentExecutionId` 与 `toolName`。
- 已落盘事件再次参与迁移、索引重建或页面恢复时，不得因为存在 `branchId` 而丢弃同一对象中的 Agent 启动身份；否则后台 Agent 的启动确认会被误判为正式完成结果。
- `branchId` 与 `launchedAgentExecutionId` 恢复前必须通过统一的 conversation branch ID 校验；无效可选字段按缺失处理，不允许覆盖事件本身可重新推导的根分支路由。
- Agent result 迁移、分支索引状态计算与实时路由必须共用同一个 `ConversationBranchRoute` 数据模型，禁止各自解析一部分元数据。

## 前端发布与内存边界

- 后端 timeline item 已经是累计快照；前端只能为每个稳定 text/thought/plan item 或 `toolCallId` 保留一个最新待发布值，新的累计快照原位替换旧值。待发布集合大小必须受活跃 stream/tool identity 数约束，不能受 raw frame 数约束。
- UI publish 采用单飞 timer：任意时刻最多一个 scheduled/in-flight publish。timer drain 后直接进入一次 React state merge，不为每次 flush 创建可延后的 transition 队列；非流式 lifecycle 先同步 drain pending，再按协议顺序应用自身。
- 现场回归以 task-159 的 6021 帧分布为基线：5209 thought chunk、534 message chunk、145 tool update、58 usage update、35 tool call 及其余 protocol/lifecycle 帧。回放必须得到正确最终累计文本、待发布集合有界、scheduled/in-flight publish 上限为 1。

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
