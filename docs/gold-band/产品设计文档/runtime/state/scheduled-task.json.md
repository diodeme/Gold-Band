# `scheduled-task.json` 规范

## 1. 文件与数据库位置

定时任务属于 application runtime store，不写入项目工作树。调度定义和 occurrence 与用户级核心状态共用一个 SQLite 数据库，instruction 与附件仍保留项目级输入快照：

```text
~/.gold-band/core.db

~/.gold-band/projects/{project-id}/scheduled-tasks/{scheduled-task-id}/
  input/
    requirement.md
    attachments/
```

附件在创建时复制到 `input/attachments/`，不依赖临时目录或原始路径。

## 2. 逻辑定义投影

下列结构描述 scheduler 数据模型，不再作为 runtime 的长期 JSON source of truth：

```json
{
  "version": "0.2",
  "id": "scheduled-task-001",
  "projectId": "gold-band--2f4a7c91",
  "enabled": true,
  "mode": "direct",
  "sessionPolicy": "continuous",
  "taskId": null,
  "contentFingerprint": "sha256:...",
  "content": {
    "instructionPath": "input/requirement.md",
    "attachmentPaths": [],
    "workflowSnapshotPath": null,
    "autoConfigPath": null,
    "workspaceProjectId": "gold-band--2f4a7c91",
    "directAgentId": "agent-001"
  },
  "executionConfig": {
    "modelOverride": null,
    "thoughtLevel": null,
    "permissionModeOverride": null,
    "agentId": "agent-001"
  },
  "schedule": {
    "kind": "every",
    "timezone": "Asia/Shanghai",
    "every": { "value": 6, "unit": "hours" },
    "anchorAt": "2026-07-30T10:10:00+08:00"
  },
  "overlapPolicy": "skip_when_running",
  "nextRunAt": "2026-07-30T16:10:00+08:00",
  "status": "active",
  "createdAt": "2026-07-30T10:10:00+08:00",
  "updatedAt": "2026-07-30T10:10:00+08:00"
}
```

## 3. 字段约束

- `version` 当前逻辑模型为 `0.2`；SQLite schema 通过 `core_schema` 的 `scheduler` component 独立管理。
- `projectId` 是应用内 workspace 的唯一业务身份。job、occurrence、查询、事件与去重都必须显式携带该作用域；不再使用 `workspaceKey` 或路径作为平行业务身份。
- `mode` 为 `direct | workflow | auto`。
- `sessionPolicy` 为 `new | continuous`；Workflow/AUTO 必须为 `new`。
- `taskId` 在首次触发前允许为 `null`，首次物化后指向当前 task。
- `contentFingerprint` 覆盖定时任务内容及 authoring 身份；Workflow/AUTO 的 Agent 身份、Agent 策略和可用 Agent 集合必须参与指纹。model、thought level、permission 和 Direct session policy 不参与指纹。
- `schedule.kind` 为 `at | repeat | every | cron`。
- `every.unit` 只能是 `minutes | hours`，`value` 必须为正整数。
- `schedule.timezone` 必须是有效 IANA 时区。
- `overlapPolicy` 为 `skip_when_running | retry_when_busy`。
- `enabled = false` 时不计算新的触发；重新启用 `every` 必须重置 `anchorAt`。
- 不存在独立 `name` 字段；展示标题取 instruction 第一条非空行，任务 ID 仍是唯一身份。

## 4. SQLite 表约束

`scheduled_jobs` 保存上面的定义投影和调度游标；`scheduled_occurrences` 保存每一个计划或手动执行。`revision` 是 deadline 回调和 runtime 投影更新的乐观并发版本，`next_run_at` 是调度器唯一使用的持久化计划点：

```sql
CREATE TABLE scheduled_jobs (
    project_id TEXT NOT NULL,
    id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    definition_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    next_run_at INTEGER,
    PRIMARY KEY (project_id, id)
);

CREATE INDEX idx_scheduled_jobs_enabled_deadline
    ON scheduled_jobs(project_id, enabled, next_run_at)
    WHERE enabled = 1;
```

SQL 行中的 `project_id` 与 `definition_json.projectId` 必须一致；不一致的数据视为损坏，不能跨项目返回。计划时间到达时，repository 在一个 immediate transaction 中校验 `project_id + id + revision + enabled`，按已保存的 `next_run_at` 插入或取得 scheduled occurrence，并推进下一计划点及 `revision`。陈旧 revision 不物化 occurrence。创建定义只写 `scheduled_jobs`；手动“立即执行”只创建 manual occurrence，不修改 `next_run_at` 或 job revision。runtime projection 写入 Task 关联或最近结果时必须在同一事务内推进 `updated_at` 至少 1ms，并让 `definition_json.updatedAt` 与 SQL `updated_at` 完全一致，使旧 authoring 时间令牌立即冲突。

