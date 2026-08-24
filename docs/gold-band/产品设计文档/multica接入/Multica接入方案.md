# 码灵客户端接入 Multica 方案

> 本文是把码灵客户端接入 Multica 的完整方案，覆盖原理、目标形态、改造/实现清单与接口文档。
> 所有接口字段与机制结论均来自 Multica 源码核实，标注了关键源码位置。
>
> **本文结构**：
> 1. **Multica 基本原理** —— 三段式架构与码灵客户端的定位、核心实体、PAT 认证、超时与重试模型（聚焦本次接入）。
> 2. **码灵客户端接入后的流程** —— 部署形态与目标产品形态（按钮拉列表 → 人工挑选 → 执行）。
> 3. **接入需要改造和实现的部分** —— server 端唯一改动（selective claim 接口）+ 码灵客户端实现清单。
> 4. **接口文档** —— 全部相关接口（含新增接口规格）+ 完整时序图 + 关键源码位置。

---

## 1. Multica 基本原理（针对本次接入）

### 1.1 三段式架构

Multica 是 AI 原生的任务管理/分发平台，标准架构是三段：

```
Server（任务管理/调度/超时扫描）  ──分发──▶  Daemon（拉取/回传桥）  ──spawn──▶  Agent CLI（真正干活）
```

- **Server**：云端，负责任务队列、状态机、WebSocket 唤醒、超时扫描、Web 管理界面。
- **Daemon**：每台执行机器上常驻或按需启动的进程，负责 **claim（领取）任务、执行、回传结果**。
- **Agent CLI**：被 daemon 拉起的子进程（如 Claude Code、Codex），**自己不拉取任务**。

### 1.2 关键定位：我们的码灵客户端 = Daemon 角色

我们的码灵客户端形态是「每台开发者主机一个本地客户端，自己拉取/执行/回传」——这**精确等于 Daemon 的职责，不是 Agent CLI**。

**当前选择的接入路线是：让码灵客户端扮演 Multica 的自建 Daemon**——直接调用 Multica 的 `/api/daemon/*` HTTP API，自己掌控拉取与回传。**不需要改造码灵客户端去说 stream-json / codex / ACP 等子进程协议**，内部执行方式自定义。

### 1.3 四个核心实体（接入必懂）

| 概念 | 是什么 | 谁创建 |
|---|---|---|
| **runtime** | 「这台机器能执行任务」的执行目标实例（`agent_runtime` 表一行） | 客户端 **register** 时由 server 创建 |
| **agent** | workspace 里的「智能体」配置：名字、指令、模型、**绑定的 runtime** | 管理员在 **Web UI** 创建 |
| **task** | 一次具体任务执行（`agent_task_queue` 表一行），指向某 agent | issue 指派给 agent 时由 server 生成 |
| **issue** | 工作项（看板上的卡片），可指派给 agent | 用户在 Web UI 创建 |

**任务路由链路**（关键）：

```
客户端 register ──▶ runtime 行（你的 runtime_id）
                              │
管理员 Web UI 建 agent ──▶ agent.runtime_id = 你的 runtime_id   ← 这步叫「绑 agent」
                              │
issue 指派给该 agent ──▶ server 生成 task，路由到 agent 的 runtime
                              │
客户端 claim ──▶ 领到 task（能领到 = 你已被某 agent 绑定）
```

> **任务按 `runtime_id` 路由**：SQL 是 `WHERE runtime_id = ANY(...) AND status='queued'`，按 runtime 实例路由、不按 provider。要让任务派到你这台机器，**必须**在 Web UI 上建 agent 并绑定你的 runtime_id。绑了 agent 之后，你的 runtime 行**永远不会被 GC**（GC 条件排除被 agent 引用的 runtime），runtime_id 长期稳定。

### 1.4 认证：PAT

| 项 | 说明 |
|---|---|
| PAT（`mul_...`） | user 级访问令牌，daemon 与 server 之间的长期凭证 |
| server 端 | **只存 SHA-256 哈希 + 前12字符前缀，不存明文**；创建时明文只在响应里返回一次 |
| 客户端 | 存**明文** PAT；每次请求带 `Authorization: Bearer <明文PAT>`；server 收到后现算哈希比对 |
| 过期 | 可空（永不过期）；Multica CLI 浏览器登录默认 90 天 |

> 这是标准 API key 模式：客户端持明文、server 存哈希、传输靠 HTTPS。

### 1.5 超时与重试模型（决定失败恢复设计）

Multica server 有一个后台 sweeper（每 30s 扫一次），相关阈值**全部是 Go `const`，无运行期配置项（env/配置文件/DB settings），只能改源码重编译**。唯一能 env 调的是 `MULTICA_DAEMON_HEARTBEAT_INTERVAL`（客户端心跳间隔，默认 15s，只能调小）。

| 机制 | 阈值 | 后果 |
|---|---|---|
| 不心跳 → 判离线 | 150 s（最迟 ~180 s 生效） | runtime → offline |
| 离线 → fail 在飞任务 | 立即 | 任务 → failed(`runtime_offline`) |
| daemon 重启 → recover-orphans | 无条件、立即 | 残留在飞任务 → failed(`runtime_recovery`) |
| claim 后不 start | 5 分钟 | 任务 → failed(`timeout`) |
| running 卡住（runtime 在线就不死） | 2.5 小时（被在线门控） | `timeout` |
| queued 等待 TTL | 2 小时 | 任务 → failed(`queued_expired`) |
| auto-retry 次数 | 默认 max_attempts=2（首次+1次重试） | 用尽后不再自动重试 |
| 离线 runtime 被 GC | 7 天（无 agent 引用时） | runtime 行删除 |

**失败恢复有两套机制**（接入方案的核心，详见第 2、4 节）：
- **auto-retry**（短期/自动）：fail 后若 failure_reason 命中白名单（`runtime_offline`/`runtime_recovery`/`timeout` 等 6 个），同事务克隆一条 queued 子任务重试（`force_fresh_session=false`，可续旧会话）。受 2h TTL 和重试次数限制。**码灵接受此机制作为兜底**（用户决策，最省接入），重派后码灵 claim 子任务 T'，按响应 `parent_task_id` 反查父任务本地索引走会话级续跑（3.2.7）。
- **issue rerun**（长期/主动）：`POST /api/issues/{id}/rerun` 随时把任务重新入队，**无时间限制、无次数上限**，是我们的长期恢复兜底。

**现象速查：正常关闭码灵后控制台的可观察表现**（把上表几行串成现象链，避免误判为 bug；源码 `server/cmd/server/runtime_sweeper.go`）

关闭码灵客户端（同构于崩溃/断电/强杀）→ 心跳停止 → 控制台依次出现：

1. **runtime 仍显示「在线」约 3 分钟**：这是 `150s 判离线 + 30s sweeper 周期 = ≤180s` 的**设计内检测延迟**，不是 bug。`staleThresholdSeconds=150` 故意大于「DB flush 60s + 心跳 15s + 批量调度 30s = 105s 最坏 DB 滞后」并留 45s 余量（`runtime_sweeper.go:22-31` 注释明述），**调小会把「心跳正常但 DB 批量写未落盘」的健康 runtime 误判离线 → 误杀健康长任务**。
2. **~3 分钟 runtime 翻「失联」**：sweeper 同一 tick 内 `SelectStaleOnlineRuntimes(150)` → 经 Redis liveness 复核（`runtimeLivenessTTL=90s`，key 已过期即确认真死，防 DB 滞后误判）→ `MarkRuntimesOfflineByIDs` 翻 offline；**紧接着同 tick** `FailTasksForOfflineRuntimes`（`runtime_sweeper.go:127/171`）把该 runtime 下所有 dispatched/running/waiting 任务置 `failed(runtime_offline)`——即上表「离线 → fail 在飞任务｜立即」的「立即」=「判离线的同一 sweep tick」。
3. **任务可能始终不显示「失败」**：`runtime_offline` 命中 auto-retry 白名单（`max_attempts=2`），原始 attempt 失败的**同一事务**克隆出 attempt N+1（`force_fresh_session=false`）→ 控制台看到的是「重试中的新 active 任务」而非「失败行」。最终稳定显示 failed 的时机取决于重试落点：
   - 重试 **dispatched 给已离线 runtime** → 下一个 sweep 再次 `runtime_offline` 失败 → 再重试 → 跑满 2 次 → **几分钟内**稳定 failed；
   - 重试 **落回 queued**（无在线 daemon 认领）→ 等 **2h** `queuedTTL` 才 `failed(queued_expired)`（不在重试白名单）→ 停留 failed。

> **结论（是否需要调整）**：码灵客户端**无需**为「3 分钟才失联」做任何调整——150s 阈值是「心跳缺席即失联」架构的下限，对正常关闭/崩溃/断电三种场景通用且鲁棒，调小反而引入误杀。唯一可选优化（**未采纳**）是码灵正常关闭时主动调 server deregister 端点，但仅对「干净退出」一种场景有效（崩溃/断电仍靠 150s 兜底），收益有限、复杂度不划算。

---

## 2. 码灵客户端接入后的流程（目标产品形态）

### 2.1 部署形态

```
┌──────────────────────────────────────────────┐
│            Multica Server（云端，1 台）         │
│   任务队列 / 状态机 / 超时扫描 / Web 管理界面    │
└──────────────────────┬───────────────────────┘
                       │ HTTP（PAT 鉴权）
         ┌─────────────┼─────────────┐
         ▼             ▼             ▼
    开发者 A        开发者 B       开发者 C
   码灵客户端      码灵客户端      码灵客户端    ← 每台一个前台客户端（Daemon 角色）
```

码灵客户端是**单前台进程**：不常驻保活、不空闲轮询、不拆分接入层/执行层。与 multica 建立连接后维持**常驻心跳**（15s，Req D），不再仅任务执行期间。

### 2.2 一次性准备（每台机器/每个开发者一次）

1. **登录拿 PAT**：开发者首次启动客户端，点【连接 Multica】弹浏览器完成 multica 原生邮箱登录（localhost callback，见 3.2.6）→ 客户端获得 PAT 并本地持久化。
2. **首次 register**：客户端生成 `daemon_id`（UUID），调 register → server 返回 `runtime_id`。
3. **建 agent 绑 runtime**：管理员/开发者在 Multica Web UI 上建一个 agent，把它的 runtime 选成刚 register 出来的 runtime_id。（这一步在 Web UI 做，客户端无感知。）

完成后本地持久化：**PAT、daemon_id、已添加 workspace 列表（每条只含 `provider`，**不再绑定本地目录**——见下「绑定模型下沉」）、active workspace、默认 provider**。**`runtime_id` 不必落盘**——register 是幂等的：启动时遍历已添加 workspace 全量 register、新添加 workspace 时 register 该 workspace，都带上同一个 `daemon_id`，server 命中同一行返回**同一个 `runtime_id`**（同一 workspace 永远稳定），所以现取即可（详见 3.2.6）。

> **绑定模型下沉到任务级（当前模型，M5-z）**：远程 workspace **只绑 provider**，**不再绑本地目录**——本地工作目录推迟到**每次执行时**由 composer 下拉选择，选中的本地工作区在发送时随 `start_multica_conversation_run` 写入任务级生命周期结构（`ActiveRemoteRun.local_project_id`、`MulticaCompletedTask.local_project_id`）。即绑定从「workspace 级」下沉到「task 级」：同一个远程 workspace 可在不同本地目录重复执行（每次执行独立落地）。原 workspace → `local_project_id` → `workspace_path` 的 `binding_for_multica()` 查找函数已删除；执行/作废路径改为直接用任务级 `local_project_id` 经 `workspace_entry_for_project(&home_state, &local_project_id)` 解析路径。

### 2.3 日常使用流程（产品形态）

客户端是个 GUI 前台应用，核心交互是「**按钮拉列表 → 人工挑选 → 执行**」：

```
① 启动客户端
   ├─ register 所有已添加 workspace（幂等，取回各 runtime_id；每次启动都做，无感）
   ├─ recover-orphans（自动收尾上次半途崩溃的任务 → runtime_recovery；属 resume-safe，可被 auto-retry 重派或会话级续跑，见 3.2.7）
   └─ 若本地有记录的「未完成 issue」→ 对其调 rerun 重新入队

② 用户点【拉取任务】按钮
   └─ GET /tasks/pending（只读）→ 展示该 runtime 名下的 queued 任务列表

> ⚠️ **下方 ③~④ 为 M5-q 的 claim-at-click 原始设计记录，已被 M5-ak（开发设计 §12.23）claim-at-send 取代**——点「执行」现只读拉需求正文（任务仍 queued），claim+start 推迟到点「发送」，prepare-lease 续期整条删除；放弃 compose 不再调 `cancel_multica_prepare_lease`（删 chip 纯本地解绑）。判定以 M5-ak 为准。

③ 用户在列表里点某条【执行】（Req D：claim-at-click，不再立即拉起会话）
   ├─ selective claim 该任务（按 task_id 领取那一条）→ status: dispatched（含 prepare lease 45s）；claim 回 requirement
   ├─ 进入与本地「+」相同的 composer/prepare 页，输入框预填该任务 requirement；compose 期间常驻循环续期 prepare lease（防 45s 回收）
   ├─ 用户选模型/模式后点【发送】→ start_multica_conversation_run：复用本地 create_conversation_run_vm 构造会话 → start → status: running
   ├─ 执行（常驻心跳维持在线；UI 显示进行中）
   └─ 完成 → complete（或失败 → fail）；放弃 compose → cancel_multica_prepare_lease，任务回 queued

④ 中途关闭客户端（可接受）
   └─ 任务 fail(runtime_offline，resume-safe) → auto-retry 重派 或 用户点【继续】走会话级续跑(session/load，接着上次会话内容) → 仅 agent_error 等才需【rerun】重跑
```

**特点**：
- **不轮询**：任务列表由用户点按钮主动拉取，不做定时轮询。
- **不保活**：空闲时关闭客户端无影响（任务在 server 队列里等）。
- **可中断可续**：执行中途关闭 → fail(`runtime_offline`) → auto-retry 重派或用户【继续】走会话级续跑（接着上次会话内容）；仅 `agent_error` 等才需【rerun】重跑（3.2.7）。
- **常驻在线（Req D）**：与 multica 建立连接后即持续维持心跳（15s，对所有已连接 workspace 的 runtime），不再仅 claim->complete 执行期；否则 150s 后被判离线导致 fail。

---

## 3. 接入需要改造和实现的部分

### 3.1 Server 端改造（需要改源码的部分：仅 selective claim）

**改造目标**：Multica 现有 claim 只能按优先级 FIFO 自动派下一条、不能指定 task_id。为支持「用户在列表里点哪条领哪条」，需新增一个 **selective claim** 接口。

> 除 selective claim 外，其余需求（登录、任务列表、失败恢复、注册心跳、PAT 获取）都复用 Multica 现成接口，**无需改源码**。登录直接复用 Multica 原生邮箱登录（浏览器 localhost callback，见 3.2.6），server 侧零改动。

**改动清单**（4 个文件 + 1 次 sqlc 重新生成）：

| 文件 | 改动 |
|---|---|
| `server/pkg/db/queries/agent.sql` | 新增一条 SQL query `ClaimSpecificQueuedTask`（见下） |
| `server/internal/service/task.go` | 新增 service 方法 `ClaimSpecificTask(ctx, runtimeID, taskID)`：在事务内调上述 SQL，校验返回行 runtime_id 匹配，生成 task token、走 `FinalizeTaskClaim`（复用现有逻辑） |
| `server/internal/handler/daemon.go` | 新增 handler `ClaimSpecificTask`：`requireDaemonRuntimeAccess` 校验 → 调 service → 复用现有 `buildClaimedTaskResponse` 构造响应 |
| `server/cmd/server/router.go` | 在 daemon 路由组注册：`r.Post("/runtimes/{runtimeId}/tasks/{taskId}/claim", h.ClaimSpecificTask)` |
| 代码生成 | `make sqlc` 重新生成 query 代码 |

**新增 SQL 示意**（参考现有 `ClaimAgentTask`，`agent.sql:508`）：

```sql
-- name: ClaimSpecificQueuedTask :one
UPDATE agent_task_queue
SET status         = 'dispatched',
    dispatched_at  = now(),
    prepare_lease_expires_at = now() + make_interval(secs => @prepare_lease_seconds)
WHERE id = @task_id
  AND runtime_id = @runtime_id
  AND status = 'queued'
RETURNING *;
```

**handler 示意**（参考现有 `ClaimTaskByRuntime`，`daemon.go:2512`）：

```go
func (h *Handler) ClaimSpecificTask(w http.ResponseWriter, r *http.Request) {
    runtimeID := chi.URLParam(r, "runtimeId")
    taskID := chi.URLParam(r, "taskId")
    // 1. requireDaemonRuntimeAccess(w, r, runtimeID) 校验 runtime 归属
    // 2. task, err := h.TaskService.ClaimSpecificTask(r.Context(), runtimeID, taskID)
    //    （任务不存在/不属该 runtime/非 queued → 返回 404/409）
    // 3. 复用 buildClaimedTaskResponse(w, task) 返回完整 Task（含 auth_token）
}
```

> 实现时直接对照 `ClaimTaskByRuntime`（`daemon.go:2512`）和 `ClaimTaskForRuntime`（`task.go:2427`）复制鉴权、token 生成、响应构造逻辑，仅把「FIFO 选下一个」换成「按 task_id 选定」。改动面很小。

**维护成本**：selective claim 是 Multica server 唯一的源码改动。每次升级 Multica 版本时需要 rebase（一个接口、四个文件），可控。登录、PAT 获取、register、recover-orphans、rerun、心跳全部复用 Multica 现成接口，**server 端不再有任何其它改动**。

### 3.2 码灵客户端需要实现的部分

> 码灵（Gold Band）是 Tauri 桌面端：核心库 `gold-band`（`src/`）提供 provider-first / ACP-first 的工作流 runtime（本地模型 `preset→task→run→round→node→attempt` 五层），`src-tauri/` 是薄壳，`web/` 是前端。本节把 multica daemon 的职责落到码灵的具体模块上，明确「复用什么 / 新增什么落哪」，遵循项目「先定数据 → 再定接口 → 再补实现」「错误码用结构体」「配置进 SettingsConfig、杜绝硬编码」等规范。

#### 3.2.1 术语对照（必须先澄清的同名冲突）

multica 的核心实体与码灵自有概念**同名但语义完全不同**，实现时不区分必然混淆。码灵侧统一用 `multica_` / `remote_` 前缀指代 multica 实体：

| multica 实体 | 含义 | 码灵自有同名概念 | 码灵侧命名 |
|---|---|---|---|
| **runtime** | 执行目标实例（`agent_runtime` 一行） | 本地工作流运行时（task→run→round…） | `multica_runtime_id` |
| **task** | issue 派生的远程任务 | 本地工作流任务（preset→task→run） | `remote_task` |
| **agent** | workspace 智能体配置（绑 runtime） | ACP agent / provider（如 claude-acp） | `multica_agent` |
| **workspace** | multica 多租户隔离边界（团队/组织资源池，agent/issue/task/runtime/member 均以其为外键硬隔离） | 本地项目目录（task 执行的 work_dir，码灵会话模式的 `conversation_workspaces` 条目） | `multica_workspaces`（已添加列表，**只绑 `provider`，不绑本地目录**）+ `active_workspace_id` |
| **daemon** | multica 拉取/执行/回传桥 | —— | 码灵客户端本身即扮演此角色 |

> 下文凡指 multica 侧的 runtime/task/agent/workspace，一律加 `multica_`/`remote_` 前缀。

> **workspace 与本地目录的关系（绑定模型下沉到任务级，M5-z）**：multica workspace 是远端 SaaS 多租户边界（决定「任务从哪个团队派来」），码灵本地项目目录是 work_dir（决定「任务在哪执行」）。**两者通过任务级 `local_project_id` 关联，不再在 workspace 级绑定**——添加 multica workspace 时**只选远程工作空间 + provider**（不弹本地目录 folder picker），本地工作目录推迟到每次执行时由 composer 下拉选择，随 `start_multica_conversation_run` 写入该次任务的生命周期结构（`ActiveRemoteRun.local_project_id` / `MulticaCompletedTask.local_project_id`）。同一个远程 workspace 可在不同本地目录重复执行（每次执行独立落地），`multica_workspaces` 列表本身无 `local_project_id` 字段。
>
> **三层 workspace 概念勿混**：① multica workspace（远端，`multica_workspaces` 条目，只绑 provider）；② 码灵本地工作目录（`conversation_workspaces` 条目，`workspace_path`，**执行时**由 composer 下拉选并写入任务级结构）；③ 码灵本地会话实例（一次会话打开的某个本地目录）。本方案中某次 multica 任务的「执行落点」= 该任务 `ActiveRemoteRun.local_project_id` 在 `conversation_workspaces` 解析出的 ② 的 `workspace_path`（不再是 workspace 级固定绑定）。

#### 3.2.2 核心设计决策：remote_task → 码灵会话执行的映射

一个 `remote_task` 被 claim 后，**映射为码灵的一次会话执行**（一个本地 task + run，单节点工作流包一个 ACP session），最大化复用码灵既有的会话生命周期与断点续跑能力，且**只触碰核心库 runtime 的公开 App API**：

> **关键澄清**：码灵库层（`gold-band` / `src/app/`）**不存在「会话 / 工作流」二分**——前端会话模式（新 UI）内部恰恰就是调 `create_task_from_requirement` + `run_start_background`（`view_models_conversation.rs` 的会话 VM 即此实现）。multica 复用同一套库层 API：**首轮 prompt = `requirement`，由 `create_task_from_requirement` 写入 `requirement.md`，不需要 `submit`**。

```
claim remote_task
   │  remote_task 的 requirement / issue 描述  ──作为──▶  本地 task 的首轮 prompt（requirement.md）
   ▼
取 task 所属 workspace 绑定的 workspace_path（见 3.2.6）
   │
App::with_config(workspace_path, config).with_lifecycle_bus(shared_bus)   →  构造绑定该目录的 App 实例
   │
构造 Direct 模式 WorkflowDsl（单 Worker 节点，provider 编码进 NodeDsl::Worker.provider；复用 gold_band::dsl::presets）
   │
app.create_task_from_requirement({ requirement_content: requirement, workflow, .. })   →  本地 task（首轮 prompt 已落盘）
   │
app.run_start_background(&task_id, None)   →  后台起 run（单节点工作流包一个 ACP session，同进程后台线程驱动）
   │
ACP session 建立  →  session_id 落 <attempt_dir>/worker-ref.json 的 continue_ref.acpSessionId
   │  （bridge 订阅 lifecycle，NodeCompleted 后用 attempt locator 调 app.worker_ref_show 读出 session_id）
   │
lifecycle 事件（NodeStarted / NodeCompleted / RunCompleted / RunPaused / InterventionRequested）  ──转译──▶  multica 基础状态（started / 心跳 / completed / failed；Paused 期间 multica 继续 running，见下）
   ▼
run 终局（订阅器穷举 4 分支，源码依据 provider/mod.rs:1086-1100 + control/mod.rs:25-43 + orchestrator.rs:2388-2437）
   ├─ RunCompleted{Success}  →  multica complete(output = 末轮产物摘要, session_id = ACP session_id) + PinTaskSession 写回 session_id
   ├─ RunCompleted{Failure}  →  multica fail(error, failure_reason = agent_error.*)
   ├─ RunCompleted{Killed}   →  通常取消检测已命中 multica cancelled：仅清本地索引不上报；罕见本地强停 → fail(timeout) 兜底
   └─ RunPaused / InterventionRequested  →  不上报终态，转本地 elicitation/permission 处理（multica 继续 running）
```

