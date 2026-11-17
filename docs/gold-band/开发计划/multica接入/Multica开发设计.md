# Multica 接入：开发设计文档

> 本文档是「码灵客户端接入 Multica」的**开发设计文档**，面向后续代码实现，给出每个改造点的文件落点、数据结构、函数签名、SQL、流程与验收。
>
> **上游输入**（仅阅读，作为本设计的依据）：
> - `.claude/design/multica/Multica接入方案.md` —— 架构、产品形态、接口契约、设计决策（**接入方案 = 为什么/做什么**）。
> - Multica 源码 `E:\MercurjiangWorkSpace\IdeaProjects\multica-main\multica-main\server`（截至当前 main）。
> - 码灵源码 `Gold-Band`（当前 main）—— 基础设施已逐文件核实 file:line。
>
> **本设计 = 怎么做**：把接入方案落到可实现的模块/文件/数据/接口/流程/验收。凡与接入方案存在事实性出入处，均显式标注「⚠️ 修正」并给出依据。
>
> **阅读对象**：实现本特性的开发者。按里程碑顺序（M0→M5）实现即可。每个新增能力标注「复用 X 模式（file:line）」，照套不重复造轮子。

---

## 0. 实现边界与阶段总览

### 0.1 三大改造域

| 域 | 仓库 | 性质 | 里程碑 |
|---|---|---|---|
| **multica server** | `multica-main/server` | 源码改造（1 项：selective claim；登录复用原生，不改） | **M0** |
| **码灵客户端（Rust）** | `Gold-Band/src-tauri` + `Gold-Band/src` | 新增 `multica/` 模块 + 配置/命令扩展 | **M1–M4** |
| **码灵前端（TS/React）** | `Gold-Band/web` | API 四层扩展 + 设置页 + 会话模式远程任务列表 | **M5** |

### 0.2 里程碑依赖图

```
M0  server 改造（仅 selective claim；登录复用 multica 原生）   ← 可独立先行，码灵联调依赖它
        │
        ▼
M1  凭证与登录（config + client 的浏览器登录(localhost callback)/PAT/me/workspaces）
        │
        ▼
M2  注册与心跳（register 全量 + 心跳 + recover-orphans）
        │   └─（Req D 演示反馈调整）心跳由「执行期」改为「常驻」：建立连接后即对全部已连接
        │      workspace 的 runtime 持续心跳，不再仅 claim→complete 期间。同循环承载 prepare-lease 续期。
        │
        ▼
M3  任务列表与领取（pending 只读列表 + selective claim + start）
        │
        ▼
M4  任务执行与终态（bridge：会话事件流转译 + complete/fail 重试 + PinTaskSession）+ 失败恢复（会话级续跑 + rerun）
        │
        ▼
M5  前端 UI（设置页连接/添加工作空间 + 会话模式远程任务管理 + 事件 + i18n）
```

> M0 与 M1–M5 分属两个仓库，M0 可先合入 multica main；码灵侧用本地 multica 分支联调。

### 0.3 命名与术语规范（强制）

multica 实体与码灵自有概念同名但语义不同，码灵侧一律加前缀区分（见接入方案 3.2.1）：

| multica 实体 | 码灵侧命名 | 码灵自有同名概念（不得混淆） |
|---|---|---|
| runtime（执行目标实例） | `multica_runtime_id` | 本地工作流运行时 |
| task（issue 派生远程任务） | `remote_task` | 本地工作流任务（preset→task→run） |
| agent（workspace 智能体配置） | `multica_agent` | ACP agent / provider |
| workspace（SaaS 多租户边界） | `multica_workspaces`（**只含 provider，不绑本地目录**）/ `active_workspace_id` | 码灵本地工作目录（`conversation_workspaces` 条目，`workspace_path`） |

> **workspace 与本地目录的关系（绑定模型下沉到任务级，M5-z）**：每个添加的 multica workspace **只绑 provider，不绑本地目录**。multica workspace 决定「任务从哪个团队派来」；本地工作目录决定「任务在哪执行」——两者通过**任务级** `local_project_id` 关联（在执行时由 composer 下拉选，随 `start_multica_conversation_run` 写入 `ActiveRemoteRun` / `MulticaCompletedTask`），不再在 workspace 级绑定。下文凡指 multica 侧概念一律带前缀。
>
> **三层 workspace 勿混**：① multica workspace（远端，`multica_workspaces`，只绑 provider）；② 码灵本地工作目录（`conversation_workspaces`，**执行时**由 composer 下拉选并写入任务级结构）；③ 会话实例（一次会话打开的目录）。某次 multica 任务的「执行落点」= 该任务 `ActiveRemoteRun.local_project_id` 在 `conversation_workspaces` 解析出的 ② 的 `workspace_path`（不再走 workspace 级固定绑定）。

---

## 1. multica server 侧改造（里程碑 M0）

### 1.0 改造总览

**唯一一项源码改造（selective claim）**；**登录复用 multica 原生邮箱登录**（浏览器 + localhost callback，server 零改动，见接入方案 3.2.6）；其余需求（任务列表、心跳、recover-orphans、rerun、PinTaskSession）全部复用 multica 现有接口，不动源码：

| # | 改造 | 动因 | 文件数 |
|---|---|---|---|
| 一 | selective claim `POST /api/daemon/runtimes/{rid}/tasks/{tid}/claim` | 现有 claim 只能 FIFO 派下一条，不能指定 task_id，不满足「点哪领哪」 | 4 文件 + sqlc |

### 1.1 登录复用（无 server 改造）

> **本节无源码改造**——登录直接复用 multica 原生邮箱登录（浏览器 + localhost callback），server 侧零改动。码灵侧实现见 2.3（client.rs 浏览器登录链路）与 4.1（首启登录流程）。保留此节是为说明「登录不属于 M0 的 server 改造额度」。

**机制**（详见接入方案 3.2.6 / 4.2 A0）：码灵起临时本地 HTTP server（`127.0.0.1:<port>`）→ 打开系统浏览器到 `<MULTICA_APP_URL>/login?cli_callback=http://127.0.0.1:<port>/callback` → 用户邮箱登录 → multica Web 校验 `cli_callback` 白名单（`validateCliCallback`，已放行 localhost / 127.0.0.1 / RFC1918）→ 302 回跳 `?token=<JWT>` → 码灵收 JWT 调 `POST /api/tokens` 换 PAT。CLI 同款实现参考 `server/cmd/multica/cmd_auth.go:235-358`。

---

### 1.2 selective claim（点哪领哪）

#### 1.2.1 ⚠️ 设计修正声明（相对接入方案 3.1）

接入方案 3.1 把 selective claim 描述为「照搬 `ClaimTaskByRuntime`，仅把 FIFO 换成按 task_id」。核实源码后发现**三处需修正**：

| # | 接入方案 3.1 表述 | 实际源码（已核实） | 修正 |
|---|---|---|---|
| ① | 示意 SQL `WHERE id=@task_id AND runtime_id=@rid AND status='queued'` | `ClaimAgentTask`(agent.sql:508) 含 **per-(issue,agent) 串行化 `NOT EXISTS` 守卫** + `prepare_lease_expires_at` | selective claim SQL **必须保留串行化守卫 + lease**，否则点选绕过并发不变量 |
| ② | handler「复用 `buildClaimedTaskResponse`」 | `ClaimTaskByRuntime`(daemon.go:2512-2664) 收尾链含 **6 步**（见 1.2.4） | handler 必须**整条复制**收尾链，只换 service 调用 |
| ③ | 「service 层走 `FinalizeTaskClaim`」 | `FinalizeTaskClaim`(task.go:2544) 在 **handler 层**调用（需 `buildClaimedTaskResponse` 产出的 deliveredCommentIDs） | service 只负责 claim + 广播；Finalize 留 handler |

> 依据：`ClaimAgentTask` agent.sql:508-546；`ClaimTaskForRuntime` task.go:2427-2537；`ClaimTaskByRuntime` daemon.go:2512-2664；`agent_task_queue.runtime_id` 列存在（migration 004:18 `ADD COLUMN runtime_id UUID`，有 FK）。

#### 1.2.2 SQL query（保留串行化守卫）

**`server/pkg/db/queries/agent.sql`** 新增，**对照 `ClaimAgentTask`(agent.sql:508) 原文移植 `NOT EXISTS` 守卫块**，仅把选行从「ORDER BY+LIMIT 1+FOR UPDATE SKIP LOCKED」换成「id+runtime_id 直定」：

```sql
-- name: ClaimSpecificQueuedTask :one
-- Selective claim: claim an EXACT task by id (user-picked from the pending list),
-- gated on runtime ownership and the SAME per-(issue, agent) serialization that
-- ClaimAgentTask enforces. Picking a specific task must NOT bypass the
-- "no concurrent tasks for the same issue+agent" invariant.
-- 实现：对照 ClaimAgentTask (agent.sql:508) 原样移植 NOT EXISTS 守卫块，
-- 仅将选行改为 id = @task_id AND runtime_id = @runtime_id。
UPDATE agent_task_queue
SET status = 'dispatched',
    dispatched_at = now(),
    prepare_lease_expires_at = now() + make_interval(secs => @prepare_lease_secs::double precision)
WHERE id = @task_id
  AND runtime_id = @runtime_id
  AND status = 'queued'
  AND NOT EXISTS (
      -- ↓↓↓ 原样复制 ClaimAgentTask (agent.sql:508-546) 的 NOT EXISTS 块 ↓↓↓
      SELECT 1 FROM agent_task_queue active
      WHERE active.agent_id = agent_task_queue.agent_id
        AND active.status IN ('dispatched', 'running', 'waiting_local_directory')
        AND (
          (agent_task_queue.issue_id IS NOT NULL AND active.issue_id = agent_task_queue.issue_id)
          OR (agent_task_queue.chat_session_id IS NOT NULL AND active.chat_session_id = agent_task_queue.chat_session_id)
          OR (
            agent_task_queue.issue_id IS NULL AND agent_task_queue.chat_session_id IS NULL
            AND agent_task_queue.autopilot_run_id IS NULL
            AND active.issue_id IS NULL AND active.chat_session_id IS NULL AND active.autopilot_run_id IS NULL
          )
        )
  )
RETURNING *;
```

> ⚠️ 上面是**参考骨架**，实现时**逐字对照 `ClaimAgentTask`(agent.sql:508-546) 的 NOT EXISTS 块**移植，列名/状态值以源码为准。

#### 1.2.3 service 层

**`server/internal/service/task.go`** 新增 `ClaimSpecificTask`，与 `ClaimTaskForRuntime`(:2427) 并列于 `TaskService`。**不做 reclaim-stale / empty-cache / 列候选循环**（指定 task 无需），命中后复用与 `ClaimTask`(:2370-) 相同的 dispatch 侧效：

```go
// ClaimSpecificTask claims an exact queued task by id for a runtime, enforcing
// the same per-(issue,agent) serialization as ClaimAgentTask. Returns:
//   - (*task, nil)   claimed
//   - (nil, nil)     task 不存在/不属该 runtime/非 queued/串行化冲突 → 视为无可领
//   - (nil, err)     DB 错误
func (s *TaskService) ClaimSpecificTask(ctx context.Context, runtimeID, taskID pgtype.UUID) (*db.AgentTaskQueue, error) {
    var claimed *db.AgentTaskQueue
    err := s.runInTx(ctx, func(qtx *db.Queries) error {
        task, err := qtx.ClaimSpecificQueuedTask(ctx, db.ClaimSpecificQueuedTaskParams{
            TaskID:           taskID,
            RuntimeID:        runtimeID,
            PrepareLeaseSecs: prepareLeaseDuration.Seconds(),
        })
        if err != nil {
            if errors.Is(err, pgx.ErrNoRows) {
                return nil // 无可领（含串行化冲突）
            }
            return fmt.Errorf("claim specific task: %w", err)
        }
        t := task
        claimed = &t
        return nil
    })
    if err != nil || claimed == nil {
        return nil, err
    }
    s.captureTaskDispatched(ctx, *claimed)   // 复用：与 ClaimTask 一致
    s.ReconcileAgentStatus(ctx, claimed.AgentID)
    s.broadcastTaskDispatch(ctx, *claimed)
    return claimed, nil
}
```

#### 1.2.4 handler 层（整条复制收尾链）

**`server/internal/handler/daemon.go`** 新增 `ClaimSpecificTask`，**完整复制 `ClaimTaskByRuntime`(daemon.go:2512-2664) 的收尾链**，**唯一改动**：把 `ClaimTaskForRuntime(ctx, runtimeID)` 换成 `ClaimSpecificTask(ctx, runtimeID, taskID)`，nil → 404（区别于 FIFO 的 `{"task":null}`）。

复制的收尾链（**逐项保留，不得删减**）：

| 步 | 调用 | 源码锚点 | 作用 |
|---|---|---|---|
| 1 | `requireDaemonRuntimeAccess` | daemon.go:2540 | 校验 runtime 归属 + 取 runtime（workspace_id / owner_id） |
| 2 | `h.TaskService.ClaimSpecificTask` | （新增 service） | **唯一改动点**：按 task_id 领取；nil → 404 |
| 3 | `repairStaleCommentPlanIfNeeded` | daemon.go:2562 | comment-backed task 的 stale 修复 |
| 4 | `buildClaimedTaskResponse` | daemon.go:2579/1590 | 构造完整 Task 响应 payload |
| 5 | `runtime.OwnerID` 校验 + `GenerateAgentTaskToken`+`HashToken` | daemon.go:2606-2628 | mint `mat_` task-scoped token（MUL-3292） |
| 6 | `FinalizeTaskClaim` + 失败 `RequeueTaskAfterClaimFailure` | daemon.go:2629-2648 | 事务内 persist token + comment receipt；失败回滚 |

```go
func (h *Handler) ClaimSpecificTask(w http.ResponseWriter, r *http.Request) {
    runtimeID := chi.URLParam(r, "runtimeId")
    taskID := chi.URLParam(r, "taskId")
    runtime, ok := h.requireDaemonRuntimeAccess(w, r, runtimeID)
    if !ok { return }
    runtimeWorkspaceID := uuidToString(runtime.WorkspaceID)
    task, err := h.TaskService.ClaimSpecificTask(r.Context(), parseUUID(runtimeID), parseUUID(taskID))
    if err != nil { writeError(w, http.StatusInternalServerError, "..."); return }
    if task == nil { writeError(w, http.StatusNotFound, "task not claimable"); return } // 404
    // ↓↓↓ 以下整段复制 ClaimTaskByRuntime (daemon.go:2562-2664)，原封不动 ↓↓↓
    //   repairStaleCommentPlanIfNeeded → buildClaimedTaskResponse →
    //   OwnerID 校验 → GenerateAgentTaskToken → FinalizeTaskClaim（失败 Requeue）
}
```

#### 1.2.5 路由注册

**`server/cmd/server/router.go`** daemon 路由组（现有 `r.Post("/runtimes/{runtimeId}/tasks/claim", h.ClaimTaskByRuntime)` 旁）新增：
```go
r.Post("/runtimes/{runtimeId}/tasks/{taskId}/claim", h.ClaimSpecificTask)
```

#### 1.2.6 错误码

| HTTP | 场景 | 码灵侧错误码 |
|---|---|---|
| 401/403 | PAT 无效 / runtime 不属本用户 | `multica.auth-failed` |
| 404 | task 不存在/不属该 runtime/非 queued/串行化冲突 | `multica.task-not-found` |
| 500 | claim/finalize 失败 | `multica.network-failed`（重试用尽后） |

#### 1.2.7 验收

- 单测：queued + 同 runtime + 无并发 → 200 + `{task:{...,auth_token}}`；串行化冲突（同 issue 已有 dispatched）→ 404；非 queued → 404；跨 runtime → 404。
- 并发：两个并发 selective claim 同一 task → 恰一个 200、一个 404（SQL 原子 UPDATE 保证）。
- 集成：码灵 pending 列表点某条 → selective claim 200 → start → 完整执行链路通。

#### 1.2.8 落地状态（本轮，multica-webank + 码灵 client）

- ✅ SQL `ClaimSpecificQueuedTask`(agent.sql) — 逐字保留 `ClaimAgentTask` 串行化 `NOT EXISTS` 守卫 + `prepare_lease_expires_at`，候选过滤换精确 `(task_id, runtime_id, 'queued')`；`sqlc generate` v1.31.1 生成通过。
- ✅ service `ClaimSpecificTask`(task.go) — runInTx + dispatch 三件套（captureTaskDispatched/ReconcileAgentStatus/broadcastTaskDispatch），不做容量/reclaim/候选循环；ErrNoRows→nil（handler 映射 404）。
- ✅ handler `ClaimSpecificTask`(daemon.go) — 完整复刻 `ClaimTaskByRuntime` 六步收尾链，唯一改动 FIFO→specific + nil→404；handler 顶部注释标注「复制自 ClaimTaskByRuntime」便于升级 rebase（见 §1.3）。
- ✅ route `POST /runtimes/{runtimeId}/tasks/{taskId}/claim`(router.go)。
- ✅ **验收修正**（§1.2.7「串行化冲突同 issue 已有 dispatched」）：实测发现同 (issue,agent) 的 queued+dispatched 组合**已被 DB 唯一索引 `idx_one_pending_task_per_issue_agent`（覆盖 `('queued','dispatched')`）在 INSERT 阶段阻止**，不靠守卫；守卫独立于索引的价值在于覆盖索引管不到的 `'running'`/`'waiting_local_directory'` 状态——对应集成测 `TestClaimSpecificTask_SerializationConflictReturns404` 用 running 构造冲突。
- ✅ 4 个集成测（成功 200+auth_token+thread_name / 非 queued→404 / 跨 runtime→404 / running 串行化冲突→404）全过；现有 claim/pending/batch-claim 零回归。码灵侧 `remote_task_reads_thread_name_wire_key` 锁定 `thread_name→title` wire 契约，desktop multica 53 测全过。

#### 1.2.9 pending 列表 thread_name 补全 4 来源（M5-o，multica-webank）

- **问题根因**：§1.2.8 落地时，`ListPendingTasksByRuntime`(daemon.go) 的 pending 列表 `thread_name` 回填**只覆盖 issue 来源**（`t.IssueID.Valid`→`GetIssue.Title`），漏了 chat / autopilot / quick-create 三类——而 claim 路径 `buildClaimedTaskResponse` 覆盖全部 4 来源（任务来源互斥：issue XOR chat XOR autopilot XOR quick-create）。导致后三类 pending 任务在码灵远程列表里无标题（只显 "queue"）。
- **修法（修根因非补丁，§1.2.8 的不完整实现补全）**：新增聚焦 helper `(h *Handler) resolvePendingThreadName(ctx context.Context, t db.AgentTaskQueue) string`，按任务来源**互斥分支**镜像 claim 路径的名字源：
  - `t.IssueID.Valid` → `GetIssue(t.IssueID).Title`
  - `t.ChatSessionID.Valid` → `GetChatSession(t.ChatSessionID).Title`
  - `t.AutopilotRunID.Valid` → `GetAutopilotRun` → `GetAutopilot(ap.AutopilotID).Title`（`AutopilotRun.AutopilotID` 既有模式）
  - `task.Context` 含 `service.QuickCreateContext`（`type == QuickCreateContextType`）→ `qc.Prompt`
  - 全空 → `""`
- 替换 `ListPendingTasksByRuntime` 的 issue-only 块为 `resp[i].ThreadName = h.resolvePendingThreadName(r.Context(), t)`。**不重构** 600 行 `buildClaimedTaskResponse`（thread_name 交织在 squad/repos/chat 投递里，抽出高风险低收益）——helper 是 pending 列表专用的名字解析对应物，注释明确这层关系。
- 验证：新增 3 集成测 `TestListPendingTasksByRuntime_{Chat,Autopilot,QuickCreate}ThreadName`（复用 seed helpers，断言 pending 列表返回对应来源名字）全过；现有 claim/pending 零回归。
- **部署提示**：本改动在 multica-webank server，需 rebuild + 重启 webank server 才生效；码灵侧 `vm.rs::from_remote` 的 task id 兜底（见 §2.x）保证即便 webank 未重建，列表也显示 task id 而非空白行。

---

### 1.3 升级 rebase 成本

- **登录**：无 server 改动，rebase 零成本。
- **selective claim**：与 `ClaimTaskByRuntime` 收尾链强相关——若 multica 升级改动了该收尾链，需同步 rebase `ClaimSpecificTask`。建议 handler 顶部注释标注「复制自 ClaimTaskByRuntime @ <commit>」，升级时 diff 该锚点。

---

## 2. 码灵客户端侧实现（里程碑 M1–M4）

### 2.1 模块划分与边界

新增 `src-tauri/src/multica/`（与 `metrics.rs` / `feedback.rs` / `updater.rs` 平级）。**multica 业务逻辑**（HTTP / 心跳 / claim / 重试 / 状态机）是 src-tauri 薄壳层，依赖 reqwest + tauri state + AppHandle，状态上报通过 `lifecycle_bus` 桥接（项目既定扩展模式，见 `register_lifecycle_subscribers` commands.rs:481-497）；但**会话执行复用 `gold-band` 库层 App 公开 API**（`create_task_from_requirement` / `run_start_background` / `worker_ref_show` / `run_continue_background_with_config_overrides`，均公开），不新造 runtime、不碰 lifecycle 契约；库层唯一改动是把会话 VM 私有 Direct/Auto workflow 构造上提到 `gold_band::dsl::presets` 公开复用（详见 2.5）。

| 子模块 | 职责 | 复用的现成模式（file:line） |
|---|---|---|
| `multica/client.rs` | reqwest client + 指数退避重试 + 全部 multica HTTP 调用 | `metrics::send_heartbeat`(metrics.rs:180-208) / `send_node_metrics_batch`(298-322) reqwest 用法；`normalize_metrics_base_url`(90-114) URL 容错 |
| `multica/config.rs` | 配置 VM + 读写 + `pat_set` getter + `multica_settings()` 聚合（**M5-z：不再做绑定查找**——`binding_for_multica()` 已删除，本地目录改由任务级 `ActiveRemoteRun.local_project_id` + `workspace_entry_for_project` 解析） | `metrics::metrics_settings`(130-167) channel-priority + normalize；`MetricsSettingsVm`(79-89) |
| `multica/state.rs` | 运行期内存状态（runtime_id 映射、在飞任务映射 + **任务级 `local_project_id`**）+ 持久化进 StateConfig | 不塞 `DesktopState`（见 2.5）；持久化参考 StateConfig(config/mod.rs:666-689) |
| `multica/loop_.rs` | 启动全量 register / 新添加 register / **常驻 3s 心跳（M5-cx 由 15s 提频）+ 工单完成后补发排空（M5-cx）** / recover-orphans / 取消检测 | `metrics::start_heartbeat_polling`(metrics.rs:218-242) spawn + 三层 guard 样板 |
| `multica/bridge.rs` | remote_task ↔ 本地 task/run 衔接（**直接调 `gold-band` 库层 App API** + 订阅 lifecycle bus，不走 Tauri command 层）；**执行注入（M5-z）**：发送时由 `start_multica_conversation_run` 接收 composer 下拉选中的 `local_project_id` 写入 `ActiveRemoteRun.local_project_id` → 按 `workspace_entry_for_project(&home_state, &local_project_id)` 解析 `workspace_path` → `App::with_config(workspace_path, config).with_lifecycle_bus(shared_bus)`(app/mod.rs) → 构造 Direct WorkflowDsl → `create_task_from_requirement` + `run_start_background` | `metrics::create_metrics_subscriber`(metrics.rs:365-638)；`lifecycle_bus.subscribe_named`(observability.rs:43-49)；`view_models_conversation.rs:2579`（Direct workflow preset 范本） |
| `multica/error.rs` | `MulticaError` 枚举 + 映射 `CommandErrorVm` | `CommandErrorVm`(commands.rs:581-634) + `command_error`(commands.rs:3529-3547) |
| `multica/transcript/` | **执行记录实时上报（M5-cy）**：`model`（wire DTO + 全部硬上限）、`projector`（`AcpUiEvent` → multica 消息的纯投影：全量快照差量、工具 use/result 配对）、`tool_input`（形状优先归一化 + Codex `apply_patch` 解析）、`reporter`（`TranscriptRegistry` managed state、500ms 批 flush、seq 分配、终态排空、用量累计）；观测点 = `acp_live_update_emitter` 在 IPC 压缩之前 | painter 模式参考 `maybe_record_agent_commands`(commands.rs:6179)；live 事件全量快照语义见 `apply_streaming_delta`(acp/client.rs:8286)；用量日志 `read_prompt_usage_segments`(acp/usage.rs:548) |
| `multica/handoff.rs` | **issue 完成输出传递（M5-bj）**：`extract_completion_output`（最终 assistant 回复中 info 恰为 `completion-output` 的围栏块、取最后一个、trim 空→None）+ `completion_output_from_reply`（组合 16k rune 截断，2026-09-21 服务端从 64k 收紧）+ `final_assistant_reply`（`AcpAttemptPaths` 定位 `acp.timeline.jsonl` → `load_timeline_items` → 取最后一条非占位 `textDelta`，复用 `is_semantically_empty_agent_content` 与 UI 投影同源）；全 fail-open。提示词侧篇幅契约（非 run 日志 / ≤2k / 超长走工作区文件 + 路径引用）见 `src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md` | `acp/events.rs AcpAttemptPaths::from_attempt_dir`(acp/events.rs:925) / `load_timeline_items` / `is_semantically_empty_agent_content`(3011) |

> ⚠️ `loop` 是 Rust 关键字，模块文件用 `loop_.rs`（`mod loop_;`，下划线后缀是 Rust 规避关键字的标准做法）。
> ⚠️ **无 `App::metrics_callback`**：metrics 不在 library crate 内，靠 `lifecycle_bus` 解耦（App 只暴露 `lifecycle_bus` 字段 + `with_lifecycle_subscriber`，app/mod.rs:680-694/997-1016）。multica 的**状态上报**完全照抄——正确做法是在 `register_lifecycle_subscribers`(commands.rs:481-497) 加一行 `subscribe_named("desktop.multica", ...)`，**不要给 App 加方法**。注：**会话执行**调的是 App **已有**公开方法（`create_task_from_requirement` / `run_start_background` / `worker_ref_show` / `run_continue_background_with_config_overrides`），不是新增 App 方法；库层唯一改动是把会话 VM 私有 Direct/Auto workflow 构造上提到 `gold_band::dsl::presets`（见 2.5）。

### 2.2 数据结构设计（先定数据）

> **关键**：码灵配置是三段式 —— `SettingsConfig`（用户可编辑，全 `Option<T>`，`user_settings.json`）→ `RuntimeConfig`（合并态，非 Option）→ `apply_settings` 灌入。channel 编译期默认值另走 `DesktopChannelConfig`（option_env!）。multica 字段必须完整走这三层 + channel，不得只加在一层。

#### 2.2.1 SettingsConfig 新增字段（`src/config/mod.rs:505-530`，全 `Option<T>`）

字段对照 metrics 三字段（`desktop_metrics_enabled/_base_url/_api_key`，config/mod.rs:525-527），全部 `Option<T>` + `desktop_multica_` 前缀：

```rust
// SettingsConfig 新增（config/mod.rs:505-530 旁，全 Option<T>）
pub desktop_multica_enabled: Option<bool>,
pub desktop_multica_base_url: Option<String>,         // API 根地址（rest 调用）
pub desktop_multica_app_url: Option<String>,          // Web 前端地址（浏览器登录页，可能与 base_url 不同）
pub desktop_multica_pat: Option<String>,              // 明文 PAT，永不回显
pub desktop_multica_daemon_id: Option<String>,        // 本机持久 UUID v4 simple
pub desktop_multica_workspaces: Option<Vec<MulticaWorkspaceRef>>,
pub desktop_multica_active_workspace_id: Option<String>,
pub desktop_multica_default_provider: Option<String>,  // 添加 workspace 时的默认 provider 预选（ACP 标识，默认 "claude-acp"）

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaWorkspaceRef {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub provider: String,            // ← 该 workspace 执行用的 ACP provider（如 "claude-acp"/"codex-acp"），添加时选定、绑定后不可变（变了=新 runtime=需重绑 agent）
    // M5-z：移除 `local_project_id` 字段——绑定模型下沉到任务级，本地目录在每次执行时由 composer 下拉选，写入 `ActiveRemoteRun.local_project_id` / `MulticaCompletedTask.local_project_id`
}
```

> PAT 明文存 SettingsConfig（项目无 keyring，与 metrics API Key 一致）。`desktop_multica_daemon_id` 首次启动为 None 时生成 `Uuid::new_v4().simple().to_string()`（参考 `TaskState::new` runtime/mod.rs:120-129 的 uuid 惯例）并落盘。

#### 2.2.2 RuntimeConfig 镜像字段（`src/config/mod.rs:993-1032`，非 Option）

非 Option 镜像（参考 metrics 字段 config/mod.rs:961-963），`apply_settings` 灌入（参考 config/mod.rs:1083-1121 的 `if let Some` 模式）：

```rust
// RuntimeConfig 新增（config/mod.rs:993-1032 旁，非 Option）
pub desktop_multica_enabled: bool,
pub desktop_multica_base_url: Option<String>,
pub desktop_multica_app_url: Option<String>,
pub desktop_multica_pat: Option<String>,
pub desktop_multica_daemon_id: Option<String>,
pub desktop_multica_workspaces: Vec<MulticaWorkspaceRef>,
pub desktop_multica_active_workspace_id: Option<String>,
pub desktop_multica_default_provider: String,  // Default "claude-acp"
```

`RuntimeConfig::default`（config/mod.rs:1034-1080）补默认值；`apply_settings` 新增块（config/mod.rs:1083-1121 模式）：
```rust
if let Some(v) = settings.desktop_multica_enabled { self.desktop_multica_enabled = v; }
self.desktop_multica_base_url = settings.desktop_multica_base_url.clone();
self.desktop_multica_app_url = settings.desktop_multica_app_url.clone();
self.desktop_multica_pat = settings.desktop_multica_pat.clone();
self.desktop_multica_daemon_id = settings.desktop_multica_daemon_id.clone();
self.desktop_multica_workspaces = settings.desktop_multica_workspaces.clone().unwrap_or_default();
self.desktop_multica_active_workspace_id = settings.desktop_multica_active_workspace_id.clone();
self.desktop_multica_default_provider = settings.desktop_multica_default_provider.clone().unwrap_or_else(|| "claude-acp".into());
```

#### 2.2.3 StateConfig 新增字段（`src/config/mod.rs:666-689`，程序写入/恢复用）

StateConfig 当前无 metrics 字段（metrics 无派生状态）。multica 需持久化 runtime_id 缓存与未完成 issue：
```rust
// StateConfig 新增（config/mod.rs:666-689 旁）
pub multica_runtime_ids: Option<HashMap<String, String>>,   // {workspace_id: runtime_id}
pub multica_pending_issues: Option<Vec<String>>,            // 失败待重试 issue_id（fail 写入 / complete·rerun 清除，见 §4.3）
pub multica_task_conversations: Option<HashMap<String, MulticaTaskConversation>>,  // {task_id: 会话引用}，断点续跑用
pub multica_completed_tasks: Option<Vec<MulticaCompletedTask>>,   // 终态任务本地历史（M5-o；改动六起不再进扁平「最近完成」桶，而按 workspace_id 并入对应工作空间 tasksByWorkspace 组），去重最新在前、截断 N=50
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaTaskConversation {
    pub local_task_id: String,      // 本地 task uuid（create_task_from_requirement 产出）
    pub local_run_id: String,       // 本地 run id（run_start_background 产出）；run_continue_background_with_config_overrides 续跑需要
    pub session_id: Option<String>, // ACP session_id：NodeCompleted 后 worker_ref_show 采集；claim 时作 prior_session_id 续跑；complete 后落盘
    pub work_dir: Option<String>,   // 工作目录（= 绑定 workspace_path，PinTaskSession 同步）
}
```

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaCompletedTask {
    pub remote_task_id: String,    // 关联键
    pub local_task_id: String,     // 终态行（改动六：并入所属 workspace 列表）整行点击 → 按 (local_project_id, local_task_id, local_run_id) 直达本地会话
    pub local_run_id: String,
    pub workspace_id: String,
    pub local_project_id: String,  // M5-z 新增：该次执行的本地工作目录（project_id）。终态行 onSelectRun 经此直达（不再依赖 workspace 级绑定解析）
    pub issue_id: Option<String>,  // rerun 用
    pub status: String,            // "completed" | "failed"
    pub title: String,             // 快照自 ActiveRemoteRun.title，缺失兜底 remote_task_id
    pub completed_at: String,      // RFC3339，finalize_terminal 时戳
}
```
> `multica_runtime_ids` 是缓存（register 幂等取回），丢失下次启动重建；`multica_pending_issues` 记录**失败待重试**的 issue_id（M4 终态 fail 时写入、complete/rerun 时清除；claim 不写——避免 running 任务被误显为 retryable），用于失败回显 + rerun；`multica_task_conversations` 是断点续跑核心索引（remote_task_id → {local_task_id, local_run_id, session_id}）——claim 时若命中（remote_task_id 存在且 session_id 非空），带 prior_session_id 续跑同一会话；complete 后清条目（详见 3.2.7 / 4.4）；`multica_completed_tasks` 是终态任务回看历史——`finalize_terminal` 移除 active 前快照一行（status 来自 PendingUpdate：ClearOnSuccess→completed / AddOnFailure→failed），去重最新在前、截断 N=50（M5-o）；**M5-z 新增 `local_project_id` 字段**（来自 `ActiveRemoteRun.local_project_id`，绑定模型下沉到任务级后任务自带本地目录）；**改动六起消费方式**：`get_multica_tasks` 按 `workspace_id` 把这些终态行并入对应工作空间的 `tasksByWorkspace` 组（`from_completed` + `merge_workspace_tasks`，不再有扁平全局「最近完成」桶 / `recentlyCompleted` 字段），`local_project_id` 直接进 `RemoteTaskVm.projectId`。

#### 2.2.4 channel config（编译期默认，5 处改动）

channel 字段是**编译期常量**（option_env!，参考 metrics 字段 channel.rs:4-22），新增 4 字段（multica 不需要 channel 级 api_key——PAT 登录后生成；浏览器登录需要 Web 前端地址，故单独设 `multicaAppUrl`，可能与 `multicaBaseUrl` 不同）：

| 字段 | DesktopChannelConfig（channel.rs:4-22） | default.json | wb.json |
|---|---|---|---|
| `multica_base_url` | `&'static str` | `http://localhost:8080` | `${GOLD_BAND_WB_MULTICA_URL}` 注入的独立 origin（不再从 WB 服务地址派生端口，见 §12.92） |
| `multica_app_url` | `&'static str` | `http://localhost:3000` | 同 `${GOLD_BAND_WB_MULTICA_URL}`（前后端同源同入口） |
| `multica_enabled` | `bool` | `true` | `true` |
| `multica_toggle_locked` | `bool` | `false` | `true` |

> **零配置直连（2026-08-05 决策）**：default 频道预填本地联调地址 + `multica_enabled=true`，使「远程任务列表」未连接空状态的【连接 Multica】按钮**无需用户先去设置页调整任何配置**即可直接触发浏览器登录（channel 回退提供 base_url/app_url，见 §2.2.5 `multica_base_url` 容错链 `config.desktop_multica_base_url → channel.multica_base_url`）。wb 频道预填企业地址。用户仍可在设置页改 provider（claude/codex）或覆盖地址。配套：`MulticaRemoteTaskList` 未连接态按钮**直接调用 `connect_multica()`**（loading/成功后 `fetchTasks`/失败回显），不再跳设置页、不再有 `onOpenSettings` prop。

**新增一个 channel 字段需同步改 5 处**（参考 metrics 字段既有改动）：
1. `DesktopChannelConfig` struct 加字段（channel.rs:4-22）
2. `current_channel_config()` 加 `option_env!("GOLD_BAND_MULTICA_XXX").unwrap_or(...)`（channel.rs:24-52）
3. `build.rs` 的 `ChannelConfig` struct（camelCase）+ `println!("cargo:rustc-env=GOLD_BAND_MULTICA_XXX={}", config.multica_xxx)` + `cargo:rerun-if-env-changed=GOLD_BAND_MULTICA_XXX`（build.rs:5-27）
4. `configs/channels/default.json` 加 `"multicaXxx": ...`
5. `configs/channels/wb.json` 加同字段（预填值）

#### 2.2.5 MulticaSettingsVm（前端 VM，永不回显明文 PAT）

参考 `MetricsSettingsVm`（metrics.rs:79-88，`#[serde(rename_all = "camelCase")]`）：用 `pat_set: bool` 表示 PAT 存在性，**永不回显明文**：

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaSettingsVm {
    pub enabled: bool,
    pub toggle_locked: bool,
    pub multica_base_url: Option<String>,
    pub multica_app_url: Option<String>,    // Web 前端地址（浏览器登录页）
    pub pat_set: bool,                      // ← 只暴露存在性，永不回显明文 PAT
    pub daemon_id_set: bool,
    pub workspaces: Vec<MulticaWorkspaceRef>,      // 每条**只含 provider，不含 local_project_id**（M5-z：绑定下沉到任务级，本地目录执行时选）
    pub active_workspace_id: Option<String>,
    pub default_provider: String,                  // 添加 workspace 时的默认 provider 预选（claude-acp）
    pub connected: bool,                    // PAT 有效（GET /api/me 通过）
}
```

`pub fn multica_settings(config: &RuntimeConfig) -> MulticaSettingsVm` 完全照搬 `metrics::metrics_settings`(metrics.rs:130-167) 的 channel-priority + normalize 模式：`enabled = config.desktop_multica_enabled || channel.multica_enabled`，base_url 用与 `normalize_metrics_base_url`(metrics.rs:90-114) 同款容错函数 normalize。

> 前端 `types.ts` 镜像该 VM（参考 `MetricsSettingsVm` types.ts:58），可选嵌入 `AppBootstrapVm`(types.ts:93-108) 让首屏一次拉到。

#### 2.2.6 SettingsConfig schema 迁移

`SettingsConfig::from_json_value_with_migration`(config/mod.rs:548-580) 按 `settingsSchemaVersion` 跑迁移。本设计字段全 Option + serde(default)，旧配置反序列化时 multica 字段为 None（等价未启用）→ **向后兼容，无需升级 schema 版本**。若后续改了既有字段形状，才把 `CURRENT_SETTINGS_SCHEMA_VERSION` 升级 + 加迁移分支。

#### 2.2.7 multica API 请求/响应 struct（`multica/client.rs`）

按接入方案 4.2 接口契约定义强类型 struct（节选；完整字段以 multica `server/internal/daemon/types.go` 的 json tag 为权威源）：
```rust
// 浏览器登录（localhost callback，见 2.3 / 4.1）
pub struct CliCallbackToken { pub token: String }          // 解析 ?token=<JWT> 回跳参数
pub struct LoginResponse { pub token: String, pub user: UserResponse }
pub struct CreateTokenRequest { pub name: String, pub expires_in_days: u32 }
pub struct TokenResponse { pub token: String, pub expires_at: String }
pub struct RegisterRequest {
    pub workspace_id: String, pub daemon_id: String, pub device_name: String,
    pub cli_version: String, pub runtimes: Vec<RuntimeSpec>,
}
pub struct RuntimeSpec { pub name: String, pub r#type: String /*=provider 固定*/, pub version: String, pub status: String }
pub struct RegisterResponse { pub runtimes: Vec<RuntimeRow> }
pub struct RuntimeRow { pub id: String /*=runtime_id*/ }
pub struct RemoteTask { pub id: String, pub issue_id: Option<String>, pub status: String, pub auth_token: Option<String> /*=server 回传 mat_；Option B 中介下不消费，§12.29 起定点 #[allow(dead_code)]*/, pub prior_session_id: Option<String> /*=响应回填，客户端只消费不发送；§12.29 起定点 #[allow(dead_code)]（主路径 parent_task_id）*/, pub parent_task_id: Option<String> /*=auto-retry 子任务 T' 指向父 T；客户端续跑反查主路径（§12.14）*/, pub title: Option<String> /*=wire thread_name*/, pub requirement: Option<String>, pub last_activity_at: Option<String>, pub issue_kind: Option<String> /*=wire issue_kind：issue 类型 dev/test/bug/general；**不复用 kind**（那是任务来源判别符）。#[serde(default)] 版本解耦（旧 server 缺失→None）（§12.40）*/, pub is_ready: Option<bool> /*=wire is_ready：仅 test 型有意义（父 dev issue 是否 done）。#[serde(default)]（§12.40）*/ }
pub struct ClaimRequest {}   // 服务端 claim 处理器不解码 body；prior_session_id 是「响应只输出」字段（agent.go:311），客户端消费响应而非塞请求体（§12.14）
pub struct PinTaskSessionRequest { pub session_id: String, pub work_dir: Option<String> }
pub struct StartRequest { pub force_fresh_session: bool }          // rerun 时 true（详见 4.4 / 接入方案 3.2.7）
pub struct CompleteRequest { pub output: String, pub session_id: Option<String>, pub work_dir: Option<String>, pub completion_output: Option<String> }  // M5-cd：issue 交付说明随 complete 同事务回传（multica 协议改造，§12.72）
pub struct FailRequest { pub error: String, pub failure_reason: String }
```

#### 2.2.8 错误码（`multica/error.rs`，映射 CommandErrorVm）

multica 错误统一走项目 `CommandErrorVm { code, params }`(commands.rs:581-634)，code 用 `multica.<kebab-case-reason>` 前缀（参考现有 `acp.*`/`workspace.*`，commands.rs:581-634）：

```rust
#[derive(Debug, thiserror::Error)]
pub enum MulticaError {
    #[error("not configured")] NotConfigured,
    #[error("auth failed: {0}")] AuthFailed(String),
    #[error("network failed: {0}")] NetworkFailed(String),
    #[error("register failed: {0}")] RegisterFailed(String),
    #[error("claim conflict")] ClaimConflict,
    #[error("task not found")] TaskNotFound,
    #[error("runtime offline")] RuntimeOffline,
    // M4-d：保留在错误码表（multica.session-resume-failed），但断点续跑路径不 emit——任何 resume Err
    // 改走 silent fresh-fallback（更稳，无需 fragile 串匹配）。变体标 #[allow(dead_code)]（§12.29）。
    #[allow(dead_code)]
    #[error("session resume failed, will rerun")] SessionResumeFailed,
    // §12.40（M5-ba）：test 型任务未就绪（父 dev issue 未 done）即被领取。claim 时码灵主动拦截并回滚
    // 自己刚发出的 claim（release_after_run_start_failure），错误上抛给用户。
    #[error("multica task not ready")] TaskNotReady,
}
// §12.29（M5-aq）dead_code 清理：WorkspaceEmpty / PinSessionFailed 变体删除（全链路零构造——
// 前端空态 UI 守卫 / pin 失败仅记日志），i18n workspace-empty + pin-session-failed 同步删除。
```
命令层通过 `command_error`(commands.rs:3529-3547) 把 `MulticaError` 映射为 `CommandErrorVm`（code: `multica.not-configured` / `multica.auth-failed` / …，params 携带 task_id/workspace_id 等上下文，**不含对客文案**）。client.rs 的 HTTP 错误按状态码映射：401→AuthFailed（PAT 失效触发重新登录），403→AuthFailed，404→TaskNotFound，409→ClaimConflict，网络超时→NetworkFailed。完整码表见第 5 章。

### 2.3 multica/client.rs

职责：reqwest client 封装 + **指数退避重试**（项目目前缺失，metrics/feedback 均 fire-and-forget）+ 全部 multica HTTP 调用。

**复用**：reqwest 用法照搬 `metrics::send_heartbeat`(metrics.rs:180-208) —— `reqwest::Client::new()`、`.timeout(Duration::from_secs(10))`、header 注入；URL 经 `normalize_multica_base_url`（同 `normalize_metrics_base_url` metrics.rs:90-114 容错）；日志用 `metrics::metrics_log`(metrics.rs:43-61) 或抽共享 helper。

方法清单（一一对应接入方案 4.2）：
- `browser_login(app_url) -> LoginResponse`（localhost callback 浏览器登录，见 4.1：起临时 HTTP server + 打开浏览器 + 收 `?token=<JWT>`）。**callback 响应**：抽纯函数 `callback_redirect_response(app_root) -> String`——回 `HTTP/1.1 302 Found` + `Location: <app_root>/`（trailing-slash 归一），**不渲染任何"登录成功"页**（multica Web 回跳 cli_callback 前已 `setLoggedInCookie`，故浏览器被 302 导回 multica web 根时带登录态 cookie、落在 multica web 界面，而非码灵自渲染的突兀结果页）；单测 `callback_redirect_response_is_302_to_app_root_without_query` 固化（断言 302、Location 归一、不含 `token=`）。
- `create_token(jwt, name, days) -> TokenResponse`
- `verify_pat() -> ()`（GET /api/me）
- `list_workspaces() -> Vec<Workspace>`
- `register(req) -> RegisterResponse`（幂等）
- `recover_orphans(runtime_id) -> ()`
- `list_pending_tasks(runtime_id) -> Vec<RemoteTask>`（只读）
- `claim_specific_task(runtime_id, task_id, prior_session_id?) -> RemoteTask`（断点续跑：命中本地 `task_conversations` 时带 prior_session_id）。`RemoteTask` claim 响应携带需求来源字段（quick_create_prompt / chat_message / trigger_comment_content / autopilot_description / handoff_note / **issue_description**——改动四新增，webank `buildClaimedTaskResponse` issue 分支回填 `issue.Description.String`，pgtype.Text NULL→""）；`requirement_text(&self) -> Option<String>` 按来源互斥优先级取首个非空（issue 型取 issue_description 正文，无正文才回退 title），供 claim VM 回填 `requirement` 预填 composer
- `start_task(task_id, force_fresh_session) / heartbeat / get_task_status`（本期状态只基础：start/complete/fail + 心跳，不接入 step/total 进度上报）。`heartbeat` 的 ack 含 `pending_readiness_changes: Option<Vec<ReadinessChange{task_id, is_ready}>>`（§12.40；仅作**刷新信号**，不本地改状态——事实源仍是 pending 列表）
- `RemoteTask::executable_ready(&self) -> bool`（**统一准入谓词**，§12.40）：`issue_kind != Some("test") || is_ready.unwrap_or(false)`——非 test 恒可执行、test 需显式 `is_ready==true`、旧 server（两字段皆 None）行为与拆分前逐字节一致。看板置灰 / claim 拦截 / 前端 `isTaskExecutable` 三处同源
- `pin_task_session(task_id, session_id, work_dir) -> ()`（写 task 行的 session_id/work_dir，断点续跑依据，对应接入方案 C8）
- `complete_task / fail_task`（**重试幂等**：4/8/16/32/64s 共 6 次，确保终态送达）
- `update_issue_status(workspace_id, issue_id, status) -> ()`（改动二：`PUT /api/issues/{id}` body `{status}` + `X-Workspace-ID` 头，`with_network_retry` 3 次；常量 `MULTICA_ISSUE_DONE_STATUS="done"` 用于完成流转，**`MULTICA_ISSUE_IN_PROGRESS_STATUS="in_progress"`（改动五新增）用于开始执行时流转**）

**重试策略**：
- **终态回调**（complete/fail）：严格 4/8/16/32/64s 共 6 次（初始尝试 + 5 次退避重试），服务端幂等。仅对 `NetworkFailed`（reqwest 传输/超时 + map_status 归类的 5xx + 解码错误）重试；AuthFailed/TaskNotFound/ClaimConflict 等确定错误码立即返回（重试无意义，与一般请求「4xx 不重试」同源）。由 `with_terminal_retry`（与 `with_network_retry` 同形，仅退避更长更密、次数 6）统一承载。
- **一般请求**（register/list/claim/pin_session/rerun）：网络错误重试 3 次（`NETWORK_RETRY_BASE_SECS=1`，第 n 次 `1*2^n`s），4xx 不重试直接映射错误码。
- PAT 走 `Authorization: Bearer <pat>`；issue 维度业务接口（D1/E1/E2，path 不含 workspace）加 `X-Workspace-ID` 头路由（接入方案 4.1，由 `post_json_with_workspace` 承载）；daemon 任务接口（C2-C8）path 自带 task_id 不需要该头。

> ⚠️ **终态重试是超出 metrics fire-and-forget 模式的增强点**：metrics 失败只记日志（metrics.rs:206 "FAILED (ignored)"），multica 终态必须送达（否则任务悬空），故 client.rs 自建退避重试，是本模块相对 metrics 的主要新增逻辑。

**性能优化（连接池复用 + liveness 短超时 + 自愈单次重试，详见 §12.13）**：
- **进程级共享 `reqwest::Client`**：`shared_http()` 用 `std::sync::OnceLock`（与 `metrics.rs`/`view_models.rs` 同惯用，零新增依赖）持有唯一 client，`MulticaClient::new` 取其廉价 `clone`（reqwest `Client` 内部 `Arc`，构造昂贵/clone 廉价）。修复「每调用点重建 client → 连接池/TLS 上下文作废 → 每次重做 TCP+TLS 握手（弱网放大失败率）」。`MulticaClient` 因此 `derive(Clone)`，为并发扇出铺路。client 级 30s 超时仍在 `shared_http` 内设定。
- **liveness per-request 短超时**（常量 `LIVENESS_TIMEOUT_SECS=10`，经 `send`/`json_send` 的 `per_request_timeout` 参数覆盖 client 级 30s）：心跳 tick 内高频调用（`heartbeat` / `extend_prepare_lease` / `get_task_status` / `register_once`）正常 <1s，server 慢响应时 10s 快速失败、下一 tick 重试。**非 liveness 调用**（verify_pat / list_workspaces / list_pending_tasks / claim / start / complete / fail / pin / rerun / update_issue_status）仍走 30s，可靠性优先于快速失败。
- **自愈 register 单次重试**（`register_once`，见 §12.1/§12.13）：常驻心跳 tick 驱动的自愈「循环即重试」，不嵌套 client 内 `with_network_retry`（3×30s）——否则弱网下单 tick 可超 90s 阻塞续期/取消检测。`register`（带 3 次重试）仅留一次性路径（启动 / connect / 绑定即时）。

### 2.4 multica/config.rs

职责：读写 SettingsConfig/StateConfig 的 multica 字段；`pat_set`/`daemon_id_set` getter（永不回显明文）；`daemon_id` 首次生成并落盘；channel base_url/app_url 默认值合并；`multica_settings()` 聚合 VM。

**复用**：
- `pat_set` 套用 `metrics::get_api_key`(metrics.rs:244-258) 的「用户优先、channel 兜底」+ `api_key_set`(metrics.rs:87) 明文不回显模式。
- `multica_settings()` 套用 `metrics::metrics_settings`(metrics.rs:130-167) 的 channel-priority + normalize + `enabled = config || channel` 模式。
- 配置原子写：`App::save_settings`(app/mod.rs:1051-1053) → `write_json`。
- daemon_id 生成：`Uuid::new_v4().simple().to_string()`（参考 runtime/mod.rs:120-129），首次为空时生成并 `save_settings` 落盘。

### 2.5 multica/state.rs

职责：运行期状态容器。

**关键决策：不塞 `DesktopState`**。agent 报告确认 `DesktopState`(state.rs:185-200) 是 12 字段 Mutex 池，metrics 在其中**无专属字段**（靠每 tick 重读 settings）。multica 同理：
- **运行期内存状态**（runtime_id 映射、在飞 `remote_task_id ↔ local_task_uuid` 映射）：由 `loop_.rs`/`bridge.rs` 持有的 `Arc<Mutex<MulticaRuntimeState>>`，不进 `DesktopState`。
- **需持久化的状态**（`multica_runtime_ids` 缓存、`multica_pending_issues`）：进 StateConfig（2.2.3），通过 `App::save_state` 落盘。
- 若未来确需桌面端独占的可观测状态（如 mcp_health 那样供前端读取），才参考 `mcp_health: Mutex<BTreeMap<...>>` + `mcp_health_snapshot()`(state.rs) 模式加进 `DesktopState`。本期不需要。

```rust
#[derive(Default)]
pub struct MulticaRuntimeState {
    pub runtime_ids: HashMap<String, String>,              // workspace_id → runtime_id（内存缓存）
    pub active_runs: HashMap<String, ActiveRemoteRun>,     // remote_task_id → 本地 task/run 信息
    pub prepare_leases: HashMap<String /*remote_task_id*/, PrepareLease { runtime_id: String }>,  // Req D：claim-at-click 后、start 前持有（claim 写入 / start 或 cancel 移除 / 常驻循环续期）
}
pub struct ActiveRemoteRun { pub local_task_uuid: String, pub local_project_id: String /* M5-z 新增：任务级本地工作目录（发送时由 composer 下拉选中写入） */, pub issue_id: Option<String>, pub title: Option<String>, pub started_at: String }
// title（M5-o）：claim 时快照自 task 标题；finalize_terminal 写「最近完成」历史时无需回读 task.json 即可拿行标签
// local_project_id（M5-z）：绑定下沉到任务级——`start_multica_conversation_run` 写入；`finalize_terminal` 快照进 `MulticaCompletedTask.local_project_id`；执行/作废路径用 `workspace_entry_for_project(&home_state, &local_project_id)` 解析 `workspace_path`，替代原 `binding_for_multica()`（已删除）
// Req D：`all_runtime_ids()` 读 runtime_ids.keys（= 全部已连接 workspace）供常驻心跳；`active_runtime_ids()`（仅 active_runs）保留给 cancel 检测。
```

**会话执行注入**（**直接调 `gold-band` 库层 App API，不重复造 runtime、不碰 lifecycle 契约**）：

> **关键澄清**：码灵库层**不存在「会话 / 工作流」二分**——前端会话模式（新 UI）的 VM 内部恰恰就是调 `create_task_from_requirement` + `run_start_background`（`view_models_conversation.rs` 会话 VM 即此实现）。multica 复用同一套库层 API：**一个 remote_task = 一个本地 task**，首轮 prompt = requirement（由 `create_task_from_requirement` 写入 `requirement.md`，Worker 节点自动读取 `node_executor.rs:610`），**不走 `submit_conversation_prompt`**。bridge 直接调库层，不经 Tauri command 层、不需前端 AttemptLocator。
>
> **会话完成判定（单轮即完成，无多轮歧义）**：**一个 remote_task = 一次执行（一个 run），不是可多轮的会话**。码灵作为 daemon 只跑 requirement 这一轮——首轮 requirement 驱动的 run 具 runtime-continue 性质，跑完自然 `RunCompleted`（码灵**不在单 remote task 上承载追问**，对话发起方是 multica）。不存在「多轮会话何时算完」的问题。**「多轮对话」由 task 序列承载**：用户在 multica web 对同一 issue 发新需求 → 新 queued task → 码灵 claim 子任务后按响应 `parent_task_id` 反查父任务本地索引，续跑**同一 ACP session**（上下文连续，见 4.4 / §12.14）=「多轮」，而非一个 task 内多轮。
>
> **run 终态 4 分支（订阅器必须穷举，不能只 match success）**（源码依据 `provider/mod.rs:1086-1100` stop_reason 分类 → `node_executor.rs:998-1067` 节点 outcome → `control/mod.rs:25-43` decide_next_step → `orchestrator.rs:2388-2437` RunCompleted 发射）：
>
> | 本地终态 | 触发（provider stop_reason） | multica 上报 |
> |---|---|---|
> | `RunCompleted{Success}` | `end-turn`（正常完成） | `complete(output, session_id, work_dir)` |
> | `RunCompleted{Failure}` | `refusal`/`error`/未知失败 | `fail(error, failure_reason=agent_error.*)` |
> | `RunCompleted{Killed}` | run 被 kill（agent 进程真死） | **M4-d 细化**：unconditional `fail(timeout)`。cancel 路径皆经 `run_pause→Paused` 从不产生 Killed，故 Killed 必为 agent 真死（无「cancel-detection 上下文」消歧）；`timeout` resume-safe，server auto-retry 兜底 |
> | `RunPaused` / `InterventionRequested` | `waiting-for-user-input`/`permission-requested`/`interrupted` | **绝对不上报终态**：multica 端继续显示 running（见下「Paused 盲区」），码灵本地处理 elicitation/permission → 响应后 run 走向 `RunCompleted` |
>
> **⚠️ Paused 盲区（已决策：接受，multica 不感知）**：multica task 状态机只有 queued/dispatched/running/completed/failed，**无 paused 态**。码灵跑 multica task 时，agent 若请求 elicitation（追问）或 permission-requested（工具授权）→ 本地 run 进 Paused，**需本地用户响应**。本期决策：**multica 端不感知此中间态、继续显示 running，码灵本地全权处理**（弹窗/通知响应），不主动上报、不超时兜底；响应后 run 自然走向 `RunCompleted` 再上报终态。代价：发起人在 multica web 此期间看不到「在等输入」，仅看到 running——本期可接受。
>
> **AcpTurnFinished 事件当前未实现**（grep 全库零 emit 点，仅定义于 `app/mod.rs:665` + 注释「Direct 后续对话走该事件」）：方案 A 不依赖它（多轮 = task 序列），订阅器**不监听** `AcpTurnFinished`，避免去找一个当前不存在的事件。

- **绑定存储（M5-z：任务级，非 workspace 级）**：本地工作目录**不再**存于 `SettingsConfig.desktop_multica_workspaces[i]`（该结构已移除 `local_project_id` 字段，2.2.1）。改为每次执行时由 composer 下拉选 → `start_multica_conversation_run` 写入 `ActiveRemoteRun.local_project_id`（2.5）→ `finalize_terminal` 快照进 `MulticaCompletedTask.local_project_id`（2.2.3）。本地目录的 `workspace_path` 经 `workspace_entry_for_project(&home_state, &local_project_id)`（`conversation_workspace.rs:24`）按需解析，**不在 multica 侧重复存 path**，避免双份不一致。
- ~~**查找函数 `binding_for_multica`**~~（**M5-z：已删除**）：原 `multica workspace → local_project_id → conversation_workspaces.workspace_path` 查找函数已删除。`invalidate_remote_task` / 会话执行注入改为直接用任务级 `ActiveRemoteRun.local_project_id` → `workspace_entry_for_project(&home_state, &run.local_project_id)` 解析 `workspace_path`。provider 仍从 `multica_workspaces[i].provider` 取（不变）。
- **会话执行注入**（**Req D + M5-z 重构**：原 `start_multica_remote_task` 原子 claim+start 已删除拆分；现在 claim 只登记 lease + 返回 requirement；本地工作目录由 composer 下拉选 + `start_multica_conversation_run` 在用户发送时写入 `ActiveRemoteRun.local_project_id`。真正的执行注入**复用本地 `create_conversation_run_vm(&App, &ConversationCreateInputVm) -> anyhow::Result<ConversationRunVm>`（`view_models_conversation.rs:2590`，pub fn）**：该函数内部已建工作流（Direct/Auto）→ `create_task_from_requirement`（requirement 作首轮 prompt）→ 写 `conversation.json` → 拷附件 → `run_start_background` → 返回带 `run_id` 的 VM。multica 发送路径直接调它，再叠加 multica 专属簿记（`register_active_run` 写入 `local_project_id` + 落 `multica_task_conversations` + `client.start_task`）。下方伪码描述的是 `create_conversation_run_vm` 内部等价机制，落点改为复用该函数而非在 multica 侧重写一遍）：
  ```rust
  // M5-z：workspace_path 来自任务级 local_project_id（composer 下拉选 + start_multica_conversation_run 传入），
  //       不再走 binding_for_multica。provider 仍来自 multica_workspaces[i].provider。
  let workspace_path = workspace_entry_for_project(&home_state, &local_project_id)?.workspace_path;
  let provider = config.desktop_multica_workspaces.iter().find(|w| w.id == workspace_id)?.provider;
  // ① 构造绑定该 workspace 的 App——**必须经 state.app()**（注入 DesktopState 持有的共享 lifecycle_bus，
  //    desktop.multica 订阅器据此收 NodeCompleted/RunCompleted）再 with_repo_root 改绑目录（保留共享 bus）。
  //    App::with_config 自带【空 bus】，直接用它订阅器收不到事件。
  let app = state.app()?.with_repo_root(Utf8PathBuf::from(workspace_path.clone()), context.config.clone());
  // ② 构造 Direct WorkflowDsl（单 Worker 节点，provider 编码进 NodeDsl::Worker.provider；复用库层 preset）
  let workflow = gold_band::dsl::presets::direct_workflow(provider, None, None, BTreeMap::new());
  // ③ 建 task（requirement 作首轮 prompt）+ 后台启动 run（库层 sync 调用经 spawn_blocking，不阻塞 Tauri runtime）
  let summary = app.create_task_from_requirement(CreateTaskInput {
      title: claimed.title.clone(), description: None, requirement_file_name: None,
      requirement_content: requirement, workflow, workflow_template_id: None,
  })?;                                  // 一个 remote_task = 一个本地 task（CreateTaskInput 无 Default，全字段显式）
  let run = app.run_start_background(&summary.task.id, None)?;          // run.id 来自此返回值（TaskSummary 无 run_id）
  // ④ 登记 active_runs（真实 run.id + 任务级 local_project_id，先于 NodeCompleted/RunCompleted 归属反查）+ 落断点续跑索引
  shared.register_active_run(&remote, ActiveRemoteRun { local_task_uuid: summary.task.id, local_project_id, /* run_id=run.id, ... */ });
  state.multica_task_conversations.insert(remote, MulticaTaskConversation {
      local_task_id: summary.task.id, local_run_id: run.id, session_id: None, work_dir: Some(workspace_path),
  });                                   // session_id=None 待 NodeCompleted 由 bridge 回填
  client.start_task(&task_id, false).await?;                            // dispatched→running（claim 后 5min 内须 start）
  ```
  > `CreateTaskInput` 字段定义见 `app/mod.rs:440-447`，**不 derive Default**（全字段显式构造）；`run_id` 来自 `run_start_background` 返回的 `RunState.id`，**非** `TaskSummary`（其仅含 `latest_run`，无 `run_id`）。`auth_token`：claim 响应带的执行凭证，本期码灵本地执行**不注入**（agent 用绑定 provider，multica 回传全走 daemon PAT）——保留字段，未来 agent→multica 直连回调再启用。本地启动失败 best-effort `fail_task(.., "agent_error")` 免 server 干等 5min 超时。
- **库层改动（M4-c 已落，Direct preset 上提）**：会话 VM 现有**私有** fn `build_direct_workflow`(`view_models_conversation.rs:2579`) 上提到 `gold_band::dsl::presets::direct_workflow(provider, model, permission_mode, config_options)` 公开复用——会话 VM 与 multica bridge 共用同一份 provider→WorkflowDsl 构造，杜绝重复造轮子（VM 改为委托 preset，2 单测固化）。**仅上提 direct**：`auto_workflow`(`:2461`) 核心是 VM 特有的 `AiDynamicAgentStrategy`（Fixed/Dynamic + available_agents/routing_prompt）配置翻译，仅会话 VM 单一消费方、无第二消费方——上提只是搬迁不构成复用（违反自身「杜绝重复造轮子」的成立前提），故保留 VM；未来出现第二消费方再上提。
- **provider 注入**：provider 编码进 `NodeDsl::Worker { provider: Some(...) }`（经 preset）；**库里没有 `App::run_with_provider`，必须经 WorkflowDsl**。`create_task_from_requirement` 内部的 `validate_workflow_agents`(`app/mod.rs:1863`) 要求该 provider 已在 `app.config.agents`(SettingsConfig) 注册——故 multica workspace 绑定的 provider 必须先在用户设置配好（与 2.2.1 workspace 条目带 provider 一致）。
- **断点续跑**：claim 时若 `multica_task_conversations[task_id].session_id` 非空，`claim_specific_task(.., Some(prior_session_id))`；续跑调 `app.run_continue_background_with_config_overrides(&local_task_id, &local_run_id, None, None, Vec::new(), None, None)`（内部自动读 `worker-ref.json` 的 `continue_ref` 走 ACP `session/load`，**无需手动传 continue_ref**）；session 已死（strict_continue 报错 `acp/client.rs:2132-2141`）→ `SessionResumeFailed` → 降级 `force_fresh_session=true` 整任务重跑（新本地 task）。详见 4.4 / 接入方案 3.2.7。
- **不改的点**（重要）：`TaskState`/`RunState`/`NodeState` **不加 work_dir 字段**——码灵设计哲学是「work_dir 隐式由 App 实例的 `repo_root` 承载」。只要 `App::with_config(workspace_path, …)` 的 workspace_path = 绑定目录，下游 cwd 全部自动正确（`node_executor.rs:612`、`acp/adapter.rs:50` `.current_dir()`、`acp/client.rs:2147-2150` `session/new` 的 cwd）。
- **session_id 采集**：ACP session_id 落在 `<attempt_dir>/worker-ref.json` 的 `continue_ref.acpSessionId`(`acp/client.rs:3759-3760`)。`RuntimeLifecycleEvent` **本身不带 session_id**——bridge 订阅 lifecycle，收到 `NodeCompleted { task_id, run_id, round_id, node_id, attempt_id, .. }` 后用该 attempt locator 调 `app.worker_ref_show(task_id, run_id, round_id, node_id, attempt_id)` 读 `WorkerRefState.continue_ref`，取 `acpSessionId`，回填 `multica_task_conversations[task_id].session_id` 并 `pin_task_session(task_id, session_id, work_dir)`。（`acp_session_update_emitter` 是 Tauri-webview 转发器，bridge 用不到，直接读 worker-ref。）
- **complete 上报**：success → `complete_task(output=会话产物摘要, session_id=ACP session_id, work_dir=该任务 local_project_id 解析的 workspace_path)`；终态 `work_dir` = `workspace_entry_for_project(&home_state, &run.local_project_id).workspace_path`（M5-z：任务级，不再走 workspace 绑定）。
- **本地目录缺失（M5-z 后边界）**：发送时若 composer 下拉无任何本地工作区 → composer UI 显示「请先添加本地工作空间」引导（i18n `conversation.composer.multicaNeedLocalWorkspace`）并禁用发送，不会进入 `start_multica_conversation_run`。原 claim 时 `binding_for_multica` 返回 None 的引导已删除（绑定不再发生在 workspace 级）。

### 2.6 multica/loop_.rs

职责（对应接入方案 3.2.4）：
- **启动全量 register**：遍历 `desktop_multica_workspaces` 逐个 register（幂等取回 runtime_id 缓存）。
- **新添加 register**：用户添加 workspace 时 register 该 workspace。
- **常驻 3s 心跳**（Req D 演示反馈调整由「执行期」改为「常驻」；M5-cx 由 15s 提频，multica 侧心跳优化对齐）：与 multica 建立连接后即对**全部已连接 workspace 的 runtime** 持续心跳，不再仅 claim→complete 期间。迭代源由 `collect_active_runtime_ids`（仅 `active_runs`）改为 `state.all_runtime_ids()`（读 `runtime_ids`，= 所有已连接工作空间）；`active_runtime_ids` 保留给 cancel 检测用。每 3s `POST /heartbeat`（≠ metrics 的 15min，两套并存）；runtime-assets 工单处理完成后经 `heartbeat_once` 补发排空（§12.94）。
- **prepare-lease 续期**（Req D 新增，claim-at-click 后、start 前）：同循环遍历 `state.prepare_leases`（claim 写入 / start 移除），对每条调 `client.extend_prepare_lease(runtime_id, task_id)`（`POST /api/daemon/runtimes/{rid}/tasks/{tid}/prepare-lease`），避免用户在 composer 选模型/改需求（可能 >45s）期间被 `ReclaimStaleDispatchedTaskForRuntime` 回收、发送时 `start_task` 失败。与 multica daemon 自带的 `startTaskPrepareLeaseExtender`（每 15s）同构。
- **recover-orphans**：启动 register 后，对每个 runtime_id 调 `POST /runtimes/{rid}/recover-orphans`（残留在飞任务无条件清为 `runtime_recovery` 失败态；multica server 对 `runtime_recovery` 等 retryable reason 自动重试 `max_attempts=2`——**本期接受该行为**，见接入方案 3.2.3）。
- **取消检测**：长任务执行期周期 `GET /tasks/{id}/status`，cancelled/failed/404 → 中断本地 run。

**复用**：`start_multica_loop<R: Runtime>(app: AppHandle<R>)` 完全照搬 `metrics::start_heartbeat_polling`(metrics.rs:218-242) 骨架：
- `tauri::async_runtime::spawn(async move { loop { ... } })`（用 Tauri 全局 runtime，不自建 tokio）
- 每 tick 用 `app.try_state::<DesktopState>()` → `state.context()` → `multica_settings(&ctx.config)` **三层 guard**（任一失败 skip 该 tick）
- **每 tick 重读配置**——用户改配置即时生效，无需重启
- `tokio::time::sleep(Duration::from_secs(...)).await`

**启动入口** `start_multica_runtime`：先 `verify_pat`(GET /api/me) → 有效则全量 register；**无效则静默、不弹任何 UI**（等用户切到「远程任务列表」未连接空状态点【连接 Multica】才触发登录，见 4.1；**首启绝不自动弹登录**）。挂载点：`main.rs:185` `start_heartbeat_polling` **之后**加 `multica::loop_::start_multica_loop(app.handle().clone())`（setup hook 内，main.rs:137-187）。

> 心跳间隔常量集中在本模块顶部（`const MULTICA_HEARTBEAT_INTERVAL_SECS: u64 = 15;`），杜绝硬编码。

**tick 耗时埋点 + 单 tick 上界（§12.13）**：每 tick 记 `tick_start`，四阶段（self_heal / heartbeat / extend_lease / cancel_detect）各记 `trace_stage`（trace 级，低噪声，含 `stage` + `elapsed_ms`）；tick 末总耗时 `elapsed_ms`，正常 debug 级、**超 30s 升 warn**（"heartbeat tick overrun"）——量化弱网下单 tick 是否威胁 prepare lease（45s）续期窗口。配合 client 侧 liveness 短超时（§2.3）+ 自愈 `register_once`（§12.1），退化网络下单 tick 不再阻塞数分钟，而是快速失败、下一 tick 重试。

**自愈 register 走单次重试**（§12.1/§12.13）：`self_heal_registration` 调 `register_workspace(retried=false)` → `client.register_once`（单次 + liveness 短超时），不再嵌套 client 内 3×30s 退避——「循环即重试」，避免单 tick 内自愈阻塞拖垮续期/取消检测。

### 2.7 multica/bridge.rs

职责（对应接入方案 3.2.2）：remote_task ↔ 本地 task/run 衔接。**bridge 直接调 `gold-band` 库层 App API，不走 Tauri command 层**。
- **（Req D）执行注入不在 bridge**：建 App + `create_conversation_run_vm`（requirement 作首轮 prompt，首轮 prompt=requirement.md，不走 submit）+ `client.start_task`（dispatched→running）发生在 `start_multica_conversation_run` 命令里——用户在预填 composer 点「发送」时才触发（claim 与发送之间常驻循环续期 45s lease，见 §2.6）。bridge 的职责是**订阅 lifecycle**：`NodeCompleted` 后用 attempt locator 调 `worker_ref_show` 采集 ACP session_id（worker-ref.json）→ `pin_task_session` 回填 task 行 + 本地 `multica_task_conversations` 索引；run outcome → complete/fail。绑定目录不存在/未绑定 → 引导用户绑定。
- 订阅 `RuntimeLifecycleBus`，把会话事件转译为 multica **基础状态**。**订阅 match 必须穷举 run 终态 4 分支**（见 2.5 终态表），不能只 match success：`NodeStarted` → 任务 running；`NodeCompleted` → 采集 session_id（见下）；`RunCompleted{Success}` → `complete`、`RunCompleted{Failure}` → `fail`、`RunCompleted{Killed}` → 不主动上报（取消检测已命中则仅清本地索引，否则 `fail(timeout)` 兜底）；`RunPaused`/`InterventionRequested` → **不上报终态**，转本地 elicitation/permission 处理（Paused 盲区，multica 继续显示 running，见 2.5）。**本期只上报 start/complete/fail + 心跳，不上报 step/total 进度**（接入方案 3.2.6 状态策略）。`RuntimeLifecycleEvent` 本身不带 session_id——session_id 在 `NodeCompleted` 后主动 `worker_ref_show` 读出（见 2.5）。**单轮即完成**：首轮 run 跑完自然 `RunCompleted`（码灵不在单 remote task 上承载追问）；「多轮」= 新 remote task 续跑同 ACP session（4.4），非单 task 多轮。**不监听 `AcpTurnFinished`**（当前未实现，见 2.5）。
- run outcome 转译：success → `complete_task(output=会话产物摘要, session_id=ACP session_id, work_dir=绑定目录)`；failure → `fail_task(error, failure_reason)`（`failure_reason` 如实传值：runtime_offline / agent_error / runtime_recovery 等，供 server 决定是否 auto-retry，见 4.4）。
- **issue 完成输出传递（M5-bj，issue done 中继的输出附带）**：`RunCompleted` 事件增 `attempt_dir: Option<String>`（编排器构造点透传，镜像 `NodeCompleted`；非 ACP 完成路径 None——bridge 不自行反推路径，desktop_context 是 home app 非 workspace app，repo root 不对会静默提取失败）。Success 分支在 `complete_task` 送达后、`update_issue_status(done)` 前做 `completion_output_for_issue_done(attempt_dir)`：投影最终 assistant 回复 → 提取 `completion-output` 围栏块（§2.1 handoff.rs）→ **status 与 completion_output 一次 PUT 原子上送**（`UpdateIssueStatusRequest.completion_output`，`skip_serializing_if`——缺键 = 服务端保持原值）。提取失败 fail-open：issue 照常 done，写作可选、不门控；有 attempt 却提不出内容时记一条 warn（仅用于观察指令层遵从度，不重试、不阻断）。提示词协议见 `src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md`（**M5-bk 起随首条 prompt 的隐式隐藏区段下发，不再进 composer 预填尾部**）；读路径 `parent_output` 三块隐式区段见 §2.2、接入方案 B2/D2 与 §12.50（改动四十八）。
- 用 multica `task_id`（remote_task_id）作为 remote_task ↔ 本地 `multica_task_conversations` 条目的关联键（remote_task_id → {local_task_id, local_run_id, session_id}）。

**复用**：
- `create_multica_subscriber<R>(app) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>` 照搬 `metrics::create_metrics_subscriber`(metrics.rs:365-638)：
  - **同步闭包**（签名 `Fn` 非 async）——`RuntimeLifecycleBus` 的约定；HTTP 调用必须自己 `tauri::async_runtime::spawn`，**不能阻塞 orchestrator 热路径**
  - 闭包顶部三层 guard（settings + endpoint + pat）一次性做完
  - `match event { NodeStarted => ..., NodeCompleted => ..., _ => {} }`
- 注册：在 `register_lifecycle_subscribers`(commands.rs:481-497) 加一行：
  ```rust
  app.lifecycle_bus.subscribe_named(
      "desktop.multica",
      crate::multica::bridge::create_multica_subscriber(app_handle.clone()),
  );
  ```
  `subscribe_named`(observability.rs:43-49) **幂等**——同名只注册一次。
- 事件优先用 uuid（每个事件都带 display id + `Option<uuid>`，app/mod.rs:568-678）。
- **事件归属反查（多 workspace 并发防串台）**：`RuntimeLifecycleEvent` 带的 `task_id` 是**本地 Gold-Band task uuid**，**不带 remote_task_id、不带 workspace 标识**。多个 App（不同绑定目录）共享同一 bus 时，订阅闭包收到事件后必须反查属于哪个 remote task。设计：
  - 运行期内存反向索引 `local_task_id → remote_task_id`（与 `MulticaRuntimeState.active_runs` 的 remote→local 互逆；在 `multica_task_conversations.insert` / `active_runs.insert` 时同步建立，run 终态清条目时同步删除）。
  - 闭包内先用事件携带的 `repo_root`（每个事件都带，app/mod.rs:568-678）定位到绑定该目录的 workspace/App，再用 `local_task_id` 命中反向索引拿到 `remote_task_id`，**双重过滤**避免把 A workspace 的事件当成 B workspace 的处理（串台）。
  - 反查未命中（local_task_id 不在反向索引）→ 该事件不属于任何在飞 multica 任务，忽略（不误报）。
  - 反向索引纯运行期内存（不落盘），与持久化的正向索引 `multica_task_conversations`（断点续跑用，见 2.2.3）职责分离。

> **不改核心库 runtime 契约**，只调公共 App API + 订阅 bus。

### 2.8 Tauri 命令层

新增命令注册到 `generate_handler!`(main.rs:188-326，metrics 命令在 254/256)，参考 `save_metrics_settings`(commands.rs:1565-1586) 风格。**纯配置读写用同步 `pub fn`（参考 get_metrics_settings commands.rs:1539-1553）；涉及 HTTP 的用 `pub async fn` + `spawn_blocking_command`(commands.rs:76-84)**。save 命令遵循 **6 步**：load → mutate → save_settings → `state.update_settings_config`(state.rs:312，刷新内存让 loop 立刻看到) → reload → return VM。

| 命令 | 同步/async | 入参 | 出参 | 作用 |
|---|---|---|---|---|
| `get_multica_settings` | sync | — | `MulticaSettingsVm` | 读配置（**不含明文 PAT**） |
| `save_multica_settings` | sync | `MulticaSettingsVm` | `MulticaSettingsVm` | 写配置（base_url/enabled 等；PAT 单独走 connect） |
| `connect_multica` | async | — | `MulticaSettingsVm` | **主入口：远程任务管理页未连接空状态点【连接 Multica】**（设置页 multica 区块同步可见，作辅助）；PAT 失效/未连接时触发浏览器登录（localhost callback，4.1 ①-⑦）+ 换 PAT；登录态变更后 emit `multica-task-updated` 让远程任务管理页 re-fetch |
| `disconnect_multica` | sync | — | `MulticaSettingsVm` | **对称于 `connect_multica` 的断开**：清 PAT（`connected` 判定依据，经纯函数 `clear_multica_session`），保留 daemon_id / workspace 绑定（与本机标识、provider 绑定正交），清运行期 register 缓存 `clear_runtime_ids`（重连后 loop 重建）；emit `multica-task-updated` 让远程任务管理页 re-fetch 回到未连接空状态。设置页 multica 区块【断开连接】入口（换账号/退出/本地反复联调；active_runs 保留——在飞本地 run 的 remote 映射，断开不改其归属） |
| `list_server_multica_workspaces` | async | — | `Vec<MulticaWorkspaceRef>` | 拉 server 全量作**添加下拉数据源**（去除已添加） |
| `add_multica_workspace` | async | `{workspace_id, provider}`（**M5-z：去掉 local_path**，name 从 server 列表取） | `MulticaSettingsVm` | 单次添加一个：写 workspaces（**只带 provider，不带 local_project_id**，provider 缺省取 `default_provider`）→ register；可重复调用添加多个。**不再有 folder picker**（本地目录改在执行时由 composer 下拉选） |
| ~~`rebind_multica_workspace`~~ | ~~async~~ | ~~`{workspace_id, local_path}`~~ | ~~`MulticaSettingsVm`~~ | **M5-z：已删除**（绑定模型下沉到任务级，无 workspace 级本地目录绑定可改） |
| `remove_multica_workspace` | sync | `workspace_id` | `MulticaSettingsVm` | 移除（不触发 server 操作） |
| `set_active_multica_workspace` | sync | `workspace_id` | — | **纯视图切换，不 register** |
| `get_multica_tasks` | async | — | `RemoteConversationSidebarVm` | 返回**按 workspace 分组**、对齐 `ConversationSidebarVm` 形状的 VM（`workspaces` / `tasksByWorkspace` / `pinnedTasks` / `recentlyCompleted` 键名一致），供远程任务管理页（`MulticaTaskManagementPage`）镜像渲染。每组下 queued + 失败可重试混排（失败任务带 `retryable` 标记）；`recentlyCompleted` 来自 `state.multica_completed_tasks`，**M5-z：终态行的 `projectId` 直接取自 `MulticaCompletedTask.local_project_id`**（不再按 `workspace_id→local_project_id` 解析）供前端 `onSelectRun` 直达（M5-o）。**保留全部 pending 任务**（含每个 workspace 的 quick-create 初始任务，可作连接测试；M5-p 初版过滤方案已应用户反馈回退）。数据源：远程 queued + 本地 `multica_pending_issues` 未完成 issue（失败回显）+ 本地 `multica_completed_tasks`（完成回看） |
| `claim_multica_task` | async | `task_id, workspace_id` | `RemoteTaskVm`（**含 `requirement`**） | selective claim（命中 `task_conversations.session_id` 带 `prior_session_id` 续跑）；需 `workspace_id` 解析 runtime_id；**claim 响应回填 `requirement`**（来源优先级取首个非空，镜像 server `computeTaskKind`：quick_create_prompt→chat_message→trigger_comment_content→autopilot_description→handoff_note→title）；**claim 成功后写入 `prepare_leases[task_id]`**（让常驻循环续期 45s lease）；**不写 pending_issues**（失败回显改由 M4 fail 写入，见 §2.2.3/§4.3）。**claim 不再立即 start**（Req D：claim-at-click）——返回 requirement 供前端预填 composer；**M5-z：预填的 multica 绑定只含 `{remoteTaskId, workspaceId}`，不含 localProjectId**（本地目录由 composer 下拉选） |
| `start_multica_conversation_run` | async | `input: ConversationCreateInputVm, remote_task_id, workspace_id`（**M5-z：input 内的本地工作区 `projectId` 即任务级 `local_project_id`**） | `ConversationRunVm` | **Req D 新增**（替代已删除的原子 `start_multica_remote_task`）：用户在预填好的 composer 点「发送」后调用。① 解析 `runtime_id`；② 用 composer 下拉选中的 `projectId` 作为任务级 `local_project_id` → `workspace_entry_for_project` 解析 `workspace_path` → 建 workspace `App`（镜像本地 `create_conversation_run`）；③ `validate_conversation_create_vm`（与本地同一校验）；④ **复用 `create_conversation_run_vm(&app, &input)`**（建工作流 + 建任务 + 写 conversation.json + 拷附件 + 启动 run，全部复用本地链路）；⑤ multica 叠加：`register_active_run`（**写入 `ActiveRemoteRun.local_project_id = composer 选中的 projectId`**）+ 落 `multica_task_conversations` + `client.start_task(remote_task_id)`（dispatched→running，lease 不再需要）+ 从 `prepare_leases` 移除；⑥ 返回 `ConversationRunVm`（前端按本地会话同一回调导航 conversation-run） |
| `cancel_multica_prepare_lease` | sync | `remote_task_id` | `—` | **Req D 新增**：从 `prepare_leases` 移除（用户放弃 compose 时调用；兜底是 45s 自然过期回收）。`pub fn`（纯配置/状态读写，无 HTTP） |
| `cancel_multica_task` | async | `task_id` | — | 中断本地 run（复用 gold-band stop session） |
| `rerun_multica_task` | async | `issue_id` | — | 用户手动重试：`POST /api/issues/{id}/rerun` |

**前端事件常量**（commands.rs:62 旁，`gold-band://` 前缀）：
- `gold-band://multica-task-updated`（任务状态变更：claimed/running/completed/failed）
- `gold-band://multica-runtime-status`（runtime 在线/离线、register 结果）

---

## 3. 前端实现（里程碑 M5）

> 双模式架构：**会话模式（新 UI，`/chat/...`，`ConversationMode`）** vs **工作流模式（旧 UI，`/tasks/...`，`WorkbenchMode`）**，`App.tsx:1453` 据 `uiMode` 分流。multica 远程任务列表**仅会话模式**做。

### 3.1 API 四层新增（`web/src/api/`）

按项目既有四层新增 multica 方法，完整调用链：`UI → api.ts(barrel) → client.ts(RuntimeApi 接口) → desktop.ts/browser.ts → invokeCommand → Rust command`。

| 层 | 文件 | 改动（参考 metrics 既有写法） |
|---|---|---|
| 底层 invoke | `shared.ts:8` | 复用 `invokeCommand<T>(name, args?)` + `normalizeCommandError`（已自动把 `CommandErrorVm` 收敛为 `{code, params}`） |
| 抽象接口 | `client.ts:126` | `RuntimeApi` 加 12 个方法签名（`getMulticaSettings/saveMulticaSettings/connectMultica/...`） |
| Tauri 实现 | `desktop.ts:248-253` | `desktopApi` 对象加方法，`invokeCommand<MulticaSettingsVm>('save_multica_settings', {...})` |
| Browser mock | `browser.ts` | 加 mock 实现（供纯浏览器开发/preview） |
| Barrel | `api.ts:300-302` | 薄包装导出：`export function getMulticaSettings() { return getRuntimeApi().getMulticaSettings(); }` |

> UI 组件只 import `api.ts` barrel，不直接接触 client/desktop/browser。

### 3.2 类型定义（`web/src/types.ts`）

镜像后端 VM（参考 `MetricsSettingsVm` types.ts:58）：
```ts
export interface MulticaSettingsVm {
  enabled: boolean;
  toggleLocked: boolean;
  multicaBaseUrl: string | null;
  multicaAppUrl: string | null;   // Web 前端地址（浏览器登录页）
  patSet: boolean;          // ← 永不回显明文 PAT
  daemonIdSet: boolean;
  workspaces: MulticaWorkspaceRef[];
  activeWorkspaceId: string | null;
  defaultProvider: string;   // 缺省 "claude-acp"
  connected: boolean;
}
export interface MulticaWorkspaceRef { id: string; name: string; slug: string; provider: string }  // M5-z：移除 localProjectId（绑定下沉到任务级）
export interface RemoteTaskVm { id: string; issueId: string | null; status: 'queued'|'running'|'completed'|'failed'; retryable: boolean; workspaceId: string; title: string; requirement: string | null /* Req D：claim 响应回填的需求正文，供预填 composer；pending 列表只有 title，正文仅 claim 后有。改动四：issue 型 claim 响应带 issue_description（issue 正文），requirement_text 优先级取首个非空来源，issue 型不再回退 title 而是正文（无正文才回退） */; lastActivityAt: string | null; localTaskId: string | null; runId: string | null; projectId: string | null /* 改动六：仅终态行（completed/failed，来自 multica_completed_tasks）回填，供整行点击 onSelectRun(projectId, localTaskId, runId) 直达本地会话；active 行（queued/running）恒 null。M5-z：终态行 projectId 直接来自 MulticaCompletedTask.local_project_id（任务级） */ }
// Req D：已删除 `MulticaRemoteTaskStartedVm`（原原子 claim+start 命令的返回类型）。执行改为 claim-at-click → 预填 composer → 发送复用本地链路，发送命令返回标准 `ConversationRunVm`。
// 远程列表复用 ConversationSidebar 同一骨架：键名与 ConversationSidebarVm 一致，TaskRow 零改复用
export interface RemoteConversationSidebarVm {
  workspaces: MulticaWorkspaceRef[];
  tasksByWorkspace: Record<string, RemoteTaskVm[]>;   // key = workspace id
  pinnedTasks: RemoteTaskVm[];
  lastActiveWorkspaceId: string | null;
  connected: boolean;        // 未连接 → 前端显示空状态 + 连接入口（不另查 patSet）
}
```
可选嵌入 `AppBootstrapVm`(types.ts:93-108) 让首屏一次拉到，省一次 round-trip。

### 3.3 设置页 multica 区块（`web/src/pages/SettingsPage.tsx`）

**复用** metrics 设置 UI 结构（SettingsPage.tsx:487-523）：复制 `<SettingsSection>` 包裹 + toggle（按 `toggleLocked` 禁用）+ `<Input>`（base_url）+ 保存按钮（`<Loader2 className="animate-spin" />` 加载态）。props 双层解耦参考 SettingsPage.tsx:51-108（父组件可控时由父控制，否则组件自治调 barrel）。

新增内容：
- 开关 `enabled`（channel 锁定时 disabled）
- base_url（API 地址）+ app_url（Web 登录页地址）输入框（channel 预填时只读；二者可能不同）
- 【连接 Multica】/【重新连接】按钮 → `connect_multica`（**辅助入口**；主入口在远程任务列表未连接空状态，见 3.4；首启不自动触发登录；已连接态文案切为「重新连接」）
- 【断开连接】按钮（**仅已连接态显示**）→ `disconnect_multica`：清**账号作用域**状态（PAT + 账号身份 + workspace 绑定 + active workspace）回到未连接，保留 daemon_id（本机持久标识，机器作用域）。`workspace_id` 由当前账号 PAT 发现、仅登录态下有效，断开/换号后残留即脏数据，故随登录态一并清空（M5-m）——断开后任务列表与设置页均回空态，重连同账号需重新绑定 workspace。对称于 connect，不再需手改 settings.json
- `patSet` 状态指示（已连接/未连接），**不展示 PAT 明文**
- **已绑定工作空间管理**（激活/删除；**M5-z：不再有「添加工作空间」行，不再有 rebind**——开发阶段破坏式更新，添加入口收敛到远程任务管理页弹窗，本地目录改为执行时由 composer 下拉选）：列表展示已添加工作空间 + 各自 provider，可切换 active（纯视图）、移除。**M5-z 后不再显示「绑定的本地目录」**（workspace 级不再绑本地目录，本地目录在执行时由 composer 下拉选）；**rebind 入口已删除**（无 workspace 级本地目录绑定可改）。

### 3.4 远程任务管理（M5-z：独立整页 + composer 执行时选本地工作区）

**挂载点：`pages/MulticaTaskManagementPage.tsx`** —— **M5-z：远程任务管理独立成整页**（新路由 `/chat/multica-tasks`，与 agent 管理 / 上下文管理 / 运行模式管理 并列的导航项，icon=Globe），内容镜像 `MulticaRemoteTaskList`（`components/conversation/MulticaRemoteTaskList.tsx`）。**会话侧栏 `components/conversation/ConversationSidebar.tsx` 移除「本地/远程」segmented 切换**（删 `localTab`/`remoteTab` 文案），侧栏纯本地任务（零 multica 入口）。仅会话模式（新 UI）有此页，工作台（旧 UI）不做双胞胎。

- **远程任务管理页按连接态分三种空状态**：
  - **未连接**（无 PAT / PAT 失效）→ 空状态卡 + 【连接 Multica】按钮 → `connect_multica`（**主入口，首启不自动触发登录**）。
  - 已连接、无 workspace → 空状态卡 + 【添加工作空间】（**M5-z：只选远程 + 选 provider**，不选本地目录）。
  - 已连接、有 workspace → workspace 分组列表。
- **页面形态**：左侧 multica workspace 分组（sticky header）→ 每个 workspace 下展开该 workspace 的 queued 任务列表 → **末尾常驻【添加工作空间】Plus 行**（连接态始终可见，对齐本地 `ConversationSidebar` 末尾添加入口）→ 点击弹**模态对话框 `MulticaAddWorkspaceDialog`**（**M5-z：只有远程工作空间下拉 `listServerMulticaWorkspaces` 过滤已添加 + provider 下拉 + 底部【添加】**，**不再有「绑定本地目录」按钮、不再调 `pickLocalDirectory`**，两项齐全才可点 → `addMulticaWorkspace(workspaceId, workspaceName, provider)`）。**已连接但无添加工作空间**时空状态文案区分（`conversation.sidebar.multica.noWorkspacesBound`，引导去添加）。`get_multica_tasks` 返回**按 workspace 分组**、对齐 `ConversationSidebarVm` 形状的 VM（不限于 active）；**失败可重试任务与 queued 同列混排**，仅多一个 `retryable` 标记 + 【rerun】按钮（不单开失败区）；每条【执行】（Req D：claim-at-click + M5-z 执行时落地）→ `claimMulticaTask(taskId, workspaceId)` 拿 `requirement` → 经 composer draft context `prefill(requirement ?? title, {remoteTaskId, workspaceId})`（**M5-z：multica 绑定不含 localProjectId**）→ `onNewConversationInWorkspace()`（**M5-z：不传 localProjectId**，落 conversation-home 由 composer 选），输入框已预填需求。
- **（M5-z 核心）composer 执行时选本地工作区（claim-at-click → 执行时落地）**：
  - App 预选最近活跃本地工作区（`activeWorkspaceId ?? lastActiveWorkspaceId`）。
  - **composer 强制显示本地工作区下拉**——即便只有 1 个本地工作区，只要 multica 绑定激活就强制显示（普通会话只有 1 个本地工作区时本会隐藏，multica 绑定激活时破例），用户可改；改工作区时**保留 multica 绑定与预填正文**（只换本地工作区 id）。
  - **0 个本地工作区时** composer 显示「请先添加本地工作空间」引导（i18n `conversation.composer.multicaNeedLocalWorkspace`）并禁用发送。
  - 用户点「发送」→ `App.tsx` conversation-home 的 `onSubmit` 据 `composerDraft.draft.multica` 分流：有绑定 → `startMulticaConversationRun(input, draft.multica.remoteTaskId, draft.multica.workspaceId)`（用 composer 下拉选中的 `projectId` 写入任务级 `ActiveRemoteRun.local_project_id`，返回 `ConversationRunVm`），否则本地 `createConversationRun(input)`；二者后续导航/侧栏刷新/reset 完全一致。multica 绑定纳入 draft 生命周期——`reset`（发送成功 / 放弃 compose）即清掉，无需各 reset 点单独清理。composer 草稿 app-root 上提（`ConversationComposerDraftBoundary` 包 `<Shell>`），跨导航存活。
  - 该远程任务执行时所在的本地侧栏对应工作区出现该任务（与本地会话同一刷新机制）。
- **（M5-o→改动六）可折叠分区 + 终态任务回看 + 按 run 直达**：
  - **可折叠**：workspace 分组 / pinned 失败段两段均**可折叠**（镜像本地 `ConversationSidebar` 的 `expandedWorkspaces` state + `ChevronDown` 旋转 + `<button>` header + 条件渲染；不用 shadcn Collapsible，跟随本地侧栏手动 toggle 风格）。
  - **终态任务并入所属工作空间组（改动六，取代 M5-o 扁平「最近完成」桶）**：原 M5-o 在 workspace 分组下方单设全局「最近完成」可折叠分区（遍历 `vm.recentlyCompleted`），不同 workspace 的终态任务挤在同一列、可读性差。改动六**删除**该分区 + `RemoteConversationSidebarVm.recentlyCompleted` 字段 + `MulticaCompletedTaskVm`（破坏式更新，无兼容层）；终态任务（`state.multica_completed_tasks`，§2.2）由 `get_multica_tasks` 按 `workspace_id` 并入对应工作空间的 `tasksByWorkspace` 组（`RemoteTaskVm::from_completed` + 纯函数 `merge_workspace_tasks`；**改动七升三参 running/pending/terminal、running > pending > terminal 按 `remote_task_id` 去重**，详见 §12.9）。**M5-z：终态行的 `projectId` 直接来自 `MulticaCompletedTask.local_project_id`（任务级），不再按 `workspace_id→local_project_id` 经 workspaces 列表解析**（任务自带本地目录）。终态行行内带 `localTaskId/runId/projectId`，整行可点 → `onSelectRun(projectId, localTaskId, runId)` 直达本地会话；`completed_at` → `lastActivityAt` 复用既有时间渲染。
  - **按 run 直达**：`MulticaRemoteTaskList` props 用 `onSelectRun(projectId, taskId, runId)`（复用本地侧栏直达指定 run 的现成回调），终态行点击 → `onSelectRun(projectId, localTaskId, runId)`，绕开「在陈旧 sidebar 快照里查刚创建的任务」的根因（直达会话页）。
- **（M5-p）任务时间本地时区展示**：pending 行 `lastActivityAt` 与终态行 `lastActivityAt`（改动六：`from_completed` 把 `completed_at` 映射为 `lastActivityAt`）改用既有 `web/src/lib/datetime.ts::formatLocalDateTime(iso)`（内置 Date 解析 ISO/epoch → 本地时区 `YYYY-MM-DD HH:mm:ss`），替换原 `.slice(0,19).replace('T',' ')`（旧实现把 UTC 墙钟当本地时间展示，偏差一个时区）。**UTC 存储不变**（`bridge.rs::finalize_terminal` 存 `Utc::now().to_rfc3339()` canonical 正确），纯展示层转本地。
- **（§12.10 / M5-aa）页头 UX + 状态色调 + 看板词汇对齐**：
  - **页头**：`MulticaTaskManagementPage` 的 `PageHeader.actions` 内含「任务来源」`Select`（i18n `multica.taskManagement.source.label`，当前唯一项 Multica），由 `REMOTE_TASK_SOURCES` 配置数组驱动、页级 `source` state 作渲染分流唯一键（`source === 'multica' ? <MulticaRemoteTaskList/> : null`）；副标题 `multica.taskManagement.subtitle` = "查看并执行远程任务"（M5-ab 再精简：页头「任务来源」下拉已点名来源，副标题不再重复 "multica" 限定词；原尾部关于执行时选本地目录的赘述半句早已删除）。
  - **手动刷新**：`MulticaRemoteTaskList` 顶部右侧常驻 ghost 图标按钮（`RotateCw` + `aria-label="common.refresh"` + Tooltip），`handleManualRefresh` 调与 mount / 事件订阅同源的 `fetchTasks()`，`refreshing` 态驱动 `animate-spin` 并 disable 按钮（不复用 `loading`——loading 用整屏 spinner 替换列表）。
  - **状态色调徽章（`MULTICA_STATUS_TONE` 导出 const）**：`MulticaRemoteTaskList.tsx` 导出 `Record<string, string>`，每个 canonical status 锁定一个 Badge className——`queued`=灰（`bg-muted text-muted-foreground`）/ `running`=黄（`bg-amber-500/15 ...`）/ `completed`=绿（`bg-emerald-500/15 ...`）/ `failed`=红（`bg-destructive/15 ...`），经 `<Badge className={cn('...', statusTone)}>` 应用、缺键回退 `queued` 灰。结构化管理色调（杜绝硬编码 / 散落三元）。
  - **看板词汇对齐（canonical→display 文案）**：`queued` 显示「待办」（Todo，旧文案暗示独立领取中间态、与 claim-at-click 冲突，已替换不保留）；`running=进行中`/`completed=已完成`/`failed=失败`。后端 canonical 状态不变，仅前端 i18n 文案对齐看板（码灵作为 daemon 直接驱动 board issue.status，本地任务生命周期与看板词汇 1:1）。详见 §12.10。
- **（§12.12 / M5-ab）列表树形视觉/层级系统（统一一次做完非补丁）**：workspace→任务树建立一致间距节奏与层级表达，仅 `MulticaRemoteTaskList.tsx`（含 `RemoteTaskRow`）改，会话模式远程任务管理页专属：
  - **组头（workspace 分组 + pinned 段）**：统一 `ChevronDown` 折叠箭头 `size-3.5 text-muted-foreground`（旋转表达展开/折叠）；workspace 名称左侧加 lucide `Server` 图标（同规格）区分「工作空间容器行」vs 下方「无图标任务叶子行」（pinned 段非 workspace 不加）；workspace 名称右侧加任务计数（i18n `conversation.sidebar.multica.taskCount`，`text-[11px] text-muted-foreground`，`tasks.length > 0` 才显示）；整行 `rounded-md px-1.5 hover:bg-muted/40 transition-colors`（容器 hover 底色，取代仅文字变色）。
  - **任务行（`RemoteTaskRow`）**：标题 `text-[14px] font-medium text-foreground`（14px 侧栏密度上限、font-medium + 全强前景色作主文本）；元信息行 Badge 居左、时间戳 `ml-auto` 推右 `text-[10px] tabular-nums text-muted-foreground`（对齐本地侧栏时间戳规格）；整行 `px-2 rounded-md hover:bg-muted/40`；任务列表容器加 `pl-2` 统一缩进。
  - **间距系统**：水平 padding 组头 `px-1.5` / 任务行 `px-2` 对齐；垂直节奏 header(刷新工具栏)↔树 `space-y-2`、组↔组 `mb-2`、任务↔任务 `space-y-0.5`、组头↔其任务 `mt-0.5`（旧 `mb-1` 加大为 `mb-2`）。
  - **组内空状态**：i18n `noTasksInWorkspace` 改为「该工作空间下暂无远程任务」/"No remote tasks in this workspace yet"；样式由 `px-2 py-1` 改为 `px-2 py-4 text-center`（居中带垂直留白）。
  - **副标题**：`multica.taskManagement.subtitle` 再精简为「查看并执行远程任务」/"View and run remote tasks"（页头「任务来源」下拉已点名来源，不再重复 "multica" 限定词）。
  - i18n 双语新增/更新：`conversation.sidebar.multica.taskCount`（zh「（{{count}}个任务）」/ en「({{count}} tasks)」）、`noTasksInWorkspace` 文案更新、`multica.taskManagement.subtitle` 再精简。详见 §12.12。

### 3.5 事件监听 + i18n

- 监听 `gold-band://multica-task-updated`：**任务生命周期**（claim/start/complete/fail/cancel、取消检测作废）→ 远程任务列表 re-fetch（`listen<T>` 参考 desktop.ts:24-51）。由 bridge 终态上报 + loop 取消检测 emit + **start_multica_conversation_run 成功路径 emit（改动七：active_runs 已登记，让 running 行即时刷出，见 §12.9）**。**（M5-o）App 顶层亦订阅此事件**：multica 任务创建会在本地工作空间落一条会话任务，需同步刷**本地侧栏**（`getConversationSidebar`+`applyConversationSidebar`，in-flight/pending 去抖，对齐 agent-registry 模式）——否则 multica 路径不像正常 `createConversationRun` 那样手动 refresh 本地侧栏，导致 multica 任务在本地侧栏不出现/状态不更新（「会话看不到」根因之一）。远程任务列表与 App 顶层**各订阅一份**（远程列表刷 `getMulticaTasks`，App 顶层刷本地侧栏，职责不同）。
- 监听 `gold-band://multica-settings-updated`：**连接/工作空间配置变更**（connect/disconnect/save/add/remove/set_active；**M5-z：rebind 已删除，不再 emit**）→ 任务列表 **+ 设置页** 都 re-fetch。任一处发起的配置改动，两端即时同步（杜绝「绑定发生在任务列表弹窗、设置页显示旧数据」之类的跨视图不一致）。
- 监听 `gold-band://multica-runtime-status`：连接状态、register 结果提示。
- i18n（`web/src/i18n.ts`）：新增 `settings.multica.*` 与 `tasks.remote.*` 中英文 key（CLAUDE.md 维护双语）。i18n key 命名参考 `settings.metrics.*`。**前端按错误码 code 查文案**，后端不返回对客文案。
  - **M5-z i18n 调整（绑定模型下沉到任务级 + 独立整页）**：**新增** `multica.taskManagement.{title,subtitle}`（远程任务管理页标题/副标题，中英双语）、`conversation.sidebar.multicaTaskManagement`（侧栏导航项文案，icon=Globe）、`conversation.composer.multicaNeedLocalWorkspace`（composer 0 本地工作区引导，禁用发送时显示）；**删除**已失效的 `conversation.sidebar.multica.localTab`/`remoteTab`（侧栏 segmented toggle 文案）、dialog 目录相关 key（`bindDirectory`/`changeDirectory`/`directoryPlaceholder`/`needDirectory`）、`settings.multica.{connecting,disconnecting,connected,disconnected,addWorkspace,selectServerWorkspace,selectProvider,selectFolder,rebind,notGitWarning}`（设置页添加行/rebind/folder picker 全链路删除后失效）。

---

## 4. 关键流程详设

### 4.1 首启登录链路（含登录复用）

```
启动客户端
  └─ verify_pat() : GET /api/me (Bearer PAT)
       ├─ 200 → PAT 有效，直接复用，跳到 4.2（启动全量 register）
       └─ 401 / 无 PAT → **静默、不弹任何 UI**（首启不自动登录）；用户切到「远程任务列表」未连接空状态点【连接 Multica】后，才走下面浏览器登录连接流程（复用 multica 原生邮箱登录，server 零改动）：
            ① 用户切到「远程任务列表」（未连接空状态）点【连接 Multica】（设置页辅助入口；**首启不自动触发**）
            ② 起临时本地 HTTP server（127.0.0.1:<port>，监听 /callback）
            ③ 打开系统浏览器到 <MULTICA_APP_URL>/login?cli_callback=http://127.0.0.1:<port>/callback
                 → 用户邮箱登录（multica Web 原生流程）
            ④ multica Web 登录成功先 setLoggedInCookie，再校验 cli_callback 白名单（validateCliCallback，已放行 localhost/127.0.0.1/RFC1918）
                 → 302 回跳 http://127.0.0.1:<port>/callback?token=<JWT>
                 → 本地 server 收 JWT：向浏览器回 302 → <MULTICA_APP_URL>/（带 cookie 落 multica web 登录态，不渲染结果页），关闭监听
            ⑤ POST <MULTICA_BASE_URL>/api/tokens (Bearer JWT) {name:"Maling Desktop", expires_in_days:90}
                 → {token:"mul_..."}（明文仅此一次）
            ⑥ GET <MULTICA_BASE_URL>/api/workspaces (Bearer PAT) → 拉 server 全量
                 ├─ 空 → multica.workspace-empty
                 └─ 【添加工作空间】（M5-z：只收远程 + provider）：从下拉列表选一个远程 workspace + 选 provider（默认 claude-acp）绑定 → register（取回 runtime_id）→ 写入 workspaces（**只带 provider，不带 local_project_id**——本地目录改在每次执行时由 composer 下拉选）；首个 workspace 自动设 active
            ⑦ 持久化 PAT + workspaces + active（SettingsConfig 明文）；JWT 用完即弃，不落盘
```

> **登录复用**：每次启动先 `GET /api/me` 校验 PAT，有效直接复用（跳过①-⑤）。PAT 临期(<7天)调 `POST /api/tokens/current/renew` 续期。浏览器登录仅在 PAT 失效或首绑时触发，无静默后台请求。`app_url`（Web 登录页）与 `base_url`（API）分离：浏览器登录用 app_url，其余 HTTP 调用用 base_url。

### 4.2 启动全量 register

```
start_multica_runtime()
  ├─ verify_pat() → 有效复用 / 无效触发 4.1 登录
  ├─ 遍历 desktop_multica_workspaces（已添加列表）逐个 register：
  │     POST /api/daemon/register {workspace_id, daemon_id, provider: workspace.provider（如 "claude-acp"）, ...}
  │     → 取回 runtime_id，写入 multica_runtime_ids[workspace_id]   ← 幂等，同一 workspace 永远稳定
  ├─ 对每个 runtime_id 调 POST /runtimes/{rid}/recover-orphans {}
  │     → 残留在飞任务清理为失败态（runtime_recovery）；server 对 runtime_recovery 等 retryable reason 自动重试 max_attempts=2（本期接受）
  └─ 失败任务（multica_pending_issues 中的）以 retryable 标记混排进任务列表，用户点 rerun
```

> **register 只在两时机**：新添加 workspace 时 / 客户端启动时（全量）。切换 active = 纯 UI 视图，不 register。同一 workspace runtime_id 永远稳定（已绑 agent 直接复用，首次 register 的新 workspace 需在 web 绑 agent）。

### 4.3 任务执行循环

> ⚠️ **本段及下方流程图为 M5-q / M5-z 的 claim-at-click 原始设计记录，已被 §12.23（M5-ak）claim-at-send 取代**——点「执行」现只读拉需求正文（任务仍 queued），claim+start 推迟到点「发送」同一事务，prepare-lease 续期机制（extend_prepare_lease / cancel_multica_prepare_lease / prepare_leases）整条删除。判定以 §12.23 为准。
>
> **（Req D + M5-z：执行流程改造）** 不再「点【执行】→ 原子 claim+start+拉起会话」。改为**点击即领取(claim-at-click) + 执行时选本地工作区(执行时落地)**：点【执行】→ claim 拿 requirement → 预填 composer（与本地『+』同一界面，唯一区别是输入框已预填需求正文 + multica 绑定激活强制显示本地工作区下拉）→ **用户在 composer 选/改本地工作区**（M5-z：选中的 projectId 即任务级 `local_project_id`）+ 选模型/模式 → 点「发送」才真正建会话执行。compose 期间常驻循环续期 prepare lease（45s）防回收；放弃 compose → `cancel_multica_prepare_lease`（或 45s 自然过期回收）。

```
用户在远程任务管理页点某条【执行】(play)  ← claim-at-click（Req D + M5-z 执行时落地）
  ├─ claim_multica_task(task_id, workspace_id) : POST /runtimes/{rid}/tasks/{tid}/claim {}  ← selective claim（body 空，服务端不读请求体）
  │     → {task:{id, issue_id, auth_token, prior_session_id?, parent_task_id?, ...}}；claim 响应的 parent_task_id/prior_session_id 写入 prepare_leases[tid]，供 start 续跑判定按 parent_task_id 反查父任务本地索引（见 §12.14）
  │     → 后端回填 VM.requirement（来源优先级取首个非空）；写入 state.prepare_leases[tid]（让常驻循环续期 45s lease）
  │     → 前端（M5-z）：composerDraft.prefill(requirement ?? title, {remoteTaskId, workspaceId})  ← multica 绑定不含 localProjectId
  │              → onNewConversationInWorkspace()（不传 localProjectId，与本地『+』同一回调）→ 落 conversation-home（composer 已预填）
  │              → composer 强制显示本地工作区下拉（即便只 1 个），App 预选 activeWorkspaceId ?? lastActiveWorkspaceId；0 个本地工作区显引导并禁用发送
  │       （**不写 pending_issues**——其语义为「失败待重试」，改由下方 Failure 分支写入，见 §2.2.3）
  │
  ├─ [compose 期间] 常驻循环每 15s extend_prepare_lease(tid)（防 45s 回收，见 §2.6）
  │   放弃 compose → cancel_multica_prepare_lease(tid)（移除 lease）；或 45s 自然过期 → server 回收回 queued
  │
  ├─ 用户在 composer 选本地工作区 + 选模型 + 点「发送」→ start_multica_conversation_run(input, remote_task_id, workspace_id)
  │     │   （M5-z：input 中的 projectId 来自 composer 下拉选中，即任务级 local_project_id）
  │     ├─ 用 composer 选中的 projectId 作为 local_project_id → workspace_entry_for_project 解析 workspace_path → validate_conversation_create_vm(&app, &input)（与本地同一校验）
  │     ├─ create_conversation_run_vm(&app, &input)（**复用本地链**：建工作流→create_task_from_requirement(requirement)→
  │     │      写 conversation.json→拷附件→run_start_background，requirement 作首轮 prompt=requirement.md，不走 submit）
  │     ├─ multica 叠加：register_active_run(state, run.id, remote_task_id, workspace_id, local_project_id /* composer 选中的 */, issue_id, title)
  │     │      + 落 multica_task_conversations[tid]（session_id=None 待 bridge 回填）+ client.start_task(tid)（dispatched→running，lease 移除）
  │     └─ 返回 ConversationRunVm（前端按本地会话同一回调导航 conversation-run；写本地侧栏）→ 该远程任务出现在本地侧栏对应工作区
  │     NodeCompleted 后 worker_ref_show 读 continue_ref.acpSessionId → pin_task_session + 回填 multica_task_conversations[tid]
  ├─ 执行期：常驻心跳每 3s POST /heartbeat {runtime_id}（**Req D：已常驻，非仅执行期**；维持在线，否则 150s 判离线→fail；M5-cx 由 15s 提频）
  │          执行期按 500ms 批上报 /messages（text/tool_use/tool_result/error）+ 节点/turn 边界上报 /usage（M5-cy，§12.95）
  │          progress（step/total）不做：multica 侧仅 WS 广播、不持久化、无 Web 消费方
  │          周期 GET /tasks/{tid}/status 检测取消（cancelled/failed/404 → 中断本地 run）
  └─ run outcome（**首轮 run 终结即触发，单轮即完成**；码灵不在单 remote task 多轮，多轮走新 task 续跑同 session，见 4.4）。订阅器穷举 4 分支（见 2.5 终态表）：
       RunCompleted{Success} → POST /tasks/{tid}/complete {output, session_id=ACP session_id, work_dir=任务级 local_project_id 解析, completion_output=交付说明（M5-cd：issue 任务随 complete 同事务回传，块优先/全文兜底 + 超长 400 立即降级，§12.72）}（重试幂等）→ 清 multica_task_conversations[tid]
       RunCompleted{Failure} → POST /tasks/{tid}/fail {error, failure_reason=agent_error.*}（重试幂等；如实传值供 server 决定 auto-retry）+ 记 issue_id 进 multica_pending_issues（失败回显，供 rerun；M4 实现）
       RunCompleted{Killed}  → fail(timeout)（M4-d：cancel 路径皆经 run_pause→Paused 从不产生 Killed，Killed 必为 agent 真死，unconditional）
       RunPaused/InterventionRequested → **不上报终态**，转本地 elicitation/permission 处理（Paused 盲区，multica 继续 running，见 2.5）
```

### 4.4 失败恢复链路（resume-safe 续跑 / resume-unsafe rerun）

```
中断（崩溃/关闭）
  └─ 心跳断 → 150s 后 runtime 离线 → remote_task fail(runtime_offline)（retryable，server 可能 auto-retry）

下次启动客户端
  ├─ 全量 register（4.2）
  ├─ recover-orphans：把上次在飞 remote_task 无条件清为 runtime_recovery 失败态
  │     → server 对 runtime_recovery 等 retryable reason 自动重试 max_attempts=2（本期接受）
  │     → retryable 失败或不可重试的（agent_error）以 retryable 标记混排进任务列表
  └─ 用户点【rerun】（仅对 agent_error 等不可自动重试、或重试耗尽的任务）
       → rerun_multica_task(issue_id) : POST /api/issues/{id}/rerun (X-Workspace-ID)  ← rerun=true/retry=false
         → 创建全新 queued 任务（force_fresh_session），按 issue assignee 的 runtime 路由
         → 新 task_id → 本地 multica_task_conversations 新条目（与旧 session 无关，整任务重跑）

断点续跑（runtime_recovery / runtime_offline 重派回本机——**auto-retry 克隆新 id 的子任务 T'，非同 task_id**，续跑靠 parent_task_id 反查父 T 本地索引，见 §12.14）
  ├─ claim 响应带回 parent_task_id（指向父任务 T）+ prior_session_id（服务端回填的父 session）→ 写入 prepare_leases[T']
  ├─ **单一执行入口**（Req D：改名 `start_multica_conversation_run`，原 `start_multica_remote_task`）：
  │     复用 `create_conversation_run_vm` 建 task+run 前先 `classify_resume(&app, remote_task_id, parent_task_id)`
  │     两级反查——① multica_task_conversations[T'.id]（同 id 场景：dispatched lease 过期同 row 重派）
  │                ② miss 且 parent_task_id 有 → multica_task_conversations[T]（auto-retry 子任务场景）
  │     命中后仍校验本地 run is_run_continuable → Resume；否则 Fresh
  ├─ Resume：run_continue_background_with_config_overrides(local_task_id, local_run_id, None,None,[],None,None)（沿用父任务 local ids；内部读 worker-ref continue_ref，session/load 续跑同一会话）
  │           续跑成功后**迁移索引**：multica_task_conversations[T'] = 父 entry（local ids + session 沿用），remove(T)——保证多次重试 T→T'→T'' 链式可续
  └─ session 已死（strict_continue 报错 acp/client.rs:2132-2141）或任何 resume Err → fresh fallback：
        复用 create_conversation_run_vm 建 task+run + start_task(force_fresh_session=true)（新本地 task，整任务重跑）
```

> **现象速查：关闭码灵后控制台的可观察表现**（完整版见接入方案 §1.5；源码 `server/cmd/server/runtime_sweeper.go`）
> - runtime 仍显示「在线」约 3 分钟 = `150s 判离线 + 30s sweeper 周期` 的**设计内延迟**（非 bug；150s 故意大于 105s 最坏 DB 滞后并留余量，调小会误杀健康长任务，`runtime_sweeper.go:22-31`）。
> - ~3 分钟 runtime 翻「失联」时，**同一 sweeper tick** 内 `FailTasksForOfflineRuntimes`（`runtime_sweeper.go:171`）把在飞任务置 `failed(runtime_offline)`——即上流程「150s 后 runtime 离线 → fail」是同 tick 发生，不存在额外等待。
> - 但 `runtime_offline` 命中 auto-retry（`max_attempts=2`），原始 attempt 失败**同事务**克隆 attempt N+1 → 控制台通常看到「重试中的新任务」而非「失败」；最终稳定 failed 取决于重试落点（dispatched→几分钟内；queued→2h `queued_expired`）。
> - **结论**：码灵客户端无需为「3 分钟才失联」调整——属心跳缺席架构下限，三种中断场景通用；可选的「正常关闭主动 deregister」未采纳（仅对干净退出有效，收益有限）。

> **⚠️ 远程 fail 本地作废的「failed」歧义（M4-d 决策，偏离设计字面）**：
> `GET /tasks/{id}/status` 返回 `failed` 无法区分 **retryable**（resume-safe，server 重派子任务供客户端按 parent_task_id 反查续跑）与 **terminal**（agent_error，应作废）。若一律作废会丢失续跑索引、击穿断点续跑。决策：
> - **C2 启动 reconcile**（崩溃残留 orphan）：**仅 `cancelled`/404 作废，不在 `failed` 作废**——保 retryable 续跑；terminal-failed 的本地 Paused run 由 **strict_continue fallback** 兜底（用户续跑死 session→自动降级 fresh）。
> - **C3 周期 cancel-detection**（在飞 active_run）：`failed`/`cancelled`/404 作废——active run + remote terminal = 停本地（省算力、同步状态），与 resume 路径（崩溃后 Paused run 的 re-claim）互斥，无冲突。
> - 取消/作废共用 `bridge::teardown_active_run`（run_pause+杀 ACP+清 active_runs+清 task_conversations[remote]）。

> **失败分类（接入方案 3.2.3 / 3.2.7）**：
> - **resume-safe（runtime_offline / runtime_recovery / timeout）**：server 自动重试 max_attempts=2，重派回本机时带 prior_session_id **断点续跑**同一会话。
> - **resume-unsafe（agent_error）**：不自动重试，需用户手动【rerun】整任务重跑（force_fresh_session）。
> recover-orphans 无条件把残留任务清为 runtime_recovery（retryable），由 server 决定重试。本期接受 multica 自动重试机制，不额外干预。
>
> **⚠️ multica fail 与本地 run Paused 的关系（需厘清）**：gold-band 的 `recover_interrupted_running_sessions`(app/mod.rs:2397-2401) 对本地 run 的策略是**只标记 Paused（ProcessInterrupted）不恢复运行**，让用户可 continue。但 multica remote_task 已在 server 侧 fail——此时本地对应 run 即便被标 Paused 也**不应再 continue**（remote_task 已作废）。处理：bridge 在检测到 remote_task 已 fail（recover-orphans 后）时，把对应本地 run 从 Paused **作废**（cancel，不 continue）；用户 rerun 会创建全新 remote_task + 全新本地 run。两者状态机独立：multica 侧 fail 作废，本地侧被同步作废。

---

## 5. 错误码完整定义（结构体，后端只返回 code+params）

| code | HTTP 映射 | 触发场景 | 前端处理（i18n 文案由前端查） |
|---|---|---|---|
| `multica.not-configured` | — | 未填 base_url / PAT | 引导去设置页连接 |
| `multica.auth-failed` | 401/403 | PAT 无效 / JWT 换 PAT 失败 / runtime 不属本用户 | 提示重新连接（connect_multica） |
| `multica.workspace-empty` | — | 用户在 multica 无 workspace | 提示先在 multica web 建 workspace |
| `multica.network-failed` | — | HTTP 不可达（重试用尽） | 提示网络/base_url 问题 |
| `multica.register-failed` | — | register 调用失败 | 提示重试 / 检查配置 |
| `multica.claim-conflict` | 409 | task 已被领 / 非 queued | 刷新列表 |
| `multica.task-not-found` | 404 | task 不存在/不属该 runtime/串行化冲突 | 刷新列表 |
| `multica.runtime-offline` | — | runtime 被判离线 | 提示检查心跳/重启 |

> 所有错误码以 `MulticaError` → `CommandErrorVm { code, params }` 返回；`params` 携带上下文（task_id/workspace_id 等），**不含对客文案**。

---

## 6. 状态机（remote_task 生命周期）

```
                  ┌─────────── rerun (用户点) ───────────┐
                  ▼                                       │
  queued ──claim──▶ dispatched ──start──▶ running ──complete──▶ completed (终态)
    │                 │                      │
    │ TTL 2h          │ 5min 未 start        │ 中断/离线/超时
    ▼                 ▼                      ▼
  failed(queued   failed(timeout)        failed(runtime_offline /
    _expired)                              runtime_recovery / agent_error)
                                              │
                                              ├─ runtime_recovery / runtime_offline / timeout（resume-safe）
                                              │    └─ server auto-retry（max_attempts=2）→ 重派回本机带 prior_session_id 续跑（4.4）
                                              └─ agent_error（resume-unsafe）→ 不自动重试，留待用户 rerun（→ 新 queued）
```

与本地会话状态同步（bridge 维护 `multica_task_conversations`：remote_task_id → {local_task_id, local_run_id, session_id}，2.5）：
- **完成判定**：**一个 remote_task = 一个本地 run（一次执行），不是多轮会话**。run 终结（`RunCompleted`）↔ remote `completed`/`failed`，无「多轮何时算完」歧义。码灵不在单 remote task 上承载追问；「多轮」= 新 remote_task claim 时带 `prior_session_id` 续跑同一 ACP session（4.4），每个 task 独立 complete。**run 终态 4 分支映射见 2.5**（Success→complete / Failure→fail / **Killed→fail(timeout)（M4-d）** / Paused→不上报本地处理）。
- remote `running` ↔ 本地会话执行中（NodeStarted→running；含本地 Paused 等待输入期间——multica 端无 paused 态，继续显示 running，见 2.5「Paused 盲区」；本期不上报 step/total progress）
- remote `completed`/`failed` ↔ 本地会话 outcome（success/failure）
- remote `failed`（recover-orphans 后，retryable）↔ 本地会话**暂不作废**（保断点续跑索引，等 server 重派子任务供客户端按 parent_task_id 反查续跑）；仅 remote `cancelled`/404（C2 启动 reconcile）/ 在飞 active_run 的 `failed`+`cancelled`+404（C3 周期检测）→ 作废本地 run（4.4 ⚠️）

---

## 7. 接口契约速查（基于接入方案 4.2）

| 组 | 接口 | method | 鉴权 | 备注 |
|---|---|---|---|---|
| A0 | `<MULTICA_APP_URL>/login?cli_callback=...` | 浏览器 | multica 原生邮箱登录 | 复用，**server 零改动**（见 4.1） |
| A1 | `/api/tokens` | POST | JWT | 一次性换 PAT |
| — | `/api/me` | GET | PAT | 校验 PAT（登录复用） |
| — | `/api/tokens/current/renew` | POST | PAT | PAT 续期 |
| A2 | `/api/daemon/register` | POST | PAT | 幂等，启动全量 + 新添加；`provider` = workspace 选定值（claude-acp 等，决定该 runtime 跑哪个 ACP） |
| A3 | `/api/daemon/runtimes/{rid}/recover-orphans` | POST | PAT | 清理为失败态 |
| B1 | `/api/daemon/runtimes/{rid}/tasks/pending` | GET | PAT | 只读列表 |
| B2 | `/api/daemon/runtimes/{rid}/tasks/{tid}/claim` | POST | PAT | **M0 新增 selective claim**；body 可带 prior_session_id（断点续跑） |
| B3 | `/api/attachments/{id}/download` | GET | **mat_ 任务令牌**（claim 响应） | **M5-bu 新增消费**：issue 直属附件下载（claim 响应 `issue_attachments` 清单；DPMS 需求文件服务端实时代理；码灵中介下载后并入原生任务输入附件通道，见 §12.61）。路由语义（webank `file.go`）：`/api/attachments/{id}` 是 workspace 作用域**元数据 JSON**（id/url/filename），文件本体在 `/download` 子路径（user-scoped auth-only，接受 mat_ actor 凭证；服务端 CloudFront 302 / presign 302 / proxy 直出三模式，客户端默认跟随重定向、跨主机自动剥 Authorization，对签名 URL 正确）——M5-bu 首版误用元数据路由，§12.63 修正 |
| C1 | `/api/daemon/heartbeat` | POST | PAT | **常驻 3s（Req D：建立连接后即对全部已连接 workspace 持续，非仅执行期；M5-cx 由 15s 提频 + 工单完成后补发排空）** |
| C2 | `/api/daemon/tasks/{tid}/start` | POST | PAT | dispatched→running；body `force_fresh_session`（整任务重跑=true） |
| C2.5 | `/api/daemon/runtimes/{rid}/tasks/{tid}/prepare-lease` | POST | PAT | **Req D 新增续期**：claim-at-click 后、start 前（用户在 composer 选模型/改需求期间）每 15s 续期 45s lease，防 `ReclaimStaleDispatchedTaskForRuntime` 回收（与 multica daemon 自带 `startTaskPrepareLeaseExtender` 同构） |
| C3 | `/api/daemon/tasks/{tid}/progress` | POST | PAT | **本期不接入**（状态只基础 start/complete/fail） |
| C5 | `/api/daemon/tasks/{tid}/status` | GET | PAT | 检测取消 |
| C6 | `/api/daemon/tasks/{tid}/complete` | POST | PAT | 重试幂等；body `session_id`+`work_dir` |
| C8 | `/api/daemon/tasks/{tid}/session` | POST | PAT | PinTaskSession：写 `session_id`/`work_dir` 到 task 行（断点续跑依据） |
| C7 | `/api/daemon/tasks/{tid}/fail` | POST | PAT | 重试幂等 |
| D1 | `/api/issues/{id}/rerun` | POST | PAT | X-Workspace-ID；用户手动重试 |
| D2 | `/api/issues/{id}` | PUT | PAT | X-Workspace-ID；issue 状态流转（M5-w in_progress / M5-t done）；**M5-bj 起 done 可带 `completion_output`**（≤16k rune，2026-09-21 从 64k 收紧；`completion-output` 围栏块提取，一次 PUT 原子上送，缺键=服务端保持原值） |

> 业务接口（D1/E1/E2）需带 `X-Workspace-ID` 头。完整字段见接入方案 4.2 与 multica `daemon/types.go`。

---

## 8. 里程碑与任务拆解

| 里程碑 | 范围 | 任务 | 验收 |
|---|---|---|---|
| **M0** | server | ① ClaimSpecificQueuedTask SQL（保留 NOT EXISTS 串行化守卫） ② ClaimSpecificTask service（复制 6 步收尾链） ③ ClaimSpecificTask handler + 路由 ④ `make sqlc` ⑤ 单测（**登录无 server 改动**，复用原生） | 1.2.7 |
| **M1** ✅ | 凭证/登录 | ① config.rs（Settings/State/Channel 字段含 app_url + pat_set + daemon_id 生成 + multica_settings） ② client.rs 的 browser_login(localhost callback)/create_token/verify_pat/list_workspaces ③ channel config 5 处改动 ④ get/save/connect_multica 命令 | ✅ 已完成：`cargo check` 通过 + 10 单测固化（map_status 码表/退避序列/URL 归一化/daemon_id 幂等/PAT 不回显/错误码 kebab 前缀/params 无对客文案）；`browser_login` 复刻 multica-main `cmd_auth.go:240-358`（IPv4 listener + CSRF state + JWT→PAT→verify）；首启登录链路待 M5 前端联调 |
| **M2** ✅ | 注册/心跳 | ① loop_.rs `start_multica_loop`（verify_pat→全量 register→recover-orphans） ② 执行期 15s 心跳循环 ③ register/heartbeat/recover client 方法 ④ 启动挂载（main.rs `start_heartbeat_polling` 后） | ✅ 已完成：`cargo check` 通过 + 15 单测全过（M2 新增 5：register body 契约/runtime_id 取回/心跳 body 形状/runtime_id 映射幂等/active runtime 去重）；loop_ 复刻 `metrics::start_heartbeat_polling`（三层 guard + spawn + 每 tick 重读配置）；PAT 无效静默跳过（首启不弹登录）；心跳为执行期（`active_runs` 驱动，M2 空转待 M4）；`SharedMulticaState` 作为独立 tauri managed state（loop/bridge 共享，不进 DesktopState） |
| **M3** ✅ | 列表/领取 | ① list_pending/claim_specific/start/get_status client 方法 ② `vm.rs`（`RemoteTaskVm`/`RemoteConversationSidebarVm`，对齐 `ConversationSidebarVm` 键名） ③ `get_multica_tasks`/`claim_multica_task` 命令 ④ `pending_issues` 生命周期澄清 | ✅ 已完成：`cargo test` 通过 + **25 单测全过**（M3 新增 10：client list/claim/start wire 契约 5 + vm 状态归一/camelCase 序列化/from_pending/from_failed_issue/sidebar 键名 5）；`get_multica_tasks` 按 workspace 分组拉 `list_pending` + `pending_issues` 失败回显进 `pinned_tasks`（未连接返回空状态 connected=false）；`claim_multica_task(task_id, workspace_id)` 命中 `task_conversations.session_id` 带 `prior_session_id` 续跑；**`pending_issues` 改为「失败待重试」语义**（M4 fail 写入、complete/rerun 清除，claim 不写——修复原设计「claim 写入」会导致 running 任务误显为 retryable、且 complete 不清理的生命周期缺陷，见 §2.2.3/§4.3）；`auth_token` 永不入 VM |
| **M4** ✅ | 执行/恢复 | ① bridge.rs（subscribe_named + 会话事件转译为基础状态） ② register_lifecycle_subscribers 加一行 ③ start/cancel_multica_remote_task 命令 ④ ACP session_id 采集（worker-ref.json）+ pin_task_session + multica_task_conversations 索引 ⑤ complete/fail 重试幂等 ⑥ rerun_multica_task 命令 ⑦ 断点续跑（prior_session_id + strict_continue 降级）+ 本地会话作废逻辑（4.4） | **✅ M4-a/b/c/d 全过** —— M4-a client 终态层（`complete_task`/`fail_task` `with_terminal_retry` + `pin_task_session` + `rerun_issue`[X-Workspace-ID] + wire 契约，5 单测）；M4-b `bridge.rs` 订阅器（`create_multica_subscriber` 注册 `desktop.multica`）：`NodeCompleted`→读 worker-ref 采 session→`pin_task_session`+落 `task_conversations`（session 变更才写）、`RunCompleted`→4 分支（Success→complete / Failure→fail+记 pending_issues / Killed→不上报 / Paused/Intervention→不上报）→`spawn` 异步 HTTP；归属靠 `active_runs` 反查 (local_task_id,local_run_id)（RunCompleted 无 repo_root）；M4-c **库层 preset 上提 + start/cancel 命令**：`gold_band::dsl::presets::direct_workflow`（单 raw-agent Worker→`$end`，会话 VM `build_direct_workflow` 改为委托，**仅上提 direct**——`auto_workflow` 核心是 VM 特有 `AiDynamicAgentStrategy` 翻译、无第二消费方，上提只是搬迁不构成复用，保留 VM）；`binding_for_multica(RuntimeConfig, StateConfig, ws_id)→(workspace_path, provider)` 复用 `workspace_entry_for_project`；`start_multica_remote_task`：claim→`binding_for_multica`→**`state.app().with_repo_root(workspace_path, config)`**（共享 `DesktopState.lifecycle_bus`，`desktop.multica` 订阅器据此收 NodeCompleted/RunCompleted——**关键**：`App::with_config` 自带空 bus，必须经 `state.app()` 注入共享 bus）→`create_task_from_requirement`(requirement 作首轮 prompt)+`run_start_background`→登记 `active_runs`(真实 run.id，先于事件归属)+落 `task_conversations`(session_id=None 待 bridge 回填)→`start_task(false)`；库层 sync App 调用经 `spawn_blocking`，本地启动失败 best-effort `fail_task` 免 server 干等；`auth_token` 本期不注入（agent 用绑定 provider，multica 回传全走 daemon PAT）；`cancel_multica_task`：反查 `active_run(remote)`→`run_pause(ProcessInterrupted)`+杀 ACP→清 `active_runs`+`task_conversations`（cancelled 不续跑）；**42 multica 测全过**（M4-c 新增 4：state `active_run` 反查 + `binding_for_multica` 命中/未绑定/本地缺失 3）+ preset 2 单测 + 会话 VM 委托回归 1。**✅ M4-d 完成（断点续跑 + rerun + 远程 fail 本地作废）**——① **断点续跑**：**未另立 `resume_multica_task` 命令**，`start_multica_remote_task` 内 `classify_resume(Option<&MulticaTaskConversation>)` 自动分支（命中先前未终态 local run→`Resume` 走 `run_continue_background_with_config_overrides(local_task_id, local_run_id, None,None,[],None,None)`[内部 worker-ref continue_ref→session/load]，否则 `Fresh`）；**strict_continue 失败（session 死，acp/client.rs:2132-2141）或任何 resume Err → fresh fallback**（`start_fresh` 新建 task+run，`start_task(force_fresh_session=true)`）；放弃原方案 `multica.session-resume-failed` 字符串匹配（错误码变体保留在 error.rs 码表但本路径不 emit——「任何 resume 失败=不可续」更稳，无需 fragile 串匹配）。② **`rerun_multica_task(issue_id, workspace_id)`**：`client.rerun_issue`(M4-a `post_json_with_workspace`) + 清 `pending_issues[issue]`(`retain`)。③ **Killed 分支**：`TerminalAction::NoReport`→`Fail{reason:"timeout"}`（删 NoReport/PendingUpdate::None 死分支）；cancel 路径皆经 `run_pause→Paused` 从不产生 Killed，故 Killed 必为 agent 真死，**unconditional fail(timeout)**（无需 cancel-detection 上下文消歧，避免补丁式修复）。④ **远程 fail 本地作废**：抽 `bridge::teardown_active_run(workspace_app, shared, home_app, remote, local_tid, local_rid)`（run_pause+杀 ACP+清 active_runs+清 task_conversations[remote]）供 cancel 命令 / C2 / C3 三处共用；**C2 启动 reconcile**(`reconcile_startup_orphans`，recover-orphans 后)：task_conversations 条目 remote cancelled/404→作废，**不在 failed 作废**（保 retryable 断点续跑，terminal-failed 本地 Paused run 由 strict_continue fallback 兜底）；**C3 周期 cancel-detection**(`detect_cancelled_active_runs`，心跳同 tick)：active_run remote failed/cancelled/404→作废（无 resume 冲突）；`invalidate_remote_task` active_run→binding_for_multica 优先，回退 task_conversations[remote].work_dir（崩溃 orphan）。**47 multica 单测全过**（M4-d 新增 5：classify_resume 3 + is_active/is_orphan_terminal 2） |
| **M5** ✅ | 前端 | ① API 四层 + types ② 设置页 multica 区块 ③ ConversationSidebar 远程任务列表切换 ④ 事件监听 ⑤ i18n 双语 | **✅ M5-a/b/c/d/e/f 全过** —— M5-a API 四层（client.ts/desktop.ts/browser.ts/api.ts）新增 13 + 1 个 multica 方法（subscribeMulticaTaskUpdates 为 M5-b 事件监听新增）含 `startMulticaRemoteTask` workspace_id 参数修复（原签名缺失该参数）；types.ts 新增 6 个 VM 类型（`MulticaSettingsVm`/`MulticaServerWorkspaceVm`/`MulticaWorkspaceRefVm`/`RemoteTaskVm`/`RemoteConversationSidebarVm`/`MulticaSettingsVm`）。M5-b 后端事件 `gold-band://multica-task-updated`（unit payload）经 bridge.rs 终端态 + loop_.rs 作废路径 emit。M5-c workspace CRUD 5 命令（`list_server_multica_workspaces`/`add_multica_workspace`/`rebind_multica_workspace`/`remove_multica_workspace`/`set_active_multica_workspace`），复用 `project_id_for_workspace`+`project_ids_match` 去重，slug=id 兜底。M5-d i18n 双语 6 组 key（`settings.multica.*` zh-CN/en 各 22 key + `conversation.sidebar.multica.*` 各 8 key + `errors.multica.*` 各 12 key，覆盖全部错误码 + workspace-already-bound/workspace-not-found）。M5-e `MulticaSettingsBlock` 自管理组件（barrel API 直连，provider 常量选项 claude-acp/codex-acp，连接/保存/workspace CRUD 全 inline，enable toggle 复用 ui/switch），SettingsPage 以 `<SettingsSection><MulticaSettingsBlock /></SettingsSection>` 嵌入 advanced tab。M5-f `MulticaRemoteTaskList` 自管理组件（getMulticaTasks + subscribeMulticaTaskUpdates 事件 → refetch，claim+start→navigate，cancel/rerun→barrel API，not connected 空态卡片），ConversationSidebar 新增 本地/远程 segmented toggle（button cva compose）→ 按 remoteView 条件渲染 ScrollArea；**49 multica 后端单测全过**，tsc 零 multica 新增错误。**M5-g/h/i（后续完善）**：M5-g 登录落点改 302→multica web 根（`callback_redirect_response` 纯函数，不渲染登录结果页）；M5-h 添加工作空间弹窗（远程工作空间 + provider 下拉 + `pickLocalDirectory` 共享原语 + 添加）+ 设置页破坏式移除「添加工作空间」行（仅留已绑定管理）；**M5-i 断开连接**：`disconnect_multica`（sync，对称 `connect_multica`）——纯函数 `clear_multica_session` 清 PAT（`connected` 判定依据）保留 daemon_id/workspaces，`MulticaRuntimeState::clear_runtime_ids` 清 register 缓存（active_runs 保留），connect/disconnect 均 emit `multica-task-updated` 让会话侧栏 re-fetch 同步；设置页 multica 区块【断开连接】按钮（仅已连接态，连接按钮文案切「重新连接」）。**52 multica 后端单测全过**（+clear_multica_session/clear_runtime_ids 各 1），tsc 零错误，816/817 前端用例过（1 既有 scrollbar CSS 失败与 multica 无关）。**M5-j 添加工作空间弹窗布局修复**：`MulticaAddWorkspaceDialog` 原用 shadcn `DialogContent` 默认 `grid` 无高度上限，长路径/多字段下 footer（【添加】）被固定居中布局顶出视口、路径文本横向溢出。改为 `flex flex-col max-h-[85vh] overflow-hidden`（twMerge 干净覆盖 `grid`）+ header/footer `shrink-0` + 中段 `min-h-0 flex-1 overflow-y-auto`，路径行按钮 `shrink-0` 配 `min-w-0 flex-1 truncate` span——footer 始终可见、路径省略号截断。同时补空状态自诊断：数据链路已验证正确（`GET /api/workspaces` 裸数组 `[{id,name,...}]` 与 `WorkspaceInfo{id,name}` 字段匹配，反序列化不报错；空列表即 server 返 `[]`），下拉为空时按 `serverWorkspaces.length===0`（去 multica Web 创建）vs `available.length===0`（全部已绑定）两态提示，不再静默空列表（i18n `noServerWorkspaces`/`allWorkspacesBound` 双语）。**M5-k 绑定后任务列表/设置页不显示 + 即时可用**（三缺陷同修）：① **渲染 bug**——`MulticaRemoteTaskList` 原 `hasAnyTasks` 总开关 + 组内 `if(!tasks.length)return null` 把「已绑定但当前无任务/未 register」的 workspace 整组隐藏，回退到「暂无会话」；改为始终按 `vm.workspaces` 成组展示，空组内显组内空状态提示（i18n `noTasksInWorkspace`，M5-ab 起居中带垂直留白），仅「无任何绑定」才 `noWorkspacesBound`。② **设置页不刷新（非数据 bug）**——`get_multica_settings` 与 `get_multica_tasks` 读同一份 RuntimeConfig，下拉能过滤已绑定 workspace 即证数据已落；设置页「显示空」纯因只 mount 时 fetch 一次、不订阅事件。新增 `gold-band://multica-settings-updated` 事件（语义=连接/workspace 配置变更，区别于任务生命周期的 `multica-task-updated`），connect/disconnect/save/add/rebind/remove/set_active 统一 emit（connect/disconnect 从 task-updated 迁移至此），任务列表（订阅 task+settings 两事件）+ 设置页（订阅 settings）任一处改动两端同步 re-fetch。③ **register-on-add**——register 原仅启动全量跑一次，`add_multica_workspace` 只落配置不 register → 绑定后须重启才有 runtime_id、任务拉不到/不能 claim；改为 `add_multica_workspace` 改 async，绑定后即时 `register_workspace_best_effort`（复刻 loop 单 workspace 注册，取回 runtime_id 缓存 SharedMulticaState，失败非致命启动 loop 兜底）实现「绑定即可用」。workspace CRUD 命令签名加 `app_handle: AppHandle`（+ add 加 `shared`）供 emit/register。验证：cargo check 过 + 52 multica 单测全过，tsc 零错误，新增 `multica-remote-task-list.test.tsx` 3 用例（空任务显 workspace 组 / 未连接显连接入口 / 订阅 task+settings 两事件）+ 既有 dialog 2 用例全过（共 5）。**M5-l 登录账号可见性 + 切换账号逃生口**（浏览器 cookie 账号歧义，码灵侧 Layer 1）：码灵认证委托给浏览器、cookie 不受控，若浏览器已登账号 B 而用户想登 A，webank 见 cookie 即签 B 的 JWT，码灵静默连错；原 `connect_multica` 更丢弃 `UserInfo{email}` 且无账号字段，连错也看不出。根因（Layer 2，webank `/login` 带 cli_callback 时显式 OAuth consent 屏）属独立 multica-webank 仓库本轮不动；码灵侧做 Layer 1：① 新增 `MulticaAccountRef{name,email}`（与 PAT 同生命周期单结构体），`connect_multica` 捕获 UserInfo 落盘 `desktop_multica_account`、`MulticaSettingsVm` 暴露 `connectedAccount`（仅展示非凭证）、`clear_multica_session` 对称清；设置页连接时显「已连接账号：{email}」。②【切换账号】按钮复用 `openExternalUrl(appUrl)` 打开 multica Web（浏览器登出/换号后回此重连，诚实标注码灵无法强制登出）。验证：cargo check + lib config 35 测（含 account roundtrip）+ desktop multica 52 测（含 clear 清 account）全过，tsc 零错误，新增 `multica-settings-block.test.tsx` 3 用例（连接显账号+切换按钮 / 未连接不显 / 点切换按钮 openExternalUrl(appUrl)），i18n 双语补 `connectedAccount`/`switchAccountHint`|

> 每个里程碑完成后按 CLAUDE.md「实现并验证无误后必须进行单元测试，从接口层面固化验收」补单测。

> **（Req D 演示反馈调整，已完成并验证）** 同事演示完整流程后提两点调整，均已落地：
> 1. **心跳常驻**：M2 心跳由「执行期（`active_runs` 驱动）」改为「常驻（`runtime_ids` 驱动 = 全部已连接 workspace）」——`state.all_runtime_ids()` 新增；`collect_active_runtime_ids`/`active_runtime_ids` 保留给 cancel 检测。同循环新增 `extend_prepare_leases` 续期 claim 后未 start 的任务 lease（45s）。
> 2. **执行流程改造**：删原子 `start_multica_remote_task` + `MulticaRemoteTaskStartedVm`；claim 不再立即 start，改 claim-at-click（回填 `RemoteTaskVm.requirement` + 登记 `prepare_leases`）→ 前端预填 composer（`composerDraft.prefill`）+ `onNewConversationInWorkspace`（与本地『+』同一回调）→ 用户选模型/模式 → 发送经 **`start_multica_conversation_run`（复用 `create_conversation_run_vm`）**；新增 `cancel_multica_prepare_lease`（放弃 compose）。
> **验证**：`cargo test -p gold-band-desktop multica::` 60 全过；web vitest（conversation-composer-draft + multica-remote-task-list）全过 + tsc 零错误。**无需 webank server 改动**（码灵 client 补字段即可解析 claim 响应里已有的来源字段；续期路由 server 既有）。待补：agent-browser 端到端验证（#92）。

---

## 9. 测试策略

### 9.1 server 侧（M0）
- **selective claim**：queued+同 runtime+无并发→200 / 串行化冲突→404 / 非 queued→404 / 跨 runtime→404 / 并发同 task 恰一成功（SQL 原子性）。
- **登录**：无 server 改动，不测 server 侧（浏览器登录链路在 9.2 client.rs 测）。

### 9.2 客户端侧（M1–M4）
- **client.rs**：重试退避（终态 6 次 4/8/16/32/64s）、错误码映射（401→auth-failed / 404→task-not-found / 409→claim-conflict）、browser_login（localhost callback 收 JWT）—— mock HTTP server。
- **config.rs**：pat_set 永不回显明文 / daemon_id 首次生成并落盘 / 旧配置 Option 兼容。
- **bridge.rs**：会话事件 → multica 基础状态转译；**run 终态 4 分支穷举**（Success→complete / Failure→fail+记 pending_issues / Killed→不上报假设取消检测已命中 multica cancelled / Paused·Intervention→不上报本地处理）；ACP session_id 采集（`NodeCompleted.attempt_dir/worker-ref.json` 的 `continue_ref.{acpSessionId,cwd}`，复用库层 `WorkerRefState`）+ `pin_task_session`（session 变更才写）；remote fail → 本地会话作废（4.4）；**多 workspace 并发事件归属反查**（`active_runs` 反查 (local_task_id, local_run_id) → remote_task_id；`RunCompleted` 事件**无 repo_root**，仅 `Node*` 有，故归属键是本地 display task_id+run_id 双键，非 repo_root）；HTTP 调用经 `tauri::async_runtime::spawn` 异步执行（订阅器回调在 runtime 热路径）。
- **ACP session 跨重启可恢复（M1 首验）**：mock requirement 跑完采 session_id → 重启进程 → run_continue_background_with_config_overrides 带 prior_session_id → 验证 session/load 续跑成功、上下文连续；session 已死 → 降级 force_fresh_session 整任务重跑。
- **断点续跑**：claim 子任务 T' 按 parent_task_id 反查父索引命中→沿用父 local ids 走 strict_continue 续跑；session 已死→降级 force_fresh_session 整任务重跑；续跑成功后迁移索引到子任务 id（§12.14）；`multica_task_conversations` complete 后清条目。

### 9.3 接口层固化（回归用）
- 首启登录链路、启动全量 register、任务执行循环、失败恢复链路各一条端到端集成测试（mock multica server）。

---

## 10. 文件变更全量清单

### 10.1 multica server（M0）
| 文件 | 变更 |
|---|---|
| `server/pkg/db/queries/agent.sql` | + ClaimSpecificQueuedTask |
| `server/internal/service/task.go` | + ClaimSpecificTask |
| `server/internal/handler/daemon.go` | + ClaimSpecificTask |
| `server/cmd/server/router.go` | + 1 路由（selective claim） |
| `server/pkg/db/generated/*` | `make sqlc` 重新生成 |

### 10.2 码灵客户端（M1–M4）
| 文件 | 变更 |
|---|---|
| `src-tauri/src/multica/mod.rs` | 新建（`pub mod client/commands/config/error/loop_/state/vm;` + M4 `bridge`） |
| `src-tauri/src/multica/{client,config,state,loop_,bridge,error,vm,commands}.rs` | 新建 8 文件（M1 `mod`/`client`/`config`/`error`；M2 `state`/`loop_`；M3 `vm`/`commands`；M4 `bridge`） |
| `src-tauri/Cargo.toml` | + tokio `net` feature（`browser_login` 的 `TcpListener`）+ `thiserror = "1"`（`MulticaError`） |
| `src-tauri/src/main.rs:11` | + `mod multica;` |
| `src-tauri/src/main.rs:185` 后 | + `multica::loop_::start_multica_loop(app.handle().clone())` |
| `src-tauri/src/main.rs:188-326` | invoke_handler 加 12 multica 命令 |
| `src-tauri/src/commands.rs:481-497` | `register_lifecycle_subscribers` 加 `subscribe_named("desktop.multica", ...)` |
| `src-tauri/src/commands.rs` | + 12 `#[tauri::command]`（参考 1536-1583 metrics 命令对） |
| `src/config/mod.rs:505-530` | SettingsConfig + 8 字段（Option，含 app_url） |
| `src/config/mod.rs:666-689` | StateConfig + 3 字段（含 multica_task_conversations） |
| `src/config/mod.rs:993-1032` | RuntimeConfig + 8 字段（非 Option，含 app_url） |
| `src/config/mod.rs:1034-1080` | RuntimeConfig::default 补默认 |
| `src/config/mod.rs:1083-1121` | apply_settings + multica 块 |
| `src-tauri/src/channel.rs:4-22` | DesktopChannelConfig + 4 字段（含 multicaAppUrl） |
| `src-tauri/src/channel.rs:24-52` | current_channel_config + 4 option_env! |
| `src-tauri/build.rs:5-27` | ChannelConfig struct + cargo:rustc-env + rerun-if-env-changed |
| `configs/channels/default.json` | + multicaBaseUrl/multicaAppUrl/multicaEnabled/multicaToggleLocked（预填 `localhost:8080`/`3000` + `enabled=true`，零配置直连，见 §2.2.4） |
| `configs/channels/wb.json` | + 同字段（预填） |
| `src-tauri/src/conversation_workspace.rs` | **复用（不改）**：`workspace_entry_for_project`:24 供 multica 按任务级 `local_project_id` 反查 workspace_path（M5-z：绑定模型下沉到任务级，workspace 注册不再写本地目录，运行时按任务级 projectId 反查）；`add_conversation_workspace` 仍服务于本地工作空间的常规添加流程（与 multica 注册解耦） |

### 10.3 前端（M5）
| 文件 | 变更 |
|---|---|
| `web/src/api/client.ts:126` | RuntimeApi + 12 方法签名 |
| `web/src/api/desktop.ts:248-253` | desktopApi + 实现 |
| `web/src/api/browser.ts` | + mock |
| `web/src/api.ts:300-302` | + barrel wrapper |
| `web/src/types.ts:58` | + MulticaSettingsVm / MulticaWorkspaceRef / RemoteTaskVm |
| `web/src/pages/SettingsPage.tsx:487-523` | + multica SettingsSection（复制 metrics 结构；M5-z：去 rebind / folder picker，添加 workspace 只收远程 + provider） |
| `web/src/components/conversation/ConversationSidebar.tsx` | 纯本地工作空间列表（M5-z：删本地/远程 toggle，远程任务搬至独立整页） |
| `web/src/pages/conversation/MulticaTaskManagementPage.tsx` | **新建（M5-z）**：远程任务管理独立整页，路由 `/chat/multica-tasks`，承载原侧栏远程任务列表（装配逻辑沿用 §12.8/§12.9） |
| `web/src/components/multica/MulticaAddWorkspaceDialog.tsx` | **M5-z 简化**：去 folder picker / pickLocalDirectory，只收远程 workspace + provider |
| `web/src/components/conversation/ConversationComposer.tsx` | **M5-z 增强**：multica binding 激活时强制显示本地工作区下拉（即便 1 个），0 本地工作区给引导并禁用发送 |
| `web/src/i18n.ts` | + multica.taskManagement.* / multicaNeedLocalWorkspace 双语；M5-z 删 localTab/remoteTab/dialog 目录键/settings.multica rebind 系列 |

---

## 11. 风险与开放问题

| # | 风险/问题 | 应对 |
|---|---|---|
| 1 | **浏览器登录依赖 multica Web 可达**：localhost callback 要求 `<MULTICA_APP_URL>` 浏览器可访问；Web 不可达则无法登录 | 确保企业内 multica Web 可达；登录失败给明确网络提示。复用原生邮箱登录，**无新增 server 信任通道**（无内网冒充暴露面） |
| 2 | **selective claim rebase 成本**：与 ClaimTaskByRuntime 收尾链强相关 | handler 顶部注释标注复制源 commit；升级时 diff 该锚点（见 1.3） |
| 3 | **两套心跳并存**：metrics 15min + multica 3s | 独立循环，互不干扰；**multica 心跳 Req D 起改为常驻**（建立连接后对全部已连接 workspace 的 runtime 持续，非仅执行期；M5-cx 由 15s 提频，工单完成后另有补发排空）；同一常驻循环还承载 claim-at-click 后的 prepare-lease 续期 |
| 4 | **PAT 明文存储** | 项目无 keyring，与 metrics API Key 一致；永不回显明文（pat_set）；换机器/账号强制重连不复用 PAT |
| 5 | **multica 版本升级** | M0 唯一改造（selective claim）为独立增量，rebase 成本可控（见 1.3）；登录无 server 改动零 rebase |
| 6 | **接受 multica auto-retry + 断点续跑** | resume-safe 失败（runtime_recovery 等）server 自动重试 max_attempts=2，克隆新 id 子任务 T'；客户端 claim T' 时按响应 parent_task_id 反查父 T 本地索引续跑（§12.14）；resume-unsafe（agent_error）用户 rerun；session 已死降级整任务重跑（4.4） |
| 7 | **终态重试是新增逻辑** | metrics 为 fire-and-forget，multica 终态必须送达 → client.rs 自建退避重试（2.3），是本模块相对 metrics 的主要新增 |
| 8 | **multica fail 与本地会话 Paused 状态张力** | remote_task fail 后本地会话作废不 continue；断点续跑走 parent_task_id 反查父索引（§12.14）而非本地 Paused continue（4.4 厘清） |
| 9 | **ACP session 跨重启可恢复性未验证（恢复断崖）** | 「多轮=task序列+session续跑」依赖 ACP provider（claude-acp）真持久化 session、能 `session/load`。未验证。**M1/Step 0 首验**：mock requirement 跑完采 session_id → 重启 → 领取子任务 T' → run_continue_background 沿用父 local ids（内部读 worker-ref continue_ref）→ 确认 session/load 成功、上下文连续。若 claude-acp 不持久化，多轮降级为每轮新会话（功能降级，非阻塞） |
| 10 | **run 终态 4 分支 + Paused 盲区**（本期补齐） | 订阅器必须穷举 Success/Failure/Killed/Paused（2.5 终态表），不能只 match success；Paused 期间 multica 继续显示 running（接受盲区）；不监听未实现的 AcpTurnFinished |

---

## 12. 联调修复（M5-r，2026-08）

### 12.1 Fix 1：心跳自愈注册

**现象**：码灵运行时心跳似乎没有保持连接。connect 后首心跳空转（没有 runtime_id）；心跳 404 后 runtime_id 被清但未补注册 → 永不长连。

**根因**（设计缺陷）：
- 注册原为「一次性」：启动全量跑一次 + workspace 绑定时注册。`connect_multica` 不注册；心跳循环不补注册。
- 心跳 404（server 端 runtime 行被删或不对应）只打 log 不清本地缓存 → 一直发错误的 heartbeat。

**修复**（`loop_.rs` + `state.rs` + `commands.rs`）：

| 文件 | 变更 |
|---|---|
| `state.rs` | `runtime_id_pairs() -> Vec<(String, String)>` 返回 workspace→runtime 映射供自愈；`clear_runtime_id(workspace_id)` 单 key 清除 |
| `loop_.rs` | 抽取 `register_workspace(retried)` 共享 helper（构造 RegisterRequest + 按 `retried` 选 `client.register`/`register_once` + 缓存 runtime_id，见 §12.13）；`run_heartbeat_loop` 每 tick 先 `self_heal_registration`（`retried=false`，循环即重试）补注册缺失 runtime_id；心跳 404 → `clear_runtime_id` → 下一 tick 自愈 |
| `commands.rs` | `connect_multica` async 改为 await `register_all_bound_workspaces`（即时注册所有已绑定 workspace）|

**测试**：`runtime_id_pairs_carries_workspace_for_self_heal` + `clear_runtime_id_singular_drops_one_keeps_rest`。

### 12.2 Fix 2：start_task 失败回传 fail + 清孤儿 run

**现象**：local start 成功但远端 `POST /tasks/{id}/start` 失败时，本地有 `active_run` 映射但远端任务还在 queued/pending，后续 `complete_task` 也失败（远端非 running）→ 任务永远 pending。

**根因**（实现不完善）：
- `start_multica_conversation_run` 的 Ok 分支原 `.map_err(|e| command_error(e.into()))?` 直接上抛 → 本地 run 已创建（`create_conversation_run_vm` 成功）但远端 start 失败时未清理 → 孤儿 run。
- 第一层原因（prepare-lease 过期被回收）由 Fix 1 根治（自愈 + 续期）。

**修复**（`commands.rs`）：
```rust
if let Err(start_err) = client.start_task(&remote_task_id, false).await {
    let _ = client.fail_task(&remote_task_id, "local start failed", "agent_error").await;
    // teardown: pause local run + cancel ACP + drop active_run
    crate::multica::bridge::teardown_active_run(...);
    emit_multica_task_updated(&app_handle);
    return Err(command_error(start_err.into()));
}
```

### 12.3 取消「显式完成远程任务」按钮（完成语义回归生命周期驱动）

**背景**：原 §12.3 设想非 direct 模式暂停后由用户点按钮显式完成远端任务。评审后决定**不加按钮**：完成动作应跟随 run 生命周期自然发生，不引入额外的手动终态入口。

**完成语义（删除按钮后，沿用既有 `bridge` 终态分支，无新代码）**：

| 模式 | 完成路径 |
|---|---|
| direct | agent 跑完 -> `RunCompleted{Success}` -> `handle_run_completed` -> `classify_terminal(Success)` -> `TerminalAction::Complete` -> `complete_task` + `finalize_terminal`（**M5-bp / §12.55 起对 issue 关联任务改为 completion-output 块门控**：无块扣留完成，见 §12.55） |
| workflow | workflow run 跑到终态（自然结束 / 节点序列走完）-> 同上 `RunCompleted{Success}` -> `complete_task`。「按终止来完成」即完成由 run 终止事件驱动，不另设手动入口（workflow graph 终态即 definition of done，**不受 §12.55 门控影响**） |

**已知缺口（已接受）**：open-ended workflow 在 interview/round 节点 `RunPaused`（等人工判定）时 `RunCompleted` 不 fire -> 远端保持 running；无手动完成入口，由 server sweeper（running-stuck 2.5h）兜底转 failed。agent-driven 模型下「暂停等人工」的预期代价，暂不补。

**删除清单**：`bridge.rs` `complete_remote_task_explicit`；`commands.rs` `complete_multica_task` + `get_multica_active_run`；`vm.rs` `MulticaActiveRunVm`；前端四层 `completeMulticaTask`/`getMulticaActiveRun` + `ConversationRunHeader`/`ConversationRunPage` `onCompleteMulticaTask` + `App.tsx` active-run useEffect + `types.ts` `MulticaActiveRunVm` + `i18n.ts` `completeRemoteTask`。

### 12.4 完成远程任务后流转 issue 到 done（接入方案 D2）

**背景**：码灵完成远程任务（`complete_task` 送达）后，关联的 multica issue 需自动推进到 `done`，避免 issue 长期停留在「任务已跑完但 issue 未关闭」的不一致状态。

**决策（选项 B：码灵作为中介，而非 A：agent 直调）**：

| 选项 | 机制 | 取舍 |
|---|---|---|
| A（弃） | 给 agent 注入 task-scoped `MULTICA_TOKEN`，agent 跑完自行调 issue 状态 API | 违反「ACP agent 从不直接调 multica API」既有约束；agent 需感知 multica 业务接口，职责越界 |
| **B（选）** | **码灵**用自身 PAT（`mul_` 用户级）在 `complete_task` 送达后调 issue 状态 API | 码灵作为 daemon 已持 PAT，天然适合做 issue 状态推进中介；agent 只管执行 |

**数据（先定数据）**：
- `client.rs` wire type `UpdateIssueStatusRequest { status }`（`PUT /api/issues/{id}` body）。
- 常量 `MULTICA_ISSUE_DONE_STATUS: &str = "done"`（完成态字面量集中管理，杜绝硬编码）。

**接口（再定接口）**：
- `MulticaClient::update_issue_status(&self, workspace_id, issue_id, status) -> Result<(), MulticaError>`
  - `PUT /api/issues/{issue_id}`，body `{status}`，带 `X-Workspace-ID` 头（issue 维度路由，开发设计 4.1）。
  - 走 `with_network_retry`（3 次）；body 丢弃（只校验 HTTP 状态，不解码响应）。

**实现（最后补实现）**：bridge `handle_run_completed` 的 `TerminalAction::Complete` 分支改造：
- 原：`complete_task` 失败 `warn`，恒返回 `ClearOnSuccess`。
- 现：`complete_task` **Ok** 后，若 `run.issue_id` 非空（`trim` 非空白）→ `update_issue_status(workspace_id, issue, "done")`；失败 `warn` 但不阻断（终态已上报）。`complete_task` **Err** → 仅 `warn`，**不流转 issue**（server 未收到 complete，issue 不应变 done）。
- issue_id 缺失（如非 issue 来源任务）跳过——并非所有远程任务都有关联 issue。

**重构（消除第三份重复）**：issue PUT 拉入第三种 JSON 请求方法，把 `post_json` / `post_json_with_workspace` 的「auth + send + map_status」模板上提为 `json_send(method, path, workspace: Option, body) -> Response`，三调用方（POST 无头 / POST 带头 / PUT 带头）共用。行为保持不变。

**验证**：`cargo check --workspace` 通过；`cargo test -p gold-band-desktop multica::` **65 测全过**（新增 3：常量 / body / path）。无需 webank server 改动。

---

### 12.5 断点续跑落地（远程任务重派后续既有本地 run）

> ⚠️ **本节记录 M5-u 初版落地**，其 `classify_resume` 当时按**字面 remote_task_id 查本地索引**——与服务端「auto-retry 克隆新 id 子任务」错配，导致崩溃重启主场景下续跑空转（落 Fresh）。该实现缺口由 **§12.14（改动十二）补完**：客户端消费 claim 响应的 `parent_task_id` 反查父任务本地索引 + 续跑后迁移索引。下面「机制前提」仍成立，签名/判定以 §12.14 为准。

**背景**：远程任务被 server 重派（码灵崩溃恢复 / agent 死亡被回收 / 失败自动重试 max_attempts=2）后，用户重新点「执行」→ `start_multica_conversation_run` 此前**总是 Fresh**（注释明说「断点续跑…不在此分支」），每次从第 1 轮重跑，丢失已完成的 round/node/部分产出。

**根因（非补丁，补完消费者而非打补丁）**：断点续跑的**数据结构已就位、消费者被删**。
- `multica_task_conversations[remote_task_id]`（{local_task_id, local_run_id, session_id, work_dir}）在 Fresh 成功时写入、bridge 在 NodeCompleted 回填 session_id。
- 但消费它的 `classify_resume`（旧 `start_multica_remote_task` 里）在改 composer 流时一并删除——再无人用 checkpoint 把本地 run 续起来。
- 附带不一致：`claim_multica_task` 只要 session_id 非空就传 `prior_session_id`，可紧接的 start 却 Fresh 建新 session（claim 说续 X、start 用 Y）。
→ 改法：补完消费者 `classify_resume`（claim/start 共用），start 里 Resume/Fresh/回退三分支，并把 claim 的 prior_session_id 接到同一判定。

**机制前提（已带 file:line 确认）**：
1. ACP session 跨码灵完整重启可冷重连（adapter 自持 session 落盘，`session/load`）。
2. 本地续跑链：`run_continue_background(task_id, run_id, prompt_id, prompt)`（app/mod.rs:2928）→ orchestrator 读 `<attempt_dir>/worker-ref.json` `continue_ref.acpSessionId` → `SessionMode::Continue`。
3. `is_run_continuable(run)`（app/mod.rs:711）= Paused + outcome None + pause_reason ∈ {ProcessInterrupted, RuntimeAbnormal, WaitingForUserInput} + round/node/attempt 齐。
4. 启动自愈：`main.rs` setup → `recover_interrupted_running_sessions` → `pause_all_running_sessions`（app/mod.rs:2370）把 stale-Running 翻成 Paused + ProcessInterrupted + outcome None → 崩溃重启后旧 run 自动满足 is_run_continuable。**原限制：仅覆盖 home repo（激活 workspace）**——multica 远程任务的 run 落在各 task 自身 `work_dir`（独立 repo），home 自愈够不到；**§12.15（改动十三）补全**：home 自愈后追加 `recover_multica_work_dir_sessions`，遍历 `multica_task_conversations` 的全部 work_dir 逐个自愈（**§12.37（M5-ax）重写为 checkpoint 定点自愈并移入 spawn_blocking 启动管线**）。
5. `start_task(task_id, force_fresh_session)`（client.rs:592）：续跑与 Fresh 都传 false（force_fresh=true 仅整任务重跑）。

**数据（先定数据）**：无新结构，复用 `MulticaTaskConversation`。新增：
- `enum ResumeDecision { Resume{local_task_id, local_run_id, session_id}, Fresh }`。
- 纯函数 `classify_resume_from(conv: Option<&MulticaTaskConversation>, run: Option<&RunState>) -> ResumeDecision`（无 I/O，可单测）。
- I/O 包装 `classify_resume(home_app: &App, remote_task_id: &str) -> ResumeDecision`（读 checkpoint → 由 work_dir 构造 workspace App → run_status → 委托纯函数）。

**接口（再定接口）**：
- `claim_multica_task`：`prior_session_id` 改为 `match classify_resume(&context.app(), &task_id) { Resume{session_id,..}=>Some, Fresh=>None }`（消除 claim/start 不一致）。
- `start_multica_conversation_run`：校验通过后、建 run 前插三分支——
  - **Resume**：`app.clone_for_background().run_continue_background(prior_task, prior_run, None, None)`（纯续跑）→ `register_active_run`(既有 ids) + `start_task(false)` + drop lease + `conversation_run_vm(既有 run)` 还原 VM 返回（导航既有会话）。Err → warn 落 Fresh。
  - **Fresh**：现有 `create_conversation_run_vm` 链；**顺带修旧漏**——覆盖既有 checkpoint 时 `entry.session_id = None`（旧 session 随旧 run 失效；新 run 的 session_id 待 bridge 回填）。

**决策（D1/D2）**：

| 决策 | 选项 | 取舍 |
|---|---|---|
| D1 续跑文本 | **纯续跑 prompt=None（选）** | 续跑=接着被中断的编排往下跑（冷重连 session、续当前 attempt），不新增 turn、不换模型；重发整段需求当新 turn 会重复上下文、干扰半截工具调用。composer 预填文本仅作「任务是什么」展示 |
| | composer 文本当新 turn（弃） | 更「像发送」，但对中断半截 run 不自然；未改文本会原样重发需求 |
| D2 stale-Running 未重启 | **仅 is_run_continuable 时续（选）** | 不主动 pause-then-resume；崩溃重启主路径由启动自愈覆盖；运行期 ACP 死亡触发 pause 留将来 |
| | 主动 pause-then-resume（弃） | 覆盖面大但要处理 pause 副作用与并发，MVP 不建议 |

**已知限制（告知，非本期修）**：启动自愈只覆盖激活 workspace；非激活 workspace 的崩溃 run 不自愈→不续跑→Fresh。后续可扩 per-workspace 自愈 / 切换 workspace 时触发。

> ✅ **multica 子系统已于 §12.15（改动十三）补全**：`recover_multica_work_dir_sessions` 在 home 自愈后遍历 `multica_task_conversations` 的全部 `work_dir` 逐个 `recover_interrupted_running_sessions`，把 multica 远程任务在各 work_dir 的孤儿 Running run 翻成 Paused + ProcessInterrupted → `classify_resume` 命中 Resume。**通用 workspace（非 multica）的该限制仍告知**。

**验证**：`cargo check` 通过（仅 4 既有死代码警告）；`cargo test -p gold-band-desktop multica::` **72 测全过**（新增 7：`classify_resume_from` 纯逻辑——无 checkpoint / session 缺或空白 / run 不可达 / Paused+ProcessInterrupted 命中 Resume 并校验 ids+session / Running→Fresh / Paused+ErrorBlocked→Fresh / 缺 locator→Fresh）。无需 webank server 改动。

> ⚠️ 其中「session 缺或空白 → Fresh」断言后被 **§12.17（改动十五）反转**：`session_id` 不再作为续跑门（崩溃常发生在首节点完成前、bridge 尚未回填 session_id，会误把可续 run 判 Fresh）。判定以 §12.17 为准。

### 12.6 改动四：issue 正文预填（claim 响应带 issue_description）

**背景**：码灵 claim 到远程分配的 issue 后，composer 输入框只预填 issue 标题（thread_name），而非 issue 正文（`issue.Description`）——演示里只见一行标题、不见需求内容，不合理。

**根因（两层，server + client 各一）**：
- **Layer 1（webank server）**：`buildClaimedTaskResponse`(daemon.go) issue 分支只回填 `resp.ThreadName = issue.Title`，**丢弃 `issue.Description`**；`AgentTaskResponse`(agent.go) 无 issue 正文字段（与 issue 级 durable 正文缺位，对比已有 project 级 `ProjectDescription`）。
- **Layer 2（码灵 client）**：`requirement_text()` 来源优先级里 issue 型无正文来源 → 回退 title（M5-n/改动三 前 issue 型预填就是标题）。

**修复**：
- **Layer 1（multica-webank）**：`AgentTaskResponse` 加 `IssueDescription string`（与 `ProjectDescription` 对称的 issue 级 durable 正文，`json:"issue_description,omitempty"`）；`buildClaimedTaskResponse` issue 分支回填 `resp.IssueDescription = issue.Description.String`（pgtype.Text → "" when NULL）。
- **Layer 2（码灵 client）**：`RemoteTask` 加 `issue_description: Option<String>`（`#[serde(default)]`）；`requirement_text()` 优先级在 `handoff_note` 后、`title` 前插入 `issue_description`（issue 型取正文，无正文才回退 title）。

**验证**：webank 新增 `TestClaimTaskByRuntime_IssueDescription`（建 runtime+agent+issue → UPDATE description 为多行正文 → seed queued issue task → claim → 解码 `task.issue_description == 正文` 且 `thread_name` 不变）过；码灵 `remote_task_requirement_text_picks_source_by_priority` 扩 issue_body / handoff_over_body 两用例过。**需 rebuild + 重启 webank server**（Layer 1 是 server 改动）。

### 12.7 改动五：开始执行时 issue 流转到 in_progress（与 done 对称）

**背景**：码灵执行某 issue 时，multica 看板上该 issue 卡片仍停在「待办」列（只挂 task 队列的「正在进行」badge），未流转到「进行中」列。

**根因（非补丁：补完对称缺失的流转，server 侧无 bug）**：multica server 的 `issue.status` 是 **agent/user-driven**（非 task 生命周期驱动）——`start_task`/`complete_task` 显式不动 issue.status（`task.go` 注释明示 "Issue status is NOT changed here — the agent manages it via the CLI"）。码灵此前只在**完成**时流转 issue（bridge `handle_run_completed` Success → `update_issue_status(.., "done")`，见 §12.4 改动二），**开始执行**时未流转 → 进行中态缺失。看板列由 `issue.status` 派生，「正在进行」badge 由 `agent_task_queue` 状态派生（两者独立，故出现「badge 在进行中、卡片仍在待办列」的错位）。issue 状态 enum（`backlog/todo/in_progress/in_review/done/blocked/cancelled`）与 `PUT /api/issues/{id}` 早已接受 `in_progress`，码灵 client 也已有 `update_issue_status`（改动二）——缺的只是「开始时调一次」。

**修复（码灵 commands.rs，纯 client 侧，无 server 改动）**：`start_multica_conversation_run` 的 **Resume / Fresh 两分支**、`start_task` 成功后、返回 VM 前，调 `mark_issue_in_progress(&client, &workspace_id, lease.issue_id.as_deref())`——复用改动二的 `client.update_issue_status(.., MULTICA_ISSUE_IN_PROGRESS_STATUS)`，与完成时 done **对称**。失败仅 `warn!`（任务已 start，issue 流转不阻断执行；issue 保持原状由 server 扫描器/用户兜底）。

**纯/IO 分离**（镜像 §12.5 classify_resume 模式）：`in_progress_target(issue_id: Option<&str>) -> Option<&str>`（过滤 None/空白）纯函数 + `mark_issue_in_progress` I/O 包装。

**验证**：`cargo test -p gold-band-desktop multica::` **75 测全过**（新增 `in_progress_status_constant_is_in_progress` + `in_progress_target_skips_absent_or_blank_issue`）。**无需 webank server 改动**。

### 12.8 改动六：终态任务留在所属工作空间列表（取代全局「最近完成」桶）

**背景**：不同 workspace 的已完成任务全挤在左侧远程列表单一全局「最近完成」列，可读性差；用户希望完成后仍留在各自 workspace 列表里，只标 completed/failed 状态。

**根因（设计选择迭代，非缺陷）**：M5-o 把终态历史放扁平全局「最近完成」桶（`recentlyCompleted`），跨 workspace 混排。改动六改为按 workspace 归组（与 active 任务同列）。

**修复（破坏式更新，删旧路径无兼容层，CLAUDE.md 开发阶段破坏式更新）**：
- **VM（vm.rs）**：`RemoteConversationSidebarVm` **删** `recently_completed` 字段；**删** `MulticaCompletedTaskVm` 整个结构；`RemoteTaskVm` 加 `local_task_id/run_id/project_id: Option<String>`（终态行才填，active 行恒 None）+ `from_completed(&MulticaCompletedTask, project_id)` 构造器（`completed_at` → `last_activity_at`，title 空白兜底 `remote_task_id`，`from_remote`/`from_failed_issue` 初始化 3 字段为 None）。
- **装配（commands.rs `get_multica_tasks`）**：删 `recently_completed` 装配；`multica_completed_tasks` 按 `workspace_id` 经 workspaces 列表解析 `project_id`（**未绑定 workspace 的终态行跳过**——无 project_id 无法直达），`from_completed` 成 `RemoteTaskVm`，按 workspace 分组进 `completed_by_workspace: BTreeMap<String, Vec<RemoteTaskVm>>`，与该 workspace active 任务经纯函数 **`merge_workspace_tasks(active, terminal)`** 合并（active 优先、按 `remote_task_id` 去重——覆盖 re-dispatch/resume 同 task 既在 queued 又在 completed 历史的场景）。
- **前端**：`MulticaRemoteTaskList.tsx` 删「最近完成」分区 + `CompletedTaskRow` + `recentlyCollapsed` state + `handleSelectCompleted`；`RemoteTaskRow` 加 `onSelectRun` prop，**终态行**（`localTaskId && runId && projectId`）内容区渲染为可点 `<button>` → `onSelectRun(projectId, localTaskId, runId)` 直达本地会话，active/pinned 行不绑点击。`types.ts` 删 `MulticaCompletedTaskVm` + `recentlyCompleted`、加 3 字段；i18n 删 `conversation.sidebar.multica.recentlyCompleted`（双语）；browser mock 同步。
- **顺带修类型不一致**：`WorkspaceShell.onNewConversationInWorkspace` 误标可选（`?`）与下游 ConversationSidebar/MulticaRemoteTaskList 必填声明类型不一致（App 恒提供）→ 改必填，消除 tsc 报错。

**纯/IO 分离**：`merge_workspace_tasks` 纯函数（active 优先 + remote_task_id 去重 + 终态追加）单测固化。

**验证**：`cargo test -p gold-band-desktop multica::` **75 测全过**（新增 `merge_workspace_tasks_appends_terminal_after_active_with_dedup` + `from_completed_carries_local_run_link_and_terminal_status`）；web vitest `multica-remote-task-list` 用例更新全过（终态行进 workspace 组 + 点击 onSelectRun / 时间本地时区）；tsc 源码零错误。**无需 webank server 改动**。

**既有无关失败（非本批次引入，留待单独处理）**：web `gold-themed-scrollbar.test.ts > keeps every theme scrollbar neutral and low contrast until hover` 在 Windows 失败——`src/styles.css` 提交为 LF，本机 `core.autocrlf=true` 检出转 CRLF，测试硬编码 `\n` 的 dark 双行选择器（`:root,\n:root[data-theme='dark']`）匹配失败（仅 dark 受影响，black/light/light-gray 单行不受影响）。属跨平台行结尾测试健壮性问题，与 multica 改动无关。

### 12.9 改动七：执行中的远程任务在侧栏可见（running 行 + 进行中标识）

**背景**：码灵点远程任务开始执行后，该任务从左侧远程列表消失，直到结束（completed/failed）才回来。用户希望执行中的任务也展示，只是标识不同（进行中）。

**根因（好设计、实现不完整，非补丁）**：`get_multica_tasks` 给每个 workspace 拼列表时只有两个数据源——`pending`（server `list_pending_tasks`，可领取 queued）+ `terminal`（本地 `multica_completed_tasks`，已完成/失败）。而"我正在执行"的任务处在 `active_runs`（claim+start 后登记、终态移除）：它既不在 server pending 池（已被本 runtime 领走）、也还没进终态历史，于是整段执行期落进空档消失。前端其实早已预留 running：`STATUS_VARIANT` 有 `running → secondary`、`canCancel = status==='running'`、`RemoteTaskRow` 行可点直达逻辑齐备——只是后端从不产出 running 行。属"实现不完整"，补全即可，不重构。

**同源不完整点（一并修，否则 running 行刷不出/标识不友好）**：
- `start_multica_conversation_run` 的**成功路径不发 `multica-task-updated`**（此前只有失败 teardown / 终态 / loop 取消检测发）→ 点发送后侧栏不即时刷新。
- 状态徽标直接渲染原始 `{task.status}`（字面 `"running"`），无 i18n → 标识不友好。

**修复（纯客户端，无需 webank server 改动）**：
- **VM（vm.rs）**：新增 `RemoteTaskVm::from_active_run(remote_task_id, &ActiveRemoteRun, project_id)`——status 固定 `"running"`、retryable=false、`last_activity_at = started_at`（在飞任务「最近活动」即启动时刻）、填 `local_task_id/run_id/project_id`（行整块可点直达进行中会话，与终态行同路径），title 空 → remote_task_id 兜底。`from_remote`/`from_failed_issue` 不变（queued/pinned 行仍无本地链接）。
- **装配（commands.rs `get_multica_tasks`）**：每个 workspace **单次取锁**取在飞 running 行（`active_runs` 按 `workspace_id` 过滤 + `from_active_run`）+ `runtime_id`，再按 runtime_id 拉 server pending，最后并入本地终态；纯函数 `merge_workspace_tasks` **升三参 `(running, pending, terminal)`**，顺序 running→pending→terminal、按 `remote_task_id` 去重、优先级 running > pending > terminal（重派/竞态下 running 是当前真相）。局部变量 `active`（实为 pending）正名 `pending`。
- **start 即时刷新（commands.rs `start_multica_conversation_run`）**：Resume/Fresh 两分支、`start_task` 成功 + `mark_issue_in_progress` 后、返回前补 `emit_multica_task_updated(&app_handle)`，让侧栏即时刷出 running 行（此前成功路径不发）。
- **前端徽标 i18n**：`MulticaRemoteTaskList.tsx` 徽标 `{task.status}` → `t('conversation.sidebar.multica.status.' + task.status, task.status)`（缺键回退原值）；`i18n.ts` 新增 `conversation.sidebar.multica.status.{queued,running,completed,failed}`：zh 待办/进行中/已完成/失败、en Todo/Running/Done/Failed（running 取「进行中」对齐 multica 看板列名；M5-aa/§12.10 起 `queued` 文案对齐为「待办」/「Todo」、替换原暗示中间态的旧词，并新增 `MULTICA_STATUS_TONE` 给徽章上色）。running 行天然带 Cancel(Ban) 按钮 + 整行可点直达，无需额外改。

**纯/IO 分离**：`from_active_run`（纯，无 I/O）+ `merge_workspace_tasks`（纯，三参顺序去重）单测固化。

**不做（避免过度设计）**：不引入 compose 准备期(prepare_lease)行——窗口短、用户正盯 composer，不在侧栏焦点，超出本次诉求。

**验证**：`cargo test -p gold-band-desktop multica::` **76 测全过**（新增 `from_active_run_marks_running_and_carries_local_link`；`merge_workspace_tasks_*` 用例改三参）；web vitest `multica-remote-task-list` **9 用例全过**（新增 running 行：渲染「进行中」key + 点击 onSelectRun + Cancel tooltip）；tsc 零错误；全量 vitest 831 过 / 1 既有无关 scrollbar 失败（Windows autocrlf，§12.8 已述）。**无需 webank server 改动**。

### 12.10 改动八：绑定模型下沉到任务级 + 远程任务管理独立页（M5-z）

**背景**：M5-e 之前为 multica workspace 引入的 `local_project_id` 字段把「远程 workspace」和「本地工作目录」一次性绑死——加 workspace 时就强制选本地目录，绑定关系长期固化在 `MulticaWorkspaceRef` 上。这与「同一个远程 workspace 可能要在不同本地目录重复执行」「想先添加 workspace、执行时再决定目录」的实际使用方式相悖。同时配套的 `rebind_multica_workspace` 全链路（命令 / API / 前端 handler）为这个错误绑定打了补丁，复杂度外溢到 UI（侧栏被迫做本地/远程双 toggle、添加弹窗夹带 folder picker）。

**根因（根本性设计缺陷，需把缺陷一起修掉而非打补丁，遵循 CLAUDE.md 设计原则 1）**：绑定的粒度错了——绑定本应是**每次执行**的事（一次任务一次落点），却被提升到**workspace 注册**层（一次注册长期固定）。修法不是再加更灵活的 rebind，而是**把绑定模型整体下沉到任务级**：workspace 注册只收 provider，本地目录延迟到每次执行时由 composer 下拉选，选中的 projectId 写入当次任务级数据结构（`ActiveRemoteRun` / `MulticaCompletedTask` 各加 `local_project_id`），整条链路按任务级字段流转。

**修复（破坏式更新，删旧全链路无兼容层，CLAUDE.md 开发阶段破坏式更新）**：

**① 绑定模型下沉（核心，数据先动）**：
- `MulticaWorkspaceRef`（§2.2.1）**移除** `local_project_id` 字段——只保留 `id / name / slug / provider`。
- `ActiveRemoteRun`（§2.5）**新增** `local_project_id: String`——claim + start 时由 composer 选中的 projectId 注入。
- `MulticaCompletedTask`（§2.2.3）**新增** `local_project_id: String`——bridge `handle_run_completed` 写终态历史时从 `ActiveRemoteRun` 拷贝过来，保证完成后直达本地会话的 link 不丢。
- `MulticaWorkspaceRef::binding_for_multica()`（§2.5）**整函数删除**——绑定现在按任务级字段直接解析，不需要 workspace 级 binding 中间层。
- `invalidate_remote_task` 解析本地工作目录改走 `workspace_entry_for_project(&home_state, &run.local_project_id)`（按任务级 projectId 反查 conversation_workspaces 的 workspace_path），不再依赖 workspace 自身的 binding。

**② 删除 rebind 全链路（破坏式，无 fallback）**：
- 后端命令 `rebind_multica_workspace`（commands.rs）**删除**。
- 前端 API 4 层 `rebindMulticaWorkspace`（types / api / vm / hook）**全删**。
- 前端 handler `handleRebind`（SettingsPage multica 区块）**删除**，对应的「重新绑定」按钮 / 目录展示 / changeDirectory UI 一并清理。
- `add_multica_workspace` 命令签名收敛为 `(workspace_id, provider)`——name 不再由前端传，统一由 server workspace 列表返回的 name 决定（杜绝前后端 name 不一致）。
- 前端 `addMulticaWorkspace(workspaceId, workspaceName, provider)` 三参（name 来自 server 响应、原样回填，前端不编辑）。

**③ 添加工作空间弹窗简化（去 folder picker）**：`MulticaAddWorkspaceDialog` 只收「远程 workspace 下拉 + provider 下拉」，移除 folder picker / `pickLocalDirectory` 调用 / `bindDirectory` / `changeDirectory` / `directoryPlaceholder` / `needDirectory` / `notGitWarning` 整组目录相关 UI 与 i18n。

**④ 侧栏去本地/远程 toggle + 远程任务独立整页**：
- `ConversationSidebar` **删除**本地/远程双 toggle（删 `localTab` / `remoteTab` 文案与切换状态）——侧栏回归纯本地工作空间列表，符合桌面端产品心智（Gold Band 不做 terminal/双模心智）。
- 远程任务管理抽出**独立整页** `MulticaTaskManagementPage`，新路由 `/chat/multica-tasks`，与会话/上下文/运行模式管理类目并列，icon=`Globe`。
- 仅会话模式（新 UI `/chat/...`）有此页；工作台模式（旧 UI `/tasks/...`）不做镜像——multica 远程任务是会话模式独有的能力。
- 既有 §12.8/§12.9 的远程任务列表装配（`get_multica_tasks` + `merge_workspace_tasks(running, pending, terminal)` + 终态行/running 行可点直达）整体平移到新页面，数据源与装配逻辑不变。

**⑤ composer 执行时选本地工作区（claim-at-click → execute-time landing，Req D）**：
- 点远程任务的 play → `claimMulticaTask` 领取 → prefill composer（文本取 `requirement_text()`，multica binding 为 `{remoteTaskId, workspaceId}`，**不带 localProjectId**——本地目录还没选）→ 落到 `conversation-home`。
- App 预选本地工作区：`activeWorkspaceId ?? lastActiveWorkspaceId`（既有侧栏字段，§3.1 ConversationSidebarVm）。
- composer 在 multica binding 激活时**强制显示本地工作区下拉**（即便只有 1 个本地 workspace，也展开让用户确认/改选）。
- 边界：0 本地 workspace 时 composer 显示 `conversation.composer.multicaNeedLocalWorkspace` 引导文案并禁用发送。
- 发送 → `start_multica_conversation_run` 用 composer 选中的 projectId 作为任务级 `local_project_id` 注入 `ActiveRemoteRun`；后续 complete/work_dir 解析/直达本地会话全部走任务级字段。
- `register_active_run(state, run.id, remote_task_id, workspace_id, local_project_id /* composer 选中的 */, issue_id, title)` 签名相应调整。

**⑥ i18n（双语 zh-CN / en，src/locales 同步）**：
- 新增 `multica.taskManagement.{title, subtitle}`（远程任务管理页标题/副标题）、`conversation.sidebar.multicaTaskManagement`（侧栏入口）、`conversation.composer.multicaNeedLocalWorkspace`（composer 0 本地工作区引导）。
- 删除 `conversation.sidebar.multica.{localTab, remoteTab}`（侧栏 toggle 文案）。
- 删除弹窗目录键 `multica.dialog.{bindDirectory, changeDirectory, directoryPlaceholder, needDirectory}`。
- 删除 `settings.multica.{connecting, disconnecting, connected, disconnected, addWorkspace, selectServerWorkspace, selectProvider, selectFolder, rebind, notGitWarning}`（settings 区块原本围绕 rebind / folder picker 的整套文案随 UI 删除一并清理）。

**纯/IO 分离**：`workspace_entry_for_project(&home_state, &local_project_id)` 纯查询函数（按 projectId 反查 conversation_workspaces 条目，无 I/O）单测固化；`invalidate_remote_task` 仅作 I/O 包装。

**验收要点**：
- 远程 workspace 添加只收 provider；本地目录延迟到执行时由 composer 选。
- 同一个远程 workspace 可在不同本地目录重复执行（每次执行独立落地、独立 task 级 local_project_id）。
- 侧栏纯本地；远程任务在独立整页 `/chat/multica-tasks` 管理。
- claim-at-click：点 play 即领取 + 预填（multica binding 不带 localProjectId），发送才真正 start。
- rebind 全链路删除，无兼容层（开发阶段破坏式更新）。

**验证**：`cargo test -p gold-band-desktop multica::` 全过（新增 `workspace_entry_for_project_*` 用例覆盖命中 / 缺 projectId / 缺 workspace 三分支）；web vitest `multica-task-management-page`（新）+ `conversation-composer` multica binding 用例全过；tsc 零错误。**无需 webank server 改动**。

---

### 12.11 改动九：远程任务管理页 5 项前端打磨（看板词汇对齐 + 状态色调徽章 + 手动刷新 + 副标题精简 + 任务来源下拉，M5-aa）

**背景**：§12.10（M5-z）远程任务管理独立成页后，前端有 5 处散落打磨点需固化。均为纯前端，无 Rust / webank server 改动。

**心智（先于实现）**：码灵作为 multica Daemon 角色，自身即驱动 board issue.status（start 时 in_progress、complete 时 done，§12.4/§12.7），故本地任务生命周期与 multica 看板词汇 1:1 对应——后端 canonical 状态仍是 `queued|running|completed|failed`（不变），但前端展示文案 / 色调直接采用看板词汇，不再用暗示中间态的内部词。

**① 状态词汇对齐看板（canonical→display 文案映射，非读看板实时状态）**：
- 缺陷：§12.9 引入的 i18n `conversation.sidebar.multica.status.queued` 旧值（暗示存在独立「领取」中间态）与 claim-at-click（点 play 即 claim+prepare lease、无独立「领取」步，§12.10 ⑤）语义冲突。
- 修复：i18n 该键对齐 multica 看板列名——zh「待办」、en「Todo」（替换原暗示中间态的旧词）；`running=进行中`/`completed=已完成`/`failed=失败` 不变。后端 canonical 状态不变，仅改 i18n 文案。

**② 状态色调徽章（`MULTICA_STATUS_TONE` 导出 const，结构化管理色调）**：
- `web/src/components/conversation/MulticaRemoteTaskList.tsx` 顶部导出 const：
  ```ts
  export const MULTICA_STATUS_TONE: Record<string, string> = {
    queued:    'border-transparent bg-muted text-muted-foreground',                                       // 待办=灰
    running:   'border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-300',                  // 进行中=黄
    completed: 'border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-300',            // 已完成=绿
    failed:    'border-transparent bg-destructive/15 text-destructive',                                  // 失败=红
  };
  ```
- 每个canonical status 锁定一个 Badge className（杜绝硬编码 / 散落三元），色调与看板列色一致。`RemoteTaskRow` 渲染：`statusTone = MULTICA_STATUS_TONE[task.status] ?? MULTICA_STATUS_TONE.queued`（缺键回退 queued 灰），`<Badge variant="outline" className={cn('h-4 px-1 text-[10px] leading-none', statusTone)}>{t('conversation.sidebar.multica.status.'+task.status, task.status)}</Badge>`。
- 导出 const 供单测固化「4 个 canonical status 各有 locked tone」接口层验收。

**③ 手动刷新按钮（`MulticaRemoteTaskList` 顶部右侧）**：
- `MulticaRemoteTaskList` 内新增 `const [refreshing, setRefreshing] = useState(false)`（不复用 `loading`——`loading` 用整屏 spinner 替换列表，`refreshing` 仅 spin 当前按钮）。
- `fetchTasks` 返回 promise（`finally` 收尾），新增 `handleManualRefresh = () => { setRefreshing(true); fetchTasks().finally(() => setRefreshing(false)); }`——调与 mount / `subscribeMulticaTaskUpdates` / `subscribeMulticaSettingsUpdates` 事件订阅**同源**的 `fetchTasks()`（getMulticaTasks）。
- UI：列表区顶部 `<div className="flex justify-end">` 内 `<Tooltip><TooltipTrigger asChild><Button size="icon" variant="ghost" className="size-7" disabled={refreshing} onClick={handleManualRefresh} aria-label={t('common.refresh')}><RotateCw className={cn('size-3.5', refreshing && 'animate-spin')} /></Button></TooltipTrigger><TooltipContent>{t('common.refresh')}</TooltipContent></Tooltip>`。免切走/重进即可拉最新任务列表。

**④ 副标题精简（`multica.taskManagement.subtitle`）**：
- i18n 删除原尾部关于执行时选本地目录的半句（旧尾部仅赘述实现细节，已被 claim-at-click → composer 执行时选本地工作区流程 §12.10 ⑤ 覆盖）。**§12.12 进一步删除 "multica" 限定词**（页头「任务来源」下拉已点名来源），最终副标题见 §12.12。
- 依据 CLAUDE.md：UI 只展示帮助决策/完成操作的文案，不展示解释实现方式/布局原因/未来扩展性的说明。

**⑤ 任务来源下拉（`REMOTE_TASK_SOURCES` 配置 + 页级 `source` 渲染分流）**：
- `web/src/pages/MulticaTaskManagementPage.tsx` 顶部导出 const + 类型：
  ```ts
  const REMOTE_TASK_SOURCES = [
    { value: 'multica', labelKey: 'multica.taskManagement.source.multica' },
  ] as const;
  type RemoteTaskSource = (typeof REMOTE_TASK_SOURCES)[number]['value'];
  ```
  新增来源 = 向数组加一项（结构化管理，杜绝硬编码）。
- 页头 `PageHeader.actions` 内追加「任务来源」`Select`（i18n label `multica.taskManagement.source.label` zh「任务来源」/ en「Task source」）：`<Select value={source} onValueChange={(v) => setSource(v as RemoteTaskSource)}>`，选项来自 `REMOTE_TASK_SOURCES.map(s => <SelectItem value={s.value}>{t(s.labelKey)}</SelectItem>)`。
- 页级 `const [source, setSource] = useState<RemoteTaskSource>('multica')` 是**渲染分流唯一键**：body 内 `{source === 'multica' ? <MulticaRemoteTaskList .../> : null}`。本期仅落地 multica，为未来多来源接入保留切换位（各来源自带数据/刷新，body 按 source 分支渲染，互不影响）。

**纯/IO 分离**：`MULTICA_STATUS_TONE` 是纯 const（无 I/O）；`handleManualRefresh` 仅作 `fetchTasks`（既有 I/O 路径）的 `refreshing` 包装。

**验收要点（已固化）**：
- `queued` 显示「待办」/「Todo」（替换原暗示中间态的旧词）；其余三态文案不变。
- 4 个 canonical status 各有 locked tone（灰 / 黄 / 绿 / 红），集中管理于 `MULTICA_STATUS_TONE`，无散落三元/硬编码。
- 刷新按钮调同一 `fetchTasks`、有 `refreshing` spin 态、不复用 `loading`。
- 副标题删除原尾部关于执行时选本地目录的半句；**§12.12 进一步删除 "multica" 限定词**，最终副标题见 §12.12 验收要点。
- 页头有任务来源 `Select`、页级 `source` state 是渲染分流唯一键、`REMOTE_TASK_SOURCES` 是配置数组（可扩展、本期仅 multica）。

**验证**：纯前端（无 Rust / 无 webank server 改动）；i18n 中英双语同步（`status.queued` zh「待办」/ en「Todo」、`multica.taskManagement.subtitle` 精简、`multica.taskManagement.source.{label,multica}` 新增、`common.refresh` 复用既有 zh「刷新」/ en「Refresh」）。

---

### 12.12 改动十：远程任务列表「树形视觉/层级系统」统一打磨（M5-ab）

**背景**：§12.10/§12.11 远程任务管理独立成页 + 看板词汇/色调/手动刷新落地后，列表本体（workspace 组头 + 任务行 + 空状态）各项视觉参数散落各自一套、未成体系——组头 hover 只有文字变色、组间距偏窄（`mb-1`）、任务行时间戳与 Badge 挤在同一行无左右分配、workspace 行与任务行无图标层级区分、空状态贴左单薄。本轮作为一次统一的「树形视觉/层级系统」打磨，建立一致间距节奏与层级表达，非 8 个孤立补丁。纯前端，无 Rust / webank server 改动。

**心智**：workspace→任务是树状结构——组头 = 可折叠的 workspace **容器**，任务行 = 其下的**叶子**。通过统一水平缩进、垂直节奏、hover 反馈、图标区分让「容器 vs 叶子」层级一眼可读。仅 `web/src/components/conversation/MulticaRemoteTaskList.tsx`（含内部 `RemoteTaskRow`）改动，会话模式远程任务管理页专属；工作台/旧 UI 与本地 `ConversationSidebar` 不受影响。

**① 副标题再精简（`multica.taskManagement.subtitle`）**：
- i18n 在 §12.11 版（已删尾部实现细节赘述）基础上**进一步删除 "multica" 限定词**，再精简为 zh「查看并执行远程任务」/ en "View and run remote tasks"。
- 依据：页头「任务来源」下拉（§12.11 ⑤）已点名来源（当前唯一项 Multica），副标题不再重复限定词（UI 不展示冗余信息）。

**② 组头图标统一 + 工作空间图标（lucide `Server`）**：
- workspace 分组头与 pinned 分组头采用**统一折叠箭头规格**：`<ChevronDown className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform', collapsed && '-rotate-90')} />`（旋转表达展开/折叠，尺寸/颜色统一）。
- workspace 分组头名称**左侧新增** lucide `Server` 图标 `<Server className="size-3.5 shrink-0 text-muted-foreground" />`——「服务器图标 = workspace 容器行」与下方「无图标 = 任务叶子行」形成视觉层级区分。
- pinned 失败段不是 workspace，**不加** Server 图标（仅折叠箭头 + 文案）。

**③ 任务计数（i18n 新键 `conversation.sidebar.multica.taskCount`）**：
- workspace 名称右侧展示 `<span className="ml-1 truncate text-[11px] font-normal normal-case tracking-normal text-muted-foreground">{t('conversation.sidebar.multica.taskCount', { count: tasks.length })}</span>`。
- i18n：zh「（{{count}}个任务）」/ en「({{count}} tasks)」。
- 仅 `tasks.length > 0` 时渲染（0 任务由组内空状态文案表达，避免与空状态冗余）。

**④ 组头 hover 底色**：
- workspace 分组头与 pinned 分组头按钮 className 由「仅 hover 文字变色」升级为整行容器 hover：`rounded-md px-1.5 py-1 ... transition-colors hover:bg-muted/40 hover:text-sidebar-accent-foreground`——组头有明确 hover 底色容器感，与下方任务行清晰区分。

**⑤ 分组间距加大**：
- workspace 分组 wrapper `mb-1` → `mb-2`，pinned 段对齐——分组之间块感更强。

**⑥ 任务行排版（`RemoteTaskRow` 内容区）**：
- 标题 `<div className="truncate text-[14px] font-medium leading-snug text-foreground">`——保持 14px（侧栏密度上限），用 `font-medium` + 全强 `text-foreground` 强调为主文本。
- 元信息行 Badge 居左、时间戳 `ml-auto` 推右：`<span className="ml-auto shrink-0 truncate text-[10px] tabular-nums">{formatLocalDateTime(task.lastActivityAt)}</span>`——时间戳对齐本地 `ConversationSidebar` 规格（`text-[10px] tabular-nums text-muted-foreground`），明确为辅助信息。
- 任务列表容器加 `pl-2`（`<div className="mt-0.5 space-y-0.5 pl-2">`）——任务行相对组头统一缩进，层级更清。
- 整行 hover 背景 `hover:bg-muted/40` 保留（行 `px-2`）。

**⑦ 组内空状态文案 + 样式**：
- i18n `conversation.sidebar.multica.noTasksInWorkspace` 由旧的两词简短文案（语义模糊、不点明所属与对象）改为 zh「该工作空间下暂无远程任务」/ en "No remote tasks in this workspace yet"（明确所属=工作空间、对象=远程任务）。
- 样式由贴左 `px-2 py-1` 改为**居中带垂直留白**：`<p className="px-2 py-4 text-center text-[11px] text-muted-foreground">`——不再单薄贴左。

**⑧ 间距系统统一（统一以上各项的根设计，杜绝散落 magic class）**：
- 水平 padding：组头 `px-1.5`、任务行 `px-2`（对齐，内容不再贴容器边）。
- 垂直节奏：列表根 `space-y-2`（header 刷新工具栏 ↔ 树）、组↔组 `mb-2`、任务↔任务 `space-y-0.5`、组头↔其任务 `mt-0.5`。

**纯/IO 分离**：本轮全为呈现层 Tailwind class / i18n 文案调整，无新 I/O 路径（`taskCount` 取自既有 `tasks.length`，无新数据源）。

**验收要点（已固化）**：
- 副标题「查看并执行远程任务」/"View and run remote tasks"（删除 "multica" 限定词）。
- workspace 组头有 `Server` 图标 + 任务计数（`tasks.length > 0` 才显）；pinned 段无 Server 图标；两段折叠箭头规格统一。
- 组头 hover 有底色（`rounded-md hover:bg-muted/40`），区别于任务行。
- 任务行：标题 14px font-medium 主文本、时间戳 `ml-auto` 推右 `tabular-nums`、整行经容器 `pl-2` 统一缩进。
- 组内空状态居中带垂直留白（`px-2 py-4 text-center`）、文案「该工作空间下暂无远程任务」/"No remote tasks in this workspace yet"。
- 间距 token 统一：组头 `px-1.5` / 任务行 `px-2`；组↔组 `mb-2`、任务↔任务 `space-y-0.5`、组头↔任务 `mt-0.5`、列表根 `space-y-2`。

**验证**：纯前端（无 Rust / 无 webank server 改动）；i18n 中英双语同步（`multica.taskManagement.subtitle` 再精简、`conversation.sidebar.multica.taskCount` 新增 zh「（{{count}}个任务）」/ en「({{count}} tasks)」、`conversation.sidebar.multica.noTasksInWorkspace` 文案更新）；vitest 19/19 全过、tsc 零错误。

### 12.13 改动十一：multica 接入性能优化（共享 client / liveness 短超时 / 自愈单次重试 / tick 埋点，M5-ac）

**背景**：对 multica 接入做系统性性能检视（并发/IO/网络/内存/循环/重复查询/锁/资源释放/对象创建/超时重试 10 维度）。业务逻辑正确性前置确认无误（锁纪律从不跨 `.await`、终态上报严格重试、4xx 不重试、前端纯事件驱动无轮询、PAT 不回显）。本次仅落地两项**严重缺陷** + 埋点；并发扇出（heartbeat/lease/cancel-detect 并发）与前端 debounce 等增强项本期不做。

**严重缺陷 S1：`reqwest::Client` 每调用重建（连接池失效）**
- 现象：`MulticaClient::new` 每次 `Client::builder().timeout(30s).build()`，而 `new` 被每个调用点重建（心跳 tick / 启动 / bridge 终态事件 / 每条命令）。
- 影响：`reqwest::Client` 内部持有连接池，官方明确「应创建一次并复用」。每次 new → 新连接池 → 旧 client 析构关闭连接 → 下次请求重做 TCP+TLS 握手。心跳 15s × N workspace 常驻握手开销；弱网下握手失败概率叠加放大重试成本。是「对象频繁创建 + 网络 + 超时」三维度叠加的系统性缺陷。
- 修复（`client.rs`）：进程级 `shared_http() -> &'static Client`（`OnceLock`，零新增依赖，同 `metrics.rs`/`view_models.rs` 惯用）；`MulticaClient::new` 取 `shared_http().clone()`（廉价 Arc clone）；`MulticaClient` 加 `#[derive(Clone)]`（为并发扇出铺路）。client 级 30s 超时仍在 `shared_http` 内设定。

**严重缺陷 S2：心跳 tick 串行 + 自愈嵌套 client 内重试 → 弱网下单 tick 超 45s**
- 现象：`run_heartbeat_loop` 每 tick 串行四阶段；`self_heal_registration` → `register_workspace` → `client.register`（`with_network_retry` 3 次，单次复用 client 级 30s）。
- 影响（弱网 / server 慢响应，恰是 runtime_id 缺失触发自愈的场景）：单个缺失 workspace 的 register 最坏 ≈ 30s+1s+30s+2s+30s ≈ 93s；多个 workspace ①阶段即数分钟 → ③`extend_prepare_lease` 推迟 > 45s → server `ReclaimStaleDispatchedTaskForRuntime` 回收用户正在 compose 的任务（功能受损，非单纯变慢）；④取消检测推迟 → 远端已取消任务本地空跑浪费算力。
- 设计根因：`register` 的 client 内 3 次重试是为**一次性**调用（启动/connect/绑定）设计的；自愈是**循环驱动**的，「循环即重试」（与 heartbeat/extend_prepare_lease 同构）才是正确语义。自愈复用了启动期重试路径 → 单 tick 内嵌套重试 → tick 超时。
- 修复（`client.rs` + `loop_.rs`）：
  - **S2-a 自愈单次重试**：新增 `register_once`（单次 + liveness 短超时）；`register_workspace` 加 `retried: bool`——一次性路径（启动/connect/绑定 `commands.rs::register_workspace_best_effort`）传 `true` 走 `register`（3 次重试），自愈路径（`self_heal_registration`）传 `false` 走 `register_once`。
  - **S2-b liveness 短超时**（常量 `LIVENESS_TIMEOUT_SECS=10`）：`send`/`json_send` 加 `per_request_timeout: Option<Duration>` 参数；`heartbeat` / `extend_prepare_lease` / `get_task_status` / `register_once` 传 `Some(10s)`，其余传 `None`（沿用 30s）。单 tick 在退化网络下快速失败、下一 tick（15s）重试。

**tick 耗时埋点**（`loop_.rs`）：每 tick 记 `tick_start`，四阶段各 `trace_stage(stage, start)`（trace 级 `elapsed_ms`）；tick 末总耗时——正常 debug、超 30s 升 warn（"heartbeat tick overrun"）。量化 S2 改善的回归依据。

**改动清单**：`src-tauri/src/multica/client.rs`（`shared_http` + `derive(Clone)` + `new` 复用 + `send`/`json_send` 加 timeout + `register_once` + 3 个 liveness 方法走 json_send/send + `LIVENESS_TIMEOUT_SECS` + 3 单测）、`src-tauri/src/multica/loop_.rs`（`register_workspace(retried)` + 自愈 `false` / 启动·connect·绑定 `true` + tick 埋点 + `trace_stage`）、`src-tauri/src/multica/commands.rs`（`register_workspace_best_effort` 调用传 `true`）。无 web / 无 webank server 改动。

**验证**：`cargo check`（gold-band-desktop）零错误（4 个 warning 均为既有非 multica 项）；`cargo test ... multica` 75 全过（含新增 `multica_client_new_rejects_empty_base_url` / `multica_client_is_clone` / `liveness_timeout_is_bounded_under_global_default`）。弱网人肉验证待补：模拟限速/断网恢复，观察日志 tick `elapsed_ms` 不再超 30s、compose 期间任务不被回收。

### 12.14 改动十二：断点续跑补完——client 消费 parent_task_id 反查父索引（M5-ad）

**背景**：§12.5（M5-u）落地的断点续跑，`classify_resume` 按**字面 `remote_task_id` 查本地索引**。但服务端 auto-retry 走 `CreateRetryTask`（`agent.sql:418,423,432`）**克隆新 id 的子任务 T'**（不复用父 T 的 id）。崩溃/关闭后重启，用户重新领取的是**子任务 T'**（父 T 已 failed 无领取按钮），`classify_resume(T'.id)` 查 `multica_task_conversations[T'.id]` → 查不到 → 落 Fresh。本地那个 Paused(ProcessInterrupted) 的 run、`multica_task_conversations[T]` 索引、pin 过的 ACP session 全在磁盘上，但**id 对不上碰不到**——整条续跑链路在主场景下空转。

**根因（修根因非打补丁）**：客户端续跑判定与服务端任务模型错配——客户端按「**同 task_id** 重派回本机」的假设写字面查找（只有服务器复用同一 id 才成立），而服务端实际用子任务。客户端同时还漏收服务端**已正确提供**的血缘：claim 响应携带 `parent_task_id`（`agent.go:296,635`）+ `prior_session_id`（响应只输出，`agent.go:311`），且 claim 处理器**不解码请求体**（`daemon.go:2508,2671`）——客户端反而往请求体塞 `prior_session_id`（死逻辑，服务端从不读）。

**修复（纯客户端，无服务端/无前端改动）**：让客户端消费服务端已给的 `parent_task_id`，续跑判定从「只按字面 id 查」升级为「字面 id 查不到时按 `parent_task_id` 反查父任务索引」，喂给**现成的** Resume 分支（不改执行引擎）。续跑成功后迁移索引到子任务 id，保证链式重试可续。

**数据（先定数据）**：
- `PrepareLease`（`state.rs`）+= `parent_task_id: Option<String>`（claim 响应带回，续跑反查主路径）+ `prior_session_id: Option<String>`（服务端回填的父 session，兜底/校验用）。
- `RemoteTask`（`client.rs`）+= `parent_task_id: Option<String>`（`#[serde(default)]`）；既有 `prior_session_id` 角色从「待发送」变为「从响应消费」。
- `ClaimRequest` → `pub struct ClaimRequest {}`（服务端不读请求体）。

**接口（再定接口）**：
- 纯函数 `resolve_resume_checkpoint(map, remote_task_id, parent_task_id) -> Option<MulticaTaskConversation>`：两级查找——`map.get(remote_task_id)` 命中（同 id 场景）→ 用之；否则 `parent_task_id.and_then(|p| map.get(p))`（子任务场景）。
- `classify_resume` 新签名 `classify_resume(home_app, remote_task_id, parent_task_id: Option<&str>) -> ResumeDecision`（读 checkpoint → 两级反查 → `is_run_continuable` 校验）。
- 纯函数 `migrate_resume_index_map(map, child_id, parent_id, local_task_id, local_run_id)`：子任务 entry 继承父 session_id/work_dir，`remove(parent_id)` 清掉被取代的父 entry；I/O 包装 `migrate_resume_index(&App, …)` best-effort load/pure/save（续跑已成功，迁移失败仅 `warn!`）。
- `claim_multica_task`：从 claim 响应取 `parent_task_id`/`prior_session_id` 写入 `register_prepare_lease`；**删除**往请求体塞 `prior_session_id` 的死逻辑，`claim_specific_task(runtime_id, task_id)`（body=`{}`）。
- `start_multica_conversation_run` Resume 分支：`classify_resume(&app, &remote_task_id, lease.parent_task_id.as_deref())`；Resume 成功且 `parent_task_id` 指向父任务（≠ 当前 id）时调 `migrate_resume_index` 把索引迁到子任务 id。

**ResumeDecision 清理**：`Resume{local_task_id, local_run_id, session_id}` 的 `session_id` 字段在 §12.5 落地后已无消费者（claim 不再需要回传它）→ 删除该字段（破坏式清理，无 fallback）。

**一致性自检**：Resume 分支以父 local ids 注册 `active_runs[T']`，bridge `find_active_run_by_local` 按父 local ids 命中 T'，终态 `finalize_terminal` 清 `active_runs[T']` + `multica_task_conversations[T']`——迁移后键链自洽。

**效果边界**：关闭/崩溃后 **2h 内**重启 → 重新领取重试子任务 T' → 续跑父任务本地 Paused run + 旧 ACP session；**超 2h** → T' 已 expired（queued TTL），只能 rerun（force_fresh_session）。续跑失败（session 死 / run 不可续）仍落 Fresh 兜底，不劣化于现状。

**前置验证（Step 0，gating）**：续跑最后一公里是 `run_continue_background` → ACP `session/load` 能否跨进程重启恢复旧会话——设计文档 §9 风险 9 / §9.2 M1 一直未验证。结果决定续跑是真续还是兜底重跑；若不可恢复，本方案仍正确（必要条件），但会落 Fresh，需另立「ACP session 持久化」课题。

**改动清单**：`commands.rs`（resolve_resume_checkpoint / migrate_resume_index(_map) / classify_resume 新签名 + parent 参数 / claim 消费响应血缘 + 删请求体死逻辑 / start Resume 分支迁移索引 + ResumeDecision 去字段）、`state.rs`（PrepareLease += parent_task_id/prior_session_id + register_prepare_lease 新参）、`client.rs`（RemoteTask += parent_task_id / ClaimRequest{} / claim_specific_task 去入参）、`config/mod.rs`（MulticaTaskConversation 文档订正两级解析）。无 web / 无 webank server 改动。

**验证**：`cargo test -p gold-band-desktop multica::` **80 测全过**（新增 `resolve_resume_checkpoint_prefers_literal_id_then_parent` / `migrate_resume_index_map_moves_parent_entry_to_child` / `migrate_resume_index_map_inserts_child_even_if_parent_missing` / `prepare_lease_carries_resume_lineage` / `remote_task_reads_parent_task_id_lineage` / `claim_request_is_empty_object`；同步更新 `resume_when_run_paused_process_interrupted`）。`cargo clippy --all-targets` 无新增 lint。运行时 e2e（kill 重启续跑、T→T'→T'' 链式、续跑失败落 Fresh、超 2h rerun、同 id 回归）待 §31 Step 0 通过后跑。

### 12.15 改动十三：断点续跑根因修复——启动自愈覆盖 multica work_dir（M5-ad 收尾）

**背景（实测复现）**：§12.14 把续跑指针从字面 id 改为 `parent_task_id` 反查父索引后，真实崩溃重启场景仍落 Fresh——启动远程任务 → 关码灵 → 10 分钟后重开 → 远程任务管理点「运行」→ **新会话**。代码级排查逐层排除：① `npm run dev` = `tauri dev` 会重编 Rust（fix 已生效）；② server `buildClaimedTaskResponse`(daemon.go:1592) 经 `taskToResponse` 继承 `ParentTaskID`（响应带 parent_task_id）；③ `RemoteTask`(client.rs:158) serde 正确解析。三者均非因。

**根因**：启动自愈 `recover_interrupted_running_sessions()`（main.rs:152）跑在 `state.app()` = **home repo** 上；其 `pause_all_running_sessions()`（app/mod.rs:2370）只遍历 `self.task_list()`，作用域锁死单一 repo。而 multica 远程任务的 run 落在**该任务自己的 `work_dir`**（独立 repo）→ 重启时**从不被 pause** → 残留 stale `Running` → `classify_resume` 读 `run_status` → `is_run_continuable` = **false** → **Fresh**。这正是 §12.5「已知限制」从「限制」升级为「bug」。

**修复（根因级，非补丁）**：把启动自愈扩展到 `multica_task_conversations` 引用的**全部 work_dir**——与 `classify_resume` 读同一张权威表，保证被判定可续的 run 在判定前已被 pause。

**数据**：无新结构。新增纯函数 `collect_multica_work_dirs(map: &HashMap<String, MulticaTaskConversation>) -> Vec<String>`（取全部 `work_dir`，trim 后去重、丢空，可单测）。

**接口/实现**：
- `recover_multica_work_dir_sessions(home_app: &App)`：load_state → `collect_multica_work_dirs` → 逐个 `home_app.with_repo_root(work_dir, config)` 构造 workspace App → `recover_interrupted_running_sessions()`；单 work_dir 失败仅 `warn!` 不阻断；末尾 `info!` 汇总（work_dir_count / recovered）。
- `main.rs` setup：home `recover_interrupted_running_sessions()` 后紧接 `recover_multica_work_dir_sessions(&runtime_app)`。
- `classify_resume` 加诊断 `info!`：`resolved_via`(literal/parent/none/no-map) / `session_present` / `run_status` / `continuable` / `decision`——供运行时复测确认修复生效（命中 Resume 时应为 `resolved_via=parent, continuable=true, decision=Resume`）。
- 安全前提：启动瞬间磁盘上所有 `Running` 都是上一轮崩溃遗留的孤儿态（进程刚起，无在飞 run），pause 全部正确。

**改动清单**：`src-tauri/src/multica/commands.rs`（`collect_multica_work_dirs` + `recover_multica_work_dir_sessions` + `classify_resume` 诊断日志 + 2 单测）、`src-tauri/src/main.rs`（import + setup 调用）。无 webank server 改动。

**验证**：`cargo check -p gold-band-desktop` 通过（仅 4 既有死代码警告）；`cargo test -p gold-band-desktop multica::` **82 测全过**（新增 `collect_multica_work_dirs_dedup_trims_and_drops_empty` / `collect_multica_work_dirs_empty_map_yields_empty`）。前端 `multica-settings-block.test.tsx` 3 测过。运行时 e2e 复测**仍落 Fresh**——本节新增的诊断 `info!` 暴露 `session_present=false → decision=Fresh`，最终根因（`session_id` 门用错信号）与修复见 **§12.17（改动十五）**。

> **更新（M5-ax，2026-09-03）**：本节的全量 work_dir 自愈（`collect_multica_work_dirs` + 逐 work_dir `recover_interrupted_running_sessions` + main.rs 同步调用）已被 **§12.37** 重写——按 checkpoint 的 local_task_id+local_run_id **定点** pause/cancel，并移入 spawn_blocking 启动恢复管线；`collect_multica_work_dirs` 已删除（破坏式更新，无兼容层）。

---

### 12.16 改动十四：multica 设置按钮改名（切换账号 / 退出登录）

**背景**：设置页 multica 接入的「重新连接 / 断开连接」语义偏技术；按产品心智改为「切换账号 / 退出登录」。未连接态首连按钮「连接 Multica」不变。

**改动**：纯 i18n 值替换（key `reconnect`/`disconnect` 不变，组件已引用）：zh `重新连接`→`切换账号`、`断开连接`→`退出登录`；en `Reconnect`→`Switch account`、`Disconnect`→`Sign out`。`connect` 保持。

**逃生口外链保留**：账号邮箱旁的「切换账号逃生口」外链按钮（`handleSwitchAccount`→`openExternalUrl(appUrl)`）是 cookie 兜底——`browser_login` 见 cookie 即签 JWT，cookie 是错账号时静默连错；点「切换账号」只是再跑一次 browser_login，同样的错 cookie 会再连错，故「去 Web 手动登出/登录」外链仍不可少（根因待 webank 加授权确认屏 M5-l）。仅把其 tooltip 里引用的旧按钮名「重新连接」改为「切换账号」。

**改动清单**：`web/src/i18n.ts`（zh/en 4 处文案 + tooltip 2 处）、`web/src/components/settings/MulticaSettingsBlock.tsx`（注释同步）。无 Rust / 无 webank server 改动。

**验证**：`npx tsc --noEmit` 无新增错误（仅既有 tests/* 噪声）；`vitest multica-settings-block` 3 测过。

---

### 12.17 改动十五：断点续跑最终根因——删除 classify_resume_from 的 session_id 门（M5-ae）

**背景（实测复现，§12.15 收尾后）**：§12.15 把启动自愈扩到 multica work_dir 后，用户复测仍落 Fresh。§12.15 新增的诊断 `info!`（`multica classify_resume decision`）四条记录精确定位到一行：

```
resolved_via="parent" session_present=false run_status=Some(Paused) continuable=true decision=Fresh
```

即：父系反查命中（`resolved_via=parent`）✓、启动自愈把 run 翻成可续（`run_status=Paused` + `continuable=true`）✓——**唯独 `session_present=false` 触发了 Fresh**。

**根因（设计层的门用错信号，非补丁）**：`classify_resume_from` 以 `checkpoint.session_id` 非空作为续跑门。但该字段**仅由 bridge 在 `NodeCompleted` 回填**（Fresh 写入时为 `None`，bridge 于节点完成后 pin 回填）。崩溃常发生在**首个节点完成前**——`worker-ref.json` 已写 ACP session（故 run 可续、`继续` 能跑），但 `NodeCompleted` 未触发 → bridge 未回填 → `checkpoint.session_id` 恒 `None` → 门误判 Fresh。

关键：该门把「checkpoint 记录了 session」当成「run 有可续 session」的代理，而这是**错代理**——续跑执行器 `run_continue_background(task_id, run_id, None, None)` 直接读 `worker-ref.json` 的 `continue_ref.acpSessionId` 恢复 session，**完全不读 checkpoint 的 `session_id` 字段**；该字段仅供 server `pin_task_session`。可续性应以 run 的真实状态（`is_run_continuable`：locator 齐 → attempt 已起 → worker-ref 已写 session）为准，而非一个回填滞后的字段。

**修复（根因级——删门而非放宽阈值）**：`classify_resume_from` 删除 `session_id` 空判定分支，续跑决策收敛为「checkpoint 解析出本地 ids + run 存在 + `is_run_continuable`」。安全前提：可续 run 必有 locator → attempt 已起 → worker-ref 有 session；即便极端（worker-ref 缺/损坏）`run_continue_background` 失败，§12.5 既有的「续跑失败 → 落 Fresh」兜底（commands.rs `Err` 分支 `warn!` + fallthrough）仍生效，无回退风险。`classify_resume` 的诊断 `info!` 仍计算 `session_present`——降级为纯诊断字段（观察 bridge 是否已回填），不再参与判定。

**改动清单**：`src-tauri/src/multica/commands.rs`（`classify_resume_from` 删门 + docstring 重写；`ResumeDecision::Resume` 文档微调；单测 `resume_fresh_when_session_id_missing_or_blank` → `resume_when_session_id_missing_or_blank`，断言反转为「session_id 缺/空白 + 可续 run → Resume」）。无 webank server 改动。

**验证**：`cargo test --bin gold-band-desktop multica::commands::` **17 测全过**（含反转后的 `resume_when_session_id_missing_or_blank`）。运行时 e2e（kill 重启续跑确认 `decision=Resume` 即便 `session_present=false`）待用户复测。

---

### 12.18 改动十六：换号/断开统一作废账号作用域状态——补齐 State 层索引 + connect 换号检测（M5-af）

**背景（M5-m 的不完整根治）**：M5-m（接入方案 M5-m）把 PAT/账号身份/workspace 绑定/active 定为账号作用域、disconnect 时清。但账号作用域状态实际**横跨两层配置**，M5-m 只清了 `SettingsConfig`，漏了 `StateConfig` 三个同样账号作用域的索引：

- `multica_pending_issues`（失败回显 issue id）
- `multica_task_conversations`（断点续跑索引，键 = remote_task_id）
- `multica_completed_tasks`（最近完成历史）

三者均以当前账号的 remote id 为键，换号/断开后对新账号无意义。且 `connect_multica` 完全不清——换号重连时旧账号状态原样保留。

**两个症状同根**：

1. 换号后设置页/任务列表仍见旧账号 workspace 绑定（M5-m 已修 disconnect，但 connect 换号路径仍漏）。
2. 换号后「置顶」折叠列表里残留旧账号失败 issue 且**点不进去**——失败回显行经 `RemoteTaskVm::from_failed_issue` 构造，**故意**不填 `local_task_id/run_id/project_id`（它是「显示失败 + 提供重试」入口，非「回看会话」入口），前端 `clickable = projectId && localTaskId && runId`（`MulticaRemoteTaskList.tsx:344`）恒 false → 不可点是**设计**；但它跨账号残留是 `multica_pending_issues` 未随换号作废的**泄漏**。

**根因（好的设计——账号作用域——但实现不完善）**：M5-m 的账号/机器作用域划分是对的（CLAUDE.md「好的设计但实现不够完善」分支）。缺的是：(a) 账号作用域全集漏了 State 三索引；(b) 只有 disconnect 一条触发路径，connect 换号未触发。补全实现而非改设计。

**修复（统一作废，非两处各打补丁）**：账号作用域状态 = Settings（workspaces/active）+ State（pending_issues/task_conversations/completed_tasks）两层全集；凭证变更（换号/断开）统一作废这两层，daemon_id（机器作用域）保留。一个不变量、两条触发路径。

**改动清单**：

- `src-tauri/src/multica/config.rs`：抽 `clear_multica_workspace_bindings(&mut SettingsConfig)`（清 workspaces/active，保留 pat/account/daemon_id——换号专用，pat/account 由新登录覆写）；新增 `clear_multica_state_indices(&mut StateConfig)`（清三索引）；`clear_multica_session` 签名不变、内部复用前者（disconnect 行为不变）；新增 `multica_account_changed(existing, new_email) -> bool`——以 email 判换号，任一 email 缺失 → false（同账号重连主流派保留绑定，脏绑定由 register 404 自愈）。+3 单测固化契约。
- `src-tauri/src/multica/mod.rs`：re-export 三个新 helper。
- `src-tauri/src/commands.rs`：`connect_multica` 加 `shared: State<'_, SharedMulticaState>` 参数（Tauri 按类型注入，main.rs 注册无需改）；browser_login 返回新 (pat,user) 后、覆写 pat/account 前——`multica_account_changed` 为真 → `clear_multica_workspace_bindings` + best-effort `clear_multica_state_indices`+save_state + `clear_runtime_ids`。`disconnect_multica` 在 `clear_multica_session` 后补 best-effort `clear_multica_state_indices`+save_state（补 M5-m 漏掉的 State 三索引）。

**不变量**：`StateConfig.multica_runtime_ids` 是死字段（仅声明、从不读写，真缓存在内存 `MulticaRuntimeState`），不处理。PAT 明文不回显（VM 仅 `pat_set`），换号清理不改变该约束。daemon_id 机器作用域，换号/断开均保留。

**验证**：`cargo check` 过（无新增 warning）；`cargo test multica::config` **8 测全过**（+3 新：`multica_account_changed_judges_by_email_with_safe_default` / `clear_multica_workspace_bindings_clears_bindings_keeps_credentials_and_daemon_id` / `clear_multica_state_indices_empties_all_three_account_scoped_indices`；既有 `clear_multica_session_*` 签名不变零回归）。无前端 / 无 webank server 改动。

---

### 12.19 改动十七：合并 origin/main 进 feature_multica（M5-ag，2026-08-12）

**背景**：feature_multica 落后 origin/main 115 commit（main 引入了 git/github 源代码管理、定时任务 scheduled-tasks、app-exit 协调器重构、agent catalog 等大量能力）；feature_multica 领先 9 commit（multica 远程任务接入）。用户要求合并 main 最新代码，冲突优先保 main，再考虑 multica 二次修复。

**安全网**：合并前在 `8fe70d9` 打备份分支 `feature_multica_premerge_backup`。

**冲突解决（11 文件全部「并集」解决——main 新能力与 multica 接入正交：定时任务 vs 远程任务，互不侵入）**：
- 纯加法并集（main 全量 + multica 增量共存）：
  - `src-tauri/Cargo.toml`：tokio features 取并集（补 `net`，main 的 macros/rt-multi-thread/sync/test-util）；保留 multica 依赖的 `thiserror = "1"`（`multica/error.rs` 用）。
  - `src-tauri/src/main.rs`：命令 import 列表并集（main 全量 + 回插 `connect_multica`/`disconnect_multica`/`get_multica_settings`，`save_multica_settings` 已在公共尾）；`multica::commands` 块与 main 的 `notifications::send_scheduled_native_notification` 共存；invoke_handler / managed state / start_multica_loop 均自动合并保留。
  - `src-tauri/src/commands.rs`：config import 并集（main 的 `DEFAULT_CUSTOM_AGENT_ICON` + multica 的 `MulticaAccountRef`）。
  - `src/config/mod.rs`：`apply_settings` 两段并集（multica 字段段 + main 的 scheduled_keep_awake/completion_notifications/occurrence_retention 段）；测试 import 并集。
  - `web/src/api/desktop.ts`：types import 并集（main 的 AppExit/Git/Scheduled* + multica 的 Multica*）。
  - `web/src/App.tsx`：subscribe / page import 并集（`subscribeMulticaTaskUpdates`+`subscribeScheduledTaskUpdates`；`MulticaTaskManagementPage`+`ScheduledTaskManagementPage`+`ScheduledTaskDetailPage`）。
  - `web/src/i18n.ts`：侧栏标签取 main 简化版（Agent / 上下文 / 运行模式）+ 保留 multica 的 `multicaTaskManagement` key（中英双版本同步）。
  - `web/src/routes.ts`：路由并集（`multica-tasks` 路由 + main 的 `scheduled-tasks`/`-create`/`-detail` 路由块；`conversation-run` 路径取 main 版以支持 roundId/attemptId——类型已要求）。
- 唯一语义融合（非冲突、非丢功能）：
  - `web/src/components/conversation/ConversationComposer.tsx`：`onSubmit` 签名并集（保留 multica 第二参 `multica?` + main 的 `onCreateScheduledTask?`）；`canSubmit` 条件融合（main 的 scheduled 机制全保留 + multica 的 `!(multicaActive && !hasLocalWorkspaces)` 门并到同一 `canSubmit`）。multica 绑定预填流（draft.multica / onSubmit 转发）未被 main 重构触碰，零回归。
  - `web/src/components/conversation/ConversationSidebar.tsx`：图标 import 并集（main 去掉的 Boxes/Workflow 不回加——已无引用；保留 multica 的 Globe + main 的 Library/Route/AlarmClock）；两个 SidebarButton 并存（multica 远程任务 + scheduled 定时任务）。
  - `web/src/pages/ConversationHomePage.tsx`：`onSubmit` 签名并集（同 Composer）。

**唯一合并诱发的代码修复（1 行）**：main 把侧栏导航从「`active.kind` 直查」重构为「`activeNavigationKey`（字符串 key）」系统，其 `conversationSidebarNavigationKey` switch 未覆盖合并后并入的 `multica-tasks` kind → TS2366。修法：switch 的 null 返回组补 `case 'multica-tasks'`（multica 按钮仍用 `active.kind === 'multica-tasks'` 直查高亮，nav key 返回 null 正确——该按钮不参与 key 系统）。

**main 新增 npm 依赖（package.json 自动合并，需安装）**：`@tomplum/react-git-log`、`@js-temporal/polyfill`、`cron-parser`、`@vvo/tzdb`——`npm install` 已装。

**验证**：`cargo check --all-targets` 绿（仅 main 既有的 dead-code warning：`scheduled_task_vms_from_sources`/`task_uuid`/`title`/`cancel_main_window_close` 等未用，非 multica、非本次引入）；`tsc -p tsconfig.build.json`（src only）绿、零错；`cargo test multica` **85 测全过、0 失败**（含 M5-af 三新测，零回归）。

**遗留（非 multica、非本次合并引入，main 既有技术债）**：`tsc` 全量（含 tests/）50 错，全在 `tests/` 目录、0 处引用 multica——是 main 的 VM 类型演化快于测试 fixture（`PreferencesVm.avatars` / `AppInfoVm.feedbackEnabled` / `AppConfigVm.workspaceFiles` / `ManagedAgentVm.command/args/env` 缺字段，以及 `node:fs`/`__dirname` 节点类型配置缺）。未在本次合并处理（属 main 测试维护债）。

**集成接缝备忘（供后续 multica 侧栏工作参考）**：multica 侧栏按钮用 `active.kind === 'multica-tasks'` 直查，main 体系用 `activeNavigationKey === '<key>'`。当前两者并存无 bug；若后续统一侧栏导航模型，应把 multica-tasks 纳入 `activeNavigationKey` 体系（届时移除此处 case，改用 key 比对）。

**结论**：用户预设的「multica 二次修复开发」基本不需要——并集解冲突使 main 全量能力与 multica 全量接入共存，multica 模块/命令/前端页面/85 单测全部零回归。

---

### 12.20 改动十八：远程任务页评审改版——去设置页 + 工作空间下拉 + 4 列看板（M5-ah，2026-08-12）

**背景（评审反馈，3 条 UX 调整）**：multica 接入评审后用户要求：①设置页不再暴露 multica 配置（实际使用只需初次连接，配置项走渠道默认）；②远程任务页加「工作空间下拉」，选定后只看该空间的任务（不再全部铺开）；③任务展示从「工作空间折叠分组列表」改为按状态的 4 列竖向看板：待办 / 进行中 / 已完成 / 失败。

**数据层无变更（纯前端重构）**：`RemoteConversationSidebarVm.tasksByWorkspace` 已按工作空间分组；`RemoteTaskVm.status` 恰为 4 个 canonical 值（`queued`/`running`/`completed`/`failed`，见 `vm.rs normalize_remote_status`），与看板列 1:1；`get_multica_tasks` 一次拉全部绑定工作空间。本次仅前端重组 + 连带死路径清理。

**用户决策（AskUserQuestion 确认）**：账号操作（切换/断开/添加/移除工作空间）集中到远程任务页头部的账号菜单（设置页不再暴露）；账号级失败任务（pinned，无工作空间、retryable）不再单独展示（失败列只显工作空间内失败任务）；来源下拉框（`REMOTE_TASK_SOURCES`，为未来多来源保留）保留。

**关键推论（pinned 不展示 → rerun 死路径）**：`retryable=true` 只有 pinned 任务（`from_failed_issue`）；所有工作空间内任务（`from_active_run`/`from_pending`/`from_claimed`/`from_completed`）`retryable=false`。pinned 不展示后 → 展示中的任务没有可重试的 → rerun 按钮永不渲染。按 dev-stage 破坏式清理一并移除 rerun 全链路，看板卡片动作与列 1:1：待办(queued)→认领执行(claim)、进行中(running)→取消(cancel)、已完成/失败(completed/failed，带本地 run 链接)→点击回看会话(selectRun)。

**实现**：
- **去设置页**：删 `SettingsPage` 的 multica section（import + JSX）+ 删 `MulticaSettingsBlock.tsx`；i18n 删 `settings.multica.*` 整块。配置项（baseUrl/appUrl/defaultProvider/enabled）不再有 UI，后端 `SettingsConfig` 字段 + 渠道默认逻辑保留（connect 仍依赖）。
- **页升级为容器**：`MulticaTaskManagementPage` 从薄壳扩为容器，承接原 `MulticaRemoteTaskList` 的数据/订阅/动作（mount 拉 `getMulticaTasks`+`getMulticaSettings`，订阅 task/settings 事件 re-fetch）。页头：来源下拉 + 工作空间下拉（选定值=active workspace，持久化 `setActiveMulticaWorkspace`，默认 `lastActiveWorkspaceId ?? workspaces[0]`）+ 账号菜单（shadcn DropdownMenu：切换账号/断开/添加/移除工作空间）+ 刷新 + 返回。体按连接/绑定态分流：未连接→连接空态、无工作空间→添加引导、否则→4 列看板（仅渲染 `tasksByWorkspace[effectiveWorkspaceId]`）。
- **看板（presentational）**：新建 `MulticaRemoteTaskBoard.tsx`（原 `MulticaRemoteTaskList.tsx` 删除，数据逻辑上移到页）。纯函数 `bucketTasksByStatus(tasks)→Record<'queued'|'running'|'completed'|'failed', RemoteTaskVm[]>`（未知 status 兜底丢弃）；布局 `grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4`（页级垂直滚动，列 `min-w-0`）；卡片包 shadcn Card（title + 状态 Badge + 时间 + claim/cancel/selectRun，clickability 规则 `projectId && localTaskId && runId` 不变）。
- **死路径清理（前后端，dev-stage 破坏式）**：`save_multica_settings`（配置表单已无消费方）+ `rerun_multica_task`（pinned 不展示后无 retryable 任务）全链路删除——前端 `api.{ts,/desktop,/browser,/client}` + 后端 `commands.rs`（save）/`multica/commands.rs`（rerun）/`multica/client.rs`（`rerun_issue` 唯一消费者即 rerun_multica_task，一并删 + 删其单测）+ `main.rs` import/invoke_handler。保留 `connectMultica`/`disconnectMultica`/`setActiveMulticaWorkspace`/`removeMulticaWorkspace`/`getMulticaSettings`/`getMulticaTasks`（页消费）+ `MulticaSettingsVm` 类型。

**验收**：`cargo test multica` **84 测全过、0 失败**（删 `rerun_issue_request_shape_is_workspace_scoped` 1 测，零回归）；`tsc -p tsconfig.build.json`（src only）绿零错；vitest 新增 `multica-remote-task-board`（`bucketTasksByStatus` 4 不变量 + 配色结构 + 渲染/claim/cancel/selectRun/时间本地时区）+ `multica-task-management-page`（容器：挂载拉取 + 双订阅 + 工作空间默认过滤 + 下拉切换持久化 + claim 预填/cancel/手动刷新）共 **21 测全过**；旧 `multica-remote-task-list`/`multica-settings-block` 两测随组件删除。

**遗留（非 multica、非本次引入）**：`scheduled-task-i18n.test` 1 失败——`ConversationComposer.tsx` 含中文注释触发其 `/[㐀-鿿]/` 扫描（该文件不在本次 diff，HEAD 即红，main 既有技术债）。

### 12.21 改动十九：远程任务页 UX 再打磨——工作空间 Popover picker + 页头对齐 + 底部工具条 source 门控（M5-ai，2026-08-12）

**背景（用户 3 条调整要求）**：M5-ah 落地的远程任务管理页有 3 处打磨调整：①添加/删除工作空间应放到工作空间下拉框内（而非散落在账号菜单中）；②页头风格需与定时任务/运行模式管理页一致（`variant="integrated"`，无分割线、无副标题、无返回按钮）；③工作空间下拉选框移到底部工具条，且仅在 multica 来源下显示（未来来源未必有工作空间概念）；账号下拉选框同理（PAT/切换账号/断开连接均为 multica 专属）。

**分析（账号菜单是否同理）**：是。账号菜单只含切换账号/断开连接，两者都基于 multica PAT 认证，新来源未必有 PAT/账号概念。两者与工作空间 picker 一起移到底部工具条，受 `source === 'multica' && connected` 双重门控——连接前不显示、非 multica 来源不显示。

**实现（纯前端，无 Rust/server 改动）**：
- **工作空间 Popover picker**：shadcn `Popover` + `PopoverTrigger`（Button outline + Folders 图标 + 当前工作空间名 + ChevronDown）→ `PopoverContent`（`w-[260px] p-0`），内含 **添加工作空间** 幽灵按钮（Plus 图标 + 文案，点击关闭 picker 并打开添加弹窗）+ Separator + 可滚动列表（`max-h-64 overflow-auto p-1`），每行 = 选中按钮（`data-testid="ws-pick-{id}"`，选定行 `bg-accent text-accent-foreground`）+ 垃圾箱移除按钮（`data-testid="ws-remove-{id}"`，Trash2 图标，aria-label 移除文案）。移除走 AlertDialog 确认（对齐定时任务 delete 模式 + ui-interaction §1）。
- **页头对齐**：`PageHeader variant="integrated"` + `icon={<Globe />}` + `title`（无 subtitle、无 actions slot、无返回按钮——会话模式 ConversationSidebar 已提供持久侧栏导航，返回按钮冗余）。
- **底部工具条（footer）**：`shrink-0 border-t border-border/60 bg-background/60 px-5 py-3 backdrop-blur xl:px-6`，渲染条件 `source === 'multica' && connected`。左区：来源 Select + 工作空间 Popover picker（仅 `hasWorkspaces` 时渲染）；右区：账号 DropdownMenu（切换账号/断开连接）+ 刷新 Tooltip 按钮。
- **账号菜单简化**：删添加/移除工作空间项（已迁入 workspace picker），仅保留切换账号（disabled 无 `multicaAppUrl` 时）和断开连接（`text-destructive`）。
- **`effectiveWorkspaceId` 健壮化**：`lastActive` 校验 `workspaces.some(w => w.id === lastActive)`，防止移除活跃空间后回退到已失效 id（state-lifecycle §4）。
- **Props 删除 `onBack`**：页不再需要返回按钮，`App.tsx` 对应调用点删除 `onBack` prop。

**文件**：`web/src/pages/MulticaTaskManagementPage.tsx`（重写 footer/header/workspace-picker）、`web/src/App.tsx`（删 onBack）、`web/src/i18n.ts`（删 subtitle + account.removeWorkspace/removeConfirm → 迁至 workspace.remove/removeConfirm）、`web/tests/multica-task-management-page.test.tsx`（更新全量：加 Popover/AlertDialog/Separator mock + Globe/Folders/Plus/Trash2 图标 + 删 onBack + 工作空间切换改 Popover 交互 + 行级移除→AlertDialog 确认 + 底部工具条 source 门控）。

**验收**：`tsc -p tsconfig.build.json`（src only）绿零错；vitest multica-task-management-page **11 测全过**（含 workspace-switch→Popover 交互、remove→AlertDialog confirm、source-gate 共 3 新/改写测试）+ board 12 测仍绿；`cargo test multica` 84 测 0 败（无 Rust 变更零回归）。

---

### 12.22 改动二十：远程任务页来源上移页头 + 会话-任务绑定可视化（独立 chip）（M5-aj，2026-08-12）

**背景（用户 2 条调整要求）**：①远程任务页「任务来源」下拉框放底部工具条不协调，应单独上移到界面上部；②点击远程任务「执行」进入会话初始输入框编辑时，一旦点别处（跳转/切页）这条会话与任务的绑定就"丢失"了——希望预填内容带一个 multica 标签把会话与任务绑定，且用户可删掉该标签解除绑定。

**根因分析（point 2，先定数据再下结论）**：核实绑定链路后发现绑定本身**并未丢失**，缺陷是**不可见 + 不可控**：
- `ConversationComposerDraftBoundary`（App.tsx）把草稿 owner 状态上提到 Shell 之上，`draft.multica` 绑定随 owner 状态存活，跨 in-app 导航（卸载/重挂 composer）天然保留。
- 服务端 prepare lease（45s TTL）由**全局**心跳 `extend_prepare_leases`（loop_.rs）每 tick 续期，与当前显示哪一页无关，compose 期间不会因切页被回收。
- 即绑定与 lease **都已跨页持久**。用户感知的"丢失"实为：绑定是纯隐式状态，UI 无任何指示，用户既看不到"此会话已绑某任务"，也无法主动解除——属"好设计、实现不完善"（CLAUDE.md），非根本性设计缺陷。

**方案（point 2）**：用**独立 chip**而非用户字面的"正文开头内嵌文本标签"。内嵌文本标签脆弱：正文可被随意编辑导致标记残缺、发送时需字符串解析还原绑定、删除键解除不可靠。独立 chip 是行业成熟范式（mention/recipient/tag），状态与正文解耦、可见可控、删除语义明确。用户已确认（"同意独立 chip"）。

**实现（纯前端，无 Rust/server 改动）**：
- **绑定结构扩 `title`**：`ConversationComposerMulticaBinding` 增 `title: string`（仅供 chip 展示，注释明示"不参与发送寻址"——发送寻址仍只看 `remoteTaskId` + composer 下拉的 `projectId`）。`handleClaimAndPrepare` 的 `prefill` 调用补传 `title: task.title`。
- **reducer 增 `clearMultica`**：`clearMultica` 仅置 `multica = null`，**保留正文与附件**——解绑后草稿降级为普通本地会话（发送走 `create_conversation_run`）。无绑定时 no-op（返回同一引用，setContent 同款稳定引用语义）。owner hook 暴露 `clearMultica()`；boundary handle 仍只对 App 暴露 `reset`（clearMultica 是 composer 局部动作，不上提）。
- **chip 渲染**：在 `PromptInput` 内作为**首子节点**（block-flow，位于 SlashCommandMenu 之前、输入区最上方）。`Badge`（`border-primary/30 bg-primary/10 text-primary`）+ Globe 图标 + `multicaBindingTag`（"Multica · {title}"，title 截断 `max-w-[260px]`）+ × 关闭按钮（aria-label `removeMulticaBinding`）。
- **解除绑定两条路径**（都走同一 handler `handleUnbindMultica`）：①点击 chip × 按钮；②Backspace 且 `multicaActive && visibleContent.trim() === '' && !committedSlashCommand`（正文为空才允许退格删 chip，避免误删用户正文；`visibleContent = committedSlashCommand?.suffix ?? content`）。handler 调 `cancelMulticaPrepareLease(remoteTaskId)`（释放服务端 lease，幂等）+ `clearMultica()`（清本地绑定）。lease 取消 best-effort，失败静默（lease 自身有 TTL 兜底）。
- **point 1 来源上移**：`REMOTE_TASK_SOURCES` 的 `<Select>` 从 footer 左区移到 `PageHeader` 的 `actions` 槽（+ 来源 label），与定时任务管理页 header actions 同构。footer 注释更新为"来源已上移页头；此处仅 multica 专属控件"。footer 渲染条件不变（`source === 'multica' && connected`），内容只剩工作空间 picker + 账号菜单 + 刷新。

**文件**：`web/src/lib/conversation-composer-draft.ts`（binding +title、`clearMultica` action/owner hook）、`web/src/pages/MulticaTaskManagementPage.tsx`（prefill +title、来源 Select 迁入 PageHeader actions、footer 删 source 块）、`web/src/components/conversation/ConversationComposer.tsx`（chip 渲染 + `handleUnbindMultica` + Backspace 解绑分支）、`web/src/i18n.ts`（zh/en 各加 `multicaBindingTag` + `removeMulticaBinding`）、`web/tests/conversation-composer-draft.test.ts`（binding 字面量补 title + 新增 clearMultica 2 测）、`web/tests/multica-task-management-page.test.tsx`（PageHeader mock 渲染 actions + source-gate 测改写为"来源常驻 header / footer 按刷新按钮门控" + claim 测断言补 title）。

**验收**：`tsc -p tsconfig.build.json`（src only）绿零错；vitest `conversation-composer-draft` **16 测全过**（含 clearMultica 保留正文/附件、无绑定 no-op 2 新测）+ `multica-task-management-page` **11 测全过**（source-gate 改写 + claim 断言补 title）+ board 12 测仍绿，共 **39 测**；`cargo test multica` 84 测 0 败（无 Rust 变更零回归）。chip 渲染/handler 接线由 tsc 校验——home `ConversationComposer` 过重不挂组件测，与既有"纯逻辑 reducer 测 + 容器测"套件策略一致。

---

### 12.23 改动二十一：claim-at-send 重构——点「发送」才 claim+start，移除 prepare-lease（M5-ak，2026-08-12）

**背景（用户根因反馈）**：用户报告「删掉 multica 绑定 chip 后再次点『执行』报『找不到该 Multica 任务』」。用户意图（原文）：不管删不删 chip，远程任务都应仍是待办；只有点「发送」才开始执行并更新服务端状态；删 chip 只是把当前会话与远程任务解绑、降级为普通会话。

**根因（claim-at-click 的设计缺陷，非实现 bug）**：M5-q 起的 claim-at-click 把 claim（领取）时机放在「点执行」——点 play 即 `claim_specific_task`（pending→dispatched，含 45s prepare lease），compose 期间靠常驻心跳 `extend_prepare_lease` 续期。这套机制有两个根本问题：
1. **解绑后任务被锁死**：点执行 → 任务 dispatched；即便删 chip（`cancel_multica_prepare_lease` 释放 lease 把任务 CAS 回 queued），在 45s lease 窗口内任务仍是「已被本 runtime 领取」状态；用户再次点执行时 claim 命中 server 串行化守卫或 lease 未过期分支 → 返回 404 → 前端报「找不到该任务」。即「领取」这一有状态副作用被错误地前置到了无状态的「查看/预填」动作上。
2. **整套 prepare-lease 续期机制是为这个错误时机服务的**：state.rs 的 `prepare_leases` HashMap、loop_.rs 的 `extend_prepare_leases`、client.rs 的 `extend_prepare_lease`、commands.rs 的 `cancel_multica_prepare_lease`——四文件能力只为「claim 与 start 之间的 compose 窗口」续命。时机一旦纠正（claim 推迟到与 start 同一事务），这整套机制即是死代码。

**方案（纠正时机，而非补丁）**：把 claim 的有状态副作用从「点执行」推迟到「点发送」，与 start 合并成单一事务边界。点执行降级为**只读**——只取需求正文预填 composer、记本地绑定，服务端任务状态不变（仍 queued）。删 chip 因不涉及任何服务端领取，纯属本地解绑。整套 prepare-lease 续期机制随之移除（根因修复的连带收益，非单独诉求）。

**实现**：
- **只读需求端点（webank server）**：新增 `GET /api/daemon/runtimes/{rid}/tasks/{tid}`（裸 `AgentTaskResponse` → `RemoteTask`，不触发 claim、不回填 claim-only 富字段 skills / mcp overlay / `prior_session_id` / connected_apps / repos / 用户上下文等——那些在发送时由 claim 取回）。码灵 client `get_task_requirement`(client.rs:663) + 命令 `get_multica_task_requirement`(commands.rs:455)：runtime_id 取自该 workspace 启动注册缓存（未注册 → runtime-offline），GET 拉详情 → `RemoteTaskVm::from_detail`。
- **claim-at-send（commands.rs `start_multica_conversation_run`:555）**：发送即事务边界——`claim_specific_task`(pending→dispatched) → 复用本地 `create_conversation_run_vm`（建工作流/任务/写 conversation.json/拷附件/起 run，与本地「+」同一链路）→ `start_task`(dispatched→running)。用户在 composer 已选好模型/模式。
- **claim 失败回滚端点（webank server + client `release_task`:707）**：claim 成功但本地起 run 失败（workspace 校验 / 模型校验 / 本地建 run / start_task 任一步）会让任务卡在 dispatched（无本地 run、无心跳）。新增 `POST /api/daemon/runtimes/{rid}/tasks/{tid}/release`，暴露 server 的 `RequeueTaskAfterClaimFailure`（CAS dispatched→queued）做事务性回滚。`release_after_run_start_failure`(commands.rs:492) 在 `start_multica_conversation_run` 的 5 个失败点调用（workspace-not-found / validation-failed / resume start_task 失败 / fresh start_task 失败 / fresh create_run 失败）；best-effort、不重试，server 侧对「已非 dispatched」幂等返回 200、真正的 404 映射 TaskNotFound 调用方忽略。替代旧 prepare-lease「45s 自然过期」兜底。
- **prepare-lease 全量移除**：state.rs 删 `prepare_leases` HashMap + `PrepareLease` 结构 + 4 方法（register/drop/snapshot/get）；loop_.rs 删 `extend_prepare_leases` 调用与函数（tick 收缩为「自愈注册 + 心跳 + 取消检测」三段）；client.rs 删 `extend_prepare_lease`；commands.rs 删 `cancel_multica_prepare_lease`。`runtime_ids` + `active_runs` + resume-on-restart 逻辑完整保留（不与 prepare-lease 耦合）。`auth_token` / `prior_session_id` 当前 claimed 但无消费方——本期保留现状、不扩范围。
- **前端 claim-at-send wiring**：api `getMulticaTaskRequirement`（只读）替换 `claimMulticaTask`；`cancelMulticaPrepareLease` 移除（删 chip 纯本地）；新增 `startMulticaConversationRun(input, remoteTaskId, workspaceId)`。看板 prop `onClaim`→`onPrepare`（点击是 prepare/预填而非 claim）；页 `handleClaimAndPrepare`→`handlePrepareRemoteTask`（调 `getMulticaTaskRequirement` 取正文 → prefill + `onPrepareMulticaTask`，任务仍 queued）；`ConversationComposer.handleUnbindMultica` 只调 `clearMultica`（不再触服务端）。
- **发送失败不清 chip（刻意决策）**：`startMulticaConversationRun` 失败时前端**不**清 multica chip——服务端已 release（任务回 queued，可重试），chip 仍有效；而失败可能是 workspace/git 等本地瞬态错误，清 chip 反而误导用户以为任务已弃。App.tsx 发送路径无需改动。
- **Issue 2（runtime 展示名）**：multica runtime 的 `name` 字段原先传 provider（如 "claude-acp"），看板展示成 provider 而非客户端名。改用 `channel::current_channel_config().app_name`（默认 "Gold Band"）——`name`（客户端展示名）与 `runtime_type`（provider 路由键）分离（loop_.rs:63 `register_workspace` + client.rs:1141 测试断言）。

**文件**：
- webank server（`E:\MercurjiangWorkSpace\IdeaProjects\multica-webank`）：新增只读 GET 任务详情端点 + release 端点（rebuild + 重启生效）。
- Rust：`src-tauri/src/multica/{client.rs, commands.rs, state.rs, loop_.rs, mod.rs, main.rs, error.rs}`。
- 前端：`web/src/api/{client.ts, desktop.ts, browser.ts, .}`, `web/src/pages/MulticaTaskManagementPage.tsx`、`web/src/components/conversation/{MulticaRemoteTaskBoard.tsx, ConversationComposer.tsx}`、`web/src/lib/conversation-composer-draft.ts`、`web/tests/multica-{task-management-page,remote-task-board}.test.tsx`。

**验收**：`cargo check` 绿（零新增 warning；state.rs `#![allow(dead_code)]` 因既有待消费字段 `ActiveRemoteRun.runtime_id` 保留并附准确注释）；`cargo test multica` 全过（prepare-lease 3 测随实现删除；新增 claim-at-send / release / get_requirement / app_name 测）；web `tsc` 零错；vitest `multica-task-management-page` + `multica-remote-task-board` 共 23 测全过（onClaim→onPrepare、`getMulticaTaskRequirement` 只读断言）。**需 rebuild + 重启 webank server**（只读 + release 两端点为 server 改动）。agent-browser 端到端 deep-link 验证待跑。

> 本节**取代** §12.22（M5-aj）中「删 chip → `cancelMulticaPrepareLease` 释放 lease」的描述，以及更早 M5-q 的 claim-at-click + prepare-lease 续期机制——时机纠正后该机制整体废弃。§2.6 执行流程图与接入方案 §3 仍保留 claim-at-click 原始设计记录，以 `⚠️` 指向本节为准。

---

### 12.24 改动二十二：🔴 running 任务永久卡 running——start 响应丢失消歧 + 手动取消远端终态上报（审计 #75，M5-al，2026-08-13）

**背景（审计复盘）**：claim-at-send（§12.23）落地后回溯 multica 全链路，发现 webank 对 **running 任务无逐任务 liveness**——`FailStaleTasks` 的 running 分支要求 daemon 非 online 才兜底，只要码灵存活并在心跳（哪怕为别的任务），一个 remote running 任务一旦失去对应本地 run 就会**永久卡 running**。两条码灵侧路径制造这种孤儿：

1. **`start_task` 响应丢失**：`start_task` 的 HTTP 响应可能在传输层丢失——server 侧 `dispatched→running` 已落库，码灵却拿到 `NetworkFailed`。旧实现无脑 `release_task` 回滚：但 `release` 对 **running** 是 no-op（只 CAS `dispatched→queued`），任务留在 running、本地 run 却被 teardown → 永久孤儿。
2. **手动取消未上报远端**：`cancel_multica_task` 旧实现只做本地 teardown（`run_pause` + 杀 ACP + 清索引），**不通知 server**——remote running 任务无人终态化 → 永久孤儿。

**方案（消歧决策表 + 双通道收尾，杜绝孤儿）**：
- **start 失败消歧（纯函数）**：新增 `decide_start_failure_action(status: Option<&str>) -> StartFailureAction`，`start_task` 失败时先 `get_task_status` 查询实际状态再决策：`Some("running")`→`Continue`（start 实际成功、响应丢失，本地 run 与 server 都 running、一致 → 继续执行，续跑进度不浪费，不 teardown）/ `Some(其他)`→`RollbackRelease`（start 未生效或已终态 → release 回滚，对 dispatched 正确 CAS、对终态幂等 no-op + 本地 teardown）/ `None`（查询失败、无法确认）→`Terminate`（**不能 release**，可能已 running 是 no-op → `fail_task(reason=timeout)` resume-safe 可 auto-retry，唯一能确定终结 running 的动作 + 本地 teardown）。新增 `fail_after_run_start_failure`（与 `release_after_run_start_failure` 对称的 best-effort 上报）。两处 `start_task` 失败点（续跑分支 + fresh 分支）统一走该决策。
- **手动取消双通道**：`cancel_multica_task` 增**通道 1**——best-effort `fail_task("cancelled by user (manual cancel)", "agent_error")` 把 remote running 终态化为 failed。bare `agent_error` 不在 webank `retryableReasons` → **不可重试**（用户主动取消不应被自动 requeue）。原本地 teardown 收编为**通道 2**。两通道职责分离：远端终态由通道 1 负责，本地 `run_pause→Paused` 事件不复用为远端上报（bridge 对 Paused 本就不上报）。

**契约澄清（client.rs 注释，无行为变更）**：`release_task` 文档补明——主动 release 是 claim-at-send 失败的**首选恢复**；server 仍写 `prepare_lease_expires_at=now+45s`（码灵不续约）+ `FailStaleTasks`（`dispatched_at+300s`）/ `FailTasksForOfflineRuntimes` 作**被动兜底**。码灵不续 lease 故 lease 兜底实际不可靠——显式 release 才是正确契约。

**文件**：`src-tauri/src/multica/commands.rs`（`StartFailureAction` 枚举 + `decide_start_failure_action` 纯函数 + `fail_after_run_start_failure` + 两处 start 失败分支改走决策 + `cancel_multica_task` 通道1 + 4 单测）、`src-tauri/src/multica/client.rs`（`release_task` 文档）。

**验收**：`cargo test -p gold-band-desktop multica::` 全过（含 4 新决策测：continue-when-running / rollback-when-other-non-running / terminate-when-unknown / status-sensitive-not-truthy）。

---

### 12.25 改动二十三：🟠 pinned_tasks / retryable 死管线删除（审计 #76 / M1，M5-am，2026-08-13）

**背景（审计复盘）**：§12.20（M5-ah）评审改版后，pinned（账号级失败回显）任务不再展示——失败列只显工作空间内失败任务。回溯发现 `retryable=true` **只**有 pinned 任务（`from_failed_issue`），所有工作空间内任务 `retryable=false`；pinned 不展示 → 展示中无任何可重试任务 → rerun 按钮永不渲染（rerun 命令已在 §12.20 删除）。但 **VM 的 `retryable` 字段、`pinned_tasks` 字段、`from_failed_issue` 构造、StateConfig 的 `multica_pending_issues` 字段、bridge `finalize_terminal` 的 pending 处置逻辑**整条死管线仍残留（rerun 命令虽删，其数据源 / VM 映射未清）。

**方案（dev-stage 破坏式全链路删除，无兼容层）**：
- **StateConfig**：删 `multica_pending_issues: Option<Vec<String>>`（`src/config/mod.rs`）。`StateConfig` 无 `deny_unknown_fields` → 旧磁盘残留该键反序列化静默忽略，迁移安全。
- **VM**：删 `RemoteTaskVm.retryable` + `RemoteConversationSidebarVm.pinned_tasks` + `from_failed_issue` 构造；`from_remote` 去掉 `retryable` 形参；4 构造点 + 全部 VM 单测同步。前端 `web/src/types.ts` 两 interface 同步删字段、`web/src/api/browser.ts` mock fixture 同步。
- **bridge `finalize_terminal`**：原 `PendingUpdate` 枚举语义 = 「失败回显（pending_issues）增删」（Success 清除 / Failure 记录）。pending_issues 删除后枚举重定义为 **completed 历史快照的 status 选择**（`ClearOnSuccess`→`"completed"` / `AddOnFailure`→`"failed"`）——同一枚举、语义随数据结构下沉而迁移，不增新类型。`multica/config.rs::clear_multica_state_indices` 同步删 `multica_pending_issues` 清理（三索引→两索引：`task_conversations` + `completed_tasks`），换号/断开作废覆盖随之收窄。
- **commands `get_multica_tasks`**：删 pinned 组装（`from_failed_issue` 映射 + `pinned_tasks` 出参），sidebar 只剩 `workspaces` + `tasks_by_workspace`（终态行已在 §12.9 归入对应工作空间组）。
- **注释**：top-level `commands.rs` 换号检测注释「三索引」→「两索引」、泄漏示例从「失败 issue 进置顶列表」改为「续跑索引/完成历史串号」。

**文件**：`src/config/mod.rs`（字段删）、`src-tauri/src/multica/{config.rs, vm.rs, commands.rs, bridge.rs}`（死管线清除 + 注释 + 单测）、`src-tauri/src/commands.rs`（注释）、`web/src/{types.ts, api/browser.ts}`（类型 + mock）。

**验收**：`cargo test -p gold-band-desktop multica::` 全过（`from_failed_issue` / `clear_..._three` 测随实现删除/改名，零回归）；前端 `tsc` + vitest 绿（board/page 测断言 retryable / pinnedTasks 已从序列化结果消失）。

---

### 12.26 改动二十四：🟠 远程任务页订阅竞态 + 事件去重（审计 #77 / M2，M5-an，2026-08-13）

**背景（审计复盘）**：`MulticaTaskManagementPage` 原订阅 effect 三缺陷：①**订阅竞态**——Tauri `listen` 异步 resolve，组件在 resolve 前卸载时 cleanup 抢跑，listener 泄漏（卸载后仍触发刷新 / setState on dead component）；②**fetch storm**——事件爆发期（多个 task 状态变更）每个事件独立触发 `refreshAll`，并发请求风暴；③**effect 依赖耦合**——`useEffect(..., [refreshAll])`，refreshAll 引用变即重订阅，放大泄漏窗口。`App.tsx` 侧栏订阅有同源 ①②。

**方案（抽通用 `useEventDrivenRefresh` hook，根治三缺陷）**：
- **in-flight + pending 合并**：事件触发时若已有刷新在飞，仅置 `pending=true` 不新发；在飞刷新结束后若 pending 则**拖尾重跑一次**（最多 1 in-flight + 1 pending）。6 个并发事件 → 2 次刷新（非 6 次），消除风暴。
- **async-unsubscribe-race 处理**：`active` 标志 + resolve 时若已 inactive 则**立即 dispose**（不进 disposes 数组），杜绝「resolve 晚于 unmount」的 listener 泄漏；listener 内 `active` 守卫拦截卸载后刷新。
- **ref 解耦**：`refresh` / `subscribeFns` / `refreshOnMount` 全存 ref、effect 依赖 `[]`——订阅**只注册一次**，最新回调从 ref 读，避免重订阅放大竞态窗口。`undefined` 通道过滤（browser client 省略 desktop-only 订阅）。失败 best-effort（吞 reject、不阻断后续 pending 消费）。

**实现**：新建 `web/src/lib/use-event-driven-refresh.ts`（~50 行，零依赖纯 React hook）。`MulticaTaskManagementPage` 换用该 hook（`refreshAll` 改返回 Promise、`refreshOnMount:true`、双通道订阅 task+settings）；`App.tsx` 侧栏订阅同步换用（原 40 行内联 effect → 一行 hook 调用）。

**文件**：`web/src/lib/use-event-driven-refresh.ts`（新）、`web/src/pages/MulticaTaskManagementPage.tsx`、`web/src/App.tsx`、`web/tests/use-event-driven-refresh.test.tsx`（新，10 测：refreshOnMount 开/关、并发合并 6→2、跨通道合并、每通道单 listener、卸载 dispose、unsubscribe-race、卸载后守卫、undefined 过滤、reject best-effort）。

**验收**：vitest `use-event-driven-refresh` 10 测全过；`multica-task-management-page` + `multica-remote-task-board` 回归绿；`tsc` 零错。

---

### 12.27 改动二十五：🟠 StateConfig 并发 RMW lost-update——App 层 with_state 原子原语（审计 #78 / M3，M5-ao，2026-08-13）

**背景（审计复盘）**：bridge 终态收尾（`finalize_terminal` / `handle_node_completed` / `teardown_active_run`）与 commands（`migrate_resume_index` / start-run 索引 upsert）经 `App::load_state() → mutate → save_state()` 三步操作 StateConfig（普通文件 I/O，**无锁**）。同一 remote task 的 `NodeCompleted`（bridge）与 `RunCompleted`（bridge）或启动 upsert（commands）并发时，两个 load-then-save 交错 → **后写覆盖前写（lost update）**：如终态清 `task_conversations[remote]` 与并发 pin 写回交错，终态清理被 pin 的旧快照覆盖 → 续跑索引残留脏数据。违反 state-lifecycle-and-data-integrity §6（RMW 须原子）。

**方案（App 层加原子 RMW 原语，最小临界区）**：
- **`App::with_state<F>(&self, update: F) -> Result<bool>`**：per-`repo_root` 分片 `Mutex`（32 shard，`DefaultHasher` 取模，镜像既有 `ATTEMPT_RUNTIME_STATE_LOCKS` 模式）——锁**仅**包住 `load_state → update → save_state` 的文件 RMW，**不含网络**（rule §6：临界区最小化、网络在锁外）。`update` 返回 `dirty: bool`，clean 则跳过 save（读改判不脏不写）。
- **5 处写点迁移**：bridge 3 处（`teardown_active_run` / `handle_node_completed` / `finalize_terminal`）+ commands 2 处（`migrate_resume_index` / start-run `task_conversations` upsert）全改 `with_state(|state| {...; true/false})`。pin / 终态的**网络上报（HTTP）在锁外**——`handle_node_completed` 先 `with_state` 落库 + 判 session 是否变，再据返回值决定是否发 `pin_task_session` HTTP；`finalize_terminal` 先 `with_state` 落 completed 历史，终态 HTTP 在调用方 `handle_run_completed`（锁外）。
- **诚实边界**：非 multica 的 StateConfig 写点（pin/unpin、workspace 等用户低并发操作）本期未迁移——`with_state` 作为**增量采用原语**，后续写点逐步迁入即可，不破坏现状。**（已关闭，见 §12.37/M5-ax：全部写点迁入 `with_state`，锁身份从 repo_root 改为 state 文件路径，`save_state` 降 `pub(crate)`。）**

**附带（非 multica，解锁验证）**：`src/config/mod.rs` 测试模块有 2 处死 import（`MANAGED_AGENT_PRESETS` / `managed_agent_preset`——符号早已不存在，仅 test `use` 残留；lib 测 crate 从未被编译故未暴露）。删除以解锁 `cargo test -p gold-band` 编译，使 `with_state` 单测可运行。

**文件**：`src/app/mod.rs`（`STATE_CONFIG_LOCKS` 分片 + `state_config_lock` + `with_state` + 4 单测：persists-mutation / skips-save-when-clean / reads-current-disk / serializes-concurrent-rmw 32 线程无 lost-update）、`src-tauri/src/multica/{bridge.rs, commands.rs}`（5 写点迁移）、`src/config/mod.rs`（死 import 清理）。

**验收**：`cargo test -p gold-band --lib with_state` 4 测全过（含 32 线程并发 RMW 终态 len==32、无 lost-update）；`cargo test -p gold-band-desktop multica::` 83 测全过（5 迁移点零回归）。

> §12.24–12.27 为 multica 全链路审计（#75–#78）回溯加固：#75（🔴 running 孤儿）纠正「server running 无逐任务 liveness」下的终态上报契约；#76（M1）清理 §12.20 起的 pinned/retryable 死管线；#77（M2）根治远程任务页订阅竞态 + 事件去重；#78（M3）补齐 StateConfig 并发 RMW 原子性。接入方案 §3 终态上报契约、§5.2 M3 数据模型（`multica_pending_issues`/`retryable`/`pinned_tasks` 已废）以本四节为准。

---

### 12.28 改动二十六：multica 绑定 chip 内嵌输入框（leading adornment + text-indent 让位）+ Backspace 删除条件放宽（M5-ap，2026-08-13）

**背景（用户 UX 调整）**：用户要求 multica 绑定 chip「不是放在输入框的上面，而是放在输入框里面，用户用删除键都可以直接删除的那种」；并明确「其余这个标签是符合预期的」（样式/文案/显示时机不变）。

**根因（好设计、实现可完善）**：§12.22（M5-aj）的独立 chip 方案（与正文解耦、可见可控、删除语义明确）本身正确，但两点实现可完善：① chip 作为 `PromptInput` 的首子节点（block-flow，输入区**上方独立行**），与正文分离，不像「正文的一部分」；② Backspace 删除要求**正文为空**（`visibleContent.trim() === ''`，避免误删正文），体感上「不够直接」——用户希望删 chip 像删正文最前一个字一样自然。

**方案（复用既有 leading adornment 机制 + 提取可测纯函数）**：
- **chip 内嵌（复用 slash 同款机制）**：chip 从 `PromptInput` 首子节点 block 行移进 textarea 的 `relative` 容器，作为正文最前的 leading adornment——绝对定位左上角（`absolute left-0 top-0 z-10`），正文首行经 `useLeadingAdornmentTextIndent`（`text-indent = chip 宽度 + 0.25rem`）缩进让位；CSS `text-indent` 只作用于首行，换行后正文回到左侧（chip 只占首行高度，不与后续行重叠）。chip 与 slash 命令标签互斥（**slash 优先**——绑定预填的是任务需求文本、非 slash 命令；⚠️ **此互斥已被 M5-bv（§12.64）取代**：`@role` + 远程绑定是合法发送组合，二者改为同槽并排显示），共用同一套 `leadingAdornmentLayout`（原仅服务 slash 的 `committedInputLayout` 重命名 + `enabled` 泛化为 `committedSlashCommand || multicaChipActive`）。× 关闭按钮保留（点 × 仍删 chip）。
- **Backspace 删除条件放宽**：从「正文为空」改为「光标停在正文最前且无选区（`selectionStart === 0 && selectionEnd === 0`）」——模拟「chip 是正文首个 token」：文本起点的 Backspace 本就是 no-op，劫持它删 chip；正文非空、光标在中间/末尾（`selectionStart > 0`）或存在选区（`selectionEnd !== 0`，即便起点为 0）时照常删字、不误伤 chip。slash 提交时让位 slash 控制器。
- **提取纯函数固化交互契约**：把 Backspace 触发条件从 `handleKeyDown` inline 提取为 `web/src/lib/conversation-composer-multica-chip.ts` 的 `shouldBackspaceClearMulticaBinding({key, multicaActive, hasCommittedSlashCommand, selectionStart, selectionEnd})`，单测覆盖 5 维输入分支。home `ConversationComposer` 依赖过重（draft context + agent commands + attachment + slash controller + right workspace）不挂组件测，与既有「纯逻辑 reducer 测 + 容器测」套件策略一致（§12.22 验收同款理由）。

**实现（纯前端，无 Rust/server 改动）**：
- `ConversationComposer.tsx`：删 `PromptInput` 首子节点 chip block；`relative min-w-0` 容器 ternary 改为 `committedSlashCommand ? <SlashTag/> : multicaBinding ? <chip Badge/> : null`，两 adornment 共用 `leadingAdornmentLayout.adornmentRef` + `.textareaStyle`；`handleKeyDown` 调 `shouldBackspaceClearMulticaBinding`（`selectionStart/End` 从 `composerTextareaRef.current` 取，ref 缺失时 `?? -1` 兜底为不删）。
- `conversation-composer-multica-chip.ts`（新）：`MulticaBindingBackspaceInput` + `shouldBackspaceClearMulticaBinding` 纯函数。
- `conversation-composer-multica-chip.test.ts`（新）：9 测——光标起点删（含正文非空）/ 非 Backspace / 无绑定 / 有 slash / 光标在中末 / 有选区 / ref 缺失兜底。

**文件**：`web/src/components/conversation/ConversationComposer.tsx`（chip 移入 relative 容器 + hook 重命名泛化 + Backspace 调纯函数）、`web/src/lib/conversation-composer-multica-chip.ts`（新）、`web/tests/conversation-composer-multica-chip.test.ts`（新）。

**验收**：`tsc --noEmit -p web/tsconfig.build.json` 零错；生产构建（`web:build` = tsc + vite build）绿；vitest `conversation-composer-multica-chip` 9 测 + composer/multica 回归 11 套件 102 测全过。

> 本节更新 §12.22（M5-aj）chip 渲染位置（`PromptInput` 首子节点 block 行 → 输入框内首行内嵌 leading adornment）与 Backspace 删除条件（正文为空 → 光标在正文起点且无选区）。chip 与正文解耦、× 按钮解绑、claim-at-send（§12.23）下删 chip 纯本地等设计不变。

---

### 12.29 改动二十七：multica dead_code 清理——移除 4 个模块级 `#![allow(dead_code)]`，分类 retire / 定点 allow（M5-aq，2026-08-13）

**背景**：M2–M5 分里程碑接入时，`multica/{error,state,bridge,client}.rs` 顶部各挂了临时 `#![allow(dead_code)]`（注释「M5 完成后审查移除」）。M5 + 26 项加固（M5-r…M5-ap）已全部落地，到了审查窗口：移除这 4 个模块级 allow，把暴露的 dead_code 逐项分类——真死的删、契约保留的改定点 `#[allow(dead_code)]` + 注释，杜绝模块级静默。

**分类与处置**：

*删除（真死代码，dev-stage 破坏式）*：
- `error.rs` `WorkspaceEmpty`：全链路零构造——空 workspace 由前端空态 UI 守卫（远程任务页「无工作空间」空态），后端从不 emit。删变体 + `code()` 臂 + 单测；i18n `workspace-empty`（zh/en）同步删。
- `error.rs` `PinSessionFailed(String)`：全链路零构造——`pin_task_session` 契约为 best-effort（失败仅 `warn!` 记日志、不阻断终态，client.rs:808），从不 raise。删变体 + `code()`/`params()` 臂 + 单测；i18n `pin-session-failed`（zh/en）同步删。
- `state.rs` `ActiveRemoteRun.runtime_id`：写入但零读取——心跳/自愈按 `MulticaRuntimeState.runtime_ids` map 寻址（`runtime_id_pairs()`），非此字段（原「心跳按它寻址」注释失真）。删字段 + 6 处构造点（commands.rs ×2、vm.rs 测试、state.rs `sample_run` + 4 调用方）；`sample_run` 同步去掉首参。
- `client.rs` `post_json_with_workspace`：零调用——唯一 issue 维度接口 `update_issue_status` 用 `json_send(PUT)` 直发；原注释提的 `rerun_issue`（接入方案 D1/D2/E1/E2 POST）随 M5-ah rerun 死链删除已不存在。删方法 + 清两处失效注释（`json_send` 共用底座 / 测试 path 注释里的 `rerun_issue` 残留）。

*保留（契约/决策性，改定点 allow + 注释）*：
- `error.rs` `SessionResumeFailed`：**M4-d 明确保留在码表**（`multica.session-resume-failed`）——resume 路径不 emit/不匹配，任何 resume Err 改 silent fresh-fallback（更稳，无需 fragile 串匹配）。变体标 `#[allow(dead_code)]` + 内联 M4-d 由来；i18n 与 `code()` 臂/单测保留（码表完整性）。
- `client.rs` `RemoteTask.auth_token`：server claim 时签发的 task-scoped 短期凭证（mat_，webank `GenerateAgentTaskToken`）。**Option B 中介设计下不消费**（选项 B：码灵作中介，agent 从不直调 multica API，所有调用用码灵 PAT）——仅按 wire 契约反序列化。**修正失真注释**（原写「M4 bridge 注入 ACP 执行」，与 Option B 矛盾）+ 定点 allow。
- `client.rs` `RemoteTask.prior_session_id`：续跑兜底指针（server 响应回填），主路径是 `parent_task_id` 反查本地索引。定点 allow（注释本就准确）。

**附带清理**：`bridge.rs` 模块级 allow 移除后无 bridge dead_code 暴露（该 allow 本就过期）；`开发设计.md` §2.3 方法清单的 `rerun_issue` 行（M5-ah 删 rerun 死链时的遗漏）一并删除。

**验证**：`cargo check -p gold-band-desktop` 绿——**multica 零 warning**（仅 main 既有 9 条非 multica dead-code：`scheduled_task_vms_from_sources`/`task_uuid`/`title`/`task_has_active_execution*`/`provider_diagnostic_snapshots`/`refresh_agent_command_catalog_for_workspace`/`NodeMetricBatch` 可见性/`expected_windows_toast_auto_dismiss_seconds`/`normalize_multica_base_url` 未用 import 等，非本次引入）；`cargo test multica::` **83 测全过、0 失败**；`tsc -p web/tsconfig.build.json` 零错；vitest **1124/1124 全过**。

**文件**：`src-tauri/src/multica/{error,state,bridge,client,commands,vm}.rs`（删 allow + retire + 定点 allow + 注释订正）、`web/src/i18n.ts`（删 workspace-empty + pin-session-failed zh/en）。

---

### 12.30 改动二十八：wb 渠道 multica 地址修正——默认端口 80 → nginx 统一入口 `:5005`（M5-ar，2026-08-13）

> **已被改动九十（§12.92）取代**：multica 已迁至独立子域（无端口），地址不再从 WB 服务域派生 `:5005`，改由 `GOLD_BAND_WB_MULTICA_URL` 单独注入。本条的「改用编译期渠道配置而非运行期旁路」判断仍然有效。

**背景（跨机器部署）**：将 MALING 客户端打包到非开发机运行、连远程 multica（部署在 172.21.18.88，经 nginx 统一转发，对外入口 `WB Multica 服务地址`，前端 `/login` 与后端 `/api/*` 同源同端口）。核查发现 `configs/channels/wb.json` 的 `multicaBaseUrl`/`multicaAppUrl` 仍是 `WB 服务基础地址`（默认 80 端口，过时）——与服务端实际 nginx 入口不符，连不上。

**根因（好设计、值过时）**：渠道配置链路本身正确（`configs/channels/wb.json` → `build.rs` 编译期 env → `channel.rs option_env!` → `config.rs multica_base_url/app_url`，且 `build.rs:146-157` 支持 env 覆盖 json），仅 multica 地址值滞后于服务端 nginx 端口调整。运行期不可改（`connect_multica` 不收 URL、设置页 multica 区块已移除、`save_multica_settings` 已删，见 §12.22/接入方案 §3.2.5），故必须修正编译期配置值而非加运行期旁路。

**方案（修正过时值，非补丁）**：wb 渠道本就是 maling 生态（appName MALING、updater/metrics/内置 MCP 均指向 WB 服务域名），multica 地址属同一生态，直接把过时默认端口修正为实际 nginx 入口：
- `configs/channels/wb.json`：`multicaBaseUrl`/`multicaAppUrl` 由 `WB 服务基础地址` → `WB Multica 服务地址`。
- 前后端同入口：`base_url`（API 根，client 拼 `/api/*`）与 `app_url`（浏览器登录页 `<app_url>/login`）同源，顺带消除「前端页面 API 地址硬编码 localhost」隐患（同源直接命中，无 CORS）。
- `browser_login` 的 `cli_callback` 仍是 Gold-Band 本机 `127.0.0.1:<port>`，nginx 只透传 multica 后端登录成功的 302，跨机器 connect 不受影响（见接入方案 §3.2.6）。

**部署前置（服务端，非本项目代码）**：multica 服务端经 nginx 对外统一 `WB 服务域名:5005`，已由用户确认就绪；Gold-Band 侧无需关心 server 内部 5000/5050 监听细节。

**验证**：`GOLD_BAND_RELEASE_CHANNEL=wb cargo check -p gold-band-desktop` 绿——build.rs 正确解析 wb.json 新值，编译期 env `GOLD_BAND_MULTICA_BASE_URL/APP_URL=WB Multica 服务地址` 生效；default 渠道 `cargo check` 无回归。纯渠道配置值修正，无运行时逻辑变化，无性能影响（仅连接地址常量）。

**文件**：`configs/channels/wb.json`（multica URL +端口 5005）；`开发设计.md` §2.2.4 表格 wb 列（泛描述 → 具体值）；`接入方案.md` §3.2.5 channel config 段（同）。

---

### 12.31 改动二十九：二次合并 origin/main——会话创建契约对齐 + composer chip 重新集成（M5-as，2026-08-24）

**背景**：feature_multica 自 M5-ag 合并后再落后 origin/main 约 200 commit（main 重构了会话创建契约、心跳机制、个性化偏好、壁纸系统、composer/工作区信息条 UI 等）；feature 领先 22 commit（multica M5-ah…M5-ar）。用户要求同 M5-ag 策略：冲突优先保 main，合并后在结果上修复 multica 功能。

**安全网**：合并前在 `3e195328` 打备份分支 `feature_multica_premerge_20260824` / `feature_multica_premerge_backup`；合并提交 `79ba1e26`。

**main 重构吸收（multica 侧适配点）**：
- `create_conversation_run_vm` 返回值由单 VM 改为 `ConversationCreateResultVm { task, run }`（main 新契约：创建即回任务行 + run 快照，侧栏即时刷新）。
- 心跳机制重构：`metrics::start_heartbeat_polling` 删除，改为 RuntimeLifecycleBus metrics subscriber + `DesktopState::reevaluate_heartbeat_config`（main.rs：`start_multica_loop` 后跟 `reevaluate_heartbeat_config`，顺序不变）。
- `WorkerNode` 增 `execution_slot_id`；`RunState` 增 `worktree`/`execution`（multica 测试构造补 `None`/`Default::default()`）。
- `DesktopThemePreference`/`DesktopFontPreference` 删除（→ PersonalizationPreference 体系），desktop.ts 类型 import 随之更新。

**冲突解决**：14 文件——import/注册类并集（commands.rs 两个 lifecycle subscriber：`desktop.multica` + main 的 `desktop.conversation-terminal-result`；main.rs 命令表 + multica 命令块 + 恢复链 `recover_interrupted_running_sessions` + `recover_multica_work_dir_sessions`）；重度重构文件取 main 全量再回插 multica 增量（ConversationSidebar：Globe 图标 + nav-key case + SidebarButton；ConversationComposer：见下）。

**合并诱发的 multica 修复（二次修复阶段）**：
- Rust：`dsl/presets.rs` 补 `execution_slot_id: None`；`view_models_conversation.rs` 回插 `PromptEnvelopeMode`/`WorkerNode` import（main 内联构造需要）；**`multica/commands.rs` 契约适配**——`start_multica_conversation_run` 返回 `ConversationCreateResultVm`：Fresh 分支簿记（register_active_run/with_state）改用 `created.run.task_id`/`run_id`，Resume 分支由 `conversation_run_vm` + `conversation_task_row_vm` 组装 `{task, run}`；`multica/mod.rs` 删除无消费者的 `normalize_multica_base_url` 再导出。
- 前端：`startMulticaConversationRun` 返回类型改 `Promise<ConversationCreateResultVm>`（client/desktop/browser 三处，browser 桩补全参数签名——其 createConversationRun 桩本就返回 `{task, run}`）；App.tsx 发送流两路径同契约解构 `{task, run}` + `applyConversationTask(task)` 刷侧栏，导航/快照链完全复用。
- **composer chip 重新集成（main 重构后结构）**：onSubmit 第二参 `multica?` 恢复；canSubmit 补 `!(multicaActive && !hasLocalWorkspaces)` 门；chip 复用 main 的 leading-adornment 缩进机制（`committedInputLayout`，与 slash tag 同 `top-2` 定位、互斥 slash 优先）；`handleUnbindMultica` + `shouldBackspaceClearMulticaBinding` 接入 handleKeyDown；**决策 d 落点迁移**——main 把工作区控件重构为 `ConversationWorkspaceControl`（info bar 内），增 `forceSelector` prop 实现单工作区强制下拉；**决策 e 落点迁移**——`ConversationWorkspaceInfoBar` 增 `emptyWorkspaceHint`，0 工作区 + multica 绑定时以虚线提示替代控件。main 已把草稿改为跨页持久（切工作区不再 reset），采纳 main 行为、原「multica 时跳过 reset」守卫随之作废。
- 文案合规：main 新增 `scheduled-task-i18n` 测试禁止实现文件含中文（对客文案与注释分离），composer 内新增注释改英文。

**warning 分诊**：合并后余量 warning 逐一比对 `git diff origin/main`——全部位于与 main 完全一致的文件（orchestrator/scheduled_runtime/state/conversation_attention/desktop_lifecycle/metrics/notifications 等），为 main 既有技术债，按「保 main 完整性」策略不动；唯一合并诱发项（multica/mod.rs 未用再导出）已修。

**验证**：`cargo check --workspace --all-targets` 零错误；`tsc -p web/tsconfig.build.json` 零错；`web:build` 成功；vitest 全量（multica 4 套件 + composer 4 套件 + sidebar 2 套件定向全绿，全量套件结果见任务记录）；`cargo test --workspace` 结果见任务记录。

---

### 12.32 改动三十：三次合并 origin/main——draft 双维度 union（submission × multica 互斥）（M5-at，2026-08-24）

**背景**：M5-as 修复提交（`634bfbe1`）后 origin/main 又新增 29 commit（`7e599f62..8c0e69bd`，173 文件 +15020/−2827）：ACP 生命周期收敛（`797b9929`/`34c614db`）、scheduler 修复（`e3405d91`）、长消息折叠（`6a614db3`）、worktree 修复与动态路径迁移（`a6cf0d3a`）、**composer draft 提交意图模式**（`4a19b1d7` 等：draft 增 `submission` 维度 `send | scheduled-task`，scheduled 配置从组件本地 useState 上提进 draft canonical 状态，切工作区不重置）。用户要求同前两次策略：冲突优先保 main，合并后在结果上修复 multica。

**安全网**：合并前在 `634bfbe1` 打备份分支 `feature_multica_premerge2_20260824`。

**冲突（6 文件）与解决**：
- `src-tauri/src/main.rs`（1 处，import 字母序重排）：main 已删除 `cancel_acp_session`（ACP 生命周期统一，全树零引用），取 main 侧并集、仅按字母序回插 `connect_multica`。
- `web/src/App.tsx`（1 处，纯注释差异）：功能代码两侧一致（main 的「切工作区保留草稿」新设计与 multica 决策 d 天然兼容），取 main 侧（删过期注释）。
- `web/src/api/desktop.ts`（1 处，超长单行 type import）：取 HEAD 行（= main 行 + 5 个 multica 类型按字母序插入的并集）；另适配 main 改名 `ConversationSessionSwitchVm` → `ConversationSessionTreeVm`。
- `web/src/lib/conversation-composer-draft.ts`（6 处，**本次合并的核心语义决策**）：draft 状态双维度 union——`{ content, attachments, multica, submission }`，reducer/action/context/owner-hook 全并集。**互斥语义**（multica 绑定与 scheduled-task 是竞争性提交意图，在状态机层显式互斥，杜绝「带远程绑定点发送却走排程」的混合态）：`prefill` 为覆盖式新草稿——绑定 multica 且 `submission` 重置为 `send`（声明本草稿为远程执行草稿）；`enterScheduledTask` 丢弃 multica 绑定（任务仍在服务端 queued，纯本地解绑，同 `clearMultica` 语义），用户可重新点「执行」恢复。
- `web/tests/conversation-composer-draft.test.ts`（6 处）：两侧测试全保留，状态字面量补齐双字段；main 的 exit/reset 断言扩 `multica: null`；**新增 2 条互斥语义固化测试**（prefill from scheduled-task → send 意图；enterScheduledTask → 丢绑定）。
- `web/src/components/conversation/ConversationComposer.tsx`（5 处）：**scheduledMode/scheduledConfig 取 main 派生式**（`composerDraft.draft.submission` 直读，canonical 状态进 draft、跨卸载存活——优于 feature 旧本地 useState）；canSubmit 在 main 基础上叠加决策 e 条件 `!(multicaActive && !hasLocalWorkspaces)`；其余（最后一处两侧字节相同的平凡冲突取一侧）。**multica 表面（chip 渲染/`handleUnbindMultica`/Backspace 键盘契约/决策 d `forceSelector`/决策 e `emptyWorkspaceHint`）全部经自动合并存活，零回插**。

**warning 分诊**：合并后 cargo check 余量 warning（orchestrator.rs 三个 dead fn、conversation_attention.rs `pub fn unread_terminal_result`、commands.rs 字段、metrics/identity.rs 枚举变体等）逐一在 origin/main blob 上定位到同名符号同位置——全部为 main 既有技术债，按「保 main 完整性」策略不动；无合并诱发项。

**验证**：`cargo check --workspace --all-targets` exit 0 零错误；`tsc -p web/tsconfig.build.json` 零错；定向 vitest（draft + i18n）24/24；全量 vitest **1627/1627**（首次与 cargo check 并发出现 worker 启动超时——1570 测试 0 失败、4 文件未跑，机器空闲后重跑全绿）；`web:build` 成功。Rust 测试定向覆盖（本机 `cargo test --workspace` 不可行，见下）：`multica::` **83/83**；desktop crate 除 `updater::` 外 **593/594**——唯一失败 `app_config_vm_exposes_workspace_layout_contract` 经 origin/main blob 比对证明 **main 自身失败**（main 把 `conversationInlineContentMaxBytes` 64000→20000 但断言未同步，view_models.rs/updater.rs 均与 main 字节一致）；lib 侧 feature 差异模块（`config::`/`dsl::`/`app::multica`）**53/53**。

**本机测试环境备注（非代码问题）**：`updater::tests::polling_reuses_one_manifest_check_for_silent_channel` 在本机无限挂起（main 既有，mock server 127.0.0.1，>20min 无进展，需 `-- --skip updater::`）；E 盘 50G 限额被 target/（24G）填满导致编译中断一次，`cargo clean` 后恢复——全量 workspace 测试在本机不可行，以「check 全绿 + 定向模块测试 + 全量 vitest」为合并验收标准。

### 12.33 改动三十一：四次合并 origin/main——composer 分支选择 × multica 表面同域合取（M5-au，2026-08-27）

**背景**：M5-at 合并提交（`6ec3ab91`）后 origin/main 又新增 31 commit（`6ec3ab91..9eb71f58`，含 release 0.14.0/0.14.1）：**composer 工作区分支选择**（`1ea3a884`/`9ea117fa`，worktree 模式下每工作区可选 git 分支、`branchMutationPending` 切换中禁发）、ACP 动画/生命周期修复（`94714db7`/`12a24db2`）、AI-DYNAMIC 路由与恢复加固（`de6310ac`/`a932ca5f`）、slash 菜单层级（`9f078236`）、菜单文案统一（`abe61aa5`/`ec0277e3`）、Git 2.36 最低版本（`deb032e3`）。策略同前三次：冲突优先保 main，合并后在结果上修复 multica。

**安全网**：合并前打备份分支 `feature_multica_premerge3_20260827`（于工作区分诊提交 `0cf5af57` 后）。

**冲突（4 文件）与解决**：
- `resources/agent-catalog.json` / `resources/acp-registry.snapshot.json`（整取一侧）：两侧均为 registry 快照例行刷新（我方 `0cf5af57` 09:00 fetch vs main `8d567601`/`0ffa9c9c` 10:26），取 main 较新版本。
- `src-tauri/src/main.rs`（2 处，机械并集）：命令 import 并集——main 新增 git 分支命令（`change_git_branch`/`get_git_branch_picker_snapshot` 等）+ multica 命令块按字母序归位保留。
- `web/src/components/conversation/ConversationComposer.tsx`（9 处，**本轮唯一业务冲突**，详见下）：main 为底座 + multica 表面增量回插，`canSubmit` 条件合取。

**核心语义决策（canSubmit 同域正交合取）**：main 新增 `!branchMutationPending`（分支切换中禁发）与 multica 既有 `!(multicaActive && !hasLocalWorkspaces)`（远程任务缺本地工作区禁发）叠加为合取——两条件正交、无混合态，不需要 M5-at 式互斥设计；**关键是验证旧设计未被破坏**：M5-at 的 draft 互斥状态机（`{content, attachments, multica, submission}`）本轮 main 未触碰、自动合并完整存活；`onSubmit` 双路径（第二参数传 multica 绑定）保持 M5-at 结果。

**multica 表面回插清单**：`forceSelector` 并进 main 的「Tooltip × 下拉展开互斥」逻辑（`onOpenChange` 联动 `selectOpen`/`hideTooltip`）；`emptyWorkspaceHint` 挂信息条 props；`multicaActive` 门进 `canSubmit`；props/destructure/InfoBar 三处接口并集（`showBranch={!scheduledMode}` 与 multica 两 prop 共存）；main 的 `branchMutationPending` 同时进发送键与 PromptInput 禁用条件、slash 菜单 `z-50`、菜单 switchTo 文案整取不动。`web/src/App.tsx`（+44/−2，全部 multica 增量）经自动合并存活零回插。

**main 自身缺陷分诊（合并验收时浮出，两例）**：
1. `web/tests/conversation-navigation.test.ts` 以字面 `\n` 匹配 App.tsx 源码（全文件唯一一条内嵌换行断言），Windows `autocrlf` 检出的 CRLF 工作区必挂；测试文件与被测代码块均与 origin/main 字节一致 → **main 自身 Windows 可移植性缺陷**。修复（`4d06ae8e`）：读取边界收敛 `readAppSource()` 统一归一化 `\r\n → \n`，属 main 正确性改进。
2. main release 0.14.1 提交（`0b341452`）只改 Cargo.toml 版本未重新生成 Cargo.lock（origin/main blob 证实 lock 仍 0.14.0），本地 cargo 自动补齐（`8377eab3`）。

**验证**：`cargo check --workspace --all-targets` exit 0（余量 warning 为 main 既有）；`tsc` 零错；`web:build` 成功；全量 vitest **1684/1684**（CRLF 修复后；一次 `acp-session-reentry` 全量负载时序抖动单独重跑 12/12、全量重跑全绿，判 flake）；Rust 定向 `multica::` **83/83**。冲突分析报告：`.claude/docs/merge/merge-conflict-analysis-2026-08-27.md`。

### 12.34 改动三十二：multica 绑定 chip 配色改 accent 配对——深色主题可辨识（2026-08-28）

**背景（用户反馈）**：切换深色主题后，composer 内 multica 绑定 chip「颜色和背景颜色相近导致难以辨认」。

**根因（正确设计但实现不完整：违反 token 配对语义）**：chip 自 M5-aj 起（§12.22）用 primary 单 token 染色：`border-primary/30 bg-primary/10 text-primary`。主题契约中 `primary/primaryForeground` 是「表面 + 表面文字」配对（button-primary recipe 同源），**不保证 primary 单独作前景色时与背景有对比**：gold-band 是单色反转主题（light `#0d0d0d` / dark `#f0f0f0`），碰巧双 scheme 均高对比，掩盖了问题；tech-neutral 的 primary 是按钮表面深灰（light `#2f2f2f` / dark `#2d2d2d`），dark 下与 composer 背景 `#1b1b1b` 几乎重合——`text-primary` 对比度 ≈1.1:1，`/10` 背景、`/30` 边框同样融入背景。`styles.css` 兜底 dark（`#313131` on `#181818`）同病。

**方案（对齐主题契约，非深色补丁）**：chip 改用契约中保证双 scheme 对比度的「强调表面」canonical 配对 `accent/accentForeground`（permission-card、recipe hover/selected、ui-interaction.md §6/§9 同源原则）：

- chip：`border-accent-foreground/15 bg-accent … font-medium text-accent-foreground`
- 关闭按钮 hover：`hover:bg-accent-foreground/15`（accent-foreground 半透明，light 下变深 / dark 下变亮，方向自然）

四组合均对比安全：gold-band 浅绿底深绿字 / 深绿底亮绿字；tech-neutral 浅灰底深灰字 / 深灰底浅灰字。第三方主题遵守契约即自动适配。本节取代 §12.22 中 chip 配色描述（结构/文案/交互不变）。

**验收固化**：`web/tests/conversation-composer-multica-chip.test.ts` 新增 `multica binding chip theme contrast` describe——chip 渲染段（`multicaBinding ? (` 至 `<PromptInputTextarea`）必须含 accent 配对三件套，且不得再匹配任何 primary 染色（`/-primary(?!-)/`，`primary-foreground` 豁免）；断言均为单行字符串（CRLF 安全）。

**验证**：vitest 16/16（multica-chip + composer-context-alignment）；`tsc -p web/tsconfig.build.json` 零错；`web:build` 成功；浏览器 light/dark × gold-band/tech-neutral 四组合 chip 静态 + 关闭按钮 hover 截图验证通过。

### 12.35 改动三十三：五次合并 origin/main——侧栏 canonical 单飞管线对齐 + composer 测量容器底座替换（M5-av，2026-09-03）

**背景**：M5-au 合并（`35c5be95`）后 origin/main 又新增 75 commit（`9eb71f58..9f9247c3`）。main 侧主要内容：**侧栏渐进加载重构**（`cdbadc7f`，`loadConversationSidebarBootstrap()` 单飞——`ConversationSidebarSingleFlight` 只去重并发不缓存结果、`invalidate(key)` 世代失效；旧 `getConversationSidebar`/`applyConversationSidebar` 直接对整体移除）、ACP 性能批处理系列（raw frame/timeline 持久化批处理、流式 tool update 合并、有界 chat 内存、长会话渲染降载 + 事件订阅 API 更名 `subscribeConversationEvents`）、会话身份与隔离（task UUID 隔离、删除时保身份、删任务前 retire run request）、webview 兼容分层（`useWebviewMeasuredContainer`、CSS 变量语义探测）、return-to-latest 触发改视口脱离（`22624727`）、个人分析系列、`.agents/skills` 文档。策略同前四次：冲突优先保 main，合并后在结果上修复 multica。

**安全网**：合并前备份分支 `feature_multica_premerge4_20260902`；合并前暂存区整理（23 个换行符噪音文件回滚、acp 快照 09-02 例行刷新单独提交 `2742de5e`）。

**冲突（8 文件 = 机械 7 + 业务 1）与解决**：
- 机械并集：`.gitignore`（main 精简规则 + `.agents/*`/`.omc`）、`src/config/mod.rs` 测试 import、`web/src/api/desktop.ts` 类型 import、`WorkspaceShell.tsx` props（main `onDeleteTask` 加 `taskUuid` + 我方 `onNewConversationInWorkspace` 保持必填）、`conversation-navigation.test.ts`（main 新测试 + `readAppSource()` 泛化）。
- 机械整取：`agent-catalog.json`/`acp-registry.snapshot.json` 本轮**反向取我方**（我方 09-02 fetch 比 main 09-01 新）。
- 业务（唯一文本冲突）：`ConversationComposer.tsx` 9 处——main 的 `useWebviewMeasuredContainer` 测量容器为底座（`a0c9111d` webview 兼容分层），multica 增量按挂载点回插（chip/draft hook/Backspace 解绑/onSubmit 第二参数/text-indent）。M5-at 互斥状态机与 M5-au canSubmit 合取连续第三轮零触碰自动存活。

**核心语义决策（隐藏接缝：对齐 canonical，不复活旧路）**：`web/src/App.tsx` multica 事件驱动侧栏刷新调用的旧 API 对被 main 整体删除（tsc 捕获）。决策：刷新目标改 `loadConversationSidebarBootstrap()`——单飞"只去重并发、不缓存结果"正合"事件到达要新数据"语义（风暴去重成一次真拉取），best-effort 吞错保持；不复活旧 API 对（那是绕过失效协议的旁路状态，违反 state-lifecycle 规则）。

**main 自身缺陷分诊（合并验收浮出，两例代码修复）**：
1. `acp-read-only-agent-panel` 的 return-to-latest 测试：`22624727` 改触发为视口脱离未更新 08-02 旧测试（clean origin/main worktree 同样失败证实）。修复（`389ff979`）：提取 `web/tests/acp/detach-conversation-viewport.ts` 共享 helper（伪造滚动几何 + wheel 派发），测试先脱离视口再断言。
2. 根 crate lib 两条稳定失败（第三次全量确认仅此两条，run1 第三条为 flake）：a) `app_config_ignores_zero_conversation_auto_title_limit`——main `2c661a55` 把 embedded toml 的 `conversationAutoTitleMaxChars` 18→48 未同步常量与测试，修复：`DEFAULT_CONVERSATION_AUTO_TITLE_MAX_CHARS` 对齐 48（恢复"默认值==常量"不变量）；b) `dynamic_group_successor_creation_emits_session_update`——main `8ce80327` 新增 creator-scope 校验（creator.group_id 须等于 group.parent_group_id）未更新旧 fixture，旧 fixture 把创建者与组内成员压在一个节点上，修复：拆双节点（创建者顶层 + 成员携带 group_id），对齐生产语义。
3. 依赖漂移伪装回归一例：node_modules remend 1.3.0 vs 锁文件 1.3.1，`npm install` 同步即绿；cargo 两起瞬时缓存损坏（后台任务被杀）重跑自愈。

**验证**：`cargo check --workspace --all-targets` exit 0；`tsc -p web/tsconfig.build.json` 零错（含 tests 的全量 tsc 541 个既有错误为缺 @types/node，tests 从不在 typecheck 门禁）；全量 vitest **1880/1880**（259 文件）；Rust 定向 `multica::` **83/83**（与 M5-au 持平）；tauri bin 全量 **644/644**；根 lib 全量 1111/1111（两条 main 侧修复后）。冲突分析报告：`.claude/docs/merge/merge-conflict-analysis-2026-09-03.md`。

### 12.36 改动三十四：六次合并 origin/main——同日追平零冲突 + helper 提取接缝核验（M5-aw，2026-09-03）

**背景**：M5-av 验收期间 origin/main 又进 5 commit（`9f9247c3..c9e82acb`，全为 fix）：pending interaction 与 canonical timeline 对账（`c9e82acb`，reentry 测试 +393 行）、跨工作流边界的 runtime session 跟随（`b81f5344`）、近底部手动滚动逃逸保留（`d8d0e7f0`）、外部工作区变更 watcher 对账（`d50bae89`）、ai-dynamic 报告清单传递后继（`c7bf84e5`）。

**冲突**：**0 个**。两侧交集 4 文件（commands.rs / App.tsx / types.ts / reentry 测试）全部自动合并。关键接缝：`c9e82acb` 新增测试大量调用 `detachConversationViewport`，其本地定义恰是 3 小时前 §12.35 中我方提取到 `tests/acp/detach-conversation-viewport.ts` 的同一段代码（两侧逐字节一致）——自动合并取我方 import + main 调用点 + 删除本地定义，语义自洽，全量测试兜底确认。

**验证**：`cargo check --workspace --all-targets` exit 0；`tsc -p web/tsconfig.build.json` 零错；全量 vitest **1894/1894**（259 文件，main 新增 14 条全过）；`multica::` **83/83** 保持。合并提交 `75059f2d`（备份分支 `feature_multica_premerge5_20260903`）；报告并入 `merge-conflict-analysis-2026-09-03.md` §6。

**结论**：同日追平兑现了 §12.35"缩短合并间隔"的建议——5 commit 间隔零冲突一轮全绿，对比 75 commit 间隔的 8 冲突 + 3 处 main 缺陷。"零冲突"仍需交集文件语义核验（本轮风险点 git 不报冲突）。

---

### 12.37 改动三十五：PR review 三问题修复——teardown 定点取消 / with_state 全量统一 / 启动自愈管线化定点收敛（M5-ax，2026-09-03）

**背景**：`feature_multica` → `main` 的 PR review 提出三个问题（`.claude/docs/problem/2026-09-03-pr-problems.md`），分析与修复全记录见 `.claude/docs/problem/2026-09-03-pr-problems-fix.md`。三者的根因分类与处置：

**P1-1 单任务 teardown 取消 workspace 内全部 ACP 会话（实现逻辑错误）**
- **根因**：`teardown_active_run`（bridge.rs）复用了全库扫描原语 `cancel_all_active_acp_attempts_best_effort`——取消主体是"单个 remote task 的一个 run"，但取消原语的作用域是整个 workspace。run 级身份（`local_task_id` + `local_run_id`）早已存在，缺的只是 run 级取消原语。
- **修复**：App 层新增 `cancel_active_acp_attempts_for_run_best_effort(task_id, run_id)`（与全量版共用逐 attempt 收尾 helper，筛选从"全部"收窄到目标 run）；`teardown_active_run` 改用之。同工作区其他任务的会话不再被连带取消。
- **验证**：`cancel_active_acp_attempts_for_run_scopes_to_target_run_only`（同 workspace 两个 run，只取消目标 run 的 attempt）。

**P1-2 StateConfig 并发写丢数据（正确设计但覆盖不完整 + 锁身份错位）**
- **根因（两层）**：① §12.27 的 `with_state` 只迁移了 multica 5 写点，pin/preference/workspace/updater 等既有写点仍是裸 `load_state → mutate → save_state`；② 锁按 `repo_root` 分片，但 `state.json` 是**用户级全局单文件**（home 目录），不同 repo_root 的 App 写同一文件却落在不同锁分片——锁身份与被保护资源不一致，互斥失效。
- **修复（`with_state` 成为 StateConfig 唯一 RMW 入口）**：
  - 签名升级 `with_state<T, F>(&self, update: F) -> Result<T>`，闭包返回 `(dirty, T)` 双通道——落盘决策与回传值（如 `CommandResult<Vm>`）分离，错误先于变更返回时 `dirty=false` 不落盘。
  - 锁身份改为 `normalize_workspace_path(user_state_file())`（状态文件路径）哈希分片——跨 repo_root 的并发写互斥。
  - `save_state` 降为 `pub(crate)`（结构性防护：src-tauri 无法裸 save）；`load_state` 保持 pub（只读不改）。
  - 全量迁移：lib 内部 5 个 setter + src-tauri multica 5 处 + `commands.rs` 2 处（connect/disconnect 账号清索引）+ `commands_conversation.rs` 12 处（pin/unpin/reorder/preference/last-workspace/workspace add/sync/remove/task-delete）。
  - **临界区纪律**：重 IO（trash/sqlite/manifest provisioning/ACP 连接关闭/coordinator.send）一律在锁外；add/sync 在事务内做权威复查（先读快照预检仅作 fail-fast），关闭"先查后写"竞态窗口。
- **新增测试**：`with_state_locks_by_state_file_across_repo_roots`（32 线程、不同 repo_root、同一 state 文件，全部写存活）；`with_state_mixes_multica_and_user_preference_writers_no_lost_update`（multica 后台写 vs 用户 recent-workspace 写并发，两列表完整）。

**P2 启动关键路径同步执行无界 work_dir 扫描（设计缺陷：无界恢复塞进同步路径）**
- **根因**：§12.15 的 `recover_multica_work_dir_sessions` 在 Tauri setup **同步**执行，且逐 work_dir 跑全量 `recover_interrupted_running_sessions()`（遍历该 repo 全部任务历史）——work_dir 数 × 任务历史数无界，阻塞首窗渲染。
- **修复（管线化 + 定点化，缺一不可）**：
  - **移入管线**：调用点移进既有 `spawn_blocking` 启动恢复任务（先于 `complete_startup_recovery`、与 conversation workspace 恢复同管线）；setup 同步路径只剩 home repo 单点自愈。
  - **定点收敛**：删 `collect_multica_work_dirs`（破坏式更新）；改为遍历 `multica_task_conversations` checkpoint，逐条按 `local_task_id`+`local_run_id` `run_status` 定点判定，仅 Running 孤儿做 `run_pause(ProcessInterrupted)` + `cancel_active_acp_attempts_for_run_best_effort`（复用 P1-1 原语）——O(checkpoint 数) 次定点 I/O 替代 O(work_dir × 任务历史) 全量扫描；work_dir → workspace App 以 HashMap 缓存复用。
  - **时序正确性（resume 等门）**：resume 判定依赖恢复收敛（未收敛时 checkpoint 指向的 run 仍残留 Running → `is_run_continuable`=false → 误落 Fresh）。`RuntimeRecoveryCoordinator` 增 `phase_change: Condvar` 与 `wait_for_startup_accepting(timeout)` 阻塞等待；`start_multica_conversation_run` 在 `classify_resume` 前 `spawn_blocking` 等门。相位完整定义为 `Recovering → Accepting | Failed | ShuttingDown`：成功进入 `Accepting`；候选读取错误或 blocking task join/panic 统一进入 `Failed` 并 `notify_all`，等待者立即得到 `runtime.startup-recovery-failed` 后按既有 best-effort 语义继续；仅真正处于 `Recovering` 时保留 30s 超时，杜绝持续失败后每次发送重复等待 30 秒。
- **新增测试**：`recover_multica_work_dir_sessions_pauses_only_checkpointed_orphan_runs`（checkpoint 命中的孤儿 Running → Paused+ProcessInterrupted；未登记的 run 不受影响）；`recover_multica_work_dir_sessions_skips_missing_and_non_running_checkpoints`（非 Running 不动、run 缺失跳过不报错）；`wait_for_startup_accepting_blocks_until_phase_change_or_timeout`（超时/放行/shutdown 三分支）；`startup_failure_is_terminal_and_wakes_waiters`（失败即时唤醒、稳定错误码、重复失败幂等、失败后不可回到 Accepting，scheduler/run 注册同样拒绝）。

**方案自评审**：过度设计——未新增持久字段、aggregate、队列、缓存或依赖；run 级取消与全量版共享收尾 helper；等门复用既有 RuntimeRecoveryCoordinator，仅增加一个表达真实恢复结果所必需的 `Failed` 相位与 Condvar 唤醒，不复制 canonical state。性能——P2 本身是性能修复（同步关键路径去掉无界扫描）；`with_state` 锁内只有校验+变更+一次读盘+一次写盘（原本裸 RMW 也需同样 I/O，锁只把交错排序，不放大 I/O）；定点恢复 O(checkpoints)；失败路径从每次固定 30 秒等待改为一次状态转换后立即返回。无新增轮询或无界工作。

**验证**：`cargo fmt --all -- --check` 通过（review 指出的 fmt 失败已修复）；`cargo test -p gold-band --lib` **1116 过 / 1 ignored（既有）**；`cargo test -p gold-band-desktop --bin gold-band-desktop` **648 过**（multica 83 含新增 3、runtime_recovery 含新增 1）。

### 12.38 改动三十六：连接地址运行期可配置——连接确认弹窗 + 设置 icon 地址入口 + 取消连接（M5-ay，2026-09-07）

**背景 / 根因分类**：用户需要在内网（webank dev，API 8080 / 登录页 3000）与外网（maling:5005 统一入口）间切换 multica 服务器。M5-ah 删除设置页 multica 区块与 `save_multica_settings` 后，`SettingsConfig.desktop_multica_base_url/_app_url` 成死字段，地址只能靠渠道 json（M5-ar）编译期决定——**好设计（settings 优先 → channel 兜底的解析链路）但写入方缺失**。修复方向：复活既有死字段为运行期覆盖（破坏式复兴，无兼容层），而非新增旁路配置。M5-ar「运行期不可改」结论就此被取代，渠道值退为默认兜底。初版实现「环境预设 + 弹窗内改地址」后按用户反馈两轮调整为最终形态：去环境概念、地址修改独立到设置弹窗、连接弹窗恢复可改地址（有变才保存、无提示文案行）、新增取消连接。

**数据 / 接口**（先定数据再定接口）：
- 零新增持久字段：复用 `desktop_multica_base_url/_app_url`；VM 增 `addressOverrideSet: bool`（`base_url.is_some()`）驱动「恢复默认地址」可见性。
- 纯函数 `apply_multica_connection_address(&mut SettingsConfig, base, app)`（config.rs）：两参数须「同为 Some 或同为 None」（前端是单一地址输入、保存即两值同址；半覆盖无从判定用户意图）→ `multica.invalid-address`；各过 `normalize_multica_base_url`；双 None=清除覆盖回落渠道默认。分端口形态（API 8080 / 登录页 3000）只由渠道编译期默认产生，运行期无预设选择。
- 命令 `save_multica_connection_address`：保存前后各算一次生效地址（`multica_base_url_for_settings` 把 settings 镜像到克隆 config 走统一解析链，判定与运行期消费同源），**生效地址变化且 `pat_set`** → 作废 server 作用域状态（`clear_multica_session` + `with_state(clear_multica_state_indices)` + `clear_runtime_ids`——PAT/runtime_id 对旧 server 签发，M5-af 账号作用域不变量向服务器身份维度延伸）；`save_settings` + `update_settings_config` + emit settings-updated。`connect_multica` 仅增取消接线（见下），地址获取零改动（从解析链拿新地址）。
- 取消链：`browser_login` 全程 tokio → 命令层 `tokio::select!` 挂 `CancellationToken`（future drop 即释放资源、无状态写入，client.rs 零改动；取消只作用登录等待段，落盘/注册是本地快操作不挂取消）。取消槽 `MulticaConnectCancel`（state.rs）：**单调递增登记 id 认领**（CancellationToken 无 PartialEq；register 替换并 cancel 旧 token 防异常残留，clear_if_same 按 id 比较防迟到清理误删下一次连接）。新命令 `cancel_multica_connect`（无进行中连接时幂等 no-op）+ 错误码 `multica.connect-cancelled`（非失败语义，前端静默关窗不作错误展示）。
- 前端两弹窗互斥单飞行（页面级 `'connect' | 'settings' | null` 状态）：
  - `MulticaConnectionSettingsDialog`（gear 入口）：单一「连接地址」输入（无环境下拉、无登录页提示行），保存 = API 与登录页同址（`saveMulticaConnectionAddress(v, v)`）；**与当前生效值相同禁用保存**（防「打开看看就保存」把分端口渠道默认 8080/3000 误覆盖成同址 8080/8080）；预填走 settingsRef 模式（ref 读最新值、仅以 open 触发，不覆写用户编辑中输入）；`addressOverrideSet` 时「恢复默认地址」双 null 回落渠道默认、用返回 VM 刷新弹窗字段留窗继续编辑。
  - `MulticaConnectDialog`（连接按钮入口）：**确认 + 可改地址 AlertDialog**——预填当前生效地址、可直接编辑（校验 http(s)，非法禁用连接、无提示文案行）；确认时**地址有变才保存**（`saveMulticaConnectionAddress(v, v)`，未变不写——与设置弹窗「未变更禁用保存」是同一防分端口覆盖不变量的两种 UI 表达）再 `connect_multica`；连接中主按钮禁用转「连接中…」、取消按钮变「取消连接」→ `cancelMulticaConnect`；**「取消连接」只发取消信号不直接关窗**——关窗统一走连接命令的 cancelled 收尾路径（单一关闭事实源，避免取消竞态下状态悬空）；取消判定本地窄化 `{code === 'multica.connect-cancelled'}`。
  - API 四层（client/desktop/api/browser）+ `cancelMulticaConnect` browser 幂等 no-op mock 同步。破坏式更新：删旧双模式组件 + `MULTICA_ADDRESS_PRESETS` + 预设/登录页提示与 changeHint i18n 键（zh+en）。URL 校验抽 `@/lib/multica-address.ts` 两弹窗共用（`isValidHttpUrl`）。已连接态无入口（改地址须先退出登录）。
- 生效时延：心跳每 tick 经 `build_client` 重新解析（既有行为），命令保存后 `update_settings_config` 刷新内存配置 → 改地址 ≤15s 生效、无需重启。

**方案自评审**：过度设计——零新增持久字段/循环/缓存/队列；保存侧一个命令一个纯函数，取消侧复用 tokio select 语义 + 一个 id 认领槽（不引入 per-request 句柄表/请求 id 路由）；服务器变更作废复用 M5-af 既有原语，仅一个字符串比较判定；拆两弹窗后各自无内部 mode 分支，比双模式组件更简。性能——保存是一条命令一次 settings.json 写；取消槽是 O(1) Mutex 单槽无争用面；地址解析是既有心跳路径的微秒级字符串规范化；弹窗按需挂载、复用既有 settings-updated 事件，无新增订阅。无风险点，不需 benchmark。

**验证**：`cargo test -p gold-band-desktop multica::` **91 过 / 0 失败**（新增 apply roundtrip/半覆盖拒绝/非法拒绝/双 None 清除、生效地址变化门（含「清除覆盖但渠道值相同 → 不作废」）、命令级作废链、取消槽 3 测：cancel 幂等取消 / clear_if_same 不误删下一次登记 / register 替换并取消残留 token）；tsc `tsconfig.build.json` 零错；vitest `multica-connect-dialog`（7 用例：预填可编辑+无 hint/地址未变直连不保存/编辑后先 save(v,v) 再 connect/非法禁用+invalidUrl/失败显错留窗/cancelled 静默关/连接中禁用+取消触发）+ `multica-connection-settings-dialog`（5 用例：预填/保存 (v,v)+关窗/未变更禁用/非法禁用/恢复默认刷新留窗）+ `multica-task-management-page`（13 用例：连接按钮开确认弹窗、gear 开地址设置弹窗、互斥单飞行）**25 过**；multica 回归 4 套件 34 过（add-workspace-dialog / remote-task-board / composer-multica-chip / app-error-i18n）。

---

### 12.38 改动三十六：侧栏低频入口收纳 + 需求管理命名（M5-ay，2026-09-04）

**设计判断**：需求管理和定时任务仍是独立领域、独立页面与独立路由，原设计无需合并；问题来自会话侧栏持续平铺一级功能，缺少随功能增长的低频导航层级。修复只重组 Shell 导航投影，不复制 canonical 数据或生命周期。

**实现**：复用仓库已有 shadcn/ui `Collapsible`，在运行模式下增加同级的 `Ellipsis + 更多 + ChevronDown` 行，原位展开“需求管理 / 定时任务”；两个入口与一级导航共用同一左对齐轨道，不通过缩进伪造业务父子关系。默认折叠，展开状态只由侧栏组件运行期持有，路由进入 `multica-tasks`、定时任务列表或详情时自动展开，子项继续使用既有类型化 `ConversationPage` 导航和选中态。Multica 页面对客名称统一为“需求管理”/`Requirements`，内部 `multica-tasks`、`RemoteTask` 与 IPC/持久化字段保持不变，展示名称不参与稳定身份。

**测试与验收**：增加侧栏 DOM 接口测试覆盖默认折叠、手动展开、展开项无额外左缩进、需求管理导航和 deep link 自动展开；增加中英文 i18n 精确值测试；执行前端定向/全量测试、TypeScript 检查、生产构建，并在内置浏览器通过 `/chat`、`/chat/multica-tasks`、`/chat/scheduled-tasks` 验证实际交互和选中态。

**方案自评审**：过度设计——不新增聚合页、路由、持久化偏好、领域状态、依赖或兼容层；现有 Collapsible 和类型化页面状态已经足够。性能——新增状态位于最小消费组件，折叠时不挂载两个子项，导航不会触发页面数据预取、I/O、订阅或刷新；常量级 DOM 与 `O(1)` 路由判断无可测性能风险，不为此增加无意义 benchmark。

---

### 12.39 改动三十七：SKILL 双向同步——心跳 ack 推送上报 + PAT REST 拉取落库（M5-az，2026-09-09）

**背景 / 根因分类**：新能力开发（非缺陷修复）。multica server 侧已具备 skill 列表/详情/文件接口与 pending 引用下发能力，码灵本地有既有 SkillManager global skill 体系。设计先行：`.claude/design/multica_skill/码灵Multica-Skill同步设计与接口文档.md` 为实现权威（Q1–Q7 已关闭），推送复用 C-link 心跳 ack + result 端点既有模式（server 零改动），拉取用 PAT REST 三接口按需落库。实现期一处设计偏差并回写设计文档：非 UTF-8 文件从「跳过」改为「整包 failed」——静默跳过二进制文件会产生不完整 skill 且用户无感知，整包失败给出明确信号（错误信息含文件相对路径）。

**数据 / 接口**（先定数据再定接口）：
- **wire 类型**（client.rs，序列化测试锁定）：`HeartbeatAck { status, pending_local_skills: Option<PendingLocalSkillsRef{id}>, pending_local_skill_imports: Option<Vec<PendingLocalSkillImportRef{id, skill_key}>>, pending_update/pending_model_list: Option<serde_json::Value>（v1 占位） }`；`LocalSkillListReport { status, skills, supported, mcp_supported: false, error }`（None→null 序列化，**无 skip_serializing_if**——server 契约要求字段恒在）；`LocalSkillSummary { key, name, description, source_path, provider, root: "provider", file_count }`；`LocalSkillImportReport { status, skill, error }`；`LocalSkillBundle { name, description, content, source_path, provider, files }`；`LocalSkillFile { path, content }`；`RemoteSkillSummary { id, name, description(#[serde(default)]), updated_at }`；`RemoteSkillDetail { id, name, description(default), content }`；`RemoteSkillFileMeta { path, size(default) }`；`SkillFilesResponse` untagged Wrapped/Bare 两形态兼容。
- **client 方法**：`report_local_skill_list_result` / `report_local_skill_import_result`（推送上报，走既有 with_network_retry）；`list_remote_skills` / `get_remote_skill` / `list_remote_skill_files`（拉取，files 必须 `?include=metadata`）；`send()` 已是 4 参（workspace_id → `X-Workspace-ID` header）。
- **local_skills.rs（新模块）**：本地 global skill 扫描与 bundle 组装（限额常量集中模块顶部：1MiB/文件、8MiB/整包、256 文件、深度≤4）；拉取侧 `multica_skill_dir_name`（名称清洗 + 短 id 后缀防跨 skill 冲突）、`own_global_skill_exists`、`assemble_pulled_skill_md`（远端 name/description/content 合成 SKILL.md）。
- **loop_.rs**：ack 分发 `dispatch_pending_skill_work` + `provider_for_workspace`（按任务 workspace 解析 agent provider 后上报，复用心跳线程既有节奏）。
- **命令层**：`list_multica_skills`（1 远端请求 + 1 次 spawn_blocking 本地扫描投影 `localState`，无 N+1）；`pull_multica_skills`（批前一次扫描 + `seen: HashSet` 批内去重；每项 detail→files 判空→目录名→exists 判定→`spawn_blocking(write_instance)`）；`write_instance` 7 参语义：新建 name=清洗目录名 directory_path=None，已存在 name=远端展示名 directory_path=Some(已存在目录名)（重写 frontmatter name、保留未知字段）；写入后 `schedule_agent_command_catalog_refresh`（pub(crate) 化，刷新 agent 命令目录）。新错误码 `multica.skill.not-found` / `multica.skill.has-files` / `multica.skill.name-collision`（reason 透传给前端映射文案，复用 `multica.remote-error`）；VM：`MulticaSkillListItemVm{id,name,description,localState}` / `MulticaPullItemResultVm{id,name,outcome,reason}` / `MulticaPullReportVm{results}`。
- **前端**：`MulticaSkillSyncDialog`（列表 + new/exists 徽标 + 勾选 + 覆盖数量插值确认 + outcome 汇总报告 + reason 错误码 zh/en 文案映射）；API 层四层（client/desktop/api/browser mock）同步；i18n zh/en 对等。

**方案自评审**：过度设计——零新增持久字段/表/队列/缓存：推送复用心跳 ack 通道与 result 端点既有模式（server 零改动、码灵侧不新增循环）；拉取是命令层按需 REST + 批前一次本地扫描（O(本地 skills) 一次），seen HashSet 批内 O(1) 去重；限额/深度常量集中模块顶部管理，无硬编码散落。未新增 aggregate——localState 是消费端投影（远端 id+本地目录名比对），不复制 canonical 数据。性能——心跳 ack 分支仅 Option 判空，无 pending 时零成本；list = 1 远端 + 1 本地扫描；pull 每项 = 2 远端请求（detail + files metadata）+ 1 次写盘，N=用户勾选数且有确认门与进度态；报告汇总 O(N)。无全量扫描/无界队列/长持锁/流式热路径改动。风险：pull 写盘与本地并发编辑——write_instance 原子重写既有语义 + 覆盖前确认弹窗。无需 benchmark（数据规模=用户 skill 数，几十量级）。

**验证**：Rust 单测已编写（`local_skills.rs` 16 项：目录名清洗幂等/非法字符/空回退、frontmatter 组装覆盖与 body-only/horizontal-rule/description 回退、库发现与存在判定按目录名作用域、bundle 读取/缺目录缺 SKILL.md/文件数·深度·大小限额/非 UTF-8 整包失败、skill key 越权校验、递归文件计数；另有 loop ack 分发与 client wire 单测），`cargo check --bin gold-band-desktop -j 1` 通过；`cargo test` 运行被本机内存阻塞（`windows` crate rlib 编码单进程峰值 ~4GB commit > 实测可用 1.9GB，rustc OOM 退出——环境问题与本次改动无关，待内存释放后以 `cargo test -p gold-band-desktop --bin gold-band-desktop multica::` 固化回归）；tsc 零错；vitest multica 8 套件 **54 过**（新增 `multica-skill-sync-i18n` 3 测：全 key 双语解析、含点错误码 reason 子 key——固化 i18next ignoreJSONStructure 默认开启前提、覆盖数量与汇总插值；回归 add-workspace-dialog / connect-dialog / connection-settings-dialog / requirements-i18n / remote-task-board / task-management-page / conversation-composer-multica-chip 7 套件 51 过）；内置浏览器双主题验收「从 Multica 同步」弹窗（深链 Context 管理 SKILL 全局 Tab；dark 以 tech-neutral 深色 token 实测——主题引擎按 inline CSS 变量下发，`bg-background` 正确解析 #111111，无浅色残留；报告页 reason 文案正确映射 `multica.skill.has-files`）。

**修复记录（2026-09-10，用户实测：拉取覆盖同名 skill 报错）**：
- **症状**：对远端同名 skill 执行「从 Multica 同步」覆盖时，报告项失败，reason 透出 `SKILL dir not found: "to-spec"`。
- **根因（设计正确、实现走样）**：`pull_multica_skills` 把**裸目录名**（`dir_name`）当作 `write_instance` 的 `current_directory_path` 传入。该参数是**路径**而非目录名——`save_target_dir` 原样取作目标目录，`write_instance` 校验 `current_dir.exists()`，裸名按进程 CWD 解析为相对路径必然不存在 → 命中 `bail!("SKILL dir not found: {current_dir}")`（`src/skill/mod.rs:389-393`）。设计 §5.3 本来就要求传「已存在目录」，正常编辑器链路传的也是前端 `skill.directoryPath`（绝对路径）。上游诱因：判定函数 `own_global_skill_exists` 只返回 bool，丢弃了 `SkillMeta` 上唯一携带既有目录的 `directory_path`，调用方无从取得绝对路径。附带后果（同一根因）：即便绕过校验，`merge_skill_edit_content` 也拿不到旧内容，覆盖会丢失本地未知 frontmatter 字段。
- **修复**：`own_global_skill_exists` → `own_global_skill_dir(skills, dir_name) -> Option<String>`，命中时返回该 skill 的绝对 `directory_path`（无命中 None）；`list_multica_skills` 用 `is_some()` 保持 `localState` 判定，`pull_multica_skills` 原样作为 `current_directory_path` 传入（`exists` 同步改为 `is_some()`）。设计文档 §5.3 把该参数语义收紧为「既有 skill 的 directory_path（绝对路径，不是裸目录名）」。
- **验证**：新增 2 项回归单测——`own_global_skill_dir_returns_absolute_dir_of_matching_own_library_skill`（返回绝对路径且仍限自有库作用域）、`overwrite_requires_absolute_existing_dir_not_bare_name`（裸目录名失败并给出 `SKILL dir not found`，绝对路径覆盖成功且写在既有目录内）；修复前该契约不存在（编译失败，即缺陷证据），修复后 `cargo test -p gold-band-desktop --bin gold-band-desktop multica::` 110 过 / 6 失败（该 6 例为同一批次代码的独立根因，见下节）。

**修复记录（2026-09-10，本批次 6 例失败用例）**：
- **分诊（三类根因，按 `bug-fix-verification.md` 逐项建立红态证据后再改实现）**：
  - **(C) 重复计数——设计正确、实现不完整。** `collect_files_recursive` 未跳过根 `SKILL.md`，与调用方注释（`read_local_skill_bundle` 第 161 行「跳过根 SKILL.md」）和 `LocalSkillFile` 定义（设计 §4.4「支撑文件（不含 SKILL.md）」）直接相悖：正文被上报两遍，且同一文件同时占掉 content 与支撑两份文件数/字节限额，使恰好 256 个文件的 skill 被误判超限（推送导入链路真实功能缺陷）。
  - **(B) 空白/控制符优先级未定义——设计表述不完整。** `\t`/`\n`/`\r` 同时满足「控制符删除」与「空白折叠」两条规则，实现先判控制符使其永久绕过空白折叠，`a\tb` 得 `ab` 而非 `a-b`（粘连会改变 skill 身份）。
  - **(A) 测试期望错误。** 4 例 `assemble_*` 按字面未加引号的格式书写断言，实现走 canonical `render_frontmatter_document`/`yaml_scalar`（与编辑器写盘同源，值含非 `[A-Za-z0-9._-]` 字符即加双引号）——实现正确，测试假定了不存在的格式。
- **红态证据（修复前实测，与分析一致）**：`assemble_*` 左侧 `name: "PR review"` / 右侧 `name: PR review`；`dir_name_strips_illegal_and_collapses_whitespace` 左侧 `"abc"` / 右侧 `"a-b-c"`；`bundle_reads_skill_md_verbatim_and_support_files` 左侧 `3` / 右侧 `2`。为 (C) 新增边界用例 `bundle_does_not_count_root_skill_md_toward_file_limit`（255 支撑文件 + SKILL.md 恰好到顶，必须通过），修复前实测 `panicked ... Err("skill bundle exceeds 256 file limit: edge-files (257 files)")`——257 = 255 + SKILL.md 重复计 2 次，精确复现分析。
- **修复**：(C) 引入 `gold_band::config::SKILL_FILE_NAME`（消除 `"SKILL.md"` 字面量散落）并在根层跳过同名文件（子目录内的 `SKILL.md` 仍是普通支撑文件）；(B) 调整 `multica_skill_dir_name` 判定顺序，空白先于控制符/非法字符；(A) 按 canonical 引号规则更新 4 处断言并注明引号来源（防止被反向「修回」）；设计文档 §4.5 / §5.2 / §9 同步显式化计数口径与优先级。
- **验证（同一测试转绿）**：`cargo test -p gold-band-desktop --bin gold-band-desktop multica::` **117 过 / 0 失败**（110 基线 + 6 修复 + 1 新增边界用例）；相邻不变量未受影响（目录名幂等与空回退、限额各边界、非 UTF-8 整包 failed 均保持通过）。

---

### 12.40 改动三十八：需求管理适配 story dev/test 拆分——类型标记 + 类型过滤 + 未就绪可查（M5-ba，2026-09-10）

**背景 / 根因分类**：新能力开发（非缺陷修复）。multica server 发布 `2-story-dev-test-split` 规格：issue 增加 `issue_kind ∈ (dev|test|bug|general)`，test 型 issue 派生 `is_ready`（父 dev issue 是否 done）。用户需求两条：① 需求管理页支持开发/测试两类 issue，块上有类型标记且可按类型过滤；② test 型可查「是否 ready」。内网确认清单（`.claude/design/multica-story-refactor/`）已与 multica 侧对齐**契约**：`AgentTaskResponse`（pending/claim/detail 三处共用）新增 `issue_kind`(string, omitempty) + `is_ready`(bool)；心跳 ack 增加就绪变更 diff。**server 侧对 pending 不做任何过滤**（未就绪的 test 任务照常下发），因此**门控责任在客户端**——这是本改动的核心约束。

**契约关键决策**：
- **字段命名不复用 `kind`**：`AgentTaskResponse.kind` 已是「任务来源判别符」（issue/chat/autopilot/quick-create 互斥），语义不同，故新增独立 `issue_kind`。码灵 `RemoteTask` 同名同义。
- **版本解耦**：码灵两个新字段全部 `#[serde(default)]` / Option，旧 server（无这两键）→ `None`，行为与拆分前逐字节一致；码灵可先于 server 上线。
- **保守缺省**：test 型缺 `is_ready` 按**未就绪**处理（覆盖 server 灰度期只发 `issue_kind` 不发 `is_ready` 的窗口），非 test 型 `is_ready` 无论真假都不参与判定（server 对非 test 亦可能回传 `is_ready=false`）。
- **就绪变更用「刷新信号」而非「本地 patch」**（state-lifecycle 规则：投影不得反向成为事实源）：心跳 ack 的 diff 非空 → 只发既有 `multica-task-updated` 事件 → 页面按既有订阅重拉 pending。**禁止**用 diff 直接改本地任务列表——那会让本地成为第二份事实源。

**数据 / 接口（先定数据、再定接口、再补实现）**：
- **wire 层（`client.rs`）**：`RemoteTask` 加 `issue_kind: Option<String>` / `is_ready: Option<bool>`（皆 `#[serde(default)]`）；新增 `HeartbeatAck.pending_readiness_changes: Option<Vec<ReadinessChange>>`（`ReadinessChange{task_id, is_ready}`，`#[serde(default)]`）；新增**唯一**准入谓词 `RemoteTask::executable_ready()`。
- **持久化（先定数据归属）**：`ActiveRemoteRun.issue_kind`（内存，per-claim 快照）+ `MulticaCompletedTask.issue_kind`（StateConfig 终态历史，`#[serde(default)]` 兼容旧条目）。**`is_ready` 刻意不落盘**——终态行没有就绪语义，落盘只会产生永不消费的死字段（`from_completed` 固定 `is_ready: None`）。
- **VM 层（`vm.rs`）**：`RemoteTaskVm` 加 `issueKind` / `isReady`（camelCase 序列化）；`from_remote` 直传、`from_active_run` 直传 issue_kind、`from_completed` 取持久化值 + `isReady: None`。
- **命令层（`commands.rs` / `loop_.rs`）**：claim 成功后、解析工作区前插入准入拦截——不通过则 `release_after_run_start_failure`（CAS dispatched→queued，复用既有回滚路径，无新增接口）并返回 `multica.task-not-ready`；`loop_.rs` 心跳 `Ok(ack)` 分支在 dispatch 前做就绪刷新判定，判定抽成纯函数 `ack_signals_readiness_change(ack) -> bool`（可单测，无需 Tauri `AppHandle`）。
- **三层门控不冗余（各自覆盖不同窗口）**：看板置灰（**显示**：用户根本点不到）、prepare 只读直通（**数据**：拉正文不受就绪影响，用户可见需求内容）、claim 时拦截（**契约**：覆盖 prepare→send 之间父 dev 任务被回退的就绪翻转竞态窗口）。
- **错误码**：新增 `MulticaError::TaskNotReady` → `multica.task-not-ready`（结构体错误码，后端零对客文案；前端 i18n `errors.multica.task-not-ready` zh/en 映射）。
- **前端**：`MulticaRemoteTaskBoard` 导出 `MULTICA_ISSUE_KINDS` / `MULTICA_ISSUE_KIND_TONE` / `visibleIssueKind` / `isTaskExecutable`（与后端 `executable_ready` **同构**）；类型徽标只在 dev/test/bug 渲染（`general` 与未知值不渲染——对齐 multica 自有 views 的 `hideGeneral` 惯例、且新类型不猜文案）；未就绪 test 卡片置灰 + 「未就绪」标记（原因经项目 Tooltip，用既有「span + tabIndex」惯例承载——`Badge` 不转发 ref，不能做 `TooltipTrigger asChild`）+ 执行入口**保留但禁用**（入口消失会被读成任务不可用）。`MulticaTaskManagementPage` 导出 `filterTasksByIssueKind`，页脚新增 shadcn `Select` 类型过滤（all/dev/test，**纯客户端过滤已加载列表**，瞬时状态不持久化；bug/general 归 all 可见）；i18n zh/en 各补 `issueKind.{dev,test,bug}` / `issueKindFilter.{label,all,dev,test}` / `readiness.{notReady,notReadyHint}` + 新错误码文案。

**方案自评审**：
- **过度设计**：零新增 aggregate / 状态机 / 队列 / 缓存 / 持久字段类型——`issue_kind` 只是既有 `ActiveRemoteRun`/`MulticaCompletedTask` 上加一个透传字段；就绪刷新复用**既有**事件与**既有**重拉路径（不新增轮询、不新增订阅通道）；claim 回滚复用**既有** `release_after_run_start_failure`；过滤是 `useMemo` 投影（不新增索引/不复制 canonical 列表）。`is_ready` 不落盘即为「现有事实足够时不复制数据模型」的直接体现。
- **性能**：无新增 I/O 频率（就绪刷新搭在既有 15s 心跳 tick 上，且**仅 diff 非空**才触发重拉）；过滤 O(已加载任务数) 且 `useMemo` 依赖 `[workspaceTasks, issueKindFilter]`，无每渲染重算；claim 拦截是一次 **O(1)** 比较，且在解析工作区/建目录**之前**返回，反而省掉一次无谓的失败路径准备；卡片新增字段读取为常量级。无全量扫描 / N+1 / 无界缓存 / 长持锁。无需 benchmark。
- **依赖与破坏式更新**：不新增依赖；不建兼容层（旧字段形状未变，新字段纯增量）。

**验证**：
- **Rust 定向单测** `cargo test --manifest-path src-tauri/Cargo.toml --bin gold-band-desktop multica::` → **127 过 / 0 失败**。新增覆盖：`RemoteTask` 解析两字段 / 缺字段仍可执行 / `executable_ready` 遍历 dev·bug·general + test(true/false/缺失)；`HeartbeatAck` diff 解析与缺失两态；`ClaimResponse`（denied/allowed/dev 三例）与 `TasksListResponse`（Wrapped/Bare 两形态）承载就绪字段；`ack_signals_readiness_change`（非空 true / `[]` false / 缺失 false）；`RemoteTaskVm::from_pending_passes_through_test_readiness`；`MulticaCompletedTask` JSON roundtrip + 旧条目（无键）→ None。
- **前端**：`npx vitest run --config web/vitest.config.ts web/tests/multica-*` → **8 套件 / 69 过**（含 board 19 + page 19 + requirements-i18n 3 及 5 套既有 multica 套件零回归）。board 新增类型徽标渲染（dev/test/bug 渲染、general 不渲染）、未就绪置灰 + 执行按钮禁用且不触发 `onPrepare`、就绪 test 可执行、dev 即使 `isReady=false` 仍可执行；page 新增类型过滤切换往返（all→dev→test→all，且不产生额外拉取）、无 workspace 时过滤入口不渲染、`filterTasksByIssueKind` 四则纯函数不变量；requirements-i18n 新增类型/过滤/就绪/新错误码的**双语精确值**断言（文案是用户可见验收面，防漂移成 key 或英文兜底）。
- `npx tsc -p web/tsconfig.build.json --noEmit` 零错（顺带修 `api/browser.ts` 的 `getMulticaTaskRequirement` 浏览器桩缺两字段）；`npm run web:build` 生产构建通过（构建产物中的主题生成文件仅 CRLF 行尾差异，已还原不入库）。
- **未做的验证（如实披露）**：**未跑浏览器视觉验收**。本会话无内置浏览器工具，`agent-browser` CLI 亦未安装（`command not found`，其 Windows CDP helper 脚本属另一用户路径）；且该页面的看板数据必须来自已连接的 multica server，而内网地址在本机不可达（`WB Multica 服务地址` 连接超时），故类型徽标 / 未就绪置灰 / 类型过滤三种新状态无法在此环境呈现。上述状态的验收证据为 **jsdom 渲染测试 + 双语精确值断言 + tsc + 生产构建**；**双主题视觉（sky/violet/destructive 三色调、`opacity-60` 置灰）仍待用户在桌面端连上 multica 后目视确认**。
- **未覆盖项（如实披露）**：`commands.rs` 的 claim 拦截**接线**无自动化覆盖——该模块无 HTTP stub 基建（既有测试全为纯逻辑，唯一 stub 先例在 `metrics/heartbeat.rs` 且未设 `no_proxy`，有环境代理抖动风险）。故契约改由**包装层**固化（`ClaimResponse` / `TasksListResponse` 解析 + `executable_ready`），拦截动作本身（含回滚调用）依赖人工验收与代码评审。若后续需要端到端，应先补 `commands.rs` 的 stub-server 基建而非为单点加特例。

**待 multica 侧最终确认（不阻塞码灵上线）**：心跳 ack 的就绪 diff 线格式 `pending_readiness_changes: [{task_id, is_ready}]` 仍是码灵侧提议方案，multica 侧尚未给出最终定义。若最终字段名/形状不同，**仅需改 `client.rs` 的反序列化**（后续消费路径与 `ack_signals_readiness_change` 判定不变）——这是把它做成独立 `ReadinessChange` 类型而非就地解析的原因。两条附加约束见 §12.41（diff 必须是**增量**；claim 路径**必须**回填 `is_ready`）。

---

### 12.41 改动三十九：story dev/test 拆分评审整改——ack fail-soft / 拦截路径刷新事件 / C1 终态快照固化（M5-ba 收尾，2026-09-11）

**背景**：§12.40 上线后用户实测通过（类型徽标 / 类型过滤 / 未就绪在连接 multica 的桌面端均已生效；首帧缺字段、经一次心跳收敛的现象见文末），随后做**独立代码评审**（独立评审 agent + 改动人逐条复核），产出 6 条问题。**根因分类：全部为「设计正确、实现不完整」**——无设计缺陷、无业务逻辑错误、无补丁式修复需求：三层门控、diff 作信号、`is_ready` 不落盘等设计均在评审中被逐条验证成立（含"旧 server 逐字节一致"与"diff 值在测试外零读取"两项独立确认）。整改如下。

**① 心跳 ack 无法 fail-soft（实现不完整）→ 修复**
- **根因与路径**：`ReadinessChange { task_id: String, is_ready: bool }` 内层字段无 serde 缺省，而 ack 是**整条**反序列化（`client.rs` `heartbeat()` 的 `resp.json::<HeartbeatAck>()`）。该字段的 wire 形态**尚未定稿**（S4 仍是码灵提案），一旦服务端键名/取值与提案不一致，失败的不是这个可选字段而是**整条 ack**：`loop_.rs` 落进通用 `multica heartbeat failed (will retry next tick)` 分支，**并连带跳过同一 tick 的 `dispatch_pending_skill_work`**——新字段把既有 skill 双向同步能力连坐，且日志无法归因。
- **修复**：两字段加 `#[serde(default)]`（`ReadinessChange` 加 `Default`）。码灵只消费「diff 是否非空」，条目内容不参与任何判定，故缺省值不改变语义；未知键本就被 serde 忽略，外层键名不同只会退化成 `None`（无信号），同样是 fail-soft。
- **最小失败测试**：`heartbeat_ack_tolerates_diverged_readiness_change_shape`——先红（`Error("missing field is_ready", line: 1, column: 61)`，精确复现"整条 ack 解码失败"），修复后转绿；并覆盖 camelCase 键名（`taskId`/`isReady`）退化为"有变化但内容不可用"。

**② claim 拦截路径缺刷新事件（实现不完整）→ 修复**
- **根因与路径**：`MulticaError::TaskNotReady` 的注释承诺"前端按此码提示…**并刷新看板**"，但拦截路径只 `release` 后 `return Err`，**没有** emit（同函数其他失败分支在回滚后均发 `multica-task-updated`）。后果：能让该错误码出现的唯一前提就是调用方缓存的 `is_ready` 已过期（看板把它当可执行），不发事件则该过期投影会一直留在任何已挂载的消费端，用户看到"按钮可点 → 领取必失败"，与错误码文案自相矛盾。
- **修复**：`release_after_run_start_failure(...).await` 之后补 `emit_multica_task_updated(&app_handle)`，与同函数失败分支惯例一致（remote 状态已回滚，必须通知刷新）。
- **未覆盖项（沿用 §12.40 披露）**：该 emit 与拦截动作本身仍无自动化覆盖（`commands.rs` 无 HTTP stub 基建），依赖代码评审 + 人工验收。

**③ C1 终态快照无单测固化（验收未固化）→ 补测**
- **根因与路径**：`MulticaCompletedTask.issue_kind` 的唯一正确来源是 `run.issue_kind`（claim 落盘的类型快照），但原实现是 `finalize_terminal` 内的内联结构体字面量——编译器只强制该字段**被赋值**、不强制取值**来源**，改成 `None` 时全部测试仍绿而终态行类型徽标静默丢失（而 `finalize_terminal` 因依赖 `AppHandle` 无法直测）。
- **修复**：抽出纯函数 `completed_task_from_run(remote_task_id, run, status, completed_at)`（不碰 AppHandle/StateConfig，`finalize_terminal` 改为调用它），新增 `completed_task_from_run_keeps_claim_kind_snapshot`：固化四类 kind 透传、无 kind（旧数据/无 issue 任务）→ `None` 不臆造、标题空/纯空白 → 回退 `remote_task_id`。

**④ 文档与注释订正**
- 两张错误码表补齐 `multica.task-not-ready`：开发设计第 5 章（含前端处理列）、接入方案码清单。
- 修正 `MulticaTaskManagementPage` 的过滤注释：原文"旧 server 无类型字段时**不过滤掉任何任务**"与实现相反——旧 server 下所有任务 `issueKind === null`，选开发/测试会把任务**全部过滤掉**（默认「全部」，故用户无感，属可接受的既定行为，但注释不能写成不存在的性质）。
- 接入方案 §5.1 S3 补**服务端上线的实测证据**（2026-09-11 客户端实测 pending 已带 `issue_kind`，徽标/过滤生效）。

**新增契约约束（待 multica 确认，已记入接入方案 §5.1 S3）**
- **claim 路径必须回填 `is_ready`**：门控判定的是 claim 响应里的字段。若 claim 只回 `issue_kind` 不回 `is_ready`，保守缺省判未就绪 → **全部 test 任务不可领**。方向本身正确（服务端零特判下客户端拦截是唯一防线，不放宽为"缺字段即放行"），但必须作为硬约束确认，否则症状是"按钮可点、领取必失败且提示误导"。
- **心跳 diff 必须是增量**：客户端无法区分"增量 diff"与"全量集合"，若服务端每 tick 下发全量，则流量变成每 15s 一次全量 pending 重取。约束应固化在契约侧，而非在客户端加变更检测状态。

**首帧缺字段现象（实测观察，非缺陷）**：用户首次实测时看板无徽标、类型过滤选开发/测试为空、看不到就绪态，一段时间（经一次心跳）后自行恢复正常。逐条核验后的结论——码灵读路径**无缓存、无"稍后补"**，看板只是单次 HTTP 响应的投影，故"先无后有"只可能来自"响应变了"；而该页面的刷新触发源穷举后，空闲态下**唯一**的自动触发源就是心跳 ack 非空 diff（`loop_.rs`）→ 这既解释了收敛，也反证了码灵链路正确（缺投影代码则永远不会出现徽标）与 multica 侧已下发该 diff。剩余解释收敛为服务端发布窗口或多实例滚动，判别方式为"重启后首帧即带徽标（一次性）vs 首个心跳后才有（系统性）"，两者都不需要码灵改动。

**性能影响**：仅两处实现改动且不改变复杂度——拦截路径多一次事件 emit，且只发生在**被拒绝**的领取上（该路径同时省掉了后续工作区解析/建 run，仍比原失败路径更便宜）；`completed_task_from_run` 是纯字段搬运，无分配变化。无新增 I/O 频率、无新增状态、无新增订阅。新增测试为零运行时开销。

**未做（评审后保留项，已知并记录）**：前端 `isTaskExecutable` 与后端 `executable_ready` 仍是两份同构谓词（评审建议收敛为 VM 派生字段 `executableReady` 单源）——本轮不改，避免为一致性在 VM 上新增派生字段；两侧各自有测试锁定同一真值表，漂移风险已知。`src/config/mod.rs` 的 `MulticaCompletedTask.issue_kind` 上 `#[serde(default)]` 对 `Option` 冗余（serde 本就把缺失映射为 `None`），无害保留。

**验证**：`cargo test --manifest-path src-tauri/Cargo.toml --bin gold-band-desktop multica::` → **129 过 / 0 失败**（§12.40 基线 127 + 心跳 fail-soft + 终态快照 2 例）；两个新测试单独复跑确认通过。web 侧本轮无行为改动（仅注释订正），既有 multica 8 套件 / 69 过不受影响（复跑确认 41 + 28 全绿）。

---

### 12.42 改动四十：未就绪 test 任务改为「仅提醒、不阻断」——删除三层门控（M5-bb，2026-09-11）

**背景与定性**：M5-ba 的 fail-closed 门控（看板置灰 + 禁用执行入口 + claim 后拦截回 `multica.task-not-ready`）上线并经用户实测后，产品决策主动放宽：**就绪是建议性信息而非硬前置**——父 dev issue 未 done 时执行 test 任务在业务上可接受，阻断带来的操作成本高于收益。这不是缺陷修复（三层门控经 §12.41 评审验证成立），而是产品行为语义变更；按开发阶段破坏式更新规则，直接删除旧阻断路径，不加兼容层或开关。

**删除（前后端两侧同源删除，杜绝「按钮可点、领取必失败」的单侧残留）**
- **前端**（`MulticaRemoteTaskBoard.tsx`）：去卡片置灰（`opacity-60`）、执行入口 disabled 收敛为仅 `busy`；准入谓词 `isTaskExecutable` 改为展示谓词 `isTaskNotReady`（`issueKind === 'test' && isReady !== true`，只驱动未就绪徽标渲染，与后端无谓词耦合）。
- **后端命令层**（`commands.rs`）：删除 claim 后 `executable_ready()` 拦截块（release 回滚 + `emit_multica_task_updated` + 错误码返回）——不删则前端放行后领取必失败，正是 §12.41 ②所防的症状。claim 后本地建 run 失败的其他回滚路径不变。
- **后端契约层**（`client.rs`）：删除 `RemoteTask::executable_ready()` 及其门控单测（`executable_ready_gates_only_test_kind` / `claim_response_surfaces_readiness_for_gate`）；`issue_kind` / `is_ready` 解析保留（pending → VM → 看板徽标的事实源不变），解析测试改回纯字段断言。
- **错误码**（`error.rs` / i18n）：删除 `MulticaError::TaskNotReady`（variant、码表、`errors.multica.task-not-ready` zh/en 文案及测试断言）。
- **保留**：未就绪徽标 + 原因 Tooltip；文案改为建议语义（zh「对应开发任务尚未完成，建议等待其完成后再执行」/ en "The matching dev task is not done yet; consider waiting for it"）；心跳 ack 就绪 diff 刷新链路不变（S4 落地后徽标仍自动纠正）。

**连带失效的契约约束**：§12.41 记录的「claim 路径必须回填 `is_ready`」随门控移除而失效（`is_ready` 事实源只剩 pending 列表，claim 载荷不再被消费判定）——接入方案 §5.1 S3 已订正。「心跳 diff 必须是增量」约束仍然有效（diff 仍驱动页面重拉 pending）。

**§12.41 保留项的解决**：「前端 `isTaskExecutable` 与后端 `executable_ready` 两份同构谓词」的漂移风险随两侧谓词一并删除而消失，无需再收敛为 VM 派生字段。

**性能影响**：纯删除——claim 拦截路径（一次 release HTTP + 一次事件 emit）整体消失，未就绪任务的领取与普通任务等价；无新增状态、订阅或 I/O。新增/更新测试为零运行时开销。

**验证（2026-09-12）**：Rust `cargo test --manifest-path src-tauri/Cargo.toml --bin gold-band-desktop multica::` **127 过 / 0 失败**（删 3 例门控测试、2 例改回解析断言）；web vitest board / i18n 两套件按新语义更新后 **22 过 / 0 失败**（未就绪任务徽标渲染且执行按钮可点、点击触发 onPrepare；提醒文案双语精确值）；`tsc -p web/tsconfig.build.json` 零错。

---

### 12.43 改动四十一：拉取落库支持多文件 skill——`write_bundle_instance` 整目录镜像写入（M5-bc，2026-09-15）

**背景与定性**：§12.39（M5-az）的拉取链路只落单文件 SKILL.md，多文件远端 skill 被 skipped（`multica.skill.has-files`）——这是 v1 的**有意设计裁剪**（非实现缺陷），但推送侧多文件感知，形成「出得去、进不来」的单向不对称。本轮经用户确认服务端契约后补齐设计（权威设计 `.claude/design/multica_skill/` 2026-09-15 修订版）：**files 列表不含根 SKILL.md**，它单独走 detail 的 `content` 字段（multica 侧每层显式排除——WalkDir 跳过 / daemon 发现排除 / execenv 落盘兜底去重），故码灵侧无需防御性处理 files 混入根 SKILL.md（落库校验拒绝即兜底）。

**写入层（`gold_band::skill` 新增 `write_bundle_instance`，与 `write_instance` 分立）**
- 分立原因：编辑器写入是 **merge 语义**（旧内容 + 增量编辑），拉取覆盖是**镜像语义**（远端是权威事实源，整目录替换），文件集与合并行为均不同，共用签名会迫使两语义互相妥协。
- 镜像覆盖：远端已删的本地支撑文件随之清除（用户确认的「远端镜像」语义）。staged 临时目录（同父同卷，`.mstaging-{目录名}-{pid}`）写满后两步 rename swap（目标→`.mbackup-*`→staging→目标），任一步失败回滚；提交后 backup 先删 SKILL.md 再整体清除（残留对扫描器不可见）。
- 扫描器不可见性：`scan_skills_dir` 只列含 SKILL.md 的目录 → staging **最后写 SKILL.md**、backup 清理**最先删 SKILL.md**。
- agent 同步不变：目录级 symlink 按路径身份工作，目录换目标（swap）对链接透明；`reconcile_skill_instance_links` / `restore_skill_links` / `previous_sync_targets` 回滚契约与 `write_instance` 同款。
- 无 `old_name` 参数：拉取不重命名（推送侧同步更新本地的场景不存在），`current_directory_path` 即覆盖目标（`save_target_dir` 原样返回）。
- 入口校验前置为纯函数 `validate_skill_bundle_entries`：尺寸（单文件 1MiB / 整包 8MiB）、总数（SKILL.md + 支撑 ≤256）、路径安全（无穿越段 / 绝对路径 / 反斜杠 / Windows 非法字符 / 尾点尾空格 / 根 SKILL.md 混入 / 重复路径）、深度 ≤4——超限整体失败不部分写入。

**限额单一事实源**：`SKILL_BUNDLE_MAX_FILE_BYTES/MAX_BUNDLE_BYTES/MAX_FILES/MAX_DEPTH` 上移 `gold_band::skill`，推送打包（`local_skills.rs`）与拉取落库共用；桌面端 `IMPORT_MAX_*` 改为 re-export（消除两份常量漂移）。

**client / 命令层**
- `list_remote_skill_files` → `get_remote_skill_files`：路径 `/api/skills/{id}/files`（服务端默认 `includeContent=true` 返回正文，去掉原 `?include=metadata`）；`RemoteSkillFileMeta` → `RemoteSkillFile` 含 `#[serde(default)] content`（size 保留仅作 wire 契约文档）；wrapped/bare 容错不变。
- `pull_multica_skills`：detail → files（含正文）→ 目录名清洗 + 批内 seen 去重 → `spawn_blocking(write_bundle_instance)`；404 → skipped not-found、网络 → failed remote-error、写失败（含超限/路径违规）→ failed 原始错误透传。
- **`multica.skill.has-files` 错误码退役**（破坏式更新）：拉取分支、VM 注释、i18n 双语 reason 映射、i18n 测试数组一并删除。

**性能影响**：拉取为用户手动触发的低频操作；每 skill 多一次 files 请求，受 8MiB 整包上限硬约束；写入走同卷 rename（O(1) 元数据操作），staging 写入是 ≤255 文件的顺序 I/O，数据规模上界明确。前端零新增渲染路径（纯删除）。

**验证（2026-09-15）**：lib `cargo test -p gold-band --lib` 全量 **1189 过 / 0 失败**（含 skill:: 20 项，bundle 新增 4 项：镜像落盘、覆盖删除 stale 文件且 symlink 不失效、当前目录缺失失败、校验拒绝越权路径/超限条目）；desktop multica:: **127 过 / 0 失败**；`tsc -p web/tsconfig.build.json` 零错；web vitest multica **8 套件 69 过**。**顺带修复**：HEAD 既有的两处 lib 测试夹具编译错（`MulticaCompletedTask` 缺 `issue_kind`，M5-ba 加字段时漏改 `src/app/mod.rs` / `src/config/mod.rs`）。

**评审整改（2026-09-15，实现完成后独立 review，三条）**
- **测试断言失效**：两处「staging/backup 无残留」断言用 `Utf8PathBuf::iter()`——迭代的是**路径组件**而非目录条目，恒真 → 改 `fs::read_dir` 逐条目检查（`assert_no_staging_leftovers`）。
- **崩溃残留清扫改前缀匹配**（`remove_stale_bundle_staging`，任意 pid）：原 pid 隔离清扫在进程崩溃重启后永远够不到残留，而「SKILL.md 已写完、swap 未完成」的崩溃残留含 SKILL.md、对 `scan_skills_dir` **可见**（其不跳点前缀目录）→ 幽灵 skill 永久残留。前缀按 skill 目录名隔离；单实例 app（`tauri-plugin-single-instance`）+ 拉取弹窗 UI 串行化保证无并发写同一 skill 的在飞 staging 被误删。
- **评审测试抓到真 bug**：`write_staged_bundle` 对零支撑文件的单文件 skill 从不创建 staging 目录（`create_dir_all` 只由支撑文件父目录触发，原 4 项 bundle 测试都有支撑文件未覆盖）→ 最常见的单文件远端 skill 拉取必失败（os error 3）。补 staging 首行 `create_dir_all` 修复。
- 回归：skill:: **21 过 / 0 失败**（新增 `bundle_write_sweeps_crash_leftovers_across_pids`：跨 pid 残留被清除、其他 skill 残留不受影响，同时固化零支撑文件路径）；desktop multica:: **127 过 / 0 失败**。

---

### 12.44 改动四十二：同步弹窗选择阶段滚动修复与全选开关（M5-bd，2026-09-15）

**背景与根因**：skill 过多时「从 Multica 同步」弹窗被拉长且无法滑动——选择阶段 body 容器（`min-h-0 flex-1 space-y-3`）不是 flex 容器，子级 ScrollArea 的 `min-h-0 flex-1` 在非 flex 父级下失效，列表自然高度溢出被 DialogContent 的 `overflow-hidden` 裁掉（无滚动条出现）。报告阶段本就正确（body 自身 `overflow-y-auto`）。定性为「好设计但实现不完善」：ui-interaction.md §5.1 固定头/中滚动/固定脚的布局原则在选择阶段漏配了 flex 链，非设计缺陷。同时用户要求补全选开关（只想同步个别 skill 时逐个取消太繁琐）。

**修复（`MulticaSkillSyncDialog.tsx`，纯前端，零后端改动）**
- flex 链：选择阶段 body 改 `flex min-h-0 flex-1 flex-col gap-3`；工作区选择行与全选行 `shrink-0`；ScrollArea 保持 `min-h-0 flex-1` 成为唯一滚动区（列表过长时内部滚动，弹窗 `max-h-[85vh]` 封顶）。
- 全选三态开关（列表非空且非 loading 时显示，同步进行中禁用）：shadcn Checkbox `checked` 三态——全选 `true` / 部分 `indeterminate`（默认「新增勾选、已存在不勾」）/ 全不选 `false`；点一次全选、再点一次全不选（连点两下即清空，再单独勾选想要的），旁附已选计数。状态仍收敛在 `selected` 单一事实源，全选是 O(n) 批量投影（`handleToggleSelectAll`），无新增状态机/派生 state。
- i18n：`contextManagement.skills.multicaSync.selectAll` / `selectedCount`（双语 + 测试键数组 + 插值断言同步）。

**性能评审**：无新增渲染路径与数据加载；全选一次 O(n) setState（n = 远端 skill 数，几十量级）；无虚拟化需求（ui-interaction.md §6，短列表不虚拟化）。复用既有 shadcn Checkbox/ScrollArea，无新组件。

**验证（2026-09-15）**：`tsc -p web/tsconfig.build.json` 零错；vitest 新增 `web/tests/multica-skill-sync-dialog.test.tsx` 2 例——① 滚动区 flex 契约（ScrollArea `min-h-0`+`flex-1`）；② 三态全选交互（默认 indeterminate → 全选 → 全不选且同步按钮禁用 → 单选后 `pullMulticaSkills('ws-1', ['s-exists'])` 仅拉取勾中项）；`multica-skill-sync-i18n.test.ts` 补 2 键与双插值断言。multica 相关套件全过（全量 vitest 另有 4 项分支既有失败，涉及文件均未触碰：ConversationSidebar/AgentManagementPage 原生 title 契约、Composer onPaste 源契约、App agent-registry listen 源契约、acp-activity scrollIntoView 行为）。浏览器视觉验证本会话不可用（M5-ba 先例：留待用户桌面端目验，重点：长列表滚动 + 全选三态样式）。

**评审整改（2026-09-15，实现完成后独立 review，三条）**
- **indeterminate 视觉缺陷**：copy-in `checkbox.tsx` 原版只渲染 CheckIcon、只有 `data-[state=checked]` 主题色样式；Radix Indicator 对 indeterminate 也渲染 → 全选部分勾选态显示「无填充方框里的对勾」，无减号、无 accent（违反 ui-interaction.md §8 选中态用主题色）。组件测试把 Checkbox mock 掉了故未抓到。修复：copy-in 补 `MinusIcon`（`props.checked === "indeterminate"` 条件渲染）+ `data-[state=indeterminate]` 主题色填充（含 dark 变体），Root 的 props spread 不变、boolean/未传 checked 的既有消费方零行为变化；新增 `web/tests/checkbox-indeterminate.test.tsx` 3 例固化真实渲染契约（indeterminate→minus+accent、checked→check、unchecked→无 Indicator）。
- **死键删除**：`multicaSync.pulling` 双语定义 + i18n 测试 KEYS 断言俱在，但无任何组件使用（同步按钮 loading 只渲染 Loader2 图标）——按开发阶段破坏式更新原则删除（i18n 双语 + KEYS 数组）。
- **测试死分支**：dialog 测试 `syncButton` 帮助函数里永不匹配的 `${BASE}.pulling` 文本分支一并移除。
- 回归：`tsc` 零错；vitest 3 套件（checkbox-indeterminate / multica-skill-sync-i18n / multica-skill-sync-dialog）**8 例全过**；全量 vitest 仍仅 4 项分支既有失败，无新增。

**用户实测反馈整改（2026-09-15，第二迭代：列表仍不滚动）**
- **根因**：flex 链修复方向正确但不充分——列表滚动走 Radix ScrollArea，真正滚动的 viewport 是 `height:100%`（size-full），其高度依赖父级（ScrollArea Root，自身无 overflow）经 flex 收缩得到的尺寸。在「高度 auto + `max-h-[85vh]` 封顶」的弹窗容器内，孙子级百分比高度解析不可靠：视口塌到内容高度、Root 无裁剪 → 溢出内容被 DialogContent 的 overflow-hidden 裁掉且无任何滚动。判定依据：代码库所有弹窗（AddWorkspace / ConnectionSettings / 本弹窗报告阶段 / ConversationSearch）均把 `overflow-y-auto` 直接放 flex item，ScrollArea 从未用于弹窗内此位置（页面内 ScrollArea 均处于确定高度布局或自带 max-h/h-* 约束）。
- **修复（`MulticaSkillSyncDialog.tsx`）**：列表容器 ScrollArea → plain div（`gold-themed-scrollbar min-h-0 flex-1 overflow-y-auto rounded-md border`），滚动直接挂在 flex item 上（与 AddWorkspace / 报告阶段同款已验证模式）；报告阶段滚动体补 `gold-themed-scrollbar`（同弹窗滚动条视觉一致）；删除 ScrollArea import。
- **测试同步**：`multica-skill-sync-dialog.test.tsx` 移除 ScrollArea 桩，结构契约改写为「滚动容器必须是 flex item 本身（overflow-y-auto + min-h-0 + flex-1 + gold-themed-scrollbar），不得经 size-full 视口百分比链」。
- **验证局限（诚实记录）**：jsdom 无布局引擎，滚动行为无法自动化复现（bug-fix-verification 规则的替代证据：结构契约测试 + 桌面端目验）。`tsc` 零错，vitest 3 套件 8 例过。

### 12.45 改动四十三：远程来源解耦 P1——词表与组件改名（M5-be，2026-09-17）

**背景与根因**：P1 三项断连/重连排查（①定时器空转断连 ②重连后列表不清空 ③multica 状态编辑不回传客户端）最终判定全部为服务端语义或既有设计（webank 源码逐条对证），客户端忠实。但排查过程暴露出真正的工程问题：**前端词表与组件命名以 multica 品牌散写**——i18n 文案直接写死「Multica」、组件名 `Multica*`、类型 `MulticaTask`、命令 `multica_*`，来源信息本应是「数据」却被编译进「代码」，换来源 = 全库改代码。

**设计（三条不变量）**：① 方言不出模块（品牌词只允许出现在 adapter 模块内部）；② 按名片渲染不按来源特判（消费方只读来源注册表名片，不写 `if source === 'multica'`）；③ 任务与 skill 来源互不感知。

**实现**
- P1a（Rust）：`src-tauri/src/multica/` → `src-tauri/src/remote/`，模块内 `multica_*` 命令保留（adapter 域白名单：`disconnect_multica`、`connect_multica` 等），对外词汇统一 remote。命令更名（如 `list_multica_tasks`→`list_remote_tasks`）与前端同 commit 提交保证 bisect 绿。
- P1b（前端）：新增 `web/src/lib/remote-sources.ts`——`REMOTE_TASK_SOURCES` 注册表、`isRemoteTaskSource` 类型守卫、`remoteTaskSourceLabel(t, source)` 名片解析；新增 `web/src/lib/app-events.ts` 收敛事件常量。组件改名 `MulticaRemoteTaskBoard`→`RemoteTaskBoard`、`MulticaTaskManagementPage`→`RemoteTaskManagementPage`、`MulticaSkillSyncDialog`→`RemoteSkillSyncDialog`（文件同步 rename）。
- 一 commit 合并 P1a+P1b（7085a5a7，62 文件），已 push。

**验证（2026-09-17）**：`tsc -p web/tsconfig.build.json` + demo 双零错；vitest multica/remote 相关套件全过；①②③ 由用户桌面端实测确认（服务端语义结论，非回归）。

**遗留（记入 P2/P3）**：skill 页文案仍含品牌名（无前端 skill 来源字段，P2 随 SkillSource 提取一并处理）；`errors.remote.*` 错误文案品牌名（displayAppError 路径无来源上下文，需传 wire 改动）；`disconnect_multica` 后端命令暂留（adapter 域白名单，P3 清理）；TaskSource 整合与 capabilities 卡片为 P3。

---

### 12.46 改动四十四：需求管理页产品形态一期——{{source}} 参数化与账号菜单移除（M5-bf，2026-09-17）

**背景**：P1 重命名完成后，用户提出产品形态问题：需求管理/skill 管理页如何展示得不与 multica 耦合过深。定调为「来源名对用户是数据不是代码」——所有用户可见的来源名一律走注册表名片（`remoteTaskSourceLabel`），i18n 文案用 `{{source}}` 插值，不在文案里散写品牌名。

**实现（`RemoteTaskManagementPage.tsx` / `MulticaConnectDialog.tsx` / `web/src/i18n.ts`）**
- {{source}} 参数化 8 处双语：空状态 `emptyTitle`「尚未连接{{source}}」/ `emptyDescription`「连接后可在此查看并领取{{source}}需求。」/ `connectButton`「连接{{source}}」；连接弹窗 `remote.connect.title`「连接{{source}}」。页面在 `accountLabel` 后新增 `sourceLabel = remoteTaskSourceLabel(t, source)` 作为插值唯一来源；`MulticaConnectDialog` 新增必填 prop `sourceLabel`（弹窗内不散写品牌名，测试同步补传）。
- 账号菜单移除（产品决策）：右下角「切换账号/断开连接」下拉删除——用户只在首次连接远程，后续无切换/断开场景；换账号/换地址走连接与地址设置弹窗即可覆盖。footer 改为纯展示账号标识（User 图标 + truncate 账号名）。i18n 按开发阶段破坏式更新删除 `account.switchAccount`/`account.disconnect` 键（保留 `connected`）。
- 后端 `disconnect_multica` 命令与 API wrapper **保留**（adapter 域白名单，避免刚 push 的 P1 commit 之后再翻动 Rust 层；P3 统一清理）。

**明确不做（过度设计防护）**：设置页不加「远程来源」集中管理区（当前单来源在架，集中管理是假设性需求）；skill 页文案参数化推 P2（无前端 skill 来源 wire 字段）；错误文案参数化推后续（displayAppError 无来源上下文，需改 wire）。

**性能评审**：无新增渲染路径/数据加载；`remoteTaskSourceLabel` 为同步查表 O(1)；footer 从 DropdownMenu（含 Radix portal）简化为纯 span，渲染成本反降。

**验证（2026-09-17）**：`tsc -p web/tsconfig.build.json` + demo 双零错；vitest 4 套件 48 例全过（含 multica-connect-dialog 补 `sourceLabel` prop 后全绿）。视觉验证按 M5-ba 先例留待用户桌面端目验（重点：空状态/连接弹窗的来源名插值、footer 纯账号标识无下拉）。

---

### 12.47 改动四十五：skill 管理页来源解耦一期——通用同步入口与弹窗内来源选择（M5-bg，2026-09-17）

**背景与根因**：M5-bf 遗留的 skill 页品牌耦合（按钮「从 Multica 同步」、弹窗标题、未连接提示、失败原因共 4 处散写品牌名）。用户设计决策指明方向：来源是 skill 同步的**选择维度**而非文案参数——「任务连着来源 A、skill 想从来源 B 同步」的场景下，{{source}} 参数化无法表达；正确形态是通用同步按钮 + 弹窗内来源选择器（与需求管理页 A6 来源选择器同款模式：注册表驱动、单来源也显示）。

**实现（`RemoteSkillSyncDialog.tsx` / `web/src/i18n.ts`，纯前端零 wire 改动）**
- 文案通用化（双语）：`remoteSync.action`「同步远程 SKILL」、`remoteSync.title`「从远程同步 SKILL」、`reason["multica.not-connected"]`「未连接远程来源」——文案零来源名，来源名只作为选择器数据出现。
- 弹窗顶部来源选择器：`REMOTE_TASK_SOURCES.map` 渲染，复用 `remote.taskManagement.source.label`；`handleSourceChange` 换来源时重置工作空间与勾选（fallback effect 重选默认空间后列表 effect 重拉）。
- 未连接分支升级：`notConnected`「未连接{{source}}」+ 新增 `connect`「连接{{source}}」按钮，就地打开 `MulticaConnectDialog`（复用 M5-bf 的 `sourceLabel` prop 与弹窗内已持有的 settingsVm）；`onConnected` 重拉设置（`fetchSettings` 单一 helper 收口），工作空间/列表级联刷新。顺带修复原文案指向已改名页面（「远程任务」→ 现名「需求管理」）的死引导。
- **wire 分期决策**：`list_remote_skills`/`pull_remote_skills` 的 source 参数随 P2 落地——当前选择器值恒为 'multica'（单来源注册表），单独加参数是恒传单值的伪接缝；P2 随第二来源接入时命令参数 + 按来源路由 + per-source 连接状态同批落地。前端接缝：设置拉取收敛在 `fetchSettings`，P2 按 source 分发只扩该 helper。

**性能评审**：零新增数据加载路径（连接成功后的重拉复用既有 getMulticaSettings 单次调用）；来源选择器为静态注册表渲染 O(1)；未连接分支复用弹窗内已持有 settingsVm，无额外请求。无性能风险。

**验证（2026-09-17）**：`tsc -p web/tsconfig.build.json` 零错（tsconfig.json 全量检查的报错均在未触碰文件，属既有环境噪音）；vitest 3 套件（remote-skill-sync-dialog / remote-skill-sync-i18n / multica-connect-dialog）17 例全过——新增：①未连接 → 来源选择器 + 就地连接按钮 → 连接弹窗携带注册表名片 sourceLabel → 连接成功重拉设置、未连接分支消失（全链路契约）；②notConnected/connect 的 {{source}} 双语插值断言。i18n 测试 KEYS 数组补 `connect` 键。视觉验证按 M5-ba 先例留待用户桌面端目验（重点：来源选择器单来源显示、未连接分支就地连接、层叠弹窗交互）。

### 12.48 改动四十六：任务接口 DPMS 溯源字段对接——会话起始输入前缀溯源块（M5-bh，2026-09-17）

**背景**：webank dev 分支 `feat(daemon): 任务接口补齐 DPMS溯源字段` 给 `AgentTaskResponse` 新增 5 个可空溯源字段（`release_plan_id` / `dev_user` / `test_user` / `business_story_id` / `origin_url`，issue 行镜像，pending / detail / claim 三接口同构，对接文档 `.claude/design/multica_issue_management/2026-09-17-daemon-task-dpms-fields-api.md`）。码灵侧对接目标：issue 任务进会话时，起始输入开头先写 DPMS 溯源信息、再写 issue 内容，统一作为会话首条输入。

**根因定性**：好设计、实现待扩展——claim-at-send 预填链（`get_remote_task_requirement` → `from_detail` → composer 预填 → `input.requirement` 即会话首条输入）早已把「详情正文 → 会话起始输入」打通，DPMS 溯源只是在该链路的 `requirement` 组装点做前缀拼接，前端与发送链路零改动。

**实现（三 layer）**
- **Layer 1 wire 解析（`client.rs`）**：`RemoteTask` 加 5 个 `#[serde(default)]` 可空字段。列 NULL → 缺 key → None（不报错、不回退默认值）；纯溯源语义，不参与 `is_ready`/`issue_kind` 门控判定。旧 server / 非 issue 任务（chat / autopilot / quick-create）恒 None（版本解耦）。
- **Layer 2 提示词模板（`src/prompts/{zh-CN,en}/runtime/remote_task_context.md`）**：双语同构、逐字段 `{% if %}` 条件渲染（minijinja strict，字段恒序列化为 null/值）；`src/prompts.rs` 注册 `RUNTIME_REMOTE_TASK_CONTEXT_{ZH_CN,EN}`，复用 `prompt_by_language` + `render`（与 `scheduled_task_context` 同款机制）。中文标签：发布计划 ID / 开发负责人 / 测试负责人 / 业务需求 ID / 需求链接。
- **Layer 3 组装（`vm.rs`）**：`from_detail` 增 `language: DesktopLanguage` 参数（调用方 `get_remote_task_requirement` 取 `context.config.desktop_language`）；`session_start_input` = 溯源块（`dpms_context_block`，5 字段全缺省/纯空白 → None）+ 空行 + `requirement_text()` 正文。字符串字段逐个空白过滤；正文缺失但溯源存在时只发块。`requirement_text()` 来源优先级不动——溯源块是上下文前缀，不是需求来源。（**M5-bk 反转并取代，二次修正后部分回到本形态**：`session_start_input` 已删除、`language` 参数经 M5-bk 移除后于二次修正中恢复；溯源块回到 composer 预填（`detail_prefill`，放需求正文之前），三次调整后上游交付说明块也进预填、仅完成输出协议留在隐式区段，见 §12.50 末「二次修正」「三次调整」。本行「正文缺失但溯源存在时只发块」语义在二次修正后重新生效。）

**设计判断**：溯源块放 user prompt（会话起始输入）而非 hidden context（`scheduled_task_context` 先例路径）——DPMS 溯源是工作项自身的需求上下文（随任务变化、与执行目标直接相关），按 AGENTS.md system/user prompt 划分标准归 user prompt，且用户与 agent 都需在会话内可见。发送时不从 claim 响应二次注入：预填即起始输入，用户编辑/删除是明确意图，二次注入属重复机制。（**M5-bk 反转、二次修正后回到本判断**：M5-bk 一度把三块一律判为隐式侧，属分层判断过度收敛——DPMS 溯源确为用户需在发送前核对的目标环境信息（user 侧）。三次调整后仅写侧协议指令必须留隐式侧（误删即静默断链），上游交付说明按同一分层标准划归 user 侧（子任务赖以执行的父交付内容，用户同样需在发送前核对）。当前形态 = 溯源块 + 上游交付说明可见预填 + 仅协议块隐式区段，见 §12.50 末「二次修正」「三次调整」。）

**性能评审**：一次「认领执行」预填多一次 minijinja 模板渲染（微秒级纯内存字符串操作），无新增 I/O / 状态订阅 / 数据加载路径；前端零改动。无性能风险。

**验证（2026-09-17）**：lib `remote_task_context_templates_render_all_and_partial_fields` 过（全字段/部分字段渲染 + 中英文标签锁定 + 无空行残留）；desktop multica 单测新增 `remote_task_parses_dpms_provenance_fields` / `remote_task_missing_dpms_fields_parse_as_none`（wire 契约：有值/缺 key）与 `from_detail_prepends_dpms_context_block_to_requirement` / `from_detail_without_dpms_fields_keeps_plain_requirement`（前缀拼接 / 无字段退化 / 空白过滤 / 双语）全过。**无需 webank server 改动**（server 已随 dev 分支发版）。

---

### 12.49 改动四十七：会话主页 composer 滚动收口迁移——multica chip 随正文滚动（M5-bi，2026-09-17）

**背景**：用户实测发现，issue 任务认领进会话后，输入框最前的 multica chip 一直悬浮在输入框左上角；正文较长需要滚动时，chip 保持悬浮遮挡后续内容行。

**根因定性**：好设计（leading adornment 首行内嵌 + text-indent 让位）、实现不完整——chip 绝对定位挂在非滚动的包装 div 上，而滚动容器是 textarea 自身（autosize 到 `maxHeight` 上限后 `overflow-y: auto`）。adornment 的定位语义是「正文首行」，却挂在视口层，滚动时首行移出视野而 chip 不动，两者脱离。text-indent 只能让位首行，天然无法覆盖滚动态。

**实现（结构统一，非补丁式 JS 滚动同步）**
- **prompt-kit copy-in（`prompt-input.tsx`）**：`maxHeight` 类型扩为 `number | string | null`，`null` 语义 = 自适应不设上限、textarea 自身永不滚动（`height: scrollHeight, overflowY: hidden`），滚动收口交由祖先容器。纯增量，既有 number/string 行为不变。
- **布局常量（`conversation-composer-layout.ts`）**：`textareaMaxHeightPx: 320` 删除，新增 `inputScrollContainerClassName: 'relative min-w-0 max-h-80 overflow-y-auto'`（max-h-80 = 320px，上限原值迁移，杜绝双处硬编码）。
- **ConversationComposer**：`PromptInput maxHeight={null}`；chip / 斜杠标签所在的包装 div 改用 `inputScrollContainerClassName`。chip 仍在包装层内绝对定位（`absolute left-0 top-2`），随正文首行一起滚出视野；斜杠命令标签共用同一 slot 同步修复。

**影响面**：仅会话主页 `ConversationComposer`（ACP 会话为独立组件 + 独立布局常量，不受影响）；斜杠命令菜单 inline 弹层是包装层兄弟节点（挂 SlashCommandMenu 外层 root），不被滚动容器裁剪，行为不变。

**性能评审**：纯 CSS/结构迁移，无新增 JS 监听、无滚动同步逻辑、无重渲染路径变化；autosize 仍是一次 `scrollHeight` 读写（原机制不变，只是不再封顶）。无性能风险。

**验证（2026-09-17）**：`conversation-composer-autosize.test.ts` 契约更新并新增用例——null maxHeight 不封顶不滚动、composer `maxHeight={null}` 且滚动容器为包装层、chip span 位于滚动容器内部（源码契约）；浏览器（deep link 会话主页）注入 40 行文本实测：textarea 976px 不封顶 `overflow-y: hidden`、包装层 320px 收口接管滚动、chip 随滚动移出视野（offset +8px → -492px）；`tsc -p web/tsconfig.build.json` + 生产构建 + 全量 vitest 过。顺手修正 `composer-context-alignment.test.ts` 中 HEAD 上已陈旧的 onPaste 契约断言（readOnly 演示守卫早已合入但测试未同步）。

---

### 12.50 改动四十八：远程任务上下文隐式区段下发 + 完成输出协议提示词精简（M5-bk，2026-09-21）

**背景（用户内网实测两点）**：① 完成输出协议提示词（`src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md`）冗长——多段说明性文字；② 该协议块与 DPMS 溯源块、上游交付说明块一起，以四段拼装（改动四十六/四十七）**全量预填进 composer**：输入框里躺着三块内部上下文，且**可编辑、可删除**——误删协议块即静默破坏写侧契约（agent 不再产出 `completion-output`，issue done 时无输出可中继，无任何提示）。用户问：「码灵是否支持把这类的 prompt 不显式地在输入框展示出来，而是隐含地给到模型？」

**先例检索（实施前，先看有没有现成机制）**：有，且是项目内既有的成熟通道——`gold_band_hidden_block(title, content)`（`provider/mod.rs`）产出 `<hidden data-gold-band-hidden="true" title="...">` 块，已被三处同类需求使用：`RUNTIME_HIDDEN_CONTEXT`（基础运行上下文）、AI-DYNAMIC 运行上下文、`scheduled_task_context`（定时任务上下文——与 `parent_output` 同形：任务变化带来的外部输入）；前端 `parseGoldBandHiddenSections` 把这些块渲染为用户气泡内**默认折叠的模块**（acp-ui.md:186）。即：通道现成（`PromptHiddenSection` 已是 `WorkerInvocation` 的一等字段）、可见性语义现成——**隐藏 ≠ 不可见**，发送后用户仍可展开审计，只是发送前不出现在输入框。

**根因定性**：原始设计缺陷（非「好设计但实现不完整」）。改动四十六/四十七把三块「由 runtime 决定、不随任务变化、需稳定执行」的上下文当成「随任务变化的需求正文」，一起塞进用户可编辑文本；更严重的是**写侧协议指令（issue 完成输出的唯一约束）落在用户可编辑文本里**——用户无从知道删除它会断链。修根因的做法不是「把文本挪走」的表面修补，而是按 AGENTS.md 的 system/user prompt 划分标准重新分层，并让 claim 时刻的 runtime 值成为唯一权威来源。

**实现（五跳，复用既有通道，零新机制）**

| 跳 | 位置 | 动作 |
| --- | --- | --- |
| 1 | `multica/vm.rs` | `from_detail` 回到裸 `requirement_text()` 预填（删除 `language` 参数）；新增 `remote_task_hidden_section(task, language) -> Option<PromptHiddenSection>`（title = `Gold Band remote task context`，与既有 `Gold Band runtime context` / `Gold Band stable system prompt` 同构且不含对内产品名，内容 = DPMS 溯源块 → 上游交付说明块 → 完成输出协议块，三块渲染条件/空白过滤/双语选择全部不变，仅去掉需求正文段）。**（二次修正：DPMS 溯源块回到预填、区段收敛为两块；三次调整：上游交付说明块也进预填、区段收敛为仅协议块，见本节末「二次修正」「三次调整」）** |
| 2 | `multica/commands.rs` `start_remote_conversation_run` | claim 成功后构建区段并写入 `input.first_prompt_hidden_sections`（claim 时刻的值即执行依据，前端不感知该字段） |
| 3 | `view_models_conversation.rs` | `ConversationCreateInputVm` 增 `#[serde(default)] first_prompt_hidden_sections: Option<Vec<PromptHiddenSection>>`；`prepare_conversation_task_vm` 在 conversation.json 之后落盘 `task_dir/authoring/first-prompt-hidden-sections.json`（storage 加 `first_prompt_hidden_sections_file` 路径 helper） |
| 4 | `app/node_executor.rs` | 抽出策略函数 `first_prompt_hidden_sections(app, task_id, session_mode)`（仅 `SessionMode::New` 返回区段——覆盖首次执行 + 自动重试；追问/续跑 `Continue` 不重放），`build_worker_invocation` 据此填 `extra_hidden_sections`；私有 `load_first_prompt_hidden_sections` 负责读盘，fail-open（缺失 → 空；损坏/不可读 → `warn!` + 空，区段是附加上下文而非执行前提） |
| 5 | `provider/mod.rs` `render_prompt_bundle` | RawAgent 信封 + `RequirementTask`（直接对话首条 prompt）→ `append_titled_hidden_sections`（每个区段保留自身标题渲染为独立 `<hidden>` 块，追加在可见正文之后）；RuntimeManaged 信封已有 `render_hidden_context` 消费 `extra_hidden_sections`，零改动 |

**两个执行面必须同时覆盖（评审发现，已修）**：Auto 模式入口即 AI-DYNAMIC 节点，其 invocation 由 `build_dynamic_worker_invocation` 独立构造（`orchestrator.rs`），与本表第 4 跳的 `build_worker_invocation`（workflow worker）是两条构造路径。若只在后者填 `extra_hidden_sections`，Auto 模式全部运行将拿不到三块——`completion-output` 协议不可达、issue done 时无输出可中继（回归）。修法不是复制一份加载逻辑，而是把策略抽为 `node_executor::first_prompt_hidden_sections` 单一入口，`build_dynamic_worker_invocation` 在 `dynamic_hidden_sections(...)` 之后 `extend` 同一函数结果——**一处策略、两个消费面**，SessionMode 语义天然一致。

**分层依据（AGENTS.md 固定标准）**：runtime 决定、用户不需操心、需稳定执行的上下文 → system/hidden 侧（三块全属此类）；与本次执行目标直接相关、随任务变化 → user 侧（只剩需求正文）。

**设计反转记录（两处，均为原始设计缺陷）**

1. 改动四十六判断「溯源块放 user prompt、发送时不从 claim 响应二次注入」→ **反转**：runtime 上下文属隐式侧；claim 时刻注入才是正确权威来源（用户可编辑的只剩他自己的执行目标）。
2. 改动四十七「四段组装进 composer 预填」→ **收敛为**「预填 = 需求正文，上下文 = 隐式区段」。

两处反转的代价为零兼容层：前端渲染面不变（折叠模块是既有能力）、无新增前端字段、旧字段（`session_start_input` / `from_detail` 的 `language`）直接删除（开发阶段破坏式更新）。

**提示词精简**：`remote_task_completion_protocol.md`（zh/en）由多段说明文改为要点式三条——交付说明覆盖内容 / 篇幅 ≤2000 字符与超长后果（完成请求被整体拒绝、工作项无法标记完成；详细材料走工作区文件 + 路径引用）/ 随完成传递给下游子任务。契约关键词（`completion-output` 围栏、最终交付说明、非 run 日志、≤2000 字符、文件路径引用、下游子任务）全部保留，`src/prompts.rs` 契约测试同步锁定中英文版本；双语目录结构保持一致。

**过度设计评审**：零新增抽象、零新增状态机、零新增持久结构——复用 `PromptHiddenSection` 既有通道（改动前已被三处使用），新增的只有一个 input 字段（`#[serde(default)]`）、一个路径 helper、一个 KB 级 JSON 落盘文件、一个策略函数与一个 fail-open 读取函数；没有为「防止用户误删」引入校验、提示、恢复机制（把这些文本移出用户可编辑范围即根除该类问题）。AI-DYNAMIC 面修的是「同一策略被两个构造路径共享」，而非新建第二套加载机制。

**性能评审**：新增一次首条 prompt 的 KB 级小文件读取（仅 `SessionMode::New`，非热路径；两个执行面共用同一函数，读取次数不随执行面翻倍——同一 invocation 只构造一次）；无新增轮询/订阅/缓存/队列/锁；模板渲染次数与路径不变；composer 预填文本量反而减少。无性能风险，不设专项 benchmark。

**独立评审与处理（2026-09-21）**

| # | 级别 | 发现 | 处理 |
| --- | --- | --- | --- |
| 1 | 高 | AI-DYNAMIC 执行面（Auto 模式）未拿到三块 → 完成输出协议不可达 | **已修**：策略抽为 `first_prompt_hidden_sections`，`build_dynamic_worker_invocation` 同步消费；新增 `dynamic_invocation_receives_first_prompt_hidden_sections_on_new_only` 固化 |
| 2 | 低 | 新区段标题未本地化——`hiddenPromptTitle` 落到 raw title（zh-CN UI 出英文） | **已修**：标题改 `Gold Band remote task context`（去对内产品名，与另两个标题同构），前端 `hiddenPromptTitle` 映射 + i18n 增 `acp.hiddenRemoteTaskContext`（zh「隐藏远程任务上下文」/ en「Hidden remote task context」） |
| 3 | 低 | 「标题与正文皆空的 issue 型远程任务」→ 区段为 None、预填文本为空 → 发送被拒、claim 回滚 | **接受**：该输入本身即不可执行（无任何需求正文），拒绝并回滚 claim 是正确行为而非缺陷；由既有前置校验门控，不新增机制 |
| 4 | 低 | 精简 en 协议提示词时丢失「工作项无法标记完成」后果句 | **已修**：补回 `and the work item cannot be marked done`，`src/prompts.rs` 契约测试同步锁定中英两侧 |

**验证（2026-09-21，改动落地后）**

| 层 | 命令 / 目标 | 结果 |
| --- | --- | --- |
| 提示词契约 | `cargo test -p gold-band --lib remote_task` | 6 passed（含 `remote_task_completion_protocol_templates_lock_fence_contract` 精简后中英契约：`最终交付说明` / `run 日志` / `2000 字符` / `文件路径引用` / `下游子任务` / `工作项无法标记完成` ↔ `final deliverable handoff` / `run log` / `2,000 characters` / `file path references` / `downstream sub-tasks` / `cannot be marked done`） |
| 首条 prompt 渲染 | `cargo test -p gold-band --test provider_prompt_bundle` | 33 passed（新增 `render_raw_agent_first_prompt_appends_titled_hidden_sections`：独立标题 `<hidden>` 块在正文之后；`render_raw_agent_continue_prompt_skips_hidden_sections`） |
| 区段加载（两执行面） | `cargo test -p gold-band --lib first_prompt_hidden` | 2 passed（`first_prompt_hidden_sections_load_on_new_only`：workflow 面 New 命中 / Continue 空 + 策略层直接断言；`dynamic_invocation_receives_first_prompt_hidden_sections_on_new_only`：AI-DYNAMIC 面 New 命中 / Continue 空——两例共享同一策略函数，共 1 处策略定义；损坏文件 fail-open 空） |
| 区段组装 | `cargo test -p gold-band-desktop --bins multica::` | 148 passed（vm.rs 5 例：`remote_task_hidden_section_assembles_blocks_in_order`（块序 DPMS→上游→协议）/ `_renders_dpms_block_with_protocol` / `_without_dpms_fields_omits_block` / `_without_parent_output_omits_upstream_block`（含空白过滤）/ `_issue_linked_without_body_yields_protocol_only`（非 issue → None）） |
| 落盘 | `cargo test -p gold-band-desktop --bins hidden_section` | 6 passed（含 `conversation_task_creation_persists_first_prompt_hidden_sections`：携带区段 → 文件存在且可反序列化；本地会话 → 不落盘） |
| 前端标题本地化 | `npx vitest run --config web/vitest.config.ts hidden-prompt-content-expansion` | 4 passed（新增 `localizes the remote task context section title in the zh-CN UI`：标题显示为 `acp.hiddenRemoteTaskContext`，不再落到 raw title） |
| 回归 | `cargo test -p gold-band-desktop --bins view_models_conversation::` / `--bins scheduled` | 94 passed / 127 passed（`ConversationCreateInputVm` 全部构造点已同步新字段，零回归） |

预填文本变短、隐藏区段渲染是既有能力（本轮前端只增标题映射），故无需启动前端做交互验证；端到端需打包内网验证（本地 multica 无最新 server）。

**二次修正（2026-09-21，用户实测反馈）：DPMS 溯源回到 composer 预填**

用户运行后发现「之前可以在输入框显示 DPMS 的信息，现在不行了」，要求按原设计恢复为**在需求正文之前展示**。

- **根因定性**：不是实现错误，是**分层判断过度收敛**——M5-bk 把三块一律判为「runtime 决定、用户不需操心」，但 DPMS 溯源（发布计划 / 开发·测试负责人 / 需求链接）是**用户在发送前需要核对的目标环境信息**，具备 user 侧价值；只有「上游交付说明」（纯执行上下文，用户无需核对）与「完成输出协议」（写侧契约，误删即静默断链）才必须留在隐式侧。修法是按实际消费主体拆开两类上下文，而不是整体回退（整体回退会把协议块重新塞回用户可编辑文本，那才是 M5-bk 修掉的原始缺陷）。
- **实现**：`vm.rs` 恢复 `from_detail(task, workspace_id, language)`，预填 = `detail_prefill(task, language)` = DPMS 溯源块（可选）+ 空行 + 需求正文（含「正文缺失但溯源存在 → 只发块」，与 M5-bh 语义一致）；`remote_task_hidden_section` 收敛为**上游交付说明块 → 完成输出协议块**两块。**同一份上下文只下发一次**：溯源块已进预填，故从区段中移除，避免模型收到两份。
- **连带效果**：低发现 #3（「标题与正文皆空的 issue 型远程任务」预填为空 → 发送被拒 + claim 回滚）在该形态下自然消失——有 DPMS 字段的任务预填不再为空。
- **措辞更正（评审追踪）**：隐藏区段在会话气泡中的呈现是**「图标 + 标题 + 字符数」的链接按钮 → 打开右侧工作区只读面板**，不是内联折叠模块（`web/src/components/acp/HiddenPromptMessageContent.tsx`；前端测试断言 `[data-slot="collapsible"]` 为 null）；链接可点需 `branchLocator && scopeKey && !optimistic`，否则按钮 disabled。另：multica 首条 prompt 含隐式区段 → 不适用 main 的需求原文 `display_text` 回落，气泡正文以完整 `user_prompt` 投影、区段可审计（详见本节末「合并接缝修正」）；追问路径（Continue + `UserMessage`）气泡只显示键入文本。
- **测试（同一轮验证）**：`cargo test -p gold-band-desktop --bins multica::` → 149 passed，其中 `from_detail_prepends_dpms_context_block_to_requirement`（溯源块在前、正文在后、双语同构、**预填不含 `completion-output`**）/ `from_detail_without_dpms_fields_keeps_plain_requirement`（全缺省退化纯正文 + 逐字段空白过滤 + 正文缺失只发块）/ `remote_task_hidden_section_excludes_prefilled_dpms_block`（**防重复下发**：区段无 DPMS 字段值、无正文）/ `remote_task_hidden_section_assembles_blocks_in_order`（上游→协议锚点递增）。管道层 fixture 文本同步改为 `upstream handoff + completion protocol`（原名含 dpms 已不符实际）；`first_prompt_hidden` 2 / `provider_prompt_bundle` 33 / `view_models_conversation::` 94 全部保持绿。

**三次调整（2026-09-21，用户指定）：上游交付说明（父任务 completion-output）进 composer 预填**

用户确认二次修正生效后提出：完成输出协议保持隐式（现行为准），但子任务从父任务拿到的输出内容（`parent_output`，即 completion-output 中继内容）应**显式放在子任务输入框里**。

- **分层依据（沿用二次修正的判据，非新标准）**：按实际消费主体划侧——上游交付说明是**子任务赖以执行的父任务交付内容**（部署/验证地址、变更范围、测试要点），用户需在发送前核对「父任务到底交付了什么」，归 user 侧可见预填；完成输出协议仍是写侧契约（落在可编辑文本里误删即静默断链），留在隐式区段。二次修正把上游块判为「纯执行上下文、用户无需核对」在这一点上欠收敛，本次按同一判据补齐，不是标准变更。
- **实现**：`detail_prefill` = DPMS 溯源块（可选）+ 上游交付说明块（可选）+ 需求正文，块间空行分隔（`parent_output` 缺省/纯空白跳过该块，版本解耦；正文缺失但任一块存在 → 只发块）；`remote_task_hidden_section` 收敛为**仅完成输出协议块**（非 issue 关联 → None）。**同一份上下文只下发一次**：上游块进预填后从区段移除。模板 `remote_task_parent_output.md` 本身零改动（预填即首条 user prompt，模板指令语气本就面向 agent）。`client.rs` `parent_output` 字段注释的消费点同步改指 `detail_prefill`。
- **连带效果**：低发现 #3 的残余场景进一步消失——只要父任务有交付内容，即使任务无正文/无 DPMS 字段，预填也不为空。
- **测试（同一轮验证）**：`cargo test -p gold-band-desktop --bins multica::` → 149 passed。重写/新增：`from_detail_prefills_upstream_handoff_between_dpms_and_body`（预填三段锚点严格递增 DPMS→上游→正文、正文收尾、父输出文本不丢、区段无上游块且含协议、双语同构）/ `from_detail_without_parent_output_omits_upstream_block`（缺省/纯空白跳过上游块、协议块不受影响）/ `remote_task_hidden_section_issue_linked_without_body_yields_protocol_only`（扩展：正文缺失但父输出存在 → 预填只剩上游块、区段仍协议块）。管道层 fixture 文本同步改为 `completion protocol`；`first_prompt_hidden` 2 / `provider_prompt_bundle` 33 / `view_models_conversation::` 94 全部保持绿；前端零改动（区段标题与 i18n 映射不变）。

**合并接缝修正（2026-09-22，合并 main `361eeb3c` 后功能 review 发现）**

- **回归现象**：合并 main 后，远程任务首条 prompt 的聊天气泡不再出现隐藏区段审计入口（「图标 + 标题 + 字符数」链接按钮），二次修正措辞更正中「气泡正文回退为完整 `user_prompt`，故区段确实可审计」的前提被破坏。
- **根因定性（合并接缝，非原设计缺陷）**：main `53491fc1`（session input history navigation）在 `render_prompt_bundle` 新增 `display_text` 回落——条件 RawAgent + New + RequirementTask 恰好精确命中远程任务首条 prompt 三元组，使 `display_text = Some(纯需求正文)`，经 `client.rs` 的 `display_text` 优先级压过 `session_prompt_text`（完整 `user_prompt`），`<hidden>` 块从气泡文本中被剥离、前端解析不到。两个设计意图各自正确（main：本地直接对话显示用户原话；M5-bk：后端注入上下文必须可审计），冲突在 main 的回落条件未考虑「user_prompt 已被 runtime 追加隐式区段」的场景。
- **修复（收紧守卫而非补投影）**：回落条件追加 `extra_hidden_sections.is_empty()`——仅当 prompt 未被 runtime 追加任何隐式区段时才回退需求原文。本地直接对话保持 main 原话显示意图；含区段的首条 prompt 恢复完整投影、审计入口回归。不选「给远程任务补 `prompt_display`」：那会复制投影逻辑、且 display_text 与 user_prompt 形成双真源易漂移。
- **最小失败测试先行**：新增 `render_raw_agent_first_prompt_with_hidden_sections_suppresses_verbatim_display`（先失败：`display_text` 为 Some）+ `render_raw_agent_first_prompt_without_sections_falls_back_to_requirement`（保护 main 回落意图不被误删），修复后同测试转绿。`provider_prompt_bundle` → 35 passed；`multica:: / view_models_conversation:: / scheduled_service:: / scheduled_runtime::` → 369 passed 零回归。main 侧无测试锁定「无 prompt_display + RawAgent」场景的回退，修改无破坏面。

**main 合并适配（2026-09-23 第十次合并，PR #131 workspace file references）**

- main 新增 composer 工作区文件引用（draft `workspaceFiles` 状态）与本分支远程绑定并集后，`prefill`（远程任务覆盖式新草稿）同时清空 `workspaceFiles`——远程任务在远程工作空间执行，本地工作区文件引用不适用（draft 测试 `prefill writes requirement text + remote binding and drops prior attachments and workspace files` 锁定）。
- main 新测试 `render_prompt_bundle_does_not_add_builtin_output_contracts` 曾锁定「RawAgent 首条 prompt 区段不进 user_prompt + display 回退原文」，与 M5-bk 契约在同一输入形态互斥；经取证（main 无 M5-bk 追加块、生产路径直接对话不设区段）并经用户确认，适配该测试到 M5-bk 语义（区段以标题 `<hidden>` 块受控追加 + `display_text = None`，其余 MUST NOT LEAK 断言保留）。详见 `merge-conflict-analysis-2026-09-22.md` 第十次合并章节。

**main 合并适配（2026-09-24 第十一次合并，七语言 i18n JSON 化 + prompts LocalizedText 化）**

- main 将 `web/src/i18n.ts` 内联翻译资源拆分为 `web/src/locales/*.json`（七语言，`fallbackLng: en`）。本分支 81 个 remote 系列 i18n key（taskManagement/connection/connect/sidebar.remoteTask\*/composer.remote\*/skills.remoteSync.\*/hiddenRemoteTaskContext/errors.remote.source-not-configured）随迁 `zh-CN.json` + `en.json`；其余五语言经 main 既有 fallback 覆盖（main 自身 25 key 缺口同走此路径）。七个 JSON 中 main 早期带入的旧 `multica.*` key 树（顶层 multica / errors.multica / sidebar.multica(TaskManagement) / composer.multica\*）按既定 Remote\* rename 决策全部删除，破坏式不留 fallback。
- main 将 `src/prompts.rs` 重构为 `LocalizedText` 结构化对象 + `localized_prompt!`/`localized_prompt_zh_en!` 宏（七语言）。remote 系列 3 个运行时提示词（remote_task_context / remote_task_parent_output / remote_task_completion_protocol）以 `localized_prompt_zh_en!` 适配（仅中英，其他语言 resolve 降级英文，与 main 的 CICD 同模式）；`vm.rs` 三处 `prompt_by_language` 调用随 main 新签名收敛为传 `LocalizedText` 常量。AGENTS.md 七语言例外条款同步补入 `runtime/remote_task_*.md`。
- 验证：provider_prompt_bundle 35 过、lib provider/prompts/dsl/config 196 过、desktop bins 371 过；tsc web/src 零错误；vitest 稳定失败集 138 例与 origin/main 逐文件一致（新增 1 例 conversation-greeting 系 main 拆 JSON 未更新 source-contract 测试），零合并回归。详见 `merge-conflict-analysis-2026-09-22.md` 第十一次合并章节。

### 12.51 改动四十九：终态后追问 run 的迟到 completion-output 补发（M5-bl，2026-09-21）

**背景（用户内网实测）**：父任务 direct 模式首 run 完成时未产出 `completion-output` 块，用户输入「完成」让 agent 在**同一本地会话**补交了块，但子任务 claim 时 `parent_output` 仍为空。

**根因追溯**：首 run Success 时 `handle_run_completed` 提不出块 → fail-open（issue 照标 done、无输出，bridge 记 warn）→ `finalize_terminal` 清 active_runs + task_conversations（终态收尾，语义正确）；追问 run 的 `RunCompleted` 到达时 `lookup_active_run` 落空 → 订阅器直接 return——**补交的块没有任何上报路径**，服务端 `completion_output` 恒空。定性：**好设计、实现不完整**——服务端设计本就支持迟到写入（`开发设计方案-issue-completion-output.md`：「不锁 done 才能写」「done 后补写自愈」），多 run 覆盖语义也已定义（每次 run 标 done 整体覆盖前值）；缺的只是客户端「本地追问 run」这条路径的中继接线。不属设计缺陷，无需动服务端或状态模型。

**实现（bridge.rs + client.rs，复用既有提取与历史，零新状态）**

| 位置 | 动作 |
| --- | --- |
| `bridge.rs` 订阅器 RunCompleted 分支 | `lookup_active_run` 落空且 `outcome == Success` → spawn `relay_late_completion_output`（state 反查是磁盘读，不进热路径；Failure/Killed 无补发语义不进 spawn） |
| `bridge.rs` `relay_late_completion_output` | 顺序即成本递增：multica 连接检查（纯内存）→ `completion_output_for_issue_done` 提块（普通本地 run 到此返回，零 state/网络开销）→ `load_state` → `find_completed_issue_task` 反查 → output-only PATCH。失败仅 warn；本地状态零改动 |
| `bridge.rs` `find_completed_issue_task`（纯函数） | 按 local_task_id 反查「最近完成」历史，命中条件 `status == completed && issue_ref 非空`，多条取最新；返回 `(workspace_id, issue_id, remote_task_id)` |
| `client.rs` `update_issue_completion_output` | 同一 `PUT /api/issues/{id}` 端点、**只写 `completion_output` 不带 status 键**（`UpdateIssueCompletionOutputRequest` 结构体上不存在 status 字段即契约本身）——issue 可能已被服务端 reopen/人工流转，迟到写入不得强行拉回 done；服务端字段级 merge「未触摸字段保留现值」 |

**关键决策**：无块不补发（不发空写；与首 run fail-open 一致，且普通本地 run 常态走此分支必须静默——逐 run warn 会成噪音）；不做去重/重试（迟到的自愈写入，下次补交整体覆盖，服务端单值语义兜底）。

**最小失败测试先行**：先写 4 个测试确认失败原因与根因分析一致（E0422/E0425——补发链路请求结构体与反查函数不存在），再实现转绿。测试固化：`find_completed_issue_task_returns_relay_target_for_completed_issue_entry` / `_skips_failed_blank_issue_and_foreign_local_tasks`（failed、空/空白 issue、异 local_task_id 三类不命中——后者保护普通本地 run 不误发）/ `_prefers_newest_match`；client 侧 `update_issue_completion_output_request_omits_status_key`（body 恰为单键，wire 契约）。`cargo test -p gold-band-desktop --bins multica::` → 153 passed。端到端需打包内网验证（本地无最新 server，`create_multica_subscriber` 环境依赖无法本地自动复现，以纯函数固化 + 代码路径审查为替代证据）。

**过度设计评审**：零新增持久字段、零状态机、零队列——反查目标是既有「最近完成」历史（本来就在），PATCH 端点是既有 issue 更新端点的字段子集；唯一新抽象是一个纯函数 + 一个薄 HTTP 方法，与「迟到写入」这一实际问题规模相称。

**性能评审**：热路径（订阅器回调）仍只有内存锁查找 + 条件 spawn；spawn 内按「内存检查 → 单文件读 → 小 JSON state 读 → 网络调用」成本递增短路，普通本地 run（绝大多数）止步于提块阶段的一次 attempt timeline 读；无轮询、无缓存、无锁竞争。RunCompleted 为每 run 一次的低频事件，无性能风险。

### 12.52 改动五十：需求管理页「移出列表」——纯本地视图过滤、刷新即恢复（M5-bm，2026-09-21）

**背景（用户需求）**：需求管理页可删除任务，但只在码灵侧删除；点刷新后仍从 multica 侧拉回（含待办与已完成）。

**语义定型（用户选定「刷新即恢复」）**：删除 = 本页内存态视图过滤（`removedTaskIds: Set<task.id>`），不删本地历史（completed 快照保留、终态行深链不受影响）、不调任何服务端接口；任何一次 `fetchTasks` 成功（手动刷新 / 事件刷新 / 取消后回拉）即清空集合，任务随新数据回到看板。服务端始终是任务列表唯一事实源，码灵侧零持久化删除痕迹（对齐 data-loading / 状态规则：不为本页建旁路状态）。

**实现**

| 位置 | 动作 |
| --- | --- |
| `RemoteTaskManagementPage.tsx` | `removedTaskIds` state（瞬时 UI 态不持久化，ui-interaction §7）；`fetchTasks().then` 内清空（= 刷新语义）；投影 = 类型过滤 ∩ 未移出（两级纯客户端过滤叠加，不动数据源）；`handleRemoveTask` 纯本地 set 操作 |
| `RemoteTaskBoard.tsx` | 卡片动作区增 Trash2「移出列表」按钮，**四列全状态可用**（含待办/已完成）；`disabled={readOnly \|\| busy}`；hover destructive 色与 cancel 同语义 |
| `web/src/i18n.ts` | `conversation.sidebar.remoteTasks.removeTask` / `removeTaskHint`（zh「移出列表」/「仅从码灵侧列表移除，刷新后恢复」；en 对应） |

**不弹确认框的依据**：操作可逆且零数据丢失（刷新即恢复），ui-interaction §1 的「删除确认用 Dialog」针对破坏性删除；按钮 Tooltip 说明语义即足。

**测试（接口层固化）**：board 侧「四状态各有移出入口 + 转发 onRemove」「busy 时禁用」；容器侧「移出后卡片消失、无任何 API 调用（getRemoteTasks 仍 1 次、cancel 未触发）」「手动刷新后任务恢复（getRemoteTasks → 2 次、卡片回来）」。`npx --prefix web vitest run --config web/vitest.config.ts tests/remote-task-board.test.tsx tests/remote-task-management-page.test.tsx` → 42 passed；`npm run web:build`（tsc -p web/tsconfig.build.json + vite）通过。

**过度设计评审**：一个 Set + 一个过滤谓词 + 一个按钮，无新组件/新抽象/新接口；未引入「本地删除清单」持久化（那会制造第二事实源，与刷新即恢复语义矛盾）。

**性能评审**：过滤是 O(n) 纯客户端投影（与既有类型过滤同层同量级，n = 单工作空间任务数）；删除操作零 IO；不新增订阅/请求。

### 12.53 改动五十一：迟到补发触发事件修正——挂到 AcpTurnFinished（M5-bn，2026-09-21）

**背景（用户内网实测）**：12.51（M5-bl）上线后，「非首次会话完成的任务 output 未送达 multica」依然存在。

**根因追溯**：M5-bl 假定「追问会产生一次新的 `RunCompleted`」——该假定对 Direct 模式后续追问**不成立**。证据链：① 前端 `submitPrompt` 对已完成任务的追问带**同一** (task/run/round/node/attempt) 五元组调 `submit_conversation_prompt`；② Direct 分支把 prompt 入队到**同一 attempt** 的 ACP prompt queue，run 已 Completed 时经 `notify_prompt_turn_finished` 触发排空，排空把 prompt 发进**同一 session/attempt**——不新建 run；③ 该 turn 的终态信号是 `RuntimeLifecycleEvent::AcpTurnFinished`（`src/app/mod.rs` 文档注释明示「Direct 后续对话……统一走该事件」），而 multica 订阅器此前**显式忽略**该事件；④ `RunCompleted` 全局唯一发射点在 orchestrator 的 run 级收尾，追问 turn 不经过。即 M5-bl 的 fallback 在等一个**永远不会来的事件**。定性：**好设计（迟到补发 + output-only PATCH）、实现不完整（触发事件挂错）**。次要问题：relay 四个静默 return 无日志，触发缺失在排查中完全不可见。

**实现（bridge.rs + app/mod.rs + commands.rs）**

| 位置 | 动作 |
| --- | --- |
| `src/app/mod.rs` `AcpTurnFinished` | 新增 `attempt_dir: Option<String>` 字段——对齐 `RunCompleted::attempt_dir` 先例（同为 multica completion-output 提取而设，免于跨 repo root 反推路径）。全项目仅 1 个真实发射点（`emit_acp_turn_finished`），消费方 match 均 `..`，非破坏 |
| `commands.rs` `emit_acp_turn_finished` | 用 `locator.attempt_dir(app)` 填充（动态节点场景由该 helper 归一到 outer attempt 路径） |
| `bridge.rs` 订阅器 | 新增 `AcpTurnFinished` 分支：`outcome == Completed` → spawn `relay_late_completion_output(task_id, attempt_dir)`（同一补写通道，PUT 幂等覆盖）。不查 active_run——relay 按 completed 历史反查，在飞任务天然不在历史里（run 级收尾仍由 RunCompleted 主路径负责）；Failed/Cancelled 无交付语义不上报 |
| `bridge.rs` `relay_late_completion_output` | 过滤顺序重排为「内存检查（连接态）→ state.json（小文件，历史反查）→ attempt timeline（大文件，提块）」——turn 级触发逐消息发生，先用小文件过滤非 multica 任务；补观测日志：命中任务但无块 → info（诊断分界点），state 读失败 → info，成功送达 → info（含字符数） |

**最小失败测试先行**：先在既有 `acp_turn_finished_event_preserves_turn_identity_outcome_and_batch_continuation` 补 `attempt_dir` 断言确认红（E0026——字段不存在），再实现转绿。测试固化：事件携带 `app.paths.attempt_dir(五元组)` 路径（追问 turn 的 timeline 就写在该 attempt 下）。**替代证据说明**：「事件 → spawn relay」路由依赖 Tauri AppHandle + 异步 spawn + 真实 HTTP，单测无法稳定复现（M5-bl 的 RunCompleted 路由同样未覆盖——正是漏洞溜过去的原因）；以编译器穷尽性（match 新增分支）+ 发射点测试 + 新增运行时 info 日志为可审计轨迹，端到端打包内网验证。`cargo test -p gold-band-desktop --bins multica::` → 154 passed；`acp_turn*` / `notification*` 套件全绿。

**过度设计评审**：一个事件字段（有 RunCompleted 同构先例）+ 一个 match 分支 + 日志，零新状态/抽象/队列；failed 任务补发、batch 中断补偿等场景明确不进 scope（维持 M5-bl 语义边界）。

**性能评审**：订阅器热路径仅 outcome 比较 + spawn（与既有 RunCompleted 分支同模式）；磁盘读全在 spawn 内，且重排后**普通本地会话**（绝大多数）止步于 state.json 小文件读、不再每条追问消息读大 timeline；频率 = 每条追问消息一次；未连接 multica 时纯内存检查直接返回。

### 12.54 改动五十二：终态行「移出列表」升级为真删除本地历史（M5-bo，2026-09-21）

**背景（用户内网实测）**：码灵侧移出 + multica 侧删除后，点击刷新任务仍在需求管理页「复活」。

**根因追溯**：12.52（M5-bm）把「服务端是任务列表唯一事实源」当作全部行的前提，但该前提只对 pending/running 行成立——`get_remote_tasks` 三源合并中，**终态行（completed/failed）的唯一数据源是本地 `remote_completed_tasks` 历史（state.json）**，服务端根本不回传终态。视图过滤 + 「任何成功 fetch 清空移出集合」的语义组合下，终态行被移出后任何一次刷新必然「复活」（不是从 multica 拉回，是从本地历史读出）；multica 侧删除对此完全无效。定性：**好设计（服务端真源）但实现不完整（对终态行前提不成立）**。

**实现（按行数据源分派，bridge.rs + multica/commands.rs + 前端）**

| 位置 | 动作 |
| --- | --- |
| `bridge.rs` `remove_completed_task_entry` | `record_completed_task` 的逆操作：按 remote_task_id 删「最近完成」历史条目；返回是否删到（false = 幂等 no-op，不落盘） |
| `multica/commands.rs` `remove_remote_completed_task`（新 Tauri 命令） | `with_state` 原子 RMW 删除条目；不存在时幂等成功（用户意图「不在列表」已满足）；删到才 `emit_remote_tasks_updated` 广播刷新 |
| `RemoteTaskManagementPage.tsx` `handleRemoveTask` | 终态行（completed/failed）→ 调 `removeRemoteCompletedTask` 真删 + 即时移除（busy/error 处理对齐 cancel 模式）；pending/running 行 → 维持纯视图过滤（每次从服务端/内存重建，服务端删除自然消失）。`removedTaskIds` 集合语义收窄为「删除到下一次 fetch 之间的即时移除层」 |
| `RemoteTaskBoard.tsx` | `terminal` 判定 + hint 按行分派：`removeTaskTerminalHint`（zh「移出并删除本地完成记录，刷新不再显示」/ en 对应）vs 既有 `removeTaskHint`。删除的是列表回看索引，**不删本地会话本体**；仍不弹确认框（对齐 ui-interaction §1，hint 如实说明语义） |

**最小失败测试先行**：Rust 侧先写 `remove_completed_task_entry_deletes_by_remote_id_and_is_noop_when_absent` 确认红（E0425——函数不存在），再实现转绿（存在→删且其余保序、不存在→no-op 幂等）。前端改写既有移出测试为新语义：「终态行移出 → 调 `removeRemoteCompletedTask('rt-done')` + 刷新后不复活」「pending 行移出 → 不调删除 API + 刷新后随服务端数据恢复（服务端真源）」。`cargo test -p gold-band-desktop --bins multica::` → 154 passed；页面 21 passed + 看板 21 passed；改动文件 tsc 无错。

**过度设计评审**：一个纯函数 + 一个薄命令 + 前端回调分派，无新组件/状态机；不为 pending 行引入本地删除清单（服务端真源语义保持不变）。

**性能评审**：`remove_remote_completed_task` 为用户手动触发的一次 state.json RMW（上限 50 条的小文件），无热路径；前端零新增渲染开销（仅既有过滤层复用）；删除后的事件广播复用既有 `remote-tasks-updated` 通道。

### 12.55 改动五十三：direct 模式远程任务完成门控——completion-output 块驱动（M5-bp，2026-09-28）

**背景（用户设计评审）**：需求管理中，direct 模式远程任务在**首轮对话轮跑完后即被标记完成**——`RunCompleted{Success}` → `complete_task` + issue done。但真实任务往往需要多轮（甚至十几轮）对话才能完成：首轮跑完后用户继续追问时，远端早已终态，后续轮次只能走 M5-bl/M5-bn 的「迟到补发」被动通道。工作流模式无此问题（graph 走到终态即 definition of done）。**该问题只针对 multica 远程任务，本地普通会话完全不受影响。**

**根因追溯**：§12.3 的「完成语义 = run 终止事件驱动」隐含前提是 **run 终态 ≡ 任务业务完成**。该前提对 workflow 模式成立（graph 终态由设计者显式定义），对 direct 模式**不成立**——direct run 是 runtime-continue 性质，首轮 requirement 驱动 run 跑完（`RunCompleted`）只是**一次对话轮的技术性成功**，与任务是否完成无关；且 direct 后续追问在同一 attempt 内收尾、不产生新的 `RunCompleted`（M5-bn 已确证）。即 direct 模式**没有结构性的 definition of done**，把「跑完一轮」当「任务完成」是概念错配。定性：**原有设计缺陷（非实现不完整）**——按工程规则应修设计而非补丁：延迟完成、超时窗口、追问计数等 patch 手段都无法回答「何时算完成」这个本体问题，必须由**执行体显式声明完成**。

**先例检索（方案 A：协议声明完成，全部复用既有资产）**：
- M5-bj/bk：`completion-output` 协议已存在——issue 关联任务的首条 prompt 以隐式隐藏区段注入协议块（`vm.rs` `completion_protocol_block`，仅 issue 关联任务注入）；`handoff.rs` 已有纯函数提取（嵌套安全、末块优先、未闭合围栏容错）+ timeline 投影（`final_assistant_reply`）。
- M5-bl/bn：迟到补发通道已建立——`AcpTurnFinished` 携带 `attempt_dir`，订阅器已有按事件提块的分支与过滤顺序（内存 → state.json 小文件 → 大 timeline）。
- server 侧：running 任务与心跳 AND 门控（2.5h backstop 只杀无心跳者），长对话期间远端保持 running **无超时风险**——门控扣留完成不会触发 sweeper 误杀。

**设计（先定数据，再定接口，再补实现）**：

数据——`ActiveRemoteRun`（state.rs）新增 `run_mode` 快照：`start_remote_conversation_run` 从 input 的 run 模式采集（Resume / Fresh 两个登记点）。**必要性**：`AcpTurnFinished` / `RunCompleted` 事件都不携带模式信息，无此快照则门控判定无事实源。**仅内存、不持久化**（实现修正，见下方修正记录）：重启后 `active_runs` 丢失由既有链路兜底——启动 `recover-orphans` 无条件 fail 上个进程的 dispatched/running 任务 → server auto-retry 克隆 T' → 用户重领 → resume 路径重新登记 `active_runs`（连同新鲜 `run_mode` 快照）。`task_conversations` / `MulticaTaskConversation` 不加字段，无启动重建、无持久化回退。

接口（两个决策点，bridge.rs）——

| 决策点 | 触发 | 判定 | 动作 |
| --- | --- | --- | --- |
| **决策点 1（run 收尾）** | `RunCompleted{Success}` | active_run 命中 + `run_mode == Direct` + `issue_id` 非空 | **有块**：`complete_task`（output 传提取的交付摘要）+ `update_issue_status(done)` + `finalize_terminal`（现行为）；**无块**：**扣留完成**——不上报终态、保留 `active_runs` + `task_conversations`，远端保持 running |
| **决策点 2（turn 收尾）** | `AcpTurnFinished{Completed}` 且**非 batch 中间态**（`batch_progress.continues` 为 false——Direct 排队的多条追问逐条触发 turn 事件，中间 turn 不是最终回复，块可能是中间产物） | 事件自带 `run_id`，既有 `find_active_run_by_local(task_id, run_id)` 归属反查（**无需新索引**）+ 快照 `run_mode` + `issue_id` 判 Direct + issue | **有块**：走决策点 1 同一完成路径（complete + issue done + finalize + 刷新通知）；**无块**：no-op（对话继续，扣留状态不变）；**归属未命中**（任务已终态/普通本地会话）→ 既有 `relay_late_completion_output` 补发通道，不动 |
| 既有路径不变 | 同上 | 非 Direct（workflow）或非 issue | 维持 §12.3 现路径 fail-open（workflow graph 终态即完成；非 issue 任务不注入协议块，无门控依据） |

提示词——`src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md` 升级为门控语义：「远程任务**只有**在最终回复末尾输出 `completion-output` 块时才会被标记完成；未完成全部工作时**不得**输出此块；需要继续对话推进任务时正常回复即可」。仅 zh-CN + en 两目录（remote_task_* 既定范围）。

**范围与边界（用户确认）**：
- 码灵客户端上来自 multica 的远程任务**目前全部来自 issue**（用户确认）；server 侧 chat/autopilot/quick-create 等非 issue 来源仅作防御性判定保留（非 issue → 现路径），后续有实例再调整。
- **App 关闭影响（接受，非新机制）**：门控把远端保持 running 的窗口从「一轮」拉长到「整个对话生命周期」，App 关闭沿既有 sweeper 路径（心跳停 → 150s offline → fail → auto-retry 克隆 T' → <2h 重领续跑 / >2h 链路终结、issue 回 todo）。「断线后自动重新认领并续跑」另立需求，不进本期。
- **逃生通道**（无新 UI）：① 会话补交——用户在对话中说「完成/可以了」，agent 输出块 → 决策点 2 完成（M5-bl 内网实测已验证该行为模式可靠）；② 看板取消——既有取消检测作废本地 run。
- `relay_late_completion_output`（M5-bl/bn 迟到补发）保留不动：它服务于「已完成任务的 output 补写」，与门控语义正交。
- 本地普通会话零影响：门控全部位于 bridge 层，只对 multica 归属 + Direct + issue 生效。

**风险与对策**：
- **提示词遵从性**：agent 若永不输出块，任务永不 complete——对策：协议块措辞强化 + 会话补交/看板取消双逃生 + server sweeper 兜底（无心跳场景）；这是 agent-driven 模型的固有代价，与 §12.3 已接受的 Paused 盲区同类。
- **同 turn 双事件并发完成（实现期发现）**：门控命中的任务在首轮带块完成时，`RunCompleted` 与 `AcpTurnFinished` 几乎同时到达，两个 handler 的归属查找都先于任一 finalize——初版「先到者 finalize 后到者落空」的互斥论证**不成立**，会并发跑两次完成路径（重复 complete HTTP + 冗余日志/通知；本地操作已核实幂等，无数据损坏）。对策：`MulticaRuntimeState` 新增**完成路径认领集**（`begin_completion` / `end_completion`，`HashSet` 按 remote_task_id 粒度）——先认领者执行，后到者整体退出；认领随 finalize 终结（成功/失败皆然，不泄漏）；认领置于 context/client 取得之后（断连早退不认领）。守卫本身 fail-open（状态不可达时放行，退回旧行为）。
- **决策点 2 归属误判**：`AcpTurnFinished` 无 remote 上下文——对策：事件自带 `run_id`，`active_runs` 按 (task_id, run_id) 双键精确反查（串台防护既有测试固化），未命中走既有补发通道（补发自身有 completed 历史反查 + 无 issue 关联静默退出两层过滤）；本地普通会话在内存检查即排除。
- **batch 中间态块的迟到补发窗口**：`continues=true` 的中间 turn 整体跳过（含补发）——若**已终态任务**的排队追问中块只出现在非最终轮，该块不补写（补发按最终 assistant 回复提取，本就不逐轮）。窗口极窄且 issue 已 done、PATCH 尽力而为，接受。
- **run_mode 快照漂移（重领换模式）**：resume 实际继续既有 run，但登记时沿用本次请求的模式选定（用户重领时若改选 workflow，快照随之变化）——接受：重领即用户重新决策执行方式，以最新意愿为准，不为回溯旧模式加持久化。

**最小失败测试先行（验收计划）**：先写 bridge 决策纯函数的最小失败测试确认红（`E0609` no field `run_mode` + `E0425` 两个决策函数未定义），再实现转绿——① Direct+issue+`RunCompleted{Success}`+无块 → 扣留（不 complete、保留 active_runs/task_conversations）；② 同场景+有块 → complete（output=块内容）+ issue done + finalize；③ 非 Direct（workflow/auto）→ 现路径不门控；④ 非 issue → 现路径 fail-open；⑤ 门控判定双维度（`is_gated_direct_issue_run`：direct+issue 命中，workflow/auto/非 issue/空白 issue 均不命中）。事件路由依赖 Tauri AppHandle + 异步 spawn（M5-bl/bn 先例），以决策纯函数覆盖 + 编译器穷尽性为可审计轨迹，端到端打包内网验证。

**过度设计评审**：无新 aggregate/状态机/持久化身份——`run_mode` 是既有输入的内存快照（补齐门控判定的事实源缺口，非新概念）；归属反查复用既有 `find_active_run_by_local`（事件自带 run_id，无需新索引/持久化回退）；完成动作收敛为公共 `complete_with_issue_done`（门控命中 / fail-open / turn 收尾三处同语义单实现）；不引入「完成中/待确认」等中间态（扣留 = 远端保持 running，天然已有）。非 issue 判定为防御性最小实现，不建兼容层。

**性能评审**：决策点 1 在 `RunCompleted` 一次性发生（读一次 timeline 提块，M5-bj 同量级）；决策点 2 每 direct turn 一次，订阅器先做纯内存过滤（非 Completed / batch 中间态 / `active_runs` 未命中均不触发块提取），门控命中才读 timeline 提块，本地普通会话纯内存检查即返回；无新增轮询/订阅/缓存/持久化字段。

**修正记录（实现期，2026-09-28）**：
1. 初版设计为决策点 2 规划了「`task_conversations` 持久化回退 + `MulticaTaskConversation` 加 run_mode 字段 + 启动重建 active_runs」。实现时复核推翻：① `AcpTurnFinished` 事件自带 `run_id`，既有 (task_id, run_id) 双键归属即可精确命中，持久化回退无对应场景；② 启动 `recover-orphans` 无条件 fail 上个进程的在飞任务，重启场景本就由「auto-retry 克隆 → 重领 → resume 重登记」链路整体接管，重建 active_runs 反而与该链路冲突（重建的映射指向已死进程的 run，无人收尾）。故收缩为纯内存快照——少一个持久化字段、少一条重建路径、少一层回退判定，行为边界更小。
2. 自评审发现两处实现缺陷并同日修复：①「同 turn 双事件并发完成」竞态（见风险与对策）→ 完成路径认领集；② run 收尾 Complete 臂无条件读 acp.timeline 提块（非 issue 任务/issue 为空路径上的大文件无谓 IO，旧实现只在 issue 非空时读）→ 提取收敛回 issue 非空守卫内（门控判定 ⊆ issue 非空，语义不变）。
3. **内网验证根因修复（2026-09-30）**：M5-bp 部署后实测——扣留行为正确，但 agent 按指示输出块后任务仍保持 running。导出该会话 `acp.timeline.jsonl` 取证还原全程四轮：前三轮 agent 按协议正确拒绝输出块（正常扣留），第四轮用户明确要求「直接输出 completion-output」后 agent 首次真实输出块——**info 串被写到裸围栏的下一行**（非合法 GFM 形态，info 串必须与 ``` 同行），提取器按 GFM 语义正确拒绝（fail-closed 不动），但该结果与「完全未输出块」不可区分，任务静默卡在进行中（UI 渲染成普通代码块，肉眼不可辨）。定性：**好设计（围栏块协议 + 严格 GFM 提取）、实现不完整**——协议把精确语法当作完成判据，提示词却从未展示完整围栏示例（仅行内代码提及 info 串），agent 首次输出只能靠想象排版；且近似形态无任何诊断（M5-bl「静默 return 让触发缺失不可见」教训在提取层的同款复现）。修复两处：① 提示词（zh-CN/en）补精确的单行开栏示例 + 同行规则文案（契约测试锁锚点）；② `handoff.rs` 新增 `CompletionBlockProbe`（`block` + `near_miss_reason`：`info_on_own_line_after_fence` / `fence_info_contains_target`，纯函数、只识别**结构化**围栏滑落——prose 提及 info 串不误报，正常扣留轮次不告警），`completion_output_for_issue_done` 改回 probe（仍一次 timeline 读，探测复用已读文本零额外 IO），bridge 两处门控与迟到补发的未命中日志按近似原因升级 WARN（内网日志一眼定位，不再需要导出 timeline）。**不放宽提取器**：容忍近似形态有真实歧义（普通围栏内容首行恰为该串会误命中），违背 fail-closed 原则（宁保持进行中、不可误完成）。测试先行：4 个 handoff 纯函数测试 + bridge 接口层近似形态测试 + 契约测试锚点，先红（`near_miss_reason` None ≠ Some / 模板无 ```` ```completion-output ````）后转绿；提示词遵从性无法本地单测（模型行为），记录局限、内网打包 E2E 验证。另发现**独立**健壮性缺陷（不属本次根因，另立处理）：`complete_with_issue_done` 中 `complete_task` HTTP 失败仅 warn 且 `finalize_terminal` 无条件执行——完成请求失败时本地映射已清、任务在远端永久搁浅（心跳是 runtime 级持续，不触发 sweeper 重开，且无重试/对账通道）。

### 12.56 改动五十四：StateConfig 远程词汇改名的前置键 serde alias——升级不丢「最近完成」历史（M5-bq，2026-10-12）

**背景（用户内网实测）**：升级（重装安装包）码灵客户端后，需求管理页已完成的 multica 任务全部消失，对应本地会话仍在。

**根因追溯**：P1a（7085a5a7，2026-09-17）词汇中立化把持久化 JSON 键改名（`multicaCompletedTasks`→`remoteCompletedTasks`、`multicaTaskConversations`→`remoteTaskConversations`，条目内 `issueId`→`issueRef`、`issueKind`→`kind`），当时按「旧 state.json 可载入、旧索引数据破坏式丢弃」处理——`#[serde(default)]` 保证解析不失败，旧键作为未知字段被 serde 忽略。该决策把两个字段都当作「可重建索引」，但 §12.54（M5-bo）已确证**终态行（completed/failed）的唯一数据源是本地 `remote_completed_tasks` 历史（服务端不回传终态）**——它是用户业务历史的唯一副本，不是可丢弃缓存。升级后果链：新版读新键得空列表（历史「消失」）→ 任意一次 state.json RMW（如更新检查时间戳 `set_user_desktop_updater_last_checked_at`；wb 渠道 silentUpdate 常驻检查）整体重写文件 → 旧键被物理删除（此后不可恢复）。会话在主目录 `~/.maling/projects/`（`user_gold_band_root`），与 state.json（`%LOCALAPPDATA%\maling\`，`user_state_file`）异位，故不受影响——「已完成任务消失、会话还在」即这对存储位置不对称的直接投影。NSIS 配置未设 `deleteAppDataOnUninstall`（默认 false），排除卸载器删数据目录链路。定性：**好设计（存储无方言）+ 实现不完整（对唯一副本数据未做读兼容）**。

**设计**：对改名的 4 个键补 `#[serde(alias)]` 反序列化兼容（`src/config/mod.rs`）：`remote_completed_tasks` alias `multicaCompletedTasks`、`remote_task_conversations` alias `multicaTaskConversations`、`RemoteCompletedTask.issue_ref` alias `issueId`、`RemoteCompletedTask.kind` alias `issueKind`。alias 只作用于反序列化，序列化仍写新键：旧 state.json 直接可读，首次 RMW 重写即自动迁移为新键，无需版本号机制或启动迁移逻辑。`RemoteTaskConversation` 条目内字段未改名（仅容器键改名），无需 alias。

**边界（重要）**：alias 只能救回「旧键还在文件里」的数据。若升级后新版已运行过（任意 RMW 已重写 state.json），旧键已被物理删除，数据不可恢复。内网验收以 `%LOCALAPPDATA%\maling\state.json` 是否仍含 `multicaCompletedTasks` 键作为可恢复性判据。

**最小失败测试先行**：`state_config_deserializes_pre_rename_multica_keys`（lib config tests）——构造旧键 + 旧条目字段名（`issueId`/`issueKind`）的 state.json 片段，断言完成历史与续跑索引读出、且重写后落为新键/不再含旧键。修复前红（`len()==0`，与根因一致：旧键被 serde 忽略），加 alias 后转绿；回归 lib config 105 + storage 41 + app 64 + desktop multica 164 全过。

**过度设计评审**：仅 4 个 serde 属性，无新抽象/状态/迁移框架；不为「已物理删除」场景造恢复机制（无法实现）。遗留风险另行评估、不混入本次：① 终态历史唯一副本存 `%LOCALAPPDATA%`（易失位）与会话（主目录）的持久性不对称；② disconnect/换号/改 server 地址三条静默清空路径（`clear_multica_state_indices`）；③ `load_state` 失败静默降级空列表（仅 warn）。

**性能评审**：alias 仅反序列化时多一次键匹配；state.json 为小文件、读写频率不变，零运行时影响。

---

### 12.57 改动五十五：locale 词汇迁移的七语言对等修复——错误码文案表恢复 + 五语言 remote 域补齐（M5-br，2026-10-12）

**背景（M5-bq 修复的 review 产出）**：对分支整体 diff 的评审发现两组 locale 回归（非 M5-bq 引入，同源于 P1a 词汇中立化）。

**根因追溯**：以 merge-base（0a9c0c51）对账，分支对 7 个 locale 各删除 57 个 multica 域键，但只给 en/zh-CN 补了 81 个 remote 域新键：① `errors.multica.*` 12 键全删——后端命名空间分层设计（error.rs `code()`：仅 NotConfigured 迁 `remote.source-not-configured`，协议/连接域错误**保留** `multica.` 前缀）仍发射其中 11 个码，删除后 `displayAppError` 全部落通用「意外错误」兜底（7 语言含 zh-CN 均受影响）；② 5 个语言（ja-JP/es/ko-KR/pt-BR/zh-TW）删旧未补新，`fallbackLng: en` 下需求管理页整页英文。现有 i18n 测试只断言 en/zh-CN，缺口随 CI 放行。定性：**正确设计（命名空间分层）+ 实现不完整（前端 locale 迁移越界删码表 + 五语言迁移半途而废）**。

**设计**：纯 locale 数据修复，不动代码。① 7 语言恢复 `errors.multica` 11 键（译文取 merge-base 各语言原文；`not-configured` 已迁移不复活；`session-resume-failed` 虽为 dead-code 码表项，为与码表对齐一并恢复）；② 5 语言补齐 81 键 = 38 纯改名键搬运 merge-base 译文 + 5 值变更键（"Multica"→`{{source}}` 插值）逐语言定稿 + 38 全新键（remoteSync 技能同步块、需求管理空态/移出列表、issueKind/过滤、readiness、`errors.remote.source-not-configured` 等）人工翻译；③ `account.disconnect`/`switchAccount` 为分支已删功能的死键（代码零引用），不恢复。

**边界**：merge-base 之后 origin/main 上已修复的键（executionPlan 25 键、`keepAwakeState.effective_other`、acp.* 新增）不在本分支补——下次合并 main 自然到位，现在补会与 main 撞车。

**回归固化（接口层验收）**：新增 `web/tests/locale-remote-parity.test.ts`——以 en 为基准直读 7 个 locale 资源文件（不能经 `i18n.t` 判断存在性，`fallbackLng` 会用英文兜底掩盖缺失），断言 remote 域 91 叶子键全语言对等、`{{占位符}}` 集合与 en 一致、multica 错误码表 11 键在位且 `not-configured` 未复活；`app-error-i18n.test.ts` 增补 `multica.claim-conflict` 经 `displayAppError` 的 zh/en 精确文案断言。4 个 i18n 测试文件 22 例全绿；新测试 tsc 零错误；python 对账脚本复核 7 语言域内 missing=0、占位符差异=0。

**过度设计评审**：无新增抽象/状态/依赖；翻译复用 merge-base 既有译文（同一概念换键名），仅 38 新键为净新增文案；不建 locale 自动同步框架（parity 测试已把该类缺陷变成回归门禁，成本一行测试）。

**性能评审**：locale JSON 每文件净增 ~5KB、加载为启动单次 import，无运行时路径变化。

### 12.58 改动五十六：default 渠道暂时隐藏需求管理入口（origin/main 合入，2026-10-12 第十三次合并适配）

**来源说明**：origin/main 侧同位置条目（其自编号 M5-az/2026-09-29）与我方 §12.39–12.57 序列冲突。合入时按我方 Remote* 命名适配：main 的路由 `/chat/multica-tasks` 对应我方 `/chat/remote-tasks`，导航可见性 prop 链为我方已改名的 `showRemoteTaskManagement`（App 传 `appInfo.channel !== 'default'`）。原文如下：

**设计判断**：需求管理页面、路由与 Multica 生命周期设计保持有效，本次只是渠道导航投影范围调整，不删除能力，也不增加 CSS 隐藏、重复渠道状态或兼容分支。复用后端启动信息中的 canonical `appInfo.channel`，由 App 映射为侧栏可见性，`default` 渠道为隐藏、其他渠道保持现状。

**实现与验收**：`App → Shell → WorkspaceShell → ConversationSidebar` 只传递导航可见性；侧栏不挂载隐藏按钮，且隐藏入口不会因 `/chat/remote-tasks` 页面状态自动展开“更多”。增加侧栏 DOM 接口测试，固定隐藏时没有“需求管理”按钮、定时任务仍可见；执行定向 Vitest、TypeScript 检查、生产构建，并在 default 渠道 `/chat` deep link 验证。

**方案自评审**：过度设计——不新增配置、持久字段、路由守卫、状态机、依赖或兼容层，直接复用现有渠道事实源与导航投影。性能——仅减少一个按钮挂载和一次常量级条件判断，不增加 I/O、订阅、缓存、渲染范围或数据加载，无需 benchmark。

**验证结果**：`conversation-sidebar-more-navigation.test.tsx` 3/3 通过；`npm run web:build` 通过（TypeScript 与 Vite 生产构建）；default 渠道 `/chat` 实际展开“更多”后 DOM 中“需求管理”按钮为 0、“定时任务”按钮为 1，视觉检查确认入口对齐和留白正常。

### 12.59 改动五十七：completion-output 协议补工作流分支——收尾节点整体交付总结职责（M5-bs，2026-10-13）

**背景（用户内网实测）**：某工作流倒数第二个节点产出构建/部署/测试要点，最后一个节点只输出一条 SQL；issue 的 `completion_output` 最终只有那条 SQL——收尾（终态化 run）节点的块覆盖了真正的交付信息，下游子任务拿不到部署/测试要点。

**根因定性：原有设计缺陷**——`RunCompleted` 携带的 `attempt_dir` 是**终态化 run 的节点**（`orchestrator.rs` `emit_run_completed_lifecycle_event` 按 `node` 填充），bridge `completion_output_for_issue_done` 只读该 attempt；而 §12.50（M5-bk）把同一份完成输出协议**无差别注入每个 workflow worker**。对工作流节点该协议有两处事实错误的契约：①「只有附加块才会被标记完成」——workflow 完成不受块门控（§12.3/§12.55，graph 终态即 definition of done）；②「内容为该工作项的最终交付说明」——没有任何节点被赋予「综合前序产出写整体总结」的职责，收尾节点只是碰巧排最后，自然只写自己节点视角的产物。写侧契约错了，读侧「取终态节点块」只是忠实消费错契约；只改提取逻辑是补丁，正解是修写侧契约。

**实现（纯提示词，零代码改动）**：`remote_task_completion_protocol.md`（zh-CN/en）按执行模式分化为三段——公共交付说明定义与建议覆盖（不变）；direct 分支保留门控语义（块在才完成 + 未完成不得输出，锚点原文不变）；新增工作流分支（节点凭自身 prompt 中的「前序链路/工作流上下文」自我识别，零注入管线改动）：如实声明「不门控工作流完成、只有收尾节点的块会传递下游」+ 收尾节点判定（产出即最终交付、不再被后续节点加工）+ **内容必须是整个工作项的交付总结**——综合前序产出与附件，优先覆盖构建产物、部署/验证地址或分支、变更范围、下游测试要点；前序产出中不可得时对整体产出做简要总结兜底；**不得只写你自己节点的产物**。中英同构（remote_task_* 双语规则），`src/prompts.rs` 契约测试新增锁定（`整个工作项`/`不得只写你自己节点的产物`/`简要总结` 及英文对等），既有锚点（门控双向、围栏同行示例、篇幅契约）全部保留。

**方案自评审**：过度设计——零新抽象/状态机/持久字段/注入管线改动，复用 §12.50 既有单文件注入链路，条件分支靠节点上下文自选；曾评估「读侧终态节点无块时回溯聚合各节点块」的兜底（覆盖「收尾节点完全没写块」失效），对本次案例无额外帮助（有块但范围错），暂不做，内网实测后再评估。性能——纯提示词文本变化，无运行时行为改动，无需 benchmark。

**验证结果**：`prompts.rs` 契约测试 `remote_task_completion_protocol_templates_lock_fence_contract` 扩展后通过（既有锚点 + 新增工作流分支锚点中英全绿）。内网工作流场景待打包实测（收尾节点输出整体总结、含部署/测试要点）。

---

### 12.60 改动五十八：PR review 六项处置——skill 目录链接拒绝打包、备份清扫 recovery-first、续跑 run_mode 事实源、同步弹窗加载期门控、常驻文案五语言补齐（M5-bt，2026-10-16）

**背景**：feature_multica PR 负责人审批六项发现，处置：五项实现（②③④⑤⑥）、一项不修（①，用户结论）。

**① 不修（用户结论）**：「甲项目结果发乙项目」依赖两个任务的 remote_task_id 撞号，而 remote_task_id 为服务端分配、不同任务 ID 各异；客户端侧任务本就不与项目绑定，只维护任务↔远程任务绑定，项目归属是用户决策事项，无需客户端按项目隔离。

**实现②（skill 根目录是目录链接时拒绝打包）**：`resolve_own_skill_dir`（推送/导入上报解析 skill 目录的唯一入口）新增两道防护——入口目录 `is_symlink()` 整体拒绝（Windows 上 std 同时覆盖 symlink 与 junction，错误按 failed 上报 server 可见）；canonical 化 root 与 dir 后要求 `real_dir.starts_with(real_root)`（防中间目录链接把入口重定向到库外）。子条目 symlink 由 `collect_files_recursive` 既有跳过逻辑覆盖；本地 skill 删除路径的 `name` 来自扫描器输出（扫描器跳过链接目录）不受影响。根因定性：好设计（入口名称合法性校验）+ 实现不完整（未校验最终打开位置）。

**实现③（备份清扫 recovery-first + 名字精确匹配）**：`remove_stale_bundle_staging` 重构——名字解析从「前缀匹配」改为精确解析 `.{mstaging|mbackup}-{skill_dir_name}-{纯数字 pid}`（`my-skill` 不再误删 `my-skill-v2-777` 的残留）；backup 残留 recovery-first：**swap 提交前它是旧内容的唯一副本**（target 已改名过去），目标目录缺失（崩溃点在两次 rename 之间）→ 改名回 target 还原崩溃前状态，目标目录存在（swap 已提交）→ 先删 SKILL.md（对扫描器不可见）再整体删除；staging 残留照旧删除（从不是唯一副本）。清扫返回是否发生恢复，`write_bundle_instance` 据此把本次写入按覆盖语义走 swap——即便调用方因崩溃后 target 缺失而按新建分类进入（pull 重试的真实路径：扫描器看不到 `.mbackup-*` 目录 → 误判新建），恢复出的目录正是本次更新要替换的旧内容；不翻转则 rename(staging→target) 会撞已恢复目录而失败。多份 backup 残留恢复首个、其余按已提交残留删除。根因定性：原子 swap 设计正确 + 崩溃恢复路径缺失（无条件删除唯一副本）。远程改名导致重试目录名漂移的残留（新旧实现同样匹配不到）不回归——不可见、无数据损失、待同名 skill 下次写入清理。

**实现④（续跑登记 run_mode 事实源）**：续跑分支登记 `ActiveRemoteRun.run_mode` 从快照本次请求 `input.run_mode` 改为被续 run 持久化的 canonical `ConversationMetadata.run_mode`（`task_dir/authoring/conversation.json`，`resumed_run_mode` helper；词表同 `REMOTE_RUN_MODE_DIRECT`，`ConversationRunMode::as_str` 的 direct/workflow/auto）；读不到（极老任务/元数据损坏）回退本次请求选定并 WARN。`run_continue_background` 不换模式，续跑的实际执行模式即创建时模式；完成门控（bridge 只认 direct）按 UI 选定快照会「未交付即完成」或「已交付仍运行中」。Fresh 分支不动（新 run 确实以 input.run_mode 创建）。否决的替代：把 run_mode 冗余进 `RemoteTaskConversation` 索引——违反单一事实源（元数据已权威）。根因定性：好设计（M5-bp 模式快照）+ 实现不完善（续跑快照了错误的事实源）。

**实现⑤（同步弹窗加载期防误提交）**：`RemoteSkillSyncDialog` 列表 effect 开始即清空旧选择（挂起期列表仍是上一份的，旧选择留在后台会被提交到新工作空间）；同步按钮 disabled 条件与 `handleSync` 守卫均补 `loadingList`。根因：按钮仅凭选中数与 pulling 判可用，加载中旧选择依旧可提交。

**实现⑥（常驻文案五语言补齐）**：`conversation.sidebar.{keepResident,releaseResident,keepResidentHint,releaseResidentHint,resident}` 五键此前只维护了 en/zh-CN，补 zh-TW/ja-JP/ko-KR/pt-BR/es（与 en/zh-CN 同位）。保留英文词 Agent 不翻译；zh-TW 用台湾术语（工作階段/點選）。

**方案自评审**：五项均为最小根因修复，零新抽象/状态机/持久字段/依赖（③复用同一清扫点升级语义，④为读取既有 canonical 数据的 helper）。性能——②的防护是每次导入上报一次的两次 metadata/canonicalize 系统调用（非热路径）；③清扫的精确解析替代字符串前缀比较、无额外 I/O；④续跑登记点一次文件读取（续跑本身是重操作）；⑤⑥无运行时开销。

**验证结果**：均最小失败测试先行后转绿——② `resolve_rejects_skill_root_that_is_a_directory_link`（junction 根 + 库外目标 + 普通目录对照）修复前红（`unwrap_err` 收到 Ok）、修复后绿，`multica::local_skills::` 19 测全绿；③ `sweep_recovers_sole_backup_when_target_missing` + `sweep_matches_exact_skill_dir_name_only` 修复前红（唯一备份被删、相近名残留被误删）、修复后绿，既有 `bundle_write_sweeps_crash_leftovers_across_pids` 在新语义下保持绿（同时固化「恢复→覆盖」端到端路径），core `skill::` 23 测全绿；④ `resumed_run_mode_*` 两测（编译红 E0425 → 绿；夹具首次跑暴露 camelCase 字段名写错、按断言红修正），desktop `multica::` 167 测全绿；⑤ 新用例「reload in flight → sync disabled, stale selection cleared」修复前红（挂起期按钮可点且会调 `pullRemoteSkills`）、修复后绿，该文件 4 测全绿；⑥ `i18n-loading` 由 2 红转 7 绿。`cargo clippy --bin gold-band-desktop` 零错误；`tsc -p web/tsconfig.build.json` 零错误。全量 web 套件存在 22 例失败，经 stash 对照确认为 HEAD（97e8c533）既有、与本批改动无关。

---

### 12.61 改动五十九：issue 需求附件中介下载——claim 时拉取并入任务输入附件通道（M5-bu，2026-10-18）

**背景**：multica dev 1be391b6 起任务载荷新增 `issue_attachments`（issue 直属附件元数据 id/filename/content_type，与 chat_message_attachments 同构、content_type 可空；omitempty 兼容——缺键=空数组；pending/detail/claim 三接口同构），DPMS 需求文件代理卡包含其中；附件内容经 `GET /api/attachments/{id}`（claim 响应的 mat_ 任务令牌鉴权，服务端实时代理）下载。multica 建议的客户端形态是「prompt 里列清单 + agent 自调 `multica attachment download` CLI」——**否决**：违反 B 方案中介设计（agent 永不直连 multica API）、用户机器无 multica CLI 可用性保证、mat_ 令牌进 agent 可见 prompt 属凭证泄漏。签名链接进 payload/prompt 同样被 multica 明令禁止（60s TTL）。

**实现**：码灵中介下载，成功件并入**原生任务输入附件通道**（零新增 agent 提示通路、零前端改动）：

- **wire**：`RemoteTask.issue_attachments`（`#[serde(default)]`，缺键/空数组归一空）+ `IssueAttachment`；`download_issue_attachment`（`Response::chunk()` 流式写 tokio::fs 文件，不在内存聚合整个文件；单次尝试不重试；per-request 120s 覆盖 client 级 30s——附件可能是大 zip；状态码不走 `map_status`——其 404→TaskNotFound 语义对附件不成立，统一按附件维度映射 `NetworkFailed`）。
- **时机**：`start_remote_conversation_run` Fresh 分支（resume 判定之后、簿记 spawn_blocking 之前）——续跑不发新首条 prompt、无新建任务附件通道；claim 时刻数据即执行依据（领取即定版）。先下载到系统临时目录 `gold-band-multica-attachments-<task_id>`，成功路径并入 `input.attachment_paths` → `create_conversation_run` 既有 `fs::copy` 进任务 `authoring/inputs` → provider `resolve_attachments`（图片预算内联派生、文本≤预算内嵌、大/二进制 File 引用不整读进内存）→ chips / 自动重试 / 整任务重跑全复用；run 创建后临时目录 best-effort 清理（create 失败回滚路径同样清理——下次重试按新 claim 重新下载）。
- **失败语义（state-lifecycle §7 局部失败隔离）**：逐文件下载失败 / 零字节响应 / 令牌缺失 = **软失败**（DPMS 同步附件用户不可控，任一文件硬失败会永久阻塞整个任务）——成功件照常附带，失败清单经隐藏区段新块 `runtime/remote_task_attachment_failures.md`（zh-CN/en）告知 agent「不臆测缺失文件内容、与用户确认替代来源」；`remote_task_hidden_section` 扩为两块（完成输出协议 + 附件失败清单，双缺席→None，空失败 byte 级回退单块形态）。仅中转目录创建失败 = **硬失败**（磁盘满/权限：附件既无法下载也无法交代）——新错误码 `multica.attachment-stage-failed`（结构化 `{message}` 参数，七语言文案）→ release 回滚回 queued 供重试。
- **文件名治理**：server filename 是不可信输入——`sanitize_attachment_filename`（剥路径成分防目录穿越 / Windows 非法字符与控制字符替换 / 保留设备名 CON/PRN/AUX/NUL/COM1-9/LPT1-9 含带扩展名变体 / 尾部点空格 / 180 字符限长保扩展名 / 空名回退附件 id）+ `disambiguate_attachment_filename`（扩展名前 -2/-3 序号消歧——create 侧按 basename 落 inputs，同名互相覆盖；与 composer 既有本地附件 basename 同池消歧）。
- **前端**：零改动（成功件 chips 走既有 task-inputs 附件渲染）；仅新增 `errors.multica.attachment-stage-failed` 文案（七语言同位）。

**方案自评审**：过度设计——两度收敛后落地：① 否决「prompt 清单 + agent CLI 下载」（中介原则 + 凭证边界）；② 否决自建 `.gold-band/multica-attachments/` 平行存储 + 自定义附件 prompt 块（发现既有 `attachment_paths → authoring/inputs → task-inputs` 原生通道后改为复用，零新持久存储、零自定义 prompt、零前端新组件）。最终新增面：一个 wire 字段镜像、一个下载方法、一个临时目录生命周期（run 创建后即清，不常驻）、一个软失败模板、一个错误码。性能——空附件（绝大多数任务）零开销（不建目录、不取令牌）；有附件时串行单次尝试、逐文件 120s 上界、流式落盘不进内存，只发生在发送路径（本身即网络事务），pending/detail 仅解析不下载，resume 路径零改动，无 UI 刷新影响。composer 附件上限校验（10 个/单件 25MB/总 100MB）不经由——server 授权的任务输入 ≠ 用户选择器文件，超限大文件由 provider File 引用兜底（不内联不整读）。

**验证结果**：wire 解析 2 测（值解析 + 缺键/空数组/无 content_type 元素兼容）、隐藏区段测（两块拼接顺序、空失败回退、非 issue 任务失败清单独立成段、双语）、sanitize/disambiguation 4 测（穿越剥离/非法字符/保留名/限长保扩展/空名回退/序号消歧）、staging 4 测（空清单零开销、令牌缺失软失败、下载失败软失败含原因、目录创建失败硬失败→错误码）全绿；desktop `multica::` 178 测、lib `remote_task` 7 测全绿；clippy multica 文件零告警；七语言 leaf-key 对等（各 2734）。内网带附件任务（zip 下载、DPMS 代理卡、失败清单展示）待打包实测。

### 12.62 改动六十：输入框附件可见性（元数据 chips）+ staging 可观测日志（M5-bu 追加，2026-10-18）

**背景**：内网实测反馈「点执行 → 进入会话初始输入框时未见附件加载」。根因两层：① **期望与实现时点错位**——12.61 把附件下载绑定在发送阶段（claim 响应带 mat_ 令牌后），「点执行」只做只读详情拉取（`get_remote_task_requirement`），附件元数据在 `RemoteTaskVm` 无字段透出、直接丢弃，输入框自然无任何附件痕迹（非下载故障）；② **服务端硬约束**（webank 源码验证）：`auth_token` 只在 claim 时铸造（`types.go:137-142`），只读详情走的 `resolveTaskRequirementBody` 不带令牌且注释明确 attachments 属 claim-only enrichment——**详情阶段下载文件本体无凭证可用**（除非 multica 改服务端令牌铸造时机，违背其令牌设计）。结论：输入框阶段唯一可行能力是元数据展示；文件本体下载保留在发送阶段不变。

**实现**：

- **元数据投影（后端）**：`RemoteTaskVm.attachments: Vec<RemoteTaskAttachmentVm>`（filename/content_type，camelCase），仅 `from_detail` 回填——`issue_attachments` 元数据经 `IssueAttachment::display_name()`（新增方法：空/纯空白文件名回退附件 id，与软失败清单同一展示名事实源）归一；pending/active/终态行不回填（侧栏轮询不为无人消费的元数据增加负载）。旧 server 不发该字段 → 空数组，前端不渲染。
- **绑定携带（前端）**：`ConversationComposerRemoteBinding` 增加 `attachments`（`readonly RemoteTaskAttachmentVm[]`），`handlePrepareRemoteTask` 从 `detail.attachments` 透传进 `prefill`。随绑定同生命周期：编辑正文不丢、删绑定 chip / 进排程（互斥解绑）即一并消失；不进本地附件管线（`draft.attachments` 是已选文件 + 预览 URL，语义不同）。
- **chips 渲染（前端）**：`ComposerContextArea` 新增可选 `remoteAttachments`——复用既有上下文 chip 行（quotes / 本地附件 / 工作区文件引用同排），只读 chip（Paperclip 图标 + 文件名截断，`data-composer-remote-attachment-chip`），无移除/预览交互，Tooltip 提示「发送时自动下载并附加」（`conversation.composer.remoteAttachmentHint`，七语言同位）。仅主 composer 传入（ACP composer / 排程对话框无远程绑定，可选参数零改动）。
- **可观测日志（后端）**：`stage_issue_attachments` 分级记录——空清单 info（区分「server claim 响应未携带 issue_attachments」与「携带但下载失败」）、逐件成功 info（task_id/attachment_id/local_name/bytes）、逐件失败与零字节 warn（task_id/attachment_id/error）、令牌缺失 warn（task_id/count）。运行日志默认 Info 级（`RuntimeLogLevel::Info`），内网排查无需改配置即可见（bug-fix-verification 外部系统例外的替代证据）。

**方案自评审**：过度设计——纯元数据透传 + 只读渲染 + 日志，无新状态机/持久化/请求（详情响应本就含该数据，`serde` 已解析）；路由键与 chips 复用既有 draft 绑定生命周期与 `ComposerContextArea` 组件，不新建平行状态。性能——发送路径每次用户动作新增至多数行日志；渲染为少量静态 chips；无 IO 新增。待内网确认的服务端事实（本地 webank fork 早于 1be391b6 无法验证）：① claim 响应是否实际携带 `issue_attachments`（空清单 info 日志可判定）；② `GET /api/attachments/{id}` 在 mat_ 下是否返回文件流（webank 基线该路由为 workspace 作用域元数据 JSON，`file.go:622`；文件流在 `/api/attachments/{id}/download`，`file.go:775`）——若 1be391b6 未将其改造为 mat_ 可用文件流，逐件失败 warn 日志会直接给出状态码与错误，届时按日志修正 client 下载路由。**（两项事实已于 2026-10-19 内网复测落定：①已携带——输入框 chips 正常展示；②基线语义未变——下载到的"文件"实为元数据 JSON，即 §12.63 修正的 bug。）**

**验证结果**：vm 投影 1 测（display_name 回退 + content_type 透传 + pending 不透出）、camelCase 序列化锁定更新（空数组 `[]`）、draft 绑定生命周期 1 测（prefill 携带 → 编辑保留 → 解绑/进排程消失，本地附件管线不混入）、page prefill 透传断言更新、chips DOM 契约 3 测（只读无按钮、空列表整区不渲染、与本地附件同排）全绿；desktop `multica::` 179 测全绿；`tsc`（build 配置）零错误；i18n 七语言 leaf-key 对等 + parity 测试通过；web 生产构建通过。clippy 无新增告警（multica 命中均为遗留风格 lint，不在本次改动行）。内网复测关注：输入框 chips 出现（server 发字段）+ 发送后 task-inputs chips / 运行日志逐件结果。

### 12.63 改动六十一：附件下载路由修正——元数据路由误用改 `/download` 子路径（M5-bu 修复，2026-10-19）

**背景**：内网复测（服务端已部署 1be391b6）反馈：输入框元数据 chips 正常展示，但发送后会话里的附件打开后**不是文件内容，而是一段含 id/url/filename 的 JSON**。根因：`download_issue_attachment` 误用了元数据路由 `GET /api/attachments/{id}`——webank 基线（`file.go:622` `GetAttachmentByID`）该路由返回附件行元数据 JSON（正是用户看到的字段集），文件本体在 `GET /api/attachments/{id}/download`（`file.go:775` `DownloadAttachment`；router.go 注册为 user-scoped auth-only，从附件行自解析 workspace 并在内部做成员校验）。§12.62 预判的「待内网确认事实②」由此落定：1be391b6 未改造基线路由语义，`/download` 即正确通道。

**实现**：`client.rs::download_issue_attachment` 请求路径一行修正（`/api/attachments/{id}` → `/api/attachments/{id}/download`）；错误消息随 `path` 变量自动携带新路由；方法与字段文档注释三处同步两路由语义（含服务端三模式：CloudFront 302 / presign 302 / proxy 直出——共享 `reqwest::Client` 默认策略跟随重定向且跨主机剥 `Authorization`，对签名 URL 恰好正确）。鉴权不变：`mat_` 是绑定 (agent, task, workspace, owner) 的通用 actor 凭证（webank daemon.go MUL-3292：「any API call the agent makes ... recognized server-side as actor=agent」），user-scoped 路由天然接受，内网实测元数据路由 200 亦已佐证中间件接受 mat_。

**方案自评审**：过度设计——零新增抽象/状态/依赖，纯路由契约修正；不加「响应像元数据 JSON 即判失败」的启发式防御（附件本就可以是 .json 文件，正确端点即正确语义）。性能——无变化。

**验证结果**：红绿测试（bug-fix-verification）——先改既有软失败测试断言为期待 `GET /api/attachments/a1/download`（错误串与真实请求共用同一 `path` 变量，钉住请求路由），旧代码红、修正后绿；desktop `multica::` 全量定向测试全绿。

### 12.64 改动六十二：composer leading adornment 槽并排化——multica 绑定 chip 不再被 @role 标签挤掉（M5-bv，2026-10-21）

**背景（用户报告）**：需求管理点 multica 任务「执行」进入输入框后，在正文开头输入 `@角色`，开头的 multica 任务标签（绑定 chip）被「覆盖」消失，但任务实际仍关联（发送仍走远程链路）。

**根因（数据显示分层错位）**：数据层无缺陷——`parseCommittedSlashItem` 认定正文开头的 `@role ` 为已提交角色**不会**清 `draft.remote`，`handleSubmit` 原样转发绑定、发送走 `start_remote_conversation_run`；缺陷纯在显示层：§12.28（M5-ap）把绑定 chip 与 slash 标签设计为同一 leading adornment 槽位**互斥**（渲染三元 `committedSlashCommand ? <SlashTag/> : remoteBinding ? <chip/> : null`），依据是「绑定预填的是任务需求文本、非 slash 命令」这一假设。但「远程任务 + 指定执行角色」是合法且有意义的用户组合（发送链路同时转发 role snapshot 与远程绑定），假设与真实用法冲突——slash 提交后 chip 消失，用户误以为绑定丢失。定性：好设计（claim-at-send 绑定生命周期）+ 实现不完善（显示层互斥假设错误）。

**方案（同槽并排，零数据层改动）**：
- leading adornment 槽抽为独立组件 `web/src/components/conversation/ComposerLeadingAdornment.tsx`：slash 标签在前（锚定正文首个 token）、绑定 chip 随后（`gap-1`），条件并排渲染；`adornmentRef` 挂槽容器，`useLeadingAdornmentTextIndent` 以 ResizeObserver 测**槽总宽**做首行缩进，天然支持多标签。抽取同时解除「home `ConversationComposer` 依赖过重不挂组件测」的历史限制（§12.22/§12.28 同款理由）——adornment 槽成为可独立 DOM 测试单元。
- `ConversationComposer`：`remoteChipActive = Boolean(remoteBinding) && !committedSlashCommand` 的抑制条件删除，indent 启用改为 `committedSlashCommand || remoteBinding`；顺带清理 5 个随迁移失效的 import（Badge/Globe/SlashCommandInputTag/agentIcon{Src,Class}/SLOT 常量）。
- **Backspace 让位契约不变**（`shouldBackspaceClearRemoteBinding` 的 `!hasCommittedSlashCommand`）：slash 提交时正文起点 Backspace 先拆角色标签（unwrap），语义正确，非本次缺陷。

**方案自评审**：过度设计——零新增状态/依赖/reducer action，纯渲染条件修正 + 为可测性抽取组件，复用既有缩进测量机制。性能——纯渲染条件变化，ResizeObserver 已存在，无影响。

**验证结果**：红绿流程（bug-fix-verification）——先按互斥行为等价抽出组件并接线（tsc 零错），新增 `web/tests/composer-leading-adornment.test.tsx`（4 测：双标签并存 + × 解绑回调 / 仅 slash / 仅 chip / 双无不渲染槽），并存断言在互斥实现上稳定红（`expected '开发' to contain 'Multica · 修复登录超时'`，与根因一致），改并排后转绿；两个既有源码契约测试锚点迁移——`conversation-composer-remote-chip`（chip 主题对比断言改读组件文件）与 `conversation-composer-autosize`（滚动收口断言改跨文件双断言：消费点在滚动包装层内 + 组件内槽常量）；相邻 composer/slash 10 套件 104 测全绿 + tsc build 范围零错。UI：本地 vite dev 打开 composer 首屏渲染正常、console 零错误；@role 菜单与真实 multica 绑定的完整视觉链路依赖 Tauri IPC + 内网 multica（本地不可达，既有环境约束），逻辑已由 DOM 测试覆盖，并排视觉留内网验收。

---

### 12.65 改动六十三：completion-output 覆盖清单分级——构建包名/部署环境升级为核心必保，收尾节点继承前序 CI/CD 产出（M5-bw，2026-10-21）

**背景（用户需求）**：completion-output 协议的设计初衷是确保下游拿到**构建与部署情况**——具体为构建的包名与部署的环境，这是最重要的输出；测试要点、变更范围等属锦上添花。原提示词把全部字段平铺为「建议覆盖」，无优先级区分；工作流场景中若 CI/CD 节点已输出构建部署信息，收尾节点（如测试/总结节点）可能因「构建部署非本节点产物」而丢掉这些信息，只写自己节点的产出。

**方案（提示词文案分级重写，零代码逻辑改动）**：`src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md` 同构重写（模板无变量，minijinja 空上下文渲染即原文；注入条件不变——issue 关联任务才注入）：
- 覆盖清单由平铺改两级：**核心必保**＝构建出的包名/制品名（依赖包名、Docker 镜像名、安装包文件名等）+ 部署的环境（环境名及访问地址/命名空间/目标位置），并加防歧义条款——本次交付不涉及构建或部署时明确写一句，区分「确实没有」与「遗漏未写」；**次要锦上添花**＝变更范围、下游测试要点、自测结论与已知边界、环境/数据/账号、验证地址或分支，篇幅允许时补充且不得挤占核心信息。
- 工作流分支补继承义务：前序节点（尤其 CI/CD 节点）产出中已给出构建包名、部署环境等核心信息时，即使构建部署并非收尾节点本节点执行，也必须携带进自己的块一并输出；仅前序不可得且本工作项确无此类信息时才退化为整体简要总结。
- 门控语义（direct 双向门控 / workflow 不门控）、≤2000 字符契约、围栏同行格式规则均原样保留。

**方案自评审**：过度设计——纯提示词文案重构，零新增状态/依赖/机制，分级只是把既有平铺列表重组为优先级表述。性能——提示词长度微增（约 +200 字符/语言），注入路径与渲染成本不变，无影响。

**验证结果**：契约测试 `remote_task_completion_protocol_templates_lock_fence_contract`（src/prompts.rs）既有锚点全部保留，新增 M5-bw 锚点两语言各 5 条（构建包名/制品名、部署的环境、不涉及构建或部署、尤其是 CI/CD 节点、携带进你的块 / en 对应），定向测试 + `prompts::tests` 全模块 16/16 绿；相邻 `multica::`（gold-band lib，4 测）与 `multica::bridge`（desktop bin，20 测，含 completion-output 提取与近形态拒绝）全绿。过程中发现并修复本机既有环境污染：260+ 个提示词文件被此前某构建步骤写成 CRLF（git 经 clean 归一化显示 clean，`include_str!` 编译期嵌入原样字节触发 `bundled_prompt_line_endings_are_platform_independent` 失败）——按 `.gitattributes` eol=lf 语义规范化后全绿（工作区重检出重建 stat 缓存，最终真 diff 仅 3 个目标文件）。实际效果依赖内网 agent 行为，留内网验收观察下游是否稳定携带包名/环境字段。

---

### 12.66 改动六十四：需求正文 markdown 转义解码——requirement_text 边界把 tiptap 序列化源码转纯文本（M5-bx，2026-10-21）

**背景（用户报告）**：需求管理点 multica 任务【执行】进入输入框后，正文里每个下划线前多出一个反斜杠（`my\_app` 形态）；multica web 页面上同内容显示正常。

**根因（源码证据）**：multica web issue 编辑器（ContentEditor，tiptap，`contentType: 'markdown'` + `editor.getMarkdown()` 保存）经 `@tiptap/markdown` 序列化，`escapeMarkdownSyntax`（`text.replace(/([\\`*_[\]~])/g, "\\$1")`）把 `\` `` ` `` `*` `_` `[` `]` `~` 转义后存库；web 端展示时再解析 markdown（`\_` → `_`），往返自洽所以 web 视觉正常。码灵拿到的是 server 原样返回的 **markdown 源码**（webank `daemon.go` detail/claim 响应不反转义），而码灵把需求正文当**纯文本**消费（composer 预填 + agent 首轮 prompt，均无 markdown 渲染层）——转义层从未被解掉。这不是码灵链路漏了转义处理（全链路审计确认前端/后端无任何转义逻辑），而是**层语义错配**：markdown 源码 → 纯文本缺一次转换。定性：好设计（server 原样存储往返一致）+ 消费端缺层转换。

**方案（码灵侧解码，方案 A）**：新增纯函数模块 `multica/markdown_source.rs`，`markdown_source_to_plain_text(source) -> String` 按 CommonMark 转义语义反转序列化转义，接线在 `RemoteTask::requirement_text()` 尾部（`.map(markdown_source_to_plain_text)`）——全部来源（quick-create/chat/comment/autopilot/handoff/issue_description/title）统一解码，composer 预填（`detail_prefill` → `vm.requirement`）与 agent 首轮 prompt 单点覆盖。规则：
- 围栏代码块（``` / ~~~，≤3 空格缩进、≥3 字符开栏、同字符且长度 ≥ 开栏才闭栏）内容逐字保留；
- 行内代码 span（等长反引号 run 闭合）内容逐字保留；**未闭合的开栏按字面处理、其后照常解码**（前瞻匹配闭合 run，避免单个杂散反引号吞掉全文解码）；代码 span 不跨段落（空行）与围栏配对；
- 其余位置 `\` + ASCII 标点 → 标点；`\` + 其他（Windows 路径 `\U`、非 ASCII、行尾硬换行）→ 反斜杠原样保留；
- 只消除转义层，不做 markdown → 文本结构转换（`**`、`#`、链接等语法原样保留，与「码灵把正文当纯文本」的消费语义一致，也避免 pulldown-cmark 全量渲染的过度转换）。
- 明确不解码：`detail_prefill` 的 DPMS 溯源块与上游交付说明块（`parent_output`）——二者非 tiptap 作者产出（服务端派生 / agent completion-output），保持原样。

**方案自评审**：过度设计——单文件纯函数 + 单点接线，无新增状态/依赖/ crate（未引入 pulldown-cmark）；前瞻匹配是 CommonMark 语义本身要求（未闭合回退字面），非假设性复杂度；文档化简化（4 空格缩进代码块、setext 标题、info 串反引号校验）均有依据：tiptap 代码块序列化为围栏式，不会产出这三类形态。性能——requirement 正文为 KB 级一次性转换，段落级 O(n) 主导、反引号前瞻最坏 O(n²) 仅在病态多反引号文本出现且 n 为段长，可忽略；转换发生在 claim/detail 拉取边界，不在任何热路径。

**验证结果**：红绿流程（bug-fix-verification.md）——先写集成测试 `remote_task_requirement_text_decodes_markdown_escapes`（client.rs，JSON fixture `\\_` 即 tiptap 产出形态），未修复实现上稳定红（`left: Some("包名 my\_app\_1 部署 dev\_cluster")`）；实现后转绿。新模块单测 11 条（标点集/非标点保留/连续转义/代码 span 等长闭合与未闭合回退/跨段与跨围栏不配对/围栏逐字与闭栏条件/≤3 空格缩进/行尾反斜杠/CRLF/纯文本不变）全绿；`multica::` 全套件 191/191 绿（含 client 优先级既有测试、vm requirement 预填、commands/bridge）。真实内网 multica 任务的端到端视觉效果（本机 server 不可达）留内网验收。内网侧根治（tiptap 序列化转义策略）另行整理需求文档交内网团队，不在码灵侧代偿。

---

### 12.67 改动六十五：completion-output 核心信息三项化 + 任务完成后会话内修订（M5-by，2026-10-21）

**背景（用户需求，与其他开发讨论后确定）**：① 交付说明至少简要覆盖三项——构建（打包的包名）、部署（部署的环境）、改动（极其精简的改动范围）；② 任务完成后用户可在会话中继续对话调整交付说明内容（如「加上建议测试点」），agent 重新生成 completion-output 并更新到 multica 侧远程任务；③ 完成判定逻辑不变——仍以首次 completion-output 判定任务完成，后续仅调整内容。

**评估结论（零代码逻辑改动的依据）**：修订链路已由 M5-bl 的「终态后补交」通道（`bridge.rs relay_late_completion_output`）完整承载，语义与本次需求同构——终态后追问 turn（`AcpTurnFinished` 无在飞映射）与终态后 run（`RunCompleted` 反查不到 active run）双触发，按 local_task_id 从 completed 历史找回 issue 关联，从最终回复提取**最后一个** completion-output 块，`PUT /api/issues/{id}` **只带 `completion_output` 键**（服务端字段级 merge，不动 status，issue 被 reopen 也不拉回 done）幂等覆盖；本地状态零改动。缺的只是 agent 行为指引——协议提示词没有「完成后可修订」条款，后续轮用户要求调整时 agent 可能只口头回复不输出块，relay 无内容可送。

**方案（纯提示词，zh/en 同构）**：`src/prompts/{zh-CN,en}/runtime/remote_task_completion_protocol.md`：
- 核心信息三项化：构建包名/制品名 + 部署环境 + **改动范围（极其精简地说明本次改了什么，一两条即可）**；防歧义条款（不涉及构建部署时明确写一句）保留；变更范围移出次要清单（其余次要项不变）。
- 新增「任务完成后的修订」段（位于工作流段之后、格式段之前，模式中立）：已输出过完成块后，用户在会话中要求调整交付说明时，回复末尾**重新输出完整的新块**——整块覆盖已存内容、非增量差异，核心信息必须保留；此次输出**只更新远程侧存储的交付说明内容、不改变任务完成状态**；格式与 ≤2000 字符限制不变；用户未要求调整时不主动再输出块（防后续对话把内容意外降级覆盖）。
- 门控语义（direct 首块门控 / workflow 收尾节点）逐字未动。

**方案自评审**：过度设计——零新增抽象/状态/端点/依赖，修订复用既有迟到补写通道，改动仅为提示词文案 + 契约锚点 + 文档；修订条款是单一新定义（不与门控条款重复表述，条件域不相交：未完成 vs 已完成）。性能——提示词约 +150 字符/语言；relay 逐 turn 触发路径既有（先 state.json 小文件过滤非 multica 任务），无新热路径。

**验证结果**：红绿流程——契约测试先加 M5-by 锚点（zh：极其精简/重新输出完整的新块/不改变任务的完成状态；en 对应）在旧正文上稳定红，改写两语言正文后 `prompts::tests` 全模块 16/16 绿（M5-bw 及更早锚点全部保留）；两文件 LF 行尾无污染。修订的实际端到端效果（agent 在后续轮遵从修订条款、relay 覆盖到 multica）依赖内网 multica 与真实 agent 行为，留内网验收；bridge/relay 侧无改动，其行为已被既有 191 项 multica 套件覆盖。

---

### 12.68 改动六十六：需求正文 HTML 层解码——requirement_text 边界剥离 HTML 标签与实体（M5-bz，2026-10-21）

**背景（用户报告）**：M5-bx 验证通过后，multica 拉取任务在码灵远程任务输入框中仍有大量 `<p></p>` 形态 HTML 要素被直接打印。

**根因（与 M5-bx 同一条序列化链路的第二层）**：tiptap 保存链路对文本节点做 HTML 实体编码（`encodeHtmlEntities`：`&`→`&amp;`、`<`→`&lt;` 等；空段落序列化为 `&nbsp;`），正文以 markdown 存储时混入 HTML 岛屿（本地 webank 版 `<strong>` 等 `htmlReopen` 标签、DPMS 同步的 `<p>` 块正文）；web 端 markdown 渲染器把 HTML 岛屿按 HTML 渲染、实体按文本解码，视觉正常。码灵纯文本消费无渲染层，HTML 标记与实体原样透传。M5-bx 只解了反斜杠转义层，HTML 层未覆盖——定性不变：好设计（server 原样往返）+ 消费端缺层转换，继续在 `requirement_text` 边界补层。

**方案（扩展 `markdown_source_to_plain_text`，单遍扫描无回环）**：解码器在既有围栏/代码 span 保护结构上，段内扫描新增 `<` 与 `&` 两类分支（与 `\`、`` ` `` 分支并列，逐字符单遍、源位置决定先后）：
- 原始 HTML 标签按 CommonMark 标签语法匹配（开/闭/自闭合/注释；属性区按属性语法校验，引号内 `>` 不终止标签），不匹配（`a<b 且 c>d`、未闭合）→ `<` 按字面。投影：`<br>` → 换行（连续 `<br>` 各自换行，保留空行意图）；块级标签封闭集（p/div/h1-h6/li/table 等）→ 段落分隔换行（行首/紧随换行后不叠加，防裸空行）；行内标签剥壳保留内容；注释消失。
- HTML 实体：封闭命名集 `amp`/`lt`/`gt`/`quot`/`nbsp`（NBSP）+ 十进制/十六进制数字实体；未知/缺分号/超标量范围 → 按字面。
- **防误剥关键**：`&lt;p&gt;` 与 `\<p>` 解出的是用户字面文本 `<p>`（实体/转义产物是文本节点，不是源内标记），单遍扫描天然不回环二次剥离；代码块/代码 span 内 HTML 与实体逐字保留。

**方案自评审**：过度设计——零新增依赖/状态/接口，纯函数内新增 4 个私有 helper + 1 个标签分类枚举 + 块级标签封闭集常量（业务常量集中定义）；标签语法校验按 CommonMark 而非「扫到 `>` 即标签」，是 `a<b 且 c>d` 这类中文正文正确性的必要精度，非假设性复杂度；实体只收封闭命名集（HTML 全量实体表 2000+ 条对 tiptap 产出无意义）。性能——与 M5-bx 同为 KB 级一次性转换，段内 O(n) 主导（标签/实体匹配各自常数级前瞻），无回环重扫，claim/detail 边界非热路径。信息有损为文档化边界：`<a href>` URL、无 alt 的 `<img>` 内容丢失（纯文本投影本质）。

**验证结果**：红绿流程——先加 8 条模块测试 + 1 条集成测试（`remote_task_requirement_text_strips_html_and_decodes_entities`，`<p>` 包裹 + `&nbsp;` 空段 + `&amp;` 混合 fixture），旧实现上稳定红（7 赤为 HTML 原样透传、1 为预期笔误修正）；实现后模块 19/19 绿、`multica::` 全套件 200/200 绿；clippy 新代码零告警。字面文本不误剥（`&lt;p&gt;`/`\<p>`）、代码区内保留、畸形尖括号字面均有测试锁定。真实内网任务端到端效果留内网验收。

---

### 12.69 改动六十七：composer 预填新增任务名称首块（M5-ca，2026-10-21）

**背景（用户需求）**：远程任务点【执行】后，输入框预填应以任务名称开头，其后才是 DPMS 溯源、上游交付说明、需求正文——远程任务从 multica 列表点入，输入框自身不带标题展示，用户与 agent 都缺任务身份上下文。

**方案**：
- 新模板 `src/prompts/{zh-CN,en}/runtime/remote_task_title.md`（`remote_task_*` 按规则只保留两语言）：`任务名称：{{ title }}` / `Task name: {{ title }}`，注册 `RUNTIME_REMOTE_TASK_TITLE`（常量 + BUNDLED_PROMPTS 清单 + 渲染测试）。
- `detail_prefill` 块序调整为 **[任务名称块, DPMS 溯源块, 上游交付说明块, 需求正文]**；`title_block` 取 `thread_name`，过 `markdown_source_to_plain_text`（与正文同一解码边界——标题同为 web 侧序列化产出，防 `\_`/HTML 残留进首行）+ 空白过滤，缺省/纯空白跳过块。
- **移除 `requirement_text()` 的「皆空回退 title」**（根因处理，非补丁）：title 原先混进正文槽是因为预填里没有它的专属位置；名称块落地后回退只剩重复（无正文 issue 的预填会把标题输出两遍）。移除后无正文预填 = 名称块 + DPMS/上游块，信息不丢。开发阶段破坏式更新，删除旧回退路径。

**方案自评审**：过度设计——单行模板复用既有 `localized_prompt_zh_en!` 机制与块拼接结构，无新抽象/状态/接口；回退移除是净删代码。性能——预填多一次常数级模板渲染（detail 拉取边界，非热路径）；`thread_name` 解码为单次 KB 级以下转换。

**验证结果**：模板渲染测试（中英标签锁定，`prompts::tests` 17/17 绿）；vm 预填测试更新为四段块序（名称块首行锚点、无正文不重复、空白标题跳过、标题解码、en 渲染），`multica::` 全套件 201/201 绿；clippy 改动文件零告警。输入框真实视觉效果留内网验收。

### 12.70 改动六十八：状态对账与任务重新执行——batch-status 回填 issue 状态 + 终态行 rerun 入口（M5-cb，2026-10-22）

**背景（multica 侧任务重试改造，码灵同步）**：multica 新增两个 daemon API——`POST /api/daemon/tasks/batch-status`（≤500 id/批，回 `{task_id, task_status, issue_id, issue_status, issue_status_category, issue_status_name?}`，`issue_status_category ∈ {todo, backlog, in_progress, done, canceled}` 是客户端开关）与 `POST /api/daemon/tasks/{taskId}/rerun`（202 回全新任务对象：新 id、queued、同 issue、`attribution.rerun_of_task_id` 指回源任务）。码灵侧需要：① 看板终态任务周期对账刷新本地镜像 issue 状态；② 终态且 issue 允许（todo/backlog/done，**in_progress 排除**——multica 按 issue+agent 串行派发）时提供「重新执行」入口；③ 202 后新任务自然进板、按 task id 去重；④ 非 202 错误原样回显可重试；⑤ **移动语义**（内网验收后产品决策修订，2026-10-22）：点击重新执行后源终态行退场、看板呈现「已完成的任务移回待办」的单行移动——旧终态行不得与活跃后继同板，旧会话悬空（本体不动，仅会话列表直达）。

**方案**：
- **数据**：`RemoteCompletedTask` 增 `issue_status_category` / `issue_status_name`（serde default——旧 `state.json` 缺键即 None，本地终态历史落库时留空、由对账回填）；wire `RemoteTask` 增可选 `attribution.rerun_of_task_id`；`RemoteTaskVm` 三投影字段 `issue_status_category` / `issue_status_name`（仅 `from_completed` 携带）/ `rerun_of_task_id`（仅 `from_remote` 从 attribution 提取）。
- **接口**：client 增 `batch_task_statuses(task_ids)`（空集短路不发请求、按 500 分批、`LIVENESS_TIMEOUT_SECS` 短超时、无重试——loop/页面驱动天然下一轮重试；404→`TaskNotFound` 供旧 server 降级）与 `rerun_task(task_id)`（网络级重试；非 2xx 经 `extract_error_message` 宽容提取错误体 `error`/`message` 字段、200 字符截断，包进 `NetworkFailed("{path}: HTTP {status}: {message}")` → 前端 `{{message}}` 透传直显）。Tauri 命令 `reconcile_remote_task_statuses`（`reconcile_in_flight` 单飞；TaskNotFound 静默 Ok(0)——旧 server 无该路由即无按钮/标记，前向兼容零协商）与 `rerun_remote_task`（回新 task id；成功即真删除源终态历史行——`with_state` 原子 RMW 复用 `remove_completed_task_entry`，先落盘后 emit `remote-tasks-updated`；删除失败不阻断 rerun（202 已成事实），仅记 warn 由读取侧过滤兜底）。
- **实现**：bridge 纯函数 `apply_reconciled_issue_status(state, rows)` 按 task_id 反查终态历史、diff 计数，无变更不落盘（`with_state` dirty 语义）；变更 >0 才 emit 事件触发前端重拉。前端**页面驱动对账**（用户决策）：multica + 已连接 + 有工作空间时 mount 即一轮 + 30s 轮询，`document.hidden` 跳过，手动刷新附带一轮（静默失败不打扰）。看板 `canRerunRemoteTask`（终态 + category 闭集）/ `showsIssueStatusChip`（todo/backlog——done 与任务终态一致无提示价值）纯函数判定；「重新执行」按钮（RotateCcw）+ rerun 徽标（queued 且 `rerunOfTaskId` 非空——multica「归组到源行」在扁平 4 列的投影）+ issue 状态 chip（自定义状态名优先于分类 i18n）；202 后**直接重取列表**（用户决策：server pending 单一事实源，不做本地注入，按 id 去重由三源合并既有逻辑保证）。**移动语义（⑤）**：`get_remote_tasks` 三源合并前对终态行做 rerun 血缘退场过滤——纯函数 `filter_rerun_retired_terminal(pending, terminal)` 收集 pending 行 `rerun_of_task_id` 集合，命中的终态行不进本次响应（只过滤不写回——data-loading 规则 §5 读命令不顺便执行维护写；pending 拉取失败集合为空即不过滤，失败安全）。持久退场（rerun 命令 202 后真删除）与视图过滤互补：前者兑现移动语义的数据层，后者兜底他端机器发起的 rerun 与删除瞬时失败的残留。他端 rerun 且他端完成（我端从未见 queued 后继）时旧终态行会重新出现——此时任务链确实回到终态，不算错误展示（罕见：同一 agent runtime 两台机器）。


**方案自评审**：过度设计——无新表/新聚合/新后台任务：对账复用终态历史（有界 50）+ 既有 `with_state`/事件通道；rerun 复用 pending 拉取与合并去重；单飞 bool 对齐 `completing` 先例；旧 server 降级走 404→静默，无版本协商；移动语义复用 `remove_completed_task_entry`（remove 命令同款内部件）与 `merge_workspace_tasks` 旁的纯过滤函数（约 15 行），无新状态/新命令/新 IPC/新轮询。性能——对账单批 ≤500 id POST（本地历史上限 50 实际一批）、30s 页面驱动且隐藏即停、无变更零落盘零事件；rerun 是用户点击级低频路径（202 后多一次有界 ≤50 条的本地 RMW 落盘）；血缘过滤 O(P+T) 纯内存集合；前端纯谓词 + 两枚 Badge，无额外渲染层。

**验证结果**：`multica::` 全套件 210/210 绿（新增 attribution 解析 / batch 请求序列化 / 行宽容解析 / `pick_error_message` JSON 优先+截断 / `apply_reconciled_issue_status` 回填与跳过 / `begin_reconcile` 单飞 / `from_completed` 新字段携带 / `filter_rerun_retired_terminal` 血缘退场与失败安全两例）；web `tsc -p web/tsconfig.build.json` 零错误、remote-task-board + remote-task-management-page 59/59 绿、locale-remote-parity 绿（rerun 判定真值表、徽标/chip 渲染、按钮禁用态、容器 rerun 流与错误回显、对账 mount/门控/手动刷新触发）；7 语言 i18n 同步（`rerunTask`/`rerunBadge` + `issueStatus.todo/.backlog`）。看板真实视觉效果与内网联调（对账回填、rerun 202、源终态行退场与新任务进待办）留内网验收。

### 12.71 改动六十九：任务名称块去标签——预填首行即标题本身，模板链路删除（M5-cc，2026-10-22）

**背景（用户需求）**：M5-ca 的名称块带「任务名称：/Task name: 」标签前缀；内网验收后用户要求去掉——输入框开头直接就是任务名称本身，标签是冗余装饰。

**方案**：
- 标签是模板存在的唯一理由（中英标签差异）；去掉后模板只剩单变量 `{{ title }}`，语言无差异、无装饰文案，模板层失去存在意义。按「杜绝无效抽象 + 开发阶段破坏式更新」删除整条链路：`src/prompts/{zh-CN,en}/runtime/remote_task_title.md`、常量 `RUNTIME_REMOTE_TASK_TITLE`（含 `_ZH_CN`/`_EN` 派生与 BUNDLED_PROMPTS 清单项）及渲染测试。
- `title_block` 去掉 language 参数与模板渲染，`markdown_source_to_plain_text` 解码 + 空白过滤后直接返回标题文本；`TitleTemplateContext` 删除。块序、解码边界、空白跳过、`requirement_text()` 不回退 title 等既有决策全部不变。
- 顺带补齐 M5-cb 遗留：`RemoteCompletedTask` 新增两 Option 字段后 `gold-band` lib 测试两处结构体字面量未补字段（当时只跑 `--bins` 目标，lib 测试未编译到），补 `issue_status_category/_name: None`。

**方案自评审**：过度设计——纯删除（模板文件×2/常量×3/清单项/上下文结构/渲染测试净减），无新增抽象。性能——少一次常数级模板渲染，其余不变。

**验证结果**：`multica::` 全套件 210/210 绿（标题断言改为裸标题、标题块的 en 重复断言随语言分支消失而删）；`prompts::` 16/16 绿（标题模板测试随删除移除）；lib `with_state` 6/6、`remote_completed_tasks` roundtrip 1/1 绿；clippy（desktop bins+tests / gold-band lib）零错误（`gold-band --tests` 下 feishu 连接器 2 条既有 deny 与本改动无关）。真实输入框视觉效果留内网验收。

---

### 12.72 改动七十：completion_output 随 /complete 回传——multica 协议改造对接（M5-cd，2026-10-23）

**背景（multica 侧协议改造）**：multica `POST /api/daemon/tasks/{taskId}/complete` body 新增可选 `completion_output`——完成与写输出**同事务**（后写胜、幂等）、≤16,000 Unicode 字符（rune 计数，超长 **400 且任务不置 completed**）、仅 issue-bound 任务生效（其余静默忽略）、旧 server 忽略未知键可安全灰度；test 子任务 claim 的 `parent_output` 即该值（领取侧无需改动）。事故动因：工作流任务完成时 `output` 字段被填节点名（「CRIS-收尾归档」），真实交付说明未送达 multica——旧通道（`update_issue_status(done)` PUT 附带）与 `output` 摘要混用，交付物无独立字段承载。

**方案（multica 改造清单 §2 对接，通道统一 + 采集分级 + 客户端降级）**：
- **通道统一**：`completion_output` 一律由 `/complete` 携带（daemon 统一回写，改造清单 §2.4 推荐项）；`update_issue_status` 回归纯 status（`UpdateIssueStatusRequest` 删输出键，开发阶段破坏式更新）；`complete_with_issue_done` 三处共用路径只改一处。迟到补发 PATCH（`update_issue_completion_output`，M5-bl/M5-by 修订链路）保留——「后写胜」语义天然兼容。fail 路径不带（改造清单 §2.3，`FailRequest` 本就无该字段）。
- **采集分级（改造清单 §2.1，用户修正后的落地形态）**：**块仍是首选 payload**——direct 门控命中传围栏块（语义不变）；工作流**块优先、最终回复全文兜底**（`workflow_completion_output`：`completion_output_from_reply` 失败 → `clamp_completion_output(全文)`）——§12.55 形态滑落（info 串另起一行等提取 fail-closed 场景）时块内容原样嵌在全文里送达，交付不落空（本次改造的终极目标）。非 issue 任务不采集不读 timeline（server 忽略该字段，保持「非 issue 不做大文件读」既有优化）。
- **超长自检 + 降级（改造清单 §2.2，必须项）**：发送前 16k 预截断、尾部截断标记（`[已截断：超出 16000 字符上限]`，**标记计入上限**——发送值恒 ≤16,000 rune，块/全文两路同口径）；`complete_task` 改自建请求回路 `complete_post_once` 四分类（Delivered/Retryable(5xx·传输)/Fatal(401/403/404/409)/Rejected(400 含 server message 透传))——不走 `post_json`+`map_status`（400 被并入 `NetworkFailed`，`with_terminal_retry` 会先盲退避 6 次才轮到降级）；携带字段收 400 → **立即**降级去字段重发一次（无退避，完成优先），降级后仍 400 才按可重试语义走终态重试上抛。
- **观测**：工作流收尾投递日志分级——near-miss 形态滑落 WARN（指令遵从度信号，全文兜底已送达）、无块正常回复 INFO（兜底送达）、连全文都无内容 WARN（issue 照常 done，fail-open）；direct 门控 miss 日志（§12.55）不变。

**方案自评审**：过度设计——无新增状态/缓存/聚合；`CompletePostOutcome` 四分类是「400 必须先于终态重试被识别」的必要载体（复用 `map_status` 做不到），自建请求沿 `rerun_task` 先例；降级态用 `Arc<AtomicBool>` 承载（`FnMut` 闭包可变捕获不能逃逸 async future）。性能——timeline 读次数不变（`final_reply_and_probe` 单次读共享给门控与兜底，替代原 `completion_output_for_issue_done`）；截断 O(n) 一次；降级重发仅罕见超长路径多一次 HTTP。

**验证结果**：`multica::` 217/217 绿（新增：client socket 集成测试×4——携带字段单请求成功（旧 server 形态）/ 400 降级两请求断言（首请求带键、降级不带且 `output` 不丢）/ 400 分类透传 server message / 404·500 分类；handoff×5——块优先 / §12.55 事故形态全文兜底 / 兜底超长截断带标记 / 空回复 None / 16k 标记计入预算；bridge 采集组合改为 `(reply, probe)` 共享单次读）。clippy 我方改动零新增警告（2 条 collapsible-if 顺手修复，let-chains 与元组匹配为仓库既有风格）。内网验收对照 multica 改造清单验收用例 1-6（工作流带值 / 空串不动 / rerun 覆盖 / 超长截断与降级 / 旧 server 忽略 / 非 issue 忽略）。

### 12.73 改动七十一：工作流 completion_output 采集改 attempt 级找块——裁决 JSON 误上送修复（M5-ce，2026-10-23）

**背景（内网实测事故）**：轻量工作流（grill → dev-test → accept）的 accept 节点启用 AI 输出验证，按协议以 JSON 裁决（`{"reason":"String","result":"boolean"}`，`task-workflow.md` §5.3）作为**最后一条** assistant 消息收尾（provider 侧 `evaluate_output_artifact_from_run` 从终端消息提取该 JSON 判定节点成败）；而 §12.72 的工作流采集只投影**最后一条**消息并做全文兜底——两条协议竞争「最终 assistant 消息」槽位，agent 折中执行（主回复带块 → 裁决 JSON 收尾）后，采集拿到的是裁决 JSON：块提取失败 → 全文兜底把裁决 JSON 整条当交付物写入 `issue.completion_output`。暴露路径：完成后用户在会话内要求补时间，终态后补交通道（最后一条消息口径 + 只认块）写回了正确块内容——两通道行为分叉定位根因。

**方案（采集口径按协议事实分叉 + 删兜底 + 提示词对齐）**：
- **attempt 级找块（工作流口径）**：`attempt_completion_scan` 在最终 attempt 的**全部** assistant 消息中倒序找块、last-block-wins（与 multica「多次输出整体覆盖，只保留最终结论」同义）——块在哪里就到哪里取，不再假设它在最后一条消息。近似形态探测同步 attempt 级（无块时保留最新一条 near-miss 原因，诊断贴近终态）。
- **删除全文兜底**：`workflow_completion_output` 移除。机器裁决/报告全文都不是交付物；提不出块 → **不发送该字段**（multica「缺省/空串不动现值」合法行为），near-miss WARN 可观测、终态后补交通道可恢复（本次事故已实测验证该恢复路径）。原文档「形态滑落时交付不落空」目标降级为「形态滑落可观测、可恢复」——兜底保证被证明会发出错误内容，宁可空缺不可错发。
- **direct 门控 / 终态后补交维持最后一条消息口径**（`final_completion_probe`）：协议语义是「块在最终回复末尾」，direct 门控的扣留设计与补交的「重新输出完整新块」都以最后一条消息为判据，不随工作流口径变化。`AttemptCompletionScan` 一次 timeline 读出双口径（`final_probe` + `attempt_probe`），run 收尾路径零额外 I/O；turn 门控/补交仍单次读只算 final 口径。
- **提示词对齐**（zh-CN/en `remote_task_completion_protocol.md`，工作流段落并入既有定义）：收尾节点需以 JSON 裁决收尾时，块输出在裁决 JSON **之前**的最后一条人类可读回复末尾——agent 行为与采集口径互相锁定。

**方案自评审**：过度设计——净删除一条代码路径（兜底）与一个启发式候选（跳过纯 JSON 消息的兜底目标判定，评审中否决：结构判定代理「机器裁决」在自定义输出约束下会误伤），无新增状态/聚合/依赖；`AttemptCompletionScan` 双口径是「run 收尾一次读供 direct/工作流两分支」的必要载体。性能——`load_timeline_items` 调用次数不变（run 收尾 1 次、turn 门控/补交各 1 次），attempt 扫描是已物化消息上的内存倒序行扫描，相对文件读可忽略；补交通道仍先小文件过滤再读 timeline。

**验证结果**：先红后绿——双消息 timeline（主回复带块 + 裁决 JSON 收尾）用例在旧实现（`final_assistant_reply` + `workflow_completion_output` 组合）上稳定失败，实际送出 `{"reason":"验收完成","result":true}` 与内网实测一致；修复后 `attempt_completion_scan` 同一 timeline 的 `attempt_probe.block` 命中前置块、`final_probe` 不命中。`multica::` 215/215 绿（handoff 新增 5 用例：裁决 JSON 回归 / 跨消息 last-block-wins / 前置消息 near-miss 只标记不发送 / 无块·缺目录默认 / final 口径仅看最后一条；删除兜底 4 用例与 bridge 采集组合 3 用例——场景由新 API 用例承接）。clippy 我方改动零新增警告。

### 12.74 改动七十二：completion-output 中段上送与主动拉取——`$upstream.completion-output` 令牌 + 外部角色（M5-cf，2026-10-24）

**背景（并行诉求）**：M5-cd/M5-ce 后 `completion_output` 单一写入来源是工作流终态 `/complete` 回传——下游测试任务必须等父 issue done 才能拿到交付说明，开发/测试天然串行。目标：开发工作流的**中段节点**（如构建/部署节点）把阶段性交付提前发布到远程工作项，测试工作流的中段节点在父未 done 时拉到快照提前启动。难点在拉取侧：测试任务要读的是**父工作项**的 completion-output 而非自己的——跨任务读取必须由码灵做中介（agent 无凭证、无全局作用域工具）。

**内网确认（2026-10-24，四项全确认，服务端零改动）**：① `GET /api/issues/{id}` 下发 `completion_output`（`*string` + omitempty，NULL 键省略）；② `PUT /api/issues/{id}` 单键写 `completion_output` **无状态门槛**（仅 ≤16,000 rune 上限；字段级 merge：缺省保留/携带覆盖/显式 null 或空串清空）；③ 子任务的 `parent_output` 派生读条件 = 父字段非空（不看父状态，「字段非空即下发」+ 派生读每轮自愈）；④ `parent_issue_id` 恒下发（无父为 null）。

**方案（写侧复用 + 读侧令牌，两端各一个外部角色）**：
- **写侧（中段上送通道）**：bridge 在 `NodeCompleted`（图节点成功边界，orchestrator 对工作流/自动编排节点发射，携带会话级 run_id + attempt_dir）后 `push_workflow_completion_snapshot`——复用 M5-ce `attempt_completion_scan` 找本轮 attempt 内最新块（last-block-wins），与 `MulticaRuntimeState.issue_output_pushed`（发送侧去重投影，内存态；`drop_active_run` 随生命周期清除防 rerun 首块误跳过）比较，**值变化才** `PUT` 单键 `completion_output`；失败仅 WARN 忽略（终态 `/complete` 仍是权威写入，后写胜自愈）。只作用于非 direct（workflow/auto 自动编排图）+ issue run（`is_workflow_issue_run`，与 `/complete` Passthrough 采集同口径）；不门控、不改完成判定、direct run 不涉及。**触发事件是 2026-10-25 内网实测回归后的修正**：初版挂在 `AcpTurnFinished` 上，但该事件是会话 prompt 层事件（仅聊天回合发射），图节点回合不经过——中段上送永不触发，multica 侧零写入（revision 停留在 claim 时刻）；同轮回归还发现谓词按字面量只认 "workflow"，漏掉 auto（发送默认模式、执行形态即工作流图）——两处已一并修正并以测试锁定。
- **读侧（拉取令牌）**：引擎令牌 `$upstream.completion-output`（与 `$end`/`$new-round` 同 `$` 前缀引用语法）可出现在节点 goal 或角色正文；`node_executor` 构建调用时先 `needs_upstream_resolution` 检查（无令牌零成本，测试锁定 resolver 不被调用），命中则经 `App::resolve_upstream_output` 回调（`with_upstream_output_resolver`——`with_prompt_turn_lifecycle` 同款 App 回调注册模式）到桌面侧 `MulticaUpstreamResolver`：自身 issue detail 解析 `parent_issue_id` → **有限退避轮询**（用户决策：15s 起、×1.5、间隔上限 60s、总预算 15 分钟，模块常量集中定义）父 `completion_output` 非空即命中 → 按界面语言渲染 `remote_task_upstream_pull.md`（父状态行 + 「父未完成时是阶段性快照可能被覆盖」口径提示）替换全部令牌出现（2026-10-26 起改为注入隐式隐藏区段并移除令牌，见改动七十四）。**单次快照语义**：值取节点启动时，父后续覆盖不自动刷新，重新进入节点重新解析。错误结构化 `UpstreamResolveError`（NoResolver / NoParentIssue / Timeout / Network）——未绑定远程任务或无父直接结构化失败，不静默空串。
- **角色交付（外部导入，用户决策：不进内置 profile，成熟后再内置）**：`docs/gold-band/产品设计文档/multica接入/外部角色/`——`导入包/`下「交付上传」（节点目标显式要求上传阶段性交付——completion 协议的显式例外条件由此满足）+「交付拉取」（角色正文嵌令牌，自包含）两角色文件，README 在导入包外（编排示例/使用前提/行为口径；「导入文件夹」递归扫 `.md`，README 混入会被误当角色导入失败）。
- **提示词协议修订**（zh-CN/en `remote_task_completion_protocol.md` 工作流段并入既有定义，不追加例外句）：其他节点输出的块作为阶段性快照上送、供下游在工作流结束前拉取提前启动（多次输出整体覆盖）；节点目标明确要求上传阶段性交付时即使产出还会被后续加工也输出块并注明阶段性口径；收尾节点的块仍是最终交付说明。新增 `remote_task_upstream_pull.md` 双语模板 + 常量/清单登记 + 渲染契约测试。

**方案自评审**：过度设计——服务端零改动、无新增表/状态机/持久字段；写侧复用 M5-ce attempt 扫描与 M5-by 单键 PUT 通道，读侧复用 App 回调注册模式与 remote_task_* 模板管线；`issue_output_pushed` 是发送侧幂等投影（值比较去重）而非第二事实源（权威值在服务端，重启丢失无害——PUT 幂等）；轮询为用户明确选择的等待策略且 15 分钟预算封顶，不为此引入长连接/推送（服务端无对应能力，不扩协议）；拒绝的替代：MCP 工具（全局作用域无会话身份、扩大写入面信任边界）、DSL schema 字段（连带前端改造）、内置 profile 特判（用户决策外部角色）。性能——上传通道每 turn 一次 attempt 内存倒序行扫描 + 值变化才网络 PUT（去重比较在锁内、PUT 在锁外，不持锁做网络 IO）；拉取仅含令牌节点启动时发生（先检查后解析），轮询最坏 ~17 次 GET（15s/22.5s/33.75s/50.6s/60s×n）预算封顶，阻塞发生在 run 专用 OS 线程（既有 orchestrator 线程模型，`block_on` 桥接为既有模式），不阻塞 UI/事件循环。

**验证结果**：引擎 `upstream` 过滤 7/7（`app::upstream` 令牌检测/替换 2 + `node_executor` goal 替换命中/无 resolver 结构化失败/无令牌零调用 3）、`node_executor` 全量 15/15、`prompts::tests` 17/17（新增 upstream_pull 双语渲染契约——状态与交付内容插值 + 阶段性快照口径锁定；completion 协议契约扩阶段性快照语义 8 断言）；desktop `multica::` 221/221（新增 upstream 渲染插值/口径锁定/轮询常量决策锁定 3、`is_workflow_issue_run` 作用域 1、`drop_active_run` 去重缓存生命周期 1、`IssueDetail` 可选键解码容忍 1）。clippy 我方改动文件零新增警告。**内网回归（2026-10-25）**：双工作流实测暴露写侧两处缺陷——①**主因：触发事件挂错层**。初版把 push 挂在 `AcpTurnFinished` 上，但该事件仅由会话 prompt 路径（`submit_conversation_prompt_inner`）在聊天回合发射，工作流/自动编排的节点回合只产生 `NodeCompleted`，中段上送永不触发；multica 侧证据（issue #1721）：claim 后 revision/updated_at 零变化，PUT 从未到达 issue 更新处理器。修正：push 迁至 `NodeCompleted` 臂（节点成功边界，`outcome == "SUCCESS"`），同一 spawn 内先 pin session 后推快照。②谓词按字面量只认 "workflow"，漏掉 auto（发送默认模式、执行形态即工作流图）——修正为非 direct 判据（与 `/complete` Passthrough 采集同口径），作用域测试先红后绿锁定 auto 命中。下游症状（测试任务拉取节点「卡住」）为①的后果：父字段恒空 → 拉取侧按设计走满 15 分钟有限退避预算后结构化 Timeout。`multica::` 221/221 复绿。**内网复验（2026-10-26）**：写侧双缺陷修复后重测，上传节点中段上送正常、multica 侧正确写入开发任务 completion_output；读侧复盘结论：拉取实际生效（agent 首条回复即引用快照结构与内容、时序吻合），「未生效」为误判——内容被内联进 system prompt 角色正文，消息流不可见；上送逐节点触发与注入位置两项缺陷由改动七十四（§12.76）修复。

### 12.75 改动七十三：completion 协议提示词重排——单一定义合并去冗（2026-10-26）

**背景**：completion 协议（`runtime/remote_task_completion_protocol.md`，隐藏区段注入）经 M5-bs→M5-cf 六轮语义叠加，「多次输出整体覆盖」三处重复、工作流段单段 600+ 字嵌套从句，用户反馈隐藏上下文过于冗长。

**方案**：按 prompt-authoring 规则 2（规则合并为单一定义、同一规则只留一处表述）整体重排，非语义删减：覆盖语义收敛到开头定义一次；内容分级（核心/次要/篇幅）合并为一段列表，CI/CD 携带条款并入核心信息条目；「何时输出」按 direct 门控 / 工作流收尾与阶段性例外 / JSON 裁决摆放 / 整工作项总结 / 完成后修订分段，每规则一处。全部既有语义锚点（围栏同行、门控双向、核心信息三项、阶段性快照口径、修订不改变完成状态等）原样保留——`remote_task_completion_protocol_templates_lock_fence_contract` 契约测试零改动通过，即语义无损的验收证据。两语言（该模板仅 zh-CN/en，属 `remote_task_*` 例外清单）结构对齐。

**验证结果**：`prompts::tests` 17/17（契约测试未改动即通过，锚点全保留）；正文长度约为重排前 60%。

### 12.76 改动七十四：上送语义收紧（节点声明门控）+ 拉取注入位置迁移（隐式隐藏区段）（2026-10-26）

**背景（两项，同日）**：
1. **上送**：M5-cf 中段上送对非 direct + issue run 的**每个成功节点**触发，而工作流节点普遍产出 completion-output 块——下游会在任意中间节点读到无交付语义的「交付」。用户语义要求：工作流模式下只有被指定承担阶段性上送职责的节点（配「交付上传」角色）或收尾节点产出并上送；direct 模式不变。
2. **拉取**：内网复验确认拉取实际生效（agent 首条回复即引用快照结构与内容），但内容被内联替换进 system prompt 的角色正文（`runtime/system.md`「当前节点角色」段）——会话消息流不可见、不可审计（用户展开「隐藏运行上下文」找不到交付内容）。按 AGENTS.md system/user prompt 划分，随执行变化的交付数据属 user prompt 区。

**方案**：
- **上送门控（节点声明制）**：「交付上传」角色正文内置 HTML 注释声明标记 `<!-- gold-band:upstream-upload -->`（常量 `UPSTREAM_UPLOAD_MARKER`，goal 直写等效）；bridge `push_workflow_completion_snapshot` 在 attempt 扫描前经 `node_declares_staged_upload` 判定——数据源为 run 启动时落盘的工作流快照（`current_run_workflow`，run 中途编辑不改变已执行节点语义）+ profile 当前存储，goal / 角色正文任一携带标记才上送；快照不可读 / 节点缺员（AiDynamic 节点无 goal/profile，动态编排 worker 不在任务工作流快照里）/ profile 解析失败一律 fail-closed（误上送会让下游拿到无交付语义的中间产物，宁缺勿滥）。**不做收尾节点检测**：终态 `/complete` 整体携带最后一块已是权威路径，last-node 判定需引入 sink 分析且与 `/complete` 存在竞态，复杂度收益为负。协议提示词同步收紧：工作流例外条件从「节点目标明确要求上传阶段性交付」改为「被指定承担阶段性上送职责的节点（配置了交付上传角色，或目标中带其声明标记）」。
- **拉取注入位置迁移**：`node_executor` 装配时解析结果不再内联替换令牌，改为追加首条 prompt 的隐式隐藏区段（标题 `Gold Band upstream deliverable`，内容即既有 `remote_task_upstream_pull.md` 渲染块，随「Gold Band runtime context」折叠块可见可审计）；正文中的令牌移除（`strip_upstream_references` 取代内联替换 API）。加载策略与首条隐藏区段完全一致：仅 `SessionMode::New` 解析并注入；Continue 回合只移除令牌不重新解析（快照已随 session 首条 prompt 存在，维持「节点启动时的值」单次快照语义）。拉取角色文件改写：令牌保留为声明开关，正文说明内容到达位置（隐藏运行上下文「上游交付说明」区段）。
- **部署耦合（内网注意）**：两个角色文件正文均已更新，**升级桌面端后需更新既有角色条目正文**（原地编辑替换，见 §12.77——「导入文件夹」是新建副本不是覆盖）。方向安全：旧桌面端 + 新角色 = 无门控仍逐节点上送（桌面端旧语义）；新桌面端 + 旧上传角色 = 无标记不上送——收紧方向一致，不存在「错误上送」组合。

**方案自评审**：过度设计——无新增持久化/状态机/跨层 identity：门控读既有 run 工作流快照 + profile 存储（节点成功边界、spawn 异步内、仅 multica 工作流 run 触发）；标记是纯文本声明，复用角色导入通道，无 DSL schema/前端改动；拒绝的替代：DSL 新字段（连带前端与旧工作流迁移成本）、last-node 检测（见上）。拉取迁移复用 `extra_hidden_sections` 既有通道与首条加载策略，消除 system prompt 中 ≤2000 字符的重复内联负载。性能——门控每成功节点至多两次小文件读（run 工作流快照 JSON + profile 文件），发生在 spawn 异步路径不阻塞热路径；隐藏区段使首条 user prompt 增量有界（交付块 ≤2000 字符 + 模板头）。

**验证结果**：lib `upstream` 过滤 9/9（令牌检测/无 resolver 结构化失败/无令牌零调用既有 3 + strip 全清除、声明标记谓词（含「前缀片段不算声明」负例）2 新增 + goal 解析→隐藏区段追加且令牌移除、Continue 回合只移除不解析且不触发 resolver 2 改写）；desktop `multica::` 221/221；`prompts::tests` 契约 17/17（协议收紧未触动任何锚点断言）；clippy 我方改动文件零新增警告。

### 12.77 改动七十五：门控数据源修复（动态编排叶子反查）+ 可判定性与部署指引修正（2026-10-26）

> **已被改动七十六（§12.78）取代**：完成边界反查（`completed_node_declaration`）整体删除——产出资格判定迁至装配侧按节点身份进行，与模型所见同源。本条的部署指引（原地编辑角色条目、新建任务验证）仍然有效。

**背景**：改动七十四合入后内网复测（两个工作流）报告两项异常：①每个节点仍产出 completion-output 块；②上报节点未中段上送（旧正文已确认带标记，排除角色未更新）。

**诊断**：
- ②**主因：门控数据源缺陷（代码缺陷，改动七十四引入的回归）**。两个工作流是 auto 执行形态（multica 默认发送模式）——run 快照编译为单个 `ai-dynamic` 外层节点（`compile_auto_workflow`），实际干活的动态 worker 是运行时编排的叶子，其 goal/profile 记在所属 dynamic graph 的 `DynamicNodeState.task/profile`（dynamic.rs:183），**不在任务工作流快照里**。门控只查快照 → 查无节点 → fail-closed 拦下所有 auto 节点（包括挂「交付上传」角色的）。改动七十四把「动态 worker 不在快照 fail-closed」当设计预期，是把 multica 默认执行形态拦死了的设计错误。
- ①**协议正文按任务冻结**：首条隐藏区段在任务创建时落盘（`first-prompt-hidden-sections`，node_executor），升级前创建的任务重跑沿用旧协议文本（「每节点上送」口径）——模型照旧文本产出属预期，新建任务才带新口径。
- **次生发现（部署陷阱）**：`import_profiles_from_folder` 对同名角色经 `resolve_unique_name` **新建副本**（「交付上传-2」，profiles.rs:761）而非覆盖——「重新导入」指引不可靠，须原地编辑既有条目替换正文。

**方案**：
- **门控数据源按执行形态分派**：lib 新增 `execution_plan::completed_node_declaration`（`App::completed_node_declaration` 薄封装）——workflow DSL 节点取 run 快照（外层 ai-dynamic 节点自身完成时 goal/profile 均空自然按未声明处理）；动态叶子按 round trace **逆序**（最新 attempt 优先）反查外层 `(node_id, attempt_id)` 的 `dynamic_graph_file` → `load_dynamic_graph` → 叶子 `task/profile`，与执行侧喂给 node_executor 的 `profile_show(node.profile)` 同源。查无此节点 `Ok(None)`；IO 失败 `Err`（fail-closed 但可判定）。
- **门控可判定性**：去静默 return——取数失败记 WARN，查无节点 / profile 引用悬空（含 builtin CI/CD profile 不在存储）记 debug，门控拦截处带 node_id 记 debug（区分「判定未声明」与「触发根本没跑」）。M5-bl 教训：静默 return 让触发缺失不可见。
- **部署指引修正**（外部角色 README 前提 4）：更新角色正文须**编辑既有条目整体粘贴替换**（条目不变，引用保持有效），禁止盲目重复「导入文件夹」；标记是 HTML 注释、渲染视图不可见需编辑视图确认；验证新语义用新建任务。

**方案自评审**：过度设计——无新增持久化/identity：动态叶子数据读既有 canonical 存储（dynamic graph 状态文件，orchestrator 同款 `load_dynamic_graph` 访问器，含既有版本迁移），返回值是纯读投影；不为单一 UI 竞态复制数据模型。性能——每成功节点至多快照 1 读 + trace 逆序至多每 attempt 一次 graph 存在性检查（workflow 节点无 dynamic 目录零额外读），小 JSON、spawn 异步路径低频，无热路径影响。

**验证结果**：先红后绿——新增 `execution_plan` 测试 4 项锁定（auto 叶子 task/profile 反查命中、同名外层多 attempt 逆序最新优先、workflow 节点走快照分支、查无节点 `Ok(None)`）；desktop `multica::bridge` 既有 20/20 全绿。

### 12.78 改动七十六：产出侧门禁（装配时节点身份指定产出资格）（M5-cg，2026-10-26）

**背景**：改动七十四/七十五的上送门控是**消费侧反查**——节点完成后，bridge 经 run 快照 + round trace + dynamic graph + profile 存储三跳反查该节点的声明再决定是否上送。用户重申根本意图：门禁应在**产出侧**（「在产出 completion-output 的地方做严格门禁」）——direct 模式不变；auto 模式只有最后一个节点（验收）产出；工作流模式只有收尾节点和「交付上传」声明节点产出。反查式门禁与模型实际收到的指令分属两个数据源（模型每节点都收到「产出块」协议、门控却在发送端再判一次），是典型的补丁式修复：判定与所见不同源，注定持续错位（改动七十四内网两轮回归的共同根因）。

**方案**：
- **指定判定进引擎、渲染进桌面（resolver 回调模式，与 `UpstreamOutputResolver` 同款）**：引擎侧纯函数判定——`workflow_node_designation(workflow, node_id, goal, profile_content)`：节点所有出边指向 `$end`（含无出边）= sink → **Final**；非 sink 且 goal / profile 正文携带 `UPSTREAM_UPLOAD_MARKER` → **Staged**；其余 → **Prohibited**。`dynamic_node_designation(kind)`：`Acceptance`（验收节点）→ **Final**，其余叶子 → **Prohibited**（动态图运行中生长，装配时无法预知谁是最后叶子——auto 的交付统一由 `/complete` 终态路径携带，无中段上送）。新 trait `CompletionProtocolSectionResolver`：`node_executor`（workflow）/`orchestrator`（动态叶子）装配首条 prompt（`SessionMode::New`）时判定 designation 并回调桌面侧 `MulticaCompletionProtocolResolver`——任务在飞映射关联 issue 才注入新隐藏区段「Gold Band remote task protocol」（`runtime/remote_task_node_protocol.md` 按 `designation` 渲染 final / staged / prohibited 三变体，双语）；本地任务静默跳过。
- **协议模板拆分**：`remote_task_completion_protocol.md` 收回 direct 专属（完成门控 + 整个工作项总结，删除工作流段）；新增 `remote_task_node_protocol.md` 承载图节点协议——final（收尾节点：整个工作项总结）、staged（阶段性上送 + 快照会被覆盖口径）、prohibited（不要输出块、不要复制前序块，短文本）三变体共享 JSON 裁决 / ≤2000 / fence 格式规则。
- **消费端收窄**：bridge `push_workflow_completion_snapshot` 删除完成边界门控（`node_declares_staged_upload` 与其数据链 `execution_plan::completed_node_declaration`、`App::completed_node_declaration` 整体删除）——scope 收窄为 `run_mode == "workflow"` + issue（auto 退出中段上送，deliverable 走 `/complete`），NodeCompleted SUCCESS + attempt 有块即 PUT，值去重与失败自愈语义不变。
- **冻结段联动**：claim 时冻结的隐藏区段（`first-prompt-hidden-sections.json`）改为按 run_mode 冻结——协议段仅 direct run 冻结（workflow/auto 只冻结附件失败段）；新协议区段注入前嗅探冻结段是否已含「completion-output」（仅比对冻结段列表——上游拉取区段含父任务任意文本，不得参与嗅探），旧任务沿用冻结正文重放。vm `remote_task_hidden_section` 增加 `run_mode` 参数。（M5-ct 起判据结构化：协议恒为冻结区段首块，判据改为区段以协议模板文档开头、位置锚定——附件失败清单里的 `completion-output` 子串不再误伤，见 §12.90 ①。）

**方案自评审**：过度设计——净删除：完成边界反查三跳（快照/round/graph + profile 存储）整体移除，新判定是装配时纯内存计算（图遍历一次 + goal/profile 字符串扫描，数据已在手）；resolver 复用 `UpstreamOutputResolver` 既定模式，无新增持久化状态/identity。性能——NodeCompleted 路径每节点少 2–3 次小文件读；装配路径 designation 判定 O(出边数) + 模板渲染一次，无 IO 新增。

**验证结果**：lib `prompts` remote_task 6/6（direct 契约改写——编排锚点移入 node 契约、direct 负锚点防两套产出规则并存；node 协议三变体 × 双语契约新增）；`app::upstream` + `app::node_executor` + `execution_plan` 过滤 30/30（新增 designation 判定：sink/标记/new-round 边非 sink/Acceptance 4 + resolver 嗅探 1 + `SessionMode::New` 才注入 1；反查函数与其 4 项测试删除后全绿）+ `orchestrator` 动态叶子指定（Acceptance=final / worker=prohibited）1；desktop `multica::` 222/222（新增 vm 按 run_mode 冻结协议 1、改写 `is_workflow_issue_run` 作用域——auto 退出中段上送 1）。clippy 我方改动文件零新增诊断（feishu/api.rs 两处 `unused_io_amount` 为 1.96 工具链 deny-by-default 存量问题，与本改动无关）。

### 12.79 改动七十七：拉取决策点 + 交付说明结构精简（M5-ch，2026-10-26）

**背景**：M5-cf/cg 在内网工作流全链路复测通过后，用户提出两项优化：①拉取节点首拉未命中时一律预算内轮询直至 Timeout（15 分钟）——父任务耗时长时用户只能干等，不灵活；应首拉空值后把控制权交给用户三选一：继续等待拉取 / 手动提供所需信息 / 跳过拉取直接继续后续步骤。②completion-output 结构中构建与部署信息是核心，产出、测试要点等附加信息应一两行带过，不得喧宾夺主。

**方案**：
- **拉取决策点（桌面侧 resolver 重构，引擎契约零改动——仍是 `resolve(task_id, node_id) -> String` 同步回调）**：
  - 首拉立即执行：命中 → 快路径照旧；**成功但空值** → 注册决策槽（`MulticaRuntimeState.upstream_pull_decisions`，键 `"{local_run_id}/{node_id}"`，Notify 唤醒，沿用 `issue_output_pushed` 的 run 级内存态定位与 `drop_active_run` 同生命周期前缀清理）+ 发 `gold-band://multica-upstream-pull-pending` 事件，前端决策 UI 三选一（M5-ci 起为会话内卡片；本版为 App 级全局弹窗）；**首拉网络失败不发决策点**（「拉不到」≠「父未产出」），预算内重试（M5-ci 修正：本版实现与该口径不符——网络失败也发了 pending，且首拉失败后即使后续轮询确认空值也永不开决策点）。
  - resolve 主循环 `tokio::select!`：决策 notify（manual → 渲染用户文本变体、skip → 渲染跳过变体、retry → 恢复轮询并立即拉取一次）vs 决策窗口 `DECISION_FALLBACK_AFTER`（60s，无人应答自动回落轮询——无人值守语义≈旧口径，不挂死）；轮询期 fetch 命中 → resolved；总预算 15 分钟不变，耗尽仍结构化 Timeout。任意退出路径统一收口：移除槽 + 发 settled 事件（resolved/manual/skipped/timeout）关闭前端弹窗；决策槽在轮询期保留（迟到 manual/skip 仍生效）。
  - 命令 `decide_upstream_pull`：决策字面量（retry/manual/skip）→ 枚举；manual 强制 trim 非空；锁内写决策取 notify、锁外 notify_one；无槽/无在飞 run/未知字面量 → `multica.upstream-pull-not-pending`（决策迟到竞态兜底，不改槽防手滑毁掉等待中的拉取）。
  - 前端：App 级 `MulticaUpstreamPullDialog`（纯事件驱动：pending 入队幂等、settled 收口清理、直接关窗=不决策由 60s 回落自愈；多个在飞拉取按到达顺序逐个弹；M5-ci 起整体移除，改为会话内 InterventionLayer 卡片）；模板 `remote_task_upstream_pull.md` 拆三变体（parent/manual/skipped，双语）——manual 明确标注「用户手动提供，非父工作项快照」，skip 带「缺信息如实说明、不要臆测」防线。
- **交付说明结构精简（协议文本收紧，无逻辑改动）**：`remote_task_{completion,node}_protocol.md` 与交付上传角色正文的次要信息段改为一两行带过且不得挤占核心信息（核心段——构建/部署事实——不动）。

**方案自评审**：过度设计——决策槽是 run 级内存态，复用 `issue_output_pushed` 生命周期口径与 MulticaConnectCancel 的 Notify 先例，无持久化字段/跨层 identity/aggregate；弹窗单组件纯事件驱动，无轮询无全局 store；未引入为单一竞态复制 canonical 数据的机制。性能——快路径（首拉命中）零新增；决策等待期零轮询零请求（旧口径此阶段已在按 15–60s 间隔打请求）；60s 回落保证无人值守带宽行为 ≤ 旧口径；协议文本每区段净减约百字符（首条 prompt 负载下降）。

**验证结果**：lib `prompts` remote_task 6/6（upstream_pull 三变体契约改写：parent/manual/skip 锚点 + manual 不带快照口径负锚点）；desktop `multica::` 228/228（新增：决策槽生命周期 3——注册/决策/取走保留槽、run+node 键隔离、drop_active_run 前缀清理含前缀投影视防；upstream 渲染 manual/skipped × 双语 2 + 决策窗口常量锁 1；错误码断言扩充）；前端 i18n 契约 7/7、locale parity/app-error 18/18、tsc 零错、`web:build` 通过。clippy 我方改动文件零新增诊断。弹窗实机交互需内网真实 multica 工作流验证（本地无法联调，见部署后复测）。


### 12.80 改动七十八：决策点收敛为会话内卡片 + 首拉网络失败口径修复（M5-ci，2026-10-26）

**背景**：M5-ch 批次分支自检发现两项问题，叠加用户对决策 UI 形态的拍板：

1. **UI 形态偏离产品先例（用户决策：改为会话内 InterventionLayer 卡片）**：权限请求、表单征询、人工检查等同类「运行中请求用户决策」均为会话内卡片（ACPChatDialog InterventionLayer），M5-ch 的 App 级全局弹窗是孤例——脱离任务上下文（看不到会话现场），且阻塞全局。
2. **代码与文档口径不符（首拉网络失败也发决策点）**：`upstream.rs` 在首拉 `match` 两分支后**无条件**注册决策槽 + 发 pending 事件，网络失败（`polling = true`）也会弹三选一——与「首拉网络失败不发决策点（『拉不到』≠『父未产出』）」的文档口径直接矛盾。
3. **根因是设计层缺陷而非实现笔误**：决策点开启绑定在「首拉」这个**时机**上，而正确的不变量应绑定在「首次**确认**父未产出」的**观察**上——拉取成功且空值才算确认。绑定时机导致两个症状：网络失败误开（把「拉不到」当成「父未产出」）；首拉失败后即使后续轮询确认空值也**永不**开决策点，用户在整个 15 分钟预算内无从介入，只能干等到 Timeout。
4. **死代码**：`handoff.rs::final_assistant_reply` 自 /complete 直传（98afa60a）后无生产调用方，`cargo check` 报 never used。

**方案**：

- **决策点开启口径改为不变量「首次确认父未产出」**：抽出纯分类 `ParentFetchObservation { Produced, ConfirmedNotProduced, Unreachable }`（`observe_parent_fetch` + `opens_decision_point`），首拉与轮询两个拉取点统一由分类驱动：
  - `Produced` → 快路径返回（不变）；
  - `ConfirmedNotProduced` 且尚未开过（`decision_open`）→ 注册槽 + 发 pending——无论该确认来自首拉还是后续轮询（修复症状 3 的第二半）；
  - `Unreachable` → 不开决策点，本地永不唤醒的 `Notify` 占位（select 纯走轮询 sleep），预算内静默重试（修复症状 2）。
  - 「只开一次」由 `decision_open` 保证：60s 回落后槽保留、迟到决策仍生效，不重复注册/发事件。
- **前端改会话内卡片**：删除 `MulticaUpstreamPullDialog.tsx` 与 App.tsx 挂载；新增 `web/src/components/conversation/MulticaUpstreamPullCard.tsx`：
  - `MulticaUpstreamPullIntervention({ taskId, readOnly })` 挂入 ACPChatDialog InterventionLayer（`pendingElicitation` 之后）：订阅 pending/settled，按 `event.taskId === taskId` 过滤（用户看哪个会话，卡片出现在哪；未打开的会话由后端 60s 回落自愈）；pending 入队幂等、settled 按 `{runId}/{nodeId}` 精确移除。
  - 卡片级 `MulticaUpstreamPullCard`：视觉对齐 PermissionRequestCard（同 `data-theme-role="permission-card"` 复用主题 recipe，图标 + 标题 + 描述 + 动作行），三选一 + manual 内联表单（trim 非空门控提交）；本地出队在**父级队列**而非卡片内部 state——同槽位 pending 再次入队时按 key 重建卡片（测试驱动修复：卡片内 `removed` state 会在实例复用时吞掉重入队的 pending）；`readOnly || useReadOnlyExperience()` 禁用全部决策入口；决策迟到（`multica.upstream-pull-not-pending`）幂等收起，其他错误保留卡片展示。
  - 卡片无关闭按钮：不阻塞会话，忽略 = 不决策，60s 回落自愈（弹窗时代的 dismiss 按钮随阻塞语义一起删除）。locale 9 key 原样复用（文案不含弹窗措辞，7 语言零改动）。
- **删除死代码**：`final_assistant_reply` 及其 2 项测试（`assistant_text_items`/`is_assistant_text` 仍有 scan/probe 生产调用方，保留）。

**方案自评审**：过度设计——净删除为主（弹窗文件 + 死代码 + 死代码测试），卡片复用既有事件/命令/locale/主题 role，无新状态机/持久字段/依赖；`ParentFetchObservation` 是三值纯分类、被两个拉取点真实消费（非测试专用抽象）；`decision_open` 是单个布尔守卫，未引入新 aggregate。性能——事件频率不变（每任务每节点至多一对 pending/settled）；卡片状态在最小消费边界（介入组件持有队列、每卡片自持表单态），订阅按 ACPChatDialog 实例一对 listener，无高频路径、无额外请求。

**验证结果**：desktop `multica::` 227/227（228 − 删 2 项死代码测试 + 新增 1：`decision_point_opens_only_on_confirmed_not_produced` 锁定口径——ConfirmedNotProduced 开、Unreachable 不开、trim 空白算未产出；resolve 装配依赖 AppHandle + 真实 client，无测试 harness，按 bug-fix-verification §3 例外以根因分析 + 纯函数契约锁定）；前端新增 `multica-upstream-pull-card.test.tsx` 6 例（taskId 过滤、重复 pending 幂等、settled 按槽位精确移除、retry/skip 决策参数、manual trim 门控与提交参数、not-pending 幂等收起 + 其他错误保留可重试、readOnly 全禁用）；`tsc -p web/tsconfig.build.json` 零错；web 全量 383 文件 2929 例通过；`cargo check` 警告数与改动前持平（20 条均为未触及模块存量）。实机事件链（pending 事件 → 卡片出现 → 决策 → settled 收口）本地无法触发（browser 模式事件订阅为 stub、multica 仅内网），待内网真实工作流复测。

### 12.81 改动七十九：PR 评审五项修复——解析注册竞态宽限、completion_output 写序、上传侧指定门禁、决策卡片水合、等待态保留（M5-cj，2026-10-27）

**背景**：feature_multica 分支 PR 评审确认 3×P1 + 2×P2，全部根因修复（非补丁式）：

1. **resolver 注册竞态（P1）**：`resolve()` 按 local task 反查 `active_runs`；run 启动发生在 `create_conversation_run_vm` 创建事务内，`register_active_run` 在其后（commands.rs fresh/resume 两路径均如此）——注册不是不发生，只可能**迟到**。上游拉取节点启动是引擎回调微秒级，旧实现 miss 即报「找不到在飞任务」直接失败节点。评审点：终态事件（秒级后）早有「先于事件归属反查」的守恒，但 resolver 路径（节点启动瞬间）未覆盖。
2. **completion_output 乱序覆盖（P1）**：快照 PUT（NodeCompleted spawn）与 `/complete`（RunCompleted spawn，同样携带 completion_output）并发飞；早节点 stalled PUT 晚于终态写入落地 → 服务端后写胜 → 过期内容永久驻留（run 已终态、无后续写入自愈），下游父任务读到旧交付。发送侧去重（`issue_output_pushed`）只防重复发送，不保到达序。用户曾提议「附带发出时间字段」——比较必须发生在**接收侧**（服务端改动，跨团队依赖，内网已确认端点无状态 2026-10-24），客户端侧比较无法撤回在飞 PUT，否决。
3. **上传侧门禁缺口（P1）**：M5-cg 产出门禁在**提示词侧**（装配时按 designation 注入协议），但协议块本质是自由文本——Prohibited 节点的回复仍可能含块（复述/引用上游交付说明），旧上送逻辑照单全收 → 禁止节点可覆写 issue 交付。
4. **决策卡片无水合（P2）**：卡片只靠 pending/settled 即时事件，重进会话后已待决的卡片消失（后端槽仍在、60s 回落在跑）。
5. **「继续等待」移除卡片（P2）**：前端 Retry 后 `onDone()` 收起卡片，但后端槽保留（`take` 不消费，迟到 manual/skip 仍可写入）——用户失去再次介入入口，与后端语义错位。

**方案**：

- **① 注册竞态宽限（#1）**：`resolve()` 反查 miss → `wait_for_active_run_within(MAPPING_GRACE=5s, POLL=50ms)` 轮询；注册到达即命中，宽限耗尽仍 miss 才报错（真异常）。不变量依据：fresh/resume 两路径注册都**无条件**紧跟 run 创建——「迟到毫秒级」是唯一正常形态，「5 秒仍不来」才是不变量被破坏。
- **② completion_output 写序（#2，状态规则 §6 合规形态）**：
  - `enum CompletionOutputJob { Snapshot {run,node_id,attempt_dir}, CompleteRun {remote_task_id,run,node_label,attempt_dir} }`；**事件回调内同步入队**（纯 channel send 不阻塞热路径，事件序 = 入队序）+ 全局单消费者 `drive_completion_output_jobs` 顺序执行（scan/dedup/PUT 全在消费者侧，临界区只有 send）。
  - 入队方：NodeCompleted SUCCESS（**入队侧 workflow+issue 预过滤**——run 在手，auto/direct 的成功节点不产生注定被拒的死 job；预过滤与执行之间 run 可能被 cancel，队列残留作业仍由执行侧会籍门禁权威拒绝）→ Snapshot；RunCompleted Success → CompleteRun（与快照同序——事件序上 RunCompleted 晚于全部 NodeCompleted，FIFO 保证终态内容最后落定）。Failure/Killed 不写 completion_output，保持旁路 spawn。
  - **不入队方及理由**：turn 收尾完成（direct 无快照写入方，与队列无并发交叠）；迟到补发（run 已终态，该任务队列作业已执行完，冲突仅「分钟级 stalled PUT 晚于补发」极端窗口，下次补发整体覆盖自愈）。
  - **认领升级 `begin_completion_for_run`**：队列把 CompleteRun 延迟到消费者执行后，与 turn 收尾（旁路 spawn）顺序不再同步——他方可在「复验映射 → 认领」两步间完成并 finalize（认领随 finalize 释放），单纯认领成功 ≠ 无人完成过 → 二次 complete。修复：单锁内完成「映射仍在本 run 名下（remote 键 + 双 local 身份匹配，防同键重注册误放行）+ 认领」；CompleteRun 执行前另有廉价映射复验（他方已收尾则零成本跳过 timeline 扫描）。
  - 全局单队列足够（不为 per-issue 建队列）：写入频率是节点边界/任务终态粒度，串行即该字段的正确语义；写入方按 run 聚簇，全局 FIFO 已保同 issue 序。
- **③ 上传侧指定门禁（#3，与模型所见同源）**：`CompletionProtocolSectionResolver` 签名加 `node_id`；装配回调时 Final/Staged 记入 `designated_output_nodes: HashMap<local_run_id, HashSet<node_id>>`（Prohibited 不记）；快照上送前查会籍，非指定（含 frozen 旧 run 无记录）fail-closed 拒绝（info 日志）并**先于** context/timeline 扫描退出（零成本）；随 `drop_active_run` 同生命周期清理。不重开完成边界反查（M5-cf 改动七十四两轮回归的根因——记录时点与模型收到的协议同一装配时点，天然同源）。
- **④ 卡片水合（#4）**：新命令 `list_upstream_pull_pendings(taskId)` → `find_active_run_by_task` → `list_upstream_pull_pendings(run)` 槽行（`MulticaUpstreamPullPendingVm`：taskId/runId/nodeId/parentIssueId/taskTitle/retried）；无在飞 run → 空列表（终态时槽已随 run 清理，空 = 无待决）。前端挂载时水合合并进既有事件队列，`settledKeys` 集合防护「settled 先于水合响应到达」的在途竞态（迟到行不复活已收口卡片）；失败静默（事件流仍是主数据源，不阻塞挂载）。
- **⑤ 等待态（#5）**：后端 `decide_upstream_pull` 的 Retry 置槽 `retried=true`（槽不消费）；前端 Retry 成功 → 卡片转 `waiting` 模式而非收起（跳过/手动入口保留——正是后端槽保留的语义；manual 进入后返回回到等待态）；水合 `retried=true` → 初始即等待态。settled 事件仍是唯一收口。

**方案自评审**：过度设计——② 是「事件序 = 执行序」的最小正确结构（unbounded channel + 单消费者，无重试/优先级/持久化队列这类假设性机制；测试执行器注入化后无生产专用抽象）；③ 是一张 HashMap<run, HashSet> 内存表，随 run 生命周期清理、零存储改动，未引入新 aggregate（不变量「只有装配时被指定的节点可写交付」在既有 canonical 状态里无法表达，属必要新增）；①④⑤ 均为单点小改。性能——热路径新增仅一次 channel send（O(1) 无锁快路径）；消费者串行化该字段本就低频（节点边界/终态粒度），快照 PUT 本身秒级 HTTP，串行不构成瓶颈且换来的正是正确性；③ 会籍检查在快照路径**前置于** timeline 大文件读，禁止节点成本反而下降；④ 水合每会话挂载一次、只读内存锁；无新增网络调用、无缓存增长（designated 集与 retried 标记随 run 清理）。

**验证结果**：desktop `multica::` 232/232（新增 5：`completion_output_queue_executes_jobs_in_fifo_order`——慢快照（120ms 挂起）不重排后入队 /complete，固化评审 #2 根因；`begin_completion_for_run_claims_only_on_live_mapping`——含身份不匹配拒绝与映射清除后拒绝；`designated_output_nodes_lifecycle_scoped`——记录幂等/默认 false/终态清理/run 隔离；`upstream_pull_pending_rows_carry_parent_and_retried`——水合行字段与 run 前缀过滤；宽限等待 2 例——120ms 迟到注册命中、宽限耗尽返回 None 且不早退）；lib `upstream` 16 + `node_executor` 18 + `orchestrator` 145 全绿（trait 签名变更消费方）；前端 `multica-upstream-pull-card` 10/10（新增水合恢复 + retried 等待态、settled-水合竞态不复活、水合失败静默、Retry 转等待态不收起且 settled 收口、等待态 manual 提交收口、manual 返回回等待态）；`waitingNote` 7 语言同步；`tsc -p web/tsconfig.build.json` 零错。resolve/快照/认领的 HTTP 端到端依赖内网 server，按 bug-fix-verification §3 以纯函数与注入化执行器契约锁定，实机链路待内网打包复测。

### 12.82 改动八十：需求管理页 multica 平台快捷跳转按钮（M5-ck，2026-10-28）

**背景**：用户反馈在需求管理页无法直接进入 multica 平台（查看/创建工作空间、issue 详情需手动开浏览器敲地址），要求放一个快捷跳转按钮；仅来源为 multica 时展示（当前唯一来源即 multica，门控语义为未来多来源保留）。

**方案**：底部工具条右侧组（账号标识与手动刷新按钮之间）增加 `ExternalLink` ghost icon 按钮（Tooltip + aria-label 同文案，i18n `remote.taskManagement.openPlatform`「打开 {{source}} 平台」，`sourceLabel` 经来源注册表名片插值，不散写品牌名）；点击调既有 `openExternalUrl(settingsVm.multicaAppUrl)`——平台 Web 前端地址（后端登录流程同源字段）本页 `fetchSettings` 已加载，零新增请求；桌面端走 tauri opener 插件（与 Git 下载/GitHub PR 视图同源基础设施），浏览器态 window.open。门控两层：随 footer 既有 `source === 'multica' && connected`（非 multica 来源/未连接不渲染）+ `multicaAppUrl` 非空才渲染。跳转目标为平台根地址，不做 workspace/issue 深链（multica Web 路由结构无本仓可验证真源，不猜路径硬编码）。

**方案自评审**：过度设计——无（零新增状态/请求/组件文件，复用 settings VM、openExternalUrl 与 footer 既有门控，不新增 source 特判）。性能——无（纯静态按钮，数据已在手，无新增渲染路径与数据加载）。

**验证结果**：`remote-task-management-page` 28/28（新增 2：已连接点击按钮调 `openExternalUrl(appUrl)`、`multicaAppUrl` 为 null 不渲染）+ i18n 契约 4 套件 41 例全绿（7 语言 `openPlatform` 同步、占位符一致）；`tsc -p web/tsconfig.build.json` 零错。浏览器实测（vite dev + 临时 mock connected 态，验证后已还原）：footer 右侧组正确渲染按钮与 aria-label「打开 Multica 平台」；本机 agent-browser/CDP 输入派发不可靠（合成与原生点击均不生成 click 事件，仅 pointerdown 到达），点击→打开链路以组件测试固化，桌面端 opener 路径为既有已验证基础设施。

### 12.83 改动八十一：DPMS 需求回传对接——分支门、记忆播种与终态回写（M5-cl，2026-10-29）

**背景**：multica 回传对接（multica 侧已完成开发，对接文档 `.claude/design/dpms_cli/`）。`dpms_required` 任务在码灵执行中由 DPMS 角色创建系统需求后，需求 id 需回链到 multica issue。职责铁律不变：agent 会话只写任务记忆（`storyId` 等），HTTP 由 daemon 侧（码灵中介，自身 PAT）执行。链路三段：**分支门**（提示词侧事实注入）→ **记忆播种**（rerun 复用）→ **终态回写**（HTTP）。

**wire 契约**（claim / pending / detail 三面同构）：`dpms_required`（**非 omitempty bool，缺键 = false 灰度契约**）、`system_story_id`（`*int64` omitempty，issue 已链接的 DPMS story id）、`dpms_product_id`（`*int64` omitempty，溯源 story 行产品）。`RemoteTask` 三键 serde-optional，门条件一律 `== Some(true)`（与 `is_ready` 先例同构）。

**方案**：

- **① 分支门 = 任务级隐式区段块**（`src/prompts/{zh-CN,en}/runtime/remote_task_dpms_gate.md`，恒定文案无变量）：issue 关联且 `dpms_required == Some(true)` 时随首条 prompt 隐式区段注入（`dpms_gate_block`，位于完成协议块之后、附件失败清单之前）。**模式无关**——direct/workflow/auto 三种执行形态都无法按载荷跳转工作流节点（direct 无图、workflow 是用户选定的静态图、auto 动态生长），按载荷做 DSL 条件执行属过度设计；门只承载事实（本任务含创建环节）与写记忆契约（`storyId`/`storyName`/`dpms.storyUrl` 任务范围、已有 `storyId` 先核实复用、溯源值是推荐值仍需用户确认、不调工作项平台接口），创建行为本身由 DPMS 角色定义承载。留在隐式侧的原因与完成输出协议块相同（用户可编辑文本误删即静默断链）。workflow/auto 无任务级协议块，门块独立成立区段。
- **② 溯源预填新增两行**：`dpms_product_id` / `system_story_id` 进 `remote_task_context.md`（产品 ID（任务指定）/ 已创建 DPMS 需求 ID），用户可见推荐值；两键缺省不渲染（旧载荷行为不变）。
- **③ 记忆播种**（`seed_dpms_story_memory`）：fresh claim 时 `system_story_id` 非空 → 经 `MemoryService` upsert 任务记忆 `storyId`（键名常量 `DPMS_STORY_MEMORY_KEY`，与角色/CICD 记忆契约同源；读-改-写带 revision，同值 no-op）。与 `dpms_required` **独立**——回写成功后的 rerun 载荷 `dpms_required=false` 但仍播种（角色核实复用、CICD 节点读取）。续跑不播种（被续任务记忆随首次 claim 已在）。任何失败仅 warn 不阻断启动（缺失时角色按证据链回落询问用户）。
- **④ 终态回写**：`ActiveRemoteRun` 快照 `dpms_required: bool` + `claim_system_story_id: Option<i64>`（claim 时定版，续 `run_mode`/`issue_kind` 快照先例）。触发判定 `should_report_dpms_story`（纯函数）= 三条件与；取值 `dpms_story_to_report` = 守卫命中后读任务记忆 `storyId` 解析正整数（非数字/零/负 → None 不回写，服务端 `dpms_story_missing` 对账告警兜底）；执行 `report_dpms_story` = 单次 `POST /api/daemon/issues/{id}/dpms-story`（body `{"dpms_story_id":N}`，PAT + X-Workspace-ID）。挂点：complete 三处（direct 门控命中 / workflow·auto passthrough / turn 收尾门控，经 `complete_with_issue_done` 尾参）+ `handle_run_failed`（fail 路径——角色可能在失败前已创建）+ `cancel_remote_task`（手动取消同 failed 终态；teardown 前取值，本地收尾后 active_runs 即清）。失败仅 warn 不重试（网络瞬时错由 client 内一般重试兜底）；**409 → `IssueLinkConflict` 确定错误码不重试**——服务端同事务把 `dpms_required` 置 false，下次 claim 载荷条件自然失效，行为等价「永不再发」（不持久化 stop 标记）。
- **⑤ 修正记录（实现期自检）**：初版 `dpms_story_to_report` 直接用调用方 home context 的 App 读记忆——home App 的 `paths.project_id` ≠ 任务项目，`MemoryService` locator 校验拒绝、回写**静默永不触发**。改为函数内按 `run.local_project_id` → home StateConfig 反查工作区路径 → `app_for_workspace` 构造绑定该目录的 App（与播种/teardown 同一解析路径，单一事实源）。

**方案自评审**：过度设计——无新增持久状态（claim 快照沿 `ActiveRemoteRun` 内存生命周期；409「永不再发」用服务端翻转 + 行为条件替代持久 stop 标记；不建 per-issue 去重表——「各至多一次」由调用点结构性保证：complete 认领单飞（M5-cj `begin_completion_for_run`）、fail/cancel 是一次性路径）；门块为恒定文案模板，无新渲染机制。性能——回写取值全部在 `should_report` 守卫之后（非 DPMS 任务零成本）；DPMS 任务终态边界各一次小文件读（home state.json + 工作区 memory.json）+ 单次 POST，无轮询无缓存。

**验证结果**：desktop `multica::` 240 passed / 0 failed（基线 232 + 新增 8：client 三键解析（显式 true/false rerun 载荷、缺键全 None 灰度契约）+ 回写请求 body 恰 `{"dpms_story_id":N}` + socket 集成 200 路径/头/断言与 409→`IssueLinkConflict` 单请求不重试 + vm 门块（三模式注入、缺键/false/非 issue 不注入、en 同构）与溯源新行（中英行序、缺省不渲染）+ bridge 触发条件穷举（false/已链接/无 issue 不触发）与快照取值（正整数/空白容忍/非数字/零/负/键缺失 → None））；lib `prompts::` 19 passed / 0 failed（溯源两新键中英渲染与缺省不渲染、门模板创建契约锁定）。

**联调注意**（multica 侧交接）：multica 测试需 `DATABASE_URL`（开发库 `postgres://multica:multica@172.21.18.88:5432/multica`）否则静默跳过；既有失败基线 TestListIssues 3 个 + TestDaemon|TestStory|TestClaim 约 40 个（与本改动无关，排查时 stash 对比 HEAD）。

### 12.84 改动八十二：DPMS 回写载荷扩展 + 早回写时机前移（M5-cm，2026-10-30）

**背景**：内网真实 `dpms_required` 任务实测——DPMS 创建成功、multica issue 字段未更新（用户内网包早于 M5-cl 属已知 stale-binary 假回归，真实增量是契约缺口：旧 body 只回链 story id）。multica 6161b33f 把端点 D4 body 扩到 7 字段：`dpms_story_id`（必填正整数）+ 可选 `dev_user`/`test_user`/`release_plan_id`/`origin_url`/`product_id`/`product_name`；**提供即更新**（出现的字段覆盖、缺省逐列 COALESCE 保留现值）、同 id 重放 200 刷新字段并把 `dpms_required` 两半置回 true、**409 不落任何字段**（仍翻 false + `IssueLinkConflict` 不重试）、`product_name` 无 `product_id` → 400、产品映射复用转换规则。码灵侧对应两件事：从任务记忆组装全量 body；回写时机从终态前移到产出边界（「不再等 `/complete` 顺带捎」）。`completion_output` 回归交付文本职责（无代码改动）。

**方案**：

- **① wire body**（client.rs）：`ReportDpmsStoryRequest` 扩 7 字段（`Debug/Clone/PartialEq/Serialize`，可选字段 `skip_serializing_if`——缺省不发，仅 story-id 的 body 与旧契约逐字节等价，灰度安全）；`report_issue_dpms_story` 改收 body 引用；ClaimConflict→`IssueLinkConflict` 映射与「409 不重试」不变。
- **② 记忆组装**（bridge.rs 纯函数 `dpms_report_body_from_snapshot`）：8 键映射 `storyId→dpms_story_id`、`dpms.devUser→dev_user`、`dpms.testUser→test_user`、`dpms.releasePlanId→release_plan_id`、`dpms.storyUrl→origin_url`、`dpms.productId→product_id`、`dpms.productName→product_name`（键名常量进 state.rs 与 `DPMS_STORY_MEMORY_KEY` 同排）；`storyName` 记忆专用（rerun 复用核对）不进 wire。边界：文本 trim 非空、数值正整数（`storyId` 不可解析 → 整体 None 不回写，沿 M5-cl 口径）；**`product_name` 只在 `product_id` 存在时随行**——服务端 400 守卫前置规避，宁缺毋发。
- **③ 早回写时机**：`handle_node_completed` 加 outcome 参（订阅器既有载荷透传），节点成功边界触发（workflow/auto 的 DPMS 创建节点成功即回写，平台立即可见）；`handle_turn_completion_gate` 门控判定后、final probe 前触发（direct 的 turn 边界；无块对话继续也先回写再退出）。两钩子都与 pin/完成判定逻辑独立（worker-ref 未就绪/扣留完成不阻断回写）。`complete_with_issue_done` 卸下回写职责（删尾参）——故事链接事实独立于任务终态上报，改由各终态路径在 **finalize 之前**（active_runs 未清，去重与「已收尾不补发」判定成立）兜底调用：complete 路径（execute_complete_run match 后）×1、turn 收尾完成路径 ×1、`handle_run_failed`、`cancel_remote_task`（teardown 前）。
- **④ 统一入口 `report_dpms_story_if_changed`**：守卫（claim 快照三条件，不变）→ 组装 → 与在飞 run 的 `last_dpms_report` 比较 → 不同才 POST → 成功/409 记录 body。**去重与渐进刷新**：`ActiveRemoteRun.last_dpms_report: Option<ReportDpmsStoryRequest>`（仅内存）同 body 跳过、不同 body 再发——配套服务端「提供即更新」：溯源值经用户确认渐进写入记忆后，下一边界把确认值刷新进早先导入的行；409 同样记录（再发只会再 409）；**网络失败不记录**（下一节点/turn/终态边界自然重试，终态是最后机会）。并发容忍：比较与发送不在同一临界区（锁不覆盖网络调用），节点与 turn 钩子同拍双发由服务端幂等（同 id 重放 200）吸收，最坏一次冗余 POST；run 已被终态收尾清出 → 比较时即整体退出。
- **⑤ 门提示词**：`remote_task_dpms_gate.md`（zh/en）写记忆契约由 3 键扩为 8 键（含键名与语义），其余锚点（复用不重复创建/推荐值需确认/不调工作项平台接口）不动。
- **⑥ 修正记录（与角色定义联审发现）**：初版产品键沿用了 multica 交接单转述的 `dpms.product`，而 DPMS 角色定义（写入方，`.claude/design/dpms_cli/`，已在内网创建）实际写 `dpms.productId`——键名不一致会使 `product_id`/`product_name` 永远读不到、静默不回写。**键名以角色定义（写入方）为准**：码灵侧常量/门块/测试/文档统一改读 `dpms.productId`（M5-cm 未提交，零成本）；反向改角色需内网重存，弃。同轮联审补齐角色侧 `dpms.devUser` 缺口（角色记忆表原只有 `dpms.testUser`，`dev_user` 无写入来源），角色设计稿同步修订。

**方案自评审**：过度设计——`last_dpms_report` 复用 `ActiveRemoteRun` 内存生命周期（不建 per-issue 发送史持久表：渐进刷新只需「最近一次」，终态即清理；409「不再发」沿服务端翻转 + claim 条件失效的既有机制）；早回写钩子全部挂在既有事件路径上（NodeCompleted/AcpTurnFinished/终态五路），无新事件无轮询；比较/记录是锁内两次 HashMap 操作。性能——回写组装全部在 `should_report_dpms_story` 守卫之后（非 DPMS 任务零成本）；DPMS 任务每边界一次小文件读（home state.json + 工作区 memory.json，与 M5-cl 终态取值同量级）+ 至多一次 POST；早回写把 POST 从终态提前到产出边界，总发送次数受 body 去重约束（同 body 只发一次，典型 1–2 次），无新增高频路径。

**验证结果**：desktop `multica::` 240 passed / 0 failed（client：全量 body 序列化（7 键 JSON + 仅 story-id 灰度等价）、socket 集成断言路径/鉴权头/全 7 字段、409→`IssueLinkConflict` 单请求不重试；bridge：组装映射穷举（全键→wire 键、trim、`storyName` 不进 body）、`product_name` 无 `product_id` 整列丢弃、文本空白/数值非正 → 列缺省、`storyId` 非数字/零/负/缺失 → 整体 None）；lib `prompts::` 19 passed / 0 failed（门模板 8 键双语契约锁定）。早回写钩子为事件布线（AppHandle/socket harness 不可注入），时机与去重语义以调用点注释 + 纯函数契约锁定，内网真实任务全链路（创建→字段补全→渐进刷新→409）待打包联调验收。

**联调注意**：用户内网包若仍报「字段未更新」，先核实运行版本含 M5-cl+M5-cm（stale-binary 前科：M5-ca）；multica 侧 6161b33f 已合入其 5 项新测试。

**联调结论（2026-10-30/31 补记）**：首跑 issue `dpms_required=false` → 回写按灰度契约静默休眠（runtime.log 全程零 DPMS 痕迹；角色仍建 story 557993、写 8 记忆键——角色行为不经门控）；排查靠日志往返坐实「守卫未命中但不可见」，催生 M5-cn 可观测性。手动置 `dpms_required=true` 重跑 → 需求 id 成功回写 issue，M5-cl/cm 链路端到端验证通过。7 字段载荷随即被 M5-cn 契约收缩替代（multica 2026-10-31 定版：字段镜像由 multica 从 DPMS 拉取）。

---

### 12.85 改动八十三：DPMS 回写契约收缩为单字段关联 + 守卫可观测（M5-cn，2026-10-31）

**背景**：M5-cm 落地并内网验证后设计复盘——码灵不应搬运 DPMS 字段值：值的所有权在 DPMS，multica 作为消费方应自己拉新鲜值（搬运时刻 ≠ 查看时刻，DPMS 侧后续变更码灵永远不知道，回写注定陈旧；「渐进刷新/晚到确认重发」等复杂度全为此而生）。码灵只回传自己独有的事实：**本任务创建出的 story 是哪一个**（join key，只能来自创建方——DPMS 不认识 multica issue，反查必靠模糊匹配）。multica 2026-10-31 契约定版：`POST /api/daemon/issues/{id}/dpms-story` body 只带 `{"dpms_story_id": N}`，字段镜像全部由 multica 从 DPMS 拉取。另以内网排查实例（守卫静默 + claim 快照不落日志 → 需日志往返定位 `dpms_required=false`）为契机补可观测性。

**方案**：

- **① wire body 收缩**（client.rs）：`ReportDpmsStoryRequest` 收缩为 `dpms_story_id` 单字段（`Debug/Clone/PartialEq/Serialize` 保留）；`report_issue_dpms_story` 签名不变（body 引用）；409→`IssueLinkConflict` 不重试、网络重试均不变。
- **② 组装收缩**（bridge.rs）：`dpms_report_body_from_snapshot` 只读 `storyId`（trim + 正整数，口径不变）；删 state.rs 的 6 个载荷键常量（`dpms.devUser/testUser/releasePlanId/storyUrl/productId/productName`）与逐键映射——这些键退回**角色自身预填/留档语义**（角色定义继续写，runtime 不消费；内网已重存的角色无需改动）。
- **③ 时机/兜底/去重骨架不动**：早回写钩子（节点成功边界 + direct turn 边界）与终态五路兜底不变——关联尽早落地仍成立；`last_dpms_report` 语义收敛为「同 id 跳过、run 内换新 story 再发」（「渐进刷新」随载荷退役）。
- **④ 可观测性**：claim 两处登记点（fresh/续跑）落 INFO 快照（`dpms_required`/`system_story_id`/`issue_id` 定版值——「领取即定版」变成事实记录）；`report_dpms_story_if_changed` 守卫未命中从静默 return 改为 debug + 首个未满足原因（`dpms_report_skip_reason` 纯函数：三条件短路序）。
- **⑤ 门提示词**（`remote_task_dpms_gate.md` zh/en）：写记忆契约收缩为 `storyId` 单键（runtime 只回传该关联；`storyName`/`dpms.*` 按角色定义写入），复用不重复创建/推荐值需确认/不调工作项平台接口锚点不动。

**方案自评审**：过度设计——净删除为主（7 字段组装、6 常量、8 键清单、`product_name` 守卫），零新增抽象（`dpms_report_skip_reason` 是三条件短路的一行化）；保留骨架与拉取设计互补非冲突。性能——回写路径每边界少读 6 个记忆键，无新增 I/O；可观测性日志每任务 claim 一次 INFO + 守卫未命中 debug（DPMS 任务才有），量级可忽略。

**验证结果**：desktop `multica::` 241 passed / 0 failed（client：单字段 body 序列化恰为 `{"dpms_story_id":N}`、socket 断言 body 单字段 + 路径/鉴权头、409→`IssueLinkConflict` 单请求不重试；bridge：组装只读 `storyId`、`storyId` 非数字/零/负/缺失 → 整体 None、`dpms_report_skip_reason` 三条件短路序穷举）；lib `prompts::` 19 passed / 0 failed（门模板单键契约 + 拉取边界双语锁定）。内网全链路（创建→关联回传→multica 从 DPMS 拉字段）待打包联调验收。

**联调注意**：回写侧 multica 已定版单字段；`dpms_required` 标记机制如何覆盖非 story_sync 来源的 issue（首跑 false 的根因）仍待 multica 侧明确——验收用 issue 必须置 true。

---

### 12.86 改动八十四：需求管理看板 DPMS 待回传徽标 + 就绪正向化 + 待办就绪优先（M5-co，2026-10-31）

**背景**：M5-cn 收口后用户提出三项看板体验需求：① 需求管理界面标出哪些任务需要创建 DPMS 需求——红色「DPMS」徽标提醒，回传成功后自动消失（用户关心的是「该任务还有 DPMS 环节未完成」，非「任务是什么类型」）；② 就绪标记反转——开发任务已完成的 test 任务显示绿色「已就绪」徽标，取代 §12.42 的「未就绪」负向提醒（正向确认更显著、更符合用户直觉：挑能做的先做）；③ 待办列内已就绪任务优先靠前展示。

**方案**：

- **① 终态快照补 DPMS 回传状态**（config/mod.rs + bridge.rs）：`RemoteCompletedTask` 增 `dpms_required: bool` + `dpms_story_id: Option<i64>`（均 `#[serde(default)]`，旧历史缺省 → 不渲染徽标；开发阶段破坏式更新无迁移）；`completed_task_from_run` 从 `ActiveRemoteRun` 快照——`dpms_story_id` 取 `last_dpms_report` 载荷（成功与 409 已链接都算已回传，两种情况关联都已在服务端成立）。
- **② VM 三源合成单字段**（vm.rs）：`RemoteTaskVm.dpms_link_pending`（camelCase `dpmsLinkPending`）——pending/detail 行 = wire `dpms_required == true && system_story_id.is_none()`；active 行 = claim 快照「`dpms_required` + `claim_system_story_id` 空 + `last_dpms_report` 空」（与 bridge 回写守卫同源判定，回传成功后徽标即消）；completed 行 = 快照「`dpms_required && dpms_story_id.is_none()`」。前端只消费这一个 bool，三行来源差异不外露。
- **③ 看板双徽标**（RemoteTaskBoard.tsx）：`dpmsLinkPending` → 红色「DPMS」徽标（destructive 色系，与 bug 徽标同 token 风格；Tooltip 说明「需创建 DPMS 系统需求并回传关联，回传完成后自动消失」；**全状态列渲染**——提醒覆盖任务全生命周期，queued/running/终态行都可能待回传）。
- **④ 就绪正向化**：谓词 `isTaskNotReady` → `isTaskReady`（`test && readiness === true`）；queued 且就绪 → 绿色「已就绪」徽标（emerald 色系，与「已完成」列同族）+ Tooltip；未就绪 / 非 test / 缺失（旧 server）→ 无徽标（**无信号不标**——修正原「缺失按未就绪展示」的灰度兜底，反转后缺失自然回到无信号）。仍不阻断执行（入口本就全开）。本改动取代 §12.42「未就绪提醒」产品决策。
- **⑤ 待办就绪优先排序**：纯函数 `orderQueuedReadyFirst`（导出供单测）对 queued 列稳定排序就绪靠前，其余保持后端顺序（ES2019 稳定排序）；仅待办列，其余列不动。
- **⑥ i18n（7 语言）**：删 `readiness.notReady/notReadyHint`（破坏式，无兼容层），增 `readiness.ready/readyHint` + `dpms.badge/hint`；DPMS 为专名，`badge` 值 7 语言一致保留拉丁字样。

**方案自评审**：过度设计——零新增抽象：字段复刻 `kind` 徽标的既有链路（wire/claim 快照 → 终态历史 → VM → 徽标），VM 侧三源合成收敛为单 bool；排序是列内一次稳定 sort（列表规模 = active 任务数，个位到几十）。性能——`dpmsLinkPending` 随行返回零新增 I/O；旧历史 serde default 不触发迁移写盘。正向化删除负向文案属破坏式更新，符合开发阶段规约。

**验证结果**：desktop `multica::` **243 passed / 0 failed**（vm：`dpms_link_pending` 三源真值表——wire 缺键/预链接/false、active 回传后即消/claim 已链接、completed 已回传/旧历史；bridge：终态快照来源锁定——`last_dpms_report` 载荷进 `dpms_story_id`；config：DPMS 字段 roundtrip + 旧条目缺省；序列化 camelCase `dpmsLinkPending`）；web vitest 5 套件 **79 passed**（board：正向谓词真值表 + `orderQueuedReadyFirst` 排序不变量 + 红/绿徽标渲染与缺席 + 待办列渲染顺序；i18n：双语精确值 + 7 语言目录契约）。

**联调注意**：徽标视觉（红/绿双主题）与真实数据下的排序效果随 M5-cn 打包联调一并目视验收；本地 multica 不可达，单元/组件测试已覆盖渲染判定与排序不变量。

**修正记录（M5-cp，2026-10-31，内网实测终态行 DPMS 徽标常驻）**：① `dpms_story_id` 终态快照数据源缺陷——回写唯一写入 live 在飞条目（`report_dpms_story_if_changed` 记 `active_runs`），而终态路径持有的 `run` 是事件回调时克隆的旧副本，「同 turn 回写 + 完成」（direct 单轮完成的典型路径）与「终态兜底才回写成功」两窗口内快照恒 None → 终态历史把「已回传」记成「未回传」，徽标常驻（active 行不受影响——VM 直接读 live 映射）。② 修复：`finalize_terminal` 在清映射同一临界区先读 live 条目 `last_dpms_report`，经纯函数 `resolve_terminal_dpms_report` live 优先、快照回退（live 缺席 = 映射已被他方收尾清除的防御路径），覆盖进 `run` 克隆后构造终态历史；三条终态路径（complete/fail/turn-gate）均汇经 `finalize_terminal` 单点收口。③ 验证：最小失败测试（live 有值 + 快照 None → 解析必为 Some）修复前稳定红（读快照恒 None）、修复后绿；desktop `multica::` **244/244**。

### 12.87 改动八十五：multica 角色共享——daemon 工单被动上传/下载（M5-cq，2026-10-31）

**背景**：multica 侧落地「角色管理」页面（角色商店，kind=role，ADR-0008，server 零改动，接口全量就绪，见 `.claude/design/multica-role-management/设计方案与码灵对接说明.md`）。经方案收敛：**码灵只做被动的角色上传和下载**——上传 = multica 网页上传面板经 export 工单到本机提取；下载 = 网页「安装到我的码灵」经 push-install 工单推送落库；码灵唯一主动行为是既有 15s 心跳认领。**零桌面 UI、零通道 A**（multica market API 不接；版本/分类/可见性等发布决策全部由 multica 网页承载）。

**方案**：

- **lib 格式转换真值源**（`src/app/profiles.rs`）：`MulticaRoleExchange {name, description, content, is_builtin}`（wire 字段名即字段名，`is_builtin` 保持 snake）。**slug = 本地 profile.id**（canonical 身份；`pf-{base36}` 天然满足 multica slug 正则 `^[a-z0-9][a-z0-9._-]{0,63}$`；内置角色 id `pf-builtin-*` 恰与 multica 官方保留命名空间对齐）。`profile_to_multica_exchange`：frontmatter 重写为发布校验形态（`name`=slug〔硬校验规则 2〕、`description`=summary〔规则 3 必填〕），display name 无 wire 顶层字段可走（wire `name` 恒为 slug）→ `title` 键往返，id/createdAt/updatedAt/dynamicTemplate 原样保留。`multica_exchange_to_profile_input`：**严格** frontmatter 解析（契约规则 1 要求 content 必带 frontmatter，未闭合/缺失即失败→failed 上报）；display name 回退链 title→name→slug、summary 回退链 wire description→frontmatter summary/description→display name；fence 后单条框架换行归一（交换文档规范示例带空行，本地格式 body 紧贴 fence，保证往返字节一致）；重名 `-{n}` 改名拷贝（`resolve_unique_name`，与文件夹导入同策略）；dynamicTemplate 坏模板降级（`dynamic_template_body_renders` 与导入共用 helper，顺带消除导入处内联循环重复）。
- **client wire 类型 + API ×3**（`client.rs`）：心跳 ack 增 `pending_runtime_assets: Option<PendingRuntimeAssetsRef>`（id + 建议性 action，**权威 action 在详情**；Option 字段旧 server 缺省 None）；`RuntimeAssetsRequest` 只建模消费字段（action/asset_type/item_id/payload，id/slug/version 信息性字段不承载——serde 未知字段容忍）；payload 形态随工单变化（export=`{asset_key}`、push-install=角色正文）承载 `Value` 由处理侧解析；上报 body `RuntimeAssetsListReport` / `RuntimeAssetsInstallReport`（Option 字段显式 null 不加 skip_serializing_if，与 skill 上报同约定）；API：详情 GET + list 结果 POST + install 结果 POST（daemon PAT，`with_network_retry` 3 次内重试）。
- **工单处理模块**（`src-tauri/src/multica/runtime_assets.rs`，新）：纯函数 `classify_runtime_assets_request`（list / export{asset_key}〔item_id 空〕/ push-install{exchange}〔item_id 非空〕，结构性失败返回错误文案→failed 上报终态化）+ `asset_summaries`（key=profile.id 映射，`updated_at` 上报层转 RFC3339——multica 反馈 P4，本地存储格式是本地约定不动）+ 入口 `handle_runtime_assets_request`（详情 404/网络失败→放弃不上报〔server 内存态请求已消失，上报同样 404〕）+ 三 handler（list：`app.profiles()` 摘要枚举，非 role 资产类型 `supported:false`；export：`profile_show(asset_key)`→交换文档回传；push-install：转换→`create_profile` 落新用户角色→回报 `installed_name`=新 profile.id〔与 list/export key 同空间，网页可回查〕+ `installed_display_name`=实际落库展示名〔multica 反馈 P3；重名改名拷贝时是「xx-2」，仅市场名兜底拿不到〕）。本地读写走 spawn_blocking（与 skill 待办同模式）；上报失败记日志由服务端 60s running 超时兜底（multica 对迟到/重复上报幂等）。
- **分发接线**（`loop_.rs` + `mod.rs`）：`dispatch_pending_runtime_assets` 复刻 `dispatch_pending_skill_work` 模式（无待办零开销、有待办 spawn 独立任务、**绝不阻塞心跳 tick**；不需要 provider——上报 body 无该字段）；心跳 tick 内先于 ack move 取引用调用。`local_skills::home_app` 开放 `pub(crate)` 复用（home 作用域 App 获取）。
- **内置拷贝语义（ADR-0008 D6）**：push-install 落库恒为 `create_profile`（新 id 新用户角色）——内置角色 compile-in 只读天然不被覆盖、用户同名角色经改名拷贝不被覆盖；`is_builtin=true` / slug 撞 `pf-builtin-*` 推送均落用户副本，`installed_name` 回报实际 id。
- **明确不做**：WS `EventDaemonPendingWork` 唤醒（advisory；15s 心跳在 3min pending 窗口 ≥12 次认领机会，丢失无害）；通道 A market API；版本管理/分类/可见性（multica 网页承载）；wire 顶层 `dynamicTemplate` 字符串字段（其他 runtime 形态，码灵单一事实源在 content frontmatter）。

**方案自评审**：过度设计——零新增持久状态/aggregate（工单无状态消费，requestId 不落盘；三 handler 复用 lib 既有 `profiles()`/`profile_show()`/`create_profile()`，无新状态机）；复刻而非抽象 skill 待办分发骨架（两处并列尚不构成第三重复制点，抽公共层属过早抽象）。性能——心跳 tick 增量 = 一次 Option 判断；list 工单只传摘要（正文仅 export/install 全量传输，工单协议 §5.1）；本地角色目录十级文件扫描 <10ms 且在 spawn_blocking；上报独立任务不占 tick。

**验证结果**：lib `app::profiles` 新增转换真值表 7 例（发布形态 frontmatter 全键、往返还原 display/summary/body/dynamicTemplate、第三方文档多级回退 + 框架换行归一、重名 `-{n}` 拷贝、坏 dynamicTemplate 降级、严格解析拒绝未闭合 fence）；desktop `multica::` 新增（ack 解析 `pending_runtime_assets` + 旧 server 缺省容忍 + action 建议字段缺省、详情三工单形态解析、list/export/install 上报 body 契约〔含显式 null 语义与 install result 双字段〕、分类三路由 + 结构性拒绝、摘要 key=profile.id + is_builtin 透传、RFC3339 转换〔含解析失败原样返回〕）；回归 lib `app::` **366/366**、desktop `multica::` **251/251**。内网网页端到端（上传面板列角色→发布→安装回装）待打包联调验收。

**修正记录（multica 联调反馈，2026-11-01）**：
1. **P3 install result 可读性**：初版 result 只回 `installed_name`（= profile.id，`pf-mcx1a2` 形态），multica 网页用户看不出装了什么。修复：result 增补 `installed_display_name`（= 实际落库 `entry.name`）——重名改名拷贝场景下它是「xx-2」而非市场原名，multica 侧市场名兜底拿不到此值，必须 daemon 回报；`installed_name` 仍恒 profile.id，key 空间统一（list key = export asset_key = install installed_name）不动。
2. **P4 updated_at 非 RFC3339**：list 摘要 `updated_at` 原样外发本地存储格式 `2026-10-31 11:00:00`（空格分隔、无时区），multica 前端 `new Date(...)`/zod 解析失败。修复：上报层 `rfc3339_from_local_timestamp` 转换（chrono NaiveDateTime 解析 + 本地时区定位 + `to_rfc3339_opts`，秒级精度带 `±HH:MM` 偏移）；解析失败原样返回（展示降级好于错误时间）；本地存储格式是本地约定不动。export wire 顶层无时间字段，仅 list 受影响。

---

### 12.88 改动八十六：multica 工作流共享——daemon 工单被动上传/下载（M5-cr，2026-11-01）

**背景**：multica「共享」组新增「工作流管理」页面（`/[workspace]/workflows`，与角色管理共用 runtime-assets 通道，见 `.claude/design/multica-workflow-management/码灵对接说明(1).md`）。**server 零改动**——工作流复用 role 的「心跳认领 → 取详情 → 执行 → 回报」框架（12.87 / M5-cq），仅新增 `asset_type=workflow` 分支与工作流特有规则：modelBindings 永不共享（ADR-0008 D3 per-device 配置）、builtin id 推送落改名副本（D6）、§5.5 角色依赖首版不处理。码灵仍**纯被动、零桌面 UI、零通道 A**。

**方案**：

- **lib 格式转换真值源**（`src/app/workflow_share.rs`，新）：`MulticaWorkflowExchange {id, name, isBuiltIn, workflow}`——wire camelCase 四键（§5.1 envelope 定稿；与角色交换的 snake `is_builtin` 不同，两类 envelope 各自跟随 multica 侧定稿形状，不强求统一）。`workflow` 即 `WorkflowDsl` 原生 wire 形态（节点 `"type"` tag / 边 `$end`·`$new-round` 伪节点 / `on` kebab 值 / `executionSlotId` 显式 camel 与其余 Rust 原名混排——两端天然一致，无需自定义 DSL 映射）。**modelBindings 剥离由构造保证**：envelope 结构上没有该字段，序列化不可能带出（multica 发布校验 `MODEL_BINDINGS_PRESENT` 双保险的另一侧）；`optionalEntryStage`/时间戳不进 envelope（前者本地私有、后者无 wire 字段）。
- **lib 安装入口**（`App::install_workflow_template_from_multica`，`src/app/mod.rs`）：同 id（模板 id）已存在且非 builtin → 跳过返回现有（skip 策略，**skip 判定先于校验**——重复安装幂等，结构问题不二次翻旧账）；`reconcile_authoring_workflow_for_save` 迁移（执行槽指派/worker 绑定搬移进 bindings——注意该副作用会为带 provider 的 worker 生成绑定条目，属本地私有数据，不影响 wire）+ `validate_authoring_workflow` 结构校验（≈multica `INVALID_TEMPLATE`），**跳过 `resolve_workflow_profiles` / `validate_ai_dynamic_allowed_workflows`**（§5.5 角色引用缺失不阻断安装，缺失提示沿用客户端打开/运行时既有交互）；inner `workflow.id` 恒重生成（`next_workflow_id`，防外来 id 撞本地 DuplicateWorkflowId）；payload id 可用作模板 id 时**保留**（list key = export asset_key = 安装去重锚点，同一 key 空间），builtin id 碰撞/空 id → `unique_workflow_template_id` 派生副本 id；不触碰 `last_used_template_id`。
- **client wire 双形态 payload**（`client.rs`）：`RuntimeAssetPayload`（serde untagged enum Role/Workflow）承载 export/install 上报 body 的 `payload`——两类资产 envelope 形状不同，enum 是最小建模而非新抽象层；其余 wire 类型与 API **零新增**。
- **工单处理扩展**（`src-tauri/src/multica/runtime_assets.rs`）：`classify_runtime_assets_request` 按详情 `asset_type` 分派——空 = 旧版 role 容忍（role 通道早于该字段），`workflow` → `ExportWorkflow{asset_key}` / `PushInstallWorkflow{exchange}`，**未知类型 → Err（"unsupported runtime-assets asset type"）→ failed 上报**（spec §6 防御：web gate 只挡界面入口，CLI 直连用户 API 同样能触发 install 端点，不得误当 role 处理）。list 工单 `want_role`/`want_workflow` 双类型过滤（`asset_type` 空 = 合并上报逐条带 `type`；非空**只回该类型**——web 弹窗按传入类型渲染，混入另一类显示错乱；未知类型 list → completed + `supported:false` 降级而非错误）。`workflow_asset_summaries`：key = 模板 id、type="workflow"、description 恒空（本地模板无描述字段）、`updated_at` epoch `{secs}Z` → RFC3339 UTC（`rfc3339_from_epoch_timestamp`，解析失败原样返回——与 role 的本地时区格式不同，工作流模板时间戳本就是 epoch 形态）。export 按 asset_key 读模板 → `workflow_template_to_multica_exchange` 回传；push-install → `install_workflow_template_from_multica` → result `{installed_name=落库模板 id, installed_display_name=实际落库名, outcome=created|renamed-copy|reused-existing（M5-ct 安装回报细化，见 §12.90 ⑥）}`。
- **明确不做**：通道 A market API（同 M5-cq）；multica 侧安装入口 gate 放开（spec §8 multica 配套事项，码灵 ready 后由 multica 侧放开 `runtime-asset-detail-page.tsx` 两处 role gate）；§5.5 角色依赖一并安装提示（multica deps 恒空未实现）。

**方案自评审**：过度设计——零新增端点/持久状态/分发骨架（全量复用 M5-cq 心跳认领-详情-回报框架与 loop 分发器；安装复用 `save_workflow_template` 同一落库管线，无平行写入路径）；`RuntimeAssetPayload` enum 是两类 envelope 并存的最小建模。性能——心跳 tick 零增量（分发器不变）；list 只传摘要（正文仅 export/install 传输，单 artifact ≤8MB 且 serde 一次性序列化）；模板读写 spawn_blocking（与角色/skill 待办同模式）；本地工作流模板数与角色同级（个位~几十），全量读 store 是既有加载路径。

**验证结果**：lib `app::workflow_share` 3 例（envelope 序列化无 modelBindings/optionalEntryStage/时间戳、wire 往返 + 键名锁定 `success_condition`/`prompt_envelope`/`executionSlotId`、缺 workflow 正文拒绝→failed）；lib `app` install 2 例（skip 幂等、builtin 副本 id 派生、payload id 保留、inner id 重生成、缺失角色引用容忍）；desktop `multica::runtime_assets` 新增 3 例（workflow 工单分类路由、未知 asset_type 拒绝 + 空=role 容忍 + workflow envelope 误投 role 通道失败、摘要 key/epoch→RFC3339 映射）+ `multica::client` 上报 body 扩展（Role 包裹 + Workflow payload 形态断言）；回归 lib `app::` **371/371**、desktop `multica::` **254/254**；clippy 两 crate 对改动文件零新告警。内网网页端到端（spec §7 联调验收用例）待打包联调，且需 multica 侧放开 workflow 安装入口（§8）。

**修正记录（M5-cr 内网反馈，2026-11-01）**：内网联调「multica 安装显示成功，但码灵工作流列表看不到新模板」。排查：`workflows.json` 已正确落库（写读同源：daemon `home_app()` 与 UI 命令同为 `App::with_config(context.repo_root, context.config)`），根因是**被动通道缺本地反馈闭环**——前端工作流模板/角色列表只在会话壳就绪时拉取一次（App.tsx `conversationShellReady` effect），后台落库无事件触发重拉；且成功安装路径零日志（仅失败有 warn），排查只能翻 store 文件。属好设计实现不完善，修复补完被动通道的本地感知，不加新状态：
1. `runtime_assets.rs`：push-install 成功（role/workflow 两 handler）后 emit `gold-band://runtime-asset-installed`（载荷 `{assetType, installedDisplayName}` camelCase；常量模块自持，先例 `upstream.rs`）+ INFO 日志（request_id/asset_type/落库名，工单关联）；emit best-effort，失败不影响工单上报。
2. 前端六点接线（既有 subscribe* 门面模式：`app-events.ts` 常量 → `types.ts` VM → `client.ts` 接口 → `desktop.ts` 实现 → `browser.ts` 桩 → `api.ts` 门面），App.tsx 订阅后按 `assetType` **最小作用域重拉**（workflow → `getWorkflowTemplates`；role → `loadProfiles`；data-loading §6），异步 unlisten 竞态用 active flag 守卫（防泄漏，同 UPDATE_STATUS_EVENT 模式）；安装事件为用户步频单发，无风暴去重需求，不引 `useEventDrivenRefresh`。
3. lib `install_workflow_template_from_multica` 补 skip/新增两条 INFO 日志——skip（同 key 已存在跳过）是 spec §4.4 预期语义，排查时要能区分「装了新模板」vs「同 key 跳过」。
4. 验证：web 契约测试 `runtime-asset-installed-event.test.ts` 2 例（事件名 + 载荷形状）与 Rust `installed_event_serializes_camel_case_wire`（wire 键名）双侧锁跨端契约；tsc 改动文件零错。端到端（multica 网页安装 → 运行中码灵列表即时出现）待内网打包验收——本地无法产生 daemon 工单（依赖 multica server 下发），按 bug-fix-verification §3 例外以跨端契约测试 + INFO 日志为可审计证据。

### 12.89 改动八十七：需求管理页任务名搜索框（M5-cs，2026-11-01）

**背景**：需求管理页此前仅类型过滤（M5-ba 引入）。任务量增长后（pending 随 backlog 增长 + 终态本地历史上限 50 + running 个位数，单工作空间现实几十条、极端低百级），在四列看板中定位具体任务变难，需要按任务名检索。

**方案**：与类型过滤同层的纯客户端过滤，零新增请求/状态源/组件文件。① 纯函数 `filterTasksByTitle(tasks, query)`（`RemoteTaskManagementPage.tsx` 导出）：query `trim().toLowerCase()` 后为空 = 原样引用返回（同 `filterTasksByIssueKind` 的 `all` 约定），非空对 `title` 做大小写不敏感子串匹配（`RemoteTaskVm.title` 恒非空——pending 行标题缺失时后端已回退 remote_task_id，退化标题仍可搜）；② 投影链扩为 类型 ∩ 标题 ∩ 未移出（沿用既有 `selectedTasks` useMemo，看板仍收「选定工作空间的过滤后列表」不变）；③ footer 工具条类型过滤旁渲染搜索框（shadcn `Input` + Search 前缀图标，`h-8` 对齐 sm 控件，ContextManagementPage 同款模式），门控同类型过滤（`hasWorkspaces`）；瞬时 useState 不持久化（ui-interaction §7）；④ i18n `remote.taskManagement.searchPlaceholder` 7 语言同步（placeholder 兼作 aria-label）。

**方案自评审**：过度设计——无（一个纯函数 + 一个 state + 一个输入框，复用既有投影链、组件与 i18n 模式；不引 debounce/虚拟滚动/卡片 memo——数据上限明确 + 人频按键单次 O(n·m) 微秒级，加了属掩盖式复杂度）。性能——无（按键重渲染页面壳（轻量控件）+ 看板（投影消费者，属预期更新）；卡片按 `task.id` key 复用 DOM 不重挂；全程零网络请求；multica 无需服务端过滤——列表已全量在手，类型过滤时已确认同口径）。

**验证结果**：`remote-task-management-page` 33/33（新增 5：容器 1——CJK/拉丁大小写跨列过滤、与类型筛选组合交集、清空恢复、纯客户端零额外拉取；纯函数 4——空/空白查询恒等、大小写不敏感、trim、无匹配空集）+ i18n 契约（i18n-loading / locale-remote-parity）全绿；`tsc -p web/tsconfig.build.json` 零错。浏览器实测（vite dev + browser 桩临时种 connected 样例，验证后已还原；deep link `/chat/remote-tasks`）：『支付』跨列命中待办 + 进行中、大写 `PAYMENT` 命中 test 任务、类型=开发 ∩ PAYMENT 全空、清空恢复 dev 全量；浅色截图 + 深色计算样式（body rgb(15,15,15)、输入框深底浅字，全部语义 token）双主题确认。

### 12.90 改动八十八：feature_multica 分支评审修复九项（M5-ct，2026-11-01）

**背景**：feature_multica 合并 new-origin/main 后做全分支评审（10 项发现，合并本身机械干净），按优先级依次修复九项。九项均为「好设计、实现欠完善」的补全（AGENTS.md 问题解决思路 §2），无设计层改动、无服务端协议破坏（⑥ 为纯增量 wire 字段）。

**方案**：

① **冻结段协议判据结构化**（`src/app/upstream.rs`）：原 `FROZEN_PROTOCOL_SNIPPET = "completion-output"` 裸子串嗅探会误伤——附件失败清单与失败原因原文同处任务级冻结区段，子串命中不代表协议已冻结，误命中即跳过注入会让节点拿不到按节点变体的产出协议。协议正文在冻结区段块序中恒为**首块**（`vm::remote_task_hidden_section`：完成协议（仅 direct）→ DPMS 门 → 附件失败清单），判据改为 `frozen_carries_completion_protocol` 纯函数：区段内容以协议模板文档开头（`completion_protocol_document_openers` 与模板同源渲染 zh/en 首行，`LazyLock` 计算一次）——位置锚定，子串不再参与比对。M5-cg 前旧任务协议同样在首块，同一判据仍正确识别。

② **auto 面上游拉取令牌补齐**（`src/app/orchestrator.rs`）：`$upstream.completion-output` 解析只接了 workflow 面（`node_executor`），auto 动态叶子 `build_dynamic_worker_invocation` 装配时令牌原样留在 goal/角色正文——模型看到未解析引用。auto 面接同一装配：`needs_upstream_resolution` 先行检查（无令牌零成本）→ 命中经 App resolver 解析 → `upstream_hidden_section` 追加隐式区段 + `strip_upstream_references` 移除令牌（与 workflow 面同口径）；continue 续跑只 strip 不再解析（单次快照语义——值取节点启动时）。

③ **终态 `/complete` 会籍门禁**（`multica/bridge.rs`）：M5-cj 上传侧门禁只覆盖中段快照，终态 Passthrough 交付是另一条路径——Prohibited 节点的终态块照带上送可覆写 issue 交付，M5-cg/M5-cj 资格集对 auto/direct 形同虚设。`CompletionOutputJob::CompleteRun` 携带 `RunCompleted` 事件已带的终态 `node_id`（身份在手，无需反查），Passthrough 分支经 `gate_terminal_completion_output` 纯函数与中段快照同判据（`is_designated_output_node`）：issue 已绑定 + 终态节点无会籍 → `completion_output` 剥离不上送（issue 现值保留），INFO 日志可审计。

④ **rerun 退场两段式**（`multica/state.rs` + `commands.rs` + `loop_.rs` + `bridge.rs`）：他端发起的 rerun，本地源终态行此前只能靠 `get_remote_tasks` 视图过滤兜底——永不真删除，终态历史上限 50 被幽灵行占据。读命令不得执行维护写（data-loading 规则 §5），改两段式：读路径把 pending 行 `rerun_of_task_id` 记入共享内存态 `observed_rerun_sources`（有界：单批 ≤ 一次拉取行数，排空即清；进程重启丢失无害——丢失窗口内退化为仅视图过滤，即本机制引入前行为）；心跳循环 ②.5 阶段 `drain_observed_reran_retirements` 排空——命中本地终态历史即 `remove_completed_task_entry` 真删除 + emit，失败下一 tick 重试。残余边界（本机全程离线错过整个排队窗口时源行保留）在代码注释诚实标注。

⑤ **列表行标题解码统一**（`multica/vm.rs`）：终态/running/pending 三处列表行标题拼装各自为政——终态历史行直接 `c.title.clone()`（无 trim/回退），且服务端 `thread_name` 是 markdown 源，看板直接显示原始标记。统一 `display_title`（`markdown_source_to_plain_text` 解码 + trim + 空白/缺失回退 remote_task_id），与 composer 预填首行 `title_block` 同一渲染口径（口径早已在 composer 侧定义，列表侧漏接）。

⑥ **被动安装回报 outcome**（`app/profiles.rs` + `app/mod.rs` + `multica/client.rs` + `runtime_assets.rs`）：安装 result 此前只有 `installed_name`/`installed_display_name`，调用方无法区分「新建 / 改名副本（D6）/ 同 key 复用」——冲突静默不可观测。`MulticaInstallOutcome { Created, RenamedCopy, ReusedExisting }`（`as_wire_str` → "created"/"renamed-copy"/"reused-existing"），重名判定与改名同点 `resolve_unique_name` 单一真源（resolved ≠ display 即 RenamedCopy；workflow 同 key 用户模板 → ReusedExisting）；wire `RuntimeAssetsInstallResult` 增 `outcome` 字段。存量 server 未知字段忽略、向后兼容；服务端如需消费该字段细化安装提示，列入内网需求清单（本仓库不改内网在运行的 multica 服务端）。

⑦ **App.tsx 事件重拉吞错策略**（`web/src/App.tsx`）：runtime-asset-installed 事件重拉 `loadProfiles().catch(() => setProfiles([]))`——重拉失败会把**已加载** profiles 清空，把「后台刷新失败」放大成「数据丢失」，违背 data-loading §3/§6 best-effort 口径。改 `catch(() => {})` 保留已加载数据。

⑧ **上游拉取决策非法载荷独立错误码**（`multica/error.rs` + `commands.rs` + locale 7 语言）：`decide_upstream_pull` 校验失败（manual 空文本 / 未知决策字面量）原折叠为 `UpstreamPullNotPending`——前端 `isDecisionNotPending` 对 not-pending 幂等收卡，会把「槽仍在等待」的卡片无声丢弃（用户失去再次介入入口）。新 `UpstreamPullInvalidDecision`（code `multica.upstream-pull-invalid-decision`）：槽未写入、等待方仍在，前端走通用错误分支（保留卡片 + 展示错误）让用户重选；字面量细节进后端日志。与 not-pending（槽不存在，幂等收卡）语义严格二分。

⑨ **锁中毒策略统一**（`multica/state.rs` 及 6 个消费文件）：`shared_state().expect(...)` 生产 10 处——锁中毒 = panic 穿透事件回调拖垮宿主，与「状态不可用」的严重度不匹配。`MulticaRuntimeState::lock_degraded`（warn + None）统一收编：读路径跳过本 tick、命令路径 `RuntimeOffline`（替代 2 处 `NetworkFailed` 折叠——语义是状态不可达而非网络失败）、`register_upstream_pull` 降级为未登记 Notify、`upstream.rs` 1 处静默 `.ok()?` 一并替换；热路径周期调用维持 `if let Ok` 静默（防 warn 刷屏）。

**方案自评审**：过度设计——九项全部复用既有机制（③ 复用 RunCompleted 已带 node_id 与 `is_designated_output_node`，无新状态/反查链；④ 复用 `remove_completed_task_entry`/`with_state` dirty 语义/既有 emit，内存备忘有界自清；⑥ 重名判定与改名同点，单一真源；⑧ 仅拆分既有错误码语义，无新错误通道）；显式拒绝的替代：④ 心跳加 list_pending 轮询（每 workspace 永久新增 GET 流量）与 ack 携带 lineage（需服务端改动）——读路径观察 + 心跳排空零新增网络；③ 不引入终态节点登记表（身份已在事件中）。性能——① `LazyLock` 一次计算 + `starts_with`（原为 `contains`，更廉）；② 无令牌节点先行零成本检查；③④ 心跳新增阶段为锁内内存操作（④ 仅命中才走 `with_state` 落盘，无命中零 IO）；⑨ 消除 panic 穿透事件循环的风险而非新增开销；⑤⑥⑦⑧ 为纯函数/文案/wire 增量字段，无热路径变化。

**验证结果**：desktop `multica::` 259 通过（新增 `apply_observed_rerun_retirements_removes_only_observed_local_rows`、`gate_terminal_completion_output_strips_block_from_undesignated_node`、`lock_degraded_returns_none_on_poison_instead_of_panicking`、error codes 契约扩展、wire `outcome` 断言）；lib `multica` 13 + `profiles` 42（conflict/复用测试扩 outcome 断言）+ workflow install 2（Created/ReusedExisting 断言）+ `orchestrator` 动态令牌 2（注入追加隐式区段并 strip、continue 只 strip 不再解析）+ `upstream` 冻结判据改写全绿；web i18n 契约 7 通过（invalid-decision 7 语言）；tsc / clippy 触碰文件零告警。③④⑥ 的端到端链路依赖 multica server / 真实工作流，待内网打包验收（本地以单测 + wire 契约锁定，bug-fix-verification §3 例外）。

### 12.91 改动八十九：DPMS 回写载荷补产品 ID 作为 join key（M5-cu，2026-11-02）

**背景**：multica 侧提出回写 body 从 `{"dpms_story_id": N}` 扩展为 `{"dpms_story_id": N, "dpms_product_id": M}`——创建需求时产品 ID 本就已知（不选定产品无法创建），码灵补一个字段即可。

**设计判断（不是 M5-cn 的回退）**：M5-cn 收缩的依据是「值所有权在 DPMS，搬运注定陈旧」；产品 ID 不属字段镜像，原因是 **DPMS 查询按产品隔离**（跨产品 `story detail --id` 返回 `status: -5`）。multica 拿到 story id 后要自行拉取字段，必须知道该需求所在产品——story id 单独不可全局寻址，**(产品 ID, 需求 ID) 才是完整 join key**。该事实只有创建方持有：claim 载荷的 `dpms_product_id` 是 issue 行溯源推荐值，用户确认后角色可能切产品创建，不能替代实际值；DPMS 不认识 multica issue，multica 无法反查。故产品 ID 与 story ID 同属「本任务创建出的哪个需求」这一关联事实，与 M5-cn 原则一致。

**实现**：

- ① **wire**（`client.rs`）：`ReportDpmsStoryRequest` 增 `dpms_product_id: Option<i64>`（`skip_serializing_if = "Option::is_none"`）。带产品时 body = `{"dpms_story_id":N,"dpms_product_id":M}`；缺省时与 M5-cn 单字段契约**逐字节等价**（旧角色未写记忆键的灰度兼容；旧 server 未知字段忽略）。
- ② **组装**（`bridge.rs` `dpms_report_body_from_snapshot`）：同口径读取任务记忆 `dpms.productId`（trim + 正整数），**可选**——缺失/非法 → 只回传 story id，不阻断关联；`state.rs` 新增键常量 `DPMS_PRODUCT_MEMORY_KEY`，与 `DPMS_STORY_MEMORY_KEY` 同排（键名以角色定义为写入方）。
- ③ **去重骨架不动**：`last_dpms_report` 比较整个 body——若产品 ID 晚到补齐，下一边界自然重发一次，不新增状态。
- ④ **门提示词**（`remote_task_dpms_gate.md` zh/en）：运行时契约由 `storyId` 单键扩为两键；明确 `dpms.productId` 必须写确认后**实际使用**的数字产品 ID（不能照抄推荐值），其余锚点不动。
- ⑤ **角色定义**（`.claude/design/dpms_cli/`）：`dpms.productId` 记忆契约显式要求数字 ID（runtime 消费键，写名称会静默缺失）。

**方案自评审**：过度设计——单 Option 字段 + 一个键常量，去重复用既有 body 比较，无新状态/队列/缓存。性能——同一次记忆快照多一次线性 key 查找（任务记忆键个位数），body 多一个小整数，无新增 I/O、请求或热路径变化。

**验证结果**：desktop `multica::` **260 passed / 0 failed**（client：缺省形态逐字节等价 + 带产品两字段精确 body + socket 断言更新 + 409 语义不变；bridge：storyId+productId 双解析、产品键缺失/非法省略不阻断、storyId 非法整体不回写；vm/bridge 既有 body 字面量同步）；lib `prompts::` **19 passed / 0 failed**（门模板双语两键契约 + `dpms.productId` 实际值锚点）。改动文件 clippy 零新增告警。**待 multica 侧部署接收 `dpms_product_id`**；端到端（创建→关联回传→multica 按产品拉字段）随打包联调验收。

### 12.92 改动九十：wb 渠道 multica 地址解耦为独立构建期注入（M5-cv，2026-11-02）

**背景**：multica 对外入口从「码灵 WB 服务域名 + `:5005`」整体替换为独立子域（与 WB 服务域不同、无端口），客户端编译期默认地址须同步。

**根因（构建期耦合，非地址值过时）**：§12.30 把 multica 地址建模为「WB 服务 origin + `multicaPort`」派生，模板只能用 `${GOLD_BAND_WB_BASE_URL}` 换 origin 再拼端口，只能表达同 origin 换端口。新部署是两个独立 origin，旧模板无法表达；若改为硬编码新域名，又违反「仓库不落真实服务域名」（`check-wb-domain` 强制）。故把 multica 地址升级为独立构建期事实源，而非改常量或加运行期旁路。

**实现**：

- ① `configs/channels/wb.json`：`multicaBaseUrl`/`multicaAppUrl` 改 `${GOLD_BAND_WB_MULTICA_URL}`，删除 `multicaPort` 与端口派生。
- ② `scripts/channel-config.mjs`：`readOrigin(env, name)` 分别校验两个 HTTP(S) origin（拒绝凭据/路径/查询/片段），`resolveWbConfig` 递归替换两个令牌；错误只回显变量名，不回显值。
- ③ `src-tauri/build_channel_config.rs`：`WbUrls::new(base, multica)` 双 origin 校验，`resolve()` 各自替换对应令牌（multica 地址不会被误用于基础地址）；`wb_urls()` 分别读取两个变量；`build.rs` 补 `rerun-if-env-changed=GOLD_BAND_WB_MULTICA_URL`。
- ④ 两条 release workflow（`release.yml`/`release-please.yml`）的 preflight、build、updater-json 三处注入 Secret `GOLD_BAND_WB_MULTICA_URL`。
- ⑤ `scripts/check-wb-domain.mjs` 同时扫描两个域名，防止新域名被硬编码进仓库。
- 运行期解析链（`desktop_multica_base_url` 覆盖 → 渠道兜底）与地址变更作废语义（`save_multica_connection_address` 改动地址清 PAT/绑定）不变；default 渠道保持 localhost。

**部署前置（非本仓库代码）**：GitHub Actions 新增 Secret `GOLD_BAND_WB_MULTICA_URL`；内网 multica 侧须同步 nginx `server_name`/端口、桌面端 `apiUrl`、`MULTICA_APP_URL`/`FRONTEND_ORIGIN`/`CORS_ALLOWED_ORIGINS`/OAuth 回调与 CLI 默认 self-host 地址（本地 multica fork 仅参考源码，不作为内网契约）。过渡期旧入口需保持可解析，供已安装客户端升级使用；用户若曾保存运行期连接地址覆盖，需在设置中改为新地址（地址变更会清 PAT 并要求重新登录）。

**方案自评审**：过度设计——仅新增一个构建期变量与令牌并删除 `multicaPort` 派生，无新状态、缓存、队列或运行期分支。性能——构建期字符串替换，运行期零变化。

**验证结果**：`node --test`（channel-config / wb-base-url / check-wb-domain / prepare-release-build / release-please-config / generate-release-assets / dev-channel / dev-static-channel / build-channel-options）**70 passed / 0 failed**（新增 multica origin 独立解析、非法 origin 拒绝、源码令牌断言、release workflow Secret 断言）；`GOLD_BAND_RELEASE_CHANNEL=wb` + 双变量 `cargo check -p gold-band-desktop` 走通 build.rs 实际解析；default 渠道 check 无回归。仅构建期配置，无运行时逻辑、算法或 I/O 变化。

**文件**：`configs/channels/wb.json`、`scripts/channel-config.mjs`(+test)、`scripts/wb-base-url.test.mjs`、`scripts/check-wb-domain.mjs`、`scripts/prepare-release-build.test.mjs`、`scripts/release-please-config.test.mjs`、`scripts/generate-release-assets.test.mjs`、`src-tauri/build_channel_config.rs`、`src-tauri/build.rs`、`.github/workflows/{release,release-please}.yml`；文档 `interaction/app/overview.md`、`接入方案.md`、本文件 §2.2.4/§12.30/§12.92。

### 12.93 改动九十一：multica 卸载工单——daemon 被动移除本地角色/工作流（M5-cw，2026-11-11）

**背景**：multica 侧上线卸载通道（「角色管理 → Runtime 页签」对某资产点「卸载」，server commit f82a4fe7）——完全复用 runtime-assets pending 通道（心跳认领 → 详情 → 回报，list/install 同款生命周期），上报共用 install result 端点（路径段 install 为历史命名，三动作 export/install/uninstall 共用，不新建 /uninstall/.../result）。码灵侧只需在工单分发处新增 uninstall 分支。

**实现**：

- ① **wire**（`client.rs`）：详情 `RuntimeAssetsRequest.action` 增 `uninstall`（ack 的 action 只是建议，分发器本就以详情为权威，`loop_.rs` 零改动）；`RuntimeAssetsInstallReport.result` 由 `Option<RuntimeAssetsInstallResult>` 改为 untagged enum `RuntimeAssetsReportResult::{Install, Uninstall}`（multica 对 result 仅透传 UI 不解析字段——自由 map；untagged 序列化即内层对象，wire 兼容且保留类型安全）；新增 `RuntimeAssetsUninstallResult { uninstalled_key }`（snake_case 随 `installed_name` 先例）。
- ② **分类**（`runtime_assets.rs`）：`RuntimeAssetsWorkOrder` 增 `Uninstall { asset_key }` / `UninstallWorkflow { asset_key }`；classify 增 `"uninstall"` 分支——payload 与 export 同形 `{asset_key}`（要卸载的资产 key，与 list 清单上报的 key/type 同源），asset_type 空 = 旧版 role 容忍、未知 → failed 上报（与 install 同防御：CLI 直连可触发，不得误当 role 处理）。
- ③ **卸载决策纯函数** `plan_uninstall(key, assets)`（`(id, is_built_in)` 投影，role/workflow 两类本地存储同构，单一真源）：**不存在 → 幂等成功**（multica 推荐语义：重复卸载/竞态删除不误报失败）、**内置 → 只读 failed**（与本地 UI 删除同约束）、**用户资产 → 删除**。删除执行：role 走 `App::delete_profile(key, force=true)`——multica 确认弹窗已承担本地「引用计数二次确认」职责（force=false 会在角色被模板/任务引用时报 DeleteConfirmationRequired，远程无法交互确认，卸载会永远失败）；workflow 走 `App::delete_workflow_template(key)`（自带 builtin 只读与 `last_used_template_id` 重置）。决策与删除同在 `spawn_blocking` 内（阻塞 FS 扫描，与 list/export 同模式）。
- ④ **语义边界**：只删本地资产，不触碰 multica 共享目录已发布内容（multica UI 文案承诺）；3 分钟回报窗口内本地删除为快操作；终态重复上报由 multica 幂等忽略（`{"status":"ok"}`）。
- ⑤ **本地反馈闭环**（M5-cr 对称）：真删除后 emit `gold-band://runtime-asset-uninstalled`（载荷 `{assetType, uninstalledKey}` camelCase）+ INFO 日志；前端 `App.tsx` 按 assetType 只重拉对应列表（workflow → 模板、role → 角色，data-loading §6 最小作用域，best-effort 吞错）。**幂等成功不 emit**（本地无变化，列表本就不含该资产）。前端五处对称扩展：`app-events.ts` 常量、`types.ts` VM、`client.ts`/`desktop.ts`/`browser.ts`（桩）/`api.ts` 门面。

**方案自评审**：过度设计——零新增端点/持久状态/分发骨架（既有 match arm 扩展），`RuntimeAssetsReportResult` enum 是 result 字段双形态的最小建模，决策纯函数是语义固化的测试锚点而非新抽象层。性能——心跳 tick 零增量；删除为本地单文件（role）/单 store 写（workflow），spawn_blocking 同既有模式；上报 body 仅一个 key。兼容——旧 server 不下发 uninstall（纯新增分支，list/install 路径逐字节不变）；旧 daemon 收到 uninstall 走「unsupported action」failed 上报（multica UI 可见失败原因，符合预期）。

**验证结果**：desktop `multica::` **263 passed / 0 failed**（新增 3 例：classify uninstall 路由〔含未知 asset_type 拒绝、空 type 容忍、payload 缺失/空 key 拒绝〕、`plan_uninstall` 三分支决策、卸载事件 camelCase wire；扩展 2 例：uninstall result body 契约〔untagged 内层对象 + 仅 uninstalled_key〕、详情 uninstall 形态解析）；web `runtime-asset-uninstalled-event.test.ts` 契约锁定；`tsc -p tsconfig.build.json` 零错误；改动文件 clippy 零新增告警。内网端到端（multica Runtime 页签卸载 → toast 成功 → 清单刷新且条目消失 → 本地文件移除 → 共享目录副本不受影响；curl 直打不存在 key 的失败路径）待打包联调验收。

**文件**：`src-tauri/src/multica/client.rs`、`src-tauri/src/multica/runtime_assets.rs`、`web/src/types.ts`、`web/src/lib/app-events.ts`、`web/src/api/client.ts`、`web/src/api/desktop.ts`、`web/src/api/browser.ts`、`web/src/api.ts`、`web/src/App.tsx`、`web/tests/runtime-asset-uninstalled-event.test.ts`；文档 `接入方案.md` D5/M5-cw、本条。

---

### 12.94 改动九十二：心跳优化——间隔 15s→3s + 工单完成后补发排空（M5-cx，2026-11-11）

**背景**：multica 侧心跳优化改动说明（对齐其 daemon `handlePendingWorkHint` 模式）：runtime-assets/待办均为拉取式通道（一次心跳最多认领一个工单），码灵无 WS/企微中继，网页端操作需等周期心跳才被认领——15s 间隔下感知等待 avg ~7.5s / max 15s。协议字段零变化，纯 daemon 行为：改动 1（必做）间隔 15s→3s；改动 2（建议做）认领请求处理完成（result 上报完成）后不等下一个 tick 立即补发心跳认领下一个，直到 ack 无 `pending_runtime_assets`（1s 最小间隔防打满）。验收口径：单操作等待 0~3s（avg ~1.5s），连续排队操作几乎无额外等待。

**实现**：

- ① **改动 1**（`loop_.rs`）：`MULTICA_HEARTBEAT_INTERVAL_SECS` 15→3。tick 内其余维护随同一节奏加速，逐一评估后**保持单循环不拆双节奏**：自愈注册正常态零 server 调用（仅对缺 runtime_id 的 workspace 动作）；rerun 排空空集零开销（锁内判空即返回）；取消检测在有任务在飞时为每 run 一次轻量 GET（×5 后约 20 req/min/run，相对执行期快照/DPMS 流量可忽略）。overrun warn 阈值 30s 不变（只报警不强停）。
- ② **改动 2 之真源抽取**（`loop_.rs`）：把 tick 循环体内「单 runtime 心跳 + ack 全字段消费」抽为 `pub(crate) async fn heartbeat_once(app, client, workspace_id, runtime_id)`——readiness diff 事件、runtime-assets 工单分发（spawn）、skill 待办分发（spawn）、`TaskNotFound` 404 清缓存触发自愈重注册，全部原样搬移；定时 tick 与补发共用同一真源，**任何 ack 信号都不因补发路径丢失**（补发心跳若带回 skill 待办/readiness diff 也被完整消费）。
- ③ **改动 2 之排空链**（`runtime_assets.rs`）：`dispatch_pending_runtime_assets` 透传 `workspace_id`（心跳按 (workspace_id, runtime_id) 定位，handler 原只有 runtime_id）；`handle_runtime_assets_request` 各分支 result 上报完成后，链内节流 `DRAIN_REHEARTBEAT_THROTTLE_SECS = 1` 秒调 `loop_::heartbeat_once` 补发——ack 又有 pending 则 dispatch spawn 新处理任务，其完成后再补发，**排空链由既有 spawn 模型自然涌现**（无显式循环/无新状态/无 Notify）；ack 无 pending 时 dispatch 不 spawn，链条自然终止（终止性：每环认领一个工单，server 队列有限）。节流**仅链内**：与定时 tick 或并行排空链之间不做全局协调——心跳幂等、server 认领原子、补发总量受队列长度有界，严格全局间隔需跨任务共享状态，收益不成比例（multica handlePendingWorkHint 的限流器守护的是其 WS 事件高频触发路径，与本通道触发频率不可比）。详情拉取失败的提前 return 不补发：工单在心跳认领时已 pending→running（对接说明 §4.1），放弃不上报由 server 60s running 超时终态化兜底、**不会重派**，补发只会认领下一个工单、跳过本单无益；补发心跳失败仅 warn，3s 定时 tick 兜底；与定时 tick 并发心跳无害（server 认领原子，最坏各认领一个）。
- ④ **模块可见性**：`runtime_assets` → `loop_::heartbeat_once` 同 crate 兄弟模块引用（`pub mod loop_` 既有可见性，无循环依赖问题——loop_ 本就引用 runtime_assets，反向仅此一处且为心跳单一真源服务）。

**方案自评审**：过度设计——无新状态/队列/Notify/WS/配置项，复用心跳函数做 ack 消费单一真源，排空链零显式编排结构；1s 节流是 multica spec 要求。性能——每 daemon 每 3s 一次心跳负载 multica 侧已评估可忽略；补发链每环一次短超时 HTTP（liveness 10s）+ 1s sleep，总量有界（pending 队列长度）；无全量扫描/无界队列/长持锁引入。正确性——排空链各环 spawn 独立任务，与 tick 并发心跳依赖 server 原子认领（既有语义），无双重处理风险。

**验证结果**：desktop `multica::` **265 passed / 0 failed**（新增 2 例契约断言：心跳间隔 3s、排空节流 1s——与 multica 侧对齐值，漂移先红；既有 263 例含 readiness 谓词/工单路由全绿，证 heartbeat_once 为行为等价搬移）；`cargo check -p gold-band-desktop` 通过（仅既有无关 dead-code 警告）。内网感知延迟验收（网页操作 → 本地生效计时：单操作 ≤3s、连续操作近似无缝）待打包联调。

**文件**：`src-tauri/src/multica/loop_.rs`、`src-tauri/src/multica/runtime_assets.rs`、`src-tauri/src/multica/mod.rs`、`src-tauri/src/multica/client.rs`（doc 同步）；文档 `接入方案.md` §2.1/§3.2.4/C1/D5/时序图/M5-cx、本条。

### 12.95 改动九十三：执行记录（transcript）实时上报——messages + usage，progress 砍掉（M5-cy，2026-11-12）

**背景**：multica 侧需求——远程任务执行时把模型文本、工具调用/结果、错误实时上报，使其**既有** Web UI 获得执行时间线、transcript 弹窗、工具步骤行、产物列表与 diff、WS 实时更新，**服务端与前端零改动**。multica 侧逐条核对服务端代码后给出权威契约，据此收敛设计：① 消息 `type` 无白名单（前端仅五种有专门渲染，其余 generic 兜底）→ 只用 text/thinking/tool_use/tool_result/error；② 空数组 200 no-op、响应 `{"status":"ok"}`、错误 400/500/404；③ 服务端无条数/大小限制，但**只清洗 NUL 且整批有一条失败即整批 500 丢弃** → 本端先清洗并自律上限；④ seq 严格递增、允许空洞、**重复 seq 产生重复行**（表上无唯一约束）→ 绝不重发同 seq、批次失败即丢不重试；⑤ **每次 retry/rerun 服务端都产生新 task_id → seq 作用域 = task_id，从 1 开始**，故原计划的「seq 水位持久化文件」**取消**（无跨进程场景）；⑥ daemon task 接口只用 PAT、不带 `X-Workspace-ID`；⑦ 自检接口 `GET /api/daemon/tasks/{id}/messages?since=N`；⑧ usage 七字段、`cost_usd_ticks=0` → NULL → 服务端按费率估算、按 `(task_id, provider, model)` **整体替换**（必须报累计总量、可重复报）、failed/cancelled 任务也接受 → 原计划「终态一次性上报」改为**边跑边报累计值**；⑨ progress 仅 WS 广播、不持久化、Web 无消费方 → **本期砍掉**；⑩ thinking 可选、可见范围是工作区成员、不传最简单 → **默认不上传**（保留 `REPORT_THINKING` 常量开关）。Codex `apply_patch` 按形状兼容（用户决策）。

**实现**：

- ① **数据/接口先定**（`transcript/model.rs`）：`TranscriptMessage { seq, type, tool?, content?, input?, output? }`（None 省略）+ `UsageRow`（七字段）+ 常量块（flush 500ms、排空 5s、output 8192、text 32768 拆条、输入字段 16k/整体 64KB、批 200 条/256KB、缓冲 512、跟踪表 2048/64）。`client.rs` 增 `post_task_messages` / `post_task_usage`（`json_send` 单次尝试、10s 超时、PAT、无 workspace 头）。
- ② **投影**（`projector.rs`）：观测点在 `acp_live_update_emitter` **IPC 压缩前**（压缩剥掉 `rawInput/rawOutput`）。文本事件是**全量累积快照**（`apply_streaming_delta` 语义），按 stable item id 的「已见字符数」差量投影，快照变短（压缩/重建）视为新分段整段重投影；每拍 `close_segment` 让文本按 500ms 成段；工具行按 `toolCallId` 配对，`use` 只发一次（`rawInput` 首次可用；终态仍无输入发 `{}`），`result` 只在终态发一次（`rawOutput.formatted_output` → agent transcript `toolOutput` → `toolCall.content`，截 8192）。工具名优先级：`_meta.goldBandConversation.toolName` → agent transcript 关系 → 标题首词 → `tool`。子 agent 分支（`event_branch_id != root`）不上报。
- ③ **输入归一化**（`tool_input.rs`）：**形状优先、不看工具名**——`changes[]` → `file_path+content` → `file_path+old_string+new_string` → `{edits[]}` 合成 `changes[]`（构造 unified diff）→ Codex patch 文本（只对含 `*** Begin Patch` 等标记的字符串解析，避免把普通内容误判）→ `{command}` → 原样透传；单字段/整体双上限，二次收紧后仍超则 `{"truncated":true}`。所有字符串清洗 NUL。
- ④ **上报器**（`reporter.rs`）：`TranscriptRegistry`（两层 map，`&str` 查找零分配）作为 managed state；每 run 一个 `Reporter`（`Projector` + `next_seq` + 用量累计 + closing/stopped + oneshot）。worker `select!`(500ms tick / Notify) 取批→锁外 POST→锁内推进 seq；`TaskNotFound` → 停止上报；`closing` → 最后一批 + 用量 + oneshot 收尾。**不重试**（重复 seq 会产生重复行）。
- ⑤ **用量**：lifecycle 回调是热路径——`NodeCompleted` / `AcpTurnFinished`（放在 `outcome != Completed` 早退之前）只做内存登记 `usage_pending.insert(attempt_dir)`；worker 经 **blocking 线程池**读取 `acp.prompt-usage.jsonl`（同步 I/O 不占 async worker 线程），按 `(attempt_dir, turn_seq)` 去重、按 `(provider, model)` 汇总，dirty 时上报累计值，终态再兜底一次。
- ⑥ **接线**：`start_run` —— resume 分支在 `register_active_run` 后、fresh 分支在 `create_conversation_run_vm` 返回后；`finish_run`（5s）——`complete_with_issue_done` 入口（完成路径 3 处共用）、`handle_run_failed`（新增 `attempt_dir` 入参，`RunCompleted{Failure|Killed}` 传入）、`cancel_remote_task`（3s）、两处启动失败 teardown 前；`report_error` —— 失败/取消各一条；取消检测（remote 已取消/404）在 `spawn_invalidate` 里 `report_error` + **后台** `finish_run`（本函数在 3s 心跳 tick 上被 await，不能为排空阻塞）。顺序统一为「执行记录完整 → 任务终态」。

**方案自评审**：过度设计——无新持久化字段/无新队列框架/无 seq 水位文件（服务端每次 retry 新 task_id 已消解跨进程问题）、progress 直接不做；抽象只到「纯投影 + 单一 worker」两层，且都可单测。性能——每任务每 500ms 至多一个 HTTP（空闲跳过），观测点是纯内存投影 + 一次注册表查找（本地会话零额外 I/O），用量文件读取由 worker 在边界后执行（每边界一次小文件读，不做轮询重读）；全部缓冲/跟踪表有界（512 / 2048 / 64），背压时丢最旧文本行并记日志；无全量扫描、无长持锁（POST 在锁外）。

**内网验证后优化（2026-11-13）**：基本流程内网跑通、未见阻塞后收尾两处——① 子 agent 分支过滤提前到注册表查找**之前**（子分支高频流式事件不再抢 registry 锁）；② `acp.prompt-usage.jsonl` 读取挪入 blocking 线程池（此前在 async worker 线程上同步读文件）。自评确认**不做**：同长度快照重写检测、usage 文件增量解析（O(T²)→O(T)）、drain 预序列化传递——绝对成本分别在「几乎不发生的场景 / 全程几十 MB 级解析 / 每批 <1ms」量级，加状态换不来可感知收益（违背过度设计约束）。

**验证结果**：`cargo test -p gold-band-desktop multica::transcript::` **25 passed**（投影差量/分段/缩短快照/工具配对/终态补 use/截断/溢出丢旧/错误清洗；输入归一化六种形状 + patch 解析 + move + 误判防护 + 双上限；用量去重与分 provider 汇总；worker finish 协议——closing 后必排空并回 done 信号）；`multica::client::tests::post_task*` **3 passed**（mock TCP 断言 path/body 形状、无 `X-Workspace-ID`、404 → `TaskNotFound`）；`multica::` 全量 **293 passed**；`cargo clippy -p gold-band-desktop` 对 `transcript/` 零告警；`cargo fmt` 通过。**内网验证（2026-11-13）**：基本流程跑通（消息时间线 / 终态 / 用量），未见阻塞；Codex `apply_patch` 真实样本校准仍待内网样本（本地无历史 Codex attempt）。

**文件**：`src-tauri/src/multica/transcript/{mod,model,projector,tool_input,reporter}.rs`（新增）、`src-tauri/src/multica/{mod,client,bridge,commands,loop_}.rs`、`src-tauri/src/commands.rs`（live emitter 观测点）、`src-tauri/src/main.rs`（managed state）；文档 `接入方案.md` §3.2.3/§3.2.8/C3/C4/时序图、本条。

---

### 12.96 改动九十四：runtime-assets 协议对齐 multica Runtime 页签重构（M5-cz，2026-11-13）

**背景**：multica 把「角色管理」「工作流管理」页面重构为**共享 / Runtime 双页签**（共享：目录发布/安装/导出/删除；Runtime：浏览本机资产清单、逐条发布/卸载/查看），并给出权威对接文档 `.claude/design/multica/2026-11-13-maling-runtime-asset-protocol.md`。逐条对照其 §4 八项改动清单：**六项已满足**（uninstall 心跳分支与幂等执行器、install 写入语义、list `is_builtin`、workflow export 剥 modelBindings、role frontmatter name==key、端点区分、60s 窗口——M5-cq/cr/cw 内网已验证）；真实差异四处，全部是对齐性小改。

**实现**：

- ① **list 支持 `asset_type:"all"`**（协议 §3.1 新枚举值）：原实现只认空串 = 全量，`all` 会落入未知类型 → `supported:false`——重构后用户打开 Runtime 页签即降级报错。提取纯函数 `list_scope(asset_type) -> ListScope{want_role, want_workflow, supported}`，空串与 `all` 同为 role+workflow 合并盘点，`handle_list` 消费之（原 inline 布尔判断上收，可单测）。
- ② **install 回报字段改名 `installed_name` → `installed_key`**（协议 §3.3「建议回传」；旧角色对接文档确用 installed_name，重构后改名）：`RuntimeAssetsInstallResult` 字段重命名，保留 `installed_display_name`（multica 反馈 P3 协商字段）与 `outcome`（#6 落库形态）；role/workflow 两个构造点同步。
- ③ **uninstall 回报字段改名 `uninstalled_key` → `removed_key`**（协议 §3.4 示例字段；result 是自由 map，multica 仅透传 UI）：`RuntimeAssetsUninstallResult` 字段重命名。**本地 Tauri 事件 `gold-band://runtime-asset-uninstalled` 载荷的 `uninstalledKey` 不动**——那是另一契约（前端 `web/tests/runtime-asset-uninstalled-event.test.ts` 锁定）。
- ④ **classify 接受详情 `action:"export"`**（协议 §2 详情枚举含 export；心跳 ack 侧仍别名 install 兼容已部署客户端）：`classify_runtime_assets_request` 的 install 臂扩为 `"install" | "export"`，export/push-install 仍按 item_id 空/非空区分。

**方案自评审**：过度设计——四处均为常量比较与字段重命名，无新状态/抽象/IO；`ListScope` 是把既有 inline 判断上收为纯函数（换可测性，非新概念）。明确**不做**：角色重装不引入「同 key 跳过」——本地角色 key 空间是 `pf-` id、安装后不保留市场 slug，「重装落改名副本」是内网多轮验证后的协商行为（P3 display name 正为此），工作流侧 ReusedExisting 跳过已实现（协议 §4 第 3 项措辞是「确认」而非新增）；256KB 上限不做 daemon 预检——服务端发布时校验（ARTIFACT_TOO_LARGE），预检属重复校验；§5 三缺口（conflict_strategy / installed_version / workflow deps）文档明说不阻塞本期。性能——零影响（无新 IO/状态，字段改名不进热路径）。

**验证结果**：`cargo test -p gold-band-desktop --bin gold-band-desktop "multica::"` **295 passed**（+2：`classify_accepts_explicit_export_action`——role/workflow 两形态显式 export 均正确路由，漂移先红；`list_scope_treats_all_as_full_enumeration`——空/all 同义、单类型只回该类型、未知降级 supported:false；既有 client 序列化测试更新为断言 `installed_key`/`removed_key` 且旧字段名不再出现）；`cargo clippy -p gold-band-desktop --all-targets` 对改动文件零告警；`cargo fmt` 通过。

**文件**：`src-tauri/src/multica/{client,runtime_assets}.rs`；文档 `接入方案.md` D5（权威契约指针 + 详情 action 枚举 + `all` 语义 + 回报字段名）、本条。

---

### 12.97 改动九十五：transcript 上报用户输入——初始需求 seq=1 + 中途插话混批（M5-da，2026-11-16）

**背景**：multica 侧需求——Transcript 弹窗完整回放会话，用户的话和 agent 的响应交替出现。契约：复用既有 `POST /api/daemon/tasks/{taskId}/messages` 批量上报，**零新增协议**；`type` 固定英文小写 `"user"`（multica 前端将针对该字符串加专门渲染，定死以后不用改数据），只填 `content`（tool/input/output 不填），服务端无 type 白名单、对 content 照常脱敏+NUL 清洗；时序上**初始输入必须在任务开始执行前作为 seq=1 第一条上报**，中途插话在收到用户消息的时刻进同一批队列（无独立用户输入通道）；内容规则——初始输入报**用户需求原话**而非组装后完整 prompt（拼系统指令/上下文注入的组装是引擎内部机制，报了会把第一行变成几千字噪音），中途消息原样上报、不改写不摘要不拼接。

**实现**：

- ① **model**：`TYPE_USER = "user"` + `TranscriptMessage::user(content)`（复用 body 模式，线上形状只有 `seq/type/content`）。
- ② **初始输入——reporter 构造期显式推入**（保证 seq=1，不依赖事件观测、天然免疫 fresh 后台启动的首拍竞态）：`Projector::new(initial_user_text)` 构造时 sanitize → 非空则占住 outbox 最前部 → 首批 drain 分到 seq=1。内容来源：fresh = `input.content`（composer 提交的需求原话，`conversation_run.rs` 逐字克隆进 requirement_content，不裁剪）；resume = 被续任务 requirement 文件读原文（`resumed_run_mode` 同款同步小文件读，读失败 → None 无初始行）。resume 产生新 task_id、transcript 从空开始，把驱动续跑的原需求作为 seq=1 是忠实回放。
- ③ **中途插话——观测 userTextDelta 回声**（`record_user_prompt_event` 合成的 goldBandPrompt 事件，在 prompt 发给 agent 之前合成、经既有观测点进同一批队列、同频率 flush）：`is_reported_event` 加 `"userTextDelta"`；`observe_user_echo` 四过滤——root 分支（既有）+ `raw.source=="goldBandPrompt"`（只认码灵合成事件，agent 回显的 session/update 原生事件 client 侧已抑制，双保险）+ `hiddenFromChat != true`（隐藏注入不报）+ `raw.originalUserText==true`（`prompt.display_text` 存在 = composer 用户亲手输入；workflow/auto 节点 turn 与带隐藏 section 的 direct 首 turn 均为 false，不会把引擎组装 prompt 报成用户的话）。入队前 `close_segment()` 收起进行中的 agent 文本段（与 `push_error` 同模式），保持「agent 说到哪 → 用户插了什么」因果顺序；超 32k 拆连续多条。
- ④ **回声去重**：fresh 直发场景 seq=1 已显式上报初始需求，首拍回声与它逐字相等且必然先于任何 agent 输出 → 双报。规则：`!agent_activity_seen && initial_user_text == content` 跳过。等式链已验证：`input.content` → requirement_content（逐字克隆）→ `initial_prompt_display`（读 requirement 文件，`From<String>` 逐字包装）→ workflow 首 turn display_text，sanitize 幂等。`agent_activity_seen` 仅在非空文本/工具事件置位（空内容事件不算）；agent 一旦有产出，之后同内容回声都是真实插话，照常上报。极端角落（agent 开工前用户排队发逐字节相同消息）误跳过一次，概率趋零，不加状态。
- ⑤ **接线**：`start_run` 增 `initial_user_text: Option<&str>`——resume 分支传 requirement 文件原文（先读后 move 进闭包）、fresh 分支传 `input.content`。

**方案自评审**：过度设计——零新增协议/端点/队列/持久状态，复用既有批队列、观测点与 flush worker；Projector 新增两个内存字段（`Option<String>`+`bool`）是去重所需最小机制，无 canonical 数据复制。性能——每条用户回声 = 一次注册表查找 + 纯内存推送，无新 I/O；userTextDelta 频率 = 用户发言频率（极低），对 500ms 批影响可忽略；resume 的 requirement 读是一次小文件读（与 `resumed_run_mode` 同款同位置）。非 multica 本地会话依旧零开销。

**验证结果**：`cargo test -p gold-band-desktop multica::transcript` **33 passed**（新增 8：初始 seq=1+线上形状仅 seq/type/content、空白初始跳过、插话与 agent 文本交替且 seq 连续、初始回声去重、agent 活动后同内容照报、四类非用户回声+无 raw+空白全拒、插话先收起开放文本段、超长拆连续两条；既有 25 回归全绿）；`cargo clippy -p gold-band-desktop --all-targets` 对 transcript/ 零告警。

**文件**：`src-tauri/src/multica/transcript/{mod,model,projector,reporter}.rs`、`src-tauri/src/multica/commands.rs`（两处 start_run 调用点）；文档 `接入方案.md` §3.2.8（user 类型语义）、本条。

---

## 附录 A：CLAUDE.md 合规自检

- ✅ 先定数据（2.2）→ 再定接口（2.8/第 7 章）→ 再补实现（2.3–2.7/第 4 章）
- ✅ 错误码用结构体（MulticaError → CommandErrorVm {code, params}），后端只返回码，不含对客文案（第 5 章）
- ✅ 杜绝硬编码：base_url/provider/重试间隔/心跳间隔均为配置或模块顶部常量，集中管理（2.2/2.3/2.6）
- ✅ 生命周期相关数据状态统一管理：multica 运行期状态集中 `MulticaRuntimeState`（2.5）；断点续跑索引 `multica_task_conversations`（remote_task_id→{local_task_id,local_run_id,session_id}）进 StateConfig（2.2.3）
- ✅ 复用现成库/模式：reqwest/tokio/tauri async runtime + metrics.rs/feedback.rs 既有模式（2.1/2.3–2.7），每个能力标注 file:line
- ✅ 复用库层会话执行 API：一个 remote_task = 一个本地 task（`create_task_from_requirement` + `run_start_background`，库层 App API），不重复造 runtime、不走 command 层；Direct/Auto workflow preset 上提 `gold_band::dsl::presets` 公开复用；浏览器登录复用 multica 原生（localhost callback），server 零改动
- ✅ 破坏式更新：旧配置 Option+serde(default) 兼容，不建兼容层/灰度/fallback；无需升 schema 版本（2.2.6）
- ✅ 外部命令约束：multica 不起外部子进程（执行用 gold-band runtime），不涉及 background_command；若 future 需 helper 则经 `background_command`(process.rs:44)
- ✅ 提示词 src/prompts/：multica 专用提示词已入 `src/prompts/{zh-CN,en}/runtime/remote_task_context.md` 双语同构（12.48 改动四十六），不在实现代码硬编码长 prompt 文本
- ⏳ 同步维护 docs/gold-band/产品设计文档 + 开发计划：实现时同步（CLAUDE.md 强制）


### WB 服务地址构建注入

WB 渠道以 `GOLD_BAND_WB_BASE_URL` 为服务地址、`GOLD_BAND_WB_MULTICA_URL` 为 Multica 地址的构建期事实源，配置模板分别使用 `${GOLD_BAND_WB_BASE_URL}` 与 `${GOLD_BAND_WB_MULTICA_URL}` 引用。本地开发和构建须同时设置这两个变量，正式发布由同名 GitHub Actions Secret 注入；缺失或不是不含路径、凭据、查询和片段的 HTTP(S) origin 时构建失败。更新、发布资源、指标、MCP、帮助信息和官网书签从基础地址派生；Multica 使用独立注入的地址，不再从基础地址派生端口。地址会进入客户端和发布产物，注入不提供运行时保密性。默认渠道不要求这两个变量。
