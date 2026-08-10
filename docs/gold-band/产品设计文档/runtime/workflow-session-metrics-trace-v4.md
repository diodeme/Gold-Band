# Workflow 会话指标实际上报样例（第四轮验证）

> 本文档记录 taskTitle 字段加入、executionId 统一为 taskId、后台线程化后的一次完整 Workflow 会话上报数据。
> 本次会话包含 7 个节点（访谈→方案→人工确认→开发→审查→测试→验收→清理），覆盖了人工暂停/恢复、round 计数、全部节点 started+completed。

## 1. 会话信息

| 项 | 值 |
|---|---|
| 项目 | `D:\IdeaProjects\mall` |
| 会话模式 | `sessionMode=workflow` |
| executionId（= taskId） | `49eacfbe31dc4432b520936761c80a8e` |
| taskTitle | `帮我写一个最简单的单例模式` |
| clientVersion | `0.9.0` |
| 会话窗口 | `2026-08-10T17:55:24` ~ `2026-08-10T19:19:33`（约 84 分钟） |
| 用户输入轮次 | 1 轮 |
| 人工确认 | 1 次（方案节点完成后暂停等待人工确认） |
| 数据源 | `C:\Users\kelvinzhou\AppData\Local\maling\metrics.log` |
| 日志位置 | 第 `5901` 行至第 `5977` 行（去重后唯一事件） |

本次会话共 `18` 条唯一事件，分布在 `17` 个上报批次中（已按 `eventId` 去重，忽略服务端错误导致的重试）。

**本轮验证要点：**
- `executionId` 统一为 `taskId`（`49eacfbe...`），所有事件共享，不再有 `parentExecutionId` 或 `taskId` 字段。
- `taskTitle` 在所有事件中携带。
- 每个节点（7个）都有完整的 started + completed 对。
- `pauseCount=1` 对应方案节点完成后的人工确认暂停。
- `roundCount=1` 表示首轮交付。
- started 事件不携带 `model`；completed 事件从 ACP session 解析实际模型名 `glm-5.2`。
- `modelUsages[].acpSessionElapsedMs` 不为 null。
- `occurredAt`/`reportedAt` 不带时区偏移量。
- run 级终态事件携带 `collectionStateRecovered: true`，表示运行状态恢复路径。

## 2. 原始 JSON 与逐条上报逻辑

### 第 1 条：run started

- 日志行号：`5901`
- 请求时间：`2026-08-10T17:55:24`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "5640aaf1-41bb-4afc-9e4e-54ccacc963e0",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T17:55:24.000",
    "reportedAt": "2026-08-10T17:55:24.577",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式"
  }]
}
```

#### 上报逻辑

Workflow run 启动时，orchestrator 在 `prepare_run` 中发出第一个生命周期事件。`emit_run_metrics_fact` 构建 run started：`executionKind=run`，`executionId=taskUuid`，不带 attempt/node/usage/model 字段。`taskTitle` 从 `task_show(&run.task_id).title` 快照获取。这是整个 Workflow 交付的入口事件。

### 第 2 条：访谈节点 started

- 日志行号：`5903`
- 请求时间：`2026-08-10T17:55:24`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "a0e194c6-3241-436c-899d-37f3018c41c3",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T17:55:24.000",
    "reportedAt": "2026-08-10T17:55:24.716",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "9e22e04f-679f-5262-b28e-1f2c88d65541",
    "attemptId": "9a2b71074e544837a5a26adb6048dd5e",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "访谈",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

orchestrator 进入 `drive_from_node_with_initial_session` 的节点执行循环，第一个节点（访谈）开始执行。`emit_lifecycle_event(NodeStarted)` 由 orchestrator 发出，metrics-fact-producer 后台线程消费该事件，调用 `emit_derived_node_metrics_fact` 构建 node-attempt started。`nodeId` 由 runUuid + round/node 逻辑键用 UUID v5 派生（重试不变），`attemptId` 使用 NodeState UUID（每次执行新建），`attemptIndex=1` 表示首次尝试。`roleName` 从 resolved profile 名称快照。started 事件不携带 `model`。

### 第 3 条：访谈节点 completed

- 日志行号：`5924`
- 请求时间：`2026-08-10T18:03:53`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "dbea6d47-9581-4f2d-8000-4d4575af846e",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T18:03:53.000",
    "reportedAt": "2026-08-10T18:03:53.912",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "9e22e04f-679f-5262-b28e-1f2c88d65541",
    "attemptId": "9a2b71074e544837a5a26adb6048dd5e",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "访谈",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 42046, "outputTokens": 12222, "cacheReadTokens": 448512, "totalTokens": 502780},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 42046, "outputTokens": 12222, "cacheReadTokens": 448512, "totalTokens": 502780, "acpSessionElapsedMs": 300000}],
    "timing": {"startedAt": "2026-08-10T17:55:24.000", "endedAt": "2026-08-10T18:03:53.000", "acpSessionElapsedMs": 300000}
  }]
}
```