> **Req D 时序澄清**：上图是 remote_task -> 码灵会话的**数据映射**，不是点击时序。Req D 起 claim 与执行拆分（claim-at-click）：点【执行】只 claim + 预填 composer；用户在 composer 点【发送】才经 `start_multica_conversation_run` 触发上图「构造 App -> create_task_from_requirement -> run_start_background -> start」链（复用本地 `create_conversation_run_vm`）。

**要点**：
- multica 的「执行业务逻辑」= 在绑定的本地目录里跑一次码灵会话执行；「状态上报」= lifecycle 事件转译为基础状态；「完成/失败」= run outcome 转译。不为 multica 重造执行链。
- **bridge 直接调 `gold-band` 库层 App API，不走 Tauri command 层**：会话模式的两个 Tauri command（`create_conversation_run` / `submit_conversation_prompt`）内部只是把库层 API 包了一层 Tauri shell；bridge 同进程直接调库层更干净——**无需伪造 AppHandle、无需前端 AttemptLocator 协议**，`acp_live_update` / `acp_session_update` 转发器留 `None` 即可（对 ACP 执行无影响）。
- **一个 remote_task ↔ 一个本地 task（= 一次执行、一个 run，不是可多轮的会话）**：multica `task_id` 即为天然关联键（无需另造 uuid 映射）；本地 task 在该 task 所属 workspace 绑定的本地目录里创建。**码灵作为 daemon，对一个 remote task 只执行 requirement 这一轮**——首轮 requirement 驱动 run 跑完（`RunCompleted`，runtime-continue 性质、自然终结）即该 task 完成；**不在单个 remote task 上承载多轮追问**（对话发起方是 multica，不是码灵客户端）。
- **「多轮对话」由 task 序列承载（非单 task 多轮）**：用户在 multica web 对同一 issue 发起新需求 → 新 queued task → 码灵 claim 子任务后按响应 `parent_task_id` 反查父任务本地索引，续跑**同一 ACP session**（上下文连续，见 3.2.7）。即「多轮」= 多个 remote task + 同一 ACP session，而非一个 task 内多轮。因此**会话完成判定无歧义**：run 跑完即 complete，不存在「多轮会话何时算完」的问题。
- **run 终态 4 分支（订阅器必须穷举，不能只 match success）**：`RunCompleted{Success}`→complete / `{Failure}`→fail / `{Killed}`→不上报（取消检测命中则清本地索引，否则 fail(timeout) 兜底）/ `RunPaused`·`InterventionRequested`→**不上报终态**，转本地 elicitation/permission 处理。源码依据 `provider/mod.rs:1086-1100` stop_reason 分类 → `node_executor.rs:998-1067` → `control/mod.rs:25-43` → `orchestrator.rs:2388-2437`（详见开发设计 2.5 终态表）。
- **Paused 盲区（已决策：接受，multica 不感知）**：multica task 状态机无 paused 态。码灵本地 run 因 elicitation/permission 进 Paused 时，multica 端继续显示 running、码灵本地全权处理，不主动上报、不超时兜底；代价是发起人此期间在 multica web 仅见 running。
- **不监听 `AcpTurnFinished`**：该事件当前未实现（仅定义于 `app/mod.rs:665`，全库零 emit 点），方案 A 不依赖它（多轮=task序列）。
- **ACP session 跨重启可恢复性未验证**（多轮=续跑前提），列为 M1 首个集成测试验证项（开发设计 9.2）。
- **首轮 prompt 不走 submit**：`create_task_from_requirement` 的 `requirement_content` 直接写入 `requirement.md`，Worker 节点自动读它作为首轮 prompt（`node_executor.rs:610`），无需 `submit_conversation_prompt`。
- **断点续跑** = `app.run_continue_background_with_config_overrides(task_id, run_id, None, None, …)`，内部自动读 `worker-ref.json` 的 `continue_ref` 走 ACP `session/load` 恢复上次上下文（见 3.2.7），不重新建会话。
- **对库的唯一轻微改动**：会话 VM 现有**私有** fn `build_direct_workflow` / `build_auto_workflow`（`view_models_conversation.rs:2579/2461`）上提到 `gold_band::dsl::presets` 公开复用——会话 VM 与 multica bridge 共用同一份 Direct/Auto workflow 构造，杜绝重复造轮子（provider 经此 preset 编码进 `NodeDsl::Worker`）。这是把已有私有逻辑公开，非新造。

#### 3.2.3 模块划分与落点

新增 `src-tauri/src/multica/` 模块（与 `metrics.rs` / `feedback.rs` / `updater.rs` 平级），作为 daemon 桥接层：

| 子模块 | 职责 |
|---|---|
| `multica/client.rs` | reqwest client 封装 + **重试/退避**（项目目前缺失，见 3.2.4）+ 所有 multica HTTP 调用 |
| `multica/config.rs` | PAT / base url / daemon_id / multica_workspaces（已添加列表，**只含 provider，不绑本地目录**）/ active_workspace_id / provider 等配置 VM 与读写 |
| `multica/state.rs` | 运行期状态：per-workspace runtime_id 映射、在飞 remote_task↔本地 task 映射（`multica_pending_issues` 失败待重试 issue 持久化在 StateConfig，非 state.rs 内存）；**执行注入**：claim 后由 composer 下拉选定的本地工作区 → 发送时随 `start_multica_conversation_run` 写入 `ActiveRemoteRun.local_project_id` → 按 `workspace_entry_for_project(&home_state, &local_project_id)` 解析 `workspace_path` → `App::with_repo_root`（参考 `commands_conversation.rs:257`）。**原 workspace 级 `binding_for_multica()` 已删除（M5-z）** |
| `multica/loop.rs` | 心跳循环、**启动全量 register 已添加 workspace / 新添加 workspace 时 register**/recover-orphans/失败任务供 rerun、取消检测轮询 |
| `multica/bridge.rs` | remote_task ↔ 本地 task/run 衔接（**直接调 `gold-band` 库层 App API** + 订阅 lifecycle bus；不走 Tauri command 层） |

**边界**：multica **业务逻辑**（HTTP / 心跳 / claim / 重试 / 状态机）留在 src-tauri 薄壳层（依赖 reqwest、tauri state、AppHandle）；但**会话执行复用 `gold-band` 库层 App 公开 API**（`create_task_from_requirement` / `run_start_background` / `worker_ref_show` / `run_continue_background_with_config_overrides`，均公开），并把会话 VM 现有私有的 Direct/Auto workflow 构造上提到 `gold_band::dsl::presets` 公开复用（库层唯一轻微改动）。multica 不新造 runtime、不碰核心 lifecycle 契约。

#### 3.2.4 分模块实现设计

| 模块 | 现状 / 复用 | 新增 / 改造 | 落点 |
|---|---|---|---|
| **身份与凭证管理** | 凭证持久化走 `SettingsConfig` 明文 JSON（项目无 keyring，与现有 metrics API key 一致）；原子写 `storage::write_json` | 新增字段 `desktop_multica_enabled` / `_base_url`（API 地址）/ `_app_url`（Web 前端地址，浏览器登录用）/ `_pat` / `_daemon_id` / `_workspaces`（已添加列表）/ `_active_workspace_id` / `_provider`；channel config 预填 base/app url 默认值（参考 metrics 模式）；PAT **永不回显明文**，只暴露 `pat_set: bool`（参考 `metrics.rs:88 api_key_set`） | `src/config/mod.rs`、`configs/channels/*.json`、`src-tauri/src/channel.rs` |
| **登录与 PAT 获取** | multica 原生邮箱登录须浏览器；码灵**复用 Multica CLI 同款「浏览器登录 + localhost callback」**（`/login?cli_callback=...`，Web 端 `validateCliCallback` 已放行 localhost / 127.0.0.1 / RFC1918，server **零改动**）；daemon 接口拒 cookie、只认 Bearer → PAT 是唯一持久凭证 | 浏览器邮箱登录 → JWT（callback 回跳）→ PAT（`/api/tokens`），完整子流程见 **3.2.6** | `multica/client.rs`（临时 callback server + 换 PAT + 列 workspace）、前端【连接 Multica】入口组件 |
| **启动流程** | 后台任务启动样板：`start_heartbeat_polling`（`metrics.rs:218`），在 `main.rs` setup 里 `tauri::async_runtime::spawn` | 新增 `start_multica_runtime`：**先 `GET /api/me` 校验 PAT**（有效则复用、进入全量 register；**无效则静默、不弹任何 UI**——等用户主动切到「远程任务列表」、在未连接空状态点【连接 Multica】才触发 3.2.6 登录；**首启绝不自动弹登录**）→ **遍历 `desktop_multica_workspaces` 已添加列表全量 register**（幂等取回各 runtime_id）→ recover-orphans → 失败任务留待用户 rerun；全部异步、失败不阻断客户端启动 | `src-tauri/src/main.rs`（setup 注册）、`multica/loop.rs` |
| **HTTP 客户端与重试** | reqwest 0.12 已引入；但**全仓库无 retry/backoff**（metrics/feedback 均 fire-and-forget） | multica client 自建指数退避重试；终态回调（complete/fail）严格按 multica 协议 4/8/16/32/64s 共 6 次、服务端幂等；client 参照 `feedback.rs:282` builder 模式，PAT 走 `Authorization: Bearer` | `multica/client.rs` |
| **心跳维持** | `start_heartbeat_polling`（15min metrics 心跳）是直接样板 | 新增 multica 心跳循环：**Req D 起为常驻**--连接后每 **15s** 一次 `POST /api/daemon/heartbeat`（≠ metrics 的 15min，两套并存），对所有已连接 workspace 的 runtime 持续心跳（不再仅 claim->complete 期间），否则 150s 后被判离线导致 fail；同 tick 续期 claim-at-click 后未 start 的 prepare lease | `multica/loop.rs`，照抄 `metrics.rs:218` 形态改间隔 |
| **远程任务列表（UI）** | 前端四层封装：`invokeCommand`(`shared.ts:8`) → `RuntimeApi`(`client.ts:126`) → desktop.ts/browser.ts → api.ts；两套 UI（会话=新主入口 / 工作流=旧）；本地会话 `ConversationSidebar` 是「workspace 分组 → 任务 → 末尾【添加工作空间】」骨架（`ConversationSidebar.tsx`，VM 为 `ConversationSidebarVm{workspaces, pinnedTasks, tasksByWorkspace, lastActiveWorkspaceId}`） | **仅会话模式（新 UI）做，工作流模式（旧 UI）暂不管**；**远程任务管理改为独立的「远程任务管理」整页**（`MulticaTaskManagementPage`，新路由 `/chat/multica-tasks`，与 agent 管理/上下文管理/运行模式管理 并列的导航项，icon=Globe），内容镜像 `MulticaRemoteTaskList`——**会话侧栏 (`ConversationSidebar`) 移除「本地/远程」切换 UI，侧栏纯本地任务**（M5-z）；**未连接时（无 PAT / PAT 失效）远程任务管理页显示空状态卡 + 【连接 Multica】按钮**（点击进入 3.2.6 登录；首启不自动触发）；**按 multica workspace 分组**（每个 workspace 按各自 runtime_id + `X-Workspace-ID` 调 `GET /tasks/pending` 过滤 `status=='queued'`，并 join 本地 `multica_pending_issues` 失败任务以 `retryable` 同列混排）；新增命令 `get_multica_tasks`（返回**按 workspace 分组**、对齐 `ConversationSidebarVm` 形状的 VM） | `web/src/api/{client,desktop,browser}.ts`、`web/src/api.ts`、`web/src/pages/MulticaTaskManagementPage.tsx`（新整页）、`web/src/components/conversation/MulticaRemoteTaskList.tsx`（镜像内容） |
| **任务领取** | -- | 调 3.1 新增的 selective claim（按 task_id 领取用户点的那条）；新增命令 `claim_multica_task(task_id, workspace_id)`（需 workspace_id 解析 runtime_id；命中 `multica_task_conversations[task_id].session_id` 带 prior_session_id 续跑）；**claim 不写 pending_issues**--失败回显改由 M4 终态 fail 写入（见状态持久化行）；**Req D：claim-at-click--只 claim + 写 prepare_lease + 回 requirement，不立即 start**（start 移到用户在 composer 点【发送】时的 `start_multica_conversation_run`） | `multica/client.rs`、新增 Tauri 命令 |
| **任务执行（核心）** | 码灵库层 App API（`gold-band` / `src/app/`）：`App::with_config`+`with_lifecycle_bus` 构造 App -> 构造 Direct 模式 WorkflowDsl（复用 `gold_band::dsl::presets`）-> `create_task_from_requirement(content=requirement)` -> `run_start_background`；首轮 prompt = requirement.md，**不走 submit**；session_id 落 `worker-ref.json` 的 `continue_ref.acpSessionId`（NodeCompleted 后 `worker_ref_show` 读出） | **Req D：claim-at-click 拆分**。点【执行】只 `claim_multica_task`（claim + 写 prepare lease + 回 requirement）；用户在 composer 点【发送】才调 `start_multica_conversation_run`：**复用本地 `create_conversation_run_vm`** 构造会话（requirement 作首轮 prompt）+ `POST /tasks/{id}/start` -> running + 订阅 `RuntimeLifecycleBus`（NodeStarted/NodeCompleted/RunCompleted）转译为基础状态 + NodeCompleted 后 `worker_ref_show` 采集 session_id -> run outcome 转译为 complete/fail；放弃 compose 调 `cancel_multica_prepare_lease`，任务回 queued | `multica/commands.rs`、`multica/bridge.rs`、`multica/loop.rs`（常驻心跳 + lease 续期） |
| **状态上报** | 会话事件流已是结构化的 | **只上报基础状态（最简版本）**：会话 run started → multica started；执行期每 **15s** 心跳 `POST /api/daemon/heartbeat`；run completed/failed → 终态。**本期不上报 step/total 进度**（用户决策：只要基础对应 multica 状态）；订阅器注册参考 `create_metrics_subscriber`(`metrics.rs:365`) | `multica/bridge.rs` |
| **终态回传** | —— | complete(output=末轮产物摘要, session_id=ACP session_id) / fail(error, failure_reason)；终态前先 `PinTaskSession`(`POST /api/daemon/tasks/{id}/session`) 把 session_id 写回 task 行（续跑依赖）；**带 3.2.4 的指数退避重试 + 幂等**确保终态送达；**接受 multica auto-retry**（`max_attempts=2`，runtime_recovery 等可重试原因由 server 兜底） | `multica/client.rs` |
| **失败恢复 / 续跑** | gold-band 已有 `recover_interrupted_running_sessions`(`app/mod.rs:2397-2401`)；库层 `run_continue_background_with_config_overrides` 原生支持 ACP `session/load` 续接（内部自动读 worker-ref.json continue_ref） | ① 启动 recover-orphans 把残留在飞 task **无条件置失败**（runtime_recovery）② **resume-safe 失败**（runtime_offline / runtime_recovery / timeout）→ `run_continue_background_with_config_overrides(task_id, run_id, None, None, …)`，内部 `session/load` 续接（见 3.2.7）③ **resume-unsafe 失败**（agent_error）→ 整 task `POST /api/issues/{id}/rerun` 重新入队（新会话）④ **接受 multica auto-retry 兜底** + 用户可手动【rerun】 | `multica/loop.rs`、`StateConfig` 新增字段、新增 `rerun_multica_task` 命令 |
| **取消检测** | gold-band 已有 stop/pause session 能力 | 长任务执行期周期 `GET /tasks/{id}/status`，发现 cancelled/failed/404 → 中断本地 run（复用 gold-band stop session） | `multica/loop.rs` |

**性能优化（连接池复用 + liveness 短超时 + 自愈单次重试 + tick 埋点）**：上述 client/loop 落地后做了一次系统性性能检视（10 维度），前置确认业务逻辑正确（锁不跨 await、终态严格重试、4xx 不重试、前端纯事件驱动、PAT 不回显），修复两项严重缺陷（实现细节见开发设计 §12.13）：
- **共享 `reqwest::Client`**：进程级 `OnceLock` 单例，`MulticaClient::new` 取廉价 clone——修复「每调用点重建 client → 连接池/TLS 上下文作废 → 每次重做 TCP+TLS 握手」（弱网放大失败率）。`MulticaClient: Clone`。client 级 30s 超时仍在单例内设定。
- **liveness 短超时**（`LIVENESS_TIMEOUT_SECS=10`，per-request 覆盖 client 级 30s）：心跳 tick 内高频调用（heartbeat / extend_prepare_lease / get_task_status / 自愈 register）正常 <1s，server 慢响应时 10s 快速失败、下一 tick 重试；**非 liveness 调用**（claim/start/complete/fail/list/verify 等）仍 30s，可靠性优先。
- **自愈 register 单次重试**（`register_once`）：常驻心跳的自愈注册「循环即重试」，不再嵌套 client 内 3×30s 退避——否则弱网下单 tick 超 90s，推迟 prepare-lease 续期（45s 被 server 回收 → 用户 compose 中的任务丢失，功能受损）与取消检测（远端已取消任务本地空跑）。一次性路径（启动/connect/绑定）仍走带重试的 `register`。
- **tick 耗时埋点**：每 tick 四阶段 + 总耗时（超 30s 升 warn），弱网回归量化依据。

#### 3.2.5 数据 / 接口 / 错误码规范

**SettingsConfig 新增字段**（`src/config/mod.rs`，用户可编辑，设置页读写）：
```jsonc
{
  "desktop_multica_enabled": false,
  "desktop_multica_base_url": "",             // multica server API 根地址（daemon/业务接口）
  "desktop_multica_app_url": "",              // multica Web 前端地址（浏览器登录页 cli_callback 用；常与 base_url 同源，分离配置便于 API/Web 分部署）
  "desktop_multica_pat": "",                  // 明文 PAT，永不回显，只暴露 pat_set: bool
  "desktop_multica_daemon_id": "",            // 本机持久 UUID v7，首次启动生成
  "desktop_multica_workspaces": [],           // 已添加的 multica workspace 列表，**只绑 provider 不绑本地目录**：[{ "id": "<uuid>", "name": "...", "slug": "...", "provider": "claude-acp" }]。本地工作目录在每次执行时由 composer 下拉选并写入任务级结构（`ActiveRemoteRun.local_project_id` / `MulticaCompletedTask.local_project_id`）；provider 选定后不可变（变了=新 runtime=需重绑 agent，见 3.2.6）
  "desktop_multica_active_workspace_id": "",  // 当前查看的 workspace（workspaces 之一）；切换=纯视图，不触发 register
  "desktop_multica_default_provider": "claude-acp",  // 添加 workspace 时的默认 provider 预选项（码灵 ACP provider 标识，如 claude-acp/codex-acp/gemini-acp）；每个 workspace 实际选定的 provider 存在 desktop_multica_workspaces 条目里
}
```

**StateConfig 新增字段**（机器管理，恢复用）：
```jsonc
{
  "multica_runtime_ids": {},                         // { "<workspace_id>": "<runtime_id>" }，每个已添加 workspace register 取回，运行期缓存
  "multica_pending_issues": [ "<issue_id>", ... ],   // 失败待重试 issue（M4 fail 写入 / complete·rerun 清除；claim 不写），以 retryable 标记混排在任务列表供用户点 rerun
  "multica_task_conversations": {                    // { "<remote_task_id>": {local_task_id, local_run_id, acp_session_id, workspace_id, work_dir} }，续跑/取消/状态上报的关联键；acp_session_id 来自 worker-ref.json + PinTaskSession 回写
    "<remote_task_id>": {
      "local_task_id": "...",      // 本地 task uuid（create_task_from_requirement 产出）
      "local_run_id": "...",       // 本地 run id（run_start_background 产出）；run_continue_background_with_config_overrides 续跑需要
      "acp_session_id": "...",     // ACP session_id，续跑用 session/load 恢复；缺失则只能整 task rerun
      "workspace_id": "...",
      "work_dir": "..."
    }
  },
  "multica_completed_tasks": [                        // 终态任务本地历史（M5-o；改动六起不再进扁平「最近完成」桶，而按 workspace_id 并入对应工作空间 tasksByWorkspace 组）；finalize_terminal 移除 active 前快照一行进此（去重最新在前，截断 N=50）；title 快照自 ActiveRemoteRun.title，completed_at RFC3339；**M5-z 起新增 `local_project_id`**（来自 `ActiveRemoteRun.local_project_id`，绑定模型下沉到任务级后任务自带本地目录、不再依赖 workspace 级绑定）
    {
      "remote_task_id": "...",
      "local_task_id": "...",
      "local_run_id": "...",       // 点击行按 (project_id, local_task_id, local_run_id) 经 onSelectRun 直达本地会话
      "workspace_id": "...",
      "local_project_id": "...",   // M5-z 新增：该次执行的本地工作目录（project_id），终态行 onSelectRun 经此解析直达
      "issue_id": "...",           // 可空，rerun 用
      "status": "completed",       // "completed" | "failed"
      "title": "...",
      "completed_at": "..."
    }
  ]
}
```

**错误码**（沿用 `CommandErrorVm { code, params }` 点分式；后端只返回码，不含对客文案）：
```
multica.not-configured          // 未填 base_url / app_url / PAT
multica.auth-failed             // PAT 无效 / JWT 换 PAT 失败
multica.login-callback-timeout  // 浏览器登录 5min 内未回跳 callback（用户未完成登录或关闭浏览器）
multica.login-callback-failed   // callback 回跳但缺 token / Multica Web 拒绝 cli_callback（非白名单）
multica.network-failed          // HTTP 不可达（重试用尽）
multica.register-failed
multica.claim-conflict          // task 已被领 / 非 queued → 409
multica.task-not-found          // 404
multica.runtime-offline
multica.session-resume-failed   // 保留在码表但 M4-d 起不 emit（resume Err 一律 silent fresh-fallback）；见开发设计 §12.29 / M5-aq
```

**Tauri 命令**（新增，注册到 `generate_handler!`，参考 `save_metrics_settings`(`commands.rs:1501`) 风格）：
`connect_multica`（PAT 失效时触发 3.2.6 浏览器登录 + localhost callback + 换 PAT）/ `list_server_multica_workspaces`（拉 server 全量供添加）/ `add_multica_workspace`（**M5-z：签名只剩 `workspace_id + provider`，name 从 server 列表取，无 local_path**；下拉单选添加 + register）/ `remove_multica_workspace` / `set_active_multica_workspace`（纯视图切换，不 register）/ `save_multica_settings` / `get_multica_settings` / `get_multica_tasks` / `claim_multica_task`（Req D：claim-at-click，只 claim + 写 prepare lease + 回 requirement，不立即 start）/ `start_multica_conversation_run`（Req D：复用本地 `create_conversation_run_vm`，发送时调，**M5-z：用 composer 下拉选中的 `project_id` 写入 `ActiveRemoteRun.local_project_id`**，含 start + 会话级续跑 classify_resume 分支）/ `cancel_multica_prepare_lease`（Req D：放弃 compose 时释放 lease，任务回 queued）/ `cancel_multica_task` / `rerun_multica_task`（用户手动整 task 重跑，新会话）。原 `start_multica_remote_task` / `resume_multica_task` 已删除（Req D）；原 `rebind_multica_workspace` 已删除（M5-z，绑定模型下沉到任务级，全链路删除无兼容层）。

