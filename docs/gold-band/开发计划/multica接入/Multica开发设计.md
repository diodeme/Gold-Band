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
M5  前端 UI（设置页连接/添加工作空间 + 会话模式本地/远程任务列表切换 + 事件 + i18n）
```

> M0 与 M1–M5 分属两个仓库，M0 可先合入 multica main；码灵侧用本地 multica 分支联调。

### 0.3 命名与术语规范（强制）

multica 实体与码灵自有概念同名但语义不同，码灵侧一律加前缀区分（见接入方案 3.2.1）：

| multica 实体 | 码灵侧命名 | 码灵自有同名概念（不得混淆） |
|---|---|---|
| runtime（执行目标实例） | `multica_runtime_id` | 本地工作流运行时 |
| task（issue 派生远程任务） | `remote_task` | 本地工作流任务（preset→task→run） |
| agent（workspace 智能体配置） | `multica_agent` | ACP agent / provider |
| workspace（SaaS 多租户边界） | `multica_workspaces`（每条含 `local_project_id`）/ `active_workspace_id` | 码灵本地工作目录（`conversation_workspaces` 条目，`workspace_path`） |

> **workspace 与本地目录一一绑定**：每个添加的 multica workspace **必须绑定一个本地工作目录**（复用会话模式 `conversation_workspaces`，以 `local_project_id` 引用）。multica workspace 决定「任务从哪个团队派来」，其绑定的 `workspace_path` 决定「任务在哪执行」——二者通过绑定关联，不再正交。下文凡指 multica 侧概念一律带前缀。
>
> **三层 workspace 勿混**：① multica workspace（远端，`multica_workspaces`）；② 码灵本地工作目录（`conversation_workspaces`，可被 ① 绑定，也可独立用于本地会话）；③ 会话实例（一次会话打开的目录）。multica workspace 的「执行落点」= 它绑定的 ② 的 `workspace_path`。

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
| `multica/config.rs` | 配置 VM + 读写 + `pat_set` getter + `multica_settings()` 聚合 + **绑定查找**（`multica workspace → local_project_id → workspace_path`，复用 `workspace_entry_for_project` conversation_workspace.rs:24） | `metrics::metrics_settings`(130-167) channel-priority + normalize；`MetricsSettingsVm`(79-89) |
| `multica/state.rs` | 运行期内存状态（runtime_id 映射、在飞任务映射）+ 持久化进 StateConfig | 不塞 `DesktopState`（见 2.5）；持久化参考 StateConfig(config/mod.rs:666-689) |
| `multica/loop_.rs` | 启动全量 register / 新添加 register / **常驻 15s 心跳 + prepare-lease 续期（Req D）** / recover-orphans / 取消检测 | `metrics::start_heartbeat_polling`(metrics.rs:218-242) spawn + 三层 guard 样板 |
| `multica/bridge.rs` | remote_task ↔ 本地 task/run 衔接（**直接调 `gold-band` 库层 App API** + 订阅 lifecycle bus，不走 Tauri command 层）；**执行注入**：claim 后按 task 所属 workspace 取绑定的 `workspace_path` → `App::with_config(workspace_path, config).with_lifecycle_bus(shared_bus)`(app/mod.rs) → 构造 Direct WorkflowDsl → `create_task_from_requirement` + `run_start_background` | `metrics::create_metrics_subscriber`(metrics.rs:365-638)；`lifecycle_bus.subscribe_named`(observability.rs:43-49)；`view_models_conversation.rs:2579`（Direct workflow preset 范本） |
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
    pub local_project_id: String,   // ← 绑定的本地目录：指向 conversation_workspaces.project_id（GoldBandPaths::project_id 派生）。一个 multica workspace 只能绑一个本地目录（唯一）
    pub provider: String,            // ← 该 workspace 执行用的 ACP provider（如 "claude-acp"/"codex-acp"），添加时选定、绑定后不可变（变了=新 runtime=需重绑 agent）
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
pub multica_completed_tasks: Option<Vec<MulticaCompletedTask>>,   // 终态任务本地历史（「最近完成」回看，M5-o），去重最新在前、截断 N=50
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
    pub local_task_id: String,     // 「最近完成」行点击 → 按 (project_id, local_task_id, local_run_id) 直达本地会话
    pub local_run_id: String,
    pub workspace_id: String,      // 经 workspaces 解析为 local_project_id（= project_id）供前端 onSelectRun
    pub issue_id: Option<String>,  // rerun 用
    pub status: String,            // "completed" | "failed"
    pub title: String,             // 快照自 ActiveRemoteRun.title，缺失兜底 remote_task_id
    pub completed_at: String,      // RFC3339，finalize_terminal 时戳
}
```
> `multica_runtime_ids` 是缓存（register 幂等取回），丢失下次启动重建；`multica_pending_issues` 记录**失败待重试**的 issue_id（M4 终态 fail 时写入、complete/rerun 时清除；claim 不写——避免 running 任务被误显为 retryable），用于失败回显 + rerun；`multica_task_conversations` 是断点续跑核心索引（remote_task_id → {local_task_id, local_run_id, session_id}）——claim 时若命中（remote_task_id 存在且 session_id 非空），带 prior_session_id 续跑同一会话；complete 后清条目（详见 3.2.7 / 4.4）；`multica_completed_tasks` 是「最近完成」回看历史——`finalize_terminal` 移除 active 前快照一行（status 来自 PendingUpdate：ClearOnSuccess→completed / AddOnFailure→failed），去重最新在前、截断 N=50（M5-o）。

#### 2.2.4 channel config（编译期默认，5 处改动）

