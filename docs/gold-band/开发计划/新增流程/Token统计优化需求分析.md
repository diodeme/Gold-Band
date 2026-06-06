# Token 统计优化需求分析

> 版本：v2.0 | 日期：2026-06-05 | 状态：已实现
>
> 基于 Deep Interview（歧义度 16%，4 轮问答）后的最终规格。原 v1.0 中的 turn 展示、accumulatedUsed、费用展示、进度条已从需求中移除。

## 一、背景与现状

当前项目已完成 Token 消耗展示的 MVP 实现（详见 `ACP消息Token消耗展示.md`），数据链路为：

```
ACP 适配器 → usage_update 事件 → Rust 后端解析 → AcpUsageVm → 前端组件展示
```

### 现有组件

| 组件 | 位置 | 功能 |
|------|------|------|
| `AcpUsagePanel` | `web/src/components/acp/AcpUsagePanel.tsx` | 会话级可折叠 Token 面板，位于 Composer 上方 |
| `AcpMessageTokenBadge` | `web/src/components/acp/AcpMessageTokenBadge.tsx` | 消息级 Token badge，位于文本气泡右下角 |
| `inject_token_delta` | `src-tauri/src/view_models.rs:2693` | 后端为 textDelta 消息注入 `_goldBand.tokens` 差值 |

### 现有数据结构

```typescript
// AcpUsageVm（前后端共用）
interface AcpUsageVm {
  used?: number | null;          // 当前上下文窗口占用（compaction 后重置）
  accumulatedUsed?: number | null; // 跨 compaction 累计（后端维护）
  size?: number | null;          // 上下文窗口总容量
  costAmountUsd?: number | null;
  inputTokens?: number | null;   // 仅 PromptRun 结束后有值
  outputTokens?: number | null;
  cachedReadTokens?: number | null;
  cachedWriteTokens?: number | null;
  totalTokens?: number | null;
}
```

---

## 二、需求清单

### 需求 1：移除消息级 Token 统计

**现状**：每条 Agent 文本消息（`textDelta`）气泡右下方展示 `AcpMessageTokenBadge`，显示该消息消耗的 token 数（如 `1,234 token`）。

**问题**：消息级 token 差值为估算值（基于 `used` 字段差值），准确度有限，且视觉上增加了消息流的信息密度，用户实际关注度低。

**变更**：

| 改动点 | 说明 |
|--------|------|
| `ACPChatDialog.tsx:1119` | 移除 `<AcpMessageTokenBadge>` 组件渲染 |
| `view_models.rs:2693-2718` | 移除 `inject_token_delta()` 函数及其调用点（L2216, L2232, L2248） |
| `view_models.rs:2148` | 移除 `last_message_used` 变量 |
| `AcpMessageTokenBadge.tsx` | 整个文件可删除（若无其他引用） |
| `i18n.ts` | 移除 `acp.messageTokens` key |

**不影响**：后端 `last_used` 和 `accumulated_used_tokens`（client.rs 内部字段）已随 `inject_token_delta` 一起移除。

---

### 需求 2（已实现）：会话级面板重构

**变更**：`AcpUsagePanel` 从折叠式改为两行固定布局：

```
上下文窗口  12K / 200K
Token 用量   输入 8.5K  输出 2.3K  缓存 1.2K  总计 12K
```

**关键决策（经 Deep Interview 确认）**：
- ~~turn（轮次）展示~~：**不实现**。经讨论后决定不展示 turn
- ~~accumulatedUsed（会话累计）~~：**移除字段**。从 AcpUsageVm 数据模型中移除，后端不再计算
- ~~费用（costAmountUsd）~~：**不展示**。字段保留在数据模型中但不展示
- ~~进度条~~：**不展示**
- **智能单位格式化**：统一 `formatTokenCount`（<1K 原数字、≥1K 用 K、≥1M 用 M）

---

### 需求 4：ACP 会话切换时的 Token 累加

#### 问题分析

一个 Gold Band **会话窗口**（`AcpConversationVm`）可能包含多个 **ACP 会话**（attempt）。每个 attempt 是一次独立的 `AcpRuntime` 生命周期，拥有独立的：

