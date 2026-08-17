# Gold Band 统一指标上报与价值评估方案 V2

> 本方案只复用现有两个接口：`POST /api/client-report/heartbeat` 与 `POST /api/client-report/metrics/batch`。不新增第三个上报端点。允许对请求体和服务端处理做开发阶段破坏式升级，不保留 V1 双写、兼容字段或 fallback。
>
> 心跳的客户端与服务端落地细节见：[心跳上报开发方案](../../开发计划/数据采集/心跳上报开发方案.md)。
>
> 批量指标的客户端可靠投递与服务端物化细节见：[批量指标上报开发方案](../../开发计划/数据采集/批量指标上报开发方案.md)。

## 0. 结论

V2 将两个接口重新定义为：

1. `heartbeat`：上报用户启动、真实界面活动、三种模式顶层 run 启动和定时任务创建事实，用于活跃/留存及事件分类统计。
2. `metrics/batch`：批量上报可持久化、可重放、可去重的执行领域事件。

三种处理模式使用同一事件信封，但统计主单位不同：

| 处理模式 | 用户心智 | 主统计单位 | 内部执行容器 | 不能错误解释为 |
|---|---|---|---|---|
| Direct | 与一个 Agent 持续对话 | ACP turn | 首轮复用单 Worker run，后续复用同一 ACP session | workflow run 成功 |
| Workflow | 执行明确工作流 | runtime run | run / round / node / attempt | 用户最终验收成功 |
| AUTO | AI 动态编排并执行 | outer runtime run | outer run + AI-DYNAMIC graph + dynamic node + child workflow | 普通单节点 workflow |

指标必须准确命名：

- Direct：回复完成率、回复失败率、取消率、单轮耗时和单轮 token。
- Workflow/AUTO：run 终局成功率、未达终局率、可靠性、自动化程度和成功 run token。
- “用户任务成功率”只有引入明确用户验收事实后才能成立，不能用 `RunCompleted(success)` 冒充。
- 仅使用 token 时只能称“token 效率”；跨模型经济性比较需要价格版本或真实账单数据。

---

## 1. 领域模型与设计原则

### 1.1 先定领域，再定接口

```text
userId
└─ app_session
   └─ workspace
      └─ task / conversation
         ├─ Direct: turn
         ├─ Workflow: run → round → node → attempt
         └─ AUTO: outer run
                    └─ AI-DYNAMIC outer attempt
                       └─ dynamic run
                          ├─ dynamic node attempt
                          └─ workflow invocation → child run
```

生命周期相关状态由所属领域统一管理：

- run 状态只由 runtime run 生命周期维护。
- turn 状态只由 ACP prompt turn 生命周期维护。
- Direct 后续追问不能重新打开或改写已经结束的 run。
- Workflow/AUTO 完成后的手动追问属于 turn，不属于新 run。
- AUTO dynamic leaf 状态属于 dynamic graph；outer run 只保存聚合结果。

### 1.2 客户端上报事实，服务端计算指标

客户端只负责上报谁在什么时候开始、暂停、恢复、结束，处理模式、实际 provider/model、本执行单元 token 增量、人工干预和 runtime 故障。成功率、留存率、全自动率和 token 效率由服务端事实表离线计算。

### 1.3 `started` 是分母，`finished` 是终局

所有完成率或成功率以显式 `execution.started` 为全集，不能从 `node.finished` 反推 run 是否存在。这样才能覆盖首节点完成前崩溃、用户关闭应用或 provider 永久无响应。

### 1.4 统一 canonical ID

所有上报与服务端 JOIN 使用不可复用的 canonical ID：

- `taskId = TaskState.uuid`
- `runId = RunState.uuid`
- `roundId = RoundState.uuid`
- `nodeId = NodeState.uuid / DynamicNodeState.uuid`
- `attemptId = 新增并持久化的 attempt UUID`
- `turnId = 后端接受 prompt 前生成或确认的稳定 UUID`

`task-001`、`run-001`、DSL node id 等可复用目录/展示编号不能作为跨表主键。分析工作流结构时使用单独的 `nodeKey`、`workflowId`。

### 1.5 usage 永远上报增量

同一 ACP session 会连续处理多个 Direct turn，也可能被 runtime continue/repair 复用。V2 禁止上报 ACP session 从创建至今的累计 token：

1. 执行单元开始时记录 usage counter 快照。
2. 结束时读取 counter。
3. 上报 `end - start` 的非负增量。
4. 每份 usage 携带唯一 `usageScopeId`。
5. 服务端按 `usageScopeId` 只计一次。

