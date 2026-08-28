# 会话指标批量上报服务端实施合同

> 状态：客户端已实现；本文是服务端仓库的实施输入。本仓库不包含服务端 controller、migration、storage 或 deployment 代码。
>
> 客户端字段与生命周期语义的唯一真源是 `metrics-collection.md`。本文已按当前 wire DTO 校正；服务端不得重新解释 `executionId`、revision、来源、counters 或代码变更口径。

## 1. 目标与边界

服务端实现 `POST /api/client-report/metrics/batch`，逐事件完成校验、幂等判定、不可变 raw 写入以及 attempt/run/task 三层投影。

必须保证：

- `executionId` 是 Task UUID；同一 Task 跨 Run、Round、attempt 和应用重启保持不变。
- `eventRevision` 的唯一作用域是 `(projectId, executionId)`，不能按 run 或 attempt 重置。
- 一个 batch 可部分 accepted、部分 duplicate、部分 rejected；每个输入 eventId 必须且只能出现在一个结果集合。
- raw event 与对应投影在同一短事务内提交；失败不得留下半投影。
- counters 是客户端 terminal snapshot，不由服务端累加 lifecycle event。
- 服务端只返回结构化错误码和参数，不返回对客文案。

本期不做跨产品统一埋点平台、实时流处理、客户端旧合同兼容或源码/diff 持久化。

## 2. 输入合同

### 2.1 HTTP

| 项 | 合同 |
|---|---|
| Method | `POST` |
| Path | `/api/client-report/metrics/batch` |
| 鉴权 | `X-Maling-Report-Key` |
| Content-Type | `application/json` |
| Body | `{ "events": [...] }` |
| 数量 | 1..100 |
| 单事件上限 | 64 KiB UTF-8 JSON |
| 批次月份 | 所有 `reportedAt` 必须属于同一 Asia/Shanghai 自然月 |

`reportedAt` 是 collector 冻结的 Asia/Shanghai 本地时间，格式 `yyyy-MM-ddTHH:mm:ss.SSS`，不带 offset。`occurredAt/scheduledAt` 来自现有 runtime 领域状态，解析器必须明确支持三种 canonical 输入：RFC 3339、同格式的 Asia/Shanghai 本地时间、`<unix-seconds>Z`。三者解析后统一保存为 UTC `DATETIME(3)`；非法或 DST 不可解析值逐项拒绝。服务端不得用接收时间覆盖这些字段。后续统一全仓时间模型时应破坏式收敛 schema，不能静默增加第四种格式。

### 2.2 必填信封

每条事件必须包含：

`eventId/eventRevision/eventType/occurredAt/reportedAt/projectId/userId/workspace/clientVersion/sessionMode/executionKind/executionId/runId/roundId/taskOrigin`。

约束：

- `eventId` 为 UUID，`eventRevision >= 1`。
- `projectId/executionId/runId/roundId` 非空并满足共享长度、字符集合同。
- `eventType` 仅允许 `execution.started`、`execution.completed`、`execution.paused`、`execution.resumed`、`intervention.requested`、`acceptance.completed`。
- `sessionMode` 仅允许 `direct/workflow/auto`。
- `taskOrigin` 是字符串，只允许 `user/scheduled`。`user` 事件必须省略 `executionTrigger`；`scheduled` 事件必须携带符合 2.5 的 `executionTrigger`。
- `taskTitle` 是允许上传的可选字符串，来源是客户端创建该事件时冻结的 Task 标题；没有标题时省略。它不是 Task identity，也不参与幂等、revision 或 locator 校验。
- 顶层未知扩展字段可保存到 raw，但不得参与当前投影；`executionTrigger`、`codeChanges` 等协议对象必须按当前 shape 严格校验并拒绝未知旧字段。未知枚举必须拒绝，不能静默降级。

### 2.3 主体矩阵

