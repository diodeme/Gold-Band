# 用户级核心状态与 Runtime 恢复

## 1. 目标

桌面端启动恢复的成本必须由“当前可能仍处于非终态的 run 数量”决定，不能随历史工作空间、task 和 run 的累计量增长。

Gold Band 为此维护一个用户级核心数据库：

```text
<user-gold-band-root>/core.db
```

`core.db` 跨工作空间共享，承载用户级核心持久状态。当前维护 `runtime_recovery_candidates` 以及 Scheduler 的 `scheduled_jobs` / `scheduled_occurrences`；它不是第二套 Runtime 生命周期数据库，Scheduler 表也不决定 Run 的 canonical 生命周期。

## 2. 数据归属

| 数据 | 归属 | 权威性 |
| --- | --- | --- |
| `run.json.status / outcome / execution` | workspace 下的 run | 唯一生命周期事实源 |
| `runtime_recovery_candidates` | 用户级 `core.db` | 有界恢复索引，不决定 run 状态 |
| `scheduled_jobs / scheduled_occurrences` | 用户级 `core.db`，按 `project_id` 分区 | Scheduler 定义与执行 occurrence 的唯一事实源 |
| `ActiveRuntimeRegistry` | 桌面进程内存 | 当前进程真实活跃 run 的投影 |

候选表按 `(project_id, task_id, run_id)` 唯一，至少记录唯一 workspace identity、workspace path、run locator、`candidate_token`、runtime instance 和登记时间。表上限为 4096；达到上限时拒绝新的 run，不淘汰仍可能活跃的旧候选。

`candidate_token` 是执行代 fencing metadata。它随当前候选写入 `run.json.execution.recoveryCandidateToken`，只用于确保旧执行的迟到清理不能删除同一 run 的新一代候选，不表达生命周期状态。

`core.db` 以用户为作用域，因此桌面应用进程也以同一用户、同一发布渠道单实例运行。各领域通过 `core_schema.component` 独立管理 schema version；Scheduler 使用 `component = 'scheduler'`，不得复用或推进 Runtime recovery 的 `component = 'core'`。Tauri 官方 single-instance plugin 必须作为第一个 plugin 注册：第二次启动只显示、恢复并聚焦已有主窗口，不得进入 setup、读取候选、恢复 run 或启动 scheduler。不用候选表中的 `runtime_instance_id` 自研进程租约或存活探测。

## 3. 写入顺序

run 进入 `Running` 的顺序固定为：

1. 在 `core.db` 原子 upsert 新一代候选并获得 `candidate_token`。
2. 将该候选作为 provisional registration 放入进程内 `ActiveRuntimeRegistry`，使并发退出要么看到它、要么先关闭 admission gate。
3. 将 token 写入待持久化的 `run.json.execution`，并持久化 canonical `run.json = Running`。
4. 提交 registration；未提交时从 active projection 撤销，持久行仍按 canonical state 在下次恢复中消费。

首次启动、显式继续、动态继续和重试必须共用这一提交边界：provisional registration 只能在 drive 首次成功持久化 `Running` 后提交；若该 execution 已被更新代取代，则撤销 registration 并条件删除候选，不能把前置失败投影为当前进程活跃 run。

候选登记失败时不得写入 `Running`。进程在第 1、3 步之间崩溃最多留下一个多余候选；下次启动读取 canonical `run.json` 后会将它作为 stale candidate 消费，不会把非 Running run 恢复为 Running。

run 持久化为 `Paused / Completed` 后，使用该 run 中的 token 条件删除候选。删除 SQL 必须同时匹配 `project_id`、task、run 和 token。旧 generation 的迟到完成、暂停或错误收敛即使晚于新 generation，也只能尝试删除自己的 token。

## 4. 启动恢复

窗口壳、前端 bootstrap 与主题初始化不等待 Runtime 恢复。桌面 setup 在后台 blocking worker 中执行以下流程：

1. 只读取 `core.db.runtime_recovery_candidates`，不读取 `conversation_workspaces` 后扫描全部历史 task/run。
2. 逐条用 `project_id` 验证 workspace path 与 project manifest；path 只是 locator，不参与业务关联。
3. run 不存在、canonical status 已非 `Running`，或 `run.json.execution.recoveryCandidateToken` 与候选 token 不一致：只条件删除旧候选，不改 run。
4. run 仍为 `Running` 且 token 与候选一致：通过现有状态接口收敛为 `Paused + ProcessInterrupted`，再删除候选。
5. 通过已有 `conversation-run-state-updated` lifecycle event 局部更新对应会话，不刷新整页或重新加载全部会话。

成功消费的候选立即删除，同一候选不会进入下一次启动。若进程在“run 已 Paused、候选尚未删除”之间再次崩溃，下一次启动只做第 3 步并删除候选，不再次改写生命周期。

## 5. 启停门闩与 scheduler

进程级 coordinator 有 `Recovering / Accepting / ShuttingDown` 三个 admission phase：

- `Recovering`：拒绝新 run；scheduler 尚未启动。
- `Accepting`：恢复完成后接受新 run；scheduler 才能注册未被隔离的工作空间。
- `ShuttingDown`：先关闭 run admission，再停止并等待 scheduler，最后只暂停 `ActiveRuntimeRegistry` 快照中的真实活跃 locator。

退出流程不扫描工作空间或历史 run。候选已登记但没有进入当前进程 active registry 时，不会被正常退出误当作活跃 run；它由下次启动按 canonical state 校验。

## 6. 一致性、锁与失败隔离

- 不做 SQLite 与多个 `run.json` 之间的跨文件强事务；候选允许多报一次，不允许漏报 Running run。
- `core.db` 的 Runtime recovery 与 Scheduler 连接统一使用 WAL、foreign keys、`synchronous=FULL`、短事务和 3 秒 busy timeout。
- registry mutex 内只检查 phase、token 和内存 map；不得在锁内执行 SQLite、文件、provider 或 scheduler I/O，也不得持锁跨 `await`。
- 单实例互斥在任何 Runtime 恢复和 scheduler setup 前完成；第二进程不得触发候选消费。
- `core.db` 整体无法列出候选时，无法证明恢复全集，保持全局 `Recovering`，scheduler 不启动。
- 单条候选或单 workspace 读取、收敛、删除失败时，只把该 `project_id` 加入 blocked set；其它 workspace 完成恢复并启动 scheduler。失败候选保留供下一次恢复，不静默丢失可能仍为 Running 的 canonical run。

该模型选择“候选可重复验证、成功后即删除”的最终收敛，不复制 `run.json` 生命周期，也不为跨 workspace 操作引入全局事务或长锁。

## 7. 性能边界

旧启动路径为 `O(W + Σ(tasks + runs))`，历史数据持续累计时启动时间无上限。新路径为 `O(C)`，其中 `C <= 4096` 且正常情况下只等于崩溃前可能非终态的 run 数量；没有候选时只打开一次用户级 SQLite 并执行一条有索引的小查询。

恢复按候选顺序执行有限本地 I/O，不启动无界任务、不扫描 timeline、不调用 provider，也不引入轮询或无界缓存。
