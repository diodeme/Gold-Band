# ACP 实时流式渲染一致性修复

## 问题

实时 ACP 会话中，部分 assistant 气泡会出现文本不完整、历史气泡为空、大段 thought 长时间不刷新后集中出现的问题。强刷后从 `acp.timeline.jsonl` 读取内容完整，说明落盘数据正常，问题位于实时事件进入前端状态的合并与 flush 链路。

补充发现 Codex 旧适配器还会把 runtime Warning 和正常回答都降级成无 `messageId` 的 `agent_message_chunk`；Gold Band 的 active stream 又没有比较后续 stable ID，最终导致警告和回答融合为一个气泡。这是 adapter 信息丢失和 timeline accumulator 边界缺失共同造成的设计问题，不能通过警告英文文案过滤解决。

## 方案

- 明确 UI 层 `textDelta` / `thoughtDelta` / `plan` 是稳定 timeline item 的累计快照，不是原始 token delta。
- 新增前端 ACP event reducer，统一 live event、session update、cache restore 的合并规则。
- session 等价判断改为比较事件窗口签名，覆盖所有已加载事件的 key、status、seq、content length 等关键字段。
- 非流式事件与 session 切换前同步 flush pending live stream，避免等待停止会话才补齐。
- 后端 live 节流按稳定 timeline item 边界 flush；text/thought/plan 切换稳定 id 时，旧 pending 快照必须先发出，不能被新 item 覆盖。
- 保留后端 timeline upsert 设计，补充 Rust 单测锁定累计内容与最新 patch 行为。
- 将内置 Codex adapter 从 `@zed-industries/codex-acp` 替换为 `@agentclientprotocol/codex-acp`，并通过 settings schema v2 迁移现有默认安装配置。
- active text/thought/plan stream 同时保存 provider source identity；相同显式身份继续累计，不同显式身份或“有身份/无身份”切换时创建新 stream。两个连续无身份 delta 保留兼容性的本地连续流合并。
- 对确认来自 `@agentclientprotocol/codex-acp` 的无 `messageId` agent text 归一化为 `codex_acp.warning` 结构化诊断，保存到 diagnostics，不写入 timeline、assistant 最终正文或 UI 消息。
- legacy events 扫描窗口采用同一身份规则，避免历史读取层再次把不同 `messageId` 拼接。

## 验收项

- partial 文本收到完整快照后实时替换为完整内容。
- 空气泡收到后续内容后实时显示，不需要停止会话。
- 非最后一条历史消息更新时，session update 不会被误判为等价并跳过。
- 旧的短快照不能覆盖新的长内容。
- text 后紧跟 plan/tool 时，text 的最终 live 快照不会被覆盖。
- `npm run web:test` 通过相关 ACP 测试。
- Rust ACP / Tauri view model 单测覆盖 streaming delta 与 timeline patch 行为。
- 同一 `messageId` 的多个 delta 合并；不同 `messageId` 的相邻 text delta 分离。
- 无 provider identity 的传统 ACP 连续 token 仍可合并，不因边界修复退化成逐 token 气泡。
- 新 Codex adapter 的无 ID warning 进入结构化 diagnostics，有 ID 正常回答仍进入 assistant 正文。
- settings v1 中的旧 Codex npm 包规格迁移为新包，自定义非旧包参数不被覆盖。

## 实施状态