channel 字段是**编译期常量**（option_env!，参考 metrics 字段 channel.rs:4-22），新增 4 字段（multica 不需要 channel 级 api_key——PAT 登录后生成；浏览器登录需要 Web 前端地址，故单独设 `multicaAppUrl`，可能与 `multicaBaseUrl` 不同）：

| 字段 | DesktopChannelConfig（channel.rs:4-22） | default.json | wb.json |
|---|---|---|---|
| `multica_base_url` | `&'static str` | `http://localhost:8080` | 预填企业 multica API 地址 |
| `multica_app_url` | `&'static str` | `http://localhost:3000` | 预填企业 multica Web 地址（浏览器登录页） |
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
    pub workspaces: Vec<MulticaWorkspaceRef>,      // 每条含 provider + local_project_id；前端 join conversation_workspaces 显示 workspace_path
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
pub struct RemoteTask { pub id: String, pub issue_id: Option<String>, pub status: String, pub auth_token: Option<String>, pub prior_session_id: Option<String>, pub title: Option<String> /*=wire thread_name*/, pub requirement: Option<String>, pub last_activity_at: Option<String> }
pub struct ClaimRequest { pub prior_session_id: Option<String> }   // 断点续跑：命中本地 task_conversations 时带
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
    #[error("workspace empty")] WorkspaceEmpty,
    #[error("network failed: {0}")] NetworkFailed(String),
    #[error("register failed: {0}")] RegisterFailed(String),
    #[error("claim conflict")] ClaimConflict,
    #[error("task not found")] TaskNotFound,
    #[error("runtime offline")] RuntimeOffline,
    #[error("session resume failed, will rerun")] SessionResumeFailed, // **M4-d：变体保留在错误码表（multica.session-resume-failed），但断点续跑路径不 emit/不匹配**——改为「任何 resume Err→fresh fallback」更稳（无需 fragile 串匹配）
    #[error("pin task session failed: {0}")] PinSessionFailed(String),
}
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
- `claim_specific_task(runtime_id, task_id, prior_session_id?) -> RemoteTask`（断点续跑：命中本地 `task_conversations` 时带 prior_session_id）
- `start_task(task_id, force_fresh_session) / heartbeat / get_task_status`（本期状态只基础：start/complete/fail + 心跳，不接入 step/total 进度上报）
- `pin_task_session(task_id, session_id, work_dir) -> ()`（写 task 行的 session_id/work_dir，断点续跑依据，对应接入方案 C8）
- `complete_task / fail_task`（**重试幂等**：4/8/16/32/64s 共 6 次，确保终态送达）
- `rerun_issue(issue_id, workspace_id) -> ()`（rerun=true/retry=false，触发整任务重跑，见接入方案 3.2.7）

**重试策略**：
- **终态回调**（complete/fail）：严格 4/8/16/32/64s 共 6 次（初始尝试 + 5 次退避重试），服务端幂等。仅对 `NetworkFailed`（reqwest 传输/超时 + map_status 归类的 5xx + 解码错误）重试；AuthFailed/TaskNotFound/ClaimConflict 等确定错误码立即返回（重试无意义，与一般请求「4xx 不重试」同源）。由 `with_terminal_retry`（与 `with_network_retry` 同形，仅退避更长更密、次数 6）统一承载。
- **一般请求**（register/list/claim/pin_session/rerun）：网络错误重试 3 次（`NETWORK_RETRY_BASE_SECS=1`，第 n 次 `1*2^n`s），4xx 不重试直接映射错误码。
- PAT 走 `Authorization: Bearer <pat>`；issue 维度业务接口（D1/E1/E2，path 不含 workspace）加 `X-Workspace-ID` 头路由（接入方案 4.1，由 `post_json_with_workspace` 承载）；daemon 任务接口（C2-C8）path 自带 task_id 不需要该头。

