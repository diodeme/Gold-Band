# Direct 会话指标实际上报样例（第一轮验证）

> 本文档记录 taskTitle 字段加入后的一次 Direct 会话上报数据。
> 本次会话验证了 Direct 模式下 executionId = taskId = attemptId 的稳定性、同一 task 多轮输入在同一 attempt 内累加 usage/counters、taskTitle 在所有事件中携带。

## 1. 会话信息

| 项 | 值 |
|---|---|
| 项目 | `D:\IdeaProjects\mall` |
| 会话模式 | `sessionMode=direct` |
| executionId（= taskId = attemptId） | `f660d49b95a940219d150fab9f01ef08` |
| taskTitle | `讲个笑话` |
| clientVersion | `0.9.0` |
| 会话窗口 | `2026-08-09T14:12:42` ~ `2026-08-09T14:14:34` |
| 用户输入轮次 | 2 轮（首轮 + 一次追问） |
| 数据源 | `C:\Users\kelvinzhou\AppData\Local\maling\metrics.log` |
| 日志位置 | 第 `5687` 行至第 `5720` 行（去重后唯一事件） |

本次会话共 `4` 条唯一事件，分布在 `4` 个上报批次中（已按 `eventId` 去重，忽略服务端 404 错误导致的重试）。

**本轮验证要点：**
- `executionId`、`attemptId` 等于 `taskId`，全程不变；`attemptIndex` 固定为 1。
- 首轮 started 事件不携带 `model`（ACP session 尚未启动，真实模型未知）；completed 事件从 ACP session 解析实际模型名 `glm-5.2`。
- 同一 task 第二轮用户输入不创建新 attempt，`followUpCount` 从 0 变为 1。
- usage 与 timing 在同一 attempt 内累计：`totalTokens` 从 48459 增长到 97173，`acpSessionElapsedMs` 从 11000 增长到 29000。
- `occurredAt`/`reportedAt` 不带时区偏移量。
- `taskTitle` 在事件 1、2 中携带。

## 2. 原始 JSON 与逐条上报逻辑

### 第 1 条：首轮 started

- 日志行号：`5687`
- 请求时间：`2026-08-09T14:12:42`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "cf9656e4-11df-468c-8af9-252369002ecd",
      "eventRevision": 1,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T14:12:42.000",
      "reportedAt": "2026-08-09T14:12:42.990",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "direct",
      "executionKind": "turn",
      "executionId": "f660d49b95a940219d150fab9f01ef08",
      "taskTitle": "讲个笑话",
      "attemptId": "f660d49b95a940219d150fab9f01ef08",
      "attemptIndex": 1,
      "provider": "claude-acp"
    }
  ]
}
```

#### 上报逻辑

用户在 Direct 会话中首次输入消息，runtime 激活一个新的 turn。`emit_derived_node_metrics_fact` 构建 turn started 事件：`executionId` 取 `taskId`，`attemptId` 同样取 `taskId`（Direct 的 attemptId 固定等于 executionId），`attemptIndex` 固定为 1。`taskTitle` 从 `task_show(&task_id).title` 快照获取。started 事件不携带 `model`，因为 ACP session 尚未启动，真实模型名未知。

### 第 2 条：首轮 completed

- 日志行号：`5698`
- 请求时间：`2026-08-09T14:13:08`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "b7fd1048-04c4-49b2-8868-d360cf2ea3e7",
      "eventRevision": 2,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T14:13:08.000",
      "reportedAt": "2026-08-09T14:13:08.323",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "direct",
      "executionKind": "turn",
      "executionId": "f660d49b95a940219d150fab9f01ef08",
      "taskTitle": "讲个笑话",
      "attemptId": "f660d49b95a940219d150fab9f01ef08",
      "attemptIndex": 1,
      "outcome": "completed",
      "terminalReason": "completed",
      "counters": {
        "pauseCount": 0,
        "resumeCount": 0,
        "permissionRequestCount": 0,
        "elicitationCount": 0,
        "manualContinueCount": 0,
        "followUpCount": 0
      },
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 46399,
        "outputTokens": 204,
        "cacheReadTokens": 1856,
        "totalTokens": 48459
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 46399,
          "outputTokens": 204,
          "cacheReadTokens": 1856,
          "totalTokens": 48459,
          "acpSessionElapsedMs": 11000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T14:12:42.000",
        "endedAt": "2026-08-09T14:13:08.000",
        "acpSessionElapsedMs": 11000
      }
    }
  ]
}
```

#### 上报逻辑

首轮 ACP prompt 执行完成，runtime 发出 turn completed 事件。`model` 从 `acp.session.json` 解析为实际模型名 `glm-5.2`（不是 `opus` 等占位值）。`counters.followUpCount=0` 表示这是首轮，尚未有追问。`usage` 是本轮的 token 消耗快照，`modelUsages` 按实际 provider+model 分组展示明细。`timing.acpSessionElapsedMs=11000` 从 ACP session 的 timing 段获取，不再是 null。eventRevision 从 1 递增到 2。

### 第 3 条：追问 started（状态恢复）

