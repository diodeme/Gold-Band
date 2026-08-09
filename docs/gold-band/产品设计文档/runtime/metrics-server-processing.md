# 会话指标上报服务端处理技术方案

## 1. 目标与范围

本文定义 `POST /api/client-report/metrics/batch` 的服务端接收、校验、幂等、月分区、插入更新和指标计算方案。客户端字段、枚举与生命周期定义以 `metrics-collection.md` 为准。

服务端覆盖 Direct、Workflow、AUTO，保存原始 `userId/workspace`，并支持：

- 完整事件时间线。
- attempt 粒度的模型、usage、可靠性分析。
- run/outer-run 粒度的交付和自动化统计。
- 以客户端上报日期进行月分区。
- 更新时最多查询上报月份及前一个自然月分区。

当前版本的幂等、身份定位和投影查找范围统一限定为“目标月份及前一个自然月”。不提供跨全部历史分区的全局幂等保证，也不处理一次任务因长期暂停而跨越两个以上自然月的场景。

## 2. 核心数据结构

原 `ml_metric_execution` 职责过多，拆为四张职责单一的表：

| 表 | 粒度 | 保存内容 | 更新方式 |
|---|---|---|---|
| `ml_metric_event` | 一条 lifecycle event | 原始事件、eventId、revision、完整时间线 | 只插入，不更新 |
| `ml_metric_attempt` | 一个 `attempt_id` | 一个“指标 attempt”（turn/node/unit 的一次真实尝试）的基础信息、终态、model usage | started 插入，terminal 更新 |
| `ml_metric_logical_execution` | 一个逻辑 `execution_id` | Direct turn 或 Workflow/AUTO node/unit 的 attempts 投影、当前/最终 attempt 和逻辑终态 | attempt 变化时由服务端重算/更新 |
| `ml_metric_delivery_stat` | 一个交付 execution | Direct turn、Workflow run、AUTO outer-run 的状态、质量字段和六个 Count | lifecycle 持续更新，terminal 覆盖统计快照 |

客户端只上报生命周期事实、终态快照和关联字段，不上报成功率、首过率、故障率、token 汇总等统计指标。所有统计结果均由服务端从 event、attempt、logical execution 和 delivery 投影计算。`ml_metric_attempt` 不保存 `*_count`；`ml_metric_delivery_stat` 不保存 node/unit 模型用量。这样 attempt 事实与交付统计不会混在一张宽表里。

## 3. 日期与月分区

### 3.1 唯一 DATE 字段

每张业务表只保留一个 `DATE` 字段：

```text
report_date = events[0].reportedAt 按 Asia/Shanghai 转换后的日期
```

- 月分区直接使用 `report_date`。
- 不设置 `partition_month`、`event_date`、`last_report_date` 等额外 DATE 字段。
- `occurred_at/started_at/ended_at` 是 `DATETIME(3)` 生命周期时间戳，不是分区日期。
- `received_at` 使用 `DATETIME(3)`，仅用于接收延迟和排障。
- API 输入时间为本地时间，不带时区偏移量，格式 `yyyy-MM-ddTHH:mm:ss.SSS`。服务端解析为时间点后统一转换为 UTC，所有 `DATETIME(3)` 字段均按 UTC 写入，数据库连接时区固定为 `+00:00`；只有 `report_date` 按 Asia/Shanghai 从 `reportedAt` 计算。
- 客户端不再单独上报 `taskId` 和 `parentExecutionId` 字段。`executionId` 即 `taskId`，服务端各表的 `task_id` 列直接从 `executionId` 提取。AUTO unit 的节点标识统一用 `nodeId`，不再有 `unitId` 字段。

### 3.2 批次分区规则

1. `reportedAt` 由客户端事件进入上报队列时生成并冻结，重试不得修改。
2. 服务端用第一条事件的 `reportedAt` 计算 `report_date` 和 `pYYYYMM`。
3. 批内其他事件必须属于同一个上报月份，否则整批返回 `METRICS_BATCH_CROSS_MONTH`。
4. 客户端必须提前按月份拆批。
5. 插入显式指定 `PARTITION (pYYYYMM)`。

### 3.3 更新分区规则

更新任意事件时，无论 eventType 是否为 started：

1. 查询目标月份分区。
2. 未找到只查询前一个自然月分区。
3. 找到后在原分区就地更新，`report_date` 不改变。
4. 两个分区都未找到时，在目标月份插入 missing-start 行。
5. 不查询更早分区。

因此跨月 execution 的主业务归属日期是首次插入的上报日期；后续真实上报时间仍保存在不可变事件事实的 `reported_at` 中。

本期只支持 execution 在目标月份或前一个自然月被定位。任务暂停后跨越两个以上自然月不属于本期处理范围，服务端不扫描更早分区，也不增加兼容或补偿路径。

## 4. API 说明

### 4.1 基本信息

| 项 | 值 |
|---|---|
| Method | `POST` |
| Path | `/api/client-report/metrics/batch` |
| Content-Type | `application/json;charset=UTF-8` |
| 鉴权 | `X-Maling-Report-Key` |
| 请求数组 | `events`，1～100 条 |
| 事务 | 整批原子；任一事件非法则整批不写 |
| 成功条件 | HTTP 200 且响应 `code=200` |

### 4.2 请求示例

```json
{
  "events": [{
    "eventId": "019c...",
    "eventRevision": 2,
    "eventType": "execution.completed",
    "occurredAt": "2026-08-01T10:20:15.120",
    "reportedAt": "2026-08-01T10:20:16.004",
    "userId": "raw-system-user",
    "workspace": "D:\\repo\\gold-band",
    "clientVersion": "0.1.0",
    "sessionMode": "workflow",
    "executionKind": "node-attempt",
    "executionId": "logical-node-execution-uuid",
    "attemptId": "node-attempt-uuid",
    "attemptIndex": 2,
    "nodeId": "node-uuid",
    "roundIndex": 1,
    "roleName": "代码审查员",
    "outcome": "success",
    "terminalReason": "completed",
    "provider": "codex-acp",
    "model": "gpt-5.6-sol",
    "usage": {
      "inputTokens": 1200,
      "outputTokens": 340,
      "cacheReadTokens": 800,
      "totalTokens": 1540
    },
    "modelUsages": [{
      "provider": "codex-acp",
      "model": "gpt-5.6-sol",
      "inputTokens": 1200,
      "outputTokens": 340,
      "cacheReadTokens": 800,
      "totalTokens": 1540,
      "acpSessionElapsedMs": 72120
    }],
    "timing": {
      "startedAt": "2026-08-01T10:19:01.000",
      "endedAt": "2026-08-01T10:20:15.120",
      "acpSessionElapsedMs": 72120
    }
  }]
}
```

### 4.3 字段适用矩阵

| 字段 | event | attempt | delivery | 说明 |
|---|---:|---:|---:|---|
| `eventId/eventRevision/eventType` | 必填 | 投影 | 投影 | 在目标月及前一个月范围内进行事件幂等与顺序处理 |
| `userId/workspace` | 必填 | 必填 | 必填 | 原始身份和 workspace |
| `executionId/executionKind` | 必填 | 必填 | 必填 | 生命周期主体 |
| `attemptId` | turn/node-attempt/unit-attempt 必填 | Usage 留存主粒度 | 不适用 | Direct 等于 executionId；Workflow/AUTO 与逻辑 executionId 分离，重试时变化 |
| `attemptIndex` | attempt 必填 | attempt 顺序 | 不适用 | 从 1 开始；Direct 固定为 1，Workflow/AUTO 在同一 executionId 下严格递增 |
| `nodeId` | node-attempt 必填 | 可选 | 不适用 | 逻辑节点 UUID |
| `roundIndex` | Workflow node-attempt 必填 | 可选 | 不适用 | 从 1 开始 |
| `roleName` | 有 resolved profile 时可选 | 可选 | 不适用 | 名称快照，不作唯一键 |
| `unitKind` | AUTO unit 必填 | 可选 | 不适用 | worker/workflow-invocation/merge/acceptance |
| `outcome/terminalReason` | terminal 同时必填 | terminal 更新 | terminal 更新 | 结果与原因 |
| `usage/modelUsages/timing` | attempt terminal | terminal 更新 | 不保存 | attempt 成本事实 |
| `roundCount` | Workflow run terminal | 不保存 | 更新 | Workflow 质量 |
| `passed/acceptanceAttempt/firstPass` | acceptance terminal | 更新 | 不保存 | AUTO 验收质量 |
| `counters` | turn/run/outer terminal | 不保存 | 覆盖更新 | 自动化与恢复统计 |

