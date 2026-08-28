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
| `multica/loop_.rs` | 启动全量 register / 新添加 register / **常驻 15s 心跳 + prepare-lease 续期（Req D）** / recover-orphans / 取消检测 | `metrics::start_heartbeat_polling`(metrics.rs:218-242) spawn + 三层 guard 样板 |
| `multica/bridge.rs` | remote_task ↔ 本地 task/run 衔接（**直接调 `gold-band` 库层 App API** + 订阅 lifecycle bus，不走 Tauri command 层）；**执行注入（M5-z）**：发送时由 `start_multica_conversation_run` 接收 composer 下拉选中的 `local_project_id` 写入 `ActiveRemoteRun.local_project_id` → 按 `workspace_entry_for_project(&home_state, &local_project_id)` 解析 `workspace_path` → `App::with_config(workspace_path, config).with_lifecycle_bus(shared_bus)`(app/mod.rs) → 构造 Direct WorkflowDsl → `create_task_from_requirement` + `run_start_background` | `metrics::create_metrics_subscriber`(metrics.rs:365-638)；`lifecycle_bus.subscribe_named`(observability.rs:43-49)；`view_models_conversation.rs:2579`（Direct workflow preset 范本） |
| `multica/error.rs` | `MulticaError` 枚举 + 映射 `CommandErrorVm` | `CommandErrorVm`(commands.rs:581-634) + `command_error`(commands.rs:3529-3547) |

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
| `multica_base_url` | `&'static str` | `http://localhost:8080` | `http://maling.weoa.com:5005`（nginx 统一入口，见 §12.30） |
| `multica_app_url` | `&'static str` | `http://localhost:3000` | `http://maling.weoa.com:5005`（前后端同源同入口） |
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
pub struct RemoteTask { pub id: String, pub issue_id: Option<String>, pub status: String, pub auth_token: Option<String> /*=server 回传 mat_；Option B 中介下不消费，§12.29 起定点 #[allow(dead_code)]*/, pub prior_session_id: Option<String> /*=响应回填，客户端只消费不发送；§12.29 起定点 #[allow(dead_code)]（主路径 parent_task_id）*/, pub parent_task_id: Option<String> /*=auto-retry 子任务 T' 指向父 T；客户端续跑反查主路径（§12.14）*/, pub title: Option<String> /*=wire thread_name*/, pub requirement: Option<String>, pub last_activity_at: Option<String> }
pub struct ClaimRequest {}   // 服务端 claim 处理器不解码 body；prior_session_id 是「响应只输出」字段（agent.go:311），客户端消费响应而非塞请求体（§12.14）
pub struct PinTaskSessionRequest { pub session_id: String, pub work_dir: Option<String> }
pub struct StartRequest { pub force_fresh_session: bool }          // rerun 时 true（详见 4.4 / 接入方案 3.2.7）
pub struct CompleteRequest { pub output: String, pub session_id: Option<String>, pub work_dir: Option<String> }
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
- `start_task(task_id, force_fresh_session) / heartbeat / get_task_status`（本期状态只基础：start/complete/fail + 心跳，不接入 step/total 进度上报）
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
- **常驻 15s 心跳**（Req D 演示反馈调整，由「执行期」改为「常驻」）：与 multica 建立连接后即对**全部已连接 workspace 的 runtime** 持续心跳，不再仅 claim→complete 期间。迭代源由 `collect_active_runtime_ids`（仅 `active_runs`）改为 `state.all_runtime_ids()`（读 `runtime_ids`，= 所有已连接工作空间）；`active_runtime_ids` 保留给 cancel 检测用。每 15s `POST /heartbeat`（≠ metrics 的 15min，两套并存）。
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
  ├─ 执行期：常驻心跳每 15s POST /heartbeat {runtime_id}（**Req D：已常驻，非仅执行期**；维持在线，否则 150s 判离线→fail）
  │          本期只基础状态（start/complete/fail + 心跳），不上报 step/total 进度
  │          周期 GET /tasks/{tid}/status 检测取消（cancelled/failed/404 → 中断本地 run）
  └─ run outcome（**首轮 run 终结即触发，单轮即完成**；码灵不在单 remote task 多轮，多轮走新 task 续跑同 session，见 4.4）。订阅器穷举 4 分支（见 2.5 终态表）：
       RunCompleted{Success} → POST /tasks/{tid}/complete {output, session_id=ACP session_id, work_dir=任务级 local_project_id 解析}（重试幂等）→ 清 multica_task_conversations[tid]
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
| C1 | `/api/daemon/heartbeat` | POST | PAT | **常驻 15s（Req D：建立连接后即对全部已连接 workspace 持续，非仅执行期）** |
| C2 | `/api/daemon/tasks/{tid}/start` | POST | PAT | dispatched→running；body `force_fresh_session`（整任务重跑=true） |
| C2.5 | `/api/daemon/runtimes/{rid}/tasks/{tid}/prepare-lease` | POST | PAT | **Req D 新增续期**：claim-at-click 后、start 前（用户在 composer 选模型/改需求期间）每 15s 续期 45s lease，防 `ReclaimStaleDispatchedTaskForRuntime` 回收（与 multica daemon 自带 `startTaskPrepareLeaseExtender` 同构） |
| C3 | `/api/daemon/tasks/{tid}/progress` | POST | PAT | **本期不接入**（状态只基础 start/complete/fail） |
| C5 | `/api/daemon/tasks/{tid}/status` | GET | PAT | 检测取消 |
| C6 | `/api/daemon/tasks/{tid}/complete` | POST | PAT | 重试幂等；body `session_id`+`work_dir` |
| C8 | `/api/daemon/tasks/{tid}/session` | POST | PAT | PinTaskSession：写 `session_id`/`work_dir` 到 task 行（断点续跑依据） |
| C7 | `/api/daemon/tasks/{tid}/fail` | POST | PAT | 重试幂等 |
| D1 | `/api/issues/{id}/rerun` | POST | PAT | X-Workspace-ID；用户手动重试 |

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
| 3 | **两套心跳并存**：metrics 15min + multica 15s | 独立循环，互不干扰；**multica 心跳 Req D 起改为常驻**（建立连接后对全部已连接 workspace 的 runtime 持续，非仅执行期）；同一常驻循环还承载 claim-at-click 后的 prepare-lease 续期 |
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
| direct | agent 跑完 -> `RunCompleted{Success}` -> `handle_run_completed` -> `classify_terminal(Success)` -> `TerminalAction::Complete` -> `complete_task` + `finalize_terminal` |
| workflow | workflow run 跑到终态（自然结束 / 节点序列走完）-> 同上 `RunCompleted{Success}` -> `complete_task`。「按终止来完成」即完成由 run 终止事件驱动，不另设手动入口 |

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
4. 启动自愈：`main.rs` setup → `recover_interrupted_running_sessions` → `pause_all_running_sessions`（app/mod.rs:2370）把 stale-Running 翻成 Paused + ProcessInterrupted + outcome None → 崩溃重启后旧 run 自动满足 is_run_continuable。**原限制：仅覆盖 home repo（激活 workspace）**——multica 远程任务的 run 落在各 task 自身 `work_dir`（独立 repo），home 自愈够不到；**§12.15（改动十三）补全**：home 自愈后追加 `recover_multica_work_dir_sessions`，遍历 `multica_task_conversations` 的全部 work_dir 逐个自愈。
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
- **诚实边界**：非 multica 的 StateConfig 写点（pin/unpin、workspace 等用户低并发操作）本期未迁移——`with_state` 作为**增量采用原语**，后续写点逐步迁入即可，不破坏现状。