### 1.6 内容最小化

heartbeat 只上传规范化 `userId`，不上传 workspace。metrics/batch 继续按执行事件需要携带 `userId` 与 `workspace`。除此之外，禁止上传 task title、prompt、Agent 回复、dynamic node task/title、工具参数/输出、附件路径/内容、profile 正文和 system prompt。

heartbeat 是 WB 渠道专属能力。渠道门禁由桌面端 Rust 配置层统一执行；
default 与其他渠道不构造或发送 heartbeat，也不保留待执行重试。

---

## 2. 接口一：`POST /api/client-report/heartbeat`

> 完整落地细节、客户端状态机和测试清单见：[心跳上报开发方案](../../开发计划/数据采集/心跳上报开发方案.md)。

### 2.1 统计目标

heartbeat 只服务：

- DAU、WAU、MAU 与 DAU/MAU；
- 周活跃回访率；
- 首次观测用户次周留存；
- 人均应用启动次数；
- Direct、Workflow、AUTO 顶层 run 启动次数；
- 定时任务 durable 创建次数；
- 客户端当天最后版本与 OS 的活跃用户分布。

不统计账号、设备、workspace、在线时长、操作强度或执行状态。Direct、Workflow、AUTO 的执行事实由 metrics/batch 负责。

### 2.2 请求体

```json
{
  "heartbeatId": "1f16eb74-4215-4c74-a2f3-2930f5df3c87",
  "userId": "kelvinzhou",
  "reason": "appStarted",
  "clientVersion": "0.1.0",
  "os": "windows"
}
```

| 字段 | 约束 | 作用 |
|---|---|---|
| `heartbeatId` | UUID；每个逻辑 heartbeat 一次 | 服务端幂等；同一逻辑事件的有限重试复用 |
| `userId` | trim 后 1～128 字符并规范化为小写 | WB 模式活跃与留存主体 |
| `reason` | `appStarted/activity/directStarted/workflowStarted/autoStarted/scheduledTaskCreated` | 应用启动、活动、三种用户顶层 run 与定时任务创建 |
| `clientVersion` | 1～64 字符 | 当天最后版本分布 |
| `os` | `windows/macos/linux` | 当天最后 OS 分布 |

明确删除 workspace、appSessionId、reportedAt、foregrounded、interval，以及 schemaVersion、accountId、installationId、workspaceId、heartbeatSeq、appState、activityState、activeExecutionCount。

### 2.3 客户端时机

#### appStarted

metrics 配置、endpoint 和 API Key 就绪后立即发送，不等待 workspace。网络错误、超时、429、5xx 或不可解析的 2xx 响应在 30 秒、2 分钟后有限重试，三次尝试复用同一 heartbeatId 和请求体。400、401、413 不自动重试；配置有效变化后可重新尝试，metrics 关闭则取消待执行重试。

重试任务通过配置 generation 失效，唤醒后使用当前 endpoint/API Key；配置变化
不改变 appStarted 的 heartbeatId 和五字段请求体。只有成功解析 `code=200` 且
`accepted=true` 或 `duplicate=true` 的 2xx envelope 才视为交付。

#### activity

窗口回前台、pointerdown、keydown，以及 Direct/Workflow/AUTO 的启动、重跑、继续等业务命令统一触发 activity。鼠标移动、滚动、动画、repaint 和纯后台执行不触发。

appStarted 或 activity 成功后 15 分钟内不发送 activity；activity 失败后退避 1 分钟，必须等待下一次真实操作，不定时补发。已有 appStarted/activity 请求 in-flight 时合并 activity，不排队、不写 durable outbox；但 appStarted 是必须保留的进程启动事实，若 activity 先占用共享发送槽，客户端先登记 appStarted 请求，并在 activity 完成释放槽位后立即续发。共享槽统一通过 acquire/release 转换管理，每次 acquire 生成绑定配置 generation 与请求 generation 的 lease；完成回调只能释放自己持有的 lease。appStarted 延迟重试唤醒后也必须先 acquire，槽位被 activity 占用时保持待发送事实，等待其所有者释放后续发，禁止并发绕过共享槽。四类业务事实的结果不改变这些时间戳。

#### directStarted / workflowStarted / autoStarted

`Started` 统一表示用户发起的顶层 Conversation 模式运行已被 Gold Band 接受并创建新的 canonical `runId`。当前 Conversation 新建和 rerun 都计一次；Direct follow-up/queued prompt、continue、same-run retry、旧工作台 `start_run`、定时后台 execution 与 AUTO dynamic/child run 不计。

