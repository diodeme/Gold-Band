# 会话停止继续与 Timeline 运行态恢复根因修复方案

本次修复分两条主线：

1. 继续/恢复会话时，不再默认全量重放所有 branch timeline 和 Blob。
2. 把 stop lifecycle 收敛到一个 canonical reducer，消除 `closing + idle + cancelled` 这类非法组合。

不建议引入数据库、工作流框架或新的全局状态机。复用现有的 timeline index、revision、operation ID、owner CAS 和原子 JSON 写入。

## 一、先明确不变量

### 1. Session availability 与 Turn lifecycle 分离

`availability` 表示 ACP session 是否可以继续使用：

```text
established
restorable
unavailable
```

`liveTurnActivity` 表示当前 turn：

```text
idle
starting
accepted
running
cancelRequested
```

`latestTurnStatus` 表示上一个 turn 的结果：

```text
none
completed
cancelled
failed
```

用户点击“停止”时，停止的是当前 turn，不是关闭 session。因此：

```text
StopRequested:
  availability      保持原值
  liveTurnActivity  = cancelRequested
  latestTurnStatus  = none

StopSettled:
  availability      established/restorable/unavailable
  liveTurnActivity  = idle
  latestTurnStatus  = cancelled
```

`availability=closing` 只允许用于真正的 session/provider close 流程。如果当前项目没有独立的 close 语义，可以保留枚举用于兼容，但 stop 不能再写入它。

### 2. 终态不变量

任何终态必须满足：

```text
liveTurnActivity == idle
latestTurnStatus != none
availability != closing
```

任何非终态必须满足：

```text
latestTurnStatus == none
```

所有展示状态都从这组 canonical 字段投影，不能再由不同模块各自推导一套状态。

## 二、生命周期修复方案

### 1. 在 `src/acp/events.rs` 增加统一 reducer

重点修改文件：

- `src/acp/events.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/view_models.rs`
- `src-tauri/src/view_models_conversation.rs`

建议增加内部事件类型：

```rust
enum AcpLifecycleTransition {
    PromptAdmitted(AcpPromptSubmission),
    ExecutionClaimed,
    StopRequested {
        operation_id: String,
        decided_at: String,
    },
    TurnSettled {
        status: AcpLatestTurnStatus,
        reason: String,
        decided_at: String,
    },
    OrphanedTurnRecovered {
        status: AcpLatestTurnStatus,
        reason: String,
        decided_at: String,
    },
}
```

不需要把这些事件另存为新的 event log。它只是统一修改 canonical metadata 的内部 command。

统一入口类似：

```rust
fn apply_lifecycle_transition(
    path: &Utf8Path,
    expected_owner: Option<&AcpLifecycleOwner>,
    transition: AcpLifecycleTransition,
) -> Result<AcpLifecycleTransitionResult>
```

这个函数必须：

1. 获取现有 `session_metadata_lock`；
2. 读取当前 metadata；
3. 校验 `turn_id + operation_id + revision`；
4. 根据 transition 修改完整 lifecycle header；
5. 校验终态/非终态不变量；
6. revision 单调递增；
7. 使用现有 `write_json()` 原子替换；
8. 终态时清理 active turn registry；
9. 返回最新 canonical header。

provider 网络调用不能放进锁内，只能在持久化 transition 前后执行。

### 2. 让现有写入口全部调用 reducer

以下函数不能再分别实现自己的生命周期字段逻辑：

- `begin_session_turn`
- `claim_session_turn_for_execution`
- `request_session_stop`
- `persist_session_turn_terminal_owned`
- `persist_session_terminal`
- `reconcile_orphaned_session_turn`
- `write_session_metadata`
- `write_session_metadata_owned`

其中：

- `write_session_metadata*` 只能合并 provider 的非生命周期字段；
- 生命周期字段必须以当前 canonical header 为准；
- provider 迟到写入不得重新推导 `availability` 或 `latestTurnStatus`；
- terminal 状态优先级高于 provider 的 running/accepted metadata。

