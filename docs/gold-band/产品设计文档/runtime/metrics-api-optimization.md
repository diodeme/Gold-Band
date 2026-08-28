# 服务端指标接口变更说明

> 目标读者：指标服务端开发、测试与发布负责人。
>
> 比较基线：`origin/main` merge base `9eb71f58d8a971273b78a8acbc592a4630ae8ce4`。
>
> 本文只描述服务端可观察的 HTTP 协议、字段语义、校验规则和相对旧接口的差异，不描述请求方内部实现。完整服务端处理合同见 [会话指标批量上报服务端实施合同](./metrics-server-processing.md)。

## 1. 改动结论

`POST /api/client-report/metrics/batch` 的地址、鉴权方式和批量请求信封保持不变，但事件 DTO 和成功响应均发生破坏式变化。服务端不能只在旧 DTO 上增加若干 nullable 字段，必须同时更新请求反序列化、条件校验、幂等语义和逐事件响应。

核心变化如下：

1. 请求新增 `projectId`、`runId`、`roundId`、`taskOrigin`、`executionTrigger` 和 `codeChanges`。
2. 请求删除 `collectionStateRecovered`。
3. `taskTitle` 保持可选，并明确允许上传；它是展示字段，不参与身份和幂等判断。
4. `executionId` 明确表示 Task UUID，`eventRevision` 明确表示同一 Task 事件流中的递增版本。
5. HTTP 200 必须逐 `eventId` 返回 `accepted`、`duplicate` 或 `rejected`，不再以 HTTP 成功代表整批全部成功。
6. `execution.paused`、`execution.resumed`、`intervention.requested` 收敛为节点事件，只允许 `node-attempt` 或 `unit-attempt`。
7. counters 明确分为 Node attempt 快照和 Task 累计快照，服务端不得跨作用域累计或覆盖。
8. 新接口不兼容缺少新增必填字段的旧请求，也不要求双协议解析或双写。

## 2. 与原接口的差异

### 2.1 HTTP 外层

| 项目 | `origin/main` | 当前分支 | 服务端改动 |
|---|---|---|---|
| Method | `POST` | 不变 | 无 |
| Path | `/api/client-report/metrics/batch` | 不变 | 无 |
| 鉴权 Header | `X-Maling-Report-Key` | 不变 | 无 |
| Content-Type | `application/json;charset=UTF-8` | 不变 | 无 |
| 请求信封 | `{ "events": [...] }` | 不变 | 无 |
| Batch 数量 | 最多 100 条 | `1..100` 条 | 拒绝空数组和超过 100 条的请求 |
| 单事件大小 | 未形成独立约束 | 最大 64 KiB UTF-8 JSON | 超限事件按事件级错误拒绝 |
| Batch 月份 | 未形成强制接口约束 | 同一 batch 的 `reportedAt` 必须属于同一 Asia/Shanghai 自然月 | 跨月请求作为请求级错误返回 |
| 成功响应 | 未定义逐事件处理结果 | 必须逐事件返回三类 disposition | 按第 8 节返回完整响应 |

### 2.2 字段增删

| 字段 | 变化 | 新接口要求 |
|---|---|---|
| `projectId` | 新增 | 必填；Project 的稳定标识 |
| `runId` | 新增 | 必填；当前 Run 标识，仅在 Project/Task 范围内解释 |
| `roundId` | 新增 | 必填；当前 Round 标识，仅在 Project/Task/Run 范围内解释 |
| `taskOrigin` | 新增 | 必填；只允许 `user` 或 `scheduled` |
| `executionTrigger` | 新增 | `scheduled` 时必填，`user` 时禁止 |
| `codeChanges` | 新增 | 可选；只允许出现在 delivery terminal 事件 |
| `collectionStateRecovered` | 删除 | 新接口不得继续接收或依赖 |
| `taskTitle` | 保留并明确协议 | 可选；允许上传标题，不参与 identity、唯一键或名称反查 |

### 2.3 既有字段的语义变化

