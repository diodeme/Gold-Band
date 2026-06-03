# ACP 消息 Token 消耗展示

## 0. 当前实现状态

- **数据链路**：ACP 适配器通过 `usage_update` 事件发送 token 消耗数据（含 `used`、`size`、`cost` 等字段），Gold Band 后端已将最近一条 `usage_update` 的原始 JSON 存入 `AcpSessionVm.usage`（类型为 `unknown`）。
- **前端消费**：`AcpSessionVm.usage` 字段**未被任何前端组件读取或展示**。
- **事件流**：`usageUpdate` 事件被标记为 hidden，不进入聊天时间线。
- **结构化缺失**：Rust 端未定义 usage 结构体，前端也未定义 TypeScript interface，三端均以原始 JSON 透传。
- **结论**：token 消耗数据已存在于数据管道中，但完全未解析、未展示。

---

## 1. 核心方向

**变更点**：在 ACP 会话 UI 中新增两处 token 消耗展示：

1. **消息级 token 消耗**：每次 AI 输出消息（textDelta 文本回复）时，在该消息气泡下方展示该次输出消耗的 token 数。
2. **会话级实时 token 消耗**：在会话底部 composer 区域上方或侧边实时动态展示当前会话的累计 token 消耗（input / output / total / 缓存命中 / context window 使用率 / 费用），随 `usage_update` 事件动态刷新。

**设计意图**：
- 消息级 token 消耗帮助用户感知每次 AI 回复的"成本"，辅助判断是否需要精简 prompt 或切换模型。
- 会话级实时 token 消耗让用户掌握整个会话的资源使用全貌，避免上下文窗口溢出或费用超预期。
- 两级展示（消息粒度 + 会话汇总）形成完整的 token 可观测性，补全 audit trail。

---

## 2. 需求规格

### 2.1 消息级 Token 消耗

#### 2.1.1 展示位置

在 Agent 文本消息（`textDelta`）气泡的**右下角**，消息正文下方，展示该次输出的 token 消耗：

```text
┌──────────────────────────────────────────┐
│  Agent 文本回复内容（Markdown 渲染）       │
│                                          │
│                          输出 1,234 token │
└──────────────────────────────────────────┘
```

#### 2.1.2 展示内容

| 字段 | 说明 | 数据来源 |
|---|---|---|
| 输出 token 数 | 该条消息消耗的 output tokens | `usage_update` 事件的 `used` 字段差值（本次更新 - 上次更新） |
| 输入 token 数（可选） | 该轮对话的 input tokens | 同上，按差值计算 |

**约束**：
- 仅 Agent 文本消息（`textDelta`）展示 token 消耗，用户消息、tool call、thought、plan 不展示。
- 若在同一轮回复中多次触发 `usage_update`（流式过程中多次更新），仅在**最终合并后的 textDelta 消息**展示**最终累计值**，不在中间 delta 展示。
- token 消耗数值格式化为千分位（如 `1,234 token`），使用 `text-[11px] text-muted-foreground/50` 弱化展示。
- 若该消息无对应 token 数据（如 `usage_update` 缺失或未关联到该消息），不展示 token 消耗，不显示 `0 token` 或空白占位。

#### 2.1.3 数据关联方案

> **方案 A（推荐）**：后端按 `seq` 区间关联。在扫描 `acp.events.jsonl` 时，将每条 `usage_update` 的 `used` 值与最近的 `textDelta` 消息关联。每条消息的 token 消耗 = 该消息完成时的 `used` - 前一条消息完成时的 `used`。

### 2.2 会话级实时 Token 消耗

#### 2.2.1 展示位置

在 ACP 会话的 **composer 上方** 或 **会话底部状态栏** 新增一个可折叠的 token 消耗面板：

```text
┌─────────────────────────────────────────────────────────┐
│  📊 Token 用量          输入 8,500  │  输出 2,340        │
│                         缓存 1,200  │  总计 12,040       │
│  ████████████░░░░░░░░  12,040 / 200,000 (6.0%)          │
│                         费用 $0.12                       │
└─────────────────────────────────────────────────────────┘
```