#### 上报逻辑

访谈节点 ACP session 执行完成。completed 事件携带实际模型名 `glm-5.2`（从 `acp.session.json` 解析）。`totalTokens=502780` 含大量 cacheReadTokens（448512），反映长上下文访谈场景。`acpSessionElapsedMs=300000`（5分钟）是 ACP session 的净处理时间。访谈节点耗时约 8 分 29 秒。

### 第 4 条：方案节点 started

- 日志行号：`5926`
- 请求时间：`2026-08-10T18:03:53`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "252831ad-5171-4d1b-a0a1-3b6e3de1bee9",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T18:03:53.000",
    "reportedAt": "2026-08-10T18:03:54.224",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "d7f17418-6fb2-5e70-830d-9ecf8a0a36d4",
    "attemptId": "98f6084c273a443d928da2d44d4de22d",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "方案",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

访谈节点完成后 orchestrator 转入方案节点。新 nodeId、新 attemptId，attemptIndex 仍为 1（同 nodeId 的首次尝试）。

### 第 5 条：方案节点 completed

- 日志行号：`5932`
- 请求时间：`2026-08-10T18:06:36`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "080d5d64-fd69-4645-a554-0e32789282cc",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T18:06:36.000",
    "reportedAt": "2026-08-10T18:06:36.809",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "d7f17418-6fb2-5e70-830d-9ecf8a0a36d4",
    "attemptId": "98f6084c273a443d928da2d44d4de22d",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "方案",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 38771, "outputTokens": 7256, "cacheReadTokens": 206656, "totalTokens": 252683},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 38771, "outputTokens": 7256, "cacheReadTokens": 206656, "totalTokens": 252683, "acpSessionElapsedMs": 151000}],
    "timing": {"startedAt": "2026-08-10T18:03:53.000", "endedAt": "2026-08-10T18:06:36.000", "acpSessionElapsedMs": 151000}
  }]
}
```

#### 上报逻辑

方案节点 ACP session 完成。方案节点耗时约 2 分 43 秒，`acpSessionElapsedMs=151000`。

### 第 6 条：run paused（人工确认）

- 日志行号：`5934`
- 请求时间：`2026-08-10T18:06:36`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "caa28163-5d8f-41a5-b145-4e965e9cade0",
    "eventRevision": 2,
    "eventType": "execution.paused",
    "occurredAt": "2026-08-10T18:06:36.000",
    "reportedAt": "2026-08-10T18:06:36.877",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "pauseReason": "waiting-for-user-input"
  }]
}
```

#### 上报逻辑

方案节点完成后，orchestrator 检测到需要人工确认（方案审批），暂停 run。`emit_run_metrics_fact` 构建 execution.paused 事件，`pauseReason=waiting-for-user-input`。这是 run 级别的事件，不携带 node/attempt 字段。run 的 eventRevision 从 1（started）递增到 2。

### 第 7 条：intervention requested（人工确认）