### 3. stop 流程保持两阶段，但每阶段只提交 canonical transition

流程应该是：

```text
1. request_session_stop
   持久化 StopRequested
   liveTurnActivity = cancelRequested
   availability 不变

2. 锁外发送 provider cancel

3. persist_session_turn_terminal_owned
   使用 turn_id + operation_id + expected_revision CAS
   持久化 TurnSettled(cancelled)

4. 如果进程崩溃
   reconcile_orphaned_session_turn
   将 durable cancelRequested 收敛为 cancelled
```

`request_session_stop` 的结果必须区分本次请求是否取得 stop owner。已经处于 `cancelRequested` 的重复请求，以及已经进入 terminal 的请求，都是幂等 no-op：调用层不得再次写 attempt-wide provider cancel latch、暂停运行态或启动旧 turn cleanup。否则旧 stop 在 terminal 后新 turn admission 的窗口内可能误取消新 turn。只有本次 transition 返回的 `turnId + operationId + revision` owner 才能执行 provider cancel 和后续 terminal settlement。

当 canonical lifecycle 已经是 `idle + latestTurnStatus=none` 时，停止同样是 no-op，不创建 `stop:<operation>` synthetic turn；retry backoff 的取消继续使用独立的 processing-prompt retry 接口。

重复 stop 必须幂等：

- 已经 `cancelRequested`：返回当前 operation；
- 已经 `cancelled`：返回 terminal；
- 旧 owner 迟到：返回 stale/no-op；
- 新 turn 已 admission：旧 stop 不得影响新 turn。

这里不需要传统数据库事务。外部 provider 调用无法参与本地事务，正确模型是“durable intent + 幂等 terminal settlement + crash recovery”。

### 4. 修复终态 availability

统一实现一个 helper：

```rust
fn terminal_availability(value: &Value) -> AcpSessionAvailability
```

建议复用现有 `persist_session_terminal()` 的策略：

- metadata 中存在 session ID：`Established`；
- 没有 session ID：`Unavailable`；
- 如果已有明确 `Restorable` 语义，则保持 `Restorable`。

所有 terminal transition 都调用这个 helper，不能只修 `persist_session_turn_terminal_owned()`。

同时处理已经存在的旧数据：

- `availability=closing`
- `liveTurnActivity=idle`
- `latestTurnStatus=cancelled`

这类数据在读取投影时必须返回 `cancelled`，不能继续返回 `closing`。

`session_metadata_status()` 的判断顺序应改为：

```text
1. activity == cancelRequested => cancelling
2. activity == idle && latestTurnStatus == completed => completed
3. activity == idle && latestTurnStatus == cancelled => cancelled
4. activity == idle && latestTurnStatus == failed => failed
5. availability == established/restorable => idle
6. 其他 => unknown
```

不要让 `availability=closing` 永远覆盖 terminal outcome。

如果项目确实存在旧 metadata 迁移路径，应在迁移时一次性修正旧的 terminal closing 数据；不要添加长期双写兼容逻辑。

### 5. 更新所有 projection

检查并统一：

- `session_metadata_status`
- `acp_session_status`
- `conversation_branch_status`
- `apply_acp_lifecycle_header`
- `normalize_preloaded_session_metadata`
- `acp_session_availability`

stop accepted 阶段应该展示 `cancelling`，terminal 阶段展示 `cancelled`。

只有真正 session close 才允许投影为 `closing`。

## 三、继续会话的 timeline 恢复优化

重点修改文件：

- `src/acp/client.rs`
- `src/acp/timeline.rs`
- `src/acp/events.rs`
- `src/acp/history.rs`
- `src/acp/usage.rs`

### 1. 不要直接删除 `load_all_branch_events`

它仍然可能用于：

- UI/详情读取；
- 一次性修复；
- 测试 oracle；
- index 损坏后的 fallback rebuild。

