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
- 原始 token delta 仍逐帧保留为 `acp.raw.jsonl` 中独立的 FIFO JSONL record，但允许多个连续 record 组成一次有界 group commit；不得由前端 chat 渲染重新解释。
- tool terminal delta 仍按 FIFO 逐帧经过 runtime 并写入 Raw 审计；Timeline 与 live event 只保留同一工具身份在当前发布窗口内的最新投影。工具成功、失败、取消等终态不得合并延迟。

## 后端实时发送规则

后端可以对流式 timeline item 做节流，但节流边界必须按稳定 item 管理：

- pending timeline/live batch 以稳定 item id 为 key；同一 key 的新快照原位替换旧值，不同 text/thought/plan/tool identity 可以同时各保留一份。
- batch 每 75ms、session route 暂时清空或会话收尾时按 revision 顺序 flush；内存规模受当前活跃 identity 数约束，不受 Raw frame 数约束。
- 权限、elicitation、usage、错误、工具终态和 session terminal 等关键事件发送前，必须先按 revision flush 全部 pending batch，再立即处理关键事件。
- durable watermark 只在对应累计快照真实写入 Timeline 后附加；尚未 flush 的 live update 不得伪造持久化水位。
- Prompt 活跃期每次最多预取 128 帧、约 4 MiB 或 25ms，再完成当前有界 Raw/canonical batch 并检查 JSON-RPC response、取消和诊断控制面；单个超过字节预算的协议帧保持原子。确认仍有 backlog 时使用零等待进入下一批，不能睡眠，也不能无限 drain 到队列清空后才观察控制面。
- 成功 response 已携带 session route watermark 时，runtime 必须直接消费至该 watermark，不能先执行一次无限 available drain；watermark 之后的持续流量只由后续 quiet drain 的既有边界处理。RPC error 没有 terminal 收敛要求，只做有界 best-effort drain。
- session route 的 `consumed` watermark 表示对应帧已经完成 runtime canonical 处理，不表示仅从 event pump 出队。批量预取不得提前推进水位；每帧处理成功后按 sequence 显式 acknowledgement，乱序 acknowledgement 必须失败关闭，terminal 收敛不得跳过已预取但尚未处理的帧。

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

## Raw 持久化 group commit 契约

