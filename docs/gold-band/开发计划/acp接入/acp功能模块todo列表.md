# ACP 接入功能模块 Todo 列表

## 当前实现状态

- 默认 provider 已切换为 `claude-acp`，新运行路径通过 ACP stdio adapter 发送 `initialize` / `session/new|load` / `session/prompt`。
- attempt 目录新增 `acp.session.json`、`acp.events.jsonl`、`acp.raw.jsonl`、`acp.diagnostics.jsonl`。
- Round 节点详情的会话 Tab 已切换为 ACP Dialog / Chat UI，legacy progress/raw stream 不再作为主会话视图。
- ACP Dialog / Chat UI 已接入 prompt-kit copy-in 组件：`ChatContainer`、`Message`、`PromptInput`、`Tool`、`ChainOfThought`。
- 权限请求可落盘为 pending event，并通过 Tauri `respond_acp_permission` 写入 response 文件供 provider loop 恢复。
- Plan 决策权限保留 composer 输入；用户提交自然语言反馈时自动选择继续规划并在当前 turn 完成后发送反馈。
- ACP prompt 会在发送 `session/prompt` 前持久化 synthetic `userTextDelta`，用于展示初始 prompt 和继续输入。
- Raw frames 诊断读取已从普通 session 刷新路径中解耦，普通刷新只统计行数；详情视图按 JSONL 行做后端分页、关键词检索、direction 和 kind/method 过滤，默认打开最新页，不把全量 `acp.raw.jsonl` 传给前端。

## 设计原则

- ACP 是 Gold Band 后续唯一的 Claude Agent / provider 接入路径。
- 不保留 Claude Code legacy CLI fallback，不做 ACP 与 legacy CLI 的双运行路径兼容。
- ACP 输入输出统一通过 Dialog / Chat UI 展示，不通过 terminal/log UI 承载主交互。
- Todo 按可独立执行的功能模块拆分，不按阶段拆分。
- 每个模块都需要明确输入、输出、边界和验收标准，便于单独认领、实现和验收。

## 相关文档

- 总体方案：`docs/gold-band/开发计划/acp接入/acp-first-refactor-plan.md`
- UI 规范：`docs/gold-band/开发计划/acp接入/acp-ui.md`
- Rust ACP client：`docs/gold-band/开发计划/acp接入/acp-rust.md`
- Jockey 参考：`docs/gold-band/开发计划/acp接入/jockey-claude-agent-sdk-bridge.md`

---

## 模块：ACP adapter 解析与启动

### 目标

为 Gold Band 提供 ACP-compatible adapter 的解析、启动和基础诊断能力。

### 输入

- provider id：`claude-agent-acp` / `claude-acp`
- workspace cwd
- adapter 配置
- 环境变量与认证状态

### 输出

- 可通信的 ACP stdio child process
- adapter 解析结果
- adapter diagnostics
- adapter 启动失败原因

### 主要任务

- 定义 adapter 解析顺序：托管目录、PATH、package runner。
- 启动 stdio child process。
- 记录 adapter binary / runner / cwd / env 摘要。
- 暴露 doctor 检查项。
- 将启动失败转换为结构化错误事件。

### 不做什么

- 不直接调用 Claude Code legacy CLI。
- 不从 terminal transcript 推导 UI 状态。
- 不把 package runner 后备解析等同于 legacy fallback。

### 验收标准

- 找不到 adapter 时能给出明确诊断。
- adapter 成功启动后可进入 ACP initialize。
- 文档和实现中没有把 Claude Code legacy CLI 作为运行路径。

---

## 模块：ACP session 生命周期

### 目标

管理 ACP session 初始化、创建、恢复、prompt、cancel 和结束状态。

### 输入

- ACP stdio connection
- PromptBundle
- worker-ref 中的 ACP session id
- continue / retry / cancel 请求

### 输出

- ACP session id
- session metadata
- prompt response
- stop reason
- lifecycle events

### 主要任务

- 执行 `initialize`。
- 根据 worker-ref 尝试 `session/load`。
- 不可恢复时创建 `session/new`。
- 将 PromptBundle 转为 ACP `session/prompt`。
- 支持 cancel 与 session 结束状态记录。
- 将 session id 写入 worker-ref。

### 不做什么

- 不让 ACP session 替代 Gold Band task / run / round / node canonical state。
- 不用 legacy CLI continue 恢复会话。