本次要做的是：从 `AcpRuntime::from_connection()` 的正常初始化路径移除它。

### 2. 从 timeline index 构造轻量 restore snapshot

在 `timeline.rs` 增加类似：

```rust
pub struct TimelineRuntimeRestore {
    pub covered_revision: u64,
    pub latest_seq: u64,
    pub projection_locator_scans: usize,
    pub pending_permissions: Vec<AcpUiEvent>,
    pub pending_elicitations: Vec<AcpUiEvent>,
    pub pending_retry_prompt: Option<AcpUiEvent>,
    pub timing: AcpTimingStateSnapshot,
    pub hot_items: Vec<AcpUiEvent>,
    pub active_stream_items: Vec<AcpUiEvent>,
    pub active_context_compaction: Option<AcpUiEvent>,
}
```

这个 snapshot 必须来自 `TimelineMaterializedIndex` 和少量 locator 定点读取，不能调用 `load_timeline_items()`。

需要注意：

- `pending_permissions`、`pending_elicitations`、retry event 允许读取少量事件；
- 大型 raw/tool output 不得 hydrate；
- active stream snapshot 必须有大小上限，复用当前 runtime hot item 的上限；
- provider history identity membership 由已加载的 `TimelineStore` index 直接 O(1) 查询，不再 clone 到 restore snapshot；
- provider prompt anchors 不属于启动 snapshot，只有显式 `session/load` 且启用 external history sync 时才按 locator 读取。

`TimelineMaterializedIndex` V7 增加 `runtimeProjection`，由统一的 `apply_index_event()` 增量维护：

- `latestSeq`；
- active tool item IDs；
- 最新 context-compaction 候选；
- 每个 branch 的 text/thought/plan current 与 suspended stable 槽位；
- provider history identity 引用计数。

index-hit restore 只枚举上述有界集合并读取对应 locator。旧索引升级或检测到改变语义顺序的迟到历史 patch 时允许在索引构建/写入路径重建 projection；不得把全量 locator 扫描留在每次启动路径。

### 3. 不要用 `AcpTimingPatch` 直接冒充 `AcpTimingState`

当前 `AcpTimingState` 内部有：

- elapsed baseline；
- active turn start；
- last activity；
- permission/elicitation wait；
- revision；
- saw turn；
- accumulated wait seconds。

应增加明确的轻量 snapshot：

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct AcpTimingStateSnapshot {
    elapsed_seconds: u64,
    active_turn_started_at: Option<u64>,
    active_turn_last_activity_at: Option<u64>,
    revision: Option<u64>,
    saw_turn: bool,
    pending_permission_ids: Vec<String>,
    pending_elicitation_ids: Vec<String>,
    user_wait_started_at: Option<u64>,
    user_wait_seconds: u64,
}
```

提供：

```rust
impl AcpTimingState {
    fn snapshot(&self) -> AcpTimingStateSnapshot;
    fn from_snapshot(snapshot: AcpTimingStateSnapshot) -> Self;
}
```

这样可以保持新旧恢复结果一致，不要通过最新 `AcpTimingPatch` 猜测内部状态。

### 4. active stream 也要保存轻量 snapshot

`active_timeline_streams_by_branch()` 现在依赖完整事件数组。应把当前 active stream 的必要字段抽成可序列化 snapshot：

```text
branch_id
stream kind
item_id
source_id
started_seq
started_at
content
content_chars
```

内容必须复用当前 `apply_streaming_delta()` 的上限，不能让 index 因流式正文无限增长。

在 index 增量应用时更新 snapshot；重建 index 时也能从 timeline event 重新生成。

### 5. 调整 `from_connection()` 初始化顺序

当前流程是：

```text
AcpRuntime::start
  -> from_connection
      -> load_all_branch_events
      -> hydrate all Blob
      -> build runtime state
  -> initialize
  -> setup_session
      -> try_reuse_attached_session
