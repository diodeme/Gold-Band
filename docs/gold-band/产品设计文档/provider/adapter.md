# Provider Adapter 接口

## 1. 定位
provider adapter 是 provider-specific 差异的隔离层。

它内部应至少包含两层接口，但这两层首先是 **ownership boundary**，其次才是代码分层：

- **A()：runtime 拥有并直接依赖的稳定接口**
- **B()：provider implementation 拥有并实现的内部执行接口**

说明：
- A() 与 B() 可以物理上同处 provider 模块中
- 但 A() 的契约归 runtime 所有，B() 的契约归 provider implementation 所有
- Gold Band runtime 只应直接依赖 A()，不应直接依赖某个 provider 的 B()

它整体负责：
- 调起 provider，优先启动 ACP-compatible adapter
- 接收 runtime 传来的外层调用请求
- 在 A() 内部选择热数据与冷数据
- 在 A() 内部把调用请求整理成 prompt bundle
- 把 prompt bundle 映射为 ACP 调用：根据 `supports_system_prompt` 决定是否通过 `_meta.systemPrompt.append` 注入稳定 system prompt；不支持时把稳定 system prompt 作为 Gold Band hidden 段内联到 user prompt 前
- 接收 ACP `session/update`、permission request 与 prompt response
- 对 `session/update` 做 provider-aware 归一化：消息正文、思考、计划和 provider 诊断必须落入不同领域；诊断不得混入 assistant 正文或最终输出
- 保存 ACP 会话观测材料、adapter 返回的 session config 快照（`models` / `modes` / `configOptions`）、通过项目级 feature flag 控制的可选 `session/list` 轮询 best-effort 拉取的 session title 缓存与 raw frame
- 提供 worker reference 与外部 CLI handoff
- 暴露能力信息

## 2. 最小接口

### `describeProvider()`
返回 provider 基本信息与能力摘要。

最少表达：
- `providerId`
- `displayName`
- `capabilities`

`capabilities` 当前包含以下静态 provider 能力（运行期声明，非 doctor 探测结果）：
- `supports_open_session` / `supports_continue_session`：是否支持新建 / 续接 ACP session；
- `supports_system_prompt`：是否接受 `session/new` 的 system prompt（claude-acp 支持，codex-acp 不支持）；
- `supports_raw_stream`：是否支持 raw frame / transcript；
- MCP transport 能力属于 MCP/Agent capability 领域，应以当前 ACP `initialize` 返回的 `agentCapabilities.mcpCapabilities` 为运行时事实源；provider adapter 不维护按 provider ID 硬编码的 transport 列表。Gold Band 在 `session/new|load` 前统一准备 MCP server：`stdio` 直接接受，HTTP/SSE 对明确声明为 `false` 的 transport 做过滤，Agent 未声明 `mcpCapabilities` 时保持原列表而不猜测。被过滤项以 `acp.mcp-transport-unsupported` 和结构化参数写入当前 attempt 的 `acp.diagnostics.jsonl`，实际请求只包含 accepted server，禁止静默丢弃。
- prompt attachment 先由统一 projection policy 生成 `Image / Resource / ResourceLink` 意图，再由当前连接的 `promptCapabilities.image / embeddedContext` 投影协议块。`ResourceLink` 是终态意图，即使 Agent 支持可选内联能力也不得重新读取或展开；不支持可选能力的 Agent 始终收到原文件 link。文本大小使用 UTF-8 字节边界，图片使用独立的编码字节与最长边边界，禁止按 Agent ID、扩展名个案或 UI 来源维护第二套阈值。
- 浏览器 `File`、剪贴板和拖放附件在进入 runtime 前先物化为不可变快照。物化接口不接收选择时缓存的文件大小；Base64 解码后的实际字节是快照大小的唯一事实源，空文件、单文件上限和总量上限均据此校验，返回的 `AttachmentFileVm.size` 也必须使用该值。源文件在选择与读取之间增长或缩小时，保存本次实际读取到的完整快照，不得因陈旧元数据拒绝。

- Agent 的 `configOptions` 是会随 adapter 升级变化的能力目录。前端使用纯函数对已保存 override 做交集规范化，保留仍存在且 value 有效的项，返回被删除的 option id；校验函数不得修改 React/persisted 输入对象，也不得把 stale override 当成阻塞会话的错误。Direct/AUTO 在提交前使用规范化结果，并在能力目录刷新后同步清理当前配置。
- `isDefault`

### `doctor()`
检查 provider 当前是否可用。

最少回答：
- provider 是否已安装
- 可执行入口是否存在
- 当前环境是否满足最小运行条件
- 失败时给出明确原因