| sessionMode | executionKind | 必填 | 禁止 |
|---|---|---|---|
| direct | turn | `attemptId,attemptIndex` | `nodeId,roundIndex,roleName,unitKind` |
| workflow | run | 无 attempt 字段 | `nodeId,attemptId,attemptIndex,roundIndex,roleName,unitKind` |
| workflow | node-attempt | `nodeId,attemptId,attemptIndex,roundIndex,roleName` | `unitKind` |
| auto | outer-run | 无 attempt 字段 | `nodeId,attemptId,attemptIndex,roundIndex,roleName,unitKind` |
| auto | unit-attempt | `nodeId,attemptId,attemptIndex,roundIndex,roleName,unitKind` | 无 |

`execution.paused/execution.resumed/intervention.requested` 必须使用 attempt 主体，Workflow/AUTO 不允许把中间态挂到 run/outer-run。

### 2.4 terminal 与统计字段

- `outcome` 与 `terminalReason` 只允许且必须同时出现在 `execution.completed`。
- attempt terminal：Workflow `node-attempt` 和 AUTO `unit-attempt` 必须带 attempt counters。
- task delivery terminal：Direct `turn`、Workflow `run`、AUTO `outer-run` 必须带 task counters。
- 非 terminal、`acceptance.completed` 均禁止 counters。
- node/unit terminal 的 `followUpCount` 必须为 0。
- `codeChanges` 只允许出现在 task delivery terminal。
- `codeChanges` 只包含 `addedLines/deletedLines/changedFiles`，出现时三个字段必须同时存在且为非负整数；不可用时客户端省略整个对象。拒绝旧 `completeness/limitationCodes` 字段，不接收部分统计。
- Direct 的每个 turn terminal 都是 task delivery terminal；后续 turn 的 `codeChanges` 是同一 Run 启动 workspace tree 到当前 turn terminal tree 的新快照。服务端按更高合法 terminal revision 覆盖 Task 投影，不累加各轮数字。
- `modelUsages` 只保存客户端给出的 attempt usage；服务端不得按 provider/model 再次猜测拆分。

### 2.5 来源联合类型

`taskOrigin` 不是 tagged object，而是扁平字符串：

- `"user"`：必须省略 `executionTrigger`。
- `"scheduled"`：必须携带 `executionTrigger`。

`executionTrigger` 使用 `type` 作为 discriminator，只允许以下三个 shape；所有 shape 都必须带非空 `scheduledTaskId/scheduledOccurrenceId/scheduledAt/timezone`：

```json
{ "type": "once", "scheduledTaskId": "...", "scheduledOccurrenceId": "...", "scheduledAt": "...", "timezone": "Asia/Shanghai" }
{ "type": "cron", "scheduledTaskId": "...", "scheduledOccurrenceId": "...", "scheduledAt": "...", "timezone": "Asia/Shanghai", "expression": "0 0 10 * * MON-FRI" }
{ "type": "repeat", "scheduledTaskId": "...", "scheduledOccurrenceId": "...", "scheduledAt": "...", "timezone": "Asia/Shanghai", "repeatKind": "daily", "hour": 10, "minute": 0 }
```

`repeatKind` 只允许 `interval/hourly/daily/weekdays/weekly`。`interval` 必须携带 `value/unit/anchorAt`；`hourly/daily/weekdays/weekly` 携带 `hour/minute`，其中 `weekly` 还必须携带非空 `weekdays`，其他 preset 不得伪造 interval 字段。拒绝旧 `{kind: ...}`、`triggerKind`、`sessionPolicy` 和 user trigger shape，不提供兼容解析。

## 3. 响应合同

成功处理请求级信封后返回 HTTP 200：

```json
{
  "data": {
    "acceptedEventIds": ["event-a"],
    "duplicateEventIds": ["event-b"],
    "rejected": [{
      "eventId": "event-c",
      "error": {
        "code": "METRICS_REVISION_CONFLICT",
        "params": {"projectId": "p", "executionId": "t", "eventRevision": 7}
      }
    }]
  }
}
```

覆盖不变量：三个集合互斥，数量之和等于请求事件数，不得遗漏、重复或返回请求外 eventId。客户端把 accepted/duplicate 标记 acked，把 rejected 逐项标记 rejected；覆盖不完整会整批重试。