- `used_tokens`（上下文窗口占用，compaction 后可能重置）
- `accumulated_used_tokens`（跨 compaction 累计）
- `input_tokens` / `output_tokens` 等（PromptRun 结束后填充）

当前前端通过 `selectedConversation.activeAttemptId` 选中最新的 attempt，仅展示该 attempt 的 `usage`。当 ACP 会话切换（retry / new attempt）时，前一个 attempt 的 token 数据被丢弃。

#### ACP 会话何时结束

根据源码分析（`src/acp/client.rs`）：

| 场景 | 触发条件 | 标志 |
|------|----------|------|
| 正常结束 | `prompt()` 返回 `stopReason` | attempt status 变为 `completed` |
| 用户取消 | `cancel()` 被调用 | attempt status 变为 `cancelled` |
| 异常退出 | 适配器进程崩溃 / 超时 | attempt status 变为 `error` |

**判断逻辑**：在 `AcpConversationVm.attempts` 数组中，除最后一个 attempt 外，其余 attempt 的 `status` 均为终态（`completed` / `cancelled` / `error`）。只有最后一个 attempt 可能处于 `running` 状态。

#### 累加方案

**前端累加**（推荐方案，无需后端改动）：

在 `AcpUsagePanel` 或其父组件中，遍历 `conversation.attempts`，对所有终态 attempt 的 `usage` 进行累加，再加上当前活跃 attempt 的实时 `usage`：

```typescript
function accumulateUsage(conversation: AcpConversationVm): AcpUsageVm {
  const accumulated: AcpUsageVm = {};
  for (const attempt of conversation.attempts) {
    const usage = attempt.acpSession?.usage;
    if (!usage) continue;
    accumulated.inputTokens = (accumulated.inputTokens ?? 0) + (usage.inputTokens ?? 0);
    accumulated.outputTokens = (accumulated.outputTokens ?? 0) + (usage.outputTokens ?? 0);
    accumulated.cachedReadTokens = (accumulated.cachedReadTokens ?? 0) + (usage.cachedReadTokens ?? 0);
    accumulated.cachedWriteTokens = (accumulated.cachedWriteTokens ?? 0) + (usage.cachedWriteTokens ?? 0);
    accumulated.totalTokens = (accumulated.totalTokens ?? 0) + (usage.totalTokens ?? 0);
    accumulated.costAmountUsd = (accumulated.costAmountUsd ?? 0) + (usage.costAmountUsd ?? 0);
  }
  // 上下文窗口 used/size 取最新 attempt 的实时值（非累加）
  const latest = conversation.attempts.at(-1)?.acpSession?.usage;
  if (latest) {
    accumulated.used = latest.used;
    accumulated.size = latest.size;
  }
  return accumulated;
}
```

**关键规则**：
- **累加项**：`inputTokens`、`outputTokens`、`cachedReadTokens`、`cachedWriteTokens`、`totalTokens`、`costAmountUsd` — 这些是消耗量，跨 attempt 需求和
- **不累加项**：`used`、`size` — 这些是上下文窗口状态量，取最新 attempt 的实时值
- **turn_count**：同样跨 attempt 累加

**组件接口变更**：

`AcpUsagePanel` 需接收完整的 `AcpConversationVm`（或预计算的累加结果），而非单个 `AcpUsageVm`：

```typescript
interface AcpUsagePanelProps {
  usage: AcpUsageVm | null | undefined;  // 累加后的 usage
  turnCount: number;                       // 累加后的轮次
  isRunning: boolean;
}
```

累加逻辑由父组件（`ACPChatDialog`）负责，`AcpUsagePanel` 保持纯展示。

---

### 需求 5：Token 数量智能单位格式化

**现状**：`formatTokenCount()` 使用 `toLocaleString()` 千分位格式化（如 `12,040`），不带智能单位。

**变更规则**（纯前端展示，后台计算/存储始终使用原始整数）：

| 数值范围 | 展示格式 | 示例 |
|----------|----------|------|
| < 1,000 | 原始整数，无单位 | `842` |
| 1,000 ~ 999,999 | `K` 为单位，保留 1 位小数 | `1.2K`, `12.0K`, `123.4K` |
| ≥ 1,000,000 | `M` 为单位，保留 1 位小数 | `1.2M`, `12.3M` |