### 验收标准

- 新建和恢复 session 都能写入一致的 worker-ref。
- prompt 完成后能记录 stop reason 与 session metadata。
- cancel 能生成可诊断的结构化状态。

---

## 模块：ACP 事件归一化

### 目标

将 ACP 原始 session events 转换为 Gold Band UI 可消费的统一事件模型。

### 输入

- ACP `session/update`
- raw ACP frame
- adapter diagnostics
- session lifecycle events

### 输出

- `TextDelta`
- `ThoughtDelta`
- `ToolCall`
- `ToolCallUpdate`
- `Plan`
- `PermissionRequest`
- `ModeUpdate`
- `ConfigUpdate`
- `SessionInfo`
- `AvailableCommands`
- `SessionError`

### 主要任务

- 定义 ACP 原始事件到 UI event model 的映射规则。
- 定义 delta 合并、seq gap、乱序检测和未知事件处理。
- 定义 tool call 生命周期状态。
- 定义 permission request 与 tool call 的关联方式。
- 输出 ViewModel 可直接消费的数据结构。

### 不做什么

- 不把 ACP 事件蒸馏成 Gold Band 自研 `progress.events.jsonl`。
- 不让前端组件直接散落解析 ACP 原始 JSON。

### 验收标准

- 文本、思考、工具调用、权限请求、计划更新能分别渲染。
- UI 不依赖 legacy CLI 输出即可展示完整会话过程。
- 未识别事件不会破坏主会话流。

---

## 模块：Chat Dialog 容器

### 目标

提供承载 ACP 会话的对话框 / 抽屉容器，替代 terminal/log 主视图。

### 输入

- session ViewModel
- node / attempt context
- connection status
- waiting state

### 输出

- `ACPChatDialog`
- 会话头部
- 消息列表区域
- composer 区域
- 状态与诊断入口

### 主要任务

- 设计 `ACPChatDialog` 布局。
- 将会话 UI 嵌入 Round 节点详情 / 会话抽屉。
- 展示 session/provider/adapter/cwd/连接状态。
- 为 raw diagnostics 提供入口。

### 不做什么

- 不在主视图中展示原始 terminal transcript。
- 不把实现说明类文案暴露给普通用户。

### 验收标准

- 用户可以在一个对话容器中查看和继续 ACP 会话。
- 会话状态清楚，不需要理解 terminal 心智。

---

## 模块：Chat Composer 用户输入

### 目标

提供用户继续 ACP 会话、回答 agent 问题和提交下一次 prompt 的输入区。

### 输入

- 用户文本输入
- node waiting state
- current session id
- permission pending state

### 输出

- 用户消息
- 下一次 ACP `session/prompt`
- composer disabled / loading / error 状态

### 主要任务

- 使用 prompt-kit `PromptInput` 实现输入、发送、清空和等待态。
- 点击发送后立即清空输入并乐观追加右侧用户气泡；调起 ACP 到真实 `userTextDelta` 写入前展示“发送中”且不计时，真实用户消息写入后到首个非用户帧前切换为“处理中”并开始计时，首帧后按思考、工具调用或回复生成继续计时。
- 将自由文本回答映射为下一次 `session/prompt`，继续会话只发送用户文本，不追加固定内部续聊说明；system prompt 仅在新建 ACP session 时通过 `_meta.systemPrompt.append` 注入。
- 在 ACP client 发送前写入 synthetic `userTextDelta`，确保初始 prompt 与继续输入都可回放。
- 在 permission pending、adapter disconnected、node not ready 时禁用发送。
- 展示发送失败并允许重试。

### 不做什么

- 不通过 terminal stdin 发送用户输入。
- 不在同一个 prompt turn 内伪造非 ACP 标准的阻塞问答。

### 验收标准

- agent 以消息提问后，用户能在 composer 中回答并继续会话。
- composer 状态与 node/session 状态一致。

---

## 模块：流式文本消息渲染

### 目标

将 `TextDelta` 合并为稳定的 agent message bubble。

### 输入

- `TextDelta`
- message id / turn id
- seq / timestamp

### 输出

- streaming agent message
- completed agent message
- text render state

### 主要任务

- 合并连续 text delta。
- 避免一 token 一行。
- 保留和 tool call / plan / permission 的时间顺序。
- 支持 markdown 或代码块展示策略。

### 不做什么