```

应改为：

```text
AcpRuntime::start
  -> from_connection
      -> TimelineStore::open
      -> read lightweight runtime snapshot
      -> initialize runtime with snapshot/default state
  -> initialize
  -> setup_session
      -> try_reuse_attached_session first
      -> if reused: never load full history
      -> if Resume: use snapshot, no history replay
      -> if Load: load only replay inputs
      -> if New: no historical body load
```

注意：`try_reuse_attached_session()` 不能在没有 runtime 对象的情况下直接调用，但必须在任何完整历史正文加载之前执行。

### 6. 按恢复方式决定是否读取历史

#### Attached session reuse

只需要：

- session ID；
- config fingerprint；
- provider freshness；
- lifecycle/runtime snapshot；
- pending interaction 和 hot state。

必须满足：

```text
Blob hydrated bytes == 0
full timeline items loaded == false
```

#### Resume

provider 不回放历史时：

- 不读取历史正文；
- 不 hydrate Blob；
- 只用 index snapshot 恢复本地 runtime；
- 设置 `AwaitingTurnStart`。

#### Load

只有 `SessionRestoreMethod::Load` 且确实启用 external history sync 时，才读取 replay inputs。

需要给 `ProviderHistoryReplay` 增加类似：

```rust
ProviderHistoryReplay::from_prompt_anchors(...)
```

只传入 Gold Band prompt anchor，不要把全部 timeline event 传入。

#### New session

新建 session 不需要读取历史正文。

### 7. 修复 `repair_attempt_usage()` 的隐藏全量读取

这是非常重要的一步。

`repair_attempt_usage()` 已按以下方式收敛，不再调用 `load_timeline_items()` 或 hydrate Timeline Blob：

```text
优先读取 usage journal；
journal 缺少 prompt start 时，通过 timeline index 的 prompt locator 恢复；
只有存在未完成 turn 时才扫描 raw usage transaction；
不再调用 load_timeline_items()。
```

可以新增：

```rust
load_prompt_starts_from_timeline_index(path)
```

它只读取：

- prompt event ID；
- seq；
- timestamp；
- provider/model 必要字段。

不要读取完整 raw，也不要 hydrate Blob。

恢复期间不能因为任意历史 prompt 缺少 usage completion 就扫描完整 raw log。只有 durable metadata 明确表明当前 turn 仍处于 `starting/accepted/running/cancelRequested`，或当前 retry 仍在 processing 时，才允许为 crash recovery 扫描 raw；terminal session 的历史缺口保留给显式修复，不能阻塞 attached reuse、Resume 或普通 follow-up。

### 7.1 Attached reuse 必须先于 usage repair

`from_connection()` 只从 snapshot 初始化轻量 prior metrics，不执行 repair。`setup_session()` 先调用 `try_reuse_attached_session()`：

- 命中时从 `AttachedSessionRuntime` 继承 live `AcpUsageState`，不得读取 journal、Timeline prompt index 或 raw；
- attached entry 已取得但因配置/freshness 需要 reload 时，仍保留该 live usage；
- 只有没有可用 attached usage 且进入 resume/load/new 分支时，才执行一次 durable repair；
- 同一 setup 从 restore 失败回退 new 时不得重复 repair。

### 7.2 Permission response 的控制面先于 Timeline 投影

permission response file 是 live ACP waiter 的可靠控制信号，Timeline 是可重建的展示投影。用户响应时必须先原子写入 response file，再依据已存在的 timeline identity 尝试 settle pending item。permission pending file 已写入、但对应 timeline event/index 尚未持久化时，不得因为无法定位 item 而拒绝响应；运行时随后创建或读取 permission item 时必须按 response signal 收敛其状态。

### 8. 正常路径允许的全量扫描边界

以下情况可以保留 O(N)：

- index 不存在；
- index 版本不兼容；
- prefix fingerprint 不匹配；
- compaction；
- 明确的数据修复命令。

但必须：

- 记录 diagnostic；
- 与正常 index-hit 路径区分；
- 不在每次 continue/follow-up 中触发；
- 测试验证正常路径只进行 bounded tail replay。

不要为了掩盖 index 损坏而每次静默 fallback 到全量 timeline。

## 四、生命周期测试

在 `src/acp/events.rs` 增加接口级测试。

至少覆盖：

1. established session stop：

```text
request stop
=> availability established
=> activity cancelRequested
=> latest status none

