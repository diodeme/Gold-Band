# 指标上报客户端变更与服务端优化方案

> 本文档梳理 `feature_metrics_report` 分支从 `8b622fb` 到 `70561c9` 共 6 个 commit 的客户端改动，分析其对服务端接收、校验、存储和投影逻辑的影响，并给出服务端适配优化清单。
>
> 服务端无存量数据，所有变更均为破坏式更新，不保留兼容层。

## 1. 客户端变更清单

### 1.1 协议字段变更

| 变更类型 | 字段 | 说明 |
|---|---|---|
| **删除** | `taskId` | 不再单独上报，`executionId` 即 `taskId`，服务端各表 `task_id` 列从 `executionId` 提取 |
| **删除** | `parentExecutionId` | AUTO 模式不再有父子 execution 层级，所有节点共享同一 `executionId` |
| **删除** | `unitId` | 节点标识统一用 `nodeId` |
| **新增** | `taskTitle` | 任务标题，即工作空间下展示的名称；所有事件携带，`Option<String>` 缺失时跳过序列化 |
| **新增** | `collectionStateRecovered` | 标记事件来自状态恢复路径（observability snapshot 恢复），布尔值 |

### 1.2 时间格式变更

| 字段 | 旧格式 | 新格式 |
|---|---|---|
| `occurredAt` | `2026-08-05T10:38:15.197+08:00`（RFC3339 带时区偏移） | `2026-08-05T10:38:15.197`（本地时间，无偏移量） |
| `reportedAt` | 同上 | 同上 |
| `timing.startedAt` | `1785900140Z`（Unix 秒，非法 ISO-8601） | `2026-08-05T10:38:15.000`（ISO-8601） |
| `timing.endedAt` | 同上 | 同上 |

服务端此前因 `occurredAt` 格式不合法返回 `METRICS_FIELD_INVALID`，根因即此。

### 1.3 executionId 语义变更

| 模式 | 旧语义 | 新语义 |
|---|---|---|
| Direct | executionId = 每轮新建 UUID | executionId = taskUuid（不变） |
| Workflow | node executionId = runUuid + round/node 派生 | executionId = taskUuid（全 task 共享） |
| AUTO | unit executionId = runUuid；有 parentExecutionId | executionId = taskUuid（全 task 共享）；无 parentExecutionId |

统一规则：一个 task 的所有事件（Direct turn / Workflow node / AUTO unit / run delivery）共享同一个 `executionId`。不同执行单元通过 `attemptId` + `nodeId` 区分。

### 1.4 attemptId / nodeId 变更

| 模式 | nodeId | attemptId | attemptIndex |
|---|---|---|---|
| Direct | 不设 | = executionId（= taskUuid），同一 task 内不变 | 固定 1 |
| Workflow | runUuid + round/node 派生（UUID v5，重试不变） | NodeState UUID（每次执行新建） | 同一 nodeId 下从 1 递增 |
| AUTO | DynamicNodeState.uuid（重试不变） | nodeId + 本地 attempt 序号派生（UUID v5） | 从 1 递增 |

### 1.5 model 字段变更

| 事件类型 | 旧行为 | 新行为 |
|---|---|---|
| `execution.started` | 携带 resolved model（可能是 `opus` 等占位值） | 不携带（`null`），因 ACP session 尚未启动 |
| `execution.completed` | 可能携带占位值 | 从 `acp.session.json` 解析实际模型名（如 `glm-5.2`） |

### 1.6 其他数据质量改进

| 改进 | 说明 |
|---|---|
| `timing.acpSessionElapsedMs` | 旧值经常为 `null`；现从 ACP session timing segment 累计获取 |
| `modelUsages[].acpSessionElapsedMs` | 同上，从 segment `elapsed_ms` 传入 |
| `counters.followUpCount` | Direct 同一 task 多轮用户输入在同一 attempt 内累加 |
| Workflow NodeCompleted 覆盖率 | 修复暂停/恢复路径下 completed 事件缺失问题，确保每个完成节点都有 started+completed |
| `counters.manualContinueCount` | 人工追问次数正确计数 |

