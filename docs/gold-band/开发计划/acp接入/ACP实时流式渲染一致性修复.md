# ACP 实时流式渲染一致性修复

## 问题

实时 ACP 会话中，部分 assistant 气泡会出现文本不完整、历史气泡为空、大段 thought 长时间不刷新后集中出现的问题。强刷后从 `acp.timeline.jsonl` 读取内容完整，说明落盘数据正常，问题位于实时事件进入前端状态的合并与 flush 链路。

## 方案

- 明确 UI 层 `textDelta` / `thoughtDelta` / `plan` 是稳定 timeline item 的累计快照，不是原始 token delta。
- 新增前端 ACP event reducer，统一 live event、session update、cache restore 的合并规则。
- session 等价判断改为比较事件窗口签名，覆盖所有已加载事件的 key、status、seq、content length 等关键字段。
- 非流式事件与 session 切换前同步 flush pending live stream，避免等待停止会话才补齐。
- 后端 live 节流按稳定 timeline item 边界 flush；text/thought/plan 切换稳定 id 时，旧 pending 快照必须先发出，不能被新 item 覆盖。
- 保留后端 timeline upsert 设计，补充 Rust 单测锁定累计内容与最新 patch 行为。

## 验收项

- partial 文本收到完整快照后实时替换为完整内容。
- 空气泡收到后续内容后实时显示，不需要停止会话。
- 非最后一条历史消息更新时，session update 不会被误判为等价并跳过。
- 旧的短快照不能覆盖新的长内容。
- text 后紧跟 plan/tool 时，text 的最终 live 快照不会被覆盖。
- `npm run web:test` 通过相关 ACP 测试。
- Rust ACP / Tauri view model 单测覆盖 streaming delta 与 timeline patch 行为。