terminal settle
=> availability established
=> activity idle
=> latest status cancelled
=> session_metadata_status == cancelled
```

2. restorable session stop：

```text
terminal settle 后 availability 仍为 restorable
```

3. orphaned `CancelRequested`：

```text
进程重启后收敛为 idle + cancelled
availability 不为 closing
```

4. 重复 stop：

```text
第一次返回 accepted
第二次返回同一 operation
terminal 后再次 stop 不推进错误状态
```

5. stale owner：

```text
旧 turn 的 terminal 写入不能覆盖新 turn
```

6. late provider metadata：

```text
terminal 后迟到 running/accepted metadata 不得恢复 activity
不得恢复 latestTurnStatus=none
不得恢复 availability=closing
```

7. legacy terminal closing：

```text
availability=closing
activity=idle
latestTurnStatus=cancelled
```

读取投影必须返回 `cancelled`，并在迁移路径中规范化落盘。

8. completed/failed regression：

确保 completed、failed 的原有 availability 和 projection 不受影响。

## 五、timeline 性能与正确性测试

### 1. Blob 不读取测试

构造：

- 1000～10000 个历史事件；
- 多个 agent branch；
- 若干大于 `TIMELINE_BLOB_MIN_BYTES` 的 Blob；
- active attached session。

断言：

```text
runtime restore 不触发 Blob read
attached reuse 不调用 load_timeline_items
hydrated bytes == 0
```

可以使用不存在的 Blob reference 作为测试数据。如果错误调用了 hydrate，测试会直接失败。

### 2. Resume/Load 分流测试

分别验证：

- attached reuse：0 个历史正文读取；
- Resume：0 个历史正文读取；
- Load：只读取 prompt anchors/replay 所需数据；
- New session：0 个历史正文读取。

### 3. restore parity 测试

用小 timeline fixture，同时执行：

```text
旧的完整 replay oracle
新的 index snapshot restore
```

比较：

- seq；
- timing；
- pending permission；
- pending elicitation；
- active stream；
- active compaction；
- pending retry；
- provider replay suppression；
- hot timeline items。

结果必须一致。

### 4. usage recovery 测试

验证：

- usage journal 完整时不读取 timeline 正文；
- journal 缺少 prompt start 时只读 prompt locator；
- 存在未完成 turn 时才读取 raw transaction；
- timeline 中存在 Blob reference 时不会被无条件 hydrate。
- terminal metadata 下存在旧的缺失 usage completion 时，不读取 raw log；active durable turn 才允许 raw recovery。
- permission pending file 已存在而 timeline identity 尚未落盘时，响应文件仍会写入并唤醒 live waiter。

### 5. 性能验收目标

不要只测“感觉变快”，至少记录：

```text
runtime_restore_ms
index_hit / tail_replay / full_rebuild
processed_tail_records
projection_locator_scans
locator_reads
full_timeline_items_loaded
hydrated_blob_count
hydrated_blob_bytes
```

目标：

- attached reuse 和 Resume 的 Blob 读取为 0；
- 正常路径不调用 full timeline replay；
- 历史 locator 数量扩大 10 倍、active tail 不变时，index-hit 的 `projection_locator_scans == 0` 且 locator body read 数保持不变；
- full rebuild 只发生在 index 缺失、损坏或 compaction 路径。
- 当前单体 JSON index 的读取和反序列化仍是 O(locator 数量)；本轮验收消除的是 restore 的第二次全量遍历、按 branch 临时分配/排序和历史 identity clone，不为此引入 SQLite、sidecar 双写或第二 canonical state。

## 六、文档同步

代码修改时必须同步维护设计文档和开发计划。

至少更新：

- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/产品设计文档/runtime/control.md`
- `docs/gold-band/开发计划/新UI/ACP Timeline增量投影与停止控制面解耦技术方案.md`
- `docs/gold-band/开发计划/acp接入/ACP会话长连接与历史同步技术方案.md`
- `docs/gold-band/开发计划/生命周期整理/ACP停止语义与Adapter长连接开发方案.md`

