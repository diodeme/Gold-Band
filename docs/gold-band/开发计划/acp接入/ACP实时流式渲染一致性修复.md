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
