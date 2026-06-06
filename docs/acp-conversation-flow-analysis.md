# ACP 对话流程与 Token 计算分析

> 基于项目 `claude-agent-acp-main` 源码分析，核心文件：`src/acp-agent.ts`

## 一、整体架构

本项目是一个**桥接层**，将 Claude Agent SDK 的能力通过 ACP (Agent Client Protocol) 标准协议暴露给客户端（如 Claude Code IDE 插件）。

```
┌──────────────┐     ACP Protocol (JSON-RPC)    ┌──────────────────┐    Claude Agent SDK    ┌────────────┐
│  ACP Client  │ ◄────────────────────────────► │ ClaudeAcpAgent   │ ◄────────────────────► │ Claude API │
│ (IDE/CLI)    │                                │ (本项目)          │                        │ (Anthropic) │
└──────────────┘                                └──────────────────┘                        └────────────┘
```

## 二、完整对话涉及的接口（按时序）

### 阶段 1：初始化 — `initialize()`

**`InitializeRequest → InitializeResponse`**（`acp-agent.ts:503`）

```
Client → Agent: InitializeRequest { clientCapabilities }
Agent → Client: InitializeResponse {
    protocolVersion: 1,
    agentInfo: { name, title, version },
    agentCapabilities: { promptCapabilities: { image, embeddedContext }, mcpCapabilities, loadSession, ... },
    authMethods: [ claude-ai-login | console-login | gateway | ... ]
}
```

- 客户端声明自己的能力（是否支持图片、MCP、终端认证等）
- Agent 返回支持的认证方式和能力集

### 阶段 2：认证 — `authenticate()`

**`AuthenticateRequest → void`**（`acp-agent.ts:724`）

```
Client → Agent: AuthenticateRequest { methodId: "gateway" | "gateway-bedrock", _meta: { gateway config } }
Agent:     记住 gateway 配置，供后续 API 调用使用
```

### 阶段 3：创建会话 — `newSession()`

**`NewSessionRequest → NewSessionResponse`**（`acp-agent.ts:651`）

```
Client → Agent: NewSessionRequest {
    cwd,                           // 工作目录
    mcpServers: [...],            // MCP 服务器配置
    additionalDirectories: [...]  // 附加目录
}
Agent → Client: NewSessionResponse {
    sessionId,
    modes: [ auto, default, plan, acceptEdits, ... ],   // 可用权限模式
    models: [...],                                       // 可用模型列表
    configOptions: [...]                                 // 配置选项
}
```

内部关键逻辑（`createSession()`，`acp-agent.ts:1894`）：

1. 创建 `SettingsManager` 并初始化
2. 调用 SDK 的 `query()` 创建 `Query` 对象（与 Claude 的长连接）
3. 创建 `Pushable<SDKUserMessage>` 输入流
4. 初始化 `accumulatedUsage = { inputTokens:0, outputTokens:0, cachedReadTokens:0, cachedWriteTokens:0 }`
5. 初始化 `contextWindowSize = DEFAULT_CONTEXT_WINDOW (200000)`

### 阶段 4：对话 — `prompt()` ⭐ 核心接口

**`PromptRequest → PromptResponse`**（`acp-agent.ts:732`）

这是最核心的接口，一次用户输入到最终响应的完整生命周期：

```
Client → Agent: PromptRequest {
    sessionId,
    prompt: [ { type: "text", text: "用户消息" } | { type: "image", ... } ]
}
```

**内部处理流程：**

```
┌──────────────────────────────────────────────────────────────────┐
│  1. 重置 accumulatedUsage（每次 prompt 重新从 0 计数）            │
│  2. 调用 promptToClaude(params) 转换为 SDKUserMessage            │
│  3. 将 userMessage push 到 session.input 流                      │
│  4. 进入 while(true) 循环，不断从 session.query.next() 读取消息   │
│                                                                  │
│  循环中处理的消息类型：                                            │
│  ┌─ "system" ─── init / status(compacting) / compact_boundary   │
│  │              local_command_output / session_state_changed     │
│  │              memory_recall / hook_* / task_*                  │
│  │                                                              │
│  ├─ "result" ── 累加 accumulatedUsage + 发送 usage_update       │
│  │              判断 stopReason (success/error/max_tokens/...)   │
│  │                                                              │
│  ├─ "stream_event" ── 实时更新 lastAssistantUsage               │
│  │                     发送 usage_update (used/size)             │
│  │                     转换为 agent_message_chunk / tool_call    │
│  │                                                              │
│  ├─ "assistant" ── 转换并发送 agent_message_chunk               │
│  │                  记录 lastAssistantUsage 快照                 │
│  │                                                              │
│  └─ "user" ───── 处理 prompt 队列和 replay 逻辑                  │
└──────────────────────────────────────────────────────────────────┘

Agent → Client: PromptResponse {
    stopReason: "end_turn" | "cancelled" | "max_tokens" | "max_turn_requests",
    usage: {
        inputTokens,
        outputTokens,
        cachedReadTokens,
        cachedWriteTokens,
        totalTokens    ← 四项之和
    }
}
```

