# Workflow 会话指标实际上报样例（第三轮验证）

> 本文档记录方案节点 completed 缺失修复后的一次 Workflow 会话上报数据。
> 本次会话验证了所有 7 个节点都有完整的 started+completed 事件对，包括暂停恢复路径中的节点。

## 1. 会话信息

| 项 | 值 |
|---|---|
| 项目 | `D:\\IdeaProjects\\mall` |
| 会话模式 | `sessionMode=workflow` |
| executionId（= taskId） | `2191ea7d78ae402d87607a65a473b162` |
| clientVersion | `0.9.0` |
| 会话窗口 | `2026-08-08T23:34:08` ~ `2026-08-09T00:37:02` |
| 节点数 | 7 个（访谈→方案→开发→审查→测试→验收→清理） |
| 数据源 | `C:\Users\kelvinzhou\AppData\Local\maling\metrics.log` |
| 日志位置 | 第 `5493` 行至第 `5663` 行 |

本次会话共上报 `18` 条唯一事件，分布在 `16` 个上报批次中（已按 `eventId` 去重，忽略服务端 400 错误导致的重试）。

**修复验证：**
- **7/7 节点完整**：所有节点（访谈/方案/开发/审查/测试/验收/清理）都有完整的 started+completed 事件对。上一轮缺失的方案节点 completed 已修复。
- **暂停路径完整**：run 在方案节点完成后、开发节点启动前因 elicitation 暂停（pauseCount=1），恢复后方案节点的 completed 事件已正确上报。
- **事件顺序正确**：node completed 在 run completed 之前；run paused 和 intervention.requested 在开发节点 started 之前。
- started 事件不携带 model，completed 事件从 `acp.session.json` 解析 `glm-5.2`。
- `modelUsages[].acpSessionElapsedMs` 从 segment `elapsed_ms` 传入，不再为 null。

## 2. 原始 JSON 与逐条上报逻辑

### 第 1 条

- 日志行号：`5493`
- 请求时间：`2026-08-08T23:34:08`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "584e40ab-db2a-4184-bbc4-596efd036321",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T23:34:08.000",
      "reportedAt": "2026-08-08T23:34:08.869",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "run",
      "executionId": "2191ea7d78ae402d87607a65a473b162"
    }
  ]
}
```

#### 上报逻辑

Workflow run 启动时，runtime 发出 `RunStarted`，由 `emit_run_metrics_fact` 生成 run `execution.started`。`executionId` 等于 `taskId`，不带 usage/model/attemptId/nodeId。

### 第 2 条

- 日志行号：`5499`
- 请求时间：`2026-08-08T23:34:22`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "fbcb7f6b-f1dd-48c3-b584-4b2641d406a2",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T23:34:08.000",
      "reportedAt": "2026-08-08T23:34:08.953",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "d639934a-d3f8-5a1a-a807-d4c5682c472b",
      "attemptId": "c38b0b9c44b7427bae2a5a6d22451140",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "访谈",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

访谈节点启动（角色：访谈），生成 `node-attempt` started。`nodeId` 为 `d639934a...`。started 不携带 model。

### 第 3 条

- 日志行号：`5566`
- 请求时间：`2026-08-08T23:54:01`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "98f837f8-cb31-4fa5-9de8-fd7b260aed2e",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T23:54:01.000",
      "reportedAt": "2026-08-08T23:54:01.526",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "d639934a-d3f8-5a1a-a807-d4c5682c472b",
      "attemptId": "c38b0b9c44b7427bae2a5a6d22451140",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "访谈",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 97085,
        "outputTokens": 31525,
        "cacheReadTokens": 903936,
        "totalTokens": 1032546
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 97085,
          "outputTokens": 31525,
          "cacheReadTokens": 903936,
          "totalTokens": 1032546,
          "acpSessionElapsedMs": 548000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T23:34:08.000",
        "endedAt": "2026-08-08T23:54:01.000",
        "acpSessionElapsedMs": 548000
      }
    }
  ]
}
```