#### 2.2.2 展示内容

| 字段 | 说明 | 数据来源 |
|---|---|---|
| 输入 token | 累计 input tokens（不含缓存） | `AcpUsageVm.inputTokens`（会话结束汇总） |
| 输出 token | 累计 output tokens | `AcpUsageVm.outputTokens`（会话结束汇总） |
| 缓存读取 token | 缓存读命中 token 数 | `AcpUsageVm.cachedReadTokens`（会话结束汇总） |
| 缓存写入 token | 缓存写入 token 数 | `AcpUsageVm.cachedWriteTokens`（会话结束汇总） |
| 总计 token | 四项求和 | `AcpUsageVm.totalTokens`（会话结束汇总） |
| 上下文窗口使用率 | `used / size` 百分比 + 进度条 | `AcpUsageVm.used` 和 `AcpUsageVm.size`（实时 `usage_update`） |
| 费用 | 累计 USD 费用 | `AcpUsageVm.costAmountUsd`（`usage_update` result 事件） |

#### 2.2.3 交互行为

- **默认折叠**：初始状态下面板折叠，仅显示一行概要（如 `📊 12,040 / 200,000 token · $0.12`）。
- **点击展开**：展开后显示完整明细（输入/输出/缓存/进度条/费用）。
- **实时更新**：会话运行中（`status === "running"`），面板随 `usage_update` 事件实时刷新数值，进度条平滑过渡。
- **会话结束后**：面板展示最终累计值，不再更新；进度条变灰或移除动画。
- **compaction 后**：`used` 重置为 0 时，面板数值归零重新累计，进度条重置。

#### 2.2.4 上下文窗口告警

- 当 `used / size >= 80%` 时，进度条颜色变为**警告色**（如 amber/orange）。
- 当 `used / size >= 95%` 时，进度条颜色变为**危险色**（如 red），并在面板中提示"接近上下文窗口上限，建议压缩或开启新会话"。
- 告警文案仅展示在展开面板中，不弹出 toast。

#### 2.2.5 深色主题适配

- 面板背景：`bg-secondary/30`，不使用浅黑色方块或嵌套卡片。
- 进度条使用 `bg-primary/40`（正常）/ `bg-amber-500/60`（警告）/ `bg-red-500/60`（危险）。
- 数值颜色：`text-muted-foreground`，重点数值使用 `text-foreground`。
- 遵循 CLAUDE.md 深色主题约束：通过留白、层级、少量边界和重点状态表达，不堆叠面板。

---

## 4. ACP 适配器 usage_update 源码分析

> 源码位置：`D:\IdeaProjects\claude-agent-acp-main\src\acp-agent.ts`

### 4.1 适配器内部分析

#### 4.1.1 内部类型定义

适配器内部定义了以下 usage 相关类型（`acp-agent.ts:117-136`）：

```typescript
// 内部累计用量（camelCase，SDK 风格）
type AccumulatedUsage = {
  inputTokens: number;        // 累计输入 token
  outputTokens: number;       // 累计输出 token
  cachedReadTokens: number;   // 累计缓存读命中 token
  cachedWriteTokens: number;  // 累计缓存写入 token
};

// 快照用量（snake_case，Anthropic API 风格）
type UsageSnapshot = {
  input_tokens: number;
  output_tokens: number;
  cache_read_input_tokens: number;
  cache_creation_input_tokens: number;
};

const ZERO_USAGE = Object.freeze({
  input_tokens: 0,
  output_tokens: 0,
  cache_read_input_tokens: 0,
  cache_creation_input_tokens: 0,
});

const DEFAULT_CONTEXT_WINDOW = 200000;
```

#### 4.1.2 会话结束时返回的累计汇总

`sessionUsage()` 函数（`acp-agent.ts:2318-2330`）在会话状态变为 `idle` 或 `end_turn` 时返回：