- 不把 thought delta 混入最终回答正文。
- 不展示 stdout/stderr 作为普通 agent 文本。

### 验收标准

- 流式输出稳定、可读、不闪烁。
- 文本消息与结构化事件顺序一致。

---

## 模块：ThoughtBlock 思考内容

### 目标

以可折叠、弱化的方式展示 agent thought / reasoning。

### 输入

- `ThoughtDelta`
- thought id / turn id
- provider capability

### 输出

- `ThoughtBlock`
- folded / expanded state

### 主要任务

- 将 thought delta 聚合为 thought block。
- 使用 prompt-kit `ChainOfThought` 默认折叠展示。
- 标题展示由 ACP event timestamp 派生的思考耗时，不展示字符数。
- 标识其为 agent 内部过程。
- provider 不返回 thought 时隐藏该模块。

### 不做什么

- 不把 thought 作为 Gold Band runtime 判定依据。
- 不和最终文本回答混排。

### 验收标准

- 有 thought 时可展开查看。
- 无 thought 时 UI 不出现空状态噪音。

---

## 模块：ToolCallCard 工具调用

### 目标

用结构化卡片展示 ACP tool call 与更新。

### 输入

- `ToolCall`
- `ToolCallUpdate`
- terminal metadata
- file locations

### 输出

- `ToolCallCard`
- tool call status
- input / output 摘要
- raw input / raw output 展开内容

### 主要任务

- 使用 prompt-kit `Tool` 创建 tool call 卡片。
- 将 update 原地合并到同一卡片。
- 展示工具名、国际化状态、参数摘要、输出摘要。
- 卡片默认紧凑显示，展开后展示路径、查询等关键参数和输出。
- 聚合 terminal metadata、cwd、exit code、文件位置。

### 不做什么

- 不为每次 update 创建新卡片刷屏。
- 不把工具调用内容混入普通文本消息。

### 验收标准

- tool call 生命周期清晰可读。
- update 能准确刷新同一张卡片。
- 失败工具调用有明确错误状态。

---

## 模块：PermissionRequest 权限请求

### 目标

将 ACP `session/request_permission` 转为可操作的 Gold Band 权限 UI。

### 输入

- `PermissionRequest`
- permission options
- related tool call id
- Gold Band 权限策略

### 输出

- permission dialog / inline approval card
- `RequestPermissionResponse`
- permission audit record

### 主要任务

- 展示请求原因、相关 tool call、可选操作。
- 支持 allow / reject / always 等选项。
- 阻塞必须决策的会话继续执行。
- 记录用户选择和时间。

### 不做什么

- 不自动批准高风险操作。
- 不绕过 Gold Band runtime 权限边界。

### 验收标准

- 权限请求能阻塞并恢复 ACP 会话。
- 用户决策能回传 ACP adapter。
- 权限记录可用于排障。

---

## 模块：Plan / Mode / Config 状态

### 目标

展示 agent 计划、模式变化和配置变化，但不让它们替代 Gold Band workflow。

### 输入

- `Plan`
- `ModeUpdate`
- `ConfigUpdate`
- `AvailableCommands`

### 输出

- `PlanBlock`
- mode / config 系统提示
- available commands 展示

### 主要任务

- 展示 plan step title、status、nested entries。
- 将 mode/config update 显示为轻量状态提示。
- 展示可用命令或快捷动作。
- 明确 plan 与 Gold Band workflow edge 的边界。

### 不做什么

- 不用 ACP plan 决定 node outcome。
- 不用 mode/config update 改写 Gold Band canonical state。

### 验收标准

- 用户能看懂 agent 当前计划。
- UI 不把 ACP plan 误呈现为 Gold Band 工作流状态。

---

## 模块：SessionInfo / 会话恢复

### 目标

展示并维护 ACP session 的身份、连接、恢复和诊断状态。

### 输入

- `SessionInfo`
- worker-ref ACP session id
- adapter metadata
- reconnect / load result

### 输出

- session header state
- recovered / new / disconnected 状态
- worker-ref 更新

### 主要任务

- 展示 provider、adapter、session id、cwd、capabilities。
- 支持 session/load 的恢复状态提示。
- 记录恢复失败原因并创建新 session。
- 与 Gold Band node / attempt 状态保持一致。

### 不做什么

- 不把 ACP session id 当成 Gold Band attempt id。
- 不通过 legacy CLI session 恢复。

