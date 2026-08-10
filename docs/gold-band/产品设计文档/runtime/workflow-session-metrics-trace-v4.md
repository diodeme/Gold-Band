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

本次会话共 `22` 条（含 4 条访谈节点 elicitation 干预事件）唯一事件，分布在 `21` 个上报批次中（已按 `eventId` 去重，忽略服务端错误导致的重试）。

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
### 第 3a 条：访谈节点 elicitation 干预（第 1 次）

- 日志行号：`5905`
- 请求时间：`2026-08-10T17:56:56`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "54aa67b3-d213-401d-9ad6-d4b18982b7e0",
    "eventRevision": 2,
    "eventType": "intervention.requested",
    "occurredAt": "2026-08-10T17:56:56.000",
    "reportedAt": "2026-08-10T17:56:56.179",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "cc17bea4d4d44073b7fe283fd9c66058",
    "taskTitle": "帮我写一个最简单的单例模式",
    "interventionKind": "elicitation",
    "collectionStateRecovered": true
  }]
}
```

#### 上报逻辑

访谈节点执行过程中，AI agent 通过 ACP elicitation（AskUserQuestion）向用户提问。`maybe_emit_elicitation_intervention` 检测到 elicitation 事件后调用 `emit_request_intervention_metrics`，由后台 worker 构建 `intervention.requested` 事件。`interventionKind=elicitation` 表示这是 AI 主动向用户提问。

> **异常：** 此事件的 `executionId` 为 `cc17bea4d4d44073b7fe283fd9c66058`（run UUID），而不是 `49eacfbe31dc4432b520936761c80a8e`（task UUID）。详见第 5 节分析。

### 第 3b 条：访谈节点 elicitation 干预（第 2 次）

- 日志行号：`5914`
- 请求时间：`2026-08-10T18:00:09`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "3000e99b-19ef-42c0-8c5e-9620fa839c54",
    "eventRevision": 3,
    "eventType": "intervention.requested",
    "occurredAt": "2026-08-10T18:00:09.000",
    "reportedAt": "2026-08-10T18:00:09.338",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "cc17bea4d4d44073b7fe283fd9c66058",
    "taskTitle": "帮我写一个最简单的单例模式",
    "interventionKind": "elicitation",
    "collectionStateRecovered": true
  }]
}
```

#### 上报逻辑

访谈节点继续执行，AI agent 发起第 2 次 elicitation 提问。同一 executionId（run UUID）下的 eventRevision 递增到 3。

### 第 3c 条：访谈节点 elicitation 干预（第 3 次）

- 日志行号：`5916`
- 请求时间：`2026-08-10T18:01:28`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "1b80e69c-bded-458c-a633-bb909e7786c0",
    "eventRevision": 4,
    "eventType": "intervention.requested",
    "occurredAt": "2026-08-10T18:01:28.000",
    "reportedAt": "2026-08-10T18:01:28.303",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "cc17bea4d4d44073b7fe283fd9c66058",
    "taskTitle": "帮我写一个最简单的单例模式",
    "interventionKind": "elicitation",
    "collectionStateRecovered": true
  }]
}
```

#### 上报逻辑

第 3 次 elicitation 提问，eventRevision 递增到 4。

### 第 3d 条：访谈节点 elicitation 干预（第 4 次）

- 日志行号：`5918`
- 请求时间：`2026-08-10T18:03:00`

#### 原始 JSON

```json
{
  "events": [{
    "eventId": "5eec28b1-e904-4cfb-ae0e-75aa0547cbcc",
    "eventRevision": 5,
    "eventType": "intervention.requested",
    "occurredAt": "2026-08-10T18:03:00.000",
    "reportedAt": "2026-08-10T18:03:00.376",
    "userId": "kelvinzhou",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.9.0",
    "sessionMode": "workflow",
    "executionKind": "run",
    "executionId": "cc17bea4d4d44073b7fe283fd9c66058",
    "taskTitle": "帮我写一个最简单的单例模式",
    "interventionKind": "elicitation",
    "collectionStateRecovered": true
  }]
}
```

#### 上报逻辑

第 4 次（最后一次）elicitation 提问，eventRevision 递增到 5。此后访谈节点 ACP session 完成（第 3 条 completed）。


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
    |         ACP session 运行中...
17:56:56  [3a] elicitation              rev=2  execId=cc17bea4...(runUuid, BUG)
18:00:09  [3b] elicitation              rev=3  execId=cc17bea4...(runUuid, BUG)
18:01:28  [3c] elicitation              rev=4  execId=cc17bea4...(runUuid, BUG)
18:03:00  [3d] elicitation              rev=5  execId=cc17bea4...(runUuid, BUG)
    |         ACP session 继续运行...
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
| executionId 全程一致（主线事件） | 通过（49eacfbe/taskUuid） |
| elicitation 事件 executionId 分离 | 异常（cc17bea4/runUuid），已修复 |
| nodeId 在节点内不变、跨节点不同 | 通过 |
| attemptIndex 全部为 1（无重试） | 通过 |
| started 事件不携带 model | 通过 |
| completed 事件 model=glm-5.2（实际模型名） | 通过 |
| acpSessionElapsedMs 均为非 null | 通过 |
| occurredAt/reportedAt 不带时区偏移量 | 通过 |
| pauseCount=1 对应 1 次 paused 事件 | 通过 |
| roundCount=1 对应首轮 | 通过 |

本次测试发现 1 个数据异常：elicitation 干预事件 executionId 分离（详见第 5 节），已在代码中修复。其余字段符合协议预期。

**注意：** `counters.resumeCount=0`，但 run 实际从 paused 恢复了 1 次。这是因为 resume 事件在恢复时作为 `execution.resumed` lifecycle event 实时上报（未出现在本次去重事件中，因为它可能因服务端错误未重试成功），终态的 `resumeCount` 是从 observability snapshot 累计的。`collectionStateRecovered=true` 也佐证了终态数据来自恢复路径。
## 5. 数据缺口：elicitation 干预事件 executionId 分离（已修复）

### 现象

4 条访谈节点的 elicitation 干预事件（第 3a-3d 条）使用了独立的 `executionId=cc17bea4d4d44073b7fe283fd9c66058`，与 run 主线事件的 `executionId=49eacfbe31dc4432b520936761c80a8e` 不一致。

| 事件来源 | executionId | 含义 |
|---|---|---|
| run started/paused/intervention/completed（orchestrator 路径） | `49eacfbe...` | task UUID（正确） |
| node started/completed（producer 路径） | `49eacfbe...` | task UUID（正确） |
| elicitation intervention（Tauri 层路径） | `cc17bea4...` | run UUID（错误） |

### 根因

`build_request_intervention_metrics`（commands.rs）构建干预事件的 `execution_id` 时：

```rust
let execution_id = active_turn
    .as_ref()
    .map(|turn| turn.execution_id.clone())
    .unwrap_or_else(|| run_uuid.clone());  // BUG: 应为 task_uuid
