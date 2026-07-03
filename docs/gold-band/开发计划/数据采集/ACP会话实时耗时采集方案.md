# ACP 会话实时耗时采集方案

## 0. 背景

当前 ACP 会话底部 compact 用量栏曾展示两类时间：

- 旧 `当前用时`：前端基于当前步骤的 `startAt` 用本地 timer 每秒递增，现已从产品展示中移除。
- `会话累计`：前端直接读取 `AcpSessionVm.sessionElapsedSeconds`。

这导致运行中普通流式消息持续到达时，旧步骤耗时会实时变化，但 `会话累计` 只在完整 session snapshot 刷新时变化，例如权限请求、会话结束、停止或显式 refresh。普通 `textDelta` / `thoughtDelta` / `toolCallUpdate` live event 只更新 timeline item，不会携带新的会话累计时间。

## 1. 问题判断

这不是单纯的 UI 显示 bug，而是数据采集边界不完整：

1. **当前好设计**：ACP live event 已经是运行中 UI 的热路径；完整 session snapshot 只用于初始化、hydrate、终态和配置类刷新，避免每个 token 都重拉完整会话。
2. **实现缺口**：运行中耗时属于会话级实时指标，但当前没有进入 live event 热路径，只在 snapshot 扫描时重建。
3. **应避免的补丁**：前端根据已加载 timeline window 临时重算累计。该方案会依赖当前分页窗口完整性，刷新/切换时可能先显示旧 snapshot 值，再跳到本地推导值。

因此应在后端 ACP runtime 聚合层维护耗时状态，并把轻量 timing patch 附着到现有 live event 中。

## 2. 目标

1. `会话累计` 在 ACP 运行中随 live event 实时校准和递增，并作为唯一对用户展示的耗时指标。
2. 刷新或重新进入会话时不出现明显旧值跳变。
3. 权限等待、用户空闲、metadata update 不计入净处理耗时。
4. 不引入每秒完整 session snapshot 推送。
5. 保持文件事实源优先，snapshot 可恢复，timeline 可回放校验。

## 3. 设计原则

- **先定数据，再定接口，再补实现**：计时状态是 ACP runtime 的会话级聚合数据，不属于单个 UI 组件私有状态。
- **实时热路径轻量化**：普通 live event 携带小型 timing patch；完整 session snapshot 不参与高频刷新。
- **后端裁决口径**：哪些事件推进耗时、哪些事件暂停耗时，由后端统一判断，前端只做展示和本地平滑。
- **snapshot 可恢复**：刷新后前端应从 snapshot 拿到累计基线和 active anchor，而不是从当前加载的部分 events 推导。
- **legacy 可读**：旧会话 snapshot 缺少 timing 字段时，仍可按现有 timeline/events 回放重建。

## 4. 数据结构

### 4.1 后端内存态

在 ACP runtime 聚合层维护 `AcpTimingState`：

```rust
struct AcpTimingState {
    session_elapsed_seconds: u64,
    active_turn_started_at: Option<String>,
    active_turn_last_activity_at: Option<String>,
    permission_wait_started_at: Option<String>,
    user_wait_started_at: Option<String>,
    user_wait_accumulated_seconds: u64,
    pending_permission_ids: HashSet<String>,
    pending_elicitation_ids: HashSet<String>,
    wait_reason: Option<String>,
    paused: bool,
}
```

字段语义：

| 字段 | 说明 |
|---|---|
| `session_elapsed_seconds` | 已结算的会话净处理耗时，不含当前 active turn 未结算增量 |
| `active_turn_started_at` | 当前 prompt turn 开始时间 |
| `active_turn_last_activity_at` | 当前 turn 最近一次有效处理事件时间 |
| `permission_wait_started_at` | 当前权限等待开始时间；保留给旧字段消费 |
| `user_wait_started_at` | 当前用户交互等待开始时间，覆盖 permission / elicitation |
| `user_wait_accumulated_seconds` | 当前 turn 内已累计扣除的用户交互等待时长 |
| `pending_permission_ids` | 当前未完成权限请求集合 |
| `pending_elicitation_ids` | 当前未完成 elicitation 请求集合 |
| `wait_reason` | 当前等待原因，典型值为 `permission` / `elicitation` |
| `paused` | 是否处于不应递增的等待态 |