### 4.4 指标 attempt 与客户端 attemptId 规则

`ml_metric_attempt` 表示可独立计算覆盖、终态、用量和模型质量的一次真实尝试，以 `attempt_id` 作为留存主粒度；`execution_id` 表示稳定的逻辑执行，同一 node/unit 重试产生多个 attempt 行：

| executionKind | 指标 attempt 身份 | 客户端原始 attemptId |
|---|---|---|
| `turn` | executionId 即稳定 attemptId | attemptId 等于 executionId，同一 task 内不变；attemptIndex 固定为 1 |
| `node-attempt` | nodeId 由 run/round/node 派生，稳定不变 | 每次真实尝试独立 attemptId；重试更换 attemptId，nodeId 不变 |
| `unit-attempt` | nodeId 为 DynamicNodeState.uuid，稳定不变 | 每次真实尝试独立 attemptId；重试更换 attemptId，nodeId 不变 |
| `run` | 不写 attempt 表 | 不适用 |
| `outer-run` | 不写 attempt 表 | 不适用 |

Direct 的 executionId/attemptId 均等于 task UUID，attemptIndex 固定为 1；同一 task 多次用户输入仍更新同一个 attempt，usage 与 counters 持续累加。Workflow/AUTO 满足 `attemptId != executionId`。Workflow/AUTO 的 node/unit 首次尝试 `attemptIndex=1`，每次真正重试保持 executionId、生成新 attemptId，并将 attemptIndex 严格加一；同一 attempt 内的 ACP 重连、多次 prompt 和模型切换仍更新同一行，attemptId/attemptIndex 均不得变化。

同一 `(executionId, attemptIndex)` 在一个自然月内只能对应一个 attemptId，同一 `(executionId, attemptId)` 的 attemptIndex 必须恒定。允许因异步上报暂时缺少中间序号，但最终统计应暴露 `attempt_index_gap=1` 作为采集质量；不得由服务端猜测或重排 attemptIndex。

层级查询固定为：delivery_stat 按 run/outer-run executionId 统计整次交付；attempt 按 executionId 分组统计一个逻辑 node/unit 的全部尝试；按 attemptId 查询单次尝试；AUTO unit 的 executionId 即 runUuid，天然归属 outer-run；Workflow node executionId 由 runUuid 派生，天然归属 run。不得用 attemptId 直接替代逻辑 executionId。

`runId/roundId` 不上报。AUTO unit 的 executionId 等于 outer run 的 runUuid，天然归属同一次交付；Workflow 使用 `roundIndex` 表示轮次。

### 4.5 ID 组合约束

协议当前只有五种 `executionKind`；“执行覆盖、交付终局、产物质量、效率成本、自动化、可靠性、模型质量”是七类统计价值维度，不是七种 executionKind。

| sessionMode | executionKind | ID 约束 |
|---|---|---|
| direct | turn | `attemptId == executionId`、`attemptIndex == 1`、node/unit 关联字段为空 |
| workflow | run | attempt 字段为空；executionId 是 run UUID |
| workflow | node-attempt | `attemptId != executionId`、attemptIndex>0、nodeId/roundIndex 必填；当前协议要求 `nodeId == executionId`，nodeId 是类型化查询别名，executionId 是通用生命周期主体 |
| auto | outer-run | attempt 字段为空；executionId 是 outer run UUID |
| auto | unit-attempt | `attemptId != executionId`、attemptIndex>0、nodeId/unitKind 必填；nodeId 与 executionId 均标识该动态单元，协议要求二者值相等 |

`run/outer-run` 是交付层主体，不写 attempt 表；`turn` 的 executionId/attemptId 是 Direct 会话/交付主体，等于 task UUID，同一 task 的所有用户输入属于同一个 attempt。服务端对不在表内的 sessionMode/executionKind 组合返回 `METRICS_FIELD_INVALID`。

### 4.6 UUID 输入格式

`eventId/executionId/attemptId/nodeId/childRunId/failedAttemptId` 出现时必须是 RFC 4122 UUID。API 同时接受 36 位带连字符 canonical 格式和 32 位 simple 格式，大小写不敏感；进入校验、唯一键比较和投影前统一解析并保存为小写 36 位 canonical 格式。禁止按原始字符串比较 UUID；非法长度、字符、variant 或 version 格式返回 `METRICS_FIELD_INVALID`。服务端不限制 UUID v4/v5，但客户端的稳定派生规则仍以采集协议为准。

### 4.7 成功响应

```json
{
  "code": 200,
  "msg": "",
  "ok": true,
  "data": {
    "acceptedCount": 20,
    "insertedEventCount": 18,
    "duplicateEventCount": 2,
    "insertedAttemptCount": 5,
    "updatedAttemptCount": 7,
    "insertedDeliveryCount": 2,
    "updatedDeliveryCount": 4,
    "partition": "p202608",
    "receivedAt": "2026-08-01T10:20:16.125"
  }
}
```

这些 Count 是服务端业务计数，不使用 MySQL affectedRows 的混合语义。

### 4.8 错误响应

```json
{
  "code": "METRICS_FIELD_INVALID",
  "msg": "",
  "ok": false,
  "data": {
    "eventId": "019c...",
    "field": "attemptIndex"
  }
}
```

后端不返回对客文案；`msg` 保持空，前端/调用方按 code 处理。

| code | HTTP | 含义 | 是否重试 |
|---|---:|---|---|
| `METRICS_UNAUTHORIZED` | 403 | API Key 无效 | 否 |
| `METRICS_DISABLED` | 403 | 服务关闭接收 | 否 |
| `METRICS_BATCH_EMPTY` | 400 | events 为空 | 否 |
| `METRICS_BATCH_TOO_LARGE` | 400 | 超过 100 条 | 否 |
| `METRICS_BATCH_CROSS_MONTH` | 400 | 批内 reportedAt 跨月 | 否 |
| `METRICS_REPORTED_AT_OUT_OF_RANGE` | 400 | 与服务端时间偏差超过 24 小时 | 否 |
| `METRICS_FIELD_INVALID` | 400 | 字段、UUID、enum、组合或 Count 非法 | 否 |
| `METRICS_USAGE_SUM_MISMATCH` | 400 | 顶层 usage 不等于非 null model usage 之和 | 否 |
| `METRICS_ATTEMPT_ID_CONFLICT` | 400 | attemptId 已归属其他 execution 或 attemptIndex | 否 |
| `METRICS_ATTEMPT_INDEX_CONFLICT` | 400 | 同 executionId/attemptIndex 对应不同 attemptId | 否 |
| `METRICS_PARTITION_MISSING` | 503 | 目标月分区未创建 | 是 |
| `METRICS_PERSIST_FAILED` | 500 | 事务写入失败 | 是 |

## 5. 数据库设计

四张表只保留 `report_date` 一个 DATE 字段，并直接以其按月 RANGE 分区。

### 5.1 事件事实表 `ml_metric_event`

