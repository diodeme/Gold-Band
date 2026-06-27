# AskUserQuestion 适配方案

## 背景

Gold Band 通过 ACP 协议调用 Claude Code 等 Agent。Agent SDK 内置 `AskUserQuestion` 工具，可向用户提问并获取选择，但在 headless/ACP 模式下该工具默认被禁用。

claude-agent-acp **v0.45+** 原生支持 Elicitation 协议。当客户端在 `initialize` 时声明 `elicitation.form` 能力后，`AskUserQuestion` 工具被自动启用，Agent 的问答请求通过 `elicitation/create` JSON-RPC 方法直达客户端。

---

## 协议流程

```
Claude Code (Agent)
  │  AskUserQuestion({ questions: [...] })
  ▼
claude-agent-acp (v0.45+)
  │  检测到 clientCapabilities.elicitation.form
  │  自动启用 AskUserQuestion → 转为 elicitation/create
  ▼
JSON-RPC: elicitation/create({ mode: "form", requestedSchema: {...} })
  │
  ▼
Gold Band Rust Backend  (client.rs handle_elicitation_request)
  │
  ├─ ① 持久化请求 → acp.elicitation-request.{id}.json
  ├─ ② emit timeline event → elicitationRequest (status: "pending")
  ├─ ③ 同步阻塞 → wait_for_elicitation_response (200ms 轮询，等待响应或取消；当前默认不超时)
  │
  │   ... 用户在前端做出选择 ...
  │
  ├─ ④ 读取响应文件 → acp.elicitation-response.{id}.json
  ├─ ⑤ send_frame → JSON-RPC response { "action": "accept", "content": {...} }
  └─ ⑥ runtime 消费响应后持久化 timeline event → elicitationResponse (status: "completed")

Frontend (ACPChatDialog)
  │
  ├─ session prop / live update 合并 → effectiveEvents
  ├─ applyEventUpdates 保留 permission / elicitation 交互事件
  ├─ pendingElicitationFromEvents 扫描 request/response → 推导当前 pending
  └─ ElicitationCard 渲染 (内联卡片，非弹窗)
```

### 协议消息示例

**Agent → Client（请求）**：

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "elicitation/create",
  "params": {
    "mode": "form",
    "sessionId": "sess_xxx",
    "message": "请选择数据库类型",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "question_0": {
          "type": "string",
          "title": "数据库选择",
          "oneOf": [
            { "const": "MySQL", "title": "MySQL — 传统关系型数据库" },
            { "const": "PostgreSQL", "title": "PostgreSQL — 高级开源数据库" }
          ]
        },
        "question_0_custom": {
          "type": "string",
          "title": "Other",
          "description": "Type your own answer instead of choosing an option above (optional)."
        }
      }
    }
  }
}
```

**Client → Agent（用户确认）**：

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "action": "accept",
    "content": { "question_0": "MySQL" }
  }
}
```

**Client → Agent（超时/取消）**：

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "action": "decline"
  }
}
```

---

## 实现架构

### 文件清单

| 文件 | 类型 | 行数 | 说明 |
|------|------|------|------|
| `src/acp/elicitation.rs` | 新建 | 345 | 文件轮询、超时、取消、8 个单元测试 |
| `src/acp/mod.rs` | 修改 | +1 | 注册 `pub mod elicitation` |
| `src/acp/client.rs` | 修改 | +105 | 能力声明 + handle_elicitation_request + timeline 处理 + 取消集成 |
| `src/acp/events.rs` | 修改 | +55 | `elicitationRequest` / `elicitationResponse` 事件构造函数 |
| `src-tauri/src/commands.rs` | 修改 | +49 | `respond_elicitation` Tauri command |
| `src-tauri/src/main.rs` | 修改 | +9 | 注册到 invoke_handler |
| `web/src/api/client.ts` | 修改 | +1 | `RuntimeApi` 接口新增 `respondElicitation` |
| `web/src/api/desktop.ts` | 修改 | +3 | Desktop 实现 |
| `web/src/api/browser.ts` | 修改 | +3 | Browser mock |
| `web/src/api.ts` | 修改 | +4 | Barrel export |
| `web/src/components/acp/ElicitationCard.tsx` | 新建 | ~320 | 内联卡片组件（非弹窗） |
| `web/src/components/acp/ACPChatDialog.tsx` | 修改 | +126 | 集成 ElicitationCard + live subscription |

### 模块一：elicitation.rs（新建）

完全复用 `permission.rs` 的文件轮询 IPC 模式：

```
elicitation/create 到达
  │
  ▼
