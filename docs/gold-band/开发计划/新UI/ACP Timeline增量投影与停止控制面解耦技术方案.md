# ACP Timeline 增量投影与停止控制面解耦技术方案

> 状态：已实施并验收  
> 日期：2026-08-19  
> 范围：ACP timeline 写入、会话分页与重进、停止控制、lifecycle 发布、旧会话迁移  
> 关联设计：[会话优化](./会话优化.md)、[会话式主页实施进度](./会话式主页实施进度.md)、[会话 runtime 产品设计](../../产品设计文档/interaction/app/conversational-runtime.md)

## 0. 结论

本方案收口本轮停止性能与终态收敛问题，包含两条必须同时完成的主线：

1. **数据面：TimelineStore + 可重建 index/checkpoint。** TimelineStore 成为 `acp.timeline.jsonl` 的唯一读写边界，在写入时增量维护物化投影；会话首次进入、重进和分页只读取 snapshot、当前页与 checkpoint 后的有界尾部，复杂度从随历史增长的 `O(N)` 降为 `O(P + Δ)`。
2. **控制面：轻量 lifecycle patch + ACP facet revision。** 停止 accepted、provider terminal 和恢复状态不再通过完整 `AcpSessionVm` 传递。ACP lifecycle 使用独立单调 revision，前端按 facet revision 合并，旧 `stopping` 不得覆盖新的 `cancelled`。

这不是给全量扫描增加缓存、后台线程或更长超时。根本修复是：

- 停止控制面不读取会话正文；
- 会话恢复不全量回放；
- timeline 写时增量维护 index/projection；
- snapshot、分页结果和 live patch 通过 generation/revision 水位交接；
- runtime、ACP、prompt queue 分属不同生命周期，各自维护 revision，不新增全局状态机。

上述改动已按本文边界完成。本文同时保留问题溯源与目标契约，并在末尾记录实现落点、回归结果和真实 Release 基线。

---

## 1. 背景与问题复盘

### 1.1 真实性能基线

本轮真实会话约有 2536 个事件，停止/终态查询曾耗时约 32 秒。现有 `scan_acp_timeline()` 会在一次会话查询中完成：

- 加载并合并全部 timeline patch；
- 从全部事件构造 semantic blocks；
- 扫描全部历史寻找最新 Todo；
- 从全部历史重建 pending permission / elicitation；
- 重算 usage、timing、available commands 等投影；
- 在活跃会话或缓存失效时重新从第一条记录开始解析。

因此即使 provider 已经立即返回 `cancelled`，停止 cleanup 或终态 session 发布仍可能被正文重建拖慢。把扫描放入 blocking pool 只能避免占用 async executor，不能消除 `O(N)` CPU、I/O、锁占用和内存分配。

### 1.2 永久 stopping 的回归链

本轮历史溯源确认：

- `6d0c58b`（2026-08-05）把同步停止改为快速 accepted + 后台 cleanup。accepted lifecycle 与 terminal event 从此可能乱序，但旧前端仍可依据 terminal session status 自愈。
- `935855a`（2026-08-13）解耦 runtime 与 ACP lifecycle 后，在 lifecycle 存在时不再使用 terminal session status。旧 `stopping` lifecycle 因而可以覆盖或遮蔽已到达的 `cancelled` session。
- `93191d0`（2026-08-18）修复了初始化取消后正常收到 terminal lifecycle 的路径，但没有覆盖“terminal 先到、accepted 后到”以及 terminal event 丢失后的单调恢复。
- `1bfb03b`（2026-08-19）把 provider cancel deadline 收紧为统一 10 秒，并将取消 drain 改为有界处理。它不是永久 stopping 的根因，但更快的终态使既有乱序窗口更容易稳定复现。

当前 `mergeConversationAttemptLifecycle()` 除 prompt queue 外基本采用“后到覆盖先到”，ACP facet 没有可比较 revision。于是下面两种情况都无法保证收敛：

```text
terminal(cancelled) 先到 -> accepted(stopping) 后到
terminal event 未到       -> session 已 cancelled，但 lifecycle 仍 stopping
```

### 1.3 task-026 暴露的 AUTO leaf 生命周期缺口

task-026 的 `goodbye-worker` 并不是被 timeline 全量查询本身卡住。真实持久化状态表明 ACP turn 已经进入取消收尾，但当时的 dynamic graph `0.2` 只有 node 的 `status/outcome` 与短生命周期 `runtimeExecutionId`，没有每个并行 leaf 自己的 runtime phase/revision。停止一个 leaf、兄弟 leaf 同时结束、父图聚合暂停和迟到 worker 消息并发发生时，恢复层无法区分“这个 leaf 已被用户停止”与“父图仍在执行，leaf 可能继续推进”，旧执行结果因而可能重新投影为 active/stopping。

修复不是在 AUTO 页面增加状态特判，而是把 dynamic graph 升级到 `0.3`：每个 leaf 持久化 `runtimeExecutionPhase / runtimeExecutionRevision / runtimeExecutionUpdatedAt`，以 execution id 作为 CAS 边界推进 `starting-node -> running-node -> finalizing/repairing -> paused/terminal`。用户停止清除 leaf execution id 并写入更高 revision 的 `paused`；迟到的旧 execution id 不再有权修改该 leaf。父 dynamic run 和 outer run 只聚合 leaf canonical state，不复制 leaf 生命周期。

历史 `0.2` graph 在统一存储读取边界一次性迁移：durable `paused` leaf 映射为 `runtimeExecutionPhase=paused` 并清除 execution id，已完成 leaf 映射为 `terminal`，仍由运行中 graph/current node 拥有的 active leaf 保留 identity 并映射到活动 phase。迁移校验后原子写回 `0.3`，重复读取不再改盘。task-026 已经通过该路径迁移，两个旧 sibling 均稳定为 `paused`，父 run 收敛为 `paused / process-interrupted`。

### 1.4 当前架构缺口

问题不是某一个慢函数，而是四个尚未完成的设计边界：

1. `TimelineStore::open()` 和查询路径仍会全量加载 canonical items，写时物化投影尚未落地。
2. 停止 cleanup 为发布状态重新构建完整 `AcpSessionVm`，控制面仍依赖正文查询。
3. prompt retry、permission、elicitation 等稳定 identity 已存在，但停止结算仍可能扫描整条 timeline 寻找目标。
4. usage repair、diagnostics scan、raw fallback、stale lifecycle fuse 等恢复工作混入普通 session 查询，导致“读取”隐式执行修复。