> ⚠️ **终态重试是超出 metrics fire-and-forget 模式的增强点**：metrics 失败只记日志（metrics.rs:206 "FAILED (ignored)"），multica 终态必须送达（否则任务悬空），故 client.rs 自建退避重试，是本模块相对 metrics 的主要新增逻辑。

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
pub struct ActiveRemoteRun { pub local_task_uuid: String, pub issue_id: Option<String>, pub title: Option<String>, pub started_at: String }
// title（M5-o）：claim 时快照自 task 标题；finalize_terminal 写「最近完成」历史时无需回读 task.json 即可拿行标签
// Req D：`all_runtime_ids()` 读 runtime_ids.keys（= 全部已连接 workspace）供常驻心跳；`active_runtime_ids()`（仅 active_runs）保留给 cancel 检测。
// title（M5-o）：claim 时快照自 task 标题；finalize_terminal 写「最近完成」历史时无需回读 task.json 即可拿行标签
```

**绑定查找与会话执行注入**（绑定关系的核心机制，**直接调 `gold-band` 库层 App API，不重复造 runtime、不碰 lifecycle 契约**）：

> **关键澄清**：码灵库层**不存在「会话 / 工作流」二分**——前端会话模式（新 UI）的 VM 内部恰恰就是调 `create_task_from_requirement` + `run_start_background`（`view_models_conversation.rs` 会话 VM 即此实现）。multica 复用同一套库层 API：**一个 remote_task = 一个本地 task**，首轮 prompt = requirement（由 `create_task_from_requirement` 写入 `requirement.md`，Worker 节点自动读取 `node_executor.rs:610`），**不走 `submit_conversation_prompt`**。bridge 直接调库层，不经 Tauri command 层、不需前端 AttemptLocator。
>
> **会话完成判定（单轮即完成，无多轮歧义）**：**一个 remote_task = 一次执行（一个 run），不是可多轮的会话**。码灵作为 daemon 只跑 requirement 这一轮——首轮 requirement 驱动的 run 具 runtime-continue 性质，跑完自然 `RunCompleted`（码灵**不在单 remote task 上承载追问**，对话发起方是 multica）。不存在「多轮会话何时算完」的问题。**「多轮对话」由 task 序列承载**：用户在 multica web 对同一 issue 发新需求 → 新 queued task → 码灵 claim 带 `prior_session_id` 续跑**同一 ACP session**（上下文连续，见 4.4）=「多轮」，而非一个 task 内多轮。
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

- **绑定存储**：`SettingsConfig.desktop_multica_workspaces[i].local_project_id`（2.2.1）→ 引用 `StateConfig.conversation_workspaces` 的 `project_id`（`config/mod.rs:682`，`ConversationWorkspaceEntry` 2005-2010）。**不在 multica 侧重复存 path**，避免双份不一致。
- **查找函数**（M4-c 已落 `multica/config.rs`）：返回绑定的本地目录 **+ provider**——
  ```rust
  /// multica workspace → 绑定的 (workspace_path, provider)。先按 multica id 查
  /// local_project_id + provider，再复用 workspace_entry_for_project (conversation_workspace.rs:24) 查 workspace_path。
  /// 入参取 RuntimeConfig（合并态，desktop_multica_workspaces 为非 Option Vec，与 multica_settings 一致）+ StateConfig。
  pub fn binding_for_multica(
      config: &RuntimeConfig, state: &StateConfig, multica_workspace_id: &str,
  ) -> Option<(String /* workspace_path */, String /* provider */)> { ... }
  ```
- **会话执行注入**（**Req D 重构**：原 `start_multica_remote_task` 原子 claim+start 已删除拆分；现在 claim 只登记 lease + 返回 requirement，真正的执行注入在用户发送时由 `start_multica_conversation_run` 完成，**复用本地 `create_conversation_run_vm(&App, &ConversationCreateInputVm) -> anyhow::Result<ConversationRunVm>`（`view_models_conversation.rs:2590`，pub fn）**：该函数内部已建工作流（Direct/Auto）→ `create_task_from_requirement`（requirement 作首轮 prompt）→ 写 `conversation.json` → 拷附件 → `run_start_background` → 返回带 `run_id` 的 VM。multica 发送路径直接调它，再叠加 multica 专属簿记（`register_active_run` + 落 `multica_task_conversations` + `client.start_task`）。下方伪码描述的是 `create_conversation_run_vm` 内部等价机制，落点改为复用该函数而非在 multica 侧重写一遍）：
  ```rust
  let (workspace_path, provider) = binding_for_multica(&context.config, &home_state, &workspace_id)?;
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
  // ④ 登记 active_runs（真实 run.id，先于 NodeCompleted/RunCompleted 归属反查）+ 落断点续跑索引
  shared.register_active_run(&remote, ActiveRemoteRun { /* local_task_id=summary.task.id, local_run_id=run.id, ... */ });
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
- **complete 上报**：success → `complete_task(output=会话产物摘要, session_id=ACP session_id, work_dir=绑定目录)`；终态 `work_dir` = 绑定 `workspace_path`。
- **未绑定/目录缺失**：claim 时 `binding_for_multica` 返回 None → 报 `multica.task-not-found` 并引导用户在远程任务列表【添加/重新绑定工作空间】。

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
| `connect_multica` | async | — | `MulticaSettingsVm` | **主入口：远程任务列表未连接空状态点【连接 Multica】**（设置页 multica 区块同步可见，作辅助）；PAT 失效/未连接时触发浏览器登录（localhost callback，4.1 ①-⑦）+ 换 PAT；登录态变更后 emit `multica-task-updated` 让会话侧栏 re-fetch |
| `disconnect_multica` | sync | — | `MulticaSettingsVm` | **对称于 `connect_multica` 的断开**：清 PAT（`connected` 判定依据，经纯函数 `clear_multica_session`），保留 daemon_id / workspace 绑定（与本机标识、目录绑定正交），清运行期 register 缓存 `clear_runtime_ids`（重连后 loop 重建）；emit `multica-task-updated` 让会话侧栏 re-fetch 回到未连接空状态。设置页 multica 区块【断开连接】入口（换账号/退出/本地反复联调；active_runs 保留——在飞本地 run 的 remote 映射，断开不改其归属） |
| `list_server_multica_workspaces` | async | — | `Vec<MulticaWorkspaceRef>` | 拉 server 全量作**添加下拉数据源**（去除已添加） |
| `add_multica_workspace` | async | `{workspace_id, local_path, provider?}` | `MulticaSettingsVm` | 单次添加一个：folder picker 选定本地目录（非 git 警告不阻断）→ `add_conversation_workspace` 取 project_id → 写 workspaces（带 local_project_id + provider，provider 缺省取 `default_provider`）→ register；可重复调用添加多个 |
| `rebind_multica_workspace` | async | `{workspace_id, local_path}` | `MulticaSettingsVm` | 重新绑定本地目录（换目录），不重 register（runtime_id 不变） |
| `remove_multica_workspace` | sync | `workspace_id` | `MulticaSettingsVm` | 移除（不触发 server 操作） |
| `set_active_multica_workspace` | sync | `workspace_id` | — | **纯视图切换，不 register** |
| `get_multica_tasks` | async | — | `RemoteConversationSidebarVm` | 返回**按 workspace 分组**、对齐 `ConversationSidebarVm` 形状的 VM（`workspaces` / `tasksByWorkspace` / `pinnedTasks` / `recentlyCompleted` 键名一致），供前端复用 `ConversationSidebar` 骨架直接渲染。每组下 queued + 失败可重试混排（失败任务带 `retryable` 标记）；`recentlyCompleted` 来自 `state.multica_completed_tasks`，按 `workspace_id→local_project_id` 解析 `project_id` 供前端 `onSelectRun` 直达（M5-o）。**保留全部 pending 任务**（含每个 workspace 的 quick-create 初始任务，可作连接测试；M5-p 初版过滤方案已应用户反馈回退）。数据源：远程 queued + 本地 `multica_pending_issues` 未完成 issue（失败回显）+ 本地 `multica_completed_tasks`（完成回看） |
| `claim_multica_task` | async | `task_id, workspace_id` | `RemoteTaskVm`（**含 `requirement`**） | selective claim（命中 `task_conversations.session_id` 带 `prior_session_id` 续跑）；需 `workspace_id` 解析 runtime_id；**claim 响应回填 `requirement`**（来源优先级取首个非空，镜像 server `computeTaskKind`：quick_create_prompt→chat_message→trigger_comment_content→autopilot_description→handoff_note→title）；**claim 成功后写入 `prepare_leases[task_id]`**（让常驻循环续期 45s lease）；**不写 pending_issues**（失败回显改由 M4 fail 写入，见 §2.2.3/§4.3）。**claim 不再立即 start**（Req D：claim-at-click）——返回 requirement 供前端预填 composer，发送时才由 `start_multica_conversation_run` 真正建会话 |
| `start_multica_conversation_run` | async | `input: ConversationCreateInputVm, remote_task_id, workspace_id` | `ConversationRunVm` | **Req D 新增**（替代已删除的原子 `start_multica_remote_task`）：用户在预填好的 composer 点「发送」后调用。① 解析 `runtime_id`；② 建 workspace `App`（镜像本地 `create_conversation_run`）；③ `validate_conversation_create_vm`（与本地同一校验）；④ **复用 `create_conversation_run_vm(&app, &input)`**（建工作流 + 建任务 + 写 conversation.json + 拷附件 + 启动 run，全部复用本地链路）；⑤ multica 叠加：`register_active_run` + 落 `multica_task_conversations` + `client.start_task(remote_task_id)`（dispatched→running，lease 不再需要）+ 从 `prepare_leases` 移除；⑥ 返回 `ConversationRunVm`（前端按本地会话同一回调导航 conversation-run） |
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
export interface MulticaWorkspaceRef { id: string; name: string; slug: string; localProjectId: string; provider: string }
export interface RemoteTaskVm { id: string; issueId: string | null; status: 'queued'|'running'|'completed'|'failed'; retryable: boolean; workspaceId: string; title: string; requirement: string | null /* Req D：claim 响应回填的需求正文，供预填 composer；pending 列表只有 title，正文仅 claim 后有（issue 型为 null，回退 title） */; lastActivityAt: string | null }
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
- **已绑定工作空间管理**（激活/改绑/删除；**不再有「添加工作空间」行**——开发阶段破坏式更新，添加入口收敛到会话侧栏远程任务列表弹窗，见 3.4）：列表展示已添加 + 绑定本地目录（join `conversation_workspaces` 显示 path）+ 各自 provider，可切换 active（纯视图）、移除、重新绑定目录。**改绑目录**经共享前端原语 `pickLocalDirectory()`（Tauri `plugin-dialog` `open({directory:true})`，浏览器态 null）选目录后调 `rebindMulticaWorkspace(id, localPath)`（显式 `localPath` 入参，文件选择器不再藏在 API 内部）；不动 provider。