| 字段/主题 | `origin/main` | 当前分支 |
|---|---|---|
| `executionId` | 缺少完整作用域约束 | 固定表示 Task UUID；同一 Task 的后续 Run、Round 和 attempt 保持不变 |
| `eventRevision` | 已有字段，但未形成完整的 Task stream 合同 | 在 `(projectId, executionId)` 内递增；允许缺口，不允许同一 revision 对应不同事件 |
| `sessionMode + executionKind` | 服务端可能按普通字符串宽松接收 | 必须按 Direct/Workflow/AUTO 主体矩阵校验 |
| paused/resumed/intervention 主体 | Workflow/AUTO 中间事件可能挂在 `run/outer-run` 并夹带节点字段 | 只允许 Workflow `node-attempt` 或 AUTO `unit-attempt`；禁止 `turn/run/outer-run` |
| 枚举字段 | 多数字段以普通 string 传输 | wire value 不变，但未知值必须拒绝，不能静默降级 |
| `counters` | Task 与节点聚合边界不明确 | node/unit terminal 携带当前节点 attempt counters；turn/run/outer-run terminal 携带整个 Task counters |
| `followUpCount` | 计数口径不稳定 | 表示去重后的后续用户输入次数；服务端按快照处理 |
| terminal | 容易被解释为 Task 事件流结束 | 只表示当前 turn/run/attempt 结束；同一 Task 后续事件继续使用更高 revision |
| Direct `codeChanges` | 无 | 每个 Direct turn terminal 都可能携带新快照；后续轮次不能复用首轮值，也不能跨轮累加 |

### 2.4 响应变化

旧接口没有要求服务端确认每个输入事件的处理结果。新接口要求 HTTP 200 响应中的三个集合互斥并精确覆盖本次请求的全部 `eventId`：

```text
acceptedEventIds ∪ duplicateEventIds ∪ rejected.eventId
= request.events.eventId
```

任何事件都不能遗漏、重复出现或出现在请求之外。详细结构见第 8 节。

## 3. 当前请求合同

### 3.1 请求信封

```json
{
  "events": [
    { "eventId": "..." }
  ]
}
```

- `events` 必须是数组，长度为 `1..100`。
- 数组中的每一项必须是第 3.2 节定义的事件对象。
- 单个事件序列化后的 UTF-8 JSON 最大为 64 KiB。
- 所有字段使用 camelCase。
- 标记为“可选”或“条件必填”的字段在不适用时应省略，不发送显式 `null`；第 5 节明确为 nullable 的嵌套数值字段除外。
- 顶层未知扩展字段不参与当前协议校验；`executionTrigger`、`usage`、`modelUsages`、`timing`、`counters` 和 `codeChanges` 必须按本文定义的 shape 严格校验。

### 3.2 顶层字段完整字典