```typescript
function sessionUsage(session: Session) {
  return {
    inputTokens: session.accumulatedUsage.inputTokens,
    outputTokens: session.accumulatedUsage.outputTokens,
    cachedReadTokens: session.accumulatedUsage.cachedReadTokens,
    cachedWriteTokens: session.accumulatedUsage.cachedWriteTokens,
    totalTokens:
      session.accumulatedUsage.inputTokens +
      session.accumulatedUsage.outputTokens +
      session.accumulatedUsage.cachedReadTokens +
      session.accumulatedUsage.cachedWriteTokens,
  };
}
```

> **注意**：此汇总对象目前仅在 `AcpPromptRun` 结构体中返回（`src/acp/client.rs:44-53` 的 `final_text` / `final_outputs` 同级），**并未透传**到 `AcpSessionVm`。当前 `AcpSessionVm` 无此字段，需新增。

#### 4.1.3 `totalTokens` 计算逻辑

```typescript
// acp-agent.ts:2336-2343
function totalTokens(usage: UsageSnapshot): number {
  return (
    usage.input_tokens +
    usage.output_tokens +
    usage.cache_read_input_tokens +
    usage.cache_creation_input_tokens
  );
}
```

**关键语义**：按 Anthropic API 规范，`input_tokens` 不包含缓存 token（`cache_read_input_tokens` 和 `cache_creation_input_tokens` 独立上报），因此四项直接求和不会重复计数。`totalTokens` 等于四项之和，即"本轮推理实际涉及的上下文总 token 数"。

### 4.2 `usage_update` 事件的 3 种发送场景

ACP 适配器在 **3 种场景** 下发 `usage_update` 事件，每次发送的字段组合不同：

#### 场景 1：流式中间更新（`acp-agent.ts:1128-1135`）

在 streaming 过程中，`message_delta` 事件触发累计 token 数变化时发送：