### 3.4 会话模式远程任务列表（仅新 UI）

**挂载点：`components/conversation/ConversationSidebar.tsx`** —— 会话模式的任务列表入口（`ConversationSidebarVm` 含 `workspaces/pinnedTasks/tasksByWorkspace`）。**远程任务列表复用 `ConversationSidebar` 同一骨架**（不另造组件；`TaskRow` 的状态圆点 / 相对时间 / pin / 重命名交互全部复用）。**不要挂 `pages/TaskListPage.tsx`**（那是工作流旧 UI，正在边缘化）。

- **sidebar 顶端加「本地 / 远程」segmented 切换**（唯一新增的层；本地视图零变化、不出现任何 multica 入口）。
- **远程视图按连接态分三种空状态**：
  - **未连接**（无 PAT / PAT 失效）→ 空状态卡 + 【连接 Multica】按钮 → `connect_multica`（**主入口，首启不自动触发登录**）。
  - 已连接、无 workspace → 空状态卡 + 【添加工作空间】（选远程 + 选本地目录 + provider 绑定）。
  - 已连接、有 workspace → workspace 分组列表。
- **远程列表形态对齐本地**：左侧 multica workspace 分组（sticky header）→ 每个 workspace 下展开该 workspace 的 queued 任务列表 → **末尾常驻【添加工作空间】Plus 行**（连接态始终可见，对齐本地 `ConversationSidebar` 末尾添加入口）→ 点击弹**模态对话框 `MulticaAddWorkspaceDialog`**（远程工作空间下拉 `listServerMulticaWorkspaces` 过滤已绑定 + provider 下拉 + 「绑定本地目录」按钮 `pickLocalDirectory()` 弹系统文件资源管理器选目录、显示选中路径 + 底部【添加】，三项齐全才可点 → `addMulticaWorkspace(id, name, provider, localPath)`）。**已连接但无绑定工作空间**时空状态文案区分（`conversation.sidebar.multica.noWorkspacesBound`，引导去添加，区别于「无会话」）。`get_multica_tasks` 返回**按 workspace 分组**、对齐 `ConversationSidebarVm` 形状的 VM（不限于 active）；**失败可重试任务与 queued 同列混排**，仅多一个 `retryable` 标记 + 【rerun】按钮（不单开失败区）；每条【执行】（Req D：claim-at-click）→ `claimMulticaTask(taskId, workspaceId)` 拿 `requirement` → 经 composer draft context `prefill(requirement ?? title, {remoteTaskId, workspaceId, localProjectId})` → `onNewConversationInWorkspace(localProjectId)`（与本地『+』同一回调），落到 conversation-home，输入框已预填需求；用户选模型/模式后点「发送」→ `startMulticaConversationRun`（复用本地发送链）。
- **（M5-o）可折叠分区 + 「最近完成」回看 + 按 run 直达**：
  - **可折叠**：workspace 分组 / pinned 失败段 / 「最近完成」段三段均**可折叠**（镜像本地 `ConversationSidebar` 的 `expandedWorkspaces` state + `ChevronDown` 旋转 + `<button>` header + 条件渲染；不用 shadcn Collapsible，跟随本地侧栏手动 toggle 风格）。
  - **「最近完成」段**：workspace 分组下方新增可折叠分区，遍历 `vm.recentlyCompleted`（数据源 `state.multica_completed_tasks`，§2.2）；行显示 title + status badge + completedAt，点击 → `onSelectRun(projectId, localTaskId, runId)` 直达本地会话。即任务完成后在远程 tab 可回看（不再「完成后消失」，详见接入方案 M5-o）。
  - **按 run 直达**：`MulticaRemoteTaskList` props 用 `onSelectRun(projectId, taskId, runId)`（复用本地侧栏直达指定 run 的现成回调），「最近完成」行点击 → `onSelectRun(ws.localProjectId, localTaskId, runId)`，绕开「在陈旧 sidebar 快照里查刚创建的任务」的根因（直达会话页）；`ConversationSidebar` 挂载处 `onSelectTask` → `onSelectRun`。
    > **（Req D：执行流程改造）** `handleClaimAndStart` → `handleClaimAndPrepare`：点【执行】不再原子 claim+start+按 run 直达，而是 claim（拿 `requirement`）→ 经 composer draft context 预填正文 + 记下 multica 绑定 → `onNewConversationInWorkspace(localProjectId)`（与本地『+』同一回调，落 conversation-home）。发送时 `App.tsx` conversation-home 的 `onSubmit` 据 `composerDraft.draft.multica` 分流：有绑定 → `startMulticaConversationRun(input, draft.multica.remoteTaskId, draft.multica.workspaceId)`（返回 `ConversationRunVm`），否则本地 `createConversationRun(input)`；二者后续导航/侧栏刷新/reset 完全一致。multica 绑定纳入 draft 生命周期——`reset`（发送成功 / 放弃 compose）即清掉，无需各 reset 点单独清理（根因收益）。composer 草稿 app-root 上提（`ConversationComposerDraftBoundary` 包 `<Shell>`，侧栏与页面均在 boundary 内），故侧栏 `prefill` 后导航到页面仍能读到。
