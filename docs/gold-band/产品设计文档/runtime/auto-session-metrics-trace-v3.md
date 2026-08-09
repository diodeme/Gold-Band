# AUTO 会话指标实际上报样例（变更后第三轮验证）

> 本文档记录执行单元模型第二轮变更后的一次 AUTO 会话上报数据。
> 本次会话验证了 `executionId = taskId`、`nodeId` 对 AUTO unit 设置、`attemptId` 从 `node_uuid` 派生、started 事件不携带 model、`modelUsages.acpSessionElapsedMs` 不再为 null。

## 1. 会话信息

| 项 | 值 |
|---|---|
| 项目 | `D:\IdeaProjects\mall` |
| 会话模式 | `sessionMode=auto` |
| executionId（= taskId） | `60f8081e93ae4414a41887801818f4bd` |
| clientVersion | `0.9.0` |
| 会话窗口 | `2026-08-08T19:23:03` ~ `2026-08-08T19:39:24` |
| 数据源 | `C:\Users\kelvinzhou\AppData\Local\maling\metrics.log` |
| 日志位置 | 第 `4938` 行至第 `5033` 行（去重后唯一事件） |

本次会话共 `13` 条唯一事件，分布在 `11` 个上报批次中（已按 `eventId` 去重，忽略服务端 400 错误导致的重试）。

**本轮变更验证要点：**
- `executionId` 统一为 `taskId`（`60f8081e93ae...`），所有事件共享，不再有 `parentExecutionId` 或 `taskId` 字段。
- `nodeId` 对所有 AUTO unit 事件设置，三个 worker 各有独立 `nodeId`（`271965e8`、`83702ec2`、`24eb71c4`），不再碰撞。
- `attemptId` 由 `derive_attempt_id(nodeId, localAttemptId)` 派生，不同 worker 的 `attemptId` 不再碰撞，各自拥有独立的 observability state。
- started 事件不携带 `model`（值为 `null`），completed 事件从 `acp.session.json` 解析实际模型名 `glm-5.2`。
- `modelUsages[].acpSessionElapsedMs` 不再为 `null`，从 segment 的 `elapsed_ms` 传入。
- `unitId` 字段已删除，节点标识统一用 `nodeId`。

## 2. 原始 JSON 与逐条上报逻辑

### 第 1 条

- 日志行号：`4938`
- 请求时间：`2026-08-08T19:23:03`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "fb1fe682-4d62-4bea-bbd5-5c5ba35694f4",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T19:23:03.000",
      "reportedAt": "2026-08-08T19:23:03.794",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "outer-run",
      "executionId": "60f8081e93ae4414a41887801818f4bd"
    }
  ]
}
```

#### 上报逻辑

AUTO 外层 run 启动时，runtime 发出 `RunStarted`，由 `emit_run_metrics_fact` 生成 outer-run started。`executionId` 等于 `taskId`，不带 usage/model/attemptId。这是整个 AUTO 交付的入口事件。

### 第 2 条

- 日志行号：`4944`
- 请求时间：`2026-08-08T19:23:06`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "bf3ce5d1-bb85-432b-8a2c-334489597b1f",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T19:23:03.000",
      "reportedAt": "2026-08-08T19:23:04.074",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "271965e8baf743e992862a153c1a3904",
      "attemptId": "f0f33ef2-d9f3-5daa-b6b6-1d9fcd5c8057",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "AI-DYNAMIC bootstrap",
      "unitKind": "worker",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

第一个 worker 单元启动（AI-DYNAMIC bootstrap），`emit_derived_node_metrics_fact` 识别到 `dynamic_kind=worker`，生成 `unit-attempt` started。`nodeId` 为 `271965e8...`，`attemptId` 由 `derive_attempt_id(nodeUuid, localAttemptId)` 派生。started 不携带 `model`。

### 第 3 条

- 日志行号：`4950`
- 请求时间：`2026-08-08T19:24:49`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "6fc76b1b-49a8-44e5-be2e-65668434c389",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T19:24:49.000",
      "reportedAt": "2026-08-08T19:24:49.768",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "271965e8baf743e992862a153c1a3904",
      "attemptId": "f0f33ef2-d9f3-5daa-b6b6-1d9fcd5c8057",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "AI-DYNAMIC bootstrap",
      "outcome": "success",
      "terminalReason": "completed",
      "unitKind": "worker",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 51506,
        "outputTokens": 4107,
        "cacheReadTokens": 1088,
        "totalTokens": 56701
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 51506,
          "outputTokens": 4107,
          "cacheReadTokens": 1088,
          "totalTokens": 56701,
          "acpSessionElapsedMs": 83000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T19:23:03.000",
        "endedAt": "2026-08-08T19:24:49.000",
        "acpSessionElapsedMs": 83000
      }
    }
  ]
}
```

#### 上报逻辑