**特殊情况**：
- 小数部分为 0 时仍保留 `.0`（如 `12.0K`），保持对齐
- `used / size` 中的 `size` 通常是 `200,000` 或 `1,000,000`，格式化为 `200K` / `1.0M`
- 后端返回的数值不变（原始整数），格式化仅在前端 `formatTokenCount()` 中执行

**实现**：

替换现有 `formatTokenCount` 函数（`AcpUsagePanel.tsx:12`）：

```typescript
export function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}
```

同时更新 `RoundDetailPage.tsx:319` 中已有的 `formatLargeToken` 函数，统一使用 `formatTokenCount`（两个函数逻辑应一致，考虑抽到共享 util）。

---

### 需求 6：节点"查看详情"页展示多会话累加 Token

**现状**：

`RoundDetailPage.tsx` 的节点详情（`detail` tab）中，`resolveTokenUsage()` 仅取第一个会话的第一个 attempt 的 usage：

```typescript
function resolveTokenUsage(detail: NodeDetailVm): AcpUsageVm | null {
  const session = detail.acpConversations?.find(c => c.key === detail.selectedConversationKey)
    ?? detail.acpConversations?.[0];
  const attempt = session?.attempts.find(a => a.attemptId === session.activeAttemptId)
    ?? session?.attempts.at(-1);
  const usage = attempt?.acpSession?.usage ?? detail.acpSession?.usage;
  // ...
}
```

**变更**：

一个节点可能有多个会话窗口（`acpConversations`），需展示**所有会话的 Token 累加**：

```typescript
function resolveNodeTokenUsage(detail: NodeDetailVm): AcpUsageVm | null {
  const conversations = detail.acpConversations ?? [];
  if (conversations.length === 0) {
    return detail.acpSession?.usage ?? null;
  }
  const accumulated: AcpUsageVm = {};
  for (const conv of conversations) {
    for (const attempt of conv.attempts) {
      const usage = attempt.acpSession?.usage;
      if (!usage) continue;
      accumulated.inputTokens = (accumulated.inputTokens ?? 0) + (usage.inputTokens ?? 0);
      // ... 其余字段同理
    }
  }
  return accumulated;
}
```

**展示要求**：

| 规则 | 说明 |
|------|------|
| 实时性 | Token 不需要等节点结束才显示，节点运行中即可实时展示和更新 |
| 累加范围 | 该节点下所有 `acpConversations` 的所有 `attempts` 的 token |
| 展示位置 | 节点详情的 `InfoGrid` 中，与现有的 `输入 / 输出 / 缓存读取 / 总计` 行一致 |
| 格式化 | 使用统一的 `formatTokenCount` 智能单位 |

---

## 三、涉及文件汇总

### 前端

| 文件 | 改动类型 | 说明 |
|------|----------|------|
| `web/src/components/acp/AcpMessageTokenBadge.tsx` | **已删除** | 移除消息级 Token badge |
| `web/src/components/acp/AcpUsagePanel.tsx` | **已重构** | 移除 Collapsible，改为两行布局；概要行改名"上下文窗口"；更新 `formatTokenCount` 为智能单位 |
| `web/src/components/acp/ACPChatDialog.tsx` | **已修改** | 移除 `AcpMessageTokenBadge` 引用和渲染 |
| `web/src/pages/RoundDetailPage.tsx` | **已修改** | 重写 `resolveNodeTokenUsage` 为跨会话累加；统一 `formatTokenCount`；新增上下文窗口行 |
| `web/src/types.ts` | **已修改** | `AcpUsageVm` 移除 `accumulatedUsed` |
| `web/src/i18n.ts` | **已修改** | 新增 `contextWindow`/`tokenUsage`；移除 `title`/`accumulated`/`currentTurn`/`messageTokens` |
| `web/tests/acp/AcpUsagePanel.test.ts` | **已修改** | 更新 `formatTokenCount` 测试用例为智能单位断言 |

### 后端（Rust）

| 文件 | 改动类型 | 说明 |
|------|----------|------|
| `src-tauri/src/view_models.rs` | **已修改** | 移除 `inject_token_delta` 函数及 `last_used`/`last_message_used`/`accumulated_used`；`AcpUsageVm` 移除 `accumulated_used` 字段 |
| `src/acp/events.rs` | **无改动** | `extract_usage_fields` 保持不变 |

