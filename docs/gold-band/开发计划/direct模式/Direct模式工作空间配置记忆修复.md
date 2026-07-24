# Direct 模式工作空间配置记忆修复

## 问题定性

后端已经按 `conversationRunModes[projectId].directPreferences[agentType]` 保存 Direct 配置，缺陷位于前端状态所有权和写入时序：

- App 只维护一份跨 workspace 共享的 `conversationRunMode`，workspace 切换依赖异步响应替换整份快照。
- Composer 另存一份 workspace 选择，运行模式保存依赖 App render 时的默认 workspace 闭包，形成两个状态源。
- Agent、模型、权限连续触发 fire-and-forget 保存时，较早请求可能晚到并覆盖用户最终选择。
- 切回 workspace 的读取没有等待尚未完成的保存，可能读到旧值并停留在默认展示。

## 修正方案

- [x] App 将运行模式缓存改为 `Record<projectId, ConversationRunModeVm>`，workspace 是一级状态 key。
- [x] Composer 删除局部 workspace 副本，显示、提交和配置修改统一使用 props 中的当前 `projectId`。
- [x] `onRunModeChange` 显式携带 `projectId`；Direct 在 workspace 内继续按 `agentType` 保存模型和权限。
- [x] 同一 workspace 的保存严格按操作顺序串行执行，不同 workspace 可以并行。
- [x] workspace 重新加载前等待其保存队列完成；本地更新使旧读取响应失效，避免异步反灌。
- [x] Workflow、AUTO 和 Direct 共用同一 workspace 状态容器，不改变既有后端字段和持久化格式。

## 回归测试

- Direct/Workflow/AUTO workspace 隔离与保存队列 Web 定向测试：21 项通过。
- Rust `workspace + agent` state JSON roundtrip 测试：通过。
- `npm run web:build`：通过。
- `/chat` deep link：Direct、工作流、AUTO 切换与对应配置区域展示正常，无前端错误。
- 用户已完成桌面端多 workspace 切换恢复验收。
