# `scheduled-task.json` 规范

## 1. 文件位置

定时任务属于 project runtime store，不写入项目工作树：

```text
~/.gold-band/projects/{project-id}/scheduled-tasks/{scheduled-task-id}/
  scheduled-task.json
  input/
    requirement.md
    attachments/
  triggers/
    trigger-001.json
```

附件在创建时复制到 `input/attachments/`，不依赖临时目录或原始路径。

## 2. 最小结构

```json
{
  "version": "0.1",
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
    "anchorAt": "2026-07-30T10:10:00+08:00",
    "nextAt": "2026-07-30T16:10:00+08:00"
  },
  "overlapPolicy": "skip_when_running",
  "status": "active",
  "createdAt": "2026-07-30T10:10:00+08:00",
  "updatedAt": "2026-07-30T10:10:00+08:00"
}
```

## 3. 字段约束

- `version` 当前固定为 `0.1`。
- `mode` 为 `direct | workflow | auto`。
- `sessionPolicy` 为 `new | continuous`；Workflow/AUTO 必须为 `new`。
- `taskId` 在首次触发前允许为 `null`，首次物化后指向当前 task。
- `contentFingerprint` 覆盖定时任务内容及 authoring 身份；Workflow/AUTO 的 Agent 身份、Agent 策略和可用 Agent 集合必须参与指纹。model、thought level、permission 和 Direct session policy 不参与指纹。
- `schedule.kind` 为 `at | repeat | every | cron`。
- `every.unit` 只能是 `minutes | hours`，`value` 必须为正整数。
- `schedule.timezone` 必须是有效 IANA 时区。
- `overlapPolicy` 为 `skip_when_running | retry_when_busy`。
- `enabled = false` 时不计算新的触发；重新启用 `every` 必须重置 `anchorAt`。

## 4. 触发记录

每个实际到达的时间点写入一个不可变的 `triggers/trigger-NNN.json`，至少包含：

```json
{
  "version": "0.1",
  "id": "trigger-001",
  "scheduledTaskId": "scheduled-task-001",
  "scheduledAt": "2026-07-30T16:10:00+08:00",
  "status": "completed",
  "taskId": "task-004",
  "runId": "run-002",
  "attempts": 1,
  "createdAt": "2026-07-30T16:10:05+08:00",
  "updatedAt": "2026-07-30T16:20:00+08:00"
}
```

## Implementation note

The runtime model now persists a structured `contentSnapshot` beside
`contentFingerprint`. The fingerprint is a canonical SHA-256 projection of
authoring content only; model, thought level, permission, and Direct session
policy remain execution settings and are excluded from the identity.

Definition deletion removes only the scheduled-task definition, its copied
inputs, and its trigger records. Materialized Task/Run history remains in the
project runtime store. Trigger files use monotonically increasing IDs and are
never rewritten.

`status` 为 `scheduled | running | skipped | missed | completed | failed`。队列重试只增加 `attempts`，不生成新的 trigger。