```sql
CREATE TABLE ml_metric_event (
    report_date DATE NOT NULL,
    event_id VARCHAR(64) NOT NULL,
    execution_id VARCHAR(192) NOT NULL,
    revision_subject_kind VARCHAR(16) NOT NULL COMMENT 'ATTEMPT 或 EXECUTION，服务端计算',
    revision_subject_id VARCHAR(128) NOT NULL COMMENT 'ATTEMPT 取规范化 attempt_id；EXECUTION 取规范化 execution_id',
    event_revision BIGINT UNSIGNED NOT NULL,
    event_type VARCHAR(32) NOT NULL,
    occurred_at DATETIME(3) NOT NULL,
    reported_at DATETIME(3) NOT NULL,
    received_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    user_id VARCHAR(128) NOT NULL,
    workspace VARCHAR(255) NOT NULL,
    task_id VARCHAR(128) NOT NULL,
    session_mode VARCHAR(16) NOT NULL,
    execution_kind VARCHAR(32) NOT NULL,
    attempt_id VARCHAR(128) NULL,
    attempt_index INT UNSIGNED NULL,
    raw_payload JSON NOT NULL,
    PRIMARY KEY (report_date, event_id),
    UNIQUE KEY uk_event_revision (report_date, revision_subject_kind, revision_subject_id, event_revision),
    KEY idx_event_timeline (report_date, execution_id, attempt_index, event_revision),
    KEY idx_event_attempt (report_date, attempt_id, event_revision),
    KEY idx_event_type (report_date, event_type, occurred_at)
) ENGINE=InnoDB
PARTITION BY RANGE COLUMNS(report_date) (
    PARTITION p202608 VALUES LESS THAN ('2026-09-01'),
    PARTITION p202609 VALUES LESS THAN ('2026-10-01'),
    PARTITION pmax VALUES LESS THAN (MAXVALUE)
);
```

用途：在目标月及前一个月范围内进行 eventId 幂等、保存完整时间线。事实只插入，不更新。本期不处理相同 eventId 携带不同 payload 的异常场景。

`revision_subject_kind/revision_subject_id` 是服务端正式计算字段，不接受客户端同名输入：`turn/node-attempt/unit-attempt` 固定为 `ATTEMPT + attemptId`，`run/outer-run` 固定为 `EXECUTION + executionId`。服务端先完成 UUID 规范化再计算 revision 主体；同一 attempt 的 started/terminal/acceptance 使用同一 revision 序列，不同重试 attempt 的 revision 互不冲突。

### 5.2 attempt 基础表 `ml_metric_attempt`

```sql
CREATE TABLE ml_metric_attempt (
    report_date DATE NOT NULL,
    execution_id VARCHAR(192) NOT NULL,
    attempt_id VARCHAR(128) NOT NULL COMMENT 'Usage 留存主粒度；Direct 等于 execution_id，Workflow/AUTO 重试时独立变化',
    attempt_index INT UNSIGNED NOT NULL COMMENT '同一节点内从 1 严格递增；Direct 固定为 1',
    execution_kind VARCHAR(32) NOT NULL,
    session_mode VARCHAR(16) NOT NULL,
    user_id VARCHAR(128) NOT NULL,
    workspace VARCHAR(255) NOT NULL,
    client_version VARCHAR(64) NULL,
    task_id VARCHAR(128) NOT NULL,
    node_id VARCHAR(128) NULL,
    round_index INT UNSIGNED NULL,
    role_name VARCHAR(255) NULL,
    unit_kind VARCHAR(32) NULL,
    child_run_id VARCHAR(128) NULL,
    state VARCHAR(16) NOT NULL,
    outcome VARCHAR(16) NULL,
    terminal_reason VARCHAR(32) NULL,
    terminal_reason_code VARCHAR(128) NULL,
    started_at DATETIME(3) NULL,
    ended_at DATETIME(3) NULL,
    final_provider VARCHAR(64) NULL,
    final_model VARCHAR(128) NULL,
    input_tokens BIGINT UNSIGNED NULL,
    output_tokens BIGINT UNSIGNED NULL,
    cache_read_tokens BIGINT UNSIGNED NULL,
    total_tokens BIGINT UNSIGNED NULL,
    acp_session_elapsed_ms BIGINT UNSIGNED NULL,
    model_usages JSON NULL,
    acceptance_attempt INT UNSIGNED NULL,
    acceptance_passed TINYINT(1) NULL,
    first_pass TINYINT(1) NULL,
    last_event_id VARCHAR(64) NOT NULL,
    last_event_revision BIGINT UNSIGNED NOT NULL,
    start_event_missing TINYINT(1) NOT NULL DEFAULT 0,
    projection_conflict TINYINT(1) NOT NULL DEFAULT 0,
    collection_state_recovered TINYINT(1) NULL,
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3)
                                  ON UPDATE CURRENT_TIMESTAMP(3),
    CONSTRAINT chk_attempt_index_positive CHECK (attempt_index > 0),
    CONSTRAINT chk_attempt_state CHECK (state IN ('running', 'paused', 'terminal')),
    PRIMARY KEY (report_date, attempt_id),
    UNIQUE KEY uk_attempt_execution_index (report_date, execution_id, attempt_index),
    KEY idx_attempt_execution (report_date, execution_id, attempt_index, state, outcome),
    KEY idx_attempt_node (report_date, node_id, outcome),
    KEY idx_attempt_role (report_date, role_name, outcome),
    KEY idx_attempt_model (report_date, final_provider, final_model),
    KEY idx_attempt_unit (report_date, unit_kind, outcome),
    UNIQUE KEY uk_acceptance_parent_attempt
        (report_date, execution_id, acceptance_attempt)
) ENGINE=InnoDB
PARTITION BY RANGE COLUMNS(report_date) (
    PARTITION p202608 VALUES LESS THAN ('2026-09-01'),
    PARTITION p202609 VALUES LESS THAN ('2026-10-01'),
    PARTITION pmax VALUES LESS THAN (MAXVALUE)
);
```

一行只表示一个指标 attempt，并由 UUID `attempt_id` 唯一留存。Direct 的 attemptId 与 executionId 相等；Workflow/AUTO 同一逻辑 node/unit 的多次重试共享 executionId、各自生成 attemptId。同一 attempt 内部的 ACP/provider 重连、多次 prompt 或模型切换继续更新当前记录。

`uk_acceptance_parent_attempt` 只约束 `acceptance_attempt` 非 NULL 的 acceptance 行；MySQL 允许普通 attempt 的 NULL 组合重复。同一 outer-run 的同一 acceptanceAttempt 只能对应一个 acceptance attempt。

### 5.3 logical execution 投影表 `ml_metric_logical_execution`

```sql
CREATE TABLE ml_metric_logical_execution (
    report_date DATE NOT NULL,
    execution_id VARCHAR(192) NOT NULL,
    execution_kind VARCHAR(32) NOT NULL COMMENT 'turn/node-attempt/unit-attempt',
    session_mode VARCHAR(16) NOT NULL,
    user_id VARCHAR(128) NOT NULL,
    workspace VARCHAR(255) NOT NULL,
    task_id VARCHAR(128) NOT NULL,
    node_id VARCHAR(128) NULL,
    round_index INT UNSIGNED NULL,
    role_name VARCHAR(255) NULL,
    unit_kind VARCHAR(32) NULL,
    attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    terminal_attempt_count INT UNSIGNED NOT NULL DEFAULT 0,
    latest_attempt_index INT UNSIGNED NULL,
    latest_attempt_id VARCHAR(128) NULL,
    final_attempt_index INT UNSIGNED NULL,
    final_attempt_id VARCHAR(128) NULL,
    final_outcome VARCHAR(16) NULL,
    final_terminal_reason VARCHAR(32) NULL,
    state VARCHAR(16) NOT NULL,
    attempt_index_gap TINYINT(1) NOT NULL DEFAULT 0,
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3)
                                  ON UPDATE CURRENT_TIMESTAMP(3),
    CONSTRAINT chk_logical_state CHECK (state IN ('running', 'paused', 'terminal')),
    PRIMARY KEY (report_date, execution_id),
    KEY idx_logical_node (report_date, node_id, round_index, final_outcome),
    KEY idx_logical_unit (report_date, unit_kind, final_outcome),
    KEY idx_logical_role (report_date, role_name, final_outcome)
) ENGINE=InnoDB
PARTITION BY RANGE COLUMNS(report_date) (
    PARTITION p202608 VALUES LESS THAN ('2026-09-01'),
    PARTITION p202609 VALUES LESS THAN ('2026-10-01'),
    PARTITION pmax VALUES LESS THAN (MAXVALUE)
);
```

