> ## Documentation Index
> Fetch the complete documentation index at: https://agentclientprotocol.com/llms.txt
> Use this file to discover all available pages before exploring further.

# Rust

## Gold Band 决策

Gold Band 将使用 Rust 侧 ACP client 接入 ACP-compatible agent adapter。Rust 负责 adapter 发现、stdio 进程管理、ACP session 生命周期、ACP 事件转发和 worker-ref 记录；Claude Agent SDK 等 provider-specific SDK 留在对应 adapter sidecar 中。

Gold Band 全面切换到 ACP，不再维护 Claude Code legacy CLI fallback、direct stream-json 可视化协议或供 UI 解析的 legacy terminal transcript。Rust 侧输出的会话数据应围绕 ACP session events、ACP raw frames、session metadata 和 adapter diagnostics 建模。

ACP 事件不再被蒸馏成 Gold Band 自研 `progress.events.jsonl`。后续会话详情直接围绕 ACP session events 建模和可视化，同时 Gold Band 继续使用 `run.json` / `round.json` / `node.json` / artifact contract 作为 runtime canonical state。

Rust 层职责边界（当前实现对应 `src/acp/*` 与 `src/provider/mod.rs`）：

- 发现并启动 ACP-compatible adapter。
- 管理 stdio child process 生命周期。
- 执行 ACP `initialize`、`session/new`、`session/load`、`session/prompt`、cancel、permission response。`session/request_permission` 的文件握手必须以 JSON-RPC 原始 request id 命名 `acp.permission-request.<id>.json` / `acp.permission-response.<id>.json`；timeline 展示层的 `permission-<id>` 不能回传给 runtime 等待逻辑。
- 持久化 synthetic `goldBandPrompt` 用户消息时保留 `promptId` 元数据；session event scan 只允许合并 `textDelta` / `thoughtDelta`，不得把不同轮次的 `userTextDelta` 拼接成一条消息。Tool/text/thought/plan/usage/config/mode/sessionInfo 都属于展示型或状态型 `session/update`，不创建 response 文件；只有 permission request 需要外部确认握手。
- 接收 `session/update` 并转发给会话详情 ViewModel。
- 由 ViewModel 扫描 `acp.events.jsonl` 计算 ACP session 累计净处理耗时；该耗时按 Gold Band prompt turn 累加，并扣除 `session/request_permission` pending 到用户选择之间的阻塞式用户决策等待区间。
- 记录 ACP session id、adapter、capabilities、stop reason、adapter 返回的 session config 快照（`models` / `modes` / `configOptions`）和诊断 metadata。
- 使用 `RuntimeConfig.acpAdapter` 配置 adapter command / args / displayName / env，默认命令为 `npx -y @agentclientprotocol/claude-agent-acp@latest`；Windows 运行时仅在启动进程前把 bare `npx` 映射为 `npx.cmd`。
- Windows 桌面端所有不需要用户交互的后台 CLI 子进程都必须通过统一进程工具启动；Rust 侧直接启动 ACP adapter、Git worktree 命令、MCP stdio 健康检查、MCP stdio `tools/list`、Windows Toast AUMID 注册或 shell fallback 时，必须复用 `background_command()` 以应用 `CREATE_NO_WINDOW`，避免 Win10 出现短暂 `git.exe` / `cmd.exe` / `reg.exe` / PowerShell 控制台窗口。
- 不解析 Claude Code CLI 文本输出。
- 不从 terminal transcript 推导 UI 状态。
- 不让 ACP session 替代 Gold Band 的 run / round / node / artifact canonical state。

**ACP session-forward 元数据水合策略**（2026-06-15，2026-06-17 更新）：

每条 worker / AI-DYNAMIC 内部 node attempt 启动时，首个对外可见 `AcpSessionVm` 必须是 session-ready 快照：ACP `session/new` 或 `session/load` 已完成，model/mode/configOptions 已捕获，system prompt 已写入 snapshot metadata，Gold Band synthetic user prompt 已写入 timeline。随后才发送真实 `session/prompt` 并开始流式输出，确保前端实时会话窗口不会先看到 agent thinking 而缺失用户消息、系统提示词按钮和模型/权限选择器。

工作原理：

- provider trait 使用 `run_worker_with_callbacks`，同时接收 `live_update`（单条 timeline event）和 `session_update`（完整 session snapshot）。
- `run_prompt` 调用方通过 `app.acp_session_update_for(context)` 创建 session-ready callback，其背后复用已存在的 `acp_session_update_emitter`。
- `run_prompt` 的启动顺序固定为：`setup_session` → `write_worker_ref` → 持久化 synthetic `goldBandPrompt` 用户消息（有 `session_update` 时不单独发 live event）→ `write_session("running")` → `session_update` → 真实 `session/prompt`。
- `AcpSessionMetadata` 直接保存 `systemPromptAppend`；`view_models` 优先读 snapshot 字段，旧历史 session 才 fallback 到 raw frame 的 `_meta.systemPrompt.append`。
- 动态节点调用同样传入 `outerNodeId / outerAttemptId`，由 `acp_session_update_emitter` 自动选择 `dynamic_acp_session_vm`。
- 用户手动发送 prompt 路径继续在完成后发送 session snapshot，不在此阶段重复。
- ACP session 终态（completed / cancelled / failed）写入 `acp.snapshot.json` 后立即发送一次 `session_update`，确保前端即使错过 running 阶段的 update 也能拿到最终的 session 状态、用量和事件列表。
- 前端兜底：live event 到达时若 base session 缺元数据或缺首个 Gold Band 用户消息，触发 `getAcpSession` hydration；session 等价判断需比较 config + adapter 元数据签名。模型/权限配置是否可展示以 options 是否存在为准，不强依赖 current id。

---

## 官方 Rust SDK 摘录

> Rust library for the Agent Client Protocol

The [agent-client-protocol](https://crates.io/crates/agent-client-protocol) Rust
crate provides implementations of both sides of the Agent Client Protocol that
you can use to build your own agent server or client.

To get started, add the crate as a dependency to your project's `Cargo.toml`:

```bash theme={null}
cargo add agent-client-protocol
```

Depending on what kind of tool you're building, you'll need to implement either
the
[Agent](https://docs.rs/agent-client-protocol/latest/agent_client_protocol/trait.Agent.html)
trait or the
[Client](https://docs.rs/agent-client-protocol/latest/agent_client_protocol/trait.Client.html)
trait to define the interaction with the ACP counterpart.

The
[agent](https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol/examples/agent.rs)
and
[client](https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol/examples/client.rs)
example binaries provide runnable examples of how to do this, which you can use
as a starting point.

You can read the full documentation for the `agent-client-protocol` crate on
[docs.rs](https://docs.rs/agent-client-protocol/latest/agent_client_protocol/).

## Users

The `agent-client-protocol` crate powers the integration with external agents in
the [Zed](https://zed.dev) editor.
