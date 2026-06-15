# AskUserQuestion 适配方案

## 背景

当前项目通过 ACP 协议调用 Claude Code 等 Agent。Agent SDK 内置了 `AskUserQuestion` 工具，可向用户提问并获取选择，但在 headless/ACP 模式下该工具默认被禁用。

**关键发现**：claude-agent-acp v0.43.0（commit #756）已原生支持 Elicitation 协议，当客户端声明 `elicitation.form` 能力后，`AskUserQuestion` 工具会被自动启用，Agent 的问答请求通过 ACP JSON-RPC 直达客户端，无需任何中间层。

## 核心结论

**无需自建 MCP Server 或注册新工具**。只需三步适配：

1. `initialize` 时声明 `clientCapabilities.elicitation.form`
2. 处理 `unstable_createElicitation` JSON-RPC 请求
3. 前端展示表单弹窗，用户选择后返回响应

---

## 协议流程

```
Claude Code (Agent)
  │
  │ 内置 tool_call: AskUserQuestion({ questions: [...] })
  ▼
claude-agent-acp (ACP Adapter)
  │
  │ 检测到客户端声明 elicitation.form 能力
  │ 自动将 AskUserQuestion 转换为 ACP form elicitation
  ▼
JSON-RPC request: unstable_createElicitation({ mode: "form", requestedSchema: {...} })
  │
  ▼
Gold Band Rust Backend
  │
  ├─ handle_inbound 识别新消息类型
  ├─ 构造 AcpUiEvent { kind: "elicitationRequest" }
  ├─ emit Tauri event ──────────────────────────────▶ Frontend
  │                                                   │
  │                                                   ├─ 渲染 ElicitationDialog
  │                                                   ├─ 用户选择选项
  │  ◀─────────────────────── 调用                    │
  │     respond_elicitation Tauri command              │
  ▼                                                   ▼
Rust 通过 pending response channel 返回 JSON-RPC response
  │
  ▼
claude-agent-acp 收到响应 → 转换回 AskUserQuestion 结果 → Claude Code 继续执行
```

### 协议消息示例