**过程中向客户端推送的 SessionNotification 类型：**

| 通知类型 | 用途 | 触发时机 |
|---------|------|---------|
| `agent_message_chunk` | 流式文本/图片内容块 | stream_event 中的 content_block_delta |
| `tool_call` | 工具调用（含 pending/completed 状态） | stream_event 中的 tool_use |
| `tool_call_update` | 工具执行结果/状态更新 | stream_event 中的 tool_result |
| `usage_update` | 实时 token 用量和上下文窗口 | stream_event message_start/delta, result |
| `plan` | 任务列表（TODO）状态变更 | TaskCreate/TaskUpdate 输出解析 |

### 阶段 5：取消 — `cancel()`

**`CancelNotification → void`**

```
Client → Agent: CancelNotification { sessionId }
Agent:     session.cancelled = true
           session.abortController.abort()
           唤醒所有 pending messages
           → 当前 prompt() 返回 { stopReason: "cancelled" }
```

### 辅助接口

| 接口 | 用途 |
|------|------|
| `loadSession()` | 加载已有会话（按 sessionId 恢复） |
| `resumeSession()` | 恢复断开的会话连接 |
| `forkSession()` | 分叉一个新会话 |
| `listSessions()` | 列出所有会话 |
| `closeSession()` | 关闭会话（释放资源） |
| `deleteSession()` | 删除会话 |
| `setSessionModel()` | 切换模型 |
| `setSessionMode()` | 切换权限模式 |
| `setSessionConfigOption()` | 修改配置项 |

## 三、Prompt 生命周期深入分析

### 1. "一次 prompt" 是否等于用户一次输入？

**是的。** 每次用户发送一条消息，客户端调用一次 `prompt()`，这就是"一次 prompt"。

证据在 `acp-agent.ts:739-744`——每次进入 `prompt()`，`accumulatedUsage` 都从零开始：

```typescript
// acp-agent.ts:738-744
session.cancelled = false;
session.accumulatedUsage = {
    inputTokens: 0, outputTokens: 0,
    cachedReadTokens: 0, cachedWriteTokens: 0,
};
```

因此 `PromptResponse.usage` 反映的是**这一次用户输入触发的全部 token 消耗**。

### 2. `result` 的触发时机

Claude Agent SDK 内部运行一个 **agentic 循环**。每次 Claude API 完成一轮 request-response，SDK 就会发出一个 `result` 消息。

**`result` ≠ 工具调用的结果。** 工具调用（Grep、Glob、Read 等）的执行和结果通过 `stream_event` 传递（`tool_use` → `tool_result`）；而 `result` 是 SDK 告知 ACP 层"**一次 Claude API 调用完成了**"。

以下是一次 prompt 中工具调用的完整消息流示例：

```
用户输入 "帮我分析这个函数"
    │
    ├── API Call 1: Claude 决定调用 Grep 工具
    │     ├── SDK 发出 stream_event(message_start)     → 客户端收到 usage_update
    │     ├── SDK 发出 stream_event(content_delta)      → 客户端收到 agent_message_chunk
    │     ├── SDK 发出 stream_event(tool_use: Grep)     → 客户端收到 tool_call (pending)
    │     ├── SDK 执行 Grep，拿到结果
    │     └── SDK 发出 stream_event(tool_result)        → 客户端收到 tool_call_update (completed)
    │
    ├── API Call 2: 把 Grep 结果发回 Claude，Claude 决定调用 Read
    │     ├── SDK 发出 result ← 第 1 个 result（usage 累加到 accumulatedUsage）
    │     ├── SDK 发出 stream_event(message_start)     → 客户端收到 usage_update
    │     ├── SDK 发出 stream_event(tool_use: Read)     → 客户端收到 tool_call (pending)
    │     ├── SDK 执行 Read，拿到结果
    │     └── SDK 发出 stream_event(tool_result)        → 客户端收到 tool_call_update (completed)
    │
    ├── API Call 3: 把 Read 结果发回 Claude，Claude 生成最终回答
    │     ├── SDK 发出 result ← 第 2 个 result（usage 累加到 accumulatedUsage）
    │     ├── SDK 发出 stream_event(message_start)     → 客户端收到 usage_update
    │     └── SDK 发出 stream_event(content_delta)      → 客户端收到 agent_message_chunk（最终回答）
    │
    └── SDK 发出 session_state_changed(idle) ← 整个 prompt 结束
```