## 2. 服务端影响分析

### 2.1 破坏式变更（需立即适配）

| 影响项 | 严重度 | 说明 |
|---|---|---|
| **时间格式解析** | P0 | 服务端必须能解析无时区偏移量的 `yyyy-MM-ddTHH:mm:ss.SSS` 格式。旧的 RFC3339 解析器无法直接解析。需将时间字符串视为本地时间（客户端时区 = Asia/Shanghai），再转换为 UTC 存储 |
| **executionId 语义** | P0 | 服务端所有以 `executionId` 为键的查找、投影和统计逻辑需要适配新语义。Workflow node 的 executionId 不再含 runUuid 前缀；AUTO unit 的 executionId 不再是 runUuid |
| **taskId 字段移除** | P1 | 服务端 `task_id` 列的取值源从 `taskId` 字段改为 `executionId`。DDL 不变，提取逻辑需更新 |
| **parentExecutionId 移除** | P1 | 服务端不再需要处理 AUTO 的父子层级关联。投影查询简化：不再通过 parentExecutionId 查找子节点 |
| **unitId 移除** | P1 | 服务端 `node_id` 列的取值源从 `unitId` 改为 `nodeId`。DDL 不变，提取逻辑需更新 |
| **started 事件 model 为 null** | P2 | 服务端 attempt 表的 `final_model` 不能从 started 事件初始化，须等到 completed 事件 |
| **taskTitle 新增** | P2 | 四张表需新增 `task_title` 列，payload 提取逻辑需增加该字段 |

### 2.2 数据质量改善（降低服务端补偿负担）

| 改善 | 服务端收益 |
|---|---|
| NodeCompleted 覆盖率提升 | `start_event_missing` 标记减少，attempt 投影准确性提高 |
| model 为实际模型名 | `final_model` 列存储真实模型（如 `glm-5.2`），不再是占位值（如 `opus`），模型质量统计不再需要后处理清洗 |
| acpSessionElapsedMs 不为 null | 效率成本统计可直接使用，无需服务端填充默认值 |
| followUpCount 累计 | Direct 交付统计的追问维度可用 |
| 时间格式统一为无偏移量 | 服务端不再需要处理混合格式（部分带偏移、部分不带）的兼容逻辑 |

### 2.3 新增字段 collectionStateRecovered

客户端新增 `collectionStateRecovered` 布尔字段，标记事件来自 observability snapshot 恢复路径而非实时生命周期。

服务端用途：写入 `ml_metric_attempt.collection_state_recovered` 列，用于采集质量分析（判断哪些 attempt 的终态数据来自恢复路径而非实时上报）。

## 3. 服务端适配优化清单

### 3.1 DDL 变更

四张表均已添加 `task_title VARCHAR(255) NULL` 列（本次客户端改动同步更新了服务端 DDL 文档）。无需额外迁移。

```sql
-- 已在 metrics-server-processing.md 中更新
task_title VARCHAR(255) NULL COMMENT '任务标题，即工作空间下展示的名称',
```

### 3.2 时间解析优化

**现状：** 文档已说明 "API 输入时间为本地时间，不带时区偏移量，格式 `yyyy-MM-ddTHH:mm:ss.SSS`。服务端解析为时间点后统一转换为 UTC"。

**需实现：**

```java
// 解析无时区偏移量的本地时间，视为 Asia/Shanghai 时区
DateTimeFormatter INPUT_FORMATTER = DateTimeFormatter
    .ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS")
    .withZone(ZoneId.of("Asia/Shanghai"));

Instant parseClientTime(String input) {
    return INPUT_FORMATTER.parse(input, Instant::from);
}
```

**校验规则不变：** `METRICS_REPORTED_AT_OUT_OF_RANGE` 仍然校验 `reportedAt` 与服务端时间偏差不超过 24 小时。