文档需要明确：

1. stop 是 turn cancel，不是 session close；
2. `availability` 不再承担 turn stopping 语义；
3. terminal 的 canonical 不变量；
4. attached/resume/load/new 四种恢复模式；
5. runtime 使用 index snapshot，正文和 Blob 按需读取；
6. index 损坏或 compaction 才允许全量 rebuild；
7. 性能指标和测试结果。

尤其要修正现有文档中“停止终态和重进不再依赖完整 timeline”的表述，使它与实际实现和测试一致。

## 本轮 UI 投影回归修复

停止控制面先于 Timeline/session 正文异步收敛。页面收到 `stopping` 或当前 turn 的 terminal lifecycle 后，必须把旧 session snapshot 中的 pending permission/elicitation 视为过期投影并立即隐藏；不能要求用户切出再进入会话才能看到一致状态。composer 的 Stop、输入锁定和交互卡片必须消费同一 lifecycle 投影。

已接受但尚未写入 canonical Timeline 的 optimistic 用户消息仍保持 `sending`，只有 canonical prompt 到达后才从 optimistic 列表移除；`processing` 只显示在 composer 状态区。停止清理同时移除 `sending` 与 `processing` 的未落盘 optimistic prompt，确保停止后 follow-up 不被残留消息锁住。

回归验收覆盖：停止后 pending interaction 卡片立即消失、terminal lifecycle 后 Stop 消失且 composer 可继续发送，以及 accepted prompt 在 canonical event 到达前始终显示“发送中”。

## 七、推荐实施顺序

建议实现模型按两个逻辑提交完成，每个提交都同步文档和测试。

### Commit A：Lifecycle canonical reducer

1. 增加 transition/reducer；
2. 迁移 stop、terminal、orphan recovery、provider metadata writer；
3. 修复 availability 归一化；
4. 更新 view model projection；
5. 增加生命周期回归测试；
6. 同步设计文档。

### Commit B：Runtime lazy restore

1. 增加 timeline runtime snapshot；
2. 增加 timing/stream snapshot；
3. 调整 `from_connection()` 初始化顺序；
4. 按 Resume/Load/Reuse 分流；
5. 修复 `repair_attempt_usage()` 的全量读取；
6. 增加 Blob 读取和 parity 测试；
7. 做 release 性能验收；
8. 同步 timeline/runtime 设计文档。

## 八、明确禁止的实现方式

实现模型不要采用以下方案：

- 只在 `persist_session_turn_terminal_owned()` 里补一句 `availability=established`；
- 保留 `request_session_stop()` 写 `closing`，再靠 view model 特判掩盖；
- 新增第二套 `RuntimeLifecycleState` 作为旁路事实源；
- 为这两个问题引入 SQLite、消息队列或新的状态机框架；
- 把 provider cancel 网络调用放进 metadata lock；
- 把完整 timeline 延迟到 `try_reuse_attached_session()` 之后，但又让 `repair_attempt_usage()` 全量读取；
- 通过无界缓存保存所有历史正文；
- 把 `AcpTimingPatch` 直接当成完整 `AcpTimingState`；
- 为旧字段增加长期双写和多层 fallback；
- 顺便修改 agent launch identity 或分页算法，扩大本次变更范围。