请求级错误仅用于鉴权失败、JSON 不可解析、events 数量越界或跨月 batch。业务字段错误必须逐项 rejected。

## 4. 数据模型

以下 DDL 以 MySQL 8.0 为例。名称可按服务端规范调整，但键、唯一约束和事务边界不能改变。

### 4.1 不可变 raw event

```sql
CREATE TABLE analytics_metric_event_raw (
  event_id             CHAR(36)      NOT NULL,
  payload_sha256       BINARY(32)    NOT NULL,
  project_id           VARCHAR(128)  NOT NULL,
  execution_id         CHAR(36)      NOT NULL,
  event_revision       BIGINT UNSIGNED NOT NULL,
  event_type           VARCHAR(40)   NOT NULL,
  session_mode         VARCHAR(16)   NOT NULL,
  execution_kind       VARCHAR(24)   NOT NULL,
  run_id               VARCHAR(96)   NOT NULL,
  round_id             VARCHAR(96)   NOT NULL,
  occurred_at          DATETIME(3)   NOT NULL,
  reported_at          DATETIME(3)   NOT NULL,
  received_at          DATETIME(3)   NOT NULL,
  payload_json         JSON          NOT NULL,
  projection_version   INT UNSIGNED  NOT NULL,
  PRIMARY KEY (event_id),
  UNIQUE KEY uk_task_revision (project_id, execution_id, event_revision),
  KEY idx_reported_at (reported_at),
  KEY idx_task_timeline (project_id, execution_id, event_revision),
  KEY idx_projection_replay (projection_version, event_id)
) ENGINE=InnoDB;
```

raw 本期不做 MySQL 物理分区，因为分区键会破坏 `event_id` 和 task revision 的全局唯一约束。达到归档阈值后，应先设计独立全局 idempotency registry，再按时间归档；不得为了分区把唯一性降为“本月唯一”。

### 4.2 task stream cursor

```sql
CREATE TABLE analytics_metric_task_cursor (
  project_id            VARCHAR(128) NOT NULL,
  execution_id          CHAR(36)     NOT NULL,
  max_accepted_revision BIGINT UNSIGNED NOT NULL DEFAULT 0,
  first_occurred_at     DATETIME(3)  NULL,
  last_occurred_at      DATETIME(3)  NULL,
  updated_at            DATETIME(3)  NOT NULL,
  PRIMARY KEY (project_id, execution_id)
) ENGINE=InnoDB;
```

cursor 是并发仲裁和诊断记录，不要求事件连续到达。`max_accepted_revision` 只能取最大值；较小 revision 迟到仍可 accepted，只要唯一约束不冲突。

### 4.3 attempt projection

```sql
CREATE TABLE analytics_metric_attempt (
  project_id          VARCHAR(128) NOT NULL,
  execution_id        CHAR(36)     NOT NULL,
  run_id              VARCHAR(96)  NOT NULL,
  round_id            VARCHAR(96)  NOT NULL,
  attempt_id          CHAR(36)     NOT NULL,
  node_id             VARCHAR(128) NOT NULL,
  session_mode        VARCHAR(16)  NOT NULL,
  execution_kind      VARCHAR(24)  NOT NULL,
  attempt_index       INT UNSIGNED NOT NULL,
  round_index         INT UNSIGNED NULL,
  role_name           VARCHAR(256) NULL,
  unit_kind           VARCHAR(32)  NULL,
  state               VARCHAR(24)  NOT NULL,
  outcome             VARCHAR(24)  NULL,
  terminal_reason     VARCHAR(40)  NULL,
  started_at          DATETIME(3)  NULL,
  ended_at            DATETIME(3)  NULL,
  counters_json       JSON         NULL,
  usage_json          JSON         NULL,
  model_usages_json   JSON         NULL,
  last_event_revision BIGINT UNSIGNED NOT NULL,
  updated_at          DATETIME(3)  NOT NULL,
  PRIMARY KEY (project_id, execution_id, run_id, round_id, attempt_id),
  KEY idx_attempt_node (project_id, execution_id, run_id, round_id, node_id, attempt_index),
  KEY idx_attempt_terminal (state, ended_at)
) ENGINE=InnoDB;
```