**前端事件常量**（加在 `commands.rs:62` 旁，`gold-band://` 前缀）：
`gold-band://multica-task-updated`、`gold-band://multica-runtime-status`。

**channel config**（`configs/channels/*.json`，参考 metrics 四字段模式）：
新增 `multicaBaseUrl`（API 地址，default 预填 `http://localhost:8080` 本地联调 / wb 预填 `http://maling.weoa.com:5005`，nginx 统一入口）、`multicaAppUrl`（Web 前端地址，浏览器登录用，default 预填 `http://localhost:3000` / wb 同 `http://maling.weoa.com:5005`，前后端同源）、`multicaEnabled` 默认开关（default/wb 均 `true`，零配置直连前提）。

#### 3.2.6 登录、PAT 获取与 workspace 选择（首启链路）

> **复用 Multica 原生邮箱登录，不改造 server、不新增任何登录接口**。码灵沿用 Multica CLI 同款的「浏览器登录 + localhost callback」模式（参考 `server/cmd/multica/cmd_auth.go:235-358`）：码灵拉起一个本地临时 HTTP server、用系统默认浏览器打开 Multica 登录页并带上 `cli_callback` 回调地址，用户在浏览器里完成邮箱登录，Multica Web 登录成功后把 token 回跳到码灵本地 server。daemon 接口拒 cookie、只认 Bearer，故登录换回的 JWT 仅用于一次性换 PAT，**长期只用 PAT**。

**触发条件（登录复用）**：每次启动客户端先检查本地 PAT 配置——`GET /api/me` 校验，**有效则直接复用 PAT**（跳过①-⑩，直接进入启动全量 register，见下「workspace」段）；**无 PAT 或校验失败（401）则静默、不弹任何 UI**——等用户主动切到「远程任务列表」、在未连接空状态点【连接 Multica】（或设置页辅助入口）才走下面的浏览器登录连接流程（①-⑩）。**首启绝不自动弹登录**（本地任务列表零变化）。

```
① 用户切到「远程任务列表」（未连接时显示空状态卡）点【连接 Multica】（设置页 multica 区块同步可见，作辅助入口；**首启不自动触发登录**；按钮**直接 `connect_multica` 触发浏览器登录、不跳转设置页**，默认配置由 channel 预填、用户无需调整）
② 码灵随机选一个空闲本地端口 port，起一个临时 HTTP server 监听 127.0.0.1:<port>，
   注册 callback 路径（如 /callback），设超时（如 5min）后自动关停
③ 码灵打开系统默认浏览器到 <MULTICA_APP_URL>/login?cli_callback=http://127.0.0.1:<port>/callback
     → MULTICA_APP_URL 是 Multica Web 前端地址（见 3.2.5，与 API base URL 分开配置）
     → 用户在浏览器用 Multica 原生邮箱登录（邮箱验证码 / OAuth）
④ Multica Web 登录成功后，校验 cli_callback 命中白名单（localhost / 127.0.0.1 / RFC1918，
   见 packages/views/auth/login-page.tsx 的 validateCliCallback），302 回跳：
     http://127.0.0.1:<port>/callback?token=<JWT>
⑤ 码灵本地 server 收到 callback：① 从 query 取出 JWT；② 向浏览器回 **HTTP 302 → `<MULTICA_APP_URL>/`**——multica Web 在回跳 cli_callback **之前**已执行 `onTokenObtained → setLoggedInCookie`（`packages/views/auth/login-page.tsx` 登录成功路径，先 set token cookie 再 redirectToCliCallback），故浏览器被 302 导回 multica web 根时**带着登录态 cookie、直接落在 multica 网页界面（已登录）**，而不是码灵自己渲染一个突兀的"登录成功，请返回码灵"页（码灵客户端**不在浏览器里展示任何登录结果页**）；③ 关停临时 server
⑥ POST <MULTICA_BASE_URL>/api/tokens   Authorization: Bearer <JWT>
     Body: { "name": "Maling Desktop", "expires_in_days": 90 }
     → { "token": "mul_..." }（明文仅此一次）；JWT 用完即弃，不落盘
⑦ GET <MULTICA_BASE_URL>/api/workspaces (Bearer mul_...)  拉取用户在 server 的所有 workspace
     → 为空：前端空态提示先在 multica web 建 workspace（无错误码——empty 由前端空态 UI 守卫，见开发设计 §12.29 / M5-aq）
     → 否则进入【添加工作空间】（M5-z：**只收远程工作空间 + provider 两个下拉**）：**从下拉列表选一个未添加的远程 workspace** → 为其选 ACP provider（默认 claude-acp）→ register（用该 provider，取回 runtime_id）→ 写入 desktop_multica_workspaces（**只带 provider，不带本地目录**——本地目录在每次执行时由 composer 下拉选，见下「执行时落地」）；若为首个 workspace 则设为 active
⑧ 持久化 PAT + workspaces 列表（**只含 provider，不含本地目录绑定**）+ active_workspace_id（SettingsConfig 明文）+ 一个稳定 daemon_id（首启生成 UUID 后持久化，register/心跳/claim 全程复用）
⑨ 新添加的 workspace 已在 ⑦ register；此后每次启动遍历已添加列表全量 register（见下「workspace」段）
⑩ 此后所有请求统一用 Authorization: Bearer <mul_...>
   PAT 临期(<7天)调 POST /api/tokens/current/renew 续期（原串不变，顺延 90 天）
```

**安全边界**：localhost callback 是 Multica CLI 同款机制，回调地址由 Multica Web 白名单校验（仅 localhost / 127.0.0.1 / RFC1918 私网，见 `validateCliCallback`），不会把 token 回跳到任意公网域名；临时 server 仅监听本机环回、登录完成后立即关停。登录本身走 Multica 原生邮箱/OAuth，凭证只在浏览器与 Multica Web 之间流转，码灵只接收一次性 JWT 再换 PAT，**全程不接触用户邮箱密码**。

**workspace：添加 = 选远程 + 选 provider + register（M5-z：本地目录改为执行时选）**：`multica_workspaces` 是用户从 server **主动添加**的 workspace 子集（≠ server 全量），**每条只绑 provider、不绑本地目录**，外加 active 指针。**切换 active 是纯 UI 视图切换，不触发 register、也不切换执行目录**（执行目录由该次任务自己的 `local_project_id` 决定，与 workspace 绑定无关）。register 只在两个时机发生：
- **新添加 workspace 时**（**唯一入口：远程任务管理页的常驻【添加工作空间】**）：① `GET /api/workspaces` 拉 server 全量（去除已添加的）→ **从下拉列表选一个要添加的远程 workspace**（每次只添加一个，可重复操作添加多个，name 从 server 列表取）；② 为该 workspace **选一个 ACP provider**（下拉来自码灵已配置 provider 列表，默认 `claude-acp`；选定后不可变）；③ 写入 `desktop_multica_workspaces`（**只带 `provider`，不带 `local_project_id`**）；④ 用同一持久化 `daemon_id` + 该 workspace 的 `provider` 调 register（幂等，取回 runtime_id 并缓存）。**每个 workspace 恰好一个 provider = 一个 runtime**（要换 provider = 换 runtime 行，需在 web 重绑 agent）。**不再有 folder picker / `pickLocalDirectory` / `rebindMulticaWorkspace`**（M5-z 全链路删除，无兼容层）。
  > **添加入口形态（M5-z 后）**：远程任务管理页（`MulticaTaskManagementPage`）常驻一个【添加工作空间】行（Plus 图标，连接态始终可见），点击弹**模态对话框 `MulticaAddWorkspaceDialog`**——内含「远程工作空间」下拉（`listServerMulticaWorkspaces` 数据源，过滤掉已添加）+「provider」下拉（claude-acp/codex-acp）+ 底部【添加】（两项齐全才可点）；**不再有「绑定本地目录」按钮**。`addMulticaWorkspace(workspaceId, workspaceName, provider)`（三参，无 localPath）。**设置页 multica 区块不再保留添加行**（开发阶段破坏式更新：删除旧入口/旧字段/旧消费路径，不留灰度/fallback），设置页只留配置 + 已添加工作空间的管理（激活/删除；**rebind 已删除**）。仅会话模式（新 UI）有此入口，工作台（旧 UI）不做双胞胎。
- **客户端启动时**：遍历 `desktop_multica_workspaces` 已添加列表，**逐个 register**（全量刷新 runtime_id 缓存，recover-orphans 依赖每个 workspace 的 runtime_id）。
- register 幂等 ⇒ 同一 workspace 无关添加/启动次数，runtime_id 永远稳定：
  - 该 workspace **已绑过 agent** → runtime_id 不变 → **直接复用，无需重绑**；
  - 该 workspace **首次 register**（新 runtime 行）→ 需管理员在 multica web 绑 agent。
- **执行时落地（M5-z 核心机制，claim-at-click → 执行时落地）**：在远程任务管理页点 play → `claimMulticaTask` 领取（后端登记 45s prepare lease，常驻心跳续期）→ 预填 composer（正文 + multica 绑定 `{remoteTaskId, workspaceId}`，**不含 localProjectId**）→ 落到 conversation-home。App 预选最近活跃本地工作区（`activeWorkspaceId ?? lastActiveWorkspaceId`），composer **强制显示本地工作区下拉**（即便只有 1 个，只要 multica 绑定激活就强制显示），用户可改；改工作区时**保留 multica 绑定与预填正文**。点击发送 → `startMulticaConversationRun`（用下拉选中的 `projectId`）→ 该远程任务出现在本地侧栏对应工作区。**0 个本地工作区时** composer 显示「请先添加本地工作空间」引导并禁用发送。`local_project_id` 写入 `ActiveRemoteRun`，执行/作废路径用 `workspace_entry_for_project(&home_state, &run.local_project_id)` 解析 `workspace_path` → 构造绑定该目录的 App 实例 → `create_task_from_requirement`(requirement 作首轮 prompt) + `run_start_background`（按该 workspace 选定的 provider 编码进 Direct WorkflowDsl，见 3.2.2 执行映射）；**一个 task ↔ 一个本地 task**，断点续跑见 3.2.7。**同一个远程 workspace 可在不同本地目录重复执行**（每次执行由 composer 下拉选不同本地工作区，各自独立落地）。
- 远程任务管理页**形态独立成页**（M5-z）：`MulticaTaskManagementPage`（路由 `/chat/multica-tasks`）镜像 `MulticaRemoteTaskList`——左侧 multica workspace 分组（sticky header）→ 每个 workspace 下展开该 workspace 的 queued 任务列表 → 末尾【添加工作空间】（不限于 active，每个 workspace 一组、按各自 runtime_id + `X-Workspace-ID` 拉取 queued）；**失败可重试任务与 queued 同列混排**，仅多一个 `retryable` 标记 + 【rerun】入口（数据源：远程 queued + 本地 `multica_pending_issues` 未完成 issue）；**provider 由 task 所属 workspace 决定；本地工作目录由该次任务自己的 `local_project_id` 决定**，与 active 视图无关。active 仅影响默认展开/高亮哪一组。
- **页头 UX（M5-aa）**：页头右侧含「任务来源」下拉（i18n `multica.taskManagement.source.label`，当前唯一项 Multica，由 `REMOTE_TASK_SOURCES` 配置数组驱动、页级 `source` state 作渲染分流唯一键，为未来多来源接入保留切换位）+ 副标题 `multica.taskManagement.subtitle`「查看并执行远程任务」（M5-ab 再精简：页头「任务来源」下拉已点名来源，副标题不再重复 "multica" 限定词；原尾部关于执行时选本地目录的半句早已删除——该流程已被 claim-at-click → composer 覆盖）；列表区内右侧常驻手动刷新按钮（`RotateCw` ghost 图标，`aria-label="common.refresh"`，`refreshing` 态驱动 `animate-spin`，调与 mount/事件订阅同源的 `fetchTasks()`）；状态词以有色 Badge 呈现，色调按看板词汇锁定并由导出常量 `MULTICA_STATUS_TONE` 集中管理（待办=灰 / 进行中=黄 / 已完成=绿 / 失败=红），`queued` 文案对齐看板列为「待办」（码灵作为 daemon 直接驱动 board issue.status，本地任务生命周期与看板词汇 1:1，不再用暗示独立「领取」中间态的旧词）。
- **列表视觉/层级系统（M5-ab，统一一次做完非补丁）**：workspace→任务树状列表建立一致的间距节奏与层级表达——**workspace 分组头**=可折叠容器（统一 `ChevronDown` 折叠箭头规格 + 名称左侧 `Server` 图标标识「工作空间容器行」+ 名称右侧轻量任务计数 `（N个任务）`/`(N tasks)`，仅在该工作空间有任务时显示 + 整行 `rounded-md hover:bg-muted/40` 容器 hover 底色）；**任务行**=其下叶子节点（标题 14px font-medium 主文本、Badge 居左 + 时间戳 `ml-auto` 推右、相对组头统一缩进）；组间距 `mb-2` 加大、任务间距 `space-y-0.5`、水平 padding 组头 `px-1.5`/任务行 `px-2` 对齐；组内空状态居中带垂直留白（`px-2 py-4 text-center`），文案「该工作空间下暂无远程任务」明确所属与对象。pinned 失败段同规格（折叠箭头 + hover 底色 + 间距），但非 workspace 故不加 Server 图标。「服务器图标 = workspace 容器行 / 无图标 = 任务叶子行」一眼可读。

**PAT 串号防护**：换机器/换账号重连时强制重新走浏览器登录 + mint PAT、不复用前任 PAT。

#### 3.2.7 断点续跑（会话级 resume）

> **用户决策**：只要能接着上一次会话的内容继续工作即可（会话级续跑）；码灵工作流本身不在本次接入范围。同时**接受 multica 的 auto-retry**（最省接入成本），并把续跑建立在 multica 既有的 session resume 契约上。

**multica 侧的 resume 契约**（全是现成能力，server 零改动）：
- multica **不存节点级进度 / checkpoint**，task 行只存「上一次的 `session_id` + `work_dir`」（由码灵通过 `PinTaskSession` 回写，见 4.2 C8）。
- 续跑 = claim 时把 `prior_session_id = task.session_id` 带回 daemon（`daemon.go:1984` 的 session resume 决策），**是否真的续上取决于 agent（码灵）自己的 session resume 能力**——multica 只负责把 id 传过来。
- `recover-orphans` 对残留 in-flight task **无条件置 `runtime_recovery` 失败**（`agent.sql:895`），该原因在 multica 可重试集合内，会被 `MaybeRetryFailedTask` 自动重派（`max_attempts=2`）。
- `force_fresh_session`：rerun（重跑）置 true；retry（同任务重试）置 false（MUL-1128）——即 **rerun 必新会话，retry 可续旧会话**。

**码灵侧的续跑实现**（全部复用会话模式现有能力，不改核心库）：
```
失败 task（resume-safe：runtime_offline / runtime_recovery / timeout）
   │  server auto-retry 克隆【新 id 的子任务 T'】（继承 session_id/work_dir，force_fresh_session=false）
   ▼  用户重启客户端后领取的是子任务 T'（父 T 已 failed 无领取按钮）
claim T' → 响应带回 parent_task_id（指向父 T）+ prior_session_id（服务端回填的父 session）
   ▼  claim 响应血缘写入 prepare_leases[T']（start 命令无权再读 claim 响应）
start_multica_conversation_run(...)   # Req D：会话级续跑并入 classify_resume 分支
   │  classify_resume 两级反查 multica_task_conversations：
   │    ① get(T'.id)（同 id 场景：dispatched lease 过期同 row 重派）
   │    ② miss 且 parent_task_id 有 → get(T)（auto-retry 子任务场景，主路径）
   │  命中且本地 run is_run_continuable → 沿用父 local ids
   │  app.run_continue_background_with_config_overrides(local_task_id, local_run_id, None, None, Vec::new(), None, None)
   ▼
库层内部：读 worker-ref.json continue_ref → ACP session/load(acp_session_id, strict_continue=true) → 恢复上次上下文，起新 run
   │
   ├─ session 仍活  →  续跑成功；迁移索引 multica_task_conversations[T']=父 entry（local ids+session 沿用）+ remove(T)
   │                   （保证链式重试 T→T'→T'' 可续），正常走 complete/fail
   └─ session 已死（strict_continue 报错，client.rs:2132-2141）
         → 自动 fallback 整 task rerun（新会话，force_fresh_session）
```

> **关键**：续跑指针从「服务端给的 `prior_session_id`」改为「客户端按 `parent_task_id` 反查父任务的**本地**索引」——前者依赖服务端 `GetLastTaskSession` 兜底（有 caveat），后者直接命中客户端自己持久化的 `{local_task_id, local_run_id, acp_session_id}`，更稳。`prior_session_id` 退为兜底/校验。详见开发设计 §12.14。

**两种「再来一次」的区分**（落点不同，勿混）：

| 场景 | 触发 | 会话 | multica 动作 |
|---|---|---|---|
| **断点续跑** | resume-safe 失败被 auto-retry（克隆子任务 T'）/ 用户点【继续】 | **同一会话** `session/load` | retry（`force_fresh_session=false`）；客户端按响应 `parent_task_id` 反查父 T 本地索引续跑 |
| **整任务重跑** | resume-unsafe 失败（agent_error）/ 用户点【rerun】 | **新会话** | rerun（`POST /api/issues/{id}/rerun`，`force_fresh_session=true`），新 task 行 |

**断电续跑**：客户端进程被杀 / 断电 → 重启后 `start_multica_runtime` 跑 `recover-orphans`，把上次 in-flight 的 task 置 `runtime_recovery`；该 task 随后被 auto-retry **克隆成子任务 T'**（继承父 T 的 session_id/work_dir）。用户重启后领取 T'，码灵按响应 `parent_task_id` 反查 `multica_task_conversations[T]`（父 T 有有效 `acp_session_id`，已 `PinTaskSession` + 本地持久化）→ 沿用父 local ids 续跑同一会话——**用户体感 = 在原会话里输入「继续」接着干**。2h 内（子任务 queued TTL）有效；超 2h 子任务 expired，只能 rerun。

**真实约束（写入设计、不掩盖）**：会话级续跑**依赖 ACP provider 的 session 在重启后仍可 `session/load`**。若 provider 不持久化 session（或已过期清理），`strict_continue` 必失败 → 码灵自动 fallback 到整 task rerun（新会话从头跑），不会卡死或静默丢上下文。该 fallback 用 `multica.session-resume-failed` 错误码上报。

---

## 4. 接口文档

### 4.1 全局约定

| 项 | 约定 |
|---|---|
| BaseURL | Multica server 地址，如 `https://multica.your-company.com`，所有路径以 `/api/...` 开头 |
| 认证 | `Authorization: Bearer <PAT>`（PAT 以 `mul_` 开头）。daemon 接口（`/api/daemon/*`）与业务接口（`/api/issues` 等）都用同一个 user PAT |
| Content-Type | `application/json` |
| Workspace 头 | 业务接口（如 rerun、list issues）需带 `X-Workspace-ID` 或 `X-Workspace-Slug` |
| 终态回调 | complete/fail 带指数退避重试（4/8/16/32/64s，共 6 次），服务端幂等 |

### 4.2 接口清单（按流程分组）

#### A. 认证与身份

**A0. 浏览器登录 + localhost callback**（码灵首启，拿一次性 JWT）｜ **复用 multica 原生，server 零改动**
- 码灵起临时本地 HTTP server（监听 `127.0.0.1:<port>`）→ 打开系统浏览器到 `<MULTICA_APP_URL>/login?cli_callback=http://127.0.0.1:<port>/callback`
- 用户在浏览器完成 multica 原生邮箱登录 → multica Web 校验 `cli_callback` 白名单（`validateCliCallback`，放行 localhost / 127.0.0.1 / RFC1918）→ 302 回跳 `http://127.0.0.1:<port>/callback?token=<JWT>`
- 码灵本地 server 收到 JWT 后**向浏览器回 302 → `<MULTICA_APP_URL>/`**（multica Web 在回跳前已 `setLoggedInCookie`，故浏览器落在 multica web 登录态），随后关停。码灵**不向浏览器渲染任何登录结果页**。**JWT 仅用于下一步换 PAT，用完即弃、不落盘**。完整链路见 3.2.6；CLI 同款实现参考 `server/cmd/multica/cmd_auth.go:235-358`。
- 错误：`multica.login-callback-timeout`（5min 未回跳）/ `multica.login-callback-failed`（回跳缺 token / 非白名单）。

**A1. 获取 PAT**（用 A0 回跳的 JWT 一次性换取，长期凭证）
- `POST /api/tokens` ｜ 需 user 身份（A0 拿到的 JWT）
- 请求：`{ "name": "Maling Desktop", "expires_in_days": 90 }`
- 响应：`{ "token": "mul_...", "expires_at": "..." }`
- 说明：PAT 是长期凭证，落盘保存。续期 `POST /api/tokens/current/renew`。

**A2. 注册 daemon 与 runtime**（启动遍历已添加 workspace + 新添加 workspace 时；均幂等）
- `POST /api/daemon/register` ｜ PAT
- 请求体：
  ```jsonc
  {
    "workspace_id": "<workspace UUID>",
    "daemon_id":    "<本机持久 UUID v7>",
    "device_name":  "dev-host-A",
    "cli_version":  "1.0.0",
    "runtimes": [
      { "name": "码灵客户端", "type": "claude-acp", "version": "1.0.0", "status": "online" }
    ]
  }
  ```
  > `runtimes[].type`（provider）= 该 workspace 添加时选定的 ACP provider（默认 `claude-acp`，可选 `codex-acp`/`gemini-acp` 等，下拉来自码灵已配置 provider 列表）。无 DB 约束、不影响路由，但**绑定后不可变**——它是 upsert 仲裁键 `(workspace_id, daemon_id, provider)` 的一部分，变了会生成新 runtime_id（旧 runtime 废弃、agent 需在 web 重绑）。因此**每个 workspace 恰好一个 provider = 一个 runtime**；要换 provider 等于换 runtime 行。
- 响应：`{ "runtimes": [{ "id": "<runtime_id>", ... }], "repos": [...], "settings": {...} }`
- 说明：**幂等**。持久化 `daemon_id`，同一 `(workspace_id, daemon_id, provider)` 命中同一行返回同一个 runtime_id，故同一 workspace 无论添加/启动多少次 runtime_id 永远稳定（已绑 agent 直接复用，无需重绑）；首次 register 的新 workspace 才需在 web 绑 agent。**全量 register**：启动时遍历 `desktop_multica_workspaces` 已添加列表逐个 register，新添加 workspace 时 register 该 workspace；不 register server 全量、也不在切换 active 时 register。runtime_id 不必本地落盘。