本次的验收核心只有两句：

```text
停止完成后，所有消费者都看到同一个 canonical terminal lifecycle。
继续/恢复会话时，正常路径只恢复 index snapshot 和必要 hot state，不读取全部历史 Blob。
```

## 九、2026-08-20 实施收口

- usage：attached runtime registry 已携带 live usage；reuse 决策前不再执行 durable repair，reuse miss 后 repair 至多一次。
- Timeline：index format 升级为 V7；runtime hot projection 在统一索引写入路径维护，index-hit restore 不再遍历、分组或排序全部 locator。
- TimelineStore：删除 open 时由 locator 再复制的全量 fingerprint HashMap 和只写不读的 canonical event body mirror；upsert/compaction 直接使用 materialized index，避免在 JSON index 反序列化之外再制造一次 O(N) 分配。
- provider replay suppression：不再在 `AcpRuntime` 启动时复制全部历史 identity，改由 root/branch `TimelineStore` 查询索引 membership；已有 branch store 在启动时保留并复用于后续写入与查询。
- context compaction：只读取最新候选，已有 `contextUsedAfter` 或非 running/completed 的候选不进入 hot state，不再恢复全部历史 completed compaction。
- 回归接口：attached usage ready 时 repair 回调不得被调用；100 与 1000 条历史 locator、固定 active tail 下，index-hit 均为 0 次 projection locator scan、1 次 locator body read；迟到旧 patch 不得复活已被用户消息关闭的 stream。
- release 验证：上述 100/1000（10×）历史规模回归在 optimized test profile 通过，测试执行耗时 0.61 秒；验收依据以固定扫描/读取计数为主，不用单次墙钟时间替代复杂度证明。
- 过度设计复核：没有增加数据库、队列、sidecar 或第二生命周期状态机；新增 projection 只物化现有 reducer 已经表达的运行态不变量。性能代价是每次正常 timeline upsert 的常数级集合/槽位更新；旧索引迁移和真实乱序 patch 允许一次 O(N log N) projection rebuild。

## 十、2026-08-20 三项页面与 finalize 回归收口

### 1. 停止后同页发送

- 删除提交路径对 stale `AcpSessionVm.status=running` 的旁路准入判断；发送按钮与 Enter 统一调用 `deriveAcpRuntimeComposerState()` 结果中的 `canSubmit/submitTarget/stopInProgress`。
- terminal lifecycle 对同 turn 的 stale session status 具有投影优先级。回归测试保持组件 mounted，先输入 running session，再送达 cancelled lifecycle，断言按钮和 Enter 均调用普通 `submitConversationPrompt`，无需切换会话。

### 2. finalize/repair 前 busy

- 根因是 `runtimeControl` 曾用独立 cursor lock 整文件覆盖共享 metadata，可能把 provider 已 claim 的 `accepted/revision=N` 回退成旧 `starting/revision=N-1`，导致业务 `end_turn` 的 terminal CAS no-op，随后 finalize 被正确 busy guard 拒绝。
- `patch_session_runtime_control()` 现在在 `session_metadata_lock` 内只修改两个 control 字段。外层 cursor lock 继续保证 control command CAS，但不再承担文件事务职责；锁内没有 provider/RPC await。
- 接口回归固定：业务 turn claim 后提交 workflow-continued patch，owner/revision/activity 保持；业务 terminal 成功后紧接着 admission finalize 不返回 `acp.prompt-session-busy`。

### 3. 旧交互卡片复活

