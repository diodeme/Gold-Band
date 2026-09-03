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

- [x] Raw 与 session route 继续以逐帧 record 保持 FIFO、不合并 payload、不重排；Raw 物理写入允许有界 group commit，保留完整协议审计。
- [x] runtime 对 text/thought/plan 和带稳定 `toolCallId` 的非终态工具投影使用按 identity 的 latest-wins batch。
- [x] Timeline 与 live batch 每 75ms、队列暂时清空、关键事件或 session 收尾时按 revision 顺序 flush。
- [x] 权限、错误、usage、工具终态与 session terminal 先 flush pending，再立即提交自身。
- [x] Prompt 活跃 drain 限定为每批最多 128 帧、约 4 MiB 或 25ms 预取；完成当前有界 Raw/canonical batch 后检查 response/cancel，有 backlog 时零等待继续。
- [x] 成功 response 直接按 route watermark 收敛，不再先执行无限 available drain；RPC error 仅有界读取。
- [x] 新增非终态工具合并/终态立即提交和 drain 公平性单元测试。

## Raw group commit 第一轮优化

task-320 最后一个新版本 turn 的独立窗口包含 13,245 帧、69,752,109 bytes，其中 12,283 帧为工具更新；现场诊断显示 Raw append 累计 23.697 秒，说明 Timeline group commit 已消除主要写放大后，Raw 每帧同步 lock/open/flush/roll check 成为新的确定性吞吐瓶颈。根因属于既有“逐帧审计”设计正确，但物理提交实现不完整：审计 record 与文件 commit 被错误绑定为一一对应。

- [x] `append_raw_frames` 将连续 FIFO frame 编码为独立 JSONL record，但一批只执行一次文件锁、open、缓冲 flush 和 roll check；单帧入口复用批量入口。
- [x] 活跃 drain 使用 128 帧、约 4 MiB、25ms 三重边界，未新增后台线程、持久队列或并行写入。
- [x] event pump 出队不再推进 `last_consumed_sequence`；每帧完成 runtime canonical 处理后显式 ack，乱序 ack 返回结构化内部错误，避免批量预取越过 terminal watermark。
- [x] 新增 roll/FIFO 正确性测试、prefetch/ack watermark 回归和 ignored Release 大数据压测。

同一 13,245 帧 / 69,752,109 bytes / 19 个工具 identity / 2 MiB max / 1 MiB target 输入的 Windows Release A/B 为：

| 版本 | 物理文件操作 | 总耗时 | 观察批次 P99 | 最大观察批次 |
| --- | ---: | ---: | ---: | ---: |
| 逐帧基线 | 13,245 | 13.804s | 101.48ms（每 64 帧统计） | 109.90ms |
| 128 帧 group commit | 104 | 2.336s | 32.48ms | 36.10ms |

总耗时改善约 5.9 倍并通过 3 秒门槛。256 帧版本曾达到 1.765 秒，但第一轮最终选择 128 帧，因为它已经满足吞吐目标且将 response/cancel 检查前的最大 frame 工作量减半；这是吞吐与控制面公平性的有数据取舍，不为追求 benchmark 峰值扩大批次。

## Timeline group commit 与长尾优化

task-320 generation 19 的可审计基线为 2,340 帧、1,067 次 Timeline upsert：Timeline upsert 累计 214,025ms、单次最大 1,659ms、最大 queue wait 158,671ms；同期 Raw 累计 3,343ms、compaction 累计 1,444ms。根因不是磁盘吞吐不足，而是同步提交次数与单次提交工作放大：旧 75ms 判断保存慢写开始时间，慢写超过窗口后下一帧会立即再写；pending identity 又逐项加锁、打开、flush。

- [x] 75ms 改为从首个 pending update 开始的固定 deadline，flush 后清空，慢写耗时不再透支下一窗口。
- [x] `TimelineStore::upsert_batch` 对 distinct identity 执行一次锁、一次 append open、一次缓冲 flush、一次 index checkpoint 判断及最多一次 compaction。
- [x] client 按 branch group commit，branch store 使用 `Entry` 惰性创建，消除已存在 branch 仍提前 `open()` 的开销。
- [x] 同批已有 item 复用单个 reader；JSONL append 与 canonical rewrite 使用标准库缓冲写，消除逐 identity open 和逐字段/逐行底层写放大。
- [x] Timeline index 升级 V9，明确 locator 指向完整 canonical item；旧 index 首次归一化迁移，之后 compaction 只读最终 locator，不 replay 全 patch。
- [x] ratio compaction 增加 4,096 patch 最小门槛，且继续保留 8 MiB 独立上限；解决单 identity 每 5 patch、低基数日志频繁全量重写的设计缺陷。
- [x] 新增 batch identity/watermark 回归、固定 deadline 回归、小 patch volume 不压缩回归，以及可配置至百万条的 ignored Release 压测。
- [x] Timeline 吞吐修复后将正常 prompt 的 terminal route 收敛默认余量由 5 秒调整为 10 秒；取消流程继续复用既有统一 10 秒 deadline 的剩余时间，不叠加等待，也不以延长超时替代吞吐根因修复。