write_pending_elicitation()          → acp.elicitation-request.{id}.json
  │
  ▼
wait_for_elicitation_response()       ← 200ms 轮询
  ├─ response file 出现 → 读取并删除 → 返回
  ├─ cancel_requested 文件存在 → 自动 Decline
  └─ 调用方提供有限 timeout 时 → 超时自动 Decline
  │
  ▼
elicitation_response_result()        → 构造 JSON-RPC result
  │
  ▼
send_frame()                          → JSON-RPC response to ACP adapter

错误/取消路径:
cancel_pending_elicitation_requests() → 批量写入 Decline 响应文件
```

核心数据结构：

- **`ElicitationAction`** — enum `{ Accept, Decline }`，杜绝字符串硬编码
- **`PendingElicitationState`** — 持久化的待处理请求（含 `jsonrpc_id`, `message`, `requested_schema`）
- **`ElicitationResponseState`** — 持久化的用户响应（含 `action`, `content`）

单元测试覆盖（8/8）：

| 测试 | 覆盖场景 |
|------|----------|
| `write_and_read_pending_elicitation` | 持久化读写 |
| `wait_for_elicitation_response_normal` | 正常等待 + 响应 |
| `wait_for_elicitation_response_timeout` | 超时 → Decline |
| `wait_for_elicitation_response_cancelled` | 取消 → Decline |
| `elicitation_response_result_accept` | Accept action 构造 |
| `elicitation_response_result_decline` | Decline action 构造 |
| `cancel_pending_elicitation_requests_*` | 批量取消 |
| `sanitize_id_replaces_special_chars` | ID 净化 |

### 模块二：client.rs handle_elicitation_request

在 `handle_inbound` 中新增 `"elicitation/create"` 路由，执行 5 步处理：

1. 解析 `params.message` + `params.requestedSchema`，生成 `elicitation_id`
2. `write_pending_elicitation` 持久化
3. `elicitation_request_event` → `persist_event` 发送 UI 事件
4. `wait_for_elicitation_response` **同步阻塞**等待
5. runtime 在消费响应文件后先持久化 `elicitationResponse` 事件，再返回 JSON-RPC response，并追加用户可读的 `userTextDelta`

取消集成：在 `run_prompt` 的 cancel 和 error 分支中调用 `cancel_pending_elicitation_requests`。

### 模块三：events.rs 事件类型

| Event Kind | status | 说明 |
|------------|--------|------|
| `elicitationRequest` | `"pending"` | 用户待响应，不设 `ended_at`，保持"进行中" |
| `elicitationResponse` | `"completed"` | 用户已响应，设 `ended_at`，关闭对应 request |

timeline 处理：`elicitationRequest` 不关闭 text/thought/plan 流，不设结束时间。`elicitationResponse` 设开始/结束时间。

### 模块四：前端 ACPChatDialog 集成

**关键架构决策**：elicitation 与 permission 复用同一条 ACP live event / timeline 管道，不再维护单独的 `liveElicitationEvents` 状态。

```
ACPChatDialog 挂载
  │
  ├─ session prop ──→ effective?.events
  │
  ├─ applyEventUpdates()
  │     ├─ 普通可渲染事件直接进入消息流
  │     └─ permissionRequest / elicitationRequest / elicitationResponse
  │        作为隐藏交互事件保留在当前 session events 中
  │
  ├─ effectiveEvents = effective?.events ?? []
  │
  ├─ pendingElicitation = pendingElicitationFromEvents(
  │     effectiveEvents, answeredElicitations
  │   )
  │     ├─ 逆序扫描 events
  │     ├─ 见到 elicitationResponse → 标记对应 request 已回答
  │     ├─ 见到未回答的 pending elicitationRequest → 渲染可交互卡片
  │     └─ answeredElicitations 仅作为本地乐观态补充，不再是唯一事实源
  │
  └─ answerElicitation(id, content)
        ├─ setAnsweredElicitations → 卡片切换为只读
        └─ respondElicitation() → Tauri command → 写入响应文件