第一个 worker 的 ACP prompt 完成后，`NodeCompleted` 触发同一个 attempt 的 `execution.completed`，revision 升到 2。model 从 `acp.session.json` 解析为 `glm-5.2`，usage total=56701，elapsed=83000ms。

### 第 4 条（批量 2 个事件）

- 日志行号：`4956`
- 请求时间：`2026-08-08T19:24:51`
- 包含事件：`execution.started` / `unit-attempt` / node `83702ec2`；`execution.started` / `unit-attempt` / node `24eb71c4`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "4d22a7aa-d344-425d-8715-f1a1873abcb0",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T19:24:49.000",
      "reportedAt": "2026-08-08T19:24:50.142",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "83702ec2702c4e048f83ed0f0e43dcab",
      "attemptId": "9111c196-2c52-5c6a-b8fb-b0eec7410fe6",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "功能盘点与业务缺口分析",
      "unitKind": "worker",
      "provider": "claude-acp"
    },
    {
      "eventId": "dd85a52e-5ac3-4a8e-86b0-4f26c2f3c191",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T19:24:50.000",
      "reportedAt": "2026-08-08T19:24:50.374",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "24eb71c411dc4fee91d0d97ef1e26f08",
      "attemptId": "2f5d6b64-2f1c-5ca8-a32b-1fb6ca82a8b3",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "现有功能增强与改进点分析",
      "unitKind": "worker",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

**事件 1：** 第二个和第三个并行 worker 同时启动，在同一个 batch 中上报两个 `execution.started` 事件。第二个 worker（功能盘点与业务缺口分析）`nodeId=83702ec2...`，第三个 worker（现有功能增强与改进点分析）`nodeId=24eb71c4...`，各自独立派生 `attemptId`。两个 started 都不携带 model。

**事件 2：** 第三个 worker（现有功能增强与改进点分析）完成，针对自己的 `nodeId=24eb71c4...` 上报 `execution.completed`，usage total=526702，elapsed=373000ms。

### 第 5 条

- 日志行号：`4962`
- 请求时间：`2026-08-08T19:31:15`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "1ea37c52-c4fd-4df6-bd48-b85940a17ad4",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T19:31:15.000",
      "reportedAt": "2026-08-08T19:31:15.959",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "24eb71c411dc4fee91d0d97ef1e26f08",
      "attemptId": "2f5d6b64-2f1c-5ca8-a32b-1fb6ca82a8b3",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "现有功能增强与改进点分析",
      "outcome": "success",
      "terminalReason": "completed",
      "unitKind": "worker",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 49661,
        "outputTokens": 9841,
        "cacheReadTokens": 467200,
        "totalTokens": 526702
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 49661,
          "outputTokens": 9841,
          "cacheReadTokens": 467200,
          "totalTokens": 526702,
          "acpSessionElapsedMs": 373000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T19:24:50.000",
        "endedAt": "2026-08-08T19:31:15.000",
        "acpSessionElapsedMs": 373000
      }
    }
  ]
}
```

#### 上报逻辑

第二个 worker（功能盘点与业务缺口分析）完成，针对自己的 `nodeId=83702ec2...` 上报 `execution.completed`，usage total=530474，elapsed=488000ms。该 worker 执行时间最长（约 8 分钟）。

### 第 6 条

- 日志行号：`4973`
- 请求时间：`2026-08-08T19:33:10`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "2751ff3a-8563-4b4a-a48f-bd5efe51684f",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T19:33:09.000",
      "reportedAt": "2026-08-08T19:33:10.262",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "83702ec2702c4e048f83ed0f0e43dcab",
      "attemptId": "9111c196-2c52-5c6a-b8fb-b0eec7410fe6",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "功能盘点与业务缺口分析",
      "outcome": "success",
      "terminalReason": "completed",
      "unitKind": "worker",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 49011,
        "outputTokens": 12855,
        "cacheReadTokens": 468608,
        "totalTokens": 530474
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 49011,
          "outputTokens": 12855,
          "cacheReadTokens": 468608,
          "totalTokens": 530474,
          "acpSessionElapsedMs": 487000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T19:24:49.000",
        "endedAt": "2026-08-08T19:33:09.000",
        "acpSessionElapsedMs": 488000
      }
    }
  ]
}
```

#### 上报逻辑

所有 worker 结束后 merge 节点启动（综合需求建议），生成 merge `unit-attempt` started。`nodeId=9c7a324f...`。started 不携带 model。

### 第 7 条

- 日志行号：`4984`
- 请求时间：`2026-08-08T19:33:11`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "126515d6-3bdb-4e19-ad86-92679c70f392",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T19:33:11.000",
      "reportedAt": "2026-08-08T19:33:11.391",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "9c7a324f32164a118c6744c8b717c801",
      "attemptId": "8bf65316-929d-554e-855b-cc549125d17b",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "综合需求建议",
      "unitKind": "merge",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