#### scheduledTaskCreated

定时任务定义和输入快照完成 durable 事务后计一次，发生在 coordinator `JobCreated` 通知之前。编辑、启停、删除和定时执行不计；coordinator 通知失败不撤销已经形成的 durable 创建事实。

六类信号统一经现有 `RuntimeLifecycleBus` 发布，由异步 metrics subscriber 映射 reason 和调用 reporter。producer 不读取 metrics 配置、不等待 HTTP、不接收上报结果；subscriber failure 不改变任务 command 结果或 canonical state。四类业务事实不参与 activity 节流/in-flight，暂时故障执行固定次数进程内重试并复用同一 heartbeatId；配置 generation 变化后取消旧重试，不建设 durable outbox。

userId 使用 `whoami::username()` 读取各系统用户名，trim 并小写化；失败、空值或 unknown 时跳过，禁止环境变量 fallback。heartbeat 不维护三种模式的 active execution，终局和执行明细全部由 metrics/batch 负责。

### 2.4 控制台处理

1. 限制 body 8 KiB，并校验 `X-Maling-Report-Key`。
2. DTO 拒绝未知字段，校验 UUID、userId、reason、version 和 os。
3. 服务端再次规范化 userId。
4. 生成 UTC receivedAt，转换到 Asia/Shanghai 得到 statDate。
5. 在事务中插入原始事件；heartbeatId 重复时返回 duplicate，不更新聚合。
6. 新事件按 `(statDate,userId)` upsert `heartbeat_daily`；appStarted 使 appStartCount 加 1。
7. latest version/OS 只允许 receivedAt 较新的事件覆盖。
8. 提交并返回 accepted。

### 2.5 数据库

```sql
CREATE TABLE client_heartbeat_event (
    heartbeat_id CHAR(36) NOT NULL,
    user_id VARCHAR(128) COLLATE utf8mb4_0900_as_ci NOT NULL,
    reason VARCHAR(32) NOT NULL,
    client_version VARCHAR(64) NOT NULL,
    os VARCHAR(16) NOT NULL,
    received_at DATETIME(3) NOT NULL COMMENT 'UTC',
    stat_date DATE NOT NULL COMMENT 'Asia/Shanghai',
    PRIMARY KEY (heartbeat_id),
    KEY idx_heartbeat_user_date (user_id, stat_date),
    KEY idx_heartbeat_received_at (received_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_ci;
```

原始表默认保留 90 天，用于幂等、排障和短期重算；按 receivedAt 索引分批清理，不做物理分区。

```sql
CREATE TABLE heartbeat_daily (
    stat_date DATE NOT NULL COMMENT 'Asia/Shanghai partition key',
    user_id VARCHAR(128) COLLATE utf8mb4_0900_as_ci NOT NULL,
    latest_client_version VARCHAR(64) NOT NULL,
    latest_os VARCHAR(16) NOT NULL,
    app_start_count INT UNSIGNED NOT NULL DEFAULT 0,
    first_received_at DATETIME(3) NOT NULL COMMENT 'UTC',
    last_received_at DATETIME(3) NOT NULL COMMENT 'UTC',
    PRIMARY KEY (stat_date, user_id),
    KEY idx_heartbeat_daily_user_date (user_id, stat_date),
    KEY idx_heartbeat_daily_version_date (latest_client_version, stat_date),
    KEY idx_heartbeat_daily_os_date (latest_os, stat_date)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_ci
PARTITION BY RANGE COLUMNS(stat_date) (
    PARTITION p202608 VALUES LESS THAN ('2026-09-01'),
    PARTITION p202609 VALUES LESS THAN ('2026-10-01'),
    PARTITION p202610 VALUES LESS THAN ('2026-11-01'),
    PARTITION p_future VALUES LESS THAN (MAXVALUE)
);
```

分区边界由实际上线月份生成。维护任务每月提前创建未来 3 个月分区并保留 p_future；分区缺失时告警。heartbeat_daily 长期保留，以支持首次观测用户计算。

### 2.6 统计口径