```

---

## 操作示例

### 示例 1：单选，点击即提交

```
┌─────────────────────────────────────────┐
│ 请选择你希望使用的后端框架/语言？         │
│                                         │
│ ┌─────────────────────────────────────┐ │
│ │ Java + Spring Boot — 企业级 Java    │ │  ← 点击即提交，无需确认按钮
│ └─────────────────────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │ Go + Gin/Echo — 高性能 Go 语言      │ │
│ └─────────────────────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │ Python + FastAPI — 现代 Python 异步  │ │
│ └─────────────────────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │ Node.js + Express/Nest — 全栈方案   │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ ┆ ✏ 其他答案...                         │  ← 点击展开自由文本输入
└─────────────────────────────────────────┘

          ↓ 用户点击 "Java + Spring Boot"

┌─────────────────────────────────────────┐
│ ✓ 请选择后端框架？  Java + Spring Boot  │  ← 已确认：只读展示，作为用户输入记录
└─────────────────────────────────────────┘
```

### 示例 2：自由文本（Other），回车提交

```
初始状态：
┌─────────────────────────────────────────┐
│ 请选择数据库                             │
│ ┌── 选项1 ──┐  ┌── 选项2 ──┐            │
│ ┆ ✏ 其他答案...                         │  ← 虚线按钮，点击展开
└─────────────────────────────────────────┘

          ↓ 点击 "✏ 其他答案..."

┌─────────────────────────────────────────┐
│ 请选择数据库                             │
│ ┌── 选项1 ──┐  ┌── 选项2 ──┐            │
│ ┌─────────────────────────────────┐ [→] │  ← 输入框 + 发送按钮
│ │ 人大金仓 KingbaseES             │    │
│ └─────────────────────────────────┘    │
└─────────────────────────────────────────┘

          ↓ 输入 "人大金仓 KingbaseES" 后点击 [→] / 回车