```json
{
  "sessionUpdate": "usage_update",
  "used": 12345,
  "size": 200000
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `used` | `number` | 当前累计已用 token 总数（= `input_tokens + output_tokens + cache_read_input_tokens + cache_creation_input_tokens`，通过 `totalTokens()` 计算） |
| `size` | `number` | 上下文窗口总容量（来自 `session.contextWindowSize`，初始值 200000，从 `result` 的 `modelUsage` 动态校准） |

#### 场景 2：上下文压缩后（`acp-agent.ts:873-879`）

compaction 完成后，`used` 归零：

```json
{
  "sessionUpdate": "usage_update",
  "used": 0,
  "size": 200000
}
```

**语义**：compaction 后上下文窗口重置，`used` 归零重新累计。

#### 场景 3：API 调用结束后带费用（`acp-agent.ts:988-1004`）

每次 SDK `result` 消息到达后发送，包含费用信息：

```json
{
  "sessionUpdate": "usage_update",
  "used": 45678,
  "size": 200000,
  "cost": {
    "amount": 0.1234,
    "currency": "USD"
  },
  "_meta": {
    "_claude/origin": { "kind": "user-prompt" }
  }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `used` | `number` | 同上，累计已用 token 总数 |
| `size` | `number` | 上下文窗口总容量 |
| `cost.amount` | `number` | 累计费用（USD），来自 SDK `message.total_cost_usd` |
| `cost.currency` | `string` | 币种，固定为 `"USD"` |
| `_meta` | `object?` | 可选，仅当 `message.origin` 存在时附带，包含 `_claude/origin` |

### 4.3 字段映射总表

综合分析，从 ACP 适配器到 Gold Band 需要解析的完整字段映射如下：

| Gold Band 字段 | 类型 | ACP 来源 | 来源路径 | 说明 |
|---|---|---|---|---|
| `used` | `u64` | `usage_update` | `raw.used` | 当前累计已用 token 总数 |
| `size` | `u64` | `usage_update` | `raw.size` | 上下文窗口总容量 |
| `costAmountUsd` | `f64` | `usage_update`（场景 3） | `raw.cost.amount` | 累计费用（USD），仅 result 事件携带 |
| `costCurrency` | `String` | `usage_update`（场景 3） | `raw.cost.currency` | 币种，固定 `"USD"` |
| `inputTokens` | `u64` | 会话结束汇总 | `AcpPromptRun.sessionUsage().inputTokens` | 累计输入 token（不含缓存） |
| `outputTokens` | `u64` | 会话结束汇总 | `AcpPromptRun.sessionUsage().outputTokens` | 累计输出 token |
| `cachedReadTokens` | `u64` | 会话结束汇总 | `AcpPromptRun.sessionUsage().cachedReadTokens` | 累计缓存读命中 token |
| `cachedWriteTokens` | `u64` | 会话结束汇总 | `AcpPromptRun.sessionUsage().cachedWriteTokens` | 累计缓存写入 token |
| `totalTokens` | `u64` | 会话结束汇总 | `AcpPromptRun.sessionUsage().totalTokens` | 四项求和 |

### 4.4 Gold Band 当前 Gap

| 问题 | 现状 | 需要改动 |
|---|---|---|
| `usage_update` 事件字段未解析 | `normalize_session_update()` 仅提取通用字段（content/title/toolCallId/status），`used`/`size`/`cost` 残留在 `raw` 中 | 新增 `extract_usage()` 函数，从 `usage_update` 的 `raw` 中提取结构化字段 |
| 事件扫描仅取最后一个 raw | `scan_acp_events()` 第 2113-2114 行直接 `usage = Some(compact_raw_value(raw.clone()))`，不做解析 | 改为解析 `raw` → `AcpUsageVm`，同时维护 `last_used` 差值分配给消息 |
| 会话结束汇总未透传 | `AcpPromptRun` 中有 sessionUsage，但未传入 `AcpSessionVm` | 在构建 `AcpSessionVm` 时补充 `inputTokens`/`outputTokens`/`cachedReadTokens`/`cachedWriteTokens`/`totalTokens` |
| 前端类型为 `unknown` | `AcpSessionVm.usage` 类型为 `unknown \| null` | 改为 `AcpUsageVm \| null` |

> **结论**：本期必须补齐 Rust 后端结构化解析，将原有"透传原始 JSON"改为"解析为 `AcpUsageVm` 结构体"。前端不能直接从 `raw` 裸解析，应在 ViewModel 层完成字段提取，确保前后端类型一致。三端补齐顺序：**Rust 结构体 → ViewModel 扫描解析 → TypeScript interface → UI 组件消费**。

---

## 5. 后端改动（本期必做）

### 5.1 Rust 结构体定义

#### 5.1.1 核心 Usage 结构体

在 `src-tauri/src/view_models.rs` 中新增 `AcpUsageVm`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AcpUsageVm {
    /// 当前累计已用 token 总数（= inputTokens + outputTokens + cachedReadTokens + cachedWriteTokens）
    /// 来源：usage_update.used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<u64>,
    /// 上下文窗口总容量
    /// 来源：usage_update.size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// 累计费用（USD）
    /// 来源：usage_update.cost.amount
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_amount_usd: Option<f64>,
    /// 累计输入 token（不含缓存）
    /// 来源：会话结束汇总 inputTokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// 累计输出 token
    /// 来源：会话结束汇总 outputTokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// 累计缓存读命中 token
    /// 来源：会话结束汇总 cachedReadTokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u64>,
    /// 累计缓存写入 token
    /// 来源：会话结束汇总 cachedWriteTokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_write_tokens: Option<u64>,
    /// 总 token（= inputTokens + outputTokens + cachedReadTokens + cachedWriteTokens）
    /// 来源：会话结束汇总 totalTokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}
```

#### 5.1.2 更新 `AcpSessionVm`

在 `AcpSessionVm` 中将 `usage` 字段类型从 `Option<serde_json::Value>` 改为 `Option<AcpUsageVm>`：

```rust
// 改前
pub usage: Option<serde_json::Value>,

// 改后
pub usage: Option<AcpUsageVm>,
```

### 5.2 usage_update 事件解析函数

在 `src/acp/events.rs` 中新增 `extract_usage()` 辅助函数，从 `usage_update` 的 raw JSON 中提取结构化字段：

```rust
/// 从 usage_update 事件的 raw JSON 中提取 usage 字段
/// 输入：compact_raw_value 处理后的 raw
/// 输出：(used, size, cost_amount_usd)
fn extract_usage_fields(raw: &Value) -> (Option<u64>, Option<u64>, Option<f64>) {
    let used = raw.get("used").and_then(Value::as_u64);
    let size = raw.get("size").and_then(Value::as_u64);
    let cost_amount = raw
        .get("cost")
        .and_then(|cost| cost.get("amount"))
        .and_then(Value::as_f64);
    (used, size, cost_amount)
}
```

### 5.3 事件扫描解析

修改 `scan_acp_events()` 函数（`view_models.rs:2059-2209`），将原有裸 JSON 赋值：

```rust
// 改前（第 2113-2114 行）
} else if is_session_update(&event, "usage_update") {
    usage = Some(compact_raw_value(raw.clone()));
}

// 改后
} else if is_session_update(&event, "usage_update") {
    let (used, size, cost_amount) = extract_usage_fields(raw);
    usage = Some(AcpUsageVm {
        used,
        size,
        cost_amount_usd: cost_amount,
        ..Default::default()
    });
}
```

### 5.4 会话结束汇总注入

当前 `AcpPromptRun` 中的 `sessionUsage()` 返回值（`inputTokens`、`outputTokens`、`cachedReadTokens`、`cachedWriteTokens`、`totalTokens`）未透传到 `AcpSessionVm`。需要在构建 `AcpSessionVm` 的流程中（`commands.rs` 的 `get_acp_session` → `build_acp_session_vm` 或等价路径）：

1. 读取 `AcpPromptRun` 的 usage 字段
2. 将 `inputTokens` / `outputTokens` / `cachedReadTokens` / `cachedWriteTokens` / `totalTokens` 合并到 `AcpUsageVm` 中
3. 若会话仍在运行中（无 `AcpPromptRun`），这些字段保持 `None`

### 5.5 消息级 Token 差值分配

在 `scan_acp_events()` 中维护 `last_used: Option<u64>` 计数器。对于每条非隐藏的 `textDelta` 事件（合并后），计算 `delta = used - last_used`，将结果写入事件的扩展字段，供前端消费。

方案：在 `AcpUiEventVm` 的 `raw` 中插入 `_goldBand` 命名空间：

```json
{
  "_goldBand": {
    "tokens": 1234
  }
}
```

前端读取 `event.raw?._goldBand?.tokens` 作为消息级 token 消耗值。

### 5.6 涉及文件（后端）

| 文件 | 改动 |
|---|---|
| `src/acp/events.rs` | **新增** `extract_usage_fields()` 函数 |
| `src/acp/client.rs` | **修改** `AcpPromptRun` 增加 usage 字段（如尚无）；**透传** sessionUsage 到 ViewModel 层 |
| `src-tauri/src/view_models.rs` | **新增** `AcpUsageVm` 结构体；**修改** `AcpSessionVm.usage` 类型；**修改** `scan_acp_events()` 解析 usage；**修改** `AcpEventScan` 的 usage 类型；**新增** `last_used` 计数器实现消息级差值分配 |
| `src-tauri/src/commands.rs` | **修改** 会话构建流程，将 `AcpPromptRun` 的 sessionUsage 合并到 `AcpUsageVm` |

---

## 6. 前端改动（与原第 3 节合并，保留已确认的前端方案）

### 6.1 TypeScript 类型定义

在 `web/src/types.ts` 中新增 `AcpUsageVm` 接口：

```typescript
interface AcpUsageVm {
  /** 当前累计已用 token 总数 */
  used?: number | null;
  /** 上下文窗口总容量 */
  size?: number | null;
  /** 累计费用（USD） */
  costAmountUsd?: number | null;
  /** 累计输入 token（不含缓存） */
  inputTokens?: number | null;
  /** 累计输出 token */
  outputTokens?: number | null;
  /** 累计缓存读命中 token */
  cachedReadTokens?: number | null;
  /** 累计缓存写入 token */
  cachedWriteTokens?: number | null;
  /** 总 token（四项求和） */
  totalTokens?: number | null;
}
```

同时将 `AcpSessionVm.usage` 类型从 `unknown | null` 改为 `AcpUsageVm | null`。

### 6.2 消息级 Token 展示组件

新建 `web/src/components/acp/AcpMessageTokenBadge.tsx`：

```tsx
interface AcpMessageTokenBadgeProps {
  /** 该条消息消耗的 output tokens 估算值 */
  tokens?: number;
}