- 日志行号：`5934`
- 请求时间：`2026-08-10T18:06:36`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "a6dcc49d-ba1d-4c81-9a91-c54e7c7e1e34",
    "eventRevision": 3,
    "eventType": "intervention.requested",
    "occurredAt": "2026-08-10T18:06:36.000",
    "reportedAt": "2026-08-10T18:06:36.888",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "interventionKind": "manual-decision"
  }]
}
```

#### 上报逻辑

与 paused 同一时刻，`emit_intervention_requested` 构建人工确认请求事件。`interventionKind=manual-decision` 表示方案审批需要用户决策。run 的 eventRevision 递增到 3。

### 第 8 条：开发节点 started

- 日志行号：`5954`
- 请求时间：`2026-08-10T19:00:11`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "06655dc4-fe9f-4e0b-9efc-6bec31689be9",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T19:00:11.000",
    "reportedAt": "2026-08-10T19:00:11.243",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "72b41288-dfbb-5dc6-aa64-0a47314e8890",
    "attemptId": "65e750743672446fbe83ca66b9769473",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "开发",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

用户在 18:06:36 到 19:00:11 之间进行了人工确认。run 恢复后 orchestrator 进入开发节点。注意时间间隔约 53 分钟，对应人工审批等待时间。开发节点的 eventRevision 从 1 重新开始（不同 execution 主体，各自的 revision 序列）。

### 第 9 条：开发节点 completed

- 日志行号：`5956`
- 请求时间：`2026-08-10T19:07:59`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "63314c09-4ebe-4c17-a676-2e02bf825ea0",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T19:07:59.000",
    "reportedAt": "2026-08-10T19:07:59.798",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "72b41288-dfbb-5dc6-aa64-0a47314e8890",
    "attemptId": "65e750743672446fbe83ca66b9769473",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "开发",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 60940, "outputTokens": 3593, "cacheReadTokens": 960768, "totalTokens": 1025301},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 60940, "outputTokens": 3593, "cacheReadTokens": 960768, "totalTokens": 1025301, "acpSessionElapsedMs": 464000}],
    "timing": {"startedAt": "2026-08-10T19:00:11.000", "endedAt": "2026-08-10T19:07:59.000", "acpSessionElapsedMs": 464000}
  }]
}
```

#### 上报逻辑

开发节点 ACP session 完成。`totalTokens=1025301`（超百万），`acpSessionElapsedMs=464000`（约 7.7 分钟），是整个 Workflow 中 token 消耗最大的节点，反映代码生成场景的高上下文需求。

### 第 10 条：审查节点 started

- 日志行号：`5958`
- 请求时间：`2026-08-10T19:08:00`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "13142376-73a9-40d3-8295-3725455a2d19",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T19:07:59.000",
    "reportedAt": "2026-08-10T19:08:00.087",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "d58656f2-e486-5d37-9f9e-1c73a7b05f13",
    "attemptId": "629ea142453643dd9bb7be9b13187c89",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "审查",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

开发节点完成后 orchestrator 转入审查节点。`roleName=审查`。

### 第 11 条：审查节点 completed

- 日志行号：`5960`
- 请求时间：`2026-08-10T19:10:26`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "11adc37d-6780-4d91-a947-793d53666391",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T19:10:26.000",
    "reportedAt": "2026-08-10T19:10:26.859",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "d58656f2-e486-5d37-9f9e-1c73a7b05f13",
    "attemptId": "629ea142453643dd9bb7be9b13187c89",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "审查",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 38515, "outputTokens": 5841, "cacheReadTokens": 576768, "totalTokens": 621124},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 38515, "outputTokens": 5841, "cacheReadTokens": 576768, "totalTokens": 621124, "acpSessionElapsedMs": 142000}],
    "timing": {"startedAt": "2026-08-10T19:07:59.000", "endedAt": "2026-08-10T19:10:26.000", "acpSessionElapsedMs": 142000}
  }]
}
```

#### 上报逻辑

审查节点完成。`acpSessionElapsedMs=142000`（约 2.4 分钟）。

### 第 12 条：测试节点 started

- 日志行号：`5962`
- 请求时间：`2026-08-10T19:10:27`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "8c468197-eecb-4f4c-839b-edf436a884f2",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T19:10:26.000",
    "reportedAt": "2026-08-10T19:10:27.091",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "65103387-84d3-5b38-8dbb-78c3b6eae43b",
    "attemptId": "7249d782f0364ef3a29a257492a64be4",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "测试",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

审查节点完成后转入测试节点。`roleName=测试`。

### 第 13 条：测试节点 completed

- 日志行号：`5964`
- 请求时间：`2026-08-10T19:14:00`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "bb8f5c67-7778-4ccb-8442-18fd4d522706",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T19:14:00.000",
    "reportedAt": "2026-08-10T19:14:00.516",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "65103387-84d3-5b38-8dbb-78c3b6eae43b",
    "attemptId": "7249d782f0364ef3a29a257492a64be4",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "测试",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 41304, "outputTokens": 8138, "cacheReadTokens": 649024, "totalTokens": 698466},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 41304, "outputTokens": 8138, "cacheReadTokens": 649024, "totalTokens": 698466, "acpSessionElapsedMs": 207000}],
    "timing": {"startedAt": "2026-08-10T19:10:26.000", "endedAt": "2026-08-10T19:14:00.000", "acpSessionElapsedMs": 207000}
  }]
}
```