该表完全由服务端根据 attempt 表投影，客户端不直接写逻辑终态。`latestAttempt` 是已观察到 attemptIndex 最大的 attempt；`finalAttempt` 仅在该最大序号 attempt 已 terminal 时成立。逻辑 execution 的最终结果固定取 `finalAttempt.outcome/terminalReason`，不得采用“任一 attempt 成功即成功”或按事件到达顺序取最后一条。若最大序号 attempt 尚未终态，则 logical execution 为 running，最终结果为 NULL；后续出现更大 attemptIndex 时重新计算。Direct 只有 attemptIndex=1，因此其 logical execution 与 turn attempt 结果一致。

### 5.4 交付统计表 `ml_metric_delivery_stat`

```sql
CREATE TABLE ml_metric_delivery_stat (
    report_date DATE NOT NULL,
    execution_id VARCHAR(192) NOT NULL,
    execution_kind VARCHAR(32) NOT NULL COMMENT 'turn/run/outer-run',
    session_mode VARCHAR(16) NOT NULL,
    user_id VARCHAR(128) NOT NULL,
    workspace VARCHAR(255) NOT NULL,
    client_version VARCHAR(64) NULL,
    task_id VARCHAR(128) NOT NULL,
    state VARCHAR(16) NOT NULL,
    outcome VARCHAR(16) NULL,
    terminal_reason VARCHAR(32) NULL,
    terminal_reason_code VARCHAR(128) NULL,
    started_at DATETIME(3) NULL,
    ended_at DATETIME(3) NULL,
    round_count INT UNSIGNED NULL,
    pause_count INT UNSIGNED NULL,
    resume_count INT UNSIGNED NULL,
    permission_request_count INT UNSIGNED NULL,
    elicitation_count INT UNSIGNED NULL,
    manual_continue_count INT UNSIGNED NULL,
    follow_up_count INT UNSIGNED NULL,
    last_event_id VARCHAR(64) NOT NULL,
    last_event_revision BIGINT UNSIGNED NOT NULL,
    start_event_missing TINYINT(1) NOT NULL DEFAULT 0,
    projection_conflict TINYINT(1) NOT NULL DEFAULT 0,
    collection_state_recovered TINYINT(1) NULL,
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3)
                                  ON UPDATE CURRENT_TIMESTAMP(3),
    CONSTRAINT chk_delivery_state CHECK (state IN ('running', 'paused', 'terminal')),
    PRIMARY KEY (report_date, execution_id),
    KEY idx_delivery_mode (report_date, session_mode, execution_kind, outcome),
    KEY idx_delivery_user (report_date, user_id, workspace, task_id),
    KEY idx_delivery_reason (report_date, terminal_reason, outcome)
) ENGINE=InnoDB
PARTITION BY RANGE COLUMNS(report_date) (
    PARTITION p202608 VALUES LESS THAN ('2026-09-01'),
    PARTITION p202609 VALUES LESS THAN ('2026-10-01'),
    PARTITION pmax VALUES LESS THAN (MAXVALUE)
);
```

Count 只存在于这张表；started/paused 状态时 Count 为 NULL，terminal 时六个 Count 全量覆盖。Direct turn 同时有一条 attempt 基础记录和一条 delivery stat，前者用于模型/usage，后者用于自动化和交付终局。

`failedAttemptId` 本期暂不处理。客户端可以继续按采集协议上报，事件事实表通过 `raw_payload` 原样留存；服务端只执行字段级 UUID 格式校验，不校验其 attempt 是否存在、归属或终态，不写入 delivery 投影，不做乱序回填，也不用于失败归因和统计。delivery 的结果只由自身 `outcome/terminalReason` 决定。

## 6. 数据写入与更新

### 6.1 整批处理流程

```text
validate api key, fields, enums, UUIDs, time skew, same report month
target = month(events[0].reportedAt)
previous = previousMonth(target)

begin transaction
for event in request order:
  normalize all UUID fields
  revisionSubjectKind = ATTEMPT for turn/node-attempt/unit-attempt, otherwise EXECUTION
  revisionSubjectId = normalized attemptId for ATTEMPT, otherwise normalized executionId
  query eventId and revisionSubjectId+revision in target/previous event partitions
  if duplicate: skip projection
  insert ml_metric_event into target partition

  if executionKind in [turn, node-attempt, unit-attempt]:
      locate metric attempt in target/previous by attemptId
      verify the stored executionId matches the event executionId
      insert or project ml_metric_attempt
      recompute/project ml_metric_logical_execution by executionId

  if executionKind in [turn, run, outer-run]:
      locate delivery in target/previous by executionId
      insert or project ml_metric_delivery_stat
commit
```

批内不考虑并发批次锁顺序优化；数据库死锁按普通事务失败返回 500。幂等查询范围仅为 target/previous，不提供跨全部历史分区的 eventId 唯一性。
单事件投影的完整决策树见 §6.10，logical execution 重算算法见 §6.8，端到端事件序列到表快照的对照示例见 §6.9。

### 6.2 幂等与 revision

| 情况 | 处理 |
|---|---|
| eventId 已存在 | duplicate success，跳过后续投影 |
| revisionSubjectId + revision 已存在 | duplicate success，跳过后续投影；本期不比较 payload 差异 |
| attemptId 已存在但 executionId 不同 | 整批 `METRICS_ATTEMPT_ID_CONFLICT` |
| revision 小于汇总表 last revision | 事实保留，汇总不回退 |
| revision 大于 last revision | 按事件类型更新汇总 |

revision 必须是大于等于 1 的正整数。允许第一条事件的 revision 大于 1，允许缺口、乱序和 terminal-first；服务端不补号、不重排，也不要求连续。revision 不表示全局顺序，也不在同一 logical execution 的多个 attempts 之间共享。同一 attempt 的 started/completed/acceptance 共享 revision 主体，不同 attempt 分别计数。服务端唯一约束正式定义为 `(report_date, revision_subject_kind, revision_subject_id, event_revision)`；revision 主体字段不由客户端传入。目标月及前一个月范围内 eventId、attemptId、executionId+attemptIndex 的唯一性由查询与数据库约束共同保证。

### 6.3 不可变字段矩阵

同一投影主体首次得到非 NULL 值后，下列字段不得被后续事件修改。违反身份或归属约束时返回 `METRICS_FIELD_INVALID` 并整批回滚；允许迟到 started 补齐的展示快照字段遵循 6.7。

| 主体 | 不可变字段 |
|---|---|
| 所有 event | `eventId` 对应的 `revisionSubjectKind/revisionSubjectId` |
| attempt | `attemptId`、`executionId`、`attemptIndex`、`executionKind`、`sessionMode`、`userId`、`workspace` |
| Workflow node attempt | `nodeId`、`roundIndex` |
| AUTO unit attempt | `nodeId`、`unitKind`；workflow-invocation 的 `childRunId` 首次非 NULL 后不可修改 |
| delivery | `executionId`、`executionKind`、`sessionMode`、`userId`、`workspace` |
| logical execution | 从其 attempts 投影得到的 `executionId`、`executionKind`、`sessionMode`、归属字段 |

`clientVersion`、`roleName`、`startedAt` 和 `collectionStateRecovered` 是开始快照字段：仅当现值为 NULL 时允许迟到 started 补齐，已有非 NULL 值不覆盖并设置 `projection_conflict=1`。

### 6.4 state 枚举与不可逆状态规则

三张投影表的 `state` 统一只允许：

| state | 含义 |
|---|---|
| `running` | 已开始或因 missing-start 推断为尚未终态 |
| `paused` | 当前已暂停且尚未终态 |
| `terminal` | 已收到合法 `execution.completed`，不可再恢复运行 |

状态转换只允许 `running -> paused -> running` 和任意非 terminal 状态 `-> terminal`。`terminal` 是吸收态：无论后到事件 revision 更高或更低，`started/paused/resumed/intervention.requested` 都只能写入事实表，不得修改 state、outcome、终态时间、Usage、counters 或其他终态投影。更高 revision 的合法 `execution.completed` 可作为新的终态快照覆盖 terminal 可变结果字段，但不得改变不可变身份与归属字段。

### 6.5 生命周期更新矩阵