| 字段 | 类型 | 必需性 | 服务端语义 |
|---|---|---|---|
| `eventId` | UUID string | 必填 | 事件幂等标识 |
| `eventRevision` | uint64，`>=1` | 必填 | `(projectId, executionId)` 事件流版本 |
| `eventType` | enum string | 必填 | 生命周期事件类型，见第 4 节 |
| `occurredAt` | datetime string | 必填 | 事件实际发生时间 |
| `reportedAt` | datetime string | 必填 | 事件形成上报记录的时间；用于 batch 月份校验 |
| `projectId` | non-empty string | 必填 | Project 稳定标识 |
| `userId` | non-empty string | 必填 | 事件所属用户 |
| `workspace` | non-empty string | 必填 | workspace 展示与诊断值；不得替代 `projectId` 做关联 |
| `clientVersion` | non-empty string | 必填 | 请求方版本 |
| `sessionMode` | enum string | 必填 | `direct/workflow/auto` |
| `executionKind` | enum string | 必填 | `turn/run/node-attempt/outer-run/unit-attempt`；三类中间事件只允许 `node-attempt/unit-attempt` |
| `executionId` | UUID string | 必填 | Task UUID |
| `runId` | non-empty string | 必填 | 当前 Run 标识 |
| `roundId` | non-empty string | 必填 | 当前 Round 标识 |
| `taskOrigin` | enum string | 必填 | `user/scheduled`；同一 Task 首次确定后不可变 |
| `executionTrigger` | object | 条件必填 | `scheduled` 必填、`user` 禁止，见 5.1 |
| `taskTitle` | string | 可选 | Task 标题；允许上传，缺省不表示清空已有标题 |
| `nodeId` | string | 条件必填 | Workflow node attempt、AUTO unit attempt 必填 |
| `attemptId` | UUID string | 条件必填 | Direct turn、Workflow node attempt、AUTO unit attempt 必填 |
| `attemptIndex` | uint32，`>=1` | 条件必填 | attempt 主体必填；Direct 固定为 1 |
| `roundIndex` | uint32，`>=1` | 条件必填 | Workflow/AUTO attempt 主体必填 |
| `roleName` | string | 条件必填 | Workflow/AUTO attempt 的角色展示名 |
| `unitKind` | enum string | 条件必填 | 仅 AUTO unit attempt 必填 |
| `childRunId` | string | 可选 | AUTO workflow invocation 与子 Workflow Run 的关系标识 |
| `outcome` | enum string | terminal 必填 | 只允许用于 `execution.completed` |
| `terminalReason` | enum string | terminal 必填 | 与 `outcome` 同时出现 |
| `terminalReasonCode` | string | 可选 | 更细的结构化运行时错误码 |
| `failedAttemptId` | UUID string | 可选 | delivery failure 对应的最终失败 attempt |
| `roundCount` | uint32 | 可选 | Workflow run terminal 的总 Round 数 |
| `passed` | boolean | acceptance 条件字段 | 本次 acceptance 结果 |
| `acceptanceAttempt` | uint32，`>=1` | acceptance 条件字段 | 当前 Task 第几次 acceptance |
| `firstPass` | boolean | acceptance 条件字段 | 是否首次 acceptance 即通过 |
| `interventionKind` | enum string | intervention 条件字段 | 人工介入类型 |
| `pauseReason` | enum string | paused 条件字段 | 当前暂停原因 |
| `previousPauseReason` | enum string | resumed 条件字段 | 恢复前的暂停原因 |
| `provider` | string | 可选 | 模型服务提供方 |
| `model` | string | 可选 | 模型名称 |
| `usage` | object | 可选 | 当前 turn/attempt 的总 token 快照，见 5.2 |
| `modelUsages` | object[] | 可选 | 按 provider/model 拆分的用量，见 5.3 |
| `timing` | object | 可选 | 当前 turn/attempt 的时间信息，见 5.4 |
| `counters` | object | terminal 必填 | terminal 时的完整快照；作用域由 execution kind 决定，见 5.5 |
| `codeChanges` | object | 可选 | delivery terminal 的代码变化快照，见 5.6 |

### 3.3 时间格式

`occurredAt`、`reportedAt`、`timing.startedAt` 和非 null 的 `timing.endedAt` 使用：

```text
YYYY-MM-DDTHH:mm:ss.SSS
```

这些时间不携带 `Z` 或 offset，协议时区固定为 `Asia/Shanghai`。服务端解析时必须显式按该时区解释，不能使用部署机器的默认时区。

## 4. 枚举值

| 字段 | 允许值 |
|---|---|
| `eventType` | `execution.started`、`execution.completed`、`execution.paused`、`execution.resumed`、`intervention.requested`、`acceptance.completed` |
| `sessionMode` | `direct`、`workflow`、`auto` |
| `executionKind` | `turn`、`run`、`node-attempt`、`outer-run`、`unit-attempt` |
| `taskOrigin` | `user`、`scheduled` |
| `unitKind` | `worker`、`workflow-invocation`、`merge`、`acceptance` |
| `outcome` | `completed`、`failed`、`cancelled`、`success`、`failure`、`killed` |
| `terminalReason` | `completed`、`user-cancelled`、`process-killed`、`provider-error`、`runtime-error`、`validation-error`、`execution-failed`、`retry-exhausted`、`acceptance-rejected`、`unknown` |
| `interventionKind` | `manual-decision`、`elicitation`、`permission`、`runtime-abnormal`、`error-blocked`、`process-interrupted` |
| `pauseReason`、`previousPauseReason` | `waiting-for-user-input`、`permission-requested`、`runtime-abnormal`、`error-blocked`、`process-interrupted` |
| `repeatKind` | `interval`、`hourly`、`daily`、`weekdays`、`weekly` |