---

## 2. 目标与非目标

### 2.1 目标

- 新会话从创建开始维护 timeline index/checkpoint。
- 旧会话最多允许一次完整扫描迁移，成功后不再扫描全历史。
- 正常重进与分页处理量不超过 `页面大小 P + checkpoint 尾部 Δ`。
- stop accepted 不打开 timeline、不构建 session 正文，耗时与历史长度无关。
- provider cancel 的 10 秒只等待 provider/route 收敛，不包含 timeline 投影重建。
- retry prompt、permission、elicitation 按稳定 ID 精确、幂等结算。
- terminal lifecycle 发布不携带历史正文。
- ACP lifecycle 在 accepted、live、terminal、reentry 之间按 revision 单调合并。
- index 损坏或丢失时可从 timeline 重建，不形成第二套业务事实。

### 2.2 非目标

- 不把 SQLite 改成会话主存储；SQLite 继续只承担跨会话搜索。
- 不增加第二套 timeline、第二套 runtime 状态机或全局统一 revision。
- 不用无界内存缓存保存全部会话投影。
- 不通过延长 timeout、轮询 session 或前端定时刷新掩盖状态缺口。
- 不在正常查询路径继续保留 legacy full scan fallback；legacy 只存在于显式迁移/重建边界。

---

## 3. 数据归属与权威事实源

### 3.1 领域划分

| 领域 | 权威事实 | 可重建投影 | revision 作用域 |
|---|---|---|---|
| 会话正文 | `acp.timeline.jsonl` | `acp.timeline.index.json`、内存 projection | timeline item revision + generation |
| ACP session lifecycle | `acp.session.json` 中的 lifecycle header | `acp.snapshot.json` 对应恢复摘要、前端 ACP facet | `acp.revision` |
| Workflow/AUTO runtime | `run.json.execution` | conversation lifecycle 的 runtime facet | `runtime.revision` |
| Direct prompt queue | prompt queue 持久文件 | conversation lifecycle 的 promptQueue facet | `promptQueue.revision` |
| 协议排障 | `acp.raw.jsonl`、diagnostics | 显式详情接口结果 | 不参与正文/lifecycle 合并 |

约束：

- timeline 是消息、工具、Todo 和交互事件的持久事实。
- index/checkpoint 只保存可从 timeline 重建的 locator、聚合摘要和读取水位；删除 index 不得改变业务结果。
- `acp.session.json` 只保存轻量 session/lifecycle header，不复制正文。
- `acp.snapshot.json` 是恢复快照，不得与 session lifecycle header 产生两个互相竞争的 revision。
- runtime、ACP、prompt queue 的 revision 不互相替代；前端按 facet 合并。

### 3.2 稳定身份

所有索引和 patch 必须使用完整 attempt/branch locator：

```text
projectId / taskId / runId / roundId /
outerNodeId? / outerAttemptId? / nodeId / attemptId / branchId
```

正文项使用 canonical `itemId`；当前 prompt 使用 `turnId + promptEventId`；permission 和 elicitation 使用各自 request identity。禁止按名称、可见文案、数组位置或“最新一条相似事件”反查。

---

## 4. TimelineStore 与物化投影

### 4.1 唯一边界

现有 `src/acp/timeline.rs` 中的 `TimelineStore` 扩展为 timeline 的唯一读写入口。下列能力必须全部收口到 TimelineStore，其他模块不得直接调用 `load_timeline_items_unlocked()` 实现业务查询或修改：

- append/upsert timeline patch；
- 按 item ID 读取最新 revision；
- 条件结算 item；
- root/Agent branch 语义块分页；
- Todo、pending interaction、commands、usage、timing 等轻量投影；
- checkpoint 尾部追平；
- index 重建与 compaction。

`events.rs` 只保留事件模型、序列化与领域级构造函数，不再拥有“加载全部 timeline 后寻找目标”的修改入口。

### 4.2 Index/checkpoint 结构

新增 `acp.timeline.index.json`，当前 V4 结构如下：

```json
{
  "formatVersion": 3,
  "timelineGeneration": 3,
  "timelineFile": {
    "coveredOffset": 482991,
    "coveredRevision": 10842,
    "observedLength": 482991,
    "identity": "generation-and-signature"
  },
  "nextTimelineRevisionHint": 10843,
  "itemLocators": {
    "prompt-001": {
      "offset": 481020,
      "revision": 10840,
      "branchId": "root",
      "kind": "userPrompt",
      "status": "processing"
    }
  },
  "pendingRetryPromptId": "prompt-001",
  "branches": {
    "root": {
      "semanticBlocks": [],
      "latestPage": {},
      "latestTodo": null,
      "pendingInteractions": {},
      "availableCommands": [],
      "usage": null,
      "timing": null
    }
  }
}
```

说明：

- `timelineGeneration` 在 compaction 后递增；旧 generation 的 offset/cursor 全部失效。
- `coveredOffset/coveredRevision` 表示 checkpoint 已消费到的 canonical 边界。
- `itemLocators` 指向每个 item 的最新 patch offset，支持 O(1) 定位后读取单条 JSONL 记录。
- `semanticBlocks` 保存有序轻量 locator，不复制正文；当前页只通过 locator 定点读取。
- `nextTimelineRevisionHint` 只是经过 generation/文件签名校验后的分配优化，不是独立事实。index 丢失时从 timeline patch revision 重建。
- branch 投影按稳定 branch ID 隔离；root 与 Agent branch 不共享“最新 Todo”或 pending interaction。

### 4.3 内存 projection

每个已打开 TimelineStore 维护当前 generation 的有界内存投影：

- `itemId -> latest locator/revision/fingerprint`；
- 语义块顺序与分页 locator；
- 最新 Todo；
- pending permission / elicitation；
- available commands；
- usage/timing 摘要；
- 当前最新页定位；
- checkpoint 后尚未持久化的 patch 数由打开的 TimelineStore 在内存维护。

内存 projection 的生命周期属于已打开的 attempt。全局 registry 必须有数量/空闲回收上限，不允许按所有历史会话永久驻留。

### 4.4 写入顺序

单次 timeline mutation 的顺序为：