- Raw 的 canonical 单位仍是逐帧 JSONL record，batch 只改变物理提交次数，不合并 payload、不丢帧、不改变 FIFO 顺序，也不成为第二条业务队列。
- 活跃 prompt 的同一预取批次只执行一次 JSON 编码集合、一次 Raw 文件锁、一次 append open、一次缓冲写与 flush，并且最多执行一次 roll；单帧 API 继续复用同一批量实现，避免两套写入语义。
- batch 内存同时受 128 帧和约 4 MiB 预取边界约束；roll 后保留目标窗口时必须保持握手帧策略、最后一帧完整和保留帧 sequence 单调。
- Raw append 继续是 best effort 诊断边界，失败不得中断 provider runtime；但 runtime 的 route consumption 只能由 canonical frame processing 成功确认，不能由 Raw batch 成功或出队动作替代。

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
- 每个 timeline live envelope 都必须携带所属 root/Agent branch 当前非空 `timelineGeneration`；`timelineRevision` 仅在该累计快照已由同一次 TimelineStore mutation 持久化时非空。尚未 flush 的累计流和不落盘的 `timingUpdate` 因而使用“generation 非空、revision 为空”，既保持 compaction 归属，又不制造可查询的持久化承诺。durable event 携带同一次写入得到的精确 generation/revision 原子对；generation 变化时旧 revision 不得与新 generation 直接比较。
- event-bearing envelope 的 generation 缺失、非正数或不是安全整数时，Router 必须在 keyed/global listener 分发前移除 event 与其 revision 水位，只保留同 envelope 的 session/lifecycle/activity 控制面，并附带 `timelineRecoveryRequired`。页面不得投影非法正文；ordinary 返回最新、页面容量 handoff 与此类 recovery 必须共用同一个 canonical-head coordinator，每个 owner 最多一个 in-flight 加一个可覆盖的 latest trailing intent，且 recovery 优先。历史窗口保留当前 DOM 和显式恢复入口；失败、新 owner 或请求期间新增 loss 不能被旧成功/finally 清除。
- durable watermark 直接来自同一次 TimelineStore append/index mutation 的 locator；live 发布不得重新 externalize、序列化或哈希大 raw payload 来猜测持久化状态。延迟到达的旧 generation live event 必须丢弃，不能让前端水位回退。
- session page 返回 index `generation / coveredRevision` 和当前页 `newestRevision`。`afterRevision` 依据语义块 `lastRevision` 查询；同 revision 块同页返回。
- 有界 replay 以明确 `sessionId + branch` 为 owner；同 attempt locator 出现新的非空 sessionId 时，必须原子清空旧 retained、loss/head 水位和旧 permission attention。ACK 同时校验 session owner、timeline generation 与 covered revision，并按 Router cut 做 prefix ACK：只删除 `retained.routerGeneration <= observedGeneration`；cut 之后到达的事件和 loss 保留。loss 额外记录其 Router generation，使旧 cut 的确认不能误清 await 期间产生的新缺口。
- 有界 replay 只在 durable event 真正被淘汰时记录 `lossWatermarkRevision`。重进捕获一次固定 C0，不追逐不断前移的 head；同 generation 最多读取 4 个 revision delta page、总计最多 2 秒，更高 generation 至多刷新一次 canonical head。追平 C0 后只同步读取一次 C1；同 session/generation 且没有新 loss 时合并 cumulative snapshot 并 prefix ACK，否则保留恢复入口。新到 live 不延长 C0 目标。
- 有界 replay 淘汰 `timelineRevision=null` 的 transient event 时必须另外记录该 event 的最大 `endedSeq/seq` 为 `lossWatermarkSeq`，并立即令 `requiresCatchUp=true`。sequence loss fence 只证明前端丢过一个尚无 durable revision 的累计快照，不能伪造成 revision delta cursor，也不能由只确认 revision 的 ACK 清除；统一 canonical-head coordinator 只有在匹配 session/generation 的权威 head 已以 `newestSeq` 覆盖该 fence 后才可消费它。
- canonical ACK 的 generation、covered revision 与 covered sequence 只能由当前交接实际收到的 full-head/delta 响应推进。页面为展示而合并的 Router replay、缓存 session 和由可见事件重算的 `newestSeq` 不构成持久覆盖证据；sequence fence 未覆盖时，在同一 4 次/2 秒总预算内有界重读 full head，失败后保留 recovery gate。
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
- 录制工具突发等规模的 Raw Release 压测固定为 13,245 帧、69,752,109 bytes、19 个工具 identity、2 MiB/1 MiB roll 边界。最终 128 帧 group commit 必须在本机 3 秒门槛内完成，并验证 roll 后 FIFO、握手帧与最后一帧完整；逐帧旧接口必须用同一输入建立 A/B，不能只测修复后接口。
- Release 压测每个场景 1,000,000 条 update、每 256 frame 一批，共 3,907 次真实提交：单 identity 与 256 identities 均保持最终 latest-wins 内容和最大 sequence 正确；本机 7,814 次提交中实测最大单批落盘 320.73ms，叠加 75ms 窗口的保守可见上界约 396ms，不得出现秒级提交或分钟级累计积压。
- 同一份 10,000 update / 256 identities Release A/B 必须能证明修复覆盖真实写放大：V8 逐 identity 提交耗时 196.89s、单批最大 7.48s、P99 6.93s；V9 group commit 耗时 1.20s、单批最大 83.75ms、P99 76.58ms。总耗时和最大批延迟分别改善约 164 倍与 89 倍；不能只用修复后的新接口压测推断旧路径存在问题。

## 性能与过度设计评审

- streaming pending map、批量准备区和写缓冲只随当前 75ms 窗口内的 distinct identity/编码字节增长，不随会话历史 frame 数增长；Timeline 文件继续受 ratio 与 8 MiB 双边界约束。
- 普通批量提交时间复杂度为 O(batch identities)，V9 压缩为 O(canonical items)，不再对全部历史 patch 做全量扫描；文件锁只覆盖同一 Timeline 的索引校验、append 或原子压缩事务。
- 方案复用现有 JSONL、materialized index、文件锁、原子写和标准库 `BufWriter`，没有新增线程、数据库、持久队列、缓存层或依赖。现有 canonical identity/revision/generation 已足够表达不变量，因此没有复制状态模型。
- Raw batch 编码暂存为 O(min(128 frames, about 4 MiB) + one atomic oversized frame)，文件操作由逐帧 O(frames) 降为 O(batches)；session sequence 仍是唯一水位，显式 acknowledgement 只修正既有水位转换，不增加持久字段或平行 identity。