未知枚举必须作为对应 event 的字段错误返回。只有请求明确传入 `terminalReason=unknown` 时，该值才合法；不能把其他未知值自动映射成 `unknown`。

## 5. 嵌套对象

### 5.1 `executionTrigger`

`taskOrigin=user` 时禁止出现。`taskOrigin=scheduled` 时必须出现，并且只能是以下三个 tagged shape 之一。

一次性任务：

```json
{
  "type": "once",
  "scheduledTaskId": "scheduled-task-001",
  "scheduledOccurrenceId": "occurrence-001",
  "scheduledAt": "2026-08-28T10:00:00.000",
  "timezone": "Asia/Shanghai"
}
```

重复任务：

```json
{
  "type": "repeat",
  "scheduledTaskId": "scheduled-task-001",
  "scheduledOccurrenceId": "occurrence-002",
  "scheduledAt": "2026-08-28T10:00:00.000",
  "timezone": "Asia/Shanghai",
  "repeatKind": "interval",
  "value": 30,
  "unit": "minutes",
  "anchorAt": "2026-08-28T09:00:00.000+08:00"
}
```

Cron 任务：

```json
{
  "type": "cron",
  "scheduledTaskId": "scheduled-task-001",
  "scheduledOccurrenceId": "occurrence-003",
  "scheduledAt": "2026-08-28T10:00:00.000",
  "timezone": "Asia/Shanghai",
  "expression": "0 0 10 * * MON-FRI"
}
```

公共字段 `scheduledTaskId/scheduledOccurrenceId/scheduledAt/timezone` 均必填。`repeatKind=interval` 必须携带 `value/unit/anchorAt`；preset repeat 使用 `hour/minute`，其中 `weekly` 还必须携带非空 `weekdays`。新接口拒绝旧 `{kind: ...}`、`triggerKind`、`sessionPolicy` 和 user trigger shape。

### 5.2 `usage`

```json
{
  "inputTokens": 1200,
  "outputTokens": 300,
  "cacheReadTokens": null,
  "totalTokens": 1500
}
```

四个键必须同时存在，值为 nullable uint64。`null` 表示未知，不能解释为 0。

### 5.3 `modelUsages[]`

| 字段 | 类型 | 必需性 |
|---|---|---|
| `provider` | non-empty string | 必填 |
| `model` | non-empty string | 必填 |
| `inputTokens` | uint64 或 null | 必填 |
| `outputTokens` | uint64 或 null | 必填 |
| `cacheReadTokens` | uint64 或 null | 必填 |
| `totalTokens` | uint64 或 null | 必填 |
| `acpSessionElapsedMs` | uint64 或 null | 必填 |

服务端按请求顺序保存，不重新拆分、合并或计算另一份用量。

### 5.4 `timing`

| 字段 | 类型 | 必需性与语义 |
|---|---|---|
| `startedAt` | datetime string | 必填 |
| `endedAt` | datetime string 或 null | 必填；未知时为 null |
| `acpSessionElapsedMs` | uint64 或 null | 必填；未知时为 null |

### 5.5 `counters`

```json
{
  "pauseCount": 1,
  "resumeCount": 1,
  "permissionRequestCount": 2,
  "elicitationCount": 0,
  "manualContinueCount": 1,
  "followUpCount": 3
}
```

六个字段均为非负 uint32，且必须同时出现。它们是 terminal 时的完整快照，服务端不得根据其他事件再次累加。