关键代码（`acp-agent.ts:964-969`）——处理完 `result` 后 **没有 return**，`while(true)` 循环继续，因为 agentic 循环可能还没结束：

```typescript
case "result": {
    // Accumulate usage from this result
    session.accumulatedUsage.inputTokens += message.usage.input_tokens;
    session.accumulatedUsage.outputTokens += message.usage.output_tokens;
    session.accumulatedUsage.cachedReadTokens += message.usage.cache_read_input_tokens;
    session.accumulatedUsage.cachedWriteTokens += message.usage.cache_creation_input_tokens;
    // ... 处理 stopReason，但继续循环
}
```

**结论：工具调用（Grep、Glob、Read 等）不会单独产生 result。每个 result 对应一次完整的 Claude API 调用完成，一次 prompt 可能产生多个 result。**

### 3. 一次 Prompt 何时结束？

**唯一正常退出点**是收到 `session_state_changed` 且 `state === "idle"`（`acp-agent.ts:893-898`）：

```typescript
case "session_state_changed": {
    if (message.state === "idle") {
        if (session.cancelled) {
            stopReason = "cancelled";
        }
        return { stopReason, usage: sessionUsage(session) };  // ← 这里才退出
    }
    break;
}
```

这意味着 SDK 内部的 agentic 循环彻底结束了（Claude 返回 `end_turn`，或达到最大轮次/预算限制），整个系统进入 idle 状态。

另外还有几个异常退出路径：

| 退出方式 | 代码位置 | 触发条件 |
|---------|---------|---------|
| `session_state_changed(idle)` | line 893-898 | agentic 循环正常结束（end_turn） |
| `session_state_changed(idle)` + cancelled | line 894-898 | 用户调用 `cancel()` 后系统 idle |
| `query.next()` 返回 `done` | line 798-803 | SDK 进程结束或连接断开 |
| 异常抛出 | line 1300 | rate limit、认证失败、进程崩溃等 |

**注意：Session（会话）本身是持久的**，可以多次调用 `prompt()`，直到调用 `closeSession()` 或 `deleteSession()` 才会销毁。

### 4. 完整 Prompt 流程图

```
prompt() 被调用
    │
    ├── 重置 accumulatedUsage = 0
    ├── 转换用户消息 → push 到 input 流
    │
    └── while(true) ──────────────────────────────────────────────┐
         │                                                         │
         ├── query.next() 取消息                                   │
         │                                                         │
         ├── stream_event ──► 推送 agent_message_chunk / tool_call │
         │                  推送 usage_update (used/size)          │
         │                  (只追踪顶层, parent_tool_use_id=null)   │
         │                                                         │
         ├── result ───────► 累加 accumulatedUsage                 │
         │                  推送 usage_update (used/size + cost)   │
         │                  判断 stopReason 但不退出 ────────────── │──► 继续循环
         │                                                         │
         ├── assistant ────► 推送 agent_message_chunk              │
         │                  更新 lastAssistantUsage 快照           │
         │                                                         │
         ├── system ───────► 处理 compacting / local_command 等    │
         │                                                         │
         └── session_state_changed(idle) ──► return PromptResponse │
                                              { stopReason, usage }
                                              ↑ 整个 prompt 结束
```

## 四、完整时序图