┌─────────────────────────────────────────┐
│ ✓ 请选择数据库？  人大金仓 KingbaseES    │  ← 已确认
└─────────────────────────────────────────┘
```

### 示例 3：有限超时——用户不响应

```
1. Agent 发送 elicitation/create
2. ElicitationCard 展示
3. 调用方传入有限 timeout，且用户在该时限内无操作
4. wait_for_elicitation_response 超时 → 返回 Decline
5. send_frame({ "action": "decline" })
6. Agent 收到 decline，自主降级继续执行
```

### 示例 4：取消 task

```
1. Agent 发送 elicitation/create
2. ElicitationCard 展示中
3. 用户取消 task
4. cancel_pending_elicitation_requests 批量写入 Decline 响应
5. wait_for_elicitation_response 轮询到取消标记 / 响应文件 → 返回 Decline
6. Agent 终止
```

---

## 影响性分析

### 对现有功能的影响

| 影响范围 | 风险等级 | 说明 |
|----------|----------|------|
| 已有 ACP session 流程 | 无 | `clientCapabilities.elicitation.form: {}` 不影响现有 `session/new`、`session/prompt`、`session/request_permission` 等协议行为 |
| Permission 交互 | 无 | `elicitation.rs` 与 `permission.rs` 文件独立，不同文件名前缀 |
| 会话模式（新 UI） | 增强 | ACPChatDialog 内联卡片与现有 PermissionRequestCard 视觉风格一致 |
| 工作台模式（旧 UI） | **修复** | 此前 AI-DYNAMIC 节点事件无法到达前端，已通过 live subscription 修复 |
| 查看已有 session 历史 | 兼容 | `elicitationRequest`/`elicitationResponse` 为新增 event kind，现有 timeline 加载逻辑兼容 |
| Browser mock | 无 | `browserApi.respondElicitation` 为 `Promise.resolve()` 空实现 |

### 兼容性矩阵

| 项目 | 要求 | 实测 |
|------|------|------|
| claude-agent-acp 版本 | ≥ v0.43.0 | ✅ v0.45.0 / v0.47.0 |
| `clientCapabilities` 格式 | `"form": {}`（空对象，非 boolean `true`） | ✅ 已实锤 |
| JSON-RPC method 名称 | `elicitation/create`（非 `unstable_createElicitation`） | ✅ 已实锤 |
| 响应格式 | `{ "action": "accept"\|"decline", "content": {...} }` | ✅ 已实锤 |
| 旧版本 adapter | 不声明 elicitation 时，`AskUserQuestion` 仍在 `disallowedTools` | ✅ 向后兼容 |

### 性能

| 项 | 影响 |
|----|------|
| `initialize` | 增加 1 个 JSON key，无性能影响 |
| elicitation 处理 | 同步阻塞 + 200ms 轮询，与 permission 一致 |
| 前端事件合并 | `useMemo` O(n)，elicitation 事件 ≤ 1 个 pending |
| 文件 I/O | 每个 elicitation 产生 2 个 JSON 文件（request + response），< 5KB |

### 安全

| 项 | 说明 |
|----|------|
| 路径安全 | `sanitize_id()` 过滤特殊字符 |
| 并发安全 | Agent SDK tool call 天然串行，同 session 内不可能并发 elicitation |
| 阻塞保护 | stop / teardown 会批量写入取消，及时解除等待；`wait_for_elicitation_response` 仍支持有限 timeout 测试 |
| 竞价条件 | 文件轮询 + 删除已消费文件，防止重复处理 |

---

## 关键设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| IPC 机制 | 文件轮询 + 同步阻塞 | 完全复用 permission 模式，无架构改动 |
| 超时策略 | runtime 默认不超时；保留 timeout 能力 | AskUserQuestion 与 permission 一样属于用户决策门，默认由用户响应或取消收敛 |
| UI 模式 | 内联卡片（非弹窗） | 不打断用户阅读流，与 PermissionRequestCard 风格一致 |
| 事件送达 | 复用 ACPChatDialog 通用 event merge | 与 permission 保持一致，避免为 elicitation 维护单独实时分支 |
| 状态管理 | `elicitationResponse` 事件 + `answeredElicitations` 乐观态 | 已回答状态可从 timeline 回放恢复，刷新后不依赖内存态 |
| Action 类型 | `ElicitationAction` enum | 杜绝字符串硬编码 |
| Schema 解析 | 前端动态渲染 JSON Schema | 适配 `oneOf`/`anyOf`/free-text 多种格式 |

---

## 验证清单

- [ ] Agent v0.45+ 正常模式：`AskUserQuestion` 不在 `disallowedTools` 中
- [ ] Agent 旧版本/不声明能力时：`AskUserQuestion` 仍在 `disallowedTools` 中（向后兼容）
- [ ] 单选：点击选项后卡片变为只读 `✓` 展示，Agent 收到 `action: "accept"` 并继续
- [ ] 自由文本：展开输入框 → 输入 → 回车/点击发送 → Agent 收到正确 `content`
- [ ] Rust 单测：有限 timeout 到期 → `wait_for_elicitation_response` 返回 `action: "decline"`
- [ ] 取消 task：`cancel_pending_elicitation_requests` 批量写入 Decline
- [ ] 多次提问：同 session 内连续多次 AskUserQuestion 均正常
- [ ] 工作台模式（RoundDetailPage）：ACP 事件实时到达 → ElicitationCard 渲染
- [ ] 会话模式（ConversationRunPage）：ACP 事件实时到达 → ElicitationCard 渲染
- [ ] Rust 单元测试 8/8 通过
- [ ] TypeScript 类型检查 0 errors
- [ ] Clippy 无新增 warning

---

## 多问题与多选场景评估

### AskUserQuestion 完整能力矩阵

`AskUserQuestion` 是 Claude Code 的内置工具，每次调用可发起 **1-4 个问题**，每个问题支持：

| 能力 | 约束 | 当前 Gold Band 适配状态 |
|------|------|------------------------|
| 问题数 | 1-4 个/次调用 | ⚠️ 多问题一次性全渲染，非逐个推进 |
| 选项数 | 2-4 个/题 | ✅ 正常 |
| 单选 (`multiSelect: false`) | 点击即选 | ⚠️ 点击即提交整个卡片（多问题时其他问题未答） |
| 多选 (`multiSelect: true`) | 复选框 + 确认 | ✅ 正常 |
| 自定义输入 ("Other") | 始终可用 | ✅ 正常（虚线按钮展开文本输入） |
| 混合模式 | 单选+多选共存 | ⚠️ 单选点击即提交，多选需点确认，行为不一致 |
| 答案键 | `question` 字段的完整文本 | ✅ 通过 `requestedSchema.properties` key |
| 答案值（单选） | 单个 label | ✅ |
| 答案值（多选） | label 数组/逗号分隔 | ✅ |

### ACP Adapter 转译格式

当 Claude Code ACP adapter（v0.45+）检测到 `clientCapabilities.elicitation.form` 后，`AskUserQuestion` 的 `questions` 数组被转译为单个 `elicitation/create` 请求：

**Claude Code 内置格式：**
```json
{
  "questions": [
    {
      "question": "请选择数据库类型：",
      "header": "数据库",
      "multiSelect": false,
      "options": [
        { "label": "MySQL", "description": "关系型数据库" },
        { "label": "PostgreSQL", "description": "高级关系型数据库" }
      ]
    },
    {
      "question": "需要哪些功能模块？",
      "header": "功能模块",
      "multiSelect": true,
      "options": [
        { "label": "用户认证", "description": "登录注册" },
        { "label": "日志系统", "description": "操作日志" }
      ]
    }
  ]
}
```

**ACP Adapter 转译为 `elicitation/create`（多问题合并到一个 schema）：**
```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "elicitation/create",
  "params": {
    "mode": "form",
    "message": "请选择数据库类型：\n需要哪些功能模块？",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "数据库": {
          "type": "string",
          "oneOf": [
            { "const": "MySQL",      "title": "MySQL" },
            { "const": "PostgreSQL", "title": "PostgreSQL" }
          ]
        },
        "功能模块": {
          "type": "array",
          "anyOf": [
            { "const": "用户认证", "title": "用户认证" },
            { "const": "日志系统", "title": "日志系统" }
          ]
        }
      }
    }
  }
}
```

**关键事实**：多个 `questions` 被合并到同一个 `elicitation/create` 的一个 `requestedSchema` 中，其中每个 `question.header` 成为一个 property key。用户需要回答**所有属性**，答案通过 `content` 对象一次性返回。

### Schema 格式兼容性

Gold Band 当前支持的 schema 格式 vs MCP 标准格式：

| Schema 关键字 | Gold Band ElicitationCard | Gold Band format_elicitation_answer | MCP 标准 | Claude Code ACP adapter |
|--------------|--------------------------|-------------------------------------|---------|------------------------|
| `oneOf` + `const`/`title` | ✅ | ✅ | ❌ 不支持 | ✅ 使用 |
| `anyOf` + `const`/`title` | ✅ | ✅ | ❌ 不支持 | ✅ 使用 |
| `enum` + `enumNames` | ❌ 不支持 | ❌ 不支持 | ✅ 标准格式 | ❌ 不使用 |
| `type: "string"` (自由文本) | ✅ | ✅ | ✅ | ✅ |
| `type: "array"` (多选) | ✅ | ✅ | ✅ | ✅ |

**结论**：当前对齐 Claude Code ACP adapter 的实际格式（`oneOf`/`anyOf`），工作正常。MCP 标准 `enum`/`enumNames` 格式作为保底可后续支持。

---

## 待优化项（按优先级排列）

### P0 — 多问题逐个展示（向导式流程）

**现状**：当一个 `elicitation/create` 请求的 `requestedSchema` 包含多个 properties 时，`ElicitationCard` 将它们全部渲染在同一张卡片中。当用户点击一个单选选项时，`onRespond(content)` 立即被调用，此时只填充了该单选字段的值，其他字段的答案丢失。

**问题**：
- 单选字段点击即提交整个卡片 → 其他问题被跳过
- 混合多选/单选时行为不一致
- 无法逐个确认每个问题的答案

**期望行为**：向导式步骤流程：

```
━━━ 步骤 1/2：选择数据库 ━━━
┌─────────────────────────────────────┐
│ 请选择数据库类型：                    │
│  ▸ MySQL                             │
│    PostgreSQL                        │
│ 进度：● ○                            │
└─────────────────────────────────────┘