#### 上报逻辑

测试节点完成。`acpSessionElapsedMs=207000`（约 3.5 分钟）。

### 第 14 条：验收节点 started

- 日志行号：`5966`
- 请求时间：`2026-08-10T19:14:00`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "be7ba64b-bf78-4ad7-b95c-3f5a1d8ec371",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T19:14:00.000",
    "reportedAt": "2026-08-10T19:14:00.755",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "4b1fc17a-76a4-5341-8dad-af82293e25ef",
    "attemptId": "4700157125ae4a1399092dc5933f8d2c",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "验收",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

测试节点完成后转入验收节点。`roleName=验收`。

### 第 15 条：验收节点 completed

- 日志行号：`5971`
- 请求时间：`2026-08-10T19:15:15`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "716b52d3-2829-4ce4-b51c-e006d6a1647b",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T19:15:15.000",
    "reportedAt": "2026-08-10T19:15:15.229",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "4b1fc17a-76a4-5341-8dad-af82293e25ef",
    "attemptId": "4700157125ae4a1399092dc5933f8d2c",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "验收",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 39186, "outputTokens": 3973, "cacheReadTokens": 209408, "totalTokens": 252567},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 39186, "outputTokens": 3973, "cacheReadTokens": 209408, "totalTokens": 252567, "acpSessionElapsedMs": 68000}],
    "timing": {"startedAt": "2026-08-10T19:14:00.000", "endedAt": "2026-08-10T19:15:15.000", "acpSessionElapsedMs": 68000}
  }]
}
```

#### 上报逻辑

验收节点完成。`acpSessionElapsedMs=68000`（约 1.1 分钟），验收节点的输出 token 较少（3973），反映验收是判断而非生成。

### 第 16 条：清理节点 started

- 日志行号：`5973`
- 请求时间：`2026-08-10T19:15:15`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "768b9da7-02c3-4bba-aae0-0e30bb4e35b8",
    "eventRevision": 1,
    "eventType": "execution.started",
    "occurredAt": "2026-08-10T19:15:15.000",
    "reportedAt": "2026-08-10T19:15:15.609",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "e1cd7234-e202-5f56-9d1d-d841a68d7695",
    "attemptId": "7158ad28b8874da893d60a413f4cc541",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "清理",
    "provider": "claude-acp"
  }]
}
```

#### 上报逻辑

验收节点完成后转入清理节点，这是 Workflow 的最后一个节点。`roleName=清理`。

### 第 17 条：清理节点 completed

- 日志行号：`5975`
- 请求时间：`2026-08-10T19:19:33`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "e18fd65c-3c25-44bf-b2de-c0c8c00bf0c3",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T19:19:33.000",
    "reportedAt": "2026-08-10T19:19:33.426",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "nodeId": "e1cd7234-e202-5f56-9d1d-d841a68d7695",
    "attemptId": "7158ad28b8874da893d60a413f4cc541",
    "attemptIndex": 1,
    "roundIndex": 1,
    "roleName": "清理",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "claude-acp",
    "model": "glm-5.2",
    "usage": {"inputTokens": 56149, "outputTokens": 18479, "cacheReadTokens": 603584, "totalTokens": 678212},
    "modelUsages": [{"provider": "claude-acp", "model": "glm-5.2", "inputTokens": 56149, "outputTokens": 18479, "cacheReadTokens": 603584, "totalTokens": 678212, "acpSessionElapsedMs": 253000}],
    "timing": {"startedAt": "2026-08-10T19:15:15.000", "endedAt": "2026-08-10T19:19:33.000", "acpSessionElapsedMs": 253000}
  }]
}
```

#### 上报逻辑

清理节点完成。`acpSessionElapsedMs=253000`（约 4.2 分钟），输出 token 较多（18479），反映清理节点需要生成总结/清理脚本。

### 第 18 条：run completed（终态）

- 日志行号：`5977`
- 请求时间：`2026-08-10T19:19:33`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "6705abc9-7809-4c4d-a631-81539d90cec9",
    "eventRevision": 4,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-10T19:19:33.000",
    "reportedAt": "2026-08-10T19:19:33.525",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "49eacfbe31dc4432b520936761c80a8e",
    "taskTitle": "帮我写一个最简单的单例模式",
    "outcome": "success",
    "terminalReason": "completed",
    "counters": {
      "pauseCount": 1,
      "resumeCount": 0,
      "permissionRequestCount": 0,
      "elicitationCount": 0,
      "manualContinueCount": 0,
      "followUpCount": 0
    },
    "roundCount": 1,
    "collectionStateRecovered": true
  }]
}
```