Direct 的 `node_id` 固定保存内部常量 `direct-turn`；该值不返回客户端，也不与 Workflow/AUTO nodeId 混用。

### 4.4 run delivery projection

```sql
CREATE TABLE analytics_metric_run (
  project_id           VARCHAR(128) NOT NULL,
  execution_id         CHAR(36)     NOT NULL,
  run_id               VARCHAR(96)  NOT NULL,
  session_mode         VARCHAR(16)  NOT NULL,
  execution_kind       VARCHAR(24)  NOT NULL,
  latest_round_id      VARCHAR(96)  NOT NULL,
  state                VARCHAR(24)  NOT NULL,
  outcome              VARCHAR(24)  NULL,
  terminal_reason      VARCHAR(40)  NULL,
  started_at           DATETIME(3)  NULL,
  ended_at             DATETIME(3)  NULL,
  last_event_revision  BIGINT UNSIGNED NOT NULL,
  updated_at           DATETIME(3)  NOT NULL,
  PRIMARY KEY (project_id, execution_id, run_id),
  KEY idx_run_terminal (session_mode, state, ended_at)
) ENGINE=InnoDB;
```

一个 Task 可有多个 run。`run-001` terminal 后不得阻止同一 `executionId` 的 `run-002` 写入。

### 4.5 task aggregate projection

```sql
CREATE TABLE analytics_metric_task (
  project_id             VARCHAR(128) NOT NULL,
  execution_id           CHAR(36)     NOT NULL,
  user_id                VARCHAR(256) NOT NULL,
  workspace              VARCHAR(2048) NOT NULL,
  task_title             VARCHAR(512) NULL,
  task_origin_kind       VARCHAR(32)  NOT NULL,
  scheduled_task_id      VARCHAR(128) NULL,
  latest_run_id          VARCHAR(96)  NOT NULL,
  latest_round_id        VARCHAR(96)  NOT NULL,
  latest_session_mode    VARCHAR(16)  NOT NULL,
  latest_outcome         VARCHAR(24)  NULL,
  latest_terminal_reason VARCHAR(40)  NULL,
  counters_json          JSON         NULL,
  code_changes_json      JSON         NULL,
  last_terminal_revision BIGINT UNSIGNED NOT NULL DEFAULT 0,
  last_event_revision    BIGINT UNSIGNED NOT NULL DEFAULT 0,
  updated_at             DATETIME(3)  NOT NULL,
  PRIMARY KEY (project_id, execution_id),
  KEY idx_task_origin (task_origin_kind, scheduled_task_id),
  KEY idx_task_terminal (latest_session_mode, latest_outcome, updated_at)
) ENGINE=InnoDB;
```

task counters/codeChanges 采用最高合法 task terminal revision 的 snapshot 覆盖，禁止把 attempt counters 或 Direct 各轮 codeChanges 求和后二次写入。`task_title` 保存最高 revision 事件实际携带的最新标题；事件省略 `taskTitle` 时保留已有值，不以 null 清空。raw payload 仍保存每个事件当时的可选标题快照。

## 5. 单事件事务算法

batch 按请求顺序逐事件处理；每条事件一个短事务。这样天然支持部分响应并限制锁范围。没有基准数据前不并行处理同一 batch。

```text
canonicalJson = RFC8785(event) 或服务端统一字段序列化
payloadHash = SHA-256(canonicalJson)

BEGIN
  1. SELECT raw WHERE event_id = ?
     - hash 相同：ROLLBACK，返回 duplicate
     - hash 不同：ROLLBACK，返回 METRICS_EVENT_ID_CONFLICT
  2. 校验字段矩阵、来源、terminal、counters、codeChanges
     - 非法：ROLLBACK，返回对应 rejected
  3. INSERT/SELECT task_cursor FOR UPDATE
  4. INSERT raw
     - uk_task_revision 冲突：ROLLBACK，返回 METRICS_REVISION_CONFLICT
  5. max_accepted_revision = GREATEST(current, eventRevision)
  6. 按 executionKind/eventType revision-gated upsert attempt/run/task
  7. COMMIT，返回 accepted
```