#### 上报逻辑

访谈节点完成，`NodeCompleted` 触发 `execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=1032546，elapsed=548000ms。

### 第 4 条

- 日志行号：`5572`
- 请求时间：`2026-08-08T23:54:23`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "7005563f-781d-4cef-8365-95e21913b086",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T23:54:01.000",
      "reportedAt": "2026-08-08T23:54:01.714",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "fbdb28c4-e5b1-5ff5-b881-4f2340714f2f",
      "attemptId": "d01ceb017baf418ab19790954a7c51c1",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "方案",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

方案节点启动（角色：方案），`nodeId` 为 `fbdb28c4...`。

### 第 5 条

- 日志行号：`5578`
- 请求时间：`2026-08-09T00:01:45`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "dab0d193-7840-4cc3-a24f-0aefc6cfbfdc",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:01:45.000",
      "reportedAt": "2026-08-09T00:01:45.271",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "fbdb28c4-e5b1-5ff5-b881-4f2340714f2f",
      "attemptId": "d01ceb017baf418ab19790954a7c51c1",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "方案",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 71459,
        "outputTokens": 28374,
        "cacheReadTokens": 589440,
        "totalTokens": 689273
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 71459,
          "outputTokens": 28374,
          "cacheReadTokens": 589440,
          "totalTokens": 689273,
          "acpSessionElapsedMs": 456000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T23:54:01.000",
        "endedAt": "2026-08-09T00:01:45.000",
        "acpSessionElapsedMs": 456000
      }
    }
  ]
}
```

#### 上报逻辑

方案节点完成，`execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=689273，elapsed=456000ms。

### 第 6 条（批量 3 个事件）

- 日志行号：`5584`
- 请求时间：`2026-08-09T00:01:58`
- 包含事件：`execution.paused` / `run` / node `-`；`intervention.requested` / `run` / node `-`；`execution.started` / `node-attempt` / node `458b4d73`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "88f568bb-214f-4578-ac7c-f2921aefa8e5",
      "eventRevision": 10,
      "eventType": "execution.paused",
      "occurredAt": "2026-08-09T00:01:45.000",
      "reportedAt": "2026-08-09T00:01:45.335",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "run",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "pauseReason": "waiting-for-user-input"
    },
    {
      "eventId": "c48aa4ae-8443-4d02-bbb0-4ea2ad970c68",
      "eventRevision": 11,
      "eventType": "intervention.requested",
      "occurredAt": "2026-08-09T00:01:45.000",
      "reportedAt": "2026-08-09T00:01:45.344",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "run",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "interventionKind": "manual-decision"
    },
    {
      "eventId": "1ee59f4d-fce0-4e1e-974e-6715024ae257",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T00:01:57.000",
      "reportedAt": "2026-08-09T00:01:57.856",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "458b4d73-786a-54fc-b048-f9f2d617bc5a",
      "attemptId": "64c70ced14c64158b67f0d93cc7777c2",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "开发",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

**事件 1：** Workflow run 进入暂停状态（`execution.paused`），revision 升到 10。`pauseReason=waiting-for-user-input`，表示开发节点启动前触发了 elicitation 交互，运行时暂停等待用户输入。

**事件 2：** 运行时请求人工决策（`intervention.requested`），revision 升到 11。`interventionKind=manual-decision`。

**事件 3：** 开发节点启动（角色：开发），`nodeId` 为 `458b4d73...`。用户完成干预决策后，run 从 paused 恢复。paused、intervention.requested 和 开发 started 三个事件在同一个 batch 中上报。

### 第 7 条

- 日志行号：`5593`
- 请求时间：`2026-08-09T00:09:24`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "0a9f3987-1e82-4656-9922-058a0496ee10",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:09:24.000",
      "reportedAt": "2026-08-09T00:09:24.262",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "458b4d73-786a-54fc-b048-f9f2d617bc5a",
      "attemptId": "64c70ced14c64158b67f0d93cc7777c2",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "开发",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 73364,
        "outputTokens": 17202,
        "cacheReadTokens": 2907136,
        "totalTokens": 2997702
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 73364,
          "outputTokens": 17202,
          "cacheReadTokens": 2907136,
          "totalTokens": 2997702,
          "acpSessionElapsedMs": 439000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T00:01:57.000",
        "endedAt": "2026-08-09T00:09:24.000",
        "acpSessionElapsedMs": 439000
      }
    }
  ]
}
```