**A3. 清理残留任务**（每次启动、register 后立即调）
- `POST /api/daemon/runtimes/{runtimeId}/recover-orphans` ｜ PAT ｜ 请求体 `{}`
- 说明：无条件 fail 掉上个进程留给该 runtime 的 dispatched/running 任务（`runtime_recovery`）。该原因属 **resume-safe**：会被 multica auto-retry 重派（克隆子任务 T'，`max_attempts=2`），重派后码灵 claim T' 按响应 `parent_task_id` 反查父任务本地索引走会话级续跑（3.2.7）；用户也可手动【继续】或【rerun】。

#### B. 任务列表与领取

**B1. 拉取任务列表（只读）** ★ 核心
- `GET /api/daemon/runtimes/{runtimeId}/tasks/pending` ｜ PAT
- 响应：任务数组，含 `status ∈ ('queued','dispatched')` 的任务
- 说明：**只读、不锁任务、不改状态**。客户端按 `status=='queued'` 过滤得到真正可领取的列表，展示给用户。

**B2. 领取指定任务（selective claim）** ★ 核心 ｜ **新增接口（需 server 改造，见 3.1）**
- `POST /api/daemon/runtimes/{runtimeId}/tasks/{taskId}/claim` ｜ PAT ｜ 请求体 `{}`
- 响应：`{ "task": <Task> }`（含 `auth_token`；auto-retry 子任务还会带 `parent_task_id`（指向父任务）+ `prior_session_id`（服务端回填的父 session）——客户端按 `parent_task_id` 反查父任务本地索引续跑，见 3.2.7 / 开发设计 §12.14）
- 说明：原子把用户点的这条任务（`runtime_id` 匹配 + `status='queued'`）置为 dispatched。任务不存在/不属该 runtime/非 queued → 404/409。
- ⚠️ 现有的 `POST /api/daemon/tasks/claim`（批量 FIFO）和 `POST /api/daemon/runtimes/{runtimeId}/tasks/claim`（逐 runtime FIFO）**不能指定 task_id**，不满足「点哪领哪」，所以才需要本接口。

> claim 之后**没有退回/释放接口**，唯一出路是执行到 complete 或 fail。因此列表展示用只读的 B1，确认要做了再用 B2 领取。

#### C. 任务执行与上报

**C1. 心跳**（Req D：常驻维持在线）
- `POST /api/daemon/heartbeat` ｜ PAT
- 请求：`{ "runtime_id": "<runtime_id>", "supports_batch_import": true }`
- 说明：Req D 起为常驻--与 multica 建立连接后即持续每 15s 一次（对所有已连接 workspace 的 runtime），不再仅任务执行期。150s 无心跳 -> runtime 离线 -> 在飞任务 fail。

**C2. 标记开始**（Req D：claim-at-click 后，用户在 composer 点「发送」时调）
- `POST /api/daemon/tasks/{taskId}/start` ｜ PAT ｜ 请求体 `{}`
- 说明：把任务从 dispatched 推进到 running。Req D 起 claim 与 start 拆分：点【执行】只 claim（含 45s prepare lease），用户在 composer 编辑/选模型后点【发送】经 `start_multica_conversation_run` 才调本接口。compose 期间由 C2.5 续期 lease 防 45s 回收；claim 后 5 分钟内不 start 会被超时 fail。

**C2.5. 续期 prepare lease**（Req D 新增，compose 期间防回收）
- `POST /api/daemon/runtimes/{runtimeId}/tasks/{taskId}/prepare-lease` ｜ PAT ｜ 请求体 `{}`
- 说明：claim-at-click 后、用户尚未点【发送】的 compose 窗口期，常驻心跳循环每 tick（15s）对本 task 调一次，续 45s prepare lease 窗口，防 server `ReclaimStaleDispatchedTaskForRuntime` 回收（与 multica daemon `startTaskPrepareLeaseExtender` 同构）。用户点【发送】（start 消费）或放弃 compose（`cancel_multica_prepare_lease`）后该 lease 移除，续期自然停止。

**C3. 上报进度**（可选，**本期不接入**）
- `POST /api/daemon/tasks/{taskId}/progress` ｜ PAT
- 请求：`{ "summary": "处理中", "step": 2, "total": 5 }`
- 说明：用户决策本期只上报基础状态（started / 心跳 / complete / fail），**不上报 step/total 进度**；接口保留备用，后续迭代再接。

**C4. 上报消息**（可选，**本期不接入**）
- `POST /api/daemon/tasks/{taskId}/messages` ｜ PAT
- 请求：`{ "messages": [ { "seq": 1, "type": "text", "content": "..." } ] }`

**C5. 查询状态**（可选，检测取消）
- `GET /api/daemon/tasks/{taskId}/status` ｜ PAT ｜ 响应 `{ "status": "running" }`
- 说明：执行长任务时周期查询，发现 `cancelled`/`failed`/404 则中断本地执行。

**C6. 完成** ✅
- `POST /api/daemon/tasks/{taskId}/complete` ｜ PAT
- 请求：`{ "output": "<末轮产物摘要>", "session_id": "<ACP session_id>", "work_dir": "..." }`（后两个可选）
- 说明：任务 → completed。带重试、幂等。`session_id` = 该会话的 ACP session_id（来自 `worker-ref.json` 的 `continue_ref.acpSessionId`，续跑依赖，故 complete 前先调 C8 `PinTaskSession` 回写）；`work_dir` 取自该任务**任务级** `ActiveRemoteRun.local_project_id` → `conversation_workspaces.workspace_path`（M5-z：绑定下沉到任务级，不再走 workspace 级 `local_project_id`）。

**C7. 失败** ❌
- `POST /api/daemon/tasks/{taskId}/fail` ｜ PAT
- 请求：`{ "error": "<错误描述>", "failure_reason": "agent_error" }`
- 说明：任务 → failed。`failure_reason` 留空默认 `agent_error`。带重试、幂等。**complete 前若已 `PinTaskSession`，fail 后该 session_id 仍可被续跑复用**（resume-safe 原因）。
> **接受 multica auto-retry**（用户决策，最省接入）：fail 时 `failure_reason` 如实传值；multica 对 `runtime_offline` / `runtime_recovery` / `timeout` 等 resume-safe 原因自动重派（`max_attempts=2`），码灵 claim 时若拿到 `prior_session_id` 即走会话级续跑（3.2.7）；`agent_error` 等 resume-unsafe 原因不自动重试，由用户点【rerun】整任务重跑（D1）。

**C8. 绑定会话到 task（PinTaskSession）** ★ 续跑前提 ｜ **复用 multica 现有接口，server 零改动**
- `POST /api/daemon/tasks/{taskId}/session` ｜ PAT
- 请求：`{ "session_id": "<ACP session_id>", "work_dir": "<本地目录>" }`
- 说明：把会话的 ACP `session_id` + `work_dir` 写回 task 行（`router.go` daemon 组已注册）。**续跑依赖此回写**：recover-orphans / auto-retry 重派时 server 把该 `session_id` 作为 `prior_session_id` 经 claim 带回（B2）。码灵在 ACP session 建立后（拿到 `worker-ref.json` 的 `continue_ref.acpSessionId`）立即调一次，本地同步记入 `multica_task_conversations`。

#### D. 失败恢复（业务 API，user PAT）

**D1. 重新执行任务（issue rerun）** ★ 长期恢复兜底
- `POST /api/issues/{id}/rerun` ｜ user PAT ｜ 需 `X-Workspace-ID` 头 ｜ 请求体可空
- 说明：**创建一条全新的 queued 任务**，按 issue 当前 assignee 的 runtime 路由（agent 还绑在你这台机器 → 派回你）。**无 issue 状态限制、无次数/时间上限**，可反复调。
- 语义：fail 后原任务行保持 failed 终态（审计），rerun 是新建任务行（`rerun_of_task_id` 指回原行）=「这件事重做一遍」。
- 前提：issue 当前 assignee 仍是绑定本 runtime 的 agent；agent 未归档；issue 未被删除（删除则 404）。
- `force_fresh_session` 强制新会话——对自建 daemon（自己执行、不依赖 provider session）无影响。

#### E. 辅助查询（本地记录丢失时兜底）

**E1. 列出 agent**（找绑定本 runtime 的 agent）
- `GET /api/agents` ｜ user PAT ｜ 需 `X-Workspace-ID`
- 说明：返回 workspace 内 agent 列表（含 `runtime_id` 字段）。客户端按 `runtime_id == 自己的 runtime_id` 过滤，找到绑定本 runtime 的 agent。

**E2. 列出 issue**（找待办）
- `GET /api/issues?assignee_id=<agent>&open_only=true` ｜ user PAT ｜ 需 `X-Workspace-ID`
- 说明：列出指派给某 agent 的未完成 issue。正常情况靠客户端本地记录的 issue_id 即可，本接口仅作兜底。

### 4.3 完整时序图

```
参与者： 码灵客户端(App) │ Multica Server(Srv) │ Multica Web(Web) │ 管理员

━━━ 一次性准备（每台机器一次） ━━━
App    起本地 callback server(127.0.0.1:<port>) → 打开浏览器到 Web /login?cli_callback=http://127.0.0.1:<port>/callback
Web    用户邮箱登录 → setLoggedInCookie → 校验 cli_callback 白名单(validateCliCallback) → 302 回跳 ?token=<JWT>
App    收 JWT → 向浏览器回 302 → Web 根（带 cookie 落 multica web 登录态）→ 关 callback server → POST /api/tokens (Bearer JWT) → PAT（本地持久化）；JWT 用完即弃
App    ──POST /api/daemon/register {daemon_id,...}──▶ Srv ──▶ runtime_id（每添加的 workspace 一次）
管理员 ──Web UI 建 agent，runtime 绑本机──▶ Srv
App    本地持久化：PAT / daemon_id / workspaces(只含 provider，本地目录执行时选) / active_workspace_id

━━━ 每次启动 ━━━
App ──GET /api/me──▶ Srv   校验 PAT（无效则回到「一次性准备」的浏览器登录）
App ──POST /register──▶ Srv ──▶ runtime_id（幂等，同一行）
App ──POST /runtimes/<rid>/recover-orphans──▶ Srv   残留 in-flight task → runtime_recovery（无条件失败，但属 resume-safe，可被续跑/auto-retry）

━━━ 拉取任务列表（用户点按钮） ━━━
App ──GET /runtimes/<rid>/tasks/pending──▶ Srv ──▶ 任务列表（只读）
App 展示 status=='queued' 的任务给用户（失败 retryable 任务同列混排）

━━━ 用户点某条「执行」（Req D：claim-at-click，不再立即拉起会话） ━━━
App ──POST /runtimes/<rid>/tasks/<tid>/claim──▶ Srv   ← selective claim（新增接口）
    ◀──── {task:{id, issue_id, auth_token, prior_session_id?,...}} ────
App    本地记 multica_task_conversations[task_id] + prepare_leases[task_id]；预填 composer（requirement 入输入框）
       （compose 期间：常驻循环每 15s 续期 prepare lease，见 C2.5）

━━━ 用户在 composer 选模型/模式后点【发送】（Req D：复用本地会话创建） ━━━
App    start_multica_conversation_run：复用本地 create_conversation_run_vm 构造会话
App ──POST /tasks/<id>/start──▶ Srv   → running（消费 prepare lease）
App    取 task 级 local_project_id → 解析 workspace_path → App::with_config(ws,config).with_lifecycle_bus(bus)
       → 构造 Direct WorkflowDsl（gold_band::dsl::presets，provider 编码进 Worker 节点）
       → create_task_from_requirement(content=requirement)（首轮 prompt = requirement.md，不走 submit）
       → run_start_background(task_id)（后台驱动单节点工作流）
       NodeCompleted 后 worker_ref_show 读 continue_ref.acpSessionId → session_id
App ──POST /tasks/<id>/session {session_id, work_dir}──▶ Srv   ← PinTaskSession（C8，续跑前提）

执行中：
App ──POST /heartbeat──▶ Srv        每15s（Req D：已常驻，连接后即持续，非仅执行期）
（本期不上报 progress/messages，只维持心跳）

完成：
✅ App ──POST /tasks/<id>/complete {output, session_id}──▶ Srv  → completed
❌ 或 ──POST /tasks/<id>/fail {error, failure_reason}──▶ Srv      → failed

放弃 compose（Req D）：
App    cancel_multica_prepare_lease → 任务回 queued（供自己或他人重新领取）

━━━ 断点续跑（resume-safe 失败：断电 / 超时 / runtime 离线） ━━━
场景A 自动：Srv auto-retry 重派(克隆子任务 T'，max_attempts=2) → claim T' 响应带 parent_task_id（客户端反查父 T 本地索引续跑）
场景B 手动：用户在失败任务点【继续】 → start_multica_conversation_run(...)（Req D：经 classify_resume 命中续跑分支，未单列 resume 命令）
App    run_continue_background_with_config_overrides(task_id, run_id, None, None)（内部读 worker-ref continue_ref）
       → ACP session/load(prior_session_id, strict_continue)
         ├─ session 活 → 续跑 → complete/fail
         └─ session 死 → multica.session-resume-failed → fallback 整 task rerun（新会话）

━━━ 整任务重跑（resume-unsafe 失败：agent_error / 用户主动重来） ━━━
App ──POST /api/issues/<id>/rerun──▶ Srv   force_fresh_session=true → 新 queued task（新会话从头跑）
```

### 4.4 关键源码位置（实现参考）

| 内容 | 文件 |
|---|---|
| daemon HTTP 客户端全部方法（URL/请求体权威来源） | `server/internal/daemon/client.go` |
| 服务端请求/响应 struct（json tag 权威） | `server/internal/handler/daemon.go` |
| Task 响应完整字段 | `server/internal/daemon/types.go` |
| 现有 claim 实现（selective claim 改造参照） | `ClaimTaskByRuntime` `daemon.go:2512`、`ClaimTaskForRuntime` `task.go:2427`、`ClaimAgentTask` `agent.sql:508` |
| rerun 实现 | `RerunIssue` `task_lifecycle.go:122`、`task.go:3897` |
| daemon 认证中间件 | `server/internal/middleware/daemon_auth.go` |
| 超时/重试常量 | `server/cmd/server/runtime_sweeper.go` |
| PAT 创建/存储（A1 权威） | `server/internal/handler/personal_access_token.go`、`auth/jwt.go`、迁移 `011_personal_access_tokens.up.sql` |
| daemon 路由表（含 PinTaskSession 注册位置） | `server/cmd/server/router.go:842-887` |
| **浏览器登录 + localhost callback 模板**（A0 抄这里） | `server/cmd/multica/cmd_auth.go:235-358` |
| cli_callback 白名单校验 | `packages/views/auth/login-page.tsx`（`validateCliCallback`）、`apps/web/app/(auth)/login/page.tsx:68` |
| PinTaskSession handler（C8，续跑前提） | `server/internal/handler/daemon.go`（PinTaskSession） |
| claim 时 session resume 决策（prior_session_id） | `server/internal/handler/daemon.go:1984` |
| recover-orphans（无条件置 runtime_recovery） | `server/pkg/db/queries/agent.sql:895`、`server/internal/handler/task_lifecycle.go:16` |
| **码灵库层会话执行 API**（bridge 直接调，不走 command 层） | `src/app/mod.rs`：`App::with_config`/`with_lifecycle_bus`、`create_task_from_requirement`、`run_start_background`、`worker_ref_show`、`run_continue_background_with_config_overrides`；Direct/Auto workflow preset 上提到 `gold_band::dsl::presets`（会话 VM `view_models_conversation.rs:2579/2461` 现有私有 fn 公开复用）；首轮 prompt 自动加载 `node_executor.rs:610` |
| ACP session_id 落盘 + strict_continue（续跑/失效） | `src/acp/client.rs:3759-3760`（worker-ref acpSessionId 写入）、`src/acp/client.rs:2132-2141`（strict_continue bail） |

---

## 5. 改动任务清单

> 按 multica server 端 / 码灵客户端两侧拆分，可并行推进。server 侧两项改造相互独立；客户端侧按里程碑 M1→M6 顺序。详细设计见《Multica开发设计》对应章节。

### 5.1 multica server 端（1 项源码改造 + 1 项运维）

- [x] **S1 · selective claim 接口**（详 3.1 改造一，「点哪领哪」核心）✅ 详见里程碑 M5-n
  - [x] `server/pkg/db/queries/agent.sql` 新增 `ClaimSpecificQueuedTask :one`（`UPDATE ... WHERE id=@task_id AND runtime_id=@runtime_id AND status='queued' RETURNING *`，逐字保留 `ClaimAgentTask` 串行化守卫 + lease）
  - [x] `server/internal/service/task.go` 新增 `ClaimSpecificTask(ctx, runtimeID, taskID)`：runInTx 调 SQL → 复用 dispatch 三件套（不做容量/reclaim/候选循环）；ErrNoRows→nil
  - [x] `server/internal/handler/daemon.go` 新增 `ClaimSpecificTask` handler：完整复刻 `ClaimTaskByRuntime` 六步收尾链（`requireDaemonRuntimeAccess` → service → `repairStaleCommentPlanIfNeeded` → `buildClaimedTaskResponse` → token mint → `FinalizeTaskClaim`），唯一改动 FIFO→specific、nil→404
  - [x] `server/cmd/server/router.go` 注册 `POST /api/daemon/runtimes/{runtimeId}/tasks/{taskId}/claim`
  - [x] `sqlc generate`（v1.31.1）重新生成
- [ ] **S2 · 运维准备**（非代码）：开发者在 multica web 自行注册账号（邮箱登录，**无需管理员预建 user / 预绑 `os_username`**）；为各 workspace 的 agent 绑定对应 runtime_id（首次 register 的新 workspace 需在 web 绑 agent）

> 其余需求（任务列表 `GET /tasks/pending`、失败恢复 `POST /api/issues/{id}/rerun`、注册 `POST /api/daemon/register`、心跳、recover-orphans、complete/fail）均用 multica 现有接口，**无需改源码**。

### 5.2 码灵客户端（按里程碑顺序）

- [x] **M1 · 配置层**（开发设计 2.1 / 2.2）✅
  - [x] `src/config/mod.rs`：`SettingsConfig` 加 `desktop_multica_enabled/_base_url/_app_url/_pat/_daemon_id/_workspaces/_active_workspace_id/_default_provider`（全 `Option<T>`，PAT 明文存储、永不回显只暴露 `pat_set`）
  - [x] `RuntimeConfig` + `apply_settings` 三层映射（`default_provider` 缺省 `claude-acp`）
  - [x] `MulticaWorkspaceRef { id, name, slug, provider }`（**M5-z：删除 `local_project_id`**，绑定下沉到任务级——`local_project_id` 现位于 `ActiveRemoteRun` / `MulticaCompletedTask`；provider 绑定后不可变）
  - [x] `configs/channels/*.json` + `channel.rs` 编译期预填 base_url / daemon_id
- [x] **M2 · multica client + loop**（开发设计 2.3 / 2.6）✅
  - [x] `multica/client.rs`：浏览器登录(localhost callback) → 换 PAT → 列 workspace → register → selective claim → start → 15s heartbeat → PinTaskSession → complete/fail（**只基础状态，不上报 progress step/total**）→ recover-orphans → tasks/pending → rerun / resume_multica_task（会话级续跑）；指数退避重试（终态 4/8/16/32/64s ×6，服务端幂等）；**接受 multica auto-retry**；PAT 走 `Authorization: Bearer`，**严禁日志泄露**
  - [x] `multica/loop_.rs`：`start_multica_loop`（启动全量 register / 新添加 register / 15s 心跳 / recover-orphans / 取消检测），照搬 `metrics::start_heartbeat_polling` 骨架 + 三层 guard + 每 tick 重读配置
  - [x] `main.rs` setup 挂载 `start_multica_loop`（`start_heartbeat_polling` 之后）
- [x] **M3 · 列表与领取**（开发设计 2.4 / 2.8）✅ `cargo test` 25 单测全过
  - [x] client `list_pending_tasks / claim_specific_task / start_task / get_task_status`
  - [x] `vm.rs`（`RemoteTaskVm` / `RemoteConversationSidebarVm`，对齐 `ConversationSidebarVm` 键名）
  - [x] 命令 `get_multica_tasks`（按 workspace 分组拉 `list_pending` + 失败 `retryable` 回显进 `pinned_tasks`）/ `claim_multica_task(task_id, workspace_id)`
  - [x] `StateConfig.multica_pending_issues` 语义澄清：**失败待重试**（M4 fail 写入、complete/rerun 清除，claim 不写）
- [x] **M3.5 · workspace 绑定 + 会话创建注入**（开发设计 2.5 / 2.8）✅ 绑定核心随 M4-c 落地，workspace-CRUD 命令随 M5-c
  - [x] `multica/config.rs` `binding_for_multica(RuntimeConfig, StateConfig, ws_id) -> (workspace_path, provider)`（workspace → `local_project_id` → `conversation_workspaces.workspace_path`，复用 `workspace_entry_for_project`）✅ 随 M4-c
  - [x] 执行衔接按 workspace provider 构造 Direct WorkflowDsl（复用 `gold_band::dsl::presets::direct_workflow`）+ `create_task_from_requirement`(requirement 作首轮 prompt) + `run_start_background`（库层 App API，不走 command 层）✅ 随 M4-c
  - [x] 命令 `list_server_multica_workspaces` / `add_multica_workspace`（下拉单选 + folder picker + provider，非 git 警告不阻断）/ `rebind_multica_workspace` / `remove_multica_workspace` / `set_active_multica_workspace`（workspace CRUD，配合 M5 设置页前端）✅ 随 M5-c
