# ACP 会话实时耗时采集方案

## 0. 背景

当前 ACP 会话底部 compact 用量栏展示两类时间：

- `当前用时`：前端基于当前步骤的 `startAt` 用本地 timer 每秒递增。
- `会话累计`：前端直接读取 `AcpSessionVm.sessionElapsedSeconds`。

这导致运行中普通流式消息持续到达时，`当前用时` 会实时变化，但 `会话累计` 只在完整 session snapshot 刷新时变化，例如权限请求、会话结束、停止或显式 refresh。普通 `textDelta` / `thoughtDelta` / `toolCallUpdate` live event 只更新 timeline item，不会携带新的会话累计时间。

## 1. 问题判断

这不是单纯的 UI 显示 bug，而是数据采集边界不完整：

1. **当前好设计**：ACP live event 已经是运行中 UI 的热路径；完整 session snapshot 只用于初始化、hydrate、终态和配置类刷新，避免每个 token 都重拉完整会话。
2. **实现缺口**：运行中耗时属于会话级实时指标，但当前没有进入 live event 热路径，只在 snapshot 扫描时重建。
3. **应避免的补丁**：前端根据已加载 timeline window 临时重算累计。该方案会依赖当前分页窗口完整性，刷新/切换时可能先显示旧 snapshot 值，再跳到本地推导值。

因此应在后端 ACP runtime 聚合层维护耗时状态，并把轻量 timing patch 附着到现有 live event 中。

## 2. 目标

1. `会话累计` 在 ACP 运行中随 live event 实时校准和递增。
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
    permission_wait_accumulated_seconds: u64,
    pending_permission_ids: HashSet<String>,
    paused: bool,
}
```

字段语义：

| 字段 | 说明 |
|---|---|
| `session_elapsed_seconds` | 已结算的会话净处理耗时，不含当前 active turn 未结算增量 |
| `active_turn_started_at` | 当前 prompt turn 开始时间 |
| `active_turn_last_activity_at` | 当前 turn 最近一次有效处理事件时间 |
| `permission_wait_started_at` | 当前权限等待开始时间 |
| `permission_wait_accumulated_seconds` | 当前 turn 内已累计扣除的权限等待时长 |
| `pending_permission_ids` | 当前未完成权限请求集合 |
| `paused` | 是否处于不应递增的等待态 |

### 4.2 live event timing patch

每个 ACP live event 可携带轻量 timing patch。推荐作为 `AcpUiEventVm` 顶层可选字段，而不是塞进不透明 `raw`：

```ts
interface AcpTimingPatchVm {
  sessionElapsedSeconds: number;
  activeTurnStartedAt?: string | null;
  activeTurnLastActivityAt?: string | null;
  permissionWaitStartedAt?: string | null;
  paused: boolean;
  reason?: "active" | "permission-wait" | "metadata" | "terminal";
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
  activeTurnStartedAt?: string | null;
  activeTurnLastActivityAt?: string | null;
  permissionWaitStartedAt?: string | null;
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

### 5.4 长时间无输出

如果 agent 正在思考但暂时没有任何 live event，有两种可选策略：

- **推荐默认**：前端根据 `timing.activeTurnStartedAt` 和 `paused=false` 本地平滑递增；下一次 live event 到达时用后端 timing patch 校准。
- **可选增强**：后端低频发送 `timing_update` heartbeat，例如 2s 或 5s 一次，仅包含 timing patch，不携带完整 session。

默认不引入 heartbeat，除非后续确认长时间无输出时用户明显需要秒级精度。

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
2. 收到 live event 时，如果 `event.timing` 存在，则更新 `displayTiming`。
3. 展示值：

```ts
if (!timing) return sessionElapsedSeconds ?? null;
if (timing.paused || !timing.activeTurnStartedAt) {
  return timing.sessionElapsedSeconds;
}
return timing.sessionElapsedSeconds + secondsSince(timing.activeTurnStartedAt) - currentOpenPermissionWaitSeconds;
```

实际实现中，后端 patch 应尽量提供已经扣除权限等待后的可递增 anchor，前端不自行解释权限集合。

UI 规则：

- 运行中：用 `timing` 平滑显示。
- 权限等待：冻结在最后后端 patch 值。
- 终态：显示 snapshot 最终值，不再本地递增。
- 若没有 `timing`：回退现有 `sessionElapsedSeconds`。

## 8. 持久化与恢复

### 8.1 snapshot

`acp.snapshot.json` 必须持久化：

- `timing.sessionElapsedSeconds`
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
| `src/acp/events.rs` | `AcpUiEvent` 增加 `timing` 字段，序列化兼容缺省 |
| `src-tauri/src/view_models.rs` | `AcpUiEventVm`、`AcpSessionVm` 增加 timing VM；legacy scan 填充 timing |
| `web/src/types.ts` | 增加 `AcpTimingPatchVm` / `AcpSessionTimingVm` |
| `web/src/components/acp/ACPChatDialog.tsx` | 从 session/live event timing 派生 compact 栏的会话累计 |
| `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md` | 同步实时会话累计口径 |

## 10. 测试计划

### 10.1 Rust 单元测试

- prompt -> text -> terminal：累计正常增长。
- prompt -> metadata update -> next prompt：metadata 间隔不计入。
- prompt -> permission pending -> selected -> text：权限等待不计入。
- 多个并发 pending permission：只在 pending 集合从空到非空、再从非空到空时计算等待。
- prompt 失败 / cancelled / interrupted：按最后有效处理事件结算。
- snapshot roundtrip：active timing 字段能序列化/反序列化。

### 10.2 前端单元测试

- session snapshot timing 初始化展示值。
- live event timing patch 到达后更新会话累计。
- `paused=true` 时不本地递增。
- terminal session 使用最终 snapshot，不继续增长。
- 缺少 timing 时回退 `sessionElapsedSeconds`。

### 10.3 集成验证

- 正常长输出：当前用时与会话累计都实时变化。
- 长时间思考无输出：前端基于 active anchor 平滑递增。
- 权限请求停留 30s：当前用时/会话累计不把等待时间计入净处理耗时。
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

- 运行中普通流式输出时，`会话累计`至少随 live event 校准，并在前端平滑递增。
- 刷新后会话累计从 snapshot timing 恢复，不出现明显旧值跳变。
- 权限等待和 metadata update 不计入净处理耗时。
- 不新增每秒完整 session snapshot 推送。
- 旧会话仍可打开，缺少 timing 字段时不报错。

## 13. 非目标

- 不改变 token usage 统计口径。
- 不把 SQLite 作为实时会话 timing 主存储。
- 不要求 provider/ACP adapter 原生支持 timing；Gold Band runtime 自行聚合。
- 不在 UI 中展示实现解释文案，仅显示对用户有用的时间指标。