```text
DAU(D) = heartbeat_daily 中 statDate=D 的行数
WAU(D) = [D-6,D] 内 distinct userId
MAU(D) = [D-29,D] 内 distinct userId
DAU/MAU(D) = DAU(D) / MAU(D)
周活跃回访率(W→W+1) = 两周都活跃的 userId 数 / W 活跃 userId 数
首次观测用户(W) = 全部历史中 MIN(statDate) 落在 W 的 userId
首次观测用户次周留存(W→W+1) = 首次观测用户中 W+1 活跃人数 / W 首次观测用户数
人均应用启动次数(D) = SUM(appStartCount) / DAU(D)
版本活跃用户(D,V) = statDate=D 且 latestClientVersion=V 的行数
系统活跃用户(D,OS) = statDate=D 且 latestOs=OS 的行数
```

自然周为周一至周日。次周留存只发布 W+1 已完整结束的 cohort，观察中不进入趋势，分母为 0 返回 null。“首次观测”从正式上线后的首批数据直接计算；存量用户少，接受其首次出现被视为新用户。

### 2.7 响应

```json
{
  "code": 200,
  "msg": "",
  "data": {
    "accepted": true,
    "duplicate": false,
    "receivedAt": "2026-07-29T00:30:01Z"
  }
}
```

重复 heartbeatId 同样返回 HTTP 200，`accepted=false、duplicate=true`。

---
## 3. 接口二：`POST /api/client-report/metrics/batch`

> 完整事件 DTO、outbox/uploader/reconciliation、服务端逐事件接收与事实表 DDL 见：[批量指标上报开发方案](../../开发计划/数据采集/批量指标上报开发方案.md)。

### 3.1 职责

V2 将该接口从“节点快照批次”升级为“执行领域事件批次”。路径保持不变，请求体从 `metrics` 破坏式升级为 `events`。

支持事件：

```text
execution.started
execution.paused
execution.resumed
execution.finished
unit.finished
intervention.requested
```

### 3.2 批次请求

```json
{
  "schemaVersion": 2,
  "batchId": "uuid",
  "sentAt": "2026-07-29T08:31:00.000Z",
  "client": {
    "clientVersion": "0.1.0",
    "os": "windows"
  },
  "events": []
}
```

约束：

- 单批最多 200 个事件，压缩前 JSON 最大 512 KiB。
- 事件按本地 `createdAt` 顺序发送。
- 单事件序列化后最大 64 KiB。
- 一个批次可含多个 workspace 的重试事件，因此 userId 与 workspace 只放在每个事件信封中，批次级 client 不重复身份。
- 客户端同一时间最多一个 metrics/batch 请求 in-flight。

### 3.3 统一事件信封

```json
{
  "eventId": "事件发生实例 UUID",
  "idempotencyKey": "同一领域事实重试时保持稳定",
  "type": "execution.started",
  "occurredAt": "2026-07-29T08:30:10.000Z",
  "mode": "direct",
  "userId": "kelvinzhou",
  "workspace": "D:\\IdeaProjects\\Gold-Band",
  "taskId": "task uuid",
  "execution": {
    "executionId": "turn uuid",
    "executionKind": "turn",
    "origin": "initialPrompt"
  },
  "refs": {
    "runId": "run uuid or null",
    "turnId": "turn uuid or null",
    "roundId": null,
    "nodeId": null,
    "nodeKey": null,
    "attemptId": null,
    "parentExecutionId": null,
    "parentUnitId": null,
    "dynamicRunId": null,
    "outerNodeId": null,
    "outerAttemptId": null
  },
  "payload": {}
}
```

公共枚举：

| 字段 | 值 |
|---|---|
| `mode` | `direct / workflow / auto` |
| `executionKind` | `turn / run` |
| `origin` | `initialPrompt / followUp / newRun / rerun / runtimeContinue` |

`eventId` 表示一次真实事件；`idempotencyKey` 表示该事件的网络重试身份。同一个 attempt 两次相同暂停原因必须有不同 `eventId`。

---

## 4. 事件定义

### 4.1 `execution.started`

这是所有完成率、成功率的唯一分母来源。

#### Direct

每个用户 prompt 被后端接受并获得稳定 `turnId` 后发送。Direct 首轮虽然内部创建单 Worker run，但分析层只产生一个 Direct turn execution，不额外产生 Workflow run execution。

```json
{
  "type": "execution.started",
  "mode": "direct",
  "execution": {
    "executionId": "turn uuid",
    "executionKind": "turn",
    "origin": "followUp"
  },
  "refs": {
    "runId": "首轮内部 run uuid 或上下文 run uuid",
    "turnId": "turn uuid"
  },
  "payload": {
    "agentId": "managed agent id",
    "requestedModel": null,
    "permissionMode": null
  }
}
```

#### Workflow

`RunState`、首轮 RoundState、NodeState 成功持久化后发送：