- [x] **M4 · 执行衔接（会话模式）**（开发设计 2.7）✅ M4-a/b/c/d 全过
  - [x] **client 层（M4-a）✅** `cargo test` 30 单测全过：`complete_task(output, session_id?, work_dir?)` / `fail_task(error, failure_reason)` 经 `with_terminal_retry`（NetworkFailed-only 退避 4/8/16/32/64s 共 6 次，确定错误码立即返回）；`pin_task_session` 走一般网络重试；`rerun_issue(workspace_id, issue_id)` 带 `X-Workspace-ID` 头（`post_json_with_workspace`）；wire 契约 `CompleteRequest`/`FailRequest`/`PinTaskSessionRequest`（缺失字段不序列化）
  - [x] **bridge 订阅器（M4-b）✅** `cargo test` 38 单测全过：`create_multica_subscriber` 注册 `desktop.multica`（`subscribe_named`）；`NodeCompleted`→读 `attempt_dir/worker-ref.json`（复用库层 `WorkerRefState`）采 `acpSessionId`+`cwd`→session 变更才 `pin_task_session`+落 `task_conversations`；`RunCompleted`→分支穷举（Success→`complete`+清 pending_issues / Failure→`fail("agent_error")`+记 pending_issues / **Killed→`fail("timeout")`（M4-d 细化）** / Paused·Intervention→不上报）；归属靠 `active_runs` 反查 (local_task_id, local_run_id)（RunCompleted 无 repo_root）；HTTP 经 `tauri::async_runtime::spawn` 异步；`ActiveRemoteRun` 重塑 + `register/drop/find_active_run_by_local`
  - [x] **执行入口（M4-c）✅** `cargo test` 42 multica 单测 + preset 2 单测全过：`gold_band::dsl::presets::direct_workflow(provider, model, permission_mode, config_options)` 上提（会话 VM `build_direct_workflow` 改委托；**仅 direct**——auto 核心是 VM 特有策略翻译、无第二消费方）；`binding_for_multica(RuntimeConfig, StateConfig, ws_id) -> (workspace_path, provider)` 复用 `workspace_entry_for_project`；`start_multica_remote_task`：claim→`binding_for_multica`→**`state.app().with_repo_root(ws_path, config)`**（共享 `DesktopState.lifecycle_bus`，订阅器据此收事件——`App::with_config` 自带空 bus 不可用）→`create_task_from_requirement`(requirement)+`run_start_background`→登记 `active_runs`(真实 run.id)+落 `task_conversations`(session_id=None 待 bridge 回填)→`start_task(false)`；sync 库层调用经 `spawn_blocking`，本地启动失败 best-effort `fail_task`；`cancel_multica_task`：反查 `active_run`→`run_pause(ProcessInterrupted)`+杀 ACP→清 `active_runs`+`task_conversations`；`auth_token` 本期不注入（保留）
  - [x] **断点续跑 + rerun + 远程 fail 本地作废（M4-d）✅** `cargo test` 47 multica 单测全过：
    - **断点续跑**（开发设计 2.5 / 4.4）— **未另立 `resume_multica_task` 命令**：`start_multica_remote_task` 经 `classify_resume(Option<&MulticaTaskConversation>)` 自动分支（命中先前未终态 local run → `Resume`，否则 `Fresh`），**单一执行入口更合 UX**。Resume 走 `run_continue_background_with_config_overrides(local_task_id, local_run_id, None,None,[],None,None)`（内部 worker-ref `continue_ref`→`session/load` 续同一 ACP session）；**strict_continue 失败（session 已死，`acp/client.rs:2132-2141`）→ fresh fallback**（`start_fresh` 新建 task+run，`start_task(force_fresh_session=true)`）；**任何 resume 失败→fresh**（最坏整任务重跑，安全，无需 fragile 匹配错误串，放弃原方案的 `multica.session-resume-failed` 字符串匹配）
    - **`rerun_multica_task(issue_id, workspace_id)`**：`client.rerun_issue`（M4-a `post_json_with_workspace`，force_fresh_session 新 queued）→ 清本地 `pending_issues[issue]`（`list.retain`）
    - **Killed 分支**（开发设计 2.5 终态表细化）：`TerminalAction::NoReport`→`Fail{reason:"timeout"}`，删 `NoReport`/`PendingUpdate::None` 死分支。关键洞察：**cancel 路径皆经 `run_pause→Paused` 从不产生 Killed**，故 Killed 必为 agent 真死，无需「cancel-detection 上下文」标志消歧（避免补丁式修复）；`timeout` 为 resume-safe，server 可 auto-retry
    - **远程 fail 本地作废**（开发设计 4.4 ⚠️）：抽 `bridge::teardown_active_run(workspace_app, shared, home_app, remote, local_task_id, local_run_id)`（`run_pause(ProcessInterrupted)`+杀 ACP+清 `active_runs`+清 `task_conversations[remote]`）供 cancel 命令 / 启动 reconcile / 周期取消检测**三处共用**；**C2 启动 reconcile**（`recover-orphans` 后 `reconcile_startup_orphans`：`task_conversations` 条目 remote cancelled/404→作废，**不在 failed 作废**——保 retryable 续跑，terminal-failed 的本地 Paused run 由 strict_continue fallback 兜底）；**C3 周期 cancel-detection**（心跳同 tick `detect_cancelled_active_runs`：在飞 active_run remote failed/cancelled/404→作废，无 resume 冲突）；`invalidate_remote_task` active_run→`binding_for_multica` 优先，回退 `task_conversations[remote].work_dir`（崩溃 orphan 场景）
- [x] **M5 · 前端**（开发设计 3.x）✅ M5-a/b/c/d/e/f 全过
  - [x] **M5-a** API 四层（client.ts/desktop.ts/browser.ts/api.ts）+ 类型（`MulticaSettingsVm` / `MulticaWorkspaceRef` / `RemoteTaskVm` / `RemoteConversationSidebarVm`）。13 个 multica 方法 + `subscribeMulticaTaskUpdates`（M5-b 事件订阅）；**修复 `startMulticaRemoteTask` 缺失 `workspaceId` 参数**（原签名仅 taskId，后端命令需 workspace_id）。
  - [x] **M5-b** 后端事件 `gold-band://multica-task-updated`（bridge.rs 终端态 + loop_.rs 作废路径 emit，unit payload，前端监听到后 refetch 全量 sidebar VM）。
  - [x] **M5-c** workspace 绑定 CRUD：5 命令（`list_server_multica_workspaces`/`add_multica_workspace`/`rebind_multica_workspace`/`remove_multica_workspace`/`set_active_multica_workspace`），复用 `project_id_for_workspace`+`project_ids_match` 去重，slug=id 兜底。
  - [x] **M5-d** i18n 双语：`settings.multica.*` zh-CN/en 各 22 key + `conversation.sidebar.multica.*` 各 8 key + `errors.multica.*` 各 12 key（覆盖全部 12 个错误码含 workspace-already-bound/workspace-not-found）。
  - [x] **M5-e** 设置页 multica 区块：`MulticaSettingsBlock`（`web/src/components/settings/MulticaSettingsBlock.tsx`）——自管理组件（barrel API 直连，复用 ui/switch toggle，provider 常量选项 claude-acp/codex-acp，连接/保存 + 已绑定工作空间管理 inline），SettingsPage advanced tab 以 `<SettingsSection title={t('settings.multica.title')} divided><MulticaSettingsBlock /></SettingsSection>` 嵌入（与 metrics 同模式）。**M5-h 起移除「添加工作空间」行**（破坏式更新：添加入口收敛到会话侧栏远程任务列表弹窗，设置页只留配置 + 已绑定工作空间管理：激活/改绑/删除；改绑经 `pickLocalDirectory()` 选目录后调 `rebindMulticaWorkspace(id, path)`）。
  - [x] **M5-f** 会话模式远程任务列表：`MulticaRemoteTaskList`（`web/src/components/conversation/MulticaRemoteTaskList.tsx`）——自管理组件（getMulticaTasks + subscribeMulticaTaskUpdates 事件→refetch，claim+start→navigate，cancel/rerun→barrel API，not connected 空态卡片+连接入口）。ConversationSidebar 新增 本地/远程 segmented toggle（button cva compose）→ 按 remoteView 条件渲染 ScrollArea（local 保留既有工作空间列表零改动，remote 显示 MulticaRemoteTaskList）。**实现偏离设计**：未复用 `TaskRow`（数据形状不同——RemoteTaskVm 无 projectId/taskId/runs），改用专用 `RemoteTaskRow`（title+status badge+内联操作按钮）。**M5-h 起**：连接态常驻【添加工作空间】入口行（对齐本地列表 Plus 行），点击弹 `MulticaAddWorkspaceDialog`；已连接但无绑定工作空间时空状态文案区分（`noWorkspacesBound`，引导去添加）。
  - [x] **M5-g** tsc 零 multica 新增错误，cargo check 通过。
  - [x] **M5-h**（本轮）登录落点 + 添加工作空间弹窗 + 目录选择原语：
    - **登录落点修正**：浏览器登录成功后，码灵本地 callback server **不再渲染"登录成功，请返回码灵"页**，改为向浏览器回 `302 → <MULTICA_APP_URL>/`（multica Web 回跳 cli_callback 前已 `setLoggedInCookie`，故浏览器落在 multica web 登录态）；client.rs 抽纯函数 `callback_redirect_response(app_root)` 生成该 302 响应，单测固化（无 token 泄露、trailing-slash 归一、query 不外泄）。
    - **添加工作空间弹窗**：新增 `MulticaAddWorkspaceDialog`（远程工作空间下拉 + provider 下拉 + 绑定本地目录按钮 + 添加），形态对齐本地任务列表添加入口；`MulticaRemoteTaskList` 接入常驻入口行 + 空状态引导。
    - **目录选择原语**：api 四层抽出共享 `pickLocalDirectory()`（Tauri `plugin-dialog` `open({directory:true})`，浏览器态 null），`addMulticaWorkspace`/`rebindMulticaWorkspace` 改为接收显式 `localPath` 入参（不再把文件选择器藏在 API 内部）。
    - 验证：50 后端 multica 单测全过（含 callback_redirect_response），tsc 源码零错误，i18n 中英双语补齐（`conversation.sidebar.multica.addWorkspace`/`noWorkspacesBound`/`dialog.*` 10 key）。
  - [x] **M5-i**（本轮）断开连接入口（对称 connect）：
    - **背景**：`connected` 判定 = PAT 存在（`multica_settings`，乐观判定）。换账号 / 退出登录 / 本地反复联调需要回到「连接 Multica」入口，原先只能手改 `settings.json` 的 `desktopMulticaPat`。补齐 connect 的对称能力 `disconnect_multica`，而非补丁式 workaround。
    - **命令**：`disconnect_multica`（sync）——纯函数 `clear_multica_session(&mut SettingsConfig)` 清 `desktop_multica_pat`（**保留** daemon_id / workspace 绑定 / active，与登录态正交），`MulticaRuntimeState::clear_runtime_ids` 清运行期 register 缓存（重连后 loop 重建；`active_runs` 保留——在飞本地 run 的 remote 映射，断开不改其归属）。
    - **侧栏同步**：connect/disconnect 均经 `emit_multica_task_updated` 发 `gold-band://multica-task-updated`，会话侧栏远程任务列表 re-fetch `get_multica_tasks`（断开后返回 `connected:false` → 回到未连接空状态）。
    - **UI**：设置页 multica 区块【连接 Multica】按钮在已连接态文案切为「重新连接」，并在已连接态显示【断开连接】按钮（ghost + hover destructive，对齐 remove workspace 风格）。
    - 验证：52 后端 multica 单测全过（+`clear_multica_session`/`clear_runtime_ids` 各 1），tsc 源码零错误，816/817 前端用例过（1 既有 scrollbar CSS 失败与 multica 无关），i18n 中英双语补 `disconnect`/`disconnecting`/`reconnect`。
  - [x] **M5-j**（本轮）添加工作空间弹窗布局修复 + 空状态自诊断：
    - **布局 bug 根因**：`MulticaAddWorkspaceDialog` 沿用 shadcn `DialogContent` 默认 `grid` + 无高度上限，弹窗 `top-1/2 -translate-y-1/2` 居中，内容（远程下拉 + provider 下拉 + 路径行 + 错误）增高后底部 `DialogFooter`（【添加】）被顶出视口；选中长本地路径横向撑破弹窗、溢出右侧。属「实现不够完善」（弹窗组件未约束尺寸），按好设计完善实现，非补丁。
    - **修复**：DialogContent 改 `flex flex-col max-h-[85vh] overflow-hidden`（twMerge 干净覆盖 base `grid`）+ header/footer `shrink-0` + 中段 `min-h-0 flex-1 overflow-y-auto`；路径行【绑定/更改目录】按钮 `shrink-0`，路径 span 保持 `min-w-0 flex-1 truncate`——footer 始终可见、路径省略号截断，无横向溢出。
    - **空状态自诊断**：远程下拉为空的数据链路已验证正确（`GET /api/workspaces` 返裸数组 `[{id,name,slug,...}]`，`WorkspaceInfo{id,name}` 字段匹配、多余字段忽略、形状不匹配会报错而非返空）——故「下拉空且无报错」即 server 真返 `[]`。下拉为空时按 `serverWorkspaces.length===0`（去 multica Web 创建）vs `available.length===0`（全部已绑定）两态显示提示文案，不再静默空列表（i18n `noServerWorkspaces`/`allWorkspacesBound` 中英双语）。
    - 验证：tsc 源码零错误，`multica-add-workspace-dialog.test.tsx` 2/2 过，i18n 中英双语补 `noServerWorkspaces`/`allWorkspacesBound`。
  - [x] **M5-k**（本轮）绑定后任务列表/设置页不显示 + 即时可用（三缺陷同修）：
    - **① 渲染 bug**：`MulticaRemoteTaskList` 原 `hasAnyTasks` 总开关 + 组内 `if(!tasks.length) return null`，把「已绑定但当前无任务 / 未 register」的 workspace 整组隐藏并回退「暂无会话」。改为始终按 `vm.workspaces` 成组展示，空组显组内空状态文案（`noTasksInWorkspace`，M5-ab 起居中带垂直留白），仅「无任何绑定」才 `noWorkspacesBound`——绑定后即便 0 任务也看得到工作空间。
    - **② 设置页不刷新（非数据 bug，根因澄清）**：`get_multica_settings` 与 `get_multica_tasks` 读**同一份** RuntimeConfig；下拉能过滤已绑定 workspace 即证数据已落，设置页「显示空」纯因它只在 mount 时 fetch 一次、不订阅事件，绑定发生在任务列表弹窗里收不到通知。新增 `gold-band://multica-settings-updated` 事件（语义=连接/workspace 配置变更，区别于任务生命周期的 `multica-task-updated`），connect/disconnect/save/add/rebind/remove/set_active 统一 emit（connect/disconnect 自 task-updated 迁入），任务列表（订阅 task+settings）+ 设置页（订阅 settings）任一处改动两端同步 re-fetch。
    - **③ register-on-add（绑定即可用）**：register 原仅启动全量跑一次，`add_multica_workspace` 只落配置 → 绑定后须重启才有 runtime_id（任务拉不到、不能 claim）。改为 `add_multica_workspace` async，绑定后即时 `register_workspace_best_effort`（复刻 loop 单 workspace 注册，取回 runtime_id 缓存 `SharedMulticaState`，失败非致命、启动 loop 兜底）。
    - 验证：cargo check 过 + 52 multica 后端单测全过，tsc 零错误，新增 `multica-remote-task-list.test.tsx` 3 用例（空任务显 workspace 组 / 未连接显连接入口 / 订阅两事件）+ 既有 dialog 2 用例共 5 过，i18n 双语补 `noTasksInWorkspace`。
  - [x] **M5-l**（本轮）登录账号可见性 + 切换账号逃生口（浏览器 cookie 账号歧义，码灵侧 Layer 1）：
    - **问题根因**：码灵把认证委托给系统浏览器（`open::that` → 默认浏览器/profile），浏览器 multica session cookie 不在码灵控制内。若浏览器已登录账号 B、用户点【连接】想登 A，multica-webank `/login?cli_callback=...` 见有效 session 直接签发 **B 的 JWT**，码灵静默连成 B。更糟：原 `connect_multica` `let (pat, _user) = browser_login(...)` **丢弃** `UserInfo{email}`，SettingsConfig 无任何账号字段——连错了用户在码灵里也看不出来；且重连仍命中同一 cookie，码灵无换账号能力。
    - **根因 vs 防护**：根因修复（Layer 2）需 multica-webank `/login` 带 `cli_callback`+`cli_state` 时即使有 cookie 也显式 OAuth consent 屏（账号在源头显式确认、可切换），属**独立仓库 multica-webank**（本工作区无其源码），本轮不动。码灵侧做 Layer 1（可见性 + 手动切换）。
    - **① 可见性**：新增 `MulticaAccountRef{name,email}`（生命周期与 PAT 绑定——connect 一起写、disconnect 一起清 → 单结构体统一管理）；`connect_multica` 捕获 `UserInfo` 落盘 `desktop_multica_account`；`MulticaSettingsVm` 暴露 `connectedAccount`（**仅展示用，非凭证**——PAT 仍只暴露 `pat_set`）；`clear_multica_session` 对称清空；设置页连接时显「已连接账号：{name} {email}」。
    - **② 切换账号逃生口**：设置页【切换账号】按钮（ExternalLink 图标 + tooltip）复用现成 `openExternalUrl(appUrl)` 打开 multica Web——用户在浏览器登出当前账号/登录目标账号后回此点【重新连接】。诚实标注：码灵无法强制登出（cookie 浏览器持有），根因换账号体验待 Layer 2 consent 屏。
    - 验证：cargo check 过；lib config 35 测全过（含 account roundtrip 扩展）+ desktop multica 52 测全过（含 clear_session 清 account 扩展）；tsc 零错误；新增 `multica-settings-block.test.tsx` 3 用例（连接显账号+切换按钮 / 未连接不显 / 点切换按钮 openExternalUrl(appUrl)）；i18n 双语补 `connectedAccount`/`switchAccountHint`。
  - [x] **M5-m**（本轮）断开连接清空账号作用域状态（断开后设置页仍显示旧工作空间）：
    - **问题根因**：`clear_multica_session` 原仅清 PAT + 账号身份，**按设计保留** workspace 绑定（注释称其与登录态正交）；而 `MulticaRemoteTaskList` 在 `connected=false` 时直接走空态（不渲染工作空间），`MulticaSettingsBlock` 渲染工作空间只看 `enabled` 不看 `connected` → 断开后「任务列表空、设置页仍展示上个账号绑定的工作空间」的不一致。属设计层歧义：workspace 绑定的 `workspace_id` 由当前账号 PAT 发现、仅登录态下有效，断开/换号后残留即脏数据，并非「与登录态正交」。
    - **决策（账号作用域 vs 机器作用域）**：PAT / 账号身份 / workspace 绑定 / active workspace 统一为**账号作用域**（connect 一起写、disconnect 一起清）；daemon_id 为**机器作用域**（本机持久标识，换账号/重连不变，保留）。数据层根治，杜绝在两处 UI 各打一遍「未连接就隐藏」的补丁式修复。
    - **实现**：`clear_multica_session` 增清 `desktop_multica_workspaces` + `desktop_multica_active_workspace_id`；翻转单测为 `clear_multica_session_clears_account_scoped_state_but_keeps_daemon_id`；同步 `disconnect_multica` 文档注释、browser mock `disconnectMultica`（清 workspaces/active）。前端无需改：断开后 `workspaces=[]`，设置页自然显 `noWorkspaces`，与任务列表空态一致。重连同账号需重新绑定 workspace。
    - **联调备注（问题2，非码灵缺陷）**：清浏览器缓存后重连，浏览器 `/login` 404。实测 `:3000` 即 multica-webank 自身 `next dev`（Next 16.2.6）：`/sitemap.xml`、`/robots.txt` 返 200，但整个 `(auth)` 路由组（`/login`、`/signup`）404，而 `app/(auth)/login/page.tsx` 源码与 `.next/dev` 编译产物均在 → **`next dev` 路由清单陈旧**（路由组在 dev 运行中被新增/移动后未重建内存路由表）。清浏览器缓存无关（缓存不可能让服务端某路由从 200 变 404）。修复：重启 webank web dev（`pnpm dev:web`，必要时先 `rm -rf apps/web/.next`）。码灵侧无误：`browser_login` 打 `<app_url>/login?cli_callback=&cli_state=`，登录页（一旦能服务）正确回跳 `127.0.0.1:<port>/callback`。
    - 验证：desktop multica 52 测全过（含翻转后的 clear_session 用例），前端 multica 3 文件 8 测全过；lib config 本轮未改动（仍 35 测）。
  - [x] **M5-n**（本轮）selective claim 落地 + pending 标题回填 + 码灵字段对齐（multica-webank 后端 + 码灵 client，开发设计 §1.2）：
    - **S1 selective claim 接口（multica-webank）**：按开发设计 §1.2 规格全量落地——`ClaimSpecificQueuedTask`(agent.sql) 逐字保留 `ClaimAgentTask` 的 per-(issue,agent) `NOT EXISTS` 串行化守卫 + `prepare_lease_expires_at`，仅候选过滤换为精确 `(task_id, runtime_id, 'queued')`；`ClaimSpecificTask`(task.go) runInTx 复用 dispatch 三件套（captureTaskDispatched/ReconcileAgentStatus/broadcastTaskDispatch），不做容量/reclaim/候选循环；`ClaimSpecificTask`(daemon.go) 完整复刻 `ClaimTaskByRuntime` 六步收尾链，唯一改动为 FIFO→specific + nil→404（区别于 FIFO 的 200 `{task:null}`）；router 注册 `POST /runtimes/{runtimeId}/tasks/{taskId}/claim`；`sqlc generate` 生成 `ClaimSpecificQueuedTaskParams{PrepareLeaseSecs,TaskID,RuntimeID}` + 方法。
    - **pending 列表回填 thread_name**：`ListPendingTasksByRuntime`(daemon.go) 循环内对 `t.IssueID.Valid` 的 task 回填 `resp.ThreadName = issue.Title`（复刻 buildClaimedTaskResponse 同款填法；taskToResponse 是纯 mapper 无 DB，故落在 handler）。修复 pending 列表只显「queue」、无标题的缺口。
    - **码灵字段对齐**：`RemoteTask.title`(client.rs) 加 `#[serde(rename="thread_name")]`——webank 权威源 `AgentTaskResponse.ThreadName`→JSON `thread_name`（claim 响应与 pending 列表均带此键）。Rust 字段名沿用语义化 `title`，仅 serde key 对齐；下游 `task.title` 消费点 / VM 输出 camelCase 不受影响。新增单测 `remote_task_reads_thread_name_wire_key` 锁定 wire 契约。
    - 验证：webank `go build`/`go vet` 过；4 新集成测 `TestClaimSpecificTask_{ClaimsExactQueuedTask,NonQueuedReturns404,CrossRuntimeReturns404,SerializationConflictReturns404}` 全过；现有 claim/pending/batch-claim 测试零回归。码灵 desktop multica 53 测全过（+1 新契约测）。
    - **范围外既有缺陷（非本批次引入，留待单独修复）**：webank `TestParseSkillArchive_RejectsUnsafeSkillMdPath` 在 Windows 失败——`validateFilePath`(skill.go:252) 用 `filepath.IsAbs`（OS 语义），Windows 上 `/abs/SKILL.md` 非绝对路径不被拒；属 zip-slip 校验跨平台 bug（`parseSkillArchive` 用 `path.Clean` 但委托给 filepath 语义校验，不一致），与本批次无关。
  - [x] **M5-o**（本轮）远程任务三缺陷同修（折叠 / 任务名 / 会话可见化，根因均为前端导航+刷新+UI 实现不完善，非持久化缺失）：
    - **问题1·列表按工作空间折叠**（纯 UI 缺失）：`MulticaRemoteTaskList` 原把 workspace 渲染成固定不可点 `<div>`，无折叠态。镜像本地 `ConversationSidebar` 现成折叠模式（`expandedWorkspaces` state + `ChevronDown` 旋转 + `<button>` header + 条件渲染），workspace 分组 / pinned 失败段 / 新增「最近完成」段**三段均可折叠**，两套 UI 视觉一致（不用 shadcn Collapsible，跟随本地侧栏手动 toggle 风格）。
    - **问题2·远程任务显示任务名**（两层缺陷，修根因非补丁）：
      - **Layer A（webank 后端）**：M5-n 的 pending 列表 `thread_name` 回填**只覆盖 issue 来源**，漏 chat / autopilot / quick-create 三类（claim 路径 `buildClaimedTaskResponse` 覆盖全部 4 来源）。新增聚焦 helper `resolvePendingThreadName(ctx, AgentTaskQueue)`，按任务来源**互斥分支**镜像 claim 路径的名字源（issue→`GetIssue.Title` / chat→`GetChatSession.Title` / autopilot→`GetAutopilotRun`→`GetAutopilot.Title` / `QuickCreateContext`→`Prompt` / 全空→`""`），替换 `ListPendingTasksByRuntime` 的 issue-only 块。**不重构** 600 行 `buildClaimedTaskResponse`（thread_name 交织在 squad/repos/chat 投递里，抽出高风险低收益）——helper 是 pending 列表专用的名字解析对应物，注释明确这层关系。
      - **Layer B（desktop 前端）**：`vm.rs::from_remote` 原空 `thread_name` → `unwrap_or_default()` 产空串（`client.rs:130` 注释声称「缺失用 id 兜底」但**从未实现** → 整行标题空白）。改为 `title.filter(!trim empty).unwrap_or_else(task.id)`，兑现既有注释承诺。
    - **问题3·multica 任务会话可见化**（**transcript 已完整落盘**，`finalize_terminal` 不删除；根因是前端导航+刷新缺陷，三层叠加）：
      - **根因①**：`MulticaRemoteTaskList` 用 `onSelectTask` → App `onConversationSelectTask` 在**陈旧** sidebar 快照里查刚创建的任务找不到 → runId undefined → 静默 no-op（不导航）。
      - **根因②**：`multica-task-updated` 事件只被远程任务列表订阅，App/本地侧栏不监听 → multica 任务创建/完成时本地侧栏永不刷新（正常 `createConversationRun` 在 App 显式 `getConversationSidebar`，multica 路径跳过）。
      - **根因③**：`start_multica_remote_task` 只返回 `local_task_id: String`，**丢弃内部全程持有的 `local_run_id`**，前端无 runId 可导航。
      - **修复 Layer A（后端先定接口）**：`start_multica_remote_task` 返回类型 `String` → `MulticaRemoteTaskStartedVm { local_task_id, run_id }`（camelCase `{localTaskId, runId}`）；闭包多回一路 `local_run_id`（Resume/Fresh 两分支均已绑定）。
      - **修复 Layer B（前端直达+刷新）**：`MulticaRemoteTaskList` props 改 `onSelectRun(projectId, taskId, runId)`（复用本地侧栏直达指定 run 的现成回调），`handleClaimAndStart` 拿 `{localTaskId, runId}` → `onSelectRun(localProjectId, localTaskId, runId)`；`ConversationSidebar` 挂载处 `onSelectTask` → `onSelectRun`（绕开陈旧快照查找的根因①）；App 新增订阅 `multica-task-updated` → `getConversationSidebar`+`applyConversationSidebar`（in-flight/pending 去抖，对齐 agent-registry 模式）→ multica 任务创建即进本地侧栏、完成即更新（根治根因②）。
      - **修复 Layer C（远程 tab「最近完成」回看，用户选定：远程 tab 也保留）**：`finalize_terminal` 原完成即清 `multica_task_conversations[remote]` → remote↔local 链接丢失无法回看。新增 StateConfig 字段 `multica_completed_tasks: Vec<MulticaCompletedTask>`（`{remote_task_id, local_task_id, local_run_id, workspace_id, issue_id, status, title, completed_at}`，有界 N=50、最新在前、按 `remote_task_id` 去重）；`finalize_terminal` 移除 active 前快照一行进 completed（status 来自 PendingUpdate：ClearOnSuccess→completed / AddOnFailure→failed；title 来自 `ActiveRemoteRun.title`）；`ActiveRemoteRun` 加 `title: Option<String>`（claim 时设，finalize 无需读 task.json 即可拿行标签）；`RemoteConversationSidebarVm` 加 `recently_completed: Vec<MulticaCompletedTaskVm>`（`project_id` 由 `workspace_id→local_project_id` 经 workspaces 解析，供 `onSelectRun` 直达）；`MulticaRemoteTaskList` workspace 分组下方加可折叠「最近完成」分区，行显示 title+status badge+completed_at，点击 → `onSelectRun` 直达本地会话。
    - **根治「会话看不到 + 完成后消失」**：执行时 Layer A+B 直达+刷新 → 实时看到会话；完成时 Layer C 在远程 tab「最近完成」常驻（事件触发 re-fetch 即时同步）+ 本地侧栏常驻（磁盘 task.json）→ 两处可回看完整 transcript。
    - 验证：desktop multica 58 测全过（+`from_pending_falls_back_to_id_when_title_missing`/`remote_task_started_vm_serializes_camel_case_keys`/`completed_task_vm_serializes_camel_case_keys` + `record_completed_task_*`），lib config 36 测全过（+`state_config_multica_completed_tasks_roundtrip_json`）；web vitest `multica-remote-task-list` 6 测全过（折叠 / claim→onSelectRun / recentlyCompleted 渲染+点击），tsc 源码零错误；webank 4 pending thread_name 测全过（issue/chat/autopilot/quick-create 各一）。**部署提示**：Layer A（webank 4 来源 backfill）需 rebuild + 重启 webank server；agent-browser 端到端验证待跑（需运行中的 webank server + 桌面端连接 multica）。
    - i18n 双语补 `conversation.sidebar.multica.recentlyCompleted`（zh-CN「最近完成」/ en「Recently completed」）。
