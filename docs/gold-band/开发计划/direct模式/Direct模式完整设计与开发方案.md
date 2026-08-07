# Gold Band Direct 模式完整设计与开发方案

## 1. 背景

Gold Band 当前会话式首页提供两种处理模式：

- `WORKFLOW`：使用用户选择的工作流模板执行任务。
- `AUTO`：由 AI-DYNAMIC 根据目标动态生成和推进内部工作流。

两种模式都以 Gold Band runtime 为核心，用户输入会被包装为 workflow 节点调用，并注入 runtime system prompt、hidden runtime context、profile、输出协议和修复协议等上下文。

现在需要新增 `DIRECT` 模式。Direct 模式的产品目标不是创建一个由用户感知的单节点工作流，而是：

> 用户选择一个 ACP Agent，像直接使用该 Agent 自身一样持续对话；Gold Band 只负责统一的桌面 UI、会话存储、流式展示、附件、权限、停止、异常恢复、耗时和 token 统计，不向 Agent 注入任何 Gold Band runtime system prompt。

Direct 模式会复用现有 ACP session、timeline、长连接、模型/权限配置和 prompt-kit 会话界面，但必须修复当前“completed workflow 上继续追问时，页面切换后生命周期丢失”的基础问题。否则 Direct 首轮结束后的每次追问都会出现：Agent 仍在输出，但 composer 已恢复空闲、停止按钮消失、计时停止或 session tree 不再显示活跃状态。

## 2. 问题定性

### 2.1 不是单纯新增第三个 Tab

Direct 与 WORKFLOW / AUTO 的差异不仅是 prompt 内容不同，还包括用户心智、状态展示、会话排序和侧边栏身份表达：

| 维度 | DIRECT | WORKFLOW | AUTO |
|---|---|---|---|
| 用户心智 | 与指定 Agent 持续对话 | 执行明确工作流 | 由 AI 动态编排任务 |
| Agent 选择 | 用户直接选择单个 Agent | 由工作流节点定义 | 固定或动态 Agent 策略 |
| System prompt | 空 | Gold Band runtime prompt | Gold Band AI-DYNAMIC prompt |
| 生命周期展示 | 当前对话 turn | run / round / node / attempt | outer runtime + dynamic graph |
| 自然回复结束 | 回到可继续输入 | 节点完成并推进工作流 | 动态节点完成并继续路由 |
| 侧边栏左侧标识 | Agent icon | runtime 状态点 | runtime 状态点 |
| 成功/失败终态 | 不作为会话主状态 | 必须展示 | 必须展示 |
| 工作流入口 | 隐藏 | 查看/编辑 | 查看动态运行图 |

因此不能在前端简单增加 `mode === "direct"` 后直接调用 `send_acp_prompt`，也不能复制一套独立的 Direct session 存储和 UI。

### 2.2 当前生命周期存在根本缺陷

当前 completed run 上的普通追问已经是独立 ACP prompt：

1. `submit_conversation_prompt` 判断 runtime 不需要继续后，进入 `send_acp_prompt`。
2. Tauri blocking task 调用 `client::run_prompt`。
3. ACP session metadata 写为 `running`，持续写 timeline、timing、usage。
4. prompt 完成后写为 `completed | failed | cancelled`。

但 `derive_conversation_attempt_lifecycle` 在 runtime terminal 且不可 runtime continue 时，会无条件压制 ACP metadata 中的 active 状态，用来规避崩溃残留的 stale `running` snapshot。这导致后端无法区分：

- 磁盘上残留但实际上已经失效的 `running` snapshot。
- completed run 上正在真实执行的 same-session follow-up turn。

前端的 `awaitingResponse`、`activeTurnPrompt`、`activeTurnPromptId` 又主要属于 `ACPChatDialog` 组件局部状态。切换 session 或页面导致组件卸载后，已经被后端接受并从 optimistic store 清除的 prompt 无法重新恢复为 active turn。

这违反现有生命周期设计原则：

- 后端 lifecycle/composer 应是唯一业务规则源。
- 前端只能保留极短暂的发送、停止命令和 optimistic overlay。
- session tree、composer、workflow graph、计时和停止入口必须消费同一份生命周期事实。

Direct 模式必须先修复该缺陷，不能通过 Direct 专用前端布尔值绕过。

### 2.3 历史回放与中断消息污染

`session/load` 的 JSON-RPC response 只表示 load request 已被接受，不表示 provider 已经发送完全部历史 `session/update`。如果在 response 返回时立即结束抑制，迟到的历史 assistant/tool patch 会被当成本轮新输出，造成消息重复、顺序跳到末尾；provider 回放的 `user_message_chunk` 还可能把原 prompt 或 `[Request interrupted by user...]` 控制文本伪装成新的用户消息。

Direct 与普通 ACP follow-up 共用以下修复：

- Gold Band synthetic `goldBandPrompt` 是本地输入事实；恢复共享 Provider session 时，`user_message_chunk` 与本地 prompt 按有序锚点对账，Provider 漏回放部分本地 turn 时跳过缺失锚点并继续识别后续本地回显。完全匹配不到剩余锚点的外部客户端 turn 连同其 user/assistant/thought/tool 历史先暂存，遇到下一个本地 prompt 后整组标记为插入该 prompt 之前；尾部历史在 load finish 时只锚定到最后一个已匹配本地 prompt。Claude 两条已知 interruption 控制文本暂由 Provider 归一化层精确隐藏并保留 raw frame，等待 ACP 提供结构化 control 标志后替换。
- restored session 维护 `Replaying -> AwaitingTurnStart -> Live` phase。`session/load` response 后不能立即退出 `Replaying`，必须由 event pump 等待回放队列达到有界静默，并在真实 prompt 前再次确认静默；超时则终止本轮，不允许历史和当前 turn 混流。外部同步关闭时静默窗口内所有 replay content 只写 raw，开启时才进入历史 importer；稳定 message/tool identity 仅作为极迟到历史的后续保险，不能承担回放结束判定。
- prompt 状态区分 `Accepted`（用户消息已持久化）与 `Running`（provider prompt 已发出），composer 以 durable prompt identity 结束“发送中”。
- Stop 返回后先合并最终 session，只删除仍未接受的 optimistic prompt；已持久化消息在当前 UI 与重启后必须一致。
- Provider history 使用独立逻辑位置结构：`historyPlacement.version/afterPromptId/beforePromptId/gapTurnIndex` 描述本地 prompt 间隙，`historyItemIndex` 描述组内顺序；`seq/timestamp` 继续作为不可改写的审计到达顺序。后端投影和前端 merge 按锚点构建展示顺序，分页 cursor 仍取窗口审计 seq 的 min/max。重复 load 对同一稳定 ID 只补写 placement patch，并保留首次时间位置；旧版本已污染的 timeline 仅在缺少 placement 时按文本锚点清理误分类整组，Provider-history patch 与既有本地 item identity 冲突时保留原始本地事件和工具卡片，不重写 raw/timeline 审计数据。

## 3. 设计目标

### 3.1 产品目标

- 快速会话首页增加 `DIRECT / 工作流 / AUTO` 三个并列模式，顺序固定为 Direct、工作流、AUTO。
- Direct 模式允许用户通过 Agent icon 列表快速选择 Agent。
- 选中 Agent 后恢复该 workspace 下这个 Agent 上次使用的模型和权限模式。
- 模型和权限模式放在输入框右下角，与发送按钮形成同一组紧凑控制。
- Direct 会话创建后持续复用同一个 ACP session，不向用户展示 workflow 成功、暂停或节点终态心智。
- Direct 会话侧边栏使用 Agent icon 表达身份，不使用绿点、黄点、红点表达 run 结果。
- 页面切换、session 切换后，正在执行的 Direct turn 必须恢复发送/处理/思考/工具/停止状态、耗时和 token。