## Agent 分支路由持久化契约

- 每个 timeline event 的 `_meta.goldBandConversation` 是分支路由的规范持久化表示，必须成组保存并恢复 `branchId`、`launchedAgentExecutionId` 与 `toolName`。
- 已落盘事件再次参与迁移、索引重建或页面恢复时，不得因为存在 `branchId` 而丢弃同一对象中的 Agent 启动身份；否则后台 Agent 的启动确认会被误判为正式完成结果。
- `branchId` 与 `launchedAgentExecutionId` 恢复前必须通过统一的 conversation branch ID 校验；无效可选字段按缺失处理，不允许覆盖事件本身可重新推导的根分支路由。
- Agent result 迁移、分支索引状态计算与实时路由必须共用同一个 `ConversationBranchRoute` 数据模型，禁止各自解析一部分元数据。

## 前端发布与内存边界

- 后端 timeline item 已经是累计快照；前端只能为每个稳定 text/thought/plan item 或 `toolCallId` 保留一个最新待发布值，新的累计快照原位替换旧值。待发布集合大小必须受活跃 stream/tool identity 数约束，不能受 raw frame 数约束。
- UI publish 采用单飞 timer：任意时刻最多一个 scheduled/in-flight publish。timer drain 后直接进入一次 React state merge，不为每次 flush 创建可延后的 transition 队列；非流式 lifecycle 先同步 drain pending，再按协议顺序应用自身。
- 页面只在当前窗口位于 live head 且未暂停时使用 latest-wins publish buffer；容量不超过当前原生 DOM 窗口，有效上限按配置的 `acpChatEventPageSize × 3` 计算，默认 `192 × 3 = 576` 个 logical timeline item。用户阅读历史、系统弹窗暂停 live 或已有 canonical recovery 时，timeline 正文不进入页面 buffer，只即时收敛 control state 与 optimistic admission，并由 Router/canonical timeline 保留最终事实；恢复时通过统一 canonical-head coordinator 执行一次 handoff，禁止把中间累计 identity 逐项补渲染。
- 页面 buffer 达到容量时必须清空本批并转 canonical recovery，不能静默淘汰一个 identity 后继续提交不完整集合。older/newer cursor 一旦观察到不同 timeline generation 即失效并熔断后续自动触边读取；只有显式“返回最新”可以重新建立 canonical cursor。
- 非 root branch 的 session envelope 只触发所选 branch 的权威读取；同一 owner 最多一个 in-flight 和一个 latest trailing refresh。每个结果必须校验 effect refresh sequence 与 `eventWindowKey`，同 generation revision 只允许前进，避免 burst 形成并发全页请求或迟到响应跨会话回写。
- Activity 展开详情沿用独立 40 项 cursor，但前端只保留 3 页、最多 120 个审计 item；向前加载时从 newer 端裁剪并保存真实 DOM item 锚点，裁剪后提供“回到最新活动”重新读取 latest page。Activity/Tool 详情请求绑定 `eventWindowKey + sessionId + timelineGeneration + logical item + observed position + requestSeq`；没有非空 canonical session owner 时不得查询，Tauri query 和 Timeline 候选扫描必须按同一 session 精确过滤。Tool 还必须绑定 `raw/status/content/title` 的语义 source fingerprint，而不是 `raw` 对象引用。等价 canonical refresh 不失效既有详情或增加扫描，真实同 position source 变化只覆盖一个 latest trailing；detail 仅补 canonical raw 缺失字段，canonical 同位置 output/status/content/title 与显式 null 优先。generation 只作异步 fence，不进入整棵 timeline row key。迟到响应、旧 session、低位置 Tool 详情、旧 error 和旧 finally 均不得覆盖或解锁新 owner。
- Session-keyed optimistic prompt store 是组件唯一的 optimistic UI snapshot；组件只订阅和写回该 store，不保留第二份可分叉数组/ref。每 session 容量跟随有效 live logical item window，默认 576 项，并只缓存最近 12 个 session；它是可丢弃 UI 投影，不是 canonical admission 队列。Activity 窗口、optimistic store、Router replay 与页面 live buffer 均有独立容量，任何一层都不得因用户持续翻页或后台会话运行而随历史无界增长。
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