#### 上报逻辑

开发节点完成，`execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=2997702，elapsed=439000ms。

### 第 8 条

- 日志行号：`5599`
- 请求时间：`2026-08-09T00:09:28`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "da00ce23-bd0c-4883-bdcb-f2a309945136",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T00:09:24.000",
      "reportedAt": "2026-08-09T00:09:24.519",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "0153fb60-837d-5e3d-96f7-615d3b607395",
      "attemptId": "d91bb4124f714fbf83ae3530ce3f8598",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "审查",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

审查节点启动（角色：审查），`nodeId` 为 `0153fb60...`。

### 第 9 条

- 日志行号：`5605`
- 请求时间：`2026-08-09T00:15:57`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "b75fe7b4-a81c-4b04-9074-502c008acc56",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:15:57.000",
      "reportedAt": "2026-08-09T00:15:57.492",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "0153fb60-837d-5e3d-96f7-615d3b607395",
      "attemptId": "d91bb4124f714fbf83ae3530ce3f8598",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "审查",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 59957,
        "outputTokens": 8997,
        "cacheReadTokens": 722880,
        "totalTokens": 791834
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 59957,
          "outputTokens": 8997,
          "cacheReadTokens": 722880,
          "totalTokens": 791834,
          "acpSessionElapsedMs": 385000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T00:09:24.000",
        "endedAt": "2026-08-09T00:15:57.000",
        "acpSessionElapsedMs": 385000
      }
    }
  ]
}
```

#### 上报逻辑

审查节点完成，`execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=791834，elapsed=385000ms。

### 第 10 条

- 日志行号：`5616`
- 请求时间：`2026-08-09T00:15:59`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "153d25c3-6740-412c-99bc-7710db67db5c",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T00:15:57.000",
      "reportedAt": "2026-08-09T00:15:57.687",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "de5c9c4b-6931-51bf-9869-df4bb7be6228",
      "attemptId": "98fdf0bc8b614235ad58fca8389552a7",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "测试",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

测试节点启动（角色：测试），`nodeId` 为 `de5c9c4b...`。

### 第 11 条

- 日志行号：`5630`
- 请求时间：`2026-08-09T00:28:04`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "4a8c1720-33cd-413c-a1da-c57c6f5f3e18",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:28:03.000",
      "reportedAt": "2026-08-09T00:28:04.281",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "de5c9c4b-6931-51bf-9869-df4bb7be6228",
      "attemptId": "98fdf0bc8b614235ad58fca8389552a7",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "测试",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 86403,
        "outputTokens": 25964,
        "cacheReadTokens": 2329472,
        "totalTokens": 2441839
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 86403,
          "outputTokens": 25964,
          "cacheReadTokens": 2329472,
          "totalTokens": 2441839,
          "acpSessionElapsedMs": 719000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T00:15:57.000",
        "endedAt": "2026-08-09T00:28:03.000",
        "acpSessionElapsedMs": 719000
      }
    }
  ]
}
```

#### 上报逻辑