```
 Client                          ClaudeAcpAgent                     Claude Agent SDK              Claude API
   │                                   │                                  │                            │
   │── initialize() ──────────────────►│                                  │                            │
   │◄── InitializeResponse ───────────│                                  │                            │
   │                                   │                                  │                            │
   │── authenticate() ────────────────►│                                  │                            │
   │◄── void ──────────────────────────│                                  │                            │
   │                                   │                                  │                            │
   │── newSession({cwd,mcpServers}) ──►│── query(options) ────────────────►│                            │
   │◄── NewSessionResponse ───────────│◄── Query object ─────────────────│                            │
   │                                   │                                  │                            │
   │── prompt({prompt:[...]}) ────────►│── input.push(userMsg) ──────────►│── API Call 1 ──────────────►│
   │                                   │                                  │◄── stream ──────────────────│
   │  ◄─ notification: usage_update ───│◄── stream_event(message_start) ─│                            │
   │  ◄─ notification: agent_msg_chunk │◄── stream_event(content_delta) ─│                            │
   │  ◄─ notification: tool_call ──────│◄── stream_event(tool_use) ──────│                            │
   │                                   │                                  │── API Call 2 ──────────────►│
   │  ◄─ notification: tool_update ────│◄── stream_event(tool_result) ───│◄── stream ──────────────────│
   │  ◄─ notification: usage_update ───│◄── stream_event(message_delta) ─│                            │
   │                                   │                                  │── API Call 3 ──────────────►│
   │  ◄─ notification: usage_update ───│◄── result ───────────────────────│◄── result ──────────────────│
   │  ◄─ notification: usage_update ───│◄── result ───────────────────────│◄── result ──────────────────│
   │                                   │                                  │◄── result ──────────────────│
   │◄── PromptResponse {stopReason,   │                                  │                            │
   │                     usage} ───────│                                  │                            │
   │                                   │                                  │                            │
   │── cancel() ──────────────────────►│── abort() ──────────────────────►│                            │
   │◄── PromptResponse {cancelled} ───│                                  │                            │
```

## 五、`used/size` vs `usage` 的 Token 计算区别

### 1. `used` + `size` — 上下文窗口占用（实时通知）

出现在 `usage_update` 通知中，**流式推送**给客户端。

**数据来源**：`lastAssistantUsage` — **最后一次顶层 assistant 消息**的 API usage 快照

```typescript
// stream_event 处理中 (acp-agent.ts:1126-1135)
const nextUsage = totalTokens(lastAssistantUsage);
if (nextUsage !== lastAssistantTotalUsage) {
    lastAssistantTotalUsage = nextUsage;
    await this.client.sessionUpdate({
        sessionId: params.sessionId,
        update: {
            sessionUpdate: "usage_update",
            used:  nextUsage,                      // 当前已占用的 token 数
            size:  session.contextWindowSize,       // 上下文窗口总大小
        },
    });
}
```

```typescript
// result 处理中 (acp-agent.ts:989-1005)
await this.client.sessionUpdate({
    update: {
        sessionUpdate: "usage_update",
        used:  lastAssistantTotalUsage,             // 该轮结束时的占用
        size:  session.contextWindowSize,            // 上下文窗口总大小
        cost: { amount: message.total_cost_usd, currency: "USD" },
    },
});
```

**`used` 的计算方式** — `totalTokens()` 函数（`acp-agent.ts:2336`）：

```typescript
function totalTokens(usage: UsageSnapshot): number {
    return (
        usage.input_tokens +            // 输入 token（不含缓存）
        usage.output_tokens +           // 输出 token
        usage.cache_read_input_tokens + // 从缓存读取的 token
        usage.cache_creation_input_tokens // 写入缓存的 token
    );
}
```

> **核心语义：将四类 token 相加作为"上下文窗口占用"的近似值。**
> 因为当前 turn 的 output 会成为下一 turn 的 input，所以总和代表上下文的实际填充量。
> 源码注释（`acp-agent.ts:2332-2335`）明确说明：
> *"input_tokens excludes cache tokens — cache_read and cache_creation are reported separately — so summing all four is not double-counting"*

**`size` 的含义** — 模型的上下文窗口大小（`contextWindowSize`）：

- 默认值：200,000
- 更新来源：`result` 消息的 `modelUsage.contextWindow`
- 推断逻辑：从模型 ID 推断（如含 `1m` → 1,000,000）

**用途**：客户端用 `used/size` 显示类似 `94k/200k` 的进度条，告诉用户上下文还剩多少空间。

### 2. `usage` — 完整的 Token 消费明细（最终返回）

出现在 `PromptResponse` 返回值中，**对话结束时一次性返回**。

**数据来源**：`accumulatedUsage` — **所有 result 消息的累加**

```typescript
// result 消息到达时累加 (acp-agent.ts:966-969)
session.accumulatedUsage.inputTokens += message.usage.input_tokens;
session.accumulatedUsage.outputTokens += message.usage.output_tokens;
session.accumulatedUsage.cachedReadTokens += message.usage.cache_read_input_tokens;
session.accumulatedUsage.cachedWriteTokens += message.usage.cache_creation_input_tokens;
```