- **（M5-p）任务时间本地时区展示**：pending 行 `lastActivityAt` 与「最近完成」行 `completedAt` 改用既有 `web/src/lib/datetime.ts::formatLocalDateTime(iso)`（内置 Date 解析 ISO/epoch → 本地时区 `YYYY-MM-DD HH:mm:ss`），替换原 `.slice(0,19).replace('T',' ')`（旧实现把 UTC 墙钟当本地时间展示，偏差一个时区）。**UTC 存储不变**（`bridge.rs::finalize_terminal` 存 `Utc::now().to_rfc3339()` canonical 正确），纯展示层转本地。

### 3.5 事件监听 + i18n

- 监听 `gold-band://multica-task-updated`：**任务生命周期**（claim/start/complete/fail/cancel、取消检测作废）→ 远程任务列表 re-fetch（`listen<T>` 参考 desktop.ts:24-51）。由 bridge 终态上报 + loop 取消检测 emit。**（M5-o）App 顶层亦订阅此事件**：multica 任务创建会在本地工作空间落一条会话任务，需同步刷**本地侧栏**（`getConversationSidebar`+`applyConversationSidebar`，in-flight/pending 去抖，对齐 agent-registry 模式）——否则 multica 路径不像正常 `createConversationRun` 那样手动 refresh 本地侧栏，导致 multica 任务在本地侧栏不出现/状态不更新（「会话看不到」根因之一）。远程任务列表与 App 顶层**各订阅一份**（远程列表刷 `getMulticaTasks`，App 顶层刷本地侧栏，职责不同）。
- 监听 `gold-band://multica-settings-updated`：**连接/工作空间配置变更**（connect/disconnect/save/add/rebind/remove/set_active）→ 任务列表 **+ 设置页** 都 re-fetch。任一处发起的配置改动，两端即时同步（杜绝「绑定发生在任务列表弹窗、设置页显示旧数据」之类的跨视图不一致）。
- 监听 `gold-band://multica-runtime-status`：连接状态、register 结果提示。
- i18n（`web/src/i18n.ts`）：新增 `settings.multica.*` 与 `tasks.remote.*` 中英文 key（CLAUDE.md 维护双语）。i18n key 命名参考 `settings.metrics.*`。**前端按错误码 code 查文案**，后端不返回对客文案。

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
                 └─ 【添加工作空间】：从下拉列表选一个远程 workspace + folder picker 选本地目录（非 git 警告不阻断）+ 选 provider（默认 claude-acp）绑定 → register（取回 runtime_id）→ 写入 workspaces（带 local_project_id + provider，本地目录同步进 conversation_workspaces）；首个 workspace 自动设 active
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