```text
持 timeline 文件锁
  -> 分配/校验 timeline item revision
  -> append timeline patch 并 flush
  -> 增量更新同一 TimelineStore 内存 projection
  -> 按 checkpoint policy 原子写 index/checkpoint
  -> 解锁
```

timeline append 是业务提交点。index 原子替换失败不得回滚已提交 timeline；下一次 open 通过旧 checkpoint 的 `coveredOffset` 回放尾部。

完整 index 不应在每个 text/tool patch 后都重写，否则会把查询侧 `O(N)` 转移到写路径。checkpoint policy 统一管理：

- 最大未 checkpoint patch 数；
- terminal/正常关闭强制 checkpoint；
- compaction 强制生成同 generation 的新 index。

这些阈值由 `TimelineCheckpointPolicy` 或应用配置统一提供，不在调用点硬编码。正常尾部 `Δ` 受 policy 约束；进程在 append 后、checkpoint 前崩溃时，只回放旧 checkpoint 后的文件尾部。

### 4.5 崩溃一致性

| 崩溃位置 | 恢复行为 |
|---|---|
| patch append 前 | 没有业务变更 |
| patch 已 flush、内存尚未更新 | 从 checkpoint offset 回放尾部 |
| 内存已更新、index 尚未替换 | 从旧 checkpoint 回放尾部 |
| index 临时文件已写、尚未替换 | 忽略临时文件，使用旧 index + tail replay |
| compaction timeline 已替换、index generation 不匹配 | 进入一次 blocking rebuild，禁止使用旧 offset |

index 必须校验 format version、generation、covered offset、文件长度/签名。校验失败返回结构化恢复结果，不静默使用可疑 locator。

---

## 5. 有界会话查询与重进

### 5.1 正常查询流程

`get_acp_session` 的正常路径调整为：

```text
读取轻量 lifecycle header + snapshot
  -> TimelineStore 校验 index/checkpoint
  -> 回放 checkpoint 后 Δ 条 patch
  -> 从 branch semantic locator 定位最近 P 个语义块
  -> 只读取这些 locator 指向的正文记录
  -> 返回 snapshot + current page + generation/revision 水位
```

返回的 `AcpSessionVm` 可以保留现有外形以降低迁移成本，但 `events/timelineProjection` 只能包含当前页，`eventCount/hasOlder/hasNewer` 来自 index，不得为填充 VM 全量加载历史。

### 5.2 live 订阅交接

会话重进使用 subscribe-before-read，防止查询期间丢失 live patch：

```text
建立匹配 attempt/branch 的 live 订阅并暂存 patch
  -> 读取 snapshot + current page + base generation/revision
  -> 丢弃不属于当前 generation 或 revision <= base 的重复 patch
  -> 按 revision 回放暂存 patch
  -> 检测 revision 缺口；只有存在缺口时请求 bounded tail catch-up
```

前端合并键必须包含完整 session/branch identity，不能仅使用 event ID。generation 改变时丢弃旧 cursor，并从新 generation 的当前页重新建立水位。

### 5.3 分页

- `beforeCursor/afterCursor` 继续作为公开分页接口，cursor 对前端不透明。
- cursor 编码 branch、generation、semantic ordinal/seq range 和必要 offset。
- 单页单位是语义块，不是 raw frame 或 patch 数。
- 工具详情、raw frame、diagnostics 使用各自显式接口，不随会话页一起加载。

### 5.4 禁止查询时修复

普通 session query 必须成为只读、有界操作。以下工作从正常查询移除：

- usage/timing 全量重算；
- diagnostics 全文件扫描；
- raw fallback 重建正文；
- stale lifecycle fuse 写回；
- legacy 文件修补；
- index schema migration。

这些工作分别移动到：

- runtime 启动/恢复边界；
- legacy 会话首次迁移；
- checkpoint tail catch-up；
- 显式 diagnostics/repair 接口。

每项恢复成功后写入 format version 与完成水位，重复查询不得再次执行。

---

## 6. 停止控制面

### 6.1 Accepted 热路径

`stop_active_session` 只提交控制事实：

```text
设置进程内停止/发送门禁
  -> 持久化 pause、auto-dispatch suspension 和 cancel latch
  -> 分配 ACP stopping revision
  -> 返回轻量 accepted lifecycle patch
  -> 后台派发 provider cancel（不读取 timeline）
  -> 无活动 provider 时立即发布该 turn 的 terminal lifecycle
  -> 最后按 identity 结算 retry / permission / elicitation
```

accepted 热路径禁止：

- 打开或扫描 timeline；
- 构建 `AcpSessionVm`；
- 扫描 diagnostics/raw；
- 等待 index rebuild/checkpoint；
- 等待 provider terminal。

如果是没有 index 的旧会话，accepted 仍立即返回。所需迁移/结算进入 blocking pool，不能把 legacy full scan 带回停止命令。

`dispatch_attempt_prompt_cancel()` 与 `settle_attempt_prompt_interactions()` 是分离的两个阶段。前者只操作 provider control/connection；后者才可能通过 checkpoint/index 回放尾部或执行旧会话迁移。不得用一个“取消”辅助函数把两者重新包回“先结算、后发送”的顺序。若 dispatch 判定没有活动 provider，必须在 settlement 前按当前 `turnId` 提交并发布 terminal lifecycle，避免旧会话 migration 让 UI 长时间保留 `stopping`。

### 6.2 按稳定 identity 精确结算

snapshot/session header 保存当前 retry prompt、permission 和 elicitation 的 canonical identity。timeline index 同步物化当前 `processing + retry` item ID，覆盖 timeline 已 flush、metadata 尚未原子替换时的崩溃/停止窗口；该字段完全由 timeline 事件重建，不成为第二业务事实源。后台停止结算调用 TimelineStore 的条件更新接口：

```text
读取 promptEventId/requestId；retry 缺失或落后时使用 index pending identity
  -> 从内存 index 或 checkpoint O(1) 取得 latest offset/revision
  -> 在 timeline 锁内读取目标最新 patch
  -> CAS 校验 expected revision/status
  -> processing/pending -> cancelled/declined
  -> append 新 revision patch
  -> 增量更新 projection
```

建议接口形态：

```rust
TimelineStore::settle_item(
    identity,
    expected_revision,
    expected_state,
    terminal_patch,
) -> Result<TimelineSettleOutcome>
```

`TimelineSettleOutcome` 至少区分：