- [x] **M5-q**（本轮）Req D 演示反馈两项调整（心跳常驻 + claim-at-click 执行流程，开发设计 §2.5/2.6/2.8/4.3/4.4）：
  - **背景**：完整流程演示后同事提出两点调整：① 与 multica 建立连接后应始终保持心跳，而非仅任务执行时；② 点【执行】后不应直接拉起会话，应进入与本地「+」相同的 composer/prepare 页（输入框预填远程任务 requirement），用户选模型/模式后点【发送】才会话开始执行，尽可能复用本地会话创建流程。
  - **① 心跳常驻（D.1）**：`loop_.rs` 心跳源由 `collect_active_runtime_ids`（仅 `active_runs`）改为 `collect_all_runtime_ids`（读 `state.runtime_ids()`，所有已连接 workspace 的 runtime）--连接后即持续在线、无任务也心跳；同 tick 新增 `extend_prepare_leases` 对 claim-at-click 后未 start 的任务续期 prepare lease（防 server 45s 回收）。`MulticaRuntimeState` 增 `prepare_leases: HashMap<remote_task_id, PrepareLease>` + `register/drop/snapshot/prepare_lease` 方法；`runtime_ids()` 公开供心跳遍历，`active_runtime_ids()` 保留给 cancel 检测。
  - **② 执行流程改造（D.2，claim-at-click）**：删除原 `start_multica_remote_task`（原子 claim+start）+ `resume_multica_task` + `MulticaRemoteTaskStartedVm`；拆为 `claim_multica_task`（只 claim + 写 prepare lease + 回 requirement，不立即 start）-> 前端预填 composer -> `start_multica_conversation_run`（**复用本地 `create_conversation_run_vm(&App, &ConversationCreateInputVm)`** 构造会话 + `POST /tasks/{id}/start` -> running + 桥接 lifecycle 转译 + run outcome complete/fail）；会话级续跑并入 `start_multica_conversation_run` 的 `classify_resume` 分支（命中先前未终态 local run -> Resume，否则 Fresh）。放弃 compose 调 `cancel_multica_prepare_lease`（sync）任务回 queued。前端 `MulticaRemoteTaskList` `handleClaimAndStart`->`handleClaimAndPrepare`（claimMulticaTask -> prefill composer draft -> onNewConversationInWorkspace）；composer draft 增 `multica` 绑定（`{remoteTaskId, workspaceId, localProjectId}`）app-root hoisted 跨导航存活；`App.tsx` onSubmit 分支 `draft.multica ? startMulticaConversationRun : createConversationRun`。
  - **验证**：60 cargo multica 单测全过；web vitest `conversation-composer-draft` + `multica-remote-task-list` 用例更新全过；tsc 零错误。**无 webank 改动**（prepare-lease 续期路由 + claim requirement 字段 server 侧已具备，码灵 client 仅解析）。联调 agent-browser 端到端验证待跑（用户自测）。

- [x] **M5-r**（本轮）联调发现两缺陷同修（心跳半空转 + start_task 失败孤儿 run）：

  **Fix 1 · 心跳自愈注册（根因修复）**
  - **根因**：原来 register 只启动全量跑一次 + workspace 绑定时注册；`connect_multica` 不注册 → 连接后首心跳空转；心跳 404 清 runtime_id 后不自愈 → 永不长连。
  - **修复**：
    - `state.rs` `runtime_id_pairs()` 返回 `Vec<(workspace_id, runtime_id)>` 供心跳循环 workspace 上下文自愈；`clear_runtime_id(workspace_id)` 单 key 定点清除
    - `loop_.rs` 抽取共享 helper `register_workspace(client, workspace_id, provider, daemon_id, shared)`，`run_heartbeat_loop` 在每次 tick 先调 `self_heal_registration` 尝试补注册缺失的 runtime_id，再对各 workspace 发送心跳；404 时 `clear_runtime_id` 清除 stale 条目 → 下一 tick 自愈自动补
    - `commands.rs` `connect_multica` 调 `register_all_bound_workspaces` 即时注册所有已绑定 workspace（claim 依赖 runtime_id，注册完成前不返回）
  - **验证**：`runtime_id_pairs_carries_workspace_for_self_heal` + `clear_runtime_id_singular_drops_one_keeps_rest` 单测通过。

  **Fix 2 · start_task 失败处理**
  - **根因**：prepare-lease 过期→server 回收→`start_task` SQL 拒绝非 dispatched 态→本地 run 孤儿（有 active_run 映射但无远端对应）+ `complete_task` 失败（SQL 要求 running）→ 远端 pending/queued 永生。
  - **修复**：`commands.rs` `start_multica_conversation_run` 的 Ok 分支在 `start_task` 失败时立即 `fail_task(remote, "local start failed", "agent_error")` + `teardown_active_run`（`run_pause(ProcessInterrupted)`+杀 ACP+清 active_run 映射）
  - **验证**：逻辑内联在既有命令流中，不增加新命令，单测通过。

- [x] **M5-s**（本轮）取消「显式完成远程任务」按钮，完成语义回归生命周期驱动：
  - **背景**：M5-r 曾设想非 direct 模式暂停后由用户点按钮显式完成远端任务（`complete_remote_task_explicit` + `complete_multica_task` 命令 + `get_multica_active_run` 反查 + `MulticaActiveRunVm` + 前端 CircleCheck 按钮）。评审后决定**不加按钮**：完成动作应跟随 run 生命周期自然发生，而非引入额外的手动终态入口。
  - **完成语义（删除按钮后，沿用既有 `bridge` 终态分支，无新代码）**：direct 模式 agent 跑完 -> `RunCompleted{Success}` -> `handle_run_completed` -> `classify_terminal(Success)` -> `complete_task` + `finalize_terminal`；workflow 模式 run 跑到终态同样经 `RunCompleted{Success}` -> `complete_task`。「按终止来完成」即完成由 run 终止事件驱动，不另设手动入口。
  - **已知缺口（已接受）**：open-ended workflow 在 interview/round 节点 `RunPaused`（等人工判定）时 `RunCompleted` 不 fire -> 远端保持 running；无手动完成入口，由 server sweeper（running-stuck 2.5h）兜底转 failed。agent-driven 模型下「暂停等人工」的预期代价，暂不补。
  - **删除清单**：`bridge.rs` `complete_remote_task_explicit`；`commands.rs` `complete_multica_task` + `get_multica_active_run`；`vm.rs` `MulticaActiveRunVm`；前端四层 `completeMulticaTask`/`getMulticaActiveRun` + `ConversationRunHeader`/`ConversationRunPage` `onCompleteMulticaTask` + `App.tsx` active-run useEffect + `types.ts` `MulticaActiveRunVm` + `i18n.ts` `completeRemoteTask`。
  - **验证**：`cargo check --workspace` 通过；`cargo test -p gold-band-desktop multica::` 62 测全过；前端 tsc 源码零新增错误（既有 tests/ 类型漂移基线与本次无关）。

- [x] **M5-t**（本轮）完成远程任务后码灵用 PAT 把关联 issue 流转到 done（接入方案 D2，选项 B：码灵作为中介）：
  - **决策（选项 B 而非 A）**：issue 完成流转由**码灵**用自身 PAT（`mul_` 用户级）调 multica issue 状态 API，而非让 agent 直调 multica API（选项 A 的 MULTICA_TOKEN 路径）。码灵作为 daemon 已持 PAT、自然适合做 issue 状态推进的中介；agent 仅负责执行，不应感知 multica 业务接口（与既有「ACP agent 从不直接调 multica API」约束一致）。
  - **数据（先定数据）**：`client.rs` 新增 wire type `UpdateIssueStatusRequest { status }`（`PUT /api/issues/{id}` body）+ 常量 `MULTICA_ISSUE_DONE_STATUS = "done"`。
  - **接口（再定接口）**：`MulticaClient::update_issue_status(workspace_id, issue_id, status) -> Result<()>`，`PUT /api/issues/{issue_id}` body `{status}`，带 `X-Workspace-ID` 头（issue 维度路由），走 `with_network_retry`（3 次）。
  - **实现（最后补实现）**：bridge `handle_run_completed` 的 `Success` 分支——`complete_task` **送达成功（Ok）**后，若 `ActiveRemoteRun.issue_id` 非空，调 `update_issue_status(workspace_id, issue, "done")`。失败仅 `warn`（任务终态已上报，issue 状态推进不阻断完成；issue 保持原状，server 扫描器/用户兜底）。**不在 `complete_task` 失败时流转**（server 未收到 complete → issue 不应变 done）。
  - **重构（杜绝第三份重复）**：issue PUT 拉入第三种 JSON 请求方法。把既有 `post_json` / `post_json_with_workspace` 的「auth + send + map_status」重复模板上提为共用底座 `json_send(method, path, workspace: Option, body)`，三个调用方（POST 无头 / POST 带头 / PUT 带头）全部走它，避免补丁式复制。行为保持不变（既有测全回归）。
  - **验证**：`cargo check --workspace` 通过（仅 4 个既有死代码警告，与本次无关）；`cargo test -p gold-band-desktop multica::` **65 测全过**（新增 3：`MULTICA_ISSUE_DONE_STATUS` 常量锁定 / `UpdateIssueStatusRequest` body `{status}` / PUT path 形状）。**无需 webank server 改动**（issue 状态 API 既有，码灵 client 补调用即可）。

- [x] **M5-u**（本轮）断点续跑落地：远程任务被 server 重派（崩溃恢复 / agent 死亡回收 / 失败重试）后重新执行时，续既有本地 run（而非每次从第 1 轮重跑），并消除 claim/start 对「续跑 vs 新建」的不一致：
  - **根因（非补丁，补完消费者）**：断点索引 `multica_task_conversations[remote_task_id]`（{local_task_id, local_run_id, session_id, work_dir}）早已在 Fresh 成功时写入、bridge 在 NodeCompleted 回填 session_id——数据结构就位；但消费它的 `classify_resume` 在改 composer 流时被删，再无人用 checkpoint 把本地 run 续起来。同时 `claim_multica_task` 只要 session_id 非空就传 `prior_session_id`，可紧接的 start 却 Fresh 建新 session，claim/start 不一致。属「设计对了、实现没补完」。
  - **机制前提（已确认）**：① ACP session 跨码灵完整重启可冷重连（adapter 自持 session，`session/load`）；② 本地续跑链 `run_continue_background(task_id, run_id, prompt_id, prompt)` → orchestrator 读 `worker-ref.json` `continue_ref.acpSessionId` → `SessionMode::Continue`；③ `is_run_continuable`（app/mod.rs:711）= Paused + outcome None + pause_reason ∈ {ProcessInterrupted, RuntimeAbnormal, WaitingForUserInput} + round/node/attempt 齐；④ 启动自愈 `recover_interrupted_running_sessions`（main.rs）把 stale-Running 翻成 Paused + ProcessInterrupted → 崩溃重启后旧 run 自动可续——**home repo 由启动自愈覆盖；multica 各 work_dir 由 `recover_multica_work_dir_sessions` 补全（M5-ac，见下）**。
  - **数据/接口**：新增纯函数 `classify_resume(home_app, remote_task_id) -> ResumeDecision{Resume{local ids, session_id} | Fresh}`（读 checkpoint → 由 work_dir 构造 workspace App → is_run_continuable），`claim` 与 `start` 共用。`claim` 的 `prior_session_id` 改为仅 Resume 命中才传；`start` 命中 Resume → `run_continue_background(prompt=None 纯续跑)` + `register_active_run`(既有 ids) + `start_task(false)` + 从既有 run 还原 `ConversationRunVm`；Err/无可续 → 落 Fresh。
  - **决策**：D1 续跑文本=纯续跑（prompt=None，接着被中断的编排跑，不新增 turn、不换模型）；D2 stale-Running 未重启时不主动 pause-then-resume，仅 is_run_continuable 命中才续（崩溃重启主路径由启动自愈覆盖）。
  - **顺带修旧漏**：Fresh 覆盖既有 checkpoint 时重置 `session_id=None`（旧 session 随旧 run 失效；新 run 的 session_id 待 bridge 回填）。
  - **验证**：`cargo check` 通过（仅 4 既有警告）；`cargo test -p gold-band-desktop multica::` **72 测全过**（新增 7：`classify_resume_from` 纯逻辑——无 checkpoint / session 空 / run 不可达 / Paused+ProcessInterrupted 命中 Resume / Running→Fresh / 非可续 reason / 缺 locator）。无需 webank server 改动。

- [x] **M5-v**（本轮）改动四：issue 正文预填 composer（multica-webank server + 码灵 client，开发设计 §12.6）：
  - **背景**：码灵 claim 到远程分配的 issue 后，composer 只预填 issue 标题（thread_name），而非 issue 正文——只见一行标题、不见需求内容。
  - **根因（两层）**：① webank `buildClaimedTaskResponse` issue 分支只回填 `ThreadName=issue.Title`，丢弃 `issue.Description`，`AgentTaskResponse` 无 issue 正文字段；② 码灵 `requirement_text()` issue 型无正文来源 → 回退 title。
  - **修复 Layer 1（multica-webank）**：`AgentTaskResponse` 加 `IssueDescription string`（与 `ProjectDescription` 对称的 issue 级 durable 正文）；`buildClaimedTaskResponse` issue 分支回填 `resp.IssueDescription = issue.Description.String`（pgtype.Text NULL→""）。
  - **修复 Layer 2（码灵 client）**：`RemoteTask` 加 `issue_description`；`requirement_text()` 优先级在 handoff_note 后、title 前插入 issue_description（issue 型取正文，无正文才回退 title）。
  - **验证**：webank `TestClaimTaskByRuntime_IssueDescription` 过；码灵 requirement_text 优先级单测扩 issue_body / handoff_over_body。**需 rebuild + 重启 webank server**。

- [x] **M5-w**（本轮）改动五：开始执行时 issue 流转到 in_progress（码灵 client，开发设计 §12.7）：
  - **背景**：码灵执行 issue 时，multica 看板该 issue 卡片仍停「待办」列（只挂「正在进行」badge），未流转「进行中」列。
  - **根因（补完对称缺失，server 无 bug）**：multica `issue.status` 是 agent/user-driven（非 task 生命周期驱动）——`start_task`/`complete_task` 显式不动 issue.status。码灵此前只在**完成**时流转（done，§M5-t），**开始**时未流转。看板列由 issue.status 派生、「正在进行」badge 由 task 队列状态派生（两者独立→错位）。enum 与 `PUT /api/issues/{id}` 早已接受 in_progress，码灵 client 已有 `update_issue_status`（§M5-t）——缺的只是「开始时调一次」。
  - **修复（码灵 commands.rs，纯 client）**：`start_multica_conversation_run` Resume/Fresh 两分支、`start_task` 成功后、返回前调 `mark_issue_in_progress(.., MULTICA_ISSUE_IN_PROGRESS_STATUS)`——复用 §M5-t 的 `update_issue_status`，与完成 done 对称。失败仅 warn（不阻断执行）。纯函数 `in_progress_target`（过滤 None/空白）+ I/O 包装，镜像 classify_resume 模式。
  - **验证**：`cargo test -p gold-band-desktop multica::` **75 测全过**（+`in_progress_status_constant_is_in_progress` / `in_progress_target_skips_absent_or_blank_issue`）。**无需 webank server 改动**。

- [x] **M5-x**（本轮）改动六：终态任务留在所属工作空间列表（开发设计 §12.8）：
  - **背景**：不同 workspace 的已完成任务全挤在左侧远程列表单一全局「最近完成」列，可读性差；用户希望完成后仍留在各自 workspace 列表里，只标 completed/failed。
  - **根因（设计迭代）**：M5-o 把终态历史放扁平全局桶（`recentlyCompleted`）跨 workspace 混排；改动六按 workspace 归组（与 active 同列）。
  - **修复（破坏式更新，删旧路径无兼容层）**：VM 删 `recently_completed` + `MulticaCompletedTaskVm`，`RemoteTaskVm` 加 `local_task_id/run_id/project_id`（终态行才填）+ `from_completed`；`get_multica_tasks` 按 workspace_id 把 `multica_completed_tasks` 解析 project_id 后并入对应工作空间 `tasksByWorkspace`（`merge_workspace_tasks` 纯函数，active 优先、remote_task_id 去重；未绑定 workspace 跳过）；前端删「最近完成」分区 + CompletedTaskRow，终态行整行可点 → onSelectRun 直达本地会话；types/i18n/browser mock 同步。顺带把 `WorkspaceShell.onNewConversationInWorkspace` 由误标可选改必填（与下游必填声明一致）。
  - **验证**：`cargo test -p gold-band-desktop multica::` **75 测全过**（+`merge_workspace_tasks_*` / `from_completed_*`）；web vitest `multica-remote-task-list` 用例更新全过；tsc 零错误。**无需 webank server 改动**。
  - **既有无关失败（非本批次）**：`gold-themed-scrollbar` 测试在 Windows 因 `core.autocrlf` 把 styles.css 转 CRLF、测试硬编码 `\n` 的 dark 双行选择器失配而失败——跨平台行结尾测试健壮性问题，与 multica 无关。

- [x] **M5-y**（本轮）改动七：执行中的远程任务在侧栏可见（开发设计 §12.9）：
  - **背景**：码灵点远程任务开始执行后，该任务从左侧远程列表消失，直到结束才回来；用户希望执行中任务也展示，标识区分（进行中）。
  - **根因（实现不完整）**：`get_multica_tasks` 列表只有 pending（server queued）+ terminal（本地 completed_tasks）两个来源；"正在执行"的任务处在 `active_runs`（已领取、未终态），落进空档消失。前端早预留 running（`STATUS_VARIANT` / `canCancel`），后端从不产出 running 行。同源缺：start 成功路径不发 `multica-task-updated`（侧栏不即时刷新）；徽标渲染原始 `{task.status}`（无 i18n）。
  - **修复（纯客户端，无 webank server 改动）**：`vm.rs` 新增 `from_active_run`（running 行 + 本地链接 + started_at→last_activity_at）；`get_multica_tasks` 单次取锁取 active_runs running 行并入 workspace 组，`merge_workspace_tasks` 升三参 running/pending/terminal（running 优先去重）；`start_multica_conversation_run` 成功路径补 `emit_multica_task_updated` 让 running 行即时刷出；徽标改 i18n `status.{queued,running,completed,failed}`（zh 待办/进行中/已完成/失败，M5-aa 起 queued 文案对齐看板列名「待办」、替换原暗示中间态的旧词）。
  - **验证**：`cargo test -p gold-band-desktop multica::` **76 测全过**（+`from_active_run_*`；merge 用例改三参）；web vitest `multica-remote-task-list` **9 用例全过**（+running 行渲染/点击/Cancel）；tsc 零错误。**无需 webank server 改动**。