### 3.2 技术目标

- 不新增第二套 ACP client、connection manager、timeline、usage 或 permission 实现。
- 不新增 Direct 专用消息容器、输入框、Markdown 或工具卡片。
- 复用现有 prompt-kit copy-in 组件和 shadcn/ui 控件。
- Direct 首轮和后续追问均保证 `system_prompt == ""`。
- Direct 首轮用户文本和后续用户文本都不注入 hidden runtime context、profile、goal、output protocol 或 repair prompt。
- 继续复用现有附件、模型、权限、MCP server、session config、token、耗时和 raw frame 管道。
- `submit_conversation_prompt` 与 `stop_active_session` 仍是会话态统一入口，前端不维护 Direct 专用发送/停止分支。
- 错误使用结构化 code + params 管理，前端根据 code 做 i18n，不在后端生成对客文案。

### 3.3 非目标

- 不让 Direct Agent 在同一会话中途切换为另一个 Agent。
- 不增加 Agent 多选、Agent 自动路由或 Agent 间 handoff。
- 不把 Direct prompt mode 暴露为普通工作流编辑器中的用户配置项。
- 不按 Agent 在侧边栏增加第二层分组；首版仍按 workspace 分组。
- 不新增第三方状态管理库、聊天 UI 库或 ACP SDK。
- 不让正在执行的 prompt 跨应用进程重启继续运行。应用重启后不存在 live provider activity，必须收敛为 interrupted/idle，而不是继续显示思考中。

## 4. 总体架构

Direct 在产品层是持续 Agent 会话，在存储和执行层复用现有 task/run/round/node/attempt 容器。

推荐结构：

```text
Conversation UI
  └─ runMode = direct
      ├─ directConfig: agent/model/permission
      ├─ internal single worker execution shell
      ├─ promptEnvelope = raw-agent
      └─ existing ACP session/timeline/usage/timing
```

内部仍生成一个单 Worker 的执行壳：

```text
direct-agent -> $end
```

该壳只用于复用：

- task/run 历史和路径布局
- attempt locator
- ACP session 文件
- timeline / raw / diagnostics
- 附件目录
- SQLite session 索引
- Tauri command 参数
- 停止和 crash recovery 基础设施

Direct UI 不展示该内部工作流，不展示 node path、workflow graph、节点成功或 run completed 横幅。

## 5. 核心数据结构

### 5.1 使用枚举替代运行模式字符串

当前运行模式主要使用简单字符串。新增 Direct 时应统一收敛为可序列化枚举，避免各层继续增加 `mode === "..."` 硬编码。

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationRunMode {
    Direct,
    Workflow,
    Auto,
}
```

TypeScript 对应：

```ts
export type ConversationRunMode = 'direct' | 'workflow' | 'auto';
```

### 5.2 Direct 配置

Direct 不复用 `ConversationAutoConfig`。AUTO 的 agent strategy、routing prompt、global goal、allowed workflows、profiles 和 dynamic control 都不属于 Direct 领域。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDirectConfig {
    pub agent_type: String,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
}
```

TypeScript：

```ts
export interface ConversationDirectConfigVm {
  agentType: string;
  modelId?: string | null;
  permissionMode?: string | null;
}
```

以下结构同步增加 `directConfig`：

- `ConversationRunModeEntry`
- `ConversationRunModeSettingsVm`
- `ConversationRunModeVm`
- `ConversationCreateInputVm`
- `ConversationCreateInput`
- `ConversationRunVm`

### 5.3 Prompt envelope

Direct 的关键不是空 profile，而是明确的数据级 prompt envelope。

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PromptEnvelopeMode {
    RuntimeManaged,
    RawAgent,
}
```

推荐在 `WorkerNode` / frozen workflow snapshot 中持久化该字段，但不在普通 WorkflowEditor 中暴露：

```rust
#[serde(default, skip_serializing_if = "is_runtime_managed_prompt")]
pub prompt_envelope: PromptEnvelopeMode,
```

旧工作流和普通作者态节点默认为 `RuntimeManaged`。Direct 创建流程生成的内部节点使用 `RawAgent`。

这样可以保证：

- run snapshot 自包含，不依赖之后可能变化的 task metadata。
- 重跑仍能精确复用 Direct prompt 语义。
- 普通 worker、AI-DYNAMIC 和 Direct 不在 renderer 中通过 node id 或 workflow id 猜测行为。
- 不使用特殊 profile、特殊 goal 或硬编码字符串模拟 Direct。

### 5.4 ACP prompt activity

当前 ACP metadata status 不能独立判断是否存在真实 live prompt。需要把已有 per-attempt provider control 状态以只读方式暴露给 lifecycle 派生层。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpPromptActivity {
    Starting,
    Running,
    CancelRequested,
}

pub fn prompt_activity(attempt_dir: &Utf8Path) -> Option<AcpPromptActivity>;
```

语义：

- `None`：当前进程中不存在该 attempt 的 live ACP prompt。
- `Starting`：命令已被后端接受，正在创建/恢复 session 或准备 prompt。
- `Running`：`session/prompt` 已发出并等待 response。
- `CancelRequested`：用户已请求停止，正在 drain prompt/cancel 结果。

Provider control 在 terminal session update 发出前必须先标记 prompt finished，避免 final update 被短暂识别为仍 active。

### 5.5 ACP lifecycle facet

建议给 `ConversationAcpFacetVm` 增加明确 phase，避免前端从 status 字符串推断：

```rust
pub struct ConversationAcpFacetVm {
    pub status: Option<String>,
    pub phase: String,
    pub active: bool,
    pub stopping: bool,
    pub terminal: bool,
}
```

`phase`：

```text
idle | starting | prompting | stopping | terminal
```

### 5.6 Direct workspace 记忆