- `Applied`；
- `AlreadyTerminal`；
- `RevisionConflict`；
- `IdentityMissing`；
- `IndexRecoveryRequired`。

重复停止遇到 `AlreadyTerminal` 直接幂等成功，不产生新 patch。`cancel_latest_processing_prompt_retry()` 中的全量 `load_timeline_items_unlocked()` 路径最终删除。

### 6.3 终态所有权

- active provider prompt 由原 prompt finalizer 在 `mark_stopped + terminal session header/snapshot` 提交后发布 terminal lifecycle patch。
- retry backoff 或尚无 active provider、但存在 durable pending identity 时，由统一 stop settlement service 在 CAS 结算成功后提交同一 terminal transition。
- cleanup 只负责投递 cancel 和触发结算，不再在“cancel 已发送”后猜测终态，也不再为了通知前端构建完整 session。
- 所有路径复用一个 terminal transition API，保证 operation id 幂等、revision 单调和 terminal dominance。

### 6.4 Provider 10 秒边界

现有统一 10 秒 cancel deadline 保留：

- 起点是 runtime 首次观察到 `CancelRequested`；
- cancel notification、response、route watermark 与 quiet drain 共用同一剩余 deadline；
- 有界 inbound drain 每批后返回 deadline/RPC 检查；
- deadline 只约束 provider/route 收敛，不包含 timeline index rebuild、完整 session 查询或 diagnostics 扫描；
- deadline 到期仍提交 `cancelled + drainTimedOut` 终态，并隔离不可直接复用的 session。

---

## 7. Lifecycle patch 与单调合并

### 7.1 ACP facet revision

`ConversationAcpFacetVm` 增加独立 revision 与当前 turn identity：

```text
revision
turnId / promptEventId
sessionAvailability
liveTurnActivity
latestTurnStatus
stopping
stopReason
```

revision 由 attempt 级 ACP lifecycle header 分配并持久化。accepted stopping 和 terminal cancelled 必须获得不同 revision：

```text
revision=40: stopping / cancel-requested
revision=41: idle / cancelled
```

不得使用 timeline generation、runtime revision 或 prompt queue revision 代替 ACP revision。

### 7.2 轻量事件

新增或收紧现有会话事件 payload：

```text
SessionLifecyclePatch {
  locator,
  branchId,
  operationId,
  acpRevision,
  turnId,
  availability,
  liveTurnActivity,
  latestTurnStatus,
  stopping,
  stopReason
}
```

terminal lifecycle patch 不包含：

- timeline 正文；
- semantic blocks；
- diagnostics/raw；
- 全量 Todo/usage 历史；
- 完整 `AcpSessionVm`。

正文只在首次进入、分页、水位缺口或显式详情请求时读取。

### 7.3 前端 facet 合并规则

`mergeConversationAttemptLifecycle()` 改为按 facet revision 合并：

- incoming revision 大于 current：接受该 facet；
- incoming revision 小于 current：丢弃；
- revision 相同且 turn identity 相同：terminal 优先于 non-terminal，禁止 `cancelled -> stopping/running`；
- turn identity 不同：只有更高 ACP revision 才能开始新 turn；
- runtime、ACP、promptQueue 分别合并，不能因某一 facet 较新而整体覆盖其他 facet；
- operation ID 只用于幂等和诊断，不替代 revision。

session status 可以作为缺失 lifecycle 的冷启动恢复输入，但一旦生成带 revision 的 ACP facet，前端只能按上述规则推进。不得再使用“lifecycle 存在就无条件忽略 terminal session”或“任意 terminal session 无条件压过当前新 turn”两种极端规则。

### 7.4 AUTO 并发 sibling

AUTO 停止父 runtime 时可以同时取消多个 sibling，但每个 leaf lifecycle 使用自己的完整 attempt locator 和 ACP revision。父 runtime 的 `runtime.revision` 只描述整图 pause/continue，不代替 leaf ACP terminal。

验收必须覆盖：

- 两个 sibling 同秒 terminal；
- sibling terminal 顺序不同；
- 当前选中 leaf 的 terminal 先于 stop accepted 响应；
- 后台 leaf terminal 不抢占用户手动选中的 session；
- 父 runtime 已 paused、某个 leaf terminal event 迟到时仍单调收敛。

---

## 8. 旧会话迁移与异常恢复

### 8.1 新会话

- 创建 attempt 时初始化 index format/generation 和 ACP lifecycle revision。
- 第一条 timeline patch 起由 TimelineStore 维护 index/projection。
- 正常关闭与 terminal 强制 checkpoint。

### 8.2 旧会话

- 首次打开发现 index 缺失时，返回明确的 migration/recovery 状态，由 blocking pool 完整扫描一次。
- 重建期间同 attempt 只允许一个 owner；其他请求等待同一结果或返回可重试的结构化状态，不并发重复扫描。
- 成功后原子写 V4 index 和迁移完成水位；之后正常查询不得再走 full scan。V2 相比 V1 增加 Agent launch/prompt/result locator 语义，V3 增加 processing retry identity，V4 增加语义块 `lastRevision`；旧版本必须整体重建，不能依赖 serde 默认值继续使用。
- 停止 accepted 不等待迁移；后台 terminal settlement 可等待同一重建 owner。

### 8.3 Index 损坏或版本变化

以下情况触发一次重建：

- index 缺失；
- format version 不支持；
- timeline generation 不匹配；
- covered offset 超过文件长度；
- checkpoint 校验失败；
- compaction 后旧 cursor/index 仍被使用。

错误使用结构化 code + params，例如：

- `acp.timeline-index-rebuild-required`；
- `acp.timeline-index-version-unsupported`；
- `acp.timeline-generation-mismatch`；
- `acp.timeline-checkpoint-corrupt`。

后端不包含对客文案；前端仅在重建确实阻塞当前首次展示时通过 i18n 映射状态。

### 8.4 Compaction

compaction 在同一 timeline 锁内完成：

1. 根据 canonical projection 写新 timeline 临时文件；
2. 分配新 generation；
3. 为新文件构建完整新 index；
4. 原子替换 timeline/index；
5. 发布 generation change 水位。

若平台无法保证两文件同时原子替换，index 必须通过 generation/文件签名检测中间态并重建，绝不能继续使用旧 offset。

---

## 9. 接口与代码落点

### 9.1 Rust 数据层