| terminal 主体 | `sessionMode/executionKind` | counters 作用域 | 服务端处理 |
|---|---|---|---|
| Workflow 节点 | `workflow/node-attempt` | 当前 `nodeId + attemptId` 对应节点 attempt | 写入该节点执行结果，不合并为其他节点 counters |
| AUTO 单元 | `auto/unit-attempt` | 当前 `nodeId + attemptId` 对应单元 attempt | 写入该单元执行结果，不合并为其他单元 counters |
| Direct delivery | `direct/turn` | 当前 Task 从创建到本次 turn terminal 的累计值 | 以更高 revision 的 Task 快照更新，不与上一轮相加 |
| Workflow delivery | `workflow/run` | 当前 Task 从创建到本次 run terminal 的累计值 | 以更高 revision 的 Task 快照更新，不从 node counters 求和 |
| AUTO delivery | `auto/outer-run` | 当前 Task 从创建到本次 outer-run terminal 的累计值 | 以更高 revision 的 Task 快照更新，不从 unit counters 求和 |

`execution.started`、`execution.paused`、`execution.resumed`、`intervention.requested` 和 `acceptance.completed` 禁止携带 counters。节点 terminal 与 Task delivery terminal 是两个独立统计域；Task counters 不是服务端对节点 counters 的求和结果。

### 5.6 `codeChanges`

```json
{
  "addedLines": 128,
  "deletedLines": 37,
  "changedFiles": 9
}
```

三个字段均为非负 uint64，且必须同时出现。对象只允许出现在以下 delivery terminal：

- Direct：`sessionMode=direct`、`executionKind=turn`、`eventType=execution.completed`。
- Workflow：`sessionMode=workflow`、`executionKind=run`、`eventType=execution.completed`。
- AUTO：`sessionMode=auto`、`executionKind=outer-run`、`eventType=execution.completed`。

该对象表示对应 delivery 截止当前 terminal 的代码净变化快照，不包含路径、源码、diff 或 Git 标识。服务端按更高合法 revision 更新快照，不跨 turn/run 累加。Direct 后续轮次传入的是新快照，不能固定保留首轮值。

## 6. 条件字段校验

### 6.1 会话主体矩阵

| `sessionMode` | `executionKind` | 必填主体字段 | 禁止字段 |
|---|---|---|---|
| `direct` | `turn` | `attemptId, attemptIndex` | `nodeId, roundIndex, roleName, unitKind` |
| `workflow` | `run` | 无 | `nodeId, attemptId, attemptIndex, roundIndex, roleName, unitKind` |
| `workflow` | `node-attempt` | `nodeId, attemptId, attemptIndex, roundIndex, roleName` | `unitKind` |
| `auto` | `outer-run` | 无 | `nodeId, attemptId, attemptIndex, roundIndex, roleName, unitKind` |
| `auto` | `unit-attempt` | `nodeId, attemptId, attemptIndex, roundIndex, roleName, unitKind` | 无 |

`execution.paused`、`execution.resumed` 和 `intervention.requested` 是节点级事件，只允许以下两种主体：

- Workflow：`sessionMode=workflow` 且 `executionKind=node-attempt`。
- AUTO：`sessionMode=auto` 且 `executionKind=unit-attempt`。

这三类事件禁止使用 Direct `turn`、Workflow `run` 或 AUTO `outer-run`。即使请求同时携带 `nodeId/attemptId`，也不能用 delivery execution kind 代替节点 execution kind。

### 6.2 事件字段矩阵

| `eventType` | 必填字段 | 禁止/限制 |
|---|---|---|
| `execution.started` | 公共字段 | 禁止 `outcome/terminalReason/counters/codeChanges` |
| `execution.completed` | `outcome/terminalReason/counters` | `codeChanges` 仅 delivery subject 可用 |
| `execution.paused` | `pauseReason` | 仅 `workflow/node-attempt` 或 `auto/unit-attempt`；禁止 counters |
| `execution.resumed` | `previousPauseReason` | 仅 `workflow/node-attempt` 或 `auto/unit-attempt`；禁止 counters |
| `intervention.requested` | `interventionKind` | 仅 `workflow/node-attempt` 或 `auto/unit-attempt`；禁止 counters |
| `acceptance.completed` | `passed/acceptanceAttempt/firstPass` | 仅 acceptance 语义的事件 |

