# 并行节点通用停止继续语义与 AI-DYNAMIC 落地方案

## 1. 背景

AI-DYNAMIC fan-out 会在一个外层 AI-DYNAMIC attempt 下创建多个内部 leaf 节点，每个内部节点都有自己的 ACP session。协议层面，`session/cancel` 只应该取消目标 session 的当前 prompt，不应该影响同一 adapter process 或同一 dynamic graph 下的其他 session。

当前实现把“停止某个动态内部节点”升级成了“暂停父 run / 外层 AI-DYNAMIC attempt / dynamic graph”。结果是主 dynamic loop 退出，兄弟节点虽然仍是独立 ACP session，但 runtime 调度层已经不再接收它们的结果，容易出现兄弟节点卡在 `running`、ACP snapshot 已 `cancelled`、会话 UI 显示“拉起下一节点中”等状态分裂。

这套语义不应只服务 AI-DYNAMIC。后续普通固定工作流如果支持多个节点并行，也应该复用同一套分层规则：单个 leaf attempt/session 独立 stop / continue，graph scheduler 继续收集并行结果，run 只在聚合条件满足时自动 paused。

## 2. 目标

1. AI-DYNAMIC 内部节点停止只影响目标 leaf attempt 和目标 ACP session。
2. 兄弟 leaf 仍为 `Ready | Running` 时，dynamic graph 和父 run 保持 `Running`。
3. 所有 active leaf 都被暂停或不再可自动推进，且剩余未完成 leaf 都是用户暂停的可继续节点时，dynamic graph / 外层 AI-DYNAMIC attempt / run 自动收敛为 `Paused + ProcessInterrupted`，不能显示为 `ErrorBlocked`。
4. 对 paused leaf 在会话中继续时，只恢复该 leaf，不恢复其他 paused sibling。
5. 侧边栏 run 列表增加右键“停止”，其语义是停止整个 run，等同 `pause_run`，不是 terminal kill。
6. 实现命名和 helper 按通用 leaf/run 聚合语义组织，避免写成 AI-DYNAMIC 私有补丁。

## 3. 生命周期分层

| 层级 | 负责对象 | 职责 |
|---|---|---|
| leaf attempt/session | 单个工作流 leaf attempt 与其 ACP session | 独立 stop / continue；持久化该 leaf 的 runtime 状态；向目标 ACP session 发送 `session/cancel` |
| graph scheduler | AI-DYNAMIC graph；未来普通并行 workflow graph | 调度 `Ready` leaf、接收 `Running` leaf 结果、物化后续节点；不因单个 leaf paused 退出 |
| run aggregate | 顶层 run / round / 当前外层 attempt | 聚合 graph 状态；仅在没有 active leaf 或用户显式停止整个 run 时写 paused |

## 4. 状态矩阵

| 操作 | leaf attempt/session | graph scheduler | run aggregate |
|---|---|---|---|
| 停止单个 leaf | 目标 leaf -> `Paused + ProcessInterrupted`，目标 ACP session 发 `session/cancel` | 继续处理其他 `Ready | Running` leaf | 保持 `Running`，除非没有 active leaf |
| 继续单个 leaf | 目标 leaf 从 `Paused` 重新进入 `Ready/Running`，使用原 ACP `sessionId` continue | 调度目标 leaf，不影响其他 paused sibling | 可保持 `Running`，或从整体 paused 恢复为 `Running` |
| 停止整个 run | 所有 active leaf -> `Paused + ProcessInterrupted`，各自发 `session/cancel` | graph 收敛为 paused | `Paused + ProcessInterrupted` |
| 所有 leaf 都停住 | 无 active leaf，剩余未完成 leaf 为用户暂停态 | graph 自动 `Paused + ProcessInterrupted` | run 自动 `Paused + ProcessInterrupted` |
| terminal kill | active leaf 走 release / close / kill 语义 | graph terminal killed | `Completed + Killed` |

## 5. 通用 helper 方向

本次可以先以 dynamic graph 数据结构为参数实现，但命名按通用 leaf 聚合语义设计：