merge 的 ACP 完成后，completed 事件补上 model=`glm-5.2`、usage total=477685、elapsed=199000ms，revision 升到 2。

### 第 8 条

- 日志行号：`4998`
- 请求时间：`2026-08-08T19:36:43`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "ee2215e0-8bf7-41c0-8cf0-15db4feb3b09",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T19:36:43.000",
      "reportedAt": "2026-08-08T19:36:43.639",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "9c7a324f32164a118c6744c8b717c801",
      "attemptId": "8bf65316-929d-554e-855b-cc549125d17b",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "综合需求建议",
      "outcome": "success",
      "terminalReason": "completed",
      "unitKind": "merge",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 38049,
        "outputTokens": 8340,
        "cacheReadTokens": 431296,
        "totalTokens": 477685
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 38049,
          "outputTokens": 8340,
          "cacheReadTokens": 431296,
          "totalTokens": 477685,
          "acpSessionElapsedMs": 199000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T19:33:11.000",
        "endedAt": "2026-08-08T19:36:43.000",
        "acpSessionElapsedMs": 199000
      }
    }
  ]
}
```

#### 上报逻辑

acceptance 节点启动（验收需求建议），`nodeId=a30d6e88...`，生成 `unit-attempt` started，不携带 usage/model。

### 第 9 条

- 日志行号：`5009`
- 请求时间：`2026-08-08T19:36:44`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "5f7f4f95-95bf-4aa9-9b32-ca04463cea66",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-08T19:36:44.000",
      "reportedAt": "2026-08-08T19:36:44.923",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "a30d6e88b856466c87f4799491d54526",
      "attemptId": "bceadcfc-7ee8-5426-930f-c0dca9c7f832",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "验收需求建议",
      "unitKind": "acceptance",
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

acceptance 执行完成后，生成 `execution.completed`，补上 model=`glm-5.2`、usage total=378738、elapsed=144000ms，revision 升到 2。

### 第 10 条

- 日志行号：`5020`
- 请求时间：`2026-08-08T19:39:21`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "72b1eb31-67bd-4edb-8f97-40079b71ab4b",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T19:39:20.000",
      "reportedAt": "2026-08-08T19:39:21.177",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "a30d6e88b856466c87f4799491d54526",
      "attemptId": "bceadcfc-7ee8-5426-930f-c0dca9c7f832",
      "attemptIndex": 1,
      "roundIndex": 1,
      "roleName": "验收需求建议",
      "outcome": "success",
      "terminalReason": "completed",
      "unitKind": "acceptance",
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 43725,
        "outputTokens": 7205,
        "cacheReadTokens": 327808,
        "totalTokens": 378738
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 43725,
          "outputTokens": 7205,
          "cacheReadTokens": 327808,
          "totalTokens": 378738,
          "acpSessionElapsedMs": 144000
        }
      ],
      "timing": {
        "startedAt": "2026-08-08T19:36:44.000",
        "endedAt": "2026-08-08T19:39:20.000",
        "acpSessionElapsedMs": 144000
      }
    }
  ]
}
```

#### 上报逻辑

acceptance 通过后产生 `acceptance.completed`（revision 升到 3，`passed=true, firstPass=true`），与 outer-run 的 `execution.completed` 在同一个 batch 中上报。outer-run completed 携带终态和 counters（全部为 0），revision 为 2，表示整个 AUTO 交付成功结束。

### 第 11 条（批量 2 个事件）

- 日志行号：`5033`
- 请求时间：`2026-08-08T19:39:24`
- 包含事件：`acceptance.completed` / `unit-attempt` / node `a30d6e88`；`execution.completed` / `outer-run` / node `-`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "beb95128-64e8-43f2-b554-1d6cf54b95e5",
      "eventRevision": 3,
      "eventType": "acceptance.completed",
      "occurredAt": "2026-08-08T19:39:20.000",
      "reportedAt": "2026-08-08T19:39:21.178",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "unit-attempt",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "nodeId": "a30d6e88b856466c87f4799491d54526",
      "attemptId": "bceadcfc-7ee8-5426-930f-c0dca9c7f832",
      "attemptIndex": 1,
      "unitKind": "acceptance",
      "passed": true,
      "acceptanceAttempt": 1,
      "firstPass": true
    },
    {
      "eventId": "b81a55fd-3ff3-430c-b006-b80996936f4f",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-08T19:39:22.000",
      "reportedAt": "2026-08-08T19:39:22.090",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "auto",
      "executionKind": "outer-run",
      "executionId": "60f8081e93ae4414a41887801818f4bd",
      "outcome": "success",
      "terminalReason": "completed",
      "counters": {
        "pauseCount": 0,
        "resumeCount": 0,
        "permissionRequestCount": 0,
        "elicitationCount": 0,
        "manualContinueCount": 0,
        "followUpCount": 0
      }
    }
  ]
}
```

#### 上报逻辑

**事件 1：** undefined

**事件 2：** undefined