| 文件 | 目标修改 |
|---|---|
| `src/acp/timeline.rs` | 扩展 TimelineStore、index/checkpoint、tail replay、分页、CAS settle、compaction generation |
| `src/acp/events.rs` | 保留模型/构造，删除按全历史寻找 retry/pending interaction 的业务修改入口 |
| `src/acp/client.rs` | 使用 TimelineStore 写入；terminal finalizer 发布轻量 lifecycle；保留统一 10 秒 provider deadline |
| `src/acp/control.rs` | snapshot/session lifecycle header 与稳定 pending identity；停止 latch 和终态幂等 |

### 9.2 Tauri 接口层

| 文件 | 目标修改 |
|---|---|
| `src-tauri/src/commands.rs` | stop accepted 只返回轻量控制结果；cleanup 不构建完整 session；事件发布区分 lifecycle 与正文 |
| `src-tauri/src/view_models.rs` | `get_acp_session` 改为 index current-page 查询；移除正常路径 full scan/repair |
| `src-tauri/src/view_models_conversation.rs` | Conversation ACP facet 暴露独立 revision/turn identity |

### 9.3 前端

| 文件 | 目标修改 |
|---|---|
| `web/src/types.ts` | 增加 ACP facet revision 和 SessionLifecyclePatch 类型 |
| `web/src/lib/acp-runtime-composer-state.ts` | lifecycle 按 facet revision/terminal dominance 合并 |
| `web/src/components/acp/ACPChatDialog.tsx` | subscribe-before-read、水位回放；stop response 不触发正文终态补拉 |
| `web/src/pages/ConversationRunPage.tsx` | 轻量 leaf lifecycle patch，不为普通 terminal patch 刷新完整 run/session 正文 |

### 9.4 配置与观测

checkpoint policy 统一归属 TimelineStore，当前包含：

- checkpoint patch interval；
- tail replay hard limit；

runtime 直接拥有当前 attempt 与已打开 Agent branch 的 TimelineStore，并在 session 结束时释放；查询在 blocking pool 中短暂读取 checkpoint，因此没有新增全局 registry 或无界驻留缓存。

Release 诊断记录有界数值，不记录正文：

- `timelineOpenMode = checkpoint | tail-replay | legacy-rebuild | corrupt-rebuild`；
- processed patch count；
- page locator/read count；
- checkpoint bytes/time；
- lock wait/hold time；
- lifecycle patch revision、丢弃旧 revision 次数；
- stop accepted、provider wait、terminal publish 分段耗时。

---

## 10. 分阶段实施

### M1：生命周期单调契约

- 为 ACP facet/session header 增加 revision 与 turn identity。
- 后端发布轻量 lifecycle patch。
- 前端按 facet revision 合并，固化 terminal-before-accepted 回归。
- cleanup 不再以完整 session 作为终态通知。

M1 是最终架构的控制面基础，不是针对当前页面增加局部 if。

### M2：TimelineStore 写入所有权

- 所有 timeline append/upsert 收口到 TimelineStore。
- 实现内存 projection、item locator 和 checkpoint policy。
- retry、permission、elicitation 改为稳定 identity + CAS settle。

### M3：有界查询与重进

- 实现 index current-page 查询和 opaque cursor。
- `get_acp_session` 只返回 snapshot + page + watermarks。
- 前端实现 subscribe-before-read 和 generation/revision gap reconciliation。

### M4：迁移与修复边界

- 旧会话一次性 index rebuild。
- 将 usage/diagnostics/raw/lifecycle repair 移出正常查询。
- 删除活跃会话绕缓存全量扫描以及查询时 fallback 修复。

### M5：Compaction 与性能验收

- compaction 同步生成新 generation/index。
- 使用真实 2536-event 会话和 10,000+ revision 合成会话跑 Release 基线。
- 达标后删除旧 full-scan 正常消费路径，不保留双主路径兼容层。

---

## 11. 测试与验收矩阵

### 11.1 TimelineStore 单元测试

- append 后 index locator、semantic order 和 projection 正确更新。
- 同 item 多 revision 只暴露最新记录。
- checkpoint 后追加 `Δ` 条，重新 open 只处理 `Δ`。
- 模拟“timeline 已 flush、index 未写”后恢复，结果与完整回放一致。
- index 缺失/损坏/generation 不匹配只重建一次。
- compaction 后旧 cursor 被拒绝，新 index 与 canonical timeline 等价。
- CAS settle 只允许期望 revision/status，重复停止返回 `AlreadyTerminal`。
- root 与多个 Agent branch 的 Todo/pending/page projection 互不污染。

### 11.2 接口级回归

- stop accepted 期间断言未调用 timeline open/full-scan/session VM 构建。
- terminal lifecycle payload 断言不含 events、timelineProjection、diagnostics 或 raw。
- terminal revision 先到、accepted revision 后到，composer 最终仍为 normal/cancelled。
- accepted 后 terminal 正常到达，状态只前进一次。
- 两个 AUTO sibling 同时取消，各自按 identity/revision 收敛。
- terminal event 丢失后重新进入，从 lifecycle header 恢复 terminal，不重建 stopping。
- retry backoff、初始化、active prompt、permission、elicitation 五种阶段停止都幂等 terminal。
- 首次进入、向前/向后分页和 live gap catch-up 的处理量不超过 `P + Δ`。
- 10,000+ revision 会话断言正常 stop/query 不进入 `legacy-rebuild/full-scan` 模式。

### 11.3 性能验收

以本轮约 2536 个事件、查询约 32 秒的真实会话作为固定基线：

- stop accepted 耗时与 timeline 大小无关，且读取正文记录数为 0；
- provider cancel 最长仍为 10 秒，分段指标证明不包含投影重建；
- stop cleanup 处理记录数为常数或 checkpoint 后有界尾部；
- 重进处理记录数不超过页面大小 + checkpoint 尾部；
- 同一真实会话 Release 后端查询目标小于 1 秒；
- terminal lifecycle patch 序列化大小与历史长度无关；
- 10,000+ revision 下无全量扫描、无长时间 timeline 锁、无无界内存增长。

性能测试必须同时记录数据规模、读取记录数、锁等待/持有、I/O 字节和分段耗时，不能只看总墙钟时间。

---

## 12. 方案自评审

### 12.1 根因