```json
{
  "type": "execution.started",
  "mode": "workflow",
  "execution": {
    "executionId": "run uuid",
    "executionKind": "run",
    "origin": "newRun"
  },
  "payload": {
    "workflowId": "stable workflow DSL id",
    "workflowSnapshotId": "snapshot digest/id",
    "declaredNodeCount": 6
  }
}
```

#### AUTO

outer runtime run 创建后发送，主 execution 仍是 outer run：

```json
{
  "type": "execution.started",
  "mode": "auto",
  "execution": {
    "executionId": "outer run uuid",
    "executionKind": "run",
    "origin": "newRun"
  },
  "payload": {
    "workflowId": "generated AUTO wrapper workflow id",
    "autoTemplateId": null,
    "agentStrategy": "dynamic",
    "dynamicLimits": {
      "maxDynamicNodes": 20,
      "maxParallel": 4,
      "maxWorkflowInvocations": 3
    }
  }
}
```

不上传 AUTO goal、动态节点正文或 agent 决策指南。

### 4.2 `execution.finished`

每个 started execution 最多一个终局事件。

Direct turn 示例：

```json
{
  "type": "execution.finished",
  "mode": "direct",
  "execution": {
    "executionId": "turn uuid",
    "executionKind": "turn",
    "origin": "followUp"
  },
  "payload": {
    "outcome": "completed",
    "terminalReasonCode": null,
    "durationMs": 18420,
    "providerId": "codex-acp",
    "requestedModel": null,
    "resolvedModel": "gpt-5.6-sol",
    "usage": {
      "usageScopeId": "turn uuid",
      "inputTokens": 1200,
      "outputTokens": 640,
      "cacheReadTokens": 300,
      "totalTokens": 1840
    }
  }
}
```

Direct outcome 为 `completed / failed / cancelled`，只表示本轮回复终态，不表示问题已解决。

Workflow/AUTO run 示例：

```json
{
  "type": "execution.finished",
  "mode": "workflow",
  "execution": {
    "executionId": "run uuid",
    "executionKind": "run",
    "origin": "newRun"
  },
  "payload": {
    "outcome": "success",
    "terminalReasonCode": null,
    "durationMs": 240000,
    "newRoundsOpened": 0
  }
}
```

run outcome 为 `success / failure / killed`。AUTO 可额外携带 `dynamicNodeCount`、`workflowInvocationCount`、`acceptanceAttemptCount`、`rejectedProposalCount`，全部从已持久化 DynamicGraphState 读取。

### 4.3 `unit.finished`

记录 Workflow/AUTO 内部实际执行单元的定局。取消 V1 的 RUNNING、前驱重复记录和开始/结束 sentinel。

```json
{
  "type": "unit.finished",
  "mode": "auto",
  "execution": {
    "executionId": "outer run uuid",
    "executionKind": "run",
    "origin": "newRun"
  },
  "refs": {
    "runId": "outer run uuid",
    "roundId": "round uuid",
    "nodeId": "dynamic node uuid",
    "nodeKey": "worker-2",
    "attemptId": "attempt uuid",
    "parentUnitId": "outer ai-dynamic attempt uuid",
    "dynamicRunId": "dynamic run id",
    "outerNodeId": "outer node uuid",
    "outerAttemptId": "outer attempt uuid"
  },
  "payload": {
    "unitKind": "dynamicNodeAttempt",
    "dynamicNodeKind": "worker",
    "depth": 1,
    "groupId": null,
    "outcome": "success",
    "pauseReason": null,
    "durationMs": 22000,
    "providerId": "claude-acp",
    "requestedModel": "sonnet",
    "resolvedModel": "claude-sonnet-4",
    "usageAttribution": "billable",
    "usage": {
      "usageScopeId": "attempt uuid",
      "inputTokens": 2200,
      "outputTokens": 800,
      "cacheReadTokens": 1000,
      "totalTokens": 3000
    },
    "workflowId": null,
    "childExecutionId": null
  }
}
```

`unitKind` 为 `workflowNodeAttempt / dynamicNodeAttempt`。AUTO 的 `dynamicNodeKind` 为 `worker / merge / acceptance / workflowInvocation`。

Token 防重规则：

- `usageAttribution=billable` 才参与 token 聚合。
- `usageAttribution=container` 只表达结构。
- AI-DYNAMIC outer container 不得再次携带内部节点累计 token。
- `workflowInvocation` 与 child run 不得重复计费。
- 同一 attempt 内 repair/continue 只上报该 attempt usage 增量。

### 4.4 `execution.paused` / `execution.resumed`