用户点击 "MySQL" → 进入步骤 2

━━━ 步骤 2/2：选择功能模块 ━━━
┌─────────────────────────────────────┐
│ 需要哪些功能模块？                    │
│  ☐ 用户认证    ☐ 日志系统             │
│         [确认选择]                    │
│ 进度：● ●                            │
└─────────────────────────────────────┘

用户勾选两项 + 确认 → 提交完整 content
```

**实现要点**：
- `ElicitationCard` 内部维护 `currentStep` 状态（0 到 fields.length-1）
- 每次只渲染一个 field 的问题和选项
- 非最后步骤：用户选择后 → 进入下一步
- 最后步骤：用户选择后 → 调用 `onRespond(fullContent)` 提交完整答案
- 进度指示器（可选 P3）
- 答案逐步积累在内部 state 中（`answers: Record<string, unknown>`）

### P1 — 答案文本格式化改进

**现状**：后端 `format_elicitation_answer` 将所有答案字段用 `；` 连接为一行：

```
MySQL；用户认证、日志系统
```

**问题**：丢失了问题-答案的对应关系。

**期望行为**：逐行格式化：

```
数据库：MySQL
功能模块：用户认证、日志系统
```

**实现要点**：
- 修改 `format_elicitation_answer` 输出逐行格式（`\n` 分隔或 Markdown 列表）
- 在 `user_prompt_event` 构造时保持消息气泡内容为格式化文本
- 可配合 P2（Markdown 渲染）实现更好的视觉效果

### P1 — 单选改为"选中态 + 确认按钮"

**现状**：单选字段（`oneOf`）点击即提交。优点：步数少。缺点：用户无法反悔。

**期望行为**：
- 点击选项 → 高亮选中态（蓝色边框/背景），不提交
- 底部出现"确认"按钮
- 用户可更改选择后点击"确认"
- 与多选操作模式一致

**实现要点**：
- `ElicitationCard` 内部新增 `selectedValue` 状态
- 点击选项设置 `selectedValue` → 显示确认按钮
- 确认按钮点击 → 调用 `handleSelect`（单步选择）或 `onRespond`（最后一步提交）

### P2 — 用户消息气泡 Markdown 渲染

**现状**：`userTextDelta` 事件在 `MessageBubble` 中以纯文本渲染。elicitation 答案源为 `userTextDelta` 时也是纯文本。

**期望行为**：elicitation 答案气泡使用 Markdown 渲染，支持粗体、列表等轻量格式。

**实现方式**：在 `user_prompt_event` 构造时增加 `raw` 标记（如 `"elicitationAnswer": true`），前端 `MessageBubble` 检测此标记后走 Markdown 渲染分支，而非纯文本分支。

### P2 — 超时时长可配置

**现状**：`ELICITATION_DEFAULT_TIMEOUT` 当前为 `Duration::MAX`，ACP runtime 默认持续等待直到用户响应或运行被取消。

**期望**：后续可通过桌面设置或配置文件调整，例如 `acp.elicitation_timeout_seconds` 配置项，让不同产品模式自行选择有限超时或长期等待。

### P3 — `enum`/`enumNames` 支持

**现状**：只支持 Claude Code ACP adapter 的 `oneOf`/`anyOf` 格式。MCP 标准使用 `enum`/`enumNames`。

```json
{
  "type": "string",
  "enum": ["mysql", "postgres", "mongodb"],
  "enumNames": ["MySQL", "PostgreSQL", "MongoDB"]
}
```

**实现**：`ElicitationCard.tsx` 的 `fields` 计算 + `elicitation.rs` 的 `format_elicitation_answer` 各新增一个 `enum`/`enumNames` 解析分支。

### P3 — 进度指示器

多问题向导式流程的顶部步骤进度 UI（`步骤 2/3`、步骤条、圆点指示器）。

### P3 — 跳过可选问题

检测 `requestedSchema.properties[].optional` 或 MCP `required` 数组，非必答问题提供"跳过"按钮。

---

## 优化总结

| 优先级 | 项目 | 影响范围 | 预期工作量 |
|--------|------|---------|-----------|
| ✅ 已完成 | 单选/多选/自定义文本基本交互 | — | — |
| ✅ 已完成 | 后端 userTextDelta 用户消息气泡 | — | — |
| ✅ 已完成 | key prop 修复选项陈旧问题 | — | — |
| ✅ 已完成 | project_id 修复响应文件目录错位 | — | — |
| **P0** | **多问题逐个展示（向导式流程）** | ElicitationCard.tsx | 中 |
| **P1** | **答案文本格式化改进** | elicitation.rs | 小 |
| **P1** | **单选改为选中态+确认按钮** | ElicitationCard.tsx | 小 |
| P2 | 消息气泡 Markdown 渲染 | events.rs + ACPChatDialog.tsx | 小 |
| P2 | 可配置超时时长 | elicitation.rs + config | 小 |
| P3 | enum/enumNames 支持 | ElicitationCard.tsx + elicitation.rs | 小 |
| P3 | 进度指示器 | ElicitationCard.tsx | 小 |
| P3 | 跳过可选问题 | ElicitationCard.tsx | 小 |