这是 timeline 物化投影/checkpoint 未完成，以及 ACP lifecycle 缺少单调 revision 的架构缺口。完整 session 查询、停止控制和终态发布耦合在一起，使性能问题与状态乱序相互放大。方案同时修复数据面与控制面，不增加特定会话或特定事件数的补丁。

### 12.2 行业实践与依赖

采用 append log + materialized projection + checkpoint + tail replay，以及 revision/CAS 的状态合并，是事件日志恢复的常见实践。现有 Rust、Serde、文件锁和原子写入能力足够，不新增运行时依赖。

### 12.3 性能影响

- 正常查询从 `O(N)` 降为 `O(P + Δ)`。
- 精确停止结算从全量寻找降为 locator/CAS 的 O(1) 定位加单条 append。
- terminal 状态发布从完整 session 重建降为常量大小 patch。
- checkpoint 采用 policy 批量落盘，避免每条 patch 重写随历史增长的完整 index。
- 内存 projection 按打开 attempt 有界持有，禁止全局无界缓存。

### 12.4 过度设计评审

- 复用现有 TimelineStore、snapshot、session header 和 conversation lifecycle。
- 不新增数据库事实源、不新增全局 revision、不引入第二状态机。
- index 是可删除重建的读取投影；只有实际存在的分页、停止结算和恢复字段进入 index。
- generation、facet revision 和 operation ID 分别解决 offset 失效、状态单调和命令幂等，职责不重复。

### 12.5 正确性

- timeline append 是正文提交点；index 落后通过 tail replay 恢复。
- item 结算使用稳定 identity + expected revision/status，重复执行幂等。
- lifecycle 按 ACP facet revision 单调合并，terminal 不被旧 stopping 覆盖。
- compaction 通过 generation 使旧 offset/cursor 明确失效。
- legacy full scan 被限制在显式、单 owner 的迁移/损坏恢复边界，不能回到正常查询或停止热路径。

---

## 13. 完成定义

以下条件全部满足才视为方案完成：

- [x] TimelineStore 成为正常 timeline 读写边界；legacy rewrite 只保留在显式迁移与测试 fixture。
- [x] 新会话写时维护 index/checkpoint，崩溃后只回放尾部。
- [x] 正常会话查询和重进不再全量扫描。
- [x] stop accepted 不读取正文，不构建完整 session。
- [x] retry、permission、elicitation 按稳定 identity/CAS 结算。
- [x] terminal 只发布轻量 lifecycle patch。
- [x] ACP facet revision 在后端持久化并由前端单调合并。
- [x] AUTO 两个 sibling 依次停止不会永久 stopping，先停止 leaf 不被迟到事件恢复。
- [x] 查询时 repair 已迁出正常 session query。
- [x] legacy/index 损坏或版本不兼容只触发一次 blocking rebuild。
- [x] 两个真实慢会话的 checkpoint-backed Release 查询均小于 1 秒。
- [x] 10,000+ revision 回归证明 checkpoint 后查询不调用 full-scan 路径。
- [x] 产品设计、开发计划、接口测试和性能基线同步更新。

---

## 14. 实施记录与验收结果

### 14.1 实际落点

- `acp.timeline.index.json` 使用 format V4；V2 已保存 Agent launch/prompt/result 角色、已接受 prompt ID 与最新 runtime control output candidate，V3 增加可由 timeline 重建的 retry-prompt role 与当前 processing retry ID，V4 增加语义块 `lastRevision` 以支持 revision delta。旧版本不兼容，打开时完整重建并把 generation 从旧值单调加一，避免旧 checkpoint 因 serde 默认值静默缺少停止或分页所需身份。
- TimelineStore 默认每 256 个 patch checkpoint，tail replay hard limit 同为 256；terminal 与正常 session 写回强制 checkpoint。timeline patch 在文件锁内 append + flush，index 使用原子替换。compaction 在锁内重新读取最新 checkpoint 后才判断与分配 generation，多个 writer 不会用陈旧 generation 覆盖彼此。
- JSONL 健康 append 的尾部完整性检查从“每次读取整文件”收敛为只读取最后 1 字节；只有末行缺失换行时才进入恢复扫描，消除了长流式会话写入的 O(N²) 放大。
- `scan_acp_timeline()` 正常路径只调用 indexed current-page query；旧 parser、semantic pagination、diagnostics scan 和 Agent rebuild 均限制为 `#[cfg(test)]` oracle。旧会话首次迁移在 Tauri blocking pool 中执行。
- retry、permission、elicitation、prompt queue accepted 恢复和 runtime artifact 展示标注均使用 index identity/locator；retry 停止优先使用 index 中最新 processing identity，snapshot/session identity 只作 durable hint，因此 timeline append 与 metadata rewrite 之间停止仍可精确结算。停止 accepted 路径不打开 timeline。Agent index 从 root launch projection 递归读取各 branch projection。
- session update emitter 只发布轻量 lifecycle；ACP metadata 持久化 revision、turn/prompt identity、operation ID 与 live activity。前端按 runtime/ACP/promptQueue facet revision 合并，并固定 terminal-before-accepted dominance。

### 14.2 Release 真实基线

测试使用原会话 timeline 的临时副本，未修改用户会话：

| Fixture | Timeline | 一次迁移 | 第二次 current-page 查询 | 返回页 | tail | index bytes |
|---|---:|---:|---:|---:|---:|---:|
| task-276 | 4.39 MB / 1699 行 | 1607 ms | 191 ms | 30 | 0 | 548,323 B |
| task-284 | 4.04 MB / 1071 行 | 2085 ms | 227 ms | 30 | 0 | 780,370 B |

两条 checkpoint-backed 正常查询均低于 1 秒目标；首次 legacy migration 是显式的一次性 O(N) 恢复边界。10,000 revision 合成回归的测试体耗时约 0.60 秒，第二次查询 `processed_tail_records=0`。

### 14.3 自评审结论

- 根因修复：控制面不再依赖正文，正文查询不再从头投影；没有用超时、后台全量扫描或缓存掩盖设计缺口。
- 过度设计：复用 TimelineStore、文件锁、Serde、原子写和既有 lifecycle；没有新增数据库、依赖、全局 registry 或第二状态机。pending retry ID 与 V4 `lastRevision` 都由同一 timeline 事件增量物化且可删除重建，只封闭已证实的跨文件落盘窗口与 revision 分页边界，不复制 canonical 业务事实。
- 性能：正常查询读取当前页与有界 tail；terminal payload 为常量大小；checkpoint 从每 patch 重写改为 256 patch 批次；健康 append 不再全文件扫描。代价是一次性 V1/legacy rebuild 与 index 文件磁盘空间，均有明确恢复边界和真实基线。
- 正确性：timeline 先提交、index 可重建；CAS settlement 幂等；schema/compaction 都推进 generation；ACP revision 单调且 terminal dominance；多个 TimelineStore writer 在同一文件锁内刷新 projection 后写入。