`outcome` 和 `terminalReason` 必须同时出现或同时省略，并且只允许出现在 `execution.completed`。

### 6.3 身份、幂等与顺序

- `eventId` 相同且 payload 相同：返回 duplicate。
- `eventId` 相同但 payload 不同：返回事件冲突。
- `(projectId, executionId, eventRevision)` 相同但 `eventId` 不同：返回 revision 冲突。
- 首次收到的较旧 revision 可以作为历史事件接受，但不得让对外查询的最新状态倒退。
- `taskOrigin` 在同一 `(projectId, executionId)` 内不可变。
- `taskTitle` 不是 identity；同名 Task 不得合并，标题缺省也不得清空已有值。
- terminal 不是 Task stream 结束标记；terminal 之后的更高 revision 仍是合法输入。

## 7. 完整请求示例

以下示例为 Direct turn terminal：

```json
{
  "events": [{
    "eventId": "83be2445-c85d-4320-8e63-75af2231848f",
    "eventRevision": 8,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-28T14:20:01.123",
    "reportedAt": "2026-08-28T14:20:01.456",
    "projectId": "project-canonical-id",
    "userId": "kevin",
    "workspace": "D:\\IdeaProjects\\mall",
    "clientVersion": "0.14.1",
    "sessionMode": "direct",
    "executionKind": "turn",
    "executionId": "6767b1c7-8c84-4092-9bd8-ddb53a982df1",
    "runId": "run-001",
    "roundId": "round-001",
    "taskOrigin": "user",
    "taskTitle": "实现订单查询",
    "attemptId": "6767b1c7-8c84-4092-9bd8-ddb53a982df1",
    "attemptIndex": 1,
    "outcome": "completed",
    "terminalReason": "completed",
    "provider": "codex-acp",
    "model": "gpt-5.6-sol",
    "usage": {
      "inputTokens": 1200,
      "outputTokens": 300,
      "cacheReadTokens": null,
      "totalTokens": 1500
    },
    "timing": {
      "startedAt": "2026-08-28T14:19:40.000",
      "endedAt": "2026-08-28T14:20:01.123",
      "acpSessionElapsedMs": 21123
    },
    "counters": {
      "pauseCount": 0,
      "resumeCount": 0,
      "permissionRequestCount": 0,
      "elicitationCount": 0,
      "manualContinueCount": 0,
      "followUpCount": 2
    },
    "codeChanges": {
      "addedLines": 18,
      "deletedLines": 4,
      "changedFiles": 3
    }
  }]
}
```

## 8. 响应合同

### 8.1 成功响应

请求信封可处理时返回 HTTP 200，并在 `data` 中逐事件给出结果：

```json
{
  "data": {
    "acceptedEventIds": ["event-a"],
    "duplicateEventIds": ["event-b"],
    "rejected": [{
      "eventId": "event-c",
      "error": {
        "code": "METRICS_SUBJECT_INVALID",
        "params": {
          "field": "executionKind"
        }
      }
    }]
  }
}
```

| 字段 | 类型 | 要求 |
|---|---|---|
| `data.acceptedEventIds` | string[] | 新接受并完成处理的 eventId |
| `data.duplicateEventIds` | string[] | 已存在且 payload 一致的 eventId |
| `data.rejected` | object[] | 事件级拒绝结果 |
| `rejected[].eventId` | string | 对应请求中的 eventId |
| `rejected[].error.code` | non-empty string | 稳定错误码 |
| `rejected[].error.params` | object | 可选的结构化诊断参数，不包含对客文案 |

三个结果字段即使为空也应返回空数组。服务端标准响应信封可以包含 `code/msg/ok` 等其他顶层字段，但 `data` 及上述三个数组必须存在。

新服务端统一输出结构化 `error.code + error.params`，不再输出旧的扁平 `errorCode`。

### 8.2 请求级错误与事件级错误

以下情况使用请求级非 2xx 响应：