> **（Req D：执行流程改造）** 不再「点【执行】→ 原子 claim+start+拉起会话」。改为**点击即领取(claim-at-click)**：点【执行】→ claim 拿 requirement → 预填 composer（与本地『+』同一界面，唯一区别是输入框已预填需求正文）→ 用户选模型/模式 → 点「发送」才真正建会话执行。compose 期间常驻循环续期 prepare lease（45s）防回收；放弃 compose → `cancel_multica_prepare_lease`（或 45s 自然过期回收）。

```
用户在远程列表点某条【执行】  ← claim-at-click（Req D）
  ├─ claim_multica_task(task_id, workspace_id) : POST /runtimes/{rid}/tasks/{tid}/claim {prior_session_id?}  ← selective claim
  │     → {task:{id, issue_id, auth_token, ... + 来源字段}}；命中本地 multica_task_conversations[tid] 时带 prior_session_id 续跑
  │     → 后端回填 VM.requirement（来源优先级取首个非空）；写入 state.prepare_leases[tid]（让常驻循环续期 45s lease）
  │     → 前端：composerDraft.prefill(requirement ?? title, {remoteTaskId, workspaceId, localProjectId})
  │              → onNewConversationInWorkspace(localProjectId)（与本地『+』同一回调）→ 落 conversation-home（composer 已预填）
  │       （**不写 pending_issues**——其语义为「失败待重试」，改由下方 Failure 分支写入，见 §2.2.3）
  │
  ├─ [compose 期间] 常驻循环每 15s extend_prepare_lease(tid)（防 45s 回收，见 §2.6）
  │   放弃 compose → cancel_multica_prepare_lease(tid)（移除 lease）；或 45s 自然过期 → server 回收回 queued
  │
  ├─ 用户点「发送」→ start_multica_conversation_run(input, remote_task_id, workspace_id)
  │     ├─ validate_conversation_create_vm(&app, &input)（与本地同一校验）
  │     ├─ create_conversation_run_vm(&app, &input)（**复用本地链**：建工作流→create_task_from_requirement(requirement)→
  │     │      写 conversation.json→拷附件→run_start_background，requirement 作首轮 prompt=requirement.md，不走 submit）
  │     ├─ multica 叠加：register_active_run(state, run.id, remote_task_id, workspace_id, issue_id, title)
  │     │      + 落 multica_task_conversations[tid]（session_id=None 待 bridge 回填）+ client.start_task(tid)（dispatched→running，lease 移除）
  │     └─ 返回 ConversationRunVm（前端按本地会话同一回调导航 conversation-run；写本地侧栏）
  │     NodeCompleted 后 worker_ref_show 读 continue_ref.acpSessionId → pin_task_session + 回填 multica_task_conversations[tid]
  ├─ 执行期：常驻心跳每 15s POST /heartbeat {runtime_id}（**Req D：已常驻，非仅执行期**；维持在线，否则 150s 判离线→fail）
  │          本期只基础状态（start/complete/fail + 心跳），不上报 step/total 进度
  │          周期 GET /tasks/{tid}/status 检测取消（cancelled/failed/404 → 中断本地 run）
  └─ run outcome（**首轮 run 终结即触发，单轮即完成**；码灵不在单 remote task 多轮，多轮走新 task 续跑同 session，见 4.4）。订阅器穷举 4 分支（见 2.5 终态表）：
       RunCompleted{Success} → POST /tasks/{tid}/complete {output, session_id=ACP session_id, work_dir}（重试幂等）→ 清 multica_task_conversations[tid]
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

断点续跑（runtime_recovery / runtime_offline 重派回本机，同 task_id）
  ├─ claim 时 multica_task_conversations[tid].session_id 命中 → claim 带 prior_session_id
  ├─ **单一执行入口**（Req D：改名 `start_multica_conversation_run`，原 `start_multica_remote_task`）：
  │     复用 `create_conversation_run_vm` 建 task+run 前先 `classify_resume(Option<&MulticaTaskConversation>)`
  │     分支——命中先前未终态 local run（local_task_id+local_run_id 非空）→ Resume；否则 Fresh
  ├─ Resume：run_continue_background_with_config_overrides(local_task_id, local_run_id, None,None,[],None,None)（内部读 worker-ref continue_ref，session/load 续跑同一会话）
  └─ session 已死（strict_continue 报错 acp/client.rs:2132-2141）或任何 resume Err → fresh fallback：
        复用 create_conversation_run_vm 建 task+run + start_task(force_fresh_session=true)（新本地 task，整任务重跑）
```