- terminal lifecycle 的 revision 合并、当前 session 的 pending permission/elicitation 清理和 cache 更新由同一个 `applyLifecycleProjection()` 入口提交；subscription、submit、queue、stop、continue 的 lifecycle 结果不再分别调用局部 setter。
- 所有 session 投影入口再次经过已合并 lifecycle settlement，防止同一 live event 后半段的旧 session body 把卡片写回。
- 为跨下一轮抵御父级 stale prop，空 pending projection 只接受更高 `eventPage.generation/coveredRevision/newestRevision/newestSeq` 的新 pending。水位来自现有 Timeline，不新增 dismissed ID、持久字段或第二状态机；request ID 复用时新 sequence 仍能显示真实新卡片。
- mounted 组件测试分别覆盖 permission 与 elicitation：第一轮卡片可见、terminal 后消失、下一轮 active 时旧卡不复活、同一 request ID/新 sequence 的真实 interaction 正常显示。

### 4. 性能与过度设计复核

- 后端 control patch 只读写单个小 metadata JSON，锁范围为同步 read-modify-write；没有新增全量 Timeline/raw 扫描或锁外等待。
- 前端 lifecycle 事件只检查两个小 pending 数组和四个常数级水位字段；不扫描历史消息，不扩大 Context，不新增缓存、队列、持久 identity 或依赖。
- 本轮复用 canonical lifecycle revision、Timeline event-page 水位、现有 metadata lock 和现有 session cache，复杂度与真实竞态相匹配，不属于过度设计。

## 十一、2026-08-20 Attempt ACP 存储版本收口

- `node.json` 中的 ACP storage schema 字段成为 attempt durable storage 的唯一 canonical 版本：普通 attempt 使用本级 `node.json.acp_storage_schema_version`；AI-DYNAMIC leaf 的固定 `attempt-001` 使用父级 `dynamic/nodes/<leaf>/node.json.acpStorageSchemaVersion`。新 attempt 创建时直接写当前版本 `2`，不再对空历史运行 migration，也不在 dynamic attempt 内新建 manifest。
- 旧 `node.json` 缺字段按 `0` 处理，迁移按 branch Timeline `0 → 1`、Agent result `1 → 2` 顺序幂等执行。每一步成功后才在同一 attempt 的 `node.json` 原子推进版本，失败不越级；future version 使用稳定错误码拒绝。
- `.acp-branch-timeline-migration-v1` 与 `.acp-agent-result-migration-v2` 已从生产代码删除。已有 marker 不读取、不删除，新 attempt 不再生成；它们不再伪装成“迁移发生过”的 durable 事实。
- 普通 `NodeState` 与 AI-DYNAMIC leaf `DynamicNodeState` 的生产写入口统一保留磁盘上更高的 ACP schema version，避免旧内存状态与迁移并发时发生版本回退。动态 `graph.json.nodes` 不序列化该字段，防止 graph 聚合与 leaf 文件形成双真源；锁只覆盖单个小 JSON 的同步 read-modify-write，不包含 Timeline 扫描或 provider 调用。
- `acp.timeline.index.json` 保持现有单体、可重建索引方案。本轮不引入分片、SQLite、sidecar 或 command catalog 引用；继续接受索引随唯一历史 item 线性增长，以已经消除的 Blob hydrate、全 locator 扫描/排序和 attached reuse 前 raw repair 为主要性能收益。
- 接口回归固定：普通 attempt 与 AI-DYNAMIC leaf 在当前版本下即使 Timeline 损坏也必须零迁移快速返回；旧 marker 完全不影响迁移判断；旧 attempt 成功后只推进对应 canonical `node.json` 到 `2` 且第二次不扫描；失败步骤不推进；future version 在扫描前拒绝；迟到 lifecycle 写回不能降低版本；dynamic `graph.json` 不复制该字段。

性能与过度设计复核：新 attempt 只增加 `node.json` 中一个整数，无新增日常扫描和额外文件；旧 attempt 最多承担一次既有迁移。统一 node 写入增加一次小型 `node.json` 读取，写入频率只在 lifecycle 转换边界，数据大小有界，不处于 Timeline/流式事件热路径。现有单体 index 足以覆盖当前主要性能目标，暂不为剩余 O(N) JSON 解析引入高复杂度存储层。