**附带（非 multica，解锁验证）**：`src/config/mod.rs` 测试模块有 2 处死 import（`MANAGED_AGENT_PRESETS` / `managed_agent_preset`——符号早已不存在，仅 test `use` 残留；lib 测 crate 从未被编译故未暴露）。删除以解锁 `cargo test -p gold-band` 编译，使 `with_state` 单测可运行。

**文件**：`src/app/mod.rs`（`STATE_CONFIG_LOCKS` 分片 + `state_config_lock` + `with_state` + 4 单测：persists-mutation / skips-save-when-clean / reads-current-disk / serializes-concurrent-rmw 32 线程无 lost-update）、`src-tauri/src/multica/{bridge.rs, commands.rs}`（5 写点迁移）、`src/config/mod.rs`（死 import 清理）。

**验收**：`cargo test -p gold-band --lib with_state` 4 测全过（含 32 线程并发 RMW 终态 len==32、无 lost-update）；`cargo test -p gold-band-desktop multica::` 83 测全过（5 迁移点零回归）。

> §12.24–12.27 为 multica 全链路审计（#75–#78）回溯加固：#75（🔴 running 孤儿）纠正「server running 无逐任务 liveness」下的终态上报契约；#76（M1）清理 §12.20 起的 pinned/retryable 死管线；#77（M2）根治远程任务页订阅竞态 + 事件去重；#78（M3）补齐 StateConfig 并发 RMW 原子性。接入方案 §3 终态上报契约、§5.2 M3 数据模型（`multica_pending_issues`/`retryable`/`pinned_tasks` 已废）以本四节为准。