| eventType | event 表 | attempt 表 | delivery stat 表 |
|---|---|---|---|
| `execution.started` | 插入 | attempt 类型：插入 `state=running` 基础行 | delivery 类型：插入 `state=running` 行 |
| `execution.paused` | 插入 | turn 可更新 state=paused | delivery 更新 state=paused |
| `execution.resumed` | 插入 | turn 可恢复 `state=running` | delivery 恢复 `state=running` |
| `intervention.requested` | 插入 | 不增 Count | 不增 Count，只更新 revision |
| `execution.completed` | 插入 | attempt 类型覆盖 terminal、usage、model | delivery 类型覆盖 terminal、quality、六个 Count |
| `acceptance.completed` | 插入 | acceptance attempt 更新 passed/attempt/firstPass | 不直接修改 outer outcome |

### 6.6 terminal 更新规则

- `outcome` 和 `terminalReason` 必须同时存在且组合合法。
- terminal 是 attempt usage/model 和 delivery Count 的权威快照。
- usage、modelUsages、Count 使用赋值，绝不 `+=`。
- Usage token 按字段独立求和。对 `inputTokens/outputTokens/cacheReadTokens/totalTokens` 中的每个字段：若 modelUsages 至少有一个非 null 值，顶层 usage 对应字段必须等于所有非 null 值之和；若全部为 null，顶层字段必须为 null。null 表示未知，不按 0 处理；空数组等价于没有已知分段。不得强制 `totalTokens=inputTokens+outputTokens`，因为 provider 口径可能不同。`timing.acpSessionElapsedMs` 是 attempt 总净时间，独立于 token sum；modelUsages 分段 elapsed 若存在可另行用于模型时间统计，但不要求其和等于 attempt 墙钟或净时间。
- 服务端保存客户端上报的 attempt 原始 Usage，但所有跨 attempt、logical execution、delivery、用户、workspace、模型统计均由服务端查询计算，客户端不得上报聚合指标。
- terminal counters 作为客户端终态快照直接覆盖写入；本期不执行 Count mismatch 重算、校验或质量标记。
- terminal 后所有非 terminal 事件均遵循 6.4 的不可逆状态规则，不因 revision 更高而回退。
- acceptance 事件必须满足 `firstPass == (passed == true AND acceptanceAttempt == 1)`；`acceptanceAttempt>=1`。同一 outer-run 的相同 acceptanceAttempt 重复上报必须内容一致，不同 attempt 按序递增；不一致返回 `METRICS_FIELD_INVALID`。

### 6.7 missing-start

当 target/previous 都找不到基础或统计行：

- terminal-first：插入 completed，`start_event_missing=1`。
- paused/intervention-first：插入可表达状态，`start_event_missing=1`。
- 后到的低 revision `execution.started` 只允许补充此前为 NULL 的不可变开始快照：`started_at`、`client_version`、`role_name`、`node_id`、`round_index`、`unit_kind`、`child_run_id` 和 `collection_state_recovered`。已有非 NULL 值必须相同，否则记事实但不覆盖，并标记投影冲突。
- 低 revision started 禁止修改：所有 ID 及归属字段、attemptIndex、state、outcome、terminalReason/Code、endedAt、Usage/modelUsages、acceptance 结果、counters、lastEventId 和 lastEventRevision。paused/resumed/intervention 低 revision 也只能保留事实，不修改任何投影字段。
- 不自动清理永久 running，不设置 stale 阈值。
 
### 6.8 logical execution 重算算法
 
每当 attempt 表发生 INSERT 或 terminal 字段 UPDATE 时，服务端必须按 `executionId` 重算 `ml_metric_logical_execution` 投影。以下是完整重算步骤，不能跳过任何一步。
 
```text
function apply_logical_execution_recompute(executionId, report_date):
    # 第 1 步：读取该 executionId 下的所有 attempt 行（target/previous 分区）
    attempts = SELECT * FROM ml_metric_attempt
               WHERE report_date IN (target, previous)
                 AND execution_id = executionId
    if attempts is empty:
        return
    # 第 2 步：计算聚合值
    attempt_count          = COUNT(attempts)
    terminal_attempt_count = COUNT(attempts WHERE state = 'terminal')
    latest_attempt         = MAX(attempts BY attempt_index)
    latest_attempt_index   = latest_attempt.attempt_index
    latest_attempt_id      = latest_attempt.attempt_id
    # final_attempt：最大 attemptIndex 且 state=terminal
    terminal_at_max_index = attempts
        WHERE attempt_index == latest_attempt_index AND state == 'terminal'
    if terminal_at_max_index is not empty:
        final_attempt_index   = latest_attempt_index
        final_attempt_id      = latest_attempt.attempt_id
        final_outcome         = latest_attempt.outcome
        final_terminal_reason = latest_attempt.terminal_reason
        logical_state         = 'terminal'
    else:
        final_attempt_index   = NULL
        final_attempt_id      = NULL
        final_outcome         = NULL
        final_terminal_reason = NULL
        logical_state         = latest_attempt.state
    # attempt_index_gap：序号是否有缺口（如 1,3 缺 2）
    sorted_indices = SORT(attempts.attempt_index ASC)
    expected       = 1
    has_gap        = FALSE
    for idx in sorted_indices:
        if idx != expected:
            has_gap = TRUE
            break
        expected += 1
    # 第 3 步：身份与归属字段（从任一 attempt 继承，理论上全一致）
    sample         = attempts[0]
    execution_kind = sample.execution_kind
    session_mode   = sample.session_mode
    node_id        = sample.node_id
    round_index    = sample.round_index
    role_name      = sample.role_name
    unit_kind      = sample.unit_kind
    # 第 4 步：UPSERT 投影行
    if ml_metric_logical_execution row exists for (report_date, executionId):
        UPDATE row SET
            attempt_count          = attempt_count,
            terminal_attempt_count = terminal_attempt_count,
            latest_attempt_index   = latest_attempt_index,
            latest_attempt_id      = latest_attempt_id,
            final_attempt_index    = final_attempt_index,
            final_attempt_id       = final_attempt_id,
            final_outcome          = final_outcome,
            final_terminal_reason  = final_terminal_reason,
            state                  = logical_state,
            attempt_index_gap      = has_gap ? 1 : 0,
            updated_at             = NOW(3)
        WHERE report_date = row.report_date AND execution_id = executionId
    else:
        INSERT row with all computed fields
    # 不可变字段（execution_kind, session_mode, parent, node, unit 等）
    # 首次写入后不修改；若发现不一致，整批 METRICS_FIELD_INVALID
```
 
关键约束：`final_outcome` 只在最大 attemptIndex 的 attempt 已 terminal 时才有值；否则为 NULL，logical execution 不是 terminal。后续出现更大 attemptIndex 的 terminal attempt 时重新计算并覆盖。Direct turn 只有 attemptIndex=1，因此其 logical execution 与 turn attempt 结果一致。
 
### 6.9 端到端 trace 示例
 
以下三个示例用固定数据展示事件序列到四张表快照的完整映射。每行标注该步完成后各表的最终状态。`event` 列只写关键字段；`attempt`、`logical`、`delivery` 列写该步 INSERT/UPDATE 后的结果。
 
**示例 1：Direct turn 成功**
 
| 步 | event 关键字段 | ml_metric_attempt | ml_metric_logical_execution | ml_metric_delivery_stat |
|---:|---|---|---|---|
| 1 | `started` rev=1, kind=turn, exec=A, attemptId=A, attemptIndex=1 | INSERT running, started_at=T1 | INSERT running, latest=1/A, final=NULL | INSERT running, started_at=T1 |
| 2 | `completed` rev=2, outcome=completed, usage tokens, counters | UPDATE terminal, outcome=completed, tokens, model_usages | UPDATE terminal, final=1/A, final_outcome=completed | UPDATE terminal, outcome=completed, counters 覆盖 |
| 3 | `started` rev=3, kind=turn, exec=A, attemptId=A, attemptIndex=1 | UPDATE running（继续同一 attempt），started_at 保留 | UPDATE running, latest=1/A, final=NULL | UPDATE running |
| 4 | `completed` rev=4, outcome=completed, usage 累计, counters 累计 | UPDATE terminal, tokens 累计, model_usages 累计 | UPDATE terminal, final=1/A, final_outcome=completed | UPDATE terminal, counters 累计覆盖 |
 