- `dynamic_leaf_is_active(status)`：判断 `Ready | Running`。
- `refresh_dynamic_current_leaf_ids(graph)`：根据 leaf status 重新计算 `currentNodeIds`。
- `dynamic_graph_has_active_leaf(graph)`：判断 graph 是否仍有可运行或正在运行的 leaf。
- `pause_dynamic_parent_if_no_active_leaf(...)`：仅在没有 active leaf 时暂停 graph / outer node / round / run。

未来固定工作流并行化时，可把这些 helper 迁移为面向普通 workflow graph 的通用实现。

## 6. AI-DYNAMIC 停止落地

目标文件：

- `src/app/mod.rs`
- `src/app/orchestrator.rs`
- `src-tauri/src/commands.rs`

### 6.1 `pause_dynamic_attempt_runtime_state`

调整 `App::pause_dynamic_attempt_runtime_state`：

1. 不再无条件写父 `RunState.status = Paused`、`RoundState.status = Paused`、外层 `NodeState.status = Paused`、`graph.run.status = Paused`。
2. 先只更新目标 dynamic node：
   - `status = Paused`
   - `outcome = None`
   - `finished_at = now`
3. 同步写目标节点独立 `dynamic/nodes/<node>/node.json`，避免 `graph.json` 与节点文件分裂。
4. 对目标 attempt dir 保持 Stop 语义：取消 pending permission、发送 `session/cancel`、持久化 cancelled/interrupted ACP snapshot。
5. 重新计算 `graph.run.currentNodeIds`，把 paused leaf 移出 active 集合。
6. 如果 graph 仍有 `Ready | Running` leaf，保持父 run / graph running。
7. 如果 graph 已无 active leaf，调用聚合暂停 helper，把 graph / outer node / round / run 写成 `Paused + ProcessInterrupted`。

### 6.2 dynamic loop

调整 `apply_dynamic_execution_message`：

1. 某个 dynamic node 返回 `Paused` 或用户取消类 interrupted/cancelled 结果时，不无条件 `pause_dynamic_graph(...)`。
2. 先把该 leaf 写为 paused，再检查 graph 是否还有 active leaf。
3. 有 active leaf：继续 loop，等待兄弟节点结果。
4. 无 active leaf：再暂停 graph，并让父 run 自动收敛为 paused。
5. 真实 provider 错误仍可按 `ErrorBlocked` 暂停整个 graph，避免错误上下文下继续推进。

`outer_attempt_is_still_current_running` 仍只表达父 run 是否被整体暂停/关闭/终止。单节点 stop 不再修改父 run running 状态，因此不会误触发该 guard。

## 7. 单 leaf continue

`DynamicResumeOverride` 是 AI-DYNAMIC 内部 leaf 精确继续的唯一 re-arm 信号。规则为：

1. `submit_conversation_prompt` 发现选中的是 dynamic inner leaf，且该 leaf 为 `Paused + ProcessInterrupted` 时，即使父 run 仍 `Running`，也把提交目标判定为 `runtime-continue`。
2. graph paused 时继续复用 `run_continue_dynamic_inner_background`，但必须携带目标 leaf 的 `DynamicResumeOverride`。
3. graph running 时只把目标 leaf 从 `Paused` 改为 `Ready`，注入 `DynamicResumeOverride`，让 dynamic loop 调度该 leaf。
4. 如果磁盘显示 graph running 但进程内没有活跃 dynamic loop，应启动外层 AI-DYNAMIC drive，避免只改状态不执行。
5. continue 必须使用该 leaf 原 ACP `sessionId`，不得创建不相关的新 session。
6. 继续目标 leaf 前，后端必须扫描同一 dynamic graph 中所有 `Ready | Running`、`outcome=null` 且 ACP snapshot/session 已 `cancelled` 的 stale active leaf，把它们先收敛为 `Paused + ProcessInterrupted`，刷新 `currentNodeIds` 并同步写 `graph.json`、`dynamic_run.json` 与 `dynamic/nodes/<node>/node.json`，再只 re-arm 本次目标 leaf，避免 sibling 停留在 `running + ACP cancelled` 并显示“拉起下一节点中”。
7. 没有明确 inner leaf override 的父 run continue 不得批量 re-arm 普通 paused worker leaf；唯一例外是 `workflow-invocation` leaf，它代表一个已暂停 child run，父 run continue 可以只把这类 leaf 置回 `Ready` 以继续 child run。

## 8. ViewModel / Composer

目标文件：

- `src-tauri/src/view_models_conversation.rs`

