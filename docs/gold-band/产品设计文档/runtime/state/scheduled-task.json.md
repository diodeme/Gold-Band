# `scheduled-task.json` 规范

## 1. 文件与数据库位置

定时任务属于 application runtime store，不写入项目工作树。调度定义和 occurrence 使用独立 SQLite 数据库，instruction 与附件仍保留项目级输入快照：

```text
~/.gold-band/projects/{project-id}/scheduled-tasks.db

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
  "projectId": "D--Projects-code-ai-Gold-Band",
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
    "workspaceProjectId": "D--Projects-code-ai-Gold-Band",
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

- `version` 当前逻辑模型为 `0.2`；SQLite schema 通过 migration 管理。
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
    id TEXT PRIMARY KEY,
    project_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    definition_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    next_run_at INTEGER
);

CREATE INDEX idx_scheduled_jobs_enabled_deadline
    ON scheduled_jobs(enabled, next_run_at)
    WHERE enabled = 1;
```

计划时间到达时，repository 在一个 immediate transaction 中校验 `project_id + id + revision + enabled`，按已保存的 `next_run_at` 插入或取得 scheduled occurrence，并推进下一计划点及 `revision`。陈旧 revision 不物化 occurrence。创建定义只写 `scheduled_jobs`；手动“立即执行”只创建 manual occurrence，不修改 `next_run_at` 或 job revision。runtime projection 写入 Task 关联或最近结果时必须在同一事务内推进 `updated_at` 至少 1ms，并让 `definition_json.updatedAt` 与 SQL `updated_at` 完全一致，使旧 authoring 时间令牌立即冲突。

```sql
CREATE TABLE scheduled_occurrences (
    id TEXT PRIMARY KEY,
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
    UNIQUE(job_id, scheduled_at, trigger_kind)
);
```

`owner_id + lease_until + heartbeat_at` 用于原子认领、心跳续租和崩溃恢复。后端只保存 `error_code` 与 `error_params`，不保存面向用户的错误文案。

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

## 6. 迁移与删除

schema v2 使用 `scheduler_migrations(name, applied_at, details_json)` 保存持久化完成标记。`legacy-json-v1` 和 `legacy-shared-db-v1` 都先把源数据读取成内存快照，再用一个 immediate transaction 导入全部 definition/occurrence，最后写 marker 并提交；任一冲突会回滚数据和 marker。marker 已存在时，即使目标数据库为空也不得重新读取或导入旧源。旧 shared SQLite 只复制当前 `project_id` 的数据到项目数据库。

schema 升级按版本顺序在事务中执行，v1 到 v2 不丢失 definition 或 occurrence。升级事务会解析已有 `definition_json`，并使用统一规则回填 enabled job 的 `next_run_at`：baseline 为 `lastTriggerAt`，没有时为 `createdAt - 1s`，再调用 schedule 的 `next_occurrence_after`；legacy JSON 和 shared SQLite 的 null deadline 使用同一派生函数。disabled job 与已完成 one-shot 自然保持 `None`。数据库版本高于当前二进制支持版本时直接拒绝打开，绝不向下覆盖版本号。

终态 occurrence 按有界批次清理，`batch_size` 必须大于零，只处理 cutoff 之前的 `succeeded | failed | skipped | missed`。`attention_required`、非终态 occurrence，以及调用方提供的活动 Run ID 所关联记录必须保留；清理不会删除 Task、Run、Round、ACP 会话或产物。

迁移完成后旧 JSON 只作为迁移证据，不再作为 repository 的读写或 fallback 权威。命令层停止 JSON 双写由统一 schedule service 切换阶段完成。

runtime 仍保存结构化 `contentSnapshot` 与 `contentFingerprint`。指纹是 authoring 内容的 canonical SHA-256 投影；model、thought level、permission 和 Direct session policy 仍属于执行配置，不参与指纹。

删除调度定义只删除 scheduler 定义、复制的输入快照和 occurrence 历史；已经物化的 Task/Run/Round/ACP 会话和产物继续保留。

系统启动或唤醒时，过去的计划时间点写入 `missed`，不自动补跑；调度器直接计算未来时间点。启动后台进程不代表 occurrence 已成功，必须等待带 occurrence ID 的真实 Task/Run/ACP 完成事件。