> **⚠️ 远程 fail 本地作废的「failed」歧义（M4-d 决策，偏离设计字面）**：
> `GET /tasks/{id}/status` 返回 `failed` 无法区分 **retryable**（resume-safe，server 重派带 prior_session_id 断点续跑）与 **terminal**（agent_error，应作废）。若一律作废会丢失续跑索引、击穿断点续跑。决策：
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
- remote `failed`（recover-orphans 后，retryable）↔ 本地会话**暂不作废**（保断点续跑索引，等 server 重派带 prior_session_id 续跑）；仅 remote `cancelled`/404（C2 启动 reconcile）/ 在飞 active_run 的 `failed`+`cancelled`+404（C3 周期检测）→ 作废本地 run（4.4 ⚠️）

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
| **M5** ✅ | 前端 | ① API 四层 + types ② 设置页 multica 区块 ③ ConversationSidebar 远程任务列表切换 ④ 事件监听 ⑤ i18n 双语 | **✅ M5-a/b/c/d/e/f 全过** —— M5-a API 四层（client.ts/desktop.ts/browser.ts/api.ts）新增 13 + 1 个 multica 方法（subscribeMulticaTaskUpdates 为 M5-b 事件监听新增）含 `startMulticaRemoteTask` workspace_id 参数修复（原签名缺失该参数）；types.ts 新增 6 个 VM 类型（`MulticaSettingsVm`/`MulticaServerWorkspaceVm`/`MulticaWorkspaceRefVm`/`RemoteTaskVm`/`RemoteConversationSidebarVm`/`MulticaSettingsVm`）。M5-b 后端事件 `gold-band://multica-task-updated`（unit payload）经 bridge.rs 终端态 + loop_.rs 作废路径 emit。M5-c workspace CRUD 5 命令（`list_server_multica_workspaces`/`add_multica_workspace`/`rebind_multica_workspace`/`remove_multica_workspace`/`set_active_multica_workspace`），复用 `project_id_for_workspace`+`project_ids_match` 去重，slug=id 兜底。M5-d i18n 双语 6 组 key（`settings.multica.*` zh-CN/en 各 22 key + `conversation.sidebar.multica.*` 各 8 key + `errors.multica.*` 各 12 key，覆盖全部错误码 + workspace-already-bound/workspace-not-found）。M5-e `MulticaSettingsBlock` 自管理组件（barrel API 直连，provider 常量选项 claude-acp/codex-acp，连接/保存/workspace CRUD 全 inline，enable toggle 复用 ui/switch），SettingsPage 以 `<SettingsSection><MulticaSettingsBlock /></SettingsSection>` 嵌入 advanced tab。M5-f `MulticaRemoteTaskList` 自管理组件（getMulticaTasks + subscribeMulticaTaskUpdates 事件 → refetch，claim+start→navigate，cancel/rerun→barrel API，not connected 空态卡片），ConversationSidebar 新增 本地/远程 segmented toggle（button cva compose）→ 按 remoteView 条件渲染 ScrollArea；**49 multica 后端单测全过**，tsc 零 multica 新增错误。**M5-g/h/i（后续完善）**：M5-g 登录落点改 302→multica web 根（`callback_redirect_response` 纯函数，不渲染登录结果页）；M5-h 添加工作空间弹窗（远程工作空间 + provider 下拉 + `pickLocalDirectory` 共享原语 + 添加）+ 设置页破坏式移除「添加工作空间」行（仅留已绑定管理）；**M5-i 断开连接**：`disconnect_multica`（sync，对称 `connect_multica`）——纯函数 `clear_multica_session` 清 PAT（`connected` 判定依据）保留 daemon_id/workspaces，`MulticaRuntimeState::clear_runtime_ids` 清 register 缓存（active_runs 保留），connect/disconnect 均 emit `multica-task-updated` 让会话侧栏 re-fetch 同步；设置页 multica 区块【断开连接】按钮（仅已连接态，连接按钮文案切「重新连接」）。**52 multica 后端单测全过**（+clear_multica_session/clear_runtime_ids 各 1），tsc 零错误，816/817 前端用例过（1 既有 scrollbar CSS 失败与 multica 无关）。**M5-j 添加工作空间弹窗布局修复**：`MulticaAddWorkspaceDialog` 原用 shadcn `DialogContent` 默认 `grid` 无高度上限，长路径/多字段下 footer（【添加】）被固定居中布局顶出视口、路径文本横向溢出。改为 `flex flex-col max-h-[85vh] overflow-hidden`（twMerge 干净覆盖 `grid`）+ header/footer `shrink-0` + 中段 `min-h-0 flex-1 overflow-y-auto`，路径行按钮 `shrink-0` 配 `min-w-0 flex-1 truncate` span——footer 始终可见、路径省略号截断。同时补空状态自诊断：数据链路已验证正确（`GET /api/workspaces` 裸数组 `[{id,name,...}]` 与 `WorkspaceInfo{id,name}` 字段匹配，反序列化不报错；空列表即 server 返 `[]`），下拉为空时按 `serverWorkspaces.length===0`（去 multica Web 创建）vs `available.length===0`（全部已绑定）两态提示，不再静默空列表（i18n `noServerWorkspaces`/`allWorkspacesBound` 双语）。**M5-k 绑定后任务列表/设置页不显示 + 即时可用**（三缺陷同修）：① **渲染 bug**——`MulticaRemoteTaskList` 原 `hasAnyTasks` 总开关 + 组内 `if(!tasks.length)return null` 把「已绑定但暂无任务/未 register」的 workspace 整组隐藏，回退到「暂无会话」；改为始终按 `vm.workspaces` 成组展示，空组内显「暂无任务」提示（i18n `noTasksInWorkspace`），仅「无任何绑定」才 `noWorkspacesBound`。② **设置页不刷新（非数据 bug）**——`get_multica_settings` 与 `get_multica_tasks` 读同一份 RuntimeConfig，下拉能过滤已绑定 workspace 即证数据已落；设置页「显示空」纯因只 mount 时 fetch 一次、不订阅事件。新增 `gold-band://multica-settings-updated` 事件（语义=连接/workspace 配置变更，区别于任务生命周期的 `multica-task-updated`），connect/disconnect/save/add/rebind/remove/set_active 统一 emit（connect/disconnect 从 task-updated 迁移至此），任务列表（订阅 task+settings 两事件）+ 设置页（订阅 settings）任一处改动两端同步 re-fetch。③ **register-on-add**——register 原仅启动全量跑一次，`add_multica_workspace` 只落配置不 register → 绑定后须重启才有 runtime_id、任务拉不到/不能 claim；改为 `add_multica_workspace` 改 async，绑定后即时 `register_workspace_best_effort`（复刻 loop 单 workspace 注册，取回 runtime_id 缓存 SharedMulticaState，失败非致命启动 loop 兜底）实现「绑定即可用」。workspace CRUD 命令签名加 `app_handle: AppHandle`（+ add 加 `shared`）供 emit/register。验证：cargo check 过 + 52 multica 单测全过，tsc 零错误，新增 `multica-remote-task-list.test.tsx` 3 用例（空任务显 workspace 组 / 未连接显连接入口 / 订阅 task+settings 两事件）+ 既有 dialog 2 用例全过（共 5）。**M5-l 登录账号可见性 + 切换账号逃生口**（浏览器 cookie 账号歧义，码灵侧 Layer 1）：码灵认证委托给浏览器、cookie 不受控，若浏览器已登账号 B 而用户想登 A，webank 见 cookie 即签 B 的 JWT，码灵静默连错；原 `connect_multica` 更丢弃 `UserInfo{email}` 且无账号字段，连错也看不出。根因（Layer 2，webank `/login` 带 cli_callback 时显式 OAuth consent 屏）属独立 multica-webank 仓库本轮不动；码灵侧做 Layer 1：① 新增 `MulticaAccountRef{name,email}`（与 PAT 同生命周期单结构体），`connect_multica` 捕获 UserInfo 落盘 `desktop_multica_account`、`MulticaSettingsVm` 暴露 `connectedAccount`（仅展示非凭证）、`clear_multica_session` 对称清；设置页连接时显「已连接账号：{email}」。②【切换账号】按钮复用 `openExternalUrl(appUrl)` 打开 multica Web（浏览器登出/换号后回此重连，诚实标注码灵无法强制登出）。验证：cargo check + lib config 35 测（含 account roundtrip）+ desktop multica 52 测（含 clear 清 account）全过，tsc 零错误，新增 `multica-settings-block.test.tsx` 3 用例（连接显账号+切换按钮 / 未连接不显 / 点切换按钮 openExternalUrl(appUrl)），i18n 双语补 `connectedAccount`/`switchAccountHint`|

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
- **断点续跑**：claim 带 prior_session_id 命中→strict_continue 续跑；session 已死→降级 force_fresh_session 整任务重跑；`multica_task_conversations` complete 后清条目。

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
| `src-tauri/src/conversation_workspace.rs` | **复用（不改）**：`workspace_entry_for_project`:24 供 multica 按 local_project_id 查 workspace_path；`add_conversation_workspace`(commands_conversation.rs:917) 复用为添加绑定目录 |