只描述 runtime run 状态变化，不用于 Direct completed-run follow-up。

```json
{
  "type": "execution.paused",
  "mode": "workflow",
  "payload": {
    "pauseReason": "waitingForUserInput",
    "errorCode": null
  }
}
```

`pauseReason` 为 `processInterrupted / runtimeAbnormal / errorBlocked / waitingForUserInput / permissionRequested`。`execution.resumed` 引用被恢复 execution，并使用新的状态转移事件身份。

### 4.5 `intervention.requested`

```json
{
  "type": "intervention.requested",
  "mode": "auto",
  "payload": {
    "kind": "permissionRequested",
    "requestId": "ACP request stable id",
    "source": "dynamicNode"
  }
}
```

`kind` 为 `manualDecisionRequired / elicitationRequested / permissionRequested / runtimeAbnormal / errorBlocked / processInterrupted`。同一 attempt 的不同 request 必须有不同 `requestId` 和 `eventId`。

---

## 5. 三种模式采集映射

| 生命周期事实 | Direct | Workflow | AUTO |
|---|---|---|---|
| 开始一次可测执行 | 每个 prompt → turn started | run 创建 → run started | outer run 创建 → run started |
| 自动执行终局 | turn finished | run finished | outer run finished |
| 节点详情 | 不上报内部单 Worker 壳 | workflow node attempt | outer node + dynamic node attempt |
| 手动追问 | 新 turn，不改 run | 新 turn，`mode=workflow` | 新 turn，`mode=auto` |
| runtime continue | 首轮未终局时恢复原容器 | 原 run resumed | outer run/dynamic leaf resumed |
| 干预 | 关联 turn | 关联 run + attempt | 关联 outer run + dynamic leaf |
| token | 每 turn 增量 | 每 node attempt 增量 | 每 billable attempt 增量 |
| 成功率口径 | 回复完成率 | run 终局成功率 | outer run 终局成功率 |

### 5.1 Direct 首轮

Direct 首轮同时存在内部 run 与用户可见 turn：analytics 只发送 Direct turn started/finished，内部单 Worker 不发送 Workflow run execution 或 workflow unit，token 全部归属 `usageScopeId=turnId`。

### 5.2 Workflow/AUTO 完成后的手动追问

已完成 run 上的手动 prompt 新建 `executionKind=turn`，mode 继承 conversation，`origin=followUp`，`refs.runId` 指向上下文 run；不新建 run、不修改既有 run outcome、成功率或耗时。

### 5.3 AUTO 父子关系

AUTO 同时保留 outer run、outer AI-DYNAMIC attempt、dynamic run、dynamic node attempt 和 child workflow run。child workflow 带：

```text
parentExecutionId = outer run id
parentUnitId = workflowInvocation dynamic node attempt id
```

AUTO 产品成功率只统计 outer run；child run 只用于下钻，不能重复进入 AUTO 总 run 分母。

---

## 6. 可靠投递

### 6.1 SQLite outbox

V2 使用项目已有 `rusqlite` 建独立 `analytics.sqlite3`，不与可重建的搜索索引数据库共用生命周期：

```text
analytics_outbox
  event_id PK
  idempotency_key UNIQUE
  schema_version
  event_type
  mode
  occurred_at
  payload_json
  status              pending / in_flight / acked / rejected
  attempt_count
  next_attempt_at
  lease_owner?
  lease_until?
  created_at
  acked_at?
  last_error_code?
```

职责分离：

- `AnalyticsRecorder`：字段规范化和 SQLite 插入。
- `AnalyticsUploader`：后台批量上传、退避重试和确认。
- `RuntimeLifecycleBus`：继续服务 UI/通知和 heartbeat transient 投影；不承担 metrics/batch 的 durable 网络可靠性。

Recorder 可作为 inline subscriber 只做快速 SQLite 插入；网络请求始终后台执行。Uploader 在事务内 claim 事件并设置 60 秒 lease，崩溃后由过期 lease 恢复，不依赖不可靠的永久 sending 状态。

### 6.2 文件状态与 outbox 的崩溃窗口

当前 runtime 状态使用文件，不能和 SQLite outbox 做单库事务。使用“领域状态先落盘 + 确定性幂等键 + 启动 reconciliation”：

1. 领域状态持久化。
2. 同步写 outbox。
3. 再发进程内 lifecycle event。
4. 若步骤 1/2 之间崩溃，重启扫描 persisted run/turn/attempt 状态。
5. 用确定性 `idempotencyKey` 补写缺失事件。