function AcpMessageTokenBadge({ tokens }: AcpMessageTokenBadgeProps) {
  if (tokens == null || tokens <= 0) return null;
  return (
    <span className="text-[11px] text-muted-foreground/50 select-none">
      输出 {tokens.toLocaleString()} token
    </span>
  );
}
```

数据来源：读取事件 `raw._goldBand.tokens` 字段（由后端 `scan_acp_events()` 的差值分配写入）。

在 `MessageBubble` 组件中，agent 文本消息气泡右下角引入此 badge。

### 6.3 会话级 Token 面板组件

新建 `web/src/components/acp/AcpUsagePanel.tsx`：

```tsx
interface AcpUsagePanelProps {
  usage: AcpUsageVm | null;
  isRunning: boolean;
}

function AcpUsagePanel({ usage, isRunning }: AcpUsagePanelProps) {
  // 默认折叠，显示概要行（used / size token · $cost）
  // 点击展开显示完整明细：
  //   - 输入 token / 输出 token
  //   - 缓存读取 / 缓存写入
  //   - 总计
  //   - 上下文窗口进度条（used / size %）
  //   - 费用
  // isRunning 控制实时更新动画
}
```

在 `ACPChatDialog` 的 composer 上方引入此面板。

> 面板布局对齐 prompt-kit + shadcn/ui 风格，优先使用 `Collapsible` / `Card` / `Progress` 等 copy-in 组件，不自研容器。

### 6.4 涉及文件（前端）

| 文件 | 改动 |
|---|---|
| `web/src/types.ts` | **新增** `AcpUsageVm` 接口；修改 `AcpSessionVm.usage` 类型 |
| `web/src/components/acp/AcpMessageTokenBadge.tsx` | **新建**：消息级 token badge 组件 |
| `web/src/components/acp/AcpUsagePanel.tsx` | **新建**：会话级 token 面板组件 |
| `web/src/components/acp/ACPMessageList.tsx` 或 `MessageBubble` | agent 文本消息引入 `AcpMessageTokenBadge` |
| `web/src/components/acp/ACPChatDialog.tsx` | composer 上方引入 `AcpUsagePanel` |
| `web/src/i18n.ts` | **新增** token 相关 i18n key（中/英） |

---

## 7. 设计约束

- **不改变数据管道**：不新增 ACP 事件类型，不修改 ACP 适配器行为，仅消费已有的 `usage_update` 事件。
- **不侵入消息流**：token 消耗信息不占据消息流主体位置，使用小字号、弱化颜色展示。
- **不阻塞流式体验**：token 消耗数值更新为纯计算/渲染，不阻塞 UI 主线程。
- **深色主题**：token 展示组件不引入浅黑色方块、嵌套卡片或强边框。通过留白和弱化文字表达层级。
- **国际化**：token 数值标签（"输入"、"输出"、"token"、"费用"等）需同时提供中英文 i18n key。
- **降级处理**：当 `usage` 为 `null` 或缺失时，不展示任何 token 信息，不影响现有 UI 布局。
- **不新增依赖**：复用 shadcn/ui `Collapsible`、`Progress` 等已有组件，不自研。

---

## 8. 本期约束

- **仅展示 Claude ACP 适配器的 usage 数据**，不扩展至其他 provider（Codex、Gemini 等）。其他 provider 的 usage 数据格式可能不同，留待后续统一。
- **消息级 token 消耗为估算值**（基于 `usage_update` 的 `used` 差值），非 API 精确 token 数。标注为"约"或使用 tooltip 注明。
- **费用仅展示 USD**，不换算其他货币。
- **不在消息流中插入独立的 token 事件卡片**，token 消耗不破坏现有时间线结构。
- **不改变 ChildAgentGroup（子 Agent）的 token 展示逻辑**，子 Agent 的 token 消耗本期不展示。

---

## 9. 验收标准

### 9.1 消息级 Token 消耗

- [ ] Agent 文本消息（`textDelta`/`MessageBubble`）右下角展示 token 消耗 badge。
- [ ] Token 数值使用千分位格式化（如 `1,234`）。
- [ ] 用户消息、tool call、thought、plan 不展示 token badge。
- [ ] 合并后的 textDelta 消息仅展示最终累计值，不在中间 delta 展示。
- [ ] 无 usage 数据时，不展示 badge，不显示占位符。
- [ ] 深色主题下 badge 文字可读但不干扰主内容。

### 9.2 会话级 Token 消耗

- [ ] Composer 上方显示可折叠的 token 消耗面板。
- [ ] 默认折叠，显示一行概要（`used / size token` + 费用）。
- [ ] 展开后显示完整明细：输入、输出、缓存读取、缓存写入、总计、进度条、费用。
- [ ] 进度条正确反映上下文窗口使用率（`used / size` %）。
- [ ] 使用率 ≥ 80% 时进度条变为警告色。
- [ ] 使用率 ≥ 95% 时进度条变为危险色，并显示告警文案。
- [ ] 会话运行中面板数值随 `usage_update` 实时刷新。
- [ ] 会话结束后面板展示最终值，不再更新动画。
- [ ] Compaction 后数值正确归零重置。
- [ ] 深色主题下面板不出现浅黑色方块、嵌套卡片、强边框。
- [ ] 无 usage 数据时面板不展示。

### 9.3 国际化

- [ ] 所有 token 相关标签（输入、输出、缓存、总计、费用、token、上下文窗口等）支持中英文切换。
- [ ] 数值格式化（千分位）在中文和英文环境下均正确。

### 9.4 边界情况

- [ ] `usage` 为 `null` 时不报错、不展示任何 token 信息。
- [ ] `cost` 字段缺失时，仅不展示费用行，不影响其他字段。
- [ ] `size` 字段为 0 或缺失时，进度条不展示，不报除零错误。
- [ ] 会话恢复（restored）后，token 数据从 `acp.session.json` 恢复，面板展示历史累计值。

---

## 10. 参考

- ACP 适配器源码：`D:\IdeaProjects\claude-agent-acp-main\src\acp-agent.ts`
  - 内部类型：`AccumulatedUsage`（L117-122）、`UsageSnapshot`（L124-129）、`ZERO_USAGE`（L131-136）
  - `sessionUsage()` 会话结束汇总：L2318-2330
  - `totalTokens()` 计算：L2336-2343
  - 流式中间 `usage_update`：L1128-1135
  - Compaction 后 `usage_update`：L873-879
  - Result 带费用 `usage_update`：L988-1004
- Gold Band ACP 事件处理：`src/acp/events.rs`（`normalize_session_update`、`kind_to_ui_kind`）
- Gold Band ViewModel 扫描：`src-tauri/src/view_models.rs`（`scan_acp_events` L2059-2209、`AcpEventScan.usage` L2045、`AcpSessionVm.usage` L402）
- Gold Band ACP client：`src/acp/client.rs`（`AcpPromptRun` L44-53、`handle_session_update`）
- Gold Band ACP UI 设计：`docs/gold-band/开发计划/acp接入/acp-ui.md`
- Gold Band 前端类型：`web/src/types.ts`（`AcpSessionVm` L429-449）
- 同类需求参考：`docs/gold-band/开发计划/新增流程/ACP消息头像与时间展示.md`