### 4.2 live event timing patch

每个 ACP live event 可携带轻量 timing patch。推荐作为 `AcpUiEventVm` 顶层可选字段，而不是塞进不透明 `raw`：

```ts
interface AcpTimingPatchVm {
  sessionElapsedSeconds: number;
  revision?: number | null;
  observedAt?: string | null;
  activeTurnStartedAt?: string | null;
  activeTurnLastActivityAt?: string | null;
  permissionWaitStartedAt?: string | null;
  userWaitStartedAt?: string | null;
  waitReason?: string | null;
  paused: boolean;
  reason?: "active" | "permission-wait" | "elicitation-wait" | "metadata" | "tick" | "terminal";
}

interface AcpUiEventVm {
  // existing fields...
  timing?: AcpTimingPatchVm | null;
}
```

如果为了兼容短期改动，也可以先放在 `event.raw._goldBand.timing`，但最终应提升为结构化字段，避免前端继续解析 raw。

### 4.3 session snapshot timing

`acp.snapshot.json` 与 `AcpSessionVm` 增加同一组结构化字段：

```ts
interface AcpSessionTimingVm {
  sessionElapsedSeconds: number;
  revision?: number | null;
  observedAt?: string | null;
  activeTurnStartedAt?: string | null;
  activeTurnLastActivityAt?: string | null;
  permissionWaitStartedAt?: string | null;
  userWaitStartedAt?: string | null;
  waitReason?: string | null;
  paused: boolean;
}

interface AcpSessionVm {
  sessionElapsedSeconds?: number | null; // 保留兼容
  timing?: AcpSessionTimingVm | null;
}
```

`sessionElapsedSeconds` 继续保留，作为旧前端兼容字段；新前端优先读取 `timing`。

## 5. 计时规则

### 5.1 turn 生命周期

1. Gold Band synthetic/user prompt 写入 timeline 时，开启新的 active turn。
2. 开启新 turn 前，先结算上一 turn。
3. assistant 文本、thought、tool call、tool update、plan、usage update 等处理相关事件可以推进 `activeTurnLastActivityAt`。
4. turn 结束、失败、取消或 session terminal 时，结算当前 turn 到最后有效处理事件或当前时间。

### 5.2 不计时事件

以下事件不推进 `activeTurnLastActivityAt`：

- `available_commands_update`
- `current_mode_update`
- `session_info_update`
- 其他纯配置、标题、能力同步事件

这些事件仍保留在 timeline/raw 中，用于 UI 元数据或诊断，但不代表 agent 仍在处理用户请求。

### 5.3 权限等待

1. `permissionRequest(status=pending)` 到达时，若 pending 集合从空变非空，记录 `permissionWaitStartedAt`。
2. 权限请求 selected/cancelled/resolved 后，从 pending 集合移除。
3. pending 集合清空时，将 `now - permissionWaitStartedAt` 累加进 `permissionWaitAccumulatedSeconds`，并清空 `permissionWaitStartedAt`。
4. 权限等待期间 live timing patch 的 `paused=true`，前端不继续本地递增会话累计。
5. timeline 压缩后可能只保留一条 selected/cancelled permission event；若事件携带 `startedAt` / `endedAt` 或 `timestamp`，后端重放必须把 `startedAt -> endedAt(timestamp)` 作为已闭合用户等待区间扣除，不能要求 pending 事件仍在当前扫描窗口中。
6. runtime 内存中的 `AcpTimingState` 只是 timeline timing 的缓存；写入 permission / elicitation 边界事件或发送用户等待期间 live-only tick 前，必须从当前 timeline item 集合重建缓存，避免 pending 被 upsert replacement、阻塞等待或恢复扫描时形成第二套计时事实。

### 5.4 长时间无输出