---

### 12.28 改动二十六：multica 绑定 chip 内嵌输入框（leading adornment + text-indent 让位）+ Backspace 删除条件放宽（M5-ap，2026-08-13）

**背景（用户 UX 调整）**：用户要求 multica 绑定 chip「不是放在输入框的上面，而是放在输入框里面，用户用删除键都可以直接删除的那种」；并明确「其余这个标签是符合预期的」（样式/文案/显示时机不变）。

**根因（好设计、实现可完善）**：§12.22（M5-aj）的独立 chip 方案（与正文解耦、可见可控、删除语义明确）本身正确，但两点实现可完善：① chip 作为 `PromptInput` 的首子节点（block-flow，输入区**上方独立行**），与正文分离，不像「正文的一部分」；② Backspace 删除要求**正文为空**（`visibleContent.trim() === ''`，避免误删正文），体感上「不够直接」——用户希望删 chip 像删正文最前一个字一样自然。

**方案（复用既有 leading adornment 机制 + 提取可测纯函数）**：
- **chip 内嵌（复用 slash 同款机制）**：chip 从 `PromptInput` 首子节点 block 行移进 textarea 的 `relative` 容器，作为正文最前的 leading adornment——绝对定位左上角（`absolute left-0 top-0 z-10`），正文首行经 `useLeadingAdornmentTextIndent`（`text-indent = chip 宽度 + 0.25rem`）缩进让位；CSS `text-indent` 只作用于首行，换行后正文回到左侧（chip 只占首行高度，不与后续行重叠）。chip 与 slash 命令标签互斥（**slash 优先**——绑定预填的是任务需求文本、非 slash 命令），共用同一套 `leadingAdornmentLayout`（原仅服务 slash 的 `committedInputLayout` 重命名 + `enabled` 泛化为 `committedSlashCommand || multicaChipActive`）。× 关闭按钮保留（点 × 仍删 chip）。
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

**背景（跨机器部署）**：将 MALING 客户端打包到非开发机运行、连远程 multica（部署在 172.21.18.88，经 nginx 统一转发，对外入口 `http://maling.weoa.com:5005`，前端 `/login` 与后端 `/api/*` 同源同端口）。核查发现 `configs/channels/wb.json` 的 `multicaBaseUrl`/`multicaAppUrl` 仍是 `http://maling.weoa.com`（默认 80 端口，过时）——与服务端实际 nginx 入口不符，连不上。

**根因（好设计、值过时）**：渠道配置链路本身正确（`configs/channels/wb.json` → `build.rs` 编译期 env → `channel.rs option_env!` → `config.rs multica_base_url/app_url`，且 `build.rs:146-157` 支持 env 覆盖 json），仅 multica 地址值滞后于服务端 nginx 端口调整。运行期不可改（`connect_multica` 不收 URL、设置页 multica 区块已移除、`save_multica_settings` 已删，见 §12.22/接入方案 §3.2.5），故必须修正编译期配置值而非加运行期旁路。