- 2026-08-10：修复持久化 `ConversationBranchRoute` 只恢复 `branchId` 的设计缺口，统一恢复并校验 `launchedAgentExecutionId` 与 `toolName`；后台 Agent 遗留启动确认迁移不再误报 completed，并以路由往返及 migration v2 单测固化。
- 2026-07-26：完成 Codex adapter preset 与 settings schema v2 迁移。
- 2026-07-26：完成 provider source identity 驱动的 stream 切分、Codex warning 诊断归一化和 legacy events 身份合并修复。
- 2026-07-26：Rust core 全量单元测试通过；Codex package 持久化迁移、warning 分类、同/不同消息身份与无身份 fallback 均已固化为接口级测试。
- 2026-07-26：Tauri 本项相关 view model 测试通过；桌面端全量测试另有既有 `timeline_permission_decision_replaces_pending_by_request_id` 失败，与本次文本 stream 身份改动无调用关系，单独留待 permission timeline 修复。
- 2026-07-26：前端 ACP/Agent 管理相关测试及生产构建通过；本地启动页面实测新增 Codex 表单显示 `-y @agentclientprotocol/codex-acp@latest`，浏览器控制台无 warning/error，测试服务已清理。本项实现关闭。

## 2026-07-31 Tooltip ref 更新环补充修复

### 根因

- 流式事件批量 flush 会连续重渲染 `ACPChatDialog` 及其顶部标题。
- 标题使用 shadcn Tooltip 的 `TooltipTrigger asChild`。项目原有 `radix-ui@1.4.3` 内部解析到 `@radix-ui/react-slot@1.2.3`，旧 Slot 在渲染期间重新创建组合 ref；React 19 在 trigger detach/attach 时调用 Tooltip 的内部 state ref，形成 `setRef -> setState -> render -> setRef` 更新环。
- 流式渲染只是稳定触发高频父级更新，消息 reducer、timeline 数据和 Markdown 内容不是本次错误的事实来源。

### 实现

- 将 `radix-ui` 统一升级为 `1.6.7`，使用其内部 `@radix-ui/react-slot@1.3.3` 与 `@radix-ui/react-tooltip@1.2.16` 稳定 ref 实现。
- 删除没有源码消费者的 scoped Radix 直依赖，并把 Button 的 Slot 入口统一为 `radix-ui`，避免聚合包与 scoped 包继续产生版本漂移。
- 引入 jsdom 测试环境，真实挂载会话标题 Tooltip；打开 trigger 后执行 80 次父级重渲染，锁定不会再次出现 maximum update depth。

### 验收

- `npm ls radix-ui @radix-ui/react-slot @radix-ui/react-tooltip --depth=1` 只显示一个 Radix primitive 所有权入口。
- Tooltip 流式重渲染测试、Button ref、ACP session header 与 conversation header 测试通过。
- 前端全量单元测试与生产构建通过。
- `npm run dev` 启动后在实际会话流式输出期间悬停标题，控制台不再出现 `Maximum update depth exceeded`。

## 2026-07-31 ACP 流式消息自动贴底生命周期修复

### 根因

- 2026-07-22 引入的 Markdown presentation controller 会在收到 timeline 累计快照后，继续以约 32ms 的局部帧推进可见前缀并改变真实 DOM 高度。
- ACP 原滚动实现只在 `timeline` 引用变化时执行一次 `scrollTop = scrollHeight`，没有观察 presentation 后续布局增长；用户重新滑到底部也只会瞬时恢复 `pinToBottomRef`，下一帧内容增长后再次脱离。
- 消息贴底、session auto-follow、历史分页和程序滚动分别维护布尔 ref，生命周期所有权重复，现有单测又只分别覆盖 live flush 与 Markdown presentation，未覆盖二者的 DOM 集成。

### 实现

- ACP 主消息区迁移到现有 prompt-kit `ChatContainer` 与 `use-stick-to-bottom`，由内容根节点 `ResizeObserver` 持续感知真实高度；保留 timeline canonical / presentation visible offset 分层，不回退实时 Markdown。
- 删除 `pinToBottomRef`、`programmaticScrollRef`、本地 `isAtBottom` 和 timeline 自动写 `scrollTop` 的旧入口；贴底、用户向上逃逸、回到底部恢复统一由组件状态机管理，并把 `onAtBottomChange` 继续传给 run 级 session auto-follow。
- 历史向上分页在请求前调用统一 `stopScroll()`，继续使用可见 timeline item 锚点补偿 prepend 高度；向下分页只保留独立的接近底部阈值，不再参与贴底状态判定。
- interaction quiet window 继续负责合并高频 live event；由 ResizeObserver 产生的程序滚动不再被误判为用户交互。

