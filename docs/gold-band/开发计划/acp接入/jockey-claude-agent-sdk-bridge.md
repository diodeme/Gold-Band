# Jockey 处理 Claude Agent SDK 语言限制的方式

## 1. 问题背景

Claude Agent SDK 当前主要以 Node.js / Python SDK 形态提供；`@agentclientprotocol/claude-agent-acp` 是基于 TypeScript Claude Agent SDK 实现的 ACP agent adapter。

Gold Band 是 Rust / Tauri 架构，因此不应把 Claude Agent SDK 直接嵌入 Rust runtime，也不应为了接入 Claude ACP 而重写一套 Rust 版 Claude Agent SDK。

Jockey 的解法是：**Rust 只实现 ACP client，Claude Agent SDK 留在 Node adapter sidecar 中运行，两者通过 ACP stdio 通信。**

```text
Rust / Tauri host
  -> ACP client over stdio
    -> Node sidecar: claude-agent-acp
      -> Claude Agent SDK
        -> Claude Code capabilities
```

Gold Band 借鉴的是这条 ACP sidecar 路径，不是恢复 Claude Code legacy CLI fallback。

## 2. Jockey 的关键做法

### 2.1 Runtime 只识别 provider/runtime 类型

Jockey 在 Rust 中定义 runtime 类型，例如：

```text
mock
claude-code / claude-acp
gemini-cli
codex-cli / codex-acp
```

在 Jockey 语境中，`claude-code` 并不表示 Rust 直接嵌入 Claude Agent SDK，而是表示 Rust 解析并启动一个 ACP-compatible adapter。Gold Band 后续不沿用 direct Claude Code CLI 作为并列运行路径，而是统一通过 ACP provider 表达 Claude 接入。

参考文件：

```text
.external/jockey/src-tauri/src/runtime_kind.rs
```

### 2.2 Adapter 解析分三层

Jockey 对 `claude-agent-acp` 的解析顺序是：

1. 优先使用应用托管目录中的 adapter binary：

```text
app_data/adapters/node_modules/.bin/claude-agent-acp
```

2. 再查找用户 PATH 中的：

```text
claude-agent-acp
```

3. 最后使用包管理器临时运行：

```text
pnpm dlx @agentclientprotocol/claude-agent-acp@latest
npx -y @agentclientprotocol/claude-agent-acp@latest
```

参考文件：

```text
.external/jockey/src-tauri/src/acp/adapter.rs
```

这里的 package runner 是 adapter 解析策略中的后备来源，不是 Claude Code legacy CLI fallback。它解决的是“如何找到 ACP adapter”，不是“是否回退到 direct CLI 运行”。

### 2.3 Rust 侧使用 ACP Rust crate 做 client

Jockey Rust 侧使用 `agent_client_protocol` crate，并创建 `ClientSideConnection`：

```text
Rust process
  -> spawn adapter process
  -> pipe stdin/stdout
  -> acp::ClientSideConnection
```

参考文件：

```text
.external/jockey/src-tauri/src/acp/connection.rs
.external/jockey/src-tauri/src/acp/session/cold_start.rs
```

### 2.4 ACP 初始化与 session 生命周期

Jockey 启动 adapter 后执行：

1. `initialize`
2. 若支持 `load_session`，尝试 `session/load`
3. 不可恢复时创建 `session/new`
4. `session/prompt`
5. 监听 `session/update`

其中 session id 会被保存，用于后续恢复。

参考文件：

```text
.external/jockey/src-tauri/src/acp/session/cold_start.rs
.external/jockey/src-tauri/src/acp/worker/handlers.rs
```

### 2.5 ACP 事件不直接给 UI 使用

Jockey 会把 ACP `SessionUpdate` 转换成自己的内部事件：

```text
TextDelta
ThoughtDelta
ToolCall
ToolCallUpdate
Plan
PermissionRequest
ModeUpdate
ConfigUpdate
SessionInfo
StatusUpdate
AvailableCommands
AvailableModes
SessionError
```

再通过 Tauri event 推给前端。

参考文件：

```text
.external/jockey/src-tauri/src/acp/client.rs
.external/jockey/src-tauri/src/acp/worker/types.rs
.external/jockey/src/lib/acpEventBus.ts
.external/jockey/src/lib/acpEventBridge.ts
```

这一点对 Gold Band 很重要：前端不应直接散落解析 ACP 原始结构，而应由会话详情 ViewModel 集中承接 ACP session events。新的方向不再把 ACP 事件蒸馏成 Gold Band 自研 `progress.events.jsonl`，而是保留 ACP 统一返回值的语义，用于 Dialog / Chat UI 可视化。