必须先用 `event_id + payloadHash` 判 duplicate，再解释 task revision 冲突。相同 eventId 的重传是 duplicate；相同 task revision 的不同 eventId 是 rejected。

任何数据库异常只影响当前事件；返回 `METRICS_STORAGE_UNAVAILABLE` 前必须回滚。请求处理线程不得吞掉异常后返回 accepted。

## 6. 投影规则

### 6.1 通用 revision gate

每张投影只在 `incoming.eventRevision > row.lastEventRevision` 时覆盖当前状态。较旧事件仍保留在 raw，但不得把投影倒退。

terminal-first 和乱序合法：不存在投影行时可直接插入 terminal 行，`startedAt` 允许 null。后续较小 revision 的 started 只保留 raw，不覆盖 terminal；重放会按 revision 升序重建完整投影。

### 6.2 attempt

- `execution.started`：state=`running`，保存 attempt identity、模型和 startedAt。
- paused/resumed/intervention：只更新 last revision；历史以 raw 为准，不覆盖 terminal outcome。
- `execution.completed`：state=`terminal`，保存 outcome、terminal reason、timing、usage/modelUsages、attempt counters。
- `acceptance.completed`：更新 acceptance 质量字段；不得产生 task counters。

### 6.3 run

- 只消费 Direct `turn`、Workflow `run`、AUTO `outer-run`。
- started 创建/推进当前 run；completed 写交付结果。
- attempt 事件不得直接 terminalize run。
- child Workflow 的 `childRunId` 只作为关系字段，不改变父 Task identity。

### 6.4 task

- 每个 accepted event 更新 `last_event_revision=max(...)`、latest locator 和稳定来源字段；更高 revision 的事件携带 `taskTitle` 时更新展示标题，缺省不清空。
- 只有 task delivery `execution.completed` 且 revision 更大时覆盖 terminal outcome、task counters 和 codeChanges。
- follow-up 后的新 run/turn 可以继续推进同一 task；旧 terminal 不是 task stream 的吸收态。
- `taskOrigin` 第一次写入后不可变；scheduled 的 `scheduledTaskId` 第一次写入后不可变，后续不一致事件 rejected 为 `METRICS_IMMUTABLE_FIELD_CONFLICT`。occurrence、scheduledAt 和具体 schedule shape 是逐次 execution trigger 快照，不得错误提升为 Task 不可变字段。

## 7. 错误码

| code | 条件 | 是否重试 |
|---|---|---|
| `METRICS_EVENT_INVALID` | 字段缺失、格式或枚举非法 | 否 |
| `METRICS_SUBJECT_INVALID` | mode/kind/主体矩阵不匹配 | 否 |
| `METRICS_TERMINAL_FIELDS_INVALID` | terminal、counters、codeChanges 作用域错误 | 否 |
| `METRICS_SCHEDULED_PROVENANCE_INVALID` | origin/trigger 缺失、shape 或 repeat 字段组合不合法 | 否 |
| `METRICS_EVENT_ID_CONFLICT` | 同 eventId 不同 payload | 否 |
| `METRICS_REVISION_CONFLICT` | 同 task/revision 不同 eventId | 否 |
| `METRICS_IMMUTABLE_FIELD_CONFLICT` | task 稳定字段发生变化 | 否 |
| `METRICS_STORAGE_UNAVAILABLE` | 当前事件事务未提交 | 是 |

`params` 只包含定位所需 identity/revision/field，不包含 workspace、prompt、模型输出、源码或完整 payload。

## 8. 重放与修复

raw 是事实源，投影是可删除重建的数据。