桌面端持久化的 doctor 结果是 `~/.gold-band/desktop/agent-diagnostics.json`。每个 Agent 的 doctor 使用独立的 `~/.gold-band/doctor/acp/<agent-id>` 作为一次性 ACP 诊断 attempt 目录，并在目录内独立维护 `provider.pid`；每次运行前只清理本 Agent 的旧目录，成功后删除该目录，失败时只保留该 Agent 最近一次有界 raw/timeline/diagnostics JSONL bundle，并移除对应 `provider.pid`。因此全量周期诊断可以并行检查不同 Agent，但同一 Agent 仍必须 singleflight。手动诊断只执行一次，后台周期诊断首次失败后重试一次，第二次失败才成为最终结果。这些文件只用于诊断，不参与 runtime、UI 状态、业务 session 判断，也不作为 `supports_system_prompt` 等静态 provider capability 的事实来源。

### `runWorker()`
运行一次 AI worker attempt。

`runWorker()` 应被理解为 **A()：runtime-facing 稳定接口**。

其正式调用契约见 [Worker Invocation Contract](invocation.md)。

最小输入语义：
- `profile` / `profileContent`
- `requirementPath` 或 `requirementText`
- `workspaceDir`
- `attemptDir`
- `outputContract`（来自当前节点 `output` DSL）
- `runtimeContext`
- `predecessors[]`
- `taskInstruction`
- `sessionMode`（可选，缺省为 `new`）
- `userPromptRenderMode`（`RequirementTask` / `WorkflowResume` / `RuntimeRepair` / `UserMessage`，用于区分 workflow runtime 调用、内部修复和用户普通追问）
- `continueRefPath`
- `streamMode`

说明：
- `sessionMode` / `continueRefPath` 只影响 provider 如何启动本次 attempt
- `userPromptRenderMode` 决定 prompt 文本形态：workflow new/resume 才注入 hidden runtime context；runtime repair 和用户普通追问直接发送对应 prompt 原文
- 未显式提供 `sessionMode` 时，默认使用 `new`
- CLI 级 `continue` / `retry` 是 runtime 对 attempt 的控制动作，不等同于 provider 输入里的 `sessionMode`

最小输出语义：
- `status`
- `exitCode`
- `resultPayload`
- `runtimeError`
- `workerRefSeed`
- `sessionEvents`（ACP normalized UI projection，聚合 patch 落盘到 `acp.timeline.jsonl`）
- `rawSession`（ACP raw frame，落盘到 `acp.raw.jsonl`）

说明：
- `resultPayload` 不要求顶层携带 `version`
- 若当前节点声明了 `output`，则 `resultPayload.outputArtifact` 必须存在
- `outputArtifact.content` 固定为字符串，表示模型按 output structure 返回的原始内容
- provider 不负责把 `outputArtifact.content` parse 成语义对象
- `sessionEvents` 保持 ACP session event 语义，用于会话详情可视化，不再转换为 Gold Band 自研 `progress.events.jsonl`
- `rawSession` 只用于 raw viewer / 排障，不作为 UI 主协议
- 流式正文、思考和计划按每条事件实际携带的 provider identity 管理，不在 `initialize` 或 Agent Catalog 中推断整段会话是否支持身份。带稳定 `messageId` / provider history identity 的流按 `branchId + identity` 累计，工具、usage、无 ID warning 等其他 identity 的事件不得终止或覆盖它；新的稳定 identity 或下一条用户 prompt 才关闭旧稳定流。没有稳定 identity 的 ACP 文本只在同 kind 的连续事件段内使用本地 fallback identity，工具等非文本事件关闭该匿名段，后续匿名文本创建新 timeline item。不得按 Agent 类型特判，也不得根据文案、时间或位置伪造 message identity
- Agent 正文或思考 chunk 若为空字符串，或整段仅包含 Unicode `Format` 类不可见控制字符，则只参与当前流的增量累计，不单独写入 canonical timeline、prompt final output 或 live UI；原始 provider frame 仍完整保存在 `acp.raw.jsonl`。后续同一流出现可见文本时，必须保留累计正文中的原始字符并正常发布。UI 对历史旧 timeline 采用同一语义可见性规则隐藏空行和头像，不得按 Agent 类型或具体字符文案特判，也不得删除真实文本内部的格式字符、普通空格或换行
- provider diagnostic 必须写入 `acp.diagnostics.jsonl`，至少保存稳定 `code`、`level`、原始 message 和 provider/update 上下文；它不参与 assistant `final_text`、`final_outputs` 或聊天消息渲染
- 若当前节点未声明 `output`，则 `resultPayload` 可以为空或缺省；runtime 不因此报错

### ACP prompt 终态归约

ACP prompt 的协议结束与 Gold Band Provider 执行结果是两个不同领域：

- `stopReason` 只描述 `session/prompt` turn 为什么结束，不能单独证明 worker 成功。
- `session/update` 中的 fatal session 状态属于本轮 prompt 生命周期证据，必须进入 Provider 返回值，不能只落入 timeline 或 diagnostics。
- Provider 必须在一个集中归约函数中同时消费 `stopReason` 与结构化 terminal failure，不能让普通 worker、AI-DYNAMIC bootstrap/worker/merge/acceptance 各自推断。

归约优先级固定为：