模型和权限记忆必须按 workspace + agent 管理：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDirectWorkspacePreference {
    pub last_agent_type: Option<String>,
    pub agents: HashMap<String, ConversationDirectAgentPreference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDirectAgentPreference {
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
}
```

存入 `StateConfig`：

```rust
pub conversation_direct_preferences:
    HashMap<String, ConversationDirectWorkspacePreference>;
```

外层 key 是 `projectId`，内层 key 是 `agentType`。

### 5.7 Direct Agent 身份

侧边栏、置顶列表和搜索结果需要稳定的 Agent 身份，不能在 JSX 中临时扫描 workflow 或根据标题猜测。

```rust
pub struct ConversationAgentIdentityVm {
    pub agent_type: String,
    pub display_name: String,
    pub icon_key: String,
}
```

`ConversationTaskRowVm`、搜索结果和必要的 run header VM 增加：

```text
agent: Option<ConversationAgentIdentityVm>
```

只对 Direct 会话返回。解析优先级：

1. 当前 Agent registry 中相同 `agentType` 的实时 metadata。
2. `conversation.json` 中创建时保存的 identity snapshot。
3. 未识别 Agent 使用通用 Bot icon 和 agentType 文本。

### 5.8 Direct 最近活动时间

Direct 是持续会话，不能继续只使用首次 run 的启动或完成时间排序。

`conversation.json` 增加：

```text
lastActivityAt
```

以下事件更新该字段：

- Direct 用户 prompt 被后端接受。
- Direct Agent 产生首个可见响应事件。
- Direct turn 正常结束、停止或失败。

侧边栏中：

- Direct 使用 `lastActivityAt` 排序和显示相对时间。
- Workflow / AUTO 继续使用最新 run 时间规则。

为了避免每个流式 delta 都写磁盘，`lastActivityAt` 只在状态边界更新，不随每个 token 更新。

## 6. Direct prompt 语义

### 6.1 首轮新 session

Direct 首轮必须满足：

```text
system_prompt = ""
user_prompt = 用户原始输入
visibility = visible
```

禁止包含：

- `src/prompts/*/runtime/system.md`
- hidden runtime context
- requirement/goal/task 模板包装
- profile 内容
- predecessor 上下文
- output contract
- invalid output repair prompt
- AI-DYNAMIC prompt
- Gold Band stable system prompt hidden block

允许继续传递：

- workspace cwd
- 用户本轮附件
- 用户选择的 model
- 用户选择的 permission mode
- Gold Band 已启用且健康的 ACP MCP servers

用户明确要求的是“不提供多余 system prompt”，不是禁用模型、权限、附件或 MCP 配置。

### 6.2 same-session 后续追问

Direct 后续输入必须：

```text
session_mode = continue
continue_ref = 原 acpSessionId
system_prompt = ""
user_prompt = 本轮用户原文
```

禁止重新附带：

- 初始任务输入文本
- 历史附件
- Gold Band runtime hidden context
- stable system prompt

只带本轮新增附件。

### 6.3 renderer 规则

`render_prompt_bundle` 根据 `PromptEnvelopeMode` 分支：

```text
RuntimeManaged
  -> 现有 render_system_prompt
  -> 现有 render_user_prompt

RawAgent
  -> system_prompt = ""
  -> new session: requirement 原文
  -> continue: resume_prompt 原文
```

Direct 输入不能在 renderer 中无条件 `trim()` 后改变用户原文。发送前只用 `trim().is_empty()` 判断是否为空；真正传给 Agent 的非空正文保留原始空格和换行。

## 7. 生命周期统一修复

### 7.1 权威来源

生命周期派生输入由以下事实组成：

- runtime state：run / round / node / dynamic graph。
- ACP persisted metadata：snapshot/session status、timing、usage、diagnostics。
- ACP live activity：per-attempt provider control。
- pending interaction：permission / elicitation。

前端 local state 不属于业务事实，只属于短暂 UI overlay。

### 7.2 stale active 与真实 follow-up 区分

核心矩阵：

| Runtime | ACP metadata | Live activity | Lifecycle 结果 |
|---|---|---|---|
| completed | running | Running | `acp.active=true`，composer 锁定，可停止 |
| completed | running | Starting | `acp.active=true`，显示拉起/发送中 |
| completed | running | None | stale snapshot，runtime terminal |
| completed | completed | None | 空闲，可继续追问 |
| completed | failed | None | 空闲但展示本轮错误，可重试 |
| completed | cancelled | None | 空闲，可继续追问 |
| 任意 | 任意 | CancelRequested | `stopping=true`，锁定输入 |
| running | completed | None | runtime 继续推进，`launching-next-node` |
| running | running | Running | runtime + ACP active |

原有“runtime terminal 无条件 suppress ACP active”测试必须拆分为：

- 没有 live activity 时压制 stale metadata。
- 存在 live activity 时保留 completed-run follow-up active。

### 7.3 composer 派生

后端继续输出统一 composer decision：

```text
permission blocked
  > stopping
  > submitting / starting
  > runtime active
  > ACP prompt active
  > invalid workflow
  > runtime error
  > runtime continue input
  > normal prompt
```

Direct 不使用 workflow invalid 和 launching-next-node 对客文案，但底层 lifecycle 仍可保留完整状态用于诊断。

Direct composer 展示映射：

| ACP phase | Composer |
|---|---|
| starting | 发送中 / 正在启动 Agent |
| prompting + thought | 思考中 |
| prompting + tool | 正在执行工具 |
| prompting + text | 正在回复 |
| permission pending | 等待权限选择 |
| elicitation pending | 等待用户输入 |
| stopping | 正在停止当前回复 |
| terminal / idle | 可输入 |

### 7.4 页面切换恢复

重新进入 Direct 会话时，不依赖旧组件的 `awaitingResponse`：

1. `get_conversation_run` 返回 selected session 和 lifecycle。
2. lifecycle 从 provider control 恢复 live activity。
3. session VM 从 timeline 恢复最新 thought/tool/text 状态。
4. timing 从 session metadata / timing event 恢复累计秒数。
5. usage 从 session metadata / usage events 恢复 token。
6. composer 根据共享后端 lifecycle 显示状态与停止按钮；Direct 依据显式 queue capability 保持输入可用并将后续提交入队，Workflow/AUTO 按 active lifecycle 锁定输入。

前端 `awaitingResponse` 只负责当前组件内从点击发送到后端 activity 可见之间的短暂覆盖。

### 7.5 应用关闭与崩溃

- 正常关闭：复用 `stop_all_running_sessions + cancel_all_active_acp_attempts`，包括 completed run 上的 Direct follow-up。
- Direct 首轮仍由 runtime interruption 收敛。
- Direct completed-run follow-up 只取消当前 ACP prompt，不把 completed run 改为 paused。
- 崩溃重启后没有 live provider activity，即使 snapshot 残留 `running`，也不能恢复“思考中”。
- 若 raw/session 文件能确认 interrupted/cancelled，则展示对应历史终态；否则按 stale active fuse 收敛为 idle/terminal，并保留 diagnostics。

## 8. 停止、继续与异常处理

### 8.1 统一入口

前端始终调用：

- 发送：`submit_conversation_prompt`
- 停止：`stop_active_session`

Direct 不增加：

- `submit_direct_prompt`
- `stop_direct_session`
- `continue_direct_session`

后端根据 locator、runtime lifecycle 和 Direct metadata 决定内部路径。

### 8.2 Direct 首轮停止

Direct 首轮仍处于内部单 Worker runtime：

1. 用户点击停止。
2. `stop_active_session` 把当前 runtime attempt 写为 `paused + process-interrupted`。
3. provider control 进入 `CancelRequested`。
4. 发送 `session/cancel` 并 drain 当前 prompt。
5. snapshot 写为 `cancelled`。
6. composer 恢复可输入。
7. 下一条文本走 `runtime-continue`，复用原 ACP session。

用户只感知“当前回复已停止，可以继续发消息”，不展示节点暂停概念。

### 8.3 首轮自然完成后的追问

内部 run 已 completed，下一条文本走 same-session ACP prompt：

- run 保持 completed。
- lifecycle 的 ACP facet 进入 active。
- sidebar 不显示 run running/paused 状态。
- 停止只取消当前 ACP turn。
- 停止后下一条消息继续复用相同 session。

### 8.4 Provider / transport 异常

Direct turn 异常不等价于整个会话失败。

推荐持久化结构化错误：

```rust
pub struct AcpTurnError {
    pub code: String,
    pub params: serde_json::Value,
    pub diagnostic: Option<String>,
}
```

约束：

- `code` 用于前端 i18n。
- `params` 提供 Agent、模型、workspace、阶段等结构化上下文。
- `diagnostic` 只用于诊断详情，不直接作为对客文案。
- 后端不返回硬编码中文/英文错误句子。

异常后：

- 当前 turn terminal。
- 输入框恢复。
- 会话内展示错误和重试入口。
- 默认重试相同 ACP session。
- session 无法 load 时返回明确结构化错误；是否提供“新建 Direct 会话”由后续产品阶段决定，不在本期自动 fallback。

### 8.5 权限和 elicitation

继续复用现有 ACP permission / elicitation durable lifecycle：

- pending 状态由 timeline/session 恢复。
- 页面切换后 composer 保持锁定。
- 停止时写 cancelled response 和 terminal timeline event。
- 已 selected/completed 的请求不会重新弹出。

Direct 不增加专用权限卡片或问答卡片。

## 9. 快速会话 UI

### 9.1 信息层级

保持当前页面顺序：

```text
输入框
处理模式
模式配置
```

模式顺序固定：

```text
DIRECT | 工作流 | AUTO
```

Direct 选中后的布局：

```text
┌──────────────────────────────────────────────┐
│ 输入你的需求……                               │
│                                              │
│ 📎  Workspace            模型 ▾  权限 ▾  发送 │
└──────────────────────────────────────────────┘

处理模式  [ DIRECT ] [ 工作流 ] [ AUTO ]

Agent     [ Claude Code ] [Codex] [Gemini] […] [+]
```

### 9.2 Agent icon picker

优先复用已有：

- `AgentRegistryVm`
- `ManagedAgentVm.iconKey`
- `agentIconSrc()`
- `agentIconClass()`
- `selectableAgentOptions()`

交互规则：

- 使用 shadcn/ui copy-in `ToggleGroup` 或等价 Radix toggle primitive。
- 当前 Agent：icon + display name 的胶囊。
- 其他 Agent：只显示 icon。
- 每个按钮具备 tooltip、`aria-label`、`aria-pressed` 或标准 toggle semantics。
- 图标视觉尺寸建议 16–18px，点击热区至少 36px。
- 选中态使用轻量背景、边界和字重，不使用大块金色填充。
- hover/pressed/focus 不改变元素尺寸，避免布局跳动。
- Agent 数量较多时横向滚动或收纳到 overflow，不换成多行密集网格。
- `+` 按钮进入 Agent 管理。

不可用 Agent：

- 已知但未配置：禁用图标，tooltip 展示前端 i18n 原因，可提供进入 Agent 管理的点击入口。
- 配置但健康检查失败：禁用发送，错误展示在 Agent 配置行下方。
- 未支持 Agent：不作为可选择项。

### 9.3 模型和权限控件

Direct 的模型和权限放在主输入框右下角，位于发送按钮左侧。

模型：

- Agent 提供 models 时展示当前模型名。
- Agent 不提供 models 时展示“默认模型”。
- 切换 Agent 后恢复该 Agent 的记忆值。
- 记忆值不在当前 models 中时回退默认值。

权限：

- 第一项始终为“Agent 默认”。
- 后续项来自 `supportedModes`。
- 记忆值失效时回退 Agent 默认。

两者继续使用 shadcn `Select`，触发器使用紧凑胶囊样式，不自研 dropdown。

### 9.4 模式切换

- 切到 Direct：恢复 Direct workspace preference，不清空正文和附件。
- 切到 Workflow：恢复上次 workflow template。
- 切到 AUTO：恢复 AUTO 配置。
- 三种模式配置互不覆盖。
- 发送成功后清空正文/附件，但保留 workspace 级模式与配置记忆。
- 发送失败不清空正文和附件。

### 9.5 Direct 创建校验

校验顺序：

1. workspace 存在。
2. content 非纯空白。
3. Agent 已选择。
4. Agent 已配置、supported、diagnostic available。
5. model 属于当前 Agent 支持列表，或为空表示默认。
6. permission mode 属于当前 Agent 支持列表，或为空表示默认。
7. 附件校验通过。

错误就近显示在模式配置区，不跳转到 Workflow 配置页。需要修复 Agent 配置时提供 Agent 管理入口。

## 10. Direct 运行时 UI

### 10.1 隐藏 runtime 心智

Direct 页面隐藏：

- workflow 查看和编辑入口
- session switcher
- round/node/attempt path
- run success/failure 状态
- launching-next-node 文案
- workflow invalid 修复入口
- system prompt 查看入口
- 节点完成横幅
- manual check

保留：

- Agent icon 和名称
- 当前 model / permission mode
- sessionId 的高级信息入口
- 用户/Agent 消息
- thought / plan / tool call
- permission / elicitation
- 发送中 / 思考中 / 工具执行中 / 回复中
- 当前 turn 和会话累计耗时
- token / context / cost
- 停止当前回复
- 附件、产物、raw、diagnostics

### 10.2 Header

Direct header 建议展示：

```text
[Agent icon] Agent display name · model · permission
```

标题仍是 task/conversation 标题，可 inline edit。

不突出 `runId`。如需要诊断，可放在高级详情或 tooltip 中。

### 10.3 Composer

Direct runtime composer 继续复用 `ACPChatDialog`：

- idle：正常输入。
- active：Direct 保持 textarea 可输入，后续提交进入持久化队列；Workflow/AUTO 仍锁定 textarea。三种模式继续显示当前 processing kind、计时和停止。
- permission/elicitation：保持同一个 composer，仅锁定和展示提示。
- failed：恢复输入，在输入区附近展示结构化错误和重试提示。

不能为 Direct 新建另一套 chat component。

## 11. 侧边栏与搜索

### 11.1 Direct task row

当前 task row 左侧状态点按 run outcome/status 着色。Direct 改为 Agent icon：

```text
[Claude icon]  随便问我几个问题                5h
[Codex icon]   修复这个类型错误                8h
```

规则：

- Direct 不显示成功绿点。
- Direct 不显示停止/暂停黄点。
- Direct 单轮失败不显示长期红点。
- 当前选中态通过整行背景表达。
- Direct 正在回复或停止当前回复时，在 Agent icon 外显示轻量主色旋转环；不使用绿/黄/红语义。状态来源是后端 task 级 canonical activity，必须覆盖首轮 runtime active 与 completed run 上的 same-session ACP follow-up，不能只读取 `latestRun.status`。
- relative time 来自 Direct `lastActivityAt`。

### 11.2 Workflow / AUTO 保持原状态点

Workflow 和 AUTO 仍然需要：

- 运行中状态
- 暂停/可继续
- 成功/失败/异常

因此只做按 `runMode` 的展示策略分流，不全局删除状态点。

### 11.3 Run 历史

Direct 主 task row 点击后直接进入最新 Direct session。

Direct 不展开 run 历史，也不展示 `run-00x` 子行。即使底层因内部执行或未来“重新开始”产生多个 run，侧边栏仍保持一个持续对话入口；具体 run 仅作为内部存储和诊断身份。

### 11.4 搜索和置顶

- Direct 搜索结果使用 Agent icon。
- Workflow / AUTO 搜索结果继续使用状态点。
- 置顶区与 workspace 区使用完全一致的身份展示策略。
- Direct Agent identity 必须来自 VM，不在组件中重复解析 metadata。

## 12. Conversation metadata

Direct `authoring/conversation.json` 建议结构：

```json
{
  "version": "2",
  "source": "conversation-ui",
  "runMode": "direct",
  "directConfig": {
    "agentType": "claude-acp",
    "modelId": "claude-sonnet",
    "permissionMode": "ask"
  },
  "agentIdentity": {
    "agentType": "claude-acp",
    "displayName": "Claude Code",
    "iconKey": "claude"
  },
  "titleAutoGenerated": true,
  "initialAttachmentNames": [],
  "createdAt": "...",
  "lastActivityAt": "..."
}
```

开发阶段采用破坏式更新：

- 新建 metadata 直接使用新版本。
- 不为未发布的 Direct 草案增加兼容层。
- 现有 AUTO / Workflow metadata 读取逻辑同步迁移到 typed struct，避免继续散落读取 `serde_json::Value`。

## 13. 后端改动

### 13.1 `src/config/mod.rs`

- 新增 `ConversationRunMode`。
- 新增 `ConversationDirectConfig`。
- 新增 Direct workspace/agent preference。
- `ConversationRunModeEntry` 增加 `direct_config`。
- 状态文件序列化和测试同步更新。

### 13.2 `src/dsl/mod.rs`

- 新增 `PromptEnvelopeMode`。
- `WorkerNode` 增加 `prompt_envelope`，默认 runtime-managed。
- workflow validation 允许内部 Direct worker 使用 raw-agent。
- WorkflowEditor 作者态保存的普通节点始终使用 runtime-managed，不暴露 Direct 字段。

### 13.3 `src/provider/mod.rs`

- `WorkerInvocation` 携带 prompt envelope。
- `render_prompt_bundle` 按 envelope 分流。
- RawAgent system prompt 永远为空。
- RawAgent 首轮和 continue 用户文本保持原文。
- 添加单元测试证明没有 runtime/profile/hidden/output prompt 泄漏。

### 13.4 `src/acp/client.rs`

- 暴露 per-attempt `prompt_activity()`。
- Provider control 增加 starting/running/cancel-requested/finished 阶段。
- prompt terminal session update 前先标记 finished。
- 保持 `session/new` / `session/load` 在空 system prompt 下不附加 Gold Band prompt。
- Direct 的 token、timing、usage、permission、elicitation 继续走现有实现。

### 13.5 `src-tauri/src/view_models_conversation.rs`

- lifecycle 派生增加 live prompt activity 输入。
- 修复 completed runtime 上真实 follow-up 被 stale fuse 压制的问题。
- active session tree 和 `activeSessions` 使用修复后的 lifecycle。
- `ConversationTaskRowVm` 增加 Direct Agent identity 和 last activity。
- `ConversationRunVm` 返回 typed run mode 和 direct config/identity。
- create validation 增加 Direct 分支。
- `build_direct_workflow()` 生成内部单 Worker workflow。
- create flow 写入 Direct conversation metadata。

### 13.6 `src-tauri/src/commands_conversation.rs`

- run mode settings 只负责持久化 Direct workspace preference，不在运行模式管理页暴露 Direct 配置 UI。
- 复用 typed `save_conversation_run_mode` 保存快速会话 composer 中的 Direct per-Agent 偏好。
- sidebar VM 读取 Direct metadata。
- create conversation 支持 Direct。

### 13.7 `src-tauri/src/commands.rs`

- `submit_conversation_prompt` 继续作为统一入口。
- Direct 首轮停止和 completed-run follow-up 停止使用相同 command，但按 runtime current 状态分层收敛。
- session update event 返回修复后的 lifecycle。
- 错误返回结构化 code/params。

### 13.8 `src/app/*`

- Direct 内部 workflow 走现有 background run。
- 首轮 node 执行使用 RawAgent envelope。
- Direct UI metadata 不影响普通 workflow runtime 控制。
- app close/cancel all 能识别 completed run 上的 Direct active prompt。

## 14. 前端改动

### 14.1 `web/src/types.ts`

- `ConversationRunMode` 增加 `direct`。
- 新增 `ConversationDirectConfigVm`。
- 新增 `ConversationAgentIdentityVm`。
- run mode、create input、task row、run VM 同步增加 Direct 字段。
- ACP lifecycle facet 增加 phase。

### 14.2 `web/src/lib/conversation-run-mode-config.ts`

- 默认模式是否改为 Direct 由产品另行决定；本期只保证三种模式可持久化。
- 新增 Direct config normalize/merge helper。
- Direct、Workflow、AUTO 配置互不覆盖。

### 14.3 `web/src/components/conversation/ConversationComposer.tsx`

- 模式顺序改为 Direct、Workflow、AUTO。
- 拆分不同模式的配置区域，避免继续扩大单组件条件分支。
- 建议抽出：
  - `ConversationRunModeTabs`
  - `DirectAgentPicker`
  - `DirectComposerControls`
  - `WorkflowQuickConfig`
  - `AutoQuickConfig`
- Direct 模型/权限控件进入输入框右下角。
- Direct Agent icon picker 位于模式配置区。
- 使用已有 `agentIconSrc/agentIconClass`。
- Agent picker 优先生成 shadcn/Radix copy-in 控件，不自研 toggle 基础行为。
- Agent 列表为空时保留“请先在 Agent 管理中添加 Agent”提示，并在旁边展示 shadcn icon button 形式的“+”；点击复用 Conversation 页面导航进入 Agent 管理，不清空 composer 草稿。

### 14.4 `web/src/components/acp/ACPChatDialog.tsx`

- Direct 不新增新 chat UI。
- session active、timer active、stop 可用性以 lifecycle 为权威。
- `awaitingResponse` 仅作为短暂 overlay。
- live session shell 同时支持 runtime active 和 ACP follow-up active。
- Direct presentation 隐藏 workflow 相关外部状态和文案。

### 14.5 `web/src/lib/acp-runtime-composer-state.ts`

- 继续只映射后端 lifecycle/composer。
- 删除依赖 raw session status 修补业务 active 的分支。
- local sending/cancelling 只覆盖命令往返窗口。

### 14.6 `web/src/components/conversation/ConversationSidebar.tsx`

- task row 按 runMode 选择 identity renderer。
- Direct 使用 Agent icon。
- Workflow/AUTO 使用 status dot。
- Direct relative time 使用 `lastActivityAt`。
- active halo 只表示当前正在回复，不表达成功/失败。

### 14.7 其他前端文件

- `ConversationSearchDialog.tsx`：Direct 搜索结果使用 Agent icon。
- `ConversationRunHeader.tsx`：Direct header 展示 Agent identity，隐藏 workflow/run 心智。
- `ConversationRunPage.tsx`：Direct 隐藏 session switcher、workflow drawer 和 active sibling runtime 行。
- `RunModeManagementPage.tsx`：保持仅有工作流与 AUTO 两个 tab；Direct 配置只存在于快速会话 composer。
- `App.tsx`：创建 input、mode persistence、snapshot merge 同步 Direct 字段。
- `api/*`：类型和 payload 同步。
- `i18n.ts`：同步中英文 Direct、Agent 默认、模型默认、错误和状态文案。

## 15. 组件和依赖策略

不新增第三方依赖。

优先复用：

- prompt-kit：消息、输入、Markdown、tool、thought。
- shadcn/ui：Button、Select、Tooltip、ToggleGroup、ScrollArea、Dialog。
- Tailwind：布局和 token 化样式。
- 现有 Agent icon SVG 和 helper。
- 现有 ACP session config normalization。

如果项目尚无 `ToggleGroup` copy-in 组件，应通过 shadcn/ui 生成后按 Gold Band token 调整，不能手写无障碍 toggle group。

## 16. 删除和收敛清单

实现时应同步删除或替换：

- completed runtime 无条件 suppress ACP active 的旧逻辑。
- 对应错误单元测试断言。
- 前端依赖 component-local awaiting state 判断持久 active 的规则。
- Direct 借用 `ConversationAutoConfig` 的临时方案。
- Direct 使用 Agent 文本下拉而不是 icon picker 的临时 UI。
- Direct task row 的 runtime 彩色状态点。
- Direct 页面中的 workflow 编辑/查看、session tree 和 launching-next-node 文案。
- conversation metadata 中散落的 untyped `serde_json::Value` runMode 读取，逐步收敛到 typed metadata。

不删除：

- Workflow/AUTO 状态点。
- Workflow/AUTO session switcher 和 workflow graph。
- ACP raw status、snapshot、timing、usage 数据；它们仍用于后端派生和诊断。

## 17. 测试方案

### 17.1 Rust lifecycle 单元测试

- completed runtime + metadata running + live Running => ACP active。
- completed runtime + metadata running + live Starting => ACP active/launching。
- completed runtime + metadata running + no live activity => stale active 被压制。
- completed runtime + live CancelRequested => stopping。
- terminal session update 发出前 activity 已 finished。
- conversation run VM 将 completed-run live follow-up 放入 activeSessions。
- background session lifecycle patch 不覆盖 selected Direct session。

### 17.2 Prompt 单元测试

- Direct 首轮 system prompt 严格等于空字符串。
- Direct 首轮 user prompt 等于用户原文。
- Direct continue system prompt 严格等于空字符串。
- Direct continue user prompt 等于本轮原文。
- Direct prompt 不包含 runtime、hidden、profile、goal、output、repair 内容。
- codex-acp 不内联 Gold Band stable system prompt。
- Claude ACP `session/new/load` 不携带 system prompt append。
- Direct continue 只携带本轮新增附件。

### 17.3 Direct create/config 单元测试

- Direct 必须选择有效 Agent。
- model/mode 必须属于所选 Agent 能力。
- build_direct_workflow 只生成一个 raw-agent worker 和 `$end` edge。
- Direct config 不包含 AUTO 字段。
- workspace + agent preference 能独立记忆。
- Agent A/B 切换能恢复各自模型/权限。
- 已失效模型/权限回退默认。

### 17.4 前端状态测试

- Direct/Workflow/AUTO tab 顺序固定。
- Direct 切换不清空正文和附件。
- Agent icon picker 选中和键盘操作正确。
- 切换 Agent 恢复对应模型/权限。
- Direct 模型/权限位于 composer controls，不渲染 AUTO 大配置卡。
- lifecycle ACP active 且没有 optimistic event 时仍锁输入、显示状态、可停止。
- 页面重新挂载后 timing/token 继续显示。
- session terminal 后恢复输入。
- Direct sidebar 使用 Agent icon，不渲染 status dot。
- Direct sidebar 不渲染 run 子列表。
- Workflow/AUTO sidebar 仍渲染 status dot。
- Direct 搜索和置顶同样使用 Agent icon。

### 17.5 停止和异常测试

- Direct 首轮停止后 runtime paused，可用文本继续。
- Direct completed-run follow-up 停止不改变 completed run。
- 停止后继续复用原 sessionId。
- provider error 持久化结构化错误并恢复输入。
- app close 能取消 completed Direct follow-up。
- crash 后 stale running 不恢复为 active。
- permission/elicitation 页面切换后仍保持 pending 和 composer 锁定。

### 17.6 建议命令

```text
cargo test
npm run web:test
npm run web:build
```

涉及 UI 后必须启动前端并使用 deep link 验证：

```text
/chat
/chat/projects/:projectId/tasks/:taskId/runs/:runId
```

页面验证结束后清理测试 task、run、ACP session 和自己启动的开发进程。

## 18. 人工验收矩阵

### 18.1 快速会话

- [ ] 模式顺序是 Direct、工作流、AUTO。
- [ ] Direct 选中后展示 Agent icon picker。
- [ ] 当前 Agent 显示 icon + 名称，其他 Agent 显示 icon。
- [ ] Agent 不可用时无法发送，并能看到原因；Direct 不跳转运行模式管理，Agent 源配置仍由独立 Agent 管理页负责。
- [ ] 模型和权限位于输入框右下角。
- [ ] Agent A/B 分别恢复自己的模型和权限记忆。
- [ ] 模式切换不丢失正文和附件。

### 18.2 Prompt

- [ ] ACP raw frame 中没有 Gold Band system prompt append。
- [ ] timeline 用户消息只包含用户原文和本轮附件。
- [ ] 首轮和后续追问都不出现 hidden runtime context。
- [ ] 模型、权限和 MCP 配置仍正常生效。

### 18.3 持续对话

- [ ] 首轮自然结束后可继续输入。
- [ ] 后续追问复用相同 ACP sessionId。
- [ ] 切换到其他会话再回来，仍显示思考/工具/回复状态。
- [x] Direct 正在执行和首次 prompt“发送中”时输入保持可用，后续消息进入队列且停止按钮可用。
- [ ] 耗时继续增长，token 继续更新。
- [ ] 回复结束后输入恢复，计时停止增长。

### 18.4 停止和异常

- [ ] 首轮停止后可继续当前会话。
- [ ] 完成后追问停止不会把会话标为 workflow paused。
- [ ] 停止中显示统一遮罩/状态，不出现重复停止入口。
- [ ] provider 异常在会话内展示，可再次发送。
- [ ] 应用关闭后重新进入不错误显示思考中。

### 18.5 侧边栏

- [ ] Direct 行首是 Agent icon。
- [ ] Direct 不显示绿/黄/红 run 状态点。
- [ ] Workflow/AUTO 状态点不受影响。
- [ ] Direct 相对时间随后续对话更新。
- [ ] Direct 置顶和搜索结果使用相同 Agent icon。
- [ ] 后台 Direct 回复可选使用轻量 icon halo，不使用成功/暂停颜色。

## 19. 实施阶段

### Phase 0：文档与状态矩阵冻结

- 更新本开发方案。
- 更新产品设计文档中的 conversational home/runtime/prompt bundle。
- 冻结 Direct 数据结构、生命周期矩阵和 UI 验收图。

### Phase 1：生命周期前置修复

- 暴露 per-attempt prompt activity。
- 修复 completed-run follow-up lifecycle。
- 前端 composer/tree/timer 统一消费后端 lifecycle。
- 补齐回归测试。

完成条件：不新增 Direct 时，现有 completed run 追问已经能在页面切换后正确恢复生命周期。

### Phase 2：Direct 数据和 prompt

- 新增 run mode/direct config/preference。
- 新增 prompt envelope。
- build_direct_workflow。
- Direct create/validation/persistence。
- RawAgent prompt 测试。

完成条件：后端可以创建 Direct 会话，首轮和追问 system prompt 均为空。

### Phase 3：快速会话 UI

- 三模式 tab。
- Agent icon picker。
- 模型/权限 composer controls。
- per-agent memory。
- 前端校验和 i18n。

完成条件：用户可以从 `/chat` 选择 Agent 并发起 Direct 会话。

### Phase 4：Direct runtime presentation

- Direct header。
- 隐藏 workflow/session tree/run 状态心智。
- 复用 ACP chat、stop、usage、timing、permission。
- 处理结构化错误。

完成条件：Direct 页面只呈现持续 Agent 对话语义。

### Phase 5：侧边栏、搜索和活动时间

- Agent identity VM。
- Direct icon row。
- lastActivityAt。
- 置顶、搜索一致性。

完成条件：Direct 历史能明确识别 Agent，并按真实最近对话活动排序。

### Phase 6：完整验证与文档同步

- Rust 单元测试。
- Web 单元测试。
- build。
- 前端 deep link 人工验证。
- 清理旧逻辑。
- 更新 MVP 计划实施结果。

## 20. 影响文件清单

预计主要涉及：

```text
src/config/mod.rs
src/dsl/mod.rs
src/provider/mod.rs
src/acp/client.rs
src/app/mod.rs
src/app/node_executor.rs
src-tauri/src/commands.rs
src-tauri/src/commands_conversation.rs
src-tauri/src/view_models_conversation.rs
src-tauri/src/main.rs
web/src/types.ts
web/src/api.ts
web/src/api/client.ts
web/src/api/desktop.ts
web/src/api/browser.ts
web/src/App.tsx
web/src/components/conversation/ConversationComposer.tsx
web/src/components/conversation/ConversationSidebar.tsx
web/src/components/conversation/ConversationRunHeader.tsx
web/src/components/conversation/ConversationSearchDialog.tsx
web/src/pages/ConversationRunPage.tsx
web/src/pages/RunModeManagementPage.tsx
web/src/components/acp/ACPChatDialog.tsx
web/src/lib/acp-runtime-composer-state.ts
web/src/lib/conversation-run-mode-config.ts
web/src/lib/run-mode-validation.ts
web/src/i18n.ts
web/tests/*
```

## 21. 文档同步要求

实现 Direct 时必须同步维护：

产品设计文档：

- `docs/gold-band/产品设计文档/interaction/app/conversational-home.md`
- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/产品设计文档/interaction/app/run-mode-management.md`
- `docs/gold-band/产品设计文档/provider/prompt-bundle.md`
- 如果 `prompt_envelope` 进入 frozen DSL：`docs/gold-band/产品设计文档/dsl/nodes/worker.md`

开发计划：

- 本文档的实施状态和决策变更。
- `docs/gold-band/开发计划/生命周期整理/工作流-ACP-生命周期统一重构.md`
- `docs/gold-band/开发计划/新UI/会话式主页实施计划.md`
- `docs/gold-band/开发计划/新UI/会话式主页实施进度.md`
- `docs/gold-band/开发计划/gold-band-mvp-plan.md`

新增或修改内置 prompt 时仍必须放入 `src/prompts/zh-CN` 和 `src/prompts/en` 对称目录。但 Direct 的目标是空 system prompt，不应为 Direct 新建一个内容为空或说明“这是 Direct”的内置 prompt 文件。

## 22. 最终决策摘要

1. Direct 是持续 Agent 会话，不是用户可见的单节点 workflow。
2. 底层复用内部单 Worker execution shell，避免新建第二套存储和生命周期。
3. Direct 首轮和追问的 system prompt 都严格为空。
4. lifecycle 必须先识别 completed run 上的真实 live ACP follow-up，不能依赖前端局部状态。
5. 快速会话模式顺序固定为 Direct、工作流、AUTO。
6. Direct 使用 Agent icon picker；模型和权限放在 composer 右下角。
7. 模型和权限按 workspace + agent 记忆。
8. Direct 侧边栏使用 Agent icon，不展示 workflow 绿/黄/红状态点。
9. Direct 使用 lastActivityAt 表达持续对话的最近活动。
10. 发送和停止继续使用 `submit_conversation_prompt` / `stop_active_session` 单一入口。
11. token、耗时、权限、elicitation、附件和工具展示全部复用现有 ACP 管道。
12. 实现后必须同步更新产品设计文档、开发计划和接口级单元测试。
13. Direct 不进入运行模式管理页；Agent、模型和权限只在快速会话 composer 内配置。
14. Direct 侧边栏使用与状态点等宽的身份槽位，不展示 run 子列表；Direct ACP header 不展示系统提示按钮。

## 23. 2026-07-23 实施结果

状态：已完成实现与验证。

- [x] ACP provider control 暴露 `Starting / Running / CancelRequested`，并在 terminal session update 前结束 live activity。
- [x] completed runtime 上的真实 follow-up 不再被 stale snapshot fuse 压制；无 live activity 的旧 `running` 仍会被压制。
- [x] 新增 typed `ConversationRunMode::Direct`、`ConversationDirectConfig` 和 workspace 内 per-Agent 偏好。
- [x] 新增 `PromptEnvelopeMode::RawAgent`；Direct 首轮和 continue 的 system prompt 严格为空，user prompt 保持本轮原文。
- [x] Direct 创建使用内部单 Worker workflow，复用现有 ACP session、附件、MCP、permission、elicitation、usage、timing 和停止链路。
- [x] 快速会话模式顺序固定为 `Direct / 工作流 / AUTO`；Agent 使用 icon picker，模型/权限进入 composer 右下角。
- [x] Direct 无可选 Agent 时展示提示文案和“+”入口，点击直接进入 Agent 管理页。
- [x] Direct 运行时隐藏 workflow、session tree、run outcome 和重跑心智，header 展示 Agent identity。
- [x] Direct 侧边栏 task 行和搜索结果使用 Agent icon；身份槽位与 Workflow/AUTO 状态点等宽，标题起点对齐，最近排序与相对时间使用 `lastActivityAt`。
- [x] Direct 侧边栏补齐 task 级活跃 turn 旋转环；后端聚合 runtime status 与 per-attempt live prompt activity，前端在 submit/stop lifecycle snapshot 和 live lifecycle 更新时同步置顶区与 workspace 区，completed run 追问不再丢失运行态提示。
- [x] Direct 侧边栏不展示 run 子列表；Direct ACP header 不展示无意义的“系统提示”按钮。
- [x] 创建接口增加 Direct Agent/model/permission 后端校验，错误以结构化 code 返回。
- [x] profile resolver 按 prompt envelope 分流：`runtime-managed` 必须解析角色，`raw-agent` 跳过角色解析且禁止绑定 profile；Direct 创建不再触发 `direct-agent is not associated with role`。
- [x] 运行模式管理页删除 Direct tab 与配置区，只保留工作流和 AUTO；Direct composer 不展示跳转该页的“去配置 / 修复”入口。
- [x] 产品设计文档、生命周期计划、会话式主页计划/进度和 MVP 计划已同步。
- [x] Rust 与 Web 增加 raw prompt、live follow-up lifecycle、Direct workflow、per-Agent 记忆、tab 顺序和 sidebar identity 回归测试。

最终验证：

- `cargo test --workspace`：通过。
- `npm run web:test`：59 个测试文件、391 项测试通过。
- `npm run web:build`：通过。
- 本地 `/chat` 深链路：已验证 `Direct / 工作流 / AUTO` 顺序、Agent icon picker、composer 右下角权限/模型区域；运行模式管理页仅展示工作流与 AUTO。
- 本地 Direct runtime 预览：已验证 header 仅展示标题与 Agent identity，不展示 runId、session switcher、workflow 或重跑心智；验证期间发现并修复 Direct picker 缺失 `TooltipProvider` 的运行时错误。
- 验证 tab、Vite dev server 和测试进程已清理。

## 24. 2026-07-24 多轮回复通知补齐

状态：已完成实现与全量 Rust 回归验证。

根因不是 Direct 通知按钮或前端页面状态缺失，而是通知总线此前只消费 `InterventionRequested / RunCompleted`。Direct 首轮会结束内部单 Worker run，因此能收到一次 `RunCompleted`；后续追问直接调用 ACP `session/prompt`，没有 turn 级 lifecycle 终态，所以无论 Direct 还是 Workflow/AUTO 节点完成后的手动追问都不会通知。

本次按统一 ACP turn 生命周期修复：

- [x] 新增 `AcpTurnOutcome::{Completed, Failed, Cancelled}` 与 `RuntimeLifecycleEvent::AcpTurnFinished`。
- [x] `send_acp_prompt` 为每个 prompt 固化 `turnId`；调用方未传时后端生成 UUID，并把同一 ID写入 prompt bundle 与终态事件。
- [x] Direct 后续追问及 Workflow/AUTO 节点完成后的普通手动追问，成功和失败都发送 turn 终态事件。
- [x] 用户主动停止映射为 Cancelled，通知构造器直接返回 None；transport interrupted 映射为 Failed。
- [x] 新增 `AgentTurnFinished` 通知类型和包含 turnId 的去重键，避免同一 attempt 的后续追问被首轮去重。
- [x] Direct 首轮继续复用 `RunCompleted`，但事件生成时从持久化 conversation metadata 写入 `completionAgentLabel`，通知层转换为 Agent 回复语义；不依赖节点 ID，也不依赖当前 UI 工作区。
- [x] 前台同 session 抑制、失焦/最小化/隐藏/其他会话通知的既有 attention policy 保持不变。
- [x] permission / elicitation 即时通知保持不变，runtime continue 不额外发 turn 通知，避免双重提醒。
- [x] 新增通知文案、turnId 去重、取消抑制、stop reason 分类与 lifecycle event 字段回归测试。

验证结果：

- `cargo check -p gold-band -p gold-band-desktop`：通过。
- 通知、attention policy、turn outcome 与 lifecycle event 定向测试：通过。
- `cargo test --workspace`：通过；核心库 322 项、桌面端 115 项及全部 integration/doc tests 无失败。

## 25. 2026-07-24 多轮 permission / elicitation 通知修正

状态：已完成实现与全量 Rust 回归验证。

Direct 与节点完成后的手动追问复用同一个 ACP attempt。原通知桥接虽然已经能把 `permissionRequest / elicitationRequest` 转成 lifecycle 事件，但桌面通知 subscriber 丢弃了上游 `eventId`，重新按 `run / round / node / attempt / reason` 构造去重键。因此同一 attempt 第一次请求通知后，后续 AskUserQuestion 或权限请求会被误判为重复。

修正方案：

- lifecycle `InterventionRequested.eventId` 是唯一 canonical 去重身份，通知层直接采用，不再二次推导。
- ACP permission/elicitation 的 eventId 增加 `AcpUiEvent.id`，形成 request-scoped key。
- 同一 request 的重复 pending update 仍去重；同一 Direct 长会话中的不同 request 分别通知。
- 当前目标 session 正在前台可见时仍不发送 OS 通知；切换会话、失焦、最小化或隐藏后才提醒。
- intervention 通知正文使用 Direct conversation metadata 中的 Agent display name，例如 `Claude 等待你的回答`、`Codex 需要授权`，不展示内部 `direct-agent` node id。

验证结果：

- 核心通知模型 19 项定向测试通过。
- 连续 elicitation、连续 permission 与非 pending 事件过滤测试通过。
- `cargo test --workspace`：通过；核心库 326 项、桌面端 120 项及全部 integration/doc tests 无失败。

## 26. 2026-08-07 Direct 运行中待发送队列

状态：实现完成，正在执行完整回归与页面验收。

本次不是在前端增加延时发送补丁，而是补齐 attempt 级发送意图和统一 turn 终态策略：Direct/Workflow/AUTO 继续共享 runtime、ACP session、timeline、停止与恢复生命周期，只有 Direct lifecycle 在 active 时把 composer submit target 投影为 `queue-prompt`。

数据与接口：

- [x] 新增 attempt 级 `acp.prompt-queue.json`、`PromptQueue / QueuedPrompt / QueuedPromptState`，FIFO 上限统一为 10。
- [x] 队列项保存稳定 item/prompt identity、正文、附件、创建时间和 dispatch 状态；编辑不改变位置。
- [x] 新增 update/delete/use Tauri commands，继续使用结构化 error code + params。
- [x] lifecycle 仅为 Direct 投影 `promptQueue`；Workflow/AUTO 的数据和 composer 规则不变。
- [x] App turn hook 从仅成功通知收敛为 success/failure 统一 finished 回调；成功才自动出下一条，失败/取消恢复 dispatching 项且停止排空。

发送仲裁与持久化：

- [x] prompt 成功后仅在 Direct 且存在 queued item 时注册 600 ms 低优先级候选。
- [x] 窗口内真实用户提交通过 queue revision 取消候选并优先进入现有 ACP attempt prompt lock；空队列 Direct、Workflow、AUTO、runtime continue/repair 不承担等待。
- [x] 所有发送继续受既有 attempt prompt lock 串行化，不允许两个 prompt 并发进入 Provider。
- [x] `dispatching` 只有 durable timeline 接受且 turn 成功后才删除；进程内 active dispatch 与 crash orphan reconciliation 分离，避免 lifecycle 读取把在途项误恢复。
- [x] 停止/失败/取消不触发下一条，队列保留；应用重启后未接受的孤儿 dispatch 恢复为 queued。
- [x] 修正 stop 与 success 终态竞态：stop command 入口先设置进程内 suspension gate，再持久化 `autoDispatchSuspended` 和 revision；scheduler claim 与 Provider 入口双重检查，停止后到达的 success 不再自动发送。用户主动发送或手动“使用”才恢复自动队列策略。

前端：

- [x] Direct 运行中 composer 保持可输入，发送按钮显示“加入队列”；满 10 条时禁止继续入队。
- [x] 队列紧贴 composer 上方、位于 usage/token 行下方；默认展示 3 条，可展开至全部 10 条。
- [x] 队列 surface 属于 attempt 底部常驻区域，不属于 active/idle/stopped/runtime-error 的 composer 分支；只要持久化队列非空，停止、刷新或重新进入会话后仍展示并可编辑、删除。
- [x] Direct 队列能力由 run mode 显式传入 composer 状态机；首次 prompt 尚处于本地“发送中”、后端 active lifecycle 尚未回传时，后续提交也立即路由到 `queue-prompt`，不出现短暂锁输入。
- [x] Conversation run/session-tree 初始与刷新快照和单 attempt lifecycle 接口统一装配同一持久化队列，杜绝停止态刷新丢失队列。
- [x] 使用 shadcn Collapsible/Button/Tooltip/Textarea、Tailwind 和现有 prompt-kit composer；提供编辑、使用、删除 icon action 与无障碍标签。
- [x] 入队响应不创建 optimistic 用户气泡、不进入 awaiting response；自动出队仍写标准用户 timeline 消息并复用标准样式。
- [x] 编辑原位保存；停止/空闲后可指定使用；删除立即回灌后端 lifecycle。
- [x] 修复 completed Direct session 进入 follow-up `session/resume` 时的会话壳竞态：同 attempt 快照合并不再降级已持久化的 `sessionEstablished / sessionId`；详细 session payload 短暂缺失时由持久化引用保持历史会话壳、composer 和待发送队列，不再误显示“ACP 会话失败”。
- [x] 根因修正：排队的 lifecycle-only 更新不再被认为 session 清空；订阅层只在带有非空 authoritative session payload 时替换当前会话，从根上避免排队操作在 `session/resume` 窗口清掉 composer。
- [x] 首轮自动出队根因修正：创建/重跑与继续入口统一装配 ACP live、session snapshot、`prompt_turn_finished` callback；首轮 `end_turn` 现在也会产生后继的 Direct 队列调度。

接口级回归：

- [x] Rust：容量、FIFO/编辑位置、用户 revision 抢占、stop suspension、主动恢复、指定使用顺序、进程内 dispatch、重启孤儿恢复。
- [x] Web semantic composer：Direct active 可输入/可入队、容量满、非 Direct active 仍锁定。
- [x] Web semantic composer：Direct 首次发送过渡期即使 lifecycle 尚未携带 queue，也可输入并入队。
- [x] Rust conversation run VM：停止态 selected leaf 仍投影持久化队列。
- [x] Web DOM：默认 3 条/展开、原位编辑、icon aria-label、运行中禁用手动使用。
- [x] Web 会话壳：已建立 session 的 detail payload 临时为空时仍渲染 composer；same-attempt resume refresh 不丢失 session 引用和历史 payload。
- [ ] 完整 Rust/Web test 与 build。
- [ ] Direct deep link 页面验收（位置、展开、编辑、删除、停止后使用、深浅主题）并清理测试资源。