- 鉴权失败。
- JSON 或请求信封无法解析。
- `events` 为空或超过 100 条。
- 同一 batch 的 `reportedAt` 跨 Asia/Shanghai 自然月。

单个 event 的字段错误、枚举错误、主体矩阵错误、幂等冲突或 revision 冲突必须放入 `rejected[]`。同一 batch 中其他合法事件仍分别进入 `acceptedEventIds` 或 `duplicateEventIds`。

## 9. 错误码

| `error.code` | 适用条件 | 可重试 |
|---|---|---|
| `METRICS_EVENT_INVALID` | 字段缺失、格式非法、未知枚举、单事件超过 64 KiB | 否 |
| `METRICS_SUBJECT_INVALID` | `sessionMode/executionKind` 或主体字段矩阵不匹配 | 否 |
| `METRICS_TERMINAL_FIELDS_INVALID` | terminal、counters、codeChanges 的出现范围错误 | 否 |
| `METRICS_SCHEDULED_PROVENANCE_INVALID` | `taskOrigin/executionTrigger` 缺失、shape 或 repeat 字段组合非法 | 否 |
| `METRICS_EVENT_ID_CONFLICT` | 相同 `eventId` 对应不同 payload | 否 |
| `METRICS_REVISION_CONFLICT` | 相同 Task revision 对应不同 `eventId` | 否 |
| `METRICS_IMMUTABLE_FIELD_CONFLICT` | 同一 Task 的 `taskOrigin` 或其他稳定来源字段发生变化 | 否 |
| `METRICS_STORAGE_UNAVAILABLE` | 当前事件未能完成持久化处理 | 是 |

`error.params` 只包含定位问题所需的 identity、revision 或字段名，不得包含 workspace、标题、模型输出、源码或完整 payload。服务端只返回稳定错误码，不在接口中生成对客错误文案。

## 10. 服务端改造与验收清单

- [ ] 请求 DTO 已新增 `projectId/runId/roundId/taskOrigin/executionTrigger/codeChanges`。
- [ ] 请求 DTO 已删除 `collectionStateRecovered`。
- [ ] `taskTitle` 可选且允许上传，不参与 identity、唯一约束或名称反查。
- [ ] 所有顶层字段、嵌套对象、枚举和 nullable 规则均有接口测试。
- [ ] Direct、Workflow、AUTO 的 mode/kind/subject 矩阵均有接口测试。
- [ ] paused/resumed/intervention 仅接受 Workflow `node-attempt` 和 AUTO `unit-attempt`；`turn/run/outer-run` 逐项拒绝。
- [ ] `user/scheduled` 及 `once/repeat/cron` 组合均有接口测试。
- [ ] node/unit terminal 返回当前节点 attempt counters；turn/run/outer-run terminal 返回整个 Task counters，且 Task counters 不从节点 counters 求和。
- [ ] terminal、codeChanges、acceptance、usage 和 timing 的作用域均有接口测试。
- [ ] 相同 eventId 的 duplicate 与 payload conflict 可稳定区分。
- [ ] `(projectId, executionId, eventRevision)` 冲突可稳定识别。
- [ ] HTTP 200 中三个 disposition 集合互斥并精确覆盖全部请求 eventId。
- [ ] 单事件错误不影响同 batch 中其他合法事件。
- [ ] Direct 后续轮次的 terminal 可以用更高 revision 更新 `codeChanges`，且不会固定为首轮值或跨轮累加。
- [ ] 新接口拒绝旧 DTO，不保留旧字段 fallback 或双协议解析。

## 11. 设计与性能评审

过度设计评审：本文只定义一个明确版本的 HTTP 合同和迁移差异，不引入兼容层、额外 API 版本、队列、缓存或服务端存储实现约束。具体持久化结构由服务端在满足接口幂等与顺序语义的前提下自行决定。

性能影响评审：本次仅新增接口文档，不改变运行时代码。接口继续限制单 batch 最多 100 条；服务端实现应避免逐事件 N+1 查询和长事务，并在上线前用 100 条混合结果 batch 验证响应时间、锁等待与回滚率。