**Agent → Client（请求）**：

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "unstable_createElicitation",
  "params": {
    "mode": "form",
    "sessionId": "sess_xxx",
    "message": "请选择数据库类型",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "answer": {
          "type": "string",
          "title": "数据库选择",
          "oneOf": [
            { "const": "mysql", "title": "MySQL" },
            { "const": "postgresql", "title": "PostgreSQL" },
            { "const": "mongodb", "title": "MongoDB" }
          ]
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
  "id": 42,
  "result": {
    "action": "accept",
    "content": { "answer": "postgresql" }
  }
}
```

**Client → Agent（用户拒绝/取消）**：

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {
    "action": "decline"
  }
}
```

---

## 详细实施方案

### 模块一：ACP 客户端能力声明

**文件**：`src/acp/client.rs`

**改动**：`initialize_with_timeout` 方法中 `clientCapabilities` 增加 `elicitation` 声明。

```rust
fn initialize_with_timeout(&mut self, timeout: Option<Duration>) -> Result<Value> {
    self.request_with_timeout(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "elicitation": {
                    "form": true  // 声明支持表单式问答
                }
            },
            "clientInfo": {
                "name": "gold-band",
                "title": "Gold Band",
                "version": crate::domain::VERSION,
            }
        }),
        timeout,
    )
}
```

**效果**：claude-agent-acp 检测到此能力后，自动启用 `AskUserQuestion` 工具，并将问答请求通过 `unstable_createElicitation` 路由到客户端。

---

### 模块二：Rust Backend 处理 Elicitation 请求

#### ②-1 `src/acp/client.rs` — 处理 `unstable_createElicitation` 消息

在 `handle_inbound` 方法中新增对 `unstable_createElicitation` 的识别和处理。

**设计要点**：复用现有的 `request_with_progress` 循环模式。`unstable_createElicitation` 是一个 JSON-RPC request（Agent 主动发给 Client 的），需要我们返回 JSON-RPC response。

```rust
// handle_inbound 中新增分支
"unstable_createElicitation" => {
    // 1. 解析请求参数
    let params = frame.get("params").cloned().unwrap_or(json!({}));
    let request_id = frame.get("id").cloned();
    let session_id = params.get("sessionId").and_then(Value::as_str).unwrap_or("");
    let message = params.get("message").and_then(Value::as_str).unwrap_or("");
    let schema = params.get("requestedSchema").cloned().unwrap_or(json!({}));

    // 2. 生成唯一 elicitation_id
    let elicitation_id = format!("elicit-{}", Uuid::new_v4().simple());

    // 3. 持久化请求到 attempt dir（供前端查询）
    self.write_elicitation_request(&elicitation_id, &params)?;

    // 4. 构造 AcpUiEvent 发送给前端
    let event = elicitation_request_event(
        self.next_seq(),
        elicitation_id.clone(),
        message.to_string(),
        schema,
        Some(session_id.to_string()),
    );
    self.emit_ui_event(&event)?;

    // 5. 将 pending response 挂起，等待前端通过 Tauri command 回填
    //    response channel 使用与 permission 相同的 oneshot 模式
    self.pending_elicitation_responses
        .lock()
        .unwrap()
        .insert(elicitation_id.clone(), (request_id, tx));

    // 6. 等待用户响应（阻塞当前 inbound 处理线程）
    //    注意：不能阻塞，需要将 pending 状态存入 map，
    //    由 respond_elicitation Tauri command 触发返回
}
```

**Pending Response 管理结构**：

```rust
// 在 AcpRuntime 中新增
pub struct PendingElicitation {
    pub jsonrpc_id: Option<Value>,       // 原始 JSON-RPC request id
    pub tx: oneshot::Sender<Value>,      // 用于唤醒等待协程
}

// AcpRuntime 中新增字段
pending_elicitation_responses: Arc<Mutex<HashMap<String, PendingElicitation>>>,
```

**响应回调**（由 Tauri command 调用）：

```rust
pub fn respond_elicitation(
    &self,
    elicitation_id: &str,
    action: &str,           // "accept" | "decline" | "cancel"
    content: Option<Value>, // 用户选择的内容
) -> Result<()> {
    let mut map = self.pending_elicitation_responses.lock().unwrap();
    if let Some(pending) = map.remove(elicitation_id) {
        let response = json!({
            "jsonrpc": "2.0",
            "id": pending.jsonrpc_id,
            "result": {
                "action": action,
                "content": content.unwrap_or(json!({})),
            }
        });
        pending.tx.send(response)?;
    }
    Ok(())
}
```

#### ②-2 `src/acp/events.rs` — 新增 Event 类型

```rust
// 新增 event kind 常量
pub const ELICITATION_REQUEST: &str = "elicitationRequest";
pub const ELICITATION_RESPONSE: &str = "elicitationResponse";

// Elicitation 请求事件构造
pub fn elicitation_request_event(
    seq: u64,
    elicitation_id: String,
    message: String,
    schema: Value,
    session_id: Option<String>,
) -> AcpUiEvent {
    AcpUiEvent {
        seq,
        kind: ELICITATION_REQUEST.to_string(),
        data: json!({
            "elicitationId": elicitation_id,
            "message": message,
            "requestedSchema": schema,
        }),
        session_id,
        ..Default::default()
    }
}

// Elicitation 决策事件（用户回答后追加）
pub fn elicitation_response_event(
    seq: u64,
    elicitation_id: String,
    action: String,
    content: Option<Value>,
) -> AcpUiEvent {
    AcpUiEvent {
        seq,
        kind: ELICITATION_RESPONSE.to_string(),
        data: json!({
            "elicitationId": elicitation_id,
            "action": action,
            "content": content,
        }),
        ..Default::default()
    }
}
```

**`timeline_item_for_event` 中新增**：

```rust
"elicitationRequest" => {
    self.active_text_stream = None;
    self.active_thought_stream = None;
    self.active_plan_stream = None;
    item.id = format!("elicitation-{}", item.id);
    item.started_seq = Some(item.started_seq.unwrap_or(seq));
    item.ended_seq = Some(seq);
    item.started_at = Some(item.started_at.clone().unwrap_or_else(|| timestamp.clone()));
    item.ended_at = Some(timestamp);
}
"elicitationResponse" => {
    item.id = format!("elicitation-response-{}", item.id);
    item.started_seq = Some(seq);
    item.ended_seq = Some(seq);
}
```

#### ②-3 `src-tauri/src/commands.rs` — 新增 Tauri Command

```rust
#[tauri::command]
pub fn respond_elicitation(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    elicitation_id: String,
    action: String,               // "accept" | "decline" | "cancel"
    content: Option<serde_json::Value>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let runtime = resolve_acp_runtime(&app, &task_id, &run_id, &round_id,
        outer_node_id.as_deref(), outer_attempt_id.as_deref(), &node_id, &attempt_id)?;

    runtime.respond_elicitation(&elicitation_id, &action, content)
        .map_err(command_error)?;

    // 追加 timeline decision event
    let seq = runtime.next_seq();
    let event = elicitation_response_event(seq, elicitation_id, action, content);
    runtime.emit_ui_event(&event)?;

    Ok(())
}
```

同时在 `main.rs` 中注册该 command。

---

### 模块三：Frontend UI

#### ③-1 `web/src/components/acp/ElicitationDialog.tsx`（新建 ~100 行）

基于 shadcn/ui `Dialog` + `RadioGroup` 组件：

```
┌──────────────────────────────────────┐
│  [title] / "需要您的选择"             │
│  ────────────────────────────────    │
│                                      │
│  message 文本（支持 Markdown）        │
│                                      │
│  ◉ option 1  — description...        │
│  ○ option 2  — description...        │
│  ○ option 3                          │
│                                      │
│  ────────────────────────────────    │
│              [跳过]      [确认]       │
└──────────────────────────────────────┘
```

```typescript
interface ElicitationDialogProps {
  open: boolean;
  elicitationId: string;
  message: string;
  schema: ElicitationSchema;
  onRespond: (action: "accept" | "decline", content?: Record<string, unknown>) => void;
}

interface ElicitationSchema {
  type: "object";
  properties?: Record<string, ElicitationPropertySchema>;
}

interface ElicitationPropertySchema {
  type: "string" | "array";
  title?: string;
  description?: string;
  oneOf?: Array<{ const: string; title: string }>;  // 单选选项
  anyOf?: Array<{ const: string; title: string }>;   // 多选选项
}
```

组件要点：
- 从 `schema.properties` 解析出每个字段及其选项
- 单选（`oneOf`）使用 `RadioGroup`
- 多选（`anyOf`）使用 `Checkbox` 组
- 无选项的 `string` 字段使用 `Input`（自由文本）
- 确认按钮在必填项未填时 disabled
- 跳过按钮 → `onRespond("decline")`，Agent 会自动降级处理

#### ③-2 `web/src/components/acp/ACPChatDialog.tsx` 集成

在 timeline 事件渲染中：

1. 识别 `kind === "elicitationRequest"` 的事件
2. 解析 `data.requestedSchema` 构建表单
3. 弹出 `ElicitationDialog`
4. 用户操作后调用 `respondElicitation` API
5. 更新事件状态（可追加 `elicitationResponse` 事件做视觉反馈）

#### ③-3 `web/src/api/*.ts` 新增 API 方法

```typescript
// client.ts - 接口定义
respondElicitation(
  taskId: string, runId: string, roundId: string,
  nodeId: string, attemptId: string,
  elicitationId: string,
  action: "accept" | "decline" | "cancel",
  content?: Record<string, unknown>,
  outerNodeId?: string | null,
  outerAttemptId?: string | null,
): Promise<void>;

// desktop.ts
respondElicitation(...) {
  return invokeCommand('respond_elicitation', {
    taskId, runId, roundId, nodeId, attemptId,
    elicitationId, action, content,
    outerNodeId, outerAttemptId,
  });
}
```

#### ③-4 `web/src/types.ts` 新增类型

```typescript
interface ElicitationRequestVm {
  elicitationId: string;
  message: string;
  requestedSchema: ElicitationSchema;
  status: "pending" | "responded";
}
```

---

## 关键设计决策汇总

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 实现方式 | ACP 原生 Elicitation | 协议内置，无需自建 MCP Server |
| 客户端能力声明 | `elicitation.form: true` | 最小改动启用 AskUserQuestion |
| 消息处理 | 在 `handle_inbound` 中新增分支 | 复用现有 JSON-RPC 处理框架 |
| Pending Response | oneshot channel + HashMap | 与现有 permission 响应模式一致 |
| UI 组件库 | shadcn/ui Dialog + RadioGroup/Checkbox | 项目统一使用的组件库 |
| Schema 解析 | 前端从 JSON Schema 动态渲染表单 | 适配 AskUserQuestion 的多 question + 多选项格式 |
| 取消/跳过语义 | 返回 `action: "decline"` | Agent SDK 会自动降级，不做该选择 |

---

## 工作量估算

| 模块 | 文件 | 预估代码量 | 难度 |
|------|------|-----------|------|
| ACP 能力声明 | `src/acp/client.rs` | ~5 行 | 低 |
| Elicitation 处理 | `src/acp/client.rs` | ~50 行 | 中等 |
| Event 定义 | `src/acp/events.rs` | ~30 行 | 低 |
| Tauri Command | `src-tauri/src/commands.rs`, `main.rs` | ~35 行 | 低 |
| Frontend Dialog | `web/src/components/acp/ElicitationDialog.tsx` | ~100 行 | 中等 |
| Frontend 集成 | `ACPChatDialog.tsx`, `api/*.ts`, `types.ts` | ~50 行 | 低 |
| **总计** | **~7 个文件** | **~270 行** | **中等** |

---

## 与原 MCP Server 方案对比

| 维度 | 原 MCP Server 方案 | 本方案（ACP 原生） |
|------|-------------------|-------------------|
| 新增代码量 | ~565 行（10 文件） | ~270 行（7 文件） |
| 新增进程 | 需管理 MCP Server 子进程 | 无新进程 |
| 通信机制 | 文件轮询（200ms） | JSON-RPC 直达 |
| 工具注册 | 需自建 askHuman 工具 | AskUserQuestion SDK 内置 |
| CLI subcommand | 需新增 `mcp-server` | 不需要 |
| Schema 支持 | 简单 string[] | 完整 JSON Schema（title/description/multiSelect） |
| 扩展性 | 仅 askHuman | form + URL 两种模式 |
| 核心风险 | MCP tools 可能不被支持 | 协议内置，无此风险 |

---

## 实施顺序

```
Phase 1: Backend 核心
  ├─ src/acp/client.rs — clientCapabilities + handle_inbound 新分支
  ├─ src/acp/events.rs — event type + timeline item
  └─ src-tauri/src/commands.rs — respond_elicitation command

Phase 2: Frontend UI
  ├─ web/src/components/acp/ElicitationDialog.tsx
  ├─ web/src/api/*.ts — API 方法
  ├─ web/src/types.ts — 类型定义
  └─ web/src/components/acp/ACPChatDialog.tsx — 集成

Phase 3: 集成验证
  └─ 端到端手动验证
```

---

## 验证步骤

1. **协议验证**：`initialize` 后确认 claude-agent-acp 不再将 `AskUserQuestion` 列入 `disallowedTools`
2. **消息接收**：Agent 调用 `AskUserQuestion` 时，Gold Band 能收到 `unstable_createElicitation` 请求
3. **前端展示**：弹窗正确渲染问题文本和选项
4. **响应返回**：用户选择后 Agent 能收到正确的 `action` + `content`
5. **跳过测试**：用户点击跳过时 Agent 收到 `decline` 并自主降级
6. **多轮测试**：同一 session 内多次 elicitation 调用是否正常
7. **Schema 复杂度**：验证单选、多选、自由文本等不同 schema 渲染正确

---

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| `unstable_` API 变更 | 低 | 中 | API 前缀表明实验性，但 ACP 协议有向后兼容承诺；即使变更，改动量小 |
| Rust crate 类型缺失 | 中 | 低 | 直接用 `serde_json::Value` 处理原始 JSON-RPC，无需等 crate 更新 |
| claude-agent-acp 版本 | 低 | 低 | 升级 npx 命令指定 ≥ v0.43.0 版本即可 |
| Schema 复杂度 | 低 | 中 | 优先支持 `oneOf` 单选 + 自由文本，后续迭代支持 `anyOf` 多选 |