- 日志行号：`5709`
- 请求时间：`2026-08-09T14:14:16`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "e0a4367d-2f1f-4f2d-99ed-3666ae1a646f",
      "eventRevision": 3,
      "eventType": "execution.started",
      "occurredAt": "2026-08-09T14:14:16.000",
      "reportedAt": "2026-08-09T14:14:16.321",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "direct",
      "executionKind": "turn",
      "executionId": "f660d49b95a940219d150fab9f01ef08",
      "attemptId": "f660d49b95a940219d150fab9f01ef08",
      "attemptIndex": 1,
      "provider": "claude-acp",
      "model": "glm-5.2",
      "collectionStateRecovered": true
    }
  ]
}
```

#### 上报逻辑

用户在同一 task 下第二次输入消息。由于上一轮 turn 已结束、生命周期状态已落盘，runtime 通过快照恢复机制重建 attempt 上下文，因此携带 `collectionStateRecovered: true`。`executionId`/`attemptId`/`attemptIndex` 保持不变（不创建新 attempt）。此事件携带 `model`（`glm-5.2`），因为恢复路径中已解析了上一轮的 provider+model。

> **注意（待修复）：** 此事件未携带 `taskTitle`。恢复路径 (`collectionStateRecovered`) 在重建 fact 时跳过了 `task_title` 赋值。同理，下面的第 4 条 completed 事件也缺失 `taskTitle`。

### 第 4 条：追问 completed

- 日志行号：`5720`
- 请求时间：`2026-08-09T14:14:34`

#### 原始 JSON

```json
{
  "events": [
    {
      "eventId": "850dd27c-3bff-4900-8ee6-e5e19520724f",
      "eventRevision": 4,
      "eventType": "execution.completed",
      "occurredAt": "2026-08-09T14:14:34.000",
      "reportedAt": "2026-08-09T14:14:34.939",
      "userId": "kelvinzhou",
      "workspace": "D:\\IdeaProjects\\mall",
      "clientVersion": "0.9.0",
      "sessionMode": "direct",
      "executionKind": "turn",
      "executionId": "f660d49b95a940219d150fab9f01ef08",
      "attemptId": "f660d49b95a940219d150fab9f01ef08",
      "attemptIndex": 1,
      "outcome": "completed",
      "terminalReason": "completed",
      "counters": {
        "pauseCount": 0,
        "resumeCount": 0,
        "permissionRequestCount": 0,
        "elicitationCount": 0,
        "manualContinueCount": 0,
        "followUpCount": 1
      },
      "provider": "claude-acp",
      "model": "glm-5.2",
      "usage": {
        "inputTokens": 46670,
        "outputTokens": 455,
        "cacheReadTokens": 50048,
        "totalTokens": 97173
      },
      "modelUsages": [
        {
          "provider": "claude-acp",
          "model": "glm-5.2",
          "inputTokens": 46670,
          "outputTokens": 455,
          "cacheReadTokens": 50048,
          "totalTokens": 97173,
          "acpSessionElapsedMs": 29000
        }
      ],
      "timing": {
        "startedAt": "2026-08-09T14:12:42.000",
        "endedAt": "2026-08-09T14:14:34.000",
        "acpSessionElapsedMs": 29000
      },
      "collectionStateRecovered": true
    }
  ]
}
```

#### 上报逻辑

追问的 ACP prompt 执行完成。与首轮 completed 相比，关键变化体现了同一 attempt 的累计语义：

- `counters.followUpCount` 从 0 变为 1 —— 用户在同一 task 下进行了 1 次追问。
- `usage.totalTokens` 从 48459 增长到 97173 —— 累计两轮的 token 消耗快照。
- `usage.cacheReadTokens` 从 1856 增长到 50048 —— 第二轮大量命中上下文缓存。
- `timing.startedAt` 保持首轮的 `14:12:42` 不变，`timing.endedAt` 更新为当前轮的结束时间。
- `timing.acpSessionElapsedMs` 从 11000 增长到 29000 —— 两轮 ACP session 的累计净处理时间。
- eventRevision 从 2 递增到 4（3 是追问 started），严格单调。

`collectionStateRecovered: true` 标记此事件来自恢复路径。

> **注意（待修复）：** 同第 3 条，此事件未携带 `taskTitle`。

## 3. 事件时序图

```
时间轴 (Direct, taskId=f660d49b...)

14:12:42  [1] execution.started  rev=1  <- 用户首次输入 "讲个笑话"
    |         provider=claude-acp (无 model)
    |         ACP session 运行中...
14:13:08  [2] execution.completed rev=2  <- 首轮回答完成
    |         model=glm-5.2, totalTokens=48459, followUpCount=0
    |
    |     --- 用户阅读回答后输入追问 ---
    |
14:14:16  [3] execution.started  rev=3  <- 状态恢复，追问开始
    |         provider=claude-acp, model=glm-5.2
    |         collectionStateRecovered=true
    |         ACP session 运行中...
14:14:34  [4] execution.completed rev=4  <- 追问回答完成
              model=glm-5.2, totalTokens=97173, followUpCount=1
              collectionStateRecovered=true
```

## 4. 数据缺口

| 问题 | 影响 | 根因 |
|---|---|---|
| 恢复路径事件（rev=3, rev=4）缺失 `taskTitle` | 服务端无法获取任务标题 | 状态恢复路径重建 fact 时未执行 `fact.task_title = task_show(&task_id)` 赋值 |

其余字段均符合预期：executionId 稳定、attemptId 不变、attemptIndex 固定为 1、followUpCount 正确递增、usage/timing 累计、model 解析为实际模型名、acpSessionElapsedMs 不为 null、时间不带偏移量。