---

## 四、数据流变更

### 变更前

```
usage_update → scan_acp_events → AcpUsageVm
                                  ├─ used/size/accumulated_used → 概要行 "Token 用量"
                                  ├─ inputTokens/outputTokens/... → 折叠明细
                                  └─ last_used 差值 → inject_token_delta → 消息 badge

RoundDetailPage → resolveTokenUsage(单会话单attempt) → InfoGrid
```

### 变更后

```
usage_update → scan_acp_events → AcpUsageVm
                                  ├─ used/size → 第一行 "上下文窗口"
                                  └─ inputTokens/outputTokens/... → 第二行 "Token 用量"

RoundDetailPage → resolveNodeTokenUsage(多会话多attempt累加) → InfoGrid
  ├─ 消耗量(inputTokens等)跨所有attempts累加
  └─ used/size取最新attempt实时值
```

---

## 五、i18n Key 变更

| Key | 中文 | English | 操作 |
|-----|------|---------|------|
| `acp.usagePanel.contextWindow` | `上下文窗口` | `Context Window` | **新增**（替换原 title） |
| `acp.usagePanel.tokenUsage` | `Token 用量` | `Token Usage` | **新增** |
| `acp.usagePanel.title` | ~~Token 用量~~ | ~~Token Usage~~ | **删除** |
| `acp.usagePanel.accumulated` | ~~会话累计~~ | ~~Session total~~ | **删除** |
| `acp.usagePanel.currentTurn` | ~~当前轮~~ | ~~Current turn~~ | **删除** |
| `acp.usagePanel.cost` | ~~费用~~ | ~~Cost~~ | **删除** |
| `acp.usagePanel.contextWarning` | ~~接近上下文窗口上限~~ | ~~Approaching context window limit~~ | **删除** |
| `acp.messageTokens` | ~~{{tokens}} token~~ | ~~{{tokens}} tokens~~ | **删除** |

---

## 六、验收标准（已实现）

### 移除消息级 Token

- [x] Agent 文本消息气泡下方不再展示 token badge
- [x] `AcpMessageTokenBadge.tsx` 文件已删除
- [x] 后端 `inject_token_delta` 函数已移除，events JSONL 中不再写入 `_goldBand.tokens`
- [x] `last_used`/`last_message_used`/`accumulated_used` 变量已从 `scan_acp_events` 中移除

### 会话级面板

- [x] 面板第一行标题为"上下文窗口"，展示 `used / size`（智能单位，如 `12K / 200K`）
- [x] 面板第二行标题为"Token 用量"，展示输入/输出/缓存/总计
- [x] 两行始终可见，无折叠交互
- [x] 不展示 turn、accumulatedUsed、费用、进度条
- [x] `accumulatedUsed` 已从 AcpUsageVm 数据模型中移除
- [x] 无 usage 数据时面板不渲染

### 智能单位格式化

- [x] < 1K 展示原始数字（如 `842`）
- [x] ≥ 1K 且 < 1M 展示 `K` 单位（如 `12.0K`）
- [x] ≥ 1M 展示 `M` 单位（如 `1.2M`）
- [x] 后端数据不变，仍为原始整数
- [x] `formatTokenCount` 在 AcpUsagePanel 和 RoundDetailPage 中统一使用

### 节点详情页

- [x] `resolveNodeTokenUsage` 遍历所有 conversations 的所有 attempts 累加 token
- [x] `used / size` 取最新 attempt 实时值，不累加
- [x] InfoGrid 展示上下文窗口 + 输入 + 输出 + 缓存读取 + 总计
- [x] 复用现有 session 刷新机制，运行中实时更新
- [x] 无会话数据时不展示 token 行

### 国际化

- [x] 新增/修改的 i18n key 中英文均正确
- [x] 移除的 key 不再被引用

---

## 七、约束与边界

- **仅前端展示层改动为主**：后端改动仅限移除 `inject_token_delta` 和 `accumulated_used`
- **不改变 ACP 适配器行为**：不新增事件类型
- **仅 Claude ACP 适配器**：不扩展至其他 provider
- **不展示 turn、费用、进度条**：经 Deep Interview 确认移除
- **深色主题**：遵循 CLAUDE.md 约束，减少浅黑色方块和嵌套卡片