analytics 写入失败不能阻断用户任务，但必须记录结构化错误并可恢复。

### 6.3 重试与响应

- 网络错误、超时、429、5xx：full jitter 退避；429 优先使用合法 Retry-After。
- 合法批次内 422 单事件失败：只标记对应事件 rejected。
- 413：二分批次；单事件仍超限则 rejected。
- 请求级 400：暂停 uploader，记录客户端契约错误。
- 401/403：暂停 uploader，配置变化后恢复。
- 正常退出最多等待 2 秒 flush，但不依赖 flush 保证正确性。
- pending 事件重启后继续上传。

批次响应：

```json
{
  "code": 200,
  "msg": "",
  "data": {
    "requestId": "server request id",
    "batchId": "client batch id",
    "acceptedEventIds": ["..."],
    "duplicateEventIds": ["..."],
    "rejected": [
      {
        "eventId": "...",
        "error": {
          "code": 422103,
          "params": { "field": "payload.outcome" }
        }
      }
    ],
    "receivedAt": "2026-07-30T02:20:01.000Z"
  }
}
```

accepted、duplicate、rejected 必须完整且互斥地覆盖请求 eventId；响应缺项或出现未知 eventId 时整批恢复 pending。错误只返回结构化 code + params，不返回对客文案。

---

## 7. 服务端数据模型

### 7.1 原始事实

```text
analytics_event_raw
  event_id PK
  idempotency_key UNIQUE
  schema_version
  event_type
  mode
  user_id
  workspace
  task_id
  execution_id
  execution_kind
  occurred_at
  received_at
  payload_json
  materialize_status
  materialize_attempts
  next_materialize_at
  materialized_at?
  materialize_error_code?
```

原始表只追加和去重，不在写入请求内计算业务指标。合法事件批量写 raw 后才返回 accepted，事实表由异步 materializer 构建并可从 raw 重放。

### 7.2 物化事实

```text
execution_fact
  execution_id PK
  mode
  execution_kind
  origin
  workspace
  task_id
  parent_execution_id?
  started_at
  finished_at?
  outcome?
  terminal_reason_code?
  duration_ms?

unit_fact
  attempt_id PK
  execution_id
  parent_unit_id?
  unit_kind
  node_key?
  dynamic_node_kind?
  provider_id?
  requested_model?
  resolved_model?
  outcome
  duration_ms

execution_transition_fact
  transition_id PK
  event_id UNIQUE
  execution_id
  transition_type
  reason?
  error_code?
  occurred_at

intervention_fact
  event_id PK
  execution_id
  attempt_id?
  request_id?
  kind
  source
  occurred_at

usage_fact
  usage_scope_id PK
  source_event_id UNIQUE
  execution_id
  attempt_id?
  attribution
  provider_id?
  requested_model?
  resolved_model?
  input_tokens
  output_tokens
  cache_read_tokens
  total_tokens
  occurred_at
```

pause/resume 历史进入 execution_transition_fact，不覆盖 execution_fact 当前状态。Direct execution usage 与 Workflow/AUTO unit usage 统一进入 usage_fact，以 usage_scope_id 主键实现跨事实表全局防重；container usage 不写 usage_fact。

---

## 8. 指标口径

### 8.1 观察窗口与可信度

started 后不能立即把未 finished 的执行认定为失败。服务端设置 `terminalGracePeriod`：未超过 grace period 的记作 `inProgressOrLate`；超过仍无 finished 的记作 `unclosed`。

```text
终局覆盖率 = 有 finished 的 eligible execution / eligible started execution
```

终局覆盖率不足时，成功率不得单独作为可靠结论。

### 8.2 Direct

```text
回复完成率 = completed turn / eligible started turn
回复失败率 = failed turn / eligible started turn
用户取消率 = cancelled turn / eligible started turn
单轮 token = Σ turn usage / completed turn
P50/P95 回复耗时 = completed turn duration 分位数
单轮干预率 = 有 permission/elicitation 的 turn / started turn
```

Direct 不计算 run 成功率、一次做对率、全自动闭环率或用户任务成功率。

### 8.3 Workflow

```text
run 终局成功率 = success run / eligible started run
未达终局率 = unclosed run / eligible started run
首过率 = newRoundsOpened=0 的 success run / success run
成功 run token = Σ success run 的 billable unit token / success run
全自动率 = 无 intervention 的 eligible run / eligible started run
```

可靠性拆分 runtime 故障率、用户/环境中断率、kill 率、权限等待率和信息补充率。