## 3. 对 Gold Band 的借鉴方式

Gold Band 可以新增 ACP-only provider adapter，但不改变 runtime canonical model。

推荐结构：

```text
Gold Band runtime
  -> ProviderAdapter
    -> ClaudeAcpProvider
      -> ACP Rust client
        -> claude-agent-acp Node sidecar
          -> Claude Agent SDK
```

Gold Band 不再把 direct Claude Code CLI 作为与 ACP 并列的 provider 路径。Claude 相关能力统一表达为：

```text
claude-agent-acp / claude-acp
```

## 4. Gold Band 最小实现建议

### 4.1 Provider ID

建议新增 ACP provider：

```text
claude-agent-acp
```

或短别名：

```text
claude-acp
```

该 provider id 代表 Gold Band 通过 ACP stdio 连接 Claude adapter，不代表直接调用 Claude Code legacy CLI。

### 4.2 doctor() 检查

`doctor()` 至少检查：

- `claude-agent-acp` 是否在托管目录或 PATH 中
- 是否存在可用 package runner（例如 `pnpm` / `npx`）以解析 adapter
- `node` 是否可用
- Claude Agent SDK / Claude Code 认证是否可用
- 当前 workspace 是否可作为 ACP session cwd

### 4.3 runWorker() 流程

```text
1. resolve adapter binary / package runner
2. spawn stdio child process
3. initialize ACP client
4. session/new 或 session/load
5. PromptBundle -> ACP PromptRequest
6. session/update -> acp.events.jsonl / 会话详情 ViewModel
7. prompt response -> ProviderRunResult
8. ACP session id -> worker-ref.json
```

注意：这里不再新增 Gold Band 自研 `progress.events.jsonl` 映射层；ACP session events 本身就是 provider 返回值统一层。

### 4.4 worker-ref 映射

ACP session id 可以写入 Gold Band `worker-ref`：

```json
{
  "provider": "claude-agent-acp",
  "mode": "continue",
  "supportsOpenSession": true,
  "supportsContinueSession": true,
  "continueRef": {
    "acpSessionId": "..."
  }
}
```

后续继续执行时，优先尝试 `session/load`。

## 5. 不建议照搬的部分

### 5.1 不要默认开放全部 ACP client capabilities

Jockey 默认声明了文件读写与 terminal 能力。Gold Band 不应无条件全开：

```text
fs.read_text_file
fs.write_text_file
terminal
mcp
permission
```

这些能力必须经过 Gold Band runtime 权限边界，否则 agent 可能绕过 workflow / artifact contract。

### 5.2 不要照搬 Jockey 的 session 核心模型

Jockey 的核心模型是：

```text
app_session + role + runtime
```

Gold Band 的核心模型是：

```text
task + run + round + node + attempt + artifact
```

因此 Gold Band 只能借鉴它的 ACP adapter / sidecar 管理方式，不能让 ACP session 成为 canonical state。

### 5.3 连接池可以后置

Jockey 有连接池、预热、idle reclaim、prompt serialization、cancel handle、health watch 等完整机制。

Gold Band MVP 可以先一 attempt 一连接，跑通后再引入：

- provider prewarm
- connection reuse
- idle reclaim
- cwd change eviction
- prompt cancel
- process health watch

## 6. 与 ACP Dialog / Chat UI 的关系

Jockey 的前端事件桥可以作为 Gold Band Chat UI 的参考：

```text
ACP raw session/update
  -> Rust ACP client
  -> internal UI event model
  -> Tauri event bridge
  -> ACP Dialog / Chat UI
```

Gold Band UI 应将事件渲染为：

- streaming message bubble
- collapsible thought block
- tool call card
- permission request dialog / inline approval card
- plan block
- mode/config/session status
- raw frame diagnostics

详细 UI 规范见：

```text
docs/gold-band/开发计划/acp接入/acp-ui.md
```

## 7. 推荐结论

Gold Band 后续接入 Claude ACP 时，应采用 Jockey 的核心思路：

> Rust/Tauri 做 ACP client 和 runtime owner；Node 的 `claude-agent-acp` 作为 sidecar adapter；Claude Agent SDK 保持在 Node 世界；ACP 输出保留为 Dialog / Chat UI 的统一可视化输入，同时 Gold Band 继续维护自己的 task/run/round/node/attempt/artifact canonical state。

这能避免等待 Rust 版 Claude Agent SDK，也避免在 Rust 中嵌入 Node/Python runtime，同时保持 Gold Band provider-agnostic 的架构边界；也能避免 Claude Code legacy CLI fallback 带来的双路径状态不一致。