注意 Direct turn 同时命中 attempt 和 delivery 两个投影分支。attempt 保存 usage/model，delivery 保存 counters，两表独立。executionId/attemptId 等于 task UUID；同一 task 多次输入继续更新同一 attempt，usage 和 counters 按累计快照覆盖。
 
**示例 2：Workflow node 重试（attempt 1 失败，attempt 2 成功）**
 
node 的 executionId 由 `run_uuid + round + node` 稳定派生（客户端 v5），重试不变；attemptId 每次重试生成新 UUID。
 
| 步 | event 关键字段 | ml_metric_attempt | ml_metric_logical_execution | ml_metric_delivery_stat |
|---:|---|---|---|---|
| 1 | `started` rev=1, kind=node-attempt, exec=NODE-EXEC, attemptId=ATT-1, attemptIndex=1, parent=RUN-UUID | INSERT running | INSERT running, latest=1/ATT-1, final=NULL | node 不写 delivery |
| 2 | `completed` rev=2, outcome=failure, reason=provider-error | UPDATE terminal, outcome=failure | UPDATE terminal, final=1/ATT-1, final_outcome=failure | node 不写 delivery |
| 3 | `started` rev=1, kind=node-attempt, exec=NODE-EXEC, attemptId=ATT-2, attemptIndex=2, parent=RUN-UUID | INSERT running（新行） | recompute: latest=2/ATT-2, final=NULL | node 不写 delivery |
| 4 | `completed` rev=2, outcome=success, reason=completed | UPDATE terminal, outcome=success | recompute: final=2/ATT-2, final_outcome=success, state=terminal | node 不写 delivery |
 
关键点：步骤 3 重算后 `latest_attempt_index` 变为 2，但 `final_outcome` 回退为 NULL（ATT-2 尚未 terminal），logical execution 从 terminal 回到 running。步骤 4 重算后 final 结果取 ATT-2（最大 attemptIndex 的 terminal attempt），不再受 ATT-1 failure 影响。这正是"重试恢复率"指标的数据来源。
 
**示例 3：AUTO acceptance 首次拒绝，修复后通过**
 
| 步 | event 关键字段 | ml_metric_attempt | ml_metric_logical_execution | ml_metric_delivery_stat |
|---:|---|---|---|---|
| 1 | `started` rev=1, kind=outer-run, exec=OUTER-UUID | outer-run 不写 attempt | outer-run 不写 logical | INSERT running |
| 2 | `started` rev=1, kind=unit-attempt, exec=UNIT-EXEC, unitKind=acceptance, attemptId=ATT-1, attemptIndex=1, parent=OUTER-UUID | INSERT running | INSERT running, latest=1/ATT-1 | unit 不写 delivery |
| 3 | `acceptance.completed` rev=2, passed=false, attempt=1, firstPass=false | UPDATE acceptance_passed=false, attempt=1, first_pass=false | recompute: 若 ATT-1 已 terminal 则 final=1/ATT-1 | unit 不写 delivery |
| 4 | `started` rev=1, kind=unit-attempt, exec=UNIT-EXEC, attemptId=ATT-2, attemptIndex=2, parent=OUTER-UUID | INSERT running | recompute: latest=2/ATT-2 | unit 不写 delivery |
| 5 | `completed` rev=2, outcome=success + `acceptance.completed` rev=3, passed=true, attempt=2, firstPass=false | UPDATE terminal + acceptance_passed=true, attempt=2 | recompute: final=2/ATT-2, final_outcome=success | unit 不写 delivery |
| 6 | `completed` rev=3, kind=outer-run, outcome=success, reason=completed | outer-run 不写 attempt | outer-run 不写 logical | UPDATE terminal, outcome=success |
 
关键点：`firstPass` 只在 `passed=true AND acceptanceAttempt=1` 时为 true；本例首次拒绝所以 firstPass 始终为 false。`acceptance_attempt` 在 outer 级别递增，由客户端维护，服务端只做幂等校验不重算。`uk_acceptance_parent_attempt` 唯一约束防止同一 outer-run 同一 acceptanceAttempt 重复写入不同 attempt。`failedAttemptId` 在 outer-run completed 时若 outcome=failure 则指向决定终局的 unit attempt；本例 success 所以不携带。
 
### 6.10 单事件投影决策流程
 
以下伪代码是 §6.1 循环体中 `apply_attempt_projection` 和 `apply_delivery_projection` 的完整展开，将 §6.2—§6.7 的幂等、不可变字段、state 吸收态、terminal 覆盖和 missing-start 合并为单一决策树。实现时可直接按此结构组织代码。
 
```text
function apply_attempt_projection(event, existing_attempt_row):
    # 第 0 步：定位或初始化
    if existing_attempt_row is None:
        # missing-start（6.7）：terminal-first / paused-first / intervention-first
        insert attempt row with:
            state = terminal if eventType == execution.completed
                    else paused if eventType == execution.paused
                    else running
            start_event_missing = 1
            所有 nullable 字段 = NULL
            last_event_id       = event.eventId
            last_event_revision = event.eventRevision
        if eventType == execution.completed:
            apply_attempt_terminal_fields(row, event)
        return
    row = existing_attempt_row
    # 第 1 步：revision 比较
    if event.eventRevision < row.last_event_revision:
        # 迟到低 revision 事件：只允许 started 补 NULL 快照（6.7）
        if eventType == execution.started:
            fill_nullable_snapshot_if_null(row, event)
            # 补充 started_at, client_version, role_name, node_id,
            # round_index, unit_kind, child_run_id,
            # collection_state_recovered
            # 已有非 NULL 值且不同则标记 projection_conflict=1，不覆盖
        # 其余低 revision 事件：仅写事实表，不修改任何投影字段
        return
    # eventRevision >= last_event_revision：允许投影
    # 第 2 步：不可变字段校验（6.3）
    verify_immutable_fields(row, event)
    # attemptId, executionId, attemptIndex, executionKind, sessionMode,
    # userId, workspace 不得变化
    # Workflow node: nodeId, roundIndex
    # AUTO unit: nodeId, unitKind, childRunId(首次非NULL后)
    # 违反时 abort batch METRICS_FIELD_INVALID
    # 第 3 步：state 吸收态判断（6.4）
    if row.state == 'terminal':
        if eventType != execution.completed:
            # terminal 是吸收态：非 terminal 事件不修改任何投影字段
            return
        # 高 revision 的 completed 允许覆盖 terminal 可变结果字段：
        # outcome, terminalReason, terminalReasonCode, endedAt,
        # usage/model/timing（attempt）, counters（delivery）
        # 但不得修改不可变身份字段（已在第 2 步校验）
    # 第 4 步：按 eventType 应用投影
    match eventType:
      case execution.started:
          if row.state == 'running' and row.start_event_missing == 1:
              fill_nullable_snapshot_if_null(row, event)
          elif row is newly inserted:
              set state = 'running'
              copy started snapshot fields
          # 已有非 missing-start 的 running 行：revision 相等时重复幂等，
          # revision 更高时仅更新 last_event_*
      case execution.paused:
          if row.executionKind == 'turn':
              update state = 'paused'
          # node/unit attempt 不维护 paused state，只写事实
      case execution.resumed:
          if row.executionKind == 'turn' and row.state == 'paused':
              update state = 'running'
      case intervention.requested:
          # 不增 Count，不修改 state；Count 由客户端 terminal 快照权威提供（6.6）
          pass
      case execution.completed:
          apply_attempt_terminal_fields(row, event)
      case acceptance.completed:
          update acceptance_passed = event.passed
          update acceptance_attempt = event.acceptanceAttempt
          update first_pass = event.passed AND event.acceptanceAttempt == 1
    # 第 5 步：更新 last_event 指针
    row.last_event_id       = event.eventId
    row.last_event_revision = event.eventRevision
```
 
```text
function apply_attempt_terminal_fields(row, event):
    row.state                = 'terminal'
    row.outcome              = event.outcome
    row.terminal_reason      = event.terminalReason
    row.terminal_reason_code = event.terminalReasonCode
    row.ended_at             = event.timing.endedAt
    # usage / model / timing：覆盖赋值，绝不 +=
    row.final_provider       = event.provider
    row.final_model          = event.model
    row.input_tokens         = event.usage.inputTokens
    row.output_tokens        = event.usage.outputTokens
    row.cache_read_tokens    = event.usage.cacheReadTokens
    row.total_tokens         = event.usage.totalTokens
    row.acp_session_elapsed_ms = event.timing.acpSessionElapsedMs
    row.model_usages         = event.modelUsages
```
 