1. 新 projection 逻辑使用递增 `PROJECTION_VERSION`。
2. replay worker 按 `(project_id, execution_id, event_revision)` 升序读取 raw。
3. 每个 Task 在独立事务中重建到 shadow tables，完成后原子切换或按 Task 替换。
4. replay checkpoint 使用 `(projectionVersion, projectId, executionId)`，失败可继续。
5. 重放不得重新调用接收校验、修改 raw 或产生新的 eventId。
6. shadow 与在线投影按 task 数、terminal 数、counter/codeChanges hash 对账后才允许切换。

## 9. 迁移与发布

当前处于开发阶段，采用破坏式替换，不双写旧表、不为旧字段增加 fallback。

1. 服务端先合入 DTO/schema、上述五张表、事务处理与 contract tests，endpoint 暂不开放。
2. 部署 migration；验证唯一约束、索引和回滚脚本。
3. 在测试环境用客户端真实 100 条 batch 验证 accepted/duplicate/rejected 精确覆盖。
4. 开放 endpoint，再发布已实现本合同的客户端。
5. 观察 raw 写入、冲突率、p95、事务回滚和客户端 pending/rejected。
6. 确认没有旧客户端流量后删除旧 controller、旧宽表消费和旧 revision 逻辑。

回滚服务端应用时保留 raw 和新表，不执行破坏性 down migration。若 endpoint 不可用，客户端 outbox 继续重试；不得把未确认事件标记成功。

## 10. 测试与验收

### Contract

- 六类 eventType、五类 executionKind、三种 sessionMode 全矩阵。
- 缺失/多余字段、未知枚举、user 携带 trigger、scheduled 缺 trigger、旧 `{kind: ...}` shape 与非法 repeat 字段组合逐项拒绝。
- `taskTitle` 缺省/出现及更高 revision 更新投影；标题不参与 identity 与 immutable-field 冲突。
- 三整数 codeChanges 与 counters scope 校验；旧 completeness/limitationCodes 和部分数字逐项拒绝。
- 响应三集合精确覆盖，错误只含 code/params。

### 幂等与并发

- 同 eventId 同 payload 并发提交：一个 accepted，其余 duplicate。
- 同 task/revision 不同 eventId 并发提交：一个 accepted，其余 revision conflict。
- 两个 Project 使用相同本地 run/round 不串投影。
- 同一 Task 跨多个 Run/Round 的 revision 与 task projection 连续推进。

### 事务与乱序

- 在 raw insert、cursor update、各投影 upsert 处故障注入，确认无半提交。
- terminal-first、revision 缺口、旧 revision 迟到不倒退投影。
- 单批混合 accepted/duplicate/rejected，合法事件正常提交。

### 重放

- 删除投影后从 raw 重建，attempt/run/task 行与在线结果一致。
- projectionVersion 升级可断点续跑并完成 hash 对账。

### 性能与安全

- 100 条 batch 在生产配置下记录 p50/p95/p99、事务时间和锁等待；先以 p95 < 500 ms、单事件事务 p95 < 20 ms 为上线门槛，按真实基线调整。
- 查询计划命中本文索引，无 raw 全表扫描、N+1 查询或长事务。
- 除协议明确允许的 `taskTitle` 外，payload、日志和错误中不出现 prompt、回复、附件、源码、diff 或 logical path；错误 params 不得回显标题。
- 限流按 report key 与 client identity 执行，不能把合法部分响应变为未覆盖结果。

## 11. 方案评审

过度设计：仅保留不可变 raw、task cursor 和三个实际查询粒度投影；不引入消息队列、流处理、分布式锁或假设性索引。revision 唯一约束和 task cursor 是并发幂等所必需，投影可由 raw 重建。

性能：单事件短事务最多处理 100 次，换取严格部分成功和最小锁范围；先用压测数据判断是否需要按 Task 分组事务，禁止未经测量改为并行 writer。raw 不分区避免牺牲全局唯一性，保留 `reported_at` 索引和可验证归档入口。

正确性：服务端不重新累计客户端 snapshot，不把 run/Direct turn terminal 当 task stream 终止，不按 attempt 分配 revision，也不通过 `taskTitle` 或其他名称反查 canonical identity。来源解析只接受当前 `taskOrigin` 字符串与 `executionTrigger.type` 合同。