### 14.4 桌面与真实会话验收

- Direct task-030：真实 `Start-Sleep -Seconds 30` 运行中点击停止，命令约 156 ms accepted；provider 在 10 秒上限前正常返回，`acp_prompt_terminal_quiet_drained elapsedMs=213`。最终 ACP snapshot/timeline 为 `cancelled`，run/node 为 `paused / process-interrupted`，没有固定等待 10 秒。
- AUTO task-032 run-002：两个并行 leaf 同时启动。`create-c` 在 `Start-Sleep` 活跃时停止，stop command 约 244 ms accepted；随后切入仍活跃的 `create-d` 并停止，stop command 约 145 ms accepted。两条 provider 路径都在统一 deadline 内收敛，没有永久 stopping。
- durable 核对：dynamic graph 为 `0.3`；`create-c` 与 `create-d` 均为 `status=paused`、`pauseReason=process-interrupted`、`runtimeExecutionPhase=paused`，ACP snapshot 均为 `latestTurnStatus=cancelled`；dynamic run、outer attempt、round 与 run 均为 `paused / process-interrupted`。切换 sibling 后先停止的 `create-c` 没有被迟到事件恢复。
- task-026 历史 graph 经 `0.2 -> 0.3` 一次性迁移后，`hello-world-worker` 与 `goodbye-worker` 都获得 durable `paused` leaf runtime facet，重复读取不再写盘。
- permission、elicitation、retry backoff、terminal event 丢失恢复需要协议态或故障注入，不在本轮桌面手工造假；这些路径由接口级回归固定 stable identity/CAS、revision 单调、terminal dominance 和重进恢复契约。桌面验收结论只覆盖 Direct 与 AUTO 的真实 active prompt 主路径。

最终验收表明：控制面 accepted 延迟与 timeline 大小无关；provider 取消可提前完成，最慢受统一 10 秒 deadline 约束；终态发布、AUTO leaf 聚合和会话重进均不再依赖完整 timeline 重建。

### 14.5 长会话重进补充修复（2026-08-19）

task-284 现场日志确认 checkpoint-backed 当前页查询约 388ms，但页面重进仍以 `headSeq` 追逐持续到达的 live head，并按 40ms 至 2s 指数退避重复查询；停止终态后还存在正文 session 构建和前端补拉。这不是 index 失效，而是 snapshot/live 交接仍使用展示序号、控制面仍残留正文消费。

本轮完成以下收口：

- index 升级到 V4，语义块维护 `lastRevision`；查询增加 `afterRevision`，页面返回 `generation / coveredRevision / newestRevision`，同 revision 作为原子分页组。
- live envelope 增加可空 `timelineGeneration + timelineRevision`。只有与 TimelineStore 中语义指纹一致、已经 durable 的事件获得水位；transient timing 和尚未 flush 的累计流不承诺持久水位。terminal/normal flush 顺序调整为 timeline patch 先于 live flush；compaction 后旧 generation 的 revision 不与新 generation 混合比较。
- replay buffer 用 `lossWatermarkRevision` 取代移动的 `headSeq + requiresCatchUp`。只有 durable payload 淘汰才推进水位；重进捕获固定目标，用 revision delta 有界追平，删除指数退避轮询。
- prompt finalizer 不再构建/发布完整 `AcpSessionVm`，停止完成后前端不再补拉正文；`get_conversation_run` 始终只返回 tree/lifecycle shell，ACPChatDialog 成为正文唯一查询边界。

接口回归覆盖 revision delta、10,000 revision checkpoint、transient event 不推进缺口、durable 淘汰固定水位、单次 revision catch-up 与 lifecycle-only terminal。复杂度保持 `O(P + Δ)`；未新增依赖、数据库、全局缓存或第二状态机。

task-284 原 timeline 副本的 V4 Release 复测：一次性 V3→V4 migration 2,979ms；随后 checkpoint-backed 当前页 297ms，30 个语义块、tail 0、index 839,075B，低于 1 秒目标。原会话文件未被修改。

自评审：该改动补齐既有 append log + materialized projection 设计的水位契约，属于根因修复。新增字段均来自现有 timeline revision/semantic block，可删除重建；固定 loss watermark 的内存量为每 branch 一个整数，和历史长度无关，因此不存在过度设计或无界内存风险。停止控制面不再执行正文 I/O，重进不再因持续 live 到达而延长等待。

### 14.6 实现后代码审查收口（2026-08-19）

实现完成后的 identity、compaction、并发与 I/O 审查发现并修复四项边界：

