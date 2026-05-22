# ACP-first Agent 会话可视化重构计划

## 1. 核心决策

Gold Band 后续不再自研 `progress.events.jsonl` 作为 provider 输出的中间事件协议，也不再保留 Claude Code legacy CLI 作为运行时 fallback。

新的方向是：**Gold Band 全面切换到 ACP，通过 ACP 协议调用 agent/provider，直接获取 ACP 统一后的 agent 会话事件，再基于 ACP 会话事件做可视化。**

```text
Gold Band Runtime
  -> ACP Client
    -> ACP-compatible Agent Adapter
      -> Claude Agent SDK / Codex / Gemini / 其他 agent backend
    <- ACP SessionUpdate / ToolCall / Plan / Permission / Error
  -> Gold Band Session Dialog / Chat UI
```

这次重构的目标不是把 provider 输出“蒸馏”为 Gold Band 自己的一套 progress event，也不是在 ACP 和 Claude Code legacy CLI 之间做双路径兼容，而是让用户在 Gold Band 会话详情中直观看到 ACP 统一后的原始 agent 过程：文本流、思考流、工具调用、计划、权限请求、terminal/file 操作和错误。

## 2. 为什么废弃自研 progress.events.jsonl 与 legacy CLI 路径

原设计中：

```text
provider raw stream
  -> Gold Band progress.events.jsonl
  -> UI visualization
```

问题是：

1. 每个 provider 的 stream 都要写一套 mapper。
2. Claude Code direct stream-json 与 ACP agent event 会形成两套返回语义。
3. 可视化层会长期背负 provider-specific 兼容逻辑。
4. Gold Band 蒸馏后的 progress event 会丢失 agent 原始交互细节，用户很难判断 agent 实际做了什么。
5. legacy CLI fallback 会让 runtime、UI、排障链路长期存在两套真实来源，增加状态不一致风险。

新设计改为：

```text
ACP-compatible provider output
  -> ACP unified session events
  -> Gold Band Dialog / Chat UI
```

也就是说，统一返回值的职责交给 ACP。Gold Band 只做 ACP client、session 生命周期管理、会话视图模型整理和 runtime canonical state 维护。

## 3. 新边界定义

### 3.1 ACP 是唯一 provider 接入协议

ACP 在 Gold Band 中承担：

- agent 调用协议
- session 生命周期协议
- agent 过程事件协议
- 工具调用 / 权限 / terminal / file 操作协议
- 会话恢复协议

ACP 不承担：

- Gold Band workflow 控制流
- run / round / node 生命周期判断
- artifact contract 校验
- acceptance loop 决策

Claude Code legacy CLI、direct stream-json、terminal transcript parser 不再作为 Gold Band 的运行时接入路径。若仓库中仍存在这些能力，应归入迁移清理范围，而不是作为新能力的兜底方案。

### 3.2 Gold Band canonical state 仍然保留

以下文件仍是 Gold Band runtime 的权威状态，并统一存放在 `~/.gold-band/projects/{project-id}/tasks/...`，不写入项目工作树：

```text
run.json
round.json
node.json
worker-ref.json
artifact files
```

ACP 会话事件用于可视化与排障，不直接决定：

- node outcome
- run outcome
- edge routing
- artifact 是否有效

### 3.3 会话详情替代 progress.events 可视化

原来的 attempt 级 `progress.events.jsonl` 不再作为新增设计目标。

新的 attempt 会话详情数据来源：

```text
ACP session events
ACP raw frames / raw transcript
worker-ref ACP session id
adapter/session diagnostic metadata
```

UI 以 Dialog / Chat UI 展示 ACP 会话过程，而不是以 Gold Band 自定义 progress event timeline 展示。排障应优先查看 ACP raw frame、session event、session metadata 和 adapter 日志，不通过 Claude Code legacy CLI 输出还原状态。

## 4. 目标架构

```text
Provider Layer
  - claude-agent-acp
  - codex-acp
  - gemini ACP mode
  - future ACP-compatible agents

ACP Client Layer
  - resolve adapter
  - spawn stdio child
  - initialize
  - session/new or session/load
  - session/prompt
  - session/update stream
  - cancel
  - permission response

Gold Band Runtime Layer
  - task / run / round / node / attempt
  - workflow control
  - artifact validation
  - worker-ref

Session Dialog / Chat UI Layer
  - chat composer
  - streaming message bubbles
  - thought blocks
  - tool call cards
  - tool call updates
  - plan blocks
  - permission requests
  - mode/config/session status
  - terminal/file events
  - errors and stop reason
  - raw frame viewer
```

## 5. Provider 策略

### 5.1 默认方向

后续多 provider 接入只面向 ACP-compatible adapter：

```text
claude-agent-acp
codex-acp
gemini ACP mode
```

不再为每个 agent 长期维护独立可视化返回协议，也不再为 Claude Code direct CLI 维护独立运行路径。

### 5.2 Claude 接入方式

Claude 接入统一使用：

```text
claude-agent-acp
```

它内部可以使用 Claude Agent SDK / Claude Code 能力，但 Gold Band 只通过 ACP stdio 与 adapter 交互。Claude Code legacy CLI 仅作为历史实现和待清理对象，不作为 fallback/debug/continue/open-session 的运行目标。

## 6. 借鉴 Jockey 的地方

Jockey 证明了该路径可行：

```text
Rust / Tauri host
  -> ACP Rust client
    -> Node sidecar adapter
      -> Claude Agent SDK
```

Gold Band 可借鉴：