### 验收

- 新增 jsdom DOM 回归测试，真实触发内容 `ResizeObserver`，固化“内容持续增长时贴底、用户离底后保持位置、用户回到底部后恢复持续跟随”。
- ACP live flush、Markdown presentation、session follow、主题滚动条与新 ChatContainer 定向测试共 41 项通过。
- 前端全量 86 个测试文件、576 项测试通过；`npm run web:build` 生产构建通过。
- 浏览器调试模式通过 `/chat/projects/default/tasks/mock-task/runs/run-052` deep link 验证：消息区保持单一滚动 viewport、内容根节点高度正确、composer 布局无回归，控制台无 warning/error。

## 2026-08-04 流式累计快照与右侧工作区内存修复

### 根因

- task-159 现场 `acp.raw.jsonl` 有 6021 帧，其中 5209 个 thought chunk、534 个 message chunk，单 chunk 最大仅 16 字符；最终 timeline 只有 96 条，说明异常内存不是业务数据量，而是高频累计快照的 UI 生命周期。
- 每条历史消息、Agent link、Turn 文件卡片和 Markdown 文件链接 Provider 都订阅了包含 tabs、activeTab、requestedOpen、width 的完整右侧工作区 context。浏览文件会使整棵历史消息树失效，静态 Streamdown 被重复解析。
- 125ms live flush 虽已有按 identity 合并的 Map，但每批再创建一个 `startTransition`；文件交互持续占用前台时，过期 transition 可同时持有多份累计字符串和对应 React 子树。

### 根因修复

- `RightWorkspaceProvider` 拆为可变 state context 与稳定 commands context；消息侧只消费 `scopeKey/projectId/openResource/getResource`，文件链接用 `getResource(key)` 调用时读取已打开资源，不订阅 tabs。
- prompt-kit `Markdown` 增加 memo 静态边界；当文本与 streaming 状态不变时，侧栏状态变化不得重新进入 Streamdown。
- live event buffer 显式收敛为 `AcpLatestWinsEventBuffer`，每个 stream/tool identity 只保留最新累计快照；单一 timer drain 后同步合并 React state，删除 per-flush transition 队列。lifecycle/terminal 事件仍先同步 flush，保持顺序。

### 固化验收

- 6021 帧分布回放验证最终累计内容、pending 上限由 24 个消息 stream 与 35 个 tool identity 约束、scheduled/in-flight publish 上限为 1。
- jsdom 接口测试连续打开 15 个文件并切换 activeTab/width，稳定 commands 消费者只渲染一次；8 条历史 Markdown 的 Streamdown 解析次数不增加，Markdown 文件链接 handler 引用不变。
- 自动化测试后继续执行真实 task-159 deep link 回放，并以 baseline（进入会话前）、target（流式并打开 15 文件后）、final（关闭文件/离开会话并 GC 后）堆快照交给 memlab；验收 detached DOM、CodeMirror EditorView、Streamdown/Markdown 子树和累计字符串保留链能够释放。
- 实施复测使用可重复的 `web/tests/performance/acp-workspace-memlab.cjs`：baseline/target/final 为 20.2MB / 26.0MB / 25.6MB。final 相对 baseline 的主要常驻差异是首次打开编辑资源后加载的模块、parser 与语言描述缓存；memlab 泄漏 trace 中没有 CodeMirror EditorView、Streamdown/Markdown 或累计消息字符串。首轮发现的 Tab strip ResizeObserver 闭包簇修复后消失；剩余 3 簇合计约 34.8KB，retainer 分别为 Chromium `SVGDocumentExtensions` 文档缓存与 DevTools console global handle，作为浏览器内部观测噪声记录，不误报为 0。