如果 agent 正在思考但暂时没有任何普通 live event，后端发送低频 live-only `timingUpdate` tick，默认 1s 一次，仅包含 timing patch，不携带完整 session，不写入 `acp.timeline.jsonl`。tick 只对活跃会话生效，且 `sessionElapsedSeconds` / `paused` / 等待字段未变化时不重复推送；权限等待、elicitation 等 `paused=true` 状态只在进入等待、退出等待或数值变化时更新。tick 调度不能只挂在 `recv_timeout` 空闲分支上；即使 adapter 持续推送 text/thought/tool/usage 等 inbound event，prompt loop 每轮也必须按 due 检查并发送 live-only timing，避免 UI 在高频输出期间停在旧秒数后一次性跳变。

## 6. 数据流

```text
ACP raw session/update
  -> normalize_session_update
  -> AcpTimingState.observe(event)
  -> AcpUiEvent.timing = timing_state.patch(now)
  -> append timeline patch
  -> emit live event
  -> frontend merge event + timing patch
  -> composer compact bar realtime display

ACP prompt loop idle tick
  -> AcpTimingState.patch(now, "tick")
  -> emit live-only timingUpdate
  -> frontend update session timing only
  -> timeline remains unchanged
```

完整 snapshot 路径：

```text
session/new or load
  -> write snapshot(timing)
  -> emit session snapshot

prompt terminal / stop / failure
  -> finalize timing
  -> write snapshot(timing)
  -> emit session snapshot
```

## 7. 前端消费方案

前端维护一个 `displayTiming`：

1. 初始值来自 `AcpSessionVm.timing`。
2. 收到 live-only `timingUpdate` 时，如果 `event.timing` 存在，也必须通过与 session payload 相同的 revision reducer 更新 `displayTiming`，不能绕过版本裁决直接覆盖。`timingUpdate` 属于 compact 栏实时指标通道，必须同步更新，不能被 text/thought/tool 等消息流的 `startTransition`、interaction quiet window 或批量 flush 拖延。
3. 普通 timeline event 上的 `timing` 只作为该事件发生时的历史锚点，用于审计和恢复扫描，不作为当前会话累计显示的实时来源，避免切换会话、权限响应或分页重放时用较旧事件 timing 覆盖 session snapshot。
4. 同一 ACP session 的 session payload 可能由 stop response、subscription session、final `getAcpSession` 等异步入口乱序到达；后端 timing 必须携带 `revision` / `observedAt`，前端 session reducer 优先按 `revision` 接受或拒绝 timing，旧 payload 可以更新 status/events/metadata，但不能用旧 timing 覆盖较新的会话累计。缺少 revision 的旧历史数据才退回秒数单调保护。
5. 外部传入的 session prop、identity session 初始化、subscription session、permission response、stop response 与 live-only `timingUpdate` 都必须进入同一 timing reducer；任何入口不得直接 `setCurrentSession(session)` 覆盖已接受的 live timing。
6. metadata hydration 只用于补齐早期缺失的 system prompt / model / mode 等会话元数据，不属于计时热路径；同一 session 的 hydration request 必须有冷却时间或 attempt guard，不能被每个 live event 重复触发。
7. 展示值：

```ts
if (!timing) return sessionElapsedSeconds ?? null;
return timing.sessionElapsedSeconds;
```

后端 patch 提供已经扣除用户交互等待后的净耗时。前端不自行解释权限 / elicitation 集合，也不基于本地时钟递增会话累计。前端不再展示当前步骤耗时，避免两个时间指标使用不同生命周期或让用户误解。

`getAcpSession` 是前端切换会话、权限响应和刷新恢复时的权威 session 入口。对仍处于 active/stopping 的 session，`AcpSessionVm.timing` 不能让 `acp.snapshot.json.timing` 优先于扫描得到的当前净累计秒数；snapshot 可能只代表上一次 flush 时刻，会把 UI 短暂带回旧值。active session 应使用 `AcpSessionElapsedState.finish(active=true)` 得到的 `sessionElapsedSeconds` 覆盖历史 event timing 锚点，并保留 event timing 中的等待字段；terminal session 则继续以 snapshot timing 作为持久化事实，但 terminal snapshot 写入时必须用 prompt 结束 / cancel 当前时刻结算当前 turn。若当前 turn 期间只有 live-only `timingUpdate`、没有普通 timeline event，终态 snapshot 也不能回退到上一条 timeline event 的锚点。