为验证压力测试不是只测新接口，曾临时完整还原四个生产修复文件，仅保留相同事件生成与统计逻辑，并将提交切回 V8 逐 identity `upsert`；随后恢复完全相同的修复，用同样数据切回 `upsert_batch`。10,000 update / 256 identities 的 Release A/B 结果为：

| 版本 | 提交方式 | 总落盘耗时 | P99 单批 | 最大单批 |
| --- | --- | ---: | ---: | ---: |
| V8 基线 | 每批最多 256 次同步 `upsert` | 196.89s | 6.93s | 7.48s |
| V9 修复 | 每批一次 `upsert_batch` | 1.20s | 76.58ms | 83.75ms |

同数据下总耗时改善约 164 倍、最大批延迟改善约 89 倍。旧版仅 10,000 条就稳定产生分钟级处理时间和秒级单批阻塞，足以解释 task-318 中 queue wait 持续增长并超过 terminal route timeout；因此后续大规模结果可以作为修复有效性的证据，而不是仅证明新接口自身很快。

Release 最终压测在 Windows 本机执行两组各 1,000,000 条 update，每 256 frame 模拟一次 group commit，每组 3,907 次提交；单场景规模约为 task-320 的 427 倍：

| 场景 | 最终 items | compactions | 总落盘耗时 | P95 | P99 | 最大单批 | 最大压缩批次 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 同一 identity 持续修订 | 1 | 0 | 101.51s | 46.59ms | 85.37ms | 320.73ms | 0ms |
| 256 identities 交错修订 | 256 | 244 | 134.82s | 80.22ms | 103.01ms | 237.22ms | 237.22ms |

两组共 2,000,000 条输入、7,814 次真实提交全部通过 latest-wins 数量和 sequence 验证；本次实测最大落盘延迟为 320.73ms，加固定 75ms 窗口后的保守可见上界约 396ms。该数字是本机长压观测最大值，不是对所有硬件的理论硬上限；关键验收是延迟不随百万条历史增长为秒级或分钟级积压。

## 验收

- production 模式在 5 秒边界不产生 window，但 prompt 终态始终产生 summary。
- detailed 模式每 5 秒产生并重置一个聚合窗口，不使用真实 sleep。
- 10,000 帧输入后聚合器对象大小不变，bucket 总数正确。
- direct route 和 early-session route 都保留原始 receipt time。
- Raw 日志未重写时没有 roll stats，真实重写后提供 before/after bytes 与 duration。
- 13,245 帧 / 69,752,109 bytes Raw Release 压测在 3 秒内完成；roll 后逐帧 record 保持 FIFO、握手帧策略与尾帧完整。
- 编译检查和上述定向单元测试通过；若仓库其他既有测试夹具阻断 lib-test 编译，需将阻断项与本次测试结果分开记录。
- 2,000 个同一 `toolCallId` 的 terminal delta 突发后，pending Timeline/live 数量按 identity 有界，最终快照完整收敛，工具终态不延迟。
- session-update 持续堆积时，prompt response/cancel 在 25ms 预取加一个最多 128 帧/约 4 MiB 的有界 Raw/canonical batch 后重新观察，不再等待队列清空。
- 成功 response 的 route watermark 已消费后，watermark 之后的 backlog 不会被前置无限 drain 纳入终态关键路径。
- 已预取但尚未 canonical 处理的 frame 不得计入 consumed watermark；只处理首帧时水位不得越过同批第二帧。

## 性能与过度设计评审

- 每帧新增成本为一个 `Instant`、固定数组计数和少量饱和整数运算，时间 O(1)、空间 O(1)。没有全量扫描、N+1 I/O、无界 map/queue、额外线程、缓存或锁层级。
- receipt time 复用已有 frame ownership；queue high-water 复用已有 route/pump mutex，仅在 prompt 起止读取并重置。5 秒 diagnostics I/O 只在用户开启详细日志时发生。
- 未引入 histogram/t-digest、遥测数据库、独立采样状态机或新 UI 开关；当前抽象与定位单条 ACP 管线的实际规模和风险匹配。
- 吞吐修复新增的 pending map 上限等于当前窗口内活跃 stream/tool identity 数，替代按 Raw frame 增长的同步写入/IPC 次数；每批排序规模为活跃 identity 数，不扫描历史 Timeline。没有新增线程、持久字段、队列或依赖。
- Timeline group commit 的普通路径为 O(batch identities)，V9 compaction 为 O(canonical items)，不再为每批 identity N 次打开文件，也不再按历史 patch 数全量 replay。写缓冲和 batch 暂存受当前窗口 distinct identity/编码量约束，持久文件受 4,096 patch ratio 门槛与 8 MiB 上限约束。
- 未新增异步持久化线程、后台 compaction 队列、数据库或第二套 canonical state；继续复用现有 JSONL/index/atomic write。相较引入并发状态机，本方案直接消除 I/O 与算法放大，复杂度与现有数据规模、崩溃一致性风险匹配，不属于过度设计。
- Raw 编码暂存受 128 帧和约 4 MiB 边界约束，时间复杂度仍为 O(frames + bytes)，文件锁/open/flush/roll 次数降为 O(batches)。显式 route ack 复用现有 sequence/watermark，不新增持久字段或 aggregate；它修复批量预取暴露的生命周期语义缺口，不是第二套状态机。