```text
function apply_delivery_projection(event, existing_delivery_row):
    if existing_delivery_row is None:
        # missing-start
        insert delivery row with state 按 eventType（同 attempt 逻辑）
        start_event_missing = 1
        if eventType == execution.completed:
            apply_delivery_terminal_fields(row, event)
        return
    row = existing_delivery_row
    # revision 比较（同 attempt 第 1 步）
    if event.eventRevision < row.last_event_revision:
        if eventType == execution.started:
            fill_nullable_snapshot_if_null(row, event)
            # 补充 started_at, client_version, collection_state_recovered
        return
    # 不可变字段校验（6.3）：executionId, executionKind, sessionMode, userId, workspace
    verify_immutable_fields(row, event)
    # state 吸收态（同 attempt 第 3 步）
    if row.state == 'terminal' and eventType != execution.completed:
        return
    match eventType:
      case execution.started:  set state = 'running'
      case execution.paused:   set state = 'paused'
      case execution.resumed:  set state = 'running'
      case intervention.requested: pass
      case execution.completed: apply_delivery_terminal_fields(row, event)
    row.last_event_id       = event.eventId
    row.last_event_revision = event.eventRevision
```
 
```text
function apply_delivery_terminal_fields(row, event):
    row.state                  = 'terminal'
    row.outcome                = event.outcome
    row.terminal_reason        = event.terminalReason
    row.terminal_reason_code   = event.terminalReasonCode
    row.ended_at               = event.timing.endedAt
    # Count：覆盖赋值，绝不 +=
    row.pause_count              = event.counters.pauseCount
    row.resume_count             = event.counters.resumeCount
    row.permission_request_count = event.counters.permissionRequestCount
    row.elicitation_count        = event.counters.elicitationCount
    row.manual_continue_count    = event.counters.manualContinueCount
    row.follow_up_count          = event.counters.followUpCount
    # roundCount 是 Workflow 质量字段，仅 run terminal 携带
    if event.executionKind == 'run':
        row.round_count = event.roundCount
```
 
`fill_nullable_snapshot_if_null(row, event)` 只在目标字段为 NULL 时赋值，已有非 NULL 且不同则置 `projection_conflict=1`。任何情况下不修改不可变身份字段、attemptIndex、state、outcome、Usage、counters。

## 7. 指标详细统计口径

所有业务报表按首次上报 `report_date` 归属，使用左闭右开 `[startDate,endDate)`。SQL 必须包含 `report_date` 范围以触发月分区裁剪。

### 7.1 统计维度表

| 价值维度 | 指标 | 来源表 | 粒度 | 过滤/分母 | 公式 |
|---|---|---|---|---|---|
| 执行覆盖 | Direct turn 数 | attempt | executionId | kind=turn | COUNT(*) |
| 执行覆盖 | Workflow run 数 | delivery_stat | executionId | kind=run | COUNT(*) |
| 执行覆盖 | Workflow node attempt 数 | attempt | attemptId | kind=node-attempt | COUNT(*) |
| 执行覆盖 | Workflow logical node execution 数 | logical_execution | executionId | kind=node-attempt | COUNT(*) |
| 执行覆盖 | AUTO outer run 数 | delivery_stat | executionId | kind=outer-run | COUNT(*) |
| 执行覆盖 | AUTO unit attempt 数 | attempt | attemptId | kind=unit-attempt | COUNT(*) |
| 执行覆盖 | AUTO logical unit execution 数 | logical_execution | executionId | kind=unit-attempt | COUNT(*) |
| 交付终局 | Direct 成功完成率 | delivery_stat | turn | terminal | completed / (completed+failed+cancelled) |
| 交付终局 | Workflow 成功率 | delivery_stat | run | terminal | success / (success+failure+killed) |
| 交付终局 | AUTO 成功率 | delivery_stat | outer-run | terminal | success / (success+failure+killed) |
| 产物质量 | Workflow 首轮交付率 | delivery_stat | run | success run | roundCount=1 / success run |
| 产物质量 | AUTO acceptance 首过率 | attempt | acceptance attempt | acceptanceAttempt=1 | passed / first acceptance |
| 效率成本 | token 总量 | attempt | attemptId | token 非 null | SUM(token) |
| 效率成本 | ACP 平均时间 | attempt | attemptId | elapsed 非 null | SUM(elapsed)/known attempts |
| 自动化 | 无干预执行率 | delivery_stat | run/outer | terminal | 三个干预 Count 全 0 / terminal |
| 自动化 | 全自动交付率 | delivery_stat | run/outer | terminal | success 且三个干预 Count 全 0 / terminal |
| 可靠性 | pause 率 | delivery_stat | run/outer | terminal | pauseCount>0 / terminal |
| 可靠性 | 恢复率 | delivery_stat | run/outer | pauseCount>0 | resumeCount>0 / paused |
| 可靠性 | node/leaf 故障率 | attempt | attemptId | terminal attempt | failure / terminal attempt |
| 可靠性 | logical execution 最终失败率 | logical_execution | executionId | final attempt 已 terminal | final failure / final terminal execution |
| 重试质量 | attempt 重试率 | logical_execution | executionId | attempt_count 已知 | attempt_count>1 / execution 数 |
| 重试质量 | 重试恢复率 | logical_execution | executionId | attempt_count>1 且 final terminal | final success / retried final terminal execution |
| 重试质量 | 首次成功率 | attempt/logical_execution | executionId | attemptIndex=1 已 terminal | attempt 1 success / attempt 1 terminal |
| 模型质量 | 模型参与尝试数 | attempt JSON | attemptId | provider+model | COUNT DISTINCT attemptId |
| 模型质量 | 角色成功率 | attempt | attemptId | roleName 非空 | success / terminal attempt |

### 7.2 执行覆盖

- started 总数和 terminal 总数必须同时展示。
- 永久 running 保留在 started/未终态，不进入成功率分母。
- 用户覆盖数：`COUNT(DISTINCT user_id)`。
- workspace 覆盖数：`COUNT(DISTINCT workspace)`。
- attempt 指标回答“执行了多少次真实尝试”；logical execution 指标回答“有多少个逻辑 node/unit”。两者必须分别命名和展示，不得把 attempt success rate 标成 execution success rate。

### 7.3 交付终局

| 模式 | 成功 outcome | 异常 outcome | 分母 |
|---|---|---|---|
| Direct | completed | failed/cancelled | completed+failed+cancelled |
| Workflow | success | failure/killed | success+failure+killed |
| AUTO | success | failure/killed | success+failure+killed |

terminalReason 用于原因分布；terminalReasonCode 只用于排障，不直接作为稳定报表维度。

成功与失败采用以下正式定义：

- `terminal` 指存在合法 `execution.completed`，running/paused/missing-start 非终态不进入成功率分母。
- Direct delivery 成功仅为 `outcome=completed`；`failed` 是执行异常失败；`cancelled` 是用户取消或进程终止。失败率可按产品口径展示 `failed/terminal`，异常终止率展示 `(failed+cancelled)/terminal`，不得把 cancelled 静默并入 failed。
- Workflow/AUTO delivery 成功仅为 `success`；业务失败为 `failure`；`killed` 是用户取消或进程终止。失败率展示 `failure/terminal`，异常终止率展示 `(failure+killed)/terminal`。
- attempt 成功为 turn 的 `completed` 或 node/unit 的 `success`；attempt 失败为 turn 的 `failed` 或 node/unit 的 `failure`；`cancelled` 单列。logical execution 的成功/失败/取消严格继承最大 attemptIndex 的 terminal attempt。
- `terminalReason` 只解释 outcome，不反向改写成功或失败。服务端首先校验 outcome/reason 合法组合，再以 outcome 统计。

### 7.4 产物质量