- `submit_conversation_prompt` admission 后，后台 helper 曾再次调用 `begin_session_turn()`；虽因相同 turnId 未重复 revision，但新 operationId 会被静默忽略。现已拆分为 `admit_conversation_prompt_turn` 与 `execute_admitted_acp_prompt_with_configured_app`，前端提交只 admission 一次，scheduled/queue 入口仍由公共 helper admission 后执行。
- prompt admission 进一步把完整 `promptSubmission` 与 lifecycle 原子写入同一 attempt metadata，并返回 `Started / ExistingActive / ExistingTerminal`。只有 `Started` 启动后台 Provider；相同 turn 的请求重试返回既有 revision，不同 payload 结构化冲突。后台只从 durable submission 读取正文、引用和附件，发送前及初始化阶段继续检查同 turn terminal/cancel；进程重启后的 orphan `Starting/Accepted/Running` submission 收敛为 `failed + process-interrupted`，已 durable `CancelRequested` 收敛为 `cancelled`。
- 手动 queue use 删除命令成功快速返回后的即时 `settle_dispatching_prompts`；admission 前同步拒绝时按 item identity 立即恢复，成功 admission 后只由 Provider accepted callback 删除，或在后台 terminal/failure 后恢复。这样既不会在 `acp-session-started` 与 canonical prompt 之间把同一项错误放回队列，也不会让同步拒绝的项目永久停在 `Dispatching`。进程重启后的旧 `Dispatching` 若对应 turn 已 terminal，则只结算该 item：timeline 已有 accepted prompt 时删除；否则保留 payload 并换发新的 turn ID 后再次 admission，避免旧 terminal identity 让队列永久无法重试，也不影响同 attempt 的其他 dispatch。
- 旧 `switch_conversation_session` 会与 ACPChatDialog 并发读取同一正文，`send_acp_prompt` 又保留已失效的完整 session 返回语义。两条旧 IPC 与前端封装已删除；run aggregate、选择操作只维护 locator/lifecycle，ACPChatDialog 是正文唯一查询者。
- 代码审查又移除了未被前端消费、但仍会在停止时构建完整 `AcpSessionVm` 的旧 `cancel_acp_session` IPC；停止控制面现在只保留 accepted/lifecycle 入口，避免任何遗留调用绕过轻量停止契约。
- compaction 后 snapshot generation 大于旧 loss watermark generation 时，旧严格相等条件会使缺口永久无法确认；迟到的旧 generation live 也可能回退 router 水位。现改为 generation 单调合并：旧 event 丢弃，新 generation 清理旧 retained window，新 snapshot 可确认更早 generation 的缺口。
- durable watermark 曾对 live item 再次执行 blob externalize 与 semantic fingerprint，大 raw payload 会重复 BLAKE3/Serde 工作。现由 TimelineStore 同一次 upsert 的 index locator 直接返回 generation/revision，并随 pending live item保存；没有新增缓存或第二事实源。

同时删除 Agent branch 对纯 lifecycle patch 的正文补查。新增回归覆盖：新 generation 确认旧缺口、迟到旧 generation 丢弃、compaction 后重进不重复刷新、Agent branch lifecycle-only 不查询正文，以及同 turn admission identity。正常重进仍为 `O(P + Δ)`，live 每次写入后的 watermark 获取为 O(1) locator 查询；删除两个重复正文入口后，同一次会话选择只发起一次正文请求。

复核发现停止 accepted 后前端会先清空本地 turn identity，而 terminal 可能只以 lifecycle patch 到达；composer 的 `sending / awaitingResponse / cancelling` 清理必须同时接受无正文的 ACP lifecycle terminal（`idle + latestTurnStatus != none + stopping=false`），不能只等待完整 session status。存在 ACP facet 后，session status 只保留为 lifecycle 缺失时的冷启动 fallback，防止旧 `completed/cancelled` snapshot 在新 turn 已 admission/running 时提前清理 local transient。该收口不增加正文查询，且保留 turn identity 匹配用于仍持有本地 turn 的路径。

自评审结论：改动收紧既有边界，没有新增 aggregate、状态机、依赖或持久事实；删除的旧接口属于开发阶段被明确替代的路径。内存仍受每 branch 64 事件/512 KiB、全局 4 MiB 上限约束；锁范围没有扩大，provider 调用仍不在 timeline/lifecycle 锁内。

### 14.7 三类并发边界补强（2026-08-19）

本轮针对实现审查发现的三个真实竞态完成收口：

- `ACPChatDialog` 的 revision catch-up 在查询异常时保留 replay watermark，不再跳出后无条件标记 ready；补偿查询复用已有有界退避，只有 ACK 成功才打开 live animation gate。失败达到上限时仍保持未确认，后续重进可继续补拉，避免丢弃的 live event 永久丢失。
- prompt queue 新增按 `promptId` 定位的单项 settlement。Direct drain、queued dispatch 和后台失败回收都只结算自己的 dispatch；全量 settlement 只保留给重启/孤儿恢复，避免一个 turn 的完成或失败释放同 attempt 其他正在 admission 的队列项。
- `spawn_blocking`/Provider 初始化等外层失败路径先按同一 turn ID 持久化 failed terminal，再发布 `turn-finished`。若 durable 写入失败或该 turn 已被更晚 terminal 覆盖，则不发布迟到的失败终态，保持 lifecycle canonical state 的单调性。

接口回归新增单项 queue settlement 隔离测试，以及 replay delta 临时失败后保持静态、按退避重试并在 ACK 后收敛的前端测试。新增逻辑只读取单个 queue item、单页 delta 和固定大小 lifecycle header；没有全量 timeline 扫描、无界重试、额外缓存或第二状态机。

### 14.8 会话停止继续与 Timeline 运行态恢复根因修复（2026-08-20）

本轮把停止控制面和运行态恢复的剩余根因收敛到既有 append log + materialized index 设计：

- stop 只取消当前 turn，不关闭 provider session；`availability` 保持 session 可用性，`cancelRequested` 只存在于 live turn activity。canonical reducer 以 `turnId + operationId + revision` 做 CAS，terminal 保证 `idle + outcome + non-closing availability`，并让 durable cancellation intent 胜过迟到 provider completion。
- `AcpRuntime::from_connection` 使用 index snapshot、bounded hot interaction 和 active stream snapshot；attached reuse、resume、new 路径不调用完整 Timeline replay，也不 hydrate Blob。`Load + externalSessionSyncEnabled` 才读取 prompt anchors 供 provider replay importer 使用。
- runtime restore 明确记录 `index-hit / tail-replay / full-rebuild`。index 缺失、损坏、版本不兼容、tail 超限和 compaction 才进入全量路径；tail 超限触发的重建与启动 compaction 均会报告 `full-rebuild`，避免性能诊断误报。
- active stream snapshot 与旧完整重放保持 parity：text/thought/plan 在 tool boundary 处保留 stable provider identity 的 suspended stream，匿名流仍按既有规则关闭；branch 合并按 branch + item identity 去重。
- `repair_attempt_usage` 只读取 usage journal 和 prompt locator，不再通过 `load_timeline_items` 无条件 hydrate Blob。性能验收记录 `restoreMs`、restore mode、tail records、locator reads、full timeline item load 和 hydrated Blob bytes；正常 attached/resume 路径 Blob bytes 必须为 0。

接口级回归新增 cancellation intent 与 provider completion 竞态、tail 超限 full-rebuild 诊断、active stream tool/revision parity；既有 10,000 revision、Blob reference、prompt anchor 延迟读取和 branch identity 测试继续作为回归门槛。自评审：未新增数据库、消息队列、全局状态机或无界缓存，复用现有 revision/CAS、Timeline index 和原子 JSON 写入，正常恢复复杂度与历史正文大小解耦。