**方案（修正过时值，非补丁）**：wb 渠道本就是 maling 生态（appName MALING、updater/metrics/内置 MCP 均指向 maling.weoa.com），multica 地址属同一生态，直接把过时默认端口修正为实际 nginx 入口：
- `configs/channels/wb.json`：`multicaBaseUrl`/`multicaAppUrl` 由 `http://maling.weoa.com` → `http://maling.weoa.com:5005`。
- 前后端同入口：`base_url`（API 根，client 拼 `/api/*`）与 `app_url`（浏览器登录页 `<app_url>/login`）同源，顺带消除「前端页面 API 地址硬编码 localhost」隐患（同源直接命中，无 CORS）。
- `browser_login` 的 `cli_callback` 仍是 Gold-Band 本机 `127.0.0.1:<port>`，nginx 只透传 multica 后端登录成功的 302，跨机器 connect 不受影响（见接入方案 §3.2.6）。

**部署前置（服务端，非本项目代码）**：multica 服务端经 nginx 对外统一 `maling.weoa.com:5005`，已由用户确认就绪；Gold-Band 侧无需关心 server 内部 5000/5050 监听细节。

**验证**：`GOLD_BAND_RELEASE_CHANNEL=wb cargo check -p gold-band-desktop` 绿——build.rs 正确解析 wb.json 新值，编译期 env `GOLD_BAND_MULTICA_BASE_URL/APP_URL=http://maling.weoa.com:5005` 生效；default 渠道 `cargo check` 无回归。纯渠道配置值修正，无运行时逻辑变化，无性能影响（仅连接地址常量）。

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

### 12.34 改动三十二：multica 绑定 chip 配色改 accent 配对——深色主题可辨识（M5-av，2026-08-28）

**背景（用户反馈）**：切换深色主题后，composer 内 multica 绑定 chip「颜色和背景颜色相近导致难以辨认」。

**根因（正确设计但实现不完整：违反 token 配对语义）**：chip 自 M5-aj 起（§12.22）用 primary 单 token 染色：`border-primary/30 bg-primary/10 text-primary`。主题契约中 `primary/primaryForeground` 是「表面 + 表面文字」配对（button-primary recipe 同源），**不保证 primary 单独作前景色时与背景有对比**：gold-band 是单色反转主题（light `#0d0d0d` / dark `#f0f0f0`），碰巧双 scheme 均高对比，掩盖了问题；tech-neutral 的 primary 是按钮表面深灰（light `#2f2f2f` / dark `#2d2d2d`），dark 下与 composer 背景 `#1b1b1b` 几乎重合——`text-primary` 对比度 ≈1.1:1，`/10` 背景、`/30` 边框同样融入背景。`styles.css` 兜底 dark（`#313131` on `#181818`）同病。

**方案（对齐主题契约，非深色补丁）**：chip 改用契约中保证双 scheme 对比度的「强调表面」canonical 配对 `accent/accentForeground`（permission-card、recipe hover/selected、ui-interaction.md §6/§9 同源原则）：

- chip：`border-accent-foreground/15 bg-accent … font-medium text-accent-foreground`
- 关闭按钮 hover：`hover:bg-accent-foreground/15`（accent-foreground 半透明，light 下变深 / dark 下变亮，方向自然）

四组合均对比安全：gold-band 浅绿底深绿字 / 深绿底亮绿字；tech-neutral 浅灰底深灰字 / 深灰底浅灰字。第三方主题遵守契约即自动适配。本节取代 §12.22 中 chip 配色描述（结构/文案/交互不变）。

**验收固化**：`web/tests/conversation-composer-multica-chip.test.ts` 新增 `multica binding chip theme contrast` describe——chip 渲染段（`multicaBinding ? (` 至 `<PromptInputTextarea`）必须含 accent 配对三件套，且不得再匹配任何 primary 染色（`/-primary(?!-)/`，`primary-foreground` 豁免）；断言均为单行字符串（CRLF 安全）。

**验证**：vitest 16/16（multica-chip + composer-context-alignment）；`tsc -p web/tsconfig.build.json` 零错；`web:build` 成功；浏览器 light/dark × gold-band/tech-neutral 四组合 chip 静态 + 关闭按钮 hover 截图验证通过。

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
- ⏳ 提示词 src/prompts/：本期 remote_task.requirement 作为本地 task 的 user prompt，不新增 system prompt，不触发该规则；若后续需 multica 专用 prompt 则入 src/prompts/ zh-CN/en 双语
- ⏳ 同步维护 docs/gold-band/产品设计文档 + 开发计划：实现时同步（CLAUDE.md 强制）