```

- **Direct 模式：** `active_turn` 存在，`execution_id` 取 `turn.execution_id`（等于 taskUuid），正确。
- **Workflow/AUTO 模式：** `active_turn` 为 None，fallback 使用 `run_uuid`，而统一后的正确值应为 `task_uuid`。

这导致 Workflow/AUTO 的 elicitation/permission 干预事件与 run 主线事件产生两个不同的 executionId，服务端无法将它们关联到同一次交付。

### 影响

1. 服务端 `ml_metric_delivery_stat` 会出现两行记录（一个 taskUuid + 一个 runUuid），同一 run 的 counters 被拆分。
2. `elicitationCount` 在 runUuid 行累加，但 taskUuid 行（终态 delivery）的 counters 中没有这些 elicitation 计数。
3. 服务端按 executionId 查询 attempt 时无法找到这些干预事件。

### 修复

将 fallback 从 `run_uuid.clone()` 改为 `task_uuid.clone()`：

```rust
let execution_id = active_turn
    .as_ref()
    .map(|turn| turn.execution_id.clone())
    .unwrap_or_else(|| task_uuid.clone());  // FIXED
```

修复后，Workflow/AUTO 的干预事件 executionId 与 run 主线一致（都为 taskUuid），eventRevision 共享同一序列。

### 补充：为什么 elicitation 事件的 eventRevision 从 2 开始

`emit_run_metrics_fact`（orchestrator）和 `build_request_intervention_metrics`（Tauri 层）在修复前都使用 `run_uuid` 作为 observability state 的 HashMap key。这意味着它们共享同一个 `eventRevision` 计数器：

1. `run.started`（orchestrator，prepare_run 阶段）→ `next_revision()` → **rev=1**
2. 第 1 次 elicitation（Tauri 层）→ 同一 state → `next_revision()` → **rev=2**
3. 第 2 次 elicitation → **rev=3**
4. 第 3 次 elicitation → **rev=4**
5. 第 4 次 elicitation → **rev=5**

因此 elicitation 事件的 `eventRevision` 从 2 开始，而非 1。修复后，elicitation 事件改用 `task_uuid` 作为 executionId 和 observability state key，与 run 级事件使用不同的 state（run 级用 `run_uuid` key），revision 序列归为各自独立。

> **注意：** run 级事件（paused=rev2, intervention=rev3, completed=rev4）的 revision 序列看起来与 elicitation 事件（rev2~5）有重叠。这是因为 `emit_run_metrics_fact` 内部的 observability state key 使用 `run_uuid`，而修复后 fact 中的 `executionId` 使用 `task_uuid`。两者 key 不同导致各自的 revision 计数器独立递增。这个 key 与 fact executionId 不一致的问题是一个已知的技术债，后续应统一为同一 key。