### 10.3 前端（M5）
| 文件 | 变更 |
|---|---|
| `web/src/api/client.ts:126` | RuntimeApi + 12 方法签名 |
| `web/src/api/desktop.ts:248-253` | desktopApi + 实现 |
| `web/src/api/browser.ts` | + mock |
| `web/src/api.ts:300-302` | + barrel wrapper |
| `web/src/types.ts:58` | + MulticaSettingsVm / MulticaWorkspaceRef / RemoteTaskVm |
| `web/src/pages/SettingsPage.tsx:487-523` | + multica SettingsSection（复制 metrics 结构） |
| `web/src/components/conversation/ConversationSidebar.tsx` | + 远程任务列表切换（本地/远程小按钮） |
| `web/src/i18n.ts` | + settings.multica.* / tasks.remote.* 双语 |

---

## 11. 风险与开放问题

| # | 风险/问题 | 应对 |
|---|---|---|
| 1 | **浏览器登录依赖 multica Web 可达**：localhost callback 要求 `<MULTICA_APP_URL>` 浏览器可访问；Web 不可达则无法登录 | 确保企业内 multica Web 可达；登录失败给明确网络提示。复用原生邮箱登录，**无新增 server 信任通道**（无内网冒充暴露面） |
| 2 | **selective claim rebase 成本**：与 ClaimTaskByRuntime 收尾链强相关 | handler 顶部注释标注复制源 commit；升级时 diff 该锚点（见 1.3） |
| 3 | **两套心跳并存**：metrics 15min + multica 15s | 独立循环，互不干扰；**multica 心跳 Req D 起改为常驻**（建立连接后对全部已连接 workspace 的 runtime 持续，非仅执行期）；同一常驻循环还承载 claim-at-click 后的 prepare-lease 续期 |
| 4 | **PAT 明文存储** | 项目无 keyring，与 metrics API Key 一致；永不回显明文（pat_set）；换机器/账号强制重连不复用 PAT |
| 5 | **multica 版本升级** | M0 唯一改造（selective claim）为独立增量，rebase 成本可控（见 1.3）；登录无 server 改动零 rebase |
| 6 | **接受 multica auto-retry + 断点续跑** | resume-safe 失败（runtime_recovery 等）server 自动重试 max_attempts=2，重派带 prior_session_id 续跑；resume-unsafe（agent_error）用户 rerun；session 已死降级整任务重跑（4.4） |
| 7 | **终态重试是新增逻辑** | metrics 为 fire-and-forget，multica 终态必须送达 → client.rs 自建退避重试（2.3），是本模块相对 metrics 的主要新增 |
| 8 | **multica fail 与本地会话 Paused 状态张力** | remote_task fail 后本地会话作废不 continue；断点续跑走 prior_session_id 而非本地 Paused continue（4.4 厘清） |
| 9 | **ACP session 跨重启可恢复性未验证（恢复断崖）** | 「多轮=task序列+session续跑」依赖 ACP provider（claude-acp）真持久化 session、能 `session/load`。未验证。**M1 第一个集成测试验证**：mock requirement 跑完采 session_id → 重启 → run_continue 带 prior_session_id → 确认 session/load 成功、上下文连续。若 claude-acp 不持久化，多轮降级为每轮新会话（功能降级，非阻塞） |
| 10 | **run 终态 4 分支 + Paused 盲区**（本期补齐） | 订阅器必须穷举 Success/Failure/Killed/Paused（2.5 终态表），不能只 match success；Paused 期间 multica 继续显示 running（接受盲区）；不监听未实现的 AcpTurnFinished |

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