UI 规则：

- 运行中：后端 tick 推动 `timing.sessionElapsedSeconds` 实时显示。
- 权限等待 / elicitation 等用户等待：冻结在最后后端 patch 值。
- 终态：显示按 prompt 结束 / cancel 时刻结算后的 snapshot 最终值，不再本地递增。
- 若没有 `timing`：回退现有 `sessionElapsedSeconds`。

## 8. 持久化与恢复

### 8.1 snapshot

`acp.snapshot.json` 必须持久化：

- `timing.sessionElapsedSeconds`
- `timing.revision`
- `timing.observedAt`
- `timing.activeTurnStartedAt`
- `timing.activeTurnLastActivityAt`
- `timing.permissionWaitStartedAt`
- `timing.paused`

这样刷新后前端无需等待 timeline hydrate 即可恢复稳定显示。

### 8.2 timeline

timeline item 可保留 `timing` patch，用于：

- 调试校验
- legacy snapshot 缺失时回放重建
- 对齐用户截图中的具体时刻

但 timeline 不作为运行中实时展示的唯一来源。

### 8.3 legacy fallback

旧会话缺少 snapshot timing 时：

1. 后端读取详情时继续用现有 `AcpSessionElapsedState` 从 timeline/events 重建最终 `sessionElapsedSeconds`。
2. 对 terminal 历史会话，返回 `timing.paused=true`。
3. 对仍 active 的旧会话，优先用 worker-ref/session status 判断是否需要补写 snapshot timing。

## 9. 接口变更范围

| 文件/模块 | 变更 |
|---|---|
| `src/acp/client.rs` | 增加 `AcpTimingState`，在 persist/emit live event 前生成 timing patch |
| `src/acp/events.rs` | `AcpUiEvent` 增加 `timing` 字段，序列化兼容缺省；`AcpTimingState` 提供 terminal snapshot 结算 |
| `src-tauri/src/view_models.rs` | `AcpUiEventVm`、`AcpSessionVm` 增加 timing VM；legacy scan 填充 timing；active session VM 用扫描累计值覆盖 stale snapshot timing |
| `web/src/types.ts` | 增加 `AcpTimingPatchVm` / `AcpSessionTimingVm` |
| `web/src/components/acp/ACPChatDialog.tsx` | 从 session snapshot / live-only `timingUpdate` timing 派生 compact 栏的会话累计，不再从普通历史事件或本地时钟补算 |
| `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md` | 同步实时会话累计口径 |

## 10. 测试计划

### 10.1 Rust 单元测试

- prompt -> text -> terminal：累计正常增长。
- prompt -> metadata update -> next prompt：metadata 间隔不计入。
- prompt -> permission pending -> selected -> text：权限等待不计入。
- prompt -> compacted permission selected(startedAt/timestamp) -> text：即使 pending 事件不在扫描窗口中，权限等待也不计入。
- prompt -> elicitation pending -> response -> text：elicitation 等待不计入。
- 多个并发 pending permission：只在 pending 集合从空到非空、再从非空到空时计算等待。
- prompt 失败 / cancelled / interrupted：按最后有效处理事件结算。
- snapshot roundtrip：active timing 字段能序列化/反序列化。
- terminal snapshot：继续后没有普通输出、只有 live-only tick 时，停止/取消仍按当前 turn 结束时刻结算。
- terminal snapshot：停止/取消发生在权限或 elicitation 等用户等待期间时，等待时间仍不计入。
- timing revision：普通 timeline event 使用事件 seq，live-only tick / terminal snapshot 使用 runtime synthetic revision，且所有 session payload 与 live tick 共享同一版本语义。
- active session VM：stale snapshot timing 小于扫描累计值时，`AcpSessionVm.timing` 返回扫描累计值。
- terminal session VM：snapshot timing 仍作为最终持久化事实。