```typescript
// 最终返回 (acp-agent.ts:2318-2329)
function sessionUsage(session: Session) {
    return {
        inputTokens:        session.accumulatedUsage.inputTokens,
        outputTokens:       session.accumulatedUsage.outputTokens,
        cachedReadTokens:   session.accumulatedUsage.cachedReadTokens,
        cachedWriteTokens:  session.accumulatedUsage.cachedWriteTokens,
        totalTokens:        /* 上述四项之和 */,
    };
}
```

**用途**：计费统计、用量分析。一次 prompt 可能触发多次 API 调用（工具调用、子 agent），每次调用都会产生一个 `result`，全部累加。

### 3. 关键区别总结

| 维度 | `used/size` (usage_update 通知) | `usage` (PromptResponse) |
|------|------|------|
| **推送时机** | 流式，每次 token 变化时实时推送 | 一次性，prompt 结束时返回 |
| **数据来源** | `lastAssistantUsage` — 最后一次顶层 assistant 消息的快照 | `accumulatedUsage` — 所有 result 消息的累加 |
| **`used` / `totalTokens`** | 只反映当前顶层模型的单次 API 调用 token | 反映整个 prompt 周期内所有 API 调用的 token 总和 |
| **是否含子 agent** | ❌ 不含（`parent_tool_use_id === null` 过滤） | ✅ 含（所有 result 都累加） |
| **是否含费用** | result 事件中带 `cost`（`total_cost_usd`） | 不含费用信息 |
| **`size` 含义** | 上下文窗口大小，用于进度展示 | 无 size 字段 |
| **用途** | 实时 UI 展示（进度条 `94k/200k`） | 计费统计、用量分析 |

### 4. 具体数值差异示例

假设一次 prompt 触发了 3 次 API 调用（主模型 + 2 个子 agent）：

```
API 调用 1（主模型）:  input=10000, output=500, cache_read=5000, cache_write=2000
API 调用 2（子agent）: input=8000,  output=300, cache_read=3000, cache_write=1000
API 调用 3（子agent）: input=6000,  output=200, cache_read=2000, cache_write=500
```

**`usage_update.used`**（只看主模型最后一次快照）：

```
used = 10000 + 500 + 5000 + 2000 = 17,500
```

**`PromptResponse.usage.totalTokens`**（全部累加）：

```
inputTokens:       10000 + 8000 + 6000 = 24,000
outputTokens:      500 + 300 + 200 = 1,000
cachedReadTokens:  5000 + 3000 + 2000 = 10,000
cachedWriteTokens: 2000 + 1000 + 500 = 3,500
totalTokens:       24,000 + 1,000 + 10,000 + 3,500 = 38,500
```

**`used` ≈ 17,500 而 `usage.totalTokens` = 38,500**，差异显著。

- `used` 只追踪顶层模型的上下文占用 → 面向 UI 进度条
- `usage` 追踪整个对话周期的全部 token 消耗 → 面向计费和统计

## 六、核心数据结构

### Session 状态

```typescript
type Session = {
    query: Query;                              // SDK query 接口（长连接）
    input: Pushable<SDKUserMessage>;           // 消息输入流
    cancelled: boolean;                        // 是否已取消
    cwd: string;                               // 工作目录
    sessionFingerprint: string;                // 会话参数指纹（检测变更）
    settingsManager: SettingsManager;          // 设置管理器
    accumulatedUsage: AccumulatedUsage;        // 累计 token 用量
    modes: SessionModeState;                   // 可用权限模式
    models: SessionModelState;                 // 可用模型
    modelInfos: ModelInfo[];                   // 模型详细信息
    configOptions: SessionConfigOption[];      // 配置选项
    promptRunning: boolean;                    // 是否有 prompt 正在执行
    pendingMessages: Map<string, {...}>;       // 等待中的消息队列
    contextWindowSize: number;                 // 上下文窗口大小
    taskState: TaskState;                      // 任务列表状态
};
```

### Token 用量类型

```typescript
// 累计用量（用于 PromptResponse.usage）
type AccumulatedUsage = {
    inputTokens: number;
    outputTokens: number;
    cachedReadTokens: number;
    cachedWriteTokens: number;
};

// 单次快照（用于 usage_update.used）
type UsageSnapshot = {
    input_tokens: number;
    output_tokens: number;
    cache_read_input_tokens: number;
    cache_creation_input_tokens: number;
};
```