要求：

1. dynamic inner leaf 的可继续性不能只依赖父 run `resumable`。
2. 父 run running、dynamic leaf paused/process-interrupted 时，该 leaf composer 应为 `runtime-continue` 输入态。
3. 父 run running、dynamic leaf running、ACP terminal 时，仍可显示合法的 `launching-next-node`。
4. 父 run 已 paused 时，陈旧的 leaf `running + ACP cancelled` 不得显示为 `launching-next-node`。
5. dynamic leaf 完成、暂停或被聚合暂停后，后端必须发出该 leaf 的 session/lifecycle update，前端收到 terminal/interactive 状态后刷新完整 run VM，避免选中 leaf 继续停留在旧的 `launching-next-node`。
6. dynamic child 已物化为 `Ready | Running` 但 ACP attempt/session 尚未创建时，VM 必须合成稳定的 pending leaf，显示为 runtime launching session，并进入 activeSessions，避免 session tree 只展示标题但右侧无会话状态。
7. 前端 auto-follow 必须区分用户手动查看历史 session 与 runtime 自然 terminal：manual 状态下新 active session 不抢焦点；auto 状态下当前选中自然 terminal 且用户仍在底部时，后续 child 首个 active/live event 可以切换过去。
8. 前端继续只消费后端 lifecycle/composer，不理解 ACP cancel/close/delete 协议细节。

## 9. 侧边栏 run 列表右键停止

目标文件：

- `web/src/components/conversation/ConversationSidebar.tsx`
- `web/src/components/conversation/ConversationShell.tsx`
- `web/src/App.tsx`

要求：

1. 在新 UI 会话侧边栏 run item 上接入 shadcn/ui `DropdownMenu`。
2. 右键打开菜单，菜单项为“停止”。
3. 仅 `run.status === "running"` 时可点击；其他状态 disabled。
4. 点击调用 `pauseRun(taskId, runId)`，语义是暂停整个 run，所有 active leaf 一起收敛为 `Paused + ProcessInterrupted`，已 completed 的 leaf 不被覆盖成 cancelled。
5. 菜单只挂在具体 run 行，不挂在任务/需求标题行；点击“停止”后立即关闭菜单，并在当前会话页展示停止遮罩，直到当前 run VM 刷新确认 run 非 running、active sessions 清空且选中 ACP session 已 terminal 后再消失。
6. 不新增 `killRun` 入口；terminal kill 必须与普通停止在 UI 上保持区分。
7. 如果 run 因所有内部 session 手动暂停而自动变为 paused，刷新后菜单自然 disabled。

## 10. 文档同步

本方案落地时同步维护：

- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/开发计划/生命周期整理/ACP停止语义与Adapter长连接开发方案.md`

## 11. 测试计划

### 11.1 Rust

1. fan-out graph 中两个 running worker，停止其中一个：目标 leaf paused，兄弟仍 running，父 run / graph 仍 running，`currentNodeIds` 不包含 paused leaf。
2. 两个 running worker 依次停止：第二次后父 run / round / outer node / dynamic graph 自动 paused/process-interrupted。
3. `apply_dynamic_execution_message` 收到某个 leaf paused 结果时，如果还有 sibling running，不暂停 graph。
4. graph running 状态下 `DynamicResumeOverride` 只 re-arm 目标 paused leaf。
5. 父 run running、dynamic leaf paused/process-interrupted 时，conversation lifecycle 输出 runtime continue composer。
6. 父 run running、dynamic leaf running、ACP terminal 时，仍可输出 `launching-next-node`。
7. 父 run paused 时，不输出 `launching-next-node`。

### 11.2 Frontend

1. running run 右键菜单展示“停止”且可点击。
2. paused run 右键菜单“停止”disabled。
3. 点击 running run 的“停止”调用 `pauseRun`，不调用 `killRun`。

### 11.3 手工验证

1. AI-DYNAMIC fan-out 两个节点运行中，停止其中一个，另一个继续输出并能完成。
2. 被停止节点在会话输入继续，只恢复该节点。
3. 两个 running 节点都手动停止后，父 run 自动变为 paused，侧边栏 run 菜单“停止”不可点。
4. 侧边栏 running run 右键“停止”会暂停整个 run，所有 active leaf 一起 paused。