1. 本轮出现 terminal failure，例如 Codex `_meta.codex.threadStatus.type = systemError`，无论 prompt response 是否返回 `end_turn`，都映射为 `ProviderRunStatus::Failure + RuntimeErrorInfo`。
2. `cancelled`、`interrupted`、`max_turn_requests`、`max_tokens` 映射为 `Interrupted`。
3. 等待用户输入和权限请求映射为对应暂停状态。
4. `refusal` 是明确业务拒绝，映射为 `Failure`，但不伪装成 provider runtime error。
5. 只有明确正常结束的 `end_turn` 且不存在 terminal failure 时才能映射为 `Success`。
6. 缺失或未知 `stopReason` 必须按 ACP 协议异常返回结构化 `RuntimeErrorInfo`，禁止默认成功。

Provider 只有在归约结果为 `Success` 或可接受中断结果时才提取 output artifact。Provider runtime error 必须先于 artifact/DSL 校验返回给 runtime；AI-DYNAMIC 不得把 provider 服务故障当成 proposal 格式错误进入 repair。

### Codex ACP 实现约束

- 内置 `codex-acp` preset 使用维护中的 `@agentclientprotocol/codex-acp`，不再使用会丢失 Codex event/item 身份的 `@zed-industries/codex-acp`
- `@agentclientprotocol/codex-acp` 的正常 agent text delta 必须携带 `messageId=itemId`；同一 ID 的 delta 累计，不同 ID 的 delta 分离
- 该 adapter 的正常正文通常携带 `messageId`，部分 warning 或兼容文本可能以无 ID `agent_message_chunk` 到达。所有 Agent 文本都进入 canonical timeline：无 ID 文本作为独立匿名连续段展示，不并入、截断或覆盖同时存在的稳定正文。warning 是否代表 prompt failure 只依据结构化 terminal failure，不按 adapter ID、英文前缀、具体文案或时间间隔过滤
- Codex `_meta.codex.error` 中 `willRetry=true` 仅记录为本轮候选错误信号；若后续恢复正常则不判失败。出现 `willRetry=false` 或 `_meta.codex.threadStatus.type=systemError` 时提升为本轮 terminal failure；后者应携带最近一次错误详情，供统一错误归一化识别 high demand、连接断开等可恢复异常
- 自定义 ACP adapter 或旧 adapter 不得套用“无 `messageId` 即 warning”的 Codex 专属规则，避免误隐藏正常回答

### `openSession(ref)`
根据 `worker-ref` 打开某个 provider 的原始会话。

说明：
- 这是 provider handoff 能力，不是 Gold Band runtime 内部的 `continue` 控制动作
- 调用它意味着 Gold Band 把交互控制权交还给 provider

### `buildContinueCommand(ref)`
用于构建 provider-specific 的继续/打开命令模板。

说明：
- 该能力既可用于 `open-session` 的 provider handoff，也可供 runtime 在内部恢复 provider 会话时使用
- 但具体使用它并不改变 `run continue` 仍属于 Gold Band runtime 控制动作这一事实

### B()：内部执行接口（实现类提供）
这是每个 provider implementation 真正需要实现的内部执行点。

其输入应是 prompt bundle，而不是路径型输入；其职责是：
- 接收 A() 组装好的 prompt bundle
- 消费已经分好层的 `systemPrompt` / `userPrompt`
- 在需要时配合模型按需访问 runtime 已暴露的冷数据文件索引
- 映射到 provider-specific 的 system/user prompt 或命令参数
- 发起真实调用
- 返回原始结果给 A() 做统一收尾

## 3. 最小能力分级

### Level 1：基础执行能力
- `describeProvider`
- `doctor`
- `runWorker`
- 最终结果返回
- 基础 `worker-ref`

### Level 2：会话可继续能力
- `openSession`
- `buildContinueCommand`
- `supports_continue_session`
- `supports_system_prompt`
- 可继续或可打开的原始会话引用

运行时规则：
- 若 workflow edge 显式请求 `session = continue`，但 provider 不支持 continue，则应在 DSL / runtime 校验阶段直接报错
- 若 provider 不支持 `openSession`，CLI `open-session` 应明确报错，而不是静默降级为其他动作

### Level 3：ACP 会话可视化能力
- ACP session events
- ACP raw frame / raw transcript
- tool call / plan / thought / permission / terminal 等原始 agent 过程展示
- 更丰富的 provider capability 暴露
- 外部 CLI handoff

## 4. 与其他文档的关系
- [CLI 规范](../interaction/cli.md)
- [Progress 规范](../interaction/progress.md)
- [Worker Invocation Contract](invocation.md)
- [Prompt Bundle 规范](prompt-bundle.md)
- [Worker Ref 规范](worker-ref.md)
- [Claude Code Provider 实现](implementations/claude-code.md)

## 5. 一句话总结

> provider adapter 的最小职责，是让 Gold Band 能描述 provider、诊断 provider、运行 worker、拿到最终结果、获取 worker reference，并在需要时继续或打开原始会话；其中 A() 归 runtime 所有，B() 归 provider implementation 所有。