### 8.4 AUTO

AUTO 产品层以 outer run 计算：outer run 终局成功率、未达终局率、成功 outer run token、全自动率、dynamic 节点数、并行度、workflow invocation 次数、acceptance 首次通过率和 rejected proposal 率。

AUTO 首次通过定义为：成功 outer run 中，所有 acceptance unit 均在第一个 attempt 成功，且 `rejectedProposalCount=0`。它与 Workflow 的 `newRoundsOpened=0` 是不同指标。

### 8.5 provider/model

V2 第一阶段只发布每 provider/model 的 unit 完成率、input/output/cache token、按 mode/workflow/dynamicNodeKind/nodeKey 分层的 token 效率，以及模型参与 run 的相关性下钻。

有同类任务分层、价格版本/真实账单、多模型归因，并最好有受控 routing experiment 前，不发布“模型经济性价比”。

---

## 9. 数据—价值追溯矩阵

| 价值点 | Direct | Workflow | AUTO | 数据来源 |
|---|---|---|---|---|
| 执行覆盖 | turn started/finished | run started/finished | outer run started/finished | execution events |
| 交付终局 | 回复完成，不代表任务成功 | run success/failure/killed | outer run success/failure/killed | execution.finished |
| 首次通过 | 不适用 | newRoundsOpened | acceptance attempts + rejected proposals | execution/unit |
| token 效率 | turn usage delta | billable node attempts | billable dynamic/top-level attempts | usage |
| 自动化 | 单轮干预负担 | 无 intervention run | 无 intervention outer run | intervention |
| 可靠性 | failed/cancelled turn | pause/kill/error | outer + dynamic leaf pause/error | execution/unit |
| 模型分析 | 单轮 provider/model | node provider/model | dynamic role provider/model | resolvedModel |
| 活跃、回访与首次观测留存 | 支持 | 支持 | 支持 | heartbeat |

任何报表字段都必须能回溯到此矩阵中的事件和字段；无来源指标不得上线。

---

## 10. 落地优先级

### 第一阶段：可信执行主链

- 固化 canonical UUID 与 attempt UUID。
- 增加 `execution.started`。
- Direct 接入 turn started/finished。
- Workflow/AUTO 接入 run started/finished。
- 直接 HTTP 改为 SQLite outbox + uploader。
- 服务端实现 V2 events 批次、幂等接收和 execution_fact。

### 第二阶段：执行单元与 token

- 删除 V1 RUNNING、前驱重复、sentinel。
- Workflow node attempt 上报。
- AUTO dynamic node / child workflow 父子关系上报。
- provider、requestedModel、resolvedModel、usage delta 上报。
- 服务端构建 unit_fact；所有可计费 usage 统一写入 usage_fact，以 usageScopeId 全局防重。

### 第三阶段：干预、留存与高级分析

- intervention 全量接入。
- 服务端构建 execution_transition_fact 与 intervention_fact，保留 pause/resume 和人工干预历史。
- heartbeat 五字段、六值 reason、生命周期总线投影、两表事务、长期日聚合与月分区。
- AUTO acceptance/proposal 指标。
- 分层 token 效率和受控模型实验。

---

## 11. 接口级验收基准

1. Direct 首轮只产生一个 turn execution，不产生重复 Workflow run execution。
2. Direct 同一 ACP session 连续两轮只上报各自 usage 增量。
3. Workflow 首节点完成前退出仍有 `execution.started`。
4. 同一 node 多 attempt 不覆盖，token 可完整聚合。
5. Workflow run 完成后的手动追问不改变 run outcome。
6. AUTO outer container 与 dynamic leaf token 不重复。
7. AUTO workflow invocation 可关联 child run，但 child run 不进入 outer run 分母。
8. 同一事件重复上传只入库一次。
9. 网络失败、应用重启后 pending event 可继续发送。
10. state 已落盘但 outbox 缺失时 reconciliation 能补事件。
11. 所有时间均为 UTC RFC 3339。
12. heartbeat payload 不含 workspace；metrics/batch 只保留结构化 workspace，所有 payload 均不含 task title、prompt、回复或工具正文。

---

## 12. 变更记录

- V1：以 heartbeat + node metrics 为基础，规划 run/pause/intervention 新端点。
- V2：取消新端点，保留 heartbeat 与 metrics/batch；heartbeat 收敛为五字段六值 reason，删除 workspace、session、客户端时间与执行周期心跳，支持 userId 活跃/留存及用户顶层模式启动、定时任务创建的分类统计。