1. adapter 解析策略：managed binary -> PATH binary -> package runner。
2. Rust 使用 `agent_client_protocol` crate 创建 `ClientSideConnection`。
3. cold start：spawn stdio adapter，执行 `initialize`。
4. session 生命周期：优先 `session/load`，不可恢复时创建 `session/new`。
5. `session/update` 转为宿主会话视图事件。
6. Agent 管理页诊断时缓存 `agentCapabilities` 里的 `modes` / `configOptions`，并把可选权限模式持久化到当前 workspace 运行时目录。
7. workflow worker 节点支持保存 `permission_mode`，运行时在 `session/new` / `session/load` 后优先通过 `session/set_config_option(configId=mode)`，兼容旧版 `session/set_mode` 应用该模式。
8. ACP 事件先归一化为 UI event model，再推送到前端。

参考文档：

- `docs/gold-band/开发计划/acp接入/jockey-claude-agent-sdk-bridge.md`
- `docs/gold-band/开发计划/acp接入/acp-ui.md`

但 Gold Band 不照搬 Jockey 的核心 session 模型。Jockey 是：

```text
app_session + role + runtime
```

Gold Band 是：

```text
task + run + round + node + attempt
```

## 7. ACP Dialog / Chat UI 方向

会话详情应使用 ACP Dialog / Chat UI，而不是复用旧 progress.events 面板或 terminal/log 输出：

- 用户输入：通过 chat composer 提交，发送下一次 ACP `session/prompt`。
- 文本流：以 agent message bubble 流式展示。
- thought/reasoning：可折叠，默认弱化。
- tool call：卡片化展示工具名、状态、输入、输出、关联文件位置。
- tool call update：原卡片内更新，避免刷屏。
- plan：独立 plan block，展示 agent 的当前计划和完成状态。
- permission request：作为可操作事件展示，接入 Gold Band 权限策略。
- terminal/file：作为工具调用或能力事件的结构化详情展示，不混入普通文本输出。
- raw：提供原始 ACP frame / transcript 查看入口，仅用于排障。

详细 UI 规范见：`docs/gold-band/开发计划/acp接入/acp-ui.md`。

## 8. Agent 提问 / 用户回答能力

ACP 标准里明确有 agent 反向请求用户决策的能力：

```text
Agent -> Client: session/request_permission
Client -> Agent: RequestPermissionResponse
```

它适合表达：

- 是否允许执行某个 tool call
- 是否允许写文件 / 运行命令
- 在多个 permission option 中选择 allow/reject/always 等决策

这是 Gold Band 必须接入的 human-in-the-loop 能力之一。Jockey 也把它转成 `PermissionRequest` 事件，UI 再让用户选择。

对于“agent 问一个自由文本问题，用户输入答案”这类澄清式交互，ACP 当前主流程更接近：

```text
Agent -> Client: session/update(agent message: question)
Agent -> Client: session/prompt response(stopReason=end_turn)
User -> Client: 在 chat composer 输入答案
Client -> Agent: 下一次 session/prompt(answer)
```

也就是说，自由文本问答通常不是同一个 prompt turn 内的阻塞 RPC，而是 agent 以普通消息提出问题并结束 turn，client 再把用户回答作为下一轮 prompt 发回。Gold Band 需要在 UI 和 runtime 上把这种情况表达成：

```text
node paused / waiting_for_user_input
  -> Dialog / Chat UI 展示 agent question
  -> 用户输入 answer
  -> run continue 发送下一次 ACP session/prompt
```

如果后续 ACP 增加通用 elicitation / input request 扩展，Gold Band 可以在 ACP client 层接入；在此之前，权限类问题走 `session/request_permission`，自由文本澄清走“agent message + next prompt”。

## 9. 落盘建议

新增或重构 attempt 级会话目录：

```text
attempt/
  acp.session.json
  acp.events.jsonl
  acp.raw.jsonl
  worker-ref.json
  artifacts/
  attachments/
```

说明：

- `acp.raw.jsonl`：ACP 原始 frame，供排障。
- `acp.events.jsonl`：ACP session/update 级事件，尽量保持 ACP 语义，不再改造成 Gold Band progress event。
- `acp.session.json`：session id、adapter、capabilities、stop reason、model/config/mode 摘要。
- `worker-ref.json`：继续承载 ACP session id、continue 信息和必要的诊断 metadata。

如果后续发现 `acp.events.jsonl` 与 raw frame 重复，可再收敛；当前建议先分层，便于 UI 和排障。

## 10. 功能模块拆分

ACP 接入任务不再按“阶段 1 / 阶段 2”组织，而是按互不影响、可单独认领的功能模块拆分。详细 todo 见：

`docs/gold-band/开发计划/acp接入/acp功能模块todo列表.md`

核心模块包括：

1. ACP adapter 解析与启动模块
2. ACP session 生命周期模块
3. ACP 事件归一化模块
4. Chat Dialog 容器模块
5. Chat Composer 用户输入模块
6. 流式文本消息渲染模块
7. ThoughtBlock 思考内容模块
8. ToolCallCard 工具调用模块
9. PermissionRequest 权限请求模块
10. Plan / Mode / Config 状态模块
11. SessionInfo / 会话恢复模块
12. Raw frame / 诊断模块
13. Legacy CLI 清理模块
14. 集成验收模块

每个模块都需要明确目标、输入、输出、主要任务、不做什么、验收标准和相关文档链接。

## 11. 一句话总结

> Gold Band 这次 ACP 重构的核心是：全面切换到 ACP，不再保留 Claude Code legacy fallback；Gold Band 用 ACP 统一 agent 返回值，并用 Dialog / Chat UI 展示 ACP 会话事件，同时继续维护自己的 runtime canonical state、artifact contract 和 workflow 控制。