### 3.3 executionId 查找优化

**旧逻辑影响：** AUTO 模式此前通过 `parentExecutionId` 关联子节点，Workflow node 通过 runUuid 派生的 executionId 查找。现在统一为 taskUuid。

**优化方向：**

1. `ml_metric_delivery_stat` 的主键 `PRIMARY KEY (report_date, execution_id)` 不变，但 `execution_id` 的值从 runUuid 变为 taskUuid。
2. AUTO unit 的 attempt 查询从 "按 parentExecutionId 查子节点" 简化为 "按 executionId 查同 task 的所有 attempt"。
3. Workflow node 的 attempt 查询从 "按 runUuid 派生 executionId" 简化为 "按 executionId + nodeId"。

**投影查询简化示例：**

```sql
-- 旧：AUTO outer-run 查子 unit（需要 parentExecutionId）
SELECT * FROM ml_metric_attempt
WHERE report_date = ? AND execution_id IN (
    SELECT execution_id FROM ml_metric_event
    WHERE report_date = ? AND raw_payload->'$.parentExecutionId' = ?
);

-- 新：按 executionId 直接查同 task 的所有 attempt
SELECT * FROM ml_metric_attempt
WHERE report_date = ? AND execution_id = ?;
```

### 3.4 字段提取映射更新

| 服务端列 | 旧提取路径 | 新提取路径 |
|---|---|---|
| `task_id` | `$.taskId` | `$.executionId`（值相同） |
| `task_title` | 不存在 | `$.taskTitle`（新增） |
| `node_id` | `$.unitId`（AUTO）/ `$.nodeId`（Workflow） | `$.nodeId`（统一） |
| `final_model` | 从 started 或 completed 均可取 | 仅从 completed 取（started 为 null） |
| `collection_state_recovered` | 不存在 | `$.collectionStateRecovered`（新增） |

### 3.5 校验规则更新

| 校验项 | 旧规则 | 新规则 |
|---|---|---|
| `occurredAt` 格式 | 必须为 RFC3339 | 必须为 `yyyy-MM-ddTHH:mm:ss.SSS`（无偏移量） |
| `reportedAt` 格式 | 同上 | 同上 |
| `taskId` 存在性 | 必填 | 字段已删除，不再校验 |
| `parentExecutionId` | AUTO 必填 | 字段已删除，不再校验 |
| `unitId` | AUTO 必填 | 字段已删除，不再校验 |
| `nodeId` | AUTO 从 `unitId` 取 | AUTO 从 `nodeId` 取 |
| started 事件 `model` | 非空校验 | 允许为 null |
| `taskTitle` | 不存在 | 可选（`skip_serializing_if` 为 None 时 JSON 中不含该字段） |

### 3.6 统计查询适配

**模型质量统计：**

```sql
-- 旧：started 和 completed 都可能有 model，取最后一条
SELECT final_model FROM ml_metric_attempt WHERE ...;

-- 新：final_model 仅来自 completed 事件，started 事件 model 为 null
-- 服务端 attempt 表的 final_model 列在 started 插入时为 NULL，
-- terminal 更新时从 completed 事件的 model 字段写入。
-- 查询逻辑不变，但插入逻辑需确保 started 不覆盖 final_model。
```

**AUTO 交付统计：**

```sql
-- 旧：outer-run delivery_stat 通过 parentExecutionId 关联子 unit
-- 新：outer-run delivery_stat 通过 executionId(=taskUuid) 直接关联所有 attempt
SELECT
    d.execution_id,
    d.outcome,
    COUNT(a.attempt_id) AS attempt_count,
    SUM(a.input_tokens) AS total_input_tokens
FROM ml_metric_delivery_stat d
JOIN ml_metric_attempt a ON d.report_date = a.report_date
    AND d.execution_id = a.execution_id
WHERE d.report_date = ? AND d.execution_kind = 'outer-run'
GROUP BY d.execution_id, d.outcome;
```