- [x] **M5-z**（本轮）绑定模型下沉到任务级 + 远程任务管理独立页（重构）：
  - **背景**：原设计「添加工作空间时一次绑定本地目录（workspace 级 `local_project_id`）+ rebind 改绑 + 侧栏本地/远程 toggle」三件套。问题：同一远程 workspace 想换本地目录执行必须先 rebind（割裂）；远程任务挤在会话侧栏 segmented toggle 里（与本地任务列表争抢侧栏空间、视觉混乱）；workspace 级绑定把「团队边界」与「执行目录」过早耦合。属设计层面需重构（CLAUDE.md：宁愿多付成本修设计缺陷）。
  - **核心：绑定模型下沉到任务级**：
    - `MulticaWorkspaceRef` 结构**移除 `local_project_id` 字段**（只剩 `id, name, slug, provider`）。远程 workspace 在「添加工作空间」时**只绑 provider**。
    - 本地工作目录推迟到**每次执行时**由 composer 下拉选择；选中的本地工作区在发送时随 `start_multica_conversation_run` 写入任务级生命周期结构：**`ActiveRemoteRun` 和 `MulticaCompletedTask` 各自新增 `local_project_id` 字段**。
    - 删除 `binding_for_multica()` 查找函数（原 workspace → `local_project_id` → `conversation_workspaces.workspace_path`）。`invalidate_remote_task` 改为直接用 `workspace_entry_for_project(&home_state, &run.local_project_id)` 解析路径（任务自带本地目录）。
  - **删除 rebind 全链路（破坏式更新，无兼容层）**：命令 `rebind_multica_workspace`、API 四层（client.ts/desktop.ts/browser.ts/api.ts）的 `rebindMulticaWorkspace`、前端 `handleRebind` 全部删除。`add_multica_workspace` 命令签名去掉本地目录参数（只剩 `workspace_id + provider`；name 从 server 列表取）。前端 `addMulticaWorkspace(workspaceId, workspaceName, provider)` 三参。
  - **添加工作空间弹窗**：`MulticaAddWorkspaceDialog` 只收「远程工作空间 + provider」两个下拉，不再有本地目录 folder picker、不再调 `pickLocalDirectory`。
  - **侧栏去本地/远程 toggle，独立成页**：会话侧栏 (`ConversationSidebar`) 移除「本地/远程」切换 UI（删 `localTab`/`remoteTab` 文案），侧栏纯本地任务。multica 远程任务管理改为**独立的「远程任务管理」整页**（新页面 `MulticaTaskManagementPage`、新路由 `/chat/multica-tasks`，与 agent 管理/上下文管理/运行模式管理 并列的导航项，icon=Globe），内容镜像 `MulticaRemoteTaskList`。仅会话模式（新 UI）有此页，工作台（旧 UI）不做双胞胎。
  - **composer 执行时选本地工作区（claim-at-click → 执行时落地）**：在远程任务管理页点 play → `claimMulticaTask` 领取（后端登记 45s prepare lease，常驻心跳续期）→ 预填 composer（正文 + multica 绑定 `{remoteTaskId, workspaceId}`，**不含 localProjectId**）→ 落 conversation-home。App 预选最近活跃本地工作区（`activeWorkspaceId ?? lastActiveWorkspaceId`），composer **强制显示本地工作区下拉**（即便只有 1 个，只要 multica 绑定激活就强制显示），用户可改；改工作区时**保留 multica 绑定与预填正文**。点击发送 → `startMulticaConversationRun`（用下拉选中的 `projectId`）→ 该远程任务出现在本地侧栏对应工作区。
    - 边界：当 0 个本地工作区时，composer 显示「请先添加本地工作空间」引导并禁用发送。
  - **i18n**：新增 `multica.taskManagement.{title,subtitle}`、`conversation.sidebar.multicaTaskManagement`、`conversation.composer.multicaNeedLocalWorkspace`（中英双语）；删除已失效的 `conversation.sidebar.multica.localTab/remoteTab`、dialog 目录相关 key（bindDirectory/changeDirectory/directoryPlaceholder/needDirectory）、`settings.multica.{connecting,disconnecting,connected,disconnected,addWorkspace,selectServerWorkspace,selectProvider,selectFolder,rebind,notGitWarning}`。
  - **验收标准**（已固化）：① 远程 workspace 添加只收 provider；本地目录执行时选。② 同一个远程 workspace 可在不同本地目录重复执行（每次执行独立落地）。③ 侧栏纯本地；远程任务在独立整页管理。④ claim-at-click：点 play 即领取 + 预填，发送才真正 start。⑤ rebind 全链路删除，无兼容层（开发阶段破坏式更新）。

- [x] **M5-aa**（本轮）远程任务管理页 5 项前端打磨（看板词汇对齐 + 状态色调徽章 + 手动刷新 + 副标题精简 + 任务来源下拉，纯前端无 Rust 改动）：
  - **心智**：码灵作为 multica Daemon 角色，自身即驱动 board issue.status（start 时 in_progress、complete 时 done），故本地任务生命周期与 multica 看板词汇 1:1 对应——canonical 状态 `queued|running|completed|failed` 的展示文案直接采用看板词汇。
  - **① 状态词汇对齐看板**：`queued` 旧文案暗示独立的「领取」中间态（与 claim-at-click 语义冲突），现对齐看板列名为「待办」（Todo）；`running=进行中`/`completed=已完成`/`failed=失败` 不变。后端 canonical 状态不变，仅改 canonical→display 文案映射（i18n `conversation.sidebar.multica.status.queued` zh「待办」/ en「Todo」，替换原暗示中间态的旧词），不读看板实时状态。
  - **② 状态色调徽章（`MULTICA_STATUS_TONE`）**：状态词以有色 Badge 呈现，色调按看板词汇锁定并集中管理为导出常量 `MULTICA_STATUS_TONE: Record<string, string>`（`web/src/components/conversation/MulticaRemoteTaskList.tsx`）：`queued`=灰（`bg-muted text-muted-foreground`）/ `running`=黄（`bg-amber-500/15 text-amber-600 dark:text-amber-300`）/ `completed`=绿（`bg-emerald-500/15 text-emerald-600 dark:text-emerald-300`）/ `failed`=红（`bg-destructive/15 text-destructive`）；每个 canonical status 一个 tone class，经 `Badge` className 应用（杜绝硬编码），缺键回退 `queued` 灰。
  - **③ 手动刷新**：`MulticaRemoteTaskList` 顶部右侧新增 ghost 图标按钮（`RotateCw`，`aria-label={t('common.refresh')}`，Tooltip 同文案），点击调既有 `fetchTasks()`（getMulticaTasks，与 mount 和 `multica-task-updated`/`multica-settings-updated` 事件订阅同源）；`refreshing` 态驱动 `animate-spin` 并 disable 按钮（不复用 `loading`——loading 会用整屏 spinner 替换列表）。免切走/重进即可拉最新任务列表。
  - **④ 副标题精简**：`multica.taskManagement.subtitle` 删除原尾部关于「执行时选本地目录」的赘述半句（该语义已被 claim-at-click → composer 执行时选本地工作区流程覆盖，副标题不再赘述实现细节）。**M5-ab 进一步删除 "multica" 限定词**（页头「任务来源」下拉已点名来源），最终副标题见 M5-ab。
  - **⑤ 任务来源下拉（多来源切换位）**：远程任务管理页页头新增「任务来源」Select（i18n `multica.taskManagement.source.label` zh「任务来源」/ en「Task source」），当前唯一项 Multica（`multica.taskManagement.source.multica`）。页级 `source` state 是渲染分流唯一键（`source === 'multica' ? <MulticaRemoteTaskList/> : null`），由 `REMOTE_TASK_SOURCES` 配置数组（`web/src/pages/MulticaTaskManagementPage.tsx`）驱动；新增来源 = 向数组加一项 + body 按 `source` 分支渲染（各来源自带数据/刷新）。本期仅落地 multica，切换位为未来多来源接入保留。
  - **验证**：纯前端，无 Rust/webank server 改动；i18n 中英双语同步（`status.queued` zh「待办」/en「Todo」、`multica.taskManagement.subtitle` 精简、`multica.taskManagement.source.{label,multica}` 新增、`common.refresh` 复用既有）。

- [x] **M5-ab**（本轮）远程任务列表「树形视觉/层级系统」统一打磨（组头图标统一 + 工作空间图标 + 任务计数 + 组头 hover 底色 + 分组间距 + 任务行排版 + 空状态 + 间距系统统一，纯前端无 Rust 改动）：
  - **心智（统一一次做完，非 8 个补丁）**：远程任务管理页 workspace→任务树状列表此前各项视觉参数（图标尺寸/颜色、组头 hover、组间距、任务行排版、空状态）散落各自一套、未成体系。本轮作为一次统一的「树形视觉/层级系统」打磨：组头 = 可折叠的 workspace 容器（Server 图标 + 名称 + 任务计数），任务行 = 其下的叶子节点（标题为主、元信息为辅），通过统一水平缩进、垂直节奏、hover 反馈让「workspace 容器 vs 任务叶子」层级一眼可读。仅 `MulticaRemoteTaskList.tsx`（会话模式远程任务管理页）改动，工作台/旧 UI 与本地侧栏不受影响。
  - **① 副标题再精简**：`multica.taskManagement.subtitle` 在 M5-aa 版（已删尾部实现细节赘述）基础上**进一步删除 "multica" 限定词**，再精简为「查看并执行远程任务」/ "View and run remote tasks"——页头「任务来源」下拉已点名来源（当前唯一项 Multica），副标题不再重复 "multica" 限定词（UI 不展示冗余信息）。
  - **② 组头图标统一 + 工作空间图标**：workspace 分组头与 pinned 分组头采用**统一折叠箭头规格**（`ChevronDown` size-3.5 text-muted-foreground，旋转表达展开/折叠）；workspace 名称左侧新增 `Server` 图标（lucide，同 size-3.5 text-muted-foreground）——「服务器图标 = workspace 容器行」与下方「无图标 = 任务叶子行」形成视觉区分；pinned 段不是 workspace、不加 Server 图标。
  - **③ 任务计数**：workspace 名称右侧展示轻量任务计数 `（N个任务）`/`(N tasks)`（i18n 新键 `conversation.sidebar.multica.taskCount`，text-[11px] text-muted-foreground），仅在该 workspace 有任务时显示（0 任务由空状态文案表达，避免冗余）。
  - **④ 组头 hover 底色**：workspace 分组头与 pinned 分组头由「仅文字变色」升级为整行 `rounded-md` + `hover:bg-muted/40`（+ transition-colors）——组头有明确 hover 底色容器感，与下方任务行清晰区分。
  - **⑤ 分组间距加大**：workspace 分组之间垂直间距 mb-1 → mb-2，pinned 段对齐——分组之间块感更强。
  - **⑥ 任务行排版**：标题保持 14px（侧栏密度上限）并用 font-medium + 全强前景色强调为主文本；元信息行 Badge 居左、时间戳 ml-auto 推右（对齐本地侧栏时间戳规格，明确为辅助信息）；整行保留 hover:bg-muted/40；任务列表容器统一 pl-2 缩进——任务行相对组头统一缩进，层级更清。
  - **⑦ 空状态文案 + 样式**：组内空状态文案由旧的两词简短文案（语义模糊、不点明所属与对象）改为「该工作空间下暂无远程任务」/ "No remote tasks in this workspace yet"（明确所属=工作空间、对象=远程任务）；样式由贴左 px-2 py-1 改为**居中带垂直留白** px-2 py-4 text-center——不再单薄贴左。
  - **⑧ 间距系统统一（统一以上各项的根设计）**：标准化整树水平 padding 与垂直节奏——水平 padding：组头 px-1.5、任务行 px-2（对齐、内容不再贴容器边）；垂直节奏：header(刷新工具栏)↔树 space-y-2、组↔组 mb-2、任务↔任务 space-y-0.5、组头↔其任务 mt-0.5。
  - **验证**：纯前端（无 Rust / 无 webank server 改动）；i18n 中英双语同步（`multica.taskManagement.subtitle` 再精简、`conversation.sidebar.multica.taskCount` 新增、`conversation.sidebar.multica.noTasksInWorkspace` 文案更新）；vitest 19/19 全过、tsc 零错误。

- [x] **M5-ac**（本轮）断点续跑根因修复——启动自愈覆盖 multica work_dir（码灵 client，开发设计 §12.15）：
  - **背景（实测复现）**：M5-ad（parent_task_id 反查）落地后，真实崩溃重启场景仍落 Fresh——启动远程任务 → 关码灵 → 10 分钟后重开 → 远程任务管理点「运行」→ 新会话。
  - **根因（非补丁）**：启动自愈 `recover_interrupted_running_sessions()`（main.rs）跑在 home repo 上，其 `pause_all_running_sessions()`（app/mod.rs:2370）只遍历单一 repo 的 `task_list`；而 multica 远程任务的 run 落在**该任务自己的 `work_dir`**（独立 repo）→ 重启时从不被 pause → 残留 stale `Running` → `is_run_continuable`=false → Fresh。即 §12.5「仅覆盖激活 workspace」限制从「限制」升级为「bug」。
  - **修复**：新增 `recover_multica_work_dir_sessions(home_app)`——home 自愈后遍历 `multica_task_conversations` 的全部 work_dir 逐个自愈（与 `classify_resume` 读同一张权威表），把孤儿 Running run 翻成 Paused + ProcessInterrupted → 命中 Resume。安全前提：启动瞬间磁盘所有 `Running` 都是上一轮崩溃遗留孤儿态。`classify_resume` 加诊断 `info!`（resolved_via/continuable/decision）供复测确认。
  - **验证**：`cargo test -p gold-band-desktop multica::` 82 测全过（新增 `collect_multica_work_dirs` 2 测）。无需 webank server 改动。运行时 e2e 复测**仍落 Fresh**——诊断 `info!` 暴露 `session_present=false → decision=Fresh`，最终根因与修复见 **M5-ae**。

- [x] **M5-ad-fix**（本轮）multica 设置按钮改名「切换账号 / 退出登录」（纯 i18n，开发设计 §12.16）：zh `重新连接`→`切换账号`、`断开连接`→`退出登录`；en `Reconnect`→`Switch account`、`Disconnect`→`Sign out`；首连按钮「连接 Multica」不变。账号旁的「切换账号逃生口」外链保留（cookie 兜底：browser_login 见 cookie 即签 JWT，错 cookie 静默连错，需去 Web 手动登出/登录；点「切换账号」只是再跑一次 browser_login，同样的错 cookie 会再连错），仅同步其 tooltip 引用的旧按钮名。

- [x] **M5-ae**（本轮）断点续跑最终根因——删除 `classify_resume_from` 的 `session_id` 门（码灵 client，开发设计 §12.17）：
  - **背景（实测复现，M5-ac 收尾后）**：M5-ac 把启动自愈扩到 multica work_dir 后，用户复测仍落 Fresh。M5-ac 新增的诊断 `info!`（`multica classify_resume decision`）精确定位：`resolved_via="parent" session_present=false run_status=Some(Paused) continuable=true decision=Fresh`——父系反查 ✓、run 可续 ✓，**唯独 `session_present=false` 触发 Fresh**。
  - **根因（设计层的门用错信号，非补丁）**：`classify_resume_from` 以 `checkpoint.session_id` 非空作续跑门；但该字段**仅由 bridge 在 `NodeCompleted` 回填**（Fresh 写入时 `None`）。崩溃常发生在**首个节点完成前**——`worker-ref.json` 已写 ACP session（故 run 可续），但 `NodeCompleted` 未触发 → bridge 未回填 → `session_id` 恒 `None` → 门误判 Fresh。该门把「checkpoint 记了 session」当「run 有可续 session」的代理，而续跑执行器 `run_continue_background(task_id, run_id, None, None)` **直接读 `worker-ref.json` 的 `continue_ref.acpSessionId`**、完全不读 checkpoint 的 `session_id`（后者仅供 server `pin_task_session`）——错代理。
  - **修复（根因级——删门）**：`classify_resume_from` 删 `session_id` 空判定分支，续跑决策收敛为「checkpoint 解析出本地 ids + run 存在 + `is_run_continuable`」。可续 run 必有 locator → attempt 已起 → worker-ref 有 session；极端失败时 §12.5 既有的「续跑失败 → 落 Fresh」兜底仍生效。诊断 `info!` 的 `session_present` 降级为纯观察字段，不再参与判定。
  - **验证**：`cargo test --bin gold-band-desktop multica::commands::` 17 测全过（`resume_fresh_when_session_id_missing_or_blank` 反转为 `resume_when_session_id_missing_or_blank`——断言 session_id 缺/空白 + 可续 run → Resume）。无 webank server 改动。运行时 e2e（kill 重启续跑确认 `decision=Resume` 即便 `session_present=false`）待用户复测。
- [x] **M5-af**（本轮）换号/断开统一作废账号作用域状态——补齐 State 层索引 + connect 换号检测（码灵 client，开发设计 §12.18）：
  - **问题根因（M5-m 的不完整根治）**：M5-m 把 PAT/账号身份/workspace 绑定/active 定为账号作用域、disconnect 时清——但**账号作用域状态实际横跨两层配置**，M5-m 只清了 `SettingsConfig`，漏了 `StateConfig` 三个同样账号作用域的索引：`multica_pending_issues`（失败回显 issue id）、`multica_task_conversations`（断点续跑索引，键=remote_task_id）、`multica_completed_tasks`（最近完成历史）。三者均以当前账号的 remote id 为键，换号/断开后对新账号无意义。且 `connect_multica` 完全不清——换号重连时旧账号状态原样保留。
  - **两个症状同根**：①换号后设置页/任务列表仍见旧账号 workspace 绑定（M5-m 已修 disconnect，但 connect 换号路径仍漏）；②换号后「置顶」折叠列表里残留旧账号失败 issue 且**点不进去**——失败回显行经 `RemoteTaskVm::from_failed_issue` 构造，**故意**不填 `local_task_id/run_id/project_id`（它是「显示失败 + 提供重试」入口，非「回看会话」入口），前端 `clickable = projectId && localTaskId && runId`（`MulticaRemoteTaskList.tsx:344`）恒 false → 不可点是**设计**；但它跨账号残留是 `multica_pending_issues` 未随换号作废的**泄漏**。
  - **决策（统一作废，非两处各打补丁）**：账号作用域状态 = Settings（workspaces/active）+ State（pending_issues/task_conversations/completed_tasks）两层全集；凭证变更（换号/断开）统一作废这两层，daemon_id（机器作用域）保留。一个不变量、两条触发路径（检测到换号的 connect、disconnect），杜绝「disconnect 清了 Settings 漏 State、connect 啥也不清」的散点修补。
  - **实现**：`config.rs` 抽 `clear_multica_workspace_bindings(&mut SettingsConfig)`（清 workspaces/active，保留 pat/account/daemon_id——换号专用，pat/account 由新登录覆写）；新增 `clear_multica_state_indices(&mut StateConfig)`（清三索引）；`clear_multica_session` 签名不变、内部复用前者（disconnect 行为不变）；新增 `multica_account_changed(existing, new_email)`——以 email 判换号，**任一 email 缺失→false**（同账号重连主流派保留绑定，脏绑定由 register 404 自愈）。`connect_multica` 加 `shared: State<'_, SharedMulticaState>` 参数（Tauri 按类型注入，main.rs 无需改）；browser_login 返回新 (pat,user) 后、覆写 pat/account 前：`multica_account_changed` 为真 → `clear_multica_workspace_bindings` + best-effort `clear_multica_state_indices`+save_state + `clear_runtime_ids`（旧 runtime_id 按旧账号 workspace 注册，换号失效，下 tick 自愈重建）。`disconnect_multica` 在 `clear_multica_session` 后补 best-effort `clear_multica_state_indices`+save_state（补上 M5-m 漏掉的 State 三索引）。
  - **不变量**：`StateConfig.multica_runtime_ids` 是死字段（仅声明、从不读写，真缓存在内存 `MulticaRuntimeState`），不处理。PAT 明文不回显（VM 仅 `pat_set`），换号清理不改变该约束。
  - **验证**：`cargo check` 过（无新增 warning）；`cargo test multica::config` **8 测全过**（+3 新：`multica_account_changed_judges_by_email_with_safe_default` / `clear_multica_workspace_bindings_clears_bindings_keeps_credentials_and_daemon_id` / `clear_multica_state_indices_empties_all_three_account_scoped_indices`；既有 `clear_multica_session_*` 签名不变零回归）。无前端 / 无 webank server 改动。

- [x] **M5-ag** 合并 origin/main 进 feature_multica——main 新能力（git/github 源代码管理、定时任务 scheduled-tasks、app-exit 协调器、agent catalog 等）与 multica 远程任务接入共存（开发设计 §12.19）：
  - **冲突解决策略**：11 文件全部「并集」解决——main 新能力与 multica 接入**正交**（定时任务 vs 远程任务，互不侵入），故无任何一方功能被丢弃。纯加法冲突（Cargo tokio features / 命令 import / config apply 段 / types import / 路由 / i18n / page import）取 main 全量 + multica 增量；唯一语义融合在 `ConversationComposer`（`onSubmit` 签名并集 + `canSubmit` 条件融合）与 `ConversationSidebar`（两个 SidebarButton 并存）。
  - **唯一合并诱发的代码修复**：main 把侧栏导航重构为 `activeNavigationKey` 字符串 key 体系，其 switch 未覆盖合并并入的 `multica-tasks` kind → 补 `case 'multica-tasks': return null`（multica 按钮仍用 `active.kind` 直查，两者并存无 bug；后续若统一导航模型再把 multica-tasks 纳入 key 体系）。
  - **main 新增依赖**：`@tomplum/react-git-log` / `@js-temporal/polyfill` / `cron-parser` / `@vvo/tzdb`（`npm install` 已装）。
  - **验证**：`cargo check --all-targets` 绿（仅 main 既有 dead-code warning）；`tsc -p tsconfig.build.json`（src only）绿零错；`cargo test multica` **85 测全过、0 失败**（零回归）。安全网：合并前 `8fe70d9` 打备份分支 `feature_multica_premerge_backup`。
  - **遗留（main 既有技术债，非 multica）**：`tsc` 全量 50 错全在 `tests/`、0 处引用 multica（VM 类型演化快于 fixture + 节点类型配置缺），未在本次合并处理。
  - **结论**：用户预设的「multica 二次修复开发」基本不需要——并集解冲突使 main 与 multica 全量共存，multica 零回归。

