# ACP 流式管线延迟诊断埋点方案

## 背景与结论边界

现场会话出现 Agent 客户端已完整输出、Gold Band 仍逐字渲染约 19 分钟的现象。已有数据证明 timeline 存在显著写放大，但最终回答阶段没有发生 compaction，且约 4 Hz 的稳定到达形状也可能来自 adapter/app-server 投递或 session route 消费。因此本阶段只补齐跨边界证据，不把 compaction 预判为完整根因，也不修改流式节流和业务状态机。

## 数据边界

单个 session frame 的诊断生命周期为：

1. stdout reader 首次路由时记录 `received_at: Instant`。
2. early-session buffer、64 MiB / 16,384 帧 ingress 和 4 MiB / 256 帧 event pump 保留同一 receipt time 与 route sequence。
3. runtime dequeue 计算 queue wait，并分别聚合 Raw append/roll、normalize/process、Timeline upsert/compaction 和 live emit 时长。
4. prompt 终态读取并重置 ingress/pump high-water，形成一条终态 summary。

这些值都是进程内性能观测，不成为 canonical session/timeline 状态，不参与恢复、合并或 UI 渲染。

## 输出与开关

- 输出位置：当前 attempt 的 `acp.diagnostics.jsonl`。
- 生产常开：每 prompt 一条 `acp.pipeline-summary`；实际 Raw roll、Timeline compaction 及严重 queue wait 记录低频结构化事件。
- 详细模式：复用设置页“记录详细日志”对应的 `Debug/Trace` log level，每 5 秒增加一条 `acp.pipeline-window`；不新增设置、数据库字段或 UI。
- 限频：queue wait 达到 1 秒后才进入异常记录，同一 prompt 间隔至少 30 秒。
- 隐私：不记录正文、prompt、文件内容、工具输出和 provider payload。
- 容量：固定 8 个 latency bucket 和固定 update kind 数组；内存不随帧数增长，热路径不逐帧写 diagnostics。默认每 prompt 预计 2～10 KiB；详细 30 分钟预计约 200～400 KiB。

## 实现状态

- [x] receipt time 跨 early buffer、route 与 pump 传递。
- [x] ingress/pump prompt 级 high-water 采集与重置。
- [x] 固定容量 prompt summary、5 秒详细窗口及 queue wait 异常限频。
- [x] Raw append/真实 roll、Timeline upsert/真实 compaction、live emit 聚合。
- [x] 详细模式复用既有 verbose logging 开关。
- [x] 结构化数据写入既有 attempt diagnostics，保持 best effort。

## task-320 根因闭环与吞吐修复

task-320 的新埋点在第二轮 prompt 记录到 `queueWaitMs = 846708`、`promptElapsedMs = 1014945`：Gold Band 正在处理约 14 分钟前已经进入 session route 的 frame。Raw 保留窗口中 2,277 个 inbound frame 有 2,045 个属于 terminal output delta；同期 13 次 Timeline compaction 合计约 12.3 秒，Raw roll 合计 4ms，不能解释队列延迟。代码回溯确认 text/thought/plan 使用 75ms 合并，但非终态 tool update 被当作普通事件逐帧 Timeline upsert 和 live emit，持续 drain 又只在队列完全清空后观察 RPC response。

- [x] Raw 与 session route 继续逐帧 FIFO，不合并、不重排，保留完整协议审计。
- [x] runtime 对 text/thought/plan 和带稳定 `toolCallId` 的非终态工具投影使用按 identity 的 latest-wins batch。
- [x] Timeline 与 live batch 每 75ms、队列暂时清空、关键事件或 session 收尾时按 revision 顺序 flush。
- [x] 权限、错误、usage、工具终态与 session terminal 先 flush pending，再立即提交自身。
- [x] Prompt 活跃 drain 限定为每批 256 帧或 25ms；批次之间检查 response/cancel，有 backlog 时零等待继续。
- [x] 成功 response 直接按 route watermark 收敛，不再先执行无限 available drain；RPC error 仅有界读取。
- [x] 新增非终态工具合并/终态立即提交和 drain 公平性单元测试。

## 验收

- production 模式在 5 秒边界不产生 window，但 prompt 终态始终产生 summary。
- detailed 模式每 5 秒产生并重置一个聚合窗口，不使用真实 sleep。
- 10,000 帧输入后聚合器对象大小不变，bucket 总数正确。
- direct route 和 early-session route 都保留原始 receipt time。
- Raw 日志未重写时没有 roll stats，真实重写后提供 before/after bytes 与 duration。
- 编译检查和上述定向单元测试通过；若仓库其他既有测试夹具阻断 lib-test 编译，需将阻断项与本次测试结果分开记录。
- 2,000 个同一 `toolCallId` 的 terminal delta 突发后，pending Timeline/live 数量按 identity 有界，最终快照完整收敛，工具终态不延迟。
- session-update 持续堆积时，prompt response/cancel 的观察间隔不超过单个 25ms drain 预算加当前正在处理的一帧，不再等待队列清空。
- 成功 response 的 route watermark 已消费后，watermark 之后的 backlog 不会被前置无限 drain 纳入终态关键路径。

## 性能与过度设计评审

- 每帧新增成本为一个 `Instant`、固定数组计数和少量饱和整数运算，时间 O(1)、空间 O(1)。没有全量扫描、N+1 I/O、无界 map/queue、额外线程、缓存或锁层级。
- receipt time 复用已有 frame ownership；queue high-water 复用已有 route/pump mutex，仅在 prompt 起止读取并重置。5 秒 diagnostics I/O 只在用户开启详细日志时发生。
- 未引入 histogram/t-digest、遥测数据库、独立采样状态机或新 UI 开关；当前抽象与定位单条 ACP 管线的实际规模和风险匹配。
- 吞吐修复新增的 pending map 上限等于当前窗口内活跃 stream/tool identity 数，替代按 Raw frame 增长的同步写入/IPC 次数；每批排序规模为活跃 identity 数，不扫描历史 Timeline。没有新增线程、持久字段、队列或依赖。