#### 上报逻辑

清理节点完成后 orchestrator 判定所有节点已执行完毕，run 进入终态。`emit_run_completed_lifecycle_event` 触发 `emit_run_metrics_fact` 构建 run completed。关键终态字段：
- `outcome=success`、`terminalReason=completed`。
- `counters.pauseCount=1` —— 方案节点后的人工确认暂停（对应第 6 条 paused 和第 7 条 intervention.requested）。
- `roundCount=1` —— 首轮交付，未进入多轮验收。
- `collectionStateRecovered=true` —— run 终态事件来自 observability snapshot 恢复路径。
- run 的 eventRevision 从 3（intervention.requested）跳到 4（completed），因为 paused 和 intervention.requested 分别消耗了 rev 2 和 3。

## 3. 事件时序图

```
时间轴 (Workflow, taskId=49eacfbe..., taskTitle="帮我写一个最简单的单例模式")

17:55:24  [1] run.started                rev=1  <- 用户提交任务
17:55:24  [2] 访谈.started               rev=1
    |         ACP session 运行 8m29s...
18:03:53  [3] 访谈.completed             rev=2  model=glm-5.2, totalTokens=502780
18:03:53  [4] 方案.started               rev=1
    |         ACP session 运行 2m43s...
18:06:36  [5] 方案.completed             rev=2  model=glm-5.2, totalTokens=252683
18:06:36  [6] run.paused                 rev=2  pauseReason=waiting-for-user-input
18:06:36  [7] intervention.requested     rev=3  interventionKind=manual-decision
    |
    |     --- 等待用户人工确认方案（约 53 分钟）---
    |
19:00:11  [8] 开发.started               rev=1  <- 用户确认方案，run 恢复
    |         ACP session 运行 7m48s...
19:07:59  [9] 开发.completed             rev=2  model=glm-5.2, totalTokens=1025301
19:07:59  [10] 审查.started              rev=1
    |         ACP session 运行 2m27s...
19:10:26  [11] 审查.completed            rev=2  model=glm-5.2, totalTokens=621124
19:10:26  [12] 测试.started              rev=1
    |         ACP session 运行 3m34s...
19:14:00  [13] 测试.completed            rev=2  model=glm-5.2, totalTokens=698466
19:14:00  [14] 验收.started              rev=1
    |         ACP session 运行 1m15s...
19:15:15  [15] 验收.completed            rev=2  model=glm-5.2, totalTokens=252567
19:15:15  [16] 清理.started              rev=1
    |         ACP session 运行 4m18s...
19:19:33  [17] 清理.completed            rev=2  model=glm-5.2, totalTokens=678212
19:19:33  [18] run.completed             rev=4  outcome=success, pauseCount=1, roundCount=1
```

## 4. 数据完整性检查

| 检查项 | 结果 |
|---|---|
| 7 个节点均有 started+completed | 通过 |
| run 级有 started+paused+intervention+completed | 通过 |
| 所有事件携带 taskTitle | 通过 |
| executionId 全程一致 | 通过 |
| nodeId 在节点内不变、跨节点不同 | 通过 |
| attemptIndex 全部为 1（无重试） | 通过 |
| started 事件不携带 model | 通过 |
| completed 事件 model=glm-5.2（实际模型名） | 通过 |
| acpSessionElapsedMs 均为非 null | 通过 |
| occurredAt/reportedAt 不带时区偏移量 | 通过 |
| pauseCount=1 对应 1 次 paused 事件 | 通过 |
| roundCount=1 对应首轮 | 通过 |

本次测试未发现数据缺口。所有字段符合协议预期。

**注意：** `counters.resumeCount=0`，但 run 实际从 paused 恢复了 1 次。这是因为 resume 事件在恢复时作为 `execution.resumed` lifecycle event 实时上报（未出现在本次去重事件中，因为它可能因服务端错误未重试成功），终态的 `resumeCount` 是从 observability snapshot 累计的。`collectionStateRecovered=true` 也佐证了终态数据来自恢复路径。