### 验收标准

- 用户能判断当前会话是新建、恢复还是断线。
- worker-ref 能支持下一次 continue。

---

## 模块：Raw frame / 诊断

### 目标

提供 ACP raw frame、session event 和 adapter diagnostics 的排障入口。

### 输入

- `acp.raw.jsonl`
- `acp.events.jsonl`
- adapter logs
- session metadata

### 输出

- `RawFrameViewer`
- event kind filter
- copy action
- linked diagnostics

### 主要任务

- 按 event kind 过滤 raw frame。
- 普通 session ViewModel 只统计 raw frame 行数，不解析完整 raw JSONL。
- Raw frame 详情按需读取，并设置读取大小边界，避免大文件阻塞会话主界面。
- 支持复制原始事件。
- 将 raw frame 关联到 message / tool call / permission request。
- 展示 adapter crash、auth required、timeout 等错误。

### 不做什么

- 不把 raw JSON 作为默认主 UI。
- 不通过 legacy CLI 日志补齐 UI 状态。

### 验收标准

- 排障人员能定位 ACP 原始事件。
- 普通用户默认不被 raw frame 打扰。

---

## 模块：Legacy CLI 清理

### 目标

移除或隔离 Claude Code legacy CLI 运行路径，避免 ACP 与 legacy 双路径并存。

### 输入

- 现有 provider 配置
- direct stream-json 调用点
- terminal transcript parser
- 旧 UI progress timeline

### 输出

- ACP-only provider 配置
- 待删除 legacy 清单
- 迁移后的文档和 UI 入口

### 主要任务

- 搜索 direct Claude Code CLI / stream-json 调用点。
- 标记需要删除、隔离或迁移的 legacy 逻辑。
- 确认新功能不依赖 legacy CLI fallback。
- 更新文档中的历史实现说明。

### 不做什么

- 不保留“出问题就切回 legacy CLI”的产品路径。
- 不新增兼容层维护两套 provider 语义。

### 验收标准

- provider 接入文档只描述 ACP 运行路径。
- legacy 相关内容只出现在历史背景、待清理对象或迁移说明中。

---

## 模块：集成验收

### 目标

验证 ACP-only provider、事件归一化和 Dialog / Chat UI 能组成完整闭环，并与主文档中的 MVP 测试计划保持一致。

### 输入

- ACP provider 配置
- 测试 prompt
- mock / real ACP session events
- Round 节点详情入口

### 输出

- 可运行的 ACP 会话
- 可查看的 Dialog / Chat UI
- 验收记录
- 问题清单

### 主要任务

- 验证 adapter 启动、initialize、session/new、session/prompt。
- 验证 text、thought、tool call、plan、permission、error 展示。
- 验证用户通过 composer 继续会话。
- 验证 raw diagnostics 可用。
- 验证不需要 legacy CLI fallback。
- 对齐 `docs/gold-band/开发计划/gold-band-mvp-plan.md` 中的总体验收口径。

### 不做什么

- 不用只跑单元测试替代 UI 交互验证。
- 不用 mock-only 结果证明真实 ACP adapter 可用。
- 不在本模块重复维护一套独立的 MVP 总测试计划。

### 测试计划对齐

- 主测试计划以 `docs/gold-band/开发计划/gold-band-mvp-plan.md` 的 `## MVP 验证标准` 为准。
- 本模块只补充 ACP 特有验证项，不重复定义通用的 `worker -> exec -> verify` 主链路标准。
- 记录 ACP 验收结果时，需要同时关联主流程状态、ACP 会话状态与 UI 展示结果。

### ACP 特有检查项

- 能成功启动 ACP adapter，并完成 initialize 与 session 创建。
- 用户能在 Gold Band 中发起、查看、继续 ACP 会话。
- ACP 输入输出以 Dialog / Chat UI 呈现，且 text、thought、tool call、plan、permission、error 展示完整。
- composer 可以继续发送消息并推动会话前进。
- raw diagnostics 可用，便于排查事件归一化或渲染问题。
- 全链路不依赖 Claude Code legacy CLI fallback。

### 验收标准

- 主文档中的 MVP 测试计划可作为总体验收依据。
- ACP 特有检查项全部通过后，才视为 ACP 集成验收通过。
- 若主链路成功但 ACP 会话展示、继续会话或诊断能力缺失，则本模块仍判定为未通过。