```sql
CREATE TABLE scheduled_occurrences (
    project_id TEXT NOT NULL,
    id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    scheduled_at INTEGER NOT NULL,
    trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('scheduled', 'manual')),
    status TEXT NOT NULL CHECK (status IN (
        'pending', 'running', 'retrying', 'succeeded', 'failed',
        'skipped', 'missed', 'attention_required'
    )),
    attempt INTEGER NOT NULL DEFAULT 0,
    owner_id TEXT,
    lease_until INTEGER,
    heartbeat_at INTEGER,
    task_id TEXT,
    run_id TEXT,
    round_id TEXT,
    attempt_id TEXT,
    error_code TEXT,
    error_params TEXT,
    started_at INTEGER,
    finished_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, id),
    FOREIGN KEY (project_id, job_id)
        REFERENCES scheduled_jobs(project_id, id)
        ON DELETE CASCADE,
    UNIQUE(project_id, job_id, scheduled_at, trigger_kind)
);
```

`owner_id + lease_until + heartbeat_at` 用于原子认领、心跳续租和崩溃恢复。所有 occurrence 读写、claim、lease、完成、恢复与历史查询都必须同时匹配 `project_id`；相同 job ID 或 occurrence ID 可以在不同项目中安全共存。后端只保存 `error_code` 与 `error_params`，不保存面向用户的错误文案。

## 5. Occurrence 生命周期

每个实际到达或手动请求的时间点创建一个 occurrence。状态为：

```json
{
  "id": "occurrence-001",
  "scheduledTaskId": "scheduled-task-001",
  "scheduledAt": "2026-07-30T16:10:00+08:00",
  "triggerKind": "scheduled",
  "status": "succeeded",
  "attempt": 1,
  "ownerId": null,
  "leaseUntil": null,
  "taskId": "task-004",
  "runId": "run-002",
  "errorCode": null,
  "createdAt": "2026-07-30T16:10:05+08:00",
  "updatedAt": "2026-07-30T16:20:00+08:00"
}
```

Occurrence 状态为 `pending | running | retrying | succeeded | failed | skipped | missed | attention_required`。`attention_required` 表示 scheduler 已释放 lease，但关联 Run 仍可由用户恢复；回答完成后可更新为 `succeeded` 或 `failed`。

队列重试只增加 `attempt`，不为同一个计划时间生成新的 occurrence。`manual` occurrence 不推进 `nextRunAt`。

## 6. Schema、破坏式切换与删除

Scheduler 在用户级 `core.db` 的 `core_schema` 中以 `component = 'scheduler'` 独立登记 schema version；它与 Runtime recovery 等 component 共用物理数据库，但不共用表或版本号。数据库统一启用 WAL、foreign keys、`synchronous = FULL` 与 3 秒 busy timeout。

本次切换是开发阶段的破坏式更新：旧的项目级或用户级 `scheduled-tasks.db`、旧 JSON definition/trigger、旧 `scheduler_schema` 与 `scheduler_migrations` 均直接废弃。runtime 不扫描、不打开、不导入、不双写，也不在新库为空时 fallback 到任何旧数据。已有用户需要重新创建定时任务。

终态 occurrence 按有界批次清理，`batch_size` 必须大于零，只处理 cutoff 之前的 `succeeded | failed | skipped | missed`。`attention_required`、非终态 occurrence，以及调用方提供的活动 Run ID 所关联记录必须保留；清理不会删除 Task、Run、Round、ACP 会话或产物。

runtime 仍保存结构化 `contentSnapshot` 与 `contentFingerprint`。指纹是 authoring 内容的 canonical SHA-256 投影；model、thought level、permission 和 Direct session policy 仍属于执行配置，不参与指纹。

删除调度定义只删除 scheduler 定义、复制的输入快照和 occurrence 历史；已经物化的 Task/Run/Round/ACP 会话和产物继续保留。

系统启动或唤醒时，过去的计划时间点写入 `missed`，不自动补跑；调度器直接计算未来时间点。启动后台进程不代表 occurrence 已成功，必须等待带 occurrence ID 的真实 Task/Run/ACP 完成事件。
