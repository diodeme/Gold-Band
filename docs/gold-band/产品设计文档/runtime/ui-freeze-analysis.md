# UI 冻结问题分析（待验证）

## 现象

Workflow 模式执行到最后一个节点（清理）完成后，UI 卡死在"拉起下一个节点"状态。除了拖动窗口外，所有交互无响应。指标日志显示清理节点已完成（`execution.completed` 已上报），但前端未收到或未处理 `RunCompleted` 事件。窗口拖动可用是因为 Windows DWM 在操作系统层处理，不经过 JS 事件循环——这是 JS 主线程阻塞的典型特征。

发生时间：2026-08-08，发生于 Workflow 会话（taskId=`d2e1e722d201495696fe700d0d390040`）。

## 可能原因（按可能性排序，暂未验证）

### 1. ACP 事件风暴导致前端 JS 主线程阻塞（最可能）

清理节点执行时，每个 ACP provider 输出帧（tool 调用、文本、permission 请求等）都通过 `emit_acp_update` → `app_handle.emit("gold-band://acp-session-updated", payload)` 实时推送到前端。清理节点如果执行了大量文件操作（删除、移动、格式化代码），会产生密集的 tool call 事件流。

前端 `ACPChatDialog` 组件收到每个事件后执行 DOM 更新。在没有虚拟化或节流的情况下，密集的 DOM 操作（Markdown 渲染、代码高亮、tool 卡片更新）会阻塞 JS 主线程。一旦阻塞，Tauri 事件队列中的 `RunCompleted` 事件无法被处理，UI 卡在 loading 状态。

**代码位置：**
- 后端推送：`src-tauri/src/commands.rs` 的 `emit_acp_update`（~line 2166）
- 前端接收：`web/src/components/acp/ACPChatDialog.tsx` 的 `subscribeAcpSessionUpdates` 回调

### 2. 每次 emit_acp_update 中的同步文件 I/O 累积

`emit_acp_update` 每次调用都会执行 `conversation_attempt_lifecycle_vm()`，该函数读取文件系统中的多个 JSON 文件来构建 payload。清理节点如果产生了大量 ACP 事件，这个函数会被高频调用，文件 I/O 累积后导致后台线程处理变慢。事件堆积在 Tauri 的 IPC 缓冲区中，最终一次性涌入前端，造成同样的 JS 阻塞。

**代码位置：**
- `src-tauri/src/commands.rs` 的 `emit_acp_update` 中 `conversation_attempt_lifecycle_vm()` 调用（~line 2175）

### 3. ACP provider 子进程未正确关闭

清理节点完成后，如果 ACP provider 子进程（如 claude-acp 进程）没有被正确终止，它可能持续向管道写入数据。`acp_live_update` 回调持续转发这些数据到前端，导致事件无限堆积。

**验证方式：** 重启后检查任务管理器中是否有遗留的 `claude` 或 `codex` 子进程。

## 验证方式（待执行）

1. 查看清理节点的 `raw.stream.jsonl` 文件大小——如果很大（几 MB），说明事件风暴是主因。
2. 重启后检查任务管理器是否有遗留的 provider 子进程。
3. 前端在 `subscribeAcpSessionUpdates` 回调中添加计数日志，观察每秒收到多少事件。

## 修复方向（待确认后实施）

- **前端节流**：对 ACP session 更新添加节流（如 100ms 内只处理最后一次），而不是每个事件都触发 DOM 更新。
- **后端 batch 合并**：在 `emit_acp_update` 中对高频事件做 batch 合并。
- **子进程清理**：确保节点完成后 ACP provider 子进程被正确终止。

## 关联代码

| 逻辑 | 代码位置 |
|---|---|
| ACP 事件推送 | `src-tauri/src/commands.rs` 的 `emit_acp_update` |
| ACP payload 构建（含文件 I/O） | `src-tauri/src/commands.rs` 的 `conversation_attempt_lifecycle_vm` |
| 前端 ACP 事件订阅 | `web/src/components/acp/ACPChatDialog.tsx` |
| 前端事件监听注册 | `web/src/api/desktop.ts` 的 `subscribeAcpSessionUpdates` |
| NodeCompleted 生命周期事件 | `src/app/orchestrator.rs` 的 `drive_from_node_with_initial_session` |
| RunCompleted 生命周期事件 | `src/app/orchestrator.rs` 的 `emit_run_completed_lifecycle_event` |