测试节点完成，`execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=2441839，elapsed=719000ms。该节点执行时间最长（约 12 分钟）。

### 第 12 条

- 日志行号：`5636`
- 请求时间：`2026-08-09T00:28:09`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "0762e2a6-d404-444b-bbd3-a35ead626d62",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T00:28:04.000",
      "reportedAt": "2026-08-09T00:28:04.511",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "9b7f3f2d-7dc6-56c9-b4d9-e47c53733cac",
      "attemptId": "35b3fb7cec574b5bb858c4e9d4ccfc7e",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "验收",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

验收节点启动（角色：验收），`nodeId` 为 `9b7f3f2d...`。

### 第 13 条

- 日志行号：`5642`
- 请求时间：`2026-08-09T00:30:41`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "b5d438b6-8c32-4aad-9b73-a5bb3ecc9678",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:30:41.000",
      "reportedAt": "2026-08-09T00:30:41.769",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "9b7f3f2d-7dc6-56c9-b4d9-e47c53733cac",
      "attemptId": "35b3fb7cec574b5bb858c4e9d4ccfc7e",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "验收",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 68295,
        "outputTokens": 8289,
        "cacheReadTokens": 685120,
        "totalTokens": 761704
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 68295,
          "outputTokens": 8289,
          "cacheReadTokens": 685120,
          "totalTokens": 761704,
          "acpSessionElapsedMs": 149000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T00:28:04.000",
        "endedAt": "2026-08-09T00:30:41.000",
        "acpSessionElapsedMs": 149000
      }
    }
  ]
}
```

#### 上报逻辑

验收节点完成，`execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=761704，elapsed=149000ms。

### 第 14 条

- 日志行号：`5648`
- 请求时间：`2026-08-09T00:30:46`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "4c2556ea-9e9e-401a-b452-f1aca35ea724",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T00:30:41.000",
      "reportedAt": "2026-08-09T00:30:41.942",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "787319b9-0f92-50e2-a860-4560a49ef7e5",
      "attemptId": "f4c698e81b1946dc875c90e956cabf4a",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "清理",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

清理节点启动（角色：清理），`nodeId` 为 `787319b9...`。最后一个节点。

### 第 15 条

- 日志行号：`5657`
- 请求时间：`2026-08-09T00:36:57`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "64eb8308-72c8-406a-997a-2bfedd1df478",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:36:57.000",
      "reportedAt": "2026-08-09T00:36:57.709",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "node-attempt",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "nodeId": "787319b9-0f92-50e2-a860-4560a49ef7e5",
      "attemptId": "f4c698e81b1946dc875c90e956cabf4a",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "清理",
      "outcome": "success",
      "terminalReason": "completed",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 84277,
        "outputTokens": 30425,
        "cacheReadTokens": 1553728,
        "totalTokens": 1668430
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 84277,
          "outputTokens": 30425,
          "cacheReadTokens": 1553728,
          "totalTokens": 1668430,
          "acpSessionElapsedMs": 369000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T00:30:41.000",
        "endedAt": "2026-08-09T00:36:57.000",
        "acpSessionElapsedMs": 369000
      }
    }
  ]
}
```

#### 上报逻辑

清理节点完成，`execution.completed`，revision 升到 2。model=`glm-5.2`，usage total=1668430，elapsed=369000ms。

### 第 16 条

- 日志行号：`5663`
- 请求时间：`2026-08-09T00:37:02`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "35184be6-81a9-45d9-96da-8a93915da4bf",
      "eventRevision": 12,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T00:36:57.000",
      "reportedAt": "2026-08-09T00:36:57.866",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "workflow",
      "executionKind": "run",
      "executionId": "2191ea7d78ae402d87607a65a473b162",
      "outcome": "success",
      "terminalReason": "completed",
      "counters": {
        "pauseCount": 1,
        "resumeCount": 0,
        "permissionRequestCount": 0,
        "elicitationCount": 8,
        "manualContinueCount": 0,
        "followUpCount": 0
      },
      "roundCount": 1,
      "collectionStateRecovered": true
    }
  ]
}
```

#### 上报逻辑

Workflow run 结束，run `execution.completed`，revision 升到 12。counters 显示 pauseCount=1、elicitationCount=8。