## 4. 实现优先级

| 优先级 | 任务 | 阻塞性 |
|---|---|---|
| P0 | 时间格式解析适配（无偏移量本地时间 转 UTC） | 阻塞所有数据接收，当前返回 `METRICS_FIELD_INVALID` |
| P0 | `task_id` 提取路径从 `taskId` 改为 `executionId` | 阻塞插入（NOT NULL 列无值） |
| P1 | `node_id` 提取路径从 `unitId` 改为 `nodeId` | 影响 AUTO 数据写入 |
| P1 | `task_title` 列和 `collectionStateRecovered` 字段提取 | 非阻塞（可为 NULL），但影响数据完整性 |
| P1 | started 事件 `model` 允许为 null，不覆盖 `final_model` | 影响模型统计 |
| P2 | 删除 `parentExecutionId` 相关的投影查询和关联逻辑 | 清理无用代码，简化查询 |
| P2 | `final_model` 插入逻辑确保 started 不覆盖 | 数据准确性 |

## 5. 客户端上报事件全字段参考

以下是当前客户端 DTO 的完整字段列表，供服务端实现 payload 解析参考：

```json
{
  "eventId": "string",
  "eventRevision": 1,
  "eventType": "execution.started|execution.completed|execution.paused|execution.resumed|intervention.requested|acceptance.completed",
  "occurredAt": "2026-08-09T14:12:42.000",
  "reportedAt": "2026-08-09T14:12:42.990",
  "userId": "kelvinzhou",
  "workspace": "D:\\IdeaProjects\\mall",
  "clientVersion": "0.9.0",
  "sessionMode": "direct|workflow|auto",
  "executionKind": "turn|run|outer-run|node-attempt|unit-attempt",
  "executionId": "task-uuid",
  "taskTitle": "可选，任务标题",
  "nodeId": "可选，节点稳定标识",
  "attemptId": "可选，每次执行尝试唯一",
  "attemptIndex": "可选，从1开始",
  "roundIndex": "可选，Workflow round 序号",
  "roleName": "可选，resolved profile 名称",
  "outcome": "可选，completed|failed|cancelled|success|failure|killed",
  "terminalReason": "可选，稳定分类枚举",
  "counters": {
    "pauseCount": 0,
    "resumeCount": 0,
    "permissionRequestCount": 0,
    "elicitationCount": 0,
    "manualContinueCount": 0,
    "followUpCount": 0
  },
  "unitKind": "可选，worker|workflow-invocation|merge|acceptance",
  "childRunId": "可选，被调用的 Workflow run UUID",
  "terminalReasonCode": "可选",
  "failedAttemptId": "可选",
  "roundCount": "可选，Workflow run 总 round 数",
  "passed": "可选，acceptance 是否通过",
  "acceptanceAttempt": "可选",
  "firstPass": "可选",
  "interventionKind": "可选",
  "pauseReason": "可选",
  "previousPauseReason": "可选",
  "provider": "可选，claude-acp|codex-acp",
  "model": "可选，实际模型名（如 glm-5.2），started 为 null",
  "usage": {
    "inputTokens": 0,
    "outputTokens": 0,
    "cacheReadTokens": 0,
    "totalTokens": 0
  },
  "modelUsages": [{
    "provider": "claude-acp",
    "model": "glm-5.2",
    "inputTokens": 0,
    "outputTokens": 0,
    "cacheReadTokens": 0,
    "totalTokens": 0,
    "acpSessionElapsedMs": 0
  }],
  "timing": {
    "startedAt": "2026-08-09T14:12:42.000",
    "endedAt": "2026-08-09T14:13:08.000",
    "acpSessionElapsedMs": 11000
  },
  "collectionStateRecovered": true
}
```

所有标注"可选"的字段在值为 `None` / `null` 时不会出现在 JSON 中（`#[serde(skip_serializing_if = "Option::is_none")]`）。服务端应将缺失字段视为 `NULL`。