### 10.2 前端单元测试

- session snapshot timing 初始化展示值。
- 普通 live event timing patch 到达后不覆盖当前会话累计。
- live-only `timingUpdate` 到达后只更新 timing，不进入 timeline。
- live-only `timingUpdate` 低于当前 timing revision 时，不覆盖 session payload 已接受的计时。
- live-only `timingUpdate` 不进入消息流 transition 批处理；即使 text/thought/tool 更新被延迟，compact 栏会话累计也必须先更新。
- 外部 session prop / identity session 不能用旧 timing 覆盖当前已接受的 live timing。
- 普通历史事件 timing 小于当前 snapshot/timingUpdate 时，不产生会话累计短暂回退。
- 同一 session 的 stale terminal payload 在 final snapshot 之后乱序到达时，因 `timing.revision` 更旧而不能覆盖当前 timing；没有 revision 的旧数据才使用秒数单调保护。
- `paused=true` 时会话累计不本地递增。
- terminal session 使用最终 snapshot，不继续增长。
- 缺少 timing 时回退 `sessionElapsedSeconds`。

调试定位：前端支持在 DevTools 中设置 `localStorage.setItem("goldBand.debug.acpTiming", "1")` 打开 ACP timing 来源日志。日志前缀为 `[GoldBand][ACP timing]`，会标记 `prop-session`、`identity-session`、`authoritative-session`、`live-timing-update` 等来源以及对应秒数；旧 payload 的 timing 被 reducer 拒绝时，日志来源使用 `*:rejected-timing`，表示该秒数只是被拒绝的原始输入，不代表 UI 展示发生回退。同一开关也会输出 `[GoldBand][ACP metadata]`，用于定位实时期间 system prompt、Gold Band user prompt、model/mode 配置是否已进入当前 session。若只想看 metadata，也可以设置 `localStorage.setItem("goldBand.debug.acpMetadata", "1")`。关闭时删除对应 localStorage key。

### 10.3 集成验证

- 正常长输出：会话累计实时变化。
- 长时间思考无输出：后端 `timingUpdate` tick 推动会话累计实时变化。
- 权限请求或 elicitation 停留 30s：会话累计不把等待时间计入净处理耗时。
- 刷新页面：会话累计不先显示旧值再跳变。
- 历史会话：终态累计稳定显示，不受 `createdAt -> updatedAt` 生命周期跨度影响。

## 11. 实施步骤

1. 在 Rust ACP event model 中增加可选 timing 字段，保持旧 JSON 兼容。
2. 在 ACP runtime 内新增 `AcpTimingState`，先覆盖 worker ACP 主路径。
3. 在 live event emit 前附加 timing patch。
4. 在 `write_session` / snapshot 写入时持久化 timing。
5. 在 `AcpSessionVm` 和前端类型中透传 timing。
6. 前端 compact 用量栏改为消费 timing；保留 `sessionElapsedSeconds` fallback。
7. 补齐 Rust 和前端测试。
8. 更新产品设计文档中的会话累计说明。

## 12. 验收标准

- 运行中普通流式输出时，`会话累计`由后端 live-only `timingUpdate` 校准并在前端平滑递增。
- 刷新后会话累计从 snapshot timing 恢复，不出现明显旧值跳变。
- 切换会话或权限响应后，不得因普通历史事件 timing 重放短暂回退到旧值。
- 重新进入已停止/已完成会话时，不得先显示停止瞬间的旧事件耗时，再跳到最终 snapshot 耗时。
- 权限等待和 metadata update 不计入净处理耗时。
- 不新增每秒完整 session snapshot 推送。
- 旧会话仍可打开，缺少 timing 字段时不报错。

## 13. 非目标

- 不改变 token usage 统计口径。
- 不把 SQLite 作为实时会话 timing 主存储。
- 不要求 provider/ACP adapter 原生支持 timing；Gold Band runtime 自行聚合。
- 不在 UI 中展示实现解释文案，仅显示对用户有用的时间指标。