- [x] **M5-ah**（本轮）远程任务页评审改版——去设置页 multica 区块 + 工作空间下拉（选定空间过滤）+ 4 列竖向看板（待办/进行中/已完成/失败，列与动作 1:1）；pinned 账号级失败不再展示 → rerun 全链路死路径 + `save_multica_settings` 一并删除（码灵 client，开发设计 §12.20）：
  - **评审 3 条 UX 调整**：①设置页不再暴露 multica 配置（初次连接即可，配置走渠道默认）；②远程任务页加工作空间下拉，选定后只看该空间任务（不再全部铺开）；③任务展示从「工作空间折叠分组列表」改为按状态的 4 列竖向看板。
  - **数据层无变更**：`tasksByWorkspace` 已按工作空间分组、`RemoteTaskVm.status` 恰为 4 个 canonical 值（与看板列 1:1），`get_multica_tasks` 一次拉全部绑定空间——纯前端重构。
  - **用户决策**：账号操作（切换/断开/添加/移除工作空间）集中到任务页头部账号菜单（设置页不再暴露）；账号级失败（pinned / `retryable=true` / 无工作空间）不再展示，失败列只显工作空间内失败任务；来源下拉（`REMOTE_TASK_SOURCES`，为未来多来源保留）保留。
  - **列与动作 1:1**：待办(queued)→认领执行(claim)、进行中(running)→取消(cancel)、已完成/失败(completed/failed，带本地 run 链接)→点击回看会话(selectRun)。
  - **死路径清理（dev-stage 破坏式）**：`save_multica_settings`（配置表单已无消费方）+ `rerun_multica_task`（pinned 不展示 → 展示中无 retryable 任务 → rerun 永不渲染）全链路删——前端 `api.{ts,/desktop,/browser,/client}` + 后端 `commands.rs`/`multica/commands.rs`/`multica/client.rs`（`rerun_issue` 唯一消费者即 rerun_multica_task，一并删 + 删其单测）+ `main.rs`。
  - **验证**：`cargo test multica` **84 过 0 败**；`tsc -p tsconfig.build.json`（src only）绿零错；vitest 新增 board（`bucketTasksByStatus` + 渲染/动作）+ page（容器逻辑）共 21 测全过；旧 list/settings-block 两测随组件删除。

- [x] **M5-ai**（本轮）远程任务页 UX 再打磨——工作空间 Popover picker（内嵌添加/移除）+ 页头对齐 sibling pages（`variant="integrated"`、去副标题/返回按钮）+ 底部工具条 source 门控 + 账号菜单简化（纯前端，开发设计 §12.21）：
  - **背景**：M5-ah 落地后用户提 3 条调整：①添加/删除工作空间放入工作空间下拉框（不再散落账号菜单）；②页头风格对齐定时任务/运行模式管理页；③工作空间下拉选框 + 账号下拉选框移到底部工具条，source 门控（仅 multica 来源且已连接时显示——未来来源未必有工作空间/PAT 账号概念）。
  - **工作空间 Popover picker**：选定当前空间 + 内嵌添加/移除。移除走 AlertDialog 确认（对齐定时任务 delete 模式）。选定值持久化 `setActiveMulticaWorkspace`，默认 `lastActiveWorkspaceId`（校验仍存在于当前列表，移除后自动回退）。
  - **底部工具条**：仅 `source === 'multica' && connected` 时渲染（source 门控让页前向兼容未来来源）。左区：来源 Select + 工作空间 picker（仅 `hasWorkspaces` 时）；右区：账号菜单（切换/断开）+ 刷新。
  - **账号菜单简化**：删添加/移除工作空间（已迁入 picker），仅保留切换账号 + 断开连接。
  - **页头**：`PageHeader variant="integrated" icon={<Globe />}`（与定时任务/运行模式管理页同构），去返回按钮（ConversationSidebar 持久导航冗余）。
  - **Props 删除 `onBack`**：`MulticaTaskManagementPage` 接口删 `onBack`，`App.tsx` 对应消费点清理。
  - **验证**：`tsc` 绿零错；vitest `multica-task-management-page` 11 测全过（含 workspace-switch→Popover、remove→AlertDialog、source-gate）+ board 12 测全过；`cargo test multica` 84 测绿（零 Rust 变更）。

- [x] **M5-aj**（本轮）远程任务页来源上移页头 + 会话-任务绑定可视化（独立 chip）（纯前端，开发设计 §12.22）：
  - **背景**：用户提 2 条调整：①「任务来源」下拉框放底部不协调，应上移界面上部；②点远程任务「执行」后会话初始输入框编辑期间，一旦跳转切页，会话与任务的绑定"丢失"——希望预填带 multica 标签绑定会话与任务，且可删标签解绑。
  - **根因（绑定并未丢失）**：`draft.multica` 由 `ConversationComposerDraftBoundary` 上提为 Shell 之上 owner 状态，跨 in-app 导航天然保留；服务端 prepare lease（45s TTL）由全局心跳 `extend_prepare_leases` 续期，与当前页无关。真实缺陷是绑定**不可见 + 不可控**（隐式状态无 UI、用户无法主动解除）——"好设计、实现不完善"，非设计缺陷。
  - **独立 chip（非内嵌文本标签）**：内嵌文本标记脆弱（正文编辑残缺、发送需字符串解析、删除解除不可靠）；独立 chip 是 mention/tag 成熟范式，状态与正文解耦。用户已确认"同意独立 chip"。
  - **绑定扩 `title`**：`ConversationComposerMulticaBinding` + `title`（仅 chip 展示，不参与发送寻址）；`prefill` 补传 `title`。
  - **`clearMultica` reducer**：仅置 `multica=null`，保留正文+附件 → 解绑后降级普通本地会话；无绑定 no-op（稳定引用）。
  - **chip 渲染**：`PromptInput` 首子节点；`Badge` + Globe + "Multica · {title}"（截断）+ × 关闭。
  - **解绑两路径**（同 handler）：①× 按钮；②Backspace（仅 `multicaActive && 正文空 && 无 slash command`）→ `cancelMulticaPrepareLease`（释放 lease，幂等、失败静默）+ `clearMultica`。
  - **来源上移**：`REMOTE_TASK_SOURCES` Select 从 footer 迁入 `PageHeader` actions（+ 来源 label），与定时任务管理页 header actions 同构；footer 仅剩工作空间 picker + 账号菜单 + 刷新。
  - **验证**：`tsc` 绿零错；vitest `conversation-composer-draft` 16 测（含 clearMultica 2 新测）+ `multica-task-management-page` 11 测（source-gate 改写 + claim 断言补 title）+ board 12 测共 39 测绿；`cargo test multica` 84 测绿（零 Rust 变更）。

- [x] **M5-ak**（本轮）claim-at-send 重构——点「发送」才 claim+start，移除 prepare-lease（码灵 client + webank server，开发设计 §12.23）：
  - **背景 / 根因**：用户报「删 chip 后再点执行报『找不到该 Multica 任务』」。根因是 M5-q 的 claim-at-click 把领取（有状态副作用）前置到「点执行」——任务进 dispatched + 45s lease，删 chip 后再点执行命中 server 串行化守卫 / 未过期 lease → 404。用户意图：删不删 chip 任务都应仍待办，只有点发送才执行并更新服务端；删 chip 只解绑、降级普通会话。
  - **方案（纠正时机，非补丁）**：claim 从「点执行」推迟到「点发送」，与 start 合并为单一事务边界。点执行降级为**只读**——`get_multica_task_requirement`（GET 任务详情，任务仍 queued）取正文预填 + 记本地绑定；删 chip 纯属本地 `clearMultica`。整套 prepare-lease 续期机制（4 文件）随之移除。
  - **只读端点（webank）**：新增 `GET /api/daemon/runtimes/{rid}/tasks/{tid}`（裸 AgentTaskResponse，不 claim、不回填 claim-only 富字段）；client `get_task_requirement`；命令 `get_multica_task_requirement`。
  - **claim-at-send**：`start_multica_conversation_run` = claim_specific_task(pending→dispatched) → 复用本地 create_conversation_run_vm → start_task(dispatched→running)。发送即事务边界。
  - **release 回滚端点（webank）**：claim 成功但起 run 失败会让任务卡 dispatched。新增 `POST .../release`（RequeueTaskAfterClaimFailure，CAS dispatched→queued），`release_after_run_start_failure` 在 start_run 5 个失败点调用；best-effort、不重试、对「已非 dispatched」幂等 200。替代旧 prepare-lease 45s 过期兜底。
  - **prepare-lease 全量移除**：state.rs（prepare_leases HashMap + PrepareLease + 4 方法）、loop_.rs（extend_prepare_leases）、client.rs（extend_prepare_lease）、commands.rs（cancel_multica_prepare_lease）。runtime_ids / active_runs / resume-on-restart 保留。
  - **前端 wiring**：`getMulticaTaskRequirement`（只读）替 `claimMulticaTask`；移除 `cancelMulticaPrepareLease`；新增 `startMulticaConversationRun`；board `onClaim`→`onPrepare`；page `handleClaimAndPrepare`→`handlePrepareRemoteTask`；`handleUnbindMultica` 只调 `clearMultica`。
  - **发送失败不清 chip（刻意）**：服务端已 release（任务回 queued 可重试），chip 仍有效；失败可能是本地瞬态（workspace/git），清 chip 反误导。
  - **Issue 2（runtime 展示名）**：runtime `name` 原传 provider（"claude-acp"），改用 `current_channel_config().app_name`（默认 "Gold Band"）——name（展示名）与 runtime_type（provider 路由键）分离（loop_.rs register_workspace + client.rs 测试）。
  - **验证**：`cargo check` 绿零新 warning；`cargo test multica` 全过；tsc 零错；vitest multica 2 套件 23 测全过。**需 rebuild + 重启 webank**（只读 + release 两端点）。联调 agent-browser deep-link 验证待跑。
  - **取代**：M5-aj「删 chip → cancelMulticaPrepareLease 释放 lease」与 M5-q claim-at-click + prepare-lease 续期机制——时机纠正后整体废弃。正文 §3 流程图保留 claim-at-click 原始设计记录，以 ⚠️ 指向开发设计 §12.23 为准。

- [x] **M5-al**（本轮）🔴 running 任务永久卡 running——start 响应丢失消歧 + 手动取消远端终态上报（审计 #75，码灵 client，开发设计 §12.24）：
  - **根因**：webank 对 **running 任务无逐任务 liveness**（`FailStaleTasks` running 分支要求 daemon 离线才兜底），码灵静默 drop 一个 remote running 任务会永久卡 running。两条制造孤儿的码灵侧路径：①`start_task` 响应传输层丢失（server 已 running、码灵拿到 NetworkFailed）→ 旧代码无脑 `release`（对 running 是 no-op）→ 任务卡 running + 本地 run 被 teardown；②`cancel_multica_task` 旧实现只做本地 teardown、**不上报远端** → remote running 无人终态化。
  - **方案**：①start 失败消歧——新增纯函数 `decide_start_failure_action(get_task_status 结果)`：`running`→Continue（start 实已成功、响应丢失，本地 run 与 server 一致，继续）/ 非 running→RollbackRelease（release 正确 CAS + teardown）/ 查询失败(None)→Terminate（`fail_task(timeout)` 唯一能确定终结 running + teardown）；新增 `fail_after_run_start_failure`，两处 start 失败点统一走决策。②手动取消双通道——`cancel_multica_task` 增通道 1 best-effort `fail_task(agent_error)`（bare agent_error 不可重试，用户取消不应被 auto-requeue），原 teardown 收编为通道 2。
  - **契约澄清**：`release_task` 文档补明主动 release 为首选恢复，server 的 prepare-lease（码灵不续约）+ dispatched/running sweeper 仅被动兜底。
  - **验证**：`cargo test multica` 全过（+4 决策测：continue/rollback/terminate/status-sensitive-not-truthy）。

- [x] **M5-am**（本轮）🟠 pinned_tasks / retryable 死管线删除（审计 #76 / M1，码灵 client + 前端，开发设计 §12.25）：
  - **根因**：M5-ah 起 pinned（账号级失败回显）不再展示，`retryable=true` 只 pinned 任务有、展示中无任何可重试任务、rerun 按钮永不渲染（rerun 命令 M5-ah 已删）——但 VM `retryable`/`pinned_tasks`/`from_failed_issue`、StateConfig `multica_pending_issues`、bridge `finalize_terminal` pending 处置整条死管线仍残留。
  - **方案（dev-stage 破坏式全链路删）**：删 StateConfig `multica_pending_issues` 字段（无 `deny_unknown_fields`，旧磁盘残留键静默忽略，迁移安全）+ VM `retryable`/`pinned_tasks`/`from_failed_issue` + bridge `PendingUpdate` 枚举语义重定义为「completed 历史 status 选择」（Success→completed / Failure→failed，不增新类型）+ `clear_multica_state_indices` 收窄（三索引→两索引）+ `get_multica_tasks` 删 pinned 组装 + 前端 types/mock 同步。
  - **数据模型影响（取代 M3 设计）**：§5.2 M3 的 `multica_pending_issues` / `retryable` / `pinned_tasks` 数据模型自此废弃，sidebar 只剩 `workspaces` + `tasks_by_workspace`（终态行已在 M5-f 归入对应工作空间组）。
  - **验证**：`cargo test multica` 全过（死管线测随实现删除/改名，零回归）；前端 tsc + vitest 绿（断言 retryable/pinnedTasks 已从序列化结果消失）。

- [x] **M5-an**（本轮）🟠 远程任务页订阅竞态 + 事件去重（审计 #77 / M2，纯前端，开发设计 §12.26）：
  - **根因**：`MulticaTaskManagementPage` 订阅 effect 三缺陷：①订阅竞态（Tauri `listen` 异步 resolve，unmount 抢跑致 listener 泄漏 + setState on dead component）；②fetch storm（每事件独立触发 refreshAll，并发请求风暴）；③effect 依赖耦合（`[refreshAll]` 引用变即重订阅，放大泄漏窗口）。`App.tsx` 侧栏订阅同源 ①②。
  - **方案（抽通用 `useEventDrivenRefresh` hook）**：in-flight + pending 合并（6 并发事件 → 2 次刷新）+ async-unsubscribe-race 处理（active 标志 + resolve 时若已 inactive 立即 dispose，杜绝泄漏）+ ref 解耦（effect `[]`、订阅只注册一次、最新回调从 ref 读）。零依赖纯 React hook，`MulticaTaskManagementPage` + `App.tsx` 侧栏两处共用。
  - **验证**：vitest `use-event-driven-refresh` 10 测全过（含并发合并 6→2、unsubscribe-race、卸载后守卫）；multica page/board 回归绿；tsc 零错。

- [x] **M5-ao**（本轮）🟠 StateConfig 并发 RMW lost-update——App 层 with_state 原子原语（审计 #78 / M3，码灵 client，开发设计 §12.27）：
  - **根因**：bridge 终态收尾（finalize_terminal/handle_node_completed/teardown_active_run）与 commands（migrate_resume_index/start-run upsert）经 `App::load_state→mutate→save_state` 三步操作 StateConfig（普通文件 I/O、**无锁**）。同一 remote 的并发事件两个 load-then-save 交错 → 后写覆盖前写（lost-update），续跑索引残留脏数据。违反 state-lifecycle-and-data-integrity §6（RMW 须原子）。
  - **方案（App 层原子 RMW 原语，最小临界区）**：新增 `App::with_state(update) -> Result<bool>`——per-repo_root 分片 Mutex（32 shard，镜像 `ATTEMPT_RUNTIME_STATE_LOCKS`），锁**仅**包文件 RMW、**不含网络**（§6 临界区最小化）；update 返回 dirty、clean 跳过 save。bridge 3 处 + commands 2 处共 5 写点迁移；pin/终态 HTTP 在锁外。非 multica 写点（用户低并发）未迁移——`with_state` 作增量采用原语。
  - **附带（非 multica，解锁验证）**：删 `src/config/mod.rs` 测试模块 2 处死 import（`MANAGED_AGENT_PRESETS`/`managed_agent_preset`，符号早不存在、lib 测 crate 从未编译故未暴露），使 `cargo test -p gold-band` 可编译。
  - **验证**：`cargo test -p gold-band --lib with_state` 4 测全过（含 32 线程并发 RMW 终态 len==32、无 lost-update）；`cargo test multica` 83 测全过（5 迁移点零回归）。

> **M5-al–ao** 为 multica 全链路审计（#75–#78）回溯加固：#75（🔴 running 孤儿）纠正「server running 无逐任务 liveness」下的终态上报契约；#76（M1）清理 M5-ah 起的 pinned/retryable 死管线（取代 M3 数据模型）；#77（M2）根治远程任务页订阅竞态 + 事件去重；#78（M3）补齐 StateConfig 并发 RMW 原子性。正文 §3 终态上报契约以 M5-al/ao 为准。

- [x] **M5-ap**（本轮）multica 绑定 chip 内嵌输入框 + Backspace 删除条件放宽（纯前端，开发设计 §12.28）：
  - **背景 / 根因**：用户要求 chip「放在输入框里面，删除键可直接删除」，其余样式不变。§12.22（M5-aj）独立 chip 正确，但两点可完善：① chip 在 `PromptInput` 首子节点 block 行（输入区上方独立行），与正文分离；② Backspace 删 chip 要求正文为空，体感不够直接。
  - **方案**：① chip 内嵌——复用 slash 命令标签同款 leading adornment 机制（`useLeadingAdornmentTextIndent`）：chip 移进 textarea 的 `relative` 容器、绝对定位左上角，正文首行 text-indent 缩进让位（换行后回左侧），chip 与 slash 互斥（slash 优先）；② Backspace 放宽——触发条件从「正文为空」改为「光标在正文起点且无选区（`selectionStart===0 && selectionEnd===0`）」，模拟删首个 token，正文非空 / 光标在中末 / 有选区时照常删字不误删；③ 提取纯函数 `shouldBackspaceClearMulticaBinding` 固化交互契约（home composer 过重不挂组件测）。
  - **验证**：tsc 零错；生产构建绿；vitest `conversation-composer-multica-chip` 9 测 + composer/multica 回归 102 测全过。
  - **更新**：§12.22（M5-aj）chip 渲染位置 + Backspace 删除条件；chip 与正文解耦 / × 解绑 / claim-at-send 删 chip 纯本地等设计不变。

- [x] **M5-aq**（本轮）multica dead_code 清理——移除 `error/state/bridge/client.rs` 4 个临时模块级 `#![allow(dead_code)]`，暴露项分类处置（开发设计 §12.29）：
  - **删**：`WorkspaceEmpty` / `PinSessionFailed` 变体（零构造——前端空态守卫 / pin best-effort 记日志）+ i18n；`ActiveRemoteRun.runtime_id` 字段（零读取，心跳走 runtime_ids map）；`client.rs::post_json_with_workspace`（零调用，issue 接口走 `json_send(PUT)`）。
  - **留（定点 allow）**：`SessionResumeFailed`（M4-d 保留码表、resume 改 silent fresh-fallback）；`RemoteTask.auth_token`（Option B 中介下不消费，订正失真注释）；`RemoteTask.prior_session_id`（parent_task_id 主路径）。
  - **错误码表订正**：删 `multica.workspace-empty`；`multica.session-resume-failed` 标注「保留码表但不 emit」。
  - **验证**：`cargo check` multica 零 warning（仅 main 既有 9 条非 multica）；`cargo test multica::` 83 过；tsc 零错；vitest 1124/1124。

- [x] **M5-ar**（本轮）wb 渠道 multica 地址修正——默认端口 80 → nginx 统一入口 `:5005`（开发设计 §12.30）：
  - **背景 / 根因**：跨机器部署连远程 multica（172.21.18.88，nginx 对外统一 `http://maling.weoa.com:5005`，前后端同源）。核查发现 `wb.json` 的 `multicaBaseUrl`/`multicaAppUrl` 仍是 `http://maling.weoa.com`（默认 80，过时）——渠道配置链路正确（json→build.rs 编译期 env→channel.rs），仅值滞后于服务端 nginx 端口；运行期不可改（connect 不收 URL、设置页区块已删、`save_multica_settings` 已删），须修正编译期值而非加运行期旁路。
  - **方案（修正过时值）**：wb 渠道本属 maling 生态（appName MALING、updater/metrics/内置 MCP 均指向 maling.weoa.com），multica 地址同生态，直接把 `multicaBaseUrl`/`multicaAppUrl` 改为 `http://maling.weoa.com:5005`（前后端同入口，消除前端 API 地址硬编码 localhost 隐患、无 CORS）；`browser_login` 的 `cli_callback` 仍走 Gold-Band 本机 127.0.0.1，nginx 透传 multica 后端登录成功的 302，跨机器 connect 不受影响（§3.2.6）。服务端 nginx 入口已由用户确认就绪。
  - **验证**：`GOLD_BAND_RELEASE_CHANNEL=wb cargo check -p gold-band-desktop` 绿——build.rs 正确解析 wb.json 新值、编译期 env 生效 `maling.weoa.com:5005`；default 渠道无回归。纯渠道配置值修正，无运行时逻辑变化、无性能影响。

- [x] **M5-as**（本轮）二次合并 origin/main——会话创建契约对齐 + composer chip 重新集成（开发设计 §12.31）：
  - **背景**：feature 自 M5-ag 后再落后 main 约 200 commit（main 重构会话创建契约/心跳/个性化/composer UI），feature 领先 22 commit。策略同 M5-ag：冲突保 main，合并后修复 multica。备份分支 `feature_multica_premerge_20260824`，合并提交 `79ba1e26`。
  - **契约影响（对外行为不变，内部对齐 main）**：main 的 `create_conversation_run_vm` 改返回 `ConversationCreateResultVm {task, run}`——`start_multica_conversation_run` 同步该契约（Fresh 透传 / Resume 由 `conversation_run_vm` + `conversation_task_row_vm` 组装）；前端 `startMulticaConversationRun` 同改，App.tsx 两路径（远程/本地）同契约解构，侧栏刷新与导航链完全复用。claim-at-send 事务边界、终态桥接、断点续跑等 multica 语义零变化。
  - **composer chip 重新集成**：main 重构 composer（工作区控件迁入 info bar `ConversationWorkspaceControl`）后 chip 全量回插——绑定 chip（Globe Badge + × 解绑）+ Backspace 删除契约（`shouldBackspaceClearMulticaBinding`）+ 预填保留；决策 d 落 `forceSelector`（单工作区强制下拉）、决策 e 落 `emptyWorkspaceHint`（0 工作区虚线提示 + canSubmit 禁发）。
  - **验证**：`cargo check --workspace --all-targets` 零错误（余量 warning 均为 main 既有，逐文件比对 origin/main 确认）；tsc src 零错；`web:build` 成功；vitest 全量 **1559/1559**（multica 4 套件 + composer chip/draft 含 clearMultica 语义全过）；`cargo test --workspace` 全过。

- [ ] **M6 · 测试**（开发设计 8）
  - [ ] 登录链路 / 全量 register / 任务执行循环 / 失败恢复 / 会话级续跑 各一条端到端集成测试（mock multica server）

      