- Workflow 首轮交付率：`success AND round_count=1` / success run。
- Workflow 平均交付轮次：success run 的 `AVG(round_count)`。
- AUTO acceptance 首次通过率：`unit_kind=acceptance AND acceptance_attempt=1 AND acceptance_passed=1` / 首次 acceptance。
- AUTO 最终通过率：每个 outer-run 是否存在 passed acceptance，再除以进入过 acceptance 的 outer-run。
- AUTO 平均验收次数：先按 executionId 求 `MAX(acceptance_attempt)`，再求平均。
- `first_pass` 不作为可独立信任的客户端结论：服务端校验后按 `passed=1 AND acceptance_attempt=1` 计算和投影。首过率分母为每个 outer-run 的 `acceptanceAttempt=1` 记录；同一 outer-run 只能贡献一次分子和一次分母。

### 7.5 效率成本与模型切换

- Direct：汇总 turn attempt usage。
- Workflow：汇总 node-attempt usage；delivery run 不保存 usage/model/timing，也不读取最后节点数据。
- AUTO：汇总 unit-attempt usage；delivery outer-run 不保存 usage/model/timing。workflow-invocation 没有独立 ACP 调用时 usage 为 NULL，child Workflow node attempts 是其模型成本来源。
- 顶层 usage 和 modelUsages 均保存已知部分；各字段分别按 6.6 的 null 规则求和。跨 attempt 汇总时 SQL `SUM` 忽略 null，同时必须返回该字段的 `known_attempt_count` 与 `unknown_attempt_count`；全部未知时结果为 NULL，禁止 `COALESCE(...,0)` 冒充零消耗。
- `model_usages` 使用 MySQL `JSON_TABLE` 展开 provider/model。
- 最终模型口径使用 `final_provider/final_model`；参与模型口径使用 JSON，二者不得混用。

### 7.6 自动化与交互 Count

| Count | 含义 | 统计用途 |
|---|---|---|
| pauseCount | 非 paused→paused 次数 | 可靠性 |
| resumeCount | paused→running 次数 | 恢复率 |
| permissionRequestCount | 唯一 permission request 次数 | 干预负担 |
| elicitationCount | 唯一 elicitation request 次数 | 干预负担 |
| manualContinueCount | 除 permission/elicitation 外的人工恢复次数 | 干预负担 |
| followUpCount | 同一 Direct task 首轮之后的用户新输入次数 | 用户交互负担；不参与自动化判定 |

无干预和全自动只判断 permissionRequestCount、elicitationCount、manualContinueCount 三个干预 Count；pause/resume 与 followUpCount 不参与自动化判定。

### 7.7 可靠性

- Direct 异常率：`failed+cancelled` / Direct terminal。
- Workflow/AUTO pause 率：`pause_count>0` / terminal delivery。
- 恢复率：有 pause 的 delivery 中 `resume_count>0` 的比例。
- 未恢复率：有 pause 的 delivery 中 `pause_count>resume_count` 的比例。
- node 故障率：Workflow node attempt outcome=failure / terminal node attempts。
- leaf 故障率：AUTO unit attempt outcome=failure / terminal unit attempts。
- `start_event_missing` 是采集质量，不混入产品可靠性。
- logical node/unit 最终故障率按 `ml_metric_logical_execution.final_outcome` 计算；attempt 故障率按每次尝试计算。重试恢复率只在 `attempt_count>1` 且最大 attemptIndex 已 terminal 的 logical execution 中，以最终成功数为分子。

### 7.8 角色与模型质量

- roleName 是执行开始时 resolved profile 名称快照；角色改名不回写历史。
- 角色执行量：按 roleName 的 terminal attempt 数。
- 角色成功率：success / terminal attempt。
- 角色 token/ACP 时间：按 roleName 汇总 attempt usage。
- 模型参与成功率：展开 modelUsages 后，参与该模型的成功 attempts / terminal attempts。
- 一个 attempt 使用多个模型时会进入多个模型参与数，因此各模型之和可大于 attempt 总数。

### 7.9 统一统计输出与采集质量

- 所有比率同时返回 `numerator/denominator/rate`；分母为 0 时 rate 返回 NULL。
- 所有统计均由服务端基于已落库事实和投影计算；客户端 counters、firstPass、Usage 是原始快照/校验输入，不是最终报表值。
- 时间范围统一使用首次归属 `report_date` 的左闭右开区间；模式、kind、terminal 条件必须出现在指标定义或查询参数中。
- 采集质量至少单独输出：`start_event_missing`、`attempt_index_gap`、低 revision 投影冲突和 Usage 未知 attempt 数。
- 同时提供 attempt 与 logical execution 两层的执行量、终态率、成功率、失败率、取消/终止率；名称必须携带粒度，禁止跨粒度比较。
- delivery 成功率、logical execution 最终成功率、attempt 成功率分别计算，任何一层不得用另一层结果代替。

## 8. 分区运维

- 每月 20 日创建未来两个月的四张表分区。
- 分区命名固定 `pYYYYMM`，应用只允许 `^p\d{6}$`。
- `pmax` 仅作保护，正常写入必须命中正式月分区。
- 当前暂不删除历史分区。
- `workspace VARCHAR(255)` 本期保持不变，不纳入此次数据库调整。
- 监控目标分区缺失、pmax 行数和跨月拒绝。

## 9. 测试与验收

### API contract

- 鉴权、1/100/101 条边界、完整 enum、UUID、outcome/reason 组合。
- 第一条 reportedAt 决定分区；跨月整批失败；时间偏差超过 24 小时失败。
- 响应各业务 Count 与实际插入/更新一致。

### 数据库

- 四表只有 `report_date` 一个 DATE 字段，EXPLAIN 分区裁剪生效。
- attempt 表主键粒度为 attemptId；Direct 同一会话 executionId/attemptId 等于 task UUID，attemptIndex 固定为 1，多次输入更新同一 attempt；Workflow/AUTO 的同一 executionId 可对应多个重试 attemptId。
- attemptIndex 从 1 开始且月内 `(executionId, attemptIndex)` 唯一；逻辑投影的 final outcome 始终来自最大 attemptIndex 的 terminal attempt。
- logical execution 投影可由 attempt 表完整重建，attempt 数、最终 attempt、序号缺口标记一致。
- Count 只进入 delivery stat；usage/model 只进入 attempt。
- 当前月未找到时只查前一月，不扫描更早分区。

### 生命周期

- eventId 在目标月及前一个月范围内幂等；正式 revision 主体字段计算正确，同主体/revision 重放直接按 duplicate success 处理。
- revision 允许从大于 1 开始、允许缺口和乱序；started→paused→resumed→completed 时间线完整，terminal 吸收态不被任何非 terminal 事件回退。
- terminal-first、迟到 started、永久 running 符合规则。
- terminal counters 以最新合法 terminal 快照覆盖，非 terminal 事件不修改 Count。

### 指标

- 使用固定小数据集逐项验证 7.1 中全部公式。
- Direct 首轮不重复算内部 Workflow。
- AUTO invocation 与 child Workflow usage 不双计。
- roundIndex、roleName、模型参与/最终模型口径分别验证。
- success/failure/cancelled 或 killed 分列；attempt、logical execution、delivery 三层口径分别验证。
- Usage 全未知、部分未知、空 modelUsages、字段独立求和与 known/unknown attempt count 分别验证。
- firstPass 公式一致性和 acceptance 序号唯一性分别验证；`failedAttemptId` 只验证合法 UUID 能进入 `raw_payload`，不验证关联和投影。

## 10. 待办与暂不处理项

- `failedAttemptId` 的关联、乱序回填、失败归因和统计整体延期；本期仅保留 raw event 字段，不建立 delivery 投影列。
- Direct 同时写 attempt、logical execution、delivery stat 的长期存储与查询简化方案待定；当前继续按三层模型实现，报表不得跨表相加。
- 长期暂停导致 execution 跨越两个以上自然月，不在本期处理。
- eventId 重复但 payload 不同，不在本期处理；同 eventId 或同 revision 主体/revision 命中即按 duplicate success。
- 分区维护 DDL、UUID 紧凑存储、事务锁顺序与死锁重试、raw payload 规范、数据保留与隐私治理、统计查询 API 契约均作为后续优化，不阻塞本期服务端接收与投影实现。
